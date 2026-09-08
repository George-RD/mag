# Memory-intelligence capture and scoring

This implements bounded slices of
`todo.build-local-memory-intelligence-eval-harness`: versioned dataset validation,
recorded-output scoring, and capture from a trusted external CLI producer.
The harness does not own model loading or memory semantics. A production MAG
producer adapter and a measured local-model baseline remain outstanding.
Python 3.10 or newer and the standard library are sufficient; capture requires
POSIX process groups (Linux or macOS).

## Commands

From the repository root:

```bash
python3 benches/memory_intelligence/evaluate.py validate \
  benches/memory_intelligence/dataset.v1.json

python3 benches/memory_intelligence/evaluate.py score \
  benches/memory_intelligence/dataset.v1.json recorded-run.json \
  --output scorecard.json

python3 -m unittest discover -s tests -p 'test_memory_intelligence_*.py' -v
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

## Trusted CLI producer capture

Supply an executable that implements the protocol below. No production MAG
adapter is bundled yet; a fixture executable tests the bridge, not model quality.
For an existing compatible producer, the command shape is:

```bash
python3 benches/memory_intelligence/capture.py \
  benches/memory_intelligence/dataset.v1.json \
  --metadata actual-producer-metadata.json --output recorded-run.json \
  --timeout-seconds 60 --max-bytes 1048576 \
  --producer /absolute/path/to/compatible-producer --its-own-options
```

`--producer` must be last. Its remaining arguments form an argument vector, not
a shell command. The executable is resolved before changing directory; symlinks
are preserved so virtual-environment launchers retain their behavior. Use absolute
paths for producer scripts, model files, configuration, or other path arguments.
Each case starts a new process in its own empty temporary working directory.

The producer reads one UTF-8 JSON document from stdin until EOF:

```json
{
  "schema_version": 1,
  "task": "facts",
  "instruction": "Extract the owner as owner=NAME.",
  "sources": [{"id": "m1", "text": "Iris owns the project."}]
}
```

It writes exactly one JSON response with the `items` shape above to stdout and
exits zero. Case IDs can contain answer hints, so they are excluded alongside
annotations, dataset identity, and run metadata. The parent joins responses to
case IDs; the producer does not supply or choose them. Requests preserve source
text and source IDs. Producers must not read the annotated dataset separately.

The metadata input contains `code_revision`, `model_profile`,
`embedding_space_identity`, and `measurement_context` from the recorded-run
contract. Optional externally observed `load_time_ms` and `peak_ram_bytes` may
also be supplied. It must not contain `schema_version`, `dataset_sha256`,
`results`, unknown fields, or the reserved `measurement_context.capture` key.
Metadata is validated before launching, copied unchanged except for the appended
capture context, and never sent to the producer. The bridge records the resolved
executable and an argument-vector digest, not raw arguments or environment values.
Keep a separately protected invocation record for reproducibility; a digest alone
cannot reconstruct arguments or authenticate the executable or supplied profile.

The timeout covers process launch and pipe I/O. Input, stdout, and stderr each
have the configured byte limit (default 1 MiB; maximum 16 MiB). Stderr is drained
and discarded, never copied into artifacts or echoed. Timeouts, nonzero exits,
invalid UTF-8/JSON, and quota failures become failed attempts; later cases still
run. Valid JSON with an invalid response shape is preserved for the scorer to
reject, not repaired. No automatic retries or selective result omission occur.
Same-group descendants are killed even after their parent exits successfully.

This is a **trusted-producer boundary, not a security sandbox**. The environment
is inherited and the filesystem/network are not isolated. Producers must not
detach into another session or read gold answers through other channels. Do not
run untrusted commands or point a producer at a personal production database.

Per-case latency is observed wall time including process startup, I/O, JSON
parsing, and cleanup. It is not isolated inference latency or evidence of warm
model performance. The runner does not measure tokens, model load time, or peak
RAM; missing observations remain null, and any supplied resource measurements
must disclose their external method. Tests are explicitly marked as fixtures.

Capture writes the run atomically, leaves stdout empty, and exits zero when an
artifact is produced, even if every attempt failed. Configuration or file errors
exit 2. Dataset/metadata output aliases are rejected before execution. A failed
validation or write does not replace an existing output with a partial run.

## Remaining harness work

Implement a compatible producer through the selected CLI-first MAG runtime,
without adding separate Python or MCP memory semantics. Capture its actual
profiles and resource observations, then record a reproducible local-model
baseline. Model promotion also needs a larger, held-out evaluation and broader
task-success evidence. The parent Cairn todo remains in progress.
