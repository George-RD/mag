//! Runtime observation types shared by the recovered family scorers.
use crate::dataset::Seed;
use crate::metrics;
use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::{SearchOptions, SemanticResult};
use std::collections::{BTreeMap, BTreeSet};
mod grouping;
mod lifecycle;
mod provenance;
mod read;
mod supersession;
pub use grouping::grouping;
pub use lifecycle::lifecycle;
pub use provenance::provenance;
pub use read::{entities, questions, relationships, temporal};
pub use supersession::{SupersessionPair, supersession};

/// Similarity threshold passed to `compact`. Matches the CLI and MCP default.
pub const COMPACT_SIMILARITY_THRESHOLD: f64 = 0.6;
/// Minimum cluster size passed to `compact`. Matches the CLI and MCP default.
pub const COMPACT_MIN_CLUSTER_SIZE: usize = 3;
/// Memory count above which `auto_compact` runs. Lowered from the production
/// default of 500 so the eval corpus is large enough to trigger it.
pub const AUTO_COMPACT_COUNT_THRESHOLD: usize = 1;
/// Result depth requested from `advanced_search`.
pub const SEARCH_LIMIT: usize = 10;

/// Whether a family produced a score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Scored against a real run.
    Measured,
    /// The runtime exposes no way to observe this family. Never scored as zero.
    NotMeasurable,
}

/// One family's result.
#[derive(Debug, Clone)]
pub struct FamilyOutcome {
    pub name: &'static str,
    pub status: Status,
    pub reason: Option<String>,
    /// Name of the headline metric, printed next to the score.
    pub metric_label: &'static str,
    /// Headline metric as a fraction in `[0.0, 1.0]`.
    pub score: f64,
    pub cases: usize,
    /// Latency of the family's primary runtime call, in microseconds.
    pub latency_micros: Vec<u128>,
    /// Family-specific measurements for the JSON summary.
    pub detail: serde_json::Value,
    /// Family-specific lines for the terminal report.
    pub lines: Vec<String>,
}

impl FamilyOutcome {
    fn measured(
        name: &'static str,
        metric_label: &'static str,
        score: f64,
        cases: usize,
        latency_micros: Vec<u128>,
        detail: serde_json::Value,
        lines: Vec<String>,
    ) -> Self {
        Self {
            name,
            status: Status::Measured,
            reason: None,
            metric_label,
            score,
            cases,
            latency_micros,
            detail,
            lines,
        }
    }

    fn not_measurable(
        name: &'static str,
        metric_label: &'static str,
        reason: String,
        cases: usize,
        detail: serde_json::Value,
    ) -> Self {
        Self {
            name,
            status: Status::NotMeasurable,
            reason: Some(reason),
            metric_label,
            score: 0.0,
            cases,
            latency_micros: Vec::new(),
            detail,
            lines: Vec::new(),
        }
    }

    pub fn p50_ms(&self) -> f64 {
        metrics::percentile_of_micros(&self.latency_micros, 50.0)
    }

    pub fn p95_ms(&self) -> f64 {
        metrics::percentile_of_micros(&self.latency_micros, 95.0)
    }
}

/// A seeded database plus the key mapping needed to score it.
pub struct SeededGroup {
    pub runtime: LocalMemoryRuntime,
    /// Dataset seed key to the uuid generated for it.
    pub key_to_id: BTreeMap<String, String>,
    /// Generated uuid back to the dataset seed key.
    pub id_to_key: BTreeMap<String, String>,
    /// Seed content back to the dataset seed key.
    pub content_to_key: BTreeMap<String, String>,
    /// Seeds attempted, in dataset order.
    pub seeded: usize,
    /// Seeds MAG actually retained. A shortfall means dedup discarded a write.
    pub retained: usize,
    /// Ids that were in the database immediately after seeding. A seed key whose
    /// id is missing here never became a row, so no later observation about it
    /// is evidence of anything MAG did after the write.
    pub retained_ids: BTreeSet<String>,
}

impl SeededGroup {
    pub fn key(&self, id: &str) -> Option<&str> {
        self.id_to_key.get(id).map(String::as_str)
    }

    /// Maps a ranked result list to dataset seed keys, dropping unknown ids.
    fn ranked_keys(&self, results: &[SemanticResult]) -> Vec<String> {
        results
            .iter()
            .filter_map(|result| self.key(&result.id).map(str::to_string))
            .collect()
    }
}

/// Every stored id in a database, superseded rows included.
pub async fn stored_ids(runtime: &LocalMemoryRuntime) -> Result<BTreeSet<String>> {
    let options = SearchOptions {
        include_superseded: Some(true),
        ..SearchOptions::default()
    };
    let listed = runtime.list(0, 1000, &options).await?;
    Ok(listed.memories.into_iter().map(|m| m.id).collect())
}

/// Builds the seed content lookup used to recover cluster membership.
pub fn content_index(seeds: &[&Seed]) -> BTreeMap<String, String> {
    seeds
        .iter()
        .map(|seed| (seed.content.clone(), seed.key.clone()))
        .collect()
}
