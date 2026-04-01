---
name: memory-store
description: Store a memory with MAG persistent memory system
---

# memory-store

Store content into MAG persistent memory for retrieval in future sessions.

## Usage

```bash
mag process "content to remember" --importance 0.7
```

> **Safety:** Do not store secrets, credentials, or large code blocks in MAG memory.

## Options

- `--importance <0.0-1.0>` — importance weight (default: 0.5)
- `--tags <tag1,tag2>` — comma-separated context tags
- `--project <name>` — associate with a project
- `--event-type <type>` — categorize the memory (e.g., decision, observation, task)
- `--session-id <id>` — associate with a session
