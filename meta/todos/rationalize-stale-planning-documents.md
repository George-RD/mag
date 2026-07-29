---
node: mag
status: in_progress
created: 2026-07-28
unblocked: 2026-07-29
---
# Rationalize Stale Planning Documents

The architecture audit and composition-root decision identified four concrete
corrections:

- historical dead-code, source-tree, tech-debt, campaign, and adversarial review
  records must be labelled as dated evidence rather than live authority;
- the superseded substrate execution roadmap and trait-surface spec must point to
  the selected local runtime decision and live Cairn migration todos;
- configuration material must state that `MAG_LLM_*` only describes an
  experimental backend and does not change CLI/MCP production behaviour until a
  caller is wired through the selected runtime;
- setup and CLI material must describe command transport as the only executable
  setup mode, which was completed by `todo.correct-setup-transport-surface`.

## Acceptance criteria

- [ ] The strongholds directory has an archive boundary and the main recon/campaign
  documents carry a direct historical warning.
- [ ] The execution roadmap and trait-surface document cannot be read as approval
  to promote the feature-gated substrate wholesale.
- [ ] LLM configuration distinguishes available backend code from production
  wiring and does not imply that environment variables activate memory behaviour.
- [x] Setup and CLI documentation describe only the verified command transport.
- [ ] Live mission, architecture, decisions, and status point to the local-first
  roadmap and bounded Cairn artefacts.
- [ ] Exact-head repository CI and the pinned Cairn gate pass.
