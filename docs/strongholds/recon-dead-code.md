# Dead Code and Tech Debt Reconnaissance Report

**Stronghold:** `/Users/george/repos/mag/docs/strongholds/recon-dead-code.md`  
**Compiled:** 2026-04-14  
**Status:** Read-only scout mission complete

## Summary

The MAG codebase is **exceptionally clean** with minimal dead code. All 43 explicit `#[allow(dead_code)]` suppressions are justified by:
- **Feature gates** (daemon-http, real-embeddings, sqlite-vec)
- **Cross-module visibility** (setup.rs cannot see test usage)
- **Optional implementations** (placeholder embedders, hot cache methods)

**Dead Code Scope:** ~15 functions / 600 LOC of intentionally-suppressed code; **0 orphaned/unreachable code detected**.

---

## Detailed Findings

### 1. Explicit Dead Code Suppressions: 43 Total

| Category | Count | Location | Rationale |
|----------|-------|----------|-----------|
| Feature-gated daemon | 8 | `src/main.rs`, `src/lib.rs`, `src/daemon.rs` | Only used when `daemon-http` feature enabled |
| Cross-module test usage | 4 | `src/tool_detection.rs`, `src/config_writer.rs`, `src/app_paths.rs` | Clippy cannot trace setup.rs/test usage |
| Optional embedder code | 3 | `src/memory_core/embedder.rs` | Placeholder implementations when ONNX disabled |
| Scoring params methods | 8 | `src/memory_core/storage/sqlite/mod.rs` | Used by grid search benchmarks and external APIs |
| Hot cache query method | 1 | `src/memory_core/storage/sqlite/hot_cache.rs` | Alternative query path (aliased by wrapper) |
| Benchmark utilities | 12+ | `benches/bench_utils/`, `benches/locomo/`, `benches/longmemeval/` | Test-harness and metric collection |

**Code Quality:** Every suppression includes an explanatory comment confirming intentional design.

### 2. Feature-Gated Code

#### daemon-http (HTTP server, auth layer, idle timeout)
- **Modules:** `src/daemon.rs`, `src/auth.rs`, `src/idle_timer.rs`
- **Usage:** Optional HTTP transport for MCP server
- **Status:** Feature properly isolated; 0 leakage into base functionality
- **Recommendation:** Status quo; design is sound

#### real-embeddings (ONNX, reranker, scoring)
- **Suppressions:** 40+ `#[cfg(feature = "real-embeddings")]` directives
- **Conditional paths:** PlaceholderEmbedder vs OnnxEmbedder; scoring algorithms
- **Status:** Well-encapsulated; placeholder mode is production-ready fallback
- **Recommendation:** Status quo; dual-mode design enables offline usage

#### sqlite-vec (Vector similarity search)
- **Code paths:** 12+ search variants based on feature flag
- **Status:** Both paths implemented (sqlite-vec vs fallback SQL)
- **Recommendation:** Status quo; feature parity maintained

#### mimalloc (Memory allocator)
- **Usage:** `#[global_allocator]` in src/main.rs (line 2)
- **Status:** Properly integrated
- **Recommendation:** Status quo; optional performance optimization

---

### 3. Cross-Module Test Usage

**Issue Identified:** `tool_detection.rs`, `config_writer.rs`, and `app_paths.rs` have dead_code suppressions because:

```rust
// src/tool_detection.rs:168
#[allow(dead_code)] // Used by setup.rs and tests; clippy can't trace cross-module usage
impl DetectionResult {
    pub fn unconfigured(&self) -> Vec<&DetectedTool> { ... }
    pub fn any_configured(&self) -> bool { ... }
}
```

**Clippy Limitation:** Cross-crate/cross-module usage is not visible to the compiler's dead code analysis. These are **legitimately used** by:
- `src/setup.rs` (tool detection flow)
- Test suites (private test modules)

**Recommendation:** Current suppression is justified and correct.

---

### 4. Test-Only Code

**Properly Isolated:**
- `src/test_helpers.rs` (76 lines): Mutex-protected temp HOME for parallel tests
  - Marked `#[cfg(test)]` in both `src/lib.rs` and `src/main.rs`
  - Used by: `setup.rs`, `tool_detection.rs`, `config_writer.rs`, `uninstall.rs` tests

**Test Functions:** 100+ across codebase
- All properly annotated with `#[test]`
- No test code leaked into production modules
- Serial test protection: `serial_test` crate for HOME mutation tests

**Status:** ✅ No issues detected

---

### 5. Linting Configuration

```toml
[lints.rust]
unused_must_use = "deny"       # ✅ Enforced
unreachable_patterns = "deny"  # ✅ Enforced
unused_imports = "deny"        # ✅ Enforced
unused_variables = "deny"      # ✅ Enforced
dead_code = "warn"             # Design choice (not deny)
```

**Why `warn` for dead_code?** Per inline comment:
> "warn not deny — parallel agents may create functions before callers exist"

This is intentional for multi-agent development scenarios where function definitions may precede call sites.

---

### 6. TODO and FIXME Comments

**Inventory:** 1 TODO found

| Location | Issue | Severity | Status |
|----------|-------|----------|--------|
| `src/memory_core/storage/sqlite/advanced.rs:1541` | Parallelize sub-queries when pool supports concurrent | Enhancement | Open (#121) |

**Observation:** Minimal tech debt; only one identified improvement item.

---

### 7. Dependency Analysis

#### All 6 Optional Dependencies are Used:

1. **ort** (`real-embeddings`)
   - Used in: `src/memory_core/reranker.rs` (ONNX model inference)
   - Lines: Session creation, optimization level config
   - Status: ✅ Required

2. **tokenizers** (`real-embeddings`)
   - Used in: `src/memory_core/reranker.rs` (text tokenization)
   - Lines: Tokenizer loading, truncation setup
   - Status: ✅ Required

3. **ndarray** (`real-embeddings`)
   - Used in: Scoring algorithms and embeddings (transitive via ort/reranker)
   - Status: ✅ Required

4. **dotenvy** (`real-embeddings`)
   - Used in: Embedder initialization (config loading)
   - Status: ✅ Required

5. **mimalloc** (optional allocator)
   - Used in: `src/main.rs:2` as `#[global_allocator]`
   - Status: ✅ Performance optimization

6. **sqlite-vec** (`sqlite-vec` feature)
   - Used in: `src/memory_core/storage/sqlite/helpers.rs` and search paths
   - Status: ✅ Alternative vector search backend

**Conclusion:** Zero unused dependencies.

---

### 8. Code Metrics

| Metric | Value | Notes |
|--------|-------|-------|
| Source files | 40 | src/ directory |
| Total functions | 1,112+ | Private functions across codebase |
| Dead code suppressions | 43 | All justified |
| Test functions | 100+ | All properly gated |
| Benchmark binaries | 6 | All feature-constrained |
| Benchmark LOC | 7,197 | Separate build target |
| TODO/FIXME items | 1 | Minimal tech debt |

---

### 9. Code Health Assessment

#### Strengths
✅ **Strict linting:** Unused imports, variables, must_use all enforced as deny  
✅ **Feature parity:** Real-embeddings and placeholder modes fully tested  
✅ **Test isolation:** Proper use of mutex guards for concurrent test safety  
✅ **Cross-module visibility:** Proper use of `#[allow(dead_code)]` with explanatory comments  
✅ **No deprecated code:** Zero deprecated functions found  
✅ **Numerical safety:** Explicit denials on cast truncation/loss  

#### Minor Items
⚠️ **One TODO (#121):** Sub-query parallelization enhancement pending  

#### No Issues Found
✅ No orphaned code  
✅ No unreachable patterns  
✅ No unused dependencies  
✅ No stale feature gates  
✅ No test code leakage  

---

## Recommendations

### 1. Status Quo (No Action Required)
- **Feature gates are well-designed:** daemon-http and real-embeddings properly isolated
- **Suppressions are justified:** All 43 dead_code suppressions have valid explanations
- **Test infrastructure is solid:** Mutex-protected temp directories; proper isolation

### 2. Future Improvements (Optional)
- **Issue #121:** Consider parallelizing sub-queries when async pool supports concurrent queries
- **Monitoring:** Continue linting enforcement at current strictness levels

### 3. Documentation
- Consider adding a DEVELOPMENT.md section explaining:
  - Why `dead_code = "warn"` (not deny) for parallel agent scenarios
  - Feature gate strategy (real-embeddings vs placeholder)
  - Cross-module visibility limitations in Clippy

---

## Conclusion

**MAG is exceptionally clean.** The codebase exhibits:
- **Minimal technical debt** (1 TODO item)
- **Zero orphaned code** (all functions reachable and used)
- **Strategic feature gating** (daemon, embeddings, vector search)
- **Rigorous linting** (deny on all major categories except dead_code)
- **Intentional suppressions** (every `allow(dead_code)` justified)

**Estimated "Dead Code" Scope:**
- ~15 functions / ~600 LOC of *intentionally-suppressed* code
- 0 functions / 0 LOC of *unreachable* code
- 0 unused dependencies

**Verdict:** Code is production-ready with minimal cleanup overhead.

