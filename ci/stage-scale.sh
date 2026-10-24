#!/usr/bin/env bash
# stage-scale.sh — the per-stage parallel-scaling curve.
#
# `worldgen-dump scale` gives the NOISE stage a speedup curve. Surface,
# biomes, carvers and decoration have never had one, and after the 2026-07-29
# retraction (the tax is not cross-CCX; see BENCH.md) they are the only place
# the +28.2% CPU-second tax can be hiding. This runs the WHOLE pipeline at a
# series of worker counts on a fixed square and reports, per stage, wall time,
# CPU time, mean cores busy, speedup against one worker, and the CPU-second
# tax that width costs.
#
# **CPU seconds, not wall.** A stage that goes 2x faster on 3 workers while
# burning 3.3x the CPU has not got cheaper; it has bought wall with silicon.
# Wall alone hides that entirely, and it is what let a wrong conclusion get
# published here twice in one day.
#
# **This runs on the Threadripper.** Nothing compiles on the laptop.
#
#   RUSTMC_REMOTE_DIR=rustmc-i2s ci/t0-remote.sh   # ships the tree
#   ssh threadripper 'cd rustmc-i2s && ci/stage-scale.sh'
#
# Knobs:
#   SCALE_SEED=20260722      world seed
#   SCALE_RADIUS=16          chunk radius; 16 is 1,089 chunks, the square
#                            every `perf stat` row in BENCH.md was taken on
#   SCALE_CENTRE="64 64"     chunk centre
#   SCALE_WIDTHS="1 2 3 6 12"  worker counts, in order; the first is the
#                            baseline every speedup is against
#   SCALE_OUT=<dir>          where the region files go (deleted between runs)
#   SCALE_LOGS=<dir>         where the raw per-width logs are kept
#
# The instrument is `RUSTMC_STAGE_CPU=1`, NOT `RUSTMC_STAGE_TIMES`. The latter
# also switches on the surface stage's `Cell` probes, which force that stage
# down the sequential path — so it can never answer how the stage scales.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2

SEED="${SCALE_SEED:-20260722}"
RADIUS="${SCALE_RADIUS:-16}"
CENTRE="${SCALE_CENTRE:-64 64}"
WIDTHS="${SCALE_WIDTHS:-1 2 3 6 12}"
OUT="${SCALE_OUT:-$ROOT/target/stage-scale-world}"
LOGS="${SCALE_LOGS:-$ROOT/target/stage-scale-logs}"

export PATH="$HOME/.cargo/bin:$PATH"
mkdir -p "$LOGS" || exit 2

echo "building worldgen-dump (release)" >&2
cargo build --release -p worldgen-dump >/dev/null 2>&1 || {
    echo "stage-scale: build failed; re-run without the redirect to see why" >&2
    exit 2
}

for width in $WIDTHS; do
    rm -rf "$OUT"
    echo "== $width worker(s) ==" >&2
    RUSTMC_STAGE_CPU=1 RUSTMC_WORKERS="$width" \
        ./target/release/worldgen-dump full "$SEED" "$RADIUS" "$OUT" $CENTRE \
        >"$LOGS/w$width.out" 2>"$LOGS/w$width.err"
    status=$?
    if [ "$status" -ne 0 ]; then
        echo "stage-scale: width $width exited $status; see $LOGS/w$width.err" >&2
        exit 1
    fi
    grep '^stage ' "$LOGS/w$width.err" >&2
done
rm -rf "$OUT"

python3 - "$LOGS" $WIDTHS <<'PY'
import re, sys
logs, widths = sys.argv[1], [int(w) for w in sys.argv[2:]]
# `stage <name> <wall> <share>% <ms/chunk> <cpu> <cores>` — the header row
# and the TOTAL row both start with `stage ` too, so match on the shape.
row = re.compile(r'^stage (\S+)\s+([\d.]+)\s+([\d.]+)%\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)')
data, order = {}, []
for w in widths:
    with open(f'{logs}/w{w}.err') as fh:
        for line in fh:
            m = row.match(line)
            if not m:
                continue
            name, wall, _share, _per, cpu, _cores = m.groups()
            if name not in data:
                data[name] = {}
                order.append(name)
            data[name][w] = (float(wall), float(cpu))

base = widths[0]
print()
print(f'per-stage scaling, {len(widths)} widths, baseline {base} worker(s)')
print('CPU seconds are the thread group\'s: every worker, including exited ones.')
for name in order:
    per = data[name]
    if base not in per:
        continue
    bw, bc = per[base]
    print()
    print(f'  {name}')
    print('    workers    wall s     cpu s    cores   speedup   eff     cpu vs base')
    for w in widths:
        if w not in per:
            continue
        wall, cpu = per[w]
        speed = bw / wall if wall else 0.0
        eff = 100.0 * speed / (w / base) if w else 0.0
        tax = 100.0 * (cpu / bc - 1.0) if bc else 0.0
        cores = cpu / wall if wall else 0.0
        print(f'    {w:>7}  {wall:8.2f}  {cpu:8.2f}  {cores:7.2f}  {speed:7.2f}x  {eff:4.0f}%  {tax:+10.1f}%')
PY
