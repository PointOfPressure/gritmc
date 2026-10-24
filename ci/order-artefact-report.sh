#!/usr/bin/env bash
# order-artefact-report.sh — score every chunk order against reference
# ensembles built along DIFFERENT order axes, and see whether the winner moves.
#
# Run after `ci/order-artefact-test.sh`. Reads only `.pos` files.
#
#   1  rows ensemble, this tree's dumps   — reproduction control. Must give the
#      same charges as the other tree's dumps did, or nothing below means
#      anything.
#   2  rows_rev ensemble (N=2)            — THE TEST. If the winning order is
#      the reference harvest order, `zx_zrev` wins here and `zx` loses.
#   3  rows ensemble cut to N=2           — the like-for-like arm for 2, so a
#      change in the winner cannot be blamed on the ensemble shrinking.
#   4  every reference, both row orders   — what the admissible set looks like
#      when it is sampled along the order axis at all.
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"
MY=${ORDART_OUT:-$HOME/rp-logs-order/order-work}
PEERPOS=${ORDART_PEERPOS:-$HOME/rp-logs/decomp-pos}
C=$MY/pos-all
mkdir -p "$C"
for f in "$PEERPOS"/*.pos; do ln -sfn "$f" "$C/$(basename "$f")"; done
for f in "$MY"/pos/*-vs-ours.pos; do ln -sfn "$f" "$C/$(basename "$f")"; done
# ci/order-charge-sweep.py wants `<r1>-vs-<r2>.pos`; the pair files are named
# for the run rather than the reference, so alias them.
ln -sfn "$MY/pos/rowsreva-vs-rowsrevb.pos" "$C/rowsrev-a-vs-rowsrev-b.pos"
ln -sfn "$MY/pos/rowsa-vs-rowsi.pos"       "$C/a-vs-rows-i.pos"
ln -sfn "$MY/pos/rowsa-vs-rowsreva.pos"    "$C/a-vs-rowsrev-a.pos"
ln -sfn "$MY/pos/rowsb-vs-rowsrevb.pos"    "$C/b-vs-rowsrev-b.pos"

echo "############ 1  rows ensemble N=8, THIS TREE'S dumps (reproduction control)"
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs a,b,c,d,e,f,g,h --with-u --expect-default 14270
echo "rc=$?"
echo
echo "############ 2  rows_rev ensemble N=2 — THE TEST"
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs rowsrev-a,rowsrev-b --with-u
echo
echo "############ 3  rows ensemble N=2 — like-for-like control for 2"
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs a,b --with-u
echo
echo "############ 4  both row orders, N=11"
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs a,b,c,d,e,f,g,h,rows-i,rowsrev-a,rowsrev-b
echo
echo "############ 5  N=9, all rows (a..h + a ninth rows reference)"
echo "#    The null arm for 4: if the collapse at N=11 is just N growing, this"
echo "#    collapses too. DETERMINISM §13's ladder says ours is flat from N=5."
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs a,b,c,d,e,f,g,h,rows-i
echo
echo "############ 6  N=10, a..h plus the two rows_rev references"
echo "#    Same N-1 as 5 plus one, but the two added references vary the ORDER."
python3 ci/order-charge-sweep.py --refdir "$C" --orderdir "$MY/pos" \
    --refs a,b,c,d,e,f,g,h,rowsrev-a,rowsrev-b --kinds-for zx
echo "ORDART REPORT DONE"
