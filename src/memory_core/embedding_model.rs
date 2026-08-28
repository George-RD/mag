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

    /// Returns the stable identity of the persisted embedding space.
    ///
    /// This identity must change whenever stored vectors would no longer be
    /// comparable, including model, revision, quantization, pooling, output
    /// dimension, or query/document transformation changes.
    fn embedding_space_identity(&self) -> &str;

    /// Generates an embedding for an explicit retrieval input kind.
    fn embed_for(&self, input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>>;

    /// Generates embeddings for an explicit retrieval input kind.
    ///
    /// The correctness-first default delegates each item to [`Self::embed_for`]
    /// so a role-sensitive implementation cannot accidentally bypass its query
    /// or document transformation. Batched backends should override this method.
    fn embed_batch_for(&self, input: EmbeddingInputKind, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
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
    embedding_space_identity: String,
}

impl LegacyEmbedderAdapter {
    pub(crate) fn new(inner: Arc<dyn Embedder>) -> Self {
        let embedding_space_identity = format!("legacy-role-neutral:v1:dim={}", inner.dimension());
        Self {
            inner,
            embedding_space_identity,
        }
    }
}

impl EmbeddingModel for LegacyEmbedderAdapter {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embedding_space_identity(&self) -> &str {
        &self.embedding_space_identity
    }

    fn embed_for(&self, _input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>> {
        self.inner.embed(text)
    }

    fn embed_batch_for(&self, _input: EmbeddingInputKind, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::embedder::PlaceholderEmbedder;

    const MODEL_CHECKSUMS: [RetrieverArtifactChecksum; 1] = [RetrieverArtifactChecksum {
        artifact: "model.onnx",
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
    }];

    fn production_profile_spec(role: &'static str) -> RetrieverModelProfileSpec {
        RetrieverModelProfileSpec {
            model_id: "example/retriever",
            revision: "0123456789abcdef",
            checksums: &MODEL_CHECKSUMS,
            role,
            runtime: "onnx-cpu",
            quantization: "int8",
            output_dimensions: 384,
            pooling: "mean-l2",
            query_transform: "query-prefix:v1",
            document_transform: "document-prefix:v1",
            max_input_tokens: 512,
            licence: "apache-2.0",
            local_resources: LocalResourceExpectations {
                model_disk_bytes: 32_000_000,
                peak_ram_bytes: 160_000_000,
            },
            production_ready: true,
        }
    }

    #[test]
    fn production_retriever_profiles_require_complete_metadata() {
        let dense = RetrieverModelProfile::new(production_profile_spec("dense-embedding"));
        assert!(dense.is_ok());

        let reranker = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            role: "cross-encoder-reranker",
            output_dimensions: 1,
            query_transform: "pair-query:v1",
            document_transform: "pair-document:v1",
            ..production_profile_spec("cross-encoder-reranker")
        });
        assert!(reranker.is_ok());

        let missing_checksum = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            checksums: &[],
            ..production_profile_spec("dense-embedding")
        });
        assert!(missing_checksum.is_err());

        let missing_resources = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            local_resources: LocalResourceExpectations {
                model_disk_bytes: 0,
                peak_ram_bytes: 0,
            },
            ..production_profile_spec("dense-embedding")
        });
        assert!(missing_resources.is_err());
    }

    #[test]
    fn embedding_space_identity_tracks_vector_semantics_not_operational_metadata() {
        let base = RetrieverModelProfile::new(production_profile_spec("dense-embedding"))
            .expect("complete dense profile should validate");
        let operational_change = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            runtime: "onnx-gpu",
            licence: "mit",
            local_resources: LocalResourceExpectations {
                model_disk_bytes: 64_000_000,
                peak_ram_bytes: 320_000_000,
            },
            ..production_profile_spec("dense-embedding")
        })
        .expect("operational metadata change should remain a valid profile");
        let vector_change = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            quantization: "fp16",
            ..production_profile_spec("dense-embedding")
        })
        .expect("vector-semantic change should remain a valid profile");

        assert_eq!(
            base.embedding_space_identity(),
            operational_change.embedding_space_identity()
        );
        assert_ne!(
            base.embedding_space_identity(),
            vector_change.embedding_space_identity()
        );
    }

    #[test]
    fn legacy_adapter_preserves_existing_embedding_space_identity() {
        let adapter = LegacyEmbedderAdapter::new(Arc::new(PlaceholderEmbedder));
        assert_eq!(
            adapter.embedding_space_identity(),
            "legacy-role-neutral:v1:dim=32"
        );
    }
}
