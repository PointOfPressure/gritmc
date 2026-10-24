#!/usr/bin/env bash
# bench-rss-player.sh — resident memory once a player has actually loaded a
# view distance, which is not the same number as resident memory at boot.
#
#   ci/bench-rss-player.sh [outdir]
#
# # Why this exists
#
# The boot table reports rustmc at 132 MB resident and vanilla at 832 MB,
# and read on its own that is a 6.3x memory win. It is measured **at the
# login->play transition, before any chunk has been streamed to anybody**.
#
# The 180-second idle runs then showed rustmc at 1,077 MB — an eight-fold
# rise, after one probe client spent three seconds in the world — against
# vanilla's 906 MB. If that reproduces, the memory result reverses: we use
# *more* than vanilla as soon as a single player loads their surroundings,
# and the boot figure is measuring an empty server rather than a cheap one.
#
# It is consistent with something already in this file: at r=32 the
# generator was observed at 14.4 GB, about 3 MB per chunk, because the
# pipeline holds the whole square's canvas and every chunk's retained
# aquifer state. rustmc pre-generates a 25-chunk tile and then has to build
# more tiles to satisfy a view distance of 4 (81 chunks), so a joining
# player triggers generation that boot did not pay for.
#
# # The two arms, which is what makes it an attribution and not an anecdote
#
#   noprobe   --chunk-probe 0: the timed join happens and the client leaves
#             immediately. Whatever chunk streaming the login itself forces
#             is included; nothing else is.
#   probe     --chunk-probe 3: a second client stays for three seconds and
#             pulls its whole view distance.
#
# The difference between the arms is the cost of a player loading their
# surroundings. Both arms idle afterwards so the reading is taken at rest
# and not mid-allocation.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
N="${BENCH_RSS_PAIRS:-8}"

for arm in noprobe probe; do
    probe=0; [ "$arm" = probe ] && probe=3
    echo "############ arm=$arm chunk-probe=$probe pairs=$N"
    for i in $(seq 1 "$N"); do
        python3 ci/bench-boot.py --flavour rustmc --runs 1 --level flat \
            --seed 0 --radius 2 --view-distance 4 --chunk-probe "$probe" \
            --idle-secs 20 --tag "player-$arm" --port 25911 \
            --out "$W/rss-player.jsonl" > /dev/null
        python3 ci/bench-boot.py --flavour vanilla --runs 1 --level flat \
            --seed 0 --heap 1G --view-distance 4 --chunk-probe "$probe" \
            --idle-secs 20 --tag "player-$arm" --port 25912 \
            --vanilla-props $'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
            --out "$W/rss-player.jsonl" > /dev/null
        echo "  $arm pair $i/$N (load $(cut -d' ' -f1 /proc/loadavg))"
    done
done
echo "############ done"
