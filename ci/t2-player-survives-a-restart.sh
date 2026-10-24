#!/usr/bin/env bash
# t2-player-survives-a-restart.sh — a player logs out somewhere, the server is
# stopped and started again, and the player comes back where they left.
#
# WHY A GATE AND NOT A UNIT TEST
#
# `players/data/<uuid>.dat` has unit tests either side of it — the format
# round-trips in `rustmc_world::playerdat` and the store round-trips in
# `rustmc_server::levelstore` — and neither of them can see the thing that
# actually matters: whether the *server* saves on disconnect and reads on join.
# That path runs in `status.rs`'s connection thread, reads the position out of
# the tracker's entity rather than out of `PlayerState` (which is a local of
# the play loop and has gone by then), and only exists over a socket.
#
# THE THREE ARMS
#
#   A  the walk moved the server's belief. Run 1 joins at the spawn, walks
#      east, and disconnects; run 2's join position must differ from run 1's
#      by the distance walked. **This arm is what stops arm B being vacuous**:
#      if the walk never reached the server, run 2 would join at the spawn and
#      "came back where it left" would be true of a player who never left.
#   B  the file exists and says the same thing. `level-probe getplayer` reads
#      `players/data/<uuid>.dat` and its position must be run 2's join
#      position. A server that kept the player in memory across a restart is
#      impossible, but a server that wrote the file and ignored it on join is
#      not, and this separates them.
#   C  the negative control. `--negative-control` runs the identical sequence
#      with the world directory wiped between the two boots. Run 2 must then
#      join at the SPAWN, not at the walked-to position — which is the state
#      this gate reports green for if the save path silently does nothing and
#      the spawn happens to be where the player walked.
#
# The uuid is `wire::offline_uuid(name)`, vanilla's own scheme, so the file
# name is predictable from the player name and arm B needs no discovery step.

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${T2PR_PORT:-26787}"
NAME="${T2PR_NAME:-Walker}"
STEPS="${T2PR_STEPS:-6}"
NEGATIVE=0
[ "${1:-}" = "--negative-control" ] && NEGATIVE=1

WORK="$ROOT/oracle-work/t2-player-survives-a-restart"
WORLD="$WORK/world"
BIN="$ROOT/target/release/rustmc-server"
PROBE="$ROOT/target/release/level-probe"
SRV_PID=""

fail() { echo "T2-PLAYER-SURVIVES-A-RESTART RED — $*"; exit 1; }
stop_server() {
    [ -n "$SRV_PID" ] || return 0
    kill "$SRV_PID" 2>/dev/null || true
    for _ in $(seq 1 60); do kill -0 "$SRV_PID" 2>/dev/null || break; sleep 0.5; done
    kill -9 "$SRV_PID" 2>/dev/null || true
    wait "$SRV_PID" 2>/dev/null || true
    SRV_PID=""
}
trap stop_server EXIT

command -v cargo >/dev/null 2>&1 || {
    echo "no cargo on PATH; over ssh prefix with: export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    exit 2
}
# shellcheck source=ci/server-ready.sh
. "$ROOT/ci/server-ready.sh"
cargo build -q --release -p rustmc-server -p level-probe || fail "could not build"

rm -rf "$WORK"; mkdir -p "$WORK"

boot() { # boot <run-label>
    local label="$1"
    local log="$WORK/$label.log"
    # `--autosave-seconds 2` so the world is written without waiting five
    # minutes; the player save is on disconnect and does not need it, but the
    # region files do and a half-saved world is not what this measures.
    "$BIN" run --port "$PORT" --radius 1 --view-distance 2 --gamemode creative \
        --world "$WORLD" --autosave-seconds 2 >"$log" 2>&1 &
    SRV_PID=$!
    local ready=0
    for _ in $(seq 1 240); do
        grep -q "listening on 0.0.0.0:$PORT[ ,]" "$log" 2>/dev/null && ready_socket_owner "$PORT" "$SRV_PID" && { ready=1; break; }
        kill -0 "$SRV_PID" 2>/dev/null || break
        sleep 0.5
    done
    [ "$ready" = 1 ] || {
        tail -20 "$log" >&2
        fail "$label: the server never bound :$PORT (see $log)"
    }
    echo "   $label: up on :$PORT (pid $SRV_PID)"
}

# --- run 1: join at the spawn, walk east, disconnect ------------------------
boot run1
out1=$(python3 "$ROOT/ci/rejoin-probe.py" 127.0.0.1 "$PORT" "$NAME" --walk-east "$STEPS") \
    || fail "run 1 probe failed: $out1"
printf '%s\n' "$out1" | sed 's/^/   run1 /'
spawn_x=$(printf '%s\n' "$out1" | grep -oP '^REJOIN-SPAWN x=\K[-0-9.]+') \
    || fail "run 1 printed no REJOIN-SPAWN"
walked_x=$(printf '%s\n' "$out1" | grep -oP '^REJOIN-WALKED x=\K[-0-9.]+') \
    || fail "run 1 printed no REJOIN-WALKED"
[ -n "$spawn_x" ] && [ -n "$walked_x" ] || fail "run 1 gave no positions"
# The walk has to have gone somewhere, or arm A below compares a number to
# itself.
[ "$spawn_x" != "$walked_x" ] || fail \
    "the probe walked $STEPS blocks and ended where it started ($spawn_x) — \
nothing downstream of this can mean anything"

# The disconnect is what triggers the save; give the connection thread a
# moment to run its leave path before the process is killed under it.
sleep 2
stop_server

if [ "$NEGATIVE" = 1 ]; then
    echo "== negative control: wiping the world directory between the two boots"
    rm -rf "$WORLD"
fi

# --- run 2: join again, and see where the server puts us --------------------
boot run2
out2=$(python3 "$ROOT/ci/rejoin-probe.py" 127.0.0.1 "$PORT" "$NAME") \
    || fail "run 2 probe failed: $out2"
printf '%s\n' "$out2" | sed 's/^/   run2 /'
rejoin_x=$(printf '%s\n' "$out2" | grep -oP '^REJOIN-SPAWN x=\K[-0-9.]+')
[ -n "$rejoin_x" ] || fail "run 2 printed no REJOIN-SPAWN"
sleep 2
stop_server

if [ "$NEGATIVE" = 1 ]; then
    if [ "$rejoin_x" = "$walked_x" ]; then
        fail "NEGATIVE CONTROL FAILED — with the world directory deleted the \
player still rejoined at $rejoin_x, the position they walked to. Whatever this \
gate is reading, it is not the saved file."
    fi
    echo "T2-PLAYER-SURVIVES-A-RESTART CONTROL GREEN — with no saved world the \
player rejoined at $rejoin_x, not the walked-to $walked_x, so arm A can go red"
    exit 0
fi

# --- arm A ------------------------------------------------------------------
[ "$rejoin_x" = "$walked_x" ] || fail "arm A: walked to x=$walked_x, rejoined \
at x=$rejoin_x (the spawn is x=$spawn_x) — the player did not come back where \
they left"

# --- arm B ------------------------------------------------------------------
# The one file in `players/data`. Asserting there is EXACTLY one is itself
# part of the arm: two would mean the offline uuid is not stable across a
# restart, and arm A would then be passing for a reason this gate does not
# name. `.dat_old` does not match `*.dat`, so a second save does not add one.
mapfile -t files < <(find "$WORLD/players/data" -name '*.dat' 2>/dev/null | sort)
[ "${#files[@]}" = 1 ] || fail "arm B: ${#files[@]} .dat files under \
$WORLD/players/data, want exactly 1 — the offline uuid is not stable across a \
restart, or something else wrote there: ${files[*]}"
uuid=$(basename "${files[0]}" .dat)
saved=$("$PROBE" getplayer "$WORLD" "$uuid") || fail "arm B: could not read the player file"
printf '%s\n' "$saved" | sed 's/^/   saved /'
saved_x=$(printf '%s\n' "$saved" | grep -oP '^pos=\K[-0-9.]+')
[ "$saved_x" = "$rejoin_x" ] || fail "arm B: players/data says x=$saved_x and \
the join put us at x=$rejoin_x — the file and the join path disagree"

echo "T2-PLAYER-SURVIVES-A-RESTART GREEN — spawned x=$spawn_x, walked to \
x=$walked_x, rejoined at x=$rejoin_x, players/data agrees"
