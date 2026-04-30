//! Phase 5: Graph neighbor enrichment + Phase 5b entity tag expansion.

use std::collections::{HashMap, HashSet};

use rusqlite::{Connection, params};

use super::super::dot_product;
use super::super::embedding_codec::decode_embedding;
use super::super::helpers::{
    EPOCH_FALLBACK, event_type_from_sql, extract_entities_from_tags, matches_search_options,
    parse_metadata_from_db, parse_tags_from_db, resolve_priority,
};
use super::super::storage::RankedSemanticCandidate;
use crate::memory_core::domain::{
    REL_PARALLEL_CONTEXT, REL_PRECEDED_BY, REL_RELATES_TO, REL_SHARES_THEME, REL_SIMILAR_TO,
};
use crate::memory_core::{
    EventType, ScoringParams, SearchOptions, SemanticResult, feedback_factor, priority_factor,
    time_decay_et, token_set, type_weight_et, word_overlap_pre,
};

/// Phase 5: Graph enrichment — inject 1-hop neighbors from top-scoring seeds.
#[allow(clippy::too_many_arguments)]
pub(crate) fn enrich_graph_neighbors(
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
pub(crate) fn expand_entity_tags(
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
