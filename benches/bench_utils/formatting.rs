/// Truncate a string to at most `max_chars` characters.
pub fn truncate(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Compute a percentage from correct/total counts.
pub fn pct(correct: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        {
            correct as f64 / total as f64 * 100.0
        }
    }
}

/// Map a percentage to a letter grade.
pub fn grade(percentage: f64) -> &'static str {
    if percentage >= 90.0 {
        "A"
    } else if percentage >= 75.0 {
        "B"
    } else if percentage >= 60.0 {
        "C"
    } else if percentage >= 40.0 {
        "D"
    } else {
        "F"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_short() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_exact() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_long() {
        assert_eq!(truncate("hello world", 5), "hello");
    }

    #[test]
    fn test_pct_normal() {
        assert!((pct(3, 4) - 75.0).abs() < 1e-9);
    }

    #[test]
    fn test_pct_zero_total() {
        assert!((pct(0, 0)).abs() < 1e-9);
    }

    #[test]
    fn test_grade_boundaries() {
        assert_eq!(grade(95.0), "A");
        assert_eq!(grade(90.0), "A");
        assert_eq!(grade(80.0), "B");
        assert_eq!(grade(75.0), "B");
        assert_eq!(grade(65.0), "C");
        assert_eq!(grade(60.0), "C");
        assert_eq!(grade(45.0), "D");
        assert_eq!(grade(40.0), "D");
        assert_eq!(grade(30.0), "F");
    }
}
