use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use super::candidate_scorer::{jaccard_pre, token_set, word_overlap_pre};
use super::helpers::{event_type_from_sql, parse_metadata_from_db, parse_tags_from_db};
use crate::memory_core::{EventType, SearchOptions, SemanticResult};

pub(super) const HOT_CACHE_CAPACITY: usize = 50;
pub(super) const HOT_CACHE_REFRESH_SECS: u64 = 300;
const HOT_CACHE_MIN_SCORE: f32 = 0.45;

#[derive(Debug, Clone)]
struct HotEntry {
    id: String,
    content: String,
    tags: Vec<String>,
    importance: f64,
    event_type: Option<EventType>,
    access_count: i64,
    session_id: Option<String>,
    project: Option<String>,
    entity_id: Option<String>,
    agent_type: Option<String>,
    metadata: serde_json::Value,
    expires_at_unix: Option<i64>,
    tokens: HashSet<String>,
}

#[derive(Debug, Clone)]
pub(super) struct HotTierCache {
    entries: Arc<RwLock<Vec<HotEntry>>>,
    initialized: Arc<AtomicBool>,
    capacity: usize,
    refresh_interval: Duration,
}

impl HotTierCache {
    pub(super) fn new(capacity: usize, refresh_interval: Duration) -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
            initialized: Arc::new(AtomicBool::new(false)),
            capacity,
            refresh_interval,
        }
    }

    pub(super) fn refresh_interval(&self) -> Duration {
        self.refresh_interval
    }

    pub(super) fn clear(&self) {
        match self.entries.write() {
            Ok(mut guard) => guard.clear(),
            Err(poison) => {
                tracing::error!("hot cache RwLock poisoned during clear; recovering");
                poison.into_inner().clear();
            }
        }
        self.initialized.store(false, Ordering::Release);
    }

    pub(super) fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    pub(super) fn refresh(&self, conn: &Connection) -> Result<()> {
        let limit = i64::try_from(self.capacity).context("hot cache capacity exceeds i64")?;
        let mut stmt = conn
            .prepare(
                "SELECT id, content, tags, importance, event_type, access_count, session_id, project,
                        entity_id, agent_type,
                        metadata,
                        CASE
                            WHEN ttl_seconds IS NULL THEN NULL
                            ELSE CAST(strftime('%s', datetime(created_at, '+' || ttl_seconds || ' seconds')) AS INTEGER)
                        END AS expires_at_unix
                 FROM memories
                 WHERE access_count > 0
                   AND superseded_by_id IS NULL
                   AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                 ORDER BY access_count DESC, last_accessed_at DESC
                 LIMIT ?1",
            )
            .context("failed to prepare hot cache refresh query")?;

        let rows = stmt
            .query_map(params![limit], |row| {
                let content: String = row.get(1)?;
                let raw_tags: String = row.get(2)?;
                let tags = parse_tags_from_db(&raw_tags);
                let tag_tokens = tags.join(" ");
                let tokens = token_set(&format!("{content} {tag_tokens}"), 3);
                let raw_metadata: String = row.get(10)?;
                Ok(HotEntry {
                    id: row.get(0)?,
                    content,
                    tags,
                    importance: row.get(3)?,
                    event_type: event_type_from_sql(row.get::<_, Option<String>>(4)?),
                    access_count: row.get(5)?,
                    session_id: row.get(6)?,
                    project: row.get(7)?,
                    entity_id: row.get(8)?,
                    agent_type: row.get(9)?,
                    metadata: parse_metadata_from_db(&raw_metadata),
                    expires_at_unix: row.get(11)?,
                    tokens,
                })
            })
            .context("failed to execute hot cache refresh query")?;

        let entries: Vec<HotEntry> = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to decode hot cache row")?;

        match self.entries.write() {
            Ok(mut guard) => {
                *guard = entries;
                self.initialized.store(true, Ordering::Release);
            }
            Err(poison) => {
                tracing::error!("hot cache RwLock poisoned during refresh; recovering");
                let mut guard = poison.into_inner();
                *guard = entries;
                self.initialized.store(true, Ordering::Release);
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub(super) fn query(&self, query: &str, limit: usize) -> Vec<SemanticResult> {
        self.query_with_options(query, limit, &SearchOptions::default())
    }

    pub(super) fn query_with_options(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Vec<SemanticResult> {
        if limit == 0
            || opts.created_after.is_some()
            || opts.created_before.is_some()
            || opts.event_after.is_some()
            || opts.event_before.is_some()
            || opts
                .context_tags
                .as_ref()
                .is_some_and(|tags| !tags.is_empty())
            || opts.entity_id.is_some()
            || opts.agent_type.is_some()
        {
            tracing::trace!("hot cache bypassed due to unsupported filter options");
            return Vec::new();
        }

        let query_tokens = token_set(query, 3);
        if query_tokens.is_empty() {
            return Vec::new();
        }

        let now_unix = chrono::Utc::now().timestamp();
        let mut results: Vec<SemanticResult> = {
            let entries = match self.entries.read() {
                Ok(guard) => guard,
                Err(poison) => {
                    tracing::error!("hot cache RwLock poisoned during query; recovering");
                    poison.into_inner()
                }
            };
            if entries.is_empty() {
                return Vec::new();
            }

            #[allow(clippy::cast_precision_loss)]
            let max_access = entries
                .iter()
                .filter(|entry| {
                    entry
                        .expires_at_unix
                        .is_none_or(|expires_at_unix| expires_at_unix > now_unix)
                })
                .map(|entry| entry.access_count)
                .max()
                .unwrap_or(1) as f64;

            entries
                .iter()
                .filter(|entry| {
                    entry
                        .expires_at_unix
                        .is_none_or(|expires_at_unix| expires_at_unix > now_unix)
                        && opts
                            .event_type
                            .as_ref()
                            .is_none_or(|event_type| entry.event_type.as_ref() == Some(event_type))
                        && opts
                            .project
                            .as_ref()
                            .is_none_or(|project| entry.project.as_ref() == Some(project))
                        && opts
                            .session_id
                            .as_ref()
                            .is_none_or(|session_id| entry.session_id.as_ref() == Some(session_id))
                        && opts
                            .importance_min
                            .is_none_or(|importance_min| entry.importance >= importance_min)
                })
                .filter_map(|entry| {
                    let overlap = word_overlap_pre(&query_tokens, &entry.tokens);
                    if overlap <= 0.0 {
                        return None;
                    }

                    let jaccard = jaccard_pre(&query_tokens, &entry.tokens);
                    #[allow(clippy::cast_precision_loss)]
                    let access_norm = if max_access > 0.0 {
                        entry.access_count as f64 / max_access
                    } else {
                        0.0
                    };
                    #[allow(clippy::cast_possible_truncation)]
                    let score = (overlap * 0.75
                        + jaccard * 0.15
                        + entry.importance * 0.05
                        + access_norm * 0.05) as f32;
                    if score < HOT_CACHE_MIN_SCORE {
                        return None;
                    }

                    let mut metadata = match &entry.metadata {
                        serde_json::Value::Object(map) => serde_json::Value::Object(map.clone()),
                        serde_json::Value::Null => serde_json::json!({}),
                        other => serde_json::json!({ "_stored_metadata": other }),
                    };
                    if let serde_json::Value::Object(meta) = &mut metadata {
                        meta.insert("_hot_cache".to_string(), serde_json::json!(true));
                        meta.insert("_hot_cache_score".to_string(), serde_json::json!(score));
                        meta.insert("_text_overlap".to_string(), serde_json::json!(overlap));
                        meta.insert("_jaccard".to_string(), serde_json::json!(jaccard));
                        meta.insert(
                            "_access_count".to_string(),
                            serde_json::json!(entry.access_count),
                        );
                    }

                    Some(SemanticResult {
                        id: entry.id.clone(),
                        content: entry.content.clone(),
                        tags: entry.tags.clone(),
                        importance: entry.importance,
                        metadata,
                        event_type: entry.event_type.clone(),
                        session_id: entry.session_id.clone(),
                        project: entry.project.clone(),
                        entity_id: entry.entity_id.clone(),
                        agent_type: entry.agent_type.clone(),
                        score,
                    })
                })
                .collect()
        };

        results.sort_by(|left, right| right.score.total_cmp(&left.score));
        results.truncate(limit);
        results
    }
}
