#!/usr/bin/env bash
# nether-ladder-refs.sh — the vanilla references `docs/DETERMINISM.md` §4's
# nether column needs, with their harvest order stamped and varied.
#
# §4 declared its N=6 ladder un-re-takeable on 2026-08-09: the six references
# per dimension were lost in the 2026-08-07 disk failure and `ls oracle-work`
# finds no surviving r=16 nether reference at all. That was true of the cost of
# a reference at the time it was written. It costs **70 s**
# (`ci/order-vanilla-refs.sh`, measured 2026-08-10), so six of them is seven
# minutes and the ladder is affordable again.
#
# SIX ROWS AND TWO ROWS_REV, not eight rows, and that is the whole point of
# re-taking it rather than reproducing it. §4's ensemble sampled vanilla's
# scheduler eight times and vanilla's chunk order once, and ORACLE rule 29 puts
# 146,169 blocks on the overworld between two runs of different orders against
# 32,127 between two of the same — so an N-reference ensemble along one order
# is not an N-reference ensemble. The two `rows_rev` references are what let
# the nether ladder say which of its floor is scheduler and which is order,
# which the overworld ladder could not say until it had them.
#
# Outputs land in `oracle-work` as NEW directories. Nothing existing is
# regenerated: those are expensive, twinned and shared, and another tree is
# reading them.
#
#   NLR_OUT   where the references go   (default ~/rustmc/oracle-work)
#   NLR_DIM   overworld|nether|end      (default nether)
#   NLR_N     how many `rows` refs      (default 6)
#   NLR_NREV  how many `rows_rev` refs  (default 2)
#   NLR_PORT  first port to use         (default 25951)
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(cd "$(dirname "$0")/.." && pwd)"
OUT=${NLR_OUT:-$HOME/rustmc/oracle-work}
DIM=${NLR_DIM:-nether}
N=${NLR_N:-6}
NREV=${NLR_NREV:-2}
PORT=${NLR_PORT:-25951}
SEED=20260722
R=16
case "$DIM" in
    nether) TAG=nether ;;
    end)    TAG=end ;;
    overworld) TAG=ow ;;
    *) echo "unknown dimension $DIM" >&2; exit 2 ;;
esac

one() {  # <suffix> <GEN_ORDER>
    local d="$OUT/gen-wide-$SEED-r$R-$TAG-$1"
    if [ -d "$d" ]; then echo "have $(basename "$d")"; return 0; fi
    # Generate into a scratch name and move it into place only once it is whole.
    # A half-written reference directory is indistinguishable from a complete
    # one to every reader downstream, and reads as perfect agreement.
    local t="$d.partial"
    rm -rf "$t"
    GEN_ORDER="$2" ./ci/gen-vanilla-region.sh "$SEED" "$R" "$t" "$PORT" 64 64 "$DIM" \
        > "$OUT/gen-wide-$SEED-r$R-$TAG-$1.log" 2>&1
    local rc=$? n
    n=$(ls "$t"/*.mca 2>/dev/null | wc -l)
    if [ "$rc" != 0 ] || [ "$n" = 0 ]; then
        echo "  $1 ($2) FAILED rc=$rc, $n region files — left at $t"
        return 1
    fi
    mv "$t" "$d"
    echo "  $1 ($2) rc=$rc $n region files  $(head -1 "$d/HARVEST-ORDER")"
    PORT=$((PORT + 1))
}

echo "== $DIM r=$R seed=$SEED: $N rows + $NREV rows_rev references into $OUT"
i=0
while [ "$i" -lt "$N" ]; do
    one "$(printf '%b' "$(printf '\\x%02x' $((97 + i)))")" rows
    i=$((i + 1))
done
i=0
while [ "$i" -lt "$NREV" ]; do
    one "rr-$(printf '%b' "$(printf '\\x%02x' $((97 + i)))")" rows_rev
    i=$((i + 1))
done
echo "NLR DONE"
