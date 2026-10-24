#!/usr/bin/env bash
# clamp-extent-control.sh — is §37's exact match a property of the SEED or of
# the GENERATED EXTENT?
#
# WHAT IS CONFOUNDED, AND WHY IT CANNOT BE LEFT THAT WAY
#
# `docs/DETERMINISM.md` §37 clamped `-Dmax.bg.threads=1` over a **25-chunk**
# generation at seed **20260722** and got an exact match on all six pairs.
# `ci/clamped-floor-r16.sh` clamped it over a **1,089-chunk** generation at seed
# **12345** and got 6,829..9,738 on all fifteen. Two variables moved between
# those two readings, so neither one can be assigned the difference.
#
# `ci/clamped-floor-ladder.sh` removed the first candidate explanation, which
# was that the clamp holds locally and only fails once you look at enough of the
# world at once. It does not: read through a 9-chunk bbox, the six clamped r=16
# references still disagree. So the failure is present at low density
# everywhere rather than appearing at a threshold, and how much of the world is
# being LOOKED at is not the variable.
#
# What that leaves is the two things the ladder holds fixed by construction:
# the seed, and how much of the world was GENERATED in one harvest. This script
# separates them, by generating at seed 12345 with the extent §37 used.
#
#   ARM A, CLAMPED     4 harvests, seed 12345, r=2, `-Dmax.bg.threads=1`, 6 pairs
#   ARM B, CONTROL     3 harvests, seed 12345, r=2, unclamped,             3 pairs
#
# READ IT AS: arm A zero means §37 reproduces at a second seed and the variable
# is the generated extent — one background worker is enough for 25 chunks and
# not for 1,089. Arm A non-zero means §37's exact match was a property of its
# own window, and the clamp is a floor REDUCTION everywhere rather than a
# determinism guarantee anywhere.
#
# ARM B IS A REFUSAL, NOT A COURTESY. Arm A's interesting answer is a row of
# zeros, and a row of zeros is what a wrong bbox, a short harvest or a differ
# that stopped counting all print. Arm B is the same seed, extent, centre, bbox
# and differ with the clamp taken off, in the same run — it is known from §36
# and §37 that unclamped harvests of one window disagree, so if arm B is quiet
# then the instrument is quiet and arm A means nothing.
#
# NOTHING HERE TOUCHES `oracle-work`. Both arms are written under $DEST, which
# is outside the tree, and no existing reference is regenerated or overwritten.
# Ports are used once, ever, per ci/refs-12345-clamped.sh's rule: 25971..25977
# for the default seed, 25981..25987 for EXTENT_SEED=20260722.
#
# EXTENT_SEED EXISTS BECAUSE THE SECOND SEED IS THE POINT. Run it at
# 20260722 and it stops being a control on §37 and becomes a measurement of
# `ci/t2-regiondiff.sh` — that gate harvests two clamped references of exactly
# this window at gate time and asserts they match at 100.000000%. §39 says that
# is a draw from a discrete distribution, so the gate's green has a failure rate
# that is a property of this window and has never been measured on it. Count the
# distinct worlds here and that rate is known instead of assumed.
#
# usage: ci/clamp-extent-control.sh
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

SEED=${EXTENT_SEED:-12345}
DEST=${EXTENT_DEST:-/home/sayer/grit/clamp-extent}
PFX=gen-$SEED-r2-ow
PORTBASE=${EXTENT_PORTBASE:-25971}
BBOX="62 62 66 66"
G=./target/release/region-differ
mkdir -p "$DEST"
[ -x "$G" ] || { echo "$G missing — cargo build --release -p region-differ" >&2; exit 2; }

# label:port:clamped
SPEC="e1:$((PORTBASE+0)):1 e2:$((PORTBASE+1)):1 e3:$((PORTBASE+2)):1 e4:$((PORTBASE+3)):1 u1:$((PORTBASE+4)):0 u2:$((PORTBASE+5)):0 u3:$((PORTBASE+6)):0"

echo "=== CLAMP EXTENT CONTROL @ $(date -Is) on $(hostname) ==="
echo "    seed $SEED, r=2, centre (64,64), 25 chunks, bbox $BBOX, GEN_ORDER=rows"
for s in $SPEC; do
    lab=${s%%:*}; rest=${s#*:}; port=${rest%%:*}; clamp=${rest#*:}
    out="$DEST/$PFX-$lab"
    if [ -d "$out" ]; then echo "SKIP  $lab exists"; continue; fi
    t0=$(date +%s)
    if [ "$clamp" = 1 ]; then
        GEN_POLL_ITERS=1200 VANILLA_JVM_OPTS=-Dmax.bg.threads=1 GEN_ORDER=rows \
            "$ROOT/ci/gen-vanilla-region.sh" "$SEED" 2 "$out" "$port" 64 64 overworld >/dev/null 2>&1
    else
        GEN_POLL_ITERS=1200 GEN_ORDER=rows \
            "$ROOT/ci/gen-vanilla-region.sh" "$SEED" 2 "$out" "$port" 64 64 overworld >/dev/null 2>&1
    fi
    rc=$?
    bg=$(grep -m1 -oP '^bg_threads=\K.*' "$out/HARVEST-ORDER" 2>/dev/null || echo unrecorded)
    printf '  %-4s clamp=%s rc=%s %ss  bg_threads=%s  %s mca\n' "$lab" "$clamp" "$rc" \
        "$(( $(date +%s) - t0 ))" "$bg" "$(ls "$out"/*.mca 2>/dev/null | wc -l)"
    # ORACLE rule 30: read the knob back off the artefact, never assert it.
    if [ "$clamp" = 1 ] && [ "$bg" != 1 ]; then
        echo "  $lab stamps bg_threads='$bg', not 1 — the clamp did not reach the JVM. REFUSING." >&2
        exit 2
    fi
done

pairs() {  # pairs <labels...>
    local -a L=("$@"); local i j n
    for ((i=0; i<${#L[@]}; i++)); do
        for ((j=i+1; j<${#L[@]}; j++)); do
            # shellcheck disable=SC2086
            n=$("$G" diff "$DEST/$PFX-${L[i]}" "$DEST/$PFX-${L[j]}" --bbox $BBOX 2>/dev/null \
                | grep -m1 -oP 'mismatches=\K[0-9]+')
            printf '  %-4s vs %-4s  %s\n' "${L[i]}" "${L[j]}" "${n:-ERROR}"
        done
    done
}

echo "--- ARM B, CONTROL: 3 UNCLAMPED, 3 pairs (must all be non-zero) ---"
B=$(pairs u1 u2 u3); echo "$B"
BZ=$(echo "$B" | awk '{print $NF}' | grep -c '^0$')
BN=$(echo "$B" | awk '{print $NF}' | grep -c '^[0-9]\+$')
if [ "$BN" != 3 ] || [ "$BZ" != 0 ]; then
    echo "  THE CONTROL ARM IS NOT LIVE: $BN of 3 parsed, $BZ zero. Arm A below would" >&2
    echo "  print zeros for the same reason and would mean nothing. REFUSING." >&2
    exit 2
fi
echo "  CONTROL, N=3, 3 pairs: $(echo "$B" | awk '{print $NF}' | sort -n | head -1)..$(echo "$B" | awk '{print $NF}' | sort -n | tail -1)  (instrument is live)"

echo "--- ARM A, CLAMPED: 4 clamped, 6 pairs ---"
A=$(pairs e1 e2 e3 e4); echo "$A"
AN=$(echo "$A" | awk '{print $NF}' | grep -c '^[0-9]\+$')
[ "$AN" = 6 ] || { echo "  only $AN of 6 clamped pairs parsed — REFUSING" >&2; exit 2; }
ANZ=$(echo "$A" | awk '{print $NF}' | grep -vc '^0$')
echo "  CLAMPED, N=4, 6 pairs: $(echo "$A" | awk '{print $NF}' | sort -n | head -1)..$(echo "$A" | awk '{print $NF}' | sort -n | tail -1)  ($ANZ pairs non-zero)"
echo "=== CLAMP EXTENT CONTROL DONE ==="
