# LoCoMo Gap Analysis: MAG vs AutoMem

## Baseline

| System | Word-Overlap Score | Dataset |
|--------|-------------------|---------|
| AutoMem | 90.5% | LoCoMo-10 |
| MAG | 70.6% | LoCoMo-10 |

Gap: ~20pp

## Root Cause Analysis

Forensic comparison of AutoMem's `test_locomo.py` vs MAG's `benches/locomo/` reveals the gap is driven by **3 concrete differences in benchmark execution**, not algorithmic quality:

### Factor 1: Retrieval Limit (top_k)

| | AutoMem | MAG |
|---|---------|-----|
| Standard | 50 | 20 |
| Temporal | 75 | 20 |
| Multi-hop | 100 | 20 |

Word-overlap scoring = `|expected_tokens ∩ retrieved_tokens| / |expected_tokens|`. More memories → more tokens → higher recall. AutoMem retrieves **50-150 memories** per question vs MAG's **20**.

**Estimated impact:** ~10pp

### Factor 2: Speaker-Tag Secondary Recall

AutoMem (`test_locomo.py:516-540`) runs a **dual-query strategy**:
1. Semantic search (standard query, limit=50)
2. Tag-based search: `tags=[speaker:{name}, conversation:{sample_id}]`, `tag_mode=all`, `limit=50`
3. Merge results, deduplicate by memory ID

MAG runs only semantic search.

**Estimated impact:** ~8pp

### Factor 3: Memory Tagging at Seed Time

| Tag Type | AutoMem | MAG |
|----------|---------|-----|
| `speaker:{name}` | Yes (`test_locomo.py:227-232`) | No |
| `conversation:{id}` | Yes | No |
| `session:{num}` | Yes | No |
| `entity:people:{slug}` | Yes | Yes (new) |

Speaker tags are a prerequisite for Factor 2's tag-based recall.

**Estimated impact:** Prerequisite for Factor 2

## Implementation Plan

### Unit 1: Add speaker/conversation/session tags + increase top_k

- Add `speaker:{name}`, `conversation:{sample_id}`, `session:{key}` tags to every memory at seed time
- Change default `top_k` from 20 to 50
- Expected gain: +5-7pp → ~76%

### Unit 2: Speaker-tag secondary recall

- Extract speaker name from question (first capitalized proper noun)
- Run tag-based search via `get_by_tags([speaker:{name}, conversation:{id}], 50)`
- Merge with semantic results, deduplicate by memory ID
- Expected gain: +6-8pp → ~83%

### Unit 3: Dynamic limits per question type

- Multi-hop (evidence.len() > 1): limit=100
- Temporal (when/what time/what date keywords): limit=75
- Standard: limit=50
- Expected gain: +3-5pp → ~87%

## Remaining Gap (~3pp to 90.5%)

Likely from:
- Evidence dialog prioritization
- Temporal query hints
- Embedding model differences (AutoMem: text-embedding-3-large 3072-dim vs MAG: bge-small-en-v1.5 384-dim)
