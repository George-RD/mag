#!/bin/sh
# Install MAG — curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh | sh
#
# Environment variables:
#   VERSION          — install a specific version (e.g. "0.1.0"); default: latest
#   MAG_INSTALL_DIR  — destination directory; default: ~/.mag/bin
#
# Flags:
#   --version <VER>  — install a specific version
#   --help           — show usage
#
# Requirements: curl or wget, tar, and optionally sha256sum/shasum for verification.

set -eu

# ---------------------------------------------------------------------------
# Colour helpers (degrade gracefully when not a TTY)
# ---------------------------------------------------------------------------
setup_colours() {
    if [ -t 1 ] && command -v tput >/dev/null 2>&1 && tput colors >/dev/null 2>&1; then
        RED=$(tput setaf 1)
        GREEN=$(tput setaf 2)
        YELLOW=$(tput setaf 3)
        CYAN=$(tput setaf 6)
        BOLD=$(tput bold)
        RESET=$(tput sgr0)
    else
        RED=""
        GREEN=""
        YELLOW=""
        CYAN=""
        BOLD=""
        RESET=""
    fi
}

info()  { printf '%s[info]%s  %s\n'  "${CYAN}"   "${RESET}" "$1"; }
ok()    { printf '%s[ok]%s    %s\n'   "${GREEN}"  "${RESET}" "$1"; }
warn()  { printf '%s[warn]%s  %s\n'  "${YELLOW}" "${RESET}" "$1"; }
err()   { printf '%s[error]%s %s\n'  "${RED}"    "${RESET}" "$1" >&2; }
die()   { err "$1"; exit 1; }

# ---------------------------------------------------------------------------
# Usage
# ---------------------------------------------------------------------------
usage() {
    cat <<EOF
${BOLD}Install MAG${RESET} — Local MCP memory server

Usage:
  curl -fsSL https://raw.githubusercontent.com/George-RD/mag/main/install.sh | sh
  ./install.sh [--version 0.1.0] [--uninstall] [--help]

Environment variables:
  VERSION          Install a specific version (default: latest release)
  MAG_INSTALL_DIR  Installation directory (default: ~/.mag/bin)
EOF
    exit 0
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --version)
                [ $# -ge 2 ] || die "--version requires a value"
                VERSION="$2"
                shift 2
                ;;
            --uninstall)
                UNINSTALL=1
                shift
                ;;
            --help|-h)
                usage
                ;;
            *)
                die "Unknown option: $1"
                ;;
        esac
    done
}

# ---------------------------------------------------------------------------
# Uninstall
# ---------------------------------------------------------------------------
do_uninstall() {
    INSTALL_DIR="${MAG_INSTALL_DIR:-${HOME}/.mag/bin}"

    if [ -f "${INSTALL_DIR}/mag" ]; then
        rm -f "${INSTALL_DIR}/mag"
        ok "Removed ${INSTALL_DIR}/mag"
    else
        warn "mag binary not found at ${INSTALL_DIR}/mag"
    fi

    # Remove empty bin directory
    if [ -d "$INSTALL_DIR" ] && [ -z "$(ls -A "$INSTALL_DIR" 2>/dev/null)" ]; then
        rmdir "$INSTALL_DIR"
    fi

    # Clean PATH from shell profiles
    for PROFILE in "$HOME/.zshrc" "$HOME/.bash_profile" "$HOME/.bashrc" "$HOME/.config/fish/config.fish"; do
        if [ -f "$PROFILE" ] && grep -q "$INSTALL_DIR" "$PROFILE" 2>/dev/null; then
            # Remove the MAG comment and PATH line
            TMPFILE="$(mktemp)"
            sed '/^# MAG$/d' "$PROFILE" | sed "\|${INSTALL_DIR}|d" > "$TMPFILE"
            mv "$TMPFILE" "$PROFILE"
            ok "Removed PATH entry from ${PROFILE}"
        fi
    done

    info "MAG binary uninstalled. Data remains at ~/.mag/ (delete manually if desired)."
    exit 0
}

# ---------------------------------------------------------------------------
# Platform detection
# ---------------------------------------------------------------------------
detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  PLATFORM_OS="unknown-linux-gnu" ;;
        Darwin) PLATFORM_OS="apple-darwin" ;;
        MINGW*|MSYS*|CYGWIN*)
            die "This installer does not support Windows. Download the .zip from GitHub Releases instead."
            ;;
        *)
            die "Unsupported operating system: $OS"
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   PLATFORM_ARCH="x86_64" ;;
        aarch64|arm64)  PLATFORM_ARCH="aarch64" ;;
        *)
            die "Unsupported architecture: $ARCH"
            ;;
    esac

    TARGET="${PLATFORM_ARCH}-${PLATFORM_OS}"
}

# ---------------------------------------------------------------------------
# HTTP fetch helper (curl preferred, wget fallback)
# ---------------------------------------------------------------------------
has_cmd() { command -v "$1" >/dev/null 2>&1; }

fetch() {
    _url="$1"
    _dest="${2:-}"

    if has_cmd curl; then
        if [ -n "$_dest" ]; then
            curl -fsSL --retry 3 -o "$_dest" "$_url"
        else
            curl -fsSL --retry 3 "$_url"
        fi
    elif has_cmd wget; then
        if [ -n "$_dest" ]; then
            wget -q -O "$_dest" "$_url"
        else
            wget -q -O- "$_url"
        fi
    else
        die "Neither curl nor wget found. Please install one and retry."
    fi
}

# ---------------------------------------------------------------------------
# Resolve version (latest from GitHub API, or from VERSION env/arg)
# ---------------------------------------------------------------------------
resolve_version() {
    if [ -n "${VERSION:-}" ]; then
        # Strip leading 'v' if present
        VERSION="${VERSION#v}"
        info "Using requested version: ${BOLD}${VERSION}${RESET}"
        return
    fi

    info "Detecting latest release..."
    LATEST_URL="https://api.github.com/repos/George-RD/mag/releases/latest"

    # Extract tag_name from JSON without jq (POSIX-portable)
    RESPONSE="$(fetch "$LATEST_URL" "")" || die "Failed to query GitHub API for latest release"
    VERSION="$(printf '%s' "$RESPONSE" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -1)"

    if [ -z "$VERSION" ]; then
        die "Could not determine latest version from GitHub. Set VERSION=x.y.z and retry."
    fi

    ok "Latest version: ${BOLD}${VERSION}${RESET}"
}

# ---------------------------------------------------------------------------
# Download & verify
# ---------------------------------------------------------------------------
download_and_verify() {
    ARCHIVE="mag-${TARGET}.tar.gz"
    URL="https://github.com/George-RD/mag/releases/download/v${VERSION}/${ARCHIVE}"
    CHECKSUMS_URL="https://github.com/George-RD/mag/releases/download/v${VERSION}/checksums.txt"

    TMPDIR_INSTALL="$(mktemp -d)" || die "Failed to create temporary directory"
    trap 'rm -rf "$TMPDIR_INSTALL"' EXIT

    info "Downloading ${BOLD}${ARCHIVE}${RESET} (v${VERSION})..."
    fetch "$URL" "${TMPDIR_INSTALL}/${ARCHIVE}" || die "Download failed. Check that version ${VERSION} exists at:\n  ${URL}"

    ok "Downloaded successfully"

    # Checksum verification (best-effort)
    verify_checksum
}

verify_checksum() {
    SHASUM_CMD=""
    if has_cmd sha256sum; then
        SHASUM_CMD="sha256sum"
    elif has_cmd shasum; then
        SHASUM_CMD="shasum -a 256"
    fi

    if [ -z "$SHASUM_CMD" ]; then
        warn "sha256sum/shasum not found — skipping checksum verification"
        return
    fi

    info "Verifying checksum..."
    if ! fetch "$CHECKSUMS_URL" "${TMPDIR_INSTALL}/checksums.txt" 2>/dev/null; then
        warn "Could not download checksums.txt — skipping verification"
        return
    fi

    EXPECTED="$(grep "${ARCHIVE}" "${TMPDIR_INSTALL}/checksums.txt" | awk '{print $1}')"
    if [ -z "$EXPECTED" ]; then
        warn "No checksum entry found for ${ARCHIVE} — skipping verification"
        return
    fi

    ACTUAL="$(cd "${TMPDIR_INSTALL}" && $SHASUM_CMD "${ARCHIVE}" | awk '{print $1}')"

    if [ "$EXPECTED" != "$ACTUAL" ]; then
        die "Checksum mismatch!\n  Expected: ${EXPECTED}\n  Got:      ${ACTUAL}\nThe download may be corrupted. Please retry."
    fi

    ok "Checksum verified"
}

# ---------------------------------------------------------------------------
# Install
# ---------------------------------------------------------------------------
install_binary() {
    INSTALL_DIR="${MAG_INSTALL_DIR:-${HOME}/.mag/bin}"

    info "Installing to ${BOLD}${INSTALL_DIR}${RESET}..."

    mkdir -p "$INSTALL_DIR" || die "Failed to create directory: ${INSTALL_DIR}"

    tar -xzf "${TMPDIR_INSTALL}/${ARCHIVE}" -C "${TMPDIR_INSTALL}" || die "Failed to extract archive"

    # The archive may contain just the binary or a directory — find it
    if [ -f "${TMPDIR_INSTALL}/mag" ]; then
        mv "${TMPDIR_INSTALL}/mag" "${INSTALL_DIR}/mag"
    elif [ -f "${TMPDIR_INSTALL}/mag/mag" ]; then
        mv "${TMPDIR_INSTALL}/mag/mag" "${INSTALL_DIR}/mag"
    else
        # Search for the binary
        FOUND="$(find "${TMPDIR_INSTALL}" -name mag -type f | head -1)"
        if [ -z "$FOUND" ]; then
            die "Could not locate 'mag' binary in the archive"
        fi
        mv "$FOUND" "${INSTALL_DIR}/mag"
    fi

    chmod +x "${INSTALL_DIR}/mag"
    ok "Installed ${BOLD}mag${RESET} to ${INSTALL_DIR}/mag"
}

# ---------------------------------------------------------------------------
# Post-install guidance
# ---------------------------------------------------------------------------
post_install() {
    # Check if already on PATH
    case ":${PATH}:" in
        *":${INSTALL_DIR}:"*)
            ok "mag is already on your PATH"
            printf '\n  %s$ mag --version%s\n\n' "${BOLD}" "${RESET}"
            return
            ;;
    esac

    PATH_LINE="export PATH=\"${INSTALL_DIR}:\$PATH\""
    SHELL_NAME="$(basename "${SHELL:-/bin/sh}")"

    case "$SHELL_NAME" in
        zsh)
            PROFILE="$HOME/.zshrc"
            ;;
        bash)
            if [ -f "$HOME/.bash_profile" ]; then
                PROFILE="$HOME/.bash_profile"
            else
                PROFILE="$HOME/.bashrc"
            fi
            ;;
        fish)
            PROFILE="$HOME/.config/fish/config.fish"
            PATH_LINE="fish_add_path ${INSTALL_DIR}"
            ;;
        *)
            PROFILE=""
            ;;
    esac

    # Auto-add to shell profile if we can
    if [ -n "$PROFILE" ]; then
        if [ -f "$PROFILE" ] && grep -q "$INSTALL_DIR" "$PROFILE" 2>/dev/null; then
            ok "PATH already configured in ${PROFILE}"
        else
            printf '%s\n' "" "# MAG" "$PATH_LINE" >> "$PROFILE"
            ok "Added mag to PATH in ${BOLD}${PROFILE}${RESET}"
        fi
        info "Restart your shell or run:"
        printf '\n  %s%s%s\n\n' "${BOLD}" "$PATH_LINE" "${RESET}"
    else
        printf '\n'
        info "Add mag to your PATH:"
        printf '\n  %s\n\n' "$PATH_LINE"
    fi
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
main() {
    setup_colours
    parse_args "$@"
    [ "${UNINSTALL:-0}" = "1" ] && do_uninstall
    detect_platform
    resolve_version
    download_and_verify
    install_binary
    post_install

    ok "${BOLD}MAG v${VERSION}${RESET} installed successfully!"
}

main "$@"
