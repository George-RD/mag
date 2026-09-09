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

After the fix, all six pass. They also prove that a live-child denial does not
wait, a repeated denial is not swallowed, existing success/missing-group behavior
still waits, and remaining descendants are signalled after reaping the leader.
The available local suites total 23 tests: 22 passed, one explicit real-binary
skip. This is the focused local subset, not a full-repository test claim.

Python compilation passes. The pushed capture and test blobs match the executed
local files: `7a687d344295a4ff5b55fdc2282b9097ccb92e5d` and
`6df70e385f7fb7db44200d168983c36569ecb963` respectively. The final PR head still
requires the full Linux/macOS matrix, Rust CLI and repository/Cairn checks.

## Scope

This repairs the existing shared process supervisor used by the new diagnostic,
not a second supervisor or memory runtime. It is part of finishing the current
diagnostic todo, not a new roadmap item. No producer/model generation is retried;
only cleanup may retry a group signal after observing direct-child exit.

The Rust runtime, prompts, scorer, datasets, negative controls and archived model
and request evidence remain unchanged. No model-quality or server-template result
is inferred from these tests. See the companion request-diagnostic review for
the overarching P1 scope and remaining qualification boundary.
