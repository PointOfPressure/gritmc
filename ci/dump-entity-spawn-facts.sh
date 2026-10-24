#!/usr/bin/env bash
# dump-entity-spawn-facts.sh — the `EntityType` fields natural spawning reads,
# from `EntityTypes.<clinit>`, as `data/vanilla/entity_spawn_facts.json`.
#
#     ./ci/dump-entity-spawn-facts.sh      # ~3 s, javap only, no build, no box
#
# # Which fields, and why each one
#
# `NaturalSpawner` and `SpawnPlacements` between them ask an `EntityType`
# six questions and every one of them is a `Builder` field:
#
#     getCategory()             which cap and which despawn distance
#     canSpawnFarFromPlayer()   isValidSpawnPostitionForType's 128*128 gate
#     canSummon()               whether canSpawnMobAt is consulted at all
#     isAllowedInPeaceful()     SpawnPlacements.checkSpawnRules' first line
#     getSpawnAABB(x,y,z)       width, height and spawnDimensionsScale
#     isBlockDangerous(state)   the `immuneTo` block tag
#
# # The default that is not `false` — and it is the one worth this script
#
# `Builder.<init>` sets `canSpawnFarFromPlayer` to
# **`category == CREATURE || category == MISC`**, at offsets 94-114, and
# `.canSpawnFarFromPlayer()` then sets it true for a handful of others. So
# **every CREATURE spawns far from a player without any builder call saying
# so**, and a port that read the flag as "false unless called" would apply a
# 128-block gate to every animal in the game. The same constructor sets
# `summon = true` (offset 16), `allowedInPeaceful = true` (79),
# `spawnDimensionsScale = 1.0f` (43) and `dimensions =
# EntityDimensions.scalable(0.6f, 1.8f)` (31-39), so an absent builder call
# is never an absent value.
#
# # Refusing rather than dropping
#
# The parse walks each registration segment and asserts every
# `EntityType$Builder` method it meets is one it knows. A builder method
# added in a later version is then a **refusal naming the method**, not a
# row quietly missing a flag — which is the failure this project keeps
# paying for elsewhere.
#
# # The cross-checks
#
# * the registration count against `minecraft:entity_type` in
#   `generated/reports/registries.json`, and the two NAME SETS compared, not
#   just the counts;
# * the `notInPeaceful` count against the figure
#   `docs/cold-start/mob-tick-surface.md` published from an independent
#   census (`ci/mob-tick-census.py`): **38 of 158**. A disagreement means one
#   of the two readings is wrong and this script says which numbers it got.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
REPORT="$VDIR/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/entity_spawn_facts.json}"

[ -f "$JAR" ]    || { echo "dump-entity-spawn-facts: no jar at $JAR" >&2; exit 1; }
[ -f "$REPORT" ] || { echo "dump-entity-spawn-facts: no report at $REPORT" >&2; exit 1; }
command -v javap >/dev/null || { echo "dump-entity-spawn-facts: javap not on PATH (JDK 25)" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q -o "$JAR" 'net/minecraft/world/entity/EntityTypes*.class' \
                   'net/minecraft/world/entity/EntityType$Builder.class' \
                   'net/minecraft/world/entity/MobCategory.class' -d "$WORK"
javap -p -c -constants -cp "$WORK" net.minecraft.world.entity.EntityTypes > "$WORK/et.txt"
javap -p -c -constants -cp "$WORK" net.minecraft.world.entity.MobCategory > "$WORK/mc.txt"

python3 - "$WORK/et.txt" "$REPORT" "$OUT" "$WORK/mc.txt" <<'PY'
import json, re, sys

dump, report, out, catdump = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
lines = open(dump).read().splitlines()
with open(report) as f:
    known = set(json.load(f)["minecraft:entity_type"]["entries"])

try:
    s0 = next(i for i, l in enumerate(lines) if l.strip() == "static {};")
except StopIteration:
    sys.exit("dump-entity-spawn-facts: no static initialiser")
body = lines[s0:]

RE_ID = re.compile(r"getstatic\s+#\d+\s+// Field net/minecraft/world/entity/EntityTypeIds\.(\w+):")
RE_CAT = re.compile(r"getstatic\s+#\d+\s+// Field net/minecraft/world/entity/MobCategory\.(\w+):")
RE_BUILD = re.compile(r"invokevirtual\s+#\d+\s+// Method net/minecraft/world/entity/EntityType\$Builder\.(\w+):")
RE_OF = re.compile(r"invokestatic\s+#\d+\s+// Method net/minecraft/world/entity/EntityType\$Builder\.(of|createNothing):")
RE_END = re.compile(r"putstatic\s+#\d+\s+// Field (\w+):Lnet/minecraft/world/entity/EntityType;")
RE_FLOAT = re.compile(r"//\s+float ([-0-9.eE]+)f?$")
RE_FCONST = re.compile(r"^\s*\d+: fconst_([012])$")
RE_TAG = re.compile(r"getstatic\s+#\d+\s+// Field net/minecraft/tags/BlockTags\.(\w+):")

# Every Builder method the 26.2 initialiser uses. A method outside this set is
# a refusal: see the header.
SPAWN_FLAGS = {"noSummon", "notInPeaceful", "canSpawnFarFromPlayer", "fireImmune"}
SIZING = {"sized", "spawnDimensionsScale", "immuneTo"}
IRRELEVANT = {"eyeHeight", "passengerAttachments", "vehicleAttachment", "ridingOffset",
              "nameTagOffset", "attach", "noSave", "clientTrackingRange",
              "updateInterval", "requiredFeatures", "noLootTable"}
KNOWN = SPAWN_FLAGS | SIZING | IRRELEVANT

# Builder.<init> defaults, read at the offsets named in the header.
DEFAULT_W, DEFAULT_H, DEFAULT_SCALE = 0.6, 1.8, 1.0
DEFAULT_IMMUNE_TO = "DEFAULT_IMMUNE_TO"

rows, cur = [], None
floats, cat, immune = [], None, None
for ln in body:
    m = RE_ID.search(ln)
    if m:
        cur = {"field": m.group(1), "flags": []}
        floats, cat, immune = [], None, None
        continue
    if cur is None:
        continue
    m = RE_FCONST.search(ln)
    if m:
        # `fconst_0/1/2` push a float with no `// float` comment, so a
        # matcher keyed on that comment alone silently loses the argument —
        # which is how BLOCK_DISPLAY's `sized(0, 0)` first read as "sized()
        # with 0 float constants in scope".
        floats.append(float(m.group(1)))
        continue
    m = RE_FLOAT.search(ln)
    if m:
        floats.append(float(m.group(1)))
        continue
    m = RE_TAG.search(ln)
    if m:
        immune = m.group(1)
        continue
    m = RE_CAT.search(ln)
    if m:
        cat = m.group(1)
        continue
    if RE_OF.search(ln):
        cur["category"] = cat
        continue
    m = RE_BUILD.search(ln)
    if m:
        name = m.group(1)
        if name not in KNOWN:
            sys.exit(f"dump-entity-spawn-facts: {cur['field']}: unknown "
                     f"EntityType$Builder method {name!r} — the parse would drop it")
        if name == "sized":
            if len(floats) < 2:
                sys.exit(f"dump-entity-spawn-facts: {cur['field']}: sized() with "
                         f"{len(floats)} float constants in scope")
            cur["width"], cur["height"] = floats[-2], floats[-1]
        elif name == "spawnDimensionsScale":
            if not floats:
                sys.exit(f"dump-entity-spawn-facts: {cur['field']}: spawnDimensionsScale() "
                         "with no float constant in scope")
            cur["spawn_dimensions_scale"] = floats[-1]
        elif name == "immuneTo":
            if immune is None:
                sys.exit(f"dump-entity-spawn-facts: {cur['field']}: immuneTo() with no "
                         "BlockTags constant in scope")
            cur["immune_to"] = immune
        elif name in SPAWN_FLAGS:
            cur["flags"].append(name)
        floats = []
        continue
    m = RE_END.search(ln)
    if m:
        if m.group(1) != cur["field"]:
            sys.exit(f"dump-entity-spawn-facts: registration opened as "
                     f"{cur['field']} and closed as {m.group(1)}")
        if cur.get("category") is None:
            sys.exit(f"dump-entity-spawn-facts: {cur['field']}: no MobCategory before Builder.of")
        name = "minecraft:" + cur["field"].lower()
        if name not in known:
            sys.exit(f"dump-entity-spawn-facts: {name} is not in minecraft:entity_type")
        category = cur["category"]
        flags = set(cur["flags"])
        rows.append({
            "type": name,
            "category": category.lower(),
            "width": cur.get("width", DEFAULT_W),
            "height": cur.get("height", DEFAULT_H),
            "spawn_dimensions_scale": cur.get("spawn_dimensions_scale", DEFAULT_SCALE),
            # THE DEFAULT IS NOT false — see the header.
            "can_spawn_far_from_player": ("canSpawnFarFromPlayer" in flags
                                          or category in ("CREATURE", "MISC")),
            "can_summon": "noSummon" not in flags,
            "allowed_in_peaceful": "notInPeaceful" not in flags,
            "fire_immune": "fireImmune" in flags,
            "immune_to": cur.get("immune_to", DEFAULT_IMMUNE_TO),
        })
        cur = None

names = {r["type"] for r in rows}
if names != known:
    sys.exit(f"dump-entity-spawn-facts: {len(rows)} registrations against "
             f"{len(known)} registry entries; missing {sorted(known - names)[:5]}, "
             f"extra {sorted(names - known)[:5]}")

not_peaceful = sum(1 for r in rows if not r["allowed_in_peaceful"])
EXPECT_NOT_PEACEFUL = 38   # ci/mob-tick-census.py, docs/cold-start/mob-tick-surface.md
if not_peaceful != EXPECT_NOT_PEACEFUL:
    sys.exit(f"dump-entity-spawn-facts: {not_peaceful} types are notInPeaceful, the "
             f"independent census in docs/cold-start/mob-tick-surface.md says "
             f"{EXPECT_NOT_PEACEFUL} — one of the two readings is wrong")

# --- MobCategory.<clinit>, so the enum's own constants are jar-derived too.
#
# The eight constructions are
# `new MobCategory; dup; ldc <NAME>; <ordinal>; ldc <serialized>;
#  ldc <abbrev>; <max>; <isFriendly>; <isPersistent>; <despawnDistance>;
#  invokespecial <init>`, in declaration order — which is also
# `NaturalSpawner.SPAWNING_CATEGORIES`' order once MISC is filtered out, and
# that order is `spawnForChunk`'s iteration order.
cat_lines = open(catdump).read().splitlines()
try:
    c0 = next(i for i, l in enumerate(cat_lines) if l.strip() == "static {};")
except StopIteration:
    sys.exit("dump-entity-spawn-facts: MobCategory has no static initialiser")

RE_STR = re.compile(r"ldc\s+#\d+\s+// String (\S+)$")
RE_INT = re.compile(r"^\s*\d+: (?:bipush|sipush)\s+(-?\d+)$|^\s*\d+: iconst_(m1|[0-5])$")
RE_NEW = re.compile(r"new\s+#\d+\s+// class net/minecraft/world/entity/MobCategory$")
RE_INIT = re.compile(r"invokespecial\s+#\d+\s+// Method \"<init>\":\(Ljava/lang/String;ILjava/lang/String;Ljava/lang/String;IZZI\)V")

categories, strs, ints, open_ctor = [], [], [], False
for ln in cat_lines[c0:]:
    if RE_NEW.search(ln):
        open_ctor, strs, ints = True, [], []
        continue
    if not open_ctor:
        continue
    m = RE_STR.search(ln)
    if m:
        strs.append(m.group(1))
        continue
    m = RE_INT.match(ln)
    if m:
        v = m.group(1)
        if v is None:
            g = m.group(2)
            v = -1 if g == "m1" else int(g)
        ints.append(int(v))
        continue
    if RE_INIT.search(ln):
        # strs = [NAME, serialized, abbreviation]; ints = [ordinal, max,
        # isFriendly, isPersistent, despawnDistance].
        if len(strs) != 3 or len(ints) != 5:
            sys.exit(f"dump-entity-spawn-facts: MobCategory ctor at an unrecognised "
                     f"shape: strings {strs}, ints {ints}")
        categories.append({
            "name": strs[0], "serialized": strs[1], "abbreviation": strs[2],
            "ordinal": ints[0], "max_instances_per_chunk": ints[1],
            "is_friendly": bool(ints[2]), "is_persistent": bool(ints[3]),
            "despawn_distance": ints[4],
        })
        open_ctor = False

if len(categories) != 8:
    sys.exit(f"dump-entity-spawn-facts: read {len(categories)} MobCategory "
             "constructions, expected 8")
if [c["ordinal"] for c in categories] != list(range(8)):
    sys.exit("dump-entity-spawn-facts: MobCategory ordinals are not 0..7 in order")
seen = {c["category"] for c in ({"category": r["category"]} for r in rows)}
known_cats = {c["serialized"] for c in categories}
if not seen <= known_cats:
    sys.exit(f"dump-entity-spawn-facts: EntityTypes names categories the enum does "
             f"not: {sorted(seen - known_cats)}")

rows.sort(key=lambda r: r["type"])
spawning = sum(1 for r in rows if r["category"] != "misc")
with open(out, "w") as f:
    json.dump({
        # `getNoDespawnDistance()` is `bipush 32; ireturn` — a method on the
        # enum, the same for every category, and NOT the per-category field
        # above it. Emitted once for that reason.
        "no_despawn_distance": 32,
        "categories": categories,
        "types": rows,
    }, f, indent=0)
print(f"{len(categories)} MobCategory constants, "
      f"{len(rows)} entity types ({spawning} in a spawning category, "
      f"{not_peaceful} notInPeaceful, "
      f"{sum(1 for r in rows if r['can_spawn_far_from_player'])} canSpawnFarFromPlayer, "
      f"{sum(1 for r in rows if not r['can_summon'])} noSummon) -> {out}")
PY
