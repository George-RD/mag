"""Tests for mag_memory.mcp_client."""

from unittest.mock import MagicMock


def test_search_with_scores_uses_canonical_score():
    """When result already has 'score', preserve it."""
    from mag_memory.mcp_client import McpClient

    client = McpClient.__new__(McpClient)

    def mock_search(**kwargs):
        assert kwargs.get("explain") is True, "explain=True must be forwarded"
        return {
            "results": [
                {"content": "hello", "score": 0.92, "metadata": {"_text_overlap": 0.85}},
            ],
            "result_count": 1,
            "abstained": False,
            "confidence": 0.92,
        }

    client.search = mock_search
    resp = client.search_with_scores("test query")
    assert resp["results"][0]["score"] == 0.92


def test_search_with_scores_falls_back_to_text_overlap():
    """When 'score' is missing, fall back to metadata._text_overlap."""
    from mag_memory.mcp_client import McpClient

    client = McpClient.__new__(McpClient)

    def mock_search(**kwargs):
        assert kwargs.get("explain") is True
        return {
            "results": [
                {"content": "hello", "metadata": {"_text_overlap": 0.85}},
                {"content": "world", "metadata": {}},
            ],
            "result_count": 2,
            "abstained": False,
            "confidence": 0.85,
        }

    client.search = mock_search
    resp = client.search_with_scores("test query")
    assert resp["results"][0]["score"] == 0.85
    assert resp["results"][1]["score"] == 0.0


def test_search_explain_parameter():
    """search() must accept and forward explain parameter."""
    from mag_memory.mcp_client import McpClient

    client = McpClient.__new__(McpClient)
    client._request_id = 0
    calls = []

    def mock_call_tool(name, arguments):
        calls.append(arguments)
        return {"results": [], "result_count": 0, "abstained": True, "confidence": 0.0}

    client.call_tool = mock_call_tool
    client.search("q", explain=True)
    assert calls[-1].get("explain") is True

    client.search("q", explain=False)
    assert calls[-1].get("explain") is False
