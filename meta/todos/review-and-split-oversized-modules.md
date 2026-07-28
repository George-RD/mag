---
node: mag.runtime
status: blocked
created: 2026-07-28
---
# Review and split oversized modules where cohesion is weak

Blocked by `todo.audit-current-architecture-and-dead-code` and the production
composition-root decision.

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
