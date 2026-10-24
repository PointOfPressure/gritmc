# COLD START — branch `structures-live`

Append-only. Newest entry last.

### COLD START — branch `structures-live`, off `main` (`bd5be4a36`), 2026-09-05

**The brief's premise was false and the first measurement killed it.** The area
was "wire the live server's chunk pipeline to run the same stages the dump
runs so the two become one configuration". They already were:
`World::generate_through` calls `worldgen_assembly::generate::generate`, which
is the same function `tools/worldgen-dump` drives. Nothing had ever measured
it, so four documents said otherwise and a published census line said the
running server places none of the twenty structure sets.

Worktree `~/grit/work-sl`, remote dir `~/rustmc-sl`.

## WHAT WAS RETIRED

`docs/FINDINGS.md`'s *"the worldgen dump places 8 of 20 structure sets and the
running server places none of them: the parity figures and the world a player
would join are different configurations"* — struck through with its reason
rather than deleted. Also corrected in the same commit: `docs/SERVER.md`'s
"No structures" bullet, `crates/rustmc-server/src/world.rs`'s module doc and
its `caveats()` string, `crates/rustmc-server/src/terrain.rs`'s beardifier
comment ("the live server generates no structures at all"),
`crates/rustmc-worldgen/src/pipeline.rs`'s "What is not here yet", and
`worldgen-assembly`'s `Generated::caveats`, which said five sets and had never
mentioned `ruined_portals`.

## WHAT SHIPPED, AND THE NUMBER EACH THING RESTS ON

**Seven of the twenty structure sets are served, all seven measured on the
wire.** Six were already there and `buried_treasures` was ported here. A
better denominator: in the seed-20260722 r7 harvest vanilla actually places
**seven** sets, and this server now places all seven. Twenty is the size of
the registry, not the size of a world.

* `crates/rustmc-server/tests/served_structures.rs` — six windows, each
  decoding the `LevelChunkWithLight` the packet writer is handed and finding
  that set's marker blocks in it, against a `Through::Carvers` control over
  the same window that must find none. Reds on every window when the structure
  stage is switched off. The default world a player joins — `WORLD_SEED` 0,
  `WORLD_RADIUS` 10 — carries 45 `rail` and 14 `cobweb`.
* `crates/rustmc-server/tests/served_vs_vanilla.rs` — the served chunks
  against a vanilla harvest, inside the boxes vanilla's own
  `structures.starts` NBT records. **5,556 of 3,461,044 positions differ over
  twelve boxes, floor 4,299**, seed 20260722 centre (64,64) r=16,
  `gen-wide-20260722-r16-ow-a` with `-b` as the twin. Six of the twelve rows
  sit at or under their own floor. Each row carries its three largest
  `vanilla -> ours` classes, asserted exactly, so a residual that changes its
  nature reads as red.
* `crates/rustmc-server/tests/structure_census.rs` — 20 sets naming exactly
  the 34 definitions, 188 pools, 1,212 templates, the extract complete against
  the manifest, every claimed set a real set name.
* `ci/dump-structure-tables.sh` — `structure_sets.json` had **no generator in
  this repo** and no provenance stamp. It does now, reproducing the inherited
  file byte for byte, plus `structure_defs.json`, `template_pools.json` and
  `structure_templates.json`, all four registered in
  `ci/gen-gameplay-tables.sh` and derived by its grep (21 tables -> 25).

## THE FOUR FINDINGS WORTH MORE THAN THE PORT

1. **A harvest's outer ring is proto-status and `block_at` answers air for
   every position in it.** Without a `status == "minecraft:full"` filter this
   comparison charged one mineshaft 560,883 of 566,720 — 99% — all of it the
   reference having no blocks rather than different ones. Any region
   comparison in this repo that does not filter on status is measuring its own
   coverage.
2. **Harvest EXTENT is a condition of a structure comparison the way chunk
   order is.** `vanilla-r7` and `gen-wide-20260722-r16-ow-a` are both seed
   20260722 and disagree by 293 of 648 positions — 45.2% — inside the
   `ruined_portal (48,64)` box, while r16-a against r16-b is 7 and r7 against
   r7-b is 0. Our served chunks read 293 wrong against one and 7 against the
   other.
3. **Most of the residual inside a structure box is not the structure.**
   `minecraft:sulfur` is 1,519 of the 5,556 — 27.3% — and is a 26.2 decoration
   feature this port does not place (`origin/worldgen-sulfur-r18`, on the
   landing stack). Stone-variant swaps are the next largest class and are
   floor noise. The parked village hay pile is exactly **30 blocks** at
   `village_savanna (81,77)`, the first measurement of what the
   `feature_pool_element` refusal costs.
4. **A gate scoped to the reference's own structures cannot see a structure
   the reference does not have.** The `buried_treasure` port shipped with
   `is_placement_chunk` — the spread grid alone, with the frequency coin
   missing — and put **68 chests in the r=16 window where vanilla has none**.
   All twelve box-scoped rows stayed green through it. The whole-window
   single-block sweep that caught it is in `served_vs_vanilla.rs` with its
   limits stated: it sweeps one block name, and the general form is a
   whole-window diff.

## NULLS — DO NOT RE-RUN THESE

* **Do not compare the server's canvas against the dump's canvas.** They are
  the same function call; the comparison is an instrument measuring its own
  input. What is worth measuring is the *serving path* — canvas to encoded
  chunk to global network state id — and that is what both server tests do.
* **Do not adjudicate a structure box against `oracle-work/vanilla-r7`.** See
  finding 2. Use the r16 pair, and read the coverage before the residual.
* **`ocean_ruin_cold (2,2)` and `shipwreck (0,11)` look like clean zeroes and
  are not.** Both score **zero positions** against the r16 references, which
  do not cover chunks near the origin. A zero-coverage box is why
  `served_vs_vanilla` asserts `positions` exactly rather than as a bound.
* **`buried_treasure (73,46)`'s box in `vanilla-r7` is at y 90 and the chest
  is at y 53.** `postProcess` reassigns the bounding box after it places, so
  the saved `Children[0].BB` is only where the search started. Read the box
  from the harvest you are scoring against.
* Neither `ocean_ruins` nor `shipwrecks` has a report function, so their
  windows were found by scanning two 49x49 censuses at seed 0, centres (30,20)
  and (-25,-25). The windows are in `served_structures.rs`; do not re-scan.

## WHAT IS STILL OPEN, IN THE ORDER I WOULD TAKE IT

1. **The two partial sets.** `ocean_ruins` reads `ocean_ruin_warm.json` and
   `#has_structure/ocean_ruin_warm` only, so the COLD variant is not placed;
   `shipwreck_starts` refuses the BEACHED half and returns a count of what it
   skipped. Both are named in `Generated::caveats` now. Neither has a measured
   cost yet, because no adjudicable window this project holds contains one —
   `vanilla-s2-r7` (seed 31415926) has a `shipwreck_beached` start at (84,57)
   and is the cheapest place to take that number.
2. **Thirteen sets absent.** The cheapest next ports, in the order their
   pieces are self-contained: `igloos` (three templates, machinery already
   here), `swamp_huts`, `desert_pyramids`, `jungle_temples` (all
   `ScatteredFeaturePiece`, procedural and local), then `pillager_outposts`
   and `trail_ruins` (jigsaw, which this tree has). `strongholds` needs a
   `concentric_rings` placement, which `StructurePlacement` refuses today;
   `ocean_monuments` and `woodland_mansions` are the two large procedural
   ones.
3. **The closing measurement for any of them is already built.** Add the set
   to `SERVED` in `structure_census.rs`, add a window to
   `served_structures.rs`, and let `served_vs_vanilla.rs` print the row —
   if vanilla places one in the r16 window it appears automatically, because
   the boxes are read from the reference at run time.

### COLD START — branch `structures-live`, rebased onto `main` (`d48904cb5`), 2026-10-01

Worktree `~/grit/work-sl2` (detached), T0 dir `~/rustmc-sl2`, dev builds
`~/rustmc-sl2w` / `~/rustmc-sl2x`, control build of `origin/main` d48904cb5 in
`~/rustmc-sl2base`. The nine 2026-09-05 commits above were rebased first.

**Fourteen of the twenty sets are generated as chunks generate in the live
server and served on the wire.** New here: `swamp_huts`, `jungle_temples`,
`desert_pyramids`, `igloos` (with the basement), `pillager_outposts`,
`trail_ruins`, `strongholds`, plus the halves of two half-placed sets
(`ocean_ruin_cold`, `shipwreck_beached`). Not generated: ocean monuments,
woodland mansions, ancient cities. `ci/structure-surface.py` reads 17 of 20
sets placed, 31 of 34 structures.

## THE NUMBERS AND HOW THEY WERE MADE

Every row is one structure start in a twin vanilla harvest made for it
(`VANILLA_JVM_OPTS=-Dmax.bg.threads=1`, `GEN_ORDER=rows`, dumped
`RUSTMC_DECORATE_ORDER=zx`; harvests `~/rustmc-sl2w/oracle-work/sl2-<name>-{a,b}`,
r=3, r=4 for outposts and the stronghold). Ours is
`worldgen-dump full <seed> 5 <out> <cx> <cz>` (r=7 for outposts and the
stronghold); the diff is `region-differ diff <vanilla-a> <ours> --bbox <harvest square> --positions`,
vanilla first. **In-box** counts only the positions inside the vanilla start's
own saved `Children` boxes; the floor is the same count for vanilla-a against
vanilla-b. Before = `origin/main` d48904cb5. Unit: block positions.

| structure | seed, start chunk | in-box before | after | floor |
|---|---|---|---|---|
| swamp hut | 31415926 (0,-21) | 134 | 1 | 0 |
| swamp hut | 42 (-17,22) | 250 | 0 | 0 |
| jungle temple | 31415926 (44,38) | 1,052 | 118 | 124 |
| jungle temple | 3 (-25,1) | 951 | 10 | 20 |
| desert pyramid | 2024 (4,-19) | 1,526 | 0 | 0 |
| igloo | 12345 (36,41) | 146 | 0 | 0 |
| igloo, basement | 12345 (20,72) | 456 | 0 | 0 |
| igloo, rotated basement | 12345 (52,116) | 475 | 0 | 0 |
| beached shipwreck | 31415926 (84,57) | 383 | 0 | 0 |
| cold ocean ruin | 12345 (65,67) | 97 | 0 | 0 |
| trail ruins | 20260722 (55,0) | 3,905 | 16 | 2 |
| trail ruins | 42 (2,-34) | 3,089 | 0 | 0 |
| pillager outpost | 12345 (17,-60) | 2,925 | 0 | 0 |
| pillager outpost | 9 (10,-19) | 3,327 | 13 | 0 |
| stronghold, 163 pieces, 40,860 positions | 12345 (110,23) | 39,640 | 0 | 0 |

Layout is checked separately from blocks: `worldgen-dump stronghold-starts`
matches vanilla's saved `Children` piece for piece at (110,23), (-35,-103) and
(-105,124) of seed 12345 (163/163, 225/225, 115/115), and
`worldgen-dump jigsaw-set-oracle outpost|trail` does the same for both
outposts and both trail ruins. The End window (seed 20260722, (83,62), r=7)
stays 0 of 14,745,600 against both references.

## BUGS IN CODE THIS BRANCH DID NOT WRITE

* `Writer::generate_air_box` placed `cave_air`; vanilla's `generateAirBox`
  places `AIR`. 323 and 319 positions in the two jungle temple boxes.
* `speleothem_cluster` wrote a bare `minecraft:water` state, which has no
  network id, so the chunk encoder refused any chunk holding one. Found by
  `served_structures` going red on `main`'s own feature.
* `list_pool_element` was refused by the jigsaw library, which cut every
  outpost's tower. Supported now; outposts read 11/11 and 17/17 pieces.
* `CappedProcessor` and `BlockRotProcessor` were unported, so trail ruins
  never buried and outposts never mossed. Trail ruins 3,905 -> 16.

## NULLS

* **Ancient city: not ported.** With the named start jigsaw in, the walk
  matched 33 of 89 pieces, and its sculk patches are a refused feature. The
  half-port is kept off the branch.
* **Desert pyramid `pyr1` (seed 11, (33,8)) cannot be measured**: `main`'s
  mesa-mineshaft refusal fails the dump at every radius tried. `pyr2` stands
  in for it.
* `t1-par-falsify` failed once on a timing case while the box was loaded; it
  is not this branch's code.

**STATE at HALT (2026-10-01):** `structures-live` tip is this commit's parent `cad4cc003` plus this note; T0 c and d were RED only on `t1-par-falsify` case 6 (timing under load, GREEN 2/2 standalone), T0e RED on the same single stage; structure T1 green on `7a4aef7e2`.
`structures-mansion` (woodland mansions, stacked on this) has no T0 yet; detached chain `sl2-mchain` (scratch `mansion-chain.sh`) measures the second mansion, re-runs every window, then T0+T1 on its tip; `sl2-retry` reruns T0 here if T0e is red.
Next: read `t0e.out`/`t0f.out`, `mansion-chain.out`; if only case 6 is red, rerun T0 on a quiet box. Ancient city half-port is scratch `ancient-city-wip.patch`, not committed.

### COLD START — branch `structures-mansion`, off `structures-live`, 2026-10-01

Woodland mansions, the fifteenth set: `crates/rustmc-worldgen/src/structure/mansion.rs`
(`MansionGrid`, `MansionPiecePlacer`), `mansion_starts` and
`place_mansions_in_chunk` in `tools/worldgen-assembly/src/generate.rs`
(templates with `BlockIgnoreProcessor.STRUCTURE_BLOCK`, data markers, the
`afterPlace` cobblestone foundation), `worldgen-dump mansion-starts`.

Layout: vanilla's saved `Children` match piece for piece, in order —
template, `Rot`, `Mi`, template position and box — 615/615 at seed 8 start
(32,16) (rotation `CLOCKWISE_180`) and 587/587 at seed 30 start (39,-47)
(rotation `NONE`), in both twin harvests each
(`~/rustmc-sl2w/oracle-work/sl2-wm{1,2}-{a,b}`, r=4, clamped, rows).

Blocks, seed 8, in-box over vanilla's 615 piece boxes (126,022 positions),
same method as the table above: 51,248 wrong before -> 77 after, floor 26.
Four shape rules the mansion was the first to need came with it: walls
(394 -> 4), ladders that lose their wall to a later chunk (5 -> 0), rail
shapes under rotation and mirror (18 -> 0), vine faces under rotation and
mirror (12 -> 0).

Not here: the evokers, vindicators and allays the `Mage`, `Warrior` and
`Group of Allays` markers spawn (their marker block goes to air, as
vanilla's does), and the level-random draws those spawns make.
