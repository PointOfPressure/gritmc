#!/usr/bin/env bash
# dump-biome-spawners.sh — every biome's `spawners` and `spawn_costs`, from
# the jar's own worldgen data, as `data/vanilla/biome_spawners.json`.
#
#     ./ci/dump-biome-spawners.sh          # ~1 s, no JVM, no build, no box
#
# # What this is, and why it is data rather than bytecode
#
# `NaturalSpawner.getRandomSpawnMobAt` picks a `MobSpawnSettings$SpawnerData`
# out of `ChunkGenerator.getMobsAt(biome, …)`, which is
# `biome.getMobSettings().getMobs(category)` for every generator but the
# debug one. `MobSpawnSettings` is built by a codec off
# `data/minecraft/worldgen/biome/*.json` inside the server jar — the
# registry is datapack-driven, so the numbers are data in the jar and not
# constants in a class. Reading them out of the bytecode would mean reading
# a codec, which is the wrong source.
#
# `spawn_costs` is the other half and is separate for a reason:
# `SpawnState.canSpawn` reads it (`getMobSpawnCost(type)`), and a type with
# no entry is unconditionally allowed. It is **five entries in two biomes**
# in 26.2, so a port that never implements the charge is wrong in exactly
# two biomes and correct everywhere else — which is worth knowing before
# deciding what to build.
#
# # The cross-checks, because a table with a plausible shape is the failure
#
# Three, all against a second source, all fatal:
#
# * **the biome set** against `assets/minecraft/lang/en_us.json`'s
#   `biome.minecraft.*` keys. That table is written by a different part of
#   the build from `data/minecraft/worldgen/biome/`, so a jar whose worldgen
#   data was truncated or filtered on the way into this script has two
#   counts that disagree. Measured 2026-09-05 on 26.2: **66 files, 66 lang
#   keys, and the two SETS are equal** — not merely the counts, so a
#   renamed biome is caught as well as a missing one;
# * every `type` in every spawner list and every `spawn_costs` key must be a
#   key of `minecraft:entity_type` in `generated/reports/registries.json` —
#   the generator's own report, produced by a vanilla server run and not by
#   this file's idea of what a mob is;
# * every category key must be one of `MobCategory`'s eight serialized
#   names. The list here is the loader's; that it is the ENUM's, in the
#   enum's order, is asserted against the bytecode elsewhere and not here,
#   because a list checked against itself is not checked:
#   `ci/dump-entity-spawn-facts.sh` reads all eight constructions out of
#   `MobCategory.<clinit>` and
#   `crates/rustmc-entity/tests/spawn_tables.rs::every_mob_category_constant_matches_the_enum_bytecode`
#   compares them, order included, against the Rust enum.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
REPORT="$VDIR/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/biome_spawners.json}"

[ -f "$JAR" ]    || { echo "dump-biome-spawners: no jar at $JAR" >&2; exit 1; }
[ -f "$REPORT" ] || { echo "dump-biome-spawners: no report at $REPORT — run ci/vanilla-server.sh reports" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
unzip -q -o "$JAR" 'data/minecraft/worldgen/biome/*.json' \
                   'assets/minecraft/lang/en_us.json' -d "$WORK"

python3 - "$WORK" "$REPORT" "$OUT" <<'PY'
import json, os, sys

work, report, out = sys.argv[1], sys.argv[2], sys.argv[3]
biome_dir = os.path.join(work, "data/minecraft/worldgen/biome")
lang_path = os.path.join(work, "assets/minecraft/lang/en_us.json")

with open(report) as f:
    known_types = set(json.load(f)["minecraft:entity_type"]["entries"])

# MobCategory's serialized names, in the enum's declaration order — which is
# also `SPAWNING_CATEGORIES`' order once `misc` is dropped, and that order is
# the order `spawnForChunk` walks. Written here so this script can refuse an
# unknown key; that it IS the enum's list, in the enum's order, is checked
# where the enum's own bytecode is read — see the header.
CATEGORIES = ["monster", "creature", "ambient", "axolotls",
              "underground_water_creature", "water_creature",
              "water_ambient", "misc"]

names = sorted(n[:-5] for n in os.listdir(biome_dir) if n.endswith(".json"))
if not names:
    sys.exit("dump-biome-spawners: the jar extract holds no biome files")

# Second source: the client lang table, written by a different part of the
# build. Sets, not counts — a rename is as much a failure as a deletion.
with open(lang_path) as f:
    lang = json.load(f)
PREFIX = "biome.minecraft."
lang_names = sorted(k[len(PREFIX):] for k in lang if k.startswith(PREFIX))
if not lang_names:
    sys.exit("dump-biome-spawners: en_us.json holds no biome.minecraft.* keys — "
             "the second source has gone stale and this run is unchecked")
if set(lang_names) != set(names):
    only_data = sorted(set(names) - set(lang_names))
    only_lang = sorted(set(lang_names) - set(names))
    sys.exit(f"dump-biome-spawners: worldgen data and en_us.json disagree — "
             f"{len(names)} files, {len(lang_names)} lang keys; "
             f"data-only {only_data}, lang-only {only_lang}")

biomes, rows, costs, used = {}, 0, 0, set()
for name in names:
    with open(os.path.join(biome_dir, name + ".json")) as f:
        d = json.load(f)
    spawners = {}
    for cat, lst in d["spawners"].items():
        if cat not in CATEGORIES:
            sys.exit(f"dump-biome-spawners: {name}: unknown MobCategory {cat!r}")
        entries = []
        for e in lst:
            t = e["type"]
            if t not in known_types:
                sys.exit(f"dump-biome-spawners: {name}/{cat}: {t} is not in "
                         "minecraft:entity_type — the report and the jar disagree")
            used.add(t)
            # weight/minCount/maxCount, in the order SpawnerData's codec
            # reads them. The list order is the jar's, and it is behaviour:
            # WeightedList.getRandom walks the entries in order subtracting
            # weights, so two lists that are permutations of each other draw
            # different mobs from the same RNG value.
            entries.append({"type": t, "weight": e["weight"],
                            "min_count": e["minCount"], "max_count": e["maxCount"]})
            rows += 1
        spawners[cat] = entries
    cost = {}
    for t, c in (d.get("spawn_costs") or {}).items():
        if t not in known_types:
            sys.exit(f"dump-biome-spawners: {name}: spawn_cost for unknown type {t}")
        cost[t] = {"charge": c["charge"], "energy_budget": c["energy_budget"]}
        costs += 1
    biomes[name] = {"spawners": spawners, "spawn_costs": cost}

with open(out, "w") as f:
    json.dump({"categories": CATEGORIES, "biomes": biomes}, f, indent=0, sort_keys=False)

print(f"{len(biomes)} biomes ({len(lang_names)} lang keys agree), {rows} spawner rows "
      f"over {len(used)} entity types, {costs} spawn_cost entries -> {out}")
PY
