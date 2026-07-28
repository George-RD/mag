#!/usr/bin/env python3
"""Complete MAG's Cairn onboarding and remove temporary machinery."""

from __future__ import annotations

from pathlib import Path
import shutil
import textwrap


def write(path: str, content: str) -> None:
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(textwrap.dedent(content).lstrip(), encoding="utf-8")


blueprint_path = Path("cairn.blueprint")
blueprint = blueprint_path.read_text(encoding="utf-8")
blueprint = blueprint.replace('                path "./src/doctor_checks.rs"\n', "")
blueprint = blueprint.replace('                path "./src/bin/fetch_benchmark_data.rs"\n', "")

setup_anchor = '''            Module MCP "Validated MCP stdio tools over memory capabilities" id "mag.runtime.mcp" @mcp {'''
doctor_node = '''            Module Doctor "Health checks, model diagnostics, and recovery guidance" id "mag.runtime.doctor" {
                path "./src/doctor_checks.rs"
                contract "./meta/contracts/runtime.doctor.md"
            }

'''
if 'id "mag.runtime.doctor"' not in blueprint:
    blueprint = blueprint.replace(setup_anchor, doctor_node + setup_anchor, 1)

quality_anchor = '''            Module Scripts "Repository automation and benchmark entry points" id "mag.quality.scripts" {'''
benchmark_data_node = '''            Module BenchmarkData "Standalone benchmark-data acquisition utility" id "mag.quality.benchmark-data" @utility @no-test-coverage {
                path "./src/bin/fetch_benchmark_data.rs"
                contract "./meta/contracts/quality.benchmark-data.md"
            }

'''
if 'id "mag.quality.benchmark-data"' not in blueprint:
    blueprint = blueprint.replace(quality_anchor, benchmark_data_node + quality_anchor, 1)

edge_anchor = '''    mag.runtime.entrypoints -> mag.runtime.daemon "Starts the optional HTTP adapter"'''
if 'mag.runtime.entrypoints -> mag.runtime.doctor' not in blueprint:
    blueprint = blueprint.replace(
        edge_anchor,
        edge_anchor + '\n    mag.runtime.entrypoints -> mag.runtime.doctor "Runs health and model diagnostics"',
        1,
    )

script_edge = '''    mag.quality.scripts -> mag.quality.benchmarks "Runs benchmark and regression gates"'''
if 'mag.quality.benchmark-data -> mag.quality.benchmarks' not in blueprint:
    blueprint = blueprint.replace(
        script_edge,
        '    mag.quality.benchmark-data -> mag.quality.benchmarks "Acquires datasets used by benchmark runners"\n' + script_edge,
        1,
    )

blueprint_path.write_text(blueprint, encoding="utf-8")

write(
    "meta/contracts/runtime.doctor.md",
    '''
    ---
    node: mag.runtime.doctor
    ---
    # mag.runtime.doctor contract

    Doctor checks report actionable runtime, storage, model, and connector health.
    They distinguish a pre-first-use missing model from a failed initialized model,
    do not silently claim repair, and keep diagnostics separate from core semantics.
    ''',
)

write(
    "meta/contracts/quality.benchmark-data.md",
    '''
    ---
    node: mag.quality.benchmark-data
    ---
    # mag.quality.benchmark-data contract

    The benchmark-data utility only acquires and verifies versioned evaluation
    inputs. It does not alter scoring, judge results, or benchmark methodology, and
    failures leave existing cached datasets intact.
    ''',
)

baseline_path = Path("meta/decisions/dec.baseline-current-module-boundaries.md")
baseline = baseline_path.read_text(encoding="utf-8")
baseline = baseline.replace(
    "mag.runtime.entrypoints, mag.runtime.setup, mag.runtime.mcp,",
    "mag.runtime.entrypoints, mag.runtime.setup, mag.runtime.doctor, mag.runtime.mcp,",
)
baseline = baseline.replace(
    "mag.quality.tests, mag.quality.benchmarks, mag.quality.scripts",
    "mag.quality.tests, mag.quality.benchmarks, mag.quality.benchmark-data, mag.quality.scripts",
)
baseline_path.write_text(baseline, encoding="utf-8")

write(
    ".github/workflows/cairn.yml",
    '''
    name: Cairn architecture gate

    on:
      pull_request:
        branches: [main]
      push:
        branches: [main]

    permissions:
      contents: read

    jobs:
      architecture:
        runs-on: ubuntu-24.04
        steps:
          - uses: actions/checkout@v6
          - name: Install Cairn 0.9.0
            run: |
              curl --proto '=https' --tlsv1.2 -LsSf \
                https://github.com/cairn-framework/cairn/releases/download/v0.9.0/cairn-installer.sh | sh
              echo "$HOME/.cargo/bin" >> "$GITHUB_PATH"
          - name: Verify architecture, decisions, and interfaces
            run: cairn hook all
    ''',
)

# Onboarding diagnostics are superseded by tracked map/state and the permanent gate.
for obsolete_dir in [Path(".cairn/onboarding")]:
    if obsolete_dir.exists():
        shutil.rmtree(obsolete_dir)

for obsolete_file in [
    Path(".cairn-version"),
    Path(".github/workflows/cairn-bootstrap.yml"),
    Path("scripts/finalize-cairn-onboarding.py"),
    Path("scripts/remediate-cairn-onboarding.py"),
]:
    if obsolete_file.exists():
        obsolete_file.unlink()

# Delete this script last; the running interpreter already has its contents.
Path(__file__).unlink()
print("Cairn onboarding completed and temporary machinery removed")
