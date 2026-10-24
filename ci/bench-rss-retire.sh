#!/usr/bin/env bash
# bench-rss-retire.sh — does resident memory come BACK when nobody is looking?
#
#   ci/bench-rss-retire.sh [outdir]
#
# # Why this is a different measurement from every other one in this family
#
# `ci/bench-rss-player.sh` and `ci/bench-rss-viewdistance.sh` ask what a
# player costs. They read RSS with a client in the world, and their idle
# window is 15-20 seconds. That was long enough when the answer could only
# ever be "the same or more": `crate::tiles` retired nothing, so the reading
# after a player left was the reading while they were there, to the megabyte.
#
# Tiles retire now, after `tiles::RETIRE_AFTER_SECS` of nobody being able to
# see them. **A 15-second idle window cannot observe a 30-second grace
# period** — it would report "no change" for a server that returns all of it
# five seconds later, which is a false negative of exactly the shape
# `docs/AGENT-BRIEF.md` warns about. So this script idles for 90 seconds and
# reads both ends:
#
#   rss_idle_start_kb   ~3s after the probe client disconnected: the tiles it
#                       caused are still resident and still inside their
#                       grace period.
#   rss_idle_end_kb     90s later: past the grace period, past several
#                       sweeps. If retirement works this is much lower; if it
#                       does not, it is the same number.
#
# **The `_start` reading is the control and it is not optional.** Without it a
# low `_end` figure cannot be told apart from a server that never allocated
# anything in the first place — which is precisely how the reload control in
# `docs/BENCH.md` nearly published "restarts 14x faster" for a world that was
# never written. `probe_chunks` is asserted for the same reason: a client that
# received no chunks forced no tiles, and a server that generated nothing has
# nothing to give back.
#
# Both view distances, because the shipped one is 10 and every other table in
# this family pins 4.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
N="${BENCH_RSS_RUNS:-3}"
IDLE="${BENCH_RETIRE_IDLE:-90}"

for vd in 4 10; do
    echo "############ retire sweep, view-distance $vd, runs=$N, idle=${IDLE}s"
    for i in $(seq 1 "$N"); do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level flat \
            --seed 0 --radius "${BENCH_RADIUS:-2}" --view-distance "$vd" \
            --chunk-probe 6 --idle-secs "$IDLE" --tag "retire-vd$vd" \
            --port 25913 --out "$W/rss-retire.jsonl" > /dev/null
        echo "  vd$vd run $i/$N (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
