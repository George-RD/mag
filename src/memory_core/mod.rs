use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

pub mod storage;

/// Search result item returned by memory queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    /// Memory identifier.
    pub id: String,
    /// Stored memory content.
    pub content: String,
}

/// Semantic search result item with similarity score.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticResult {
    /// Memory identifier.
    pub id: String,
    /// Stored memory content.
    pub content: String,
    /// Similarity score in the range [0.0, 1.0].
    pub score: f32,
}

/// A directed relationship between two memories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// Relationship identifier.
    pub id: String,
    /// Source memory identifier.
    pub source_id: String,
    /// Target memory identifier.
    pub target_id: String,
    /// Relationship type label (e.g. "links_to", "related").
    pub rel_type: String,
}

/// Result of a paginated list query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListResult {
    /// Memories in the current page.
    pub memories: Vec<SearchResult>,
    /// Total number of memories in the store.
    pub total: usize,
}

/// Trait for ingesting raw content into the memory system.
#[async_trait]
pub trait Ingestor: Send + Sync {
    /// Ingests the provided content and returns a processed string.
    async fn ingest(&self, content: &str) -> Result<String>;
}

/// Trait for processing ingested content (e.g., summarization, embedding).
#[async_trait]
pub trait Processor: Send + Sync {
    /// Processes the input string and returns a refined result.
    async fn process(&self, input: &str) -> Result<String>;
}

/// Trait for storing processed memory data.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Stores the data under the given ID with optional tags.
    async fn store(&self, id: &str, data: &str, tags: &[String]) -> Result<()>;
}

/// Trait for retrieving stored memory data.
#[async_trait]
pub trait Retriever: Send + Sync {
    /// Retrieves the data associated with the given ID.
    async fn retrieve(&self, id: &str) -> Result<String>;
}

/// Trait for searching stored memory data.
#[async_trait]
pub trait Searcher: Send + Sync {
    /// Searches for memories matching the query string.
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>>;
}

#[async_trait]
pub trait Recents: Send + Sync {
    async fn recent(&self, limit: usize) -> Result<Vec<SearchResult>>;
}

#[async_trait]
pub trait SemanticSearcher: Send + Sync {
    async fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SemanticResult>>;
}

/// Trait for deleting stored memories.
#[async_trait]
pub trait Deleter: Send + Sync {
    /// Deletes the memory with the given ID. Returns `true` if a row was removed.
    async fn delete(&self, id: &str) -> Result<bool>;
}

/// Trait for updating stored memory content and tags.
#[async_trait]
pub trait Updater: Send + Sync {
    /// Updates an existing memory. At least one of content or tags must be provided.
    async fn update(&self, id: &str, content: Option<&str>, tags: Option<&[String]>) -> Result<()>;
}

/// Trait for querying memories by tag.
#[async_trait]
pub trait Tagger: Send + Sync {
    /// Returns memories whose tags contain **all** of the supplied tags.
    async fn get_by_tags(&self, tags: &[String], limit: usize) -> Result<Vec<SearchResult>>;
}

/// Trait for paginated listing of memories.
#[async_trait]
pub trait Lister: Send + Sync {
    /// Lists memories with pagination, returning the page and total count.
    async fn list(&self, offset: usize, limit: usize) -> Result<ListResult>;
}

/// Trait for querying relationships of a memory.
#[async_trait]
pub trait RelationshipQuerier: Send + Sync {
    /// Returns all relationships where `memory_id` is either source or target.
    async fn get_relationships(&self, memory_id: &str) -> Result<Vec<Relationship>>;
}

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
    pub async fn run(&self, content: &str, id: Option<&str>, tags: &[String]) -> Result<String> {
        let id = id
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let ingested = self.ingestor.ingest(content).await?;
        let processed = self.processor.process(&ingested).await?;
        self.storage.store(&id, &processed, tags).await?;
        Ok(id)
    }

    /// Retrieves data from storage via the retriever.
    pub async fn retrieve(&self, id: &str) -> Result<String> {
        self.retriever.retrieve(id).await
    }

    /// Searches for stored memories matching the provided query.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
        self.searcher.search(query, limit).await
    }

    pub async fn recent(&self, limit: usize) -> Result<Vec<SearchResult>> {
        self.recents.recent(limit).await
    }

    pub async fn semantic_search(&self, query: &str, limit: usize) -> Result<Vec<SemanticResult>> {
        self.semantic_searcher.semantic_search(query, limit).await
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

#[async_trait]
impl Storage for PlaceholderPipeline {
    async fn store(&self, id: &str, data: &str, _tags: &[String]) -> Result<()> {
        info!(memory_id = %id, content_len = data.len(), "Storing placeholder payload");
        Ok(())
    }
}

#[async_trait]
impl Retriever for PlaceholderPipeline {
    async fn retrieve(&self, id: &str) -> Result<String> {
        Ok(format!("retrieved: {}", id))
    }
}

#[async_trait]
impl Searcher for PlaceholderPipeline {
    async fn search(&self, query: &str, _limit: usize) -> Result<Vec<SearchResult>> {
        Ok(vec![SearchResult {
            id: "placeholder".to_string(),
            content: format!("search result for: {query}"),
        }])
    }
}

#[async_trait]
impl Recents for PlaceholderPipeline {
    async fn recent(&self, _limit: usize) -> Result<Vec<SearchResult>> {
        Ok(vec![SearchResult {
            id: "placeholder-recent".to_string(),
            content: "recent result".to_string(),
        }])
    }
}

#[async_trait]
impl SemanticSearcher for PlaceholderPipeline {
    async fn semantic_search(&self, query: &str, _limit: usize) -> Result<Vec<SemanticResult>> {
        Ok(vec![SemanticResult {
            id: "placeholder-semantic".to_string(),
            content: format!("semantic result for: {query}"),
            score: 1.0,
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

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
        async fn store(&self, _id: &str, _data: &str, _tags: &[String]) -> Result<()> {
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
        async fn search(&self, query: &str, _limit: usize) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "result-1".to_string(),
                content: format!("match: {query}"),
            }])
        }
    }

    #[async_trait]
    impl Recents for MockPipeline {
        async fn recent(&self, _limit: usize) -> Result<Vec<SearchResult>> {
            Ok(vec![SearchResult {
                id: "recent-1".to_string(),
                content: "recent value".to_string(),
            }])
        }
    }

    #[async_trait]
    impl SemanticSearcher for MockPipeline {
        async fn semantic_search(&self, query: &str, _limit: usize) -> Result<Vec<SemanticResult>> {
            Ok(vec![SemanticResult {
                id: "semantic-1".to_string(),
                content: format!("semantic match: {query}"),
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

        let result = pipeline.run("hello", Some("custom_id"), &[]).await;
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

        let result = pipeline.run("hello", None, &[]).await;
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

        let result = pipeline.run("hello", None, &[]).await;
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

        let results = pipeline.search("needle", 5).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "result-1");
        assert_eq!(results[0].content, "match: needle");
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

        let results = pipeline.recent(3).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "recent-1");
        assert_eq!(results[0].content, "recent value");
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

        let results = pipeline.semantic_search("vector", 4).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "semantic-1");
        assert_eq!(results[0].content, "semantic match: vector");
        assert!(results[0].score > 0.9);
    }
}
