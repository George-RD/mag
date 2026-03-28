//! Entity extraction from memory content.
//!
//! Extracts named entities (people, tools, projects) from text using regex
//! patterns, generates `entity:{category}:{slug}` tags for storage.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Extract entity tags from text content.
///
/// Returns a vector of tags in format `entity:{category}:{slug}`.
/// Categories: people, tools, projects.
pub(super) fn extract_entities(text: &str) -> Vec<String> {
    let mut entity_tags: Vec<String> = Vec::new();
    let mut seen_slugs: HashSet<String> = HashSet::new();

    // Extract people
    for name in extract_people(text) {
        let slug = slugify(&name);
        if is_valid_entity(&slug) && seen_slugs.insert(format!("people:{slug}")) {
            entity_tags.push(format!("entity:people:{slug}"));
        }
    }

    // Extract tools
    for tool in extract_tools(text) {
        let slug = slugify(&tool);
        if is_valid_entity(&slug) && seen_slugs.insert(format!("tools:{slug}")) {
            entity_tags.push(format!("entity:tools:{slug}"));
        }
    }

    // Extract projects
    for project in extract_projects(text) {
        let slug = slugify(&project);
        if is_valid_entity(&slug) && seen_slugs.insert(format!("projects:{slug}")) {
            entity_tags.push(format!("entity:projects:{slug}"));
        }
    }

    entity_tags
}

/// Extract person names from text.
///
/// Detects:
/// - Capitalized name sequences NOT at sentence start
/// - Names after "met with", "talked to", "spoke with" patterns
fn extract_people(text: &str) -> Vec<String> {
    static PEOPLE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:met with|talked to|spoke with|meeting with|spoke to|discussed with|working with)\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+)?)").unwrap()
    });

    let mut names: Vec<String> = Vec::new();
    let mut seen = HashSet::new();

    // Pattern-based extraction (highest confidence)
    for cap in PEOPLE_PATTERN.captures_iter(text) {
        if let Some(name) = cap.get(1) {
            let n = name.as_str().to_string();
            if seen.insert(n.to_lowercase()) {
                names.push(n);
            }
        }
    }

    // Capitalized names not at sentence start
    // Split into sentences, skip first word of each
    for sentence in text.split(['.', '!', '?', '\n']) {
        let words: Vec<&str> = sentence.split_whitespace().collect();
        // Skip first word (sentence-initial capitalization)
        for (i, word) in words.iter().enumerate() {
            if i == 0 {
                continue;
            }
            // Strip trailing punctuation for matching
            let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
            if clean.len() >= 2
                && clean.chars().next().is_some_and(|c| c.is_uppercase())
                && clean.chars().skip(1).all(|c| c.is_lowercase())
                && !is_common_word(&clean)
                && seen.insert(clean.to_lowercase())
            {
                names.push(clean);
            }
        }
    }

    names
}

/// Extract tool names from text.
fn extract_tools(text: &str) -> Vec<String> {
    static TOOL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:use|using|deploy|deployed|via|with|adopting|adopted|installed|running)\s+([A-Z][\w\-]+)").unwrap()
    });

    let mut tools = Vec::new();
    let mut seen = HashSet::new();
    for cap in TOOL_PATTERN.captures_iter(text) {
        if let Some(tool) = cap.get(1) {
            let t = tool.as_str().to_string();
            if !is_common_word(&t) && seen.insert(t.to_lowercase()) {
                tools.push(t);
            }
        }
    }
    tools
}

/// Extract project names from text.
fn extract_projects(text: &str) -> Vec<String> {
    static BACKTICK_PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"`([^`]{2,30})`").unwrap());
    static QUOTED_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?:project|initiative|codebase|repo)\s*[:\-]?\s*"?([A-Za-z][\w\-]{1,29})"?"#)
            .unwrap()
    });
    static CAMEL_PATTERN: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b([A-Z][a-z]+(?:[A-Z][a-z]+)+)\b").unwrap());

    let mut projects = Vec::new();
    let mut seen = HashSet::new();

    for cap in BACKTICK_PATTERN.captures_iter(text) {
        if let Some(p) = cap.get(1) {
            let name = p.as_str().trim().to_string();
            if name.len() >= 2 && seen.insert(name.to_lowercase()) {
                projects.push(name);
            }
        }
    }
    for cap in QUOTED_PATTERN.captures_iter(text) {
        if let Some(p) = cap.get(1) {
            let name = p.as_str().to_string();
            if seen.insert(name.to_lowercase()) {
                projects.push(name);
            }
        }
    }
    for cap in CAMEL_PATTERN.captures_iter(text) {
        if let Some(p) = cap.get(0) {
            let name = p.as_str().to_string();
            if !is_common_word(&name) && seen.insert(name.to_lowercase()) {
                projects.push(name);
            }
        }
    }

    projects
}

/// Convert a name to a URL-safe slug.
pub(super) fn slugify(value: &str) -> String {
    let cleaned: String = value
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse multiple hyphens and trim
    let mut result = String::new();
    let mut prev_hyphen = true; // treat start as hyphen to trim leading
    for c in cleaned.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }
    // Trim trailing hyphen
    while result.ends_with('-') {
        result.pop();
    }
    result
}

/// Validate an entity slug.
///
/// Rules:
/// - Min 2 chars after slugification
/// - Max 6 words (hyphens count as word separators)
/// - Reject pure stopwords, code class names (*Handler/*Service), boolean literals
pub(super) fn is_valid_entity(slug: &str) -> bool {
    if slug.len() < 2 {
        return false;
    }
    let word_count = slug.split('-').filter(|w| !w.is_empty()).count();
    if word_count > 6 {
        return false;
    }
    // Reject code class name patterns
    if slug.ends_with("handler")
        || slug.ends_with("service")
        || slug.ends_with("manager")
        || slug.ends_with("controller")
        || slug.ends_with("factory")
    {
        // Only reject if it looks like a code class (e.g. "request-handler")
        if word_count >= 2 {
            return false;
        }
    }
    // Reject boolean-like values
    if matches!(slug, "true" | "false" | "null" | "none" | "undefined") {
        return false;
    }
    // Reject pure stopwords
    if matches!(
        slug,
        "the"
            | "and"
            | "for"
            | "with"
            | "this"
            | "that"
            | "from"
            | "are"
            | "was"
            | "were"
            | "has"
            | "have"
            | "had"
            | "not"
            | "but"
            | "all"
            | "can"
            | "will"
            | "just"
            | "more"
            | "some"
            | "very"
            | "also"
            | "been"
            | "would"
            | "could"
            | "should"
            | "their"
            | "there"
            | "about"
            | "which"
            | "when"
            | "what"
            | "where"
            | "how"
    ) {
        return false;
    }
    true
}

/// Expand a tag into its hierarchical prefixes.
///
/// `entity:people:alice` -> `["entity", "entity:people", "entity:people:alice"]`
#[allow(dead_code)]
pub(super) fn expand_tag_prefixes(tag: &str) -> Vec<String> {
    let parts: Vec<&str> = tag.split(':').collect();
    let mut prefixes = Vec::with_capacity(parts.len());
    let mut current = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            current.push(':');
        }
        current.push_str(part);
        prefixes.push(current.clone());
    }
    prefixes
}

/// Common words that should not be extracted as person/tool/project names.
///
/// Overlaps significantly with `scoring::is_stopword()` but is intentionally a
/// separate list: `is_stopword` targets BM25/FTS dilution (short function words)
/// while this list targets entity-extraction false positives (auxiliaries, calendar
/// names, temporal adverbs). Merging them changes entity tags and graph edges,
/// which regresses retrieval benchmarks (see #72).
fn is_common_word(word: &str) -> bool {
    let lower = word.to_lowercase();
    matches!(
        lower.as_str(),
        "the"
            | "and"
            | "for"
            | "with"
            | "this"
            | "that"
            | "from"
            | "are"
            | "was"
            | "were"
            | "has"
            | "have"
            | "had"
            | "not"
            | "but"
            | "all"
            | "can"
            | "will"
            | "just"
            | "also"
            | "been"
            | "would"
            | "could"
            | "should"
            | "there"
            | "about"
            | "which"
            | "when"
            | "what"
            | "where"
            | "how"
            | "who"
            | "whom"
            | "whose"
            | "then"
            | "than"
            | "some"
            | "many"
            | "much"
            | "other"
            | "each"
            | "every"
            | "both"
            | "few"
            | "more"
            | "most"
            | "very"
            | "such"
            | "only"
            | "january"
            | "february"
            | "march"
            | "april"
            | "may"
            | "june"
            | "july"
            | "august"
            | "september"
            | "october"
            | "november"
            | "december"
            | "monday"
            | "tuesday"
            | "wednesday"
            | "thursday"
            | "friday"
            | "saturday"
            | "sunday"
            | "today"
            | "yesterday"
            | "tomorrow"
            | "recently"
            | "however"
            | "therefore"
            | "because"
            | "although"
            | "during"
            | "before"
            | "after"
            | "since"
            | "until"
            | "while"
            | "still"
            | "even"
            | "here"
            | "now"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_people_pattern() {
        let text = "I met with Alice about the project. Then talked to Bob about deployment.";
        let people = extract_people(text);
        assert!(
            people.iter().any(|n| n == "Alice"),
            "expected Alice in {people:?}"
        );
        assert!(
            people.iter().any(|n| n == "Bob"),
            "expected Bob in {people:?}"
        );
    }

    #[test]
    fn test_extract_people_no_sentence_start() {
        let text = "The meeting went well. Alice presented the results.";
        let people = extract_people(text);
        // "The" should not be extracted (sentence start)
        assert!(
            !people.iter().any(|n| n.to_lowercase() == "the"),
            "should not extract sentence-start words"
        );
        // "Alice" at sentence start should not be extracted via the non-pattern path
        // but could still match if not first word in its sentence fragment
    }

    #[test]
    fn test_extract_tools() {
        let text = "We are using React for the frontend and deployed Redis for caching.";
        let tools = extract_tools(text);
        assert!(
            tools.iter().any(|t| t == "React"),
            "expected React in {tools:?}"
        );
        assert!(
            tools.iter().any(|t| t == "Redis"),
            "expected Redis in {tools:?}"
        );
    }

    #[test]
    fn test_extract_projects_backtick() {
        let text = "Working on `Launchpad` this sprint.";
        let projects = extract_projects(text);
        assert!(
            projects.iter().any(|p| p == "Launchpad"),
            "expected Launchpad in {projects:?}"
        );
    }

    #[test]
    fn test_extract_projects_camelcase() {
        let text = "The SuperWhisper integration is complete.";
        let projects = extract_projects(text);
        assert!(
            projects.iter().any(|p| p == "SuperWhisper"),
            "expected SuperWhisper in {projects:?}"
        );
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Alice"), "alice");
        assert_eq!(slugify("John Smith"), "john-smith");
        assert_eq!(slugify("React.js"), "react-js");
        assert_eq!(slugify("  hello  world  "), "hello-world");
    }

    #[test]
    fn test_is_valid_entity() {
        assert!(is_valid_entity("alice"));
        assert!(is_valid_entity("john-smith"));
        assert!(!is_valid_entity("a")); // too short
        assert!(!is_valid_entity("true")); // boolean
        assert!(!is_valid_entity("request-handler")); // code pattern
    }

    #[test]
    fn test_expand_tag_prefixes() {
        let prefixes = expand_tag_prefixes("entity:people:alice");
        assert_eq!(
            prefixes,
            vec!["entity", "entity:people", "entity:people:alice"]
        );
    }

    #[test]
    fn test_extract_entities_integration() {
        let text = "Met with Alice about using React on project `Launchpad`.";
        let tags = extract_entities(text);
        assert!(
            tags.contains(&"entity:people:alice".to_string()),
            "expected entity:people:alice in {tags:?}"
        );
        assert!(
            tags.contains(&"entity:tools:react".to_string()),
            "expected entity:tools:react in {tags:?}"
        );
        assert!(
            tags.contains(&"entity:projects:launchpad".to_string()),
            "expected entity:projects:launchpad in {tags:?}"
        );
    }
}
