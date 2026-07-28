---
id: res.local-first-sequencing
nodes: [mag.runtime.memory, mag.runtime.substrate, mag.quality.benchmarks]
sources: [src.local-first-roadmap, src.mag-agents-guide, src.hindsight-comparison]
date: 2026-07-28
---
# Corrected local-first sequencing

The earlier roadmap correctly identified the competing composition roots as a
blocker, but its numbered next-PR list placed production LLM wiring before that
decision. That would either duplicate behavior or make the evaluation harness
target a temporary path.

The dependency-respecting sequence is:

1. audit the current architecture and identify live/legacy paths;
2. choose and document the production composition root;
3. build the local intelligence evaluation harness against that boundary;
4. wire LFM2.5 1.2B into production with observable fallback;
5. add intelligence and optimize models only behind measured gates.
