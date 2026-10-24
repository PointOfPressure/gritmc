#!/usr/bin/env bash
# dump-recipe-unlocks.sh — item -> recipes it unlocks, and each recipe's
# captured `recipe_book_add` entry bytes.
#
#     ./ci/dump-recipe-unlocks.sh
#     ./ci/dump-recipe-unlocks.sh data/vanilla/recipe_unlocks.json
#
# `ServerRecipeBook.addRecipes` (`javap -p -c -constants`, offsets 45-56,
# 72-98, 115-137) sends one `ClientboundRecipeBookAddPacket` per newly
# unlocked recipe set. What decides *which* recipes is `PARITY-ROADMAP.md`
# §3 S2's finding: an advancement whose `minecraft:inventory_changed`
# criterion names an item, and whose `rewards.recipes` names the recipe —
# read from vanilla's own advancement JSON, never transcribed. What decides
# the packet's *bytes* is `RecipeDisplayEntry`, a polymorphic tree this
# reads nowhere near — see `crates/rustmc-server/src/recipe_book.rs`'s doc
# comment. So this script has two halves that do not share a language:
#
#   1. a static read of `data/<ns>/advancement/**/*.json` (Python, no
#      running server) for item -> candidate recipe ids;
#   2. a live capture against a real vanilla server (`oracle-runner
#      dump-recipe-displays`) of each single-recipe candidate's exact
#      packet bytes.
#
# Only single-recipe candidates are captured. An item naming more than one
# recipe would need `RecipeDisplayEntry` parsed to split its entries apart,
# which is exactly the reconstruction this table exists to avoid — such an
# item is dropped rather than guessed at, and stays an honest gap.
#
# # `requirements`, and why a second inventory_changed criterion is not free
#
# An advancement's `requirements` is a list of AND'd groups, OR'd within each
# group (vanilla default when absent: every criterion its own group, all
# required). `recipes/smoker.json`'s is `[["has_the_recipe", "has_furnace"]]`
# — one group, so satisfying *either* name completes the whole advancement,
# which is why holding a furnace alone is sufficient. A criterion is treated
# as standalone-sufficient here only when every group containing it has no
# other member, or has only members whose own trigger is
# `minecraft:recipe_unlocked` (the "already know it another way" idiom
# `has_the_recipe` is). An item reachable only through a group that also
# needs a *second* `inventory_changed` criterion — a genuine multi-material
# advancement — is dropped: claiming it standalone would unlock a recipe
# vanilla would not yet grant, which is a wrong answer and not merely a gap.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
OUT="${1:-$ROOT/data/vanilla/recipe_unlocks.json}"

VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
GEN="$VDIR/generated-data/data"
if [ ! -d "$GEN" ]; then
    echo "dump-recipe-unlocks: no datapack at $GEN — running the generator" >&2
    "$ROOT/ci/vanilla-server.sh" data || {
        echo "dump-recipe-unlocks: ci/vanilla-server.sh data failed" >&2
        exit 1
    }
fi
[ -d "$GEN" ] || { echo "dump-recipe-unlocks: still no datapack at $GEN" >&2; exit 1; }

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
CANDIDATES="$WORK/candidates.json"

python3 - "$GEN" "$CANDIDATES" <<'PY'
import json, os, sys

gen, out_candidates = sys.argv[1], sys.argv[2]


def collect(subpath):
    """Every JSON file under data/<ns>/<subpath>, keyed by its name.
    Same walk as ci/dump-recipes.sh's, kept independent on purpose: this
    script must not depend on data/vanilla/tags_item.json already existing,
    or the two generators would have an undeclared order between them.
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


tags = collect("tags/item")
advancements = collect("advancement")
if not advancements:
    sys.exit("dump-recipe-unlocks: no advancements found — the datapack layout has moved")


def resolve_tag(tag, seen):
    """`#ns:name` -> the concrete item ids it names, recursively.

    A tag's own `values` can name another tag — `minecraft:logs` is
    `["#minecraft:logs_that_burn", "#minecraft:crimson_stems",
    "#minecraft:warped_stems"]`, no literal item anywhere in it — so one
    level of resolution silently returns nothing for `smoker.json`'s own
    `#minecraft:logs` criterion. `seen` guards a cycle vanilla itself would
    refuse to load; nothing in the extract has one, and this is cheaper than
    trusting that to stay true.
    """
    if tag in seen:
        return []
    seen = seen | {tag}
    bare = tag.split(":", 1)[-1]
    out = []
    for v in tags.get(bare, {}).get("values", []):
        if isinstance(v, str) and v.startswith("#"):
            out.extend(resolve_tag(v, seen))
        elif isinstance(v, str):
            out.append(v)
    return out


def items_of(predicate_element):
    """One element of an `items` predicate list -> the concrete item ids it
    names. `#ns:tag` is resolved through the item tags collected above,
    recursively; everything else is already a bare item id (`recipes/`
    advancements were checked 2026-08-08 and never use the multi-item
    `items: [...]` form — see the commit message this shipped in for the
    count)."""
    v = predicate_element.get("items")
    if not isinstance(v, str):
        return []
    if v.startswith("#"):
        return resolve_tag(v, frozenset())
    return [v]


def standalone_sufficient(adv, crit_name, trigger_of):
    """Whether satisfying `crit_name` alone can complete `adv` — see the
    header comment on `requirements`."""
    reqs = adv.get("requirements")
    if reqs is None:
        reqs = [[n] for n in adv.get("criteria", {})]
    for group in reqs:
        if crit_name not in group:
            continue
        others = [n for n in group if n != crit_name]
        if all(trigger_of.get(n) == "minecraft:recipe_unlocked" for n in others):
            return True
    return False


by_item = {}
scanned = 0
for name, adv in sorted(advancements.items()):
    recipes = adv.get("rewards", {}).get("recipes") or []
    criteria = adv.get("criteria", {})
    if not recipes or not criteria:
        continue
    trigger_of = {n: c.get("trigger") for n, c in criteria.items()}
    for crit_name, c in criteria.items():
        if c.get("trigger") != "minecraft:inventory_changed":
            continue
        scanned += 1
        if not standalone_sufficient(adv, crit_name, trigger_of):
            continue
        for el in c.get("conditions", {}).get("items") or []:
            for item in items_of(el):
                bucket = by_item.setdefault(item, set())
                bucket.update(f"minecraft:{r}" if ":" not in r else r for r in recipes)

with open(out_candidates, "w") as fh:
    json.dump({k: sorted(v) for k, v in sorted(by_item.items())}, fh, indent=0)
multi = sum(1 for v in by_item.values() if len(v) != 1)
print(
    f"{len(advancements)} advancements, {scanned} inventory_changed criteria seen, "
    f"{len(by_item)} candidate items ({multi} ambiguous, not attempted) -> {out_candidates}"
)
PY

# --- live capture: boot real vanilla, hold each single-recipe candidate ----
#
# Probed rather than fixed, for the reason `ci/t2-gameplay.sh` gives for
# doing the same: two trees hardcoding one port collide, and the box already
# has claims on 25694-25705 (selfcheck), 25698 (chunk-batch-probe), 25801+
# (bench-servers), 25896/25897 (t2-gameplay-subset) and 25911-25915 (boot
# benches). 25750 is a gap between the probe and bench-servers nothing else
# in `ci/` uses.
pick_port() {
    python3 - <<'PY'
import socket, sys
def free(p):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(("0.0.0.0", p))
        return True
    except OSError:
        return False
    finally:
        s.close()
for p in range(25750, 25800):
    if free(p):
        print(p)
        sys.exit(0)
sys.exit(1)
PY
}
PORT="${RUSTMC_RECIPE_DUMP_PORT:-}"
if [ -z "$PORT" ]; then
    PORT="$(pick_port)" || {
        echo "dump-recipe-unlocks: no free port in 25750-25799 — override with RUSTMC_RECIPE_DUMP_PORT" >&2
        exit 1
    }
fi
DIR="$ROOT/oracle-work/recipe-dump"
rm -rf "$DIR"
cleanup() { "$ROOT/ci/vanilla-server.sh" stop "$DIR" >/dev/null 2>&1 || true; }
trap 'cleanup; rm -rf "$WORK"' EXIT

# **`gamemode=creative`, and this is not optional.** `vanilla-server.sh`
# boots survival by default, and `ServerGamePacketListenerImpl.
# handleSetCreativeModeSlot` refuses the whole packet outright when
# `!player.isCreative()` — silently, no error, nothing in the log. The first
# version of this script omitted it: every one of 236 non-ambiguous
# candidates held nothing, because vanilla never put the item in the
# inventory to trigger anything from. Matched to `ci/t2-gameplay.sh`'s own
# `VANILLA_EXTRA_PROPS` so the substrate is the one the corpus already
# measures against, not a second one with its own quirks.
VANILLA_EXTRA_PROPS=$'gamemode=creative\ngenerate-structures=false\nspawn-monsters=false\ndifficulty=peaceful\nspawn-radius=0' \
"$ROOT/ci/vanilla-server.sh" start "$DIR" "$PORT" "minecraft:flat"

cargo build -q -p oracle-runner
BIN="$ROOT/target/debug/oracle-runner"

DISPLAYS="$WORK/displays.json"
"$BIN" dump-recipe-displays "127.0.0.1:$PORT" "$CANDIDATES" --out "$DISPLAYS"

cleanup
trap 'rm -rf "$WORK"' EXIT

# --- merge: by_item keeps only what was actually captured ------------------
python3 - "$CANDIDATES" "$DISPLAYS" "$OUT" <<'PY'
import json, os, sys

candidates_path, displays_path, out_path = sys.argv[1:4]
candidates = json.load(open(candidates_path))
displays = json.load(open(displays_path))

by_item = {}
for item, recipes in candidates.items():
    if len(recipes) == 1 and recipes[0] in displays:
        by_item[item] = recipes

os.makedirs(os.path.dirname(out_path), exist_ok=True)
with open(out_path, "w") as fh:
    json.dump({"by_item": dict(sorted(by_item.items())), "displays": displays}, fh, indent=0)
print(f"{len(by_item)} items, {len(displays)} captured recipe display(s) -> {out_path}")
PY
