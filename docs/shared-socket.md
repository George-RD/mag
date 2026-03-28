# Shared Socket — One MAG Instance, All Tools
<!-- Last verified: 2026-03-28 | Valid for: v0.1.2+ -->

By default, each MCP client starts its own MAG process. This means each tool maintains its own connection to the database. SQLite handles concurrent reads well, but concurrent writes from multiple processes can cause lock contention.

## Why Share

- **Consistency.** One process, one write path. No WAL contention across processes.
- **Lower resource usage.** One ONNX model loaded in memory instead of N copies.
- **Simpler debugging.** One log stream, one process to monitor.

## How It Works

SQLite with WAL mode (which MAG uses) supports multiple readers and one writer. Multiple MAG processes reading the same `memory.db` file works fine. Write contention only becomes an issue under heavy concurrent ingest from multiple tools simultaneously.

For most users (1-3 tools, occasional writes), running separate MAG processes per tool is fine. The shared-socket approach is for power users who:
- Run 4+ MCP clients simultaneously
- Do heavy batch ingestion across tools
- Want a single log stream

## Setup

### Option 1: Shared Database (Recommended for Most Users)

All MAG instances already share the same database file (`~/.mag/memory.db`) by default. No configuration needed. SQLite WAL mode handles concurrent access.

### Option 2: Single Process (Power Users)

Run one MAG instance as a long-lived process and point all tools at it. This requires a TCP or Unix socket transport instead of stdio.

> **Note:** MAG defaults to stdio transport for MCP. HTTP daemon mode is available behind the `daemon-http` feature flag (`mag serve` with HTTP transport). See the daemon documentation for details. Until TCP/Unix socket transport is added, Option 1 (shared database file) remains the recommended approach for multi-client setups.

## What Breaks with Two Instances

Concurrent reads work correctly. Concurrent writes may see transient `SQLITE_BUSY` errors under heavy load, but MAG retries automatically with bounded backoff (5 attempts, 10-160 ms). The "shared socket" optimization is about resource efficiency, not correctness.
