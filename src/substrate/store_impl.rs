use crate::memory_core::storage::sqlite::SqliteStorage;
use crate::memory_core::storage::sqlite::pipeline::enrichment::{
    enrich_graph_neighbors, expand_entity_tags,
};
use crate::memory_core::storage::sqlite::pipeline::retrieval::{
    collect_fts_candidates as collect_fts, collect_vector_candidates as collect_vec,
};
use crate::memory_core::{
    BackupInfo, CheckpointInput, GraphNode, ListResult, MemoryInput, MemoryUpdate, SearchOptions,
    SearchResult, SemanticResult,
};
use crate::substrate::traits::MemoryStore;
use crate::substrate::types::{CandidateSet, ScoredCandidate};
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

fn convert_ranked(
    c: crate::memory_core::storage::sqlite::RankedSemanticCandidate,
) -> ScoredCandidate {
    ScoredCandidate {
        result: c.result,
        created_at: c.created_at,
        event_at: c.event_at,
        score: c.score,
        priority_value: c.priority_value,
        vec_sim: c.vec_sim,
        text_overlap: c.text_overlap,
        entity_id: c.entity_id,
        agent_type: c.agent_type,
        explain: c.explain,
    }
}

#[async_trait]
impl MemoryStore for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        self.store(id, data, input).await
    }

    async fn retrieve(&self, id: &str) -> Result<String> {
        self.retrieve(id).await
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        self.delete(id).await
    }

    async fn update(&self, id: &str, update: &MemoryUpdate) -> Result<()> {
        self.update(id, update).await
    }

    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.search(query, limit, opts).await
    }

    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.semantic_search(query, limit, opts).await
    }

    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.advanced_search(query, limit, opts).await
    }

    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.phrase_search(phrase, limit, opts).await
    }

    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        self.recent(limit, opts).await
    }

    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.get_by_tags(tags, limit, opts).await
    }

    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult> {
        self.list(offset, limit, opts).await
    }

    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>> {
        self.traverse(start_id, max_hops, min_weight, edge_types)
            .await
    }

    async fn get_relationships(
        &self,
        memory_id: &str,
    ) -> Result<Vec<crate::memory_core::Relationship>> {
        self.get_relationships(memory_id).await
    }

    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        self.find_similar(memory_id, limit).await
    }

    async fn get_version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>> {
        self.get_version_chain(memory_id).await
    }

    async fn sweep_expired(&self) -> Result<usize> {
        self.sweep_expired().await
    }

    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.record_feedback(memory_id, rating, reason).await
    }

    async fn get_profile(&self) -> Result<serde_json::Value> {
        self.get_profile().await
    }

    async fn set_profile(&self, updates: &serde_json::Value) -> Result<()> {
        self.set_profile(updates).await
    }

    async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String> {
        self.save_checkpoint(input).await
    }

    async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        self.resume_task(query, project, limit).await
    }

    async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.create_reminder(text, duration_str, context, session_id, project)
            .await
    }

    async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>> {
        self.list_reminders(status).await
    }

    async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value> {
        self.dismiss_reminder(reminder_id).await
    }

    async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        self.query_lessons(task, project, exclude_session, agent_type, limit)
            .await
    }

    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value> {
        self.check_health(warn_mb, critical_mb, max_nodes).await
    }

    async fn consolidate(&self, prune_days: i64, max_summaries: i64) -> Result<serde_json::Value> {
        self.consolidate(prune_days, max_summaries).await
    }

    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        self.compact(event_type, similarity_threshold, min_cluster_size, dry_run)
            .await
    }

    async fn clear_session(&self, session_id: &str) -> Result<usize> {
        self.clear_session(session_id).await
    }

    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        self.auto_compact(count_threshold, dry_run).await
    }

    async fn type_stats(&self) -> Result<serde_json::Value> {
        self.type_stats().await
    }

    async fn session_stats(&self) -> Result<serde_json::Value> {
        self.session_stats().await
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        self.weekly_digest(days).await
    }

    async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        self.access_rate_stats().await
    }

    async fn create_backup(&self) -> Result<BackupInfo> {
        self.create_backup().await
    }

    async fn rotate_backups(&self, max_count: usize) -> Result<usize> {
        self.rotate_backups(max_count).await
    }

    async fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        self.list_backups().await
    }

    async fn restore_backup(&self, backup_path: &std::path::Path) -> Result<()> {
        self.restore_backup(backup_path).await
    }

    async fn maybe_startup_backup(&self) -> Result<Option<BackupInfo>> {
        self.maybe_startup_backup().await
    }

    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        self.welcome(session_id, project).await
    }

    async fn collect_vector_candidates(
        &self,
        query_embedding: &[f32],
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &crate::memory_core::scoring::ScoringParams,
    ) -> Result<CandidateSet> {
        let pool = Arc::clone(&self.pool);
        let query_embedding = query_embedding.to_vec();
        let opts = opts.clone();
        let scoring_params = scoring_params.clone();

        let candidates = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            collect_vec(
                &conn,
                &query_embedding,
                limit,
                include_superseded,
                &opts,
                &scoring_params,
            )
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(candidates
            .into_iter()
            .map(|(id, score, c)| (id, score, convert_ranked(c)))
            .collect())
    }

    async fn collect_fts_candidates(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
        include_superseded: bool,
        scoring_params: &crate::memory_core::scoring::ScoringParams,
    ) -> Result<CandidateSet> {
        let pool = Arc::clone(&self.pool);
        let query = query.to_string();
        let opts = opts.clone();
        let scoring_params = scoring_params.clone();

        let candidates = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            collect_fts(
                &conn,
                &query,
                limit,
                &opts,
                include_superseded,
                &scoring_params,
            )
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(candidates
            .into_iter()
            .map(|(id, score, c)| (id, score, convert_ranked(c)))
            .collect())
    }
    async fn enrich_graph_neighbors(
        &self,
        mut candidates: HashMap<String, ScoredCandidate>,
        query_tokens: &HashSet<String>,
        query_embedding: &[f32],
        limit: usize,
        include_superseded: bool,
        explain_enabled: bool,
        scoring_params: &crate::memory_core::scoring::ScoringParams,
    ) -> Result<HashMap<String, ScoredCandidate>> {
        let pool = Arc::clone(&self.pool);
        let query_tokens = query_tokens.clone();
        let query_embedding = query_embedding.to_vec();
        let scoring_params = scoring_params.clone();

        let candidates = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            enrich_graph_neighbors(
                &conn,
                &mut candidates,
                &query_tokens,
                &query_embedding,
                limit,
                include_superseded,
                explain_enabled,
                &scoring_params,
            );
            Ok::<_, anyhow::Error>(candidates)
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(candidates)
    }

    async fn expand_entity_tags(
        &self,
        mut candidates: HashMap<String, ScoredCandidate>,
        query_tokens: &HashSet<String>,
        limit: usize,
        include_superseded: bool,
        explain_enabled: bool,
        scoring_params: &crate::memory_core::scoring::ScoringParams,
        opts: &SearchOptions,
    ) -> Result<HashMap<String, ScoredCandidate>> {
        let pool = Arc::clone(&self.pool);
        let query_tokens = query_tokens.clone();
        let scoring_params = scoring_params.clone();
        let opts = opts.clone();

        let candidates = tokio::task::spawn_blocking(move || {
            let conn = pool.reader()?;
            expand_entity_tags(
                &conn,
                &mut candidates,
                &query_tokens,
                limit,
                include_superseded,
                explain_enabled,
                &scoring_params,
                &opts,
            );
            Ok::<_, anyhow::Error>(candidates)
        })
        .await
        .context("spawn_blocking join error")??;

        Ok(candidates)
    }
}
