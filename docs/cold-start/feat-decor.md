# Cold start — branch `feat-decor`

---

### COLD START — branch `feat-decor` off `c9bfcefb5` (`feat-worldgen-refused-on-main`), 2026-08-18

Three items from the decorator area: `multiface_growth`/`glow_lichen`, the huge
mushrooms, and `pale_moss`. Worktree `~/grit/work-decor`.
`docs/PARITY-ROADMAP.md` §79.

## The thing to know before anything else

**A feature can spend vanilla's draws exactly and still write the wrong
blocks, and the project's cheapest instrument reports it as identical.**

§76 introduced the per-feature draw table — vanilla's `"draws"` per feature
from `DecorateGolden`'s vectors against our `ATTEMPT ... draws=N` — and used
it to rule out sixty-seven of sixty-nine features in a jungle chunk with one
run a side. That was the right move and the table is still the right first
move. But `MultifaceSpreader.spreadAll` **takes no random at all**: it is
every face crossed with every direction, walked in `Direction.values()` order
with nothing shuffled. The sculk features run through it, and they sit inside
that identical sixty-seven while putting sculk in the wrong places.

> "67 of 69 byte-identical" was **67 of 69 draw-identical**. Take the coarse
> table first. Do not read a match in it as a match in the world.

The instrument that *can* see it is already in the repo and was not being
used: `ci/decorate-once.sh` writes vanilla's own world into its vectors, under
`worlds[0].chunks[].runs` as run-length pairs over a `yzx` index. So any block
vanilla has can be read out and compared against ours without a second vanilla
run. `/home/sayer/grit/work-decor-logs/vanprobe.py` is the ten-line reader.

**The snapshot is the world as the traced feature saw it**, not the final
world — with `-Ddecorate.draws=glow_lichen` the centre chunk carries sculk
(earlier step) and no lichen. That is what you want for reading a feature's
inputs, and it is a trap if you expect the finished chunk.

## `glow_lichen` was never the bug

Vanilla 478 draws against our 475 in chunk (957,-195), reproduced exactly.
The streams are **identical for 138 draws**. At 139 vanilla enters the feature
at `[15324,-46,-3120]` and spends ten — a four-draw shuffle of five valid
directions (`glow_lichen` has no `can_place_on_floor`, so `validDirections` is
five, not six), `nextFloat` for `chanceOfSpreading`, then `allShuffled`'s five
— and we spend none, because the block is not air on our side.

```text
  y     vanilla                          ours
  -48   air                              sculk_vein[north]
  -47   sculk_vein[north]                air
  -46   air                              sculk_vein[north]
  -45   air                              sculk_vein[east]
  -44   air                              sculk_vein[east]
```

§66.5 inferred "ours converts UPWARD where vanilla converts DOWNWARD" from
counts. It can now be read out of both worlds a block at a time.

## Nulls — do not re-run these

* **The discharge experiment is done and it does not resolve.** `configured.rs`
  discharges all six faces where vanilla's `attemptPlaceSculk` skips
  `direction.getOpposite()`, and left the plumbing for the alternative in
  place. It is now `RUSTMC_SCULK_DISCHARGE=skip`, measured on positions:

  ```text
                          sculk           sculk_vein
      vanilla               196                   76
      discharge all six     337   1.72x           59   0.78x
      skip the source       300   1.53x          184   2.42x
  ```

  Skipping recovers the vein at (15324,-47,-3120) that this port lacks and
  takes sculk 1.72x -> 1.53x, and triples the veins. **The two settings fail
  in different directions** — all-six is short of veins and long on sculk,
  skip is long on both — which no single sign error in that loop produces. So
  the inversion is upstream of the discharge, not in it.
* **`spread_vein` is already wired after a conversion.** `sculk_spreader.rs`
  calls it at the right place; a reading that says the `spreadAll` is missing
  is wrong, and this file said so for a while before the call was found.
* **Do not adjudicate the mushroom port on `dark_forest` or `pale_garden`.**
  0.67x and 0.52x their floors in window A. Only the census speaks.

## What is ported, and the number that went the wrong way

Both huge mushrooms: `AbstractHugeMushroomFeature`, both caps,
`HugeMushroomFeatureConfiguration`, `placeMushroomBlock` against
`replaceable_by_mushrooms`, `isValidPosition`. Five tests.

`dark_forest_vegetation`'s first divergence moves **793 -> 813** and the
`nextInt(3)`/`nextInt(12)` pair is gone from the diff. **The block census goes
the other way**: 45 brown cap + 4 stem in vanilla's 3x3, 0 before, 135 + 27
red + 23 stem after — absolute error 49 -> 136. The geometry is right (45 is
exactly `7*7 - 4`); this port builds three mushrooms where vanilla builds one,
downstream of the still-open divergence at draw 814.

Two things the bytecode settles: the state provider is asked at the **origin**
in both `makeCap` and `placeTrunk`, not at the block being written; and the
red mushroom's `isValidPosition` is a single column forever, because it passes
`-1` where the red override reads `height`.

**The dark oak overshoot §76 hoped this would explain is still there.** Same
3x3, final world both sides: `dark_oak_leaves` 398 vanilla against 922 ours
(2.32x), `dark_oak_log` 165 against 317 (1.92x). The mushroom pair was the
divergence at 793 and it is gone; the overshoot is downstream of 814.

## `pale_moss`: the obstacle is `UndoWorld`

§76 refused it for "the registry and a `FeatureWorld`". §79.6's first draft
said it was one injected closure. **Both are wrong.** The attempt that found
out was written, built against and reverted; it is not in this branch.

* Its feature is **already ported** — `pale_moss_patch` is a
  `vegetation_patch` over a `simple_block` with a `weighted_state_provider`.
* The registry **is** reachable at parse time through `resolve_configured`;
  threading it into `TreeConfig::parse` is eight call sites, six of them
  tests. §76 over-priced this half.
* **`tree::place` runs decorators inside an `UndoWorld`**, which implements
  `PredicateWorld` and `TreeWorld` and nothing else. A nested
  `ConfiguredFeature::place` wants a `FeatureWorld`. An injected closure does
  not escape it: whatever the closure takes, it is handed the wrapper.
  Handing it the inner world does not type-check — the wrapper holds the only
  `&mut` — though it would be *sound* if it did, because `UndoWorld::set_block`
  writes through immediately and `pale_moss` is first in both pale oak
  decorator lists.

Two routes: give `UndoWorld` the ~20 delegating `FeatureWorld` impls (the
stronger answer, rollback then covers the nested feature), or run decorators
outside the wrapper as vanilla does (smaller, but it changes when a refusal
can still put the world back and needs its own justification).

Worth it: `pale_moss` is the **only** entry still refusing on both
`minecraft:pale_oak` and `minecraft:pale_oak_creaking` — their placers and
`creaking_heart` all landed on the base branch. It is `pale_garden`'s 9,129
missing blocks in window A behind one decorator.

The decorator itself is ordinary: `logs - 1` draws for a `shuffledCopy` whose
only use is a `min` — and the shuffle still decides it, because
`Collections.min` keeps the first element attaining the minimum and a dark
oak's two-by-two trunk ties the lowest y four ways every time — then one
`nextFloat` for the ground patch, one per log, one per leaf, and a hanger loop
that draws only while there is air below and writes its tip unconditionally.

## A free answer to §76's third item

§76 left `dripstone_caves` A at 6.36x its floor and asked for its residual to
be decomposed. One run a side on chunk (-1050,-668), seed 20260722
(`old_growth_birch_forest`/`dripstone_caves`):

```text
  61 features   60 identical   1 differ
    step  7  idx  5   minecraft:pointed_dripstone   van 3477   ours 3374
```

Not `large_dripstone`, which spends vanilla's draws here. 103 draws short,
nothing else in the chunk to explain it. A starting position, not a diagnosis
— and §79.2's caution applies to the sixty.

## T0

**RED, and RED identically on the base.** `ci/figure-provenance.py --check`
reports 56 figures re-derived and 56 whose derived commit is not in their
provenance; the same command in a clean worktree at
`feat-worldgen-refused-on-main` reports the same 56 and exits 1. It stops T0
before the test suite, so run `cargo test --release --workspace` separately.

`golden_carver_chunk` is separately known-bad on the base — see §76.

## Two environment repairs this lane needed

* `cargo` is **not on the non-login shell's PATH**. `bash -lc` does not find it
  either; it is at `~/.cargo/bin`. A `cargo build | tail` reports exit 0 on
  `command not found`, which is how this cost a run.
* A fresh worktree has **no `data/vanilla` and no `oracle-work`**. Symlink
  `oracle-work/*` to `~/grit/repo/oracle-work/`, but **copy** `data/vanilla`
  from a live worktree: the one in `~/grit/repo/data/vanilla` is from 3 August
  and predates `randomlyTicking`, so `worldgen-dump` refuses it.

## THE SINGLE NEXT ITEM

**Decompose the sculk displacement by block pair the way §66.3 decomposed
`onDischarged`.** The oracle now hands you vanilla's blocks for free, so the
census can be taken per position rather than per count, and the two failure
shapes in the discharge table are the thing to explain: what makes one setting
short of veins and long on sculk while the other is long on both. Start at
(15324,-47,-3120) in chunk (957,-195) at seed 20260722, where vanilla has one
vein and this port has three.

Second: the divergence at draw 814 of `dark_forest_vegetation`, which is a
position this port rejects before the selector and vanilla does not — the same
shape as the jungle's, and the thing standing between the mushroom port and a
census that clears.
