#!/usr/bin/env bash
# bench-radius-sweep.sh — what a tile radius costs, across the range.
#
#     ci/bench-radius-sweep.sh [outdir]
#
# # Why the range and not two points
#
# `ci/bench-boot-sweep.sh` samples r = 2, 5, 8, 12 at view-distance 4 and
# found the shipped default of 2 losing on both memory and generation
# throughput. Picking the better of two measured points is not choosing a
# default, it is choosing between two arbitrary numbers — so this walks
# 2..12 and reports a curve.
#
# Two things changed under it since that sweep, which is why it has to be
# re-taken rather than read off:
#
#   * `World` no longer re-parses `data/vanilla/block_props.json` per tile.
#     That file is 15.8 MB. The old per-tile fixed cost — the whole reason
#     small tiles looked expensive — was mostly that parse, so the shape of
#     the memory curve is not the shape that sweep measured;
#   * tiles retire, so resident memory is a function of what is in view
#     rather than of everywhere anybody has been.
#
# # The settings, and why each is what it is
#
#   --level normal   the noise world. Superflat's canvases are mostly
#                    untouched zero pages, which never enter RSS, so a
#                    memory curve taken on superflat measures almost nothing
#                    about the canvas. An operator runs terrain.
#   --view-distance  10, the shipped default on both servers. The earlier
#                    sweep used 4, which is `ci/t2-gameplay.sh`'s setting and
#                    not anybody's server.
#   --chunk-probe 6  a client that actually pulls its view, so the tiles a
#                    view distance needs are really forced. `probe_chunks` is
#                    recorded on every run and a run that delivered none has
#                    measured nothing.
#
# The vanilla arm is deliberately absent. `docs/BENCH.md` disqualified its own
# vanilla forceload column under rule 7 — the external idle detector fires in
# the lulls between vanilla's asynchronous chunk generation, and it reported
# 289 and 625 chunks at the same 4.50 s. `ci/bench-servers.sh` remains the
# throughput comparison against vanilla; this script compares rustmc radii
# against each other, which its own internal timer can do honestly.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
N="${BENCH_RADIUS_RUNS:-3}"
RADII="${BENCH_RADII:-2 3 4 5 6 8 10 12}"
VD="${BENCH_RADIUS_VD:-10}"
LEVEL="${BENCH_RADIUS_LEVEL:-normal}"

for r in $RADII; do
    echo "############ radius $r (tile side $((2 * r + 1)), $(((2 * r + 1) ** 2)) chunks)"
    for i in $(seq 1 "$N"); do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level "$LEVEL" \
            --seed 0 --radius "$r" --view-distance "$VD" --chunk-probe 6 \
            --idle-secs 10 --tag "radius-r$r" --port 25915 \
            --out "$W/radius-curve.jsonl" > /dev/null
        echo "  r=$r run $i/$N (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
