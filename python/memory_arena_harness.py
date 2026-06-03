#!/usr/bin/env python3
"""
MemoryArena Progressive Web Search harness for MAG.

Tests three conditions head-to-head:
1. No-memory baseline
2. Passive retrieval (agent asks MAG for relevant context)
3. Active injection (MAG automatically surfaces context at subtask boundaries)

Usage::

    python python/memory_arena_harness.py --samples 10 --output results/arena.json

Requires OPENAI_API_KEY (env var or .env.local).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

sys.path.insert(0, str(Path(__file__).parent))

from mag_memory.mcp_client import McpClient

# Optional dependencies
try:
    from openai import OpenAI
    from datasets import load_dataset

    _HAS_DEPS = True
except ImportError:
    _HAS_DEPS = False


# ---------------------------------------------------------------------------
# LLM interface
# ---------------------------------------------------------------------------

@dataclass
class LlmConfig:
    model: str = "gpt-5.4"
    api_key: Optional[str] = None
    base_url: str = "https://api.openai.com/v1"
    max_completion_tokens: int = 512
    temperature: float = 0.0


class LlmClient:
    def __init__(self, cfg: LlmConfig):
        self.cfg = cfg
        self._client = OpenAI(api_key=cfg.api_key, base_url=cfg.base_url)

    def answer(
        self,
        question: str,
        context: str = "",
        system: Optional[str] = None,
    ) -> Tuple[str, float]:
        """Generate answer. Returns (text, latency_ms)."""
        system_msg = system or (
            "You are a helpful research assistant. Answer the question "
            "concisely and factually. If you are unsure, say 'I don't know'."
        )

        user_parts = []
        if context:
            user_parts.append(f"Relevant context from previous research:\n{context}")
        user_parts.append(f"Question: {question}")
        user_msg = "\n\n".join(user_parts)

        t0 = time.time()
        response = self._client.chat.completions.create(
            model=self.cfg.model,
            messages=[
                {"role": "system", "content": system_msg},
                {"role": "user", "content": user_msg},
            ],
            max_completion_tokens=self.cfg.max_completion_tokens,
            temperature=self.cfg.temperature,
        )
        latency_ms = (time.time() - t0) * 1000

        text = (response.choices[0].message.content or "").strip()
        return text, latency_ms


# ---------------------------------------------------------------------------
# Scoring
# ---------------------------------------------------------------------------

def word_overlap_f1(prediction: str, reference: str) -> float:
    import re

    def tokens(text: str) -> set:
        return set(
            re.sub(r"[^\w]", "", t.lower())
            for t in text.split()
            if re.sub(r"[^\w]", "", t.lower())
        )

    pred_tok = tokens(prediction)
    ref_tok = tokens(reference)
    if not pred_tok or not ref_tok:
        return 0.0

    overlap = pred_tok & ref_tok
    precision = len(overlap) / len(pred_tok)
    recall = len(overlap) / len(ref_tok)
    if precision + recall == 0:
        return 0.0
    return 2 * precision * recall / (precision + recall)


def exact_match(prediction: str, reference: str) -> float:
    return 1.0 if prediction.strip().lower() == reference.strip().lower() else 0.0


# ---------------------------------------------------------------------------
# Memory backends
# ---------------------------------------------------------------------------

class NoMemory:
    """Baseline: no memory between subtasks."""

    def observe(self, subtask_idx: int, question: str, answer: str) -> None:
        pass

    def retrieve(self, question: str) -> str:
        return ""

    def close(self) -> None:
        pass


class PassiveMemory:
    """
    Passive retrieval: the agent decides what to search for.

    We simulate this by always searching with the current question,
    but the *agent* could theoretically choose a different query.
    """

    def __init__(self, client: McpClient):
        self.client = client
        self.project = f"arena_passive_{int(time.time())}"

    def observe(self, subtask_idx: int, question: str, answer: str) -> None:
        self.client.store(
            content=f"Subtask {subtask_idx}\nQuestion: {question}\nAnswer: {answer}",
            tags=["arena", f"subtask_{subtask_idx}"],
            project=self.project,
            metadata={"subtask_idx": subtask_idx, "question": question},
        )

    def retrieve(self, question: str) -> str:
        result = self.client.search(
            query=question,
            mode="semantic",
            limit=10,
            advanced=True,
            project=self.project,
        )
        if result.get("abstained") or not result.get("results"):
            # Fallback to plain search
            result = self.client.search(
                query=question,
                mode="semantic",
                limit=10,
                advanced=False,
                project=self.project,
            )
        results = result.get("results", [])
        return "\n\n".join(r.get("content", "") for r in results[:3])

    def close(self) -> None:
        pass


class ActiveMemory:
    """
    Active injection: memory is automatically retrieved based on the
    upcoming question and injected into the prompt.
    """

    def __init__(self, client: McpClient, similarity_threshold: float = 0.5):
        self.client = client
        self.project = f"arena_active_{int(time.time())}"
        self.threshold = similarity_threshold

    def observe(self, subtask_idx: int, question: str, answer: str) -> None:
        self.client.store(
            content=f"Subtask {subtask_idx}\nQuestion: {question}\nAnswer: {answer}",
            tags=["arena", f"subtask_{subtask_idx}"],
            project=self.project,
            metadata={"subtask_idx": subtask_idx, "question": question},
        )

    def retrieve(self, question: str) -> str:
        # Same retrieval logic as passive — the difference is WHO decides
        # to call retrieve(). In active mode, the harness calls it
        # automatically before every subtask.
        result = self.client.search(
            query=question,
            mode="semantic",
            limit=10,
            advanced=True,
            project=self.project,
        )
        if result.get("abstained") or not result.get("results"):
            result = self.client.search(
                query=question,
                mode="semantic",
                limit=10,
                advanced=False,
                project=self.project,
            )
        results = result.get("results", [])

        # Optional: filter by similarity threshold
        # (MCP doesn't expose raw scores easily, so we skip threshold filtering
        # for now and use top-3 results regardless.)
        return "\n\n".join(r.get("content", "") for r in results[:3])

    def close(self) -> None:
        pass


# ---------------------------------------------------------------------------
# Task runner
# ---------------------------------------------------------------------------

@dataclass
class SubtaskResult:
    subtask_idx: int
    question: str
    reference: str
    prediction: str
    context: str
    exact_match: float
    word_overlap_f1: float
    llm_ms: float


@dataclass
class TaskResult:
    task_id: int
    condition: str
    subtasks: List[SubtaskResult]
    total_llm_ms: float


def run_task(
    task: Dict[str, Any],
    condition: str,
    memory: Any,
    llm: LlmClient,
) -> TaskResult:
    """Run a single MemoryArena task under a given condition."""
    subtasks: List[SubtaskResult] = []
    total_llm_ms = 0.0

    questions: List[str] = task["questions"]
    references: List[str] = task["answers"]

    for idx, (question, reference) in enumerate(zip(questions, references)):
        # Retrieve context based on condition
        context = ""
        if condition != "no_memory":
            context = memory.retrieve(question)

        # Generate answer
        prediction, llm_ms = llm.answer(question, context)
        total_llm_ms += llm_ms

        # Score
        em = exact_match(prediction, reference)
        f1 = word_overlap_f1(prediction, reference)

        subtasks.append(
            SubtaskResult(
                subtask_idx=idx,
                question=question,
                reference=reference,
                prediction=prediction,
                context=context,
                exact_match=em,
                word_overlap_f1=f1,
                llm_ms=llm_ms,
            )
        )

        # Store observation for next subtask
        memory.observe(idx, question, prediction)

    return TaskResult(
        task_id=task["id"],
        condition=condition,
        subtasks=subtasks,
        total_llm_ms=total_llm_ms,
    )


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description="MemoryArena harness for MAG")
    parser.add_argument(
        "--samples",
        type=int,
        default=None,
        help="Number of tasks to evaluate (default: all 221)",
    )
    parser.add_argument(
        "--conditions",
        type=str,
        default="no_memory,passive,active",
        help="Comma-separated list of conditions to run",
    )
    parser.add_argument(
        "--output",
        type=str,
        default="results/memory_arena.json",
        help="Output JSON file",
    )
    parser.add_argument(
        "--llm-model",
        type=str,
        default="gpt-5.4",
        help="LLM model",
    )
    parser.add_argument(
        "--llm-url",
        type=str,
        default="https://api.openai.com/v1",
        help="OpenAI-compatible API base",
    )
    parser.add_argument(
        "--max-completion-tokens",
        type=int,
        default=512,
        help="Max tokens per LLM call",
    )
    parser.add_argument(
        "--home-dir",
        type=str,
        default=None,
        help="MAG home directory",
    )
    args = parser.parse_args()

    if not _HAS_DEPS:
        print(
            "ERROR: Missing dependencies. Run:\n"
            "  pip install openai datasets",
            file=sys.stderr,
        )
        return 1

    # Resolve API key
    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key and os.path.exists(".env.local"):
        with open(".env.local") as f:
            for line in f:
                if line.startswith("OPENAI_API_KEY="):
                    api_key = line.strip().split("=", 1)[1]
                    break
    if not api_key:
        print("ERROR: OPENAI_API_KEY not found", file=sys.stderr)
        return 1

    # Load dataset
    print("Loading MemoryArena progressive_search dataset...")
    ds = load_dataset("ZexueHe/memoryarena", "progressive_search", split="test")
    tasks = list(ds)
    if args.samples is not None:
        tasks = tasks[: args.samples]
    print(f"Loaded {len(tasks)} tasks")

    llm = LlmClient(
        LlmConfig(
            model=args.llm_model,
            api_key=api_key,
            base_url=args.llm_url,
            max_completion_tokens=args.max_completion_tokens,
        )
    )

    conditions = [c.strip() for c in args.conditions.split(",")]
    print(f"Conditions: {conditions}")

    all_results: List[Dict[str, Any]] = []

    for condition in conditions:
        print(f"\n{'='*60}")
        print(f"Running condition: {condition}")
        print(f"{'='*60}")

        condition_results: List[Dict[str, Any]] = []

        for i, task in enumerate(tasks, 1):
            # Fresh MAG instance per condition to avoid cross-contamination
            mcp = McpClient(home_dir=args.home_dir)
            mcp.start()

            if condition == "no_memory":
                memory: Any = NoMemory()
            elif condition == "passive":
                memory = PassiveMemory(mcp)
            elif condition == "active":
                memory = ActiveMemory(mcp)
            else:
                print(f"ERROR: Unknown condition: {condition}", file=sys.stderr)
                return 1

            result = run_task(task, condition, memory, llm)

            # Convert to serializable dict
            condition_results.append(
                {
                    "task_id": result.task_id,
                    "subtasks": [
                        {
                            "subtask_idx": s.subtask_idx,
                            "question": s.question,
                            "reference": s.reference,
                            "prediction": s.prediction,
                            "exact_match": s.exact_match,
                            "word_overlap_f1": s.word_overlap_f1,
                            "llm_ms": s.llm_ms,
                        }
                        for s in result.subtasks
                    ],
                    "total_llm_ms": result.total_llm_ms,
                }
            )

            memory.close()
            mcp.stop()

            # Progress
            avg_f1 = sum(s.word_overlap_f1 for s in result.subtasks) / len(
                result.subtasks
            )
            print(
                f"  [{i}/{len(tasks)}] Task {result.task_id} — "
                f"subtasks={len(result.subtasks)} avg_f1={avg_f1:.2f}"
            )

        # Aggregate condition stats
        total_subtasks = sum(len(r["subtasks"]) for r in condition_results)
        total_em = sum(
            s["exact_match"] for r in condition_results for s in r["subtasks"]
        )
        total_f1 = sum(
            s["word_overlap_f1"] for r in condition_results for s in r["subtasks"]
        )

        summary = {
            "condition": condition,
            "tasks_evaluated": len(condition_results),
            "subtasks_evaluated": total_subtasks,
            "exact_match": total_em / total_subtasks if total_subtasks else 0.0,
            "mean_f1": total_f1 / total_subtasks if total_subtasks else 0.0,
            "tasks": condition_results,
        }
        all_results.append(summary)

        print(f"\n{condition} summary:")
        print(f"  Exact match: {summary['exact_match']:.2%}")
        print(f"  Mean F1:     {summary['mean_f1']:.2%}")

    # Write output
    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(
            {
                "timestamp": time.time(),
                "model": args.llm_model,
                "conditions": all_results,
            },
            f,
            indent=2,
            ensure_ascii=False,
        )

    print(f"\nResults written to: {args.output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
