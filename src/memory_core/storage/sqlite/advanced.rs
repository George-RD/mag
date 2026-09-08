use super::pipeline;
use super::query_classifier::{
    IntentProfile, QueryIntent, classify_query_intent, detect_dynamic_limit_mult,
};
use super::*;
use crate::memory_core::EmbeddingInputKind;
use crate::memory_core::retrieval_strategy::{CandidateSet, FtsSearcher, QueryContext};

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
            let conn = pool.embedding_snapshot(&conn)?;
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
        let result = self.advanced_search_in_generation(query, limit, opts).await;
        self.finish_embedding_read(result).await
    }
}

impl SqliteStorage {
    async fn advanced_search_in_generation(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
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

        let cache_event_type_filter = opts.event_type.as_ref().map(|et| et.to_string());
        let cache_project_filter = opts.project.clone();
        let cache_session_id_filter = opts.session_id.clone();

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

        let mut ctx = QueryContext {
            query,
            limit,
            candidate_limit,
            opts,
            scoring_params,
            query_embedding: None,
            include_superseded,
            explain_enabled,
        };

        let results = if intent == QueryIntent::Keyword || ctx.query.trim().is_empty() {
            tracing::debug!(query = %ctx.query, strategy = "keyword-only", "dispatching retrieval strategy");
            let hot_match = hot_has_confident_match.then_some(hot_results);
            pipeline::run_keyword_only_search(self, &self.scoring_strategy, &ctx, hot_match).await?
        } else {
            // Phase 0: Embedding computation (blocking).
            // Keyword queries have already been handled above via
            // KeywordOnlyStrategy, so all remaining queries require an
            // embedding.
            let query_embedding = tokio::task::spawn_blocking({
                let embedder = Arc::clone(&embedder);
                let query = ctx.query.clone();
                move || {
                    let emb = if query.is_empty() {
                        Vec::new()
                    } else {
                        embedder
                            .embed_for(EmbeddingInputKind::Query, &query)
                            .context("failed to compute query embedding")?
                    };
                    Ok::<_, anyhow::Error>(emb)
                }
            })
            .await
            .context("spawn_blocking join error")??;

            ctx.query_embedding = Some(query_embedding);

            let (vector_candidates, fts_candidates) =
                pipeline::collect_dual_candidates(&pool, &ctx).await?;

            // Build the sub-query decomposition context now, before `ctx`
            // is moved into the fusion `spawn_blocking`. Sub-queries inherit
            // intent-adjusted scoring params and the candidate / final limits
            // but recompute their own embeddings, so we explicitly omit the
            // parent's embedding (would otherwise be a wasted Vec<f32> clone).
            let decomp_ctx = QueryContext {
                query: query_for_decomp,
                limit: ctx.limit,
                candidate_limit: ctx.candidate_limit,
                opts: ctx.opts.clone(),
                scoring_params: ctx.scoring_params.clone(),
                query_embedding: None,
                include_superseded: ctx.include_superseded,
                explain_enabled: ctx.explain_enabled,
            };

            // Phases 3-6: RRF fusion, score refinement, graph enrichment,
            // abstention + dedup. Needs one reader for graph queries.
            let reranker = self.reranker.clone();
            let scoring_strategy = Arc::clone(&self.scoring_strategy);
            let results = tokio::task::spawn_blocking({
                let pool = Arc::clone(&pool);
                // Move `ctx` into the closure to hand the embedding off
                // without an extra clone; `decomp_ctx` already holds the
                // copies it needs for the post-fuse decomposition step.
                let ctx_for_fuse = ctx;
                move || {
                    // Optional cross-encoder reranking (sync, safe inside spawn_blocking)
                    let ce_scores = pipeline::compute_cross_encoder_scores(
                        reranker.as_ref(),
                        &ctx_for_fuse.query,
                        &vector_candidates,
                        &fts_candidates,
                        &ctx_for_fuse.scoring_params,
                    );

                    let conn = pool.reader()?;
                    let conn = pool.embedding_snapshot(&conn)?;
                    // `fuse_and_score` orchestrates phases 3-6 internally
                    // (RRF fusion -> refine -> graph enrichment -> entity expansion
                    // -> abstention/dedup via `abstain_and_dedup`).
                    pipeline::fuse_and_score(
                        &conn,
                        vector_candidates,
                        fts_candidates,
                        &ctx_for_fuse,
                        ce_scores.as_ref(),
                        scoring_strategy.as_ref(),
                    )
                }
            })
            .await
            .context("spawn_blocking join error")??;

            let results = pipeline::enrich_with_decomposition(
                &self.pool,
                &self.embedder,
                &self.scoring_strategy,
                &decomp_ctx,
                results,
            )
            .await?;

            if hot_has_confident_match {
                pipeline::merge_hot_cache_results(hot_results, results, limit)
            } else {
                results
            }
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
#[path = "advanced_tests.rs"]
mod tests;
