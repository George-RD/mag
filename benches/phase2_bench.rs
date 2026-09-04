// cairn:allow-large-module reason: roughly 3100 lines of inline fixture data; the harness itself is under 200 lines
//! Phase 2 benchmark: measure retrieval quality from LLM-extracted metadata.
//!
//! Fast, deterministic benchmark for autoresearch iteration.
//! Uses PlaceholderEmbedder + MockLlmBackend so no real LLM calls are needed.
//!
//! ## Usage
//!
//! ```bash
//! cargo run --release --bin phase2_bench --features llm,substrate
//! ```
//!
//! ## Output
//!
//! Prints `METRIC hit_rate=<0..1>` on stdout for the autoresearch harness.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mag::memory_core::{
    MemoryInput, SearchOptions, embedder::PlaceholderEmbedder, llm::LlmBackend,
    storage::sqlite::SqliteStorage,
};
use mag::substrate::{
    EmbedAndExtractPipeline, IngestionPipeline, MemoryStore, WriteContext,
    extraction::{ExtractedEntity, ExtractedRelationship, ExtractedTemporal, ExtractionResult},
};

// ---------------------------------------------------------------------------
// Mock LLM backend — returns deterministic extraction results
// ---------------------------------------------------------------------------

struct MockLlmBackend {
    responses: HashMap<String, ExtractionResult>,
}

impl MockLlmBackend {
    fn new(responses: HashMap<String, ExtractionResult>) -> Self {
        Self { responses }
    }
}

#[async_trait]
impl LlmBackend for MockLlmBackend {
    async fn complete(&self, _prompt: &str, _system: Option<&str>) -> Result<String> {
        Ok(String::new())
    }

    async fn complete_structured(
        &self,
        _prompt: &str,
        _system: Option<&str>,
        _schema: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        // The extraction module calls complete_structured with the content as prompt.
        // We look up the response by prompt text.
        if let Some(result) = self.responses.get(_prompt) {
            Ok(serde_json::to_value(result)?)
        } else {
            Ok(serde_json::json!({}))
        }
    }
}

// ---------------------------------------------------------------------------
// Synthetic dataset
// ---------------------------------------------------------------------------

fn build_dataset() -> Vec<(MemoryInput, ExtractionResult)> {
    vec![
        (
            MemoryInput {
                content: "Alice and Bob met on 2025-01-15 to discuss Project Alpha using React.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Alice".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "Bob".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "Project Alpha".into(), entity_type: "project".into() },
                    ExtractedEntity { name: "React".into(), entity_type: "tool".into() },
                ],
                facts: vec![
                    "Alice and Bob met to discuss Project Alpha".into(),
                    "React is used for Project Alpha".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "2025-01-15".into(), normalized: Some("2025-01-15".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deployment to production failed because the Docker container ran out of memory on 2025-03-10.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Docker".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "production".into(), entity_type: "place".into() },
                ],
                facts: vec![
                    "Deployment to production failed".into(),
                    "Docker container ran out of memory".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "2025-03-10".into(), normalized: Some("2025-03-10".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Charlie refactored the authentication service to use OAuth2 instead of basic auth.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Charlie".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "OAuth2".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "authentication service".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "Charlie refactored the authentication service".into(),
                    "OAuth2 replaces basic auth".into(),
                ],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The CI pipeline was migrated from Jenkins to GitHub Actions in December 2023.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Jenkins".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "GitHub Actions".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "CI pipeline".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "CI pipeline migrated from Jenkins to GitHub Actions".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "December 2023".into(), normalized: Some("2025-09-01".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Dana presented the quarterly review to the board on 2025-06-01. Revenue was up 15%.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Dana".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "board".into(), entity_type: "organization".into() },
                ],
                facts: vec![
                    "Dana presented quarterly review to the board".into(),
                    "Revenue was up 15%".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "2025-06-01".into(), normalized: Some("2025-06-01".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The new caching layer uses Redis with a TTL of 5 minutes for session data.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Redis".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "caching layer".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "Caching layer uses Redis".into(),
                    "TTL is 5 minutes for session data".into(),
                ],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Eve and Frank pair-programmed the GraphQL resolver on 2025-02-20.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Eve".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "Frank".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "GraphQL resolver".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "Eve and Frank pair-programmed the GraphQL resolver".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "2025-02-20".into(), normalized: Some("2025-02-20".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The database migration from PostgreSQL 13 to 15 was completed by the infrastructure team.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "PostgreSQL".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "infrastructure team".into(), entity_type: "organization".into() },
                ],
                facts: vec![
                    "Database migration from PostgreSQL 13 to 15 completed".into(),
                ],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Grace implemented the new search algorithm using BM25 and vector similarity on 2025-04-10.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Grace".into(), entity_type: "person".into() },
                    ExtractedEntity { name: "BM25".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "search algorithm".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "Grace implemented new search algorithm".into(),
                    "Uses BM25 and vector similarity".into(),
                ],
                relationships: vec![],
                temporal: vec![
                    ExtractedTemporal { expression: "2025-04-10".into(), normalized: Some("2025-04-10".into()) },
                ],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The frontend team switched from Vue to Svelte for the dashboard rewrite.".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![
                    ExtractedEntity { name: "Vue".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "Svelte".into(), entity_type: "tool".into() },
                    ExtractedEntity { name: "frontend team".into(), entity_type: "organization".into() },
                    ExtractedEntity { name: "dashboard".into(), entity_type: "project".into() },
                ],
                facts: vec![
                    "Frontend team switched from Vue to Svelte".into(),
                    "Dashboard is being rewritten".into(),
                ],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Relationship memories: same entity, different relationships --
        (
            MemoryInput {
                content: "The gathering was successful. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Alice".into(), entity_type: "person".into() }],
                facts: vec!["Gathering was successful".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Alice".into(), predicate: "led".into(), object: "gathering".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The gathering was successful. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Alice".into(), entity_type: "person".into() }],
                facts: vec!["Gathering was successful".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Alice".into(), predicate: "facilitated".into(), object: "gathering".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assignment was completed. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Charlie".into(), entity_type: "person".into() }],
                facts: vec!["Assignment was completed".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Charlie".into(), predicate: "presented".into(), object: "assignment".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assignment was completed. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Charlie".into(), entity_type: "person".into() }],
                facts: vec!["Assignment was completed".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Charlie".into(), predicate: "reviewed".into(), object: "assignment".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The work finished on schedule. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Eve".into(), entity_type: "person".into() }],
                facts: vec!["Work finished on schedule".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Eve".into(), predicate: "developed".into(), object: "work".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The work finished on schedule. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Eve".into(), entity_type: "person".into() }],
                facts: vec!["Work finished on schedule".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Eve".into(), predicate: "tested".into(), object: "work".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The review was thorough. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Grace".into(), entity_type: "person".into() }],
                facts: vec!["Review was thorough".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Grace".into(), predicate: "approved".into(), object: "review".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The review was thorough. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Grace".into(), entity_type: "person".into() }],
                facts: vec!["Review was thorough".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Grace".into(), predicate: "rejected".into(), object: "review".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The procedure completed yesterday. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Ivan".into(), entity_type: "person".into() }],
                facts: vec!["Procedure completed yesterday".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Ivan".into(), predicate: "deployed".into(), object: "procedure".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The procedure completed yesterday. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "Ivan".into(), entity_type: "person".into() }],
                facts: vec!["Procedure completed yesterday".into()],
                relationships: vec![
                    ExtractedRelationship { subject: "Ivan".into(), predicate: "monitored".into(), object: "procedure".into(), when: None },
                ],
                temporal: vec![],
                topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Temporal memories: identical content, different dates (4 per group) --
        (
            MemoryInput {
                content: "The zephyr conclave convened. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "January 15 2025".into(), normalized: Some("2025-01-15".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The zephyr conclave convened. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "March 10 2025".into(), normalized: Some("2025-03-10".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The zephyr conclave convened. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "May 20 2025".into(), normalized: Some("2025-05-20".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The zephyr conclave convened. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "July 1 2025".into(), normalized: Some("2025-07-01".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The aurora summit transpired. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "February 20 2025".into(), normalized: Some("2025-02-20".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The aurora summit transpired. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "April 10 2025".into(), normalized: Some("2025-04-10".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The aurora summit transpired. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "June 15 2025".into(), normalized: Some("2025-06-15".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The aurora summit transpired. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "August 1 2025".into(), normalized: Some("2025-08-01".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The nexus forum assembled. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "October 10 2025".into(), normalized: Some("2025-10-10".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The nexus forum assembled. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "November 20 2025".into(), normalized: Some("2025-11-20".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The nexus forum assembled. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "December 25 2025".into(), normalized: Some("2025-12-25".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The nexus forum assembled. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![ExtractedTemporal { expression: "July 4 2025".into(), normalized: Some("2025-07-04".into()) }],
            topics: vec![],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Topic memories: identical vague content, different topics --
        (
            MemoryInput {
                content: "The discussion covered various points. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["architecture".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The discussion covered various points. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["security".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The discussion covered various points. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["performance".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The discussion covered various points. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["refactoring".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The session addressed multiple concerns. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["deployment".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The session addressed multiple concerns. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["monitoring".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The session addressed multiple concerns. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["incident".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The session addressed multiple concerns. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["postmortem".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The meeting explored different angles. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["planning".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The meeting explored different angles. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["review".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The meeting explored different angles. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["retrospective".into()],
            sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The meeting explored different angles. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["roadmap".into()],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Sentiment memories: vague content, different sentiments --
        (
            MemoryInput {
                content: "The luminary event transpired. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("positive".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The luminary event transpired. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("negative".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The luminary event transpired. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("neutral".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The luminary event transpired. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("positive".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The stellar outcome emerged. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("negative".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The stellar outcome emerged. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("positive".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The stellar outcome emerged. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("neutral".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The stellar outcome emerged. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("negative".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The quantum result manifested. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("neutral".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The quantum result manifested. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("positive".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The quantum result manifested. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("negative".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The quantum result manifested. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: Some("neutral".into()),
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Action memories: vague content, different actions --
        (
            MemoryInput {
                content: "The paradigm shift occurred. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["deployed".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The paradigm shift occurred. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["tested".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The paradigm shift occurred. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["reviewed".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The paradigm shift occurred. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["migrated".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The catalyst emerged. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["refactored".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The catalyst emerged. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["optimized".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The catalyst emerged. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["debugged".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The catalyst emerged. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["documented".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone arrived. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["analyzed".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone arrived. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["implemented".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone arrived. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["verified".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone arrived. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec!["deprecated".into()],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Location memories: vague content, different locations --
        (
            MemoryInput {
                content: "The encounter unfolded. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["tokyo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The encounter unfolded. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["berlin".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The encounter unfolded. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["sydney".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The encounter unfolded. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["cairo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The revelation surfaced. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["mumbai".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The revelation surfaced. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["lagos".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The revelation surfaced. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["oslo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The revelation surfaced. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["lima".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assembly formed. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["dubai".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assembly formed. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["seoul".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assembly formed. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["prague".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The assembly formed. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec!["helsinki".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // -- Decision memories: vague content, different decisions --
        (
            MemoryInput {
                content: "The deliberation concluded. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["deferred".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deliberation concluded. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["accepted".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deliberation concluded. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["revised".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deliberation concluded. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["approved".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The negotiation ended. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["escalated".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The negotiation ended. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["delegated".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The negotiation ended. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["consolidated".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The negotiation ended. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["rejected".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The evaluation finished. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["expedited".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The evaluation finished. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["suspended".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The evaluation finished. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["prioritized".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The evaluation finished. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec!["postponed".into()],
                questions: vec![],
                status: None,
            },
        ),
        // -- Question memories: vague content, different questions --
        (
            MemoryInput {
                content: "The inquiry arose. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["pending".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The inquiry arose. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["disputed".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The inquiry arose. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["verified".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The inquiry arose. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["resolved".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The concern emerged. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["dismissed".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The concern emerged. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["escalated".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The concern emerged. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["deferred".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The concern emerged. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["validated".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The matter appeared. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["assigned".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The matter appeared. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["transferred".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The matter appeared. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["tabled".into()],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The matter appeared. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec!["clarified".into()],
                status: None,
            },
        ),
        // -- Cross-cutting memories: identical content, multi-dimensional tags --
        (
            MemoryInput {
                content: "The initiative advanced. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "xander".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["architecture".into()],
                sentiment: None,
                actions: vec!["reviewed".into()],
                locations: vec!["tokyo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The initiative advanced. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "xander".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["architecture".into()],
                sentiment: None,
                actions: vec!["deployed".into()],
                locations: vec!["berlin".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The initiative advanced. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "yara".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["architecture".into()],
                sentiment: None,
                actions: vec!["reviewed".into()],
                locations: vec!["tokyo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The initiative advanced. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "xander".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["design".into()],
                sentiment: None,
                actions: vec!["reviewed".into()],
                locations: vec!["tokyo".into()],
                decisions: vec!["approved".into()],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The endeavor progressed. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "zara".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["security".into()],
                sentiment: None,
                actions: vec!["tested".into()],
                locations: vec!["cairo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The endeavor progressed. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "zara".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["security".into()],
                sentiment: None,
                actions: vec!["optimized".into()],
                locations: vec!["lima".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The endeavor progressed. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "wren".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["security".into()],
                sentiment: None,
                actions: vec!["tested".into()],
                locations: vec!["cairo".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The endeavor progressed. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "zara".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["performance".into()],
                sentiment: None,
                actions: vec!["tested".into()],
                locations: vec!["cairo".into()],
                decisions: vec!["rejected".into()],
                questions: vec![],
                status: None,
            },
        ),
        // -- Cross-cutting group 3: identical content, multi-dimensional tags --
        (
            MemoryInput {
                content: "The milestone achieved. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "quinn".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["performance".into()],
                sentiment: None,
                actions: vec!["optimized".into()],
                locations: vec!["dublin".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone achieved. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "quinn".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["performance".into()],
                sentiment: None,
                actions: vec!["tested".into()],
                locations: vec!["dublin".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone achieved. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "quinn".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["security".into()],
                sentiment: None,
                actions: vec!["optimized".into()],
                locations: vec!["dublin".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The milestone achieved. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "quinn".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["performance".into()],
                sentiment: None,
                actions: vec!["optimized".into()],
                locations: vec!["dublin".into()],
                decisions: vec!["approved".into()],
                questions: vec![],
                status: None,
            },
        ),
        // -- Cross-cutting group 4: identical content, multi-dimensional tags --
        (
            MemoryInput {
                content: "The deadline approached. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "tessa".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["backend".into()],
                sentiment: None,
                actions: vec!["refactored".into()],
                locations: vec!["mumbai".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deadline approached. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "tessa".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["backend".into()],
                sentiment: None,
                actions: vec!["deployed".into()],
                locations: vec!["mumbai".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deadline approached. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "tessa".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["frontend".into()],
                sentiment: None,
                actions: vec!["refactored".into()],
                locations: vec!["mumbai".into()],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The deadline approached. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![ExtractedEntity { name: "tessa".into(), entity_type: "person".into() }],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec!["backend".into()],
                sentiment: None,
                actions: vec!["refactored".into()],
                locations: vec!["mumbai".into()],
                decisions: vec!["expedited".into()],
                questions: vec![],
                status: None,
            },
        ),
        // -- Status memories: vague content, different statuses --
        (
            MemoryInput {
                content: "The verdict rendered. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("pending".into()),
            },
        ),
        (
            MemoryInput {
                content: "The verdict rendered. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("blocked".into()),
            },
        ),
        (
            MemoryInput {
                content: "The verdict rendered. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("in-progress".into()),
            },
        ),
        (
            MemoryInput {
                content: "The verdict rendered. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("completed".into()),
            },
        ),
        (
            MemoryInput {
                content: "The analysis wrapped. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("cancelled".into()),
            },
        ),
        (
            MemoryInput {
                content: "The analysis wrapped. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("deferred".into()),
            },
        ),
        (
            MemoryInput {
                content: "The analysis wrapped. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("pending".into()),
            },
        ),
        (
            MemoryInput {
                content: "The analysis wrapped. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("blocked".into()),
            },
        ),
        (
            MemoryInput {
                content: "The sprint ended. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("in-progress".into()),
            },
        ),
        (
            MemoryInput {
                content: "The sprint ended. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("completed".into()),
            },
        ),
        (
            MemoryInput {
                content: "The sprint ended. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("cancelled".into()),
            },
        ),
        (
            MemoryInput {
                content: "The sprint ended. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: Some("deferred".into()),
            },
        ),
        (
            MemoryInput {
                content: "The team built the payment module. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The team tested the payment module. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The team deployed the payment module. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The team patched the payment module. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The alpha system was built. (A)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The beta system was tested. (B)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The gamma system was deployed. (C)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "The alpha beta system was fixed. (D)".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Priority evaluation item (A)".to_string(),
                importance: 0.2,
                tags: vec!["topic:priority-evaluation".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Priority evaluation item (B)".to_string(),
                importance: 0.4,
                tags: vec!["topic:priority-evaluation".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Priority evaluation item (C)".to_string(),
                importance: 0.6,
                tags: vec!["topic:priority-evaluation".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        (
            MemoryInput {
                content: "Priority evaluation item (D)".to_string(),
                importance: 0.8,
                tags: vec!["topic:priority-evaluation".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![],
                facts: vec![],
                relationships: vec![],
                temporal: vec![],
                topics: vec![],
                sentiment: None,
                actions: vec![],
                locations: vec![],
                decisions: vec![],
                questions: vec![],
                status: None,
            },
        ),
        // ── Content-only group (FTS5-only path) ────────────────────────────
        (
            MemoryInput {
                content: "Alpha content only item".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Beta content only item".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Gamma content only item".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Delta content only item".to_string(),
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        // ── Tag-only group (tag search carries, no FTS5 match) ─────────────
        (
            MemoryInput {
                content: "Unrelated note alpha".to_string(),
                tags: vec!["topic:target-query".to_string()],
                importance: 0.9,
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Unrelated note beta".to_string(),
                tags: vec!["topic:other-topic".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Unrelated note gamma".to_string(),
                tags: vec!["topic:another-topic".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Unrelated note delta".to_string(),
                tags: vec!["topic:yet-another".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        // ── Low-coverage group (many query tokens, few tag matches) ─────────
        (
            MemoryInput {
                content: "Quasar nebula partial match test (A)".to_string(),
                tags: vec!["topic:quasar".to_string(), "topic:nebula".to_string(), "topic:pulsar".to_string(), "topic:comet".to_string(), "priority:low".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Quasar nebula partial match test (B)".to_string(),
                tags: vec!["topic:quasar".to_string(), "topic:nebula".to_string(), "topic:pulsar".to_string(), "topic:comet".to_string(), "priority:medium".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Quasar nebula partial match test (C)".to_string(),
                tags: vec!["topic:quasar".to_string(), "topic:nebula".to_string(), "topic:pulsar".to_string(), "topic:comet".to_string(), "priority:high".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Quasar nebula partial match test (D)".to_string(),
                tags: vec!["topic:quasar".to_string(), "topic:nebula".to_string(), "topic:pulsar".to_string(), "topic:comet".to_string(), "priority:critical".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        // ── Tag-synonym group (tag search synonym expansion) ───────────────
        (
            MemoryInput {
                content: "Zenith system note (A)".to_string(),
                tags: vec!["action:patch".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Zenith system note (B)".to_string(),
                tags: vec!["action:patch".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Zenith system note (C)".to_string(),
                tags: vec!["action:patch".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Zenith system note (D)".to_string(),
                tags: vec!["action:patch".to_string(), "topic:vertex-fix".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        // ── Temporal date expansion group ──────────────────────────────────
        (
            MemoryInput {
                content: "Orion quarterly review (A)".to_string(),
                tags: vec!["temporal:2024-01-15".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Orion quarterly review (B)".to_string(),
                tags: vec!["temporal:2024-04-20".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Orion quarterly review (C)".to_string(),
                tags: vec!["temporal:2024-07-10".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
        (
            MemoryInput {
                content: "Orion quarterly review (D)".to_string(),
                tags: vec!["temporal:2024-10-05".to_string()],
                ..Default::default()
            },
            ExtractionResult {
                entities: vec![], facts: vec![], relationships: vec![],
                temporal: vec![], topics: vec![], sentiment: None,
                actions: vec![], locations: vec![], decisions: vec![],
                questions: vec![], status: None,
            },
        ),
    ]
}

// ---------------------------------------------------------------------------
// Queries — each targets a specific memory by entity/fact/temporal
// ---------------------------------------------------------------------------

fn build_queries() -> Vec<(&'static str, usize)> {
    // (query_text, expected_memory_index)
    vec![
        ("Who met to discuss Project Alpha?", 0),
        ("What tool is used for Project Alpha?", 0),
        ("What happened on January 15 2025?", 0),
        ("Docker memory issue", 1),
        ("deployment failure March 2024", 1),
        ("Who refactored authentication?", 2),
        ("OAuth2 migration", 2),
        ("Jenkins to GitHub Actions", 3),
        ("CI pipeline migration 2023", 3),
        ("quarterly review board presentation", 4),
        ("Dana revenue report", 4),
        ("Redis caching session data", 5),
        ("caching layer TTL", 5),
        ("Eve Frank pair programming", 6),
        ("GraphQL resolver February 2024", 6),
        ("PostgreSQL migration infrastructure", 7),
        ("database upgrade team", 7),
        ("Grace search algorithm BM25", 8),
        ("vector similarity implementation", 8),
        ("Vue to Svelte switch", 9),
        ("frontend dashboard rewrite", 9),
        // Relationship queries (require tag search on relationships)
        ("Alice led gathering", 10),
        ("Alice facilitated gathering", 11),
        ("Charlie presented task", 12),
        ("Charlie reviewed assignment", 13),
        ("Eve developed activity", 14),
        ("Eve tested work", 15),
        ("Grace approved process", 16),
        ("Grace rejected review", 17),
        ("Ivan deployed operation", 18),
        ("Ivan monitored procedure", 19),
        // Temporal queries (require tag search on temporal tags; expect 4th in group)
        ("zephyr conclave 2025-07-01", 23),
        ("nexus forum 2025-07-04", 31),
        // Topic queries (require tag search on topic tags; expect 4th in group)
        ("refactoring discussion", 35),
        ("postmortem session", 39),
        ("roadmap meeting", 43),
        // Sentiment queries (require tag search on sentiment tags; expect 4th in group)
        ("positive luminary event", 47),
        ("negative stellar outcome", 51),
        ("neutral quantum result", 55),
        // Action queries (require tag search on action tags; expect 4th in group)
        ("migrated paradigm shift", 59),
        ("documented catalyst", 63),
        ("deprecated milestone", 67),
        // Location queries (require tag search on location tags; expect 4th in group)
        ("cairo encounter", 71),
        ("lima revelation", 75),
        ("helsinki assembly", 79),
        // Decision queries (require tag search on decision tags; expect 4th in group)
        ("approved deliberation", 83),
        ("rejected negotiation", 87),
        ("postponed evaluation", 91),
        // Question queries (require tag search on question tags; expect 4th in group)
        ("resolved inquiry", 95),
        ("validated concern", 99),
        ("clarified matter", 103),
        // Cross-cutting queries (match multiple dimensions; expect specific combination)
        ("xander architecture tokyo reviewed initiative", 104),
        ("xander tokyo reviewed approved initiative", 107),
        ("zara security cairo tested endeavor", 108),
        ("zara cairo tested rejected endeavor", 111),
        // Cross-cutting group 3/4 queries
        ("quinn performance optimized dublin milestone", 112),
        ("quinn performance optimized dublin approved milestone", 115),
        ("tessa backend refactored mumbai deadline", 116),
        ("tessa backend refactored mumbai expedited deadline", 119),
        // Status queries (require tag search on status tags; expect 4th in group)
        ("completed verdict", 123),
        ("blocked analysis", 127),
        ("deferred sprint", 131),
        // Synonym queries (require FTS5 synonym expansion; expect 4th in group)
        ("fix payment", 135),
        // Bigram phrase queries (test FTS5 bigram path; expect 4th in group)
        ("alpha beta system", 139),
        // Importance queries (test importance ranking; expect highest importance)
        ("priority evaluation", 143),
        // Content-only group
        ("delta content only", 147),
        // Tag-only group
        ("target query", 148),
        // Low-coverage group
        ("quasar nebula pulsar comet epsilon zeta critical", 155),
        // Tag-synonym group
        ("fix zenith system", 159),
        // Temporal date expansion group
        ("july orion", 162),
    ]
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[allow(clippy::cast_precision_loss)]
#[tokio::main]
async fn main() -> Result<()> {
    let dataset = build_dataset();
    let queries = build_queries();

    // Build mock LLM responses keyed by content string.
    let mut responses = HashMap::<String, ExtractionResult>::new();
    for (input, result) in &dataset {
        responses.insert(input.content.clone(), result.clone());
    }
    let mock_llm = Arc::new(MockLlmBackend::new(responses));

    // Create storage with PlaceholderEmbedder (fast, deterministic).
    let embedder: Arc<dyn mag::memory_core::embedder::Embedder> = Arc::new(PlaceholderEmbedder);
    let storage = SqliteStorage::new_in_memory_with_embedder(Arc::clone(&embedder))?;

    // Create pipeline with mock LLM.
    let pipeline = EmbedAndExtractPipeline::new(Arc::clone(&embedder)).with_llm(mock_llm);

    // Store all memories with deterministic IDs.
    let mut ids = Vec::new();
    for (idx, (input, _result)) in dataset.iter().enumerate() {
        let assigned_id = format!("mem-{idx:03}");
        let ctx = WriteContext {
            input: input.clone(),
            assigned_id: assigned_id.clone(),
            embedding: None,
        };
        pipeline.ingest(ctx, &storage).await?;
        ids.push(assigned_id);
    }

    // Run queries and compute hit rate.
    let opts = SearchOptions::default();
    let mut hits = 0usize;

    for (query, expected_idx) in &queries {
        let results = storage.search(query, 3, &opts).await?;

        let expected_id = &ids[*expected_idx];
        let found = results.iter().any(|r| r.id == *expected_id);
        if found {
            hits += 1;
        }
    }

    let hit_rate = hits as f64 / queries.len() as f64;
    println!("METRIC hit_rate={hit_rate:.4}");
    println!(
        "ASI total_queries={} hits={} miss={}",
        queries.len(),
        hits,
        queries.len() - hits
    );

    Ok(())
}
