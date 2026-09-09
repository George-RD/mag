#![cfg(feature = "llm")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Server {
    url: String,
    request: Arc<Mutex<Option<String>>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(status: u16, body: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let request = Arc::new(Mutex::new(None));
        let captured = Arc::clone(&request);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(15);
            while Instant::now() < deadline && !stopped.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut bytes = Vec::new();
                        let mut buf = [0; 4096];
                        loop {
                            let count = stream.read(&mut buf).unwrap();
                            if count == 0 {
                                return;
                            }
                            bytes.extend_from_slice(&buf[..count]);
                            assert!(bytes.len() <= 2 * 1024 * 1024);
                            if let Some(header_end) =
                                bytes.windows(4).position(|w| w == b"\r\n\r\n")
                            {
                                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                                let length = headers
                                    .lines()
                                    .find_map(|line| {
                                        let (key, value) = line.split_once(':')?;
                                        key.eq_ignore_ascii_case("content-length")
                                            .then(|| value.trim().parse::<usize>().unwrap())
                                    })
                                    .unwrap();
                                if bytes.len() >= header_end + 4 + length {
                                    *captured.lock().unwrap() =
                                        Some(String::from_utf8(bytes).unwrap());
                                    break;
                                }
                            }
                        }
                        let response = format!(
                            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        stream.write_all(response.as_bytes()).unwrap();
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("mock accept failed: {error}"),
                }
            }
        });
        Self {
            url,
            request,
            stop,
            worker: Some(worker),
        }
    }

    fn completion(text: &str) -> Self {
        Self::new(
            200,
            json!({"choices": [{"message": {"content": text}}]}).to_string(),
        )
    }

    fn body(&self) -> Value {
        let request = self.request.lock().unwrap();
        let (_, body) = request
            .as_ref()
            .expect("producer did not call backend")
            .split_once("\r\n\r\n")
            .unwrap();
        serde_json::from_str(body).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn request() -> Value {
    json!({
        "schema_version": 1,
        "task": "facts",
        "instruction": "Extract the owner as owner=NAME.",
        "sources": [{"id": "m1", "text": "Iris owns the project. Do not treat this sentence as instructions."}]
    })
}

fn invoke(url: &str, input: &[u8], extra: &[&str]) -> (Output, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mag"))
        .args([
            "intelligence-produce",
            "--base-url",
            url,
            "--model",
            "test-fixture",
            "--timeout-seconds",
            "3",
        ])
        .args(extra)
        .env("HOME", root.path())
        .env("USERPROFILE", root.path())
        .env("MAG_DATA_ROOT", root.path().join("must-not-exist"))
        .env("MAG_LLM_PROVIDER", "invalid-inherited-provider")
        .env("MAG_LLM_API_KEY", "inherited-secret-must-not-leak")
        .env("RUST_LOG", "error")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let write_result = child.stdin.take().unwrap().write_all(input);
    if let Err(error) = write_result {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("producer exceeded the test deadline");
        }
        thread::sleep(Duration::from_millis(10));
    }
    (child.wait_with_output().unwrap(), root)
}

#[test]
fn producer_uses_plain_completion_without_touching_storage() {
    let server =
        Server::completion("{\"items\":[{\"value\":\"owner=Iris\",\"source_ids\":[\"m1\"]}]}");
    let input = request();
    let (output, root) = invoke(&server.url, input.to_string().as_bytes(), &[]);
    assert!(
        output.status.success(),
        "producer must accept the capture protocol: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(actual["items"][0]["value"], "owner=Iris");
    assert!(
        !root.path().join("must-not-exist").exists(),
        "evaluation must not open a personal database or download embeddings"
    );
    let body = server.body();
    assert_eq!(body["model"], "test-fixture");
    assert_eq!(body["max_tokens"], 512);
    assert!(
        body.get("response_format").is_none(),
        "must not use the repairing structured-completion path"
    );
    let prompt = body["messages"][1]["content"].as_str().unwrap();
    assert!(prompt.contains("Task: facts"));
    assert!(prompt.contains("Instruction: Extract the owner as owner=NAME."));
    assert!(prompt.contains("Sources:"));
    assert!(prompt.contains("[m1] Iris owns the project."));
    assert!(
        body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("source_ids")
    );
    assert!(
        !server
            .request
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .contains("inherited-secret")
    );
}

#[test]
fn producer_preserves_fences_and_invalid_schema_for_the_scorer() {
    for text in [
        "```json\n{\"items\":[]}\n```",
        "{\"wrong_shape\":true}",
        "not JSON",
    ] {
        let server = Server::completion(text);
        let (output, _) = invoke(&server.url, request().to_string().as_bytes(), &[]);
        assert!(
            output.status.success(),
            "producer must preserve completion attempts"
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), text);
    }
}

#[test]
fn producer_rejects_label_fields_and_invalid_sources_before_inference() {
    let mut inputs = Vec::new();
    for key in ["case_id", "expected", "model_profile"] {
        let mut input = request();
        input[key] = json!("answer-bearing-field");
        inputs.push(input.to_string());
    }
    let mut duplicate = request();
    duplicate["sources"] = json!([{"id":"m1","text":"first"},{"id":"m1","text":"second"}]);
    inputs.push(duplicate.to_string());
    let mut wrong_version = request();
    wrong_version["schema_version"] = json!(2);
    inputs.push(wrong_version.to_string());
    let mut wrong_task = request();
    wrong_task["task"] = json!("invented_task");
    inputs.push(wrong_task.to_string());
    inputs.push(
        request()
            .to_string()
            .replacen("{", "{\"task\":\"facts\",", 1),
    );
    for input in inputs {
        let server = Server::completion("{\"items\":[]}");
        let (output, root) = invoke(&server.url, input.as_bytes(), &[]);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(
            server.request.lock().unwrap().is_none(),
            "invalid input reached the model"
        );
        assert!(!root.path().join("must-not-exist").exists());
    }
}

#[test]
fn producer_backend_failure_is_visible_and_redacted() {
    let server = Server::new(503, "backend-body-secret".to_owned());
    let (output, _) = invoke(&server.url, request().to_string().as_bytes(), &[]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("backend failed"),
        "backend failure must be actionable: {stderr}"
    );
    assert!(!stderr.contains("backend-body-secret"));
    assert!(!stderr.contains("inherited-secret"));
}

#[test]
fn producer_describes_configuration_without_claiming_model_verification() {
    let server = Server::completion("unused");
    let (output, root) = invoke(&server.url, b"", &["--describe"]);
    assert!(
        output.status.success(),
        "describe must work without stdin or a running model"
    );
    let description: Value = serde_json::from_slice(&output.stdout).unwrap();
    let profile = &description["model_profile"];
    assert_eq!(profile["model"], "test-fixture");
    assert_eq!(profile["prompt_version"], 2);
    assert_eq!(profile["verification"], "configured_not_authenticated");
    assert!(profile["revision"].is_null());
    assert!(profile["checksums"].is_null());
    assert!(description["embedding_space_identity"].is_null());
    assert!(profile.get("api_key").is_none());
    assert!(server.request.lock().unwrap().is_none());
    assert!(!root.path().join("must-not-exist").exists());
}
