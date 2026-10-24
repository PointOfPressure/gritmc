# Cold start — branch `worldgen-decoration-margin`

Every `### COLD START` entry ever written for this branch, newest last.

---

### COLD START — branch `worldgen-decoration-margin` off `main` (`d049ef5a`), 2026-08-12

**THE ONE LINE: no generator code changed. Tests and docs only. Any
adjudication in flight against a dump of `d049ef5a` is still valid against this
branch's tip — `git diff d049ef5a..HEAD -- crates/rustmc-worldgen/` touches
nothing but a doc comment, and `DECORATION_MARGIN` is still 2.**

**Item: `DECORATION_MARGIN = 2` does not converge, and
`tiled_world.rs::a_chunk_does_not_depend_on_how_big_the_area_around_it_was`
asserted that it does and passed.** The brief offered three outcomes. **The
answer is outcome 3 — no margin converges — with outcome 1's first half
attached: the gate was under-powered as well, and one line of scope fixes it.**

Worktree `~/grit/work-decor`, remote dir `~/rustmc-decor`, logs in
`~/grit/private/decor-logs/{run1,run2,run3}.log`. Write-up:
`docs/DETERMINISM.md` §31.

**What was measured, in order:**

1. **§30.3 reproduces to the block** on its own dumps
   (`~/rp-sulfur19/12345/overworld/dump-zx-r{16,18,20}`), which are dumps of
   this tip's generator: 23 over `48 48 80 80`, 2,713 over `46 46 82 82`.
2. **The silent-drop hazard witnessed.** The same r=16/r=18 pair diffed over
   the *wider* window returns `chunks=1089 mismatches=23` — identical to the
   narrow window, `EXACTNESS` and all — because 280 of 1,369 chunks are absent
   from our r=16 dump. The `ebed533a` coverage refusal fires and discriminates
   (exit 3 with the count, exit 0 on the five covered windows). `region-differ`
   on its own will not tell you.
3. **The ladder.** `probe_margin_context.rs` grew `RUSTMC_MARGIN_MAX_RADIUS`
   (capping at radius 4 is why the shape had never been read) and a per-line
   centre-chunk column. Seed 12345's delivered square is **never zero at any
   of twelve widths out to a 14-ring context**; its centre chunk settles at
   depth 9. Seed 0's square is at 45,145 at a 10-ring context. Seed 20260722
   is **0 everywhere**, which is the good news: every parity figure this
   project publishes sits on a stable square.
   **And seed 31415926, which that probe's header has called stable since
   2026-07-28, reads `0, 0, 0, 0` over the four rungs it used to run and
   `833, 1917, 2916, 2439` over the next four.** The ladder stopped before the
   effect started. Of the three seeds the header called clean, only 20260722
   survives the longer one — a second instance, in the same file, of the same
   defect class as the gate this branch was sent to fix.
4. **Why no width works, and it is the shape not the number.** A tile of
   radius `r` decorates to `r + m` and delivers to `r`, so its rim is at depth
   `m` below the outermost decorated ring *for every m*. Widening the margin
   moves the rim out with it. Injections: margin 1 / 2 / 4 give **5,753 /
   2,201 / 1,130** blocks — decreasing, never zero.
5. **The gate did not respond to the margin at all.** The shipped test passes
   at margin 1, 2 **and** 4. Margin 1 is not hypothetical — it is what this
   tree shipped until 2026-07-27 and `pipeline.rs` records it being wrong by
   604 blocks. The strengthened gate fails at all three.

**What landed, two commits in that order deliberately:**

* **`a945b479` is RED and is meant to be.** It strengthens the gate to four
  cases (three seeds; at 12345 a second centre) comparing every delivered
  chunk, and fails at 2,201 blocks. The record shows the test catching it.
* The commit after it carries the no-width-converges measurement and converts
  the assertion to what the tree can honestly hold: **exact zeroes at seed
  20260722**, **exactly 23 at seed 12345 centre (48,54) radius 0 -> 1**, and
  "some blocks move" elsewhere.

**The 23 is §30.3's 23.** Same chunk (48,54), same first position
`(768,63,865)`, same `grass_block[snowy=false] -> grass_block[snowy=true]`.
§30.3 took it from a 37x37-chunk dump against a 41x41 one; the unit test takes
it from a 5x5 generation against a 7x7 in the time it takes to generate 74
chunks. The effect was always short-range enough to be a unit test; nothing had
ever pointed a tile at that chunk.

**Killed as nulls — do not re-run these:**

* **Raising `DECORATION_MARGIN`.** Not "expensive", *structurally unable* to
  reach zero (§31.3), and measured at three widths. Do not open this again
  without a change to how the rim is generated.
* **"Contamination reaches exactly two rings."** It reaches depth 3 in the
  (64,64) window at seed 12345 and depth 8 in a tile at the origin at the same
  seed on the same tip. The 2 in "two rings" and the 2 in the margin are
  different quantities that coincide by arithmetic accident (§31.7).
* **A third `ci/checks/t1-known-red.toml` entry.** Considered and declined —
  reasoning in §31.6. It is not a red one ring short of green.
* **Blaming the instrument.** `Through::Carvers` is 0 at every seed and every
  rung out to a 14-ring context; terrain is per-chunk pure and the movement is
  decoration alone.

**Stale caveats fixed in the same commit, because they read as authoritative:**
`pipeline.rs`'s "two is where it stops moving" (one seed's answer read as the
constant's), `probe_margin_context.rs`'s 2026-07-28 decoration column (seed 0
now reads 2, 81, 2736, 7383 — the generator has moved), its "both parity seeds
are byte-stable" (seed 12345 did not exist as a parity seed then and moves 40
at the first rung), `tiles.rs`'s "whatever reaches a chunk from more than two
rings away has to be found first" (there is nothing to find), and
`world.rs`'s player-facing caveat naming only seed 0.

**The single next item, and the result that closes it.** Generate-once
status-based generation over one canvas — the fix `tiles.rs` and
`probe_margin_context.rs` both already name and which §31 now supports with a
ladder instead of an argument. **It closes when
`a_chunk_does_not_depend_on_how_big_the_area_around_it_was` can assert
`Some(0)` on all four cases and does.** That is a rewrite of how the world
grows, not a worldgen feature, and it is the server track's to schedule; it is
also the only thing that makes tiling honest and makes a parity figure at a
seed other than 20260722 mean what it says. Nothing else on this item is
outstanding.
