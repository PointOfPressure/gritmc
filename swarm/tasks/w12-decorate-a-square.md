# w12 — make `DecorateGolden` decorate a square, not a 3x3 centre

This is the single change two open clusters are waiting on: the sulfur pool
(~860 blocks at seed 20260722) and the lush-caves `moss_block` (170 at
31415926). Both have been chased to the same place and stopped there.

## What is wrong with the harness

`DecorateGolden` generates a 3x3 world and decorates **only its centre
chunk**. A real server decorates every chunk, and a feature writes one chunk
out — so by the time a server reaches chunk C, its already-decorated
neighbours have written into C.

Any placement chain that *reads* the world therefore sees a different world
in the harness than on a server. That is exactly the chains these clusters
use: `environment_scan` and `block_predicate_filter`.

The proof, measured, is in the sulfur section of `docs/BENCH.md`: chunk
(58, 62) digs a lake at (943, −18, 1006) before (58, 63) runs. A server does
it, this port does it, the harness does not. At chunk (59, 62) the harness
places four lakes and **this port correctly places none**, because two of the
harness's four origins sit inside that lake's box, above its waterline, so
`cave_air` rather than the `sulfur` the filter demands. There the harness is
the oracle that is wrong.

## The shape of the fix

Decorate an inner square, not one chunk. Two constraints, the second of which
is not obvious and will silently produce a wrong oracle if missed:

1. **The generated grid has to be two chunks bigger than the decorated
   square.** Decorating chunk C faithfully needs C's 8 neighbours decorated,
   and decorating those needs *their* neighbours to exist. To decorate a 3x3
   and trust its centre, generate 5x5. Cost is 25 `generateChunk` calls
   instead of 9.

2. **`GenLevel`'s write clipping must move with the chunk being decorated,
   not stay on the grid's centre.** It is currently constructed once with a
   centre and a `writeRadius`, and `ensureCanWrite` is relative to that
   centre. Vanilla's `WorldGenRegion` is rebuilt per chunk, so its cutoff is
   always relative to the chunk being decorated. Leaving it pinned to the
   grid centre would let a corner chunk's features write four chunks across
   the grid, which vanilla drops — and that error looks like a *better*
   oracle, because it produces more blocks.

Also per decorated chunk: its own origin, its own `setDecorationSeed`
population seed, and its own `present` biome set collected from *its* 3x3.
The current code computes all three once. Extract the body of the step loop
into a method taking the chunk being decorated.

Decoration order should be the raster the rest of this project uses — x
outer, z inner, ascending — which was measured against four alternatives and
beat them by between 3,000 and 48,000 blocks (see `docs/BENCH.md`).

## How to know it worked

Not "the vectors regenerate". The three numbers that must move:

- At chunk (59, 62), seed 20260722, the harness should stop placing the four
  lakes it currently places, agreeing with this port and the r7 world.
- At chunk (58, 63), the 26 "only vanilla" writes should fall to about 1.
- `lush_caves_ceiling_vegetation` should start producing a position that
  reaches (1068, −15, 1007) at seed 31415926, which nothing currently does.

Regenerate the committed vectors afterwards and re-run `golden_decorate`;
expect it to move, and read every changed line rather than accepting it.

## Watch out

`golden_decorate`'s vectors are committed. Regenerating them changes a gate's
baseline, which is exactly the move this project forbids doing casually — so
the diff has to be explained line by line, not just re-blessed. If the new
vectors disagree with the port somewhere unrelated to this change, that is a
finding, not noise.
