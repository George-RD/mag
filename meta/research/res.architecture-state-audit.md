---
id: res.architecture-state-audit
nodes: [mag.runtime, mag.runtime.memory, mag.runtime.substrate]
sources: [src.mag-agents-guide, src.dead-code-recon, src.source-tree-recon]
date: 2026-07-28
---
# Current architecture and cleanup evidence

Two historical recon reports point in different directions. The dead-code report
describes no unreachable code and justified feature-gated suppressions. The
source-tree report identifies large modules and concern concentration. Both were
produced against an older repository shape and reference files that have since
moved or been split.

The current material architecture risk is not proven dead code. It is semantic
duplication: `memory_core::Pipeline` and the newer `substrate` orchestration
coexist, while optional LLM intelligence could be wired through either. Deleting
code before tracing current callers, feature flags, tests, and benchmark paths
would be unsafe. Cleanup must begin with a current dependency/call-path audit and
an explicit production-composition decision.
