---
id: dec.use-cairn-development-context
nodes: [mag]
status: accepted
date: 2026-07-28
revisit_triggers:
  - "Cairn maintenance repeatedly costs more than the context and drift it prevents"
  - "Cairn cannot represent a material MAG architecture or decision relationship"
  - "The map produces persistent false structural blockers after curation"
informed_by: [res.cairn-fit-for-mag]
related: [dec.sequence-architecture-before-llm-wiring]
---
# Use Cairn for MAG development context

MAG will use Cairn as the queryable architecture, decision, research, contract,
and work-status layer. The narrative roadmap remains useful for mission, order,
and exit gates, but `meta/todos/` is the authoritative execution status.

Agents should query the smallest connected slice instead of loading all planning
material. Typical entry points are `cairn next`, `cairn brief <todo>`,
`cairn context --scope <node>`, and `cairn rationale <node>`.

Alternatives considered were a conventional ADR directory plus a custom JSONL
index, or continuing with prose roadmaps. The former duplicates functionality
already present in Cairn; the latter does not scale to bounded queries.
