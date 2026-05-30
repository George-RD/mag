#!/usr/bin/env python3
"""
Standalone AMA-Bench runner for MAG (SWE domain only).

Usage::

    python python/ama_bench_runner.py \
        --dataset data/ama_bench/open_end_qa_set.jsonl \
        --domain SOFTWARE \
        --samples 5 \
        --output results/ama_bench_swe.json

Exit codes:
    0 — success
    1 — error
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

# Ensure mag_memory is on path
sys.path.insert(0, str(Path(__file__).parent))

from mag_memory.ama_adapter import MagMethod

# Optional LLM integration
try:
    from openai import OpenAI

    _HAS_OPENAI = True
except ImportError:
    _HAS_OPENAI = False


def load_dataset(path: str, domain: Optional[str] = None) -> List[Dict[str, Any]]:
    """Load AMA-Bench dataset and filter by domain."""
    episodes: List[Dict[str, Any]] = []
    with open(path, "r", encoding="utf-8") as f:
        for line in f:
            ep = json.loads(line)
            if domain is None or ep.get("domain") == domain:
                episodes.append(ep)
    return episodes


def trajectory_to_text(trajectory: List[Dict[str, Any]]) -> str:
    """Convert AMA-Bench trajectory list to text format."""
    parts: List[str] = []
    for step in trajectory:
        turn_idx = step.get("turn_idx", 0)
        action = step.get("action", "")
        observation = step.get("observation", "")
        parts.append(f"Step {turn_idx}:")
        if action:
            parts.append(f"Action: {action}")
        if observation:
            parts.append(f"Observation: {observation}")
        parts.append("")
    return "\n".join(parts)


def word_overlap_score(prediction: str, reference: str) -> float:
    """
    Compute word-overlap F1 between prediction and reference.
    Simple tokenisation: lowercase, split on whitespace, strip punctuation.
    """
    import re

    def tokens(text: str) -> set:
        return set(
            re.sub(r"[^\w]", "", t.lower())
            for t in text.split()
            if re.sub(r"[^\w]", "", t.lower())
        )

    pred_tokens = tokens(prediction)
    ref_tokens = tokens(reference)

    if not pred_tokens or not ref_tokens:
        return 0.0

    overlap = pred_tokens & ref_tokens
    precision = len(overlap) / len(pred_tokens) if pred_tokens else 0.0
    recall = len(overlap) / len(ref_tokens) if ref_tokens else 0.0
    if precision + recall == 0:
        return 0.0
    return 2 * precision * recall / (precision + recall)


def exact_match_score(prediction: str, reference: str) -> float:
    """1.0 if normalised strings match exactly, else 0.0."""
    return 1.0 if prediction.strip().lower() == reference.strip().lower() else 0.0


def generate_answer(
    question: str,
    context: str,
    model: str,
    api_key: Optional[str],
    base_url: str,
    max_tokens: int,
) -> Tuple[str, float]:
    """
    Generate an answer from retrieved context using an LLM.

    Returns (answer_text, latency_ms).
    """
    if not _HAS_OPENAI:
        raise RuntimeError(
            "openai package not installed. Run: pip install openai"
        )

    client = OpenAI(api_key=api_key, base_url=base_url)

    system_msg = (
        "You are a precise assistant. Answer the user's question using ONLY "
        "the provided context. If the context does not contain the answer, "
        "say 'I don't know'. Be concise."
    )

    user_msg = f"Context:\n{context}\n\nQuestion: {question}\n\nAnswer:"

    t0 = time.time()
    response = client.chat.completions.create(
        model=model,
        messages=[
            {"role": "system", "content": system_msg},
            {"role": "user", "content": user_msg},
        ],
        max_completion_tokens=max_tokens,
        temperature=0.0,
    )
    latency_ms = (time.time() - t0) * 1000

    answer = response.choices[0].message.content or ""
    return answer.strip(), latency_ms


def run_episode(
    method: MagMethod,
    episode: Dict[str, Any],
    top_k: int = 5,
    llm_model: Optional[str] = None,
    llm_url: str = "https://api.openai.com/v1",
    llm_api_key: Optional[str] = None,
    max_completion_tokens: int = 256,
) -> Dict[str, Any]:
    """Run memory construction + retrieval for a single episode."""
    episode_id = episode["episode_id"]
    task = episode.get("task", "")
    trajectory = episode.get("trajectory", [])
    qa_pairs = episode.get("qa_pairs", [])

    traj_text = trajectory_to_text(trajectory)

    # Phase 1: memory construction
    t0 = time.time()
    memory = method.memory_construction(traj_text, task=task)
    construction_ms = (time.time() - t0) * 1000

    # Phase 2: retrieval for each question
    results: List[Dict[str, Any]] = []
    total_retrieve_ms = 0.0

    for qa in qa_pairs:
        question = qa["question"]
        reference = qa.get("answer", "")

        t1 = time.time()
        context = method.memory_retrieve(memory, question)
        retrieve_ms = (time.time() - t1) * 1000
        total_retrieve_ms += retrieve_ms

        # Generate answer from context using LLM (if available)
        prediction = context
        llm_ms = 0.0
        if llm_model is not None:
            try:
                prediction, llm_ms = generate_answer(
                    question,
                    context,
                    llm_model,
                    llm_api_key,
                    llm_url,
                    max_completion_tokens,
                )
            except Exception as exc:
                prediction = f"[LLM_ERROR: {exc}]"

        em = exact_match_score(prediction, reference)
        f1 = word_overlap_score(prediction, reference)

        result_entry: Dict[str, Any] = {
            "question": question,
            "reference": reference,
            "prediction": prediction,
            "context_chars": len(context),
            "exact_match": em,
            "word_overlap_f1": f1,
            "retrieve_ms": retrieve_ms,
        }
        if llm_model is not None:
            result_entry["llm_ms"] = llm_ms
        results.append(result_entry)

    return {
        "episode_id": episode_id,
        "domain": episode.get("domain"),
        "task_type": episode.get("task_type"),
        "num_turns": len(trajectory),
        "total_tokens": episode.get("total_tokens"),
        "construction_ms": construction_ms,
        "avg_retrieve_ms": total_retrieve_ms / len(qa_pairs) if qa_pairs else 0,
        "results": results,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="AMA-Bench runner for MAG")
    parser.add_argument(
        "--dataset",
        type=str,
        default="data/ama_bench/open_end_qa_set.jsonl",
        help="Path to AMA-Bench JSONL dataset",
    )
    parser.add_argument(
        "--domain",
        type=str,
        default="SOFTWARE",
        help="Domain filter (e.g. SOFTWARE, WEB, Game)",
    )
    parser.add_argument(
        "--samples",
        type=int,
        default=None,
        help="Number of episodes to evaluate (default: all)",
    )
    parser.add_argument(
        "--top-k",
        type=int,
        default=5,
        help="Number of chunks to retrieve per question",
    )
    parser.add_argument(
        "--chunk-size",
        type=int,
        default=2000,
        help="Trajectory chunk size in characters",
    )
    parser.add_argument(
        "--search-mode",
        type=str,
        default="semantic",
        choices=["text", "semantic"],
        help="Search mode for retrieval",
    )
    parser.add_argument(
        "--search-limit",
        type=int,
        default=20,
        help="Candidate limit before top-k truncation",
    )
    parser.add_argument(
        "--output",
        type=str,
        default="results/ama_bench_mag.json",
        help="Output JSON file for results",
    )
    parser.add_argument(
        "--trace-dir",
        type=str,
        default=None,
        help="Directory for JSONL trace files",
    )
    parser.add_argument(
        "--home-dir",
        type=str,
        default=None,
        help="MAG home directory (default: ~/.mag)",
    )
    parser.add_argument(
        "--mag-binary",
        type=str,
        default=None,
        help="Path to mag binary",
    )
    parser.add_argument(
        "--llm-model",
        type=str,
        default="gpt-5.4",
        help="LLM model for answer generation",
    )
    parser.add_argument(
        "--llm-url",
        type=str,
        default="https://api.openai.com/v1",
        help="OpenAI-compatible API base URL",
    )
    parser.add_argument(
        "--no-llm",
        action="store_true",
        help="Skip LLM answer generation (score raw retrieved context instead)",
    )
    parser.add_argument(
        "--max-completion-tokens",
        type=int,
        default=256,
        help="Max tokens for LLM answer generation",
    )
    args = parser.parse_args()

    if not os.path.exists(args.dataset):
        print(f"ERROR: Dataset not found: {args.dataset}", file=sys.stderr)
        print("Download with:", file=sys.stderr)
        print(
            "  curl -L https://huggingface.co/datasets/AMA-bench/AMA-bench/resolve/main/test/open_end_qa_set.jsonl "
            "-o data/ama_bench/open_end_qa_set.jsonl",
            file=sys.stderr,
        )
        return 1

    episodes = load_dataset(args.dataset, domain=args.domain)
    if args.samples is not None:
        episodes = episodes[: args.samples]

    print(f"Loaded {len(episodes)} episodes (domain={args.domain})")

    method = MagMethod(
        top_k=args.top_k,
        chunk_size=args.chunk_size,
        search_mode=args.search_mode,
        search_limit=args.search_limit,
        trace_dir=args.trace_dir,
        mag_binary=args.mag_binary,
        home_dir=args.home_dir,
    )

    all_episode_results: List[Dict[str, Any]] = []
    total_questions = 0
    total_em = 0.0
    total_f1 = 0.0

    # Resolve LLM config
    use_llm = not args.no_llm
    llm_api_key: Optional[str] = None
    if use_llm:
        if not _HAS_OPENAI:
            print(
                "WARNING: openai not installed; falling back to no-LLM mode.\n"
                "Install with: pip install openai",
                file=sys.stderr,
            )
            use_llm = False
        else:
            llm_api_key = os.environ.get("OPENAI_API_KEY")
            if not llm_api_key and os.path.exists(".env.local"):
                with open(".env.local") as f:
                    for line in f:
                        if line.startswith("OPENAI_API_KEY="):
                            llm_api_key = line.strip().split("=", 1)[1]
                            break
            if not llm_api_key:
                print(
                    "WARNING: OPENAI_API_KEY not found; falling back to no-LLM mode.",
                    file=sys.stderr,
                )
                use_llm = False

    llm_model = args.llm_model if use_llm else None

    try:
        for i, ep in enumerate(episodes, 1):
            print(f"\n[{i}/{len(episodes)}] Episode {ep['episode_id']} — {ep.get('task_type')} ({ep.get('num_turns')} turns)")
            result = run_episode(
                method,
                ep,
                top_k=args.top_k,
                llm_model=llm_model,
                llm_url=args.llm_url,
                llm_api_key=llm_api_key,
                max_completion_tokens=args.max_completion_tokens,
            )
            all_episode_results.append(result)

            ep_questions = len(result["results"])
            ep_em = sum(r["exact_match"] for r in result["results"])
            ep_f1 = sum(r["word_overlap_f1"] for r in result["results"])

            total_questions += ep_questions
            total_em += ep_em
            total_f1 += ep_f1

            print(
                f"  Construction: {result['construction_ms']:.0f}ms | "
                f"Avg retrieve: {result['avg_retrieve_ms']:.0f}ms | "
                f"EM: {ep_em}/{ep_questions} | F1: {ep_f1/ep_questions:.2f}"
            )
    finally:
        method.close()

    # Aggregate summary
    summary = {
        "domain": args.domain,
        "episodes_evaluated": len(all_episode_results),
        "questions_evaluated": total_questions,
        "exact_match": total_em / total_questions if total_questions else 0.0,
        "mean_f1": total_f1 / total_questions if total_questions else 0.0,
        "timestamp": time.time(),
        "config": {
            "top_k": args.top_k,
            "chunk_size": args.chunk_size,
            "search_mode": args.search_mode,
            "search_limit": args.search_limit,
            "llm_model": llm_model,
            "llm_url": args.llm_url if use_llm else None,
        },
        "episodes": all_episode_results,
    }

    os.makedirs(os.path.dirname(args.output) or ".", exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(summary, f, indent=2, ensure_ascii=False)

    print(f"\n{'='*60}")
    print(f"Results written to: {args.output}")
    print(f"Episodes: {summary['episodes_evaluated']} | Questions: {summary['questions_evaluated']}")
    print(f"Exact Match: {summary['exact_match']:.2%}")
    print(f"Mean F1:     {summary['mean_f1']:.2%}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
