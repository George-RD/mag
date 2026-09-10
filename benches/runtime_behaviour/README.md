# Runtime behaviour diagnostic

Recovered from `claude/mag-workflows-demos-dprysm` at `a2bbb71c05be39272b7efff77197bd8a080f2574`.
The original 36-seed dataset and manifest are byte-identical in
`data/runtime_behaviour_eval/v1/`. This diagnostic measures stored-memory
behaviour through `LocalMemoryRuntime`; it does not generate or score LLM output.
The existing `benches/memory_intelligence/` extraction evaluation is unchanged.

## Commands

```bash
cargo run --release --bin memory_runtime_eval -- --json
cargo run --no-default-features --bin memory_runtime_eval -- --embedder placeholder --json
cargo run --no-default-features --bin memory_runtime_eval -- --validate-only --json
cargo test --no-default-features --bin memory_runtime_eval --test runtime_behaviour_eval
```

The default uses MAG's shared checksum-verified BGE production adapter and its
persisted embedding-space identity, not the source branch's private profile.
Models may download on first use. Database writes are confined to a new private
temporary directory; normal MAG databases are not opened. The placeholder
embedder is only a deterministic test stand-in, not a quality reference.

`--dataset DIR` selects another versioned corpus. `--family NAME` selects one
family and is repeatable. `--json` emits one scorecard on stdout. Without it,
`--quiet` hides per-case prose. The binary does not overwrite a baseline or CSV.

## Observations

| Family | Observation | Headline |
| --- | --- | --- |
| entities | Entity tags after raw storage | Micro F1 |
| temporal | Relative-date advanced search | Mean recall at 10 |
| relationships | Annotated graph edges | Recall, not precision |
| lifecycle | Expiry sweep and retained rows | Accuracy |
| supersession | Version chains and superseding edges | F1 |
| grouping | Dry-run clusters and applied compact result | Cluster coverage |
| provenance | New auto-compact links and source visibility | Link integrity |
| questions | Answerable questions and negative controls | Recall at 10; abstention in detail |

Annotations, rather than current MAG output, define expected results. SHA-256,
version/count checks, references, family partitions, supported operations and
representable dates are validated before opening any database. Invalid input
fails the process. Metadata retains the shared path sanitization, records the dataset hash, and
distinguishes the default repository corpus from user-supplied directories.
Local directory names are not disclosed in the dataset-path field.
Missing measurements remain null. Each latency reports its sample count; p95
is null for fewer than five observations. There is no average of unlike family
scores and no letter grade. Linux RAM is process VmHWM; macOS is a sampled RSS
maximum. Neither is a separately isolated model memory measurement. Startup
includes warmup, and the resource estimates in the model profile are not observed
usage. Generated tokens are null because this path has no generative model.

The corpus, grouping, lifecycle and provenance families use separate databases.
Each supersession pair has its own database. `store_raw` avoids the compatibility
`processed: ` prefix used by `mag ingest`; this suite therefore does not measure
that command's prefix effect. Selected families are not a whole-suite score.

## Limits and preserved failures

This is a small synthetic development diagnostic, not a held-out benchmark.
It complements the newer 14-case model extraction evaluation but cannot serve
as its matched rule-only comparator: the tasks, inputs and scoring differ.
Neither a high score here nor schema-valid extraction qualifies production LLM
ingestion. Keep the current P0/P1 status and model-selection gates separate.

Original branch findings include tools tagged as people, meaning-reversed
preferences discarded by content deduplication, failed duplicate grouping, and
unrelated questions returning rows. They are hypotheses to reproduce on each
current build, not assertions that those defects have been fixed by this port.

Content deduplication can discard seeds before a family runs. Retained counts
and family details expose that; lifecycle does not count a discarded write as
a successful expiry. The grouping default requires three members, while genuine
pairs exist in the annotations. Auto-compaction lowers its count trigger from
500 to 1 for this corpus. Provenance measures only links auto-compaction actually
writes: no links means not measurable. It is not full source-lineage coverage;
manual compact can delete originals and is not covered by that link score.
Relationship annotations specify required edges, not every forbidden pair, so
unannotated edges do not establish false positives.

Historical dataset notes call one dedup example a trigram comparison. Production
actually compares stemmed, stopword-filtered token sets; the third argument is
minimum token length. That explanatory note is not scored and is retained to
preserve the dataset bytes. Likewise `historical_unimplemented_annotations`
reports the source dataset's omissions, not automatic current-capability detection.
No old 61.1% aggregate or CSV history is imported as a current result.
