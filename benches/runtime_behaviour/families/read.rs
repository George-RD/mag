use super::{FamilyOutcome, SEARCH_LIMIT, SeededGroup};
use crate::dataset::{EntityCase, QuestionCase, RelationshipCase, TemporalCase};
use crate::metrics::{self, Counts};
use anyhow::Result;
use mag::memory_core::SearchOptions;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

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
