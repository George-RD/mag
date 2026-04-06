# Contributing to MAG

> **Full guidance is in [AGENTS.md](./AGENTS.md)** — this document covers contributor-facing setup and process only.

## Prerequisites

- **Rust toolchain** — install via [rustup.rs](https://rustup.rs) (stable)
- **jj** (recommended) — [jj-vcs.github.io](https://jj-vcs.github.io/jj/latest/install-and-setup/); the repo uses jj in colocated mode
- **prek** (optional) — quality gate runner; install with `cargo install prek`

## Dev Setup

```bash
git clone https://github.com/George-RD/mag.git
cd mag
cargo build
cargo test --all-features
```

Model files (~134 MB) download automatically on first use and are cached at `~/.mag/models/`.

## Commit Convention

Use semantic commits:

```
<type>(<scope>): <description>
```

Examples: `feat(memory): add TTL sweep`, `fix(storage): handle lock contention`, `chore: update deps`

Common types: `feat`, `fix`, `perf`, `refactor`, `test`, `chore`, `docs`

All commits must include a DCO sign-off:

```
Signed-off-by: Your Name <your@email.com>
```

Add it with `jj describe`, including the sign-off line in the message:

```
jj describe -m "feat(scope): description

Signed-off-by: Your Name <your@email.com>"
```

## PR Process

1. Branch from `main` (`jj new main`)
2. Make changes, run quality gates (see below)
3. Push and open a PR against `main`
4. Address review feedback; squash or rebase as needed before merge

## Quality Gates

Every PR must pass all three gates:

```bash
# Run all three at once (requires prek):
prek run

# Or manually:
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

**Benchmark gate** — required if your change touches scoring, search, or storage:

```bash
./scripts/bench.sh --gate
```

See [AGENTS.md § Quality Gates](./AGENTS.md#quality-gates) for thresholds and full options.
