---
node: mag.quality.benchmarks
status: done
created: 2026-07-28
completed: 2026-09-04
---
# Build Local Memory Intelligence Eval Harness

A versioned local dataset and runner now exist and report a first measured
baseline. Both blockers were closed before this work: the production composition
root is selected, and the role-aware model-profile boundary landed through PRs
#414, #417 and #429, so every run records the exact embedding-space identity and
model profile under test.

## What shipped

- `data/memory_intelligence_eval/v1/` — 36 seeds and their annotations, with a
  `manifest.json` recording the dataset SHA-256 and array counts. Annotations are
  ground truth authored from the seed text, never from MAG's output.
- `benches/memory_intelligence/` — the runner, registered as the
  `memory_intelligence_eval` binary with no `required-features`, so it builds and
  runs under `--no-default-features` with `--embedder placeholder`.
- `scripts/memory-intelligence-eval.sh` — logs a run to its own
  `docs/benchmarks/memory_intelligence_log.csv` and regenerates
  `docs/benchmarks/MEMORY-INTELLIGENCE.md`. It does not touch
  `benchmark_log.csv`, whose columns `scripts/bench.sh` consumes positionally.
- `docs/benchmarks/memory-intelligence.md` — method, metric definitions, and the
  limits of each score.

Evaluation runs entirely through `LocalMemoryRuntime`, seeding with `store_raw`.
No library visibility was widened and no benchmark-governed path was touched:
`python3 scripts/retrieval_benchmark_gate.py` reports `false` for this change.

## Reported per run

Schema validity as a pass/fail list, per-family precision/recall/F1 or task
success, p50 and p95 latency (with the sample count shown when a family makes
fewer than five timed calls), peak RSS from `VmHWM`, model load time, the
embedding-space identity MAG persisted, and the validated `RetrieverModelProfile`
when one is pinned. `tokens` is reported as null with a reason: there is no
generative model in the evaluated path, and a character count is not a token
count.

`--embedder profile-bge-small` constructs a pinned `RetrieverModelProfile` over
the ONNX embedder with the real artifact checksums and HF revision. This is the
first production-shaped use of that contract outside its own unit tests.

## First baseline, 2026-09-04, bge-small-en-v1.5-int8

| Family | Metric | Score |
|---|---|---|
| entities | micro F1 | 7.4% |
| temporal | mean recall@10 | 75.0% |
| relationships | recall | 66.7% |
| lifecycle | accuracy | 100.0% |
| supersession | F1 | 50.0% |
| grouping | cluster coverage | 0.0% |
| provenance | link integrity | 100.0% |
| questions | mean recall@10 | 90.0% |

Unweighted mean of the eight scored families: 61.1%. Six further families have no
production implementation and are recorded with their target output shape rather
than scored as zero: fact extraction, contradiction detection, summarisation,
relationship typing, entity normalisation, and referenced-date inference.

## Defects the first run surfaced

These are the calibration inputs `todo.calibrate-retrieval-and-reranking` needs.

1. **Entity extraction is broadly wrong.** One true positive across the corpus.
   Tool names are classified as people (`people:redis`, `people:nginx`,
   `people:postgres`, `people:dockerfile`), and a two-token name fragments into
   three tags. `extract_people` treats any non-sentence-initial capitalised word
   as a name.
2. **The CLI write path corrupts entity extraction further.**
   `LocalMemoryRuntime::store` prefixes `"processed: "`, which shifts the real
   first word off position 0 and creates a false person tag. Seeding through
   `store_raw` does not reproduce it, which localises the defect to the CLI path
   rather than to `extract_entities`.
3. **A supersession pair is destroyed by content dedup before supersession
   runs.** `"I prefer tabs over spaces"` and `"I prefer spaces over tabs"` have
   identical token sets after stopwording and stemming, so the `user_preference`
   dedup threshold of 0.75 discards the update. Meaning reversal is invisible to
   a bag-of-tokens gate.
4. **Clustering recovers none of the labelled duplicate groups.** `compact`
   found no cluster at `similarity_threshold` 0.6, and the production
   `min_cluster_size` of 3 cannot recover a genuine pair.
5. **Abstention returns a false answer.** A query about on-call compensation,
   with nothing relevant in the store, returns ten results.
6. **A thin query abstains before its date filter is consulted.** Stripping the
   date phrase leaves a stub below `abstention_min_text`, so the `event_at`
   bounds never matter. The dataset keeps one such case on purpose.

## Note on the dataset

Two temporal annotations were originally calendar-dependent. `this week` holds
for its expected seeds on only four of seven weekdays, and `last month` on 85%
of run dates. A score that changes with the day of the week is not a
measurement, so those cases were rewritten onto the deterministic
`last N days` form that `try_prefix_n_unit` parses.

Rewriting a case to remove non-determinism is legitimate; rewriting one to make
it easier for MAG is not. The dataset now states that rule, and the rewrite kept
a thin-content case that still scores zero, so the failure the original phrasing
exposed is still on the record.
