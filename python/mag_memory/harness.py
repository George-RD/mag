"""
Minimal AgentMemoryHarness interface.

This is the Python-side counterpart to the Rust ``AgentMemoryHarness`` trait.
It provides three operations — *observe*, *retrieve_context*, *trace* —
without prescribing how they are implemented underneath.
"""

from __future__ import annotations

import json
import os
import time
import uuid
from abc import ABC, abstractmethod
from pathlib import Path
from typing import Any, Dict, Optional

from .mcp_client import McpClient


class AgentTurn:
    """
    A single turn in an agent trajectory.

    This is intentionally unstructured — callers pass whatever fields
    are relevant for their domain.
    """

    def __init__(
        self,
        turn_idx: int,
        action: str = "",
        observation: str = "",
        metadata: Optional[Dict[str, Any]] = None,
    ):
        self.turn_idx = turn_idx
        self.action = action
        self.observation = observation
        self.metadata = metadata or {}

    def to_text(self) -> str:
        """Serialise to the AMA-Bench trajectory text format."""
        parts = [f"Step {self.turn_idx}:"]
        if self.action:
            parts.append(f"Action: {self.action}")
        if self.observation:
            parts.append(f"Observation: {self.observation}")
        return "\n".join(parts)


class AgentMemoryHarness(ABC):
    """
    Three-method harness that sits between a benchmark and MAG.

    Implementations may use the MCP stdio interface (for Python callers)
    or the Rust storage traits directly (for in-process callers).
    """

    @abstractmethod
    def observe(self, turn: AgentTurn) -> None:
        """Ingest a single turn into memory."""
        ...

    @abstractmethod
    def retrieve_context(self, query: str) -> str:
        """Retrieve relevant context for *query*."""
        ...

    @abstractmethod
    def trace(self, event: Dict[str, Any]) -> None:
        """Emit an unstructured JSON trace event."""
        ...


class MagHarness(AgentMemoryHarness):
    """
    In-process harness backed by a :class:`McpClient`.

    This is suitable for Python benchmarks that want to drive MAG
    turn-by-turn rather than batch-store an entire trajectory.
    """

    def __init__(
        self,
        session_id: Optional[str] = None,
        project: Optional[str] = None,
        trace_dir: Optional[str] = None,
        mag_binary: Optional[str] = None,
        home_dir: Optional[str] = None,
        search_mode: str = "text",
        search_limit: int = 10,
    ):
        self.session_id = session_id or str(uuid.uuid4())
        self.project = project or f"harness_{self.session_id[:8]}"
        self.search_mode = search_mode
        self.search_limit = search_limit

        self._trace_dir = Path(trace_dir or os.path.expanduser("~/.mag/traces"))
        self._trace_dir.mkdir(parents=True, exist_ok=True)
        self._trace_file = self._trace_dir / f"{self.session_id}.jsonl"

        self._client = McpClient(
            mag_binary=mag_binary,
            home_dir=home_dir,
            timeout_secs=120.0,
        )
        self._client.start()

    def observe(self, turn: AgentTurn) -> None:
        content = turn.to_text()
        self._client.store(
            content=content,
            tags=["harness", "turn"],
            project=self.project,
            session_id=self.session_id,
            metadata={
                "turn_idx": turn.turn_idx,
                **turn.metadata,
            },
        )
        self.trace(
            {
                "event": "observe",
                "turn_idx": turn.turn_idx,
                "content_chars": len(content),
            }
        )

    def retrieve_context(self, query: str) -> str:
        result = self._client.search(
            query=query,
            mode=self.search_mode,
            limit=self.search_limit,
            advanced=True,
            project=self.project,
        )
        results = result.get("results", [])
        context = "\n\n".join(r.get("content", "") for r in results)
        self.trace(
            {
                "event": "retrieve",
                "query": query,
                "results": len(results),
                "context_chars": len(context),
            }
        )
        return context

    def trace(self, event: Dict[str, Any]) -> None:
        event["_ts"] = time.time()
        event["_session"] = self.session_id
        with open(self._trace_file, "a", encoding="utf-8") as f:
            f.write(json.dumps(event, ensure_ascii=False) + "\n")

    def close(self) -> None:
        self._client.stop()
