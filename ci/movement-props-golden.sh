#!/usr/bin/env bash
# movement-props-golden.sh — the per-block floats the movement tick reads.
#
#     ./ci/movement-props-golden.sh [out.json]
#
# Produces `data/vanilla/movement_props.json`: `friction`, `speedFactor` and
# `bounceRestitution` for every block. See
# `tools/goldengen/vanilla/MovementPropsGolden.java` for where each is read.
# `block_props.json` cannot carry them — it is keyed by block *state* and
# these are `Block` fields — and there is no rule connecting them to a block's
# name, so they are data or they are wrong.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`. A stale one silently changes how far a dropped item
# slides.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/movement_props.json}"

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
    "$ROOT/tools/goldengen/vanilla/MovementPropsGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.MovementPropsGolden 2>/dev/null > "$1"
    test -s "$1"
}

run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of MovementPropsGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, struct, sys

d = json.load(open(sys.argv[1]))
print(f"movement props -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['blocks']} blocks over {d['states']} states; "
      f"{d['nonDefaultFriction']} not friction 0.6, "
      f"{d['nonDefaultSpeedFactor']} not speed 1.0, "
      f"{d['nonZeroBounceRestitution']} bouncy, "
      f"{d['fenceTagBlocks']} in #fences, {d['wallTagBlocks']} in #walls")

by = {r["block"]: r for r in d["table"]}
fail = []
if len(d["table"]) != d["blocks"]:
    fail.append(f"{len(d['table'])} rows for {d['blocks']} blocks")
if len(d["stateRows"]) != d["states"]:
    fail.append(f"{len(d['stateRows'])} state rows for {d['states']} states")
# Unlike a collision shape, every block state has these three floats. A `-1`
# here can only mean the dumper failed to map one, and a reader that treated
# it as "default" would give that block frictionless ground.
if d["unmappedStates"] != 0:
    fail.append(f"{d['unmappedStates']} states map to no block row")

# A table in which every block is the default is a table that cannot be
# telling the truth: ice and soul sand exist, and a dumper that read a
# not-yet-constructed field would give 0.6/1.0/0.0 for everything and look
# exactly like a working one.
if d["nonDefaultFriction"] == 0:
    fail.append("no block has a non-default friction — the dumper is reading nothing")
if d["nonDefaultSpeedFactor"] == 0:
    fail.append("no block has a non-default speed factor — the dumper is reading nothing")
if d["nonZeroBounceRestitution"] == 0:
    fail.append("no block bounces — the dumper is reading nothing")

for name, field, want in (
    # The default, on the block the whole gameplay corpus stands on.
    ("minecraft:stone", "friction", 0.6),
    ("minecraft:stone", "speedFactor", 1.0),
    ("minecraft:stone", "bounceRestitution", 0.0),
    # Three that must not be the default, named individually so that a table
    # right in aggregate and wrong on the interesting rows still fails.
    ("minecraft:ice", "friction", 0.98),
    ("minecraft:soul_sand", "speedFactor", 0.4),
    # **1.0, not 0.8, and the first run of this generator corrected the
    # expectation rather than the data.** 0.8 is the factor
    # `Entity.getBlockBounciness` applies to a non-`LivingEntity`, not the
    # block's own restitution; `SlimeBlock`'s is 1.0. The two numbers sit one
    # multiply apart and this assertion had them conflated.
    ("minecraft:slime_block", "bounceRestitution", 1.0),
):
    row = by.get(name)
    if row is None:
        fail.append(f"no row for {name}")
        continue
    got = row[field]
    # **Both sides through a float32 round trip.** These are Java `float`s
    # printed by `Float.toString`, so the JSON text is the shortest string that
    # round-trips *as a float*; Python parses it as a double, and
    # `0.6 != float32(0.6)` by 2.4e-8. The first run of this generator failed
    # three of these rows for exactly that reason, in the gate rather than in
    # the data. Comparing the doubles is the bug; comparing the floats is the
    # question being asked.
    got32 = struct.unpack("<f", struct.pack("<f", got))[0]
    if got32 != struct.unpack("<f", struct.pack("<f", want))[0]:
        fail.append(f"{name}.{field} is {got}, expected {want}")
    bits = row[field + "Bits"] & 0xFFFFFFFF
    if struct.unpack("<f", struct.pack("<I", bits))[0] != got32:
        fail.append(f"{name}.{field}: bits and value disagree")

# `fence` and `wallOrGate` are read from BlockTags.FENCES and BlockTags.WALLS,
# and **a tag is only bound after the server's reload**, not after
# Bootstrap.bootStrap(). If the binding has not happened this dumper emits
# `false` for every block and looks exactly like a working one, so the blocks
# that must be true are named here. This is the assertion that turned "the tags
# were empty" from a silent wrong table into a failed generator, on this
# generator's very first run.
#
# **The two columns are separate and must stay separate**: `Entity.getOnPos`
# guards the fence arm with `offset <= 0.5` and tests walls and gates
# unconditionally, and `Entity.move` calls it at 0.2 and at 0.500001 — one
# either side. A fence that answered `true` to the wall predicate would behave
# identically at both offsets, which is wrong at both.
for name, field, want in (
    ("minecraft:oak_fence", "fence", True),
    ("minecraft:oak_fence", "wallOrGate", False),
    ("minecraft:cobblestone_wall", "wallOrGate", True),
    ("minecraft:cobblestone_wall", "fence", False),
    ("minecraft:oak_fence_gate", "wallOrGate", True),
    ("minecraft:oak_fence_gate", "fence", False),
    ("minecraft:stone", "fence", False),
    ("minecraft:stone", "wallOrGate", False),
    ("minecraft:water", "waterlike", True),
    ("minecraft:bubble_column", "waterlike", True),
    ("minecraft:stone", "waterlike", False),
):
    row = by.get(name)
    if row is None:
        fail.append(f"no row for {name}")
    elif row[field] != want:
        fail.append(f"{name}.{field} is {row[field]}, expected {want} "
                    "(tags unbound? see the comment above this check)")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
