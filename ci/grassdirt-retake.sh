#!/usr/bin/env bash
# grassdirt-retake.sh — re-take the seed-20260722 r=16 overworld charge from
# THIS tree, in the matched chunk order, and adjudicate it per block.
#
# WHAT THIS REPRODUCES
#
#   GD_SEED=20260722 (default), refs a..h, N=8
#   base tip cef0f06b     charge 2,436   grass_block[snowy=false] -> dirt  646
#   with the fix          charge 1,786   grass_block[snowy=false] -> dirt    0
#                         delta -650, newly charged 0
#
#   GD_SEED=12345, refs c..g, N=5 — the window that can state a LEVEL
#   base tip cef0f06b     charge 7,940   99.992583%
#   with the fix          charge 7,704   99.992804%
#                         delta -236, newly charged 0
#
# and the closing figures `ow-r16-20260722-topmaterial-delta` and
# `ow-r16-12345-admissible-topmaterial`.
#
# THE SEED-12345 ARM IS THE QUOTABLE HALF and it is a different kind of
# result: 20260722 is registered `quotable = false` and supports a delta only,
# while 12345 clears its floor and carries an exactness figure. The same fix
# moves both, which is what "fixes generalise" means here. On 12345 the blocks
# come back as `grass_block[snowy=true]` (213 of 236) rather than
# `[snowy=false]`, because the surface rule is genuinely being evaluated at
# the exposed block and it sees the snow — a stub could not produce that.
#
# The base tip reproduces the registered 7,940 on seed 12345 EXACTLY, split
# 6,854 / 509 / 577, which is the positive control on that arm. It does NOT
# reproduce the registered 2,450 on seed 20260722 — see control 3 below.
#
# THE ORDER IS READ, NEVER ASSUMED. ORACLE rule 29 and rule 30: a reference
# carries a chunk order and the dump must match it, and an unstamped reference
# is not a known one. On seed 20260722 the eight references this window is
# scored against carry no `HARVEST-ORDER` file at all —
# `ci/classify-20260722-order.sh` recovers their order from the data with
# controls — so the order is read off `-i`, which IS stamped. On seed 12345 it
# is read off `-c`. Either way the anchor is a reference stamped AT
# GENERATION, and the script refuses rather than defaulting if that stamp says
# anything it does not recognise.
#
# THE THREE CONTROLS, and none of them is optional.
#
# 1. WORKER COUNT. This dumps at RUSTMC_WORKERS=6 because the box is shared,
#    while the stored ladder dump was taken at the default pool. That is only
#    admissible because the two are byte-identical over this window:
#
#      ./target/release/region-differ diff <dump@6>/region <dump@default>/region \
#          --bbox 48 48 80 80
#      -> mismatches=0   EXACTNESS 100.000000%
#
#    Re-take that if you change anything about threading. A parity difference
#    hiding behind a worker count would invalidate every number here.
#
# 2. THE POISON CONTROL. Revert ONLY the `has_grass` arm in
#    `crates/rustmc-worldgen/src/carver/carve.rs` to the state-string compare
#    (`existing == "minecraft:grass_block"`), leaving the whole `top_material`
#    closure in place, and re-run. The charge must return to 2,436 and the
#    pair to 646, and `RUSTMC_CARVER_TOPMAT=1` must report 0 asks against
#    4,897 for the fixed build. A control that cannot fire is not a control:
#    this one distinguishes "the closure is routed through the measured path"
#    from "the closure exists".
#
# 3. THE BASELINE'S OWN TIP. The stored `dump-zx` that
#    `ow-r16-admissible-matched` (2,450) was measured on differs from a dump of
#    base cef0f06b by 14 blocks — stronghold brickwork in y32..47 around
#    (771..774, 44, 1035) — so it was not made from that tip. This script
#    therefore quotes 2,436 as the base and says so. The ITEM is unaffected:
#    649 / 648 / 650-clear-in-both reproduce on either dump.
#
# The vanilla-vs-vanilla half of the matrix is a property of the references
# alone and is reused rather than re-diffed; nothing here writes into
# `oracle-work` or into the ladder's directory.
#
# usage: ci/grassdirt-retake.sh <label>      e.g. `fixed`, `poison`, `base`
# env:   GD_SEED   20260722 (default) or 12345
#        GD_REFS   (default ~/rustmc/oracle-work)
#        GD_OUT    (default ~/gd2-logs)
#        GD_LADDER (default per seed — read-only, for the vanilla-vs-vanilla pairs)
#        RUSTMC_WORKERS (default 6 here)
set -uo pipefail

LABEL="${1:?usage: ci/grassdirt-retake.sh <label>}"
SEED="${GD_SEED:-20260722}"
REFS="${GD_REFS:-$HOME/rustmc/oracle-work}"
ROOT="${GD_OUT:-$HOME/gd2-logs}"
BBOX="48 48 80 80"

# The pool, the candidate set and the STAMPED anchor differ per seed. The
# anchor is the reference whose `HARVEST-ORDER` the dump order is read from,
# and it must be one that was stamped at generation (ORACLE rule 30).
case "$SEED" in
  20260722) PFX=gen-wide-20260722-r16-ow-; ALL="a b c d e f g h i"; CAND="a b c d e f g h"
            ANCHOR=i; LADDER="${GD_LADDER:-$HOME/rp-logs-ladder/pos}" ;;
  12345)    PFX=gen-wide-12345-r16-ow-;    ALL="c d e f g h";       CAND="c d e f g"
            ANCHOR=c; LADDER="${GD_LADDER:-$HOME/rp-logs-ladder/12345/overworld/pos}" ;;
  *) echo "REFUSING: GD_SEED must be 20260722 or 12345 (got '$SEED') — this"
     echo "          script's reference pools and floors are per window."; exit 2 ;;
esac
OUT="$ROOT/$LABEL"
POS="$ROOT/pos-$LABEL"

cd "$(dirname "$0")/.." || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

echo "== build"
cargo build --release -p worldgen-dump -p region-differ || exit 2
D=./target/release/worldgen-dump
G=./target/release/region-differ

MATCHED=$(grep -m1 -oP '^.*?dump_order=\K[a-z_]+' "$REFS/$PFX$ANCHOR/HARVEST-ORDER" 2>/dev/null)
case "$MATCHED" in
  zx|zx_zrev) echo "== matched dump_order '$MATCHED' read off $PFX$ANCHOR/HARVEST-ORDER" ;;
  *) echo "REFUSING: $PFX$ANCHOR has no recognised dump_order stamp (got '$MATCHED').";
     echo "          ORACLE rule 30 — an unrecognised knob is not a turned one."; exit 2 ;;
esac

rm -rf "$OUT" "$POS"; mkdir -p "$OUT" "$POS"

echo "== dump seed $SEED r=16 centre 64,64"
RUSTMC_WORKERS="${RUSTMC_WORKERS:-6}" RUSTMC_CARVER_TOPMAT=1 RUSTMC_DECORATE_ORDER="$MATCHED" \
  $D full "$SEED" 16 "$OUT" 64 64 > "$OUT/dump.log" 2>&1 || {
    echo "dump FAILED"; tail -20 "$OUT/dump.log"; exit 2; }
grep -E 'topMaterial|decorate-order' "$OUT/dump.log" | head -4

for r1 in $ALL; do for r2 in $ALL; do
  [ "$r1" = "$r2" ] && continue
  [ -f "$LADDER/$r1-vs-$r2.pos" ] && ln -sf "$LADDER/$r1-vs-$r2.pos" "$POS/$r1-vs-$r2.pos"
done; done

echo "== diffs, VANILLA FIRST"
for r in $ALL; do
  $G diff "$REFS/$PFX$r" "$OUT/region" --bbox $BBOX --positions \
      > "$POS/$r-vs-$LABEL.pos" 2>/dev/null || { echo "differ FAILED on $r"; exit 2; }
done

N=$(set -- $CAND; echo $#)
echo "== per-block adjudication, charge against the per-block floor at N=$N"
python3 ci/floor-census.py "$POS" --refs "$(echo $ALL | tr ' ' ,)" --top 8 \
    --label "seed-$SEED r=16 ow, matched-order charge, per block ($LABEL)" \
    --adjudicate "$LABEL" --cand-refs "$(echo $CAND | tr ' ' ,)" || exit 2

echo
echo "== the item, both marginals and the joint"
python3 ci/confusion-pair-census.py "$POS" --cand "$LABEL" --refs "$(echo $CAND | tr ' ' ,)" \
    --pair 'grass_block[snowy=false]->dirt' || exit 2
