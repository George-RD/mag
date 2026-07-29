---
node: mag.runtime.memory.storage.sqlite
---
# mag.runtime.memory.storage.sqlite contract

SQLite is the durable local source of truth. Database work runs off the async
executor, writes are transactional, migrations are additive and idempotent,
FTS/vector/graph indexes are repairable, and caches never become durability
authorities.

The database records the profile and embedding-space identity used for persisted
vectors. Re-embedding and dimension changes update the memory BLOB and vector
index through an interruption-safe, recoverable migration. Opening or querying
mixed, stale, or unknown embedding spaces fails visibly rather than returning
uncalibrated results.
