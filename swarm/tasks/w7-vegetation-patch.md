# w7 — `vegetation_patch` and `waterlogged_vegetation_patch`

**Worth 850+ blocks**, the top row at seed 31415926 (`moss_block` against
`deepslate`). Invisible at seed 20260722, which has no lush caves in range —
this is the feature that justified running a second seed.

Refusals it accounts for: `30x minecraft:vegetation_patch`,
`3x minecraft:waterlogged_vegetation_patch`, and probably
`3x canSurvive(minecraft:spore_blossom)` downstream of it.

Reference: `VegetationPatchFeature`, read from the mapped jar — the class is
absent from `vanilla-src/`. Configs: `configured_feature/moss_patch.json` and
friends (`moss_patch_ceiling`, `pale_moss_patch`, `clay_with_dripleaves`,
`clay_pool_with_dripleaves`, and the `_bonemeal` variants).

## Config

```
depth                       IntProvider
extra_bottom_block_chance   float
extra_edge_column_chance    float
ground_state                BlockStateProvider
replaceable                 block tag
surface                     "floor" | "ceiling"   (CaveSurface)
vegetation_chance           float
vegetation_feature          a PlacedFeature, inline
vertical_range              int
xz_radius                   IntProvider
```

`surface` gives a direction: `floor` → DOWN, `ceiling` → UP.

## `place`

```text
predicate = state -> state.is(config.replaceable)
xz1 = config.xzRadius.sample(random) + 1        // two separate draws
xz2 = config.xzRadius.sample(random) + 1
set = placeGroundPatch(level, config, random, origin, predicate, xz1, xz2)
distributeVegetation(ctx, level, config, random, set, xz1, xz2)
return !set.isEmpty()
```

## `placeGroundPatch`

```text
dir      = config.surface.getDirection()
opposite = dir.getOpposite()
set      = new HashSet()

for x in -xz1 ..= xz1:
  edgeX = (x == -xz1 || x == xz1)
  for z in -xz2 ..= xz2:
    edgeZ = (z == -xz2 || z == xz2)
    edge   = edgeX || edgeZ
    corner = edgeX && edgeZ
    if corner: continue                      // no draw
    if (edge && !corner) and
       (config.extraEdgeColumnChance == 0.0 || random.nextFloat() > chance):
        continue

    mutable.setWithOffset(origin, x, 0, z)
    i = 0; while level.isStateAtPosition(mutable, isAir) && i < verticalRange:
              mutable.move(dir); i++
    j = 0; while level.isStateAtPosition(mutable, !isAir) && j < verticalRange:
              mutable.move(opposite); j++

    mutable2.setWithOffset(mutable, dir)
    state = level.getBlockState(mutable2)
    if level.isEmptyBlock(mutable)
       && state.isFaceSturdy(level, mutable2, opposite):
        depth = config.depth.sample(random)
              + (config.extraBottomBlockChance > 0
                 && random.nextFloat() < config.extraBottomBlockChance ? 1 : 0)
        saved = mutable2.immutable()
        if placeGround(level, config, predicate, random, mutable2, depth):
            set.add(saved)
```

**Draw traps, in order:**

1. A **corner** column is skipped with no draw at all. An edge column that is
   not a corner draws `nextFloat()` — but *only* when
   `extraEdgeColumnChance != 0.0`, because the `== 0.0` test short-circuits
   ahead of it. A port that always draws will be wrong on every config whose
   chance is zero, and right on `moss_patch` (0.3), which is exactly the way
   round that hides the bug.
2. `extraBottomBlockChance` draws under the same short-circuit: the `> 0`
   test comes first.
3. Both `xzRadius` samples happen before anything touches the world.
4. `set.add` uses the position **saved before** `placeGround` runs, because
   `placeGround` mutates the cursor.

## `placeGround` — the one that will catch people

```text
for i in 0 .. depth:
    ground = config.groundState.getState(level, random, pos)
    here   = level.getBlockState(pos)
    if ground.is(here.getBlock()):
        continue                 // <-- does NOT move the cursor
    if !replaceable.test(here):
        return i != 0
    level.setBlock(pos, ground, 2)
    pos.move(dir)
return true
```

When the block is already the ground block the loop **spins on the same
position** for its remaining iterations. It is not a no-op: `getState` is
called every time, so a weighted or noise state provider draws once per
spin. `moss_patch` uses a simple provider and does not, which means this
detail is free to get wrong at seed 31415926 and costs blocks somewhere else.

Also note the early return is `i != 0`, not `false` — a patch that laid at
least one block counts as placed.

## What is still to read

`distributeVegetation` (bytecode lines 272–307 of the disassembly) and
`placeVegetation` (307–325). They walk the returned set, draw
`vegetation_chance` per position, and run the inline `vegetation_feature` —
which for `moss_patch` is `moss_vegetation`, a `random_patch`. Check whether
the set is iterated in insertion order or `HashSet` order: it is a
`HashSet<BlockPos>`, and if the iteration order reaches the stream then this
is an ORACLE.md-rule-12 situation and needs its own measurement rather than
an assumption.

`waterlogged_vegetation_patch` is a subclass overriding only the ground
placement to waterlog it; do the base first and measure before porting it.
