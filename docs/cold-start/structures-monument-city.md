# COLD START — branch `structures-monument-city`

Append-only. Newest entry last.

### COLD START — branch `structures-monument-city`, off `structures-mansion` (`a566dfb07`), 2026-10-01

Worktree `~/grit/work-smc` (detached), T0 dir `~/rustmc-sl2`, dev build
`~/rustmc-sl2x`, base build of `origin/main` 03fda6cd0 in `~/rustmc-smbase`.
Stacked on `structures-mansion`, whose tip this session also fixed (cargo fmt,
two clippy lints, and the cross-collision gate below).

**All seventeen overworld structure sets are generated and served.** New here:
ancient cities (step 7, `underground_decoration`) and ocean monuments (step 4).
`ci/structure-surface.py --dispatch` reads 20 of 20 sets, 34 of 34 structures.

## THE NUMBERS AND HOW THEY WERE MADE

Same method as `structures-live.md`: clamped twin vanilla harvests
(`-Dmax.bg.threads=1`, `GEN_ORDER=rows`) at `~/rustmc-sl2w/oracle-work/sl2-<name>-{a,b}`,
ours `worldgen-dump full <seed> <r> <out> <cx> <cz>` with `RUSTMC_DECORATE_ORDER=zx`,
`region-differ diff <vanilla-a> <ours> --bbox <harvest square> --positions`,
vanilla first, counted inside the vanilla start's own saved `Children` boxes;
floor = the same count for vanilla-a against vanilla-b; before = origin/main
03fda6cd0. Scripts `measure.sh`, `inbox2.py` in the session scratchpad.

| structure | seed, start chunk | in-box positions | before | after | floor |
|---|---|---|---|---|---|
| ancient city (89 pieces) | 12345 (32,37) | 758,532 | 253,606 | 13,408 | 5,329 |
| ocean monument, east | 42 (49,-11) | 77,372 | 14,927 | 0 | 0 |
| ocean monument, south | 8 (37,102) | 77,372 | 15,495 | 2 | 0 |
| ocean monument, west | 20260722 (14,-17) | 77,372 | 17,294 | 23 | 23 |
| ocean monument, north | 30 (15,-80) | 77,372 | 17,531 | 0 | 0 |

The city's 13,408: 4 do not touch sculk (glow lichen, which also differs in
open caves), 13,404 do; the floor's 5,329 all do. Outside the boxes the same
window reads 27,678 against a floor of 13,397 — the same ratio — so the excess
is the deep dark's own sculk decoration, not the city. The monument's 2 are an
unported `sea_pickle`; its 23 are seagrass, equal to the floor.

Layout: `worldgen-dump jigsaw-set-oracle city 12345 sl2-city1-a 4 32 38` reads
89/89. A monument saves only its building piece; facing and box of all four
starts are pinned by `ocean_monument::tests::the_saved_building_is_vanillas`.

## WHY THE ANCIENT CITY MATCHED ONLY 33 OF 89

`ancient_city/walls/no_corners` names `intact_horizontal_wall_stairs_5`, which
is not in the 26.2 jar. `StructureTemplateManager.getOrCreate` gives vanilla an
empty template; our loader refused the whole pool, so every wall it joins was
skipped. `Template::empty` fixed the walk outright.

## BUGS IN CODE THIS BRANCH DID NOT WRITE

* `golden_cross_collision_blocks` asserted vine, tripwire, glow lichen and sculk
  vein never rotate; all override `rotate` in the jar. Red on `structures-mansion`
  T1 since its vine fix. Fixed there (`a566dfb07`), both halves checked.
* `SculkPatchConfig` read `extra_rare_growths` as an int with `unwrap_or(0)`.
* `BlockRotProcessor`'s `rottable_blocks` was refused; redstone wire and every
  `ROTATION_16` block never turned with a template.

## NULLS

* The multiface rotation fix moved nothing in the city window (13,408 before and
  after it).
* The monument's first pass read ~940 wrong per start, all in the wings: the
  decompiler printed `++i` where the bytecode is `i++`. Read `iinc` against the
  load order before trusting a decompiled increment.

## WHAT IS STILL OPEN

1. **The deep dark's sculk decoration is 2x its floor** in the seed 12345
   (32,38) window, inside and outside the city. Closing measurement: the
   sculk-touching residual of `m-city1` at or under its floor.
2. Not spawned: monument elder guardians, mansion illagers and allays.
3. Village `feature_pool_element`s still wait on the `RUSTMC_JIGSAW_STREAM`
   decision; the ancient city runs its own on a forced `Faithful` stream.

### PAUSED 2026-10-01 (usage pacing) — exact next steps

Tips: `structures-mansion` a566dfb07, `structures-monument-city` e2cb3c77e (stacked).
Detached jobs left running (`~/.rustmc-detached/<job>.log`, sentinel `DETACHED-DONE`):

* `sm-gate2` — T0 on 7f748bbc4 (superseded tip), result `<scratchpad>/sm/gate-sm2.out`.
* `sm-gate3` — T0 + T1 on the mansion tip a566dfb07: `<scratchpad>/sm/gate-sm3.out`,
  logs `~/rustmc-sl2-logs/t0-sm3.log`, `t1-sm3.log`.
* `sm-gate4` — T0 + T1 on this tip e2cb3c77e (runs after sm-gate3, same dir
  `~/rustmc-sl2`): `<scratchpad>/sm/gate-sm4.out`, `~/rustmc-sl2-logs/t0-sm4.log`, `t1-sm4.log`.
* `sm-regress` — every structures-live/mansion window + the End on the
  monument-city binary (`~/rustmc-sl2x`): `<scratchpad>/sm/regress.out`.
  Compare each `in-box: after=` with the structures-live/mansion tables; End
  must read `mismatches=0` against both references.

`<scratchpad>` = `/tmp/claude-1000/-/befd9b5b-5163-468a-9d93-210a7ac9ac95/scratchpad`.

On resume: read those four outputs. If T0/T1 red, fix on the owning branch
(mansion first, then rebase monument-city onto it), re-gate with
`WT=<worktree> bash <scratchpad>/sm/gate.sh <tag>` via `ci/run-detached.sh`.
If regress moved any window, bisect between a566dfb07 and e2cb3c77e. Then the
open item: the deep dark's sculk at 2x floor (seed 12345, window (32,38) r=4).
