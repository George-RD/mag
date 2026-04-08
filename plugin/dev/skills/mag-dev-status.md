# mag-dev-status

Report the status of the MAG dev plugin environment.

## Trigger

Use this skill when the user asks about the dev plugin status, dev memory stats, dev log, or anything related to `~/.dev-mag/`.

## Steps

1. Check if the dev data root exists:

   ```bash
   ls -la "$HOME/.dev-mag/" 2>/dev/null || echo "NOT FOUND"
   ```

2. Show memory count (if DB exists):

   ```bash
   MAG_DATA_ROOT="$HOME/.dev-mag" mag doctor 2>/dev/null || echo "mag doctor failed"
   ```

3. Show last 10 JSONL log entries (pretty-printed if jq available):

   ```bash
   if command -v jq >/dev/null 2>&1; then
     tail -20 "$HOME/.dev-mag/auto-capture.jsonl" 2>/dev/null | jq -r '[.ts, .event, .project, .hook.status] | @tsv' | tail -10 || echo "No JSONL log found"
   else
     tail -10 "$HOME/.dev-mag/auto-capture.jsonl" 2>/dev/null || echo "No JSONL log found"
   fi
   ```

4. Show any pre-compact state snapshots:

   ```bash
   ls "$HOME/.dev-mag/state/" 2>/dev/null || echo "No state files"
   ```

5. Report summary to user: dev root path, memory count, last event timestamp, and any warnings.
