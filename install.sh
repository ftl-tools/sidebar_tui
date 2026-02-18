#!/usr/bin/env bash
# Build and install sidebar-tui (sb) locally and optionally on elate_container

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building and installing sidebar-tui locally ==="

# Increment patch version in Cargo.toml
CARGO_TOML="$SCRIPT_DIR/Cargo.toml"
CURRENT_VERSION=$(grep '^version = ' "$CARGO_TOML" | head -1 | sed 's/version = "\(.*\)"/\1/')
MAJOR=$(echo "$CURRENT_VERSION" | cut -d. -f1)
MINOR=$(echo "$CURRENT_VERSION" | cut -d. -f2)
PATCH=$(echo "$CURRENT_VERSION" | cut -d. -f3)
NEW_PATCH=$((PATCH + 1))
NEW_VERSION="$MAJOR.$MINOR.$NEW_PATCH"
sed -i '' "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" "$CARGO_TOML"
echo "Version: $NEW_VERSION"

echo "Building and installing to ~/.cargo/bin/sb..."
cargo install --path . --force

echo "Done. sb installed to ~/.cargo/bin/sb"

# Check if elate_container is running
if docker ps --filter "name=elate_container" --format "{{.Names}}" 2>/dev/null | grep -q "elate_container"; then
    echo ""
    echo "=== Installing on elate_container ==="

    CONTAINER_SRC_DIR="/tmp/sidebar_tui_src"

    # Create source directory in container
    docker exec elate_container mkdir -p "$CONTAINER_SRC_DIR"

    # Copy source files to container (excluding target directory and git)
    echo "Copying source files to container..."
    tar --exclude='target' --exclude='.git' --exclude='*.state' -cf - . | \
        docker exec -i elate_container tar -xf - -C "$CONTAINER_SRC_DIR"

    # Build in container
    echo "Building in container..."
    docker exec -w "$CONTAINER_SRC_DIR" elate_container \
        bash -c 'source ~/.cargo/env && cargo build --release'

    # Install to /usr/local/bin
    echo "Installing to /usr/local/bin/sb in container..."
    docker exec elate_container cp "$CONTAINER_SRC_DIR/target/release/sb" /usr/local/bin/sb
    docker exec elate_container chmod +x /usr/local/bin/sb

    # Clean up source directory
    docker exec elate_container rm -rf "$CONTAINER_SRC_DIR"

    # Verify installation
    VERSION=$(docker exec elate_container sb --version 2>/dev/null || echo "unknown")
    echo "Done. sb installed on elate_container: $VERSION"
else
    echo ""
    echo "Note: elate_container is not running. Skipping docker installation."
fi

echo ""
echo "=== Installation complete ==="
