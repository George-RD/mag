#!/usr/bin/env python3
"""Score versioned, recorded memory-intelligence outputs without running a model.

Labels are exact, unordered matches. Grounded matches also require the complete
annotated source-ID set. Missing or invalid outputs never leave the denominator.
Only supplied measurements are reported; scoring overhead is not model latency.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
from pathlib import Path
import re
import tempfile
from typing import Any

TASKS = {
    "facts", "entities", "temporal", "relationships", "decisions", "questions",
    "status", "grouping", "contradictions", "provenance",
}
COUNTS = ("true_positives", "false_positives", "false_negatives")


def _canonical(value: Any) -> bytes:
    return json.dumps(
        value, ensure_ascii=False, sort_keys=True, separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def dataset_sha256(dataset: dict[str, Any]) -> str:
    """Hash canonical JSON, including instructions, source text and annotations."""
    return hashlib.sha256(_canonical(dataset)).hexdigest()


def _object(
    value: Any, required: set[str], label: str, optional: set[str] | None = None,
) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    if not required <= value.keys() or value.keys() - required - (optional or set()):
        raise ValueError(f"{label} has missing or unknown fields")
    return value


def _text(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{label} must be a nonempty string")
    return value


def _version(value: Any, label: str) -> None:
    if type(value) is not int or value != 1:
        raise ValueError(f"{label} schema_version must be 1")


def _items(value: Any) -> dict[str, frozenset[str]]:
    if not isinstance(value, list):
        raise ValueError("items must be an array")
    parsed = {}
    for entry in value:
        entry = _object(entry, {"value", "source_ids"}, "item")
        label = _text(entry["value"], "item value")
        sources = entry["source_ids"]
        if not isinstance(sources, list) or not sources:
            raise ValueError("source_ids must be a nonempty array")
        for source in sources:
            _text(source, "source ID")
        if len(set(sources)) != len(sources) or label in parsed:
            raise ValueError("duplicate value or source ID")
        parsed[label] = frozenset(sources)
    return parsed


def validate_dataset(dataset: Any) -> dict[str, dict[str, Any]]:
    dataset = _object(dataset, {"schema_version", "dataset_id", "cases"}, "dataset")
    _version(dataset["schema_version"], "dataset")
    _text(dataset["dataset_id"], "dataset_id")
    if not isinstance(dataset["cases"], list) or not dataset["cases"]:
        raise ValueError("dataset cases must be a nonempty array")
    cases = {}
    for case in dataset["cases"]:
        case = _object(case, {"id", "task", "instruction", "sources", "expected"}, "case")
        case_id = _text(case["id"], "case ID")
        task = _text(case["task"], "task")
        if case_id in cases or task not in TASKS:
            raise ValueError(f"duplicate case ID or unsupported task: {case_id}")
        _text(case["instruction"], "instruction")
        if not isinstance(case["sources"], list) or not case["sources"]:
            raise ValueError(f"case {case_id} must have source memories")
        source_ids = set()
        for source in case["sources"]:
            source = _object(source, {"id", "text"}, "source")
            source_id = _text(source["id"], "source ID")
            _text(source["text"], "source text")
            if source_id in source_ids:
                raise ValueError(f"duplicate source ID in case {case_id}")
            source_ids.add(source_id)
        for support in _items(case["expected"]).values():
            if not support <= source_ids:
                raise ValueError(f"unknown annotated source ID in case {case_id}")
        cases[case_id] = case
    return cases


def _measurement(value: Any, label: str, *, integer: bool = False) -> None:
    if value is None:
        return
    valid_type = type(value) is int or (not integer and type(value) is float)
    if not valid_type or value < 0 or (type(value) is float and not math.isfinite(value)):
        kind = "integer" if integer else "number"
        raise ValueError(f"{label} must be a finite nonnegative {kind} or null")


def _validate_run(
    run: Any, dataset: dict[str, Any], cases: dict[str, Any],
) -> dict[str, dict[str, Any]]:
    run = _object(run, {
        "schema_version", "dataset_sha256", "code_revision", "model_profile",
        "embedding_space_identity", "measurement_context", "results",
    }, "run", {"load_time_ms", "peak_ram_bytes"})
    _version(run["schema_version"], "run")
    if run["dataset_sha256"] != dataset_sha256(dataset):
        raise ValueError("run dataset_sha256 does not match this dataset")
    if not re.fullmatch(r"[0-9a-f]{40}", _text(run["code_revision"], "code_revision")):
        raise ValueError("code_revision must be a full lowercase Git commit SHA")
    for field in ("model_profile", "measurement_context"):
        if not isinstance(run[field], dict) or not run[field]:
            raise ValueError(f"{field} must be a nonempty metadata snapshot")
        _canonical(run[field])
    if run["embedding_space_identity"] is not None:
        _text(run["embedding_space_identity"], "embedding_space_identity")
    _measurement(run.get("load_time_ms"), "load_time_ms")
    _measurement(run.get("peak_ram_bytes"), "peak_ram_bytes", integer=True)
    if not isinstance(run["results"], list):
        raise ValueError("run results must be an array")
    results = {}
    for result in run["results"]:
        result = _object(result, {"case_id"}, "result", {
            "output", "error", "latency_ms", "input_tokens", "output_tokens",
        })
        case_id = _text(result["case_id"], "result case_id")
        if case_id not in cases or case_id in results:
            raise ValueError(f"unknown or duplicate result case_id: {case_id}")
        if ("output" in result) == ("error" in result):
            raise ValueError(f"result {case_id} must contain exactly one of output or error")
        if "error" in result:
            _text(result["error"], "error")
        for field in ("latency_ms", "input_tokens", "output_tokens"):
            _measurement(result.get(field), field, integer=field != "latency_ms")
        results[case_id] = result
    return results


def _counts(expected: set, predicted: set) -> dict[str, int]:
    return dict(zip(COUNTS, (
        len(expected & predicted), len(predicted - expected), len(expected - predicted),
    )))


def _score_case(case: dict[str, Any], result: dict[str, Any] | None) -> dict[str, Any]:
    expected = _items(case["expected"])
    predicted = {}
    error = None
    if result is None:
        error = "missing result"
    elif "error" in result:
        error = f"runtime error: {result['error']}"
    else:
        try:
            output = _object(result["output"], {"items"}, "output")
            predicted = _items(output["items"])
        except ValueError as exc:
            error = f"invalid output: {exc}"
    known_sources = {source["id"] for source in case["sources"]}
    cited_sources = {source for support in predicted.values() for source in support}
    return {
        "id": case["id"], "task": case["task"], "schema_valid": error is None,
        "success": error is None and predicted == expected, "error": error,
        "unknown_source_ids": sorted(cited_sources - known_sources),
        "content": _counts(set(expected), set(predicted)),
        "grounded": _counts(set(expected.items()), set(predicted.items())),
    }


def _ratio(numerator: int, denominator: int) -> float | None:
    return numerator / denominator if denominator else None


def _summarize(cases: list[dict[str, Any]]) -> dict[str, Any]:
    total = len(cases)
    valid = sum(case["schema_valid"] for case in cases)
    succeeded = sum(case["success"] for case in cases)
    summary = {
        "schema_validity": {"valid": valid, "total": total, "rate": _ratio(valid, total)},
        "task_success": {"succeeded": succeeded, "total": total, "rate": _ratio(succeeded, total)},
    }
    for metric in ("content", "grounded"):
        counts = {key: sum(case[metric][key] for case in cases) for key in COUNTS}
        tp, fp, fn = (counts[key] for key in COUNTS)
        summary[metric] = {
            **counts, "precision": _ratio(tp, tp + fp), "recall": _ratio(tp, tp + fn),
            "f1": _ratio(2 * tp, 2 * tp + fp + fn),
        }
    return summary


def _performance(run: dict[str, Any], results: dict[str, Any]) -> dict[str, Any]:
    samples = {
        field: [result[field] for result in results.values() if result.get(field) is not None]
        for field in ("latency_ms", "input_tokens", "output_tokens")
    }
    latencies = sorted(samples["latency_ms"])
    # Nearest-rank quantiles: observed values only, including recorded failed attempts.
    quantiles = {
        name: latencies[math.ceil(len(latencies) * percentile) - 1] if latencies else None
        for name, percentile in (("p50", 0.5), ("p95", 0.95))
    }
    return {
        "attempted_cases": len(results),
        "latency_ms": {**quantiles, "samples": len(latencies)},
        **{
            field: {
                "total": sum(samples[field]) if samples[field] else None,
                "samples": len(samples[field]),
            }
            for field in ("input_tokens", "output_tokens")
        },
        "load_time_ms": run.get("load_time_ms"), "peak_ram_bytes": run.get("peak_ram_bytes"),
    }


def evaluate(dataset: dict[str, Any], run: dict[str, Any]) -> dict[str, Any]:
    """Return an auditable scorecard. Invalid run metadata raises ValueError."""
    cases = validate_dataset(dataset)
    results = _validate_run(run, dataset, cases)
    scored = [_score_case(case, results.get(case_id)) for case_id, case in cases.items()]
    return {
        "schema_version": 1, "scoring_method": "exact-label-and-source-set-v1",
        "dataset_id": dataset["dataset_id"], "dataset_sha256": dataset_sha256(dataset),
        "code_revision": run["code_revision"],
        "model_profile": copy.deepcopy(run["model_profile"]),
        "model_profile_sha256": hashlib.sha256(_canonical(run["model_profile"])).hexdigest(),
        "embedding_space_identity": run["embedding_space_identity"],
        "measurement_context": copy.deepcopy(run["measurement_context"]),
        "overall": _summarize(scored),
        "by_task": {
            task: _summarize([case for case in scored if case["task"] == task])
            for task in sorted({case["task"] for case in scored})
        },
        "performance": _performance(run, results), "cases": scored,
    }


def _unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError(f"duplicate JSON key: {key}")
        value[key] = item
    return value


def _reject_constant(value: str) -> None:
    raise ValueError(f"nonfinite JSON constant: {value}")


def _finite_float(value: str) -> float:
    parsed = float(value)
    if not math.isfinite(parsed):
        raise ValueError(f"nonfinite JSON number: {value}")
    return parsed


def load_json(path: Path) -> Any:
    return json.loads(
        path.read_text(encoding="utf-8"),
        object_pairs_hook=_unique_object,
        parse_constant=_reject_constant,
        parse_float=_finite_float,
    )


def _write_report(path: Path, text: str, inputs: tuple[Path, ...]) -> None:
    for source in inputs:
        if path.resolve() == source.resolve() or (path.exists() and path.samefile(source)):
            raise ValueError("output must not overwrite an input file")
    # An invalid run or interrupted write must not leave a partially written scorecard.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", dir=path.parent, delete=False,
        ) as stream:
            temporary = Path(stream.name)
            stream.write(text)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate", help="validate a dataset and print its identity")
    validate.add_argument("dataset", type=Path)
    score = commands.add_parser("score", help="score recorded outputs; does not invoke a model")
    score.add_argument("dataset", type=Path)
    score.add_argument("run", type=Path)
    score.add_argument("--output", type=Path, help="write JSON atomically instead of printing it")
    args = parser.parse_args()
    try:
        dataset = load_json(args.dataset)
        if args.command == "validate":
            cases = validate_dataset(dataset)
            report = {
                "schema_version": 1, "dataset_id": dataset["dataset_id"],
                "dataset_sha256": dataset_sha256(dataset), "cases": len(cases),
                "tasks": sorted({case["task"] for case in cases.values()}),
            }
        else:
            report = evaluate(dataset, load_json(args.run))
        text = json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False) + "\n"
        if args.command == "score" and args.output is not None:
            _write_report(args.output, text, (args.dataset, args.run))
        else:
            print(text, end="")
    except (OSError, ValueError, TypeError, RecursionError) as exc:
        parser.exit(2, f"error: {exc}\n")


if __name__ == "__main__":
    main()
