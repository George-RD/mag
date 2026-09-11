use anyhow::Result;
use clap::ValueEnum;
use mag::LocalMemoryRuntime;
use mag::memory_core::embedder::{Embedder, PlaceholderEmbedder};
use mag::memory_core::embedding_model::RetrieverModelProfile;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum EmbedderChoice {
    /// Deterministic 32-dimensional test stand-in, not a quality reference.
    Placeholder,
    /// MAG's shared checksum-verified production BGE adapter.
    #[default]
    BgeSmall,
}

#[derive(Clone)]
pub enum Backend {
    Placeholder(Arc<dyn Embedder>),
    #[cfg(feature = "real-embeddings")]
    Profile(Arc<dyn mag::memory_core::EmbeddingModel>),
}
impl Backend {
    pub fn dimension(&self) -> usize {
        match self {
            Self::Placeholder(e) => e.dimension(),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(m) => m.dimension(),
        }
    }
    pub fn warm_up(&self) -> Result<()> {
        match self {
            Self::Placeholder(e) => e.embed("warm up").map(|_| ()),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(m) => m
                .embed_for(mag::memory_core::EmbeddingInputKind::Document, "warm up")
                .map(|_| ()),
        }
    }
    pub fn profile(&self) -> Option<RetrieverModelProfile> {
        match self {
            Self::Placeholder(_) => None,
            #[cfg(feature = "real-embeddings")]
            Self::Profile(m) => m.model_profile(),
        }
    }
    pub fn open(&self, path: PathBuf) -> Result<LocalMemoryRuntime> {
        match self {
            Self::Placeholder(e) => LocalMemoryRuntime::new_with_path(path, e.clone()),
            #[cfg(feature = "real-embeddings")]
            Self::Profile(m) => Ok(LocalMemoryRuntime::from_storage(
                mag::memory_core::storage::sqlite::SqliteStorage::new_with_path_and_embedding_model(path, m.clone())?)),
        }
    }
}
pub fn build(choice: EmbedderChoice) -> Result<(Backend, String)> {
    match choice {
        EmbedderChoice::Placeholder => Ok((
            Backend::Placeholder(Arc::new(PlaceholderEmbedder)),
            "placeholder".to_string(),
        )),
        #[cfg(feature = "real-embeddings")]
        EmbedderChoice::BgeSmall => Ok((
            Backend::Profile(
                mag::memory_core::embedding_model::bge_small_en_v1_5_embedding_model(Arc::new(
                    mag::memory_core::OnnxEmbedder::new()?,
                ))?,
            ),
            "bge-small-en-v1.5-int8 (production profile)".to_string(),
        )),
        #[cfg(not(feature = "real-embeddings"))]
        EmbedderChoice::BgeSmall => anyhow::bail!(
            "--embedder bge-small requires real-embeddings; rebuild or use --embedder placeholder"
        ),
    }
}

#[cfg(all(test, feature = "real-embeddings"))]
mod tests {
    use super::*;
    #[test]
    fn bge_uses_shared_production_identity() {
        let (backend, _) = build(EmbedderChoice::BgeSmall).unwrap();
        let expected = mag::memory_core::embedding_model::bge_small_en_v1_5_embedding_model(
            Arc::new(mag::memory_core::OnnxEmbedder::new().unwrap()),
        )
        .unwrap();
        assert_eq!(
            backend.profile().unwrap().embedding_space_identity(),
            expected.embedding_space_identity()
        );
    }
}
