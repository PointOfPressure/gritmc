# Cold start — branch `nether-biome-census`

---

### COLD START — branch `nether-biome-census` off `b94f3ebf`, 2026-08-18

**The nether went from "the honest weak spot" to a list of four names, and the
headline moved 8,472 -> 104 on the window it was quoted from.** Remote dir
`~/rustmc-nether` on the Threadripper. Worktree `~/grit/work-nether`.

## The thing to know before anything else

**Every vanilla reference `docs/PARITY.md` §5 rested on was destroyed in the
2026-08-07 disk failure.** `van-nether-c64`, `van-nether-c100`,
`van-nether-s2-c64` and their `-b` twins are nowhere on the box —
`find / -name 'van-nether*'` matches nothing. So the published 99.885% had no
artefact behind it and could not be re-taken. Sixteen replacements were
harvested on 2026-08-18 (`GEN_ORDER=rows`, order-stamped, 45 s each), plus six
more for the two biomes nobody had ever measured.

## What moved

All twin-masked, vanilla first, `--ybox 0 127`, dump order `zx` **read off the
reference's `HARVEST-ORDER`**, not asserted:

```text
  window                          before          after
  20260722 c(64,64)   r=7          8,472*           104
  20260722 c(100,100) r=7         10,038*           253
  20260722 c(64,64)   r=16       254,799*       179,653   (aggregate, unquotable)
                                                   70,130   soul_sand_valley, 14.1x floor
  * against references that no longer exist
```

Two causes and they are separable, which was measured rather than assumed:

* **The port genuinely improved** — fifteen days of `main` since 2026-08-02.
* **`SoulFireBlock.canSurvive` was ported on this branch** and is worth
  **327 -> 104** and **477 -> 253** on the two 20260722 r=7 windows on its own.
* **The dump order was NOT the cause**, and this was the control: the same
  dump against the same reference is 335 in `zx`, **700 in `default`** and
  **7,638 in `zx_zrev`**. So order is worth 2x, not 25x, here — the 8,472 was
  not an order artefact.

## The four things that are unmatched, and nothing else is

Charged to vanilla's own saved starts at **margin 0** on the r=16 window's
`soul_sand_valley` residual of 70,130:

```text
  46,856   66.8%   bastion_remnant (55,63)      105 pieces, NOT PORTED
  21,591   30.8%   bastion_remnant (70,49)       89 pieces, NOT PORTED
   1,261    1.8%   outside every structure box   BELOW the 3,442 biome floor
     313    0.4%   ruined_portal_nether (48,64)  portable today, never placed here
      75    0.1%   two nether_fossil boxes       of ~137 starts
```

**Margin 0 and not the tool's default 16.** At 16, 13,844 blocks charge to
three `nether_fossil` boxes and read as a defect in a structure this port
*does* place. They are `polished_blackstone_bricks` and `blackstone` — bastion
material — and the rings overlap. `bastion_remnant` declares
`terrain_adaptation: none`, so it beards nothing and margin 0 is its correct
margin.

## THE BIGGEST THING FOUND, and it was found last

**All five nether biomes are now measured, and the two that never had been are
where the largest unported thing in the dimension lives.** `crimson_forest`
and `warped_forest` appear in **none** of the four windows this project had
ever generated, so they were unmeasured for a window-finding reason. Two
windows were found in an afternoon by dumping candidate centres at r=4 and
reading the per-biome denominators off a **self-diff** — `region-differ diff X
X --by-biome` is zero mismatches and a free biome census, which is the cheapest
trick on this branch.

```text
  window                biome            ours          of    exactness    x floor
  31415926 c(0,0)       crimson_forest  79,911   6,674,283   98.802703%      1.7
  31415926 c(-200,200)  crimson_forest  58,229   4,819,195   98.791728%      1.6
                        warped_forest   13,795   1,795,823   99.231829%      1.8
                        nether_wastes    5,053     722,927   99.301036%    297.2
```

Two independent crimson windows a thousand blocks apart agree to **0.011
percentage points**. `worldgen-dump`'s REFUSED block names the entire cause and
holds nothing else: **`huge_fungus` (3,871 / 4,103), `nether_forest_vegetation`
(2,870 / 3,425), `weeping_vines` (2,378 / 1,913)** — three feature families,
none ported, **~152,000 blocks**, which is more than twice the r=16 window's
whole adjudicable residual.

`nether_wastes` is below its floor on three windows and at **297x** it on this
one, because this one borders the forests and those features spill over. Same
biome, four windows, and the floor is a property of the window.

## THE SINGLE NEXT ITEM

**Port `bastion_remnant` for the number, `huge_fungus` for the player.** The
bastion is 97.6% of the residual on the sharpest window (14.1x its floor); the
three forest features are 2x more blocks on windows that only resolve at
1.6–1.8x. Whichever is picked, say which of the two goals it is for.

`bastion_remnant` is 97.6% of the only strongly adjudicable residual in the
dimension. This port already walks jigsaws (villages, trial chambers, gated
piece-for-piece against vanilla's `Children[]`); bastion adds its own pool set,
`pool_alias` handling and processors. **The result that closes it**:
`soul_sand_valley` at seed 20260722 r=16 below 3,442, with the attribution
table empty of structure rows. `docs/PARITY-ROADMAP.md` §57 has the ranking and
the three items behind it.

## Nulls — do not re-run these

* **`-Dmax.bg.threads=1` does NOT make a nether harvest reproducible.** Three
  clamped runs of one window give pairs of **34, 438, 460** against **75, 134,
  151** for three default-pool runs: the clamped spread is *wider*. Mechanism
  from the pinned jar: `Util.makeExecutor` is
  `new ForkJoinPool(maxAllowedExecutorThreads(), ...)` and that argument is
  target **parallelism**, not a thread cap — such a pool spawns compensation
  workers. Counted: **2** `Worker-Main` threads clamped against 23 unclamped,
  no property warning. `docs/DETERMINISM.md` §1 said "exactly one" and is
  corrected; §46 is the whole finding. **The six harvests fall into three
  groups and `clamp-c` groups with two DEFAULT-POOL runs**, so the clamp does
  not even select the group.
* **Do not open an investigation on `basalt_deltas`.** 109,518 blocks at r=16
  against a vanilla floor of 313,045..398,660, 100% of it outside every
  structure box. Vanilla is 500x less reproducible per block there than in
  soul sand valley. No instrument this project owns can see a defect in it.
* **Do not quote an aggregate nether exactness.** It is a weighted average
  whose weights are the window's biome areas. 15.6% of the r=16 window carries
  98.9% of its floor. `region-differ diff --by-biome` exists for this;
  `docs/DETERMINISM.md` §47.
* **Seed 31415926 does not adjudicate this port in any biome.** Its basalt
  floor is 3.4x our residual and its other two floors exceed our residual
  outright. It is corroboration, and §5 now says so.
* **The 20260722 c(64,64) r=7 window has stopped adjudicating us** — residual
  104 against a mask of 151. Deltas on it are still exact; levels are not.

## Tools added

* `region-differ diff --by-biome` — charges every compared position and every
  mismatch to the biome **vanilla** records there, and prints window share
  beside residual share. Under `--by-biome`, `--positions` gains a sixth field
  so an attribution and a decomposition can be joined; that join is what
  caught the margin-16 misattribution above.
* `ci/nether-biome-window.sh` — re-takes all eight published per-biome figures,
  reading the dump order off each reference and refusing rather than guessing.
* `ci/gen-vanilla-region.sh` now stamps `jvm_opts=` into `HARVEST-ORDER`. A
  clamped reference and a default-pool one are samples of two distributions and
  nothing on disk could tell them apart before.
* `golden_survival`'s `UNPORTED_CEILING` — the header promised a ratchet since
  the file was written and no line enforced it.
