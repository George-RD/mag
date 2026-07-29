---
node: mag.runtime.entrypoints
status: blocked
created: 2026-07-29
---
# Migrate CLI command families to the local runtime

Blocked by `todo.introduce-local-memory-runtime-facade`.

Move production CLI callers through the selected runtime in bounded command
families rather than one large switch:

1. ingest/process, with byte-for-byte regression coverage for the current
   `processed: ` stored-content behaviour;
2. retrieve/delete/update and other basic CRUD;
3. search, semantic/advanced retrieval, graph, session, and administration.

Each PR must begin with a failing caller-level parity test, keep the previous
delegate easy to restore, and preserve JSON/stdout behaviour. Retrieval,
scoring, reranking, or storage changes require the benchmark gate and are not
permitted to hide inside a caller migration.
