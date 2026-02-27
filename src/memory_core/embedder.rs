use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use sha2::{Digest, Sha256};

/// Trait for generating text embeddings.
pub trait Embedder: Send + Sync {
    /// Returns the embedding dimension.
    fn dimension(&self) -> usize;
    /// Generates an embedding vector for the given text.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
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
#[derive(Debug)]
pub struct OnnxEmbedder {
    model_dir: PathBuf,
    runtime: std::sync::OnceLock<OnnxRuntime>,
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
    tokenizer_path: PathBuf,
}

#[cfg(feature = "real-embeddings")]
impl OnnxEmbedder {
    pub fn new() -> Result<Self> {
        Ok(Self {
            model_dir: default_model_dir()?,
            runtime: std::sync::OnceLock::new(),
        })
    }

    fn init_runtime(&self) -> Result<OnnxRuntime> {
        let files = ensure_model_files_blocking(self.model_dir.clone())?;
        let session = ort::session::Session::builder()?
            .with_intra_threads(1)?
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
        384
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Try to get the cached runtime, or initialize it.
        // If initialization fails, we DON'T cache the error so retries can succeed.
        let runtime = match self.runtime.get() {
            Some(rt) => rt,
            None => {
                let rt = self.init_runtime()?;
                // Ignore set errors (another thread may have set it concurrently)
                let _ = self.runtime.set(rt);
                self.runtime
                    .get()
                    .ok_or_else(|| anyhow!("failed to retrieve runtime after initialization"))?
            }
        };

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
        let token_type_ids = vec![0_i64; seq_len];
        let input_ids_value = ort::value::Value::from_array(([1_usize, seq_len], input_ids))
            .context("failed to create ONNX input_ids value")?;
        let token_type_ids_value =
            ort::value::Value::from_array(([1_usize, seq_len], token_type_ids))
                .context("failed to create ONNX token_type_ids value")?;
        let attention_mask_value =
            ort::value::Value::from_array(([1_usize, seq_len], attention_mask))
                .context("failed to create ONNX attention_mask value")?;

        let mut session = runtime
            .session
            .lock()
            .map_err(|_| anyhow!("onnx session mutex poisoned"))?;
        let outputs = session
            .run(ort::inputs![
                input_ids_value,
                attention_mask_value,
                token_type_ids_value
            ])
            .context("ONNX inference failed")?;
        let first_output = outputs
            .get("last_hidden_state")
            .ok_or_else(|| anyhow!("missing ONNX output tensor 'last_hidden_state'"))?;
        let (shape, output) = first_output
            .try_extract_tensor::<f32>()
            .context("failed to extract ONNX output tensor")?;

        if shape.len() != 3 || shape[0] != 1 {
            return Err(anyhow!("unexpected ONNX output shape: {shape:?}"));
        }
        let output_seq_len = usize::try_from(shape[1]).context("invalid output sequence length")?;
        let hidden_size = usize::try_from(shape[2]).context("invalid output hidden size")?;
        if hidden_size != self.dimension() {
            return Err(anyhow!(
                "unexpected embedding dimension: got {hidden_size}, expected {}",
                self.dimension()
            ));
        }
        if output_seq_len == 0 {
            return Err(anyhow!("ONNX output sequence length is zero"));
        }

        let effective_len = output_seq_len.min(seq_len);
        let mut pooled = vec![0.0f32; hidden_size];
        let mut mask_sum = 0.0f32;

        for token_idx in 0..effective_len {
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
        Ok(pooled)
    }
}

#[cfg(feature = "real-embeddings")]
pub async fn download_bge_small_model() -> Result<PathBuf> {
    let model_dir = default_model_dir()?;
    let files = ensure_model_files_async(model_dir).await?;
    Ok(files.directory)
}

#[cfg(feature = "real-embeddings")]
fn default_model_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            anyhow!("neither HOME nor USERPROFILE is set — cannot resolve model directory")
        })?;
    Ok(PathBuf::from(home)
        .join(".romega-memory")
        .join("models")
        .join(MODEL_NAME))
}

#[cfg(feature = "real-embeddings")]
fn ensure_model_files_blocking(model_dir: PathBuf) -> Result<ModelFiles> {
    if model_files_exist(&model_dir) {
        return Ok(model_files_for_dir(model_dir));
    }

    // Create a dedicated single-threaded runtime for model download.
    // We avoid block_in_place because embed() runs inside spawn_blocking
    // threads where block_in_place panics. A lightweight current-thread
    // runtime is safe and sufficient for the download I/O.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create temporary tokio runtime for model download")?;
    runtime.block_on(ensure_model_files_async(model_dir))
}

#[cfg(feature = "real-embeddings")]
async fn ensure_model_files_async(model_dir: PathBuf) -> Result<ModelFiles> {
    let files = model_files_for_dir(model_dir);
    if model_files_exist(&files.directory) {
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
        download_file(MODEL_URL, &files.model_path).await?;
    }
    if !tokio::fs::try_exists(&files.tokenizer_path)
        .await
        .context("failed to check tokenizer.json path")?
    {
        download_file(TOKENIZER_URL, &files.tokenizer_path).await?;
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

#[cfg(feature = "real-embeddings")]
async fn download_file(url: &str, path: &Path) -> Result<()> {
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
}
