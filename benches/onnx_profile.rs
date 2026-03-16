//! Micro-benchmark to profile each step of the ONNX embedding pipeline.
//!
//! Run with:
//!   cargo run --release --bin onnx_profile --features real-embeddings

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use mag::app_paths;

const ITERATIONS: usize = 100;
const MODEL_NAME: &str = "bge-small-en-v1.5";

/// Varied text inputs to avoid caching artifacts.
fn test_texts() -> Vec<String> {
    let templates = [
        "The quick brown fox jumps over the lazy dog.",
        "Rust is a systems programming language focused on safety and performance.",
        "Machine learning models can be deployed using ONNX runtime for cross-platform inference.",
        "SQLite is a self-contained, serverless, zero-configuration database engine.",
        "Embeddings represent text as dense vectors in a high-dimensional space.",
        "The bge-small-en-v1.5 model produces 384-dimensional embedding vectors.",
        "Tokenization splits text into subword units that the model can process.",
        "Mean pooling aggregates token-level representations into a single vector.",
        "Reciprocal rank fusion combines multiple ranking signals into a unified score.",
        "Memory systems need efficient retrieval to support real-time applications.",
        "Knowledge graphs represent relationships between entities as directed edges.",
        "Vector databases enable approximate nearest neighbor search at scale.",
        "The attention mechanism allows transformers to weigh input tokens differently.",
        "Batch processing can amortize fixed costs across multiple inputs.",
        "Session management in ONNX runtime controls thread allocation and memory.",
        "Cosine similarity measures the angle between two vectors regardless of magnitude.",
        "BM25 is a probabilistic ranking function used in information retrieval.",
        "Time decay functions reduce the relevance of older memories over time.",
        "Architectural decisions should be documented for future maintainability.",
        "Concurrent access patterns require careful synchronization in Rust.",
    ];
    // Cycle through templates to get ITERATIONS texts, each slightly varied
    (0..ITERATIONS)
        .map(|i| {
            let base = &templates[i % templates.len()];
            format!("{base} (variant {i})")
        })
        .collect()
}

fn default_model_dir() -> Result<PathBuf> {
    Ok(app_paths::resolve_app_paths()?.model_root.join(MODEL_NAME))
}

struct TimingStats {
    tokenization: Vec<Duration>,
    value_creation: Vec<Duration>,
    session_run: Vec<Duration>,
    output_extraction: Vec<Duration>,
    total_embed: Vec<Duration>,
}

impl TimingStats {
    fn new() -> Self {
        Self {
            tokenization: Vec::with_capacity(ITERATIONS),
            value_creation: Vec::with_capacity(ITERATIONS),
            session_run: Vec::with_capacity(ITERATIONS),
            output_extraction: Vec::with_capacity(ITERATIONS),
            total_embed: Vec::with_capacity(ITERATIONS),
        }
    }
}

fn report(label: &str, durations: &[Duration]) {
    if durations.is_empty() {
        println!("  {label:<22} no samples");
        return;
    }
    #[allow(clippy::cast_precision_loss)]
    let count = durations.len() as f64;
    let total: Duration = durations.iter().sum();
    let mean = total.as_secs_f64() / count * 1000.0; // ms
    let mut sorted: Vec<f64> = durations.iter().map(|d| d.as_secs_f64() * 1000.0).collect();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let last = sorted.len() - 1;
    let min = sorted[0];
    let max = sorted[last];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let p50 = sorted[((last as f64) * 0.50).round() as usize];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let p95 = sorted[((last as f64) * 0.95).round() as usize];
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let p99 = sorted[((last as f64) * 0.99).round() as usize];
    let std_dev = {
        let variance = sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count;
        variance.sqrt()
    };

    println!(
        "  {label:<22} mean={mean:>8.3}ms  p50={p50:>8.3}ms  p95={p95:>8.3}ms  p99={p99:>8.3}ms  min={min:>8.3}ms  max={max:>8.3}ms  std={std_dev:>7.3}ms  total={total_s:>8.3}s",
        total_s = total.as_secs_f64(),
    );
}

fn normalize_embedding(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vec {
            *value /= norm;
        }
    }
}

fn main() -> Result<()> {
    println!("=== ONNX Embedding Pipeline Profiler ===");
    println!("Iterations: {ITERATIONS}");
    println!();

    // ── Load model files ──────────────────────────────────────────────
    let model_dir = default_model_dir()?;
    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if !model_path.exists() || !tokenizer_path.exists() {
        return Err(anyhow!(
            "Model files not found at {}. Run the server once to download them.",
            model_dir.display()
        ));
    }

    // ── Session creation (one-time cost) ──────────────────────────────
    println!("[1/6] Creating ONNX session...");
    let t0 = Instant::now();
    // Match Python defaults: 0 threads (auto), ORT_ENABLE_ALL optimization
    let mut session = ort::session::Session::builder()?
        .with_intra_threads(0)?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
        .commit_from_file(&model_path)
        .context("failed to create ONNX session")?;
    let session_create_time = t0.elapsed();
    println!(
        "  Session creation: {:.3}ms",
        session_create_time.as_secs_f64() * 1000.0
    );
    println!();

    // ── Tokenizer creation (one-time cost) ────────────────────────────
    println!("[2/6] Loading tokenizer...");
    let t0 = Instant::now();
    let mut tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| anyhow!("failed to load tokenizer: {e}"))?;
    tokenizer
        .with_truncation(Some(tokenizers::TruncationParams {
            max_length: 512,
            ..Default::default()
        }))
        .map_err(|e| anyhow!("failed to configure truncation: {e}"))?;
    let tokenizer_create_time = t0.elapsed();
    println!(
        "  Tokenizer load: {:.3}ms",
        tokenizer_create_time.as_secs_f64() * 1000.0
    );
    println!();

    // ── Warmup run (exclude from stats) ───────────────────────────────
    println!("[3/6] Warmup run (1 embedding, excluded from stats)...");
    {
        let encoding = tokenizer
            .encode("warmup text for ONNX runtime", true)
            .map_err(|e| anyhow!("tokenization failed: {e}"))?;
        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let seq_len = input_ids.len();
        let token_type_ids = vec![0_i64; seq_len];

        let input_ids_val = ort::value::Value::from_array(([1usize, seq_len], input_ids))?;
        let token_type_ids_val =
            ort::value::Value::from_array(([1usize, seq_len], token_type_ids))?;
        let attention_mask_val =
            ort::value::Value::from_array(([1usize, seq_len], attention_mask))?;

        let _outputs = session.run(ort::inputs![
            input_ids_val,
            attention_mask_val,
            token_type_ids_val
        ])?;
    }
    println!("  Warmup complete.");
    println!();

    // ── Generate test texts ───────────────────────────────────────────
    let texts = test_texts();

    // ── Profile each step ─────────────────────────────────────────────
    println!("[4/6] Running {ITERATIONS} embedding calls with per-step timing...");
    println!();

    let mut stats = TimingStats::new();
    let overall_start = Instant::now();

    for text in &texts {
        let embed_start = Instant::now();

        // Step 1: Tokenization
        let t1 = Instant::now();
        let encoding = tokenizer
            .encode(text.as_str(), true)
            .map_err(|e| anyhow!("tokenization failed: {e}"))?;
        let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();
        let seq_len = input_ids.len();
        let token_type_ids = vec![0_i64; seq_len];
        stats.tokenization.push(t1.elapsed());

        // Step 2: Value creation (3 tensors)
        let t2 = Instant::now();
        let input_ids_val = ort::value::Value::from_array(([1usize, seq_len], input_ids))
            .context("failed to create input_ids value")?;
        let token_type_ids_val = ort::value::Value::from_array(([1usize, seq_len], token_type_ids))
            .context("failed to create token_type_ids value")?;
        let attention_mask_val = ort::value::Value::from_array(([1usize, seq_len], attention_mask))
            .context("failed to create attention_mask value")?;
        stats.value_creation.push(t2.elapsed());

        // Step 3: Session.run() — the actual ONNX inference
        let t3 = Instant::now();
        let outputs = session
            .run(ort::inputs![
                input_ids_val,
                attention_mask_val,
                token_type_ids_val
            ])
            .context("ONNX inference failed")?;
        stats.session_run.push(t3.elapsed());

        // Step 4: Output extraction + mean pooling + normalization
        let t4 = Instant::now();
        let first_output = outputs
            .get("last_hidden_state")
            .ok_or_else(|| anyhow!("missing last_hidden_state"))?;
        let (shape, output) = first_output
            .try_extract_tensor::<f32>()
            .context("failed to extract tensor")?;

        if shape.len() != 3 || shape[0] != 1 {
            return Err(anyhow!("unexpected ONNX output shape: {shape:?}"));
        }
        let output_seq_len = usize::try_from(shape[1]).unwrap_or(0);
        let hidden_size = usize::try_from(shape[2]).unwrap_or(0);
        let expected_len = output_seq_len * hidden_size;
        if output.len() != expected_len {
            return Err(anyhow!(
                "unexpected tensor buffer length: got {}, expected {expected_len} (shape={shape:?})",
                output.len(),
            ));
        }
        let effective_len = output_seq_len.min(seq_len);

        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;
        for token_idx in 0..effective_len {
            #[allow(clippy::cast_precision_loss)]
            let mask_value = encoding.get_attention_mask()[token_idx] as f32;
            if mask_value <= 0.0 {
                continue;
            }
            mask_sum += mask_value;
            for (dim, pooled_value) in pooled.iter_mut().enumerate() {
                let flat_index = token_idx * hidden_size + dim;
                *pooled_value += output[flat_index] * mask_value;
            }
        }
        if mask_sum <= 0.0 {
            return Err(anyhow!("attention mask sum is zero during mean pooling"));
        }
        for value in &mut pooled {
            *value /= mask_sum;
        }
        normalize_embedding(&mut pooled);
        stats.output_extraction.push(t4.elapsed());

        stats.total_embed.push(embed_start.elapsed());
    }

    let overall_elapsed = overall_start.elapsed();

    // ── Report ────────────────────────────────────────────────────────
    println!("[5/6] Per-step timing results ({ITERATIONS} iterations):");
    println!();
    report("Tokenization", &stats.tokenization);
    report("Value creation", &stats.value_creation);
    report("Session.run()", &stats.session_run);
    report("Output extraction", &stats.output_extraction);
    report("Total per-embed", &stats.total_embed);
    println!();

    // Percentage breakdown
    let total_tok: f64 = stats.tokenization.iter().map(|d| d.as_secs_f64()).sum();
    let total_val: f64 = stats.value_creation.iter().map(|d| d.as_secs_f64()).sum();
    let total_run: f64 = stats.session_run.iter().map(|d| d.as_secs_f64()).sum();
    let total_ext: f64 = stats
        .output_extraction
        .iter()
        .map(|d| d.as_secs_f64())
        .sum();
    let total_emb: f64 = stats.total_embed.iter().map(|d| d.as_secs_f64()).sum();
    let overhead = total_emb - (total_tok + total_val + total_run + total_ext);

    println!("[6/6] Percentage breakdown:");
    println!();
    if total_emb > 0.0 {
        println!(
            "  Tokenization:      {:>6.1}%  ({:.3}s)",
            total_tok / total_emb * 100.0,
            total_tok
        );
        println!(
            "  Value creation:    {:>6.1}%  ({:.3}s)",
            total_val / total_emb * 100.0,
            total_val
        );
        println!(
            "  Session.run():     {:>6.1}%  ({:.3}s)",
            total_run / total_emb * 100.0,
            total_run
        );
        println!(
            "  Output extraction: {:>6.1}%  ({:.3}s)",
            total_ext / total_emb * 100.0,
            total_ext
        );
        println!(
            "  Measurement noise: {:>6.1}%  ({:.3}s)",
            overhead / total_emb * 100.0,
            overhead
        );
    } else {
        println!("  no samples");
    }
    println!();
    println!(
        "  Wall time for {ITERATIONS} embeddings: {:.3}s",
        overall_elapsed.as_secs_f64()
    );
    #[allow(clippy::cast_precision_loss)]
    let throughput = ITERATIONS as f64 / overall_elapsed.as_secs_f64();
    println!("  Throughput: {throughput:.1} embeddings/sec");
    println!();

    // ── Mutex overhead measurement ────────────────────────────────────
    println!("=== Bonus: Mutex overhead measurement ===");
    println!();
    {
        let session_mutex = std::sync::Mutex::new(());
        let mut lock_times = Vec::with_capacity(ITERATIONS);
        for _ in 0..ITERATIONS {
            let t = Instant::now();
            let _guard = session_mutex
                .lock()
                .map_err(|_| anyhow!("mutex poisoned"))?;
            lock_times.push(t.elapsed());
        }
        report("Mutex lock (uncontended)", &lock_times);
    }
    println!();

    // ── spawn_blocking overhead measurement ───────────────────────────
    println!("=== Bonus: spawn_blocking overhead (async context) ===");
    println!();
    {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .context("failed to create tokio runtime")?;

        rt.block_on(async {
            let mut spawn_times = Vec::with_capacity(ITERATIONS);

            // Measure spawn_blocking overhead with a no-op task
            for _ in 0..ITERATIONS {
                let t = Instant::now();
                tokio::task::spawn_blocking(|| {
                    // no-op — just measuring scheduling overhead
                })
                .await
                .context("spawn_blocking join error")?;
                spawn_times.push(t.elapsed());
            }
            report("spawn_blocking (no-op)", &spawn_times);

            // Measure spawn_blocking with a real embed call
            // We can't move the session into the closure easily, so we measure
            // the scheduling overhead separately from the inference time above.
            println!();
            println!(
                "  (Inference overhead from spawn_blocking = spawn_blocking(no-op) time above)"
            );
            println!("  (In production, each embed() call pays this cost on top of inference)");
            Ok::<_, anyhow::Error>(())
        })?;
    }
    println!();

    // ── Token count distribution ──────────────────────────────────────
    println!("=== Token count distribution for test texts ===");
    println!();
    let mut token_counts: Vec<usize> = texts
        .iter()
        .map(|t| {
            tokenizer
                .encode(t.as_str(), true)
                .map(|e| e.get_ids().len())
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| anyhow!("tokenization failed: {e}"))?;
    token_counts.sort();
    if token_counts.is_empty() {
        println!("  no samples");
    } else {
        #[allow(clippy::cast_precision_loss)]
        let avg_tokens: f64 = token_counts.iter().sum::<usize>() as f64 / token_counts.len() as f64;
        println!(
            "  min={} avg={:.1} max={} (for {} texts)",
            token_counts[0],
            avg_tokens,
            token_counts[token_counts.len() - 1],
            token_counts.len()
        );
    }
    println!();

    Ok(())
}
