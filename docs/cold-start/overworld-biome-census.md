# Cold start — branch `overworld-biome-census`

---

### COLD START — branch `overworld-biome-census` off `5f1bd4ad` (`nether-biome-census`), 2026-08-18

**The overworld's 41 published figures are one place on the map, they hold 12
of the dimension's 55 biomes, and not one of those 12 is above its own floor.
Three squares harvested elsewhere hold 23 biomes, 16 of which resolve.** Remote
dir `~/rustmc-owcensus` on the Threadripper. Worktree `~/grit/work-owcensus`.

This branch is based on `nether-biome-census` rather than `main`, because
`region-differ --by-biome` — the whole instrument — lives there and is not in
`main`. Size it with `git cherry origin/nether-biome-census overworld-biome-census`.

## The thing to know before anything else

**Every published overworld figure is centre chunk (64,64) at r=16.** All 41
entries in `ci/figures.toml` with `dimension = "overworld"`, across seeds
20260722, 12345 and 31415926. `docs/PARITY.md` §2's two narrower rows — chunks
57..71 and chunks 59..69 — are sub-squares of that same centre. Three points on
the map, and the great majority of this project's published parity rests on
them.

## What is in them

`region-differ diff X X --by-biome` is zero mismatches and a free
decomposition. Six biomes each:

```text
  20260722 r=16                    12345 r=16
    savanna           42.74%         frozen_ocean      78.70%
    lukewarm_ocean    39.34%         snowy_plains       8.43%
    river              7.44%         snowy_beach        8.30%
    beach              6.48%         frozen_river       1.88%
    sulfur_caves       3.49%         snowy_taiga        1.84%
    windswept_savanna  0.51%         sulfur_caves       0.84%
```

r=18 on 12345 adds `ice_spikes` at 0.10% and nothing else. **Twelve of
fifty-five — CORRECTED to eighteen, see below.** The thirty-seven with no
figure at all include `desert`, `swamp`, `taiga`, `badlands`, `deep_dark`,
`dripstone_caves`, `mangrove_swamp`, `cherry_grove` and `pale_garden`.

Seed 31415926's r=16 square holds ten of them — `lukewarm_ocean` 61.74%,
`beach` 22.17%, `savanna` 5.92%, `river` 3.77%, `sulfur_caves` 2.20%, `jungle`
1.72%, `lush_caves` 0.94%, `plains` 0.73%, `deep_lukewarm_ocean` 0.34%,
`forest` 0.25%, `sparse_jungle` 0.23% — and this doc first listed six of those
as unmeasured. They sit in the window §2 calls corroboration only, so they are
in a square carrying no quotable figure, which is weaker than measured and
different from unmeasured. The reference note at the bottom says how the window
was missed.

So "the overworld at r=16" is savanna-and-warm-ocean on the flagship seed, and
**78.7% deep frozen water** on the seed carrying the only quotable r=16
overworld exactness this project has.

## THE NULL THAT IS STRONGER THAN THE AGGREGATE ONE

Both r=16 rows were already disqualified by rule 6 in aggregate. Per biome,
against the N=2 floor from the same pair, **not one biome of either window is
above its floor**:

```text
  20260722   ours  floor   x        12345          ours  floor   x
   lukewarm  2387   6941 0.34        frozen_ocean  1834  13896 0.13
   savanna   1714   8105 0.21        sulfur_caves  1213   1698 0.71
   sulfur     874  12969 0.07        snowy_beach    244   1104 0.22
   river      434    730 0.59        frozen_river   145    360 0.40
   beach      135    697 0.19        snowy_plains    79    472 0.17
   w_savanna    0    118 0.00        snowy_taiga     38    249 0.15
```

**Third window, added by correction:** `lush_caves` 5765 / 5672 = **1.02x**,
the only biome in any of the three published squares that clears its floor;
then `jungle` 0.66, `savanna` 0.61, `lukewarm_ocean` 0.18, `sulfur_caves` 0.11,
`beach` 0.06, `river` 0.01.

An aggregate under its floor leaves open that some sub-window still resolves —
which is exactly what the nether was doing, `soul_sand_valley` at 14.1x hiding
inside an r=16 aggregate below its own. **Here no sub-window does.** The r=16
overworld square is exhausted per biome and a sharper reading cannot produce
anything. The masked residuals fell out at 5,544 and 3,553 against the 29,490
and 77,067 §2 still carries — both of those are **not quotable** and are named
here only so the movement is visible.

## What moved

Three r=7 squares, seed 20260722, found by `ci/ow-biome-scout.sh` and ranked by
`ci/ow-biome-coverage.py`, then harvested as fresh twin pairs:

```text
  A  (-1056,-672)  12      E  (-960,672)   7      B  (-480,-1440)  9
  J  ( 960, -192)   9      O (-1248,-672)  4
```

J and O were harvested second and each answers a question the first three
raised. **J is the jungle-dominant square** the artefact said had to exist
before `cocoa` could be ported — `jungle` is 3.83x its floor there against
0.53x in B. **O is the control**: four ocean biomes, **zero feature refusals in
the whole square**, and `cold_ocean` byte-identical to vanilla over 1,253,376
blocks (registered UNQUOTABLE, because its floor is 0 as well).

**40 of 55 overworld biomes now carry a figure, against 18 when this branch
started.** The five squares hold 29 between them, 22 of which had never been
inside any window this project had generated. Fifteen still have none:
`bamboo_jungle`, `cherry_grove`, `deep_frozen_ocean`, `eroded_badlands`,
`flower_forest`, `frozen_peaks`, `mushroom_fields`, `old_growth_pine_taiga`,
`old_growth_spruce_taiga`, `savanna_plateau`, `snowy_slopes`, `taiga`,
`warm_ocean`, `windswept_forest`, `windswept_gravelly_hills`. The scout reaches
all fifteen and three more squares would cover them.

**23 biomes against 12, in squares each a fifth of the area. 16 resolve; the
published corpus has 0.** Sharpest: `river` 10.03x, `old_growth_birch_forest`
8.79x, `dripstone_caves` 7.49x, `windswept_hills` 5.64x, `meadow` 3.52x,
`deep_dark` 2.67x, `sparse_jungle` 2.57x.

**Corroborated across independent windows**, which is what makes them results.
`deep_dark` in A and B, 900 chunks apart, at 2.67x and 1.57x.
`dripstone_caves` in all three, at 7.49x, 3.63x, 1.87x.

`river` is the cleanest illustration of the area: **10.03x its floor here and
0.59x in the published square.** One biome, two windows. The floor is a
property of the window.

## The repair

**`leave_vine`, and `swamp` went 21,030 -> 4,506 — 4.97x its floor to 1.07x.**
160 declines of `straight trunk, blob foliage, decorators
["minecraft:leave_vine"]`, where both placers were already supported and one
decorator refused the whole tree, because `TreeConfig::is_supported` is an AND.
Ported from `LeaveVineDecorator`; one enum variant and one function. The only
subtlety is that an accepted side places a **run** — `addHangingVine` walks
down through air up to four more blocks — and that run spends no randomness, so
the draw sequence stays exactly four `nextFloat()` per leaf.

## The second repair, and the lesson it carries

**`cocoa`, and it is the more important of the two because it went the "wrong"
way.** 344 declines in window J of `straight trunk, blob foliage, decorators
["minecraft:cocoa", "trunk_vine", "leave_vine"]`. Ported from
`CocoaDecorator` — one `nextFloat()` for the whole decorator before the logs
are walked, a 0.25 **literal** per side that is not the configured
probability, `Direction.Plane.HORIZONTAL` = NORTH, EAST, SOUTH, WEST read off
the jar, and `nextInt(3)` for the age drawn only when the side is air.

Block census: `cocoa` 0 → 65 of vanilla's 67, `jungle_leaves` 0 → 8,984 of
12,001, `vine` 532 → 9,388 of 11,569, `oak_leaves` overshoot +7,234 → +1,265.
About twenty thousand blocks of jungle canopy that did not exist.

Positionally: `sparse_jungle` **improved in two independent squares**
(13,116 → 11,793 in J, 7,976 → 7,547 in B) and `jungle` **got worse in both**
(39,874 → 41,024, 6,818 → 7,498).

> **A refused tree spends no randomness at all, while vanilla spends the whole
> tree's draws.**

So every unported tree desynchronises the stream for everything decorated after
it in that chunk. `mega_jungle_trunk_placer` is still unported — 93 declines in
J, 69 in B — so `jungle` still desyncs and the small trees `cocoa` now places
land displaced. `sparse_jungle` has no mega trees, which is why it is the biome
that validates the port. **Port a biome's tree family whole, or expect its
positional residual to move the wrong way — and read the block census beside
the residual on any port that unblocks a refusal.**

## What is hiding there, by name

Block-family census of window A against its own reference — no mechanism shared
with the positional diff:

```text
  sculk           30184 -> 10       deep_dark, not generated at all
  sculk_vein       9053 -> 412
  pale_oak_leaves  6732 -> 0        pale_garden, no tree placed
  pale_oak_log     2397 -> 0
  dark_oak_leaves  6017 -> 0        dark_forest, no tree placed
  dark_oak_log     2185 -> 0
  dripstone_block 12033 -> 10440    large_dripstone, 543 declines
```

**`minecraft:large_dripstone` is this port's largest feature refusal by COUNT
anywhere — 4,569 declines across the three squares — and only the fifth largest
by blocks.** Rank by blocks, corroborate by count. Dark oak and pale oak are
one item: `dark_oak_trunk_placer` is not in
`TrunkPlacer::is_supported` (`Straight | Forking | Fancy`).

## THE SINGLE NEXT ITEM

**Port `sculk_patch_deep_dark` and `sculk_vein`**, then
`mega_jungle_trunk_placer` as the other half of `cocoa` — the only item whose
closing result is already measured (beat 41,024; 39,874 is what `jungle` read
before `cocoa`).

**Why sculk is first.** Largest unported family by
blocks (38,815 in window A alone); the only overworld defect corroborated
across two independent windows; and both windows already exist and are
harvested. **The result that closes it**: `deep_dark` below 17,695 at
(-1056,-672) and below 21,900 at (-480,-1440). `docs/PARITY-ROADMAP.md` §58 has
the ranking and the two items behind it.

## Nulls — do not re-run these

* **Do not investigate `mangrove_swamp`.** 82,596 against a floor of 214,991 —
  vanilla disagrees with itself over **2.4% of the biome**. It is the
  overworld's `basalt_deltas`. Its trees also need
  `upwards_branching_trunk_placer` and a root placer.
* **Do not widen N on the published r=16 squares.** `sulfur_caves` carries
  43.87% of the 20260722 floor in 3.49% of the window, and floors only grow
  with N (§4). The biome furthest under its floor stays there.
* **Do not port `cocoa` blind.** Jungle's
  `["minecraft:cocoa", "trunk_vine", "leave_vine"]` is 186 declines and is now
  one decorator from unblocking — but `jungle` sits at **0.53 of its floor** in
  window B, so the port could be made and nothing this project owns could tell
  whether it worked. Harvest a jungle-dominant square first; the scout finds
  one without harvesting anything.
* **`deep_ocean` at 2.46x is terrain or carvers and nothing else.** Window O
  declines NO feature at all over 225 chunks and `cold_ocean` is byte-identical
  to vanilla in the same square, so the 555 blocks cannot be an unported
  feature. The most narrowly-scoped defect on the branch and the only one with
  no candidate cause.
* **`forest` is measured and needs nothing.** 17 blocks against a floor of
  1,046 in A, 15,413 against 17,406 in B. Both under.
* **Do not quote an aggregate overworld exactness.** Same as `DETERMINISM` §47
  for the nether; the overworld's weights are worse.

## References — the state of them, which is not what the docs said

* `gen-wide-20260722-r16-ow-{a..i,rr-a,rr-b}` all **exist** in
  `~/rustmc/oracle-work` with region data. The 2026-08-09 header in
  `ci/ow-r16-window.sh` describing the pair as destroyed is out of date.
* **Eight of the nine carry no `HARVEST-ORDER` at all.** `-a` .. `-h` are
  region data and nothing else. Their order was DERIVED on 2026-08-10 by
  `ci/classify-20260722-order.sh` into `~/w22-logs/stamps/<same-name>/` — a
  parallel tree of stamps whose data lives elsewhere. `ci/harvest-order.sh` is
  the resolver that knows this, and refuses rather than defaulting.
* **Seed 31415926 HAS overworld references — twenty of them — and this branch
  said otherwise first.** They are `floors-ow31415926r16-{a..h,rr-a,rr-b}`
  under `~/rustmc-floors/oracle-work`, all stamped, and NOT named `gen-wide-*`
  like every other window's. Two searches keyed on the other naming family —
  one on the presence of a `HARVEST-ORDER`, one on the `gen-wide-` prefix —
  each returned nothing and each looked conclusive, and a biome count was
  published on that basis. **If you are looking for a reference on this box,
  search by CONTENT and not by name.** `docs/DETERMINISM.md` §48.5 is the
  correction.
* Seed 12345 r=16 is healthy: 46 references in `zx`, 4 in `zx_zrev`.

## Tools added

* `ci/ow-biome-scout.sh` — `RUSTMC_THROUGH=biomes` stops the dump after the
  biome stage, about **one second for an r=4 square**, and a self-diff of the
  result is a full biome decomposition. A thousand candidate centres cost
  minutes of our own CPU and **nothing of vanilla's**: the corpus lock is not
  needed until the windows are chosen.
* `ci/ow-biome-coverage.py` — reads the scout and greedy-set-covers the
  unmeasured biomes. Six windows cover 30 of the 43 on seed 20260722 alone.
* `ci/ow-biome-census.sh` — composition, per-biome floor and per-biome residual
  for both published r=16 squares.
* `ci/ow-new-windows.sh` — all sixteen new figures.
* `ci/harvest-order.sh` — order resolution with three distinct outcomes
  (stamped beside its data / derived from the stamp tree / refuse), and it
  refuses a stamp directory passed as a reference.
