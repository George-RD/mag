---
name: memory-recall
description: Search and recall relevant memories for the current context
user-invocable: true
allowed-tools:
  - Bash
---

# Memory Recall

Search MAG memory for context relevant to your current task.

1. Determine search strategy:
   - **Project context**: `mag advanced-search "PROJECT TOPIC" --project PROJECT --limit 10`
   - **Error lookup**: `mag advanced-search "error message keywords" --project PROJECT --limit 5`
   - **Recent work**: `mag recent --limit 5 --project PROJECT`

2. Present results naturally. Do not list raw JSON. Synthesize the relevant context into your response.

3. If nothing relevant is found, say nothing about the search.
