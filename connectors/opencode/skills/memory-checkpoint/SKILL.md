---
name: memory-checkpoint
description: Save and resume work checkpoints with MAG persistent memory system
---

# memory-checkpoint

Save progress checkpoints and resume incomplete work across sessions.

## Save a checkpoint

```bash
mag checkpoint "task title" "progress description" --project <name>
```

## Resume from checkpoint

```bash
mag resume-task --task-title "task title" --project <name>
```

> **Safety:** Do not store secrets, credentials, or large code blocks in MAG memory.

## Options

- `--plan <text>` — overall plan for the task
- `--next-steps <text>` — what to do next
- `--session-id <id>` — associate with a session
- `--project <name>` — associate with a project
