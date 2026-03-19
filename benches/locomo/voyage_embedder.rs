use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use mag::memory_core::embedder::Embedder;
use sha2::{Digest, Sha256};

/// Maximum number of texts per Voyage AI embeddings API call.
const BATCH_SIZE: usize = 128;

const EMBEDDING_URL: &str = "https://api.voyageai.com/v1/embeddings";

/// Voyage AI embedder.
///
/// Uses the Voyage AI embeddings API via reqwest. The sync `embed()` trait method
/// bridges to async via a dedicated single-threaded tokio runtime (safe from
/// within `spawn_blocking` contexts where `block_in_place` would panic).
pub(crate) struct VoyageApiEmbedder {
    api_key: String,
    model: String,
    dimension: usize,
    client: reqwest::Client,
    /// Dedicated runtime for bridging sync -> async HTTP calls.
    runtime: tokio::runtime::Runtime,
    /// LRU cache keyed by SHA-256 of input text.
    cache: Mutex<lru::LruCache<[u8; 32], Vec<f32>>>,
}

impl VoyageApiEmbedder {
    pub fn new(api_key: String, model: String, dimension: usize) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .context("failed to build HTTP client for Voyage AI embeddings")?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to create tokio runtime for Voyage AI embedder")?;
        let cache_capacity = std::num::NonZeroUsize::new(4096)
            .ok_or_else(|| anyhow!("cache capacity must be non-zero"))?;
        Ok(Self {
            api_key,
            model,
            dimension,
            client,
            runtime,
            cache: Mutex::new(lru::LruCache::new(cache_capacity)),
        })
    }

    /// Call Voyage AI embeddings API for a batch of texts, returning one vector per text.
    fn call_api(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let model = self.model.clone();
        let dimension = self.dimension;
        self.runtime.block_on(async {
            let mut body = serde_json::json!({
                "model": model,
                "input": texts,
            });
            // Only include output_dimension if it differs from the default (1024).
            if dimension != 1024 {
                body["output_dimension"] = serde_json::json!(dimension);
            }

            let response = self
                .client
                .post(EMBEDDING_URL)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
                .context("Voyage AI embeddings request failed")?;

            if !response.status().is_success() {
                let status = response.status();
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<unreadable>".to_string());
                return Err(anyhow!(
                    "Voyage AI embeddings API returned {status}: {body_text}"
                ));
            }

            let parsed: VoyageEmbeddingResponse = response
                .json()
                .await
                .context("failed to parse Voyage AI embeddings response")?;

            // The API returns embeddings in order of the input index, but we
            // sort by index to be safe.
            let mut items = parsed.data;
            items.sort_by_key(|item| item.index);

            if items.len() != texts.len() {
                return Err(anyhow!(
                    "Voyage AI returned {} embeddings for {} inputs",
                    items.len(),
                    texts.len()
                ));
            }

            Ok(items.into_iter().map(|item| item.embedding).collect())
        })
    }

    fn cache_key(text: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        hasher.finalize().into()
    }
}

impl std::fmt::Debug for VoyageApiEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VoyageApiEmbedder")
            .field("model", &self.model)
            .field("dimension", &self.dimension)
            .finish()
    }
}

impl Embedder for VoyageApiEmbedder {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let key = Self::cache_key(text);
        if let Ok(mut cache) = self.cache.lock()
            && let Some(cached) = cache.get(&key)
        {
            return Ok(cached.clone());
        }

        let mut results = self.call_api(&[text])?;
        let embedding = results
            .pop()
            .ok_or_else(|| anyhow!("empty response from Voyage AI embeddings API"))?;

        if let Ok(mut cache) = self.cache.lock() {
            cache.put(key, embedding.clone());
        }
        Ok(embedding)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        if texts.len() == 1 {
            return Ok(vec![self.embed(texts[0])?]);
        }

        // --- Cache probe: split hits from misses ---
        let keys: Vec<[u8; 32]> = texts.iter().map(|t| Self::cache_key(t)).collect();
        let mut results: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut miss_indices: Vec<usize> = Vec::new();

        if let Ok(mut cache) = self.cache.lock() {
            for (i, key) in keys.iter().enumerate() {
                if let Some(cached) = cache.get(key) {
                    results[i] = Some(cached.clone());
                } else {
                    miss_indices.push(i);
                }
            }
        } else {
            miss_indices.extend(0..texts.len());
        }

        if miss_indices.is_empty() {
            return results
                .into_iter()
                .map(|opt| opt.ok_or_else(|| anyhow!("unexpected None in cache-hit path")))
                .collect();
        }

        // --- Batched API calls for cache misses ---
        let miss_texts: Vec<&str> = miss_indices.iter().map(|&i| texts[i]).collect();
        let mut all_computed: Vec<Vec<f32>> = Vec::with_capacity(miss_texts.len());

        for chunk in miss_texts.chunks(BATCH_SIZE) {
            let batch_result = self.call_api(chunk)?;
            all_computed.extend(batch_result);
        }

        if all_computed.len() != miss_indices.len() {
            return Err(anyhow!(
                "Voyage AI returned {} embeddings for {} miss texts",
                all_computed.len(),
                miss_indices.len()
            ));
        }

        // --- Populate cache and assemble results ---
        if let Ok(mut cache) = self.cache.lock() {
            for (embedding, &orig_idx) in all_computed.iter().zip(miss_indices.iter()) {
                cache.put(keys[orig_idx], embedding.clone());
                results[orig_idx] = Some(embedding.clone());
            }
        } else {
            for (embedding, &orig_idx) in all_computed.into_iter().zip(miss_indices.iter()) {
                results[orig_idx] = Some(embedding);
            }
        }

        results
            .into_iter()
            .map(|opt| opt.ok_or_else(|| anyhow!("unexpected None in batch result")))
            .collect()
    }
}

// ── Voyage AI Embeddings API response types ──────────────────────────────

#[derive(Debug, serde::Deserialize)]
struct VoyageEmbeddingResponse {
    data: Vec<VoyageEmbeddingItem>,
}

#[derive(Debug, serde::Deserialize)]
struct VoyageEmbeddingItem {
    embedding: Vec<f32>,
    index: usize,
}
