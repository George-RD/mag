---
node: mag.runtime.substrate
status: done
created: 2026-07-28
unblocked: 2026-07-29
completed: 2026-07-29
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

## Verification

- The first Cairn run (`30474382280`) correctly rejected stale hashes for two
  verified sources changed by the decision. Their manifests and the generated
  Cairn views were refreshed from the checked-out bytes.
- Repository CI `30474849387` passed all-feature tests, formatting, Clippy, smoke,
  npm installation, Python wrappers, version checks, and all installer-integrity
  variants on the corrected decision and blueprint head.
- Pinned Cairn 0.9 architecture gate `30474849201` passed on the same head.
- The generated map was refreshed after replacing the pending substrate label
  with the accepted migration-only state and removing the stale setup-to-daemon
  edge.

## Acceptance criteria

- [x] One production composition root is selected.
- [x] Public parity, quality/resource evidence, complexity, testability, and
  reversibility are compared.
- [x] Migration slices, compatibility period, rollback, and removal path are
  explicit.
- [x] Existing `processed: ` behaviour remains protected by a separate migration
  decision and regression coverage.
- [x] Exact-head repository CI and the pinned Cairn gate pass.
