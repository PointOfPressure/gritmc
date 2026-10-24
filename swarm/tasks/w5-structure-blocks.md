# w5 — structure blocks (mineshaft postProcess and the pipeline stage)

Status: **layout DONE and proved; blocks NOT started.** Everything below was
read from the 26.2 jar during the layout port and is recorded so the next
stretch does not re-derive it.

## What is already proved

- `structure::placement` — 20 sets, 715,084 chunk decisions, 0 wrong
  (`golden_structure_placement`), plus 11 real starts accepted and
  placement/starts coinciding both ways in the diff world
  (`golden_structure_starts`).
- `structure::mineshaft` — **100.000000% over 659 real pieces**, 5
  mineshafts: id, bounding box, saved `O`, recursion depth.
- `structure::pieces` — boxes, the builder, `moveBelowSeaLevel`, and the
  local→world rotation.
- `structure::writer` — placeBlock/getBlock/isInterior/generateBox/
  generateMaybeBox/maybeGenerateBlock, `canBeReplaced`,
  `isInInvalidLocation`.

## The block stream is the DECORATION stream

`ChunkGenerator.applyBiomeDecoration` makes one
`WorldgenRandom(XoroshiroRandomSource(generateUniqueSeed()))`, calls
`setDecorationSeed(levelSeed, originX, originZ)`, then per step:

```text
index = 0
for structure in structuresByStep.getOrDefault(step, []):     # registry order
    random.setFeatureSeed(decorationSeed, index, step)
    for start in startsForStructure(chunkPos, structure):
        start.placeInChunk(...)
    index++                    # increments per structure TYPE, not per start
# then the features of this step, seeded by their GLOBAL sorted index
```

Three consequences, each checked:

1. **Structures slot into the existing decorate stage.** Same seeding
   ladder, same eleven steps. `minecraft:mineshaft`'s step is
   `underground_structures`.
2. **The structure counter is per-step and per-structure-type**, and it
   increments whether or not a start exists in this chunk — so it does not
   depend on what is present, only on the registry.
3. **Structures and features share the index space** — both start at 0, so
   a structure at index 0 in a step collides with the feature at global
   index 0 in the same step. That is vanilla's own behaviour, not a bug to
   fix.

**Checked and cleared:** our feature indexing already uses the global sorted
index from `FeatureSorter`, which is what vanilla passes. The structure
counter does not shift it. No change needed in `feature::decorate`.

`StructureStart.placeInChunk` then walks the pieces in order and calls
`postProcess` on each whose box intersects the chunk — so a piece spanning
four chunks is written four times with a different clip each time.

## What is left, in order

1. `MineShaftCorridor.postProcess` — the big one. Reads so far:
   - `isInInvalidLocation` first; return if true.
   - `generateBox(0,0,0, 2,1,len, CAVE_AIR, CAVE_AIR, false)` — the tunnel.
   - `generateMaybeBox(0.8, 0,2,0, 2,2,len, CAVE_AIR, CAVE_AIR, false,false)`
     — the ceiling. **~2,150 of the ~2,370 mineshaft blocks in the diff are
     this hollowing**, not the decoration.
   - `if (spiderCorridor) generateMaybeBox(0.6, 0,0,0, 2,1,len, COBWEB, CAVE_AIR, false, true)`
   - per section `s`, `z = 2 + s*5`: `placeSupport`, then eight
     `maybePlaceCobWeb` (0.1 at z±1 on both walls, 0.05 at z±2), then two
     `nextInt(100)==0` chests, then the spider spawner.
   - still unread: the tail after the section loop (rails, floor).
2. `MineShaftCrossing/Stairs/Room.postProcess`.
3. The pipeline stage: place structures at their step inside
   `stage::decorate`, before that step's features.

## Traps already found and pinned in code

- `generateMaybeBox` draws per position **before** every other test;
  `maybePlaceCobWeb` tests `isInterior` **before** its draw. Opposite orders,
  same file.
- `generateMaybeBox` places on `nextFloat() <= chance`; `maybeGenerateBlock`
  on `nextFloat() < chance` (`fcmpl` vs `fcmpg`).
- Box iteration is Y, then X, then Z.
- Reads outside the clip return **air**, not the world's block.
- `isInterior` asks about `y + 1`.
- A mineshaft never overwrites its own planks/log/fence/chain
  (`canBeReplaced`), so a corridor crossing an older one keeps its supports.

## The thing to be careful about

Block **states**, not names — oracle fault 7. Fences carry `west=true`,
rails carry a shape, chains an axis. Thread `state_spec::DefaultStates`
through `postProcess` rather than writing bare names.
