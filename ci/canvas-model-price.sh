#!/usr/bin/env bash
# The price of the generate-once model, on our own output and against a
# twinned vanilla reference. `docs/DETERMINISM.md` §32.
#
# Two dumps of one seed, one window and one binary: the shipped isolated
# model and the shared-canvas model `RUSTMC_DECORATE_ISOLATED=0` restores.
# Diffed against each other (how much the model moved) and each against the
# reference pair, VANILLA FIRST (what the move cost or bought).
#
# The coverage refusal is run on every window before any figure is believed:
# `region-differ` silently drops chunks absent from either side.
#
# usage: ci/canvas-model-price.sh <seed> <radius> <cx> <cz> <ref> [twin]
set -uo pipefail
SEED="${1:?seed}"; R="${2:?radius}"; CX="${3:?cx}"; CZ="${4:?cz}"
REF="${5:?reference}"; TWIN="${6:-}"
# References are named on the command line; only ones the coverage refusal
# passes are quotable. `van-c6458` fails it on this window (171 of 225
# chunks absent) and is deliberately not in the default set.
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT" || exit 2
OUT="$HOME/rp-canvas-price"; LOG="$HOME/rp-logs/canvas-price-$SEED-$R-$CX-$CZ.log"
mkdir -p "$HOME/rp-logs"
LO_X=$((CX - R)); HI_X=$((CX + R)); LO_Z=$((CZ - R)); HI_Z=$((CZ + R))
{
  echo "=== CANVAS MODEL PRICE seed $SEED r=$R centre ($CX,$CZ) @ $(date -Is) ==="
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(no harness live)"
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -3
  D=./target/release/worldgen-dump; G=./target/release/region-differ
  for MODEL in isolated extent shared; do
    d="$OUT/$MODEL"; rm -rf "$d"
    echo "=== dump $MODEL ==="
    case "$MODEL" in
      shared)   RUSTMC_DECORATE_ISOLATED=0 "$D" full "$SEED" "$R" "$d" "$CX" "$CZ" 2>&1 | tail -3 ;;
      isolated) RUSTMC_DECORATE_ISOLATED=1 "$D" full "$SEED" "$R" "$d" "$CX" "$CZ" 2>&1 | tail -3 ;;
      extent)   env -u RUSTMC_DECORATE_ISOLATED "$D" full "$SEED" "$R" "$d" "$CX" "$CZ" 2>&1 | tail -3 ;;
    esac
    [ -d "$d/region" ] || { echo "$MODEL: dump failed"; exit 2; }
    echo "--- coverage refusal on $MODEL ---"
    ./ci/ref-chunk-extent.py --window "$LO_X" "$LO_Z" "$HI_X" "$HI_Z" "$d/region"
    echo "coverage exit $?"
  done
  for M in extent isolated; do
    echo "=== ours(shared) vs ours($M): what the model moved ==="
    "$G" diff "$OUT/shared/region" "$OUT/$M/region" \
        --bbox "$LO_X" "$LO_Z" "$HI_X" "$HI_Z" 2>&1 | grep -E 'DIFF|EXACTNESS'
  done
  for T in $REF $TWIN; do
    echo "=== coverage refusal on reference $T ==="
    ./ci/ref-chunk-extent.py --window "$LO_X" "$LO_Z" "$HI_X" "$HI_Z" "$HOME/rustmc/oracle-work/$T"
    echo "coverage exit $?"
    for MODEL in shared extent isolated; do
      echo "=== diff VANILLA FIRST: $T vs ours($MODEL) ==="
      "$G" diff "$HOME/rustmc/oracle-work/$T" "$OUT/$MODEL/region" \
          --bbox "$LO_X" "$LO_Z" "$HI_X" "$HI_Z" 2>&1 | grep -E 'DIFF|EXACTNESS'
    done
  done
  echo "=== CANVAS MODEL PRICE DONE ==="
} > "$LOG" 2>&1
echo "$LOG"
