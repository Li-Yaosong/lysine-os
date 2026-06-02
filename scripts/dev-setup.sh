#!/usr/bin/env bash
# LysineOS development environment setup script
# Usage: bash scripts/dev-setup.sh

set -euo pipefail

GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }

info "=== LysineOS Development Environment Setup ==="

# --- OS detection ---
detect_os() {
    if [[ -f /etc/os-release ]]; then
        # shellcheck disable=SC1091
        source /etc/os-release
        echo "$ID"
    elif [[ "$(uname)" == "Darwin" ]]; then
        echo "macos"
    else
        echo "unknown"
    fi
}

OS=$(detect_os)
info "Detected OS: ${OS}"

# --- System packages ---
case "$OS" in
    ubuntu|debian)
        info "Installing system packages..."
        sudo apt-get update
        sudo apt-get install -y --no-install-recommends \
            curl ca-certificates build-essential pkg-config \
            libarchive-dev libsodium-dev libbtrfs-dev \
            binutils bison gawk m4 texinfo \
            pandoc git jq ripgrep
        ;;
    fedora)
        info "Installing system packages..."
        sudo dnf install -y \
            curl ca-certificates gcc make pkg-config \
            libarchive-devel libsodium-devel btrfs-progs-devel \
            binutils bison gawk m4 texinfo \
            pandoc git jq ripgrep
        ;;
    arch|manjaro)
        info "Installing system packages..."
        sudo pacman -S --noconfirm --needed \
            curl ca-certificates base-devel pkg-config \
            libarchive libsodium btrfs-progs \
            binutils bison gawk m4 texinfo \
            pandoc git jq ripgrep
        ;;
    macos)
        warn "macOS detected. Some LFS-specific tools may not be available."
        info "Installing packages via Homebrew..."
        if ! command -v brew &> /dev/null; then
            warn "Homebrew not found. Please install it first: https://brew.sh"
            exit 1
        fi
        brew install pkg-config libsodium pandoc jq ripgrep
        ;;
    *)
        warn "Unsupported OS: ${OS}. Please install dependencies manually."
        ;;
esac

# --- Rust toolchain ---
if ! command -v rustc &> /dev/null; then
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "${HOME}/.cargo/env"
fi

REQUIRED_MAJOR=1
REQUIRED_MINOR=90
RUST_VERSION_STR=$(rustc --version | grep -oP '\d+\.\d+\.\d+' | head -1)
RUST_MAJOR=$(echo "$RUST_VERSION_STR" | cut -d. -f1)
RUST_MINOR=$(echo "$RUST_VERSION_STR" | cut -d. -f2)

if [[ "$RUST_MAJOR" -lt "$REQUIRED_MAJOR" ]] || \
   [[ "$RUST_MAJOR" -eq "$REQUIRED_MAJOR" && "$RUST_MINOR" -lt "$REQUIRED_MINOR" ]]; then
    info "Updating Rust to ${REQUIRED_MAJOR}.${REQUIRED_MINOR}+..."
    rustup update stable && rustup default stable
fi

info "Installing Rust components..."
rustup component add rustfmt clippy rust-analyzer rust-src

# --- Verification ---
info "=== Verification ==="
echo "Rust:       $(rustc --version)"
echo "Cargo:      $(cargo --version)"
echo "rustfmt:    $(cargo fmt --version 2>/dev/null || echo 'N/A')"
echo "clippy:     $(cargo clippy --version 2>/dev/null || echo 'N/A')"

# --- Fetch dependencies ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

if [[ -f "${PROJECT_ROOT}/ribosome/Cargo.toml" ]]; then
    info "Fetching Rust dependencies..."
    (cd "${PROJECT_ROOT}/ribosome" && cargo fetch)
fi

info "=== Setup Complete ==="
