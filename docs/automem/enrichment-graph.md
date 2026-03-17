# AutoMem — Enrichment & Graph System

## Entity Extraction

AutoMem uses spaCy NER + regex patterns to extract entities from memory content into 5 categories:

| Category | Examples | Detection |
|----------|---------|-----------|
| `tools` | SuperWhisper, VSCode, Docker | spaCy PRODUCT + regex for known tools |
| `people` | Alice, Bob | spaCy PERSON |
| `projects` | Launchpad, ProjectX | spaCy ORG + custom regex |
| `concepts` | async programming, TDD | noun phrases, filtered for abstract terms |
| `organizations` | Anthropic, OpenAI | spaCy ORG |

Extracted entities are stored in memory metadata (e.g., `entity:tools:SuperWhisper`, `entity:projects:Launchpad`) and used to create graph nodes.

## Graph Node Types

```
Memory      — one per stored memory (UUID as identifier)
Entity      — canonical entity node (tool, person, project, concept, org)
Pattern     — detected behavioral/preference pattern (3+ similar memories required)
ConsolidationControl — singleton scheduler state node
ConsolidationRun     — run history records
```

## Relationship Types (13 total)

| Relationship | Created When |
|-------------|-------------|
| `MENTIONS` | Memory content mentions an entity |
| `INVOLVES` | Memory directly involves a person (stronger than MENTIONS) |
| `USES` | Memory involves using a tool/technology |
| `PART_OF` | Memory belongs to a project |
| `RELATED_TO` | General semantic relation between memories |
| `SIMILAR_TO` | High cosine similarity between two memories |
| `PRECEDED_BY` | Temporal: memory created within 7 days of another |
| `FOLLOWED_BY` | Reverse of PRECEDED_BY |
| `EXEMPLIFIES` | Memory is an example of a Pattern node |
| `CONTRASTS_WITH` | Opposing decisions/preferences (from consolidation) |
| `DISCOVERED` | Creative association found during consolidation |
| `SUMMARIZES` | Consolidated summary memory → source memories |
| `CLUSTERS_WITH` | Memories grouped in same 30-day cluster |

Edge properties include `strength` (0–1), `score`, `confidence`, `similarity`, `created_at`, `origin`.

## Enrichment Pipeline

Enrichment runs in two phases after a memory is stored and embedded:

### JIT Phase (~25–125ms)
Runs immediately after embedding completes (async):
- Extract entities from memory content
- Create `Entity` nodes in FalkorDB if they don't exist
- Create `MENTIONS`/`INVOLVES`/`USES`/`PART_OF` edges
- Tag memory with `entity:<category>:<name>` tags
- Check for `PRECEDED_BY` temporal links (within 7-day window)

### Batch Phase (~110–500ms)
Deferred, processed by enrichment worker:
- `SIMILAR_TO` relationship detection (requires Qdrant lookup of nearby embeddings)
- Pattern detection and `EXEMPLIFIES` edge creation
- Summary rewrite (e.g., "Met with Alice" condensed from verbose content)
- Update enrichment metadata: `temporal_links`, `patterns_detected`, etc.

### Enrichment Status API

`GET /enrichment/status` returns:
- `queue_size` — pending items
- `pending` — count waiting to be processed
- `inflight` — count currently being processed

`POST /enrichment/reprocess` — accepts memory IDs (comma-separated or list), re-queues for enrichment.

## Pattern Detection

A `Pattern` node is created when 3 or more memories are sufficiently similar:
- Pattern confidence range: **0.35 – 0.95**
- Pattern stores top-5 key terms extracted from member memories
- Each member memory gets an `EXEMPLIFIES` edge to the pattern node
- Pattern nodes are used during creative consolidation to generate meta-memories

## Temporal Relationships

`PRECEDED_BY` edges are created between memories when:
- Both memories belong to the same user/context
- The time delta between creation timestamps is ≤ 7 days
- Direction: newer → `PRECEDED_BY` → older

These are used during recall to surface temporally adjacent memories (e.g., "what happened around the same time").

## Graph Query Pattern

Typical FalkorDB Cypher for relation expansion:
```cypher
MATCH (m:Memory {id: $memory_id})-[r]-(related:Memory)
WHERE r.strength >= 0.3
RETURN related, r.strength, type(r)
ORDER BY r.strength DESC
LIMIT 20
```

Multi-hop (depth 1–3):
```cypher
MATCH (m:Memory {id: $id})-[*1..3]-(related:Memory)
RETURN DISTINCT related
LIMIT 60
```
