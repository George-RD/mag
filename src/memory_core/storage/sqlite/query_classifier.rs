/// Returns `true` when the query looks like a keyword/identifier lookup
/// that should skip ONNX embedding and vector search (FTS5 only).
///
/// Heuristics:
/// - Contains backtick-wrapped code (`` `...` ``)
/// - Looks like a file path (contains `/` with no spaces)
/// - Is a CamelCase identifier (2+ uppercase letters, no spaces)
/// - Is a snake_case identifier (contains `_`, no spaces, all lowercase)
pub(super) fn is_keyword_query(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Contains backtick-wrapped code
    if trimmed.matches('`').count() >= 2 {
        return true;
    }

    // Looks like a file path (contains `/` or `\` with no spaces)
    if !trimmed.contains(' ') && (trimmed.contains('/') || trimmed.contains('\\')) {
        return true;
    }

    // Is a CamelCase identifier (2+ uppercase letters, no spaces, alphanumeric only)
    if !trimmed.contains(' ') {
        let upper_count = trimmed.chars().filter(|c| c.is_uppercase()).count();
        if upper_count >= 2 && trimmed.chars().all(|c| c.is_alphanumeric()) {
            return true;
        }
    }

    // Is a snake_case identifier (contains `_`, no spaces, all lowercase alphanumeric + `_`)
    if !trimmed.contains(' ')
        && trimmed.contains('_')
        && trimmed
            .chars()
            .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return true;
    }

    false
}

/// Broad intent categories for incoming queries.
/// Used to adjust scoring multipliers so that, e.g., factual lookups
/// favour FTS/BM25 while conceptual queries lean on vector similarity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueryIntent {
    /// Code identifier / file-path lookup — FTS only, skip embedding.
    Keyword,
    /// Factual "who/what/when/where" lookups — precise term matching matters.
    Factual,
    /// Conceptual "why/how/explain" queries — semantic similarity matters.
    Conceptual,
    /// Everything else — balanced scoring.
    General,
}

/// Per-intent multipliers applied on top of the base `ScoringParams`.
///
/// Each field is a multiplicative factor:
///   effective_value = base_value × multiplier
///
/// `top_k_mult` scales the internal candidate oversampling (not the final
/// result limit) so that precision-oriented intents can oversample less
/// while recall-oriented intents pull in more candidates.
#[derive(Debug, Clone, Copy)]
pub(super) struct IntentProfile {
    pub vec_weight_mult: f64,
    pub fts_weight_mult: f64,
    pub word_overlap_mult: f64,
    pub top_k_mult: f64,
}

impl IntentProfile {
    /// Returns the tuned profile for the given intent.
    pub fn for_intent(intent: QueryIntent) -> Self {
        match intent {
            QueryIntent::Keyword => Self {
                vec_weight_mult: 0.0, // no vector search
                fts_weight_mult: 1.0,
                word_overlap_mult: 1.3,
                top_k_mult: 1.0,
            },
            QueryIntent::Factual => Self {
                vec_weight_mult: 1.0,
                fts_weight_mult: 1.1,
                word_overlap_mult: 1.15,
                top_k_mult: 1.0,
            },
            QueryIntent::Conceptual => Self {
                vec_weight_mult: 1.5,
                fts_weight_mult: 0.85,
                word_overlap_mult: 0.7,
                top_k_mult: 1.3,
            },
            QueryIntent::General => Self {
                vec_weight_mult: 1.0,
                fts_weight_mult: 1.0,
                word_overlap_mult: 1.0,
                top_k_mult: 1.0,
            },
        }
    }
}

/// Classify a query into a `QueryIntent` category.
///
/// Uses lightweight heuristics (no model inference):
/// 1. `Keyword` — code identifiers, file paths (delegates to `is_keyword_query`).
/// 2. `Factual` — starts with who/what/when/where/which, or asks for
///    names/numbers/dates. These benefit from exact term matching.
/// 3. `Conceptual` — starts with why/how/explain/describe, or contains
///    phrases like "relationship between", "difference between". These
///    benefit from semantic similarity.
/// 4. `General` — everything else.
pub(super) fn classify_query_intent(query: &str) -> QueryIntent {
    if is_keyword_query(query) {
        return QueryIntent::Keyword;
    }

    let lower = query.trim().to_lowercase();
    if lower.is_empty() {
        return QueryIntent::General;
    }

    // Split on first word for prefix matching.
    let first_word = lower.split_whitespace().next().unwrap_or("");

    // Factual "how many/much/old/long/often" — must be checked before the generic
    // "how" → Conceptual rule so that "How many children …" is Factual.
    if lower.starts_with("how many")
        || lower.starts_with("how much")
        || lower.starts_with("how old")
        || lower.starts_with("how long")
        || lower.starts_with("how often")
    {
        return QueryIntent::Factual;
    }

    // Conceptual signals.
    if matches!(
        first_word,
        "why" | "how" | "explain" | "describe" | "elaborate"
    ) || lower.starts_with("what is the relationship")
        || lower.starts_with("what is the difference")
        || lower.contains("difference between")
        || lower.contains("relationship between")
        || lower.contains("compared to")
        || lower.contains("pros and cons")
    {
        return QueryIntent::Conceptual;
    }

    // Factual signals.
    if matches!(
        first_word,
        "who" | "what" | "when" | "where" | "which" | "name" | "list" | "find"
    ) || lower.contains("date of")
        || lower.contains("name of")
        || lower.contains("number of")
    {
        return QueryIntent::Factual;
    }

    QueryIntent::General
}

/// Detects whether a query warrants additional retrieval candidates.
///
/// Returns a multiplier for the candidate limit:
/// - 2.0x for multi-hop indicators ("and" combined with relationship/connect/between/both)
/// - 1.5x for broad temporal queries ("last month/year", "this month", "over the past")
/// - 1.0x otherwise (no adjustment)
pub(super) fn detect_dynamic_limit_mult(query: &str) -> f64 {
    let lower = query.trim().to_lowercase();

    // Multi-hop: "and" + relationship words
    let has_and = lower.contains(" and ");
    let multi_hop_indicators = [
        "relationship",
        "connect",
        "between",
        "both",
        "related",
        "sister",
        "brother",
        "friend",
    ];
    if has_and && multi_hop_indicators.iter().any(|w| lower.contains(w)) {
        return 2.0;
    }

    // Broad temporal patterns
    let temporal_patterns = [
        "last month",
        "last year",
        "this month",
        "this year",
        "over the past",
        "in the past",
        "recent months",
        "past year",
        "past month",
        "few months",
    ];
    if temporal_patterns.iter().any(|p| lower.contains(p)) {
        return 1.5;
    }

    1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_keyword_query tests ──────────────────────────────────────────

    #[test]
    fn keyword_backtick_code() {
        assert!(is_keyword_query("`FooBar`"));
        assert!(is_keyword_query("find `my_func` usage"));
    }

    #[test]
    fn keyword_file_path() {
        assert!(is_keyword_query("src/main.rs"));
        assert!(is_keyword_query("./config.toml"));
        assert!(is_keyword_query("~/projects/foo"));
        assert!(is_keyword_query("/usr/local/bin"));
    }

    #[test]
    fn keyword_camel_case() {
        assert!(is_keyword_query("SqliteStorage"));
        assert!(is_keyword_query("McpMemoryServer"));
        assert!(!is_keyword_query("sqlite"));
    }

    #[test]
    fn keyword_snake_case() {
        assert!(is_keyword_query("query_cache_key"));
        assert!(is_keyword_query("embed_batch"));
        assert!(!is_keyword_query("query"));
    }

    #[test]
    fn keyword_natural_language_not_keyword() {
        assert!(!is_keyword_query("what framework are we using"));
        assert!(!is_keyword_query("database connection pool"));
        assert!(!is_keyword_query(""));
    }

    // ── classify_query_intent tests ─────────────────────────────────────

    #[test]
    fn intent_keyword_backtick() {
        assert_eq!(classify_query_intent("`FooBar`"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_keyword_file_path() {
        assert_eq!(classify_query_intent("src/main.rs"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_keyword_camel_case() {
        assert_eq!(classify_query_intent("SqliteStorage"), QueryIntent::Keyword);
    }

    #[test]
    fn intent_factual_who() {
        assert_eq!(
            classify_query_intent("Who created the memory system?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_what() {
        assert_eq!(
            classify_query_intent("What database does the project use?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_when() {
        assert_eq!(
            classify_query_intent("When did we deploy the fix?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_where() {
        assert_eq!(
            classify_query_intent("Where is the config file stored?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_many() {
        assert_eq!(
            classify_query_intent("How many times has she visited?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_long() {
        assert_eq!(
            classify_query_intent("How long has the project been running?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_factual_how_often() {
        assert_eq!(
            classify_query_intent("How often does she exercise?"),
            QueryIntent::Factual
        );
    }

    #[test]
    fn intent_conceptual_why() {
        assert_eq!(
            classify_query_intent("Why did we choose SQLite?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_how() {
        assert_eq!(
            classify_query_intent("How does the scoring pipeline work?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_explain() {
        assert_eq!(
            classify_query_intent("Explain the RRF fusion strategy"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_difference() {
        assert_eq!(
            classify_query_intent("What is the difference between FTS and vector search?"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_conceptual_compared_to() {
        assert_eq!(
            classify_query_intent("performance of SQLite compared to Postgres"),
            QueryIntent::Conceptual
        );
    }

    #[test]
    fn intent_general_statement() {
        assert_eq!(
            classify_query_intent("database connection pool"),
            QueryIntent::General
        );
    }

    #[test]
    fn intent_general_would() {
        assert_eq!(
            classify_query_intent("Would she want to pursue that career?"),
            QueryIntent::General
        );
    }

    #[test]
    fn intent_general_empty() {
        assert_eq!(classify_query_intent(""), QueryIntent::General);
    }

    // ── IntentProfile tests ──────────────────────────────────────────────

    #[test]
    fn intent_profile_keyword_disables_vec() {
        let profile = IntentProfile::for_intent(QueryIntent::Keyword);
        assert!((profile.vec_weight_mult - 0.0).abs() < 1e-9);
    }

    #[test]
    fn intent_profile_factual_boosts_fts() {
        let profile = IntentProfile::for_intent(QueryIntent::Factual);
        assert!(profile.fts_weight_mult > 1.0);
        assert!(profile.word_overlap_mult > 1.0);
    }

    #[test]
    fn intent_profile_conceptual_boosts_vec() {
        let profile = IntentProfile::for_intent(QueryIntent::Conceptual);
        assert!(profile.vec_weight_mult > 1.0);
        assert!(profile.fts_weight_mult < 1.0);
    }

    #[test]
    fn intent_profile_general_is_neutral() {
        let profile = IntentProfile::for_intent(QueryIntent::General);
        assert!((profile.vec_weight_mult - 1.0).abs() < 1e-9);
        assert!((profile.fts_weight_mult - 1.0).abs() < 1e-9);
        assert!((profile.word_overlap_mult - 1.0).abs() < 1e-9);
        assert!((profile.top_k_mult - 1.0).abs() < 1e-9);
    }

    // ── detect_dynamic_limit_mult tests ─────────────────────────────────

    #[test]
    fn test_dynamic_limit_mult_multi_hop() {
        assert!(
            (detect_dynamic_limit_mult("What is Amanda's sister and how are they related") - 2.0)
                .abs()
                < 1e-9
        );
        assert!(
            (detect_dynamic_limit_mult("Tell me about the relationship between Alice and Bob")
                - 2.0)
                .abs()
                < 1e-9
        );
    }

    #[test]
    fn test_dynamic_limit_mult_temporal() {
        assert!((detect_dynamic_limit_mult("What happened last month") - 1.5).abs() < 1e-9);
        assert!((detect_dynamic_limit_mult("Events over the past year") - 1.5).abs() < 1e-9);
    }

    #[test]
    fn test_dynamic_limit_mult_simple() {
        assert!((detect_dynamic_limit_mult("What is Alice's job?") - 1.0).abs() < 1e-9);
        assert!((detect_dynamic_limit_mult("Tell me about the weather") - 1.0).abs() < 1e-9);
    }
}
