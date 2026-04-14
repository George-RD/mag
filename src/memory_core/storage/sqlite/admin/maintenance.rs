//! Maintenance operations for the SQLite storage backend.
//!
//! Implements the `MaintenanceManager` trait, covering database health checks,
//! memory consolidation (pruning stale/orphaned data), content-based compaction
//! (Jaccard clustering), embedding-based auto-compaction (cosine clustering),
//! and session data clearing.

use super::super::*;

#[async_trait]
impl MaintenanceManager for SqliteStorage {
    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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
            #[allow(clippy::cast_precision_loss)]
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
        let pool = Arc::clone(&self.pool);

        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let before: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories before consolidation")?;

            // Delete stale zero-access memories older than prune_days
            let pruned_stale = conn
                .execute(
                    "DELETE FROM memories WHERE access_count = 0 AND datetime(created_at) < datetime('now', '-' || ?1 || ' days')",
                    params![prune_days],
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("maintenance: failed to prune stale memories: {e}");
                    0
                });

            // Cap session summaries
            let summary_count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE event_type = 'session_summary'",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("maintenance: failed to count session summaries: {e}");
                    0
                });

            let pruned_summaries = if summary_count > max_summaries {
                conn.execute(
                    "DELETE FROM memories WHERE event_type = 'session_summary' AND id NOT IN (
                        SELECT id FROM memories WHERE event_type = 'session_summary' ORDER BY created_at DESC LIMIT ?1
                    )",
                    params![max_summaries],
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("maintenance: failed to prune excess summaries: {e}");
                    0
                })
            } else {
                0
            };

            // Clean orphaned relationships
            let pruned_edges = conn
                .execute(
                    "DELETE FROM relationships WHERE source_id NOT IN (SELECT id FROM memories) OR target_id NOT IN (SELECT id FROM memories)",
                    [],
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("maintenance: failed to prune orphaned relationships: {e}");
                    0
                });

            // Sync FTS
            let _fts_cleaned = conn
                .execute(
                    "DELETE FROM memories_fts WHERE id NOT IN (SELECT id FROM memories)",
                    [],
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("maintenance: failed to sync FTS orphans: {e}");
                    0
                });

            // Sync vec_memories
            #[cfg(feature = "sqlite-vec")]
            {
                let _ = conn
                    .execute(
                        "DELETE FROM vec_memories WHERE memory_id NOT IN (SELECT id FROM memories)",
                        [],
                    )
                    .unwrap_or_else(|e| {
                        tracing::warn!("maintenance: failed to sync vec_memories orphans: {e}");
                        0
                    });
            }

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
        .context("spawn_blocking join error")?;

        // Maintenance may have pruned memories — full cache clear.
        self.invalidate_query_cache();

        result
    }

    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);
        let event_type = event_type.to_string();

        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

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
                        #[allow(clippy::cast_precision_loss)]
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
                let preview: String = candidates[cluster[0]].1.chars().take(100).collect();

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

                    let tx = retry_on_lock(|| conn.unchecked_transaction())
                        .context("failed to start compaction transaction")?;

                    tx.execute(
                        "UPDATE memories SET content = ?1 WHERE id = ?2",
                        params![merged_content, keep_id],
                    )
                    .context("failed to update merged memory")?;

                    // Update FTS (keyed by id, not rowid)
                    tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![keep_id])
                        .context("failed to delete existing FTS row for merged memory")?;
                    tx.execute(
                        "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                        params![keep_id, merged_content],
                    )
                    .context("failed to update FTS during compaction")?;

                    for &idx in &cluster[1..] {
                        let del_id = &candidates[idx].0;
                        tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![del_id])
                            .context("failed to delete compacted memory from FTS")?;

                        #[cfg(feature = "sqlite-vec")]
                        vec_delete(&tx, del_id)?;

                        tx.execute(
                            "DELETE FROM relationships WHERE source_id = ?1 OR target_id = ?1",
                            params![del_id],
                        )
                        .context("failed to delete relationships for compacted memory")?;
                        tx.execute("DELETE FROM memories WHERE id = ?1", params![del_id])
                            .context("failed to delete compacted memory")?;
                    }

                    tx.commit()
                        .context("failed to commit compaction transaction")?;

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
        .context("spawn_blocking join error")?;

        if !dry_run {
            // Compaction may have modified/deleted memories — full cache clear.
            self.invalidate_query_cache();
        }

        result
    }

    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        let result = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories for auto_compact")?;

            if usize::try_from(total).unwrap_or(0) < count_threshold {
                return Ok::<_, anyhow::Error>(serde_json::json!({
                    "triggered": false,
                    "total_memories": total,
                    "count_threshold": count_threshold,
                    "message": "Memory count below threshold; skipping auto-compact",
                }));
            }

            // Sweep event types that have dedup thresholds (derived from EventType)
            let sweep_types = crate::memory_core::EventType::types_with_dedup_threshold();

            let mut total_compacted = 0usize;
            let mut type_results: Vec<serde_json::Value> = Vec::new();

            for et in &sweep_types {
                let threshold = et.dedup_threshold().unwrap_or(0.80);
                let et_str = et.to_string();

                // Fetch candidates with embeddings (capped at 1000 most recent to bound O(n^2))
                let mut stmt = conn
                    .prepare(
                        "SELECT id, LENGTH(content), embedding FROM memories \
                         WHERE event_type = ?1 AND embedding IS NOT NULL \
                         AND superseded_by_id IS NULL \
                         ORDER BY created_at DESC LIMIT 1000",
                    )
                    .context("failed to prepare auto_compact query")?;

                let candidates: Vec<(String, usize, Vec<u8>)> = stmt
                    .query_map(params![et_str], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, usize>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                        ))
                    })
                    .context("failed to query auto_compact candidates")?
                    .filter_map(|r| r.ok())
                    .collect();

                if candidates.len() < 2 {
                    continue;
                }

                // Decode embeddings
                let embeddings: Vec<Option<Vec<f32>>> = candidates
                    .iter()
                    .map(|(_, _, blob)| decode_embedding(blob).ok())
                    .collect();

                // Union-Find clustering by cosine similarity
                let n = candidates.len();
                let mut parent: Vec<usize> = (0..n).collect();

                fn find(parent: &mut [usize], x: usize) -> usize {
                    if parent[x] != x {
                        parent[x] = find(parent, parent[x]);
                    }
                    parent[x]
                }

                for i in 0..n {
                    if embeddings[i].is_none() {
                        continue;
                    }
                    for j in (i + 1)..n {
                        if embeddings[j].is_none() {
                            continue;
                        }
                        let sim = dot_product(
                            embeddings[i].as_ref().unwrap(),
                            embeddings[j].as_ref().unwrap(),
                        ) as f64;
                        if sim >= threshold {
                            let pi = find(&mut parent, i);
                            let pj = find(&mut parent, j);
                            parent[pi] = pj;
                        }
                    }
                }

                // Group clusters
                let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
                for i in 0..n {
                    let root = find(&mut parent, i);
                    clusters.entry(root).or_default().push(i);
                }

                let valid_clusters: Vec<Vec<usize>> =
                    clusters.into_values().filter(|c| c.len() >= 2).collect();

                let mut type_compacted = 0usize;

                if !valid_clusters.is_empty() && !dry_run {
                    // Single transaction for all clusters of this event type
                    let tx = retry_on_lock(|| conn.unchecked_transaction())
                        .context("failed to start auto_compact transaction")?;

                    for cluster in &valid_clusters {
                        // Keep the longest content, supersede the rest
                        let keep_idx = *cluster
                            .iter()
                            .max_by_key(|&&idx| candidates[idx].1)
                            .unwrap();
                        let keep_id = &candidates[keep_idx].0;

                        for &idx in cluster {
                            if idx == keep_idx {
                                continue;
                            }
                            let del_id = &candidates[idx].0;
                            tx.execute(
                                "UPDATE memories SET superseded_by_id = ?1 WHERE id = ?2",
                                params![keep_id, del_id],
                            )
                            .context("failed to supersede memory during auto_compact")?;
                            type_compacted += 1;
                        }
                    }

                    tx.commit()
                        .context("failed to commit auto_compact transaction")?;
                } else if dry_run {
                    for cluster in &valid_clusters {
                        type_compacted += cluster.len() - 1;
                    }
                }

                if !valid_clusters.is_empty() {
                    type_results.push(serde_json::json!({
                        "event_type": et_str,
                        "candidates": candidates.len(),
                        "clusters": valid_clusters.len(),
                        "compacted": type_compacted,
                        "threshold": threshold,
                    }));
                }

                total_compacted += type_compacted;
            }

            Ok::<_, anyhow::Error>(serde_json::json!({
                "triggered": true,
                "total_memories": total,
                "count_threshold": count_threshold,
                "dry_run": dry_run,
                "total_compacted": total_compacted,
                "by_type": type_results,
            }))
        })
        .await
        .context("spawn_blocking join error")?;

        if !dry_run {
            // Auto-compaction may have superseded memories — full cache clear.
            self.invalidate_query_cache();
        }

        result
    }

    async fn clear_session(&self, session_id: &str) -> Result<usize> {
        let pool = Arc::clone(&self.pool);
        let session_id = session_id.to_string();

        let deleted = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start clear_session transaction")?;

            // Delete relationships first
            tx.execute(
                "DELETE FROM relationships WHERE source_id IN (SELECT id FROM memories WHERE session_id = ?1) OR target_id IN (SELECT id FROM memories WHERE session_id = ?1)",
                params![session_id],
            ).context("failed to delete session relationships")?;

            // Delete FTS entries
            tx.execute(
                "DELETE FROM memories_fts WHERE id IN (SELECT id FROM memories WHERE session_id = ?1)",
                params![session_id],
            ).context("failed to delete session FTS entries")?;

            // Delete vec_memories entries
            #[cfg(feature = "sqlite-vec")]
            {
                tx.execute(
                    "DELETE FROM vec_memories WHERE memory_id IN (SELECT id FROM memories WHERE session_id = ?1)",
                    params![session_id],
                ).context("failed to delete session vec_memories entries")?;
            }

            // Delete memories
            let deleted = tx
                .execute(
                    "DELETE FROM memories WHERE session_id = ?1",
                    params![session_id],
                )
                .context("failed to clear session memories")?;

            tx.commit()
                .context("failed to commit clear_session transaction")?;

            Ok::<_, anyhow::Error>(deleted)
        })
        .await
        .context("spawn_blocking join error")??;

        if deleted > 0 {
            // Bulk session deletion — full cache clear.
            self.invalidate_query_cache();
        }

        Ok(deleted)
    }
}
