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
    let len = blob.len() / 4;
    let mut vec = Vec::with_capacity(len);
    #[cfg(target_endian = "little")]
    // SAFETY: blob.len() is a multiple of 4 (checked by caller). We copy the
    // raw bytes directly into a properly-aligned Vec<f32> buffer. On
    // little-endian platforms the le f32 wire format is layout-compatible.
    unsafe {
        std::ptr::copy_nonoverlapping(blob.as_ptr(), vec.as_mut_ptr() as *mut u8, blob.len());
        vec.set_len(len);
    }
    #[cfg(not(target_endian = "little"))]
    {
        for chunk in blob.chunks_exact(4) {
            let mut bytes = [0_u8; 4];
            bytes.copy_from_slice(chunk);
            vec.push(f32::from_le_bytes(bytes));
        }
    }
    vec
}
/// Dot product of a f32 slice with a little-endian binary embedding blob.
/// Avoids decoding the blob to a Vec<f32> when the caller only needs the dot product.
pub(crate) fn dot_product_bytes(a: &[f32], blob: &[u8]) -> f32 {
    if blob.len() != a.len() * 4 || a.is_empty() {
        return 0.0;
    }
    #[cfg(target_endian = "little")]
    // SAFETY: blob.len() == a.len() * 4 (checked above). On little-endian
    // platforms the le f32 wire format is layout-compatible with &[f32].
    unsafe {
        let b = std::slice::from_raw_parts(blob.as_ptr() as *const f32, a.len());
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }
    #[cfg(not(target_endian = "little"))]
    {
        a.iter()
            .zip(blob.chunks_exact(4))
            .map(|(x, chunk)| {
                let mut bytes = [0_u8; 4];
                bytes.copy_from_slice(chunk);
                x * f32::from_le_bytes(bytes)
            })
            .sum()
    }
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
