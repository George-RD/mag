#!/bin/sh
# MAG prompt gate — pure regex, NO daemon call, <1ms
# Outputs a hint only when the prompt suggests memory would help
# Silence (empty stdout) is the default for ~95% of prompts

read -r PROMPT

# Check for memory-relevant signals
case "$PROMPT" in
  *remember*|*"don't forget"*|*"store this"*|*"note that"*|*"save this"*)
    echo '{"additionalContext":"<MAG_HINT>User wants to store something. Consider using mag memory_store.</MAG_HINT>"}'
    exit 0
    ;;
  *"last time"*|*previously*|*"we discussed"*|*"what did we"*|*"we decided"*|*recall*)
    echo '{"additionalContext":"<MAG_HINT>User references prior context. Consider using mag memory_search.</MAG_HINT>"}'
    exit 0
    ;;
  *checkpoint*|*handoff*|*"wrap up"*|*"pick up where"*)
    echo '{"additionalContext":"<MAG_HINT>User wants checkpoint/handoff. Consider using mag memory_checkpoint.</MAG_HINT>"}'
    exit 0
    ;;
esac

# Default: silence. No memory injection needed.
