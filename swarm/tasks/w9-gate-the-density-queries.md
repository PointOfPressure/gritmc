# w9 — gate the density queries that place no blocks

Both open faults in `docs/BENCH.md` end at the same kind of thing, and it is
the kind nothing in this project measures.

| fault | blocks | last suspect |
|---|---:|---|
| carve jitter | ~1,458 at seed 31415926 | `Aquifer#computeSubstance(pos, 0.0)` at carve positions |
| `packed_ice` | 4,110 in nine chunks | `minSurfaceLevel` behind `above_preliminary_surface` |

Both are **density-function queries that write no block of their own**. A
block-parity gate cannot see either: it sees what a later stage did with the
answer, and a stage that agrees for the wrong reason is indistinguishable
from one that agrees for the right one. That is why "the noise stage is
bit-exact over 1.77M blocks" covers neither.

Two independent investigations have now walked eight or nine links down a
chain and arrived here. Gating the functions is worth more than either fix.

## Status

**Done, and both were innocent.** `preliminarySurfaceLevel` and
`getSurfaceDepth` are gated in `golden_surface_chunk` and report 0 of 12,288
each. Keep that result in mind before spending on the rest: this note was
written on the theory that these values were where the bugs were, and the two
that have been checked were exact. The gates are still worth having — they
close blind spots permanently — but they have not yet found a fault.

Remaining, in increasing order of difficulty:

| value | where | difficulty |
|---|---|---|
| `stoneDepthAbove` | `stone_depth`, `water` with `add_stone_depth` | hard — per-block running state |
| `waterHeight` | `water` | hard — same |
| `computeSubstance` | the aquifer, at carve positions | hard — scattered positions |

### Why the remaining three are harder

The two that are done are **per column**: 256 values a chunk, and both had a
public or package-visible accessor that could be called from outside the
walk. `stoneDepthAbove` and `waterHeight` are neither. They are fields on
`SurfaceRules.Context` updated as the column walk descends, so their value
depends on *y*, and `SurfaceSystem#buildSurface` constructs the `Context`
internally and never hands it out.

Three options, none free:

1. Dump them for one chunk over a narrow y band by reaching the `Context` —
   its fields are package-private and the dumper is in
   `net.minecraft.world.level.levelgen`, so access is legal *if* a reference
   can be obtained. Look for a seam in `buildSurface` rather than
   reconstructing one.
2. Recompute them in the dumper from vanilla's own post-surface blocks. This
   is the tempting one and it is an **oracle fault waiting to happen**: it
   compares this port against a second implementation written by the same
   person on the same day, not against vanilla. ORACLE.md 5b.
3. Leave them, and attack `packed_ice` from the block side instead — the two
   deep chains are now identified, so the next question is which of their
   five conditions is false at a *specific* failing block, which a targeted
   trace through our own evaluator would answer without any dumper at all.

Option 3 is probably the cheapest next move and does not need this note.

## 1. `preliminarySurfaceLevel` — DONE, 0/12,288

`NoiseChunk#preliminarySurfaceLevel(int, int)` is **public**, and
`SurfaceChunkGolden.generateChunk` already builds the `NoiseChunk` it needs
(around line 395). It currently returns only the `ProtoChunk`, so the emit
site cannot reach it — return a small holder, or have `generateChunk` fill a
`int[256]` the caller passes in.

Emit it beside `oceanFloorWg`, in the surface-chunk case rather than
`encodeWorld`:

```java
List<String> prelim = new ArrayList<>();
for (int z = 0; z < 16; z++)
    for (int x = 0; x < 16; x++)
        prelim.add(Integer.toString(
            noiseChunk.preliminarySurfaceLevel(chunkMinX + x, chunkMinZ + z)));
```

Note the arguments are **world** coordinates, not local — check that against
the bytecode before trusting this snippet.

Then a gate comparing `SurfaceLevels::at` against it, the same shape as
`golden_ocean_floor_wg.rs`. Expect it to fail at chunk (0, −135) seed
31415926, which is the `packed_ice` chunk: if it does, that closes a
4,110-block fault and the surface gate goes green with it.

## 2. `computeSubstance` at carve positions

Harder, because the carver asks it at scattered positions rather than per
column, and the answer depends on the aquifer's state after the fill.

The cheap version: have the harness log every `(x, y, z) -> substance` the
carvers ask for in one chunk, and compare against ours replayed over the same
positions. `golden_carver_chunk.rs` already builds our aquifer from the same
`PostFill`, so the machinery to replay is in place — what is missing is
vanilla's side of the log.

Do this one second, and only after the preliminary-surface gate has either
found its fault or cleared it. If `minSurfaceLevel` turns out to be wrong,
check whether the same root cause reaches the aquifer before building a
second dumper: both read the density router, and one wrong shared input would
explain both faults at once.

## Why this ordering

The `packed_ice` fault has a red gate attached to it and a nine-chunk
reproduction. The carve jitter has neither — it is only visible in a
whole-world diff. Fix the one that can prove itself first.

---

# w10 — `CacheAllInCell` per-cell semantics (the packed_ice cause)

Diagnosis is complete; see the `CacheAllInCell` section of `docs/BENCH.md`.
Vanilla fills every `cache_all_in_cell` node once per cell over 128 positions
inside a `fillingCell` window, during which an interpolated descendant
returns its **raw** value. The cache then serves those raw values for the
whole cell. We pass the marker through and interpolate per block, which
agrees only at cell corners — a third of a column wrong, by up to 0.32.

## The fix is smaller than it sounds

`MarkerKind::Interpolated` already degrades gracefully:

```rust
MarkerKind::Interpolated => match slot {
    Some(index) if *index < ctx.interpolated.len() => ctx.interpolated[*index],
    _ => wrapped.compute(ctx),
},
```

So evaluating a `CacheAllInCell` child with an **empty** `ctx.interpolated`
reproduces vanilla's `fillingCell` behaviour exactly, with no new flag on the
context and no change to any density function.

Steps:

1. Give `CacheAllInCell` markers a slot, as `Interpolated` and `FlatCache`
   have. Check whether the graph builder already assigns one.
2. Add a per-cell store to the evaluator: one `[f64; cell_width * cell_height
   * cell_width]` per marker slot — 128 for the overworld, but derive it from
   the geometry rather than hard-coding.
3. In `evaluate`, where the cell corners are reloaded on `loaded_cell`
   change, also fill those arrays: iterate the cell's positions in vanilla's
   order and compute the child with `interpolated: &[]`.
4. Make the marker read `store[slot][cell_local_index]`.

## Gate it with the fill dump, not a sampled value

Every existing gate is blind to this: sampling one point never enters a
`fillingCell` window, so both sides take the pass-through path and agree.
Use the pair built for the diagnosis —
`-Ddump.fill.density=x,0,z` on the Java side and
`RUSTMC_DUMP_FILL_DENSITY=x,z` on ours, each emitting a seed-tagged column —
and compare those. Column (4, −2160) at seed 31415926 has 137 of 384
positions wrong today, which makes it a good first case.

Expect the `packed_ice` failure and a share of the r7 carve jitter to move
together, since both were downstream of this.
