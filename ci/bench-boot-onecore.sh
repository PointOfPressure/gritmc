#!/usr/bin/env bash
# bench-boot-onecore.sh — the "anything on one core" gap, closed.
#
#   ci/bench-boot-onecore.sh [pairs] [outdir]
#
# docs/PRODUCT.md's hardware floor is 1 vCPU and nothing in docs/BENCH.md was
# ever measured under one — `ci/bench-boot.py --taskset-cpus` existed as a
# capability from 2026-08-17 but had never been run, because writing it
# needed no CPU and running it does. This is that run: both flavours pinned
# with `taskset -c <cpus>` (default core 0), superflat, radius 2, matching
# ci/bench-boot-run.sh's flat-r2 headline pair so the one-core figure is
# comparable to the existing 24-thread one rather than a new, incomparable
# measurement.
#
# Same load gate as ci/bench-attrib-through.sh and for the same reason: a
# JVM boot pinned to one core is still starved to nothing by a neighbour that
# also lands on that core, and this box's scheduler does not respect the pin
# of a process it never runs. BENCH_BOOT_ONECORE_FAKE_LOAD overrides the
# measured load for --selftest, same contract as the sibling script.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CEILING="${BENCH_BOOT_ONECORE_LOAD_CEILING:-4.0}"
CPUS="${BENCH_BOOT_ONECORE_CPUS:-0}"

current_load() {
    if [ -n "${BENCH_BOOT_ONECORE_FAKE_LOAD:-}" ]; then
        echo "$BENCH_BOOT_ONECORE_FAKE_LOAD"; return
    fi
    awk '{print $1}' /proc/loadavg
}

gate_or_refuse() {
    local load; load="$(current_load)"
    echo "bench-boot-onecore: 1-min load = $load, ceiling = $CEILING"
    if awk -v l="$load" -v c="$CEILING" 'BEGIN{exit !(l+0 <= c+0)}'; then
        echo "bench-boot-onecore: box quiet enough, proceeding"; return 0
    fi
    echo "bench-boot-onecore: REFUSED — load $load exceeds ceiling $CEILING"
    return 3
}

if [ "${1:-}" = "--selftest" ]; then
    gate_or_refuse; rc=$?
    echo "bench-boot-onecore: --selftest gate rc=$rc (no server is ever started by --selftest)"
    exit "$rc"
fi

command -v taskset >/dev/null || { echo "FATAL: taskset not on PATH (util-linux)"; exit 2; }

PAIRS="${1:-5}"
W="${2:-$ROOT/oracle-work/bench-boot}"
mkdir -p "$W"
OUT="$W/onecore-cpu${CPUS}.jsonl"

gate_or_refuse || exit 3

VPROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0'
common=(--level flat --seed 0 --view-distance 4 --chunk-probe 3
        --taskset-cpus "$CPUS" --tag "onecore-cpu$CPUS" --out "$OUT")

echo "== bench-boot-onecore: cpus=$CPUS pairs=$PAIRS box=$(nproc)t"
for i in $(seq 1 "$PAIRS"); do
    gate_or_refuse || exit 3
    python3 ci/bench-boot.py --flavour rustmc  --runs 1 --radius 2 \
        --port 25911 "${common[@]}" > /dev/null
    python3 ci/bench-boot.py --flavour vanilla --runs 1 --heap 1G \
        --port 25912 --vanilla-props "$VPROPS" "${common[@]}" > /dev/null
    echo "  pair $i/$PAIRS done (load $(cut -d' ' -f1 /proc/loadavg))"
done
echo "== raw runs in $OUT"
echo "== aggregate with: ci/bench-boot-report.py $OUT"
