#!/usr/bin/env bash
# damage-types-golden.sh — dump the `minecraft:damage_type` registry and every
# `DamageTypeTags` membership from the jar.
#
#     ./ci/damage-types-golden.sh [out.json]
#
# Produces `data/vanilla/damage_types.json`. See
# `tools/goldengen/vanilla/DamageTypesGolden.java` for why this one reads the
# jar's own data files rather than a `BuiltInRegistries` walk (damage types are
# a datapack registry in 26.2) and for the independent cross-check that stops a
# missed file reading as a smaller registry.
#
# **This is a runtime extract, not a fixture.** It goes under `data/vanilla/`,
# it is gitignored, it is regenerated per tree by `ci/gen-gameplay-tables.sh`
# and stamped by `ci/provenance.sh` — the same distinction
# `ci/attributes-golden.sh` documents.
#
# Same signature-stripping as `ci/attributes-golden.sh`, for the same reason:
# the dumper lives inside vanilla's own package and the JVM refuses to mix an
# unsigned class into a signed one.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/damage_types.json}"

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
    "$ROOT/tools/goldengen/vanilla/DamageTypesGolden.java" 2>&1 | grep -v '^Note' || true

run() {
    java -Xmx2G -Drustmc.data="$VDIR/generated-data" -Drustmc.jar="$UNSIGNED" -cp "$CP:$WORK" \
        net.minecraft.world.level.levelgen.DamageTypesGolden 2>/dev/null > "$1"
    test -s "$1"
}

# Twice, and they must agree. Nothing here draws from a random source, so a
# disagreement means the dump is reading something ambient — a registry
# iteration order that is not stable, or a modifier map whose iteration order
# moves — and a table whose row order moves is a table whose diff is unreadable.
run "$WORK/out.json"
run "$WORK/again.json"
cmp -s "$WORK/out.json" "$WORK/again.json" || {
    echo "two runs of DamageTypesGolden disagree — it is reading something ambient" >&2
    exit 1
}

mv "$WORK/out.json" "$OUT"
python3 - "$OUT" <<'PYEOF'
import json, struct, sys

d = json.load(open(sys.argv[1]))
print(f"damage types -> {sys.argv[1]} (identical across two runs)")
print(f"  {d['damageTypes']} damage types ({d['declaredInCode']} keys declared in "
      f"DamageTypes), {d['tags']} tags")

fail = []
if len(d["table"]) != d["damageTypes"]:
    fail.append(f"{len(d['table'])} rows for {d['damageTypes']} types")
if d["damageTypes"] < d["declaredInCode"]:
    fail.append("fewer types than the code declares — the jar walk missed files")

by_id = {r["id"]: r for r in d["table"]}
tags = {t["tag"]: set(t["values"]) for t in d["tagTable"]}

# The ten memberships crates/rustmc-server/src/health.rs and crate::hurt read.
# A tag that resolved empty would make the corresponding DamageTags field
# false for every type, which is a silent behaviour change and not an error.
NEEDED = ["bypasses_armor", "bypasses_effects", "bypasses_resistance",
          "bypasses_enchantments", "bypasses_cooldown", "is_fire", "is_freezing",
          "damages_helmet", "no_impact", "no_knockback"]
empty_needed = []
for t in NEEDED:
    key = "minecraft:" + t
    if key not in tags or not tags[key]:
        empty_needed.append(key)
print(f"  {len(NEEDED) - len(empty_needed)} of the 10 tags the hurt path reads are non-empty")

# `DamageTypeTags` declares TagKeys with no data file, and a TagKey with no
# file is an EMPTY tag: every `is()` against it is false and the branch it
# guards is dead. Pinned rather than tolerated, so that a later version adding
# the file reds this check and somebody reads the branch again.
NO_FILE_EXPECTED = ["minecraft:bypasses_cooldown"]
if sorted(d["tagsWithNoFile"]) != sorted(NO_FILE_EXPECTED):
    fail.append(f"the set of declared-but-file-less tags moved: "
                f"{sorted(d['tagsWithNoFile'])} (expected {sorted(NO_FILE_EXPECTED)})")
print(f"  {d['tagsDeclaredInCode']} tags declared in DamageTypeTags, "
      f"{len(d['tagsWithNoFile'])} of them with no data file: {d['tagsWithNoFile']}")
if sorted(empty_needed) != sorted(NO_FILE_EXPECTED):
    fail.append(f"a tag the hurt path reads is empty for a reason other than "
                f"having no file: {sorted(empty_needed)}")

# Three rows whose values are known from the bytecode paths that read them, so
# a table of plausible-looking wrong numbers cannot pass.
#   starve  bypasses armor and is what FoodData.tick deals
#   in_fire is in is_fire, which FIRE_RESISTANCE immunises against
#   magic   bypasses armor and is what PoisonMobEffect deals
for want, tag in (("minecraft:starve", "minecraft:bypasses_armor"),
                  ("minecraft:in_fire", "minecraft:is_fire"),
                  ("minecraft:magic", "minecraft:bypasses_armor")):
    if want not in by_id:
        fail.append(f"no {want} row")
    elif want not in tags.get(tag, set()):
        fail.append(f"{want} is not in {tag}")

# Membership must DISCRIMINATE, or a reader would get the same DamageTags for
# every type and no test could tell a correct table from a constant.
sets = {frozenset(r["tags"]) for r in d["table"]}
print(f"  {len(sets)} distinct tag-set signatures across {d['damageTypes']} types")
if len(sets) < 10:
    fail.append(f"only {len(sets)} distinct tag sets — the table is close to a constant")
if "minecraft:player_attack" in tags.get("minecraft:bypasses_armor", set()):
    fail.append("player_attack bypasses armor, which would make armour do nothing")

# `#other_tag` references must actually have been followed. is_fire is the
# canonical nested one; if it resolved to the literal string it would show up
# as a value starting with '#'.
for tag, values in tags.items():
    for v in values:
        if v.startswith("#"):
            fail.append(f"{tag} still holds an unresolved reference {v}")

for row in d["table"]:
    bits = row["exhaustionBits"] & 0xFFFFFFFFFFFFFFFF
    if struct.unpack("<d", struct.pack("<Q", bits))[0] != row["exhaustion"]:
        fail.append(f"{row['id']}: exhaustion bits and value disagree")

if fail:
    for f in fail:
        print(f"  !! {f}")
    sys.exit(1)
PYEOF
