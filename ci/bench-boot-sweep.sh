#!/usr/bin/env bash
# bench-boot-sweep.sh — the whole boot/RSS measurement, in the order it was
# taken, so the docs/BENCH.md tables can be reproduced by running one thing.
#
#   ci/bench-boot-sweep.sh [outdir]
#
# Sequential on purpose. Two of these running at once measure each other.
#
# The five experiments, and the question each one answers:
#
#   flat-r2      superflat, 12 pairs. Boot with the world cost taken out, so
#                the number is server startup and not terrain. The headline.
#   normal-r2    the noise world, 8 pairs. Boot as an operator experiences
#                it, terrain included.
#   flat-cold    5 pairs with the page cache dropped before every boot.
#                "Cold" in the only sense that can be restored on demand.
#   flat-idle    3 pairs, 180 s of doing nothing after the join. Answers
#                "is the resident set still growing", which one reading
#                cannot.
#   radius       RSS and generation cost against a defined chunk square, at
#                r = 2, 5, 8 and 12 — 25, 121, 289 and 625 chunks. rustmc
#                generates it before it listens (`--radius`); vanilla is
#                driven to it afterwards with `forceload`. Same end state,
#                different route, and the route is the finding.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"

echo "############ normal-r2"
./ci/bench-boot-run.sh "$W/normal-r2.jsonl" 8 normal 2

echo "############ flat-cold"
BENCH_BOOT_COLD=1 BENCH_BOOT_TAG=cold \
    ./ci/bench-boot-run.sh "$W/flat-cold.jsonl" 5 flat 2

echo "############ flat-idle"
BENCH_BOOT_IDLE_SECS=180 BENCH_BOOT_TAG=idle180 \
    ./ci/bench-boot-run.sh "$W/flat-idle.jsonl" 3 flat 2

echo "############ radius sweep"
for r in 2 5 8 12; do
    for i in 1 2 3; do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level normal \
            --seed 0 --radius "$r" --view-distance 4 --forceload-radius "$r" \
            --chunk-probe 3 --tag "sweep-r$r" --port 25911 \
            --out "$W/radius.jsonl" > /dev/null
        python3 ci/bench-boot.py --flavour vanilla --runs 1 --level normal \
            --seed 0 --heap 1G --view-distance 4 --forceload-radius "$r" \
            --forceload-timeout 300 --chunk-probe 3 --tag "sweep-r$r" \
            --port 25912 --out "$W/radius.jsonl" > /dev/null
        echo "  radius $r, pair $i/3 (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
