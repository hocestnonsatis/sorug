#!/usr/bin/env bash
# Run the sorug URL differential fuzzer continuously for 24 hours.
#
# Requirements: nightly Rust + cargo-fuzz (`cargo install cargo-fuzz`).
# Crashes land under fuzz/artifacts/url_fuzz/; corpus under fuzz/corpus/url_fuzz/.
#
# Usage (from repo root):
#   ./scripts/run_fuzz_24h.sh
#   JOBS=8 MAX_TOTAL_TIME=3600 ./scripts/run_fuzz_24h.sh   # override defaults
#
# cargo-fuzz exits when the harness panics (differential finding). This script
# restarts until the wall-clock budget is exhausted so a full 24h campaign can
# accumulate corpus + crash artifacts. Set STOP_ON_CRASH=1 to abort on first hit.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TARGET="${FUZZ_TARGET:-url_fuzz}"
MAX_TOTAL_TIME="${MAX_TOTAL_TIME:-86400}"   # 24h
MAX_LEN="${MAX_LEN:-4096}"                  # match harness MAX_INPUT_LEN
RSS_LIMIT_MB="${RSS_LIMIT_MB:-2048}"
DICT="${DICT:-fuzz/url_dict.txt}"
CORPUS_DIR="${CORPUS_DIR:-fuzz/corpus/${TARGET}}"
ARTIFACT_DIR="${ARTIFACT_DIR:-fuzz/artifacts/${TARGET}}"
SEED_DIR="${SEED_DIR:-fuzz/seed_corpus}"
LOG_DIR="${LOG_DIR:-fuzz/logs}"
JOBS="${JOBS:-}"
STOP_ON_CRASH="${STOP_ON_CRASH:-0}"

if [[ -z "$JOBS" ]]; then
  if command -v nproc >/dev/null 2>&1; then
    JOBS="$(nproc)"
  else
    JOBS=1
  fi
  if (( JOBS > 8 )); then
    JOBS=8
  fi
fi

mkdir -p "$CORPUS_DIR" "$ARTIFACT_DIR" "$LOG_DIR"

if [[ -d "$SEED_DIR" ]]; then
  shopt -s nullglob
  for f in "$SEED_DIR"/*; do
    base="$(basename "$f")"
    if [[ ! -e "$CORPUS_DIR/$base" ]]; then
      cp "$f" "$CORPUS_DIR/$base"
    fi
  done
  shopt -u nullglob
fi

if [[ ! -f "$DICT" ]]; then
  echo "error: dictionary not found: $DICT" >&2
  exit 1
fi

if ! cargo fuzz --help >/dev/null 2>&1; then
  echo "error: cargo-fuzz not installed. Run: cargo install cargo-fuzz" >&2
  exit 1
fi

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_FILE="${LOG_DIR}/${TARGET}_${STAMP}.log"
CRASH_ARCHIVE="${ARTIFACT_DIR}/archive_${STAMP}"
mkdir -p "$CRASH_ARCHIVE"

echo "==> sorug 24h fuzz campaign"
echo "    target:         $TARGET"
echo "    duration:       ${MAX_TOTAL_TIME}s"
echo "    jobs:           $JOBS"
echo "    max_len:        $MAX_LEN"
echo "    rss_limit_mb:   $RSS_LIMIT_MB"
echo "    stop_on_crash:  $STOP_ON_CRASH"
echo "    dict:           $DICT"
echo "    corpus:         $CORPUS_DIR"
echo "    artifacts:      $ARTIFACT_DIR"
echo "    crash archive:  $CRASH_ARCHIVE"
echo "    log:            $LOG_FILE"
echo

export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"

# Note: do not install a custom panic hook — libFuzzer must see panics as crashes.
START_EPOCH="$(date +%s)"
END_EPOCH=$((START_EPOCH + MAX_TOTAL_TIME))
RUN=0
CRASHES=0

archive_new_crashes() {
  shopt -s nullglob
  local moved=0
  for f in "$ARTIFACT_DIR"/crash-* "$ARTIFACT_DIR"/leak-* "$ARTIFACT_DIR"/timeout-*; do
    [[ -e "$f" ]] || continue
    mv -n "$f" "$CRASH_ARCHIVE/" 2>/dev/null || mv "$f" "$CRASH_ARCHIVE/"
    moved=1
  done
  shopt -u nullglob
  if (( moved )); then
    CRASHES=$((CRASHES + 1))
  fi
}

{
  echo "campaign start $(date -u -Iseconds)"
  while true; do
    NOW="$(date +%s)"
    REMAINING=$((END_EPOCH - NOW))
    if (( REMAINING <= 0 )); then
      break
    fi
    RUN=$((RUN + 1))
    echo
    echo "==> run #${RUN}  remaining=${REMAINING}s  crashes_so_far=${CRASHES}"

    set +e
    cargo +nightly fuzz run "$TARGET" -- \
      -max_total_time="$REMAINING" \
      -max_len="$MAX_LEN" \
      -rss_limit_mb="$RSS_LIMIT_MB" \
      -dict="$DICT" \
      -jobs="$JOBS" \
      -workers="$JOBS"
    STATUS=$?
    set -e

    archive_new_crashes

    NOW="$(date +%s)"
    REMAINING=$((END_EPOCH - NOW))
    if (( STATUS == 0 )); then
      # libFuzzer may exit 0 early (worker teardown, external signal, merge).
      # Only treat as done when the wall-clock budget is actually exhausted.
      if (( REMAINING <= 5 )); then
        echo "==> fuzzer exited cleanly (time budget exhausted)"
        break
      fi
      echo "==> fuzzer exited status=0 early; remaining=${REMAINING}s — restarting"
      sleep 1
      continue
    fi

    echo "==> fuzzer exited status=${STATUS} (likely finding); archived under ${CRASH_ARCHIVE}"
    if [[ "$STOP_ON_CRASH" == "1" ]]; then
      echo "==> STOP_ON_CRASH=1 — aborting campaign"
      exit "$STATUS"
    fi
    # Brief pause so tight crash loops cannot peg the machine.
    sleep 1
  done

  echo
  echo "campaign end $(date -u -Iseconds)"
  echo "runs=${RUN} crash_batches=${CRASHES}"
  echo "corpus: $(find "$CORPUS_DIR" -type f 2>/dev/null | wc -l) files"
  echo "archived crashes: $(find "$CRASH_ARCHIVE" -type f 2>/dev/null | wc -l) files"
} 2>&1 | tee "$LOG_FILE"

echo
echo "==> finished. Inspect crashes with:"
echo "    ls -la $CRASH_ARCHIVE"
echo "    cargo +nightly fuzz run $TARGET $CRASH_ARCHIVE/crash-* -- -runs=1"
