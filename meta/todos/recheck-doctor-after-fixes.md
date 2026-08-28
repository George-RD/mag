---
node: mag.runtime.doctor
status: in_progress
created: 2026-08-07
---
# Recheck doctor state after applying fixes

`mag doctor --fix` currently computes its final failure result from the original
pre-fix collection. Even when every auto-fix succeeds, the command can still exit
non-zero and report the stale failures.

## Acceptance criteria

- [ ] Automatic and interactively accepted fixes are followed by a fresh doctor
  check pass.
- [ ] Exit status and final readiness text use the post-fix collection.
- [ ] Successfully fixed failures do not remain in the final issue count.
- [ ] Unresolved or newly observed failures still produce a non-zero result.
- [ ] Failed fix actions return their existing actionable error without claiming
  readiness.
- [ ] The workflow is covered without downloading real models in the test suite.
- [ ] Existing check content, fix actions, interactive behavior, and non-fix
  diagnostics remain compatible.
- [ ] Exact-head CI and Cairn pass before merge.

## Boundary

This slice corrects doctor control flow only. It does not change model sources,
download behavior, check severity, storage repair, or the set of auto-fixable
actions.
