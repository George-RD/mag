use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::memory_core::{Retriever, Storage};

/// Controls how the SQLite storage backend is initialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InitMode {
    /// Use the default database path (`~/.romega-memory/memory.db`).
    Default,
    /// Reserved for future advanced configuration (currently delegates to `Default`).
    Advanced,
}

/// SQLite-backed persistent storage for the memory system.
///
/// Wraps a shared `rusqlite::Connection` behind `Arc<Mutex<_>>` so it can
/// be cloned into both the `Storage` and `Retriever` roles of a [`Pipeline`].
#[derive(Clone)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    /// Creates a new `SqliteStorage` using the given [`InitMode`].
    pub fn new(mode: InitMode) -> Result<Self> {
        match mode {
            InitMode::Default => Self::new_default(),
            InitMode::Advanced => Self::new_advanced_placeholder(),
        }
    }

    /// Opens (or creates) the database at the default path.
    pub fn new_default() -> Result<Self> {
        let path = default_db_path()?;
        Self::new_with_path(path)
    }

    /// Placeholder for advanced initialization (currently delegates to [`new_default`](Self::new_default)).
    pub fn new_advanced_placeholder() -> Result<Self> {
        Self::new_default()
    }

    /// Opens (or creates) a database at the given `path`, creating parent directories as needed.
    ///
    /// Performs blocking filesystem and SQLite I/O. Call before entering the
    /// async runtime or wrap the call in [`tokio::task::spawn_blocking`].
    pub fn new_with_path(path: PathBuf) -> Result<Self> {
        initialize_parent_dir(&path)?;
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;

        initialize_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Inserts a directed relationship between two memories.
    ///
    /// Returns the generated relationship ID.
    #[allow(dead_code)]
    pub async fn add_relationship(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: &str,
    ) -> Result<String> {
        let rel_id = Uuid::new_v4().to_string();
        let conn = Arc::clone(&self.conn);
        let source_id = source_id.to_string();
        let target_id = target_id.to_string();
        let rel_type = rel_type.to_string();
        let rid = rel_id.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            conn.execute(
                "INSERT INTO relationships (id, source_id, target_id, rel_type) VALUES (?1, ?2, ?3, ?4)",
                params![rid, source_id, target_id, rel_type],
            )
            .context("failed to insert relationship")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(rel_id)
    }

    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    #[cfg(test)]
    fn debug_get_last_accessed_at(&self, id: &str) -> Result<String> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

        let value: Option<String> = conn
            .query_row(
                "SELECT last_accessed_at FROM memories WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to query last_accessed_at")?;

        value.ok_or_else(|| anyhow!("memory not found for id={id}"))
    }

    #[cfg(test)]
    fn debug_force_last_accessed_at(&self, id: &str, timestamp: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

        conn.execute(
            "UPDATE memories SET last_accessed_at = ?2 WHERE id = ?1",
            params![id, timestamp],
        )
        .context("failed to force last_accessed_at")?;

        Ok(())
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn store(&self, id: &str, data: &str) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());

        let conn = Arc::clone(&self.conn);
        let id = id.to_string();
        let data = data.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sqlite transaction")?;

            tx.execute(
                "INSERT INTO memories (
                    id,
                    content,
                    embedding,
                    parent_id,
                    event_at,
                    content_hash,
                    source_type,
                    last_accessed_at,
                    tags
                ) VALUES (
                    ?1,
                    ?2,
                    NULL,
                    NULL,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?3,
                    'cli_input',
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ''
                )
                ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    content_hash = excluded.content_hash,
                    source_type = excluded.source_type,
                    tags = excluded.tags,
                    last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![id, data, content_hash],
            )
            .context("failed to insert memory")?;

            tx.commit().context("failed to commit sqlite transaction")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(())
    }
}

#[async_trait]
impl Retriever for SqliteStorage {
    async fn retrieve(&self, id: &str) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sqlite transaction")?;

            let content: Option<String> = tx
                .query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query memory content")?;

            let content = content.ok_or_else(|| anyhow!("memory not found for id={id}"))?;

            tx.execute(
                "UPDATE memories SET last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
                params![id],
            )
            .context("failed to update last_accessed_at")?;

            tx.commit().context("failed to commit sqlite transaction")?;
            Ok::<_, anyhow::Error>(content)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

/// Ensures the parent directory of `path` exists, creating it recursively if needed.
fn initialize_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory {}", parent.display()))?;
    Ok(())
}

/// Creates the `memories` and `relationships` tables if they don't exist and enables foreign keys.
fn initialize_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("failed to enable foreign key enforcement")?;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memories (
            id TEXT PRIMARY KEY,
            content TEXT NOT NULL,
            embedding BLOB,
            parent_id TEXT,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            event_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            content_hash TEXT NOT NULL,
            source_type TEXT NOT NULL,
            last_accessed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            tags TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS relationships (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            rel_type TEXT NOT NULL,
            FOREIGN KEY(source_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY(target_id) REFERENCES memories(id) ON DELETE CASCADE
        );",
    )
    .context("failed to initialize sqlite schema")?;

    Ok(())
}

/// Resolves the default database path (`$HOME/.romega-memory/memory.db`), falling back to `USERPROFILE` on Windows.
fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            anyhow!("neither HOME nor USERPROFILE is set — cannot resolve default database path")
        })?;
    Ok(PathBuf::from(home).join(".romega-memory").join("memory.db"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::{Retriever, Storage};

    #[test]
    fn test_new_with_path_creates_parent_and_db() {
        let base = std::env::temp_dir().join(format!("romega-sqlite-test-{}", Uuid::new_v4()));
        let db_path = base.join("nested").join("memory.db");

        let storage = SqliteStorage::new_with_path(db_path.clone());
        assert!(storage.is_ok());
        assert!(db_path.exists());
        assert!(db_path.parent().is_some_and(Path::exists));

        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn test_schema_contains_required_tables_and_columns() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let conn = storage
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))
            .unwrap();

        let memories_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };

        for col in [
            "id",
            "content",
            "embedding",
            "parent_id",
            "created_at",
            "event_at",
            "content_hash",
            "source_type",
            "last_accessed_at",
            "tags",
        ] {
            assert!(memories_cols.iter().any(|c| c == col));
        }

        let relationships_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(relationships)").unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };

        for col in ["id", "source_id", "target_id", "rel_type"] {
            assert!(relationships_cols.iter().any(|c| c == col));
        }
    }

    #[tokio::test]
    async fn test_store_and_retrieve_roundtrip() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        storage.store("m1", "hello world").await.unwrap();
        let content = storage.retrieve("m1").await.unwrap();

        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_retrieve_updates_last_accessed_at() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        storage.store("m2", "payload").await.unwrap();
        storage
            .debug_force_last_accessed_at("m2", "2000-01-01T00:00:00.000Z")
            .unwrap();

        let before = storage.debug_get_last_accessed_at("m2").unwrap();
        assert_eq!(before, "2000-01-01T00:00:00.000Z");

        let _ = storage.retrieve("m2").await.unwrap();
        let after = storage.debug_get_last_accessed_at("m2").unwrap();

        assert_ne!(after, before);
    }

    #[tokio::test]
    async fn test_add_relationship() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("a", "alpha").await.unwrap();
        storage.store("b", "beta").await.unwrap();

        let rel_id = storage
            .add_relationship("a", "b", "links_to")
            .await
            .unwrap();

        let conn = storage
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))
            .unwrap();

        let stored_rel_type: String = conn
            .query_row(
                "SELECT rel_type FROM relationships WHERE id = ?1",
                params![rel_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(stored_rel_type, "links_to");
    }
}
