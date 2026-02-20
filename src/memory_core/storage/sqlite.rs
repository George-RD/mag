use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::memory_core::{
    Deleter, ListResult, Lister, Recents, Relationship, RelationshipQuerier, Retriever,
    SearchResult, Searcher, SemanticResult, SemanticSearcher, Storage, Tagger, Updater,
};

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
    async fn store(&self, id: &str, data: &str, tags: &[String]) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());
        let embedding = serde_json::to_vec(&embedding_for_text(data))
            .context("failed to serialize embedding")?;
        let tags_json = serde_json::to_string(tags).context("failed to serialize tags to JSON")?;

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
                    ?3,
                    NULL,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?4,
                    'cli_input',
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?5
                )
                ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    embedding = excluded.embedding,
                    content_hash = excluded.content_hash,
                    source_type = excluded.source_type,
                    tags = excluded.tags,
                    last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![id, data, embedding, content_hash, tags_json],
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

#[async_trait]
impl Searcher for SqliteStorage {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let escaped = query
            .to_lowercase()
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let effective_limit = i64::try_from(limit).context("search limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content
                     FROM memories
                     WHERE lower(content) LIKE ?1 ESCAPE '\\'
                     ORDER BY last_accessed_at DESC
                     LIMIT ?2",
                )
                .context("failed to prepare search query")?;

            let rows = stmt
                .query_map(params![pattern, effective_limit], |row| {
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                    })
                })
                .context("failed to execute search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode search row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Recents for SqliteStorage {
    async fn recent(&self, limit: usize) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let effective_limit = i64::try_from(limit).context("recent limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content
                     FROM memories
                     ORDER BY last_accessed_at DESC
                     LIMIT ?1",
                )
                .context("failed to prepare recent query")?;

            let rows = stmt
                .query_map(params![effective_limit], |row| {
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                    })
                })
                .context("failed to execute recent query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode recent row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl SemanticSearcher for SqliteStorage {
    async fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let query_embedding = embedding_for_text(query);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content, embedding
                     FROM memories
                     WHERE embedding IS NOT NULL",
                )
                .context("failed to prepare semantic search query")?;

            let rows = stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let embedding_blob: Vec<u8> = row.get(2)?;
                    Ok((id, content, embedding_blob))
                })
                .context("failed to execute semantic search query")?;

            let mut ranked = Vec::new();
            for row in rows {
                let (id, content, embedding_blob) =
                    row.context("failed to decode semantic search row")?;
                let candidate: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                let score = cosine_similarity(&query_embedding, &candidate);
                ranked.push(SemanticResult { id, content, score });
            }

            ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
            ranked.truncate(limit);

            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Deleter for SqliteStorage {
    async fn delete(&self, id: &str) -> Result<bool> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let changes = conn
                .execute("DELETE FROM memories WHERE id = ?1", params![id])
                .context("failed to delete memory")?;
            Ok::<_, anyhow::Error>(changes > 0)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Updater for SqliteStorage {
    async fn update(&self, id: &str, content: Option<&str>, tags: Option<&[String]>) -> Result<()> {
        if content.is_none() && tags.is_none() {
            return Err(anyhow!("at least one of content or tags must be provided"));
        }

        let content_fields = content
            .map(|c| {
                let mut hasher = Sha256::new();
                hasher.update(c.as_bytes());
                let hash = format!("{:x}", hasher.finalize());
                let emb = serde_json::to_vec(&embedding_for_text(c))
                    .context("failed to serialize embedding")?;
                Ok::<_, anyhow::Error>((c.to_string(), hash, emb))
            })
            .transpose()?;

        let tags_json = tags
            .map(|t| serde_json::to_string(t).context("failed to serialize tags"))
            .transpose()?;

        let conn = Arc::clone(&self.conn);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let changes = match (&content_fields, &tags_json) {
                (Some((content, hash, emb)), Some(tags)) => conn.execute(
                    "UPDATE memories SET
                        content = ?2, content_hash = ?3, embedding = ?4, tags = ?5,
                        last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![id, content, hash, emb, tags],
                ),
                (Some((content, hash, emb)), None) => conn.execute(
                    "UPDATE memories SET
                        content = ?2, content_hash = ?3, embedding = ?4,
                        last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![id, content, hash, emb],
                ),
                (None, Some(tags)) => conn.execute(
                    "UPDATE memories SET
                        tags = ?2,
                        last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![id, tags],
                ),
                (None, None) => unreachable!(),
            }
            .context("failed to update memory")?;

            if changes == 0 {
                return Err(anyhow!("memory not found for id={id}"));
            }
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(())
    }
}

#[async_trait]
impl Tagger for SqliteStorage {
    async fn get_by_tags(&self, tags: &[String], limit: usize) -> Result<Vec<SearchResult>> {
        if tags.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let tags = tags.to_vec();
        let effective_limit = i64::try_from(limit).context("tag search limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            // Build dynamic WHERE clause with dual-read support:
            // - JSON tags: json_valid + json_each
            // - Legacy CSV tags: instr-based comma-delimited matching
            let mut json_conditions = Vec::new();
            let mut csv_conditions = Vec::new();
            let mut param_values: Vec<String> = Vec::new();
            for (i, tag) in tags.iter().enumerate() {
                let p = i + 1;
                json_conditions.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(memories.tags) WHERE value = ?{p})"
                ));
                csv_conditions.push(format!(
                    "instr(',' || memories.tags || ',', ',' || ?{p} || ',') > 0"
                ));
                param_values.push(tag.clone());
            }
            let limit_param_idx = param_values.len() + 1;
            let json_clause = json_conditions.join(" AND ");
            let csv_clause = csv_conditions.join(" AND ");
            let sql = format!(
                "SELECT id, content FROM memories \
                 WHERE ((json_valid(memories.tags) AND {json_clause}) \
                        OR (NOT json_valid(memories.tags) AND memories.tags != '' AND {csv_clause})) \
                 ORDER BY last_accessed_at DESC LIMIT ?{limit_param_idx}"
            );

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare tag search query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for v in &param_values {
                param_refs.push(v);
            }
            param_refs.push(&effective_limit);

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                    })
                })
                .context("failed to execute tag search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode tag search row")?);
            }
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Lister for SqliteStorage {
    async fn list(&self, offset: usize, limit: usize) -> Result<ListResult> {
        if limit == 0 {
            let conn = Arc::clone(&self.conn);
            let total = tokio::task::spawn_blocking(move || {
                let conn = conn
                    .lock()
                    .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                    .context("failed to count memories")?;
                Ok::<_, anyhow::Error>(count as usize)
            })
            .await
            .context("spawn_blocking join error")??;
            return Ok(ListResult {
                memories: Vec::new(),
                total,
            });
        }

        let conn = Arc::clone(&self.conn);
        let effective_limit = i64::try_from(limit).context("list limit exceeds i64")?;
        let effective_offset = i64::try_from(offset).context("list offset exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories")?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content FROM memories
                     ORDER BY created_at DESC
                     LIMIT ?1 OFFSET ?2",
                )
                .context("failed to prepare list query")?;

            let rows = stmt
                .query_map(params![effective_limit, effective_offset], |row| {
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                    })
                })
                .context("failed to execute list query")?;

            let mut memories = Vec::new();
            for row in rows {
                memories.push(row.context("failed to decode list row")?);
            }

            Ok::<_, anyhow::Error>(ListResult {
                memories,
                total: total as usize,
            })
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl RelationshipQuerier for SqliteStorage {
    async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>> {
        let conn = Arc::clone(&self.conn);
        let memory_id = memory_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, source_id, target_id, rel_type
                     FROM relationships
                     WHERE source_id = ?1 OR target_id = ?1",
                )
                .context("failed to prepare relationships query")?;

            let rows = stmt
                .query_map(params![memory_id], |row| {
                    Ok(Relationship {
                        id: row.get(0)?,
                        source_id: row.get(1)?,
                        target_id: row.get(2)?,
                        rel_type: row.get(3)?,
                    })
                })
                .context("failed to execute relationships query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode relationship row")?);
            }
            Ok::<_, anyhow::Error>(results)
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
            tags TEXT NOT NULL DEFAULT '[]'
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

fn embedding_for_text(input: &str) -> Vec<f32> {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let mut vec: Vec<f32> = digest.iter().map(|b| *b as f32 / 255.0).collect();
    normalize_embedding(&mut vec);
    vec
}

fn normalize_embedding(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vec {
            *value /= norm;
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::{Recents, Retriever, Searcher, SemanticSearcher, Storage};

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

        storage.store("m1", "hello world", &[]).await.unwrap();
        let content = storage.retrieve("m1").await.unwrap();

        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_retrieve_updates_last_accessed_at() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        storage.store("m2", "payload", &[]).await.unwrap();
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
        storage.store("a", "alpha", &[]).await.unwrap();
        storage.store("b", "beta", &[]).await.unwrap();

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

    #[tokio::test]
    async fn test_search_matches_content_case_insensitive() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("s1", "Rust memory store", &[]).await.unwrap();
        storage.store("s2", "another note", &[]).await.unwrap();

        let results = storage.search("MEMORY", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s1");
        assert_eq!(results[0].content, "Rust memory store");
    }

    #[tokio::test]
    async fn test_search_treats_like_wildcards_as_literals() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("p1", "value 100% done", &[]).await.unwrap();
        storage.store("p2", "value 1000 done", &[]).await.unwrap();

        let results = storage.search("100%", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "p1");
    }

    #[tokio::test]
    async fn test_search_with_zero_limit_returns_no_results() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage
            .store("z1", "zero limit candidate", &[])
            .await
            .unwrap();

        let results = storage.search("zero", 0).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_escapes_underscore_and_backslash_literals() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("u1", r"file_a\b", &[]).await.unwrap();
        storage.store("u2", r"fileXab", &[]).await.unwrap();

        let underscore_results = storage.search("file_a", 10).await.unwrap();
        assert_eq!(underscore_results.len(), 1);
        assert_eq!(underscore_results[0].id, "u1");

        let backslash_results = storage.search(r"a\b", 10).await.unwrap();
        assert_eq!(backslash_results.len(), 1);
        assert_eq!(backslash_results[0].id, "u1");
    }

    #[tokio::test]
    async fn test_recent_returns_most_recently_accessed_first() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("r1", "older", &[]).await.unwrap();
        storage.store("r2", "newer", &[]).await.unwrap();

        storage
            .debug_force_last_accessed_at("r1", "2000-01-01T00:00:00.000Z")
            .unwrap();
        storage
            .debug_force_last_accessed_at("r2", "2001-01-01T00:00:00.000Z")
            .unwrap();

        let results = storage.recent(2).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "r2");
        assert_eq!(results[1].id, "r1");
    }

    #[tokio::test]
    async fn test_semantic_search_prefers_exact_text_match() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("e1", "alpha beta gamma", &[]).await.unwrap();
        storage.store("e2", "other content", &[]).await.unwrap();

        let results = storage
            .semantic_search("alpha beta gamma", 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "e1");
        assert!(results[0].score >= results[1].score);
    }

    #[tokio::test]
    async fn test_semantic_search_zero_limit_returns_no_results() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("e3", "candidate", &[]).await.unwrap();

        let results = storage.semantic_search("candidate", 0).await.unwrap();
        assert!(results.is_empty());
    }

    // ── Delete tests ──

    #[tokio::test]
    async fn test_delete_existing_memory() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("d1", "to-delete", &[]).await.unwrap();

        let deleted = storage.delete("d1").await.unwrap();
        assert!(deleted);

        let err = storage.retrieve("d1").await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_returns_false() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let deleted = storage.delete("no-such-id").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_delete_cascades_relationships() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("ca", "alpha", &[]).await.unwrap();
        storage.store("cb", "beta", &[]).await.unwrap();
        storage
            .add_relationship("ca", "cb", "links_to")
            .await
            .unwrap();

        storage.delete("ca").await.unwrap();

        let rels = storage.get_relationships("cb").await.unwrap();
        assert!(rels.is_empty());
    }

    // ── Update tests ──

    #[tokio::test]
    async fn test_update_content_changes_value() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("up1", "original", &[]).await.unwrap();

        storage.update("up1", Some("updated"), None).await.unwrap();
        let content = storage.retrieve("up1").await.unwrap();
        assert_eq!(content, "updated");
    }

    #[tokio::test]
    async fn test_update_with_tags() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["a".to_string(), "b".to_string()];
        storage.store("up2", "data", &tags).await.unwrap();

        let new_tags = vec!["x".to_string()];
        storage
            .update("up2", Some("data-v2"), Some(&new_tags))
            .await
            .unwrap();

        let results = storage.get_by_tags(&new_tags, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up2");
        assert_eq!(results[0].content, "data-v2");
    }

    #[tokio::test]
    async fn test_update_without_tags_preserves_existing() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["keep".to_string()];
        storage.store("up3", "data", &tags).await.unwrap();

        storage.update("up3", Some("data-v2"), None).await.unwrap();

        let results = storage.get_by_tags(&tags, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up3");
    }

    #[tokio::test]
    async fn test_update_tags_only_preserves_content() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage
            .store("up4", "keep-this", &["old".to_string()])
            .await
            .unwrap();

        let new_tags = vec!["new-tag".to_string()];
        storage.update("up4", None, Some(&new_tags)).await.unwrap();

        let content = storage.retrieve("up4").await.unwrap();
        assert_eq!(content, "keep-this");
        let results = storage.get_by_tags(&new_tags, 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up4");
    }

    #[tokio::test]
    async fn test_update_neither_content_nor_tags_errors() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("up5", "data", &[]).await.unwrap();
        let err = storage.update("up5", None, None).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_update_nonexistent_errors() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let err = storage.update("ghost", Some("data"), None).await;
        assert!(err.is_err());
    }

    // ── Tags tests ──

    #[tokio::test]
    async fn test_get_by_tags_filters_correctly() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage
            .store("t1", "one", &["rust".to_string(), "memory".to_string()])
            .await
            .unwrap();
        storage
            .store("t2", "two", &["rust".to_string()])
            .await
            .unwrap();
        storage
            .store("t3", "three", &["python".to_string()])
            .await
            .unwrap();

        let results = storage
            .get_by_tags(&["rust".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let results = storage
            .get_by_tags(&["rust".to_string(), "memory".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");
    }

    #[tokio::test]
    async fn test_get_by_tags_legacy_csv_backward_compat() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Insert a row with legacy CSV tags directly via raw SQL
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, content_hash, source_type, tags)
                 VALUES ('csv1', 'legacy data', 'hash1', 'test', 'rust,memory')",
                [],
            )
            .unwrap();
        }
        // JSON-tagged row via normal API
        storage
            .store(
                "json1",
                "json data",
                &["rust".to_string(), "search".to_string()],
            )
            .await
            .unwrap();

        // Search for 'rust' should find both CSV and JSON rows
        let results = storage
            .get_by_tags(&["rust".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        // Search for 'memory' should find only the CSV row
        let results = storage
            .get_by_tags(&["memory".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "csv1");

        // Search for 'search' should find only the JSON row
        let results = storage
            .get_by_tags(&["search".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "json1");
    }

    #[tokio::test]
    async fn test_get_by_tags_empty_returns_empty() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage
            .store("te", "data", &["tag".to_string()])
            .await
            .unwrap();
        let results = storage.get_by_tags(&[], 10).await.unwrap();
        assert!(results.is_empty());
    }

    // ── List tests ──

    #[tokio::test]
    async fn test_list_with_pagination() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for i in 0..5 {
            storage
                .store(&format!("l{i}"), &format!("item-{i}"), &[])
                .await
                .unwrap();
        }

        let result = storage.list(0, 3).await.unwrap();
        assert_eq!(result.memories.len(), 3);
        assert_eq!(result.total, 5);

        let result = storage.list(3, 3).await.unwrap();
        assert_eq!(result.memories.len(), 2);
        assert_eq!(result.total, 5);
    }

    #[tokio::test]
    async fn test_list_zero_limit_returns_count_only() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("lz1", "a", &[]).await.unwrap();
        storage.store("lz2", "b", &[]).await.unwrap();

        let result = storage.list(0, 0).await.unwrap();
        assert!(result.memories.is_empty());
        assert_eq!(result.total, 2);
    }

    // ── Relationship query tests ──

    #[tokio::test]
    async fn test_get_relationships_both_directions() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("ra", "alpha", &[]).await.unwrap();
        storage.store("rb", "beta", &[]).await.unwrap();
        storage.store("rc", "gamma", &[]).await.unwrap();

        storage
            .add_relationship("ra", "rb", "links_to")
            .await
            .unwrap();
        storage
            .add_relationship("rc", "ra", "depends_on")
            .await
            .unwrap();

        let rels = storage.get_relationships("ra").await.unwrap();
        assert_eq!(rels.len(), 2);

        let types: Vec<&str> = rels.iter().map(|r| r.rel_type.as_str()).collect();
        assert!(types.contains(&"links_to"));
        assert!(types.contains(&"depends_on"));
    }

    #[tokio::test]
    async fn test_get_relationships_empty() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        storage.store("lonely", "alone", &[]).await.unwrap();

        let rels = storage.get_relationships("lonely").await.unwrap();
        assert!(rels.is_empty());
    }

    // ── Tags roundtrip with store ──

    #[tokio::test]
    async fn test_store_with_tags_roundtrip() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["project-x".to_string(), "important".to_string()];
        storage.store("st1", "tagged content", &tags).await.unwrap();

        let results = storage
            .get_by_tags(&["project-x".to_string()], 10)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "st1");
        assert_eq!(results[0].content, "tagged content");
    }
}
