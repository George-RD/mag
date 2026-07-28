---
        node: mag.runtime.memory.retrieval
        status: blocked
        created: 2026-07-28
        ---
        # Calibrate Retrieval And Reranking

        Blocked by the local evaluation harness.

Replace provisional global cutoffs with calibrated confidence from semantic
score, margin, lexical agreement, reranker score, intent, and candidate
diversity. Evaluate the existing cross-encoder and local embedding/reranker
alternatives. Add dynamic result count and token budget.
