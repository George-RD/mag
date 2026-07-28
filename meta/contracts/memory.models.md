---
        node: mag.runtime.memory.models
        ---
        # mag.runtime.memory.models contract

        Embedding, generation, and reranking are separate model roles behind
explicit interfaces. The core quality baseline must run locally without
cloud credentials. Remote and self-hosted adapters remain optional and use
the same memory semantics. Missing models fail visibly and fall back only
when the caller explicitly permits it.
