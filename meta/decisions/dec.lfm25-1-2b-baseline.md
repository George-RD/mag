---
id: dec.lfm25-1-2b-baseline
nodes: [mag.runtime.memory.models, mag.quality.benchmarks]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "A smaller local model matches the task-level quality gates"
  - "LFM2.5 1.2B cannot meet ordinary-computer latency or memory budgets"
  - "A clearly better permissively deployable local model becomes available"
informed_by: [res.local-first-sequencing]
related: [dec.local-first-dual-mode, dec.preserve-derived-memory-provenance]
---
# LFM2.5 1.2B is the local generative reference

LFM2.5 1.2B Instruct is the reference model for local extraction, relationship
reasoning, grouping, and consolidation evaluations. LFM2.5 350M is not an
automatic fallback; individual tasks move only after measured parity within a
predeclared tolerance.
