#!/usr/bin/env bash
# clamped-floor-r16.sh — does the `-Dmax.bg.threads=1` clamp hold at r=16?
#
# THE QUESTION
#
# `docs/DETERMINISM.md` §37 established the clamp on the `ci/t2-regiondiff.sh`
# window: r=2, 25 chunks, 884,736 blocks, four clamped runs, six pairs, all
# zero. **That is 25 chunks.** An r=16 window is 1,089 — 43x the chunks, and far
# more of what actually causes the floor, which is two chunks' features writing
# into the same block. A property that holds over 25 chunks and fails over 1,089
# would be an entirely ordinary result, and until this script ran, nobody knew
# which it was.
#
# It matters because **every reference ensemble in `oracle-work/` predates §37**.
# The floors this project publishes — overworld 3,006/4,105, nether 48, End 252,
# and the 2,577..4,835 at N=5 that `docs/PARITY.md` prints beside its one
# quotable worldgen exactness — were all taken over unclamped references. A
# floor measured that way is partly, and possibly mostly, **vanilla disagreeing
# with itself about nothing**.
#
# THE ARMS, on one window: seed 12345, r=16, centre chunk (64,64), 1,089
# chunks, bbox 48 48 80 80, 107,053,056 blocks, `GEN_ORDER=rows` throughout.
#
#   CONTROL   4 unclamped references,  6 pairs   ~/rustmc/oracle-work c d e f
#   CLAMPED   6 clamped references,   15 pairs   $CLAMP_REFS          k1..k6
#
# The clamped arm is 6 rather than 4 because the property under test is a
# SCHEDULING one. k1..k4 were taken back to back on a quiet box; k5 and k6 were
# taken hours later under a load average near 15. Four runs sampled from one
# scheduler state is the weakest evidence a scheduling claim can rest on.
#
# Same seed, same centre, same radius, same bbox, same harvest order, same
# differ, same afternoon. The only difference between the arms is one system
# property, so the difference between the two floors is what that property is
# worth.
#
# THE CONTROL IS A REFUSAL, NOT A COURTESY — and it is this script's
# red-when-broken evidence.
#
# The result this script exists to report is a row of zeros, and a row of zeros
# is exactly what a broken instrument prints. A wrong bbox, a differ that stops
# counting, a reference path that resolves to one directory four times, an
# `--bbox` that misses the generated square — every one of those produces
# `mismatches=0` on all six pairs and reads as a triumph. So the unclamped arm
# runs FIRST and the script REFUSES if its six pairs are not all non-zero. Those
# four references are known to disagree (13,317..17,779, recorded in
# `ci/ow-r16-window-12345.sh` over the full 15-pair set), they are read by the
# same differ through the same bbox in the same run, and if they come back quiet
# then the instrument is what is quiet.
#
# That is the whole of the falsification argument and it needs no injection
# harness: the two arms differ in one flag, and one of them is a known non-zero.
#
# NOTHING HERE WRITES TO `oracle-work`. The unclamped references are read-only
# inputs and are not regenerated, not deleted and not stamped. The clamped ones
# live outside it entirely.
#
#   CLAMP_REFS      clamped ensemble root    (default /home/sayer/grit/clamp-refs)
#   UNCLAMP_REFS    unclamped control root   (default ~/rustmc/oracle-work)
#
# THIS IS A MANUAL INSTRUMENT AND IT IS DELIBERATELY NOT IN `ci/t0.sh`. Both of
# its reference roots hold ~22 MB per reference and neither is in the repository
# — `oracle-work` is gitignored and `$CLAMP_REFS` is outside the tree entirely.
# A check pinned to a path the gate cannot see passes on the box that made the
# fixture and refuses everywhere else, which is a failure this project has
# already paid a full gate cycle for. So this script refuses, by name, with the
# command that provisions what it is missing, and the thing that *is* gated is
# the differ underneath it: `ci/regiondiff-falsify.sh` runs in T0 and injects a
# parser fault, a differ that stops counting and a differ that drops a chunk.
# Every number this script prints rests on that, not on itself.
#
# PROVISION: ./ci/refs-12345-clamped.sh  (the clamped arm, ~90 s per reference)
#
# usage: ci/clamped-floor-r16.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

CLAMP_REFS=${CLAMP_REFS:-/home/sayer/grit/clamp-refs}
UNCLAMP_REFS=${UNCLAMP_REFS:-$HOME/rustmc/oracle-work}
PFX=gen-wide-12345-r16-ow
BBOX="48 48 80 80"
G=./target/release/region-differ
CLAMPED="k1 k2 k3 k4 k5 k6"
CLAMPED_RREV="kr-a kr-b"
CONTROL="c d e f"

[ -x "$G" ] || { echo "$G missing — cargo build --release -p region-differ" >&2; exit 2; }

pairs() {  # pairs <root> <prefix> <labels...>
    local root=$1 pfx=$2; shift 2
    local -a L=("$@")
    local i j n
    for ((i=0; i<${#L[@]}; i++)); do
        for ((j=i+1; j<${#L[@]}; j++)); do
            # shellcheck disable=SC2086
            n=$("$G" diff "$root/$pfx-${L[i]}" "$root/$pfx-${L[j]}" --bbox $BBOX 2>/dev/null \
                | grep -m1 -oP 'mismatches=\K[0-9]+')
            printf '  %-6s vs %-6s  %s\n' "${L[i]}" "${L[j]}" "${n:-ERROR}"
        done
    done
}

echo "=== CLAMPED FLOOR AT r=16 @ $(date -Is) on $(hostname) ==="
echo "    window: seed 12345, r=16, centre (64,64), 1,089 chunks, bbox $BBOX,"
echo "            107,053,056 blocks, GEN_ORDER=rows on both arms"

# ---------------------------------------------------------------------------
# Coverage and stamps. A reference that is present but short reads as agreement
# and is indistinguishable from a good one downstream.
# ---------------------------------------------------------------------------
echo "--- coverage and stamps ---"
WANT=""
for r in $CONTROL; do
    d="$UNCLAMP_REFS/$PFX-$r"
    [ -d "$d" ] || { echo "  no control reference at $d — REFUSING" >&2; exit 2; }
    # shellcheck disable=SC2086
    got=$("$G" scan "$d" --bbox $BBOX 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
    [ -z "$WANT" ] && WANT="$got"
    bg=$(grep -m1 -oP '^bg_threads=\K.*' "$d/HARVEST-ORDER" 2>/dev/null || echo "unrecorded")
    printf '  control %-6s full=%-6s %s bg_threads=%s\n' "$r" "$got" \
        "$(grep -m1 '^gen_order=' "$d/HARVEST-ORDER" 2>/dev/null || echo gen_order=UNSTAMPED)" "$bg"
    [ "$got" = "$WANT" ] && [ "$got" != 0 ] || { echo "  $r covers $got, expected $WANT — REFUSING" >&2; exit 2; }
done
for r in $CLAMPED $CLAMPED_RREV; do
    d="$CLAMP_REFS/$PFX-$r"
    [ -d "$d" ] || {
        echo "  no clamped reference at $d — REFUSING" >&2
        echo "  PROVISION IT: ./ci/refs-12345-clamped.sh  (~90 s per reference, eight" >&2
        echo "  references, writes only to \$REFS_DEST which defaults to $CLAMP_REFS)." >&2
        exit 2; }
    # shellcheck disable=SC2086
    got=$("$G" scan "$d" --bbox $BBOX 2>/dev/null | grep -oP 'full=\K[0-9]+' || echo 0)
    bg=$(grep -m1 -oP '^bg_threads=\K.*' "$d/HARVEST-ORDER" 2>/dev/null || echo missing)
    printf '  clamped %-6s full=%-6s %s bg_threads=%s\n' "$r" "$got" \
        "$(grep -m1 '^gen_order=' "$d/HARVEST-ORDER" 2>/dev/null || echo gen_order=UNSTAMPED)" "$bg"
    [ "$got" = "$WANT" ] || { echo "  $r covers $got, expected $WANT — REFUSING" >&2; exit 2; }
    # ORACLE rule 30: an unrecognised knob is not a turned one. The clamp is
    # read back off the artefact, never asserted by this script.
    [ "$bg" = 1 ] || { echo "  $r stamps bg_threads='$bg', not 1 — REFUSING (rule 30)" >&2; exit 2; }
done

# ---------------------------------------------------------------------------
# CONTROL FIRST. See the header: a row of zeros is what this script is looking
# for and also what a broken instrument prints.
# ---------------------------------------------------------------------------
echo "--- ARM 1, CONTROL: 4 UNCLAMPED references, 6 pairs (must all be non-zero) ---"
CTL=$(pairs "$UNCLAMP_REFS" "$PFX" $CONTROL)
echo "$CTL"
CTL_N=$(echo "$CTL" | awk '{print $NF}' | grep -c '^[0-9]\+$')
CTL_ZERO=$(echo "$CTL" | awk '{print $NF}' | grep -c '^0$')
if [ "$CTL_N" != 6 ] || [ "$CTL_ZERO" != 0 ]; then
    echo "  THE CONTROL ARM IS NOT LIVE: $CTL_N of 6 pairs parsed, $CTL_ZERO of them zero." >&2
    echo "  These four references are known to disagree by 13,317..17,779 blocks on" >&2
    echo "  this window. If they read zero here, the instrument is what is quiet, and" >&2
    echo "  the clamped arm below would print zeros for the same reason and mean" >&2
    echo "  nothing at all. REFUSING." >&2
    exit 2
fi
CTL_MIN=$(echo "$CTL" | awk '{print $NF}' | sort -n | head -1)
CTL_MAX=$(echo "$CTL" | awk '{print $NF}' | sort -n | tail -1)
echo "  CONTROL FLOOR, N=4, n=6 pairs: $CTL_MIN..$CTL_MAX  (instrument is live)"

CLP_N_REFS=$(echo $CLAMPED | wc -w)
CLP_N_PAIRS=$(( CLP_N_REFS * (CLP_N_REFS - 1) / 2 ))
echo "--- ARM 2, CLAMPED: $CLP_N_REFS clamped references, $CLP_N_PAIRS pairs ---"
CLP=$(pairs "$CLAMP_REFS" "$PFX" $CLAMPED)
echo "$CLP"
# A pair that failed to parse prints ERROR, and `sort -n` would sort it to the
# front and let it read as a small floor. Count what parsed before quoting a
# range over it — the arm this script exists to report is the one whose
# expected answer is zero, so a non-numeric row must never be silently minned.
CLP_OK=$(echo "$CLP" | awk '{print $NF}' | grep -c '^[0-9]\+$')
[ "$CLP_OK" = "$CLP_N_PAIRS" ] || {
    echo "  only $CLP_OK of $CLP_N_PAIRS clamped pairs parsed — REFUSING" >&2; exit 2; }
CLP_MIN=$(echo "$CLP" | awk '{print $NF}' | sort -n | head -1)
CLP_MAX=$(echo "$CLP" | awk '{print $NF}' | sort -n | tail -1)
CLP_NONZERO=$(echo "$CLP" | awk '{print $NF}' | grep -vc '^0$')
echo "  CLAMPED FLOOR, N=$CLP_N_REFS, n=$CLP_N_PAIRS pairs: $CLP_MIN..$CLP_MAX  ($CLP_NONZERO pairs non-zero)"

echo "--- ARM 3, CLAMPED rows_rev: the clamp under the other request order ---"
pairs "$CLAMP_REFS" "$PFX" $CLAMPED_RREV

echo "--- ARM 4, cross-order (clamped rows vs clamped rows_rev) ---"
echo "  A zero here would mean the request order does not reach the world, which"
echo "  would contradict §11 and would be a finding about the harness, not the clamp."
for a in k1 k2; do
    for b in $CLAMPED_RREV; do
        # shellcheck disable=SC2086
        n=$("$G" diff "$CLAMP_REFS/$PFX-$a" "$CLAMP_REFS/$PFX-$b" --bbox $BBOX 2>/dev/null \
            | grep -m1 -oP 'mismatches=\K[0-9]+')
        printf '  %-6s vs %-6s  %s\n' "$a" "$b" "${n:-ERROR}"
    done
done

echo "--- ARM 5, cross-clamp (clamped rows vs UNCLAMPED rows) ---"
echo "  Is the clamped world a member of the population the unclamped runs draw"
echo "  from, or is it somewhere else entirely? Vanilla first (the unclamped one)."
for a in $CONTROL; do
    for b in k1; do
        # shellcheck disable=SC2086
        n=$("$G" diff "$UNCLAMP_REFS/$PFX-$a" "$CLAMP_REFS/$PFX-$b" --bbox $BBOX 2>/dev/null \
            | grep -m1 -oP 'mismatches=\K[0-9]+')
        printf '  unclamped %-4s vs clamped %-4s  %s\n' "$a" "$b" "${n:-ERROR}"
    done
done
echo "=== CLAMPED FLOOR AT r=16 DONE ==="
