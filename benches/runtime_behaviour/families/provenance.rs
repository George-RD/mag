use super::{AUTO_COMPACT_COUNT_THRESHOLD, FamilyOutcome, SeededGroup, stored_ids};
use crate::dataset::ProvenanceCase;
use crate::metrics;
use anyhow::Result;
use mag::memory_core::SearchOptions;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

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
