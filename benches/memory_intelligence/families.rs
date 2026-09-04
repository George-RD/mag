// cairn:allow-large-module reason: one scorer per evaluation family; each reads the same SeededGroup and returns the same FamilyOutcome, so splitting scatters one contract across files
//! One scorer per task family.
//!
//! Every scorer observes MAG through `LocalMemoryRuntime` only. Nothing here
//! reimplements production logic, and nothing infers an outcome MAG did not
//! actually produce.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use anyhow::Result;
use mag::LocalMemoryRuntime;
use mag::memory_core::{SearchOptions, SemanticResult};
use serde_json::json;

use crate::dataset::{
    EntityCase, GroupingCase, LifecycleCase, ProvenanceCase, QuestionCase, RelationshipCase, Seed,
    SupersessionCase, TemporalCase,
};
use crate::metrics::{self, Counts};

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

// ── entities ──────────────────────────────────────────────────────────────

/// Reads `entity:*` tags off every stored memory and scores them against the
/// annotated entity sets.
pub async fn entities(group: &SeededGroup, cases: &[EntityCase]) -> Result<FamilyOutcome> {
    let options = SearchOptions {
        include_superseded: Some(true),
        ..SearchOptions::default()
    };
    let started = Instant::now();
    let listed = group.runtime.list(0, 1000, &options).await?;
    let latency = vec![started.elapsed().as_micros()];

    let mut observed: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for memory in &listed.memories {
        let Some(key) = group.key(&memory.id) else {
            continue;
        };
        let tags = memory
            .tags
            .iter()
            .filter_map(|tag| tag.strip_prefix("entity:"))
            .map(str::to_string)
            .collect();
        observed.insert(key, tags);
    }

    let mut micro = Counts::default();
    let mut per_case_f1 = Vec::new();
    let mut lines = Vec::new();
    let mut case_details = Vec::new();

    for case in cases {
        let expected = metrics::set_of(case.expected.clone());
        let predicted = observed
            .get(case.seed.as_str())
            .cloned()
            .unwrap_or_default();
        let counts = metrics::compare_sets(&predicted, &expected);
        micro.add(counts);
        let prf = counts.prf();
        per_case_f1.push(prf.f1);

        let spurious: Vec<String> = predicted.difference(&expected).cloned().collect();
        let missed: Vec<String> = expected.difference(&predicted).cloned().collect();
        if !spurious.is_empty() || !missed.is_empty() {
            lines.push(format!(
                "{:<22} missed [{}] spurious [{}]",
                case.seed,
                missed.join(", "),
                spurious.join(", ")
            ));
        }
        case_details.push(json!({
            "seed": case.seed,
            "expected": expected.iter().collect::<Vec<_>>(),
            "observed": predicted.iter().collect::<Vec<_>>(),
            "f1": prf.f1,
            "note": case.note,
        }));
    }

    let micro_prf = micro.prf();
    let macro_f1 = metrics::mean(&per_case_f1);
    let detail = json!({
        "micro_precision": micro_prf.precision,
        "micro_recall": micro_prf.recall,
        "micro_f1": micro_prf.f1,
        "macro_f1": macro_f1,
        "true_positives": micro.true_positives,
        "false_positives": micro.false_positives,
        "false_negatives": micro.false_negatives,
        "cases": case_details,
    });

    lines.insert(
        0,
        format!(
            "micro P {:.1}% / R {:.1}% / F1 {:.1}%   macro F1 {:.1}%",
            micro_prf.precision * 100.0,
            micro_prf.recall * 100.0,
            micro_prf.f1 * 100.0,
            macro_f1 * 100.0
        ),
    );

    Ok(FamilyOutcome::measured(
        "entities",
        "micro F1",
        micro_prf.f1,
        cases.len(),
        latency,
        detail,
        lines,
    ))
}

// ── temporal ──────────────────────────────────────────────────────────────

/// Issues each relative-date query through `advanced_search` and checks which
/// annotated memories land in the top `SEARCH_LIMIT`.
pub async fn temporal(group: &SeededGroup, cases: &[TemporalCase]) -> Result<FamilyOutcome> {
    let mut latency = Vec::new();
    let mut recalls = Vec::new();
    let mut false_inclusions = 0usize;
    let mut absent_expectations = 0usize;
    let mut lines = Vec::new();
    let mut case_details = Vec::new();

    for case in cases {
        let started = Instant::now();
        let results = group
            .runtime
            .advanced_search(&case.query, SEARCH_LIMIT, &SearchOptions::default())
            .await?;
        latency.push(started.elapsed().as_micros());

        let ranked = group.ranked_keys(&results);
        let ranked_set: BTreeSet<String> = ranked.iter().cloned().collect();
        let expected = metrics::set_of(case.expect_keys.clone());
        let recall = metrics::recall_at_k(&ranked, &expected, SEARCH_LIMIT);
        recalls.push(recall);

        let leaked: Vec<String> = case
            .expect_absent_keys
            .iter()
            .filter(|key| ranked_set.contains(*key))
            .cloned()
            .collect();
        absent_expectations += case.expect_absent_keys.len();
        false_inclusions += leaked.len();

        let missed: Vec<String> = expected.difference(&ranked_set).cloned().collect();
        lines.push(format!(
            "{:<18} recall@{SEARCH_LIMIT} {:5.1}%  returned {:2}  missed [{}]  leaked [{}]",
            case.id,
            recall * 100.0,
            results.len(),
            missed.join(", "),
            leaked.join(", ")
        ));
        case_details.push(json!({
            "id": case.id,
            "query": case.query,
            "recall_at_10": recall,
            "returned": results.len(),
            "missed": missed,
            "false_inclusions": leaked,
        }));
    }

    let mean_recall = metrics::mean(&recalls);
    let false_inclusion_rate = metrics::ratio(false_inclusions, absent_expectations);
    let detail = json!({
        "mean_recall_at_10": mean_recall,
        "false_inclusion_rate": false_inclusion_rate,
        "false_inclusions": false_inclusions,
        "absent_expectations": absent_expectations,
        "cases": case_details,
    });

    lines.insert(
        0,
        format!(
            "mean recall@{SEARCH_LIMIT} {:.1}%   false inclusion {:.1}% ({false_inclusions}/{absent_expectations})",
            mean_recall * 100.0,
            false_inclusion_rate * 100.0
        ),
    );

    Ok(FamilyOutcome::measured(
        "temporal",
        "mean recall@10",
        mean_recall,
        cases.len(),
        latency,
        detail,
        lines,
    ))
}

// ── relationships ─────────────────────────────────────────────────────────

/// Checks each annotated edge against `get_relationships` and records the
/// observed edge-type histogram.
///
/// Precision is not reported: the dataset annotates the edges a correct system
/// must create, not every pair that must stay unlinked, so an unannotated edge
/// is not evidence of an error.
pub async fn relationships(
    group: &SeededGroup,
    cases: &[RelationshipCase],
) -> Result<FamilyOutcome> {
    let mut latency = Vec::new();
    let mut found = 0usize;
    let mut lines = Vec::new();
    let mut case_details = Vec::new();
    let mut histogram: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_edges: BTreeSet<String> = BTreeSet::new();

    for case in cases {
        let (Some(from_id), Some(to_id)) = (
            group.key_to_id.get(&case.from),
            group.key_to_id.get(&case.to),
        ) else {
            case_details.push(json!({
                "from": case.from,
                "to": case.to,
                "found": false,
                "note": "seed was not stored",
            }));
            continue;
        };

        let started = Instant::now();
        let edges = group.runtime.get_relationships(from_id).await?;
        latency.push(started.elapsed().as_micros());

        for edge in &edges {
            if seen_edges.insert(edge.id.clone()) {
                *histogram.entry(edge.rel_type.clone()).or_default() += 1;
            }
        }

        let matched = edges.iter().find(|edge| {
            let connects = (edge.source_id == *from_id && edge.target_id == *to_id)
                || (edge.source_id == *to_id && edge.target_id == *from_id);
            let typed = case.rel_type == "any" || edge.rel_type == case.rel_type;
            connects && typed && edge.weight >= case.min_weight
        });

        if matched.is_some() {
            found += 1;
        }
        lines.push(format!(
            "{:<18} -> {:<18} {}",
            case.from,
            case.to,
            matched.map_or_else(
                || format!("missing (min weight {:.2})", case.min_weight),
                |edge| format!("{} weight {:.3}", edge.rel_type, edge.weight)
            )
        ));
        case_details.push(json!({
            "from": case.from,
            "to": case.to,
            "found": matched.is_some(),
            "rel_type": matched.map(|edge| edge.rel_type.clone()),
            "weight": matched.map(|edge| edge.weight),
            "note": case.note,
        }));
    }

    let recall = metrics::ratio(found, cases.len());
    let detail = json!({
        "recall": recall,
        "annotated_edges": cases.len(),
        "annotated_edges_found": found,
        "precision": serde_json::Value::Null,
        "precision_reason":
            "the dataset annotates required edges only, so unannotated edges are not labelled negatives",
        "observed_edge_types": histogram,
        "cases": case_details,
    });

    let histogram_text = histogram
        .iter()
        .map(|(name, count)| format!("{name} {count}"))
        .collect::<Vec<_>>()
        .join(", ");
    lines.insert(
        0,
        format!(
            "recall {:.1}% ({found}/{})   observed edge types: {}",
            recall * 100.0,
            cases.len(),
            if histogram_text.is_empty() {
                "none".to_string()
            } else {
                histogram_text
            }
        ),
    );
    lines.push(
        "precision not reported: only required edges are annotated, so unannotated edges are unlabelled".to_string(),
    );

    Ok(FamilyOutcome::measured(
        "relationships",
        "recall",
        recall,
        cases.len(),
        latency,
        detail,
        lines,
    ))
}

// ── lifecycle ─────────────────────────────────────────────────────────────

/// Sweeps expired memories and compares what disappeared with the annotation.
///
/// A seed that never became a row is not scored. Its absence after the sweep is
/// the write being discarded, not `sweep_expired` removing anything, and
/// crediting it would award the sweep a correct expiry it never performed.
pub async fn lifecycle(group: &SeededGroup, cases: &[LifecycleCase]) -> Result<FamilyOutcome> {
    let started = Instant::now();
    let swept = group.runtime.sweep_expired().await?;
    let latency = vec![started.elapsed().as_micros()];

    let survivors = stored_ids(&group.runtime).await?;
    let mut correct = 0usize;
    let mut scored = 0usize;
    let mut never_stored: Vec<String> = Vec::new();
    let mut lines = Vec::new();
    let mut case_details = Vec::new();

    for case in cases {
        let stored_id = group
            .key_to_id
            .get(&case.seed)
            .filter(|id| group.retained_ids.contains(*id));
        let Some(id) = stored_id else {
            never_stored.push(case.seed.clone());
            lines.push(format!(
                "{:<14} not scored: the seed never reached the database, so the sweep had nothing to remove",
                case.seed
            ));
            case_details.push(json!({
                "seed": case.seed,
                "expect_expired_after_sweep": case.expect_expired_after_sweep,
                "scored": false,
                "reason": "seed was discarded on write and never became a row",
            }));
            continue;
        };

        let expired = !survivors.contains(id);
        let matches = expired == case.expect_expired_after_sweep;
        scored += 1;
        if matches {
            correct += 1;
        }
        lines.push(format!(
            "{:<14} expected {:<11} observed {:<11} {}",
            case.seed,
            if case.expect_expired_after_sweep {
                "expired"
            } else {
                "retained"
            },
            if expired { "expired" } else { "retained" },
            if matches { "ok" } else { "MISMATCH" }
        ));
        case_details.push(json!({
            "seed": case.seed,
            "expect_expired_after_sweep": case.expect_expired_after_sweep,
            "scored": true,
            "expired": expired,
            "correct": matches,
        }));
    }

    if scored == 0 {
        return Ok(FamilyOutcome::not_measurable(
            "lifecycle",
            "accuracy",
            format!(
                "none of the {} annotated seeds reached the database, so sweep_expired had nothing to act on",
                cases.len()
            ),
            cases.len(),
            json!({
                "accuracy": serde_json::Value::Null,
                "swept_rows": swept,
                "seeds_never_stored": never_stored,
                "cases": case_details,
            }),
        ));
    }

    let accuracy = metrics::ratio(correct, scored);
    let exact_match = correct == scored;
    let detail = json!({
        "accuracy": accuracy,
        "exact_set_match": exact_match,
        "scored_cases": scored,
        "annotated_cases": cases.len(),
        "seeds_never_stored": never_stored,
        "swept_rows": swept,
        "cases": case_details,
    });

    lines.insert(
        0,
        format!(
            "accuracy {:.1}% ({correct}/{scored} scored of {} annotated)   sweep removed {swept} row(s)   exact set match: {}",
            accuracy * 100.0,
            cases.len(),
            if exact_match { "yes" } else { "no" }
        ),
    );
    if !never_stored.is_empty() {
        lines.insert(
            1,
            format!(
                "{} seed(s) never reached the database and are not scored: {}",
                never_stored.len(),
                never_stored.join(", ")
            ),
        );
    }

    Ok(FamilyOutcome::measured(
        "lifecycle",
        "accuracy",
        accuracy,
        scored,
        latency,
        detail,
        lines,
    ))
}

// ── supersession ──────────────────────────────────────────────────────────

/// One annotated supersession pair, seeded into its own database.
pub struct SupersessionPair<'a> {
    pub case: &'a SupersessionCase,
    pub group: &'a SeededGroup,
}

/// Stores each pair in its own database, then reads the version chain and the
/// `SUPERSEDES` edge to decide whether MAG superseded the older memory.
pub async fn supersession(pairs: &[SupersessionPair<'_>]) -> Result<FamilyOutcome> {
    let mut latency = Vec::new();
    let mut counts = Counts::default();
    let mut per_kind: BTreeMap<String, Counts> = BTreeMap::new();
    let mut lines = Vec::new();
    let mut case_details = Vec::new();
    let mut deduped = 0usize;

    for pair in pairs {
        let case = pair.case;
        let old_id = pair.group.key_to_id.get(&case.old).cloned();
        let new_id = pair.group.key_to_id.get(&case.new).cloned();
        let stored = stored_ids(&pair.group.runtime).await?;

        let new_discarded = new_id.as_ref().is_none_or(|id| !stored.contains(id));
        if new_discarded {
            deduped += 1;
        }

        let mut detected = false;
        let mut edge_seen = false;
        let mut chain_seen = false;
        let old_discarded = old_id.as_ref().is_none_or(|id| !stored.contains(id));
        if let (Some(old_id), Some(new_id)) = (old_id.as_ref(), new_id.as_ref())
            && !new_discarded
            && !old_discarded
        {
            let started = Instant::now();
            let chain = pair.group.runtime.version_chain(new_id).await?;
            latency.push(started.elapsed().as_micros());

            chain_seen = chain.iter().any(|entry| {
                entry.id == *old_id
                    && entry
                        .metadata
                        .get("superseded_by_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(new_id.as_str())
            });

            let edges = pair.group.runtime.get_relationships(new_id).await?;
            edge_seen = edges.iter().any(|edge| {
                edge.rel_type == "SUPERSEDES"
                    && edge.source_id == *new_id
                    && edge.target_id == *old_id
            });
            detected = chain_seen || edge_seen;
        }

        let case_counts = Counts {
            true_positives: usize::from(detected && case.expect_supersession),
            false_positives: usize::from(detected && !case.expect_supersession),
            false_negatives: usize::from(!detected && case.expect_supersession),
        };
        counts.add(case_counts);
        per_kind
            .entry(case.kind.clone())
            .or_default()
            .add(case_counts);

        lines.push(format!(
            "{:<16} {:<22} expected {:<5} observed {:<5} {}{}",
            case.kind,
            format!("{} -> {}", case.old, case.new),
            case.expect_supersession,
            detected,
            if detected == case.expect_supersession {
                "ok"
            } else {
                "MISMATCH"
            },
            if new_discarded {
                "  (new memory discarded by dedup before supersession could run)"
            } else {
                ""
            }
        ));
        case_details.push(json!({
            "old": case.old,
            "new": case.new,
            "kind": case.kind,
            "expect_supersession": case.expect_supersession,
            "detected": detected,
            "detected_via_version_chain": chain_seen,
            "detected_via_supersedes_edge": edge_seen,
            "new_memory_discarded_by_dedup": new_discarded,
            "note": case.note,
        }));
    }

    let prf = counts.prf();
    let by_kind: BTreeMap<String, serde_json::Value> = per_kind
        .iter()
        .map(|(kind, kind_counts)| {
            let kind_prf = kind_counts.prf();
            (
                kind.clone(),
                json!({
                    "precision": kind_prf.precision,
                    "recall": kind_prf.recall,
                    "f1": kind_prf.f1,
                    "true_positives": kind_counts.true_positives,
                    "false_positives": kind_counts.false_positives,
                    "false_negatives": kind_counts.false_negatives,
                }),
            )
        })
        .collect();

    let detail = json!({
        "precision": prf.precision,
        "recall": prf.recall,
        "f1": prf.f1,
        "pairs_lost_to_dedup": deduped,
        "by_kind": by_kind,
        "cases": case_details,
    });

    lines.insert(
        0,
        format!(
            "P {:.1}% / R {:.1}% / F1 {:.1}%   {deduped} pair(s) lost to dedup before supersession",
            prf.precision * 100.0,
            prf.recall * 100.0,
            prf.f1 * 100.0
        ),
    );

    Ok(FamilyOutcome::measured(
        "supersession",
        "F1",
        prf.f1,
        pairs.len(),
        latency,
        detail,
        lines,
    ))
}

// ── grouping ──────────────────────────────────────────────────────────────

/// Runs `compact` as a dry run, applies it, then reconstructs the clusters MAG
/// produced from the merged content and scores them against the labelled
/// clusters.
///
/// `compact` reports only cluster sizes and a 100-character preview on a dry
/// run, so membership is recovered from the applied merge, which joins member
/// content with `\n---\n`.
///
/// Coverage is scored over labelled clusters of two or more members. A
/// one-member labelled cluster is satisfied by the memory simply existing
/// un-clustered, so folding it into the headline would award a point for doing
/// nothing. Singletons are reported separately as left alone, which a wrongly
/// merged singleton would fail.
pub async fn grouping(group: &SeededGroup, cases: &[GroupingCase]) -> Result<FamilyOutcome> {
    let event_type = "task_completion";
    let started = Instant::now();
    let dry = group
        .runtime
        .compact(
            event_type,
            COMPACT_SIMILARITY_THRESHOLD,
            COMPACT_MIN_CLUSTER_SIZE,
            true,
        )
        .await?;
    // The dry run is the family's primary call. The applied merge that follows
    // exists to recover membership; timing it alongside would put two different
    // operations under one percentile.
    let latency = vec![started.elapsed().as_micros()];

    let apply_started = Instant::now();
    let applied = group
        .runtime
        .compact(
            event_type,
            COMPACT_SIMILARITY_THRESHOLD,
            COMPACT_MIN_CLUSTER_SIZE,
            false,
        )
        .await?;
    let apply_micros = apply_started.elapsed().as_micros();

    let listed = group
        .runtime
        .list(0, 1000, &SearchOptions::default())
        .await?;
    let mut predicted: Vec<BTreeSet<String>> = Vec::new();
    let mut unmapped = 0usize;
    for memory in &listed.memories {
        let mut cluster = BTreeSet::new();
        for part in memory.content.split("\n---\n") {
            match group.content_to_key.get(part.trim()) {
                Some(key) => {
                    cluster.insert(key.clone());
                }
                None => unmapped += 1,
            }
        }
        if !cluster.is_empty() {
            predicted.push(cluster);
        }
    }

    let gold: Vec<BTreeSet<String>> = cases
        .iter()
        .map(|case| metrics::set_of(case.members.clone()))
        .collect();
    let multi_member: Vec<BTreeSet<String>> = gold
        .iter()
        .filter(|cluster| cluster.len() >= 2)
        .cloned()
        .collect();
    let singletons: Vec<&BTreeSet<String>> =
        gold.iter().filter(|cluster| cluster.len() == 1).collect();
    let singletons_left_alone = singletons
        .iter()
        .filter(|reference| predicted.contains(**reference))
        .count();

    let purity = metrics::cluster_purity(&predicted, &gold);
    let coverage = metrics::cluster_coverage(&predicted, &multi_member);

    let clusters_found = dry
        .get("clusters_found")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let memories_compacted = applied
        .get("memories_compacted")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);

    let mut lines = vec![
        format!(
            "coverage {:.1}% over {} multi-member labelled cluster(s); {}/{} singleton(s) left alone",
            coverage * 100.0,
            multi_member.len(),
            singletons_left_alone,
            singletons.len()
        ),
        format!(
            "purity {:.1}%   clusters found {clusters_found}   memories compacted {memories_compacted}",
            purity * 100.0
        ),
    ];
    lines.push(format!(
        "compact parameters: event_type={event_type} similarity_threshold={COMPACT_SIMILARITY_THRESHOLD} min_cluster_size={COMPACT_MIN_CLUSTER_SIZE}"
    ));
    lines.push(format!(
        "applied merge took {:.1}ms; it is excluded from the latency columns, which time the dry run only",
        metrics::micros_to_ms(apply_micros)
    ));
    if group.retained < group.seeded {
        lines.push(format!(
            "{} of {} seeds reached the database; content dedup discarded the rest before clustering ran",
            group.retained, group.seeded
        ));
    }
    for case in cases {
        let reference = metrics::set_of(case.members.clone());
        let recovered = predicted.contains(&reference);
        let verdict = match (case.members.len(), recovered) {
            (1, true) => "left alone (not scored)",
            (1, false) => "MERGED AWAY (not scored)",
            (_, true) => "recovered",
            (_, false) => "SPLIT",
        };
        lines.push(format!(
            "{:<16} members {:<2} {verdict}",
            case.cluster_id,
            case.members.len(),
        ));
    }
    if unmapped > 0 {
        lines.push(format!(
            "{unmapped} merged segment(s) did not match any seed content"
        ));
    }

    let detail = json!({
        "cluster_purity": purity,
        "cluster_coverage": coverage,
        "coverage_denominator": multi_member.len(),
        "coverage_denominator_rule": "labelled clusters of two or more members; a one-member cluster is recovered by the memory merely existing",
        "singleton_clusters": singletons.len(),
        "singleton_clusters_left_alone": singletons_left_alone,
        "similarity_threshold": COMPACT_SIMILARITY_THRESHOLD,
        "min_cluster_size": COMPACT_MIN_CLUSTER_SIZE,
        "applied_merge_ms": metrics::micros_to_ms(apply_micros),
        "dry_run_result": dry,
        "applied_result": applied,
        "observed_clusters": predicted
            .iter()
            .map(|cluster| cluster.iter().collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        "unmapped_segments": unmapped,
        "seeded_memories": group.seeded,
        "retained_memories": group.retained,
    });

    if multi_member.is_empty() {
        return Ok(FamilyOutcome::not_measurable(
            "grouping",
            "cluster coverage",
            format!(
                "all {} labelled cluster(s) have a single member, so nothing in the dataset requires clustering",
                cases.len()
            ),
            cases.len(),
            detail,
        ));
    }

    // Coverage is the headline: purity alone reaches 100% when MAG produces
    // only singletons, which is the failure mode this family exists to catch.
    Ok(FamilyOutcome::measured(
        "grouping",
        "cluster coverage",
        coverage,
        multi_member.len(),
        latency,
        detail,
        lines,
    ))
}

// ── provenance ────────────────────────────────────────────────────────────

/// Reads each seeded row's `superseded_by_id`, returning the ones that carry a
/// non-empty link.
async fn source_links(
    group: &SeededGroup,
    ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut links = BTreeMap::new();
    for id in ids {
        let chain = group.runtime.version_chain(id).await?;
        if let Some(target) = chain
            .iter()
            .find(|entry| entry.id == *id)
            .and_then(|entry| entry.metadata.get("superseded_by_id"))
            .and_then(serde_json::Value::as_str)
            .filter(|target| !target.is_empty())
        {
            links.insert(id.clone(), target.to_string());
        }
    }
    Ok(links)
}

/// Applies `auto_compact` and checks that each link it wrote leads somewhere.
///
/// Counting retired rows that carry a `superseded_by_id` would be a tautology:
/// `auto_compact` increments its retired count inside the same statement that
/// writes that column, so the two can only ever agree. What is falsifiable is
/// whether the link is usable afterwards. For every row whose link appeared
/// during the call this scores four conditions: the target row exists, the
/// target was not itself retired, the retired row is hidden from a default
/// `list()`, and the retired row is still readable with `include_superseded`.
///
/// Links present before the call are excluded from both sides of the ratio.
/// `store_raw` also writes `superseded_by_id` for the event types in
/// `is_supersession_type`, and attributing those to `auto_compact` would put a
/// numerator and a denominator that count different events into one fraction.
pub async fn provenance(group: &SeededGroup, cases: &[ProvenanceCase]) -> Result<FamilyOutcome> {
    // Seeds that dedup discarded never became rows, so asking for their version
    // chain is an error rather than an empty answer.
    let before_ids = stored_ids(&group.runtime).await?;
    let links_before = source_links(group, &before_ids).await?;

    let started = Instant::now();
    let result = group
        .runtime
        .auto_compact(AUTO_COMPACT_COUNT_THRESHOLD, false)
        .await?;
    let latency = vec![started.elapsed().as_micros()];

    let reported_retired = usize::try_from(
        result
            .get("total_compacted")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    )
    .unwrap_or(0);

    let after_ids = stored_ids(&group.runtime).await?;
    let links_after = source_links(group, &after_ids).await?;
    let visible: BTreeSet<String> = group
        .runtime
        .list(0, 1000, &SearchOptions::default())
        .await?
        .memories
        .into_iter()
        .map(|memory| memory.id)
        .collect();

    let new_links: BTreeMap<&String, &String> = links_after
        .iter()
        .filter(|(id, _)| !links_before.contains_key(*id))
        .collect();

    let operations: Vec<serde_json::Value> = cases
        .iter()
        .map(|case| {
            json!({
                "operation": case.operation,
                "expect_source_link_field": case.expect_source_link_field,
                "note": case.note,
            })
        })
        .collect();
    let operation_labels: Vec<String> = cases
        .iter()
        .map(|case| format!("{} -> {}", case.operation, case.expect_source_link_field))
        .collect();

    let mut intact = 0usize;
    let mut row_details = Vec::new();
    let mut failures = Vec::new();
    for (id, target) in &new_links {
        let target_exists = after_ids.contains(*target);
        let target_survived = target_exists && !links_after.contains_key(*target);
        let hidden_by_default = !visible.contains(*id);
        let readable_when_included = after_ids.contains(*id);
        let ok = target_survived && hidden_by_default && readable_when_included;
        if ok {
            intact += 1;
        } else {
            failures.push(format!(
                "{}: target_exists={target_exists} target_survived={target_survived} hidden_by_default={hidden_by_default} readable_with_include_superseded={readable_when_included}",
                group.key(id).unwrap_or(id.as_str())
            ));
        }
        row_details.push(json!({
            "retired_seed": group.key(id),
            "target_seed": group.key(target),
            "target_exists": target_exists,
            "target_survived": target_survived,
            "hidden_by_default_list": hidden_by_default,
            "readable_with_include_superseded": readable_when_included,
            "link_intact": ok,
        }));
    }

    let mut detail = json!({
        "link_integrity": serde_json::Value::Null,
        "links_written_by_auto_compact": new_links.len(),
        "links_intact": intact,
        "links_present_before_call": links_before.len(),
        "auto_compact_reported_retired": reported_retired,
        "auto_compact_result": result,
        "annotated_operations": operations,
        "retired_rows": row_details,
    });

    if new_links.is_empty() {
        return Ok(FamilyOutcome::not_measurable(
            "provenance",
            "link integrity",
            format!(
                "auto_compact wrote no new source link at count_threshold={AUTO_COMPACT_COUNT_THRESHOLD} (it reported {reported_retired} retired), so there is no link to follow"
            ),
            cases.len(),
            detail,
        ));
    }

    let integrity = metrics::ratio(intact, new_links.len());
    detail["link_integrity"] = json!(integrity);

    let mut lines = vec![format!(
        "link integrity {:.1}%   auto_compact wrote {} source link(s), {intact} lead to a surviving row that still hides the retired one",
        integrity * 100.0,
        new_links.len()
    )];
    if new_links.len() != reported_retired {
        lines.push(format!(
            "auto_compact reported {reported_retired} retired row(s) but {} gained a superseded_by_id during the call; the score counts the links observed",
            new_links.len()
        ));
    }
    if links_before.is_empty() {
        lines.push(
            "no seeded row carried a source link before the call, so every link scored here was written by auto_compact".to_string(),
        );
    } else {
        lines.push(format!(
            "{} row(s) already carried a link before the call and are excluded from both sides of the ratio",
            links_before.len()
        ));
    }
    lines.push(format!(
        "auto_compact parameters: count_threshold={AUTO_COMPACT_COUNT_THRESHOLD} dry_run=false, triggered={}",
        result
            .get("triggered")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
    ));
    lines.push(format!(
        "annotated operations: {}",
        operation_labels.join(", ")
    ));
    lines.push(
        "compact is not covered by this score: it hard-deletes cluster members and records no source link"
            .to_string(),
    );
    for failure in &failures {
        lines.push(format!("BROKEN LINK {failure}"));
    }

    Ok(FamilyOutcome::measured(
        "provenance",
        "link integrity",
        integrity,
        new_links.len(),
        latency,
        detail,
        lines,
    ))
}

// ── questions ─────────────────────────────────────────────────────────────

/// Scores retrieval and abstention over the annotated question set.
pub async fn questions(group: &SeededGroup, cases: &[QuestionCase]) -> Result<FamilyOutcome> {
    let mut latency = Vec::new();
    let mut recall5 = Vec::new();
    let mut recall10 = Vec::new();
    let mut reciprocal = Vec::new();
    let mut abstain_counts = Counts::default();
    let mut lines = Vec::new();
    let mut case_details = Vec::new();

    for case in cases {
        let started = Instant::now();
        let results = group
            .runtime
            .advanced_search(&case.query, SEARCH_LIMIT, &SearchOptions::default())
            .await?;
        latency.push(started.elapsed().as_micros());

        let ranked = group.ranked_keys(&results);
        let relevant = metrics::set_of(case.relevant_keys.clone());
        let abstained = results.is_empty();

        abstain_counts.add(Counts {
            true_positives: usize::from(abstained && case.expect_abstain),
            false_positives: usize::from(abstained && !case.expect_abstain),
            false_negatives: usize::from(!abstained && case.expect_abstain),
        });

        if relevant.is_empty() {
            lines.push(format!(
                "{:<22} abstain expected {:<5} observed {:<5} {}",
                case.id,
                case.expect_abstain,
                abstained,
                if abstained == case.expect_abstain {
                    "ok".to_string()
                } else {
                    format!("FALSE ANSWER ({} results)", results.len())
                }
            ));
            case_details.push(json!({
                "id": case.id,
                "query": case.query,
                "expect_abstain": case.expect_abstain,
                "abstained": abstained,
                "returned": results.len(),
                "note": case.note,
            }));
            continue;
        }

        let r5 = metrics::recall_at_k(&ranked, &relevant, 5);
        let r10 = metrics::recall_at_k(&ranked, &relevant, SEARCH_LIMIT);
        let rr = metrics::reciprocal_rank(&ranked, &relevant);
        recall5.push(r5);
        recall10.push(r10);
        reciprocal.push(rr);

        lines.push(format!(
            "{:<22} R@5 {:5.1}%  R@10 {:5.1}%  RR {:.2}  returned {:2}{}",
            case.id,
            r5 * 100.0,
            r10 * 100.0,
            rr,
            results.len(),
            if abstained { "  ABSTAINED" } else { "" }
        ));
        case_details.push(json!({
            "id": case.id,
            "query": case.query,
            "recall_at_5": r5,
            "recall_at_10": r10,
            "reciprocal_rank": rr,
            "returned": results.len(),
            "abstained": abstained,
            "note": case.note,
        }));
    }

    let mean_r5 = metrics::mean(&recall5);
    let mean_r10 = metrics::mean(&recall10);
    let mrr = metrics::mean(&reciprocal);
    let abstain_prf = abstain_counts.prf();

    let detail = json!({
        "mean_recall_at_5": mean_r5,
        "mean_recall_at_10": mean_r10,
        "mean_reciprocal_rank": mrr,
        "abstention_precision": abstain_prf.precision,
        "abstention_recall": abstain_prf.recall,
        "abstention_f1": abstain_prf.f1,
        "answerable_questions": recall10.len(),
        "abstention_questions": cases.len() - recall10.len(),
        "cases": case_details,
    });

    lines.insert(
        0,
        format!(
            "R@5 {:.1}%   R@10 {:.1}%   MRR {:.3}   abstention P {:.1}% / R {:.1}%",
            mean_r5 * 100.0,
            mean_r10 * 100.0,
            mrr,
            abstain_prf.precision * 100.0,
            abstain_prf.recall * 100.0
        ),
    );

    Ok(FamilyOutcome::measured(
        "questions",
        "mean recall@10",
        mean_r10,
        cases.len(),
        latency,
        detail,
        lines,
    ))
}

/// Builds the seed content lookup used to recover cluster membership.
pub fn content_index(seeds: &[&Seed]) -> BTreeMap<String, String> {
    seeds
        .iter()
        .map(|seed| (seed.content.clone(), seed.key.clone()))
        .collect()
}
