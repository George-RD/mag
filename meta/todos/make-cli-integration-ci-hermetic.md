---
node: mag.quality.tests
status: in_progress
created: 2026-08-28
---
# Make CLI Integration CI Hermetic

The `mag.quality.tests` contract requires hermetic tests, but the all-feature CI
suite currently builds CLI integration tests with the production ONNX embedder.
Those tests use fresh temporary HOME directories, so commands that embed text can
download BGE artifacts from Hugging Face during unrelated pull requests.

PR #430 reproduced the defect twice on 2026-08-28: all preceding Rust tests
passed, then `cli_lessons_runtime_migration` failed because the BGE model request
returned HTTP 429. Retrying the failed exact-head job produced the same external
failure. GitHub issue #431 records the audited roadmap gap.

Keep production behavior unchanged. Preserve all-feature coverage for library,
binary, and non-CLI integration tests, including real-embedding path tests. Run
CLI integration contracts without the `real-embeddings` feature so their binary
uses the deterministic `PlaceholderEmbedder` already selected by the normal
feature boundary. Do not add retries, skip coverage, or add a hidden production
runtime switch solely for CI.
