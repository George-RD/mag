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
            let mut all_results: Vec<SearchResult> = Vec::with_capacity(limit * 2);
            let mut seen_ids: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(limit * 2);
            let mut tag_match_count: std::collections::HashMap<String, usize> =
                std::collections::HashMap::with_capacity(limit * 2);
            let mut fts_position: std::collections::HashMap<String, usize> =
                std::collections::HashMap::with_capacity(limit * 2);
            // ── Phase 1: FTS5 content search ──
            let mut fts_sql = String::from(
                "SELECT f.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project, m.entity_id, m.agent_type
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
            let candidate_limit = effective_limit * 10;
            fts_sql.push_str(&format!(" LIMIT ?{param_idx}"));
            fts_params.push(SqlValue::Integer(candidate_limit));

            if let Ok(mut stmt) = conn.prepare(&fts_sql) {
                let fts_param_refs = to_param_refs(&fts_params);
                let rows = stmt.query_map(fts_param_refs.as_slice(), search_result_from_row);
                if let Err(ref e) = rows {
                    tracing::warn!("FTS5 search failed: {e}");
                }
                if let Ok(rows) = rows {
                    for (pos, row) in rows.enumerate() {
                        let result = row.context("failed to decode FTS5 search row")?;
                        seen_ids.insert(result.id.clone());
                        fts_position.insert(result.id.clone(), pos);
                        all_results.push(result);
                    }
                }
            }

            // ── Phase 2: Tokenized tag search ──
            let raw_tokens: Vec<&str> = query.split_whitespace().collect();
            let tag_tokens: Vec<String> = raw_tokens
                .iter()
                .map(|t| t.to_lowercase())
                .filter(|t| !t.is_empty() && !is_stopword(t))
                .collect();

            if !tag_tokens.is_empty() {
                let mut tag_sql = String::from(
                    "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
                     FROM memories
                     WHERE (",
                );
                let mut tag_params: Vec<SqlValue> = Vec::new();
                for (i, token) in tag_tokens.iter().enumerate() {
                    if i > 0 {
                        tag_sql.push_str(" OR ");
                    }
                    tag_sql.push_str(&format!("lower(tags) LIKE ?{}", i + 1));
                    tag_params.push(SqlValue::Text(format!("%{}%", token)));
                }
                tag_sql.push(')');

                if !include_superseded {
                    tag_sql.push_str(" AND superseded_by_id IS NULL");
                }
                let mut tag_idx = tag_tokens.len() + 1;
                append_search_filters(
                    &mut tag_sql, &mut tag_params, &mut tag_idx, &opts, "");
                append_context_tag_filters(
                    &mut tag_sql,
                    &mut tag_params,
                    &mut tag_idx,
                    opts.context_tags.as_deref(),
                    "tags",
                );
                tag_sql.push_str(" ORDER BY last_accessed_at DESC");

                if let Ok(mut stmt) = conn.prepare(&tag_sql) {
                    let tag_param_refs = to_param_refs(&tag_params);
                    if let Ok(rows) = stmt.query_map(tag_param_refs.as_slice(), search_result_from_row) {
                        for row in rows {
                            let result = row.context("failed to decode tag search row")?;
                            let mut tag_text = result.tags.join(" ");
                            if tag_text.is_ascii() {
                                tag_text.make_ascii_lowercase();
                            } else {
                                tag_text = tag_text.to_lowercase();
                            }
                            let mut tag_words: Vec<&str> = tag_text
                                .split(|c: char| !c.is_alphanumeric() && c != '-')
                                .filter(|w| !w.is_empty())
                                .collect();
                            // Also add fully-split words (e.g. "machine-learning" → "machine", "learning")
                            let split_words: Vec<&str> = tag_text
                                .split(|c: char| !c.is_alphanumeric())
                                .filter(|w| !w.is_empty() && w.len() > 2)
                                .collect();
                            tag_words.extend(split_words);
                            // Expand ISO dates in tags to natural language month names
                            let date_expansion = expand_date_tokens(&tag_text);
                            let date_words: Vec<&str> = date_expansion.split_whitespace().collect();
                            let match_count = tag_tokens
                                .iter()
                                .filter(|t| {
                                    if tag_words.contains(&t.as_str()) || date_words.contains(&t.as_str()) {
                                        return true;
                                    }
                                    let stemmed_t = simple_stem(t);
                                    if stemmed_t != **t
                                        && (tag_words.iter().any(|word| simple_stem(word) == stemmed_t)
                                            || date_words.iter().any(|word| simple_stem(word) == stemmed_t))
                                    {
                                        return true;
                                    }
                                    // Synonym expansion: check if any synonym matches a tag word
                                    for syn in get_synonyms(t) {
                                        if tag_words.contains(syn) || date_words.contains(syn) {
                                            return true;
                                        }
                                        let stemmed_syn = simple_stem(syn);
                                        if stemmed_syn != *syn
                                            && (tag_words.iter().any(|word| simple_stem(word) == stemmed_syn)
                                                || date_words.iter().any(|word| simple_stem(word) == stemmed_syn))
                                        {
                                            return true;
                                        }
                                    }
                                    false
                                })
                                .count();
                            if match_count > 0 {
                                tag_match_count.insert(result.id.clone(), match_count);
                            }
                            if seen_ids.insert(result.id.clone()) {
                                all_results.push(result);
                            }
                        }
                    }
                }
            }
            // ── Phase 3: Re-rank by composite score ──
            // Score = tag_matches * 100 * coverage - fts_position + dual_bonus
            // where coverage = match_count / query_token_count.
            // Coverage weighting rewards memories that match a higher fraction
            // of the query tokens, creating tighter margins between fully
            // relevant and partially relevant matches.
            let query_token_count = tag_tokens.len().max(1) as i64;
            #[allow(clippy::cast_precision_loss)]
            all_results.sort_by(|a, b| {
                let a_match = tag_match_count.get(&a.id).copied().unwrap_or(0) as i64;
                let b_match = tag_match_count.get(&b.id).copied().unwrap_or(0) as i64;
                let a_pos = fts_position.get(&a.id).copied().unwrap_or(1000) as i64;
                let b_pos = fts_position.get(&b.id).copied().unwrap_or(1000) as i64;
                let a_dual = if fts_position.contains_key(&a.id) && tag_match_count.contains_key(&a.id) { 10 } else { 0 };
                let b_dual = if fts_position.contains_key(&b.id) && tag_match_count.contains_key(&b.id) { 10 } else { 0 };
                let a_coverage = a_match as f64 / query_token_count as f64;
                let b_coverage = b_match as f64 / query_token_count as f64;
                let a_imp = a.importance * 50.0;
                let b_imp = b.importance * 50.0;
                let a_score = (a_match * 100) as f64 * a_coverage - a_pos as f64 + a_dual as f64 + a_imp;
                let b_score = (b_match * 100) as f64 * b_coverage - b_pos as f64 + b_dual as f64 + b_imp;
                b_score.partial_cmp(&a_score).unwrap_or(std::cmp::Ordering::Equal)
            });
            if all_results.is_empty() {
                let pattern = escape_like_pattern(&query);
                let mut sql = String::from(
                    "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
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
                for row in rows {
                    all_results.push(row.context("failed to decode LIKE search row")?);
                }
            }

            all_results.truncate(limit);
            Ok::<_, anyhow::Error>(all_results)
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
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
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
                        #[allow(clippy::cast_possible_truncation)]
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
                                entity_id: row_data.entity_id,
                                agent_type: row_data.agent_type,
                                score: similarity,
                            });
                        }
                    }
                }
            }

            #[cfg(not(feature = "sqlite-vec"))]
            {
                let mut sql = String::from(
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
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
                        let entity_id: Option<String> = row.get(9).ok();
                        let agent_type: Option<String> = row.get(10).ok();
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
                            entity_id,
                            agent_type,
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
                        entity_id,
                        agent_type,
                    ) = row.context("failed to decode semantic search row")?;
                    let candidate: Vec<f32> = decode_embedding(&embedding_blob)
                        .context("failed to decode stored embedding")?;
                    let score = dot_product(&query_embedding, &candidate);
                    ranked.push(SemanticResult {
                        id,
                        content,
                        tags: parse_tags_from_db(&raw_tags),
                        importance,
                        metadata: parse_metadata_from_db(&raw_metadata),
                        event_type: event_type_from_sql(event_type_str),
                        session_id,
                        project,
                        entity_id,
                        agent_type,
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
                "SELECT id, content, tags, importance, metadata, event_type, session_id, project, entity_id, agent_type
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
