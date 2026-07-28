---
        node: mag.runtime.entrypoints
        ---
        # mag.runtime.entrypoints contract

        The entrypoint layer owns process startup, CLI dispatch, and assembly of
concrete components. It must not contain independent storage, retrieval,
extraction, or connector semantics. MCP mode keeps stdout exclusively for
protocol traffic and sends diagnostics to stderr.
