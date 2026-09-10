//! Per-family scorecard. No aggregate grade or fabricated missing measurements.
use crate::dataset::{UnimplementedFamily, ValidationCheck};
use crate::families::{FamilyOutcome, Status};
use mag::benchmarking::BenchmarkMetadata;
use mag::memory_core::embedding_model::RetrieverModelProfile;
use serde::Serialize;
use std::collections::BTreeMap;

/// Pinned model metadata, present only when the selected embedder is
/// profile-backed.
#[derive(Debug, Clone, Serialize)]
pub struct ModelProfileSummary {
    pub model_id: String,
    pub revision: String,
    pub role: String,
    pub runtime: String,
    pub quantization: String,
    pub output_dimensions: usize,
    pub pooling: String,
    pub query_transform: String,
    pub document_transform: String,
    pub max_input_tokens: usize,
    pub licence: String,
    pub checksums: BTreeMap<String, String>,
    pub expected_model_disk_bytes: u64,
    pub expected_peak_ram_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct FamilySummary {
    pub status: Status,
    pub reason: Option<String>,
    pub metric: String,
    pub score_percentage: Option<f64>,
    pub cases: usize,
    pub p50_latency_ms: Option<f64>,
    pub p95_latency_ms: Option<f64>,
    pub latency_samples: usize,
    pub detail: serde_json::Value,
}
impl From<&FamilyOutcome> for FamilySummary {
    fn from(outcome: &FamilyOutcome) -> Self {
        let samples = outcome.latency_micros.len();
        Self {
            status: outcome.status,
            reason: outcome.reason.clone(),
            metric: outcome.metric_label.to_string(),
            score_percentage: (outcome.status == Status::Measured).then_some(outcome.score * 100.0),
            cases: outcome.cases,
            p50_latency_ms: (samples > 0).then(|| outcome.p50_ms()),
            p95_latency_ms: (samples >= 5).then(|| outcome.p95_ms()),
            latency_samples: samples,
            detail: outcome.detail.clone(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct EvalSummary {
    pub metadata: BenchmarkMetadata,
    pub dataset_version: String,
    pub dataset_sha256: String,
    pub schema_validity_percentage: f64,
    pub schema_checks: Vec<ValidationCheck>,
    pub embedder_name: String,
    pub embedding_dimension: usize,
    pub embedding_space_identity: String,
    pub model_profile: Option<ModelProfileSummary>,
    pub model_profile_reason: Option<String>,
    pub tokens: Option<u64>,
    pub tokens_reason: String,
    pub model_startup_and_warmup_ms: f64,
    pub total_duration_seconds: f64,
    pub peak_rss_kb: Option<u64>,
    pub ram_measurement: String,
    pub seeded_memories: usize,
    pub retained_memories: usize,
    pub selected_families: usize,
    pub total_families: usize,
    pub families: BTreeMap<String, FamilySummary>,
    pub historical_unimplemented_annotations: Vec<UnimplementedFamily>,
}
#[derive(Debug, Serialize)]
pub struct ValidationSummary {
    pub metadata: BenchmarkMetadata,
    pub dataset_version: String,
    pub dataset_sha256: String,
    pub schema_validity_percentage: f64,
    pub schema_checks: Vec<ValidationCheck>,
}
pub fn profile_summary(profile: &RetrieverModelProfile) -> ModelProfileSummary {
    let spec = profile.metadata();
    ModelProfileSummary {
        model_id: spec.model_id.to_string(),
        revision: spec.revision.to_string(),
        role: spec.role.to_string(),
        runtime: spec.runtime.to_string(),
        quantization: spec.quantization.to_string(),
        output_dimensions: spec.output_dimensions,
        pooling: spec.pooling.to_string(),
        query_transform: spec.query_transform.to_string(),
        document_transform: spec.document_transform.to_string(),
        max_input_tokens: spec.max_input_tokens,
        licence: spec.licence.to_string(),
        checksums: spec
            .checksums
            .iter()
            .map(|checksum| (checksum.artifact.to_string(), checksum.sha256.to_string()))
            .collect(),
        expected_model_disk_bytes: spec.local_resources.model_disk_bytes,
        expected_peak_ram_bytes: spec.local_resources.peak_ram_bytes,
    }
}

pub fn print_report(summary: &EvalSummary, outcomes: &[FamilyOutcome], quiet: bool) {
    println!(
        "MAG runtime behaviour — {} — dataset {} ({})",
        summary.embedder_name, summary.dataset_version, summary.dataset_sha256
    );
    println!(
        "{} / {} seeds retained; {} / {} families selected",
        summary.retained_memories,
        summary.seeded_memories,
        summary.selected_families,
        summary.total_families
    );
    println!("This is a synthetic runtime diagnostic, not a generative model qualification.");
    for outcome in outcomes {
        let family = FamilySummary::from(outcome);
        let score = family
            .score_percentage
            .map_or_else(|| "not measurable".to_string(), |s| format!("{s:.1}%"));
        println!(
            "{}: {} {}, cases={}, timed calls={}",
            outcome.name, score, outcome.metric_label, outcome.cases, family.latency_samples
        );
        if let Some(reason) = &family.reason {
            println!("  {reason}");
        }
        if !quiet {
            for line in &outcome.lines {
                println!("  {line}");
            }
        }
    }
    println!(
        "No overall grade: families measure different quantities. Unimplemented families are historical source annotations, not current capability detection."
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    fn outcome(status: Status, latency: Vec<u128>) -> FamilyOutcome {
        FamilyOutcome {
            name: "test",
            status,
            reason: None,
            metric_label: "test",
            score: 0.5,
            cases: 1,
            latency_micros: latency,
            detail: serde_json::Value::Null,
            lines: vec![],
        }
    }
    #[test]
    fn missing_measurement_serializes_as_null() {
        let value =
            serde_json::to_value(FamilySummary::from(&outcome(Status::NotMeasurable, vec![])))
                .unwrap();
        assert!(value["score_percentage"].is_null());
        assert!(value["p50_latency_ms"].is_null());
        assert!(value["p95_latency_ms"].is_null());
    }
    #[test]
    fn percentiles_disclose_sample_size() {
        let one = FamilySummary::from(&outcome(Status::Measured, vec![1000]));
        assert_eq!(one.p50_latency_ms, Some(1.0));
        assert!(one.p95_latency_ms.is_none());
        let five = FamilySummary::from(&outcome(
            Status::Measured,
            vec![1000, 2000, 3000, 4000, 5000],
        ));
        assert_eq!(five.p95_latency_ms, Some(5.0));
        assert_eq!(five.latency_samples, 5);
    }
}
