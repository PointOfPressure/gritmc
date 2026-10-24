# Cold start — branch `worldgen-generate-once`

Every `### COLD START` entry ever written for this branch, newest last.

---

### COLD START — branch `worldgen-generate-once` off `main` (`d049ef5a`), on top of `worldgen-decoration-margin` (`3eeed9b5`), 2026-08-12

**THE ONE LINE: generate-once over one canvas *does* take the strengthened
gate to zero on all four cases — and it is measurably further from vanilla,
so it ships switched off. The item does not close, and the reason it does not
close is a stronger result than closing it would have been.**

Worktree `~/grit/work-canvas`, remote dir `~/rustmc-canvas`, logs in
`~/grit/private/canvas-logs/` and `~/rp-logs/`. Write-up:
`docs/DETERMINISM.md` §32.

**What was measured, in order.**

1. **The baseline reproduces.** The strengthened gate at `3eeed9b5` reads
   0 / 0 / 40 / 1,397 / 23 / 658 / 2 / 81 — 2,201 blocks — on this tree.
2. **The pyramid, from the jar.** `ChunkPyramid.GENERATION_PYRAMID`'s
   `FEATURES` step declares `addRequirement(CARVERS, 1)` and
   `blockStateWriteRadius(1)`: **a chunk decorates when its neighbours are
   CARVED, not decorated, and vanilla never requires a decorated neighbour of
   a decorating chunk at any distance.** `ChunkGenerationTask` walks one
   status at a time over a square whose radius it reads off
   `ChunkStep.getAccumulatedRadiusOf`, x-then-z, skipping chunks already at
   that status. For a chunk reaching `FULL`: `FEATURES` at radius 1,
   `CARVERS` at 2, `BIOMES` at 3, `STRUCTURE_STARTS` at 11. Ported with the
   radii pinned as read, in `crates/rustmc-worldgen/src/status.rs`.
3. **Isolated decoration takes the gate to zero — all eight rungs.** Each
   chunk decorates against a copy of the carved canvas that no other chunk
   has written into; the write lists are replayed onto the one canvas in the
   canonical order. Every one of the gate's eight rungs reads 0.
4. **And the control is in the same binary.**
   `RUSTMC_DECORATE_ISOLATED=0` reproduces 2,201 to the block — the same
   test, the same build, the same seeds. The zeroes are not the instrument
   going quiet.
5. **The price, and it is why this does not ship on.** Seed 20260722, r=7 at
   (64,64), 225 chunks, VANILLA FIRST, coverage refusal exit 0 on every side
   — and it **refused** `van-c6458` on that window (171 of 225 chunks absent)
   after `region-differ` had already returned a confident 83.77% for it:

   ```text
                          van-rows-c64   van-s1-c62   vs ours(shared)
     ours(shared)              18,779        8,712        —
     ours(extent)              18,729        8,762           50
     ours(isolated)            42,085       42,873       39,796
   ```

   The two references disagree about our `shared` output by 10,067, so that
   is their own floor on this window; `isolated` is +23,306 and +34,161 and
   lands at ~42k against **both**, which is rule 6 firing. The End city
   window, both twins: `extent` **100.000000%** (the standing figure holds),
   `isolated` **99.998861%** — 168 blocks, every one `chorus_plant` or

   (Unquotable: **not the model this server runs**.)
   `chorus_flower`, the feature the End driver's own comment named in
   advance. (`ci/figures.toml` registers that reading as
   `end-city-window-isolated-model`, disqualified as **not the model this
   server runs** — it is the `isolated` arm, which is not what ships. The
   standing End figure is the `extent` 100.000000%.)
6. **What ships.** The pyramid extent — one ring fewer decorated over the
   same terrain. `tiled_world.rs` is **unchanged**: its four cases read
   0 / 0 / 66 / 608 / 23 / 986 / 12 / 2,364 against 0 / 0 / 40 / 1,397 / 23 /
   658 / 2 / 81 before, and every assertion it already made still holds,
   including §30.3's exact 23. Widening the terrain two rings past what the
   pyramid asks for changes none of those eight numbers (ORACLE rule 14).

**Killed as a null — do not re-run this:**

* **Read-isolated decoration as the shipped model.** It is the only thing
  that zeroes the gate and it costs more than it buys. Do not re-derive this
  from the gate reading alone: the gate is green under it and the End window
  is not.
* **Any margin, any extent, as a route to zero.** §31.3 killed widening;
  §32.4 kills the whole family, and it is a chain argument rather than a
  ladder: a delivered chunk that reads its neighbours' features depends on
  chunks two rings out, which depend on chunks four rings out, and no finite
  square closes. Vanilla has that dependence and pays for it by not being a
  function of its own seed (32,127 blocks between two runs of one seed in one
  chunk order). **We cannot have extent-independence and vanilla's reads.**
* **Expecting the pyramid extent to shrink the gate's numbers.** It does not
  — 2,201 to 4,059 — and it is not supposed to. A delivered chunk now misses
  an undecorated ring's writes where before it got a badly-decorated ring's
  wrong ones. Parity is what says neither is closer: 18,779 / 8,712 against
  18,729 / 8,762.

**Gate state at hand-off.** `ci/t0.sh` **T0 GREEN** on the committed tree.
`cargo test --release -p rustmc-server --test tiled_world` **3 passed, 0
failed** — `two_tiles_hold_what_one_generation_would_have`,
`a_chunk_does_not_depend_on_how_big_the_area_around_it_was` (0 / 0 / 66 / 608
/ 23 / 986 / 12 / 2,364, every assertion it already carried still holding),
and the new
`the_delivered_square_is_the_decorated_world_and_not_the_carved_one`
(122,560 blocks). `ci/end-city-window.sh` **100.000000%** against both twins.
No corpus run: this branch touches neither `ci/checks/` nor
`crates/rustmc-server/`'s gameplay paths, so `t0-remote` plus the T1 target
is the whole gate condition.

**One trap this branch fell into and climbed back out of, because the next
person restructuring a driver will meet it.** The isolated pass has no order
between chunks and therefore no barriers, so its driver is a flat work queue
— and replacing the old driver with it deleted the **level machinery the
shared-canvas models still need**. The default model then ran decoration
*sequentially*, silently, on a stage that is 47% of the pipeline's wall at
twelve workers. Nothing failed: every block was right, every gate was green,
and the only symptom was wall time nobody was measuring on that run. Both
paths are present now and `!isolation.on` picks between them.

**One thing left unexplained, deliberately.** The `isolated` arm is
byte-identical to itself within a build (two dumps, 0 of 22,118,400) but
moved ~200 blocks across three builds of this branch against code changes
that touched only the other arm. 0.5% of a 4.8x effect, on a switched-off
path, so nothing here is quoted to better than "about 42,000" — but the arm's
whole claim is purity, and anyone reopening it starts there.

**The single next item, and the result that closes it.** §32.6, and it is a
*different property* from the one this gate has been asking for. Not "a tile
equals one pass" — vanilla does not have that — but "a chunk, once
delivered, is never written again": one canvas per world instead of one per
tile, a status per chunk, at most one upgrade each, a **world-anchored**
decoration order, and delivery only once a chunk's own 3x3 has decorated,
after which `blockStateWriteRadius = 1` means nothing can reach it. **It
closes when a canvas grown from radius `r` to `r + 1` leaves every
already-delivered chunk byte-identical, and the delivered set is what
`ChunkStep.getAccumulatedRadiusOf` says it may be.** `tiles.rs` is where it
lands; `crates/rustmc-worldgen/src/status.rs` is the half that had to come
out of the jar and is already here.
