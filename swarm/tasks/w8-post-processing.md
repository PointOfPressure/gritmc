# w8 — the post-processing shape pass

**144 blocks at seed 31415926**, and every one of them is the same block on
both sides with different properties:

```text
  52  oak_fence[all false]  ->  oak_fence[south=true]
  50  oak_fence[all false]  ->  oak_fence[west=true]
  42  oak_fence[all false]  ->  oak_fence[east=true]
```

Vanilla has the fence with **no** connections; we have one. That is the
opposite of the usual shape of a bug — we are not failing to set something,
we are failing to *unset* it.

## Why

`MineShaftCorridor.placeSupport` sets `WEST` and `EAST` explicitly, in 26.2
exactly as we do — verified from the bytecode, so the placement is right.
What we do not do is what happens next.

`StructurePiece#placeBlock` ends with:

```text
level.setBlock(pos, state, 2)
if (!level.getFluidState(pos).isEmpty()) level.scheduleTick(pos, fluid, 0)
if (SHAPE_CHECK_BLOCKS.contains(state.getBlock()))
    level.getChunk(pos).markPosForPostprocessing(pos)
```

Those marks become the chunk's `PostProcessing` list, and the pass that
consumes them calls `Block.updateFromNeighbourShapes`, which re-derives each
marked block's state from its six neighbours. A mineshaft fence set to
`west=true` against a bare cave wall loses that connection there.

We write `PostProcessing: []` and never run the pass, so the explicitly-set
connection survives into the file.

`SHAPE_CHECK_BLOCKS`, read from the jar — it is a short, closed list:

```text
  cave_air, torch, wall_torch, ladder, iron_bars,
  oak_fence, spruce_fence, birch_fence, jungle_fence,
  acacia_fence, dark_oak_fence, pale_oak_fence, nether_brick_fence
```

## What to build

1. A marked-position set on the canvas, written by
   `structure::writer::place_block` when the block is in that list — this is
   the same set vanilla saves as `PostProcessing`, so writing it into the
   region file is a free bonus and makes it diffable.
2. A pass after decoration that, for each marked position, recomputes the
   state from its neighbours.
3. `FenceBlock` is the only shape rule the current diff needs:

   ```text
   connectsTo(neighbour, sideSolid, direction) =
       (!isExceptionForConnection(neighbour) && sideSolid) || isSameFence(neighbour)

   sideSolid  = neighbour.isFaceSturdy(level, pos, direction.getOpposite())   // FULL
   isSameFence(s) = s.is(#fences) && s.is(#wooden_fences) == self.is(#wooden_fences)
   isExceptionForConnection(s) = s.is(#leaves) || barrier || carved_pumpkin
                               || jack_o_lantern || melon || pumpkin || shulker boxes
   ```

   `sturdy_faces` (SupportType.FULL) is already in `block_props.json`, and
   `#fences` / `#wooden_fences` / `#leaves` are already loaded.

Do the fences first and measure before adding torch, ladder or iron_bars —
none of those appear in either seed's diff today, and a rule written without
a number attached to it is a rule nobody can check.

## Note for whoever does it

This is the first thing in the project that needs a *post*-decoration pass
over marked positions, so it is worth getting the plumbing right rather than
special-casing fences inside the mineshaft writer. The same list is what a
running server replays when a chunk is loaded, so the set has a second use
beyond parity.
