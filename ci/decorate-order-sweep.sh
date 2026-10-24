#!/usr/bin/env bash
# decorate-order-sweep.sh — the order-sweep gate for a parallel decoration change.
#
# **It sweeps nine names and eight orders.** `diag` produces a region tree
# byte-identical to `default` — verified with `diff -rq`, not inferred, by
# `ci/decorate-order-intersect.sh` on 2026-07-30 — while carrying a traversal
# digest unlike any other. So the digest-uniqueness check below is necessary
# and *not* sufficient: distinct digests do not imply distinct worlds, and
# this script has counted nine orders while walking eight since it was
# written. Nothing measured with it is wrong; the coverage claim was.
#
# WORKLIST's NOW block makes this the gate a parallel-decoration commit cannot
# skip. It sweeps `RUSTMC_DECORATE_ORDER` over all nine names and, for each,
# compares the region files a **pristine control build** produces sequentially
# against the ones the working build produces at full width.
#
# It is a stronger test than re-running the parity diff nine times, and much
# cheaper. The parallel pass is designed to reproduce the sequential order's
# answer exactly — it only relaxes the order between chunks that cannot touch
# each other — so if that holds, each order's *bytes* are unchanged and every
# parity number ever taken from it is unchanged with them. `cmp` says so
# directly; a re-measured parity percentage would only say it did not move by
# more than the noise.
#
# It also does what the region-differ cannot: it covers the orders whose
# vanilla reference does not exist on this box.
#
#   CTL=~/rustmc-i2p-ctl   a pristine `git archive` checkout of the parent
#                          commit, already built
#   SWEEP_SEED=20260722
#   SWEEP_RADIUS=16
#   SWEEP_CENTRE="64 64"
#   SWEEP_WORKERS=12       the width the parallel side runs at
#
# ## Second mode: SWEEP_REF — nine orders against a real vanilla world
#
# `SWEEP_REF=<vanilla reference dir>` switches the comparison. Instead of
# control-bytes vs parallel-bytes, each order's dump is diffed against a
# **vanilla** reference with `region-differ`, and every order must report the
# same mismatch count as the reference's own baseline (0, where the dimension
# is at 100%).
#
# That mode exists because the bytes-vs-bytes one cannot answer item 1 for a
# dimension whose decoration has no parallel path to compare against — the End
# has none — and because agreeing with your own control build under nine orders
# is a weaker claim than agreeing with vanilla under nine orders. Where both
# are available, run both: they fail for different reasons.
#
# **A per-order mismatch in SWEEP_REF mode is not by itself a defect, and this
# has already cost a turn.** The mode compares nine of *our* orders against
# **one** vanilla world, which was generated in one traversal. Vanilla's own
# features read the world back, so vanilla's answer moves with vanilla's
# traversal too, and a reference is only authoritative for the orders that sit
# in the same plane as the harvest that produced it. Measured 2026-07-30, seed
# 20260722, r=7, y 0..127: the End's `reverse`/`zx_zrev` miss
# `van-end-20260722-r7` by 253 blocks and hit `van-end-20260722-r7-rowsrev` —
# a vanilla world of the same seed and extent, forceloaded z descending — by
# **0**. The 253 was vanilla's order floor the whole time. So before treating
# an order's count as ours, harvest vanilla in that order's plane with
# `GEN_ORDER=rows_rev` and diff against that.
#
#   SWEEP_REF=oracle-work/van-end-20260722-r7
#   SWEEP_DIM=end          `worldgen-dump full-end` rather than `full`
#   SWEEP_BBOX="57 57 71 71"   the measured window inside the dump
#   SWEEP_YBOX="0 127"     mandatory in the End and the nether: half of a
#                          256-tall canvas is guaranteed air on both sides and
#                          including it inflates the denominator
#   SWEEP_EXPECT=0         the reference's own baseline, in blocks
#   SWEEP_OUT=~/end-sweep  dump root; keep it OUTSIDE RUSTMC_REMOTE_DIR,
#                          because ci/t0-remote.sh rsyncs with --delete
#
# **The differ takes VANILLA FIRST** and this script passes `$SWEEP_REF` first
# for that reason. Reversing it inverts the sign of every number while still
# printing a perfectly plausible one.
#
# Both modes check that the traversal *actually changed*: `decorate_order_for`
# prints one `decorate-order:` line per run with a digest of the sequence, and
# a sweep whose nine digests are not nine distinct values is reported as a
# failure however good its mismatch counts look. A sweep that silently ran the
# default nine times passes every other check in here.
#
# Runs on the Threadripper. Nothing compiles on the laptop.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

CTL="${CTL:-$HOME/rustmc-i2p-ctl}"
SEED="${SWEEP_SEED:-20260722}"
RADIUS="${SWEEP_RADIUS:-16}"
CENTRE="${SWEEP_CENTRE:-64 64}"
WORKERS="${SWEEP_WORKERS:-12}"
REF="${SWEEP_REF:-}"
DIM="${SWEEP_DIM:-overworld}"
BBOX="${SWEEP_BBOX:-}"
YBOX="${SWEEP_YBOX:-}"
EXPECT="${SWEEP_EXPECT:-0}"
OUT="${SWEEP_OUT:-$HOME/order-sweep}"
ORDERS="raster reverse zx zx_rev zx_zrev diag spiral spiral_in spawn"
case "$DIM" in
overworld) SUB=full ;;
end) SUB=full-end ;;
nether) SUB=full-nether ;;
*)
    echo "sweep: unknown SWEEP_DIM=$DIM" >&2
    exit 2
    ;;
esac

cargo build --release -p worldgen-dump >/dev/null 2>&1 || {
    echo "sweep: build failed" >&2
    exit 2
}

# Every run's digest line, so uniqueness can be checked at the end. A sweep
# that ran one order nine times is otherwise indistinguishable from a pass.
digests=""
note_digest() {
    # $1 = order, $2 = the run's stderr log
    local d
    d="$(sed -n 's/.*digest=\([0-9a-f]*\).*/\1/p' "$2" | head -1)"
    if [ -z "$d" ]; then
        echo "$1: NO decorate-order line — the order knob never reached the stage" >&2
        return 1
    fi
    digests="$digests$1 $d
"
    return 0
}
check_digests() {
    local n u want
    n="$(printf '%s' "$digests" | grep -c . || true)"
    u="$(printf '%s' "$digests" | awk '{print $2}' | sort -u | grep -c . || true)"
    # `spiral` rings out from the window centre and `spawn` rings out from
    # (0,0), so at centre (0,0) they are the same function and produce the same
    # sequence. That is one legitimate collision, not a broken sweep — measured
    # at the End's central window, where the check said "8 distinct of 9" and
    # the missing one was exactly that pair. Every other collision is a
    # failure: two differently-named orders that walk the same sequence prove
    # the knob is not being honoured.
    want="$n"
    case "$CENTRE" in
    "0 0") want=$((n - 1)) ;;
    esac
    echo "digests: $u distinct of $n runs (expect $want)"
    printf '%s' "$digests"
    [ "$n" -gt 0 ] && [ "$u" -eq "$want" ]
}

# ---- SWEEP_REF mode: each order against a vanilla reference ----
if [ -n "$REF" ]; then
    [ -d "$REF" ] || {
        echo "sweep: no reference at $REF" >&2
        exit 2
    }
    cargo build -p region-differ >/dev/null 2>&1 || {
        echo "sweep: region-differ build failed" >&2
        exit 2
    }
    fail=0
    mkdir -p "$OUT"
    for order in $ORDERS; do
        d="$OUT/$order"
        rm -rf "$d"
        # `raster` is the unset default; the driver only recognises the other
        # eight. It is still passed through, so its digest line is printed and
        # the nine-distinct check covers it too.
        RUSTMC_DECORATE_ORDER="$order" RUSTMC_WORKERS=1 \
            ./target/release/worldgen-dump "$SUB" "$SEED" "$RADIUS" "$d" $CENTRE \
            >"$d.log" 2>&1 || {
            echo "$order: dump failed (see $d.log)" >&2
            fail=1
            continue
        }
        note_digest "$order" "$d.log" || fail=1
        # shellcheck disable=SC2086  # BBOX/YBOX are deliberately word-split
        out="$(./target/debug/region-differ diff "$REF" "$d/region" \
            ${BBOX:+--bbox $BBOX} ${YBOX:+--ybox $YBOX} 2>&1 | grep '^DIFF')"
        miss="$(printf '%s' "$out" | sed -n 's/.*mismatches=\([0-9]*\).*/\1/p')"
        blocks="$(printf '%s' "$out" | sed -n 's/.*blocks=\([0-9]*\).*/\1/p')"
        echo "$order: $miss / $blocks mismatched blocks (expect $EXPECT)"
        [ "$miss" = "$EXPECT" ] || fail=1
    done
    check_digests || fail=1
    exit "$fail"
fi

[ -x "$CTL/target/release/worldgen-dump" ] || {
    echo "sweep: no control build at $CTL" >&2
    exit 2
}

fail=0
for order in $ORDERS; do
    # `raster` is the unset default; the driver only recognises the other
    # eight, so passing it through as an unknown value is exactly the default.
    rm -rf /tmp/sweep-ctl /tmp/sweep-par
    RUSTMC_DECORATE_ORDER="$order" RUSTMC_WORKERS=1 \
        "$CTL/target/release/worldgen-dump" full "$SEED" "$RADIUS" /tmp/sweep-ctl $CENTRE \
        >/dev/null 2>&1 || { echo "$order: control run failed" >&2; fail=1; continue; }
    RUSTMC_DECORATE_ORDER="$order" RUSTMC_WORKERS="$WORKERS" \
        ./target/release/worldgen-dump full "$SEED" "$RADIUS" /tmp/sweep-par $CENTRE \
        >/tmp/sweep-par.log 2>&1 || { echo "$order: parallel run failed" >&2; fail=1; continue; }
    note_digest "$order" /tmp/sweep-par.log || fail=1
    differ=0
    total=0
    for f in /tmp/sweep-ctl/region/*.mca; do
        total=$((total + 1))
        cmp -s "$f" "/tmp/sweep-par/region/$(basename "$f")" || differ=$((differ + 1))
    done
    echo "$order: $differ of $total region files differ"
    [ "$differ" -eq 0 ] || fail=1
done
rm -rf /tmp/sweep-ctl /tmp/sweep-par /tmp/sweep-par.log
check_digests || fail=1
exit "$fail"
