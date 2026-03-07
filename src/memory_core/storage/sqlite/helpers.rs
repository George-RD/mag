use std::sync::MutexGuard;

use super::*;

/// Fallback timestamp used when a row's `created_at` column is missing or unparseable.
pub(super) const EPOCH_FALLBACK: &str = "1970-01-01T00:00:00.000Z";

pub(super) fn lock_conn(conn: &Mutex<Connection>) -> Result<MutexGuard<'_, Connection>> {
    conn.lock()
        .map_err(|_| anyhow!("sqlite connection mutex poisoned"))
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
    let tokens: Vec<String> = input
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();

    if tokens.is_empty() {
        return "\"\"".to_string();
    }

    tokens.join(" ")
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

pub(super) fn resolve_priority(event_type: Option<&str>, priority: Option<i64>) -> u8 {
    if let Some(value) = priority
        && (1..=5).contains(&value)
    {
        return value as u8;
    }
    event_type
        .map(|et| {
            let p = crate::memory_core::default_priority_for_event_type(et);
            if p == 0 { 3 } else { p as u8 }
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
    Ok(SearchResult {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: parse_tags_from_db(&raw_tags),
        importance: row.get(3)?,
        metadata: parse_metadata_from_db(&raw_metadata),
        event_type: row.get(5).ok(),
        session_id: row.get(6).ok(),
        project: row.get(7).ok(),
    })
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
        params.push(SqlValue::Text(event_type.clone()));
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
        sql.push_str(&format!(" AND {}event_at >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(event_after.clone()));
        *idx += 1;
    }
    if let Some(ref event_before) = opts.event_before {
        sql.push_str(&format!(" AND {}event_at <= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(event_before.clone()));
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
    if let Some(event_type) = opts.event_type.as_deref()
        && candidate.result.event_type.as_deref() != Some(event_type)
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
