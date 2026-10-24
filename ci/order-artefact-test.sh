#!/usr/bin/env bash
# order-artefact-test.sh — is the winning chunk order a property of VANILLA, or
# of how this project harvests its references?
#
# `ci/order-charge-sweep.py` found the r=16 admissible charge falls from 14,270
# to 2,450 when our decoration stage walks `zx` (z outer, x inner ascending)
# instead of the default `Area::chunks()` (x outer, z inner).  Before that is
# read as a defect, note what `zx` is: `ci/gen-vanilla-region.sh GEN_ORDER=rows`
# forceloads **one z row at a time, z ascending**, so the references were
# themselves harvested z-outer-x-inner.  The winning order may be the harvest
# order and nothing else.
#
# The prediction that separates the two, written down before the run:
#
#   * if it is the HARVEST order, then against `rows_rev` references (z
#     descending) `zx_zrev` must win and `zx` must lose by about as much;
#   * if it is something intrinsic to vanilla, `zx` wins against both.
#
# Controls, all of them measured here rather than assumed:
#
#   * `rows-i` is a NINTH `rows` reference taken the same day.  It is the
#     null arm: adding a same-order reference must not move which order wins.
#   * our own dumps are re-taken in this tree and `diff -rq`'d against the ones
#     `ci/order-stability.sh` produced in another tree.  Identical bytes are
#     the determinism control — without it "order changes the world" cannot be
#     told from "our generator is not reproducible".
#   * `rows-a vs rows-i` (same order, different run) beside `rows-a vs
#     rowsrev-a` (different order) is vanilla's own answer to the brief's
#     question: is vanilla order-dependent at all, and by how much.
#
# Vanilla first in every region-differ call.
set -uo pipefail
# OUTPUTS LIVE OUTSIDE $RUSTMC_REMOTE_DIR -- see ci/order-vanilla-refs.sh.
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
D=./target/release/worldgen-dump
G=./target/release/region-differ
MY=${ORDART_OUT:-$HOME/rp-logs-order/order-work}
REFS=${ORDART_REFS:-$HOME/rustmc/oracle-work}
NEW=${ORDART_NEW:-$HOME/rp-logs-order/order-refs}
PEER=${ORDART_PEER:-$HOME/rp-logs/decomp-orders}
PEERBASE=${ORDART_PEERBASE:-$HOME/rustmc-decomp/oracle-work/decomp-ours-r16/region}
mkdir -p "$MY/pos"

dump() {  # <name> <order-or-empty>
    local name="$1" order="$2"
    [ -s "$MY/dump-$name/region/r.1.1.mca" ] && { echo "have dump-$name"; return; }
    rm -rf "$MY/dump-$name"
    if [ -z "$order" ]; then
        $D full 20260722 16 "$MY/dump-$name" 64 64 > "$MY/dump-$name.log" 2>&1
    else
        RUSTMC_DECORATE_ORDER="$order" $D full 20260722 16 "$MY/dump-$name" 64 64 \
            > "$MY/dump-$name.log" 2>&1
    fi
    echo "dump-$name rc=$?"
}

echo "== our dumps, this tree"
dump default ""
dump default2 ""
for o in zx zx_rev zx_zrev reverse spiral spiral_in spawn diag; do dump "$o" "$o"; done

echo "== CONTROL: our generator is reproducible"
if diff -rq "$MY/dump-default/region" "$MY/dump-default2/region" >/dev/null 2>&1; then
    echo "  same order twice, same tree      IDENTICAL"
else
    echo "  same order twice, same tree      *** DIFFERS ***"
fi
if [ -d "$PEERBASE" ]; then
    if diff -rq "$MY/dump-default/region" "$PEERBASE" >/dev/null 2>&1; then
        echo "  default vs the other tree's      IDENTICAL"
    else
        echo "  default vs the other tree's      *** DIFFERS ***"
    fi
fi
for o in zx spawn reverse; do
    [ -d "$PEER/dump-$o/region" ] || continue
    if diff -rq "$MY/dump-$o/region" "$PEER/dump-$o/region" >/dev/null 2>&1; then
        echo "  $o vs the other tree's           IDENTICAL"
    else
        echo "  $o vs the other tree's           *** DIFFERS ***"
    fi
done

echo "== our default vs each order, this tree"
for o in zx zx_rev zx_zrev reverse spiral spiral_in spawn diag; do
    p="$MY/pos/default-vs-$o.pos"
    [ -s "$p" ] || $G diff "$MY/dump-default/region" "$MY/dump-$o/region" \
        --bbox 48 48 80 80 --positions > "$p" 2>&1
    echo "  $o $(grep -E '^DIFF' "$p")"
done

echo "== new references vs our default   (vanilla first)"
for r in rowsrev-a rowsrev-b rows-i; do
    [ -d "$NEW/$r" ] || { echo "  no $r yet"; continue; }
    p="$MY/pos/$r-vs-ours.pos"
    [ -s "$p" ] || $G diff "$NEW/$r" "$MY/dump-default/region" \
        --bbox 48 48 80 80 --positions > "$p" 2>&1
    echo "  $r $(grep -E '^DIFF' "$p")"
done

echo "== vanilla against vanilla: same order vs different order"
pair() {  # <label> <dirA> <dirB>
    local p="$MY/pos/$1.pos"
    [ -s "$p" ] || $G diff "$2" "$3" --bbox 48 48 80 80 --positions > "$p" 2>&1
    echo "  $1 $(grep -E '^DIFF' "$p")"
}
pair rowsa-vs-rowsi     "$REFS/gen-wide-20260722-r16-ow-a" "$NEW/rows-i"
pair rowsa-vs-rowsreva  "$REFS/gen-wide-20260722-r16-ow-a" "$NEW/rowsrev-a"
pair rowsb-vs-rowsrevb  "$REFS/gen-wide-20260722-r16-ow-b" "$NEW/rowsrev-b"
pair rowsreva-vs-rowsrevb "$NEW/rowsrev-a" "$NEW/rowsrev-b"
echo "ORDART DONE"
