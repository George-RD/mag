#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, bail};
use clap::Parser;

use romega_memory::memory_core::storage::sqlite::SqliteStorage;
use romega_memory::memory_core::{ABSTENTION_MIN_TEXT, OnnxEmbedder};

mod display;
mod grid_search;
mod helpers;
mod judge;
mod local;
mod official;
mod types;

/// Fallback score for basic-search results after abstention.
/// Must stay below ABSTENTION_MIN_TEXT so the abstention grading gate passes.
const ABSTENTION_FALLBACK_SCORE: f32 = 0.1;
const DEFAULT_JUDGE_MODEL: &str = "gpt-4o-mini";
const INPUT_RATE_PER_1M_GPT_4O_MINI: f64 = 0.15;
const INPUT_RATE_PER_1M_GPT_4_1: f64 = 2.00;
const DEFAULT_OFFICIAL_DATASET: &str = "data/longmemeval_s_cleaned.json";

#[derive(Debug, Parser)]
#[command(name = "longmemeval_bench")]
#[command(about = "LongMemEval-inspired retrieval benchmark for MAG")]
struct Args {
    #[arg(long)]
    verbose: bool,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    grid_search: bool,
    #[arg(long)]
    llm_judge: bool,
    #[arg(long, default_value = DEFAULT_JUDGE_MODEL)]
    judge_model: String,
    /// Run against the official LongMemEval_S dataset (500 questions)
    #[arg(long)]
    official: bool,
    /// Path to the official dataset JSON (default: data/longmemeval_s_cleaned.json)
    #[arg(long)]
    dataset_path: Option<PathBuf>,
    /// Limit number of questions for quick testing (applies to --official only)
    #[arg(long)]
    questions: Option<usize>,
    /// Retrieve top-k results per question (default: 5 for official, 3 for local)
    #[arg(long)]
    top_k: Option<usize>,
    /// Run concurrent query throughput measurement after the sequential benchmark
    #[arg(long)]
    concurrent: bool,
    /// Use a file-backed SQLite database (WAL mode, 4 readers) instead of in-memory
    #[arg(long)]
    file_backed: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.top_k == Some(0) {
        bail!("--top-k must be greater than 0");
    }
    let mut rss = helpers::PeakRss::default();
    rss.sample();

    let runtime = tokio::runtime::Runtime::new()?;

    if args.grid_search {
        if args.llm_judge {
            eprintln!("warning: --llm-judge is ignored in --grid-search mode");
        }
        let grid_start = Instant::now();
        let results = runtime.block_on(grid_search::run_grid_search(args.verbose))?;
        let duration_seconds = grid_start.elapsed().as_secs_f64();
        if args.json {
            let summary = grid_search::build_grid_search_summary(&results, duration_seconds)?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            grid_search::print_grid_search_report(&results, duration_seconds)?;
        }
        return Ok(());
    }

    // ── Official LongMemEval_S mode ────────────────────────────────────────
    if args.official {
        if args.llm_judge {
            judge::load_api_key_from_dotenv();
            judge::init_llm_judge(args.judge_model.as_str())?;
        }
        let dataset_path = args
            .dataset_path
            .unwrap_or_else(|| PathBuf::from(DEFAULT_OFFICIAL_DATASET));
        if !args.json {
            println!(
                "Loading official dataset from {}...",
                dataset_path.display()
            );
        }
        let mut questions = official::load_official_dataset(&dataset_path)?;
        let dataset_total = questions.len();
        if !args.json {
            println!("Loaded {dataset_total} questions.");
        }

        if let Some(limit) = args.questions {
            questions.truncate(limit);
            if !args.json {
                println!("Limited to {limit} questions.");
            }
        }

        let top_k = args.top_k.unwrap_or(5);
        let embedder = std::sync::Arc::new(OnnxEmbedder::new()?);
        if !args.json {
            println!(
                "Running official LongMemEval_S benchmark (top_k={top_k}, llm_judge={})...",
                args.llm_judge
            );
        }

        let summary = runtime.block_on(official::run_official_benchmark(
            &questions,
            dataset_total,
            embedder,
            args.verbose,
            args.llm_judge,
            top_k,
            &mut rss,
        ))?;

        if args.json {
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            official::print_official_results(&summary);
        }
        return Ok(());
    }

    // ── Local hand-crafted benchmark (default) ────────────────────────────
    if args.llm_judge {
        judge::load_api_key_from_dotenv();
        judge::init_llm_judge(args.judge_model.as_str())?;
    }
    let embedder: std::sync::Arc<dyn romega_memory::memory_core::embedder::Embedder> =
        std::sync::Arc::new(OnnxEmbedder::new()?);
    let temp_db_path = if args.file_backed {
        Some(std::env::temp_dir().join(format!("romega-bench-{}.db", std::process::id())))
    } else {
        None
    };
    let storage = if let Some(ref path) = temp_db_path {
        SqliteStorage::new_with_path(path.clone(), embedder.clone())?
    } else {
        SqliteStorage::new_in_memory_with_embedder(embedder.clone())?
    };

    if !args.json {
        if args.file_backed {
            println!("Mode: file-backed (WAL, 4 readers)");
        } else {
            println!("Mode: in-memory");
        }
        println!("Creating benchmark database...");
        println!("Seeding memories...");
    }
    let seed_start = Instant::now();
    let seeded_memories = runtime.block_on(local::seed_memories(&storage, &embedder, &mut rss))?;
    let seeding_ms = seed_start.elapsed().as_millis();
    if !args.json {
        println!("Seeded {seeded_memories} memories.");
    }

    if !args.json {
        println!("Running benchmark...");
        if args.llm_judge {
            println!("  (LLM-as-judge mode: {})", args.judge_model);
        }
    }
    let query_start = Instant::now();
    let mut llm_judge_calls = 0usize;
    let top_k = args.top_k.unwrap_or(3);
    let substring_results = runtime.block_on(local::run_benchmark(
        &storage,
        args.verbose,
        &mut rss,
        ABSTENTION_MIN_TEXT as f32,
        top_k,
    ))?;
    let querying_ms = query_start.elapsed().as_millis();

    let evals = helpers::snapshot_question_evals();
    let (total_correct, total_questions, overall);
    let mut llm_results_summary: Option<BTreeMap<String, types::CategoryResult>> = None;
    if args.llm_judge {
        llm_judge_calls = evals.len();
        let (llm_results, cost, fallback_count) =
            runtime.block_on(judge::run_llm_judge(&evals, args.verbose));
        if !args.json {
            let (substring_totals, llm_totals) =
                display::print_side_by_side_results(&substring_results, &llm_results);
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
            (total_correct, total_questions, overall) = helpers::summarize_totals(&llm_results);
        }
        llm_results_summary = Some(llm_results);
    } else if !args.json {
        (total_correct, total_questions, overall) = display::print_results(&substring_results);
    } else {
        (total_correct, total_questions, overall) = helpers::summarize_totals(&substring_results);
    }
    if !args.json {
        println!("Seeding time:  {seeding_ms} ms");
        println!("Query time:    {querying_ms} ms");
        if args.llm_judge {
            println!("LLM judge calls: {llm_judge_calls}");
        }
        println!("Peak RSS:      {} KB", rss.peak_kb);
    }

    let summary_categories = match llm_results_summary {
        Some(llm_results) => llm_results,
        None => substring_results.clone(),
    };

    let summary = types::Summary {
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

    // ── Concurrent query throughput measurement ───────────────────────
    if args.concurrent {
        runtime.block_on(local::run_concurrent_benchmark(&storage, args.json))?;
    }

    // Clean up file-backed temp database
    if let Some(ref path) = temp_db_path {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
    }

    Ok(())
}
