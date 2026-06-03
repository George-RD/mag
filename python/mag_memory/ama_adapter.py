"""
MAG adapter for AMA-Bench.

Implements the two-stage protocol via the MCP stdio interface:

1. ``memory_construction(traj_text, task)`` — chunk and store trajectory.
2. ``memory_retrieve(memory, question)`` — search and return context.

Usage with AMA-Bench::

    from mag_memory.ama_adapter import MagMethod

    method = MagMethod(top_k=5)
    memory = method.memory_construction(trajectory_text, task="")
    context = method.memory_retrieve(memory, "What was the first action?")

**Concurrency note:** The adapter uses a single ``mag serve`` subprocess.
Run AMA-Bench with ``--max-concurrency-episodes 1`` to avoid contention.
"""

from __future__ import annotations

import json
import os
import re
import time
import uuid
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

# When running inside the AMA-Bench repo, BaseMethod is at src.method.base_method.
# When running standalone, we define a minimal compatible base class.
try:
    from src.method.base_method import BaseMethod
except ImportError:
    # Standalone fallback — enough duck-typing for manual use.
    from abc import ABC, abstractmethod

    class BaseMethod(ABC):  # type: ignore[no-redef]
        @abstractmethod
        def memory_construction(self, traj_text: str, task: str = "") -> Any:
            pass

        @abstractmethod
        def memory_retrieve(self, memory: Any, question: str) -> str:
            pass

from .mcp_client import McpClient


# ---------------------------------------------------------------------------
# Chunking
# ---------------------------------------------------------------------------

DEFAULT_CHUNK_SIZE = 2_000  # characters per chunk
DEFAULT_CHUNK_OVERLAP = 200


def _chunk_text(text: str, chunk_size: int = DEFAULT_CHUNK_SIZE, overlap: int = DEFAULT_CHUNK_OVERLAP) -> List[str]:
    """
    Split text into overlapping chunks.

    Tries to split on ``Step N:`` or ``Turn N:`` boundaries first;
    falls back to sentence boundaries, then hard character cuts.
    """
    lines = text.splitlines()
    if not lines:
        return []

    # Try step/turn boundaries
    boundaries = [0]
    for i, line in enumerate(lines):
        if re.match(r"^(Step|Turn)\s+\d+", line.strip(), re.IGNORECASE):
            boundaries.append(i)
    boundaries.append(len(lines))

    chunks: List[str] = []
    current: List[str] = []
    current_len = 0

    for b in range(len(boundaries) - 1):
        segment = "\n".join(lines[boundaries[b]:boundaries[b + 1]])
        seg_len = len(segment)

        if current_len + seg_len <= chunk_size:
            current.append(segment)
            current_len += seg_len + 1  # +1 for joining newline
        else:
            if current:
                chunks.append("\n".join(current))
            current = [segment]
            current_len = seg_len

    if current:
        chunks.append("\n".join(current))

    # If any chunk is still too large, hard-split it
    final_chunks: List[str] = []
    for chunk in chunks:
        if len(chunk) <= chunk_size * 1.2:  # allow 20% overshoot for natural boundaries
            final_chunks.append(chunk)
            continue
        start = 0
        while start < len(chunk):
            end = min(start + chunk_size, len(chunk))
            final_chunks.append(chunk[start:end])
            start += chunk_size - overlap

    return final_chunks if final_chunks else [text]


# ---------------------------------------------------------------------------
# Memory object
# ---------------------------------------------------------------------------

@dataclass
class MagMemory:
    """Opaque memory handle returned by ``memory_construction``."""
    episode_id: str
    project: str
    chunk_count: int
    trace_path: Optional[Path] = None
    _client: Optional[McpClient] = field(default=None, repr=False)


# ---------------------------------------------------------------------------
# Adapter
# ---------------------------------------------------------------------------

class MagMethod(BaseMethod):
    """
    AMA-Bench memory method backed by MAG via MCP.

    Parameters
    ----------
    top_k:
        Number of memories to retrieve per question.
    chunk_size:
        Maximum characters per trajectory chunk.
    chunk_overlap:
        Overlap between adjacent chunks.
    search_mode:
        ``text`` (default, FTS5 + advanced pipeline) or ``semantic``.
    search_limit:
        How many candidates to fetch from MAG before truncating to ``top_k``.
    trace_dir:
        Directory for JSONL trace files. Default: ``~/.mag/traces``.
    mag_binary:
        Path to the ``mag`` binary. Auto-detected if omitted.
    home_dir:
        MAG home directory (where the SQLite DB lives).
    """

    def __init__(
        self,
        top_k: int = 5,
        chunk_size: int = DEFAULT_CHUNK_SIZE,
        chunk_overlap: int = DEFAULT_CHUNK_OVERLAP,
        search_mode: str = "semantic",
        search_limit: int = 20,
        trace_dir: Optional[str] = None,
        mag_binary: Optional[str] = None,
        home_dir: Optional[str] = None,
        **kwargs: Any,
    ):
        self.top_k = top_k
        self.chunk_size = chunk_size
        self.chunk_overlap = chunk_overlap
        self.search_mode = search_mode
        self.search_limit = search_limit

        self._trace_dir = Path(trace_dir or os.path.expanduser("~/.mag/traces"))
        self._trace_dir.mkdir(parents=True, exist_ok=True)

        self._mag_binary = mag_binary
        self._home_dir = home_dir
        self._client: Optional[McpClient] = None
        self._run_id = str(uuid.uuid4())[:8]

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _ensure_client(self) -> McpClient:
        if self._client is None:
            self._client = McpClient(
                mag_binary=self._mag_binary,
                home_dir=self._home_dir,
                timeout_secs=120.0,
            )
            self._client.start()
        return self._client

    def _trace(self, event: Dict[str, Any]) -> None:
        event["_ts"] = time.time()
        event["_run"] = self._run_id
        trace_file = self._trace_dir / f"{self._run_id}.jsonl"
        with open(trace_file, "a", encoding="utf-8") as f:
            f.write(json.dumps(event, ensure_ascii=False) + "\n")

    # ------------------------------------------------------------------
    # BaseMethod interface
    # ------------------------------------------------------------------

    def memory_construction(self, traj_text: str, task: str = "") -> MagMemory:
        """
        Chunk *traj_text* and store each chunk in MAG.

        Returns a :class:`MagMemory` handle that carries the episode-scoped
        ``project`` filter used during retrieval.
        """
        client = self._ensure_client()
        episode_id = str(uuid.uuid4())
        project = f"ama_bench_{episode_id[:8]}"

        chunks = _chunk_text(traj_text, self.chunk_size, self.chunk_overlap)

        # Build batch items
        items: List[Dict[str, Any]] = []
        for idx, chunk in enumerate(chunks):
            items.append(
                {
                    "content": chunk,
                    "tags": ["ama_bench", f"episode_{episode_id[:8]}"],
                    "project": project,
                    "metadata": {
                        "episode_id": episode_id,
                        "chunk_index": idx,
                        "total_chunks": len(chunks),
                        "task": task,
                    },
                }
            )

        # Store in batches (memory_store_batch has a limit; chunk if needed)
        BATCH_SIZE = 50
        stored = 0
        for i in range(0, len(items), BATCH_SIZE):
            batch = items[i : i + BATCH_SIZE]
            result = client.store_batch(batch)
            stored += len(batch)

        self._trace(
            {
                "event": "memory_construction",
                "episode_id": episode_id,
                "project": project,
                "chunks": len(chunks),
                "task": task,
                "traj_chars": len(traj_text),
            }
        )

        trace_path = self._trace_dir / f"{self._run_id}.jsonl"
        return MagMemory(
            episode_id=episode_id,
            project=project,
            chunk_count=len(chunks),
            trace_path=trace_path,
            _client=client,
        )

    def memory_retrieve(self, memory: MagMemory, question: str) -> str:
        """
        Search MAG for memories scoped to *memory.project* and return the
        top ``self.top_k`` results concatenated.
        """
        if not isinstance(memory, MagMemory):
            raise ValueError("memory must be a MagMemory object")

        client = self._ensure_client()

        # Try advanced search first; fall back to plain search if abstained.
        result = client.search(
            query=question,
            mode=self.search_mode,
            limit=self.search_limit,
            advanced=True,
            project=memory.project,
        )

        results = result.get("results", [])
        abstained = result.get("abstained", False)

        if not results or abstained:
            result = client.search(
                query=question,
                mode=self.search_mode,
                limit=self.search_limit,
                advanced=False,
                project=memory.project,
            )
            results = result.get("results", [])
            abstained = False

        # Take top_k
        top = results[: self.top_k]
        context_parts: List[str] = []
        for r in top:
            content = r.get("content", "")
            if content:
                context_parts.append(content)

        context = "\n\n".join(context_parts)

        self._trace(
            {
                "event": "memory_retrieve",
                "episode_id": memory.episode_id,
                "project": memory.project,
                "question": question,
                "candidates": len(results),
                "used": len(top),
                "abstained": abstained,
                "context_chars": len(context),
            }
        )

        return context

    def close(self) -> None:
        """Terminate the underlying MCP client."""
        if self._client is not None:
            self._client.stop()
            self._client = None
