---
id: dec.preserve-derived-memory-provenance
nodes: [mag.runtime.memory.domain, mag.runtime.memory.storage.sqlite, mag.runtime.memory.models]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "A derived artefact cannot retain source lineage within practical storage limits"
  - "A stronger immutable event model replaces the current memory representation"
informed_by: [res.local-first-sequencing]
related: [dec.lfm25-1-2b-baseline]
---
# Generated intelligence remains derived and traceable

Model-generated facts, relationships, clusters, contradictions, and summaries
never overwrite raw memories. Derived records retain source memory IDs, model
profile, prompt/schema version, confidence, and creation time so they can be
inspected, invalidated, and regenerated.
