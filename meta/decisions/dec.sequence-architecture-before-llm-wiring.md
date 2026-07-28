---
id: dec.sequence-architecture-before-llm-wiring
nodes: [mag.runtime.memory, mag.runtime.substrate, mag.quality.benchmarks]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "The two composition paths are proven to be the same production path"
  - "A minimal reversible spike is required to gather evidence for the decision"
informed_by: [res.architecture-state-audit, res.local-first-sequencing]
refines: [dec.local-first-dual-mode]
related: [dec.use-cairn-development-context]
---
# Resolve composition before production LLM wiring

MAG will first audit and select the production composition root, then build the
evaluation harness, then wire LFM2.5 1.2B into production. A short disposable
spike is allowed for evidence, but permanent intelligence must not be duplicated
across `memory_core::Pipeline` and `substrate`.
