use std::collections::HashSet;
use std::sync::LazyLock;

use crate::types::RetrievalHit;

// ── Stopwords ───────────────────────────────────────────────────────────

// NOTE: "not" and "no" are intentionally excluded from stopwords to preserve
// negation semantics (e.g., "did go" vs "did not go" must score differently).
static STOPWORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "might", "shall", "can", "to",
        "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through", "during",
        "before", "after", "above", "below", "between", "out", "off", "over", "under", "again",
        "further", "then", "once", "and", "but", "or", "nor", "so", "yet", "both", "either",
        "neither", "each", "every", "all", "any", "few", "more", "most", "other", "some", "such",
        "only", "own", "same", "than", "too", "very", "just", "about", "up", "down", "it", "its",
        "this", "that", "these", "those", "i", "me", "my", "we", "our", "you", "your", "he", "him",
        "his", "she", "her", "they", "them", "their", "what", "which", "who", "whom",
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
    // Handle -ing with double-consonant reduction:
    //   "running" → "run" (not "runn")
    //   "swimming" → "swim" (not "swimm")
    //   "planning" → "plan" (not "plann")
    if let Some(base) = w.strip_suffix("ing")
        && base.len() >= 3
    {
        return dedup_trailing_consonant(base);
    }
    for suffix in &["ous", "ful", "ive", "ize", "ise", "ity", "ary", "ory"] {
        if let Some(base) = w.strip_suffix(suffix)
            && base.len() >= 3
        {
            return base.to_string();
        }
    }
    // Handle -ied/-ies → -y (e.g., "married" → "marry", "trophies" → "trophy").
    // Must come before the -ed/-es rules which would produce "marri"/"trophi".
    for suffix in &["ied", "ies"] {
        if let Some(base) = w.strip_suffix(suffix)
            && base.len() >= 2
        {
            return format!("{base}y");
        }
    }
    for suffix in &["ed", "ly", "er", "es", "al"] {
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

/// Check if byte is a consonant (not a vowel).
fn is_consonant(b: u8) -> bool {
    !matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// Remove doubled trailing consonant: "runn" → "run", "plann" → "plan".
fn dedup_trailing_consonant(base: &str) -> String {
    let bytes = base.as_bytes();
    let len = bytes.len();
    if len >= 2 && bytes[len - 1] == bytes[len - 2] && is_consonant(bytes[len - 1]) {
        base[..len - 1].to_string()
    } else {
        base.to_string()
    }
}

// ── Tokenization ────────────────────────────────────────────────────────

/// Normalize text into a set of stemmed, non-stopword tokens.
pub(crate) fn normalize_tokens(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            !word.is_empty()
                && (word.len() > 1 || word.chars().all(|c| c.is_ascii_digit()))
                && !is_stopword(word)
        })
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
    {
        found as f64 / expected_dia_ids.len() as f64
    }
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

// ── Date expansion ───────────────────────────────────────────────────────

/// Expand ISO date strings (e.g. "2023-05-07") into natural language tokens
/// so word-overlap can match expected answers like "May 2023" or "7 May".
/// Also expands standalone 4-digit years (1900-2099) that aren't part of an
/// ISO date, so that e.g. "back in 2022" emits "2022" as an explicit token.
fn expand_date_tokens(text: &str) -> String {
    const MONTHS: [&str; 13] = [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    let mut extra = String::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        // Check if current position starts a 4-digit sequence
        if i + 4 <= len
            && bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            // Ensure this is a word boundary (not preceded by alphanumeric)
            let at_word_start = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();

            if at_word_start
                && i + 10 <= len
                && bytes[i + 4] == b'-'
                && bytes[i + 5].is_ascii_digit()
                && bytes[i + 6].is_ascii_digit()
                && bytes[i + 7] == b'-'
                && bytes[i + 8].is_ascii_digit()
                && bytes[i + 9].is_ascii_digit()
            {
                // Full YYYY-MM-DD pattern
                let year = &text[i..i + 4];
                let month_num: usize = text[i + 5..i + 7].parse().unwrap_or(0);
                let day: usize = text[i + 8..i + 10].parse().unwrap_or(0);
                if (1..=12).contains(&month_num) && (1..=31).contains(&day) {
                    let month_name = MONTHS[month_num];
                    // Add expanded forms: "May 2023", "7 May 2023", "May", "2023"
                    extra.push(' ');
                    extra.push_str(&format!("{month_name} {year} {day} {month_name} {year}"));
                }
                i += 10;
            } else if at_word_start {
                // Standalone 4-digit year (1900-2099) not part of ISO date
                let at_word_end = i + 4 >= len || !bytes[i + 4].is_ascii_alphanumeric();
                if at_word_end {
                    let year_num: u16 = text[i..i + 4].parse().unwrap_or(0);
                    if (1900..=2099).contains(&year_num) {
                        extra.push(' ');
                        extra.push_str(&text[i..i + 4]);
                    }
                }
                i += 4;
            } else {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    extra
}

// ── Multi-hop answer preprocessing (LoCoMo official methodology) ─────────

/// Extract the answer portion before a semicolon.  LoCoMo multi-hop answers
/// often have "answer; explanation" format — only the answer part should be
/// scored (e.g., "National park; she likes the outdoors" → "National park").
fn extract_before_semicolon(answer: &str) -> &str {
    answer.split(';').next().unwrap_or(answer).trim()
}

/// Score a multi-hop answer: extract before semicolon, then compute
/// word-overlap on the cleaned answer.  LoCoMo multi-hop answers often
/// have "answer; explanation" format — scoring the explanation inflates
/// token count and hurts F1 when explanation tokens don't match.
pub(crate) fn multi_hop_word_overlap_score(hits: &[RetrievalHit], expected: &str) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    let answer_part = extract_before_semicolon(expected);
    if answer_part.is_empty() {
        return 0.0;
    }
    word_overlap_score(hits, answer_part)
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

    // Expand ISO dates to natural language for better token matching.
    let date_expansion = expand_date_tokens(&combined);
    if !date_expansion.is_empty() {
        combined.push_str(&date_expansion);
    }

    hybrid_overlap(&combined, expected)
}

// ── Word-overlap on text (for E2E scoring) ───────────────────────────────

/// Compute word-overlap recall between LLM-generated text and expected answer.
/// Reuses `normalize_tokens()` for consistency with retrieval word-overlap.
/// Applies date expansion so ISO dates in LLM output (e.g. "2023-05-07")
/// match expected natural-language dates (e.g. "May 2023").
pub(crate) fn word_overlap_on_text(generated: &str, expected: &str) -> f64 {
    let date_expansion = expand_date_tokens(generated);
    if date_expansion.is_empty() {
        hybrid_overlap(generated, expected)
    } else {
        let expanded = format!("{generated}{date_expansion}");
        hybrid_overlap(&expanded, expected)
    }
}

/// Shared hybrid overlap: stemmed-token set membership OR substring match.
/// Returns `matched_expected_tokens / total_expected_tokens`.
fn hybrid_overlap(source: &str, expected: &str) -> f64 {
    if expected.is_empty() {
        return 0.0;
    }
    let expected_tokens = normalize_tokens(expected);
    if expected_tokens.is_empty() {
        return 0.0;
    }
    let source_tokens = normalize_tokens(source);
    let source_lower = source.to_lowercase();
    #[allow(clippy::cast_precision_loss)]
    let overlap = expected_tokens
        .iter()
        .filter(|token| source_tokens.contains(*token) || source_lower.contains(token.as_str()))
        .count() as f64;
    #[allow(clippy::cast_precision_loss)]
    {
        overlap / expected_tokens.len() as f64
    }
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

/// Detect whether an expected answer is a "not mentioned" / "cannot be
/// determined" style adversarial answer.  Used in word-overlap scoring to
/// avoid matching "not" against retrieved content.
pub(crate) fn is_adversarial_expected(expected: &str) -> bool {
    adversarial_check(expected)
}

/// Score a retrieval result for an adversarial question in word-overlap mode.
///
/// In retrieval-only evaluation the system always returns its top-k results and
/// has no mechanism to abstain — there is no LLM to decide "not mentioned".
/// We therefore award full credit (1.0) to avoid penalizing the retrieval
/// pipeline for something only an LLM can detect.  Adversarial detection is
/// properly tested in the E2E and LLM-F1 scoring modes where an LLM can
/// recognize the question is unanswerable.
pub(crate) fn adversarial_retrieval_score(_hits: &[RetrievalHit]) -> f64 {
    1.0
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
        assert_eq!(stem("running"), "run");
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

    // ── word_overlap_on_text tests ─────────────────────────────────────

    #[test]
    fn test_word_overlap_on_text_exact_match() {
        let score = word_overlap_on_text("Alice went to Paris", "Alice went to Paris");
        assert!((score - 1.0).abs() < 1e-9, "expected 1.0, got {score}");
    }

    #[test]
    fn test_word_overlap_on_text_partial_match() {
        // Expected: "Paris on Tuesday" => tokens {pari, tuesday}
        // Generated: "Alice visited Paris" => tokens {alic, visit, pari}
        // Overlap: {pari} => 1/2 = 0.5
        let score = word_overlap_on_text("Alice visited Paris", "Paris on Tuesday");
        assert!((score - 0.5).abs() < 0.01, "expected ~0.5, got {score}");
    }

    #[test]
    fn test_word_overlap_on_text_no_match() {
        let score = word_overlap_on_text("something completely different", "Paris Tuesday");
        assert!(score < 0.01, "expected ~0.0, got {score}");
    }

    #[test]
    fn test_word_overlap_on_text_empty_expected() {
        let score = word_overlap_on_text("some generated text", "");
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_on_text_empty_generated() {
        let score = word_overlap_on_text("", "Paris Tuesday");
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_on_text_both_empty() {
        let score = word_overlap_on_text("", "");
        assert!(score.abs() < 1e-9);
    }

    #[test]
    fn test_word_overlap_on_text_case_insensitive() {
        let score = word_overlap_on_text("PARIS TUESDAY", "paris tuesday");
        assert!((score - 1.0).abs() < 1e-9, "expected 1.0, got {score}");
    }

    #[test]
    fn test_word_overlap_on_text_date_expansion() {
        // LLM outputs ISO date "2023-05-07", expected is "May 2023".
        // Without date expansion this would score ~0.5 (only "2023" matches).
        // With date expansion, "May" is generated from the ISO date.
        let score = word_overlap_on_text("2023-05-07", "May 2023");
        assert!(
            score > 0.99,
            "date expansion should match 'May 2023' from ISO date, got {score}"
        );
    }

    #[test]
    fn test_word_overlap_on_text_stopwords_only_expected() {
        // "the a an" are all stopwords, so expected_tokens is empty → returns 0.0
        let score = word_overlap_on_text("the a an is are", "the a an");
        assert!(score.abs() < 1e-9);
    }

    // ── single-char digit token tests ─────────────────────────────────

    #[test]
    fn test_normalize_tokens_keeps_single_digit() {
        let tokens = normalize_tokens("the answer is 2");
        assert!(tokens.contains("2"), "single digit '2' should be kept");
    }

    #[test]
    fn test_normalize_tokens_drops_single_letter() {
        let tokens = normalize_tokens("x marks the spot");
        // "x" is single-char alphabetic, should be dropped
        assert!(!tokens.contains("x"), "single letter 'x' should be dropped");
    }

    // ── expand_date_tokens tests ──────────────────────────────────────

    #[test]
    fn test_expand_date_tokens_basic() {
        let result = expand_date_tokens("2023-05-07");
        assert!(result.contains("May"), "should contain month name");
        assert!(result.contains("2023"), "should contain year");
        assert!(result.contains("7"), "should contain day");
    }

    #[test]
    fn test_expand_date_tokens_no_dates() {
        let result = expand_date_tokens("no dates here");
        assert!(result.is_empty(), "expected empty, got: {result}");
    }

    #[test]
    fn test_expand_date_tokens_embedded() {
        let result = expand_date_tokens("meeting on 2024-01-15 at noon");
        assert!(result.contains("January"), "should contain January");
        assert!(result.contains("2024"), "should contain 2024");
    }

    #[test]
    fn test_word_overlap_date_expansion() {
        let hits = vec![RetrievalHit {
            content: "Alice traveled on 2023-05-07".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        let score = word_overlap_score(&hits, "May 2023");
        assert!(
            score > 0.5,
            "date expansion should match 'May 2023', got {score}"
        );
    }

    // ── standalone year expansion tests ──────────────────────────────

    #[test]
    fn test_expand_date_tokens_standalone_year() {
        let result = expand_date_tokens("back in 2022 things changed");
        assert!(
            result.contains("2022"),
            "standalone year should be expanded, got: {result}"
        );
    }

    #[test]
    fn test_expand_date_tokens_year_at_start() {
        let result = expand_date_tokens("2022 was a big year");
        assert!(
            result.contains("2022"),
            "year at start of text should be expanded"
        );
    }

    #[test]
    fn test_expand_date_tokens_year_at_end() {
        let result = expand_date_tokens("it happened in 2022");
        assert!(
            result.contains("2022"),
            "year at end of text should be expanded"
        );
    }

    #[test]
    fn test_expand_date_tokens_not_a_year() {
        // 1800 is outside 1900-2099 range
        let result = expand_date_tokens("code 1800 here");
        assert!(
            !result.contains("1800"),
            "1800 is outside year range, got: {result}"
        );
    }

    #[test]
    fn test_expand_date_tokens_year_in_larger_number() {
        // "12022" should NOT be expanded — the 4-digit span is not at a word boundary
        let result = expand_date_tokens("id12022end");
        assert!(
            !result.contains("2022"),
            "year embedded in larger token should not expand, got: {result}"
        );
    }

    // ── adversarial expected detection tests ─────────────────────────

    #[test]
    fn test_is_adversarial_expected_not_mentioned() {
        assert!(is_adversarial_expected("Not mentioned"));
        assert!(is_adversarial_expected("not mentioned"));
        assert!(is_adversarial_expected("Not mentioned in the conversation"));
    }

    #[test]
    fn test_is_adversarial_expected_cannot_be_determined() {
        assert!(is_adversarial_expected("Cannot be determined"));
        assert!(is_adversarial_expected(
            "This cannot be determined from the context"
        ));
    }

    #[test]
    fn test_is_adversarial_expected_normal_answer() {
        assert!(!is_adversarial_expected("Paris"));
        assert!(!is_adversarial_expected("2022"));
        assert!(!is_adversarial_expected("Alice went hiking"));
    }

    // ── adversarial retrieval score tests ────────────────────────────

    #[test]
    fn test_adversarial_retrieval_score_no_hits() {
        let score = adversarial_retrieval_score(&[]);
        assert!(
            (score - 1.0).abs() < 1e-9,
            "no hits = full credit, got {score}"
        );
    }

    #[test]
    fn test_adversarial_retrieval_score_with_hits() {
        // In retrieval-only mode, adversarial questions always get 1.0
        // because the pipeline has no mechanism to abstain.
        let hits = vec![RetrievalHit {
            content: "some content".to_string(),
            score: 0.9,
            metadata: serde_json::json!({}),
        }];
        let score = adversarial_retrieval_score(&hits);
        assert!(
            (score - 1.0).abs() < 1e-9,
            "retrieval-only adversarial always 1.0, got {score}"
        );
    }
}
