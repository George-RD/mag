use anyhow::Result;
use async_trait::async_trait;
use uuid::Uuid;

mod domain;
mod traits;

pub use domain::*;
pub use traits::*;

pub mod embedder;
pub mod reranker;
pub mod scoring;
pub mod scoring_strategy;
pub mod storage;

#[cfg(feature = "real-embeddings")]
#[allow(unused_imports)]
pub use embedder::OnnxEmbedder;
#[allow(unused_imports)]
pub use embedder::{Embedder, PlaceholderEmbedder};
#[allow(unused_imports)]
pub use reranker::{NoOpReranker, Reranker};
#[allow(unused_imports)]
pub use scoring::{
    ABSTENTION_MIN_TEXT, GRAPH_MIN_EDGE_WEIGHT, GRAPH_NEIGHBOR_FACTOR, RRF_WEIGHT_FTS,
    RRF_WEIGHT_VEC, ScoringParams, feedback_factor, jaccard_pre, jaccard_similarity,
    priority_factor, time_decay_et, type_weight_et, word_overlap_pre,
};
#[allow(unused_imports)]
pub(crate) use scoring::{is_stopword, simple_stem, token_set};
#[allow(unused_imports)]
pub use scoring_strategy::{DefaultScoringStrategy, ScoringStrategy};

/// Orchestrates the memory pipeline by coordinating ingestors, processors, and storage.
pub struct Pipeline {
    ingestor: Box<dyn Ingestor>,
    processor: Box<dyn Processor>,
    storage: Box<dyn Storage>,
    retriever: Box<dyn Retriever>,
    searcher: Box<dyn Searcher>,
    recents: Box<dyn Recents>,
    semantic_searcher: Box<dyn SemanticSearcher>,
}

impl Pipeline {
    /// Creates a new Pipeline with the provided components.
    pub fn new(
        ingestor: Box<dyn Ingestor>,
        processor: Box<dyn Processor>,
        storage: Box<dyn Storage>,
        retriever: Box<dyn Retriever>,
        searcher: Box<dyn Searcher>,
        recents: Box<dyn Recents>,
        semantic_searcher: Box<dyn SemanticSearcher>,
    ) -> Self {
        Self {
            ingestor,
            processor,
            storage,
            retriever,
            searcher,
            recents,
            semantic_searcher,
        }
    }

    /// Runs the full pipeline: ingest -> process -> store.
    pub async fn run(&self, content: &str, input: &MemoryInput) -> Result<String> {
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let mut store_input = input.clone();
        if store_input.id.is_none() {
            store_input.id = Some(id.clone());
        }
        let content_to_ingest = if content.is_empty() {
            input.content.as_str()
        } else {
            content
        };
        let ingested = self.ingestor.ingest(content_to_ingest).await?;
        let processed = self.processor.process(&ingested).await?;
        self.storage.store(&id, &processed, &store_input).await?;
        Ok(id)
    }

    /// Retrieves data from storage via the retriever.
    pub async fn retrieve(&self, id: &str) -> Result<String> {
        self.retriever.retrieve(id).await
    }

    /// Searches for stored memories matching the provided query.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SearchResult>> {
        self.searcher.search(query, limit, opts).await
    }

    pub async fn recent(&self, limit: usize, opts: &SearchOptions) -> Result<Vec<SearchResult>> {
        self.recents.recent(limit, opts).await
    }

    pub async fn semantic_search(
        &self,
        query: &str,
        limit: usize,
        opts: &SearchOptions,
    ) -> Result<Vec<SemanticResult>> {
        self.semantic_searcher
            .semantic_search(query, limit, opts)
            .await
    }
}

/// A placeholder implementation of the memory pipeline for development and testing.
pub struct PlaceholderPipeline;

#[async_trait]
impl Ingestor for PlaceholderPipeline {
    async fn ingest(&self, content: &str) -> Result<String> {
        Ok(content.to_string())
    }
}

#[async_trait]
impl Processor for PlaceholderPipeline {
    async fn process(&self, input: &str) -> Result<String> {
        Ok(format!("processed: {}", input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use serde_json::json;

    struct MockPipeline;

    #[async_trait]
    impl Ingestor for MockPipeline {
        async fn ingest(&self, content: &str) -> Result<String> {
            Ok(content.to_string())
        }
    }

    #[async_trait]
    impl Processor for MockPipeline {
        async fn process(&self, input: &str) -> Result<String> {
            Ok(format!("processed: {}", input))
        }
    }

    #[async_trait]
    impl Storage for MockPipeline {
        async fn store(&self, _id: &str, _data: &str, _input: &MemoryInput) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Retriever for MockPipeline {
        async fn retrieve(&self, id: &str) -> Result<String> {
            Ok(format!("retrieved: {}", id))
        }
    }

    #[async_trait]
    impl Searcher for MockPipeline {
        async fn search(
            &self,
            query: &str,
            _limit: usize,
            _opts: &SearchOptions,
        ) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "result-1".to_string(),
                content: format!("match: {query}"),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
                entity_id: None,
                agent_type: None,
            }])
        }
    }

    #[async_trait]
    impl Recents for MockPipeline {
        async fn recent(&self, _limit: usize, _opts: &SearchOptions) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "recent-1".to_string(),
                content: "recent value".to_string(),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
                entity_id: None,
                agent_type: None,
            }])
        }
    }

    #[async_trait]
    impl SemanticSearcher for MockPipeline {
        async fn semantic_search(
            &self,
            query: &str,
            _limit: usize,
            _opts: &SearchOptions,
        ) -> Result<Vec<SemanticResult>> {
            Ok(vec![SemanticResult {
                id: "semantic-1".to_string(),
                content: format!("semantic match: {query}"),
                tags: Vec::new(),
                importance: 0.5,
                metadata: json!({}),
                event_type: None,
                session_id: None,
                project: None,
                entity_id: None,
                agent_type: None,
                score: 0.99,
            }])
        }
    }

    struct FailingIngestor;

    #[async_trait]
    impl Ingestor for FailingIngestor {
        async fn ingest(&self, _content: &str) -> Result<String> {
            Err(anyhow!("Ingestion failed"))
        }
    }

    #[tokio::test]
    async fn test_ingestor_trait() {
        let ingestor: Box<dyn Ingestor> = Box::new(MockPipeline);
        let result = ingestor.ingest("test").await.unwrap();
        assert_eq!(result, "test");
    }

    #[tokio::test]
    async fn test_pipeline_run_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            id: Some("custom_id".to_string()),
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "custom_id");
    }

    #[tokio::test]
    async fn test_pipeline_run_default_id() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_ok());
        let id = result.unwrap();
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[tokio::test]
    async fn test_pipeline_retrieve_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let result = pipeline.retrieve("test_id").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "retrieved: test_id");
    }

    #[tokio::test]
    async fn test_pipeline_failure() {
        let pipeline = Pipeline::new(
            Box::new(FailingIngestor),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let input = MemoryInput {
            content: "hello".to_string(),
            importance: 0.5,
            metadata: json!({}),
            ..Default::default()
        };
        let result = pipeline.run("hello", &input).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Ingestion failed");
    }

    #[tokio::test]
    async fn test_pipeline_search_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline
            .search("needle", 5, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "result-1");
        assert_eq!(results[0].content, "match: needle");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
    }

    #[tokio::test]
    async fn test_pipeline_recent_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline.recent(3, &SearchOptions::default()).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "recent-1");
        assert_eq!(results[0].content, "recent value");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
    }

    #[tokio::test]
    async fn test_pipeline_semantic_search_success() {
        let pipeline = Pipeline::new(
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
            Box::new(MockPipeline),
        );

        let results = pipeline
            .semantic_search("vector", 4, &SearchOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "semantic-1");
        assert_eq!(results[0].content, "semantic match: vector");
        assert!(results[0].tags.is_empty());
        assert_eq!(results[0].importance, 0.5);
        assert_eq!(results[0].metadata, json!({}));
        assert!(results[0].score > 0.9);
    }

    #[test]
    fn test_memory_kind_for_semantic_event_type() {
        assert_eq!(EventType::Decision.memory_kind(), MemoryKind::Semantic);
    }

    #[test]
    fn test_memory_kind_defaults_to_episodic_for_unknown_type() {
        assert_eq!(
            EventType::Unknown("totally_unknown".to_string()).memory_kind(),
            MemoryKind::Episodic
        );
    }
}
