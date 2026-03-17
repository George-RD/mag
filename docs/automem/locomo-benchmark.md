# AutoMem — LoCoMo Benchmark

Primary file: `tests/benchmarks/test_locomo.py` (~2600 lines)
Supporting: `tests/test_locomo_cat5_judge.py`, `tests/test_locomo_speaker_extraction.py`, `tests/benchmarks/test_multihop_quick.py`

## Dataset

- **10 conversations**, **1,986 questions** total
- Baseline SOTA (CORE): 88.24% overall accuracy
- AutoMem reported: **89.27%** on `locomo-mini` (cat 1-4), **87.56%** full with cat-5

## 5 Question Categories

| Cat | Type | Scoring Method |
|-----|------|----------------|
| 1 | Single-hop fact retrieval | Word-overlap (0.5 threshold) |
| 2 | Temporal understanding | Word-overlap + fuzzy date match |
| 3 | Multi-hop reasoning | Word-overlap + embedding similarity (≥ 0.50) |
| 4 | Open domain knowledge | Word-overlap (0.5 threshold) |
| 5 | Complex reasoning | LLM judge (GPT-4o), not word-overlap |

## Scoring Algorithm

### Word-Overlap (categories 1-4)

This is **binary, not continuous**:
1. Normalize both expected answer and recalled memory content (lowercase, strip punctuation)
2. Compute word-level overlap between normalized strings
3. If overlap ≥ **0.5** → mark correct (score = 1)
4. If overlap < 0.5 → mark incorrect (score = 0)
5. Final accuracy = correct_count / total_questions

Temporal questions (cat 2) additionally use fuzzy date matching — if dates align, confidence is forced to **0.95**.

### Embedding Similarity (cat 3 multi-hop fallback)

When word-overlap alone is insufficient for multi-hop:
- Compute cosine similarity between question embedding and recalled memory embeddings
- Threshold: **≥ 0.50** for semantic relevance
- Maximum memories checked: **10** (for speed)

### LLM Judge (category 5 only)

- Model: **GPT-4o** (not gpt-4o-mini)
- Prompt uses chain-of-thought
- Judge evaluates whether drafted answer "materially agrees" with evidence dialogs
- Allowed responses: Yes/No/I don't know/Question premise is wrong
- Abstention is valid — if the question premise is wrong, judge can say so
- Timeout: **90 seconds** per call
- Cached: `(question, evidence, evidence_dialog_ids)` tuples are cached to avoid duplicate calls

If no LLM model is configured, cat-5 returns `is_correct=None` and is skipped from accuracy calculation.

## Recall Configuration for Benchmark

- **10 memories recalled per question** (`recall_limit=10`, configurable)
- **Tag-based filtering** — each question filtered by conversation ID, session, and speaker tags
- **Evidence memory direct fetch** — for questions with `dialog_ids`, evidence memories are fetched directly (not via search) and appended to recalled set for verification
- **Local conversation cache** — all conversation dialogs loaded into memory for fast evidence lookup without re-querying

## Multi-hop Recall Parameters

`multi_hop_recall_with_graph()` call in benchmark:
- `initial_limit=20` — seeds from initial vector search
- `max_connected=60` — maximum graph-traversal results to consider

## Memory Loading

Per-conversation setup:
- ~250+ memories loaded per conversation
- Batch API endpoint (`POST /memory/batch`) used first (up to 500 memories per call)
- Falls back to individual `POST /memory` if batch unavailable

## Quick Multi-hop Benchmark (`test_multihop_quick.py`)

Filters to cat-3 questions only (~96 questions across 10 conversations):
- ~3-5 minute runtime vs ~20 minutes for full benchmark
- Identical scoring logic to full benchmark
- Reports accuracy by conversation + sample failures

## Speaker Extraction

`test_locomo_speaker_extraction.py` tests that speaker names are extracted correctly from questions:
- Handles ASCII possessive (`'s`) and Unicode curly possessive (`'s`)
- Example: "Would Caroline's sister pursue writing?" → extracts "Caroline"
- Used to set `speaker` tag on recalled memories for filtering

## Key Thresholds Summary

| Parameter | Value |
|-----------|-------|
| Word-overlap correct threshold | 0.5 |
| Embedding similarity threshold (multi-hop) | 0.50 |
| Temporal date match confidence | 0.95 |
| Memories recalled per question | 10 |
| Max embeddings checked for similarity | 10 |
| LLM judge timeout | 90s |
| initial_limit for multi-hop | 20 |
| max_connected for multi-hop | 60 |
