## Summary

<!-- Briefly describe what this PR does and why. Reference the issue if applicable (e.g. Closes #123). -->

## Quality Gate Checklist

- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo test --all-features` passes
- [ ] Benchmark shows no regression (`./scripts/bench.sh --gate`) — required if touching scoring, search, or storage
- [ ] New public APIs have tests

## Notes for Reviewers

<!-- Anything unusual about the approach, trade-offs made, or areas that need extra attention. -->
