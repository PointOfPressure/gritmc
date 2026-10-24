# w6 — the amethyst geode

**Now the largest single item in the diff**: ~700 blocks across six rows, all
in one band at `y -54..-42` (docs/BENCH.md). One configured feature,
`minecraft:amethyst_geode`, `minecraft:geode`.

`GeodeFeature.place` is fully read; what follows is that reading. The only
thing still to extract is a handful of codec defaults — see the last
section, and do that *before* writing, because a wrong default changes every
geode in the world identically and invisibly.

## The shape

```text
points        = distribution_points.sample(random)
noise         = NormalNoise(WorldgenRandom(LegacyRandomSource(LEVEL SEED)), -4, [1.0])
invDist       = points / (double) outer_wall_distance.getMaxValue()
filling       = 1 / sqrt(layers.filling)
inner         = 1 / sqrt(layers.inner_layer  + invDist)
middle        = 1 / sqrt(layers.middle_layer + invDist)
outer         = 1 / sqrt(layers.outer_layer  + invDist)
crackSize     = 1 / sqrt(crack.base_crack_size + random.nextDouble()/2
                         + (points > 3 ? invDist : 0.0))
generateCrack = random.nextFloat() < crack.generate_crack_chance
```

Then the distribution points, each **three** draws for the offset and one
more for its own point offset:

```text
invalid = 0
for i in 0..points:
    p = origin.offset(wall.sample(r), wall.sample(r), wall.sample(r))
    s = level.getBlockState(p)
    if s.isAir() || s.is(blocks.invalid_blocks):
        invalid += 1
        if invalid > invalid_blocks_threshold: return false     # BAILS OUT
    pointOffsets.add((p, point_offset.sample(r)))
```

Note the bail-out is **after** the draws for that point and before the rest —
a geode that refuses has still moved the stream.

If cracking, one `nextInt(4)` picks a side and three crack points are added
at fixed offsets `(span, 7, 0) (span, 5, 0) (span, 1, 0)` rotated per side,
where `span = points * 2 + 1`.

## The field

```text
for pos in betweenClosed(origin + minGenOffset, origin + maxGenOffset):
    n = noise.getValue(x, y, z) * noise_multiplier
    distance = sum over points     of invSqrt(pos.distSqr(p) + offset) + n
    crackDist = sum over crackPts  of invSqrt(pos.distSqr(c) + crack_point_offset) + n
    if distance < outer: continue                      # nothing out here
    if generateCrack && crackDist >= crackSize && distance < filling:
        set AIR; schedule a fluid tick on each of the six neighbours
    elif distance >= filling:  set filling_provider
    elif distance >= inner:
        alternate = random.nextFloat() < use_alternate_layer0_chance
        set alternate ? alternate_inner_layer_provider : inner_layer_provider
        if (!placements_require_layer0_alternate || alternate)
           && random.nextFloat() < use_potential_placements_chance:
               placements.add(pos)
    elif distance >= middle:  set middle_layer_provider
    elif distance >= outer:   set outer_layer_provider
```

**The distance field is larger nearer the centre** — it is a sum of inverse
square roots — so `filling > inner > middle > outer` reads inside-out, and
the early `continue` is the *outside* of the geode. Getting that backwards
builds a geode with its layers inverted, which still looks like a geode.

Every write is `safeSetBlock(..., cannotReplace)` — the `#features_cannot_
replace` predicate, not a bare set.

`random.nextFloat()` for the alternate layer is drawn **for every block in
the inner layer**, and the potential-placement draw only follows it when the
first condition holds — so the two are not a fixed pair.

## The buds

```text
for pos in placements:
    bud = innerPlacements[random.nextInt(innerPlacements.size())]     # ONE draw
    for d in Direction.values():
        if bud.hasProperty(FACING): bud = bud.setValue(FACING, d)
        n = pos.relative(d)
        at = level.getBlockState(n)
        if bud.hasProperty(WATERLOGGED):
            bud = bud.setValue(WATERLOGGED, at.getFluidState().isSource())
        if AmethystClusterBlock.canClusterGrowAtState(at):
            safeSetBlock(n, bud); break
```

One draw per placement, then a walk that stops at the first face that will
take a cluster. The `bud` variable is **mutated as the loop goes**, so a
later direction inherits the FACING and WATERLOGGED of an earlier one — that
is the reference's own behaviour and not a bug to tidy.

## The noise is seeded from the WORLD seed

`NormalNoise.create(new WorldgenRandom(new LegacyRandomSource(level.getSeed())),
-4, [1.0])` — not from the feature stream, and not positionally forked. Every
geode in a world shares one noise field. This port already has NormalNoise
and LegacyRandomSource, so it is wiring rather than new maths.

## What is still to read

The codec defaults for the fields `amethyst_geode` does **not** set:

- `distribution_points`, `point_offset` (both IntProviders, validated to
  1..20 and 0..10 respectively — the *range* is visible in the bytecode, the
  *default* is not)
- `min_gen_offset` (-16 seen), `max_gen_offset`
- `placements_require_layer0_alternate`
- `crack.base_crack_size` (2.0 seen), `crack.crack_point_offset` (5 seen)

Confirmed already: `layers.filling` 1.7, `inner_layer` 2.2, `middle_layer`
3.2, `outer_layer` 4.2, `use_potential_placements_chance` 0.35,
`noise_multiplier` 0.05.

Read them from `GeodeConfiguration`'s static initialiser before writing a
line — the datapack leaves most of them unset, so they are load-bearing and
a wrong one is invisible.

## How it gets gated

The whole-world diff, directly: the geode's rows are unmistakable
(`smooth_basalt`, `calcite`, `amethyst_block`, `budding_amethyst`) and sit in
one band. It is currently 4 refusals and ~700 blocks; both should go to
approximately zero.
