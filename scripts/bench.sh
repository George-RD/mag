#!/usr/bin/env bash
# bench.sh — Standardized LoCoMo benchmark runner
#
# Usage:
#   ./scripts/bench.sh
#   ./scripts/bench.sh --model voyage-nano-int8
#   ./scripts/bench.sh --model bge-small --samples 10
#   ./scripts/bench.sh --model voyage-nano-fp32 --dim 512 --scoring-mode word-overlap
#
# Appends a row to docs/benchmark_log.csv and prints a comparison table of all
# runs with the same scoring-mode.  Also writes benchmarks/LATEST.md.
#
# CSV format (16 columns):
#   date,commit,branch,issue_or_pr,scoring_mode,samples,embedding_model,
#   overall_score,single_hop,temporal,multi_hop,open_domain,adversarial,
#   evidence_recall,avg_query_ms,notes

set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RESULTS_CSV="${REPO_DIR}/docs/benchmark_log.csv"
LATEST_MD="${REPO_DIR}/benchmarks/LATEST.md"

CSV_HEADER="date,commit,branch,issue_or_pr,scoring_mode,samples,embedding_model,overall_score,single_hop,temporal,multi_hop,open_domain,adversarial,evidence_recall,avg_query_ms,notes"

# ── Defaults ─────────────────────────────────────────────────────────────────
MODEL="bge-small"
DIM=""            # empty = use model default
SAMPLES=2
SCORING_MODE="word-overlap"
NOTES=""

# ── Parse flags ───────────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --model)        MODEL="$2";        shift 2 ;;
        --dim)          DIM="$2";          shift 2 ;;
        --samples)      SAMPLES="$2";      shift 2 ;;
        --scoring-mode) SCORING_MODE="$2"; shift 2 ;;
        --notes)        NOTES="$2";        shift 2 ;;
        *) echo "Unknown flag: $1" >&2; exit 1 ;;
    esac
done

# ── Model → default dim + cargo flags ────────────────────────────────────────
case "$MODEL" in
    bge-small)
        DEFAULT_DIM=384
        EMBEDDING_MODEL="onnx-bge-small"
        CARGO_FLAGS=(--release --bin locomo_bench -- --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    voyage-nano-int8)
        DEFAULT_DIM=1024
        EMBEDDING_MODEL="voyage-nano-int8"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant int8 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    voyage-nano-fp16)
        DEFAULT_DIM=1024
        EMBEDDING_MODEL="voyage-nano-fp16"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant fp16 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    voyage-nano-fp32)
        DEFAULT_DIM=1024
        EMBEDDING_MODEL="voyage-nano-fp32"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant fp32 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    voyage-nano-q4)
        DEFAULT_DIM=1024
        EMBEDDING_MODEL="voyage-nano-q4"
        DIM_ARG="${DIM:-1024}"
        CARGO_FLAGS=(--release --bin locomo_bench -- --voyage-onnx --voyage-quant q4 --embedder-dim "${DIM_ARG}" --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    granite)
        DEFAULT_DIM=384
        EMBEDDING_MODEL="granite-embedding-30m-english"
        CARGO_FLAGS=(--release --bin locomo_bench -- --granite --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    minilm-l6)
        DEFAULT_DIM=384
        EMBEDDING_MODEL="all-MiniLM-L6-v2"
        CARGO_FLAGS=(--release --bin locomo_bench -- --minilm-l6 --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    minilm-l12)
        DEFAULT_DIM=384
        EMBEDDING_MODEL="all-MiniLM-L12-v2"
        CARGO_FLAGS=(--release --bin locomo_bench -- --minilm-l12 --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    e5-small)
        DEFAULT_DIM=384
        EMBEDDING_MODEL="e5-small-v2"
        CARGO_FLAGS=(--release --bin locomo_bench -- --e5-small --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    bge-base)
        DEFAULT_DIM=768
        EMBEDDING_MODEL="bge-base-en-v1.5"
        CARGO_FLAGS=(--release --bin locomo_bench -- --bge-base --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    nomic)
        DEFAULT_DIM=768
        EMBEDDING_MODEL="nomic-embed-text-v1.5-int8"
        CARGO_FLAGS=(--release --bin locomo_bench -- --nomic --scoring-mode "${SCORING_MODE}" --samples "${SAMPLES}")
        ;;
    *)
        echo "Unknown model: ${MODEL}" >&2
        echo "Valid models: bge-small, voyage-nano-int8, voyage-nano-fp16, voyage-nano-fp32, voyage-nano-q4," >&2
        echo "              granite, minilm-l6, minilm-l12, e5-small, bge-base, nomic" >&2
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
echo "Running: cargo ${CARGO_FLAGS[*]}"
echo "──────────────────────────────────────────────────────────────────────────"

RAW_OUTPUT="$(cargo "${CARGO_FLAGS[@]}" 2>&1)"
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

# Timing
avg_query_ms=$(echo "${RAW_OUTPUT}" | grep "Avg query:" | grep -oE '[0-9]+ms' | tr -d 'ms' | head -1)
avg_query_ms="${avg_query_ms:-}"

# Git metadata
DATE_STR="$(date '+%Y-%m-%d')"
COMMIT="$(git -C "${REPO_DIR}" rev-parse --short HEAD 2>/dev/null || echo '')"
BRANCH="$(git -C "${REPO_DIR}" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '')"

# ── Append CSV row ────────────────────────────────────────────────────────────
CSV_ROW="${DATE_STR},${COMMIT},${BRANCH},,${SCORING_MODE},${SAMPLES},${EMBEDDING_MODEL},${overall_score},${single_hop},${temporal},${multi_hop},${open_domain},${adversarial},${evidence_recall},${avg_query_ms},${NOTES}"
echo "${CSV_ROW}" >> "${RESULTS_CSV}"
echo "Appended result to ${RESULTS_CSV}"

# ── Print comparison table ────────────────────────────────────────────────────
print_table() {
    local mode="$1"
    printf "\n## LoCoMo Benchmark — %s scoring\n\n" "${mode}"
    printf "| Date | Model | Overall%% | 1-Hop | Temporal | Multi-Hop | Open | Adv | EvRec%% | Avg Q (ms) |\n"
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

# ── Write benchmarks/LATEST.md ────────────────────────────────────────────────
mkdir -p "$(dirname "${LATEST_MD}")"
{
    printf "# MAG Benchmark Results\n\n"
    printf "Latest benchmark runs. Updated automatically by \`./scripts/bench.sh\`.\n\n"
    printf "See \`docs/benchmark_log.csv\` for full history.\n"
    printf "%s\n" "${TABLE}"
} > "${LATEST_MD}"
echo "Updated ${LATEST_MD}"
