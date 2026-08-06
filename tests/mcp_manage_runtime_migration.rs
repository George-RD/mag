use std::{fs, time::Duration};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};
use tokio::{process::Command, time::timeout};

fn arguments(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    value
        .as_object()
        .expect("tool arguments should be an object")
        .clone()
}

fn text_contents(result: &rmcp::model::CallToolResult) -> Vec<String> {
    result
        .content
        .iter()
        .filter_map(|content| content.as_text().map(|text| text.text.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_mode_routes_legacy_and_unified_manage_tools_through_one_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let test_home = std::env::temp_dir().join(format!("mag-mcp-manage-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&test_home)?;

    let mut service = ()
        .serve(TokioChildProcess::new(
            Command::new(env!("CARGO_BIN_EXE_mag")).configure(|cmd| {
                cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
                cmd.arg("serve").arg("--mcp-tools").arg("full");
                cmd.env("HOME", &test_home);
                cmd.env("USERPROFILE", &test_home);
            }),
        )?)
        .await?;

    let tools = timeout(
        Duration::from_secs(20),
        service.list_tools(Default::default()),
    )
    .await??;
    let tool_names: Vec<String> = tools
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    assert_eq!(tool_names.len(), 19);
    for required in [
        "memory_manage",
        "memory_update",
        "memory_feedback",
        "memory_relations",
        "memory_lifecycle",
    ] {
        assert!(
            tool_names.iter().any(|name| name == required),
            "missing expected tool {required}: {tool_names:?}"
        );
    }

    for (id, content) in [
        ("stdio-manage-source", "stdio source memory"),
        ("stdio-manage-target", "stdio target memory"),
    ] {
        let stored = timeout(
            Duration::from_secs(20),
            service.call_tool(CallToolRequestParams {
                meta: None,
                name: "memory_store".into(),
                arguments: Some(arguments(serde_json::json!({
                    "id": id,
                    "content": content
                }))),
                task: None,
            }),
        )
        .await??;
        assert_eq!(text_contents(&stored), vec![format!(r#"{{"id":"{id}"}}"#)]);
    }

    let updated = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_update".into(),
            arguments: Some(arguments(serde_json::json!({
                "id": "stdio-manage-source",
                "content": "stdio source updated",
                "tags": ["runtime"]
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&updated),
        vec![r#"{"id":"stdio-manage-source","updated":true}"#]
    );

    let feedback = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_feedback".into(),
            arguments: Some(arguments(serde_json::json!({
                "memory_id": "stdio-manage-source",
                "rating": "helpful",
                "reason": "stdio parity"
            }))),
            task: None,
        }),
    )
    .await??;
    let feedback_payload: serde_json::Value = serde_json::from_str(&text_contents(&feedback)[0])?;
    assert_eq!(feedback_payload["feedback"]["rating"], "helpful");
    assert_eq!(feedback_payload["feedback"]["new_score"], 1);

    let added = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_relations".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "add",
                "source_id": "stdio-manage-source",
                "target_id": "stdio-manage-target",
                "rel_type": "supports",
                "weight": 0.7
            }))),
            task: None,
        }),
    )
    .await??;
    let added_payload: serde_json::Value = serde_json::from_str(&text_contents(&added)[0])?;
    assert_eq!(added_payload["source_id"], "stdio-manage-source");
    assert_eq!(added_payload["target_id"], "stdio-manage-target");

    let listed = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_relations".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "list",
                "id": "stdio-manage-source"
            }))),
            task: None,
        }),
    )
    .await??;
    let listed_payload: serde_json::Value = serde_json::from_str(&text_contents(&listed)[0])?;
    let relationships = listed_payload["relationships"]
        .as_array()
        .expect("relationships should be an array");
    let supports = relationships
        .iter()
        .find(|relationship| relationship["rel_type"] == "supports")
        .expect("added supports relationship should be visible over stdio");
    assert_eq!(supports["target_id"], "stdio-manage-target");

    let swept = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_lifecycle".into(),
            arguments: Some(arguments(serde_json::json!({"action": "sweep"}))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(text_contents(&swept), vec![r#"{"swept_count":0}"#]);

    let facade_update = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_manage".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "update",
                "id": "stdio-manage-target",
                "content": "stdio target updated"
            }))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&facade_update),
        vec![r#"{"id":"stdio-manage-target","updated":true}"#]
    );

    let retrieved = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_retrieve".into(),
            arguments: Some(arguments(serde_json::json!({"id": "stdio-manage-target"}))),
            task: None,
        }),
    )
    .await??;
    assert_eq!(
        text_contents(&retrieved),
        vec![r#"{"content":"stdio target updated","id":"stdio-manage-target"}"#]
    );

    let missing_relation_source = timeout(
        Duration::from_secs(20),
        service.call_tool(CallToolRequestParams {
            meta: None,
            name: "memory_manage".into(),
            arguments: Some(arguments(serde_json::json!({
                "action": "relations",
                "relations_action": "add"
            }))),
            task: None,
        }),
    )
    .await?
    .expect_err("missing unified relation source should remain invalid params");
    assert!(
        format!("{missing_relation_source:?}")
            .contains("source_id is required for relations_action=add"),
        "unexpected unified relation error: {missing_relation_source:?}"
    );

    let shutdown = timeout(
        Duration::from_secs(20),
        service.close_with_timeout(Duration::from_secs(5)),
    )
    .await?;
    assert!(shutdown?.is_some());
    let _ = fs::remove_dir_all(&test_home);
    Ok(())
}
