#!/usr/bin/env bash
# bench-rss-viewdistance.sh — what one player costs at the SHIPPED view
# distance, not at the oracle's.
#
#   ci/bench-rss-viewdistance.sh [outdir]
#
# Every other table in this section uses view-distance 4, because that is
# what `ci/t2-gameplay.sh` pins and it makes the two servers comparable. It
# is not what either server ships: `status::DEFAULT_VIEW_DISTANCE` is **10**
# and so is vanilla's `server.properties` default.
#
# That matters more here than anywhere else, because rustmc's memory is a
# function of how much world a player has caused to exist:
#
#   * a tile is `(2*radius+1)^2` chunks and the shipped radius is 2, so 25;
#   * tiles are generated when a player's view reaches them, and at
#     view-distance 4 one joining player was observed pulling in 8 tiles
#     beyond the spawn tile — 225 chunks for one stationary client;
#   * **tiles are never freed.** `crate::tiles` keeps them as `&'static
#     World` in a map and says so: "The `World`s are leaked on purpose ...
#     nothing retires a tile yet."
#
# At view-distance 10 a player's view is 441 chunks rather than 81, so the
# tile count should rise steeply. Whether it rises as the naive area ratio
# suggests is exactly the sort of thing that should be measured rather than
# extrapolated, which is why this script exists instead of a multiplication
# in the docs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
N="${BENCH_VD_PAIRS:-4}"

for vd in 4 10; do
    echo "############ view-distance $vd, pairs=$N"
    for i in $(seq 1 "$N"); do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level flat \
            --seed 0 --radius 2 --view-distance "$vd" --chunk-probe 6 \
            --idle-secs 15 --tag "vd$vd" --port 25911 \
            --out "$W/rss-vd.jsonl" > /dev/null
        python3 ci/bench-boot.py --flavour vanilla --runs 1 --level flat \
            --seed 0 --heap 1G --view-distance "$vd" --chunk-probe 6 \
            --idle-secs 15 --tag "vd$vd" --port 25912 \
            --vanilla-props $'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
            --out "$W/rss-vd.jsonl" > /dev/null
        echo "  vd$vd pair $i/$N (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
