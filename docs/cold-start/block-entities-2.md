# Cold start — branch `block-entities-2`

---

### COLD START — branch `block-entities-2` off `f4fa28ff8`, 2026-09-05

**All forty-nine block-entity types are registered and their saved state is
checked against the bootstrapped registry rather than against a second copy of
itself.** Remote dir `~/rustmc-be2` on the Threadripper.

    Kind                14 types  ->  49        (registry, both directions)
    for_state          132 blocks -> 186        (BlockEntityType.isValid)
    container_size        3 kinds ->  16        (getContainerSize / NonNullList.withSize)
    saved NBT keys        0       ->  86        (name, tag id AND rendered value)
    types whose fresh saved compound matches vanilla exactly:  47 of 47 dumpable

The last line is the one that matters. `ci/block-entity-golden.sh` dumps
`saveWithFullMetadata` on a freshly constructed instance of every type — 47 of
49; `minecraft:piston` and `minecraft:vault` throw in a bootstrapped-only JVM —
and `a_fresh_block_entity_saves_vanillas_keys_at_vanillas_tag_widths` compares
that to `BlockEntity::to_nbt` as a set of `(name, Tag.getId(), rendered value)`.
`OUTSTANDING_SAVE_KEYS`, the ratchet that exists to hold the differences, is
**empty**, and it fails in both directions: a new difference reds, and an entry
that has stopped being one reds too.

**The brief's premise was wrong and the tree said so first.** It said `main`
carried 3 of 49 (chest, furnace, sign) and the gap was 46. `main` carries 14 —
the eleven added 2026-08-12 for `block_entity_data` visibility — and the gap
was 35. `docs/SERVER.md`'s "Three block-entity types" bullet was a month stale
and is rewritten in this branch. The brief's port order also names a **bed**;
`minecraft:bed` is not a block-entity type in 26.2 and `BedBlock` is not an
`EntityBlock`, so there is nothing there to port.

## The instrument, and why it had to exist before the transcription

The block-to-type map is in **neither** generated report. It exists only as the
`Block...` varargs handed to `BlockEntityTypes.register`, and four of the
forty-nine registrations hand it a collection the bytecode never enumerates
(`Util.copyAndAdd(Blocks.COPPER_CHEST.asList(), Blocks.CHEST)`,
`Util.join` of two `ColorCollection.asList()`s, and so on). Transcribing it
means expanding those lists by hand, which is where a hand table goes wrong
quietly.

    tools/goldengen/vanilla/BlockEntityGolden.java
    ci/block-entity-golden.sh   ->  data/vanilla/block_entities.json
    registered in ci/gen-gameplay-tables.sh

bootstraps the registry and asks `BlockEntityType.isValid` of every block, then
instantiates each type for its container size, its `NonNullList` length and
whether any class up to `BlockEntity` declares `getUpdatePacket`. **49 types
over 186 blocks, 19 declaring an update packet, identical across two runs.**

It paid for itself immediately: `for_state` matched the literal
`"minecraft:chest"`, so all **eight copper chests** had a container, a menu
path and nowhere to keep an item.

**Its own first version carried this project's central defect and it is worth
knowing about.** The `updatePacket` column read `getUpdatePacket() != null` on
a fresh instance and came back **false for all forty-nine** —
`ClientboundBlockEntityDataPacket.create` goes through
`getUpdateTag(this.level.registryAccess())` and nothing there has a level, so
every call threw and every answer was the same one. A column with one value
distinguishes nothing. It is a *declaration* walk now, which needs no level.

**One key was missing from every block entity this server has ever saved.**
`BlockEntity.saveWithoutMetadata` ends with an unconditional
`store("components", DataComponentMap.CODEC, this.components)` — a `store`,
not a `storeNullable` — so all forty-nine write `components: {}` and we wrote
it for none. It is not in any `saveAdditional`, which is why reading the
per-class methods one at a time never found it and dumping the finished
compound found it in one pass.

**And the defaults are not zeroes.** `StructureBlockEntity.posY` starts at 1
where `posX` and `posZ` start at 0; `ignoreEntities` and `showboundingbox`
start true where `powered`, `showair` and `strict` start false; `integrity` is
a float and `seed` a long. A command block's `TrackOutput` and
`UpdateLastExecution` start true. A jigsaw's `name`, `pool` and `target` are
`minecraft:empty` and its `final_state` is `minecraft:air`, not "". A fresh
spawner saves `Delay: 20s`. `CrafterBlockEntity.triggered` is an **int** where
every other boolean-shaped field in the registry is a byte. Every one of those
is a place a plausible guess is wrong and a round-trip test sees nothing.

## What was found in our own source

* `containers::trigger_event` ignored `b0`. `ChestBlockEntity.triggerEvent`'s
  `if_icmpne` at offset 2 jumps to the `super` call at 23 and
  `RandomizableContainerBlockEntity` does not override it, so a chest is
  **false** for every `b0` but 1. The doc comment above the function said the
  opposite in so many words ("accepts unconditionally"). Seven types override
  the method, all seven gate on `b0 == 1`, and only the decorated pot reads
  `b1` — bounded by `WobbleStyle.values().length`, which is 2.
* `containers::has_openers_counter` was keyed on the block name and so said no
  for nine of the ten `ChestBlockEntity`-family blocks. Three classes hold a
  `ContainerOpenersCounter` and the **barrel's is a different mechanism**:
  `BarrelBlockEntity$1` calls `updateBlockState` where the chest's and the
  ender chest's call `signalOpenCount`/`Level.blockEvent`, so opening a barrel
  sends no `block_event` at all. Putting the barrel in that predicate would be
  a packet vanilla never sends, waiting for the day a barrel menu lands.

## What I killed as a null — do not re-run these

* **A placement scenario for any of the thirty-five new types is refused, not
  unwritten.** Twenty-seven of them inherit `BlockEntity.getUpdatePacket()`'s
  `aconst_null; areturn`, so vanilla puts nothing on the wire when one is
  placed and a scenario for it records `inert` — which is exactly what
  `ci/checks/gameplay/761`'s lectern half already demonstrates and why that
  half lives in the trace instead of carrying the step. Of the other eight,
  five are unreachable from this corpus (`GameMasterBlock`, or no placing
  item) and three redraw random state every run.
* **`getUpdatePacket() != null` is not a probe.** See above. Do not add that
  column back to any golden.
* **`minecraft:vault` cannot be instantiated without a server.**
  `VaultConfig.DEFAULT`'s class initialiser throws in a bootstrapped-only JVM.
  The extract records `"created": false` and the consumers skip that row
  rather than reading an absent answer as a zero; `javap` shows the class does
  declare `getUpdatePacket`, so the census is 20 and the machine-checkable
  figure is 19 + 1 unknown.

## The single next item, and the result that closes it

**A block-entity behaviour that is visible on the wire, and the cheapest one is
the bell.** `BellBlock.attemptToRing` is gated on
`getBlockEntity(pos) instanceof BellBlockEntity` — the mechanic literally does
not run without the block entity — and it produces two things this server can
already send: a `block_event(1, direction.get3DDataValue())` and a
`Level.playSound(null, pos, SoundEvents.BELL_BLOCK, BLOCKS, 2.0f, 1.0f)`.
`containers::trigger_event` already answers for it as of this branch.

What is missing is on the **block** side, not the block-entity side, and it is
a shape `useblock.rs` cannot currently express: `Ported { to, flags }` is one
replacement block state, and a bell writes no state at all. Closing this needs
a third result kind carrying a queued block event and a sound, which is
`useblock.rs`'s surface and belongs to whoever owns block interactions.

It closes when `ci/checks/gameplay/<n>-ringing-a-bell-...` reaches **tier 2**
in `ci/checks/gameplay-falsifiable.toml`. That needs a corpus recording, which
needs the box-wide lock, which this lane did not take.
