use super::*;

#[async_trait]
impl AdvancedSearcher for SqliteStorage {
    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = Arc::clone(&self.conn);
        let embedder = Arc::clone(&self.embedder);
        let scoring_params = self.scoring_params.clone();
        let query = query.to_string();
        let opts = opts.clone();

        tokio::task::spawn_blocking(move || {
            use rusqlite::types::Value as SqlValue;
            let include_superseded = opts.include_superseded.unwrap_or(false);
            let explain_enabled = opts.explain.unwrap_or(false);

            let query_embedding = embedder
                .embed(&query)
                .context("failed to compute query embedding")?;
            let conn = lock_conn(&conn)?;

            // ── RRF (Reciprocal Rank Fusion) hybrid search ─────────
            // Rank each signal independently then fuse with 1/(k+rank).

            // Phase 1: Collect vector candidates sorted by cosine similarity
            let mut vector_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

            #[cfg(feature = "sqlite-vec")]
            {
                let knn_limit = limit.saturating_mul(10).clamp(200, 10_000);
                let knn_results = vec_knn_search(&conn, &query_embedding, knn_limit)?;
                let row_sql = if include_superseded {
                    "SELECT content, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type
                     FROM memories WHERE id = ?1"
                } else {
                    "SELECT content, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type
                     FROM memories WHERE id = ?1 AND superseded_by_id IS NULL"
                };
                let mut row_stmt = conn
                    .prepare(row_sql)
                    .context("failed to prepare vec result lookup")?;
                for (memory_id, distance) in knn_results {
                    let similarity = vec_distance_to_similarity(distance);
                    if similarity < 0.1 {
                        continue;
                    }
                    let row_data = row_stmt
                        .query_row(params![memory_id], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, f64>(2)?,
                                row.get::<_, String>(3)?,
                                row.get::<_, Option<String>>(4).ok().flatten(),
                                row.get::<_, Option<String>>(5).ok().flatten(),
                                row.get::<_, Option<String>>(6).ok().flatten(),
                                row.get::<_, Option<i64>>(7).ok().flatten(),
                                row.get::<_, String>(8)
                                    .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                                row.get::<_, Option<String>>(9).ok().flatten(),
                                row.get::<_, Option<String>>(10).ok().flatten(),
                            ))
                        })
                        .optional()
                        .context("failed to fetch memory for vec result")?;
                    if let Some((content, raw_tags, importance, raw_metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type)) = row_data {
                        let priority_value = resolve_priority(event_type.as_deref(), priority);
                        vector_candidates.push((
                            memory_id.clone(),
                            similarity,
                            RankedSemanticCandidate {
                                result: SemanticResult {
                                    id: memory_id,
                                    content,
                                    tags: parse_tags_from_db(&raw_tags),
                                    importance,
                                    metadata: parse_metadata_from_db(&raw_metadata),
                                    event_type: event_type.clone(),
                                    session_id,
                                    project,
                                    score: 0.0,
                                },
                                created_at,
                                score: type_weight(event_type.as_deref().unwrap_or("memory"))
                                    * priority_factor(priority_value, &scoring_params),
                                vec_sim: Some(similarity),
                                text_overlap: 0.0,
                                entity_id,
                                agent_type,
                                explain: None,
                            },
                        ));
                    }
                }
            }

            #[cfg(not(feature = "sqlite-vec"))]
            {
                let vector_sql = if include_superseded {
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type
                     FROM memories WHERE embedding IS NOT NULL"
                } else {
                    "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type
                     FROM memories WHERE embedding IS NOT NULL AND superseded_by_id IS NULL"
                };
                let mut vector_stmt = conn
                    .prepare(vector_sql)
                    .context("failed to prepare advanced vector query")?;
                let vector_rows = vector_stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Vec<u8>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, f64>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, Option<String>>(6).ok().flatten(),
                            row.get::<_, Option<String>>(7).ok().flatten(),
                            row.get::<_, Option<String>>(8).ok().flatten(),
                            row.get::<_, Option<i64>>(9).ok().flatten(),
                            row.get::<_, String>(10)
                                .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                            row.get::<_, Option<String>>(11).ok().flatten(),
                            row.get::<_, Option<String>>(12).ok().flatten(),
                        ))
                    })
                    .context("failed to execute advanced vector query")?;

                for row in vector_rows {
                    let (
                        id,
                        content,
                        embedding_blob,
                        raw_tags,
                        importance,
                        raw_metadata,
                        event_type,
                        session_id,
                        project,
                        priority,
                        created_at,
                        entity_id,
                        agent_type,
                    ) = row.context("failed to decode advanced vector row")?;
                    let candidate_emb: Vec<f32> = decode_embedding(&embedding_blob)
                        .context("failed to decode stored embedding")?;
                    let similarity = cosine_similarity(&query_embedding, &candidate_emb) as f64;
                    if similarity < 0.1 {
                        continue;
                    }

                    let priority_value = resolve_priority(event_type.as_deref(), priority);
                    vector_candidates.push((
                        id.clone(),
                        similarity,
                        RankedSemanticCandidate {
                            result: SemanticResult {
                                id,
                                content,
                                tags: parse_tags_from_db(&raw_tags),
                                importance,
                                metadata: parse_metadata_from_db(&raw_metadata),
                                event_type: event_type.clone(),
                                session_id,
                                project,
                                score: 0.0,
                            },
                            created_at,
                            score: type_weight(event_type.as_deref().unwrap_or("memory"))
                                * priority_factor(priority_value, &scoring_params),
                            vec_sim: Some(similarity),
                            text_overlap: 0.0,
                            entity_id,
                            agent_type,
                            explain: None,
                        },
                    ));
                }
            }
            // Sort by cosine similarity descending for rank assignment
            vector_candidates.sort_by(|a, b| b.1.total_cmp(&a.1));

            // Phase 2: Collect FTS candidates sorted by BM25
            let mut fts_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

            let fts_query = build_fts5_query(&query);
            let mut fts_sql = String::from(
                "SELECT m.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project, m.priority, m.created_at, bm25(memories_fts), m.entity_id, m.agent_type
                 FROM memories_fts
                 JOIN memories m ON m.id = memories_fts.id
                 WHERE memories_fts MATCH ?1",
            );
            let mut fts_params: Vec<SqlValue> = vec![SqlValue::Text(fts_query)];
            let mut param_idx = 2;
            // Strip temporal filters for FTS: time_decay() handles temporal
            // scoring post-RRF, so we avoid removing candidates before fusion.
            let fts_opts = SearchOptions {
                created_after: None,
                created_before: None,
                ..opts.clone()
            };
            append_search_filters(&mut fts_sql, &mut fts_params, &mut param_idx, &fts_opts, "m.");
            if !include_superseded {
                fts_sql.push_str(" AND m.superseded_by_id IS NULL");
            }

            if let Ok(mut stmt) = conn.prepare(&fts_sql) {
                let refs = to_param_refs(&fts_params);

                let rows = stmt.query_map(refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, f64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5).ok().flatten(),
                        row.get::<_, Option<String>>(6).ok().flatten(),
                        row.get::<_, Option<String>>(7).ok().flatten(),
                        row.get::<_, Option<i64>>(8).ok().flatten(),
                        row.get::<_, String>(9)
                            .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                        row.get::<_, f64>(10).unwrap_or(1.0),
                        row.get::<_, Option<String>>(11).ok().flatten(),
                        row.get::<_, Option<String>>(12).ok().flatten(),
                    ))
                });

                if let Ok(rows) = rows {
                    for row in rows {
                        let (
                            id,
                            content,
                            raw_tags,
                            importance,
                            raw_metadata,
                            event_type,
                            session_id,
                            project,
                            priority,
                            created_at,
                            bm25,
                            entity_id,
                            agent_type,
                        ) = row.context("failed to decode advanced FTS row")?;

                        let priority_value = resolve_priority(event_type.as_deref(), priority);
                        fts_candidates.push((
                            id.clone(),
                            bm25, // raw BM25: more negative = better match
                            RankedSemanticCandidate {
                                result: SemanticResult {
                                    id,
                                    content,
                                    tags: parse_tags_from_db(&raw_tags),
                                    importance,
                                    metadata: parse_metadata_from_db(&raw_metadata),
                                    event_type: event_type.clone(),
                                    session_id,
                                    project,
                                    score: 0.0,
                                },
                                created_at,
                                score: type_weight(event_type.as_deref().unwrap_or("memory"))
                                    * priority_factor(priority_value, &scoring_params),
                                vec_sim: None,
                                text_overlap: 0.0,
                                entity_id,
                                agent_type,
                                explain: None,
                            },
                        ));
                    }
                }
            }
            // BM25 returns negative values where more negative = better match,
            // so sort ascending (most negative first = best rank for RRF)
            fts_candidates.sort_by(|a, b| a.1.total_cmp(&b.1));

            // Phase 3: Weighted RRF fusion -- vector similarity weighted higher
            // for semantic discrimination (Oracle recommendation)
            let mut ranked: HashMap<String, RankedSemanticCandidate> = HashMap::new();

            // Track which IDs appear in FTS results for dual_match detection
            let fts_ids: HashSet<String> = if explain_enabled {
                fts_candidates.iter().map(|(id, _, _)| id.clone()).collect()
            } else {
                HashSet::new()
            };

            for (rank, (id, _sim, mut candidate)) in vector_candidates.into_iter().enumerate() {
                let rrf_score =
                    scoring_params.rrf_weight_vec / (scoring_params.rrf_k + rank as f64 + 1.0);
                let type_w = type_weight(candidate.result.event_type.as_deref().unwrap_or("memory"));
                let priority_value = resolve_priority(
                    candidate.result.event_type.as_deref(),
                    None, // priority already baked into candidate.score
                );
                let pf = priority_factor(priority_value, &scoring_params);

                if explain_enabled {
                    let dual = fts_ids.contains(&id);
                    candidate.explain = Some(serde_json::json!({
                        "vec_sim": candidate.vec_sim,
                        "fts_rank": null,
                        "rrf_score": rrf_score,
                        "dual_match": dual,
                        "type_weight": type_w,
                        "priority_factor": pf,
                    }));
                }

                candidate.score *= rrf_score;
                ranked.insert(id, candidate);
            }
            for (rank, (id, bm25_raw, candidate)) in fts_candidates.into_iter().enumerate() {
                let rrf_score =
                    scoring_params.rrf_weight_fts / (scoring_params.rrf_k + rank as f64 + 1.0);
                if let Some(existing) = ranked.get_mut(&id) {
                    // Present in both -- add the FTS RRF contribution
                    existing.score += candidate.score * rrf_score;
                    if explain_enabled
                        && let Some(ref mut exp) = existing.explain
                    {
                        exp["fts_rank"] = serde_json::json!(rank);
                        exp["fts_bm25"] = serde_json::json!(bm25_raw);
                        exp["dual_match"] = serde_json::json!(true);
                        // Update rrf_score to show the combined contribution
                        let vec_rrf = exp["rrf_score"].as_f64().unwrap_or(0.0);
                        exp["rrf_score"] = serde_json::json!(vec_rrf + rrf_score);
                    }
                } else {
                    let type_w = type_weight(candidate.result.event_type.as_deref().unwrap_or("memory"));
                    let priority_value = resolve_priority(
                        candidate.result.event_type.as_deref(),
                        None,
                    );
                    let pf = priority_factor(priority_value, &scoring_params);

                    let explain_data = if explain_enabled {
                        Some(serde_json::json!({
                            "vec_sim": null,
                            "fts_rank": rank,
                            "fts_bm25": bm25_raw,
                            "rrf_score": rrf_score,
                            "dual_match": false,
                            "type_weight": type_w,
                            "priority_factor": pf,
                        }))
                    } else {
                        None
                    };

                    let mut merged = candidate;
                    merged.score *= rrf_score;
                    merged.explain = explain_data;
                    ranked.insert(id, merged);
                }
            }

            let query_tokens = token_set(&query, 3);
            for candidate in ranked.values_mut() {
                let with_tags = if candidate.result.tags.is_empty() {
                    candidate.result.content.clone()
                } else {
                    format!("{} {}", candidate.result.content, candidate.result.tags.join(" "))
                };
                // Pre-tokenize once for this candidate
                let candidate_tokens = token_set(&with_tags, 3);
                let overlap = word_overlap_pre(&query_tokens, &candidate_tokens);
                candidate.text_overlap = overlap;
                let fb_score = candidate
                    .result
                    .metadata
                    .get("feedback_score")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let fb_dampening = if fb_score < 0 { 0.5 } else { 1.0 };
                candidate.score *=
                    1.0 + overlap * scoring_params.word_overlap_weight * fb_dampening;
                let jaccard = jaccard_pre(&query_tokens, &candidate_tokens);
                candidate.score *= 1.0 + jaccard * scoring_params.jaccard_weight;
                let fb_factor = feedback_factor(fb_score, &scoring_params);
                candidate.score *= fb_factor;
                let event_type = candidate.result.event_type.as_deref().unwrap_or("");
                let td = time_decay(&candidate.created_at, event_type, &scoring_params);
                candidate.score *= td;
                let importance_factor_val = scoring_params.importance_floor
                    + candidate.result.importance * scoring_params.importance_scale;
                candidate.score *= importance_factor_val;

                if let Some(context_tags) = opts.context_tags.as_ref() {
                    let candidate_tags: HashSet<String> = candidate
                        .result
                        .tags
                        .iter()
                        .map(|t| t.to_lowercase())
                        .collect();
                    let context_norm: Vec<String> = context_tags
                        .iter()
                        .map(|t| t.to_lowercase())
                        .filter(|t| !t.is_empty())
                        .collect();
                    if !context_norm.is_empty() {
                        let matched = context_norm
                            .iter()
                            .filter(|t| candidate_tags.contains(*t))
                            .count();
                        let ratio = matched as f64 / context_norm.len() as f64;
                        candidate.score *= 1.0 + ratio * scoring_params.context_tag_weight;
                    }
                }

                // Record refinement factors for explain mode
                if explain_enabled
                    && let Some(ref mut exp) = candidate.explain
                {
                    exp["word_overlap"] = serde_json::json!(overlap);
                    exp["text_overlap"] = serde_json::json!(overlap);
                    exp["importance_factor"] = serde_json::json!(importance_factor_val);
                    exp["feedback_factor"] = serde_json::json!(fb_factor);
                    exp["time_decay"] = serde_json::json!(td);
                }
            }


            // ── Phase 5: Graph enrichment -- inject 1-hop neighbors from top seeds ──
            {
                let mut seed_list: Vec<(String, f64)> = ranked
                    .iter()
                    .map(|(id, c)| (id.clone(), c.score))
                    .collect();
                seed_list.sort_by(|a, b| b.1.total_cmp(&a.1));
                let k = limit.clamp(scoring_params.graph_seed_min, scoring_params.graph_seed_max);
                seed_list.truncate(k);

                let neighbor_sql = if include_superseded {
                    "\
                    SELECT m.id, m.content, m.tags, m.importance, m.metadata, \
                           m.event_type, m.session_id, m.project, m.priority, m.created_at, \
                           m.embedding, r.weight, m.entity_id, m.agent_type \
                    FROM relationships r \
                    JOIN memories m ON m.id = CASE \
                        WHEN r.source_id = ?1 THEN r.target_id \
                        ELSE r.source_id END \
                    WHERE (r.source_id = ?1 OR r.target_id = ?1) \
                      AND r.weight >= ?2 \
                      AND m.id != ?1"
                } else {
                    "\
                    SELECT m.id, m.content, m.tags, m.importance, m.metadata, \
                           m.event_type, m.session_id, m.project, m.priority, m.created_at, \
                           m.embedding, r.weight, m.entity_id, m.agent_type \
                    FROM relationships r \
                    JOIN memories m ON m.id = CASE \
                        WHEN r.source_id = ?1 THEN r.target_id \
                        ELSE r.source_id END \
                    WHERE (r.source_id = ?1 OR r.target_id = ?1) \
                      AND r.weight >= ?2 \
                      AND m.id != ?1 \
                      AND m.superseded_by_id IS NULL"
                };

                let mut neighbors_to_add: Vec<(String, RankedSemanticCandidate)> = Vec::new();

                if let Ok(mut stmt) = conn.prepare(neighbor_sql) {
                    for (seed_id, seed_score) in &seed_list {
                        if let Ok(rows) = stmt.query_map(
                            params![seed_id, scoring_params.graph_min_edge_weight],
                            |row| {
                                Ok((
                                    row.get::<_, String>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, f64>(3)?,
                                    row.get::<_, String>(4)?,
                                    row.get::<_, Option<String>>(5).ok().flatten(),
                                    row.get::<_, Option<String>>(6).ok().flatten(),
                                    row.get::<_, Option<String>>(7).ok().flatten(),
                                    row.get::<_, Option<i64>>(8).ok().flatten(),
                                    row.get::<_, String>(9)
                                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                                    row.get::<_, Option<Vec<u8>>>(10).ok().flatten(),
                                    row.get::<_, f64>(11).unwrap_or(0.5),
                                    row.get::<_, Option<String>>(12).ok().flatten(),
                                    row.get::<_, Option<String>>(13).ok().flatten(),
                                ))
                            },
                        ) {
                            for row_res in rows {
                                let row = match row_res {
                                    Ok(r) => r,
                                    Err(e) => {
                                        tracing::warn!("failed to decode graph neighbor row: {e}");
                                        continue;
                                    }
                                };
                                let (
                                    id, content, raw_tags, importance, raw_metadata,
                                    event_type, session_id, project, _priority, created_at,
                                    embedding_blob, edge_weight, entity_id, agent_type,
                                ) = row;

                                let mut neighbor_score =
                                    scoring_params.graph_neighbor_factor * seed_score * edge_weight;

                                let tags = parse_tags_from_db(&raw_tags);
                                let metadata = parse_metadata_from_db(&raw_metadata);
                                let with_tags = if tags.is_empty() {
                                    content.clone()
                                } else {
                                    format!("{} {}", content, tags.join(" "))
                                };
                                let overlap = word_overlap_pre(&query_tokens, &token_set(&with_tags, 3));
                                let fb_score = metadata
                                    .get("feedback_score")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or(0);
                                let fb_dampening = if fb_score < 0 { 0.5 } else { 1.0 };
                                neighbor_score *= 1.0
                                    + overlap
                                        * scoring_params.neighbor_word_overlap_weight
                                        * fb_dampening;
                                let event_type_for_decay = event_type.as_deref().unwrap_or("");
                                neighbor_score *=
                                    time_decay(&created_at, event_type_for_decay, &scoring_params);
                                neighbor_score *= scoring_params.neighbor_importance_floor
                                    + importance * scoring_params.neighbor_importance_scale;

                                let vec_sim = embedding_blob.and_then(|blob| {
                                    decode_embedding(&blob)
                                        .ok()
                                        .map(|emb| cosine_similarity(&query_embedding, &emb) as f64)
                                });
                                neighbor_score *= feedback_factor(fb_score, &scoring_params);

                                let explain_data = if explain_enabled {
                                    Some(serde_json::json!({
                                        "vec_sim": vec_sim,
                                        "fts_rank": null,
                                        "rrf_score": null,
                                        "dual_match": false,
                                        "type_weight": type_weight(event_type.as_deref().unwrap_or("memory")),
                                        "word_overlap": overlap,
                                        "text_overlap": overlap,
                                        "graph_injected": true,
                                        "graph_seed_id": seed_id,
                                        "graph_edge_weight": edge_weight,
                                    }))
                                } else {
                                    None
                                };

                                neighbors_to_add.push((
                                    id.clone(),
                                    RankedSemanticCandidate {
                                        result: SemanticResult {
                                            id,
                                            content,
                                            tags,
                                            importance,
                                            metadata,
                                            event_type,
                                            session_id,
                                            project,
                                            score: 0.0,
                                        },
                                        created_at,
                                        score: neighbor_score,
                                        vec_sim,
                                        text_overlap: overlap,
                                        entity_id,
                                        agent_type,
                                        explain: explain_data,
                                    },
                                ));
                            }
                        }
                    }
                }

                for (id, neighbor) in neighbors_to_add {
                    if let Some(existing) = ranked.get_mut(&id) {
                        if neighbor.score > existing.score {
                            existing.score = neighbor.score;
                        }
                    } else {
                        ranked.insert(id, neighbor);
                    }
                }
            }
            // ── Phase 6: Collection-level abstention + dedup ─────────────
            // Dense embeddings (bge-small-en-v1.5) produce high cosine similarity
            // (0.80+) even for completely unrelated content, making vec_sim
            // useless for abstention. Text overlap is the discriminative signal:
            //   - Legitimate queries: max text_overlap typically >= 0.33
            //   - Irrelevant queries: max text_overlap typically 0.00-0.25
            // Apply a collection-level gate on the best text overlap.
            // Skip the abstention gate when the query has no eligible word
            // tokens (all tokens <= 2 chars, e.g. "AI", "C++") -- text overlap
            // would always be 0.0, causing false abstention.
            // NOTE: Gate is applied AFTER search-option filtering (below) so
            // that out-of-scope high-overlap candidates don't suppress
            // abstention for scoped queries.
            let mut deduped = Vec::new();
            let mut seen = HashSet::new();
            for candidate in ranked.into_values() {
                if !matches_search_options(&candidate, &opts) {
                    continue;
                }
                let fingerprint = normalize_for_dedup(&candidate.result.content);
                if seen.insert(fingerprint) {
                    deduped.push(candidate);
                }
            }

            // Apply abstention gate on the filtered (in-scope) candidates.
            if !query_tokens.is_empty() {
                let max_text_overlap = deduped
                    .iter()
                    .map(|c| c.text_overlap)
                    .fold(0.0f64, f64::max);
                if max_text_overlap < scoring_params.abstention_min_text {
                    return Ok::<_, anyhow::Error>(Vec::new());
                }
            }
            if deduped.is_empty() {
                return Ok::<_, anyhow::Error>(Vec::new());
            }

            deduped.sort_by(|a, b| b.score.total_cmp(&a.score));
            let max_score = deduped.first().map(|c| c.score).unwrap_or(0.0);
            let mut out = Vec::new();
            for mut candidate in deduped.into_iter().take(limit) {
                let normalized = if max_score > 0.0 {
                    (candidate.score / max_score).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                candidate.result.score = normalized as f32;

                // Inject explain data into result metadata when enabled
                if explain_enabled
                    && let Some(mut exp) = candidate.explain.take()
                {
                    exp["final_score"] = serde_json::json!(normalized);
                    if let serde_json::Value::Object(ref mut meta) = candidate.result.metadata {
                        meta.insert("_explain".to_string(), exp);
                    }
                }

                out.push(candidate.result);
            }
            Ok::<_, anyhow::Error>(out)
        })
        .await
        .context("spawn_blocking join error")?
    }
}
