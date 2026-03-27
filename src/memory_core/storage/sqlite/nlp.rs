use std::collections::HashSet;
use std::sync::LazyLock;

/// Stopwords for entity extraction from queries — words that appear capitalized
/// but are not entities (question words, auxiliaries, months, etc.)
const ENTITY_STOPWORDS: &[&str] = &[
    "What",
    "How",
    "Why",
    "When",
    "Where",
    "Which",
    "Who",
    "Whose",
    "Whom",
    "Is",
    "Are",
    "Was",
    "Were",
    "Do",
    "Does",
    "Did",
    "Has",
    "Have",
    "Had",
    "Will",
    "Would",
    "Could",
    "Should",
    "Can",
    "May",
    "Might",
    "Must",
    "Being",
    "Been",
    "Having",
    "The",
    "This",
    "That",
    "These",
    "Those",
    "It",
    "Its",
    "Yes",
    "No",
    "Not",
    "Very",
    "Also",
    "Just",
    "Even",
    "Still",
    "January",
    "February",
    "March",
    "April",
    // "May" already listed above (line 1686)
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "National",
    "American",
    "European",
    "Asian",
    "African",
    "International",
    "Global",
    "Local",
    "Regional",
    "Western",
    "Eastern",
    "Northern",
    "Southern",
    "Answer",
    "Based",
    "According",
    "Since",
    "Because",
    "Although",
    "However",
    "Therefore",
    "Furthermore",
    "Moreover",
    "Nevertheless",
    "Likely",
    "Probably",
    "Certainly",
    "Actually",
    "Recently",
    "Today",
    "Yesterday",
    "Tomorrow",
];

/// Extract capitalized proper nouns (entity names) from a query string.
///
/// Skips sentence-initial words, stopwords, and words < 2 chars.
/// Also extracts possessives (e.g., "John's" -> "John").
pub(super) fn extract_query_entities(query: &str) -> Vec<String> {
    let mut entities = Vec::new();
    let mut seen = HashSet::new();
    static STOPWORDS_SET: LazyLock<HashSet<&str>> =
        LazyLock::new(|| ENTITY_STOPWORDS.iter().copied().collect());
    let stopwords = &*STOPWORDS_SET;

    let words: Vec<&str> = query.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();

        if clean.len() < 2 {
            continue;
        }
        if stopwords.contains(clean.as_str()) {
            continue;
        }
        if i == 0 {
            continue;
        }
        if i > 0 {
            let prev = words[i - 1];
            if prev.ends_with('.') || prev.ends_with('?') || prev.ends_with('!') {
                continue;
            }
        }
        if clean.len() > 1
            && clean.chars().next().is_some_and(|c| c.is_uppercase())
            && clean.chars().skip(1).all(|c| c.is_lowercase())
            && seen.insert(clean.clone())
        {
            entities.push(clean);
        }
    }

    // Extract possessives: "Name's" -> "Name"
    for word in &words {
        if let Some(pos) = word.find("'s") {
            let name = &word[..pos];
            if name.len() >= 2
                && name.chars().next().is_some_and(|c| c.is_uppercase())
                && name.chars().skip(1).all(|c| c.is_lowercase())
                && !stopwords.contains(name)
                && seen.insert(name.to_string())
            {
                entities.push(name.to_string());
            }
        }
    }

    entities
}

/// Extract semantic topic keywords from a query, excluding entities and common words.
///
/// Returns up to 5 unique topic words that are 4+ chars and not in the skip list.
pub(super) fn extract_topic_keywords(query: &str, exclude_entities: &[String]) -> Vec<String> {
    let exclude_lower: HashSet<String> =
        exclude_entities.iter().map(|e| e.to_lowercase()).collect();

    static SKIP_WORDS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
        [
            "what",
            "when",
            "where",
            "which",
            "who",
            "whom",
            "whose",
            "how",
            "why",
            "that",
            "this",
            "these",
            "those",
            "there",
            "here",
            "then",
            "than",
            "been",
            "being",
            "have",
            "having",
            "does",
            "doing",
            "done",
            "will",
            "would",
            "could",
            "should",
            "shall",
            "might",
            "must",
            "about",
            "after",
            "also",
            "another",
            "away",
            "back",
            "because",
            "before",
            "between",
            "both",
            "came",
            "come",
            "coming",
            "each",
            "even",
            "every",
            "first",
            "from",
            "gets",
            "give",
            "given",
            "goes",
            "going",
            "gone",
            "good",
            "great",
            "into",
            "just",
            "keep",
            "kind",
            "know",
            "known",
            "last",
            "left",
            "like",
            "liked",
            "likely",
            "long",
            "look",
            "made",
            "make",
            "making",
            "many",
            "more",
            "most",
            "much",
            "need",
            "never",
            "next",
            "once",
            "only",
            "other",
            "over",
            "part",
            "past",
            "people",
            "perhaps",
            "place",
            "point",
            "probably",
            "pursue",
            "quite",
            "rather",
            "really",
            "right",
            "said",
            "same",
            "seem",
            "show",
            "side",
            "since",
            "some",
            "something",
            "sometimes",
            "still",
            "such",
            "sure",
            "take",
            "tell",
            "them",
            "they",
            "thing",
            "things",
            "think",
            "time",
            "told",
            "took",
            "turn",
            "under",
            "upon",
            "used",
            "using",
            "very",
            "want",
            "well",
            "went",
            "were",
            "while",
            "with",
            "within",
            "without",
            "work",
            "year",
            "years",
            "your",
            "able",
            "already",
            "always",
            "around",
            "called",
            "certain",
            "during",
            "either",
            "else",
            "enough",
            "ever",
            "fact",
            "find",
            "found",
            "hard",
            "help",
            "high",
            "hold",
            "home",
            "however",
            "important",
            "include",
            "indeed",
            "inside",
            "instead",
            "itself",
            "large",
            "later",
            "least",
            "less",
            "life",
            "line",
            "little",
            "live",
            "lived",
            "lives",
            "living",
            "longer",
            "mean",
            "means",
            "mention",
            "mentioned",
            "name",
            "near",
            "number",
            "often",
            "open",
            "order",
            "others",
            "outside",
            "particular",
            "play",
            "possible",
            "prefer",
            "pretty",
            "provide",
            "real",
            "reason",
            "regarding",
            "remember",
            "result",
            "seems",
            "sense",
            "several",
            "simply",
            "small",
            "sort",
            "speak",
            "start",
            "state",
            "talk",
            "together",
            "true",
            "type",
            "types",
            "usually",
            "various",
            "ways",
        ]
        .iter()
        .copied()
        .collect()
    });
    let skip_words = &*SKIP_WORDS;

    let lower = query.to_lowercase();

    let mut topics = Vec::new();
    let mut seen = HashSet::new();

    let mut current_word = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_lowercase() {
            current_word.push(ch);
        } else {
            if current_word.len() >= 4
                && !skip_words.contains(current_word.as_str())
                && !exclude_lower.contains(&current_word)
                && seen.insert(current_word.clone())
            {
                topics.push(current_word.clone());
                if topics.len() >= 5 {
                    return topics;
                }
            }
            current_word.clear();
        }
    }
    if current_word.len() >= 4
        && !skip_words.contains(current_word.as_str())
        && !exclude_lower.contains(&current_word)
        && seen.insert(current_word.clone())
    {
        topics.push(current_word);
    }

    topics
}

/// Generate decomposed sub-queries from extracted entities and topics.
///
/// Returns the original query first, then entity-based and topic-based variations.
/// Max 2 entities, max 3 topics per entity.
pub(super) fn generate_sub_queries(
    query: &str,
    entities: &[String],
    topics: &[String],
) -> Vec<String> {
    let mut queries = vec![query.to_string()];

    for entity in entities.iter().take(2) {
        queries.push(entity.clone());
        for topic in topics.iter().take(3) {
            queries.push(format!("{entity} {topic}"));
        }
        if topics.iter().any(|t| {
            matches!(
                t.as_str(),
                "career" | "jobs" | "work" | "occupation" | "employment"
            )
        }) {
            queries.push(format!("{entity} interests goals plans"));
        }
    }

    if entities.is_empty() && !topics.is_empty() {
        for topic in topics.iter().take(3) {
            queries.push(topic.clone());
        }
    }

    queries
}

/// Content fingerprint for dedup: first 320 chars, normalized, lowered, ASCII-only.
pub(super) fn content_fingerprint(content: &str) -> String {
    let cleaned: String = content
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || c.is_ascii_whitespace())
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.len() > 320 {
        collapsed[..320].to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── extract_query_entities tests ───────────────────────────────────

    #[test]
    fn test_extract_query_entities() {
        let entities = extract_query_entities("Would Caroline pursue writing?");
        assert!(
            entities.contains(&"Caroline".to_string()),
            "expected Caroline in {entities:?}"
        );
        assert!(
            !entities.contains(&"Would".to_string()),
            "should skip question word Would"
        );
    }

    #[test]
    fn test_extract_query_entities_possessive() {
        let entities = extract_query_entities("What is Amanda's sister's career?");
        assert!(
            entities.contains(&"Amanda".to_string()),
            "expected Amanda in {entities:?}"
        );
    }

    // ── extract_topic_keywords tests ───────────────────────────────────

    #[test]
    fn test_extract_topic_keywords() {
        let topics = extract_topic_keywords(
            "Would Caroline pursue writing as a career?",
            &["Caroline".to_string()],
        );
        assert!(
            topics.contains(&"writing".to_string()),
            "expected 'writing' in {topics:?}"
        );
        assert!(
            topics.contains(&"career".to_string()),
            "expected 'career' in {topics:?}"
        );
        assert!(
            !topics.contains(&"caroline".to_string()),
            "should exclude entity 'caroline'"
        );
    }

    // ── generate_sub_queries tests ─────────────────────────────────────

    #[test]
    fn test_generate_sub_queries() {
        let queries = generate_sub_queries(
            "Would Caroline pursue writing?",
            &["Caroline".to_string()],
            &["writing".to_string()],
        );
        assert_eq!(queries[0], "Would Caroline pursue writing?");
        assert!(queries.contains(&"Caroline".to_string()));
        assert!(queries.contains(&"Caroline writing".to_string()));
    }

    #[test]
    fn test_generate_sub_queries_no_entities() {
        let queries = generate_sub_queries("Tell me about writing", &[], &["writing".to_string()]);
        assert_eq!(queries[0], "Tell me about writing");
        assert!(queries.contains(&"writing".to_string()));
    }
}
