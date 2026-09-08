---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-08
reviewer: ChatGPT
---
# Memory-intelligence producer capture

## Scope

Continue `todo.build-local-memory-intelligence-eval-harness` after merged #441,
from main `6f7f0548e9e48d0fe1601e12102120e4667d37c1`. The open-PR search was empty;
reviewed the branch list, merged work, benchmark contract, and the relevant
local-first roadmap sequence before choosing this slice. The parent todo stays
in progress. No new roadmap item or production dependency was introduced.

This is a trusted external CLI capture bridge, not the selected MAG runtime
producer or a local-model baseline. It adds no Python model client, extraction
rules, database writes, or MCP-specific semantics. The two-case subprocess
fixture is only a harness regression, not a model-quality score.

This record is an introspective review, not independent approval or execution of
a slash-command `/code-review`. Live PR reviews and exact-head CI are separate.

## Design and adversarial review

- Allowlist producer inputs rather than deleting only `expected` from a case.
  Case IDs can also disclose negative controls or expected classifications.
  Neither IDs, annotations, dataset identity, nor run metadata are sent.
- Reuse strict JSON decoding, input validation, and atomic artifact helpers from
  the existing scorer. Do not add a competing profile validator or normalize
  malformed output into success.
- Start one process per case with an argument vector and a fresh working
  directory. Concurrent nonblocking pipe I/O avoids stderr/stdin deadlocks.
  Independently bound requests, stdout, stderr, and process duration.
- Retain failed attempts and measured wall time. Never silently retry, omit a
  hard case, accept partial output from a nonzero exit, or manufacture token/RAM
  observations. Report the process-startup/I/O/parsing/cleanup measurement scope.
- Kill same-group descendants on normal parent exit as well as failures. Tests
  prove child processes actually started before asserting their delayed side
  effects never occur. A mutation skipping successful-parent cleanup is caught.
- Resolve executable paths without dereferencing symlinks; dereferencing a Python
  virtual-environment launcher changes its behavior. The initial failing alias
  regression was fixed with an absolute path that preserves the symlink.
- Do not publish raw argv/environment or stderr. Preserve a command digest and
  describe inherited environment and the need for a separately protected
  invocation record. Metadata remains supplied, not authenticated.

## TDD and local evidence

The first two request-boundary assertions failed when the whole case crossed the
producer boundary. They pass after the allowlist projection. The executable
symlink assertion also failed before its correction. Local Python 3.13.5 then
passed all 22 new tests with real subprocesses, including CLI artifact behavior,
closed-pipe timeouts, quota errors, malformed/duplicate/nonfinite JSON, invalid
UTF-8 and lone surrogates, input aliases, fresh directories, and literal argv.

The test subprocesses use `python -S` to exclude this authoring environment's
site startup hooks. Production capture does not alter a producer's Python flags.
The descendant regression explicitly records two child PIDs, waits beyond the
child's delayed action, and checks that cleanup prevented that action.

Five local mutations each produced the intended assertion failure, not an import
or compilation error, and were restored before rerunning the green suite:

| Mutation | Detecting boundary |
| --- | --- |
| Include expected annotations in the request | Input allowlist assertion |
| Omit failed attempts | Two-case count and failure denominator |
| Disable stream quotas | Independent stdout/stderr overflow assertions |
| Replace observed latency with zero | Positive observed latency assertion |
| Skip cleanup after successful parent exit | Delayed child side-effect assertion |

Source blobs matched the locally tested copies at publication:

- `capture.py`: `29b0307bc28f6209a54845b2aefe04ff9addcee7`
- `evaluate.py`: `09af165408e59a0cf68f2dd4e798004654fa0c34`
- `test_memory_intelligence_capture.py`: `4a8acb883d5765d9e5a9cf6728221718cfdf42e3`

The existing 29 scorer tests belong to #441's evidence; this session does not
claim a local execution of those tests or of Rust/Cairn. The updated evaluation
workflow runs both suites on Linux (Python 3.10/3.13) and macOS (Python 3.13).
Full repository CI and pinned Cairn scan/hooks must pass at the exact PR head
before merge. Any legacy Cairn warnings must remain visible, not suppressed.

## Remaining limits

The process group and empty directory are not a hostile-code sandbox. Filesystem,
network, and environment remain accessible; producers must not detach sessions
or independently read the annotated corpus. A compatible MAG producer still
needs to use the selected CLI-first runtime and capture actual profile/resource
observations. This bridge does not measure model-only latency, tokens, model load
time, or peak RAM. Do not claim completion of P0 or a model-quality improvement
until a reproducible live-runtime baseline exists.
