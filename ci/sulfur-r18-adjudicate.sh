#!/usr/bin/env bash
# sulfur-r18-adjudicate.sh — is the `sulfur -> cinnabar` charge on chunk
# (77,47) REAL, or was it the r=16 references measuring their own generation
# extent?
#
# THE QUESTION, AND WHY IT COULD NOT BE ASKED BEFORE
#
# 35 of the 49 blocks charged on chunk (77,47) read `sulfur -> cinnabar`
# against the forty-reference seed-12345 r=16 pool (docs/DETERMINISM.md 30).
# Every one of those forty references completes chunks 46..82 and no further,
# so (77,47) sits ONE ring inside the outermost completed ring. A chunk that
# close to the edge can differ for no reason but where the generated area
# stopped, and the pool cannot see it: the truncation is identical in all
# forty, so the leave-one-out floor is blind to exactly the artefact in
# question. Widening the window does not help either — the references do not
# hold a wider window, and `ci/ref-chunk-extent.py` refuses one.
#
# What settles it is a SECOND GENERATION at a larger radius, twinned so that
# ORACLE rule 6 has its precondition. `gen-wide-12345-r18-ow-x1` and `-x2` are
# two independent vanilla runs of seed 12345 at radius 18, identical in every
# HARVEST-ORDER parameter (`gen_order=rows`, `dump_order=zx`, centre 64,64,
# overworld), harvested 2026-08-11. They complete chunks 44..84, which puts
# (77,47) THREE rings inside the completed edge instead of one.
#
# THE FLOOR IS NOT INHERITED. The r=16 adjudication's floor was measured on
# that pool and belongs to it — "a floor belongs to the pair of runs that
# produced it, not to the seed and the window" (AGENT-BRIEF). The floor here is
# derived from the twins themselves: their disagreement with each other IS the
# floor for this comparison, and it is reported beside every figure and never
# subtracted.
#
# THREE CONDITIONS THIS SCRIPT CHECKS RATHER THAN ASSUMES
#
#  1. COVERAGE. `region-differ` silently drops chunks absent from either side,
#     so an under-covered window measures its own input and reports success.
#     Stage R demonstrates that failure and the refusal that now stops it.
#  2. OUR OWN EXTENT. `DECORATION_MARGIN` of 2 does not converge, so our dump
#     has a generation-extent artefact too. Stage C measures how far it reaches
#     and shows (77,47) is outside it.
#  3. VANILLA FIRST in every charge diff, without exception.
#
# usage: sulfur-r18-adjudicate.sh [all|dump|diff|adjudicate|converge|refusal]
# env:   SULFUR_R18_OUT (default ~/rp-logs-sulfur-r18), REFS_DEST, REMOTE_DIR
# exit 0 ok, 2 preconditions/missing inputs, 1 an EXPECT did not hold.
set -uo pipefail

STAGE="${1:-all}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${REFS_DEST:-$HOME/rustmc/oracle-work}"
OUT="${SULFUR_R18_OUT:-$HOME/rp-logs-sulfur-r18}"
X1="$DEST/gen-wide-12345-r18-ow-x1"
X2="$DEST/gen-wide-12345-r18-ow-x2"
G="$ROOT/target/release/region-differ"
WD="$ROOT/target/release/worldgen-dump"
DUMP="$OUT/dump-zx-r20"

# THE WINDOW IS THE TWINS' OWN COMPLETED EXTENT, and it is derived rather than
# chosen: `ci/ref-chunk-extent.py --centre 64 64` reports r=20 on both twins,
# and `worldgen-dump full 12345 20` writes exactly 44..84. Every side of every
# diff below is `minecraft:full` across all 1,681 chunks of it.
W="44 44 84 84"
CHUNK="77 47"

mkdir -p "$OUT/pos"
[ -d "$X1" ] && [ -d "$X2" ] || { echo "no r=18 twins under $DEST" >&2; exit 2; }
[ -x "$G" ] || { echo "no $G — cargo build --release -p region-differ" >&2; exit 2; }

# ------------------------------------------------ stamps are a refusal ------
# AN ASSERTION IS NOT A STAMP (ORACLE rule 30). The twins must name the same
# order as each other and as the dump this script takes.
for d in "$X1" "$X2"; do
    h="$d/HARVEST-ORDER"
    [ -f "$h" ] || { echo "REFUSED: $(basename "$d") has no HARVEST-ORDER" >&2; exit 2; }
    grep -qx 'gen_order=rows' "$h" || { echo "REFUSED: $(basename "$d") is not rows" >&2; exit 2; }
    grep -qx 'dump_order=zx' "$h" || { echo "REFUSED: $(basename "$d") is not zx" >&2; exit 2; }
    grep -qx 'seed=12345 radius=18 centre=64,64 dimension=overworld' "$h" || {
        echo "REFUSED: $(basename "$d") is not the r=18 seed-12345 geometry" >&2; exit 2; }
done
echo "STAMPS OK: both twins gen_order=rows dump_order=zx seed=12345 r=18 centre 64,64"

# ------------------------------------------------------------- dump --------
if [ "$STAGE" = all ] || [ "$STAGE" = dump ]; then
    if [ -d "$DUMP/region" ]; then
        echo "DUMP reused: $DUMP"
    else
        [ -x "$WD" ] || { echo "no $WD" >&2; exit 2; }
        t0=$(date +%s)
        RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER=zx "$WD" full 12345 20 "$DUMP" 64 64 \
            > "$DUMP.log" 2>&1 || { echo "DUMP FAILED — see $DUMP.log" >&2; exit 2; }
        echo "DUMP r=20 in $(( $(date +%s) - t0 ))s"
    fi
    grep -m1 'decorate-order' "$DUMP.log"
    grep -m1 'wrote' "$DUMP.log"
    # A dump whose order is not stamped is not matched to these references.
    grep -q 'decorate-order: zx' "$DUMP.log" || {
        echo "REFUSED: $DUMP.log does not stamp decorate-order: zx" >&2; exit 2; }
fi

# --------------------------------------------------------- coverage --------
# A STAMP IS NOT COVERAGE. Ask the chunks, on every side, before diffing.
python3 "$ROOT/ci/ref-chunk-extent.py" --window $W "$X1" "$X2" "$DUMP/region" || {
    echo "REFUSED: some side is not minecraft:full across '$W'" >&2; exit 2; }

# ------------------------------------------------------------- diffs -------
if [ "$STAGE" = all ] || [ "$STAGE" = diff ]; then
    # VANILLA FIRST in the two charge diffs. The twin-vs-twin pair is taken in
    # BOTH directions because ci/chunk-charge-floor.py reads the first column,
    # and a leave-one-out floor needs `x2-vs-x1` as well as `x1-vs-x2`.
    for spec in "x1 $X1 x2 $X2" "x2 $X2 x1 $X1" \
                "x1 $X1 ours $DUMP/region" "x2 $X2 ours $DUMP/region"; do
        set -- $spec
        p="$OUT/pos/$1-vs-$3.pos"
        [ -s "$p" ] && { echo "reuse $1-vs-$3"; continue; }
        "$G" diff "$2" "$4" --bbox $W --positions > "$p.part" 2>&1 || {
            echo "FAIL diff $1 $3" >&2; exit 2; }
        mv "$p.part" "$p"
        echo "$1-vs-$3  $(grep -m1 'DIFF chunks' "$p")"
    done
fi

# -------------------------------------------------- our own extent ---------
# `DECORATION_MARGIN` of 2 does not converge, so OUR side has a generation
# extent too and the adjudicated chunk has to sit outside it. Ours at r=18
# writes 46..82, ours at r=20 writes 44..84; where they disagree is how far
# our own truncation reaches inward.
if [ "$STAGE" = all ] || [ "$STAGE" = converge ]; then
    R18="${OURS_R18:-$HOME/rp-sulfur19/12345/overworld/dump-zx-r18/region}"
    if [ -d "$R18" ]; then
        echo "CONVERGE ours-r18 vs ours-r20:"
        for lbl in "the adjudicated chunk|$CHUNK $CHUNK" \
                   "its whole ring z=47|46 47 82 47" \
                   "the r=18 window|46 46 82 82" \
                   "ring x=46|46 46 46 82" "ring x=47|47 46 47 82" \
                   "ring x=48|48 46 48 82" "ring x=49|49 46 49 82"; do
            echo -n "  ${lbl%%|*}: "
            "$G" diff "$R18" "$DUMP/region" --bbox ${lbl##*|} 2>&1 | grep -m1 'DIFF chunks'
        done
    else
        echo "CONVERGE skipped — no ours-r18 dump at $R18 (set OURS_R18)"
    fi
fi

# ------------------------------------------------------ the refusal --------
# TRAP 2, DEMONSTRATED RATHER THAN ASSERTED. A refusal nobody has watched fire
# is an assertion. Feed it a window it must refuse, and show what
# `region-differ` says when nothing refuses first.
if [ "$STAGE" = all ] || [ "$STAGE" = refusal ]; then
    R16="${OURS_R16:-$HOME/rp-sulfur19/12345/overworld/dump-zx-r16/region}"
    echo "REFUSAL one ring wider than the twins (43 43 85 85):"
    python3 "$ROOT/ci/ref-chunk-extent.py" --window 43 43 85 85 "$X1" "$X2" \
        2>&1 | grep -E 'REFUSED|statuses' | head -4
    rc=$?
    if [ -d "$R16" ]; then
        echo "REFUSAL our r=16 dump (writes 48..80) on '$W':"
        python3 "$ROOT/ci/ref-chunk-extent.py" --window $W "$R16" 2>&1 \
            | grep -E 'REFUSED|statuses' | head -3
        echo "  rc=$? (3 = refused)"
        echo "  and what region-differ says with no refusal in front of it:"
        echo -n "    honest 48 48 80 80: "
        "$G" diff "$X1" "$R16" --bbox 48 48 80 80 2>&1 | grep -m1 'DIFF chunks'
        echo -n "    wide   $W: "
        "$G" diff "$X1" "$R16" --bbox $W 2>&1 | grep -m1 'DIFF chunks'
        echo "    identical counts on a window 54% larger is the silent drop."
    fi
fi

# -------------------------------------------------- the adjudication -------
if [ "$STAGE" = all ] || [ "$STAGE" = adjudicate ]; then
    echo
    echo "=== the window, with the twins' own disagreement beside it ==="
    for f in x1-vs-x2 x1-vs-ours x2-vs-ours; do
        printf '  %-12s %s  %s\n' "$f" \
            "$(grep -m1 'DIFF chunks' "$OUT/pos/$f.pos")" \
            "$(grep -m1 EXACTNESS "$OUT/pos/$f.pos")"
    done

    echo
    echo "=== the adjudication: chunk (77,47), pool = the twins, N=1 leave-one-out ==="
    # EXPECT sulfur -> cinnabar 35 on chunk (77,47), twin floor 0, charge 51
    python3 "$ROOT/ci/chunk-charge-floor.py" "$OUT/pos" --cand ours --refs x1,x2 \
        --chunk $CHUNK --top 12 --expect-charge 51
    rc=$?
    [ $rc -eq 0 ] || { echo "EXPECT FAILED: charge on (77,47) is not 51" >&2; exit 1; }

    row=$(python3 "$ROOT/ci/chunk-charge-floor.py" "$OUT/pos" --cand ours \
            --refs x1,x2 --chunk $CHUNK --top 40 2>/dev/null \
          | awk '/minecraft:sulfur -> minecraft:cinnabar/{print $1; exit}')
    echo "  sulfur -> cinnabar on (77,47): ${row:-0}"
    [ "${row:-0}" = "35" ] || {
        echo "EXPECT FAILED: sulfur -> cinnabar is ${row:-0}, not 35" >&2; exit 1; }

    twin=$("$G" diff "$X1" "$X2" --bbox $CHUNK $CHUNK 2>&1 \
           | sed -n 's/.*mismatches=\([0-9]*\).*/\1/p' | head -1)
    echo "  the twins against each other on (77,47): ${twin:-?} of 98,304"
    [ "${twin:-1}" = "0" ] || {
        echo "EXPECT FAILED: the twins disagree on (77,47) — the 35 is not" >&2
        echo "               adjudicable at r=18 either, and that is a result" >&2
        exit 1; }

    echo
    echo "=== the row per chunk, same pool, same window ==="
    for c in "77 47" "77 48" "78 48" "79 48" "79 49" "80 50" "78 47" "76 47"; do
        set -- $c
        printf '  chunk (%s,%s)  ' "$1" "$2"
        python3 "$ROOT/ci/chunk-charge-floor.py" "$OUT/pos" --cand ours --refs x1,x2 \
            --chunk $1 $2 --top 40 2>&1 \
          | awk '/charge on this chunk/{t=$5}
                 /innocent floor on this chunk/{fl=$NF}
                 /minecraft:sulfur -> minecraft:cinnabar/{s=$1}
                 END{printf "charge %-4s sulfur->cinnabar %-4s twin floor %s\n",
                            t, (s==""?0:s), fl}'
    done

    echo
    echo "=== the same row over the whole window, with its own floor ==="
    python3 "$ROOT/ci/confusion-pair-census.py" "$OUT/pos" --cand ours --refs x1,x2 \
        --pair 'minecraft:sulfur->minecraft:cinnabar' --top 6 2>&1 \
      | sed -n '1,12p'
fi

echo "SULFUR-R18 DONE"
