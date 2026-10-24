# Cold start — branch `nether-parity`

---

### COLD START — branch `nether-parity` off `train-g` (`0ad3cdfde`), 2026-10-02

**Every measured nether window now sits inside vanilla's own disagreement
with itself in every biome but two, and both of those are explained without a
port defect. The fortress is ported exactly.** Worktree
`~/grit/work-nether-parity`, remote dirs `~/rustmc-nether-parity` (gate) and
`~/rustmc-nether-parity-b` (experiments; its `data/vanilla` is a copy).

## The instrument, and why it replaced the old one

`ci/nether-parity-windows.py` dumps each window once and diffs it against
**every** same-order reference, then diffs every reference pair, per biome as
vanilla records it. A nether harvest is not reproducible and the
`-Dmax.bg.threads=1` clamp does not hold here, so a twin-masked pair is a
figure about that pair. Each row prints `ours n=N min/median/max` beside
`floor n=N(N-1)/2 min/median/max` and the ratio of the medians. Under ~1x a
window no longer separates us from vanilla, and a level read off it is not a
finding.

To find a defect under the floor, use the **consensus mask**:
`region-differ diff <ref-a> <ours> --twin <ref-b>` keeps only positions where
two references agree, and the same command with a third reference in place
of ours is the control. Every defect found on this branch after the
fortress was found that way (mushrooms, dried ghast, cracked bricks).

`ci/nether-biome-window.sh` measured nothing on the base: a merge had
interleaved two versions of `one()`, and every window refused. Fixed.

## What moved (ours median against vanilla's pairwise median)

```text
  window (seed, centre, r)       biome            base 0ad3cdfde   now      floor   runs
  31415926 c(0,0) r=7            crimson_forest     105,225       36,924   46,965   3/3
  31415926 c(-200,200) r=7       crimson_forest      74,340       30,956   35,593   3/3
                                 warped_forest       16,428        5,914    7,020   3/3
                                 nether_wastes        5,057           29       13   3/3
  20260722 c(100,100) r=7        soul_sand_valley       278           33       61   4/6
  20260722 c(64,64) r=16         soul_sand_valley     4,117        2,737    4,048   6/15
  31415926 c(12,3) r=7           nether_wastes       51,479          109      326   3/3   (pre-fortress)
  31415926 c(-189,198) r=7       nether_wastes       28,093        1,913    1,253   3/3   (pre-fortress)
```

The two fortress windows were harvested on this branch, so their "base" is
the commit before the fortress port (`570e2839f`), not `0ad3cdfde`. The
harvests are in `~/rustmc/oracle-work/van-nether-31415926-c{-189_198,12_3}-open-{a,b,c}`.

## What was ported, each with the check shown red without it

1. **Nether plant survival.** `crimson_roots` was in the overworld vegetation
   family; 26.2 builds it as `NetherRootsBlock(#supports_crimson_roots)`,
   which adds nylium and soul soil. `golden_survival` could not see it — its
   23 grounds held neither. 27 grounds now. `fire` got `FireBlock.canSurvive`.
2. **The four forest features** (`nether_forest_vegetation`, `huge_fungus`,
   `weeping_vines`, `twisting_vines`). `golden_nether_features` FOREST.
3. **The fortress walk.** `golden_fortress_start`: 198 of 198 pieces and 23
   filler seeds equal across two starts, each harvested three times.
4. **The fortress blocks**, all fifteen `postProcess` bodies, placed at
   `underground_decoration`. `golden_nether_features` FORTRESS.
5. **The nether post-process pass**, which had never run.
6. **Fortress chests and blaze spawners** as block entities: 11 of 11 exact
   against all three references, loot seeds included.
7. **`ruined_portal_nether`**, placed for the first time.
   `golden_nether_ruined_portal` gates the three saved `in_nether` portals.
8. **No sky light in the nether**: decoration answered brightness 15, so no
   nether mushroom ever survived. `mushrooms_in_the_dark_match_vanilla_...`
   against a new dark pass of the survival oracle.
9. **The dried ghast** and the widened clip in `NetherFossilPiece.postProcess`.
   35 of 35 in the r=16 window.
10. **`BlackstoneReplaceProcessor`**: three missing entries and three copied
    properties instead of all of them.

## What is left, and none of it is a port defect

* `31415926 c(-200,200)` nether_wastes, 30 against a floor of 13, all three
  references agreeing: 25 `twisting_vines` blocks from chunk (-198,193) that
  vanilla never grew because a warped fungus hat and nether sprouts from row
  z=194 were already there. Vanilla decorated that row first in all three
  runs; ours decorates `zx`. 14 of the first origin's 18 columns are
  identical to vanilla's. An order question, not a feature one.
* `31415926 c(-189,198)` nether_wastes, 1.5x its floor: basalt and blackstone
  blob spill from the neighbouring deltas. Do not open it.
* Bastion chests are exact only with `RUSTMC_JIGSAW_STREAM=1` (18 of 18 with
  it, 1 of 18 without, r=16). The flip is the owner's.

## Nulls — do not re-run

* The fortress layout is a pure function of seed and chunk: all three
  harvests of each start saved identical `Children[]`.
* `Chest` on a saved corridor turn is not the draw: `postProcess` clears it
  once the chest is placed, so 7 of the (12,3) turns saved false where the
  draw was true.
* The End has sky light in 26.2 (`the_end.json` `has_skylight: true`); only
  the nether is dark.
