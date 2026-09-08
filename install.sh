#!/usr/bin/env bash
# Build and install sidebar-tui (sb) locally and optionally on elate_container_1

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

CONTAINER_NAME="elate_container_1"
# The old partial-name check matched elate_container_1 but then executed against
# elate_container. Use one exact container name for detection and every command.
if docker ps --format "{{.Names}}" 2>/dev/null | grep -Fxq "$CONTAINER_NAME"; then
    echo ""
    echo "=== Installing on $CONTAINER_NAME ==="

    CONTAINER_SRC_DIR="/tmp/sidebar_tui_src"

    # Create source directory in container
    docker exec "$CONTAINER_NAME" mkdir -p "$CONTAINER_SRC_DIR"

    # Copy source files to container (excluding target directory and git)
    echo "Copying source files to container..."
    tar --exclude='target' --exclude='.git' --exclude='*.state' -cf - . | \
        docker exec -i "$CONTAINER_NAME" tar -xf - -C "$CONTAINER_SRC_DIR"

    # Build in container
    echo "Building in container..."
    docker exec -w "$CONTAINER_SRC_DIR" "$CONTAINER_NAME" \
        bash -c 'source ~/.cargo/env && cargo build --release'

    # A direct copy over a running executable failed with "Text file busy".
    # Stage the binary and atomically rename it so active sessions can keep running.
    echo "Installing to /usr/local/bin/sb in container..."
    docker exec "$CONTAINER_NAME" cp "$CONTAINER_SRC_DIR/target/release/sb" /usr/local/bin/sb.new
    docker exec "$CONTAINER_NAME" chmod +x /usr/local/bin/sb.new
    docker exec "$CONTAINER_NAME" mv -f /usr/local/bin/sb.new /usr/local/bin/sb

    # Clean up source directory
    docker exec "$CONTAINER_NAME" rm -rf "$CONTAINER_SRC_DIR"

    # Verify installation
    VERSION=$(docker exec "$CONTAINER_NAME" sb --version 2>/dev/null || echo "unknown")
    echo "Done. sb installed on $CONTAINER_NAME: $VERSION"
else
    echo ""
    echo "Note: $CONTAINER_NAME is not running. Skipping docker installation."
fi

echo ""
echo "=== Installation complete ==="
