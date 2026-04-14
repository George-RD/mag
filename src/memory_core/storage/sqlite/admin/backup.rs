//! Backup management for the SQLite storage backend.
//!
//! Provides automatic and on-demand backups of the database file, including
//! timestamped snapshot creation, rotation to keep only the N most recent
//! backups, restore from a snapshot, and startup-time conditional backups.

use super::super::*;
use crate::memory_core::BackupInfo;

/// Maximum number of automatic backups to keep.
const MAX_BACKUPS: usize = 5;

/// Minimum interval between automatic backups (in seconds).
const BACKUP_INTERVAL_SECS: u64 = 24 * 60 * 60; // 24 hours

/// The backup file pattern: `memory.db.YYYYMMDD_HHMMSS.bak`
const BACKUP_PREFIX: &str = "memory.db.";
const BACKUP_SUFFIX: &str = ".bak";

/// Returns the backups directory for a given database path.
pub(super) fn backups_dir(db_path: &Path) -> Result<PathBuf> {
    let parent = db_path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent directory"))?;
    Ok(parent.join("backups"))
}

/// Collects backup entries from the backups directory, sorted oldest-first by filename.
pub(super) fn collect_backup_entries(backups_dir: &Path) -> Result<Vec<fs::DirEntry>> {
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
pub(super) fn create_backup_sync(conn: &Connection, db_path: &Path) -> Result<BackupInfo> {
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
pub(super) fn rotate_backups_sync(db_path: &Path, max_count: usize) -> Result<usize> {
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
pub(super) fn list_backups_sync(db_path: &Path) -> Result<Vec<BackupInfo>> {
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
pub(super) fn restore_backup_sync(
    conn: &Connection,
    backup_path: &Path,
    db_path: &Path,
) -> Result<()> {
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
pub(super) fn needs_backup(db_path: &Path) -> Result<bool> {
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
