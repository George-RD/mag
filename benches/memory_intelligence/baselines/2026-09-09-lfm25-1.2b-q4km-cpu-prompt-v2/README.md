# Rejected prompt-v2 diagnostic — 9 September 2026

**Reject this prompt. Exact task successes fell from 3/14 to 2/14, schema-valid
outputs from 11/14 to 4/14, and positive item matches remained zero.** Prompt v1
is restored in the active runtime. Neither result qualifies production ingestion.
This experiment belongs to the existing `todo.wire-lfm25-production-ingestion`.

## Original evidence

`evidence.zip` is the original 8,085-byte Actions artifact, copied without
transcription, filtering or repair. It contains `run.json`, `scorecard.json`,
`pin.json`, `cpu.json` and `toolchain.txt`. Its SHA-256 is
`d1edb58a36ef676ce92ef29fd95b9eaf4cb53478f34a415a3723337dfe8f090c`.
No executable, model weights, credentials or private memories are included.

- Producing commit: `9eb7c6048e09d61c111b3e2d7760f1a27503de65`.
- Producing tree: `cc7b6c059740429d61019bece4a8f5c3ad3f26b1`.
- Actions run: https://github.com/George-RD/mag/actions/runs/34359655347;
  artifact `10107880199`.
- Dataset SHA-256: `7a001a0aed48bc0e457c155b6ba0780343bb528c661b7473c79dd653a76ce26e`.
- Model: LFM2.5-1.2B-Instruct Q4_K_M, with verified file bytes and exact revision
  recorded in the archived pin/profile.
- Comparison: [original prompt-v1 baseline](../2026-09-09-lfm25-1.2b-q4km-cpu/).
  Its archive and documentation are unchanged.

## Result, without salvaging malformed items

| Observation | Prompt v1 | Rejected prompt v2 |
| --- | --- | --- |
| Attempts retained | 14 | 14 |
| JSON captured | 12/14 | 14/14 |
| Schema-valid outputs | 11/14 | 4/14 |
| Exact task successes | 3/14 | 2/14 |
| Positive content / grounded item matches | 0 / 0 | 0 / 0 |

The two successes are the proposal-is-not-a-fact and already-answered-question
negative controls. The explicit-supersession negative control regressed to
`{"items":[null]}`. Nine outputs contain items of the wrong type (null, string or
array), and one has objects with missing required fields. Valid JSON is not a
valid result schema or a useful memory extraction. Even the two schema-valid
positive outputs fail the instruction's canonical-label contract.

Do not repair these values, infer missing citations, discard failures or relax
the scorer. Every attempt is present. Independent rescoring exactly reproduces
the archived scorecard; the replay test is an integrity check, not a quality gate
requiring future models to repeat this failure.

Observed per-attempt wall latency p50/p95 was **3,128.989 / 6,291.753 ms**, with
14 samples. Server readiness was **866.876 ms** and observed server peak RAM was
**1,372,991,488 bytes**. Input/output tokens and isolated model-load time are
unmeasured and remain null. These are whole-process attempt latency, readiness
and Linux server VmHWM, not model-only inference, cold-cache startup or total
system RAM. See [runner methodology](../../LOCAL_BASELINE.md).

## What this comparison establishes

The intended change was prompt framing: explicit task/instruction/source sections
and a stronger schema/extraction instruction, without changing the answer-blind
request boundary, single plain completion call, scorer or dataset expectations.
The complete model pin, verified GGUF bytes, claimed llama.cpp revision, dataset
digest, producer decoding settings and non-ephemeral server settings match the
original baseline. Both runs use two CPU threads and no GPU.

However, observed **server and MAG binary hashes differ** between the runs despite
the shared server-source pin. Build environment and hosted-runner variation were
not eliminated. This is not a hermetic build comparison or proof that framing
alone caused the regression. It is sufficient reason not to promote this
candidate. It does not establish a model-family capability limit. Both runs use
the development seed, not held-out qualification, and no favorable rerun replaces
the recorded outcome.

## Replay without a model

From the repository root:

```bash
out=$(mktemp -d)
python3 -m zipfile -e \
  benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu-prompt-v2/evidence.zip \
  "$out"
python3 benches/memory_intelligence/evaluate.py score \
  benches/memory_intelligence/dataset.v1.json "$out/run.json" \
  --output "$out/recomputed.json"
python3 -m unittest discover -s tests -p test_memory_intelligence_baseline_evidence.py
```

## Reconstruct the rejected source

`experiment.patch` preserves the entire measured experiment, including its
original temporary runner, as inert historical evidence. Its SHA-256 is
`b7ff0fe568065c924c93d343146bf4aa0ec61bb138fef1a5e31923886c4cb0c9`.
The patch is not an active workflow and is not applied to current MAG.

Use a separate worktree based on the recorded main commit; the zero-context patch
requires `--unidiff-zero`. These commands reconstruct source only, not a new run:

```bash
patch="$PWD/benches/memory_intelligence/baselines/2026-09-09-lfm25-1.2b-q4km-cpu-prompt-v2/experiment.patch"
out=$(mktemp -d)
git worktree add --detach "$out/rejected-source" \
  badcd90ac996322ceddd11642ef1b6e9524f5e55
git -C "$out/rejected-source" apply --unidiff-zero --index "$patch"
git -C "$out/rejected-source" write-tree
# Expected: cc7b6c059740429d61019bece4a8f5c3ad3f26b1
```

This exact-tree reconstruction was verified. A new measurement would require
building the pinned server/model and following the restored historical runner;
it would remain a separate experiment with its own producing revision and
observed binary hashes, not a replacement for this archive.

## Next diagnostic boundary

Keep P1 open. Inspect the actual answer-blind HTTP request and server-rendered
chat template before another prompt change. A non-repairing schema-constrained
comparison may then distinguish output-shape compliance from extraction quality;
it must retain all failed attempts and disclose the changed request semantics.
Any apparent improvement still needs additional held-out cases and a rule-only
comparison before production wiring or a model-default change.
