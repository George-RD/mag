use std::{fmt::Write as _, sync::Arc};

use anyhow::{Result, ensure};

use super::embedder::Embedder;

/// One immutable artifact checksum recorded by a retriever model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetrieverArtifactChecksum {
    pub(super) artifact: &'static str,
    pub(super) sha256: &'static str,
}

/// Expected local footprint for a retriever model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LocalResourceExpectations {
    pub(super) model_disk_bytes: u64,
    pub(super) peak_ram_bytes: u64,
}

/// Complete metadata used to construct an immutable retriever model profile.
///
/// Production profiles must fill every field. Compatibility adapters may mark
/// themselves as not production-ready while they are being retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetrieverModelProfileSpec {
    pub(super) model_id: &'static str,
    pub(super) revision: &'static str,
    pub(super) checksums: &'static [RetrieverArtifactChecksum],
    pub(super) role: &'static str,
    pub(super) runtime: &'static str,
    pub(super) quantization: &'static str,
    pub(super) output_dimensions: usize,
    pub(super) pooling: &'static str,
    pub(super) query_transform: &'static str,
    pub(super) document_transform: &'static str,
    pub(super) max_input_tokens: usize,
    pub(super) licence: &'static str,
    pub(super) local_resources: LocalResourceExpectations,
    pub(super) production_ready: bool,
}

/// Validated, immutable contract shared by dense encoders and rerankers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetrieverModelProfile {
    spec: RetrieverModelProfileSpec,
}

impl RetrieverModelProfile {
    pub(super) fn new(spec: RetrieverModelProfileSpec) -> Result<Self> {
        let profile = Self { spec };
        profile.validate()?;
        Ok(profile)
    }

    pub(super) const fn metadata(&self) -> RetrieverModelProfileSpec {
        self.spec
    }

    /// Stable identity for semantics that determine persisted vector values.
    ///
    /// Runtime choice, licence text, and resource estimates are intentionally
    /// excluded: changing those does not make otherwise identical vectors
    /// incomparable and therefore must not force a re-embedding migration.
    pub(super) fn embedding_space_identity(&self) -> String {
        let spec = self.metadata();
        let mut identity = String::from("retriever-profile:v1");
        push_identity_component(&mut identity, "model", spec.model_id);
        push_identity_component(&mut identity, "revision", spec.revision);
        push_identity_component(&mut identity, "role", spec.role);
        for checksum in spec.checksums {
            push_identity_component(&mut identity, "artifact", checksum.artifact);
            push_identity_component(&mut identity, "sha256", checksum.sha256);
        }
        push_identity_component(&mut identity, "quantization", spec.quantization);
        push_identity_component(
            &mut identity,
            "dimensions",
            &spec.output_dimensions.to_string(),
        );
        push_identity_component(&mut identity, "pooling", spec.pooling);
        push_identity_component(&mut identity, "query", spec.query_transform);
        push_identity_component(&mut identity, "document", spec.document_transform);
        push_identity_component(
            &mut identity,
            "max_input_tokens",
            &spec.max_input_tokens.to_string(),
        );
        identity
    }

    fn validate(&self) -> Result<()> {
        let spec = self.metadata();
        for (name, value) in [
            ("model_id", spec.model_id),
            ("revision", spec.revision),
            ("role", spec.role),
            ("runtime", spec.runtime),
            ("quantization", spec.quantization),
            ("pooling", spec.pooling),
            ("query_transform", spec.query_transform),
            ("document_transform", spec.document_transform),
            ("licence", spec.licence),
        ] {
            ensure!(!value.trim().is_empty(), "retriever profile {name} is empty");
        }
        ensure!(
            spec.output_dimensions > 0,
            "retriever profile output_dimensions must be greater than zero"
        );

        for checksum in spec.checksums {
            ensure!(
                !checksum.artifact.trim().is_empty(),
                "retriever profile checksum artifact is empty"
            );
            ensure!(
                checksum.sha256.len() == 64
                    && checksum.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "retriever profile checksum for {} is not a SHA-256 digest",
                checksum.artifact
            );
        }

        if spec.production_ready {
            ensure!(
                !spec.checksums.is_empty(),
                "production retriever profile must record at least one artifact checksum"
            );
            ensure!(
                spec.max_input_tokens > 0,
                "production retriever profile must record max_input_tokens"
            );
            ensure!(
                spec.local_resources.model_disk_bytes > 0,
                "production retriever profile must record expected model disk bytes"
            );
            ensure!(
                spec.local_resources.peak_ram_bytes > 0,
                "production retriever profile must record expected peak RAM bytes"
            );
            ensure!(
                !self.embedding_space_identity().is_empty(),
                "production retriever profile must have an embedding-space identity"
            );
        }

        Ok(())
    }

    fn legacy_role_neutral(dimension: usize) -> Result<Self> {
        Self::new(RetrieverModelProfileSpec {
            model_id: "legacy-role-neutral",
            revision: "v1",
            checksums: &[],
            role: "dense-embedding",
            runtime: "compatibility-adapter",
            quantization: "unknown",
            output_dimensions: dimension,
            pooling: "legacy-role-neutral",
            query_transform: "identity",
            document_transform: "identity",
            max_input_tokens: 0,
            licence: "unknown",
            local_resources: LocalResourceExpectations {
                model_disk_bytes: 0,
                peak_ram_bytes: 0,
            },
            production_ready: false,
        })
    }
}

fn push_identity_component(identity: &mut String, name: &str, value: &str) {
    write!(identity, ";{name}={}:{}", value.len(), value)
        .expect("writing retriever identity to String cannot fail");
}

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
        let profile = RetrieverModelProfile::legacy_role_neutral(inner.dimension())
            .expect("built-in legacy retriever profile must be valid");
        let embedding_space_identity = format!(
            "legacy-role-neutral:v1:dim={}",
            profile.metadata().output_dimensions
        );
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
