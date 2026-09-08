---
node: mag.runtime.entrypoints
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# CLI-first memory-intelligence producer

## Scope and decision

PR #443 continues `todo.build-local-memory-intelligence-eval-harness` after
merged #442, from main `924581c447c73030c1eed19dcc773cd8e6ff2b55`. The open-PR,
branch, roadmap, and Cairn audit selected the existing producer integration gap;
no new roadmap todo was created. The parent remains in progress.

`mag intelligence-produce` is an opt-in stdio/configuration adapter over the
typed, non-persisting `LocalMemoryRuntime::produce_intelligence` workflow. Early
dispatch avoids opening SQLite or loading embeddings. The runtime validates an
answer-blind protocol request and invokes the existing plain generation backend
once. It does not parse or repair the completion, retry, persist, or introduce
separate MCP or Python memory semantics.

## Review of boundaries

Strict Serde structures reject unknown and duplicate JSON fields, including
answer annotations, case IDs, and metadata. Semantic validation rejects invalid
versions/tasks, empty instructions/sources, blank source IDs/text, duplicate
source IDs, and oversized requests before inference. Typed runtime callers pass
through the same semantic validation as the CLI.

The plain completion boundary is deliberate: the existing structured helper
can repair malformed JSON. Runtime tests make any structured call panic and
assert one plain call for successful, malformed, empty, and failing attempts.
The existing HTTP provider trims surrounding whitespace; the profile describes
that transformation. Fences and invalid schemas remain invalid for the scorer.

Explicit command arguments override local defaults without loading inherited
`MAG_LLM_*` provider or API-key settings. URL configuration disallows embedded
credentials, queries, and fragments. Backend failures are redacted before they
cross the runtime boundary. Mock HTTP tests exercise the actual executable,
request payload, failure handling, and absence of storage initialization.

`--describe` is configuration inspection, not model verification. It contacts
neither stdin nor a model. Artifact revision, checksums, quantization, licence,
and embedding identity remain null; the profile explicitly says
`configured_not_authenticated`. No token, load-time, or peak-RAM observations
are fabricated. These tests prove wiring, not model quality.

The one-MiB completion check happens after the existing HTTP adapter buffers its
response. This is a trusted-endpoint evaluation path, not bounded HTTP memory
isolation. Capture owns whole-process deadlines and stream quotas. A real-model
baseline still needs authenticated artifacts and honest measurement context.

Review found stale README claims that the adapter was unimplemented; both were
reconciled with this slice. Source ownership and model/runtime contracts are
updated. Temporary authoring infrastructure is absent from the final diff.
CI permissions remain unchanged and existing gates are not weakened; a permanent
explicit-feature release job adds coverage for the documented producer build.

## Observed verification

The test-only head `c08964b88f3a0d3ddb9a72492c5d2b7cef3ae424` reached the new CLI
tests in CI run `34276483942`, job `102230654988`. Three assertions failed with
`unrecognized subcommand 'intelligence-produce'`; this was the RED boundary,
not a compilation failure. Implementation wiring followed that observation.

Remote verification run `34277767812` applied the bounded integration patch and
passed formatting, both producer integration-test binaries with all features,
Clippy across all targets/features with warnings denied, the no-default-feature
producer-test build, and the Python capture/scorer regressions. Only after that
success did an isolated publication job commit the verified patch. The code
execution job had read-only repository permissions; the write job applied only
allowlisted paths and executed no repository code. Both temporary files were
then deleted using ordinary connector commits.

The cleaned implementation head `fe4bee662bf15becbee5647dc720a24eadb91146` passed
ordinary CI `34278288096`, Cairn `34278288146`, and the Python evaluation matrix
`34278287995`. The retrieval classifier did not require a benchmark rerun.
Cairn's existing orphan/provenance and oversized-module warnings remained.

## Independent review correction: explicit build profile

Codex review of that head found that the README incorrectly said `llm` was a
default feature. Cargo defaults and packaged release profiles do not enable it,
so the documented `cargo build --release` could not produce this command even
though all-features tests passed. Review comment `3962279771` identified this
real build/documentation mismatch.

A documentation regression was committed first at
`62ec3edf657f9c9571e1bdb00fe1b797baeb4768`. Evaluation run `34279336804`, job
`102240041170`, ran 53 tests: the existing 52 passed and the new build-profile
test failed because the README command lacked `llm`. The README was then fixed
to require `cargo build --release --features llm` and explicitly state that
default and packaged binaries exclude this opt-in evaluation command.

The permanent evaluation workflow now tests both producer integration binaries
in the default-plus-`llm` release profile, then runs `intelligence-produce
--describe` on that binary. This supplements, rather than replaces, full
all-features CI. Cargo defaults and release packaging remain unchanged. Final
same-head CI and a fresh independent review must verify the correction.

No local Rust or Cairn execution is claimed: the authoring container lacked
those executables and direct repository network access. Final ordinary full CI,
pinned Cairn, retrieval classification, and independent review are separate
merge gates on the cleaned PR head. Their live run/review identifiers and final
SHA are recorded in the PR discussion, rather than claiming them in advance or
creating another commit solely to refer to its own hash.

This document is an introspective review, not independent approval or execution
of a slash-command `/code-review`. Independent PR review remains required.
