#!/usr/bin/env python3
"""Strategy comparison report generator for LoCoMo benchmarks.

Reads two JSON benchmark outputs and produces a comparison report
in both JSON and Markdown formats.

Python 3 stdlib only -- zero pip dependencies.
"""

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime, timezone

# ── Verdict thresholds (tune without touching algorithm) ────────────────
BETTER_OVERALL_THRESHOLD = 0.5     # B must be >= +0.5pp overall to be BETTER
BETTER_CATEGORY_MAX_REGRESSION = 2.0  # No category may regress > 2pp for BETTER
EQUIVALENT_THRESHOLD = 0.5        # Abs delta < 0.5pp in all categories = EQUIVALENT
REGRESSION_THRESHOLD = -2.0       # B overall delta <= -2pp = REGRESSION

# ── Category keys (order for table display) ─────────────────────────────
CATEGORY_KEYS = [
    ("single_hop", "Single-Hop QA"),
    ("temporal", "Temporal"),
    ("multi_hop", "Multi-Hop QA"),
    ("open_domain", "Open-Domain"),
    ("adversarial", "Adversarial"),
]


def extract_scores(data):
    """Extract score dict from a benchmark JSON output."""
    categories = data.get("categories", {})

    def cat_f1(key):
        """Mean F1 for a category, as a percentage."""
        cat = categories.get(key, {})
        total = cat.get("total", 0)
        f1_sum = cat.get("f1_sum", 0.0)
        if total == 0:
            return 0.0
        return (f1_sum / total) * 100.0

    return {
        "overall": data.get("mean_f1", 0.0) * 100.0,
        "single_hop": cat_f1("single-hop"),
        "temporal": cat_f1("temporal"),
        "multi_hop": cat_f1("multi-hop"),
        "open_domain": cat_f1("open-domain"),
        "adversarial": cat_f1("adversarial"),
        "evidence_recall": data.get("mean_evidence_recall", 0.0) * 100.0,
        "avg_query_ms": data.get("avg_query_ms", 0.0),
        "p95_query_ms": data.get("p95_query_ms", 0.0),
    }


def compute_deltas(scores_a, scores_b):
    """Compute B minus A deltas for all metrics."""
    deltas = {}
    for key in scores_a:
        deltas[key] = round(scores_b[key] - scores_a[key], 4)
    return deltas


def determine_verdict(deltas):
    """Determine comparison verdict based on delta thresholds."""
    overall_delta = deltas["overall"]
    category_deltas = [
        deltas[k] for k, _ in CATEGORY_KEYS
    ]

    max_cat_regression = min(category_deltas) if category_deltas else 0.0

    if overall_delta >= BETTER_OVERALL_THRESHOLD and max_cat_regression > -BETTER_CATEGORY_MAX_REGRESSION:
        return "BETTER", (
            f"B is +{overall_delta:.1f}pp overall with no category regressing "
            f"more than {BETTER_CATEGORY_MAX_REGRESSION}pp"
        )

    all_within_equiv = all(abs(d) < EQUIVALENT_THRESHOLD for d in category_deltas)
    if all_within_equiv and abs(overall_delta) < EQUIVALENT_THRESHOLD:
        return "EQUIVALENT", (
            f"All category deltas within {EQUIVALENT_THRESHOLD}pp noise floor"
        )

    if overall_delta <= REGRESSION_THRESHOLD:
        return "REGRESSION", (
            f"B is {overall_delta:.1f}pp overall -- exceeds "
            f"{abs(REGRESSION_THRESHOLD)}pp hard regression threshold"
        )

    return "INCONCLUSIVE", (
        f"Mixed category results; overall delta {overall_delta:+.1f}pp "
        f"within noise floor"
    )


def build_comparison_report(data_a, data_b, baselines_data=None):
    """Build the full comparison report dict."""
    strategy_a = data_a.get("strategy", "sqlite-v1")
    strategy_b = data_b.get("strategy", "sqlite-v1")
    scoring_mode = data_a.get("scoring_mode", "unknown")
    samples = data_a.get("samples_evaluated", 0)
    embedder = data_a.get("embedder_name", "unknown")

    scores_a = extract_scores(data_a)
    scores_b = extract_scores(data_b)
    deltas = compute_deltas(scores_a, scores_b)
    verdict, verdict_reason = determine_verdict(deltas)

    # Look up baseline for strategy_a
    baseline_used = None
    if baselines_data and strategy_a in baselines_data:
        mode_baseline = baselines_data[strategy_a].get(scoring_mode)
        if mode_baseline:
            baseline_used = {
                "overall": mode_baseline.get("overall"),
                "samples": mode_baseline.get("samples"),
            }

    return {
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "strategy_a": strategy_a,
        "strategy_b": strategy_b,
        "scoring_mode": scoring_mode,
        "samples": samples,
        "embedder": embedder,
        "verdict": verdict,
        "verdict_reason": verdict_reason,
        "baseline_used": baseline_used,
        "results": {
            "strategy_a": scores_a,
            "strategy_b": scores_b,
        },
        "deltas": deltas,
    }


def format_delta(val):
    """Format a delta value with sign."""
    if val > 0:
        return f"+{val:.1f}"
    return f"{val:.1f}"


def generate_markdown(report):
    """Generate a Markdown comparison report."""
    a_name = report["strategy_a"]
    b_name = report["strategy_b"]
    scores_a = report["results"]["strategy_a"]
    scores_b = report["results"]["strategy_b"]
    deltas = report["deltas"]

    lines = [
        f"# Strategy Comparison: {a_name} vs {b_name}",
        "",
        f"- Date: {report['generated_at'][:10]}",
        f"- Scoring mode: {report['scoring_mode']}",
        f"- Samples: {report['samples']}",
        f"- Embedder: {report['embedder']}",
        f"- Verdict: **{report['verdict']}** -- {report['verdict_reason']}",
        "",
        "## Score Comparison",
        "",
        f"| Category      | {a_name} | {b_name} | Delta |",
        f"|---------------|{'---:' + ' ' * (len(a_name) - 3)}|{'---:' + ' ' * (len(b_name) - 3)}|------:|",
    ]

    for key, label in CATEGORY_KEYS:
        lines.append(
            f"| {label:<13} | {scores_a[key]:>{len(a_name)}.1f}% "
            f"| {scores_b[key]:>{len(b_name)}.1f}% "
            f"| {format_delta(deltas[key]):>5} |"
        )

    # Overall (bold)
    lines.append(
        f"| **Overall**   | **{scores_a['overall']:.1f}%** "
        f"| **{scores_b['overall']:.1f}%** "
        f"| **{format_delta(deltas['overall'])}** |"
    )

    # Evidence recall
    lines.append(
        f"| Evidence Rec  | {scores_a['evidence_recall']:>{len(a_name)}.1f}% "
        f"| {scores_b['evidence_recall']:>{len(b_name)}.1f}% "
        f"| {format_delta(deltas['evidence_recall']):>5} |"
    )

    lines.extend([
        "",
        "## Latency",
        "",
        f"| Metric        | {a_name} | {b_name} | Delta |",
        f"|---------------|{'---:' + ' ' * (len(a_name) - 3)}|{'---:' + ' ' * (len(b_name) - 3)}|------:|",
        f"| Avg query ms  | {scores_a['avg_query_ms']:>{len(a_name)}.1f} "
        f"| {scores_b['avg_query_ms']:>{len(b_name)}.1f} "
        f"| {format_delta(deltas['avg_query_ms']):>5} |",
        f"| P95 query ms  | {scores_a['p95_query_ms']:>{len(a_name)}.1f} "
        f"| {scores_b['p95_query_ms']:>{len(b_name)}.1f} "
        f"| {format_delta(deltas['p95_query_ms']):>5} |",
    ])

    # Baseline comparison
    baseline = report.get("baseline_used")
    if baseline and baseline.get("overall") is not None:
        bl_overall = baseline["overall"]
        bl_samples = baseline.get("samples", "?")
        delta_from_bl = scores_a["overall"] - bl_overall
        gate = "PASS" if abs(delta_from_bl) <= 2.0 else "WARN"
        lines.extend([
            "",
            f"## Baseline comparison ({a_name} reference)",
            "",
            f"Baseline: {bl_overall}% ({bl_samples}-sample)",
            f"Current A: {scores_a['overall']:.1f}% -- "
            f"within {abs(delta_from_bl):.1f}pp of baseline. Gate: {gate}",
        ])

    lines.append("")
    return "\n".join(lines)


def _git_short_hash() -> str:
    """Return the current git short commit hash, or 'unknown' if unavailable."""
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "--short", "HEAD"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except (subprocess.CalledProcessError, FileNotFoundError):
        return "unknown"


def main():
    parser = argparse.ArgumentParser(
        description="Compare two LoCoMo benchmark strategy results"
    )
    parser.add_argument(
        "--a-json", required=True,
        help="Path to strategy A JSON result"
    )
    parser.add_argument(
        "--b-json", required=True,
        help="Path to strategy B JSON result"
    )
    parser.add_argument(
        "--baselines", default=None,
        help="Path to baselines.json"
    )
    parser.add_argument(
        "--output-dir", default=None,
        help="Directory to write comparison reports"
    )
    parser.add_argument(
        "--update-baseline", action="store_true",
        help="Update baselines.json with strategy B result"
    )

    args = parser.parse_args()

    # Load inputs
    try:
        with open(args.a_json) as f:
            data_a = json.load(f)
        with open(args.b_json) as f:
            data_b = json.load(f)
    except (ValueError, json.JSONDecodeError) as e:
        print(f"error: failed to parse benchmark JSON: {e}", file=sys.stderr)
        sys.exit(1)

    baselines_data = None
    if args.baselines and os.path.exists(args.baselines):
        try:
            with open(args.baselines) as f:
                baselines_data = json.load(f)
        except (ValueError, json.JSONDecodeError) as e:
            print(f"error: failed to parse baselines JSON: {e}", file=sys.stderr)
            sys.exit(1)

    # Build report
    report = build_comparison_report(data_a, data_b, baselines_data)

    # Generate markdown
    markdown = generate_markdown(report)

    # Output
    if args.output_dir:
        strategy_a = report["strategy_a"]
        strategy_b = report["strategy_b"]
        date_str = datetime.now().strftime("%Y-%m-%d")
        dirname = f"{date_str}_{strategy_a}_vs_{strategy_b}"
        out_path = os.path.join(args.output_dir, dirname)
        os.makedirs(out_path, exist_ok=True)

        json_path = os.path.join(out_path, "comparison_report.json")
        md_path = os.path.join(out_path, "comparison_report.md")

        with open(json_path, "w") as f:
            json.dump(report, f, indent=2)
            f.write("\n")
        with open(md_path, "w") as f:
            f.write(markdown)

        print(f"Reports written to {out_path}/", file=sys.stderr)

    # Always print markdown to stdout
    print(markdown)

    # Update baseline if requested
    if args.update_baseline and args.baselines:
        strategy_b_id = report["strategy_b"]
        scoring_mode = report["scoring_mode"]
        scores_b = report["results"]["strategy_b"]

        if baselines_data is None:
            baselines_data = {"schema_version": 1}

        if strategy_b_id not in baselines_data:
            baselines_data[strategy_b_id] = {}

        # Preserve old entry under a superseded key
        if scoring_mode in baselines_data[strategy_b_id]:
            date_str = datetime.now().strftime("%Y-%m-%d")
            old_key = f"_superseded_{date_str}_{scoring_mode}"
            baselines_data[strategy_b_id][old_key] = baselines_data[strategy_b_id][scoring_mode]

        baselines_data[strategy_b_id][scoring_mode] = {
            "samples": report["samples"],
            "overall": round(scores_b["overall"], 1),
            "single_hop": round(scores_b["single_hop"], 1),
            "temporal": round(scores_b["temporal"], 1),
            "multi_hop": round(scores_b["multi_hop"], 1),
            "open_domain": round(scores_b["open_domain"], 1),
            "adversarial": round(scores_b["adversarial"], 1),
            "evidence_recall": round(scores_b["evidence_recall"], 1),
            "avg_query_ms": round(scores_b["avg_query_ms"], 1),
            "p95_query_ms": round(scores_b["p95_query_ms"], 1),
            "embedder": report["embedder"],
            "commit": _git_short_hash(),
            "date": datetime.now().strftime("%Y-%m-%d"),
            "notes": f"auto-updated via compare_strategies.py",
        }

        with open(args.baselines, "w") as f:
            json.dump(baselines_data, f, indent=2)
            f.write("\n")
        print(f"Updated baseline for {strategy_b_id}/{scoring_mode}", file=sys.stderr)


if __name__ == "__main__":
    main()
