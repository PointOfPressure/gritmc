#!/usr/bin/env bash
# path-facts-golden.sh — the per-STATE block facts a mob's pathfinder,
# navigation and movement tick read.
#
#     ./ci/path-facts-golden.sh [out.json]
#
# Produces `data/vanilla/path_facts.json`: for every global block state,
# `isPathfindable(LAND/WATER/AIR)`, `blocksMotion`, the fluid's kind, amount
# and own height, `Block.getJumpFactor()`, the ten block tags the AI tests
# and the door / fence-gate / bed / leaves / rail / lit-campfire facts. See
# `tools/goldengen/vanilla/PathFactsGolden.java` for where each is read.
# `crates/rustmc-entity/src/host.rs::BlockFacts` is the consumer.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/path_facts.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

UNSIGNED="$VDIR/versions/$VER/server-$VER-unsigned.jar"
if [ ! -s "$UNSIGNED" ] || [ "$JAR" -nt "$UNSIGNED" ]; then
    command -v zip >/dev/null || { echo "need 'zip' to strip jar signatures" >&2; exit 1; }
    cp -f "$JAR" "$UNSIGNED"
    for f in $(unzip -l "$UNSIGNED" | grep -oE 'META-INF/[A-Za-z0-9_]+\.(SF|RSA|DSA)' || true); do
        zip -q -d "$UNSIGNED" "$f"
    done
fi

CP="$UNSIGNED:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$(dirname "$OUT")"
javac -nowarn -cp "$CP" -d "$WORK" \
    "$ROOT/tools/goldengen/vanilla/PathFactsGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.PathFactsGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of PathFactsGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, struct, sys

d = json.load(open(sys.argv[1]))
rows = d["rows"]
sr = d["stateRows"]
print(f"path facts -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['states']} states in {len(rows)} distinct rows; "
      f"{d['pathLandStates']} walkable, {d['fluidStates']} fluid")

fail = []
if len(sr) != d["states"]:
    fail.append(f"{len(sr)} state rows for {d['states']} states")
if any(i < 0 or i >= len(rows) for i in sr):
    fail.append("a stateRows entry points outside rows")

blocks = d["blocks"]
def rows_of(name):
    b = blocks.index(name)
    return [r for r in rows if r["block"] == b]

# Named expectations, one per column class, so a table right in aggregate
# and wrong on the interesting rows still fails. Every one of these is a
# fact the pig's first tick reads or the pathfinder's node types test.
checks = [
    ("minecraft:air", "pathLand", True), ("minecraft:stone", "pathLand", False),
    ("minecraft:water", "pathWater", True), ("minecraft:stone", "pathWater", False),
    ("minecraft:oak_fence", "tag_fences", True), ("minecraft:cobblestone_wall", "tag_walls", True),
    ("minecraft:oak_trapdoor", "tag_trapdoors", True), ("minecraft:fire", "tag_fire", True),
    ("minecraft:red_bed", "tag_beds", True), ("minecraft:short_grass", "tag_edible_for_sheep", True),
    ("minecraft:grass_block", "tag_edible_for_sheep", False),
    ("minecraft:oak_door", "tag_mob_interactable_doors", True),
    ("minecraft:iron_door", "tag_mob_interactable_doors", False),
    ("minecraft:oak_leaves", "leaves", True), ("minecraft:rail", "rail", True),
    ("minecraft:stone", "blocksMotion", True), ("minecraft:air", "blocksMotion", False),
    ("minecraft:pointed_dripstone", "tag_speleothems", True),
]
for name, field, want in checks:
    rs = rows_of(name)
    if not rs:
        fail.append(f"no row for {name}")
        continue
    if not any(r[field] == want for r in rs):
        fail.append(f"{name}.{field}: no state is {want}")

# The two door columns exist exactly on door blocks, and both values of OPEN
# occur, or the property read is dead.
door = rows_of("minecraft:oak_door")
if {r.get("doorOpen") for r in door} != {True, False}:
    fail.append("oak_door has no open/closed split")
if not all(r.get("doorByHand") is True for r in door):
    fail.append("oak_door is not openable by hand")
iron = rows_of("minecraft:iron_door")
if not all(r.get("doorByHand") is False for r in iron):
    fail.append("iron_door is openable by hand")
if any("doorOpen" in r for r in rows_of("minecraft:stone")):
    fail.append("stone carries a door column")

# Fluids: a water source is amount 8 and its own height 8/9
# (`FlowingFluid.getOwnHeight` = amount / 9).
water = rows_of("minecraft:water")
amounts = sorted({r["fluidAmount"] for r in water})
if 8 not in amounts or not any(r["fluidSource"] for r in water):
    fail.append(f"water rows carry no source (amounts {amounts})")
f32 = lambda v: struct.unpack("<f", struct.pack("<f", v))[0]
for r in water:
    if r["fluidSource"]:
        bits = r["fluidOwnHeightBits"] & 0xFFFFFFFF
        if struct.unpack("<f", struct.pack("<I", bits))[0] != f32(8.0 / 9.0):
            fail.append("a water source's own height is not 8/9")
        break
lava = rows_of("minecraft:lava")
if not lava or any(r["fluidKind"] != 2 for r in lava):
    fail.append("lava rows are not fluid kind 2")

# Jump factor: honey is the one block that is not 1.0.
honey = rows_of("minecraft:honey_block")
if not honey or f32(honey[0]["jumpFactor"]) != f32(0.5):
    fail.append("honey_block jump factor is not 0.5")
if f32(rows_of("minecraft:stone")[0]["jumpFactor"]) != 1.0:
    fail.append("stone jump factor is not 1.0")

if fail:
    print("path facts: REFUSED", file=sys.stderr)
    for f in fail:
        print("  " + f, file=sys.stderr)
    sys.exit(1)
PYEOF
