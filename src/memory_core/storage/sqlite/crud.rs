use super::*;

#[async_trait]
impl Storage for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        let entity_tags = crate::memory_core::entity::extract_entity_tags(data);
        let tags_json = if entity_tags.is_empty() {
            serde_json::to_string(&input.tags).context("failed to serialize tags to JSON")?
        } else {
            let mut tags = input.tags.clone();
            tags.extend(entity_tags);
            serde_json::to_string(&tags).context("failed to serialize tags to JSON")?
        };
        let metadata_json = serde_json::to_string(&input.metadata)
            .context("failed to serialize metadata to JSON")?;

        // Capture filter dimensions for selective cache invalidation.
        let invalidation_event_type = event_type_to_sql(&input.event_type);
        let invalidation_project = input.project.clone();
        let invalidation_session_id = input.session_id.clone();

        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();
        let data = data.to_string();
        let importance = input.importance;
        let event_type = invalidation_event_type.clone();
        let event_type_enum = input.event_type.clone();
        let session_id = input.session_id.clone();
        let project = input.project.clone();
        let priority = input.priority;
        let entity_id = input.entity_id.clone();
        let agent_type = input.agent_type.clone();
        let ttl_seconds = input.ttl_seconds;
        let referenced_date = input.referenced_date.clone();
        let id_for_store = id.clone();

        let (outcome, superseded_ids) = tokio::task::spawn_blocking(move || {
            let c_hash = content_hash(&data);
            let normalized_hash = canonical_hash(&data);
            let conn = pool.writer()?;
            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start sqlite transaction")?;

            // ── Phase 1: Combined canonical-hash + Jaccard dedup (single query) ──
            //
            // Fetch the canonical-hash match (if any) AND Jaccard candidates in one
            // CTE-based round-trip, eliminating the previous two separate SELECTs.
            let jaccard_threshold = event_type_enum.as_ref().and_then(|et| et.dedup_threshold());
            // We only need Jaccard candidates when a threshold exists for this event type.
            let need_jaccard = jaccard_threshold.is_some();

            // result_kind: 'canonical' for a canonical-hash hit, 'jaccard' for a candidate row
            let mut dedup_stmt = tx
                .prepare(
                    "WITH canonical_hit AS (
                         SELECT id, 'canonical' AS kind
                         FROM memories
                         WHERE canonical_hash = ?1
                           AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                         LIMIT 1
                     ),
                     jaccard_candidates AS (
                         SELECT id, content
                         FROM memories
                         WHERE ?2 AND event_type = ?3
                           AND NOT EXISTS (SELECT 1 FROM canonical_hit)
                           AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                         ORDER BY created_at DESC
                         LIMIT 5
                     )
                     SELECT kind, id, NULL AS content FROM canonical_hit
                     UNION ALL
                     SELECT 'jaccard' AS kind, id, content FROM jaccard_candidates",
                )
                .context("failed to prepare combined dedup query")?;

            let event_type_param = event_type.as_deref().unwrap_or("");
            let dedup_rows = dedup_stmt
                .query_map(
                    params![normalized_hash, need_jaccard, event_type_param],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .context("failed to execute combined dedup query")?;

            let mut canonical_dedup_id: Option<String> = None;
            let mut jaccard_candidates: Vec<(String, String)> = Vec::new();
            for row in dedup_rows {
                let (kind, row_id, content) = row.context("failed to decode combined dedup row")?;
                match kind.as_str() {
                    "canonical" => {
                        canonical_dedup_id = Some(row_id);
                    }
                    _ => {
                        if let Some(c) = content {
                            jaccard_candidates.push((row_id, c));
                        }
                    }
                }
            }
            drop(dedup_stmt);

            // Canonical-hash early return (cheapest dedup — skips embedding entirely)
            if let Some(existing_id) = canonical_dedup_id {
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

            // Jaccard dedup check (Rust-side similarity on pre-fetched candidates)
            if let Some(threshold) = jaccard_threshold {
                let matched_id = jaccard_candidates.iter().find_map(|(cid, ccontent)| {
                    let similarity = jaccard_similarity(&data, ccontent, 3);
                    if similarity >= threshold {
                        Some(cid.clone())
                    } else {
                        None
                    }
                });

                if let Some(existing_id) = matched_id {
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

            // ── Phase 2: Embedding (the real bottleneck, ~8ms) ──
            let embedding_vec = embedder.embed(&data)?;
            let embedding = encode_embedding(&embedding_vec);

            // ── Phase 3: Supersession detection ──
            let mut superseded_ids: Vec<String> = Vec::new();
            if let Some(ref event_type_value) = event_type
                && event_type_enum.as_ref().is_some_and(|et| et.is_supersession_type())
            {
                // Build supersession query with optional entity_id narrowing
                let entity_narrowing = if entity_id.is_some() {
                    " AND entity_id = ?3"
                } else {
                    ""
                };
                let sup_sql = format!(
                    "SELECT id, content, embedding FROM memories
                     WHERE event_type = ?1
                       AND id != ?2
                       AND superseded_by_id IS NULL
                       AND (ttl_seconds IS NULL OR datetime(created_at, '+' || ttl_seconds || ' seconds') > datetime('now'))
                       {entity_narrowing}
                     ORDER BY created_at DESC LIMIT 10"
                );
                let mut sup_stmt = tx
                    .prepare(&sup_sql)
                    .context("failed to prepare supersession query")?;

                let row_mapper = |row: &rusqlite::Row<'_>| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<Vec<u8>>>(2).ok().flatten(),
                    ))
                };
                let sup_candidates: Vec<(String, String, Option<Vec<u8>>)> =
                    if let Some(ref eid) = entity_id {
                        sup_stmt
                            .query_map(params![event_type_value, &id_for_store, eid], row_mapper)
                            .context("failed to execute supersession query")?
                            .collect::<Result<Vec<_>, _>>()
                            .context("failed to decode supersession rows")?
                    } else {
                        sup_stmt
                            .query_map(params![event_type_value, &id_for_store], row_mapper)
                            .context("failed to execute supersession query")?
                            .collect::<Result<Vec<_>, _>>()
                            .context("failed to decode supersession rows")?
                    };

                let emb_data = &embedding_vec;
                for (candidate_id, candidate_content, candidate_emb) in &sup_candidates {

                    // Primary signal: cosine similarity (catches semantic overlap
                    // even when wording changes significantly)
                    let cosine_ok = if let Some(emb_blob) = candidate_emb
                        && let Ok(candidate_embedding) =
                            decode_embedding(emb_blob)
                    {
                        let cosine = cosine_similarity(emb_data, &candidate_embedding);
                        cosine >= SUPERSESSION_COSINE_THRESHOLD
                    } else {
                        false // No embedding = cannot supersede
                    };
                    if !cosine_ok {
                        continue;
                    }

                    // Secondary signal: Jaccard word overlap (prevents cross-topic
                    // false matches from cosine alone)
                    let jaccard = jaccard_similarity(&data, candidate_content, 3);
                    if jaccard < SUPERSESSION_JACCARD_THRESHOLD {
                        continue;
                    }

                    superseded_ids.push(candidate_id.clone());
                }
                drop(sup_stmt);
            }

            // Use referenced_date for event_at when provided, otherwise default to now.
            let event_at_value: Option<String> = referenced_date.clone().and_then(|d| {
                if validate_iso8601(&d) { Some(d) } else { None }
            });

            // ── Phase 4: INSERT memory + FTS5 sync ──
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
                    COALESCE(?16, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
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
                    event_at = excluded.event_at,
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
                    event_at_value,
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

            #[cfg(feature = "sqlite-vec")]
            vec_upsert(&tx, &id_for_store, &embedding)?;

            // ── Phase 5: Batched supersession chain management ──
            if !superseded_ids.is_empty() {
                let now_str =
                    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

                // Batch-mark all superseded memories and collect their chain_ids in
                // a single pass (replaces per-id UPDATE + SELECT pair).
                let mut canonical_chain_id: Option<String> = None;
                let mut other_chain_ids: Vec<String> = Vec::new();

                for old_id in &superseded_ids {
                    // UPDATE ... RETURNING merges the mark + chain_id fetch into one
                    // round-trip per row. SQLite 3.35+ required (bundled ≥ 3.45).
                    let old_chain_id: Option<String> = tx
                        .query_row(
                            "UPDATE memories
                             SET superseded_by_id = ?1, superseded_at = ?2
                             WHERE id = ?3 AND superseded_by_id IS NULL
                             RETURNING version_chain_id",
                            params![id_for_store, now_str, old_id],
                            |row| row.get(0),
                        )
                        .optional()
                        .context("failed to mark memory as superseded")?
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

                let chain_id =
                    canonical_chain_id.unwrap_or_else(|| id_for_store.clone());

                // Merge divergent chains into the canonical one
                for other_chain in &other_chain_ids {
                    tx.execute(
                        "UPDATE memories SET version_chain_id = ?1 WHERE version_chain_id = ?2",
                        params![chain_id, other_chain],
                    )
                    .context("failed to merge version chains")?;
                }

                // Batch-set chain_id on old memories that had none + the new memory
                // in a single UPDATE using IN(...) + OR, replacing N+1 separate UPDATEs.
                let id_placeholders: String = (0..superseded_ids.len())
                    .map(|i| format!("?{}", i + 3))
                    .collect::<Vec<_>>()
                    .join(", ");
                let batch_sql = format!(
                    "UPDATE memories SET version_chain_id = ?1
                     WHERE (id IN ({id_placeholders}) AND version_chain_id IS NULL)
                        OR id = ?2"
                );
                let mut param_values: Vec<rusqlite::types::Value> = Vec::with_capacity(
                    superseded_ids.len() + 2,
                );
                param_values
                    .push(rusqlite::types::Value::Text(chain_id));
                param_values
                    .push(rusqlite::types::Value::Text(id_for_store.clone()));
                for old_id in &superseded_ids {
                    param_values.push(rusqlite::types::Value::Text(
                        old_id.clone(),
                    ));
                }
                let param_refs = to_param_refs(&param_values);
                tx.execute(&batch_sql, param_refs.as_slice())
                    .context("failed to batch-set version chain ids")?;
            }

            tx.commit().context("failed to commit sqlite transaction")?;
            drop(conn); // Release writer Mutex before note_write to avoid deadlock
            pool.note_write();
            Ok::<_, anyhow::Error>((StoreOutcome::Inserted, superseded_ids))
        })
        .await
        .context("spawn_blocking join error")??;

        self.invalidate_cache_selective(
            invalidation_event_type.as_deref(),
            invalidation_project.as_deref(),
            invalidation_session_id.as_deref(),
        );
        self.refresh_hot_cache_best_effort();

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
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = retry_on_lock(|| conn.unchecked_transaction())
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
        let pool = Arc::clone(&self.pool);
        let id = id.to_string();

        let deleted = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start delete transaction")?;
            tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                .context("failed to delete memory from FTS index")?;

            #[cfg(feature = "sqlite-vec")]
            vec_delete(&tx, &id)?;

            let changes = tx
                .execute("DELETE FROM memories WHERE id = ?1", params![id])
                .context("failed to delete memory")?;
            tx.commit().context("failed to commit delete transaction")?;
            drop(conn); // Release writer Mutex before note_write to avoid deadlock
            pool.note_write();
            Ok::<_, anyhow::Error>(changes > 0)
        })
        .await
        .context("spawn_blocking join error")??;

        self.invalidate_query_cache();
        self.refresh_hot_cache_best_effort();
        Ok(deleted)
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
        let event_type = event_type_to_sql(&input.event_type);
        let priority = input.priority;
        let importance = input.importance;
        let content = input.content.clone();

        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        let id = id.to_string();

        tokio::task::spawn_blocking(move || {
            let content_fields = match content.as_deref() {
                Some(new_content) => {
                    let hash = content_hash(new_content);
                    let canonical = canonical_hash(new_content);
                    let emb = encode_embedding(&embedder.embed(new_content)?);
                    Some((new_content.to_string(), hash, canonical, emb))
                }
                None => None,
            };
            use rusqlite::types::Value as SqlValue;

            let conn = pool.writer()?;

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

            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start update transaction")?;

            let changes = tx
                .execute(&sql, params.as_slice())
                .context("failed to update memory")?;

            if changes == 0 {
                return Err(anyhow!("memory not found for id={id}"));
            }

            if let Some((new_content, _, _, ref _embedding)) = content_fields {
                tx.execute("DELETE FROM memories_fts WHERE id = ?1", params![id])
                    .context("failed to delete existing FTS row during update")?;
                tx.execute(
                    "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
                    params![id, new_content],
                )
                .context("failed to insert FTS row during update")?;

                #[cfg(feature = "sqlite-vec")]
                vec_upsert(&tx, &id, _embedding)?;
            }

            tx.commit().context("failed to commit update transaction")?;
            drop(conn); // Release writer Mutex before note_write to avoid deadlock
            pool.note_write();

            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("spawn_blocking join error")??;

        self.invalidate_query_cache();
        self.refresh_hot_cache_best_effort();
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

        let pool = Arc::clone(&self.pool);
        let tags = tags.to_vec();
        let effective_limit = i64::try_from(limit).context("tag search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;

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
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type FROM memories \
                 WHERE ((json_valid(memories.tags) AND {json_clause}) \
                         OR (NOT json_valid(memories.tags) AND memories.tags != '' AND {csv_clause})) \
                 "
            );

            let mut next_idx = param_values.len();
            if let Some(ref event_type) = opts.event_type {
                next_idx += 1;
                sql.push_str(&format!(" AND event_type = ?{next_idx}"));
                param_values.push(event_type.to_string());
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

impl SqliteStorage {
    /// Batch store multiple memories with optimized embedding computation.
    ///
    /// Pre-warms the embedding LRU cache with a single `embed_batch()` call,
    /// then loops individual `store()` calls which hit the warm cache.
    ///
    /// Not atomic: if a store fails mid-batch, previously stored items remain committed.
    #[allow(dead_code)]
    pub async fn store_batch(&self, items: &[(String, String, MemoryInput)]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        // Phase 1: Pre-warm embedding cache with batched ONNX inference.
        let contents: Vec<String> = items.iter().map(|(_, data, _)| data.clone()).collect();
        let embedder = Arc::clone(&self.embedder);
        tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = contents.iter().map(|s| s.as_str()).collect();
            embedder.embed_batch(&refs)
        })
        .await
        .context("spawn_blocking join error for embed_batch")??;

        // Phase 2: Store each item (hits warm embedding cache).
        for (id, data, input) in items {
            <Self as Storage>::store(self, id, data, input).await?;
        }

        Ok(())
    }
}
