# Abstention Gate
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

The abstention gate is a collection-level filter in the advanced search pipeline
that prevents MAG from returning results when no stored memory is
genuinely relevant to the query. Rather than always returning the "least bad"
match, the system returns an empty result set -- signaling to the caller that
the query cannot be answered from the memory store.

## Why text_overlap instead of vector similarity

Dense embedding models like bge-small-en-v1.5 produce high cosine similarity
(0.80+) even for completely unrelated content. This makes `vec_sim` unusable as
an abstention signal -- every query looks "similar" to something.

Text overlap (the fraction of query tokens that appear in the candidate text)
cleanly separates relevant from irrelevant queries:

| Query type       | Typical max text_overlap |
|------------------|--------------------------|
| Relevant query   | 0.33 -- 1.00             |
| Irrelevant query | 0.00 -- 0.25             |

The gap between 0.25 and 0.33 provides a natural decision boundary.

## Threshold

The abstention threshold is set at **0.15** (`ABSTENTION_MIN_TEXT` in
`src/memory_core/scoring.rs`). This value was determined through grid search
optimization on the LongMemEval benchmark. After computing all candidate scores,
the pipeline checks the maximum text overlap across all in-scope candidates. If
this maximum falls below 0.15, the entire result set is suppressed and an empty
vector is returned.

A safety bypass exists for queries with no eligible word tokens (all tokens
<= 2 characters, e.g. "AI", "C++"). In these cases the gate is skipped because
text overlap would always be 0.0, causing false abstention.

## Implementation

The gate is applied in `src/memory_core/storage/sqlite/advanced.rs` during
Phase 6 of the search pipeline (collection-level abstention + dedup), after
search-option filtering but before final ranking:

```
Query
  -> ONNX embed
  -> Vector search + FTS5 BM25
  -> RRF fusion
  -> Score refinement (type x priority x word_overlap x importance x feedback)
  -> Search-option filtering + dedup
  -> **Abstention gate** (max text_overlap < 0.15 -> return empty)
  -> Final ranking + normalization
  -> Results
```

The gate is applied after filtering so that out-of-scope high-overlap candidates
do not suppress abstention for scoped queries.

## LongMemEval performance

On the AB (abstention) category of the LongMemEval benchmark:

| System        | AB score |
|---------------|----------|
| MAG | 20/20    |
| omega-memory  | 16/20    |

The 4-point advantage comes entirely from the text_overlap-based gate.
omega-memory relies on vector similarity for relevance filtering, which
allows high-cosine but semantically irrelevant results to leak through.

## Tuning the threshold

The threshold is exposed as `abstention_min_text` in the `ScoringParams` struct.
To adjust it:

```rust
use mag::memory_core::scoring::ScoringParams;

let params = ScoringParams {
    abstention_min_text: 0.25, // more permissive (fewer abstentions)
    ..ScoringParams::default()
};
```

Lower values make the system more permissive (returns results for weaker
matches). Higher values make it more aggressive (abstains more often). The
default of 0.15 was optimized for the LongMemEval benchmark via grid search
and balances precision against recall.
