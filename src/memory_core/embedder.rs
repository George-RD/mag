use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

use crate::app_paths;

/// Trait for generating text embeddings.
pub trait Embedder: Send + Sync {
    /// Returns the embedding dimension.
    fn dimension(&self) -> usize;
    /// Generates an embedding vector for the given text.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Generates embedding vectors for multiple texts in a single call.
    /// The default implementation calls `embed()` in a loop; backends that
    /// support true batched inference (e.g. ONNX) override this for better
    /// throughput.
    #[allow(dead_code)]
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct PlaceholderEmbedder;

impl Embedder for PlaceholderEmbedder {
    fn dimension(&self) -> usize {
        32
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Ok(embedding_for_text(text))
    }
}

#[allow(dead_code)]
pub(crate) fn embedding_for_text(input: &str) -> Vec<f32> {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut vec: Vec<f32> = digest.iter().map(|b| *b as f32 / 255.0).collect();
    normalize_embedding(&mut vec);
    vec
}

pub(crate) fn normalize_embedding(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vec {
            *value /= norm;
        }
    }
}

#[cfg(feature = "real-embeddings")]
const MODEL_NAME: &str = "bge-small-en-v1.5";
#[cfg(feature = "real-embeddings")]
const MODEL_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/onnx/model.onnx";
#[cfg(feature = "real-embeddings")]
const TOKENIZER_URL: &str =
    "https://huggingface.co/BAAI/bge-small-en-v1.5/resolve/main/tokenizer.json";

#[cfg(feature = "real-embeddings")]
const EMBEDDING_CACHE_CAPACITY: std::num::NonZeroUsize = std::num::NonZeroUsize::new(2048).unwrap();

#[cfg(feature = "real-embeddings")]
const IDLE_TIMEOUT_SECS: u64 = 600; // 10 minutes

#[cfg(feature = "real-embeddings")]
#[derive(Debug)]
pub struct OnnxEmbedder {
    model_dir: PathBuf,
    model_url: String,
    model_data_url: Option<String>,
    tokenizer_url: String,
    dimension: usize,
    output_tensor_name: String,
    use_token_type_ids: bool,
    runtime: std::sync::Mutex<Option<OnnxRuntime>>,
    last_used: std::sync::atomic::AtomicU64,
    cache: std::sync::Mutex<lru::LruCache<[u8; 32], Vec<f32>>>,
}

#[cfg(feature = "real-embeddings")]
#[derive(Debug)]
struct OnnxRuntime {
    session: std::sync::Mutex<ort::session::Session>,
    tokenizer: tokenizers::Tokenizer,
}

#[cfg(feature = "real-embeddings")]
#[derive(Debug, Clone)]
struct ModelFiles {
    directory: PathBuf,
    model_path: PathBuf,
    model_data_path: Option<PathBuf>,
    tokenizer_path: PathBuf,
}

#[cfg(feature = "real-embeddings")]
impl OnnxEmbedder {
    pub fn new() -> Result<Self> {
        Self::with_model(
            "bge-small-en-v1.5",
            MODEL_URL,
            TOKENIZER_URL,
            384,
            "last_hidden_state",
        )
    }

    pub fn with_model(
        name: &str,
        model_url: &str,
        tokenizer_url: &str,
        dimension: usize,
        output_tensor_name: &str,
    ) -> Result<Self> {
        Self::with_model_and_data(name, model_url, None, tokenizer_url, dimension, output_tensor_name, true)
    }

    pub fn with_model_and_data(
        name: &str,
        model_url: &str,
        model_data_url: Option<&str>,
        tokenizer_url: &str,
        dimension: usize,
        output_tensor_name: &str,
        use_token_type_ids: bool,
    ) -> Result<Self> {
        let model_dir = app_paths::resolve_app_paths()?.model_root.join(name);
        Ok(Self {
            model_dir,
            model_url: model_url.to_string(),
            model_data_url: model_data_url.map(str::to_string),
            tokenizer_url: tokenizer_url.to_string(),
            dimension,
            output_tensor_name: output_tensor_name.to_string(),
            use_token_type_ids,
            runtime: std::sync::Mutex::new(None),
            last_used: std::sync::atomic::AtomicU64::new(0),
            cache: std::sync::Mutex::new(lru::LruCache::new(EMBEDDING_CACHE_CAPACITY)),
        })
    }

    fn epoch_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    fn touch_last_used(&self) {
        self.last_used
            .store(Self::epoch_secs(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Eagerly load the ONNX session so the first `embed()` call doesn't pay
    /// the cold-start penalty. Must be called from an async context (uses
    /// `spawn_blocking` internally since ONNX init creates a mini runtime).
    pub async fn warmup(self: &std::sync::Arc<Self>) -> Result<()> {
        {
            let guard = self
                .runtime
                .lock()
                .map_err(|_| anyhow!("onnx runtime mutex poisoned"))?;
            if guard.is_some() {
                return Ok(());
            }
        }
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let mut guard = this
                .runtime
                .lock()
                .map_err(|_| anyhow!("onnx runtime mutex poisoned"))?;
            if guard.is_none() {
                let rt = this.init_runtime()?;
                *guard = Some(rt);
                this.touch_last_used();
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")?
    }

    /// Drops the ONNX session if it has been idle for longer than the timeout,
    /// freeing ~240 MB RSS. The LRU embedding cache is preserved. The session
    /// is re-initialised transparently on the next `embed()` call.
    pub fn try_unload_if_idle(&self) -> bool {
        let last = self.last_used.load(std::sync::atomic::Ordering::Relaxed);
        if last == 0 {
            return false; // never loaded
        }
        if Self::epoch_secs().saturating_sub(last) < IDLE_TIMEOUT_SECS {
            return false;
        }
        if let Ok(mut guard) = self.runtime.lock()
            && guard.is_some()
        {
            *guard = None;
            tracing::info!("unloaded idle ONNX session after {IDLE_TIMEOUT_SECS}s");
            return true;
        }
        false
    }

    /// Periodic maintenance entry-point. Call from a tokio interval timer.
    pub async fn maintenance_tick(self: &std::sync::Arc<Self>) {
        let this = std::sync::Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || {
            this.try_unload_if_idle();
        })
        .await;
    }

    fn init_runtime(&self) -> Result<OnnxRuntime> {
        let files = ensure_model_files_blocking(
            self.model_dir.clone(),
            &self.model_url,
            self.model_data_url.as_deref(),
            &self.tokenizer_url,
        )?;
        // Force CPU-only execution (skip CoreML/Metal which leak memory on
        // long-running macOS processes) and disable the CPU memory arena to
        // reduce RSS by ~50 MB.
        let cpu_ep = ort::ep::CPU::default().with_arena_allocator(false).build();
        let session = ort::session::Session::builder()?
            .with_execution_providers([cpu_ep])?
            .with_intra_threads(num_cpus::get())?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .commit_from_file(&files.model_path)
            .with_context(|| {
                format!(
                    "failed to create ONNX session from {}",
                    files.model_path.display()
                )
            })?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(&files.tokenizer_path)
            .map_err(|e| anyhow!("failed to load tokenizer: {e}"))?;
        // bge-small-en-v1.5 supports max 512 tokens. Truncate longer inputs to avoid
        // ONNX positional-encoding broadcast errors.
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .map_err(|e| anyhow!("failed to configure tokenizer truncation: {e}"))?;
        Ok(OnnxRuntime {
            session: std::sync::Mutex::new(session),
            tokenizer,
        })
    }
}

#[cfg(feature = "real-embeddings")]
impl Embedder for OnnxEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Cache lookup by SHA256 hash of input text
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let key: [u8; 32] = hasher.finalize().into();

        match self.cache.lock() {
            Ok(mut cache) => {
                if let Some(cached) = cache.get(&key) {
                    return Ok(cached.clone());
                }
            }
            Err(_) => tracing::warn!("embedding cache mutex poisoned, bypassing cache"),
        }

        // Cache miss — compute embedding.
        // Acquire the runtime, initialising on demand if it was unloaded.
        // Scoped so all ONNX borrows are released before caching.
        let pooled = {
            let mut rt_guard = self
                .runtime
                .lock()
                .map_err(|_| anyhow!("onnx runtime mutex poisoned"))?;
            if rt_guard.is_none() {
                *rt_guard = Some(self.init_runtime()?);
                self.touch_last_used();
            }
            let runtime = rt_guard
                .as_ref()
                .ok_or_else(|| anyhow!("runtime missing after init"))?;

            let encoding = runtime
                .tokenizer
                .encode(text, true)
                .map_err(|e| anyhow!("tokenization failed: {e}"))?;
            let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
            let attention_mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| m as i64)
                .collect();
            if input_ids.is_empty() || input_ids.len() != attention_mask.len() {
                return Err(anyhow!("invalid tokenization output for embedding"));
            }

            let seq_len = input_ids.len();
            let input_ids_value = ort::value::Value::from_array(([1_usize, seq_len], input_ids))
                .context("failed to create ONNX input_ids value")?;
            let attention_mask_value =
                ort::value::Value::from_array(([1_usize, seq_len], attention_mask))
                    .context("failed to create ONNX attention_mask value")?;

            let mut session = runtime
                .session
                .lock()
                .map_err(|_| anyhow!("onnx session mutex poisoned"))?;
            let outputs = if self.use_token_type_ids {
                let token_type_ids_value =
                    ort::value::Value::from_array(([1_usize, seq_len], vec![0_i64; seq_len]))
                        .context("failed to create ONNX token_type_ids value")?;
                session
                    .run(ort::inputs![
                        input_ids_value,
                        attention_mask_value,
                        token_type_ids_value
                    ])
                    .context("ONNX inference failed")?
            } else {
                session
                    .run(ort::inputs![input_ids_value, attention_mask_value])
                    .context("ONNX inference failed")?
            };
            let first_output = outputs
                .get(self.output_tensor_name.as_str())
                .ok_or_else(|| {
                    anyhow!("missing ONNX output tensor '{}'", self.output_tensor_name)
                })?;
            let (shape, output) = first_output
                .try_extract_tensor::<f32>()
                .context("failed to extract ONNX output tensor")?;

            // Support both 2D pre-pooled tensors (shape [1, hidden]) and
            // 3D token-level tensors (shape [1, seq_len, hidden]) that need mean-pooling.
            let mut pooled = if shape.len() == 2 {
                // Pre-pooled output (e.g. onnx-community/voyage-4-nano-ONNX `pooler_output`).
                // Shape: [1, hidden_size] — already mean-pooled and L2-normalised by the model.
                if shape[0] != 1 {
                    return Err(anyhow!("unexpected ONNX output shape: {shape:?}"));
                }
                let hidden_size =
                    usize::try_from(shape[1]).context("invalid output hidden size")?;
                if hidden_size < self.dimension {
                    return Err(anyhow!(
                        "ONNX output dim {hidden_size} is smaller than requested dim {}",
                        self.dimension
                    ));
                }
                // Matryoshka truncation: slice to requested dimension then re-normalise.
                output[..self.dimension].to_vec()
            } else if shape.len() == 3 {
                // Token-level output — apply mean pooling with attention mask.
                if shape[0] != 1 {
                    return Err(anyhow!("unexpected ONNX output shape: {shape:?}"));
                }
                let output_seq_len =
                    usize::try_from(shape[1]).context("invalid output sequence length")?;
                let hidden_size =
                    usize::try_from(shape[2]).context("invalid output hidden size")?;
                if hidden_size < self.dimension {
                    return Err(anyhow!(
                        "ONNX output dim {hidden_size} smaller than requested dim {}",
                        self.dimension
                    ));
                }
                if output_seq_len == 0 {
                    return Err(anyhow!("ONNX output sequence length is zero"));
                }
                let effective_len = output_seq_len.min(seq_len);
                let mut pooled = vec![0.0f32; self.dimension];
                let mut mask_sum = 0.0f32;
                for token_idx in 0..effective_len {
                    #[allow(clippy::cast_precision_loss)]
                    let mask_value = encoding.get_attention_mask()[token_idx] as f32;
                    if mask_value <= 0.0 {
                        continue;
                    }
                    mask_sum += mask_value;
                    for (d, pooled_value) in pooled.iter_mut().enumerate() {
                        let flat_index = token_idx * hidden_size + d;
                        *pooled_value += output[flat_index] * mask_value;
                    }
                }
                if mask_sum <= 0.0 {
                    return Err(anyhow!("attention mask sum is zero during mean pooling"));
                }
                for value in &mut pooled {
                    *value /= mask_sum;
                }
                pooled
            } else {
                return Err(anyhow!("unexpected ONNX output shape: {shape:?}"));
            };
            normalize_embedding(&mut pooled);
            pooled
        };
        self.touch_last_used();

        // Cache the result before returning
        let result = pooled.clone();
        match self.cache.lock() {
            Ok(mut cache) => {
                cache.put(key, pooled);
            }
            Err(_) => tracing::warn!("embedding cache mutex poisoned, bypassing cache"),
        }

        Ok(result)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Single-element batch: delegate to the optimised single-text path.
        if texts.len() == 1 {
            return Ok(vec![self.embed(texts[0])?]);
        }

        // --- Cache probe: split into hits and misses ---
        let mut keys: Vec<[u8; 32]> = Vec::with_capacity(texts.len());
        for text in texts {
            let mut hasher = Sha256::new();
            hasher.update(text.as_bytes());
            keys.push(hasher.finalize().into());
        }

        // `results[i]` = Some(embedding) if cached, None if needs compute.
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        // Indices (into `texts`) that still need ONNX inference.
        let mut miss_indices: Vec<usize> = Vec::new();

        match self.cache.lock() {
            Ok(mut cache) => {
                for (i, key) in keys.iter().enumerate() {
                    if let Some(cached) = cache.get(key) {
                        results[i] = Some(cached.clone());
                    } else {
                        miss_indices.push(i);
                    }
                }
            }
            Err(_) => {
                tracing::warn!("embedding cache mutex poisoned, bypassing cache");
                miss_indices.extend(0..texts.len());
            }
        }

        // All hits — return immediately.
        if miss_indices.is_empty() {
            return results
                .into_iter()
                .map(|opt| opt.ok_or_else(|| anyhow!("unexpected None in cache-hit path")))
                .collect();
        }

        // --- Batched ONNX inference for cache misses ---
        let computed = {
            let mut rt_guard = self
                .runtime
                .lock()
                .map_err(|_| anyhow!("onnx runtime mutex poisoned"))?;
            if rt_guard.is_none() {
                *rt_guard = Some(self.init_runtime()?);
                self.touch_last_used();
            }
            let runtime = rt_guard
                .as_ref()
                .ok_or_else(|| anyhow!("runtime missing after init"))?;

            // Tokenize all miss texts.
            let miss_texts: Vec<&str> = miss_indices.iter().map(|&i| texts[i]).collect();
            let encodings: Vec<tokenizers::Encoding> = miss_texts
                .iter()
                .map(|t| {
                    runtime
                        .tokenizer
                        .encode(*t, true)
                        .map_err(|e| anyhow!("tokenization failed: {e}"))
                })
                .collect::<Result<Vec<_>>>()?;

            // Find the maximum sequence length across the batch for padding.
            let max_len = encodings
                .iter()
                .map(|enc| enc.get_ids().len())
                .max()
                .ok_or_else(|| anyhow!("empty encodings in batch"))?;
            if max_len == 0 {
                return Err(anyhow!("all tokenizations produced zero-length sequences"));
            }

            let batch_size = encodings.len();

            // Build padded flat tensors: [batch_size * max_len].
            let mut flat_input_ids = vec![0_i64; batch_size * max_len];
            let mut flat_attention_mask = vec![0_i64; batch_size * max_len];

            for (b, enc) in encodings.iter().enumerate() {
                let ids = enc.get_ids();
                let mask = enc.get_attention_mask();
                let seq_len = ids.len();
                if seq_len != mask.len() {
                    return Err(anyhow!(
                        "tokenization ids/mask length mismatch for batch item {b}"
                    ));
                }
                let offset = b * max_len;
                for j in 0..seq_len {
                    flat_input_ids[offset + j] = ids[j] as i64;
                    flat_attention_mask[offset + j] = mask[j] as i64;
                }
                // Remaining positions stay 0 (padding).
            }

            let input_ids_value =
                ort::value::Value::from_array(([batch_size, max_len], flat_input_ids))
                    .context("failed to create batched ONNX input_ids value")?;
            let attention_mask_value =
                ort::value::Value::from_array(([batch_size, max_len], flat_attention_mask))
                    .context("failed to create batched ONNX attention_mask value")?;

            let mut session = runtime
                .session
                .lock()
                .map_err(|_| anyhow!("onnx session mutex poisoned"))?;
            let outputs = if self.use_token_type_ids {
                let token_type_ids_value =
                    ort::value::Value::from_array(([batch_size, max_len], vec![0_i64; batch_size * max_len]))
                        .context("failed to create batched ONNX token_type_ids value")?;
                session
                    .run(ort::inputs![
                        input_ids_value,
                        attention_mask_value,
                        token_type_ids_value
                    ])
                    .context("batched ONNX inference failed")?
            } else {
                session
                    .run(ort::inputs![input_ids_value, attention_mask_value])
                    .context("batched ONNX inference failed")?
            };
            let first_output = outputs
                .get(self.output_tensor_name.as_str())
                .ok_or_else(|| {
                    anyhow!("missing ONNX output tensor '{}'", self.output_tensor_name)
                })?;
            let (shape, output) = first_output
                .try_extract_tensor::<f32>()
                .context("failed to extract batched ONNX output tensor")?;

            // Support both 2D pre-pooled tensors (shape [batch, hidden]) and
            // 3D token-level tensors (shape [batch, seq_len, hidden]).
            let mut batch_embeddings: Vec<Vec<f32>> = Vec::with_capacity(batch_size);
            if shape.len() == 2 {
                // Pre-pooled output — shape [batch, hidden_size].
                let out_batch =
                    usize::try_from(shape[0]).context("invalid output batch dimension")?;
                let hidden_size =
                    usize::try_from(shape[1]).context("invalid output hidden size")?;
                if out_batch != batch_size {
                    return Err(anyhow!(
                        "output batch size mismatch: got {out_batch}, expected {batch_size}"
                    ));
                }
                if hidden_size < self.dimension {
                    return Err(anyhow!(
                        "ONNX output dim {hidden_size} is smaller than requested dim {}",
                        self.dimension
                    ));
                }
                for b in 0..batch_size {
                    let row_start = b * hidden_size;
                    // Matryoshka truncation + re-normalise.
                    let mut pooled = output[row_start..row_start + self.dimension].to_vec();
                    normalize_embedding(&mut pooled);
                    batch_embeddings.push(pooled);
                }
            } else if shape.len() == 3 {
                let out_batch =
                    usize::try_from(shape[0]).context("invalid output batch dimension")?;
                let out_seq_len =
                    usize::try_from(shape[1]).context("invalid output sequence length")?;
                let hidden_size =
                    usize::try_from(shape[2]).context("invalid output hidden size")?;
                if out_batch != batch_size {
                    return Err(anyhow!(
                        "output batch size mismatch: got {out_batch}, expected {batch_size}"
                    ));
                }
                if hidden_size < self.dimension {
                    return Err(anyhow!(
                        "unexpected embedding dimension: got {hidden_size}, expected >= {}",
                        self.dimension
                    ));
                }
                if out_seq_len == 0 {
                    return Err(anyhow!("ONNX output sequence length is zero"));
                }
                // Mean-pool each item in the batch using its own attention mask.
                for (b, enc) in encodings.iter().enumerate() {
                    let seq_len = enc.get_ids().len();
                    let effective_len = out_seq_len.min(seq_len);
                    let mut pooled = vec![0.0f32; self.dimension];
                    let mut mask_sum = 0.0f32;

                    for token_idx in 0..effective_len {
                        // Attention mask values are 0 or 1, so cast to f32 is lossless.
                        #[allow(clippy::cast_precision_loss)]
                        let mask_value = enc.get_attention_mask()[token_idx] as f32;
                        if mask_value <= 0.0 {
                            continue;
                        }
                        mask_sum += mask_value;
                        let row_offset = b * out_seq_len * hidden_size + token_idx * hidden_size;
                        for (d, pooled_value) in pooled.iter_mut().enumerate() {
                            *pooled_value += output[row_offset + d] * mask_value;
                        }
                    }

                    if mask_sum <= 0.0 {
                        return Err(anyhow!(
                            "attention mask sum is zero during mean pooling for batch item {b}"
                        ));
                    }
                    for value in &mut pooled {
                        *value /= mask_sum;
                    }
                    normalize_embedding(&mut pooled);
                    batch_embeddings.push(pooled);
                }
            } else {
                return Err(anyhow!("unexpected batched ONNX output shape: {shape:?}"));
            }
            batch_embeddings
        };
        self.touch_last_used();

        // --- Populate cache and assemble final result vector ---
        match self.cache.lock() {
            Ok(mut cache) => {
                for (embedding, &orig_idx) in computed.into_iter().zip(miss_indices.iter()) {
                    cache.put(keys[orig_idx], embedding.clone());
                    results[orig_idx] = Some(embedding);
                }
            }
            Err(_) => {
                tracing::warn!("embedding cache mutex poisoned, bypassing cache");
                for (embedding, &orig_idx) in computed.into_iter().zip(miss_indices.iter()) {
                    results[orig_idx] = Some(embedding);
                }
            }
        }

        results
            .into_iter()
            .map(|opt| opt.ok_or_else(|| anyhow!("unexpected None in batch result")))
            .collect()
    }
}

#[cfg(feature = "real-embeddings")]
pub async fn download_bge_small_model() -> Result<PathBuf> {
    let model_dir = default_model_dir()?;
    let files = ensure_model_files_async(model_dir, MODEL_URL, None, TOKENIZER_URL).await?;
    Ok(files.directory)
}

#[cfg(feature = "real-embeddings")]
fn default_model_dir() -> Result<PathBuf> {
    Ok(app_paths::resolve_app_paths()?.model_root.join(MODEL_NAME))
}

#[cfg(feature = "real-embeddings")]
fn ensure_model_files_blocking(
    model_dir: PathBuf,
    model_url: &str,
    model_data_url: Option<&str>,
    tokenizer_url: &str,
) -> Result<ModelFiles> {
    if model_files_exist(&model_dir, model_data_url.is_some()) {
        return Ok(model_files_for_dir(model_dir, model_data_url));
    }

    // Create a dedicated single-threaded runtime for model download.
    // We avoid block_in_place because embed() runs inside spawn_blocking
    // threads where block_in_place panics. A lightweight current-thread
    // runtime is safe and sufficient for the download I/O.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create temporary tokio runtime for model download")?;
    let model_data_url_owned = model_data_url.map(str::to_string);
    runtime.block_on(ensure_model_files_async(
        model_dir,
        model_url,
        model_data_url_owned.as_deref(),
        tokenizer_url,
    ))
}

#[cfg(feature = "real-embeddings")]
async fn ensure_model_files_async(
    model_dir: PathBuf,
    model_url: &str,
    model_data_url: Option<&str>,
    tokenizer_url: &str,
) -> Result<ModelFiles> {
    let files = model_files_for_dir(model_dir, model_data_url);
    if model_files_exist(&files.directory, model_data_url.is_some()) {
        return Ok(files);
    }

    tokio::fs::create_dir_all(&files.directory)
        .await
        .with_context(|| {
            format!(
                "failed to create model directory {}",
                files.directory.display()
            )
        })?;

    if !tokio::fs::try_exists(&files.model_path)
        .await
        .context("failed to check model.onnx path")?
    {
        download_file(model_url, &files.model_path).await?;
    }
    if let (Some(data_url), Some(data_path)) = (model_data_url, &files.model_data_path) {
        if !tokio::fs::try_exists(data_path)
            .await
            .context("failed to check model data path")?
        {
            download_file(data_url, data_path).await?;
        }
    }
    if !tokio::fs::try_exists(&files.tokenizer_path)
        .await
        .context("failed to check tokenizer.json path")?
    {
        download_file(tokenizer_url, &files.tokenizer_path).await?;
    }

    Ok(files)
}

#[cfg(feature = "real-embeddings")]
fn model_files_exist(model_dir: &Path, has_data_file: bool) -> bool {
    let files = model_files_for_dir(model_dir.to_path_buf(), if has_data_file { Some("") } else { None });
    let base = files.model_path.exists() && files.tokenizer_path.exists();
    if has_data_file {
        base && files.model_data_path.as_ref().is_some_and(|p| p.exists())
    } else {
        base
    }
}

#[cfg(feature = "real-embeddings")]
fn model_files_for_dir(model_dir: PathBuf, model_data_url: Option<&str>) -> ModelFiles {
    let model_data_path = model_data_url.and_then(|url| {
        let filename = url.split('/').next_back()?;
        if filename.is_empty() { None } else { Some(model_dir.join(filename)) }
    });
    ModelFiles {
        model_path: model_dir.join("model.onnx"),
        model_data_path,
        tokenizer_path: model_dir.join("tokenizer.json"),
        directory: model_dir,
    }
}

#[cfg(feature = "real-embeddings")]
pub(crate) async fn download_file(url: &str, path: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download request failed for {url}"))?;
    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("failed to read body from {url}"))?;

    // Write to a temporary .part file, then atomically rename to avoid
    // leaving corrupt files on interrupted downloads.
    let mut part_name = path.file_name().unwrap_or_default().to_os_string();
    part_name.push(".part");
    let part_path = path.with_file_name(part_name);
    tokio::fs::write(&part_path, &bytes)
        .await
        .with_context(|| format!("failed to write temporary file {}", part_path.display()))?;
    tokio::fs::rename(&part_path, path).await.with_context(|| {
        format!(
            "failed to rename {} to {}",
            part_path.display(),
            path.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::storage::sqlite::cosine_similarity;

    #[test]
    fn test_placeholder_embedder_dimension() {
        let embedder = PlaceholderEmbedder;
        assert_eq!(embedder.dimension(), 32);
    }

    #[test]
    fn test_placeholder_embedder_deterministic() {
        let embedder = PlaceholderEmbedder;
        let first = embedder.embed("hello world").unwrap();
        let second = embedder.embed("hello world").unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_placeholder_embedder_different_inputs() {
        let embedder = PlaceholderEmbedder;
        let first = embedder.embed("hello world").unwrap();
        let second = embedder.embed("different text").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn test_placeholder_embedder_normalized() {
        let embedder = PlaceholderEmbedder;
        let embedding = embedder.embed("normalized").unwrap();
        let norm = embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_placeholder_embedder_empty_input() {
        let embedder = PlaceholderEmbedder;
        let embedding = embedder.embed("").unwrap();
        assert_eq!(embedding.len(), 32);
    }

    #[test]
    fn test_cosine_similarity_identical() {
        let a = vec![0.5_f32, 0.5, 0.5, 0.5];
        let score = cosine_similarity(&a, &a);
        assert!((score - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![0.0_f32, 1.0, 0.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-6);
    }

    #[test]
    fn test_cosine_similarity_different_lengths() {
        let a = vec![1.0_f32, 0.0, 0.0];
        let b = vec![1.0_f32, 0.0];
        let score = cosine_similarity(&a, &b);
        assert_eq!(score, 0.0);
    }

    // --- embed_batch tests (PlaceholderEmbedder / default impl) ---

    #[test]
    fn test_placeholder_embed_batch_empty() {
        let embedder = PlaceholderEmbedder;
        let results = embedder.embed_batch(&[]).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_placeholder_embed_batch_single() {
        let embedder = PlaceholderEmbedder;
        let single = embedder.embed("hello").unwrap();
        let batch = embedder.embed_batch(&["hello"]).unwrap();
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0], single);
    }

    #[test]
    fn test_placeholder_embed_batch_multiple() {
        let embedder = PlaceholderEmbedder;
        let texts = ["alpha", "beta", "gamma"];
        let batch = embedder.embed_batch(&texts).unwrap();
        assert_eq!(batch.len(), 3);
        // Each result should match the individual embed call.
        for (i, text) in texts.iter().enumerate() {
            let individual = embedder.embed(text).unwrap();
            assert_eq!(batch[i], individual);
        }
    }

    #[test]
    fn test_placeholder_embed_batch_normalized() {
        let embedder = PlaceholderEmbedder;
        let batch = embedder.embed_batch(&["one", "two", "three"]).unwrap();
        for emb in &batch {
            let norm = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn test_placeholder_embed_batch_deterministic() {
        let embedder = PlaceholderEmbedder;
        let first = embedder.embed_batch(&["a", "b"]).unwrap();
        let second = embedder.embed_batch(&["a", "b"]).unwrap();
        assert_eq!(first, second);
    }
}
