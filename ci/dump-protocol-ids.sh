#!/usr/bin/env bash
# dump-protocol-ids.sh — numeric registry ids that appear on the wire but are
# never sent to a client, from vanilla's own report.
#
# # Which registries, and why these three together
#
# `data/vanilla/registries.bin` is the capture of the `registry_data` packets
# vanilla sends during configuration. **Only datapack-driven registries are in
# it.** The built-in ones — blocks, items, entity types, menu types — are
# compiled into the client and never travel, so a server that needs one of
# their numbers has to read it from the generator's report instead. That is
# already why `ci/dump-entity-types.sh` exists; this is the same rule applied
# to the registries the container and block-entity channels need:
#
#   * `minecraft:menu` — `ClientboundOpenScreenPacket` writes its `MenuType`
#     through `ByteBufCodecs.registry(Registries.MENU)`, a plain varint of the
#     registry id. Without it a server cannot say *which* screen opened, and a
#     client shown the wrong number renders the wrong container.
#   * `minecraft:block` — `ClientboundBlockEventPacket` writes the **block**,
#     not the block *state*: `Block.getId(block)`, again a varint of a
#     registry id. Everything else this server puts on the wire uses the
#     global *state* id out of `block_props.json`, and the two are different
#     numbers for the same block — `minecraft:chest` is block 201 and its
#     states start at a five-figure id. Reusing a state id here would have
#     named a different block and the packet would have been dropped.
#   * `minecraft:sound_event` — `ClientboundSoundPacket` writes the event as a
#     `Holder`, which on the wire is `registry id + 1` (0 meaning an inline
#     event follows). It is the same situation as the three below for the same
#     reason — built into the client, never sent — and it is a **fourth** id
#     space: `minecraft:block.chest.open` is sound 349 and has nothing to do
#     with `minecraft:chest` being block 201.
#   * `minecraft:block_entity_type` —
#     `ClientboundBlockEntityDataPacket.STREAM_CODEC` writes its type through
#     `ByteBufCodecs.registry(Registries.BLOCK_ENTITY_TYPE)`, again a plain
#     varint. It is a **third** id space over the same objects: a sign is
#     block-entity type 7, its block `minecraft:oak_sign` is block 84, and its
#     states are five-figure. All three are small integers and none of them is
#     interchangeable with the others.
#
# # The cross-check that makes the block half worth trusting
#
# The block ids are derivable a second way without this file, and were derived
# that way first: `Block.BLOCK_STATE_REGISTRY` is filled by walking
# `BuiltInRegistries.BLOCK` in registry order and adding every state of each
# block, so a block's registry id is its **first-appearance ordinal** in
# `block_props.json`'s state table. Computed that way on 2026-08-04 over all
# 32,366 states: 1,196 blocks, `air` 0, `stone` 1, `chest` 201,
# `crafting_table` 206, `furnace` 209 — identical to this report's
# `protocol_id` for every one of them.
#
# Two independent derivations agreeing is why the report is used rather than
# the inference: the inference is now known to be sound, and the report is
# still the primary source, so the numbers do not depend on a claim about how
# a registry is bootstrapped.
#
# The output is gitignored like the rest of `data/vanilla/`. Regenerate on a
# version bump.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VER="${RUSTMC_VANILLA_VERSION:-26.2}"
REPORT="$ROOT/oracle-work/vanilla-$VER/generated/reports/registries.json"
OUT="${1:-$ROOT/data/vanilla/protocol_ids.json}"

if [ ! -f "$REPORT" ]; then
    echo "dump-protocol-ids: no report at $REPORT — run ci/vanilla-server.sh reports first" >&2
    exit 1
fi

python3 - "$REPORT" "$OUT" <<'PY'
import json, sys
report, out = sys.argv[1], sys.argv[2]
with open(report) as f:
    doc = json.load(f)

def ids(registry):
    entries = doc[registry]["entries"]
    # Sorted by id, so a diff between two versions reads as a diff rather than
    # as a reshuffle: the ids are what the wire carries and the order is what
    # moved.
    return dict(sorted(
        ((name, e["protocol_id"]) for name, e in entries.items()),
        key=lambda kv: kv[1],
    ))

menus, blocks = ids("minecraft:menu"), ids("minecraft:block")
block_entities = ids("minecraft:block_entity_type")
sounds = ids("minecraft:sound_event")
# `ClientboundUpdateAttributesPacket` writes each attribute through
# `Attribute.STREAM_CODEC` = `ByteBufCodecs.holderRegistry(ATTRIBUTE)`, a
# plain varint of the registry id. A fifth built-in id space over names
# that share nothing with the others.
attributes = ids("minecraft:attribute")
# `PotionContents` and `SuspiciousStewEffects` name a potion and a mob effect
# through `ByteBufCodecs.holderRegistry`, a plain varint of these built-in
# registries' ids (`crate::patch::HolderIds`).
potions = ids("minecraft:potion")
mob_effects = ids("minecraft:mob_effect")
with open(out, "w") as f:
    json.dump(
        {
            "menus": menus,
            "blocks": blocks,
            "block_entity_types": block_entities,
            "sounds": sounds,
            "attributes": attributes,
            "potions": potions,
            "mob_effects": mob_effects,
        },
        f,
        indent=0,
    )
print(
    f"{len(menus)} menu types, {len(blocks)} blocks, "
    f"{len(block_entities)} block entity types, {len(sounds)} sound events, "
    f"{len(attributes)} attributes, {len(potions)} potions, "
    f"{len(mob_effects)} mob effects -> {out}"
)
PY
