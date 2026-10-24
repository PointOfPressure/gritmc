### COLD START — branch `items`, 2026-09-05

The item area: vanilla 26.2's item behaviour classes and data components,
transcribed into `rustmc-server` on top of `main`.

#### What "items" meant on `main` before this branch

Placement, and only placement. `use_item_on` decides *where* a block goes,
`set_creative_mode_slot` says *what* is held, `data/vanilla/items.json` maps
the wire's varint to a name, and `crate::itemuse` had two `ItemStack.useOn`
overrides (`HoeItem`, `FlintAndSteelItem`). Every question about what an item
*is* was answered by `crate::item::DEFAULT_MAX_STACK_SIZE = 64`, whose own doc
comment said so and named the fix.

#### What this branch adds

**One table.** `ci/item-props-golden.sh` +
`tools/goldengen/vanilla/ItemPropsGolden.java` write
`data/vanilla/item_props.json` from `Item.components()`: 1,537 items, 19,356
components, 59 of 111 component types, 0 refusals, plus each item's Java class
and the 17 resolved `HolderSet`s the `minecraft:tool` rules name. Registered
in `ci/gen-gameplay-tables.sh` (22 tables now, was 21); ~10 s, one JVM, no
world.

**Five things that read it** — `crate::itemprops` (the loader and the
transcriptions), `crate::player::Hotbar` (pickup), `crate::click::Slots`
(containers), `crate::blockitem` (the class dispatch),
`crate::session_tables` (the boot line).

**Four transcriptions from bytecode**: `Tool.getMiningSpeed` /
`isCorrectForDrops`, `Item.use`'s four-component ladder, and three
`BlockItem` overrides in the new `crate::blockitem` —
`DoubleHighBlockItem.placeBlock` (`clear_above`, 28 items),
`PlaceOnWaterBlockItem`/`GameMasterBlockItem` (`places_from_use_on`, 2 + 7)
and `BedItem.placeBlock` (`place_flags`, 16).

**Four new `useOn` overrides**: `ShovelItem`, `AxeItem` (strip only),
`ShearsItem`, `FireChargeItem`. `crate::itemuse` now covers six classes, 24
items.

**One gap named at its call site**: `doClick` never asks
`overrideStackedOnOther` / `overrideOtherStackedOnMe`, which is 17 items, all
`BundleItem`.

#### The four findings that would cost a re-derivation

1. **26.2 has no `SwordItem`, `PickaxeItem`, `DiggerItem` or `TieredItem`.**
   Swords and pickaxes are plain `Item`; mining and attack are the
   `minecraft:tool` and `minecraft:weapon` components. This refutes the port
   order this lane was briefed with.
2. **`Tool` is walked twice, not once.** `getMiningSpeed` and
   `isCorrectForDrops` each skip the rules that lack their own `Optional`
   field, so a **golden pickaxe mines obsidian at 12.0 and drops nothing**. A
   fused loop gives one of those answers and never both.
3. **An unbound block tag says no rather than throwing.** Before
   `ItemPropsGolden.bindBlockTags`, 16 of 17 tool `HolderSet`s resolved to
   zero blocks and the table said every tool mines everything at 1.0. The
   binding must be **recursive**: `tags/block/mineable/*.json` is a
   subdirectory and `SurfaceChunkGolden.bindBlockTags` is a flat
   `Files.list`.
4. **Three placement numbers, not one.** `BlockItem.placeBlock` writes with
   11, `setBlockAndUpdate` with 3 and `BedItem.placeBlock` with **26** — no
   `UPDATE_NEIGHBORS` and `UPDATE_KNOWN_SHAPE` set, so a bed's foot runs
   neither the neighbour update nor the shape cascade. Flags are a per-effect
   field here for that reason, and `FireChargeItem` differs from
   `FlintAndSteelItem` on exactly this axis while doing the same thing.
5. **`DoubleHighBlockItem.placeBlock` is a wire divergence with no world
   divergence.** It clears the block above with flags 27 before the lower half
   goes down, and `setPlacedBy` overwrites that position anyway — so no test
   that reads the finished world can see it, and the packet-comparing corpus
   can.

#### Killed as nulls — do not re-run these

* **A corpus scenario cannot be registered from a lane worktree today.** The
  only recordings on the box are `oracle-work/gameplay/{vanilla,rustmc}.json`
  from 2026-08-08. A `--sweep` files a new check at `tier = 0,
  why = "not-in-recording"` **and** rewrites the ledger's own counters against
  that stale universe: it reported `UNASKABLE 19 (ceiling 12)` and
  `FIELD-UNIVERSE ledger says 66 recorded pairs, this file pins 90`, neither
  about the new check. The scenario written for the shovel is held below
  rather than committed; it needs a corpus run first, which is the chain's
  job.
* **Item attack damage has no consumer.** `crate::attack` stops at the
  `nodamage` sound because both corpus players are creative and a creative
  player is invulnerable (`docs/PARITY-ROADMAP.md` §20.5). Porting
  `minecraft:weapon` or the `attack_damage` attribute modifier changes nothing
  observable.
* **`AxeItem`'s scrape and wax-off, and `HoneycombItem`, all end in
  `Block.withPropertiesOf(state)`** — a general cross-block property copy over
  the copper stairs/slabs/doors/grates/bulbs family. Neither
  `WeatheringCopper`'s map nor `WAX_OFF_BY_BLOCK` is extracted here and
  `withPropertiesOf` has no analogue, so all three return `PASS`, which is
  what those items already did.
* **`AxeItem.playerHasBlockingItemUseIntent` is one item wide.** Exactly one
  item of 1,537 carries `minecraft:blocks_attacks`: `minecraft:shield`.
* **`Tool.defaultMiningSpeed` has no witness in the extract** — no 26.2 item
  writes the field — so the real table cannot separate a correct read of it
  from a hard-coded 1.0. Same for `ItemStack.isDamageableItem`'s
  `!has(UNBREAKABLE)`: zero items carry a default `minecraft:unbreakable`.
  Both are covered by synthetic rows and neither can be covered any other way.
* **Everything left in the class census is blocked on one of five missing
  subsystems, and the blocker is the same for whole families.** No
  `canSurvive`: `StandingAndWallBlockItem.canPlace` and its `SignItem` /
  `BannerItem` inheritors, 48 items, and `HangingSignItem.canPlace`. No
  `BonemealableBlock`: `BoneMealItem`, 438 lines of bytecode. No component
  patch on a stack: `BundleItem`, 17. No durability: every `hurtAndBreak` and
  `Tool.damage_per_block`. No `use_item` packet: `Item.use`'s four arms.
  `SmithingTemplateItem` (19 items) is the one large class that is **inert by
  construction** — `javap -p` shows a bare class body — rather than unported.
* **`SpawnEggItem` (88 items) is refused as briefed** — no mobs spawn yet.
  `BoatItem` (20), `MinecartItem` (6), `ItemFrameItem` (2), `LeadItem`,
  `NameTagItem`, `ArmorStandItem`, `EndCrystalItem` and `DyeItem`'s
  `interactLivingEntity` are refused for the same reason: they all end in an
  entity this server does not model.

#### The single next item, and the result that closes it

**Decode `minecraft:use_item` (`ids::play::sb::USE_ITEM`, id 67).** No file
under `crates/rustmc-server/` decodes it, so right-click-with-nothing-aimed is
entirely unhandled and `Item.use` has no caller. The dispatch is already
transcribed and asserted — `ItemProps::use_arm`, 1,411 Pass / 75 Equippable /
43 Consumable / 7 KineticWeapon / 1 BlocksAttacks — so the port is packet
decode plus arm bodies, not a re-derivation. It closes when a scenario that
right-clicks air holding a spear observes the `KineticWeapon.makeSound` that
vanilla sends and this server does not.

#### Held: the scenario the shovel wants, once a recording exists

Drop this at `ci/checks/gameplay/312-a-shovel-turns-grass-into-a-path.toml`
**in the same run as a corpus recording**, then sweep and commit the ledger.

```toml
name = "a-shovel-turns-grass-into-a-path"
about = "a shovel used on grass turns it into a dirt path — the second ITEM-decides-the-block check, and the one that separates ShovelItem's FLATTENABLES from HoeItem's TILLABLES on the same block: a hoe makes farmland of grass and a shovel makes a path of it, so a dispatch that routed both to one table would pass 310 and fail this"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "hold"
item = "minecraft:iron_shovel"

[[step]]
do = "place"
on = [1, -1, 0]
face = "up"
```
