---
node: mag.quality.benchmarks
review_type: agent_introspective
date: 2026-09-09
reviewer: ChatGPT
---
# Preserve cleanup semantics across an exiting-process race

## Observed blocker

PR #447 head `df8dab09614e289097ebf023de2de4adf97633f1` passed Linux
Python 3.10/3.13 and the actual release CLI in evaluation run `34373798380`.
macOS job `102541354630` failed in pre-existing capture/baseline tests, before
running the new diagnostic tests. All new diagnostic tests then passed.

The startup-deadline traceback shows `capture._stop_group` raising EPERM from
`os.killpg` while handling the expected producer timeout. The cleanup exception
masked that timeout, and the subsequent stream-closing loop was skipped. The
stderr-quota test also failed with subprocess/pipe resource warnings. Neither
failure is being treated as a green check or dismissed by an unexamined retry.

## Source-backed diagnosis and conservative fix

Apple's published XNU implementation filters SZOMB entries from process-group
iteration and can return EPERM when no eligible process was found. A group
whose only member is our unreaped exited child can therefore look like a
permission denial until that child is reaped:

https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_sig.c#L1676-L1730

This source supports an exit-race explanation; it is not proof of the hosted
runner's exact kernel build. EPERM can also be a genuine permissions failure.
The fix consequently does not ignore permission errors or assume every group
has disappeared. On the first denial it polls/reaps the owned direct child.
If that child is still running, the error propagates without a blocking wait.
If it has exited, it retries the group signal once, so remaining descendants
still receive SIGKILL. Only a missing group is ignored; repeated permission
failures remain visible. There is no unbounded retry loop.

The invocation supervisor now closes all pipes in a nested finally even if
group cleanup fails. This does not claim that a denied live process was killed.
It prevents a cleanup exception from skipping descriptor cleanup.

## Test-first evidence

Six focused cleanup tests were added before modifying capture.py. The initial
run had two failures and two errors: the unreaped-leader and remaining-descendant
paths raised EPERM, the expected second denial was never attempted, and the
real-process injected-denial case left pipes open. The fixture independently
cleans up its own real child after the injected error.

After the fix, all six passed. They also prove that a live-child denial does not
wait, a repeated denial is not swallowed, existing success/missing-group behavior
still waits, and remaining descendants are signalled after reaping the leader.
At that stage the local suites totaled 23 tests: 22 passed, one explicit
real-binary skip. This was the focused subset, not a full-repository test claim.

That head, `9483ba5daecab3ef011064e04e0285b41fd53f30`, then passed all remote
workflows: evaluation `34375011362`, repository CI `34375011353` and Cairn
`34375011373`. Its actual-CLI artifact `10113537910` was independently checked:
all 14 request bodies match the preserved original byte-for-byte. The synthetic
checkout `b6ae45ba3bb3e07ca51ec7df89d07fb3d4b675b9` has the exact head tree.

## Review finding: do not re-enter denied cleanup

Codex comment `3970622870` identified that a denial in the parent-exit branch
left the old group_stopped flag false. The outer finally then re-entered cleanup,
resulting in four signals instead of one initial attempt plus one retry.

A deterministic invocation-level regression reproduced four calls before the
fix. The supervisor now marks cleanup_attempted before calling the helper. That
name describes an attempt, not successful termination. The regression verifies
exactly two signals, propagation of the original second denial, no blocking wait,
and closure of every pipe. The outer finally does not restart failed cleanup.

After this review fix, local verification ran 24 tests: 23 passed, one explicit
real-binary skip. Source/test blobs match the executed files:
`fdbfb11845147a36d7d95e365bb3ebf96edbfcb2` and
`208f207db282dd1a342d1648382a107525e01f65`. An additional 15 real Linux process
attempts retained their expected stdout/stderr quota or timeout failures.

## Review finding: preserve the revision lookup failure

CodeRabbit comment `3970639267` noted that combining export with command
substitution masks git rev-parse's failure status. The workflow now assigns the
revision separately before exporting it, so its existing set -e stops immediately.

A test executes the checked-in step body with git returning seven and a harmless
python3 invocation marker. Before the fix, it returned zero and reached the
marker. After the fix, it returns seven without invoking the diagnostic. No real
Git operation, model or external service is invoked by this regression. The
Python matrix runs it on Linux and macOS without an added dependency.

Current local verification: 25 tests, 24 passed, one explicit real-binary skip.
The workflow and new test match local Git blobs
`2e291b9296dec996ef19c717cc950da5cf5b88a9` and
`4dea4c8dc1962acc6ca9b77f164c69861a2fae97`. Final exact-head CI and review are
still required; earlier green runs are not substituted for the final revision.

## Scope

This repairs the existing shared process supervisor used by the new diagnostic,
not a second supervisor or memory runtime. It is part of finishing the current
diagnostic todo, not a new roadmap item. No producer/model generation is retried;
only cleanup may retry a group signal after observing direct-child exit.

The Rust runtime, prompts, scorer, datasets, negative controls and archived model
and request evidence remain unchanged. No model-quality or server-template result
is inferred from these tests. See the companion request-diagnostic review for
the overarching P1 scope and remaining qualification boundary.
