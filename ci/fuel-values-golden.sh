#!/usr/bin/env bash
# fuel-values-golden.sh — `FuelValues.vanillaBurnTimes`, item by item.
#
#     ./ci/fuel-values-golden.sh [out.json]
#
# Produces `data/vanilla/fuel_values.json`: every item the vanilla fuel map
# contains, with its burn duration in ticks and its `isFuel` answer. See
# `tools/goldengen/vanilla/FuelValuesGolden.java` for the three call sites
# that read it and for why an absent item and an item mapped to zero are
# different answers.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`. A stale one makes a furnace refuse coal, which reads in
# a log exactly like a furnace with no recipe.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/fuel_values.json}"

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
    "$ROOT/tools/goldengen/vanilla/FuelValuesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" \
        -Drustmc.tags.dir="$VDIR/generated-data/data/minecraft/tags" \
        -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.FuelValuesGolden 2>"$WORK/err" > "$1"
    # The stderr goes to a file rather than to /dev/null: the whole table is
    # built inside one `vanillaBurnTimes` call, so the failure mode is a
    # stack trace and an empty file, and swallowing it leaves a `set -e` exit
    # with nothing said about why.
    test -s "$1" || { sed -n "1,20p" "$WORK/err" >&2; return 1; }
}

# Twice, and they must agree. `fuelItems()` is a SequencedSet over a sorted
# map, so the order should be stable — this is the check that says so rather
# than the assumption that it does.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of FuelValuesGolden disagree — the item order is not stable" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"fuel values -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['fuels']} fuel items, {d['zeroDurationFuels']} of them at duration 0")

fail = []
table = d["table"]
if len(table) != d["fuels"]:
    fail.append(f"{len(table)} rows for {d['fuels']} fuels")

# THE CONTROL THAT MAKES THE REST MEAN ANYTHING. `Object2IntSortedMap.getInt`
# answers 0 for an absent key, so a dumper that had lost its map entirely
# would emit a plausible-looking file of zeroes. Stone is not a fuel and
# answers 0/false; anything in this table answering 0/true would be a real
# zero-duration fuel and is counted above rather than silently dropped.
if d["stoneDuration"] != 0 or d["stoneIsFuel"]:
    fail.append(f"stone reads {d['stoneDuration']}/{d['stoneIsFuel']}, expected 0/False")
if d["emptyDuration"] != 0 or d["emptyIsFuel"]:
    fail.append(f"an empty stack reads {d['emptyDuration']}/{d['emptyIsFuel']}, expected 0/False")

# A map that collapsed to one value distinguishes nothing, which is this
# project's central defect class. Fuels really do span three orders of
# magnitude, from a stick to a lava bucket.
durations = {r["burnDuration"] for r in table}
if len(durations) < 5:
    fail.append(f"only {len(durations)} distinct burn durations — the map has collapsed")
if d["fuels"] < 50:
    fail.append(f"only {d['fuels']} fuels — the tag expansion did not run")

# Four rows pinned against numbers taken from somewhere other than this
# dumper: `AbstractFurnaceBlockEntity`'s own arithmetic is what makes them
# checkable. A furnace smelts eight items per coal at 200 ticks each, which
# is 1600; a lava bucket is twenty thousand ticks, a hundred items; a plank
# is a third of a coal-item and a stick a sixth.
want = {
    "minecraft:coal": 1600,
    "minecraft:lava_bucket": 20000,
    "minecraft:oak_planks": 300,
    "minecraft:stick": 100,
}
have = {r["item"]: r["burnDuration"] for r in table}
for item, ticks in want.items():
    if item not in have:
        fail.append(f"{item} is not in the fuel table at all")
    elif have[item] != ticks:
        fail.append(f"{item} burns {have[item]} ticks, expected {ticks}")

# `fuelItems()` and the map it is a view of must agree. A non-zero here means
# the dumper read one object and reported another.
if d["fuelItemsNotInMap"] != 0:
    fail.append(f"{d['fuelItemsNotInMap']} items are in fuelItems() and not in the map")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
