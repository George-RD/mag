# Recorded memory-intelligence evaluation

This is the first slice of `todo.build-local-memory-intelligence-eval-harness`.
It validates a versioned development dataset and scores **recorded outputs**. It
does not load a model, call an endpoint, exercise production ingestion, or claim
a measured local-model baseline. Python 3.10 or newer and the standard library
are sufficient.

## Commands

From the repository root:

```bash
python3 benches/memory_intelligence/evaluate.py validate \
  benches/memory_intelligence/dataset.v1.json

python3 benches/memory_intelligence/evaluate.py score \
  benches/memory_intelligence/dataset.v1.json recorded-run.json \
  --output scorecard.json

python3 -m unittest discover -s tests -p test_memory_intelligence_eval.py -v
```

Without `--output`, scoring prints JSON to stdout. Exit 0 means a valid scorecard
was produced, **not** that every case passed. Invalid datasets, run metadata, or
file operations exit 2 with an error on stderr. Output files are replaced
atomically and cannot overwrite either input, including input aliases.

## Dataset and response contract

`dataset.v1.json` contains 14 synthetic development cases covering facts,
entities, temporal references, relationships, decisions, questions, status,
grouping, contradictions, and provenance. It includes three negative controls,
a two-source inference, and an Arabic-to-English fact case. It is a small
regression seed, not a held-out benchmark or evidence of broad model quality.

Each case has an instruction, immutable source memories identified by case-local
IDs, and expected annotations. A producer receives the instruction and sources,
**never the expected annotations**. Its output is exactly this shape:

```json
{"items": [{"value": "owner=Iris", "source_ids": ["m1"]}]}
```

The instruction defines canonical labels. Values match exactly: no case folding,
whitespace normalization, paraphrase judging, or fuzzy matching. Item order and
source-ID order do not matter. A grounded match additionally requires the exact
annotated source set. Even an extra valid citation fails that strict comparison;
it is an annotation-match metric, not an independent factual-entailment judge.
Empty `items` is valid for a negative control. Duplicate values or source IDs,
empty support, and malformed shapes invalidate the entire output rather than
salvaging a favorable subset. Invented source IDs are reported separately; they
can match a content label but cannot earn grounded credit.

Changing an instruction, source, annotation, or case order changes the dataset
identity. The SHA-256 is over canonical JSON: UTF-8, sorted object keys, compact
separators, unescaped Unicode, and no nonfinite numbers. JSON formatting and
object-key order alone do not change it. Record this digest from `validate`.

## Recorded-run envelope

The run is a JSON object with these required fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`. |
| `dataset_sha256` | Exact digest printed by dataset validation. |
| `code_revision` | Full 40-character lowercase MAG Git commit SHA used by the producer. |
| `model_profile` | Nonempty, verbatim adapter profile snapshot, or a bundle when multiple roles participated. |
| `embedding_space_identity` | Exact runtime identity, or JSON `null` when no embedding space participated. |
| `measurement_context` | Nonempty metadata describing hardware, runtime, measurement method, and whether the source is a model run or a test fixture. |
| `results` | Array of per-case results. Missing cases are scored as failures. |

Profile snapshots and the embedding identity are preserved, not reconstructed.
The evaluator hashes the profile snapshot but does **not** introduce another
model-profile validator or prove that supplied metadata is authentic. Real
producers must capture the selected runtime's actual role, revision, checksums,
quantization, transformations, and other profile metadata. A rule-only producer
must identify itself explicitly; it must not fabricate a pinned model profile.
The tests use metadata explicitly marked as fixtures, never model results.

Each result contains `case_id` and exactly one of `output` or a nonempty `error`
string. Optional per-result observations are `latency_ms`, `input_tokens`, and
`output_tokens`. Optional whole-run observations are `load_time_ms` and
`peak_ram_bytes`. Measurements may be absent or `null`; measured zero remains
zero. Durations must be finite and nonnegative. Token and byte counts must be
nonnegative integers, not booleans or fractions. Unknown fields, duplicate result
IDs, unexpected cases, and mismatched dataset digests reject the run. The JSON
reader also rejects duplicate object keys and nonfinite numeric values.

## Scorecard interpretation

Overall and per-task sections report schema validity, exact task success, and
micro precision/recall/F1 for content and grounded annotations. A task succeeds
only when its output is valid and its labels **and** complete source sets equal
the expected annotations. Missing, malformed, and runtime-error cases remain in
the denominator. They contribute all expected annotations as false negatives.
Precision or recall with an empty denominator is `null`; valid negative controls
still contribute to task success. Raw counts and per-case failures are retained.

Latency uses nearest-rank p50/p95 over supplied observations, including measured
failed attempts. Token totals are sums of **observed** counts, not estimates for
missing observations. Every aggregate includes its sample count. No samples
means `null`, never an invented zero. Load time and peak RAM are copied only when
supplied. This scorer's execution time is not model latency, and resource
expectations in a profile are not measured resource usage.

## Remaining harness work

Add capture through the CLI-first selected runtime, without feeding gold labels
to the model or creating a separate MCP/runtime implementation. Capture actual
profiles, latency, load time, peak RAM, and tokens; then record a reproducible
local baseline. Model promotion also needs a larger, held-out evaluation and
broader task-success evidence. The parent Cairn todo remains in progress.
