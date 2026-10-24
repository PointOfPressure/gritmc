#!/usr/bin/env bash
# next-12345-measure.sh — re-take the seed-12345 r=16 overworld charge on THIS
# tip, and lay down the per-block/both-direction census the next item is picked
# from. One command, so the numbers in docs/DETERMINISM.md §22 can be checked.
#
# WHY THIS EXISTS AS A SEPARATE ARTEFACT. `ci/ow-r16-window-12345.sh` is the
# registered artefact for the *published* level and it scores the N=2..6 `c..h`
# arm; `ci/floor-census-12345.sh` re-takes the *floor* numbers off a stored
# matrix. Neither of them takes a fresh charge for a new tip against the
# WIDENED sixteen-reference pool, which is the instrument with the resolving
# power (floor max 2,241 / median 590 at N=15, against 2,577..4,835 at N=5).
# This script does exactly that and nothing else, so it can be re-run on any
# later tip by changing only LABEL.
#
# WINDOW, identical to §17/§17.1/§18/§20.7 on purpose: seed 12345, overworld,
# r=16, centre chunk (64,64), 33x33 = 1,089 chunks, bbox 48 48 80 80, y full,
# 107,053,056 blocks, no mask, VANILLA FIRST in every region-differ call.
#
# ENSEMBLE: gen-wide-12345-r16-ow-{c..r}, sixteen references, gen_order=rows,
# dump_order=zx. The order is READ OFF each reference's own HARVEST-ORDER stamp
# and the script refuses on a missing or mixed one (ORACLE rule 30 — an
# assertion is not a stamp). The unstamped `a`/`b` pair is not used and not
# touched; the `rr-*` rows_rev pair is not mixed into a rows pool, because
# widening across orders made the floor WORSE (1,476..6,128 against
# 2,577..4,835, docs/DETERMINISM.md §18).
#
# THE FLOOR IS NEVER SUBTRACTED. It is printed beside the charge as a positive
# control: the charge this instrument lays on a known-innocent candidate, i.e.
# a genuine unmodified vanilla run substituted for our dump. A charge below its
# floor is not a better result, it means the instrument has stopped separating
# us from vanilla.
#
# THE REF-VS-REF HALF IS REUSED, AND CONTROLLED FOR. A `<r1>-vs-<r2>.pos` for
# two vanilla references is a function of the two references only — no tree, no
# binary, no branch — so copying that half of the matrix from another scratch
# root costs nothing and saves 324 diffs. That is an argument, not a
# measurement, so stage `control` re-derives one pair from the references
# themselves and `cmp`s it against the copied file. If they differ, everything
# downstream is void and the script exits 3.
#
# THE REGISTERED NUMBERS. Stated here because `ci/check-figures.py` requires a
# figure's artefact to contain the claim it is registered against — a figure
# whose artefact cannot be re-run, or does not state what it produced, is a
# figure nobody can check. Tip: `worldgen-carver-topmaterial` c1654bcc plus a
# cherry-pick of the spruce canopy fix `3262cf3c`; the second line adds the
# `LakeWorld::should_freeze` fix. NOT main 21e25c48.
#
#   before the lake-ice fix   4,140 of 107,053,056 — 99.996133%
#   with the lake-ice fix     3,113 of 107,053,056 — 99.997092%
#   delta                     -1,027, fixed 1,027, newly charged 0
#   floor, unchanged          141..2,241, median 590, n=16   (N=15, p_hide 18.1%)
#   margin                    1.85x -> 1.39x on the max, 7.02x -> 5.28x on median
#
# AND THEN, 2026-08-10, WITH THE ICEBERG NaN GUARDS FIXED (branch
# worldgen-iceberg-912, docs/DETERMINISM.md §23):
#
#   base, restacked tip       3,117 of 107,053,056 — 99.997088%
#   with the iceberg fix      1,329 of 107,053,056 — 99.998759%
#   delta                     -1,788, fixed 1,788, newly charged 0
#   floor, still unchanged    141..2,241, median 590, n=16   (N=15)
#   margin                    1.39x -> 0.59x on the max, 5.28x -> 2.25x on median
#
# BOTH ENDPOINTS ARE ON THE RESTACKED TIP, and that is not pedantry. Before the
# restack onto `worldgen-carver-topmaterial` 8c7e024c the same two arms read
# 3,113 and 1,325. Both moved by 4 because the restack brings 571 changed lines
# of `stage/fluid.rs`; the guard restructure that satisfies clippy was measured
# separately and moves ZERO blocks over the identical position set. The fixed
# set is the SAME 1,788 positions on both trees, so the delta never depended on
# the base — only the endpoints did. A delta between two trees is not a delta.
#
# **THAT LAST LINE RETIRES THIS WINDOW'S EXACTNESS FIGURE.** 1,325 is smaller
# than the 2,241 this same instrument charges reference `m` playing the
# innocent candidate, so the charge has fallen BELOW its floor and
# `99.998762%` is NOT quotable — the instrument has stopped separating us from
# vanilla in aggregate. Deltas on this window remain exact (the charge is a
# deterministic function of our output against a fixed reference set) and the
# per-block rows still adjudicate: `sulfur -> cinnabar` is 312 against a
# binding floor of 3. Do not quote the percentage; quote the delta, or quote a
# row with its own floor beside it.
#
# The floor is UNCHANGED across the delta because it is a property of the
# reference ensemble and the fix did not touch it; the whole of the improvement
# is in the charge, which is what distinguishes a repair from a luckier
# ensemble. The percentage improved and the resolving power fell — that is what
# a smaller charge against a fixed floor means, and it is not a regression.
#
# usage: ci/next-12345-measure.sh [build|dump|diffs|control|census|all]
# env:   LABEL      scratch root name under $HOME       (default rp-next12345)
#        RUSTMC_REMOTE_DIR  tree to build/run in        (default rustmc-next12345)
# exit 0 ok, 2 preconditions/inputs, 3 the reused matrix failed its control.
set -uo pipefail

STAGE="${1:-all}"
LABEL="${LABEL:-rp-next12345}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REFDIR="$HOME/rustmc/oracle-work"
PFX="$REFDIR/gen-wide-12345-r16-ow"
OUT="$HOME/$LABEL/12345/overworld"
POS="$OUT/pos"
DUMP="$OUT/dump-zx"
BBOX="48 48 80 80"
ROWS="c d e f g h i j k l m n o p q r"
DIFFER="$ROOT/target/release/region-differ"
DUMPER="$ROOT/target/release/worldgen-dump"

mkdir -p "$POS"

# ---- preconditions: every reference present, stamped, and of ONE order ------
order=""
for r in $ROWS; do
    [ -d "$PFX-$r" ] || { echo "REFUSED  missing reference $PFX-$r"; exit 2; }
    s="$PFX-$r/HARVEST-ORDER"
    [ -s "$s" ] || { echo "REFUSED  $r carries no HARVEST-ORDER stamp (ORACLE rule 30)"; exit 2; }
    d=$(sed -n 's/.*dump_order=\([a-z_]*\).*/\1/p' "$s" | head -1)
    g=$(sed -n 's/.*gen_order=\([a-z_]*\).*/\1/p' "$s" | head -1)
    case "$d" in zx|zx_zrev) ;; *) echo "REFUSED  $r dump_order=$d not understood"; exit 2;; esac
    [ "$g" = rows ] || { echo "REFUSED  $r gen_order=$g, pool must not mix orders"; exit 2; }
    if [ -z "$order" ]; then order="$d"
    elif [ "$order" != "$d" ]; then echo "REFUSED  pool mixes dump orders ($order vs $d)"; exit 2; fi
done
echo "ensemble  16 references c..r  gen_order=rows  dump_order=$order  (read off stamps)"
MATCHED="$order"

run_build() {
    echo "== build"
    ( cd "$ROOT" && cargo build --release -p worldgen-dump -p region-differ ) || exit 2
}

run_dump() {
    echo "== dump  OUR tip, decorate order $MATCHED, workers 1"
    [ -x "$DUMPER" ] || { echo "REFUSED  no $DUMPER — run the build stage"; exit 2; }
    rm -rf "$DUMP"
    RUSTMC_WORKERS=1 RUSTMC_DECORATE_ORDER="$MATCHED" \
        "$DUMPER" full 12345 16 "$DUMP" 64 64 2>&1 | tee "$OUT/dump-zx.log" | \
        grep -E 'decorate-order:|wrote|BIOME_PALETTE'
    grep -q "decorate-order: $MATCHED" "$OUT/dump-zx.log" || {
        echo "REFUSED  dump did not report decorate-order: $MATCHED"; exit 2; }
}

run_diffs() {
    echo "== diffs  VANILLA FIRST, 16 x (ref vs ours)"
    [ -x "$DIFFER" ] || { echo "REFUSED  no $DIFFER — run the build stage"; exit 2; }
    [ -d "$DUMP/region" ] || { echo "REFUSED  no $DUMP/region — run the dump stage"; exit 2; }
    for r in $ROWS; do
        f="$POS/$r-vs-ours.pos"
        if [ -s "$f" ] && grep -q '^DIFF chunks=' "$f"; then continue; fi
        "$DIFFER" diff "$PFX-$r" "$DUMP/region" --bbox $BBOX --positions > "$f" || exit 2
        grep -q '^DIFF chunks=' "$f" || { echo "REFUSED  $f has no DIFF summary"; rm -f "$f"; exit 2; }
        printf '  %s  %s\n' "$r" "$(grep '^DIFF chunks=' "$f")"
    done
}

# The control on the half of the matrix this script did not generate.
run_control() {
    echo "== control  re-derive one ref-vs-ref pair and cmp against the reused copy"
    [ -x "$DIFFER" ] || { echo "REFUSED  no $DIFFER"; exit 2; }
    have="$POS/c-vs-d.pos"
    [ -s "$have" ] || { echo "REFUSED  no reused $have to control"; exit 2; }
    tmp="$OUT/control-c-vs-d.pos"
    "$DIFFER" diff "$PFX-c" "$PFX-d" --bbox $BBOX --positions > "$tmp" || exit 2
    if cmp -s "$tmp" "$have"; then
        echo "  OK  c-vs-d re-derived byte-identical to the reused copy"
        echo "      $(grep '^DIFF chunks=' "$have")"
        rm -f "$tmp"
    else
        echo "  VOID  the reused ref-vs-ref matrix does not reproduce:"
        diff <(grep '^DIFF' "$tmp") <(grep '^DIFF' "$have") | head
        exit 3
    fi
}

run_census() {
    echo "== census  per block, BOTH directions, full-state keyed, N=15 of 16"
    refs=$(echo $ROWS | tr ' ' ',')
    # --stamps/--ref-prefix are `--emit`'s required companions and floor-census
    # refuses them on their own; this stage prints rather than emits, so it
    # must not pass them. Getting that wrong exits 2 and prints NOTHING, which
    # reads exactly like a census that found nothing to say.
    python3 "$ROOT/ci/floor-census.py" "$POS" --refs "$refs" \
        --adjudicate ours --cand-refs "$refs" --top 40 \
        --label "seed 12345 r=16 ow, tip $(cd "$ROOT" && git rev-parse --short HEAD), N=15 of 16"
    echo
    echo "== joint  the ORDERED PAIR each item actually is, not either marginal"
    python3 "$ROOT/ci/next-12345-joint.py" "$POS" --cand ours --refs "$refs" --top 24
}

case "$STAGE" in
    build)   run_build ;;
    dump)    run_dump ;;
    diffs)   run_diffs ;;
    control) run_control ;;
    census)  run_census ;;
    all)     run_build; run_dump; run_control; run_diffs; run_census ;;
    *) echo "usage: $0 [build|dump|diffs|control|census|all]"; exit 2 ;;
esac
