#![cfg(feature = "llm")]

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use mag::memory_core::llm::LlmBackend;
use mag::{IntelligenceRequest, LocalMemoryRuntime, MAX_INTELLIGENCE_BYTES};
use serde_json::json;

struct Backend {
    calls: AtomicUsize,
    completion: String,
    fail: bool,
}

#[async_trait]
impl LlmBackend for Backend {
    async fn complete(&self, prompt: &str, system: Option<&str>) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(system.unwrap().contains("source_ids"));
        let prompt: serde_json::Value = serde_json::from_str(prompt)?;
        assert!(prompt.get("expected").is_none());
        assert!(prompt.get("case_id").is_none());
        if self.fail {
            anyhow::bail!("secret backend response");
        }
        Ok(self.completion.clone())
    }

    async fn complete_structured(
        &self,
        _: &str,
        _: Option<&str>,
        _: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        panic!("evaluation must not call the repairing structured path");
    }
}

fn backend(completion: &str) -> Backend {
    Backend { calls: AtomicUsize::new(0), completion: completion.to_string(), fail: false }
}

fn request() -> IntelligenceRequest {
    serde_json::from_value(json!({
        "schema_version": 1, "task": "facts", "instruction": "Extract owner=NAME.",
        "sources": [{"id": "m1", "text": "Iris owns Atlas."}]
    })).unwrap()
}

#[tokio::test]
async fn runtime_preserves_every_completion_without_repair_or_retry() {
    for text in ["", "not json", "```json\n{\"items\":[]}\n```", " {\"items\":[]}\n"] {
        let backend = backend(text);
        let actual = LocalMemoryRuntime::produce_intelligence(&backend, &request()).await.unwrap();
        assert_eq!(actual, text);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn runtime_validates_requests_before_calling_model() {
    let mut invalid = Vec::new();
    let mut version = request(); version.schema_version = 2; invalid.push(version);
    let mut blank = request(); blank.instruction = " \n".into(); invalid.push(blank);
    let mut empty = request(); empty.sources.clear(); invalid.push(empty);
    let mut id = request(); id.sources[0].id = " ".into(); invalid.push(id);
    let mut text = request(); text.sources[0].text.clear(); invalid.push(text);
    let mut duplicate = request(); duplicate.sources.push(duplicate.sources[0].clone()); invalid.push(duplicate);
    let mut huge = request(); huge.sources[0].text = "a".repeat(MAX_INTELLIGENCE_BYTES); invalid.push(huge);
    for request in invalid {
        let backend = backend("unused");
        assert!(LocalMemoryRuntime::produce_intelligence(&backend, &request).await.is_err());
        assert_eq!(backend.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn runtime_redacts_backend_errors_and_never_retries() {
    let mut backend = backend("unused"); backend.fail = true;
    let error = LocalMemoryRuntime::produce_intelligence(&backend, &request()).await.unwrap_err();
    assert!(error.to_string().contains("backend failed"));
    assert!(!format!("{error:#}").contains("secret"));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn runtime_rejects_oversized_completions_without_salvage() {
    let backend = backend(&"x".repeat(MAX_INTELLIGENCE_BYTES + 1));
    let error = LocalMemoryRuntime::produce_intelligence(&backend, &request()).await.unwrap_err();
    assert!(error.to_string().contains("completion exceeds byte limit"));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn request_contract_accepts_all_tasks_and_rejects_duplicate_or_unknown_fields() {
    for task in ["facts", "entities", "temporal", "relationships", "decisions", "questions", "status", "grouping", "contradictions", "provenance"] {
        let mut value = serde_json::to_value(request()).unwrap(); value["task"] = json!(task);
        assert!(serde_json::from_value::<IntelligenceRequest>(value).is_ok());
    }
    let mut nested = serde_json::to_value(request()).unwrap();
    nested["sources"][0]["expected"] = json!("gold");
    assert!(serde_json::from_value::<IntelligenceRequest>(nested).is_err());
    for field in ["schema_version", "task", "instruction", "sources"] {
        let value = serde_json::to_value(request()).unwrap();
        let input = format!("{{\"{field}\":{},{}", value[field], &value.to_string()[1..]);
        assert!(serde_json::from_str::<IntelligenceRequest>(&input).is_err());
    }
}
