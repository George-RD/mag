---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Pinned local memory-intelligence baseline

PR #444 continues `todo.build-local-memory-intelligence-eval-harness` from main
`3744763670000431bee3a46d6b336e9f6c9b25dd`; no open PR was displaced and no new
roadmap todo is introduced. The parent remains in progress pending real evidence.

The new quality runner owns a trusted local llama.cpp process, verifies a private
read-only GGUF copy against the pin before any executable starts, and invokes the
existing `mag intelligence-produce` through the answer-blind capture bridge.
No Python extraction or parallel memory runtime is introduced. Existing benches
and tests ownership covers the new files.

The exact configured producer profile remains unauthenticated and is preserved
verbatim alongside verified local-file evidence. Binary hashes are observed;
source-to-binary relationships remain declared, not cryptographic attestation.
Readiness is measured separately from isolated load time. Linux server VmHWM is
sampled; tokens, isolated load duration, and unavailable RAM stay null.

## TDD evidence so far

Test-only df1a742e6aef2e2d71babef55ff3dd295c94656c preceded implementation.
Evaluation run 34307431428, job 102326932486, reached the new tests and failed
because `local_baseline` did not exist; the other 53 existing Python tests passed.
A local slow-drip HTTP regression then reproduced urllib's per-read timeout
escaping the whole startup budget. Health probes now use the existing process
supervisor to enforce a whole-response deadline. Python 3.13 locally passes all
64 evaluation tests, including 11 new orchestration tests. Fixture success is
not a model result. Rust/Cairn verification is remote, not local.

## Outstanding verification

The actual pinned LFM2.5 1.2B CPU run, independent review, final exact-head CI and
Cairn gates are not yet claimed. The temporary authoring trigger must be removed
before merge; the durable measurement workflow remains explicitly opt-in.
