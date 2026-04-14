use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::memory_core::{REL_PRECEDED_BY, REL_RELATED, REL_RELATES_TO};

use super::embedding_codec::decode_embedding;

#[cfg(not(feature = "sqlite-vec"))]
use super::embedding_codec::dot_product;

#[cfg(feature = "sqlite-vec")]
use super::helpers::{vec_distance_to_similarity, vec_knn_search};

impl super::SqliteStorage {
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
        let pool = Arc::clone(&self.pool);
        let source_id = source_id.to_string();
        let target_id = target_id.to_string();
        let rel_type = rel_type.to_string();
        let metadata_json =
            serde_json::to_string(metadata).context("failed to serialize relationship metadata")?;
        let rid = rel_id.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
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

    /// Returns a breakdown of relationship counts grouped by `rel_type`.
    ///
    /// Returns a `Vec<(rel_type, count)>` sorted by count descending.
    #[allow(dead_code)]
    pub async fn graph_edge_stats(&self) -> Result<Vec<(String, i64)>> {
        let pool = Arc::clone(&self.pool);
        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let mut stmt = conn
                .prepare("SELECT rel_type, COUNT(*) FROM relationships GROUP BY rel_type ORDER BY COUNT(*) DESC")
                .context("failed to prepare graph edge stats query")?;
            let rows = stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
                .context("failed to query graph edge stats")?;
            let mut result = Vec::new();
            for row in rows {
                result.push(row.context("failed to read graph edge stats row")?);
            }
            Ok(result)
        })
        .await
        .context("spawn_blocking join error")?
    }

    pub(super) async fn try_auto_relate(&self, memory_id: &str) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();
        let source_id_for_query = memory_id.clone();

        let similar_ids = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let source_embedding: Vec<u8> = conn
                .query_row(
                    "SELECT embedding FROM memories WHERE id = ?1",
                    params![source_id_for_query],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query source embedding for auto relate")?
                .ok_or_else(|| anyhow!("memory not found for auto relate"))?;
            let source_embedding: Vec<f32> = decode_embedding(&source_embedding)
                .context("failed to decode source embedding for auto relate")?;

            let mut ranked = Vec::new();

            #[cfg(feature = "sqlite-vec")]
            {
                let knn_results = vec_knn_search(&conn, &source_embedding, 20)?;
                let mut ttl_stmt = conn
                    .prepare(
                        "SELECT 1 FROM memories WHERE id = ?1
                         AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))",
                    )
                    .context("failed to prepare TTL check for auto relate")?;
                for (candidate_id, distance) in knn_results {
                    if candidate_id == source_id_for_query {
                        continue;
                    }
                    #[allow(clippy::cast_possible_truncation)]
                    let similarity = vec_distance_to_similarity(distance) as f32;
                    if similarity < 0.45 {
                        continue;
                    }
                    let valid: bool = ttl_stmt
                        .query_row(params![candidate_id], |_| Ok(true))
                        .optional()
                        .context("failed to check TTL for auto relate")?
                        .unwrap_or(false);
                    if valid {
                        ranked.push((candidate_id, similarity));
                    }
                }
                ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
                ranked.truncate(3);
            }

            #[cfg(not(feature = "sqlite-vec"))]
            {
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

                for row in rows {
                    let (id, embedding_blob) =
                        row.context("failed to decode auto relate row")?;
                    let embedding: Vec<f32> = decode_embedding(&embedding_blob)
                        .context("failed to decode candidate embedding for auto relate")?;
                    let score = dot_product(&source_embedding, &embedding);
                    if score >= 0.45 {
                        ranked.push((id, score));
                    }
                }

                ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
                ranked.truncate(3);
            }

            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")??;

        for (target_id, score) in similar_ids {
            if let Err(e) = self
                .add_relationship(
                    &memory_id,
                    &target_id,
                    REL_RELATED,
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

    /// Creates a PRECEDED_BY edge from the most recent memory in the same session
    /// to the newly stored memory. This gives the graph traversal temporal adjacency
    /// signals that vector similarity alone cannot provide.
    pub(super) async fn try_create_temporal_edges(
        &self,
        memory_id: &str,
        session_id: &str,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();
        let session_id = session_id.to_string();
        let memory_id_clone = memory_id.clone();

        let predecessor_id = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let id: Option<String> = conn
                .query_row(
                    "SELECT id FROM memories WHERE session_id = ?1 AND id != ?2 ORDER BY created_at DESC LIMIT 1",
                    params![session_id, memory_id_clone],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query predecessor for temporal edge")?;
            Ok::<_, anyhow::Error>(id)
        })
        .await
        .context("spawn_blocking join error")??;

        if let Some(pred_id) = predecessor_id {
            self.add_relationship(
                &pred_id,
                &memory_id,
                REL_PRECEDED_BY,
                1.0,
                &serde_json::json!({"source": "temporal_adjacency"}),
            )
            .await?;
        }

        Ok(())
    }

    /// Creates RELATES_TO edges between the new memory and other memories that share
    /// entity tags (e.g., `entity:people:alice`). Caps at 3 target memories per entity
    /// tag, max 5 entity tags processed, to keep edge creation bounded.
    pub(super) async fn try_create_entity_edges(
        &self,
        memory_id: &str,
        tags: &[String],
    ) -> Result<()> {
        let entity_tags: Vec<String> = tags
            .iter()
            .filter(|t| t.starts_with("entity:"))
            .take(5)
            .cloned()
            .collect();

        if entity_tags.is_empty() {
            return Ok(());
        }

        let pool = Arc::clone(&self.pool);
        let memory_id_owned = memory_id.to_string();

        // Fetch candidate targets and filter out existing relationships in one
        // spawn_blocking call, avoiding N separate relationship_exists round-trips.
        let targets = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let mut targets: Vec<(String, String)> = Vec::new(); // (target_id, entity_tag)

            let mut stmt = conn
                .prepare(
                    "SELECT m.id FROM memories m
                     WHERE json_valid(m.tags)
                       AND EXISTS (SELECT 1 FROM json_each(m.tags) WHERE value = ?1)
                       AND m.id != ?2
                       AND m.superseded_by_id IS NULL
                       AND NOT EXISTS (
                           SELECT 1 FROM relationships r
                           WHERE r.source_id = ?2 AND r.target_id = m.id AND r.rel_type = 'RELATES_TO'
                       )
                     ORDER BY m.created_at DESC LIMIT 3",
                )
                .context("failed to prepare entity co-occurrence query")?;

            for entity_tag in &entity_tags {
                let rows = stmt
                    .query_map(params![entity_tag, memory_id_owned], |row| {
                        row.get::<_, String>(0)
                    })
                    .context("failed to execute entity co-occurrence query")?;

                for row in rows {
                    let target_id = row.context("failed to decode entity co-occurrence row")?;
                    targets.push((target_id, entity_tag.clone()));
                }
            }

            Ok::<_, anyhow::Error>(targets)
        })
        .await
        .context("spawn_blocking join error")??;

        for (target_id, entity_tag) in targets {
            if let Err(e) = self
                .add_relationship(
                    memory_id,
                    &target_id,
                    REL_RELATES_TO,
                    0.7,
                    &serde_json::json!({"source": "entity_cooccurrence", "entity": entity_tag}),
                )
                .await
            {
                tracing::warn!(
                    memory_id = %memory_id,
                    target_id = %target_id,
                    error = %e,
                    "entity co-occurrence edge creation failed"
                );
            }
        }

        Ok(())
    }

    #[cfg(test)]
    pub(super) fn debug_has_relationship(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: &str,
    ) -> Result<bool> {
        let conn = self.pool.reader()?;

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM relationships WHERE source_id = ?1 AND target_id = ?2 AND rel_type = ?3",
                params![source_id, target_id, rel_type],
                |row| row.get(0),
            )
            .context("failed to query relationship")?;
        Ok(count > 0)
    }

    /// Checks whether a relationship already exists between source and target with the given type.
    #[allow(dead_code)]
    pub(super) async fn relationship_exists(
        &self,
        source_id: &str,
        target_id: &str,
        rel_type: &str,
    ) -> Result<bool> {
        let pool = Arc::clone(&self.pool);
        let source_id = source_id.to_string();
        let target_id = target_id.to_string();
        let rel_type = rel_type.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM relationships
                     WHERE source_id = ?1 AND target_id = ?2 AND rel_type = ?3)",
                    params![source_id, target_id, rel_type],
                    |row| row.get(0),
                )
                .context("failed to check relationship existence")?;
            Ok::<_, anyhow::Error>(exists)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
