#!/usr/bin/env bash
# dump-recipes.sh — the crafting recipes and the item tags they are written in
# terms of, from vanilla's own data generator.
#
#     ./ci/dump-recipes.sh
#     ./ci/dump-recipes.sh data/vanilla/recipes.json data/vanilla/tags_item.json
#
# Writes **both** files, because they are one dependency and not two: almost
# every ingredient in vanilla's recipe set is a tag reference — `oak_planks` is
# `#minecraft:oak_logs` — so recipes without tags would load and then match
# almost nothing, which reads as a broken recipe engine rather than a missing
# file. `ci/gen-gameplay-tables.sh` maps both table names at this one script
# for that reason; running it twice is idempotent.
#
# # Where the data comes from
#
# `ci/vanilla-server.sh data` runs Mojang's own generator (`--server`), which
# writes the built-in datapack to `generated-data/`. Recipes are one JSON file
# per recipe under `data/<ns>/recipe/`, item tags one per tag under
# `data/<ns>/tags/item/`. This flattens each set into a single object keyed by
# the recipe or tag name, which is the shape `crate::recipes` reads.
#
# **Recipe and tag data is datapack data, not code**, so the rule this project
# already follows applies unchanged: read it from the generator's own output,
# never transcribe it. A recipe typed from memory is a crafting table that
# hands out the wrong item, and nothing in the corpus would notice until the
# one scenario that crafts that item.
#
# # The names are kept bare
#
# A file at `data/minecraft/recipe/oak_planks.json` becomes the key
# `oak_planks`, not `minecraft:oak_planks`. That is what the existing
# `data/vanilla/tags_item.json` already does and what `ItemTags::contains`
# expects — it strips a `minecraft:` prefix on lookup rather than adding one
# here, so a modded tag in another namespace cannot silently match a vanilla
# one. Namespaces other than `minecraft` are kept whole (`ns:name`) so they
# cannot collide.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
VDIR="$ROOT/oracle-work/vanilla-$VER"
GEN="$VDIR/generated-data/data"
OUT_RECIPES="${1:-$ROOT/data/vanilla/recipes.json}"
OUT_TAGS="${2:-$ROOT/data/vanilla/tags_item.json}"

if [ ! -d "$GEN" ]; then
    echo "dump-recipes: no datapack at $GEN — running the generator" >&2
    "$ROOT/ci/vanilla-server.sh" data || {
        echo "dump-recipes: ci/vanilla-server.sh data failed" >&2
        exit 1
    }
fi
[ -d "$GEN" ] || { echo "dump-recipes: still no datapack at $GEN" >&2; exit 1; }

python3 - "$GEN" "$OUT_RECIPES" "$OUT_TAGS" <<'PY'
import json, os, sys

gen, out_recipes, out_tags = sys.argv[1], sys.argv[2], sys.argv[3]


def collect(subpath):
    """Every JSON file under data/<ns>/<subpath>, keyed by its name.

    Recursive, because vanilla nests some sets a directory deep and a
    non-recursive walk would silently return a subset — which is the failure
    mode that looks like a working dumper and a missing recipe.
    """
    out = {}
    for ns in sorted(os.listdir(gen)):
        root = os.path.join(gen, ns, *subpath.split("/"))
        if not os.path.isdir(root):
            continue
        for dirpath, _, files in os.walk(root):
            for f in sorted(files):
                if not f.endswith(".json"):
                    continue
                rel = os.path.relpath(os.path.join(dirpath, f), root)
                name = rel[: -len(".json")].replace(os.sep, "/")
                key = name if ns == "minecraft" else f"{ns}:{name}"
                with open(os.path.join(dirpath, f)) as fh:
                    out[key] = json.load(fh)
    return out


recipes = collect("recipe")
tags = collect("tags/item")
if not recipes:
    sys.exit("dump-recipes: no recipes found — the datapack layout has moved")
if not tags:
    sys.exit("dump-recipes: no item tags found — the datapack layout has moved")

for path, data, what in (
    (out_recipes, recipes, "recipes"),
    (out_tags, tags, "item tags"),
):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as fh:
        json.dump(dict(sorted(data.items())), fh, indent=0)
    print(f"{len(data)} {what} -> {path}")

# A crafting recipe count on its own is not evidence the file is usable, so
# say how many are the two types `crate::recipes` can actually run.
usable = sum(
    1
    for r in recipes.values()
    if r.get("type") in ("minecraft:crafting_shapeless", "minecraft:crafting_shaped")
)
print(f"  of which {usable} are crafting_shaped or crafting_shapeless")
PY
