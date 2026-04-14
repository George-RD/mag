# Benchmark Harness Extension Spec
<!-- Status: DRAFT | Valid for: v0.1.9+ -->

This spec defines the multi-strategy comparison layer for the LoCoMo benchmark harness.
It extends the existing infrastructure without breaking any current workflows.

---

## 1. Strategy Registry (`benches/locomo/strategies.rs`)

### Purpose

Centralise all retrieval-strategy configurations so that `main.rs`, `bench.sh`, and the
Python comparison script share a single source of truth. Strategy selection happens at
**runtime via `--strategy <name>`**, mirroring the existing embedder-selection pattern
(the long if-else chain in `main.rs` lines 281-506).

### File layout

```
benches/locomo/strategies.rs   ← new file, mod declared in main.rs
```

### Public API

```rust
/// Stable identifier used on the CLI and in CSV/JSON outputs.
/// kebab-case, e.g. "sqlite-v1", "sqlite-v1-no-graph".
pub type StrategyId = &'static str;

#[derive(Debug, Clone)]
pub struct StrategyConfig {
    pub id: StrategyId,
    /// One-line human description printed in --list-strategies output.
    pub description: &'static str,
    /// Override graph_neighbor_factor. None = use storage default.
    pub graph_neighbor_factor: Option<f64>,
    /// Disable graph traversal entirely when true.
    pub no_graph: bool,
    /// Disable cross-encoder reranking even when the binary supports it.
    pub no_rerank: bool,
    /// Disable entity-tag extraction during seeding.
    pub no_entity_tags: bool,
    /// Override RRF k constant. None = use storage default.
    pub rrf_k: Option<f64>,
}

/// Ordered registry of all known strategies.
pub const STRATEGIES: &[StrategyConfig] = &[
    StrategyConfig {
        id: "sqlite-v1",
        description: "Reference strategy — production defaults (graph + entity tags, no reranking)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-no-graph",
        description: "Ablation: disables graph traversal to measure its contribution",
        graph_neighbor_factor: Some(0.0),
        no_graph: true,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-no-rerank",
        description: "Ablation: disables cross-encoder reranking (for when --cross-encoder is on)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: true,
        no_entity_tags: false,
        rrf_k: None,
    },
    StrategyConfig {
        id: "sqlite-v1-rrf-tuned",
        description: "Experimental: RRF k=30 (lower k up-weights top results)",
        graph_neighbor_factor: None,
        no_graph: false,
        no_rerank: false,
        no_entity_tags: false,
        rrf_k: Some(30.0),
    },
];

/// Look up a strategy by id. Returns None for unknown names.
pub fn find_strategy(id: &str) -> Option<&'static StrategyConfig> { ... }

/// Print all strategies to stdout in human-readable form.
pub fn list_strategies() { ... }

/// Construct and configure a SqliteStorage instance from a StrategyConfig.
/// Mirrors the per-sample storage setup in main.rs (lines 560-573).
pub fn build_storage(
    cfg: &StrategyConfig,
    embedder: Arc<dyn Embedder>,
    graph_factor_override: Option<f64>, // from --graph-factor CLI flag; wins over cfg
) -> Result<SqliteStorage> { ... }
```

### Integration with `main.rs`

Add to `Args`:

```rust
/// Retrieval strategy to use. Defaults to "sqlite-v1".
/// Use --list-strategies to see all available options.
#[arg(long, default_value = "sqlite-v1")]
strategy: String,

/// List all available strategies and exit.
#[arg(long)]
list_strategies: bool,
```

In `main()`, before the sample loop:
1. If `args.list_strategies`, call `strategies::list_strategies()` and exit 0.
2. Call `strategies::find_strategy(&args.strategy)` — bail with a clear error if unknown.
3. In the per-sample storage setup block, replace the inline `graph_factor` override with
   `strategies::build_storage(cfg, embedder.clone(), args.graph_factor)`.

The `strategy` field must be included in `LoCoMoSummary` (see Section 3).

---

## 2. P95 Latency Tracking (`benches/bench_utils/stats.rs`)

### Purpose

Surface tail-latency behaviour so strategy comparisons can catch regressions that are
invisible in mean query time (e.g. graph traversal spiking on large conversations).

### New file

```
benches/bench_utils/stats.rs
```

Add `pub mod stats;` to `benches/bench_utils/mod.rs`.

### API

```rust
/// Compute the p-th percentile of a slice of millisecond measurements.
/// `percentile` must be in [0.0, 100.0]. Returns 0.0 for empty slices.
///
/// Uses the nearest-rank method (sorted, index = ceil(p/100 * n) - 1).
pub fn percentile_ms(samples: &[u128], percentile: f64) -> f64 { ... }
```

### Integration with `main.rs`

- Accumulate per-question query durations in `Vec<u128>` alongside the existing
  `total_query_ms` accumulator.
- After the sample loop, call `percentile_ms(&query_durations, 95.0)`.
- Store the result in `LoCoMoSummary::p95_query_ms` (see Section 3).

---

## 3. `LoCoMoSummary` Changes (`benches/locomo/types.rs`)

Add two fields to the existing `LoCoMoSummary` struct:

```rust
/// Strategy identifier used for this run (e.g. "sqlite-v1").
pub strategy: String,

/// 95th-percentile per-question query latency in milliseconds.
pub p95_query_ms: f64,
```

`strategy` defaults to `"sqlite-v1"` when the field is populated from the existing
`--strategy` default. Both fields are `#[serde(default)]` so that old JSON outputs
(from CI artifacts) remain deserializable.

No other existing fields change. Serialization order: insert `strategy` after
`scoring_mode`, insert `p95_query_ms` after `avg_query_ms`.

---

## 4. Baseline File (`docs/benchmarks/baselines.json`)

### Purpose

Replace the implicit CSV grep in `bench.sh` (lines 218-219) with an explicit, version-
controlled source of truth. The gate logic becomes deterministic regardless of CSV
history.

### Schema

```jsonc
{
  "schema_version": 1,
  // Top-level keys are strategy IDs. Each strategy holds per-scoring-mode baselines.
  "sqlite-v1": {
    "word-overlap": {
      "samples": 10,
      "overall": 90.1,
      "single_hop": 86.9,
      "temporal": 85.0,
      "multi_hop": 56.2,
      "open_domain": 95.7,
      "adversarial": 92.6,
      "evidence_recall": 92.0,
      "avg_query_ms": 7.0,
      "p95_query_ms": null,       // null until first --update-baseline run
      "embedder": "bge-small-en-v1.5 (onnx, 384-dim)",
      "commit": "83abccf",
      "date": "2026-03-28",
      "notes": "validated 10-sample; source: methodology.md"
    },
    "e2e-word-overlap": {
      "samples": 2,
      "overall": 91.2,
      // ... category fields ...
      "embedder": "bge-small-en-v1.5 (onnx, 384-dim)",
      "commit": "83abccf",
      "date": "2026-03-28",
      "notes": "gpt-5.4, 2-sample; source: methodology.md"
    }
  }
  // Additional strategies added as they are validated.
}
```

### Append-only contract

- Entries are **never deleted or overwritten** by tooling.
- `--update-baseline` in `bench.sh` and `compare_strategies.py` appends a new key
  (e.g. `"sqlite-v1-no-graph"`) or replaces a scoring-mode sub-object for an existing
  strategy. A new timestamp + commit are written; the old entry is left commented out
  via a `"_superseded_YYYY-MM-DD"` sibling key pattern (JSON does not support
  comments, so the prior value is preserved under a prefixed key).
- CI never writes to `baselines.json`; only humans and explicit `--update-baseline`
  invocations do.

### Gate logic replacement in `bench.sh`

Replace the current grep-based baseline lookup:

```bash
# OLD (fragile — depends on CSV row ordering):
BASELINE=$(grep ",10," "${RESULTS_CSV}" | grep "bge-small" | tail -1 | cut -d',' -f8)

# NEW:
BASELINE=$(python3 -c "
import json, sys
with open('${REPO_DIR}/docs/benchmarks/baselines.json') as f:
    db = json.load(f)
strategy = '${STRATEGY:-sqlite-v1}'
mode = '${SCORING_MODE}'
try:
    print(db[strategy][mode]['overall'])
except KeyError:
    print('')
")
```

---

## 5. Comparison Mode (`bench.sh --compare A B` and `scripts/compare_strategies.py`)

### `bench.sh` interface extension

```bash
# New flag parsed before the model/samples block:
--compare    STRATEGY_A STRATEGY_B   # run both, produce ComparisonReport
--update-baseline                    # after a run, write result to baselines.json
```

Example invocations:

```bash
./scripts/bench.sh --compare sqlite-v1 sqlite-v1-no-graph --samples 10
./scripts/bench.sh --compare sqlite-v1 sqlite-v1-rrf-tuned --scoring-mode word-overlap
```

When `--compare` is active, `bench.sh`:
1. Runs `locomo_bench --strategy A --json` and captures JSON output.
2. Runs `locomo_bench --strategy B --json` and captures JSON output.
3. Calls `scripts/compare_strategies.py --a-json <path> --b-json <path>
   --baselines docs/benchmarks/baselines.json --output-dir docs/benchmarks/comparisons/`
4. Appends **two** CSV rows to `benchmark_log.csv` — one per strategy — with
   `strategy=<id>` included in the `notes` field (space-separated key=value tokens).
5. Prints the Markdown comparison report to stdout.

### `scripts/compare_strategies.py`

**Python 3 stdlib only — zero pip dependencies.**

```
python3 scripts/compare_strategies.py \
    --a-json /tmp/result_a.json \
    --b-json /tmp/result_b.json \
    --baselines docs/benchmarks/baselines.json \
    --output-dir docs/benchmarks/comparisons/ \
    [--update-baseline]
```

#### Output artefacts

`docs/benchmarks/comparisons/YYYY-MM-DD_<A>_vs_<B>/`

```
comparison_report.md    # Human-readable table
comparison_report.json  # Machine-readable ComparisonReport
```

#### `ComparisonReport` JSON schema

```jsonc
{
  "generated_at": "2026-04-14T12:00:00Z",
  "strategy_a": "sqlite-v1",
  "strategy_b": "sqlite-v1-no-graph",
  "scoring_mode": "word-overlap",
  "samples": 10,
  "embedder": "bge-small-en-v1.5 (onnx, 384-dim)",
  "verdict": "BETTER",        // BETTER | EQUIVALENT | REGRESSION | INCONCLUSIVE
  "verdict_reason": "...",
  "baseline_used": {          // null if no baseline for strategy_a
    "overall": 90.1,
    "samples": 10
  },
  "results": {
    "strategy_a": {
      "overall": 90.1,
      "single_hop": 86.9,
      "temporal": 85.0,
      "multi_hop": 56.2,
      "open_domain": 95.7,
      "adversarial": 92.6,
      "evidence_recall": 92.0,
      "avg_query_ms": 7.0,
      "p95_query_ms": 18.5
    },
    "strategy_b": { ... }
  },
  "deltas": {                 // strategy_b minus strategy_a (positive = B wins)
    "overall": -3.2,
    "single_hop": -1.1,
    // ...
    "avg_query_ms": -0.5,
    "p95_query_ms": -2.1
  }
}
```

#### Verdict logic

| Condition | Verdict |
|-----------|---------|
| B overall delta >= +0.5pp and no category regresses > 2pp | `BETTER` |
| Abs(B delta) < 0.5pp across all categories | `EQUIVALENT` |
| B overall delta <= -2pp (hard threshold) | `REGRESSION` |
| Mixed category results; overall delta -2pp to +0.5pp | `INCONCLUSIVE` |

The thresholds are constants at the top of the script so they can be tuned without
touching the algorithm.

#### Markdown report format

```markdown
# Strategy Comparison: sqlite-v1 vs sqlite-v1-no-graph

- Date: 2026-04-14
- Scoring mode: word-overlap
- Samples: 10
- Embedder: bge-small-en-v1.5 (onnx, 384-dim)
- Verdict: **INCONCLUSIVE** — mixed category results; overall delta within noise floor

## Score Comparison

| Category      | sqlite-v1 | sqlite-v1-no-graph | Delta |
|---------------|----------:|-------------------:|------:|
| Single-Hop QA |     86.9% |              85.8% |  -1.1 |
| Temporal      |     85.0% |              83.4% |  -1.6 |
| Multi-Hop QA  |     56.2% |              52.1% |  -4.1 |
| Open-Domain   |     95.7% |              95.9% |  +0.2 |
| Adversarial   |     92.6% |              92.4% |  -0.2 |
| **Overall**   | **90.1%** |          **87.9%** | **-2.2** |
| Evidence Rec  |     92.0% |              91.5% |  -0.5 |

## Latency

| Metric        | sqlite-v1 | sqlite-v1-no-graph | Delta |
|---------------|----------:|-------------------:|------:|
| Avg query ms  |       7.0 |                6.5 |  -0.5 |
| P95 query ms  |      18.5 |               16.4 |  -2.1 |

## Baseline comparison (sqlite-v1 reference)

Baseline: 90.1% (10-sample, 2026-03-28, commit 83abccf)
Current A: 90.1% — within 0.0pp of baseline. Gate: PASS
```

---

## 6. CI Integration (`.github/workflows/ci.yml`)

### New job: `benchmark-compare`

Add after the existing `benchmark` job:

```yaml
benchmark-compare:
  name: Strategy Comparison
  runs-on: ubuntu-latest
  if: github.event_name == 'pull_request'
  steps:
    - uses: actions/checkout@v6

    - name: Detect strategy/baseline changes
      id: filter
      uses: dorny/paths-filter@v3
      with:
        filters: |
          strategies:
            - 'benches/locomo/strategies.rs'
            - 'docs/benchmarks/baselines.json'

    - name: Install Rust
      if: steps.filter.outputs.strategies == 'true'
      uses: dtolnay/rust-toolchain@stable

    - name: Cache Rust build artifacts
      if: steps.filter.outputs.strategies == 'true'
      uses: Swatinem/rust-cache@v2

    - name: Run strategy comparison
      if: steps.filter.outputs.strategies == 'true'
      run: |
        ./scripts/bench.sh --compare sqlite-v1 sqlite-v1-no-graph \
          --samples 2 --scoring-mode word-overlap

    - name: Upload comparison report
      if: steps.filter.outputs.strategies == 'true'
      uses: actions/upload-artifact@v4
      with:
        name: strategy-comparison-${{ github.sha }}
        path: docs/benchmarks/comparisons/
        retention-days: 30
```

### Notes

- `--samples 2` keeps the CI run under 5 minutes on a cold Rust cache.
- The job is **additive** — it does not replace the existing `benchmark` job. Both run
  independently on PRs.
- The `dorny/paths-filter` version matches what the existing `benchmark` job already
  uses (`@v3`), avoiding a second copy of the action.
- No secrets are required; the comparison uses the local ONNX embedder.

---

## 7. Notes Field Extension for `benchmark_log.csv`

No schema change is required. The existing `notes` (column 16) is free text. The
convention is to embed space-separated `key=value` tokens for machine parseability:

```
strategy=sqlite-v1 compare_with=sqlite-v1-no-graph
```

`bench.sh` in `--compare` mode appends this automatically. Manual runs can add it
via `--notes`.

The `print_table()` function in `bench.sh` does not need to change; the notes column
is already present in the CSV and displayed in the table when non-empty. Future tooling
can filter on `strategy=` tokens using awk.

---

## 8. Implementation Sequence

Implement in this order to keep the repo in a working state at each step:

### Step 1 — `stats.rs` (isolated, zero-risk)

Create `benches/bench_utils/stats.rs` with `percentile_ms()`. Add `pub mod stats;` to
`benches/bench_utils/mod.rs`. Add unit tests (empty slice, single element, known
distribution). No changes to any existing code yet.

### Step 2 — `LoCoMoSummary` additions

Add `strategy: String` and `p95_query_ms: f64` to `LoCoMoSummary` with
`#[serde(default)]`. Update `main.rs` to populate both fields (accumulate
`query_durations: Vec<u128>`, call `percentile_ms`, pass `"sqlite-v1"` as the hardcoded
strategy string for now). Run `cargo test` and a `--json` bench invocation to verify
JSON output.

### Step 3 — `strategies.rs`

Create `benches/locomo/strategies.rs` with `STRATEGIES`, `find_strategy()`,
`list_strategies()`, and `build_storage()`. Declare `mod strategies;` in `main.rs`.
Add `--strategy` and `--list-strategies` flags to `Args`. Wire `build_storage()` into
the per-sample loop, replacing the inline `graph_factor` override block. The
`"sqlite-v1"` default must produce identical output to the pre-patch behaviour — verify
by running a 2-sample word-overlap bench and comparing scores.

### Step 4 — `baselines.json`

Create `docs/benchmarks/baselines.json` with the `sqlite-v1` `word-overlap` and
`e2e-word-overlap` entries populated from `methodology.md` values. Update `bench.sh`
gate logic to read from the JSON file instead of CSV grep. Run `./scripts/bench.sh
--gate` locally to confirm the gate still passes.

### Step 5 — `compare_strategies.py`

Write `scripts/compare_strategies.py`. Test locally:

```bash
cargo run --release --bin locomo_bench -- --strategy sqlite-v1 --samples 2 \
    --scoring-mode word-overlap --json > /tmp/a.json
cargo run --release --bin locomo_bench -- --strategy sqlite-v1-no-graph --samples 2 \
    --scoring-mode word-overlap --json > /tmp/b.json
python3 scripts/compare_strategies.py \
    --a-json /tmp/a.json --b-json /tmp/b.json \
    --baselines docs/benchmarks/baselines.json \
    --output-dir /tmp/comparison-test/
```

Verify JSON and Markdown outputs are well-formed.

### Step 6 — `bench.sh --compare`

Extend `bench.sh` to parse `--compare A B` and `--update-baseline`. Wire the two
`locomo_bench` invocations and the Python script call. Test the full pipeline locally.

### Step 7 — CI job

Add the `benchmark-compare` job to `.github/workflows/ci.yml`. Open a draft PR that
touches `benches/locomo/strategies.rs` to confirm the path filter triggers correctly
and the artifact upload works.

---

## Appendix A: File Inventory

| Path | Status | Description |
|------|--------|-------------|
| `benches/locomo/strategies.rs` | new | Strategy registry |
| `benches/bench_utils/stats.rs` | new | Percentile helper |
| `benches/bench_utils/mod.rs` | modify | Add `pub mod stats;` |
| `benches/locomo/main.rs` | modify | `--strategy`, `--list-strategies`, p95 accumulation |
| `benches/locomo/types.rs` | modify | `strategy` + `p95_query_ms` fields |
| `scripts/bench.sh` | modify | `--compare`, `--update-baseline`, JSON gate |
| `scripts/compare_strategies.py` | new | Comparison + report generator |
| `docs/benchmarks/baselines.json` | new | Per-strategy per-mode baselines |
| `docs/benchmarks/comparisons/` | new dir | Comparison report artefacts (gitignored or tracked) |
| `.github/workflows/ci.yml` | modify | `benchmark-compare` job |

## Appendix B: Key Invariants

1. **No Cargo features added.** All strategy selection is runtime CLI flags. The build
   matrix stays identical to today.
2. **sqlite-v1 parity.** Running with `--strategy sqlite-v1` (the default) must produce
   scores within floating-point rounding of the current unpatched binary. This is the
   correctness gate for Step 3.
3. **bench.sh remains standalone.** The Python script is invoked as a subprocess; it is
   never imported. `bench.sh` continues to work without `--compare` exactly as it does
   today.
4. **baselines.json is append-only by convention.** CI reads it but never writes it.
   Only `--update-baseline` writes it, and only a human merges that change.
5. **CSV format unchanged.** No new columns are added; the `notes` field carries the
   `strategy=` token as free text. Existing tooling that reads the CSV is unaffected.
