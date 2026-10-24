# COLD START — branch `structures-2`

Append-only. Newest entry last.

### COLD START — branch `structures-2`, off `train-g` (`0ad3cdfde`), 2026-10-02

`structures-mansion` and `structures-monument-city` (stacked, 14 commits) rebased
onto `origin/train-g`; the one conflict was this directory's
`structures-live.md`, both sides appended, kept both. Worktree `~/grit/work-smc`
(branch `structures-2`), gate worktree `~/grit/work-sm`, T0 dir `~/rustmc-sl2`,
dev dirs `~/rustmc-sl2x` and `~/rustmc-sl2w`. Harvests:
`~/rustmc-sl2w/oracle-work/sl2-<name>-{a,b}`; the vanilla world dirs behind them
(`gen-<seed>-<port>`, entities included) are still on disk.

## WHAT THIS BRANCH ADDS

**Block entities.** Every structure figure in this tree was a block-state diff,
and a block-state diff cannot see a chest's table, its seed or a banner's
patterns. `region-differ bediff` (vanilla first, dumps with
`RUSTMC_RECORD_BLOCK_ENTITIES=1`), filtered to the start's own saved `Children`
boxes, floor = vanilla-a against vanilla-b, before -> after, unit positions:

| structure | window | before | after | floor |
|---|---|---|---|---|
| end city | 20260722 (83,62) | 0/20 | 20/20 | 20/20 |
| end city with ship | 20260722 (81,3) | 4/6* | 6/6 | 6/6 |
| end city with ship | 20260722 (-176,-99) | — | 12/12 | 12/12 |
| woodland mansion | 8 (32,16) | 0/7 | 7/7 | 7/7 |
| woodland mansion | 30 (37,-47) | 0/36 | 36/36 | 36/36 |
| stronghold | 12345 (110,23) | 0/10 | 10/10 | 10/10 |
| trail ruins | 42 (2,-34) | 0/91 | 91/91 | 91/91 |
| trail ruins | 20260722 (55,0) | 0/90 | 90/90 | 90/90 |
| ancient city | 12345 (32,37) | 14/273 | 30/273 | 177/273 |
| cold ocean ruin | 12345 (65,67) | 0/8 | 8/8 | 8/8 |
| beached shipwreck | 31415926 (84,57) | 0/2 | 2/2 | 2/2 |
| igloo basement | 12345 (20,72), (52,116) | 1/4 | 4/4 | 4/4 |
| pillager outpost | 12345 (17,-60), 9 (10,-19) | 0/9 | 8/9 | 9/9 |

\* measured after the first emitter commit. Jungle temples and desert pyramids
put their chests and dispensers outside the saved box, so they are read
window-wide: jungle 8/8 and 9/9, desert pyramid 17/17 (was 9/17), buried
treasure in the wreck window 1/1. The census window (seed 12345 r16,
`ci/be-payload-r16-12345.sh`): exact where both have one 99/99 -> 109/109,
missing 74 -> 64, spurious 3 -> 3.

**Shapes.** A mirrored door cycles its hinge; a mirrored chest pair splits
(`ChestBlock.mirror` keeps `type`, and `ChestBlock.updateShape` is ported).
Seed 30's mansion in-box blocks 96 -> 54 against a floor of 19.

**The end city ship branch is verified** (`worldgen-dump end-city-starts`, two
twin harvests, layout 20/20 and 15/15, blocks at floor).

## WHAT IS STILL OPEN

1. **The deep dark's sculk.** Sculk sensors, shriekers and catalysts are now
   derived block entities, so the ancient city's in-box block entities read
   30/273 against a floor of 177/273 with 145 spurious (floor 113): every gap
   is a sculk BLOCK placed where vanilla has none or missing where it has one.
   This is the monument-city cold start's open item 1 and it is now visible
   in two instruments.
2. **Entities are listed, not spawned.** `worldgen_assembly::generate::structure_spawns`
   (and `worldgen-dump structure-spawns <overworld|end> <seed> <cx> <cz> <r>`)
   gives every evoker, vindicator, allay group, elder guardian, shulker and
   ship item frame, 117 of 117 where vanilla saved them across the eight
   windows (scratch `spawncmp.py` reads the vanilla `entities/` files in the
   `gen-<seed>-<port>` world dirs). Nothing records an entity in worldgen, no
   dump writes `entities/*.mca`, and the server does not spawn them; the
   allay count (1..3) is a level-random draw this port does not make.
3. **Pillager outpost chest**: its table is template-carried and its seed is a
   `placeInWorld` draw on the jigsaw piece stream, which waits on the
   `RUSTMC_JIGSAW_STREAM` decision.
4. The mansion's remaining in-box residual is dark forest vegetation and
   fence/wall shapes under those leaves (wm1 77/26, wm2 54/19).

### structures-2, second round, 2026-10-02

**The deep dark.** `feature::sculk_worldgen` is `SculkPatchFeature` and the
worldgen `SculkSpreader` ported again from the bytecode with every write
immediate; the old deferred-write port (`sculk_patch::place`,
`ChargeCursor::update_worldgen`) is deleted. Eleven departures are listed in
the module header. Unit: block positions; floor = vanilla-a vs vanilla-b.

| window | before | after | floor |
|---|---|---|---|
| seed 12345 city, 81 chunks, all | 41,220 | 33,669 | 18,726 |
| ... sculk-touching | 34,266 | 26,635 | 18,308 |
| ... inner 7x7, sculk-touching | 21,557 | 17,098 | 11,823 |
| ... in the city's boxes | 13,755 | 9,161 | 5,329 |
| window A deep_dark, twin-masked | 44,245 | 5,117 | 17,695 (unmasked) |
| window A, all | 75,773 | 30,522 | 58,769 |

Sensors in the city window 140 -> 256 (vanilla 246 / 271); city block
entities exact 30 -> 129 of 273 (floor 177). Seven chunks where both
vanilla runs agree read 0 sculk-family mismatches and are pinned by
`deep_dark_sculk` (a new T1 binary; red on the old code).

**Structure entities.** Spawned by the placers into a canvas entity channel
(on with the block-entity channel), written by `worldgen-dump` to
`entities/`, added to the live level by `World::generate_in`
(`crate::structure_entities`), and written beside saved chunks by
`World::save_to`. Typed against vanilla's own `entities/` files (scratch
`entcmp2.py`): both mansions 32/32 and 52/52 exact but UUID, four
monuments 3/3 each, ship cities 14/14 and 7/7 present (shulker yaw is
unseeded in vanilla too). Before: none existed.

Then, the same way: a mineshaft corridor's chest minecarts (4/4 and 3/3 with
their `LootTableSeed`), a swamp hut's witch and black cat (2/2 and 2/2,
`sound_variant` included) and an ocean ruin's drowned (5/5; their
`spawn_reinforcements` and knockback bonus come from the entity's own
unseeded random and are not claimed). Each has a test in
`structure_block_entities` that went red under a named sabotage.

**Still open.** The level entities have no AI (this server ticks only
`crate::mob::KINDS`); template entities (igloo villagers, outpost cage allays
and iron golem, village villagers and cats) are not placed, and the
harvests cannot adjudicate them — vanilla had ticked them before the save; a
drowned or zombie that rolls a jockey refuses it; the deep dark's remaining
sculk residual sits mostly at the harvest's edge ring and in the city; the
city window's surface vegetation residual is not a structure.
