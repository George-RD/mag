use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};

#[cfg(feature = "real-embeddings")]
const CROSS_ENCODER_MODEL_NAME: &str = "ms-marco-MiniLM-L-6-v2";
#[cfg(feature = "real-embeddings")]
const CROSS_ENCODER_MODEL_URL: &str =
    "https://huggingface.co/cross-encoder/ms-marco-MiniLM-L-6-v2/resolve/main/onnx/model.onnx";
#[cfg(feature = "real-embeddings")]
const CROSS_ENCODER_TOKENIZER_URL: &str =
    "https://huggingface.co/cross-encoder/ms-marco-MiniLM-L-6-v2/resolve/main/tokenizer.json";

#[cfg(feature = "real-embeddings")]
const IDLE_TIMEOUT_SECS: u64 = 600; // 10 minutes

#[cfg(feature = "real-embeddings")]
#[derive(Debug)]
pub struct CrossEncoderReranker {
    model_dir: PathBuf,
    runtime: std::sync::Mutex<Option<CrossEncoderRuntime>>,
    last_used: std::sync::atomic::AtomicU64,
}

#[cfg(feature = "real-embeddings")]
#[derive(Debug)]
struct CrossEncoderRuntime {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
}

#[cfg(feature = "real-embeddings")]
#[derive(Debug, Clone)]
struct ModelFiles {
    directory: PathBuf,
    model_path: PathBuf,
    tokenizer_path: PathBuf,
}

#[cfg(feature = "real-embeddings")]
impl CrossEncoderReranker {
    pub fn new() -> Result<Self> {
        Ok(Self {
            model_dir: default_cross_encoder_dir()?,
            runtime: std::sync::Mutex::new(None),
            last_used: std::sync::atomic::AtomicU64::new(0),
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

    /// Eagerly load the ONNX session for warm start.
    pub async fn warmup(self: &std::sync::Arc<Self>) -> Result<()> {
        {
            let guard = self
                .runtime
                .lock()
                .map_err(|_| anyhow!("cross-encoder runtime mutex poisoned"))?;
            if guard.is_some() {
                return Ok(());
            }
        }
        let this = std::sync::Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            let mut guard = this
                .runtime
                .lock()
                .map_err(|_| anyhow!("cross-encoder runtime mutex poisoned"))?;
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

    /// Drops the ONNX session if idle for longer than the timeout.
    pub fn try_unload_if_idle(&self) -> bool {
        // Quick pre-check without lock to avoid contention in the common case.
        let last = self.last_used.load(std::sync::atomic::Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        if Self::epoch_secs().saturating_sub(last) < IDLE_TIMEOUT_SECS {
            return false;
        }
        // Re-check after acquiring the mutex — score_batch() may have updated
        // last_used while we were waiting for the lock.
        if let Ok(mut guard) = self.runtime.lock()
            && guard.is_some()
        {
            let fresh = self.last_used.load(std::sync::atomic::Ordering::Relaxed);
            if Self::epoch_secs().saturating_sub(fresh) < IDLE_TIMEOUT_SECS {
                return false;
            }
            *guard = None;
            tracing::info!("unloaded idle cross-encoder session after {IDLE_TIMEOUT_SECS}s");
            return true;
        }
        false
    }

    /// Periodic maintenance entry-point.
    pub async fn maintenance_tick(self: &std::sync::Arc<Self>) {
        let this = std::sync::Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || {
            this.try_unload_if_idle();
        })
        .await;
    }

    fn init_runtime(&self) -> Result<CrossEncoderRuntime> {
        let files = ensure_cross_encoder_files_blocking(self.model_dir.clone())?;
        let cpu_ep = ort::ep::CPU::default().with_arena_allocator(false).build();
        let session = ort::session::Session::builder()?
            .with_execution_providers([cpu_ep])?
            .with_intra_threads(num_cpus::get())?
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)?
            .commit_from_file(&files.model_path)
            .with_context(|| {
                format!(
                    "failed to create cross-encoder ONNX session from {}",
                    files.model_path.display()
                )
            })?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(&files.tokenizer_path)
            .map_err(|e| anyhow!("failed to load cross-encoder tokenizer: {e}"))?;
        tokenizer
            .with_truncation(Some(tokenizers::TruncationParams {
                max_length: 512,
                ..Default::default()
            }))
            .map_err(|e| anyhow!("failed to configure cross-encoder tokenizer truncation: {e}"))?;
        Ok(CrossEncoderRuntime { session, tokenizer })
    }

    /// Score a batch of query-passage pairs. Returns a relevance score (0-1) for each.
    pub fn score_batch(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        if passages.is_empty() {
            return Ok(Vec::new());
        }

        let mut rt_guard = self
            .runtime
            .lock()
            .map_err(|_| anyhow!("cross-encoder runtime mutex poisoned"))?;
        if rt_guard.is_none() {
            *rt_guard = Some(self.init_runtime()?);
            self.touch_last_used();
        }
        let runtime = rt_guard
            .as_mut()
            .ok_or_else(|| anyhow!("cross-encoder runtime missing after init"))?;

        // Tokenize all query-passage pairs
        let encodings: Vec<tokenizers::Encoding> = passages
            .iter()
            .map(|passage| {
                runtime
                    .tokenizer
                    .encode((query, *passage), true)
                    .map_err(|e| anyhow!("cross-encoder tokenization failed: {e}"))
            })
            .collect::<Result<Vec<_>>>()?;

        let max_len = encodings
            .iter()
            .map(|enc| enc.get_ids().len())
            .max()
            .ok_or_else(|| anyhow!("empty encodings in cross-encoder batch"))?;
        if max_len == 0 {
            return Err(anyhow!(
                "all cross-encoder tokenizations produced zero-length sequences"
            ));
        }

        let batch_size = encodings.len();

        // Build padded flat tensors
        let mut flat_input_ids = vec![0_i64; batch_size * max_len];
        let mut flat_attention_mask = vec![0_i64; batch_size * max_len];
        let mut flat_token_type_ids = vec![0_i64; batch_size * max_len];

        for (b, enc) in encodings.iter().enumerate() {
            let ids = enc.get_ids();
            let mask = enc.get_attention_mask();
            let type_ids = enc.get_type_ids();
            let seq_len = ids.len();
            let offset = b * max_len;
            for j in 0..seq_len {
                flat_input_ids[offset + j] = ids[j] as i64;
                flat_attention_mask[offset + j] = mask[j] as i64;
                flat_token_type_ids[offset + j] = type_ids[j] as i64;
            }
        }

        let input_ids_value =
            ort::value::Value::from_array(([batch_size, max_len], flat_input_ids))
                .context("failed to create cross-encoder input_ids value")?;
        let attention_mask_value =
            ort::value::Value::from_array(([batch_size, max_len], flat_attention_mask))
                .context("failed to create cross-encoder attention_mask value")?;
        let token_type_ids_value =
            ort::value::Value::from_array(([batch_size, max_len], flat_token_type_ids))
                .context("failed to create cross-encoder token_type_ids value")?;

        let outputs = runtime
            .session
            .run(ort::inputs![
                input_ids_value,
                attention_mask_value,
                token_type_ids_value
            ])
            .context("cross-encoder ONNX inference failed")?;

        // Cross-encoder output: logits of shape [batch_size, 1] or [batch_size]
        let logits_output = outputs
            .get("logits")
            .ok_or_else(|| anyhow!("missing cross-encoder output tensor 'logits'"))?;
        let (shape, logits) = logits_output
            .try_extract_tensor::<f32>()
            .context("failed to extract cross-encoder logits tensor")?;

        let shape_dims = shape.as_ref();
        let bs = batch_size as i64;
        let valid = match shape_dims {
            [n] => *n == bs,
            [n, 1] => *n == bs,
            _ => false,
        };
        if !valid {
            return Err(anyhow!(
                "unexpected cross-encoder logits shape {shape_dims:?}, expected [{batch_size}] or [{batch_size}, 1]"
            ));
        }

        // Apply sigmoid to each logit — tensor is contiguous so flat indexing works for both shapes
        let scores: Vec<f32> = (0..batch_size).map(|i| sigmoid(logits[i])).collect();

        self.touch_last_used();
        Ok(scores)
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

#[cfg(feature = "real-embeddings")]
fn default_cross_encoder_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            anyhow!("neither HOME nor USERPROFILE is set — cannot resolve model directory")
        })?;
    Ok(PathBuf::from(home)
        .join(".romega-memory")
        .join("models")
        .join(CROSS_ENCODER_MODEL_NAME))
}

#[cfg(feature = "real-embeddings")]
fn ensure_cross_encoder_files_blocking(model_dir: PathBuf) -> Result<ModelFiles> {
    if model_files_exist(&model_dir) {
        return Ok(model_files_for_dir(model_dir));
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create temporary tokio runtime for cross-encoder model download")?;
    runtime.block_on(ensure_cross_encoder_files_async(model_dir))
}

#[cfg(feature = "real-embeddings")]
pub async fn download_cross_encoder_model() -> Result<PathBuf> {
    let model_dir = default_cross_encoder_dir()?;
    let files = ensure_cross_encoder_files_async(model_dir).await?;
    Ok(files.directory)
}

#[cfg(feature = "real-embeddings")]
async fn ensure_cross_encoder_files_async(model_dir: PathBuf) -> Result<ModelFiles> {
    let files = model_files_for_dir(model_dir);
    if model_files_exist(&files.directory) {
        return Ok(files);
    }

    tokio::fs::create_dir_all(&files.directory)
        .await
        .with_context(|| {
            format!(
                "failed to create cross-encoder model directory {}",
                files.directory.display()
            )
        })?;

    if !tokio::fs::try_exists(&files.model_path)
        .await
        .context("failed to check cross-encoder model.onnx path")?
    {
        super::embedder::download_file(CROSS_ENCODER_MODEL_URL, &files.model_path).await?;
    }
    if !tokio::fs::try_exists(&files.tokenizer_path)
        .await
        .context("failed to check cross-encoder tokenizer.json path")?
    {
        super::embedder::download_file(CROSS_ENCODER_TOKENIZER_URL, &files.tokenizer_path).await?;
    }

    Ok(files)
}

#[cfg(feature = "real-embeddings")]
fn model_files_exist(model_dir: &Path) -> bool {
    let files = model_files_for_dir(model_dir.to_path_buf());
    files.model_path.exists() && files.tokenizer_path.exists()
}

#[cfg(feature = "real-embeddings")]
fn model_files_for_dir(model_dir: PathBuf) -> ModelFiles {
    ModelFiles {
        model_path: model_dir.join("model.onnx"),
        tokenizer_path: model_dir.join("tokenizer.json"),
        directory: model_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sigmoid_zero() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_sigmoid_large_positive() {
        assert!((sigmoid(10.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_sigmoid_large_negative() {
        assert!(sigmoid(-10.0) < 1e-4);
    }

    #[test]
    fn test_sigmoid_monotonic() {
        let a = sigmoid(-1.0);
        let b = sigmoid(0.0);
        let c = sigmoid(1.0);
        assert!(a < b);
        assert!(b < c);
    }
}
