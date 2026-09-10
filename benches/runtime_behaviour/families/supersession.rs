use super::{FamilyOutcome, SeededGroup, stored_ids};
use crate::dataset::SupersessionCase;
use crate::metrics::Counts;
use anyhow::Result;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Instant;

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
