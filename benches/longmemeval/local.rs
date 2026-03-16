use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use chrono::{Duration, SecondsFormat, Utc};
use serde::Deserialize;

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::*;

use crate::ABSTENTION_FALLBACK_SCORE;
use crate::helpers::{
    PeakRss, clear_question_evals, iso, record_question_eval, record_result, substring_match,
    truncate,
};
use crate::types::{CategoryResult, Hit};

// ── JSON data types ───────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct BenchmarkData {
    seed_memories: SeedMemories,
    questions: Questions,
}

#[derive(Debug, Deserialize)]
struct SeedMemories {
    information_extraction: Vec<IeSeedMemory>,
    multi_session: Vec<MsSeedMemory>,
    temporal: TemporalConfig,
    knowledge_update: Vec<KuPair>,
}

#[derive(Debug, Deserialize)]
struct IeSeedMemory {
    content: String,
    event_type: String,
    priority: i32,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MsSeedMemory {
    content: String,
    session_id: String,
    event_type: String,
    priority: i32,
    tags: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TemporalConfig {
    sprint_count: usize,
    interval_days: i64,
}

#[derive(Debug, Deserialize)]
struct KuPair {
    old_content: String,
    new_content: String,
}

#[derive(Debug, Deserialize)]
struct Questions {
    information_extraction: Vec<SimpleQuestion>,
    multi_session: Vec<SimpleQuestion>,
    temporal: TemporalQuestions,
    knowledge_update: KuQuestions,
    abstention: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SimpleQuestion {
    query: String,
    expected: String,
}

#[derive(Debug, Deserialize)]
struct TemporalQuestions {
    recent_week: Vec<SimpleQuestion>,
    two_weeks: Vec<SimpleQuestion>,
    month: Vec<SimpleQuestion>,
    empty_old_range: TemporalEmptyRange,
    window_checks: TemporalWindowChecks,
    rolling_windows: TemporalRollingWindows,
}

#[derive(Debug, Deserialize)]
struct TemporalEmptyRange {
    query: String,
    #[allow(dead_code)]
    expected_description: String,
    count: usize,
    range_start_days_ago: i64,
    range_end_days_ago: i64,
}

#[derive(Debug, Deserialize)]
struct TemporalWindowChecks {
    query: String,
    #[allow(dead_code)]
    expected_description_template: String,
    windows_days: Vec<i64>,
}

#[derive(Debug, Deserialize)]
struct TemporalRollingWindows {
    query: String,
    #[allow(dead_code)]
    expected_description: String,
    count: usize,
    window_size_days: i64,
}

#[derive(Debug, Deserialize)]
struct KuQuestions {
    new_value: Vec<SimpleQuestion>,
    old_not_ranked_first: Vec<KuOldNotFirst>,
    additional_new: Vec<SimpleQuestion>,
}

#[derive(Debug, Deserialize)]
struct KuOldNotFirst {
    query: String,
    old_substring: String,
}

// ── Data loading ──────────────────────────────────────────────────────────

fn load_benchmark_data() -> BenchmarkData {
    let json = include_str!("../../data/local_benchmark.json");
    serde_json::from_str(json).expect("failed to parse local_benchmark.json")
}

fn default_input(
    tags: Vec<String>,
    event_type: &str,
    session_id: &str,
    priority: i32,
    metadata: serde_json::Value,
) -> MemoryInput {
    MemoryInput {
        content: String::new(),
        id: None,
        tags,
        importance: 0.5,
        metadata,
        event_type: Some(EventType::from_str(event_type).unwrap_or_else(|e| match e {})),
        session_id: Some(session_id.to_string()),
        project: None,
        priority: Some(priority),
        entity_id: None,
        agent_type: None,
        ttl_seconds: None,
        referenced_date: None,
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

// ── Seeding ───────────────────────────────────────────────────────────────

pub(crate) async fn seed_memories(
    storage: &SqliteStorage,
    embedder: &Arc<dyn mag::memory_core::embedder::Embedder>,
    rss: &mut PeakRss,
) -> Result<usize> {
    let data = load_benchmark_data();
    let now = Utc::now();
    let mut total = 0usize;

    // Pre-warm the embedding LRU cache with a single batched ONNX inference
    // call. Individual store() calls will then hit the cache instead of running
    // per-item inference (~8ms each).
    {
        let tc = &data.seed_memories.temporal;
        let mut all_contents: Vec<String> = Vec::new();
        for mem in &data.seed_memories.information_extraction {
            all_contents.push(mem.content.clone());
        }
        for mem in &data.seed_memories.multi_session {
            all_contents.push(mem.content.clone());
        }
        for i in 0..tc.sprint_count {
            let days_ago = i as i64 * tc.interval_days;
            let ref_date = now - Duration::days(days_ago);
            let sprint_num = tc.sprint_count - i;
            all_contents.push(format!(
                "Sprint {sprint_num} completed: deployed feature batch #{sprint_num} to production on {}.",
                ref_date.format("%Y-%m-%d")
            ));
        }
        for pair in &data.seed_memories.knowledge_update {
            all_contents.push(pair.old_content.clone());
            all_contents.push(pair.new_content.clone());
        }
        let embedder = embedder.clone();
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = all_contents.iter().map(|s| s.as_str()).collect();
            embedder.embed_batch(&refs)
        })
        .await??;
    }

    // Information extraction memories.
    for (index, mem) in data.seed_memories.information_extraction.iter().enumerate() {
        let input = default_input(
            mem.tags.clone(),
            &mem.event_type,
            "bench-ie",
            mem.priority,
            serde_json::json!({}),
        );
        store_item(storage, &format!("ie-{index}"), &mem.content, &input, rss).await?;
        total += 1;
    }

    // Multi-session memories.
    for (index, mem) in data.seed_memories.multi_session.iter().enumerate() {
        let input = default_input(
            mem.tags.clone(),
            &mem.event_type,
            &mem.session_id,
            mem.priority,
            serde_json::json!({}),
        );
        store_item(storage, &format!("ms-{index}"), &mem.content, &input, rss).await?;
        total += 1;
    }

    // Temporal memories (sprint completions).
    let tc = &data.seed_memories.temporal;
    for i in 0..tc.sprint_count {
        let days_ago = i as i64 * tc.interval_days;
        let ref_date = now - Duration::days(days_ago);
        let sprint_num = tc.sprint_count - i;
        let content = format!(
            "Sprint {sprint_num} completed: deployed feature batch #{sprint_num} to production on {}.",
            ref_date.format("%Y-%m-%d")
        );
        let ref_date_str = ref_date.to_rfc3339_opts(SecondsFormat::Secs, true);
        let mut input = default_input(
            vec!["sprint".to_string()],
            "task_completion",
            &format!("bench-tr-{i}"),
            3,
            serde_json::json!({"referenced_date": ref_date_str}),
        );
        input.referenced_date = Some(ref_date_str.clone());
        store_item(storage, &format!("tr-{i}"), &content, &input, rss).await?;
        total += 1;
    }

    // Knowledge update pairs.
    let old_date = iso(now - Duration::days(60));
    let new_date = iso(now - Duration::days(2));
    for (index, pair) in data.seed_memories.knowledge_update.iter().enumerate() {
        let mut old_input = default_input(
            vec![],
            "decision",
            &format!("bench-ku-old-{index}"),
            3,
            serde_json::json!({"referenced_date": old_date, "feedback_score": -1}),
        );
        old_input.referenced_date = Some(old_date.clone());
        store_item(
            storage,
            &format!("ku-old-{index}"),
            &pair.old_content,
            &old_input,
            rss,
        )
        .await?;
        total += 1;

        let mut new_input = default_input(
            vec![],
            "decision",
            &format!("bench-ku-new-{index}"),
            4,
            serde_json::json!({"referenced_date": new_date, "feedback_score": 2}),
        );
        new_input.referenced_date = Some(new_date.clone());
        store_item(
            storage,
            &format!("ku-new-{index}"),
            &pair.new_content,
            &new_input,
            rss,
        )
        .await?;
        total += 1;
    }

    Ok(total)
}

// ── Query helpers ─────────────────────────────────────────────────────────

pub(crate) async fn query_top3(
    storage: &SqliteStorage,
    query: &str,
    top_k: usize,
    opts: &SearchOptions,
) -> Result<Vec<Hit>> {
    let advanced =
        <SqliteStorage as AdvancedSearcher>::advanced_search(storage, query, top_k, opts).await;
    match advanced {
        Ok(items) if !items.is_empty() => Ok(items
            .into_iter()
            .map(|item| Hit {
                content: item.content,
                score: item.score,
            })
            .collect()),
        Ok(_) => {
            let items = <SqliteStorage as Searcher>::search(storage, query, top_k, opts).await?;
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
            let items = <SqliteStorage as Searcher>::search(storage, query, top_k, opts).await?;
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

#[allow(clippy::too_many_arguments)]
async fn check_top3(
    storage: &SqliteStorage,
    by_category: &mut BTreeMap<String, CategoryResult>,
    rss: &mut PeakRss,
    query_text: &str,
    expected_substring: &str,
    category: &str,
    verbose: bool,
    top_k: usize,
    opts: &SearchOptions,
) -> Result<()> {
    let hits = query_top3(storage, query_text, top_k, opts).await?;
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
        let mut detail = format!(
            "  [{status}] Q: {}  E: {}",
            truncate(query_text, 60),
            truncate(expected_substring, 40)
        );
        if !passed {
            detail.push_str(&format!("  A: {}", summarize_hits(&hits)));
        }
        Some(detail)
    } else {
        None
    };
    record_result(by_category, category, passed, detail);
    Ok(())
}

// ── Benchmark runner ──────────────────────────────────────────────────────

pub(crate) async fn run_benchmark(
    storage: &SqliteStorage,
    verbose: bool,
    rss: &mut PeakRss,
    abstention_threshold: f32,
    top_k: usize,
) -> Result<BTreeMap<String, CategoryResult>> {
    let data = load_benchmark_data();
    clear_question_evals();
    let mut results = BTreeMap::<String, CategoryResult>::new();
    let no_filter = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        include_superseded: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
        event_after: None,
        event_before: None,
        explain: None,
    };

    // ── Information extraction questions ───────────────────────────────
    for q in &data.questions.information_extraction {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "information_extraction",
            verbose,
            top_k,
            &no_filter,
        )
        .await?;
    }

    // ── Multi-session questions ───────────────────────────────────────
    for q in &data.questions.multi_session {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "multi_session",
            verbose,
            top_k,
            &no_filter,
        )
        .await?;
    }

    // ── Temporal questions ────────────────────────────────────────────
    let now = Utc::now();
    let week_ago = iso(now - Duration::days(7));
    let now_iso = iso(now);
    let recent_week_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        include_superseded: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
        event_after: Some(week_ago.clone()),
        event_before: Some(now_iso.clone()),
        explain: None,
    };
    for q in &data.questions.temporal.recent_week {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "temporal",
            verbose,
            top_k,
            &recent_week_opts,
        )
        .await?;
    }

    let two_weeks_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        include_superseded: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
        event_after: Some(iso(now - Duration::days(14))),
        event_before: Some(now_iso.clone()),
        explain: None,
    };
    for q in &data.questions.temporal.two_weeks {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "temporal",
            verbose,
            top_k,
            &two_weeks_opts,
        )
        .await?;
    }

    let month_opts = SearchOptions {
        event_type: None,
        project: None,
        session_id: None,
        include_superseded: None,
        importance_min: None,
        created_after: None,
        created_before: None,
        context_tags: None,
        entity_id: None,
        agent_type: None,
        event_after: Some(iso(now - Duration::days(30))),
        event_before: Some(now_iso.clone()),
        explain: None,
    };
    for q in &data.questions.temporal.month {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "temporal",
            verbose,
            top_k,
            &month_opts,
        )
        .await?;
    }

    // Empty old range checks.
    let eor = &data.questions.temporal.empty_old_range;
    for _ in 0..eor.count {
        let old_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            include_superseded: None,
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
            entity_id: None,
            agent_type: None,
            event_after: Some(iso(now - Duration::days(eor.range_start_days_ago))),
            event_before: Some(iso(now - Duration::days(eor.range_end_days_ago))),
            explain: None,
        };
        let hits = query_top3(storage, &eor.query, top_k, &old_opts).await?;
        rss.sample();
        let passed = hits.is_empty();
        let detail = if verbose && !passed {
            Some(format!(
                "  [FAIL] Expected no results for {}-{} days ago range, got {}",
                eor.range_end_days_ago,
                eor.range_start_days_ago,
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
            &eor.query,
            &format!(
                "no results expected for {}-{} days ago",
                eor.range_end_days_ago, eor.range_start_days_ago
            ),
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, detail);
    }

    // Window checks.
    let wc = &data.questions.temporal.window_checks;
    for days_window in &wc.windows_days {
        let window_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            include_superseded: None,
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
            entity_id: None,
            agent_type: None,
            event_after: Some(iso(now - Duration::days(*days_window))),
            event_before: Some(now_iso.clone()),
            explain: None,
        };
        let hits = query_top3(storage, &wc.query, top_k, &window_opts).await?;
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
            &wc.query,
            expected.as_str(),
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, detail);
    }

    // Rolling windows.
    let rw = &data.questions.temporal.rolling_windows;
    for i in 0..rw.count as i64 {
        let rolling_opts = SearchOptions {
            event_type: None,
            project: None,
            session_id: None,
            include_superseded: None,
            importance_min: None,
            created_after: None,
            created_before: None,
            context_tags: None,
            entity_id: None,
            agent_type: None,
            event_after: Some(iso(
                now - Duration::days(rw.window_size_days + i * rw.window_size_days)
            )),
            event_before: Some(iso(now - Duration::days(i * rw.window_size_days))),
            explain: None,
        };
        let hits = query_top3(storage, &rw.query, top_k, &rolling_opts).await?;
        rss.sample();
        let passed = !hits.is_empty();
        let actual = hits
            .iter()
            .map(|hit| hit.content.as_str())
            .collect::<Vec<_>>()
            .join("\n---\n");
        record_question_eval(
            "temporal",
            &rw.query,
            "at least one result expected within rolling window",
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "temporal", passed, None);
    }

    // ── Knowledge update questions ────────────────────────────────────
    for q in &data.questions.knowledge_update.new_value {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "knowledge_update",
            verbose,
            top_k,
            &no_filter,
        )
        .await?;
    }

    // Old value should NOT be ranked first.
    for item in &data.questions.knowledge_update.old_not_ranked_first {
        let hits = query_top3(storage, &item.query, top_k, &no_filter).await?;
        rss.sample();
        let top_is_old = hits
            .first()
            .map(|hit| {
                hit.content
                    .to_lowercase()
                    .contains(&item.old_substring.to_lowercase())
            })
            .unwrap_or(false);
        let passed = !hits.is_empty() && !top_is_old;
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
        let expected = format!("top result should not be old value: {}", item.old_substring);
        record_question_eval(
            "knowledge_update",
            &item.query,
            expected.as_str(),
            actual.as_str(),
            passed,
        );
        record_result(&mut results, "knowledge_update", passed, detail);
    }

    // Additional new-value checks.
    for q in &data.questions.knowledge_update.additional_new {
        check_top3(
            storage,
            &mut results,
            rss,
            &q.query,
            &q.expected,
            "knowledge_update",
            verbose,
            top_k,
            &no_filter,
        )
        .await?;
    }

    // ── Abstention questions ──────────────────────────────────────────
    for query_text in &data.questions.abstention {
        let hits = query_top3(storage, query_text, top_k, &no_filter).await?;
        rss.sample();
        let passed = hits.is_empty() || hits.iter().all(|hit| hit.score < abstention_threshold);
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

// ── Concurrent query throughput benchmark ──────────────────────────

/// Queries used for the concurrent benchmark. A mix of different query types
/// to stress the search pipeline realistically.
const CONCURRENT_QUERIES: &[&str] = &[
    "database connection pool",
    "authentication system",
    "error handling patterns",
    "sprint completion",
    "deployment pipeline",
    "what framework are we using",
    "API rate limiting",
    "memory management",
    "test coverage goals",
    "logging strategy",
];

pub(crate) async fn run_concurrent_benchmark(storage: &SqliteStorage, json: bool) -> Result<()> {
    let storage = Arc::new(storage.clone());
    let no_filter = SearchOptions::default();

    if !json {
        println!();
        println!("── Concurrent Query Throughput ──────────────────────────");
    }

    for &concurrency in &[4, 8, 16] {
        let mut handles = Vec::with_capacity(concurrency);
        let start = Instant::now();

        for i in 0..concurrency {
            let storage = Arc::clone(&storage);
            let opts = no_filter.clone();
            let query = CONCURRENT_QUERIES[i % CONCURRENT_QUERIES.len()].to_string();
            handles.push(tokio::spawn(async move {
                let t0 = Instant::now();
                let _ = <SqliteStorage as AdvancedSearcher>::advanced_search(
                    &storage, &query, 3, &opts,
                )
                .await;
                t0.elapsed()
            }));
        }

        let mut latencies = Vec::with_capacity(concurrency);
        for handle in handles {
            #[allow(clippy::cast_precision_loss)]
            latencies.push(handle.await?.as_micros() as f64 / 1000.0);
        }
        #[allow(clippy::cast_precision_loss)]
        let wall_ms = start.elapsed().as_micros() as f64 / 1000.0;

        latencies.sort_by(|a, b| a.total_cmp(b));
        let p50 = latencies[latencies.len() / 2];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let p95 = latencies[(latencies.len() as f64 * 0.95) as usize];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
        let p99 =
            latencies[(latencies.len() as f64 * 0.99).min((latencies.len() - 1) as f64) as usize];
        #[allow(clippy::cast_precision_loss)]
        let qps = concurrency as f64 / (wall_ms / 1000.0);

        if json {
            println!(
                "{}",
                serde_json::json!({
                    "concurrency": concurrency,
                    "wall_ms": format!("{wall_ms:.1}"),
                    "qps": format!("{qps:.1}"),
                    "p50_ms": format!("{p50:.1}"),
                    "p95_ms": format!("{p95:.1}"),
                    "p99_ms": format!("{p99:.1}"),
                })
            );
        } else {
            println!(
                "  N={concurrency:>2}  wall={wall_ms:>7.1}ms  qps={qps:>6.1}  p50={p50:>6.1}ms  p95={p95:>6.1}ms  p99={p99:>6.1}ms"
            );
        }
    }

    Ok(())
}

fn summarize_hits(hits: &[Hit]) -> String {
    if hits.is_empty() {
        return "NO RESULTS".to_string();
    }

    hits.iter()
        .take(3)
        .enumerate()
        .map(|(index, hit)| {
            format!(
                "#{} {:.2} {}",
                index + 1,
                hit.score,
                truncate(hit.content.as_str(), 72)
            )
        })
        .collect::<Vec<_>>()
        .join(" | ")
}
