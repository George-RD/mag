use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, Transaction};

use super::schema::{
    initialize_schema, read_embedding_generation, verified_embedding_generation,
    verify_embedding_space_identity,
};

/// Returns `true` when a rusqlite error indicates SQLite lock contention
/// (`SQLITE_BUSY` or `SQLITE_LOCKED`). Used to decide whether to retry a transaction.
pub(super) fn is_lock_error(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == rusqlite::ffi::ErrorCode::DatabaseBusy
                || e.code == rusqlite::ffi::ErrorCode::DatabaseLocked
    )
}

/// Maximum number of retries after the initial call (so total calls is
/// `RETRY_MAX_RETRIES + 1`).
const RETRY_MAX_RETRIES: u32 = 5;

/// Base delay between retries (doubled each attempt).
const RETRY_BASE_DELAY_MS: u64 = 10;

/// Retries `f` with exponential backoff when it returns a lock contention error.
///
/// This is a **synchronous** function intended to run inside `spawn_blocking`.
/// Uses `std::thread::sleep` (not tokio) for delays.
///
/// - Max retries: 5 (so up to 6 calls including the initial attempt)
/// - Backoff: 10ms, 20ms, 40ms, 80ms, 160ms (+ random 0-50% jitter)
/// - Non-lock errors are returned immediately without retry.
pub(super) fn retry_on_lock<T, F>(mut f: F) -> std::result::Result<T, rusqlite::Error>
where
    F: FnMut() -> std::result::Result<T, rusqlite::Error>,
{
    let mut retries = 0_u32;
    loop {
        match f() {
            Ok(val) => return Ok(val),
            Err(err) if is_lock_error(&err) && retries < RETRY_MAX_RETRIES => {
                let base_ms = RETRY_BASE_DELAY_MS * 2_u64.pow(retries);
                // Simple jitter: add 0-50% of base_ms using a cheap hash of the
                // retry counter and a timestamp nanos component to avoid pulling
                // in a rand dependency.
                let jitter_ms = {
                    let nanos = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos() as u64)
                        .unwrap_or(0);
                    let seed = nanos.wrapping_mul(u64::from(retries).wrapping_add(7));
                    seed % (base_ms / 2 + 1)
                };
                let delay = Duration::from_millis(base_ms + jitter_ms);
                retries += 1;
                tracing::debug!(
                    retry = retries,
                    delay_ms = delay.as_millis(),
                    "sqlite lock contention, retrying"
                );
                std::thread::sleep(delay);
            }
            Err(err) => {
                if is_lock_error(&err) {
                    tracing::warn!(
                        retries = RETRY_MAX_RETRIES,
                        "sqlite lock contention persisted after all retries"
                    );
                }
                return Err(err);
            }
        }
    }
}

/// Default number of reader connections in the pool for file-backed databases.
const DEFAULT_READER_COUNT: usize = 4;

/// Default number of writes between WAL checkpoints.
const WAL_CHECKPOINT_INTERVAL: u64 = 10;

/// Connection pool providing one writer connection and N reader connections.
///
/// In WAL mode, SQLite allows concurrent readers while a single writer holds
/// the write lock. `rusqlite::Connection` is `!Sync`, so each connection is
/// wrapped in its own `Mutex`.
///
/// For in-memory databases (`:memory:` or `file::memory:`) that cannot share
/// state across connections, the pool falls back to single-connection mode
/// where all operations use the writer.
#[derive(Debug)]
pub(crate) struct ConnPool {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin counter for reader selection.
    reader_idx: AtomicUsize,
    /// Monotonic write counter for periodic WAL checkpoints.
    write_count: AtomicU64,
    /// Whether this pool is file-backed (WAL checkpoints only apply to files).
    is_file_backed: bool,
    /// Immutable generation binding; migration requires opening a fresh runtime.
    embedding_space: String,
    embedding_generation: u64,
}

impl ConnPool {
    /// Opens a file-backed connection pool: one writer + N readers.
    ///
    /// All connections share the same SQLite database file and run in WAL mode.
    /// Performs a startup WAL checkpoint (TRUNCATE) to reclaim any stale WAL
    /// from previous runs.
    pub(super) fn open_file(
        path: &Path,
        embedding_dim: usize,
        embedding_space: &str,
    ) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        super::schema::initialize_parent_dir(path)?;

        let writer = open_connection(path)?;
        // Writer sets WAL mode; readers inherit it from the file.
        initialize_schema(&writer, embedding_dim, embedding_space)?;

        // Startup checkpoint: safe because we're the only writer at init time.
        if let Err(e) = writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::debug!("startup WAL checkpoint skipped: {e}");
        }

        // Self-heal a diverged FTS5 index from a reused database (issue #343).
        // Non-fatal: a repair failure must not block opening the DB.
        match super::schema::ensure_fts_sync(&writer) {
            Ok(0) => {}
            Ok(repaired) => tracing::warn!(repaired, "repaired out-of-sync FTS5 index on open"),
            Err(e) => tracing::warn!("FTS5 sync check failed (continuing): {e}"),
        }

        let mut readers = Vec::with_capacity(DEFAULT_READER_COUNT);
        for _ in 0..DEFAULT_READER_COUNT {
            let reader = open_connection(path)?;
            configure_reader(&reader)?;
            readers.push(Mutex::new(reader));
        }

        let embedding_generation = verified_embedding_generation(&writer, embedding_space)?;
        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            reader_idx: AtomicUsize::new(0),
            write_count: AtomicU64::new(0),
            is_file_backed: true,
            embedding_space: embedding_space.to_owned(),
            embedding_generation,
        })
    }

    /// Opens an in-memory pool (single connection, no reader pool).
    ///
    /// Used for tests and non-file-backed scenarios where multiple connections
    /// cannot share state.
    pub(super) fn open_in_memory(embedding_dim: usize, embedding_space: &str) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn, embedding_dim, embedding_space)?;

        let embedding_generation = verified_embedding_generation(&conn, embedding_space)?;
        Ok(Self {
            writer: Mutex::new(conn),
            readers: Vec::new(),
            reader_idx: AtomicUsize::new(0),
            write_count: AtomicU64::new(0),
            is_file_backed: false,
            embedding_space: embedding_space.to_owned(),
            embedding_generation,
        })
    }

    /// Pin metadata and subsequent vector/index/hydration reads to one SQLite
    /// snapshot. Every phase of a multi-connection search must use this binding.
    /// Dropping the returned transaction releases the read snapshot.
    pub(crate) fn embedding_snapshot<'conn>(
        &self,
        conn: &'conn Connection,
    ) -> Result<Transaction<'conn>> {
        let snapshot = conn
            .unchecked_transaction()
            .context("failed to begin embedding read snapshot")?;
        verify_embedding_space_identity(&snapshot, &self.embedding_space)?;
        let generation = read_embedding_generation(&snapshot)?;
        if generation != self.embedding_generation {
            return Err(anyhow!(
                "embedding generation mismatch: database uses {generation} but runtime uses {}; restart MAG after re-embedding",
                self.embedding_generation,
            ));
        }
        Ok(snapshot)
    }

    /// Acquires the writer connection.
    pub(super) fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| anyhow!("sqlite writer mutex poisoned"))
    }

    /// Returns `true` when the pool has dedicated reader connections.
    pub(crate) fn has_readers(&self) -> bool {
        !self.readers.is_empty()
    }

    /// Increments the write counter and runs a passive WAL checkpoint every
    /// [`WAL_CHECKPOINT_INTERVAL`] writes. Call after each write transaction
    /// commits. No-op for in-memory databases.
    pub(super) fn note_write(&self) {
        if !self.is_file_backed {
            return;
        }
        let count = self.write_count.fetch_add(1, Ordering::Relaxed) + 1;
        if !count.is_multiple_of(WAL_CHECKPOINT_INTERVAL) {
            return;
        }
        // Use try_lock to avoid deadlock if the caller still holds the writer guard.
        if let Ok(conn) = self.writer.try_lock() {
            match conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
                Ok(()) => tracing::debug!(writes = count, "WAL passive checkpoint completed"),
                Err(e) => tracing::debug!(writes = count, "WAL passive checkpoint failed: {e}"),
            }
        } else {
            tracing::debug!(writes = count, "WAL checkpoint skipped — writer busy");
        }
    }

    /// Runs a passive WAL checkpoint on an already-held writer connection and
    /// counts it toward checkpoint cadence. Use after a bulk write (e.g. a batch
    /// transaction) that holds the writer for the whole transaction, to flush the
    /// accumulated WAL without blocking readers. Retries on transient lock
    /// contention with bounded backoff; no-op for in-memory databases.
    pub(super) fn checkpoint_passive(&self, conn: &Connection) {
        if !self.is_file_backed {
            return;
        }
        self.write_count.fetch_add(1, Ordering::Relaxed);
        if let Err(e) = retry_on_lock(|| conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")) {
            tracing::debug!("batch WAL passive checkpoint failed: {e}");
        }
    }

    /// Acquires a reader connection via round-robin. Falls back to the writer
    /// when no dedicated readers exist (in-memory mode).
    pub(crate) fn reader(&self) -> Result<MutexGuard<'_, Connection>> {
        if self.readers.is_empty() {
            return self.writer();
        }
        let idx = self.reader_idx.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        self.readers[idx]
            .lock()
            .map_err(|_| anyhow!("sqlite reader mutex poisoned"))
    }
}

/// Opens a single SQLite connection with standard PRAGMAs.
fn open_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;
    configure_reader(&conn)?;
    Ok(conn)
}

/// Applies read-oriented PRAGMAs shared by all connections (writer and readers).
fn configure_reader(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA busy_timeout=5000;\
         PRAGMA cache_size=-16000;\
         PRAGMA mmap_size=33554432;\
         PRAGMA synchronous=NORMAL;\
         PRAGMA temp_store=MEMORY;",
    )
    .context("failed to set connection PRAGMAs")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedding_snapshot_keeps_metadata_blob_and_index_together() -> Result<()> {
        use super::super::{encode_embedding, schema};
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("snapshot.db");
        let pool = ConnPool::open_file(&path, 4, "space-a")?;
        let original = encode_embedding(&[1.0, 0.0, 0.0, 0.0]);
        let replacement = encode_embedding(&[0.0, 1.0, 0.0, 0.0]);
        {
            let writer = pool.writer()?;
            writer.execute(
                "INSERT INTO memories (id, content, embedding, content_hash, source_type) VALUES ('alpha', 'alpha', ?1, 'alpha', 'user')",
                [&original],
            )?;
            #[cfg(feature = "sqlite-vec")]
            writer.execute(
                "INSERT INTO vec_memories (memory_id, embedding) VALUES ('alpha', ?1)",
                [&original],
            )?;
        }
        let reader = pool.reader()?;
        let snapshot = pool.embedding_snapshot(&reader)?;

        // Commit the vector/identity/generation change AFTER the fence but BEFORE
        // its vector read. A preflight check without a snapshot fails this test.
        let mut concurrent = Connection::open(&path)?;
        let migration = concurrent.transaction()?;
        migration.execute(
            "UPDATE memories SET embedding = ?1 WHERE id = 'alpha'",
            [&replacement],
        )?;
        #[cfg(feature = "sqlite-vec")]
        {
            migration.execute("DELETE FROM vec_memories WHERE memory_id = 'alpha'", [])?;
            migration.execute(
                "INSERT INTO vec_memories (memory_id, embedding) VALUES ('alpha', ?1)",
                [&replacement],
            )?;
        }
        schema::advance_embedding_generation(&migration)?;
        schema::update_embedding_space_identity(&migration, "space-b")?;
        migration.commit()?;

        assert_eq!(
            schema::read_embedding_space_identity(&snapshot)?.as_deref(),
            Some("space-a")
        );
        assert_eq!(schema::read_embedding_generation(&snapshot)?, 0);
        let blob: Vec<u8> = snapshot.query_row(
            "SELECT embedding FROM memories WHERE id = 'alpha'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(blob, original);
        #[cfg(feature = "sqlite-vec")]
        {
            let indexed: Vec<u8> = snapshot.query_row(
                "SELECT embedding FROM vec_memories WHERE memory_id = 'alpha'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(indexed, original);
        }
        drop(snapshot);
        assert!(
            pool.embedding_snapshot(&reader).is_err(),
            "a subsequent snapshot must reject the migrated database"
        );
        Ok(())
    }

    #[test]
    fn embedding_snapshot_rejects_missing_or_invalid_generation() -> Result<()> {
        let pool = ConnPool::open_in_memory(4, "space-a")?;
        let conn = pool.reader()?;
        for value in ["not-a-generation", "-1", "18446744073709551616"] {
            conn.execute(
                "UPDATE runtime_metadata SET value = ?1 WHERE key = 'embedding_generation'",
                [value],
            )?;
            assert!(pool.embedding_snapshot(&conn).is_err());
        }
        conn.execute(
            "DELETE FROM runtime_metadata WHERE key = 'embedding_generation'",
            [],
        )?;
        assert!(pool.embedding_snapshot(&conn).is_err());
        Ok(())
    }

    #[test]
    fn is_lock_error_detects_busy() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                extended_code: 5,
            },
            Some("database is locked".to_string()),
        );
        assert!(is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_detects_locked() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::DatabaseLocked,
                extended_code: 6,
            },
            Some("database table is locked".to_string()),
        );
        assert!(is_lock_error(&err));
    }

    #[test]
    fn is_lock_error_rejects_other_errors() {
        let err = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ffi::ErrorCode::ReadOnly,
                extended_code: 8,
            },
            None,
        );
        assert!(!is_lock_error(&err));

        let err2 = rusqlite::Error::QueryReturnedNoRows;
        assert!(!is_lock_error(&err2));
    }

    #[test]
    fn retry_on_lock_returns_immediately_for_non_lock_error() {
        let mut attempts = 0u32;
        let result: std::result::Result<(), rusqlite::Error> = retry_on_lock(|| {
            attempts += 1;
            Err(rusqlite::Error::QueryReturnedNoRows)
        });
        assert!(result.is_err());
        assert_eq!(attempts, 1, "should not retry for non-lock errors");
    }

    #[test]
    fn retry_on_lock_succeeds_on_first_try() {
        let mut attempts = 0u32;
        let result = retry_on_lock(|| {
            attempts += 1;
            Ok(42)
        });
        assert_eq!(result.unwrap(), 42);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn retry_on_lock_retries_on_busy_then_succeeds() {
        let mut attempts = 0u32;
        let result = retry_on_lock(|| {
            attempts += 1;
            if attempts < 3 {
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                        extended_code: 5,
                    },
                    None,
                ))
            } else {
                Ok("success")
            }
        });
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn retry_on_lock_exhausts_retries() {
        let mut attempts = 0u32;
        let result: std::result::Result<(), rusqlite::Error> = retry_on_lock(|| {
            attempts += 1;
            Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error {
                    code: rusqlite::ffi::ErrorCode::DatabaseBusy,
                    extended_code: 5,
                },
                None,
            ))
        });
        assert!(result.is_err());
        assert_eq!(
            attempts,
            RETRY_MAX_RETRIES + 1,
            "should attempt initial + RETRY_MAX_RETRIES times"
        );
    }
}
