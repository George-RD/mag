---
id: res.cairn-fit-for-mag
nodes: [mag]
sources: [src.cairn-framework, src.mag-agents-guide, src.local-first-roadmap]
date: 2026-07-28
---
# Cairn fit assessment for MAG

MAG had an ordered prose roadmap but no small, authoritative status records.
Decisions and research were spread across large roadmap, stronghold, conductor,
and benchmark documents. A single append-only ADR JSONL would improve structure
but would still become a large merge-conflict and retrieval surface.

Cairn directly matches the requirement: stable architecture node IDs; separate
decision, research, source, contract, and todo files; typed links between them;
and bounded JSON queries such as `cairn brief`, `cairn rationale`, `cairn todos`,
and `cairn context --scope`.

The automatic brownfield draft was not accepted unchanged. It discovered useful
directories but flattened overlapping paths such as `src`, `src/memory_core`,
and `src/memory_core/storage/sqlite` into sibling modules. MAG therefore uses a
reviewed blueprint based on semantic ownership rather than filesystem depth.
