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
