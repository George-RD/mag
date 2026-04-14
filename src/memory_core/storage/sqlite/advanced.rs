use super::helpers::{extract_entities_from_tags, resolve_priority};
use super::nlp::{
    content_fingerprint, extract_query_entities, extract_topic_keywords, generate_sub_queries,
};
use super::query_classifier::{
    IntentProfile, QueryIntent, classify_query_intent, detect_dynamic_limit_mult,
};
use super::*;
use crate::memory_core::domain::{
    REL_PARALLEL_CONTEXT, REL_PRECEDED_BY, REL_RELATES_TO, REL_SHARES_THEME, REL_SIMILAR_TO,
};
use crate::memory_core::scoring::query_coverage_boost;

const ADVANCED_FTS_CANDIDATE_MULTIPLIER: usize = 20;
const ADVANCED_FTS_CANDIDATE_MIN: usize = 100;
const ADVANCED_FTS_CANDIDATE_MAX: usize = 5_000;

fn advanced_fts_candidate_limit(limit: usize) -> usize {
    let oversampled_limit = limit
        .saturating_mul(ADVANCED_FTS_CANDIDATE_MULTIPLIER)
        .clamp(ADVANCED_FTS_CANDIDATE_MIN, ADVANCED_FTS_CANDIDATE_MAX);
    oversampled_limit.max(limit)
}

/// Phase 1: Collect vector candidates sorted by cosine similarity.
fn collect_vector_candidates(
    conn: &Connection,
    query_embedding: &[f32],
    #[cfg_attr(not(feature = "sqlite-vec"), allow(unused))] limit: usize,
    include_superseded: bool,
    #[cfg_attr(not(feature = "sqlite-vec"), allow(unused))] opts: &SearchOptions,
    scoring_params: &ScoringParams,
) -> Result<Vec<(String, f64, RankedSemanticCandidate)>> {
    let mut vector_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

    #[cfg(feature = "sqlite-vec")]
    {
        let knn_limit = limit.saturating_mul(10).clamp(200, 10_000);
        let knn_results = vec_knn_search(conn, query_embedding, knn_limit)?;
        let ordered_ids: Vec<String> = knn_results
            .iter()
            .filter_map(|(memory_id, distance)| {
                let similarity = vec_distance_to_similarity(*distance);
                (similarity >= 0.1).then_some(memory_id.clone())
            })
            .collect();
        let mut hydrated_rows =
            hydrate_memories_by_ids(conn, &ordered_ids, include_superseded, Some(opts), true)?;
        for (memory_id, distance) in knn_results {
            let similarity = vec_distance_to_similarity(distance);
            if similarity < 0.1 {
                continue;
            }
            if let Some(row_data) = hydrated_rows.remove(&memory_id) {
                let et = row_data.event_type.clone();
                let et_ref = et.as_ref().unwrap_or(&EventType::Memory);
                let priority_value = resolve_priority(et.as_ref(), row_data.priority);
                let initial_score =
                    type_weight_et(et_ref) * priority_factor(priority_value, scoring_params);
                vector_candidates.push((
                    memory_id.clone(),
                    similarity,
                    RankedSemanticCandidate {
                        result: SemanticResult {
                            id: memory_id,
                            content: row_data.content,
                            tags: row_data.tags,
                            importance: row_data.importance,
                            metadata: row_data.metadata,
                            event_type: et,
                            session_id: row_data.session_id,
                            project: row_data.project,
                            entity_id: row_data.entity_id.clone(),
                            agent_type: row_data.agent_type.clone(),
                            score: 0.0,
                        },
                        created_at: row_data.created_at,
                        event_at: row_data.event_at,
                        score: initial_score,
                        priority_value,
                        vec_sim: Some(similarity),
                        text_overlap: 0.0,
                        entity_id: row_data.entity_id,
                        agent_type: row_data.agent_type,
                        explain: None,
                    },
                ));
            }
        }
    }

    #[cfg(not(feature = "sqlite-vec"))]
    {
        let vector_sql = if include_superseded {
            "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at
             FROM memories WHERE embedding IS NOT NULL"
        } else {
            "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at
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
                    row.get::<_, String>(13)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
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
                event_type_str,
                session_id,
                project,
                priority,
                created_at,
                entity_id,
                agent_type,
                event_at,
            ) = row.context("failed to decode advanced vector row")?;
            let candidate_emb: Vec<f32> =
                decode_embedding(&embedding_blob).context("failed to decode stored embedding")?;
            let similarity = dot_product(query_embedding, &candidate_emb) as f64;
            if similarity < 0.1 {
                continue;
            }

            let et = event_type_from_sql(event_type_str.clone());
            let et_ref = et.as_ref().unwrap_or(&EventType::Memory);
            let priority_value = resolve_priority(et.as_ref(), priority);
            let initial_score =
                type_weight_et(et_ref) * priority_factor(priority_value, scoring_params);
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
                        event_type: et,
                        session_id,
                        project,
                        entity_id: entity_id.clone(),
                        agent_type: agent_type.clone(),
                        score: 0.0,
                    },
                    created_at,
                    event_at,
                    score: initial_score,
                    priority_value,
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
    Ok(vector_candidates)
}

/// Phase 2: Collect FTS candidates sorted by BM25.
fn collect_fts_candidates(
    conn: &Connection,
    query: &str,
    limit: usize,
    opts: &SearchOptions,
    include_superseded: bool,
    scoring_params: &ScoringParams,
) -> Result<Vec<(String, f64, RankedSemanticCandidate)>> {
    use rusqlite::types::Value as SqlValue;

    let mut fts_candidates: Vec<(String, f64, RankedSemanticCandidate)> = Vec::new();

    let fts_query = build_fts5_query(query);
    let mut fts_sql = String::from(
        "SELECT m.id, m.content, m.tags, m.importance, m.metadata, m.event_type, m.session_id, m.project, m.priority, m.created_at, bm25(memories_fts), m.entity_id, m.agent_type, m.event_at
         FROM memories_fts
         JOIN memories m ON m.id = memories_fts.id
         WHERE memories_fts MATCH ?1",
    );
    let mut fts_params: Vec<SqlValue> = vec![SqlValue::Text(fts_query)];
    let mut param_idx = 2;
    append_search_filters(&mut fts_sql, &mut fts_params, &mut param_idx, opts, "m.");
    if !include_superseded {
        fts_sql.push_str(" AND m.superseded_by_id IS NULL");
    }
    fts_sql.push_str(" ORDER BY bm25(memories_fts) ASC LIMIT ?");
    fts_sql.push_str(&param_idx.to_string());
    let sql_limit = i64::try_from(advanced_fts_candidate_limit(limit)).unwrap_or(i64::MAX);
    fts_params.push(SqlValue::Integer(sql_limit));

    let fts_stmt = conn.prepare(&fts_sql);
    if let Err(e) = &fts_stmt {
        tracing::warn!("failed to prepare FTS query: {e}");
    }
    if let Ok(mut stmt) = fts_stmt {
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
                row.get::<_, String>(13)
                    .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
            ))
        });

        if let Err(e) = &rows {
            tracing::warn!("FTS query_map failed: {e}");
        }
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
                    event_at,
                ) = row.context("failed to decode advanced FTS row")?;

                let et = event_type_from_sql(event_type.clone());
                let et_ref = et.as_ref().unwrap_or(&EventType::Memory);
                let priority_value = resolve_priority(et.as_ref(), priority);
                let initial_score =
                    type_weight_et(et_ref) * priority_factor(priority_value, scoring_params);
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
                            event_type: et,
                            session_id,
                            project,
                            entity_id: entity_id.clone(),
                            agent_type: agent_type.clone(),
                            score: 0.0,
                        },
                        created_at,
                        event_at,
                        score: initial_score,
                        priority_value,
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
    Ok(fts_candidates)
}

/// Compute cross-encoder scores for the top candidates if a reranker is available.
#[cfg(feature = "real-embeddings")]
fn compute_cross_encoder_scores(
    reranker: Option<&std::sync::Arc<crate::memory_core::reranker::CrossEncoderReranker>>,
    query: &str,
    vector_candidates: &[(String, f64, RankedSemanticCandidate)],
    fts_candidates: &[(String, f64, RankedSemanticCandidate)],
    scoring_params: &ScoringParams,
) -> Option<HashMap<String, f32>> {
    let reranker = reranker?;
    let mut seen: HashSet<&str> = HashSet::with_capacity(scoring_params.rerank_top_n);
    let mut candidates_for_rerank: Vec<(&str, &str)> =
        Vec::with_capacity(scoring_params.rerank_top_n);
    // Interleave vector and FTS candidates to avoid biasing towards either source
    // when truncating to rerank_top_n.
    let max_idx = vector_candidates.len().max(fts_candidates.len());
    for i in 0..max_idx {
        if candidates_for_rerank.len() >= scoring_params.rerank_top_n {
            break;
        }
        if let Some((id, _, candidate)) = vector_candidates.get(i)
            && seen.insert(id.as_str())
        {
            candidates_for_rerank.push((id.as_str(), &candidate.result.content));
        }
        if candidates_for_rerank.len() >= scoring_params.rerank_top_n {
            break;
        }
        if let Some((id, _, candidate)) = fts_candidates.get(i)
            && seen.insert(id.as_str())
        {
            candidates_for_rerank.push((id.as_str(), &candidate.result.content));
        }
    }
    if candidates_for_rerank.is_empty() {
        return None;
    }
    let passages: Vec<&str> = candidates_for_rerank.iter().map(|(_, c)| *c).collect();
    match reranker.score_batch(query, &passages) {
        Ok(scores) => {
            let mut map = HashMap::with_capacity(scores.len());
            for (i, &(id, _)) in candidates_for_rerank.iter().enumerate() {
                map.insert(id.to_owned(), scores[i]);
            }
            Some(map)
        }
        Err(e) => {
            tracing::warn!("cross-encoder reranking failed, skipping: {e}");
            None
        }
    }
}

/// Phase 4: Score refinement — word overlap, coverage boost, Jaccard, feedback,
/// time decay, importance, and context tag matching.
fn refine_scores(
    ranked: &mut HashMap<String, RankedSemanticCandidate>,
    query_tokens: &HashSet<String>,
    opts: &SearchOptions,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
) {
    for candidate in ranked.values_mut() {
        let with_tags = if candidate.result.tags.is_empty() {
            candidate.result.content.clone()
        } else {
            format!(
                "{} {}",
                candidate.result.content,
                candidate.result.tags.join(" ")
            )
        };
        let candidate_tokens = token_set(&with_tags, 3);
        let overlap = word_overlap_pre(query_tokens, &candidate_tokens);
        candidate.text_overlap = overlap;
        let fb_score = candidate
            .result
            .metadata
            .get("feedback_score")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let fb_dampening = if fb_score < 0 { 0.5 } else { 1.0 };
        let coverage_boost =
            1.0 + (query_coverage_boost(overlap, scoring_params) - 1.0) * fb_dampening;
        candidate.score *= coverage_boost;
        candidate.score *= 1.0 + overlap * scoring_params.word_overlap_weight * fb_dampening;
        let jaccard = jaccard_pre(query_tokens, &candidate_tokens);
        candidate.score *= 1.0 + jaccard * scoring_params.jaccard_weight;
        let fb_factor = feedback_factor(fb_score, scoring_params);
        candidate.score *= fb_factor;
        let et_ref = candidate
            .result
            .event_type
            .as_ref()
            .unwrap_or(&EventType::Memory);
        let td = time_decay_et(&candidate.created_at, et_ref, scoring_params);
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
                #[allow(clippy::cast_precision_loss)]
                let ratio = matched as f64 / context_norm.len() as f64;
                candidate.score *= 1.0 + ratio * scoring_params.context_tag_weight;
            }
        }

        if explain_enabled && let Some(ref mut exp) = candidate.explain {
            exp["word_overlap"] = serde_json::json!(overlap);
            exp["query_coverage_boost"] = serde_json::json!(coverage_boost);
            exp["text_overlap"] = serde_json::json!(overlap);
            exp["importance_factor"] = serde_json::json!(importance_factor_val);
            exp["feedback_factor"] = serde_json::json!(fb_factor);
            exp["time_decay"] = serde_json::json!(td);
        }
    }
}

/// Phase 5: Graph enrichment — inject 1-hop neighbors from top-scoring seeds.
#[allow(clippy::too_many_arguments)]
fn enrich_graph_neighbors(
    conn: &Connection,
    ranked: &mut HashMap<String, RankedSemanticCandidate>,
    query_tokens: &HashSet<String>,
    query_embedding: &[f32],
    limit: usize,
    include_superseded: bool,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
) {
    if scoring_params.graph_neighbor_factor <= 0.0 {
        return;
    }

    let mut seed_list: Vec<(String, f64)> =
        ranked.iter().map(|(id, c)| (id.clone(), c.score)).collect();
    seed_list.sort_by(|a, b| b.1.total_cmp(&a.1));
    let k = limit.clamp(scoring_params.graph_seed_min, scoring_params.graph_seed_max);
    seed_list.truncate(k);

    let neighbor_sql = if include_superseded {
        "\
        SELECT m.id, m.content, m.tags, m.importance, m.metadata, \
               m.event_type, m.session_id, m.project, m.priority, m.created_at, \
               m.embedding, r.weight, m.entity_id, m.agent_type, m.event_at, r.rel_type \
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
               m.embedding, r.weight, m.entity_id, m.agent_type, m.event_at, r.rel_type \
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
                        row.get::<_, String>(14)
                            .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                        row.get::<_, String>(15).unwrap_or_else(|_| String::new()),
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
                        embedding_blob,
                        edge_weight,
                        entity_id,
                        agent_type,
                        event_at,
                        rel_type,
                    ) = row;

                    let mut neighbor_score =
                        scoring_params.graph_neighbor_factor * seed_score * edge_weight;

                    match rel_type.as_str() {
                        REL_PRECEDED_BY => {
                            neighbor_score *= scoring_params.preceded_by_boost;
                        }
                        REL_RELATES_TO | REL_SIMILAR_TO | REL_SHARES_THEME
                        | REL_PARALLEL_CONTEXT => {
                            neighbor_score *= scoring_params.entity_relation_boost;
                        }
                        _ => {}
                    }

                    let tags = parse_tags_from_db(&raw_tags);
                    let metadata = parse_metadata_from_db(&raw_metadata);
                    let with_tags = if tags.is_empty() {
                        content.clone()
                    } else {
                        format!("{} {}", content, tags.join(" "))
                    };
                    let overlap = word_overlap_pre(query_tokens, &token_set(&with_tags, 3));
                    let fb_score = metadata
                        .get("feedback_score")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let fb_dampening = if fb_score < 0 { 0.5 } else { 1.0 };
                    neighbor_score *=
                        1.0 + overlap * scoring_params.neighbor_word_overlap_weight * fb_dampening;
                    let neighbor_et = event_type_from_sql(event_type);
                    let neighbor_et_ref = neighbor_et.as_ref().unwrap_or(&EventType::Memory);
                    let neighbor_pv = resolve_priority(neighbor_et.as_ref(), priority);
                    neighbor_score *= time_decay_et(&created_at, neighbor_et_ref, scoring_params);
                    neighbor_score *= scoring_params.neighbor_importance_floor
                        + importance * scoring_params.neighbor_importance_scale;

                    let vec_sim = embedding_blob.and_then(|blob| {
                        decode_embedding(&blob)
                            .ok()
                            .map(|emb| dot_product(query_embedding, &emb) as f64)
                    });
                    neighbor_score *= feedback_factor(fb_score, scoring_params);

                    let explain_data = if explain_enabled {
                        Some(serde_json::json!({
                            "vec_sim": vec_sim,
                            "fts_rank": null,
                            "rrf_score": null,
                            "dual_match": false,
                            "type_weight": type_weight_et(neighbor_et_ref),
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
                                event_type: neighbor_et,
                                session_id,
                                project,
                                entity_id: entity_id.clone(),
                                agent_type: agent_type.clone(),
                                score: 0.0,
                            },
                            created_at,
                            event_at,
                            score: neighbor_score,
                            priority_value: neighbor_pv,
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
                if let (Some(existing_explain), Some(neighbor_explain)) =
                    (&mut existing.explain, &neighbor.explain)
                {
                    if let (Some(dst), Some(src)) = (
                        existing_explain.as_object_mut(),
                        neighbor_explain.as_object(),
                    ) {
                        for key in ["graph_injected", "graph_seed_id", "graph_edge_weight"] {
                            if let Some(v) = src.get(key) {
                                dst.insert(key.to_string(), v.clone());
                            }
                        }
                    }
                } else if existing.explain.is_none() {
                    existing.explain = neighbor.explain;
                }
            }
        } else {
            ranked.insert(id, neighbor);
        }
    }
}

/// Phase 5b: Entity expansion — find memories tagged with entities from seed results.
#[allow(clippy::too_many_arguments)]
fn expand_entity_tags(
    conn: &Connection,
    ranked: &mut HashMap<String, RankedSemanticCandidate>,
    query_tokens: &HashSet<String>,
    limit: usize,
    include_superseded: bool,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
    opts: &SearchOptions,
) {
    use crate::memory_core::scoring::ENTITY_EXPANSION_BOOST;

    let mut seed_list_for_expansion: Vec<(String, f64, Vec<String>)> = ranked
        .iter()
        .map(|(id, c)| (id.clone(), c.score, c.result.tags.clone()))
        .collect();
    seed_list_for_expansion.sort_by(|a, b| b.1.total_cmp(&a.1));
    seed_list_for_expansion.truncate(limit.min(20));

    let mut entity_tags_to_search: Vec<String> = Vec::new();
    let mut seen_entity_tags = HashSet::new();
    for (_id, _score, tags) in &seed_list_for_expansion {
        for tag in extract_entities_from_tags(tags) {
            if seen_entity_tags.insert(tag.clone()) {
                entity_tags_to_search.push(tag);
            }
        }
    }
    entity_tags_to_search.truncate(5);

    if entity_tags_to_search.is_empty() {
        return;
    }

    let expansion_limit = 25usize;
    let mut expanded_count = 0usize;
    let existing_ids: HashSet<String> = ranked.keys().cloned().collect();

    let tag_sql = if include_superseded {
        "SELECT id, content, tags, importance, metadata, event_type, session_id, \
         project, priority, created_at, entity_id, agent_type, event_at \
         FROM memories \
         WHERE json_valid(tags) AND EXISTS ( \
             SELECT 1 FROM json_each(tags) WHERE value = ?1 \
         ) \
         ORDER BY created_at DESC LIMIT ?2"
    } else {
        "SELECT id, content, tags, importance, metadata, event_type, session_id, \
         project, priority, created_at, entity_id, agent_type, event_at \
         FROM memories \
         WHERE json_valid(tags) AND EXISTS ( \
             SELECT 1 FROM json_each(tags) WHERE value = ?1 \
         ) \
         AND superseded_by_id IS NULL \
         ORDER BY created_at DESC LIMIT ?2"
    };

    let per_entity_limit = 5i64;
    if let Ok(mut stmt) = conn.prepare(tag_sql) {
        for entity_tag in &entity_tags_to_search {
            if expanded_count >= expansion_limit {
                break;
            }

            if let Ok(rows) = stmt.query_map(params![entity_tag, per_entity_limit], |row| {
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
                    row.get::<_, Option<String>>(10).ok().flatten(),
                    row.get::<_, Option<String>>(11).ok().flatten(),
                    row.get::<_, String>(12)
                        .unwrap_or_else(|_| EPOCH_FALLBACK.to_string()),
                ))
            }) {
                for row_res in rows {
                    if expanded_count >= expansion_limit {
                        break;
                    }
                    let row = match row_res {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("failed to decode entity expansion row: {e}");
                            continue;
                        }
                    };
                    let (
                        id,
                        content,
                        raw_tags,
                        importance,
                        raw_metadata,
                        event_type_str,
                        session_id,
                        project,
                        priority,
                        created_at,
                        entity_id,
                        agent_type,
                        event_at,
                    ) = row;

                    if existing_ids.contains(&id) {
                        continue;
                    }

                    let tags = parse_tags_from_db(&raw_tags);
                    let metadata = parse_metadata_from_db(&raw_metadata);
                    let et = event_type_from_sql(event_type_str);
                    let et_ref = et.as_ref().unwrap_or(&EventType::Memory);
                    let priority_value = resolve_priority(et.as_ref(), priority);

                    let base_score =
                        type_weight_et(et_ref) * priority_factor(priority_value, scoring_params);
                    let importance_factor_val = scoring_params.importance_floor
                        + importance * scoring_params.importance_scale;
                    let with_tags_text = if tags.is_empty() {
                        content.clone()
                    } else {
                        format!("{} {}", content, tags.join(" "))
                    };
                    let overlap = word_overlap_pre(query_tokens, &token_set(&with_tags_text, 3));
                    let td = time_decay_et(&created_at, et_ref, scoring_params);

                    let expanded_score = base_score
                        * importance_factor_val
                        * (1.0 + overlap * scoring_params.word_overlap_weight)
                        * td
                        * ENTITY_EXPANSION_BOOST;

                    let max_seed_score = seed_list_for_expansion
                        .first()
                        .map(|(_, s, _)| *s)
                        .unwrap_or(1.0);
                    let final_score = expanded_score.min(max_seed_score * 0.8);

                    let explain_data = if explain_enabled {
                        Some(serde_json::json!({
                            "entity_expansion": true,
                            "expanded_from_tag": entity_tag,
                            "entity_boost": ENTITY_EXPANSION_BOOST,
                            "word_overlap": overlap,
                            "importance_factor": importance_factor_val,
                            "time_decay": td,
                        }))
                    } else {
                        None
                    };

                    let candidate = RankedSemanticCandidate {
                        result: SemanticResult {
                            id: id.clone(),
                            content,
                            tags,
                            importance,
                            metadata,
                            event_type: et,
                            session_id,
                            project,
                            entity_id: entity_id.clone(),
                            agent_type: agent_type.clone(),
                            score: 0.0,
                        },
                        created_at,
                        event_at,
                        score: final_score,
                        priority_value,
                        vec_sim: None,
                        text_overlap: overlap,
                        entity_id,
                        agent_type,
                        explain: explain_data,
                    };
                    if !matches_search_options(&candidate, opts) {
                        continue;
                    }
                    ranked.insert(id, candidate);
                    expanded_count += 1;
                }
            }
        }
    }
}

/// Phases 3-6: RRF fusion, score refinement, graph enrichment, abstention.
#[allow(clippy::too_many_arguments)]
fn fuse_refine_and_output(
    conn: &Connection,
    vector_candidates: Vec<(String, f64, RankedSemanticCandidate)>,
    fts_candidates: Vec<(String, f64, RankedSemanticCandidate)>,
    query: &str,
    query_embedding: &[f32],
    opts: &SearchOptions,
    limit: usize,
    include_superseded: bool,
    explain_enabled: bool,
    scoring_params: &ScoringParams,
    cross_encoder_scores: Option<&HashMap<String, f32>>,
) -> Result<Vec<SemanticResult>> {
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
        #[allow(clippy::cast_precision_loss)]
        let rrf_score = scoring_params.rrf_weight_vec / (scoring_params.rrf_k + rank as f64 + 1.0);
        let et_ref = candidate
            .result
            .event_type
            .as_ref()
            .unwrap_or(&EventType::Memory);
        let type_w = type_weight_et(et_ref);
        let pf = priority_factor(candidate.priority_value, scoring_params);

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
    let mut dual_match_ids: HashMap<String, usize> = HashMap::new();
    for (rank, (id, bm25_raw, candidate)) in fts_candidates.into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let rrf_score = scoring_params.rrf_weight_fts / (scoring_params.rrf_k + rank as f64 + 1.0);
        if let Some(existing) = ranked.get_mut(&id) {
            // Present in both -- add the FTS RRF contribution
            existing.score += candidate.score * rrf_score;
            dual_match_ids.insert(id, rank);
            if explain_enabled && let Some(ref mut exp) = existing.explain {
                exp["fts_rank"] = serde_json::json!(rank);
                exp["fts_bm25"] = serde_json::json!(bm25_raw);
                exp["dual_match"] = serde_json::json!(true);
                // Update rrf_score to show the combined contribution
                let vec_rrf = exp["rrf_score"].as_f64().unwrap_or(0.0);
                exp["rrf_score"] = serde_json::json!(vec_rrf + rrf_score);
            }
        } else {
            let et_ref = candidate
                .result
                .event_type
                .as_ref()
                .unwrap_or(&EventType::Memory);
            let type_w = type_weight_et(et_ref);
            let pf = priority_factor(candidate.priority_value, scoring_params);

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
    // Apply adaptive dual-match boost: candidates in both vector and FTS
    // lists get a multiplicative boost scaled by their FTS rank position.
    // text_rel uses inverse-rank decay: 1/(1+rank), giving a soft falloff
    // (rank 0 → 1.0, rank 1 → 0.5, rank 5 → 0.17, …).  The adaptive
    // boost ranges from 1.3x (low text relevance) to 1.8x (top FTS match).
    if !dual_match_ids.is_empty() {
        for (id, fts_rank) in &dual_match_ids {
            if let Some(candidate) = ranked.get_mut(id) {
                #[allow(clippy::cast_precision_loss)]
                let text_rel = 1.0 / (1.0 + *fts_rank as f64);
                let base = scoring_params.dual_match_boost.max(1.0);
                let adaptive_boost = base + text_rel * 0.5;
                candidate.score *= adaptive_boost;
                if explain_enabled && let Some(ref mut exp) = candidate.explain {
                    exp["adaptive_dual_boost"] = serde_json::json!(adaptive_boost);
                    exp["fts_text_rel"] = serde_json::json!(text_rel);
                }
            }
        }
    }

    // ── Phase 3b: Cross-encoder reranking blend ──────────────────────
    if let Some(ce_scores) = cross_encoder_scores {
        let alpha = scoring_params.rerank_blend_alpha;
        for (id, candidate) in ranked.iter_mut() {
            if let Some(&ce_score) = ce_scores.get(id) {
                let rrf_score = candidate.score;
                candidate.score = alpha * rrf_score + (1.0 - alpha) * ce_score as f64;
                if explain_enabled && let Some(ref mut exp) = candidate.explain {
                    exp["cross_encoder_score"] = serde_json::json!(ce_score);
                    exp["rerank_blend_alpha"] = serde_json::json!(alpha);
                }
            }
        }
    }

    let query_tokens = token_set(query, 3);
    refine_scores(
        &mut ranked,
        &query_tokens,
        opts,
        explain_enabled,
        scoring_params,
    );

    // ── Phase 5: Graph enrichment ──
    enrich_graph_neighbors(
        conn,
        &mut ranked,
        &query_tokens,
        query_embedding,
        limit,
        include_superseded,
        explain_enabled,
        scoring_params,
    );

    // ── Phase 5b: Entity expansion ──
    expand_entity_tags(
        conn,
        &mut ranked,
        &query_tokens,
        limit,
        include_superseded,
        explain_enabled,
        scoring_params,
        opts,
    );

    // ── Phase 6: Collection-level abstention + dedup ─────────────
    let mut deduped = Vec::new();
    let mut seen = HashSet::new();
    for candidate in ranked.into_values() {
        if !matches_search_options(&candidate, opts) {
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
            return Ok(Vec::new());
        }
    }
    if deduped.is_empty() {
        return Ok(Vec::new());
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
        #[allow(clippy::cast_possible_truncation)]
        let normalized_f32 = normalized as f32;
        candidate.result.score = normalized_f32;

        // Always inject text_overlap for confidence computation
        if let serde_json::Value::Object(ref mut meta) = candidate.result.metadata {
            meta.insert(
                "_text_overlap".to_string(),
                serde_json::json!(candidate.text_overlap),
            );
        }

        // Inject explain data into result metadata when enabled
        if explain_enabled && let Some(mut exp) = candidate.explain.take() {
            exp["final_score"] = serde_json::json!(normalized);
            if let serde_json::Value::Object(ref mut meta) = candidate.result.metadata {
                meta.insert("_explain".to_string(), exp);
            }
        }

        out.push(candidate.result);
    }
    Ok(out)
}

fn merge_hot_cache_results(
    hot_results: Vec<SemanticResult>,
    mut results: Vec<SemanticResult>,
    limit: usize,
) -> Vec<SemanticResult> {
    if hot_results.is_empty() || limit == 0 {
        results.truncate(limit);
        return results;
    }

    let mut merged: HashMap<String, SemanticResult> = results
        .drain(..)
        .map(|result| (result.id.clone(), result))
        .collect();

    for hot_result in hot_results {
        if let Some(existing) = merged.get_mut(&hot_result.id) {
            merge_semantic_result(existing, hot_result);
        }
    }

    let mut merged_results: Vec<_> = merged.into_values().collect();
    merged_results.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut deduped = Vec::new();
    let mut seen = HashMap::new();
    for result in merged_results {
        let fingerprint = normalize_for_dedup(&result.content);
        if let Some(existing_idx) = seen.get(&fingerprint).copied() {
            merge_semantic_result(&mut deduped[existing_idx], result);
            continue;
        }
        seen.insert(fingerprint, deduped.len());
        deduped.push(result);
    }
    deduped.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.id.cmp(&right.id))
    });
    deduped.truncate(limit);
    deduped
}

fn merge_semantic_result(existing: &mut SemanticResult, incoming: SemanticResult) {
    if incoming.score > existing.score
        || (incoming.score == existing.score && incoming.id < existing.id)
    {
        let mut replacement = incoming;
        merge_semantic_metadata(
            &mut replacement.metadata,
            std::mem::take(&mut existing.metadata),
        );
        replacement.score = replacement.score.max(existing.score);
        *existing = replacement;
        return;
    }

    existing.score = existing.score.max(incoming.score);
    merge_semantic_metadata(&mut existing.metadata, incoming.metadata);
}

fn merge_semantic_metadata(existing: &mut serde_json::Value, incoming: serde_json::Value) {
    if let (serde_json::Value::Object(existing_meta), serde_json::Value::Object(incoming_meta)) =
        (existing, incoming)
    {
        for (key, value) in incoming_meta {
            existing_meta.entry(key).or_insert(value);
        }
    }
}

/// Run the core search pipeline for a single query: embed -> vector+FTS -> fuse -> refine.
///
/// Used by query decomposition to run each sub-query through the full pipeline.
#[allow(clippy::too_many_arguments)]
async fn run_single_query_pipeline(
    pool: &Arc<ConnPool>,
    embedder: &Arc<dyn Embedder>,
    query: &str,
    candidate_limit: usize,
    limit: usize,
    opts: &SearchOptions,
    scoring_params: &ScoringParams,
    include_superseded: bool,
    explain_enabled: bool,
) -> Result<Vec<SemanticResult>> {
    let intent = classify_query_intent(query);
    let keyword_only = intent == QueryIntent::Keyword;

    let query_embedding = if keyword_only || query.is_empty() {
        Vec::new()
    } else {
        let embedder = Arc::clone(embedder);
        let q = query.to_string();
        tokio::task::spawn_blocking(move || embedder.embed(&q))
            .await
            .context("spawn_blocking join error")??
    };

    let (vector_candidates, fts_candidates) = if keyword_only {
        let pool = Arc::clone(pool);
        let q = query.to_string();
        let o = opts.clone();
        let sp = scoring_params.clone();
        let fts_result = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
        })
        .await
        .context("spawn_blocking join error")??;
        (Vec::new(), fts_result)
    } else if pool.has_readers() {
        let (vec_result, fts_result) = tokio::try_join!(
            tokio::task::spawn_blocking({
                let pool = Arc::clone(pool);
                let emb = query_embedding.clone();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_vector_candidates(
                        &conn,
                        &emb,
                        candidate_limit,
                        include_superseded,
                        &o,
                        &sp,
                    )
                }
            }),
            tokio::task::spawn_blocking({
                let pool = Arc::clone(pool);
                let q = query.to_string();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
                }
            }),
        )
        .context("parallel search join error")?;
        (vec_result?, fts_result?)
    } else {
        let pool = Arc::clone(pool);
        let emb = query_embedding.clone();
        let q = query.to_string();
        let o = opts.clone();
        let sp = scoring_params.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            let vec_c = collect_vector_candidates(
                &conn,
                &emb,
                candidate_limit,
                include_superseded,
                &o,
                &sp,
            )?;
            let fts_c =
                collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)?;
            Ok::<_, anyhow::Error>((vec_c, fts_c))
        })
        .await
        .context("spawn_blocking join error")??
    };

    let ce_scores: Option<HashMap<String, f32>> = None;

    let pool_for_fuse = Arc::clone(pool);
    let q = query.to_string();
    let emb = query_embedding;
    let o = opts.clone();
    let sp = scoring_params.clone();
    tokio::task::spawn_blocking(move || {
        let conn = pool_for_fuse.reader()?;
        fuse_refine_and_output(
            &conn,
            vector_candidates,
            fts_candidates,
            &q,
            &emb,
            &o,
            limit,
            include_superseded,
            explain_enabled,
            &sp,
            ce_scores.as_ref(),
        )
    })
    .await
    .context("spawn_blocking join error")?
}

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

        let today = chrono::Local::now().date_naive();
        let temporal = expand_temporal_query(query, &today);
        let query = temporal.cleaned_query;
        let query_for_decomp = query.clone();
        let mut opts = opts.clone();
        if opts.event_after.is_none()
            && let Some(after) = temporal.event_after
        {
            opts.event_after = Some(after);
        }
        if opts.event_before.is_none()
            && let Some(before) = temporal.event_before
        {
            opts.event_before = Some(before);
        }
        let intent = classify_query_intent(&query);
        let keyword_only = intent == QueryIntent::Keyword;
        let intent_profile = IntentProfile::for_intent(intent);
        let cache_key = query_cache_key(&query, limit, &opts);

        // ── Cache check ──────────────────────────────────────────────────
        if let Ok(mut cache) = self.query_cache.lock()
            && let Some(cached) = cache.get(&cache_key)
            && cached.inserted_at.elapsed().as_secs() < super::QUERY_CACHE_TTL_SECS
        {
            return Ok(cached.results.clone());
        }

        let pool = Arc::clone(&self.pool);
        let embedder = Arc::clone(&self.embedder);
        // Apply intent-based multipliers to scoring params.
        let mut scoring_params = self.scoring_params.clone();
        scoring_params.rrf_weight_vec *= intent_profile.vec_weight_mult;
        scoring_params.rrf_weight_fts *= intent_profile.fts_weight_mult;
        scoring_params.word_overlap_weight *= intent_profile.word_overlap_mult;
        let hot_results = if let Some(hot_cache) = &self.hot_cache {
            if let Err(error) = self.ensure_hot_cache_ready().await {
                tracing::error!(error = %error, "failed to refresh hot tier cache");
            }
            hot_cache.query_with_options(&query, limit, &opts)
        } else {
            Vec::new()
        };
        let hot_has_confident_match = hot_results.iter().any(|result| {
            result
                .metadata
                .get("_text_overlap")
                .and_then(serde_json::Value::as_f64)
                .is_some_and(|overlap| overlap >= scoring_params.abstention_min_text)
        });

        // Phase 0: Embedding computation (blocking).
        // For keyword queries, skip the ONNX embedding (~8ms savings).
        let query_embedding = tokio::task::spawn_blocking({
            let embedder = Arc::clone(&embedder);
            let query = query.clone();
            move || {
                let emb = if keyword_only || query.is_empty() {
                    Vec::new()
                } else {
                    embedder
                        .embed(&query)
                        .context("failed to compute query embedding")?
                };
                Ok::<_, anyhow::Error>(emb)
            }
        })
        .await
        .context("spawn_blocking join error")??;

        let include_superseded = opts.include_superseded.unwrap_or(false);
        let explain_enabled = opts.explain.unwrap_or(false);

        // Apply top_k_mult: scale candidate oversampling while keeping final limit intact.
        let dynamic_mult = detect_dynamic_limit_mult(&query);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let candidate_limit =
            ((limit as f64 * intent_profile.top_k_mult * dynamic_mult).ceil() as usize).max(1);

        // Phases 1+2: Vector search and FTS5 search.
        // Keyword queries skip vector search entirely (FTS5 only).
        // When the pool has dedicated readers, run them on separate
        // connections in parallel. In-memory mode (no readers) falls
        // back to sequential execution on the single writer connection.
        let (vector_candidates, fts_candidates) = if keyword_only {
            let fts_result = tokio::task::spawn_blocking({
                let pool = Arc::clone(&pool);
                let q = query.clone();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
                }
            })
            .await
            .context("spawn_blocking join error")??;
            (Vec::new(), fts_result)
        } else if pool.has_readers() {
            let (vec_result, fts_result) = tokio::try_join!(
                tokio::task::spawn_blocking({
                    let pool = Arc::clone(&pool);
                    let emb = query_embedding.clone();
                    let o = opts.clone();
                    let sp = scoring_params.clone();
                    move || {
                        let conn = pool.reader()?;
                        collect_vector_candidates(
                            &conn,
                            &emb,
                            candidate_limit,
                            include_superseded,
                            &o,
                            &sp,
                        )
                    }
                }),
                tokio::task::spawn_blocking({
                    let pool = Arc::clone(&pool);
                    let q = query.clone();
                    let o = opts.clone();
                    let sp = scoring_params.clone();
                    move || {
                        let conn = pool.reader()?;
                        collect_fts_candidates(
                            &conn,
                            &q,
                            candidate_limit,
                            &o,
                            include_superseded,
                            &sp,
                        )
                    }
                }),
            )
            .context("parallel search join error")?;
            (vec_result?, fts_result?)
        } else {
            // Sequential: single connection (in-memory / test mode).
            tokio::task::spawn_blocking({
                let pool = Arc::clone(&pool);
                let emb = query_embedding.clone();
                let q = query.clone();
                let o = opts.clone();
                let sp = scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    let vec_c = collect_vector_candidates(
                        &conn,
                        &emb,
                        candidate_limit,
                        include_superseded,
                        &o,
                        &sp,
                    )?;
                    let fts_c = collect_fts_candidates(
                        &conn,
                        &q,
                        candidate_limit,
                        &o,
                        include_superseded,
                        &sp,
                    )?;
                    Ok::<_, anyhow::Error>((vec_c, fts_c))
                }
            })
            .await
            .context("spawn_blocking join error")??
        };

        // Capture filter dimensions for cache metadata before opts moves into closure.
        let cache_event_type_filter = opts.event_type.as_ref().map(|et| et.to_string());
        let cache_project_filter = opts.project.clone();
        let cache_session_id_filter = opts.session_id.clone();
        // Clone opts before it moves into the fuse closure so sub-queries can reuse it.
        let opts_for_decomp = opts.clone();

        // Phases 3-6: RRF fusion, score refinement, graph enrichment,
        // abstention + dedup. Needs one reader for graph queries.
        #[cfg(feature = "real-embeddings")]
        let reranker = self.reranker.clone();
        let results = tokio::task::spawn_blocking({
            let pool = Arc::clone(&pool);
            move || {
                // Optional cross-encoder reranking
                #[cfg(feature = "real-embeddings")]
                let ce_scores = compute_cross_encoder_scores(
                    reranker.as_ref(),
                    &query,
                    &vector_candidates,
                    &fts_candidates,
                    &scoring_params,
                );
                #[cfg(not(feature = "real-embeddings"))]
                let ce_scores: Option<HashMap<String, f32>> = None;

                let conn = pool.reader()?;
                fuse_refine_and_output(
                    &conn,
                    vector_candidates,
                    fts_candidates,
                    &query,
                    &query_embedding,
                    &opts,
                    limit,
                    include_superseded,
                    explain_enabled,
                    &scoring_params,
                    ce_scores.as_ref(),
                )
            }
        })
        .await
        .context("spawn_blocking join error")??;

        // ── Query decomposition: enrich results for multi-entity queries ──
        let decomp_entities = extract_query_entities(&query_for_decomp);
        let results = if decomp_entities.len() >= 2 {
            let topics = extract_topic_keywords(&query_for_decomp, &decomp_entities);
            let sub_queries = generate_sub_queries(&query_for_decomp, &decomp_entities, &topics);

            if !topics.is_empty() && sub_queries.len() > 1 {
                let mut all_results = results;
                let mut seen_ids: HashSet<String> =
                    all_results.iter().map(|r| r.id.clone()).collect();

                let decomp_pool = Arc::clone(&self.pool);
                let decomp_embedder = Arc::clone(&self.embedder);
                let decomp_sp = self.scoring_params.clone();
                let decomp_opts = opts_for_decomp.clone();
                // Parallel sub-query execution (resolves #121).
                // ConnPool has 4 dedicated reader connections in WAL mode.
                // Each sub-query internally runs vector + FTS in try_join!,
                // consuming 2 readers simultaneously, so effective parallelism
                // is ~2 sub-queries at a time; additional queries queue on the
                // reader mutexes without deadlock.  Results are merged with
                // dedup after all tasks complete.
                let mut join_set: tokio::task::JoinSet<Result<Vec<SemanticResult>>> =
                    tokio::task::JoinSet::new();
                for sub_query in sub_queries.iter().skip(1) {
                    let pool = Arc::clone(&decomp_pool);
                    let embedder = Arc::clone(&decomp_embedder);
                    let sq = sub_query.clone();
                    let opts = decomp_opts.clone();
                    let sp = decomp_sp.clone();
                    join_set.spawn(async move {
                        run_single_query_pipeline(
                            &pool,
                            &embedder,
                            &sq,
                            candidate_limit,
                            limit,
                            &opts,
                            &sp,
                            include_superseded,
                            explain_enabled,
                        )
                        .await
                    });
                }
                while let Some(task_result) = join_set.join_next().await {
                    let sub_results = task_result.context("sub-query task panicked")??;
                    for result in sub_results {
                        if seen_ids.insert(result.id.clone()) {
                            all_results.push(result);
                        } else if let Some(existing) =
                            all_results.iter_mut().find(|r| r.id == result.id)
                            && result.score > existing.score
                        {
                            existing.score = result.score;
                        }
                    }
                }

                let mut deduped: Vec<SemanticResult> = Vec::new();
                let mut fingerprints: HashSet<String> = HashSet::new();
                all_results.sort_by(|a, b| b.score.total_cmp(&a.score));
                for result in all_results {
                    let fp = content_fingerprint(&result.content);
                    if fingerprints.insert(fp) {
                        deduped.push(result);
                    }
                }
                deduped.truncate(limit);
                deduped
            } else {
                results
            }
        } else {
            results
        };

        let results = if hot_has_confident_match {
            merge_hot_cache_results(hot_results, results, limit)
        } else {
            results
        };

        // ── Cache store ──────────────────────────────────────────────────
        if let Ok(mut cache) = self.query_cache.lock() {
            cache.put(
                cache_key,
                super::CachedQuery {
                    inserted_at: std::time::Instant::now(),
                    results: results.clone(),
                    event_type_filter: cache_event_type_filter,
                    project_filter: cache_project_filter,
                    session_id_filter: cache_session_id_filter,
                },
            );
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::{advanced_fts_candidate_limit, collect_fts_candidates};
    use crate::memory_core::{MemoryInput, SearchOptions, Storage, storage::SqliteStorage};
    use rusqlite::params;

    #[test]
    fn advanced_fts_candidate_limit_is_bounded() {
        assert_eq!(advanced_fts_candidate_limit(1), 100);
        assert_eq!(advanced_fts_candidate_limit(10), 200);
        assert_eq!(advanced_fts_candidate_limit(1_000), 5_000);
        assert_eq!(advanced_fts_candidate_limit(5_001), 5_001);
    }

    #[tokio::test]
    async fn bounded_fts_candidates_preserve_created_at_filters() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        for idx in 0..120 {
            let id = format!("old-{idx}");
            <SqliteStorage as Storage>::store(
                &storage,
                &id,
                "alpha",
                &MemoryInput {
                    content: "alpha".to_string(),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        <SqliteStorage as Storage>::store(
            &storage,
            "recent-match",
            "alpha context details",
            &MemoryInput {
                content: "alpha context details".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.test_conn().unwrap();
        conn.execute(
            "UPDATE memories SET created_at = '2000-01-01T00:00:00.000Z' WHERE id LIKE 'old-%'",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
            params![],
        )
        .unwrap();

        let candidates = collect_fts_candidates(
            &conn,
            "alpha",
            1,
            &SearchOptions {
                created_after: Some("2025-01-01T00:00:00.000Z".to_string()),
                ..Default::default()
            },
            true,
            &storage.scoring_params,
        )
        .unwrap();

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "recent-match");
    }

    #[tokio::test]
    async fn bounded_fts_candidates_preserve_event_at_filters() {
        let storage = SqliteStorage::new_in_memory().unwrap();

        for idx in 0..120 {
            let id = format!("old-event-{idx}");
            <SqliteStorage as Storage>::store(
                &storage,
                &id,
                "alpha",
                &MemoryInput {
                    content: "alpha".to_string(),
                    referenced_date: Some("2000-01-01T00:00:00.000Z".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }

        <SqliteStorage as Storage>::store(
            &storage,
            "recent-event-match",
            "alpha context details",
            &MemoryInput {
                content: "alpha context details".to_string(),
                referenced_date: Some("2025-06-01T00:00:00.000Z".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let conn = storage.test_conn().unwrap();
        let recent_candidates = collect_fts_candidates(
            &conn,
            "alpha",
            1,
            &SearchOptions {
                event_after: Some("2025-01-01T00:00:00.000Z".to_string()),
                ..Default::default()
            },
            true,
            &storage.scoring_params,
        )
        .unwrap();

        assert_eq!(recent_candidates.len(), 1);
        assert_eq!(recent_candidates[0].0, "recent-event-match");
    }
}
