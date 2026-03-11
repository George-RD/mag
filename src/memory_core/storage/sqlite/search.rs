use super::*;
use crate::memory_core::storage::sqlite::helpers::append_context_tag_filters;

#[async_trait]
impl Searcher for SqliteStorage {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let query = query.to_string();
        let effective_limit = i64::try_from(limit).context("search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let conn = pool.reader()?;

            let fts_query = build_fts5_query(&query);
            let include_superseded = opts.include_superseded.unwrap_or(false);
            let mut fts_sql = String::from(
                "SELECT f.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project
                 FROM memories_fts f
                 JOIN memories m ON m.id = f.id
                 WHERE memories_fts MATCH ?1",
            );
            if !include_superseded {
                fts_sql.push_str(" AND m.superseded_by_id IS NULL");
            }
            let mut fts_params: Vec<SqlValue> = vec![SqlValue::Text(fts_query)];
            let mut param_idx = 2;
            append_search_filters(&mut fts_sql, &mut fts_params, &mut param_idx, &opts, "m.");
            append_context_tag_filters(
                &mut fts_sql,
                &mut fts_params,
                &mut param_idx,
                opts.context_tags.as_deref(),
                "m.tags",
            );
            fts_sql.push_str(" ORDER BY bm25(memories_fts)");
            fts_sql.push_str(&format!(" LIMIT ?{param_idx}"));
            fts_params.push(SqlValue::Integer(effective_limit));

            let fts_result = conn.prepare(&fts_sql);

            if let Ok(mut stmt) = fts_result {
                let fts_param_refs = to_param_refs(&fts_params);
                let rows = stmt
                    .query_map(fts_param_refs.as_slice(), search_result_from_row);

                if let Err(ref e) = rows {
                    tracing::warn!("FTS5 search failed, falling back to LIKE: {e}");
                }
                if let Ok(rows) = rows {
                    let mut results = Vec::new();
                    for row in rows {
                        results.push(row.context("failed to decode FTS5 search row")?);
                    }

                    if !results.is_empty() {
                        return Ok(results);
                    }
                }
            }

            let pattern = escape_like_pattern(&query);

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE lower(content) LIKE ?1 ESCAPE '\\'",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut params_values: Vec<SqlValue> = vec![SqlValue::Text(pattern)];
            let mut idx = 2;
            append_search_filters(&mut sql, &mut params_values, &mut idx, &opts, "");
            append_context_tag_filters(
                &mut sql,
                &mut params_values,
                &mut idx,
                opts.context_tags.as_deref(),
                "tags",
            );
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare LIKE search query")?;

            let like_param_refs = to_param_refs(&params_values);

            let rows = stmt
                .query_map(like_param_refs.as_slice(), search_result_from_row)
                .context("failed to execute LIKE search query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode LIKE search row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl Recents for SqliteStorage {
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let effective_limit = i64::try_from(limit).context("recent limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;
            let include_superseded = opts.include_superseded.unwrap_or(false);

            let conn = pool.reader()?;

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE 1 = 1",
            );
            let mut params_values: Vec<SqlValue> = Vec::new();
            let mut idx = 1;
            append_search_filters(&mut sql, &mut params_values, &mut idx, &opts, "");
            append_context_tag_filters(
                &mut sql,
                &mut params_values,
                &mut idx,
                opts.context_tags.as_deref(),
                "tags",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            sql.push_str(" ORDER BY last_accessed_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(effective_limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare recent query")?;

            let param_refs = to_param_refs(&params_values);

            let rows = stmt
                .query_map(param_refs.as_slice(), search_result_from_row)
                .context("failed to execute recent query")?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row.context("failed to decode recent row")?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl SemanticSearcher for SqliteStorage {
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        let query = query.to_string();
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            #[cfg(not(feature = "sqlite-vec"))]
            use rusqlite::types::Value as SqlValue;

            let include_superseded = opts.include_superseded.unwrap_or(false);
            let query_embedding = embedder
                .embed(&query)
                .context("failed to compute query embedding")?;

            let conn = pool.reader()?;

            let mut ranked = Vec::new();

            #[cfg(feature = "sqlite-vec")]
            {
                let knn_limit = limit.saturating_mul(5).clamp(100, 5_000);
                let knn_results = vec_knn_search(&conn, &query_embedding, knn_limit)?;
                let hydration_batch_size = limit.saturating_mul(4).clamp(32, 256);

                for knn_chunk in knn_results.chunks(hydration_batch_size) {
                    if ranked.len() >= limit {
                        break;
                    }

                    let ordered_ids: Vec<String> = knn_chunk
                        .iter()
                        .map(|(memory_id, _)| memory_id.clone())
                        .collect();
                    let mut hydrated_rows = hydrate_memories_by_ids(
                        &conn,
                        &ordered_ids,
                        include_superseded,
                        Some(&opts),
                        true,
                    )?;

                    for (memory_id, distance) in knn_chunk {
                        if ranked.len() >= limit {
                            break;
                        }
                        let similarity = vec_distance_to_similarity(*distance) as f32;
                        if let Some(row_data) = hydrated_rows.remove(memory_id) {
                            ranked.push(SemanticResult {
                                id: memory_id.clone(),
                                content: row_data.content,
                                tags: row_data.tags,
                                importance: row_data.importance,
                                metadata: row_data.metadata,
                                event_type: row_data.event_type,
                                session_id: row_data.session_id,
                                project: row_data.project,
                                score: similarity,
                            });
                        }
                    }
                }
            }

            #[cfg(not(feature = "sqlite-vec"))]
            {
                let mut sql = String::from(
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project
                     FROM memories
                     WHERE embedding IS NOT NULL",
                );
                if !include_superseded {
                    sql.push_str(" AND superseded_by_id IS NULL");
                }
                let mut params_values: Vec<SqlValue> = Vec::new();
                let mut idx = 1;
                append_search_filters(&mut sql, &mut params_values, &mut idx, &opts, "");
                append_context_tag_filters(
                    &mut sql,
                    &mut params_values,
                    &mut idx,
                    opts.context_tags.as_deref(),
                    "tags",
                );

                let mut stmt = conn
                    .prepare(&sql)
                    .context("failed to prepare semantic search query")?;

                let param_refs = to_param_refs(&params_values);

                let rows = stmt
                    .query_map(param_refs.as_slice(), |row| {
                        let id: String = row.get(0)?;
                        let content: String = row.get(1)?;
                        let embedding_blob: Vec<u8> = row.get(2)?;
                        let tags: String = row.get(3)?;
                        let importance: f64 = row.get(4)?;
                        let metadata: String = row.get(5)?;
                        let event_type: Option<String> = row.get(6).ok();
                        let session_id: Option<String> = row.get(7).ok();
                        let project: Option<String> = row.get(8).ok();
                        Ok((
                            id,
                            content,
                            embedding_blob,
                            tags,
                            importance,
                            metadata,
                            event_type,
                            session_id,
                            project,
                        ))
                    })
                    .context("failed to execute semantic search query")?;

                for row in rows {
                    let (
                        id,
                        content,
                        embedding_blob,
                        raw_tags,
                        importance,
                        raw_metadata,
                        event_type_str,
                        session_id,
                        project,
                    ) = row.context("failed to decode semantic search row")?;
                    let candidate: Vec<f32> = decode_embedding(&embedding_blob)
                        .context("failed to decode stored embedding")?;
                    let score = cosine_similarity(&query_embedding, &candidate);
                    ranked.push(SemanticResult {
                        id,
                        content,
                        tags: parse_tags_from_db(&raw_tags),
                        importance,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: event_type_from_sql(event_type_str),
                        session_id,
                        project,
                        score,
                    });
                }

                ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
                ranked.truncate(limit);
            }

            Ok::<_, anyhow::Error>(ranked)
        })
        .await
        .context("spawn_blocking join error")?
    }
}

#[async_trait]
impl PhraseSearcher for SqliteStorage {
    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let pool = Arc::clone(&self.pool);
        let phrase = phrase.to_string();
        let limit = i64::try_from(limit).context("phrase search limit exceeds i64")?;
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;

            let include_superseded = opts.include_superseded.unwrap_or(false);
            let conn = pool.reader()?;

            let pattern = escape_like_pattern(&phrase);

            let mut sql = String::from(
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project
                 FROM memories
                 WHERE lower(content) LIKE ?1 ESCAPE '\\'",
            );
            if !include_superseded {
                sql.push_str(" AND superseded_by_id IS NULL");
            }
            let mut params_values: Vec<SqlValue> = vec![SqlValue::Text(pattern)];
            let mut idx = 2;
            append_search_filters(&mut sql, &mut params_values, &mut idx, &opts, "");
            append_context_tag_filters(
                &mut sql,
                &mut params_values,
                &mut idx,
                opts.context_tags.as_deref(),
                "tags",
            );
            sql.push_str(" ORDER BY created_at DESC");
            sql.push_str(&format!(" LIMIT ?{idx}"));
            params_values.push(SqlValue::Integer(limit));

            let mut stmt = conn
                .prepare(&sql)
                .context("failed to prepare phrase search query")?;
            let refs = to_param_refs(&params_values);

            let rows = stmt
                .query_map(refs.as_slice(), search_result_from_row)
                .context("failed to execute phrase search query")?;

            let mut out = Vec::new();
            for row in rows {
                out.push(row.context("failed to decode phrase search row")?);
            }

            Ok::<_, anyhow::Error>(out)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
