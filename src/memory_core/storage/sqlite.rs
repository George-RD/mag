use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::memory_core::{
    AdvancedSearcher, Deleter, ExpirationSweeper, FeedbackRecorder, GraphNode, GraphTraverser,
    ListResult, Lister, MemoryInput, MemoryUpdate, PhraseSearcher, Recents, Relationship,
    RelationshipQuerier, Retriever, SearchOptions, SearchResult, Searcher, SemanticResult,
    SemanticSearcher, SimilarFinder, Storage, Tagger, Updater, embedder::Embedder,
    jaccard_similarity, priority_factor, time_decay, type_weight, word_overlap,
};

const DEDUP_THRESHOLDS: &[(&str, f64)] = &[
    ("error_pattern", 0.70),
    ("session_summary", 0.95),
    ("task_completion", 0.85),
    ("decision", 0.85),
    ("lesson_learned", 0.85),
    ("checkpoint", 0.90),
];

#[derive(Debug)]
enum StoreOutcome {
    Inserted,
    Deduped,
}

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
                            created_at, event_at, content_hash, canonical_hash, source_type, last_accessed_at,
                            access_count, session_id, event_type, project, priority, entity_id, agent_type, ttl_seconds
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
                    let canonical_hash: Option<String> = row.get(10).ok();
                    let source_type: String = row.get(11)?;
                    let last_accessed_at: String = row.get(12)?;
                    let access_count: i64 = row.get(13)?;
                    let session_id: Option<String> = row.get(14).ok();
                    let event_type: Option<String> = row.get(15).ok();
                    let project: Option<String> = row.get(16).ok();
                    let priority: Option<i64> = row.get(17).ok();
                    let entity_id: Option<String> = row.get(18).ok();
                    let agent_type: Option<String> = row.get(19).ok();
                    let ttl_seconds: Option<i64> = row.get(20).ok();
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
                        "canonical_hash": canonical_hash,
                        "source_type": source_type,
                        "last_accessed_at": last_accessed_at,
                        "access_count": access_count,
                        "session_id": session_id,
                        "event_type": event_type,
                        "project": project,
                        "priority": priority,
                        "entity_id": entity_id,
                        "agent_type": agent_type,
                        "ttl_seconds": ttl_seconds,
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
                let canonical_hash_value = mem["canonical_hash"]
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| canonical_hash(content));
                let source_type = mem["source_type"].as_str().unwrap_or("import");
                let access_count = mem["access_count"].as_i64().unwrap_or(0);
                let session_id = mem["session_id"].as_str();
                let event_type = mem["event_type"].as_str();
                let project = mem["project"].as_str();
                let priority = mem["priority"].as_i64();
                let entity_id = mem["entity_id"].as_str();
                let agent_type = mem["agent_type"].as_str();
                let ttl_seconds = mem["ttl_seconds"].as_i64();

                tx.execute(
                    "INSERT OR REPLACE INTO memories (
                        id, content, content_hash, source_type, tags, importance, metadata, access_count,
                        session_id, event_type, project, priority, entity_id, agent_type, ttl_seconds, canonical_hash
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                        ttl_seconds,
                        canonical_hash_value,
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

    async fn try_auto_relate(&self, memory_id: &str) -> Result<()> {
        let conn = Arc::clone(&self.conn);
        let memory_id = memory_id.to_string();
        let source_id_for_query = memory_id.clone();

        let similar_ids = tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let source_embedding: Vec<u8> = conn
                .query_row(
                    "SELECT embedding FROM memories WHERE id = ?1",
                    params![source_id_for_query],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query source embedding for auto relate")?
                .ok_or_else(|| anyhow!("memory not found for auto relate"))?;
            let source_embedding: Vec<f32> = serde_json::from_slice(&source_embedding)
                .context("failed to decode source embedding for auto relate")?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, embedding FROM memories WHERE embedding IS NOT NULL AND id != ?1
                     AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                     ORDER BY created_at DESC LIMIT 100",
                )
                .context("failed to prepare auto relate query")?;

            let rows = stmt
                .query_map(params![source_id_for_query], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
                })
                .context("failed to execute auto relate query")?;

            let mut ranked = Vec::new();
            for row in rows {
                let (id, embedding_blob) = row.context("failed to decode auto relate row")?;
                let embedding: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode candidate embedding for auto relate")?;
                let score = cosine_similarity(&source_embedding, &embedding);
                if score >= 0.45 {
                    ranked.push((id, score));
                }
            }

            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            ranked.truncate(3);

            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")??;

        for (target_id, score) in similar_ids {
            self.add_relationship(
                &memory_id,
                &target_id,
                "related",
                f64::from(score),
                &serde_json::json!({}),
            )
            .await?;
        }

        Ok(())
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
        let ttl_seconds = input.ttl_seconds;
        let id_for_store = id.clone();

        let outcome = tokio::task::spawn_blocking(move || {
            let mut hasher = Sha256::new();
            hasher.update(data.as_bytes());
            let content_hash = format!("{:x}", hasher.finalize());
            let normalized_hash = canonical_hash(&data);
            let embedding = serde_json::to_vec(&embedder.embed(&data)?)
                .context("failed to serialize embedding")?;
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sqlite transaction")?;

            let existing_canonical_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM memories
                     WHERE canonical_hash = ?1
                       AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                     LIMIT 1",
                    params![normalized_hash],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query canonical hash dedup")?;

            if let Some(existing_id) = existing_canonical_id {
                tx.execute(
                    "UPDATE memories
                     SET access_count = access_count + 1,
                         last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![existing_id],
                )
                .context("failed to update access_count for canonical dedup")?;
                tx.commit().context("failed to commit canonical dedup")?;
                return Ok::<_, anyhow::Error>(StoreOutcome::Deduped);
            }

            if let Some(ref event_type_value) = event_type {
                let threshold = DEDUP_THRESHOLDS
                    .iter()
                    .find(|(kind, _)| kind == &event_type_value.as_str())
                    .map(|(_, threshold)| *threshold);

                if let Some(threshold) = threshold {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id, content FROM memories WHERE event_type = ?1
                             AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                             ORDER BY created_at DESC LIMIT 5",
                        )
                        .context("failed to prepare Jaccard dedup query")?;
                    let rows = stmt
                        .query_map(params![event_type_value], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })
                        .context("failed to execute Jaccard dedup query")?;

                    let mut matched_id: Option<String> = None;
                    for row in rows {
                        let (candidate_id, candidate_content) =
                            row.context("failed to decode Jaccard dedup row")?;
                        let similarity = jaccard_similarity(&data, &candidate_content, 3);
                        if similarity >= threshold {
                            matched_id = Some(candidate_id);
                            break;
                        }
                    }

                    if let Some(existing_id) = matched_id {
                        drop(stmt);
                        tx.execute(
                            "UPDATE memories
                             SET access_count = access_count + 1,
                                 last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                             WHERE id = ?1",
                            params![existing_id],
                        )
                        .context("failed to update access_count for Jaccard dedup")?;
                        tx.commit().context("failed to commit Jaccard dedup")?;
                        return Ok::<_, anyhow::Error>(StoreOutcome::Deduped);
                    }
                }
            }

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
                    agent_type,
                    ttl_seconds,
                    canonical_hash
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
                    ?13,
                    ?14,
                    ?15
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
                    ttl_seconds = excluded.ttl_seconds,
                    canonical_hash = excluded.canonical_hash,
                    last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    id_for_store,
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
                    agent_type,
                    ttl_seconds,
                    normalized_hash,
                ],
            )
            .context("failed to insert memory")?;

            tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id_for_store])
                .context("failed to delete existing FTS row during store")?;
            tx.execute(
                "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                params![id_for_store, data],
            )
            .context("failed to insert FTS row during store")?;

            tx.commit().context("failed to commit sqlite transaction")?;
            Ok::<_, anyhow::Error>(StoreOutcome::Inserted)
        })
        .await
        .context("spawn_blocking join error")??;

        if matches!(outcome, StoreOutcome::Inserted)
            && let Err(error) = self.try_auto_relate(&id).await
        {
            tracing::warn!(memory_id = %id, error = %error, "auto-relate failed");
        }

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

#[derive(Debug, Clone)]
struct RankedSemanticCandidate {
    result: SemanticResult,
    created_at: String,
    score: f64,
}

#[async_trait]
impl AdvancedSearcher for SqliteStorage {
    async fn advanced_search(
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
            let query_words_owned: Vec<String> = query
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 2)
                .map(|w| w.to_lowercase())
                .collect();
            let query_word_refs: Vec<&str> = query_words_owned.iter().map(String::as_str).collect();

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut ranked: HashMap<String, RankedSemanticCandidate> = HashMap::new();

            let mut vector_stmt = conn
                .prepare(
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at
                     FROM memories WHERE embedding IS NOT NULL",
                )
                .context("failed to prepare advanced vector query")?;
            let vector_rows = vector_stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, f64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                        row.get::<_, Option<String>>(8).ok().flatten(),
                        row.get::<_, Option<i64>>(9).ok().flatten(),
                        row.get::<_, String>(10)
                            .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".to_string()),
                    ))
                })
                .context("failed to execute advanced vector query")?;

            for row in vector_rows {
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
                    priority,
                    created_at,
                ) = row.context("failed to decode advanced vector row")?;
                let candidate: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                let similarity = cosine_similarity(&query_embedding, &candidate) as f64;
                if similarity < 0.1 {
                    continue;
                }

                let priority_value = resolve_priority(event_type.as_deref(), priority);
                let mut score =
                    similarity * type_weight(event_type.as_deref().unwrap_or("memory"));
                score *= priority_factor(priority_value);

                ranked.insert(
                    id.clone(),
                    RankedSemanticCandidate {
                        result: SemanticResult {
                            id,
                            content,
                            tags: parse_tags_from_db(&raw_tags),
                            importance,
                            metadata: parse_metadata_from_db(&raw_metadata),
                            event_type,
                            session_id,
                            project,
                            score: 0.0,
                        },
                        created_at,
                        score,
                    },
                );
            }

            let fts_query = build_fts5_query(&query);
            let mut fts_sql = String::from(
                "SELECT m.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project, m.priority, m.created_at, bm25(memories_fts)
                 FROM memories_fts
                 JOIN memories m ON m.id = memories_fts.id
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
            }

            if let Ok(mut stmt) = conn.prepare(&fts_sql) {
                let mut refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                for value in &fts_params {
                    refs.push(value);
                }

                let rows = stmt.query_map(refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, f64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5).ok().flatten(),
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                        row.get::<_, Option<i64>>(8).ok().flatten(),
                        row.get::<_, String>(9)
                            .unwrap_or_else(|_| "1970-01-01T00:00:00.000Z".to_string()),
                        row.get::<_, f64>(10).unwrap_or(1.0),
                    ))
                });

                if let Ok(rows) = rows {
                    for row in rows {
                        let (
                            id,
                            content,
                            raw_tags,
                            importance,
                            raw_metadata,
                            event_type,
                            session_id,
                            project,
                            priority,
                            created_at,
                            bm25,
                        ) = row.context("failed to decode advanced FTS row")?;

                        let text_relevance = (1.0 / (1.0 + bm25.abs())).clamp(0.0, 1.0);

                        if let Some(existing) = ranked.get_mut(&id) {
                            existing.score *= 1.3 + text_relevance * 0.5;
                            continue;
                        }

                        let priority_value = resolve_priority(event_type.as_deref(), priority);
                        let mut score = text_relevance
                            * type_weight(event_type.as_deref().unwrap_or("memory"));
                        score *= priority_factor(priority_value);

                        ranked.insert(
                            id.clone(),
                            RankedSemanticCandidate {
                                result: SemanticResult {
                                    id,
                                    content,
                                    tags: parse_tags_from_db(&raw_tags),
                                    importance,
                                    metadata: parse_metadata_from_db(&raw_metadata),
                                    event_type,
                                    session_id,
                                    project,
                                    score: 0.0,
                                },
                                created_at,
                                score,
                            },
                        );
                    }
                }
            }

            for candidate in ranked.values_mut() {
                let with_tags = if candidate.result.tags.is_empty() {
                    candidate.result.content.clone()
                } else {
                    format!("{} {}", candidate.result.content, candidate.result.tags.join(" "))
                };
                let overlap = word_overlap(&query_word_refs, &with_tags);
                candidate.score *= 1.0 + overlap * 0.5;
                let jaccard = jaccard_similarity(&query, &with_tags, 3);
                candidate.score *= 1.0 + jaccard * 0.25;

                candidate.score *= time_decay(&candidate.created_at);
                candidate.score *= 0.5 + candidate.result.importance * 0.5;

                if let Some(context_tags) = opts.context_tags.as_ref() {
                    let candidate_tags: HashSet<String> = candidate
                        .result
                        .tags
                        .iter()
                        .map(|t| t.to_lowercase())
                        .collect();
                    let context_norm: Vec<String> = context_tags
                        .iter()
                        .map(|t| t.to_lowercase())
                        .filter(|t| !t.is_empty())
                        .collect();
                    if !context_norm.is_empty() {
                        let matched = context_norm
                            .iter()
                            .filter(|t| candidate_tags.contains(*t))
                            .count();
                        let ratio = matched as f64 / context_norm.len() as f64;
                        candidate.score *= 1.0 + ratio * 0.25;
                    }
                }
            }

            let mut deduped = Vec::new();
            let mut seen = HashSet::new();
            for candidate in ranked.into_values() {
                if !matches_search_options(&candidate, &opts) {
                    continue;
                }
                let fingerprint = normalize_for_dedup(&candidate.result.content);
                if seen.insert(fingerprint) {
                    deduped.push(candidate);
                }
            }

            if deduped.is_empty() {
                return Ok::<_, anyhow::Error>(Vec::new());
            }

            deduped.sort_by(|a, b| b.score.total_cmp(&a.score));
            let max_score = deduped.first().map(|c| c.score).unwrap_or(0.0);
            let mut out = Vec::new();
            for mut candidate in deduped.into_iter().take(limit) {
                let normalized = if max_score > 0.0 {
                    (candidate.score / max_score).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                candidate.result.score = normalized as f32;
                out.push(candidate.result);
            }
            Ok::<_, anyhow::Error>(out)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl GraphTraverser for SqliteStorage {
    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>> {
        let conn = Arc::clone(&self.conn);
        let start_id = start_id.to_string();
        let hop_limit = max_hops.clamp(1, 5);
        if max_hops > 5 {
            tracing::warn!(requested = max_hops, capped = 5, "max_hops capped to 5");
        }
        let edge_types = edge_types.map(|edges| edges.to_vec());

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut frontier = vec![start_id.clone()];
            let mut visited: HashSet<String> = HashSet::from([start_id.clone()]);
            let mut nodes = Vec::new();

            for hop in 1..=hop_limit {
                if frontier.is_empty() {
                    break;
                }
                let mut next_frontier = Vec::new();

                for current in &frontier {
                    let mut sql = String::from(
                        "SELECT source_id, target_id, rel_type, weight
                         FROM relationships
                         WHERE (source_id = ?1 OR target_id = ?1)
                           AND weight >= ?2",
                    );
                    let mut params_values: Vec<SqlValue> = vec![
                        SqlValue::Text(current.clone()),
                        SqlValue::Real(min_weight),
                    ];
                    if let Some(types) = edge_types.as_ref()
                        && !types.is_empty()
                    {
                        sql.push_str(" AND rel_type IN (");
                        for idx in 0..types.len() {
                            if idx > 0 {
                                sql.push_str(", ");
                            }
                            sql.push_str(&format!("?{}", idx + 3));
                        }
                        sql.push(')');
                        for rel_type in types {
                            params_values.push(SqlValue::Text(rel_type.clone()));
                        }
                    }
                    sql.push_str(" ORDER BY weight DESC");

                    let mut stmt = conn
                        .prepare(&sql)
                        .context("failed to prepare traversal query")?;
                    let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
                    for value in &params_values {
                        param_refs.push(value);
                    }

                    let edges = stmt
                        .query_map(param_refs.as_slice(), |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, String>(2)?,
                                row.get::<_, f64>(3).unwrap_or(1.0),
                            ))
                        })
                        .context("failed to execute traversal query")?;

                    for edge in edges {
                        let (source_id, target_id, rel_type, weight) =
                            edge.context("failed to decode traversal edge")?;
                        let neighbor = if source_id == *current {
                            target_id
                        } else {
                            source_id
                        };

                        if neighbor == start_id || !visited.insert(neighbor.clone()) {
                            continue;
                        }

                        let memory: Option<(String, Option<String>, String, String)> = conn
                            .query_row(
                                "SELECT content, event_type, metadata, created_at FROM memories WHERE id = ?1",
                                params![neighbor],
                                |row| {
                                    Ok((
                                        row.get::<_, String>(0)?,
                                        row.get::<_, Option<String>>(1).ok().flatten(),
                                        row.get::<_, String>(2)
                                            .unwrap_or_else(|_| "{}".to_string()),
                                        row.get::<_, String>(3).unwrap_or_else(|_| {
                                            "1970-01-01T00:00:00.000Z".to_string()
                                        }),
                                    ))
                                },
                            )
                            .optional()
                            .context("failed to fetch neighbor memory")?;

                        if let Some((content, event_type, metadata_raw, created_at)) = memory {
                            next_frontier.push(neighbor.clone());
                            nodes.push(GraphNode {
                                id: neighbor,
                                content,
                                event_type,
                                metadata: parse_metadata_from_db(&metadata_raw),
                                hop,
                                weight,
                                edge_type: rel_type,
                                created_at,
                            });
                        }
                    }
                }

                frontier = next_frontier;
            }

            nodes.sort_by(|a, b| a.hop.cmp(&b.hop).then_with(|| b.weight.total_cmp(&a.weight)));
            Ok::<_, anyhow::Error>(nodes)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl SimilarFinder for SqliteStorage {
    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let memory_id = memory_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let source_embedding: Vec<u8> = conn
                .query_row(
                    "SELECT embedding FROM memories WHERE id = ?1",
                    params![memory_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query source embedding")?
                .ok_or_else(|| anyhow!("memory not found for id={memory_id}"))?;
            let source_embedding: Vec<f32> = serde_json::from_slice(&source_embedding)
                .context("failed to decode source embedding")?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project
                     FROM memories WHERE embedding IS NOT NULL AND id != ?1",
                )
                .context("failed to prepare similar query")?;
            let rows = stmt
                .query_map(params![memory_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Vec<u8>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, f64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                        row.get::<_, Option<String>>(8).ok().flatten(),
                    ))
                })
                .context("failed to execute similar query")?;

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
                ) = row.context("failed to decode similar row")?;
                let embedding: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode candidate embedding")?;
                let score = cosine_similarity(&source_embedding, &embedding);
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
impl PhraseSearcher for SqliteStorage {
    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let phrase = phrase.to_string();
        let limit = i64::try_from(limit).context("phrase search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let escaped = phrase
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
            if let Some(importance_min) = opts.importance_min {
                sql.push_str(&format!(" AND importance >= ?{idx}"));
                params_values.push(SqlValue::Real(importance_min));
                idx += 1;
            }
            if let Some(created_after) = opts.created_after.clone() {
                sql.push_str(&format!(" AND created_at >= ?{idx}"));
                params_values.push(SqlValue::Text(created_after));
                idx += 1;
            }
            if let Some(created_before) = opts.created_before.clone() {
                sql.push_str(&format!(" AND created_at <= ?{idx}"));
                params_values.push(SqlValue::Text(created_before));
                idx += 1;
            }
            sql.push_str(" ORDER BY created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare phrase search query")?;
            let mut refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                refs.push(value);
            }

            let rows = stmt
                .query_map(refs.as_slice(), |row| {
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
                .context("failed to execute phrase search query")?;

            let mut out = Vec::new();
            for row in rows {
                let result = row.context("failed to decode phrase search row")?;
                if let Some(context_tags) = opts.context_tags.as_ref()
                    && !context_tags.is_empty()
                    && !context_tags
                        .iter()
                        .all(|tag| result.tags.iter().any(|r| r.eq_ignore_ascii_case(tag)))
                {
                    continue;
                }
                out.push(result);
            }

            Ok::<_, anyhow::Error>(out)
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
                    let canonical = canonical_hash(new_content);
                    let emb = serde_json::to_vec(&embedder.embed(new_content)?)
                        .context("failed to serialize embedding")?;
                    Some((new_content.to_string(), hash, canonical, emb))
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

            if let Some((new_content, hash, canonical, embedding)) = &content_fields {
                set_clauses.push(format!("content = ?{next_param_index}"));
                values.push(SqlValue::Text(new_content.clone()));
                next_param_index += 1;

                set_clauses.push(format!("content_hash = ?{next_param_index}"));
                values.push(SqlValue::Text(hash.clone()));
                next_param_index += 1;

                set_clauses.push(format!("embedding = ?{next_param_index}"));
                values.push(SqlValue::Blob(embedding.clone()));
                next_param_index += 1;

                set_clauses.push(format!("canonical_hash = ?{next_param_index}"));
                values.push(SqlValue::Text(canonical.clone()));
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

            if let Some((new_content, _, _, _)) = &content_fields {
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
impl FeedbackRecorder for SqliteStorage {
    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value> {
        let delta = match rating {
            "helpful" => 1_i64,
            "unhelpful" => -1_i64,
            "outdated" => -2_i64,
            _ => return Err(anyhow!("invalid rating: {rating}")),
        };

        let conn = Arc::clone(&self.conn);
        let memory_id = memory_id.to_string();
        let rating = rating.to_string();
        let reason = reason.map(ToString::to_string);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let metadata_raw: Option<String> = conn
                .query_row(
                    "SELECT metadata FROM memories WHERE id = ?1",
                    params![memory_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query metadata for feedback")?;

            let metadata_raw =
                metadata_raw.ok_or_else(|| anyhow!("memory not found for id={memory_id}"))?;
            let mut metadata = parse_metadata_from_db(&metadata_raw);

            let mut feedback_signals = metadata
                .get("feedback_signals")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();

            feedback_signals.push(serde_json::json!({
                "rating": rating,
                "reason": reason,
                "at": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            }));

            let new_score = metadata
                .get("feedback_score")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                + delta;
            let flagged = new_score <= -3;

            metadata["feedback_signals"] = serde_json::Value::Array(feedback_signals.clone());
            metadata["feedback_score"] = serde_json::Value::Number(new_score.into());
            if flagged {
                metadata["flagged_for_review"] = serde_json::Value::Bool(true);
            }

            let metadata_json = serde_json::to_string(&metadata)
                .context("failed to serialize feedback metadata")?;
            conn.execute(
                "UPDATE memories
                 SET metadata = ?2,
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![memory_id, metadata_json],
            )
            .context("failed to persist feedback metadata")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "memory_id": memory_id,
                "rating": rating,
                "new_score": new_score,
                "total_signals": feedback_signals.len(),
                "flagged": flagged,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl ExpirationSweeper for SqliteStorage {
    async fn sweep_expired(&self) -> Result<usize> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sweep transaction")?;

            let mut stmt = tx
                .prepare(
                    "SELECT id FROM memories
                     WHERE ttl_seconds IS NOT NULL
                       AND datetime(created_at, '+' || ttl_seconds || ' seconds') < datetime('now')",
                )
                .context("failed to prepare expiration query")?;
            let expired_rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .context("failed to execute expiration query")?;

            let mut expired_ids = Vec::new();
            for row in expired_rows {
                expired_ids.push(row.context("failed to decode expiration row")?);
            }
            drop(stmt);

            for id in &expired_ids {
                tx.execute(
                    "DELETE FROM relationships WHERE source_id = ?1 OR target_id = ?1",
                    params![id],
                )
                .context("failed to delete relationships during sweep")?;
                tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to delete FTS during sweep")?;
                tx.execute("DELETE FROM memories WHERE id = ?1", params![id])
                    .context("failed to delete memory during sweep")?;
            }

            tx.commit().context("failed to commit sweep transaction")?;
            Ok::<_, anyhow::Error>(expired_ids.len())
        })
        .await
        .context("spawn_blocking join error")?
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
        "ALTER TABLE memories ADD COLUMN ttl_seconds INTEGER",
        "ALTER TABLE memories ADD COLUMN canonical_hash TEXT",
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

fn canonicalize(content: &str) -> String {
    let stripped: String = content
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '*' | '#' | '`' | '~' | '[' | ']' | '(' | ')' | '>' | '|' | '_'
            )
        })
        .collect();
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

fn canonical_hash(content: &str) -> String {
    let canonical = canonicalize(content);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
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

fn resolve_priority(event_type: Option<&str>, priority: Option<i64>) -> u8 {
    if let Some(value) = priority
        && (1..=5).contains(&value)
    {
        return value as u8;
    }
    event_type
        .map(|et| {
            let p = crate::memory_core::default_priority_for_event_type(et);
            if p == 0 { 3 } else { p as u8 }
        })
        .unwrap_or(3)
}

fn normalize_for_dedup(content: &str) -> String {
    let collapsed = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    collapsed.chars().take(150).collect()
}

fn matches_search_options(candidate: &RankedSemanticCandidate, opts: &SearchOptions) -> bool {
    if let Some(event_type) = opts.event_type.as_deref()
        && candidate.result.event_type.as_deref() != Some(event_type)
    {
        return false;
    }
    if let Some(project) = opts.project.as_deref()
        && candidate.result.project.as_deref() != Some(project)
    {
        return false;
    }
    if let Some(session_id) = opts.session_id.as_deref()
        && candidate.result.session_id.as_deref() != Some(session_id)
    {
        return false;
    }
    if let Some(importance_min) = opts.importance_min
        && candidate.result.importance < importance_min
    {
        return false;
    }
    if let Some(created_after) = opts.created_after.as_deref()
        && candidate.created_at.as_str() < created_after
    {
        return false;
    }
    if let Some(created_before) = opts.created_before.as_deref()
        && candidate.created_at.as_str() > created_before
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::{
        AdvancedSearcher, ExpirationSweeper, FeedbackRecorder, GraphTraverser, PhraseSearcher,
        Recents, Retriever, SearchOptions, Searcher, SemanticSearcher, SimilarFinder, Storage,
        TTL_EPHEMERAL, TTL_LONG_TERM, TTL_SHORT_TERM, Updater, default_priority_for_event_type,
        default_ttl_for_event_type, is_valid_event_type,
    };

    #[derive(Debug, Clone)]
    struct KeywordEmbedder;

    impl Embedder for KeywordEmbedder {
        fn dimension(&self) -> usize {
            4
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            if text.contains("alpha") {
                Ok(vec![1.0, 0.0, 0.0, 0.0])
            } else if text.contains("beta") {
                Ok(vec![0.9, 0.1, 0.0, 0.0])
            } else {
                Ok(vec![0.0, 0.0, 1.0, 0.0])
            }
        }
    }

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
            "ttl_seconds",
            "canonical_hash",
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
        let types: Vec<&str> = rels
            .iter()
            .filter(|r| r.rel_type == "links_to" || r.rel_type == "depends_on")
            .map(|r| r.rel_type.as_str())
            .collect();
        assert_eq!(types.len(), 2);
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

    #[test]
    fn test_ttl_auto_assignment() {
        assert_eq!(
            default_ttl_for_event_type("session_summary"),
            Some(TTL_SHORT_TERM)
        );
        assert_eq!(
            default_ttl_for_event_type("task_completion"),
            Some(TTL_LONG_TERM)
        );
        assert_eq!(default_ttl_for_event_type("error_pattern"), None);
        assert_eq!(default_ttl_for_event_type("user_preference"), None);
        assert_eq!(default_ttl_for_event_type("checkpoint"), Some(604_800));
        assert_eq!(
            default_ttl_for_event_type("code_chunk"),
            Some(TTL_EPHEMERAL)
        );
        assert_eq!(
            default_ttl_for_event_type("file_summary"),
            Some(TTL_SHORT_TERM)
        );
        assert_eq!(
            default_ttl_for_event_type("unknown_event"),
            Some(TTL_LONG_TERM)
        );
    }

    #[tokio::test]
    async fn test_canonical_hash_dedup() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let input = MemoryInput {
            event_type: Some("memory".to_string()),
            ..Default::default()
        };

        <SqliteStorage as Storage>::store(&storage, "dup-a", "Hello World", &input)
            .await
            .unwrap();
        <SqliteStorage as Storage>::store(&storage, "dup-b", "Hello World", &input)
            .await
            .unwrap();

        let listed = storage
            .list(0, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(listed.total, 1);
        assert_eq!(storage.debug_get_access_count("dup-a").unwrap(), 1);
    }

    #[tokio::test]
    async fn test_canonical_hash_ignores_formatting() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let input = MemoryInput {
            event_type: Some("memory".to_string()),
            ..Default::default()
        };

        <SqliteStorage as Storage>::store(&storage, "fmt-a", "Hello World", &input)
            .await
            .unwrap();
        <SqliteStorage as Storage>::store(&storage, "fmt-b", "# hello    world", &input)
            .await
            .unwrap();

        let listed = storage
            .list(0, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(listed.total, 1);
    }

    #[tokio::test]
    async fn test_jaccard_dedup() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "jac-a",
            "alpha beta gamma delta epsilon zeta eta theta iota",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "jac-b",
            "alpha beta gamma delta epsilon zeta eta theta iota kappa",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let listed = storage
            .list(0, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(listed.total, 1);
    }

    #[tokio::test]
    async fn test_no_dedup_different_event_types() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "nd-a",
            "database migration plan finalized today",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "nd-b",
            "database migration plan finalized today with notes",
            &MemoryInput {
                event_type: Some("reminder".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let listed = storage
            .list(0, 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(listed.total, 2);
    }

    #[tokio::test]
    async fn test_auto_relate_creates_edges() {
        let storage =
            SqliteStorage::new_in_memory_with_embedder(Arc::new(KeywordEmbedder)).unwrap();

        <SqliteStorage as Storage>::store(&storage, "rel-a", "alpha seed", &MemoryInput::default())
            .await
            .unwrap();
        <SqliteStorage as Storage>::store(&storage, "rel-b", "beta seed", &MemoryInput::default())
            .await
            .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "rel-c",
            "alpha latest",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.conn.lock().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relationships WHERE source_id = 'rel-c' AND rel_type = 'related'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count >= 1);
    }

    #[tokio::test]
    async fn test_auto_relate_failure_doesnt_fail_store() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO memories (id, content, embedding, content_hash, source_type, tags, metadata)
                 VALUES ('broken', 'broken embedding', x'00', 'h-broken', 'test', '[]', '{}')",
                [],
            )
            .unwrap();
        }

        let result = <SqliteStorage as Storage>::store(
            &storage,
            "rel-safe",
            "safe store payload",
            &MemoryInput::default(),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(
            storage.retrieve("rel-safe").await.unwrap(),
            "safe store payload"
        );
    }

    #[tokio::test]
    async fn test_feedback_helpful() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fb-1",
            "feedback target",
            &MemoryInput::default(),
        )
        .await
        .unwrap();

        let result = <SqliteStorage as FeedbackRecorder>::record_feedback(
            &storage,
            "fb-1",
            "helpful",
            Some("useful"),
        )
        .await
        .unwrap();

        assert_eq!(result["new_score"].as_i64(), Some(1));
        assert_eq!(result["total_signals"].as_u64(), Some(1));
        assert_eq!(result["flagged"].as_bool(), Some(false));
    }

    #[tokio::test]
    async fn test_feedback_unhelpful_flagged() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "fb-2",
            "feedback target",
            &MemoryInput::default(),
        )
        .await
        .unwrap();

        for _ in 0..3 {
            let _ = <SqliteStorage as FeedbackRecorder>::record_feedback(
                &storage,
                "fb-2",
                "unhelpful",
                None,
            )
            .await
            .unwrap();
        }

        let conn = storage.conn.lock().unwrap();
        let metadata_raw: String = conn
            .query_row(
                "SELECT metadata FROM memories WHERE id = 'fb-2'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let metadata = parse_metadata_from_db(&metadata_raw);
        assert_eq!(metadata["feedback_score"].as_i64(), Some(-3));
        assert_eq!(metadata["flagged_for_review"].as_bool(), Some(true));
    }

    #[tokio::test]
    async fn test_sweep_expired() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ttl-exp",
            "expires",
            &MemoryInput {
                ttl_seconds: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id = 'ttl-exp'",
                [],
            )
            .unwrap();
        }

        let swept = <SqliteStorage as ExpirationSweeper>::sweep_expired(&storage)
            .await
            .unwrap();
        assert_eq!(swept, 1);
        assert!(storage.retrieve("ttl-exp").await.is_err());
    }

    #[tokio::test]
    async fn test_sweep_preserves_permanent() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ttl-perm",
            "permanent",
            &MemoryInput {
                ttl_seconds: None,
                ..Default::default()
            },
        )
        .await
        .unwrap();

        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id = 'ttl-perm'",
                [],
            )
            .unwrap();
        }

        let swept = <SqliteStorage as ExpirationSweeper>::sweep_expired(&storage)
            .await
            .unwrap();
        assert_eq!(swept, 0);
        assert_eq!(storage.retrieve("ttl-perm").await.unwrap(), "permanent");
    }

    #[tokio::test]
    async fn test_ttl_seconds_stored() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ttl-set",
            "ttl value",
            &MemoryInput {
                ttl_seconds: Some(1234),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.conn.lock().unwrap();
        let ttl: Option<i64> = conn
            .query_row(
                "SELECT ttl_seconds FROM memories WHERE id = 'ttl-set'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ttl, Some(1234));
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
        let links_to: Vec<_> = rels
            .into_iter()
            .filter(|r| r.rel_type == "links_to")
            .collect();
        assert_eq!(links_to.len(), 1);
        assert_eq!(links_to[0].weight, 0.7);
        assert_eq!(links_to[0].metadata, serde_json::json!({"k":"v"}));
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
                    importance_min: None,
                    created_after: None,
                    created_before: None,
                    context_tags: None,
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
    async fn test_advanced_search_basic() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content, event_type) in [
            ("adv1", "alpha memory context", "decision"),
            ("adv2", "alpha context details", "reminder"),
            ("adv3", "unrelated content", "task_completion"),
        ] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
                &MemoryInput {
                    event_type: Some(event_type.to_string()),
                    tags: vec!["alpha".to_string()],
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &storage,
            "alpha context",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
        assert!(!results.is_empty());
        assert!(results.iter().all(|r| (0.0..=1.0).contains(&r.score)));
    }

    #[tokio::test]
    async fn test_advanced_search_type_weight() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "weight1",
            "same searchable text decision",
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
            "weight2",
            "same searchable text reminder",
            &MemoryInput {
                event_type: Some("reminder".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &storage,
            "same searchable text",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].id, "weight2");
    }

    #[tokio::test]
    async fn test_advanced_search_dedup() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for id in ["dup1", "dup2"] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                "identical duplicate content",
                &MemoryInput {
                    event_type: Some("decision".to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &storage,
            "identical duplicate",
            10,
            &SearchOptions::default(),
        )
        .await
        .unwrap();
        let duplicate_count = results
            .iter()
            .filter(|r| r.content == "identical duplicate content")
            .count();
        assert_eq!(duplicate_count, 1);
    }

    #[tokio::test]
    async fn test_advanced_search_filters() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "flt1",
            "filter target text",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                project: Some("project-a".to_string()),
                importance: 0.8,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "flt2",
            "filter target text",
            &MemoryInput {
                event_type: Some("reminder".to_string()),
                project: Some("project-b".to_string()),
                importance: 0.2,
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = <SqliteStorage as AdvancedSearcher>::advanced_search(
            &storage,
            "filter target",
            10,
            &SearchOptions {
                event_type: Some("decision".to_string()),
                project: Some("project-a".to_string()),
                importance_min: Some(0.5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "flt1");
    }

    #[tokio::test]
    async fn test_graph_traverse_basic() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content) in [("ga", "A"), ("gb", "B"), ("gc", "C")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
                &MemoryInput {
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        storage
            .add_relationship("ga", "gb", "links", 0.9, &serde_json::json!({}))
            .await
            .unwrap();
        storage
            .add_relationship("gb", "gc", "links", 0.8, &serde_json::json!({}))
            .await
            .unwrap();

        let edge_types = vec!["links".to_string()];
        let nodes =
            <SqliteStorage as GraphTraverser>::traverse(&storage, "ga", 2, 0.0, Some(&edge_types))
                .await
                .unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].id, "gb");
        assert_eq!(nodes[0].hop, 1);
        assert_eq!(nodes[1].id, "gc");
        assert_eq!(nodes[1].hop, 2);
    }

    #[tokio::test]
    async fn test_graph_traverse_min_weight() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for id in ["gwa", "gwb", "gwc"] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                id,
                &MemoryInput {
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        storage
            .add_relationship("gwa", "gwb", "links", 0.9, &serde_json::json!({}))
            .await
            .unwrap();
        storage
            .add_relationship("gwa", "gwc", "links", 0.1, &serde_json::json!({}))
            .await
            .unwrap();

        let edge_types = vec!["links".to_string()];
        let nodes =
            <SqliteStorage as GraphTraverser>::traverse(&storage, "gwa", 2, 0.5, Some(&edge_types))
                .await
                .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "gwb");
    }

    #[tokio::test]
    async fn test_graph_traverse_max_hops() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content) in [("mh1", "n1"), ("mh2", "n2"), ("mh3", "n3"), ("mh4", "n4")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
                &MemoryInput {
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        storage
            .add_relationship("mh1", "mh2", "links", 1.0, &serde_json::json!({}))
            .await
            .unwrap();
        storage
            .add_relationship("mh2", "mh3", "links", 1.0, &serde_json::json!({}))
            .await
            .unwrap();
        storage
            .add_relationship("mh3", "mh4", "links", 1.0, &serde_json::json!({}))
            .await
            .unwrap();

        let edge_types = vec!["links".to_string()];
        let nodes =
            <SqliteStorage as GraphTraverser>::traverse(&storage, "mh1", 2, 0.0, Some(&edge_types))
                .await
                .unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|n| n.hop <= 2));
    }

    #[tokio::test]
    async fn test_find_similar() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "sim1",
            "alpha beta",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "sim2",
            "alpha beta extra",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "sim3",
            "zzzz qqqq",
            &MemoryInput {
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = <SqliteStorage as SimilarFinder>::find_similar(&storage, "sim1", 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "sim2");
        assert!(results[0].score >= results[1].score);
    }

    #[tokio::test]
    async fn test_phrase_search() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ph1",
            "this has exact phrase inside",
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
            "ph2",
            "this has different words",
            &MemoryInput {
                event_type: Some("decision".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let results = <SqliteStorage as PhraseSearcher>::phrase_search(
            &storage,
            "exact phrase",
            10,
            &SearchOptions {
                event_type: Some("decision".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "ph1");
    }

    #[tokio::test]
    async fn test_search_empty_opts_returns_all() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content) in [("all_1", "same one"), ("all_2", "same two")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
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
            "ttl_seconds",
            "canonical_hash",
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
