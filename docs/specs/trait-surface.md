# Historical substrate trait-surface design

This path previously specified seven substrate interfaces, a broad `MemoryStore`
supertrait, candidate orchestrators, and a deprecation campaign. It remains useful
design research, but it is not an approved implementation campaign or a current
production contract.

The complete pre-retirement document is pinned at:

- [Trait-surface design at the selected-runtime decision baseline](https://github.com/George-RD/mag/blob/64d2004b656ac44aaa9bf78ffb5ec2fc7947369a/docs/specs/trait-surface.md)

`dec.select-local-runtime-composition-root` rejects wholesale promotion of the
current feature-gated substrate. MAG instead introduces one entrypoint-owned,
transport-independent local runtime over the verified SQLite-backed behaviour.
Only narrow interfaces that solve a demonstrated migration need may be folded
into that live path, with public parity coverage and the benchmark/evaluation
gates required for retrieval, scoring, reranking, or storage changes.

Use these live Cairn items rather than inferring work from this historical spec:

```bash
cairn brief todo.introduce-local-memory-runtime-facade
cairn brief todo.migrate-cli-to-local-memory-runtime
cairn brief todo.migrate-mcp-to-local-memory-runtime
cairn brief todo.retire-legacy-and-substrate-orchestration
```

The broad substrate `MemoryStore`, duplicate query context, and candidate
orchestrators do not become production extension points by default.
