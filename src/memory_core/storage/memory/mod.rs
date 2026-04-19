//! In-memory HashMap-backed storage backend.
//!
//! A minimal, testing-oriented storage implementation that stores memories
//! in a `HashMap` and embeddings in a parallel `Vec`. Useful for:
//! - Fast unit tests without SQLite overhead
//! - Conformance testing of the storage trait surface
//! - Reference implementation proving the trait substrate works
//!
//! Not intended for production use. Uses brute-force cosine similarity for
//! vector search and linear scans for text/tag queries.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::memory_core::{
    AdvancedSearcher, Deleter, EventType, ListResult, Lister, MemoryInput, MemoryUpdate,
    PhraseSearcher, Recents, Retriever, SearchOptions, SearchResult, Searcher, SemanticResult,
    SemanticSearcher, StatsProvider, Storage, Tagger, Updater, embedder::Embedder,
    scoring_strategy::ScoringStrategy,
};

// ── Internal stored entry ──────────────────────────────────────────────

/// Internal representation of a stored memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredMemory {
    id: String,
    content: String,
    tags: Vec<String>,
    importance: f64,
    metadata: serde_json::Value,
    event_type: Option<EventType>,
    session_id: Option<String>,
    project: Option<String>,
    priority: Option<i32>,
    entity_id: Option<String>,
    agent_type: Option<String>,
    embedding: Option<Vec<f32>>,
    created_at: String,
    last_accessed_at: String,
    access_count: u64,
}

impl StoredMemory {
    fn to_search_result(&self) -> SearchResult {
        SearchResult {
            id: self.id.clone(),
            content: self.content.clone(),
            tags: self.tags.clone(),
            importance: self.importance,
            metadata: self.metadata.clone(),
            event_type: self.event_type.clone(),
            session_id: self.session_id.clone(),
            project: self.project.clone(),
            entity_id: self.entity_id.clone(),
            agent_type: self.agent_type.clone(),
        }
    }

    fn to_semantic_result(&self, score: f32) -> SemanticResult {
        SemanticResult {
            id: self.id.clone(),
            content: self.content.clone(),
            tags: self.tags.clone(),
            importance: self.importance,
            metadata: self.metadata.clone(),
            event_type: self.event_type.clone(),
            session_id: self.session_id.clone(),
            project: self.project.clone(),
            entity_id: self.entity_id.clone(),
            agent_type: self.agent_type.clone(),
            score,
        }
    }

    /// Check if this memory matches the given search options filters.
    fn matches_options(&self, opts: &SearchOptions) -> bool {
        if let Some(ref event_type) = opts.event_type
            && self.event_type.as_ref() != Some(event_type)
        {
            return false;
        }
        if let Some(ref project) = opts.project
            && self.project.as_deref() != Some(project.as_str())
        {
            return false;
        }
        if let Some(ref session_id) = opts.session_id
            && self.session_id.as_deref() != Some(session_id.as_str())
        {
            return false;
        }
        if let Some(importance_min) = opts.importance_min
            && self.importance < importance_min
        {
            return false;
        }
        if let Some(ref created_after) = opts.created_after
            && self.created_at.as_str() < created_after.as_str()
        {
            return false;
        }
        if let Some(ref created_before) = opts.created_before
            && self.created_at.as_str() > created_before.as_str()
        {
            return false;
        }
        if let Some(ref entity_id) = opts.entity_id
            && self.entity_id.as_deref() != Some(entity_id.as_str())
        {
            return false;
        }
        if let Some(ref agent_type) = opts.agent_type
            && self.agent_type.as_deref() != Some(agent_type.as_str())
        {
            return false;
        }
        true
    }
}

/// Relationship between two memories.
#[derive(Debug, Clone)]
struct StoredRelationship {
    id: String,
    source_id: String,
    target_id: String,
    rel_type: String,
    weight: f64,
    metadata: serde_json::Value,
    created_at: String,
}

// ── MemoryStorage ──────────────────────────────────────────────────────

/// In-memory HashMap-backed storage backend.
///
/// Stores memories in a `HashMap<String, StoredMemory>` behind a `RwLock`
/// for async trait compatibility. Uses brute-force cosine similarity for
/// vector search.
pub struct MemoryStorage {
    memories: Arc<RwLock<HashMap<String, StoredMemory>>>,
    relationships: Arc<RwLock<Vec<StoredRelationship>>>,
    embedder: Arc<dyn Embedder>,
    #[allow(dead_code)]
    scoring_strategy: Arc<dyn ScoringStrategy>,
}

impl MemoryStorage {
    /// Creates a new empty `MemoryStorage` with the given embedder and
    /// the default scoring strategy.
    #[allow(dead_code)]
    pub fn new(embedder: Arc<dyn Embedder>, scoring_strategy: Arc<dyn ScoringStrategy>) -> Self {
        Self {
            memories: Arc::new(RwLock::new(HashMap::new())),
            relationships: Arc::new(RwLock::new(Vec::new())),
            embedder,
            scoring_strategy,
        }
    }

    /// Returns the current number of stored memories.
    #[allow(dead_code)]
    pub async fn len(&self) -> usize {
        self.memories.read().await.len()
    }

    /// Returns `true` if the storage is empty.
    #[allow(dead_code)]
    pub async fn is_empty(&self) -> bool {
        self.memories.read().await.is_empty()
    }

    /// Exports all memories as a JSON string.
    #[allow(dead_code)]
    pub async fn export_all(&self) -> Result<String> {
        let memories = self.memories.read().await;
        let relationships = self.relationships.read().await;

        let mem_values: Vec<&StoredMemory> = memories.values().collect();
        let rel_values: Vec<serde_json::Value> = relationships
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "source_id": r.source_id,
                    "target_id": r.target_id,
                    "rel_type": r.rel_type,
                    "weight": r.weight,
                    "metadata": r.metadata,
                    "created_at": r.created_at,
                })
            })
            .collect();

        let export = serde_json::json!({
            "version": 1,
            "memories": mem_values,
            "relationships": rel_values,
        });

        serde_json::to_string_pretty(&export).map_err(|e| anyhow!("failed to serialize: {e}"))
    }

    /// Imports memories from a JSON string. Returns the count of imported memories.
    #[allow(dead_code)]
    pub async fn import_all(&self, data: &str) -> Result<usize> {
        let parsed: serde_json::Value =
            serde_json::from_str(data).map_err(|e| anyhow!("failed to parse import JSON: {e}"))?;

        let raw_memories = parsed["memories"]
            .as_array()
            .ok_or_else(|| anyhow!("import JSON missing 'memories' array"))?;

        let mut memories = self.memories.write().await;
        let mut count = 0;

        for mem in raw_memories {
            let stored: StoredMemory = serde_json::from_value(mem.clone())
                .map_err(|e| anyhow!("failed to deserialize memory: {e}"))?;
            memories.insert(stored.id.clone(), stored);
            count += 1;
        }

        Ok(count)
    }
}

// ── Cosine similarity ──────────────────────────────────────────────────

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

// ── Trait implementations ──────────────────────────────────────────────

#[async_trait]
impl Storage for MemoryStorage {
    async fn store(&self, id: &str, data: &str, input: &MemoryInput) -> Result<()> {
        let embedding = self.embedder.embed(data).ok();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

        let stored = StoredMemory {
            id: id.to_string(),
            content: data.to_string(),
            tags: input.tags.clone(),
            importance: input.importance,
            metadata: input.metadata.clone(),
            event_type: input.event_type.clone(),
            session_id: input.session_id.clone(),
            project: input.project.clone(),
            priority: input.priority,
            entity_id: input.entity_id.clone(),
            agent_type: input.agent_type.clone(),
            embedding,
            created_at: now.clone(),
            last_accessed_at: now,
            access_count: 0,
        };

        let mut memories = self.memories.write().await;
        memories.insert(id.to_string(), stored);
        Ok(())
    }
}

#[async_trait]
impl Retriever for MemoryStorage {
    async fn retrieve(&self, id: &str) -> Result<String> {
        let mut memories = self.memories.write().await;
        let entry = memories
            .get_mut(id)
            .ok_or_else(|| anyhow!("memory not found for id={id}"))?;

        entry.access_count += 1;
        entry.last_accessed_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

        Ok(entry.content.clone())
    }
}

#[async_trait]
impl Deleter for MemoryStorage {
    async fn delete(&self, id: &str) -> Result<bool> {
        let mut memories = self.memories.write().await;
        let removed = memories.remove(id).is_some();

        if removed {
            // Also remove any relationships involving this memory.
            let mut relationships = self.relationships.write().await;
            relationships.retain(|r| r.source_id != id && r.target_id != id);
        }

        Ok(removed)
    }
}

#[async_trait]
impl Updater for MemoryStorage {
    async fn update(&self, id: &str, input: &MemoryUpdate) -> Result<()> {
        if input.content.is_none()
            && input.tags.is_none()
            && input.importance.is_none()
            && input.metadata.is_none()
            && input.event_type.is_none()
            && input.priority.is_none()
        {
            return Err(anyhow!(
                "at least one of content, tags, importance, metadata, event_type, or priority must be provided"
            ));
        }

        let mut memories = self.memories.write().await;
        let entry = memories
            .get_mut(id)
            .ok_or_else(|| anyhow!("memory not found for id={id}"))?;

        if let Some(ref content) = input.content {
            entry.content = content.clone();
            // Re-embed on content change.
            entry.embedding = self.embedder.embed(content).ok();
        }
        if let Some(ref tags) = input.tags {
            entry.tags = tags.clone();
        }
        if let Some(importance) = input.importance {
            entry.importance = importance;
        }
        if let Some(ref metadata) = input.metadata {
            entry.metadata = metadata.clone();
        }
        if let Some(ref event_type) = input.event_type {
            entry.event_type = Some(event_type.clone());
        }
        if let Some(priority) = input.priority {
            entry.priority = Some(priority);
        }

        entry.last_accessed_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        Ok(())
    }
}

#[async_trait]
impl Searcher for MemoryStorage {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let memories = self.memories.read().await;
        let query_lower = query.to_lowercase();

        let mut results: Vec<SearchResult> = memories
            .values()
            .filter(|m| m.matches_options(opts))
            .filter(|m| m.content.to_lowercase().contains(&query_lower))
            .map(|m| m.to_search_result())
            .collect();

        // Sort by most recently accessed, matching SQLite LIKE fallback behavior.
        results.sort_by(|a, b| b.id.cmp(&a.id));
        results.truncate(limit);
        Ok(results)
    }
}

#[async_trait]
impl Recents for MemoryStorage {
    async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let memories = self.memories.read().await;
        let mut entries: Vec<&StoredMemory> = memories
            .values()
            .filter(|m| m.matches_options(opts))
            .collect();

        // Sort by last_accessed_at descending.
        entries.sort_by(|a, b| b.last_accessed_at.cmp(&a.last_accessed_at));
        entries.truncate(limit);

        Ok(entries.iter().map(|m| m.to_search_result()).collect())
    }
}

#[async_trait]
impl SemanticSearcher for MemoryStorage {
    async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let query_embedding = self.embedder.embed(query)?;
        let memories = self.memories.read().await;

        let mut scored: Vec<(f32, SemanticResult)> = memories
            .values()
            .filter(|m| m.matches_options(opts))
            .filter_map(|m| {
                let emb = m.embedding.as_ref()?;
                let sim = cosine_similarity(&query_embedding, emb);
                Some((sim, m.to_semantic_result(sim)))
            })
            .collect();

        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored.truncate(limit);

        Ok(scored.into_iter().map(|(_, r)| r).collect())
    }
}

#[async_trait]
impl Tagger for MemoryStorage {
    async fn get_by_tags(
        &self,
        tags: &[String],
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        if tags.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }

        let memories = self.memories.read().await;

        let mut results: Vec<SearchResult> = memories
            .values()
            .filter(|m| m.matches_options(opts))
            .filter(|m| {
                // All supplied tags must be present in the memory's tags.
                tags.iter().all(|t| m.tags.contains(t))
            })
            .map(|m| m.to_search_result())
            .collect();

        results.sort_by(|a, b| b.id.cmp(&a.id));
        results.truncate(limit);
        Ok(results)
    }
}

#[async_trait]
impl Lister for MemoryStorage {
    async fn list(&self, offset: usize, limit: usize, opts: &SearchOptions) -> Result<ListResult> {
        let memories = self.memories.read().await;

        let filtered: Vec<SearchResult> = memories
            .values()
            .filter(|m| m.matches_options(opts))
            .map(|m| m.to_search_result())
            .collect();

        let total = filtered.len();

        if limit == 0 {
            return Ok(ListResult {
                memories: Vec::new(),
                total,
            });
        }

        let page: Vec<SearchResult> = filtered.into_iter().skip(offset).take(limit).collect();

        Ok(ListResult {
            memories: page,
            total,
        })
    }
}

#[async_trait]
impl StatsProvider for MemoryStorage {
    async fn type_stats(&self) -> Result<serde_json::Value> {
        let memories = self.memories.read().await;

        let mut counts: HashMap<String, i64> = HashMap::new();
        for m in memories.values() {
            let key = m
                .event_type
                .as_ref()
                .map(|et| et.to_string())
                .unwrap_or_else(|| "untyped".to_string());
            *counts.entry(key).or_insert(0) += 1;
        }

        let total: i64 = counts.values().sum();
        let mut result = serde_json::Map::new();
        for (k, v) in &counts {
            result.insert(k.clone(), serde_json::json!(v));
        }
        result.insert("_total".to_string(), serde_json::json!(total));

        Ok(serde_json::Value::Object(result))
    }

    async fn session_stats(&self) -> Result<serde_json::Value> {
        let memories = self.memories.read().await;

        let mut session_counts: HashMap<String, i64> = HashMap::new();
        for m in memories.values() {
            if let Some(ref sid) = m.session_id {
                *session_counts.entry(sid.clone()).or_insert(0) += 1;
            }
        }

        let total_sessions = session_counts.len() as i64;

        // Sort by count descending, take top 20.
        let mut sorted: Vec<_> = session_counts.into_iter().collect();
        sorted.sort_by_key(|b| std::cmp::Reverse(b.1));
        sorted.truncate(20);

        let sessions: Vec<serde_json::Value> = sorted
            .into_iter()
            .map(|(sid, cnt)| {
                serde_json::json!({
                    "session_id": sid,
                    "count": cnt,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "sessions": sessions,
            "total_sessions": total_sessions,
        }))
    }

    async fn weekly_digest(&self, days: i64) -> Result<serde_json::Value> {
        if days <= 0 {
            anyhow::bail!("days must be > 0");
        }

        let memories = self.memories.read().await;
        let total = memories.len() as i64;

        // Simple stub: count all as "period_new" since we don't track historical data deeply.
        Ok(serde_json::json!({
            "period_days": days,
            "total_memories": total,
            "period_new": total,
            "session_count": 0,
            "type_breakdown": {},
            "growth_pct": 0.0,
            "prev_period_count": 0,
        }))
    }

    async fn access_rate_stats(&self) -> Result<serde_json::Value> {
        let memories = self.memories.read().await;

        let total = memories.len() as i64;
        let zero_access = memories.values().filter(|m| m.access_count == 0).count() as i64;
        #[allow(clippy::cast_precision_loss)]
        let avg_access = if total > 0 {
            memories.values().map(|m| m.access_count).sum::<u64>() as f64 / total as f64
        } else {
            0.0
        };
        #[allow(clippy::cast_precision_loss)]
        let never_pct = if total > 0 {
            (zero_access as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        Ok(serde_json::json!({
            "total_memories": total,
            "zero_access_count": zero_access,
            "never_accessed_pct": (never_pct * 100.0).round() / 100.0,
            "avg_access_count": (avg_access * 100.0).round() / 100.0,
            "by_type": [],
            "top_accessed": [],
        }))
    }
}

#[async_trait]
impl AdvancedSearcher for MemoryStorage {
    async fn advanced_search(
        &self,
        _query: &str,
        _limit: usize,
        _opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        unimplemented!("AdvancedSearcher requires SQLite-specific features (FTS5 + RRF pipeline)")
    }
}

#[async_trait]
impl PhraseSearcher for MemoryStorage {
    async fn phrase_search(
        &self,
        _phrase: &str,
        _limit: usize,
        _opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        unimplemented!("PhraseSearcher requires SQLite-specific features (FTS5)")
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_core::PlaceholderEmbedder;
    use crate::memory_core::scoring_strategy::DefaultScoringStrategy;

    fn make_storage() -> MemoryStorage {
        MemoryStorage::new(
            Arc::new(PlaceholderEmbedder),
            Arc::new(DefaultScoringStrategy::new()),
        )
    }

    fn make_input(content: &str) -> MemoryInput {
        MemoryInput {
            content: content.to_string(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        }
    }

    fn make_input_with_tags(content: &str, tags: Vec<&str>) -> MemoryInput {
        MemoryInput {
            content: content.to_string(),
            tags: tags.into_iter().map(String::from).collect(),
            importance: 0.5,
            metadata: serde_json::json!({}),
            ..Default::default()
        }
    }

    // ── Store + Retrieve round-trip ────────────────────────────────────

    #[tokio::test]
    async fn test_store_and_retrieve() {
        let storage = make_storage();
        let input = make_input("hello world");

        Storage::store(&storage, "id-1", "hello world", &input)
            .await
            .unwrap();

        let content = storage.retrieve("id-1").await.unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_retrieve_not_found() {
        let storage = make_storage();
        let result = storage.retrieve("nonexistent").await;
        assert!(result.is_err());
    }

    // ── Delete ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_delete_existing() {
        let storage = make_storage();
        let input = make_input("to be deleted");

        Storage::store(&storage, "id-del", "to be deleted", &input)
            .await
            .unwrap();

        let deleted = storage.delete("id-del").await.unwrap();
        assert!(deleted);

        let result = storage.retrieve("id-del").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let storage = make_storage();
        let deleted = storage.delete("nope").await.unwrap();
        assert!(!deleted);
    }

    // ── Update ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_content() {
        let storage = make_storage();
        let input = make_input("original");

        Storage::store(&storage, "id-upd", "original", &input)
            .await
            .unwrap();

        let update = MemoryUpdate {
            content: Some("updated".to_string()),
            ..Default::default()
        };
        storage.update("id-upd", &update).await.unwrap();

        let content = storage.retrieve("id-upd").await.unwrap();
        assert_eq!(content, "updated");
    }

    #[tokio::test]
    async fn test_update_tags() {
        let storage = make_storage();
        let input = make_input("tagged");

        Storage::store(&storage, "id-tag-upd", "tagged", &input)
            .await
            .unwrap();

        let update = MemoryUpdate {
            tags: Some(vec!["new-tag".to_string()]),
            ..Default::default()
        };
        storage.update("id-tag-upd", &update).await.unwrap();

        let results = storage
            .get_by_tags(&["new-tag".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "id-tag-upd");
    }

    #[tokio::test]
    async fn test_update_not_found() {
        let storage = make_storage();
        let update = MemoryUpdate {
            content: Some("nope".to_string()),
            ..Default::default()
        };
        let result = storage.update("nonexistent", &update).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_update_empty_rejected() {
        let storage = make_storage();
        let input = make_input("data");

        Storage::store(&storage, "id-empty-upd", "data", &input)
            .await
            .unwrap();

        let update = MemoryUpdate::default();
        let result = storage.update("id-empty-upd", &update).await;
        assert!(result.is_err());
    }

    // ── Search ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_search_text_match() {
        let storage = make_storage();

        Storage::store(
            &storage,
            "s1",
            "the quick brown fox",
            &make_input("the quick brown fox"),
        )
        .await
        .unwrap();
        Storage::store(&storage, "s2", "lazy dog", &make_input("lazy dog"))
            .await
            .unwrap();

        let results = storage
            .search("quick", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "s1");
    }

    #[tokio::test]
    async fn test_search_no_match() {
        let storage = make_storage();
        Storage::store(&storage, "s1", "hello world", &make_input("hello world"))
            .await
            .unwrap();

        let results = storage
            .search("nonexistent", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // ── Tag search ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_tag_search() {
        let storage = make_storage();

        let input1 = make_input_with_tags("rust code", vec!["rust", "code"]);
        let input2 = make_input_with_tags("python code", vec!["python", "code"]);

        Storage::store(&storage, "t1", "rust code", &input1)
            .await
            .unwrap();
        Storage::store(&storage, "t2", "python code", &input2)
            .await
            .unwrap();

        // Search for "code" tag — both should match.
        let results = storage
            .get_by_tags(&["code".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 2);

        // Search for "rust" tag — only t1.
        let results = storage
            .get_by_tags(&["rust".to_string()], 10, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");

        // Search for both "rust" AND "code" tags — only t1.
        let results = storage
            .get_by_tags(
                &["rust".to_string(), "code".to_string()],
                10,
                &SearchOptions::default(),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");
    }

    // ── List with pagination ───────────────────────────────────────────

    #[tokio::test]
    async fn test_list_with_pagination() {
        let storage = make_storage();

        for i in 0..5 {
            let id = format!("list-{i}");
            let content = format!("content {i}");
            Storage::store(&storage, &id, &content, &make_input(&content))
                .await
                .unwrap();
        }

        let result = storage.list(0, 3, &SearchOptions::default()).await.unwrap();
        assert_eq!(result.total, 5);
        assert_eq!(result.memories.len(), 3);

        let result = storage.list(3, 3, &SearchOptions::default()).await.unwrap();
        assert_eq!(result.total, 5);
        assert_eq!(result.memories.len(), 2);
    }

    #[tokio::test]
    async fn test_list_zero_limit_returns_total() {
        let storage = make_storage();

        for i in 0..3 {
            let id = format!("lz-{i}");
            let content = format!("content {i}");
            Storage::store(&storage, &id, &content, &make_input(&content))
                .await
                .unwrap();
        }

        let result = storage.list(0, 0, &SearchOptions::default()).await.unwrap();
        assert_eq!(result.total, 3);
        assert!(result.memories.is_empty());
    }

    // ── Stats ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_type_stats() {
        let storage = make_storage();

        let mut input1 = make_input("decision content");
        input1.event_type = Some(EventType::Decision);
        let mut input2 = make_input("another decision");
        input2.event_type = Some(EventType::Decision);
        let input3 = make_input("no type");

        Storage::store(&storage, "ts1", "decision content", &input1)
            .await
            .unwrap();
        Storage::store(&storage, "ts2", "another decision", &input2)
            .await
            .unwrap();
        Storage::store(&storage, "ts3", "no type", &input3)
            .await
            .unwrap();

        let stats = storage.type_stats().await.unwrap();
        assert_eq!(stats["_total"], 3);
        assert_eq!(stats["decision"], 2);
        assert_eq!(stats["untyped"], 1);
    }

    #[tokio::test]
    async fn test_session_stats() {
        let storage = make_storage();

        let mut input1 = make_input("s1 mem");
        input1.session_id = Some("sess-a".to_string());
        let mut input2 = make_input("s2 mem");
        input2.session_id = Some("sess-a".to_string());
        let mut input3 = make_input("s3 mem");
        input3.session_id = Some("sess-b".to_string());

        Storage::store(&storage, "ss1", "s1 mem", &input1)
            .await
            .unwrap();
        Storage::store(&storage, "ss2", "s2 mem", &input2)
            .await
            .unwrap();
        Storage::store(&storage, "ss3", "s3 mem", &input3)
            .await
            .unwrap();

        let stats = storage.session_stats().await.unwrap();
        assert_eq!(stats["total_sessions"], 2);
    }

    // ── Semantic search ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_semantic_search_returns_results() {
        let storage = make_storage();

        Storage::store(
            &storage,
            "sem1",
            "artificial intelligence",
            &make_input("artificial intelligence"),
        )
        .await
        .unwrap();
        Storage::store(
            &storage,
            "sem2",
            "cooking recipes",
            &make_input("cooking recipes"),
        )
        .await
        .unwrap();

        let results = storage
            .semantic_search("AI and machine learning", 10, &SearchOptions::default())
            .await
            .unwrap();
        assert!(!results.is_empty());
        // Results should be sorted by similarity score descending.
        if results.len() > 1 {
            assert!(results[0].score >= results[1].score);
        }
    }

    // ── Recent ─────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_recent_ordering() {
        let storage = make_storage();

        Storage::store(&storage, "r1", "first", &make_input("first"))
            .await
            .unwrap();

        // Force r1 to have an older timestamp so r2 is definitively more recent.
        {
            let mut memories = storage.memories.write().await;
            if let Some(entry) = memories.get_mut("r1") {
                entry.last_accessed_at = "2020-01-01T00:00:00.000Z".to_string();
            }
        }

        Storage::store(&storage, "r2", "second", &make_input("second"))
            .await
            .unwrap();

        let results = storage.recent(10, &SearchOptions::default()).await.unwrap();
        assert_eq!(results.len(), 2);
        // r2 should be first since r1 has an older timestamp.
        assert_eq!(results[0].id, "r2");

        // Now retrieve r1 to bump its access time.
        storage.retrieve("r1").await.unwrap();

        let results = storage.recent(10, &SearchOptions::default()).await.unwrap();
        assert_eq!(results.len(), 2);
        // r1 should now be first since its last_accessed_at was just updated.
        assert_eq!(results[0].id, "r1");
    }

    // ── Access count tracking ──────────────────────────────────────────

    #[tokio::test]
    async fn test_access_count_increments() {
        let storage = make_storage();

        Storage::store(&storage, "ac1", "content", &make_input("content"))
            .await
            .unwrap();

        // Retrieve multiple times.
        storage.retrieve("ac1").await.unwrap();
        storage.retrieve("ac1").await.unwrap();
        storage.retrieve("ac1").await.unwrap();

        let stats = storage.access_rate_stats().await.unwrap();
        assert_eq!(stats["total_memories"], 1);
        assert_eq!(stats["zero_access_count"], 0);
    }

    // ── Export / Import round-trip ──────────────────────────────────────

    #[tokio::test]
    async fn test_export_import_roundtrip() {
        let storage = make_storage();

        let input = make_input_with_tags("important thing", vec!["tag1"]);
        Storage::store(&storage, "exp1", "important thing", &input)
            .await
            .unwrap();

        let exported = storage.export_all().await.unwrap();

        // Import into fresh storage.
        let storage2 = make_storage();
        let count = storage2.import_all(&exported).await.unwrap();
        assert_eq!(count, 1);

        let content = storage2.retrieve("exp1").await.unwrap();
        assert_eq!(content, "important thing");
    }

    // ── Search with options filter ─────────────────────────────────────

    #[tokio::test]
    async fn test_search_with_project_filter() {
        let storage = make_storage();

        let mut input1 = make_input("project-a item");
        input1.project = Some("project-a".to_string());
        let mut input2 = make_input("project-b item");
        input2.project = Some("project-b".to_string());

        Storage::store(&storage, "pf1", "project-a item", &input1)
            .await
            .unwrap();
        Storage::store(&storage, "pf2", "project-b item", &input2)
            .await
            .unwrap();

        let opts = SearchOptions {
            project: Some("project-a".to_string()),
            ..Default::default()
        };
        let results = storage.search("item", 10, &opts).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "pf1");
    }

    // ── AdvancedSearcher / PhraseSearcher stubs ────────────────────────

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn test_advanced_search_unimplemented() {
        let storage = make_storage();
        let _ = storage
            .advanced_search("query", 10, &SearchOptions::default())
            .await;
    }

    #[tokio::test]
    #[should_panic(expected = "not implemented")]
    async fn test_phrase_search_unimplemented() {
        let storage = make_storage();
        let _ = storage
            .phrase_search("phrase", 10, &SearchOptions::default())
            .await;
    }
}
