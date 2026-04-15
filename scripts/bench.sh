#!/usr/bin/env bash
# bench.sh — Standardized LoCoMo benchmark runner
#
# Usage:
#   ./scripts/bench.sh
#   ./scripts/bench.sh --model voyage-nano-int8
#   ./scripts/bench.sh --model bge-small --samples 10
#   ./scripts/bench.sh --model voyage-nano-fp32 --dim 512 --scoring-mode word-overlap
#
# Appends a row to docs/benchmarks/benchmark_log.csv and prints a comparison table of all
# runs with the same scoring-mode.  Also writes docs/benchmarks/LATEST.md.
#
# CSV format (16 columns):
#   date,commit,branch,issue_or_pr,scoring_mode,samples,embedding_model,
#   overall_score,single_hop,temporal,multi_hop,open_domain,adversarial,
#   evidence_recall,avg_embed_ms,notes

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESULTS_CSV="${REPO_DIR}/docs/benchmarks/benchmark_log.csv"
LATEST_MD="${REPO_DIR}/docs/benchmarks/LATEST.md"

CSV_HEADER="date,commit,branch,issue_or_pr,scoring_mode,samples,embedding_model,overall_score,single_hop,temporal,multi_hop,open_domain,adversarial,evidence_recall,avg_embed_ms,notes"

# ── Defaults ─────────────────────────────────────────────────────────────────
MODEL="bge-small"
DIM=""            # empty = use model default
SAMPLES=2
SCORING_MODE="word-overlap"
NOTES=""
GATE=false        # --gate: compare against 10-sample baseline, fail on regression
STRATEGY="sqlite-v1"
COMPARE_A=""
COMPARE_B=""
UPDATE_BASELINE=false

# ── Parse flags ───────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --model)        MODEL="$2";        shift 2 ;;
        --dim)          DIM="$2";          shift 2 ;;
        --samples)      SAMPLES="$2";      shift 2 ;;
        --scoring-mode) SCORING_MODE="$2"; shift 2 ;;
        --notes)        NOTES="$2";        shift 2 ;;
        --gate)         GATE=true;         shift ;;
        --strategy)     STRATEGY="$2";     shift 2 ;;
        --compare)      COMPARE_A="$2"; COMPARE_B="$3"; shift 3 ;;
        --update-baseline) UPDATE_BASELINE=true; shift ;;
        *) echo "Unknown flag: $1" >&2; exit 1 ;;
    esac
done

# ── Model → default dim + cargo flags ────────────────────────────────────────
case "$MODEL" in
    bge-small)
        EMBEDDING_MODEL="onnx-bge-small"
        CARGO_FLAGS=(--release --bin locomo_bench -- --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    voyage-nano-int8)
        EMBEDDING_MODEL="voyage-nano-int8"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant int8 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    voyage-nano-fp16)
        EMBEDDING_MODEL="voyage-nano-fp16"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant fp16 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    voyage-nano-fp32)
        EMBEDDING_MODEL="voyage-nano-fp32"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant fp32 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    voyage-nano-q4)
        EMBEDDING_MODEL="voyage-nano-q4"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant q4 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    granite)
        EMBEDDING_MODEL="granite-embedding-30m-english"
        CARGO_FLAGS=(--release --bin locomo_bench -- --granite --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    minilm-l6)
        EMBEDDING_MODEL="all-MiniLM-L6-v2-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --minilm-l6 --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    minilm-l12)
        EMBEDDING_MODEL="all-MiniLM-L12-v2-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --minilm-l12 --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    e5-small)
        EMBEDDING_MODEL="e5-small-v2-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --e5-small --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    bge-base)
        EMBEDDING_MODEL="bge-base-en-v1.5-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --bge-base --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    nomic)
        EMBEDDING_MODEL="nomic-embed-text-v1.5-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --nomic --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    arctic-xs)
        EMBEDDING_MODEL="snowflake-arctic-embed-xs-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --arctic-xs --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    arctic-s)
        EMBEDDING_MODEL="snowflake-arctic-embed-s-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --arctic-s --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    gte-small)
        EMBEDDING_MODEL="gte-small-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --gte-small --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${STRATEGY}")
        ;;
    *)
        echo "Unknown model: ${MODEL}" >&2
        echo "Valid models: bge-small, voyage-nano-int8, voyage-nano-fp16, voyage-nano-fp32, voyage-nano-q4," >&2
        echo "              granite, minilm-l6, minilm-l12, e5-small, bge-base, nomic," >&2
        echo "              arctic-xs, arctic-s, gte-small" >&2
        exit 1
        ;;
esac

# ── Ensure CSV exists ─────────────────────────────────────────────────────────
if [[ ! -f "${RESULTS_CSV}" ]]; then
    echo "${CSV_HEADER}" > "${RESULTS_CSV}"
    echo "Created ${RESULTS_CSV}"
fi

# ── Run benchmark, capture output ────────────────────────────────────────────
cd "${REPO_DIR}"
echo "Running: cargo run ${CARGO_FLAGS[*]}"
echo "──────────────────────────────────────────────────────────────────────────"

RAW_OUTPUT="$(cargo run "${CARGO_FLAGS[@]}" 2>&1)"
EXIT_CODE=$?
echo "${RAW_OUTPUT}"
echo "──────────────────────────────────────────────────────────────────────────"

if [[ $EXIT_CODE -ne 0 ]]; then
    echo "Benchmark run failed (exit ${EXIT_CODE})" >&2
    exit "${EXIT_CODE}"
fi

# ── Parse output ─────────────────────────────────────────────────────────────

# Primary score: word overlap / mean score line
overall_score=$(echo "${RAW_OUTPUT}" | grep -E "WORD OVERLAP|E2E WORD OVERLAP|MEAN TOKEN F1|MEAN LLM F1" | grep -oE '[0-9]+\.[0-9]+' | head -1)
overall_score="${overall_score:-0}"

# Evidence recall
evidence_recall=$(echo "${RAW_OUTPUT}" | grep "MEAN EV. RECALL" | grep -oE '[0-9]+\.[0-9]+' | head -1)
evidence_recall="${evidence_recall:-}"

# Category lines: "  Single-Hop QA            18.6%    87.6%    82.7%"
# Columns: Substr%, WdOvlp%, Ev.Rec% — we want WdOvlp (2nd percentage)
single_hop=$(echo "${RAW_OUTPUT}" | grep "Single-Hop QA" | grep -oE '[0-9]+\.[0-9]+%' | sed -n '2p' | tr -d '%')
single_hop="${single_hop:-}"

temporal=$(echo "${RAW_OUTPUT}" | grep "Temporal Reasoning" | grep -oE '[0-9]+\.[0-9]+%' | sed -n '2p' | tr -d '%')
temporal="${temporal:-}"

multi_hop=$(echo "${RAW_OUTPUT}" | grep "Multi-Hop QA" | grep -oE '[0-9]+\.[0-9]+%' | sed -n '2p' | tr -d '%')
multi_hop="${multi_hop:-}"

open_domain=$(echo "${RAW_OUTPUT}" | grep "Open-Domain" | grep -oE '[0-9]+\.[0-9]+%' | sed -n '2p' | tr -d '%')
open_domain="${open_domain:-}"

adversarial=$(echo "${RAW_OUTPUT}" | grep "Adversarial" | grep -oE '[0-9]+\.[0-9]+%' | sed -n '2p' | tr -d '%')
adversarial="${adversarial:-}"

# Timing — log avg embed latency (ms) for the embedding step only.
avg_embed_ms=$(echo "${RAW_OUTPUT}" | grep "Avg embed:" | grep -oE '[0-9]+(\.[0-9]+)?ms' | tr -d 'ms' | head -1)
avg_embed_ms="${avg_embed_ms:-}"

# Git metadata
DATE_STR="$(date '+%Y-%m-%d')"
COMMIT="$(git -C "${REPO_DIR}" rev-parse --short HEAD 2>/dev/null || echo '')"
BRANCH="$(git -C "${REPO_DIR}" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '')"

# ── Append CSV row ────────────────────────────────────────────────────────────
FULL_NOTES="${NOTES:+${NOTES} }strategy=${STRATEGY}"
CSV_ROW="${DATE_STR},${COMMIT},${BRANCH},,${SCORING_MODE},${SAMPLES},${EMBEDDING_MODEL},${overall_score},${single_hop},${temporal},${multi_hop},${open_domain},${adversarial},${evidence_recall},${avg_embed_ms},${FULL_NOTES}"
echo "${CSV_ROW}" >> "${RESULTS_CSV}"
echo "Appended result to ${RESULTS_CSV}"

# ── Print comparison table ────────────────────────────────────────────────────
print_table() {
    local mode="$1"
    printf "\n## LoCoMo Benchmark — %s scoring\n\n" "${mode}"
    printf "| Date | Model | Overall%% | 1-Hop | Temporal | Multi-Hop | Open | Adv | EvRec%% | Avg Emb (ms) |\n"
    printf "|------|-------|---------|-------|----------|-----------|------|-----|--------|------------|\n"

    # Skip header line, filter by scoring_mode (col 5)
    tail -n +2 "${RESULTS_CSV}" | awk -F',' -v mode="${mode}" '
        NF > 0 && $5 == mode {
            printf "| %s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n",
                $1, $7, $8, $9, $10, $11, $12, $13, $14, $15
        }
    '
    printf "\n"
}

TABLE="$(print_table "${SCORING_MODE}")"
echo "${TABLE}"

# ── Write docs/benchmarks/LATEST.md ───────────────────────────────────────────
mkdir -p "$(dirname "${LATEST_MD}")"
{
    printf "# MAG Benchmark Results\n\n"
    printf "Latest benchmark runs. Updated automatically by \`./scripts/bench.sh\`.\n\n"
    printf "See \`docs/benchmarks/benchmark_log.csv\` for full history.\n"
    printf "%s\n" "${TABLE}"
} > "${LATEST_MD}"
echo "Updated ${LATEST_MD}"

# ── Compare mode: run two strategies head-to-head ────────────────────────────
if [[ -n "${COMPARE_A}" && -n "${COMPARE_B}" ]]; then
    echo ""
    echo "=== Strategy Comparison: ${COMPARE_A} vs ${COMPARE_B} ==="
    echo ""

    COMPARE_TMP_DIR="$(mktemp -d)"
    trap "rm -rf '${COMPARE_TMP_DIR}'" EXIT

    # Run strategy A
    # Note: compare mode uses default (bge-small) model flags; --model flags are not forwarded.
    echo "Running strategy A: ${COMPARE_A}..."
    A_FLAGS=(--release --bin locomo_bench -- --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${COMPARE_A}" --json)
    cargo run "${A_FLAGS[@]}" 2>/dev/null > "${COMPARE_TMP_DIR}/a.json"
    echo "Strategy A complete."

    # Run strategy B
    echo "Running strategy B: ${COMPARE_B}..."
    B_FLAGS=(--release --bin locomo_bench -- --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}" --strategy "${COMPARE_B}" --json)
    cargo run "${B_FLAGS[@]}" 2>/dev/null > "${COMPARE_TMP_DIR}/b.json"
    echo "Strategy B complete."

    # Generate comparison report
    COMPARE_SCRIPT="${REPO_DIR}/scripts/compare_strategies.py"
    BASELINES_JSON="${REPO_DIR}/docs/benchmarks/baselines.json"
    COMPARE_OUTPUT_DIR="${REPO_DIR}/docs/benchmarks/comparisons"

    COMPARE_CMD=(python3 "${COMPARE_SCRIPT}"
        --a-json "${COMPARE_TMP_DIR}/a.json"
        --b-json "${COMPARE_TMP_DIR}/b.json"
        --output-dir "${COMPARE_OUTPUT_DIR}")

    if [[ -f "${BASELINES_JSON}" ]]; then
        COMPARE_CMD+=(--baselines "${BASELINES_JSON}")
    fi

    if [[ "${UPDATE_BASELINE}" == true ]]; then
        COMPARE_CMD+=(--update-baseline)
    fi

    "${COMPARE_CMD[@]}"

    # Append CSV rows for both strategies
    for STRAT_FILE in a b; do
        STRAT_JSON="${COMPARE_TMP_DIR}/${STRAT_FILE}.json"
        STRAT_ID=$(python3 -c "import json; print(json.load(open('${STRAT_JSON}')).get('strategy', 'unknown'))")
        STRAT_OVERALL=$(python3 -c "import json; d=json.load(open('${STRAT_JSON}')); print(f'{d.get(\"mean_f1\", 0)*100:.1f}')")
        OTHER_STRAT="${COMPARE_B}"
        if [[ "${STRAT_FILE}" == "b" ]]; then OTHER_STRAT="${COMPARE_A}"; fi
        STRAT_NOTES="strategy=${STRAT_ID} compare_with=${OTHER_STRAT}"
        STRAT_ROW="${DATE_STR},${COMMIT},${BRANCH},,${SCORING_MODE},${SAMPLES},${EMBEDDING_MODEL},${STRAT_OVERALL},,,,,,,,${STRAT_NOTES}"
        echo "${STRAT_ROW}" >> "${RESULTS_CSV}"
    done
    echo "Appended comparison CSV rows to ${RESULTS_CSV}"

    echo ""
    echo "=== Comparison complete ==="
    exit 0
fi

# ── Gate mode: compare against 10-sample baseline ────────────────────────────
if [[ "${GATE}" == true ]]; then
    echo ""
    echo "=== PR Benchmark Gate ==="

    WARN_THRESHOLD=2.0   # suggest 10-sample confirmation
    FAIL_THRESHOLD=5.0   # hard fail — too large for variance

    BASELINES_JSON="${REPO_DIR}/docs/benchmarks/baselines.json"
    BASELINE=""
    if [[ -f "${BASELINES_JSON}" ]]; then
        BASELINE=$(python3 -c "
import json, sys
with open('${BASELINES_JSON}') as f:
    db = json.load(f)
strategy = '${STRATEGY}'
mode = '${SCORING_MODE}'
try:
    print(db[strategy][mode]['overall'])
except KeyError:
    print('')
" 2>/dev/null || echo "")
    fi
    # Fallback to CSV grep if baselines.json lookup failed.
    if [ -z "$BASELINE" ]; then
        BASELINE=$(grep ",10," "${RESULTS_CSV}" | grep "bge-small" | tail -1 | cut -d',' -f8)
    fi
    if [ -z "$BASELINE" ]; then
        echo "WARNING: No 10-sample baseline found — skipping gate"
    else
        echo "10-sample baseline: ${BASELINE}%"
        echo "Current (${SAMPLES}-sample): ${overall_score}%"

        DIFF=$(echo "$BASELINE - $overall_score" | bc -l)
        HARD_FAIL=$(echo "$DIFF > $FAIL_THRESHOLD" | bc -l)
        SOFT_WARN=$(echo "$DIFF > $WARN_THRESHOLD" | bc -l)

        if [ "$HARD_FAIL" = "1" ]; then
            echo ""
            echo "FAIL: Likely regression — ${overall_score}% vs ${BASELINE}% (delta: -${DIFF}pp)"
            echo "Exceeds ${FAIL_THRESHOLD}pp hard-fail threshold."
            exit 1
        elif [ "$SOFT_WARN" = "1" ]; then
            echo ""
            echo "WARNING: Possible regression — ${overall_score}% vs ${BASELINE}% (delta: -${DIFF}pp)"
            echo "Within ${SAMPLES}-sample variance but suspicious. Run full validation:"
            echo "  ./scripts/bench.sh --samples 10 --notes 'pre-merge validation'"
        else
            echo "PASS: within ${WARN_THRESHOLD}pp of baseline"
        fi
    fi

    # Docs freshness check
    echo ""
    if [ -f "${REPO_DIR}/docs/benchmarks/methodology.md" ]; then
        METH_DATE=$(grep -oE 'Last verified: [0-9]{4}-[0-9]{2}-[0-9]{2}' "${REPO_DIR}/docs/benchmarks/methodology.md" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' || \
                    grep -m1 "^- Date:" "${REPO_DIR}/docs/benchmarks/methodology.md" | grep -oE '[0-9]{4}-[0-9]{2}-[0-9]{2}' || echo "")
        if [ -n "$METH_DATE" ]; then
            METH_EPOCH=$(date -j -f "%Y-%m-%d" "$METH_DATE" "+%s" 2>/dev/null || date -d "$METH_DATE" "+%s" 2>/dev/null || echo "0")
            NOW_EPOCH=$(date "+%s")
            DAYS_OLD=$(( (NOW_EPOCH - METH_EPOCH) / 86400 ))
            if [ "$DAYS_OLD" -gt 7 ]; then
                echo "WARNING: methodology.md is ${DAYS_OLD}d stale (${METH_DATE})"
            else
                echo "Docs: methodology.md updated ${DAYS_OLD}d ago"
            fi
        fi
    fi
    if [ -f "${REPO_DIR}/README.md" ] && [ -n "${BASELINE:-}" ]; then
        README_PCT=$(grep -oE '[0-9]+\.[0-9]+% retrieval accuracy' "${REPO_DIR}/README.md" | grep -oE '[0-9]+\.[0-9]+' || echo "")
        if [ -n "$README_PCT" ]; then
            ABS_DIFF=$(echo "$README_PCT - $BASELINE" | bc -l | tr -d '-')
            TOO_FAR=$(echo "$ABS_DIFF > 1.5" | bc -l)
            if [ "$TOO_FAR" = "1" ]; then
                echo "WARNING: README says ${README_PCT}% but baseline is ${BASELINE}%"
            fi
        fi
    fi

    echo ""
    echo "=== Gate complete ==="
fi
