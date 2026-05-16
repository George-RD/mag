#!/usr/bin/env python3
"""
LongMemEval Multi-Session Active Injection A/B Test Runner.

Usage::

    python python/longmemeval_active_runner.py \
        --dataset data/longmemeval_s_cleaned.json \
        --strategies passive,active_threshold \
        --output results/lme_active.json

    python python/longmemeval_active_runner.py \
        --dataset data/longmemeval_s_cleaned.json \
        --strategies all \
        --question-types multi-session \
        --output results/lme_multi_session.json \
        --trace results/lme_multi_session.jsonl
"""

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional

sys.path.insert(0, str(Path(__file__).parent))

from mag_memory.mcp_client import McpClient
from mag_memory.longmemeval_runner import LongMemEvalRunner
from mag_memory.active_strategies import STRATEGIES


def load_longmemeval_dataset(path: str) -> List[Dict[str, Any]]:
    """Load the official LongMemEval_S cleaned JSON."""
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def build_strategy(name: str, **kwargs: Any):
    """Instantiate a strategy by name with optional overrides."""
    cls = STRATEGIES.get(name)
    if not cls:
        raise ValueError(f"Unknown strategy: {name}. Available: {list(STRATEGIES.keys())}")
    return cls(**kwargs)


def main() -> int:
    parser = argparse.ArgumentParser(description="LongMemEval Active Injection A/B Test")
    parser.add_argument("--dataset", type=str, default="data/longmemeval_s_cleaned.json",
                        help="Path to LongMemEval_S JSON")
    parser.add_argument("--strategies", type=str, default="passive,active_threshold",
                        help="Comma-separated strategy names, or 'all'")
    parser.add_argument("--question-types", type=str, default="multi-session",
                        help="Comma-separated question types to include, or 'all'")
    parser.add_argument("--limit", type=int, default=5, help="Retrieval limit per search")
    parser.add_argument("--threshold", type=float, default=0.5,
                        help="Confidence threshold for active_threshold")
    parser.add_argument("--output", type=str, default="results/lme_active.json",
                        help="JSON output path")
    parser.add_argument("--trace", type=str, default=None,
                        help="JSONL trace output path")
    parser.add_argument("--samples", type=int, default=None,
                        help="Limit number of questions (for quick testing)")
    parser.add_argument("--mag-binary", type=str, default=None)
    parser.add_argument("--home-dir", type=str, default=None)
    args = parser.parse_args()

    if not os.path.exists(args.dataset):
        print(f"ERROR: Dataset not found: {args.dataset}", file=sys.stderr)
        return 1

    # Load and filter dataset
    raw_data = load_longmemeval_dataset(args.dataset)
    print(f"Loaded {len(raw_data)} total questions")

    if args.question_types == "all":
        question_types = None
    else:
        question_types = set(t.strip() for t in args.question_types.split(","))
        print(f"Filtering to question types: {question_types}")

    questions = []
    for q in raw_data:
        if question_types is None or q.get("question_type", "") in question_types:
            questions.append(q)

    if args.samples is not None:
        questions = questions[: args.samples]

    print(f"Evaluating {len(questions)} questions")
    if not questions:
        print("ERROR: No questions match filters", file=sys.stderr)
        return 1

    # Build strategies
    if args.strategies == "all":
        strategy_names = list(STRATEGIES.keys())
    else:
        strategy_names = [s.strip() for s in args.strategies.split(",")]

    strategies = []
    for name in strategy_names:
        kwargs = {"limit": args.limit}
        if name == "active_threshold":
            kwargs["threshold"] = args.threshold
        strategies.append(build_strategy(name, **kwargs))

    print(f"Strategies: {[s.name for s in strategies]}")

    # Run A/B test
    client = McpClient(mag_binary=args.mag_binary, home_dir=args.home_dir)
    client.start()
    trace_path = Path(args.trace) if args.trace else None
    runner = LongMemEvalRunner(client=client, trace_path=trace_path)

    try:
        # Seed all sessions from all questions upfront.
        # For fair A/B comparison, each strategy runs against the same seeded state.
        print("\n>>> Seeding sessions...")
        total_stored = 0
        seen_sessions = set()
        for q in questions:
            session_ids = q.get("haystack_session_ids", [])
            sessions = q.get("haystack_sessions", [])
            for sid, turns in zip(session_ids, sessions):
                key = (q["question_id"], sid)
                if key not in seen_sessions:
                    seen_sessions.add(key)
                    # Store as a single batch per session
                    for tidx, turn in enumerate(turns):
                        runner.client.store(
                            content=turn["content"],
                            id=f"{q['question_id']}_{sid}_t{tidx}",
                            project=runner.project,
                            session_id=sid,
                            agent_type=turn.get("role", "unknown"),
                            tags=["longmemeval", q.get("question_type", "unknown")],
                        )
                        total_stored += 1
        print(f"Stored {total_stored} turns across {len(seen_sessions)} unique sessions")

        summaries = runner.run_ab_test(questions, strategies)
    finally:
        runner.close()
        client.stop()

    # Write report
    report = {
        "timestamp": time.time(),
        "dataset": args.dataset,
        "question_types": args.question_types,
        "total_questions": len(questions),
        "strategies": {},
    }
    for name, summary in summaries.items():
        report["strategies"][name] = {
            "accuracy": summary.accuracy,
            "passed": summary.passed,
            "failed": summary.failed,
            "total": summary.total_questions,
            "avg_latency_ms": summary.avg_latency_ms,
            "category_breakdown": summary.category_breakdown,
        }

    # Print comparison table
    print("\n" + "=" * 60)
    print("A/B TEST RESULTS")
    print("=" * 60)
    for name, summary in summaries.items():
        print(f"\n{name}:")
        print(f"  Accuracy: {summary.accuracy:.1%} ({summary.passed}/{summary.total_questions})")
        print(f"  Avg latency: {summary.avg_latency_ms:.0f}ms")
        for cat, stats in summary.category_breakdown.items():
            print(f"  {cat}: {stats['accuracy']:.1%} ({stats['passed']}/{stats['total']})")

    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2, ensure_ascii=False)
    print(f"\nReport written to: {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
