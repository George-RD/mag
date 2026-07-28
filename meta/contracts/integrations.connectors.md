---
        node: mag.integrations.connectors
        ---
        # mag.integrations.connectors contract

        Connectors translate MAG's stable capabilities into agent-specific files
and lifecycle hooks. They do not own separate memory semantics. Installation
and removal are idempotent and preserve user edits.
