---
node: mag.quality.benchmarks
status: in_progress
created: 2026-07-28
---
# Build Local Memory Intelligence Eval Harness

P0's runtime, capture, scoring, and reproducible local-scorecard boundary is
implemented through PRs #441–#444. Completion means a usable measurement
foundation, not a passing production model. Sequencing remains in
`docs/specs/local-first-roadmap.md`; do not create a duplicate harness todo.

## Delivered boundary

The versioned 14-case synthetic development dataset covers facts, entities,
temporal references, relationships, decisions, questions, status, grouping,
contradictions, and provenance, with negative controls, multi-source support,
and Arabic-to-English extraction. It is not held out.

The strict recorded-output scorer distinguishes content and complete provenance
sets, keeps missing/invalid cases in the denominator, preserves exact supplied
profile/dataset/code identity, and discloses performance sample counts. Missing
measurements remain null. Exact labels are not a factual-entailment judgment.

The answer-blind capture bridge sends only protocol version, task, instruction,
and immutable source memories. Neither expected annotations nor case IDs are
sent. Each case has one attempt, including errors, without retries or repair.
Fresh working directories and bounded POSIX process groups are not a sandbox.

`mag intelligence-produce` calls the selected typed `LocalMemoryRuntime` through
one plain generation request. The opt-in `llm` command is non-persisting and does
not open storage or embeddings. No Python extraction, alternative production
runtime, or MCP memory semantics were added.

The local baseline runner owns a trusted llama.cpp process, checks a private
read-only GGUF copy before launching any executable, and records binary hashes,
server readiness, and observed Linux server RAM. It preserves the producer's
configured profile verbatim alongside verified-file evidence. This does not
claim cryptographic execution attestation or authenticate a hostile process.
Source revisions are declared; observed binary hashes are separate evidence.
Isolated load duration, tokens, and unavailable RAM remain null.

## Measured local baseline

PR #444 recorded the actual LFM2.5-1.2B-Instruct Q4_K_M CPU run at commit
`6a3c3d46b83c73a0a2ce12ba136c41b4b0811824`, Actions run `34308189840`.
The original archive and methodology are preserved at
`benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu/`.
Independent local rescoring exactly matches its archived scorecard.

All 14 attempts were recorded: 11 valid outputs, 3 task successes (all negative
controls), and zero positive matches. Eight positive cases returned empty items;
grouping violated the unique-value contract and two cases failed JSON capture.
Observed wall latency p50/p95 was 2.976/4.464 seconds; server VmHWM was
1,372,626,944 bytes. This is one hosted CPU run, not model-promotion evidence.

The P0 reproducible-scorecard gate is met. The separate P1 usefulness gate is
not. Continue `todo.wire-lfm25-production-ingestion` by diagnosing the selected
request/chat-template/output behavior before enabling ingestion. Do not conflate
a completed measurement harness with production-quality extraction.

## Verification and ownership

- PR #441 established scorer/dataset regressions, 29 hermetic tests, and four
  rejected scoring mutations. Its review record retains the JSON overflow,
  recursion handling, annotation vocabulary, and Arabic ownership corrections.
- PR #442 added 23 capture tests and five rejected mutations, including bounded
  pipes/quotas, failed-attempt retention, executable symlinks, and descendants.
- PR #443 added runtime and real-process mock-HTTP tests for the selected CLI.
- PR #444's test-only `df1a742e6aef2e2d71babef55ff3dd295c94656c` failed at the
  missing runner before implementation. A slow-drip response regression exposed
  per-read timeouts; the existing process supervisor now bounds whole probes.
  The macOS fixture's reverse-DNS stall was traced on-runner and removed without
  relaxing production deadlines. Additional tests cover atomic artifacts,
  cleanup, unavailable RSS, wrong aliases, and post-run binary mutation.
- Local Python 3.13 passes 53 existing tests and 14 orchestration tests. The
  historical evidence replay is an additional consistency test, not model
  success. Three local mutations were rejected by assertions: skipping the
  prelaunch checksum, fabricating zero RSS, and ignoring changed binaries.

Ownership remains `mag.quality.benchmarks` (`benches/`) and `mag.quality.tests`
(`tests/`). Durable methodology, review, and exact-head evidence are recorded in
`meta/reviews/memory-intelligence-local-baseline.md` and PR #444. Local authoring
cannot execute Rust/Cairn; their remote gates are mandatory, not waived.
