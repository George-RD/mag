# Track Specification: SQLite Storage Backend

## Overview

This track replaces the current `PlaceholderPipeline` implementation for the `Storage` and `Retriever` traits with a persistent SQLite-based backend. This will enable `romega-memory` to store and retrieve data across different CLI sessions and provides the foundation for the semantic graph memory system found in the original `omega-memory`.

## Functional Requirements

- **Persistent Storage:** Implement the `Storage` and `Retriever` traits using the `rusqlite` library.
- **Default Database Location:** Store the SQLite database at a fixed default path: `~/.romega-memory/memory.db`.
- **Automatic Initialization:** Automatically create the parent directory and initialize the SQLite database file with the required schema if they do not exist on startup.
- **Rich Metadata & Graph Schema:** The storage schema must support the following:
    - **`memories` Table:**
        - `id`: Unique identifier (UUID/Primary Key).
        - `content`: The processed memory string.
        - `embedding`: `BLOB` placeholder for future vector storage (semantic search).
        - `parent_id`: `TEXT` reference to another memory (for simple hierarchy).
        - `created_at`: Automatic timestamp of when the entry was created.
        - `event_at`: A user-provided timestamp for historical data, defaulting to `created_at`.
        - `content_hash`: A SHA-256 hash of the content for duplicate detection.
        - `source_type`: A string identifying the source (e.g., "cli_input").
        - `last_accessed_at`: Timestamp updated on every retrieval.
        - `tags`: A string or comma-separated list for simple categorization.
    - **`relationships` Table:**
        - `id`: Unique identifier.
        - `source_id`: Reference to a memory ID.
        - `target_id`: Reference to a memory ID.
        - `rel_type`: Type of relationship (e.g., "links_to", "conflicts_with").
- **Configuration Flow:** Implement a basic "Default vs. Advanced" initialization flow. "Default" uses the hardcoded path; "Advanced" is a placeholder for future extensions.

## Non-Functional Requirements

- **Performance:** Sub-millisecond response times for local lookups.
- **Data Integrity:** Use SQLite transactions for atomic storage of content and metadata.
- **Portability:** Maintain a zero-dependency requirement for the end-user (aside from the binary).

## Acceptance Criteria

- [ ] Running `romega ingest "some content"` stores the data in the local SQLite file.
- [ ] Subsequent calls to `romega retrieve <id>` correctly return the stored content.
- [ ] All specified schema tables (`memories`, `relationships`) and columns are correctly created and populated.
- [ ] The `~/.romega-memory/` directory and `memory.db` file are created automatically if missing.
- [ ] Unit tests for the new `SqliteStorage` module pass with high coverage.

## Out of Scope

- **Semantic Search Implementation:** Generating and querying embeddings is deferred to a future "Vector Search" track.
- **Complex Graph Traversal:** Deep relationship queries will be handled in a "Graph Analytics" track.
- **Remote Backends:** Support for external databases (e.g., Pinecone, Weaviate).
