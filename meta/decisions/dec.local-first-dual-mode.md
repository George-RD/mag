---
id: dec.local-first-dual-mode
nodes: [mag, mag.runtime.memory.models, mag.runtime.daemon]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "Local and service deployments require incompatible memory semantics"
  - "A supported platform cannot run the minimum local quality baseline"
informed_by: [res.local-first-sequencing]
related: [dec.lfm25-1-2b-baseline]
---
# Local-first core with optional service deployment

Local and hosted operation are deployment modes of the same memory system, not
separate products. Local mode must reach the core quality baseline without cloud
credentials. Service mode may add cross-device access, teams, administration,
and larger hardware, but it reuses the same model-role and memory contracts.
