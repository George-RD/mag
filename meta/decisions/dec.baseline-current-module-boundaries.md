---
id: dec.baseline-current-module-boundaries
nodes: [mag.runtime.entrypoints, mag.runtime.setup, mag.runtime.doctor, mag.runtime.mcp, mag.runtime.memory.domain, mag.runtime.memory.models, mag.runtime.memory.retrieval, mag.runtime.memory.storage.api, mag.runtime.memory.storage.sqlite, mag.runtime.memory.storage.memory, mag.runtime.substrate, mag.runtime.daemon, mag.integrations.connectors, mag.integrations.python, mag.integrations.packaging, mag.quality.tests, mag.quality.benchmarks, mag.quality.benchmark-data, mag.quality.scripts]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "The current architecture audit identifies a better semantic ownership boundary"
  - "The production composition-root decision changes module responsibilities"
  - "Cairn reconciliation shows a persistent false ownership model"
informed_by: [res.cairn-fit-for-mag, res.architecture-state-audit]
related: [dec.evidence-based-cleanup, dec.sequence-architecture-before-llm-wiring]
---
# Baseline the current semantic module boundaries

The curated Cairn map records the smallest useful current architecture for
navigation and drift detection. It is a baseline, not a claim that every current
boundary is ideal or permanent. Directory-depth discovery was rejected because
it produced overlapping `src` and nested-module ownership.

The architecture audit may split, merge, or retire these nodes through recorded
changes. Until then, every module has an explicit contract and rationale rather
than being an untracked addition to the blueprint.
