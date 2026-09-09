from pathlib import Path

def edit(name, old, new):
    p = Path(name)
    s = p.read_text()
    assert s.count(old) == 1, (name, s.count(old), old[:80])
    p.write_text(s.replace(old, new))

edit('tests/test_memory_intelligence_local_baseline.py', "        'test_fixture': True,", "        'test_fixture': True,\n        'output_mode': 'json_schema' if '--json-schema' in args else 'unconstrained',\n        'output_schema': {'type': 'object'} if '--json-schema' in args else None,")
anchor = '    def test_checksum_mismatch_fails_before_any_executable(self):\n'
edit('tests/test_memory_intelligence_local_baseline.py', anchor, r'''    def test_schema_flag_is_explicit_for_description_and_every_attempt(self):
        """The same requested mode reaches describe and both answer-blind cases."""
        guarded = PRODUCER.replace("assert args[0] == 'intelligence-produce'",
            "assert args[0] == 'intelligence-produce'\nassert '--json-schema' in args")
        self.mag = self.executable("schema-fixture", guarded)
        run = self.run_baseline(json_schema=True)
        self.assertEqual(run["model_profile"]["producer"]["output_mode"], "json_schema")
        self.assertEqual(self.calls.read_text().splitlines(), ["case", "case"])
        self.assert_server_stopped()

    def test_schema_description_mismatch_fails_before_server_launch(self):
        """Do not silently report constrained execution from an old/ignoring CLI."""
        wrong = PRODUCER.replace("'json_schema' if '--json-schema' in args else 'unconstrained'",
                                 "'unconstrained'")
        self.mag = self.executable("wrong-schema-fixture", wrong)
        with self.assertRaisesRegex(ValueError, "matching bounded MAG producer description"):
            self.run_baseline(json_schema=True)
        self.assertFalse(self.pid.exists())
        self.assertFalse(self.calls.exists())

    def test_schema_mode_requires_a_boolean_before_any_execution(self):
        """Truthiness must not silently switch experimental request semantics."""
        for invalid in ("true", 1, None):
            with self.subTest(invalid=invalid), self.assertRaisesRegex(ValueError, "json_schema must be a boolean"):
                self.run_baseline(json_schema=invalid)
        self.assertFalse(self.pid.exists())
        self.assertFalse(self.calls.exists())

''' + anchor)
p = Path('tests/memory_intelligence_producer_cli.rs')
p.write_text(p.read_text() + r'''
#[test]
fn constrained_request_changes_only_response_format_and_keeps_raw_output() {
    for text in ["not JSON", "```json\n{\"items\":[]}\n```", "{\"wrong_shape\":true}"] {
        let plain = Server::completion(text);
        let constrained = Server::completion(text);
        let input = request().to_string();
        let (plain_output, _) = invoke(&plain.url, input.as_bytes(), &[]);
        let (output, root) = invoke(&constrained.url, input.as_bytes(), &["--json-schema"]);
        assert!(plain_output.status.success());
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(output.stdout, text.as_bytes());
        assert_eq!(output.stdout, plain_output.stdout);
        assert!(!root.path().join("must-not-exist").exists());
        let mut body = constrained.body();
        let format = body.as_object_mut().unwrap().remove("response_format").unwrap();
        assert_eq!(format["type"], "json_schema");
        assert_eq!(format["json_schema"]["strict"], true);
        assert_eq!(format["json_schema"]["schema"], mag::intelligence_output_schema());
        assert_eq!(body, plain.body(), "schema must not change messages, temperature or tokens");
    }
}

#[test]
fn constrained_http_failure_is_not_retried_repaired_or_fallen_back() {
    let server = Server::new(400, "unsupported-schema-secret".to_owned());
    let (output, _) = invoke(&server.url, request().to_string().as_bytes(), &["--json-schema"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("backend failed"));
    assert!(!stderr.contains("unsupported-schema-secret"));
    assert_eq!(server.body()["response_format"]["type"], "json_schema");
}

#[tokio::test]
async fn legacy_structured_provider_keeps_temperature_and_fence_repair() {
    use mag::memory_core::llm::{LlmBackend, LlmConfig, OpenAiProvider};
    let server = Server::completion("```json\n{\"items\":[]}\n```");
    let mut config = LlmConfig::ollama("legacy-fixture", &server.url);
    config.temperature = 0.37;
    let provider = OpenAiProvider::new(config).unwrap();
    let schema = json!({"type": "object"});
    let result = provider.complete_structured("legacy prompt", Some("legacy system"), &schema).await.unwrap();
    assert_eq!(result, json!({"items": []}));
    let body = server.body();
    assert_eq!(body["temperature"], 0.0);
    assert_eq!(body["messages"][1]["content"], "legacy prompt");
    assert_eq!(body["response_format"]["json_schema"]["schema"], schema);
}
''')
edit('tests/memory_intelligence_producer_runtime.rs', '    async fn complete_structured(\n', '''    async fn complete_constrained(
        &self, prompt: &str, system: Option<&str>, schema: &serde_json::Value,
    ) -> Result<String> {
        assert_eq!(*schema, mag::intelligence_output_schema());
        self.complete(prompt, system).await
    }

    async fn complete_structured(
''')
p = Path('tests/memory_intelligence_producer_runtime.rs')
p.write_text(p.read_text() + r'''
#[tokio::test]
async fn constrained_runtime_preserves_raw_output_validation_limits_and_redaction() {
    for text in ["", "not json", "```json\n{\"items\":[]}\n```", " {\"items\":[]}\n"] {
        let backend = backend(text);
        let actual = LocalMemoryRuntime::produce_intelligence_with_schema(&backend, &request()).await.unwrap();
        assert_eq!(actual, text);
        assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    }
    let mut invalid = request();
    invalid.instruction.clear();
    let untouched = backend("unused");
    assert!(LocalMemoryRuntime::produce_intelligence_with_schema(&untouched, &invalid).await.is_err());
    assert_eq!(untouched.calls.load(Ordering::SeqCst), 0);
    let oversized = backend(&"x".repeat(MAX_INTELLIGENCE_BYTES + 1));
    let error = LocalMemoryRuntime::produce_intelligence_with_schema(&oversized, &request()).await.unwrap_err();
    assert!(error.to_string().contains("completion exceeds byte limit"));
    assert_eq!(oversized.calls.load(Ordering::SeqCst), 1);
    let mut failing = backend("unused");
    failing.fail = true;
    let error = LocalMemoryRuntime::produce_intelligence_with_schema(&failing, &request()).await.unwrap_err();
    assert!(error.to_string().contains("backend failed"));
    assert!(!format!("{error:#}").contains("secret"));
    assert_eq!(failing.calls.load(Ordering::SeqCst), 1);
}

struct UnsupportedBackend;

#[async_trait]
impl LlmBackend for UnsupportedBackend {
    async fn complete(&self, _: &str, _: Option<&str>) -> Result<String> {
        panic!("unsupported native constraints must not silently fall back");
    }
}

#[tokio::test]
async fn unsupported_constraints_fail_without_calling_plain_completion() {
    let error = LocalMemoryRuntime::produce_intelligence_with_schema(&UnsupportedBackend, &request()).await.unwrap_err();
    assert!(error.to_string().contains("backend failed"));
}
''')
edit('benches/memory_intelligence/local_baseline.py', 'case_timeout: int = 60, threads: int = 2,', 'case_timeout: int = 60, threads: int = 2, json_schema: bool = False,')
edit('benches/memory_intelligence/local_baseline.py', '    validate_pin(pin)\n', '    if type(json_schema) is not bool:\n        raise ValueError("json_schema must be a boolean")\n    validate_pin(pin)\n')
edit('benches/memory_intelligence/local_baseline.py', '        with tempfile.TemporaryDirectory(prefix="mag-describe-") as cwd:', '        if json_schema:\n            producer.append("--json-schema")\n        with tempfile.TemporaryDirectory(prefix="mag-describe-") as cwd:')
anchor = '                    raise ValueError("producer description did not match the configured generation command")\n'
edit('benches/memory_intelligence/local_baseline.py', anchor, anchor + '''                mode = profile.get("output_mode")
                if json_schema and (mode != "json_schema" or not isinstance(profile.get("output_schema"), dict)):
                    raise ValueError("producer description did not match requested schema mode")
                if not json_schema and mode not in (None, "unconstrained"):
                    raise ValueError("producer description did not match unconstrained mode")
''')
edit('benches/memory_intelligence/local_baseline.py', '    parser.add_argument("--threads", type=int, default=2)\n', '    parser.add_argument("--threads", type=int, default=2)\n    parser.add_argument("--json-schema", action="store_true", help="request native output constraints without repair or fallback")\n')
edit('benches/memory_intelligence/local_baseline.py', 'startup_timeout=args.startup_timeout, case_timeout=args.case_timeout, threads=args.threads)', 'startup_timeout=args.startup_timeout, case_timeout=args.case_timeout, threads=args.threads,\n                           json_schema=args.json_schema)')
edit('.github/workflows/memory-intelligence-eval.yml', '--test memory_intelligence_producer_runtime\n', '--test memory_intelligence_producer_runtime --test memory_intelligence_schema\n')
