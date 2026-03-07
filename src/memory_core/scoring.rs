use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{MemoryKind, memory_kind_for_event_type};

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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScoringParams {
    pub rrf_k: f64,
    pub rrf_weight_vec: f64,
    pub rrf_weight_fts: f64,
    pub abstention_min_text: f64,
    pub graph_neighbor_factor: f64,
    pub graph_min_edge_weight: f64,
    pub word_overlap_weight: f64,
    pub jaccard_weight: f64,
    pub importance_floor: f64,
    pub importance_scale: f64,
    pub context_tag_weight: f64,
    pub time_decay_days: f64,
    pub priority_base: f64,
    pub priority_scale: f64,
    pub feedback_heavy_suppress: f64,
    pub feedback_strong_suppress: f64,
    pub feedback_positive_scale: f64,
    pub feedback_positive_cap: f64,
    pub feedback_heavy_threshold: i64,
    pub neighbor_word_overlap_weight: f64,
    pub neighbor_importance_floor: f64,
    pub neighbor_importance_scale: f64,
    pub graph_seed_min: usize,
    pub graph_seed_max: usize,
}

impl Default for ScoringParams {
    fn default() -> Self {
        Self {
            rrf_k: 60.0,
            rrf_weight_vec: RRF_WEIGHT_VEC,
            rrf_weight_fts: RRF_WEIGHT_FTS,
            abstention_min_text: ABSTENTION_MIN_TEXT,
            graph_neighbor_factor: GRAPH_NEIGHBOR_FACTOR,
            graph_min_edge_weight: GRAPH_MIN_EDGE_WEIGHT,
            word_overlap_weight: 0.75,
            jaccard_weight: 0.25,
            importance_floor: 0.3,
            importance_scale: 0.5,
            context_tag_weight: 0.25,
            time_decay_days: 0.0,
            priority_base: 0.7,
            priority_scale: 0.08,
            feedback_heavy_suppress: 0.1,
            feedback_strong_suppress: 0.3,
            feedback_positive_scale: 0.05,
            feedback_positive_cap: 1.3,
            feedback_heavy_threshold: -3,
            neighbor_word_overlap_weight: 0.5,
            neighbor_importance_floor: 0.5,
            neighbor_importance_scale: 0.5,
            graph_seed_min: 5,
            graph_seed_max: 8,
        }
    }
}

pub fn type_weight(event_type: &str) -> f64 {
    TYPE_WEIGHTS
        .iter()
        .find_map(|(kind, weight)| (*kind == event_type).then_some(*weight))
        .unwrap_or(1.0)
}

pub fn priority_factor(priority: u8, scoring_params: &ScoringParams) -> f64 {
    scoring_params.priority_base + (priority as f64 * scoring_params.priority_scale)
}

pub fn time_decay(created_at: &str, event_type: &str, scoring_params: &ScoringParams) -> f64 {
    if memory_kind_for_event_type(event_type) == MemoryKind::Semantic {
        return 1.0;
    }

    if !scoring_params.time_decay_days.is_finite() || scoring_params.time_decay_days <= 0.0 {
        return 1.0;
    }

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
    1.0 / (1.0 + (days_old / scoring_params.time_decay_days))
}

#[cfg(test)]
fn word_overlap(query_words: &[&str], text: &str) -> f64 {
    let text_words = token_set(text, 3);
    let filtered_query: HashSet<String> = query_words
        .iter()
        .map(|w| simple_stem(&w.trim().to_lowercase()))
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

/// Graph enrichment — disabled by grid search (neighbor injection hurts accuracy).
pub const GRAPH_NEIGHBOR_FACTOR: f64 = 0.0;
pub const GRAPH_MIN_EDGE_WEIGHT: f64 = 0.3;
/// Weighted RRF fusion — equal weight for vector and FTS (grid search optimal).
/// Previous bias (1.5 vec / 1.0 fts) was suboptimal on LongMemEval.
pub const RRF_WEIGHT_VEC: f64 = 1.0;
pub const RRF_WEIGHT_FTS: f64 = 1.0;

/// Feedback is an explicit user/system signal — asymmetric by design.
/// Negative feedback aggressively suppresses (explicit downvote).
/// Positive feedback gives only a mild boost (prevents displacing unrelated results).
pub fn feedback_factor(feedback_score: i64, scoring_params: &ScoringParams) -> f64 {
    if feedback_score < 0 {
        if feedback_score <= scoring_params.feedback_heavy_threshold {
            scoring_params.feedback_heavy_suppress // flagged for review — near-total suppress
        } else {
            scoring_params.feedback_strong_suppress // explicit negative — strong suppress
        }
    } else if feedback_score > 0 {
        (1.0 + (feedback_score as f64 * scoring_params.feedback_positive_scale))
            .min(scoring_params.feedback_positive_cap)
    } else {
        1.0 // neutral (no feedback = no effect)
    }
}

pub(crate) fn token_set(text: &str, min_word_len: usize) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(str::trim)
        .filter(|word| word.len() >= min_word_len)
        .map(|word| simple_stem(&word.to_lowercase()))
        .collect()
}

/// Simple suffix stemmer for English words.
///
/// Strips common suffixes to normalize inflected forms so that e.g.
/// "threading" matches "threads" (both stem to "thread").
///
/// Design constraints:
/// - Never reduces a word below 3 characters.
/// - Idempotent: stemming an already-stemmed word returns the same result.
/// - No external crates — pure string operations.
fn simple_stem(word: &str) -> String {
    // Short words are returned as-is (nothing to strip safely).
    if word.len() < 4 {
        return word.to_string();
    }

    // -ies → -y  (e.g. "memories" → "memory")
    // Check this before -s to avoid "memori" from -s stripping.
    if word.ends_with("ies") && word.len() >= 4 {
        let base_len = word.len() - 3;
        if base_len >= 3 {
            let mut result = word[..base_len].to_string();
            result.push('y');
            return result;
        }
    }

    // ── Compound suffixes (checked before their single-suffix components) ──

    // -tions (e.g. "connections" → "connec")
    if word.ends_with("tions") && word.len() - 5 >= 4 {
        return word[..word.len() - 5].to_string();
    }

    // -ments (e.g. "deployments" → "deploy")
    if word.ends_with("ments") && word.len() - 5 >= 4 {
        return word[..word.len() - 5].to_string();
    }

    // -ings (e.g. "settings" → "sett")
    if word.ends_with("ings") && word.len() - 4 >= 5 {
        return word[..word.len() - 4].to_string();
    }

    // -ers (e.g. "workers" → "work")
    if word.ends_with("ers") && word.len() - 3 >= 4 {
        return word[..word.len() - 3].to_string();
    }

    // ── Single suffixes ──

    // -tion (e.g. "connection" → "connec")
    if word.ends_with("tion") && word.len() - 4 >= 4 {
        return word[..word.len() - 4].to_string();
    }

    // -ment (e.g. "deployment" → "deploy")
    if word.ends_with("ment") && word.len() - 4 >= 4 {
        return word[..word.len() - 4].to_string();
    }

    // -ness (e.g. "darkness" → "dark")
    if word.ends_with("ness") && word.len() - 4 >= 4 {
        return word[..word.len() - 4].to_string();
    }

    // -able / -ible (e.g. "readable" → "read")
    if (word.ends_with("able") || word.ends_with("ible")) && word.len() - 4 >= 4 {
        return word[..word.len() - 4].to_string();
    }

    // -ing (e.g. "threading" → "thread", but not "ring" or "king")
    if word.ends_with("ing") && word.len() - 3 >= 5 {
        return word[..word.len() - 3].to_string();
    }

    // -est (e.g. "fastest" → "fast", but not "est" or "best")
    // Check before -ed/-er so "fastest" doesn't lose just -t.
    if word.ends_with("est") && word.len() - 3 >= 4 {
        return word[..word.len() - 3].to_string();
    }

    // -ed (e.g. "created" → "creat", but not "red" or "bed")
    if word.ends_with("ed") && word.len() - 2 >= 4 {
        return word[..word.len() - 2].to_string();
    }

    // -er (e.g. "worker" → "work", but not "her")
    if word.ends_with("er") && word.len() - 2 >= 4 {
        return word[..word.len() - 2].to_string();
    }

    // -ly (e.g. "quickly" → "quick", but not "fly")
    if word.ends_with("ly") && word.len() - 2 >= 4 {
        return word[..word.len() - 2].to_string();
    }

    // -s (e.g. "threads" → "thread", but not "is"/"as", and not -ss like "glass")
    if word.ends_with('s') && !word.ends_with("ss") && word.len() > 4 {
        return word[..word.len() - 1].to_string();
    }

    word.to_string()
}

/// Like `word_overlap`, but accepts pre-computed token sets for both query and candidate.
pub fn word_overlap_pre(query_tokens: &HashSet<String>, text_tokens: &HashSet<String>) -> f64 {
    if query_tokens.is_empty() {
        return 0.0;
    }

    let overlap = query_tokens
        .iter()
        .filter(|w| text_tokens.contains(*w))
        .count();

    overlap as f64 / query_tokens.len() as f64
}

/// Like `jaccard_similarity`, but accepts pre-computed token sets.
pub fn jaccard_pre(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }

    let intersection = a.intersection(b).count();
    let union = a.union(b).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
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
        let scoring_params = ScoringParams::default();
        assert!((priority_factor(1, &scoring_params) - 0.78).abs() < 1e-9);
        assert!((priority_factor(5, &scoring_params) - 1.10).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_recent() {
        let scoring_params = ScoringParams::default();
        let now = iso_string_days_ago(0.0);
        let decay = time_decay(&now, "session_summary", &scoring_params);
        // Default time_decay_days=0.0 → always 1.0 (no decay)
        assert!((decay - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_default_disables_decay() {
        let scoring_params = ScoringParams::default();
        let old = iso_string_days_ago(365.0);
        let decay = time_decay(&old, "task_completion", &scoring_params);
        // Default time_decay_days=0.0 → no decay even for old episodic memories
        assert!((decay - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_old() {
        let params = ScoringParams {
            time_decay_days: 30.0,
            ..ScoringParams::default()
        };
        let old = iso_string_days_ago(30.0);
        let decay = time_decay(&old, "task_completion", &params);
        assert!((decay - 0.5).abs() < 0.03);
    }

    #[test]
    fn test_time_decay_semantic_type_has_zero_decay() {
        let params = ScoringParams {
            time_decay_days: 30.0,
            ..ScoringParams::default()
        };
        let old = iso_string_days_ago(3650.0);
        let decay = time_decay(&old, "decision", &params);
        assert!((decay - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_unknown_type_defaults_to_episodic() {
        let params = ScoringParams {
            time_decay_days: 30.0,
            ..ScoringParams::default()
        };
        let old = iso_string_days_ago(30.0);
        let decay = time_decay(&old, "totally_unknown", &params);
        assert!((decay - 0.5).abs() < 0.03);
    }

    #[test]
    fn test_word_overlap() {
        let ratio = word_overlap(
            &["rust", "memory", "an"],
            "Rust-based memory system with tags",
        );
        // "an" filtered (len<=2), "rust" + "memory" match → 2/2 = 1.0
        assert!((ratio - 1.0).abs() < 1e-9);
        // partial overlap: "rust" matches, "python" doesn't → 1/2 = 0.5
        let miss_ratio = word_overlap(&["rust", "python"], "Rust-based memory system with tags");
        assert!((miss_ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_similarity() {
        let similarity = jaccard_similarity("alpha beta gamma", "beta gamma delta", 2);
        assert!((similarity - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_neutral() {
        let scoring_params = ScoringParams::default();
        assert!((feedback_factor(0, &scoring_params) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_strong_suppress() {
        let scoring_params = ScoringParams::default();
        // fb=-1 → 0.3 (strong explicit downvote)
        assert!((feedback_factor(-1, &scoring_params) - 0.3).abs() < 1e-9);
        assert!((feedback_factor(-2, &scoring_params) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_heavy_suppress() {
        let scoring_params = ScoringParams::default();
        // fb<=-3 → 0.1 (near-total suppress)
        assert!((feedback_factor(-3, &scoring_params) - 0.1).abs() < 1e-9);
        assert!((feedback_factor(-100, &scoring_params) - 0.1).abs() < 1e-9);
    }

    #[test]
    fn test_feedback_factor_positive_boost() {
        let scoring_params = ScoringParams::default();
        // fb=+1 → 1.05, fb=+2 → 1.1, fb=+6 → 1.3 (capped)
        assert!((feedback_factor(1, &scoring_params) - 1.05).abs() < 1e-9);
        assert!((feedback_factor(2, &scoring_params) - 1.1).abs() < 1e-9);
        assert!((feedback_factor(6, &scoring_params) - 1.3).abs() < 1e-9);
        assert!((feedback_factor(100, &scoring_params) - 1.3).abs() < 1e-9);
    }

    #[test]
    fn test_priority_factor_custom_params() {
        let params = ScoringParams {
            priority_base: 1.0,
            priority_scale: 0.2,
            ..ScoringParams::default()
        };
        assert!((priority_factor(5, &params) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn test_time_decay_custom_window() {
        let old = iso_string_days_ago(60.0);
        let params = ScoringParams {
            time_decay_days: 60.0,
            ..ScoringParams::default()
        };
        let decay = time_decay(&old, "task_completion", &params);
        assert!((decay - 0.5).abs() < 0.03);
    }

    #[test]
    fn test_time_decay_zero_days_returns_one() {
        let old = iso_string_days_ago(30.0);
        let params = ScoringParams {
            time_decay_days: 0.0,
            ..ScoringParams::default()
        };
        assert!((time_decay(&old, "task_completion", &params) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_pre() {
        let query_tokens = token_set("rust memory an", 3);
        let text_tokens = token_set("Rust-based memory system with tags", 3);
        // "an" filtered (len<3), "rust" + "memory" match → 2/2 = 1.0
        let ratio = word_overlap_pre(&query_tokens, &text_tokens);
        assert!((ratio - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_jaccard_pre() {
        let a = token_set("alpha beta gamma", 2);
        let b = token_set("beta gamma delta", 2);
        let similarity = jaccard_pre(&a, &b);
        assert!((similarity - 0.5).abs() < 1e-9);
    }

    // ── simple_stem tests ──────────────────────────────────────────────

    #[test]
    fn test_stem_ing() {
        assert_eq!(simple_stem("threading"), "thread");
        assert_eq!(simple_stem("processing"), "process");
        assert_eq!(simple_stem("computing"), "comput");
    }

    #[test]
    fn test_stem_ing_short_words_preserved() {
        // "ring" → only 1 char base after stripping → kept as-is
        assert_eq!(simple_stem("ring"), "ring");
        // "king" → only 1 char base → kept
        assert_eq!(simple_stem("king"), "king");
        // "bring" → 2 chars base → kept (need 5+ remaining)
        assert_eq!(simple_stem("bring"), "bring");
        // "string" has only 3 chars remaining, need 5+
        assert_eq!(simple_stem("string"), "string");
    }

    #[test]
    fn test_stem_ed() {
        assert_eq!(simple_stem("created"), "creat");
        assert_eq!(simple_stem("processed"), "process");
        assert_eq!(simple_stem("stored"), "stor");
    }

    #[test]
    fn test_stem_ed_short_words_preserved() {
        // "red" → too short for any suffix
        assert_eq!(simple_stem("red"), "red");
        // "bed" → too short
        assert_eq!(simple_stem("bed"), "bed");
        // "shed" → base "sh" is only 2 chars, need 4+
        assert_eq!(simple_stem("shed"), "shed");
        // "used" → base "us" is only 2 chars, need 4+
        assert_eq!(simple_stem("used"), "used");
    }

    #[test]
    fn test_stem_s() {
        assert_eq!(simple_stem("threads"), "thread");
        assert_eq!(simple_stem("systems"), "system");
        assert_eq!(simple_stem("memories"), "memory"); // -ies rule catches first
    }

    #[test]
    fn test_stem_s_guards() {
        // "is" and "as" too short (< 4 chars)
        assert_eq!(simple_stem("is"), "is");
        assert_eq!(simple_stem("as"), "as");
        // "-ss" words should NOT be stripped
        assert_eq!(simple_stem("glass"), "glass");
        assert_eq!(simple_stem("class"), "class");
        assert_eq!(simple_stem("moss"), "moss");
    }

    #[test]
    fn test_stem_tion() {
        assert_eq!(simple_stem("connection"), "connec");
        assert_eq!(simple_stem("collection"), "collec");
        assert_eq!(simple_stem("abstention"), "absten");
    }

    #[test]
    fn test_stem_ment() {
        assert_eq!(simple_stem("deployment"), "deploy");
        assert_eq!(simple_stem("management"), "manage");
        assert_eq!(simple_stem("environment"), "environ");
    }

    #[test]
    fn test_stem_ness() {
        assert_eq!(simple_stem("darkness"), "dark");
        assert_eq!(simple_stem("happiness"), "happi");
        assert_eq!(simple_stem("awareness"), "aware");
    }

    #[test]
    fn test_stem_ly() {
        assert_eq!(simple_stem("quickly"), "quick");
        assert_eq!(simple_stem("slowly"), "slow");
    }

    #[test]
    fn test_stem_ly_short_preserved() {
        // "fly" → too short
        assert_eq!(simple_stem("fly"), "fly");
        // "holy" → base "ho" is only 2 chars, need 4+
        assert_eq!(simple_stem("holy"), "holy");
    }

    #[test]
    fn test_stem_er() {
        assert_eq!(simple_stem("worker"), "work");
        assert_eq!(simple_stem("builder"), "build");
        assert_eq!(simple_stem("handler"), "handl");
    }

    #[test]
    fn test_stem_er_short_preserved() {
        // "her" → too short
        assert_eq!(simple_stem("her"), "her");
    }

    #[test]
    fn test_stem_est() {
        assert_eq!(simple_stem("fastest"), "fast");
        assert_eq!(simple_stem("largest"), "larg");
    }

    #[test]
    fn test_stem_est_short_preserved() {
        // "best" → base "b" only 1 char, need 4+
        assert_eq!(simple_stem("best"), "best");
        // "rest" → base "r" only 1 char
        assert_eq!(simple_stem("rest"), "rest");
    }

    #[test]
    fn test_stem_ies() {
        assert_eq!(simple_stem("memories"), "memory");
        assert_eq!(simple_stem("queries"), "query");
        assert_eq!(simple_stem("entries"), "entry");
    }

    #[test]
    fn test_stem_able_ible() {
        assert_eq!(simple_stem("readable"), "read");
        assert_eq!(simple_stem("searchable"), "search");
        assert_eq!(simple_stem("flexible"), "flex");
        assert_eq!(simple_stem("convertible"), "convert");
    }

    // ── Compound suffix tests ──────────────────────────────────────────

    #[test]
    fn test_stem_compound_ers() {
        // "workers" → "work" (same as "worker" → "work")
        assert_eq!(simple_stem("workers"), "work");
        assert_eq!(simple_stem("builders"), "build");
        assert_eq!(simple_stem("handlers"), "handl");
    }

    #[test]
    fn test_stem_compound_ings() {
        // "settings" base is only 4 chars, so -ings doesn't fire; -s strips to "setting"
        assert_eq!(simple_stem("settings"), "setting");
        // "buildings" base is 5 chars, so -ings fires → "build"
        assert_eq!(simple_stem("buildings"), "build");
        // "proceedings" base is 7 chars → "proceed"
        assert_eq!(simple_stem("proceedings"), "proceed");
    }

    #[test]
    fn test_stem_compound_tions() {
        assert_eq!(simple_stem("connections"), "connec");
        assert_eq!(simple_stem("collections"), "collec");
    }

    #[test]
    fn test_stem_compound_ments() {
        assert_eq!(simple_stem("deployments"), "deploy");
        assert_eq!(simple_stem("environments"), "environ");
    }

    #[test]
    fn test_stem_idempotent() {
        // Stemming an already-stemmed word should return the same result
        let words = [
            "thread", "process", "deploy", "dark", "quick", "work", "fast", "memory", "read",
            "search", "flex",
        ];
        for word in &words {
            let once = simple_stem(word);
            let twice = simple_stem(&once);
            assert_eq!(
                once, twice,
                "stem('{}') = '{}' but stem('{}') = '{}'",
                word, once, once, twice
            );
        }
    }

    #[test]
    fn test_stem_never_below_3_chars() {
        // Verify we never produce a result shorter than 3 characters
        // for any input that is 3+ characters.
        let words = [
            "the", "ing", "bed", "red", "ant", "are", "ate", "use", "ring", "king", "sing", "dies",
            "ties",
        ];
        for word in &words {
            let stemmed = simple_stem(word);
            assert!(
                stemmed.len() >= word.len().min(3),
                "stem('{}') = '{}' is too short",
                word,
                stemmed
            );
        }
    }

    // ── token_set stemming integration tests ───────────────────────────

    #[test]
    fn test_token_set_stems_inflections() {
        // "threading" and "threads" should both stem to "thread"
        let a = token_set("threading issues", 3);
        let b = token_set("thread issues", 3);
        assert!(a.contains("thread"), "expected 'thread' in {:?}", a);
        assert!(b.contains("thread"), "expected 'thread' in {:?}", b);
    }

    #[test]
    fn test_token_set_stemming_improves_overlap() {
        // Before stemming these wouldn't match; now they should
        let query = token_set("threading", 3);
        let text = token_set("threads are useful", 3);
        let overlap = word_overlap_pre(&query, &text);
        assert!(
            (overlap - 1.0).abs() < 1e-9,
            "expected overlap 1.0, got {}",
            overlap
        );
    }

    #[test]
    fn test_token_set_stemming_jaccard() {
        // "deploying workers quickly" vs "deployment worker quick"
        // all three content words should match after stemming
        let a = token_set("deploying workers quickly", 3);
        let b = token_set("deployment worker quick", 3);
        let j = jaccard_pre(&a, &b);
        assert!(
            (j - 1.0).abs() < 1e-9,
            "expected Jaccard 1.0, got {} (a={:?}, b={:?})",
            j,
            a,
            b,
        );
    }
}
