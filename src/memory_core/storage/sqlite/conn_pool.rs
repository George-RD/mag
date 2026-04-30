use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rusqlite::Connection;

use super::schema::initialize_schema;

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
pub(super) struct ConnPool {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin counter for reader selection.
    reader_idx: AtomicUsize,
    /// Monotonic write counter for periodic WAL checkpoints.
    write_count: AtomicU64,
    /// Whether this pool is file-backed (WAL checkpoints only apply to files).
    is_file_backed: bool,
}

impl ConnPool {
    /// Opens a file-backed connection pool: one writer + N readers.
    ///
    /// All connections share the same SQLite database file and run in WAL mode.
    /// Performs a startup WAL checkpoint (TRUNCATE) to reclaim any stale WAL
    /// from previous runs.
    pub(super) fn open_file(path: &Path, embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        super::schema::initialize_parent_dir(path)?;

        let writer = open_connection(path)?;
        // Writer sets WAL mode; readers inherit it from the file.
        initialize_schema(&writer, embedding_dim)?;

        // Startup checkpoint: safe because we're the only writer at init time.
        if let Err(e) = writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::debug!("startup WAL checkpoint skipped: {e}");
        }

        let mut readers = Vec::with_capacity(DEFAULT_READER_COUNT);
        for _ in 0..DEFAULT_READER_COUNT {
            let reader = open_connection(path)?;
            configure_reader(&reader)?;
            readers.push(Mutex::new(reader));
        }

        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            reader_idx: AtomicUsize::new(0),
            write_count: AtomicU64::new(0),
            is_file_backed: true,
        })
    }

    /// Opens an in-memory pool (single connection, no reader pool).
    ///
    /// Used for tests and non-file-backed scenarios where multiple connections
    /// cannot share state.
    pub(super) fn open_in_memory(embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn, embedding_dim)?;

        Ok(Self {
            writer: Mutex::new(conn),
            readers: Vec::new(),
            reader_idx: AtomicUsize::new(0),
            write_count: AtomicU64::new(0),
            is_file_backed: false,
        })
    }

    /// Acquires the writer connection.
    pub(super) fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| anyhow!("sqlite writer mutex poisoned"))
    }

    /// Returns `true` when the pool has dedicated reader connections.
    pub(super) fn has_readers(&self) -> bool {
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

    /// Acquires a reader connection via round-robin. Falls back to the writer
    /// when no dedicated readers exist (in-memory mode).
    pub(super) fn reader(&self) -> Result<MutexGuard<'_, Connection>> {
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
