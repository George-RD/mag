use super::*;

fn shipped_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/runtime_behaviour_eval/v1")
}

#[test]
fn shipped_dataset_passes_every_check() {
    let (dataset, manifest, sha) = load(&shipped_dir()).expect("dataset loads");
    let checks = validate(&dataset, &manifest, &sha);
    let failures: Vec<&ValidationCheck> = checks.iter().filter(|c| !c.passed).collect();
    assert!(failures.is_empty(), "unexpected failures: {failures:?}");
    assert!((validity_percentage(&checks) - 100.0).abs() < f64::EPSILON);
}

#[test]
fn manifest_sha256_matches_shipped_dataset() {
    let dir = shipped_dir();
    let raw = std::fs::read(dir.join("dataset.json")).expect("dataset bytes");
    let (_, manifest, _) = load(&dir).expect("dataset loads");
    assert_eq!(manifest.sha256, sha256_hex(&raw));
}

#[test]
fn sha256_of_empty_input_is_the_known_digest() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn broken_cross_reference_fails_validation() {
    let dir = shipped_dir();
    let (mut dataset, manifest, sha) = load(&dir).expect("dataset loads");
    dataset.questions[0].relevant_keys = vec!["no-such-seed".to_string()];
    let checks = validate(&dataset, &manifest, &sha);
    let check = checks
        .iter()
        .find(|c| c.name == "cross_references_resolve")
        .expect("check present");
    assert!(!check.passed);
    assert!(check.detail.as_deref().unwrap().contains("no-such-seed"));
}

#[test]
fn mismatched_sha256_fails_validation() {
    let dir = shipped_dir();
    let (dataset, manifest, _) = load(&dir).expect("dataset loads");
    let checks = validate(&dataset, &manifest, "0".repeat(64).as_str());
    let check = checks
        .iter()
        .find(|c| c.name == "manifest_sha256_matches")
        .expect("check present");
    assert!(!check.passed);
    assert!(validity_percentage(&checks) < 100.0);
}
