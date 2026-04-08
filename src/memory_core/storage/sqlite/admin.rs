use super::*;
use crate::memory_core::{BackupInfo, WelcomeOptions};

/// Maximum number of automatic backups to keep.
const MAX_BACKUPS: usize = 5;

/// Minimum interval between automatic backups (in seconds).
const BACKUP_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

/// The backup file pattern: `memory.db.YYYYMMDD_HHMMSS.bak`
const BACKUP_PREFIX: &str = "memory.db.";
const BACKUP_SUFFIX: &str = ".bak";

/// Returns the backups directory for a given database path.
fn backups_dir(db_path: &Path) -> Result<PathBuf> {
    let parent = db_path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent directory"))?;
    Ok(parent.join("backups"))
}

/// Collects backup entries from the backups directory, sorted oldest-first by filename.
fn collect_backup_entries(backups_dir: &Path) -> Result<Vec<fs::DirEntry>> {
    if !backups_dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries: Vec<fs::DirEntry> = fs::read_dir(backups_dir)
        .with_context(|| format!("failed to read backups directory {}", backups_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            name_str.starts_with(BACKUP_PREFIX) && name_str.ends_with(BACKUP_SUFFIX)
        })
        .collect();

    // Sort by filename ascending (timestamp-based, so alphabetical = chronological)
    entries.sort_by_key(|e| e.file_name());
    Ok(entries)
}

/// Create a binary backup of the database file.
/// Performs WAL checkpoint first to ensure all data is in the main file.
fn create_backup_sync(conn: &Connection, db_path: &Path) -> Result<BackupInfo> {
    // Skip for in-memory databases
    if db_path.as_os_str() == ":memory:" {
        anyhow::bail!("cannot create backup of in-memory database");
    }

    // 1. Checkpoint WAL to flush pending writes
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        tracing::debug!("backup WAL checkpoint skipped: {e}");
    }

    // 2. Create backups directory
    let dir = backups_dir(db_path)?;
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create backups directory {}", dir.display()))?;

    // 3. Copy DB file with timestamp
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let backup_filename = format!("{BACKUP_PREFIX}{timestamp}{BACKUP_SUFFIX}");
    let backup_path = dir.join(&backup_filename);
    fs::copy(db_path, &backup_path).with_context(|| {
        format!(
            "failed to copy database to backup {}",
            backup_path.display()
        )
    })?;

    let metadata = fs::metadata(&backup_path)
        .with_context(|| format!("failed to stat backup file {}", backup_path.display()))?;

    tracing::info!(
        path = %backup_path.display(),
        size_bytes = metadata.len(),
        "database backup created"
    );

    Ok(BackupInfo {
        path: backup_path,
        size_bytes: metadata.len(),
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Rotate backups, keeping only the `max_count` most recent. Returns the number removed.
fn rotate_backups_sync(db_path: &Path, max_count: usize) -> Result<usize> {
    let dir = backups_dir(db_path)?;
    let mut backups = collect_backup_entries(&dir)?;

    let mut removed = 0usize;
    while backups.len() > max_count {
        if let Some(oldest) = backups.first() {
            let path = oldest.path();
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove old backup {}", path.display()))?;
            tracing::debug!(path = %path.display(), "removed old backup");
            backups.remove(0);
            removed += 1;
        }
    }

    if removed > 0 {
        tracing::info!(
            removed,
            remaining = backups.len(),
            "backup rotation completed"
        );
    }

    Ok(removed)
}

/// List available backups with path and size.
fn list_backups_sync(db_path: &Path) -> Result<Vec<BackupInfo>> {
    let dir = backups_dir(db_path)?;
    let entries = collect_backup_entries(&dir)?;

    let mut backups = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to stat backup {}", path.display()))?;

        // Extract timestamp from filename: memory.db.YYYYMMDD_HHMMSS.bak
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let timestamp_str = name_str
            .strip_prefix(BACKUP_PREFIX)
            .and_then(|s| s.strip_suffix(BACKUP_SUFFIX))
            .unwrap_or("");
        // Parse YYYYMMDD_HHMMSS into a rough ISO 8601 timestamp
        let created_at = if timestamp_str.len() == 15 {
            format!(
                "{}-{}-{}T{}:{}:{}Z",
                &timestamp_str[0..4],
                &timestamp_str[4..6],
                &timestamp_str[6..8],
                &timestamp_str[9..11],
                &timestamp_str[11..13],
                &timestamp_str[13..15],
            )
        } else {
            String::new()
        };

        backups.push(BackupInfo {
            path,
            size_bytes: metadata.len(),
            created_at,
        });
    }

    Ok(backups)
}

/// Restore from a backup file. Creates a safety backup of the current DB first.
fn restore_backup_sync(conn: &Connection, backup_path: &Path, db_path: &Path) -> Result<()> {
    if !backup_path.exists() {
        anyhow::bail!("backup file does not exist: {}", backup_path.display());
    }
    if db_path.as_os_str() == ":memory:" {
        anyhow::bail!("cannot restore backup to in-memory database");
    }

    // Create a safety backup of the current DB
    let dir = backups_dir(db_path)?;
    fs::create_dir_all(&dir)?;
    let safety_name = format!(
        "{BACKUP_PREFIX}pre_restore_{}{BACKUP_SUFFIX}",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let safety_path = dir.join(&safety_name);

    // Checkpoint WAL before copying
    if let Err(e) = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
        tracing::debug!("restore pre-backup WAL checkpoint skipped: {e}");
    }

    fs::copy(db_path, &safety_path).with_context(|| {
        format!(
            "failed to create safety backup at {}",
            safety_path.display()
        )
    })?;
    tracing::info!(
        safety_backup = %safety_path.display(),
        "created safety backup before restore"
    );

    // Copy backup over current DB
    fs::copy(backup_path, db_path).with_context(|| {
        format!(
            "failed to restore backup from {} to {}",
            backup_path.display(),
            db_path.display()
        )
    })?;

    // Remove any WAL/SHM files that might be stale after restore
    let wal_path = db_path.with_extension("db-wal");
    let shm_path = db_path.with_extension("db-shm");
    if wal_path.exists() {
        let _ = fs::remove_file(&wal_path);
    }
    if shm_path.exists() {
        let _ = fs::remove_file(&shm_path);
    }

    tracing::info!(
        from = %backup_path.display(),
        to = %db_path.display(),
        "database restored from backup"
    );

    Ok(())
}

/// Check if the latest backup is older than the threshold interval.
fn needs_backup(db_path: &Path) -> Result<bool> {
    let dir = backups_dir(db_path)?;
    let entries = collect_backup_entries(&dir)?;

    let Some(latest) = entries.last() else {
        // No backups at all
        return Ok(true);
    };

    let metadata = fs::metadata(latest.path())?;
    let modified = metadata
        .modified()
        .context("failed to get backup modification time")?;
    let age = std::time::SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default();

    Ok(age.as_secs() >= BACKUP_INTERVAL_SECS)
}

#[async_trait]
impl crate::memory_core::BackupManager for SqliteStorage {
    async fn create_backup(&self) -> Result<BackupInfo> {
        let pool = Arc::clone(&self.pool);
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            create_backup_sync(&conn, &db_path)
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn rotate_backups(&self, max_count: usize) -> Result<usize> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || rotate_backups_sync(&db_path, max_count))
            .await
            .context("spawn_blocking join error")?
    }

    async fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || list_backups_sync(&db_path))
            .await
            .context("spawn_blocking join error")?
    }

    async fn restore_backup(&self, backup_path: &Path) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let db_path = self.db_path.clone();
        let backup_path = backup_path.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            restore_backup_sync(&conn, &backup_path, &db_path)
        })
        .await
        .context("spawn_blocking join error")?
    }

    async fn maybe_startup_backup(&self) -> Result<Option<BackupInfo>> {
        let pool = Arc::clone(&self.pool);
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            // Skip for in-memory databases
            if db_path.as_os_str() == ":memory:" {
                return Ok(None);
            }

            if !needs_backup(&db_path)? {
                tracing::debug!("startup backup skipped: latest backup is recent enough");
                return Ok(None);
            }

            let conn = pool.writer()?;
            let info = create_backup_sync(&conn, &db_path)?;
            let removed = rotate_backups_sync(&db_path, MAX_BACKUPS)?;
            if removed > 0 {
                tracing::info!(removed, "rotated old backups during startup");
            }

            Ok(Some(info))
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

/// Estimate the number of LLM tokens in a string using the 4-chars-per-token heuristic.
fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
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
        let project_for_semantic = project.clone();

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

        let (total, mut recent, user_context) = db_result;

        // ── Semantic search phase for welcome() ────────────────────────
        // If a project is specified, supplement recent memories with
        // semantically relevant results using a fixed ~1500-token budget.
        if let Some(ref proj) = project_for_semantic {
            const WELCOME_SEMANTIC_BUDGET: usize = 1500;
            let used_tokens: usize = recent
                .iter()
                .map(|m| estimate_tokens(m.get("content").and_then(|v| v.as_str()).unwrap_or("")))
                .sum();
            let remaining = WELCOME_SEMANTIC_BUDGET.saturating_sub(used_tokens);

            if remaining > 0 {
                let search_opts = SearchOptions {
                    project: Some(proj.clone()),
                    ..SearchOptions::default()
                };

                let mut seen_ids: HashSet<String> = recent
                    .iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect();

                let candidate_count = 10usize;
                match <SqliteStorage as AdvancedSearcher>::advanced_search(
                    self,
                    proj,
                    candidate_count,
                    &search_opts,
                )
                .await
                {
                    Ok(semantic_results) => {
                        let mut sem_remaining = remaining;
                        for sr in semantic_results {
                            if seen_ids.contains(&sr.id) {
                                continue;
                            }
                            let truncated: String = sr.content.chars().take(200).collect();
                            let tokens = estimate_tokens(&truncated);
                            if tokens > sem_remaining {
                                break;
                            }
                            sem_remaining = sem_remaining.saturating_sub(tokens);
                            seen_ids.insert(sr.id.clone());

                            let et_str = sr.event_type.as_ref().map(|e| e.to_string());
                            recent.push(serde_json::json!({
                                "id": sr.id,
                                "content": truncated,
                                "event_type": et_str,
                                "importance": sr.importance,
                                "source": "semantic",
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            project = proj.as_str(),
                            query_len = proj.len(),
                            candidate_count,
                            error_kind = %e.root_cause(),
                            "semantic search failed in welcome()"
                        );
                    }
                }
            }
        }

        // Get profile and pending reminders via existing trait impls
        let profile = <Self as ProfileManager>::get_profile(self)
            .await
            .unwrap_or(serde_json::json!({}));
        let reminders = <Self as ReminderManager>::list_reminders(self, Some("pending"))
            .await
            .unwrap_or_default();

        let greeting = if total == 0 {
            "Welcome to MAG! Store your first memory to get started.".to_string()
        } else {
            format!("Welcome back! You have {total} memories stored.")
        };

        Ok(serde_json::json!({
            "greeting": greeting,
            "memory_count": total,
            "recent_memories": recent,
            "user_context": user_context,
            "profile": profile,
            "pending_reminders": reminders,
        }))
    }

    async fn welcome_scoped(&self, opts: &WelcomeOptions) -> Result<serde_json::Value> {
        // If no budget and no scoping beyond what welcome() already supports, delegate exactly.
        // Note: welcome() already handles project filtering, so only agent_type and entity_id
        // count as "extra scope" that requires the budgeted path.
        let has_extra_scope = opts.agent_type.is_some() || opts.entity_id.is_some();
        if opts.budget_tokens.is_none() && !has_extra_scope {
            return self
                .welcome(opts.session_id.as_deref(), opts.project.as_deref())
                .await;
        }

        let budget = opts.budget_tokens.unwrap_or(usize::MAX);
        let pool = Arc::clone(&self.pool);
        let project = opts.project.clone();
        let agent_type = opts.agent_type.clone();
        let entity_id = opts.entity_id.clone();

        let db_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            // Total memory count (active, non-superseded)
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE superseded_by_id IS NULL",
                    [],
                    |row| row.get(0),
                )
                .context("failed to count memories")?;

            // Helper: build a common scope filter clause and params.
            // Returns (where_fragment, params_vec) where where_fragment starts with " AND …"
            // or is empty.  project / agent_type / entity_id filters are combined.
            let mut scope_clauses: Vec<String> = Vec::new();
            let mut scope_params: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(ref proj) = project {
                scope_clauses.push(format!("project = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(proj.clone()));
            }
            if let Some(ref at) = agent_type {
                scope_clauses.push(format!("agent_type = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(at.clone()));
            }
            if let Some(ref eid) = entity_id {
                scope_clauses.push(format!("entity_id = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(eid.clone()));
            }
            let scope_sql = if scope_clauses.is_empty() {
                String::new()
            } else {
                format!(" AND {}", scope_clauses.join(" AND "))
            };

            // Each tier: (sql_condition, content_cap_chars, order_by_clause)
            // Base filter: superseded_by_id IS NULL + scope filters, applied to all tiers.
            struct Tier {
                cond: &'static str,
                cap_chars: usize,
                order: &'static str,
            }
            let tiers = [
                // Tier 1: pinned
                Tier {
                    cond: "json_extract(metadata, '$.pinned') = 1",
                    cap_chars: 300,
                    order: "importance DESC, created_at DESC",
                },
                // Tier 2: user preferences and facts with high importance
                Tier {
                    cond: "event_type IN ('user_preference','user_fact') AND importance >= 0.5",
                    cap_chars: 300,
                    order: "importance DESC, created_at DESC",
                },
                // Tier 3: moderate-importance memories
                Tier {
                    cond: "importance >= 0.3",
                    cap_chars: 200,
                    order: "created_at DESC",
                },
                // Tier 4: low-importance / auto-captured
                Tier {
                    cond: "importance < 0.3",
                    cap_chars: 150,
                    order: "created_at DESC",
                },
            ];

            // Reserve ~200 tokens for greeting/profile/reminders overhead.
            const OVERHEAD_TOKENS: usize = 200;
            let mut remaining = budget.saturating_sub(OVERHEAD_TOKENS);

            let mut all_memories: Vec<serde_json::Value> = Vec::new();
            let mut seen_ids: HashSet<String> = HashSet::new();

            for tier in &tiers {
                if remaining == 0 {
                    break;
                }

                let sql = format!(
                    "SELECT id, content, event_type, importance, priority, created_at FROM memories \
                     WHERE superseded_by_id IS NULL{scope_sql} AND {} ORDER BY {} LIMIT 50",
                    tier.cond, tier.order
                );

                let mut stmt = conn.prepare(&sql).context("failed to prepare tier query")?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    scope_params.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, f64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .context("failed to query tier memories")?;

                for row_result in rows {
                    let (id, content, event_type, importance, priority, created_at) =
                        row_result.context("failed to decode tier row")?;

                    if seen_ids.contains(&id) {
                        continue;
                    }

                    let truncated: String = content.chars().take(tier.cap_chars).collect();
                    let tokens = estimate_tokens(&truncated);
                    if tokens > remaining {
                        break;
                    }

                    remaining = remaining.saturating_sub(tokens);
                    seen_ids.insert(id.clone());
                    all_memories.push(serde_json::json!({
                        "id": id,
                        "content": truncated,
                        "event_type": event_type,
                        "importance": importance,
                        "priority": priority,
                        "created_at": created_at,
                        "source": "tiered",
                    }));

                    if remaining == 0 {
                        break;
                    }
                }
            }

            Ok::<_, anyhow::Error>((total, all_memories))
        })
        .await
        .context("spawn_blocking join error")??;

        let (total, mut all_memories) = db_result;

        // ── Semantic search phase ──────────────────────────────────────
        // If a project is specified and we have remaining token budget,
        // use AdvancedSearcher to find project-relevant memories that
        // the tiered SQL queries may have missed.
        if opts.project.is_some() {
            let overhead_tokens: usize = 200;
            let used_tokens: usize = all_memories
                .iter()
                .map(|m| estimate_tokens(m.get("content").and_then(|v| v.as_str()).unwrap_or("")))
                .sum();
            let remaining = budget
                .saturating_sub(overhead_tokens)
                .saturating_sub(used_tokens);

            if remaining > 0 {
                let search_opts = SearchOptions {
                    project: opts.project.clone(),
                    agent_type: opts.agent_type.clone(),
                    entity_id: opts.entity_id.clone(),
                    ..SearchOptions::default()
                };

                let query = opts.project.as_deref().unwrap_or("project context");

                let mut seen_ids: HashSet<String> = all_memories
                    .iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect();

                let candidate_count = 10usize;
                match <SqliteStorage as AdvancedSearcher>::advanced_search(
                    self,
                    query,
                    candidate_count,
                    &search_opts,
                )
                .await
                {
                    Ok(semantic_results) => {
                        let mut sem_remaining = remaining;
                        for sr in semantic_results {
                            if seen_ids.contains(&sr.id) {
                                continue;
                            }
                            let truncated: String = sr.content.chars().take(200).collect();
                            let tokens = estimate_tokens(&truncated);
                            if tokens > sem_remaining {
                                break;
                            }
                            sem_remaining = sem_remaining.saturating_sub(tokens);
                            seen_ids.insert(sr.id.clone());

                            let et_str = sr.event_type.as_ref().map(|e| e.to_string());
                            all_memories.push(serde_json::json!({
                                "id": sr.id,
                                "content": truncated,
                                "event_type": et_str,
                                "importance": sr.importance,
                                "source": "semantic",
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            project = query,
                            query_len = query.len(),
                            candidate_count,
                            error_kind = %e.root_cause(),
                            "semantic search failed in welcome_scoped()"
                        );
                    }
                }
            }
        }

        // Profile and reminders (same as welcome())
        let profile = <Self as ProfileManager>::get_profile(self)
            .await
            .unwrap_or(serde_json::json!({}));
        let reminders = <Self as ReminderManager>::list_reminders(self, Some("pending"))
            .await
            .unwrap_or_default();

        let greeting = if total == 0 {
            "Welcome to MAG! Store your first memory to get started.".to_string()
        } else {
            format!("Welcome back! You have {total} memories stored.")
        };

        // Split into recent_memories and user_context to preserve JSON shape.
        // user_context = user_preference / user_fact entries; recent_memories = everything else.
        let mut recent_memories: Vec<serde_json::Value> = Vec::new();
        let mut user_context: Vec<serde_json::Value> = Vec::new();
        for m in all_memories {
            let et = m.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
            if et == "user_preference" || et == "user_fact" {
                user_context.push(m);
            } else {
                recent_memories.push(m);
            }
        }

        Ok(serde_json::json!({
            "greeting": greeting,
            "memory_count": total,
            "recent_memories": recent_memories,
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

            #[allow(clippy::cast_precision_loss)]
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

            #[allow(clippy::cast_precision_loss)]
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
