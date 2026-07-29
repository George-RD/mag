---
id: dec.classify-planning-authority
nodes:
  - mag
  - mag.runtime.memory.models
  - mag.runtime.substrate
status: accepted
date: 2026-07-29
revisit_triggers:
  - "MAG adopts a new durable planning or work-status system"
  - "A historical campaign is intentionally reactivated through a new accepted decision"
  - "The generative backend becomes wired into production CLI or MCP composition"
informed_by:
  - res.planning-authority-inventory
refines:
  - dec.classify-current-runtime-boundaries
  - dec.select-local-runtime-composition-root
---
# Classify live planning authority and historical evidence

## Decision

MAG uses one live planning hierarchy:

1. the user's request;
2. accepted Cairn decisions and contracts for the affected nodes;
3. `docs/specs/local-first-roadmap.md` for mission, dependency order, phases, and
   quality gates;
4. Cairn todo artefacts for work status.

`docs/strongholds/` is a historical evidence archive. Reconnaissance, campaign,
adversarial review, metrics, line counts, tool counts, and completion claims in
that directory describe the repository at their recorded date and must be
re-verified before use.

The former substrate execution roadmap and trait-surface spec are superseded
historical design inputs. Their original text remains pinned in Git history, while
the live paths contain concise authority pointers. They do not authorize
wholesale substrate promotion or a second production composition path.

The optional LLM module is available backend code, not production wiring.
`MAG_LLM_*` variables affect only experiments or future callers that explicitly
load `LlmConfig` until a verified local-runtime migration wires a generative role
into CLI or MCP.

## Consequences

- Historical evidence remains accessible without competing with live status.
- Agents query Cairn rather than interpreting old roadmap checklists.
- Reusing an old plan requires a new accepted decision and current code evidence.
- Documentation distinguishes compiled capability from executable product
  behaviour.
