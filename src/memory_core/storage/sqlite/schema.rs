use super::*;

/// Ensures the parent directory of `path` exists, creating it recursively if needed.
pub(super) fn initialize_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory {}", parent.display()))?;
    Ok(())
}

/// Creates the `memories` and `relationships` tables if they don't exist and enables foreign keys.
pub(super) fn initialize_schema(conn: &Connection) -> Result<()> {
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
        "ALTER TABLE memories ADD COLUMN version_chain_id TEXT",
        "ALTER TABLE memories ADD COLUMN superseded_by_id TEXT",
        "ALTER TABLE memories ADD COLUMN superseded_at TEXT",
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
        "CREATE INDEX IF NOT EXISTS idx_memories_version_chain ON memories(version_chain_id)",
        "CREATE INDEX IF NOT EXISTS idx_memories_superseded ON memories(superseded_by_id)",
        "CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id)",
        "CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id)",
    ];
    for idx in &indexes {
        let _ = conn.execute_batch(idx);
    }

    Ok(())
}

pub(super) fn rebuild_fts_index(conn: &Connection) -> Result<()> {
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
pub(super) fn default_db_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            anyhow!("neither HOME nor USERPROFILE is set — cannot resolve default database path")
        })?;
    Ok(PathBuf::from(home).join(".romega-memory").join("memory.db"))
}
