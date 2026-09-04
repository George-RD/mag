---
node: mag.runtime
status: in_progress
created: 2026-07-28
unblocked: 2026-09-04
---
# Review and split oversized modules where cohesion is weak

Unblocked by `todo.audit-current-architecture-and-dead-code` (done) and
`dec.select-local-runtime-composition-root` (accepted). The classification step
this todo asked for is complete and recorded below. What remains is the
node-level refactoring changes, which are deliberately not bundled with the
classification.

## Correction: there is no allow marker

The original text said to "add an allow marker only with a durable cohesion
reason". No such marker exists. Cairn 0.9.0 has no size finding and no
size suppression:

- `cairn.blueprint`'s grammar has no size field. Tags are inert
  (`.claude/skills/cairn-dev/references/blueprint-syntax.md`).
- No finding code in `.claude/skills/cairn-dev/references/finding-codes.md`
  concerns file size.
- `cairn.config.yaml` supports only `multi_target.intentional_asymmetry`
  (suppresses CT001) and `exclude_paths` (suppresses orphaned-file findings).
- `prek.toml` checks added-file byte size, not per-module line count.

Record "intentionally large, do not split" as an accepted decision artefact in
`meta/decisions/` naming the file and the cohesion reason, reinforced by a
sentence in the owning node's contract. Do not invent a tag.

## Classification

Thresholds: `src/` and `benches/` over 450 lines, `tests/` over 800 lines.
"prod" is the line count before the file's own `mod tests` block.

| File | Lines | Classification | Owning node |
|---|---|---|---|
| `src/memory_core/storage/sqlite/tests.rs` | 8378 | test-or-bench-support | `mag.runtime.memory.storage.sqlite` |
| `benches/phase2_bench.rs` | 3350 | generated-or-data-heavy | `mag.quality.benchmarks` |
| `src/setup.rs` | 1978 (938) | **mixed-responsibility** | `mag.runtime.setup` |
| `src/config_writer.rs` | 1833 (905) | **mixed-responsibility** | `mag.runtime.setup` |
| `src/main.rs` | 1799 (1793) | **mixed-responsibility** | `mag.runtime.entrypoints` |
| `src/tool_detection.rs` | 1439 (624) | cohesive | `mag.runtime.setup` |
| `src/uninstall.rs` | 1402 (894) | cohesive | `mag.runtime.setup` |
| `src/memory_core/storage/sqlite/helpers.rs` | 1322 (1057) | **mixed-responsibility** | `mag.runtime.memory.storage.sqlite` |
| `src/cli.rs` | 1289 (417) | cohesive | `mag.runtime.entrypoints` |
| `src/memory_core/scoring.rs` | 1223 (599) | cohesive | `mag.runtime.memory.retrieval` |
| `src/memory_core/storage/memory/mod.rs` | 1179 (681) | cohesive | `mag.runtime.memory.storage.memory` |
| `benches/locomo/main.rs` | 1021 | test-or-bench-support | `mag.quality.benchmarks` |
| `src/memory_core/storage/sqlite/crud.rs` | 1009 | cohesive | `mag.runtime.memory.storage.sqlite` |
| `benches/locomo/scoring.rs` | 978 (426) | test-or-bench-support | `mag.quality.benchmarks` |
| `src/memory_core/embedder.rs` | 971 (844) | **mixed-responsibility** | `mag.runtime.memory.models` |
| `benches/longmemeval/local.rs` | 935 | test-or-bench-support | `mag.quality.benchmarks` |
| `tests/mcp_smoke.rs` | 843 | test-or-bench-support | `mag.quality.tests` |
| `src/mcp/mod.rs` | 752 (641) | **mixed-responsibility** | `mag.runtime.mcp` |
| `src/memory_core/storage/sqlite/session.rs` | 711 | cohesive | `mag.runtime.memory.storage.sqlite` |
| `src/memory_core/storage/sqlite/admin/maintenance.rs` | 660 (643) | cohesive | `mag.runtime.memory.storage.sqlite` |
| `benches/scale_bench.rs` | 627 | test-or-bench-support | `mag.quality.benchmarks` |
| `src/memory_core/domain.rs` | 596 | cohesive | `mag.runtime.memory.domain` |
| `src/memory_core/storage/sqlite/search.rs` | 584 | cohesive | `mag.runtime.memory.storage.sqlite` |
| `src/memory_core/storage/sqlite/schema.rs` | 563 | cohesive | `mag.runtime.memory.storage.sqlite` |
| `src/mcp/tools/facades.rs` | 550 | cohesive | `mag.runtime.mcp` |
| `src/memory_core/storage/sqlite/nlp.rs` | 547 (476) | generated-or-data-heavy | `mag.runtime.memory.storage.sqlite` |
| `src/memory_core/llm.rs` | 505 (487) | cohesive | `mag.runtime.memory.models` |
| `src/memory_core/storage/sqlite/admin/welcome.rs` | 504 | cohesive | `mag.runtime.memory.storage.sqlite` |
| `src/memory_core/storage/sqlite/advanced.rs` | 491 (249) | cohesive | `mag.runtime.memory.storage.sqlite` |
| `src/memory_core/storage/sqlite/entities.rs` | 482 (373) | generated-or-data-heavy | `mag.runtime.memory.storage.sqlite` |
| `src/substrate/extraction.rs` | 461 | cohesive | `mag.runtime.substrate` |
| `src/local_memory_runtime.rs` | 460 | cohesive | `mag.runtime.entrypoints` |
| `benches/onnx_profile.rs` | 454 | test-or-bench-support | `mag.quality.benchmarks` |
| `src/substrate/store_impl.rs` | 453 | cohesive | `mag.runtime.substrate` |

Cohesion reasons worth recording as decisions rather than revisiting each audit:

- `src/local_memory_runtime.rs` — its length is its contract. One place naming
  every capability is what `meta/contracts/runtime.entrypoints.md` requires;
  splitting it recreates the per-command branching the contract forbids.
- `src/memory_core/domain.rs` — `EventType`'s hand-written `Display`, `FromStr`,
  `Serialize` and `JsonSchema` must agree. Splitting fragments one serialization
  contract.
- `src/memory_core/storage/sqlite/crud.rs` — the six phases of `store_internal`
  share one `rusqlite` transaction. The function is the weight, not the file.
- `src/memory_core/scoring.rs` — every item is a scoring factor over
  `ScoringParams`. The node is benchmark-gated, so moving factors between files
  changes nothing measurable and breaks gate blame.
- `src/uninstall.rs` — the steps are one reversal transaction reporting one
  `UninstallSummary`. Splitting separates a transaction from its rollback report.
- `src/cli.rs` — a single `#[derive(Subcommand)]` enum the macro requires in one
  place.

## Remaining refactoring changes, in priority order

Priority is architectural ambiguity and change risk, not line count.

1. **`src/main.rs`** — the only file contradicting an accepted decision.
   `dec.select-local-runtime-composition-root` requires new behaviour to enter
   through `LocalMemoryRuntime` rather than a command-specific branch, and
   `main()` is roughly 970 lines of command-specific branches. Three
   responsibilities: process bootstrap and composition; command execution and
   stdout rendering; and a doctor subsystem at L1227-1792.
   Split: move the doctor subsystem to `src/doctor/` and fold in the existing
   `src/doctor_checks.rs` (one call site, roughly 570 lines out — do this
   first); then split the command arms into `src/commands/{write,read,search,
   session,admin}.rs` following the groupings already present in `cli.rs`.
2. **`src/memory_core/storage/sqlite/helpers.rs`** — the node's dumping ground;
   its name conveys no boundary. Four responsibilities: lexical query expansion,
   SQL predicate construction, sqlite-vec I/O, row hydration. Split into
   `query_expansion.rs`, `sql_filters.rs`, `vec_ext.rs`, `row_mapping.rs` and
   delete `helpers.rs`. Only `query_expansion.rs` moves retrieval scores, so the
   split lets the benchmark gate distinguish what today it cannot.
3. **`src/config_writer.rs`** — highest blast radius per line; it writes into
   other tools' config files. Four responsibilities named by its own section
   banners: JSON MCP entries, the Claude plugin CLI shell-out, Codex TOML
   splicing, and the sandbox allowlist patcher. Split into `src/config_writer/`.
4. **`src/setup.rs`** — the connector-installation half carries the only
   cross-node edge in `mag.runtime.setup`, and it is invisible in the layout
   (`include_str!` calls buried mid-file). Split into `src/setup/{mod,
   connectors,transport}.rs`. Do this in one change with (3): both duplicate
   `atomic_write`, and the setup contract treats them as one obligation.
5. **`src/mcp/mod.rs`** — the target shape is already written in
   `docs/specs/module-decomposition.md` §2a and only partly landed. Create
   `src/mcp/protocol.rs` for the tool registry and generated protocol
   documentation, move the validation helpers into the existing stub
   `src/mcp/validation.rs`, leave server construction and dispatch in `mod.rs`.
   The `#[tool_router]` constraint noted in `docs/strongholds/palantir-r1-attack.md`
   applies to the dispatch half, which does not move.
6. **`src/memory_core/embedder.rs`** — a clean seam and no ambiguity about where
   new code goes, so this is a legibility win only. Move model asset
   provisioning to `src/memory_core/model_assets.rs`. Check
   `memory_core/reranker.rs` first so `download_cross_encoder_model` lands in the
   same module.

## Explicitly not a decomposition item

`benches/locomo/scoring.rs` duplicates production logic: `stem` and the stopword
set against `scoring::simple_stem` and `scoring::is_stopword`, and
`expand_date_tokens` against the sqlite helper of the same name. That is either
deliberate divergence or drift, and either way it needs a decision, not a file
split. Raise it separately.

`src/substrate/extraction.rs` and `src/substrate/store_impl.rs` are on the
retirement path in `todo.retire-legacy-and-substrate-orchestration`. Refactoring
them is wasted work.
