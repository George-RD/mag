#!/usr/bin/env python3
"""Compare MAG (Rust) vs omega-memory (Python) on the same workload.

Runs the EXACT same local_benchmark.json seed + query operations through
omega-memory's SQLiteStore. All 100 questions match the Rust benchmark 1:1.

Usage:
    cd /path/to/mag
    uv run --project ~/repos/omega-memory python benches/python_comparison.py [--verbose]
"""

import argparse
import json
import os
import resource
import shutil
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from typing import TYPE_CHECKING

DEFAULT_OMEGA_REPO = os.path.expanduser("~/repos/omega-memory")
VERBOSE = False

if TYPE_CHECKING:
    from omega.sqlite_store import SQLiteStore


def peak_rss_kb() -> int:
    ru = resource.getrusage(resource.RUSAGE_SELF)
    if sys.platform == "darwin":
        return ru.ru_maxrss // 1024
    return ru.ru_maxrss


def load_benchmark_data() -> dict:
    script_dir = os.path.dirname(os.path.abspath(__file__))
    data_path = os.path.join(script_dir, "..", "data", "local_benchmark.json")
    with open(data_path) as f:
        return json.load(f)


def iso(dt: datetime) -> str:
    return dt.strftime("%Y-%m-%dT%H:%M:%SZ")


def seed_memories(store: "SQLiteStore", data: dict) -> int:
    now = datetime.now(timezone.utc)
    count = 0
    seeds = data["seed_memories"]

    # Information extraction memories
    for mem in seeds["information_extraction"]:
        store.store(
            content=mem["content"],
            session_id="bench-ie",
            metadata={
                "event_type": mem["event_type"],
                "priority": mem["priority"],
            },
        )
        count += 1

    # Multi-session memories
    for mem in seeds["multi_session"]:
        store.store(
            content=mem["content"],
            session_id=mem["session_id"],
            metadata={
                "event_type": mem["event_type"],
                "priority": mem["priority"],
            },
        )
        count += 1

    # Temporal memories (sprint completions with referenced_date)
    sprint_count = seeds["temporal"]["sprint_count"]
    interval_days = seeds["temporal"]["interval_days"]
    for i in range(sprint_count):
        days_ago = i * interval_days
        ref_date = now - timedelta(days=days_ago)
        sprint_num = sprint_count - i
        content = (
            f"Sprint {sprint_num} completed: deployed feature batch "
            f"#{sprint_num} to production on {ref_date.strftime('%Y-%m-%d')}."
        )
        store.store(
            content=content,
            session_id=f"bench-tr-{i}",
            metadata={
                "event_type": "task_completion",
                "priority": 3,
                "referenced_date": iso(ref_date),
            },
        )
        count += 1

    # Knowledge update pairs (old with negative feedback, new with positive)
    old_date = iso(now - timedelta(days=60))
    new_date = iso(now - timedelta(days=2))
    for i, pair in enumerate(seeds["knowledge_update"]):
        store.store(
            content=pair["old_content"],
            session_id=f"bench-ku-old-{i}",
            metadata={
                "event_type": "decision",
                "priority": 3,
                "referenced_date": old_date,
                "feedback_score": -1,
            },
        )
        count += 1
        store.store(
            content=pair["new_content"],
            session_id=f"bench-ku-new-{i}",
            metadata={
                "event_type": "decision",
                "priority": 4,
                "referenced_date": new_date,
                "feedback_score": 2,
            },
        )
        count += 1

    return count


class Results:
    def __init__(self):
        self.categories: dict = {}
        self.total = 0
        self.correct = 0

    def record(self, category: str, passed: bool, detail: str = ""):
        self.total += 1
        if passed:
            self.correct += 1
        cat = self.categories.setdefault(category, {"total": 0, "correct": 0, "details": []})
        cat["total"] += 1
        if passed:
            cat["correct"] += 1
        if detail and VERBOSE:
            cat["details"].append(detail)


def substring_match(results, expected: str) -> bool:
    expected_lower = expected.lower()
    return any(expected_lower in r.content.lower() for r in results)


def run_queries(store: "SQLiteStore", data: dict) -> Results:
    now = datetime.now(timezone.utc)
    questions = data["questions"]
    res = Results()

    def query3(query_text: str, temporal_range=None):
        return store.query(
            query_text,
            limit=3,
            use_cache=False,
            temporal_range=temporal_range,
            include_infrastructure=True,
        )

    def check(category: str, query_text: str, expected: str, temporal_range=None):
        results = query3(query_text, temporal_range=temporal_range)
        hit = substring_match(results, expected)
        detail = ""
        if not hit:
            actual = results[0].content[:60] if results else "NO RESULTS"
            detail = f"  [FAIL] Q: {query_text[:60]}  E: {expected[:40]}  Got: {actual}"
        res.record(category, hit, detail)
        return hit

    # ── Information extraction (20 questions) ──
    for q in questions["information_extraction"]:
        check("information_extraction", q["query"], q["expected"])

    # ── Multi-session (20 questions) ──
    for q in questions["multi_session"]:
        check("multi_session", q["query"], q["expected"])

    # ── Temporal (20 questions) ──
    now_iso = iso(now)

    # recent_week (2)
    week_range = (iso(now - timedelta(days=7)), now_iso)
    for q in questions["temporal"]["recent_week"]:
        check("temporal", q["query"], q["expected"], temporal_range=week_range)

    # two_weeks (1)
    two_weeks_range = (iso(now - timedelta(days=14)), now_iso)
    for q in questions["temporal"]["two_weeks"]:
        check("temporal", q["query"], q["expected"], temporal_range=two_weeks_range)

    # month (1)
    month_range = (iso(now - timedelta(days=30)), now_iso)
    for q in questions["temporal"]["month"]:
        check("temporal", q["query"], q["expected"], temporal_range=month_range)

    # empty_old_range (4)
    eor = questions["temporal"]["empty_old_range"]
    for _ in range(eor["count"]):
        old_range = (
            iso(now - timedelta(days=eor["range_start_days_ago"])),
            iso(now - timedelta(days=eor["range_end_days_ago"])),
        )
        results = query3(eor["query"], temporal_range=old_range)
        passed = len(results) == 0
        detail = ""
        if not passed:
            detail = f"  [FAIL] Expected no results for {eor['range_end_days_ago']}-{eor['range_start_days_ago']} days ago, got {len(results)}"
        res.record("temporal", passed, detail)

    # window_checks (7)
    wc = questions["temporal"]["window_checks"]
    for days_window in wc["windows_days"]:
        window_range = (iso(now - timedelta(days=days_window)), now_iso)
        results = query3(wc["query"], temporal_range=window_range)
        passed = len(results) > 0
        detail = ""
        if not passed:
            detail = f"  [FAIL] Expected results for last {days_window} days, got 0"
        res.record("temporal", passed, detail)

    # rolling_windows (5)
    rw = questions["temporal"]["rolling_windows"]
    for i in range(rw["count"]):
        rolling_range = (
            iso(now - timedelta(days=rw["window_size_days"] + i * rw["window_size_days"])),
            iso(now - timedelta(days=i * rw["window_size_days"])),
        )
        results = query3(rw["query"], temporal_range=rolling_range)
        passed = len(results) > 0
        res.record("temporal", passed)

    # ── Knowledge update (20 questions) ──

    # new_value (10)
    for q in questions["knowledge_update"]["new_value"]:
        check("knowledge_update", q["query"], q["expected"])

    # old_not_ranked_first (5)
    for item in questions["knowledge_update"]["old_not_ranked_first"]:
        results = query3(item["query"])
        top_is_old = (
            bool(results)
            and item["old_substring"].lower() in results[0].content.lower()
        )
        passed = bool(results) and not top_is_old
        detail = ""
        if not passed:
            actual = results[0].content[:60] if results else "NO RESULTS"
            detail = f"  [FAIL] Old version ranked #1: {actual}"
        res.record("knowledge_update", passed, detail)

    # additional_new (5)
    for q in questions["knowledge_update"]["additional_new"]:
        check("knowledge_update", q["query"], q["expected"])

    # ── Abstention (20 questions) ──
    for query_text in questions["abstention"]:
        results = query3(query_text)
        # Python doesn't have the same abstention gate as Rust.
        # Count as pass if no results returned.
        passed = len(results) == 0
        detail = ""
        if not passed:
            top_score = results[0].relevance if results else 0
            detail = f"  [FAIL] Q: {query_text[:40]}  top_relevance={top_score:.2f}"
        res.record("abstention", passed, detail)

    return res


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--verbose", action="store_true")
    parser.add_argument("--omega-repo", default=os.environ.get("OMEGA_REPO", DEFAULT_OMEGA_REPO))
    return parser.parse_args()


def machine_descriptor() -> str:
    return f"{os.uname().sysname} {os.uname().machine}"


def git_commit() -> str | None:
    try:
        import subprocess

        output = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        return output or None
    except Exception:
        return None


def emit_skip(reason: str):
    print(
        json.dumps(
            {
                "runtime": "python/omega-memory",
                "status": "skipped",
                "reason": reason,
            },
            indent=2,
        )
    )


def main():
    global VERBOSE
    args = parse_args()
    VERBOSE = args.verbose
    omega_repo = os.path.expanduser(args.omega_repo)
    omega_src = os.path.join(omega_repo, "src")
    if not os.path.isdir(omega_src):
        emit_skip(f"omega-memory repo not found at {omega_repo}")
        return

    sys.path.insert(0, omega_src)
    try:
        from omega.sqlite_store import SQLiteStore  # noqa: E402
    except Exception as exc:
        emit_skip(f"failed to import omega-memory from {omega_repo}: {exc}")
        return

    data = load_benchmark_data()

    tmpdir = tempfile.mkdtemp(prefix="mag_pybench_")
    os.environ["OMEGA_HOME"] = tmpdir
    db_path = os.path.join(tmpdir, "bench.db")

    store = None
    try:
        store = SQLiteStore(db_path=db_path)

        # Seed
        t0 = time.monotonic()
        seeded = seed_memories(store, data)
        seeding_ms = int((time.monotonic() - t0) * 1000)

        # Query
        t1 = time.monotonic()
        results = run_queries(store, data)
        querying_ms = int((time.monotonic() - t1) * 1000)

        rss = peak_rss_kb()

        # Build output matching Rust format
        categories = {}
        for name, cat in results.categories.items():
            categories[name] = {
                "total": cat["total"],
                "correct": cat["correct"],
                "details": cat.get("details", []),
            }

        output = {
            "benchmark": "omega_memory_comparison",
            "runtime": "python/omega-memory",
            "command": " ".join(sys.argv),
            "date": datetime.now(timezone.utc).isoformat(),
            "commit": git_commit(),
            "machine": machine_descriptor(),
            "dataset_source": "repo-local",
            "dataset_path": "data/local_benchmark.json",
            "seeded_memories": seeded,
            "seeding_ms": seeding_ms,
            "querying_ms": querying_ms,
            "peak_rss_kb": rss,
            "total_correct": results.correct,
            "total_questions": results.total,
            "overall_percentage": round(100 * results.correct / results.total, 1) if results.total > 0 else 0,
            "categories": categories,
        }

        print(json.dumps(output, indent=2))
    finally:
        if store is not None:
            store.close()
        shutil.rmtree(tmpdir, ignore_errors=True)


if __name__ == "__main__":
    main()
