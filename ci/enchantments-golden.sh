#!/usr/bin/env bash
# enchantments-golden.sh — the 43 enchantment registry entries, as data.
#
#     ./ci/enchantments-golden.sh [out.json]
#
# Produces `data/vanilla/enchantments.json`: registry order, every entry as
# the jar's own datapack writes it, every enchantment tag resolved in
# `TagLoader` order, the resolved `supported_items` /
# `primary_items` / `exclusive_set` of each entry, and the three item-side
# numbers the enchanting-table and anvil maths read (`ENCHANTABLE`,
# `MAX_DAMAGE`, `REPAIRABLE`). See `tools/goldengen/vanilla/EnchantmentGolden.java`
# for what each of those is for and where it is read.
#
# **A runtime extract, not a fixture**: `data/vanilla/`, gitignored,
# regenerated per tree by `ci/gen-gameplay-tables.sh`, stamped by
# `ci/provenance.sh`.
#
# # The cross-check is inside the dumper, and it is not a formality
#
# 26.2 carries the same 43 enchantments three times: `Enchantments.class`
# builds them in code, the jar bundles them as datapack JSON under
# `data/minecraft/enchantment/`, and Mojang's own data generator writes them
# again into `generated-data/`. The dumper reads all three — membership from
# the code registry, documents from the other two — and refuses if any one of
# the 43 disagrees after key-canonicalisation. **The counts are asserted
# beside the mismatch total**, because a reader that found no files reports no
# mismatches and looks perfect; that is this project's central defect class
# and it is cheap to close here.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/enchantments.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

TAGS="$VDIR/generated-data/data/minecraft/tags"
if [ ! -d "$TAGS/enchantment" ]; then
    echo "enchantments-golden: no datapack at $TAGS — running the generator" >&2
    "$ROOT/ci/vanilla-server.sh" data || { echo "enchantments-golden: ci/vanilla-server.sh data failed" >&2; exit 1; }
fi
[ -d "$TAGS/enchantment" ] || { echo "enchantments-golden: still no tags at $TAGS" >&2; exit 1; }

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
    "$ROOT/tools/goldengen/vanilla/EnchantmentGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -cp "$CP:$WORK" \
        -Drustmc.jar="$JAR" \
        -Drustmc.tags.dir="$TAGS" \
        -Drustmc.generated.dir="$VDIR/generated-data/data/minecraft/enchantment" \
        -Drustmc.enchantments.out="$1" \
        net.minecraft.world.level.levelgen.EnchantmentGolden 2>&1 | grep -E '^enchantments:|^  !!' || true
    test -s "$1"
}

# Twice, and they must agree byte for byte. Two `Files.walk` orders and two
# `ZipFile` enumerations feed this table, and a row order that moved between
# runs of the same jar would make every `enchantClue` id in it a coin flip.
# This is the check that says it does not rather than the assumption.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of EnchantmentGolden disagree — the registry order is not stable" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, sys

d = json.load(open(sys.argv[1]))
print(f"enchantments -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['count']} registry entries, {d['shipped_files']} bundled in the jar, "
      f"{d['generated_files']} written by the data generator, "
      f"{d['crosscheck_mismatches']} mismatches; "
      f"{len(d['tags'])} tags, {len(d['in_enchanting_table'])} in "
      f"#minecraft:in_enchanting_table, "
      f"enchantability on {len(d['enchantability'])} items")

fail = []
# The dumper exits 1 on a mismatch, so this is belt and braces — but a dumper
# that read zero files reports zero mismatches, which is the shape this
# project keeps paying for. Assert what was read, not only what disagreed.
if d["crosscheck_mismatches"] != 0:
    fail.append(f"{d['crosscheck_mismatches']} entries disagree across the three sources")
if not d["count"] == d["shipped_files"] == d["generated_files"]:
    fail.append(f"{d['count']} in code, {d['shipped_files']} bundled, {d['generated_files']} generated")
if d["count"] < 40:
    fail.append(f"only {d['count']} enchantments — the registry read has collapsed")
if len(d["order"]) != d["count"] or len(set(d["order"])) != d["count"]:
    fail.append("the order list is not a permutation of the registry")
# `RegistryDataLoader` walks a sorted resource listing, so a live server's
# `registry_data` packet carries these sorted — verified against a real
# capture in `crates/rustmc-server/tests/enchantment_registry_order.rs`. If
# the sort ever stops being applied here the table's `enchantClue` ids go
# wrong and nothing on the wire says so.
if d["order"] != sorted(d["order"]):
    fail.append("the order list is not sorted by identifier")
if len(d["enchantments"]) != d["count"]:
    fail.append(f"{len(d['enchantments'])} definitions for {d['count']} entries")
if not d["in_enchanting_table"]:
    fail.append("#minecraft:in_enchanting_table resolved empty — an enchanting table would offer nothing")
if set(d["in_enchanting_table"]) - set(d["order"]):
    fail.append("in_enchanting_table names an enchantment the registry does not hold")

# Four rows pinned against numbers read out of the jar's own JSON by hand
# (`unzip -p … data/minecraft/enchantment/<x>.json`), so the table is checked
# against something other than the process that produced it. Sharpness and
# protection are the two the enchanting maths is exercised on; mending is a
# treasure enchantment and must NOT be in the enchanting-table tag;
# unbreaking is the one with the widest supported set.
E = d["enchantments"]
for name, weight, maxlvl, anvil in (
    ("minecraft:sharpness", 10, 5, 1),
    ("minecraft:protection", 10, 4, 1),
    ("minecraft:mending", 2, 1, 4),
    ("minecraft:unbreaking", 5, 3, 2),
):
    e = E.get(name)
    if e is None:
        fail.append(f"{name} is absent")
        continue
    if e["weight"] != weight:
        fail.append(f"{name}: weight {e['weight']}, expected {weight}")
    if e["max_level"] != maxlvl:
        fail.append(f"{name}: max_level {e['max_level']}, expected {maxlvl}")
    if e["anvil_cost"] != anvil:
        fail.append(f"{name}: anvil_cost {e['anvil_cost']}, expected {anvil}")
if "minecraft:mending" in d["in_enchanting_table"]:
    fail.append("mending is in #in_enchanting_table — the treasure split has collapsed")
if "minecraft:sharpness" not in d["in_enchanting_table"]:
    fail.append("sharpness is not in #in_enchanting_table")

# The tag set, and the two properties the enchanting table's own behaviour
# rests on. `in_enchanting_table` is literally `["#minecraft:non_treasure"]`
# in the datapack, so the resolved lists must be equal; and treasure and
# non_treasure must partition the registry, because an entry in neither would
# be unreachable from every route and an entry in both would be offerable at
# a table while being marked treasure.
T = d["tags"]
for want in ("minecraft:in_enchanting_table", "minecraft:treasure",
             "minecraft:non_treasure", "minecraft:curse", "minecraft:tradeable",
             "minecraft:on_random_loot"):
    if want not in T:
        fail.append(f"{want} is not in the resolved tag set")
if T.get("minecraft:in_enchanting_table") != d["in_enchanting_table"]:
    fail.append("the in_enchanting_table tag and the top-level list disagree")
if T.get("minecraft:in_enchanting_table") != T.get("minecraft:non_treasure"):
    fail.append("in_enchanting_table is not exactly #non_treasure")
treasure = set(T.get("minecraft:treasure", []))
non = set(T.get("minecraft:non_treasure", []))
if treasure & non:
    fail.append(f"{len(treasure & non)} enchantments are both treasure and non_treasure")
if treasure | non != set(d["order"]):
    missing = set(d["order"]) - (treasure | non)
    fail.append(f"treasure and non_treasure do not cover the registry; {len(missing)} missing")
for name, members in T.items():
    stray = set(members) - set(d["order"])
    if stray:
        fail.append(f"{name} names {len(stray)} ids the registry does not hold")

# Sharpness and smite exclude each other. If exclusive_set resolution has
# silently produced empty lists, `selectEnchantment` would hand out both.
R = d["resolved"]
if "minecraft:smite" not in R.get("minecraft:sharpness", {}).get("exclusive_set", []):
    fail.append("sharpness's exclusive_set does not hold smite — tag resolution has collapsed")
if "minecraft:diamond_sword" not in R.get("minecraft:sharpness", {}).get("supported_items", []):
    fail.append("sharpness does not support a diamond sword — item tag resolution has collapsed")

if d["enchantability"].get("minecraft:diamond_sword") != 10:
    fail.append(f"diamond_sword enchantability {d['enchantability'].get('minecraft:diamond_sword')}, expected 10")
if d["max_damage"].get("minecraft:diamond_sword") != 1561:
    fail.append(f"diamond_sword max_damage {d['max_damage'].get('minecraft:diamond_sword')}, expected 1561")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
