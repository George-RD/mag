---
node: mag
status: done
created: 2026-07-28
unblocked: 2026-07-29
completed: 2026-07-29
---
# Rationalize Stale Planning Documents

MAG now has one explicit planning hierarchy: the user's request, binding Cairn
decisions/contracts, the local-first roadmap for sequence and gates, and Cairn
todos for status. Historical reports and superseded substrate plans remain
accessible without competing with that authority.

## Completed changes

- `docs/strongholds/` has an archive boundary, and the principal recon, campaign,
  adversarial review, and improvement-plan files carry dated warnings.
- The obsolete substrate execution roadmap and trait-surface spec are concise
  pointers to their pinned historical versions and the selected local-runtime
  decision/todos.
- `docs/configuration.md` and the MAG development skill state that `MAG_LLM_*`
  configures experimental backend code only; no current CLI or MCP caller uses it.
- `dec.classify-planning-authority` binds the classification and links the
  planning-authority research.
- Verified recon source hashes were refreshed after warning-only edits.
- Setup and CLI material already describe command transport as the only executable
  setup mode through `todo.correct-setup-transport-surface`.

## Acceptance criteria

- [x] The strongholds directory has an archive boundary and the main recon/campaign
  documents carry a direct historical warning.
- [x] The execution roadmap and trait-surface document cannot be read as approval
  to promote the feature-gated substrate wholesale.
- [x] LLM configuration distinguishes available backend code from production
  wiring and does not imply that environment variables activate memory behaviour.
- [x] Setup and CLI documentation describe only the verified command transport.
- [x] Live mission, architecture, decisions, and status point to the local-first
  roadmap and bounded Cairn artefacts.
- [x] Exact-head repository CI and the pinned Cairn gate pass.

## Verification

- Repository CI `30476639058` passed all-feature tests, formatting, Clippy, smoke,
  npm installation, Python wrappers, version checks, and all installer-integrity
  variants on the completed source-change head.
- Pinned Cairn 0.9 gate `30476638584` passed on the same head. The benchmark gate
  correctly skipped because no retrieval, scoring, reranking, or storage code
  changed.
