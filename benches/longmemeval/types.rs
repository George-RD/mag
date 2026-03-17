use std::collections::BTreeMap;

use mag::benchmarking::BenchmarkMetadata;
use mag::memory_core::ScoringParams;
use serde::{Deserialize, Serialize};

pub(crate) use crate::bench_utils::openai_types::{
    OpenAiChatRequest, OpenAiChatResponse, OpenAiMessage,
};

// ── Official LongMemEval_S dataset types ──────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct OfficialTurn {
    pub role: String,
    pub content: String,
}

/// Custom deserializer: the dataset has `answer` as either a string or int.
fn deserialize_answer<'de, D>(deserializer: D) -> std::result::Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        other => Ok(other.to_string()),
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct OfficialQuestion {
    pub question_id: String,
    pub question_type: String,
    pub question: String,
    #[serde(deserialize_with = "deserialize_answer")]
    pub answer: String,
    #[allow(dead_code)]
    pub question_date: Option<String>,
    #[allow(dead_code)]
    pub answer_session_ids: Vec<String>,
    #[allow(dead_code)]
    pub haystack_dates: Vec<String>,
    pub haystack_session_ids: Vec<String>,
    pub haystack_sessions: Vec<Vec<OfficialTurn>>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OfficialSummary {
    pub metadata: BenchmarkMetadata,
    pub dataset: String,
    pub total_questions: usize,
    pub questions_evaluated: usize,
    pub total_memories_ingested: usize,
    pub total_duration_seconds: f64,
    pub avg_memories_per_question: f64,
    pub avg_query_ms: f64,
    pub peak_rss_kb: u64,
    pub raw_correct: usize,
    pub raw_percentage: f64,
    pub task_averaged_percentage: f64,
    pub categories: BTreeMap<String, CategoryResult>,
}

// ── Shared evaluation types ───────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct QuestionEvaluation {
    pub category: String,
    pub question_type: String,
    pub question: String,
    pub expected: String,
    pub actual: String,
    pub substring_passed: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct JudgeCostEstimate {
    pub model: String,
    pub input_tokens_estimate: usize,
    pub input_rate_per_million_usd: f64,
    pub estimated_input_cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CategoryResult {
    pub total: usize,
    pub correct: usize,
    pub details: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Summary {
    pub metadata: BenchmarkMetadata,
    pub seeded_memories: usize,
    pub seeding_ms: u128,
    pub querying_ms: u128,
    pub peak_rss_kb: u64,
    pub total_correct: usize,
    pub total_questions: usize,
    pub overall_percentage: f64,
    pub categories: BTreeMap<String, CategoryResult>,
}

// ── Grid search types ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) struct GridSearchResult {
    pub label: String,
    pub params: ScoringParams,
    pub total_correct: usize,
    pub total_questions: usize,
    pub overall_percentage: f64,
    pub categories: BTreeMap<String, CategoryResult>,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GridSearchResultSummary {
    pub label: String,
    pub params: ScoringParams,
    pub total_correct: usize,
    pub total_questions: usize,
    pub overall_percentage: f64,
    pub categories: BTreeMap<String, CategoryResult>,
    pub duration_ms: u128,
}

#[derive(Debug, Serialize)]
pub(crate) struct GridSearchSummary {
    pub grid_size: usize,
    pub duration_seconds: f64,
    pub top_10: Vec<GridSearchResultSummary>,
    pub results: Vec<GridSearchResultSummary>,
}

// ── Search hit ────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct Hit {
    pub content: String,
    pub score: f32,
}
