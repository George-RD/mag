use anyhow::{Result, anyhow};

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::*;

use crate::types::{DialogueTurn, LoCoMoSample, RetrievalHit};

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

            // Extract entity tags from turn text (capitalized words as entity:people:slug)
            let tags: Vec<String> = if entity_tags {
                let mut t: Vec<String> = Vec::new();
                for word in turn.text.split_whitespace() {
                    let clean: String = word.chars().filter(|c| c.is_alphanumeric()).collect();
                    if clean.len() >= 2
                        && clean.chars().next().is_some_and(|c| c.is_uppercase())
                        && clean.chars().skip(1).all(|c| c.is_lowercase())
                    {
                        // Simple heuristic: skip very common words
                        let lower = clean.to_lowercase();
                        if !matches!(
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
                                | "then"
                                | "than"
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
                        ) {
                            let slug = lower.replace(' ', "-");
                            let tag = format!("entity:people:{slug}");
                            if !t.contains(&tag) {
                                t.push(tag);
                            }
                        }
                    }
                }
                t
            } else {
                Vec::new()
            };

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
