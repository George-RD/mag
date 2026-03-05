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
    true
}
