## Memory (MAG)

You have persistent memory via MAG. Memories survive across sessions and are shared across all your AI tools.

### When to use memory

**Store** (importance 0.7-0.9): architectural decisions with rationale, bug fixes with root cause, user preferences, workflow conventions, project patterns.

**Search** when: user references prior work, you need project context, debugging a recurring issue, making a decision that might have precedent.

**Skip** memory for: syntax questions, trivial edits, one-off tasks, anything the codebase itself answers.

### How

Use `mag hook` CLI commands via Bash:
- `mag hook search "query" --project PROJECT` — search memories
- `mag hook store "content" --project PROJECT --event-type TYPE` — store a memory
- `mag checkpoint "title" "progress"` — save task state for handoff
- `mag resume-task` — resume from last checkpoint

### Behavior

- Do not announce memory operations. Weave recalled context naturally.
- Do not store secrets, credentials, or large code blocks.
- Store signal, not noise. If memory_search returns nothing, do not mention it.
