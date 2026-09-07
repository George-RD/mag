use std::{fmt::Write as _, sync::Arc};

use anyhow::{Result, ensure};

use super::embedder::Embedder;
#[cfg(feature = "real-embeddings")]
use super::embedder::{
    BGE_SMALL_EN_V1_5_MODEL_ID, BGE_SMALL_EN_V1_5_MODEL_SHA256, BGE_SMALL_EN_V1_5_REVISION,
    BGE_SMALL_EN_V1_5_TOKENIZER_SHA256, OnnxEmbedder,
};

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

    /// Returns validated immutable model metadata when this adapter has a
    /// pinned retriever profile.
    ///
    /// Compatibility-only adapters return `None` rather than fabricating
    /// revision or checksum metadata.
    fn model_profile(&self) -> Option<RetrieverModelProfile> {
        None
    }

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

/// One immutable artifact checksum recorded by a retriever model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrieverArtifactChecksum {
    pub artifact: &'static str,
    pub sha256: &'static str,
}

/// Expected local footprint for a retriever model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalResourceExpectations {
    pub model_disk_bytes: u64,
    pub peak_ram_bytes: u64,
}

/// Complete metadata used to construct an immutable retriever model profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrieverModelProfileSpec {
    pub model_id: &'static str,
    pub revision: &'static str,
    pub checksums: &'static [RetrieverArtifactChecksum],
    pub role: &'static str,
    pub runtime: &'static str,
    pub quantization: &'static str,
    pub output_dimensions: usize,
    pub pooling: &'static str,
    pub query_transform: &'static str,
    pub document_transform: &'static str,
    pub max_input_tokens: usize,
    pub licence: &'static str,
    pub local_resources: LocalResourceExpectations,
}

/// Validated, immutable contract shared by dense encoders and rerankers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetrieverModelProfile {
    spec: RetrieverModelProfileSpec,
}

impl RetrieverModelProfile {
    pub fn new(spec: RetrieverModelProfileSpec) -> Result<Self> {
        let profile = Self { spec };
        profile.validate()?;
        Ok(profile)
    }

    pub const fn metadata(&self) -> RetrieverModelProfileSpec {
        self.spec
    }

    /// Stable identity for semantics that determine persisted vector values.
    ///
    /// Runtime choice, licence text, resource estimates, and artifact-list
    /// ordering are intentionally excluded: changing those does not make
    /// otherwise identical vectors incomparable and must not force migration.
    pub fn embedding_space_identity(&self) -> String {
        let spec = self.metadata();
        let mut identity = String::from("retriever-profile:v1");
        push_identity_component(&mut identity, "model", spec.model_id);
        push_identity_component(&mut identity, "revision", spec.revision);
        push_identity_component(&mut identity, "role", spec.role);

        let mut checksums: Vec<_> = spec.checksums.iter().collect();
        checksums.sort_unstable_by(|left, right| {
            left.artifact
                .cmp(right.artifact)
                .then_with(|| left.sha256.cmp(right.sha256))
        });
        for checksum in checksums {
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
            ensure!(
                !value.trim().is_empty(),
                "retriever profile {name} is empty"
            );
        }
        ensure!(
            matches!(spec.role, "dense-embedding" | "cross-encoder-reranker"),
            "unsupported retriever profile role: {}",
            spec.role
        );
        ensure!(
            spec.output_dimensions > 0,
            "retriever profile output_dimensions must be greater than zero"
        );
        ensure!(
            !spec.checksums.is_empty(),
            "retriever profile must record at least one artifact checksum"
        );
        ensure!(
            spec.max_input_tokens > 0,
            "retriever profile must record max_input_tokens"
        );
        ensure!(
            spec.local_resources.model_disk_bytes > 0,
            "retriever profile must record expected model disk bytes"
        );
        ensure!(
            spec.local_resources.peak_ram_bytes > 0,
            "retriever profile must record expected peak RAM bytes"
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

        Ok(())
    }
}

#[cfg(any(feature = "real-embeddings", test))]
struct ProfiledEmbedderAdapter {
    inner: Arc<dyn Embedder>,
    profile: RetrieverModelProfile,
    embedding_space_identity: String,
}

#[cfg(any(feature = "real-embeddings", test))]
impl ProfiledEmbedderAdapter {
    fn new(inner: Arc<dyn Embedder>, profile: RetrieverModelProfile) -> Result<Self> {
        let metadata = profile.metadata();
        ensure!(
            metadata.role == "dense-embedding",
            "profile-backed embedder requires a dense-embedding profile"
        );
        ensure!(
            inner.dimension() == metadata.output_dimensions,
            "profile output dimension {} does not match embedder dimension {}",
            metadata.output_dimensions,
            inner.dimension()
        );
        let embedding_space_identity = profile.embedding_space_identity();
        Ok(Self {
            inner,
            profile,
            embedding_space_identity,
        })
    }
}

#[cfg(any(feature = "real-embeddings", test))]
impl EmbeddingModel for ProfiledEmbedderAdapter {
    fn dimension(&self) -> usize {
        self.inner.dimension()
    }

    fn embedding_space_identity(&self) -> &str {
        &self.embedding_space_identity
    }

    fn model_profile(&self) -> Option<RetrieverModelProfile> {
        Some(self.profile)
    }

    fn embed_for(&self, _input: EmbeddingInputKind, text: &str) -> Result<Vec<f32>> {
        self.inner.embed(text)
    }

    fn embed_batch_for(&self, _input: EmbeddingInputKind, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.inner.embed_batch(texts)
    }
}

#[cfg(feature = "real-embeddings")]
const BGE_SMALL_EN_V1_5_CHECKSUMS: [RetrieverArtifactChecksum; 2] = [
    RetrieverArtifactChecksum {
        artifact: "onnx/model_int8.onnx",
        sha256: BGE_SMALL_EN_V1_5_MODEL_SHA256,
    },
    RetrieverArtifactChecksum {
        artifact: "tokenizer.json",
        sha256: BGE_SMALL_EN_V1_5_TOKENIZER_SHA256,
    },
];

#[cfg(feature = "real-embeddings")]
fn bge_small_en_v1_5_profile() -> Result<RetrieverModelProfile> {
    RetrieverModelProfile::new(RetrieverModelProfileSpec {
        model_id: BGE_SMALL_EN_V1_5_MODEL_ID,
        revision: BGE_SMALL_EN_V1_5_REVISION,
        checksums: &BGE_SMALL_EN_V1_5_CHECKSUMS,
        role: "dense-embedding",
        runtime: "onnx-runtime-cpu",
        quantization: "int8",
        output_dimensions: 384,
        pooling: "attention-mask-mean+l2-normalize:v1",
        query_transform: "identity:v1",
        document_transform: "identity:v1",
        max_input_tokens: 512,
        licence: "apache-2.0",
        local_resources: LocalResourceExpectations {
            model_disk_bytes: 34_472_227,
            peak_ram_bytes: 180_000_000,
        },
    })
}

/// Wraps MAG's pinned production ONNX embedder in its immutable model profile.
///
/// The adapter deliberately preserves the incumbent role-neutral text handling;
/// the profile records that behavior explicitly so future transformation changes
/// produce a different embedding-space identity and require migration.
#[cfg(feature = "real-embeddings")]
pub fn bge_small_en_v1_5_embedding_model(
    embedder: Arc<OnnxEmbedder>,
) -> Result<Arc<dyn EmbeddingModel>> {
    ensure!(
        embedder.uses_pinned_bge_artifacts(),
        "pinned BGE profile requires the default checksum-verified model; custom ONNX models need their own profile"
    );
    let embedder: Arc<dyn Embedder> = embedder;
    Ok(Arc::new(ProfiledEmbedderAdapter::new(
        embedder,
        bge_small_en_v1_5_profile()?,
    )?))
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

fn push_identity_component(identity: &mut String, name: &str, value: &str) {
    write!(identity, ";{name}={}:{}", value.len(), value)
        .expect("writing retriever identity to String cannot fail");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::{
        embedder::PlaceholderEmbedder,
        reranker::{NoOpReranker, Reranker},
    };

    const MODEL_CHECKSUMS: [RetrieverArtifactChecksum; 1] = [RetrieverArtifactChecksum {
        artifact: "model.onnx",
        sha256: "0000000000000000000000000000000000000000000000000000000000000000",
    }];
    const ORDERED_CHECKSUMS: [RetrieverArtifactChecksum; 2] = [
        RetrieverArtifactChecksum {
            artifact: "model.onnx",
            sha256: "0000000000000000000000000000000000000000000000000000000000000000",
        },
        RetrieverArtifactChecksum {
            artifact: "tokenizer.json",
            sha256: "1111111111111111111111111111111111111111111111111111111111111111",
        },
    ];
    const REORDERED_CHECKSUMS: [RetrieverArtifactChecksum; 2] =
        [ORDERED_CHECKSUMS[1], ORDERED_CHECKSUMS[0]];

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

        let unsupported_role =
            RetrieverModelProfile::new(production_profile_spec("late-interaction"));
        assert!(unsupported_role.is_err());

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
    fn embedding_space_identity_ignores_artifact_checksum_order() {
        let ordered = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            checksums: &ORDERED_CHECKSUMS,
            ..production_profile_spec("dense-embedding")
        })
        .expect("ordered profile should validate");
        let reordered = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            checksums: &REORDERED_CHECKSUMS,
            ..production_profile_spec("dense-embedding")
        })
        .expect("reordered profile should validate");

        assert_eq!(
            ordered.embedding_space_identity(),
            reordered.embedding_space_identity()
        );
    }

    #[test]
    fn profiled_embedder_exposes_validated_identity_and_preserves_role_neutral_behavior() {
        let profile = RetrieverModelProfile::new(RetrieverModelProfileSpec {
            output_dimensions: 32,
            query_transform: "identity:v1",
            document_transform: "identity:v1",
            ..production_profile_spec("dense-embedding")
        })
        .expect("profile should validate");
        let model = ProfiledEmbedderAdapter::new(Arc::new(PlaceholderEmbedder), profile)
            .expect("profile dimension should match the embedder");

        assert_eq!(model.model_profile(), Some(profile));
        assert_eq!(
            model.embedding_space_identity(),
            profile.embedding_space_identity()
        );
        assert_eq!(
            model
                .embed_for(EmbeddingInputKind::Query, "same text")
                .expect("query embedding should succeed"),
            model
                .embed_for(EmbeddingInputKind::Document, "same text")
                .expect("document embedding should succeed")
        );
    }

    #[test]
    fn profiled_embedder_rejects_dimension_mismatch() {
        let profile = RetrieverModelProfile::new(production_profile_spec("dense-embedding"))
            .expect("profile should validate");
        let result = ProfiledEmbedderAdapter::new(Arc::new(PlaceholderEmbedder), profile);

        assert!(result.is_err());
    }

    #[cfg(feature = "real-embeddings")]
    #[test]
    fn production_bge_profile_is_pinned_to_verified_artifacts() {
        crate::test_helpers::with_temp_home(|_| {
            let embedder = Arc::new(
                OnnxEmbedder::new().expect("production ONNX embedder should be constructed"),
            );
            let model = bge_small_en_v1_5_embedding_model(embedder)
                .expect("production embedder should be profile-backed");
            let profile = model
                .model_profile()
                .expect("production model should expose its profile");
            let metadata = profile.metadata();

            assert_eq!(metadata.model_id, BGE_SMALL_EN_V1_5_MODEL_ID);
            assert_eq!(metadata.revision, BGE_SMALL_EN_V1_5_REVISION);
            assert_eq!(metadata.checksums, &BGE_SMALL_EN_V1_5_CHECKSUMS);
            assert_eq!(
                model.embedding_space_identity(),
                profile.embedding_space_identity()
            );
        });
    }

    #[test]
    fn compatibility_adapters_do_not_claim_validated_profiles() {
        let embedder = LegacyEmbedderAdapter::new(Arc::new(PlaceholderEmbedder));
        assert!(embedder.model_profile().is_none());

        let reranker = NoOpReranker;
        assert!(reranker.model_profile().is_none());
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
