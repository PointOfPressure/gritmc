#!/usr/bin/env bash
# dump-block-tags.sh — vanilla's block tags, from vanilla's own data generator.
#
#     ./ci/dump-block-tags.sh
#     ./ci/dump-block-tags.sh data/vanilla/tags_block.json
#
# # Why this exists, and what it does NOT do
#
# `data/vanilla/tags_block.json` has been on this box since 2026-07-21 and had
# no generator. `ci/gen-gameplay-tables.sh`'s `generator_for()` had no arm for
# it, so no tree could make it, no tree stamped it, and the copy every tree
# holds arrived through the shared `data/vanilla` copy from whichever branch
# made it — the 2026-08-03 shared-`oracle-work` failure by another door, where
# a gate measures whichever branch last regenerated a table.
#
# `crates/rustmc-server/src/itemuse.rs` names this exactly and refuses on it:
# `FlintAndSteelItem`'s lighting branch is
# `state.is(BlockTags.CAMPFIRES / CANDLES / CANDLE_CAKES)`, all three are in
# that file, and the movement lane declined to read it because reading an
# unstamped artefact would gate its branch against another one. That refusal
# was right and this script is what removes the reason for it.
#
# **It does not make anything read block tags.** `ci/gen-gameplay-tables.sh`
# derives its required set from quoted `"data/vanilla/x.json"` literals in
# `crates`, `tools` and `plugins`, and nothing names this one — deliberately:
# a literal with no reader is a dead reference, and this project has paid for
# one of those already (`ACCEPT_TELEPORTATION`, decoded into a field read by
# nothing). The arm in `generator_for()` fires the moment a consumer names the
# literal, and until then this script is reachable by hand and covered by
# `ci/gen-tables-falsify.sh` case 8.
#
# # Where the data comes from
#
# Identical to `ci/dump-recipes.sh`, which is the point: `ci/vanilla-server.sh
# data` runs Mojang's own generator, block tags land one JSON file per tag
# under `data/<ns>/tags/block/`, and this flattens them into one object keyed
# by tag name. Read from the generator's own output, never transcribed — a tag
# membership typed from memory is a `soul_campfire` that lights when vanilla's
# does not.
#
# Names are kept bare (`mineable/axe`, not `minecraft:mineable/axe`), which is
# what `data/vanilla/tags_item.json` already does and what a `Tags::contains`
# lookup expects; a non-`minecraft` namespace is kept whole (`ns:name`) so it
# cannot collide. The output is gitignored like the rest of `data/vanilla/`.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
VDIR="$ROOT/oracle-work/vanilla-$VER"
GEN="$VDIR/generated-data/data"
OUT_TAGS="${1:-$ROOT/data/vanilla/tags_block.json}"

if [ ! -d "$GEN" ]; then
    echo "dump-block-tags: no datapack at $GEN — running the generator" >&2
    "$ROOT/ci/vanilla-server.sh" data || {
        echo "dump-block-tags: ci/vanilla-server.sh data failed" >&2
        exit 1
    }
fi
[ -d "$GEN" ] || { echo "dump-block-tags: still no datapack at $GEN" >&2; exit 1; }

python3 - "$GEN" "$OUT_TAGS" <<'PY'
import json, os, sys

gen, out_tags = sys.argv[1], sys.argv[2]


def collect(subpath):
    """Every JSON file under data/<ns>/<subpath>, keyed by its name.

    Recursive, and that is load-bearing rather than defensive: block tags nest
    a directory deep — `mineable/axe`, `incorrect_for_iron_tool` — and a
    non-recursive walk returns a plausible subset, which looks like a working
    dumper and a missing tag. Copied in shape from `ci/dump-recipes.sh`; the
    two are the same walk over two subtrees of one generator output.
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


tags = collect("tags/block")
if not tags:
    sys.exit("dump-block-tags: no block tags found — the datapack layout has moved")

# THE THREE THIS WAS ASKED FOR, ASSERTED BY NAME. A count on its own cannot
# say the walk reached the tags a caller needs: `itemuse.rs` names CAMPFIRES,
# CANDLES and CANDLE_CAKES as the reason this file is wanted, and a dumper
# that returned 300 tags without them would read as a success.
need = ["campfires", "candles", "candle_cakes"]
missing = [t for t in need if t not in tags]
if missing:
    sys.exit(
        "dump-block-tags: %d tags found but %s absent — these are the three "
        "FlintAndSteelItem.useOn tests and the reason this table is wanted, "
        "so their absence is a broken walk, not a thin vanilla"
        % (len(tags), ", ".join(missing))
    )

os.makedirs(os.path.dirname(out_tags), exist_ok=True)
with open(out_tags, "w") as fh:
    json.dump(dict(sorted(tags.items())), fh, indent=0)
print(f"{len(tags)} block tags -> {out_tags}")
nested = sum(1 for k in tags if "/" in k)
print(f"  of which {nested} are nested (mineable/*, incorrect_for_*), which a "
      f"non-recursive walk would have dropped")
PY
