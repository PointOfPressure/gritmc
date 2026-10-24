#!/usr/bin/env bash
# be-census-12345.sh — the block-entity census, and its floor, on the one
# overworld window that carries a quotable exactness figure.
#
# WHAT THIS IS FOR, AND WHY IT IS A CENSUS BEFORE IT IS A PORT.
#
# `docs/PARITY.md` §6 names the largest hole in worldgen parity in a single
# line: *"Anything that is not a block state. Block entities are not written,
# so no chest contents, no spawner contents, no sign text."* Every published
# worldgen exactness figure — the quotable seed-12345 r=16 row included —
# compares block states and is silent here **by construction**: a chest whose
# state is right and whose contents are absent scores as a match.
#
# Nothing measured the size of that hole. This script does, and it is landed
# before any generation change on purpose, so the number it prints is a
# property of vanilla rather than a scorecard for us. If the implementation
# that follows stalls, this is still the durable artefact: it is what tells
# anyone whether the rest was worth doing.
#
# THE WINDOW is the one from `ci/ow-r16-window-12345.sh`, unchanged, so that
# the block-entity count and the block-state count share a denominator's
# worth of geometry and can be quoted in the same sentence: seed 12345,
# overworld, r=16, centre chunk (64,64), 33x33 = 1,089 chunks, bbox
# 48 48 80 80, y unclipped.
#
# THE ENSEMBLE is the sixteen same-order references c..r behind
# `ow-r16-12345-floor-n15`. Every one is checked for its own `HARVEST-ORDER`
# stamp and refused without one — ORACLE rule 30, an assertion is not a stamp.
# Block entities are read out of the chunk NBT the harvest already wrote, so
# **this script needs no vanilla boot and does not take the corpus lock.** It
# is the `ci/corpus-floor.sh` trick applied to a different tag: the expensive
# part was paid for when the references were generated.
#
# THE FLOOR IS NOT ASSUMED TO BE ZERO. Two unmodified vanilla runs of one seed
# disagree about block states, and there is no reason to expect them to agree
# about block entities: the threaded decoration order that moves a lake moves
# the dungeon inside it. So the pair sweep runs vanilla-against-vanilla over
# every unordered pair in the pool and reports the spread. **The floor is
# printed beside the census and is never subtracted from anything.**
#
# WHAT IT CANNOT TELL YOU. This is one seed, one window, one dimension. The
# reachable share it reports is reachable *here*: a structure set with no
# start inside these 1,089 chunks contributes nothing to either column, which
# is a fact about the window and not about the structure set.
#
# THE REACHABLE SHARE IS MEASURED POSITIONALLY, NOT BY COUNTING. `reach`
# walks vanilla's block-entity positions and asks what block WE have at each
# one. Reading the ceiling off two censuses instead — vanilla places 32
# potent_sulfur, we place 31, so 31 are reachable — is the mistake this repo
# keeps paying for: two counts agree perfectly while every position differs.
# It needs a dump of ours, so it is the one stage that is not free; skip it
# with `ci/be-census-12345.sh census` or `... floor` if you only want the
# vanilla-side numbers.
#
# WHAT IT READ ON 2026-08-16, branch worldgen-block-entities, Threadripper,
# anchor reference `c`, our dump at RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER=zx:
#
#   census:  173 block entities in 1,089 chunks, 10 distinct ids, 0 malformed
#   floor:   0..13 positions, median 5, n=120 pairs over 16 references,
#            with id=0 and payload=0 in EVERY pair
#   reach:   160 of 173, 92.486%
#
# AND WHAT THE FIRST IMPLEMENTED CLASS THEN READ, same day, same window,
# `region-differ bediff --match potent_sulfur`, vanilla first:
#
#   payload 28 of 28 byte-identical, floor payload=0 across 120 pairs
#   presence charge 7 of a 35-position union, floor 0..12 median 4 n=120
#
# Those two are one class scored on two axes and only the first resolves.
# `docs/DETERMINISM.md` §37 is why, and the short version is that the content
# floor is exactly zero while the presence floor is nearly the whole of this
# window's block-entity noise.
#
# `docs/DETERMINISM.md` §36 is the write-up and `ci/figures.toml` registers
# all three. The floor line is the one to read twice: no pair of independent
# vanilla runs has ever disagreed about the CONTENT of a co-located block
# entity, only about whether one is there at all.
#
# usage: ci/be-census-12345.sh [census|floor|reach|all]
#   BE_OURS=<dir> points `reach` at a dump's region directory.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

STAGE=${1:-all}
REFROOT=${BE_REFROOT:-$HOME/rustmc/oracle-work}
PFX="$REFROOT/gen-wide-12345-r16-ow"
POOL=${BE_POOL:-"c d e f g h i j k l m n o p q r"}
BBOX="48 48 80 80"
G=./target/release/region-differ

[ -x "$G" ] || { echo "build first: cargo build --release -p region-differ" >&2; exit 2; }

# ORACLE rule 30. A reference with no stamp has a *recoverable* order, not a
# known one, and an ensemble anchored on an unstamped reference is an
# ensemble whose order is a guess. `a` and `b` are deliberately not in the
# pool for exactly this reason; see ci/classify-12345-ab.sh.
MATCHED=""
for r in $POOL; do
  [ -d "$PFX-$r" ] || { echo "missing reference $PFX-$r" >&2; exit 2; }
  ho="$PFX-$r/HARVEST-ORDER"
  [ -s "$ho" ] || { echo "$PFX-$r carries no HARVEST-ORDER stamp — REFUSING (ORACLE rule 29)" >&2; exit 2; }
  g=$(grep -m1 '^gen_order=' "$ho" | cut -d= -f2)
  if [ -z "$MATCHED" ]; then MATCHED="$g"
  elif [ "$MATCHED" != "$g" ]; then
    echo "the pool mixes harvest orders ($MATCHED and $g) — REFUSING" >&2; exit 2
  fi
done
echo "pool: $POOL"
echo "every reference stamps gen_order=$MATCHED"
echo "window: seed 12345, overworld, r=16, centre (64,64), 1089 chunks, bbox $BBOX"
echo

ANCHOR=${BE_ANCHOR:-c}

if [ "$STAGE" = census ] || [ "$STAGE" = all ]; then
  echo "=== CENSUS — vanilla $PFX-$ANCHOR ==="
  $G becensus "$PFX-$ANCHOR" --bbox $BBOX --fields
  echo
fi

if [ "$STAGE" = reach ] || [ "$STAGE" = all ]; then
  OURS=${BE_OURS:-}
  if [ -z "$OURS" ]; then
    echo "=== REACH — skipped, set BE_OURS=<our dump>/region ==="
  elif [ ! -d "$OURS" ]; then
    echo "BE_OURS=$OURS is not a directory — REFUSING" >&2; exit 2
  else
    # VANILLA FIRST, as everywhere else in this tool.
    echo "=== REACH — vanilla $PFX-$ANCHOR against ours $OURS ==="
    $G bereach "$PFX-$ANCHOR" "$OURS" --bbox $BBOX
  fi
  echo
fi

if [ "$STAGE" = floor ] || [ "$STAGE" = all ]; then
  echo "=== FLOOR — every unordered pair, vanilla against vanilla ==="
  # Vanilla first on both sides is vacuously true here, but the ORDER of the
  # two arguments still decides which column is `missing` and which is
  # `spurious`; over a symmetric sweep they swap, which is itself a check
  # that the classifier is not favouring one side.
  set -- $POOL
  n=0
  for a in $POOL; do
    for b in $POOL; do
      [ "$a" \< "$b" ] || continue
      line=$($G bediff "$PFX-$a" "$PFX-$b" --bbox $BBOX | sed -n '1,3p' | tr '\n' ' ')
      echo "PAIR $a $b  $line"
      n=$((n + 1))
    done
  done
  echo "PAIRS $n"
fi
