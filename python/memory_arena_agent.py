#!/usr/bin/env python3
"""
MemoryArena Progressive Web Search agent with MAG memory.

Follows the Mem0 benchmark pattern:
1. Ingest: agent runs through tasks, storing observations in MAG
2. Search/Evaluate: score answers against ground truth

Three conditions:
- no_memory: agent uses web search only, no memory between subtasks
- passive: agent can query MAG for relevant context before each subtask
- active: MAG automatically injects context at subtask boundaries

Usage::

    python python/memory_arena_agent.py \
        --samples 5 \
        --conditions no_memory,passive,active \
        --output results/memory_arena_agent.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

sys.path.insert(0, str(Path(__file__).parent))

from mag_memory.mcp_client import McpClient

try:
    from openai import OpenAI
    from datasets import load_dataset
    from duckduckgo_search import DDGS

    _HAS_DEPS = True
except ImportError:
    _HAS_DEPS = False


# ---------------------------------------------------------------------------
# Web search
# ---------------------------------------------------------------------------

def web_search(query: str, max_results: int = 5) -> List[Dict[str, str]]:
    """Search the web via DuckDuckGo. Returns list of {title, href, body}."""
    try:
        with DDGS() as ddgs:
            results = ddgs.text(query, max_results=max_results)
            return [
                {"title": r["title"], "href": r["href"], "body": r["body"]}
                for r in results
            ]
    except Exception as exc:
        print(f"  [search warning] {exc}", file=sys.stderr)
        return []


# ---------------------------------------------------------------------------
# LLM
# ---------------------------------------------------------------------------

@dataclass
class LlmConfig:
    model: str = "gpt-5.4"
    api_key: Optional[str] = None
    base_url: str = "https://api.openai.com/v1"
    max_completion_tokens: int = 1024
    temperature: float = 0.0


class LlmClient:
    def __init__(self, cfg: LlmConfig):
        self.cfg = cfg
        self._client = OpenAI(api_key=cfg.api_key, base_url=cfg.base_url)

    def generate_search_query(
        self, subtask: str, memory_context: str
    ) -> Tuple[str, float]:
        """Generate a web search query for the subtask."""
        system_msg = (
            "You are a research assistant. Given a question and any relevant "
            "context from prior research, generate a concise web search query "
            "that will help find the answer. Output ONLY the search query, "
            "nothing else."
        )
        user_parts = [f"Question: {subtask}"]
        if memory_context:
            user_parts.append(f"Prior context:\n{memory_context}")
        user_parts.append("Search query:")
        user_msg = "\n\n".join(user_parts)

        t0 = time.time()
        response = self._client.chat.completions.create(
            model=self.cfg.model,
            messages=[
                {"role": "system", "content": system_msg},
                {"role": "user", "content": user_msg},
            ],
            max_completion_tokens=100,
            temperature=0.0,
        )
        latency_ms = (time.time() - t0) * 1000
        query = (response.choices[0].message.content or "").strip().strip('"')
        return query, latency_ms

    def answer_from_search(
        self, subtask: str, search_results: List[Dict[str, str]], memory_context: str
    ) -> Tuple[str, float]:
        """Generate an answer from search results."""
        system_msg = (
            "You are a precise research assistant. Answer the user's question "
            "using ONLY the provided search results and prior context. "
            "If the information is insufficient, say 'I don't know'. "
            "Be concise but complete."
        )

        context_parts = []
        if memory_context:
            context_parts.append(f"Prior context from memory:\n{memory_context}")

        if search_results:
            search_text = "\n\n".join(
                f"Result {i+1}:\n{r['body'][:500]}"
                for i, r in enumerate(search_results[:5])
            )
            context_parts.append(f"Search results:\n{search_text}")

        user_parts = context_parts + [f"Question: {subtask}", "Answer:"]
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
        answer = (response.choices[0].message.content or "").strip()
        return answer, latency_ms


# ---------------------------------------------------------------------------
# Memory backends
# ---------------------------------------------------------------------------

@dataclass
class SubtaskObservation:
    subtask_idx: int
    question: str
    search_query: str
    search_results: List[Dict[str, str]]
    answer: str


class NoMemory:
    def observe(self, obs: SubtaskObservation) -> None:
        pass

    def retrieve(self, question: str) -> str:
        return ""

    def close(self) -> None:
        pass


class PassiveMemory:
    """Agent decides when to retrieve. We simulate by always retrieving."""

    def __init__(self, client: McpClient):
        self.client = client
        self.project = f"arena_passive_{uuid.uuid4().hex[:8]}"

    def observe(self, obs: SubtaskObservation) -> None:
        results_text = "\n".join(
            f"- {r['title']}: {r['body'][:200]}" for r in obs.search_results
        )
        content = (
            f"Subtask {obs.subtask_idx}\n"
            f"Question: {obs.question}\n"
            f"Search query: {obs.search_query}\n"
            f"Search results:\n{results_text}\n"
            f"Answer: {obs.answer}"
        )
        self.client.store(
            content=content,
            tags=["arena", f"subtask_{obs.subtask_idx}"],
            project=self.project,
            metadata={
                "subtask_idx": obs.subtask_idx,
                "question": obs.question,
            },
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
    """Memory is automatically retrieved and injected at subtask boundaries."""

    def __init__(self, client: McpClient):
        self.client = client
        self.project = f"arena_active_{uuid.uuid4().hex[:8]}"

    def observe(self, obs: SubtaskObservation) -> None:
        results_text = "\n".join(
            f"- {r['title']}: {r['body'][:200]}" for r in obs.search_results
        )
        content = (
            f"Subtask {obs.subtask_idx}\n"
            f"Question: {obs.question}\n"
            f"Search query: {obs.search_query}\n"
            f"Search results:\n{results_text}\n"
            f"Answer: {obs.answer}"
        )
        self.client.store(
            content=content,
            tags=["arena", f"subtask_{obs.subtask_idx}"],
            project=self.project,
            metadata={
                "subtask_idx": obs.subtask_idx,
                "question": obs.question,
            },
        )

    def retrieve(self, question: str) -> str:
        # Same retrieval as passive — difference is WHO calls it
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
        return "\n\n".join(r.get("content", "") for r in results[:3])

    def close(self) -> None:
        pass


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
# Task runner
# ---------------------------------------------------------------------------

@dataclass
class SubtaskResult:
    subtask_idx: int
    question: str
    reference: str
    prediction: str
    search_query: str
    exact_match: float
    word_overlap_f1: float
    llm_ms: float
    search_ms: float


@dataclass
class TaskResult:
    task_id: int
    condition: str
    subtasks: List[SubtaskResult]


def run_task(
    task: Dict[str, Any],
    condition: str,
    memory: Any,
    llm: LlmClient,
) -> TaskResult:
    subtasks: List[SubtaskResult] = []
    questions: List[str] = task["questions"]
    references: List[str] = task["answers"]

    for idx, (question, reference) in enumerate(zip(questions, references)):
        # Retrieve memory
        mem_context = ""
        if condition != "no_memory":
            mem_context = memory.retrieve(question)

        # Generate search query
        search_query, query_ms = llm.generate_search_query(question, mem_context)

        # Execute web search
        t_search = time.time()
        search_results = web_search(search_query, max_results=5)
        search_ms = (time.time() - t_search) * 1000

        # Generate answer
        prediction, answer_ms = llm.answer_from_search(
            question, search_results, mem_context
        )

        em = exact_match(prediction, reference)
        f1 = word_overlap_f1(prediction, reference)

        subtasks.append(
            SubtaskResult(
                subtask_idx=idx,
                question=question,
                reference=reference,
                prediction=prediction,
                search_query=search_query,
                exact_match=em,
                word_overlap_f1=f1,
                llm_ms=query_ms + answer_ms,
                search_ms=search_ms,
            )
        )

        # Store observation
        memory.observe(
            SubtaskObservation(
                subtask_idx=idx,
                question=question,
                search_query=search_query,
                search_results=search_results,
                answer=prediction,
            )
        )

    return TaskResult(task_id=task["id"], condition=condition, subtasks=subtasks)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main() -> int:
    parser = argparse.ArgumentParser(description="MemoryArena agent harness")
    parser.add_argument("--samples", type=int, default=None)
    parser.add_argument(
        "--conditions",
        type=str,
        default="no_memory,passive,active",
        help="Comma-separated: no_memory,passive,active",
    )
    parser.add_argument("--output", type=str, default="results/memory_arena_agent.json")
    parser.add_argument("--llm-model", type=str, default="gpt-5.4")
    parser.add_argument("--llm-url", type=str, default="https://api.openai.com/v1")
    parser.add_argument("--max-completion-tokens", type=int, default=1024)
    parser.add_argument("--home-dir", type=str, default=None)
    parser.add_argument("--skip-search", action="store_true", help="Skip web search (for testing)")
    args = parser.parse_args()

    if not _HAS_DEPS:
        print("ERROR: pip install openai datasets duckduckgo-search", file=sys.stderr)
        return 1

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

    # Monkey-patch web_search if --skip-search
    if args.skip_search:
        global web_search
        web_search = lambda query, max_results=5: []
        print("WARNING: Web search disabled (--skip-search)")

    all_results: List[Dict[str, Any]] = []

    for condition in conditions:
        print(f"\n{'='*60}")
        print(f"Condition: {condition}")
        print(f"{'='*60}")

        condition_results: List[Dict[str, Any]] = []

        for i, task in enumerate(tasks, 1):
            mcp = McpClient(home_dir=args.home_dir)
            mcp.start()

            if condition == "no_memory":
                memory: Any = NoMemory()
            elif condition == "passive":
                memory = PassiveMemory(mcp)
            elif condition == "active":
                memory = ActiveMemory(mcp)
            else:
                print(f"ERROR: unknown condition {condition}", file=sys.stderr)
                return 1

            result = run_task(task, condition, memory, llm)

            condition_results.append(
                {
                    "task_id": result.task_id,
                    "subtasks": [
                        {
                            "subtask_idx": s.subtask_idx,
                            "question": s.question,
                            "reference": s.reference,
                            "prediction": s.prediction,
                            "search_query": s.search_query,
                            "exact_match": s.exact_match,
                            "word_overlap_f1": s.word_overlap_f1,
                            "llm_ms": s.llm_ms,
                            "search_ms": s.search_ms,
                        }
                        for s in result.subtasks
                    ],
                }
            )

            memory.close()
            mcp.stop()

            avg_f1 = sum(s.word_overlap_f1 for s in result.subtasks) / len(
                result.subtasks
            )
            print(
                f"  [{i}/{len(tasks)}] Task {result.task_id} — "
                f"subtasks={len(result.subtasks)} avg_f1={avg_f1:.2f}"
            )

        # Aggregate
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
