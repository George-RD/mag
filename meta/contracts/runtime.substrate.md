---
node: mag.runtime.substrate
---
# mag.runtime.substrate contract

Substrate is a feature-gated candidate composition root, not a second product.
It is currently constructed by integration tests and benchmark binaries only;
no CLI or MCP production entrypoint uses it.

Until the composition decision is accepted, new production intelligence must
not be implemented independently in both substrate and the legacy/direct SQLite
paths. Promotion requires public CLI/MCP parity, retrieval-quality and resource
benchmarks, a reversible migration of live callers, and removal or deliberate
retention of the superseded adapter. If substrate is not selected, only its
proven useful boundaries should be folded into the production path.
