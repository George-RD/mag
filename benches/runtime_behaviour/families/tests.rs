use super::*;
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
        runtime
            .store_raw(id, content, &MemoryInput::default())
            .await
            .unwrap();
    }
    let retained_ids = stored_ids(&runtime).await.unwrap();
    assert!(retained_ids.contains(&kept));
    assert!(
        !retained_ids.contains(&discarded),
        "fixture must exercise a real discarded write"
    );
    let group = SeededGroup {
        runtime,
        key_to_id: BTreeMap::from([
            ("kept".into(), kept.clone()),
            ("discarded".into(), discarded.clone()),
        ]),
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
    let error = stored_ids(&runtime)
        .await
        .expect_err("truncated list must not be scored");
    assert!(
        error
            .to_string()
            .contains("incomplete full-table observation")
    );
}

#[tokio::test]
async fn complete_listing_accepts_exact_page_boundary() {
    let (_directory, runtime) = fixture(1000).await;
    assert_eq!(stored_ids(&runtime).await.unwrap().len(), 1000);
}

#[tokio::test]
async fn review_entities_disclose_discards_without_dropping_denominator() {
    let (_directory, group) = discarded_seed_group().await;
    let outcome = entities(
        &group,
        &[EntityCase {
            seed: "discarded".into(),
            expected: vec!["tools:rust".into()],
            note: None,
        }],
    )
    .await
    .unwrap();
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
    let outcome = relationships(
        &group,
        &[RelationshipCase {
            from: "discarded".into(),
            to: "kept".into(),
            rel_type: "any".into(),
            min_weight: 0.0,
            note: None,
        }],
    )
    .await
    .unwrap();
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
    let outcome = provenance(
        &group,
        &[ProvenanceCase {
            operation: "auto_compact".into(),
            expect_source_link_field: "superseded_by_id".into(),
            note: None,
        }],
    )
    .await
    .unwrap();
    assert_eq!(
        outcome.detail["verification_conditions"],
        serde_json::json!([
            "target_exists",
            "target_not_retired",
            "source_hidden_by_default"
        ])
    );
    for row in outcome.detail["retired_rows"].as_array().unwrap() {
        assert!(row.get("readable_with_include_superseded").is_none());
    }
}

async fn assert_relationship_direction(reverse: bool, annotation: &str, expected: f64) {
    let (_directory, runtime) = fixture(2).await;
    let (from, to) = if reverse {
        ("row-1", "row-0")
    } else {
        ("row-0", "row-1")
    };
    runtime
        .add_relationship(from, to, "PRECEDED_BY", 1.0, &serde_json::json!({}))
        .await
        .unwrap();
    let group = SeededGroup {
        runtime,
        key_to_id: BTreeMap::from([
            ("from".into(), "row-0".into()),
            ("to".into(), "row-1".into()),
        ]),
        id_to_key: BTreeMap::from([
            ("row-0".into(), "from".into()),
            ("row-1".into(), "to".into()),
        ]),
        content_to_key: BTreeMap::new(),
        seeded: 2,
        retained: 2,
        retained_ids: ["row-0".to_string(), "row-1".to_string()]
            .into_iter()
            .collect(),
    };
    let outcome = relationships(
        &group,
        &[RelationshipCase {
            from: "from".into(),
            to: "to".into(),
            rel_type: annotation.into(),
            min_weight: 0.5,
            note: None,
        }],
    )
    .await
    .unwrap();
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

#[tokio::test]
async fn closed_schema_preserves_historical_temporal_note() {
    let (_directory, group) = discarded_seed_group().await;
    let case: crate::dataset::TemporalCase = serde_json::from_value(serde_json::json!({
        "id": "annotated-window",
        "query": "Amber telescope",
        "expect_keys": ["kept"],
        "expect_absent_keys": [],
        "note": "Historical temporal limitation must remain visible"
    }))
    .unwrap();
    let outcome = temporal(&group, &[case]).await.unwrap();
    assert_eq!(
        outcome.detail["cases"][0]["note"],
        "Historical temporal limitation must remain visible"
    );
}
