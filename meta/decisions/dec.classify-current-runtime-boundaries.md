---
id: dec.classify-current-runtime-boundaries
nodes:
  - mag.runtime.entrypoints
  - mag.runtime.setup
  - mag.runtime.mcp
  - mag.runtime.memory.domain
  - mag.runtime.memory.models
  - mag.runtime.memory.storage.sqlite
  - mag.runtime.substrate
  - mag.runtime.daemon
status: accepted
date: 2026-07-29
revisit_triggers:
  - "The production composition-root decision is accepted"
  - "A production entrypoint constructs substrate or an LLM backend"
  - "An HTTP MCP server is assembled and verified end to end"
  - "The legacy CLI Pipeline is removed or changes stored-content behaviour"
informed_by: [res.architecture-state-audit]
refines: [dec.baseline-current-module-boundaries, dec.evidence-based-cleanup]
related: [dec.sequence-architecture-before-llm-wiring]
---
# Classify the current runtime boundaries by actual construction

MAG will describe runtime maturity from current production construction rather
than feature names, compilation, tests, or historical plans.

- `SqliteStorage` is the current production semantic centre for MCP and most CLI
  capabilities.
- `memory_core::Pipeline` is a compatibility-sensitive CLI adapter, not the
  universal composition root.
- `substrate` is the single candidate orchestration path and is currently used
  only by tests and benchmarks.
- the `llm` boundary is experimental infrastructure with no production caller;
- `daemon-http` contains support primitives but no assembled HTTP server;
- command transport (`mag serve`) is the only setup transport currently verified
  end to end.

This classification does not choose the future composition root. It prevents
new intelligence from being duplicated across current paths, prevents incomplete
features from being presented as live, and preserves local stdio operation as
the mandatory independent baseline.
