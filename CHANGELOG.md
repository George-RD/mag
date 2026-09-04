# Changelog
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

Notable changes to MAG. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Added
- `memory_intelligence_eval` binary (`benches/memory_intelligence/`) scoring eight task families through `LocalMemoryRuntime`: entities, temporal reference, relationships, lifecycle, supersession, grouping, provenance and question retrieval. It declares no `required-features`, so `--embedder placeholder` builds and runs under `--no-default-features`; `bge-small` and `profile-bge-small` need `real-embeddings`
- `data/memory_intelligence_eval/v1/` — 36 seeds in isolated groups, one SQLite database per group so auto-supersession in one group cannot corrupt another. Annotations are authored from the seed text rather than from MAG output, so a family can score zero on behaviour MAG has never implemented. `manifest.json` records the dataset SHA-256 and the per-family counts. Temporal annotations use a "last N days" form, so a score does not depend on which weekday the run happens
- `scripts/memory-intelligence-eval.sh` — appends one row to `docs/benchmarks/memory_intelligence_log.csv`, rewrites `docs/benchmarks/MEMORY-INTELLIGENCE.md`, and refuses to log a run whose dataset SHA-256 differs from the last row carrying the same `dataset_version`. It does not write `benchmark_log.csv`, whose columns `scripts/bench.sh` reads positionally
- `docs/benchmarks/memory-intelligence.md` — flags, metric definitions, the group isolation rule and the JSON summary fields
- First baseline, `bge-small-en-v1.5-int8` on 2026-09-04 against dataset `v1` at sha `3260e0a00beb`, run as `./scripts/memory-intelligence-eval.sh --embedder bge-small` at commit `f977ee0`: entities 7.4% micro F1, temporal 75.0% recall@10, relationships 66.7% recall, lifecycle 100% accuracy, supersession 50.0% F1, grouping 0.0% cluster coverage, provenance 100% link integrity, questions 90.0% recall@10. The harness reports current behaviour: entity extraction tags tool names as people, no labelled duplicate group is recovered, content dedup destroys one supersession pair before the supersession gate runs, and one query returns an answer where the store holds nothing relevant. The 61.1% overall figure is an unweighted mean over eight incommensurable metrics and moves when the set of scoring families changes. The corpus is small enough that one case moves a family by tens of points: `relationships` has three annotated edges, so one missed edge is 33 points
- Six families have no production implementation — fact extraction, contradiction detection, summarisation, relationship typing, entity normalisation and `referenced_date` inference. The harness prints the target output shape for each and leaves them out of the mean instead of scoring them zero
- `site/demos/` — three stepped walkthroughs (retrieval pipeline, cross-tool handoff, abstention gate) built from measured `_explain` values over a three-memory database, plus an index page. Each respects `prefers-reduced-motion`
- `site/assets/pipeline.svg` — the seven retrieval stages of an advanced search, animated in CSS, with `<title>` and `<desc>` for screen readers
- `docs/writing-style.md` — the prose standard for `README.md`, `docs/`, `meta/todos/`, commit messages and code comments: banned vocabulary and constructions, no hedging of deterministic behaviour, and a performance number carries its dataset, date, commit and command

### Changed
- `cairn:allow-large-module` markers on 24 files — 14 source modules, 3 integration tests, 7 benchmark and evaluation harnesses — each carrying a cohesion reason that stays true as the file changes. The six mixed-responsibility files keep their `CAIRN_MODULE_OVERSIZED` warning until they are split. `src/memory_core/scoring.rs` and `src/memory_core/storage/sqlite/search.rs` stay unmarked because both match `BENCHMARK_RELEVANT_STEMS` in `scripts/retrieval_benchmark_gate.py`, where a one-line comment would pull a full `./scripts/bench.sh --gate` run
- `meta/todos/review-and-split-oversized-modules.md` — classification of the 39 files over the size thresholds (`src/` and `benches/` over 450 lines, `tests/` over 800), with the split each of the six mixed-responsibility files would take. `src/main.rs` is first: roughly 970 lines of command-specific branching that contradicts `dec.select-local-runtime-composition-root`
- `.github/workflows/pages.yml` — the validator checks lang, title and description on all five site pages (`site/index.html` and the four under `site/demos/`), and rejects `pipeline.svg` if it loses `<title>`, `<desc>` or its `prefers-reduced-motion` guard, or gains `<animate>`, `xlink:href` or `<image>`
- README and `site/index.html` roadmaps — the architecture audit and the evaluation gate now read as done, retrieval calibration as in progress, and the LFM2.5 work as blocked behind calibration

### Fixed
- Front matter in five `meta/todos/` files was indented eight spaces, closing `---` included, so the YAML did not parse and `cairn todo set` could not read their status: `add-provenance-preserving-memory-intelligence`, `design-service-and-cross-device-mode`, `prototype-direct-onnx-lfm25`, `qualify-lfm25-350m-task-routing`, `wire-lfm25-production-ingestion`

## [0.1.5] - 2026-04-01

### Added
- 3 unified MCP facade tools (`memory`, `memory_recall`, `memory_admin`) replacing 16 individual tools (#175)
- `--mcp-tools full|minimal` flag for tool mode selection (#175)
- Token-budgeted `welcome_scoped()` with 4-tier priority injection (#174, #176)
- `Hit@1/Hit@3/Hit@5` metrics in LoCoMo benchmark (#170)
- `UserPreference` dedup threshold + schema migration (#173)
- `--budget-tokens` flags in hook scripts (#177)

### Fixed
- `welcome_scoped` project guard and `memory_admin` default doc mismatch (#180)

## [0.1.4]

### Added
- `mag setup` CLI wizard — auto-detects installed AI tools and writes MCP configs (#106-109, #112-113)
- Daemon mode — `mag serve` with HTTP transport for persistent access (#97-104)
- Claude Code plugin with hooks, skills, and AutoMem integration (#98)
- MCP smoke tests covering all 16 tools (#124, #136)
- Schema version tracking for additive migrations (#123, #134)
- Input validation limits on MCP tool parameters (#116, #130)
- Safety documentation for sqlite-vec extension loading (#127, #131)

### Changed
- Split monolithic `mod.rs` and `helpers.rs` into focused modules: `nlp.rs`, `query_classifier.rs`, `temporal.rs`, `conn_pool.rs`, `embedding_codec.rs`, `domain.rs`, `traits.rs` (#118)
- Extract pipeline phases from 680-line `fuse_refine_and_output` into `refine_scores()`, `enrich_graph_neighbors()`, `expand_entity_tags()` (#71, #142)
- Consolidate entity stopwords — `is_common_word()` now delegates to shared `is_stopword()` core (#72, #142)
- Define relationship type constants (`REL_PRECEDED_BY`, `REL_RELATES_TO`, etc.) replacing string literals (#74, #142)
- Activate `resolve_priority` helper, eliminate 5 inline copies (#122, #135)
- Rename `cosine_similarity` to `dot_product`, add `source_type` to `MemoryInput` (#117, #129)
- Gate dead modules behind `daemon-http` feature flag (#120, #132)
- Use SHA-256 hash comparison in `constant_time_eq` (#115, #128)
- Hoist `conn.prepare` out of entity loop for search performance (#121, #137)

### Removed
- Dead `suggested_limit_mult` field from `IntentProfile` (#73, #142)

### Fixed
- Flaky timing-dependent tests with `serial_test` (#126, #133)
- Clippy warnings and formatting in new test code (#138)

## [0.1.2] - 2026-03-20

Initial public release on crates.io, npm, and PyPI.

[Unreleased]: https://github.com/George-RD/mag/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/George-RD/mag/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/George-RD/mag/compare/v0.1.2...v0.1.4
[0.1.2]: https://github.com/George-RD/mag/releases/tag/v0.1.2
