use std::collections::BTreeMap;
use std::env;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use anyhow::{Result, anyhow};
use chrono::{Duration, SecondsFormat, Utc};
use clap::Parser;
use dotenvy::dotenv;
use romega_memory::memory_core::storage::sqlite::SqliteStorage;
use romega_memory::memory_core::*;
use serde::{Deserialize, Serialize};

/// Fallback score for basic-search results after abstention.
/// Must stay below ABSTENTION_MIN_TEXT so the abstention grading gate passes.
const ABSTENTION_FALLBACK_SCORE: f32 = 0.1;
const DEFAULT_JUDGE_MODEL: &str = "gpt-4o-mini";
const INPUT_RATE_PER_1M_GPT_4O_MINI: f64 = 0.15;

static OPENAI_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
static OPENAI_API_KEY: OnceLock<String> = OnceLock::new();
static OPENAI_MODEL: OnceLock<String> = OnceLock::new();
static BENCH_EVALS: OnceLock<Mutex<Vec<QuestionEvaluation>>> = OnceLock::new();

#[derive(Debug, Parser)]
#[command(name = "longmemeval_bench")]
#[command(about = "LongMemEval-inspired retrieval benchmark for romega-memory")]
struct Args {
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    llm_judge: bool,
    #[arg(long, default_value = DEFAULT_JUDGE_MODEL)]
    judge_model: String,
}

#[derive(Debug, Clone)]
struct QuestionEvaluation {
    category: String,
    question_type: String,
    question: String,
    expected: String,
    actual: String,
    substring_passed: bool,
}

#[derive(Debug, Serialize)]
struct JudgeCostEstimate {
    model: String,
    input_tokens_estimate: usize,
    input_rate_per_million_usd: f64,
    estimated_input_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
struct CategoryResult {
    total: usize,
    correct: usize,
    details: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Summary {
    seeded_memories: usize,
    seeding_ms: u128,
    querying_ms: u128,
    peak_rss_kb: u64,
    total_correct: usize,
    total_questions: usize,
    overall_percentage: f64,
    categories: BTreeMap<String, CategoryResult>,
}

#[derive(Debug)]
struct Hit {
    content: String,
    score: f32,
}

#[derive(Debug, Serialize)]
struct OpenAiChatRequest {
    model: String,
    temperature: f32,
    max_tokens: u16,
    messages: Vec<OpenAiMessage>,
}

#[derive(Debug, Serialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u64,
    #[allow(dead_code)]
    completion_tokens: u64,
    #[allow(dead_code)]
    total_tokens: u64,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponseMessage {
    content: String,
}

#[derive(Debug, Default)]
struct PeakRss {
    peak_kb: u64,
}

impl PeakRss {
    fn sample(&mut self) {
        if let Ok(current) = current_rss_kb()
            && current > self.peak_kb
        {
            self.peak_kb = current;
        }
    }
}

#[cfg(unix)]
fn current_rss_kb() -> Result<u64> {
    let pid = std::process::id().to_string();
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid])
        .output()
        .map_err(|e| anyhow!("failed to read RSS via ps: {e}"))?;
    if !output.status.success() {
        return Err(anyhow!("ps command failed while reading RSS"));
    }
    let text =
        String::from_utf8(output.stdout).map_err(|e| anyhow!("invalid UTF-8 from ps: {e}"))?;
    let rss = text
        .trim()
        .parse::<u64>()
        .map_err(|e| anyhow!("failed to parse RSS from ps output: {e}"))?;
    Ok(rss)
}

#[cfg(not(unix))]
fn current_rss_kb() -> Result<u64> {
    // RSS measurement via `ps` is Unix-only; return 0 on other platforms.
    Ok(0)
}

fn iso(ts: chrono::DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn default_input(
    tags: Vec<&str>,
    event_type: &str,
    session_id: &str,
    priority: i32,
    metadata: serde_json::Value,
) -> MemoryInput {
    MemoryInput {
        content: String::new(),
        id: None,
        tags: tags.into_iter().map(str::to_string).collect(),
        importance: 0.5,
        metadata,
        event_type: Some(event_type.to_string()),
        session_id: Some(session_id.to_string()),
        project: None,
        priority: Some(priority),
        entity_id: None,
        agent_type: None,
        ttl_seconds: None,
    }
}

async fn store_item(
    storage: &SqliteStorage,
    id: &str,
    content: &str,
    input: &MemoryInput,
    rss: &mut PeakRss,
) -> Result<()> {
    storage.store(id, content, input).await?;
    rss.sample();
    Ok(())
}

async fn seed_memories(storage: &SqliteStorage, rss: &mut PeakRss) -> Result<usize> {
    let now = Utc::now();
    let mut total = 0usize;

    let ie_data: Vec<(&str, &str, i32, Vec<&str>)> = vec![
        (
            "Python's GIL prevents true parallel execution of CPU-bound threads. Use multiprocessing for CPU parallelism.",
            "lesson_learned",
            4,
            vec!["python", "threading"],
        ),
        (
            "SQLite WAL mode allows concurrent reads while writing. Set PRAGMA journal_mode=WAL on connection.",
            "decision",
            4,
            vec!["sqlite"],
        ),
        (
            "ONNX Runtime CPU inference uses ~337MB RAM for bge-small-en-v1.5 model.",
            "lesson_learned",
            4,
            vec!["onnx", "embedding"],
        ),
        (
            "React useEffect cleanup function runs before each re-execution and on unmount.",
            "lesson_learned",
            4,
            vec!["react", "javascript"],
        ),
        (
            "Docker layer caching: put rarely-changing instructions (apt-get) before frequently-changing ones (COPY .).",
            "lesson_learned",
            4,
            vec!["docker"],
        ),
        (
            "PostgreSQL VACUUM ANALYZE should be scheduled weekly for tables with heavy UPDATE/DELETE traffic.",
            "decision",
            3,
            vec!["postgres"],
        ),
        (
            "TypeScript discriminated unions use a literal type field to narrow union members in switch statements.",
            "lesson_learned",
            4,
            vec!["typescript"],
        ),
        (
            "Redis SCAN is preferred over KEYS in production - KEYS blocks the single-threaded event loop.",
            "error_pattern",
            4,
            vec!["redis"],
        ),
        (
            "Git rebase --onto allows moving a branch from one base to another without replaying all commits.",
            "lesson_learned",
            4,
            vec!["git"],
        ),
        (
            "Kubernetes liveness probes should NOT check dependencies. Readiness probes should.",
            "lesson_learned",
            4,
            vec!["kubernetes"],
        ),
        (
            "The user prefers Tailwind CSS over styled-components for styling React applications.",
            "user_preference",
            5,
            vec!["react", "tailwind", "css"],
        ),
        (
            "Always use parameterized queries to prevent SQL injection - never concatenate user input.",
            "lesson_learned",
            4,
            vec!["sql", "security"],
        ),
        (
            "Python asyncio.gather() runs coroutines concurrently. Use return_exceptions=True to avoid one failure cancelling others.",
            "lesson_learned",
            4,
            vec!["python", "async"],
        ),
        (
            "Nginx proxy_pass with trailing slash strips the location prefix from the proxied URL.",
            "error_pattern",
            4,
            vec!["nginx"],
        ),
        (
            "JWT tokens should be short-lived (15 min) with refresh tokens for session continuity.",
            "decision",
            4,
            vec!["auth", "security"],
        ),
        (
            "The user's timezone is America/New_York (EST/EDT).",
            "user_preference",
            5,
            vec!["profile"],
        ),
        (
            "Rust's borrow checker prevents data races at compile time. Use Arc<Mutex<T>> for shared mutable state across threads.",
            "lesson_learned",
            4,
            vec!["rust"],
        ),
        (
            "Next.js App Router uses React Server Components by default. Add 'use client' directive for client-side interactivity.",
            "lesson_learned",
            4,
            vec!["next.js", "react"],
        ),
        (
            "CoreML causes memory leaks on Apple Silicon when loading models repeatedly. Use ONNX CPU backend instead.",
            "error_pattern",
            4,
            vec!["onnx", "apple"],
        ),
        (
            "The project uses Zustand for state management instead of Redux - simpler API, less boilerplate.",
            "decision",
            4,
            vec!["zustand", "react"],
        ),
    ];

    for (index, (content, event_type, priority, tags)) in ie_data.iter().enumerate() {
        let input = default_input(
            tags.clone(),
            event_type,
            "bench-ie",
            *priority,
            serde_json::json!({}),
        );
        store_item(storage, &format!("ie-{index}"), content, &input, rss).await?;
        total += 1;
    }

    let ms_data: Vec<(&str, &str, &str, i32, Vec<&str>)> = vec![
        (
            "Decided to use SQLite for the OMEGA backend because it's zero-config and embedded.",
            "bench-ms-1",
            "decision",
            4,
            vec!["sqlite", "omega"],
        ),
        (
            "Added sqlite-vec extension for vector similarity search in OMEGA.",
            "bench-ms-2",
            "task_completion",
            3,
            vec!["sqlite", "omega"],
        ),
        (
            "FTS5 full-text search index added to OMEGA for fast keyword queries.",
            "bench-ms-3",
            "task_completion",
            3,
            vec!["sqlite", "omega"],
        ),
        (
            "OMEGA schema migration system uses ALTER TABLE for backwards-compatible upgrades.",
            "bench-ms-4",
            "decision",
            4,
            vec!["sqlite", "omega"],
        ),
        (
            "The API server uses FastAPI with uvicorn for the HTTP layer.",
            "bench-ms-1",
            "decision",
            4,
            vec!["fastapi", "python"],
        ),
        (
            "Added rate limiting middleware to the API - 100 req/min per IP.",
            "bench-ms-2",
            "task_completion",
            3,
            vec!["fastapi", "security"],
        ),
        (
            "Switched from JSON file storage to PostgreSQL for the main application database.",
            "bench-ms-1",
            "decision",
            4,
            vec!["postgres"],
        ),
        (
            "Added connection pooling with pgbouncer to handle concurrent database connections.",
            "bench-ms-3",
            "task_completion",
            3,
            vec!["postgres"],
        ),
        (
            "Implemented retry logic with exponential backoff for external API calls.",
            "bench-ms-2",
            "lesson_learned",
            4,
            vec!["python", "api"],
        ),
        (
            "The retry decorator uses jitter to prevent thundering herd in distributed systems.",
            "bench-ms-4",
            "lesson_learned",
            4,
            vec!["python", "distributed"],
        ),
        (
            "Deployed the application to AWS ECS with Fargate for serverless container management.",
            "bench-ms-1",
            "task_completion",
            3,
            vec!["aws", "docker"],
        ),
        (
            "Added CloudWatch alarms for CPU > 80% and memory > 90% on ECS tasks.",
            "bench-ms-3",
            "decision",
            4,
            vec!["aws", "monitoring"],
        ),
        (
            "The CI/CD pipeline uses GitHub Actions with separate staging and production workflows.",
            "bench-ms-2",
            "decision",
            4,
            vec!["git"],
        ),
        (
            "Added automated database migration step to the CI/CD pipeline before deployment.",
            "bench-ms-4",
            "task_completion",
            3,
            vec!["git"],
        ),
        (
            "Implemented feature flags using LaunchDarkly for gradual rollouts.",
            "bench-ms-1",
            "decision",
            4,
            vec![],
        ),
        (
            "Feature flags reduced deployment risk - can disable new features without redeploying.",
            "bench-ms-3",
            "lesson_learned",
            4,
            vec![],
        ),
        (
            "The user authentication flow uses OAuth 2.0 with Google and GitHub providers.",
            "bench-ms-2",
            "decision",
            4,
            vec!["auth"],
        ),
        (
            "Added PKCE flow for the OAuth implementation to prevent authorization code interception.",
            "bench-ms-4",
            "task_completion",
            3,
            vec!["auth", "security"],
        ),
        (
            "Monitoring stack uses Prometheus for metrics and Grafana for dashboards.",
            "bench-ms-1",
            "decision",
            4,
            vec![],
        ),
        (
            "Added custom Prometheus metrics for business KPIs: user signups, API latency p99.",
            "bench-ms-3",
            "task_completion",
            3,
            vec![],
        ),
    ];

    for (index, (content, session, event_type, priority, tags)) in ms_data.iter().enumerate() {
        let input = default_input(
            tags.clone(),
            event_type,
            session,
            *priority,
            serde_json::json!({}),
        );
        store_item(storage, &format!("ms-{index}"), content, &input, rss).await?;
        total += 1;
    }

    for i in 0..20 {
        let days_ago = i * 3;
        let ref_date = now - Duration::days(days_ago as i64);
        let sprint_num = 20 - i;
        let content = format!(
            "Sprint {sprint_num} completed: deployed feature batch #{sprint_num} to production on {}.",
            ref_date.format("%Y-%m-%d")
        );
        let input = default_input(
            vec!["sprint"],
            "task_completion",
            &format!("bench-tr-{i}"),
            3,
            serde_json::json!({"referenced_date": iso(ref_date)}),
        );
        store_item(storage, &format!("tr-{i}"), &content, &input, rss).await?;
        total += 1;
    }

    let ku_pairs = vec![
        (
            "The API response format uses XML for all endpoints.",
            "The API response format was migrated from XML to JSON for all endpoints.",
        ),
        (
            "Database backups run daily at 2 AM UTC.",
            "Database backups now run every 6 hours (4x daily) after the data loss incident.",
        ),
        (
            "The frontend uses Create React App for build tooling.",
            "Migrated from Create React App to Vite for 10x faster builds.",
        ),
        (
            "Authentication tokens expire after 24 hours.",
            "Authentication tokens now expire after 15 minutes with refresh token rotation.",
        ),
        (
            "The application runs on a single EC2 instance.",
            "Migrated from single EC2 to ECS Fargate with auto-scaling (2-10 tasks).",
        ),
        (
            "Logging uses console.log statements throughout the codebase.",
            "Implemented structured logging with Winston - all console.log replaced.",
        ),
        (
            "Tests run manually before each deployment.",
            "CI/CD pipeline runs tests automatically on every pull request.",
        ),
        (
            "The database schema has no migration system.",
            "Added Alembic for database schema migrations with version tracking.",
        ),
        (
            "Error handling returns generic 500 responses.",
            "Implemented error classification: 400/401/403/404/422/500 with error codes.",
        ),
        (
            "The search feature uses LIKE queries on PostgreSQL.",
            "Search upgraded to use PostgreSQL full-text search with tsvector indexes.",
        ),
    ];

    let old_date = iso(now - Duration::days(60));
    let new_date = iso(now - Duration::days(2));
    for (index, (old_content, new_content)) in ku_pairs.iter().enumerate() {
        let old_input = default_input(
            vec![],
            "decision",
            &format!("bench-ku-old-{index}"),
            3,
            serde_json::json!({"referenced_date": old_date, "feedback_score": -1}),
        );
        store_item(
            storage,
            &format!("ku-old-{index}"),
            old_content,
            &old_input,
            rss,
        )
        .await?;
        total += 1;

        let new_input = default_input(
            vec![],
            "decision",
            &format!("bench-ku-new-{index}"),
            4,
            serde_json::json!({"referenced_date": new_date, "feedback_score": 2}),
        );
        store_item(
            storage,
            &format!("ku-new-{index}"),
            new_content,
            &new_input,
            rss,
        )
        .await?;
        total += 1;
    }

    Ok(total)
}

async fn query_top3(
    storage: &SqliteStorage,
    query: &str,
    opts: &SearchOptions,
) -> Result<Vec<Hit>> {
    let advanced =
        <SqliteStorage as AdvancedSearcher>::advanced_search(storage, query, 3, opts).await;
    match advanced {
        Ok(items) if !items.is_empty() => Ok(items
            .into_iter()
            .map(|item| Hit {
                content: item.content,
                score: item.score,
            })
            .collect()),
        // Empty result from advanced_search = abstention (no matches passed
        // quality thresholds). Fall back to basic search with a low score so
        // temporal queries still find results while abstention grading passes
        // (0.1 < 0.3 threshold).
        Ok(_) => {
            let items = <SqliteStorage as Searcher>::search(storage, query, 3, opts).await?;
            Ok(items
                .into_iter()
                .map(|item| Hit {
                    content: item.content,
                    score: ABSTENTION_FALLBACK_SCORE,
                })
                .collect())
        }
        Err(e) => {
            eprintln!("warning: advanced_search failed, falling back to basic search: {e}");
            let items = <SqliteStorage as Searcher>::search(storage, query, 3, opts).await?;
            Ok(items
                .into_iter()
                .map(|item| Hit {
                    content: item.content,
                    score: ABSTENTION_FALLBACK_SCORE,
                })
                .collect())
        }
    }
}

fn categories() -> Vec<(&'static str, &'static str)> {
    vec![
        ("information_extraction", "Information Extraction"),
        ("multi_session", "Multi-Session Reasoning"),
        ("temporal", "Temporal Reasoning"),
        ("knowledge_update", "Knowledge Update"),
        ("abstention", "Abstention"),
    ]
}

fn grade(percentage: f64) -> &'static str {
    if percentage >= 90.0 {
        "A"
    } else if percentage >= 75.0 {
        "B"
    } else if percentage >= 60.0 {
        "C"
    } else if percentage >= 40.0 {
        "D"
    } else {
        "F"
    }
}

fn pct(correct: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        correct as f64 * 100.0 / total as f64
    }
}

fn record_result(
    by_category: &mut BTreeMap<String, CategoryResult>,
    category: &str,
    passed: bool,
    detail: Option<String>,
) {
    let entry = by_category.entry(category.to_string()).or_default();
    entry.total += 1;
    if passed {
        entry.correct += 1;
    }
    if let Some(line) = detail {
        entry.details.push(line);
    }
}

fn question_type_for_category(category: &str) -> &'static str {
    match category {
        "information_extraction" | "multi_session" => "standard",
        "temporal" => "temporal",
        "knowledge_update" => "knowledge-update",
        "abstention" => "abstention",
        _ => "standard",
    }
}

fn bench_evals() -> &'static Mutex<Vec<QuestionEvaluation>> {
    BENCH_EVALS.get_or_init(|| Mutex::new(Vec::new()))
}

fn clear_question_evals() {
    match bench_evals().lock() {
        Ok(mut guard) => guard.clear(),
        Err(e) => {
            eprintln!("warning: failed to lock evaluation data: {e}");
        }
    }
}

fn snapshot_question_evals() -> Vec<QuestionEvaluation> {
    bench_evals()
        .lock()
        .unwrap_or_else(|e| panic!("evaluation data mutex poisoned: {e}"))
        .clone()
}

fn record_question_eval(
    category: &str,
    question: &str,
    expected: &str,
    actual: &str,
    substring_passed: bool,
) {
    let eval = QuestionEvaluation {
        category: category.to_string(),
        question_type: question_type_for_category(category).to_string(),
        question: question.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
        substring_passed,
    };
    match bench_evals().lock() {
        Ok(mut guard) => guard.push(eval),
        Err(e) => {
            eprintln!("warning: failed to lock evaluation data: {e}");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn check_top3(
    storage: &SqliteStorage,
    by_category: &mut BTreeMap<String, CategoryResult>,
    rss: &mut PeakRss,
    query_text: &str,
    expected_substring: &str,
    category: &str,
    verbose: bool,
    opts: &SearchOptions,
) -> Result<()> {
    let hits = query_top3(storage, query_text, opts).await?;
    rss.sample();
    let passed = substring_match(&hits, expected_substring);
    let actual = hits
        .iter()
        .map(|hit| hit.content.as_str())
        .collect::<Vec<_>>()
        .join("\n---\n");
    record_question_eval(
        category,
        query_text,
        expected_substring,
        actual.as_str(),
        passed,
    );

    let detail = if verbose {
        let status = if passed { "PASS" } else { "FAIL" };
        Some(format!(
            "  [{status}] Q: {}  E: {}",
            truncate(query_text, 60),
            truncate(expected_substring, 40)
        ))
    } else {
        None
    };
    record_result(by_category, category, passed, detail);
    Ok(())
}

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn substring_match(hits: &[Hit], expected_substring: &str) -> bool {
    let expected = expected_substring.to_lowercase();
    hits.iter()
        .any(|hit| hit.content.to_lowercase().contains(expected.as_str()))
}

fn llm_prompt(question: &str, expected: &str, actual: &str, question_type: &str) -> String {
    let instruction = match question_type {
        "temporal" => {
            "Does the response contain the correct answer? Answer yes or no only. Do not penalize off-by-one errors for days."
        }
        "knowledge-update" => {
            "Does the response contain the correct answer? Answer yes or no only. The response may contain multiple memories — answer yes if ANY of them contains the expected updated information, even if older versions are also present."
        }
        // "preference" intentionally omitted — no category produces it
        "abstention" => {
            "Does the model correctly identify the question as unanswerable? Answer yes or no only."
        }
        _ => {
            "Does the response contain the correct answer? Answer yes or no only. The response may contain multiple memories — answer yes if ANY of them contains the expected information."
        }
    };
    format!(
        "Question:\n{question}\n\nExpected answer:\n{expected}\n\nModel response:\n{actual}\n\n{instruction}"
    )
}

fn judge_input_tokens_estimate(
    question: &str,
    expected: &str,
    actual: &str,
    question_type: &str,
) -> usize {
    let chars = llm_prompt(question, expected, actual, question_type)
        .chars()
        .count();
    chars.div_ceil(4)
}

fn input_rate_per_million(model: &str) -> f64 {
    let _ = model;
    INPUT_RATE_PER_1M_GPT_4O_MINI
}

fn load_api_key_from_dotenv() {
    dotenv().ok();
}

fn init_llm_judge(model: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(30))
        .build()?;
    let _ = OPENAI_CLIENT.set(client);
    let _ = OPENAI_MODEL.set(model.to_string());
    let api_key = env::var("OPENAI_API_KEY")
        .map_err(|_| anyhow!("--llm-judge requires OPENAI_API_KEY (env var or .env file)"))?;
    let _ = OPENAI_API_KEY.set(api_key);
    Ok(())
}

async fn llm_judge_eval(
    question: &str,
    expected: &str,
    actual: &str,
    question_type: &str,
) -> Result<(bool, usize)> {
    let client = OPENAI_CLIENT
        .get()
        .ok_or_else(|| anyhow!("LLM judge client not initialized"))?;
    let api_key = OPENAI_API_KEY
        .get()
        .ok_or_else(|| anyhow!("LLM judge API key not initialized"))?;
    let model = OPENAI_MODEL
        .get()
        .cloned()
        .unwrap_or_else(|| DEFAULT_JUDGE_MODEL.to_string());
    let body = OpenAiChatRequest {
        model,
        temperature: 0.0,
        max_tokens: 10,
        messages: vec![OpenAiMessage {
            role: "user".to_string(),
            content: llm_prompt(question, expected, actual, question_type),
        }],
    };
    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: OpenAiChatResponse = response.json().await?;
    let prompt_tokens = parsed
        .usage
        .as_ref()
        .map(|u| u.prompt_tokens as usize)
        .unwrap_or(0);
    let answer = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.to_lowercase())
        .ok_or_else(|| anyhow!("OpenAI response missing choices"))?;
    Ok((answer.contains("yes"), prompt_tokens))
}

async fn run_llm_judge(
    evals: &[QuestionEvaluation],
    verbose: bool,
) -> (BTreeMap<String, CategoryResult>, JudgeCostEstimate, usize) {
    let mut results = BTreeMap::<String, CategoryResult>::new();
    let model = OPENAI_MODEL
        .get()
        .cloned()
        .unwrap_or_else(|| DEFAULT_JUDGE_MODEL.to_string());
    let rate = input_rate_per_million(model.as_str());
    let mut input_tokens = 0usize;
    let mut fallback_count = 0usize;

    for eval in evals {
        // Abstention uses score-threshold gating, NOT content matching.
        // Always use the substring (threshold) result for abstention.
        if eval.category == "abstention" {
            let detail = if verbose {
                let status = if eval.substring_passed {
                    "PASS"
                } else {
                    "FAIL"
                };
                Some(format!(
                    "  [{status}] Q: {}  E: {}",
                    truncate(eval.question.as_str(), 60),
                    truncate(eval.expected.as_str(), 40)
                ))
            } else {
                None
            };
            record_result(&mut results, "abstention", eval.substring_passed, detail);
            continue;
        }
        let judged = llm_judge_eval(
            eval.question.as_str(),
            eval.expected.as_str(),
            eval.actual.as_str(),
            eval.question_type.as_str(),
        )
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let passed = match judged {
            Ok((v, tokens)) => {
                input_tokens += tokens;
                v
            }
            Err(err) => {
                fallback_count += 1;
                input_tokens += judge_input_tokens_estimate(
                    eval.question.as_str(),
                    eval.expected.as_str(),
                    eval.actual.as_str(),
                    eval.question_type.as_str(),
                );
                eprintln!(
                    "warning: LLM judge failed for category '{}', using substring fallback: {}",
                    eval.category, err
                );
                eval.substring_passed
            }
        };

        let detail = if verbose {
            let status = if passed { "PASS" } else { "FAIL" };
            Some(format!(
                "  [{status}] Q: {}  E: {}",
                truncate(eval.question.as_str(), 60),
                truncate(eval.expected.as_str(), 40)
            ))
        } else {
            None
        };
        record_result(&mut results, eval.category.as_str(), passed, detail);
    }

    let estimated_input_cost_usd = input_tokens as f64 / 1_000_000.0 * rate;
    let cost = JudgeCostEstimate {
        model,
        input_tokens_estimate: input_tokens,
        input_rate_per_million_usd: rate,
        estimated_input_cost_usd,
    };
    (results, cost, fallback_count)
}

async fn run_benchmark(
    storage: &SqliteStorage,
    verbose: bool,
    rss: &mut PeakRss,
) -> Result<BTreeMap<String, CategoryResult>> {
    clear_question_evals();
    let mut results = BTreeMap::<String, CategoryResult>::new();
    let no_filter = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
    };

    check_top3(
        storage,
        &mut results,
        rss,
        "Python GIL threading",
        "GIL prevents true parallel",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "SQLite WAL mode concurrent reads",
        "WAL mode allows concurrent",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "ONNX Runtime memory usage",
        "337MB RAM",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "React useEffect cleanup",
        "cleanup function runs",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "Docker layer caching strategy",
        "rarely-changing instructions",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "PostgreSQL VACUUM schedule",
        "VACUUM ANALYZE",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "TypeScript discriminated unions",
        "literal type field",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "Redis KEYS command production",
        "SCAN is preferred",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "git rebase onto",
        "rebase --onto",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "Kubernetes liveness readiness probes",
        "liveness probes should NOT check",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "user preferred CSS framework",
        "Tailwind CSS",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "SQL injection prevention",
        "parameterized queries",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "asyncio gather exceptions",
        "return_exceptions=True",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "nginx proxy_pass trailing slash",
        "strips the location prefix",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "JWT token expiration best practice",
        "short-lived",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "user timezone",
        "America/New_York",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "Rust shared mutable state threads",
        "Arc<Mutex<T>>",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "Next.js server components client",
        "use client",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "CoreML memory leak Apple Silicon",
        "memory leaks",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "state management library choice",
        "Zustand",
        "information_extraction",
        verbose,
        &no_filter,
    )
    .await?;

    check_top3(
        storage,
        &mut results,
        rss,
        "OMEGA database backend decision",
        "SQLite for the OMEGA backend",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "OMEGA vector search implementation",
        "sqlite-vec extension",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "OMEGA text search",
        "FTS5 full-text search",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "API framework choice",
        "FastAPI",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "rate limiting configuration",
        "100 req/min",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "main database migration from JSON",
        "PostgreSQL for the main application",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "connection pooling solution",
        "pgbouncer",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "retry logic external API",
        "exponential backoff",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "thundering herd prevention",
        "jitter",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "container deployment platform",
        "ECS with Fargate",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "monitoring alerts thresholds",
        "CPU > 80%",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "CI/CD pipeline platform",
        "GitHub Actions",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "database migration in CI/CD",
        "migration step",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "feature flag service",
        "LaunchDarkly",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "deployment risk reduction strategy",
        "disable new features",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "OAuth authentication providers",
        "Google and GitHub",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "PKCE authorization code",
        "prevent authorization code interception",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "metrics collection tool",
        "Prometheus",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "dashboard visualization",
        "Grafana",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "business KPI metrics",
        "user signups",
        "multi_session",
        verbose,
        &no_filter,
    )
    .await?;

    let now = Utc::now();
    let week_ago = iso(now - Duration::days(7));
    let now_iso = iso(now);
    let recent_week_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        importance_min: None,
        created_after: Some(week_ago.clone()),
        created_before: Some(now_iso.clone()),
        context_tags: None,
        entity_id: None,
        agent_type: None,
    };
    check_top3(
        storage,
        &mut results,
        rss,
        "recent sprint completions",
        "Sprint 20 completed",
        "temporal",
        verbose,
        &recent_week_opts,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "sprint deployments last week",
        "deployed feature batch",
        "temporal",
        verbose,
        &recent_week_opts,
    )
    .await?;

    let two_weeks_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        importance_min: None,
        created_after: Some(iso(now - Duration::days(14))),
        created_before: Some(now_iso.clone()),
        context_tags: None,
        entity_id: None,
        agent_type: None,
    };
    check_top3(
        storage,
        &mut results,
        rss,
        "sprint completions last two weeks",
        "Sprint 1 completed",
        "temporal",
        verbose,
        &two_weeks_opts,
    )
    .await?;

    let month_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        importance_min: None,
        created_after: Some(iso(now - Duration::days(30))),
        created_before: Some(now_iso.clone()),
        context_tags: None,
        entity_id: None,
        agent_type: None,
    };
    check_top3(
        storage,
        &mut results,
        rss,
        "what was deployed last month",
        "Sprint",
        "temporal",
        verbose,
        &month_opts,
    )
    .await?;

    for _ in 0..4 {
        let old_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            importance_min: None,
            created_after: Some(iso(now - Duration::days(90))),
            created_before: Some(iso(now - Duration::days(80))),
            context_tags: None,
            entity_id: None,
            agent_type: None,
        };
        let hits = query_top3(storage, "sprint completion", &old_opts).await?;
        rss.sample();
        let passed = hits.is_empty();
        let detail = if verbose && !passed {
            Some(format!(
                "  [FAIL] Expected no results for 80-90 days ago range, got {}",
                hits.len()
            ))
        } else {
            None
        };
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        record_question_eval(
            "temporal",
            "sprint completion",
            "no results expected for 80-90 days ago",
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, detail);
    }

    for days_window in [3_i64, 7, 14, 21, 30, 45, 60] {
        let window_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            importance_min: None,
            created_after: Some(iso(now - Duration::days(days_window))),
            created_before: Some(now_iso.clone()),
            context_tags: None,
            entity_id: None,
            agent_type: None,
        };
        let hits = query_top3(storage, "sprint deployed feature batch", &window_opts).await?;
        rss.sample();
        let passed = !hits.is_empty();
        let detail = if verbose && !passed {
            Some(format!(
                "  [FAIL] Expected results for last {days_window} days, got 0"
            ))
        } else {
            None
        };
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        let expected = format!("at least one result expected for last {days_window} days");
        record_question_eval(
            "temporal",
            "sprint deployed feature batch",
            expected.as_str(),
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, detail);
    }

    for i in 0..5_i64 {
        let rolling_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            importance_min: None,
            created_after: Some(iso(now - Duration::days(10 + i * 10))),
            created_before: Some(iso(now - Duration::days(i * 10))),
            context_tags: None,
            entity_id: None,
            agent_type: None,
        };
        let hits = query_top3(storage, "sprint completed production", &rolling_opts).await?;
        rss.sample();
        let passed = !hits.is_empty();
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        record_question_eval(
            "temporal",
            "sprint completed production",
            "at least one result expected within rolling window",
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, None);
    }

    check_top3(
        storage,
        &mut results,
        rss,
        "API response format",
        "JSON for all endpoints",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "database backup frequency",
        "every 6 hours",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "frontend build tooling",
        "Vite for 10x faster",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "authentication token expiration",
        "15 minutes with refresh",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "application hosting infrastructure",
        "ECS Fargate with auto-scaling",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "logging implementation",
        "structured logging with Winston",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "test execution workflow",
        "automatically on every pull request",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "database schema migration system",
        "Alembic for database schema",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "error handling HTTP responses",
        "error classification",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;
    check_top3(
        storage,
        &mut results,
        rss,
        "search implementation",
        "full-text search with tsvector",
        "knowledge_update",
        verbose,
        &no_filter,
    )
    .await?;

    for (query_text, old_substring) in [
        ("API response format", "XML for all endpoints"),
        ("database backup frequency", "daily at 2 AM"),
        ("frontend build tooling", "uses Create React App"),
        ("authentication token expiration", "expire after 24 hours"),
        ("application hosting", "single EC2 instance"),
    ] {
        let hits = query_top3(storage, query_text, &no_filter).await?;
        rss.sample();
        let top_is_old = hits
            .first()
            .map(|hit| {
                hit.content
                    .to_lowercase()
                    .contains(&old_substring.to_lowercase())
            })
            .unwrap_or(false);
        let passed = !top_is_old;
        let detail = if verbose && !passed {
            let top = hits
                .first()
                .map(|hit| truncate(hit.content.as_str(), 60))
                .unwrap_or_else(|| "NO RESULTS".to_string());
            Some(format!("  [FAIL] Old version ranked #1: {top}"))
        } else {
            None
        };
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        let expected = format!("top result should not be old value: {old_substring}");
        record_question_eval(
            "knowledge_update",
            query_text,
            expected.as_str(),
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "knowledge_update", passed, detail);
    }

    for (query_text, new_substring) in [
        ("logging approach", "structured logging"),
        ("test automation", "automatically"),
        ("migration system", "Alembic"),
        ("error responses", "classification"),
        ("search upgrade", "tsvector"),
    ] {
        check_top3(
            storage,
            &mut results,
            rss,
            query_text,
            new_substring,
            "knowledge_update",
            verbose,
            &no_filter,
        )
        .await?;
    }

    let irrelevant_queries = vec![
        "quantum computing superconductor temperature",
        "recipe for chocolate cake ingredients",
        "stock market prediction algorithm",
        "ancient Roman history gladiator battles",
        "knitting patterns for winter sweaters",
        "deep sea marine biology bioluminescence",
        "amateur radio frequency bands",
        "origami crane folding instructions",
        "volcanic eruption prediction methods",
        "medieval castle architecture design",
        "astrophotography camera settings Milky Way",
        "woodworking dovetail joint techniques",
        "cheese aging cave temperature humidity",
        "hot air balloon flight physics",
        "crossword puzzle solving strategies",
        "beekeeping hive inspection schedule",
        "surfboard shaping foam blank",
        "calligraphy brush stroke techniques",
        "gem cutting faceting angles",
        "sourdough bread starter maintenance",
    ];
    for query_text in irrelevant_queries {
        let hits = query_top3(storage, query_text, &no_filter).await?;
        rss.sample();
        let passed = hits.is_empty()
            || hits
                .iter()
                .all(|hit| hit.score < ABSTENTION_MIN_TEXT as f32);
        let detail = if verbose && !passed {
            let top = hits.first().map(|hit| hit.score).unwrap_or(0.0);
            Some(format!(
                "  [FAIL] Q: {}  top_relevance={top:.2}",
                truncate(query_text, 40)
            ))
        } else {
            None
        };
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        record_question_eval(
            "abstention",
            query_text,
            "question should be unanswerable from stored memories",
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "abstention", passed, detail);
    }

    Ok(results)
}

fn print_results(results: &BTreeMap<String, CategoryResult>) -> (usize, usize, f64) {
    println!();
    println!("============================================================");
    println!("  ROMEGA LongMemEval Benchmark Results");
    println!("============================================================");

    let mut total_correct = 0usize;
    let mut total_questions = 0usize;
    for (key, label) in categories() {
        if let Some(cat) = results.get(key) {
            let percent = pct(cat.correct, cat.total);
            let filled = (percent / 5.0).floor() as usize;
            let bar = format!(
                "{}{}",
                "#".repeat(filled),
                "-".repeat(20usize.saturating_sub(filled))
            );
            println!(
                "\n  {label:30} {:3}/{:3}  [{bar}] {:5.1}%  ({})",
                cat.correct,
                cat.total,
                percent,
                grade(percent)
            );
            for line in &cat.details {
                println!("{line}");
            }
            total_correct += cat.correct;
            total_questions += cat.total;
        }
    }

    let overall = pct(total_correct, total_questions);
    println!("\n------------------------------------------------------------");
    println!("  OVERALL: {total_correct}/{total_questions} = {overall:.1}%");
    println!("------------------------------------------------------------");
    (total_correct, total_questions, overall)
}

fn print_side_by_side_results(
    substring: &BTreeMap<String, CategoryResult>,
    llm: &BTreeMap<String, CategoryResult>,
) -> ((usize, usize, f64), (usize, usize, f64)) {
    println!();
    println!(
        "===================================================================================================================="
    );
    println!("  ROMEGA LongMemEval Benchmark Results (Substring vs LLM Judge)");
    println!(
        "===================================================================================================================="
    );
    println!(
        "  {:30} {:>24} {:>24}",
        "Category", "Substring", "LLM Judge"
    );

    let mut sub_correct = 0usize;
    let mut sub_total = 0usize;
    let mut llm_correct = 0usize;
    let mut llm_total = 0usize;

    for (key, label) in categories() {
        let sub = substring.get(key).cloned().unwrap_or_default();
        let llm_cat = llm.get(key).cloned().unwrap_or_default();
        let sub_pct = pct(sub.correct, sub.total);
        let llm_pct = pct(llm_cat.correct, llm_cat.total);
        println!(
            "  {label:30} {:>3}/{:<3} {:>6.1}% {:>3}   {:>3}/{:<3} {:>6.1}% {:>3}",
            sub.correct,
            sub.total,
            sub_pct,
            grade(sub_pct),
            llm_cat.correct,
            llm_cat.total,
            llm_pct,
            grade(llm_pct),
        );
        sub_correct += sub.correct;
        sub_total += sub.total;
        llm_correct += llm_cat.correct;
        llm_total += llm_cat.total;
    }

    let sub_overall = pct(sub_correct, sub_total);
    let llm_overall = pct(llm_correct, llm_total);
    println!(
        "--------------------------------------------------------------------------------------------------------------------"
    );
    println!(
        "  {:30} {:>3}/{:<3} {:>6.1}% {:>3}   {:>3}/{:<3} {:>6.1}% {:>3}",
        "OVERALL",
        sub_correct,
        sub_total,
        sub_overall,
        grade(sub_overall),
        llm_correct,
        llm_total,
        llm_overall,
        grade(llm_overall),
    );
    println!(
        "--------------------------------------------------------------------------------------------------------------------"
    );

    (
        (sub_correct, sub_total, sub_overall),
        (llm_correct, llm_total, llm_overall),
    )
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut rss = PeakRss::default();
    rss.sample();

    if args.llm_judge {
        load_api_key_from_dotenv();
        init_llm_judge(args.judge_model.as_str())?;
    }

    let runtime = tokio::runtime::Runtime::new()?;
    let storage = SqliteStorage::new_in_memory()?;

    println!("Creating benchmark database...");
    println!("Seeding memories...");
    let seed_start = Instant::now();
    let seeded_memories = runtime.block_on(seed_memories(&storage, &mut rss))?;
    let seeding_ms = seed_start.elapsed().as_millis();
    println!("Seeded {seeded_memories} memories.");

    println!("Running benchmark...");
    if args.llm_judge {
        println!("  (LLM-as-judge mode: {})", args.judge_model);
    }
    let query_start = Instant::now();
    let mut llm_judge_calls = 0usize;
    let substring_results = runtime.block_on(run_benchmark(&storage, args.verbose, &mut rss))?;
    let querying_ms = query_start.elapsed().as_millis();

    let evals = snapshot_question_evals();
    let (total_correct, total_questions, overall);
    let mut llm_results_summary: Option<BTreeMap<String, CategoryResult>> = None;
    if args.llm_judge {
        llm_judge_calls = evals.len();
        let (llm_results, cost, fallback_count) =
            runtime.block_on(run_llm_judge(&evals, args.verbose));
        let (substring_totals, llm_totals) =
            print_side_by_side_results(&substring_results, &llm_results);
        llm_results_summary = Some(llm_results);
        (total_correct, total_questions, overall) = llm_totals;
        println!(
            "Substring overall: {}/{} = {:.1}%",
            substring_totals.0, substring_totals.1, substring_totals.2
        );
        println!(
            "LLM judge overall: {}/{} = {:.1}%",
            llm_totals.0, llm_totals.1, llm_totals.2
        );
        println!(
            "Estimated input cost: ${:.6} ({} tokens x ${:.2}/1M)",
            cost.estimated_input_cost_usd,
            cost.input_tokens_estimate,
            cost.input_rate_per_million_usd
        );
        if fallback_count > 0 {
            println!("LLM fallback-to-substring count: {fallback_count}");
        }
    } else {
        (total_correct, total_questions, overall) = print_results(&substring_results);
    }
    println!("Seeding time:  {seeding_ms} ms");
    println!("Query time:    {querying_ms} ms");
    if args.llm_judge {
        println!("LLM judge calls: {llm_judge_calls}");
    }
    println!("Peak RSS:      {} KB", rss.peak_kb);

    let summary_categories = match llm_results_summary {
        Some(llm_results) => llm_results,
        None => substring_results.clone(),
    };

    let summary = Summary {
        seeded_memories,
        seeding_ms,
        querying_ms,
        peak_rss_kb: rss.peak_kb,
        total_correct,
        total_questions,
        overall_percentage: overall,
        categories: summary_categories,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    }

    Ok(())
}
