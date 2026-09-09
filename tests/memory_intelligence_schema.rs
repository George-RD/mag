#![cfg(feature = "llm")]

use std::process::{Command, Stdio};

use serde_json::Value;

fn describe(constrained: bool) -> Value {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_mag"));
    command.args([
        "intelligence-produce",
        "--describe",
        "--base-url",
        "http://127.0.0.1:1/v1",
    ]);
    if constrained {
        command.arg("--json-schema");
    }
    let output = command
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("MAG_DATA_ROOT", root.path().join("must-not-exist"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!root.path().join("must-not-exist").exists());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn schema_mode_is_explicit_and_described_without_model_access() {
    let description = describe(true);
    let profile = &description["model_profile"];
    assert_eq!(profile["output_mode"], "json_schema");
    assert_eq!(profile["prompt_version"], 1);
    assert_eq!(profile["verification"], "configured_not_authenticated");
    let schema = &profile["output_schema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["required"], serde_json::json!(["items"]));
    assert_eq!(schema["properties"]["items"]["type"], "array");
    let item = &schema["properties"]["items"]["items"];
    assert_eq!(item["additionalProperties"], false);
    assert_eq!(item["properties"]["value"]["type"], "string");
    assert_eq!(item["properties"]["source_ids"]["type"], "array");
    assert!(
        !schema.to_string().contains("enum"),
        "schema must not encode answers"
    );
}

#[test]
fn default_mode_stays_unconstrained() {
    let description = describe(false);
    assert_eq!(description["model_profile"]["output_mode"], "unconstrained");
    assert!(description["model_profile"]["output_schema"].is_null());
}
