# Memory Intelligence Evaluation
<!-- Last verified: 2026-09-04 | Valid for: v0.1.10-dev+ -->

This harness measures what MAG does with a memory after it is stored: which
entities it tags, which relative-date queries reach the right rows, which
relationships it infers, whether TTLs expire, whether a later memory supersedes
an earlier one, how it clusters near-duplicates, whether derived rows keep a link
to their sources, and whether question retrieval answers or abstains.

It is not a retrieval benchmark. LoCoMo and LongMemEval already cover retrieval
quality; see `docs/benchmarks/methodology.md`.

## Command

```bash
cargo run --release --bin memory_intelligence_eval -- --embedder bge-small
```

Wrapper that logs the run:

```bash
./scripts/memory-intelligence-eval.sh --embedder bge-small
```

The binary writes no files. `--json` prints the summary to stdout and suppresses
human output. The wrapper appends one row to
`docs/benchmarks/memory_intelligence_log.csv` and rewrites
`docs/benchmarks/MEMORY-INTELLIGENCE.md`. It does not touch
`benchmark_log.csv`, whose 16 columns are consumed positionally by
`scripts/bench.sh`.

### Flags

| Flag | Default | Effect |
|---|---|---|
| `--dataset <dir>` | `data/memory_intelligence_eval/v1` | Directory holding `dataset.json` and `manifest.json`. |
| `--embedder <name>` | `bge-small` | `placeholder`, `bge-small`, or `profile-bge-small`. |
| `--family <name>` | all | Score one family. Repeatable. |
| `--json` | off | Print the JSON summary and suppress human output. |
| `--validate-only` | off | Run schema validation and exit. |
| `--quiet` | off | Print the header and summary table without per-family detail. |

`--embedder placeholder` builds and runs under `--no-default-features`. The two
ONNX embedders need `real-embeddings`; without that feature the binary still
builds and exits with a message naming the flag.

## Dataset

`data/memory_intelligence_eval/v1/` holds `dataset.json` and a `manifest.json`
recording its SHA-256 and array counts. Annotations are ground truth authored
from the seed text, not from MAG output, so a family can score zero on behaviour
MAG has never implemented. An annotation is never rewritten in place to suit
what MAG's parser accepts: a changed annotation goes in a new version directory
with its own manifest, and `scripts/memory-intelligence-eval.sh` refuses to log
a run whose dataset SHA-256 differs from the last row carrying the same
`dataset_version`.

Every seed carries a `group`. The harness opens one SQLite database per group in
a temporary directory and deletes the directory when the run ends. Groups are
separate because auto-supersession fires on every store of an eligible
`event_type`: mixing the supersession pairs into the corpus database would change
the other families' results. The `provenance` family gets its own copy of the
`grouping` seeds, because it applies `auto_compact`, which mutates rows the
`grouping` score reads.

Seeding uses `LocalMemoryRuntime::store_raw`. `store` routes through the
compatibility `PlaceholderPipeline`, which prefixes `"processed: "` into the
stored content, the FTS index and the embedding, and shifts word positions enough
to change entity extraction.

`day_offset` is applied through `MemoryInput::referenced_date`, which sets the
`event_at` column. That is the column the relative-date filters in
`advanced_search` compare against. `created_at` is always the run time and cannot
be set through the public API, so families that depend on write time rather than
event time are not measurable here.

## Schema validity

Nine checks run before any scoring. A failure is fatal: the binary prints the
offending item and exits 1 rather than reporting scores over an invalid dataset.

| Check | What it asserts |
|---|---|
| `manifest_sha256_matches` | `manifest.sha256` equals the SHA-256 of `dataset.json`. |
| `manifest_version_matches` | Schema and dataset versions agree across both files. |
| `seed_keys_unique_and_non_empty` | Every `seed.key` is unique and non-empty. |
| `seed_groups_non_empty` | Every seed names a database group. |
| `cross_references_resolve` | Every annotation key resolves to a seed key. |
| `event_types_valid` | Every `event_type` passes `mag::memory_core::is_valid_event_type`. |
| `importance_in_range` | Every `importance` is within `0.0..=1.0`. |
| `entity_annotations_prefixed` | Every entity annotation is `people:`, `tools:` or `projects:` prefixed. |
| `manifest_counts_match` | `manifest.counts` matches the actual array lengths. |

`schema_validity_percentage` is passed checks over total checks.

## Families

| Family | Observed through | Headline metric |
|---|---|---|
| `entities` | `list()`, reading `entity:*` tags | micro F1 |
| `temporal` | `advanced_search()` with relative-date phrasing | mean recall@10 |
| `relationships` | `get_relationships()` | recall over annotated edges |
| `lifecycle` | `sweep_expired()` then `list()` | accuracy |
| `supersession` | `version_chain()` and the `SUPERSEDES` edge | F1 |
| `grouping` | `compact()` dry run, then applied | cluster coverage |
| `provenance` | `auto_compact()` then `version_chain()` | source-link integrity |
| `questions` | `advanced_search()` | mean recall@10 |

Each family also records the latency of its primary runtime call. Four families
make that call once per run, so their p95 column prints `n=1` rather than a
percentile over a single measurement; the JSON carries `latency_samples` and a
null `p95_latency_ms` for those. `grouping` times the dry run only, because the
applied merge that follows it is a different operation.

`overall_percentage` is the unweighted mean of the families that produced a
score. That denominator moves: a family that measures nothing leaves the mean
rather than scoring zero, which raises it, and `--family` narrows it further. The
score line and the JSON therefore carry `scored_families`, `selected_families`,
`total_families` and `not_measurable_families` beside it, and a run that selected
a subset prints no letter grade.

### Metric definitions

- Precision is 1.0 when nothing was predicted; recall is 1.0 when nothing was
  expected; F1 is 0.0 when precision and recall are both 0.0. `entities` reports
  micro-averaged P/R/F1 over all (seed, tag) pairs and a macro F1 averaged over
  seeds.
- `recall@k` is the fraction of annotated keys appearing in the first `k`
  results. `temporal` also reports a false-inclusion rate: annotated
  `expect_absent_keys` that appeared, over all such annotations.
- `relationships` reports recall only. Precision is null with a stated reason:
  the dataset annotates the edges a correct system must create, not every pair
  that must stay unlinked, so an unannotated edge is not evidence of an error.
  The observed edge-type histogram is reported alongside.
- `grouping` reports cluster coverage as the headline and cluster purity in the
  detail. Coverage is the fraction of labelled clusters recovered whole inside a
  single observed cluster, over the labelled clusters with two or more members.
  A one-member labelled cluster is excluded from that denominator: it is
  satisfied by the memory existing un-clustered, so counting it would award a
  point for doing nothing. Singletons are reported beside the headline as left
  alone, which a wrongly merged singleton fails. Purity alone reaches 100% when
  MAG produces only singletons, which is exactly the failure this family exists
  to catch. `compact` reports only cluster sizes and a 100-character preview on
  a dry run, so membership is recovered from the applied merge, which joins
  member content with `\n---\n`.
- `provenance` is the fraction of the `superseded_by_id` links `auto_compact`
  wrote that lead somewhere usable. Counting retired rows that carry a link
  would be a tautology: `auto_compact` increments its retired count inside the
  same statement that writes the column, so the two can only agree. Each link is
  scored on four conditions instead: the target row exists, the target was not
  itself retired, the retired row is hidden from a default `list()`, and the
  retired row is still readable with `include_superseded`. Links already present
  before the call are excluded from both sides, because `store_raw` writes the
  same column for the event types in `is_supersession_type` and those are not
  `auto_compact`'s doing. When `auto_compact` writes no link the family reports
  `not_measurable` with that reason rather than a zero. `compact` is not covered
  by this score: it hard-deletes cluster members and records no link.
- `questions` reports recall@5, recall@10 and MRR over answerable questions, and
  abstention precision and recall over the questions annotated
  `expect_abstain: true`.

### Parameters that are not defaults

`compact` runs with the CLI and MCP defaults, `similarity_threshold = 0.6` and
`min_cluster_size = 3`. `auto_compact` runs with `count_threshold = 1` instead
of the production default of 500, because the eval corpus is smaller than that
threshold and the family would otherwise never trigger.

## Model identity

The JSON summary records `embedder_name`, `embedding_dimension`, and
`embedding_space_identity` read from the `runtime_metadata` row MAG persisted in
the seeded database.

`model_profile` is populated only by `--embedder profile-bge-small`, which wraps
`OnnxEmbedder` in a harness-owned `EmbeddingModel` carrying a pinned
`RetrieverModelProfile`. For the other two embedders it is `null` with
`model_profile_reason` explaining that MAG's compatibility adapter records no
revision or checksum metadata. The harness does not fabricate that metadata for
the legacy path.

`tokens` is always `null` with the reason `no generative model in the evaluated
path`. Character counts are not reported as tokens.

`peak_rss_kb` is `VmHWM` from `/proc/self/status`, the kernel's own high-water
mark, so it covers the ONNX model load and the inside of each family loop rather
than only the moments the harness sampled. On macOS `ps` reports current RSS and
the figure is a sampled maximum instead.

## Limits

- The corpus is 36 seeds. Family scores move in coarse steps; `relationships`
  has three annotated edges, so one missed edge is 33 percentage points.
- `overall_percentage` averages six different kinds of metric — micro F1,
  recall@10, accuracy, F1, cluster coverage, link integrity — with equal weight.
  It is a summary of this harness's families, not a measure of anything MAG
  reports about itself.
- The harness measures MAG through its public runtime API. It cannot observe
  intermediate values, so a family scores on the outcome, not on why the outcome
  happened.
- Content deduplication runs before clustering and before supersession. A seed
  whose Jaccard similarity to an earlier same-type memory clears that type's
  `dedup_threshold` is discarded on write, and the families downstream of it see
  a smaller corpus. The run header prints seeded and retained counts, and
  `grouping`, `supersession` and `lifecycle` name the discarded cases.
  `lifecycle` does not score a seed that never became a row: its absence after
  the sweep is the write being discarded, not `sweep_expired` removing anything.
- Six families have no production implementation and are listed under
  "Families with no production implementation" with the target output shape
  rather than scored: fact extraction, contradiction detection, summarisation,
  relationship typing, entity normalization, and `referenced_date` inference.
