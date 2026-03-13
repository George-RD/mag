#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Parser;
use mag::benchmarking::{self, BenchmarkMetadata, DatasetKind};
use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::{AdvancedSearcher, MemoryInput, OnnxEmbedder, SearchOptions, Searcher};
use serde::{Deserialize, Serialize};

#[derive(Debug, Parser)]
#[command(name = "locomo_bench")]
#[command(about = "LoCoMo retrieval benchmark for MAG")]
struct Args {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    dataset_path: Option<PathBuf>,
    #[arg(long)]
    force_refresh: bool,
    #[arg(long)]
    temp_dataset: bool,
    #[arg(long)]
    samples: Option<usize>,
    #[arg(long)]
    questions: Option<usize>,
    #[arg(long)]
    top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct LoCoMoSample {
    sample_id: String,
    conversation: serde_json::Map<String, serde_json::Value>,
    qa: Vec<LoCoMoQuestion>,
}

#[derive(Debug, Deserialize)]
struct DialogueTurn {
    speaker: String,
    dia_id: String,
    text: String,
}

#[derive(Debug, Deserialize)]
struct LoCoMoQuestion {
    question: String,
    #[serde(default, deserialize_with = "deserialize_optional_answer")]
    answer: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_answer")]
    adversarial_answer: Option<String>,
    evidence: Vec<String>,
    category: i64,
}

impl LoCoMoQuestion {
    fn expected_answer(&self) -> &str {
        self.answer
            .as_deref()
            .or(self.adversarial_answer.as_deref())
            .unwrap_or("")
    }
}

fn deserialize_optional_answer<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::String(text) => Ok(Some(text)),
        serde_json::Value::Number(number) => Ok(Some(number.to_string())),
        other => Ok(Some(other.to_string())),
    }
}

#[derive(Debug, Default, Clone, Serialize)]
struct CategoryResult {
    total: usize,
    correct: usize,
    details: Vec<String>,
}

#[derive(Debug, Serialize)]
struct LoCoMoSummary {
    metadata: BenchmarkMetadata,
    dataset: String,
    samples_evaluated: usize,
    questions_evaluated: usize,
    total_memories_ingested: usize,
    total_duration_seconds: f64,
    avg_query_ms: f64,
    peak_rss_kb: u64,
    raw_correct: usize,
    raw_percentage: f64,
    categories: BTreeMap<String, CategoryResult>,
}

#[derive(Debug, Default)]
struct PeakRss {
    peak_kb: u64,
}

impl PeakRss {
    fn sample(&mut self) {
        if let Ok(kb) = current_rss_kb()
            && kb > self.peak_kb
        {
            self.peak_kb = kb;
        }
    }
}

#[cfg(target_os = "macos")]
fn current_rss_kb() -> Result<u64> {
    let pid = std::process::id();
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text.trim().parse()?)
}

#[cfg(not(target_os = "macos"))]
fn current_rss_kb() -> Result<u64> {
    Ok(0)
}

fn pct(correct: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        correct as f64 / total as f64 * 100.0
    }
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

fn load_dataset(path: &std::path::Path) -> Result<Vec<LoCoMoSample>> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("failed to open dataset at {}: {e}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let samples: Vec<LoCoMoSample> = serde_json::from_reader(reader)
        .map_err(|e| anyhow!("failed to parse dataset JSON: {e}"))?;
    Ok(samples)
}

async fn seed_sample(storage: &SqliteStorage, sample: &LoCoMoSample) -> Result<usize> {
    let mut count = 0usize;
    let mut session_keys = sample
        .conversation
        .keys()
        .filter(|key| {
            key.starts_with("session_")
                && !key.ends_with("_date_time")
                && !key.ends_with("_summary")
                && !key.ends_with("_observation")
        })
        .cloned()
        .collect::<Vec<_>>();
    session_keys.sort_by_key(|key| {
        key.trim_start_matches("session_")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    for key in session_keys {
        let Some(turns_value) = sample.conversation.get(&key) else {
            continue;
        };
        let Some(turns) = turns_value.as_array() else {
            continue;
        };
        let date_key = format!("{key}_date_time");
        let referenced_date = sample
            .conversation
            .get(&date_key)
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        for (turn_idx, turn) in turns.iter().enumerate() {
            let turn: DialogueTurn = serde_json::from_value(turn.clone()).map_err(|e| {
                anyhow!(
                    "failed to parse dialogue turn for {}: {e}",
                    sample.sample_id
                )
            })?;
            if turn.text.trim().is_empty() {
                continue;
            }
            let memory_id = format!("locomo-{}-{key}-{turn_idx}", sample.sample_id);
            let input = MemoryInput {
                content: String::new(),
                id: None,
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({ "dia_id": turn.dia_id }),
                session_id: Some(key.clone()),
                agent_type: Some(turn.speaker.clone()),
                referenced_date: referenced_date.clone(),
                ..MemoryInput::default()
            };
            storage
                .store(
                    &memory_id,
                    &format!("{}: {}", turn.speaker, turn.text),
                    &input,
                )
                .await?;
            count += 1;
        }
    }

    Ok(count)
}

fn no_filter() -> SearchOptions {
    SearchOptions {
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
    }
}

async fn query_hits(storage: &SqliteStorage, question: &str, top_k: usize) -> Result<Vec<String>> {
    let filters = no_filter();
    let advanced = storage.advanced_search(question, top_k, &filters).await?;
    if !advanced.is_empty() {
        return Ok(advanced.into_iter().map(|hit| hit.content).collect());
    }

    let basic = storage.search(question, top_k, &filters).await?;
    Ok(basic.into_iter().map(|hit| hit.content).collect())
}

fn normalize_answer(text: &str) -> String {
    text.to_lowercase()
}

fn answer_present(hits: &[String], expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let expected = normalize_answer(expected);
    hits.iter()
        .any(|hit| normalize_answer(hit).contains(expected.as_str()))
}

fn category_key(category: i64) -> String {
    format!("category_{category}")
}

fn print_summary(summary: &LoCoMoSummary) {
    println!();
    println!("========================================================================");
    println!("  MAG — LoCoMo Benchmark Results");
    println!("========================================================================");
    println!("  Dataset: {}", summary.dataset);
    println!("  Source: {}", summary.metadata.dataset_source);
    println!("  Cache:  {}", summary.metadata.dataset_path);
    println!(
        "  Samples: {}  Questions: {}",
        summary.samples_evaluated, summary.questions_evaluated
    );
    println!(
        "  Memories ingested: {}  Duration: {:.1}s  Avg query: {:.0}ms",
        summary.total_memories_ingested, summary.total_duration_seconds, summary.avg_query_ms
    );
    println!("  Peak RSS: {} KB", summary.peak_rss_kb);
    println!();
    for (key, cat) in &summary.categories {
        let percentage = pct(cat.correct, cat.total);
        println!(
            "  {key:12} {:>3}/{:<3} {:>5.1}% ({})",
            cat.correct,
            cat.total,
            percentage,
            grade(percentage)
        );
        for line in &cat.details {
            println!("{line}");
        }
    }
    println!();
    println!(
        "  RAW: {}/{} = {:.1}%",
        summary.raw_correct, summary.questions_evaluated, summary.raw_percentage
    );
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.top_k == Some(0) {
        bail!("--top-k must be greater than 0");
    }
    if args.dataset_path.is_some() && (args.force_refresh || args.temp_dataset) {
        bail!("--dataset-path cannot be combined with --force-refresh or --temp-dataset");
    }

    let runtime = tokio::runtime::Runtime::new()?;
    let dataset = runtime.block_on(benchmarking::resolve_dataset(
        DatasetKind::LoCoMo10,
        args.dataset_path.clone(),
        args.force_refresh,
        args.temp_dataset,
    ))?;
    let mut samples = load_dataset(&dataset.path)?;
    if let Some(limit) = args.samples {
        samples.truncate(limit);
    }

    let metadata = benchmarking::benchmark_metadata("locomo", &dataset);
    let embedder = std::sync::Arc::new(OnnxEmbedder::new()?);
    let top_k = args.top_k.unwrap_or(5);
    let start = Instant::now();
    let mut rss = PeakRss::default();
    rss.sample();

    let mut total_memories = 0usize;
    let mut total_queries = 0usize;
    let mut total_query_ms = 0u128;
    let mut total_correct = 0usize;
    let mut categories = BTreeMap::new();
    let mut samples_evaluated = 0usize;

    'samples: for sample in &samples {
        if let Some(limit) = args.questions
            && total_queries >= limit
        {
            break;
        }

        let storage = SqliteStorage::new_in_memory_with_embedder(embedder.clone())?;
        total_memories += runtime.block_on(seed_sample(&storage, sample))?;
        samples_evaluated += 1;
        rss.sample();

        for qa in &sample.qa {
            if let Some(limit) = args.questions
                && total_queries >= limit
            {
                break 'samples;
            }

            let query_start = Instant::now();
            let hits = runtime.block_on(query_hits(&storage, &qa.question, top_k))?;
            let query_ms = query_start.elapsed().as_millis();
            total_query_ms += query_ms;
            total_queries += 1;

            let expected_answer = qa.expected_answer();
            let passed = answer_present(&hits, expected_answer);
            if passed {
                total_correct += 1;
            }
            let category = category_key(qa.category);
            let detail = if args.verbose && !passed {
                Some(format!(
                    "  [FAIL] {} → expected {} (evidence: {})",
                    qa.question,
                    expected_answer,
                    qa.evidence.join(", ")
                ))
            } else {
                None
            };
            record_result(&mut categories, &category, passed, detail);
            rss.sample();
        }
    }

    let total_duration_seconds = start.elapsed().as_secs_f64();
    let avg_query_ms = if total_queries == 0 {
        0.0
    } else {
        total_query_ms as f64 / total_queries as f64
    };
    let summary = LoCoMoSummary {
        metadata,
        dataset: "LoCoMo10".to_string(),
        samples_evaluated,
        questions_evaluated: total_queries,
        total_memories_ingested: total_memories,
        total_duration_seconds,
        avg_query_ms,
        peak_rss_kb: rss.peak_kb,
        raw_correct: total_correct,
        raw_percentage: pct(total_correct, total_queries),
        categories,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_summary(&summary);
    }
    Ok(())
}
