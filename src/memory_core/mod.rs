use anyhow::Result;
use async_trait::async_trait;
use tracing::info;
use uuid::Uuid;

pub mod storage;

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
    /// Stores the data under the given ID.
    async fn store(&self, id: &str, data: &str) -> Result<()>;
}

/// Trait for retrieving stored memory data.
#[async_trait]
pub trait Retriever: Send + Sync {
    /// Retrieves the data associated with the given ID.
    async fn retrieve(&self, id: &str) -> Result<String>;
}

/// Orchestrates the memory pipeline by coordinating ingestors, processors, and storage.
pub struct Pipeline {
    ingestor: Box<dyn Ingestor>,
    processor: Box<dyn Processor>,
    storage: Box<dyn Storage>,
    retriever: Box<dyn Retriever>,
}

impl Pipeline {
    /// Creates a new Pipeline with the provided components.
    pub fn new(
        ingestor: Box<dyn Ingestor>,
        processor: Box<dyn Processor>,
        storage: Box<dyn Storage>,
        retriever: Box<dyn Retriever>,
    ) -> Self {
        Self {
            ingestor,
            processor,
            storage,
            retriever,
        }
    }

    /// Runs the full pipeline: ingest -> process -> store.
    pub async fn run(&self, content: &str, id: Option<&str>) -> Result<String> {
        let id = id
            .map(std::string::ToString::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let ingested = self.ingestor.ingest(content).await?;
        let processed = self.processor.process(&ingested).await?;
        self.storage.store(&id, &processed).await?;
        Ok(id)
    }

    /// Retrieves data from storage via the retriever.
    pub async fn retrieve(&self, id: &str) -> Result<String> {
        self.retriever.retrieve(id).await
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
    async fn store(&self, id: &str, data: &str) -> Result<()> {
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
        async fn store(&self, _id: &str, _data: &str) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl Retriever for MockPipeline {
        async fn retrieve(&self, id: &str) -> Result<String> {
            Ok(format!("retrieved: {}", id))
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
        );

        let result = pipeline.run("hello", Some("custom_id")).await;
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
        );

        let result = pipeline.run("hello", None).await;
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
        );

        let result = pipeline.run("hello", None).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Ingestion failed");
    }
}
