use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use romega_memory::memory_core::ScoringParams;

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
pub(crate) struct ScoringParamsSnapshot {
    pub rrf_k: f64,
    pub rrf_weight_vec: f64,
    pub rrf_weight_fts: f64,
    pub abstention_min_text: f64,
    pub graph_neighbor_factor: f64,
    pub graph_min_edge_weight: f64,
    pub word_overlap_weight: f64,
    pub jaccard_weight: f64,
    pub importance_floor: f64,
    pub importance_scale: f64,
    pub context_tag_weight: f64,
    pub time_decay_days: f64,
    pub priority_base: f64,
    pub priority_scale: f64,
    pub feedback_heavy_suppress: f64,
    pub feedback_strong_suppress: f64,
    pub feedback_positive_scale: f64,
    pub feedback_positive_cap: f64,
    pub feedback_heavy_threshold: i64,
    pub neighbor_word_overlap_weight: f64,
    pub neighbor_importance_floor: f64,
    pub neighbor_importance_scale: f64,
    pub graph_seed_min: usize,
    pub graph_seed_max: usize,
}

impl From<&ScoringParams> for ScoringParamsSnapshot {
    fn from(params: &ScoringParams) -> Self {
        Self {
            rrf_k: params.rrf_k,
            rrf_weight_vec: params.rrf_weight_vec,
            rrf_weight_fts: params.rrf_weight_fts,
            abstention_min_text: params.abstention_min_text,
            graph_neighbor_factor: params.graph_neighbor_factor,
            graph_min_edge_weight: params.graph_min_edge_weight,
            word_overlap_weight: params.word_overlap_weight,
            jaccard_weight: params.jaccard_weight,
            importance_floor: params.importance_floor,
            importance_scale: params.importance_scale,
            context_tag_weight: params.context_tag_weight,
            time_decay_days: params.time_decay_days,
            priority_base: params.priority_base,
            priority_scale: params.priority_scale,
            feedback_heavy_suppress: params.feedback_heavy_suppress,
            feedback_strong_suppress: params.feedback_strong_suppress,
            feedback_positive_scale: params.feedback_positive_scale,
            feedback_positive_cap: params.feedback_positive_cap,
            feedback_heavy_threshold: params.feedback_heavy_threshold,
            neighbor_word_overlap_weight: params.neighbor_word_overlap_weight,
            neighbor_importance_floor: params.neighbor_importance_floor,
            neighbor_importance_scale: params.neighbor_importance_scale,
            graph_seed_min: params.graph_seed_min,
            graph_seed_max: params.graph_seed_max,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GridSearchResultSummary {
    pub label: String,
    pub params: ScoringParamsSnapshot,
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

// ── OpenAI types ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiChatRequest {
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u16,
    pub messages: Vec<OpenAiMessage>,
}

#[derive(Debug, Serialize)]
pub(crate) struct OpenAiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChatResponse {
    pub choices: Vec<OpenAiChoice>,
    #[serde(default)]
    pub usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiUsage {
    pub prompt_tokens: u64,
    #[allow(dead_code)]
    pub completion_tokens: u64,
    #[allow(dead_code)]
    pub total_tokens: u64,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiChoice {
    pub message: OpenAiResponseMessage,
}

#[derive(Debug, Deserialize)]
pub(crate) struct OpenAiResponseMessage {
    pub content: String,
}
