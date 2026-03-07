use super::*;

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

        tokio::task::spawn_blocking(move || {
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
        .context("spawn_blocking join error")?
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

        tokio::task::spawn_blocking(move || {
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

                    let tx = conn
                        .unchecked_transaction()
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
        .context("spawn_blocking join error")?
    }

    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .context("failed to count memories for auto_compact")?;

            if (total as usize) < count_threshold {
                return Ok::<_, anyhow::Error>(serde_json::json!({
                    "triggered": false,
                    "total_memories": total,
                    "count_threshold": count_threshold,
                    "message": "Memory count below threshold; skipping auto-compact",
                }));
            }

            // Sweep event types that have dedup thresholds
            let sweep_types = [
                (crate::memory_core::EventType::ErrorPattern, "error_pattern"),
                (
                    crate::memory_core::EventType::SessionSummary,
                    "session_summary",
                ),
                (
                    crate::memory_core::EventType::TaskCompletion,
                    "task_completion",
                ),
                (crate::memory_core::EventType::Decision, "decision"),
                (
                    crate::memory_core::EventType::LessonLearned,
                    "lesson_learned",
                ),
            ];

            let mut total_compacted = 0usize;
            let mut type_results: Vec<serde_json::Value> = Vec::new();

            for (et, et_str) in &sweep_types {
                let threshold = et.dedup_threshold().unwrap_or(0.80);

                // Fetch candidates with embeddings
                let mut stmt = conn
                    .prepare(
                        "SELECT id, content, embedding FROM memories \
                         WHERE event_type = ?1 AND embedding IS NOT NULL \
                         AND superseded_by_id IS NULL",
                    )
                    .context("failed to prepare auto_compact query")?;

                let candidates: Vec<(String, String, Vec<u8>)> = stmt
                    .query_map(params![et_str], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
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
                        let sim = cosine_similarity(
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

                for cluster in &valid_clusters {
                    if !dry_run {
                        // Keep the first (longest content), supersede the rest
                        let keep_idx = *cluster
                            .iter()
                            .max_by_key(|&&idx| candidates[idx].1.len())
                            .unwrap();
                        let keep_id = &candidates[keep_idx].0;

                        let tx = conn
                            .unchecked_transaction()
                            .context("failed to start auto_compact transaction")?;

                        for &idx in cluster {
                            if idx == keep_idx {
                                continue;
                            }
                            let del_id = &candidates[idx].0;

                            // Mark as superseded rather than delete
                            tx.execute(
                                "UPDATE memories SET superseded_by_id = ?1 WHERE id = ?2",
                                params![keep_id, del_id],
                            )
                            .context("failed to supersede memory during auto_compact")?;

                            type_compacted += 1;
                        }

                        tx.commit()
                            .context("failed to commit auto_compact transaction")?;
                    } else {
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
        .context("spawn_blocking join error")?
    }

    async fn clear_session(&self, session_id: &str) -> Result<usize> {
        let pool = Arc::clone(&self.pool);
        let session_id = session_id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let tx = conn
                .unchecked_transaction()
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
        let pool = Arc::clone(&self.pool);
        let project = project.map(ToString::to_string);

        let db_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories WHERE superseded_by_id IS NULL", [], |row| row.get(0))
                .context("failed to count memories")?;

            let mut sql =
                String::from("SELECT id, content, event_type, priority, created_at FROM memories WHERE superseded_by_id IS NULL");
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(ref proj) = project {
                sql.push_str(" AND project = ?1");
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
                     AND superseded_by_id IS NULL \
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
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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

            let total_sessions: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT session_id) FROM memories WHERE session_id IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or_else(|e| {
                    tracing::warn!("failed to count total sessions: {e}");
                    results.len() as i64
                });

            Ok::<_, anyhow::Error>(serde_json::json!({
                "sessions": results,
                "total_sessions": total_sessions,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
                .unwrap_or(0);

            if days <= 0 {
                anyhow::bail!("days must be > 0");
            }
            let prev_days = days
                .checked_mul(2)
                .ok_or_else(|| anyhow::anyhow!("days value is too large"))?;
            let days_str = format!("-{days} days");
            let prev_days_str = format!("-{prev_days} days");

            let period_new: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1)",
                    params![days_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let session_count: i64 = conn
                .query_row(
                    "SELECT COUNT(DISTINCT session_id) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) AND session_id IS NOT NULL",
                    params![days_str],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            // Type breakdown in period
            let mut stmt = conn
                .prepare("SELECT COALESCE(event_type, 'untyped'), COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) GROUP BY event_type ORDER BY COUNT(*) DESC")
                .context("failed to prepare digest type breakdown")?;

            let breakdown_rows = stmt
                .query_map(params![days_str], |row| {
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
                    "SELECT COUNT(*) FROM memories WHERE datetime(created_at) >= datetime('now', ?1) AND datetime(created_at) < datetime('now', ?2)",
                    params![prev_days_str, days_str],
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
        let pool = Arc::clone(&self.pool);

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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
