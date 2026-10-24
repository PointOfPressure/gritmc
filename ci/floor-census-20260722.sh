#!/usr/bin/env bash
# floor-census-20260722.sh — re-take every registered number about the
# seed-20260722 r=16 overworld window's PER-BLOCK floor, and the adjudication
# of our charge against it, in one command, and fail if any has moved.
#
# This is the artefact behind the `ow-r16-20260722-perblock*` figures in
# `ci/figures.toml`, and it is the sibling of `ci/floor-census-12345.sh`. It is
# a separate file rather than a `SEED=` knob on that one for the reason its
# header gives about `ci/ow-r16-window-12345.sh`: `ci/check-figures.py` reads
# published figures out of an artefact's own output, and one file holding two
# seeds' figures makes ARTEFACT-DRIFT unable to say which one drifted.
#
# WINDOW: seed 20260722, overworld, r=16, centre chunk (64,64), 33x33 = 1,089
# chunks, bbox 48 48 80 80, y full, 107,053,056 blocks, no mask, VANILLA FIRST.
#
# ENSEMBLE: gen-wide-20260722-r16-ow-{a..i}, nine references, gen_order=rows,
# dump_order=zx. **EIGHT OF THE NINE CARRY NO `HARVEST-ORDER` STAMP.** Only `i`
# does, and `ci/figures.toml`'s `harvest = "rows"` on this window was therefore
# an assertion of exactly the kind ORACLE rule 30 exists to refuse. It is now a
# measurement: `ci/classify-20260722-order.sh` recovers each one's order from
# the matrix against a stamped reference of each order, with this window's own
# same-order and cross-order controls and a negative control, and writes a
# derived stamps tree that `floor-census.py` consumes and records. That script
# is stage 0 here and is not optional.
#
# THE REGISTERED NUMBERS
#
#   order classification, eight unknowns against stamped anchors
#     cross-order scale (i vs rr-a)      149,216
#     same-order scale  (rr-a vs rr-b)    36,810
#     all eight -> rows, separations 107,688..124,772 — no unknown is within
#     the same-order scale of the wrong answer, so none is a coin toss
#
#   aggregate floor, leave-one-out N=8, n=9 innocent candidates
#     2,202..5,263, median 3,281, n=9        (p_hide at N=8 = 31.2%)
#
#   structure of that floor
#     union of the nine innocent charges 30,574 positions, of which 293 are
#     charged more than once
#     73 distinct vanilla blocks carry any MISS floor
#     84 distinct blocks were PLACED by any innocent run
#
#   THE ITEM. The registered 2,450 — `ow-r16-admissible-matched`, quotable =
#   false, disqualifier `below floor` because 2,450 sits inside 2,202..5,263 —
#   re-adjudicated per block instead of in aggregate. Adjudicated against the
#   SAME EIGHT references the floor is a leave-one-out over, the charge is
#   2,450 to the block, which is the positive control on this whole section.
#   Over all nine it is 2,443; the nine 8-of-9 subsets span 2,443..2,514.
#
#     MISS direction    (keyed on the vanilla spelling, `[[block]]`)
#       CLEAR 803, marginal 449, at/under floor 1,198
#     SURPLUS direction (keyed on what we placed, `[[placed]]`)
#       CLEAR 824, marginal 394, at/under floor 1,232
#     BOTH, each position taking the WEAKER of its two verdicts — QUOTE THIS
#       CLEAR in both directions 650, marginal 96, at or under a floor in
#       one direction 1,704
#     The join is 650 at N=8 and 650 at N=9. Only the unadjudicable bucket
#     moves with N, which is what a robust headline looks like.
#
#   and the 650 is one defect, not a spread:
#     grass_block[snowy=false]  miss 649 against a miss floor of 1     649.0x
#     dirt                      surplus 648 against a placed floor of 71 9.1x
#
#   held-out validation, floor estimated on 4 of 9 and tested on the other 5,
#   40 splits — the guard on every floor of zero above
#     PER-BLOCK  called CLEAR 803..1,232, still CLEAR 803..803,
#                survival 65.2%..100.0%          -> the table GENERALISES
#     PER-CHUNK  793..850 chunks read zero-floor on the 4, and the held-out
#                candidates are charged 2,219..4,102 INSIDE them against our
#                1,968..2,218                     -> the sub-window DOES NOT,
#                and on this window our charge inside the "clean" chunks is
#                BELOW what an innocent run is charged there.
#                DISQUALIFIER: selected on the same runs that validate it.
#
#   the null that has to stay measured — widening ACROSS harvest orders
#     9 rows + 2 rows_rev, N=10: 1,066..13,273, median 2,540, n=11. The MAX
#     RISES 5,263 -> 13,273, eight thousand blocks, against a rise of 1,293
#     for the same move on seed 12345 — and all of it is the two minority
#     members, charged 9,086 and 13,273 because each is adjudicated mostly
#     against references of the other order. Charged for the order, not for
#     being a different run. DISQUALIFIER: mixed harvest order.
#
# INPUT. The `<ref>-vs-<cand>.pos` matrix `ci/order-level-retake.sh pairs`
# leaves at `~/rp-logs-ladder/pos` — 132 ordered diffs including both of that
# script's own candidate dumps, of which `zx` is the order-matched one. Nothing
# here regenerates a reference or a dump; everything is read-only.
#
# usage: ci/floor-census-20260722.sh
# exit 0 every registered number reproduced, 1 one of them moved, 2 inputs are
# missing or malformed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

POS=${FLOOR_POS:-$HOME/rp-logs-ladder/pos}
STAMPS=${FLOOR_STAMPS:-$HOME/w22-logs/stamps}
PFX=gen-wide-20260722-r16-ow-
TABLE=ci/floor-tables/ow-r16-20260722-rows-n8.toml
C=ci/floor-census.py
REFS=a,b,c,d,e,f,g,h,i

[ -d "$POS" ] || { echo "no position matrix at $POS — see the header" >&2; exit 2; }

rc=0
step() { echo; echo "=== $*"; }

# Stage 0. The order is MEASURED before anything is scored against it, and the
# derived stamps tree this writes is what the census reads. A run that skipped
# it would be scoring against an assertion.
step "harvest order of the nine references, classified rather than asserted"
CLASSIFY_OUT="$(dirname "$STAMPS")" ./ci/classify-20260722-order.sh || rc=1

step "aggregate floor and its structure, rows pool a..i, N=8"
python3 "$C" "$POS" --refs "$REFS" --top 6 \
    --label "seed-20260722 r=16 overworld, rows harvest, pool a..i" \
    --expect-range 2202..5263 \
    --stamps "$STAMPS" --ref-prefix "$PFX" \
    --emit /tmp/floor-census-20260722.$$.toml || rc=1

# The committed table is the published artefact, so it is compared rather than
# trusted. A table that has drifted from what the matrix now says is exactly
# the "stale caveat list that reads as authoritative" failure.
step "committed per-block floor table still matches the matrix"
if diff -u "$TABLE" /tmp/floor-census-20260722.$$.toml > /tmp/floor-census-20260722.$$.diff; then
    echo "TABLE GREEN  $TABLE reproduces"
else
    echo "TABLE RED    $TABLE has drifted from the matrix:"
    head -40 /tmp/floor-census-20260722.$$.diff
    rc=1
fi
rm -f /tmp/floor-census-20260722.$$.toml /tmp/floor-census-20260722.$$.diff

step "THE ITEM — the registered 2,450 adjudicated per block, in BOTH directions"
python3 "$C" "$POS" --refs "$REFS" --top 14 \
    --label "seed-20260722 r=16 ow, the matched-order charge, per block" \
    --adjudicate zx --cand-refs a,b,c,d,e,f,g,h --expect-charge 2450 || rc=1

step "held-out validation — does a floor of zero survive references that did not choose it?"
python3 "$C" "$POS" --refs "$REFS" --top 0 \
    --label "held-out validation" \
    --adjudicate zx --holdout || rc=1

# The null. If somebody widens this ensemble with the rows_rev references
# because more is more, this is the line that says no — and unlike seed 12345
# this window has only two of them, so the minority is smaller and the effect
# is larger.
step "mixed-order pool is WORSE — the null that must stay measured"
python3 "$C" "$POS" --refs "$REFS",rr-a,rr-b --top 0 \
    --label "MIXED ORDER pool: 9 rows + 2 rows_rev" \
    --stamps "$STAMPS" --ref-prefix "$PFX" || rc=1

echo
if [ "$rc" = 0 ]; then
    echo "FLOOR-CENSUS-20260722 GREEN — every registered number reproduced"
else
    echo "FLOOR-CENSUS-20260722 RED — a registered number moved (see above)"
fi
exit $rc
