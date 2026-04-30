use super::nlp::{
    content_fingerprint, extract_query_entities, extract_topic_keywords, generate_sub_queries,
};
use super::pipeline;
use super::query_classifier::{
    IntentProfile, QueryIntent, classify_query_intent, detect_dynamic_limit_mult,
};
use super::*;
use crate::memory_core::retrieval_strategy::{CandidateSet, FtsSearcher};

#[async_trait]
impl FtsSearcher for SqliteStorage {
    async fn fts_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &ScoringParams,
    ) -> Result<CandidateSet> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let pool = Arc::clone(&self.pool);
        let query = query.to_string();
        let opts = opts.clone();
        let scoring_params = scoring_params.clone();

        tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            pipeline::collect_fts_candidates(
                &conn,
                &query,
                limit,
                &opts,
                include_superseded,
                &scoring_params,
            )
        })
        .await
        .context("spawn_blocking join error")?
    }
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

        // ── KeywordOnlyStrategy dispatch ────────────────────────────────
        // For keyword-intent queries, skip embedding, vector search,
        // RRF fusion, reranker, and graph enrichment. Use FTS5 BM25 only.
        if intent == QueryIntent::Keyword {
            tracing::debug!(query = %query, "dispatching to KeywordOnlyStrategy");
            let include_superseded = opts.include_superseded.unwrap_or(false);
            let explain_enabled = opts.explain.unwrap_or(false);
            let candidates: CandidateSet = self
                .fts_search(&query, limit, &opts, include_superseded, &scoring_params)
                .await?;

            let scoring_strategy = Arc::clone(&self.scoring_strategy);
            let query_owned = query.clone();
            let sp = scoring_params.clone();
            let results = tokio::task::spawn_blocking(move || {
                pipeline::keyword_candidates_to_results(
                    candidates,
                    &query_owned,
                    limit,
                    &sp,
                    scoring_strategy.as_ref(),
                    explain_enabled,
                )
            })
            .await
            .context("spawn_blocking join error")?;

            let results = if hot_has_confident_match {
                pipeline::merge_hot_cache_results(hot_results, results, limit)
            } else {
                results
            };

            // ── Cache store ──────────────────────────────────────────────
            let cache_event_type_filter = opts.event_type.as_ref().map(|et| et.to_string());
            let cache_project_filter = opts.project.clone();
            let cache_session_id_filter = opts.session_id.clone();
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

            return Ok(results);
        }

        // Phase 0: Embedding computation (blocking).
        // Keyword queries have already returned above via KeywordOnlyStrategy,
        // so all remaining queries require an embedding.
        let query_embedding = tokio::task::spawn_blocking({
            let embedder = Arc::clone(&embedder);
            let query = query.clone();
            move || {
                let emb = if query.is_empty() {
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

        // Phases 1+2: Vector search and FTS5 search (non-keyword queries).
        // Keyword queries were dispatched via KeywordOnlyStrategy above.
        // When the pool has dedicated readers, run them on separate
        // connections in parallel. In-memory mode (no readers) falls
        // back to sequential execution on the single writer connection.
        let (vector_candidates, fts_candidates) = if pool.has_readers() {
            let (vec_result, fts_result) = tokio::try_join!(
                tokio::task::spawn_blocking({
                    let pool = Arc::clone(&pool);
                    let emb = query_embedding.clone();
                    let o = opts.clone();
                    let sp = scoring_params.clone();
                    move || {
                        let conn = pool.reader()?;
                        pipeline::collect_vector_candidates(
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
                        pipeline::collect_fts_candidates(
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
                    let vec_c = pipeline::collect_vector_candidates(
                        &conn,
                        &emb,
                        candidate_limit,
                        include_superseded,
                        &o,
                        &sp,
                    )?;
                    let fts_c = pipeline::collect_fts_candidates(
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
        let reranker = self.reranker.clone();
        let scoring_strategy = Arc::clone(&self.scoring_strategy);
        let results = tokio::task::spawn_blocking({
            let pool = Arc::clone(&pool);
            move || {
                // Optional cross-encoder reranking (sync, safe inside spawn_blocking)
                let ce_scores = pipeline::compute_cross_encoder_scores(
                    reranker.as_ref(),
                    &query,
                    &vector_candidates,
                    &fts_candidates,
                    &scoring_params,
                );

                let conn = pool.reader()?;
                // `fuse_and_score` orchestrates phases 3-6 internally
                // (RRF fusion -> refine -> graph enrichment -> entity expansion
                // -> abstention/dedup via `abstain_and_dedup`).
                pipeline::fuse_and_score(
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
                    scoring_strategy.as_ref(),
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
                let decomp_strat = Arc::clone(&self.scoring_strategy);
                // Parallel sub-query execution (resolves #121).
                // ConnPool has 4 dedicated reader connections in WAL mode.
                // Each sub-query internally runs vector + FTS in try_join!,
                // consuming 2 readers simultaneously, so effective parallelism
                // is ~2 sub-queries at a time; additional queries queue on the
                // reader mutexes without deadlock.  Results are collected with
                // their original index and sorted before merging to preserve
                // deterministic dedup ordering.
                let mut join_set: tokio::task::JoinSet<(usize, Result<Vec<SemanticResult>>)> =
                    tokio::task::JoinSet::new();
                for (idx, sub_query) in sub_queries.iter().skip(1).enumerate() {
                    let pool = Arc::clone(&decomp_pool);
                    let embedder = Arc::clone(&decomp_embedder);
                    let sq = sub_query.clone();
                    let opts = decomp_opts.clone();
                    let sp = decomp_sp.clone();
                    let strat = Arc::clone(&decomp_strat);
                    join_set.spawn(async move {
                        let res = pipeline::run_single_query_pipeline(
                            &pool,
                            &embedder,
                            &sq,
                            candidate_limit,
                            limit,
                            &opts,
                            &sp,
                            include_superseded,
                            explain_enabled,
                            &strat,
                        )
                        .await;
                        (idx, res)
                    });
                }
                // Collect all results, then sort by original sub-query index
                // so merge order is deterministic (same as the old sequential loop).
                let mut indexed_results: Vec<(usize, Vec<SemanticResult>)> = Vec::new();
                while let Some(task_result) = join_set.join_next().await {
                    let (idx, sub_results) = task_result.context("sub-query task panicked")?;
                    indexed_results.push((idx, sub_results?));
                }
                indexed_results.sort_by_key(|(idx, _)| *idx);
                for (_idx, sub_results) in indexed_results {
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
            pipeline::merge_hot_cache_results(hot_results, results, limit)
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
    use super::pipeline::{advanced_fts_candidate_limit, collect_fts_candidates};
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

    /// Integration test: keyword-intent queries go through KeywordOnlyStrategy
    /// dispatch and still return relevant FTS5 results.
    #[tokio::test]
    async fn keyword_dispatch_returns_fts_results() {
        use crate::memory_core::AdvancedSearcher;

        let storage = SqliteStorage::new_in_memory().unwrap();

        // Store memories with identifiable content.
        <SqliteStorage as Storage>::store(
            &storage,
            "func-1",
            "SqliteStorage implementation details",
            &MemoryInput {
                content: "SqliteStorage implementation details".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "func-2",
            "McpMemoryServer handles tool routing",
            &MemoryInput {
                content: "McpMemoryServer handles tool routing".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // CamelCase query triggers keyword intent -> KeywordOnlyStrategy.
        let results = storage
            .advanced_search("SqliteStorage", 10, &SearchOptions::default())
            .await
            .unwrap();

        assert!(!results.is_empty(), "keyword query should return results");
        assert!(
            results.iter().any(|r| r.content.contains("SqliteStorage")),
            "should find the SqliteStorage memory"
        );
    }

    /// Integration test: non-keyword queries still go through the full pipeline.
    #[tokio::test]
    async fn non_keyword_query_uses_full_pipeline() {
        use crate::memory_core::AdvancedSearcher;

        let storage = SqliteStorage::new_in_memory().unwrap();

        <SqliteStorage as Storage>::store(
            &storage,
            "mem-1",
            "The database uses SQLite for storage",
            &MemoryInput {
                content: "The database uses SQLite for storage".to_string(),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Natural language query -> NOT keyword intent -> full pipeline.
        let results = storage
            .advanced_search(
                "What database does the project use?",
                10,
                &SearchOptions::default(),
            )
            .await
            .unwrap();

        // Should still return results through the full pipeline.
        assert!(!results.is_empty(), "full pipeline should return results");
    }
}
