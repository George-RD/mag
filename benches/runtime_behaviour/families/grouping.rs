use super::complete_listing;
use super::{COMPACT_MIN_CLUSTER_SIZE, COMPACT_SIMILARITY_THRESHOLD, FamilyOutcome, SeededGroup};
use crate::dataset::{GROUPING_COMPACT_EVENT_TYPE, GroupingCase};
use crate::metrics;
use anyhow::Result;
use mag::memory_core::SearchOptions;
use serde_json::json;
use std::collections::BTreeSet;
use std::time::Instant;

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
    let started = Instant::now();
    let dry = group
        .runtime
        .compact(
            GROUPING_COMPACT_EVENT_TYPE,
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
            GROUPING_COMPACT_EVENT_TYPE,
            COMPACT_SIMILARITY_THRESHOLD,
            COMPACT_MIN_CLUSTER_SIZE,
            false,
        )
        .await?;
    let apply_micros = apply_started.elapsed().as_micros();

    let listed = complete_listing(&group.runtime, &SearchOptions::default()).await?;
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
    let mut singleton_retained = 0usize;
    let mut singletons_left_alone = 0usize;
    let mut singleton_unretained = 0usize;
    for reference in &singletons {
        let key = reference
            .iter()
            .next()
            .expect("singleton reference must contain one key");
        if group.retained_id(key).is_none() {
            singleton_unretained += 1;
            continue;
        }
        singleton_retained += 1;
        if predicted.contains(*reference) {
            singletons_left_alone += 1;
        }
    }

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
            "coverage {:.1}% over {} multi-member labelled cluster(s); {}/{} retained singleton(s) left alone; {} not retained before compaction",
            coverage * 100.0,
            multi_member.len(),
            singletons_left_alone,
            singleton_retained,
            singleton_unretained
        ),
        format!(
            "purity {:.1}%   clusters found {clusters_found}   memories compacted {memories_compacted}",
            purity * 100.0
        ),
    ];
    lines.push(format!(
        "compact parameters: event_type={GROUPING_COMPACT_EVENT_TYPE} similarity_threshold={COMPACT_SIMILARITY_THRESHOLD} min_cluster_size={COMPACT_MIN_CLUSTER_SIZE}"
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
        let singleton_retained = case
            .members
            .first()
            .is_none_or(|key| group.retained_id(key).is_some());
        let verdict = match (case.members.len(), singleton_retained, recovered) {
            (1, false, _) => "NOT RETAINED BEFORE COMPACTION (not scored)",
            (1, true, true) => "left alone (not scored)",
            (1, true, false) => "MERGED AWAY (not scored)",
            (_, _, true) => "recovered",
            (_, _, false) => "SPLIT",
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
        "singleton_clusters_retained": singleton_retained,
        "singleton_clusters_not_retained": singleton_unretained,
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
