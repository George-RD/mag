---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Pinned local memory-intelligence baseline — PR #444

## Reconciliation and scope

Started from main `3744763670000431bee3a46d6b336e9f6c9b25dd` after merged #443.
No open PR was displaced. This completes the existing P0 evaluation todo, not a
new roadmap gap. The roadmap remains the sequence/exit-gate source and Cairn
holds work status. P1's existing ingestion todo is the next diagnostic boundary;
a failed model baseline must not be mistaken for an incomplete measurement tool.

The runner owns a trusted local server and invokes the existing CLI/runtime
through answer-blind capture. It adds no Python extraction or memory semantics,
no new production dependency edge, and no model/default promotion. Existing
benchmark/test ownership covers the new files. Shared strict JSON, process
supervision, scoring, and atomic artifact helpers are reused rather than copied.

## TDD and defects found

Test-only `df1a742e6aef2e2d71babef55ff3dd295c94656c` preceded implementation.
Evaluation run `34307431428`, job `102326932486`, passed the 53 existing Python
tests and failed the new cases because the local baseline runner was absent.

A local slow-drip HTTP regression reproduced urllib's per-read timeout escaping
the overall startup budget. Health probes now use the existing bounded process
supervisor. A strengthened test confirms that the response actually started
before asserting deadline/cleanup; alias tests assert the specific failure.

Initial Linux 3.10/3.13 CI passed while macOS 3.13 timed out. Diagnosis run
`34308776381`, job `102330870762`, captured repeated server stacks blocked in
`socket.getfqdn` from `HTTPServer.server_bind`; the unchanged harness succeeded
with a DNS-free fixture listener. The fixture now uses `TCPServer` plus the same
HTTP handler and explicitly forbids hostname resolution. No production timeout
was increased and no macOS test was skipped to mask this defect.

Local Python 3.13 verification passed all 53 existing tests and all 14 new
orchestration tests. The historical artifact has an additional replay test.
Three behavioral mutations were killed by assertion failures (not import errors):
removing the prelaunch checksum fence, fabricating zero when RSS is unavailable,
and ignoring changed binaries after capture. The mutation edits were discarded.

An accidentally expanded fixture assertion was restored to the exact locally
tested form; its complete file blob is
`d51092b854f36d4482e0eda22e96ca0eb847c6d7`. The evidence import verified the
archive SHA-256 and reran all 67 Python tests before committing it. Temporary
source-export, diagnosis, and evidence-import workflows are removed from the
final tree. The durable local-model workflow is opt-in only.

## Independent review

Codex reviewed implementation commit `6a3c3d46b83c73a0a2ce12ba136c41b4b0811824`
and reported no major issues in PR comment `5595480702`. Subsequent edits retain
that runner and generation workflow unchanged; they correct/extend fixtures,
record the measured evidence, and reconcile documentation. Final review/CI
observations belong to the exact final PR head, not just that earlier review.

## Actual model evidence and interpretation

Actions run `34308189840` at `6a3c3d46b83c73a0a2ce12ba136c41b4b0811824`
built real MAG and pinned llama.cpp, verified the pinned LFM2.5 1.2B Q4_K_M
artifact, captured all 14 cases once, and produced the separate scorecard.
Artifact `10087628388` is preserved byte-for-byte in the dated baseline folder.
Its ZIP digest matches Actions; local rescoring exactly matches the archived
JSON. No favorable rerun, retry, output repair, prompt change, or dataset tuning
was used to improve the reported result.

The result is 3/14 task success, exclusively negative controls, zero positive
matches, and 11/14 schema validity. Per-attempt p50/p95 is 2.976/4.464 seconds;
observed server VmHWM is 1,372,626,944 bytes (925 samples). Readiness is 873.671 ms,
not a cold-cache or isolated model-load measure. Tokens and isolated model load
time are null. Full hardware and build context is in the archive.

Verified staged-file provenance is separate from the unchanged configured
producer profile. Binary hashes are observations, not source-to-binary execution
attestation. The experiment trusts its host and binaries; loopback aliases are
not security credentials. RAM excludes MAG, descendants, GPU, and whole-host
memory. The one-run development seed is neither held out nor a model-quality
promotion gate. Empty output could reflect prompt/template/protocol behavior or
model limitations; this run does not isolate the cause.

## Final merge boundary

The final head must pass ordinary repository CI, the Linux 3.10/3.13 and macOS
3.13 evaluation matrix, opt-in release-producer tests, and Cairn `scan`/`hook all`.
Rust and Cairn are executed remotely, not claimed locally. PR #444 records their
exact-head run IDs and merge result; no pending check is treated as green.
