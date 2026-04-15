use rmcp::ErrorData as McpError;

// ──────────────────────── Validation constants ────────────────────────

/// Hard upper bound for any `limit` parameter to prevent OOM via giant result sets.
pub(crate) const MAX_RESULT_LIMIT: usize = 1000;

/// Maximum number of items in a single `store_batch` call.
pub(crate) const MAX_BATCH_SIZE: usize = 1000;

/// Validate that a float parameter is finite (not NaN or Infinity).
pub(crate) fn require_finite(name: &str, value: f64) -> Result<(), McpError> {
    if value.is_nan() || value.is_infinite() {
        return Err(McpError::invalid_params(
            format!("{name} must be a finite number"),
            None,
        ));
    }
    Ok(())
}
