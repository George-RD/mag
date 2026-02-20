# Parity Harness

This directory provides a local scaffold for comparing `romega-memory` and `omega-memory` behavior/latency.

## Quick start

Set the omega command once:

```bash
export OMEGA_MEMORY_CMD="python /path/to/omega-memory/main.py"
```

Run the harness:

```bash
bash parity/run_parity.sh
```

## Notes

- If `OMEGA_MEMORY_CMD` is not set, the harness still runs `romega-memory` checks.
- This scaffold is intentionally minimal and safe for local development.
