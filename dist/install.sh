#!/usr/bin/env bash
# Install sidebar-tui (sb) from GitHub Releases
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/ftl-tools/sidebar_tui/main/dist/install.sh | bash
#
# Downloads the pre-built binary for the current platform and installs it to
# /usr/local/bin/sb or ~/.local/bin/sb if /usr/local/bin is not writable.

set -euo pipefail

GITHUB_OWNER="ftl-tools"
GITHUB_REPO="sidebar_tui"
BIN_NAME="sb"

# ── Helpers ───────────────────────────────────────────────────────────────────

say() { printf "\033[1;32m==>\033[0m %s\n" "$*"; }
err() { printf "\033[1;31merror:\033[0m %s\n" "$*" >&2; exit 1; }

need_cmd() {
    if ! command -v "$1" >/dev/null 2>&1; then
        err "Required command not found: $1"
    fi
}

download() {
    local url="$1" dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$dest"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO "$dest" "$url"
    else
        err "Neither curl nor wget found. Please install one and retry."
    fi
}

download_stdout() {
    local url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- "$url"
    else
        err "Neither curl nor wget found."
    fi
}

# ── Platform detection ────────────────────────────────────────────────────────

detect_target() {
    local os arch

    case "$(uname -s)" in
        Darwin) os="apple-darwin" ;;
        Linux)  os="unknown-linux-musl" ;;
        *) err "Unsupported OS: $(uname -s). Install from source: https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}" ;;
    esac

    case "$(uname -m)" in
        arm64|aarch64) arch="aarch64" ;;
        x86_64)        arch="x86_64" ;;
        *) err "Unsupported architecture: $(uname -m)" ;;
    esac

    echo "${arch}-${os}"
}

# ── Version detection ─────────────────────────────────────────────────────────

get_latest_version() {
    local url="https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/latest"
    local version
    version=$(download_stdout "$url" | grep '"tag_name"' | sed -E 's/.*"tag_name": ?"([^"]+)".*/\1/')

    if [ -z "$version" ]; then
        err "Could not determine latest version. Check https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases"
    fi
    echo "$version"
}

# ── Install directory ─────────────────────────────────────────────────────────

get_install_dir() {
    if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
        echo "/usr/local/bin"
    else
        local local_bin="${HOME}/.local/bin"
        mkdir -p "$local_bin"
        echo "$local_bin"
    fi
}

# ── Main ──────────────────────────────────────────────────────────────────────

main() {
    need_cmd uname
    need_cmd tar

    say "Detecting platform..."
    local target
    target=$(detect_target)
    echo "  Target: ${target}"

    say "Fetching latest release..."
    local version
    version=$(get_latest_version)
    echo "  Version: ${version}"

    local archive_name="${BIN_NAME}-${version}-${target}.tar.gz"
    local archive_url="https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/releases/download/${version}/${archive_name}"

    local install_dir
    install_dir=$(get_install_dir)
    echo "  Install directory: ${install_dir}"

    local tmp_dir
    tmp_dir=$(mktemp -d)
    # shellcheck disable=SC2064
    trap "rm -rf '$tmp_dir'" EXIT

    say "Downloading ${archive_name}..."
    download "$archive_url" "${tmp_dir}/${archive_name}"

    say "Extracting..."
    tar xzf "${tmp_dir}/${archive_name}" -C "${tmp_dir}"

    local bin_path
    bin_path=$(find "${tmp_dir}" -name "${BIN_NAME}" -type f | head -1)
    if [ -z "$bin_path" ]; then
        err "Could not find '${BIN_NAME}' binary in the downloaded archive."
    fi

    chmod +x "$bin_path"

    local dest="${install_dir}/${BIN_NAME}"
    if [ -w "$install_dir" ]; then
        mv "$bin_path" "$dest"
    else
        say "Installing to ${dest} (requires sudo)..."
        sudo mv "$bin_path" "$dest"
    fi

    say "${BIN_NAME} ${version} installed successfully to ${dest}"

    # Warn if install_dir is not on PATH
    case ":${PATH}:" in
        *":${install_dir}:"*) ;;
        *)
            echo ""
            echo "  Note: ${install_dir} is not in your PATH."
            echo "  Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
            echo "    export PATH=\"${install_dir}:\$PATH\""
            ;;
    esac

    echo ""
    echo "  Run '${BIN_NAME}' to get started."
}

main "$@"
