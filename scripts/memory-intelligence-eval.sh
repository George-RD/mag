#!/bin/sh
# memory-intelligence-eval.sh — Memory-intelligence evaluation runner
#
# Usage:
#   ./scripts/memory-intelligence-eval.sh
#   ./scripts/memory-intelligence-eval.sh --embedder placeholder
#   ./scripts/memory-intelligence-eval.sh --embedder profile-bge-small --notes 'profile check'
#   ./scripts/memory-intelligence-eval.sh --validate-only
#
# Appends a row to docs/benchmarks/memory_intelligence_log.csv and writes
# docs/benchmarks/MEMORY-INTELLIGENCE.md.  It never touches benchmark_log.csv,
# whose 16 columns are consumed positionally by scripts/bench.sh.
#
# CSV format (20 columns):
#   date,commit,branch,embedder,dataset_version,dataset_sha256_12,
#   schema_validity,overall_score,families_scored,families_selected,
#   entities,temporal,relationships,lifecycle,supersession,grouping,provenance,
#   questions,peak_rss_kb,notes
#
# dataset_sha256_12 is the 12-character prefix the harness prints beside the
# dataset version.  dataset_version alone does not identify the annotations: an
# in-place edit of data/memory_intelligence_eval/v1/ leaves it reading "v1"
# while ground truth moves under it.  A run whose dataset sha differs from the
# last row with the same dataset_version is refused; put changed annotations in
# a new version directory instead.
#
# families_scored is the denominator of overall_score.  It shrinks when a family
# measures nothing, which raises the mean, so a row with a smaller denominator
# is not comparable with a full one.

set -eu

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RESULTS_CSV="${REPO_DIR}/docs/benchmarks/memory_intelligence_log.csv"
LATEST_MD="${REPO_DIR}/docs/benchmarks/MEMORY-INTELLIGENCE.md"

CSV_HEADER="date,commit,branch,embedder,dataset_version,dataset_sha256_12,schema_validity,overall_score,families_scored,families_selected,entities,temporal,relationships,lifecycle,supersession,grouping,provenance,questions,peak_rss_kb,notes"

# ── Defaults ─────────────────────────────────────────────────────────────────
EMBEDDER="bge-small"
DATASET="data/memory_intelligence_eval/v1"
NOTES=""
VALIDATE_ONLY=false

# ── Parse flags ───────────────────────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        --embedder)      EMBEDDER="$2";      shift 2 ;;
        --dataset)       DATASET="$2";       shift 2 ;;
        --notes)         NOTES="$2";         shift 2 ;;
        --validate-only) VALIDATE_ONLY=true; shift ;;
        *) echo "Unknown flag: $1" >&2; exit 1 ;;
    esac
done

case "${EMBEDDER}" in
    placeholder|bge-small|profile-bge-small) ;;
    *)
        echo "Unknown embedder: ${EMBEDDER}" >&2
        echo "Valid embedders: placeholder, bge-small, profile-bge-small" >&2
        exit 1
        ;;
esac

cd "${REPO_DIR}"

# ── Validate-only mode: no CSV row, no doc rewrite ───────────────────────────
if [ "${VALIDATE_ONLY}" = true ]; then
    cargo run --release --bin memory_intelligence_eval -- \
        --dataset "${DATASET}" --validate-only
    exit 0
fi

# ── Ensure CSV exists and matches the current column set ─────────────────────
if [ ! -f "${RESULTS_CSV}" ]; then
    mkdir -p "$(dirname "${RESULTS_CSV}")"
    echo "${CSV_HEADER}" > "${RESULTS_CSV}"
    echo "Created ${RESULTS_CSV}"
elif [ "$(head -1 "${RESULTS_CSV}")" != "${CSV_HEADER}" ]; then
    echo "Column mismatch in ${RESULTS_CSV}" >&2
    echo "  on disk: $(head -1 "${RESULTS_CSV}")" >&2
    echo "  expected: ${CSV_HEADER}" >&2
    echo "Migrate the existing rows to the new columns, or move the file aside." >&2
    exit 1
fi

# ── Run the harness, capture output ──────────────────────────────────────────
echo "Running: cargo run --release --bin memory_intelligence_eval -- --embedder ${EMBEDDER}"
echo "──────────────────────────────────────────────────────────────────────────"

set +e
RAW_OUTPUT="$(cargo run --release --bin memory_intelligence_eval -- \
    --dataset "${DATASET}" --embedder "${EMBEDDER}" 2>&1)"
EXIT_CODE=$?
set -e
echo "${RAW_OUTPUT}"
echo "──────────────────────────────────────────────────────────────────────────"

if [ "${EXIT_CODE}" -ne 0 ]; then
    echo "Evaluation run failed (exit ${EXIT_CODE})" >&2
    exit "${EXIT_CODE}"
fi

# ── Parse output ─────────────────────────────────────────────────────────────
# Summary-table rows are "  <family> <metric> <score>%  <p50>  <p95>  <grade>".
# A family reported as not measurable prints "n/a" and yields an empty cell.
family_score() {
    echo "${RAW_OUTPUT}" | grep -E "^  $1 .*%" | grep -oE '[0-9]+\.[0-9]+%' \
        | head -1 | tr -d '%'
}

overall_score=$(echo "${RAW_OUTPUT}" | grep "OVERALL:" | grep -oE '[0-9]+\.[0-9]+' | head -1)
schema_validity=$(echo "${RAW_OUTPUT}" | grep "Schema validity:" | grep -oE '[0-9]+\.[0-9]+' | head -1)
dataset_version=$(echo "${RAW_OUTPUT}" | grep "^  Dataset:" | awk '{print $2}')
dataset_sha=$(echo "${RAW_OUTPUT}" | grep "^  Dataset:" | grep -oE '[0-9a-f]{12}' | head -1)
peak_rss_kb=$(echo "${RAW_OUTPUT}" | grep "Peak RSS:" | grep -oE '[0-9]+' | head -1)

# The OVERALL line states its own denominator:
#   "OVERALL: 59.1% (D) — unweighted mean of 8 scored families of 8 run; ..."
families_scored=$(echo "${RAW_OUTPUT}" | grep "OVERALL:" \
    | sed -n 's/.*unweighted mean of \([0-9][0-9]*\) scored.*/\1/p' | head -1)
families_selected=$(echo "${RAW_OUTPUT}" | grep "OVERALL:" \
    | sed -n 's/.* of \([0-9][0-9]*\) run.*/\1/p' | head -1)

if [ -z "${dataset_sha}" ]; then
    echo "Could not read the dataset sha from the run header; refusing to log a row" >&2
    exit 1
fi

# ── Refuse an in-place annotation change under an unchanged version ──────────
previous_sha=$(tail -n +2 "${RESULTS_CSV}" \
    | awk -F',' -v version="${dataset_version}" '$5 == version { print $6 }' | tail -1)
if [ -n "${previous_sha}" ] && [ "${previous_sha}" != "${dataset_sha}" ]; then
    echo "Dataset ${dataset_version} hashes to ${dataset_sha}, but the last logged" >&2
    echo "${dataset_version} row was measured against ${previous_sha}." >&2
    echo "Ground truth moved without the version changing, so the new row would not be" >&2
    echo "comparable with the old ones. Copy the annotations into a new version" >&2
    echo "directory (data/memory_intelligence_eval/v2/) and log against that." >&2
    exit 1
fi

entities=$(family_score entities)
temporal=$(family_score temporal)
relationships=$(family_score relationships)
lifecycle=$(family_score lifecycle)
supersession=$(family_score supersession)
grouping=$(family_score grouping)
provenance=$(family_score provenance)
questions=$(family_score questions)

# Git metadata
DATE_STR="$(date '+%Y-%m-%d')"
# Read the seed count from the manifest so the generated preamble cannot drift
# from the dataset it describes.
SEED_COUNT=$(python3 -c "import json,sys;print(json.load(open(sys.argv[1]))['counts']['seed'])" \
    "${DATASET}/manifest.json" 2>/dev/null || echo "an unknown number of")
COMMIT="$(git -C "${REPO_DIR}" rev-parse --short HEAD 2>/dev/null || echo '')"
BRANCH="$(git -C "${REPO_DIR}" rev-parse --abbrev-ref HEAD 2>/dev/null || echo '')"

# ── Append CSV row ────────────────────────────────────────────────────────────
FULL_NOTES="${NOTES:+${NOTES} }dataset=${DATASET}"
CSV_ROW="${DATE_STR},${COMMIT},${BRANCH},${EMBEDDER},${dataset_version},${dataset_sha},${schema_validity},${overall_score},${families_scored},${families_selected},${entities},${temporal},${relationships},${lifecycle},${supersession},${grouping},${provenance},${questions},${peak_rss_kb},${FULL_NOTES}"
echo "${CSV_ROW}" >> "${RESULTS_CSV}"
echo "Appended result to ${RESULTS_CSV}"

# ── Print comparison table ────────────────────────────────────────────────────
print_table() {
    printf "\n## Memory Intelligence Evaluation\n\n"
    printf "| Date | Embedder | Dataset | Dataset sha | Schema%% | Overall%% | Scored | Entities | Temporal | Rel | Lifecycle | Superseded | Grouping | Provenance | Questions | Peak RSS (KB) |\n"
    printf "|------|----------|---------|-------------|---------|----------|--------|----------|----------|-----|-----------|------------|----------|------------|-----------|---------------|\n"

    tail -n +2 "${RESULTS_CSV}" | awk -F',' '
        NF > 0 {
            printf "| %s | %s | %s | %s | %s | %s | %s/%s | %s | %s | %s | %s | %s | %s | %s | %s | %s |\n",
                $1, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19
        }
    '
    printf "\n"
}

TABLE="$(print_table)"
echo "${TABLE}"

# ── Write docs/benchmarks/MEMORY-INTELLIGENCE.md ─────────────────────────────
mkdir -p "$(dirname "${LATEST_MD}")"
{
    printf "# MAG Memory Intelligence Results\n\n"
    printf "Latest evaluation runs. Updated automatically by \`./scripts/memory-intelligence-eval.sh\`.\n\n"
    printf "The family columns are headline metrics as percentages, and they measure\n"
    printf "different things: micro F1, recall@10, accuracy, F1, cluster coverage and link\n"
    printf "integrity. \`Overall%%\` is their unweighted mean, so it is a mean over\n"
    printf "incommensurable numbers and moves when the set of scoring families changes.\n"
    printf "\`Scored\` is that mean's denominator over the families the run selected; an\n"
    printf "empty family cell means the family measured nothing and left the denominator.\n\n"
    printf "Six families have no production implementation and are excluded from\n"
    printf "\`Overall%%\`: fact extraction, contradiction detection, summarisation,\n"
    printf "relationship typing, entity normalisation, and \`referenced_date\` inference.\n\n"
    printf "The corpus is %s seeds, so single cases move a family score by tens of points:\n" "$SEED_COUNT"
    printf "\`relationships\` has three annotated edges, and one missed edge is 33 points.\n\n"
    printf "\`Dataset sha\` is the first 12 characters of the SHA-256 of \`dataset.json\`.\n"
    printf "Rows sharing a dataset version but not a sha were measured against different\n"
    printf "ground truth. Metric definitions are in \`docs/benchmarks/memory-intelligence.md\`.\n\n"
    printf "See \`docs/benchmarks/memory_intelligence_log.csv\` for full history.\n"
    printf "%s\n" "${TABLE}"
} > "${LATEST_MD}"
echo "Updated ${LATEST_MD}"
