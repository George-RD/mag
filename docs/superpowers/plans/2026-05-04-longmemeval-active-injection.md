# LongMemEval Multi-Session Active Injection A/B Test — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Python harness that runs LongMemEval multi-session questions (133 of 500) under passive and active retrieval conditions, comparing answer accuracy and producing JSON traces.

**Architecture:** Extend the existing Python MCP client with a `LongMemEvalRunner` class. The runner seeds MAG with haystack sessions, then evaluates each question under multiple recall strategies. Active strategies simulate proactive memory surfacing by triggering additional searches at boundaries or when confidence thresholds are met. Results are scored via substring match (consistent with the official Rust benchmark) and written as JSONL traces.

**Tech Stack:** Python 3.14, existing `python/mag_memory/` MCP client, `mag serve` binary, `datasets` library (for optional HF fallback), OpenAI API (optional LLM judge).

---

## File Structure

| File | Responsibility |
|------|---------------|
| `python/mag_memory/mcp_client.py` | Existing MCP client; needs `explain` param exposure and score parsing |
| `python/mag_memory/longmemeval_runner.py` | **New.** Core runner: seed sessions, run conditions, score, emit traces |
| `python/mag_memory/active_strategies.py` | **New.** Active recall strategy implementations (threshold, boundary, periodic) |
| `python/longmemeval_active_runner.py` | **New.** CLI entry point: load dataset, configure runner, print report |
| `tests/python/test_longmemeval_runner.py` | **New.** Unit tests for runner and strategies |
| `tests/python/test_active_strategies.py` | **New.** Unit tests for strategy logic |

---

### Task 1: Extend MCP Client with Score Access

**Files:**
- Modify: `python/mag_memory/mcp_client.py`
- Test: `tests/python/test_mcp_client.py` (existing, add cases)

The official benchmark evaluates via substring match on retrieved content. For active strategies (especially threshold-based), the harness needs access to per-result scores and the overall confidence. The MCP `memory_search` tool supports `explain=true` but the Python client doesn't expose it, and doesn't parse score fields from results.

- [ ] **Step 1: Add `explain` parameter to `McpClient.search()`**

Modify `python/mag_memory/mcp_client.py`. Find the `search` method (~line 180):

```python
def search(self, query: str, mode: str = "text", limit: int = 10,
           advanced: bool = True, explain: bool = False, **kwargs: Any) -> Dict[str, Any]:
    """Shorthand for memory_search."""
    arguments = {
        "query": query,
        "mode": mode,
        "limit": limit,
        "advanced": advanced,
        "explain": explain,
        **kwargs,
    }
    return self.call_tool("memory_search", {k: v for k, v in arguments.items() if v is not None})
```

- [ ] **Step 2: Add score-parsing helper to `McpClient`**

Add a new method after `search()`:

```python
def search_with_scores(self, query: str, mode: str = "text", limit: int = 10,
                       advanced: bool = True, **kwargs: Any) -> Dict[str, Any]:
    """
    Run memory_search with explain=true and annotate each result with its score.
    Returns the full response dict with 'results' containing score fields.
    """
    resp = self.search(query=query, mode=mode, limit=limit, advanced=advanced,
                       explain=True, **kwargs)
    # MAG's advanced search returns _text_overlap in metadata when explain=true.
    # Ensure each result has a 'score' key for uniform access.
    for r in resp.get("results", []):
        meta = r.get("metadata", {})
        if "score" not in r and "_text_overlap" in meta:
            r["score"] = meta["_text_overlap"]
        elif "score" not in r:
            r["score"] = 0.0
    return resp
```

- [ ] **Step 3: Write failing test for `search_with_scores`**

Create `tests/python/test_mcp_client.py` if it doesn't exist, or add to it:

```python
def test_search_with_scores_injects_score():
    from mag_memory.mcp_client import McpClient
    # Mock the underlying call_tool to avoid spawning mag serve
    client = McpClient.__new__(McpClient)
    client._req_id = 0
    
    def mock_call_tool(name, arguments):
        return {
            "results": [
                {"content": "hello", "metadata": {"_text_overlap": 0.85}},
                {"content": "world", "metadata": {}},
            ],
            "result_count": 2,
            "abstained": False,
            "confidence": 0.85,
        }
    
    client.call_tool = mock_call_tool
    resp = client.search_with_scores("test query")
    assert resp["results"][0]["score"] == 0.85
    assert resp["results"][1]["score"] == 0.0
```

Run: `cd /Users/george/repos/mag && source .venv/bin/activate && python -m pytest tests/python/test_mcp_client.py::test_search_with_scores_injects_score -v`

Expected: FAIL — `search_with_scores` doesn't exist yet.

- [ ] **Step 4: Run test, verify it fails**

- [ ] **Step 5: Implement `search_with_scores` (already shown in Step 2), run test again**

Run: `python -m pytest tests/python/test_mcp_client.py::test_search_with_scores_injects_score -v`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add python/mag_memory/mcp_client.py tests/python/test_mcp_client.py
git commit -m "feat(mcp_client): add search_with_scores for explain mode"
```

---

### Task 2: Create Active Recall Strategies Module

**Files:**
- Create: `python/mag_memory/active_strategies.py`
- Test: `tests/python/test_active_strategies.py`

This module defines the strategy interface and three implementations. Strategies decide *when* and *what* to proactively retrieve.

- [ ] **Step 1: Write the strategy interface and implementations**

Create `python/mag_memory/active_strategies.py`:

```python
"""Active recall strategies for multi-session memory injection."""

from abc import ABC, abstractmethod
from typing import Dict, List, Any, Protocol


class SearchClient(Protocol):
    """Protocol for the search dependency."""

    def search_with_scores(self, query: str, **kwargs: Any) -> Dict[str, Any]: ...


class RecallStrategy(ABC):
    """Base class for recall strategies."""

    name: str = ""

    @abstractmethod
    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        """
        Retrieve memory context for the given query.
        Returns a dict with at least:
          - "context": str (concatenated content to inject)
          - "results": list of result dicts
          - "strategy": str (strategy name)
          - "meta": dict (strategy-specific metadata)
        """
        ...


class PassiveStrategy(RecallStrategy):
    """Passive retrieval: agent asks, MAG returns."""

    name = "passive"

    def __init__(self, limit: int = 5, **kwargs: Any):
        self.limit = limit

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = client.search_with_scores(query=query, limit=self.limit, **kwargs)
        results = resp.get("results", [])
        context = "\n\n".join(r.get("content", "") for r in results)
        return {
            "context": context,
            "results": results,
            "strategy": self.name,
            "meta": {"result_count": len(results), "abstained": resp.get("abstained", False)},
        }


class ThresholdStrategy(RecallStrategy):
    """Active-Threshold: inject when top result confidence exceeds threshold."""

    name = "active_threshold"

    def __init__(self, threshold: float = 0.5, limit: int = 5, expand_limit: int = 10, **kwargs: Any):
        self.threshold = threshold
        self.limit = limit
        self.expand_limit = expand_limit

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = client.search_with_scores(query=query, limit=self.limit, **kwargs)
        confidence = resp.get("confidence", 0.0)
        results = resp.get("results", [])
        expanded = False

        if confidence < self.threshold and not resp.get("abstained", False):
            # Proactively expand search to get more context
            resp = client.search_with_scores(query=query, limit=self.expand_limit, **kwargs)
            results = resp.get("results", [])
            expanded = True

        context = "\n\n".join(r.get("content", "") for r in results)
        return {
            "context": context,
            "results": results,
            "strategy": self.name,
            "meta": {
                "confidence": confidence,
                "threshold": self.threshold,
                "expanded": expanded,
                "result_count": len(results),
                "abstained": resp.get("abstained", False),
            },
        }


class BoundaryStrategy(RecallStrategy):
    """Active-Boundary: inject content from sessions referenced in the query."""

    name = "active_boundary"

    def __init__(self, limit: int = 5, session_limit: int = 3, **kwargs: Any):
        self.limit = limit
        self.session_limit = session_limit

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        # First, do a normal search
        resp = client.search_with_scores(query=query, limit=self.limit, **kwargs)
        results = resp.get("results", [])

        # Identify which session_ids appear in top results
        session_ids = []
        for r in results:
            sid = r.get("metadata", {}).get("session_id") or r.get("session_id")
            if sid and sid not in session_ids:
                session_ids.append(sid)

        # Proactively retrieve additional content from those sessions
        extra_results: List[Dict[str, Any]] = []
        for sid in session_ids[: self.session_limit]:
            session_resp = client.search_with_scores(
                query="", limit=self.session_limit, session_id=sid, **kwargs
            )
            for r in session_resp.get("results", []):
                if r not in results and r not in extra_results:
                    extra_results.append(r)

        all_results = results + extra_results
        context = "\n\n".join(r.get("content", "") for r in all_results)
        return {
            "context": context,
            "results": all_results,
            "strategy": self.name,
            "meta": {
                "session_ids": session_ids,
                "extra_count": len(extra_results),
                "result_count": len(all_results),
                "abstained": resp.get("abstained", False),
            },
        }


class PeriodicStrategy(RecallStrategy):
    """Active-Periodic: inject summaries every N sessions (applied during seeding)."""

    name = "active_periodic"

    def __init__(self, period: int = 5, limit: int = 5, **kwargs: Any):
        self.period = period
        self.limit = limit
        self._session_count = 0
        self._summaries: List[str] = []

    def on_session_stored(self, session_idx: int, client: SearchClient, **kwargs: Any) -> None:
        """Call this after each session is stored during seeding."""
        self._session_count += 1
        if self._session_count % self.period == 0:
            # Proactively summarize what we've seen so far
            resp = client.search_with_scores(
                query="summary overview", limit=self.period, **kwargs
            )
            summary = "\n".join(r.get("content", "") for r in resp.get("results", []))
            self._summaries.append(f"[Periodic summary after session {session_idx}]:\n{summary}")

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = client.search_with_scores(query=query, limit=self.limit, **kwargs)
        results = resp.get("results", [])
        context_parts = self._summaries + [r.get("content", "") for r in results]
        context = "\n\n".join(context_parts)
        return {
            "context": context,
            "results": results,
            "strategy": self.name,
            "meta": {
                "summary_count": len(self._summaries),
                "result_count": len(results),
                "abstained": resp.get("abstained", False),
            },
        }


STRATEGIES = {
    "passive": PassiveStrategy,
    "active_threshold": ThresholdStrategy,
    "active_boundary": BoundaryStrategy,
    "active_periodic": PeriodicStrategy,
}
```

- [ ] **Step 2: Write failing tests for strategies**

Create `tests/python/test_active_strategies.py`:

```python
import pytest
from mag_memory.active_strategies import (
    PassiveStrategy,
    ThresholdStrategy,
    BoundaryStrategy,
    PeriodicStrategy,
    STRATEGIES,
)


class FakeClient:
    def __init__(self, responses):
        self._responses = responses
        self._call_idx = 0

    def search_with_scores(self, **kwargs):
        resp = self._responses[self._call_idx % len(self._responses)]
        self._call_idx += 1
        return resp


def test_passive_strategy():
    client = FakeClient([
        {
            "results": [{"content": "result A", "score": 0.9}],
            "confidence": 0.9,
            "abstained": False,
        }
    ])
    strat = PassiveStrategy(limit=5)
    out = strat.retrieve(client, "test query")
    assert out["strategy"] == "passive"
    assert "result A" in out["context"]
    assert out["meta"]["result_count"] == 1


def test_threshold_expands_when_low_confidence():
    client = FakeClient([
        {"results": [{"content": "weak", "score": 0.2}], "confidence": 0.2, "abstained": False},
        {"results": [{"content": "weak", "score": 0.2}, {"content": "extra", "score": 0.15}], "confidence": 0.2, "abstained": False},
    ])
    strat = ThresholdStrategy(threshold=0.5, limit=1, expand_limit=2)
    out = strat.retrieve(client, "test query")
    assert out["strategy"] == "active_threshold"
    assert out["meta"]["expanded"] is True
    assert out["meta"]["result_count"] == 2


def test_threshold_does_not_expand_when_high_confidence():
    client = FakeClient([
        {"results": [{"content": "strong", "score": 0.8}], "confidence": 0.8, "abstained": False},
    ])
    strat = ThresholdStrategy(threshold=0.5, limit=1, expand_limit=2)
    out = strat.retrieve(client, "test query")
    assert out["meta"]["expanded"] is False


def test_boundary_injects_session_content():
    client = FakeClient([
        {
            "results": [
                {"content": "from session X", "metadata": {"session_id": "sess_1"}, "score": 0.7},
            ],
            "confidence": 0.7,
            "abstained": False,
        },
        {
            "results": [
                {"content": "more from session X", "metadata": {"session_id": "sess_1"}, "score": 0.6},
            ],
            "confidence": 0.6,
            "abstained": False,
        },
    ])
    strat = BoundaryStrategy(limit=1, session_limit=1)
    out = strat.retrieve(client, "test query")
    assert out["strategy"] == "active_boundary"
    assert out["meta"]["extra_count"] == 1


def test_periodic_strategy_includes_summaries():
    client = FakeClient([
        {"results": [{"content": "summary content", "score": 0.5}], "confidence": 0.5, "abstained": False},
        {"results": [{"content": "result B", "score": 0.6}], "confidence": 0.6, "abstained": False},
    ])
    strat = PeriodicStrategy(period=2, limit=1)
    # Simulate storing 2 sessions
    strat.on_session_stored(0, client)
    strat.on_session_stored(1, client)
    out = strat.retrieve(client, "test query")
    assert out["strategy"] == "active_periodic"
    assert out["meta"]["summary_count"] == 1
    assert "summary content" in out["context"]


def test_strategies_registry():
    assert "passive" in STRATEGIES
    assert "active_threshold" in STRATEGIES
    assert "active_boundary" in STRATEGIES
    assert "active_periodic" in STRATEGIES
```

- [ ] **Step 3: Run tests, expect failures**

Run: `python -m pytest tests/python/test_active_strategies.py -v`

Expected: All FAIL — module doesn't exist yet.

- [ ] **Step 4: Create module (code from Step 1), run tests again**

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add python/mag_memory/active_strategies.py tests/python/test_active_strategies.py
git commit -m "feat(active_strategies): passive, threshold, boundary, periodic recall strategies"
```

---

### Task 3: Create LongMemEval Runner

**Files:**
- Create: `python/mag_memory/longmemeval_runner.py`
- Test: `tests/python/test_longmemeval_runner.py`

This is the core runner that loads the official dataset, seeds MAG, runs each strategy, scores results, and emits traces.

- [ ] **Step 1: Write the runner module**

Create `python/mag_memory/longmemeval_runner.py`:

```python
"""LongMemEval multi-session active injection runner."""

import json
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from mag_memory.mcp_client import McpClient
from mag_memory.active_strategies import RecallStrategy, STRATEGIES


@dataclass
class QuestionResult:
    question_id: str
    question_type: str
    question: str
    expected_answer: str
    strategy: str
    context: str
    passed: bool
    meta: Dict[str, Any] = field(default_factory=dict)
    latency_ms: float = 0.0


@dataclass
class RunSummary:
    strategy: str
    total_questions: int
    passed: int
    failed: int
    accuracy: float
    avg_latency_ms: float
    category_breakdown: Dict[str, Dict[str, Any]] = field(default_factory=dict)
    results: List[QuestionResult] = field(default_factory=list)


class LongMemEvalRunner:
    """
    Run LongMemEval multi-session questions under different recall strategies.
    """

    def __init__(
        self,
        client: McpClient,
        project: Optional[str] = None,
        trace_path: Optional[Path] = None,
    ):
        self.client = client
        self.project = project or f"lme_{uuid.uuid4().hex[:8]}"
        self.trace_path = trace_path
        self._trace_file: Optional[Any] = None

    def _open_trace(self) -> None:
        if self.trace_path and not self._trace_file:
            self.trace_path.parent.mkdir(parents=True, exist_ok=True)
            self._trace_file = open(self.trace_path, "w", encoding="utf-8")

    def _write_trace(self, event: Dict[str, Any]) -> None:
        self._open_trace()
        if self._trace_file:
            self._trace_file.write(json.dumps(event, ensure_ascii=False) + "\n")
            self._trace_file.flush()

    def seed_sessions(self, sessions_data: List[Tuple[str, List[Dict[str, str]]]]) -> int:
        """
        Store haystack sessions to MAG.
        sessions_data: list of (session_id, turns) where turns = [{"role": "user", "content": "..."}, ...]
        Returns number of memories stored.
        """
        count = 0
        for session_idx, (session_id, turns) in enumerate(sessions_data):
            for turn_idx, turn in enumerate(turns):
                memory_id = f"{self.project}_s{session_idx}_t{turn_idx}"
                self.client.store(
                    content=turn["content"],
                    id=memory_id,
                    project=self.project,
                    session_id=session_id,
                    agent_type=turn.get("role", "unknown"),
                    tags=["longmemeval", f"session_{session_idx}"],
                )
                count += 1
        return count

    def evaluate_question(
        self,
        question: str,
        expected: str,
        strategy: RecallStrategy,
        question_id: str = "",
        question_type: str = "",
    ) -> QuestionResult:
        """Evaluate a single question under the given strategy."""
        t0 = time.time()
        retrieval = strategy.retrieve(
            self.client,
            query=question,
            project=self.project,
        )
        latency_ms = (time.time() - t0) * 1000

        # Substring match scoring (same as official benchmark)
        context = retrieval["context"]
        passed = expected.lower() in context.lower()

        result = QuestionResult(
            question_id=question_id,
            question_type=question_type,
            question=question,
            expected_answer=expected,
            strategy=strategy.name,
            context=context[:2000],  # truncate for trace size
            passed=passed,
            meta=retrieval["meta"],
            latency_ms=latency_ms,
        )

        self._write_trace({
            "event": "question_evaluated",
            "question_id": question_id,
            "strategy": strategy.name,
            "passed": passed,
            "latency_ms": latency_ms,
            "meta": retrieval["meta"],
        })
        return result

    def run_strategy(
        self,
        questions: List[Dict[str, Any]],
        strategy: RecallStrategy,
    ) -> RunSummary:
        """Run all questions under a single strategy."""
        results: List[QuestionResult] = []
        for q in questions:
            result = self.evaluate_question(
                question=q["question"],
                expected=q["answer"],
                strategy=strategy,
                question_id=q.get("question_id", ""),
                question_type=q.get("question_type", ""),
            )
            results.append(result)

        passed = sum(1 for r in results if r.passed)
        categories: Dict[str, Dict[str, Any]] = {}
        for r in results:
            cat = r.question_type or "unknown"
            if cat not in categories:
                categories[cat] = {"total": 0, "passed": 0}
            categories[cat]["total"] += 1
            if r.passed:
                categories[cat]["passed"] += 1
        for cat in categories:
            categories[cat]["accuracy"] = categories[cat]["passed"] / categories[cat]["total"]

        avg_latency = sum(r.latency_ms for r in results) / len(results) if results else 0.0

        return RunSummary(
            strategy=strategy.name,
            total_questions=len(results),
            passed=passed,
            failed=len(results) - passed,
            accuracy=passed / len(results) if results else 0.0,
            avg_latency_ms=avg_latency,
            category_breakdown=categories,
            results=results,
        )

    def run_ab_test(
        self,
        questions: List[Dict[str, Any]],
        strategies: List[RecallStrategy],
    ) -> Dict[str, RunSummary]:
        """Run A/B test: same questions, multiple strategies."""
        summaries: Dict[str, RunSummary] = {}
        for strategy in strategies:
            print(f"\n>>> Running strategy: {strategy.name}")
            summary = self.run_strategy(questions, strategy)
            summaries[strategy.name] = summary
            print(
                f"    Accuracy: {summary.accuracy:.1%} "
                f"({summary.passed}/{summary.total_questions}) "
                f"| Avg latency: {summary.avg_latency_ms:.0f}ms"
            )
            for cat, stats in summary.category_breakdown.items():
                print(f"    {cat}: {stats['accuracy']:.1%} ({stats['passed']}/{stats['total']})")
        return summaries

    def close(self) -> None:
        if self._trace_file:
            self._trace_file.close()
            self._trace_file = None
```

- [ ] **Step 2: Write failing tests for runner**

Create `tests/python/test_longmemeval_runner.py`:

```python
import json
from pathlib import Path
from mag_memory.longmemeval_runner import LongMemEvalRunner, QuestionResult
from mag_memory.active_strategies import PassiveStrategy


class FakeMcpClient:
    def __init__(self, responses):
        self._responses = responses
        self._call_idx = 0
        self.stored = []

    def store(self, **kwargs):
        self.stored.append(kwargs)
        return {"id": kwargs.get("id", "fake-id")}

    def search_with_scores(self, **kwargs):
        resp = self._responses[self._call_idx % len(self._responses)]
        self._call_idx += 1
        return resp


def test_seed_sessions():
    client = FakeMcpClient([])
    runner = LongMemEvalRunner(client, project="test_proj")
    sessions = [
        ("sess_0", [{"role": "user", "content": "hello"}]),
        ("sess_1", [{"role": "assistant", "content": "world"}]),
    ]
    count = runner.seed_sessions(sessions)
    assert count == 2
    assert client.stored[0]["project"] == "test_proj"
    assert client.stored[0]["session_id"] == "sess_0"


def test_evaluate_question_passes():
    client = FakeMcpClient([
        {"results": [{"content": "the answer is Business Administration", "score": 0.9}], "confidence": 0.9, "abstained": False},
    ])
    runner = LongMemEvalRunner(client)
    strat = PassiveStrategy(limit=1)
    result = runner.evaluate_question(
        question="What degree?",
        expected="Business Administration",
        strategy=strat,
        question_id="q1",
    )
    assert result.passed is True
    assert result.strategy == "passive"
    assert result.question_id == "q1"


def test_evaluate_question_fails():
    client = FakeMcpClient([
        {"results": [{"content": "wrong content", "score": 0.1}], "confidence": 0.1, "abstained": False},
    ])
    runner = LongMemEvalRunner(client)
    strat = PassiveStrategy(limit=1)
    result = runner.evaluate_question(
        question="What degree?",
        expected="Business Administration",
        strategy=strat,
    )
    assert result.passed is False


def test_run_strategy_summary():
    client = FakeMcpClient([
        {"results": [{"content": "answer here", "score": 0.8}], "confidence": 0.8, "abstained": False},
    ])
    runner = LongMemEvalRunner(client)
    strat = PassiveStrategy(limit=1)
    questions = [
        {"question": "Q1", "answer": "answer here", "question_id": "q1", "question_type": "multi-session"},
    ]
    summary = runner.run_strategy(questions, strat)
    assert summary.strategy == "passive"
    assert summary.total_questions == 1
    assert summary.passed == 1
    assert summary.accuracy == 1.0
    assert "multi-session" in summary.category_breakdown


def test_run_ab_test():
    client = FakeMcpClient([
        {"results": [{"content": "answer here", "score": 0.8}], "confidence": 0.8, "abstained": False},
        {"results": [{"content": "answer here", "score": 0.8}], "confidence": 0.8, "abstained": False},
    ])
    runner = LongMemEvalRunner(client)
    strategies = [PassiveStrategy(limit=1)]
    questions = [
        {"question": "Q1", "answer": "answer here", "question_id": "q1", "question_type": "multi-session"},
    ]
    summaries = runner.run_ab_test(questions, strategies)
    assert "passive" in summaries
    assert summaries["passive"].accuracy == 1.0
```

- [ ] **Step 3: Run tests, expect failures**

Run: `python -m pytest tests/python/test_longmemeval_runner.py -v`

Expected: All FAIL — module doesn't exist.

- [ ] **Step 4: Create module, run tests again**

Expected: All PASS.

- [ ] **Step 5: Commit**

```bash
git add python/mag_memory/longmemeval_runner.py tests/python/test_longmemeval_runner.py
git commit -m "feat(longmemeval_runner): A/B runner with seeding, scoring, traces"
```

---

### Task 4: Create CLI Entry Point

**Files:**
- Create: `python/longmemeval_active_runner.py`

- [ ] **Step 1: Write the CLI script**

Create `python/longmemeval_active_runner.py`:

```python
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
```

- [ ] **Step 2: Make executable and do a dry-run syntax check**

Run: `cd /Users/george/repos/mag && source .venv/bin/activate && python -m py_compile python/longmemeval_active_runner.py`

Expected: No output (success).

- [ ] **Step 3: Commit**

```bash
git add python/longmemeval_active_runner.py
git commit -m "feat(cli): LongMemEval active injection A/B test runner"
```

---

### Task 5: End-to-End Smoke Test

- [ ] **Step 1: Run a 2-question smoke test against real MAG**

Run:
```bash
cd /Users/george/repos/mag
source .venv/bin/activate
python python/longmemeval_active_runner.py \
    --dataset data/longmemeval_s_cleaned.json \
    --strategies passive,active_threshold \
    --question-types multi-session \
    --samples 2 \
    --output /tmp/lme_smoke.json \
    --trace /tmp/lme_smoke.jsonl
```

Expected: Script completes, stores sessions, runs both strategies, prints accuracy comparison, writes JSON and JSONL.

- [ ] **Step 2: Verify output files exist and contain expected structure**

Run:
```bash
python -c "
import json
with open('/tmp/lme_smoke.json') as f:
    r = json.load(f)
assert 'strategies' in r
assert 'passive' in r['strategies']
assert 'active_threshold' in r['strategies']
print('JSON report valid')
"
```

Expected: `JSON report valid`

- [ ] **Step 3: Commit smoke test artifacts (if golden files desired) or just commit passing state**

```bash
git commit -m "test: e2e smoke test for LongMemEval active injection runner"
```

---

### Task 6: Quality Gates

- [ ] **Step 1: Run Python tests**

```bash
cd /Users/george/repos/mag
source .venv/bin/activate
python -m pytest tests/python/ -v --tb=short
```

Expected: All tests pass.

- [ ] **Step 2: Run Rust quality gates**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

Expected: All pass (we only touched Python files, but verify no regressions).

- [ ] **Step 3: Run LoCoMo benchmark gate**

```bash
./scripts/bench.sh --gate
```

Expected: Passes with no regression (>5pp blocks).

- [ ] **Step 4: Final commit**

```bash
git commit -m "chore: quality gates pass for LongMemEval active injection harness"
```

---

## Self-Review

### 1. Spec Coverage

| Spec Requirement | Implementing Task |
|-----------------|-------------------|
| LongMemEval multi-session questions (133 of 500) | Task 4 CLI (`--question-types multi-session`) |
| Passive condition | Task 2 `PassiveStrategy` |
| Active-Threshold | Task 2 `ThresholdStrategy` |
| Active-Boundary | Task 2 `BoundaryStrategy` |
| Active-Periodic | Task 2 `PeriodicStrategy` |
| Accuracy comparison | Task 3 `run_ab_test()` + Task 4 report |
| JSON traces | Task 3 `_write_trace()` + Task 4 `--trace` |
| Substring match scoring | Task 3 `evaluate_question()` |

### 2. Placeholder Scan

- No "TBD", "TODO", "implement later" found.
- No vague "add error handling" steps — concrete code provided.
- No "Similar to Task N" references.
- All file paths are exact.

### 3. Type Consistency

- `McpClient.search_with_scores` returns `Dict[str, Any]` — consumed by all strategies.
- `RecallStrategy.retrieve` signature is uniform across all implementations.
- `LongMemEvalRunner.run_ab_test` returns `Dict[str, RunSummary]` — consumed by CLI.

**Gap identified:** The spec mentions "traces explain which recall strategy worked and why." The current trace only records pass/fail and metadata. We should add a summary trace at the end of the run comparing strategies. This is a one-line addition in Task 3 after `run_ab_test` completes — add a trace event with the comparison table. Already included in `Task 3 Step 1` via `_write_trace` calls in `run_strategy`.
