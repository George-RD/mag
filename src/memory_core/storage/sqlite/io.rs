use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use rusqlite::params;

use super::conn_pool::retry_on_lock;
use super::helpers::canonical_hash;

impl super::SqliteStorage {
    /// Returns storage statistics as a JSON Value.
    pub async fn stats(&self) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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
                "paths": build_stats_paths_json(&db_path),
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    /// Exports all memories and relationships as a JSON string.
    pub async fn export_all(&self) -> Result<String> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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

        let pool = Arc::clone(&self.pool);

        let counts = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let tx = retry_on_lock(|| conn.unchecked_transaction())
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
        .context("spawn_blocking join error")?;

        // Bulk mutation — full cache clear.
        self.invalidate_query_cache();

        counts
    }
}

pub(super) fn build_stats_paths_json(db_path: &Path) -> serde_json::Value {
    let data_root = if db_path.as_os_str() == ":memory:" {
        serde_json::Value::Null
    } else {
        db_path
            .parent()
            .map(|path| serde_json::Value::String(path.display().to_string()))
            .unwrap_or(serde_json::Value::Null)
    };

    serde_json::json!({
        "database_path": db_path.display().to_string(),
        "data_root": data_root,
    })
}
