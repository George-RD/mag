use super::*;
use crate::memory_core::storage::sqlite::pipeline::scoring::bump_token_cache_gen;
use crate::memory_core::{EmbeddingInputKind, EmbeddingModel, REL_PRECEDED_BY};

#[async_trait]
impl Storage for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        self.store_internal(id, data, input, None).await
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
        bump_token_cache_gen();
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
                    let emb = encode_embedding(
                        &embedder.embed_for(EmbeddingInputKind::Document, new_content)?,
                    );
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
        bump_token_cache_gen();
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

/// Per-item outcome collected from a single-transaction batch insert.
#[derive(Debug)]
struct BatchItemResult {
    pub id: String,
    pub outcome: StoreOutcome,
    pub superseded_ids: Vec<String>,
    pub final_tags: Vec<String>,
    pub session_id: Option<String>,
}
impl SqliteStorage {
    pub(crate) async fn store_internal(
        &self,
        id: &str,
        data: &str,
        input: &MemoryInput,
        precomputed_embedding: Option<Vec<f32>>,
    ) -> Result<()> {
        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        let post_id = id.to_string();
        let post_input = input.clone();
        let id = id.to_string();
        let data = data.to_string();
        let input = input.clone();

        let (outcome, superseded_ids, final_tags) = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start sqlite transaction")?;
            let result = Self::store_one_in_tx(
                &tx,
                embedder.as_ref(),
                &id,
                &data,
                &input,
                precomputed_embedding,
            )?;
            tx.commit().context("failed to commit sqlite transaction")?;
            drop(conn);
            pool.note_write();
            Ok::<_, anyhow::Error>(result)
        })
        .await
        .context("spawn_blocking join error")??;

        let persisted_id = match &outcome {
            StoreOutcome::Inserted => post_id.clone(),
            StoreOutcome::Deduped { existing_id } => existing_id.clone(),
        };
        self.finish_store(
            &persisted_id,
            &post_input,
            &outcome,
            &superseded_ids,
            &final_tags,
        )
        .await;
        Ok(())
    }

    pub(super) async fn finish_store(
        &self,
        post_id: &str,
        post_input: &MemoryInput,
        outcome: &StoreOutcome,
        superseded_ids: &[String],
        final_tags: &[String],
    ) {
        let invalidation_event_type = event_type_to_sql(&post_input.event_type);
        self.invalidate_cache_selective(
            invalidation_event_type.as_deref(),
            post_input.project.as_deref(),
            post_input.session_id.as_deref(),
        );
        self.refresh_hot_cache_best_effort();

        if matches!(outcome, &StoreOutcome::Inserted) {
            if let Err(error) = self.try_auto_relate(post_id).await {
                tracing::warn!(memory_id = %post_id, error = %error, "auto-relate failed");
            }

            if let Some(ref session_id) = post_input.session_id
                && let Err(error) = self.try_create_temporal_edges(post_id, session_id).await
            {
                tracing::warn!(memory_id = %post_id, error = %error, "temporal edge creation failed");
            }

            if !final_tags.is_empty()
                && let Err(error) = self.try_create_entity_edges(post_id, final_tags).await
            {
                tracing::warn!(memory_id = %post_id, error = %error, "entity edge creation failed");
            }
        }

        for old_id in superseded_ids {
            if let Err(error) = self
                .add_relationship(old_id, post_id, "SUPERSEDES", 1.0, &serde_json::json!({}))
                .await
            {
                tracing::warn!(old_id = %old_id, new_id = %post_id, error = %error, "failed to create SUPERSEDES edge");
            }
        }
        bump_token_cache_gen();
    }

    pub(super) fn store_one_in_tx(
        tx: &rusqlite::Transaction<'_>,
        embedder: &dyn EmbeddingModel,
        id: &str,
        data: &str,
        input: &MemoryInput,
        precomputed_embedding: Option<Vec<f32>>,
    ) -> Result<(StoreOutcome, Vec<String>, Vec<String>)> {
        let tags_json =
            serde_json::to_string(&input.tags).context("failed to serialize tags to JSON")?;
        let metadata_json = serde_json::to_string(&input.metadata)
            .context("failed to serialize metadata to JSON")?;

        let event_type = event_type_to_sql(&input.event_type);
        let event_type_enum = input.event_type.clone();
        let session_id = input.session_id.clone();
        let project = input.project.clone();
        let importance = input.importance;
        let priority = input.priority;
        let entity_id = input.entity_id.clone();
        let agent_type = input.agent_type.clone();
        let ttl_seconds = input.ttl_seconds;
        let referenced_date = input.referenced_date.clone();
        let source_type = input.source_type.clone();
        let id_for_store = id.to_string();

        let c_hash = content_hash(data);
        let normalized_hash = canonical_hash(data);

        // ── Phase 1: Combined canonical-hash + Jaccard dedup (single query) ──
        let jaccard_threshold = event_type_enum.as_ref().and_then(|et| et.dedup_threshold());
        let need_jaccard = jaccard_threshold.is_some();

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

        if let Some(existing_id) = canonical_dedup_id {
            tx.execute(
                "UPDATE memories
                 SET access_count = access_count + 1,
                     last_accessed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
                params![&existing_id],
            )
            .context("failed to update access_count for canonical dedup")?;
            return Ok((
                StoreOutcome::Deduped { existing_id },
                Vec::new(),
                Vec::new(),
            ));
        }

        if let Some(threshold) = jaccard_threshold {
            let matched_id = jaccard_candidates.iter().find_map(|(cid, ccontent)| {
                let similarity = jaccard_similarity(data, ccontent, 3);
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
                    params![&existing_id],
                )
                .context("failed to update access_count for Jaccard dedup")?;
                return Ok((
                    StoreOutcome::Deduped { existing_id },
                    Vec::new(),
                    Vec::new(),
                ));
            }
        }

        // ── Phase 2: Embedding ──
        let embedding_vec = match precomputed_embedding {
            Some(vec) => vec,
            None => embedder.embed_for(EmbeddingInputKind::Document, data)?,
        };
        let embedding = encode_embedding(&embedding_vec);

        // ── Phase 3: Supersession detection ──
        let mut superseded_ids: Vec<String> = Vec::new();
        if let Some(ref event_type_value) = event_type
            && event_type_enum
                .as_ref()
                .is_some_and(|et| et.is_supersession_type())
        {
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
                let cosine_ok = if let Some(emb_blob) = candidate_emb
                    && let Ok(candidate_embedding) = decode_embedding(emb_blob)
                {
                    let cosine = dot_product(emb_data, &candidate_embedding);
                    cosine >= SUPERSESSION_COSINE_THRESHOLD
                } else {
                    false
                };
                if !cosine_ok {
                    continue;
                }

                let jaccard = jaccard_similarity(data, candidate_content, 3);
                if jaccard < SUPERSESSION_JACCARD_THRESHOLD {
                    continue;
                }

                superseded_ids.push(candidate_id.clone());
            }
            drop(sup_stmt);
        }

        // ── Phase 4: INSERT memory + FTS5 sync ──
        let event_at_value: Option<String> = referenced_date
            .clone()
            .filter(|date| validate_iso8601(date));

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
                ?17,
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
                source_type.as_deref().unwrap_or("cli_input"),
            ],
        )
        .context("failed to insert memory")?;

        tx.execute(
            "DELETE FROM memories_fts WHERE id = ?1",
            params![id_for_store],
        )
        .context("failed to delete existing FTS row during store")?;
        tx.execute(
            "INSERT INTO memories_fts(id, content) VALUES (?1, ?2)",
            params![id_for_store, data],
        )
        .context("failed to insert FTS row during store")?;

        // ── Phase 4b: Entity extraction ──
        let entity_tags = super::entities::extract_entities(data);
        let final_tags: Vec<String> = if !entity_tags.is_empty() {
            let mut current_tags: Vec<String> =
                serde_json::from_str(&tags_json).unwrap_or_default();
            for etag in &entity_tags {
                if !current_tags.contains(etag) {
                    current_tags.push(etag.clone());
                }
            }
            let merged_json =
                serde_json::to_string(&current_tags).unwrap_or_else(|_| tags_json.clone());
            tx.execute(
                "UPDATE memories SET tags = ?1 WHERE id = ?2",
                params![merged_json, id_for_store],
            )
            .context("failed to update memory with entity tags")?;
            current_tags
        } else {
            serde_json::from_str(&tags_json).unwrap_or_default()
        };

        #[cfg(feature = "sqlite-vec")]
        vec_upsert(tx, &id_for_store, &embedding)?;

        // ── Phase 5: Batched supersession chain management ──
        if !superseded_ids.is_empty() {
            let now_str = chrono::Utc::now()
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();

            let mut canonical_chain_id: Option<String> = None;
            let mut other_chain_ids: Vec<String> = Vec::new();

            let in_clause: String = (0..superseded_ids.len())
                .map(|i| format!("?{}", i + 3))
                .collect::<Vec<_>>()
                .join(", ");

            let batch_update_sql = format!(
                "UPDATE memories
                 SET superseded_by_id = ?1, superseded_at = ?2
                 WHERE id IN ({in_clause}) AND superseded_by_id IS NULL
                 RETURNING id, version_chain_id"
            );

            let mut param_values: Vec<rusqlite::types::Value> =
                Vec::with_capacity(superseded_ids.len() + 2);
            param_values.push(rusqlite::types::Value::Text(id_for_store.clone()));
            param_values.push(rusqlite::types::Value::Text(now_str.clone()));
            for old_id in &superseded_ids {
                param_values.push(rusqlite::types::Value::Text(old_id.clone()));
            }

            {
                let param_refs = to_param_refs(&param_values);
                let mut stmt = tx.prepare(&batch_update_sql)?;
                let mut rows = stmt.query(param_refs.as_slice())?;

                while let Some(row) = rows.next()? {
                    let old_id: String = row.get(0)?;
                    let old_chain_id: Option<String> = row.get(1)?;

                    match (&canonical_chain_id, &old_chain_id) {
                        (None, Some(chain)) => canonical_chain_id = Some(chain.clone()),
                        (None, None) => canonical_chain_id = Some(old_id),
                        (Some(canonical), Some(chain)) if chain != canonical => {
                            other_chain_ids.push(chain.clone());
                        }
                        _ => {}
                    }
                }
            }

            let chain_id = canonical_chain_id.unwrap_or_else(|| id_for_store.clone());

            for other_chain in &other_chain_ids {
                tx.execute(
                    "UPDATE memories SET version_chain_id = ?1 WHERE version_chain_id = ?2",
                    params![chain_id, other_chain],
                )
                .context("failed to merge version chains")?;
            }

            let id_placeholders: String = (0..superseded_ids.len())
                .map(|i| format!("?{}", i + 3))
                .collect::<Vec<_>>()
                .join(", ");
            let batch_sql = format!(
                "UPDATE memories SET version_chain_id = ?1
                 WHERE (id IN ({id_placeholders}) AND version_chain_id IS NULL)
                    OR id = ?2"
            );
            let mut param_values: Vec<rusqlite::types::Value> =
                Vec::with_capacity(superseded_ids.len() + 2);
            param_values.push(rusqlite::types::Value::Text(chain_id));
            param_values.push(rusqlite::types::Value::Text(id_for_store.clone()));
            for old_id in &superseded_ids {
                param_values.push(rusqlite::types::Value::Text(old_id.clone()));
            }
            let param_refs = to_param_refs(&param_values);
            tx.execute(&batch_sql, param_refs.as_slice())
                .context("failed to batch-set version chain ids")?;
        }

        Ok((StoreOutcome::Inserted, superseded_ids, final_tags))
    }

    /// Batch store multiple memories inside a single SQLite transaction.
    ///
    /// Pre-computes embeddings with one batched `embed_batch_for()` call, then inserts
    /// every item through [`Self::store_one_in_tx`] within a single transaction.
    /// This eliminates the per-item transaction overhead — and the mid-batch WAL
    /// auto-checkpoint / FTS5 segment-merge stalls — that caused pathological
    /// multi-minute hangs on large batches (issue #342). A single passive WAL
    /// checkpoint after commit flushes the accumulated WAL without blocking readers.
    ///
    /// Atomic: if any item fails, the whole batch rolls back. Graph-edge creation
    /// and cache invalidation run once after the transaction commits.
    pub async fn store_batch(&self, items: &[(String, String, MemoryInput)]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        // ── Phase 1: Batched embedding inference (results returned in input order). ──
        let contents: Vec<String> = items.iter().map(|(_, data, _)| data.clone()).collect();
        let embedder = Arc::clone(&self.embedder);
        let embeddings = tokio::task::spawn_blocking(move || {
            let refs: Vec<&str> = contents.iter().map(|s| s.as_str()).collect();
            embedder.embed_batch_for(EmbeddingInputKind::Document, &refs)
        })
        .await
        .context("spawn_blocking join error for embed_batch")??;

        // The all-or-nothing batch contract requires exactly one embedding per
        // item; a backend returning a different count would make the zip below
        // silently truncate, committing a prefix and dropping the rest. Fail fast.
        anyhow::ensure!(
            embeddings.len() == items.len(),
            "embed_batch returned {} embeddings for {} batch items",
            embeddings.len(),
            items.len()
        );

        // ── Phase 2: Insert all items inside one transaction. ──
        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        let owned_items: Vec<(String, String, MemoryInput)> = items.to_vec();
        let (results, session_tails) = tokio::task::spawn_blocking(move || {
            let conn = pool.writer()?;
            let tx = retry_on_lock(|| conn.unchecked_transaction())
                .context("failed to start sqlite batch transaction")?;

            // Snapshot each session's pre-batch temporal tail (most recent memory)
            // BEFORE inserting any batch row, so deferred temporal-edge creation can
            // chain batch items in input order instead of picking a later batch row
            // as a predecessor (parity with the per-item store() path).
            let mut session_tails: HashMap<String, String> = HashMap::new();
            let mut queried_sessions: HashSet<String> = HashSet::new();
            for (_, _, input) in &owned_items {
                if let Some(sid) = &input.session_id
                    && queried_sessions.insert(sid.clone())
                {
                    let tail: Option<String> = tx
                        .query_row(
                            "SELECT id FROM memories WHERE session_id = ?1 \
                             ORDER BY created_at DESC LIMIT 1",
                            params![sid],
                            |row| row.get(0),
                        )
                        .optional()
                        .context("failed to snapshot pre-batch session tail")?;
                    if let Some(tail_id) = tail {
                        session_tails.insert(sid.clone(), tail_id);
                    }
                }
            }

            let mut results: Vec<BatchItemResult> = Vec::with_capacity(owned_items.len());
            for ((id, data, input), embedding) in owned_items.iter().zip(embeddings) {
                let (outcome, superseded_ids, final_tags) = Self::store_one_in_tx(
                    &tx,
                    embedder.as_ref(),
                    id,
                    data,
                    input,
                    Some(embedding),
                )?;
                results.push(BatchItemResult {
                    id: id.clone(),
                    outcome,
                    superseded_ids,
                    final_tags,
                    session_id: input.session_id.clone(),
                });
            }

            tx.commit()
                .context("failed to commit sqlite batch transaction")?;

            // Flush the WAL accumulated by the batch with a single passive checkpoint
            // (does not block readers). No-op for in-memory databases.
            pool.checkpoint_passive(&conn);

            Ok::<_, anyhow::Error>((results, session_tails))
        })
        .await
        .context("spawn_blocking join error for store_batch")??;

        // ── Phase 3: Deferred post-processing after commit. ──
        // A batch may span multiple (event_type, project, session) dimensions;
        // a single full cache clear is simpler and correct for bulk writes.
        self.invalidate_query_cache();
        self.refresh_hot_cache_best_effort();

        // Chain temporal edges in input order, anchored to each session's
        // pre-batch tail. `session_prev` tracks the last inserted memory per session.
        let mut session_prev: HashMap<String, String> = session_tails;

        for result in &results {
            if matches!(result.outcome, StoreOutcome::Inserted) {
                if let Err(error) = self.try_auto_relate(&result.id).await {
                    tracing::warn!(memory_id = %result.id, error = %error, "auto-relate failed");
                }

                if let Some(ref sid) = result.session_id {
                    if let Some(pred_id) = session_prev.get(sid).cloned()
                        && let Err(e) = self
                            .add_relationship(
                                &pred_id,
                                &result.id,
                                REL_PRECEDED_BY,
                                1.0,
                                &serde_json::json!({"source": "temporal_adjacency"}),
                            )
                            .await
                    {
                        tracing::warn!(memory_id = %result.id, error = %e, "temporal edge creation failed");
                    }
                    session_prev.insert(sid.clone(), result.id.clone());
                }

                if !result.final_tags.is_empty()
                    && let Err(e) = self
                        .try_create_entity_edges(&result.id, &result.final_tags)
                        .await
                {
                    tracing::warn!(memory_id = %result.id, error = %e, "entity edge creation failed");
                }
            }

            for old_id in &result.superseded_ids {
                if let Err(error) = self
                    .add_relationship(
                        old_id,
                        &result.id,
                        "SUPERSEDES",
                        1.0,
                        &serde_json::json!({}),
                    )
                    .await
                {
                    tracing::warn!(old_id = %old_id, new_id = %result.id, error = %error, "failed to create SUPERSEDES edge");
                }
            }
        }
        bump_token_cache_gen();

        Ok(())
    }
}
