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


def test_run_ab_test_with_two_strategies():
    from mag_memory.active_strategies import ThresholdStrategy
    client = FakeMcpClient([
        {"results": [{"content": "answer here", "score": 0.8}], "confidence": 0.8, "abstained": False},
        {"results": [{"content": "answer here", "score": 0.8}], "confidence": 0.8, "abstained": False},
    ])
    runner = LongMemEvalRunner(client)
    strategies = [PassiveStrategy(limit=1), ThresholdStrategy(threshold=0.5, limit=1)]
    questions = [
        {"question": "Q1", "answer": "answer here", "question_id": "q1", "question_type": "multi-session"},
    ]
    summaries = runner.run_ab_test(questions, strategies)
    assert "passive" in summaries
    assert "active_threshold" in summaries


def test_context_manager():
    client = FakeMcpClient([])
    with LongMemEvalRunner(client, project="ctx_test") as runner:
        assert runner.project == "ctx_test"
    # After exiting, trace file should be closed
    assert runner._trace_file is None
