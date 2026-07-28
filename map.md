---
generated: true
---

# Cairn Map

## Synced
- mag
- mag.integrations
- mag.integrations.connectors
- mag.integrations.packaging
- mag.integrations.python
- mag.quality
- mag.quality.benchmarks
- mag.quality.scripts
- mag.quality.tests
- mag.runtime
- mag.runtime.daemon
- mag.runtime.entrypoints
- mag.runtime.mcp
- mag.runtime.memory
- mag.runtime.memory.domain
- mag.runtime.memory.models
- mag.runtime.memory.retrieval
- mag.runtime.memory.storage
- mag.runtime.memory.storage.api
- mag.runtime.memory.storage.memory
- mag.runtime.memory.storage.sqlite
- mag.runtime.setup
- mag.runtime.substrate

## Ghost
None

## Orphaned
- src/bin/fetch_benchmark_data.rs
- src/doctor_checks.rs

## Active changes

None in Phase 1.

## Findings
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.benchmarks` claims `benches/locomo/main.rs` at 1021 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.benchmarks` claims `benches/locomo/scoring.rs` at 978 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.benchmarks` claims `benches/longmemeval/local.rs` at 935 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.benchmarks` claims `benches/phase2_bench.rs` at 3350 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.benchmarks` claims `benches/scale_bench.rs` at 627 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.tests` claims `tests/cli_output_smoke.rs` at 583 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.tests` claims `tests/mcp_smoke.rs` at 1099 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.quality.tests` claims `tests/storage_conformance.rs` at 639 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.entrypoints` claims `src/cli.rs` at 1255 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.entrypoints` claims `src/main.rs` at 1714 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.mcp` claims `src/mcp/mod.rs` at 764 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.mcp` claims `src/mcp/tools/facades.rs` at 796 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.domain` claims `src/memory_core/domain.rs` at 589 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.models` claims `src/memory_core/embedder.rs` at 967 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.models` claims `src/memory_core/llm.rs` at 505 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.retrieval` claims `src/memory_core/scoring.rs` at 1223 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.memory` claims `src/memory_core/storage/memory/mod.rs` at 1179 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/admin/maintenance.rs` at 637 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/admin/welcome.rs` at 504 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/crud.rs` at 981 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/helpers.rs` at 1322 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/nlp.rs` at 547 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/schema.rs` at 609 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/search.rs` at 548 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/session.rs` at 611 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/tests.rs` at 8378 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/config_writer.rs` at 1825 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/setup.rs` at 1969 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/tool_detection.rs` at 1439 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/uninstall.rs` at 1402 lines, over the 500-line guideline with no allow-list marker
- info: CAIRN_RECONCILE_ORPHANED_FILE Rust file `src/bin/fetch_benchmark_data.rs` is not owned by any eligible node
- info: CAIRN_RECONCILE_ORPHANED_FILE Rust file `src/doctor_checks.rs` is not owned by any eligible node
