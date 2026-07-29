---
node: mag.runtime.substrate
---
# mag.runtime.substrate contract

The current feature-gated substrate is not the production composition root. It is
constructed by tests and benchmark binaries only and must not gain independent
product behaviour, schemas, retrieval semantics, or model wiring.

Useful narrow implementations may be folded into the selected local runtime only
when a bounded migration slice has public CLI/MCP parity coverage. Retrieval,
scoring, reranking, or storage changes also require the benchmark and local
quality gates. Prefer the existing live capability and retrieval boundaries over
the broad substrate `MemoryStore` supertrait.

After production callers use the local runtime and the compatibility period has
elapsed, move any retained implementation to the selected vocabulary and remove
the duplicate substrate orchestrators and types. Keeping substrate as a second
extension surface without a production caller violates this contract.
