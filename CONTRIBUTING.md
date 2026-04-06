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

This runs a 2-sample benchmark, logs results to `docs/benchmarks/benchmark_log.csv`, and compares against the 10-sample baseline. Warns at >2 pp delta, fails at >5 pp. If the gate warns (>2 pp delta), run `./scripts/bench.sh --samples 10` for full validation before merging. See `scripts/bench.sh` for full options.

See [AGENTS.md § Quality Gates](./AGENTS.md#quality-gates) for the authoritative list.

## Running Benchmarks

```bash
# Fast iteration (2-sample, ~15s):
./scripts/bench.sh

# PR gate check (compares vs baseline):
./scripts/bench.sh --gate

# Full validation (10-sample, authoritative):
./scripts/bench.sh --samples 10 --notes "pre-merge validation"
```

See [AGENTS.md § Commands](./AGENTS.md#commands) for all benchmark variants.

## Maintainer Setup

Recommended branch protection for `main` on GitHub:

- Require pull request before merging
- Require at least 1 approving review
- Require status checks to pass (CI: `cargo fmt`, `cargo clippy`, `cargo test`)
- Restrict pushes to maintainers only
- Do not allow force pushes

> Branch protection rules are set in **Settings → Branches** on GitHub. These are not enforced automatically by this repository — they must be configured manually by a maintainer.
