use super::*;
use crate::app_paths;

/// Ensures the parent directory of `path` exists, creating it recursively if needed.
pub(super) fn initialize_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("database path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory {}", parent.display()))?;
    Ok(())
}

/// Returns the current schema version from the `schema_migrations` table.
/// Returns `0` if the table does not exist or is empty.
fn current_schema_version(conn: &Connection) -> Result<i64> {
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if !table_exists {
        return Ok(0);
    }

    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
    .map_err(|e| anyhow!("Failed to read schema version: {e}"))
}

/// Creates the `memories` and `relationships` tables if they don't exist and enables foreign keys.
pub(super) fn initialize_schema(conn: &Connection, embedding_dim: usize) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .context("failed to enable foreign key enforcement")?;

    // Enable WAL mode for better concurrent read performance
    let _ = conn.execute_batch("PRAGMA journal_mode=WAL;");
    // Truncate WAL on startup to reclaim disk space
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");

    // Performance PRAGMAs
    let _ = conn.execute_batch(
        "PRAGMA cache_size=-16000; PRAGMA mmap_size=33554432; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    );

    // --- Schema version tracking ---
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );",
    )
    .context("failed to create schema_migrations table")?;

    // Seed version records for existing databases that pre-date version tracking.
    // If the memories table already exists but schema_migrations is empty, this is
    // an existing DB — mark all historical migrations as applied.
    let memories_exist: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='memories'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;

    if memories_exist && current_schema_version(conn)? == 0 {
        for v in 1..=4 {
            let _ = conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (?1)",
                params![v],
            );
        }
    }

    let current = current_schema_version(conn)?;

    // --- v1: Base schema (CREATE TABLE IF NOT EXISTS is always idempotent) ---
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
            tokenize='porter unicode61'
        );",
    )
    .context("failed to initialize sqlite schema")?;

    if current < 1 {
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (1)",
            [],
        )?;
    }

    // --- v2: Memory + relationship column ALTERs ---
    if current < 2 {
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

        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (2)",
            [],
        )?;
    }

    // --- v3: Performance indexes ---
    if current < 3 {
        let indexes = [
            "CREATE INDEX IF NOT EXISTS idx_memories_last_accessed ON memories(last_accessed_at)",
            "CREATE INDEX IF NOT EXISTS idx_memories_ttl ON memories(ttl_seconds)",
            "CREATE INDEX IF NOT EXISTS idx_memories_event_access ON memories(event_type, access_count)",
            "CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at)",
            "CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project)",
            "CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id)",
            "CREATE INDEX IF NOT EXISTS idx_memories_project_last_accessed_active ON memories(project, last_accessed_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_session_last_accessed_active ON memories(session_id, last_accessed_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_event_last_accessed_active ON memories(event_type, last_accessed_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_project_created_active ON memories(project, created_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_session_created_active ON memories(session_id, created_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_event_created_active ON memories(event_type, created_at DESC) WHERE superseded_by_id IS NULL",
            "CREATE INDEX IF NOT EXISTS idx_memories_entity_id ON memories(entity_id)",
            "CREATE INDEX IF NOT EXISTS idx_memories_canonical ON memories(canonical_hash)",
            "CREATE INDEX IF NOT EXISTS idx_memories_version_chain ON memories(version_chain_id)",
            "CREATE INDEX IF NOT EXISTS idx_memories_superseded ON memories(superseded_by_id)",
            "CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id)",
            "CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id)",
            "CREATE INDEX IF NOT EXISTS idx_relationships_source_type ON relationships(source_id, rel_type)",
            "CREATE INDEX IF NOT EXISTS idx_relationships_target_type ON relationships(target_id, rel_type)",
        ];
        for idx in &indexes {
            let _ = conn.execute_batch(idx);
        }

        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (3)",
            [],
        )?;
    }

    // --- v4: FTS porter migration ---
    if current < 4 {
        migrate_fts_to_porter(conn)?;
        rebuild_fts_index(conn)?;

        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (4)",
            [],
        )?;
    }

    // --- v5: vec_memories initialization (feature-gated) ---
    #[cfg(feature = "sqlite-vec")]
    {
        if current < 5 {
            initialize_vec_table(conn, embedding_dim)?;
            conn.execute(
                "INSERT OR IGNORE INTO schema_migrations (version) VALUES (5)",
                [],
            )?;
        }
    }

    // --- v6: last_confirmed_at column for UserPreference dedup ---
    if current < 6 {
        let _ = conn.execute_batch("ALTER TABLE memories ADD COLUMN last_confirmed_at TEXT");
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (6)",
            [],
        )?;
    }

    #[cfg(not(feature = "sqlite-vec"))]
    let _ = embedding_dim;

    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn initialize_vec_table(conn: &Connection, embedding_dim: usize) -> Result<()> {
    // Check if vec_memories already exists with a different dimension.
    // vec0 shadow tables store column metadata; if dimension mismatches we must
    // recreate. We detect this by attempting a probe insert with a zero vector
    // of the expected dimension — a dimension mismatch produces an error.
    let table_exists: bool = conn
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='vec_memories'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .context("failed to check sqlite_master for vec_memories")?
        > 0;

    if table_exists {
        let probe = vec![0.0_f32; embedding_dim];
        let probe_blob = encode_embedding(&probe);
        match conn.execute(
            "INSERT INTO vec_memories(memory_id, embedding) VALUES ('__dim_probe__', ?1)",
            params![probe_blob],
        ) {
            Ok(_) => {
                let _ = conn.execute(
                    "DELETE FROM vec_memories WHERE memory_id = '__dim_probe__'",
                    [],
                );
            }
            Err(e) => {
                let msg = e.to_string().to_lowercase();
                if msg.contains("dimension") || msg.contains("size") {
                    tracing::warn!(
                        "vec_memories dimension mismatch detected — recreating with {embedding_dim} dims"
                    );
                    conn.execute_batch("DROP TABLE vec_memories;")
                        .context("failed to drop stale vec_memories")?;
                } else {
                    return Err(e)
                        .context("vec_memories probe insert failed (not a dimension mismatch)");
                }
            }
        }
    }

    conn.execute_batch(&format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(
            memory_id text primary key,
            embedding float[{embedding_dim}] distance_metric=cosine
        );"
    ))
    .context("failed to create vec_memories virtual table")?;

    // Idempotent migration: populate from existing embeddings.
    // Uses streaming decode/re-encode to handle legacy JSON embeddings.
    let vec_count: i64 = conn
        .query_row("SELECT count(*) FROM vec_memories", [], |row| row.get(0))
        .context("failed to count vec_memories rows")?;
    if vec_count == 0 {
        let tx = retry_on_lock(|| conn.unchecked_transaction())
            .context("failed to begin vec migration transaction")?;

        let mut read_stmt = tx
            .prepare("SELECT id, embedding FROM memories WHERE embedding IS NOT NULL")
            .context("failed to prepare migration read query")?;
        let mut insert_stmt = tx
            .prepare("INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)")
            .context("failed to prepare migration insert")?;

        let rows = read_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .context("failed to read embeddings for migration")?;

        let mut migrated = 0_u64;
        let mut skipped = 0_u64;
        for row in rows {
            let (id, blob) = row.context("failed to decode migration row")?;
            match decode_embedding(&blob) {
                Ok(vec) if vec.len() == embedding_dim => {
                    let encoded = encode_embedding(&vec);
                    if let Err(e) = insert_stmt.execute(params![id, encoded]) {
                        tracing::warn!("skipping vec migration for {id}: {e}");
                        skipped += 1;
                    } else {
                        migrated += 1;
                    }
                }
                Ok(vec) => {
                    tracing::warn!(
                        "skipping vec migration for {id}: dimension {} != expected {embedding_dim}",
                        vec.len()
                    );
                    skipped += 1;
                }
                Err(e) => {
                    tracing::warn!("skipping vec migration for {id}: {e}");
                    skipped += 1;
                }
            }
        }

        // Drop statements before committing to release borrows on tx
        drop(insert_stmt);
        drop(read_stmt);
        tx.commit()
            .context("failed to commit vec migration transaction")?;

        if migrated > 0 || skipped > 0 {
            tracing::info!("vec_memories migration: {migrated} migrated, {skipped} skipped");
        }
    }

    Ok(())
}

/// Migrates an existing FTS5 table from `unicode61` to `porter unicode61` tokenizer.
///
/// For existing databases the `CREATE VIRTUAL TABLE IF NOT EXISTS` in `initialize_schema`
/// is a no-op because the table already exists with the old tokenizer.  This migration
/// detects that case by inspecting `sqlite_master`, drops the stale virtual table, and
/// recreates it with the porter stemmer.  Data is repopulated from `memories`.
///
/// The check is idempotent: once the table uses `porter unicode61` the function is a no-op.
fn migrate_fts_to_porter(conn: &Connection) -> Result<()> {
    // Read the DDL that SQLite recorded for the FTS virtual table.
    let sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
            [],
            |row| row.get(0),
        )
        .ok();

    let needs_migration = match &sql {
        // Table exists but doesn't contain 'porter' → old tokenizer
        Some(ddl) => !ddl.to_lowercase().contains("porter"),
        // Table doesn't exist yet (fresh DB) → CREATE in initialize_schema already uses porter
        None => false,
    };

    if !needs_migration {
        return Ok(());
    }

    tracing::info!("migrating FTS5 index from unicode61 to porter unicode61 tokenizer");

    conn.execute_batch("DROP TABLE IF EXISTS memories_fts;")
        .context("failed to drop old FTS5 table during porter migration")?;

    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
            id UNINDEXED,
            content,
            tokenize='porter unicode61'
        );",
    )
    .context("failed to recreate FTS5 table with porter tokenizer")?;

    // Repopulate from existing memories (rebuild_fts_index will also check, but we do it
    // here so the migration is self-contained).
    conn.execute_batch("INSERT INTO memories_fts(id, content) SELECT id, content FROM memories;")
        .context("failed to repopulate FTS5 index during porter migration")?;

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

/// Resolves the default database path: `$HOME/.mag/memory.db`.
pub(super) fn default_db_path() -> Result<PathBuf> {
    app_paths::resolve_app_paths().map(|paths| paths.database_path)
}
