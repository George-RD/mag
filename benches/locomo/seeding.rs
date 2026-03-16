use anyhow::{Result, anyhow};

use mag::memory_core::storage::sqlite::SqliteStorage;
use mag::memory_core::*;

use crate::types::{DialogueTurn, LoCoMoSample, RetrievalHit};

/// Seed all conversation turns from a LoCoMo sample as memories.
///
/// Each turn is stored with `dia_id` in metadata for evidence recall tracking,
/// `session_id` from the conversation session key, and `referenced_date` from
/// the session's date_time field if available.
pub(crate) async fn seed_sample(storage: &SqliteStorage, sample: &LoCoMoSample) -> Result<usize> {
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
            let input = MemoryInput {
                content: String::new(),
                metadata: meta,
                session_id: Some(key.clone()),
                agent_type: Some(turn.speaker.clone()),
                referenced_date: referenced_date.clone(),
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
