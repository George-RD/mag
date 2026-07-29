---
node: mag.runtime.memory.storage.sqlite
---
# mag.runtime.memory.storage.sqlite contract

SQLite is the durable local source of truth. Database work runs off the async
executor, writes are transactional, migrations are additive and idempotent,
FTS/vector/graph indexes are repairable, and caches never become durability
authorities.

SQLite currently implements most live retrieval, graph, lifecycle, maintenance,
and extended CLI/MCP behaviour. The selected local runtime delegates to that
verified implementation first; it must not rewrite those semantics merely to
introduce the composition boundary. SQLite is the production backend and current
semantic implementation, but it is not the application composition root.

Moving an algorithm or caller behind a narrower runtime capability requires
public parity tests. Retrieval, scoring, reranking, or storage changes require
the benchmark and local quality gates before the previous path is removed.

The database records the profile and embedding-space identity used for persisted
vectors. Re-embedding and dimension changes update the memory BLOB and vector
index through an interruption-safe, recoverable migration. Opening or querying
mixed, stale, or unknown embedding spaces fails visibly rather than returning
uncalibrated results.
