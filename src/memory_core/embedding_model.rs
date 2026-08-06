use std::sync::Arc;

use anyhow::Result;

use super::embedder::Embedder;

/// Identifies how text participates in retrieval.
///
/// Callers pass semantic intent only. Model-specific prefixes, prompts, and
/// tokenization differences belong inside the embedding model adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmbeddingInputKind {
    /// Text used to search an existing embedding space.
    Query,
    /// Text persisted into the embedding space.
    Document,
}

/// Production embedding boundary for retrieval models.
///
/// Models whose contract distinguishes queries from documents implement the
/// role-specific methods directly. Role-neutral embedders are adapted through
/// [`LegacyEmbedderAdapter`] while callers migrate to this explicit boundary.
pub trait EmbeddingModel: Send + Sync {
    /// Returns the embedding dimension.
    fn dimension(&self) -> usize;

    /// Generates an embedding for an explicit retrieval input kind.
    fn embed_for(&self, input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>>;

    /// Generates embeddings for an explicit retrieval input kind.
    ///
    /// The correctness-first default delegates each item to [`Self::embed_for`]
    /// so a role-sensitive implementation cannot accidentally bypass its query
    /// or document transformation. Batched backends should override this method.
    fn embed_batch_for(
        &self,
        input: EmbeddingInputKind,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        texts
            .iter()
            .map(|text| self.embed_for(input, text))
            .collect()
    }
}

/// Compatibility adapter for the role-neutral [`Embedder`] interface.
///
/// It preserves optimized legacy batch inference while the production storage
/// boundary carries explicit query/document intent.
pub(crate) struct LegacyEmbedderAdapter {
    inner: Arc<dyn Embedder>,
}

impl LegacyEmbedderAdapter {
    pub(crate) fn new(inner: Arc<dyn Embedder>) -> Self {
        Self { inner }
    }
}

impl EmbeddingModel for LegacyEmbedderAdapter {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embed_for(&self, _input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>> {
        self.inner.embed(text)
    }

    fn embed_batch_for(
        &self,
        _input: EmbeddingInputKind,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }
}
