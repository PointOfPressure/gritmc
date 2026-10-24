#!/usr/bin/env bash
# item-props-golden.sh — every item's default data components, off the pinned jar.
#
# See `tools/goldengen/vanilla/ItemPropsGolden.java` for what is walked and
# why it is one table rather than five. The short version: in 26.2
# `Item.Properties` is a builder over `DataComponentMap`, so max stack size,
# max damage, rarity, food, tool and equippable are all the same map read at
# different keys, and `Item.getDefaultMaxStackSize` reads it through
# `components().getOrDefault(MAX_STACK_SIZE, 1)`.
#
# Run twice and `cmp`ed, like `ci/block-items-golden.sh` and for the same
# reason: this is a registry walk with an imposed component order and has no
# business varying between runs, so a difference is a defect in the dumper.
#
# The dumper writes the JSON to a path given on its command line rather than
# to stdout, so nothing here has to recover a table from log4j's `[STDOUT]: `
# prefix. Its stdout carries one `WROTE` line, and that line is the assertion
# that the walk ran at all.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$ROOT/data/vanilla/item_props.json}"
VER="$(grep -oP 'GAME_VERSION: &str = "\K[^"]+' "$ROOT/crates/rustmc-proto/src/lib.rs")"
VDIR="$ROOT/oracle-work/vanilla-$VER"
JAR="$VDIR/versions/$VER/server-$VER.jar"
[ -s "$JAR" ] || { echo "no jar at $JAR" >&2; exit 1; }
CP="$JAR:$(find "$VDIR/libraries" -name '*.jar' | tr '\n' ':')"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

javac -nowarn -cp "$CP" -d "$WORK" "$ROOT/tools/goldengen/vanilla/ItemPropsGolden.java" \
    2>&1 | grep -v '^Note' || true

run() {
    # stderr is kept, not discarded: a component this dumper cannot encode is
    # recorded as a refusal inside the table, which is the design, but a
    # failure to bind components at all is a top-level throw and the two look
    # nothing alike.
    java -Drustmc.data="$VDIR/generated-data" -cp "$CP:$WORK" ItemPropsGolden "$1" \
        > "$1.out" 2> "$1.log" || {
        echo "ItemPropsGolden failed; the last of what it said:" >&2
        tail -30 "$1.log" >&2
        return 1
    }
    grep -q 'WROTE' "$1.out" || {
        echo "ItemPropsGolden printed no WROTE line — the walk did not finish" >&2
        tail -20 "$1.out" >&2
        return 1
    }
}
run "$WORK/a.json"
run "$WORK/b.json"
cmp -s "$WORK/a.json" "$WORK/b.json" || {
    echo "REFUSED  two runs of the component walk differ" >&2
    echo "  sizes: $(stat -c%s "$WORK/a.json") and $(stat -c%s "$WORK/b.json")" >&2
    exit 3
}

mkdir -p "$(dirname "$OUT")"
python3 - "$WORK/a.json" "$OUT" <<'PY'
import json, sys

doc = json.load(open(sys.argv[1]))
table = doc["table"]

# **Refuse an implausible walk rather than write it.** The three assertions
# below are the ones whose failure would otherwise land as a silently wrong
# table downstream, and each names a real quantity rather than "not empty":
#
#  * the row count must match the count the dumper reports about itself. A
#    walk that threw halfway writes a short table and a matching short count
#    only if it also lied about the count, which it cannot: the count is
#    incremented in the same loop.
#  * every item must carry a `minecraft:max_stack_size`. `Item.Properties`'s
#    constructor writes it unconditionally, so an item without one means the
#    binding did not run for that item and its stack size would silently fall
#    back to a default on our side.
#  * one item at each of the three stack sizes vanilla actually uses.
#    `minecraft:stone` is 64, `minecraft:bucket` is **16** and
#    `minecraft:water_bucket` is 1. The whole reason this table exists is
#    that `crates/rustmc-server/src/item.rs` assumed 64 for everything, so a
#    dumper that came back all-64 would have reproduced the bug it is here to
#    fix and this is where that is caught. Three points and not one because
#    the histogram has exactly three values (1242 / 49 / 246 at 64 / 16 / 1,
#    measured 2026-09-05 on the pinned 26.2 jar) and a single 64 probe would
#    pass against a dumper that had lost the component map entirely — 64 is
#    also `Item.Properties`'s default.
#
#    **The 16 was a corrected expectation, not a guess that happened to
#    work.** This check was first written asserting `minecraft:bucket == 1`
#    from recall; the dumper refused, and the refusal was right. An empty
#    bucket stacks to 16 in 26.2 and only a *filled* one is 1.
if not table:
    raise SystemExit("REFUSED  the component walk produced no items")
if len(table) != doc["items"]:
    raise SystemExit(f"REFUSED  {len(table)} rows against a reported {doc['items']}")

rows = {r["item"]: r["components"] for r in table}
missing = [i for i, c in rows.items() if "minecraft:max_stack_size" not in c]
if missing:
    raise SystemExit(
        f"REFUSED  {len(missing)} items carry no minecraft:max_stack_size, "
        f"first {missing[:5]}"
    )

MAX = "minecraft:max_stack_size"
for item, expect in (
    ("minecraft:stone", 64),
    ("minecraft:bucket", 16),
    ("minecraft:water_bucket", 1),
):
    got = rows.get(item, {}).get(MAX)
    if got != expect:
        raise SystemExit(
            f"REFUSED  {item} stacks to {got}, not {expect} — this table's "
            f"whole purpose is that stack sizes are not all 64"
        )

classes = {}
for r in table:
    classes[r["class"]] = classes.get(r["class"], 0) + 1

# **The positive control for the block-tag binding.** An unbound tag resolves
# to an empty HolderSet and throws nothing, so without this the table would
# say every tool mines everything at speed 1.0 and drops nothing — a plausible
# answer, uniformly wrong. Measured before `bindBlockTags` existed: 16 of 17
# sets empty. Two sets are *legitimately* empty in 26.2 —
# `incorrect_for_diamond_tool` and `incorrect_for_netherite_tool`, because
# those tiers mine everything — so "no empty set" is the wrong assertion and
# this names a block instead.
sets = doc["tool_block_sets"]
PICKAXE = '"#minecraft:mineable/pickaxe"'
if PICKAXE not in sets:
    raise SystemExit(f"REFUSED  no {PICKAXE} rule anywhere in the registry")
if "minecraft:stone" not in sets[PICKAXE]:
    raise SystemExit(
        f"REFUSED  {PICKAXE} resolved to {len(sets[PICKAXE])} blocks and stone is not "
        f"among them — the block tags are not bound and every tool rule is empty"
    )
empty = sorted(k for k, v in sets.items() if not v)

sizes = {}
for c in rows.values():
    sizes[c[MAX]] = sizes.get(c[MAX], 0) + 1
comp_names = sorted({k for c in rows.values() for k in c})

json.dump(
    {
        "what": "Item.components() — the DataComponentMap Item.Properties builds, per item",
        "items": doc["items"],
        "componentsWritten": doc["components"],
        "componentTypesInRegistry": doc["component_types"],
        "componentTypesSeen": len(comp_names),
        "stackSizeHistogram": {str(k): v for k, v in sorted(sizes.items())},
        "classHistogram": dict(sorted(classes.items(), key=lambda kv: (-kv[1], kv[0]))),
        "refusals": doc["refusals"],
        # `LivingEntity.entityEventForEquipmentBreak(slot)`, invoked once per
        # slot in the dumper rather than read off its `tableswitch`: that
        # switch runs over a `$SwitchMap` whose numbering is the compiler's,
        # so which arm is MAINHAND cannot be read from the listing at all.
        "equipmentBreakEvents": doc["equipmentBreakEvents"],
        # The registry id of every data component type, which is what
        # `DataComponentPatch`'s stream codec writes for each key. All 111
        # rather than the 59 with a default, because a patch names components
        # no prototype carries.
        "componentTypeIds": doc["componentTypeIds"],
        # The resolved HolderSets, so a consumer of `minecraft:tool` needs no
        # block-tag machinery. Passed through rather than recomputed.
        "toolBlockSets": doc["tool_block_sets"],
        # Recorded rather than dropped: a set that becomes empty at the next
        # version bump is a finding, and the two that are empty today are
        # empty because diamond and netherite mine everything.
        "emptyToolBlockSets": empty,
        "componentTypes": comp_names,
        "table": table,
    },
    open(sys.argv[2], "w"),
    indent=0,
)
print(
    f"item props -> {sys.argv[2]} ({doc['items']} items, {doc['components']} components, "
    f"{len(comp_names)} of {doc['component_types']} component types seen, "
    f"{len(doc['refusals'])} refusals, stack sizes {dict(sorted(sizes.items()))}, "
    f"{len(classes)} item classes, "
    f"{len(doc['tool_block_sets'])} tool block sets, {len(empty)} of them empty)"
)
PY
