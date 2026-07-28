---
        node: mag.quality.tests
        ---
        # mag.quality.tests contract

        Tests are hermetic, deterministic where possible, and isolated from user
data through temporary HOME, USERPROFILE, and MAG_DATA_ROOT values. Product
tests cover real CLI/MCP behavior, not only internal units.
