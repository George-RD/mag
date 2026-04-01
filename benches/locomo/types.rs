use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub(crate) use crate::bench_utils::openai_types::{
    OpenAiChatRequest, OpenAiChatResponse, OpenAiMessage,
};

// ── LoCoMo dataset types ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct LoCoMoSample {
    pub sample_id: String,
    pub conversation: serde_json::Map<String, serde_json::Value>,
    pub qa: Vec<LoCoMoQuestion>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct DialogueTurn {
    pub speaker: String,
    pub dia_id: String,
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LoCoMoQuestion {
    pub question: String,
    #[serde(default, deserialize_with = "deserialize_optional_answer")]
    pub answer: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_answer")]
    pub adversarial_answer: Option<String>,
    #[serde(deserialize_with = "deserialize_evidence")]
    pub evidence: Vec<String>,
    pub category: i64,
}

impl LoCoMoQuestion {
    pub fn expected_answer(&self) -> &str {
        self.answer
            .as_deref()
            .or(self.adversarial_answer.as_deref())
            .unwrap_or("")
    }

    pub fn category_key(&self) -> &'static str {
        match self.category {
            1 => "single-hop",
            2 => "temporal",
            3 => "multi-hop",
            4 => "open-domain",
            5 => "adversarial",
            _ => "unknown",
        }
    }
}

fn deserialize_optional_answer<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(text) => Ok(Some(text)),
        serde_json::Value::Number(number) => Ok(Some(number.to_string())),
        other => Ok(Some(other.to_string())),
    }
}

/// Deserialize evidence IDs, splitting entries that contain spaces or
/// semicolons (dataset has malformed entries like "D9:1 D4:4 D4:6").
/// Also normalizes "D:11:26" → "D11:26" and "D30:05" → "D30:5".
fn deserialize_evidence<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Vec<String> = Vec::deserialize(deserializer)?;
    let mut result = Vec::new();
    for entry in raw {
        // Split on spaces and semicolons
        for part in entry.split([' ', ';']) {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Fix "D:11:26" → "D11:26"
            let fixed = trimmed
                .strip_prefix("D:")
                .map_or_else(|| trimmed.to_string(), |rest| format!("D{rest}"));
            // Normalize leading zeros: "D30:05" → "D30:5"
            if let Some((prefix, num)) = fixed.rsplit_once(':')
                && let Ok(n) = num.parse::<u32>()
            {
                result.push(format!("{prefix}:{n}"));
                continue;
            }
            result.push(fixed);
        }
    }
    Ok(result)
}

// ── Retrieval hit with metadata ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct RetrievalHit {
    pub content: String,
    #[allow(dead_code)]
    pub score: f32,
    pub metadata: serde_json::Value,
}

impl RetrievalHit {
    pub fn dia_id(&self) -> Option<&str> {
        self.metadata.get("dia_id").and_then(|v| v.as_str())
    }
}

// ── Category result ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CategoryResult {
    pub total: usize,
    pub correct: usize,
    pub f1_sum: f64,
    pub evidence_recall_sum: f64,
    /// Number of questions where at least one gold dia_id appears in top-1 results.
    pub hit_at_1: usize,
    /// Number of questions where at least one gold dia_id appears in top-3 results.
    pub hit_at_3: usize,
    /// Number of questions where at least one gold dia_id appears in top-5 results.
    pub hit_at_5: usize,
    pub details: Vec<String>,
}

// ── Summary ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct LoCoMoSummary {
    pub metadata: mag::benchmarking::BenchmarkMetadata,
    pub dataset: String,
    pub scoring_mode: String,
    pub samples_evaluated: usize,
    pub questions_evaluated: usize,
    pub total_memories_ingested: usize,
    pub total_duration_seconds: f64,
    pub avg_query_ms: f64,
    pub peak_rss_kb: u64,
    pub raw_correct: usize,
    pub raw_percentage: f64,
    pub mean_f1: f64,
    pub mean_evidence_recall: f64,
    /// Fraction of questions with a gold dia_id in top-1 retrieved results.
    pub hit_at_1: f64,
    /// Fraction of questions with a gold dia_id in top-3 retrieved results.
    pub hit_at_3: f64,
    /// Fraction of questions with a gold dia_id in top-5 retrieved results.
    pub hit_at_5: f64,
    pub categories: BTreeMap<String, CategoryResult>,
    pub embedder_name: String,
    pub total_seed_ms: u64,
    pub total_embed_calls: u64,
    pub avg_embed_ms: f64,
    /// Graph edge counts by relationship type (aggregated across all samples).
    pub graph_edge_totals: BTreeMap<String, i64>,
}

/// Per-category F1 sums and counts for a single sweep factor.
pub(crate) type SweepCategoryScores = BTreeMap<String, (f64, usize)>;

/// A single row in the graph factor sweep: `(factor, per-category scores)`.
pub(crate) type SweepRow = (f64, SweepCategoryScores);
