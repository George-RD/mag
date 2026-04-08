# Tech Debt Recon

## Status: Complete (2026-04-08)

### Summary

All planned refactors (#118–#123, Wave 1, Wave 2) are merged and shipped through v0.1.7. The codebase is in a clean post-refactor state. No major refactor is needed before starting JSONL telemetry work.

### Findings

| Area | Status | Detail |
|------|--------|--------|
| Refactor PRs | Done | #118–#123 all merged |
| Dead code | Minimal | ~20 `#[allow(dead_code)]` — all justified |
| TODOs | 1 | `advanced.rs:1541` — parallelize sub-queries (#121) |
| Dependencies | Clean | No deprecated deps; `ort =2.0.0-rc.11` pinned intentionally |
| Plugin sync | Identical | `plugin/` and cache are byte-for-byte match |
| Stale docs | Yes | `wave1-implementation-blueprint.md` describes completed work as "READY FOR IMPLEMENTATION" |

### Largest Files (potential future splits)

- `mcp_server.rs` — 2,709 lines (16 legacy + 4 facade tools)
- `advanced.rs` — 1,739 lines (search pipeline)
- `admin.rs` — 1,619 lines (maintenance)
- `mod.rs` (sqlite) — 1,281 lines (storage layer)
- `cli.rs` — 1,251 lines (CLI interface)

### Recommendation

JSONL work can begin immediately. The only future refactor candidate is `mcp_server.rs` splitting once `--mcp-tools=minimal` proves stable, but that's independent of telemetry work.
