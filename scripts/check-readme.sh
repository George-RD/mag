#!/usr/bin/env bash
# check-readme.sh — Analyze recent code changes and suggest README updates
#
# Usage:
#   ./scripts/check-readme.sh
#   ./scripts/check-readme.sh "voyage-nano int8, word-overlap 91.1%"
#
# Requires: ANTHROPIC_API_KEY set in environment (or .env.local)

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONTEXT_NOTE="${1:-}"

# Load .env.local if present (same pattern as the rest of the codebase)
if [[ -f "${REPO_DIR}/.env.local" ]]; then
    # shellcheck disable=SC2046
    export $(grep -v '^#' "${REPO_DIR}/.env.local" | xargs) 2>/dev/null || true
fi

if [[ -z "${ANTHROPIC_API_KEY:-}" ]]; then
    echo "Error: ANTHROPIC_API_KEY is not set." >&2
    echo "Set it in your environment or in .env.local" >&2
    exit 1
fi

cd "${REPO_DIR}"

# ── Gather inputs ─────────────────────────────────────────────────────────────
DIFF="$(git diff HEAD~3..HEAD -- src/ benches/ 2>/dev/null || echo '(no diff available)')"
README="$(cat README.md)"

# Trim diff if it's very large (keep first 6000 chars to stay within context)
if [[ ${#DIFF} -gt 6000 ]]; then
    DIFF="${DIFF:0:6000}
... (diff truncated for brevity)"
fi

CONTEXT_SECTION=""
if [[ -n "$CONTEXT_NOTE" ]]; then
    CONTEXT_SECTION="Additional context from the caller: ${CONTEXT_NOTE}"
fi

# ── Build prompt ──────────────────────────────────────────────────────────────
PROMPT="You are reviewing a Rust MCP memory server project (MAG). Your task is to identify what in the diff below would be meaningful for users to know about in the README.

Focus only on:
1. New features or CLI flags users can invoke
2. Changed benchmark scores (only if improved)
3. Changed architecture that affects how users configure or run the system
4. New supported models or embedders

Do NOT suggest adding:
- Internal refactoring details
- Test changes
- Documentation-only changes
- Minor bug fixes unless user-visible

Output format: for each suggested change, write a short block like:
  SECTION: <which README section>
  WHY: <one sentence reason>
  BEFORE: <existing text to replace, or 'N/A' if new>
  AFTER: <suggested replacement text>

Be concise. If no README changes are needed, say so clearly.

${CONTEXT_SECTION}

--- README.md ---
${README}

--- git diff HEAD~3..HEAD -- src/ benches/ ---
${DIFF}"

# ── Call Haiku API ────────────────────────────────────────────────────────────
echo "Analyzing diff against README..."
echo "──────────────────────────────────────────────────────────────────────────"

RESPONSE=$(curl -sf https://api.anthropic.com/v1/messages \
    -H "x-api-key: ${ANTHROPIC_API_KEY}" \
    -H "anthropic-version: 2023-06-01" \
    -H "content-type: application/json" \
    -d "$(jq -n \
        --arg model "claude-haiku-4-5-20251001" \
        --arg prompt "${PROMPT}" \
        '{
            model: $model,
            max_tokens: 1024,
            messages: [{role: "user", content: $prompt}]
        }'
    )")

# Extract text from response
echo "${RESPONSE}" | jq -r '.content[0].text // "Error: no response text"'
echo "──────────────────────────────────────────────────────────────────────────"
echo "(This is a suggestion only — no files were modified)"
