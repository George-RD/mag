from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


setup_path = Path("src/setup.rs")
setup = setup_path.read_text()

old_transport_tests = '''    #[test]
    fn parse_transport_http() {
        let mode = parse_transport("http", 9090).unwrap();
        assert_eq!(mode, TransportMode::Http { port: 9090 });
    }

    #[test]
    fn parse_transport_stdio() {
        let mode = parse_transport("stdio", 4242).unwrap();
        assert_eq!(mode, TransportMode::Stdio);
    }

    #[test]
    fn parse_transport_case_insensitive() {
        let mode = parse_transport("HTTP", 8080).unwrap();
        assert_eq!(mode, TransportMode::Http { port: 8080 });
    }
'''
new_transport_tests = '''    #[test]
    fn parse_transport_http_reports_unavailable() {
        let error = parse_transport("http", 9090).unwrap_err().to_string();
        assert!(
            error.contains("not available") && error.contains("--transport command"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parse_transport_stdio_reports_unavailable() {
        let error = parse_transport("stdio", 4242).unwrap_err().to_string();
        assert!(
            error.contains("not available") && error.contains("--transport command"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parse_transport_command_is_case_insensitive() {
        let mode = parse_transport("COMMAND", 8080).unwrap();
        assert_eq!(mode, TransportMode::Command);
    }
'''
setup = replace_once(setup, old_transport_tests, new_transport_tests, "transport parser tests")

configure_anchor = '''    #[test]
    fn configure_tools_writes_config() {
'''
configure_regression = '''    #[test]
    fn configure_tools_rejects_unavailable_transport_before_writing() {
        with_temp_home(|home| {
            let config_path = home.join(".cursor/mcp.json");
            std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
            let initial = r#"{"mcpServers":{"existing":{"command":"existing"}}}"#;
            std::fs::write(&config_path, initial).unwrap();

            let dt = DetectedTool {
                tool: AiTool::Cursor,
                config_path: config_path.clone(),
                scope: ConfigScope::Global,
                mag_status: MagConfigStatus::NotConfigured,
            };
            let tools: Vec<&DetectedTool> = vec![&dt];

            let error = configure_tools(&tools, TransportMode::Stdio, &tools)
                .unwrap_err()
                .to_string();

            assert!(
                error.contains("not available") && error.contains("--transport command"),
                "unexpected error: {error}"
            );
            assert_eq!(std::fs::read_to_string(config_path).unwrap(), initial);
        });
    }

    #[test]
    fn command_transport_generates_current_serve_invocation() {
        let entry = config_writer::build_mag_entry(AiTool::Cursor, TransportMode::Command);
        assert_eq!(entry["args"], serde_json::json!(["serve"]));
        assert!(entry.get("url").is_none());
    }

'''
setup = replace_once(
    setup,
    configure_anchor,
    configure_regression + configure_anchor,
    "configure regression insertion",
)
setup_path.write_text(setup)

todo_path = Path("meta/todos/correct-setup-transport-surface.md")
todo = todo_path.read_text()
todo = replace_once(todo, "status: open\n", "status: in_progress\n", "todo status")
todo_path.write_text(todo)

Path("scripts/apply_setup_transport_tests.py").unlink()
Path(".github/workflows/apply-setup-transport-tests.yml").unlink()
