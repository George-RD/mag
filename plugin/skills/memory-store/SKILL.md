---
name: memory-store
description: Store a decision, pattern, or insight to persistent memory
user-invocable: true
allowed-tools:
  - Bash
---

# Memory Store

Store important information for future sessions.

1. Identify the right event type:
   - `decision` (importance 0.9): architecture, library, pattern choices
   - `lesson_learned` (importance 0.8): bug root causes, non-obvious solutions
   - `error_pattern` (importance 0.8): recurring errors with fix
   - `user_preference` (importance 0.7): style, workflow, tool preferences
   - `user_fact` (importance 0.7): factual info about the user or team

2. Store via CLI:
   ```
   mag process "Brief title. Context. Rationale." \
     --event-type TYPE \
     --project PROJECT \
     --importance 0.8 \
     --tags "tag1,tag2"
   ```

3. Format content: "Brief title. Context. Rationale. Impact."
