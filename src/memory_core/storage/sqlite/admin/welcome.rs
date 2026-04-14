//! Welcome and session-start context for the SQLite storage backend.
//!
//! Implements the `WelcomeProvider` trait, which assembles the greeting payload
//! returned at session start — recent memories, user preferences, profile, and
//! pending reminders. Both a simple (`welcome`) and a token-budget-aware
//! (`welcome_scoped`) variant are provided, each optionally enriched with a
//! semantic search pass for project-scoped context.

use super::super::*;
use crate::memory_core::WelcomeOptions;

/// Estimate the number of LLM tokens in a string using the 4-chars-per-token heuristic.
fn estimate_tokens(s: &str) -> usize {
    s.len().div_ceil(4)
}

#[async_trait]
impl WelcomeProvider for SqliteStorage {
    async fn welcome(
        &self,
        _session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        let pool = Arc::clone(&self.pool);
        let project = project.map(ToString::to_string);
        let project_for_semantic = project.clone();

        let db_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM memories WHERE superseded_by_id IS NULL", [], |row| row.get(0))
                .context("failed to count memories")?;

            let mut sql =
                String::from("SELECT id, content, event_type, priority, created_at FROM memories WHERE superseded_by_id IS NULL");
            let mut params_values: Vec<rusqlite::types::Value> = Vec::new();

            if let Some(ref proj) = project {
                sql.push_str(" AND project = ?1");
                params_values.push(rusqlite::types::Value::Text(proj.clone()));
            }

            sql.push_str(" ORDER BY created_at DESC LIMIT 15");

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare welcome query")?;
            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for v in &params_values {
                param_refs.push(v);
            }

            let rows = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(200).collect::<String>(),
                        "event_type": row.get::<_, Option<String>>(2)?,
                        "priority": row.get::<_, Option<i64>>(3)?,
                        "created_at": row.get::<_, String>(4)?,
                        "source": "tiered",
                    }))
                })
                .context("failed to query recent memories")?;

            let recent: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();

            // Explicitly surface user_preference and user_fact memories
            let mut prefs_stmt = conn
                .prepare(
                    "SELECT id, content, event_type, importance, created_at FROM memories \
                     WHERE event_type IN ('user_preference', 'user_fact') \
                     AND superseded_by_id IS NULL \
                     ORDER BY importance DESC, created_at DESC LIMIT 20",
                )
                .context("failed to prepare user preferences query")?;
            let pref_rows = prefs_stmt
                .query_map([], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "content": row.get::<_, String>(1)?.chars().take(300).collect::<String>(),
                        "event_type": row.get::<_, Option<String>>(2)?,
                        "importance": row.get::<_, f64>(3)?,
                        "created_at": row.get::<_, String>(4)?,
                        "source": "tiered",
                    }))
                })
                .context("failed to query user preferences")?;
            let user_context: Vec<serde_json::Value> = pref_rows.filter_map(|r| r.ok()).collect();

            Ok::<_, anyhow::Error>((total, recent, user_context))
        })
        .await
        .context("spawn_blocking join error")??;

        let (total, mut recent, user_context) = db_result;

        // ── Semantic search phase for welcome() ────────────────────────
        // If a project is specified, supplement recent memories with
        // semantically relevant results using a fixed ~1500-token budget.
        if let Some(ref proj) = project_for_semantic {
            const WELCOME_SEMANTIC_BUDGET: usize = 1500;
            let used_tokens: usize = recent
                .iter()
                .map(|m| estimate_tokens(m.get("content").and_then(|v| v.as_str()).unwrap_or("")))
                .sum();
            let remaining = WELCOME_SEMANTIC_BUDGET.saturating_sub(used_tokens);

            if remaining > 0 {
                let search_opts = SearchOptions {
                    project: Some(proj.clone()),
                    ..SearchOptions::default()
                };

                let mut seen_ids: HashSet<String> = recent
                    .iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect();

                // Build a meaningful semantic query from tiered results rather
                // than using the raw project label (which is unlikely to appear
                // verbatim in memory content).
                let semantic_query: String = {
                    let snippets: Vec<&str> = recent
                        .iter()
                        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
                        .take(5)
                        .collect();
                    if snippets.is_empty() {
                        "recent important memories".to_string()
                    } else {
                        let joined: String = snippets
                            .iter()
                            .map(|s| s.chars().take(100).collect::<String>())
                            .collect::<Vec<_>>()
                            .join("; ");
                        format!("recent context: {joined}")
                    }
                };

                let candidate_count = 10usize;
                match <SqliteStorage as AdvancedSearcher>::advanced_search(
                    self,
                    &semantic_query,
                    candidate_count,
                    &search_opts,
                )
                .await
                {
                    Ok(semantic_results) => {
                        let mut sem_remaining = remaining;
                        for sr in semantic_results {
                            if seen_ids.contains(&sr.id) {
                                continue;
                            }
                            let truncated: String = sr.content.chars().take(200).collect();
                            let tokens = estimate_tokens(&truncated);
                            if tokens > sem_remaining {
                                break;
                            }
                            sem_remaining = sem_remaining.saturating_sub(tokens);
                            seen_ids.insert(sr.id.clone());

                            let et_str = sr.event_type.as_ref().map(|e| e.to_string());
                            recent.push(serde_json::json!({
                                "id": sr.id,
                                "content": truncated,
                                "event_type": et_str,
                                "importance": sr.importance,
                                "source": "semantic",
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            project = proj.as_str(),
                            query_len = semantic_query.len(),
                            candidate_count,
                            error_kind = %e.root_cause(),
                            "semantic search failed in welcome()"
                        );
                    }
                }
            }
        }

        // Get profile and pending reminders via existing trait impls
        let profile = <Self as ProfileManager>::get_profile(self)
            .await
            .unwrap_or(serde_json::json!({}));
        let reminders = <Self as ReminderManager>::list_reminders(self, Some("pending"))
            .await
            .unwrap_or_default();

        let greeting = if total == 0 {
            "Welcome to MAG! Store your first memory to get started.".to_string()
        } else {
            format!("Welcome back! You have {total} memories stored.")
        };

        Ok(serde_json::json!({
            "greeting": greeting,
            "memory_count": total,
            "recent_memories": recent,
            "user_context": user_context,
            "profile": profile,
            "pending_reminders": reminders,
        }))
    }

    async fn welcome_scoped(&self, opts: &WelcomeOptions) -> Result<serde_json::Value> {
        // If no budget and no scoping beyond what welcome() already supports, delegate exactly.
        // Note: welcome() already handles project filtering, so only agent_type and entity_id
        // count as "extra scope" that requires the budgeted path.
        let has_extra_scope = opts.agent_type.is_some() || opts.entity_id.is_some();
        if opts.budget_tokens.is_none() && !has_extra_scope {
            return self
                .welcome(opts.session_id.as_deref(), opts.project.as_deref())
                .await;
        }

        let budget = opts.budget_tokens.unwrap_or(usize::MAX);

        // Reserve ~200 tokens for greeting/profile/reminders overhead.
        // Used by both the tiered-SQL phase (inside spawn_blocking) and
        // the semantic-search phase (outside it).
        const OVERHEAD_TOKENS: usize = 200;

        let pool = Arc::clone(&self.pool);
        let project = opts.project.clone();
        let agent_type = opts.agent_type.clone();
        let entity_id = opts.entity_id.clone();

        let db_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

            // Total memory count (active, non-superseded)
            let total: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM memories WHERE superseded_by_id IS NULL",
                    [],
                    |row| row.get(0),
                )
                .context("failed to count memories")?;

            // Helper: build a common scope filter clause and params.
            // Returns (where_fragment, params_vec) where where_fragment starts with " AND …"
            // or is empty.  project / agent_type / entity_id filters are combined.
            let mut scope_clauses: Vec<String> = Vec::new();
            let mut scope_params: Vec<rusqlite::types::Value> = Vec::new();
            if let Some(ref proj) = project {
                scope_clauses.push(format!("project = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(proj.clone()));
            }
            if let Some(ref at) = agent_type {
                scope_clauses.push(format!("agent_type = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(at.clone()));
            }
            if let Some(ref eid) = entity_id {
                scope_clauses.push(format!("entity_id = ?{}", scope_params.len() + 1));
                scope_params.push(rusqlite::types::Value::Text(eid.clone()));
            }
            let scope_sql = if scope_clauses.is_empty() {
                String::new()
            } else {
                format!(" AND {}", scope_clauses.join(" AND "))
            };

            // Each tier: (sql_condition, content_cap_chars, order_by_clause)
            // Base filter: superseded_by_id IS NULL + scope filters, applied to all tiers.
            struct Tier {
                cond: &'static str,
                cap_chars: usize,
                order: &'static str,
            }
            let tiers = [
                // Tier 1: pinned
                Tier {
                    cond: "json_extract(metadata, '$.pinned') = 1",
                    cap_chars: 300,
                    order: "importance DESC, created_at DESC",
                },
                // Tier 2: user preferences and facts with high importance
                Tier {
                    cond: "event_type IN ('user_preference','user_fact') AND importance >= 0.5",
                    cap_chars: 300,
                    order: "importance DESC, created_at DESC",
                },
                // Tier 3: moderate-importance memories
                Tier {
                    cond: "importance >= 0.3",
                    cap_chars: 200,
                    order: "created_at DESC",
                },
                // Tier 4: low-importance / auto-captured
                Tier {
                    cond: "importance < 0.3",
                    cap_chars: 150,
                    order: "created_at DESC",
                },
            ];

            let mut remaining = budget.saturating_sub(OVERHEAD_TOKENS);

            let mut all_memories: Vec<serde_json::Value> = Vec::new();
            let mut seen_ids: HashSet<String> = HashSet::new();

            for tier in &tiers {
                if remaining == 0 {
                    break;
                }

                let sql = format!(
                    "SELECT id, content, event_type, importance, priority, created_at FROM memories \
                     WHERE superseded_by_id IS NULL{scope_sql} AND {} ORDER BY {} LIMIT 50",
                    tier.cond, tier.order
                );

                let mut stmt = conn.prepare(&sql).context("failed to prepare tier query")?;
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    scope_params.iter().map(|v| v as &dyn rusqlite::types::ToSql).collect();

                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, f64>(3)?,
                            row.get::<_, Option<i64>>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    })
                    .context("failed to query tier memories")?;

                for row_result in rows {
                    let (id, content, event_type, importance, priority, created_at) =
                        row_result.context("failed to decode tier row")?;

                    if seen_ids.contains(&id) {
                        continue;
                    }

                    let truncated: String = content.chars().take(tier.cap_chars).collect();
                    let tokens = estimate_tokens(&truncated);
                    if tokens > remaining {
                        break;
                    }

                    remaining = remaining.saturating_sub(tokens);
                    seen_ids.insert(id.clone());
                    all_memories.push(serde_json::json!({
                        "id": id,
                        "content": truncated,
                        "event_type": event_type,
                        "importance": importance,
                        "priority": priority,
                        "created_at": created_at,
                        "source": "tiered",
                    }));

                    if remaining == 0 {
                        break;
                    }
                }
            }

            Ok::<_, anyhow::Error>((total, all_memories))
        })
        .await
        .context("spawn_blocking join error")??;

        let (total, mut all_memories) = db_result;

        // ── Semantic search phase ──────────────────────────────────────
        // If a project is specified and we have remaining token budget,
        // use AdvancedSearcher to find project-relevant memories that
        // the tiered SQL queries may have missed.
        if opts.project.is_some() {
            let used_tokens: usize = all_memories
                .iter()
                .map(|m| estimate_tokens(m.get("content").and_then(|v| v.as_str()).unwrap_or("")))
                .sum();
            let remaining = budget
                .saturating_sub(OVERHEAD_TOKENS)
                .saturating_sub(used_tokens);

            if remaining > 0 {
                let search_opts = SearchOptions {
                    project: opts.project.clone(),
                    agent_type: opts.agent_type.clone(),
                    entity_id: opts.entity_id.clone(),
                    ..SearchOptions::default()
                };

                // Build a meaningful semantic query from tiered result snippets
                // rather than using the raw project label.
                let semantic_query: String = {
                    let snippets: Vec<&str> = all_memories
                        .iter()
                        .filter_map(|m| m.get("content").and_then(|v| v.as_str()))
                        .take(5)
                        .collect();
                    if snippets.is_empty() {
                        "recent important memories".to_string()
                    } else {
                        let joined: String = snippets
                            .iter()
                            .map(|s| s.chars().take(100).collect::<String>())
                            .collect::<Vec<_>>()
                            .join("; ");
                        format!("recent context: {joined}")
                    }
                };

                let mut seen_ids: HashSet<String> = all_memories
                    .iter()
                    .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                    .collect();

                let candidate_count = 10usize;
                match <SqliteStorage as AdvancedSearcher>::advanced_search(
                    self,
                    &semantic_query,
                    candidate_count,
                    &search_opts,
                )
                .await
                {
                    Ok(semantic_results) => {
                        let mut sem_remaining = remaining;
                        for sr in semantic_results {
                            if seen_ids.contains(&sr.id) {
                                continue;
                            }
                            let truncated: String = sr.content.chars().take(200).collect();
                            let tokens = estimate_tokens(&truncated);
                            if tokens > sem_remaining {
                                break;
                            }
                            sem_remaining = sem_remaining.saturating_sub(tokens);
                            seen_ids.insert(sr.id.clone());

                            let et_str = sr.event_type.as_ref().map(|e| e.to_string());
                            all_memories.push(serde_json::json!({
                                "id": sr.id,
                                "content": truncated,
                                "event_type": et_str,
                                "importance": sr.importance,
                                "source": "semantic",
                            }));
                        }
                    }
                    Err(e) => {
                        tracing::debug!(
                            project = opts.project.as_deref().unwrap_or(""),
                            query_len = semantic_query.len(),
                            candidate_count,
                            error_kind = %e.root_cause(),
                            "semantic search failed in welcome_scoped()"
                        );
                    }
                }
            }
        }

        // Profile and reminders (same as welcome())
        let profile = <Self as ProfileManager>::get_profile(self)
            .await
            .unwrap_or(serde_json::json!({}));
        let reminders = <Self as ReminderManager>::list_reminders(self, Some("pending"))
            .await
            .unwrap_or_default();

        let greeting = if total == 0 {
            "Welcome to MAG! Store your first memory to get started.".to_string()
        } else {
            format!("Welcome back! You have {total} memories stored.")
        };

        // Split into recent_memories and user_context to preserve JSON shape.
        // user_context = user_preference / user_fact entries; recent_memories = everything else.
        let mut recent_memories: Vec<serde_json::Value> = Vec::new();
        let mut user_context: Vec<serde_json::Value> = Vec::new();
        for m in all_memories {
            let et = m.get("event_type").and_then(|v| v.as_str()).unwrap_or("");
            if et == "user_preference" || et == "user_fact" {
                user_context.push(m);
            } else {
                recent_memories.push(m);
            }
        }

        Ok(serde_json::json!({
            "greeting": greeting,
            "memory_count": total,
            "recent_memories": recent_memories,
            "user_context": user_context,
            "profile": profile,
            "pending_reminders": reminders,
        }))
    }
}
