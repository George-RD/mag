---
name: mag-development
description: Build, run, test, benchmark, and debug the MAG Rust memory server in clean, ephemeral, VM, CI, or local-model environments. Use for MAG repository work, first-run model downloads, MCP stdio testing, retrieval changes, local LLM configuration, and benchmark-gated pull requests.
compatibility: Requires Rust/Cargo. Model tests may require outbound network access on first use. Git and Python 3 are useful for benchmark harnesses.
metadata:
  project: mag
  scope: repository
---

# MAG development

Read the root `AGENTS.md` first. It is the progressive-disclosure router and
repository-invariant guide. This skill contains operational details for clean
environments, model startup, MCP testing, and retrieval validation.

## Clean-environment baseline

```bash
cargo build --release
export MAG_DATA_ROOT="$(mktemp -d)/.mag"
export RUST_LOG=warn

target/release/mag doctor || true
target/release/mag ingest 'MAG clean-environment smoke memory'
target/release/mag search 'clean environment'
target/release/mag doctor
```

The first `doctor` may report missing embedding artifacts. Normal ingest/search
commands can download the model and tokenizer on first use, so rerun `doctor`
after one real command before concluding the runtime is broken.

## CLI versus MCP server

- CLI commands initialize SQLite and the embedder directly. No daemon is needed.
- `mag serve` is an MCP **stdio** server, not a background system daemon.
- A client must keep the child process alive and speak JSON-RPC over stdin/stdout.
- Never write logs to stdout in server mode; stdout is the protocol channel.
- Use `HOME`, `USERPROFILE`, and `MAG_DATA_ROOT` isolation for hermetic tests.

## Product-level smoke test

Do not stop at the unit suite. Store several distinct memories, then check:

1. exact recall;
2. paraphrase recall with no shared words;
3. unrelated-query abstention;
4. persistence across processes;
5. MCP `initialize`, `tools/list`, store, and search;
6. duplicate/conflicting memory handling;
7. cold-start and warm latency.

For retrieval changes, run:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
./scripts/bench.sh --gate
bash scripts/smoke-test.sh
```

Any change under `src/memory_core/storage/sqlite/pipeline/` is retrieval logic and
must trigger the benchmark gate.

## Experimental local generative backend

The optional `llm` module contains HTTP backend clients and configuration, but no
production CLI or MCP caller currently constructs it. `MAG_LLM_*` variables only
affect experiments or future code that explicitly loads `LlmConfig`; exporting
them does not activate generative memory behaviour today.

The local reference profile for bounded backend experiments is:

- model: `LiquidAI/lfm2.5-1.2b-instruct`
- endpoint: `http://localhost:11434/v1`
- direct ONNX candidate: `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`

```bash
ollama pull LiquidAI/lfm2.5-1.2b-instruct
export MAG_LLM_PROVIDER=ollama
```

Do not describe this as production wiring or in-process ONNX. MAG's embeddings
already use ONNX; experimental causal generation crosses the `LlmBackend` HTTP
boundary. Production wiring follows the selected local runtime and evaluation
harness; direct ONNX generation remains a roadmap candidate.

The 350M model is an experimental future speed tier only. Establish task-level
eval parity with 1.2B before routing extraction, relationship creation, grouping,
or consolidation work to it.

## Common failure interpretations

- Missing model reported before first use: run one real command, then diagnose.
- Exact search works but paraphrase fails: inspect advanced-search explain output,
  candidate vector scores, reranking, and abstention independently.
- Server appears to exit: verify the MCP client is holding stdin open.
- Tests contaminate user data: ensure all three isolation variables are set.
- First call is slow: separate model download, model warmup, and steady-state time.
