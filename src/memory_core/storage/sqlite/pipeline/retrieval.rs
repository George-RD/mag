//! Phase 1 (vector candidates) and Phase 2 (FTS candidates) retrieval.

use anyhow::{Context, Result};
use rusqlite::Connection;

#[cfg(not(feature = "sqlite-vec"))]
use super::super::dot_product;
#[cfg(not(feature = "sqlite-vec"))]
use super::super::embedding_codec::decode_embedding;
use super::super::helpers::{
    EPOCH_FALLBACK, append_search_filters, build_fts5_query, event_type_from_sql,
    parse_metadata_from_db, parse_tags_from_db, resolve_priority, to_param_refs,
};
#[cfg(feature = "sqlite-vec")]
use super::super::helpers::{hydrate_memories_by_ids, vec_distance_to_similarity, vec_knn_search};
use super::super::storage::RankedSemanticCandidate;
use super::advanced_fts_candidate_limit;
use crate::memory_core::{
    EventType, ScoringParams, SearchOptions, SemanticResult, priority_factor, type_weight_et,
};

/// Phase 1: Collect vector candidates sorted by cosine similarity.
pub(crate) fn collect_vector_candidates(
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
pub(crate) fn collect_fts_candidates(
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
