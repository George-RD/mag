---
node: mag.runtime.memory.storage.sqlite
---
# mag.runtime.memory.storage.sqlite contract

SQLite is the durable local source of truth. Database work runs off the async
executor, writes are transactional, migrations are additive and idempotent,
FTS/vector/graph indexes are repairable, and caches never become durability
authorities.

In the current runtime, SQLite also implements most live retrieval, graph,
lifecycle, maintenance, and extended CLI/MCP behaviour. This describes the
audited implementation; it does not settle the composition-root decision. Any
future boundary change must preserve these behaviours through parity tests and
benchmarks before call sites move.

The database records the profile and embedding-space identity used for persisted
vectors. Re-embedding and dimension changes update the memory BLOB and vector
index through an interruption-safe, recoverable migration. Opening or querying
mixed, stale, or unknown embedding spaces fails visibly rather than returning
uncalibrated results.
