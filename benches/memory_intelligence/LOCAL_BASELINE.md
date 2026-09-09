# Pinned local-model baseline

This runner connects the existing `mag intelligence-produce` CLI to an owned
local llama.cpp server. Python supervises the experiment; all generation still
uses MAG's typed runtime. It does not enable generation during ingestion, change
retrieval, or introduce MCP memory semantics.

## Run the experiment

The opt-in **Local memory intelligence baseline** GitHub Actions workflow builds
MAG with `--locked --release --features llm`, builds the pinned CPU-only server,
downloads the pinned GGUF, captures every development case once, then scores the
recorded artifact independently. Invoke it manually at the revision being tested.
It uploads raw attempts, scorecard, pin, CPU details, and toolchain versions.
Ordinary PR CI never downloads the model.

For a local run, use Python 3.10 or newer on POSIX, a trusted MAG binary built
with `llm`, and a trusted llama.cpp server built from the revision recorded in
`local-baseline.pin.json`. Obtain the model from the exact model revision in that
file, and review its linked licence. The runner does not download files or infer
model identity from a filename. Pass absolute executable and model paths:

```bash
cargo build --locked --release --features llm

python3 benches/memory_intelligence/local_baseline.py \
  benches/memory_intelligence/dataset.v1.json \
  --pin benches/memory_intelligence/local-baseline.pin.json \
  --mag "$PWD/target/release/mag" \
  --server /absolute/path/to/llama-server \
  --model /absolute/path/to/LFM2.5-1.2B-Instruct-Q4_K_M.gguf \
  --code-revision "$(git rev-parse HEAD)" \
  --startup-timeout 180 --case-timeout 90 --threads 2 \
  --output run.json

python3 benches/memory_intelligence/evaluate.py score \
  benches/memory_intelligence/dataset.v1.json run.json \
  --output scorecard.json
```

Use a clean checkout and binaries built from it. `--code-revision` and the server
source revision are caller declarations, not automatically verified build
attestations. The workflow records both the checked-out source and observed
binary hashes. The model pin records a Q4_K_M LFM2.5-1.2B-Instruct artifact; its
presence is a reproducibility choice, not evidence that this model meets a
production quality threshold.

The output path cannot alias the dataset, pin, either executable, or the model,
including symlink and hardlink aliases. A successful capture exits 0 even when
individual cases fail: inspect the scorecard. Setup or integrity failures exit 2
without replacing an existing artifact. Do not treat an absent artifact as a
zero-quality model result; it is an unsuccessful experiment setup.

## Integrity and process boundaries

The model is copied into a private temporary directory, checked against the pin,
and made read-only before any executable starts. The runner checks model and
binary hashes again after the experiment. It preserves the actual `--describe`
producer profile verbatim, including `configured_not_authenticated` and its null
provenance fields. Verified staged-file evidence is separate under
`model_profile.local_artifact`; it does not authenticate an arbitrary process's
loaded weights. The host and both executables must be trusted. This is not a
security sandbox or a cryptographic execution attestation.

The server binds loopback, uses a unique per-launch model alias, and excludes
inherited `LLAMA_*` settings. Probes ignore proxies and redirects; the MAG
producer explicitly bypasses proxies for loopback. The unique alias catches
accidental port reuse, not malicious local impersonation. Health probes use the
same bounded process supervisor as capture, so a slow-drip HTTP response cannot
extend the whole startup deadline. All owned process groups are cleaned up on
normal return, Python exceptions, and timeouts. Operating-system termination of
the orchestrator itself is outside that Python cleanup guarantee.

Each request contains only task, instruction, and source memories. Expected
annotations and case IDs are not sent. The server remains warm between cases;
MAG starts a fresh producer per case. No attempt is retried or repaired. The
server uses context 4096, one slot, CPU only, seed 42, temperature 0.1, top-k 50,
and repeat penalty 1.05. MAG's configured generation settings remain the
producer snapshot; caller arguments and observed hardware are also recorded.
Other inherited environment settings and platform differences can affect
results. An exact seed is not a cross-hardware determinism guarantee.

## Interpret the measurements

`server_ready_ms` is process launch through successful health and model-alias
probes. It includes server initialization, default warmup, polling, and probe
startup. It excludes download and checksum/copy staging. Staging reads the model
before launch and can warm the filesystem cache: this is **not a cold-cache
start**. Isolated `load_time_ms` remains null.

Per-case latency is the existing capture bridge's wall time, including MAG
startup, HTTP, parsing, and cleanup. It is not model-only inference latency.
A warm persistent server can reuse prompt cache; this is not a cold per-case
benchmark. Tokens and derived tokens/second are not estimated.

`peak_ram_bytes` is sampled Linux server-process VmHWM, in bytes, including a
final sample before cleanup. It excludes MAG, descendants, GPU memory, and
whole-host memory. It is an observed high-water mark, not a process-tree total.
The sample interval and successful sample count are disclosed; unsupported or
unavailable observation is null, not zero. macOS capture remains supported but
this Linux-specific RAM observer does not report a macOS value.

Profile digests include the exact runtime snapshot, including the random alias
and ephemeral endpoint. Across runs, compare the stable model checksum, declared
source revisions, generation settings, dataset digest, hardware, and methodology;
do not expect those exact-snapshot digests to remain identical.

The 14-case synthetic development seed is not held out. Exact canonical-label
and source-set scores are not factual-entailment judgments or broad evidence of
quality. Preserve all raw attempts and report per-task failures before using this
experiment to choose the next improvement. Neither a green workflow nor high
seed accuracy justifies changing the production model by itself.
