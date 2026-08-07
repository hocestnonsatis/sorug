#!/usr/bin/env bash
# Refresh vendored Unicode UCD files for sorug IDNA table generation.
# Usage: ./scripts/refresh-ucd.sh [VERSION]
# Default VERSION is read from data/ucd/UNICODE_VERSION or 16.0.0.
#
# Layout notes:
# - UCD extract files: Public/<ver>/ucd/...
# - IdnaMappingTable: Public/idna/<ver>/... for ≤16; Public/<ver>/idna/... for 17+
#   (script tries both).
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
# Prefer versioned Public/<ver>/idna (Unicode 17+); fall back to Public/idna/<ver>.
IDNA_CANDIDATES=(
  "https://www.unicode.org/Public/${VERSION}/idna/IdnaMappingTable.txt"
  "https://www.unicode.org/Public/idna/${VERSION}/IdnaMappingTable.txt"
)

mkdir -p "$DEST"
echo "$VERSION" >"$DEST/UNICODE_VERSION"

echo "Fetching Unicode ${VERSION} → $DEST"
curl -fsSL -o "$DEST/DerivedBidiClass.txt" "${BASE_UCD}/extracted/DerivedBidiClass.txt"
curl -fsSL -o "$DEST/Scripts.txt" "${BASE_UCD}/Scripts.txt"
curl -fsSL -o "$DEST/DerivedJoiningType.txt" "${BASE_UCD}/extracted/DerivedJoiningType.txt"

IDNA_OK=0
for url in "${IDNA_CANDIDATES[@]}"; do
  echo "Trying IdnaMappingTable: $url"
  if curl -fsSL -o "$DEST/IdnaMappingTable.txt" "$url"; then
    IDNA_OK=1
    break
  fi
done
if [[ "$IDNA_OK" -ne 1 ]]; then
  echo "error: could not download IdnaMappingTable.txt for ${VERSION}" >&2
  exit 1
fi

echo "Done. Checklist:"
echo "  1. cargo test"
echo "  2. cargo test --test wpt --test wpt_setters"
echo "  3. Review data/idna_overlay.txt if Node/WPT deltas appear"
echo "  4. Commit data/ucd/ + overlay changes together"
