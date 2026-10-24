#!/usr/bin/env bash
# refs-12345-widen2.sh — take the seed-12345 r=16 overworld pool from 16 to 40,
# and MEASURE the per-reference cost while doing it.
#
# WHY, AND WHAT THE COSTING SAID BEFORE A SINGLE REFERENCE WAS SPENT
#
# The window's charge is 1,329 (`ow-r16-12345-level-n15-iceberg`, tip
# b9f41f7c) against a max innocent floor of 2,241 at pool 16 / N=15, so it sits
# at 0.59x — BELOW its own floor — and no exactness percentage may be quoted
# from it. `ci/floor-vs-n.py` subsampled the existing sixteen references at
# every pool size from 2 to 16 and measured what a figure at each n would
# carry:
#
#     n     2      4      6      8     10     12     14     16
#   max  14,800  6,743  4,815  3,685  2,980  2,675  2,380  2,241
#   med  14,800  5,210  3,276  2,235  1,564  1,068    734    590
#
# A power fit over n=8..16 gives max ~ 15,930 * n^-0.716, so max < 1,329 at
# n ~= 33 and max < 665 (a 2x margin) at n ~= 85. Fitting only n=12..16, where
# the decay is visibly slowing, moves those to 37 and 113. **The decay is
# slowing, so the low end of each range is the optimistic one.**
#
# 40 rather than 33 is chosen deliberately: 33 is the centre of the estimate
# for the thing we need to clear, and stopping at the centre of an estimate
# that is drifting the wrong way buys an hour and a coin flip. 40 also puts
# real measurements either side of the predicted crossing, which turns the
# extrapolation into an interpolation for whoever asks the 2x question next.
#
# WHY THE MAX IS THE STICKY ONE, AND WHY THAT IS NOT AN ARTEFACT
#
# §18.8 warned that a max is an extreme-value statistic over the pool, so a
# larger pool gets more chances to draw a bad candidate. `ci/floor-vs-n.py`
# settled whether that is a sampling artefact or a real property of the runs:
# the Spearman rank correlation between each candidate's floor at N=5 and at
# N=15 is **+0.826**. A bad reading is a property of the RUN, not of the draw.
# Candidate `m` reads 4,663 / 2,767 / 2,241 at N=5/10/15 and is the worst at
# every N. So the pool really does keep meeting new bad runs as it grows, the
# inflation is not going to wash out, and the extrapolation above is the
# honest one rather than the pessimistic one.
#
# THE NULL THIS REPLACES — do not re-run it
#
# Restricting the window in y is free (no references at all) and §18.2's census
# invited it: 15,893 of 21,650 floor positions sit in y 0..47, mostly diorite,
# andesite and granite substituting for each other. It does not work, because
# the charge bands away faster than the floor does:
#
#   y band        charge   max floor   margin
#   full           1,329       2,241    0.59x
#   0..319           859       2,238    0.38x
#   48..319          241       1,415    0.17x
#   64..319           91       1,271    0.07x
#
# Every band is WORSE than the full window on the max. The floor's max is not
# made of the low-y stone-variant blobs at all — one candidate carries 1,271
# floor blocks above y=64 on its own — so banding removes our residual and
# leaves the extreme candidate untouched. `ci/floor-vs-n.py --ybox` reproduces
# the table above in seconds if anyone wants to re-check it.
#
# GEOMETRY, identical to `ci/refs-12345-ensemble.sh` and
# `ci/refs-12345-widen.sh` on purpose: seed 12345, r=16, centre chunk (64,64),
# 33x33 = 1,089 chunks, bbox 48 48 80 80, dimension overworld, GEN_ORDER=rows
# so every reference is stamped `dump_order=zx`.
#
# LABELS AND PORTS. One port is used once, ever — `gen-vanilla-region.sh`
# derives its scratch server directory from the port, so two runs sharing one
# collide. 25701 is the default, 25921/25922 are a/b, 25931..25936 are c..h,
# 25941/25942 are rr-a/rr-b, 25951..25960 are i..r. This script starts at
# **25961** and takes 24: s..z then aa..ap.
#
# THE CORPUS LOCK, AND THE WINDOW THIS SITS IN IT FOR. AGENT-BRIEF requires a
# job that measures wall time to hold the corpus lock as role `run` and to say
# how long its window is. The generation stage is **24 references at ~60 s,
# about twenty-five minutes**, and the lock is handed back before the diff
# stage, which is CPU-only, needs no boots, and excludes nobody.
#
# IDEMPOTENT BY REFUSAL. `gen-vanilla-region.sh` does `rm -rf "$OUT"` and does
# not refuse, so the guard lives here: an existing reference directory is
# SKIPPED, never regenerated. There are 47 reference directories in this repo
# and they are its most expensive artefact.
#
# THE DIFFING WINDOW IS A PARAMETER, AND IT IS NOT THE GENERATION EXTENT.
# 2026-08-11. `BBOX` was a constant here and was read, by a brief, as the thing
# that decides how much world exists. It is not: it decides only which chunks
# `region-differ` scores. The references cover what they cover whatever this
# says, and `WIDEN2_BBOX` cannot conjure a chunk that was never decorated.
#
# It defaults to the historical `48 48 80 80` so that every figure already
# published off this script keeps its meaning. **A new window is a new figure**
# — do not rebase a published number onto a wider window, give it a new id and
# a new `WIDEN2_OUT`. Three refusals below enforce the parts of that a comment
# cannot:
#
#   1. the window is stamped into `$OUT/WINDOW`, and an `$OUT` already holding
#      a different one is refused rather than mixed;
#   2. the 240 ordered diffs and the 16 charge diffs are reused by symlink from
#      directories that were computed at the DEFAULT window, so the reuse is
#      taken only when the window IS the default. At any other window they are
#      a different measurement wearing the right filename;
#   3. every reference, and our own charge dump, must be `minecraft:full`
#      across the whole window — `ci/ref-chunk-extent.py`. For this pool that
#      caps the window at **46 46 82 82**, r=18, and the reason is worth
#      keeping: `gen-vanilla-region.sh 12345 16` decorates a ring beyond what
#      it completes, so chunks 45..83 exist and only 46..82 are `full`. A diff
#      on 45..83 would score the outermost decorated ring, which never received
#      the cross-border writes of the neighbours nobody generated. That is
#      DECORATION_MARGIN by another door and ORACLE rule 14 already priced it.
#
# usage: refs-12345-widen2.sh [refs|pairs|charge|all]
# env:   WIDEN2_BBOX (default "48 48 80 80"), WIDEN2_OUT, REFS_DEST,
#        SEED_POS, CHARGE_DUMP, CHARGE_POS
# exit 0 ok, 2 preconditions, 3 a reference came out unstamped or wrong-order,
#        or the window is not covered, or $OUT carries a different window.

set -uo pipefail

STAGE="${1:-all}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${REFS_DEST:-$HOME/rustmc/oracle-work}"
PFX="gen-wide-12345-r16-ow"
OUT="${WIDEN2_OUT:-$HOME/rp-logs-floor12345-w2}"
DEFAULT_BBOX="48 48 80 80"
BBOX="${WIDEN2_BBOX:-$DEFAULT_BBOX}"
G="$ROOT/target/release/region-differ"

OLD="c d e f g h i j k l m n o p q r"
NEW="s t u v w x y z aa ab ac ad ae af ag ah ai aj ak al am an ao ap"
ALL="$OLD $NEW"

mkdir -p "$OUT/pos" "$OUT/logs"

# -------------------------------------------------- the window is stamped ---
# NEVER SWAP A NUMBER UNDER A LIVE FIGURE ID. A `.pos` directory is only
# meaningful beside the window it was computed at, and the filenames carry no
# trace of it, so the directory carries it instead.
if [ -f "$OUT/WINDOW" ]; then
    have="$(cat "$OUT/WINDOW")"
    if [ "$have" != "$BBOX" ]; then
        echo "REFUSED: $OUT was built at window '$have', you asked for '$BBOX'." >&2
        echo "         A new window is a new figure — set WIDEN2_OUT to a new" >&2
        echo "         directory rather than mixing two windows in one matrix." >&2
        exit 3
    fi
else
    printf '%s\n' "$BBOX" > "$OUT/WINDOW"
fi
echo "WIDEN2 window: $BBOX  (default $DEFAULT_BBOX)  out $OUT"

# ---------------------------------------------------------------- refs -----
if [ "$STAGE" = refs ] || [ "$STAGE" = all ]; then
    [ -x "$ROOT/ci/gen-vanilla-region.sh" ] || { echo "no gen-vanilla-region.sh" >&2; exit 2; }

    . "$ROOT/ci/corpus-lock.sh"
    RUSTMC_CORPUS_LOCK_ROLE=run corpus_lock_acquire

    port=25961
    echo "WIDEN2 refs: 24 references, ports 25961..25984, GEN_ORDER=rows"
    echo "WIDEN2 exclusive: corpus lock held as role run for the boots only"
    for r in $NEW; do
        out="$DEST/$PFX-$r"
        if [ -d "$out" ]; then
            echo "SKIP $r — $out exists (references are never regenerated)"
            port=$((port + 1))
            continue
        fi
        t0=$(date +%s)
        GEN_ORDER=rows "$ROOT/ci/gen-vanilla-region.sh" 12345 16 "$out" \
            "$port" 64 64 overworld > "$OUT/logs/gen-$r.log" 2>&1
        rc=$?
        t1=$(date +%s)
        if [ $rc -ne 0 ]; then
            echo "FAIL $r rc=$rc — see $OUT/logs/gen-$r.log" >&2
            corpus_lock_release
            exit 2
        fi
        echo "REF $r  port $port  $((t1 - t0)) s"
        port=$((port + 1))
    done

    corpus_lock_release
    echo "WIDEN2 refs done, corpus lock released"
fi

# ------------------------------------------------- stamps are a refusal ----
# AN ASSERTION IS NOT A STAMP (ORACLE rule 30). Every reference this script
# will score against must carry HARVEST-ORDER and must name the SAME order;
# widening an ensemble ACROSS orders made a floor worse by 1,293 (§18.4).
for r in $ALL; do
    h="$DEST/$PFX-$r/HARVEST-ORDER"
    [ -f "$h" ] || { echo "REFUSED: $PFX-$r has no HARVEST-ORDER stamp" >&2; exit 3; }
    grep -qx 'gen_order=rows' "$h" || {
        echo "REFUSED: $PFX-$r is not gen_order=rows: $(head -1 "$h")" >&2; exit 3; }
    grep -qx 'dump_order=zx' "$h" || {
        echo "REFUSED: $PFX-$r is not dump_order=zx: $(head -1 "$h")" >&2; exit 3; }
done
echo "WIDEN2 stamps OK: $(echo $ALL | wc -w) references, all gen_order=rows dump_order=zx"

# ------------------------------------------- the window must be COVERED -----
# A STAMP IS NOT COVERAGE. The block above proves every reference was harvested
# in the right order; it says nothing about how much of the world it holds. A
# region file covers 32x32 chunks whether or not they were generated, so a
# directory listing reads as coverage of chunks 32..95 and is not. Ask the
# chunks.
if [ "$BBOX" != "$DEFAULT_BBOX" ]; then
    refdirs=""
    for r in $ALL; do refdirs="$refdirs $DEST/$PFX-$r"; done
    if ! python3 "$ROOT/ci/ref-chunk-extent.py" --window $BBOX $refdirs \
            > "$OUT/logs/extent.log" 2>&1; then
        echo "REFUSED: window '$BBOX' is not minecraft:full in every reference." >&2
        grep -A2 REFUSED "$OUT/logs/extent.log" | head -20 >&2
        echo "         see $OUT/logs/extent.log; --centre 64 64 prints the widest" >&2
        echo "         square this pool can honestly be scored on." >&2
        exit 3
    fi
    echo "WIDEN2 coverage OK: all $(echo $ALL | wc -w) references are full across $BBOX"
fi

# --------------------------------------------------------------- pairs -----
if [ "$STAGE" = pairs ] || [ "$STAGE" = all ]; then
    [ -x "$G" ] || { echo "no $G — cargo build --release -p region-differ" >&2; exit 2; }

    # Reuse the 240 ordered diffs the 16-reference matrix already has. They are
    # symlinked rather than copied: 240 files at ~900 KB is 215 MB, and a copy
    # of an input nobody writes to is 215 MB of drift waiting to happen.
    #
    # AND THEY ARE ONLY REUSABLE AT THE WINDOW THEY WERE TAKEN AT. Those files
    # were computed at the default bbox and their names say nothing about it,
    # so at any other window they would be a different measurement wearing the
    # right filename — the quietest way there is to mix two windows in one
    # floor.
    SEED_POS="${SEED_POS:-$HOME/rp-iceberg912/12345/overworld/pos}"
    reused=0
    if [ "$BBOX" = "$DEFAULT_BBOX" ]; then
        for r1 in $OLD; do for r2 in $OLD; do
            [ "$r1" = "$r2" ] && continue
            s="$SEED_POS/$r1-vs-$r2.pos"
            d="$OUT/pos/$r1-vs-$r2.pos"
            if [ -s "$s" ] && [ ! -e "$d" ]; then ln -s "$s" "$d"; reused=$((reused + 1)); fi
        done; done
        echo "WIDEN2 pairs: reused $reused ordered diffs from $SEED_POS"
    else
        echo "WIDEN2 pairs: window is not the default — reusing nothing, all diffs are re-taken"
    fi

    # WIDEN2_JOBS defaults to 1, which is exactly the loop this replaced: one
    # diff at a time, in the same order, writing the same files. It is opt-in
    # because a `.pos` file that is half-written when something dies still
    # passes the `[ -s ]` resume check above, and N ways to die at once is N
    # times the chance of a truncated file being mistaken for a finished one.
    # The `.part` rename makes that safe at any N: a file only appears under
    # its real name once the differ has exited 0.
    #
    # Off the default window the full 40x39 matrix is 1,560 diffs and none of
    # them can be reused, which is ~80 minutes serial on the Threadripper. The
    # diffs are CPU-only and take no boots, so they are not under the corpus
    # lock and parallelism here excludes nobody.
    JOBS="${WIDEN2_JOBS:-1}"
    t0=$(date +%s); made=0
    todo="$OUT/logs/pairs.todo"
    : > "$todo"
    for r1 in $ALL; do for r2 in $ALL; do
        [ "$r1" = "$r2" ] && continue
        p="$OUT/pos/$r1-vs-$r2.pos"
        [ -s "$p" ] && continue
        printf '%s\t%s\n' "$r1" "$r2" >> "$todo"
        made=$((made + 1))
    done; done
    if [ $made -gt 0 ]; then
        echo "WIDEN2 pairs: $made diffs to take, WIDEN2_JOBS=$JOBS"
        # VANILLA FIRST is vacuous between two references, but the ORDER of the
        # pair fixes which block is printed first and ci/floor-census.py reads
        # that column, so both directions are generated rather than symlinked.
        export G DEST PFX OUT BBOX
        if ! < "$todo" xargs -P "$JOBS" -n 2 sh -c '
                p="$OUT/pos/$1-vs-$2.pos"
                "$G" diff "$DEST/$PFX-$1" "$DEST/$PFX-$2" --bbox $BBOX --positions \
                    > "$p.part" 2>&1 || { echo "FAIL diff $1 $2" >&2; exit 255; }
                mv "$p.part" "$p"' sh; then
            echo "WIDEN2 pairs: a diff failed — see above; partials left as .part" >&2
            exit 2
        fi
    fi
    t1=$(date +%s)
    if [ $made -gt 0 ]; then
        echo "WIDEN2 pairs: $made new diffs in $((t1 - t0))s = $(awk -v a=$((t1-t0)) -v b=$made 'BEGIN{printf "%.2f", a/b}') s/diff"
    else
        echo "WIDEN2 pairs: nothing to do"
    fi
fi

# -------------------------------------------------------------- charge -----
# The charge at the widened N, against the SAME dump the 1,329 was taken from.
#
# `ci/next-12345-measure.sh` left `dump-zx/` beside its `.pos` directory and it
# is still there, stamped `decorate-order: zx` in its own log. Reusing it is
# not a shortcut, it is the point: re-dumping would re-measure our tree, and
# the tip has to be held still for the floor to be the only thing that moved.
# A re-dump would also have to be proven byte-identical before any comparison
# was legitimate, which costs more than it saves.
if [ "$STAGE" = charge ] || [ "$STAGE" = all ]; then
    DEFAULT_CDUMP="$HOME/rp-ice912-final/12345/overworld/dump-zx"
    CDUMP="${CHARGE_DUMP:-$DEFAULT_CDUMP}"
    CPOS="${CHARGE_POS:-$HOME/rp-ice912-final/12345/overworld/pos}"
    [ -d "$CDUMP/region" ] || { echo "no dump at $CDUMP/region" >&2; exit 2; }
    grep -q 'decorate-order: zx' "$CDUMP.log" 2>/dev/null || {
        echo "REFUSED: $CDUMP.log does not stamp decorate-order: zx. A dump" >&2
        echo "         whose order is not stamped is not matched to these" >&2
        echo "         references — the same instrument charges an unmodified" >&2
        echo "         vanilla run 117,628 blocks for the wrong chunk order." >&2
        exit 3; }

    # OUR DUMP HAS A GENERATION EXTENT TOO, and it is the tighter of the two:
    # `worldgen-dump full 12345 R` decorates [64-R-2, 64+R+2] and WRITES only
    # [64-R, 64+R], so the r=16 dump this stage defaults to holds chunks 48..80
    # and nothing else. Scored on a wider window the 432 chunks it does not
    # have are not a residual, they are an absence — and whether that reads as
    # a charge or as a skip is a property of `region-differ`, not of our port.
    # Refuse instead of finding out.
    if [ "$BBOX" != "$DEFAULT_BBOX" ]; then
        if ! python3 "$ROOT/ci/ref-chunk-extent.py" --window $BBOX "$CDUMP/region" \
                > "$OUT/logs/extent-ours.log" 2>&1; then
            echo "REFUSED: our dump $CDUMP/region is not full across '$BBOX'." >&2
            grep -A2 REFUSED "$OUT/logs/extent-ours.log" | head -10 >&2
            echo "         Re-dump at a radius whose WRITTEN extent covers the" >&2
            echo "         window: worldgen-dump full 12345 R OUT 64 64 writes" >&2
            echo "         chunks 64-R .. 64+R." >&2
            exit 3
        fi
        echo "WIDEN2 charge: our dump is full across $BBOX"
    fi

    # A CHARGE DIFF BELONGS TO A TIP AS WELL AS TO A WINDOW, and unlike the
    # reference-against-reference diffs above it is not tip-free: one side of
    # it is OUR output. The 16 files under the default CHARGE_POS were taken
    # against the default CHARGE_DUMP at tip b9f41f7c. Symlinking them beside
    # diffs taken against a different dump would put two tips in one charge and
    # nothing downstream could see it — the filenames are identical either way.
    reused=0
    if [ "$BBOX" = "$DEFAULT_BBOX" ] && [ "$CDUMP" = "$DEFAULT_CDUMP" ]; then
        for r in $OLD; do
            s="$CPOS/$r-vs-ours.pos"; d="$OUT/pos/$r-vs-ours.pos"
            if [ -s "$s" ] && [ ! -e "$d" ]; then ln -s "$s" "$d"; reused=$((reused + 1)); fi
        done
    else
        echo "WIDEN2 charge: dump or window is not the default — reusing no charge diffs"
    fi
    t0=$(date +%s); made=0
    for r in $ALL; do
        p="$OUT/pos/$r-vs-ours.pos"
        [ -s "$p" ] && continue
        # VANILLA FIRST — backwards inverts the sign of every number.
        "$G" diff "$DEST/$PFX-$r" "$CDUMP/region" --bbox $BBOX --positions \
            > "$p.part" 2>&1 || { echo "FAIL charge diff $r" >&2; exit 2; }
        mv "$p.part" "$p"
        made=$((made + 1))
    done
    echo "WIDEN2 charge: reused $reused, $made new, $(( $(date +%s) - t0 ))s"
fi

echo "WIDEN2 done. pos matrix: $OUT/pos ($(ls "$OUT/pos" | wc -l) files)"
