#!/usr/bin/env bash
# Localise the `sulfur -> cinnabar` divergence to a single world READ.
#
# Why this exists, and why the existing instruments could not do it
# -----------------------------------------------------------------
# `docs/DETERMINISM.md` §21 established that the divergence is a world read
# inside `sulfur_spike_cluster`'s body, that it is triggered by chunk (78,47)
# decorating before (77,48), and that the only ours-vs-ours difference found
# in the overlapping columns is 57 blocks below the window the scan can see.
# A `javap` read of `Column.scan` then proved that gap unbridgeable: the walk
# is anchored on the column's own y and takes at most `range - 1` steps, with
# no clamp and no alternate anchor.
#
# The reason the census could not find the real interfering write is
# structural, and it is worth stating plainly: **a diff of our own two chunk
# orders is blind to anything our code writes in both of them.** Chunk
# (78,47) decorates identically in either order — only its *timing* moves — so
# whatever it writes into the overlap is present in both arms and cancels. The
# census could only ever see the downstream damage, never the cause.
#
# So this script does not diff worlds. It diffs what the cluster *saw*:
# `RUSTMC_TRACE_READS` logs every canvas read inside a box, tagged with the
# feature that asked, and the two arms are the same four chunks decorated in
# the two orders. The first line where the arms disagree is the interfering
# block, by construction — no reference, no bbox, no attribution census.
#
# This needs no vanilla reference, which matters: chunk (78,47) is at z=47,
# outside the r=16 reference set entirely, so nothing measured adjudicates it.
#
# Runtime: two full r=16 generations at RUSTMC_WORKERS=1. Only four chunks
# decorate, but every chunk still generates, so budget ~15 min per arm. It
# takes no lock and measures no wall time.
set -uo pipefail

ROOT=${ROOT:-$(cd "$(dirname "$0")/.." && pwd)}
cd "$ROOT" || exit 2
OUT=${OUT:-$ROOT/read-trace}
mkdir -p "$OUT"

# The cluster whose body diverges sits at (1245,24,771) in chunk (77,48) with
# `floor_to_ceiling_search_range: 12`, so it can see y 13..35 and no further.
# The box is that band, widened by a few blocks on each face so that a read
# just outside the predicted window is visible as a finding rather than
# silently cropped — a box sized exactly to the hypothesis can only confirm it.
BOX=${BOX:-1244,1255,0,40,760,775}

# `78,48,1` is the raster prefix of the 3x3 around (78,48) broken at the
# centre exclusive: exactly four chunks, including both (78,47) and (77,48).
# Holding the decorating SET fixed and varying only the ORDER is what makes
# the two arms comparable at all.
PREFIX=${PREFIX:-78,48,1}

# `zx` is the order the r=16 references were harvested under
# (`HARVEST-ORDER: rows -> dump_order=zx`) and the one we are charged 312
# blocks on. `default` is the x-major raster, on which the same row is
# charged 0. Vanilla runs (78,47) before (77,48) under `rows` too and is
# still correct, which is the whole puzzle.
ARMS=${ARMS:-"zx default"}

for arm in $ARMS; do
    echo "=== arm $arm ==="
    RUSTMC_WORKERS=1 \
    RUSTMC_DECORATE_PREFIX="$PREFIX" \
    RUSTMC_DECORATE_ORDER="$arm" \
    RUSTMC_TRACE_READS="$BOX" \
    RUSTMC_TRACE_FEATURE=minecraft \
        ./target/release/worldgen-dump full 12345 16 "$OUT/dump-$arm" 64 64 \
        > "$OUT/raw-$arm.txt" 2> "$OUT/err-$arm.txt"
    rc=$?
    # The order line is on stderr and carries the traversal digest. Two arms
    # reporting the same digest means the knob did not turn, and a comparison
    # of one order against itself is not a control (ORACLE rule 30).
    grep -m1 '^decorate-order:' "$OUT/err-$arm.txt" || echo "decorate-order: MISSING"
    if [ "$rc" -ne 0 ]; then
        echo "FAIL arm $arm exited $rc"
        tail -20 "$OUT/err-$arm.txt"
        exit 1
    fi
    grep '^READ ' "$OUT/raw-$arm.txt" > "$OUT/reads-$arm.txt"
    awk '/^FEATWRITE /{split($2,p,","); \
         if (p[1]>=1244 && p[1]<=1255 && p[2]>=0 && p[2]<=40 && p[3]>=760 && p[3]<=775) print}' \
        "$OUT/raw-$arm.txt" > "$OUT/writes-$arm.txt"
    echo "  reads=$(wc -l < "$OUT/reads-$arm.txt")  writes-in-box=$(wc -l < "$OUT/writes-$arm.txt")"
done

# A digest collision across arms means the two runs traversed the same order.
if [ "$(grep -h '^decorate-order:' "$OUT"/err-*.txt | grep -c .)" -ge 2 ]; then
    if [ "$(grep -h '^decorate-order:' "$OUT"/err-*.txt | grep -oE 'digest=[0-9a-fx]+' \
            | sort -u | wc -l)" -lt 2 ]; then
        echo "REFUSED  both arms report the same traversal digest — the order knob did not turn"
        exit 1
    fi
fi

echo
echo "=== first divergence in what the cluster SAW ==="
# `diff` rather than `comm`: order matters here. The reads are emitted in
# execution order, so the first differing line is the first question whose
# answer moved, and everything after it is downstream of that one.
if diff -u "$OUT/reads-zx.txt" "$OUT/reads-default.txt" > "$OUT/reads.diff"; then
    echo "IDENTICAL — the two orders asked the same questions and got the same"
    echo "answers inside the box. Either the box misses the interfering read,"
    echo "or the divergence is not a read in this band."
else
    head -40 "$OUT/reads.diff"
fi
echo
echo "full diff: $OUT/reads.diff"
