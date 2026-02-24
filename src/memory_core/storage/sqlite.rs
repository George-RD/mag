use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::memory_core::{
    ABSTENTION_MIN_TEXT, AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter,
    ExpirationSweeper, FeedbackRecorder, GRAPH_MIN_EDGE_WEIGHT, GRAPH_NEIGHBOR_FACTOR, GraphNode,
    GraphTraverser, LessonQuerier, ListResult, Lister, MaintenanceManager, MemoryInput,
    MemoryUpdate, PhraseSearcher, ProfileManager, Recents, Relationship, RelationshipQuerier,
    ReminderManager, Retriever, SearchOptions, SearchResult, Searcher, SemanticResult,
    SemanticSearcher, SimilarFinder, StatsProvider, Storage, Tagger, Updater, WelcomeProvider,
    default_priority_for_event_type, default_ttl_for_event_type, embedder::Embedder,
    feedback_factor, jaccard_similarity, priority_factor, time_decay, type_weight, word_overlap,
};

const DEDUP_THRESHOLDS: &[(&str, f64)] = &[
    ("error_pattern", 0.70),
    ("session_summary", 0.75),
    ("task_completion", 0.85),
    ("decision", 0.80),
    ("lesson_learned", 0.85),
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

            let mut profile_stmt = conn
                .prepare("SELECT key, value FROM user_profile")
                .context("failed to prepare user profile export query")?;
            let mut user_profile = serde_json::Map::new();
            let profile_rows = profile_stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("failed to query user profile for export")?;
            for row in profile_rows {
                let (key, value_raw) = row.context("failed to decode user profile row")?;
                let value = serde_json::from_str(&value_raw)
                    .unwrap_or(serde_json::Value::String(value_raw));
                user_profile.insert(key, value);
            }

            let export = serde_json::json!({
                "version": 1,
                "memories": memories,
                "relationships": relationships,
                "user_profile": user_profile,
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

        let user_profile = parsed["user_profile"]
            .as_object()
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

            for (key, value) in &user_profile {
                let value_json =
                    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string());
                tx.execute(
                    "INSERT OR REPLACE INTO user_profile (key, value, updated_at)
                     VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                    params![key, value_json],
                )
                .context("failed to import user profile value")?;
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
            if let Some(entity_id) = opts.entity_id.clone() {
                fts_sql.push_str(&format!(" AND m.entity_id = ?{param_idx}"));
                fts_params.push(SqlValue::Text(entity_id));
                param_idx += 1;
            }
            if let Some(agent_type) = opts.agent_type.clone() {
                fts_sql.push_str(&format!(" AND m.agent_type = ?{param_idx}"));
                fts_params.push(SqlValue::Text(agent_type));
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
            if let Some(entity_id) = opts.entity_id.clone() {
                sql.push_str(&format!(" AND entity_id = ?{idx}"));
                params_values.push(SqlValue::Text(entity_id));
                idx += 1;
            }
            if let Some(agent_type) = opts.agent_type.clone() {
                sql.push_str(&format!(" AND agent_type = ?{idx}"));
                params_values.push(SqlValue::Text(agent_type));
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
    #[allow(dead_code)] // Stored for diagnostics; abstention uses collection-level text_overlap
    vec_sim: Option<f64>,
    text_overlap: f64,
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

            // ── RRF (Reciprocal Rank Fusion) hybrid search ─────────
            // Rank each signal independently then fuse with 1/(k+rank).
            const RRF_K: f64 = 60.0;

            // Phase 1: Collect vector candidates sorted by cosine similarity
            let mut vector_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

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
                let candidate_emb: Vec<f32> = serde_json::from_slice(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                let similarity = cosine_similarity(&query_embedding, &candidate_emb) as f64;
                if similarity < 0.1 {
                    continue;
                }

                let priority_value = resolve_priority(event_type.as_deref(), priority);
                vector_candidates.push((
                    id.clone(),
                    similarity,
                    RankedSemanticCandidate {
                        result: SemanticResult {
                            id,
                            content,
                            tags: parse_tags_from_db(&raw_tags),
                            importance,
                            metadata: parse_metadata_from_db(&raw_metadata),
                            event_type: event_type.clone(),
                            session_id,
                            project,
                            score: 0.0,
                        },
                        created_at,
                        score: type_weight(event_type.as_deref().unwrap_or("memory"))
                            * priority_factor(priority_value),
                        vec_sim: Some(similarity),
                        text_overlap: 0.0,
                    },
                ));
            }
            // Sort by cosine similarity descending for rank assignment
            vector_candidates.sort_by(|a, b| b.1.total_cmp(&a.1));

            // Phase 2: Collect FTS candidates sorted by BM25
            let mut fts_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

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

                        let priority_value = resolve_priority(event_type.as_deref(), priority);
                        fts_candidates.push((
                            id.clone(),
                            bm25, // raw BM25: more negative = better match
                            RankedSemanticCandidate {
                                result: SemanticResult {
                                    id,
                                    content,
                                    tags: parse_tags_from_db(&raw_tags),
                                    importance,
                                    metadata: parse_metadata_from_db(&raw_metadata),
                                    event_type: event_type.clone(),
                                    session_id,
                                    project,
                                    score: 0.0,
                                },
                                created_at,
                                score: type_weight(event_type.as_deref().unwrap_or("memory"))
                                    * priority_factor(priority_value),
                                vec_sim: None,
                                text_overlap: 0.0,
                            },
                        ));
                    }
                }
            }
            // BM25 returns negative values where more negative = better match,
            // so sort ascending (most negative first = best rank for RRF)
            fts_candidates.sort_by(|a, b| a.1.total_cmp(&b.1));

            // Phase 3: RRF fusion — assign reciprocal rank scores and merge
            let mut ranked: HashMap<String, RankedSemanticCandidate> = HashMap::new();

            for (rank, (id, _sim, candidate)) in vector_candidates.into_iter().enumerate() {
                let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
                let mut merged = candidate;
                merged.score *= rrf_score;
                ranked.insert(id, merged);
            }

            for (rank, (id, _bm25, candidate)) in fts_candidates.into_iter().enumerate() {
                let rrf_score = 1.0 / (RRF_K + rank as f64 + 1.0);
                if let Some(existing) = ranked.get_mut(&id) {
                    // Present in both — add the FTS RRF contribution
                    existing.score += candidate.score * rrf_score;
                } else {
                    let mut merged = candidate;
                    merged.score *= rrf_score;
                    ranked.insert(id, merged);
                }
            }

            for candidate in ranked.values_mut() {
                let with_tags = if candidate.result.tags.is_empty() {
                    candidate.result.content.clone()
                } else {
                    format!("{} {}", candidate.result.content, candidate.result.tags.join(" "))
                };
                let overlap = word_overlap(&query_word_refs, &with_tags);
                candidate.text_overlap = overlap;
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

                // Phase 4b: Feedback score weighting
                let fb_score = candidate
                    .result
                    .metadata
                    .get("feedback_score")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                candidate.score *= feedback_factor(fb_score);
            }


            // ── Phase 5: Graph enrichment — inject 1-hop neighbors from top seeds ──
            {
                let mut seed_list: Vec<(String, f64)> = ranked
                    .iter()
                    .map(|(id, c)| (id.clone(), c.score))
                    .collect();
                seed_list.sort_by(|a, b| b.1.total_cmp(&a.1));
                let k = limit.clamp(5, 8);
                seed_list.truncate(k);

                let neighbor_sql = "\
                    SELECT m.id, m.content, m.tags, m.importance, m.metadata, \
                           m.event_type, m.session_id, m.project, m.priority, m.created_at, \
                           m.embedding, r.weight \
                    FROM relationships r \
                    JOIN memories m ON m.id = CASE \
                        WHEN r.source_id = ?1 THEN r.target_id \
                        ELSE r.source_id END \
                    WHERE (r.source_id = ?1 OR r.target_id = ?1) \
                      AND r.weight >= ?2 \
                      AND m.id != ?1";

                let mut neighbors_to_add: Vec<(String, RankedSemanticCandidate)> = Vec::new();

                if let Ok(mut stmt) = conn.prepare(neighbor_sql) {
                    for (seed_id, seed_score) in &seed_list {
                        if let Ok(rows) = stmt.query_map(
                            params![seed_id, GRAPH_MIN_EDGE_WEIGHT],
                            |row| {
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
                                    row.get::<_, Option<Vec<u8>>>(10).ok().flatten(),
                                    row.get::<_, f64>(11).unwrap_or(0.5),
                                ))
                            },
                        ) {
                            for row_res in rows {
                                let row = match row_res {
                                    Ok(r) => r,
                                    Err(e) => {
                                        eprintln!("warning: failed to decode graph neighbor row: {e}");
                                        continue;
                                    }
                                };
                                let (
                                    id, content, raw_tags, importance, raw_metadata,
                                    event_type, session_id, project, _priority, created_at,
                                    embedding_blob, edge_weight,
                                ) = row;

                                let mut neighbor_score =
                                    GRAPH_NEIGHBOR_FACTOR * seed_score * edge_weight;

                                let tags = parse_tags_from_db(&raw_tags);
                                let metadata = parse_metadata_from_db(&raw_metadata);
                                let with_tags = if tags.is_empty() {
                                    content.clone()
                                } else {
                                    format!("{} {}", content, tags.join(" "))
                                };
                                let overlap = word_overlap(&query_word_refs, &with_tags);
                                neighbor_score *= 1.0 + overlap * 0.5;
                                neighbor_score *= time_decay(&created_at);
                                neighbor_score *= 0.5 + importance * 0.5;

                                let vec_sim = embedding_blob.and_then(|blob| {
                                    serde_json::from_slice::<Vec<f32>>(&blob)
                                        .ok()
                                        .map(|emb| cosine_similarity(&query_embedding, &emb) as f64)
                                });

                                let fb_score = metadata
                                    .get("feedback_score")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                neighbor_score *= feedback_factor(fb_score);

                                neighbors_to_add.push((
                                    id.clone(),
                                    RankedSemanticCandidate {
                                        result: SemanticResult {
                                            id,
                                            content,
                                            tags,
                                            importance,
                                            metadata,
                                            event_type,
                                            session_id,
                                            project,
                                            score: 0.0,
                                        },
                                        created_at,
                                        score: neighbor_score,
                                        vec_sim,
                                        text_overlap: overlap,
                                    },
                                ));
                            }
                        }
                    }
                }

                for (id, neighbor) in neighbors_to_add {
                    if let Some(existing) = ranked.get_mut(&id) {
                        if neighbor.score > existing.score {
                            existing.score = neighbor.score;
                        }
                    } else {
                        ranked.insert(id, neighbor);
                    }
                }
            }
            // ── Phase 6: Collection-level abstention + dedup ─────────────
            // Dense embeddings (bge-small-en-v1.5) produce high cosine similarity
            // (0.80+) even for completely unrelated content, making vec_sim
            // useless for abstention. Text overlap is the discriminative signal:
            //   • Legitimate queries: max text_overlap typically ≥ 0.33
            //   • Irrelevant queries: max text_overlap typically 0.00–0.25
            // Apply a collection-level gate on the best text overlap.
            // Skip the abstention gate when the query has no eligible word
            // tokens (all tokens ≤ 2 chars, e.g. "AI", "C++") — text overlap
            // would always be 0.0, causing false abstention.
            // NOTE: Gate is applied AFTER search-option filtering (below) so
            // that out-of-scope high-overlap candidates don't suppress
            // abstention for scoped queries.
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

            // Apply abstention gate on the filtered (in-scope) candidates.
            if !query_word_refs.is_empty() {
                let max_text_overlap = deduped
                    .iter()
                    .map(|c| c.text_overlap)
                    .fold(0.0f64, f64::max);
                if max_text_overlap < ABSTENTION_MIN_TEXT {
                    return Ok::<_, anyhow::Error>(Vec::new());
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

#[async_trait]
impl ProfileManager for SqliteStorage {
    async fn get_profile(&self) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut profile = serde_json::Map::new();
            let mut stmt = conn
                .prepare("SELECT key, value FROM user_profile")
                .context("failed to prepare user profile query")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("failed to query user profile rows")?;

            for row in rows {
                let (key, value_raw) = row.context("failed to decode user profile row")?;
                let value = serde_json::from_str::<serde_json::Value>(&value_raw)
                    .unwrap_or(serde_json::Value::String(value_raw));
                profile.insert(key, value);
            }

            let mut pref_stmt = conn
                .prepare(
                    "SELECT id, content, metadata, created_at
                     FROM memories
                     WHERE event_type = 'user_preference'
                     ORDER BY created_at DESC
                     LIMIT 20",
                )
                .context("failed to prepare preference query")?;
            let pref_rows = pref_stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?,
                        "metadata": parse_metadata_from_db(&row.get::<_, String>(2)?),
                        "created_at": row.get::<_, String>(3)?,
                    }))
                })
                .context("failed to query preferences from memory")?;

            let mut preferences_from_memory = Vec::new();
            for row in pref_rows {
                preferences_from_memory
                    .push(row.context("failed to decode preference from memory row")?);
            }
            profile.insert(
                "preferences_from_memory".to_string(),
                serde_json::Value::Array(preferences_from_memory),
            );

            Ok::<_, anyhow::Error>(serde_json::Value::Object(profile))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn set_profile(&self, updates: &serde_json::Value) -> Result<()> {
        let updates = updates.clone();
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let updates_obj = updates
                .as_object()
                .ok_or_else(|| anyhow!("profile updates must be a JSON object"))?;
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            for (key, value) in updates_obj {
                let value_json = serde_json::to_string(value)
                    .context("failed to serialize user profile value")?;
                conn.execute(
                    "INSERT OR REPLACE INTO user_profile (key, value, updated_at)
                     VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
                    params![key, value_json],
                )
                .context("failed to upsert user profile value")?;
            }

            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(())
    }
}

#[async_trait]
impl CheckpointManager for SqliteStorage {
    async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let task_title = input.task_title.clone();
        let task_marker = format!("## Checkpoint: {}", task_title);
        let existing_count = tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;
            let mut stmt = conn
                .prepare(
                    "SELECT COUNT(*) FROM memories
                     WHERE event_type = 'checkpoint' AND lower(content) LIKE lower(?1)",
                )
                .context("failed to prepare checkpoint count query")?;
            let pattern = format!("%{}%", task_marker.replace('%', "\\%").replace('_', "\\_"));
            let count: i64 = stmt
                .query_row(params![pattern], |row| row.get(0))
                .context("failed to count matching checkpoints")?;
            Ok::<_, anyhow::Error>(count)
        })
        .await
        .context("spawn_blocking join error")??;

        let checkpoint_number = existing_count + 1;
        let mut content = format!(
            "## Checkpoint: {}\n### Progress\n{}",
            input.task_title, input.progress
        );
        if let Some(plan) = input.plan.as_deref() {
            content.push_str("\n\n### Plan\n");
            content.push_str(plan);
        }
        if let Some(files_touched) = input.files_touched.as_ref() {
            content.push_str("\n\n### Files Touched\n");
            content.push_str(
                &serde_json::to_string_pretty(files_touched)
                    .context("failed to serialize files_touched for checkpoint")?,
            );
        }
        if let Some(decisions) = input.decisions.as_ref()
            && !decisions.is_empty()
        {
            content.push_str("\n\n### Decisions\n");
            for decision in decisions {
                content.push_str("- ");
                content.push_str(decision);
                content.push('\n');
            }
        }
        if let Some(key_context) = input.key_context.as_deref() {
            content.push_str("\n### Key Context\n");
            content.push_str(key_context);
        }
        if let Some(next_steps) = input.next_steps.as_deref() {
            content.push_str("\n\n### Next Steps\n");
            content.push_str(next_steps);
        }

        let metadata = serde_json::json!({
            "checkpoint_number": checkpoint_number,
            "checkpoint_data": {
                "task_title": input.task_title,
                "plan": input.plan,
                "progress": input.progress,
                "files_touched": input.files_touched,
                "decisions": input.decisions,
                "key_context": input.key_context,
                "next_steps": input.next_steps,
            }
        });

        let id = Uuid::new_v4().to_string();
        let memory_input = MemoryInput {
            content: content.clone(),
            id: Some(id.clone()),
            metadata,
            event_type: Some("checkpoint".to_string()),
            priority: Some(default_priority_for_event_type("checkpoint")),
            ttl_seconds: default_ttl_for_event_type("checkpoint"),
            session_id: input.session_id,
            project: input.project,
            ..Default::default()
        };

        <Self as Storage>::store(self, &id, &content, &memory_input).await?;
        Ok(id)
    }

    async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let query = query.to_string();
        let project = project.map(ToString::to_string);
        let limit = i64::try_from(limit).context("resume_task limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut sql = String::from(
                "SELECT content, metadata, created_at
                 FROM memories
                 WHERE event_type = 'checkpoint'",
            );
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();
            let mut idx = 1;

            if !query.trim().is_empty() {
                sql.push_str(&format!(" AND lower(content) LIKE ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(format!(
                    "%{}%",
                    query.to_lowercase()
                )));
                idx += 1;
            }

            if let Some(project_value) = project {
                sql.push_str(&format!(" AND project = ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(project_value));
                idx += 1;
            }

            sql.push_str(" ORDER BY created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(rusqlite::types::Value::Integer(limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare resume_task query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(serde_json::json!({
                        "content": row.get::<_, String>(0)?,
                        "metadata": parse_metadata_from_db(&row.get::<_, String>(1)?),
                        "created_at": row.get::<_, String>(2)?,
                    }))
                })
                .context("failed to execute resume_task query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode resume_task row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl ReminderManager for SqliteStorage {
    async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        let duration = crate::memory_core::parse_duration(duration_str)?;
        let now = Utc::now();
        let remind_at = now + duration;
        let remind_at_iso = remind_at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let now_iso = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let mut metadata = serde_json::json!({
            "event_type": "reminder",
            "reminder_status": "pending",
            "remind_at": remind_at_iso,
            "created_at_utc": now_iso,
        });
        if let Some(context_value) = context {
            metadata["context"] = serde_json::Value::String(context_value.to_string());
        }
        if let Some(session_value) = session_id {
            metadata["session_id"] = serde_json::Value::String(session_value.to_string());
        }
        if let Some(project_value) = project {
            metadata["project"] = serde_json::Value::String(project_value.to_string());
        }

        let reminder_id = Uuid::new_v4().to_string();
        let content = format!("{text}\n[due: {remind_at_iso}]");
        let input = MemoryInput {
            content: content.clone(),
            id: Some(reminder_id.clone()),
            metadata,
            event_type: Some("reminder".to_string()),
            priority: Some(default_priority_for_event_type("reminder")),
            ttl_seconds: default_ttl_for_event_type("reminder"),
            session_id: session_id.map(ToString::to_string),
            project: project.map(ToString::to_string),
            ..Default::default()
        };

        <Self as Storage>::store(self, &reminder_id, &content, &input).await?;

        Ok(serde_json::json!({
            "reminder_id": reminder_id,
            "text": text,
            "remind_at": remind_at_iso,
            "duration": duration_str,
        }))
    }

    async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let conn = Arc::clone(&self.conn);
        let status = status.map(ToString::to_string);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare(
                    "SELECT id, content, metadata, created_at
                     FROM memories
                     WHERE event_type = 'reminder'",
                )
                .context("failed to prepare reminder list query")?;
            let rows = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })
                .context("failed to execute reminder list query")?;

            let now = Utc::now();
            let status_filter = status.unwrap_or_else(|| "pending".to_string());
            let include_all = status_filter == "all";

            let mut reminders: Vec<(bool, DateTime<Utc>, serde_json::Value)> = Vec::new();

            for row in rows {
                let (id, content, metadata_raw, created_at) =
                    row.context("failed to decode reminder row")?;
                let metadata = parse_metadata_from_db(&metadata_raw);
                let reminder_status = metadata
                    .get("reminder_status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("pending");

                if !include_all && reminder_status != status_filter {
                    continue;
                }

                let remind_at_str = metadata
                    .get("remind_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("1970-01-01T00:00:00.000Z");
                let remind_at = DateTime::parse_from_rfc3339(remind_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| {
                        DateTime::parse_from_rfc3339("9999-12-31T23:59:59.000Z")
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or(now)
                    });
                let is_due = now >= remind_at;
                let is_overdue = is_due && reminder_status == "pending";

                reminders.push((
                    is_overdue,
                    remind_at,
                    serde_json::json!({
                        "reminder_id": id,
                        "text": content,
                        "status": reminder_status,
                        "remind_at": remind_at_str,
                        "is_due": is_due,
                        "is_overdue": is_overdue,
                        "metadata": metadata,
                        "created_at": created_at,
                    }),
                ));
            }

            reminders.sort_by(|a, b| {
                b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)).then_with(|| {
                    a.2["created_at"]
                        .as_str()
                        .unwrap_or_default()
                        .cmp(b.2["created_at"].as_str().unwrap_or_default())
                })
            });

            Ok::<_, anyhow::Error>(reminders.into_iter().map(|(_, _, value)| value).collect())
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);
        let reminder_id = reminder_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let row: Option<(Option<String>, String)> = conn
                .query_row(
                    "SELECT event_type, metadata FROM memories WHERE id = ?1",
                    params![reminder_id],
                    |row| Ok((row.get(0).ok(), row.get(1)?)),
                )
                .optional()
                .context("failed to query reminder for dismiss")?;

            let (event_type, metadata_raw) =
                row.ok_or_else(|| anyhow!("reminder not found for id={reminder_id}"))?;
            if event_type.as_deref() != Some("reminder") {
                return Err(anyhow!("memory is not a reminder for id={reminder_id}"));
            }

            let mut metadata = parse_metadata_from_db(&metadata_raw);
            let dismissed_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
            metadata["reminder_status"] = serde_json::Value::String("dismissed".to_string());
            metadata["dismissed_at"] = serde_json::Value::String(dismissed_at.clone());
            let metadata_json = serde_json::to_string(&metadata)
                .context("failed to serialize dismissed reminder metadata")?;

            conn.execute(
                "UPDATE memories
                 SET metadata = ?2,
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![reminder_id, metadata_json],
            )
            .context("failed to update reminder status")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "reminder_id": reminder_id,
                "status": "dismissed",
                "dismissed_at": dismissed_at,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl LessonQuerier for SqliteStorage {
    async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let task = task.map(ToString::to_string);
        let project = project.map(ToString::to_string);
        let exclude_session = exclude_session.map(ToString::to_string);
        let agent_type = agent_type.map(ToString::to_string);
        let limit = i64::try_from(limit).context("lessons query limit exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut sql = String::from(
                "SELECT id, content, session_id, access_count, created_at, metadata, project, agent_type
                 FROM memories
                 WHERE event_type = 'lesson_learned'",
            );
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();
            let mut idx = 1;

            if let Some(task_value) = task {
                sql.push_str(&format!(" AND lower(content) LIKE ?{idx}"));
                params_values.push(rusqlite::types::Value::Text(format!(
                    "%{}%",
                    task_value.to_lowercase()
                )));
                idx += 1;
            }

            sql.push_str(" ORDER BY access_count DESC, created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(rusqlite::types::Value::Integer(limit * 4));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare lesson query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for value in &params_values {
                param_refs.push(value);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2).ok().flatten(),
                        row.get::<_, i64>(3).unwrap_or(0),
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                    ))
                })
                .context("failed to execute lesson query")?;

            let mut dedup_keys = HashSet::new();
            let mut results = Vec::new();
            for row in rows {
                let (
                    id,
                    content,
                    session_id,
                    access_count,
                    created_at,
                    metadata_raw,
                    project_col,
                    agent_type_col,
                ) = row.context("failed to decode lesson row")?;
                let metadata = parse_metadata_from_db(&metadata_raw);

                if let Some(project_filter) = project.as_deref() {
                    let metadata_project = metadata
                        .get("project")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string)
                        .or(project_col);
                    if metadata_project.as_deref() != Some(project_filter) {
                        continue;
                    }
                }

                if let Some(exclude) = exclude_session.as_deref()
                    && session_id.as_deref() == Some(exclude)
                {
                    continue;
                }

                if let Some(agent_filter) = agent_type.as_deref() {
                    let metadata_agent = metadata
                        .get("agent_type")
                        .and_then(serde_json::Value::as_str)
                        .map(ToString::to_string)
                        .or(agent_type_col);
                    if metadata_agent.as_deref() != Some(agent_filter) {
                        continue;
                    }
                }

                let dedup_key = content.chars().take(80).collect::<String>().to_lowercase();
                if !dedup_keys.insert(dedup_key) {
                    continue;
                }

                results.push(serde_json::json!({
                    "content": content,
                    "lesson_id": id,
                    "session_id": session_id,
                    "access_count": access_count,
                    "created_at": created_at,
                }));
            }

            results.sort_by(|a, b| {
                b["access_count"]
                    .as_i64()
                    .unwrap_or(0)
                    .cmp(&a["access_count"].as_i64().unwrap_or(0))
            });
            results.truncate(limit as usize);

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl MaintenanceManager for SqliteStorage {
    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let node_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories")?;

            let integrity: String = conn
                .query_row("PRAGMA integrity_check", [], |row| row.get(0))
                .unwrap_or_else(|_| "error".to_string());
            let integrity_ok = integrity == "ok";

            // Attempt to get db file size from the database path
            let db_size_bytes: i64 = conn
                .query_row(
                    "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            let db_size_mb = db_size_bytes as f64 / (1024.0 * 1024.0);

            let mut warnings: Vec<String> = Vec::new();
            let status;

            if !integrity_ok {
                status = "critical";
                warnings.push("Database integrity check failed".to_string());
            } else if db_size_mb >= critical_mb {
                status = "critical";
                warnings.push(format!(
                    "Database size {db_size_mb:.1}MB exceeds critical threshold {critical_mb}MB"
                ));
            } else if db_size_mb >= warn_mb {
                status = "warning";
                warnings.push(format!(
                    "Database size {db_size_mb:.1}MB exceeds warning threshold {warn_mb}MB"
                ));
            } else if node_count >= max_nodes {
                status = "warning";
                warnings.push(format!(
                    "Node count {node_count} exceeds max_nodes {max_nodes}"
                ));
            } else {
                status = "healthy";
            }

            Ok::<_, anyhow::Error>(serde_json::json!({
                "status": status,
                "db_size_mb": (db_size_mb * 100.0).round() / 100.0,
                "node_count": node_count,
                "max_nodes": max_nodes,
                "integrity_ok": integrity_ok,
                "warnings": warnings,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn consolidate(&self, prune_days: i64, max_summaries: i64) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let before: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories before consolidation")?;

            // Delete stale zero-access memories older than prune_days
            let pruned_stale = conn
                .execute(
                    "DELETE FROM memories WHERE access_count = 0 AND datetime(created_at) < datetime('now', '-' || ?1 || ' days')",
                    params![prune_days],
                )
                .unwrap_or(0);

            // Cap session summaries
            let summary_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE event_type = 'session_summary'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let pruned_summaries = if summary_count > max_summaries {
                conn.execute(
                    "DELETE FROM memories WHERE event_type = 'session_summary' AND id NOT IN (
                        SELECT id FROM memories WHERE event_type = 'session_summary' ORDER BY created_at DESC LIMIT ?1
                    )",
                    params![max_summaries],
                )
                .unwrap_or(0)
            } else {
                0
            };

            // Clean orphaned relationships
            let pruned_edges = conn
                .execute(
                    "DELETE FROM relationships WHERE source_id NOT IN (SELECT id FROM memories) OR target_id NOT IN (SELECT id FROM memories)",
                    [],
                )
                .unwrap_or(0);

            // Sync FTS
            let _fts_cleaned = conn
                .execute(
                    "DELETE FROM memories_fts WHERE rowid NOT IN (SELECT rowid FROM memories)",
                    [],
                )
                .unwrap_or(0);

            let after: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories after consolidation")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "before": before,
                "after": after,
                "pruned_stale": pruned_stale,
                "pruned_summaries": pruned_summaries,
                "pruned_edges": pruned_edges,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);
        let event_type = event_type.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            // Get candidates
            let mut stmt = conn
                .prepare("SELECT id, content FROM memories WHERE event_type = ?1")
                .context("failed to prepare compact query")?;

            let candidates: Vec<(String, String)> = stmt
                .query_map(params![event_type], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })
                .context("failed to query compact candidates")?
                .filter_map(|r| r.ok())
                .collect();

            if candidates.len() < min_cluster_size {
                return Ok::<_, anyhow::Error>(serde_json::json!({
                    "clusters_found": 0,
                    "memories_compacted": 0,
                    "dry_run": dry_run,
                    "clusters": [],
                }));
            }

            // Build word sets for Jaccard comparison
            let word_sets: Vec<HashSet<String>> = candidates
                .iter()
                .map(|(_, content)| {
                    content
                        .split_whitespace()
                        .map(|w| w.to_lowercase())
                        .collect()
                })
                .collect();

            // Union-Find clustering
            let n = candidates.len();
            let mut parent: Vec<usize> = (0..n).collect();

            fn find(parent: &mut [usize], x: usize) -> usize {
                if parent[x] != x {
                    parent[x] = find(parent, parent[x]);
                }
                parent[x]
            }

            for i in 0..n {
                for j in (i + 1)..n {
                    let intersection = word_sets[i].intersection(&word_sets[j]).count();
                    let union = word_sets[i].union(&word_sets[j]).count();
                    if union > 0 {
                        let similarity = intersection as f64 / union as f64;
                        if similarity >= similarity_threshold {
                            let pi = find(&mut parent, i);
                            let pj = find(&mut parent, j);
                            parent[pi] = pj;
                        }
                    }
                }
            }

            // Group clusters
            let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
            for i in 0..n {
                let root = find(&mut parent, i);
                clusters.entry(root).or_default().push(i);
            }

            let valid_clusters: Vec<Vec<usize>> = clusters
                .into_values()
                .filter(|c| c.len() >= min_cluster_size)
                .collect();

            let mut total_compacted = 0usize;
            let mut cluster_details: Vec<serde_json::Value> = Vec::new();

            for cluster in &valid_clusters {
                let preview: String = candidates[cluster[0]]
                    .1
                    .chars()
                    .take(100)
                    .collect();

                cluster_details.push(serde_json::json!({
                    "size": cluster.len(),
                    "preview": preview,
                }));

                if !dry_run {
                    // Merge: keep the first, update its content, delete the rest
                    let merged_content: String = cluster
                        .iter()
                        .map(|&idx| candidates[idx].1.as_str())
                        .collect::<Vec<_>>()
                        .join("\n---\n");

                    let keep_id = &candidates[cluster[0]].0;
                    conn.execute(
                        "UPDATE memories SET content = ?1 WHERE id = ?2",
                        params![merged_content, keep_id],
                    )
                    .context("failed to update merged memory")?;

                    // Update FTS
                    let _fts = conn.execute(
                        "UPDATE memories_fts SET content = ?1 WHERE rowid = (SELECT rowid FROM memories WHERE id = ?2)",
                        params![merged_content, keep_id],
                    );

                    for &idx in &cluster[1..] {
                        let del_id = &candidates[idx].0;
                        conn.execute("DELETE FROM memories_fts WHERE rowid = (SELECT rowid FROM memories WHERE id = ?1)", params![del_id]).ok();
                        conn.execute("DELETE FROM relationships WHERE source_id = ?1 OR target_id = ?1", params![del_id]).ok();
                        conn.execute("DELETE FROM memories WHERE id = ?1", params![del_id])
                            .context("failed to delete compacted memory")?;
                    }

                    total_compacted += cluster.len() - 1;
                }
            }

            Ok::<_, anyhow::Error>(serde_json::json!({
                "clusters_found": valid_clusters.len(),
                "memories_compacted": total_compacted,
                "dry_run": dry_run,
                "clusters": cluster_details,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn clear_session(&self, session_id: &str) -> Result<usize> {
        let conn = Arc::clone(&self.conn);
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            // Delete relationships first
            conn.execute(
                "DELETE FROM relationships WHERE source_id IN (SELECT id FROM memories WHERE session_id = ?1) OR target_id IN (SELECT id FROM memories WHERE session_id = ?1)",
                params![session_id],
            ).ok();

            // Delete FTS entries
            conn.execute(
                "DELETE FROM memories_fts WHERE rowid IN (SELECT rowid FROM memories WHERE session_id = ?1)",
                params![session_id],
            ).ok();

            // Delete memories
            let deleted = conn
                .execute(
                    "DELETE FROM memories WHERE session_id = ?1",
                    params![session_id],
                )
                .context("failed to clear session memories")?;

            Ok::<_, anyhow::Error>(deleted)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl WelcomeProvider for SqliteStorage {
    async fn welcome(
        &self,
        _session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);
        let project = project.map(ToString::to_string);

        let db_result = tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories")?;

            let mut sql =
                String::from("SELECT id, content, event_type, priority, created_at FROM memories");
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(ref proj) = project {
                sql.push_str(" WHERE project = ?1");
                params_values.push(rusqlite::types::Value::Text(proj.clone()));
            }

            sql.push_str(" ORDER BY created_at DESC LIMIT 15");

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare welcome query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for v in &params_values {
                param_refs.push(v);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(200).collect::<String>(),
                        "event_type": row.get::<_, Option<String>>(2)?,
                        "priority": row.get::<_, Option<i64>>(3)?,
                        "created_at": row.get::<_, String>(4)?,
                    }))
                })
                .context("failed to query recent memories")?;

            let recent: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

            // Explicitly surface user_preference and user_fact memories
            let mut prefs_stmt = conn
                .prepare(
                    "SELECT id, content, event_type, importance, created_at FROM memories \
                     WHERE event_type IN ('user_preference', 'user_fact') \
                     ORDER BY importance DESC, created_at DESC LIMIT 20",
                )
                .context("failed to prepare user preferences query")?;
            let pref_rows = prefs_stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(300).collect::<String>(),
                        "event_type": row.get::<_, Option<String>>(2)?,
                        "importance": row.get::<_, f64>(3)?,
                        "created_at": row.get::<_, String>(4)?,
                    }))
                })
                .context("failed to query user preferences")?;
            let user_context: Vec<serde_json::Value> = pref_rows.filter_map(|r| r.ok()).collect();

            Ok::<_, anyhow::Error>((total, recent, user_context))
        })
        .await
        .context("spawn_blocking join error")??;

        let (total, recent, user_context) = db_result;

        // Get profile and pending reminders via existing trait impls
        let profile = <Self as ProfileManager>::get_profile(self)
            .await
            .unwrap_or(serde_json::json!({}));
        let reminders = <Self as ReminderManager>::list_reminders(self, Some("pending"))
            .await
            .unwrap_or_default();

        let greeting = format!("Welcome back! You have {total} memories stored.");

        Ok(serde_json::json!({
            "greeting": greeting,
            "memory_count": total,
            "recent_memories": recent,
            "user_context": user_context,
            "profile": profile,
            "pending_reminders": reminders,
        }))
    }
}

#[async_trait]
impl StatsProvider for SqliteStorage {
    async fn type_stats(&self) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped') as etype, COUNT(*) as cnt FROM memories GROUP BY event_type ORDER BY cnt DESC")
                .context("failed to prepare type_stats query")?;

            let rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .context("failed to query type stats")?;

            let mut result = serde_json::Map::new();
            let mut total = 0i64;
            for row in rows {
                let (etype, cnt) = row.context("failed to decode type stat row")?;
                total += cnt;
                result.insert(etype, serde_json::json!(cnt));
            }
            result.insert("_total".to_string(), serde_json::json!(total));

            Ok::<_, anyhow::Error>(serde_json::Value::Object(result))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn session_stats(&self) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let mut stmt = conn
                .prepare("SELECT session_id, COUNT(*) as cnt FROM memories WHERE session_id IS NOT NULL GROUP BY session_id ORDER BY cnt DESC LIMIT 20")
                .context("failed to prepare session_stats query")?;

            let rows = stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "session_id": row.get::<_, String>(0)?,
                        "count": row.get::<_, i64>(1)?,
                    }))
                })
                .context("failed to query session stats")?;

            let results: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

            Ok::<_, anyhow::Error>(serde_json::json!({
                "sessions": results,
                "total_sessions": results.len(),
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);

            let period_new: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', '-{days} days')"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let session_count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(DISTINCT session_id) FROM memories WHERE datetime(created_at) >= datetime('now', '-{days} days') AND session_id IS NOT NULL"),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Type breakdown in period
            let mut stmt = conn
                .prepare(&format!("SELECT COALESCE(event_type, 'untyped'), COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', '-{days} days') GROUP BY event_type ORDER BY COUNT(*) DESC"))
                .context("failed to prepare digest type breakdown")?;

            let breakdown_rows = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .context("failed to query type breakdown")?;

            let mut type_breakdown = serde_json::Map::new();
            for (etype, cnt) in breakdown_rows.flatten() {
                type_breakdown.insert(etype, serde_json::json!(cnt));
            }

            // Previous period count for growth calc
            let prev_count: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', '-{} days') AND datetime(created_at) < datetime('now', '-{days} days')", days * 2),
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let growth_pct = if prev_count > 0 {
                ((period_new - prev_count) as f64 / prev_count as f64) * 100.0
            } else if period_new > 0 {
                100.0
            } else {
                0.0
            };

            Ok::<_, anyhow::Error>(serde_json::json!({
                "period_days": days,
                "total_memories": total,
                "period_new": period_new,
                "session_count": session_count,
                "type_breakdown": serde_json::Value::Object(type_breakdown),
                "growth_pct": (growth_pct * 100.0).round() / 100.0,
                "prev_period_count": prev_count,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        let conn = Arc::clone(&self.conn);

        tokio::task::spawn_blocking(move || {
            let conn = conn
                .lock()
                .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);

            let zero_access: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE access_count = 0",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let avg_access: f64 = conn
                .query_row(
                    "SELECT COALESCE(AVG(access_count), 0.0) FROM memories",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0.0);

            // By type breakdown
            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped') as etype, COUNT(*) as cnt, AVG(access_count) as avg_ac, SUM(CASE WHEN access_count = 0 THEN 1 ELSE 0 END) as zero_cnt FROM memories GROUP BY event_type ORDER BY avg_ac DESC")
                .context("failed to prepare access rate by-type query")?;

            let by_type_rows = stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "event_type": row.get::<_, String>(0)?,
                        "count": row.get::<_, i64>(1)?,
                        "avg_access_count": row.get::<_, f64>(2)?,
                        "zero_access_count": row.get::<_, i64>(3)?,
                    }))
                })
                .context("failed to query access rate by type")?;

            let by_type: Vec<serde_json::Value> = by_type_rows.filter_map(|r| r.ok()).collect();

            // Top 10 most accessed
            let mut stmt2 = conn
                .prepare("SELECT id, content, access_count, event_type FROM memories WHERE access_count > 0 ORDER BY access_count DESC LIMIT 10")
                .context("failed to prepare top accessed query")?;

            let top_rows = stmt2
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(100).collect::<String>(),
                        "access_count": row.get::<_, i64>(2)?,
                        "event_type": row.get::<_, Option<String>>(3)?,
                    }))
                })
                .context("failed to query top accessed")?;

            let top_accessed: Vec<serde_json::Value> = top_rows.filter_map(|r| r.ok()).collect();

            let never_pct = if total > 0 {
                (zero_access as f64 / total as f64) * 100.0
            } else {
                0.0
            };

            Ok::<_, anyhow::Error>(serde_json::json!({
                "total_memories": total,
                "zero_access_count": zero_access,
                "never_accessed_pct": (never_pct * 100.0).round() / 100.0,
                "avg_access_count": (avg_access * 100.0).round() / 100.0,
                "by_type": by_type,
                "top_accessed": top_accessed,
            }))
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

    // Enable WAL mode for better concurrent read performance
    let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");
    // Truncate WAL on startup to reclaim disk space
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

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

        CREATE TABLE IF NOT EXISTS user_profile (
            key TEXT NOT NULL PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
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

    // Performance indexes
    let indexes = [
        "CREATE INDEX IF NOT EXISTS idx_memories_last_accessed ON memories(last_accessed_at)",
        "CREATE INDEX IF NOT EXISTS idx_memories_ttl ON memories(ttl_seconds)",
        "CREATE INDEX IF NOT EXISTS idx_memories_event_access ON memories(event_type, access_count)",
        "CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at)",
        "CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project)",
        "CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id)",
        "CREATE INDEX IF NOT EXISTS idx_memories_canonical ON memories(canonical_hash)",
        "CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id)",
        "CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id)",
    ];
    for idx in &indexes {
        let _ = conn.execute_batch(idx);
    }

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
        AdvancedSearcher, CheckpointInput, CheckpointManager, ExpirationSweeper, FeedbackRecorder,
        GraphTraverser, LessonQuerier, MaintenanceManager, PhraseSearcher, ProfileManager, Recents,
        ReminderManager, Retriever, SearchOptions, Searcher, SemanticSearcher, SimilarFinder,
        StatsProvider, Storage, TTL_EPHEMERAL, TTL_LONG_TERM, TTL_SHORT_TERM, Updater,
        WelcomeProvider, default_priority_for_event_type, default_ttl_for_event_type,
        is_valid_event_type, parse_duration,
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
            Some(TTL_EPHEMERAL)
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
                    entity_id: None,
                    agent_type: None,
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

    #[tokio::test]
    async fn test_profile_set_and_get() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as ProfileManager>::set_profile(
            &storage,
            &serde_json::json!({"name": "George", "timezone": "UTC"}),
        )
        .await
        .unwrap();

        let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
            .await
            .unwrap();
        assert_eq!(profile["name"], "George");
        assert_eq!(profile["timezone"], "UTC");
    }

    #[tokio::test]
    async fn test_profile_update_merge() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as ProfileManager>::set_profile(
            &storage,
            &serde_json::json!({"name": "George", "timezone": "UTC"}),
        )
        .await
        .unwrap();
        <SqliteStorage as ProfileManager>::set_profile(
            &storage,
            &serde_json::json!({"timezone": "PST"}),
        )
        .await
        .unwrap();

        let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
            .await
            .unwrap();
        assert_eq!(profile["name"], "George");
        assert_eq!(profile["timezone"], "PST");
    }

    #[tokio::test]
    async fn test_profile_preferences_augmentation() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content) in [
            ("pref-1", "User prefers small PRs"),
            ("pref-2", "User prefers concise status updates"),
        ] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
                &MemoryInput {
                    event_type: Some("user_preference".to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let profile = <SqliteStorage as ProfileManager>::get_profile(&storage)
            .await
            .unwrap();
        let prefs = profile["preferences_from_memory"].as_array().unwrap();
        assert!(prefs.len() >= 2);
    }

    #[tokio::test]
    async fn test_checkpoint_save_and_resume() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let id = <SqliteStorage as CheckpointManager>::save_checkpoint(
            &storage,
            CheckpointInput {
                task_title: "Cross-session work".to_string(),
                progress: "Added profile trait".to_string(),
                plan: Some("Implement storage next".to_string()),
                files_touched: None,
                decisions: None,
                key_context: None,
                next_steps: Some("Add tests".to_string()),
                session_id: Some("s-1".to_string()),
                project: Some("romega".to_string()),
            },
        )
        .await
        .unwrap();
        assert!(!id.is_empty());

        let resumed =
            <SqliteStorage as CheckpointManager>::resume_task(&storage, "Cross-session", None, 1)
                .await
                .unwrap();
        assert_eq!(resumed.len(), 1);
        assert!(
            resumed[0]["content"]
                .as_str()
                .unwrap()
                .contains("## Checkpoint: Cross-session work")
        );
    }

    #[tokio::test]
    async fn test_checkpoint_numbering() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for idx in 1..=3 {
            <SqliteStorage as CheckpointManager>::save_checkpoint(
                &storage,
                CheckpointInput {
                    task_title: "Repeated task".to_string(),
                    progress: format!("Progress {idx}"),
                    plan: None,
                    files_touched: None,
                    decisions: None,
                    key_context: None,
                    next_steps: None,
                    session_id: None,
                    project: None,
                },
            )
            .await
            .unwrap();
        }

        let resumed =
            <SqliteStorage as CheckpointManager>::resume_task(&storage, "Repeated", None, 3)
                .await
                .unwrap();
        let mut numbers: Vec<i64> = resumed
            .iter()
            .map(|entry| entry["metadata"]["checkpoint_number"].as_i64().unwrap())
            .collect();
        numbers.sort_unstable();
        assert_eq!(numbers, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn test_checkpoint_project_filter() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (project, progress) in [("proj-a", "A progress"), ("proj-b", "B progress")] {
            <SqliteStorage as CheckpointManager>::save_checkpoint(
                &storage,
                CheckpointInput {
                    task_title: "Shared task".to_string(),
                    progress: progress.to_string(),
                    plan: None,
                    files_touched: None,
                    decisions: None,
                    key_context: None,
                    next_steps: None,
                    session_id: None,
                    project: Some(project.to_string()),
                },
            )
            .await
            .unwrap();
        }

        let resumed = <SqliteStorage as CheckpointManager>::resume_task(
            &storage,
            "Shared",
            Some("proj-b"),
            5,
        )
        .await
        .unwrap();
        assert_eq!(resumed.len(), 1);
        assert!(
            resumed[0]["content"]
                .as_str()
                .unwrap()
                .contains("B progress")
        );
    }

    #[tokio::test]
    async fn test_reminder_create_and_list() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let reminder = <SqliteStorage as ReminderManager>::create_reminder(
            &storage,
            "Review PR E",
            "1h",
            Some("after lunch"),
            Some("session-1"),
            Some("romega"),
        )
        .await
        .unwrap();

        let reminder_id = reminder["reminder_id"].as_str().unwrap();
        let listed = <SqliteStorage as ReminderManager>::list_reminders(&storage, None)
            .await
            .unwrap();
        assert!(
            listed
                .iter()
                .any(|entry| entry["reminder_id"].as_str() == Some(reminder_id))
        );
    }

    #[tokio::test]
    async fn test_reminder_dismiss() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let reminder = <SqliteStorage as ReminderManager>::create_reminder(
            &storage,
            "Dismiss me",
            "30m",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let reminder_id = reminder["reminder_id"].as_str().unwrap();

        let dismissed = <SqliteStorage as ReminderManager>::dismiss_reminder(&storage, reminder_id)
            .await
            .unwrap();
        assert_eq!(dismissed["status"], "dismissed");

        let dismissed_list =
            <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("dismissed"))
                .await
                .unwrap();
        assert_eq!(dismissed_list.len(), 1);
        assert_eq!(dismissed_list[0]["status"], "dismissed");
    }

    #[tokio::test]
    async fn test_reminder_status_filter() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let first = <SqliteStorage as ReminderManager>::create_reminder(
            &storage,
            "pending item",
            "1h",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        let second = <SqliteStorage as ReminderManager>::create_reminder(
            &storage,
            "to dismiss",
            "2h",
            None,
            None,
            None,
        )
        .await
        .unwrap();
        <SqliteStorage as ReminderManager>::dismiss_reminder(
            &storage,
            second["reminder_id"].as_str().unwrap(),
        )
        .await
        .unwrap();

        let pending = <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("pending"))
            .await
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0]["reminder_id"], first["reminder_id"]);

        let all = <SqliteStorage as ReminderManager>::list_reminders(&storage, Some("all"))
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_reminder_duration_parsing() {
        assert_eq!(parse_duration("1h").unwrap().num_minutes(), 60);
        assert_eq!(parse_duration("30m").unwrap().num_minutes(), 30);
        assert_eq!(parse_duration("2d").unwrap().num_hours(), 48);
        assert_eq!(parse_duration("1w").unwrap().num_days(), 7);
        assert_eq!(parse_duration("1d12h").unwrap().num_hours(), 36);
    }

    #[test]
    fn test_reminder_invalid_duration() {
        for input in ["", "0m", "10x", "1h30", "1m1h", "-1h"] {
            assert!(parse_duration(input).is_err());
        }
    }

    #[tokio::test]
    async fn test_lessons_query_basic() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, content) in [
            ("lesson-1", "Learned to keep checkpoints small"),
            ("lesson-2", "Learned to run clippy before commit"),
        ] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                content,
                &MemoryInput {
                    event_type: Some("lesson_learned".to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let lessons = <SqliteStorage as LessonQuerier>::query_lessons(
            &storage,
            Some("checkpoints"),
            None,
            None,
            None,
            5,
        )
        .await
        .unwrap();
        assert_eq!(lessons.len(), 1);
        assert!(
            lessons[0]["content"]
                .as_str()
                .unwrap()
                .contains("checkpoints")
        );
    }

    #[tokio::test]
    async fn test_lessons_exclude_session() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for (id, session) in [("ls-1", "s1"), ("ls-2", "s2")] {
            <SqliteStorage as Storage>::store(
                &storage,
                id,
                &format!("Lesson from {session}"),
                &MemoryInput {
                    event_type: Some("lesson_learned".to_string()),
                    session_id: Some(session.to_string()),
                    metadata: serde_json::json!({}),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        let lessons = <SqliteStorage as LessonQuerier>::query_lessons(
            &storage,
            None,
            None,
            Some("s2"),
            None,
            5,
        )
        .await
        .unwrap();
        assert_eq!(lessons.len(), 1);
        assert_eq!(lessons[0]["session_id"], "s1");
    }

    #[tokio::test]
    async fn test_lessons_dedup() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "dup-1",
            "placeholder one",
            &MemoryInput {
                event_type: Some("memory".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "dup-2",
            "placeholder two",
            &MemoryInput {
                event_type: Some("memory".to_string()),
                metadata: serde_json::json!({}),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Updater>::update(
            &storage,
            "dup-1",
            &MemoryUpdate {
                content: Some(
                    "The first eighty characters of this lesson are intentionally identical across both entries AAA111"
                        .to_string(),
                ),
                event_type: Some("lesson_learned".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Updater>::update(
            &storage,
            "dup-2",
            &MemoryUpdate {
                content: Some(
                    "The first eighty characters of this lesson are intentionally identical across both entries BBB222 with extra detail"
                        .to_string(),
                ),
                event_type: Some("lesson_learned".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let lessons =
            <SqliteStorage as LessonQuerier>::query_lessons(&storage, None, None, None, None, 10)
                .await
                .unwrap();
        assert_eq!(lessons.len(), 1);
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

        let profile_cols: Vec<String> = {
            let mut stmt = conn.prepare("PRAGMA table_info(user_profile)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        for col in ["key", "value", "updated_at"] {
            assert!(profile_cols.contains(&col.to_string()));
        }
    }

    // ── MaintenanceManager tests ──────────────────────────────────

    #[tokio::test]
    async fn test_health_check() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        let result =
            <SqliteStorage as MaintenanceManager>::check_health(&storage, 100.0, 200.0, 10000)
                .await
                .unwrap();
        assert_eq!(result["status"], "healthy");
        assert_eq!(result["integrity_ok"], true);
        assert_eq!(result["node_count"], 0);
    }

    #[tokio::test]
    async fn test_health_check_node_limit() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(&storage, "h-1", "some content", &MemoryInput::default())
            .await
            .unwrap();

        let result = <SqliteStorage as MaintenanceManager>::check_health(&storage, 100.0, 200.0, 1)
            .await
            .unwrap();
        assert_eq!(result["status"], "warning");
        assert_eq!(result["node_count"], 1);
    }

    #[tokio::test]
    async fn test_consolidate_prunes_stale() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "stale-1",
            "old content",
            &MemoryInput::default(),
        )
        .await
        .unwrap();
        // Back-date the memory and ensure zero access
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE memories SET created_at = datetime('now', '-60 days'), access_count = 0 WHERE id = ?1",
                params!["stale-1"],
            )
            .unwrap();
        }

        let result = <SqliteStorage as MaintenanceManager>::consolidate(&storage, 30, 100)
            .await
            .unwrap();
        assert!(result["pruned_stale"].as_i64().unwrap() >= 1);
        assert!(result["after"].as_i64().unwrap() < result["before"].as_i64().unwrap());
    }

    #[tokio::test]
    async fn test_consolidate_caps_summaries() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Insert directly via SQL to bypass store-time dedup
        let contents = [
            "Alpha quarterly revenue growth exceeded projections by fifteen percent",
            "Beta deployment pipeline migration completed with zero downtime achieved",
            "Gamma user authentication overhaul implemented with biometric support added",
            "Delta database sharding strategy finalized across three geographic regions",
            "Epsilon frontend performance optimization reduced load times significantly",
        ];
        {
            let conn = storage.conn.lock().unwrap();
            for (i, content) in contents.iter().enumerate() {
                conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, ?2, ?3, 'direct', 'session_summary', '[]', 0.5, '{}', 1)",
                    params![format!("sum-{i}"), content, format!("hash-{i}")],
                )
                .unwrap();
            }
        }

        let result = <SqliteStorage as MaintenanceManager>::consolidate(&storage, 365, 2)
            .await
            .unwrap();
        assert_eq!(result["pruned_summaries"].as_i64().unwrap(), 3);
        assert_eq!(result["after"].as_i64().unwrap(), 2);
    }

    #[tokio::test]
    async fn test_compact_dry_run() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Insert directly via SQL to bypass store-time dedup
        {
            let conn = storage.conn.lock().unwrap();
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, 'the exact same decision content repeated here', ?2, 'direct', 'decision', '[]', 0.5, '{}', 0)",
                    params![format!("dup-{i}"), format!("hash-dup-{i}")],
                )
                .unwrap();
            }
        }

        let result =
            <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, true)
                .await
                .unwrap();
        assert!(result["clusters_found"].as_i64().unwrap() >= 1);
        assert_eq!(result["memories_compacted"].as_i64().unwrap(), 0);
        assert_eq!(result["dry_run"], true);
    }

    #[tokio::test]
    async fn test_compact_merges() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Insert directly via SQL to bypass store-time dedup
        {
            let conn = storage.conn.lock().unwrap();
            for i in 0..3 {
                conn.execute(
                    "INSERT INTO memories (id, content, content_hash, source_type, event_type, tags, importance, metadata, access_count)
                     VALUES (?1, 'the exact same decision content repeated here for merging', ?2, 'direct', 'decision', '[]', 0.5, '{}', 0)",
                    params![format!("cm-{i}"), format!("hash-cm-{i}")],
                )
                .unwrap();
            }
        }

        let result =
            <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, false)
                .await
                .unwrap();
        assert!(result["memories_compacted"].as_i64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn test_compact_below_threshold() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "lone-1",
            "only one decision memory",
            &MemoryInput {
                event_type: Some("decision".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result =
            <SqliteStorage as MaintenanceManager>::compact(&storage, "decision", 0.5, 2, false)
                .await
                .unwrap();
        assert_eq!(result["clusters_found"].as_i64().unwrap(), 0);
    }

    #[tokio::test]
    async fn test_clear_session() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for i in 0..2 {
            <SqliteStorage as Storage>::store(
                &storage,
                &format!("cs-a-{i}"),
                &format!("session a content {i}"),
                &MemoryInput {
                    session_id: Some("sess-a".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        <SqliteStorage as Storage>::store(
            &storage,
            "cs-b-0",
            "session b content",
            &MemoryInput {
                session_id: Some("sess-b".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let deleted = <SqliteStorage as MaintenanceManager>::clear_session(&storage, "sess-a")
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        // Verify only sess-b remains
        let remaining: i64 = {
            let conn = storage.conn.lock().unwrap();
            conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap()
        };
        assert_eq!(remaining, 1);
    }

    // ── WelcomeProvider tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_welcome_briefing() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(&storage, "w-1", "first memory", &MemoryInput::default())
            .await
            .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "w-2",
            "second memory",
            &MemoryInput::default(),
        )
        .await
        .unwrap();

        let result = <SqliteStorage as WelcomeProvider>::welcome(&storage, None, None)
            .await
            .unwrap();
        assert_eq!(result["memory_count"], 2);
        assert!(result["greeting"].as_str().unwrap().contains("2 memories"));
        assert_eq!(result["recent_memories"].as_array().unwrap().len(), 2);
        assert!(result["user_context"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_welcome_surfaces_user_context() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Store a user_preference memory
        <SqliteStorage as Storage>::store(
            &storage,
            "uc-1",
            "user prefers dark mode for all interfaces",
            &MemoryInput {
                event_type: Some("user_preference".to_string()),
                importance: 0.9,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // Store a user_fact memory
        <SqliteStorage as Storage>::store(
            &storage,
            "uc-2",
            "user lives in San Francisco and works in fintech",
            &MemoryInput {
                event_type: Some("user_fact".to_string()),
                importance: 0.8,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        // Store a regular memory (should not appear in user_context)
        <SqliteStorage as Storage>::store(
            &storage,
            "uc-3",
            "deployed version 2.1 to production successfully",
            &MemoryInput {
                event_type: Some("task_completion".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = <SqliteStorage as WelcomeProvider>::welcome(&storage, None, None)
            .await
            .unwrap();
        assert_eq!(result["memory_count"], 3);
        let user_ctx = result["user_context"].as_array().unwrap();
        assert_eq!(user_ctx.len(), 2);
        // Ordered by importance DESC — user_preference (0.9) first
        assert_eq!(user_ctx[0]["event_type"], "user_preference");
        assert_eq!(user_ctx[1]["event_type"], "user_fact");
    }

    // ── StatsProvider tests ───────────────────────────────────────

    #[tokio::test]
    async fn test_type_stats() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        // Use very different content to avoid Jaccard dedup for same event_type
        <SqliteStorage as Storage>::store(
            &storage,
            "ts-d-0",
            "chose postgresql for the primary relational datastore backend",
            &MemoryInput {
                event_type: Some("decision".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ts-d-1",
            "migrated frontend framework from angular to react with typescript",
            &MemoryInput {
                event_type: Some("decision".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ts-l-0",
            "learned that connection pooling prevents timeout errors under load",
            &MemoryInput {
                event_type: Some("lesson_learned".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = <SqliteStorage as StatsProvider>::type_stats(&storage)
            .await
            .unwrap();
        assert_eq!(result["decision"], 2);
        assert_eq!(result["lesson_learned"], 1);
        assert_eq!(result["_total"], 3);
    }

    #[tokio::test]
    async fn test_session_stats() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        for i in 0..2 {
            <SqliteStorage as Storage>::store(
                &storage,
                &format!("ss-1-{i}"),
                &format!("s1 content {i}"),
                &MemoryInput {
                    session_id: Some("s1".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        <SqliteStorage as Storage>::store(
            &storage,
            "ss-2-0",
            "s2 content",
            &MemoryInput {
                session_id: Some("s2".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let result = <SqliteStorage as StatsProvider>::session_stats(&storage)
            .await
            .unwrap();
        assert_eq!(result["total_sessions"], 2);
        let sessions = result["sessions"].as_array().unwrap();
        assert_eq!(sessions.len(), 2);
    }

    #[tokio::test]
    async fn test_weekly_digest() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "wd-1",
            "recent memory one",
            &MemoryInput::default(),
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "wd-2",
            "recent memory two",
            &MemoryInput::default(),
        )
        .await
        .unwrap();

        let result = <SqliteStorage as StatsProvider>::weekly_digest(&storage, 7)
            .await
            .unwrap();
        assert_eq!(result["total_memories"], 2);
        assert_eq!(result["period_new"], 2);
        assert_eq!(result["period_days"], 7);
    }

    #[tokio::test]
    async fn test_access_rate_stats() {
        let storage = SqliteStorage::new_in_memory().unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ar-1",
            "accessed memory",
            &MemoryInput::default(),
        )
        .await
        .unwrap();
        <SqliteStorage as Storage>::store(
            &storage,
            "ar-2",
            "never accessed memory",
            &MemoryInput::default(),
        )
        .await
        .unwrap();
        // Set access_count on one
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "UPDATE memories SET access_count = 5 WHERE id = ?1",
                params!["ar-1"],
            )
            .unwrap();
        }

        let result = <SqliteStorage as StatsProvider>::access_rate_stats(&storage)
            .await
            .unwrap();
        assert_eq!(result["total_memories"], 2);
        assert_eq!(result["zero_access_count"], 1);
        assert!(!result["top_accessed"].as_array().unwrap().is_empty());
    }
}
