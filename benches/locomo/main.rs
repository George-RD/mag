#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, bail};
use clap::{Parser, ValueEnum};

use mag::benchmarking::{self, DatasetKind};
use mag::memory_core::OnnxEmbedder;
use mag::memory_core::embedder::Embedder;
use mag::memory_core::storage::sqlite::SqliteStorage;

/// Limit mode: static (flat top_k) or dynamic (50/75/100 per question type).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
enum LimitMode {
    /// Flat top_k for all question types.
    Static,
    /// Dynamic: scales with conversation size (turns/5, cap 200), 1.5x temporal, 2x multi-hop (cap 250).
    #[default]
    Dynamic,
}

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
    /// E2E: LLM-generated answer scored via word-overlap recall (requires --llm-judge or --local).
    E2eWordOverlap,
}

#[path = "../bench_utils/mod.rs"]
mod bench_utils;
mod dataset;
mod display;
mod llm;
mod openai_embedder;
mod scoring;
mod seeding;
mod types;
mod voyage_embedder;

use bench_utils::formatting::{pct, truncate};
use bench_utils::metrics::PeakRss;

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
    /// Retrieve top-k results per question (default: 50).
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
    /// Use voyage-4-nano ONNX (onnx-community, 2048-dim native, use --voyage-quant for variant).
    #[arg(long)]
    voyage_onnx: bool,
    /// Use Voyage AI API embeddings (requires VOYAGE_API_KEY env var or .env.local).
    #[arg(long)]
    voyage_api: bool,
    /// Use ibm-granite/granite-embedding-30m-english ONNX (384-dim, RoBERTa).
    #[arg(long)]
    granite: bool,
    /// Use sentence-transformers/all-MiniLM-L6-v2 ONNX (384-dim, BERT).
    #[arg(long)]
    minilm_l6: bool,
    /// Use sentence-transformers/all-MiniLM-L12-v2 ONNX (384-dim, BERT).
    #[arg(long)]
    minilm_l12: bool,
    /// Use Xenova/e5-small-v2 ONNX (384-dim, BERT).
    #[arg(long)]
    e5_small: bool,
    /// Use BAAI/bge-base-en-v1.5 ONNX (768-dim, BERT).
    #[arg(long)]
    bge_base: bool,
    /// Use nomic-ai/nomic-embed-text-v1.5 int8 ONNX (768-dim, NomicBERT).
    /// Note: nomic requires "search_document:" / "search_query:" prefixes for optimal MTEB scores.
    /// The benchmark harness does NOT add these prefixes — scores will be slightly below MTEB reported.
    #[arg(long)]
    nomic: bool,
    /// Use Snowflake/snowflake-arctic-embed-xs ONNX (22M, 384-dim, MiniLM-based).
    #[arg(long)]
    arctic_xs: bool,
    /// Use Snowflake/snowflake-arctic-embed-s ONNX (33M, 384-dim, e5-small-unsupervised based).
    #[arg(long)]
    arctic_s: bool,
    /// Use thenlper/gte-small ONNX (33M, 384-dim, pure BERT).
    #[arg(long)]
    gte_small: bool,
    /// Voyage AI model name (default: voyage-4-lite).
    #[arg(long)]
    voyage_model: Option<String>,
    /// Embedding dimension override for Matryoshka variants (256/512/1024/2048).
    #[arg(long)]
    embedder_dim: Option<usize>,
    /// Voyage ONNX quantization variant: fp32, fp16, q4, int8 (default: int8).
    #[arg(long, default_value = "int8")]
    voyage_quant: String,
    /// Scoring mode: substring, word-overlap, llm-f1, or e2e-word-overlap.
    /// If omitted, defaults to substring unless --llm-judge/--local implies llm-f1.
    #[arg(long, value_enum)]
    scoring_mode: Option<ScoringMode>,
    /// Shorthand for --scoring-mode e2e-word-overlap (requires --llm-judge or --local).
    #[arg(long)]
    e2e: bool,
    /// Disable entity tag extraction during seeding (baseline comparison).
    #[arg(long)]
    no_entity_tags: bool,
    /// Override graph_neighbor_factor (default: from ScoringParams).
    #[arg(long)]
    graph_factor: Option<f64>,
    /// Limit mode: static (flat top_k) or dynamic (50/75/100 per question type).
    #[arg(long, value_enum, default_value_t = LimitMode::Dynamic)]
    limit_mode: LimitMode,
    /// Run a graph_neighbor_factor sweep: seed once, then re-query at multiple factor values.
    #[arg(long)]
    graph_sweep: bool,
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
    let embedder_flags = [
        args.openai_embeddings,
        args.voyage_onnx,
        args.voyage_api,
        args.granite,
        args.minilm_l6,
        args.minilm_l12,
        args.e5_small,
        args.bge_base,
        args.nomic,
        args.arctic_xs,
        args.arctic_s,
        args.gte_small,
    ]
    .iter()
    .filter(|&&b| b)
    .count();
    if embedder_flags > 1 {
        bail!(
            "only one embedder flag may be specified at a time \
             (--openai-embeddings, --voyage-onnx, --voyage-api, --granite, \
             --minilm-l6, --minilm-l12, --e5-small, --bge-base, --nomic, \
             --arctic-xs, --arctic-s, --gte-small)"
        );
    }

    // Resolve effective scoring mode: --e2e shorthand wins over implicit default,
    // explicit --scoring-mode always wins.
    let scoring_mode = match args.scoring_mode {
        Some(mode) => mode,
        None if args.e2e => ScoringMode::E2eWordOverlap,
        None if args.llm_judge || args.local => ScoringMode::LlmF1,
        None => ScoringMode::Substring,
    };

    if scoring_mode == ScoringMode::LlmF1 && !(args.llm_judge || args.local) {
        bail!("--scoring-mode llm-f1 requires --llm-judge or --local");
    }
    if scoring_mode == ScoringMode::E2eWordOverlap && !(args.llm_judge || args.local) {
        bail!("--e2e / --scoring-mode e2e-word-overlap requires --llm-judge or --local");
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
    let use_llm = matches!(
        scoring_mode,
        ScoringMode::LlmF1 | ScoringMode::E2eWordOverlap
    );
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

    let (inner_embedder, embedder_name): (Arc<dyn Embedder>, String) = if args.openai_embeddings {
        llm::load_api_key_from_dotenv();
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            anyhow::anyhow!(
                "--openai-embeddings requires OPENAI_API_KEY (env var or .env.local file)"
            )
        })?;
        if !args.json {
            eprintln!("Embedder: OpenAI text-embedding-3-large (3072-dim)");
        }
        (
            Arc::new(openai_embedder::OpenAiEmbedder::new(api_key)?),
            "text-embedding-3-large (openai api, 3072-dim)".to_string(),
        )
    } else if args.voyage_onnx {
        let dim = args.embedder_dim.unwrap_or(2048);
        let quant = args.voyage_quant.as_str();
        let (model_file, data_file, model_label) = match quant {
            "fp32" => ("onnx/model.onnx", "onnx/model.onnx_data", "FP32"),
            "fp16" => ("onnx/model_fp16.onnx", "onnx/model_fp16.onnx_data", "FP16"),
            "q4" => ("onnx/model_q4.onnx", "onnx/model_q4.onnx_data", "Q4"),
            _ => (
                "onnx/model_quantized.onnx",
                "onnx/model_quantized.onnx_data",
                "INT8",
            ), // default
        };
        let base = "https://huggingface.co/onnx-community/voyage-4-nano-ONNX/resolve/main";
        let model_url = format!("{base}/{model_file}");
        let model_data_url = format!("{base}/{data_file}");
        let tokenizer_url = format!("{base}/tokenizer.json");
        if !args.json {
            eprintln!("Embedder: voyage-4-nano {model_label} ONNX ({dim}-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "voyage-4-nano-onnx-community",
                &model_url,
                Some(&model_data_url),
                &tokenizer_url,
                dim,
                "pooler_output",
                false, // voyage model uses only input_ids + attention_mask
            )?),
            format!("voyage-4-nano {model_label} onnx ({dim}-dim)"),
        )
    } else if args.voyage_api {
        llm::load_api_key_from_dotenv();
        let api_key = std::env::var("VOYAGE_API_KEY").map_err(|_| {
            anyhow::anyhow!("--voyage-api requires VOYAGE_API_KEY (env var or .env.local file)")
        })?;
        let model = args
            .voyage_model
            .clone()
            .unwrap_or_else(|| "voyage-4-lite".to_string());
        let dim = args.embedder_dim.unwrap_or(1024);
        if !args.json {
            eprintln!("Embedder: Voyage API {model} ({dim}-dim)");
        }
        (
            Arc::new(voyage_embedder::VoyageApiEmbedder::new(
                api_key,
                model.clone(),
                dim,
            )?),
            format!("{model} (voyage api, {dim}-dim)"),
        )
    } else if args.granite {
        if !args.json {
            eprintln!("Embedder: granite-embedding-30m-english ONNX (384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "granite-embedding-30m-english",
                "https://huggingface.co/ibm-granite/granite-embedding-30m-english/resolve/main/model.onnx",
                None,
                "https://huggingface.co/ibm-granite/granite-embedding-30m-english/resolve/main/tokenizer.json",
                384,
                "logits",
                false, // RoBERTa: no token_type_ids
            )?),
            "granite-embedding-30m-english".to_string(),
        )
    } else if args.minilm_l6 {
        if !args.json {
            eprintln!("Embedder: all-MiniLM-L6-v2 ONNX (384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "all-MiniLM-L6-v2",
                "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // BERT: uses token_type_ids
            )?),
            "all-MiniLM-L6-v2".to_string(),
        )
    } else if args.minilm_l12 {
        if !args.json {
            eprintln!("Embedder: all-MiniLM-L12-v2 ONNX (384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "all-MiniLM-L12-v2",
                "https://huggingface.co/sentence-transformers/all-MiniLM-L12-v2/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/sentence-transformers/all-MiniLM-L12-v2/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // BERT: uses token_type_ids
            )?),
            "all-MiniLM-L12-v2".to_string(),
        )
    } else if args.e5_small {
        if !args.json {
            eprintln!("Embedder: e5-small-v2 ONNX (384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "e5-small-v2",
                "https://huggingface.co/Xenova/e5-small-v2/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/Xenova/e5-small-v2/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // BERT: uses token_type_ids
            )?),
            "e5-small-v2".to_string(),
        )
    } else if args.bge_base {
        if !args.json {
            eprintln!("Embedder: bge-base-en-v1.5 ONNX (768-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "bge-base-en-v1.5",
                "https://huggingface.co/BAAI/bge-base-en-v1.5/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/BAAI/bge-base-en-v1.5/resolve/main/tokenizer.json",
                768,
                "last_hidden_state",
                true, // BERT: uses token_type_ids
            )?),
            "bge-base-en-v1.5".to_string(),
        )
    } else if args.nomic {
        if !args.json {
            eprintln!("Embedder: nomic-embed-text-v1.5 int8 ONNX (768-dim)");
            eprintln!(
                "Note: nomic optimal retrieval requires search_document:/search_query: prefixes."
            );
            eprintln!(
                "      Benchmark harness does NOT add prefixes — scores will be slightly below MTEB."
            );
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "nomic-embed-text-v1.5-int8",
                "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/onnx/model_int8.onnx",
                None,
                "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/tokenizer.json",
                768,
                "last_hidden_state",
                true, // NomicBERT: uses token_type_ids
            )?),
            "nomic-embed-text-v1.5-int8".to_string(),
        )
    } else if args.arctic_xs {
        if !args.json {
            eprintln!("Embedder: snowflake-arctic-embed-xs ONNX (22M, 384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "snowflake-arctic-embed-xs",
                "https://huggingface.co/Snowflake/snowflake-arctic-embed-xs/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/Snowflake/snowflake-arctic-embed-xs/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // arctic-xs ONNX export includes token_type_ids
            )?),
            "snowflake-arctic-embed-xs".to_string(),
        )
    } else if args.arctic_s {
        if !args.json {
            eprintln!("Embedder: snowflake-arctic-embed-s ONNX (33M, 384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "snowflake-arctic-embed-s",
                "https://huggingface.co/Snowflake/snowflake-arctic-embed-s/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/Snowflake/snowflake-arctic-embed-s/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // arctic-s ONNX export includes token_type_ids
            )?),
            "snowflake-arctic-embed-s".to_string(),
        )
    } else if args.gte_small {
        if !args.json {
            eprintln!("Embedder: gte-small ONNX (33M, 384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::with_model_and_data(
                "gte-small",
                "https://huggingface.co/thenlper/gte-small/resolve/main/onnx/model.onnx",
                None,
                "https://huggingface.co/thenlper/gte-small/resolve/main/tokenizer.json",
                384,
                "last_hidden_state",
                true, // pure BERT: uses token_type_ids
            )?),
            "gte-small".to_string(),
        )
    } else {
        if !args.json {
            eprintln!("Embedder: ONNX bge-small-en-v1.5 (384-dim)");
        }
        (
            Arc::new(OnnxEmbedder::new()?),
            "bge-small-en-v1.5 (onnx, 384-dim)".to_string(),
        )
    };

    let timing = Arc::new(bench_utils::timing_embedder::TimingEmbedder::new(
        inner_embedder,
    ));
    let embedder: Arc<dyn Embedder> = timing.clone();
    let top_k = args.top_k.unwrap_or(50);
    let start = Instant::now();
    let mut rss = PeakRss::default();
    rss.sample();

    let mut total_memories = 0usize;
    let mut total_queries = 0usize;
    let mut total_query_ms = 0u128;
    let mut total_seed_ms = 0u128;
    let mut total_correct = 0usize;
    let mut total_f1_sum = 0.0f64;
    let mut total_evidence_recall_sum = 0.0f64;
    let mut categories = BTreeMap::new();
    let mut samples_evaluated = 0usize;
    let total_question_count = args
        .questions
        .unwrap_or(usize::MAX)
        .min(samples.iter().map(|s| s.qa.len()).sum::<usize>());

    // Aggregate graph edge stats across all samples.
    let mut graph_edge_totals: BTreeMap<String, i64> = BTreeMap::new();

    // Keep seeded storages for --graph-sweep (re-query without re-seeding).
    let mut sweep_storages: Vec<(SqliteStorage, usize)> = Vec::new();

    'samples: for sample in &samples {
        if let Some(limit) = args.questions
            && total_queries >= limit
        {
            break;
        }

        // Fresh database per sample -- isolates conversations.
        let mut storage = SqliteStorage::new_in_memory_with_embedder(embedder.clone())?;

        // Apply CLI overrides to scoring params.
        if let Some(gf) = args.graph_factor {
            let mut params = storage.scoring_params().clone();
            params.graph_neighbor_factor = gf;
            storage.set_scoring_params(params);
        }

        let seed_start = Instant::now();
        let seeded =
            runtime.block_on(seeding::seed_sample(&storage, sample, !args.no_entity_tags))?;
        total_seed_ms += seed_start.elapsed().as_millis();
        total_memories += seeded;
        samples_evaluated += 1;
        rss.sample();

        // Collect graph edge stats after seeding.
        if let Ok(edge_stats) = runtime.block_on(storage.graph_edge_stats()) {
            for (rel_type, count) in &edge_stats {
                *graph_edge_totals.entry(rel_type.clone()).or_insert(0) += count;
            }
        }

        for qa in &sample.qa {
            if let Some(limit) = args.questions
                && total_queries >= limit
            {
                break 'samples;
            }

            let query_start = Instant::now();
            let effective_limit = if args.limit_mode == LimitMode::Dynamic {
                let is_multihop = qa.evidence.len() > 1;
                let is_temporal = is_temporal_question(&qa.question);
                // Scale base limit with conversation size: larger conversations
                // need more results to maintain the same coverage ratio.
                // Floor at top_k, ceiling at 15% of conversation or 200.
                let scaled_base = (seeded / 5).max(top_k).min(200);
                if is_multihop {
                    // Multi-hop needs the most coverage
                    (scaled_base * 2).min(250)
                } else if is_temporal {
                    // Temporal needs moderate extra coverage
                    ((scaled_base * 3) / 2).min(200)
                } else {
                    scaled_base
                }
            } else {
                top_k
            };
            let hits = if matches!(scoring_mode, ScoringMode::WordOverlap) {
                runtime.block_on(seeding::query_with_speaker_recall(
                    &storage,
                    &qa.question,
                    &sample.sample_id,
                    effective_limit,
                ))?
            } else {
                runtime.block_on(seeding::query_with_metadata(
                    &storage,
                    &qa.question,
                    effective_limit,
                ))?
            };
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
                    } else if scoring::is_adversarial_expected(expected_answer) {
                        // Adversarial questions: expected answer is "Not
                        // mentioned" etc. Score based on whether the system
                        // correctly abstained rather than token-matching "not"
                        // against retrieved content.
                        scoring::adversarial_retrieval_score(&hits)
                    } else if category == "multi-hop" {
                        // LoCoMo official: split multi-hop answers by comma,
                        // extract before semicolon, compute per-part F1, average.
                        scoring::multi_hop_word_overlap_score(&hits, expected_answer)
                    } else {
                        scoring::word_overlap_score(&hits, expected_answer)
                    };
                    (score, String::new())
                }
                ScoringMode::E2eWordOverlap if !expected_answer.is_empty() => {
                    // E2E: generate LLM answer, then score with word-overlap recall.
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
                                let score =
                                    scoring::word_overlap_on_text(&generated, expected_answer);
                                (score, generated)
                            }
                        }
                        Err(err) => {
                            eprintln!(
                                "warning: LLM generation failed, falling back to retrieval word-overlap: {err}"
                            );
                            let score = scoring::word_overlap_score(&hits, expected_answer);
                            (score, String::new())
                        }
                    }
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
                ScoringMode::Substring | ScoringMode::LlmF1 | ScoringMode::E2eWordOverlap => {
                    // Substring mode, or LlmF1/E2eWordOverlap fallback when expected is empty.
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

        // Retain seeded storage for graph-sweep (avoids re-seeding).
        if args.graph_sweep {
            sweep_storages.push((storage, seeded));
        }
    }
    eprintln!(); // Finish progress line.

    let total_duration_seconds = start.elapsed().as_secs_f64();
    #[allow(clippy::cast_precision_loss)]
    let avg_query_ms = if total_queries == 0 {
        0.0
    } else {
        total_query_ms as f64 / total_queries as f64
    };
    #[allow(clippy::cast_precision_loss)]
    let mean_f1 = if total_queries == 0 {
        0.0
    } else {
        total_f1_sum / total_queries as f64
    };
    #[allow(clippy::cast_precision_loss)]
    let mean_evidence_recall = if total_queries == 0 {
        0.0
    } else {
        total_evidence_recall_sum / total_queries as f64
    };

    let scoring_label = match scoring_mode {
        ScoringMode::Substring => "substring",
        ScoringMode::WordOverlap => "word-overlap",
        ScoringMode::LlmF1 => "llm-f1",
        ScoringMode::E2eWordOverlap => "e2e-word-overlap",
    };

    let total_embed_calls = timing.total_calls();
    let avg_embed_ms = timing.avg_embed_ms();
    #[allow(clippy::cast_possible_truncation)]
    let total_seed_ms_u64 = total_seed_ms as u64;

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
        raw_percentage: pct(total_correct, total_queries),
        mean_f1,
        mean_evidence_recall,
        categories,
        embedder_name,
        total_seed_ms: total_seed_ms_u64,
        total_embed_calls,
        avg_embed_ms,
        graph_edge_totals,
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        display::print_results(&summary);
    }

    // ── Graph factor sweep ──────────────────────────────────────────────
    if args.graph_sweep && !sweep_storages.is_empty() {
        let factors = [0.0, 0.05, 0.1, 0.2, 0.3, 0.5, 0.75, 1.0];
        let use_word_overlap = matches!(scoring_mode, ScoringMode::WordOverlap);

        eprintln!();
        eprintln!("Running graph factor sweep ({} factors)...", factors.len());

        // Collect (factor, per-category f1 sums, per-category totals, overall f1 sum, overall total).
        let mut sweep_results: Vec<types::SweepRow> = Vec::new();

        for &factor in &factors {
            let mut cat_scores: types::SweepCategoryScores = BTreeMap::new();

            for (idx, (storage, seeded)) in sweep_storages.iter_mut().enumerate() {
                // Update scoring params for this factor.
                let mut params = storage.scoring_params().clone();
                params.graph_neighbor_factor = factor;
                storage.set_scoring_params(params);

                let sample = &samples[idx];
                for qa in &sample.qa {
                    let effective_limit = if args.limit_mode == LimitMode::Dynamic {
                        let is_multihop = qa.evidence.len() > 1;
                        let is_temporal = is_temporal_question(&qa.question);
                        let scaled_base = (*seeded / 5).max(top_k).min(200);
                        if is_multihop {
                            (scaled_base * 2).min(250)
                        } else if is_temporal {
                            ((scaled_base * 3) / 2).min(200)
                        } else {
                            scaled_base
                        }
                    } else {
                        top_k
                    };

                    let hits = if use_word_overlap {
                        runtime.block_on(seeding::query_with_speaker_recall(
                            storage,
                            &qa.question,
                            &sample.sample_id,
                            effective_limit,
                        ))?
                    } else {
                        runtime.block_on(seeding::query_with_metadata(
                            storage,
                            &qa.question,
                            effective_limit,
                        ))?
                    };

                    let expected_answer = qa.expected_answer();
                    let category = qa.category_key();

                    let f1 = if use_word_overlap {
                        if expected_answer.is_empty() {
                            0.0
                        } else if scoring::is_adversarial_expected(expected_answer) {
                            scoring::adversarial_retrieval_score(&hits)
                        } else if category == "multi-hop" {
                            scoring::multi_hop_word_overlap_score(&hits, expected_answer)
                        } else {
                            scoring::word_overlap_score(&hits, expected_answer)
                        }
                    } else {
                        // Substring mode uses token F1 on concatenated text.
                        let concat = hits
                            .iter()
                            .map(|h| h.content.as_str())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if expected_answer.is_empty() {
                            0.0
                        } else {
                            let (_, _, f) = scoring::token_f1(&concat, expected_answer);
                            f
                        }
                    };

                    let entry = cat_scores.entry(category.to_string()).or_insert((0.0, 0));
                    entry.0 += f1;
                    entry.1 += 1;
                }
            }

            eprint!("\r  factor {factor:.2} done         ");
            let _ = std::io::stderr().flush();
            sweep_results.push((factor, cat_scores));
        }
        eprintln!();

        display::print_sweep_results(&sweep_results);
    }

    Ok(())
}

fn is_temporal_question(question: &str) -> bool {
    let lower = question.to_lowercase();
    lower.starts_with("when ")
        || lower.starts_with("what time ")
        || lower.starts_with("what date ")
        || lower.contains(" when did ")
        || lower.contains(" when was ")
        || lower.contains(" what year ")
        || lower.contains(" what month ")
        || lower.contains(" how long ago ")
}
