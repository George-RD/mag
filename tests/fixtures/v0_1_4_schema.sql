-- MAG v0.1.4 database snapshot (schema versions 1-5, no last_confirmed_at)
--
-- Used by the schema migration upgrade test to verify that a database from
-- a previous release is handled gracefully by the current binary.  The current
-- code must apply the v6 migration (ADD COLUMN last_confirmed_at) and leave
-- all pre-existing data intact.

PRAGMA journal_mode=WAL;
PRAGMA foreign_keys = ON;

-- Schema version tracking table (versions 1-5 recorded)
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO schema_migrations (version, applied_at) VALUES (1, '2025-10-01T00:00:00.000Z');
INSERT INTO schema_migrations (version, applied_at) VALUES (2, '2025-10-01T00:00:00.000Z');
INSERT INTO schema_migrations (version, applied_at) VALUES (3, '2025-10-01T00:00:00.000Z');
INSERT INTO schema_migrations (version, applied_at) VALUES (4, '2025-10-01T00:00:00.000Z');
INSERT INTO schema_migrations (version, applied_at) VALUES (5, '2025-10-01T00:00:00.000Z');

-- Base memories table (v1 shape + v2 columns, WITHOUT v6 last_confirmed_at)
CREATE TABLE IF NOT EXISTS memories (
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
    -- v2 additions
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
    -- NOTE: last_confirmed_at (v6) intentionally absent — this is the pre-migration state
);

-- Relationships table with v2 extra columns
CREATE TABLE IF NOT EXISTS relationships (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    rel_type TEXT NOT NULL,
    -- v2 additions
    weight REAL NOT NULL DEFAULT 1.0,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    FOREIGN KEY(source_id) REFERENCES memories(id) ON DELETE CASCADE,
    FOREIGN KEY(target_id) REFERENCES memories(id) ON DELETE CASCADE
);

-- User profile table
CREATE TABLE IF NOT EXISTS user_profile (
    key TEXT NOT NULL PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- FTS5 virtual table (v4: porter tokenizer already applied)
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    id UNINDEXED,
    content,
    tokenize='porter unicode61'
);

-- v3 performance indexes
CREATE INDEX IF NOT EXISTS idx_memories_last_accessed ON memories(last_accessed_at);
CREATE INDEX IF NOT EXISTS idx_memories_ttl ON memories(ttl_seconds);
CREATE INDEX IF NOT EXISTS idx_memories_event_access ON memories(event_type, access_count);
CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at);
CREATE INDEX IF NOT EXISTS idx_memories_project ON memories(project);
CREATE INDEX IF NOT EXISTS idx_memories_session ON memories(session_id);
CREATE INDEX IF NOT EXISTS idx_memories_project_last_accessed_active ON memories(project, last_accessed_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_session_last_accessed_active ON memories(session_id, last_accessed_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_event_last_accessed_active ON memories(event_type, last_accessed_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_project_created_active ON memories(project, created_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_session_created_active ON memories(session_id, created_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_event_created_active ON memories(event_type, created_at DESC) WHERE superseded_by_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_memories_entity_id ON memories(entity_id);
CREATE INDEX IF NOT EXISTS idx_memories_canonical ON memories(canonical_hash);
CREATE INDEX IF NOT EXISTS idx_memories_version_chain ON memories(version_chain_id);
CREATE INDEX IF NOT EXISTS idx_memories_superseded ON memories(superseded_by_id);
CREATE INDEX IF NOT EXISTS idx_relationships_source ON relationships(source_id);
CREATE INDEX IF NOT EXISTS idx_relationships_target ON relationships(target_id);
CREATE INDEX IF NOT EXISTS idx_relationships_source_type ON relationships(source_id, rel_type);
CREATE INDEX IF NOT EXISTS idx_relationships_target_type ON relationships(target_id, rel_type);

-- Sample data: a few memories that must survive the migration
INSERT INTO memories (
    id, content, content_hash, source_type, tags, importance, metadata,
    created_at, event_at, last_accessed_at, access_count,
    session_id, event_type, project
) VALUES (
    'v014-mem-001',
    'George prefers Rust for systems programming',
    'hash-001',
    'user',
    '["preference","rust"]',
    0.9,
    '{"source":"test"}',
    '2025-10-01T10:00:00.000Z',
    '2025-10-01T10:00:00.000Z',
    '2025-10-01T10:00:00.000Z',
    3,
    'session-abc',
    'user_preference',
    'mag'
);

INSERT INTO memories (
    id, content, content_hash, source_type, tags, importance, metadata,
    created_at, event_at, last_accessed_at, access_count,
    session_id, event_type, project
) VALUES (
    'v014-mem-002',
    'MAG uses additive SQLite migrations for schema evolution',
    'hash-002',
    'user',
    '["architecture","database"]',
    0.8,
    '{}',
    '2025-10-02T09:00:00.000Z',
    '2025-10-02T09:00:00.000Z',
    '2025-10-02T09:00:00.000Z',
    1,
    'session-abc',
    'lesson_learned',
    'mag'
);

INSERT INTO memories (
    id, content, content_hash, source_type, tags, importance, metadata,
    created_at, event_at, last_accessed_at, access_count,
    session_id, event_type, project
) VALUES (
    'v014-mem-003',
    'The embedding model is bge-small-en-v1.5 with 384 dimensions',
    'hash-003',
    'user',
    '["architecture","embeddings"]',
    0.85,
    '{"model":"bge-small-en-v1.5"}',
    '2025-10-03T11:00:00.000Z',
    '2025-10-03T11:00:00.000Z',
    '2025-10-03T11:00:00.000Z',
    2,
    'session-xyz',
    'technical_note',
    'mag'
);

-- Sample relationship between memories
INSERT INTO relationships (id, source_id, target_id, rel_type, weight, metadata, created_at)
VALUES (
    'rel-001',
    'v014-mem-001',
    'v014-mem-002',
    'related_to',
    0.75,
    '{}',
    '2025-10-02T12:00:00.000Z'
);

-- Populate FTS index
INSERT INTO memories_fts (id, content)
SELECT id, content FROM memories;
