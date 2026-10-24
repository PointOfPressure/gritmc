#!/usr/bin/env bash
# finish-dig-probe.sh — what does vanilla answer a `finish_dig` on this substrate?
#
#     ./ci/finish-dig-probe.sh
#
# `Step::FinishDig` sends `STOP_DESTROY_BLOCK`, the packet a survival client
# sends when its own mining progress reaches one. It is used by **zero** checks
# in the corpus and `./ci/corpus-coverage.py` prints that zero. Before writing
# the check that would close it, this measures whether such a check could say
# anything at all — because reading `ServerPlayerGameMode.handleBlockBreakAction`
# says it cannot, and a reading that clears every candidate is exactly the kind
# this project has been caught by ~20 times.
#
# The prediction being tested, from the bytecode:
#
#   * in creative, `START_DESTROY_BLOCK` takes the `instabuild` branch and
#     **returns before `destroyPos` is assigned** — that assignment is on the
#     survival path, at the "actual start of destroying" log line;
#   * `STOP_DESTROY_BLOCK` therefore fails `pos.equals(this.destroyPos)` and
#     falls to a `debugLogging` call, which sends nothing;
#   * the only packet is `ackBlockChangesUpTo(seq)`, which
#     `ServerGamePacketListenerImpl.handlePlayerAction` makes unconditionally
#     after the game-mode call returns — offsets 295 then 303, no branch.
#
# So the prediction is: **exactly one `block_changed_ack`, and no block
# change**. That is precisely what the scenario's own action forced, which is
# the definition of `inert`, so the gate would refuse a check built on it.
#
# # Measured 2026-08-04, vanilla 26.2, superflat creative, Threadripper
#
#     probe-finish-dig-alone   block_changed_ack seq=3      and NOTHING else
#     probe-dig-only           block_update [1,0,0] -> air, block_changed_ack
#
# The prediction holds. The stone the scenario placed is still standing after
# a `finish_dig`; the control removes it. `finish_dig` therefore cannot carry
# a check on this substrate, and closing that coverage zero needs a survival
# player rather than another scenario. Both orderings collapse to the same
# path: a preceding `START_DESTROY_BLOCK` does not help, because in creative
# it returns before `destroyPos` is assigned, so the `STOP` still mismatches.
#
# This probe writes nothing into the corpus and does not touch the baseline.
# It boots its own vanilla on its own port and records a throwaway scenario.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${FINISH_DIG_PORT:-25896}"
WORK="$ROOT/oracle-work/gameplay/finish-dig-probe"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"

# Two scenarios, so the answer is a comparison and not a single reading: the
# same block, left alone under a survival `STOP` and removed by a creative
# `START`. The control is what makes the first one's silence mean something —
# without it, a rig that never placed the block would look identical.
cat > "$SCEN/900-probe-finish-dig-alone.toml" <<'EOF'
name = "probe-finish-dig-alone"
about = "probe only"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]
settle_ms = 1500

[[setup]]
do = "hold"
item = "minecraft:stone"

[[setup]]
do = "place"
on = [1, -1, 0]
face = "up"

[[step]]
do = "finish_dig"
at = [1, 0, 0]
face = "up"
EOF

cat > "$SCEN/901-probe-dig-only.toml" <<'EOF'
name = "probe-dig-only"
about = "probe only — the control"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]
settle_ms = 1500

[[setup]]
do = "hold"
item = "minecraft:stone"

[[setup]]
do = "place"
on = [1, -1, 0]
face = "up"

[[step]]
do = "dig"
at = [1, 0, 0]
face = "up"
EOF

cargo build -q -p oracle-runner --release

echo "== booting vanilla on :$PORT"
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
for r in advance_time spawn_mobs random_tick_speed advance_weather; do
    case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
done
sleep 2

"$ROOT/target/release/oracle-runner" record "127.0.0.1:$PORT" "$SCEN" --out "$WORK/vanilla.json"

python3 - "$WORK/vanilla.json" <<'PY'
import json, sys, collections
d = json.load(open(sys.argv[1]))
for s in d["scenarios"]:
    obs = [o for o in s["obs"] if o["who"] == "actor"]
    c = collections.Counter(o["kind"] for o in obs)
    print(f"\n=== {s['name']}: {len(obs)} actor observations ===")
    for k, n in sorted(c.items()):
        print(f"    {k:26s} {n}")
    for o in obs:
        print(f"      {o['kind']:24s} {json.dumps(o['fields'])[:120]}")
PY
echo
echo "Read the two together. If probe-finish-dig-alone shows an ack and no block"
echo "change while probe-dig-only shows the block going, the prediction holds and"
echo "a finish_dig check is inert by construction on this substrate."
