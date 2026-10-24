# Cold start — branch `worldgen-deliver-once`

Every `### COLD START` entry ever written for this branch, newest last.

---

### COLD START — branch `worldgen-deliver-once`, stacked on `worldgen-generate-once` (`dd6f029d`), 2026-08-12

**THE ONE LINE: §32.6's property holds, and it is measured against today's
behaviour on one instrument — growing one canvas moves 0 blocks in every
already-delivered chunk over eight rungs, where regenerating the bigger square
moves 4,059 over the same eight, and the same growth moves 50,378 in the ring a
one-ring-early delivery would have handed out.**

Worktree `~/grit/work-once`, remote dir `~/rustmc-once`, logs in
`~/grit/private/once-*.log`. Write-up: `docs/DETERMINISM.md` §35.

**What was built.**

1. **`rustmc_worldgen::pipeline::Resumed`** — a canvas plus the status map on
   it (`carved`, `decorated`, as sets rather than squares, because a server's
   growth is a player walking east and not a concentric ladder).
   `Pipeline::resuming` builds onto it; `needs_terrain` / `needs_decoration`
   are the two predicates.
2. **`worldgen_assembly::generate::generate_resuming`** — the same 2,200-line
   overworld pass, with the noise fill, the biome containers, the surface
   rule's interior and perimeter halves, the carvers and the decoration
   targets each filtered by the status map. `generate_decorating` delegates to
   it with `None`, and every filter is then a test against an empty set, so
   the from-scratch path is byte-neutral by construction.
3. **`rustmc_server::growing::GrowingWorld`** — one canvas, a `ChunkStatus`
   per chunk raised at most once, `grow_to(radius)` returning how many chunks
   it had to carve, and `is_delivered` derived from the status map and
   `ChunkStep.getAccumulatedRadiusOf(FEATURES)` rather than from the radius
   that was asked for.
4. **`crates/rustmc-server/tests/growing_world.rs`** — the gate, two arms in
   one binary on one instrument.

**What was measured.**

```text
  seed       centre     r0 -> r1   r1 -> r2   arm
  20260722   (0,0)             0          0   regenerate  (today)
  12345      (0,0)            66        608   regenerate
  12345      (48,54)          23        986   regenerate
  0          (0,0)            12      2,364   regenerate     total 4,059
  every case, both rungs                  0   grow           total     0
  every case, both rungs        5,032/6,151, 6,243/7,509,
                                3,236/7,767, 6,118/8,322
                                             one ring early  total 50,378
```

The regenerate arm reproduces `tiled_world.rs`'s eight numbers to the block,
from a different file and a different loop, which is what makes 4,059 -> 0 one
instrument's before and after rather than two instruments agreeing.

Carve cost: **81 chunks to deliver radius 2, against 155** for the regenerate
arm. Asserted against `extents_for(...).1.chunk_count()`, so a chunk carved
twice reddens the gate.

**The failure was witnessed, twice, not argued.**

* **Deliver one ring early** — score the ring outside the delivered square as
  delivered. `50378 moved in chunks that had already been delivered`, red.
  That is §32.6's own named failure and it is exactly the number the passing
  run reports beside its zero.
* **Drop the generate-once filter** on the decoration targets, so a decorated
  chunk decorates again on the next growth. Red as well — 62 / 361 / 304 / 879
  / 90 / 369 / 111 / 795, **2,971** blocks — and this is the sharper of the
  two. It is a defect in the *mechanism* rather than in the delivery rule, and
  it reddens at **seed 20260722**, where the regenerate arm reads 0 on both
  rungs. So this gate is not a re-spelling of `tiled_world.rs`'s: at the seed
  every parity figure in this project is taken at, there is a class of defect
  that file cannot see and this one convicts.

**Killed as a null / do not re-run:**

* **Do not re-open extent-independence.** §32.4 retired it and §35.5 restates
  the price. A grown world is *not* the world one pass at the final radius
  produces and cannot be; the 4,059 is the size of that difference at these
  four ladders and no margin or read model takes it to zero without moving
  8,712 -> 42,873 away from vanilla.
* **Do not expect a bare zero to be evidence.** The delivered-square zero is
  only worth anything beside the 50,378. A run that reported the first without
  the second would be `docs/AGENT-BRIEF.md` rule 7's shape.
* **Do not simulate growth by regenerating and comparing.** They are not the
  same thing and the whole branch is the measurement of how different: the
  `grow` arm holds one canvas across the growth and the `regenerate` arm does
  not, and they read 0 and 4,059.

**Three honest gaps, all inside the 4,059 and none of them a violation.**

* The shared `ParameterList`'s `find_value` breaks ties off the previous
  query, so a growing pass can hand a *fresh* chunk a different biome on an
  exact tie than a from-scratch pass would.
* Structure starts are computed over the requested square, so a start
  discovered only once the square is bigger never reaches a chunk that has
  already decorated.
* The decoration order is **growth-anchored, not world-anchored**. §32.6 asked
  for world-anchored; what is built is a function of the growth history rather
  than of the window. Enough for this property, not enough to make two
  differently-grown worlds agree.

**Gate state at hand-off.** `ci/t0-remote.sh` **T0 GREEN** on the committed
tree. `cargo test --release -p rustmc-server --test growing_world` **2 passed,
0 failed**, 48.1 s. `--test tiled_world` **3 passed, 0 failed** with its four
cases unchanged (0 / 0 / 66 / 608 / 23 / 986 / 12 / 2,364, and 122,560
decoration blocks). `ci/end-city-window.sh` **100.000000%** against both twins,
`chunks=225 blocks=14745600 mismatches=0`, VANILLA FIRST — re-run because this
branch touches `pipeline.rs`, which the End path builds its `Pipeline` through.
`ci/ref-chunk-extent.py --window 76 55 90 69` exits **0** on both twins and on
our dump, so those 225 chunks are 225 that are `minecraft:full` on every side
rather than 225 `region-differ` kept after silently dropping the rest. No
corpus run: this branch touches neither `ci/checks/` nor the server's gameplay
paths.

**The single next item, and the result that closes it.** Wire `tiles.rs` onto
`GrowingWorld`. That is the memory and retirement half rather than the
generator half: one canvas per world instead of one per tile changes what
`retain_around` can retire (a chunk, not a tile), what the per-tile
`BlockEdits` overlay is keyed on, and the `MAX_CONCURRENT_GENERATIONS` bound
that `docs/BENCH.md`'s radius sweep set. **It closes when the RSS figures in
`docs/BENCH.md` are re-taken on the growing world and a player walking a
straight line grows the world without the server holding more than one
canvas** — and the §35 gate must still read 0 with `Tiles` driving the growth
rather than the test.

**One collision to know about before merging.** Two unmerged branches both
claim `docs/claims/DETERMINISM-31-*`: `worldgen-decoration-margin`
(`-decoration-margin-does-not-converge`, which is the §31 this branch's
ancestry cites) and `worldgen-sulfur-r18` (`-the-35-survive-at-r18`).
`ci/check-doc-claims.py` refuses a duplicate `(doc, number)`, so whichever
merges second has to renumber. Not this branch's to fix — this branch claims
33 and 33 is free on every ref.
