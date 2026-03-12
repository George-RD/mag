# Rust Style Guide: MAG

## 1. Tooling & Linting
- **Formatting:** Use `rustfmt` for all code. Run `cargo fmt` before every commit.
- **Linting:** Use `clippy`. All code must pass `cargo clippy --all-targets --all-features -- -D warnings`.
- **Documentation:** Use `cargo doc` to ensure all public APIs are documented and links are valid.

## 2. Coding Standards
- **Idiomatic Rust:** Follow the conventions in "The Rust Programming Language" and "Rust API Guidelines".
- **Error Handling:** Use `Result` for recoverable errors. Prefer specific error types (using crates like `thiserror`) over generic ones. Avoid `unwrap()` or `expect()` in production code unless the invariant is clearly documented.
- **Async Code:** When using `tokio`, ensure tasks are properly managed and avoid blocking the async executor.
- **Safety:** Minimize the use of `unsafe` code. If `unsafe` is necessary, it must be encapsulated in a safe abstraction and accompanied by a safety comment explaining why it is sound.

## 3. Testing
- **Unit Tests:** Place unit tests in the same file as the code being tested in a `mod tests` block.
- **Integration Tests:** Place integration tests in the `tests/` directory.
- **Documentation Tests:** Use doc-tests to verify examples in documentation.
- **Test Coverage:** Aim for high coverage of core logic, focusing on edge cases and error paths.
