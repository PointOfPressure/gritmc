#!/usr/bin/env bash
# floor-ensemble.sh — what the noise floor and the residual look like when the
# twin mask is built from an ENSEMBLE of vanilla references instead of a pair.
#
# The problem this exists for is written in `docs/PARITY.md` §6: at r=16 the
# overworld's floor is the same order as the residual, so most of what we
# report is the same size as the ground vanilla is unsure about. A defect of
# ours sitting on one of those positions is invisible, and the window is
# nearly out of resolving power.
#
# The numbers, and they have moved twice — 2026-08-09. When this script was
# written the pair gave a floor of 35,531 against a residual of 43,041 (83%).
# That reference pair was lost in the 2026-08-07 disk failure; on the pair
# regenerated to replace it the floor is 29,560 against a masked residual of
# 29,490, so the floor now EXCEEDS the residual and the ratio argument below
# has stopped being a warning and become the verdict. A floor belongs to the
# pair of runs that produced it — do not carry either set into the other.
#
# A pair of references is a sample of size two of vanilla's own scheduling
# noise. It finds the positions where *those two runs* happened to disagree
# and nothing about the positions where a third run would have disagreed with
# both. Those are the dangerous ones: vanilla is not actually reliable there,
# our value may well be one of vanilla's own answers, and we are charged for
# it anyway.
#
# So, with N references and the first one as the primary:
#
#   floor_N     positions where the N runs are NOT unanimous
#               = union over j of { p : ref1(p) != refj(p) }
#   residual_N  positions where the N runs ARE unanimous and we differ
#               = intersection over j of mismatches(ref1, ours, --twin refj)
#
# `--twin refj` already drops the positions where ref1 and refj disagree, so
# intersecting those sets over every j leaves exactly the positions on which
# every reference agrees with ref1 and we do not. Unanimity with ref1 is
# unanimity, so the pairwise-with-primary form is the whole answer and costs
# N-1 diffs rather than N(N-1)/2.
#
# residual_N is monotonically non-increasing in N and floor_N is monotonically
# non-decreasing, which is the point: every reference added can only move a
# position from "charged to us" to "vanilla is unsure", never back. What the
# run reports is where that stops moving. If the marginal change per added
# reference has decayed to near zero, the remaining residual is standing on
# ground vanilla really does reproduce, and it is a defect count rather than a
# noise measurement.
#
#   ENS_REFS="oracle-work/a oracle-work/b ..."   vanilla FIRST is ref1
#   ENS_OURS=/dev/shm/dump/region                omit to measure the floor only
#   ENS_BBOX="48 48 80 80"
#   ENS_YBOX="0 127"                             mandatory in the nether
#   ENS_OUT=/dev/shm/ens                         position lists land here
#   ENS_PAIRWISE=1                               also do all N(N-1)/2 pairs,
#                                                for the median and the spread
#
# Runs on the Threadripper. Nothing compiles on the laptop.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
export PATH="$HOME/.cargo/bin:$PATH"

REFS="${ENS_REFS:?set ENS_REFS to the reference directories, vanilla first}"
OURS="${ENS_OURS:-}"
BBOX="${ENS_BBOX:-48 48 80 80}"
YBOX="${ENS_YBOX:-}"
OUT="${ENS_OUT:-/dev/shm/ens}"
DIFFER="$ROOT/target/release/region-differ"
[ -x "$DIFFER" ] || { echo "no region-differ at $DIFFER" >&2; exit 2; }

# `--positions` writes its position lines into the SAME stream as the summary,
# the per-family confusion table and the y histogram. A filter loose enough to
# let a summary line through poisons every set operation below, and it does it
# silently: a line that appears in every run survives every intersection, so
# the residual bottoms out at the number of summary lines rather than at the
# number of blocks.
#
# This is not hypothetical. The first run of this script used `awk 'NF>=5'`
# and reported the End's residual falling to **1** at N=6 — a spectacular
# result that was the string `DIFF chunks=1089 (a=1369` intersecting with
# itself five times. The counter said 125 and the filter yielded 165 lines,
# and the discrepancy is the only reason it was caught.
#
# So: anchored, three integers, and the fourth field must be a block id. Same
# shape as `ci/decorate-order-intersect.sh`, which got this right first.
# The `wc -l` of the result is checked against the differ's own
# `mismatches=` counter by the caller below; if they disagree, the filter is
# wrong and nothing downstream means anything.
positions() {
    grep -E '^-?[0-9]+ -?[0-9]+ -?[0-9]+ minecraft:' | awk '{print $1, $2, $3}' | sort -u
}

# shellcheck disable=SC2086
set -- $REFS
N=$#
[ "$N" -ge 2 ] || { echo "need at least two references" >&2; exit 2; }
for r in $REFS; do
    [ -d "$r" ] || { echo "ensemble: no reference at $r — REFUSING." >&2; exit 2; }
done
# A reference that is present but empty reads as "perfect agreement" and is the
# failure mode ci/decorate-order-intersect.sh's header warns about. Coverage
# first, always.
WANT=""
for r in $REFS; do
    # shellcheck disable=SC2086
    got=$("$DIFFER" scan "$r" --bbox $BBOX 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
    [ -z "$WANT" ] && WANT="$got"
    echo "  coverage $r: full=$got"
    [ "$got" = "$WANT" ] && [ "$got" != "0" ] || {
        echo "ensemble: $r has full=$got, expected $WANT — REFUSING to report a" >&2
        echo "          floor from references that do not cover the same square." >&2
        exit 2
    }
done

rm -rf "$OUT"; mkdir -p "$OUT"
# shellcheck disable=SC2086
YB=""; [ -n "$YBOX" ] && YB="--ybox $YBOX"

REF1="$1"; shift
echo "== ensemble of $N references, primary $REF1"

# --- floor_N: the union of every disagreement with the primary --------------
: > "$OUT/floor-union.txt"

# Run one diff, filter it, and REFUSE if the filtered line count disagrees with
# the differ's own counter. That equality is the whole guarantee that the set
# operations below are operating on blocks.
diff_positions() {   # diff_positions <a> <b> <extra-args...> -> stdout is a path
    local a="$1" b="$2"; shift 2
    # shellcheck disable=SC2086
    "$DIFFER" diff "$a" "$b" --bbox $BBOX $YB "$@" --positions \
        > "$OUT/raw.txt" 2> "$OUT/raw.err" || {
        echo "ensemble: region-differ failed on $a vs $b" >&2
        cat "$OUT/raw.err" >&2
        exit 2
    }
    local want got
    want=$(grep -oP 'mismatches=\K[0-9]+' "$OUT/raw.txt" | head -1)
    positions < "$OUT/raw.txt" > "$OUT/pos.txt"
    got=$(wc -l < "$OUT/pos.txt")
    [ -n "$want" ] || {
        echo "ensemble: no DIFF line from $a vs $b — refusing to read silence" >&2
        echo "          as a zero." >&2
        exit 2
    }
    [ "$want" = "$got" ] || {
        echo "ensemble: $a vs $b — the differ counted $want mismatches but the" >&2
        echo "          position filter yielded $got lines. The filter is wrong;" >&2
        echo "          every set operation downstream would be meaningless." >&2
        exit 2
    }
}

j=1
for r in "$@"; do
    j=$((j + 1))
    diff_positions "$REF1" "$r"
    cp "$OUT/pos.txt" "$OUT/floor-1-$j.txt"
    printf '  floor(ref1, ref%d) = %s\n' "$j" "$(wc -l < "$OUT/floor-1-$j.txt")"
    cat "$OUT/floor-1-$j.txt" >> "$OUT/floor-union.txt"
done
sort -u "$OUT/floor-union.txt" -o "$OUT/floor-union.txt"
FLOOR_N=$(wc -l < "$OUT/floor-union.txt")
echo "  FLOOR_N (not unanimous across all $N) = $FLOOR_N"

# --- how the floor grows as references are added ----------------------------
echo "-- floor as a function of N (cumulative union) --"
: > "$OUT/cum.txt"
j=1
for r in "$@"; do
    j=$((j + 1))
    cat "$OUT/floor-1-$j.txt" >> "$OUT/cum.txt"
    sort -u "$OUT/cum.txt" -o "$OUT/cum.txt"
    printf '   N=%d  floor=%s\n' "$j" "$(wc -l < "$OUT/cum.txt")"
done

# --- residual_N: unanimous ground on which we still differ ------------------
if [ -n "$OURS" ]; then
    [ -d "$OURS" ] || { echo "ensemble: no dump at $OURS" >&2; exit 2; }
    echo "-- residual as a function of N (vanilla first, intersected masks) --"
    PREV=""
    j=1
    for r in "$@"; do
        j=$((j + 1))
        diff_positions "$REF1" "$OURS" --twin "$r"
        cp "$OUT/pos.txt" "$OUT/res-$j.txt"
        if [ -z "$PREV" ]; then
            cp "$OUT/res-$j.txt" "$OUT/res-cum.txt"
        else
            comm -12 "$OUT/res-cum.txt" "$OUT/res-$j.txt" > "$OUT/res-tmp.txt"
            mv "$OUT/res-tmp.txt" "$OUT/res-cum.txt"
        fi
        PREV=1
        printf '   N=%d  residual=%s   (this twin alone: %s)\n' \
            "$j" "$(wc -l < "$OUT/res-cum.txt")" "$(wc -l < "$OUT/res-$j.txt")"
    done
    RES_N=$(wc -l < "$OUT/res-cum.txt")
    echo "  RESIDUAL_N = $RES_N   FLOOR_N = $FLOOR_N"
    if [ "$RES_N" -gt 0 ]; then
        awk -v r="$RES_N" -v f="$FLOOR_N" \
            'BEGIN { printf "  floor/residual = %.3f  %s\n", f / r,
                     (f < r ? "(floor below residual: QUOTABLE)" \
                            : "(floor at or above residual: CORROBORATION ONLY)") }'
    fi
fi

# --- every pair, for the median and the spread ------------------------------
if [ -n "${ENS_PAIRWISE:-}" ]; then
    echo "-- all pairwise floors --"
    set -- $REFS
    ALL="$*"
    i=0
    for a in $ALL; do
        i=$((i + 1)); k=0
        for b in $ALL; do
            k=$((k + 1))
            [ "$k" -le "$i" ] && continue
            # shellcheck disable=SC2086
            m=$("$DIFFER" diff "$a" "$b" --bbox $BBOX $YB 2>/dev/null \
                | grep -oP 'mismatches=\K[0-9]+' || echo "?")
            printf '   %s vs %s: %s\n' "$(basename "$a")" "$(basename "$b")" "$m"
        done
    done | tee "$OUT/pairwise.txt"
    awk '{print $NF}' "$OUT/pairwise.txt" | grep -E '^[0-9]+$' | sort -n > "$OUT/pw.txt"
    awk '{a[NR]=$1} END {
        if (NR == 0) exit
        printf "   pairs=%d  min=%d  median=%d  max=%d  spread=%.2fx\n",
               NR, a[1], (NR % 2 ? a[(NR+1)/2] : int((a[NR/2]+a[NR/2+1])/2)),
               a[NR], (a[1] > 0 ? a[NR]/a[1] : 0)
    }' "$OUT/pw.txt"
fi
