//! Process-level contract for the recovered, non-generative runtime diagnostic.
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_memory_runtime_eval"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run diagnostic")
}

fn document(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).expect("one JSON document on stdout")
}

fn edited_dataset(edit: impl FnOnce(&mut Value, &mut Value)) -> tempfile::TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/runtime_behaviour_eval/v1");
    let mut data: Value = serde_json::from_slice(&std::fs::read(source.join("dataset.json")).unwrap()).unwrap();
    let mut manifest: Value = serde_json::from_slice(&std::fs::read(source.join("manifest.json")).unwrap()).unwrap();
    edit(&mut data, &mut manifest);
    let bytes = serde_json::to_vec(&data).unwrap();
    manifest["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("dataset.json"), bytes).unwrap();
    std::fs::write(temp.path().join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
    temp
}

#[test]
fn preserves_original_dataset_identity_and_declared_path() {
    let output = document(run(&["--validate-only", "--json"]));
    assert_eq!(output["dataset_version"], "v1");
    assert_eq!(output["schema_validity_percentage"], 100.0);
    assert!(output["dataset_sha256"].as_str().unwrap().starts_with("3260e0a00beb"));
    assert!(output["metadata"]["dataset_path"].as_str().unwrap().ends_with("data/runtime_behaviour_eval/v1/dataset.json"));
}

#[test]
fn rejects_unknown_schema_even_when_manifest_agrees() {
    let temp = edited_dataset(|data, manifest| {
        data["schema_version"] = json!(99);
        manifest["schema_version"] = json!(99);
    });
    let result = run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only", "--json"]);
    assert!(!result.status.success());
}

#[test]
fn rejects_unknown_manifest_filename() {
    let temp = edited_dataset(|_, manifest| manifest["dataset_file"] = json!("other.json"));
    assert!(!run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only"]).status.success());
}

#[test]
fn rejects_annotation_in_an_unobserved_group() {
    let temp = edited_dataset(|data, _| data["seed"][0]["group"] = json!("not-evaluated"));
    assert!(!run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only"]).status.success());
}

#[test]
fn rejects_unrepresentable_day_offsets() {
    let temp = edited_dataset(|data, _| data["seed"][0]["day_offset"] = json!(i64::MAX));
    assert!(!run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only"]).status.success());
}

#[test]
fn rejects_changed_dataset_bytes() {
    let temp = edited_dataset(|_, _| {});
    let path = temp.path().join("dataset.json");
    let mut data = std::fs::read(&path).unwrap();
    data.push(b' ');
    std::fs::write(path, data).unwrap();
    assert!(!run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only"]).status.success());
}

#[test]
fn reports_actual_custom_dataset_path() {
    let temp = edited_dataset(|_, _| {});
    let result = document(run(&["--dataset", temp.path().to_str().unwrap(), "--validate-only", "--json"]));
    assert_eq!(result["metadata"]["dataset_path"], temp.path().join("dataset.json").to_str().unwrap());
}

#[test]
fn placeholder_reports_all_families_without_claiming_model_quality() {
    let result = document(run(&["--embedder", "placeholder", "--json"]));
    let families = result["families"].as_object().unwrap();
    assert_eq!(families.len(), 8);
    for name in ["entities", "temporal", "relationships", "lifecycle", "supersession", "grouping", "provenance", "questions"] {
        assert!(families.contains_key(name), "missing {name}");
        let family = &families[name];
        if family["status"] == "not_measurable" {
            assert!(family["score_percentage"].is_null());
            assert!(family["p50_latency_ms"].is_null());
        }
    }
    assert!(result["model_profile"].is_null());
    assert!(result["tokens"].is_null());
    assert_eq!(result["embedding_dimension"], 32);
    assert!(result.get("overall_grade").is_none());
    assert!(result.get("overall_percentage").is_none());
    assert!(result["seeded_memories"].as_u64().unwrap() >= result["retained_memories"].as_u64().unwrap());
    assert!(result["families"]["entities"]["detail"].is_object());
}

#[test]
fn family_selection_and_unknown_names_are_explicit() {
    let result = document(run(&["--embedder", "placeholder", "--family", "entities", "--json"]));
    assert_eq!(result["families"].as_object().unwrap().len(), 1);
    assert!(!run(&["--embedder", "placeholder", "--family", "unknown"]).status.success());
}
