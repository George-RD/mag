# Re-embedding an existing database

`mag re-embed` migrates a database to the same pinned BGE profile used by normal
CLI startup. The current CLI does not expose arbitrary model selection. The
runtime API accepts an explicit `EmbeddingModel`; custom models must provide
their own stable profile identity rather than claim the default BGE identity.

## Offline maintenance only

Stop every MAG client, server, and embedded runtime sharing the database before
migrating. Restart them only after the command finishes. The transaction-level
write fence rejects stale vector writes, but an already-running process can
still serve stale semantic-query or cache results. Live read/cache generation
fencing remains tracked in issue #89 and
`todo.implement-embedding-space-migration`.

Use the same `MAG_DATA_ROOT` for inspection, migration, and subsequent startup.
`mag paths` reports the selected database path without opening the database.

```bash
mag paths
mag re-embed --dry-run
mag re-embed --batch-size 100
mag list --limit 1
```

The dry-run reports source and target identities, target dimensions, and the
number of affected memories without embedding or changing rows. A completed
migration reports `migrated_count` and `backup_path`; progress logs go to stderr
and the JSON report goes to stdout. A database already in the target space is a
no-op with no new backup.

The default model and tokenizer are pinned by revision and verified SHA-256
checksums. Actual migration may download them on first use. A build without
`real-embeddings` rejects the CLI command rather than using placeholder vectors.

## Recovery

Migration reserves the SQLite writer slot, creates a SQLite-consistent backup,
then changes memory vectors, the vector index, and the persisted identity in one
transaction. An embedding failure or interruption rolls back those changes.
Keep the reported backup after a successful migration. For a later rollback,
stop all runtimes and restore that complete backup with SQLite-aware tooling;
it retains the source-space identity and needs a source-compatible runtime.
Never replace database files underneath running MAG processes.

An existing database whose identity differs from the selected profile fails
normal startup. Use the explicit offline migration command; do not rewrite
`runtime_metadata` to make the identity check pass. A database missing identity
metadata must first be opened by its source-compatible runtime.
