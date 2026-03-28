use anyhow::{Context, Result};

pub(super) fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &val in v {
        buf.extend_from_slice(&val.to_le_bytes());
    }
    buf
}

/// Decodes an embedding BLOB. Tries binary (little-endian f32) first,
/// falls back to JSON for backwards compatibility with existing data.
///
/// Binary embeddings are always a multiple of 4 bytes. If the blob starts
/// with `[` (0x5B) it *could* be JSON — try JSON first, then fall back to
/// binary decode (a binary f32 may coincidentally start with 0x5B).
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

/// Dot product of two vectors. Equivalent to cosine similarity when inputs are L2-normalized.
pub(crate) fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    debug_assert!(
        (a.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() < 0.01,
        "input a is not L2-normalized"
    );
    debug_assert!(
        (b.iter().map(|v| v * v).sum::<f32>().sqrt() - 1.0).abs() < 0.01,
        "input b is not L2-normalized"
    );
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}
