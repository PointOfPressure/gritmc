#!/usr/bin/env bash
# ambient-census.sh — measure every classified packet family before classifying it.
#
#     ./ci/ambient-census.sh                       # the whole check directory
#     AMBIENT_GLOB='5*' ./ci/ambient-census.sh     # a subset, for the inner loop
#
# # WHAT THIS ANSWERS, AND WHY NO EXISTING RECORDING CAN
#
# `trace::FAMILIES` classifies fourteen packet families, and every row's class
# is a claim about a measurement: how many observations the family accounts
# for, whether **both** servers emit it, and whether vanilla is self-consistent
# about it between two runs of itself. A family vanilla emits on a timer, or
# whose order is not reproducible run to run, must stay `Class::Ambient`,
# because a check on it would flap rather than measure.
#
# **None of that could be read off the 100-odd recordings on this box.** Until
# 2026-09-06 `trace::is_housekeeping` dropped twelve of the families at record
# time, so every recording taken before then holds *nothing* of them: censusing
# those files censuses the old recorder, not the servers. The measurement
# therefore needs fresh recordings, and it needs **two** vanilla ones, because
# a single run cannot distinguish "vanilla always says this" from "vanilla said
# this once".
#
# So this script boots vanilla, records, boots vanilla **again from a fresh
# world directory**, records again, then boots this server and records once —
# and hands the three files to `ci/ambient-falsify.py --sweep`, which does the
# arithmetic and writes `ci/checks/ambient-families.toml`.
#
# # IT IS NOT A GATE AND NO NUMBER IT PRINTS IS A CORPUS SCORE
#
# Same rule as `ci/block-entity-subset.sh`, for the same reason: the
# denominator here is whatever matched the glob, and `docs/PARITY-ROADMAP.md`
# §20 exists because a corpus score was once compared across two different
# denominators. `ci/t2-gameplay.sh` is the gate. This writes an evidence file
# and nothing else.
#
# # THE LOCK IS TAKEN ON SIZE, AND THE THRESHOLD IS THE ARGUMENT
#
# Three recording passes at the ~9 s per scenario `ci/corpus-lock.sh` documents
# is about **fifty minutes over the whole directory** and about **eight over a
# glob of eighteen**. Those are different animals and the same script should
# not treat them alike:
#
#   * a fifty-minute unlocked run is throughput taken from whoever holds the
#     lock, and two overlapping corpus runs look exactly like an idle box —
#     the 2026-08-06 incident, where nothing crashed and three runs died;
#   * an eight-minute one is the same shape as `ci/block-entity-probe.sh` and
#     `ci/block-entity-subset.sh`, neither of which locks, and queueing it
#     behind a ten-deep corpus queue would cost hours to save minutes.
#
# So: **at most `AMBIENT_LOCK_ABOVE` scenarios runs unlocked; more takes the
# corpus lock as role `run`**, which still lets a real merge gate jump ahead.
# The evidence file records the denominator it was taken over, so a subset
# census cannot later be read as a whole-corpus one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

GLOB="${AMBIENT_GLOB:-*}"
VAN_PORT="${AMBIENT_VAN_PORT:-25894}"
OUR_PORT="${AMBIENT_OUR_PORT:-25895}"
WORK="$ROOT/oracle-work/gameplay/ambient-census"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"
LOCK_ABOVE="${AMBIENT_LOCK_ABOVE:-30}"
RUSTMC_PID=""

cleanup() {
    [ -n "$RUSTMC_PID" ] && kill "$RUSTMC_PID" 2>/dev/null || true
    for d in "$WORK/vanilla-a" "$WORK/vanilla-b"; do
        "$ROOT/ci/vanilla-server.sh" stop "$d" >/dev/null 2>&1 || true
    done
    if declare -F corpus_lock_release >/dev/null 2>&1; then corpus_lock_release; fi
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"
# shellcheck disable=SC2086
cp $ROOT/ci/checks/gameplay/$GLOB.toml "$SCEN/"
n=$(find "$SCEN" -name '*.toml' | wc -l)
[ "$n" -gt 0 ] || { echo "no checks matched $GLOB" >&2; exit 2; }
echo "== ambient census over $n scenario(s) matching '$GLOB' — NOT a corpus score"

# The tree this census measures, resolved HERE and not in the sweep: this
# script is normally run in the rsynced remote directory, which has no `.git`,
# so the sweep's own `git rev-parse` cannot answer and used to write
# `commit = ""`. `RUSTMC_CENSUS_COMMIT` is passed by the caller when the
# source checkout is elsewhere; otherwise this resolves it locally.
export RUSTMC_CENSUS_COMMIT="${RUSTMC_CENSUS_COMMIT:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null)}"
[ -n "$RUSTMC_CENSUS_COMMIT" ] || {
    echo "ambient-census: no commit for this tree — run from a checkout or set RUSTMC_CENSUS_COMMIT" >&2
    exit 2
}
echo "== tree $RUSTMC_CENSUS_COMMIT"

cargo build -q -p oracle-runner --release
cargo build -q --release -p rustmc-server
BIN="$ROOT/target/release/oracle-runner"
. "$ROOT/ci/server-ready.sh"
if [ "$n" -gt "$LOCK_ABOVE" ]; then
    echo "== $n > $LOCK_ABOVE scenarios — taking the corpus lock as role 'run'"
    . "$ROOT/ci/corpus-lock.sh"
    corpus_lock_acquire
else
    echo "== $n <= $LOCK_ABOVE scenarios — running unlocked, same as ci/block-entity-subset.sh"
fi

# `spawn-radius=0` and the four game rules are exactly `ci/t2-gameplay.sh`'s,
# so the census is taken under the conditions the gate records under. A census
# taken with random ticks on would attribute vanilla's own tick traffic to the
# scenario.
boot_vanilla() {   # $1 = world dir, $2 = port
    VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$1" "$2" "minecraft:flat" "$SEED"
    for r in advance_time spawn_mobs random_tick_speed advance_weather; do
        case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
        "$ROOT/ci/vanilla-server.sh" console "$1" "gamerule $r $v" || {
            echo "ambient-census: gamerule $r was refused — see the AGENT-BRIEF on 26.2 ids" >&2
            exit 3
        }
    done
    sleep 2
}

# **Two vanilla runs, two world directories, one after the other.** A second
# recording against the *same* live server would share its tick phase, its
# entity counter and its chunk-send state, so it would agree with the first for
# reasons having nothing to do with vanilla being reproducible. Self-consistency
# is a claim about two independent runs or it is not a claim at all.
echo "== vanilla A on :$VAN_PORT"
boot_vanilla "$WORK/vanilla-a" "$VAN_PORT"
"$BIN" record "127.0.0.1:$VAN_PORT" "$SCEN" --out "$WORK/vanilla-a.json"
"$ROOT/ci/vanilla-server.sh" stop "$WORK/vanilla-a"

echo "== vanilla B on :$VAN_PORT"
boot_vanilla "$WORK/vanilla-b" "$VAN_PORT"
"$BIN" record "127.0.0.1:$VAN_PORT" "$SCEN" --out "$WORK/vanilla-b.json"
"$ROOT/ci/vanilla-server.sh" stop "$WORK/vanilla-b"

echo "== rustmc on :$OUR_PORT"
./target/release/rustmc-server run --port "$OUR_PORT" --level-type flat --radius 2 \
    --view-distance 4 >"$WORK/rustmc.log" 2>&1 &
RUSTMC_PID=$!
# The same three arms `ci/block-entity-subset.sh` uses, and the socket-owner
# one for the same reason: a port a process was told to release is not a port
# it has released, and vanilla was on a different one but the box is shared.
RUSTMC_READY=0
for _ in $(seq 1 120); do
    grep -q "listening on 0.0.0.0:$OUR_PORT[ ,]" "$WORK/rustmc.log" 2>/dev/null && ready_socket_owner "$OUR_PORT" "$RUSTMC_PID" && ready_speaks_status "$BIN" "127.0.0.1:$OUR_PORT" && { RUSTMC_READY=1; break; }
    kill -0 "$RUSTMC_PID" 2>/dev/null || { echo "rustmc died — tail:"; tail -20 "$WORK/rustmc.log"; exit 1; }
    sleep 1
done
[ "$RUSTMC_READY" = 1 ] || {
    echo "rustmc-server never proved it was listening on :$OUR_PORT within 120s" >&2
    tail -20 "$WORK/rustmc.log" >&2
    exit 1
}
sleep 1
"$BIN" record "127.0.0.1:$OUR_PORT" "$SCEN" --out "$WORK/rustmc.json"

# The jar half of the `no-emitter` claim. No servers, no lock, ~90 s, and it
# is REQUIRED by the sweep rather than optional: a census can count zero over
# eighteen scenarios, and only the jar can say a packet has no construction
# site at all.
echo "== emitter sites"
./ci/packet-emitter-sites.py --json "$WORK/emitters.json"

echo "== sweeping"
./ci/ambient-falsify.py --sweep \
    --vanilla-a "$WORK/vanilla-a.json" \
    --vanilla-b "$WORK/vanilla-b.json" \
    --ours "$WORK/rustmc.json" \
    --scenarios "$SCEN" \
    --emitters "$WORK/emitters.json"
