use crate::memory_core::embedder::Embedder;
use crate::substrate::traits::{IngestionPipeline, MemoryStore};
use crate::substrate::types::WriteContext;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub struct EmbedAndExtractPipeline {
    pub embedder: Arc<dyn Embedder>,
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

        // TODO: The embedding is computed here but MemoryStore::store does not accept
        // a pre-computed embedding, so it is recomputed inside SqliteStorage::store.
        // Extend MemoryStore with a store_with_embedding method to avoid double work.
        let _embedding = tokio::task::spawn_blocking(move || embedder.embed(&content))
            .await
            .map_err(|e| anyhow::anyhow!("spawn_blocking join error: {e:?}"))??;

        store.store(&id, &ctx.input.content, &ctx.input).await?;

        Ok(id)
    }
}
