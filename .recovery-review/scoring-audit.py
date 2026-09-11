from pathlib import Path
import sys

phase = sys.argv[1]
if phase == 'tests':
    p = Path('benches/runtime_behaviour/families/mod.rs')
    p.write_text(p.read_text() + '\n#[cfg(test)]\nmod tests;\n')
    Path('benches/runtime_behaviour/families/tests.rs').write_text(r'''use super::*;
use crate::dataset::{EntityCase, ProvenanceCase, RelationshipCase};
use mag::memory_core::MemoryInput;
use mag::memory_core::embedder::PlaceholderEmbedder;
use std::sync::Arc;

async fn fixture(rows: usize) -> (tempfile::TempDir, LocalMemoryRuntime) {
    let directory = tempfile::TempDir::new().unwrap();
    let path = directory.path().join("listing.db");
    let runtime = tokio::task::spawn_blocking(move || {
        let runtime = LocalMemoryRuntime::new_with_path(path.clone(), Arc::new(PlaceholderEmbedder)).unwrap();
        let mut connection = rusqlite::Connection::open(path).unwrap();
        let transaction = connection.transaction().unwrap();
        for index in 0..rows {
            let id = format!("row-{index}");
            transaction.execute(
                "INSERT INTO memories(id, content, content_hash, source_type) VALUES(?1, ?1, ?1, 'test-fixture')",
                [&id],
            ).unwrap();
        }
        transaction.commit().unwrap();
        runtime
    }).await.unwrap();
    (directory, runtime)
}

async fn discarded_seed_group() -> (tempfile::TempDir, SeededGroup) {
    let (directory, runtime) = fixture(0).await;
    let kept = uuid::Uuid::new_v4().to_string();
    let discarded = uuid::Uuid::new_v4().to_string();
    let content = "Amber telescope calibration uses Rust";
    for id in [&kept, &discarded] {
        runtime.store_raw(id, content, &MemoryInput::default()).await.unwrap();
    }
    let retained_ids = stored_ids(&runtime).await.unwrap();
    assert!(retained_ids.contains(&kept));
    assert!(!retained_ids.contains(&discarded), "fixture must exercise a real discarded write");
    let group = SeededGroup {
        runtime,
        key_to_id: BTreeMap::from([("kept".into(), kept.clone()), ("discarded".into(), discarded.clone())]),
        id_to_key: BTreeMap::from([(kept, "kept".into()), (discarded, "discarded".into())]),
        content_to_key: BTreeMap::new(),
        seeded: 2,
        retained: retained_ids.len(),
        retained_ids,
    };
    (directory, group)
}

#[tokio::test]
async fn review_rejects_a_truncated_full_table_observation() {
    let (_directory, runtime) = fixture(1001).await;
    let error = stored_ids(&runtime).await.expect_err("truncated list must not be scored");
    assert!(error.to_string().contains("incomplete full-table observation"));
}

#[tokio::test]
async fn complete_listing_accepts_exact_page_boundary() {
    let (_directory, runtime) = fixture(1000).await;
    assert_eq!(stored_ids(&runtime).await.unwrap().len(), 1000);
}

#[tokio::test]
async fn review_entities_disclose_discards_without_dropping_denominator() {
    let (_directory, group) = discarded_seed_group().await;
    let outcome = entities(&group, &[EntityCase {
        seed: "discarded".into(), expected: vec!["tools:rust".into()], note: None,
    }]).await.unwrap();
    assert_eq!(outcome.cases, 1);
    assert_eq!(outcome.score, 0.0);
    assert_eq!(outcome.detail["false_negatives"], 1);
    assert_eq!(outcome.detail["scoring_scope"], "end_to_end");
    assert_eq!(outcome.detail["cases_with_unretained_seed"], 1);
    assert_eq!(outcome.detail["cases"][0]["seed_retained"], false);
}

#[tokio::test]
async fn review_relationships_disclose_discards_without_dropping_denominator() {
    let (_directory, group) = discarded_seed_group().await;
    let outcome = relationships(&group, &[RelationshipCase {
        from: "discarded".into(), to: "kept".into(), rel_type: "any".into(), min_weight: 0.0, note: None,
    }]).await.unwrap();
    assert_eq!(outcome.cases, 1);
    assert_eq!(outcome.score, 0.0);
    assert_eq!(outcome.detail["annotated_edges"], 1);
    assert_eq!(outcome.detail["scoring_scope"], "end_to_end");
    assert_eq!(outcome.detail["cases_with_unretained_endpoint"], 1);
    assert_eq!(outcome.detail["cases"][0]["endpoints_retained"], false);
}

#[tokio::test]
async fn review_provenance_declares_only_falsifiable_conditions() {
    let (_directory, group) = discarded_seed_group().await;
    let outcome = provenance(&group, &[ProvenanceCase {
        operation: "auto_compact".into(), expect_source_link_field: "superseded_by_id".into(), note: None,
    }]).await.unwrap();
    assert_eq!(outcome.detail["verification_conditions"], serde_json::json!([
        "target_exists", "target_not_retired", "source_hidden_by_default"
    ]));
    for row in outcome.detail["retired_rows"].as_array().unwrap() {
        assert!(row.get("readable_with_include_superseded").is_none());
    }
}

async fn assert_relationship_direction(reverse: bool, annotation: &str, expected: f64) {
    let (_directory, runtime) = fixture(2).await;
    let (from, to) = if reverse { ("row-1", "row-0") } else { ("row-0", "row-1") };
    runtime.add_relationship(from, to, "PRECEDED_BY", 1.0, &serde_json::json!({})).await.unwrap();
    let group = SeededGroup {
        runtime,
        key_to_id: BTreeMap::from([("from".into(), "row-0".into()), ("to".into(), "row-1".into())]),
        id_to_key: BTreeMap::from([("row-0".into(), "from".into()), ("row-1".into(), "to".into())]),
        content_to_key: BTreeMap::new(), seeded: 2, retained: 2,
        retained_ids: ["row-0".to_string(), "row-1".to_string()].into_iter().collect(),
    };
    let outcome = relationships(&group, &[RelationshipCase {
        from: "from".into(), to: "to".into(), rel_type: annotation.into(), min_weight: 0.5, note: None,
    }]).await.unwrap();
    assert_eq!(outcome.score, expected);
}

#[tokio::test]
async fn review_typed_relationship_does_not_credit_reversed_edge() {
    assert_relationship_direction(true, "PRECEDED_BY", 0.0).await;
}

#[tokio::test]
async fn typed_relationship_credits_forward_edge() {
    assert_relationship_direction(false, "PRECEDED_BY", 1.0).await;
}

#[tokio::test]
async fn any_relationship_allows_either_direction() {
    assert_relationship_direction(true, "any", 1.0).await;
}
''')
elif phase == 'apply':
    p = Path('benches/runtime_behaviour/families/mod.rs')
    text = p.read_text().replace('use mag::memory_core::{SearchOptions, SemanticResult};', 'use mag::memory_core::{ListResult, SearchOptions, SemanticResult};')
    needle = '/// Whether a family produced a score.'
    text = text.replace(needle, '/// Maximum rows accepted by a full-table diagnostic observation.\nconst LIST_PAGE_SIZE: usize = 1000;\n\n' + needle)
    needle = '    /// Maps a ranked result list to dataset seed keys, dropping unknown ids.'
    helper = '''    /// The generated id only if this seed actually became a stored row.
    fn retained_id(&self, key: &str) -> Option<&String> {
        self.key_to_id.get(key).filter(|id| self.retained_ids.contains(*id))
    }

'''
    assert text.count(needle) == 1
    text = text.replace(needle, helper + needle)
    needle = '/// Every stored id in a database, superseded rows included.'
    helper = '''/// Reject a partial snapshot rather than score a silently truncated database.
async fn complete_listing(runtime: &LocalMemoryRuntime, options: &SearchOptions) -> Result<ListResult> {
    let listed = runtime.list(0, LIST_PAGE_SIZE, options).await?;
    anyhow::ensure!(
        listed.total == listed.memories.len(),
        "incomplete full-table observation: received {} of {} rows (limit {LIST_PAGE_SIZE})",
        listed.memories.len(), listed.total,
    );
    Ok(listed)
}

'''
    text = text.replace(needle, helper + needle)
    text = text.replace('let listed = runtime.list(0, 1000, &options).await?;', 'let listed = complete_listing(runtime, &options).await?;')
    p.write_text(text)
    for name in ['read.rs', 'grouping.rs', 'provenance.rs']:
        p = Path('benches/runtime_behaviour/families') / name
        text = 'use super::complete_listing;\n' + p.read_text()
        text = text.replace('group.runtime.list(0, 1000, &options).await?', 'complete_listing(&group.runtime, &options).await?')
        text = text.replace('group\n        .runtime\n        .list(0, 1000, &SearchOptions::default())\n        .await?', 'complete_listing(&group.runtime, &SearchOptions::default()).await?')
        assert '.list(0, 1000' not in text
        p.write_text(text)
    p = Path('benches/runtime_behaviour/families/read.rs')
    text = p.read_text()
    text = text.replace('/// annotated entity sets.', '/// annotated entity sets. This is end-to-end storage/retrieval evidence, not\n/// isolated extractor accuracy: discarded inputs remain in the denominator.')
    needle = '    for case in cases {\n        let expected = metrics::set_of(case.expected.clone());'
    assert text.count(needle) == 1
    text = text.replace(needle, '''    let mut unretained = 0usize;
    for case in cases {
        let seed_retained = group.retained_id(&case.seed).is_some();
        if !seed_retained {
            unretained += 1;
            lines.push(format!("{}: seed not retained; kept in end-to-end denominator", case.seed));
        }
        let expected = metrics::set_of(case.expected.clone());''')
    text = text.replace('"seed": case.seed,\n            "expected":', '"seed": case.seed,\n            "seed_retained": seed_retained,\n            "expected":', 1)
    text = text.replace('"micro_precision": micro_prf.precision,', '"scoring_scope": "end_to_end",\n        "cases_with_unretained_seed": unretained,\n        "micro_precision": micro_prf.precision,', 1)
    text = text.replace('/// observed edge-type histogram.', '/// observed edge-type histogram. A discarded endpoint remains a failed\n/// end-to-end case, with its retention failure identified separately.')
    needle = '    let mut seen_edges: BTreeSet<String> = BTreeSet::new();'
    text = text.replace(needle, needle + '\n    let mut unretained = 0usize;')
    text = text.replace('group.key_to_id.get(&case.from),\n            group.key_to_id.get(&case.to),', 'group.retained_id(&case.from),\n            group.retained_id(&case.to),')
    needle = '''        ) else {
            case_details.push(json!({
                "from": case.from,'''
    assert text.count(needle) == 1
    text = text.replace(needle, '''        ) else {
            unretained += 1;
            lines.push(format!("{} -> {}: endpoint not retained; kept in end-to-end denominator", case.from, case.to));
            case_details.push(json!({
                "endpoints_retained": false,
                "from": case.from,''')
    text = text.replace('"note": "seed was not stored",', '"note": "endpoint not retained after seeding",')
    text = text.replace('"found": matched.is_some(),', '"endpoints_retained": true,\n            "found": matched.is_some(),', 1)
    old = '''            let connects = (edge.source_id == *from_id && edge.target_id == *to_id)
                || (edge.source_id == *to_id && edge.target_id == *from_id);'''
    new = '''            let connects = (edge.source_id == *from_id && edge.target_id == *to_id)
                || (case.rel_type == "any" && edge.source_id == *to_id && edge.target_id == *from_id);'''
    assert text.count(old) == 1
    text = text.replace(old, new)
    text = text.replace('"annotated_edges": cases.len(),', '"scoring_scope": "end_to_end",\n        "cases_with_unretained_endpoint": unretained,\n        "annotated_edges": cases.len(),', 1)
    p.write_text(text)
    p = Path('benches/runtime_behaviour/families/provenance.rs')
    text = p.read_text()
    text = text.replace('during the call this scores four conditions:', 'during the call this scores three conditions:')
    text = text.replace('/// `list()`, and the retired row is still readable with `include_superseded`.', '/// `list()`. Source readability is how links are discovered, not an independent\n/// scored condition. Links on deleted source rows cannot enter this observation.')
    text = text.replace('        let readable_when_included = after_ids.contains(*id);\n', '')
    text = text.replace('target_survived && hidden_by_default && readable_when_included', 'target_survived && hidden_by_default')
    text = text.replace(' readable_with_include_superseded={readable_when_included}', '')
    text = text.replace('            "readable_with_include_superseded": readable_when_included,\n', '')
    text = text.replace('"link_integrity": serde_json::Value::Null,', '"verification_conditions": ["target_exists", "target_not_retired", "source_hidden_by_default"],\n        "link_integrity": serde_json::Value::Null,')
    p.write_text(text)
    p = Path('benches/runtime_behaviour/README.md')
    p.write_text(p.read_text() + '''

## Scoring boundaries

Entity and relationship scores remain end-to-end observations over every annotated
case. A discarded input is not silently removed from either denominator. Per-case
retention flags and discarded-case counts distinguish ingestion loss from missing
entity tags or links on retained rows; these are not isolated extractor or graph
algorithm accuracy scores. Lifecycle is different: it conditions expiry on an
actually stored row, so an absent write cannot become evidence of successful expiry.

Every full-table observation checks the runtime's total against the returned rows.
More than 1000 rows, or any other partial listing, aborts the diagnostic rather than
publishing a truncated score. This bound applies after compaction too.

Provenance verifies three conditions on discoverable new links: target existence,
target survival and default-list source hiding. Source readability is a discovery
precondition, not a fourth independently verified property. Deleted source rows
are outside this link-only measure. This correction removes the redundant detail
field without changing the link-integrity value or the preserved historical JSON.
Explicit relationship types require the annotated from-to direction; only `any`
allows either direction. This does not change production relationship semantics.
''')
else:
    raise SystemExit('use tests or apply')
