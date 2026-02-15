#!/usr/bin/env bash
# Build and install sidebar-tui (sb) locally to ~/.cargo/bin

set -e

cd "$(dirname "$0")"

echo "Building sidebar-tui..."
cargo build --release

echo "Installing to ~/.cargo/bin/sb..."
cargo install --path . --force

echo "Done. sb installed to ~/.cargo/bin/sb"
