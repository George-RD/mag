---
id: dec.evidence-based-cleanup
nodes: [mag.runtime, mag.runtime.memory, mag.runtime.substrate]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "Current compiler and coverage evidence proves a simpler safe bulk-removal path"
informed_by: [res.architecture-state-audit]
related: [dec.sequence-architecture-before-llm-wiring]
---
# Cleanup follows current evidence, not stale file-size reports

Feature-gated, benchmark-only, connector, or fallback code is not dead merely
because it is not on the default path. The cleanup pass must trace current
callers and features, run all-feature tests, and preserve behavior before
deletion or consolidation. Architectural duplication is prioritized over raw
line-count reduction.
