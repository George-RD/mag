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


def _search_with_fallback(
    client: SearchClient,
    query: str,
    limit: int,
    **kwargs: Any,
) -> Dict[str, Any]:
    """
    Run advanced search; if it abstains, fall back to basic search.
    Matches the official benchmark's behaviour.
    """
    resp = client.search_with_scores(query=query, limit=limit, **kwargs)
    if resp.get("abstained", False) or not resp.get("results", []):
        # Remove any kwargs that would conflict with our explicit args
        fallback_kwargs = {k: v for k, v in kwargs.items() if k not in ("advanced", "explain")}
        resp = client.search_with_scores(
            query=query, limit=limit, advanced=False, **fallback_kwargs
        )
        resp["_fallback"] = True
    return resp


class PassiveStrategy(RecallStrategy):
    """Passive retrieval: agent asks, MAG returns."""

    name = "passive"

    def __init__(self, limit: int = 5, **kwargs: Any):
        self.limit = limit

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = _search_with_fallback(client, query, self.limit, **kwargs)
        results = resp.get("results", [])
        context = "\n\n".join(r.get("content", "") for r in results)
        return {
            "context": context,
            "results": results,
            "strategy": self.name,
            "meta": {
                "result_count": len(results),
                "abstained": resp.get("abstained", False),
                "fallback": resp.get("_fallback", False),
            },
        }


class ThresholdStrategy(RecallStrategy):
    """Active-Threshold: inject when top result confidence exceeds threshold."""

    name = "active_threshold"

    def __init__(self, threshold: float = 0.5, limit: int = 5, expand_limit: int = 10, **kwargs: Any):
        self.threshold = threshold
        self.limit = limit
        self.expand_limit = expand_limit

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = _search_with_fallback(client, query, self.limit, **kwargs)
        confidence = resp.get("confidence", 0.0)
        results = resp.get("results", [])
        expanded = False

        if confidence < self.threshold and not resp.get("abstained", False) and not resp.get("_fallback", False):
            # Proactively expand search to get more context
            resp = _search_with_fallback(client, query, self.expand_limit, **kwargs)
            results = resp.get("results", [])
            confidence = resp.get("confidence", confidence)
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
        resp = _search_with_fallback(client, query, self.limit, **kwargs)
        results = resp.get("results", [])

        # Identify which session_ids appear in top results
        session_ids = []
        for r in results:
            sid = r.get("metadata", {}).get("session_id") or r.get("session_id")
            if sid and sid not in session_ids:
                session_ids.append(sid)

        # Proactively retrieve additional content from those sessions
        extra_results: List[Dict[str, Any]] = []
        seen_ids = {r.get("id") or r.get("memory_id") for r in results}
        for sid in session_ids[: self.session_limit]:
            session_resp = _search_with_fallback(
                client, "", self.session_limit, session_id=sid, **kwargs
            )
            for r in session_resp.get("results", []):
                rid = r.get("id") or r.get("memory_id")
                if rid is None:
                    # No stable id — fall back to identity check
                    if r not in results and r not in extra_results:
                        extra_results.append(r)
                elif rid not in seen_ids:
                    seen_ids.add(rid)
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
            resp = _search_with_fallback(
                client, "summary overview", self.period, **kwargs
            )
            summary = "\n".join(r.get("content", "") for r in resp.get("results", []))
            self._summaries.append(f"[Periodic summary after session {session_idx}]:\n{summary}")

    def retrieve(self, client: SearchClient, query: str, **kwargs: Any) -> Dict[str, Any]:
        resp = _search_with_fallback(client, query, self.limit, **kwargs)
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
