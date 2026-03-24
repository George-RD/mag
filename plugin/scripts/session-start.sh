#!/bin/sh
# MAG session start — recall project context
# Outputs memory context for Claude Code injection
mag hook session-start --project "$(basename "$PWD")" --budget-tokens 2000 2>/dev/null || true
