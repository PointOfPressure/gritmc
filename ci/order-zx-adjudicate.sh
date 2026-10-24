#!/usr/bin/env bash
# order-zx-adjudicate.sh — measure the winning order DIRECTLY, and re-take the
# window's resolving power with it.
#
# `ci/order-charge-sweep.py` reconstructs each order's charge from `.pos` files
# rather than diffing that order's dump against the references, which is what
# makes it cost seconds instead of an hour.  A reconstruction is only as good as
# its argument, so this diffs the dump for real: eight references against the
# `zx` dump, straight through `ci/admissible-residual.py` and
# `ci/window-resolving-power.py`.  **If the direct charge is not the
# reconstructed one to the block, the reconstruction is wrong and everything
# built on it goes with it.**
#
# The second half is the part that decides what the window is worth afterwards.
# `ci/window-resolving-power.py` charges an *innocent* unmodified vanilla run
# against subsets of the others, which is the only floor this instrument has.
# Our charge falling below that floor would not be a triumph, it would be a
# result that needs saying carefully: a deterministic generator locked to the
# reference's own harvest order can beat a vanilla run that has scheduler
# jitter, and that is a statement about the ensemble, not about fidelity.
#
# Vanilla first in every region-differ call.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
G=./target/release/region-differ
MY=${ORDART_OUT:-$HOME/rp-logs-order/order-work}
REFS=${ORDART_REFS:-$HOME/rustmc/oracle-work}
CAND=${ORDZX_CAND:-zx}
C=$MY/pos-all
mkdir -p "$C"

echo "== eight references against our $CAND dump   (vanilla first)"
for r in a b c d e f g h; do
    p="$C/$r-vs-$CAND.pos"
    [ -s "$p" ] || $G diff "$REFS/gen-wide-20260722-r16-ow-$r" "$MY/dump-$CAND/region" \
        --bbox 48 48 80 80 --positions > "$p" 2>&1
    echo "  $r $(grep -E '^DIFF' "$p")"
done

echo
echo "== N=2 admissible charge, direct   (the reconstruction's cross-check)"
python3 ci/admissible-residual.py "$C/a-vs-$CAND.pos" "$C/b-vs-$CAND.pos" \
    --floor "$C/a-vs-b.pos"

echo
echo "== N=8 admissible charge, direct"
python3 - "$C" "$CAND" <<'PY'
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
print(f"  |{cand} not in V(p)| over eight references: {len(acc):,}")
PY

echo
echo "== resolving power with $CAND as the candidate"
python3 ci/window-resolving-power.py "$C" --refs a,b,c,d,e,f,g,h --cand "$CAND" \
    --max-n 7 --label "$CAND dump, seed 20260722 r=16 c(64,64) bbox 48 48 80 80"
echo "ORDZX DONE"
