# Native-schema comparison — 9 September 2026

**Shape improved; extraction remains below the production gate.** The native
schema arm produced 13/14 scorer-valid outputs and 4/14 exact successes, versus
11/14 and 3/14 without constraints. Only one of eleven positive cases succeeded;
the other three successes are negative controls. Keep this mode diagnostic-only.

## Original evidence

`evidence.zip` is the original 17,946-byte Actions artifact, not a transcription
or a filtered rerun. Its SHA-256 is
`436b4c1c23d653164f35f94e25b3ed83e6d6574119a51af5fffad7861064b88e`.
It contains both original runs and scorecards, the pin, CPU/toolchain context,
source identity, and the measurement workflow and its source-only patch. No model
weights, executables, credentials or private memories are included.

Producing commit: `c1537d9b55b19bc9c3b673d18642e73be2d5da2d`; tree:
`61bec81cca7f9cf83ab91b035a3d160166fd8817`. It adds only the archived measurement
workflow to implementation commit `3b9bc46fae0f40b1fb374479f79c4d57489f9156`.
Actions run: https://github.com/George-RD/mag/actions/runs/34389969043;
artifact `10119587157`. Both arms were executed once on that same host using the
same observed MAG and server binary hashes, pinned GGUF bytes, prompt v1, dataset,
decoding parameters and independent scorer. Only native output constraints differ
in the producer request. Ephemeral endpoints, aliases and staged paths differ.

## Result without repair or discarded failures

| Observation | Unconstrained | Native schema |
| --- | --- | --- |
| Attempts retained | 14 | 14 |
| JSON captured | 12/14 | 14/14 |
| Scorer-valid outputs | 11/14 | 13/14 |
| Exact task successes | 3/14 | 4/14 |
| Positive cases solved | 0/11 | 1/11 |
| Negative controls passed | 3/3 | 3/3 |
| Grounded item matches | 0 | 1 |
| Grounded false positives | 0 | 3 |
| Attempt latency p50 / p95, ms | 3,029.007 / 4,529.728 | 3,008.390 / 4,971.590 |
| Observed server peak RSS, bytes | 1,372,499,968 | 1,416,990,720 |

The sole new exact success is the Arabic-to-English owner extraction. Eight
positive cases still return empty items. Grouping emits three source sentences
rather than the required canonical group label, including an unrelated project.
The contradiction output repeats a label with separate source sets; the unchanged
scorer rejects duplicate values. The native schema deliberately does not encode
gold labels or uniqueness/provenance semantics. Malformed outputs are not salvaged.
Both full archived scorecards replay exactly with the existing evaluator.

Unconstrained mode ran first, native schema second, with separate server
lifecycles. Fixed order, hosted-runner variation and warm-cache effects are not
controlled. Latency includes producer startup, I/O, parsing and cleanup; RSS is
sampled Linux server VmHWM, not total system/GPU memory. Token counts and isolated
model-load time remain null. These observations do not establish a causal speed
comparison, a model-family capability limit, or held-out generalization.

## Replay without a model

```bash
python3 -m unittest discover -s tests -p test_memory_intelligence_schema_evidence.py -v
```

The replay test checks archive identity, both full scorecards, matching pins and
binary hashes, unchanged non-ephemeral settings, and all fourteen cases per arm.
Its recorded counts protect evidence integrity; future models need not reproduce
these weak results. Original prompt-v1 and rejected prompt-v2 archives are unchanged.

## Decision

Keep production ingestion and default profiles unchanged. Establish the rule-only
comparator and additional held-out qualification boundary before claiming useful
improvement. Do not keep tuning this development seed, weaken the scorer, or treat
requested schema support as correct memory extraction. This remains part of the
existing `wire-lfm25-production-ingestion` todo.
