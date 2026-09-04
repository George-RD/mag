//! Terminal report and JSON summary.

use std::collections::BTreeMap;

use mag::benchmarking::BenchmarkMetadata;
use serde::Serialize;

use crate::bench_utils::formatting::{grade, truncate};
use crate::dataset::{UnimplementedFamily, ValidationCheck};
use crate::families::{FamilyOutcome, Status};

const BANNER: &str = "========================================================================";
const RULE: &str = "------------------------------------------------------------------------";

/// Fewest latency samples that make a p95 mean anything. Below this the p95
/// column prints the sample count, because an order statistic over one or two
/// measurements is just one of those measurements wearing a percentile's label.
pub const MIN_LATENCY_SAMPLES: usize = 5;

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

/// One family as it appears in the JSON summary.
#[derive(Debug, Clone, Serialize)]
pub struct FamilySummary {
    pub status: Status,
    pub reason: Option<String>,
    pub metric: String,
    /// Headline metric scaled to 0-100.
    pub score_percentage: f64,
    pub cases: usize,
    pub p50_latency_ms: f64,
    /// Null when fewer than `MIN_LATENCY_SAMPLES` calls were timed.
    pub p95_latency_ms: Option<f64>,
    /// Number of timed calls behind the two latency figures.
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
            score_percentage: outcome.score * 100.0,
            cases: outcome.cases,
            p50_latency_ms: outcome.p50_ms(),
            p95_latency_ms: (samples >= MIN_LATENCY_SAMPLES).then(|| outcome.p95_ms()),
            latency_samples: samples,
            detail: outcome.detail.clone(),
        }
    }
}

/// Everything the run measured.
#[derive(Debug, Clone, Serialize)]
pub struct EvalSummary {
    pub metadata: BenchmarkMetadata,
    pub dataset_version: String,
    pub dataset_sha256: String,
    pub schema_validity_percentage: f64,
    pub schema_checks: Vec<ValidationCheck>,
    pub embedder_name: String,
    pub embedding_dimension: usize,
    /// The identity MAG persisted in `runtime_metadata` for the seeded database.
    pub embedding_space_identity: String,
    pub model_profile: Option<ModelProfileSummary>,
    /// Why `model_profile` is null, when it is.
    pub model_profile_reason: Option<String>,
    pub tokens: Option<u64>,
    pub tokens_reason: String,
    pub model_load_ms: f64,
    pub total_duration_seconds: f64,
    pub peak_rss_kb: u64,
    pub seeded_memories: usize,
    pub retained_memories: usize,
    /// Unweighted mean of the families that produced a score. Read it with the
    /// four fields below: the denominator changes when a family stops
    /// measuring, and `--family` narrows it further.
    pub overall_percentage: f64,
    /// Families that produced a score, the denominator of `overall_percentage`.
    pub scored_families: usize,
    /// Families this run was asked to score.
    pub selected_families: usize,
    /// Families the harness knows about, whether or not this run scored them.
    pub total_families: usize,
    /// Selected families that ran but could observe nothing.
    pub not_measurable_families: Vec<String>,
    /// Families with no production implementation, never part of the mean.
    pub unimplemented_families: usize,
    /// Null when `--family` selected a subset: a letter grade over part of the
    /// harness would read as a verdict on the whole of it.
    pub overall_grade: Option<String>,
    /// One-line statement of what `overall_percentage` is a mean of.
    pub overall_scope: String,
    pub families: BTreeMap<String, FamilySummary>,
    pub unimplemented: Vec<UnimplementedFamily>,
}

/// Summary emitted by `--validate-only`.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationSummary {
    pub metadata: BenchmarkMetadata,
    pub dataset_version: String,
    pub dataset_sha256: String,
    pub schema_validity_percentage: f64,
    pub schema_checks: Vec<ValidationCheck>,
}

/// The overall score together with the denominator it was taken over.
///
/// The mean is over families that produced a score, so the denominator moves
/// when a family stops measuring and when `--family` narrows the run. Reporting
/// the number without that count lets a run that lost a measurement read as an
/// improvement, so the two travel together.
pub struct Overall {
    pub percentage: f64,
    pub scored: usize,
    pub selected: usize,
    pub total: usize,
    pub not_measurable: Vec<String>,
    pub unimplemented: usize,
    pub grade: Option<String>,
    pub scope: String,
}

/// Builds the overall score from the families that ran.
pub fn overall(outcomes: &[FamilyOutcome], selected: usize, unimplemented: usize) -> Overall {
    let scores: Vec<f64> = outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::Measured)
        .map(|outcome| outcome.score)
        .collect();
    let not_measurable: Vec<String> = outcomes
        .iter()
        .filter(|outcome| outcome.status == Status::NotMeasurable)
        .map(|outcome| outcome.name.to_string())
        .collect();
    let percentage = crate::metrics::mean(&scores) * 100.0;
    let total = crate::ALL_FAMILIES.len();
    let partial = selected < total;

    let mut scope = format!(
        "unweighted mean of {} scored {} of {selected} run",
        scores.len(),
        if scores.len() == 1 {
            "family"
        } else {
            "families"
        }
    );
    if !not_measurable.is_empty() {
        scope.push_str(&format!(
            "; {} measured nothing this run",
            not_measurable.join(", ")
        ));
    }
    if partial {
        scope.push_str(&format!(
            "; --family selected {selected} of {total}, so this is not a whole-harness score"
        ));
    }
    scope.push_str(&format!(
        "; {unimplemented} families have no production implementation and are not scored"
    ));

    Overall {
        percentage,
        scored: scores.len(),
        selected,
        total,
        not_measurable,
        unimplemented,
        grade: if partial {
            None
        } else {
            Some(grade(percentage).to_string())
        },
        scope,
    }
}

/// Prints the schema-validity block.
pub fn print_validation(
    dataset_version: &str,
    dataset_sha256: &str,
    checks: &[ValidationCheck],
    validity: f64,
) {
    let passed = checks.iter().filter(|check| check.passed).count();
    println!("{BANNER}");
    println!("  MAG — Memory Intelligence Dataset Validation");
    println!("{BANNER}");
    println!(
        "  Dataset: {dataset_version} (sha256 {})",
        truncate(dataset_sha256, 12)
    );
    println!(
        "  Schema validity: {validity:.1}% ({passed}/{} checks)",
        checks.len()
    );
    println!();
    for check in checks {
        println!(
            "  {:<32} {}",
            check.name,
            if check.passed { "pass" } else { "FAIL" }
        );
        if let Some(detail) = &check.detail {
            println!("    {detail}");
        }
    }
}

/// Prints the full human report.
pub fn print_report(summary: &EvalSummary, outcomes: &[FamilyOutcome], quiet: bool) {
    let passed = summary.schema_checks.iter().filter(|c| c.passed).count();

    println!("{BANNER}");
    println!("  MAG — Memory Intelligence Evaluation");
    println!("{BANNER}");
    println!(
        "  Dataset: {} (sha256 {})",
        summary.dataset_version,
        truncate(&summary.dataset_sha256, 12)
    );
    println!(
        "  Schema validity: {:.1}% ({passed}/{} checks)",
        summary.schema_validity_percentage,
        summary.schema_checks.len()
    );
    println!(
        "  Embedder: {} ({}-dim)",
        summary.embedder_name, summary.embedding_dimension
    );
    let identity = &summary.embedding_space_identity;
    if identity.chars().count() > 100 {
        println!(
            "  Embedding space: {} (truncated, full value in --json)",
            truncate(identity, 100)
        );
    } else {
        println!("  Embedding space: {identity}");
    }
    match (&summary.model_profile, &summary.model_profile_reason) {
        (Some(profile), _) => {
            println!(
                "  Model profile: {} @ {}",
                profile.model_id, profile.revision
            );
        }
        (None, Some(reason)) => println!("  Model profile: none — {reason}"),
        (None, None) => println!("  Model profile: none"),
    }
    println!("  Model load: {:.1}ms", summary.model_load_ms);
    println!(
        "  Seeded: {} memories, {} retained",
        summary.seeded_memories, summary.retained_memories
    );
    println!("  Duration {:.1}s", summary.total_duration_seconds);
    println!("  Peak RSS: {} KB", summary.peak_rss_kb);
    println!("  Tokens: none — {}", summary.tokens_reason);

    if !quiet {
        for outcome in outcomes {
            println!();
            println!("  {} — {}", outcome.name, outcome.metric_label);
            println!("  {RULE}");
            if outcome.status == Status::NotMeasurable {
                println!(
                    "    not measurable: {}",
                    outcome.reason.as_deref().unwrap_or("no reason recorded")
                );
                continue;
            }
            for line in &outcome.lines {
                println!("    {line}");
            }
        }
    }

    println!();
    println!("{BANNER}");
    println!(
        "  {:<16} {:<17} {:>7}  {:>8}  {:>8}  {:>5}",
        "Family", "Metric", "Score", "p50", "p95", "Grade"
    );
    println!("  {RULE}");
    let mut small_samples = false;
    for outcome in outcomes {
        if outcome.status == Status::NotMeasurable {
            println!(
                "  {:<16} {:<17} {:>7}  {:>8}  {:>8}  {:>5}",
                outcome.name, outcome.metric_label, "n/a", "-", "-", "-"
            );
            continue;
        }
        let percentage = outcome.score * 100.0;
        let samples = outcome.latency_micros.len();
        let p95 = if samples >= MIN_LATENCY_SAMPLES {
            format!("{:.1}ms", outcome.p95_ms())
        } else {
            small_samples = true;
            format!("n={samples}")
        };
        println!(
            "  {:<16} {:<17} {:>6.1}%  {:>6.1}ms  {:>8}  {:>5}",
            outcome.name,
            outcome.metric_label,
            percentage,
            outcome.p50_ms(),
            p95,
            grade(percentage)
        );
    }
    println!("  {RULE}");
    if small_samples {
        println!(
            "  p95 reads n=<count> where the family made fewer than {MIN_LATENCY_SAMPLES} timed calls; the p50 column is then the measurement itself, not a percentile"
        );
    }
    match &summary.overall_grade {
        Some(letter) => println!(
            "  OVERALL: {:.1}% ({letter}) — {}",
            summary.overall_percentage, summary.overall_scope
        ),
        None => println!(
            "  OVERALL: {:.1}% (partial run, no grade) — {}",
            summary.overall_percentage, summary.overall_scope
        ),
    }

    println!();
    println!("  Families with no production implementation");
    println!("  {RULE}");
    for family in &summary.unimplemented {
        println!("  {}", family.family);
        println!("    {}", family.reason);
        println!("    Target shape: {}", family.target_shape);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(name: &'static str, score: f64, status: Status) -> FamilyOutcome {
        FamilyOutcome {
            name,
            status,
            reason: None,
            metric_label: "metric",
            score,
            cases: 1,
            latency_micros: vec![1],
            detail: serde_json::Value::Null,
            lines: Vec::new(),
        }
    }

    #[test]
    fn overall_names_its_denominator() {
        let outcomes: Vec<FamilyOutcome> = crate::ALL_FAMILIES
            .iter()
            .map(|name| outcome(name, 0.5, Status::Measured))
            .collect();
        let result = overall(&outcomes, crate::ALL_FAMILIES.len(), 6);
        assert!((result.percentage - 50.0).abs() < 1e-12);
        assert_eq!(result.scored, 8);
        assert_eq!(result.grade.as_deref(), Some("D"));
        assert!(result.scope.contains("8 scored families of 8 run"));
    }

    #[test]
    fn a_family_that_measures_nothing_leaves_the_mean_and_is_named() {
        let mut outcomes: Vec<FamilyOutcome> = crate::ALL_FAMILIES
            .iter()
            .map(|name| outcome(name, 0.5, Status::Measured))
            .collect();
        outcomes[6] = outcome("provenance", 0.0, Status::NotMeasurable);
        let result = overall(&outcomes, crate::ALL_FAMILIES.len(), 6);
        // Dropping a family raises the mean, so the count has to travel with it.
        assert!((result.percentage - 50.0).abs() < 1e-12);
        assert_eq!(result.scored, 7);
        assert_eq!(result.not_measurable, vec!["provenance".to_string()]);
        assert!(result.scope.contains("7 scored families of 8 run"));
        assert!(result.scope.contains("provenance measured nothing"));
    }

    #[test]
    fn a_partial_selection_gets_no_grade() {
        let outcomes = vec![outcome("lifecycle", 1.0, Status::Measured)];
        let result = overall(&outcomes, 1, 6);
        assert!((result.percentage - 100.0).abs() < 1e-12);
        assert!(result.grade.is_none());
        assert!(result.scope.contains("selected 1 of 8"));
    }

    #[test]
    fn one_sample_reports_no_p95() {
        let summary = FamilySummary::from(&outcome("entities", 1.0, Status::Measured));
        assert_eq!(summary.latency_samples, 1);
        assert!(summary.p95_latency_ms.is_none());
    }

    #[test]
    fn enough_samples_report_a_p95() {
        let mut single = outcome("questions", 1.0, Status::Measured);
        single.latency_micros = vec![1_000, 2_000, 3_000, 4_000, 5_000];
        let summary = FamilySummary::from(&single);
        assert_eq!(summary.latency_samples, 5);
        assert!((summary.p95_latency_ms.expect("p95 present") - 5.0).abs() < 1e-12);
    }
}
