use std::collections::HashSet;
use std::sync::LazyLock;

use crate::types::RetrievalHit;

// ── Stopwords ───────────────────────────────────────────────────────────

// NOTE: "not" and "no" are intentionally excluded from stopwords to preserve
// negation semantics (e.g., "did go" vs "did not go" must score differently).
static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
        "during", "before", "after", "above", "below", "between", "out", "off", "over", "under",
        "again", "further", "then", "once", "and", "but", "or", "nor", "so", "yet", "both",
        "either", "neither", "each", "every", "all", "any", "few", "more", "most", "other", "some",
        "such", "only", "own", "same", "than", "too", "very", "just", "about", "up", "down", "it",
        "its", "this", "that", "these", "those", "i", "me", "my", "we", "our", "you", "your", "he",
        "him", "his", "she", "her", "they", "them", "their", "what", "which", "who", "whom",
    ]
    .into_iter()
    .collect()
});

fn is_stopword(word: &str) -> bool {
    STOPWORDS.contains(word)
}

// ── Minimal stemmer ─────────────────────────────────────────────────────

/// Minimal suffix-stripping stemmer. Good enough for F1 scoring without
/// pulling in a full Porter stemmer dependency.
fn stem(word: &str) -> String {
    let w = word;
    if w.len() <= 3 {
        return w.to_string();
    }
    // Order matters: longest suffixes first.
    for suffix in &[
        "ingly", "edly", "tion", "sion", "ment", "ness", "able", "ible", "ical",
    ] {
        if let Some(base) = w.strip_suffix(suffix)
            && base.len() >= 3
        {
            return base.to_string();
        }
    }
    for suffix in &[
        "ing", "ous", "ful", "ive", "ize", "ise", "ity", "ary", "ory",
    ] {
        if let Some(base) = w.strip_suffix(suffix)
            && base.len() >= 3
        {
            return base.to_string();
        }
    }
    for suffix in &["ly", "ed", "er", "es", "al"] {
        if let Some(base) = w.strip_suffix(suffix)
            && base.len() >= 3
        {
            return base.to_string();
        }
    }
    if let Some(base) = w.strip_suffix('s')
        && base.len() >= 3
        && !base.ends_with('s')
    {
        return base.to_string();
    }
    w.to_string()
}

// ── Tokenization ────────────────────────────────────────────────────────

/// Normalize text into a set of stemmed, non-stopword tokens.
pub(crate) fn normalize_tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty() && word.len() > 1 && !is_stopword(word))
        .map(stem)
        .collect()
}

// ── Token-level F1 ──────────────────────────────────────────────────────

/// Compute token-level F1 between a predicted answer and the expected answer.
/// Returns (precision, recall, f1).
pub(crate) fn token_f1(predicted: &str, expected: &str) -> (f64, f64, f64) {
    let pred_tokens = normalize_tokens(predicted);
    let exp_tokens = normalize_tokens(expected);

    if pred_tokens.is_empty() && exp_tokens.is_empty() {
        return (1.0, 1.0, 1.0);
    }
    if pred_tokens.is_empty() || exp_tokens.is_empty() {
        return (0.0, 0.0, 0.0);
    }

    #[allow(clippy::cast_precision_loss)]
    let overlap = pred_tokens.intersection(&exp_tokens).count() as f64;
    #[allow(clippy::cast_precision_loss)]
    let precision = overlap / pred_tokens.len() as f64;
    #[allow(clippy::cast_precision_loss)]
    let recall = overlap / exp_tokens.len() as f64;

    if precision + recall == 0.0 {
        return (0.0, 0.0, 0.0);
    }

    let f1 = 2.0 * precision * recall / (precision + recall);
    (precision, recall, f1)
}

// ── Evidence recall ─────────────────────────────────────────────────────

/// Compute evidence recall: what fraction of the expected dia_ids were
/// found in the retrieved results' metadata.
pub(crate) fn evidence_recall(hits: &[RetrievalHit], expected_dia_ids: &[String]) -> f64 {
    if expected_dia_ids.is_empty() {
        return 1.0; // No evidence required = perfect recall.
    }

    let retrieved_ids: HashSet<&str> = hits.iter().filter_map(|hit| hit.dia_id()).collect();

    let found = expected_dia_ids
        .iter()
        .filter(|id| retrieved_ids.contains(id.as_str()))
        .count();

    #[allow(clippy::cast_precision_loss)]
    { found as f64 / expected_dia_ids.len() as f64 }
}

// ── Substring match (backward compat) ───────────────────────────────────

pub(crate) fn substring_match(hits: &[RetrievalHit], expected: &str) -> bool {
    if expected.is_empty() {
        return false;
    }
    let expected = expected.to_lowercase();
    hits.iter()
        .any(|hit| hit.content.to_lowercase().contains(expected.as_str()))
}

// ── Word-overlap score (AutoMem-compatible) ─────────────────────────────

/// Compute recall-oriented word overlap between retrieved hits and the expected
/// answer.  This mirrors the AutoMem evaluation approach: concatenate all hit
/// content + metadata dates into one text, normalize both sides, then return
/// `overlap_count / expected_token_count`.
pub(crate) fn word_overlap_score(hits: &[RetrievalHit], expected: &str) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }

    // Build a single text from all hits: content + any metadata date field.
    let mut combined = String::new();
    for hit in hits {
        if !combined.is_empty() {
            combined.push(' ');
        }
        combined.push_str(&hit.content);
        if let Some(date) = hit.metadata.get("date").and_then(|v| v.as_str()) {
            combined.push(' ');
            combined.push_str(date);
        }
    }

    let expected_tokens = normalize_tokens(expected);
    if expected_tokens.is_empty() {
        return 0.0;
    }
    let combined_tokens = normalize_tokens(&combined);

    #[allow(clippy::cast_precision_loss)]
    let overlap = expected_tokens.intersection(&combined_tokens).count() as f64;
    #[allow(clippy::cast_precision_loss)]
    { overlap / expected_tokens.len() as f64 }
}

// ── Adversarial detection ────────────────────────────────────────────────

/// Phrases that indicate the LLM correctly identified information as absent.
const ADVERSARIAL_PHRASES: &[&str] = &[
    "not mentioned",
    "no information",
    "cannot be determined",
    "no evidence",
    "not discussed",
    "not specified",
    "not provided",
    "no mention",
    "doesn't mention",
    "does not mention",
    "not available",
    "cannot determine",
    "no relevant",
    "not enough information",
    "insufficient information",
    "unanswerable",
    "not answerable",
    "cannot be answered",
];

/// Check whether an LLM-generated answer indicates the information is not
/// present in the context (correct behavior for adversarial questions).
pub(crate) fn adversarial_check(answer: &str) -> bool {
    let lower = answer.to_lowercase();
    ADVERSARIAL_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
}

// ── Unit tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tokens_basic() {
        let tokens = normalize_tokens("The quick brown fox jumps over the lazy dog");
        assert!(tokens.contains("quick"));
        assert!(tokens.contains("brown"));
        assert!(tokens.contains("fox"));
        assert!(tokens.contains("jump")); // stemmed
        assert!(tokens.contains("lazy"));
        assert!(tokens.contains("dog"));
        // Stopwords removed
        assert!(!tokens.contains("the"));
        assert!(!tokens.contains("over"));
    }

    #[test]
    fn test_normalize_tokens_punctuation() {
        let tokens = normalize_tokens("Hello, world! This is a test.");
        assert!(tokens.contains("hello"));
        assert!(tokens.contains("world"));
        assert!(tokens.contains("test"));
        assert!(!tokens.contains("this"));
        assert!(!tokens.contains("is"));
    }

    #[test]
    fn test_normalize_tokens_empty() {
        let tokens = normalize_tokens("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_normalize_tokens_only_stopwords() {
        let tokens = normalize_tokens("the a an is are was were");
        assert!(tokens.is_empty());
    }

    #[test]
    fn test_stem_basic() {
        assert_eq!(stem("running"), "runn");
        assert_eq!(stem("played"), "play");
        assert_eq!(stem("cats"), "cat");
        assert_eq!(stem("quickly"), "quick");
    }

    #[test]
    fn test_stem_short_words() {
        assert_eq!(stem("go"), "go");
        assert_eq!(stem("cat"), "cat");
    }

    #[test]
    fn test_token_f1_exact_match() {
        let (p, r, f1) = token_f1("The quick brown fox", "The quick brown fox");
        assert!((p - 1.0).abs() < 1e-9);
        assert!((r - 1.0).abs() < 1e-9);
        assert!((f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_token_f1_partial_match() {
        let (p, r, f1) = token_f1("brown fox", "The quick brown fox");
        // predicted has {brown, fox}, expected has {quick, brown, fox}
        // overlap = 2, precision = 2/2 = 1.0, recall = 2/3 ≈ 0.667
        assert!((p - 1.0).abs() < 1e-9);
        assert!((r - 2.0 / 3.0).abs() < 0.01);
        assert!(f1 > 0.7);
    }

    #[test]
    fn test_token_f1_no_match() {
        let (_, _, f1) = token_f1("completely different text", "another sentence entirely");
        assert!(f1 < 0.01);
    }

    #[test]
    fn test_token_f1_empty() {
        let (_, _, f1) = token_f1("", "expected answer");
        assert!(f1 < 0.01);

        let (p, r, f1) = token_f1("", "");
        assert!((p - 1.0).abs() < 1e-9);
        assert!((r - 1.0).abs() < 1e-9);
        assert!((f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_token_f1_case_insensitive() {
        let (_, _, f1) = token_f1("HELLO WORLD", "hello world");
        assert!((f1 - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_evidence_recall_full() {
        let hits = vec![
            RetrievalHit {
                content: "test".to_string(),
                score: 0.9,
                metadata: serde_json::json!({"dia_id": "d1"}),
            },
            RetrievalHit {
                content: "test2".to_string(),
                score: 0.8,
                metadata: serde_json::json!({"dia_id": "d2"}),
            },
        ];
        let expected = vec!["d1".to_string(), "d2".to_string()];
        assert!((evidence_recall(&hits, &expected) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_evidence_recall_partial() {
        let hits = vec![RetrievalHit {
            content: "test".to_string(),
            score: 0.9,
            metadata: serde_json::json!({"dia_id": "d1"}),
        }];
        let expected = vec!["d1".to_string(), "d2".to_string()];
        assert!((evidence_recall(&hits, &expected) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn test_evidence_recall_none() {
        let hits = vec![RetrievalHit {
            content: "test".to_string(),
            score: 0.9,
            metadata: serde_json::json!({"dia_id": "d3"}),
        }];
        let expected = vec!["d1".to_string(), "d2".to_string()];
        assert!(evidence_recall(&hits, &expected) < 0.01);
    }

    #[test]
    fn test_evidence_recall_empty_expected() {
        let hits = vec![RetrievalHit {
            content: "test".to_string(),
            score: 0.9,
            metadata: serde_json::json!({"dia_id": "d1"}),
        }];
        assert!((evidence_recall(&hits, &[]) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn test_substring_match_found() {
        let hits = vec![RetrievalHit {
            content: "The answer is forty-two".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        assert!(substring_match(&hits, "forty-two"));
    }

    #[test]
    fn test_substring_match_case_insensitive() {
        let hits = vec![RetrievalHit {
            content: "Alice went to Paris".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        assert!(substring_match(&hits, "paris"));
    }

    #[test]
    fn test_substring_match_not_found() {
        let hits = vec![RetrievalHit {
            content: "something else entirely".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        assert!(!substring_match(&hits, "paris"));
    }

    #[test]
    fn test_substring_match_empty_expected() {
        let hits = vec![RetrievalHit {
            content: "anything".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        assert!(!substring_match(&hits, ""));
    }

    #[test]
    fn test_adversarial_check_positive() {
        assert!(adversarial_check(
            "This information is not mentioned in the conversation."
        ));
        assert!(adversarial_check(
            "There is no evidence to support this claim."
        ));
        assert!(adversarial_check(
            "Based on the context, this cannot be determined."
        ));
        assert!(adversarial_check(
            "No information is available about this topic."
        ));
    }

    #[test]
    fn test_adversarial_check_negative() {
        assert!(!adversarial_check("Alice went to Paris on Tuesday."));
        assert!(!adversarial_check("The answer is 42."));
        assert!(!adversarial_check(
            "Bob mentioned he likes hiking in the mountains."
        ));
    }

    // ── word_overlap_score tests ────────────────────────────────────────

    #[test]
    fn test_word_overlap_perfect_recall() {
        let hits = vec![RetrievalHit {
            content: "Alice went to Paris last Tuesday".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        // "Paris" appears in the hit content, so recall should be 1.0
        let score = word_overlap_score(&hits, "Paris");
        assert!((score - 1.0).abs() < 1e-9, "score was {score}");
    }

    #[test]
    fn test_word_overlap_partial_recall() {
        let hits = vec![RetrievalHit {
            content: "Alice went to Paris".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        // Expected: "Paris on Tuesday" => tokens: {pari, tuesday}
        // Hit tokens include {alic, went, pari} — overlap = {pari} = 1/2
        let score = word_overlap_score(&hits, "Paris on Tuesday");
        assert!((score - 0.5).abs() < 0.01, "expected ~0.5, got {score}");
    }

    #[test]
    fn test_word_overlap_no_match() {
        let hits = vec![RetrievalHit {
            content: "something completely unrelated".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        let score = word_overlap_score(&hits, "Paris Tuesday");
        assert!(score < 0.01, "expected ~0.0, got {score}");
    }

    #[test]
    fn test_word_overlap_empty_expected() {
        let hits = vec![RetrievalHit {
            content: "some content".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        let score = word_overlap_score(&hits, "");
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_empty_hits() {
        let score = word_overlap_score(&[], "Paris Tuesday");
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_includes_metadata_date() {
        let hits = vec![RetrievalHit {
            content: "Alice went somewhere".to_string(),
            score: 0.9,
            metadata: serde_json::json!({"date": "2023-03-15", "dia_id": "d1"}),
        }];
        // "2023" should appear in combined text via metadata date
        let score = word_overlap_score(&hits, "2023");
        assert!((score - 1.0).abs() < 1e-9, "expected 1.0, got {score}");
    }

    #[test]
    fn test_word_overlap_multiple_hits() {
        let hits = vec![
            RetrievalHit {
                content: "Alice went to Paris".to_string(),
                score: 0.9,
                metadata: serde_json::json!({}),
            },
            RetrievalHit {
                content: "Bob arrived on Tuesday".to_string(),
                score: 0.8,
                metadata: serde_json::json!({}),
            },
        ];
        // Expected: "Paris Tuesday" => tokens {pari, tuesday}
        // Hit 1 has "pari", hit 2 has "tuesday" — full recall
        let score = word_overlap_score(&hits, "Paris Tuesday");
        assert!((score - 1.0).abs() < 1e-9, "expected 1.0, got {score}");
    }
}
