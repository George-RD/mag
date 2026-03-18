use anyhow::{Result, anyhow};

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::*;

use crate::types::{DialogueTurn, LoCoMoSample, RetrievalHit};

/// Common words that should not be extracted as person/entity names.
/// Used by both entity tag extraction during seeding and speaker extraction from questions.
fn is_common_word(word: &str) -> bool {
    matches!(
        word,
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
            | "then"
            | "than"
            | "when"
            | "where"
            | "what"
            | "which"
            | "who"
            | "how"
            | "why"
            | "does"
            | "did"
            | "his"
            | "her"
            | "its"
            | "our"
            | "your"
            | "she"
            | "yes"
            | "yeah"
            | "well"
            | "okay"
            | "sure"
            | "really"
            | "actually"
            | "maybe"
            | "probably"
            | "definitely"
            | "certainly"
    )
}

/// Format a name as a `speaker:{slug}` tag.
fn speaker_tag(name: &str) -> String {
    format!("speaker:{}", name.to_lowercase().replace(' ', "-"))
}

/// Seed all conversation turns from a LoCoMo sample as memories.
///
/// Each turn is stored with `dia_id` in metadata for evidence recall tracking,
/// `session_id` from the conversation session key, and `referenced_date` from
/// the session's date_time field if available.
///
/// When `entity_tags` is true, capitalized words in each turn are extracted as
/// `entity:people:<slug>` tags to enable entity-based graph enrichment.
pub(crate) async fn seed_sample(
    storage: &SqliteStorage,
    sample: &LoCoMoSample,
    entity_tags: bool,
) -> Result<usize> {
    let mut count = 0usize;

    // Collect session keys, filtering out metadata keys.
    let mut session_keys: Vec<String> = sample
        .conversation
        .keys()
        .filter(|key| {
            key.starts_with("session_")
                && !key.ends_with("_date_time")
                && !key.ends_with("_summary")
                && !key.ends_with("_observation")
        })
        .cloned()
        .collect();
    session_keys.sort_by_key(|key| {
        key.trim_start_matches("session_")
            .parse::<u32>()
            .unwrap_or(u32::MAX)
    });

    for key in session_keys {
        let Some(turns_value) = sample.conversation.get(&key) else {
            continue;
        };
        let Some(turns) = turns_value.as_array() else {
            continue;
        };

        // Look up the session date.
        let date_key = format!("{key}_date_time");
        let referenced_date = sample
            .conversation
            .get(&date_key)
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);

        for (turn_idx, turn_value) in turns.iter().enumerate() {
            let turn: DialogueTurn = serde_json::from_value(turn_value.clone()).map_err(|e| {
                anyhow!(
                    "failed to parse dialogue turn for sample {}: {e}",
                    sample.sample_id
                )
            })?;
            if turn.text.trim().is_empty() {
                continue;
            }

            let memory_id = format!("locomo-{}-{key}-{turn_idx}", sample.sample_id);
            let mut meta = serde_json::json!({ "dia_id": turn.dia_id });
            if let Some(ref date) = referenced_date {
                meta["date"] = serde_json::Value::String(date.clone());
            }
            let content = if let Some(ref date) = referenced_date {
                format!("[{}] {}: {}", date, turn.speaker, turn.text)
            } else {
                format!("{}: {}", turn.speaker, turn.text)
            };

            // Structural metadata tags (always added regardless of entity_tags).
            let conversation_tag = format!("conversation:{}", sample.sample_id);
            let session_tag = format!("session:{key}");

            let mut tags: Vec<String> =
                vec![speaker_tag(&turn.speaker), conversation_tag, session_tag];

            // Extract entity tags from turn text (capitalized words as entity:people:slug)
            if entity_tags {
                for word in turn.text.split_whitespace() {
                    let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                    if clean.len() >= 2
                        && clean.chars().next().is_some_and(|c| c.is_uppercase())
                        && clean.chars().skip(1).all(|c| c.is_lowercase())
                    {
                        let lower = clean.to_lowercase();
                        if !is_common_word(&lower) {
                            let tag = format!("entity:people:{lower}");
                            if !tags.contains(&tag) {
                                tags.push(tag);
                            }
                        }
                    }
                }
            }

            let input = MemoryInput {
                content: String::new(),
                metadata: meta,
                session_id: Some(key.clone()),
                agent_type: Some(turn.speaker.clone()),
                referenced_date: referenced_date.clone(),
                tags,
                ..MemoryInput::default()
            };
            storage.store(&memory_id, &content, &input).await?;
            count += 1;
        }
    }

    Ok(count)
}

/// Query the storage and return hits with metadata (for evidence recall).
pub(crate) async fn query_with_metadata(
    storage: &SqliteStorage,
    question: &str,
    top_k: usize,
) -> Result<Vec<RetrievalHit>> {
    let filters = SearchOptions::default();

    let advanced =
        <SqliteStorage as AdvancedSearcher>::advanced_search(storage, question, top_k, &filters)
            .await;

    match advanced {
        Ok(items) if !items.is_empty() => {
            return Ok(items
                .into_iter()
                .map(|item| RetrievalHit {
                    content: item.content,
                    score: item.score,
                    metadata: item.metadata,
                })
                .collect());
        }
        Err(err) => {
            eprintln!("warning: advanced_search failed, falling back: {err}");
        }
        _ => {}
    }

    // Fallback to basic search.
    let items = <SqliteStorage as Searcher>::search(storage, question, top_k, &filters).await?;
    Ok(items
        .into_iter()
        .map(|item| RetrievalHit {
            content: item.content,
            score: 0.1,
            metadata: item.metadata,
        })
        .collect())
}

/// Query with semantic search + speaker-tag secondary recall (AutoMem-compatible).
///
/// 1. Run semantic `query_with_metadata()` (limit = `top_k`)
/// 2. Extract speaker name from question (first capitalized proper noun)
/// 3. If speaker found: tag-search `get_by_tags([speaker:{name}, conversation:{id}], 50)`
/// 4. Merge results, dedup by dia_id, keep higher score on collision
pub(crate) async fn query_with_speaker_recall(
    storage: &SqliteStorage,
    question: &str,
    sample_id: &str,
    top_k: usize,
) -> Result<Vec<RetrievalHit>> {
    // Step 1: Semantic search
    let mut hits = query_with_metadata(storage, question, top_k).await?;

    // Step 2: Extract speaker name from question
    let speaker = extract_speaker_from_question(question);

    // Step 3: Speaker-tag recall
    if let Some(speaker_name) = speaker {
        let spk_tag = speaker_tag(&speaker_name);
        let conversation_tag = format!("conversation:{sample_id}");
        let tags = vec![spk_tag, conversation_tag];

        let tag_results =
            <SqliteStorage as Tagger>::get_by_tags(storage, &tags, 50, &SearchOptions::default())
                .await
                .unwrap_or_default();

        // Step 4: Merge, dedup by dia_id
        let mut seen_dia_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for hit in &hits {
            if let Some(dia_id) = hit.dia_id() {
                seen_dia_ids.insert(dia_id.to_string());
            }
        }

        for result in tag_results {
            let dia_id = result
                .metadata
                .get("dia_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if !dia_id.is_empty() && !seen_dia_ids.contains(&dia_id) {
                seen_dia_ids.insert(dia_id);
                hits.push(RetrievalHit {
                    content: result.content,
                    #[allow(clippy::cast_possible_truncation)]
                    score: result.importance as f32,
                    metadata: result.metadata,
                });
            }
        }
    }

    Ok(hits)
}

/// Extract the first capitalized proper noun from a question as the speaker name.
fn extract_speaker_from_question(question: &str) -> Option<String> {
    let words: Vec<&str> = question.split_whitespace().collect();
    for (i, word) in words.iter().enumerate() {
        // Strip possessives and punctuation
        let clean: String = word
            .replace("'s", "")
            .replace("\u{2019}s", "")
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        if clean.len() < 2 {
            continue;
        }

        let first_char = clean.chars().next()?;
        if !first_char.is_uppercase() {
            continue;
        }
        if !clean.chars().skip(1).all(|c| c.is_lowercase()) {
            continue;
        }

        // Skip sentence-initial words (index 0, or after sentence-ending punctuation)
        if i == 0 {
            continue;
        }
        // Also skip after "?" at the previous word
        if words[i - 1].ends_with('?') {
            continue;
        }

        // Skip common non-name words
        let lower = clean.to_lowercase();
        if is_common_word(&lower) {
            continue;
        }

        return Some(clean);
    }
    None
}
