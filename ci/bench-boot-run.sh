#!/usr/bin/env bash
# bench-boot-run.sh — the pinned boot/RSS sweep, rustmc against vanilla.
#
#   ci/bench-boot-run.sh <out.jsonl> [runs] [level] [radius]
#
# Every setting this measurement depends on is pinned **here**, in one place,
# so the run is repeatable by somebody who was not there. Changing one of
# them produces a different measurement and it must be said so in the table.
#
# # Why the two flavours alternate instead of running in blocks
#
# This box is shared. Other agents build, gate and boot their own vanilla
# servers on it, and a block design charges whatever they happened to be
# doing entirely to whichever flavour ran during it — which is how you get a
# 2x that is really somebody else's `cargo build`. Alternating rustmc,
# vanilla, rustmc, vanilla spreads any drift across both sides, and the
# 1-minute load average is recorded on both sides of every single run so a
# reader can throw out the contaminated ones rather than take it on trust.
#
# It does not make a busy box into an idle one. It makes the *comparison*
# survive a busy box, which is not the same claim.
#
# # What is pinned
#
#   seed              0 — NOT a free choice. `rustmc-server run` calls
#                     `Tiles::prepare(0, radius)`: its world is always seed 0
#                     and it has no seed flag. Vanilla is pinned to match, so
#                     the terrain comparison is of one world and not two.
#   view-distance     4 on both, as `ci/t2-gameplay.sh` does. Vanilla reads it
#                     from server.properties, rustmc from `--view-distance`;
#                     they were once accidentally different and it moved an
#                     entity-tracking result, so it is set explicitly here.
#   radius            rustmc's spawn tile. 2 is the shipped default
#                     (WORLD_RADIUS) = 25 chunks. 5 = 121 chunks, which is
#                     about what vanilla actually delivers to a joining
#                     client at view-distance 4, and is the matched-work run.
#   vanilla heap      -Xmx1G and **no -Xms**, so the heap grows to what the
#                     workload asks for. `-Xms4G` would make the RSS column
#                     report the flag instead of the need.
#   flat substrate    exactly `ci/t2-gameplay.sh`'s: creative, structures
#                     off, no monsters, peaceful, spawn-radius 0.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT="${1:?usage: bench-boot-run.sh <out.jsonl> [runs] [level] [radius]}"
RUNS="${2:-12}"
LEVEL="${3:-flat}"
RADIUS="${4:-2}"
SEED="${BENCH_BOOT_SEED:-0}"
VD="${BENCH_BOOT_VIEW_DISTANCE:-4}"
HEAP="${BENCH_BOOT_HEAP:-1G}"
IDLE="${BENCH_BOOT_IDLE_SECS:-0}"
CHUNK_PROBE="${BENCH_BOOT_CHUNK_PROBE:-3}"
TAG="${BENCH_BOOT_TAG:-r$RADIUS}"

# The gameplay oracle's substrate, for the superflat runs only. On the noise
# world structures stay on, because that is the world an operator boots.
if [ "$LEVEL" = flat ]; then
    VPROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0'
else
    VPROPS=""
fi

common=(--level "$LEVEL" --seed "$SEED" --view-distance "$VD"
        --idle-secs "$IDLE" --chunk-probe "$CHUNK_PROBE" --tag "$TAG"
        --out "$OUT")
# BENCH_BOOT_COLD=1 drops the page cache before every boot on both sides.
# Needs passwordless root; the record carries `drop_caches_ok` so a run that
# silently stayed warm cannot be reported as cold.
[ "${BENCH_BOOT_COLD:-0}" = 1 ] && common+=(--drop-caches)

echo "== boot sweep: level=$LEVEL seed=$SEED radius=$RADIUS vd=$VD heap=$HEAP runs=$RUNS"
echo "== box: $(nproc) cores, load $(cut -d' ' -f1-3 /proc/loadavg)"
for i in $(seq 1 "$RUNS"); do
    python3 ci/bench-boot.py --flavour rustmc  --runs 1 --radius "$RADIUS" \
        --port 25911 "${common[@]}" > /dev/null
    python3 ci/bench-boot.py --flavour vanilla --runs 1 --heap "$HEAP" \
        --port 25912 --vanilla-props "$VPROPS" "${common[@]}" > /dev/null
    echo "  pair $i/$RUNS done (load $(cut -d' ' -f1 /proc/loadavg))"
done
echo "== raw runs in $OUT"
echo "== aggregate with: ci/bench-boot-report.py $OUT"
