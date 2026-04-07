<!-- MAG_MEMORY_START -->
<!-- MAG_VERSION: {{MAG_VERSION}} -->
# MAG — Persistent Memory

MAG provides persistent memory across sessions via an MCP server.

> **Safety:** Do not store secrets, credentials, or large code blocks in MAG memory.

## Store a memory

```bash
mag process "content to remember" --importance 0.8 --tags "tag1,tag2" --project <name>
```

Options: `--importance <0.0-1.0>` (default 0.5) · `--tags <tag1,tag2>` · `--project <name>` · `--event-type <type>` (decision, bugfix, observation…) · `--session-id <id>`

## Search memories

```bash
mag advanced-search "query" --limit 5 --project <name>
```

Options: `--limit <n>` (default 10) · `--project <name>` · `--importance-min <0.0-1.0>` · `--context-tags <tag1,tag2>` · `--explain`

## Session context (run at session start)

```bash
mag welcome --project <project_name>
```

## Save / resume a work checkpoint

```bash
mag checkpoint "task title" "progress description" --project <name> --next-steps "..."
mag resume-task --task-title "task title" --project <name>
```

Options: `--plan <text>` · `--next-steps <text>` · `--session-id <id>` · `--project <name>`

## Health & diagnostics

```bash
mag maintain --action health   # quick health check
mag stats                      # memory statistics
mag doctor --verbose           # full diagnostics
```
<!-- MAG_MEMORY_END -->
