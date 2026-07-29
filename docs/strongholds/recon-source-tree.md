# MAG Rust Source Code Structure — Complete Recon Map

> **Historical snapshot — 2026-04-14.** File counts, module sizes, and ownership claims below are dated evidence.
> Use `cairn bundle <node>` and the current source tree for implementation decisions.

**Generated**: 2026-04-14  
**Scout**: Uruk-hai Agent  
**Stronghold Path**: `/Users/george/repos/mag/docs/strongholds/recon-source-tree.md`

---

## Executive Summary

The MAG codebase comprises **41 Rust source files** across two main domains:
- **Domain Layer**: Memory system abstractions (traits, domain models)
- **Storage/Backend Layer**: SQLite-backed implementation with pluggable embeddings
- **CLI/Server Layer**: Command-line interface and MCP protocol server

**Key Findings**:
- **9 god modules** identified (>500 lines) — primarily in sqlite storage subsystem
- **28 public traits** — core abstractions for memory operations
- **22 core domain structs** — data models and pipeline configurations
- **Module hierarchy**: Well-separated concerns (domain → traits → storage → sqlite)
- **Concerns Analysis**: SQLite module handles too many responsibilities (CRUD, graph ops, lifecycle, NLP)

---

## I. File Inventory with Line Counts
