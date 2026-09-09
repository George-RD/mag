---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Pinned-server template diagnostic

## Scope and authority

Reconciled main `20ac7de32c217d9fad500ab8f03ada30d80016e7`, open PRs,
branches, the merged #447 review threads and exact-head checks, and the existing
`wire-lfm25-production-ingestion` todo. No open PR remained. The former root
ROADMAP.md is absent; the node-linked todo and accepted planning decision remain
the authority. Cairn status, next, bundle and rationale selected this existing
P1 slice rather than a new roadmap item.

The new Python client belongs to `mag.quality.benchmarks`; its tests belong to
`mag.quality.tests`. It reuses capture's process supervisor and the existing
strict JSON and atomic-write helpers. It does not own a server lifecycle or add
an alternative memory/runtime path. Caller-owned server verification is recorded
separately from this client's explicitly unauthenticated context.

The production runtime, prompt v1, dataset, scorer, model defaults, MCP and prior
baseline/request archives are unchanged. This diagnostic does not qualify P1 or
prove better extraction. No production enabling decision is made.

## Test-first evidence

The first eight test methods were written before the implementation existed;
the run failed on the missing module and the real CLI's absent script. This was
a missing-implementation RED boundary, not a pre-existing production regression.
After implementation, eight tests passed; a ninth covers a failed `/props` probe
and a source body over quota. All nine passed after that addition.

Four temporary behavioral mutations were independently rejected by assertions:
reserializing the original wire JSON; dropping failed source cases; reporting
replayed templates as actual generation prompts; and ignoring the exchange
deadline. All mutations were restored, and the nine tests passed again.

The archive-integrity test first failed because the new observation ZIP did not
exist. This is a missing-evidence RED, not claimed production-behavior evidence.
It compares every archived request against the original #447 bytes and every
response/prompt digest, without running a server or treating fixtures as model
answers.

The local focused set passed 51 tests. A separate capture/request set ran 40
with 39 passes and one explicit real-MAG-binary skip. Combined local invocations,
including the existing baseline suite, exceeded this container's tool execution
limit and are not recorded as complete. One existing drip-readiness test failed
during the combined attempt and passed in isolation. No timing threshold or
baseline implementation was changed to conceal it. The full CI, evaluation
matrix and Cairn workflows all passed at initial head
`278db719e9ca5687a0ba9cc5a739b0219898fca4`; final-head verification and review
are separate PR evidence, not inferred from that earlier green run.

## Server source audit and interpretation boundary

At the pinned llama.cpp commit
`304665fe7ac957df95e3ff8c8c4ffdf92dd6ffa3`, inspect
`tools/server/server-context.cpp` (blob
`fe068d3e9104f7d2fa1405f827acc22e072b768b`):

- lines 4930-4942: `post_chat_completions` parses the body through
  `oaicompat_chat_params_parse(body, meta->chat_params, files)` before completion;
- lines 5060-5070: `post_apply_template` uses the same parser and server chat
  parameters, then returns its `prompt` without scheduling completion.

The replay deliberately changes only the endpoint, not the captured request
body. Shared parsing at a pinned source revision links the diagnostic to the
chat-completion path. It does not establish identical server binaries across
runs, prove an earlier actual generation prompt, or explain poor extraction by
itself. Every `generation_prompt` remains null. A subsequent schema-constrained
comparison must disclose its changed request semantics and preserve negative
controls, original outputs and the scorer; held-out and rule-only/resource
comparisons still govern production promotion.

## Actual observation and cleanup

Run `34379968205` succeeded at the explicit producing head above. The original
artifact `10115793367` is retained byte-for-byte under
`benches/memory_intelligence/diagnostics/2026-09-09-template-v1/`. Its README
records the original ZIP and member digests, server executable identity, model
verification, source audit and interpretation limits. All 14 observed templates
preserve the source system/user messages, Arabic text and assistant prefix. The
independent archive test passes alongside the nine transport tests.

The rendered string omits the displayed BOS token, but pinned `common/chat.cpp`
lines 949-954 can remove BOS/EOS text before completion's special-token-aware
tokenization (`server-context.cpp` line 4299). This is not evidence of a missing
BOS token during generation. Neither token IDs nor model answers were measured.

The local review removed a duplicated JSON canonicalization helper and reused
`evaluate._canonical`; all nine transport tests still passed. The observation
archive retains its earlier producing head rather than being relabelled as a
later verification run. No server lifecycle refactor, runtime prompt change or
new dependency was needed. A temporary publisher copied only the known original
artifact, checked both its ZIP SHA-256 and Git blob identity, and used a normal
fast-forward push to the expected feature branch. All temporary workflows are
excluded from the final diff.

## Review follow-up: configured HTTP deadline

Codex identified that the worker's fixed five-second socket timeout could abort
an otherwise valid response before the caller's configured exchange deadline.
A new regression uses real loopback HTTP with headers delayed 5.25 seconds and
an eight-second budget. It failed on the fixed timeout (producer exit one), then
passed when the configured timeout was passed to the worker. The outer shared
supervisor still bounds the entire exchange, including slow-drip progress.
All eleven diagnostic tests pass after the fix. This does not rewrite or rerun
the earlier successful observation archive.

## Cairn and environment

Cairn 0.9.0 ran locally from an exported release binary. `scan` and `hook all`
returned zero with the repository's pre-existing 30 warnings and two information
findings; this is not a zero-warning claim. No new node/edge is needed.

`cairn brief todo.wire-lfm25-production-ingestion` returned no todo; omitting the
prefix returned no bead. The documented `cairn feedback` command recorded that
CLI incompatibility. The status/next/bundle/rationale commands and linked todo
file supplied the needed context without bypassing gates. Generated maps and
Cairn caches are not part of this implementation diff.

Rust is not installed in this container. The repository's exact-head CI owns
Rust format, clippy, tests and CLI smoke verification. Nothing is merged merely
because local Python tests passed. The temporary acquisition workflow is removed
from the final code change; its exact producing version is retained as evidence.
