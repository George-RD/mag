//! Dataset types and schema validation for the memory-intelligence evaluation.
//!
//! The dataset carries ground truth authored from the seed text. Validation runs
//! before any scoring so a broken annotation file fails loudly instead of
//! producing scores nobody can trust.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// One memory to seed before scoring.
#[derive(Debug, Clone, Deserialize)]
pub struct Seed {
    /// Stable slug, unique across the dataset. Mapped to a generated uuid at run time.
    pub key: String,
    /// Database partition this seed belongs to.
    pub group: String,
    pub content: String,
    pub event_type: String,
    pub tags: Vec<String>,
    pub importance: f64,
    pub session_id: String,
    /// Days relative to run start, applied through `MemoryInput::referenced_date`.
    /// `null` records the event at run time.
    pub day_offset: Option<i64>,
    pub ttl_seconds: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EntityCase {
    pub seed: String,
    /// `"<category>:<slug>"` entries; categories are `people`, `tools`, `projects`.
    pub expected: Vec<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemporalCase {
    pub id: String,
    pub query: String,
    pub expect_keys: Vec<String>,
    pub expect_absent_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RelationshipCase {
    pub from: String,
    pub to: String,
    /// `"any"` accepts whatever type MAG assigns.
    pub rel_type: String,
    pub min_weight: f64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LifecycleCase {
    pub seed: String,
    pub expect_expired_after_sweep: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SupersessionCase {
    pub old: String,
    pub new: String,
    pub expect_supersession: bool,
    pub kind: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GroupingCase {
    pub cluster_id: String,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProvenanceCase {
    pub operation: String,
    pub expect_source_link_field: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionCase {
    pub id: String,
    pub query: String,
    pub relevant_keys: Vec<String>,
    pub expect_abstain: bool,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct UnimplementedFamily {
    pub family: String,
    pub reason: String,
    pub target_shape: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dataset {
    pub schema_version: u32,
    pub dataset_version: String,
    #[allow(dead_code)]
    pub description: String,
    pub seed: Vec<Seed>,
    pub entities: Vec<EntityCase>,
    pub temporal: Vec<TemporalCase>,
    pub relationships: Vec<RelationshipCase>,
    pub lifecycle: Vec<LifecycleCase>,
    pub supersession: Vec<SupersessionCase>,
    pub grouping: Vec<GroupingCase>,
    pub provenance: Vec<ProvenanceCase>,
    pub questions: Vec<QuestionCase>,
    pub unimplemented: Vec<UnimplementedFamily>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub dataset_version: String,
    #[allow(dead_code)]
    pub dataset_file: String,
    pub sha256: String,
    pub counts: BTreeMap<String, usize>,
}

/// One named schema-validity check and its outcome.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ValidationCheck {
    pub name: String,
    pub passed: bool,
    /// Names the offending item when `passed` is false.
    pub detail: Option<String>,
}

impl ValidationCheck {
    fn pass(name: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            detail: None,
        }
    }

    fn fail(name: &str, detail: String) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            detail: Some(detail),
        }
    }

    fn from_failures(name: &str, failures: Vec<String>) -> Self {
        if failures.is_empty() {
            Self::pass(name)
        } else {
            Self::fail(name, failures.join("; "))
        }
    }
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Loads `dataset.json` and `manifest.json` from `dir`, returning the dataset,
/// the manifest, and the hex SHA-256 of the dataset bytes.
pub fn load(dir: &std::path::Path) -> Result<(Dataset, Manifest, String)> {
    let dataset_path = dir.join("dataset.json");
    let manifest_path = dir.join("manifest.json");

    let raw = std::fs::read(&dataset_path)
        .with_context(|| format!("failed to read {}", dataset_path.display()))?;
    let dataset: Dataset = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", dataset_path.display()))?;

    let manifest_raw = std::fs::read(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_raw)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    let sha = sha256_hex(&raw);
    Ok((dataset, manifest, sha))
}

const ENTITY_CATEGORIES: [&str; 3] = ["people:", "tools:", "projects:"];

/// Runs every schema-validity check. A failing check is fatal to the caller.
pub fn validate(
    dataset: &Dataset,
    manifest: &Manifest,
    dataset_sha256: &str,
) -> Vec<ValidationCheck> {
    let keys: BTreeSet<&str> = dataset.seed.iter().map(|s| s.key.as_str()).collect();
    let mut checks = Vec::new();

    checks.push(if manifest.sha256 == dataset_sha256 {
        ValidationCheck::pass("manifest_sha256_matches")
    } else {
        ValidationCheck::fail(
            "manifest_sha256_matches",
            format!(
                "manifest records {} but dataset.json hashes to {dataset_sha256}",
                manifest.sha256
            ),
        )
    });

    checks.push(
        if manifest.schema_version == dataset.schema_version
            && manifest.dataset_version == dataset.dataset_version
        {
            ValidationCheck::pass("manifest_version_matches")
        } else {
            ValidationCheck::fail(
                "manifest_version_matches",
                format!(
                    "manifest {}/{} against dataset {}/{}",
                    manifest.schema_version,
                    manifest.dataset_version,
                    dataset.schema_version,
                    dataset.dataset_version
                ),
            )
        },
    );

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut key_failures = Vec::new();
    for seed in &dataset.seed {
        if seed.key.trim().is_empty() {
            key_failures.push("empty seed key".to_string());
        } else if !seen.insert(seed.key.as_str()) {
            key_failures.push(format!("duplicate seed key {}", seed.key));
        }
    }
    checks.push(ValidationCheck::from_failures(
        "seed_keys_unique_and_non_empty",
        key_failures,
    ));

    let group_failures: Vec<String> = dataset
        .seed
        .iter()
        .filter(|s| s.group.trim().is_empty())
        .map(|s| format!("seed {} has an empty group", s.key))
        .collect();
    checks.push(ValidationCheck::from_failures(
        "seed_groups_non_empty",
        group_failures,
    ));

    let mut reference_failures = Vec::new();
    let check_ref = |origin: String, key: &str, failures: &mut Vec<String>| {
        if !keys.contains(key) {
            failures.push(format!("{origin} references unknown seed key {key}"));
        }
    };
    for case in &dataset.entities {
        check_ref(
            format!("entities[{}]", case.seed),
            &case.seed,
            &mut reference_failures,
        );
    }
    for case in &dataset.temporal {
        for key in case.expect_keys.iter().chain(&case.expect_absent_keys) {
            check_ref(
                format!("temporal[{}]", case.id),
                key,
                &mut reference_failures,
            );
        }
    }
    for case in &dataset.relationships {
        for key in [&case.from, &case.to] {
            check_ref(
                format!("relationships[{}->{}]", case.from, case.to),
                key,
                &mut reference_failures,
            );
        }
    }
    for case in &dataset.lifecycle {
        check_ref(
            format!("lifecycle[{}]", case.seed),
            &case.seed,
            &mut reference_failures,
        );
    }
    for case in &dataset.supersession {
        for key in [&case.old, &case.new] {
            check_ref(
                format!("supersession[{}->{}]", case.old, case.new),
                key,
                &mut reference_failures,
            );
        }
    }
    for case in &dataset.grouping {
        for key in &case.members {
            check_ref(
                format!("grouping[{}]", case.cluster_id),
                key,
                &mut reference_failures,
            );
        }
    }
    for case in &dataset.questions {
        for key in &case.relevant_keys {
            check_ref(
                format!("questions[{}]", case.id),
                key,
                &mut reference_failures,
            );
        }
    }
    checks.push(ValidationCheck::from_failures(
        "cross_references_resolve",
        reference_failures,
    ));

    let event_type_failures: Vec<String> = dataset
        .seed
        .iter()
        .filter(|s| !mag::memory_core::is_valid_event_type(&s.event_type))
        .map(|s| format!("seed {} has event_type {}", s.key, s.event_type))
        .collect();
    checks.push(ValidationCheck::from_failures(
        "event_types_valid",
        event_type_failures,
    ));

    let importance_failures: Vec<String> = dataset
        .seed
        .iter()
        .filter(|s| !(0.0..=1.0).contains(&s.importance))
        .map(|s| format!("seed {} has importance {}", s.key, s.importance))
        .collect();
    checks.push(ValidationCheck::from_failures(
        "importance_in_range",
        importance_failures,
    ));

    let mut entity_failures = Vec::new();
    for case in &dataset.entities {
        for expected in &case.expected {
            if !ENTITY_CATEGORIES
                .iter()
                .any(|prefix| expected.starts_with(prefix))
            {
                entity_failures.push(format!(
                    "entities[{}] annotation {expected} is not people:, tools: or projects: prefixed",
                    case.seed
                ));
            }
        }
    }
    checks.push(ValidationCheck::from_failures(
        "entity_annotations_prefixed",
        entity_failures,
    ));

    let actual_counts: BTreeMap<&str, usize> = BTreeMap::from([
        ("seed", dataset.seed.len()),
        ("entities", dataset.entities.len()),
        ("temporal", dataset.temporal.len()),
        ("relationships", dataset.relationships.len()),
        ("lifecycle", dataset.lifecycle.len()),
        ("supersession", dataset.supersession.len()),
        ("grouping", dataset.grouping.len()),
        ("provenance", dataset.provenance.len()),
        ("questions", dataset.questions.len()),
        ("unimplemented", dataset.unimplemented.len()),
    ]);
    let mut count_failures = Vec::new();
    for (name, actual) in &actual_counts {
        match manifest.counts.get(*name) {
            Some(declared) if declared == actual => {}
            Some(declared) => {
                count_failures.push(format!("{name}: manifest {declared}, actual {actual}"));
            }
            None => count_failures.push(format!("{name}: missing from manifest.counts")),
        }
    }
    for name in manifest.counts.keys() {
        if !actual_counts.contains_key(name.as_str()) {
            count_failures.push(format!("{name}: manifest declares an unknown array"));
        }
    }
    checks.push(ValidationCheck::from_failures(
        "manifest_counts_match",
        count_failures,
    ));

    checks
}

/// Percentage of validation checks that passed.
pub fn validity_percentage(checks: &[ValidationCheck]) -> f64 {
    let passed = checks.iter().filter(|c| c.passed).count();
    crate::metrics::ratio(passed, checks.len()) * 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shipped_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("data/memory_intelligence_eval/v1")
    }

    #[test]
    fn shipped_dataset_passes_every_check() {
        let (dataset, manifest, sha) = load(&shipped_dir()).expect("dataset loads");
        let checks = validate(&dataset, &manifest, &sha);
        let failures: Vec<&ValidationCheck> = checks.iter().filter(|c| !c.passed).collect();
        assert!(failures.is_empty(), "unexpected failures: {failures:?}");
        assert!((validity_percentage(&checks) - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn manifest_sha256_matches_shipped_dataset() {
        let dir = shipped_dir();
        let raw = std::fs::read(dir.join("dataset.json")).expect("dataset bytes");
        let (_, manifest, _) = load(&dir).expect("dataset loads");
        assert_eq!(manifest.sha256, sha256_hex(&raw));
    }

    #[test]
    fn sha256_of_empty_input_is_the_known_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn broken_cross_reference_fails_validation() {
        let dir = shipped_dir();
        let (mut dataset, manifest, sha) = load(&dir).expect("dataset loads");
        dataset.questions[0].relevant_keys = vec!["no-such-seed".to_string()];
        let checks = validate(&dataset, &manifest, &sha);
        let check = checks
            .iter()
            .find(|c| c.name == "cross_references_resolve")
            .expect("check present");
        assert!(!check.passed);
        assert!(check.detail.as_deref().unwrap().contains("no-such-seed"));
    }

    #[test]
    fn mismatched_sha256_fails_validation() {
        let dir = shipped_dir();
        let (dataset, manifest, _) = load(&dir).expect("dataset loads");
        let checks = validate(&dataset, &manifest, "0".repeat(64).as_str());
        let check = checks
            .iter()
            .find(|c| c.name == "manifest_sha256_matches")
            .expect("check present");
        assert!(!check.passed);
        assert!(validity_percentage(&checks) < 100.0);
    }
}
