use super::*;

#[async_trait]
impl Storage for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        let tags_json =
            serde_json::to_string(&input.tags).context("failed to serialize tags to JSON")?;
        let metadata_json = serde_json::to_string(&input.metadata)
            .context("failed to serialize metadata to JSON")?;

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();
        let data = data.to_string();
        let importance = input.importance;
        let event_type = input.event_type.clone();
        let session_id = input.session_id.clone();
        let project = input.project.clone();
        let priority = input.priority;
        let entity_id = input.entity_id.clone();
        let agent_type = input.agent_type.clone();
        let ttl_seconds = input.ttl_seconds;
        let id_for_store = id.clone();

        let (outcome, superseded_ids) = tokio::task::spawn_blocking(move || {
            let c_hash = content_hash(&data);
            let normalized_hash = canonical_hash(&data);
            let embedding = serde_json::to_vec(&embedder.embed(&data)?)
                .context("failed to serialize embedding")?;
            let conn = lock_conn(&conn)?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sqlite transaction")?;

            let existing_canonical_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM memories
                     WHERE canonical_hash = ?1
                       AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                     LIMIT 1",
                    params![normalized_hash],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query canonical hash dedup")?;

            if let Some(existing_id) = existing_canonical_id {
                tx.execute(
                    "UPDATE memories
                     SET access_count = access_count + 1,
                         last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?1",
                    params![existing_id],
                )
                .context("failed to update access_count for canonical dedup")?;
                tx.commit().context("failed to commit canonical dedup")?;
                return Ok::<_, anyhow::Error>((StoreOutcome::Deduped, Vec::new()));
            }

            if let Some(ref event_type_value) = event_type {
                let threshold = DEDUP_THRESHOLDS
                    .iter()
                    .find(|(kind, _)| kind == &event_type_value.as_str())
                    .map(|(_, threshold)| *threshold);

                if let Some(threshold) = threshold {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id, content FROM memories WHERE event_type = ?1
                             AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                             ORDER BY created_at DESC LIMIT 5",
                        )
                        .context("failed to prepare Jaccard dedup query")?;
                    let rows = stmt
                        .query_map(params![event_type_value], |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                        })
                        .context("failed to execute Jaccard dedup query")?;

                    let mut matched_id: Option<String> = None;
                    for row in rows {
                        let (candidate_id, candidate_content) =
                            row.context("failed to decode Jaccard dedup row")?;
                        let similarity = jaccard_similarity(&data, &candidate_content, 3);
                        if similarity >= threshold {
                            matched_id = Some(candidate_id);
                            break;
                        }
                    }

                    if let Some(existing_id) = matched_id {
                        drop(stmt);
                        tx.execute(
                            "UPDATE memories
                             SET access_count = access_count + 1,
                                 last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                             WHERE id = ?1",
                            params![existing_id],
                        )
                        .context("failed to update access_count for Jaccard dedup")?;
                        tx.commit().context("failed to commit Jaccard dedup")?;
                        return Ok::<_, anyhow::Error>((StoreOutcome::Deduped, Vec::new()));
                    }
                }
            }

            let mut superseded_ids: Vec<String> = Vec::new();
            if let Some(ref event_type_value) = event_type
                && SUPERSESSION_TYPES.contains(&event_type_value.as_str())
            {
                let mut sup_stmt = tx
                    .prepare(
                        "SELECT id, content, embedding FROM memories
                         WHERE event_type = ?1
                           AND id != ?2
                           AND superseded_by_id IS NULL
                           AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                         ORDER BY created_at DESC LIMIT 10",
                    )
                    .context("failed to prepare supersession query")?;

                let sup_rows = sup_stmt
                    .query_map(params![event_type_value, &id_for_store], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<Vec<u8>>>(2).ok().flatten(),
                        ))
                    })
                    .context("failed to execute supersession query")?;

                let emb_data: Vec<f32> = serde_json::from_slice(&embedding).unwrap_or_default();
                for row in sup_rows {
                    let (candidate_id, candidate_content, candidate_emb) =
                        row.context("failed to decode supersession row")?;

                    // Primary signal: cosine similarity (catches semantic overlap
                    // even when wording changes significantly)
                    let cosine_ok = if let Some(emb_blob) = candidate_emb
                        && let Ok(candidate_embedding) =
                            serde_json::from_slice::<Vec<f32>>(&emb_blob)
                    {
                        let cosine = cosine_similarity(&emb_data, &candidate_embedding);
                        cosine >= SUPERSESSION_COSINE_THRESHOLD
                    } else {
                        false // No embedding = cannot supersede
                    };
                    if !cosine_ok {
                        continue;
                    }

                    // Secondary signal: Jaccard word overlap (prevents cross-topic
                    // false matches from cosine alone)
                    let jaccard = jaccard_similarity(&data, &candidate_content, 3);
                    if jaccard < SUPERSESSION_JACCARD_THRESHOLD {
                        continue;
                    }

                    superseded_ids.push(candidate_id);
                }
                drop(sup_stmt);
            }

            tx.execute(
                "INSERT INTO memories (
                    id,
                    content,
                    embedding,
                    parent_id,
                    event_at,
                    content_hash,
                    source_type,
                    last_accessed_at,
                    tags,
                    importance,
                    metadata,
                    session_id,
                    event_type,
                    project,
                    priority,
                    entity_id,
                    agent_type,
                    ttl_seconds,
                    canonical_hash
                ) VALUES (
                    ?1,
                    ?2,
                    ?3,
                    NULL,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?4,
                    'cli_input',
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    ?5,
                    ?6,
                    ?7,
                    ?8,
                    ?9,
                    ?10,
                    ?11,
                    ?12,
                    ?13,
                    ?14,
                    ?15
                )
                ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    embedding = excluded.embedding,
                    content_hash = excluded.content_hash,
                    source_type = excluded.source_type,
                    tags = excluded.tags,
                    importance = excluded.importance,
                    metadata = excluded.metadata,
                    session_id = excluded.session_id,
                    event_type = excluded.event_type,
                    project = excluded.project,
                    priority = excluded.priority,
                    entity_id = excluded.entity_id,
                    agent_type = excluded.agent_type,
                    ttl_seconds = excluded.ttl_seconds,
                    canonical_hash = excluded.canonical_hash,
                    last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![
                    id_for_store,
                    data,
                    embedding,
                    c_hash,
                    tags_json,
                    importance,
                    metadata_json,
                    session_id,
                    event_type,
                    project,
                    priority,
                    entity_id,
                    agent_type,
                    ttl_seconds,
                    normalized_hash,
                ],
            )
            .context("failed to insert memory")?;

            tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id_for_store])
                .context("failed to delete existing FTS row during store")?;
            tx.execute(
                "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                params![id_for_store, data],
            )
            .context("failed to insert FTS row during store")?;

            let now_str: String = tx
                .query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                    row.get::<_, String>(0)
                })
                .context("failed to get current timestamp from sqlite")?;

            // Determine a single canonical chain_id for all superseded memories
            let mut canonical_chain_id: Option<String> = None;
            let mut other_chain_ids: Vec<String> = Vec::new();
            for old_id in &superseded_ids {
                tx.execute(
                    "UPDATE memories SET superseded_by_id = ?1, superseded_at = ?2
                     WHERE id = ?3 AND superseded_by_id IS NULL",
                    params![id_for_store, now_str, old_id],
                )
                .context("failed to mark memory as superseded")?;

                let old_chain_id: Option<String> = tx
                    .query_row(
                        "SELECT version_chain_id FROM memories WHERE id = ?1",
                        params![old_id],
                        |row| row.get(0),
                    )
                    .optional()
                    .context("failed to query old chain_id")?
                    .flatten();
                match (&canonical_chain_id, &old_chain_id) {
                    (None, Some(chain)) => canonical_chain_id = Some(chain.clone()),
                    (None, None) => canonical_chain_id = Some(old_id.clone()),
                    (Some(canonical), Some(chain)) if chain != canonical => {
                        other_chain_ids.push(chain.clone());
                    }
                    _ => {}
                }
            }

            let chain_id = canonical_chain_id.unwrap_or_else(|| id_for_store.clone());

            // Merge any divergent chains into the canonical one
            for other_chain in &other_chain_ids {
                tx.execute(
                    "UPDATE memories SET version_chain_id = ?1 WHERE version_chain_id = ?2",
                    params![chain_id, other_chain],
                )
                .context("failed to merge version chains")?;
            }

            // Set chain_id on old memories that had none
            for old_id in &superseded_ids {
                tx.execute(
                    "UPDATE memories SET version_chain_id = ?1 WHERE id = ?2 AND version_chain_id IS NULL",
                    params![chain_id, old_id],
                )
                .context("failed to set chain_id on old memory")?;
            }

            // Set chain_id on the new memory
            tx.execute(
                "UPDATE memories SET version_chain_id = ?1 WHERE id = ?2",
                params![chain_id, id_for_store],
            )
            .context("failed to set chain_id on new memory")?;

            tx.commit().context("failed to commit sqlite transaction")?;
            Ok::<_, anyhow::Error>((StoreOutcome::Inserted, superseded_ids))
        })
        .await
        .context("spawn_blocking join error")??;

        if matches!(outcome, StoreOutcome::Inserted)
            && let Err(error) = self.try_auto_relate(&id).await
        {
            tracing::warn!(memory_id = %id, error = %error, "auto-relate failed");
        }

        for old_id in &superseded_ids {
            if let Err(error) = self
                .add_relationship(old_id, &id, "SUPERSEDES", 1.0, &serde_json::json!({}))
                .await
            {
                tracing::warn!(old_id = %old_id, new_id = %id, error = %error, "failed to create SUPERSEDES edge");
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Retriever for SqliteStorage {
    async fn retrieve(&self, id: &str) -> Result<String> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = lock_conn(&conn)?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start sqlite transaction")?;

            let content: Option<String> = tx
                .query_row(
                    "SELECT content FROM memories WHERE id = ?1",
                    params![id],
                    |row| row.get(0),
                )
                .optional()
                .context("failed to query memory content")?;

            let content = content.ok_or_else(|| anyhow!("memory not found for id={id}"))?;

            tx.execute(
                "UPDATE memories
                 SET
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                     access_count = access_count + 1
                 WHERE id = ?1",
                params![id],
            )
            .context("failed to update last_accessed_at")?;

            tx.commit().context("failed to commit sqlite transaction")?;
            Ok::<_, anyhow::Error>(content)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Deleter for SqliteStorage {
    async fn delete(&self, id: &str) -> Result<bool> {
        let conn = Arc::clone(&self.conn);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = lock_conn(&conn)?;
            let tx = conn
                .unchecked_transaction()
                .context("failed to start delete transaction")?;
            tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                .context("failed to delete memory from FTS index")?;
            let changes = tx
                .execute("DELETE FROM memories WHERE id = ?1", params![id])
                .context("failed to delete memory")?;
            tx.commit().context("failed to commit delete transaction")?;
            Ok::<_, anyhow::Error>(changes > 0)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Updater for SqliteStorage {
    async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        if input.content.is_none()
            && input.tags.is_none()
            && input.importance.is_none()
            && input.metadata.is_none()
            && input.event_type.is_none()
            && input.priority.is_none()
        {
            return Err(anyhow!(
                "at least one of content, tags, importance, metadata, event_type, or priority must be provided"
            ));
        }

        let tags_json = input
            .tags
            .as_ref()
            .map(|tags| serde_json::to_string(tags).context("failed to serialize tags"))
            .transpose()?;
        let metadata_json = input
            .metadata
            .as_ref()
            .map(|metadata| serde_json::to_string(metadata).context("failed to serialize metadata"))
            .transpose()?;
        let event_type = input.event_type.clone();
        let priority = input.priority;
        let importance = input.importance;
        let content = input.content.clone();

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let content_fields = match content.as_deref() {
                Some(new_content) => {
                    let hash = content_hash(new_content);
                    let canonical = canonical_hash(new_content);
                    let emb = serde_json::to_vec(&embedder.embed(new_content)?)
                        .context("failed to serialize embedding")?;
                    Some((new_content.to_string(), hash, canonical, emb))
                }
                None => None,
            };
            use rusqlite::types::Value as SqlValue;

            let conn = lock_conn(&conn)?;

            let mut set_clauses = Vec::new();
            let mut values: Vec<SqlValue> = Vec::new();
            let mut next_param_index = 2;

            if let Some((new_content, hash, canonical, embedding)) = &content_fields {
                set_clauses.push(format!("content = ?{next_param_index}"));
                values.push(SqlValue::Text(new_content.clone()));
                next_param_index += 1;

                set_clauses.push(format!("content_hash = ?{next_param_index}"));
                values.push(SqlValue::Text(hash.clone()));
                next_param_index += 1;

                set_clauses.push(format!("embedding = ?{next_param_index}"));
                values.push(SqlValue::Blob(embedding.clone()));
                next_param_index += 1;

                set_clauses.push(format!("canonical_hash = ?{next_param_index}"));
                values.push(SqlValue::Text(canonical.clone()));
                next_param_index += 1;
            }

            if let Some(new_tags) = &tags_json {
                set_clauses.push(format!("tags = ?{next_param_index}"));
                values.push(SqlValue::Text(new_tags.clone()));
                next_param_index += 1;
            }

            if let Some(new_importance) = importance {
                set_clauses.push(format!("importance = ?{next_param_index}"));
                values.push(SqlValue::Real(new_importance));
                next_param_index += 1;
            }

            if let Some(new_metadata) = &metadata_json {
                set_clauses.push(format!("metadata = ?{next_param_index}"));
                values.push(SqlValue::Text(new_metadata.clone()));
                next_param_index += 1;
            }

            if let Some(new_event_type) = &event_type {
                set_clauses.push(format!("event_type = ?{next_param_index}"));
                values.push(SqlValue::Text(new_event_type.clone()));
                next_param_index += 1;
            }

            if let Some(new_priority) = priority {
                set_clauses.push(format!("priority = ?{next_param_index}"));
                values.push(SqlValue::Integer(i64::from(new_priority)));
            }

            let sql = format!(
                "UPDATE memories SET {},
                 last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                set_clauses.join(", ")
            );

            let mut params: Vec<&dyn rusqlite::types::ToSql> = Vec::with_capacity(values.len() + 1);
            params.push(&id);
            for value in &values {
                params.push(value);
            }

            let tx = conn
                .unchecked_transaction()
                .context("failed to start update transaction")?;

            let changes = tx
                .execute(&sql, params.as_slice())
                .context("failed to update memory")?;

            if changes == 0 {
                return Err(anyhow!("memory not found for id={id}"));
            }

            if let Some((new_content, _, _, _)) = &content_fields {
                tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to delete existing FTS row during update")?;
                tx.execute(
                    "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                    params![id, new_content],
                )
                .context("failed to insert FTS row during update")?;
            }

            tx.commit().context("failed to commit update transaction")?;

            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(())
    }
}

#[async_trait]
impl Tagger for SqliteStorage {
    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if tags.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let tags = tags.to_vec();
        let effective_limit = i64::try_from(limit).context("tag search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            let conn = lock_conn(&conn)?;

            // Build dynamic WHERE clause with dual-read support:
            // - JSON tags: json_valid + json_each
            // - Legacy CSV tags: instr-based comma-delimited matching
            let mut json_conditions = Vec::new();
            let mut csv_conditions = Vec::new();
            let mut param_values: Vec<String> = Vec::new();
            for (i, tag) in tags.iter().enumerate() {
                let p = i + 1;
                json_conditions.push(format!(
                    "EXISTS (SELECT 1 FROM json_each(memories.tags) WHERE value = ?{p})"
                ));
                csv_conditions.push(format!(
                    "instr(',' || memories.tags || ',', ',' || ?{p} || ',') > 0"
                ));
                param_values.push(tag.clone());
            }
            let json_clause = json_conditions.join(" AND ");
            let csv_clause = csv_conditions.join(" AND ");
            let mut sql = format!(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project FROM memories \
                 WHERE ((json_valid(memories.tags) AND {json_clause}) \
                         OR (NOT json_valid(memories.tags) AND memories.tags != '' AND {csv_clause})) \
                 "
            );

            let mut next_idx = param_values.len();
            if let Some(event_type) = opts.event_type.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND event_type = ?{next_idx}"));
                param_values.push(event_type);
            }
            if let Some(project) = opts.project.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND project = ?{next_idx}"));
                param_values.push(project);
            }
            if let Some(session_id) = opts.session_id.clone() {
                next_idx += 1;
                sql.push_str(&format!(" AND session_id = ?{next_idx}"));
                param_values.push(session_id);
            }
            next_idx += 1;
            sql.push_str(&format!(" ORDER BY last_accessed_at DESC LIMIT ?{next_idx}"));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare tag search query")?;

            let mut param_refs: Vec<&dyn rusqlite::types::ToSql> = Vec::new();
            for v in &param_values {
                param_refs.push(v);
            }
            param_refs.push(&effective_limit);

            let rows = stmt
                .query_map(param_refs.as_slice(), search_result_from_row)
                .context("failed to execute tag search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode tag search row")?);
            }
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
