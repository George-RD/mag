# Tech Stack: MAG

## Core Language
- **Rust:** The primary language for the rewrite, ensuring memory safety, high performance, and zero-dependency portability.

## CLI Framework
- **Clap (Command Line Argument Parser):** Used for building a robust and user-friendly CLI with support for subcommands, flags, and help generation.

## Core Components (Implemented)
- **Serialization:** `serde` (v1.0.228) with `derive` features.
- **Async Runtime:** `tokio` (v1.49.0) with `full` features.
- **Async Abstractions:** `async-trait` (v0.1.89) for modular trait definitions.
- **Error Handling:** `anyhow` (v1.0.101) for flexible and detailed error reporting.
- **Logging/Tracing:** `tracing` (v0.1.44) and `tracing-subscriber` (v0.3.22) for structured logging.

## Planned Components
- **Storage Backends:**
    - `rusqlite` for local SQLite support.
    - Potential abstractions for external vector databases.
