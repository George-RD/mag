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
