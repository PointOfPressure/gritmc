#!/usr/bin/env bash
# dark-systems-probe.sh — can the three game-rule-shut systems be observed at all?
#
#     ./ci/dark-systems-probe.sh              # 6 cold vanilla runs
#     DARK_RUNS=2 ./ci/dark-systems-probe.sh  # cheaper, weaker
#
# `docs/PARITY-ROADMAP.md` §21.2 measures eight of vanilla's 120 per-tick
# profiler sections as **provably never run during the corpus**, and the proof
# is three game rules `ci/t2-gameplay.sh` sets itself:
#
#     spawn_mobs         false     6 sections
#     random_tick_speed  0         1 section
#     advance_weather    false     1 section
#
# Those three are the cheapest rows on §21.5's work order because they are
# ours, and the cheapness is exactly what makes them dangerous: the rules were
# set to make the corpus reproducible, so turning them on is a way to destroy
# the 4/92 floor without noticing. This probe answers, per system and before
# anything is added to the corpus, the only question that decides whether the
# coverage is real: **can it be observed reproducibly at all?**
#
# # It boots its own vanilla and touches nothing
#
# Its own port, its own world directory, its own throwaway corpus. It writes
# nothing into `ci/checks/gameplay/` — it *copies* the one candidate that
# graduated out of it, so what the floor is measured over is byte-identical to
# what ships — and it does not touch `gameplay-baseline.toml`, the standing
# floor, or the box-wide corpus lock. Same shape as `ci/finish-dig-probe.sh`.
#
# # What it prints
#
# Per scenario, per run: every observation, so a null can be read as a null
# rather than as a missing instrument. Then `ci/corpus-floor.sh` over the N
# recordings, scoped with `--checks` to this probe's corpus, which is the
# floor for these scenarios and nobody else's. `--no-adjudicate` because
# `ci/checks/gameplay-unstable.toml` is a statement about the 92 and these are
# not in it.
#
# # The three questions, and the controls that make an answer mean something
#
# 900/901 are a pair. One covers a grass block and one does not, under the
# same random tick rate for the same time: without 901 a `block_update` in 900
# could be anything at all that a busy random tick did, and with it the check
# is about the cover.
#
# 902 is the "is it already there" control. `Step::Command` exists, so the
# obvious mechanism is a scenario that sends `gamerule` as a chat command. The
# harness's players are `oracle` and `oracle_eye` and nothing in
# `ci/vanilla-server.sh` writes an `ops.json`, so this should come back as a
# refusal in chat and a rule that did not move — and 900 running beside it is
# what proves the *console* route did move one.
#
# 903 and 904 are the two expected nulls, run rather than argued.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${DARK_PROBE_PORT:-25914}"
RUNS="${DARK_RUNS:-6}"
WORK="$ROOT/oracle-work/gameplay/dark-probe"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"

# The shipped candidate, copied rather than re-typed: a floor taken over a
# paraphrase of a check is a floor for the paraphrase.
cp "$ROOT/ci/checks/gameplay/780-random-ticks-turn-covered-grass-to-dirt.toml" \
   "$SCEN/900-probe-random-tick-covered.toml"

cat > "$SCEN/901-probe-random-tick-uncovered.toml" <<'EOF'
name = "probe-random-tick-uncovered"
about = "probe control — the same random tick rate for the same time over grass nobody covered"
observe = ["block_update", "section_blocks_update"]
settle_ms = 4000
measured_step = "silent"

[[gamerule]]
rule = "random_tick_speed"
value = "1000"
restore = "0"

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[step]]
do = "wait"
ms = 100
EOF

cat > "$SCEN/902-probe-gamerule-as-a-chat-command.toml" <<'EOF'
name = "probe-gamerule-as-a-chat-command"
about = "probe — can an unopped harness player set a game rule with Step::Command"
observe_from = "actor"
observe = ["chat"]
settle_ms = 1500

[[step]]
do = "command"
text = "gamerule random_tick_speed 1000"
EOF

cat > "$SCEN/903-probe-advance-weather.toml" <<'EOF'
name = "probe-advance-weather"
about = "probe — does turning advance_weather on produce anything on the wire inside a window this harness can afford"
observe = ["game_event", "block_update", "section_blocks_update", "sound"]
settle_ms = 8000
measured_step = "silent"

[[gamerule]]
rule = "advance_weather"
value = "true"
restore = "false"

[[step]]
do = "wait"
ms = 100
EOF

cat > "$SCEN/904-probe-spawn-mobs.toml" <<'EOF'
name = "probe-spawn-mobs"
about = "probe — does turning spawn_mobs on put an entity in the world inside a window this harness can afford"
observe = ["add_entity", "set_entity_motion", "entity_event"]
settle_ms = 8000
within = -1
measured_step = "silent"

[[gamerule]]
rule = "spawn_mobs"
value = "true"
restore = "false"

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
    # The same substrate `ci/t2-gameplay.sh` boots, because a floor taken on a
    # different world is a floor for a different check.
    VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
        "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
    for r in advance_time spawn_mobs random_tick_speed advance_weather; do
        case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
        "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
    done
    sleep 2
    out="$WORK/r$run.json"
    "$BIN" record "127.0.0.1:$PORT" "$SCEN" --out "$out" \
        --console "$VAN_DIR/console.fifo"
    # **The rule really did go back.** A restore that silently failed would
    # leave the reference random-ticking for the rest of the run, and the next
    # thing to notice would be the 92 going red on somebody else's branch.
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule random_tick_speed"
    sleep 1
    tail -40 "$VAN_DIR/logs/latest.log" | grep -iE "random_tick_speed|advance_weather|spawn_mobs|Incorrect argument|permission" \
        | tail -8 || true
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR"
    RECS+=("$out")
done

echo
python3 - "${RECS[@]}" <<'PY'
import json, sys, collections
runs = [json.load(open(p)) for p in sys.argv[1:]]
names = [s["name"] for s in runs[0]["scenarios"]]
for n in names:
    print(f"\n=== {n} ===")
    for i, r in enumerate(runs, 1):
        s = next(x for x in r["scenarios"] if x["name"] == n)
        if s.get("error"):
            print(f"  r{i}: ERROR {s['error']}")
            continue
        c = collections.Counter(o["kind"] for o in s["obs"])
        summary = ", ".join(f"{k}x{v}" for k, v in sorted(c.items())) or "(nothing)"
        print(f"  r{i}: {len(s['obs']):3d} obs  {summary}")
        for o in s["obs"]:
            print(f"        {o['who']:8s} {o['kind']:22s} {json.dumps(o['fields'])[:150]}")
PY

echo
echo "== floor over $RUNS runs, this probe's corpus only"
"$ROOT/ci/corpus-floor.sh" --checks "$SCEN" --no-adjudicate \
    --id "dark-probe-n$RUNS" --out "$WORK/floor" "${RECS[@]}"
