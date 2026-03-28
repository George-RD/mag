#!/usr/bin/env bash
# PR Benchmark Gate — runs before PR creation to catch regressions.
#
# Runs 2-sample LoCoMo word-overlap benchmark (~15s) and compares against
# the last 10-sample baseline in benchmark_log.csv.
#
# IMPORTANT: 2-sample has inherent variance (~2pp). This gate only WARNS
# on moderate dips and FAILS on large drops. It never updates 10-sample
# baseline data — that requires a deliberate full validation run.
#
# Exit 0 = pass, Exit 1 = hard fail (definite regression)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LOG_FILE="$REPO_ROOT/docs/benchmarks/benchmark_log.csv"

# 2-sample variance is ~2pp, so:
WARN_THRESHOLD=2.0   # flag for attention — suggest 10-sample confirmation
FAIL_THRESHOLD=5.0   # hard fail — definitely a regression, not variance

echo "=== PR Benchmark Gate ==="
echo ""

# ── Step 1: Get 10-sample baseline from benchmark_log.csv ──
BASELINE=""
if [ -f "$LOG_FILE" ]; then
    # Column 8 = overall_score; find most recent 10-sample bge-small entry
    BASELINE=$(grep ",10," "$LOG_FILE" | grep "bge-small" | tail -1 | cut -d',' -f8)
fi

if [ -z "$BASELINE" ]; then
    echo "WARNING: No 10-sample baseline found in $LOG_FILE"
    echo "Skipping benchmark comparison (no baseline to compare against)"
    exit 0
fi

echo "10-sample baseline: ${BASELINE}%"
echo ""

# ── Step 2: Run 2-sample benchmark ──
echo "Running 2-sample LoCoMo benchmark..."
BENCH_OUTPUT=$(cargo run --release --bin locomo_bench -- --samples 2 --scoring-mode word-overlap 2>&1)
CURRENT=$(echo "$BENCH_OUTPUT" | grep "WORD OVERLAP" | grep -oE '[0-9]+\.[0-9]+')

if [ -z "$CURRENT" ]; then
    echo "ERROR: Could not parse benchmark output"
    echo "$BENCH_OUTPUT" | tail -20
    exit 1
fi

echo "2-sample result:    ${CURRENT}%"
echo ""

# ── Step 3: Compare with tiered thresholds ──
DIFF=$(echo "$BASELINE - $CURRENT" | bc -l)

HARD_FAIL=$(echo "$DIFF > $FAIL_THRESHOLD" | bc -l)
SOFT_WARN=$(echo "$DIFF > $WARN_THRESHOLD" | bc -l)

if [ "$HARD_FAIL" = "1" ]; then
    echo "FAIL: Likely regression: ${CURRENT}% vs ${BASELINE}% baseline (delta: -${DIFF}pp)"
    echo ""
    echo "This exceeds the ${FAIL_THRESHOLD}pp hard-fail threshold."
    echo "The 2-sample run suggests a real regression, not just variance."
    echo ""
    echo "Full output:"
    echo "$BENCH_OUTPUT" | tail -15
    exit 1
elif [ "$SOFT_WARN" = "1" ]; then
    echo "WARNING: Possible regression: ${CURRENT}% vs ${BASELINE}% baseline (delta: -${DIFF}pp)"
    echo ""
    echo "This exceeds the ${WARN_THRESHOLD}pp soft threshold but is within 2-sample variance."
    echo "Before merging, run a full 10-sample validation to confirm:"
    echo ""
    echo "  cargo run --release --bin locomo_bench -- --samples 10 --scoring-mode word-overlap"
    echo ""
    echo "If the 10-sample result matches the baseline, this is just variance — merge freely."
    echo "If 10-sample also shows a drop, investigate before merging."
else
    echo "PASS: ${CURRENT}% vs ${BASELINE}% baseline (delta: ${DIFF}pp)"
fi

echo ""

# ── Step 4: Check docs freshness ──
echo "=== Docs Freshness Check ==="

if [ -f "$REPO_ROOT/docs/benchmarks/methodology.md" ]; then
    METH_DATE=$(grep -m1 "^- Date:" "$REPO_ROOT/docs/benchmarks/methodology.md" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' || echo "unknown")
    if [ "$METH_DATE" != "unknown" ]; then
        METH_EPOCH=$(date -j -f "%Y-%m-%d" "$METH_DATE" "+%s" 2>/dev/null || date -d "$METH_DATE" "+%s" 2>/dev/null || echo "0")
        NOW_EPOCH=$(date "+%s")
        DAYS_OLD=$(( (NOW_EPOCH - METH_EPOCH) / 86400 ))
        if [ "$DAYS_OLD" -gt 7 ]; then
            echo "WARNING: methodology.md benchmark date is ${DAYS_OLD} days old (${METH_DATE})"
            echo "  Consider updating with a fresh 10-sample run."
        else
            echo "PASS: methodology.md is ${DAYS_OLD} days old"
        fi
    fi
fi

if [ -f "$REPO_ROOT/README.md" ]; then
    README_PCT=$(grep -oE '[0-9]+\.[0-9]+% retrieval accuracy' "$REPO_ROOT/README.md" | grep -oE '[0-9]+\.[0-9]+' || echo "")
    if [ -n "$README_PCT" ] && [ -n "$BASELINE" ]; then
        ABS_DIFF=$(echo "$README_PCT - $BASELINE" | bc -l | tr -d '-')
        TOO_FAR=$(echo "$ABS_DIFF > 1.5" | bc -l)
        if [ "$TOO_FAR" = "1" ]; then
            echo "WARNING: README claims ${README_PCT}% but 10-sample baseline is ${BASELINE}%"
            echo "  Update README to match the validated baseline."
        else
            echo "PASS: README (${README_PCT}%) matches baseline (${BASELINE}%)"
        fi
    fi
fi

echo ""
echo "=== Gate complete ==="
