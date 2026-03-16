use super::*;
use crate::memory_core::storage::sqlite::helpers::append_context_tag_filters;

#[async_trait]
impl FeedbackRecorder for SqliteStorage {
    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value> {
        let delta = match rating {
            "helpful" => 1_i64,
            "unhelpful" => -1_i64,
            "outdated" => -2_i64,
            _ => return Err(anyhow!("invalid rating: {rating}")),
        };

        let pool = Arc::clone(&self.pool);
        let memory_id = memory_id.to_string();
        let rating = rating.to_string();
        let reason = reason.map(ToString::to_string);

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;

            let metadata_raw: Option<String> = conn
                .query_row(
                    "SELECT metadata FROM memories WHERE id = ?1",
                    params![memory_id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query metadata for feedback")?;

            let metadata_raw =
                metadata_raw.ok_or_else(|| anyhow!("memory not found for id={memory_id}"))?;
            let mut metadata = parse_metadata_from_db(&metadata_raw);

            let mut feedback_signals = metadata
                .get("feedback_signals")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();

            feedback_signals.push(serde_json::json!({
                "rating": rating,
                "reason": reason,
                "at": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            }));

            let new_score = metadata
                .get("feedback_score")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                + delta;
            let flagged = new_score <= -3;

            metadata["feedback_signals"] = serde_json::Value::Array(feedback_signals.clone());
            metadata["feedback_score"] = serde_json::Value::Number(new_score.into());
            if flagged {
                metadata["flagged_for_review"] = serde_json::Value::Bool(true);
            }

            let metadata_json = serde_json::to_string(&metadata)
                .context("failed to serialize feedback metadata")?;
            conn.execute(
                "UPDATE memories
                 SET metadata = ?2,
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![memory_id, metadata_json],
            )
            .context("failed to persist feedback metadata")?;

            Ok::<_, anyhow::Error>(serde_json::json!({
                "memory_id": memory_id,
                "rating": rating,
                "new_score": new_score,
                "total_signals": feedback_signals.len(),
                "flagged": flagged,
            }))
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl ExpirationSweeper for SqliteStorage {
    async fn sweep_expired(&self) -> Result<usize> {
        let pool = Arc::clone(&self.pool);

        let count = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sweep transaction")?;

            let mut stmt = tx
                .prepare(
                    "SELECT id FROM memories
                     WHERE ttl_seconds IS NOT NULL
                       AND datetime(created_at, '+' || ttl_seconds || ' seconds') < datetime('now')",
                )
                .context("failed to prepare expiration query")?;
            let expired_rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .context("failed to execute expiration query")?;

            let mut expired_ids = Vec::new();
            for row in expired_rows {
                expired_ids.push(row.context("failed to decode expiration row")?);
            }
            drop(stmt);

            for id in &expired_ids {
                tx.execute(
                    "DELETE FROM relationships WHERE source_id = ?1 OR target_id = ?1",
                    params![id],
                )
                .context("failed to delete relationships during sweep")?;
                tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to delete FTS during sweep")?;

                #[cfg(feature = "sqlite-vec")]
                vec_delete(&tx, id)?;

                tx.execute("DELETE FROM memories WHERE id = ?1", params![id])
                    .context("failed to delete memory during sweep")?;
            }

            tx.commit().context("failed to commit sweep transaction")?;
            Ok::<_, anyhow::Error>(expired_ids.len())
        })
        .await
        .context("spawn_blocking join error")??;

        if count > 0 {
            // Expired memories could belong to any type/project/session, so full clear.
            self.invalidate_query_cache();
        }

        Ok(count)
    }
}

#[async_trait]
impl Lister for SqliteStorage {
    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult> {
        let opts = opts.clone();
        let include_superseded = opts.include_superseded.unwrap_or(false);
        if limit == 0 {
            let pool = Arc::clone(&self.pool);
            let count_opts = opts.clone();
            let total = tokio::task::spawn_blocking(move || {
                use rusqlite::types::Value as SqlValue;

                let conn = pool.reader()?;
                let mut sql = String::from("SELECT COUNT(*) FROM memories WHERE 1 = 1");
                if !include_superseded {
                    sql.push_str(" AND superseded_by_id IS NULL");
                }
                let mut params_values: Vec<SqlValue> = Vec::new();
                let mut idx = 1;
                append_search_filters(&mut sql, &mut params_values, &mut idx, &count_opts, "");
                append_context_tag_filters(
                    &mut sql,
                    &mut params_values,
                    &mut idx,
                    count_opts.context_tags.as_deref(),
                    "tags",
                );
                let param_refs = to_param_refs(&params_values);
                let mut stmt = conn
                    .prepare(&sql)
                    .context("failed to prepare list count query")?;
                let count: i64 = stmt
                    .query_row(param_refs.as_slice(), |row| row.get(0))
                    .context("failed to count memories")?;
                Ok::<_, anyhow::Error>(usize::try_from(count).unwrap_or(0))
            })
            .await
            .context("spawn_blocking join error")??;
            return Ok(ListResult {
                memories: Vec::new(),
                total,
            });
        }

        let pool = Arc::clone(&self.pool);
        let effective_limit = i64::try_from(limit).context("list limit exceeds i64")?;
        let effective_offset = i64::try_from(offset).context("list offset exceeds i64")?;

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = pool.reader()?;

            let mut count_sql = String::from("SELECT COUNT(*) FROM memories WHERE 1 = 1");
            if !include_superseded {
                count_sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut filter_params: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            append_search_filters(&mut count_sql, &mut filter_params, &mut idx, &opts, "");
            append_context_tag_filters(
                &mut count_sql,
                &mut filter_params,
                &mut idx,
                opts.context_tags.as_deref(),
                "tags",
            );

            let mut count_stmt = conn
                .prepare(&count_sql)
                .context("failed to prepare list count query")?;
            let count_param_refs = to_param_refs(&filter_params);
            let total: i64 = count_stmt
                .query_row(count_param_refs.as_slice(), |row| row.get(0))
                .context("failed to count memories")?;

            let mut data_sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type FROM memories WHERE 1 = 1",
            );
            if !include_superseded {
                data_sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut data_params: Vec<SqlValue> = Vec::new();
            let mut next_idx = 1;
            append_search_filters(&mut data_sql, &mut data_params, &mut next_idx, &opts, "");
            append_context_tag_filters(
                &mut data_sql,
                &mut data_params,
                &mut next_idx,
                opts.context_tags.as_deref(),
                "tags",
            );
            data_sql.push_str(" ORDER BY created_at DESC");
            data_sql.push_str(&format!(" LIMIT ?{next_idx}"));
            data_params.push(SqlValue::Integer(effective_limit));
            next_idx += 1;
            data_sql.push_str(&format!(" OFFSET ?{next_idx}"));
            data_params.push(SqlValue::Integer(effective_offset));

            let mut stmt = conn
                .prepare(&data_sql)
                .context("failed to prepare list query")?;

            let data_param_refs = to_param_refs(&data_params);

            let rows = stmt
                .query_map(data_param_refs.as_slice(), search_result_from_row)
                .context("failed to execute list query")?;

            let mut memories = Vec::new();
            for row in rows {
                memories.push(row.context("failed to decode list row")?);
            }

            Ok::<_, anyhow::Error>(ListResult {
                memories,
                total: usize::try_from(total).unwrap_or(0),
            })
        })
        .await
        .context("spawn_blocking join error")?
    }
}
