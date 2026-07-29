---
node: mag.runtime.substrate
status: blocked
created: 2026-07-29
---
# Retire legacy and unselected orchestration surfaces

Blocked until all production CLI and MCP callers use the selected local runtime
and the compatibility period in `dec.select-local-runtime-composition-root` has
elapsed.

Before removal:

- move any proven substrate implementation still used by tests or benchmarks to
  the selected runtime vocabulary;
- prove there is no production caller, benchmark gate, or evaluation harness that
  requires the duplicate substrate types;
- remove the legacy `memory_core::Pipeline` construction only after its
  `processed: ` compatibility behaviour has an explicit migration outcome;
- deprecate public surfaces for one released compatibility period before removal;
- run public CLI/MCP parity, full repository CI, benchmark gates for affected
  retrieval/scoring code, and the pinned Cairn gate.

The removal PR must reduce semantic duplication; replacing the old paths with a
third orchestration vocabulary does not satisfy this todo.
