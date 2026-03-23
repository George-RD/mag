# CLI Reference

All commands follow the form `mag <command> [args] [flags]`. Run `mag --help` or `mag <command> --help` for built-in usage.

Global flag: `--init-mode <default|advanced>` (default: `default`).

---

## Core

### `mag ingest <content>`

Store a memory.

| Flag | Default | Description |
|------|---------|-------------|
| `--tags` | -- | Comma-separated tags |
| `--importance` | `0.5` | Priority weight (0.0-1.0) |
| `--event-type` | -- | Category (e.g., `decision`, `bugfix`, `preference`) |
| `--session-id` | -- | Session identifier |
| `--project` | -- | Project scope |
| `--priority` | -- | Integer priority |
| `--entity-id` | -- | Entity identifier |
| `--agent-type` | -- | Agent type label |
| `--metadata` | -- | Arbitrary metadata string |
| `--ttl-seconds` | -- | Time-to-live in seconds (non-negative) |
| `--referenced-date` | -- | ISO 8601 timestamp for when the event occurred |

```bash
mag ingest "Deploy requires manual approval in staging" \
  --tags "deploy,staging" --importance 0.8

mag ingest "Use exponential backoff with jitter for retries" \
  --tags "pattern,retry" --event-type decision --project myapp
```

### `mag process <content>`

Alias for `ingest`. Accepts the same flags.

### `mag retrieve <id>`

Fetch a single memory by its ID.

```bash
mag retrieve 01J5K9X2...
```

### `mag delete <id>`

Delete a memory by its ID.

```bash
mag delete 01J5K9X2...
```

### `mag update <id>`

Update an existing memory's content, tags, importance, metadata, event type, or priority.

| Flag | Description |
|------|-------------|
| `--content` | New content text |
| `--tags` | Replacement tags (comma-separated) |
| `--importance` | New importance score |
| `--metadata` | New metadata string |
| `--event-type` | New event type |
| `--priority` | New priority |

```bash
mag update 01J5K9X2... --content "Updated deploy process" --importance 0.9
```

---

## Search

All search commands accept these shared filter flags:

| Flag | Description |
|------|-------------|
| `--event-type` | Filter by event type |
| `--project` | Filter by project |
| `--session-id` | Filter by session |
| `--entity-id` | Filter by entity |
| `--agent-type` | Filter by agent type |
| `--include-superseded` | Include superseded memories |
| `--importance-min` | Minimum importance threshold |
| `--created-after` | ISO 8601 lower bound on creation time |
| `--created-before` | ISO 8601 upper bound on creation time |
| `--context-tags` | Comma-separated tag filter |
| `--event-after` | ISO 8601 lower bound on event time |
| `--event-before` | ISO 8601 upper bound on event time |

### `mag search <query>`

Full-text search over stored memories.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Maximum results |

```bash
mag search "retry backoff" --limit 5
mag search "deploy" --project myapp --importance-min 0.7
```

### `mag semantic-search <query>`

Pure vector similarity search using ONNX embeddings.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Maximum results |

```bash
mag semantic-search "error handling strategies"
```

### `mag advanced-search <query>`

Multi-phase retrieval pipeline: vector + FTS5 BM25, RRF fusion, optional cross-encoder rerank, score refinement, graph enrichment, and abstention gating.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Maximum results |
| `--explain` | `false` | Show scoring breakdown |

```bash
mag advanced-search "how do we handle database migrations" --explain
mag advanced-search "retry logic" --limit 3 --project myapp
```

### `mag phrase-search <phrase>`

Exact phrase match search.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Maximum results |

```bash
mag phrase-search "exponential backoff"
```

### `mag similar <id>`

Find memories with similar embeddings to a given memory.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `5` | Maximum results |

```bash
mag similar 01J5K9X2... --limit 10
```

---

## Browse

### `mag list`

List stored memories with pagination.

| Flag | Default | Description |
|------|---------|-------------|
| `--offset` | `0` | Pagination offset |
| `--limit` | `10` | Page size |

Accepts all shared filter flags.

```bash
mag list --limit 20 --project myapp
mag list --event-type decision --importance-min 0.8
```

### `mag recent`

Show recently accessed memories.

| Flag | Default | Description |
|------|---------|-------------|
| `--limit` | `10` | Maximum results |

Accepts all shared filter flags.

```bash
mag recent --limit 5
```

### `mag relations <id>`

Show relationships for a given memory (PRECEDED_BY, RELATES_TO, etc.).

```bash
mag relations 01J5K9X2...
```

### `mag traverse <id>`

Walk the relationship graph from a memory using BFS.

| Flag | Default | Description |
|------|---------|-------------|
| `--max-hops` | `2` | Maximum graph traversal depth |
| `--min-weight` | `0.0` | Minimum edge weight to follow |

```bash
mag traverse 01J5K9X2... --max-hops 3 --min-weight 0.5
```

### `mag version-chain <id>`

Get the version history chain for a memory (all edits/supersessions).

```bash
mag version-chain 01J5K9X2...
```

---

## Lifecycle

### `mag feedback <memory_id> <rating>`

Rate a memory to influence future retrieval scoring. Rating must be one of: `helpful`, `unhelpful`, `outdated`.

| Flag | Description |
|------|-------------|
| `--reason` | Optional text explanation |

```bash
mag feedback 01J5K9X2... helpful
mag feedback 01J5K9X2... outdated --reason "API changed in v3"
```

### `mag sweep`

Run TTL-based cleanup. Deletes expired memories (those past their `ttl_seconds`).

```bash
mag sweep
```

### `mag maintain --action <action>`

System maintenance operations.

| Action | Description |
|--------|-------------|
| `health` | Run health check |
| `consolidate` | Merge duplicate/similar memories |
| `compact` | Optimize database storage |
| `clear-session` | Clear session data |
| `backup` | Create a database backup |
| `backup-list` | List available backups |
| `backup-restore` | Restore from a backup |

| Flag | Description |
|------|-------------|
| `--warn-mb` | Warning threshold for database size (MB) |
| `--critical-mb` | Critical threshold for database size (MB) |
| `--max-nodes` | Maximum graph nodes for pruning |
| `--prune-days` | Age threshold for pruning (days) |
| `--max-summaries` | Maximum summaries to retain |
| `--event-type` | Filter maintenance to specific event type |
| `--similarity-threshold` | Similarity threshold for consolidation |
| `--min-cluster-size` | Minimum cluster size for consolidation |
| `--dry-run` | Preview changes without applying |
| `--session-id` | Session to clear |
| `--backup-path` | Path to backup file (for `backup-restore`) |

```bash
mag maintain --action health
mag maintain --action consolidate --similarity-threshold 0.9 --dry-run
mag maintain --action backup
mag maintain --action backup-restore --backup-path ~/.mag/backups/2026-03-23.db
```

---

## Cross-Session

### `mag checkpoint <task_title> <progress>`

Save a work-in-progress checkpoint for resumption later.

| Flag | Description |
|------|-------------|
| `--plan` | Overall plan text |
| `--next-steps` | What to do next |
| `--session-id` | Session identifier |
| `--project` | Project scope |

```bash
mag checkpoint "Auth refactor" "Completed token validation, middleware pending" \
  --next-steps "Wire up middleware, add tests" --project myapp
```

### `mag resume-task`

Resume a previously checkpointed task.

| Flag | Default | Description |
|------|---------|-------------|
| `--task-title` | -- | Find checkpoint by title |
| `--project` | -- | Find checkpoint by project |
| `--limit` | `1` | Number of checkpoints to return |

```bash
mag resume-task --task-title "Auth refactor"
mag resume-task --project myapp --limit 3
```

### `mag remind <action>`

Manage reminders. Action is the operation to perform (e.g., `set`, `list`, `dismiss`).

| Flag | Description |
|------|-------------|
| `--text` | Reminder text |
| `--duration` | Duration string (e.g., `30m`, `2h`) |
| `--context` | Additional context |
| `--session-id` | Session identifier |
| `--project` | Project scope |
| `--status` | Filter by status |
| `--reminder-id` | Target a specific reminder |

```bash
mag remind set --text "Review PR #42" --duration 2h --project myapp
mag remind list --project myapp
mag remind dismiss --reminder-id abc123
```

### `mag lessons`

Retrieve lessons learned from past sessions.

| Flag | Default | Description |
|------|---------|-------------|
| `--task` | -- | Filter by task |
| `--project` | -- | Filter by project |
| `--limit` | `5` | Maximum results |

```bash
mag lessons --project myapp --limit 10
mag lessons --task "database migrations"
```

### `mag welcome`

Session startup briefing. Returns context from recent activity, pending tasks, and relevant memories.

| Flag | Description |
|------|-------------|
| `--session-id` | Session identifier |
| `--project` | Project scope |

```bash
mag welcome --project myapp
```

### `mag profile <action> [data]`

Manage user profile information. Action is `get` or `set`.

```bash
mag profile set "Prefers Rust, uses Neovim, deploys to Fly.io"
mag profile get
```

---

## System

### `mag stats`

Show memory store statistics (count, size, tags).

```bash
mag stats
```

### `mag stats-extended --action <action>`

Detailed statistics.

| Action | Description |
|--------|-------------|
| `types` | Memory count by event type |
| `sessions` | Session activity summary |
| `digest` | Activity digest |
| `access-rate` | Access frequency analysis |

| Flag | Default | Description |
|------|---------|-------------|
| `--days` | `7` | Lookback window in days |

```bash
mag stats-extended --action digest --days 30
mag stats-extended --action types
```

### `mag export`

Export all memories and relationships as JSON to stdout.

```bash
mag export > backup.json
```

### `mag import <path>`

Import memories and relationships from a JSON file. Use `-` for stdin.

```bash
mag import backup.json
mag export | mag import -
```

### `mag paths`

Show the active MAG data, model, and benchmark cache paths.

```bash
mag paths
```

### `mag doctor`

Check MAG setup and report diagnostics.

| Flag | Description |
|------|-------------|
| `--verbose` | Show detailed output |

```bash
mag doctor
mag doctor --verbose
```

### `mag protocol`

Show available MCP tools and operational guidelines.

| Flag | Description |
|------|-------------|
| `--section` | Show a specific section only |

```bash
mag protocol
mag protocol --section tools
```

### `mag serve`

Start the MCP server over stdio transport.

| Flag | Default | Description |
|------|---------|-------------|
| `--cross-encoder` | `false` | Enable cross-encoder reranking |

```bash
mag serve
mag serve --cross-encoder
```

### `mag download-model`

Download the ONNX embedding model and tokenizer. Run this once before first use if you want to avoid download delays.

```bash
mag download-model
```

### `mag download-cross-encoder`

Download the cross-encoder model used for reranking.

```bash
mag download-cross-encoder
```
