# Cold start — branch `mobs-brains-4`

### COLD START — branch `mobs-brains-4` off `origin/mobs-brains-3` (`71a127aee`), 2026-09-05

**Twenty-five of the twenty-six behaviour classes the fifth lane left
pending are transcribed: `tests/brain_tables.rs::PENDING` went from **26 to
1**, out of the same 131 classes the twenty providers name.
`PENDING_SENSORS` stays 0.** Worktree `~/grit/work-brains3`, remote dir
`~/rustmc-brains4` (inner-loop compile dir `~/rustmc-brains4-dev`). Not
merged — the orchestrator gates. 15 commits over `mobs-brains-3`.
`cargo test -p rustmc-entity`: **452 unit tests + 22 integration tests
green**, `cargo clippy -p rustmc-entity --all-targets -- -D warnings`
clean, `cargo fmt` applied; T0 chained through `ci/run-detached.sh`.

    SNIFFER (9)   SnifferAi$Digging + $FinishedDigging (dig activity),
                  $Sniffing + $Scenting + $FeelingHappy (idle states),
                  $Searching (sniff activity), and the three anonymous
                  $1/$2/$3 over AnimalPanic / AnimalMakeLove / FollowTemptation
    BREEZE (5)    ShootWhenStuck, Slide + BreezeAi$SlideToTargetSink, Shoot,
                  LongJump; plus `breeze_util` (hasLineOfSight,
                  randomPointBehindTarget, getMaxLineOfSightTestRange)
    ARMADILLO (3) ArmadilloAi$1, $ArmadilloPanic, $ArmadilloBallUp
    CAMEL (2)     CamelAi$CamelPanic, $RandomSitting
    CREAKING (1)  CreakingAi$1
    GOAT (2)      PrepareRamNearestTarget (+ RamCandidate), RamTarget
    LOOSE (3)     ChargeAttack, GoAndGiveItemsToTarget, animal/frog/ShootTongue
    INFRA         `BehaviorBody::stop_timed` (a `stop` that can read the base's
                  `endTimestamp`, which `this.timedOut(time)` needs);
                  `Behavior::map_body` (how an anonymous subclass is expressed —
                  the superclass constructor's entry condition and durations are
                  kept, not rebuilt); `host::SnifferState`, `host::ArmadilloState`;
                  `mth::sign`, `mth::lerp_f32`; `util_shuffle`
                  (`net.minecraft.util.Util.shuffle`); ~30 new `MobHost` calls
                  with base defaults

**Checked against the jar:** every class carries its offsets and draw order
in its doc comment and seed-replayed tests on `testing::TestMob` /
`GridLevel`. `ci/brain-tables.sh` is unchanged and still passes (this branch
adds no table).

**Four findings worth the lane, all read from bytecode rather than
reasoned about, all in `docs/FINDINGS.md`:**

1. **An absent `DANGER_DETECTED_RECENTLY` reads as MAXIMAL danger.**
   `ArmadilloBallUp.tick` computes `danger = getTimeUntilExpiry(...) > 75L`;
   `Brain.getTimeUntilExpiry` is `getMemorySlot(type).timeToLive()` and
   `MemorySlot.clear()` (0..12) puts `timeToLive` back to
   `NEVER_EXPIRE = Long.MAX_VALUE`. So an expired or never-set memory
   answers `Long.MAX_VALUE`, `danger` is true, and the armadillo stays
   balled up. The obvious reading is the opposite one.
2. **`RamTarget.tick`'s `closerThan(ramTarget, 0.25)` disjunct can never
   fire.** `WalkTarget(Vec3, float, int)` is
   `BlockPosTracker(BlockPos.containing(v))` — the block constructor, y =
   blockY + 0.5 — and every `RAM_TARGET` is
   `getEdgeOfBlock` = `atBottomCenterOf(to).add(x, 0, z)`, y = blockY. The
   gap is exactly 0.5 on y alone. `BlockPosTracker(Vec3)` *does* keep its
   argument verbatim, but `WalkTarget` never calls that one, which is why
   this needed both classes' bytecode.
3. **`ChargeAttack`'s constructor assigns its two doubles in the opposite
   order to their field declarations** (slot 5-6 to `maxChargeDistance`,
   slot 7-8 to `maxTargetDetectionDistance`). Both are used squared, so a
   transcription from the field list swaps them silently.
4. **`breeze.LongJump`'s angle shuffle IS seeded** —
   `Util.shuffledCopy(ALLOWED_ANGLES, breeze.getRandom())`, four `nextInt`
   draws for five angles — unlike `LongJumpToRandomPos`'s unseeded
   `Collections.shuffle`, which the fifth lane recorded as unverifiable by
   construction. That class is therefore deterministic from a seed.

**Deferred goldens, each named in the class rather than approximated:** no
class here has a jar golden, for the reason every earlier lane gave — each
needs a `LivingEntity` in a `ServerLevel`, and `mob-in-world` is the host
that will change it. Three tests carry a **second independent transcription
of the same algorithm** as their twin (`BreezeUtil.randomPointBehindTarget`'s
draw order, `Util.shuffle`, `UniformInt.sample` over the level random);
each says in the test that it is a cross-check and not a jar golden.

**Unverifiable by construction and said so in the code:**
* `Shoot.tick`'s `Projectile.spawnProjectileUsingShoot` applies its
  inaccuracy with three `nextGaussian()` draws from the **projectile's**
  random, so the draw count is not observable from this crate;
  `MobHost::breeze_shoot_wind_charge` takes the five arguments and the host
  takes the draws. The arguments are transcribed and tested.
* `LongJump.snapToSurface` needs `Level.clip`'s exact hit **location**, and
  `PathLevel::clip_misses` answers only whether a ray is clear. It is
  `MobHost::snap_to_surface` carrying the bytecode rather than a
  transcription — writing a `GridLevel` double for it would have produced a
  test scoring the double, which is this project's named defect class.

**Nulls, so nobody re-runs them:** `SnifferAi$FinishedDigging`'s
`checkExtraStartConditions` is a bare `iconst_1`; `ArmadilloBallUp`'s
`TICKS_DELAY_TO_DETERMINE_IF_DANGER_IS_STILL_AROUND = 5` is declared and
read by nothing, and its `tick` opens with a `super.tick` that is empty;
`getEdgeOfBlock`'s first local (`0.5d` at offset 0) is dead;
`BreezeUtil.randomPointBehindTarget`'s `MAX_ANGLE` local is dead; none of
`SnifferAi$1/$2/$3` overrides `stop`; `ChargeAttack.tick` calls `stop`
directly so `stop` runs twice per hit, both statements idempotent, nothing
observable; `ShootTongue.tick` re-sets the tongue target every tick
including in `DONE`.

**Needs hands: nothing in this branch.** What is left in `PENDING` is
**one class, `TransportItemsBetweenContainers`**, and it is a lane of its
own rather than a leftover: 39.5 KB of class file, 25 methods, four inner
types (`TransportItemTarget`, `TransportItemState`,
`ContainerInteractionState`, `OnTargetReachedInteraction`), a
`Map<ContainerInteractionState, OnTargetReachedInteraction>` of callbacks,
eleven constants, and a host surface this crate does not have yet —
`BlockEntity` scans over a box, `Container` reads and writes, container
locking, `GlobalPos` visited/unreachable sets in memories, and
`getPositionToReachTargetFrom(Path, mob)` which needs a real path's end
node. Budget it as an area, not as a class. After it, the `*Ai` provider
lambdas themselves (`AxolotlAi`, `FrogAi`, `WardenAi` with its
`AngerManagement`, `SnifferAi`, `BreezeAi`, `GoatAi`, `ArmadilloAi`,
`CamelAi`) as data in the tables the same way villager's are — every
behaviour those providers construct now exists.
