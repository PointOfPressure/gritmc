#!/usr/bin/env bash
# ow-biome-scout.sh — find overworld windows for biomes no published figure
# has ever been measured in.
#
# docs/PARITY.md publishes 41 overworld figures and every one of them is
# centre chunk (64,64), r=16, on three seeds. Twelve of the overworld's 55
# biomes appear in those windows; 43 do not, including `plains`, `forest`,
# `desert`, `jungle` and `swamp`. A window is chosen by centre and radius, so
# a biome that no centre happens to land on is not measured badly — it is not
# measured at all, and nothing in the figure ledger says so.
#
# THE SCOUT IS FREE AND NEEDS NO VANILLA. `RUSTMC_THROUGH=biomes` stops the
# dump after the biome stage — about one second for an r=4 square — and
# `region-differ diff X X --by-biome` on the result is zero mismatches and a
# complete biome decomposition of that square. That is the nether lane's
# self-diff trick (docs/cold-start/nether-biome-census.md) applied to window
# FINDING rather than to window reporting, and it is why this file harvests
# nothing: the corpus lock is only needed once the windows are chosen.
#
# Usage: ow-biome-scout.sh <seed> <step-chunks> <half-extent-chunks> [radius]
set -uo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)" || exit 2
SEED=${1:?seed}; STEP=${2:-96}; EXT=${3:-1536}; R=${4:-4}
OUT=${SCOUT_OUT:-$HOME/owcensus-scout}/s$SEED
JOBS=${SCOUT_JOBS:-6}
D=./target/release/worldgen-dump
G=./target/release/region-differ
for b in "$D" "$G"; do [ -x "$b" ] || { echo "$b missing. REFUSING." >&2; exit 2; }; done
mkdir -p "$OUT"
one() {
    local cx=$1 cz=$2 tag="c${cx}_${cz}"
    [ -s "$OUT/$tag.census" ] && return 0
    RUSTMC_THROUGH=biomes "$D" full "$SEED" "$R" "$OUT/$tag" "$cx" "$cz" >"$OUT/$tag.log" 2>&1 \
        || { echo "DUMP FAILED $tag" >&2; return 1; }
    "$G" diff "$OUT/$tag/region" "$OUT/$tag/region" --by-biome 2>/dev/null \
        | awk '/^ +[a-z_]+ +[0-9]/ {print $1, $2, $5}' > "$OUT/$tag.census"
    rm -rf "$OUT/$tag" "$OUT/$tag.log"
}
n=0
for ((cx=-EXT; cx<=EXT; cx+=STEP)); do
    for ((cz=-EXT; cz<=EXT; cz+=STEP)); do
        one "$cx" "$cz" &
        n=$((n+1)); [ $((n % JOBS)) -eq 0 ] && wait
    done
done
wait
echo "scouted $n centres for seed $SEED into $OUT"
