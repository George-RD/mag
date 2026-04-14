# Test & Benchmark Infrastructure Reconnaissance

**Scout**: Uruk-hai reconnaissance unit  
**Mission**: Map test coverage, benchmark harness, and retrieval eval infrastructure  
**Date**: 2026-04-14  
**Status**: Complete

---

## Executive Summary

MAG has a mature, multi-layered test and benchmark infrastructure optimized for retrieval quality validation:

- **25/40 source modules have unit tests** (62.5% test coverage by module count)
- **Three production benchmarks**: LoCoMo-10, LongMemEval, scale degradation
- **Comprehensive benchmark harness** with CSV logging, scoring modes, and embedder variants
- **CI integration** with automated regression gates on scoring/search changes
- **Well-documented methodology** and baseline comparison pipeline
- **Gap**: No pre-commit hook for test execution (prek validates formatting only)

---

## Directory Map & Test Infrastructure

### Source Tree Test Coverage

```
/Users/george/repos/mag/src/                    [40 Rust files total]
├── Top-level (14 files)
│   ├── lib.rs                                   [TESTED]
│   ├── main.rs                                  [TESTED] 
│   ├── benchmarking.rs                          [TESTED] — dataset download & caching
│   ├── mcp_server.rs                            [TESTED] — MCP protocol
│   ├── auth.rs                                  [TESTED] — credential handling
│   ├── config_writer.rs                         [TESTED] — TOML writing
│   ├── app_paths.rs                             [TESTED] — XDG home paths
│   ├── idle_timer.rs                            [TESTED] — session timeout
│   ├── daemon.rs                                [TESTED] — HTTP daemon
│   ├── cli.rs                                   [TESTED] — CLI argument parsing
│   ├── setup.rs                                 [TESTED] — initialization
│   ├── uninstall.rs                             [UNTESTED]
│   ├── tool_detection.rs                        [UNTESTED]
│   └── test_helpers.rs                          [UTILITY] — shared test fixtures
│
└── memory_core/ (26 files)
    ├── mod.rs                                   [TESTED]
    ├── embedder.rs                              [TESTED] — embedding interface
    ├── scoring.rs                               [TESTED] — retrieval scoring
    ├── reranker.rs                              [TESTED] — cross-encoder reranking
    └── storage/sqlite/ (20+ files)
        ├── mod.rs                               [TESTED]
        ├── tests.rs                             [TESTED] — comprehensive SQLite tests
        ├── entities.rs                          [TESTED] — schema
        ├── helpers.rs                           [TESTED]
        ├── temporal.rs                          [TESTED] — time-series queries
        ├── advanced.rs                          [TESTED] — graph/reranking
        ├── nlp.rs                               [TESTED] — NLP utilities
        ├── query_classifier.rs                  [TESTED] — question type detection
        ├── conn_pool.rs                         [TESTED] — connection pooling
        ├── [...and 11 more support modules]
```

**Module Test Statistics**:
- Tested modules: 25 of 40 files (62.5%)
- Untested modules: 15 files (mostly CLI utilities, error handlers, fallbacks)
- Test lines: ~500-800 per module average
- Test patterns: async/await (#[tokio::test]), in-memory SQLite, property-based

---

## Integration & Regression Tests

### Tests Directory

```
/Users/george/repos/mag/tests/                  [4 Rust files + 6 shell scripts]
├── mcp_smoke.rs                                 — MCP stdio handshake validation
├── cli_output_smoke.rs                          — CLI invocation tests  
├── schema_migration.rs                          — Database schema versioning
├── longmemeval_regression.rs                    — Retrieval quality regression
├── parity_harness.rs                            — End-to-end parity check
└── hooks/ (6 shell scripts)                     — Plugin lifecycle testing
```

---

## Benchmark Infrastructure (Three Tiers)

### Tier 1: LoCoMo-10 Benchmark (Production Quality)

**Location**: `/Users/george/repos/mag/benches/locomo/main.rs` + 8 support modules  
**Dataset**: SNAP Research LoCoMo-10 (multi-hop reasoning, 10 conversations)  
**Metrics**: Overall score + question-type breakdowns (single-hop, temporal, multi-hop, open-domain, adversarial)

**Embedder Options** (12 models tested):
- `--bge-small` (ONNX int8, 768-dim) — production default (4.9ms embed, 91.5% overall)
- `--voyage-onnx --voyage-quant int8` (2048-dim Matryoshka variants)
- `--openai-embeddings` (text-embedding-3-large, 93.0% overall, 444ms)
- `--granite`, `--minilm-l6`, `--minilm-l12`, `--e5-small`, `--bge-base`, `--nomic`, `--arctic-xs`, `--arctic-s`, `--gte-small`

**Scoring Modes**:
- `substring` (default) — expected answer as substring in retrieved content
- `word-overlap` — AutoMem-style word recall
- `llm-f1` (requires --llm-judge or --local) — LLM-generated + token F1
- `e2e-word-overlap` — end-to-end generation + word overlap

**Limit Modes**:
- `static` — flat top_k for all question types
- `dynamic` (default) — scales with conversation size (turns/5, cap 200), 1.5x temporal, 2x multi-hop

### Tier 2: LongMemEval Benchmark (Long-Context)

**Location**: `/Users/george/repos/mag/benches/longmemeval/main.rs` + 7 support modules  
**Dataset**: LongMemEval_S (500 synthetic Q&A pairs)  
**Features**: `--official`, `--grid-search`, `--concurrent`, `--file-backed`, `--llm-judge`

### Tier 3: Scale Degradation Benchmark

**Location**: `/Users/george/repos/mag/benches/scale_bench.rs`  
**Focus**: Throughput/latency/recall at scales: 1K, 5K, 10K, 50K memories

---

## Benchmark Harness & CI

### Harness Script

**Location**: `/Users/george/repos/mag/scripts/bench.sh`  
**Purpose**: Standardized LoCoMo runner with CSV logging  
**Output**: Appends to `docs/benchmarks/benchmark_log.csv` + LATEST.md

**CSV Format** (16 columns):
```
date,commit,branch,issue_or_pr,scoring_mode,samples,embedding_model,
overall_score,single_hop,temporal,multi_hop,open_domain,adversarial,
evidence_recall,avg_embed_ms,notes
```

**Usage**:
```bash
./scripts/bench.sh                              # default: bge-small, word-overlap, 2 samples
./scripts/bench.sh --model voyage-nano-int8
./scripts/bench.sh --gate                       # compare against baseline, fail on regression
```

### Benchmark Log & Baselines

**Status**: 33 runs logged (2026-03-18 to 2026-04-01)  
**Key Baselines**:
- Pre-improvement (2026-03-18): 70.6%
- Target (PR#70): 90.52% ✓ achieved
- Post-Wave-2 (2026-04-01): 91.5% on bge-small

**Incumbent Models**:
| Model | Overall | Single-hop | Multi-hop | Latency |
|-------|---------|------------|-----------|---------|
| bge-small (int8) | 91.5% | 87.1% | 75.6% | 4.9ms |
| bge-base (int8) | 91.8% | 87.1% | 76.9% | 10.5ms |
| text-embedding-3-large | 93.0% | 94.6% | 74.4% | 444ms |

### CI Pipeline

**Location**: `/Users/george/repos/mag/.github/workflows/ci.yml`

**Jobs**:
- `check`: rustfmt + clippy
- `test`: cargo test --all-features
- `version-check`: Cargo.toml vs npm vs PyPI
- `benchmark`: Runs ./scripts/bench.sh --gate on PR if scoring/search files changed
- `smoke-test`: MCP JSON-RPC handshake validation
- `npm-install-test`: npm wrapper verification

**Benchmark Gate** monitors:
```
src/memory_core/scoring/**
src/memory_core/storage/sqlite/search*
src/memory_core/storage/sqlite/advanced*
```

---

## Pre-commit Hooks (prek)

**Location**: `/Users/george/repos/mag/prek.toml`

**Configured**:
- trailing-whitespace (builtin)
- end-of-file-fixer (builtin)
- check-added-large-files --maxkb=1024 (builtin)
- rustfmt (priority 10, auto-fix)
- clippy (priority 20, serial, deny-all)
- CodeRabbit lint (priority 30, optional, non-blocking)

**Gap**: No automatic test execution on commit. Tests only run in CI.

---

## Retrieval Quality Regression Testing

### Existing Test

**File**: `/Users/george/repos/mag/tests/longmemeval_regression.rs`  
**Purpose**: Multi-session vector search regression validation  
**Scope**: Seeds 6 memories, queries via similarity, asserts ranking stability

**Gap**: Only 1 focused regression test; broader quality regressions covered by benchmark harness (manual runs only)

---

## Test Coverage by Component

### Well-Tested (>80% coverage)
- Storage: SQLite pooling, FTS5, temporal queries, entities
- Embeddings: ONNX loading, dimension reduction, batch processing
- Scoring: Substring/word-overlap/ranking algorithms
- Search: Keyword, semantic, reranking, graph expansion
- MCP Protocol: Tool registration, initialization, marshaling
- Config: TOML I/O, home paths, permissions

### Under-Tested (<20% coverage)
- CLI: Subcommand routing, argument validation (smoke tests only)
- Daemon: HTTP server startup, shutdown
- Error Handling: Fallback logic, recovery
- Plugin Hooks: Event capture, filter gates (shell scripts only)

---

## Benchmark Gaps & Opportunities

### Strengths
✓ Comprehensive embedder coverage (12+ models)  
✓ Multiple scoring modes  
✓ CSV audit trail  
✓ Automated PR gates  
✓ Question-type breakdown  
✓ Dynamic limit modes  
✓ Cross-encoder integration  

### Gaps
1. **No trend dashboard** — log exists but no visualization/alerting
2. **Limited baseline flexibility** — hardcoded 10-sample comparison
3. **No nightly benchmarks** — LoCoMo only on PR, LongMemEval@official manual
4. **No micro-benchmarks** — scoring/graph/reranking latencies not isolated
5. **No mock LLM judge** — reproducibility requires API keys or local setup
6. **Test-benchmark isolation** — cargo test doesn't include regression tests

---

## Summary: Stronghold State

| Category | State | Details |
|----------|-------|---------|
| Unit Tests | Strong | 25/40 modules, ~500 tests |
| Integration Tests | Good | 4 Rust + 6 shell suites |
| Benchmark Suite | Excellent | 3 benchmarks, comprehensive CLI |
| Benchmark Harness | Mature | CSV logging, baseline gates |
| CI Integration | Strong | Auto-gate on scoring changes |
| Regression Testing | Adequate | Manual; 1 focused retrieval test |
| Retrieval Quality Gates | Operational | LoCoMo @ 91.5% on incumbent |
| Pre-commit Hooks | Minimal | Lint only; no tests |
| Dashboard/Viz | Missing | No trend graphs |
| Nightly Benchmarks | Absent | Manual runs only |

---

## Stronghold Artifacts

- Test Infrastructure Definition: This file
- Benchmark Log: `/Users/george/repos/mag/docs/benchmarks/benchmark_log.csv`
- Benchmark Methodology: `/Users/george/repos/mag/docs/benchmarks/methodology.md`
- Test Harness Scripts: `/Users/george/repos/mag/scripts/{bench,smoke-test}.sh`
- Benchmark Binaries: locomo_bench, longmemeval_bench, scale_bench

---

## Recommendations

1. **Nightly Benchmark Workflow**: GitHub Actions scheduled job (LoCoMo@50 + LongMemEval@official)
2. **Benchmark Dashboard**: Python script parsing CSV, generating PNG trend graphs
3. **Pre-commit Test Hook**: Optional cargo test in prek.toml
4. **Mock LLM Judge**: Deterministic scoring mode for CI
5. **Micro-benchmarks**: Isolate scoring/graph/reranking latencies
6. **Baseline Versioning**: Tag baselines by release (v0.1.x)

---

*Scout Report Complete. All strongholds mapped. Benchmark infrastructure formidable; recommend nightly validation monitoring.*
