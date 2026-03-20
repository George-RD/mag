# Security

MAG is a local-first memory system. This document describes the security model.

## Network

MAG makes zero outbound network calls during normal operation. It communicates only via stdio (MCP protocol) with the connecting client. Verify this yourself:

```bash
# macOS: monitor network activity during a MAG session
sudo nettop -p $(pgrep mag) -L 1
```

Network activity occurs:
- **On first use** — if the ONNX model is not cached locally, MAG auto-downloads it from Hugging Face before processing the first request
- **`mag download-model`** — explicitly pre-downloads the embedding model
- **`mag download-cross-encoder`** — explicitly pre-downloads the cross-encoder model
- **API embedders (optional)** — if configured to use Voyage or OpenAI, query text is sent to those services

After models are cached and with the default local embedder, MAG operates entirely offline.

## Data Location

All data lives in a single directory, usually `~/.mag/`:

| File | Purpose |
|---|---|
| `~/.mag/memory.db` | SQLite database — all memories, embeddings, metadata |
| `~/.mag/models/` | Cached ONNX model files |
| `~/.mag/benchmarks/` | Benchmark dataset cache (optional) |

There is no cloud sync, no telemetry, no analytics. The database is a standard SQLite file readable by any SQLite-compatible tool (DB Browser for SQLite, `sqlite3` CLI, etc.).

## Encryption at Rest

SQLite databases are stored as plaintext on disk. MAG does not implement application-level encryption.

**Recommendation:** Enable full-disk encryption on your operating system:
- **macOS:** FileVault (System Settings → Privacy & Security → FileVault)
- **Linux:** LUKS or dm-crypt
- **Windows:** BitLocker

If your threat model requires per-file encryption, use an encrypted filesystem or volume for the `~/.mag/` directory.

## Data Flow

### On store (ingest)

1. Client sends text via MCP `store_memory` tool
2. MAG tokenizes and embeds the text using the local ONNX model (no network)
3. Text, embedding vector, and metadata are written to `memory.db`
4. Entity extraction runs locally for auto-tagging
5. Response returned to client via stdio

### On retrieve (search/recall)

1. Client sends query via MCP tool (e.g., `search`, `recall`, `advanced_search`)
2. MAG embeds the query using the local ONNX model (no network)
3. Hybrid retrieval: FTS5 lexical search + vector similarity + graph traversal
4. Ranked results returned to client via stdio

### Nothing else

Data stays in-process — there are no external logs, crash reports, or usage tracking.

## Reporting Security Issues

If you find a security vulnerability, open a private security advisory on [GitHub](https://github.com/George-RD/mag/security/advisories/new). Do not open a public issue for security vulnerabilities.
