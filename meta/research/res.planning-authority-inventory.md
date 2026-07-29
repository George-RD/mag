---
id: res.planning-authority-inventory
nodes:
  - mag
  - mag.runtime.entrypoints
  - mag.runtime.memory.models
  - mag.runtime.substrate
sources:
  - src.local-first-roadmap
  - src.current-runtime-baseline
  - src.mag-agents-guide
date: 2026-07-29
---
# Planning authority inventory

## Live authority

- The user's request is primary.
- Accepted Cairn decisions and contracts bind the nodes they name.
- `docs/specs/local-first-roadmap.md` owns mission, dependency order, development
  phases, and quality gates.
- `meta/todos/` owns work status. `cairn status`, `cairn next`, and
  `cairn brief todo.<slug>` are the supported status views.
- Current setup, CLI, and configuration guidance is authoritative only where it
  matches the executable code and binding decisions.

## Historical evidence

`docs/strongholds/` preserves dated reconnaissance, campaign, adversarial review,
and implementation records. The reports remain useful as sources for why work
was proposed or how the repository looked at a point in time. Their line counts,
feature status, tool counts, completion claims, and future recommendations must
be re-verified before reuse.

## Superseded planning

`docs/specs/execution-roadmap.md` records the completed v0.1.9-v0.2.2 structural
campaign. `docs/specs/trait-surface.md` records a proposed substrate design. They
remain design history, but neither authorizes the historical v0.3.x wholesale
substrate campaign.

`dec.select-local-runtime-composition-root` is the current architectural decision:
MAG introduces one entrypoint-owned local runtime over the verified SQLite-backed
behaviour, migrates CLI and MCP in bounded slices, and retires duplicate legacy
and substrate orchestration after a compatibility period.

## Experimental configuration

The optional LLM module can parse `MAG_LLM_*` and construct backend clients, but
current production CLI and MCP composition does not call it. Environment
variables therefore configure code only when an experiment or future runtime
wiring explicitly consumes that configuration; they do not activate extraction,
reflection, retrieval, answer generation, or any other production memory
behaviour today.
