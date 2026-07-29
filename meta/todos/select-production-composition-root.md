---
node: mag.runtime.substrate
status: in_progress
created: 2026-07-28
unblocked: 2026-07-29
---
# Select Production Composition Root

The evidence-backed decision is recorded in
`dec.select-local-runtime-composition-root`: MAG will use one entrypoint-owned,
transport-independent local runtime facade over the current SQLite-backed
implementation. The current feature-gated substrate will not be promoted
wholesale.

## Comparison outcome

- Public CLI/MCP and retrieval behaviour can be preserved immediately by
  delegation; the substrate has no production caller or parity evidence.
- No substrate-versus-production quality, latency, or memory comparison exists.
  A wholesale promotion would therefore combine architecture migration with an
  unmeasured retrieval rewrite.
- The local runtime path is testable and reversible by command family and does
  not require schema or data migration in its additive phase.
- Narrow, demonstrated interfaces may be folded into the live path; duplicate
  substrate and legacy surfaces are retired after caller migration and one
  released compatibility period.

## Migration slices

1. Introduce the additive local runtime facade and public-surface parity harness.
2. Migrate CLI write, basic read, retrieval, and administration command families.
3. Migrate MCP full/minimal tools through the same runtime.
4. Add separated model roles only after the local evaluation harness exists.
5. Retire the legacy `Pipeline` construction and duplicate substrate surfaces.

## Verification state

The first Cairn run (`30474382280`) failed only because this decision changed two
verified sources while their pinned hashes still described the previous bytes.
The source manifests and generated Cairn snapshots have been refreshed from the
checked-out files. Exact-head repository and Cairn verification is rerunning on
the corrected branch.

## Acceptance criteria

- [x] One production composition root is selected.
- [x] Public parity, quality/resource evidence, complexity, testability, and
  reversibility are compared.
- [x] Migration slices, compatibility period, rollback, and removal path are
  explicit.
- [x] Existing `processed: ` behaviour remains protected by a separate migration
  decision and regression coverage.
- [ ] Exact-head repository CI and the pinned Cairn gate pass.
