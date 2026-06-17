//! Phase 2: LLM-powered extraction of facts, entities, relationships, and temporal data.
//!
//! Gated by the `llm` feature flag. When disabled, the module still compiles but
//! the extractor is a no-op stub so that callers do not need cfg-gates everywhere.
//!
//! ## Extraction schema
//!
//! The LLM returns structured JSON with:
//! - `entities`: named entities with type classification
//! - `facts`: discrete factual statements
//! - `relationships`: subject-predicate-object triples
//! - `temporal`: date/time expressions with ISO 8601 normalization
//!
//! ## Integration
//!
//! Results are merged into `MemoryInput` before storage:
//! - Entities -> `entity:{type}:{slug}` tags (augmenting rule-based extraction)
//! - Facts -> `llm:fact:{slug}` tags + full list in `metadata["llm_facts"]`
//! - Relationships -> `metadata["llm_relationships"]`
//! - Temporal -> normalizes `referenced_date` if unset

#[cfg(feature = "llm")]
use std::sync::Arc;

#[cfg(feature = "llm")]
use crate::memory_core::llm::LlmBackend;
use crate::memory_core::storage::sqlite::{is_valid_entity, slugify};
#[cfg(feature = "llm")]
use anyhow::Context;
use anyhow::Result;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

/// Entity extracted from memory content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// Display name, e.g. "Alice Smith"
    pub name: String,
    /// One of: person | tool | project | organization | place | concept | other
    pub entity_type: String,
}

/// A relationship triple.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedRelationship {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// ISO 8601 date/time when the relationship occurred, if known.
    pub when: Option<String>,
}

/// Temporal expression with normalized form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedTemporal {
    /// Raw text from source ("last Tuesday", "January 15, 2024")
    pub expression: String,
    /// ISO 8601 normalized form, if determinable ("2024-01-15")
    pub normalized: Option<String>,
}

/// Full extraction result from the LLM.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExtractionResult {
    /// Named entities found in the content.
    #[serde(default)]
    pub entities: Vec<ExtractedEntity>,
    /// Discrete factual statements (short, imperative form).
    #[serde(default)]
    pub facts: Vec<String>,
    /// Relationship triples.
    #[serde(default)]
    pub relationships: Vec<ExtractedRelationship>,
    /// Date/time references.
    #[serde(default)]
    pub temporal: Vec<ExtractedTemporal>,
    /// High-level topics or themes.
    #[serde(default)]
    pub topics: Vec<String>,
    /// Overall sentiment: positive | negative | neutral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sentiment: Option<String>,
    /// Key actions or verbs describing what happened.
    #[serde(default)]
    pub actions: Vec<String>,
    /// Geographic or location references.
    #[serde(default)]
    pub locations: Vec<String>,
    /// Questions raised in the content.
    #[serde(default)]
    pub questions: Vec<String>,
    /// Decisions or resolutions made.
    #[serde(default)]
    pub decisions: Vec<String>,
    /// Status or state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

impl ExtractionResult {
    /// Convert extracted entities to `entity:{type}:{slug}` tags.
    pub fn entity_tags(&self) -> Vec<String> {
        self.entities
            .iter()
            .filter_map(|e| {
                let s = slugify(&e.name);
                let ty = normalize_entity_type(&e.entity_type);
                if is_valid_entity(&s) {
                    Some(format!("entity:{ty}:{s}"))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Convert extracted facts to `llm:fact:{slug}` tags (capped at 32 chars per slug).
    pub fn fact_tags(&self) -> Vec<String> {
        self.facts
            .iter()
            .map(|f| {
                let s = slugify_fact(f);
                format!("llm:fact:{s}")
            })
            .collect()
    }
    /// Convert extracted relationships to `rel:{subject}:{predicate}:{object}` tags.
    pub fn relationship_tags(&self) -> Vec<String> {
        self.relationships
            .iter()
            .filter_map(|r| {
                let sub = slugify(&r.subject);
                let pred = slugify(&r.predicate);
                let obj = slugify(&r.object);
                if sub.is_empty() || pred.is_empty() || obj.is_empty() {
                    return None;
                }
                Some(format!("rel:{sub}:{pred}:{obj}"))
            })
            .collect()
    }

    /// Return the earliest/most specific normalized temporal expression, for
    /// use as `referenced_date` when the caller has not set one.
    pub fn best_temporal_date(&self) -> Option<&str> {
        for t in &self.temporal {
            if let Some(ref norm) = t.normalized
                && norm.len() >= 10
            {
                return Some(norm.as_str());
            }
        }
        None
    }

    /// Convert extracted temporal expressions to `temporal:{date}` tags.
    pub fn temporal_tags(&self) -> Vec<String> {
        self.temporal
            .iter()
            .filter_map(|t| {
                t.normalized.as_ref().and_then(|norm| {
                    if norm.len() >= 7 {
                        Some(format!("temporal:{norm}"))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Convert extracted topics to `topic:{slug}` tags.
    pub fn topic_tags(&self) -> Vec<String> {
        self.topics
            .iter()
            .filter_map(|t| {
                let s = slugify(t);
                if s.is_empty() {
                    None
                } else {
                    Some(format!("topic:{s}"))
                }
            })
            .collect()
    }

    /// Convert sentiment to a `sentiment:{value}` tag.
    pub fn sentiment_tags(&self) -> Vec<String> {
        self.sentiment
            .as_ref()
            .and_then(|s| {
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "positive" | "negative" | "neutral" => Some(format!("sentiment:{lower}")),
                    _ => None,
                }
            })
            .into_iter()
            .collect()
    }

    /// Convert extracted actions to `action:{slug}` tags.
    pub fn action_tags(&self) -> Vec<String> {
        self.actions
            .iter()
            .filter_map(|a| {
                let s = slugify(a);
                if s.is_empty() {
                    None
                } else {
                    Some(format!("action:{s}"))
                }
            })
            .collect()
    }

    /// Convert extracted locations to `location:{slug}` tags.
    pub fn location_tags(&self) -> Vec<String> {
        self.locations
            .iter()
            .filter_map(|l| {
                let s = slugify(l);
                if s.is_empty() {
                    None
                } else {
                    Some(format!("location:{s}"))
                }
            })
            .collect()
    }
    /// Convert extracted decisions to `decision:{slug}` tags.
    pub fn decision_tags(&self) -> Vec<String> {
        self.decisions
            .iter()
            .filter_map(|d| {
                let s = slugify(d);
                if s.is_empty() {
                    None
                } else {
                    Some(format!("decision:{s}"))
                }
            })
            .collect()
    }
    /// Convert extracted questions to `question:{slug}` tags.
    pub fn question_tags(&self) -> Vec<String> {
        self.questions
            .iter()
            .filter_map(|q| {
                let s = slugify(q);
                if s.is_empty() {
                    None
                } else {
                    Some(format!("question:{s}"))
                }
            })
            .collect()
    }
    /// Convert extracted status to status tag.
    pub fn status_tags(&self) -> Vec<String> {
        self.status
            .as_ref()
            .and_then(|s: &String| {
                let lower = s.to_lowercase();
                match lower.as_str() {
                    "completed" | "blocked" | "in-progress" | "pending" | "deferred"
                    | "cancelled" => Some(format!("status:{lower}")),
                    _ => None,
                }
            })
            .into_iter()
            .collect()
    }
    /// Return all extraction data as a JSON value for storage in metadata.
    pub fn to_metadata(&self) -> serde_json::Value {
        match serde_json::to_value(self) {
            Ok(v) => v,
            Err(_) => serde_json::json!({}),
        }
    }
}

fn normalize_entity_type(raw: &str) -> &'static str {
    let lower = raw.to_lowercase();
    match lower.as_str() {
        "people" | "person" => "people",
        "tool" | "tools" => "tools",
        "project" | "projects" => "projects",
        "organization" | "org" | "company" => "organization",
        "place" | "location" => "place",
        "concept" | "idea" => "concept",
        _ => "other",
    }
}
fn slugify_fact(fact: &str) -> String {
    let cleaned: String = fact
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == ' ' {
                c
            } else {
                ' '
            }
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().take(5).collect();
    let slug = words.join("-");
    if slug.len() > 32 {
        slug[..32].to_string()
    } else {
        slug
    }
}

#[cfg(feature = "llm")]
const SYSTEM_PROMPT: &str = r#"Extract structured metadata from the memory snippet:

- entities: people, tools, projects, organizations, places, concepts
- facts: standalone factual statements
- relationships: subject-predicate-object triples (optional date)
- temporal: date/time expressions (ISO 8601 YYYY-MM-DD when possible)
- topics: high-level themes (1-3 words each)
- sentiment: positive, negative, or neutral
- actions: key verbs describing what happened
- locations: cities, countries, buildings, regions
- decisions: resolutions or conclusions reached
- questions: questions raised in the content
- status: completed, blocked, in-progress, pending, deferred, cancelled

Return valid JSON only. No prose outside JSON."#;

#[cfg(feature = "llm")]
fn extraction_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "entities": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "entity_type": { "type": "string", "enum": ["person", "tool", "project", "organization", "place", "concept", "other"] }
                    },
                    "required": ["name", "entity_type"]
                }
            },
            "facts": {
                "type": "array",
                "items": { "type": "string" }
            },
            "relationships": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "subject": { "type": "string" },
                        "predicate": { "type": "string" },
                        "object": { "type": "string" },
                        "when": { "type": ["string", "null"] }
                    },
                    "required": ["subject", "predicate", "object"]
                }
            },
            "temporal": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string" },
                        "normalized": { "type": ["string", "null"] }
                    },
                    "required": ["expression"]
                }
            },
            "topics": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Themes (1-3 words)"
            },
            "sentiment": {
                "type": "string",
                "enum": ["positive", "negative", "neutral"],
                "description": "Emotional tone"
            },
            "actions": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Key verbs or actions"
            },
            "locations": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Geographic references"
            },
            "decisions": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Resolutions or conclusions"
            },
            "questions": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Questions raised"
            },
            "status": {
                "type": "string",
                "enum": ["completed", "blocked", "in-progress", "pending", "deferred", "cancelled"],
                "description": "Task or project status"
            }
        }
    })
}

// ---------------------------------------------------------------------------
// LlmExtractor (requires llm feature)
#[cfg(feature = "llm")]
/// Extracts structured information from memory content using an LLM backend.
pub struct LlmExtractor {
    // ---------------------------------------------------------------------------
    pub llm: Arc<dyn LlmBackend>,
}

#[cfg(feature = "llm")]
impl LlmExtractor {
    pub async fn extract(&self, content: &str) -> Result<ExtractionResult> {
        let schema = extraction_schema();
        let raw: serde_json::Value = self
            .llm
            .complete_structured(content, Some(SYSTEM_PROMPT), &schema)
            .await
            .context("LLM extraction failed")?;
        let result: ExtractionResult =
            serde_json::from_value(raw).context("LLM extraction deserialization failed")?;
        Ok(result)
    }
}
// No-op stub (used when llm feature is off)
// ---------------------------------------------------------------------------

/// When the `llm` feature is disabled, extraction returns empty results.
pub struct NoOpExtractor;

impl Default for NoOpExtractor {
    fn default() -> Self {
        Self::new()
    }
}
impl NoOpExtractor {
    pub fn new() -> Self {
        Self
    }

    pub async fn extract(&self, _content: &str) -> Result<ExtractionResult> {
        Ok(ExtractionResult::default())
    }
}
