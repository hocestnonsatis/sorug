#!/usr/bin/env bash
# Refresh in-tree Web Platform Tests URL fixtures.
# Usage: ./scripts/refresh-wpt.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/tests"
BASE="https://raw.githubusercontent.com/web-platform-tests/wpt/master/url/resources"

mkdir -p "$DEST"

echo "Fetching WPT urltestdata.json → $DEST/urltestdata.json"
curl -fsSL -o "$DEST/urltestdata.json" "${BASE}/urltestdata.json"

echo "Fetching WPT setters_tests.json → $DEST/setters_tests.json"
curl -fsSL -o "$DEST/setters_tests.json" "${BASE}/setters_tests.json"

echo "Done. Checklist:"
echo "  1. cargo test --test wpt --test wpt_setters"
echo "  2. cargo test"
echo "  3. Commit fixture updates together with any harness fixes"
echo "  4. Update WPT badge counts in README if totals change"
