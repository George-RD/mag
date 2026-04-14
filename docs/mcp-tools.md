# MCP Tools Reference
<!-- Last verified: 2026-04-14 | Valid for: v0.1.9+ -->

MAG exposes 19 tools via the Model Context Protocol. Any MCP-compatible client can use these tools.

## Storage

### `memory_store`

Store a new memory.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| content | string | yes | -- | Memory content |
| id | string | no | UUID v4 | Custom memory ID |
| tags | string[] | no | [] | Classification tags |
| importance | number | no | 0.5 | Priority weight (0.0--1.0) |
| metadata | object | no | {} | Arbitrary JSON metadata |
| event_type | string | no | -- | Memory type (see Event Types below) |
| session_id | string | no | -- | Session identifier |
| project | string | no | -- | Project scope |
| priority | integer | no | -- | Priority level |
| entity_id | string | no | -- | Entity identifier |
| agent_type | string | no | -- | Agent type label |
| ttl_seconds | integer | no | -- | Time-to-live in seconds (auto-expires) |
| referenced_date | string | no | -- | ISO 8601 timestamp for when the event actually occurred |

### `memory_store_batch`

Batch store multiple memories with optimized embedding computation. Pre-warms the embedding cache with a single batched inference call for better throughput.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| items | StoreRequest[] | yes | -- | Array of memory objects (same schema as `memory_store`) |

### `memory_retrieve`

Retrieve a memory by ID.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Memory ID |

### `memory_delete`

Delete a memory by ID.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Memory ID |

### `memory_update`

Update an existing memory. At least one optional field must be provided.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Memory ID |
| content | string | no | -- | New content |
| tags | string[] | no | -- | Replacement tags |
| importance | number | no | -- | New importance (0.0--1.0) |
| metadata | object | no | -- | New metadata |
| event_type | string | no | -- | New event type |
| priority | integer | no | -- | New priority |

---

## Search

### `memory_search`

Unified search with five modes and an optional advanced multi-phase retrieval path.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| mode | string | no | "text" | Search mode (see below) |
| advanced | boolean | no | true for text | Enable 6-phase RRF pipeline (text and semantic only) |
| query | string | mode-dep | -- | Query string (required for text, semantic, phrase) |
| tags | string[] | mode-dep | -- | Tags to match (required for tag mode, AND logic) |
| memory_id | string | mode-dep | -- | Source memory ID (required for similar mode) |
| limit | integer | no | 10 | Maximum results |
| event_type | string | no | -- | Filter by event type |
| project | string | no | -- | Filter by project |
| session_id | string | no | -- | Filter by session |
| include_superseded | boolean | no | -- | Include superseded memories |
| event_after | string | no | -- | ISO 8601 lower bound for event_at |
| event_before | string | no | -- | ISO 8601 upper bound for event_at |
| importance_min | number | no | -- | Minimum importance threshold |
| created_after | string | no | -- | ISO 8601 lower bound for created_at |
| created_before | string | no | -- | ISO 8601 upper bound for created_at |
| context_tags | string[] | no | -- | Context tags for scoring boost |
| explain | boolean | no | -- | Inject component scores into `_explain` metadata |

#### Search Modes

| Mode | Required Params | Description | Use Case |
|------|----------------|-------------|----------|
| `text` | query | FTS5 full-text search with BM25 ranking | Keyword lookups, general queries |
| `semantic` | query | ONNX embedding cosine similarity | Conceptual/meaning-based queries |
| `phrase` | query | Exact substring match | Finding specific phrases or error messages |
| `tag` | tags | AND-match on tags | Browsing by category |
| `similar` | memory_id | Find memories similar to a given one | "More like this" discovery |

**Advanced mode** (`advanced=true`) enables the full 6-phase retrieval pipeline: vector search + FTS5 BM25 --> RRF fusion --> cross-encoder rerank --> score refinement --> graph enrichment --> abstention gate. Only `text` and `semantic` modes support it. Text mode defaults to `advanced=true`.

When advanced mode is active, the response includes `result_count`, `abstained` (boolean), and `confidence` (max text overlap score). If the abstention gate fires (no results met the relevance threshold), `abstained` is true and `results` is empty.

### `memory_list`

List memories with pagination and filtering.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| sort | string | no | "created" | Sort order: "created" (paginated) or "recent" (recently accessed) |
| offset | integer | no | 0 | Pagination offset (created sort only) |
| limit | integer | no | 10 | Maximum results |
| event_type | string | no | -- | Filter by event type |
| project | string | no | -- | Filter by project |
| session_id | string | no | -- | Filter by session |
| include_superseded | boolean | no | -- | Include superseded memories |
| event_after | string | no | -- | ISO 8601 lower bound for event_at |
| event_before | string | no | -- | ISO 8601 upper bound for event_at |
| importance_min | number | no | -- | Minimum importance threshold |
| created_after | string | no | -- | ISO 8601 lower bound for created_at |
| created_before | string | no | -- | ISO 8601 upper bound for created_at |
| context_tags | string[] | no | -- | Context tags |

---

## Relationships

### `memory_relations`

Manage memory relationships and graph traversal.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "list" | Action: list, add, traverse, version_chain |

#### Action: `list`

Get all relationships for a memory.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Memory ID |

#### Action: `add`

Create a directed relationship between two memories.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| source_id | string | yes | -- | Source memory ID |
| target_id | string | yes | -- | Target memory ID |
| rel_type | string | yes | -- | Relationship type (e.g., PRECEDED_BY, RELATES_TO, SIMILAR_TO, SUPERSEDES) |
| weight | number | no | 1.0 | Edge weight (0.0--1.0) |
| metadata | object | no | {} | Edge metadata |

#### Action: `traverse`

BFS graph traversal from a starting memory.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Starting memory ID |
| max_hops | integer | no | 2 | Maximum traversal depth (1--5) |
| min_weight | number | no | 0.0 | Minimum edge weight threshold |

#### Action: `version_chain`

Get the full version history of a memory.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| id | string | yes | -- | Memory ID |

---

## Lifecycle

### `memory_feedback`

Record user feedback signal for a memory. Feedback affects future search ranking.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| memory_id | string | yes | -- | Memory ID |
| rating | string | yes | -- | One of: "helpful", "unhelpful", "outdated" |
| reason | string | no | -- | Explanation for the rating |

### `memory_lifecycle`

System maintenance operations.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "sweep" | Action (see below) |

#### Action: `sweep`

Expire memories that have exceeded their TTL. No additional parameters.

#### Action: `health`

Run diagnostic health check.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| warn_mb | number | no | 350.0 | Warning threshold in MB |
| critical_mb | number | no | 800.0 | Critical threshold in MB |
| max_nodes | integer | no | 10000 | Maximum node count threshold |

#### Action: `consolidate`

Prune stale data.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| prune_days | integer | no | 30 | Age threshold in days |
| max_summaries | integer | no | 50 | Maximum summaries to retain |

#### Action: `compact`

Merge near-duplicate memories.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| event_type | string | no | "lesson_learned" | Event type to compact |
| similarity_threshold | number | no | 0.6 | Similarity threshold for clustering |
| min_cluster_size | integer | no | 3 | Minimum cluster size to merge |
| dry_run | boolean | no | false | Preview without applying changes |

#### Action: `auto_compact`

Embedding-based automatic deduplication.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| count_threshold | integer | no | 500 | Memory count threshold to trigger compaction |
| dry_run | boolean | no | false | Preview without applying changes |

#### Action: `clear_session`

Remove all data for a session.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| session_id | string | yes | -- | Session ID to clear |

#### Action: `backup`

Create a binary backup of the database. Retains the 5 most recent backups. No additional parameters.

#### Action: `backup_list`

List available backups. No additional parameters.

---

## Cross-Session

### `memory_checkpoint`

Manage cross-session task checkpoints for resuming work.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "save" | Action: "save" or "resume" |

#### Action: `save`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| task_title | string | yes | -- | Task title |
| progress | string | yes | -- | Progress description |
| plan | string | no | -- | Current plan |
| files_touched | object | no | -- | Files modified |
| decisions | string[] | no | -- | Decisions made |
| key_context | string | no | -- | Important context |
| next_steps | string | no | -- | What to do next |
| session_id | string | no | -- | Session identifier |
| project | string | no | -- | Project scope |

#### Action: `resume`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| task_title | string | no | "" | Filter checkpoints by task title |
| project | string | no | -- | Filter by project |
| limit | integer | no | 1 | Number of checkpoints to return |

### `memory_remind`

Set, list, or dismiss time-based reminders.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "set" | Action: "set", "list", "dismiss" |

#### Action: `set`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| text | string | yes | -- | Reminder text |
| duration | string | yes | -- | Duration string (e.g., "2h", "1d") |
| context | string | no | -- | Additional context |
| session_id | string | no | -- | Session identifier |
| project | string | no | -- | Project scope |

#### Action: `list`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| status | string | no | -- | Filter by status |

#### Action: `dismiss`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| reminder_id | string | yes | -- | Reminder ID to dismiss |

### `memory_lessons`

Query `lesson_learned` memories relevant to a task or project.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| task | string | no | -- | Task description to match |
| project | string | no | -- | Project filter |
| limit | integer | no | 5 | Maximum results |
| exclude_session | string | no | -- | Session ID to exclude |
| agent_type | string | no | -- | Agent type filter |

### `memory_profile`

Read or update the persistent cross-session user profile.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "read" | Action: "read" or "update" |
| update | object | no | -- | JSON payload (required for action=update) |

---

## System

### `memory_admin`

Administrative actions for health monitoring, data export, and import.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| action | string | no | "health" | Action: "health", "export", "import" |

#### Action: `health`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| detail | string | no | "basic" | Detail level: basic, stats, types, sessions, digest, access_rate |
| days | integer | no | 7 | Days for digest detail level |

#### Action: `export`

Dump all memories and relationships as JSON. No additional parameters.

#### Action: `import`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| data | string | yes | -- | JSON data to import (output of export) |

### `memory_session_info`

Session-oriented information and tool discovery.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| mode | string | no | "welcome" | Mode: "welcome" or "protocol" |
| session_id | string | no | -- | Session identifier |
| project | string | no | -- | Project scope |

- **welcome** -- Returns the startup briefing with recent memories, pending reminders, and session context.
- **protocol** -- Returns the full tool inventory and usage guidelines.

---

## Event Types

The `event_type` parameter accepts any of the following values:

| Event Type | Description |
|------------|-------------|
| session_summary | Session summary |
| task_completion | Completed task record |
| error_pattern | Recurring error pattern |
| lesson_learned | Lesson or insight |
| decision | Architectural/design decision |
| blocked_context | Blocked or stalled context |
| user_preference | User preference |
| user_fact | User fact |
| advisor_insight | Advisor insight |
| git_commit | Git commit record |
| git_merge | Git merge record |
| git_conflict | Git conflict record |
| session_start | Session start marker |
| session_end | Session end marker |
| context_warning | Context window warning |
| budget_alert | Budget alert |
| coordination_snapshot | Multi-agent coordination snapshot |
| checkpoint | Task checkpoint |
| reminder | Reminder |
| memory | General memory |
| code_chunk | Code chunk |
| file_summary | File summary |
