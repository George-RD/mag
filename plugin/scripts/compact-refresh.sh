#!/bin/sh
# MAG post-compact — re-inject top memories after context compaction
mag hook compact-refresh --project "$(basename "$PWD")" --budget-tokens 800 2>/dev/null || true
