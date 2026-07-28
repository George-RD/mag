#!/usr/bin/env python3
"""Replace Cairn's mechanical brownfield draft with MAG's reviewed architecture.

Temporary onboarding script. It is executed by the branch workflow and removed
before the pull request is merged.
"""

from __future__ import annotations

from pathlib import Path
import shutil
import textwrap

DATE = "2026-07-28"


def write(path: str, content: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")


write(
    "cairn.blueprint",
    r'''
    # MAG — reviewed architecture map.
    #
    # Status and work live in node-linked Cairn artefacts. The generated map is a
    # view; this blueprint and its artefacts are the durable source in Git.

    System MAG "Local-first durable memory for AI agents" id "mag" @memory @local-first {
        decisions "./meta/decisions"
        research "./meta/research"
        sources "./meta/sources"
        todos "./meta/todos"
        reviews "./meta/reviews"

        Container Runtime "Production Rust runtime and memory semantics" id "mag.runtime" @rust {
            Module Entrypoints "CLI, crate surface, and process orchestration" id "mag.runtime.entrypoints" {
                path "./src/main.rs"
                path "./src/lib.rs"
                path "./src/cli.rs"
                contract "./meta/contracts/runtime.entrypoints.md"
            }

            Module Setup "Tool detection, setup, configuration, paths, and uninstall" id "mag.runtime.setup" {
                path "./src/setup.rs"
                path "./src/uninstall.rs"
                path "./src/tool_detection.rs"
                path "./src/config_writer.rs"
                path "./src/app_paths.rs"
                contract "./meta/contracts/runtime.setup.md"
            }

            Module MCP "Validated MCP stdio tools over memory capabilities" id "mag.runtime.mcp" @mcp {
                path "./src/mcp"
                contract "./meta/contracts/runtime.mcp.md"
            }

            Container Memory "Memory domain, model roles, retrieval, and storage" id "mag.runtime.memory" {
                Module Domain "Stable memory types, traits, and legacy pipeline surface" id "mag.runtime.memory.domain" {
                    path "./src/memory_core/mod.rs"
                    path "./src/memory_core/domain.rs"
                    path "./src/memory_core/traits.rs"
                    contract "./meta/contracts/memory.domain.md"
                }

                Module Models "Embedding, generation, and local/remote model adapters" id "mag.runtime.memory.models" @local-inference {
                    path "./src/memory_core/embedder.rs"
                    path "./src/memory_core/llm.rs"
                    contract "./meta/contracts/memory.models.md"
                }

                Module Retrieval "Candidate retrieval, reranking, scoring, and abstention" id "mag.runtime.memory.retrieval" @benchmark-gated {
                    path "./src/memory_core/reranker.rs"
                    path "./src/memory_core/retrieval_strategy.rs"
                    path "./src/memory_core/scoring.rs"
                    path "./src/memory_core/scoring_strategy.rs"
                    contract "./meta/contracts/memory.retrieval.md"
                }

                Container Storage "Durable and reference memory backends" id "mag.runtime.memory.storage" {
                    Module StorageAPI "Storage module facade and backend boundary" id "mag.runtime.memory.storage.api" {
                        path "./src/memory_core/storage/mod.rs"
                        contract "./meta/contracts/storage.api.md"
                    }

                    Module SQLite "Production SQLite, FTS, graph, lifecycle, and query pipeline" id "mag.runtime.memory.storage.sqlite" @source-of-truth {
                        path "./src/memory_core/storage/sqlite"
                        contract "./meta/contracts/storage.sqlite.md"
                    }

                    Module InMemory "Reference in-memory backend used for parity and tests" id "mag.runtime.memory.storage.memory" {
                        path "./src/memory_core/storage/memory"
                        contract "./meta/contracts/storage.memory.md"
                    }
                }
            }

            Module Substrate "Newer trait-composed orchestration under evaluation" id "mag.runtime.substrate" @architecture-decision-pending {
                path "./src/substrate"
                contract "./meta/contracts/runtime.substrate.md"
            }

            Module Daemon "Optional authenticated HTTP deployment adapter" id "mag.runtime.daemon" @optional {
                path "./src/daemon.rs"
                path "./src/auth.rs"
                path "./src/idle_timer.rs"
                contract "./meta/contracts/runtime.daemon.md"
            }
        }

        Container Integrations "Agent connectors, Python tooling, and package wrappers" id "mag.integrations" {
            Module Connectors "Agent-facing setup content and plugin hooks" id "mag.integrations.connectors" {
                path "./connectors"
                path "./plugin"
                contract "./meta/contracts/integrations.connectors.md"
            }

            Module Python "Python client and active-strategy research harnesses" id "mag.integrations.python" {
                path "./python"
                contract "./meta/contracts/integrations.python.md"
            }

            Module Packaging "npm distribution wrapper and release metadata" id "mag.integrations.packaging" {
                path "./npm"
                contract "./meta/contracts/integrations.packaging.md"
            }
        }

        Container Quality "Tests, benchmarks, and repeatable engineering gates" id "mag.quality" {
            Module Tests "Hermetic unit, integration, migration, and MCP tests" id "mag.quality.tests" @test {
                path "./tests"
                path "./src/test_helpers.rs"
                contract "./meta/contracts/quality.tests.md"
            }

            Module Benchmarks "LongMemEval, LoCoMo, scale, and model evaluation" id "mag.quality.benchmarks" @evaluation {
                path "./benches"
                path "./src/benchmarking.rs"
                contract "./meta/contracts/quality.benchmarks.md"
            }

            Module Scripts "Repository automation and benchmark entry points" id "mag.quality.scripts" {
                path "./scripts"
                contract "./meta/contracts/quality.scripts.md"
            }
        }
    }

    # Runtime assembly and public surfaces
    mag.runtime.entrypoints -> mag.runtime.setup "Dispatches setup, configuration, and uninstall commands"
    mag.runtime.entrypoints -> mag.runtime.mcp "Starts and hosts the MCP stdio server"
    mag.runtime.entrypoints -> mag.runtime.memory.domain "Builds the public memory pipeline"
    mag.runtime.entrypoints -> mag.runtime.memory.models "Constructs embedding and optional LLM backends"
    mag.runtime.entrypoints -> mag.runtime.memory.storage.sqlite "Constructs the production storage backend"
    mag.runtime.entrypoints -> mag.runtime.daemon "Starts the optional HTTP adapter"

    # Memory internals
    mag.runtime.mcp -> mag.runtime.memory.domain "Exposes domain operations as validated tools"
    mag.runtime.mcp -> mag.runtime.memory.storage.sqlite "Delegates tool execution to SQLite storage"
    mag.runtime.memory.models -> mag.runtime.memory.domain "Implements model traits used by memory operations"
    mag.runtime.memory.retrieval -> mag.runtime.memory.domain "Scores and returns domain search results"
    mag.runtime.memory.retrieval -> mag.runtime.memory.models "Uses embeddings and optional rerankers"
    mag.runtime.memory.storage.api -> mag.runtime.memory.domain "Defines backend access around domain contracts"
    mag.runtime.memory.storage.sqlite -> mag.runtime.memory.domain "Persists and queries domain records"
    mag.runtime.memory.storage.sqlite -> mag.runtime.memory.models "Creates and consumes embeddings"
    mag.runtime.memory.storage.sqlite -> mag.runtime.memory.retrieval "Runs the advanced retrieval pipeline"
    mag.runtime.memory.storage.memory -> mag.runtime.memory.domain "Implements the reference backend contract"

    # Competing orchestration path to be resolved before more production wiring
    mag.runtime.substrate -> mag.runtime.memory.domain "Composes stable memory traits"
    mag.runtime.substrate -> mag.runtime.memory.models "Uses model-role abstractions"
    mag.runtime.substrate -> mag.runtime.memory.retrieval "Composes retrieval and scoring strategies"
    mag.runtime.substrate -> mag.runtime.memory.storage.sqlite "Bridges to the production backend"

    # Deployment and integrations
    mag.runtime.daemon -> mag.runtime.mcp "Serves the same MCP semantics over HTTP"
    mag.runtime.setup -> mag.integrations.connectors "Installs and removes connector content"
    mag.integrations.python -> mag.runtime.mcp "Uses MAG through its protocol surface"

    # Verification
    mag.quality.tests -> mag.runtime "Verifies runtime behavior and contracts"
    mag.quality.benchmarks -> mag.runtime.memory "Measures memory quality and latency"
    mag.quality.benchmarks -> mag.runtime.substrate "Compares candidate orchestration paths"
    mag.quality.scripts -> mag.quality.tests "Runs repository quality gates"
    mag.quality.scripts -> mag.quality.benchmarks "Runs benchmark and regression gates"
    ''',
)

write(
    "cairn.config.yaml",
    r'''
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

contracts = {
    "runtime.entrypoints": """
        The entrypoint layer owns process startup, CLI dispatch, and assembly of
        concrete components. It must not contain independent storage, retrieval,
        extraction, or connector semantics. MCP mode keeps stdout exclusively for
        protocol traffic and sends diagnostics to stderr.
    """,
    "runtime.setup": """
        Setup owns tool detection, generated configuration, data-path resolution,
        installation, and uninstall symmetry. Operations must be idempotent, must
        not silently overwrite user-managed content, and must leave an explicit
        recovery path when setup is partial.
    """,
    "runtime.mcp": """
        MCP handlers validate protocol inputs and delegate to domain/backend
        capabilities. They do not reimplement memory semantics. Tool schemas and
        errors remain stable, bounded, and safe for stdio transport.
    """,
    "memory.domain": """
        Domain types and traits are the stable semantic contract of MAG. They may
        not depend on a concrete storage engine, transport, or hosted provider.
        Legacy and replacement orchestration surfaces must not diverge in behavior.
    """,
    "memory.models": """
        Embedding, generation, and reranking are separate model roles behind
        explicit interfaces. The core quality baseline must run locally without
        cloud credentials. Remote and self-hosted adapters remain optional and use
        the same memory semantics. Missing models fail visibly and fall back only
        when the caller explicitly permits it.
    """,
    "memory.retrieval": """
        Retrieval combines independently useful channels, preserves explainability,
        abstains when evidence is weak, and is benchmark-gated. Fixed thresholds are
        provisional; changes report recall, precision, abstention, latency, memory,
        and context-budget effects.
    """,
    "storage.api": """
        The storage facade exposes backend-neutral memory operations and keeps
        concrete implementation details below the boundary. Backends are tested
        against shared behavioral contracts.
    """,
    "storage.sqlite": """
        SQLite is the durable local source of truth. Database work runs off the
        async executor, writes are transactional, migrations are additive and
        idempotent, FTS/vector/graph indexes are repairable, and caches never become
        durability authorities.
    """,
    "storage.memory": """
        The in-memory backend is a small reference implementation for conformance
        and tests. It must make intentional parity gaps explicit rather than
        silently approximating production behavior.
    """,
    "runtime.substrate": """
        Substrate is a candidate composition root, not a second product. Until the
        composition decision is accepted, new production intelligence must not be
        implemented independently in both substrate and the legacy pipeline.
    """,
    "runtime.daemon": """
        The daemon is an optional authenticated deployment adapter. It reuses the
        same domain behavior as local CLI/MCP mode, keeps auth and lifecycle at the
        boundary, and must never become required for local operation.
    """,
    "integrations.connectors": """
        Connectors translate MAG's stable capabilities into agent-specific files
        and lifecycle hooks. They do not own separate memory semantics. Installation
        and removal are idempotent and preserve user edits.
    """,
    "integrations.python": """
        Python code supports protocol clients, active-injection research, and
        evaluation. It is not a hidden production implementation or an authority
        for semantics that differ from the Rust runtime.
    """,
    "integrations.packaging": """
        Package wrappers are thin distribution surfaces around the Rust binary.
        Versions remain consistent and wrappers do not alter runtime behavior.
    """,
    "quality.tests": """
        Tests are hermetic, deterministic where possible, and isolated from user
        data through temporary HOME, USERPROFILE, and MAG_DATA_ROOT values. Product
        tests cover real CLI/MCP behavior, not only internal units.
    """,
    "quality.benchmarks": """
        Benchmarks use versioned datasets, disclose methodology, and distinguish
        retrieval from end-to-end answer quality. Local-first scorecards include
        quality, latency, RAM, disk size, cold start, and offline success.
    """,
    "quality.scripts": """
        Scripts provide repeatable developer and CI entry points. They fail loudly,
        avoid machine-specific assumptions, and keep benchmark-gate coverage aligned
        with every retrieval/scoring path.
    """,
}

for slug, body in contracts.items():
    node = {
        "runtime.entrypoints": "mag.runtime.entrypoints",
        "runtime.setup": "mag.runtime.setup",
        "runtime.mcp": "mag.runtime.mcp",
        "memory.domain": "mag.runtime.memory.domain",
        "memory.models": "mag.runtime.memory.models",
        "memory.retrieval": "mag.runtime.memory.retrieval",
        "storage.api": "mag.runtime.memory.storage.api",
        "storage.sqlite": "mag.runtime.memory.storage.sqlite",
        "storage.memory": "mag.runtime.memory.storage.memory",
        "runtime.substrate": "mag.runtime.substrate",
        "runtime.daemon": "mag.runtime.daemon",
        "integrations.connectors": "mag.integrations.connectors",
        "integrations.python": "mag.integrations.python",
        "integrations.packaging": "mag.integrations.packaging",
        "quality.tests": "mag.quality.tests",
        "quality.benchmarks": "mag.quality.benchmarks",
        "quality.scripts": "mag.quality.scripts",
    }[slug]
    write(
        f"meta/contracts/{slug}.md",
        f"""
        ---
        node: {node}
        ---
        # {node} contract

        {textwrap.dedent(body).strip()}
        """,
    )

sources = {
    "src.cairn-framework": (
        "https://cairn-framework.github.io/cairn/",
        "project-documentation",
        "Cairn documentation and brownfield onboarding procedure reviewed for MAG adoption.",
    ),
    "src.mag-agents-guide": (
        "AGENTS.md",
        "repository-guide",
        "Current repository architecture, commands, and coding constraints.",
    ),
    "src.local-first-roadmap": (
        "docs/specs/local-first-roadmap.md",
        "roadmap",
        "Original local-first direction and model/retrieval sequence before Cairn onboarding.",
    ),
    "src.hindsight-comparison": (
        "docs/benchmarks/comparison-hindsight.md",
        "benchmark-analysis",
        "Existing methodology and comparison context for the Hindsight quality target.",
    ),
    "src.dead-code-recon": (
        "docs/strongholds/recon-dead-code.md",
        "historical-recon",
        "April 2026 dead-code assessment; useful evidence but stale and not deletion authority.",
    ),
    "src.source-tree-recon": (
        "docs/strongholds/recon-source-tree.md",
        "historical-recon",
        "April 2026 architecture-size assessment; useful evidence but based on an older tree.",
    ),
}

for source_id, (file_value, source_type, note) in sources.items():
    write(
        f"meta/sources/{source_id}.md",
        f"""
        ---
        id: {source_id}
        file: {file_value}
        verification: verified
        type: {source_type}
        date: {DATE}
        ---

        {note}
        """,
    )

write(
    "meta/research/res.cairn-fit-for-mag.md",
    f'''
    ---
    id: res.cairn-fit-for-mag
    nodes: [mag]
    sources: [src.cairn-framework, src.mag-agents-guide, src.local-first-roadmap]
    date: {DATE}
    ---
    # Cairn fit assessment for MAG

    MAG had an ordered prose roadmap but no small, authoritative status records.
    Decisions and research were spread across large roadmap, stronghold, conductor,
    and benchmark documents. A single append-only ADR JSONL would improve structure
    but would still become a large merge-conflict and retrieval surface.

    Cairn directly matches the requirement: stable architecture node IDs; separate
    decision, research, source, contract, and todo files; typed links between them;
    and bounded JSON queries such as `cairn brief`, `cairn rationale`, `cairn todos`,
    and `cairn context --scope`.

    The automatic brownfield draft was not accepted unchanged. It discovered useful
    directories but flattened overlapping paths such as `src`, `src/memory_core`,
    and `src/memory_core/storage/sqlite` into sibling modules. MAG therefore uses a
    reviewed blueprint based on semantic ownership rather than filesystem depth.
    ''',
)

write(
    "meta/research/res.architecture-state-audit.md",
    f'''
    ---
    id: res.architecture-state-audit
    nodes: [mag.runtime, mag.runtime.memory, mag.runtime.substrate]
    sources: [src.mag-agents-guide, src.dead-code-recon, src.source-tree-recon]
    date: {DATE}
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
    ''',
)

write(
    "meta/research/res.local-first-sequencing.md",
    f'''
    ---
    id: res.local-first-sequencing
    nodes: [mag.runtime.memory, mag.runtime.substrate, mag.quality.benchmarks]
    sources: [src.local-first-roadmap, src.mag-agents-guide, src.hindsight-comparison]
    date: {DATE}
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
    ''',
)

write(
    "meta/decisions/dec.use-cairn-development-context.md",
    f'''
    ---
    id: dec.use-cairn-development-context
    nodes: [mag]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "Cairn maintenance repeatedly costs more than the context and drift it prevents"
      - "Cairn cannot represent a material MAG architecture or decision relationship"
      - "The map produces persistent false structural blockers after curation"
    informed_by: [res.cairn-fit-for-mag]
    related: [dec.sequence-architecture-before-llm-wiring]
    ---
    # Use Cairn for MAG development context

    MAG will use Cairn as the queryable architecture, decision, research, contract,
    and work-status layer. The narrative roadmap remains useful for mission, order,
    and exit gates, but `meta/todos/` is the authoritative execution status.

    Agents should query the smallest connected slice instead of loading all planning
    material. Typical entry points are `cairn next`, `cairn brief <todo>`,
    `cairn context --scope <node>`, and `cairn rationale <node>`.

    Alternatives considered were a conventional ADR directory plus a custom JSONL
    index, or continuing with prose roadmaps. The former duplicates functionality
    already present in Cairn; the latter does not scale to bounded queries.
    ''',
)

write(
    "meta/decisions/dec.local-first-dual-mode.md",
    f'''
    ---
    id: dec.local-first-dual-mode
    nodes: [mag, mag.runtime.memory.models, mag.runtime.daemon]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "Local and service deployments require incompatible memory semantics"
      - "A supported platform cannot run the minimum local quality baseline"
    informed_by: [res.local-first-sequencing]
    related: [dec.lfm25-1-2b-baseline]
    ---
    # Local-first core with optional service deployment

    Local and hosted operation are deployment modes of the same memory system, not
    separate products. Local mode must reach the core quality baseline without cloud
    credentials. Service mode may add cross-device access, teams, administration,
    and larger hardware, but it reuses the same model-role and memory contracts.
    ''',
)

write(
    "meta/decisions/dec.lfm25-1-2b-baseline.md",
    f'''
    ---
    id: dec.lfm25-1-2b-baseline
    nodes: [mag.runtime.memory.models, mag.quality.benchmarks]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "A smaller local model matches the task-level quality gates"
      - "LFM2.5 1.2B cannot meet ordinary-computer latency or memory budgets"
      - "A clearly better permissively deployable local model becomes available"
    informed_by: [res.local-first-sequencing]
    related: [dec.local-first-dual-mode, dec.preserve-derived-memory-provenance]
    ---
    # LFM2.5 1.2B is the local generative reference

    LFM2.5 1.2B Instruct is the reference model for local extraction, relationship
    reasoning, grouping, and consolidation evaluations. LFM2.5 350M is not an
    automatic fallback; individual tasks move only after measured parity within a
    predeclared tolerance.
    ''',
)

write(
    "meta/decisions/dec.sequence-architecture-before-llm-wiring.md",
    f'''
    ---
    id: dec.sequence-architecture-before-llm-wiring
    nodes: [mag.runtime.memory, mag.runtime.substrate, mag.quality.benchmarks]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "The two composition paths are proven to be the same production path"
      - "A minimal reversible spike is required to gather evidence for the decision"
    informed_by: [res.architecture-state-audit, res.local-first-sequencing]
    refines: [dec.local-first-dual-mode]
    related: [dec.use-cairn-development-context]
    ---
    # Resolve composition before production LLM wiring

    MAG will first audit and select the production composition root, then build the
    evaluation harness, then wire LFM2.5 1.2B into production. A short disposable
    spike is allowed for evidence, but permanent intelligence must not be duplicated
    across `memory_core::Pipeline` and `substrate`.
    ''',
)

write(
    "meta/decisions/dec.preserve-derived-memory-provenance.md",
    f'''
    ---
    id: dec.preserve-derived-memory-provenance
    nodes: [mag.runtime.memory.domain, mag.runtime.memory.storage.sqlite, mag.runtime.memory.models]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "A derived artefact cannot retain source lineage within practical storage limits"
      - "A stronger immutable event model replaces the current memory representation"
    informed_by: [res.local-first-sequencing]
    related: [dec.lfm25-1-2b-baseline]
    ---
    # Generated intelligence remains derived and traceable

    Model-generated facts, relationships, clusters, contradictions, and summaries
    never overwrite raw memories. Derived records retain source memory IDs, model
    profile, prompt/schema version, confidence, and creation time so they can be
    inspected, invalidated, and regenerated.
    ''',
)

write(
    "meta/decisions/dec.evidence-based-cleanup.md",
    f'''
    ---
    id: dec.evidence-based-cleanup
    nodes: [mag.runtime, mag.runtime.memory, mag.runtime.substrate]
    status: accepted
    date: {DATE}
    revisit_triggers:
      - "Current compiler and coverage evidence proves a simpler safe bulk-removal path"
    informed_by: [res.architecture-state-audit]
    related: [dec.sequence-architecture-before-llm-wiring]
    ---
    # Cleanup follows current evidence, not stale file-size reports

    Feature-gated, benchmark-only, connector, or fallback code is not dead merely
    because it is not on the default path. The cleanup pass must trace current
    callers and features, run all-feature tests, and preserve behavior before
    deletion or consolidation. Architectural duplication is prioritized over raw
    line-count reduction.
    ''',
)

todos = {
    "audit-current-architecture-and-dead-code": (
        "mag.runtime",
        "in_progress",
        """
        Build a current dependency and feature-usage picture. Confirm which legacy
        pipeline, substrate, daemon, connector, benchmark, and fallback paths are
        live. Identify removable code, semantic duplication, and stale documents.

        Acceptance: every proposed deletion or consolidation has current caller,
        feature, test, and benchmark evidence. Update the blueprint and contracts
        with the resulting boundaries.
        """,
    ),
    "select-production-composition-root": (
        "mag.runtime.substrate",
        "blocked",
        """
        Blocked by `todo.audit-current-architecture-and-dead-code`.

        Decide whether substrate becomes the production composition root or its
        useful trait boundaries are folded into the existing pipeline. Record an
        accepted decision, migration slices, compatibility period, and removal path.
        """,
    ),
    "build-local-memory-intelligence-eval-harness": (
        "mag.quality.benchmarks",
        "blocked",
        """
        Blocked by `todo.select-production-composition-root`.

        Create a small versioned local dataset and runner for facts, entities,
        temporal references, relationships, decisions, questions, status, grouping,
        contradictions, and provenance. Report schema validity, precision/recall,
        task success, p50/p95 latency, peak RAM, load time, and tokens.
        """,
    ),
    "wire-lfm25-production-ingestion": (
        "mag.runtime.memory.models",
        "blocked",
        """
        Blocked by the composition-root and evaluation-harness todos.

        Wire LFM2.5 1.2B into the chosen production write path behind explicit
        enable/disable configuration, observable health, bounded structured output,
        and safe rule-based fallback. No cloud credential may be required.
        """,
    ),
    "prototype-direct-onnx-lfm25": (
        "mag.runtime.memory.models",
        "blocked",
        """
        Blocked until the 1.2B HTTP-local production baseline is measurable.

        Prototype direct ONNX generation behind the same model-role boundary with a
        manifest, checksums, tokenizer/chat template, quantization, provider
        detection, cancellation, warmup, and KV-cache reuse. Keep it only if it wins
        the operational quality/latency/portability trade-off.
        """,
    ),
    "calibrate-retrieval-and-reranking": (
        "mag.runtime.memory.retrieval",
        "blocked",
        """
        Blocked by the local evaluation harness.

        Replace provisional global cutoffs with calibrated confidence from semantic
        score, margin, lexical agreement, reranker score, intent, and candidate
        diversity. Evaluate the existing cross-encoder and local embedding/reranker
        alternatives. Add dynamic result count and token budget.
        """,
    ),
    "add-provenance-preserving-memory-intelligence": (
        "mag.runtime.memory",
        "blocked",
        """
        Blocked by production LFM2.5 wiring and its evaluation gates.

        Add evaluated fact/decision extraction, entity normalization, relationship
        proposals, topic grouping, contradiction/supersession proposals, and
        consolidation summaries. All derived records follow
        `dec.preserve-derived-memory-provenance`.
        """,
    ),
    "qualify-lfm25-350m-task-routing": (
        "mag.runtime.memory.models",
        "blocked",
        """
        Blocked until the 1.2B baseline is stable per task.

        Run the same tasks through LFM2.5 350M. Promote only tasks within declared
        quality tolerances. Keep relationship reasoning and consolidation on 1.2B
        unless evidence supports moving them.
        """,
    ),
    "design-service-and-cross-device-mode": (
        "mag.runtime.daemon",
        "blocked",
        """
        Blocked until local model/runtime interfaces are stable.

        Reuse the same contracts behind hosted workers or HTTP. Add authentication,
        encryption, synchronization, tenancy, and conflict handling without making
        service infrastructure a dependency of local single-user operation.
        """,
    ),
    "rationalize-stale-planning-documents": (
        "mag",
        "blocked",
        """
        Blocked by the current architecture audit.

        Mark historical stronghold and roadmap documents as superseded, archival,
        or still authoritative. Move live decisions/research/todos into Cairn
        artefacts and leave concise pointers rather than competing sources of truth.
        """,
    ),
}

for slug, (node, status, body) in todos.items():
    write(
        f"meta/todos/{slug}.md",
        f"""
        ---
        node: {node}
        status: {status}
        created: {DATE}
        ---
        # {slug.replace('-', ' ').title()}

        {textwrap.dedent(body).strip()}
        """,
    )

write(
    "docs/specs/local-first-roadmap.md",
    r'''
    # MAG local-first development roadmap

    <!-- Revised: 2026-07-28 | Status is held in Cairn todo artefacts -->

    ## Mission

    Make MAG more useful than Hindsight for durable agent memory while remaining
    fully functional with local inference on an ordinary modern computer. Cloud and
    self-hosted deployments remain supported, but they are not prerequisites for the
    core quality baseline.

    Local and service operation are complementary deployment modes:

    - **Local mode** prioritizes privacy, offline use, low marginal cost, and simple
      single-user operation.
    - **Service mode** adds cross-device access, team memory, centralized
      administration, and larger hardware.
    - Both implement the same model-role, memory, provenance, and retrieval contracts.

    ## How this roadmap is executed

    This document records mission, sequence, trade-offs, and exit gates. It is not a
    second task tracker. Current status is stored in small node-linked files under
    `meta/todos/` and queried through Cairn:

    ```bash
    cairn status
    cairn next
    cairn todos
    cairn brief todo.audit-current-architecture-and-dead-code
    cairn context --scope mag.runtime.memory
    cairn rationale mag.runtime.memory
    ```

    Decisions and research are similarly bounded:

    ```bash
    cairn decisions mag.runtime.memory
    cairn research mag.runtime.memory
    cairn bundle mag.runtime.memory.models
    ```

    A todo changes state with `cairn todo set <slug> <status>`. Do not add checkboxes
    here; doing so would create two status sources.

    ## Decisions in force

    1. Cairn is the queryable development-context and work-status layer.
    2. LFM2.5 1.2B Instruct is the local generative quality reference.
    3. LFM2.5 350M is eligible only after task-level parity against 1.2B.
    4. Generation, embedding, and reranking are separate model roles.
    5. Current generation is HTTP-local; direct ONNX generation is a candidate
       runtime, not a preselected outcome.
    6. Derived facts, relationships, clusters, and summaries never overwrite raw
       memories and retain provenance.
    7. Cleanup is evidence-based; stale recon reports are inputs, not authority.

    ## Contradictions resolved

    ### Production wiring versus architecture boundary

    The previous roadmap identified the coexistence of `memory_core::Pipeline` and
    `substrate` as a blocker but listed production LLM wiring before resolving it.
    The corrected dependency order is architecture audit, composition decision,
    evaluation harness, then production wiring.

    ### Local-first versus hosted service

    There is no product contradiction. The local baseline is mandatory; hosted mode
    is an optional deployment and synchronization layer over the same semantics.

    ### “Clean codebase” versus “faulty architecture”

    Historical reports found little unreachable code while also identifying large,
    concentrated modules. Those statements can both be true. The current priority is
    live semantic duplication and composition ambiguity, not deleting code because a
    file is large or feature-gated.

    ## Ordered development plan

    ### P0 — Establish the architecture and evaluation foundation

    - Onboard and curate Cairn against the actual repository.
    - Audit current callers, feature flags, fallback paths, tests, and benchmarks.
    - Choose the production composition root and define the migration/removal path.
    - Separate model role, runtime, and transport in the chosen architecture.
    - Build the local memory-intelligence evaluation harness and versioned dataset.

    **Gate:** one documented production path, no unexplained duplicate semantics,
    bounded Cairn queries for connected context, and a reproducible local scorecard.

    ### P1 — Establish the local LFM2.5 1.2B production baseline

    - Wire `LlmBackend` into the chosen production ingestion/write path.
    - Require explicit enable/disable behavior and observable fallback.
    - Evaluate facts, entities, temporal references, relationships, decisions,
      questions, status, grouping, contradictions, and provenance.
    - Ensure a missing local model produces an actionable warning.

    **Gate:** the 1.2B profile works end to end without cloud credentials and improves
    memory usefulness over rule-only extraction within local latency/RAM budgets.

    ### P2 — Evaluate direct local inference

    - Prototype `LiquidAI/LFM2.5-1.2B-Instruct-ONNX` behind the model-runtime boundary.
    - Add manifest, revision, checksums, quantization, tokenizer/chat template,
      bounded structured output, cancellation, warmup, and KV-cache reuse.
    - Detect CPU/GPU/NPU execution providers and fall back predictably.
    - Compare direct ONNX with Ollama/llama.cpp transport for quality, startup,
      steady-state latency, RAM, disk size, and packaging complexity.

    **Decision rule:** direct ONNX becomes default only if it lowers end-to-end
    operational cost without a material quality or portability regression.

    ### P3 — Calibrate retrieval and reranking

    - Calibrate confidence from semantic score, score margin, lexical/semantic
      agreement, reranker score, query intent, and candidate diversity.
    - Benchmark the existing cross-encoder within session-start latency limits.
    - Evaluate local embedding/reranker alternatives against the current BGE path.
    - Use dynamic result count and token budget.
    - Measure active injection versus passive retrieval on agent-resumption tasks.

    **Gate:** useful recall and injection precision improve without exceeding local
    latency, RAM, and context budgets.

    ### P4 — Add local memory intelligence

    Use the 1.2B model for narrow, evaluated operations:

    - canonical fact and decision extraction;
    - entity normalization and relationship proposals;
    - memory clustering and topic grouping;
    - contradiction and supersession proposals;
    - consolidation summaries with provenance;
    - query decomposition only when retrieval evaluations justify it.

    **Gate:** derived structures improve downstream task success, retain full source
    lineage, and can be invalidated or regenerated independently of raw memories.

    ### P5 — Qualify the 350M speed tier

    Run the same task-level evaluations with LFM2.5 350M. Promote only tasks within a
    predeclared tolerance of the 1.2B reference. Classification and constrained
    tagging are likely candidates; relationship reasoning and consolidation remain
    on 1.2B unless evidence says otherwise.

    ### P6 — Add service and cross-device mode

    - Reuse the same model/runtime interfaces behind hosted workers or HTTP.
    - Add authentication, encryption, synchronization, tenancy, and conflict handling
      separately from memory-quality logic.
    - Preserve an entirely local single-binary mode with no service dependency.

    ## Baseline scorecard

    Every model, runtime, retrieval, or intelligence change reports:

    - extraction schema validity;
    - fact/entity/relationship precision and recall;
    - retrieval Recall@5/10, MRR, and abstention accuracy;
    - injected-memory precision and active-injection task-success delta;
    - p50/p95 cold and warm latency;
    - peak RAM, on-disk model size, and model load time;
    - generated tokens and context tokens injected;
    - offline success after model installation;
    - quality delta versus the LFM2.5 1.2B local reference.
    ''',
)

# The machine-generated proposal was useful as reconnaissance but is not the map.
for obsolete in [Path("meta/changes/brownfield-init"), Path(".cairn/bootstrap")]:
    if obsolete.exists():
        shutil.rmtree(obsolete)

print("curated Cairn architecture, artefacts, and roadmap written")
