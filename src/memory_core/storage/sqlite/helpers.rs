use std::sync::MutexGuard;

use super::*;

/// Fallback timestamp used when a row's `created_at` column is missing or unparseable.
pub(super) const EPOCH_FALLBACK: &str = "1970-01-01T00:00:00.000Z";

/// Computes a u64 hash key for the query result cache.
pub(super) fn query_cache_key(query: &str, limit: usize, opts: &super::SearchOptions) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    limit.hash(&mut hasher);
    opts.event_type
        .as_ref()
        .map(|et| et.to_string())
        .hash(&mut hasher);
    opts.project.hash(&mut hasher);
    opts.session_id.hash(&mut hasher);
    opts.include_superseded.hash(&mut hasher);
    opts.importance_min.map(|f| f.to_bits()).hash(&mut hasher);
    opts.created_after.hash(&mut hasher);
    opts.created_before.hash(&mut hasher);
    opts.context_tags.hash(&mut hasher);
    opts.entity_id.hash(&mut hasher);
    opts.agent_type.hash(&mut hasher);
    opts.event_after.hash(&mut hasher);
    opts.event_before.hash(&mut hasher);
    opts.explain.hash(&mut hasher);
    hasher.finish()
}

/// Returns `true` when the query looks like a keyword/identifier lookup
/// that should skip ONNX embedding and vector search (FTS5 only).
///
/// Heuristics:
/// - Contains backtick-wrapped code (`` `...` ``)
/// - Looks like a file path (contains `/` with no spaces)
/// - Is a CamelCase identifier (2+ uppercase letters, no spaces)
/// - Is a snake_case identifier (contains `_`, no spaces, all lowercase)
pub(super) fn is_keyword_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Contains backtick-wrapped code
    if trimmed.matches('`').count() >= 2 {
        return true;
    }

    // Looks like a file path (contains `/` with no spaces)
    if !trimmed.contains(' ') && trimmed.contains('/') {
        return true;
    }

    // Is a CamelCase identifier (2+ uppercase letters, no spaces, alphanumeric only)
    if !trimmed.contains(' ') {
        let upper_count = trimmed.chars().filter(|c| c.is_uppercase()).count();
        if upper_count >= 2 && trimmed.chars().all(|c| c.is_alphanumeric()) {
            return true;
        }
    }

    // Is a snake_case identifier (contains `_`, no spaces, all lowercase alphanumeric + `_`)
    if !trimmed.contains(' ')
        && trimmed.contains('_')
        && trimmed
            .chars()
            .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return true;
    }

    false
}

/// Default number of reader connections in the pool for file-backed databases.
const DEFAULT_READER_COUNT: usize = 4;

/// Default number of writes between WAL checkpoints.
const WAL_CHECKPOINT_INTERVAL: u64 = 10;

/// Connection pool providing one writer connection and N reader connections.
///
/// In WAL mode, SQLite allows concurrent readers while a single writer holds
/// the write lock. `rusqlite::Connection` is `!Sync`, so each connection is
/// wrapped in its own `Mutex`.
///
/// For in-memory databases (`:memory:` or `file::memory:`) that cannot share
/// state across connections, the pool falls back to single-connection mode
/// where all operations use the writer.
#[derive(Debug)]
pub(super) struct ConnPool {
    writer: Mutex<Connection>,
    readers: Vec<Mutex<Connection>>,
    /// Round-robin counter for reader selection.
    reader_idx: std::sync::atomic::AtomicUsize,
    /// Monotonic write counter for periodic WAL checkpoints.
    write_count: std::sync::atomic::AtomicU64,
    /// Whether this pool is file-backed (WAL checkpoints only apply to files).
    is_file_backed: bool,
}

impl ConnPool {
    /// Opens a file-backed connection pool: one writer + N readers.
    ///
    /// All connections share the same SQLite database file and run in WAL mode.
    /// Performs a startup WAL checkpoint (TRUNCATE) to reclaim any stale WAL
    /// from previous runs.
    pub(super) fn open_file(path: &Path, embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        super::initialize_parent_dir(path)?;

        let writer = open_connection(path)?;
        // Writer sets WAL mode; readers inherit it from the file.
        initialize_schema(&writer, embedding_dim)?;

        // Startup checkpoint: safe because we're the only writer at init time.
        if let Err(e) = writer.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::debug!("startup WAL checkpoint skipped: {e}");
        }

        let mut readers = Vec::with_capacity(DEFAULT_READER_COUNT);
        for _ in 0..DEFAULT_READER_COUNT {
            let reader = open_connection(path)?;
            configure_reader(&reader)?;
            readers.push(Mutex::new(reader));
        }

        Ok(Self {
            writer: Mutex::new(writer),
            readers,
            reader_idx: std::sync::atomic::AtomicUsize::new(0),
            write_count: std::sync::atomic::AtomicU64::new(0),
            is_file_backed: true,
        })
    }

    /// Opens an in-memory pool (single connection, no reader pool).
    ///
    /// Used for tests and non-file-backed scenarios where multiple connections
    /// cannot share state.
    pub(super) fn open_in_memory(embedding_dim: usize) -> Result<Self> {
        #[cfg(feature = "sqlite-vec")]
        super::ensure_vec_extension_registered();

        let conn = Connection::open_in_memory().context("failed to open in-memory sqlite")?;
        initialize_schema(&conn, embedding_dim)?;

        Ok(Self {
            writer: Mutex::new(conn),
            readers: Vec::new(),
            reader_idx: std::sync::atomic::AtomicUsize::new(0),
            write_count: std::sync::atomic::AtomicU64::new(0),
            is_file_backed: false,
        })
    }

    /// Acquires the writer connection.
    pub(super) fn writer(&self) -> Result<MutexGuard<'_, Connection>> {
        self.writer
            .lock()
            .map_err(|_| anyhow!("sqlite writer mutex poisoned"))
    }

    /// Returns `true` when the pool has dedicated reader connections.
    pub(super) fn has_readers(&self) -> bool {
        !self.readers.is_empty()
    }

    /// Increments the write counter and runs a passive WAL checkpoint every
    /// [`WAL_CHECKPOINT_INTERVAL`] writes. Call after each write transaction
    /// commits. No-op for in-memory databases.
    pub(super) fn note_write(&self) {
        if !self.is_file_backed {
            return;
        }
        let count = self
            .write_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        if !count.is_multiple_of(WAL_CHECKPOINT_INTERVAL) {
            return;
        }
        if let Ok(conn) = self.writer() {
            match conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);") {
                Ok(()) => tracing::debug!(writes = count, "WAL passive checkpoint completed"),
                Err(e) => tracing::debug!(writes = count, "WAL passive checkpoint failed: {e}"),
            }
        }
    }

    /// Acquires a reader connection via round-robin. Falls back to the writer
    /// when no dedicated readers exist (in-memory mode).
    pub(super) fn reader(&self) -> Result<MutexGuard<'_, Connection>> {
        if self.readers.is_empty() {
            return self.writer();
        }
        let idx = self
            .reader_idx
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            % self.readers.len();
        self.readers[idx]
            .lock()
            .map_err(|_| anyhow!("sqlite reader mutex poisoned"))
    }
}

/// Opens a single SQLite connection with standard PRAGMAs.
fn open_connection(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)
        .with_context(|| format!("failed to open sqlite database at {}", path.display()))?;
    configure_reader(&conn)?;
    Ok(conn)
}

/// Applies read-oriented PRAGMAs shared by all connections (writer and readers).
fn configure_reader(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;\
         PRAGMA busy_timeout=5000;\
         PRAGMA cache_size=-16000;\
         PRAGMA mmap_size=33554432;\
         PRAGMA synchronous=NORMAL;\
         PRAGMA temp_store=MEMORY;",
    )
    .context("failed to set connection PRAGMAs")?;
    Ok(())
}

pub(super) fn canonicalize(content: &str) -> String {
    let stripped: String = content
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '*' | '#' | '`' | '~' | '[' | ']' | '(' | ')' | '>' | '|' | '_'
            )
        })
        .collect();
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

pub(super) fn canonical_hash(content: &str) -> String {
    let canonical = canonicalize(content);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn parse_tags_from_db(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub(super) fn parse_metadata_from_db(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()))
}

pub(super) fn build_fts5_query(input: &str) -> String {
    let raw_tokens: Vec<&str> = input.split_whitespace().filter(|t| !t.is_empty()).collect();

    if raw_tokens.is_empty() {
        return "\"\"".to_string();
    }

    // Escape each token for FTS5 (double-quote escaping) and wrap in quotes.
    let escaped: Vec<String> = raw_tokens
        .iter()
        .map(|t| {
            let e = t.replace('"', "\"\"");
            format!("\"{e}\"")
        })
        .collect();

    // For 1-2 token queries, bigrams would be redundant (either a single
    // token or an exact duplicate of the full query). Just join with OR.
    if raw_tokens.len() < 3 {
        return escaped.join(" OR ");
    }

    // 3+ tokens: append adjacent-token bigrams as quoted phrases.
    let bigrams: Vec<String> = raw_tokens
        .windows(2)
        .map(|pair| {
            let a = pair[0].replace('"', "\"\"");
            let b = pair[1].replace('"', "\"\"");
            format!("\"{a} {b}\"")
        })
        .collect();

    let mut parts = escaped;
    parts.extend(bigrams);
    parts.join(" OR ")
}

/// Encodes a slice of f32 values as little-endian bytes.
pub(super) fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &val in v {
        buf.extend_from_slice(&val.to_le_bytes());
    }
    buf
}

/// Decodes an embedding BLOB. Tries binary (little-endian f32) first,
/// falls back to JSON for backwards compatibility with existing data.
pub(super) fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>> {
    if blob.is_empty() {
        return Ok(Vec::new());
    }
    // Binary format: length must be a multiple of 4
    if blob.len().is_multiple_of(4) {
        // Quick heuristic: JSON always starts with '[' (0x5B)
        if blob[0] != b'[' {
            return Ok(decode_binary_embedding(blob));
        }
        // First byte is '[' — could be JSON or binary coincidence.
        // Try JSON first (backwards compat), fall back to binary.
        if let Ok(v) = serde_json::from_slice::<Vec<f32>>(blob) {
            return Ok(v);
        }
        return Ok(decode_binary_embedding(blob));
    }
    // Not a multiple of 4 — must be JSON
    serde_json::from_slice(blob).context("failed to decode embedding (neither binary nor JSON)")
}

fn decode_binary_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| {
            let mut bytes = [0_u8; 4];
            bytes.copy_from_slice(chunk);
            f32::from_le_bytes(bytes)
        })
        .collect()
}

pub(crate) fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[allow(dead_code)]
pub(super) fn resolve_priority(event_type: Option<&str>, priority: Option<i64>) -> u8 {
    if let Some(value) = priority
        && (1..=5).contains(&value)
    {
        return u8::try_from(value).unwrap_or(3);
    }
    event_type
        .map(|value| {
            let event_type = value
                .parse::<EventType>()
                .unwrap_or_else(|err| match err {});
            let priority = event_type.default_priority();
            if priority == 0 {
                3
            } else {
                u8::try_from(priority).unwrap_or(3)
            }
        })
        .unwrap_or(3)
}

pub(super) fn normalize_for_dedup(content: &str) -> String {
    let collapsed = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    collapsed.chars().take(150).collect()
}

/// Basic ISO 8601 validation: accepts date-only (`YYYY-MM-DD`) or
/// datetime with optional timezone (`YYYY-MM-DDThh:mm:ss…`).
/// This is intentionally lenient — it catches gross typos without
/// requiring a full RFC 3339 parser.
pub(super) fn validate_iso8601(s: &str) -> bool {
    // Minimum: "YYYY-MM-DD" (10 chars)
    if s.len() < 10 {
        return false;
    }
    let bytes = s.as_bytes();
    // First 4 chars must be digits (year)
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Hyphens at positions 4 and 7
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    // Month and day digits
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    true
}

pub(super) fn escape_like_pattern(input: &str) -> String {
    let escaped = input
        .to_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub(super) fn search_result_from_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    let raw_tags: String = row.get(2)?;
    let raw_metadata: String = row.get(4)?;
    let event_type_str: Option<String> = row.get(5).ok();
    Ok(SearchResult {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: parse_tags_from_db(&raw_tags),
        importance: row.get(3)?,
        metadata: parse_metadata_from_db(&raw_metadata),
        event_type: event_type_from_sql(event_type_str),
        session_id: row.get(6).ok(),
        project: row.get(7).ok(),
        entity_id: row.get(8).ok(),
        agent_type: row.get(9).ok(),
    })
}

/// Converts an `Option<EventType>` to an `Option<String>` for SQL parameter binding.
pub(super) fn event_type_to_sql(et: &Option<EventType>) -> Option<String> {
    et.as_ref().map(|e| e.to_string())
}

/// Parses an `Option<String>` from the DB into an `Option<EventType>`.
pub(super) fn event_type_from_sql(s: Option<String>) -> Option<EventType> {
    EventType::from_optional(s.as_deref())
}

/// Appends WHERE-clause fragments for every non-None field in `opts` to `sql`,
/// pushing corresponding values into `params`. `idx` is the next `?N` placeholder
/// index and is updated in place. `col_prefix` is prepended to column names
/// (e.g. `"m."` for joined queries, `""` for direct table queries).
pub(super) fn append_search_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    opts: &SearchOptions,
    col_prefix: &str,
) {
    use rusqlite::types::Value as SqlValue;

    if let Some(ref event_type) = opts.event_type {
        sql.push_str(&format!(" AND {}event_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(event_type.to_string()));
        *idx += 1;
    }
    if let Some(ref project) = opts.project {
        sql.push_str(&format!(" AND {}project = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(project.clone()));
        *idx += 1;
    }
    if let Some(ref session_id) = opts.session_id {
        sql.push_str(&format!(" AND {}session_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(session_id.clone()));
        *idx += 1;
    }
    if let Some(ref entity_id) = opts.entity_id {
        sql.push_str(&format!(" AND {}entity_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(entity_id.clone()));
        *idx += 1;
    }
    if let Some(ref agent_type) = opts.agent_type {
        sql.push_str(&format!(" AND {}agent_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(agent_type.clone()));
        *idx += 1;
    }
    if let Some(importance_min) = opts.importance_min {
        sql.push_str(&format!(" AND {}importance >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Real(importance_min));
        *idx += 1;
    }
    if let Some(ref created_after) = opts.created_after {
        sql.push_str(&format!(" AND {}created_at >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_after.clone()));
        *idx += 1;
    }
    if let Some(ref created_before) = opts.created_before {
        sql.push_str(&format!(" AND {}created_at <= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_before.clone()));
        *idx += 1;
    }
    if let Some(ref event_after) = opts.event_after {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') >= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_after.clone()));
        *idx += 1;
    }
    if let Some(ref event_before) = opts.event_before {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') <= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_before.clone()));
        *idx += 1;
    }
}

pub(super) fn append_context_tag_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    context_tags: Option<&[String]>,
    tags_expr: &str,
) {
    use rusqlite::types::Value as SqlValue;

    for tag in context_tags
        .into_iter()
        .flatten()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        sql.push_str(&format!(
            " AND ((json_valid({tags_expr}) AND EXISTS (SELECT 1 FROM json_each({tags_expr}) WHERE lower(value) = lower(?{idx}))) \
               OR (NOT json_valid({tags_expr}) AND {tags_expr} != '' AND instr(',' || lower({tags_expr}) || ',', ',' || lower(?{idx}) || ',') > 0))"
        ));
        params.push(SqlValue::Text(tag.to_owned()));
        *idx += 1;
    }
}

/// Converts a `Vec<SqlValue>` to a `Vec<&dyn ToSql>` for rusqlite parameter binding.
pub(super) fn to_param_refs(values: &[rusqlite::types::Value]) -> Vec<&dyn rusqlite::types::ToSql> {
    values
        .iter()
        .map(|v| v as &dyn rusqlite::types::ToSql)
        .collect()
}

pub(super) fn matches_search_options(
    candidate: &RankedSemanticCandidate,
    opts: &SearchOptions,
) -> bool {
    if let Some(ref event_type) = opts.event_type
        && candidate.result.event_type.as_ref() != Some(event_type)
    {
        return false;
    }
    if let Some(project) = opts.project.as_deref()
        && candidate.result.project.as_deref() != Some(project)
    {
        return false;
    }
    if let Some(session_id) = opts.session_id.as_deref()
        && candidate.result.session_id.as_deref() != Some(session_id)
    {
        return false;
    }
    if let Some(importance_min) = opts.importance_min
        && candidate.result.importance < importance_min
    {
        return false;
    }
    if let Some(created_after) = opts.created_after.as_deref()
        && candidate.created_at.as_str() < created_after
    {
        return false;
    }
    if let Some(created_before) = opts.created_before.as_deref()
        && candidate.created_at.as_str() > created_before
    {
        return false;
    }
    if let Some(entity_id) = opts.entity_id.as_deref()
        && candidate.entity_id.as_deref() != Some(entity_id)
    {
        return false;
    }
    if let Some(agent_type) = opts.agent_type.as_deref()
        && candidate.agent_type.as_deref() != Some(agent_type)
    {
        return false;
    }
    if let Some(event_after) = opts.event_after.as_deref()
        && candidate.event_at.as_str() < event_after
    {
        return false;
    }
    if let Some(event_before) = opts.event_before.as_deref()
        && candidate.event_at.as_str() > event_before
    {
        return false;
    }
    if let Some(context_tags) = opts.context_tags.as_ref() {
        let mut normalized_tags = context_tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
            .peekable();
        if normalized_tags.peek().is_some()
            && !normalized_tags.all(|tag| {
                candidate
                    .result
                    .tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(tag))
            })
        {
            return false;
        }
    }
    true
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_distance_to_similarity(distance: f64) -> f64 {
    (1.0 - distance).max(0.0)
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_delete(conn: &rusqlite::Connection, memory_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM vec_memories WHERE memory_id = ?1",
        params![memory_id],
    )
    .context("failed to delete from vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_upsert(
    conn: &rusqlite::Connection,
    memory_id: &str,
    embedding: &[u8],
) -> Result<()> {
    // vec0 virtual tables don't support INSERT OR REPLACE; delete first then insert.
    vec_delete(conn, memory_id)?;
    conn.execute(
        "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
        params![memory_id, embedding],
    )
    .context("failed to upsert into vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_knn_search(
    conn: &rusqlite::Connection,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<(String, f64)>> {
    let embedding_blob = encode_embedding(query_embedding);
    let k = k as i64;
    let mut stmt = conn
        .prepare("SELECT memory_id, distance FROM vec_memories WHERE embedding MATCH ?1 AND k = ?2")
        .context("failed to prepare vec KNN query")?;
    let rows = stmt
        .query_map(params![embedding_blob, k], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .context("failed to execute vec KNN query")?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.context("failed to decode vec KNN row")?);
    }
    Ok(results)
}

/// Maximum number of memory IDs hydrated per SQLite `IN (...)` batch.
#[cfg(feature = "sqlite-vec")]
pub(super) const HYDRATE_ID_CHUNK_SIZE: usize = 900;

#[cfg(feature = "sqlite-vec")]
#[derive(Debug, Clone)]
/// Fully decoded memory row used by sqlite-vec search paths after batched lookup.
pub(super) struct HydratedMemoryRow {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub metadata: serde_json::Value,
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i64>,
    pub created_at: String,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub event_at: String,
}

#[cfg(feature = "sqlite-vec")]
/// Hydrates memory rows for an ordered ID list while applying the same filters as search paths.
///
/// The returned map is keyed by memory ID so callers can preserve their own ranking order.
pub(super) fn hydrate_memories_by_ids(
    conn: &rusqlite::Connection,
    ids: &[String],
    include_superseded: bool,
    opts: Option<&SearchOptions>,
    include_context_tags: bool,
) -> Result<HashMap<String, HydratedMemoryRow>> {
    use rusqlite::types::Value as SqlValue;

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut hydrated = HashMap::with_capacity(ids.len());

    for chunk in ids.chunks(HYDRATE_ID_CHUNK_SIZE) {
        let mut sql = String::from(
            "SELECT id, content, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at
             FROM memories
             WHERE id IN (",
        );
        let mut params: Vec<SqlValue> = Vec::with_capacity(chunk.len() + 12);
        for (idx, memory_id) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", idx + 1));
            params.push(SqlValue::Text(memory_id.clone()));
        }
        sql.push(')');

        let mut next_idx = chunk.len() + 1;
        if !include_superseded {
            sql.push_str(" AND superseded_by_id IS NULL");
        }
        if let Some(search_opts) = opts {
            append_search_filters(&mut sql, &mut params, &mut next_idx, search_opts, "");
            if include_context_tags {
                append_context_tag_filters(
                    &mut sql,
                    &mut params,
                    &mut next_idx,
                    search_opts.context_tags.as_deref(),
                    "tags",
                );
            }
        }

        let mut stmt = conn
            .prepare(&sql)
            .context("failed to prepare batched memory hydration query")?;
        let param_refs = to_param_refs(&params);
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(HydratedMemoryRow {
                    id: row.get::<_, String>(0)?,
                    content: row.get::<_, String>(1)?,
                    tags: parse_tags_from_db(&row.get::<_, String>(2)?),
                    importance: row.get::<_, f64>(3)?,
                    metadata: parse_metadata_from_db(&row.get::<_, String>(4)?),
                    event_type: event_type_from_sql(row.get::<_, Option<String>>(5).ok().flatten()),
                    session_id: row.get::<_, Option<String>>(6).ok().flatten(),
                    project: row.get::<_, Option<String>>(7).ok().flatten(),
                    priority: row.get::<_, Option<i64>>(8).ok().flatten(),
                    created_at: row
                        .get::<_, String>(9)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                    entity_id: row.get::<_, Option<String>>(10).ok().flatten(),
                    agent_type: row.get::<_, Option<String>>(11).ok().flatten(),
                    event_at: row
                        .get::<_, String>(12)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                })
            })
            .context("failed to execute batched memory hydration query")?;

        for row in rows {
            let row = row.context("failed to decode hydrated memory row")?;
            hydrated.insert(row.id.clone(), row);
        }
    }

    Ok(hydrated)
}

/// Result of expanding temporal references from a query string.
#[derive(Debug, Default)]
pub(super) struct TemporalExpansion {
    /// The query with temporal phrases stripped out.
    pub cleaned_query: String,
    /// ISO 8601 lower bound inferred from the temporal phrase.
    pub event_after: Option<String>,
    /// ISO 8601 upper bound inferred from the temporal phrase.
    pub event_before: Option<String>,
}

/// Detects temporal references in a query and converts them to date filters.
///
/// Recognized patterns:
/// - "today", "yesterday"
/// - "this week", "last week", "this month", "last month"
/// - "N days ago", "N weeks ago", "N months ago"
/// - "last N days", "last N weeks", "last N months", "past N days/weeks/months"
///
/// Returns the cleaned query (temporal phrase removed) and inferred date bounds.
/// If no temporal phrase is detected, returns the original query unchanged.
pub(super) fn expand_temporal_query(query: &str, now: &chrono::NaiveDate) -> TemporalExpansion {
    use chrono::{Datelike, Duration, NaiveDate};

    let lower = query.to_lowercase();

    // Try each pattern; first match wins.
    let result = if let Some(idx) = lower.find("yesterday") {
        let cleaned = remove_phrase(query, idx, 9);
        let date = (*now - Duration::days(1)).format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(idx) = lower.find("today") {
        let cleaned = remove_phrase(query, idx, 5);
        let date = now.format("%Y-%m-%d").to_string();
        Some((
            cleaned,
            Some(date.clone()),
            Some(format!("{date}T23:59:59")),
        ))
    } else if let Some(re_pos) = lower.find(" ago") {
        // "N days/weeks/months ago"
        let before = &lower[..re_pos];
        let all_words: Vec<&str> = before.split_whitespace().collect();
        if all_words.len() >= 2 {
            let unit = all_words[all_words.len() - 1];
            let num_str = all_words[all_words.len() - 2];
            num_str.parse::<i64>().ok().and_then(|n| {
                let (after, before_date) = compute_ago(now, n, unit)?;
                let phrase = format!("{num_str} {unit} ago");
                let phrase_start = lower.find(&phrase)?;
                let cleaned = remove_phrase(query, phrase_start, phrase.len());
                Some((cleaned, Some(after), Some(before_date)))
            })
        } else {
            None
        }
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "last ", now) {
        Some(m)
    } else if let Some(m) = try_prefix_n_unit(&lower, query, "past ", now) {
        Some(m)
    } else if let Some(idx) = lower.find("last month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1).and_then(|first_this| {
            let last_day_prev = first_this - Duration::days(1);
            let first_prev =
                NaiveDate::from_ymd_opt(last_day_prev.year(), last_day_prev.month(), 1)?;
            Some((
                cleaned,
                Some(first_prev.format("%Y-%m-%d").to_string()),
                Some(format!("{}T23:59:59", last_day_prev.format("%Y-%m-%d"))),
            ))
        })
    } else if let Some(idx) = lower.find("last week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let this_monday = *now - Duration::days(weekday as i64);
        let last_monday = this_monday - Duration::days(7);
        let last_sunday = this_monday - Duration::days(1);
        Some((
            cleaned,
            Some(last_monday.format("%Y-%m-%d").to_string()),
            Some(format!("{}T23:59:59", last_sunday.format("%Y-%m-%d"))),
        ))
    } else if let Some(idx) = lower.find("this week") {
        let cleaned = remove_phrase(query, idx, 9);
        let weekday = now.weekday().num_days_from_monday();
        let start = *now - Duration::days(weekday as i64);
        Some((cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else if let Some(idx) = lower.find("this month") {
        let cleaned = remove_phrase(query, idx, 10);
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .map(|start| (cleaned, Some(start.format("%Y-%m-%d").to_string()), None))
    } else {
        None
    };

    match result {
        Some((cleaned, after, before)) => {
            let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
            if trimmed.is_empty() {
                TemporalExpansion {
                    cleaned_query: query.to_string(),
                    event_after: after,
                    event_before: before,
                }
            } else {
                TemporalExpansion {
                    cleaned_query: trimmed,
                    event_after: after,
                    event_before: before,
                }
            }
        }
        None => TemporalExpansion {
            cleaned_query: query.to_string(),
            event_after: None,
            event_before: None,
        },
    }
}

/// Tries to match "last N unit" or "past N unit" at `prefix` position.
fn try_prefix_n_unit(
    lower: &str,
    query: &str,
    prefix: &str,
    now: &chrono::NaiveDate,
) -> Option<(String, Option<String>, Option<String>)> {
    let idx = lower.find(prefix)?;
    let rest = &lower[idx + prefix.len()..];
    let tokens: Vec<&str> = rest.split_whitespace().take(2).collect();
    if tokens.len() < 2 {
        return None;
    }
    let n: i64 = tokens[0].parse().ok()?;
    let unit = tokens[1];
    let (after, before_date) = compute_ago(now, n, unit)?;
    let phrase_len = prefix.len() + tokens[0].len() + 1 + tokens[1].len();
    let cleaned = remove_phrase(query, idx, phrase_len);
    Some((cleaned, Some(after), Some(before_date)))
}

fn remove_phrase(query: &str, start: usize, len: usize) -> String {
    let mut result = String::with_capacity(query.len());
    result.push_str(&query[..start]);
    if start + len < query.len() {
        result.push_str(&query[start + len..]);
    }
    result
}

fn compute_ago(now: &chrono::NaiveDate, n: i64, unit: &str) -> Option<(String, String)> {
    use chrono::{Datelike, Duration};
    let start = match unit.trim_end_matches('s') {
        "day" => *now - Duration::days(n),
        "week" => *now - Duration::weeks(n),
        "month" => {
            // Approximate: subtract n months
            let n_i32 = i32::try_from(n).ok()?;
            #[allow(clippy::cast_sign_loss)]
            let total_months = now.year() * 12 + now.month() as i32 - 1 - n_i32;
            let year = total_months / 12;
            let month = u32::try_from(total_months % 12 + 1).ok()?;
            let day = now.day().min(28); // safe day
            chrono::NaiveDate::from_ymd_opt(year, month, day)?
        }
        _ => return None,
    };
    Some((
        start.format("%Y-%m-%d").to_string(),
        format!("{}T23:59:59", now.format("%Y-%m-%d")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_query_empty_input() {
        assert_eq!(build_fts5_query(""), "\"\"");
        assert_eq!(build_fts5_query("   "), "\"\"");
    }

    #[test]
    fn fts5_query_single_token() {
        assert_eq!(build_fts5_query("database"), "\"database\"");
    }

    #[test]
    fn fts5_query_two_tokens_no_bigrams() {
        // Two tokens: bigrams would duplicate the full query, so skip them.
        assert_eq!(
            build_fts5_query("database connection"),
            "\"database\" OR \"connection\""
        );
    }

    #[test]
    fn fts5_query_three_tokens_with_bigrams() {
        assert_eq!(
            build_fts5_query("database connection pool"),
            "\"database\" OR \"connection\" OR \"pool\" \
             OR \"database connection\" OR \"connection pool\""
        );
    }

    #[test]
    fn fts5_query_four_tokens_with_bigrams() {
        assert_eq!(
            build_fts5_query("the quick brown fox"),
            "\"the\" OR \"quick\" OR \"brown\" OR \"fox\" \
             OR \"the quick\" OR \"quick brown\" OR \"brown fox\""
        );
    }

    #[test]
    fn fts5_query_special_chars_escaped() {
        // Double-quotes in tokens are escaped by doubling.
        assert_eq!(
            build_fts5_query("say \"hello\" world"),
            "\"say\" OR \"\"\"hello\"\"\" OR \"world\" \
             OR \"say \"\"hello\"\"\" OR \"\"\"hello\"\" world\""
        );
    }

    #[test]
    fn fts5_query_extra_whitespace_collapsed() {
        assert_eq!(
            build_fts5_query("  database   connection   pool  "),
            "\"database\" OR \"connection\" OR \"pool\" \
             OR \"database connection\" OR \"connection pool\""
        );
    }

    #[test]
    fn temporal_today() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors today", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-08"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-08T23:59:59"));
    }

    #[test]
    fn temporal_yesterday() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("what happened yesterday", &now);
        assert_eq!(exp.cleaned_query, "what happened");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-07"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-07T23:59:59"));
    }

    #[test]
    fn temporal_n_days_ago() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("decisions 3 days ago", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-05"));
    }

    #[test]
    fn temporal_last_n_days() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("bugs last 7 days", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_this_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap(); // Sunday
        let exp = expand_temporal_query("decisions this week", &now);
        assert_eq!(exp.cleaned_query, "decisions");
        // March 8 2026 is Sunday, weekday=6 from Monday. Monday = March 2.
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-02"));
        assert!(exp.event_before.is_none()); // open-ended
    }

    #[test]
    fn temporal_last_week() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors last week", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-23"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-03-01T23:59:59"));
    }

    #[test]
    fn temporal_this_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("lessons this month", &now);
        assert_eq!(exp.cleaned_query, "lessons");
        assert_eq!(exp.event_after.as_deref(), Some("2026-03-01"));
    }

    #[test]
    fn temporal_last_month() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let exp = expand_temporal_query("bugs last month", &now);
        assert_eq!(exp.cleaned_query, "bugs");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-01"));
        assert_eq!(exp.event_before.as_deref(), Some("2026-02-28T23:59:59"));
    }

    #[test]
    fn temporal_no_match() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("database connection pool", &now);
        assert_eq!(exp.cleaned_query, "database connection pool");
        assert!(exp.event_after.is_none());
        assert!(exp.event_before.is_none());
    }

    #[test]
    fn temporal_query_only_temporal() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("today", &now);
        // When only temporal phrase, keep original query for search
        assert_eq!(exp.cleaned_query, "today");
        assert!(exp.event_after.is_some());
    }

    #[test]
    fn temporal_past_n_weeks() {
        let now = chrono::NaiveDate::from_ymd_opt(2026, 3, 8).unwrap();
        let exp = expand_temporal_query("errors past 2 weeks", &now);
        assert_eq!(exp.cleaned_query, "errors");
        assert_eq!(exp.event_after.as_deref(), Some("2026-02-22"));
    }

    // ── is_keyword_query tests ──────────────────────────────────────────

    #[test]
    fn keyword_backtick_code() {
        assert!(is_keyword_query("`FooBar`"));
        assert!(is_keyword_query("find `my_func` usage"));
    }

    #[test]
    fn keyword_file_path() {
        assert!(is_keyword_query("src/main.rs"));
        assert!(is_keyword_query("./config.toml"));
        assert!(is_keyword_query("~/projects/foo"));
        assert!(is_keyword_query("/usr/local/bin"));
    }

    #[test]
    fn keyword_camel_case() {
        assert!(is_keyword_query("SqliteStorage"));
        assert!(is_keyword_query("McpMemoryServer"));
        assert!(!is_keyword_query("sqlite"));
    }

    #[test]
    fn keyword_snake_case() {
        assert!(is_keyword_query("query_cache_key"));
        assert!(is_keyword_query("embed_batch"));
        assert!(!is_keyword_query("query"));
    }

    #[test]
    fn keyword_natural_language_not_keyword() {
        assert!(!is_keyword_query("what framework are we using"));
        assert!(!is_keyword_query("database connection pool"));
        assert!(!is_keyword_query(""));
    }
}
