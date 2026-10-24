#!/usr/bin/env bash
# weather-probe.sh — what does REAL vanilla put on the wire after `/weather rain`?
#
#     ./ci/weather-probe.sh                 # 3 cold vanilla runs
#     WEATHER_RUNS=1 ./ci/weather-probe.sh  # cheaper, weaker
#
# # Why this exists
#
# `crates/rustmc-server/src/weather.rs` is transcribed from
# `ServerLevel.advanceWeatherCycle`'s bytecode and gated by thirty unit tests
# and one wiring test. Every one of those compares this server against **our
# reading of the jar**. None of them has ever seen vanilla run.
#
# The corpus is where that comparison normally happens, and weather cannot
# reach it yet: the scenario needs a `do = "console"` step, which is on
# `origin/world-systems-2` and not on `main` (`docs/cold-start/weather.md`).
# This probe is the part of that comparison that can be taken **without** the
# runner change, without the corpus lock and without touching the corpus:
# vanilla is told `weather rain 300` by the same route `ci/dark-systems-probe.sh`
# uses for `gamerule` — a line written into `ci/vanilla-server.sh`'s console
# fifo from the shell — and a scenario with a single `wait` records what
# arrives.
#
# It answers exactly one question and it is the one the unit tests cannot:
# **is the packet sequence this port produces the sequence vanilla produces?**
#
# # What it does NOT do
#
# It records no subject side and compares nothing automatically. There is no
# verdict here and no ledger entry, because a scoring instrument built beside
# the thing it scores is this project's central defect and one is already
# specified: the scenario in `docs/cold-start/weather.md`, swept by
# `ci/corpus-falsifiable.py`, graded by `ci/t2-gameplay.sh`. This prints
# vanilla's observations and stops. Read them against
# `weather::tests::the_transition_tick_sends_five_packets_and_repeats_the_rain_level`
# and `tests/weather_console.rs`, both of which state the expected sequence in
# full.
#
# **So the honest description of this file is "an oracle reading", not "a
# check".** It cannot go red. It is run by hand, its output is quoted in a
# commit or a cold start, and nothing gates on it — which is why it is not in
# `ci/checks/required-checks.toml` and why `ci/t0.sh` does not call it.
#
# # It boots its own vanilla and touches nothing
#
# Its own port, its own world directory, its own throwaway corpus, the same
# substrate `ci/t2-gameplay.sh` boots. Same shape and same isolation as
# `ci/dark-systems-probe.sh`; it writes nothing into `ci/checks/gameplay/`,
# does not touch `gameplay-baseline.toml` and does not take the box-wide
# corpus lock.
#
# # Why three runs and not one
#
# `weather rain 300` passes an explicit duration, so `WeatherCommand.getDuration`
# takes its `ticks != DEFAULT_TIME` arm and **draws nothing** from
# `level.random` — the sequence should be identical across cold boots. Three
# runs is how that claim gets tested rather than assumed. If the three differ,
# the port has a reproducibility problem the corpus would have found later and
# more expensively.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${WEATHER_PROBE_PORT:-25917}"
RUNS="${WEATHER_RUNS:-3}"
WORK="$ROOT/oracle-work/gameplay/weather-probe"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"

# `measured_step = "silent"` because the measured step is a `wait`: the whole
# of what this observes is the settle window, which is the shape
# `ci/dark-systems-probe.sh`'s 903 and the shipped 780 both use.
#
# 3000 ms is about 60 ticks. `Level.isRaining()` is `getRainLevel(1.0f) > 0.2`
# and the level climbs 0.01f a tick from 0, so the crossing is at tick 20 and
# the window carries it with 40 ticks to spare on either side of nothing.
cat > "$SCEN/910-probe-weather-rain.toml" <<'EOF'
name = "probe-weather-rain"
about = "probe — what vanilla puts on the wire in the three seconds after `/weather rain 300` on the console"
observe = ["game_event"]
settle_ms = 3000
measured_step = "silent"

[[step]]
do = "wait"
ms = 100
EOF

# The control, and it is not optional. Without it "60 game_event packets
# arrived" could be read as "this substrate emits game_event packets", which
# it does — `LEVEL_CHUNKS_LOAD_START` (id 13) goes out at every join. The
# control runs the identical scenario with no `weather` line sent, so the
# difference between the two IS the command's effect.
cat > "$SCEN/911-probe-weather-control.toml" <<'EOF'
name = "probe-weather-control"
about = "probe control — the same window with no weather command sent, so the join-time game_event 13 can be subtracted"
observe = ["game_event"]
settle_ms = 3000
measured_step = "silent"

[[step]]
do = "wait"
ms = 100
EOF

cargo build -q -p oracle-runner --release
BIN="$ROOT/target/release/oracle-runner"

RECS=()
for run in $(seq 1 "$RUNS"); do
    echo
    echo "== run $run/$RUNS — cold vanilla on :$PORT"
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
    rm -rf "$VAN_DIR"
    VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
    # The corpus substrate, exactly: `advance_weather false` included, which
    # is the point. The level ramp at offset 325 is OUTSIDE that rule, so
    # `/weather rain` works under it — `docs/PARITY-ROADMAP.md` §107.1.
    for r in advance_time spawn_mobs random_tick_speed advance_weather; do
        case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
        "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
    done
    sleep 2

    # ---- the control first, on a dry world -------------------------------
    ctl="$WORK/control-r$run.json"
    "$BIN" record "127.0.0.1:$PORT" "$SCEN/911-probe-weather-control.toml" --out "$ctl"

    # ---- then the command, then the same window --------------------------
    #
    # An explicit duration so nothing is drawn from `level.random`
    # (`WeatherCommand.getDuration`'s `ticks != DEFAULT_TIME(-1)` arm), which
    # is what makes the three runs comparable at all.
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "weather rain 300"
    out="$WORK/r$run.json"
    "$BIN" record "127.0.0.1:$PORT" "$SCEN/910-probe-weather-rain.toml" --out "$out"

    # And put it back, for the same reason the scenario carries
    # `console_restore`: weather in 26.2 is ONE `WeatherData` for the whole
    # server, so a raining world outlives whatever set it raining.
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "weather clear"
    sleep 1
    grep -iE "commands.weather|Incorrect argument|Unknown or incomplete|permission" \
        "$VAN_DIR/logs/latest.log" | tail -6 || true
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"
    RECS+=("$ctl" "$out")
done

echo
python3 - "${RECS[@]}" <<'PY'
import collections
import json
import sys

# `ClientboundGameEventPacket$Type`'s ids, from that class's `<clinit>`.
NAMES = {
    1: "START_RAINING",
    2: "STOP_RAINING",
    7: "RAIN_LEVEL_CHANGE",
    8: "THUNDER_LEVEL_CHANGE",
    13: "LEVEL_CHUNKS_LOAD_START",
}

seqs = collections.defaultdict(list)
for path in sys.argv[1:]:
    doc = json.load(open(path))
    for s in doc["scenarios"]:
        name = s["name"]
        if s.get("error"):
            print(f"{path}: {name}: ERROR {s['error']}")
            continue
        obs = s["obs"]
        counts = collections.Counter(int(o["fields"]["event"]) for o in obs)
        line = ", ".join(
            f"{NAMES.get(k, k)}({k})x{v}" for k, v in sorted(counts.items())
        )
        print(f"\n=== {path.rsplit('/', 1)[-1]}  {name}  {len(obs)} obs")
        print(f"    {line or '(nothing)'}")
        # The first twelve in order, with their float payloads: the shape of
        # the transition is a sequence, not a histogram.
        for o in obs[:12]:
            ev = int(o["fields"]["event"])
            print(f"      {NAMES.get(ev, ev):24s} value={o['fields']['value']}")
        if len(obs) > 12:
            print(f"      ... {len(obs) - 12} more")
        seqs[name].append([int(o["fields"]["event"]) for o in obs])

print()
for name, runs in seqs.items():
    same = all(r == runs[0] for r in runs)
    print(
        f"{name}: {len(runs)} runs, event sequences "
        f"{'IDENTICAL' if same else 'DIFFER — the port has a reproducibility problem'}"
        f" (lengths {[len(r) for r in runs]})"
    )
PY
