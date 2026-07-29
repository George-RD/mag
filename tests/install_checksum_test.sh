#!/bin/sh
set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

INSTALL_LIB="${TMP_ROOT}/install-lib.sh"
awk '/^main "\$@"$/ { exit } { print }' "${ROOT_DIR}/install.sh" > "$INSTALL_LIB"

PASS_COUNT=0
FAIL_COUNT=0

detect_fixture_tool() {
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s\n' sha256sum
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s\n' shasum
    elif command -v openssl >/dev/null 2>&1; then
        printf '%s\n' openssl
    else
        printf 'No SHA-256 utility available for installer tests\n' >&2
        exit 1
    fi
}

FIXTURE_TOOL=$(detect_fixture_tool)

fixture_sha256() {
    _fixture_file="$1"
    case "$FIXTURE_TOOL" in
        sha256sum)
            sha256sum "$_fixture_file" | awk '{print $1}'
            ;;
        shasum)
            shasum -a 256 "$_fixture_file" | awk '{print $1}'
            ;;
        openssl)
            openssl dgst -sha256 "$_fixture_file" | awk '{print $NF}'
            ;;
    esac
}

installer_has_fixture_tool() {
    [ "$1" = "$FIXTURE_TOOL" ]
}

pass() {
    PASS_COUNT=$((PASS_COUNT + 1))
    printf 'ok - %s\n' "$1"
}

fail() {
    FAIL_COUNT=$((FAIL_COUNT + 1))
    printf 'not ok - %s\n' "$1" >&2
}

run_expect_failure() {
    _name="$1"
    _needle="$2"
    _case_fn="$3"
    _output="${TMP_ROOT}/${_case_fn}.out"

    if "$_case_fn" >"$_output" 2>&1; then
        fail "${_name}: unexpectedly succeeded"
        return
    fi

    if ! grep -F "$_needle" "$_output" >/dev/null 2>&1; then
        cat "$_output" >&2
        fail "${_name}: missing error '${_needle}'"
        return
    fi

    pass "$_name"
}

run_expect_success() {
    _name="$1"
    _case_fn="$2"
    _output="${TMP_ROOT}/${_case_fn}.out"

    if ! "$_case_fn" >"$_output" 2>&1; then
        cat "$_output" >&2
        fail "${_name}: unexpectedly failed"
        return
    fi

    pass "$_name"
}

load_installer() {
    # shellcheck disable=SC1090
    . "$INSTALL_LIB"
    RED=""
    GREEN=""
    YELLOW=""
    CYAN=""
    BOLD=""
    RESET=""
}

prepare_archive() {
    _case_dir="$1"
    mkdir -p "$_case_dir"
    TMPDIR_INSTALL="$_case_dir"
    ARCHIVE="mag-x86_64-unknown-linux-gnu.tar.gz"
    CHECKSUMS_URL="https://example.invalid/checksums.txt"
    printf 'verified archive bytes' > "${TMPDIR_INSTALL}/${ARCHIVE}"
}

case_missing_hash_tool() (
    load_installer
    prepare_archive "${TMP_ROOT}/missing-tool"
    has_cmd() { return 1; }
    verify_checksum
)

case_manifest_download_failure() (
    load_installer
    prepare_archive "${TMP_ROOT}/manifest-download"
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() { return 1; }
    verify_checksum
)

case_missing_exact_entry() (
    load_installer
    prepare_archive "${TMP_ROOT}/missing-entry"
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() {
        printf '%064d  %s\n' 0 "mag-aarch64-unknown-linux-gnu.tar.gz" > "$2"
    }
    verify_checksum
)

case_malformed_entry() (
    load_installer
    prepare_archive "${TMP_ROOT}/malformed-entry"
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() { printf 'not-a-sha256  %s\n' "$ARCHIVE" > "$2"; }
    verify_checksum
)

case_duplicate_entry() (
    load_installer
    prepare_archive "${TMP_ROOT}/duplicate-entry"
    _actual=$(fixture_sha256 "${TMPDIR_INSTALL}/${ARCHIVE}")
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() {
        printf '%s  %s\n%s  %s\n' "$_actual" "$ARCHIVE" "$_actual" "$ARCHIVE" > "$2"
    }
    verify_checksum
)

case_checksum_mismatch() (
    load_installer
    prepare_archive "${TMP_ROOT}/mismatch"
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() { printf '%064d  %s\n' 0 "$ARCHIVE" > "$2"; }
    verify_checksum
)

case_exact_entry_ignores_filename_collision() (
    load_installer
    prepare_archive "${TMP_ROOT}/filename-collision"
    _actual=$(fixture_sha256 "${TMPDIR_INSTALL}/${ARCHIVE}")
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() {
        printf '%064d  prefix-%s\n%s  %s\n' 0 "$ARCHIVE" "$_actual" "$ARCHIVE" > "$2"
    }
    verify_checksum
)

case_matching_checksum() (
    load_installer
    prepare_archive "${TMP_ROOT}/matching"
    _actual=$(fixture_sha256 "${TMPDIR_INSTALL}/${ARCHIVE}")
    has_cmd() { installer_has_fixture_tool "$1"; }
    fetch() { printf '%s  %s\n' "$_actual" "$ARCHIVE" > "$2"; }
    verify_checksum
)

run_expect_failure \
    "missing hash utility fails closed" \
    "Checksum verification requires" \
    case_missing_hash_tool
run_expect_failure \
    "manifest download failure fails closed" \
    "Failed to download checksums.txt" \
    case_manifest_download_failure
run_expect_failure \
    "missing exact archive entry fails closed" \
    "No checksum entry found" \
    case_missing_exact_entry
run_expect_failure \
    "malformed archive checksum fails closed" \
    "Malformed checksum entry" \
    case_malformed_entry
run_expect_failure \
    "duplicate archive entries fail closed" \
    "Duplicate checksum entries" \
    case_duplicate_entry
run_expect_failure \
    "checksum mismatch fails closed" \
    "Checksum mismatch" \
    case_checksum_mismatch
run_expect_success \
    "exact archive matching ignores filename collisions" \
    case_exact_entry_ignores_filename_collision
run_expect_success \
    "matching checksum succeeds" \
    case_matching_checksum

if [ "$FAIL_COUNT" -ne 0 ]; then
    printf '%s test(s) failed; %s passed\n' "$FAIL_COUNT" "$PASS_COUNT" >&2
    exit 1
fi

printf '%s installer checksum tests passed using %s\n' "$PASS_COUNT" "$FIXTURE_TOOL"
