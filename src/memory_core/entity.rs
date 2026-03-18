use std::collections::BTreeSet;

/// Extracts entity tags from memory content using lightweight regex patterns.
/// Returns tags in the format `entity:person:name`, sorted for deterministic output.
///
/// This is additive-only — if extraction produces no results, the caller's
/// tags remain unchanged.
pub fn extract_entity_tags(content: &str) -> Vec<String> {
    let mut tags = BTreeSet::new();

    // Extract person names: capitalized words that appear after speaker patterns
    // like "Speaker:", "with Name", "from Name", "'s" possessives
    extract_person_names(content, &mut tags);

    tags.into_iter().collect()
}

fn extract_person_names(content: &str, tags: &mut BTreeSet<String>) {
    let words: Vec<&str> = content.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        // Skip very short words and common non-name capitalized words
        let clean = word.trim_matches(|c: char| !c.is_alphanumeric());
        if clean.len() < 2 || !clean.chars().next().is_some_and(|c| c.is_uppercase()) {
            continue;
        }
        if is_common_non_name(clean) {
            continue;
        }

        // Pattern 1: "Speaker: text" — word before colon is likely a person name
        // (is_common_non_name already checked above, no need to re-check)
        if word.ends_with(':') {
            let name = clean.to_lowercase();
            tags.insert(format!("entity:person:{name}"));
            continue;
        }

        // Pattern 2: After "with", "from", "and" + capitalized word = likely person name
        if i > 0 {
            let prev = words[i - 1].to_lowercase();
            let prev_clean = prev.trim_matches(|c: char| !c.is_alphanumeric());
            if matches!(
                prev_clean,
                "with" | "from" | "and" | "told" | "asked" | "said" | "called" | "named" | "met"
            ) {
                let name = clean.to_lowercase();
                tags.insert(format!("entity:person:{name}"));
                continue;
            }
        }

        // Pattern 3: Possessive "'s" — "Caroline's" → entity:person:caroline
        // Strips the trailing "'s" suffix rather than splitting on apostrophe,
        // so names like "O'Connor's" correctly extract "O'Connor" not "O".
        if word.ends_with("'s") || word.ends_with("\u{2019}s") {
            let name_part = if word.ends_with("'s") {
                &word[..word.len() - 2]
            } else {
                // '\u{2019}' is 3 bytes in UTF-8
                &word[..word.len() - 4]
            };
            let name_clean = name_part.trim_matches(|c: char| !c.is_alphanumeric());
            if name_clean.len() >= 2
                && name_clean.chars().next().is_some_and(|c| c.is_uppercase())
                && !is_common_non_name(name_clean)
            {
                tags.insert(format!("entity:person:{}", name_clean.to_lowercase()));
            }
        }
    }
}

/// Common words that are capitalized but are NOT person names.
///
/// Delegates to the canonical `is_stopword()` in `scoring.rs` for the base set,
/// then adds entity-extraction-specific extras (interjections, greetings, pronouns,
/// days/months, filler words) that aren't needed for BM25 scoring but cause false
/// positives in name extraction.
fn is_common_non_name(word: &str) -> bool {
    let lower = word.to_lowercase();
    // Base stopwords shared with BM25/token_set
    if crate::memory_core::scoring::is_stopword(&lower) {
        return true;
    }
    // Entity-extraction-specific extras
    matches!(
        lower.as_str(),
        // Pronouns (short, caught by is_stopword len filter but capitalized in text)
        "it" | "he" | "she" | "they" | "we" | "you"
        // Days and months
        | "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
        | "january" | "february" | "march" | "april" | "may" | "june"
        | "july" | "august" | "september" | "october" | "november" | "december"
        // Interjections and fillers
        | "oh" | "ah" | "hmm" | "huh" | "wow" | "ooh"
        | "yes" | "yeah" | "yep" | "sure" | "okay" | "well" | "right"
        | "hey" | "hello" | "bye" | "thanks" | "thank" | "please" | "sorry"
        | "great" | "good" | "nice" | "cool" | "awesome"
        // Adverbs/hedges
        | "actually" | "basically" | "literally" | "definitely" | "absolutely"
        | "maybe" | "probably" | "perhaps" | "certainly" | "honestly"
        | "really" | "quite" | "rather" | "very" | "much"
        // Adjectives/determiners
        | "new" | "old" | "first" | "last" | "next" | "other"
        | "some" | "any" | "each" | "every" | "both"
        // Misc common words
        | "being" | "been" | "shall" | "must" | "might" | "could" | "should" | "would"
        | "also" | "just" | "even" | "still" | "already"
        | "about" | "after" | "before" | "between" | "during" | "until"
        | "here" | "there" | "then" | "now" | "today" | "yesterday" | "tomorrow"
        // Domain terms
        | "speaker" | "user" | "assistant" | "system"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_speaker_pattern() {
        let tags = extract_entity_tags("Caroline: I went to the store today");
        assert!(
            tags.iter().any(|t| t == "entity:person:caroline"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_extract_possessive_pattern() {
        let tags = extract_entity_tags("I visited Caroline's house yesterday");
        assert!(
            tags.iter().any(|t| t == "entity:person:caroline"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_extract_possessive_oconnor() {
        // O'Connor's should extract "O'Connor", not just "O"
        let tags = extract_entity_tags("I went to O'Connor's pub last night");
        assert!(
            tags.iter().any(|t| t == "entity:person:o'connor"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_extract_with_pattern() {
        let tags = extract_entity_tags("I went shopping with Alice and Bob");
        assert!(
            tags.iter().any(|t| t == "entity:person:alice"),
            "tags: {:?}",
            tags
        );
        assert!(
            tags.iter().any(|t| t == "entity:person:bob"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_extract_from_pattern() {
        let tags = extract_entity_tags("Got a call from David about the project");
        assert!(
            tags.iter().any(|t| t == "entity:person:david"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_no_false_positives_on_common_words() {
        let tags = extract_entity_tags("The weather was really nice today");
        assert!(
            tags.is_empty(),
            "should not extract common words, got: {:?}",
            tags
        );
    }

    #[test]
    fn test_empty_input() {
        let tags = extract_entity_tags("");
        assert!(tags.is_empty());
    }

    #[test]
    fn test_date_prefixed_speaker() {
        let tags = extract_entity_tags("[2023-05-15] Alice: We should plan the trip");
        assert!(
            tags.iter().any(|t| t == "entity:person:alice"),
            "tags: {:?}",
            tags
        );
    }

    #[test]
    fn test_graceful_no_entities() {
        // All lowercase, no names
        let tags = extract_entity_tags("went to the store and bought groceries");
        assert!(tags.is_empty());
    }
}
