---
name: memory-recall
description: Search and recall memories from MAG persistent memory system
---

# memory-recall

Search stored memories using MAG's multi-phase retrieval pipeline.

## Usage

```bash
mag advanced-search "query" --limit 5
```

## Options

- `--limit <n>` — maximum results to return (default: 10)
- `--project <name>` — filter by project
- `--importance-min <0.0-1.0>` — minimum importance threshold
- `--context-tags <tag1,tag2>` — filter by tags
- `--explain` — show scoring breakdown for each result
