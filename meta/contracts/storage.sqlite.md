---
        node: mag.runtime.memory.storage.sqlite
        ---
        # mag.runtime.memory.storage.sqlite contract

        SQLite is the durable local source of truth. Database work runs off the
async executor, writes are transactional, migrations are additive and
idempotent, FTS/vector/graph indexes are repairable, and caches never become
durability authorities.
