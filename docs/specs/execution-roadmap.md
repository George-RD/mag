# Historical substrate execution roadmap

This path previously held the v0.1.9–v0.2.2 structural campaign and described a
future wholesale substrate campaign. That work is retained as historical design
and implementation evidence, but this file is no longer a source of current work
status or architectural authority.

The complete pre-retirement document is pinned at:

- [Execution roadmap at the selected-runtime decision baseline](https://github.com/George-RD/mag/blob/64d2004b656ac44aaa9bf78ffb5ec2fc7947369a/docs/specs/execution-roadmap.md)

Current authority is:

1. [`local-first-roadmap.md`](local-first-roadmap.md) for mission, dependency
   order, phases, and quality gates;
2. accepted Cairn decisions and contracts for the nodes they name;
3. `meta/todos/`, queried with `cairn status`, `cairn next`, and
   `cairn brief todo.<slug>`, for work status.

`dec.select-local-runtime-composition-root` rejects wholesale promotion of the
feature-gated substrate. MAG will introduce one entrypoint-owned,
transport-independent local runtime over the verified SQLite-backed behaviour,
migrate CLI and MCP through bounded parity-tested slices, and retire duplicate
legacy/substrate orchestration after the compatibility period.

Useful scoring, reranking, retrieval, benchmark, and decomposition work from the
historical campaign remains in the code. Re-verify any old PR item or metric
against the current repository before using it.
