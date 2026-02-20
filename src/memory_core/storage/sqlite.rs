use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::memory_core::{
    Deleter, ListResult, Lister, MemoryInput, MemoryUpdate, Recents, Relationship,
    RelationshipQuerier, Retriever, SearchOptions, SearchResult, Searcher, SemanticResult,
    SemanticSearcher, Storage, Tagger, Updater, embedder::Embedder,
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
    embedder: Arc<dyn Embedder>,
}

impl SqliteStorage {
    /// Creates a new `SqliteStorage` using the given [`InitMode`].
    pub fn new(mode: InitMode, embedder: Arc<dyn Embedder>) -> Result<Self> {
        match mode {
            InitMode::Default => Self::new_default(embedder),
            InitMode::Advanced => Self::new_advanced_placeholder(embedder),
        }
    }

    /// Opens (or creates) the database at the default path.
    pub fn new_default(embedder: Arc<dyn Embedder>) -> Result<Self> {
        let path = default_db_path()?;
        Self::new_with_path(path, embedder)
    }

    /// Placeholder for advanced initialization (currently delegates to [`new_default`](Self::new_default)).
    pub fn new_advanced_placeholder(embedder: Arc<dyn Embedder>) -> Result<Self> {
        Self::new_default(embedder)
    }

    /// Opens (or creates) a database at the given `path`, creating parent directories as needed.
    ///
    /// Performs blocking filesystem and SQLite I/O. Call before entering the
    /// async runtime or wrap the call in [`tokio::task::spawn_blocking`].
    pub fn new_with_path(path: PathBuf, embedder: Arc<dyn Embedder>) -> Result<Self> {
        initialize_parent_dir(&path)?;
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;

        initialize_schema(&conn)?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            embedder,
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
        weight: f64,
        metadata: &serde_json::Value,
    ) -> Result<String> {
        let rel_id = Uuid::new_v4().to_string();
        let conn = Arc::clone(&self.conn);
        let source_id = source_id.to_string();
        let target_id = target_id.to_string();
        let rel_type = rel_type.to_string();
        let metadata_json =
            serde_json::to_string(metadata).context("failed to serialize relationship metadata")?;
        let rid = rel_id.clone();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            conn.execute(
                "INSERT INTO relationships (id, source_id, target_id, rel_type, weight, metadata) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![rid, source_id, target_id, rel_type, weight, metadata_json],
            )
            .context("failed to insert relationship")?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(rel_id)
    }

    #[allow(dead_code)]
    pub async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        <Self as Storage>::store(self, id, data, input).await
    }

    #[allow(dead_code)]
    pub async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        <Self as Updater>::update(self, id, input).await
    }

    /// Returns storage statistics as a JSON Value.
    pub async fn stats(&self) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let total_memories: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories")?;

            let total_relationships: i64 = conn
                .query_row("SELECT COUNT(*) FROM relationships", [], |row| row.get(0))
                .context("failed to count relationships")?;

            let avg_importance: f64 = conn
                .query_row(
                    "SELECT COALESCE(AVG(importance), 0.0) FROM memories",
                    [],
                    |row| row.get(0),
                )
                .context("failed to get average importance")?;

            let total_access: i64 = conn
                .query_row(
                    "SELECT COALESCE(SUM(access_count), 0) FROM memories",
                    [],
                    |row| row.get(0),
                )
                .context("failed to get total access count")?;

            let fts_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories_fts", [], |row| row.get(0))
                .context("failed to count FTS5 entries")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "total_memories": total_memories,
                "total_relationships": total_relationships,
                "average_importance": avg_importance,
                "total_access_count": total_access,
                "fts5_indexed": fts_count,
                "fts5_in_sync": fts_count == total_memories,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    /// Exports all memories and relationships as a JSON string.
    pub async fn export_all(&self) -> Result<String> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut mem_stmt = conn
                .prepare(
                    "SELECT id, content, tags, importance, metadata, embedding, parent_id,
                            created_at, event_at, content_hash, source_type, last_accessed_at,
                            access_count, session_id, event_type, project, priority, entity_id, agent_type
                     FROM memories ORDER BY created_at",
                )
                .context("failed to prepare export query")?;

            let memories: Vec<serde_json::Value> = mem_stmt
                .query_map([], |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let tags: String = row.get(2)?;
                    let importance: f64 = row.get(3)?;
                    let metadata: String = row.get(4)?;
                    let parent_id: Option<String> = row.get(6)?;
                    let created_at: String = row.get(7)?;
                    let event_at: String = row.get(8)?;
                    let content_hash: String = row.get(9)?;
                    let source_type: String = row.get(10)?;
                    let last_accessed_at: String = row.get(11)?;
                    let access_count: i64 = row.get(12)?;
                    let session_id: Option<String> = row.get(13).ok();
                    let event_type: Option<String> = row.get(14).ok();
                    let project: Option<String> = row.get(15).ok();
                    let priority: Option<i64> = row.get(16).ok();
                    let entity_id: Option<String> = row.get(17).ok();
                    let agent_type: Option<String> = row.get(18).ok();
                    let tags_value = serde_json::from_str::<serde_json::Value>(&tags)
                        .unwrap_or_else(|_| serde_json::Value::Array(vec![]));
                    let metadata_value = serde_json::from_str::<serde_json::Value>(&metadata)
                        .unwrap_or_else(|_| serde_json::json!({}));
                    Ok(serde_json::json!({
                        "id": id,
                        "content": content,
                        "tags": tags_value,
                        "importance": importance,
                        "metadata": metadata_value,
                        "parent_id": parent_id,
                        "created_at": created_at,
                        "event_at": event_at,
                        "content_hash": content_hash,
                        "source_type": source_type,
                        "last_accessed_at": last_accessed_at,
                        "access_count": access_count,
                        "session_id": session_id,
                        "event_type": event_type,
                        "project": project,
                        "priority": priority,
                        "entity_id": entity_id,
                        "agent_type": agent_type,
                    }))
                })
                .context("failed to query memories for export")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to decode memory row for export")?;

            let mut rel_stmt = conn
                .prepare("SELECT id, source_id, target_id, rel_type, weight, metadata, created_at FROM relationships ORDER BY id")
                .context("failed to prepare relationship export query")?;

            let relationships: Vec<serde_json::Value> = rel_stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "source_id": row.get::<_, String>(1)?,
                        "target_id": row.get::<_, String>(2)?,
                        "rel_type": row.get::<_, String>(3)?,
                        "weight": row.get::<_, f64>(4).unwrap_or(1.0),
                        "metadata": serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(5).unwrap_or_else(|_| "{}".to_string())).unwrap_or_else(|_| serde_json::json!({})),
                        "created_at": row.get::<_, String>(6).unwrap_or_else(|_| "".to_string()),
                    }))
                })
                .context("failed to query relationships for export")?
                .collect::<std::result::Result<Vec<_>, _>>()
                .context("failed to decode relationship row for export")?;

            let export = serde_json::json!({
                "version": 1,
                "memories": memories,
                "relationships": relationships,
            });

            serde_json::to_string_pretty(&export).context("failed to serialize export data")
        })
        .await
        .context("spawn_blocking join error")?
    }

    /// Imports memories and relationships from a JSON string.
    /// Returns (memories_imported, relationships_imported).
    pub async fn import_all(&self, data: &str) -> Result<(usize, usize)> {
        let parsed: serde_json::Value =
            serde_json::from_str(data).context("failed to parse import JSON")?;

        let memories = parsed["memories"]
            .as_array()
            .ok_or_else(|| anyhow!("import JSON missing 'memories' array"))?
            .clone();

        let relationships = parsed["relationships"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let tx = conn
                .unchecked_transaction()
                .context("failed to start import transaction")?;

            let mut mem_count = 0usize;
            for mem in &memories {
                let id = mem["id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("memory missing id"))?;
                let content = mem["content"]
                    .as_str()
                    .ok_or_else(|| anyhow!("memory missing content"))?;
                let tags = serde_json::to_string(&mem["tags"]).unwrap_or_else(|_| "[]".to_string());
                let importance = mem["importance"].as_f64().unwrap_or(0.5);
                let metadata =
                    serde_json::to_string(&mem["metadata"]).unwrap_or_else(|_| "{}".to_string());
                let content_hash = mem["content_hash"].as_str().unwrap_or("");
                let source_type = mem["source_type"].as_str().unwrap_or("import");
                let access_count = mem["access_count"].as_i64().unwrap_or(0);
                let session_id = mem["session_id"].as_str();
                let event_type = mem["event_type"].as_str();
                let project = mem["project"].as_str();
                let priority = mem["priority"].as_i64();
                let entity_id = mem["entity_id"].as_str();
                let agent_type = mem["agent_type"].as_str();

                tx.execute(
                    "INSERT OR REPLACE INTO memories (
                        id, content, content_hash, source_type, tags, importance, metadata, access_count,
                        session_id, event_type, project, priority, entity_id, agent_type
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                    params![
                        id,
                        content,
                        content_hash,
                        source_type,
                        tags,
                        importance,
                        metadata,
                        access_count,
                        session_id,
                        event_type,
                        project,
                        priority,
                        entity_id,
                        agent_type,
                    ],
                )
                .context("failed to import memory")?;

                tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to clean FTS5 for import")?;
                tx.execute(
                    "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                    params![id, content],
                )
                .context("failed to sync FTS5 for import")?;

                mem_count += 1;
            }

            let mut rel_count = 0usize;
            for rel in &relationships {
                let id = rel["id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("relationship missing id"))?;
                let source_id = rel["source_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("relationship missing source_id"))?;
                let target_id = rel["target_id"]
                    .as_str()
                    .ok_or_else(|| anyhow!("relationship missing target_id"))?;
                let rel_type = rel["rel_type"]
                    .as_str()
                    .ok_or_else(|| anyhow!("relationship missing rel_type"))?;
                let weight = rel["weight"].as_f64().unwrap_or(1.0);
                let metadata = serde_json::to_string(&rel["metadata"])
                    .unwrap_or_else(|_| "{}".to_string());
                let created_at = rel["created_at"].as_str();

                tx.execute(
                    "INSERT OR REPLACE INTO relationships (id, source_id, target_id, rel_type, weight, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, COALESCE(?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
                    params![id, source_id, target_id, rel_type, weight, metadata, created_at],
                )
                .context("failed to import relationship")?;

                rel_count += 1;
            }

            tx.commit()
                .context("failed to commit import transaction")?;
            Ok::<_, anyhow::Error>((mem_count, rel_count))
        })
        .await
        .context("spawn_blocking join error")?
    }

    #[cfg(test)]
    pub fn new_in_memory() -> Result<Self> {
        Self::new_in_memory_with_embedder(Arc::new(crate::memory_core::PlaceholderEmbedder))
    }

    #[cfg(test)]
    pub fn new_in_memory_with_embedder(embedder: Arc<dyn Embedder>) -> Result<Self> {
        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            embedder,
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

    #[cfg(test)]
    fn debug_get_access_count(&self, id: &str) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

        let value: Option<i64> = conn
            .query_row(
                "SELECT access_count FROM memories WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .context("failed to query access_count")?;

        value.ok_or_else(|| anyhow!("memory not found for id={id}"))
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        let tags_json =
            serde_json::to_string(&input.tags).context("failed to serialize tags to JSON")?;
        let metadata_json = serde_json::to_string(&input.metadata)
            .context("failed to serialize metadata to JSON")?;

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();
        let data = data.to_string();
        let importance = input.importance;
        let event_type = input.event_type.clone();
        let session_id = input.session_id.clone();
        let project = input.project.clone();
        let priority = input.priority;
        let entity_id = input.entity_id.clone();
        let agent_type = input.agent_type.clone();

        tokio::task::spawn_blocking(move || {
            let mut hasher = Sha256::new();
            hasher.update(data.as_bytes());
            let content_hash = format!("{:x}", hasher.finalize());
            let embedding = serde_json::to_vec(&embedder.embed(&data)?)
                .context("failed to serialize embedding")?;
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
                    tags,
                    importance,
                    metadata,
                    session_id,
                    event_type,
                    project,
                    priority,
                    entity_id,
                    agent_type
                ) VALUES (
                    ?1,
                    ?2,
                    ?3,
                    NULL,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?4,
                    'cli_input',
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?5,
                    ?6,
                    ?7,
                    ?8,
                    ?9,
                    ?10,
                    ?11,
                    ?12,
                    ?13
                )
                ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    embedding = excluded.embedding,
                    content_hash = excluded.content_hash,
                    source_type = excluded.source_type,
                    tags = excluded.tags,
                    importance = excluded.importance,
                    metadata = excluded.metadata,
                    session_id = excluded.session_id,
                    event_type = excluded.event_type,
                    project = excluded.project,
                    priority = excluded.priority,
                    entity_id = excluded.entity_id,
                    agent_type = excluded.agent_type,
                    last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    id,
                    data,
                    embedding,
                    content_hash,
                    tags_json,
                    importance,
                    metadata_json,
                    session_id,
                    event_type,
                    project,
                    priority,
                    entity_id,
                    agent_type
                ],
            )
            .context("failed to insert memory")?;

            tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                .context("failed to delete existing FTS row during store")?;
            tx.execute(
                "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                params![id, data],
            )
            .context("failed to insert FTS row during store")?;

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
                "UPDATE memories
                 SET
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     access_count = access_count + 1
                 WHERE id = ?1",
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
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let query = query.to_string();
        let effective_limit = i64::try_from(limit).context("search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let fts_query = build_fts5_query(&query);
            let mut fts_sql = String::from(
                "SELECT f.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project
                 FROM memories_fts f
                 JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1",
            );
            let mut fts_params: Vec<SqlValue> = vec![SqlValue::Text(fts_query)];
            let mut param_idx = 2;
            if let Some(event_type) = opts.event_type.clone() {
                fts_sql.push_str(&format!(" AND m.event_type = ?{param_idx}"));
                fts_params.push(SqlValue::Text(event_type));
                param_idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                fts_sql.push_str(&format!(" AND m.project = ?{param_idx}"));
                fts_params.push(SqlValue::Text(project));
                param_idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                fts_sql.push_str(&format!(" AND m.session_id = ?{param_idx}"));
                fts_params.push(SqlValue::Text(session_id));
                param_idx += 1;
            }
            fts_sql.push_str(" ORDER BY bm25(memories_fts)");
            fts_sql.push_str(&format!(" LIMIT ?{param_idx}"));
            fts_params.push(SqlValue::Integer(effective_limit));

            let fts_result = conn.prepare(&fts_sql);

            if let Ok(mut stmt) = fts_result {
                let mut fts_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                for value in &fts_params {
                    fts_param_refs.push(value);
                }
                let rows = stmt.query_map(fts_param_refs.as_slice(), |row| {
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: row.get(5).ok(),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
                    })
                });

                if let Ok(rows) = rows {
                    let mut results = Vec::new();
                    for row in rows {
                        results.push(row.context("failed to decode FTS5 search row")?);
                    }

                    if !results.is_empty() {
                        return Ok(results);
                    }
                }
            }

            let escaped = query
                .to_lowercase()
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE lower(content) LIKE ?1 ESCAPE '\\'",
            );
            let mut params_values: Vec<SqlValue> = vec![SqlValue::Text(pattern)];
            let mut idx = 2;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
                idx += 1;
            }
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare LIKE search query")?;

            let mut like_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                like_param_refs.push(value);
            }

            let rows = stmt
                .query_map(like_param_refs.as_slice(), |row| {
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: row.get(5).ok(),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
                    })
                })
                .context("failed to execute LIKE search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode LIKE search row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Recents for SqliteStorage {
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let effective_limit = i64::try_from(limit).context("recent limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE 1 = 1",
            );
            let mut params_values: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
                idx += 1;
            }
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare recent query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: row.get(5).ok(),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
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
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let query = query.to_string();
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let query_embedding = embedder
                .embed(&query)
                .context("failed to compute query embedding")?;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut sql = String::from(
                "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE embedding IS NOT NULL",
            );
            let mut params_values: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                sql.push_str(&format!(" AND event_type = ?{idx}"));
                params_values.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                sql.push_str(&format!(" AND session_id = ?{idx}"));
                params_values.push(SqlValue::Text(session_id));
            }

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare semantic search query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    let id: String = row.get(0)?;
                    let content: String = row.get(1)?;
                    let embedding_blob: Vec<u8> = row.get(2)?;
                    let tags: String = row.get(3)?;
                    let importance: f64 = row.get(4)?;
                    let metadata: String = row.get(5)?;
                    let event_type: Option<String> = row.get(6).ok();
                    let session_id: Option<String> = row.get(7).ok();
                    let project: Option<String> = row.get(8).ok();
                    Ok((
                        id,
                        content,
                        embedding_blob,
                        tags,
                        importance,
                        metadata,
                        event_type,
                        session_id,
                        project,
                    ))
                })
                .context("failed to execute semantic search query")?;

            let mut ranked = Vec::new();
            for row in rows {
                let (
                    id,
                    content,
                    embedding_blob,
                    raw_tags,
                    importance,
                    raw_metadata,
                    event_type,
                    session_id,
                    project,
                ) = row.context("failed to decode semantic search row")?;
                let candidate: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                let score = cosine_similarity(&query_embedding, &candidate);
                ranked.push(SemanticResult {
                    id,
                    content,
                    tags: parse_tags_from_db(&raw_tags),
                    importance,
                    metadata: parse_metadata_from_db(&raw_metadata),
                    event_type,
                    session_id,
                    project,
                    score,
                });
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
            conn.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                .context("failed to delete memory from FTS index")?;
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
    async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        if input.content.is_none()
            && input.tags.is_none()
            && input.importance.is_none()
            && input.metadata.is_none()
            && input.event_type.is_none()
            && input.priority.is_none()
        {
            return Err(anyhow!(
                "at least one of content, tags, importance, metadata, event_type, or priority must be provided"
            ));
        }

        let tags_json = input
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).context("failed to serialize tags"))
            .transpose()?;
        let metadata_json = input
            .metadata
            .as_ref()
            .map(|metadata| serde_json::to_string(metadata).context("failed to serialize metadata"))
            .transpose()?;
        let event_type = input.event_type.clone();
        let priority = input.priority;
        let importance = input.importance;
        let content = input.content.clone();

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let content_fields = match content.as_deref() {
                Some(new_content) => {
                    let mut hasher = Sha256::new();
                    hasher.update(new_content.as_bytes());
                    let hash = format!("{:x}", hasher.finalize());
                    let emb = serde_json::to_vec(&embedder.embed(new_content)?)
                        .context("failed to serialize embedding")?;
                    Some((new_content.to_string(), hash, emb))
                }
                None => None,
            };
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut set_clauses = Vec::new();
            let mut values: Vec<SqlValue> = Vec::new();
            let mut next_param_index = 2;

            if let Some((new_content, hash, embedding)) = &content_fields {
                set_clauses.push(format!("content = ?{next_param_index}"));
                values.push(SqlValue::Text(new_content.clone()));
                next_param_index += 1;

                set_clauses.push(format!("content_hash = ?{next_param_index}"));
                values.push(SqlValue::Text(hash.clone()));
                next_param_index += 1;

                set_clauses.push(format!("embedding = ?{next_param_index}"));
                values.push(SqlValue::Blob(embedding.clone()));
                next_param_index += 1;
            }

            if let Some(new_tags) = &tags_json {
                set_clauses.push(format!("tags = ?{next_param_index}"));
                values.push(SqlValue::Text(new_tags.clone()));
                next_param_index += 1;
            }

            if let Some(new_importance) = importance {
                set_clauses.push(format!("importance = ?{next_param_index}"));
                values.push(SqlValue::Real(new_importance));
                next_param_index += 1;
            }

            if let Some(new_metadata) = &metadata_json {
                set_clauses.push(format!("metadata = ?{next_param_index}"));
                values.push(SqlValue::Text(new_metadata.clone()));
                next_param_index += 1;
            }

            if let Some(new_event_type) = &event_type {
                set_clauses.push(format!("event_type = ?{next_param_index}"));
                values.push(SqlValue::Text(new_event_type.clone()));
                next_param_index += 1;
            }

            if let Some(new_priority) = priority {
                set_clauses.push(format!("priority = ?{next_param_index}"));
                values.push(SqlValue::Integer(i64::from(new_priority)));
            }

            let sql = format!(
                "UPDATE memories SET {},
                 last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                set_clauses.join(", ")
            );

            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(values.len() + 1);
            params.push(&id);
            for value in &values {
                params.push(value);
            }

            let changes = conn
                .execute(&sql, params.as_slice())
                .context("failed to update memory")?;

            if changes == 0 {
                return Err(anyhow!("memory not found for id={id}"));
            }

            if let Some((new_content, _, _)) = &content_fields {
                conn.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to delete existing FTS row during update")?;
                conn.execute(
                    "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                    params![id, new_content],
                )
                .context("failed to insert FTS row during update")?;
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
    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if tags.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let tags = tags.to_vec();
        let effective_limit = i64::try_from(limit).context("tag search limit exceeds i64")?;
        let opts = opts.clone();

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
            let json_clause = json_conditions.join(" AND ");
            let csv_clause = csv_conditions.join(" AND ");
            let mut sql = format!(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project FROM memories \
                 WHERE ((json_valid(memories.tags) AND {json_clause}) \
                         OR (NOT json_valid(memories.tags) AND memories.tags != '' AND {csv_clause})) \
                 "
            );

            let mut next_idx = param_values.len();
            if let Some(event_type) = opts.event_type.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND event_type = ?{next_idx}"));
                param_values.push(event_type);
            }
            if let Some(project) = opts.project.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND project = ?{next_idx}"));
                param_values.push(project);
            }
            if let Some(session_id) = opts.session_id.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND session_id = ?{next_idx}"));
                param_values.push(session_id);
            }
            next_idx += 1;
            sql.push_str(&format!(" ORDER BY last_accessed_at DESC LIMIT ?{next_idx}"));

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
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: row.get(5).ok(),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
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
    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult> {
        let opts = opts.clone();
        if limit == 0 {
            let conn = Arc::clone(&self.conn);
            let count_opts = opts.clone();
            let total = tokio::task::spawn_blocking(move || {
                use rusqlite::types::Value as SqlValue;

                let conn = conn
                    .lock()
                    .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
                let mut sql = String::from("SELECT COUNT(*) FROM memories WHERE 1 = 1");
                let mut params_values: Vec<SqlValue> = Vec::new();
                let mut idx = 1;
                if let Some(event_type) = count_opts.event_type {
                    sql.push_str(&format!(" AND event_type = ?{idx}"));
                    params_values.push(SqlValue::Text(event_type));
                    idx += 1;
                }
                if let Some(project) = count_opts.project {
                    sql.push_str(&format!(" AND project = ?{idx}"));
                    params_values.push(SqlValue::Text(project));
                    idx += 1;
                }
                if let Some(session_id) = count_opts.session_id {
                    sql.push_str(&format!(" AND session_id = ?{idx}"));
                    params_values.push(SqlValue::Text(session_id));
                }
                let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                for value in &params_values {
                    param_refs.push(value);
                }
                let mut stmt = conn
                    .prepare(&sql)
                    .context("failed to prepare list count query")?;
                let count: i64 = stmt
                    .query_row(param_refs.as_slice(), |row| row.get(0))
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
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut count_sql = String::from("SELECT COUNT(*) FROM memories WHERE 1 = 1");
            let mut filter_params: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                count_sql.push_str(&format!(" AND event_type = ?{idx}"));
                filter_params.push(SqlValue::Text(event_type));
                idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                count_sql.push_str(&format!(" AND project = ?{idx}"));
                filter_params.push(SqlValue::Text(project));
                idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                count_sql.push_str(&format!(" AND session_id = ?{idx}"));
                filter_params.push(SqlValue::Text(session_id));
            }

            let mut count_stmt = conn
                .prepare(&count_sql)
                .context("failed to prepare list count query")?;
            let mut count_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &filter_params {
                count_param_refs.push(value);
            }
            let total: i64 = count_stmt
                .query_row(count_param_refs.as_slice(), |row| row.get(0))
                .context("failed to count memories")?;

            let mut data_sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project FROM memories WHERE 1 = 1",
            );
            let mut data_params: Vec<SqlValue> = Vec::new();
            let mut next_idx = 1;
            if let Some(event_type) = opts.event_type.clone() {
                data_sql.push_str(&format!(" AND event_type = ?{next_idx}"));
                data_params.push(SqlValue::Text(event_type));
                next_idx += 1;
            }
            if let Some(project) = opts.project.clone() {
                data_sql.push_str(&format!(" AND project = ?{next_idx}"));
                data_params.push(SqlValue::Text(project));
                next_idx += 1;
            }
            if let Some(session_id) = opts.session_id.clone() {
                data_sql.push_str(&format!(" AND session_id = ?{next_idx}"));
                data_params.push(SqlValue::Text(session_id));
                next_idx += 1;
            }
            data_sql.push_str(" ORDER BY created_at DESC");
            data_sql.push_str(&format!(" LIMIT ?{next_idx}"));
            data_params.push(SqlValue::Integer(effective_limit));
            next_idx += 1;
            data_sql.push_str(&format!(" OFFSET ?{next_idx}"));
            data_params.push(SqlValue::Integer(effective_offset));

            let mut stmt = conn
                .prepare(&data_sql)
                .context("failed to prepare list query")?;

            let mut data_param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &data_params {
                data_param_refs.push(value);
            }

            let rows = stmt
                .query_map(data_param_refs.as_slice(), |row| {
                    let raw_tags: String = row.get(2)?;
                    let raw_metadata: String = row.get(4)?;
                    Ok(SearchResult {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        tags: parse_tags_from_db(&raw_tags),
                        importance: row.get(3)?,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: row.get(5).ok(),
                        session_id: row.get(6).ok(),
                        project: row.get(7).ok(),
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
                    "SELECT id, source_id, target_id, rel_type, weight, metadata, created_at
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
                        weight: row.get::<_, f64>(4).unwrap_or(1.0),
                        metadata: parse_metadata_from_db(
                            &row.get::<_, String>(5).unwrap_or_else(|_| "{}".to_string()),
                        ),
                        created_at: row.get::<_, String>(6).unwrap_or_else(|_| "".to_string()),
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
            tags TEXT NOT NULL DEFAULT '[]',
            importance REAL NOT NULL DEFAULT 0.5,
            metadata TEXT NOT NULL DEFAULT '{}',
            access_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS relationships (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            rel_type TEXT NOT NULL,
            FOREIGN KEY(source_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY(target_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            id UNINDEXED,
            content,
            tokenize='unicode61'
        );",
    )
    .context("failed to initialize sqlite schema")?;

    let new_columns = [
        "ALTER TABLE memories ADD COLUMN session_id TEXT",
        "ALTER TABLE memories ADD COLUMN event_type TEXT",
        "ALTER TABLE memories ADD COLUMN project TEXT",
        "ALTER TABLE memories ADD COLUMN priority INTEGER",
        "ALTER TABLE memories ADD COLUMN entity_id TEXT",
        "ALTER TABLE memories ADD COLUMN agent_type TEXT",
    ];
    for alter in &new_columns {
        let _ = conn.execute_batch(alter);
    }

    let new_rel_columns = [
        "ALTER TABLE relationships ADD COLUMN weight REAL NOT NULL DEFAULT 1.0",
        "ALTER TABLE relationships ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE relationships ADD COLUMN created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
    ];
    for alter in &new_rel_columns {
        let _ = conn.execute_batch(alter);
    }

    rebuild_fts_index(conn)?;

    Ok(())
}

fn rebuild_fts_index(conn: &Connection) -> Result<()> {
    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories_fts", [], |row| row.get(0))
        .context("failed to count FTS5 rows")?;
    let mem_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .context("failed to count memory rows")?;

    if mem_count > 0 && fts_count == 0 {
        conn.execute_batch(
            "INSERT INTO memories_fts(id, content) SELECT id, content FROM memories",
        )
        .context("failed to rebuild FTS5 index from existing data")?;
    }

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

fn parse_tags_from_db(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

fn parse_metadata_from_db(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()))
}

fn build_fts5_query(input: &str) -> String {
    let tokens: Vec<String> = input
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();

    if tokens.is_empty() {
        return "\"\"".to_string();
    }

    tokens.join(" ")
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::{
        Recents, Retriever, SearchOptions, Searcher, SemanticSearcher, Storage, Updater,
        default_priority_for_event_type, is_valid_event_type,
    };

    #[test]
    fn test_new_with_path_creates_parent_and_db() {
        let base = std::env::temp_dir().join(format!("romega-sqlite-test-{}", Uuid::new_v4()));
        let db_path = base.join("nested").join("memory.db");

        let storage = SqliteStorage::new_with_path(
            db_path.clone(),
            std::sync::Arc::new(crate::memory_core::PlaceholderEmbedder),
        );
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
            "importance",
            "metadata",
            "access_count",
            "session_id",
            "event_type",
            "project",
            "priority",
            "entity_id",
            "agent_type",
        ] {
            assert!(memories_cols.iter().any(|c| c == col));
        }

        let relationships_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(relationships)").unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
            rows.map(|r| r.unwrap()).collect()
        };

        for col in [
            "id",
            "source_id",
            "target_id",
            "rel_type",
            "weight",
            "metadata",
            "created_at",
        ] {
            assert!(relationships_cols.iter().any(|c| c == col));
        }
    }

    #[test]
    fn test_schema_contains_fts5_table() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let conn = storage
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_store_and_retrieve_roundtrip() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "m1",
            "hello world",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let content = storage.retrieve("m1").await.unwrap();

        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_retrieve_updates_last_accessed_at() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "m2",
            "payload",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
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
        <SqliteStorage as Storage>::store(
            &storage,
            "a",
            "alpha",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "b",
            "beta",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let rel_id = storage
            .add_relationship("a", "b", "links_to", 1.0, &serde_json::json!({}))
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
        <SqliteStorage as Storage>::store(
            &storage,
            "s1",
            "Rust memory store",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "s2",
            "another note",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("MEMORY", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s1");
        assert_eq!(results[0].content, "Rust memory store");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_search_treats_like_wildcards_as_literals() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "p1",
            "value 100% done",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "p2",
            "value 1000 done",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("100%", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "p1");
    }

    #[tokio::test]
    async fn test_search_with_zero_limit_returns_no_results() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "z1",
            "zero limit candidate",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("zero", 0, &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_escapes_underscore_and_backslash_literals() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "u1",
            r"file_a\b",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "u2",
            r"fileXab",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let underscore_results = storage
            .search("file_a", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(underscore_results.len(), 1);
        assert_eq!(underscore_results[0].id, "u1");

        let backslash_results = storage
            .search(r"a\b", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(backslash_results.len(), 1);
        assert_eq!(backslash_results[0].id, "u1");
    }

    #[tokio::test]
    async fn test_fts5_search_basic() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts1",
            "Rust memory management",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("memory", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "fts1");
    }

    #[tokio::test]
    async fn test_fts5_search_multiple_terms() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts2",
            "rust memory ownership",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts3",
            "rust tooling",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("rust memory", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "fts2");
    }

    #[tokio::test]
    async fn test_fts5_search_returns_importance_metadata() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts4",
            "fts metadata payload",
            &MemoryInput {
                tags: vec!["alpha".to_string()],
                importance: 0.91,
                metadata: serde_json::json!({"scope":"fts"}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("metadata", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "fts4");
        assert_eq!(results[0].tags, vec!["alpha".to_string()]);
        assert_eq!(results[0].importance, 0.91);
        assert_eq!(results[0].metadata, serde_json::json!({"scope":"fts"}));
    }

    #[tokio::test]
    async fn test_fts5_fallback_to_like() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts5",
            "value with % symbol and _ marker",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let percent_results = storage
            .search("%", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(percent_results.len(), 1);
        assert_eq!(percent_results[0].id, "fts5");

        let underscore_results = storage
            .search("_", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(underscore_results.len(), 1);
        assert_eq!(underscore_results[0].id, "fts5");
    }

    #[tokio::test]
    async fn test_fts5_sync_on_update() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts6",
            "before update",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "fts6",
            &MemoryUpdate {
                content: Some("after update".to_string()),
                tags: None,
                importance: None,
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("after", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "fts6");
    }

    #[tokio::test]
    async fn test_fts5_sync_on_delete() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fts7",
            "delete candidate",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let deleted = storage.delete("fts7").await.unwrap();
        assert!(deleted);

        let results = storage
            .search("candidate", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_recent_returns_most_recently_accessed_first() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "r1",
            "older",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "r2",
            "newer",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        storage
            .debug_force_last_accessed_at("r1", "2000-01-01T00:00:00.000Z")
            .unwrap();
        storage
            .debug_force_last_accessed_at("r2", "2001-01-01T00:00:00.000Z")
            .unwrap();

        let results = storage.recent(2, &SearchOptions::default()).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "r2");
        assert_eq!(results[1].id, "r1");
    }

    #[tokio::test]
    async fn test_semantic_search_prefers_exact_text_match() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "e1",
            "alpha beta gamma",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "e2",
            "other content",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .semantic_search("alpha beta gamma", 2, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "e1");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, serde_json::json!({}));
        assert!(results[0].score >= results[1].score);
    }

    #[tokio::test]
    async fn test_semantic_search_zero_limit_returns_no_results() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "e3",
            "candidate",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .semantic_search("candidate", 0, &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // ── Delete tests ──

    #[tokio::test]
    async fn test_delete_existing_memory() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "d1",
            "to-delete",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

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
        <SqliteStorage as Storage>::store(
            &storage,
            "ca",
            "alpha",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "cb",
            "beta",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        storage
            .add_relationship("ca", "cb", "links_to", 1.0, &serde_json::json!({}))
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
        <SqliteStorage as Storage>::store(
            &storage,
            "up1",
            "original",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "up1",
            &MemoryUpdate {
                content: Some("updated".to_string()),
                tags: None,
                importance: None,
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let content = storage.retrieve("up1").await.unwrap();
        assert_eq!(content, "updated");
    }

    #[tokio::test]
    async fn test_update_with_tags() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["a".to_string(), "b".to_string()];
        <SqliteStorage as Storage>::store(
            &storage,
            "up2",
            "data",
            &MemoryInput {
                tags: tags.clone(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let new_tags = vec!["x".to_string()];
        <SqliteStorage as Updater>::update(
            &storage,
            "up2",
            &MemoryUpdate {
                content: Some("data-v2".to_string()),
                tags: Some(new_tags.clone()),
                importance: None,
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .get_by_tags(&new_tags, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up2");
        assert_eq!(results[0].content, "data-v2");
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_update_without_tags_preserves_existing() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["keep".to_string()];
        <SqliteStorage as Storage>::store(
            &storage,
            "up3",
            "data",
            &MemoryInput {
                tags: tags.clone(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "up3",
            &MemoryUpdate {
                content: Some("data-v2".to_string()),
                tags: None,
                importance: None,
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .get_by_tags(&tags, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up3");
    }

    #[tokio::test]
    async fn test_update_tags_only_preserves_content() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "up4",
            "keep-this",
            &MemoryInput {
                tags: vec!["old".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let new_tags = vec!["new-tag".to_string()];
        <SqliteStorage as Updater>::update(
            &storage,
            "up4",
            &MemoryUpdate {
                content: None,
                tags: Some(new_tags.clone()),
                importance: None,
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let content = storage.retrieve("up4").await.unwrap();
        assert_eq!(content, "keep-this");
        let results = storage
            .get_by_tags(&new_tags, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "up4");
    }

    #[tokio::test]
    async fn test_update_neither_content_nor_tags_errors() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "up5",
            "data",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let err =
            <SqliteStorage as Updater>::update(&storage, "up5", &MemoryUpdate::default()).await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn test_update_nonexistent_errors() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let err = <SqliteStorage as Updater>::update(
            &storage,
            "ghost",
            &MemoryUpdate {
                content: Some("data".to_string()),
                ..Default::default()
            },
        )
        .await;
        assert!(err.is_err());
    }

    // ── Tags tests ──

    #[tokio::test]
    async fn test_get_by_tags_filters_correctly() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "t1",
            "one",
            &MemoryInput {
                tags: vec!["rust".to_string(), "memory".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "t2",
            "two",
            &MemoryInput {
                tags: vec!["rust".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "t3",
            "three",
            &MemoryInput {
                tags: vec!["python".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .get_by_tags(&["rust".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        let results = storage
            .get_by_tags(
                &["rust".to_string(), "memory".to_string()],
                10,
                &SearchOptions::default(),
            )
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
        <SqliteStorage as Storage>::store(
            &storage,
            "json1",
            "json data",
            &MemoryInput {
                tags: vec!["rust".to_string(), "search".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Search for 'rust' should find both CSV and JSON rows
        let results = storage
            .get_by_tags(&["rust".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        // Search for 'memory' should find only the CSV row
        let results = storage
            .get_by_tags(&["memory".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "csv1");

        // Search for 'search' should find only the JSON row
        let results = storage
            .get_by_tags(&["search".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "json1");
    }

    #[tokio::test]
    async fn test_get_by_tags_empty_returns_empty() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "te",
            "data",
            &MemoryInput {
                tags: vec!["tag".to_string()],
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let results = storage
            .get_by_tags(&[], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // ── List tests ──

    #[tokio::test]
    async fn test_list_with_pagination() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for i in 0..5 {
            <SqliteStorage as Storage>::store(
                &storage,
                &format!("l{i}"),
                &format!("item-{i}"),
                &MemoryInput {
                    tags: Vec::new(),
                    importance: 0.5,
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let result = storage.list(0, 3, &SearchOptions::default()).await.unwrap();
        assert_eq!(result.memories.len(), 3);
        assert_eq!(result.total, 5);

        let result = storage.list(3, 3, &SearchOptions::default()).await.unwrap();
        assert_eq!(result.memories.len(), 2);
        assert_eq!(result.total, 5);
    }

    #[tokio::test]
    async fn test_list_zero_limit_returns_count_only() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "lz1",
            "a",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "lz2",
            "b",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = storage.list(0, 0, &SearchOptions::default()).await.unwrap();
        assert!(result.memories.is_empty());
        assert_eq!(result.total, 2);
    }

    // ── Relationship query tests ──

    #[tokio::test]
    async fn test_get_relationships_both_directions() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ra",
            "alpha",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rb",
            "beta",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rc",
            "gamma",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        storage
            .add_relationship("ra", "rb", "links_to", 1.0, &serde_json::json!({}))
            .await
            .unwrap();
        storage
            .add_relationship("rc", "ra", "depends_on", 1.0, &serde_json::json!({}))
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
        <SqliteStorage as Storage>::store(
            &storage,
            "lonely",
            "alone",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let rels = storage.get_relationships("lonely").await.unwrap();
        assert!(rels.is_empty());
    }

    // ── Tags roundtrip with store ──

    #[tokio::test]
    async fn test_store_with_tags_roundtrip() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["project-x".to_string(), "important".to_string()];
        <SqliteStorage as Storage>::store(
            &storage,
            "st1",
            "tagged content",
            &MemoryInput {
                tags: tags.clone(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .get_by_tags(&["project-x".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "st1");
        assert_eq!(results[0].content, "tagged content");
        assert_eq!(results[0].tags, tags);
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_store_with_importance_and_metadata() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let metadata = serde_json::json!({"key":"val"});

        <SqliteStorage as Storage>::store(
            &storage,
            "im1",
            "priority note",
            &MemoryInput {
                tags: vec!["ranked".to_string()],
                importance: 0.9,
                metadata: metadata.clone(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("priority", 5, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "im1");
        assert_eq!(results[0].importance, 0.9);
        assert_eq!(results[0].metadata, metadata);
        assert_eq!(results[0].tags, vec!["ranked".to_string()]);
    }

    #[tokio::test]
    async fn test_update_importance_only() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["persist".to_string()];
        <SqliteStorage as Storage>::store(
            &storage,
            "im2",
            "keep me",
            &MemoryInput {
                tags: tags.clone(),
                importance: 0.5,
                metadata: serde_json::json!({"scope":"base"}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "im2",
            &MemoryUpdate {
                content: None,
                tags: None,
                importance: Some(0.88),
                metadata: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("keep me", 1, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "keep me");
        assert_eq!(results[0].tags, tags);
        assert_eq!(results[0].importance, 0.88);
        assert_eq!(results[0].metadata, serde_json::json!({"scope":"base"}));
    }

    #[tokio::test]
    async fn test_update_metadata_only() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let tags = vec!["persist".to_string()];
        <SqliteStorage as Storage>::store(
            &storage,
            "im3",
            "keep metadata",
            &MemoryInput {
                tags: tags.clone(),
                importance: 0.6,
                metadata: serde_json::json!({"v":1}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let updated_metadata = serde_json::json!({"v":2, "extra":"ok"});
        <SqliteStorage as Updater>::update(
            &storage,
            "im3",
            &MemoryUpdate {
                content: None,
                tags: None,
                importance: None,
                metadata: Some(updated_metadata.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("keep metadata", 1, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].content, "keep metadata");
        assert_eq!(results[0].tags, tags);
        assert_eq!(results[0].importance, 0.6);
        assert_eq!(results[0].metadata, updated_metadata);
    }

    #[tokio::test]
    async fn test_retrieve_increments_access_count() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "ac1",
            "read me",
            &MemoryInput {
                tags: Vec::new(),
                importance: 0.5,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let _ = storage.retrieve("ac1").await.unwrap();
        let _ = storage.retrieve("ac1").await.unwrap();

        let access_count = storage.debug_get_access_count("ac1").unwrap();
        assert_eq!(access_count, 2);
    }

    #[tokio::test]
    async fn test_search_returns_importance_and_metadata() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "im4",
            "search payload",
            &MemoryInput {
                tags: vec!["alpha".to_string(), "beta".to_string()],
                importance: 0.77,
                metadata: serde_json::json!({"team":"memory"}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search("payload", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "im4");
        assert_eq!(
            results[0].tags,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(results[0].importance, 0.77);
        assert_eq!(results[0].metadata, serde_json::json!({"team":"memory"}));
    }

    #[tokio::test]
    async fn test_default_importance_is_half() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, content_hash, source_type, tags)
                 VALUES ('defimp', 'default importance', 'hash-default', 'test', '[]')",
                [],
            )
            .unwrap();
        }

        let results = storage
            .search("default importance", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "defimp");
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_store_with_event_type_and_session() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "evt1",
            "event scoped",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                session_id: Some("ses_1".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .search(
                "event scoped",
                5,
                &SearchOptions {
                    session_id: Some("ses_1".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type.as_deref(), Some("decision"));
        assert_eq!(results[0].session_id.as_deref(), Some("ses_1"));
    }

    #[tokio::test]
    async fn test_store_with_project_and_priority() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "evt2",
            "project scoped",
            &MemoryInput {
                project: Some("myproj".to_string()),
                priority: Some(3),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.conn.lock().unwrap();
        let got: (Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT project, priority FROM memories WHERE id='evt2'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(got.0.as_deref(), Some("myproj"));
        assert_eq!(got.1, Some(3));
    }

    #[tokio::test]
    async fn test_search_filter_by_event_type() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, event_type) in [("f1", "decision"), ("f2", "reminder")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                "same query",
                &MemoryInput {
                    event_type: Some(event_type.to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let results = storage
            .search(
                "same query",
                10,
                &SearchOptions {
                    event_type: Some("decision".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "f1");
    }

    #[tokio::test]
    async fn test_search_filter_by_project() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, project) in [("p1", "myproj"), ("p2", "other")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                "project query",
                &MemoryInput {
                    project: Some(project.to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let results = storage
            .search(
                "project query",
                10,
                &SearchOptions {
                    project: Some("myproj".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "p1");
    }

    #[tokio::test]
    async fn test_search_filter_by_session_id() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, session_id) in [("s1", "ses_a"), ("s2", "ses_b")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                "session query",
                &MemoryInput {
                    session_id: Some(session_id.to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let results = storage
            .search(
                "session query",
                10,
                &SearchOptions {
                    session_id: Some("ses_a".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s1");
    }

    #[tokio::test]
    async fn test_recent_filter_by_project() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rp1",
            "recent in proj",
            &MemoryInput {
                project: Some("myproj".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rp2",
            "recent out proj",
            &MemoryInput {
                project: Some("other".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .recent(
                10,
                &SearchOptions {
                    project: Some("myproj".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rp1");
    }

    #[tokio::test]
    async fn test_semantic_search_filter_by_event_type() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "se1",
            "semantic token",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "se2",
            "semantic token",
            &MemoryInput {
                event_type: Some("reminder".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .semantic_search(
                "semantic token",
                10,
                &SearchOptions {
                    event_type: Some("decision".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "se1");
    }

    #[tokio::test]
    async fn test_list_filter_by_project() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "lp1",
            "list1",
            &MemoryInput {
                project: Some("myproj".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "lp2",
            "list2",
            &MemoryInput {
                project: Some("other".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = storage
            .list(
                0,
                10,
                &SearchOptions {
                    project: Some("myproj".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.memories.len(), 1);
        assert_eq!(result.memories[0].id, "lp1");
    }

    #[tokio::test]
    async fn test_update_event_type() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ue1",
            "updatable",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "ue1",
            &MemoryUpdate {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = storage
            .search(
                "updatable",
                1,
                &SearchOptions {
                    event_type: Some("decision".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
    }

    #[tokio::test]
    async fn test_update_priority() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "upprio",
            "priority target",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "upprio",
            &MemoryUpdate {
                priority: Some(4),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.conn.lock().unwrap();
        let priority: Option<i64> = conn
            .query_row(
                "SELECT priority FROM memories WHERE id='upprio'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(priority, Some(4));
    }

    #[test]
    fn test_valid_event_types() {
        assert!(is_valid_event_type("decision"));
        assert!(is_valid_event_type("error_pattern"));
        assert!(!is_valid_event_type("unknown_event_type"));
    }

    #[test]
    fn test_default_priority_for_event_type() {
        assert_eq!(default_priority_for_event_type("error_pattern"), 4);
        assert_eq!(default_priority_for_event_type("decision"), 3);
        assert_eq!(default_priority_for_event_type("git_merge"), 2);
        assert_eq!(default_priority_for_event_type("session_summary"), 1);
        assert_eq!(default_priority_for_event_type("not_real"), 0);
    }

    #[tokio::test]
    async fn test_relationship_with_weight_and_metadata() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rw1",
            "a",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rw2",
            "b",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        storage
            .add_relationship("rw1", "rw2", "links_to", 0.7, &serde_json::json!({"k":"v"}))
            .await
            .unwrap();

        let rels = storage.get_relationships("rw1").await.unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].weight, 0.7);
        assert_eq!(rels[0].metadata, serde_json::json!({"k":"v"}));
    }

    #[tokio::test]
    async fn test_export_includes_new_fields() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ex1",
            "export me",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                session_id: Some("ses_export".to_string()),
                project: Some("proj_export".to_string()),
                priority: Some(3),
                entity_id: Some("ent_1".to_string()),
                agent_type: Some("assistant".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        storage
            .add_relationship("ex1", "ex1", "self", 0.5, &serde_json::json!({"a":1}))
            .await
            .unwrap();

        let export = storage.export_all().await.unwrap();
        assert!(export.contains("session_id"));
        assert!(export.contains("event_type"));
        assert!(export.contains("project"));
        assert!(export.contains("priority"));
        assert!(export.contains("entity_id"));
        assert!(export.contains("agent_type"));
        assert!(export.contains("weight"));
        assert!(export.contains("created_at"));
    }

    #[tokio::test]
    async fn test_import_with_new_fields() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let data = serde_json::json!({
            "memories": [{
                "id":"imx1",
                "content":"imported",
                "tags":["t"],
                "importance":0.8,
                "metadata":{"m":1},
                "content_hash":"h",
                "source_type":"import",
                "access_count":1,
                "session_id":"ses_i",
                "event_type":"decision",
                "project":"proj_i",
                "priority":4,
                "entity_id":"e1",
                "agent_type":"assistant"
            }],
            "relationships":[{
                "id":"rel_i",
                "source_id":"imx1",
                "target_id":"imx1",
                "rel_type":"self",
                "weight":0.9,
                "metadata":{"x":1}
            }]
        });
        let imported = storage.import_all(&data.to_string()).await.unwrap();
        assert_eq!(imported.0, 1);
        assert_eq!(imported.1, 1);

        let results = storage
            .search(
                "imported",
                5,
                &SearchOptions {
                    event_type: Some("decision".to_string()),
                    project: Some("proj_i".to_string()),
                    session_id: Some("ses_i".to_string()),
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
    }

    #[tokio::test]
    async fn test_tag_search_filter_by_event_type() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "tag_evt_1",
            "tagged",
            &MemoryInput {
                tags: vec!["alpha".to_string()],
                event_type: Some("decision".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "tag_evt_2",
            "tagged",
            &MemoryInput {
                tags: vec!["alpha".to_string()],
                event_type: Some("reminder".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = storage
            .get_by_tags(
                &["alpha".to_string()],
                10,
                &SearchOptions {
                    event_type: Some("decision".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "tag_evt_1");
    }

    #[tokio::test]
    async fn test_search_empty_opts_returns_all() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for id in ["all_1", "all_2"] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                "same",
                &MemoryInput {
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        let results = storage
            .search("same", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_schema_contains_new_columns() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let conn = storage.conn.lock().unwrap();
        let memories_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        for col in [
            "session_id",
            "event_type",
            "project",
            "priority",
            "entity_id",
            "agent_type",
        ] {
            assert!(memories_cols.contains(&col.to_string()));
        }

        let rel_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(relationships)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        for col in ["weight", "metadata", "created_at"] {
            assert!(rel_cols.contains(&col.to_string()));
        }
    }
}
