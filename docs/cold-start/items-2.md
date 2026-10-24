### COLD START — branch `items-2`, 2026-09-05

The second item lane: the five things `docs/cold-start/items.md` refused and
named. All five were opened; three are ported and gated, one is ported and
cannot be wired, one is framed and has three payloads.

#### What each of the five came to

| refused by `items` | what happened here |
| --- | --- |
| `canSurvive` placement, 48 items | **ported and gated** — 19 families, 640,532 of vanilla's own answers, 0 disagreements. The 48 are a small part of it: 311 of 1,115 blocks have a rule. |
| `BonemealableBlock` | **census extracted, five of 63 ported** — the crops. The other 58 answer `Unported`. |
| item durability and damage | **ported and gated, NOT wired** — see the null below. Two independent reasons, both measured. |
| the component patch on a stack | **framed and gated, three payloads** — `damage`, `bundle_contents`, `custom_name`. |
| the `use_item` packet | **decoded and dispatched.** The brief's description of its body was wrong; see below. |

#### Three new tables, and one column that was a lie

* **`data/vanilla/item_survive.json`** (`ci/item-survive-golden.sh`) — 1,115
  blocks, 6,600 states, 79 neighbourhoods, 2 light levels, **1,042,800
  answers, 0 refused**, 1.85 MB, 14 s, two runs identical. A neighbourhood is
  one non-air block at one `Direction` offset with air everywhere else, so
  every cell is a fact and not a refusal — unlike `SurvivalGolden`'s reader,
  which throws outside one position.
* **`data/vanilla/bonemeal.json`** (`ci/bonemeal-golden.sh`) — 63 blocks, 45
  classes, 721 states, 303 valid targets, 0 refused.
* **`data/vanilla/block_props.json`** gained `rigidFaces`, 14 `placeTags`
  bits and one `placeFluidTags` bit, all order-checked at load the way
  `directionOrder` is.
* **`unstableBottomCenter` was `0` for all 32,366 states** and had been since
  it was written, because `BlockPropsGolden` never bound block tags. 384 now.
  Its doc comment described a refusal no row implemented.

#### The five findings that would cost a re-derivation

1. **The `canSurvive` dispatch key is two classes.**
   `VegetationBlock.canSurvive` is `mayPlaceOn(below)` and nothing else, and
   eleven classes override `mayPlaceOn` while inheriting `canSurvive`. Keyed
   on one class a port is wrong for `lily_pad`, `dead_bush` and
   `cactus_flower`. `item_survive.json` carries both.
2. **Two `$SwitchMap` tableswitches in this area cannot be read off their
   listings, and both were read wrong first.**
   `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection` points
   *away* from the support (a floor button's is UP) — 8,784 cells said so.
   `LivingEntity.entityEventForEquipmentBreak` is worse: **three of its eight
   arms** differ from the listing's arm order paired with
   `EquipmentSlot.values()`, and the two that agree are the two a MAINHAND
   test would have checked. Anything behind a `$SwitchMap` needs invoking,
   not reading.
3. **`TorchflowerCropBlock.getBonemealAgeIncrease` draws no RNG.** It is
   `iconst_1; ireturn`. Routing every crop through `CropBlock`'s
   `Mth.nextInt(random, 2, 5)` takes one draw too many and desynchronises
   every later draw on that level's source. `BeetrootBlock`'s is `super / 3`
   on the same single draw, so a beetroot fails one bone meal in four and
   **still consumes it** — `growCrop`'s `shrink` is above the success test.
4. **`0, 0` on a stack does not mean "no components".** It means "exactly the
   prototype's", so every stack this server has ever sent claims to be fresh.
   `DataComponentPatch` has **no length prefix** around a payload, so a
   component whose codec a port has not read must not be written at all — a
   wrong payload desynchronises the rest of the packet rather than corrupting
   one field.
5. **`use_item` (67) carries no `BlockHitResult`.** Four fields: hand,
   sequence, two floats. This lane's brief said otherwise. A decoder written
   from `use_item_on` consumes 22 bytes of a 10-byte body.

#### Killed as nulls — do not re-run these

* **Durability cannot be wired into the dig, and creative is not the only
  reason.** `ServerPlayerGameMode.destroyBlock` offsets 190-200:
  `if (player.preventsBlockDrops()) return true` — which is `isCreative()` —
  sits **above** the `mineBlock` call at 241, so in creative the call is not
  reached at all and not even the statistic is awarded. That is independent
  of `processDurabilityChange`'s own `hasInfiniteMaterials` check. Both
  corpus players are creative (`docs/PARITY-ROADMAP.md` §20.5), so no
  scenario in `ci/checks/gameplay/` can observe a tool wearing down;
  `ci/t2-gameplay-survival.sh` is the pass that could.
* **And it needs a table column that does not exist.** `Item.mineBlock` gates
  on `state.getDestroySpeed(level, pos) != 0`, which is
  `BlockBehaviour.destroySpeed`, a per-block float. Nothing under `crates/`
  or in `data/vanilla/` carries it — `block_props.json` has nine face and
  material columns and not this one. `crate::durability::mine_block_damage`
  takes it as a parameter and has no caller for that reason.
* **A corpus scenario still cannot be registered from a lane worktree**, and
  the reason has changed since `docs/cold-start/items.md` recorded it. There
  are recordings on the box now — `~/rustmc-stack/oracle-work/gameplay/` was
  written today — but a **new** check has no rows in any of them, so
  `--sweep` files it at `tier = 0, why = "not-in-recording"` exactly as
  before. The three scenarios are held below rather than committed, because
  an unregistered `ci/checks/gameplay/*.toml` is a T0 red.
* **`bundle_contents` cannot hold a bundle.** `crate::patch` writes a nested
  stack's patch as `0, 0` and refuses to recurse, because the inner call
  would need the same `type_id` resolver the outer one has and writing it
  wrong is worse than not writing it.
* **`enchantments` is not portable here.** `EnchantmentHelper.processDurabilityChange`
  runs a `MutableFloat` through `runIterationOnItem`, so Unbreaking is a
  function of the stack's enchantment component **and** of each enchantment's
  own value-effect definition. `hurt_and_break` takes the amount after that
  step; the caller passes the raw amount, which is exact for every unenchanted
  item — every item this server can hold.
* **The 58 unported `BonemealableBlock`s are not effort.** Most place a
  **configured feature** in `performBonemeal` — a sapling grows a tree,
  nylium grows a nether forest patch — which is worldgen reached through an
  item, drawing from `Level.getRandom()` in an order this lane has not
  derived. A wrong tree is a worse answer than no tree.
* **Six blocks read an instance-field tag and are refused for that.**
  `StemBlock`, `NetherFungusBlock` and `NetherRootsBlock` reach
  `getfield stemSupportBlocks` / `supportBlocks`, so the tag is a constructor
  argument and cannot be named from the class. They need a per-block tag
  column, which nothing has asked for.

#### One thing fixed that is not this lane's

`ci/command-parse-golden.sh` **could not succeed on any tree, ever**: it
recorded `String.valueOf(e.getType())`, which is an identity hash, and the
script dumps twice and `cmp`s. Because `ci/gen-gameplay-tables.sh` exits on
the first failing generator, **no table listed after `command_parse.json`
could be regenerated anywhere on the box** — which is how eight `block_props`
tests came back red on this branch against a copy of the table that predated
its own new column. One line, and it is the line the comment above it already
described.

#### The single next item, and the result that closes it

**Add `destroySpeed` to `block_props.json` and wire
`crate::durability::mine_block_damage` into `handle_player_action`'s destroy
arm, behind the `preventsBlockDrops` gate.** Everything else is in place: the
rule is transcribed and gated, `crate::patch::Component::Damage` writes the
component, and `crate::menu::write_patched_item` puts it in a
`container_set_slot`. It closes when `ci/t2-gameplay-survival.sh` records a
shovel used on grass and the `container_set_slot` that follows carries
`added = 1, type 3, value 1` where this server sends `0, 0`.

#### Held: three scenarios, for the run that has a recording

Drop these into `ci/checks/gameplay/` **in the same run as a corpus
recording**, then
`ci/corpus-falsifiable.py --sweep --bin target/release/oracle-runner
oracle-work/gameplay/vanilla.json oracle-work/gameplay/rustmc.json` and commit
the ledger. The first two are the ones this branch's code can pass today; the
third needs the survival pass and the next item above.

```toml
# 313-a-torch-on-air-is-refused.toml
name = "a-torch-on-air-is-refused"
about = "a torch aimed at a block face with nothing under it is not placed — BlockItem.canPlace's canSurvive gate, which this server never asked. It is the first check here whose expected outcome is SILENCE from a click that used to write a block, so it is also the first that would pass against a server which had simply stopped placing anything; 314 beside it is the control that separates the two"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "hold"
item = "minecraft:torch"

# The side face of a block with air beneath the position the torch would go
# into: a wall torch needs the block behind it and a standing one needs
# support under it, and this geometry gives neither.
[[step]]
do = "place"
on = [2, 2, 1]
face = "up"
```

```toml
# 314-a-torch-on-stone-is-placed.toml
name = "a-torch-on-stone-is-placed"
about = "the same torch on a block that can hold it — the control for 313. Without it a server that had stopped placing torches altogether would pass 313, which is the one-sided-refusal shape this corpus has been caught by before"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "hold"
item = "minecraft:torch"

[[step]]
do = "place"
on = [1, -1, 1]
face = "up"
```

```toml
# 315-bone-meal-grows-a-crop.toml
name = "bone-meal-grows-a-crop"
about = "bone meal used on wheat writes a new age and sends level_event 1505 — the first check here where an item BOTH writes a block and sends a level event, in that order, and the order is the finding: performBonemeal is inside growCrop and the event is two calls above it in useOn. The age it grows to is a random draw, so the check reads the block_update's presence and the level_event, not the state"
observe_from = "actor"
observe = ["block_update", "section_blocks_update", "level_event", "block_changed_ack"]

[[setup]]
do = "look"
yaw = 0.0
pitch = 0.0

[[setup]]
do = "setblock"
at = [1, -1, 1]
block = "minecraft:farmland"

[[setup]]
do = "setblock"
at = [1, 0, 1]
block = "minecraft:wheat[age=0]"

[[setup]]
do = "hold"
item = "minecraft:bone_meal"

[[step]]
do = "place"
on = [1, 0, 1]
face = "up"
```
