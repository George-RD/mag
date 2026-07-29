from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


def insert_after_heading(path: str, warning: str, marker: str) -> None:
    file_path = Path(path)
    text = file_path.read_text()
    if marker in text:
        return
    heading, remainder = text.split("\n", 1)
    file_path.write_text(f"{heading}\n\n{warning}\n\n{remainder.lstrip(chr(10))}")


historical_warnings = {
    "docs/strongholds/recon-dead-code.md": (
        "> **Historical snapshot — 2026-04-14.** This reconnaissance describes the repository at that date; it is not current dead-code or architecture authority.\n"
        "> Re-verify every count and conclusion against the code and use Cairn for live boundaries, decisions, and work status.",
        "Historical snapshot — 2026-04-14",
    ),
    "docs/strongholds/tech-debt-recon.md": (
        "> **Historical snapshot — 2026-04-08.** This report records a completed review, not the repository's current technical-debt queue.\n"
        "> Query live Cairn todos and re-check the code before acting on its recommendations or completion claims.",
        "Historical snapshot — 2026-04-08",
    ),
    "docs/strongholds/recon-source-tree.md": (
        "> **Historical snapshot — 2026-04-14.** File counts, module sizes, and ownership claims below are dated evidence.\n"
        "> Use `cairn bundle <node>` and the current source tree for implementation decisions.",
        "Historical snapshot — 2026-04-14",
    ),
    "docs/strongholds/substrate-campaign.md": (
        "> **Historical campaign record.** The merged substrate work remains useful design evidence, but it is not the current production roadmap.\n"
        "> `dec.select-local-runtime-composition-root` rejects wholesale substrate promotion and retains it only during bounded migration and retirement.",
        "Historical campaign record",
    ),
    "docs/strongholds/palantir-r3-final.md": (
        "> **Historical adversarial review — 2026-04-14.** This verdict evaluated the old execution roadmap at that revision.\n"
        "> It does not override accepted Cairn decisions, the local-first roadmap, or current code evidence.",
        "Historical adversarial review — 2026-04-14",
    ),
    "docs/strongholds/mag-improvement-plan.md": (
        "> **Historical plan — 2026-03-31.** Metrics, tool counts, priorities, and next actions below are a dated planning snapshot.\n"
        "> Use the local-first roadmap for sequencing and Cairn todos for current status.",
        "Historical plan — 2026-03-31",
    ),
}
for path, (warning, marker) in historical_warnings.items():
    insert_after_heading(path, warning, marker)

execution_path = Path("docs/specs/execution-roadmap.md")
execution = execution_path.read_text()
execution = replace_once(
    execution,
    '''# Execution Roadmap
<!-- Generated: 2026-04-14 | Baseline: v0.1.9-dev | Horizon: v0.2.2 -->

This document is the single source of truth for planned structural improvements to MAG. It covers four phases spanning v0.1.9 through v0.2.2, 15 pull requests, their dependencies, quality gates, risk mitigations, and parked items.

> **Current priority overlay:** [`local-first-roadmap.md`](local-first-roadmap.md)
> governs model-runtime, retrieval-quality, and memory-intelligence sequencing.
> Where the two documents conflict, the local-first roadmap takes precedence.
''',
    '''# Execution Roadmap
<!-- Generated: 2026-04-14 | Baseline: v0.1.9-dev | Horizon: v0.2.2 -->

> **Historical structural campaign.** This document preserves the v0.1.9-v0.2.2
> plan and its quality gates; it is not live work status or authority for future
> substrate promotion. Current sequencing lives in
> [`local-first-roadmap.md`](local-first-roadmap.md), accepted Cairn decisions bind
> the nodes they name, and `cairn status`/`cairn next` expose current work.

The completed campaign remains useful implementation history. Re-verify any
uncompleted-looking item against the code and live Cairn todos before acting.
''',
    "execution roadmap authority header",
)
execution = replace_once(
    execution,
    '''## Relationship to trait-surface.md

This roadmap (Phases 1-4) delivers **2 of the 7 substrate traits** defined in `trait-surface.md` — `ScoringStrategy` (PR-2a/2d) and `Reranker` (PR-2b) — plus structural decomposition of the four largest files and one reference backend (`MemoryStorage`). PR-4a-i adds a third trait (`RetrievalStrategy`), aligned with `trait-surface.md` §3.2's signature.

The full `substrate/` module — the remaining 5 traits (`FusionStrategy`, `Scorer`, `LifecyclePolicy`, `ConsolidationStrategy`, `IngestionPipeline`), the `SearchPipeline` and `WritePipeline` orchestrators, `MemoryStore` supertrait, blanket impls for backward compatibility, and the 5-phase deprecation path (Define, Implement, Wire, Deprecate, Remove) — is the **v0.3.x campaign**. `trait-surface.md` is the design reference for that campaign. It is not superseded by this roadmap.

Implementation agents working on v0.2.x should treat this roadmap as authoritative. When this roadmap and `trait-surface.md` conflict on trait signatures or module layout, this roadmap governs for Phases 1-4. For work beyond v0.2.2, `trait-surface.md` governs.
''',
    '''## Relationship to trait-surface.md

The scoring, reranking, and retrieval boundaries extracted into the live
`memory_core` path remain useful where they reduce coupling without changing
behaviour. `trait-surface.md` is historical design input, not an automatic v0.3.x
implementation campaign.

`dec.select-local-runtime-composition-root` rejects wholesale promotion of the
feature-gated substrate. MAG will introduce one entrypoint-owned local runtime
over the current SQLite-backed implementation and fold in only narrow, proven
boundaries through parity- and benchmark-gated migration slices. The broad
substrate `MemoryStore`, duplicate query context, and candidate orchestrators do
not become production contracts by default.

Later work is selected from the local-first roadmap and live Cairn todos rather
than inferred from this completed campaign.
''',
    "execution roadmap substrate relationship",
)
execution_path.write_text(execution)

trait_path = Path("docs/specs/trait-surface.md")
trait = trait_path.read_text()
trait = replace_once(
    trait,
    "**Status:** Draft  ",
    "**Status:** Historical design input — not an approved implementation campaign  ",
    "trait-surface status",
)
trait = replace_once(
    trait,
    '''**Scope:** 7 swap-point traits + supporting types, relationship mapping, composition patterns, reference impls, and deprecation path.

---
''',
    '''**Scope:** 7 swap-point traits + supporting types, relationship mapping, composition patterns, reference impls, and deprecation path.

> `dec.select-local-runtime-composition-root` rejects wholesale promotion of the
> current substrate. This document remains useful as design research for narrow
> interfaces, but it does not authorize a second production path. Follow the
> local-first roadmap, accepted Cairn decisions, and live migration todos.

---
''',
    "trait-surface authority notice",
)
trait_path.write_text(trait)

config_path = Path("docs/configuration.md")
config = config_path.read_text()
config = replace_once(
    config,
    '''## Optional Local Generative LLM

MAG's optional `llm` feature currently talks to OpenAI-compatible HTTP endpoints.
The local-first default profile is **LFM2.5 1.2B Instruct**, exposed through a
local runtime such as Ollama:

```bash
ollama pull LiquidAI/lfm2.5-1.2b-instruct
export MAG_LLM_PROVIDER=ollama
# Optional overrides:
# export MAG_LLM_MODEL=LiquidAI/lfm2.5-1.2b-instruct
# export MAG_LLM_BASE_URL=http://localhost:11434/v1
```

When `MAG_LLM_PROVIDER=ollama` is set and `MAG_LLM_MODEL` is omitted, MAG uses
`LiquidAI/lfm2.5-1.2b-instruct`. Cloud and self-hosted OpenAI-compatible endpoints
remain supported through the same `LlmBackend` boundary.

This is a transport-level default, not yet an in-process causal-model runtime.
The target direct model is `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`. The smaller
`LiquidAI/LFM2.5-350M-ONNX` is reserved for later task-by-task optimization after
the 1.2B quality baseline is established.
''',
    '''## Experimental Local Generative LLM Backend

The optional `llm` Cargo feature compiles backend clients and can parse
`MAG_LLM_*` configuration. Current production CLI and MCP composition does not
construct or call that backend. Setting these variables therefore does **not**
change stored memories, extraction, reflection, retrieval, or answer generation
in the current runtime.

For isolated backend experiments, the local reference profile is **LFM2.5 1.2B
Instruct** through an OpenAI-compatible runtime such as Ollama:

```bash
ollama pull LiquidAI/lfm2.5-1.2b-instruct
export MAG_LLM_PROVIDER=ollama
# Optional overrides consumed only by code that explicitly loads LlmConfig:
# export MAG_LLM_MODEL=LiquidAI/lfm2.5-1.2b-instruct
# export MAG_LLM_BASE_URL=http://localhost:11434/v1
```

When experiment code calls `LlmConfig::from_env`, Ollama without an explicit model
uses `LiquidAI/lfm2.5-1.2b-instruct`. OpenAI, Anthropic, and self-hosted client
implementations exist behind `LlmBackend`; their presence is not production
wiring.

Production integration is gated by the selected local runtime and evaluation
harness in the local-first roadmap. Direct in-process generation remains a
candidate using `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`; the 350M checkpoint remains
a later task-by-task speed candidate after 1.2B quality parity is established.
''',
    "configuration LLM section",
)
config_path.write_text(config)

skill_path = Path("skills/mag-development/SKILL.md")
skill = skill_path.read_text()
skill = replace_once(
    skill,
    '''## Local generative model

The current optional `llm` module uses an OpenAI-compatible HTTP transport. The
local-first default profile is:

- model: `LiquidAI/lfm2.5-1.2b-instruct`
- endpoint: `http://localhost:11434/v1`
- direct ONNX target: `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`

Example:

```bash
ollama pull LiquidAI/lfm2.5-1.2b-instruct
export MAG_LLM_PROVIDER=ollama
```

Do not describe the current generative path as in-process ONNX. MAG's embeddings
already use ONNX; causal generation still crosses the `LlmBackend` HTTP boundary.
Direct ONNX generation is a roadmap item.
''',
    '''## Experimental local generative backend

The optional `llm` module contains HTTP backend clients and configuration, but no
production CLI or MCP caller currently constructs it. `MAG_LLM_*` variables only
affect experiments or future code that explicitly loads `LlmConfig`; exporting
them does not activate generative memory behaviour today.

The local reference profile for bounded backend experiments is:

- model: `LiquidAI/lfm2.5-1.2b-instruct`
- endpoint: `http://localhost:11434/v1`
- direct ONNX candidate: `LiquidAI/LFM2.5-1.2B-Instruct-ONNX`

```bash
ollama pull LiquidAI/lfm2.5-1.2b-instruct
export MAG_LLM_PROVIDER=ollama
```

Do not describe this as production wiring or in-process ONNX. MAG's embeddings
already use ONNX; experimental causal generation crosses the `LlmBackend` HTTP
boundary. Production wiring follows the selected local runtime and evaluation
harness; direct ONNX generation remains a roadmap candidate.
''',
    "development skill LLM section",
)
skill_path.write_text(skill)

Path("scripts/apply_planning_authority_cleanup.py").unlink()
Path(".github/workflows/apply-planning-authority-cleanup.yml").unlink()
