#!/usr/bin/env bash
# Refresh vendored Unicode UCD files for sorug IDNA table generation.
# Usage: ./scripts/refresh-ucd.sh [VERSION]
# Default VERSION is read from data/ucd/UNICODE_VERSION or 16.0.0.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/data/ucd"
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  if [[ -f "$DEST/UNICODE_VERSION" ]]; then
    VERSION="$(tr -d '[:space:]' <"$DEST/UNICODE_VERSION")"
  else
    VERSION="16.0.0"
  fi
fi

BASE_UCD="https://www.unicode.org/Public/${VERSION}/ucd"
BASE_IDNA="https://www.unicode.org/Public/idna/${VERSION}"

mkdir -p "$DEST"
echo "$VERSION" >"$DEST/UNICODE_VERSION"

echo "Fetching Unicode ${VERSION} → $DEST"
curl -fsSL -o "$DEST/DerivedBidiClass.txt" "${BASE_UCD}/extracted/DerivedBidiClass.txt"
curl -fsSL -o "$DEST/Scripts.txt" "${BASE_UCD}/Scripts.txt"
curl -fsSL -o "$DEST/DerivedJoiningType.txt" "${BASE_UCD}/extracted/DerivedJoiningType.txt"
curl -fsSL -o "$DEST/IdnaMappingTable.txt" "${BASE_IDNA}/IdnaMappingTable.txt"

echo "Done. Checklist:"
echo "  1. cargo test"
echo "  2. cargo test --test wpt --test wpt_setters"
echo "  3. Review data/idna_overlay.txt if Node/WPT deltas appear"
echo "  4. Commit data/ucd/ + overlay changes together"
