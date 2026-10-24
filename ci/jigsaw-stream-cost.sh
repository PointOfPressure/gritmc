#!/usr/bin/env bash
# WHAT THE PIECE STREAM COSTS IN BLOCK STATES — the measurement the decision to
# thread it turns on, and the one that has to be allowed to come back "no".
#
# The claim under test is not "should the stream be threaded". It is:
#
#     threading the jigsaw driver's piece stream is a DRAW-ORDER change, and
#     every published floor on the r=16 window rests on draw order
#
# and the answer this script measures is a number of BLOCK STATES, taken three
# ways off one binary so the arms cannot differ by anything but the knob:
#
#     off       RUSTMC_JIGSAW_STREAM unset  — the code that shipped
#     stream    RUSTMC_JIGSAW_STREAM=1      — the draws, and nothing else
#     faithful  RUSTMC_JIGSAW_STREAM=2      — the draws plus vanilla's two
#                                             start-walk orders
#
# THE PRIMARY FIGURE IS `off` AGAINST `stream`, OURS AGAINST OURS. It needs no
# reference and no floor: both arms are this generator, so a single block of
# difference is a real block of difference and zero is zero. That is the only
# comparison in this file that can be read as an exact quantity. Everything
# else here is scored VANILLA FIRST against the stamped ensemble and carries
# its floor.
#
# WHY A ZERO WOULD BE A FINDING RATHER THAN A FAILED MEASUREMENT, and how this
# script keeps that distinction. A pass with no draws in it produces a zero for
# the same reason a pass with no *effect* does, so a bare zero is
# indistinguishable from a knob that never turned. Two positive controls, both
# printed, both required before the zero means anything:
#
#   1. **The knob is observed to turn.** `RUSTMC_JIGSAW_TRACE=1` makes
#      `place_jigsaw_starts` print one line per structure group naming the
#      structure, the step, the derived index and the draws the group spent.
#      A `stream` arm whose total draws is 0 is REFUSED — there was nothing to
#      cost, and reporting "threading is free" off that would be the
#      unfalsifiable-check class this repo is made of.
#   2. **The instrument is observed to bite.** The `faithful` arm changes the
#      order two overlapping starts are written in. If `faithful` also comes
#      back 0 against `off`, that is reported as such rather than as
#      corroboration — but the `bediff` arm still has to move, because the
#      draws must reach the containers.
#
# usage: ci/jigsaw-stream-cost.sh <label>
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
LABEL="${1:?need a label}"
SEED=12345
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
LOG="$HOME/rp-logs/jigsaw-stream-$LABEL.log"
POSDIR="$HOME/rp-logs/jigsaw-stream-$LABEL-pos"
mkdir -p "$HOME/rp-logs" "$POSDIR"
REFROOT="$HOME/rustmc/oracle-work"
PFX="$REFROOT/gen-wide-12345-r16-ow"
# The same stamped ensemble `ci/ow-r16-window-12345.sh` adjudicates on, and the
# same exclusion of `a` and `b`: they carry no `HARVEST-ORDER` stamp, so their
# order is stated nowhere and matching to it would be matching to a guess.
ROWS="c d e f g h"
BBOX="--bbox 48 48 80 80"

for r in $ROWS; do
  [ -d "$PFX-$r" ] || { echo "missing reference $PFX-$r" >&2; exit 2; }
done

# THE MATCHED DUMP ORDER, READ OFF THE REFERENCES AND NEVER ASSERTED.
# ORACLE rule 29. An unstamped anchor is a refusal, not a default.
MATCHED=""
for r in $ROWS; do
  ho="$PFX-$r/HARVEST-ORDER"
  [ -s "$ho" ] || { echo "$PFX-$r carries no HARVEST-ORDER stamp — REFUSING (ORACLE rule 29)" >&2; exit 2; }
  d=$(grep -m1 -oP '^dump_order=\K.*' "$ho")
  case "$d" in zx|zx_zrev) ;; *) echo "$PFX-$r stamps dump_order='$d' — REFUSING" >&2; exit 2;; esac
  if [ -z "$MATCHED" ]; then MATCHED="$d"
  elif [ "$MATCHED" != "$d" ]; then
    echo "the rows ensemble mixes harvest orders ($MATCHED and $d) — REFUSING" >&2; exit 2
  fi
done

{
  echo "=== JIGSAW PIECE STREAM COST: $LABEL @ $(date -Is) ==="
  echo "seed $SEED, centre chunk (64,64), r=16, 1,089 chunks, bbox 48 48 80 80"
  echo "matched dump order (off the references' own stamps): RUSTMC_DECORATE_ORDER=$MATCHED"
  echo "--- harnesses live at launch (count them; do NOT read the load average) ---"
  pgrep -af "[o]racle-runner|[t]2-gameplay|[h]andoff" || echo "(none)"
  echo "--- tree marker ---"
  cat "$ROOT/.rp-commit" 2>/dev/null || git -C "$ROOT" log --oneline -1 2>/dev/null \
    || echo "(no marker: rsynced trees carry no .git)"

  echo "=== build ==="
  cargo build --release -p worldgen-dump -p region-differ 2>&1 | tail -3
  [ -x ./target/release/worldgen-dump ] || { echo "FAIL: no worldgen-dump binary"; exit 3; }

  # One binary, three arms. The dumps are taken in knob order so a partial run
  # still leaves `off` and `stream` comparable.
  for arm in off stream faithful; do
    case "$arm" in
      off)      KNOB="" ;;
      stream)   KNOB="1" ;;
      faithful) KNOB="2" ;;
    esac
    OUT="$HOME/rp-jigsaw-$LABEL-$arm"
    rm -rf "$OUT"
    echo "=== dump arm=$arm RUSTMC_JIGSAW_STREAM='${KNOB}' ==="
    RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER="$MATCHED" RUSTMC_JIGSAW_STREAM="$KNOB" \
      RUSTMC_JIGSAW_TRACE=1 \
      ./target/release/worldgen-dump full "$SEED" 16 "$OUT" 64 64 \
        > "$POSDIR/dump-$arm.txt" 2>&1
    tail -6 "$POSDIR/dump-$arm.txt"
    echo "--- POSITIVE CONTROL 1: the draws this arm actually spent ---"
    grep -c '^JIGSAW-STREAM' "$POSDIR/dump-$arm.txt" | sed 's/^/  trace lines: /'
    awk '/^JIGSAW-STREAM/ {for(i=1;i<=NF;i++) if($i ~ /^draws=/) {split($i,a,"="); t+=a[2]}} \
         END {printf "  draws spent: %d\n", t+0}' "$POSDIR/dump-$arm.txt"
    grep -m8 '^JIGSAW-STREAM' "$POSDIR/dump-$arm.txt" || echo "  (no trace lines)"
  done

  OFF="$HOME/rp-jigsaw-$LABEL-off/region"
  STR="$HOME/rp-jigsaw-$LABEL-stream/region"
  FTH="$HOME/rp-jigsaw-$LABEL-faithful/region"

  echo
  echo "############################################################"
  echo "# THE PRIMARY FIGURE — ours against ours, no reference, no floor."
  echo "# Both arms are this generator, so this is an exact quantity and a"
  echo "# zero here is a zero. Order is off-first only because a delta has no"
  echo "# sign convention to get wrong when both sides are the same tree."
  echo "############################################################"
  echo "=== off vs stream (the draws alone) ==="
  ./target/release/region-differ diff "$OFF" "$STR" $BBOX 2>&1 | tail -14
  echo "=== off vs faithful (the draws plus vanilla's two start orders) ==="
  ./target/release/region-differ diff "$OFF" "$FTH" $BBOX 2>&1 | tail -14
  echo "=== stream vs faithful (the orders alone) ==="
  ./target/release/region-differ diff "$STR" "$FTH" $BBOX 2>&1 | tail -14

  echo
  echo "=== BYTE-DISTINCTNESS OF THE THREE DUMPS ==="
  echo "# Three arms that produced byte-identical region trees would make every"
  echo "# diff above trivially zero. Printed so the zero cannot be read off an"
  echo "# instrument that never ran three times."
  for pair in "off stream" "off faithful" "stream faithful"; do
    set -- $pair
    a="$HOME/rp-jigsaw-$LABEL-$1/region"; b="$HOME/rp-jigsaw-$LABEL-$2/region"
    if diff -rq "$a" "$b" >/dev/null 2>&1; then
      echo "  $1 vs $2: region trees are BYTE-IDENTICAL"
    else
      echo "  $1 vs $2: region trees differ on disk ($(diff -rq "$a" "$b" 2>/dev/null | wc -l) files)"
    fi
  done

  echo
  echo "############################################################"
  echo "# VANILLA FIRST, against the stamped ensemble, with the floor."
  echo "# THE FLOOR IS NEVER SUBTRACTED. It is the charge this instrument"
  echo "# lays on a known-innocent candidate and it is printed beside ours."
  echo "############################################################"
  REF_A="$PFX-c"; REF_B="$PFX-d"
  ./target/release/region-differ diff "$REF_A" "$REF_B" $BBOX --positions \
      > "$POSDIR/floor.pos" 2>&1
  echo "=== TWIN FLOOR (N=2, harvest rows, no dump order involved): vanilla c vs vanilla d ==="
  ./target/release/region-differ diff "$REF_A" "$REF_B" $BBOX 2>&1 | tail -12
  for arm in off stream faithful; do
    OUTR="$HOME/rp-jigsaw-$LABEL-$arm/region"
    ./target/release/region-differ diff "$REF_A" "$OUTR" $BBOX --positions \
        > "$POSDIR/a-vs-$arm.pos" 2>&1
    ./target/release/region-differ diff "$REF_B" "$OUTR" $BBOX --positions \
        > "$POSDIR/b-vs-$arm.pos" 2>&1
    echo "=== ADMISSIBLE at N=2, arm=$arm — A DELTA BASELINE, NOT A LEVEL ==="
    echo "# N=2 on this window is the regime where the innocent floor alone"
    echo "# spans thousands of blocks. Against a FIXED reference set the charge"
    echo "# is a deterministic function of our output, so the DIFFERENCE"
    echo "# between two arms here is real; the absolute value is not a level."
    python3 "$ROOT/ci/admissible-residual.py" \
        "$POSDIR/a-vs-$arm.pos" "$POSDIR/b-vs-$arm.pos" --floor "$POSDIR/floor.pos" 2>&1
  done

  echo
  echo "############################################################"
  echo "# THE BLOCK-ENTITY SIDE, vanilla first — what the draws buy."
  echo "############################################################"
  for arm in off stream faithful; do
    OUTR="$HOME/rp-jigsaw-$LABEL-$arm/region"
    echo "=== bediff arm=$arm (vanilla $PFX-c first) ==="
    ./target/release/region-differ bediff "$REF_A" "$OUTR" $BBOX 2>&1 | tail -8
  done

  echo "=== JIGSAW PIECE STREAM COST DONE: $LABEL ==="
} > "$LOG" 2>&1
echo "log: $LOG"
