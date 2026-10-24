#!/usr/bin/env bash
# Of the positions our N=8 admissible charge holds, how many does OUR OWN
# chunk-visitation order move? `ci/decorate-order-intersect.sh` answers the
# neighbouring question by re-diffing every order against a reference; this is
# the cheaper and, for this purpose, stronger form: dump our generator under
# each order and diff OURS-vs-OURS. A charged position whose block is the same
# under all eight distinct orders is order-stable and the charge there is about
# our port; one that moves is chunk-visitation order, which is what
# docs/DETERMINISM.md §11 says the floor is made of.
# `diag` writes bytes identical to `default` (AGENT-BRIEF: nine names, eight
# orders), so it is dumped and then expected to contribute nothing.
# FIRST READING, 2026-08-09, seed 20260722, r=16 centre (64,64), bbox
# 48 48 80 80, against the eight-reference admissible charge:
#   zx 18,508  zx_rev 121,505  zx_zrev 121,063  reverse 210,373
#   spiral 79,498  spiral_in 61,453  spawn 10,464  diag 0
#   any order moves our block at 246,100 positions of 107,053,056
#   N=8 admissible charge 14,270 -> ORDER-STABLE 853 (6.0%)
# So 94.0% of the residual is our own chunk-visitation order, matching the
# 95.5% ci/decorate-order-intersect.sh measured on the r=7 window in July.
# `diag` moving 0 blocks is the positive control for AGENT-BRIEF's "nine names,
# eight orders" — measured here, not assumed.
#
# THIS SCRIPT IS CORRECT AND ITS HEADLINE WAS READ THE WRONG WAY — 2026-08-10.
# Nothing below is being changed; the semantics are right and the first
# consumer's job was to say so or say why not. What has to be said loudly is
# what the 94% does NOT mean. "Order-sensitive" was read as "noise, and
# therefore not ours to fix". It is not: 82% of that 94% was OUR OWN DEFAULT
# ORDER BEING THE WRONG ONE. `Area::chunks()` walks x outer, z inner, and every
# reference on this window was harvested by `GEN_ORDER=rows`, which forceloads
# one z row at a time — z outer, x inner, i.e. `zx`. Match the two and the N=8
# charge goes 14,270 -> 2,450.
#
# The missing question is the adjudicating one, and this script cannot ask it:
# it counts what each order MOVES, which cannot say whether moving helps.
# `ci/order-charge-sweep.py` scores the CHARGE under each order, costs seconds,
# needs no dumps, and reconstructs this script's own default charge to the
# block as its self-check. Run both. ORACLE rule 29, docs/DETERMINISM.md §14.
#
# THE METHODOLOGICAL POINT, learned by getting it wrong first: a per-kind
# vanilla-vs-vanilla floor CANNOT see this. That floor varies the references
# and never varies OUR order, so it is blind to our own order sensitivity.
# Ranked by per-kind floor alone, `diorite -> andesite` looked like the top
# item at 1,642 against a floor of 764; it is 0/1,642 order-stable, i.e. pure
# noise. An item has to clear BOTH filters.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
D=./target/release/worldgen-dump
G=./target/release/region-differ
# `ORDSTAB_OUT` is not decoration. The dump loop below **skips a dump whose
# region file already exists**, so a second run left on the default path
# silently re-diffs the first run's dumps against a new base and reports the
# old generator's order sensitivity under a new commit's name. One run per
# tree, one directory per run.
OUT=${ORDSTAB_OUT:-~/rp-logs/decomp-orders}
mkdir -p $OUT
BASE="${ORDSTAB_BASE:?set ORDSTAB_BASE to the default-order dump region dir}"
for order in zx zx_rev zx_zrev reverse spiral spiral_in spawn diag; do
    d=$OUT/dump-$order
    if [ ! -s "$d/region/r.1.1.mca" ]; then
        rm -rf "$d"
        RUSTMC_DECORATE_ORDER="$order" $D full 20260722 16 "$d" 64 64 \
            > $OUT/dump-$order.log 2>&1
        echo "dump $order rc=$?"
    fi
    $G diff "$BASE" "$d/region" --bbox 48 48 80 80 --positions \
        > $OUT/default-vs-$order.pos 2>&1
    echo "$order $(grep -E '^DIFF' $OUT/default-vs-$order.pos)"
done
echo "ORDERSTAB DONE"
