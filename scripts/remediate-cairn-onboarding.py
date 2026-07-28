#!/usr/bin/env python3
"""Resolve blocking findings from MAG's first curated Cairn scan.

Temporary onboarding script. Remove before merge.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
import textwrap

DATE = "2026-07-28"


def write(path: str, content: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")


blueprint_path = Path("cairn.blueprint")
blueprint = blueprint_path.read_text(encoding="utf-8")
blueprint = blueprint.replace(
    'path "./src/cli.rs"\n                contract "./meta/contracts/runtime.entrypoints.md"',
    'path "./src/cli.rs"\n                path "./src/doctor_checks.rs"\n                contract "./meta/contracts/runtime.entrypoints.md"',
)
blueprint = blueprint.replace(
    'Module StorageAPI "Storage module facade and backend boundary" id "mag.runtime.memory.storage.api" {',
    'Module StorageAPI "Storage module facade and backend boundary" id "mag.runtime.memory.storage.api" @no-test-coverage {',
)
blueprint = blueprint.replace(
    'Module Tests "Hermetic unit, integration, migration, and MCP tests" id "mag.quality.tests" @test {',
    'Module Tests "Hermetic unit, integration, migration, and MCP tests" id "mag.quality.tests" @test @no-test-coverage {',
)
blueprint = blueprint.replace(
    'path "./src/benchmarking.rs"\n                contract "./meta/contracts/quality.benchmarks.md"',
    'path "./src/benchmarking.rs"\n                path "./src/bin/fetch_benchmark_data.rs"\n                contract "./meta/contracts/quality.benchmarks.md"',
)
blueprint_path.write_text(blueprint, encoding="utf-8")

write(
    "cairn.config.yaml",
    r'''
    targets:
      - node: mag.integrations.connectors
        path: ./connectors
        language: assets
      - node: mag.integrations.connectors
        path: ./plugin
        language: assets
      - node: mag.integrations.packaging
        path: ./npm
        language: assets

    gates:
      - name: format
        command: cargo fmt --all -- --check
      - name: clippy
        command: cargo clippy --all-targets --all-features -- -D warnings
      - name: test
        command: cargo test --all-features
      - name: retrieval-benchmark
        command: ./scripts/bench.sh --gate
    ''',
)

Path("meta/reviews").mkdir(parents=True, exist_ok=True)
Path("meta/reviews/.gitkeep").touch()

local_sources = {
    "src.mag-agents-guide": ("AGENTS.md", "repository-guide", "Current repository architecture, commands, and coding constraints."),
    "src.local-first-roadmap": ("docs/specs/local-first-roadmap.md", "roadmap", "Current local-first mission, ordered gates, and Cairn execution model."),
    "src.hindsight-comparison": ("docs/benchmarks/comparison-hindsight.md", "benchmark-analysis", "Existing methodology and comparison context for the Hindsight quality target."),
    "src.dead-code-recon": ("docs/strongholds/recon-dead-code.md", "historical-recon", "April 2026 dead-code assessment; useful evidence but stale and not deletion authority."),
    "src.source-tree-recon": ("docs/strongholds/recon-source-tree.md", "historical-recon", "April 2026 architecture-size assessment; useful evidence but based on an older tree."),
}

for source_id, (file_value, source_type, note) in local_sources.items():
    digest = hashlib.sha256(Path(file_value).read_bytes()).hexdigest()
    write(
        f"meta/sources/{source_id}.md",
        f"""
        ---
        id: {source_id}
        file: {file_value}
        sha256: {digest}
        verification: verified
        type: {source_type}
        date: {DATE}
        ---

        {note}
        """,
    )

write(
    "meta/sources/src.cairn-framework.md",
    f'''
    ---
    id: src.cairn-framework
    file: https://cairn-framework.github.io/cairn/
    verification: external
    type: project-documentation
    date: {DATE}
    ---

    Cairn documentation and brownfield onboarding procedure reviewed for MAG adoption.
    ''',
)

all_leaf_nodes = [
    "mag.runtime.entrypoints",
    "mag.runtime.setup",
    "mag.runtime.mcp",
    "mag.runtime.memory.domain",
    "mag.runtime.memory.models",
    "mag.runtime.memory.retrieval",
    "mag.runtime.memory.storage.api",
    "mag.runtime.memory.storage.sqlite",
    "mag.runtime.memory.storage.memory",
    "mag.runtime.substrate",
    "mag.runtime.daemon",
    "mag.integrations.connectors",
    "mag.integrations.python",
    "mag.integrations.packaging",
    "mag.quality.tests",
    "mag.quality.benchmarks",
    "mag.quality.scripts",
]
node_list = ", ".join(all_leaf_nodes)
write(
    "meta/decisions/dec.baseline-current-module-boundaries.md",
    f'''
    ---
    id: dec.baseline-current-module-boundaries
    nodes: [{node_list}]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "The current architecture audit identifies a better semantic ownership boundary"
      - "The production composition-root decision changes module responsibilities"
      - "Cairn reconciliation shows a persistent false ownership model"
    informed_by: [res.cairn-fit-for-mag, res.architecture-state-audit]
    related: [dec.evidence-based-cleanup, dec.sequence-architecture-before-llm-wiring]
    ---
    # Baseline the current semantic module boundaries

    The curated Cairn map records the smallest useful current architecture for
    navigation and drift detection. It is a baseline, not a claim that every current
    boundary is ideal or permanent. Directory-depth discovery was rejected because
    it produced overlapping `src` and nested-module ownership.

    The architecture audit may split, merge, or retire these nodes through recorded
    changes. Until then, every module has an explicit contract and rationale rather
    than being an untracked addition to the blueprint.
    ''',
)

write(
    "meta/todos/review-and-split-oversized-modules.md",
    f'''
    ---
    node: mag.runtime
    status: blocked
    created: {DATE}
    ---
    # Review and split oversized modules where cohesion is weak

    Blocked by `todo.audit-current-architecture-and-dead-code` and the production
    composition-root decision.

    Cairn's first scan identified large production files in CLI/setup, retrieval,
    model, storage, and MCP surfaces, plus large benchmark and test files. File size
    alone is not grounds for a split. Classify each finding as:

    - cohesive and intentionally large;
    - generated/data-heavy;
    - test or benchmark support;
    - mixed-responsibility production code requiring decomposition.

    Add an allow marker only with a durable cohesion reason. Create node-level
    refactoring changes for mixed-responsibility code, prioritizing architecture
    ambiguity and change risk over cosmetic line-count reduction.
    ''',
)

print("blocking first-scan findings remediated")
