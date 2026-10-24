#!/usr/bin/env bash
# bench-boot-controls.sh — the controls for the boot table, run after it.
#
#   ci/bench-boot-controls.sh [outdir]
#
# Rule 7 in docs/AGENT-BRIEF.md: when a measurement clears every candidate,
# suspect the measurement. The boot table says rustmc comes up 15x faster
# than vanilla and in a sixth of the memory. Two things could produce that
# number without it being true, and both are cheap to test.
#
#   heap   Vanilla was measured at `-Xmx1G` and sat at 832 MB resident —
#          within 20% of its own ceiling, which is where a JVM starts
#          collecting constantly. If the boot figure is partly GC thrash
#          against a starved heap then it is an artefact of the flag and not
#          a property of the server. Re-run at -Xmx4G: if boot time does not
#          move, the 1G figure was not heap-starved and the table stands.
#
#   reload Every boot in the table generates a fresh world. An operator
#          restarting a server loads an existing one, which is different
#          work, and it is the case a "fast restart" claim would actually be
#          about. Vanilla persists by default; rustmc only persists when
#          given `--world`, so it has to be asked explicitly. --keep-world
#          leaves the world in place between runs, so run 1 generates and
#          runs 2..n reload.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
W="${1:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"

echo "############ heap control: vanilla at -Xmx4G, superflat, n=5"
for i in 1 2 3 4 5; do
    python3 ci/bench-boot.py --flavour vanilla --runs 1 --level flat --seed 0 \
        --heap 4G --view-distance 4 --chunk-probe 3 --tag heap4G --port 25912 \
        --vanilla-props $'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        --out "$W/controls.jsonl" > /dev/null
    echo "  heap4G $i/5 (load $(cut -d' ' -f1 /proc/loadavg))"
done

echo "############ reload control: existing world, superflat, n=6 pairs"
# The first run of each flavour generates; the rest reload. `--keep-world` is
# what makes runs 2..n a reload rather than a regeneration, and the seq=0 run
# of each flavour is therefore NOT comparable with the rest — it is dropped
# by the reporter's own grouping only if tagged apart, so it is tagged apart.
for i in $(seq 1 6); do
    tag=reload; [ "$i" = 1 ] && tag=reload-first
    python3 ci/bench-boot.py --flavour rustmc --runs 1 --level flat --seed 0 \
        --radius 2 --view-distance 4 --keep-world --autosave-seconds 2 --idle-secs 6 --chunk-probe 3 \
        --tag "$tag" --port 25911 --out "$W/controls.jsonl" > /dev/null
    python3 ci/bench-boot.py --flavour vanilla --runs 1 --level flat --seed 0 \
        --heap 1G --view-distance 4 --keep-world --chunk-probe 3 \
        --tag "$tag" --port 25912 \
        --vanilla-props $'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        --out "$W/controls.jsonl" > /dev/null
    echo "  reload pair $i/6 (load $(cut -d' ' -f1 /proc/loadavg))"
done
echo "############ done"
