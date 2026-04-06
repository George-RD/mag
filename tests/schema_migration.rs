//! Schema migration upgrade tests.
//!
//! These tests verify that MAG's SQLite storage layer handles databases from
//! previous releases gracefully.  Each test loads a fixture that captures the
//! schema state of a specific release, then opens it with the current binary's
//! `initialize_schema` path, and asserts:
//!
//! 1. Migrations apply without error.
//! 2. Pre-existing memories survive the migration and are still retrievable.
//! 3. FTS search over migrated data returns expected results.
//! 4. New columns added by pending migrations have their correct defaults.
//! 5. The `schema_migrations` table reflects the latest version after upgrade.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use mag::memory_core::storage::SqliteStorage;
use mag::memory_core::{PlaceholderEmbedder, Retriever, SearchOptions, Searcher};
use rusqlite::Connection;

// ── helpers ────────────────────────────────────────────────────────────────

/// RAII guard that removes its directory on drop, even if the test panics.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Build a fresh on-disk SQLite database from a SQL fixture file, returning the
/// path and a cleanup guard.  The guard removes the directory when dropped, so
/// callers just need to hold `_guard` in scope for the duration of the test.
///
/// The connection is opened directly with `rusqlite` (bypassing `initialize_schema`)
/// so the fixture represents the exact state a user's database would have been in
/// before upgrading.
fn build_db_from_fixture(sql: &str) -> Result<(PathBuf, TempDir)> {
    let dir = std::env::temp_dir().join(format!("mag-migration-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let db_path = dir.join("memory.db");

    let conn = Connection::open(&db_path)?;
    conn.execute_batch(sql)?;
    // Explicit checkpoint so the WAL is flushed before we hand the file to the pool.
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    drop(conn);

    let guard = TempDir(dir);
    Ok((db_path, guard))
}

/// Read the max applied version from `schema_migrations` via a raw connection.
fn read_schema_version(db_path: &std::path::Path) -> Result<i64> {
    let conn = Connection::open(db_path)?;
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    Ok(version)
}

/// Check whether a column exists on a table using PRAGMA table_info.
fn column_exists(db_path: &std::path::Path, table: &str, column: &str) -> Result<bool> {
    let conn = Connection::open(db_path)?;
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let found = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .any(|r| r.is_ok_and(|name| name == column));
    Ok(found)
}

// ── v0.1.4 → current ───────────────────────────────────────────────────────

/// Builds a v0.1.4 snapshot database and opens it with the current storage
/// layer.  The v0.1.4 snapshot has schema versions 1-5 (no `last_confirmed_at`
/// column).  Opening it must trigger the v6 migration that adds the column.
#[tokio::test]
async fn test_migration_from_v0_1_4() -> Result<()> {
    let sql = include_str!("fixtures/v0_1_4_schema.sql");
    let (db_path, _guard) = build_db_from_fixture(sql)?;

    // Pre-condition: fixture is at version 5, column absent.
    let pre_version = read_schema_version(&db_path)?;
    assert_eq!(
        pre_version, 5,
        "fixture should start at schema version 5 (v0.1.4)"
    );
    assert!(
        !column_exists(&db_path, "memories", "last_confirmed_at")?,
        "last_confirmed_at should be absent before migration"
    );

    // Open with current storage code — this triggers initialize_schema, which
    // applies the v6 migration (ADD COLUMN last_confirmed_at TEXT).
    let storage = SqliteStorage::new_with_path(db_path.clone(), Arc::new(PlaceholderEmbedder))?;

    // Post-condition: schema version advanced to 6.
    let post_version = read_schema_version(&db_path)?;
    assert_eq!(
        post_version, 6,
        "schema version must be 6 after migration from v0.1.4"
    );

    // Post-condition: new column now exists with NULL default for old rows.
    assert!(
        column_exists(&db_path, "memories", "last_confirmed_at")?,
        "last_confirmed_at column must exist after v6 migration"
    );

    // Existing memories survive: retrieve by ID.
    let expected_memories = [
        (
            "v014-mem-001",
            "George prefers Rust for systems programming",
        ),
        (
            "v014-mem-002",
            "MAG uses additive SQLite migrations for schema evolution",
        ),
        (
            "v014-mem-003",
            "The embedding model is bge-small-en-v1.5 with 384 dimensions",
        ),
    ];
    for (id, expected) in expected_memories {
        let content = storage.retrieve(id).await?;
        assert_eq!(
            content, expected,
            "memory {id} must be retrievable after migration"
        );
    }

    // FTS search over migrated data works.
    let results = storage
        .search("rust systems programming", 10, &SearchOptions::default())
        .await?;
    assert!(
        results.iter().any(|r| r.id == "v014-mem-001"),
        "FTS search must find v014-mem-001 after migration"
    );

    // Verify old rows have NULL for the new column (correct nullable default).
    {
        let conn = Connection::open(&db_path)?;
        let val: Option<String> = conn.query_row(
            "SELECT last_confirmed_at FROM memories WHERE id = 'v014-mem-001'",
            [],
            |row| row.get(0),
        )?;
        assert!(
            val.is_none(),
            "last_confirmed_at must be NULL for rows that pre-date the v6 migration"
        );
    }

    Ok(())
}

// ── v0.1.5 idempotency ─────────────────────────────────────────────────────

/// Verifies that opening a fully-migrated v0.1.5 database (schema versions 1-6)
/// is a no-op: no migrations fire, all data is intact, and the schema version
/// stays at 6.
#[tokio::test]
async fn test_migration_idempotent_v0_1_5() -> Result<()> {
    // Build a v0.1.5 snapshot by applying the v0.1.4 fixture and then running
    // the v6 migration manually — this is equivalent to what a v0.1.5 binary
    // would have produced.
    let sql = include_str!("fixtures/v0_1_4_schema.sql");
    let (db_path, _guard) = build_db_from_fixture(sql)?;

    // Manually apply the v6 migration (as v0.1.5 binary would have).
    {
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("ALTER TABLE memories ADD COLUMN last_confirmed_at TEXT;")?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations (version) VALUES (6)",
            [],
        )?;
        let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    }

    let pre_version = read_schema_version(&db_path)?;
    assert_eq!(pre_version, 6, "fixture must start at schema version 6");

    // Open with current code — must not re-apply any migration.
    let storage = SqliteStorage::new_with_path(db_path.clone(), Arc::new(PlaceholderEmbedder))?;

    let post_version = read_schema_version(&db_path)?;
    assert_eq!(
        post_version, 6,
        "schema version must remain 6 for an already-migrated database"
    );

    // Pre-existing data is still accessible.
    let content = storage.retrieve("v014-mem-001").await?;
    assert_eq!(
        content, "George prefers Rust for systems programming",
        "memory must survive idempotent re-open"
    );

    // New column still exists.
    assert!(
        column_exists(&db_path, "memories", "last_confirmed_at")?,
        "last_confirmed_at must still exist after idempotent re-open"
    );

    // FTS search still works.
    let results = storage
        .search("SQLite migrations", 10, &SearchOptions::default())
        .await?;
    assert!(
        results.iter().any(|r| r.id == "v014-mem-002"),
        "FTS search must find v014-mem-002 in the idempotent-open scenario"
    );

    Ok(())
}

// ── legacy database without schema_migrations table ────────────────────────

/// Verifies that a very old database that pre-dates the schema_migrations table
/// is handled by the seed-versions logic: all historical migrations are
/// recorded as applied, and the database is brought to the current version.
#[tokio::test]
async fn test_migration_from_pre_versioning_database() -> Result<()> {
    // Construct a pre-versioning database by hand: the memories table exists with
    // ALL columns that were present before the schema_migrations table was introduced
    // (refactor(schema) #134), but there is NO schema_migrations table.
    //
    // A real pre-versioning DB would already have had all ALTER TABLE migrations
    // applied (session_id, event_type, etc.) — the version-tracking PR (#134) only
    // added the schema_migrations table.  The seed-versions logic in initialize_schema
    // therefore marks v1–4 as applied WITHOUT re-running the ALTER TABLEs (which
    // would fail since the columns already exist).  The search code expects all v2
    // columns (session_id, event_type, project, entity_id, agent_type) to be present.
    let sql = "
        PRAGMA journal_mode=WAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE memories (
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
            access_count INTEGER NOT NULL DEFAULT 0,
            session_id TEXT,
            event_type TEXT,
            project TEXT,
            priority INTEGER,
            entity_id TEXT,
            agent_type TEXT,
            ttl_seconds INTEGER,
            canonical_hash TEXT,
            version_chain_id TEXT,
            superseded_by_id TEXT,
            superseded_at TEXT
        );

        CREATE TABLE relationships (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            rel_type TEXT NOT NULL,
            weight REAL NOT NULL DEFAULT 1.0,
            metadata TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            FOREIGN KEY(source_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY(target_id) REFERENCES memories(id) ON DELETE CASCADE
        );

        CREATE TABLE user_profile (
            key TEXT NOT NULL PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );

        CREATE VIRTUAL TABLE memories_fts USING fts5(
            id UNINDEXED,
            content,
            tokenize='porter unicode61'
        );

        INSERT INTO memories (id, content, content_hash, source_type, created_at, event_at, last_accessed_at)
        VALUES ('legacy-001', 'old memory from before versioning', 'h1', 'user',
                '2024-01-01T00:00:00.000Z', '2024-01-01T00:00:00.000Z', '2024-01-01T00:00:00.000Z');

        INSERT INTO memories_fts (id, content) VALUES ('legacy-001', 'old memory from before versioning');
    ";

    let (db_path, _guard) = build_db_from_fixture(sql)?;

    // Pre-condition: no schema_migrations table exists.
    {
        let conn = Connection::open(&db_path)?;
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(exists, 0, "schema_migrations must not exist before open");
    }

    // Opening with current code must bootstrap the migration table and bring
    // the database to version 6.
    let storage = SqliteStorage::new_with_path(db_path.clone(), Arc::new(PlaceholderEmbedder))?;

    let post_version = read_schema_version(&db_path)?;
    assert_eq!(
        post_version, 6,
        "pre-versioning database must be brought to schema version 6"
    );

    // Legacy data must still be accessible.
    let content = storage.retrieve("legacy-001").await?;
    assert_eq!(
        content, "old memory from before versioning",
        "legacy memory must survive full migration from pre-versioning state"
    );

    // FTS search on migrated legacy data works.
    let results = storage
        .search("old memory", 10, &SearchOptions::default())
        .await?;
    assert!(
        results.iter().any(|r| r.id == "legacy-001"),
        "FTS search must find legacy-001 after full migration"
    );

    // v2 columns must be present (they were in the fixture already; seed-versions
    // marks them applied without re-running the ALTERs).
    assert!(
        column_exists(&db_path, "memories", "session_id")?,
        "session_id column must exist in the pre-versioning database"
    );
    assert!(
        column_exists(&db_path, "memories", "event_type")?,
        "event_type column must exist in the pre-versioning database"
    );
    // v6 column must have been added by the migration.
    assert!(
        column_exists(&db_path, "memories", "last_confirmed_at")?,
        "last_confirmed_at column must exist after v6 migration"
    );

    Ok(())
}
