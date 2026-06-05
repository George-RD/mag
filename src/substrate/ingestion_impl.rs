use crate::memory_core::embedder::Embedder;
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
    async fn ingest(&self, ctx: WriteContext, store: &dyn MemoryStore) -> Result<String> {
        let id = if ctx.assigned_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            ctx.assigned_id
        };

        let content = ctx.input.content.clone();
        let embedder = Arc::clone(&self.embedder);

        let embedding = tokio::task::spawn_blocking(move || embedder.embed(&content))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e:?}"))??;

        #[cfg(feature = "llm")]
        if let Some(ref _llm) = self.llm {
            // Phase 2: LLM-powered extraction (facts, entities, relationships, temporal).
            // For now, the store performs rule-based entity extraction.
            // TODO: Use LLM to enrich tags/metadata before storing.
        }

        store
            .store_with_embedding(&id, &ctx.input.content, &ctx.input, embedding)
            .await?;

        Ok(id)
    }
}
