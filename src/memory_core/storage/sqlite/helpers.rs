use super::candidate_scorer::{is_stopword, simple_stem};
use super::*;

/// Fallback timestamp used when a row's `created_at` column is missing or unparseable.
pub(super) const EPOCH_FALLBACK: &str = "1970-01-01T00:00:00.000Z";

/// Computes a u64 hash key for the query result cache.
pub(super) fn query_cache_key(query: &str, limit: usize, opts: &super::SearchOptions) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    limit.hash(&mut hasher);
    opts.event_type
        .as_ref()
        .map(|et| et.to_string())
        .hash(&mut hasher);
    opts.project.hash(&mut hasher);
    opts.session_id.hash(&mut hasher);
    opts.include_superseded.hash(&mut hasher);
    opts.importance_min.map(|f| f.to_bits()).hash(&mut hasher);
    opts.created_after.hash(&mut hasher);
    opts.created_before.hash(&mut hasher);
    opts.context_tags.hash(&mut hasher);
    opts.entity_id.hash(&mut hasher);
    opts.agent_type.hash(&mut hasher);
    opts.event_after.hash(&mut hasher);
    opts.event_before.hash(&mut hasher);
    opts.explain.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn canonicalize(content: &str) -> String {
    let stripped: String = content
        .chars()
        .filter(|c| {
            !matches!(
                c,
                '*' | '#' | '`' | '~' | '[' | ']' | '(' | ')' | '>' | '|' | '_'
            )
        })
        .collect();
    stripped
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_lowercase()
}

pub(super) fn canonical_hash(content: &str) -> String {
    let canonical = canonicalize(content);
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Extract entity slugs from tags matching the `entity:*` prefix pattern.
///
/// Given tags like `["entity:people:alice", "entity:tools:react", "locomo-test"]`,
/// returns `["entity:people:alice", "entity:tools:react"]`.
pub(super) fn extract_entities_from_tags(tags: &[String]) -> Vec<String> {
    tags.iter()
        .filter(|tag| tag.starts_with("entity:"))
        .cloned()
        .collect()
}

pub(super) fn parse_tags_from_db(raw: &str) -> Vec<String> {
    serde_json::from_str(raw).unwrap_or_default()
}

pub(super) fn parse_metadata_from_db(raw: &str) -> serde_json::Value {
    serde_json::from_str(raw)
        .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::default()))
}

/// Maximum number of synonyms to inject per query token.
/// Keeps FTS5 queries from growing too large on synonym-rich words.
const SYNONYM_CAP: usize = 3;

/// Returns synonyms for common memory-relevant words.
///
/// Each entry maps a word to its synonym group. The mapping is bidirectional:
/// every word in a group maps to all *other* words in that group.
///
/// Returns an empty slice for words without known synonyms.
fn get_synonyms(word: &str) -> &'static [&'static str] {
    match word {
        "buy" | "purchase" | "bought" => match word {
            "buy" => &["purchase", "bought"],
            "purchase" => &["buy", "bought"],
            "bought" => &["buy", "purchase"],
            _ => &[],
        },
        "movie" | "film" => match word {
            "movie" => &["film"],
            "film" => &["movie"],
            _ => &[],
        },
        "doctor" | "physician" | "dr" => match word {
            "doctor" => &["physician", "dr"],
            "physician" => &["doctor", "dr"],
            "dr" => &["doctor", "physician"],
            _ => &[],
        },
        "phone" | "telephone" | "mobile" | "cell" => match word {
            "phone" => &["telephone", "mobile", "cell"],
            "telephone" => &["phone", "mobile", "cell"],
            "mobile" => &["phone", "telephone", "cell"],
            "cell" => &["phone", "telephone", "mobile"],
            _ => &[],
        },
        "car" | "automobile" | "vehicle" => match word {
            "car" => &["automobile", "vehicle"],
            "automobile" => &["car", "vehicle"],
            "vehicle" => &["car", "automobile"],
            _ => &[],
        },
        "happy" | "glad" | "pleased" | "joyful" => match word {
            "happy" => &["glad", "pleased", "joyful"],
            "glad" => &["happy", "pleased", "joyful"],
            "pleased" => &["happy", "glad", "joyful"],
            "joyful" => &["happy", "glad", "pleased"],
            _ => &[],
        },
        "sad" | "unhappy" | "depressed" => match word {
            "sad" => &["unhappy", "depressed"],
            "unhappy" => &["sad", "depressed"],
            "depressed" => &["sad", "unhappy"],
            _ => &[],
        },
        "big" | "large" | "huge" | "enormous" => match word {
            "big" => &["large", "huge", "enormous"],
            "large" => &["big", "huge", "enormous"],
            "huge" => &["big", "large", "enormous"],
            "enormous" => &["big", "large", "huge"],
            _ => &[],
        },
        "small" | "little" | "tiny" | "mini" => match word {
            "small" => &["little", "tiny", "mini"],
            "little" => &["small", "tiny", "mini"],
            "tiny" => &["small", "little", "mini"],
            "mini" => &["small", "little", "tiny"],
            _ => &[],
        },
        "start" | "begin" | "commence" => match word {
            "start" => &["begin", "commence"],
            "begin" => &["start", "commence"],
            "commence" => &["start", "begin"],
            _ => &[],
        },
        "end" | "finish" | "complete" | "conclude" => match word {
            "end" => &["finish", "complete", "conclude"],
            "finish" => &["end", "complete", "conclude"],
            "complete" => &["end", "finish", "conclude"],
            "conclude" => &["end", "finish", "complete"],
            _ => &[],
        },
        "fast" | "quick" | "rapid" | "swift" => match word {
            "fast" => &["quick", "rapid", "swift"],
            "quick" => &["fast", "rapid", "swift"],
            "rapid" => &["fast", "quick", "swift"],
            "swift" => &["fast", "quick", "rapid"],
            _ => &[],
        },
        "slow" | "sluggish" | "gradual" => match word {
            "slow" => &["sluggish", "gradual"],
            "sluggish" => &["slow", "gradual"],
            "gradual" => &["slow", "sluggish"],
            _ => &[],
        },
        "old" | "ancient" | "elderly" | "aged" => match word {
            "old" => &["ancient", "elderly", "aged"],
            "ancient" => &["old", "elderly", "aged"],
            "elderly" => &["old", "ancient", "aged"],
            "aged" => &["old", "ancient", "elderly"],
            _ => &[],
        },
        "new" | "fresh" | "recent" | "modern" => match word {
            "new" => &["fresh", "recent", "modern"],
            "fresh" => &["new", "recent", "modern"],
            "recent" => &["new", "fresh", "modern"],
            "modern" => &["new", "fresh", "recent"],
            _ => &[],
        },
        "house" | "home" | "residence" => match word {
            "house" => &["home", "residence"],
            "home" => &["house", "residence"],
            "residence" => &["house", "home"],
            _ => &[],
        },
        "job" | "work" | "employment" | "occupation" | "career" => match word {
            "job" => &["work", "employment", "occupation", "career"],
            "work" => &["job", "employment", "occupation", "career"],
            "employment" => &["job", "work", "occupation", "career"],
            "occupation" => &["job", "work", "employment", "career"],
            "career" => &["job", "work", "employment", "occupation"],
            _ => &[],
        },
        "trip" | "travel" | "journey" | "vacation" => match word {
            "trip" => &["travel", "journey", "vacation"],
            "travel" => &["trip", "journey", "vacation"],
            "journey" => &["trip", "travel", "vacation"],
            "vacation" => &["trip", "travel", "journey"],
            _ => &[],
        },
        "food" | "meal" | "cuisine" | "dish" => match word {
            "food" => &["meal", "cuisine", "dish"],
            "meal" => &["food", "cuisine", "dish"],
            "cuisine" => &["food", "meal", "dish"],
            "dish" => &["food", "meal", "cuisine"],
            _ => &[],
        },
        "child" | "kid" | "offspring" => match word {
            "child" => &["kid", "offspring"],
            "kid" => &["child", "offspring"],
            "offspring" => &["child", "kid"],
            _ => &[],
        },
        "friend" | "buddy" | "pal" | "companion" => match word {
            "friend" => &["buddy", "pal", "companion"],
            "buddy" => &["friend", "pal", "companion"],
            "pal" => &["friend", "buddy", "companion"],
            "companion" => &["friend", "buddy", "pal"],
            _ => &[],
        },
        "money" | "cash" | "funds" | "currency" => match word {
            "money" => &["cash", "funds", "currency"],
            "cash" => &["money", "funds", "currency"],
            "funds" => &["money", "cash", "currency"],
            "currency" => &["money", "cash", "funds"],
            _ => &[],
        },
        "talk" | "speak" | "chat" | "discuss" | "conversation" => match word {
            "talk" => &["speak", "chat", "discuss", "conversation"],
            "speak" => &["talk", "chat", "discuss", "conversation"],
            "chat" => &["talk", "speak", "discuss", "conversation"],
            "discuss" => &["talk", "speak", "chat", "conversation"],
            "conversation" => &["talk", "speak", "chat", "discuss"],
            _ => &[],
        },
        "like" | "enjoy" | "prefer" | "fond" => match word {
            "like" => &["enjoy", "prefer", "fond"],
            "enjoy" => &["like", "prefer", "fond"],
            "prefer" => &["like", "enjoy", "fond"],
            "fond" => &["like", "enjoy", "prefer"],
            _ => &[],
        },
        "hate" | "dislike" | "detest" | "loathe" => match word {
            "hate" => &["dislike", "detest", "loathe"],
            "dislike" => &["hate", "detest", "loathe"],
            "detest" => &["hate", "dislike", "loathe"],
            "loathe" => &["hate", "dislike", "detest"],
            _ => &[],
        },
        "want" | "desire" | "wish" | "need" => match word {
            "want" => &["desire", "wish", "need"],
            "desire" => &["want", "wish", "need"],
            "wish" => &["want", "desire", "need"],
            "need" => &["want", "desire", "wish"],
            _ => &[],
        },
        "think" | "believe" | "consider" | "reckon" => match word {
            "think" => &["believe", "consider", "reckon"],
            "believe" => &["think", "consider", "reckon"],
            "consider" => &["think", "believe", "reckon"],
            "reckon" => &["think", "believe", "consider"],
            _ => &[],
        },
        "look" | "see" | "watch" | "observe" | "view" => match word {
            "look" => &["see", "watch", "observe", "view"],
            "see" => &["look", "watch", "observe", "view"],
            "watch" => &["look", "see", "observe", "view"],
            "observe" => &["look", "see", "watch", "view"],
            "view" => &["look", "see", "watch", "observe"],
            _ => &[],
        },
        "give" | "provide" | "offer" | "donate" => match word {
            "give" => &["provide", "offer", "donate"],
            "provide" => &["give", "offer", "donate"],
            "offer" => &["give", "provide", "donate"],
            "donate" => &["give", "provide", "offer"],
            _ => &[],
        },
        "take" | "grab" | "seize" | "accept" => match word {
            "take" => &["grab", "seize", "accept"],
            "grab" => &["take", "seize", "accept"],
            "seize" => &["take", "grab", "accept"],
            "accept" => &["take", "grab", "seize"],
            _ => &[],
        },
        "make" | "create" | "build" | "construct" => match word {
            "make" => &["create", "build", "construct"],
            "create" => &["make", "build", "construct"],
            "build" => &["make", "create", "construct"],
            "construct" => &["make", "create", "build"],
            _ => &[],
        },
        "show" | "display" | "demonstrate" | "present" | "exhibit" => match word {
            "show" => &["display", "demonstrate", "present", "exhibit"],
            "display" => &["show", "demonstrate", "present", "exhibit"],
            "demonstrate" => &["show", "display", "present", "exhibit"],
            "present" => &["show", "display", "demonstrate", "exhibit"],
            "exhibit" => &["show", "display", "demonstrate", "present"],
            _ => &[],
        },
        "tell" | "inform" | "notify" => match word {
            "tell" => &["inform", "notify"],
            "inform" => &["tell", "notify"],
            "notify" => &["tell", "inform"],
            _ => &[],
        },
        "help" | "assist" | "support" | "aid" => match word {
            "help" => &["assist", "support", "aid"],
            "assist" => &["help", "support", "aid"],
            "support" => &["help", "assist", "aid"],
            "aid" => &["help", "assist", "support"],
            _ => &[],
        },
        "move" | "relocate" | "transfer" => match word {
            "move" => &["relocate", "transfer"],
            "relocate" => &["move", "transfer"],
            "transfer" => &["move", "relocate"],
            _ => &[],
        },
        "play" | "perform" | "game" => match word {
            "play" => &["perform", "game"],
            "perform" => &["play", "game"],
            "game" => &["play", "perform"],
            _ => &[],
        },
        "run" | "execute" | "sprint" | "jog" => match word {
            "run" => &["execute", "sprint", "jog"],
            "execute" => &["run", "sprint", "jog"],
            "sprint" => &["run", "execute", "jog"],
            "jog" => &["run", "execute", "sprint"],
            _ => &[],
        },
        "eat" | "consume" | "dine" => match word {
            "eat" => &["consume", "dine"],
            "consume" => &["eat", "dine"],
            "dine" => &["eat", "consume"],
            _ => &[],
        },
        "drink" | "beverage" | "sip" => match word {
            "drink" => &["beverage", "sip"],
            "beverage" => &["drink", "sip"],
            "sip" => &["drink", "beverage"],
            _ => &[],
        },
        "sleep" | "rest" | "nap" | "slumber" => match word {
            "sleep" => &["rest", "nap", "slumber"],
            "rest" => &["sleep", "nap", "slumber"],
            "nap" => &["sleep", "rest", "slumber"],
            "slumber" => &["sleep", "rest", "nap"],
            _ => &[],
        },
        "sick" | "ill" | "unwell" => match word {
            "sick" => &["ill", "unwell"],
            "ill" => &["sick", "unwell"],
            "unwell" => &["sick", "ill"],
            _ => &[],
        },
        "pain" | "ache" | "hurt" | "sore" => match word {
            "pain" => &["ache", "hurt", "sore"],
            "ache" => &["pain", "hurt", "sore"],
            "hurt" => &["pain", "ache", "sore"],
            "sore" => &["pain", "ache", "hurt"],
            _ => &[],
        },
        "dog" | "puppy" | "canine" | "pup" => match word {
            "dog" => &["puppy", "canine", "pup"],
            "puppy" => &["dog", "canine", "pup"],
            "canine" => &["dog", "puppy", "pup"],
            "pup" => &["dog", "puppy", "canine"],
            _ => &[],
        },
        "cat" | "kitten" | "feline" => match word {
            "cat" => &["kitten", "feline"],
            "kitten" => &["cat", "feline"],
            "feline" => &["cat", "kitten"],
            _ => &[],
        },
        "book" | "novel" | "publication" => match word {
            "book" => &["novel", "publication"],
            "novel" => &["book", "publication"],
            "publication" => &["book", "novel"],
            _ => &[],
        },
        "school" | "college" | "university" | "academy" => match word {
            "school" => &["college", "university", "academy"],
            "college" => &["school", "university", "academy"],
            "university" => &["school", "college", "academy"],
            "academy" => &["school", "college", "university"],
            _ => &[],
        },
        "city" | "town" | "urban" => match word {
            "city" => &["town", "urban"],
            "town" => &["city", "urban"],
            "urban" => &["city", "town"],
            _ => &[],
        },
        "country" | "nation" | "state" => match word {
            "country" => &["nation", "state"],
            "nation" => &["country", "state"],
            "state" => &["country", "nation"],
            _ => &[],
        },
        "meet" | "encounter" | "rendezvous" => match word {
            "meet" => &["encounter", "rendezvous"],
            "encounter" => &["meet", "rendezvous"],
            "rendezvous" => &["meet", "encounter"],
            _ => &[],
        },
        "leave" | "depart" | "exit" => match word {
            "leave" => &["depart", "exit"],
            "depart" => &["leave", "exit"],
            "exit" => &["leave", "depart"],
            _ => &[],
        },
        "arrive" | "reach" | "come" => match word {
            "arrive" => &["reach", "come"],
            "reach" => &["arrive", "come"],
            "come" => &["arrive", "reach"],
            _ => &[],
        },
        "fix" | "repair" | "mend" => match word {
            "fix" => &["repair", "mend"],
            "repair" => &["fix", "mend"],
            "mend" => &["fix", "repair"],
            _ => &[],
        },
        "break" | "shatter" | "crack" | "damage" => match word {
            "break" => &["shatter", "crack", "damage"],
            "shatter" => &["break", "crack", "damage"],
            "crack" => &["break", "shatter", "damage"],
            "damage" => &["break", "shatter", "crack"],
            _ => &[],
        },
        "close" | "shut" | "near" => match word {
            "close" => &["shut", "near"],
            "shut" => &["close", "near"],
            "near" => &["close", "shut"],
            _ => &[],
        },
        "open" | "unlock" | "accessible" => match word {
            "open" => &["unlock", "accessible"],
            "unlock" => &["open", "accessible"],
            "accessible" => &["open", "unlock"],
            _ => &[],
        },
        _ => &[],
    }
}

pub(super) fn build_fts5_query(input: &str) -> String {
    let raw_tokens: Vec<&str> = input.split_whitespace().filter(|t| !t.is_empty()).collect();

    if raw_tokens.is_empty() {
        return "\"\"".to_string();
    }

    // Filter out stopwords before constructing the FTS5 query to prevent
    // common words like "the", "to", "is" from diluting BM25 scores.
    let filtered_tokens: Vec<&str> = raw_tokens
        .iter()
        .copied()
        .filter(|t| !is_stopword(&t.to_lowercase()))
        .collect();

    // If all tokens were stopwords, fall back to using the original tokens
    // so we don't produce an empty query.
    let effective_tokens = if filtered_tokens.is_empty() {
        &raw_tokens
    } else {
        &filtered_tokens
    };

    // Escape each token for FTS5 (double-quote escaping) and wrap in quotes.
    let escaped: Vec<String> = effective_tokens
        .iter()
        .map(|t| {
            let e = t.replace('"', "\"\"");
            format!("\"{e}\"")
        })
        .collect();

    // ── Synonym expansion ──
    // For each non-stopword token, look up synonyms and add them as OR terms.
    // Also apply simple_stem() to both originals and synonyms so inflected
    // forms match (e.g. "bought" stems to "bought", synonym "purchase" stems
    // to "purchas" which matches "purchased" in FTS5).
    // Capped at SYNONYM_CAP synonyms per token to avoid query explosion.
    let mut synonym_terms: Vec<String> = Vec::new();
    for token in effective_tokens {
        let lower = token.to_lowercase();
        let syns = get_synonyms(&lower);
        if syns.is_empty() {
            continue;
        }
        // Collect unique stemmed forms to avoid duplicates.
        let original_stem = simple_stem(&lower);
        for syn in syns.iter().take(SYNONYM_CAP) {
            let syn_stem = simple_stem(syn);
            // Skip if the stemmed synonym is the same as the original token
            // (would be redundant with the already-present term).
            if syn_stem == lower || syn_stem == original_stem {
                continue;
            }
            let escaped_syn = syn.replace('"', "\"\"");
            synonym_terms.push(format!("\"{escaped_syn}\""));
        }
    }

    // For 1-2 token queries, bigrams would be redundant (either a single
    // token or an exact duplicate of the full query). Just join with OR.
    if effective_tokens.len() < 3 {
        let mut parts = escaped;
        parts.extend(synonym_terms);
        return parts.join(" OR ");
    }

    // 3+ tokens: append adjacent-token bigrams as quoted phrases.
    let bigrams: Vec<String> = effective_tokens
        .windows(2)
        .map(|pair| {
            let a = pair[0].replace('"', "\"\"");
            let b = pair[1].replace('"', "\"\"");
            format!("\"{a} {b}\"")
        })
        .collect();

    let mut parts = escaped;
    parts.extend(bigrams);
    parts.extend(synonym_terms);
    parts.join(" OR ")
}

/// Encodes a slice of f32 values as little-endian bytes.
pub(super) fn resolve_priority(event_type: Option<&EventType>, priority: Option<i64>) -> u8 {
    if let Some(value) = priority
        && (1..=5).contains(&value)
    {
        return u8::try_from(value).unwrap_or(3);
    }
    event_type
        .map(|et| {
            let dp = et.default_priority();
            if dp == 0 {
                3
            } else {
                u8::try_from(dp).unwrap_or(3)
            }
        })
        .unwrap_or(3)
}

pub(super) fn normalize_for_dedup(content: &str) -> String {
    let collapsed = content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    collapsed.chars().take(150).collect()
}

/// Basic ISO 8601 validation: accepts date-only (`YYYY-MM-DD`) or
/// datetime with optional timezone (`YYYY-MM-DDThh:mm:ss…`).
/// This is intentionally lenient — it catches gross typos without
/// requiring a full RFC 3339 parser.
pub(super) fn validate_iso8601(s: &str) -> bool {
    // Minimum: "YYYY-MM-DD" (10 chars)
    if s.len() < 10 {
        return false;
    }
    let bytes = s.as_bytes();
    // First 4 chars must be digits (year)
    if !bytes[0..4].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Hyphens at positions 4 and 7
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    // Month and day digits
    if !bytes[5..7].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    if !bytes[8..10].iter().all(|b| b.is_ascii_digit()) {
        return false;
    }
    true
}

pub(super) fn escape_like_pattern(input: &str) -> String {
    let escaped = input
        .to_lowercase()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub(super) fn search_result_from_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    let raw_tags: String = row.get(2)?;
    let raw_metadata: String = row.get(4)?;
    let event_type_str: Option<String> = row.get(5).ok();
    Ok(SearchResult {
        id: row.get(0)?,
        content: row.get(1)?,
        tags: parse_tags_from_db(&raw_tags),
        importance: row.get(3)?,
        metadata: parse_metadata_from_db(&raw_metadata),
        event_type: event_type_from_sql(event_type_str),
        session_id: row.get(6).ok(),
        project: row.get(7).ok(),
        entity_id: row.get(8).ok(),
        agent_type: row.get(9).ok(),
    })
}

/// Converts an `Option<EventType>` to an `Option<String>` for SQL parameter binding.
pub(super) fn event_type_to_sql(et: &Option<EventType>) -> Option<String> {
    et.as_ref().map(|e| e.to_string())
}

/// Parses an `Option<String>` from the DB into an `Option<EventType>`.
pub(super) fn event_type_from_sql(s: Option<String>) -> Option<EventType> {
    EventType::from_optional(s.as_deref())
}

/// Appends WHERE-clause fragments for every non-None field in `opts` to `sql`,
/// pushing corresponding values into `params`. `idx` is the next `?N` placeholder
/// index and is updated in place. `col_prefix` is prepended to column names
/// (e.g. `"m."` for joined queries, `""` for direct table queries).
pub(super) fn append_search_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    opts: &SearchOptions,
    col_prefix: &str,
) {
    use rusqlite::types::Value as SqlValue;

    if let Some(ref event_type) = opts.event_type {
        sql.push_str(&format!(" AND {}event_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(event_type.to_string()));
        *idx += 1;
    }
    if let Some(ref project) = opts.project {
        sql.push_str(&format!(" AND {}project = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(project.clone()));
        *idx += 1;
    }
    if let Some(ref session_id) = opts.session_id {
        sql.push_str(&format!(" AND {}session_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(session_id.clone()));
        *idx += 1;
    }
    if let Some(ref entity_id) = opts.entity_id {
        sql.push_str(&format!(" AND {}entity_id = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(entity_id.clone()));
        *idx += 1;
    }
    if let Some(ref agent_type) = opts.agent_type {
        sql.push_str(&format!(" AND {}agent_type = ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(agent_type.clone()));
        *idx += 1;
    }
    if let Some(importance_min) = opts.importance_min {
        sql.push_str(&format!(" AND {}importance >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Real(importance_min));
        *idx += 1;
    }
    if let Some(ref created_after) = opts.created_after {
        sql.push_str(&format!(" AND {}created_at >= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_after.clone()));
        *idx += 1;
    }
    if let Some(ref created_before) = opts.created_before {
        sql.push_str(&format!(" AND {}created_at <= ?{}", col_prefix, *idx));
        params.push(SqlValue::Text(created_before.clone()));
        *idx += 1;
    }
    if let Some(ref event_after) = opts.event_after {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') >= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_after.clone()));
        *idx += 1;
    }
    if let Some(ref event_before) = opts.event_before {
        sql.push_str(&format!(
            " AND COALESCE({}event_at, '{}') <= ?{}",
            col_prefix, EPOCH_FALLBACK, *idx
        ));
        params.push(SqlValue::Text(event_before.clone()));
        *idx += 1;
    }
}

pub(super) fn append_context_tag_filters(
    sql: &mut String,
    params: &mut Vec<rusqlite::types::Value>,
    idx: &mut usize,
    context_tags: Option<&[String]>,
    tags_expr: &str,
) {
    use rusqlite::types::Value as SqlValue;

    for tag in context_tags
        .into_iter()
        .flatten()
        .map(|tag| tag.trim())
        .filter(|tag| !tag.is_empty())
    {
        sql.push_str(&format!(
            " AND ((json_valid({tags_expr}) AND EXISTS (SELECT 1 FROM json_each({tags_expr}) WHERE lower(value) = lower(?{idx}))) \
               OR (NOT json_valid({tags_expr}) AND {tags_expr} != '' AND instr(',' || lower({tags_expr}) || ',', ',' || lower(?{idx}) || ',') > 0))"
        ));
        params.push(SqlValue::Text(tag.to_owned()));
        *idx += 1;
    }
}

/// Converts a `Vec<SqlValue>` to a `Vec<&dyn ToSql>` for rusqlite parameter binding.
pub(super) fn to_param_refs(values: &[rusqlite::types::Value]) -> Vec<&dyn rusqlite::types::ToSql> {
    values
        .iter()
        .map(|v| v as &dyn rusqlite::types::ToSql)
        .collect()
}

pub(super) fn matches_search_options(
    candidate: &RankedSemanticCandidate,
    opts: &SearchOptions,
) -> bool {
    if let Some(ref event_type) = opts.event_type
        && candidate.result.event_type.as_ref() != Some(event_type)
    {
        return false;
    }
    if let Some(project) = opts.project.as_deref()
        && candidate.result.project.as_deref() != Some(project)
    {
        return false;
    }
    if let Some(session_id) = opts.session_id.as_deref()
        && candidate.result.session_id.as_deref() != Some(session_id)
    {
        return false;
    }
    if let Some(importance_min) = opts.importance_min
        && candidate.result.importance < importance_min
    {
        return false;
    }
    if let Some(created_after) = opts.created_after.as_deref()
        && candidate.created_at.as_str() < created_after
    {
        return false;
    }
    if let Some(created_before) = opts.created_before.as_deref()
        && candidate.created_at.as_str() > created_before
    {
        return false;
    }
    if let Some(entity_id) = opts.entity_id.as_deref()
        && candidate.entity_id.as_deref() != Some(entity_id)
    {
        return false;
    }
    if let Some(agent_type) = opts.agent_type.as_deref()
        && candidate.agent_type.as_deref() != Some(agent_type)
    {
        return false;
    }
    if let Some(event_after) = opts.event_after.as_deref()
        && candidate.event_at.as_str() < event_after
    {
        return false;
    }
    if let Some(event_before) = opts.event_before.as_deref()
        && candidate.event_at.as_str() > event_before
    {
        return false;
    }
    if let Some(context_tags) = opts.context_tags.as_ref() {
        let mut normalized_tags = context_tags
            .iter()
            .map(|tag| tag.trim())
            .filter(|tag| !tag.is_empty())
            .peekable();
        if normalized_tags.peek().is_some()
            && !normalized_tags.all(|tag| {
                candidate
                    .result
                    .tags
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(tag))
            })
        {
            return false;
        }
    }
    true
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_distance_to_similarity(distance: f64) -> f64 {
    (1.0 - distance).max(0.0)
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_delete(conn: &rusqlite::Connection, memory_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM vec_memories WHERE memory_id = ?1",
        params![memory_id],
    )
    .context("failed to delete from vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_upsert(
    conn: &rusqlite::Connection,
    memory_id: &str,
    embedding: &[u8],
) -> Result<()> {
    // vec0 virtual tables don't support INSERT OR REPLACE; delete first then insert.
    vec_delete(conn, memory_id)?;
    conn.execute(
        "INSERT INTO vec_memories(memory_id, embedding) VALUES (?1, ?2)",
        params![memory_id, embedding],
    )
    .context("failed to upsert into vec_memories")?;
    Ok(())
}

#[cfg(feature = "sqlite-vec")]
pub(super) fn vec_knn_search(
    conn: &rusqlite::Connection,
    query_embedding: &[f32],
    k: usize,
) -> Result<Vec<(String, f64)>> {
    let embedding_blob = encode_embedding(query_embedding);
    let k = k as i64;
    let mut stmt = conn
        .prepare("SELECT memory_id, distance FROM vec_memories WHERE embedding MATCH ?1 AND k = ?2")
        .context("failed to prepare vec KNN query")?;
    let rows = stmt
        .query_map(params![embedding_blob, k], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })
        .context("failed to execute vec KNN query")?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.context("failed to decode vec KNN row")?);
    }
    Ok(results)
}

/// Maximum number of memory IDs hydrated per SQLite `IN (...)` batch.
#[cfg(feature = "sqlite-vec")]
pub(super) const HYDRATE_ID_CHUNK_SIZE: usize = 900;

#[cfg(feature = "sqlite-vec")]
#[derive(Debug, Clone)]
/// Fully decoded memory row used by sqlite-vec search paths after batched lookup.
pub(super) struct HydratedMemoryRow {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub metadata: serde_json::Value,
    pub event_type: Option<EventType>,
    pub session_id: Option<String>,
    pub project: Option<String>,
    pub priority: Option<i64>,
    pub created_at: String,
    pub entity_id: Option<String>,
    pub agent_type: Option<String>,
    pub event_at: String,
}

#[cfg(feature = "sqlite-vec")]
/// Hydrates memory rows for an ordered ID list while applying the same filters as search paths.
///
/// The returned map is keyed by memory ID so callers can preserve their own ranking order.
pub(super) fn hydrate_memories_by_ids(
    conn: &rusqlite::Connection,
    ids: &[String],
    include_superseded: bool,
    opts: Option<&SearchOptions>,
    include_context_tags: bool,
) -> Result<HashMap<String, HydratedMemoryRow>> {
    use rusqlite::types::Value as SqlValue;

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut hydrated = HashMap::with_capacity(ids.len());

    for chunk in ids.chunks(HYDRATE_ID_CHUNK_SIZE) {
        let mut sql = String::from(
            "SELECT id, content, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at
             FROM memories
             WHERE id IN (",
        );
        let mut params: Vec<SqlValue> = Vec::with_capacity(chunk.len() + 12);
        for (idx, memory_id) in chunk.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            sql.push_str(&format!("?{}", idx + 1));
            params.push(SqlValue::Text(memory_id.clone()));
        }
        sql.push(')');

        let mut next_idx = chunk.len() + 1;
        if !include_superseded {
            sql.push_str(" AND superseded_by_id IS NULL");
        }
        if let Some(search_opts) = opts {
            append_search_filters(&mut sql, &mut params, &mut next_idx, search_opts, "");
            if include_context_tags {
                append_context_tag_filters(
                    &mut sql,
                    &mut params,
                    &mut next_idx,
                    search_opts.context_tags.as_deref(),
                    "tags",
                );
            }
        }

        let mut stmt = conn
            .prepare(&sql)
            .context("failed to prepare batched memory hydration query")?;
        let param_refs = to_param_refs(&params);
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(HydratedMemoryRow {
                    id: row.get::<_, String>(0)?,
                    content: row.get::<_, String>(1)?,
                    tags: parse_tags_from_db(&row.get::<_, String>(2)?),
                    importance: row.get::<_, f64>(3)?,
                    metadata: parse_metadata_from_db(&row.get::<_, String>(4)?),
                    event_type: event_type_from_sql(row.get::<_, Option<String>>(5).ok().flatten()),
                    session_id: row.get::<_, Option<String>>(6).ok().flatten(),
                    project: row.get::<_, Option<String>>(7).ok().flatten(),
                    priority: row.get::<_, Option<i64>>(8).ok().flatten(),
                    created_at: row
                        .get::<_, String>(9)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                    entity_id: row.get::<_, Option<String>>(10).ok().flatten(),
                    agent_type: row.get::<_, Option<String>>(11).ok().flatten(),
                    event_at: row
                        .get::<_, String>(12)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                })
            })
            .context("failed to execute batched memory hydration query")?;

        for row in rows {
            let row = row.context("failed to decode hydrated memory row")?;
            hydrated.insert(row.id.clone(), row);
        }
    }

    Ok(hydrated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_query_empty_input() {
        assert_eq!(build_fts5_query(""), "\"\"");
        assert_eq!(build_fts5_query("   "), "\"\"");
    }

    #[test]
    fn fts5_query_single_token() {
        assert_eq!(build_fts5_query("database"), "\"database\"");
    }

    #[test]
    fn fts5_query_two_tokens_no_bigrams() {
        // Two tokens: bigrams would duplicate the full query, so skip them.
        assert_eq!(
            build_fts5_query("database connection"),
            "\"database\" OR \"connection\""
        );
    }

    #[test]
    fn fts5_query_three_tokens_with_bigrams() {
        let q = build_fts5_query("database connection pool");
        // Original tokens + bigrams, no synonyms for these words
        assert!(q.starts_with("\"database\" OR \"connection\" OR \"pool\""));
        assert!(q.contains("\"database connection\""));
        assert!(q.contains("\"connection pool\""));
    }

    #[test]
    fn fts5_query_four_tokens_with_bigrams() {
        // "the" is a stopword and gets filtered; remaining 3 tokens get bigrams
        // "quick" has synonyms: fast, rapid, swift
        let q = build_fts5_query("the quick brown fox");
        assert!(q.contains("\"quick\""));
        assert!(q.contains("\"brown\""));
        assert!(q.contains("\"fox\""));
        assert!(q.contains("\"quick brown\""));
        assert!(q.contains("\"brown fox\""));
        // Synonym expansion for "quick"
        assert!(q.contains("\"fast\""));
        assert!(q.contains("\"rapid\""));
        assert!(q.contains("\"swift\""));
    }

    #[test]
    fn fts5_query_special_chars_escaped() {
        // Double-quotes in tokens are escaped by doubling.
        assert_eq!(
            build_fts5_query("say \"hello\" world"),
            "\"say\" OR \"\"\"hello\"\"\" OR \"world\" \
             OR \"say \"\"hello\"\"\" OR \"\"\"hello\"\" world\""
        );
    }

    #[test]
    fn fts5_query_extra_whitespace_collapsed() {
        assert_eq!(
            build_fts5_query("  database   connection   pool  "),
            "\"database\" OR \"connection\" OR \"pool\" \
             OR \"database connection\" OR \"connection pool\""
        );
    }

    // ── FTS5 stopword filtering tests ─────────────────────────────────

    #[test]
    fn fts5_query_filters_stopwords() {
        // "to" and "the" are stopwords; only "path" and "database" should remain
        assert_eq!(
            build_fts5_query("path to the database"),
            "\"path\" OR \"database\""
        );
    }

    #[test]
    fn fts5_query_all_stopwords_fallback() {
        // When all tokens are stopwords, fall back to original tokens (with bigrams for 3+)
        assert_eq!(
            build_fts5_query("is it the"),
            "\"is\" OR \"it\" OR \"the\" OR \"is it\" OR \"it the\""
        );
    }

    #[test]
    fn fts5_query_stopwords_with_bigrams() {
        // "how to deploy the application" → stopwords "how", "to", "the" removed
        // → "deploy", "application" (2 tokens, no bigrams, no synonyms)
        assert_eq!(
            build_fts5_query("how to deploy the application"),
            "\"deploy\" OR \"application\""
        );
    }

    // ── Synonym expansion tests ──────────────────────────────────────

    #[test]
    fn synonym_map_bidirectional() {
        // Every word in a synonym group should map back to the others
        let groups: &[&[&str]] = &[
            &["buy", "purchase", "bought"],
            &["movie", "film"],
            &["doctor", "physician", "dr"],
            &["car", "automobile", "vehicle"],
            &["house", "home", "residence"],
            &["dog", "puppy", "canine", "pup"],
        ];
        for group in groups {
            for &word in *group {
                let syns = get_synonyms(word);
                assert!(
                    !syns.is_empty(),
                    "get_synonyms({word:?}) returned empty slice"
                );
                // Every other word in the group should be a synonym
                for &other in *group {
                    if other == word {
                        continue;
                    }
                    assert!(
                        syns.contains(&other),
                        "get_synonyms({word:?}) missing {other:?}, got {syns:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn synonym_map_no_match() {
        assert!(get_synonyms("database").is_empty());
        assert!(get_synonyms("connection").is_empty());
        assert!(get_synonyms("xyzzy").is_empty());
    }

    #[test]
    fn fts5_query_single_token_with_synonym() {
        // "movie" has synonym "film"
        let q = build_fts5_query("movie");
        assert!(q.contains("\"movie\""), "missing original: {q}");
        assert!(q.contains("\"film\""), "missing synonym 'film': {q}");
    }

    #[test]
    fn fts5_query_two_tokens_with_synonyms() {
        // "buy car" → "buy" has synonyms (purchase, bought), "car" has synonyms (automobile, vehicle)
        let q = build_fts5_query("buy car");
        assert!(q.contains("\"buy\""), "missing original 'buy': {q}");
        assert!(q.contains("\"car\""), "missing original 'car': {q}");
        assert!(
            q.contains("\"purchase\""),
            "missing synonym 'purchase': {q}"
        );
        assert!(q.contains("\"bought\""), "missing synonym 'bought': {q}");
        assert!(
            q.contains("\"automobile\""),
            "missing synonym 'automobile': {q}"
        );
        assert!(q.contains("\"vehicle\""), "missing synonym 'vehicle': {q}");
    }

    #[test]
    fn fts5_query_synonym_cap_respected() {
        // "phone" has 3 synonyms: telephone, mobile, cell — all should appear (cap=3)
        let q = build_fts5_query("phone");
        assert!(q.contains("\"phone\""));
        // Count synonym terms (all should be present since cap is 3)
        let synonym_count = ["telephone", "mobile", "cell"]
            .iter()
            .filter(|s| q.contains(&format!("\"{s}\"")))
            .count();
        assert!(
            synonym_count <= SYNONYM_CAP,
            "got {synonym_count} synonyms, expected at most {SYNONYM_CAP}"
        );
    }

    #[test]
    fn fts5_query_no_synonym_for_non_synonym_words() {
        // "database" has no synonyms, query should be unchanged
        assert_eq!(build_fts5_query("database"), "\"database\"");
    }

    #[test]
    fn fts5_query_synonym_deduplication() {
        // Synonyms whose stems match the original token should be skipped.
        // This is a general property test — no synonym should produce a
        // quoted term identical to the original token.
        let q = build_fts5_query("help");
        // "help" synonyms: assist, support, aid — all different stems
        assert!(q.contains("\"help\""));
        assert!(q.contains("\"assist\""));
        assert!(q.contains("\"support\""));
        assert!(q.contains("\"aid\""));
    }

    #[test]
    fn fts5_query_synonym_with_bigrams() {
        // 3+ tokens where some have synonyms
        // "buy fast car" → "buy"(purchase,bought), "fast"(quick,rapid,swift), "car"(automobile,vehicle)
        let q = build_fts5_query("buy fast car");
        // Original tokens
        assert!(q.contains("\"buy\""), "missing 'buy': {q}");
        assert!(q.contains("\"fast\""), "missing 'fast': {q}");
        assert!(q.contains("\"car\""), "missing 'car': {q}");
        // Bigrams
        assert!(q.contains("\"buy fast\""), "missing bigram 'buy fast': {q}");
        assert!(q.contains("\"fast car\""), "missing bigram 'fast car': {q}");
        // Synonyms
        assert!(
            q.contains("\"purchase\""),
            "missing synonym 'purchase': {q}"
        );
        assert!(q.contains("\"quick\""), "missing synonym 'quick': {q}");
        assert!(
            q.contains("\"automobile\""),
            "missing synonym 'automobile': {q}"
        );
    }

    #[test]
    fn test_extract_entities_from_tags() {
        let tags = vec![
            "entity:people:alice".to_string(),
            "entity:tools:react".to_string(),
            "locomo-test".to_string(),
            "session:1".to_string(),
        ];
        let entities = extract_entities_from_tags(&tags);
        assert_eq!(entities.len(), 2);
        assert!(entities.contains(&"entity:people:alice".to_string()));
        assert!(entities.contains(&"entity:tools:react".to_string()));
    }

    #[test]
    fn test_extract_entities_from_tags_empty() {
        let tags: Vec<String> = vec!["locomo-test".to_string()];
        let entities = extract_entities_from_tags(&tags);
        assert!(entities.is_empty());
    }

    #[test]
    fn fts5_query_stopword_synonym_interaction() {
        // Stopwords are filtered first; synonyms only apply to non-stopwords
        // "the movie" → "the" filtered → "movie" with synonym "film"
        let q = build_fts5_query("the movie");
        assert!(q.contains("\"movie\""));
        assert!(q.contains("\"film\""));
        assert!(!q.contains("\"the\""));
    }
}
