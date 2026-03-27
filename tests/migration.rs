//! Migration tests: verify that `SqliteStorage` correctly upgrades old schema
//! versions to the current schema, preserving existing data.
//!
//! # Migration architecture
//!
//! `initialize_schema` in `schema.rs` handles several scenarios:
//!
//! - **Fresh DB**: No tables exist. Creates everything from scratch with
//!   current schema (v1 base + all migrations run sequentially).
//!
//! - **Pre-tracking DB**: `memories` table exists but `schema_migrations`
//!   does not. This was the state before version tracking was introduced
//!   alongside v4. The backfill seeds versions 1..=4 (assuming all
//!   migrations were already applied) then continues from v5+.
//!
//! - **Versioned DB at v<N>**: `schema_migrations` exists with max version
//!   N. Migrations v(N+1).. run in order.
//!
//! - **Idempotent re-open**: Opening an already-current DB is a no-op.

use std::path::PathBuf;
use std::sync::Arc;

use mag::memory_core::{
    PlaceholderEmbedder, Retriever, SearchOptions, Searcher, storage::sqlite::SqliteStorage,
};
use rusqlite::{Connection, params};

/// Create a "pre-tracking" schema: all v1-v3 columns applied (the state of
/// databases before `schema_migrations` was introduced alongside v4), but
/// using the old `unicode61` FTS tokenizer (pre-v4) and no `schema_migrations`
/// table.
///
/// This is the realistic scenario for the backfill path: a database that had
/// all ALTERs applied by earlier code, but doesn't have version tracking yet.
fn create_pre_tracking_schema(conn: &Connection) {
    conn.execute_batch(
        "CREATE TABLE memories (
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
            -- v2 columns (already applied before tracking was introduced)
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

        -- Pre-v4: using unicode61 without porter stemmer
        CREATE VIRTUAL TABLE memories_fts USING fts5(
            id UNINDEXED,
            content,
            tokenize='unicode61'
        );",
    )
    .expect("failed to create pre-tracking schema");
}

/// Create a schema at a specific version by having `schema_migrations` set
/// to the given version, and skipping the columns/FTS that come after it.
fn create_schema_at_version(conn: &Connection, version: i64) {
    // v1 base table
    conn.execute_batch(
        "CREATE TABLE memories (
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

        CREATE TABLE relationships (
            id TEXT PRIMARY KEY,
            source_id TEXT NOT NULL,
            target_id TEXT NOT NULL,
            rel_type TEXT NOT NULL,
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
            tokenize='unicode61'
        );

        CREATE TABLE schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );",
    )
    .expect("failed to create base schema");

    // Record version(s) as applied
    for v in 1..=version {
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            params![v],
        )
        .expect("failed to seed version");
    }

    // If version >= 2, apply v2 ALTER TABLE columns
    if version >= 2 {
        let alters = [
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
            "ALTER TABLE relationships ADD COLUMN weight REAL NOT NULL DEFAULT 1.0",
            "ALTER TABLE relationships ADD COLUMN metadata TEXT NOT NULL DEFAULT '{}'",
            "ALTER TABLE relationships ADD COLUMN created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        ];
        for alter in &alters {
            let _ = conn.execute_batch(alter);
        }
    }

    // v3 is indexes only (CREATE INDEX IF NOT EXISTS), handled idempotently
    // v4 is the porter migration — left un-applied so the migration can run
}

/// Insert a row with all required columns into a pre-tracking or versioned DB.
fn insert_row(conn: &Connection, id: &str, content: &str) {
    conn.execute(
        "INSERT INTO memories (id, content, content_hash, source_type)
         VALUES (?1, ?2, ?3, 'conversation')",
        params![id, content, format!("hash_{id}")],
    )
    .expect("failed to insert row");

    conn.execute(
        "INSERT INTO memories_fts (id, content) VALUES (?1, ?2)",
        params![id, content],
    )
    .expect("failed to insert FTS row");
}

/// Return a temp DB path.
fn temp_db_path(label: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!(
        "mag-migration-test-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&base).expect("failed to create temp dir");
    base.join("memory.db")
}

/// Open a raw `rusqlite::Connection` at the given path.
fn raw_conn(path: &PathBuf) -> Connection {
    Connection::open(path).expect("failed to open raw connection")
}

// ---------------------------------------------------------------------------
// Test 1: v1 → current migration (schema_migrations at v1, apply v2-v4)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn v1_to_current_migration() {
    let db_path = temp_db_path("v1-to-current");

    // Phase 1: Create a DB at schema version 1 (only base columns, no v2 ALTERs).
    {
        let conn = raw_conn(&db_path);
        create_schema_at_version(&conn, 1);
    }

    // Phase 2: Open via SqliteStorage — v2, v3, v4 migrations should run.
    let embedder = Arc::new(PlaceholderEmbedder);
    let _storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed on v1 DB");

    // Phase 3: Verify schema via a fresh raw connection.
    let conn = raw_conn(&db_path);

    // 3a. All v2 ALTER TABLE columns should exist.
    let columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    let expected_v2_columns = [
        "session_id",
        "event_type",
        "project",
        "priority",
        "entity_id",
        "agent_type",
        "ttl_seconds",
        "canonical_hash",
        "version_chain_id",
        "superseded_by_id",
        "superseded_at",
    ];
    for col in &expected_v2_columns {
        assert!(
            columns.contains(&col.to_string()),
            "missing v2 column: {col}"
        );
    }

    // 3b. Relationship v2 columns should exist.
    let rel_columns: Vec<String> = {
        let mut stmt = conn
            .prepare("PRAGMA table_info(relationships)")
            .unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    for col in ["weight", "metadata", "created_at"] {
        assert!(
            rel_columns.contains(&col.to_string()),
            "missing relationship v2 column: {col}"
        );
    }

    // 3c. FTS should use porter tokenizer (v4 migration).
    let fts_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
            [],
            |row| row.get(0),
        )
        .expect("memories_fts not found in sqlite_master");
    assert!(
        fts_sql.to_lowercase().contains("porter"),
        "FTS table should use porter tokenizer, got: {fts_sql}"
    );

    // 3d. schema_migrations should show current version >= 4.
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .expect("failed to read schema_migrations");
    assert!(
        version >= 4,
        "expected schema version >= 4, got {version}"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 2: v1 data is readable after migration through v2-v4
// ---------------------------------------------------------------------------
#[tokio::test]
async fn v1_read_after_migration() {
    let db_path = temp_db_path("v1-read");
    let test_id = "test-mem-001";
    let test_content = "The quick brown fox jumps over the lazy dog";

    // Phase 1: Create v1 DB with a row.
    {
        let conn = raw_conn(&db_path);
        create_schema_at_version(&conn, 1);
        insert_row(&conn, test_id, test_content);
    }

    // Phase 2: Open via SqliteStorage (runs v2-v4 migrations).
    let embedder = Arc::new(PlaceholderEmbedder);
    let storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed on v1 DB with data");

    // Phase 3: Retrieve by ID.
    let retrieved = storage
        .retrieve(test_id)
        .await
        .expect("retrieve failed after migration");
    assert_eq!(
        retrieved, test_content,
        "content should survive migration unchanged"
    );

    // Phase 4: Search via FTS (porter-migrated by v4).
    let results = storage
        .search("fox jumps", 10, &SearchOptions::default())
        .await
        .expect("search failed after migration");
    assert!(
        !results.is_empty(),
        "FTS search should return results after migration"
    );
    assert_eq!(
        results[0].id, test_id,
        "search result should match the inserted memory"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 3: FTS porter migration (v3 → v4, unicode61 → porter unicode61)
// ---------------------------------------------------------------------------
#[tokio::test]
async fn fts_porter_migration() {
    let db_path = temp_db_path("fts-porter");

    // Phase 1: Create DB at v3 (has all columns/indexes but old FTS tokenizer).
    {
        let conn = raw_conn(&db_path);
        create_schema_at_version(&conn, 3);
        insert_row(&conn, "run-mem", "The runners were running a marathon");
        insert_row(&conn, "cook-mem", "She was cooking delicious cookies");
    }

    // Phase 2: Open via SqliteStorage — v4 porter migration should fire.
    let embedder = Arc::new(PlaceholderEmbedder);
    let storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed");

    // Phase 3: Verify FTS now uses porter by checking the DDL.
    {
        let conn = raw_conn(&db_path);
        let fts_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
                [],
                |row| row.get(0),
            )
            .expect("memories_fts not in sqlite_master");
        assert!(
            fts_sql.to_lowercase().contains("porter"),
            "FTS should use porter tokenizer after migration, got: {fts_sql}"
        );
    }

    // Phase 4: Verify porter stemming works -- searching for "run" should
    // match "runners" and "running" thanks to the porter stemmer.
    let results = storage
        .search("run", 10, &SearchOptions::default())
        .await
        .expect("FTS search failed after porter migration");
    assert!(
        results.iter().any(|r| r.id == "run-mem"),
        "porter stemmer should match 'run' against 'runners/running', got: {results:?}"
    );

    // Phase 5: Searching for "cook" should match "cooking"/"cookies".
    let results = storage
        .search("cook", 10, &SearchOptions::default())
        .await
        .expect("FTS search for cook failed");
    assert!(
        results.iter().any(|r| r.id == "cook-mem"),
        "porter stemmer should match 'cook' against 'cooking/cookies', got: {results:?}"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 4: schema_migrations backfill for pre-tracking databases
// ---------------------------------------------------------------------------
#[tokio::test]
async fn schema_version_backfill() {
    let db_path = temp_db_path("version-backfill");

    // Phase 1: Create a pre-tracking DB (memories table with all columns
    // applied, but NO schema_migrations table). This was the real state
    // before version tracking was introduced.
    {
        let conn = raw_conn(&db_path);
        create_pre_tracking_schema(&conn);

        // Confirm schema_migrations does NOT exist.
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 0,
            "schema_migrations should not exist in pre-tracking DB"
        );
    }

    // Phase 2: Open via SqliteStorage — should create and backfill schema_migrations.
    let embedder = Arc::new(PlaceholderEmbedder);
    let _storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed on pre-tracking DB");

    // Phase 3: Verify schema_migrations exists and is populated.
    let conn = raw_conn(&db_path);

    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_migrations'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        > 0;
    assert!(
        table_exists,
        "schema_migrations table should exist after opening with current code"
    );

    // The backfill should have inserted versions 1..=4 for the pre-existing
    // memories table.
    let versions: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    for v in 1..=4 {
        assert!(
            versions.contains(&v),
            "schema_migrations should contain version {v} from backfill, got: {versions:?}"
        );
    }

    // Every version record should have a non-null applied_at timestamp.
    let null_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE applied_at IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        null_count, 0,
        "all schema_migrations rows should have applied_at timestamps"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 5: Pre-tracking DB data survives backfill + porter migration
// ---------------------------------------------------------------------------
#[tokio::test]
async fn pre_tracking_data_survives_migration() {
    let db_path = temp_db_path("pre-tracking-data");

    // Phase 1: Create pre-tracking DB with data (old FTS tokenizer).
    {
        let conn = raw_conn(&db_path);
        create_pre_tracking_schema(&conn);
        insert_row(
            &conn,
            "legacy-1",
            "Important meeting notes from the architecture review",
        );
        insert_row(
            &conn,
            "legacy-2",
            "User preference: dark theme, vim keybindings",
        );
    }

    // Phase 2: Open via SqliteStorage — backfill + porter migration.
    let embedder = Arc::new(PlaceholderEmbedder);
    let storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed");

    // Phase 3: Both memories should be retrievable.
    let content1 = storage
        .retrieve("legacy-1")
        .await
        .expect("retrieve legacy-1 failed");
    assert_eq!(
        content1, "Important meeting notes from the architecture review"
    );

    let content2 = storage
        .retrieve("legacy-2")
        .await
        .expect("retrieve legacy-2 failed");
    assert_eq!(content2, "User preference: dark theme, vim keybindings");

    // Phase 4: FTS search should work with porter stemming.
    let results = storage
        .search("meeting", 10, &SearchOptions::default())
        .await
        .expect("search failed");
    assert!(
        results.iter().any(|r| r.id == "legacy-1"),
        "search for 'meeting' should find legacy-1, got: {results:?}"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 6: Fresh DB (no pre-existing tables) gets correct schema
// ---------------------------------------------------------------------------
#[tokio::test]
async fn fresh_db_gets_current_schema() {
    let db_path = temp_db_path("fresh-db");

    // Open a completely empty database via SqliteStorage.
    let embedder = Arc::new(PlaceholderEmbedder);
    let _storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("SqliteStorage::new_with_path failed on fresh DB");

    let conn = raw_conn(&db_path);

    // Verify all expected tables exist.
    let tables: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };

    for expected in [
        "memories",
        "relationships",
        "schema_migrations",
        "user_profile",
    ] {
        assert!(
            tables.contains(&expected.to_string()),
            "fresh DB should have table '{expected}', got: {tables:?}"
        );
    }

    // FTS should already use porter.
    let fts_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'memories_fts'",
            [],
            |row| row.get(0),
        )
        .expect("memories_fts should exist in fresh DB");
    assert!(
        fts_sql.to_lowercase().contains("porter"),
        "fresh DB FTS should use porter tokenizer, got: {fts_sql}"
    );

    // All versions through 4 should be recorded.
    let max_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(
        max_version >= 4,
        "fresh DB should have at least version 4 in schema_migrations, got: {max_version}"
    );

    // All v2 columns should be present.
    let columns: Vec<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(memories)").unwrap();
        let rows = stmt.query_map([], |row| row.get::<_, String>(1)).unwrap();
        rows.map(|r| r.unwrap()).collect()
    };
    for col in [
        "session_id",
        "event_type",
        "project",
        "superseded_by_id",
    ] {
        assert!(
            columns.contains(&col.to_string()),
            "fresh DB missing column: {col}"
        );
    }

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

// ---------------------------------------------------------------------------
// Test 7: Idempotent re-open — running migrations twice is safe
// ---------------------------------------------------------------------------
#[tokio::test]
async fn idempotent_reopen() {
    let db_path = temp_db_path("idempotent");
    let embedder = Arc::new(PlaceholderEmbedder);

    // Open 1: create a v1 DB with data, then open via SqliteStorage (runs v2-v4).
    {
        let conn = raw_conn(&db_path);
        create_schema_at_version(&conn, 1);
        insert_row(&conn, "idem-1", "first memory for idempotent test");
    }
    let _s1 = SqliteStorage::new_with_path(db_path.clone(), embedder.clone())
        .expect("first open failed");
    drop(_s1);

    // Open 2: re-open the same (now fully-migrated) DB — should not fail.
    let storage = SqliteStorage::new_with_path(db_path.clone(), embedder)
        .expect("second open (idempotent) failed");

    let content = storage
        .retrieve("idem-1")
        .await
        .expect("retrieve after re-open failed");
    assert_eq!(content, "first memory for idempotent test");

    // schema_migrations should not have duplicate version rows.
    let conn = raw_conn(&db_path);
    let version_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let distinct_count: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT version) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        version_count, distinct_count,
        "schema_migrations should not have duplicate versions"
    );

    // Cleanup.
    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}
