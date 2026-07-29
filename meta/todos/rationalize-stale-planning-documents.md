---
node: mag
status: open
created: 2026-07-28
unblocked: 2026-07-29
---
# Rationalize Stale Planning Documents

The current architecture audit identified the first concrete corrections:

- historical dead-code and tech-debt recon reports must be marked archival;
- configuration material must not imply that `MAG_LLM_*` activates production
  behaviour before an LLM backend is wired;
- setup and CLI material must distinguish verified command transport from the
  incomplete stdio/HTTP setup modes.

Mark historical stronghold and roadmap documents as superseded, archival, or
still authoritative. Move live decisions/research/todos into Cairn artefacts and
leave concise pointers rather than competing sources of truth.
