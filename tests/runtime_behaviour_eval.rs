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
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("one JSON document on stdout")
}

fn edited_dataset(edit: impl FnOnce(&mut Value, &mut Value)) -> tempfile::TempDir {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/runtime_behaviour_eval/v1");
    let mut data: Value =
        serde_json::from_slice(&std::fs::read(source.join("dataset.json")).unwrap()).unwrap();
    let mut manifest: Value =
        serde_json::from_slice(&std::fs::read(source.join("manifest.json")).unwrap()).unwrap();
    edit(&mut data, &mut manifest);
    let bytes = serde_json::to_vec(&data).unwrap();
    manifest["sha256"] = json!(format!("{:x}", Sha256::digest(&bytes)));
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("dataset.json"), bytes).unwrap();
    std::fs::write(
        temp.path().join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    temp
}

fn mutated_validation(edit: impl FnOnce(&mut Value)) -> Output {
    let temporary = edited_dataset(|data, _| edit(data));
    run(&[
        "--dataset",
        temporary.path().to_str().unwrap(),
        "--validate-only",
    ])
}

#[test]
fn preserves_original_dataset_identity_and_sanitized_path() {
    let output = document(run(&["--validate-only", "--json"]));
    assert_eq!(output["dataset_version"], "v1");
    assert_eq!(output["schema_validity_percentage"], 100.0);
    assert!(
        output["dataset_sha256"]
            .as_str()
            .unwrap()
            .starts_with("3260e0a00beb")
    );
    assert_eq!(output["metadata"]["dataset_path"], "dataset.json");
    assert_eq!(output["metadata"]["dataset_source"], "repo-local");
}

#[test]
fn rejects_unknown_schema_even_when_manifest_agrees() {
    let temp = edited_dataset(|data, manifest| {
        data["schema_version"] = json!(99);
        manifest["schema_version"] = json!(99);
    });
    let result = run(&[
        "--dataset",
        temp.path().to_str().unwrap(),
        "--validate-only",
        "--json",
    ]);
    assert!(!result.status.success());
}

#[test]
fn rejects_unknown_manifest_filename() {
    let temp = edited_dataset(|_, manifest| manifest["dataset_file"] = json!("other.json"));
    assert!(
        !run(&[
            "--dataset",
            temp.path().to_str().unwrap(),
            "--validate-only"
        ])
        .status
        .success()
    );
}

#[test]
fn rejects_annotation_in_an_unobserved_group() {
    let temp = edited_dataset(|data, _| data["seed"][0]["group"] = json!("not-evaluated"));
    assert!(
        !run(&[
            "--dataset",
            temp.path().to_str().unwrap(),
            "--validate-only"
        ])
        .status
        .success()
    );
}

#[test]
fn rejects_unrepresentable_day_offsets() {
    let temp = edited_dataset(|data, _| data["seed"][0]["day_offset"] = json!(i64::MAX));
    assert!(
        !run(&[
            "--dataset",
            temp.path().to_str().unwrap(),
            "--validate-only"
        ])
        .status
        .success()
    );
}

#[test]
fn rejects_changed_dataset_bytes() {
    let temp = edited_dataset(|_, _| {});
    let path = temp.path().join("dataset.json");
    let mut data = std::fs::read(&path).unwrap();
    data.push(b' ');
    std::fs::write(path, data).unwrap();
    assert!(
        !run(&[
            "--dataset",
            temp.path().to_str().unwrap(),
            "--validate-only"
        ])
        .status
        .success()
    );
}

#[test]
fn reports_custom_dataset_source_without_disclosing_local_path() {
    let temp = edited_dataset(|_, _| {});
    let result = document(run(&[
        "--dataset",
        temp.path().to_str().unwrap(),
        "--validate-only",
        "--json",
    ]));
    assert_eq!(result["metadata"]["dataset_path"], "dataset.json");
    assert_eq!(result["metadata"]["dataset_source"], "user-supplied");
    assert!(!result.to_string().contains(temp.path().to_str().unwrap()));
}

#[test]
fn placeholder_reports_all_families_without_claiming_model_quality() {
    let result = document(run(&["--embedder", "placeholder", "--json"]));
    let families = result["families"].as_object().unwrap();
    assert_eq!(families.len(), 8);
    for name in [
        "entities",
        "temporal",
        "relationships",
        "lifecycle",
        "supersession",
        "grouping",
        "provenance",
        "questions",
    ] {
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
    assert!(
        result["seeded_memories"].as_u64().unwrap()
            >= result["retained_memories"].as_u64().unwrap()
    );
    assert!(result["families"]["entities"]["detail"].is_object());
}

#[test]
fn family_selection_and_unknown_names_are_explicit() {
    let result = document(run(&[
        "--embedder",
        "placeholder",
        "--family",
        "entities",
        "--json",
    ]));
    assert_eq!(result["families"].as_object().unwrap().len(), 1);
    assert!(
        !run(&["--embedder", "placeholder", "--family", "unknown"])
            .status
            .success()
    );
}

#[test]
fn archived_runtime_observation_retains_exact_source_and_dataset() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(
        root.join("benches/runtime_behaviour/baselines/2026-09-10-bge-small/run.json"),
    )
    .expect("recorded runtime observation must be retained");
    assert_eq!(
        format!("{:x}", Sha256::digest(&bytes)),
        "3f21b208374e6543287723ca90a1ce9b3e31d316e06a7d3540d002da35a3e7c6"
    );
    let observation: Value = serde_json::from_slice(&bytes).unwrap();
    let dataset = std::fs::read(root.join("data/runtime_behaviour_eval/v1/dataset.json")).unwrap();
    assert_eq!(
        observation["dataset_sha256"],
        format!("{:x}", Sha256::digest(dataset))
    );
    assert_eq!(
        observation["metadata"]["commit"],
        "66796b3995766ead2429e56ccdc3d4ba2568a1be"
    );
    assert_eq!(observation["families"].as_object().unwrap().len(), 8);
    assert_eq!(observation["seeded_memories"], 36);
    assert_eq!(observation["retained_memories"], 34);
    assert_eq!(observation["model_profile"]["output_dimensions"], 384);
    assert!(observation["tokens"].is_null());
    assert!(observation.get("overall_percentage").is_none());
}

fn review_private_relative_command(equals_form: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temporary = tempfile::tempdir().unwrap();
    let private = temporary.path().join("customer_secret_case_7391");
    std::fs::create_dir(&private).unwrap();
    for filename in ["dataset.json", "manifest.json"] {
        std::fs::copy(
            root.join("data/runtime_behaviour_eval/v1").join(filename),
            private.join(filename),
        )
        .unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_memory_runtime_eval"));
    command
        .current_dir(temporary.path())
        .args(["--validate-only", "--json"]);
    if equals_form {
        command.arg("--dataset=customer_secret_case_7391");
    } else {
        command.args(["--dataset", "customer_secret_case_7391"]);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        !text.contains("customer_secret_case_7391"),
        "metadata leaked a private relative dataset directory"
    );
    let result: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(result["metadata"]["dataset_source"], "user-supplied");
    assert!(
        result["metadata"]["command"]
            .as_str()
            .unwrap()
            .contains("--validate-only")
    );
}

#[test]
fn review_redacts_relative_dataset_split_option() {
    review_private_relative_command(false);
}

#[test]
fn review_redacts_relative_dataset_equals_option() {
    review_private_relative_command(true);
}

#[test]
fn review_rejects_abstention_with_positive_references() {
    let data = serde_json::from_slice::<Value>(include_bytes!(
        "../data/runtime_behaviour_eval/v1/dataset.json"
    ))
    .unwrap();
    let id = data["questions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| !c["relevant_keys"].as_array().unwrap().is_empty())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let out = mutated_validation(|data| {
        let case = data["questions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|c| c["id"] == id)
            .unwrap();
        case["expect_abstain"] = Value::Bool(true);
    });
    assert!(
        !out.status.success(),
        "contradictory abstention annotation was accepted"
    );
}

#[test]
fn review_rejects_answerable_question_without_references() {
    let out = mutated_validation(|data| {
        let case = &mut data["questions"][0];
        case["expect_abstain"] = Value::Bool(false);
        case["relevant_keys"] = serde_json::json!([]);
    });
    assert!(
        !out.status.success(),
        "answerable question with no reference was accepted"
    );
}
