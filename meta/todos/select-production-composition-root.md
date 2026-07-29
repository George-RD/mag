---
node: mag.runtime.substrate
status: open
created: 2026-07-28
unblocked: 2026-07-29
---
# Select Production Composition Root

The current architecture audit is complete. Decide whether substrate becomes the
production composition root or its useful trait boundaries are folded into the
existing SQLite-centred path.

Record an accepted decision, migration slices, compatibility period, and removal
path. Compare public CLI/MCP parity, retrieval quality, latency, memory use,
operational complexity, testability, and reversibility. Preserve the current
`processed: ` CLI behaviour until a deliberate compatibility decision changes it.
