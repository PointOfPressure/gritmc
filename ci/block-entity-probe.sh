#!/usr/bin/env bash
# block-entity-probe.sh — which block entity types can this corpus ever observe?
#
#     ./ci/block-entity-probe.sh
#
# `docs/PARITY-ROADMAP.md` §21 ranks block entities fourth: **1 of 49
# `minecraft:block_entity_type`s has ever been observed**, and that one is
# `sign`. The natural reading is "forty-eight scenarios to write". It is
# wrong, and this probe exists because the reason it is wrong is a fact about
# vanilla rather than about us.
#
# `ci/vanilla-reach-census.py` counts a block entity type as touched when a
# `block_entity_data` observation names it — that is its only channel, because
# a block entity's other route to a client is inside the chunk payload and
# `trace::FAMILIES` records the chunk payload with no decoder arm, so its block
# entities stay inside one opaque `rest` digest and no decoder sees them.
# And `block_entity_data` is sent from exactly one place:
#
#     ChunkHolder.broadcastChanges
#       -> broadcastBlockEntityIfNeeded   if state.hasBlockEntity()
#         -> broadcastBlockEntity         be.getUpdatePacket(), and
#                                         **ifnull 31: return** if it is null
#
# `BlockEntity.getUpdatePacket()` is `aconst_null; areturn`. Twenty-nine of
# the forty-nine types inherit it unchanged, so vanilla never sends
# `block_entity_data` for a chest, a furnace, a hopper or a barrel, however
# they change. **The census's block-entity row has a ceiling of 20/49**, and
# the twenty are the classes that override the method — all twenty
# unconditionally, `ClientboundBlockEntityDataPacket.create(this); areturn`.
#
# That is a reading of bytecode, which by ORACLE rule 9 moves no suspects on
# its own. This probe is the measurement: it boots its own vanilla, places one
# of each candidate, and prints what came back — including the exact NBT bytes
# under `RUSTMC_TRACE_RAW`, so the payload this server has to reproduce is
# vanilla's own output and not a transcription of anything.
#
# **The negative controls are the point of the shape.** Four blocks whose
# types inherit the null are placed by the same rig in the same run. If they
# are silent and the twenty are not, the silence is the type's. If everything
# is silent, the rig is broken and nothing here means anything — which is the
# failure this project keeps meeting from the other side.
#
# This probe writes nothing into the corpus, touches no baseline, and takes no
# corpus lock: it boots its own vanilla on its own port, exactly as
# `ci/finish-dig-probe.sh` does.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PORT="${BE_PROBE_PORT:-25898}"
WORK="$ROOT/oracle-work/gameplay/block-entity-probe"
VAN_DIR="$WORK/vanilla"
SCEN="$WORK/checks"
SEED="${GAMEPLAY_SEED:-20260803}"

cleanup() {
    "$ROOT/ci/vanilla-server.sh" stop "$VAN_DIR" >/dev/null 2>&1 || true
}
trap cleanup EXIT

rm -rf "$WORK"
mkdir -p "$SCEN"

n=100

# gen <item> [<face>] — one scenario that holds an item and puts it on the
# ground beside the player. The observation set is the same for every one of
# them, so a type that says nothing is saying it under the same projection as
# a type that does.
gen() {
    local item="$1" face="${2:-up}" on="${3:-1, -1, 0}"
    n=$((n + 1))
    cat > "$SCEN/${n}-probe-be-${item#minecraft:}.toml" <<EOF
name = "probe-be-${item#minecraft:}"
about = "probe only — place ${item} and see whether vanilla sends block_entity_data"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_entity_data"]
settle_ms = 1200

[[setup]]
do = "hold"
item = "${item}"

[[step]]
do = "place"
on = [${on}]
face = "${face}"
EOF
}

# The twenty types whose class overrides getUpdatePacket(), less the two that
# no item can place — `end_portal` and `end_gateway` have no item form at all.
for i in spawner creaking_heart beacon skeleton_skull white_banner \
         structure_block conduit jigsaw campfire oak_shelf suspicious_sand \
         decorated_pot trial_spawner vault test_block test_instance_block \
         copper_golem_statue oak_sign; do
    gen "minecraft:$i"
done

# The hanging sign needs something to hang from, so it gets its own rig rather
# than the shared one. Kept on +z: relative z >= 0 is chunk z 0 and z <= -1 is
# chunk z -1, and a rig spanning the two measures vanilla's inter-chunk send
# order (ci/checks/gameplay/README.md).
n=$((n + 1))
cat > "$SCEN/${n}-probe-be-oak_hanging_sign.toml" <<'EOF'
name = "probe-be-oak_hanging_sign"
about = "probe only — a hanging sign needs a wall, so it gets one"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_entity_data"]
settle_ms = 1200

[[setup]]
do = "hold"
item = "minecraft:stone"

[[setup]]
do = "place"
on = [1, -1, 1]
face = "up"

[[setup]]
do = "hold"
item = "minecraft:oak_hanging_sign"

[[step]]
do = "place"
on = [1, 0, 1]
face = "south"
EOF

# The negative controls. All four have a block entity — `hasBlockEntity()` is
# true, so `broadcastBlockEntityIfNeeded` calls through — and all four inherit
# the null `getUpdatePacket()`. Vanilla should place the block and say nothing
# else about it.
for i in chest barrel hopper lectern; do
    gen "minecraft:$i"
done

cargo build -q -p oracle-runner --release

echo "== booting vanilla on :$PORT"
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
    "$ROOT/ci/vanilla-server.sh" start "$VAN_DIR" "$PORT" "minecraft:flat" "$SEED"
for r in advance_time spawn_mobs random_tick_speed advance_weather; do
    case $r in random_tick_speed) v=0 ;; *) v=false ;; esac
    "$ROOT/ci/vanilla-server.sh" console "$VAN_DIR" "gamerule $r $v"
done
sleep 2

RUSTMC_TRACE_RAW=block_entity_data \
    "$ROOT/target/release/oracle-runner" record "127.0.0.1:$PORT" "$SCEN" \
    --out "$WORK/vanilla.json" 2> "$WORK/raw.log"

echo
echo "== raw block_entity_data tails, in the order they were recorded =="
grep '^raw block_entity_data' "$WORK/raw.log" || echo "  (none)"

python3 - "$WORK/vanilla.json" <<'PY'
import json, sys, collections
d = json.load(open(sys.argv[1]))
print(f"\n{'scenario':40s} {'be_data':>7} {'type':>5}  blocks placed")
print("-" * 78)
for s in d["scenarios"]:
    obs = [o for o in s["obs"] if o["who"] == "actor"]
    bed = [o for o in obs if o["kind"] == "block_entity_data"]
    states = []
    for o in obs:
        if o["kind"] == "block_update":
            states.append(o["fields"].get("state", "?"))
        elif o["kind"] == "section_blocks_update":
            states += [b.get("state", "?") for b in o["fields"].get("blocks", ())]
    ty = ",".join(str(o["fields"].get("block_entity_type")) for o in bed) or "-"
    short = [x.split("[")[0].removeprefix("minecraft:") for x in states]
    err = f"  ERROR {s['error']}" if s.get("error") else ""
    print(f"{s['name']:40s} {len(bed):>7} {ty:>5}  {','.join(short) or '(nothing)'}{err}")
    for o in bed:
        print(f"{'':40s}         at={o['fields'].get('at')} rest={o['fields'].get('rest')}")
PY
echo
echo "Read the four controls (chest, barrel, hopper, lectern) first. If they"
echo "placed a block and sent no block_entity_data while the others did, the"
echo "20/49 ceiling is measured and not merely read off the bytecode."
