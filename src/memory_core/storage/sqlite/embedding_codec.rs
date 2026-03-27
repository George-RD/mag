use anyhow::{Context, Result};

pub(super) fn encode_embedding(v: &[f32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &val in v {
        buf.extend_from_slice(&val.to_le_bytes());
    }
    buf
}

/// Decodes an embedding BLOB. Detects format by inspecting the first
/// non-whitespace byte: `[` means JSON, anything else means binary
/// (little-endian f32). Malformed JSON is reported as an error rather
/// than silently reinterpreted as binary.
pub(super) fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>> {
    if blob.is_empty() {
        return Ok(Vec::new());
    }
    // Trim leading ASCII whitespace before format detection
    let trimmed = blob
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(blob.len());
    let effective = &blob[trimmed..];

    if effective.first() == Some(&b'[') {
        // Looks like JSON — parse as JSON only, don't fall through to binary
        return serde_json::from_slice(blob).context("failed to decode JSON embedding");
    }
    // Binary format: length must be a multiple of 4
    if blob.len().is_multiple_of(4) {
        return Ok(decode_binary_embedding(blob));
    }
    anyhow::bail!(
        "failed to decode embedding: not valid binary (len={} not multiple of 4) and not JSON",
        blob.len()
    )
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
