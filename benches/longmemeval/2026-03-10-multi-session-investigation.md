# Multi-Session Retrieval Investigation

Date: 2026-03-10

Command run:

```bash
cargo run --release --bin longmemeval_bench -- --verbose
```

Observed state before tuning:

- Multi-session: `18/20`
- Misses:
  - `OMEGA vector search implementation` expected `sqlite-vec extension`
  - `database migration in CI/CD` expected `migration step`

Root cause:

- Both misses were task-completion memories with strong lexical coverage of the query.
- Broader decision memories on the same topic were outranking them because the current score stack favored general semantic/topic matches once they entered the fused candidate set.
- The system already had a linear word-overlap boost, but it did not separate `matches most of the query` from `matches a couple of broad topic words` strongly enough.

Fix:

- Added `query_coverage_weight` to `ScoringParams`.
- Added `query_coverage_boost()` in `src/memory_core/scoring.rs`.
- Applied the boost during advanced-search score refinement in `src/memory_core/storage/sqlite/advanced.rs`.
- Added focused regressions in `tests/longmemeval_regression.rs` for the two failed multi-session queries.

Expected effect:

- High-coverage memories, especially task-completion entries that mention most query terms, get a stronger boost without globally retuning event-type weights.
- This keeps the fix local to retrieval ranking instead of making task completions universally heavier than decisions.
