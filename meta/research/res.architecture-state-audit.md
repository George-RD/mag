---
id: res.architecture-state-audit
nodes:
  - mag.runtime
  - mag.runtime.entrypoints
  - mag.runtime.setup
  - mag.runtime.mcp
  - mag.runtime.memory.domain
  - mag.runtime.memory.models
  - mag.runtime.memory.storage.sqlite
  - mag.runtime.substrate
  - mag.runtime.daemon
  - mag.quality.tests
  - mag.quality.benchmarks
sources: [src.mag-agents-guide, src.dead-code-recon, src.source-tree-recon]
date: 2026-07-29
baseline_commit: 9eba6157a225f244b94eefcf83e883c1309d30ec
---
# Current architecture and cleanup evidence

## Scope and method

This audit traces the current production constructors, feature gates, direct
callers, tests, and benchmarks from the repository state after PR #374. A path
is classified as **production-wired** only when a user-facing binary entrypoint
constructs or calls it. Compilation, unit tests, integration tests, and
benchmarks are recorded separately; they do not by themselves make a path live.

The earlier dead-code and source-tree reports remain useful historical inputs,
but they predate the current module split and cannot establish present reachability.
The current risk is not proven bulk dead code. It is three competing shapes:

1. direct `SqliteStorage` calls for most CLI and all MCP semantics;
2. the legacy `memory_core::Pipeline` wrapper used by a subset of CLI commands;
3. the feature-gated `substrate` orchestration used by tests and benchmarks only.

## Findings

### 1. SQLite is the current production semantic centre

The binary constructs one `SqliteStorage` from the selected embedder and clones
it into the other live surfaces. MCP constructs `McpMemoryServer` directly over
that storage and serves stdio. Most extended CLI commands also call
`SqliteStorage` capability traits directly. The advanced retrieval, graph,
lifecycle, backup, profile, reminder, checkpoint, and maintenance behaviour is
therefore owned by the SQLite backend today.

This is an observation about the current implementation, not a decision that
storage should permanently own orchestration. New work must first choose a
composition root rather than adding another direct path.

### 2. `memory_core::Pipeline` is a live CLI adapter, not the universal root

The binary creates `Pipeline` with `PlaceholderPipeline` as both ingestor and
processor, then clones `SqliteStorage` into storage and search roles. `ingest`
and `process` call `Pipeline::run`; basic retrieve/search/recent operations also
use the wrapper. MCP does not. Most advanced CLI operations do not.

`PlaceholderPipeline::process` prefixes content with `processed: `. That visible
stored-content behaviour is a compatibility surface. Removing or bypassing the
legacy pipeline requires regression tests and a migration decision; it is not a
safe dead-code deletion.

### 3. `substrate` is a tested candidate architecture, not production runtime

The `substrate` module is compiled only with the `substrate` feature. Repository
callers are confined to the module itself, `tests/substrate_pipeline.rs`,
`benches/substrate_bench.rs`, and `benches/phase2_bench.rs`. Neither the CLI nor
MCP constructs `SearchPipeline`, `WritePipeline`, or `ConsolidationRunner`.

The integration tests prove an FTS search composition and precomputed-embedding
write path. The benchmarks measure synthetic substrate search latency and a
mock-LLM metadata experiment. They do not prove production parity with the
SQLite advanced-search path, public CLI/MCP compatibility, migration safety, or
real-model resource use.

### 4. The LLM boundary is implemented but not production-wired

`memory_core::llm` defines providers and local defaults behind the `llm` feature.
Its only non-test consumer is `substrate::EmbedAndExtractPipeline`, itself not
constructed by a production entrypoint. `phase2_bench` uses a deterministic mock
backend. Setting `MAG_LLM_*` variables therefore does not currently change CLI or
MCP behaviour.

Documentation must describe this as an experimental backend boundary, not an
active optional production mode, until the composition decision and local eval
gate are complete.

### 5. `daemon-http` contains support primitives but no HTTP server

The feature gates `daemon.rs`, `auth.rs`, and `idle_timer.rs`. These provide
metadata, token/auth middleware, and idle-lifecycle primitives with unit tests.
No production code constructs `AuthLayer` or `IdleTimerLayer`, no code writes a
live `DaemonInfo`, and no HTTP router or listener composes MCP semantics.

Setup can still generate an HTTP MCP URL. Its daemon check only reads stale
metadata and prints a suggestion to run `mag serve`; `mag serve` is explicitly a
stdio server. The separately advertised `stdio` setup mode writes
`["serve", "--stdio"]`, but the `serve` command has no `--stdio` argument.
Only command transport (`mag serve`) is currently end-to-end valid.

This is an incomplete experimental deployment surface, not a working optional
cloud adapter and not a dependency of local operation.

## Current production composition

| Surface | Feature/runtime gate | Constructed path | Current semantic owner | Classification |
|---|---|---|---|---|
| CLI ingest/process | default binary | `Pipeline(PlaceholderPipeline, PlaceholderPipeline, SqliteStorage...)` | legacy pipeline plus SQLite | Production-wired; compatibility-sensitive |
| Basic CLI retrieve/search/recent | default binary | `memory_core::Pipeline` delegating to SQLite roles | SQLite through adapter | Production-wired |
| Extended CLI operations | default binary | direct `SqliteStorage` capability calls | SQLite | Production-wired |
| MCP | default binary, stdio | `McpMemoryServer<SqliteStorage>::serve_stdio()` | SQLite plus MCP validation | Production-wired |
| Embeddings | default `real-embeddings` feature | `OnnxEmbedder`; placeholder when feature disabled | model adapter plus SQLite | Production-wired with explicit fallback build |
| Cross-encoder | `serve --cross-encoder` and `real-embeddings` | reranker attached to SQLite | SQLite retrieval path | Optional production path |
| Substrate orchestration | `substrate` | tests and benchmarks only | candidate | Experimental, not production-wired |
| LLM extraction/generation | `llm` plus substrate consumer | unit tests and mock benchmark | candidate | Experimental, not production-wired |
| HTTP daemon | `daemon-http` | metadata/auth/idle primitives only | none assembled | Incomplete, not production-wired |
| In-memory storage | library/test construction | parity and test callers | reference backend | Test/reference path |
| Connector setup | setup command | tool detection/config writer/plugin hooks | setup/integrations | Production-wired; command transport valid |

## Cleanup and consolidation evidence

| Candidate | Current callers | Feature | Test evidence | Benchmark evidence | Audit disposition |
|---|---|---|---|---|---|
| Consolidate `memory_core::Pipeline` into the selected composition root | CLI ingest/process and basic retrieval in `main.rs` | default | pipeline unit tests, CLI tests, smoke coverage | no dedicated behavioural-parity benchmark | Do not delete. First pin current CLI behaviour, especially `processed: `, then migrate one command family at a time. |
| Promote or fold `substrate` | integration tests and two benchmark binaries; no production caller | `substrate` | `tests/substrate_pipeline.rs` plus module tests | `substrate_bench`, `phase2_bench` | Decide whether its boundaries replace thin adapters or are folded into the SQLite-centred path. Do not maintain a parallel product pipeline. |
| Remove LLM provider infrastructure | substrate extraction and tests only | `llm` | provider/extraction unit tests | `phase2_bench` uses a mock backend | Keep as a reversible experimental boundary until the composition/eval decision. Stop documenting it as active production behaviour. |
| Remove daemon/auth/idle modules | setup reads `DaemonInfo`; no assembled server | `daemon-http` | module unit tests | none | Do not advertise as working. Either implement behind an explicit service milestone or remove after the cross-device design decision. |
| Remove `PlaceholderEmbedder` | non-`real-embeddings` builds, tests, substrate benchmark | feature fallback | broad unit/integration use | substrate synthetic benchmark | Keep as explicit test/fallback implementation; never treat its output as production quality. |
| Remove in-memory backend | tests/reference callers | default library surface | backend parity and unit tests | none material | Keep as reference backend unless parity coverage is replaced. |
| Bulk-delete feature-gated or `allow(dead_code)` code | varies | varies | varies | varies | Rejected. Feature gating, test-only use, and benchmark use must be considered individually. |

## Stale or misleading documents

- `docs/strongholds/recon-dead-code.md` and `tech-debt-recon.md` are historical
  snapshots, not current reachability evidence.
- `docs/configuration.md` describes `MAG_LLM_*` as though it activates an
  optional production mode; no production caller currently reads the backend.
- Setup/CLI documentation that advertises `http` or `stdio` setup transports is
  ahead of the executable surface. Command transport is the only verified mode.
- The Cairn blueprint previously stated that entrypoints start an HTTP adapter
  and that the daemon serves MCP over HTTP. Those edges are not implemented.

## Resulting boundaries

1. Treat `SqliteStorage` as the **current** production semantic centre while the
   composition decision remains open.
2. Treat `memory_core::Pipeline` as a compatibility-sensitive CLI adapter, not a
   target for new intelligence.
3. Treat `substrate` as the one candidate orchestration source. If selected,
   migrate live call sites into it; if not, fold only its useful interfaces into
   the existing path and retire the duplicate.
4. Treat `llm` and `daemon-http` as experimental, non-production-wired features.
5. Keep local stdio operation independent of any daemon, hosted service, or
   generative model.
6. Correct broken setup transports before expanding cloud/cross-device work.

## Next decisions and checks

- Select the production composition root using CLI/MCP parity, retrieval quality,
  latency, memory use, migration cost, and reversibility as decision criteria.
- Add regression tests for setup-generated command lines before disabling or
  implementing the invalid stdio/HTTP modes.
- Build the local intelligence eval harness before production LLM wiring.
- Rationalize stale stronghold and configuration documents after the composition
  decision establishes the durable vocabulary.
