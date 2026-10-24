# Cold start — branch `passive-mobs`

### COLD START — branch `passive-mobs` off `origin/main` (`7df907ba5`), 2026-10-01

Worktree `~/grit/work-passive-mobs` (detached at hand-back; push with
`git push origin HEAD:refs/heads/passive-mobs`), remote dir `~/rustmc-passive`
(builds, recordings, T0), scratch compile dir `~/rustmc-passive-chk`.

**The base carries `animals` and one `shard-equivalence` commit.** The
interact path this lane needs (`handleInteract`, `Mob.interact`,
`Animal.mobInteract`) exists only on `origin/animals`, so its 11 commits are
replayed here onto main (one conflict, `ci/t2-gameplay-subset.sh`, resolved to
main's identical `--console` fix), plus `da4cab0be` (Silent spawn-egg
`entity_data`, the harness half included). Both land patch-equivalent; a
rebase after either reaches main drops them.

#### What was ported (offsets in each module's header)

    KINDS      cow.rs      Cow + MushroomCow (AbstractCow goals, sounds, milk, bowl,
                           shears -> convert.rs: Mob.convertTo SINGLE)
               sheep.rs    goals with EatBlockGoal, wool byte, SheepColorSpawnRules,
                           shear (shearing/sheep random_sequence), dye, ate, grass bite,
                           getMixedColor through the crafting recipes
               chicken.rs  goals, constructor egg-timer draw, aiStep tail (0.6 fall,
                           flap state, egg lay from gameplay/chicken_lay)
               egg.rs      EggItem.use, ThrownEgg flight/hit/break, hatching,
                           fudgePositionAfterSizeChange / findFreePosition
    TABLES     variants.rs + dump-mob-variants.sh: cow/chicken variants and sound
               variants, the farm-animal biome tags
    DATA       data.rs accessors 18/19 per kind, serializer ids 23/24/30/31;
               synched.rs Stack (accessor 8 ITEM_STACK) for dropped wool/mushrooms/eggs
    SEED       --level-seed (status::level_seed), passed by t2-gameplay*.sh

#### Checks `640`-`654`, `659`

All in `083`'s pit at |z| 480..1040 (perpendicular to the workspace line),
every mob hatched Silent. What each leaves unobserved and why is in its header:
mooshroom conversion's cow carries the spawn yaw (unseeded), and vanilla's
dropped items tick and merge (this server's do not), so shear checks observe
sound/add_entity and not set_entity_data/remove_entities.

VERDICTS: see the entry below once recorded.

#### Refused or deferred, by name

* Corpus grading of the chicken's slowed fall: `sendChanges`' 3-tick phase
  against the dig is wall-clock; unit-tested instead (`mob_in_world`).
* The egg timer and the sheep's grazing: unseeded per-tick rolls.
* A hatched chick's position near walls: findFreePosition is ported on the
  merged box grid; ties between equidistant boxes may resolve differently.
* `ThrownEgg.onHitEntity`'s zero damage, `onProjectileHit` block arms,
  projectile deflection, bubble columns; eggs tick after mobs.
* Brown mooshroom flower feeding (suspicious stew): no stack carries
  `SUSPICIOUS_STEW_INGREDIENT`.
* `/seed` and the login hash still answer `WORLD_SEED`.

### STATE at the PAUSE, 2026-10-01

Tip pushed; unit `mob::` tests and `mob_in_world` green in `~/rustmc-passive`
(1 passed, 1.82 s). Detached job `pm-rec3` (log `~/.rustmc-detached/pm-rec3.log`)
may still be running: it rebuilds, re-runs the tests, then records the subset
640-654+659 twice via `ci/t2-gameplay-subset.sh`, copying each run to
`~/rustmc-passive/oracle-work/gameplay/pm1` and `pm2` (logs `pm-subset-{1,2}.log`
beside them). Scripts are in the session scratchpad `mobs2/` (build.sh,
record.sh, splice.py); re-create from this entry if gone.

Next, in order:
1. Read the two subset verdicts; vanilla-vs-vanilla agreement across pm1/pm2 is
   the floor. Fix any divergence on our side (none recorded yet).
2. Splice: copy `~/rustmc-stack/oracle-work/gameplay/{vanilla,rustmc}.json` to a
   scratch dir, swap in pm1's 16 scenarios by name, ordered by corpus file name.
3. On the splice: `ci/corpus-falsifiable.py --sweep` (tier 2 for 640-659),
   `ci/corpus-grades.py`, `ci/mob-observability.py --ledger` (mob-blind rows),
   order-blind / entity-id-allocator ledgers as T0 names them; re-pin
   `ci/mob-tick-census.py`'s `our_entity_kinds` (6 -> 11).
4. T0 via `ci/run-detached.sh` in `~/rustmc-passive` (rsync, touch,
   gen-gameplay-tables twice, `T0_KEEP_GOING=1`).
5. Then rebase onto main (the `animals` replay and `da4cab0be` drop if landed).
