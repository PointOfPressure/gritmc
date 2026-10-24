#!/usr/bin/env bash
# order-innocent-floor.sh — what does the admissible instrument charge a run it
# cannot possibly have a complaint about?
#
# §14's floor substitutes an *innocent* candidate — an unmodified vanilla run —
# for ours and charges it the same way, which is what turns the charge into a
# number with a floor beside it.  Every one of those innocent candidates was
# harvested `GEN_ORDER=rows`, exactly like the references.  So that floor
# measures the instrument's sensitivity to vanilla's scheduler jitter and to
# nothing else.
#
# This substitutes an innocent candidate that differs from the references in
# **one controlled way**: `rowsrev-a` is unmodified vanilla of the same seed and
# window, generated with the chunk rows walked z-descending instead of
# z-ascending.  Nothing about it is a port defect.  Whatever the instrument
# charges it is the price of an uncontrolled chunk order, and our own charge has
# to be read against that and not against zero.
#
# Vanilla first in every region-differ call.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
G=./target/release/region-differ
MY=${ORDART_OUT:-$HOME/rp-logs-order/order-work}
REFS=${ORDART_REFS:-$HOME/rustmc/oracle-work}
NEW=${ORDART_NEW:-$HOME/rp-logs-order/order-refs}
C=$MY/pos-all
mkdir -p "$C"

# `rows-i` is the null arm: an innocent candidate harvested in the SAME order as
# the references. Its charge is §14's floor re-measured on a run taken today,
# and the pair of numbers is the whole point — one candidate differs from the
# ensemble by scheduler jitter, the other by scheduler jitter AND chunk order.
for cand in rowsrev-a rows-i; do
    echo "== eight references against $cand   (innocent vanilla, order $( [ "$cand" = rows-i ] && echo rows || echo rows_rev ))"
    for r in a b c d e f g h; do
        p="$C/$r-vs-$cand.pos"
        [ -s "$p" ] || $G diff "$REFS/gen-wide-20260722-r16-ow-$r" "$NEW/$cand" \
            --bbox 48 48 80 80 --positions > "$p" 2>&1
    done
    python3 - "$C" "$cand" <<'PY'
import sys, os
c, cand = sys.argv[1], sys.argv[2]
acc = None
for r in "abcdefgh":
    s = set()
    with open(os.path.join(c, f"{r}-vs-{cand}.pos")) as fh:
        for line in fh:
            f = line.split()
            if len(f) >= 5:
                try:
                    s.add((int(f[0]), int(f[1]), int(f[2])))
                except ValueError:
                    pass
    acc = s if acc is None else (acc & s)
    print(f"  N={len('abcdefgh'[:'abcdefgh'.index(r)+1]):>2}  |{cand} not in V| = {len(acc):>9,}")
PY
    echo
done
echo "ORDFLOOR DONE"
