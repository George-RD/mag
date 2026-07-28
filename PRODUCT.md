# MAG product context

## Product

MAG is a local memory layer for coding agents. It stores useful decisions, bug fixes, work rules and session handoffs in one SQLite database, then makes that context available to MCP clients. Claude Code also has a native plugin and lifecycle hooks.

## Audience

Primary:

- Developers who move between two or more coding agents.
- Developers working with client, private or regulated context that should remain on their machine.
- Technical users who prefer inspectable local data over another hosted account.

Wrong fit:

- Teams that need managed cloud sync now.
- Users who need application-level database encryption.
- Users who want a browser-only service with no local model or setup.
- Users who expect useful long-term memory with no capture rules or cleanup.

## Job to be done

When I switch coding tools or resume work later, bring back the small amount of prior context that changes the next decision, without sending my memory store to a third party by default.

## Primary action

Install MAG, let `mag setup` configure known clients, store one real decision, then recall it with different wording.

## Product truth

- The default setup uses one local SQLite database for memories.
- Embeddings and search run locally by default.
- First use downloads local ONNX models.
- Optional API embedding providers can send data outside the machine when explicitly configured.
- The SQLite database is plaintext. Users should not store secrets and should use full-disk encryption when needed.
- MAG is a 0.1.x project. Interfaces and setup may still change.
- The published 90.1% LoCoMo result is retrieval-oriented word-overlap recall, not a general score for agent quality.
- Team-wide cloud sync is not built in today.

## Position

MAG competes on cross-tool continuity, local ownership and inspectability. It is not positioned as a universal replacement for project documentation, built-in memory, or managed team-memory services.

## Voice

Direct, technical and bounded by evidence. Explain the mechanism. State limitations without hiding them in footnotes. Avoid generic AI language, inflated claims and false certainty.

## Sources of truth

- `docs/architecture.md`
- `docs/benchmarks/methodology.md`
- `docs/SETUP.md`
- `SECURITY.md`
- `src/tool_detection.rs`
- `src/setup.rs`
- Open GitHub issues for roadmap status
