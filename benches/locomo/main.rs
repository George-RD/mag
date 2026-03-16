#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};

use mag::benchmarking::{self, DatasetKind};
use mag::memory_core::OnnxEmbedder;
use mag::memory_core::embedder::Embedder;
use mag::memory_core::storage::sqlite::SqliteStorage;

/// How to score each question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum ScoringMode {
    /// Compare expected answer as substring of retrieved content (default).
    #[default]
    Substring,
    /// AutoMem-compatible word-overlap recall on retrieved content + metadata.
    WordOverlap,
    /// LLM-generated answer scored via token F1 (requires --llm-judge or --local).
    LlmF1,
}

mod dataset;
mod display;
mod llm;
mod openai_embedder;
mod scoring;
mod seeding;
mod types;

const DEFAULT_LLM_MODEL: &str = "gpt-4o-mini";
const DEFAULT_LOCAL_MODEL: &str = "qwen3.5-9b-optiq";
const DEFAULT_LOCAL_URL: &str = "http://localhost:1234/v1/chat/completions";

#[derive(Debug, Parser)]
#[command(name = "locomo_bench")]
#[command(about = "LoCoMo retrieval benchmark for MAG")]
struct Args {
    /// Output results as JSON.
    #[arg(long)]
    json: bool,
    /// Print per-question details.
    #[arg(long)]
    verbose: bool,
    /// Path to a pre-downloaded locomo10.json dataset.
    #[arg(long)]
    dataset_path: Option<PathBuf>,
    /// Force re-download of the dataset.
    #[arg(long)]
    force_refresh: bool,
    /// Use a temporary dataset path (cleaned up on exit).
    #[arg(long)]
    temp_dataset: bool,
    /// Limit the number of conversation samples to evaluate.
    #[arg(long)]
    samples: Option<usize>,
    /// Limit the total number of questions to evaluate.
    #[arg(long)]
    questions: Option<usize>,
    /// Retrieve top-k results per question (default: 5).
    #[arg(long)]
    top_k: Option<usize>,
    /// Use LLM generation + token F1 scoring instead of substring matching.
    #[arg(long)]
    llm_judge: bool,
    /// Use a local LM Studio server (no API key needed, implies --llm-judge).
    #[arg(long)]
    local: bool,
    /// OpenAI-compatible API endpoint URL (default: OpenAI, or localhost:1234 with --local).
    #[arg(long)]
    llm_url: Option<String>,
    /// Model name for LLM generation (default: gpt-4o-mini, or qwen3.5-9b-optiq with --local).
    #[arg(long)]
    llm_model: Option<String>,
    /// Use OpenAI text-embedding-3-large (3072-dim) instead of local ONNX bge-small-en-v1.5.
    /// Requires OPENAI_API_KEY in environment or .env.local file.
    #[arg(long)]
    openai_embeddings: bool,
    /// Scoring mode: substring, word-overlap, or llm-f1.
    /// If omitted, defaults to substring unless --llm-judge/--local implies llm-f1.
    #[arg(long, value_enum)]
    scoring_mode: Option<ScoringMode>,
}

// ── Peak RSS tracking ───────────────────────────────────────────────────

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

#[cfg(target_os = "linux")]
fn current_rss_kb() -> Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmRSS:") {
            let kb: u64 = value.trim().trim_end_matches(" kB").trim().parse()?;
            return Ok(kb);
        }
    }
    Ok(0)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn current_rss_kb() -> Result<u64> {
    Ok(0)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

// ── Main ────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let args = Args::parse();
    if args.top_k == Some(0) {
        bail!("--top-k must be greater than 0");
    }
    if args.dataset_path.is_some() && (args.force_refresh || args.temp_dataset) {
        bail!("--dataset-path cannot be combined with --force-refresh or --temp-dataset");
    }

    // Resolve effective scoring mode: explicit --scoring-mode wins.
    // Otherwise, --llm-judge/--local implies llm-f1; fallback default is substring.
    let scoring_mode = match args.scoring_mode {
        Some(mode) => mode,
        None if args.llm_judge || args.local => ScoringMode::LlmF1,
        None => ScoringMode::Substring,
    };

    if scoring_mode == ScoringMode::LlmF1 && !(args.llm_judge || args.local) {
        bail!("--scoring-mode llm-f1 requires --llm-judge or --local");
    }
    if scoring_mode == ScoringMode::WordOverlap && (args.llm_judge || args.local) {
        bail!("--scoring-mode word-overlap is incompatible with --llm-judge / --local");
    }

    let runtime = tokio::runtime::Runtime::new()?;

    // Resolve and load dataset via the shared benchmarking module.
    let dataset = runtime.block_on(benchmarking::resolve_dataset(
        DatasetKind::LoCoMo10,
        args.dataset_path.clone(),
        args.force_refresh,
        args.temp_dataset,
    ))?;
    let mut samples = dataset::load_dataset(&dataset.path)?;
    if !args.json {
        eprintln!(
            "Loaded {} samples from {}",
            samples.len(),
            dataset.path.display()
        );
    }
    if let Some(limit) = args.samples {
        samples.truncate(limit);
    }

    let metadata = benchmarking::benchmark_metadata("locomo", &dataset);

    // Initialize LLM if needed.
    let use_llm = scoring_mode == ScoringMode::LlmF1;
    if use_llm {
        let model = args.llm_model.as_deref().unwrap_or(if args.local {
            DEFAULT_LOCAL_MODEL
        } else {
            DEFAULT_LLM_MODEL
        });
        let url = args.llm_url.as_deref().unwrap_or(if args.local {
            DEFAULT_LOCAL_URL
        } else {
            llm::OPENAI_URL
        });
        if args.local {
            llm::init_llm_local(model, url)?;
        } else {
            llm::load_api_key_from_dotenv();
            llm::init_llm(model, url)?;
        }
        if !args.json {
            eprintln!("LLM generation mode: {} @ {}", model, url);
        }
    }
    if !args.json {
        eprintln!("Scoring mode: {scoring_mode:?}");
    }

    let embedder: std::sync::Arc<dyn Embedder> = if args.openai_embeddings {
        llm::load_api_key_from_dotenv();
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            anyhow::anyhow!(
                "--openai-embeddings requires OPENAI_API_KEY (env var or .env.local file)"
            )
        })?;
        if !args.json {
            eprintln!("Embedder: OpenAI text-embedding-3-large (3072-dim)");
        }
        std::sync::Arc::new(openai_embedder::OpenAiEmbedder::new(api_key)?)
    } else {
        if !args.json {
            eprintln!("Embedder: ONNX bge-small-en-v1.5 (384-dim)");
        }
        std::sync::Arc::new(OnnxEmbedder::new()?)
    };
    let top_k = args.top_k.unwrap_or(5);
    let start = Instant::now();
    let mut rss = PeakRss::default();
    rss.sample();

    let mut total_memories = 0usize;
    let mut total_queries = 0usize;
    let mut total_query_ms = 0u128;
    let mut total_correct = 0usize;
    let mut total_f1_sum = 0.0f64;
    let mut total_evidence_recall_sum = 0.0f64;
    let mut categories = BTreeMap::new();
    let mut samples_evaluated = 0usize;
    let total_question_count = args
        .questions
        .unwrap_or(usize::MAX)
        .min(samples.iter().map(|s| s.qa.len()).sum::<usize>());

    'samples: for sample in &samples {
        if let Some(limit) = args.questions
            && total_queries >= limit
        {
            break;
        }

        // Fresh database per sample -- isolates conversations.
        let storage = SqliteStorage::new_in_memory_with_embedder(embedder.clone())?;
        let seeded = runtime.block_on(seeding::seed_sample(&storage, sample))?;
        total_memories += seeded;
        samples_evaluated += 1;
        rss.sample();

        for qa in &sample.qa {
            if let Some(limit) = args.questions
                && total_queries >= limit
            {
                break 'samples;
            }

            let query_start = Instant::now();
            let hits =
                runtime.block_on(seeding::query_with_metadata(&storage, &qa.question, top_k))?;
            let query_ms = query_start.elapsed().as_millis();
            total_query_ms += query_ms;
            total_queries += 1;
            rss.sample();

            let expected_answer = qa.expected_answer();
            let category = qa.category_key();

            // Substring match (always computed for backward compat).
            let substr_passed = scoring::substring_match(&hits, expected_answer);

            // Evidence recall.
            let ev_recall = scoring::evidence_recall(&hits, &qa.evidence);

            // Primary score: depends on the chosen scoring mode.
            let is_adversarial = category == "adversarial";
            let (f1, _actual_text) = match scoring_mode {
                ScoringMode::WordOverlap => {
                    // AutoMem-compatible: recall-oriented word overlap on
                    // retrieved content + metadata dates.
                    let score = if expected_answer.is_empty() {
                        0.0
                    } else {
                        scoring::word_overlap_score(&hits, expected_answer)
                    };
                    (score, String::new())
                }
                ScoringMode::LlmF1 if !expected_answer.is_empty() => {
                    match runtime.block_on(llm::generate_answer(&qa.question, &hits)) {
                        Ok(generated) => {
                            if is_adversarial {
                                let score = if scoring::adversarial_check(&generated) {
                                    1.0
                                } else {
                                    0.0
                                };
                                (score, generated)
                            } else {
                                let (_, _, f1) = scoring::token_f1(&generated, expected_answer);
                                (f1, generated)
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "warning: LLM generation failed, using retrieval text: {err}"
                            );
                            let concat = hits
                                .iter()
                                .map(|h| h.content.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            let (_, _, f1) = scoring::token_f1(&concat, expected_answer);
                            (f1, concat)
                        }
                    }
                }
                ScoringMode::Substring | ScoringMode::LlmF1 => {
                    // Substring mode, or LlmF1 fallback when expected is empty.
                    let concat = hits
                        .iter()
                        .map(|h| h.content.as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    let (_, _, f1) = if expected_answer.is_empty() {
                        (0.0, 0.0, 0.0)
                    } else {
                        scoring::token_f1(&concat, expected_answer)
                    };
                    (f1, concat)
                }
            };

            total_f1_sum += f1;
            total_evidence_recall_sum += ev_recall;
            if substr_passed {
                total_correct += 1;
            }

            let detail = if args.verbose {
                let status = if substr_passed { "PASS" } else { "FAIL" };
                Some(format!(
                    "  [{status}] Q: {}  E: {}  F1={:.2}  EvR={:.2}",
                    truncate(&qa.question, 50),
                    truncate(expected_answer, 30),
                    f1,
                    ev_recall,
                ))
            } else {
                None
            };

            display::record_result(
                &mut categories,
                category,
                substr_passed,
                f1,
                ev_recall,
                detail,
            );

            // Progress on stderr.
            let (cat_correct, cat_total) = categories
                .get(category)
                .map(|c| (c.correct, c.total))
                .unwrap_or((0, 0));
            let status_char = if substr_passed { '✓' } else { '✗' };
            eprint!(
                "\r[{}/{}] {status_char} {} — {}/{} substr, F1={:.2} ({seeded} mems, {query_ms}ms)         ",
                total_queries,
                total_question_count,
                truncate(category, 15),
                cat_correct,
                cat_total,
                f1,
            );
            let _ = std::io::stderr().flush();
        }
    }
    eprintln!(); // Finish progress line.

    let total_duration_seconds = start.elapsed().as_secs_f64();
    let avg_query_ms = if total_queries == 0 {
        0.0
    } else {
        total_query_ms as f64 / total_queries as f64
    };
    let mean_f1 = if total_queries == 0 {
        0.0
    } else {
        total_f1_sum / total_queries as f64
    };
    let mean_evidence_recall = if total_queries == 0 {
        0.0
    } else {
        total_evidence_recall_sum / total_queries as f64
    };

    let scoring_label = match scoring_mode {
        ScoringMode::Substring => "substring",
        ScoringMode::WordOverlap => "word-overlap",
        ScoringMode::LlmF1 => "llm-f1",
    };

    let summary = types::LoCoMoSummary {
        metadata,
        dataset: "LoCoMo10".to_string(),
        scoring_mode: scoring_label.to_string(),
        samples_evaluated,
        questions_evaluated: total_queries,
        total_memories_ingested: total_memories,
        total_duration_seconds,
        avg_query_ms,
        peak_rss_kb: rss.peak_kb,
        raw_correct: total_correct,
        raw_percentage: display::pct(total_correct, total_queries),
        mean_f1,
        mean_evidence_recall,
        categories,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        display::print_results(&summary);
    }

    Ok(())
}
