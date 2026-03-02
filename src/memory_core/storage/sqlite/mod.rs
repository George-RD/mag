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
    AdvancedSearcher, CheckpointInput, CheckpointManager, Deleter, ExpirationSweeper,
    FeedbackRecorder, GraphNode, GraphTraverser, LessonQuerier, ListResult, Lister,
    MaintenanceManager, MemoryInput, MemoryUpdate, PhraseSearcher, ProfileManager, Recents,
    Relationship, RelationshipQuerier, ReminderManager, Retriever, ScoringParams, SearchOptions,
    SearchResult, Searcher, SemanticResult, SemanticSearcher, SimilarFinder, StatsProvider,
    Storage, Tagger, Updater, VersionChainQuerier, WelcomeProvider,
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

/// Event types that support ingest-time auto-supersession.
/// These represent facts/preferences that evolve over time - newer replaces older.
/// Excluded: error_pattern (accumulates), session_summary (episodic), task_completion (one-time).
const SUPERSESSION_TYPES: &[&str] = &["decision", "lesson_learned", "user_preference"];

/// Cosine similarity threshold for auto-supersession detection (primary signal).
/// Semantic similarity catches updates even when wording changes significantly.
const SUPERSESSION_COSINE_THRESHOLD: f32 = 0.70;

/// Jaccard similarity threshold for auto-supersession detection (secondary signal).
/// Ensures topical overlap — prevents cross-topic false matches from cosine alone.
/// Must be well below dedup thresholds (0.80-0.85).
const SUPERSESSION_JACCARD_THRESHOLD: f64 = 0.30;

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
    scoring_params: ScoringParams,
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
            scoring_params: ScoringParams::default(),
        })
    }

    #[allow(dead_code)]
    pub fn with_scoring_params(mut self, mut params: ScoringParams) -> Self {
        if params.graph_seed_min > params.graph_seed_max {
            std::mem::swap(&mut params.graph_seed_min, &mut params.graph_seed_max);
        }
        if !params.rrf_k.is_finite() || params.rrf_k <= 0.0 {
            params.rrf_k = ScoringParams::default().rrf_k;
        }
        self.scoring_params = params;
        self
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
        if !(0.0..=1.0).contains(&weight) {
            return Err(anyhow!(
                "relationship weight must be between 0.0 and 1.0, got {weight}"
            ));
        }
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
                            access_count, session_id, event_type, project, priority, entity_id, agent_type,
                            ttl_seconds, version_chain_id, superseded_by_id, superseded_at
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
                    let version_chain_id: Option<String> = row.get(21).ok();
                    let superseded_by_id: Option<String> = row.get(22).ok();
                    let superseded_at: Option<String> = row.get(23).ok();
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
                        "version_chain_id": version_chain_id,
                        "superseded_by_id": superseded_by_id,
                        "superseded_at": superseded_at,
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
                let version_chain_id = mem["version_chain_id"].as_str();
                let superseded_by_id = mem["superseded_by_id"].as_str();
                let superseded_at = mem["superseded_at"].as_str();
                let parent_id = mem["parent_id"].as_str();
                let created_at = mem["created_at"].as_str();
                let event_at = mem["event_at"].as_str();
                let last_accessed_at = mem["last_accessed_at"].as_str();

                tx.execute(
                    "INSERT OR REPLACE INTO memories (
                        id, content, content_hash, source_type, tags, importance, metadata, access_count,
                        session_id, event_type, project, priority, entity_id, agent_type, ttl_seconds,
                        canonical_hash, version_chain_id, superseded_by_id, superseded_at,
                        parent_id, created_at, event_at, last_accessed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
                               ?20, COALESCE(?21, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                               COALESCE(?22, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                               COALESCE(?23, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
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
                        version_chain_id,
                        superseded_by_id,
                        superseded_at,
                        parent_id,
                        created_at,
                        event_at,
                        last_accessed_at,
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
            scoring_params: ScoringParams::default(),
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

    #[cfg(test)]
    fn debug_get_versioning_fields(&self, id: &str) -> Result<(Option<String>, Option<String>)> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

        let value: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT superseded_by_id, version_chain_id FROM memories WHERE id = ?1",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .context("failed to query versioning fields")?;

        value.ok_or_else(|| anyhow!("memory not found for id={id}"))
    }

    #[cfg(test)]
    fn debug_has_relationship(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: &str,
    ) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| anyhow!("sqlite connection mutex poisoned"))?;

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relationships WHERE source_id = ?1 AND target_id = ?2 AND rel_type = ?3",
                params![source_id, target_id, rel_type],
                |row| row.get(0),
            )
            .context("failed to query relationship")?;
        Ok(count > 0)
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
            if let Err(e) = self
                .add_relationship(
                    &memory_id,
                    &target_id,
                    "related",
                    f64::from(score),
                    &serde_json::json!({}),
                )
                .await
            {
                tracing::warn!("auto-relate failed for memory {memory_id} -> {target_id}: {e}");
            }
        }

        Ok(())
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

mod admin;
mod advanced;
mod crud;
mod graph;
mod helpers;
mod lifecycle;
mod schema;
mod search;
mod session;

pub(crate) use helpers::cosine_similarity;
use helpers::{
    build_fts5_query, canonical_hash, matches_search_options, normalize_for_dedup,
    parse_metadata_from_db, parse_tags_from_db, resolve_priority,
};
use schema::{default_db_path, initialize_parent_dir, initialize_schema};

#[cfg(test)]
mod tests;
