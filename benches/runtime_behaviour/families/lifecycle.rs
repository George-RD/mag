use super::{FamilyOutcome, SeededGroup, stored_ids};
use crate::dataset::LifecycleCase;
use crate::metrics;
use anyhow::Result;
use serde_json::json;
use std::time::Instant;

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
