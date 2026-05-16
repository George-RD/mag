"""LongMemEval multi-session active injection runner."""

import json
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, TextIO, Tuple

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
        self._trace_file: Optional[TextIO] = None

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
        Store haystack sessions to MAG via batch store.
        sessions_data: list of (session_id, turns) where turns = [{"role": "user", "content": "..."}, ...]
        Returns number of memories stored.
        """
        batch: List[Dict[str, Any]] = []
        for session_idx, (session_id, turns) in enumerate(sessions_data):
            for turn_idx, turn in enumerate(turns):
                memory_id = f"{self.project}_s{session_idx}_t{turn_idx}"
                batch.append({
                    "content": turn["content"],
                    "id": memory_id,
                    "project": self.project,
                    "session_id": session_id,
                    "agent_type": turn.get("role", "unknown"),
                    "tags": ["longmemeval", f"session_{session_idx}"],
                })
        if batch:
            self.client.store_batch(batch)
        return len(batch)

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
        context = retrieval.get("context", "")
        expected_str = str(expected) if expected is not None else ""
        passed = expected_str.lower() in context.lower() if context else False

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

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()
        return False
