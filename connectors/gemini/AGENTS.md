<!-- MAG_MEMORY_START -->
# MAG — Persistent Memory

MAG provides persistent memory across sessions. Available commands:

> **Safety:** Do not store secrets, credentials, or large code blocks in MAG memory.

## Store a memory

```bash
mag process "content to remember" --importance 0.7
```

## Search memories

```bash
mag advanced-search "query" --limit 5
```

## Session context

```bash
mag welcome --project <project_name>
```

## Checkpoint current work

```bash
mag checkpoint "task title" "progress description"
mag resume-task --task-title "task title"
```

## Health check

```bash
mag maintain --action health
```
<!-- MAG_MEMORY_END -->
