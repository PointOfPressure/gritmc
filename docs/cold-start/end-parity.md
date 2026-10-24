# Cold start — branch `end-parity`

---

### COLD START — branch `end-parity` off `nether-parity` (`247ddecdb`), 2026-10-04

**Handoff file: kept current at every state change, so a session in any tool
can pick it up cold.** Worktree `~/grit/work-nether-parity` (the nether
worktree, now on this branch), remote dir `~/rustmc-end-parity` on the
Threadripper (`threadripper` is an ssh alias to localhost). Push after every
commit: `git push origin HEAD:refs/heads/end-parity`. No attribution lines.

## How to measure

`ci/nether-parity-windows.py --only end-r7-20260722-c0_0 ...` (names starting
`end-` dump with `full-end` and compare y 0..255) diffs our dump against every
stamped reference and every reference pair, per biome. Binaries:
`cargo build --release -p worldgen-dump -p region-differ` in the remote dir.

## Baseline at `247ddecdb` (ours median vs vanilla pairwise median, blocks)

```text
  window                       biome              ours   floor   runs
  20260722 c(0,0) r=7          the_end               0       0   3/3
  20260722 c(83,62) r=7        all four outer        0       0   3/3
  20260722 c(64,64) r=16       end_highlands        23      46   6/15   chorus order
                               others                0       0   6/15
```

So the measured End is exact. The work is what no window reaches:

* `end_gateway_return` — the configured feature is unported and no
  reference holds one (`rarity_filter 700` in `end_highlands`).
* the dragon fight's spawn structures — the references hold the 10 spikes
  with 146 `iron_bars` and 10 `fire`, and NO exit portal: the harvest server
  has no player, so `EnderDragonFight` never builds the podium. The end
  crystals are entities and the harvest does not save `entities/`.
* only seed 20260722 has End references.

## State (newest last)

1. Windows script extended to the End; baseline taken (above). `d97c99e68`.
2. Exit portal: `feature::end::podium_writes` ports `EndPodiumFeature`,
   gated by `golden_end_podium` against `EndPodiumGolden.java` (the feature
   called on a recording level; `ci/end-podium-golden.sh`). NOT wired into a
   running server: `EnderDragonFight` (needs a player in the End) is
   unported server work. `b1936a3ef`.
3. Return gateway (`EndGatewayFeature` + block entity) ported and gated:
   our own scan of 16 r=16 windows found two at seed 20260722, chunks
   (-99,-5) and (116,109); harvested x3 each (`van-end-20260722-c{-99_-5,
   116_109}-gw-{a,b,c}` in `~/rustmc/oracle-work`). First port had the
   cross/corner rule inverted (34 of 45 blocks wrong); fixed. End crystals
   now recorded as entities (`EndWorld::add_fresh_entity`), gated against
   `entities/` saved by the harvest at three seeds
   (`van-end-{12345,31415926,20260722}-c0_0-ent-{a,b,c}`). Test:
   `tools/worldgen-assembly/tests/golden_end_features.rs`.

## Measured after step 3 (ours median vs vanilla pairwise median, blocks)

```text
  window                         biome           ours   floor  runs
  12345 c(0,0) r=7               the_end            0       0   3/3
  31415926 c(0,0) r=7            the_end            0       0   3/3
  20260722 c(-99,-5) r=7         end_highlands      0     130   3/3   gateway
  20260722 c(116,109) r=7        end_highlands      0       0   3/3   gateway
  gateway block entity (bediff)                   1/1 exact    each of 6 refs
  end crystals, key for key but UUID              10/10        3 seeds x 3 refs
```

4. End cities with a ship: (81,3) and (107,-37), harvested x3 each
   (`van-end-20260722-c{81_3,107_-37}-ship-{a,b,c}`). One stair wrong: the
   End never ran the post-process pass and end city pieces were placed
   `knownShape` (vanilla's `EndCityPiece.makeSettings` never sets it). Both
   fixed; `the_ship_cities_match_vanillas_consensus` holds every position the
   three references agree on, and every block entity (9 and 129).

5. Ship-city entities: 12 and 59 (shulkers, the elytra frame) equal to
   every reference key for key but `UUID` and a shulker's `Rotation`, which
   vanilla draws from the entity's own unseeded random (`LivingEntity.<init>`
   `random.nextFloat() * 6.2831855f`): vanilla a vs b disagree on all 60
   shulker yaws at (107,-37). Ours writes 0.

6. Outer End at seeds 12345 c(-16,83) and 31415926 c(25,87), around ship
   cities, harvested x3 each. Wall banners: `WallBannerBlock.updateShape`
   had no rule (drops a banner whose support is not `isSolid`), and the end
   city piece world could not answer `isSolid`/`isFaceSturdy`. Both added.
   12345: 4 spurious banners -> 0. 31415926: 4 -> 2, and the 2 left are
   ORDER: their support stair is written by chunk (25,86)'s pass, which ours
   (zx) runs before chunk (26,86)'s banner; all three vanilla runs dropped
   the banners, so vanilla decorated (26,86) first. Not a port defect.
   Also the c(107,-37) ladder: ours now matches 2 of 3 references.

7. Exit portal LOCATION: `feature::end::exit_portal_origin` ports
   `EnderDragonFight.spawnExitPortal` (heightmap MOTION_BLOCKING_NO_LEAVES
   at (0,0) minus one, step down through bedrock while y > 63). References
   need a player in the End (the fight only scans with one), so
   `ci/harvest-end-podium.sh <seed> <tag>` boots vanilla, joins
   `oracle-runner`, tps it to the End, waits for "Found that the dragon has
   not yet been killed", saves. Six harvests: `van-end-{20260722,12345}-
   podium-{a,b,c}`. Test `the_exit_portal_stands_where_the_dragon_fight_
   puts_it`: origin (0,64,0) at both seeds, all 1,048,576 consensus
   positions of x,z in -32..32, y 0..256 equal, 4 torches. The bedrock
   step-down is vacuous on a fresh End (only bites on a dragon respawn).

8. End city PLACEMENT over three r=24 squares (2,401 chunks each), one
   harvest per seed (`van-end-12345-c130_-70-r24-a`,
   `van-end-31415926-c130_-70-r24-a`, `van-end-20260722-c95_-20-r24-a`,
   `GEN_ORDER=rows ci/gen-vanilla-region.sh <seed> 24 <dir> <port> <cx> <cz>
   end`): 5 starts, 406 piece bounding boxes equal in saved order. Test
   `the_end_city_starts_match_vanilla_over_three_r24_squares`. Note:
   `worldgen-dump end-city-starts` also prints starts up to 12 chunks
   OUTSIDE the square (its reach); filter by start chunk before comparing.
   Ship-city windows re-measured at this tip, ours median vs floor median:
   20260722 c(81,3) 0/0, c(107,-37) 0/3, 12345 c(-16,83) 181/181,
   31415926 c(25,87) 15/13 (the 2 banners of step 6). 3 runs each.

9. Return-gateway RING (`EnderDragonFight.spawnNewGateway`), ported as
   `gateway_spawn_order(seed)` (jar's `Util.shuffle` on
   `createThreadLocalInstance(worldSeed)`, popped from the end) and
   `gateway_ring_position(i)`; `gateway_writes` split out of
   `place_gateway`. Gated against `data/vanilla/end_gateway_ring.json`
   (`ci/end-gateway-ring-golden.sh`, `GatewayRingGolden.java`): 20
   positions, 8 seeds. Live: `KILL_DRAGON=1 ci/harvest-end-podium.sh` kills
   the dragon and also saves `data/`; at 20260722 run a the saved
   `ender_dragon_fight.dat` `gateways` list equals our order minus the
   first, the gateway stands at (77,75,-57), egg at (0,68,0).

10. Outer islands, r=24 (2,401 chunks, 157,351,936 blocks each), 3 rows
    harvests per seed, `ci/nether-parity-windows.py --only end-r24-...`,
    ours min/median/max vs floor min/median/max (blocks):
    12345 c(130,-70)     2/87/215   vs  89/217/302
    31415926 c(130,-70) 14/215/451  vs 229/240/465
    20260722 c(95,-20) 182/251/558  vs 307/433/740
    Every divergence is in end_highlands/midlands (chorus decoration
    order) and ours sits at or below vanilla's own run-to-run spread;
    barrens and small islands 0 in every run.
    `van-end-*-c-3125_6250-far-*` (x -50,000, z 100,000) were first
    harvested square (no dump_order stamp, refused); re-harvested as rows
    by `ep-far2`.

11. First entry into a return gateway (`TheEndGatewayBlockEntity
    .getPortalPosition` -> `findOrCreateValidTeleportPos`) ported as
    `feature::end::gateway_exit` / `find_tallest_block`. Vanilla quirk:
    `findValidSpawnInChunk` keeps the `betweenClosed` MutableBlockPos, so
    it returns (maxX, top, maxZ) of the box, not the nearest candidate
    (proved with log4j DEBUG on that class:
    `VANILLA_JVM_OPTS=-Dlog4j2.configurationFile=$PWD/ci/log4j-gateway-debug.xml`). References:
    `KILL_DRAGON=1 GATEWAY="x y z" ci/harvest-end-podium.sh`,
    `van-end-{20260722,12345}-exit-{a,b,c}`. Test
    `the_first_gateway_exit_matches_vanilla`: exits (848,75,-638) and
    (-929,48,-305), both gateways' blocks and block entities, 3 worlds each.
    The island fallback (`END_ISLAND` with `RandomSource.create()`) is
    unseeded in vanilla and not checked.
12. Far out, x -50,000 z 100,000 (`end-r7-*-c-3125_6250`, 3 rows runs):
    20260722 ours 70 = floor 70; 12345 ours 950/950/950 vs floor 0/0/0 in
    end_highlands 139 + end_midlands 811: a real divergence, FIXED in
    step 13.
13. That divergence was an end city ship missing: `generate_end` read the
    city's corner heights off the canvas, so a start whose corners lay
    outside the generated margin (chunk (-3136,6243), 4 chunks west of the
    window) was skipped. Vanilla asks the generator
    (`ChunkGenerator.getFirstOccupiedHeight`, a noise column), as
    `end_city_report` already did; `generate_end` now calls `base_height`
    too. 12345 far window 950 -> 0 (floor 0, 3 runs); 20260722 far 70 = 70,
    and the ship windows unchanged (0/0/3, 181, 15). Gated as the fourth
    window of `the_ship_cities_match_vanillas_consensus`.

T0 STATE: `ep-t0b`/`ep-t0c` (87de96d357) red only on the main-side tick
flake. `ep-t0d` (cccbdf2230) red on 6 cheap stages, fixed in the next
commit: 2 stale `place.rs::block_age_with` citations (`--fix`), the ring
table needed a quoted literal (`GATEWAY_RING_TABLE`), vacuous-green census
re-derived (539 scripts), 2 readiness proofs of `ci/harvest-end-podium.sh`
registered in `ci/checks/liveness-proofs.toml`. The 6th, check-use-probe
`FOREIGN` tables, is the T0 dir: its data/vanilla was copied from
rustmc-nether-parity; `./ci/gen-gameplay-tables.sh` in the remote dir fixes
it. Run T0 as `RUSTMC_REMOTE_DIR=rustmc-end-parity ./ci/t0-remote.sh --sync`
then `T0_KEEP_GOING=1 ./ci/t0-remote.sh`, full output to a file.

GOTCHA (in FINDINGS): after a by-hand sabotage, `rsync -a` from the worktree
puts source mtimes back below the sabotaged artifact and cargo reuses it.
`find crates tools -name '*.rs' -print0 | xargs -0 touch` after every rsync.

## Next

T0 GREEN on `4a75c408ce` (`~/grit/logs/end-parity-t0e.log`); queued.
The brief's measurement and port work is done. What is left is outside it:

* Server: no `EnderDragonFight` in `rustmc-server` (no fight state, no
  dragon entity, no trigger on a player entering the End). Everything it
  builds is ported and gated as pure functions in `feature::end`
  (`exit_portal_origin`, `podium_writes`, `gateway_spawn_order`,
  `gateway_ring_position`, `gateway_writes`, `gateway_exit`); the hook
  belongs to the rustmc-server surface lane (`crates/rustmc-server/src/
  world.rs` `generate_in` is where End chunks are served).
* The r=16 `end_highlands` chorus difference at 20260722 c(64,64) (ours 23
  vs floor 46, 6 runs) is decoration order, inside vanilla's own spread.
