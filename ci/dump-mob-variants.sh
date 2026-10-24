#!/usr/bin/env bash
# dump-mob-variants.sh — the datapack side of a mob's spawn-time variant
# draws, straight out of the jar's own data files.
#
#     ./ci/dump-mob-variants.sh [out.json]
#
# Produces `data/vanilla/mob_variants.json`:
#
#   * `pig_variant`: for each `data/minecraft/pig_variant/<name>.json`, its
#     `spawn_conditions` as `PriorityProvider.select` reads them — a list
#     of `{priority, biomes}` where `biomes` is the `minecraft:biome`
#     condition's set, resolved recursively through
#     `data/minecraft/tags/worldgen/biome/*.json` (a `#tag` reference is
#     followed; a value beginning `#` inside a tag file is another tag), or
#     `null` for a selector with no condition;
#   * `sky_light_level`: `data/minecraft/timeline/day.json`'s
#     `minecraft:gameplay/sky_light_level` track — the keyframes
#     `(ticks, value)` in file order, its `ease` (absent = `linear`, the
#     codec's default) and `modifier` (`multiply`), and the timeline's
#     `period_ticks`. `ServerLevel.updateSkyBrightness` is
#     `skyDarken = (int)(15 - environmentAttributes.getDimensionValue(SKY_LIGHT_LEVEL))`
#     and this track is the whole of that attribute's variation in the
#     overworld;
#   ALL FIVE sounds of each set are carried, not the two the ambient/step
#   paths needed. `hurt_sound` and `death_sound` are what `LivingEntity
#   .getHurtSound`/`.getDeathSound` resolve to for a pig, and a table that
#   omitted them would make a combat port reach for `SoundEvents.GENERIC_HURT`
#   — `LivingEntity`'s own fallback, and audibly wrong for every pig in the
#   game. `eat_sound` comes with them because leaving one of the five out is
#   how this list needed changing twice.
#   * `cow_variant` and `chicken_variant`: the same shape as `pig_variant`,
#     for `Cow.finalizeSpawn` and `Chicken.finalizeSpawn`, which make the same
#     `VariantUtils.selectVariantToSpawn` call over their own registries;
#   * `cow_sound_variant`: `{ambient, step}` per entry (a `CowSoundVariant` has
#     no baby set — `AbstractCow.getAmbientSound` reads one sound whatever the
#     age); `chicken_sound_variant`: `{ambient, step, baby_ambient,
#     baby_step}` (`Chicken.getSoundSet` picks `babySounds` for a baby);
#   * `farm_animal_biomes`: `{warm, cold}`, the two biome tags
#     `SheepColorSpawnRules.getSheepColorConfiguration` reads
#     (`#minecraft:spawns_warm_variant_farm_animals`, then `..._cold_...`),
#     resolved the same way;
#   * `pig_sound_variant`: the entry names, sorted, which is the order
#     `RegistryDataLoader` registers datapack entries in and therefore the
#     `byId` order `Registry.getRandom` draws over. The client-synced ids in
#     `registries.bin` are the same order; the server reads those for the
#     wire and this list for the draw, and `tests/mob_variants.rs` refuses a
#     tree where the two disagree.
#
# No JVM: these are JSON files in the jar. **A runtime extract, not a
# fixture**: `data/vanilla/`, gitignored, regenerated per tree by
# `ci/gen-gameplay-tables.sh`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/mob_variants.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
JAR="$ROOT/oracle-work/vanilla-$VER/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR — run ci/vanilla-server.sh fetch, then start once to unpack" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q -o "$JAR" 'data/minecraft/pig_variant/*' 'data/minecraft/pig_sound_variant/*' \
    'data/minecraft/cow_variant/*' 'data/minecraft/cow_sound_variant/*' \
    'data/minecraft/chicken_variant/*' 'data/minecraft/chicken_sound_variant/*' \
    'data/minecraft/tags/worldgen/biome/*' 'data/minecraft/timeline/day.json' -d "$WORK"

mkdir -p "$(dirname "$OUT")"
python3 - "$WORK" "$OUT" <<'PYEOF'
import json, os, sys

work, out = sys.argv[1], sys.argv[2]
tagdir = os.path.join(work, "data/minecraft/tags/worldgen/biome")

def resolve(tag, seen):
    if tag in seen:
        return set()
    seen.add(tag)
    name = tag.split(":", 1)[1]
    with open(os.path.join(tagdir, name + ".json")) as f:
        doc = json.load(f)
    values = set()
    for v in doc["values"]:
        s = v["id"] if isinstance(v, dict) else v
        if s.startswith("#"):
            values |= resolve(s[1:], seen)
        else:
            values.add(s if ":" in s else "minecraft:" + s)
    return values

def biome_set(cond):
    b = cond["biomes"]
    if isinstance(b, str):
        if b.startswith("#"):
            return sorted(resolve(b[1:], set()))
        return [b if ":" in b else "minecraft:" + b]
    return sorted(x if ":" in x else "minecraft:" + x for x in b)

def variant_selectors(registry):
    out = {}
    vdir = os.path.join(work, "data/minecraft", registry)
    for fn in sorted(os.listdir(vdir)):
        with open(os.path.join(vdir, fn)) as f:
            doc = json.load(f)
        sels = []
        for sel in doc.get("spawn_conditions", []):
            cond = sel.get("condition")
            if cond is None:
                sels.append({"priority": sel["priority"], "biomes": None})
            elif cond["type"] == "minecraft:biome":
                sels.append({"priority": sel["priority"], "biomes": biome_set(cond)})
            else:
                raise SystemExit(f"{registry}/{fn}: unhandled condition {cond['type']}")
        out["minecraft:" + fn[:-5]] = sels
    if len(out) != 3 or not any(all(s["biomes"] is None for s in v) for v in out.values()):
        raise SystemExit(f"{registry}: expected 3 entries, one unconditional; got {sorted(out)}")
    return out

variants = variant_selectors("pig_variant")
cow_variants = variant_selectors("cow_variant")
chicken_variants = variant_selectors("chicken_variant")

def flat_sounds(registry, keys):
    out = {}
    sdir = os.path.join(work, "data/minecraft", registry)
    for fn in sorted(os.listdir(sdir)):
        with open(os.path.join(sdir, fn)) as f:
            doc = json.load(f)
        out["minecraft:" + fn[:-5]] = {k: v(doc) for k, v in keys.items()}
    return out

cow_sounds = flat_sounds("cow_sound_variant", {
    "ambient": lambda d: d["ambient_sound"],
    "step": lambda d: d["step_sound"],
})
chicken_sounds = flat_sounds("chicken_sound_variant", {
    "ambient": lambda d: d["adult_sounds"]["ambient_sound"],
    "step": lambda d: d["adult_sounds"]["step_sound"],
    "baby_ambient": lambda d: d["baby_sounds"]["ambient_sound"],
    "baby_step": lambda d: d["baby_sounds"]["step_sound"],
})
farm_biomes = {
    "warm": sorted(resolve("minecraft:spawns_warm_variant_farm_animals", set())),
    "cold": sorted(resolve("minecraft:spawns_cold_variant_farm_animals", set())),
}

sdir = os.path.join(work, "data/minecraft/pig_sound_variant")
sounds = {}
for fn in sorted(os.listdir(sdir)):
    with open(os.path.join(sdir, fn)) as f:
        doc = json.load(f)
    sounds["minecraft:" + fn[:-5]] = {
        "ambient": doc["adult_sounds"]["ambient_sound"],
        "step": doc["adult_sounds"]["step_sound"],
        "hurt": doc["adult_sounds"]["hurt_sound"],
        "death": doc["adult_sounds"]["death_sound"],
        "eat": doc["adult_sounds"]["eat_sound"],
        "baby_ambient": doc["baby_sounds"]["ambient_sound"],
        "baby_step": doc["baby_sounds"]["step_sound"],
        "baby_hurt": doc["baby_sounds"]["hurt_sound"],
        "baby_death": doc["baby_sounds"]["death_sound"],
        "baby_eat": doc["baby_sounds"]["eat_sound"],
    }

with open(os.path.join(work, "data/minecraft/timeline/day.json")) as f:
    day = json.load(f)
track = day["tracks"]["minecraft:gameplay/sky_light_level"]
sky = {
    "period_ticks": day["period_ticks"],
    "ease": track.get("ease", "linear"),
    "modifier": track.get("modifier", "override"),
    "keyframes": [[k["ticks"], k["value"]] for k in track["keyframes"]],
}
if sky["modifier"] != "multiply" or len(sky["keyframes"]) < 2:
    raise SystemExit(f"sky_light_level track has an unexpected shape: {sky}")

if len(variants) != 3 or len(sounds) != 3:
    raise SystemExit(f"expected 3 pig variants and 3 sound variants, got {len(variants)} / {len(sounds)}")
if not any(all(s["biomes"] is None for s in v) for v in variants.values()):
    raise SystemExit("no pig variant is unconditional — temperate should be")

with open(out, "w") as f:
    json.dump({"pig_variant": variants, "pig_sound_variant": sounds, "sky_light_level": sky,
               "cow_variant": cow_variants, "cow_sound_variant": cow_sounds,
               "chicken_variant": chicken_variants, "chicken_sound_variant": chicken_sounds,
               "farm_animal_biomes": farm_biomes},
              f, indent=1, sort_keys=True)
print(f"mob variants -> {out}: {len(variants)} pig variants, {len(sounds)} pig sound variants, "
      f"{len(cow_variants)}/{len(cow_sounds)} cow, {len(chicken_variants)}/{len(chicken_sounds)} chicken")
PYEOF
