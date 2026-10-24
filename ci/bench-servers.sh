#!/usr/bin/env bash
# bench-servers.sh — item 2 of the definition of complete: "fastest
# vanilla-parity server", measured against vanilla, Paper and Folia on the
# same machine, in parity mode, with the method written down.
#
#   ci/bench-servers.sh
#
# Everything is env-tunable:
#
#   BENCH_SEED=20260722          world seed, all four servers
#   BENCH_RADIUS=7               chunk radius. r<=7 is issued as one
#                                `forceload add`; past 256 chunks the command
#                                refuses, so bigger squares go row by row
#   BENCH_CENTRES="64,64 ..."    chunk centres, one square each
#   BENCH_WARMUP=1024,1024       a square generated and thrown away first
#   BENCH_WARMUP_RADIUS=$R       size of that square; 7 warms on exactly what
#                                the r=7 table warmed on, at any radius
#   BENCH_PROBE_TIMEOUT=1200     seconds before the probe gives up on a square
#   BENCH_FLAVOURS="vanilla paper folia rustmc"
#   BENCH_HEAP=4G                -Xms/-Xmx for all three JVMs
#
# # What is being timed
#
# **Producing a finished (2r+1)^2 chunk square**, seed and extent identical
# on every server: noise, surfaces, carvers, features, structures. Not the
# noise stage — `worldgen-dump bench` and `ci/bench-vanilla.sh` do that, and
# they are a different measurement with a much narrower denominator.
#
# This is the only workload the four servers currently share. rustmc has no
# entities, no block updates and no random ticks (docs/SERVER.md), so a TPS
# or MSPT comparison would time an empty loop against a full one and would
# flatter us by any factor we cared to pick.
#
# The cost includes each server's own margin: a chunk cannot reach `full`
# until its neighbours have reached the statuses it reads, so all four pay
# for a ring they were not asked for. The denominator is the square that was
# ASKED FOR — (2r+1)^2 — on every side, because that is the work the caller
# receives.
#
# # Why the numbers are comparable
#
# - **Same machine, same session, back to back.** A remembered figure for
#   one side and a measured one for the other are two unrelated facts.
# - **Same JVM flags for all three Java servers** (`-Xms4G -Xmx4G`, nothing
#   else). Paper's recommended G1 flags would make Paper faster and would
#   also stop this being like-for-like; tuning is a separate measurement,
#   and it is Paper's to make rather than ours to guess.
# - **The JVM is warm.** Each server generates its spawn area during boot
#   and then a full throwaway square at `BENCH_WARMUP` before any timer
#   starts. Timing a cold JVM makes Java look about twice as slow — the
#   same trap `ci/bench-vanilla.sh` documents — and being wrong in our own
#   favour is worse than being slow.
# - **No console commands inside the timed window, on any server.** This
#   matters more than it sounds: **Folia implements neither `/tick` nor
#   `/save-all`, and rejects `/gamerule randomTickSpeed`** with "Incorrect
#   argument for command". The first version of this script froze ticks and
#   polled `save-all flush` + `region-differ scan` until the square was on
#   disk; on Folia every one of those commands failed, so what it actually
#   measured was Folia's five-minute autosave interval — 303 s of wall for
#   33 s of CPU. A probe that one participant silently ignores is not a
#   measurement.
#
# # The probe
#
# Completion is detected from **CPU alone**: `/proc/<pid>/stat` (utime +
# stime) is sampled every 0.2 s, and the square is done when the process has
# first gone busy and then stayed under `IDLE_CPU` core-fraction for
# `IDLE_WINDOWS` consecutive samples. Nothing is sent to the server, nothing
# is written to disk, and the same code runs against all three. The
# confirmation window is subtracted from the reported wall time, so the
# number is the time to the last CPU spent, not the time to notice.
#
# Ticks are frozen where the server allows it, which is vanilla and Paper but
# not Folia — Folia has no `/tick` command. With 26.2's real rule ids it does
# take all seven gamerules (it rejected them alongside `/tick` only while
# this script was sending the old camelCase names). So Folia
# alone runs a live server while it generates: its CPU column carries the
# cost of ticking the forceloaded square and its parity row carries whatever
# that ticking mutated. Both are over-estimates against it, neither is
# removable from outside the process, and the alternative — freezing nobody,
# so the method is identical — was measured too and costs about 30 blocks of
# drift per 25 chunks, which is an order above rustmc's own residual and
# would have made the parity column useless.
#
# # CPU seconds as well as wall clock
#
# rustmc's pipeline generates on a worker pool too — `RUSTMC_WORKERS`, see
# `crates/rustmc-worldgen/src/pipeline.rs` — and so do all three Java servers.
# This comment used to say rustmc was single-threaded, which stopped being
# true when parallel decoration landed on 2026-07-29 and was still here on
# 2026-08-03. On a 24-thread box a wall-only table says almost nothing about the
# generator and almost everything about the thread count, so both are
# reported: wall from the clock, CPU from `/proc/<pid>/stat` for the JVMs
# and `/usr/bin/time` for rustmc.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
WORK="$ROOT/oracle-work/bench-servers"
SEED="${BENCH_SEED:-20260722}"
R="${BENCH_RADIUS:-7}"
CENTRES="${BENCH_CENTRES:-64,64 256,-192 -320,128}"
WARMUP="${BENCH_WARMUP:-1024,1024}"
# The warmup square exists to JIT the generator, not to be measured, and it
# does not have to be the size of the timed square. Defaulting it to R keeps
# every previous invocation identical; setting it to 7 at r=32 warms each JVM
# on exactly the square the r=7 table warmed on, which is the more comparable
# thing to do as well as the cheaper one.
WARMUP_R="${BENCH_WARMUP_RADIUS:-${BENCH_RADIUS:-7}}"
FLAVOURS="${BENCH_FLAVOURS:-vanilla paper folia rustmc}"
WANT=$(( (2 * R + 1) * (2 * R + 1) ))
# Same fixed heap on every JVM. Raising it for a bigger radius is fine but it
# makes that run incomparable with the r=7 table, so say so if you do.
HEAP="${BENCH_HEAP:-4G}"
RESULTS="$WORK/results.tsv"

# ------------------------------------------------------- the worker pool ----
#
# THIS SCRIPT PUBLISHES CPU-SECONDS AND UNTIL 2026-08-18 IT DID NOT PIN OR
# RECORD THE POOL WIDTH THAT DECIDES THEM.
#
# `RUSTMC_WORKERS` was named in a comment eighty lines up and never set here,
# so the pool came from `pipeline::worker_threads()`, which falls back to the
# box's physical core count (12 on the 1920X) — *unless the invoking shell had
# exported the variable*, in which case every rustmc row moved and no artefact
# said so. `results.tsv` carries flavour, centre, chunks, wall, CPU, peak cores
# and spawn ms; none of those is the pool width. `peak_cores` is an
# observation and not the setting: a pool of 24 that happened to peak at 11
# and a pool of 12 that peaked at 11 write the same number there.
#
# The width is not a detail of how the run was made, it is a condition of the
# quantity. `pipeline::worker_threads`'s own doc comment prices it on this
# exact box, seed and square — r=16 at (64,64), seed 20260722, 1,089 chunks,
# identical build, `perf stat`:
#
#     1 thread    78.99 s wall   78.54 CPU-s
#    12 (phys)    60.08 s wall   82.69 CPU-s
#    24 (SMT)     59.72 s wall   98.48 CPU-s
#
# 78.54 to 98.48 is a 25.4% span on the published metric with the generator
# unchanged — the same 810 G instructions to within 0.2%, arriving as 383 G
# cycles instead of 325 G. A CPU-seconds figure quoted without its pool width
# is not a weaker claim than one quoted with it; it is a different one, and
# every other worldgen harness in `ci/` already knew that
# (`ci/biome-palette-order.sh` exports it AND prints it in its banner,
# `ci/ow-r16-window.sh` and five others pin it to 1, `ci/grassdirt-retake.sh`
# to 6). This script was the exception and it is the one that publishes a
# performance table.
#
# So: resolve it, EXPORT it so the value is the one the binary uses rather
# than one we hope it uses, and stamp it everywhere the run is read.
#
# The default mirrors `pipeline::physical_cores()` — one entry per distinct
# `thread_siblings_list`, which is the core count and not the thread count —
# so pinning changes no previously published number on this box. It only
# stops the number changing without saying so.
if [ -z "${BENCH_WORKERS:-}" ] && [ -z "${RUSTMC_WORKERS:-}" ]; then
    WORKERS="$(cat /sys/devices/system/cpu/cpu[0-9]*/topology/thread_siblings_list \
                 2>/dev/null | sort -u | wc -l)"
    [ "${WORKERS:-0}" -ge 1 ] 2>/dev/null || WORKERS="$(nproc 2>/dev/null || echo 1)"
    WORKERS_SOURCE="physical cores (sysfs topology), the binary's own default"
else
    WORKERS="${BENCH_WORKERS:-$RUSTMC_WORKERS}"
    WORKERS_SOURCE="pinned by the caller"
fi
export RUSTMC_WORKERS="$WORKERS"

# The JVMs are NOT pinned and this is deliberate rather than an oversight left
# standing. Vanilla sizes its worldgen pool from the box too, and
# `-Dmax.bg.threads` would change what the reference server IS — the launch
# headline of this project is that our parity is measured against an
# UNMODIFIED vanilla, and a JVM property is a modification. The CPU column is
# therefore ours-pinned against theirs-default, which is the honest shape of
# the comparison and is now said out loud in the banner instead of being
# invisible in both directions.
JVM_WORKERS="unpinned (server default; no -Dmax.bg.threads — pinning it would
make the reference a modified vanilla)"

# Idle detection. 0.2 s samples; "busy" is a quarter of one core, "idle" is
# an eighth. A live server with no players and a forceloaded square sits far
# below the idle line and chunk generation on a worker pool sits far above
# the busy one, so the gap is wide; both are printed per run in the trace so
# a future reading can check that it still is.
SAMPLE="${BENCH_SAMPLE:-0.2}"
BUSY_CPU="${BENCH_BUSY_CPU:-0.25}"
IDLE_CPU="${BENCH_IDLE_CPU:-0.125}"
IDLE_WINDOWS="${BENCH_IDLE_WINDOWS:-15}"
# The probe gives up after this many seconds. r=32 is 18.8x r=7's chunks, so
# the default that was generous there is not obviously generous here.
PROBE_TIMEOUT="${BENCH_PROBE_TIMEOUT:-1200}"

# The harvested worlds are cleared, not merged into: a shorter run leaves a
# previous run's worlds behind, and the parity pass at the bottom will then
# happily diff this run's vanilla against last run's Paper and print a number
# that means nothing. Seen once, at r=8 against an r=7 leftover.
rm -rf "$WORK/worlds"
mkdir -p "$WORK/jars" "$WORK/worlds"
: > "$RESULTS"
# The window this run was taken under, machine-readable and beside the numbers,
# so a reading months from now does not have to reconstruct it from a banner in
# a lost terminal. `ci/figures.toml` cites this file.
{
    printf 'radius\t%s\n'          "$R"
    printf 'chunks_per_square\t%s\n' "$WANT"
    printf 'squares\t%s\n'         "$(set -- $CENTRES; echo $#)"
    printf 'centres\t%s\n'         "$CENTRES"
    printf 'seed\t%s\n'            "$SEED"
    printf 'heap\t%s\n'            "$HEAP"
    printf 'rustmc_workers\t%s\n'  "$RUSTMC_WORKERS"
    printf 'rustmc_workers_source\t%s\n' "$WORKERS_SOURCE"
    printf 'jvm_workers\t%s\n'     "$(echo "$JVM_WORKERS" | tr '\n' ' ')"
    printf 'commit\t%s\n'          "$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
    printf 'dirty\t%s\n'           "$(git -C "$ROOT" status --porcelain 2>/dev/null | head -1 | grep -c . >/dev/null && echo yes || echo no)"
    printf 'machine\t%s\n'         "$(uname -n)"
    printf 'taken\t%s\n'           "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
} > "$WORK/window.tsv"

export PATH="$HOME/.cargo/bin:$PATH"
DIFFER="$ROOT/target/release/region-differ"
DUMP="$ROOT/target/release/worldgen-dump"
if command -v cargo >/dev/null 2>&1; then
    (cd "$ROOT" && cargo build -q --release -p region-differ -p worldgen-dump)
fi
[ -x "$DIFFER" ] || { echo "no region-differ at $DIFFER" >&2; exit 2; }

# ---------------------------------------------------------------- jars ----

paper_jar() {           # paper_jar <paper|folia> -> path on stdout
    local proj="$1" jar="$WORK/jars/$1-$VER.jar" meta url sha
    if [ ! -s "$jar" ]; then
        meta="$(curl -fsSL "https://fill.papermc.io/v3/projects/$proj/versions/$VER/builds/latest")"
        url="$(printf '%s' "$meta" | python3 -c 'import sys,json;print(json.load(sys.stdin)["downloads"]["server:default"]["url"])')"
        sha="$(printf '%s' "$meta" | python3 -c 'import sys,json;print(json.load(sys.stdin)["downloads"]["server:default"]["checksums"]["sha256"])')"
        curl -fsSL -o "$jar" "$url" >&2
        echo "$sha  $jar" | sha256sum -c - >/dev/null
        echo "fetched + sha256-verified: $jar" >&2
    fi
    printf '%s\n' "$jar"
}

vanilla_jar() {
    "$ROOT/ci/vanilla-server.sh" fetch >&2
    printf '%s\n' "$ROOT/oracle-work/vanilla-$VER/server.jar"
}

# -------------------------------------------------------------- driver ----

# A write to `console.fifo` can only complete if something has the fifo open
# for reading, and the only reader is ever the server. Against a server that
# has already died, `open(2)` blocks forever and takes this script with it —
# no output, no exit, no timeout, indistinguishable from a benchmark still
# running. That is exactly how `ci/t2-gameplay.sh` lost 67 minutes twice on
# 2026-08-05 through the same three lines in `ci/vanilla-server.sh`, and this
# copy of them has the same defect. Fixed here at the same time because a
# correction that lives only in the file it corrected does not travel, which
# this repo has now paid for three times.
console() {
    local dir="$1" cmd="$2" pid
    pid="$(cat "$dir/server.pid" 2>/dev/null || true)"
    if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
        echo "bench-servers: console — server in $dir is not running; DROPPED: $cmd" >&2
        return 2
    fi
    # And the race: it can die between that check and the open.
    if ! timeout -k 2 "${BENCH_CONSOLE_TIMEOUT:-15}" \
            bash -c 'printf "%s\n" "$2" > "$1/console.fifo"' _ "$dir" "$cmd"; then
        echo "bench-servers: console — write to $dir/console.fifo timed out; DROPPED: $cmd" >&2
        return 3
    fi
}

# The overworld region directory: vanilla nests dimensions under the world,
# Paper and Folia keep Bukkit's one-folder-per-dimension layout. Probe.
region_dir() {
    local d="$1"
    for c in "$d/world/dimensions/minecraft/overworld/region" "$d/world/region"; do
        [ -d "$c" ] && { printf '%s\n' "$c"; return 0; }
    done
    return 1
}

boot() {                # boot <dir> <jar> <port>
    local dir="$1" jar="$2" port="$3"
    rm -rf "$dir"; mkdir -p "$dir"
    dir="$(cd "$dir" && pwd)"
    echo "eula=true" > "$dir/eula.txt"
    cat > "$dir/server.properties" <<P
server-port=$port
online-mode=false
level-seed=$SEED
level-type=minecraft:normal
view-distance=4
simulation-distance=4
spawn-protection=0
enforce-secure-profile=false
sync-chunk-writes=false
max-tick-time=-1
motd=rustmc-bench
P
    mkfifo "$dir/console.fifo" 2>/dev/null || true
    (exec 3<>"$dir/console.fifo"; cd "$dir" && exec java -Xms$HEAP -Xmx$HEAP -jar "$jar" --nogui <&3 >"$dir/server.log" 2>&1) &
    echo $! > "$dir/server.pid"
    local i
    for i in $(seq 1 300); do
        grep -q 'Done (' "$dir/server.log" 2>/dev/null && return 0
        kill -0 "$(cat "$dir/server.pid")" 2>/dev/null || { echo "server died:"; tail -20 "$dir/server.log"; return 1; }
        sleep 1
    done
    echo "timeout waiting for Done"; tail -20 "$dir/server.log"; return 1
}

halt() {
    local dir="$1" pid
    [ -f "$dir/server.pid" ] || return 0
    # Say so rather than sending 'stop' into a fifo nothing is reading. A
    # server that died before it could be halted is a fact worth one line in
    # the bench log; before this it was a place the script stopped for good.
    pid="$(cat "$dir/server.pid" 2>/dev/null || true)"
    if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
        echo "bench-servers: halt — server in $dir was already dead (pid ${pid:-none})" >&2
        rm -f "$dir/server.pid"
        return 0
    fi
    console "$dir" "stop" 2>/dev/null || true
    local i
    for i in $(seq 1 120); do kill -0 "$(cat "$dir/server.pid")" 2>/dev/null || { rm -f "$dir/server.pid"; return 0; }; sleep 1; done
    kill -9 "$(cat "$dir/server.pid")" 2>/dev/null || true; rm -f "$dir/server.pid"
}

# The `forceload add` commands for one square.
#
# `forceload` refuses more than 256 chunks in one command, so anything past
# r=7 has to go row by row. The single-command form is kept for the squares
# that fit, because the r=7 table in docs/BENCH.md was taken with it and a
# driver change is not free: chunks reach FEATURES in whatever order the
# server's scheduler promotes them, features write one chunk out, and
# `ci/gen-vanilla-region.sh` measures 4,885 blocks between two vanilla
# worlds that differ only in extent. Timing is not affected; the parity
# column is, so the two radii's parity rows are not comparable to each other.
forceload_cmds() {      # forceload_cmds <cx> <cz> <radius>
    local cx="$1" cz="$2" r="$3" cz_row
    local b1x=$(( (cx - r) * 16 )) b1z=$(( (cz - r) * 16 ))
    local b2x=$(( (cx + r) * 16 + 15 )) b2z=$(( (cz + r) * 16 + 15 ))
    if [ $(( (2 * r + 1) * (2 * r + 1) )) -le 256 ]; then
        echo "forceload add $b1x $b1z $b2x $b2z"
        return
    fi
    for cz_row in $(seq $((cz - r)) $((cz + r))); do
        echo "forceload add $b1x $((cz_row * 16)) $b2x $((cz_row * 16 + 15))"
    done
}

# forceload one square and wait for the JVM to go quiet again.
# Prints "<wall> <cpu> <peak-cores>", or "-1 -1 -1" if the probe failed.

# Peak cores for a process this script started, sampled the same way
# `ci/bench-probe.py` samples the JVMs: `/proc/<pid>/stat` fields 14 and 15
# (utime, stime) over a wall interval.
#
# This exists because the rustmc row used to print a **hardcoded `1.00`** in
# the peak-cores column — a literal, never a measurement, left over from when
# the pipeline really was single-threaded. The 2026-08-03 r=7 run published it
# next to 17.35 CPU-seconds in 6.1 wall seconds, which is 2.84 cores on
# average and so cannot have peaked at one. A fabricated column is worse than
# an absent one.
watch_cores() {
    local pid="$1" interval="${2:-0.25}"
    python3 - "$pid" "$interval" <<'PYEOF'
import os, sys, time
pid, interval = int(sys.argv[1]), float(sys.argv[2])
TICK = os.sysconf("SC_CLK_TCK")
def cpu():
    with open(f"/proc/{pid}/stat", "rb") as f:
        fields = f.read().rpartition(b")")[2].split()
    return (int(fields[11]) + int(fields[12])) / TICK
peak = 0.0
try:
    prev_c, prev_t = cpu(), time.monotonic()
    while True:
        time.sleep(interval)
        now_c, now_t = cpu(), time.monotonic()
        dt = now_t - prev_t
        if dt > 0:
            peak = max(peak, (now_c - prev_c) / dt)
        prev_c, prev_t = now_c, now_t
except Exception:
    pass
print(f"{peak:.2f}")
PYEOF
}

square() {              # square <dir> <pid> <cx> <cz> [radius]
    local dir="$1" pid="$2" cx="$3" cz="$4" r="${5:-$R}" out
    out="$(forceload_cmds "$cx" "$cz" "$r" | python3 "$ROOT/ci/bench-probe.py" "$pid" \
            "$dir/console.fifo" "$SAMPLE" "$BUSY_CPU" "$IDLE_CPU" "$IDLE_WINDOWS" \
            "$PROBE_TIMEOUT")" \
        || out="-1 -1 -1"
    console "$dir" "forceload remove all" 2>/dev/null || true
    printf '%s\n' "$out"
}

# Wait for the server to finish whatever the last square left it doing, and
# say what that cost.
#
# `forceload remove all` drops the tickets on the square just timed; the
# server then unloads it and writes it to disk. At r=7 that is 225 chunks and
# a `sleep 3` covers it. At r=32 it is 4,225 chunks, and anything still
# running when the next square starts is charged to the next square. So it is
# measured with the same probe, sending nothing, and printed — an unquantified
# `sleep` is exactly the shape of mistake this harness has already made once.
settle() {              # settle <dir> <pid>
    local dir="$1" pid="$2" out
    out="$(: | python3 "$ROOT/ci/bench-probe.py" "$pid" "$dir/console.fifo" \
            "$SAMPLE" "$BUSY_CPU" "$IDLE_CPU" "$IDLE_WINDOWS" "$PROBE_TIMEOUT" settle)" \
        || out="-1 -1 -1"
    printf '%s\n' "$out"
}

run_java() {            # run_java <flavour> <jar> <port>
    local flavour="$1" jar="$2" port="$3"
    local dir="$WORK/$flavour"
    echo "=== $flavour ==="
    boot "$dir" "$jar" "$port" || { echo "$flavour: boot failed, skipping"; return 0; }
    local pid; pid="$(cat "$dir/server.pid")"
    # Each server's own spawn-area timer, before anything is sent to it.
    # Vanilla prints "Time elapsed: N ms", Paper "Prepared spawn area in N ms".
    local spawn_ms
    spawn_ms="$(grep -oP '(Time elapsed: |Prepared spawn area in )\K[0-9]+' "$dir/server.log" | head -1 || true)"
    : "${spawn_ms:=-1}"
    echo "$flavour: booted, first spawn area in ${spawn_ms} ms"

    # Quiet the world down, best effort. Vanilla and Paper take all of it;
    # Folia takes only `weather clear` and rejects the rest, so its CPU
    # column carries the cost of live-ticking the forceloaded square and its
    # parity column carries whatever that ticking mutates. Both are
    # over-estimates against it and neither can be removed from outside.
    # What each server accepted is counted from its own log, below, rather
    # than assumed.
    #
    # The rule ids are 26.2's, which are snake_case and in several cases not
    # renamings of the camelCase ones at all: `doMobSpawning` is `spawn_mobs`,
    # `doWeatherCycle` is `advance_weather`, and `doFireTick` has become the
    # integer `fire_spread_radius_around_player`. Sending the old names gets
    # "Incorrect argument for command" and nothing else — silently, since the
    # console is fire-and-forget. Read out of the constant pool of
    # `net/minecraft/world/level/gamerules/GameRules.class`, not remembered.
    local before after n=0
    before="$(grep -c 'Unknown or incomplete command\|Incorrect argument for command' "$dir/server.log" || true)"
    for rule in "tick freeze" "gamerule random_tick_speed 0" \
                "gamerule fire_spread_radius_around_player 0" "gamerule spawn_mobs false" \
                "gamerule mob_griefing false" "gamerule advance_weather false" \
                "gamerule water_source_conversion false" "gamerule lava_source_conversion false" \
                "weather clear 1000000"; do
        console "$dir" "$rule"; n=$((n + 1))
    done
    sleep 3
    after="$(grep -c 'Unknown or incomplete command\|Incorrect argument for command' "$dir/server.log" || true)"
    echo "$flavour: quiescing — $((after - before)) of $n commands rejected"

    echo "$flavour: warmup square at $WARMUP, r=$WARMUP_R"
    square "$dir" "$pid" "${WARMUP%%,*}" "${WARMUP##*,}" "$WARMUP_R" > "$dir/warmup.txt"
    cat "$dir/warmup.txt"
    echo "$flavour: settle after warmup -> $(settle "$dir" "$pid")"

    local centre wall cpu peak
    for centre in $CENTRES; do
        read -r wall cpu peak < <(square "$dir" "$pid" "${centre%%,*}" "${centre##*,}")
        echo "$flavour $centre -> wall=$wall cpu=$cpu peak_cores=$peak"
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$flavour" "$centre" "$wall" "$cpu" \
            "$peak" "$spawn_ms" >> "$WORK/raw.tsv"
        echo "$flavour: settle after $centre -> $(settle "$dir" "$pid")"
    done
    halt "$dir"

    # Counts come after the stop: nothing is saved during the timed window,
    # by design, so the only honest place to verify the square is complete is
    # once the server has written everything out.
    local rd; rd="$(region_dir "$dir")" || { echo "$flavour: no region dir"; return 0; }
    mkdir -p "$WORK/worlds/$flavour"; cp "$rd"/*.mca "$WORK/worlds/$flavour/" 2>/dev/null || true
    for centre in $CENTRES; do
        local cx="${centre%%,*}" cz="${centre##*,}" got
        got="$("$DIFFER" scan "$rd" --bbox $((cx - R)) $((cz - R)) $((cx + R)) $((cz + R)) 2>/dev/null \
                | grep -oP 'full=\K[0-9]+' || echo 0)"
        awk -v f="$flavour" -v c="$centre" -v g="$got" -F'\t' -v OFS='\t' \
            '$1==f && $2==c {print $1, $2, g, $3, $4, $5, $6}' "$WORK/raw.tsv" >> "$RESULTS"
    done
}

run_rustmc() {
    echo "=== rustmc ==="
    [ -x "$DUMP" ] || { echo "rustmc: no worldgen-dump binary, skipping"; return 0; }
    local centre cx cz out t0 t1 got cpu gen
    for centre in $CENTRES; do
        cx="${centre%%,*}"; cz="${centre##*,}"
        out="$WORK/worlds/rustmc-$cx-$cz"
        rm -rf "$out"
        t0="$(date +%s.%N)"
        # `%M` as well as the two CPU fields: at r=32 this process was
        # observed at 14.4 GB resident, ~3 MB per generated chunk, because
        # the pipeline holds the whole square's canvas *and* every chunk's
        # retained aquifer state until the carver stage. The JVMs did the
        # same square in a 4 GB heap. That is a structural fact about the
        # design and it should be in the table rather than in someone's
        # memory of a `ps` output.
        /usr/bin/time -f '%U %S %M' -o "$WORK/rustmc.time" \
            "$DUMP" full "$SEED" "$R" "$out" "$cx" "$cz" > "$WORK/rustmc-$cx-$cz.log" 2>&1 &
        local tpid=$! dpid="" peak="-1"
        # `/usr/bin/time` forks rather than execs, so the process that does
        # the work is its child.
        for _ in $(seq 1 40); do
            dpid="$(pgrep -P "$tpid" 2>/dev/null | head -1)"; [ -n "$dpid" ] && break; sleep 0.05
        done
        if [ -n "$dpid" ]; then peak="$(watch_cores "$dpid" "$SAMPLE")"; fi
        wait "$tpid" 2>/dev/null || true
        t1="$(date +%s.%N)"
        cpu="$(awk '{print $1+$2}' "$WORK/rustmc.time" 2>/dev/null || echo 0)"
        rss="$(awk '{printf "%.2f", $3/1048576}' "$WORK/rustmc.time" 2>/dev/null || echo -1)"
        # The tool times generation alone; the wall above also carries the
        # region write, which the JVMs defer to shutdown and so do not pay
        # inside their window.
        gen="$(grep -oP 'in \K[0-9.]+(?=s)' "$WORK/rustmc-$cx-$cz.log" | head -1 || echo -1)"
        got="$("$DIFFER" scan "$out/region" --bbox $((cx - R)) $((cz - R)) $((cx + R)) $((cz + R)) 2>/dev/null \
                | grep -oP 'full=\K[0-9]+' || echo 0)"
        echo "rustmc $centre -> gen=$gen wall_with_write=$(python3 -c "print(f'{$t1-$t0:.2f}')") cpu=$cpu peak_rss=${rss}G peak_cores=$peak"
        printf 'rustmc\t%s\t%s\t%s\t%s\t%s\t-1\n' "$centre" "$got" "$gen" "$cpu" "$peak" >> "$RESULTS"
    done
    local first="${CENTRES%% *}"
    mkdir -p "$WORK/worlds/rustmc"
    cp "$WORK/worlds/rustmc-${first%%,*}-${first##*,}/region"/*.mca "$WORK/worlds/rustmc/" 2>/dev/null || true
}

# ---------------------------------------------------------------- main ----

: > "$WORK/raw.tsv"
port=25801
for flavour in $FLAVOURS; do
    case "$flavour" in
        vanilla) run_java vanilla "$(vanilla_jar)" $((port++)) ;;
        paper)   run_java paper   "$(paper_jar paper)" $((port++)) ;;
        folia)   run_java folia   "$(paper_jar folia)" $((port++)) ;;
        rustmc)  run_rustmc ;;
        *) echo "unknown flavour $flavour" >&2; exit 2 ;;
    esac
done

echo
echo "=== results: flavour centre chunks wall_s cpu_s peak_cores spawn_ms ==="
echo "=== r=$R, ${WANT} chunks asked for per square, seed $SEED ==="
echo "=== RUSTMC_WORKERS=$RUSTMC_WORKERS ($WORKERS_SOURCE); heap $HEAP ==="
echo "=== JVM worldgen pool: $JVM_WORKERS ==="
cat "$RESULTS"

# Parity over the first centre's square. Vanilla FIRST, always: region-differ
# labels its pairs from the first directory argument, and read the wrong way
# round a missing structure looks like an invented one.
first="${CENTRES%% *}"; fcx="${first%%,*}"; fcz="${first##*,}"
if [ -d "$WORK/worlds/vanilla" ]; then
    echo
    echo "=== parity over centre $first, chunks $((fcx-R)),$((fcz-R)) -> $((fcx+R)),$((fcz+R)) ($WANT chunks) ==="
    for flavour in paper folia rustmc; do
        [ -d "$WORK/worlds/$flavour" ] || continue
        echo "--- vanilla vs $flavour ---"
        "$DIFFER" diff "$WORK/worlds/vanilla" "$WORK/worlds/$flavour" \
            --bbox $((fcx - R)) $((fcz - R)) $((fcx + R)) $((fcz + R)) 2>&1 | head -6
    done
fi
