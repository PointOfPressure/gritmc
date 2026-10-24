#!/usr/bin/env bash
# decorate-order-intersect.sh — how much of the residual is OURS, per position.
#
# `ci/decorate-order-sweep.sh` is a gate and reports one *total* per order.
# Totals cannot answer the question that matters, because a residual of 1,130
# under one order and 3,767 under another says nothing about whether they are
# the **same** 1,130 blocks. This walks the same orders, keeps every mismatch
# *position*, and intersects them.
#
# The inference, which WORKLIST has asserted for months without ever taking
# the measurement: a mismatch that moves when the chunk visitation order moves
# is order noise, because vanilla's own answer moves with vanilla's traversal
# too (`vanilla-r7` against `van-rows-c64`, one seed and one extent, differ by
# 13,366 blocks). A mismatch that survives **every** order is a candidate port
# defect.
#
# First reading, 2026-07-30, seed 20260722, r=7 centre (64,64), twin-masked
# against `vanilla-r7-b`, on the Threadripper:
#
#   default 1130   diag 1130   spawn 2682   zx 3767   spiral_in 18328
#   spiral 26224   zx_zrev 38826   zx_rev 38984   reverse 59218
#   intersection of all nine: 51
#
# So **95.5% of the twin-masked overworld residual is chunk-visitation order**,
# and 1,024 of the 1,130 fall out on the first perturbation alone. `diag` is
# byte-identical to `default` — checked here with `diff -rq`, not assumed — so
# the nine-order sweep is eight orders and always has been.
#
# Read it as a bound, not a verdict, in both directions. Eight of our orders
# cannot find a position that only a ninth would move, so the intersection is
# an **upper** bound on our exactness; and an order that changes the terrain
# under a genuine defect can hide it, so it is a **lower** bound on the defect
# count. It is the same logic as `region-differ --twin` and it has the same
# caveat.
#
#   ISECT_SEED=20260722
#   ISECT_RADIUS=7
#   ISECT_CENTRE="64 64"
#   ISECT_REF=oracle-work/vanilla-r7        vanilla FIRST, always
#   ISECT_TWIN=oracle-work/vanilla-r7-b     omit to skip the twin mask
#   ISECT_BBOX="57 57 71 71"                optional window inside the dump
#   ISECT_YBOX="0 127"                      mandatory in the End and nether
#   ISECT_DIM=overworld|nether|end
#   ISECT_OUT=/tmp/order-intersect          dumps + position lists; keep it
#                                           OUTSIDE RUSTMC_REMOTE_DIR, which
#                                           ci/t0-remote.sh rsyncs --delete
#
# Runs on the Threadripper. Nothing compiles on the laptop.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

SEED="${ISECT_SEED:-20260722}"
RADIUS="${ISECT_RADIUS:-7}"
CENTRE="${ISECT_CENTRE:-64 64}"
REF="${ISECT_REF:-oracle-work/vanilla-r7}"
TWIN="${ISECT_TWIN:-oracle-work/vanilla-r7-b}"
BBOX="${ISECT_BBOX:-}"
YBOX="${ISECT_YBOX:-}"
DIM="${ISECT_DIM:-overworld}"
OUT="${ISECT_OUT:-/tmp/order-intersect}"
ORDERS="default zx zx_rev zx_zrev reverse spiral spiral_in spawn diag"

case "$DIM" in
overworld) SUB=full ;;
end) SUB=full-end ;;
nether) SUB=full-nether ;;
*)
    echo "intersect: unknown ISECT_DIM=$DIM" >&2
    exit 2
    ;;
esac
[ -d "$REF" ] || { echo "intersect: no reference at $REF" >&2; exit 2; }
# A twin that is not there is the most dangerous input this script takes.
# `region-differ` exits non-zero and prints nothing, the grep below sees an
# empty stream, and every order reports **zero mismatches** — which reads as a
# perfect port rather than as a broken run. That happened on 2026-08-02: seed
# 31415926 was measured against `oracle-work/van-s2-c62`, which existed on the
# laptop and had never been copied to the Threadripper, and the script cheerfully
# reported an order-invariant core of 0 for a window whose previous measured
# core was 260. Named, not silent, and checked before anything is generated.
if [ -n "$TWIN" ] && [ ! -d "$TWIN" ]; then
    echo "intersect: ISECT_TWIN=$TWIN does not exist — a missing twin makes" >&2
    echo "           every order report zero. Set it to a real directory, or" >&2
    echo "           set ISECT_TWIN= to run without the mask deliberately." >&2
    exit 2
fi

cargo build --release -p worldgen-dump >/dev/null 2>&1 &&
    cargo build --release -p region-differ >/dev/null 2>&1 || {
    echo "intersect: build failed" >&2
    exit 2
}
D=./target/release/worldgen-dump
G=./target/release/region-differ

mkdir -p "$OUT"
digests=""
for order in $ORDERS; do
    d="$OUT/w-$order"
    rm -rf "$d"
    # `default` is the knob unset, not a value the driver recognises: passing
    # a name it does not know would still print a digest line and look like a
    # ninth order while walking the eighth.
    if [ "$order" = default ]; then
        env -u RUSTMC_DECORATE_ORDER "$D" "$SUB" "$SEED" "$RADIUS" "$d" $CENTRE \
            >"$d.log" 2>&1
    else
        RUSTMC_DECORATE_ORDER="$order" "$D" "$SUB" "$SEED" "$RADIUS" "$d" $CENTRE \
            >"$d.log" 2>&1
    fi
    [ -d "$d/region" ] || { echo "$order: dump failed (see $d.log)" >&2; exit 2; }
    digests="$digests$order $(sed -n 's/.*digest=\([0-9a-f]*\).*/\1/p' "$d.log" | head -1)
"
    # shellcheck disable=SC2086  # BBOX/YBOX are deliberately word-split
    #
    # The differ's output goes to a file FIRST and is filtered afterwards, so
    # that its exit status is the differ's and not the grep's. Piping it
    # straight into `grep` threw the status away, and with `2>/dev/null` on top
    # a failed diff was indistinguishable from a diff that found nothing.
    "$G" diff "$REF" "$d/region" ${TWIN:+--twin $TWIN} \
        ${BBOX:+--bbox $BBOX} ${YBOX:+--ybox $YBOX} --positions \
        > "$OUT/raw-$order.txt" 2> "$OUT/raw-$order.err" || {
        echo "$order: region-differ failed —" >&2
        cat "$OUT/raw-$order.err" >&2
        exit 2
    }
    grep -E '^-?[0-9]+ -?[0-9]+ -?[0-9]+ ' "$OUT/raw-$order.txt" \
        > "$OUT/full-$order.txt" || true
    grep -E '^DIFF' "$OUT/raw-$order.txt" >&2 || {
        echo "$order: region-differ printed no DIFF line — refusing to read" >&2
        echo "         its silence as a zero" >&2
        exit 2
    }
    awk '{print $1" "$2" "$3}' "$OUT/full-$order.txt" | sort > "$OUT/p-$order.txt"
    echo "$order  $(wc -l < "$OUT/p-$order.txt")"
done

# Two orders that produce the same bytes are one order wearing two names, and
# an intersection over them overstates how many orders it survived.
echo "--- bytes, against default:"
for order in $ORDERS; do
    [ "$order" = default ] && continue
    if diff -rq "$OUT/w-default/region" "$OUT/w-$order/region" >/dev/null 2>&1; then
        echo "  $order IDENTICAL to default"
    else
        echo "  $order differs"
    fi
done

echo "--- intersection:"
cp "$OUT/p-default.txt" "$OUT/isect.txt"
for order in $ORDERS; do
    [ "$order" = default ] && continue
    comm -12 "$OUT/isect.txt" "$OUT/p-$order.txt" > "$OUT/isect.next"
    mv "$OUT/isect.next" "$OUT/isect.txt"
    echo "  after $order: $(wc -l < "$OUT/isect.txt")"
done

echo "--- the survivors, by family:"
awk 'NR==FNR { keep[$1" "$2" "$3]=1; next }
     ($1" "$2" "$3) in keep {
        va=$4; vb=$5; sub(/\[.*/, "", va); sub(/\[.*/, "", vb)
        if (va == vb) k = va "  STATE-ONLY"; else k = va " -> " vb
        n[k]++
     }
     END { for (k in n) printf "%5d  %s\n", n[k], k }' \
    "$OUT/isect.txt" "$OUT/full-default.txt" | sort -rn

# The order digest is of the *traversal*, not of the world, and the two are
# not the same test: `diag` has a digest unlike any other order here and still
# writes default's bytes exactly. `decorate-order-sweep.sh` checks only that
# the digests are distinct, which is why it has counted nine orders while
# walking eight. `default` prints none — the knob is unset, by design.
echo "--- traversal digests (distinct digests, NOT distinct worlds):"
printf '%s' "$digests"
