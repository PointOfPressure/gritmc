#!/usr/bin/env bash
# clamped-floor-ladder.sh — where does the `-Dmax.bg.threads=1` clamp stop holding?
#
# THE QUESTION THIS EXISTS TO ANSWER, AND THE CONFOUND IT REMOVES
#
# `docs/DETERMINISM.md` §37 measured the clamp at r=2 on seed **20260722** and
# got an exact match on every pair. `ci/clamped-floor-r16.sh` measured it at
# r=16 on seed **12345** and got 6,829..9,738 on all fifteen. Two things differ
# between those runs, not one: the radius and the seed. On its own that pair of
# readings cannot say which of them the clamp is sensitive to, and "the clamp
# does not hold at scale" would be an unearned reading of it.
#
# This script removes the seed. It re-reads the SAME six clamped r=16
# references through progressively narrower bounding boxes, all concentric on
# the same centre. Nothing is regenerated and nothing is re-harvested: every
# rung is the same pair of fully-decorated worlds, restricted. So the seed, the
# clamp, the harvest order, the generation extent and the decoration margin are
# all held fixed **by construction**, and the only thing that varies across the
# ladder is how much of the world is being looked at.
#
# That also disposes of the margin question ORACLE rule 14 would otherwise
# raise. A narrow rung is not a narrow *generation* — features from outside it
# have already been written into it by the r=16 harvest that produced both
# sides. Restricting a comparison is not the same operation as shrinking a
# window, and only the former is happening here.
#
# THE RUNGS: centre chunk (64,64), r = 1, 2, 4, 8, 12, 16.
#
# THE CONTROL, AND IT IS A REFUSAL. The unclamped arm runs at every rung and
# must be non-zero at every rung. The result this script is looking for at the
# bottom of the ladder is a row of zeros, which is also exactly what a differ
# reading an empty bbox prints. If a rung's unclamped pairs go quiet then that
# rung's bbox is what is quiet, and its clamped zeros mean nothing.
#
# usage: ci/clamped-floor-ladder.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

CLAMP_REFS=${CLAMP_REFS:-/home/sayer/grit/clamp-refs}
UNCLAMP_REFS=${UNCLAMP_REFS:-$HOME/rustmc/oracle-work}
PFX=gen-wide-12345-r16-ow
G=./target/release/region-differ
CLAMPED="k1 k2 k3 k4 k5 k6"
CONTROL="c d e f"
RUNGS="1 2 4 8 12 16"

[ -x "$G" ] || { echo "$G missing — cargo build --release -p region-differ" >&2; exit 2; }

span() {  # span <root> <bbox> <labels...> -> "min max nonzero npairs"
    local root=$1 bb=$2; shift 2
    local -a L=("$@") V=()
    local i j n
    for ((i=0; i<${#L[@]}; i++)); do
        for ((j=i+1; j<${#L[@]}; j++)); do
            # shellcheck disable=SC2086
            n=$("$G" diff "$root/$PFX-${L[i]}" "$root/$PFX-${L[j]}" --bbox $bb 2>/dev/null \
                | grep -m1 -oP 'mismatches=\K[0-9]+')
            V+=("${n:-ERROR}")
        done
    done
    local ok; ok=$(printf '%s\n' "${V[@]}" | grep -c '^[0-9]\+$')
    [ "$ok" = "${#V[@]}" ] || { echo "PARSE-FAIL $ok/${#V[@]}"; return; }
    printf '%s %s %s %s\n' \
        "$(printf '%s\n' "${V[@]}" | sort -n | head -1)" \
        "$(printf '%s\n' "${V[@]}" | sort -n | tail -1)" \
        "$(printf '%s\n' "${V[@]}" | grep -vc '^0$')" "${#V[@]}"
}

echo "=== CLAMPED FLOOR LADDER @ $(date -Is) on $(hostname) ==="
echo "    seed 12345, centre chunk (64,64), overworld, GEN_ORDER=rows on both arms."
echo "    Both arms are r=16 harvests read through a narrower bbox — nothing regenerated."
printf '\n%-4s %-8s %-9s %-24s %-24s\n' "r" "chunks" "blocks" "UNCLAMPED N=4 (6 pairs)" "CLAMPED N=6 (15 pairs)"
for r in $RUNGS; do
    x0=$((64-r)); x1=$((64+r)); bb="$x0 $x0 $x1 $x1"
    ch=$(( (2*r+1) * (2*r+1) )); bl=$(( ch * 98304 ))
    read -r umin umax unz un <<<"$(span "$UNCLAMP_REFS" "$bb" $CONTROL)"
    read -r cmin cmax cnz cn <<<"$(span "$CLAMP_REFS"   "$bb" $CLAMPED)"
    if [ "$umin" = "PARSE-FAIL" ] || [ "$unz" = 0 ] 2>/dev/null; then
        printf '%-4s %-8s %-9s  UNCLAMPED ARM QUIET AT THIS RUNG — bbox %s is what is quiet, REFUSING\n' \
            "$r" "$ch" "$bl" "$bb" >&2
        exit 2
    fi
    printf '%-4s %-8s %-9s %-24s %-24s\n' "$r" "$ch" "$bl" \
        "$umin..$umax ($unz/$un nz)" "$cmin..$cmax ($cnz/$cn nz)"
done
echo
echo "READ IT AS: the rung at which the clamped column stops being 0/15 non-zero"
echo "is the scale at which one serialised background executor stops being enough."
echo "=== CLAMPED FLOOR LADDER DONE ==="
