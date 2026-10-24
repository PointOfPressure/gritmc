#!/usr/bin/env bash
# t2-sigterm-saves.sh — a clean stop saves a player the server was never told
# was leaving, and a kill -9 does not.
#
# WHAT THIS MEASURES THAT NO OTHER GATE DOES
#
# Three paths in this server write `players/data/<uuid>.dat`, and until
# 2026-09-05 only the first existed:
#
#   the disconnect path   `crate::status`, when the play loop ends
#   the autosave          `crate::autosave`, every 6000 ticks
#   the shutdown hook     `crate::shutdown`, on SIGTERM
#
# `ci/t2-player-survives-a-restart.sh` exercises the first and cannot see the
# other two: its probe closes its socket, so the file is on disk before
# anything else could have written it. A gate that let the probe disconnect
# could never tell a shutdown hook from a disconnect handler, because all
# three end with the same file in the same place.
#
# So this gate keeps the player CONNECTED — `ci/rejoin-probe.py --stay` holds
# the socket open and answers keep-alives — and signals the process
# underneath them. The autosave is pushed out to an hour
# (`--autosave-seconds 3600`) so it cannot be what wrote the file, and the
# world directory is fresh, so there is no earlier file to be mistaken for a
# new one.
#
#   arm A   after SIGTERM, players/data/<uuid>.dat exists and its x is the
#           position the probe walked to — not the spawn.
#   arm B   the server said so: `Saving players`, `Saving worlds` and
#           `All dimensions are saved`, which are `stopServer`'s own log lines
#           in its own order. Arm A alone would pass on a file written by any
#           of the three paths; arm B is what says which one.
#   arm C   the exit status is 0. A process that died of the default SIGTERM
#           disposition exits 143 (128 + 15) and would have written nothing —
#           but a process that wrote the file and THEN crashed would pass arms
#           A and B, and a shutdown that ends in a crash is not a clean stop.
#
# THE NEGATIVE CONTROL IS `kill -9`, AND IT IS THE RIGHT ONE
#
# SIGKILL cannot be caught, by this or any program. So the control differs
# from the gate in exactly one respect — which signal — and every other thing
# is held: the same binary, the same world, the same walk, the same probe
# still connected, the same hour-long autosave. If the file appears anyway,
# something other than the hook wrote it and arm A is not measuring the hook.
#
# That is a stronger control than deleting the world or disabling the flag,
# both of which change what is on disk as well as what ran.
#
# Usage: ci/t2-sigterm-saves.sh [--negative-control]
#   T2SS_PORT    default 26791
#   T2SS_STEPS   blocks to walk east, default 6

set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${T2SS_PORT:-26791}"
NAME="${T2SS_NAME:-Stopper}"
STEPS="${T2SS_STEPS:-6}"
NEGATIVE=0
[ "${1:-}" = "--negative-control" ] && NEGATIVE=1

WORK="$ROOT/oracle-work/t2-sigterm-saves"
WORLD="$WORK/world"
BIN="$ROOT/target/release/rustmc-server"
PROBE="$ROOT/target/release/level-probe"
SRV_PID=""
PROBE_PID=""

fail() { echo "T2-SIGTERM-SAVES RED — $*"; exit 1; }
cleanup() {
    [ -n "$PROBE_PID" ] && kill -9 "$PROBE_PID" 2>/dev/null
    [ -n "$SRV_PID" ] && kill -9 "$SRV_PID" 2>/dev/null
    return 0
}
trap cleanup EXIT

command -v cargo >/dev/null 2>&1 || {
    echo "no cargo on PATH; over ssh prefix with: export PATH=\"\$HOME/.cargo/bin:\$PATH\"" >&2
    exit 2
}
# shellcheck source=ci/server-ready.sh
. "$ROOT/ci/server-ready.sh"
cargo build -q --release -p rustmc-server -p level-probe || fail "could not build"

rm -rf "$WORK"; mkdir -p "$WORK"
LOG="$WORK/server.log"
PLOG="$WORK/probe.log"

# --- boot, with the autosave pushed out of reach ----------------------------
#
# 3600 seconds is 72,000 ticks. The whole gate runs in under a minute, so the
# autosave cannot fire — and that is asserted below rather than assumed, by
# grepping the log for the line it would print.
"$BIN" run --port "$PORT" --radius 1 --view-distance 2 --gamemode creative \
    --world "$WORLD" --autosave-seconds 3600 >"$LOG" 2>&1 &
SRV_PID=$!
ready=0
for _ in $(seq 1 240); do
    grep -q "listening on 0.0.0.0:$PORT[ ,]" "$LOG" 2>/dev/null && ready_socket_owner "$PORT" "$SRV_PID" && { ready=1; break; }
    kill -0 "$SRV_PID" 2>/dev/null || break
    sleep 0.5
done
[ "$ready" = 1 ] || { tail -20 "$LOG" >&2; fail "the server never bound :$PORT (see $LOG)"; }
echo "== up on :$PORT (pid $SRV_PID), autosave pushed to 3600s"

# The hook has to be armed, or the whole gate is measuring its absence and
# calling it a red. This is a precondition, not an arm: a build without
# `--world` support, or one where `libc::signal` failed, says so here and the
# gate refuses rather than reporting a parity failure.
grep -q "^shutdown: SIGTERM and SIGINT save before exiting" "$LOG" || {
    grep -q "^shutdown: could not install" "$LOG" && \
        fail "the server could not install its signal handler — see $LOG"
    fail "the server never said it armed a shutdown hook; this build has none, \
so nothing below would mean anything (see $LOG)"
}

# --- the player joins, walks, and STAYS -------------------------------------
"$ROOT/ci/rejoin-probe.py" 127.0.0.1 "$PORT" "$NAME" --walk-east "$STEPS" --stay 45 \
    >"$PLOG" 2>&1 &
PROBE_PID=$!
# Wait for `STAYING`, not for a sleep. The probe flushes that line only after
# the walk has finished and it is holding the socket, so it is the exact
# moment the signal may be sent — and it is registered in
# `ci/checks/liveness-proofs.toml` with a log that a probe which has joined
# but not yet walked produces, which the predicate must refuse.
staying=0
for _ in $(seq 1 120); do
    grep -q '^STAYING ' "$PLOG" 2>/dev/null && { staying=1; break; }
    kill -0 "$PROBE_PID" 2>/dev/null || break
    sleep 0.5
done
[ "$staying" = 1 ] || { cat "$PLOG" >&2; fail "the probe never reported STAYING (see $PLOG)"; }

spawn_x=$(grep -oP '^REJOIN-SPAWN x=\K[-0-9.]+' "$PLOG")
walked_x=$(grep -oP '^REJOIN-WALKED x=\K[-0-9.]+' "$PLOG")
[ -n "$spawn_x" ] && [ -n "$walked_x" ] || { cat "$PLOG" >&2; fail "the probe gave no positions"; }
[ "$spawn_x" != "$walked_x" ] || fail \
    "the probe walked $STEPS blocks and ended where it started ($spawn_x) — \
nothing downstream of this can mean anything"
echo "== connected, spawned at x=$spawn_x, walked to x=$walked_x, still holding the socket"

# Nothing may have written the player file yet. This is what makes arm A
# evidence about the signal rather than about the join: if the file is already
# there, some other path wrote it and the comparison below is not about the
# hook at all.
if [ -e "$WORLD/players/data" ] && [ -n "$(find "$WORLD/players/data" -name '*.dat' 2>/dev/null)" ]; then
    fail "players/data already holds a .dat before any signal was sent — \
something other than the shutdown hook writes it, and arm A could not tell them apart"
fi

# --- the signal -------------------------------------------------------------
if [ "$NEGATIVE" = 1 ]; then
    echo "== negative control: SIGKILL, which no program can catch"
    kill -9 "$SRV_PID"
else
    echo "== SIGTERM"
    kill -TERM "$SRV_PID"
fi
wait "$SRV_PID"; SRV_RC=$?
SRV_PID=""
kill -9 "$PROBE_PID" 2>/dev/null; PROBE_PID=""
echo "   the server exited $SRV_RC"

# The autosave must NOT be what wrote anything. Asserted from the server's own
# log rather than from the interval arithmetic, because the arithmetic is a
# claim about a flag and this is a reading of what happened.
if grep -q "^autosave: wrote " "$LOG"; then
    fail "the autosave fired during this run despite --autosave-seconds 3600, \
so the file below is not evidence about the shutdown hook"
fi

mapfile -t files < <(find "$WORLD/players/data" -name '*.dat' 2>/dev/null | sort)

if [ "$NEGATIVE" = 1 ]; then
    if [ "${#files[@]}" -ne 0 ]; then
        fail "NEGATIVE CONTROL FAILED — kill -9 left ${#files[@]} player file(s) \
under $WORLD/players/data: ${files[*]}. SIGKILL reaches no handler, so whatever \
wrote that is not the shutdown hook and arm A does not measure one."
    fi
    # 137 = 128 + 9. Asserted so that "no file" cannot be produced by the
    # server having died of something else before the walk.
    [ "$SRV_RC" = 137 ] || fail "NEGATIVE CONTROL FAILED — the server exited \
$SRV_RC, not 137; it did not die of the SIGKILL this control sent"
    echo "T2-SIGTERM-SAVES CONTROL GREEN — kill -9 wrote no player file and \
exited 137, so arm A can go red"
    exit 0
fi

# --- arm A ------------------------------------------------------------------
[ "${#files[@]}" = 1 ] || fail "arm A: ${#files[@]} .dat files under \
$WORLD/players/data, want exactly 1 — the shutdown hook wrote nothing, or \
wrote more than one player: ${files[*]}"
uuid=$(basename "${files[0]}" .dat)
saved=$("$PROBE" getplayer "$WORLD" "$uuid") || fail "arm A: could not read the player file"
printf '%s\n' "$saved" | sed 's/^/   saved /'
saved_x=$(printf '%s\n' "$saved" | grep -oP '^pos=\K[-0-9.]+')
[ "$saved_x" = "$walked_x" ] || fail "arm A: the hook saved x=$saved_x; the \
player was standing at x=$walked_x (the spawn is x=$spawn_x). A file holding \
the spawn is a file written before the walk."

# --- arm B ------------------------------------------------------------------
#
# `stopServer`'s three lines, in its order. Checked as an ORDER and not as a
# set: `saveEverything` saves the players before it touches a chunk, and a
# server that got that backwards would print the same three words and write a
# world whose players are older than its chunks.
order=$(grep -nE '^(Saving players|Saving worlds|ThreadedAnvilChunkStorage: All dimensions are saved)$' "$LOG" \
    | sed 's/:.*//' | tr '\n' ' ')
set -- $order
[ $# -eq 3 ] || fail "arm B: found $# of stopServer's three log lines in $LOG, \
so this was not a clean stop: [$order]"
[ "$1" -lt "$2" ] && [ "$2" -lt "$3" ] || fail \
    "arm B: stopServer's lines are out of order at lines $order — players must \
be saved before the worlds, and the chunks before the 'all saved' line"

# --- arm C ------------------------------------------------------------------
[ "$SRV_RC" = 0 ] || fail "arm C: the server exited $SRV_RC. 143 is the \
default SIGTERM disposition (no handler ran); anything else is a crash after \
the save, which is not a clean stop."

echo "T2-SIGTERM-SAVES GREEN — SIGTERM to a server with a connected player \
saved them at x=$saved_x (spawn x=$spawn_x), in stopServer's own order, \
exiting 0"
