---
node: mag.runtime.memory
review_type: agent_introspective
date: 2026-09-07
reviewer: ChatGPT
---

# PR #437: ONNX session-builder compatibility

Scope: continue the existing dependency PR rather than open a new roadmap task.
The reviewed source and tests are at `c552271cd8c36bff2779ad602dcdf57b20ef7be3`.
This does not complete or change the scope of the existing
[embedding-space migration todo](../todos/implement-embedding-space-migration.md).

## Regression and cause

The original PR head, `22829f169296787e97c0a8cf77dd7f1792b99073`, fails
`cargo test --quiet --all-features` in
[CI run 33443724905, Test job 99657764624](https://github.com/George-RD/mag/actions/runs/33443724905/job/99657764624).
The compiler reports E0277 at the execution-provider, thread-count, and
optimization-level builder calls in both `embedder.rs` and `reranker.rs`.
This existing failing build/test target is the red regression boundary.

With `ort` rc.13 these operations return `ort::Error<SessionBuilder>`.
The recovery builder contains non-Send/Sync state and cannot be stored directly
in `anyhow::Error`. Upstream supplies
`From<Error<SessionBuilder>> for Error<()>`, which drops only the recovery
payload and retains the error's code, message, and source.

Primary implementation references:

- [ort rc.13 error conversion](https://github.com/pykeio/ort/blob/v2.0.0-rc.13/src/error.rs)
- [ort rc.13 builder options](https://github.com/pykeio/ort/blob/v2.0.0-rc.13/src/session/builder/impl_options.rs)

## Change and review

All six production boundaries explicitly map to `ort::Error<()>` before `?`
converts the error into `anyhow::Error`. The existing CPU-only provider,
disabled arena allocator, CPU-derived thread count, optimization level, model
loading, and inference paths are unchanged. No errors are ignored or retried;
no unsafe trait implementations or string-only error wrappers are introduced.

Two model-free native-runtime tests in `tests/ort_builder_compatibility.rs`
exercise the existing CPU configuration and verify that a recoverable builder
error remains downcastable with its code and message intact after conversion
and application context. They complement compilation of the actual production
adapters; they are not end-to-end model-quality tests.

The source diff was reviewed against the original head: the production change
is limited to two comments and six conversions. No new module, public interface,
or cross-module dependency is introduced. The new integration test is already
owned by `mag.quality.tests` in `cairn.blueprint`.

No additional code-review findings were identified. This is an introspective
review, not an independent or cross-model approval.

## Verification and landing gate

The source-only fix at `7939bb7dadf77c496d397aae6c5bc555d234d786` passed the
[Cairn architecture gate](https://github.com/George-RD/mag/actions/runs/34144779920).
That earlier result does not authorize merging a later head.

Before merge, verify CI and the Cairn architecture gate against the current PR
head, including both new native-runtime tests, the all-features suite, the
without-sqlite-vec checks, lint, benchmark gate, and packaging/smoke checks.
Record the exact final head and workflow results in the PR discussion so the
evidence does not require a self-referential documentation commit.

This editing environment has no Rust or Cairn executables and cannot resolve
GitHub for a local checkout. No local test, scan, or hook execution is claimed;
GitHub Actions provides the executable verification for this change.
