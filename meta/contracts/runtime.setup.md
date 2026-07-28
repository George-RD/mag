---
        node: mag.runtime.setup
        ---
        # mag.runtime.setup contract

        Setup owns tool detection, generated configuration, data-path resolution,
installation, and uninstall symmetry. Operations must be idempotent, must
not silently overwrite user-managed content, and must leave an explicit
recovery path when setup is partial.
