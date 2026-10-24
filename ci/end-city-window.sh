#!/usr/bin/env bash
# The End city window, re-run. AGENT-BRIEF: anything touching `place_piece` or
# `rotate_state` must re-run this before handing off, because no gate in
# `ci/t0.sh` would catch a regression there.
#
# Seed 20260722, centre chunk (83,62), r=7, y 0..255, denominator 14,745,600,
# VANILLA FIRST, against van-end-20260722-c8362 and its -b twin.
#
# usage: .end-window.sh <label>
set -uo pipefail
LABEL="${1:?need a label}"
OUT="$HOME/rp-end-$LABEL"
LOG="$HOME/rp-logs/end-$LABEL.log"
mkdir -p "$HOME/rp-logs"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

{
  echo "=== END WINDOW: $LABEL @ $(date -Is) ==="
  echo "--- harnesses live at launch (count them; do NOT read load average) ---"
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(none)"
  echo "--- tree marker (rsynced, no .git) ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null || echo "(no marker: rsynced trees carry no .git)"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -3

  rm -rf "$OUT"
  echo "=== dump full-end seed 20260722 r=7 centre (83,62) ==="
  time ./target/release/worldgen-dump full-end 20260722 7 "$OUT" 83 62 2>&1 | tail -5

  for TWIN in van-end-20260722-c8362 van-end-20260722-c8362-b; do
    echo "=== diff VANILLA FIRST: $TWIN vs ours ==="
    ./target/release/region-differ diff \
        "$HOME/rustmc/oracle-work/$TWIN" "$OUT/region" \
        --bbox 76 55 90 69 --ybox 0 255 2>&1 | tail -12
  done
  echo "=== END WINDOW DONE: $LABEL ==="
} > "$LOG" 2>&1
