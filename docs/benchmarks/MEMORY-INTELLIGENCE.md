# MAG Memory Intelligence Results

Latest evaluation runs. Updated automatically by `./scripts/memory-intelligence-eval.sh`.

The family columns are headline metrics as percentages, and they measure
different things: micro F1, recall@10, accuracy, F1, cluster coverage and link
integrity. `Overall%` is their unweighted mean, so it is a mean over
incommensurable numbers and moves when the set of scoring families changes.
`Scored` is that mean's denominator over the families the run selected; an
empty family cell means the family measured nothing and left the denominator.

Six families have no production implementation and are excluded from
`Overall%`: fact extraction, contradiction detection, summarisation,
relationship typing, entity normalization, and `referenced_date` inference.

The corpus is 36 seeds, so single cases move a family score by tens of points:
`relationships` has three annotated edges, and one missed edge is 33 points.

`Dataset sha` is the first 12 characters of the SHA-256 of `dataset.json`.
Rows sharing a dataset version but not a sha were measured against different
ground truth. Metric definitions are in `docs/benchmarks/memory-intelligence.md`.

See `docs/benchmarks/memory_intelligence_log.csv` for full history.

## Memory Intelligence Evaluation

| Date | Embedder | Dataset | Dataset sha | Schema% | Overall% | Scored | Entities | Temporal | Rel | Lifecycle | Superseded | Grouping | Provenance | Questions | Peak RSS (KB) |
|------|----------|---------|-------------|---------|----------|--------|----------|----------|-----|-----------|------------|----------|------------|-----------|---------------|
| 2026-09-04 | bge-small | v1 | 3260e0a00beb | 100.0 | 61.1 | 8/8 | 7.4 | 75.0 | 66.7 | 100.0 | 50.0 | 0.0 | 100.0 | 90.0 | 91112 |
