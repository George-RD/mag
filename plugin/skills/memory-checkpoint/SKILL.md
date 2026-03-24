---
name: memory-checkpoint
description: Save or resume task state across sessions
user-invocable: true
allowed-tools:
  - Bash
---

# Checkpoint

**Save**: `mag checkpoint "task title" "progress description" --project PROJECT --next-steps "what to do next"`

Include: what's done, what's blocked, key decisions made, files touched.

**Resume**: `mag resume-task --project PROJECT`

After resuming, briefly summarize the prior state and confirm next steps with the user.
