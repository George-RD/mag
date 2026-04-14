use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result};

#[cfg(test)]
use rusqlite::{Connection, OptionalExtension};

use crate::memory_core::{
    MemoryInput, MemoryUpdate, ScoringParams, SemanticResult, Storage, Updater, embedder::Embedder,
    reranker::Reranker,
};

use super::cache::{QueryCache, new_query_cache};
use super::conn_pool::ConnPool;
use super::hot_cache::{HOT_CACHE_CAPACITY, HOT_CACHE_REFRESH_SECS, HotTierCache};
use super::schema::default_db_path;

/// Controls how the SQLite storage backend is initialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitMode {
    /// Use the resolved default database path (`~/.mag/memory.db` with legacy fallback).
    Default,
}

#[derive(Debug)]
pub(super) enum StoreOutcome {
    Inserted,
    Deduped,
}

/// SQLite-backed persistent storage for the memory system.
///
/// Uses a connection pool with one writer and N readers (WAL mode) for
/// concurrent read access. The pool is behind `Arc` so the struct can
/// be cloned into both the `Storage` and `Retriever` roles of a [`Pipeline`].
#[derive(Clone)]
pub struct SqliteStorage {
    pub(super) db_path: PathBuf,
    pub(super) pool: Arc<ConnPool>,
    pub(super) embedder: Arc<dyn Embedder>,
    pub(super) scoring_params: ScoringParams,
    pub(super) query_cache: QueryCache,
    pub(super) hot_cache: Option<HotTierCache>,
    pub(super) hot_cache_refresh_guard: Arc<()>,
    pub(super) hot_cache_refresh_started: Arc<AtomicBool>,
    pub(super) reranker: Option<Arc<dyn Reranker>>,
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn ensure_vec_extension_registered() {
    use std::sync::Once;
    static VEC_INIT: Once = Once::new();
    VEC_INIT.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
            *const (),
            unsafe extern "C" fn(
                *mut rusqlite::ffi::sqlite3,
                *mut *mut i8,
                *const rusqlite::ffi::sqlite3_api_routines,
            ) -> i32,
        >(
            sqlite_vec::sqlite3_vec_init as *const ()
        )));
    });
}

impl SqliteStorage {
    /// Creates a new `SqliteStorage` using the given [`InitMode`].
    pub fn new(mode: InitMode, embedder: Arc<dyn Embedder>) -> Result<Self> {
        match mode {
            InitMode::Default => Self::new_default(embedder),
        }
    }

    /// Opens (or creates) the database at the default path.
    pub fn new_default(embedder: Arc<dyn Embedder>) -> Result<Self> {
        let path = default_db_path()?;
        Self::new_with_path(path, embedder)
    }

    /// Opens (or creates) a database at the given `path`, creating parent directories as needed.
    ///
    /// If `path` is `:memory:`, an in-memory single-connection pool is used
    /// (reader pool is skipped because in-memory databases cannot share state
    /// across connections).
    ///
    /// Performs blocking filesystem and SQLite I/O. Call before entering the
    /// async runtime or wrap the call in [`tokio::task::spawn_blocking`].
    pub fn new_with_path(path: PathBuf, embedder: Arc<dyn Embedder>) -> Result<Self> {
        let pool = if path.as_os_str() == ":memory:" {
            ConnPool::open_in_memory(embedder.dimension())?
        } else {
            ConnPool::open_file(&path, embedder.dimension())?
        };

        Ok(Self {
            db_path: path,
            pool: Arc::new(pool),
            embedder,
            scoring_params: ScoringParams::default(),
            query_cache: new_query_cache(),
            hot_cache: Some(HotTierCache::new(
                HOT_CACHE_CAPACITY,
                std::time::Duration::from_secs(HOT_CACHE_REFRESH_SECS),
            )),
            hot_cache_refresh_guard: Arc::new(()),
            hot_cache_refresh_started: Arc::new(AtomicBool::new(false)),
            reranker: None,
        })
    }

    /// Sets the reranker for this storage instance.
    pub fn with_reranker(mut self, reranker: Arc<dyn Reranker>) -> Self {
        self.reranker = Some(reranker);
        self
    }

    /// Returns a reference to the reranker, if configured.
    #[allow(dead_code)]
    pub fn reranker(&self) -> Option<&Arc<dyn Reranker>> {
        self.reranker.as_ref()
    }

    /// Runs `PRAGMA optimize` to update SQLite query planner statistics.
    /// Call periodically (e.g. on shutdown or after large writes).
    pub async fn optimize(&self) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            conn.execute_batch("PRAGMA optimize;")
                .context("failed to run PRAGMA optimize")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")?
    }

    /// Returns a reference to the current scoring parameters.
    #[allow(dead_code)]
    pub fn scoring_params(&self) -> &ScoringParams {
        &self.scoring_params
    }

    #[allow(dead_code)]
    pub fn with_scoring_params(mut self, params: ScoringParams) -> Self {
        self.set_scoring_params(params);
        self
    }

    /// Replaces the scoring parameters on an existing instance.
    ///
    /// Also invalidates the query cache so subsequent searches use the new
    /// parameters.  This is cheaper than rebuilding the entire storage when
    /// only the ranking configuration changes (e.g. during grid search).
    #[allow(dead_code)]
    pub fn set_scoring_params(&mut self, mut params: ScoringParams) {
        if params.graph_seed_min > params.graph_seed_max {
            std::mem::swap(&mut params.graph_seed_min, &mut params.graph_seed_max);
        }
        if !params.rrf_k.is_finite() || params.rrf_k <= 0.0 {
            params.rrf_k = ScoringParams::default().rrf_k;
        }
        self.scoring_params = params;
        self.invalidate_query_cache();
    }

    #[allow(dead_code)]
    pub async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        <Self as Storage>::store(self, id, data, input).await
    }

    #[allow(dead_code)]
    pub async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        <Self as Updater>::update(self, id, input).await
    }

    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        Self::new_in_memory_with_embedder(Arc::new(crate::memory_core::PlaceholderEmbedder))
    }

    #[cfg(test)]
    pub fn new_in_memory_with_embedder(embedder: Arc<dyn Embedder>) -> Result<Self> {
        let pool = ConnPool::open_in_memory(embedder.dimension())?;
        Ok(Self {
            db_path: PathBuf::from(":memory:"),
            pool: Arc::new(pool),
            embedder,
            scoring_params: ScoringParams::default(),
            query_cache: new_query_cache(),
            hot_cache: Some(HotTierCache::new(
                HOT_CACHE_CAPACITY,
                std::time::Duration::from_secs(HOT_CACHE_REFRESH_SECS),
            )),
            hot_cache_refresh_guard: Arc::new(()),
            hot_cache_refresh_started: Arc::new(AtomicBool::new(false)),
            reranker: None,
        })
    }

    /// Returns a guard to the writer connection for test assertions.
    ///
    /// In single-connection (in-memory) mode this is the only connection;
    /// in pooled mode it is the dedicated writer.
    #[cfg(test)]
    pub(super) fn test_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.pool.writer()
    }

    #[cfg(test)]
    pub(super) fn debug_get_last_accessed_at(&self, id: &str) -> Result<String> {
        let conn = self.pool.reader()?;

        let value: Option<String> = conn
            .query_row(
                "SELECT last_accessed_at FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to query last_accessed_at")?;

        value.ok_or_else(|| anyhow::anyhow!("memory not found for id={id}"))
    }

    #[cfg(test)]
    pub(super) fn debug_force_last_accessed_at(&self, id: &str, timestamp: &str) -> Result<()> {
        let conn = self.pool.writer()?;

        conn.execute(
            "UPDATE memories SET last_accessed_at = ?2 WHERE id = ?1",
            rusqlite::params![id, timestamp],
        )
        .context("failed to force last_accessed_at")?;

        Ok(())
    }

    #[cfg(test)]
    pub(super) fn debug_get_access_count(&self, id: &str) -> Result<i64> {
        let conn = self.pool.reader()?;

        let value: Option<i64> = conn
            .query_row(
                "SELECT access_count FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to query access_count")?;

        value.ok_or_else(|| anyhow::anyhow!("memory not found for id={id}"))
    }

    #[cfg(test)]
    pub(super) fn debug_force_access_count(&self, id: &str, access_count: i64) -> Result<()> {
        let conn = self.pool.writer()?;
        conn.execute(
            "UPDATE memories SET access_count = ?2 WHERE id = ?1",
            rusqlite::params![id, access_count],
        )
        .context("failed to force access_count")?;
        if let Some(hot_cache) = &self.hot_cache {
            hot_cache.clear();
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn debug_get_versioning_fields(
        &self,
        id: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        let conn = self.pool.reader()?;

        let value: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT superseded_by_id, version_chain_id FROM memories WHERE id = ?1",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("failed to query versioning fields")?;

        value.ok_or_else(|| anyhow::anyhow!("memory not found for id={id}"))
    }
}

#[derive(Debug, Clone)]
pub struct RankedSemanticCandidate {
    pub result: SemanticResult,
    pub created_at: String,
    /// The `event_at` timestamp for temporal filtering (may differ from `created_at`).
    pub event_at: String,
    pub score: f64,
    /// Resolved priority (from stored value or event-type default), for explain mode.
    pub priority_value: u8,
    #[allow(dead_code)] // Stored for diagnostics; abstention uses collection-level text_overlap
    pub vec_sim: Option<f64>,
    pub text_overlap: f64,
    /// Stored for in-memory filtering and kept in sync with `SemanticResult`.
    pub entity_id: Option<String>,
    /// Stored for in-memory filtering and kept in sync with `SemanticResult`.
    pub agent_type: Option<String>,
    /// Component scores for explain mode; populated only when `SearchOptions.explain` is true.
    pub explain: Option<serde_json::Value>,
}
