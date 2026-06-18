use crate::memory_core::embedder::Embedder;
#[cfg(feature = "llm")]
use crate::substrate::extraction::LlmExtractor;
use crate::substrate::traits::{IngestionPipeline, MemoryStore};
use crate::substrate::types::WriteContext;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

/// Write-path pipeline: computes embedding, performs dedup/supersession checks,
/// extracts entities (rule-based; optionally LLM-enhanced), and stores.
pub struct EmbedAndExtractPipeline {
    pub embedder: Arc<dyn Embedder>,
    /// Optional LLM backend for advanced extraction (facts, entities, relationships).
    /// When None, falls back to rule-based extraction performed by the store.
    #[cfg(feature = "llm")]
    pub llm: Option<Arc<dyn crate::memory_core::llm::LlmBackend>>,
}

impl EmbedAndExtractPipeline {
    pub fn new(embedder: Arc<dyn Embedder>) -> Self {
        Self {
            embedder,
            #[cfg(feature = "llm")]
            llm: None,
        }
    }

    /// Attach an LLM backend for advanced extraction.
    #[cfg(feature = "llm")]
    pub fn with_llm(mut self, llm: Arc<dyn crate::memory_core::llm::LlmBackend>) -> Self {
        self.llm = Some(llm);
        self
    }
}

#[async_trait]
impl IngestionPipeline for EmbedAndExtractPipeline {
    async fn ingest(&self, mut ctx: WriteContext, store: &dyn MemoryStore) -> Result<String> {
        let id = if ctx.assigned_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            ctx.assigned_id
        };

        let embedding = if let Some(embedding) = ctx.embedding.take() {
            embedding
        } else {
            let content = ctx.input.content.clone();
            let embedder = Arc::clone(&self.embedder);
            tokio::task::spawn_blocking(move || embedder.embed(&content))
                .await
                .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e:?}"))??
        };
        #[cfg(feature = "llm")]
        if let Some(ref llm_backend) = self.llm {
            let extractor = LlmExtractor {
                llm: Arc::clone(llm_backend),
            };
            match extractor.extract(&ctx.input.content).await {
                Ok(result) => {
                    use std::collections::HashSet;
                    let mut tag_set: HashSet<String> =
                        HashSet::with_capacity(ctx.input.tags.len() + 30);
                    for tag in &ctx.input.tags {
                        tag_set.insert(tag.clone());
                    }
                    for tag in result.entity_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.fact_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.relationship_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.temporal_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.topic_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.sentiment_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.action_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.location_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.decision_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.question_tags() {
                        tag_set.insert(tag);
                    }
                    for tag in result.status_tags() {
                        tag_set.insert(tag);
                    }
                    ctx.input.tags = tag_set.into_iter().collect();
                    if let Ok(meta) = serde_json::to_value(&result) {
                        ctx.input.metadata["llm_extraction"] = meta;
                    }
                    if ctx.input.referenced_date.is_none()
                        && let Some(date) = result.best_temporal_date()
                    {
                        ctx.input.referenced_date = Some(date.to_string());
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "LLM extraction failed, falling back to rule-based");
                }
            }
        }

        store
            .store_with_embedding(&id, &ctx.input.content, &ctx.input, embedding)
            .await?;

        Ok(id)
    }
}
