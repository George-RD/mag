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
- error: CAIRN_SOURCE_SHA256_MISMATCH verified source `src.mag-agents-guide` sha256 mismatch
- warning: CAIRN_DECISION_UNKNOWN_PROVENANCE decision `dec.retain-mcp-full-compatibility-mode` references unknown provenance `dec.select-local-runtime-composition-root`
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.entrypoints` claims `src/main.rs` at 1799 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.mcp` claims `src/mcp/mod.rs` at 752 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.models` claims `src/memory_core/embedder.rs` at 971 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.retrieval` claims `src/memory_core/scoring.rs` at 1223 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/helpers.rs` at 1322 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.memory.storage.sqlite` claims `src/memory_core/storage/sqlite/search.rs` at 584 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/config_writer.rs` at 1833 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_MODULE_OVERSIZED module `mag.runtime.setup` claims `src/setup.rs` at 1978 lines, over the 500-line guideline with no allow-list marker
- warning: CAIRN_TODO_ORPHAN_NODE todo `././meta/todos/recheck-doctor-after-fixes.md` references unknown node `mag.runtime.doctor`
- info: CAIRN_RECONCILE_ORPHANED_FILE Rust file `src/bin/fetch_benchmark_data.rs` is not owned by any eligible node
- info: CAIRN_RECONCILE_ORPHANED_FILE Rust file `src/doctor_checks.rs` is not owned by any eligible node
