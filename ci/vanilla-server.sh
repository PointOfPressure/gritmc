#!/usr/bin/env bash
# vanilla-server.sh — fetch/boot/drive the REAL vanilla server (the oracle's
# ground truth, ORACLE.md top block). Jar + worlds live in /oracle-work/
# (gitignored — Mojang-derived, never committed).
#
#   fetch                       download server jar for the pinned version
#   reports                     run Mojang's data generator (packets.json etc.)
#   data                        run Mojang's data generator (--server): the
#                               built-in datapack, i.e. worldgen JSON
#   start <dir> <port> [type] [seed]   boot offline-mode server, wait for Done
#   stop <dir>                  stop a started server
#   console <dir> "<command>"   send a console command to a running server
#   alive <dir>                 exit 0 iff the server started in <dir> is running
#   preserve <dir> "<reason>"   copy a failed run's evidence somewhere durable
#
# `start` exit codes are distinct on purpose, because two of these failures are
# nothing like each other and used to be reported identically:
#   0  up      1  died for some other reason      2  120s without `Done (`
#   3  the port was already in use
#
# Pinned version comes from rustmc-proto GAME_VERSION (single source of truth).
#
# STRUCTURES ARE ON, and the place to check is not this script. The properties
# block in `start` does not mention `generate-structures`, so the server fills
# in its own default and writes `generate-structures=true` back into
# server.properties. The authoritative record is the world, and **26.2 moved
# it**: level.dat is now 513 bytes with no seed, no gamerules and no
# `WorldGenSettings` at all, and worldgen settings live in
# `world/data/minecraft/world_gen_settings.dat` under the **renamed** key
# `generate_structures` (it was `generate_features`). Verified 2026-07-30 on
# oracle-work/gen-20260722-25875: TAG_Byte generate_structures = 1, seed
# 0x01352772 = 20260722. Grepping level.dat for `generate_features` finds
# nothing, which reads exactly like "structures are off" and is not.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORK="$ROOT/oracle-work"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$WORK/vanilla-$VER"
JAR="$VDIR/server.jar"

# Where a failed boot's evidence goes. **Under `oracle-work/`, and that is the
# whole point**: `oracle-work` is gitignored *and* excluded from
# `ci/t0-remote.sh`'s `rsync --delete`, so it is the one directory on the box
# that neither the next commit nor the next sync can wipe. `ci/logs/` would
# have been the obvious home and is the wrong one — the next rsync deletes it.
FAILDIR="$WORK/server-failures"

# --- liveness, evidence, and a console that cannot block ---------------------
#
# All of this exists because of one incident, 2026-08-05 22:55 and again
# 2026-08-06 00:16, and every line below is one of its lessons.
#
# **The hang.** Vanilla failed to boot in `~/rustmc-gate`. `start` returned 1;
# `ci/t2-gameplay.sh`'s EXIT trap ran `stop`; `stop` wrote `stop` into
# `console.fifo` — and *nothing had that fifo open for reading, because the
# only reader was the server and the server was dead*. `open(2)` on a fifo for
# writing blocks until a reader appears, so it blocked forever and took the
# whole gate shell with it. No log line, no exit, no timeout: **67 minutes
# indistinguishable from a stage still running**, ended by hand. A gate that
# hangs is strictly worse than a gate that fails — a red gate is information, a
# hang is not. So: `console` checks liveness *and* bounds the write, and `stop`
# never sends anything to a process that is not there.
#
# **The destroyed evidence.** `ci/t2-gameplay.sh` wipes
# `oracle-work/gameplay/vanilla` at the start of every run and again on the way
# out, so the `crash-reports/` the server had just written was gone before
# anyone could read it. All that survived was `tail -5` of the log.
#
# **And `tail -5` names no cause.** Those five lines —
# `at java.base/java.lang.Thread.run`, `This crash report has been saved to:`,
# `Stopping server`, `Saving worlds`, `All dimensions are saved` — are
# **byte-identical for every server-thread exception there is**. Measured
# 2026-08-06: booting a second server onto an occupied port reproduces that
# exact tail. The three lines that actually name the cause —
# `**** FAILED TO BIND TO PORT!`, `bind(..) failed with error(-98): Address
# already in use`, `Perhaps a server is already running on that port?` — sit
# six lines *above* the window. The gate was reading the part of the log that
# is the same no matter what went wrong, and a port collision was chased as a
# mystery crash. `boot_failure` below prints the cause, not the tail.

server_pid() {          # server_pid <dir> — echo the pid, non-zero if none
    local pid
    [ -f "$1/server.pid" ] || return 1
    pid="$(cat "$1/server.pid" 2>/dev/null)" || return 1
    [ -n "$pid" ] || return 1
    printf '%s' "$pid"
}

alive() {               # alive <dir> — is the server we started still running?
    local pid
    pid="$(server_pid "$1")" || return 1
    kill -0 "$pid" 2>/dev/null
}

# Copy the four things that explain a failed boot into $FAILDIR, which nothing
# in the gate deletes. Deliberately *not* called on success: this is evidence,
# not an archive. The newest 20 are kept.
preserve() {            # preserve <dir> <reason...>
    local dir="$1"; shift
    local reason="${*:-unspecified}" dest
    [ -d "$dir" ] || return 0
    # One failure, one copy. `start` preserves when the boot fails, then the
    # caller's EXIT trap runs `stop`, which finds the same corpse and would
    # preserve it again, and t2-gameplay's cleanup would make it three — three
    # identical copies, filling a ring that only keeps twenty. The marker lives
    # inside $dir, which every caller wipes before the next run, so it expires
    # exactly when it should.
    if [ -e "$dir/.preserved" ]; then
        echo "vanilla-server: evidence for $dir already preserved in $(cat "$dir/.preserved")"
        return 0
    fi
    mkdir -p "$FAILDIR"
    dest="$FAILDIR/$(date -u +%Y%m%dT%H%M%SZ)-$(basename "$dir")-$$"
    mkdir -p "$dest"
    {
        printf 'reason:  %s\n' "$reason"
        printf 'dir:     %s\n' "$dir"
        printf 'when:    %s\n' "$(date -Is)"
        printf 'host:    %s\n' "$(hostname)"
        printf 'tree:    %s\n' "$ROOT"
    } > "$dest/WHY.txt"
    for f in server.log server.properties server.pid; do
        [ -e "$dir/$f" ] && cp -p "$dir/$f" "$dest/" 2>/dev/null || true
    done
    [ -f "$dir/logs/latest.log" ] && cp -p "$dir/logs/latest.log" "$dest/latest.log" 2>/dev/null || true
    [ -d "$dir/crash-reports" ] && cp -rp "$dir/crash-reports" "$dest/" 2>/dev/null || true
    ( ls -1dt "$FAILDIR"/*/ 2>/dev/null || true ) | tail -n +21 | xargs -r rm -rf
    printf '%s\n' "$dest" > "$dir/.preserved" 2>/dev/null || true
    echo "vanilla-server: PRESERVED the evidence in $dest"
    echo "                ($(find "$dest" -type f | wc -l) files: server.log, latest.log, crash-reports/, WHY.txt)"
}

# Would a server be able to bind this port? SO_REUSEADDR is set so this mirrors
# what the JVM's listener does — a socket sitting in TIME_WAIT from a server we
# just stopped is *not* a conflict and must not be reported as one, or this
# check would turn working boots into refusals. Only a live listener fails it.
port_free() {           # port_free <port>
    # No python3 means no opinion, and no opinion must mean "go ahead". A probe
    # that cannot run is not evidence of a busy port, and refusing on it would
    # turn a working boot into a refusal — the one way this check could be
    # worse than not having it.
    command -v python3 >/dev/null 2>&1 || return 0
    python3 - "$1" <<'PY'
import socket, sys
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
try:
    s.bind(("0.0.0.0", int(sys.argv[1])))
except OSError:
    sys.exit(1)
finally:
    s.close()
PY
}

# Who holds it — for the message only, never for the decision. `ss` lives in
# /usr/sbin, which is not on a non-interactive ssh PATH, and a silent empty
# answer here once read as "nothing is listening".
port_holder() {         # port_holder <port>
    local PATH="/usr/sbin:/sbin:$PATH" out
    command -v ss >/dev/null 2>&1 || { echo "(ss unavailable on this PATH)"; return 0; }
    out="$(ss -ltnp 2>/dev/null | awk -v p=":$1\$" '$4 ~ p' | head -3)"
    [ -n "$out" ] && printf '%s\n' "$out" || echo "(no listener visible now — it may have exited since)"
}

# Print the cause of a failed boot and preserve it. Returns the exit code
# `start` should return: 3 for a port collision, 1 for anything else.
boot_failure() {        # boot_failure <dir> <port> <headline>
    local dir="$1" port="$2" headline="$3" rc=1 cr
    echo "vanilla-server: $headline"
    if grep -q 'FAILED TO BIND TO PORT' "$dir/server.log" 2>/dev/null; then
        rc=3
        echo "vanilla-server: CAUSE — PORT COLLISION on :$port."
        echo "                Vanilla could not bind, threw 'Failed to initialize server' and"
        echo "                wrote a crash report. Its last five lines look like every other"
        echo "                server-thread exception; these are the lines that mean something:"
        grep -E 'FAILED TO BIND|The exception was:|Perhaps a server' "$dir/server.log" \
            | sed 's/^/                  /'
        echo "                holding :$port now:"
        port_holder "$port" | sed 's/^/                  /'
    else
        echo "vanilla-server: the exception itself, from the log (NOT the tail):"
        grep -m1 -A14 -E 'Encountered an unexpected exception|Failed to start the minecraft server|ERROR\]: Failed' \
            "$dir/server.log" 2>/dev/null | sed 's/^/                  /' \
            || tail -20 "$dir/server.log" 2>/dev/null | sed 's/^/                  /' || true
        cr="$( (ls -1t "$dir/crash-reports/"*.txt 2>/dev/null || true) | head -1 )"
        if [ -n "$cr" ]; then
            echo "vanilla-server: and the crash report's own description:"
            sed -n '1,10p' "$cr" | sed 's/^/                  /'
        fi
    fi
    preserve "$dir" "$headline (port $port)"
    return "$rc"
}

fetch() {
    mkdir -p "$VDIR"
    [ -s "$JAR" ] && { echo "jar present: $JAR"; return 0; }
    local manifest url sha1
    manifest=$(curl -fsSL https://piston-meta.mojang.com/mc/game/version_manifest_v2.json)
    url=$(echo "$manifest" | python3 -c "import sys,json;m=json.load(sys.stdin);print([v['url'] for v in m['versions'] if v['id']=='$VER'][0])")
    local vjson; vjson=$(curl -fsSL "$url")
    read -r durl sha1 < <(echo "$vjson" | python3 -c "import sys,json;d=json.load(sys.stdin)['downloads']['server'];print(d['url'],d['sha1'])")
    echo "downloading vanilla $VER server jar..."
    curl -fsSL -o "$JAR" "$durl"
    echo "$sha1  $JAR" | sha1sum -c - >/dev/null
    echo "fetched + sha1-verified: $JAR"
}

# The built-in datapack, straight from Mojang's own generator.
#
# Worldgen stopped shipping as JSON in the jar years ago — it is built in
# code by `VanillaRegistries` — but the data generator still emits it. That
# matters for anything shaped like a rule tree rather than an algorithm: the
# overworld surface rule is 35 KB of JSON, 22 levels deep, with 149
# conditions across 15 operator types. Transcribing that from
# `SurfaceRuleData`'s builder calls would be 149 chances to be quietly wrong
# in a way no near-origin test could catch.
#
# Same principle as `reports` and packet ids: the generator is the source,
# never memory and never a hand copy.
#
# Output lands in oracle-work/ and is gitignored, like every other
# Mojang-derived artifact.
data() {
    fetch
    [ -s "$VDIR/generated-data/data/minecraft/worldgen/noise_settings/overworld.json" ] \
        && { echo "server data present"; return 0; }
    echo "running Mojang data generator (--server)..."
    (cd "$VDIR" && java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar \
        --server --output generated-data >/dev/null 2>&1)
    test -s "$VDIR/generated-data/data/minecraft/worldgen/noise_settings/overworld.json"
    echo "server data at $VDIR/generated-data/data/"
}

reports() {
    fetch
    [ -s "$VDIR/generated/reports/packets.json" ] && { echo "reports present"; return 0; }
    echo "running Mojang data generator (--reports)..."
    (cd "$VDIR" && java -DbundlerMainClass=net.minecraft.data.Main -jar server.jar --reports >/dev/null 2>&1)
    test -s "$VDIR/generated/reports/packets.json"
    echo "reports at $VDIR/generated/reports/"
}

start() {
    local dir port="$2" ltype="${3:-minecraft:normal}" seed="${4:-}"
    mkdir -p "$1"
    dir="$(cd "$1" && pwd)"   # absolutize: the subshell cd's before redirecting
    # The preserve marker belongs to one server's lifetime, not to the
    # directory. `ci/t2-gameplay.sh` wipes its run directory every run so it would
    # expire anyway, but `ci/gen-vanilla-region.sh` and friends reuse theirs —
    # a marker left by an earlier failure would silently suppress preserving
    # the next one, which is the same evidence-destroying bug in a new place.
    rm -f "$dir/.preserved"
    # Refuse before spending 40s of JVM startup on a boot that cannot succeed.
    # Sixty trees share this box and several scripts still hardcode a port; the
    # loser of that race used to spend the startup, write a crash report, and
    # report five lines that named nothing.
    if ! port_free "$port"; then
        echo "vanilla-server: REFUSING to boot in $dir — :$port is already in use." >&2
        echo "                Nothing was started; no crash report will be written; this is" >&2
        echo "                a collision, not a defect in the tree. Holding it:" >&2
        port_holder "$port" | sed 's/^/                  /' >&2
        echo "                Set the caller's port variable to something free and re-run." >&2
        return 3
    fi
    fetch
    echo "eula=true" > "$dir/eula.txt"
    cat > "$dir/server.properties" <<P
server-port=$port
online-mode=false
level-type=$ltype
level-seed=$seed
view-distance=4
spawn-protection=0
enforce-secure-profile=false
sync-chunk-writes=false
motd=rustmc-oracle
# The watchdog kills the server when a tick blocks for 60s, and generating a
# forceloaded r7 square blocks the main thread for far longer than that on a
# busy machine. It killed an oracle world mid-generation while three agents
# and a build were competing for the CPU, and the failure is quiet: the
# script reports the server came up, the world directory stays empty, and an
# empty world is indistinguishable from one that has not finished yet.
#
# -1 disables it. Nothing here is a live server, so there is no watchdog to
# want.
max-tick-time=-1
P
    # Extra properties, appended verbatim and empty by default so every
    # existing gate produces the same server.properties it always has.
    #
    # The gameplay oracle needs a *different* server from the one the region
    # diffs need — creative, superflat, and structures off, because a village
    # under the check's feet is a difference between the two worlds and not
    # between the two servers. Rather than branch this function, the caller
    # says what it needs and this appends it.
    if [ -n "${VANILLA_EXTRA_PROPS:-}" ]; then
        printf '%s\n' "$VANILLA_EXTRA_PROPS" >> "$dir/server.properties"
    fi
    mkfifo "$dir/console.fifo" 2>/dev/null || true
    # Extra JVM flags, empty by default so every existing caller launches the
    # exact same JVM it always has. **This does not patch the jar and must
    # never be used to**: the only thing it is for is system properties the
    # unmodified server already reads for itself.
    #
    # The one that matters is `-Dmax.bg.threads=N`. Read out of the bytecode
    # 2026-08-06: `net/minecraft/util/Util.maxAllowedExecutorThreads()` is
    # `Mth.clamp(availableProcessors() - 1, 1, getMaxThreads())` and
    # `getMaxThreads()` is `Integer.parseInt(System.getProperty("max.bg.threads"))`
    # accepted in 1..255, defaulting to 255. `ChunkMap` dispatches every chunk
    # status upgrade onto `Util.backgroundExecutor()`, so `=1` makes worldgen
    # run on a single worker thread. It is the ONLY system property in the
    # server jar that touches worldgen — the full set the server reads is
    # `max.bg.threads`, `management.tls.keystore.password`, and the standard
    # `java.*`/`os.*`/`user.dir` crash-report fields.
    #
    # Whether it actually buys determinism is a measurement, not a claim; see
    # docs/DETERMINISM.md for the answer.
    # shellcheck disable=SC2086
    (exec 3<>"$dir/console.fifo"; cd "$dir" && exec java -Xmx1G ${VANILLA_JVM_OPTS:-} -jar "$JAR" --nogui <&3 >"$dir/server.log" 2>&1) &
    echo $! > "$dir/server.pid"
    local rc
    for _ in $(seq 1 120); do
        grep -q 'Done (' "$dir/server.log" 2>/dev/null && { echo "vanilla up on :$port ($dir)"; return 0; }
        if ! kill -0 "$(cat "$dir/server.pid")" 2>/dev/null; then
            rc=0; boot_failure "$dir" "$port" "server died before reaching 'Done ('" || rc=$?
            return "$rc"
        fi
        sleep 1
    done
    rc=0; boot_failure "$dir" "$port" "timeout: 120s elapsed without 'Done ('" || rc=$?
    if [ "$rc" -eq 1 ]; then rc=2; fi   # a timeout is its own kind of failure
    return "$rc"
}

# A console write must never be able to outlive the server it is talking to.
#
# Two guards, because they cover different things. The liveness check covers
# the common case — the server is already gone and the write would block
# immediately and forever. The timeout covers the race: the server can die
# between the check and the `open`, and then the open blocks just the same.
# Neither alone is enough, and the second one is what makes the property
# unconditional rather than probable.
#
# `timeout` really can interrupt this: a process blocked opening a fifo is in an
# interruptible sleep, so SIGTERM lands. `-k` is belt and braces.
console() {
    local dir="$1"; shift
    local cmd="$*"
    if [ ! -p "$dir/console.fifo" ]; then
        echo "vanilla-server: console — no fifo at $dir/console.fifo; DROPPED: $cmd" >&2
        return 2
    fi
    if ! alive "$dir"; then
        echo "vanilla-server: console — the server in $dir is NOT RUNNING; DROPPED: $cmd" >&2
        echo "                A write to the fifo would block forever: its only reader was" >&2
        echo "                the server process, and that process is gone." >&2
        return 2
    fi
    if ! timeout -k 2 "${VANILLA_CONSOLE_TIMEOUT:-15}" \
            bash -c 'printf "%s\n" "$2" > "$1/console.fifo"' _ "$dir" "$cmd"; then
        echo "vanilla-server: console — write to $dir/console.fifo did not complete in" >&2
        echo "                ${VANILLA_CONSOLE_TIMEOUT:-15}s (the server died mid-write); DROPPED: $cmd" >&2
        return 3
    fi
}

# `stop` returns 0 on an already-dead server *on purpose*, and the reasoning
# matters because the obvious alternative is wrong.
#
# Its postcondition is "no server is running in this directory", and that holds.
# Every caller in this tree — eight of them — invokes `stop` from an EXIT trap
# as best-effort cleanup, most with `|| true`; making it non-zero would change
# the exit status of scripts that are shutting down normally and would gate
# nothing. So `stop` is loud but successful, and the *redness* belongs to the
# caller, which is why `alive` is a subcommand: `ci/t2-gameplay.sh` asserts
# liveness at the two points where a dead vanilla invalidates the run, and dies
# there, with a message, instead of here.
stop() {
    local dir="$1" pid
    [ -f "$dir/server.pid" ] || return 0
    pid="$(cat "$dir/server.pid" 2>/dev/null || true)"
    if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
        echo "vanilla-server: stop — the server in $dir was ALREADY DEAD (pid ${pid:-none})."
        echo "                It did not survive to be stopped, so there is nothing to send"
        echo "                'stop' to. Before 2026-08-06 this is exactly where the gate hung."
        preserve "$dir" "stop found the server already dead"
        rm -f "$dir/server.pid"
        return 0
    fi
    console "$dir" "stop" || true
    for _ in $(seq 1 30); do kill -0 "$pid" 2>/dev/null || { rm -f "$dir/server.pid"; return 0; }; sleep 1; done
    echo "vanilla-server: stop — pid $pid ignored 'stop' for 30s; killing it" >&2
    kill -9 "$pid" 2>/dev/null || true; rm -f "$dir/server.pid"
}

"$@"
