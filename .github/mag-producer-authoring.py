"""Temporary PR-443 authoring patch; removed before the final merge gate."""
from pathlib import Path


def replace(path: str, old: str, new: str) -> None:
    file = Path(path)
    text = file.read_text()
    if text.count(old) != 1:
        raise RuntimeError(f"{path}: expected exactly one patch anchor, found {text.count(old)}")
    file.write_text(text.replace(old, new, 1))


def append(path: str, heading: str, body: str) -> None:
    file = Path(path)
    text = file.read_text()
    if heading in text:
        raise RuntimeError(f"{path}: section already exists")
    file.write_text(text.rstrip() + "\n\n" + heading + "\n\n" + body.strip() + "\n")


replace("src/local_memory_runtime.rs", "use std::path::{Path, PathBuf};", '#[cfg(feature = "llm")]\npub mod intelligence;\n\nuse std::path::{Path, PathBuf};')
append("src/lib.rs", '#[cfg(feature = "llm")]', """
pub use local_memory_runtime::intelligence::{
    INTELLIGENCE_PROMPT_VERSION, IntelligenceRequest, IntelligenceSource,
    IntelligenceTask, MAX_INTELLIGENCE_BYTES,
};
""")
args = '''/// Explicit settings for the opt-in, non-persisting evaluation producer.
#[cfg(feature = "llm")]
#[derive(Args)]
pub struct IntelligenceProducerArgs {
    /// OpenAI-compatible endpoint. MAG_LLM_* environment settings are not loaded.
    #[arg(long, default_value = mag::memory_core::llm::DEFAULT_LOCAL_LLM_BASE_URL)]
    pub base_url: String,
    /// Model identifier configured on the endpoint, not an authenticated artifact ID.
    #[arg(long, default_value = mag::memory_core::llm::DEFAULT_LOCAL_LLM_MODEL)]
    pub model: String,
    /// HTTP timeout; use capture.py for a whole-process deadline.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(1..=600))]
    pub timeout_seconds: u64,
    /// Maximum completion tokens requested from the server.
    #[arg(long, default_value_t = 512, value_parser = clap::value_parser!(u32).range(1..=16384))]
    pub max_tokens: u32,
    /// Describe configured, unverified settings without stdin or model access.
    #[arg(long)]
    pub describe: bool,
}

'''
anchor = "/// Available subcommands for the memory pipeline.\n#[derive(Subcommand)]\npub enum Commands {"
replace("src/cli.rs", anchor, args + anchor + '''
    /// Produces one answer-blind intelligence evaluation attempt from stdin.
    #[cfg(feature = "llm")]
    IntelligenceProduce(IntelligenceProducerArgs),''')
replace("src/main.rs", "mod cli;", 'mod cli;\n#[cfg(feature = "llm")]\nmod intelligence_cli;')
replace("src/main.rs", "    let cli = Cli::parse();", '''    let cli = Cli::parse();
    #[cfg(feature = "llm")]
    if let Commands::IntelligenceProduce(args) = &cli.command {
        return intelligence_cli::run(args).await;
    }''')
replace("src/main.rs", "        Commands::Paths => {", '''        #[cfg(feature = "llm")]
        Commands::IntelligenceProduce(_) => {
            unreachable!("IntelligenceProduce is handled before storage initialization");
        }
        Commands::Paths => {''')
replace("cairn.blueprint", '            path "./src/local_memory_runtime.rs"', '            path "./src/local_memory_runtime.rs"\n            path "./src/local_memory_runtime"\n            path "./src/intelligence_cli.rs"')
replace("cairn.blueprint", "Constructs the production embedding adapter", "Constructs production embedding and opt-in evaluation generation adapters")
replace("meta/contracts/memory.models.md", '''`memory_core::llm` is feature-gated
infrastructure consumed by substrate experiments, tests, and a mock benchmark;
no production CLI or MCP entrypoint currently constructs an LLM backend.
Environment defaults must not be documented as active product behaviour until
the selected composition root wires them behind evaluation gates.''', '''`memory_core::llm` is feature-gated
infrastructure used by the opt-in, non-persisting `intelligence-produce` CLI
workflow as well as retained experiments and tests. Ordinary ingestion, search,
and MCP do not construct a generation backend. The evaluation command uses
explicit flags over local defaults, not inherited `MAG_LLM_*` configuration.
Its profile describes configured settings, not authenticated model artifacts or
a measured local-model baseline.''')
append("meta/contracts/memory.models.md", "## Evaluation generation boundary", '''
The selected runtime calls plain `LlmBackend::complete` once per request, never
the repairing structured-completion path. Malformed output remains an attempt
for the independent scorer. The existing HTTP provider trims surrounding
whitespace; this transformation is disclosed in `--describe` metadata. Backend
errors crossing this boundary are redacted, without retries or fallback.

The request and returned completion each have a one-MiB application limit.
The existing HTTP adapter buffers its response before the completion limit is
checked: this is a trusted-endpoint evaluation path, not HTTP memory isolation.
Capture owns the whole-process deadline and stream quotas. No artifact revision,
checksum, quantization, licence, token count, load time, or peak RAM is inferred
from an endpoint's model name. Configured profiles do not satisfy the production
model-verification contract above.
''')
append("meta/contracts/runtime.entrypoints.md", "## Non-persisting intelligence evaluation", '''
With the `llm` feature, `mag intelligence-produce` is a thin stdio/configuration
adapter over `LocalMemoryRuntime::produce_intelligence`. It dispatches before
storage or embedding initialization. One strict, answer-blind protocol-v1
request carries only task, instruction, and immutable source IDs/text; unknown
and duplicate JSON fields, invalid versions/tasks, blank inputs, duplicate
source IDs, and oversized requests fail before inference.

The runtime owns validation and one plain completion attempt. It does not
persist, repair, parse the output schema, retry, or supply expected annotations.
The independent evaluation scorer owns output validity and quality judgment.
`--describe` reads neither stdin nor a model and labels its settings
`configured_not_authenticated`, with absent artifact and embedding identity
fields explicitly null. This opt-in workflow does not add ingestion-time
memory generation or a second MCP implementation.
''')
replace("benches/memory_intelligence/README.md", '''The harness does not own model loading or memory semantics. A production MAG
producer adapter and a measured local-model baseline remain outstanding.''', '''The harness does not own model loading or memory semantics. The opt-in MAG
runtime producer is available through `mag intelligence-produce`; a measured
local-model baseline and authenticated generation artifacts remain outstanding.''')
append("benches/memory_intelligence/README.md", "## Selected-runtime producer", '''
Build MAG with the `llm` feature (included in the default build), then inspect
the explicit settings without opening a database or contacting a model:

```bash
cargo build --release
./target/release/mag intelligence-produce --describe
```

Use the absolute path to that binary as the capture producer. Keep
`--producer` last because it consumes the remaining arguments:

```bash
python3 benches/memory_intelligence/capture.py \\
  benches/memory_intelligence/dataset.v1.json \\
  --metadata /tmp/mag-run-metadata.json \\
  --output /tmp/mag-captured-run.json \\
  --timeout-seconds 60 \\
  --producer /absolute/path/to/mag intelligence-produce \\
  --base-url http://localhost:11434/v1 \\
  --model LiquidAI/lfm2.5-1.2b-instruct --timeout-seconds 50
```

The endpoint must already serve an OpenAI-compatible chat-completion API.
This command does not download or verify a generation model, load embeddings,
open SQLite, inherit `MAG_LLM_*` configuration, retry, or repair malformed
completions. It makes one plain completion call per request. The existing HTTP
adapter trims surrounding whitespace; fences and incorrect schemas remain
incorrect and receive no producer-side salvage. Use only a trusted endpoint;
the application completion-size check is not a bounded HTTP-body reader.

`--describe` emits the `model_profile` and null `embedding_space_identity`
fragments, not a complete capture metadata file. Add the producing binary's
actual source revision and honest measurement context using the metadata
contract above. `configured_not_authenticated` does not prove that the server
loaded the named model. Unknown revision, checksums, quantization, and licence
stay null. Do not present absent token, load-time, or RAM measurements as zero.
The hermetic mock-server tests prove wiring, not model quality.
''')
replace("meta/todos/build-local-memory-intelligence-eval-harness.md", '''Implement a compatible producer through the selected CLI-first MAG runtime and
capture its actual profile/resource observations, then record a real local-model
baseline.''', '''The selected CLI-first runtime producer is implemented in PR #443. Capture its
actual authenticated profile/resource observations and record a real local-model
baseline before completing this todo.''')
append("meta/todos/build-local-memory-intelligence-eval-harness.md", "## Selected-runtime producer slice (PR #443)", '''
`mag intelligence-produce` exposes the typed, non-persisting runtime workflow
without storage or embedding initialization. Strict answer-blind requests call
the existing plain generation backend once; malformed responses are not repaired.
`--describe` reports configured, unauthenticated settings with absent provenance
explicitly null. Runtime and real-process mock-HTTP tests cover this boundary.

The test-only commit preceded implementation and requires the missing CLI
command. Exact red/green CI and review evidence belongs in the PR and its review
record. Source ownership is extended under `mag.runtime.entrypoints`; generation
wiring updates the existing model dependency rather than introducing a parallel
runtime. Authoring uses remote Rust verification because no local Rust/Cairn
execution is available. The ordinary exact-head CI and Cairn gates remain
mandatory; this section is not itself a claim that those gates have passed.

The parent remains in progress. This slice does not measure a real local model,
authenticate server-side model artifacts, or enable generation during ingestion.
''')
