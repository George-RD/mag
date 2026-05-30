pub mod app_paths;
pub mod benchmarking;
pub(crate) mod config_writer;
#[cfg(feature = "daemon-http")]
pub mod daemon;
pub mod harness;
pub mod memory_core;
pub mod setup;
#[cfg(feature = "substrate")]
pub mod substrate;
pub(crate) mod tool_detection;
pub mod uninstall;

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

#[cfg(not(test))]
impl memory_core::storage::sqlite::SqliteStorage {
    pub fn new_in_memory() -> anyhow::Result<Self> {
        Self::new_in_memory_with_embedder(std::sync::Arc::new(memory_core::PlaceholderEmbedder))
    }

    pub fn new_in_memory_with_embedder(
        embedder: std::sync::Arc<dyn memory_core::embedder::Embedder>,
    ) -> anyhow::Result<Self> {
        Self::new_with_path(std::path::PathBuf::from(":memory:"), embedder)
    }
}
