use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// Type weights for search scoring — higher = more important in results
pub const TYPE_WEIGHTS: &[(&str, f64)] = &[
    ("checkpoint", 2.5),
    ("reminder", 3.0),
    ("decision", 2.0),
    ("lesson_learned", 2.0),
    ("error_pattern", 2.0),
    ("user_preference", 2.0),
    ("task_completion", 1.4),
    ("session_summary", 1.2),
    ("blocked_context", 1.0),
    ("git_commit", 1.0),
    ("git_merge", 1.0),
    ("git_conflict", 1.0),
    ("coordination_snapshot", 0.2),
    ("memory", 1.0),
];

pub fn type_weight(event_type: &str) -> f64 {
    TYPE_WEIGHTS
        .iter()
        .find_map(|(kind, weight)| (*kind == event_type).then_some(*weight))
        .unwrap_or(1.0)
}

pub fn priority_factor(priority: u8) -> f64 {
    0.7 + (priority as f64 * 0.08)
}

pub fn time_decay(created_at: &str) -> f64 {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs_f64(),
        Err(_) => return 1.0,
    };

    let created = match parse_iso8601_to_unix_seconds(created_at) {
        Some(value) => value,
        None => return 1.0,
    };

    let age_seconds = (now - created).max(0.0);
    let days_old = age_seconds / 86_400.0;
    1.0 / (1.0 + (days_old / 30.0))
}

pub fn word_overlap(query_words: &[&str], text: &str) -> f64 {
    let text_words = token_set(text, 3);
    let filtered_query: HashSet<String> = query_words
        .iter()
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.len() > 2)
        .collect();

    if filtered_query.is_empty() {
        return 0.0;
    }

    let overlap = filtered_query
        .iter()
        .filter(|w| text_words.contains(*w))
        .count();

    overlap as f64 / filtered_query.len() as f64
}

pub fn jaccard_similarity(text_a: &str, text_b: &str, min_word_len: usize) -> f64 {
    let a = token_set(text_a, min_word_len);
    let b = token_set(text_b, min_word_len);

    if a.is_empty() && b.is_empty() {
        return 0.0;
    }

    let intersection = a.intersection(&b).count();
    let union = a.union(&b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Abstention threshold — collection-level gate on max text overlap.
/// Dense embeddings (bge-small-en-v1.5) produce 0.80+ cosine similarity even
/// for unrelated content, so vec_sim is NOT used for abstention.
/// Text overlap cleanly separates relevant (0.33+) from irrelevant (0.00–0.25).
pub const ABSTENTION_MIN_TEXT: f64 = 0.30;

/// Graph enrichment — inject 1-hop neighbors from top seed results
pub const GRAPH_NEIGHBOR_FACTOR: f64 = 0.4;
pub const GRAPH_MIN_EDGE_WEIGHT: f64 = 0.3;

/// Compute feedback dampening/boosting factor from accumulated feedback_score.
/// Negative feedback suppresses results; positive gives a mild boost (capped at 1.3×).
pub fn feedback_factor(feedback_score: i64) -> f64 {
    if feedback_score <= -3 {
        0.3 // flagged for review — heavily suppress
    } else if feedback_score < 0 {
        0.7 // negative feedback — mild suppress
    } else if feedback_score > 0 {
        (1.0 + (feedback_score as f64 * 0.05)).min(1.3)
    } else {
        1.0 // neutral
    }
}

fn token_set(text: &str, min_word_len: usize) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|word| word.len() >= min_word_len)
        .map(|word| word.to_lowercase())
        .collect()
}

fn parse_iso8601_to_unix_seconds(value: &str) -> Option<f64> {
    if !value.ends_with('Z') || value.len() < 20 {
        return None;
    }

    let year: i32 = value.get(0..4)?.parse().ok()?;
    let month: u32 = value.get(5..7)?.parse().ok()?;
    let day: u32 = value.get(8..10)?.parse().ok()?;
    let hour: u32 = value.get(11..13)?.parse().ok()?;
    let minute: u32 = value.get(14..16)?.parse().ok()?;
    let second: u32 = value.get(17..19)?.parse().ok()?;

    if value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value.as_bytes().get(10) != Some(&b'T')
        || value.as_bytes().get(13) != Some(&b':')
        || value.as_bytes().get(16) != Some(&b':')
    {
        return None;
    }

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    let mut fraction = 0.0;
    if let Some(dot_index) = value.find('.') {
        let end = value.len() - 1;
        if dot_index >= end {
            return None;
        }
        let frac_str = value.get(dot_index + 1..end)?;
        if !frac_str.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let frac_num: f64 = format!("0.{frac_str}").parse().ok()?;
        fraction = frac_num;
    }

    let days = days_from_civil(year, month as i32, day as i32);
    let day_seconds = (hour as i64 * 3600 + minute as i64 * 60 + second as i64) as f64;
    Some(days as f64 * 86_400.0 + day_seconds + fraction)
}

fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let adjusted_year = year - if month <= 2 { 1 } else { 0 };
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let yoe = adjusted_year - era * 400;
    let adjusted_month = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * adjusted_month + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iso_string_days_ago(days_ago: f64) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs_f64())
            .unwrap_or(0.0);
        let target = now - (days_ago * 86_400.0);
        unix_to_iso8601(target)
    }

    fn unix_to_iso8601(timestamp: f64) -> String {
        let total_seconds = timestamp.floor() as i64;
        let day = total_seconds.div_euclid(86_400);
        let second_of_day = total_seconds.rem_euclid(86_400);

        let (year, month, day_of_month) = civil_from_days(day);
        let hour = second_of_day / 3600;
        let minute = (second_of_day % 3600) / 60;
        let second = second_of_day % 60;

        format!("{year:04}-{month:02}-{day_of_month:02}T{hour:02}:{minute:02}:{second:02}Z")
    }

    fn civil_from_days(days: i64) -> (i32, i32, i32) {
        let z = days + 719_468;
        let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe as i32 + era as i32 * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as i32;
        let month = (mp + if mp < 10 { 3 } else { -9 }) as i32;
        let year = y + if month <= 2 { 1 } else { 0 };
        (year, month, day)
    }

    #[test]
    fn test_type_weight_known() {
        assert_eq!(type_weight("reminder"), 3.0);
    }

    #[test]
    fn test_type_weight_unknown() {
        assert_eq!(type_weight("totally_unknown"), 1.0);
    }

    #[test]
    fn test_priority_factor() {
        assert!((priority_factor(1) - 0.78).abs() < 1e-9);
        assert!((priority_factor(5) - 1.10).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_recent() {
        let now = iso_string_days_ago(0.0);
        let decay = time_decay(&now);
        assert!(decay > 0.99);
    }

    #[test]
    fn test_time_decay_old() {
        let old = iso_string_days_ago(30.0);
        let decay = time_decay(&old);
        assert!((decay - 0.5).abs() < 0.03);
    }

    #[test]
    fn test_word_overlap() {
        let ratio = word_overlap(
            &["rust", "memory", "an"],
            "Rust-based memory system with tags",
        );
        assert!((ratio - 1.0).abs() < 1e-9);

        let miss_ratio = word_overlap(&["alpha", "beta", "is"], "alpha only present");
        assert!((miss_ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_similarity() {
        let similarity = jaccard_similarity("alpha beta gamma", "beta gamma delta", 2);
        assert!((similarity - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_neutral() {
        assert!((feedback_factor(0) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_mild_suppress() {
        assert!((feedback_factor(-1) - 0.7).abs() < 1e-9);
        assert!((feedback_factor(-2) - 0.7).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_heavy_suppress() {
        assert!((feedback_factor(-3) - 0.3).abs() < 1e-9);
        assert!((feedback_factor(-100) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_positive_boost() {
        assert!((feedback_factor(1) - 1.05).abs() < 1e-9);
        assert!((feedback_factor(6) - 1.3).abs() < 1e-9);
        assert!((feedback_factor(100) - 1.3).abs() < 1e-9);
    }
}
