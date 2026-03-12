# Track Specification: Project Scaffolding & Core Architecture Design

## Objective
Establish a robust, modular, and extensible foundation for the `mag` rewrite in Rust. This includes the project structure, CLI entry point, and the core abstractions (traits) for the memory pipeline.

## Requirements
- Define the core pipeline traits: `Ingestor`, `Processor`, `Storage`, and `Retriever`.
- Set up a standard Rust project structure using `cargo`.
- Implement a basic CLI entry point using `clap`.
- Ensure all components are designed for zero-dependency portability.

## Success Criteria
- `cargo check` and `cargo clippy` pass with zero warnings.
- Architecture supports swappable modules as per product guidelines.
- Foundational traits are documented and tested.
