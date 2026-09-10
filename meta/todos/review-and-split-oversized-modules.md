---
node: mag.runtime
status: open
created: 2026-07-28
---
# Review and split oversized modules where cohesion is weak

The architecture audit is done and the production composition-root decision is
accepted. The recorded blockers are cleared; decomposition is not complete.

## Recovered classification, 2026-09-10

The unmerged branch at `a2bbb71` contains a useful responsibility audit. Rechecked
against `09c8634`: `src/main.rs` still combines bootstrap, command dispatch and
doctor rendering; SQLite `helpers.rs` combines query expansion, SQL filters,
vector-extension I/O and row mapping; setup/configuration span several client
formats. Start with those seams, not a line-count target. Keep CLI rendering out
of `LocalMemoryRuntime` and preserve the one-transaction write boundary.

Retain the branch's remaining proposed seams as audit inputs: MCP protocol versus
validation; model provisioning shared with the reranker; benchmark/production
stemming divergence. Verify callers and parity before each split. The branch's
line counts and blanket large-module exemptions were not imported. Existing
warnings remain visible. See `meta/reviews/claude-branch-recovery.md`.

Cairn's first scan identified large production files in CLI/setup, retrieval,
model, storage, and MCP surfaces, plus large benchmark and test files. File size
alone is not grounds for a split. Classify each finding as:

- cohesive and intentionally large;
- generated/data-heavy;
- test or benchmark support;
- mixed-responsibility production code requiring decomposition.

Add an allow marker only with a durable cohesion reason. Create node-level
refactoring changes for mixed-responsibility code, prioritizing architecture
ambiguity and change risk over cosmetic line-count reduction.
