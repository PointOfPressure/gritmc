#!/usr/bin/env bash
# bench-rss-players.sh — what each additional player costs in resident
# memory, and whether any of it comes back when they leave.
#
#   ci/bench-rss-players.sh [outdir]
#
# Two properties of the source predict that this will be worse for rustmc
# than for vanilla, and Rule 9 says a reading of the source moves nothing on
# its own, so both are measured:
#
#   * **the block tables are per connection.** `serve_connection` in
#     `crates/rustmc-server/src/status.rs` calls `ItemIds::load`,
#     `PlacementTable::load` and the shape/use/setPlacedBy loads inside the
#     per-connection thread, not behind a `OnceLock`. The server logs them
#     once per joining client, which is how this was noticed. Ten players
#     should therefore mean ten parsed copies of a 12,716-pair shape table.
#     (That table carries 12,748 pairs since 2026-08-18, when `ShapeGolden`'s
#     discovery widened to all 32,366 states. The figure above is left as the
#     size the RSS measurement was actually taken against, because a bench
#     number silently re-based on a table it never saw is worse than a dated
#     one — and 32 more pairs is far inside this measurement's noise.)
#   * **tiles are never retired.** `crate::tiles` holds `&'static World` and
#     its own comment says the worlds "are leaked on purpose ... nothing
#     retires a tile yet". So whatever a player caused to be generated
#     should still be resident after they disconnect.
#
# The arms are 0, 1 and 5 concurrent clients, each held in the world for 12
# seconds. RSS is read with them connected and again three seconds after the
# last one leaves; the second reading is what tells a per-connection cost
# that is released from one that is not.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
N="${BENCH_PLAYERS_PAIRS:-3}"

for players in 1 5; do
    echo "############ players=$players pairs=$N"
    for i in $(seq 1 "$N"); do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level flat \
            --seed 0 --radius 2 --view-distance 4 --chunk-probe 0 \
            --players "$players" --player-secs 12 --tag "players$players" \
            --port 25911 --out "$W/rss-players.jsonl" > /dev/null
        python3 ci/bench-boot.py --flavour vanilla --runs 1 --level flat \
            --seed 0 --heap 1G --view-distance 4 --chunk-probe 0 \
            --players "$players" --player-secs 12 --tag "players$players" \
            --port 25912 \
            --vanilla-props $'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
            --out "$W/rss-players.jsonl" > /dev/null
        echo "  players=$players pair $i/$N (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
