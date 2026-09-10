use super::*;

const ENTITY_CATEGORIES: [&str; 3] = ["people:", "tools:", "projects:"];

/// Runs every schema-validity check. A failing check is fatal to the caller.
pub fn validate(
    dataset: &Dataset,
    manifest: &Manifest,
    dataset_sha256: &str,
) -> Vec<ValidationCheck> {
    let mut extra = validate_supported_contract(dataset, manifest);
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

    checks.append(&mut extra);
    checks
}

fn validate_supported_contract(data: &Dataset, manifest: &Manifest) -> Vec<ValidationCheck> {
    let mut failures = Vec::new();
    if data.schema_version != 1
        || manifest.schema_version != 1
        || manifest.dataset_file != "dataset.json"
    {
        failures
            .push("only schema_version 1 and dataset_file dataset.json are supported".to_string());
    }
    if data.dataset_version.trim().is_empty() || data.seed.is_empty() {
        failures.push("dataset version and seeds must be non-empty".to_string());
    }
    let by_key: BTreeMap<&str, &Seed> = data.seed.iter().map(|s| (s.key.as_str(), s)).collect();
    let mut covered = BTreeSet::new();
    let mut partition = |key: &str, group: &str| {
        if let Some(seed) = by_key.get(key) {
            covered.insert(key.to_string());
            if seed.group != group {
                failures.push(format!(
                    "seed {key} belongs to {}, not observed partition {group}",
                    seed.group
                ));
            }
        }
    };
    for c in &data.entities {
        partition(&c.seed, "corpus");
    }
    for c in &data.temporal {
        for k in c.expect_keys.iter().chain(&c.expect_absent_keys) {
            partition(k, "corpus");
        }
    }
    for c in &data.relationships {
        partition(&c.from, "corpus");
        partition(&c.to, "corpus");
    }
    for c in &data.questions {
        for k in &c.relevant_keys {
            partition(k, "corpus");
        }
    }
    for c in &data.lifecycle {
        partition(&c.seed, "lifecycle");
    }
    for c in &data.grouping {
        for k in &c.members {
            partition(k, "grouping");
        }
    }
    let mut supersession_keys = BTreeSet::new();
    for c in &data.supersession {
        if let (Some(old), Some(new)) = (by_key.get(c.old.as_str()), by_key.get(c.new.as_str())) {
            covered.insert(c.old.clone());
            covered.insert(c.new.clone());
            if old.group != new.group
                || c.old == c.new
                || !supersession_keys.insert(c.old.clone())
                || !supersession_keys.insert(c.new.clone())
            {
                failures.push(format!(
                    "supersession pair {} / {} is not isolated",
                    c.old, c.new
                ));
            }
            let old_pos = data.seed.iter().position(|s| s.key == c.old);
            let new_pos = data.seed.iter().position(|s| s.key == c.new);
            if old_pos >= new_pos {
                failures.push("supersession old seed must precede new seed".to_string());
            }
        }
    }
    let today = chrono::Local::now().date_naive();
    for seed in &data.seed {
        if seed.group != "provenance" && !covered.contains(&seed.key) {
            failures.push(format!("seed {} has no observing family", seed.key));
        }
        if let Some(offset) = seed.day_offset
            && chrono::Duration::try_days(offset)
                .and_then(|d| today.checked_add_signed(d))
                .is_none()
        {
            failures.push(format!(
                "seed {} has an unrepresentable day offset",
                seed.key
            ));
        }
    }
    for c in &data.provenance {
        if c.operation != "auto_compact" || c.expect_source_link_field != "superseded_by_id" {
            failures.push("provenance supports only auto_compact / superseded_by_id".to_string());
        }
    }
    for (name, count) in [
        ("entities", data.entities.len()),
        ("temporal", data.temporal.len()),
        ("relationships", data.relationships.len()),
        ("lifecycle", data.lifecycle.len()),
        ("supersession", data.supersession.len()),
        ("grouping", data.grouping.len()),
        ("provenance", data.provenance.len()),
        ("questions", data.questions.len()),
    ] {
        if count == 0 {
            failures.push(format!("{name} has no annotations"));
        }
    }
    vec![ValidationCheck::from_failures(
        "supported_observation_contract",
        failures,
    )]
}
