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

mod validation;
pub use validation::validate;

/// Percentage of validation checks that passed.
pub fn validity_percentage(checks: &[ValidationCheck]) -> f64 {
    let passed = checks.iter().filter(|c| c.passed).count();
    crate::metrics::ratio(passed, checks.len()) * 100.0
}

#[cfg(test)]
mod tests;
