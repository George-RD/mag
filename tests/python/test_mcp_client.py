"""Tests for mag_memory.mcp_client."""


def test_search_with_scores_injects_score():
    from mag_memory.mcp_client import McpClient

    # Mock the underlying call_tool to avoid spawning mag serve
    client = McpClient.__new__(McpClient)
    client._req_id = 0

    def mock_call_tool(name, arguments):
        return {
            "results": [
                {"content": "hello", "metadata": {"_text_overlap": 0.85}},
                {"content": "world", "metadata": {}},
            ],
            "result_count": 2,
            "abstained": False,
            "confidence": 0.85,
        }

    client.call_tool = mock_call_tool
    resp = client.search_with_scores("test query")
    assert resp["results"][0]["score"] == 0.85
    assert resp["results"][1]["score"] == 0.0
