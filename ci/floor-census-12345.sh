#!/usr/bin/env bash
# floor-census-12345.sh — re-take every registered number about the seed-12345
# r=16 overworld window's FLOOR, in one command, and fail if any has moved.
#
# This is the artefact behind the `ow-r16-12345-floor-*` figures in
# `ci/figures.toml`, and it exists for the reason `ci/ow-r16-window-12345.sh`
# exists for the level: a figure whose artefact cannot be re-run is a figure
# nobody can check, and `ci/check-figures.py` requires the artefact to contain
# the claim it is registered against. Every number this script prints is stated
# in the block below, so a drift shows up as a diff in this file and not only
# as a red run.
#
# WINDOW, fixed and shared with §17, §17.1 and §18 of docs/DETERMINISM.md:
# seed 12345, overworld, r=16, centre chunk (64,64), 33x33 = 1,089 chunks,
# bbox 48 48 80 80, y full, 107,053,056 blocks, no mask, VANILLA FIRST.
#
# ENSEMBLE: gen-wide-12345-r16-ow-{c..h}, gen_order=rows, dump_order=zx, read
# off each reference's own HARVEST-ORDER stamp rather than asserted (ORACLE
# rule 30). The unstamped `a`/`b` pair is not used and not touched.
#
# THE REGISTERED NUMBERS
#
#   aggregate floor, leave-one-out N=5, n=6 innocent candidates
#     2,577..4,835, median 3,387, n=6
#
#   structure of that floor
#     union of the six innocent charges 21,650 positions, of which 2 are
#     charged more than once
#     64 distinct vanilla blocks carry any floor
#     diorite 1,398, andesite 1,377, granite 1,187 — 55.5% of the floor
#
#   the same instrument on a pool widened ACROSS harvest orders — WORSE
#     6 rows + 2 rows_rev, N=7: 1,476..6,128, median 2,903, n=8
#
#   held-out validation, floor estimated on 3 of 6 candidates and tested on
#   the other 3, 20 splits — the guard on everything above
#     PER-BLOCK  called CLEAR 6,854..7,402, still CLEAR 6,854..6,854,
#                survival 92.6%..100.0%          -> the table GENERALISES
#     PER-CHUNK  805..842 chunks read zero-floor on the 3, and the held-out
#                candidates are charged 2,893..4,694 INSIDE them
#                                                 -> the sub-window DOES NOT
#
#   the registered residual, adjudicated per block instead of in aggregate
#     7,940 over refs c,d,e,f,g; CLEAR 6,854, marginal 509, at/under 577
#     the same charge over every other 5-of-6 subset: 7,871..8,418, median
#     7,913, n=6
#
#   THE WIDENED ENSEMBLE — docs/DETERMINISM.md §18.8, and the reason this item
#   existed. Ten more `rows` references take the pool to 16 and N to 15.
#     floor            141..2,241, median 590, n=16   (was 2,577..4,835)
#     p_hide           18.1%                          (was 45.1%)
#     our charge       7,755..7,827 over the sixteen 15-of-16 subsets
#     margin           3.46x on the max, 13.1x on the median  (was 1.64x)
#     per block        CLEAR 7,088 / marginal 226 / at-or-under-floor 441
#     like-for-like    on the SAME six candidates the max fell 4,835 -> 1,210;
#                      2,241 is candidate `m`, which did not exist to be drawn
#                      from before. A max over 16 is not a max over 6.
#
# INPUT. A `<ref>-vs-<cand>.pos` matrix. The default is this item's own durable
# copy; the reference-against-reference half is symlinked from the matrix
# `ci/order-level-retake.sh` produced and the `zx` column is copied from the
# run that took the 7,940, so nothing here depends on another tree staying put.
# Regenerating any of it costs 2.6 s per diff (Threadripper, 2026-08-10) and
# `ci/refs-12345-widen.sh pairs` does it.
#
# usage: ci/floor-census-12345.sh
# exit 0 every registered number reproduced, 1 one of them moved, 2 inputs are
# missing or malformed.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

POS=${FLOOR_POS:-$HOME/rp-logs-floor12345/pos}
STAMPS=${FLOOR_STAMPS:-$HOME/rustmc/oracle-work}
PFX=gen-wide-12345-r16-ow-
TABLE=ci/floor-tables/ow-r16-12345-rows-n5.toml
WTABLE=ci/floor-tables/ow-r16-12345-rows-n15.toml
C=ci/floor-census.py

[ -d "$POS" ] || { echo "no position matrix at $POS — see the header" >&2; exit 2; }

rc=0
step() { echo; echo "=== $*"; }

step "aggregate floor and its structure, rows pool c..h, N=5"
python3 "$C" "$POS" --refs c,d,e,f,g,h --top 5 \
    --label "seed-12345 r=16 overworld, rows harvest, pool c..h" \
    --expect-range 2577..4835 \
    --stamps "$STAMPS" --ref-prefix "$PFX" \
    --emit /tmp/floor-census-12345.$$.toml || rc=1

# The committed table is the published artefact, so it is compared rather than
# trusted. A table that has drifted from what the matrix now says is exactly
# the "stale caveat list that reads as authoritative" failure, and it would be
# invisible if this script only re-printed the aggregate.
step "committed per-block floor table still matches the matrix"
if diff -u "$TABLE" /tmp/floor-census-12345.$$.toml > /tmp/floor-census-12345.$$.diff; then
    echo "TABLE GREEN  $TABLE reproduces"
else
    echo "TABLE RED    $TABLE has drifted from the matrix:"
    head -40 /tmp/floor-census-12345.$$.diff
    rc=1
fi
rm -f /tmp/floor-census-12345.$$.toml /tmp/floor-census-12345.$$.diff

# The null that has to stay measured. If somebody widens this ensemble with the
# rows_rev references because more is more, this is the line that says no.
step "mixed-order pool is WORSE — the max rises 4,835 -> 6,128"
python3 "$C" "$POS" --refs c,d,e,f,g,h,rr-a,rr-b --top 0 \
    --label "MIXED ORDER pool: 6 rows + 2 rows_rev" \
    --expect-range 1476..6128 \
    --stamps "$STAMPS" --ref-prefix "$PFX" || rc=1

step "the registered 7,940 adjudicated against the per-block floor"
python3 "$C" "$POS" --refs c,d,e,f,g,h --top 8 \
    --label "seed-12345 r=16 ow, AFTER iceberg" \
    --adjudicate zx --cand-refs c,d,e,f,g --expect-charge 7940 || rc=1

# The guard, and it is the step that stops this whole section being a way of
# writing a smaller number. Any floor of zero is estimated from six innocent
# readings and a small enough bucket reads zero by luck. Estimate on half the
# pool, test on the other half: the per-block table survives and the sub-window
# the same census suggests does not.
step "held-out validation — does a zero survive references that did not choose it?"
python3 "$C" "$POS" --refs c,d,e,f,g,h --top 0 \
    --label "held-out validation" \
    --adjudicate zx --cand-refs c,d,e,f,g --holdout || rc=1

# ---------------------------------------------------------------------------
# The widened ensemble. Everything above is the six-reference pool and stays,
# because ow-r16-12345-floor-n5 is still the correct floor for that pool and
# every number quoted against it stands. This is a different ensemble and so a
# different figure, not an overwrite.
# ---------------------------------------------------------------------------
WIDE=c,d,e,f,g,h,i,j,k,l,m,n,o,p,q,r

step "the floor RE-TAKEN on sixteen rows references, N=15"
python3 "$C" "$POS" --refs "$WIDE" --top 6 \
    --label "seed-12345 r=16 ow, WIDENED rows pool c..r" \
    --expect-range 141..2241 \
    --stamps "$STAMPS" --ref-prefix "$PFX" \
    --emit "$WTABLE.new" || rc=1
if diff -u "$WTABLE" "$WTABLE.new" > /dev/null; then
    echo "TABLE GREEN  $WTABLE reproduces"
else
    echo "TABLE RED    $WTABLE has drifted from the matrix"
    diff -u "$WTABLE" "$WTABLE.new" | head -40
    rc=1
fi
rm -f "$WTABLE.new"

step "the same dump adjudicated on the widened pool, and held out"
python3 "$C" "$POS" --refs "$WIDE" --top 6 \
    --label "WIDENED, adjudicated" \
    --adjudicate zx --expect-charge 7755 --holdout || rc=1

echo
if [ "$rc" = 0 ]; then
    echo "FLOOR-CENSUS-12345 GREEN — every registered number reproduced"
else
    echo "FLOOR-CENSUS-12345 RED — a registered number moved (see above)"
fi
exit $rc
