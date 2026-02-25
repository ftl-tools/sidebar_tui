#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 <version>"
  echo "  Example: $0 0.1.12"
  exit 1
}

[[ $# -eq 1 ]] || usage
VERSION="$1"
TAG="v${VERSION}"

# Validate semver format
if ! [[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: version must be in semver format (e.g. 0.1.12)"
  exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# Ensure working tree is clean
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Error: working tree has uncommitted changes. Commit or stash them first."
  exit 1
fi

# Ensure tag doesn't already exist
if git rev-parse "$TAG" &>/dev/null; then
  echo "Error: tag $TAG already exists."
  exit 1
fi

echo "Bumping version to $VERSION..."

# Bump Cargo.toml
CURRENT=$(grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
sed -i '' "s/^version = \"${CURRENT}\"/version = \"${VERSION}\"/" Cargo.toml

# Bump all npm package.json files
for pkg_json in npm/*/package.json; do
  # Update "version" field
  node -e "
    const fs = require('fs');
    const p = JSON.parse(fs.readFileSync('${pkg_json}', 'utf8'));
    p.version = '${VERSION}';
    if (p.optionalDependencies) {
      for (const dep of Object.keys(p.optionalDependencies)) {
        p.optionalDependencies[dep] = '${VERSION}';
      }
    }
    fs.writeFileSync('${pkg_json}', JSON.stringify(p, null, 2) + '\n');
  "
done

# Sync Cargo.lock
cargo update --workspace --quiet

echo "Committing..."
git add Cargo.toml Cargo.lock npm/*/package.json
git commit -m "chore: bump version to ${VERSION}"

echo "Tagging $TAG..."
git tag "$TAG"

echo ""
echo "Ready to push. Run:"
echo "  git push && git push --tags"
echo ""
echo "Or to push now:"
echo "  $0 will not push automatically — review the commit first with: git log --oneline -3"
