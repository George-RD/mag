/// Compute the p-th percentile of a slice of millisecond measurements.
/// `percentile` must be in [0.0, 100.0]. Returns 0.0 for empty slices.
///
/// Uses the nearest-rank method (sorted, index = ceil(p/100 * n) - 1).
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn percentile_ms(samples: &[u128], percentile: f64) -> f64 {
    if samples.is_empty() || !(0.0..=100.0).contains(&percentile) {
        return 0.0;
    }
    let mut sorted: Vec<u128> = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    // Nearest-rank: index = ceil(p/100 * n) - 1, clamped to valid range.
    let rank = ((percentile / 100.0) * n as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(n - 1);
    sorted[idx] as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slice_returns_zero() {
        assert_eq!(percentile_ms(&[], 95.0), 0.0);
    }

    #[test]
    fn single_element() {
        assert_eq!(percentile_ms(&[42], 95.0), 42.0);
        assert_eq!(percentile_ms(&[42], 50.0), 42.0);
        assert_eq!(percentile_ms(&[42], 0.0), 42.0);
    }

    #[test]
    fn known_distribution() {
        // 20 elements: 1..=20
        let samples: Vec<u128> = (1..=20).collect();
        // P95: ceil(0.95 * 20) - 1 = ceil(19) - 1 = 18 => value 19
        assert_eq!(percentile_ms(&samples, 95.0), 19.0);
        // P50: ceil(0.50 * 20) - 1 = ceil(10) - 1 = 9 => value 10
        assert_eq!(percentile_ms(&samples, 50.0), 10.0);
        // P100: ceil(1.0 * 20) - 1 = 19 => value 20
        assert_eq!(percentile_ms(&samples, 100.0), 20.0);
    }

    #[test]
    fn unsorted_input() {
        let samples = vec![50, 10, 30, 20, 40];
        // Sorted: [10, 20, 30, 40, 50]
        // P95: ceil(0.95 * 5) - 1 = ceil(4.75) - 1 = 4 => value 50
        assert_eq!(percentile_ms(&samples, 95.0), 50.0);
        // P50: ceil(0.50 * 5) - 1 = ceil(2.5) - 1 = 2 => value 30
        assert_eq!(percentile_ms(&samples, 50.0), 30.0);
    }

    #[test]
    fn boundary_percentiles() {
        let samples = vec![5, 10, 15];
        assert_eq!(percentile_ms(&samples, 0.0), 5.0);
        assert_eq!(percentile_ms(&samples, 100.0), 15.0);
    }
}
