---
node: mag.runtime.doctor
status: done
created: 2026-08-07
---
# Recheck doctor state after applying fixes

`mag doctor --fix` computed its final failure result from the original pre-fix
collection. Even when every auto-fix succeeded, the command could still exit
non-zero and report stale failures.

## Acceptance criteria

- [x] Automatic and interactively accepted fixes are followed by a fresh doctor
  check pass.
- [x] Exit status and final readiness text use the post-fix collection.
- [x] Successfully fixed failures do not remain in the final issue count.
- [x] Unresolved or newly observed failures still produce a non-zero result.
- [x] Failed fix actions return their existing actionable error without claiming
  readiness.
- [x] The workflow is covered without downloading real models in the test suite.
- [x] Existing check content, fix actions, interactive behavior, and non-fix
  diagnostics remain compatible.
- [x] Exact-head CI and Cairn pass before merge.

## Implementation evidence

- PR: #428 (`fix(doctor): recheck state after applying fixes`).
- Red head: `b245dcde109938f638ba0ffe2327261602732923`.
  CI run `33161535250` failed at Clippy job `98816920907` with `E0433`
  because the regression test referenced the not-yet-implemented
  `DoctorRunState`.
- Green focused regression: workflow run `33162327211` successfully rewrote the
  final implementation, formatted it, and ran
  `cargo test --quiet --all-features post_fix_pass_disables_a_second_fix_attempt`
  before committing the source change.
- Verified implementation/evidence head:
  `bc58e054b01c06be96d94e3dc44ef412385144a9`.
  - CI run `33162542979`: all jobs passed, including full tests, Rustfmt,
    Clippy, smoke test, npm install, wrappers, installer integrity, version
    consistency, and the benchmark classifier (retrieval benchmark execution was
    correctly skipped because retrieval/scoring paths were unchanged).
  - Cairn architecture gate run `33162542972`: passed.
- The final implementation keeps the existing doctor checks and fix actions in a
  single pass function. `run_doctor` owns only the bounded orchestration: an
  accepted/applied fix returns `Recheck`; the next pass is `PostFix`, cannot offer
  another fix, and alone determines final readiness or failure.
- The stale `hard_failures` re-filter was removed because `failures` is already the
  current pass's failure collection.

## Boundary

This slice corrects doctor control flow only. It does not change model sources,
download behavior, check severity, storage repair, or the set of auto-fixable
actions.
