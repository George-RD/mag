//! Phase 1 (vector candidates) and Phase 2 (FTS candidates) retrieval.

use std::sync::Arc;

use anyhow::{Context, Result};
use rusqlite::Connection;

use super::super::conn_pool::ConnPool;
#[cfg(not(feature = "sqlite-vec"))]
use super::super::dot_product;
#[cfg(not(feature = "sqlite-vec"))]
use super::super::embedding_codec::{decode_embedding, dot_product_bytes};
use super::super::helpers::{
    EPOCH_FALLBACK, append_search_filters, build_fts5_query, event_type_from_sql,
    parse_metadata_from_db, parse_tags_from_db, resolve_priority, to_param_refs,
};
#[cfg(feature = "sqlite-vec")]
use super::super::helpers::{hydrate_memories_by_ids, vec_distance_to_similarity, vec_knn_search};
use super::super::storage::RankedSemanticCandidate;
use super::advanced_fts_candidate_limit;
use crate::memory_core::retrieval_strategy::{CandidateSet, QueryContext};
use crate::memory_core::{
    EventType, ScoringParams, SearchOptions, SemanticResult, priority_factor, type_weight_et,
};

/// Phase 1: Collect vector candidates sorted by cosine similarity.
pub(crate) fn collect_vector_candidates(
    conn: &Connection,
    query_embedding: &[f32],
    limit: usize,
    include_superseded: bool,
    opts: &SearchOptions,
    scoring_params: &ScoringParams,
) -> Result<Vec<(String, f64, RankedSemanticCandidate)>> {
    let mut vector_candidates: Vec<(String, f64, RankedSemanticCandidate)> =
        Vec::with_capacity(limit.saturating_mul(10).clamp(200, 10_000));

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
        use rusqlite::types::Value as SqlValue;

        // Apply SearchOptions filters (project, session_id, event_type,
        // dates, etc.) at the SQL layer so the brute-force scan is bounded
        // to the candidate set the user actually asked for. Mirrors the
        // pre-filter semantics of the sqlite-vec branch.
        let mut vector_sql = String::from(
            "SELECT id, content, embedding, tags, importance, metadata, event_type, session_id, project, priority, created_at, entity_id, agent_type, event_at \
             FROM memories WHERE embedding IS NOT NULL",
        );
        if !include_superseded {
            vector_sql.push_str(" AND superseded_by_id IS NULL");
        }
        let mut vector_params: Vec<SqlValue> = Vec::new();
        let mut param_idx = 1;
        append_search_filters(
            &mut vector_sql,
            &mut vector_params,
            &mut param_idx,
            opts,
            "",
        );
        // Cap the brute-force scan to keep latency bounded on large tables.
        // Uses limit*5 with a floor of 100 (vs limit*10 floor 200 for sqlite-vec)
        // because the non-vec branch does a recency-ordered scan rather than
        // true KNN, so fewer candidates are needed to bound latency.
        let scan_limit = limit.saturating_mul(5).clamp(100, 10_000);
        let scan_limit_sql = i64::try_from(scan_limit).unwrap_or(i64::MAX);
        vector_sql.push_str(" ORDER BY created_at DESC LIMIT ?");
        vector_sql.push_str(&param_idx.to_string());
        vector_params.push(SqlValue::Integer(scan_limit_sql));
        let mut vector_stmt = conn
            .prepare_cached(&vector_sql)
            .context("failed to prepare advanced vector query")?;
        let param_refs = to_param_refs(&vector_params);
        let mut rows = vector_stmt
            .query(param_refs.as_slice())
            .context("failed to execute advanced vector query")?;
        while let Some(row) = rows.next()? {
            // Fetch embedding first and compute similarity; skip early if
            // the candidate is too dissimilar, avoiding the cost of cloning
            // the remaining fields.
            let embedding_blob: Vec<u8> = row.get(2)?;
            let similarity = if embedding_blob.len() == query_embedding.len() * 4
                && embedding_blob.first().copied().unwrap_or(0) != b'['
            {
                dot_product_bytes(query_embedding, &embedding_blob) as f64
            } else {
                let candidate_emb: Vec<f32> = decode_embedding(&embedding_blob)
                    .context("failed to decode stored embedding")?;
                dot_product(query_embedding, &candidate_emb) as f64
            };
            if similarity < 0.1 {
                continue;
            }
            let id: String = row.get(0)?;
            let content: String = row.get(1)?;
            let raw_tags: String = row.get(3)?;
            let importance: f64 = row.get(4)?;
            let raw_metadata: String = row.get(5)?;
            let event_type_str: Option<String> = row.get(6).ok().flatten();
            let session_id: Option<String> = row.get(7).ok().flatten();
            let project: Option<String> = row.get(8).ok().flatten();
            let priority: Option<i64> = row.get(9).ok().flatten();
            let created_at: String = row.get(10).unwrap_or_else(|_| EPOCH_FALLBACK.to_string());
            let entity_id: Option<String> = row.get(11).ok().flatten();
            let agent_type: Option<String> = row.get(12).ok().flatten();
            let event_at: String = row.get(13).unwrap_or_else(|_| EPOCH_FALLBACK.to_string());
            let et = event_type_from_sql(event_type_str);
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
    vector_candidates.sort_unstable_by(|a, b| b.1.total_cmp(&a.1));
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

    let mut fts_candidates: Vec<(String, f64, RankedSemanticCandidate)> =
        Vec::with_capacity(advanced_fts_candidate_limit(limit));

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

    let fts_stmt = conn.prepare_cached(&fts_sql);
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
                let et = event_type_from_sql(event_type);
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
    fts_candidates.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
    Ok(fts_candidates)
}

/// Run Phase 1 (vector) and Phase 2 (FTS) candidate retrieval, in parallel
/// when the connection pool has dedicated readers and sequentially otherwise.
///
/// In WAL-pooled mode the two scans run on independent reader connections
/// via `try_join!`. In single-connection (in-memory / test) mode they share
/// the writer connection, so we run them sequentially inside one
/// `spawn_blocking` to keep the SQLite connection on a single thread.
pub(crate) async fn collect_dual_candidates(
    pool: &Arc<ConnPool>,
    ctx: &QueryContext,
) -> Result<(CandidateSet, CandidateSet)> {
    let candidate_limit = ctx.candidate_limit;
    let include_superseded = ctx.include_superseded;

    if pool.has_readers() {
        let (vec_result, fts_result) = tokio::try_join!(
            tokio::task::spawn_blocking({
                let pool = Arc::clone(pool);
                let emb = ctx.embedding_slice().to_vec();
                let o = ctx.opts.clone();
                let sp = ctx.scoring_params.clone();
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
                let q = ctx.query.clone();
                let o = ctx.opts.clone();
                let sp = ctx.scoring_params.clone();
                move || {
                    let conn = pool.reader()?;
                    collect_fts_candidates(&conn, &q, candidate_limit, &o, include_superseded, &sp)
                }
            }),
        )
        .context("parallel search join error")?;
        Ok((vec_result?, fts_result?))
    } else {
        // Sequential: single connection (in-memory / test mode).
        tokio::task::spawn_blocking({
            let pool = Arc::clone(pool);
            let emb = ctx.embedding_slice().to_vec();
            let q = ctx.query.clone();
            let o = ctx.opts.clone();
            let sp = ctx.scoring_params.clone();
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
        .context("spawn_blocking join error")?
    }
}
