use crate::memory_core::storage::sqlite::SqliteStorage;
use crate::memory_core::storage::sqlite::pipeline::enrichment::{
    enrich_graph_neighbors, expand_entity_tags,
};
use crate::memory_core::storage::sqlite::pipeline::retrieval::{
    collect_fts_candidates as collect_fts, collect_vector_candidates as collect_vec,
};
use crate::memory_core::{
    AdvancedSearcher, BackupInfo, BackupManager, CheckpointInput, CheckpointManager, Deleter,
    ExpirationSweeper, FeedbackRecorder, GraphNode, GraphTraverser, LessonQuerier, ListResult,
    Lister, MaintenanceManager, MemoryInput, MemoryUpdate, PhraseSearcher, ProfileManager, Recents,
    RelationshipQuerier, ReminderManager, Retriever, SearchOptions, SearchResult, Searcher,
    SemanticResult, SemanticSearcher, SimilarFinder, StatsProvider, Storage, Tagger, Updater,
    VersionChainQuerier, WelcomeProvider,
};
use crate::substrate::traits::MemoryStore;
use crate::substrate::types::{CandidateSet, ScoredCandidate};
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
#[async_trait]
impl MemoryStore for SqliteStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        <Self as Storage>::store(self, id, data, input).await
    }

    async fn retrieve(&self, id: &str) -> Result<String> {
        <Self as Retriever>::retrieve(self, id).await
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        <Self as Deleter>::delete(self, id).await
    }

    async fn update(&self, id: &str, update: &MemoryUpdate) -> Result<()> {
        <Self as Updater>::update(self, id, update).await
    }

    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        <Self as Searcher>::search(self, query, limit, opts).await
    }

    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        <Self as SemanticSearcher>::semantic_search(self, query, limit, opts).await
    }

    async fn advanced_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        <Self as AdvancedSearcher>::advanced_search(self, query, limit, opts).await
    }

    async fn phrase_search(
        &self,
        phrase: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        <Self as PhraseSearcher>::phrase_search(self, phrase, limit, opts).await
    }

    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        <Self as Recents>::recent(self, limit, opts).await
    }

    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        <Self as Tagger>::get_by_tags(self, tags, limit, opts).await
    }

    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult> {
        <Self as Lister>::list(self, offset, limit, opts).await
    }

    async fn traverse(
        &self,
        start_id: &str,
        max_hops: usize,
        min_weight: f64,
        edge_types: Option<&[String]>,
    ) -> Result<Vec<GraphNode>> {
        <Self as GraphTraverser>::traverse(self, start_id, max_hops, min_weight, edge_types).await
    }

    async fn get_relationships(
        &self,
        memory_id: &str,
    ) -> Result<Vec<crate::memory_core::Relationship>> {
        <Self as RelationshipQuerier>::get_relationships(self, memory_id).await
    }

    async fn find_similar(&self, memory_id: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        <Self as SimilarFinder>::find_similar(self, memory_id, limit).await
    }

    async fn get_version_chain(&self, memory_id: &str) -> Result<Vec<SearchResult>> {
        <Self as VersionChainQuerier>::get_version_chain(self, memory_id).await
    }

    async fn sweep_expired(&self) -> Result<usize> {
        <Self as ExpirationSweeper>::sweep_expired(self).await
    }

    async fn record_feedback(
        &self,
        memory_id: &str,
        rating: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value> {
        <Self as FeedbackRecorder>::record_feedback(self, memory_id, rating, reason).await
    }

    async fn get_profile(&self) -> Result<serde_json::Value> {
        <Self as ProfileManager>::get_profile(self).await
    }

    async fn set_profile(&self, updates: &serde_json::Value) -> Result<()> {
        <Self as ProfileManager>::set_profile(self, updates).await
    }

    async fn save_checkpoint(&self, input: CheckpointInput) -> Result<String> {
        <Self as CheckpointManager>::save_checkpoint(self, input).await
    }

    async fn resume_task(
        &self,
        query: &str,
        project: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        <Self as CheckpointManager>::resume_task(self, query, project, limit).await
    }
    async fn create_reminder(
        &self,
        text: &str,
        duration_str: &str,
        context: Option<&str>,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        <Self as ReminderManager>::create_reminder(
            self,
            text,
            duration_str,
            context,
            session_id,
            project,
        )
        .await
    }

    async fn list_reminders(&self, status: Option<&str>) -> Result<Vec<serde_json::Value>> {
        <Self as ReminderManager>::list_reminders(self, status).await
    }

    async fn dismiss_reminder(&self, reminder_id: &str) -> Result<serde_json::Value> {
        <Self as ReminderManager>::dismiss_reminder(self, reminder_id).await
    }

    async fn query_lessons(
        &self,
        task: Option<&str>,
        project: Option<&str>,
        exclude_session: Option<&str>,
        agent_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<serde_json::Value>> {
        <Self as LessonQuerier>::query_lessons(
            self,
            task,
            project,
            exclude_session,
            agent_type,
            limit,
        )
        .await
    }

    async fn check_health(
        &self,
        warn_mb: f64,
        critical_mb: f64,
        max_nodes: i64,
    ) -> Result<serde_json::Value> {
        <Self as MaintenanceManager>::check_health(self, warn_mb, critical_mb, max_nodes).await
    }

    async fn consolidate(&self, prune_days: i64, max_summaries: i64) -> Result<serde_json::Value> {
        <Self as MaintenanceManager>::consolidate(self, prune_days, max_summaries).await
    }

    async fn compact(
        &self,
        event_type: &str,
        similarity_threshold: f64,
        min_cluster_size: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        <Self as MaintenanceManager>::compact(
            self,
            event_type,
            similarity_threshold,
            min_cluster_size,
            dry_run,
        )
        .await
    }

    async fn clear_session(&self, session_id: &str) -> Result<usize> {
        <Self as MaintenanceManager>::clear_session(self, session_id).await
    }

    async fn auto_compact(
        &self,
        count_threshold: usize,
        dry_run: bool,
    ) -> Result<serde_json::Value> {
        <Self as MaintenanceManager>::auto_compact(self, count_threshold, dry_run).await
    }

    async fn type_stats(&self) -> Result<serde_json::Value> {
        <Self as StatsProvider>::type_stats(self).await
    }

    async fn session_stats(&self) -> Result<serde_json::Value> {
        <Self as StatsProvider>::session_stats(self).await
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        <Self as StatsProvider>::weekly_digest(self, days).await
    }

    async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        <Self as StatsProvider>::access_rate_stats(self).await
    }

    async fn create_backup(&self) -> Result<BackupInfo> {
        <Self as BackupManager>::create_backup(self).await
    }

    async fn rotate_backups(&self, max_count: usize) -> Result<usize> {
        <Self as BackupManager>::rotate_backups(self, max_count).await
    }

    async fn list_backups(&self) -> Result<Vec<BackupInfo>> {
        <Self as BackupManager>::list_backups(self).await
    }

    async fn restore_backup(&self, backup_path: &std::path::Path) -> Result<()> {
        <Self as BackupManager>::restore_backup(self, backup_path).await
    }

    async fn maybe_startup_backup(&self) -> Result<Option<BackupInfo>> {
        <Self as BackupManager>::maybe_startup_backup(self).await
    }

    async fn welcome(
        &self,
        session_id: Option<&str>,
        project: Option<&str>,
    ) -> Result<serde_json::Value> {
        <Self as WelcomeProvider>::welcome(self, session_id, project).await
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

        // In-memory databases have a single connection; skip spawn_blocking
        // overhead and run synchronously to avoid mutex contention between
        // concurrent blocking tasks.
        if !pool.has_readers() {
            let conn = pool.reader()?;
            let candidates = collect_vec(
                &conn,
                &query_embedding,
                limit,
                include_superseded,
                &opts,
                &scoring_params,
            )?;
            return Ok(candidates);
        }

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

        Ok(candidates)
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

        // In-memory databases have a single connection; skip spawn_blocking
        // overhead and run synchronously to avoid mutex contention between
        // concurrent blocking tasks.
        if !pool.has_readers() {
            let conn = pool.reader()?;
            let candidates = collect_fts(
                &conn,
                &query,
                limit,
                &opts,
                include_superseded,
                &scoring_params,
            )?;
            return Ok(candidates);
        }

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

        Ok(candidates)
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
