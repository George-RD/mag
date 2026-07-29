from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected one match, found {count}")
    return text.replace(old, new, 1)


local_path = Path("docs/specs/local-first-roadmap.md")
local = local_path.read_text()
local = replace_once(
    local,
    '''10. Retriever and generative fine-tuning follow stable architecture and
    held-out evaluations. Evaluation runs begin collecting training evidence
    before training begins.
''',
    '''10. Retriever and generative fine-tuning follow stable architecture and
    held-out evaluations. Evaluation runs begin collecting training evidence
    before training begins.
11. The production composition root is one entrypoint-owned local runtime over
    the current SQLite-backed implementation. The feature-gated substrate is not
    promoted wholesale; only proven narrow boundaries move into the live path.
''',
    "local-first decision list",
)
local = replace_once(
    local,
    "- Choose the production composition root and define the migration/removal path.\n",
    "- Introduce the selected local runtime facade and execute its migration/removal path.\n",
    "P0 composition step",
)
local_path.write_text(local)

execution_path = Path("docs/specs/execution-roadmap.md")
execution = execution_path.read_text()
old_relationship = '''## Relationship to trait-surface.md

This roadmap (Phases 1-4) delivers **2 of the 7 substrate traits** defined in `trait-surface.md` — `ScoringStrategy` (PR-2a/2d) and `Reranker` (PR-2b) — plus structural decomposition of the four largest files and one reference backend (`MemoryStorage`). PR-4a-i adds a third trait (`RetrievalStrategy`), aligned with `trait-surface.md` §3.2's signature.

The full `substrate/` module — the remaining 5 traits (`FusionStrategy`, `Scorer`, `LifecyclePolicy`, `ConsolidationStrategy`, `IngestionPipeline`), the `SearchPipeline` and `WritePipeline` orchestrators, `MemoryStore` supertrait, blanket impls for backward compatibility, and the 5-phase deprecation path (Define, Implement, Wire, Deprecate, Remove) — is the **v0.3.x campaign**. `trait-surface.md` is the design reference for that campaign. It is not superseded by this roadmap.

Implementation agents working on v0.2.x should treat this roadmap as authoritative. When this roadmap and `trait-surface.md` conflict on trait signatures or module layout, this roadmap governs for Phases 1-4. For work beyond v0.2.2, `trait-surface.md` governs.
'''
new_relationship = '''## Relationship to trait-surface.md

The scoring, reranking, and retrieval boundaries already extracted into the live
`memory_core` path remain valid when they reduce coupling without changing
behaviour. `trait-surface.md` is now historical design input, not an automatic
v0.3.x implementation campaign.

`dec.select-local-runtime-composition-root` rejects wholesale promotion of the
current feature-gated substrate. MAG will introduce one entrypoint-owned local
runtime over the current SQLite-backed implementation and fold in only narrow,
proven boundaries through parity- and benchmark-gated migration slices. The broad
substrate `MemoryStore`, duplicate query context, and candidate orchestrators do
not become production contracts by default.

For v0.2.x, this roadmap remains authoritative only where it is consistent with
the local-first roadmap and accepted Cairn decisions. Later work is selected from
live Cairn todos rather than inferred from the historical substrate campaign.
'''
execution = replace_once(
    execution,
    old_relationship,
    new_relationship,
    "execution roadmap relationship section",
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
> interfaces, but it is not authority to create a second production path. Follow
> the local-first roadmap, accepted Cairn decisions, and live migration todos.

---
''',
    "trait-surface decision notice",
)
trait_path.write_text(trait)

Path("scripts/apply_composition_root_docs.py").unlink()
Path(".github/workflows/apply-composition-root-docs.yml").unlink()
