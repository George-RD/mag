---
node: mag.runtime
status: done
created: 2026-07-28
completed: 2026-07-29
satisfies: current-runtime-boundary-audit
---
# Audit Current Architecture And Dead Code

The current production constructors, feature gates, direct callers, tests, and
benchmarks are recorded in `res.architecture-state-audit` against immutable
commit `9eba6157a225f244b94eefcf83e883c1309d30ec`.

## Acceptance

- [x] Legacy pipeline, substrate, daemon, connector, benchmark, and fallback paths
  are classified as production-wired, experimental, test/reference, or incomplete.
- [x] Every proposed deletion or consolidation records current callers, feature
  gates, test evidence, benchmark evidence, and a disposition.
- [x] Semantic duplication and unsafe bulk-deletion assumptions are identified.
- [x] Stale and misleading documents are identified for the follow-up
  rationalization task.
- [x] The Cairn blueprint and runtime contracts reflect the audited boundaries.

## Result

SQLite is the current production semantic centre. The legacy Pipeline remains a
compatibility-sensitive CLI adapter. Substrate and LLM orchestration are
feature-gated test/benchmark candidates, and the daemon feature contains support
primitives without an assembled HTTP server. No bulk code deletion is justified
before the production composition-root decision and parity evidence.
