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
   - **Project context**: `mag hook search "PROJECT TOPIC" --project PROJECT`
   - **Error lookup**: `mag hook search "error message keywords" --project PROJECT`
   - **Recent work**: `mag hook search "PROJECT" --limit 5`

2. Present results naturally. Do not list raw JSON. Synthesize the relevant context into your response.

3. If nothing relevant is found, say nothing about the search.
