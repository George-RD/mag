# Changelog
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

Notable changes to MAG. Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

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
