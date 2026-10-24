//! `net.minecraft.world.entity.ai.behavior.PrepareRamNearestTarget` and its
//! inner `RamCandidate`, from the 26.2 jar.
//!
//! ```text
//! PrepareRamNearestTarget$RamCandidate(BlockPos startPosition,
//!                                     BlockPos targetPosition,
//!                                     LivingEntity target)
//!    three final fields and three getters, nothing else.
//!
//! PrepareRamNearestTarget(ToIntFunction<E> getCooldownOnFail, int minRamDistance,
//!                         int maxRamDistance, float walkSpeed,
//!                         TargetingConditions ramTargeting, int ramPrepareTime,
//!                         Function<E,SoundEvent> getPrepareRamSound)   (0..87)
//!   TIME_OUT_DURATION = 160
//!   entry ImmutableMap.of(LOOK_TARGET:REGISTERED, RAM_COOLDOWN_TICKS:VALUE_ABSENT,
//!         NEAREST_VISIBLE_LIVING_ENTITIES:VALUE_PRESENT, RAM_TARGET:VALUE_ABSENT)
//!   Behavior(map, 160) -> (160, 160)
//!   reachedRamPositionTimestamp = empty;  ramCandidate = empty
//!
//!   start (0..35):
//!        brain.getMemory(NEAREST_VISIBLE_LIVING_ENTITIES)
//!             .flatMap(nv -> nv.findClosest(e -> ramTargeting.test(level, mob, e)))
//!             .ifPresent(target -> chooseRamPosition(mob, target))
//!   canStillUse (0..34):
//!        ramCandidate.isPresent() && ramCandidate.get().getTarget().isAlive()
//!   stop (0..45):
//!        if !brain.hasMemoryValue(RAM_TARGET):
//!           level.broadcastEntityEvent(mob, (byte) 59)
//!           brain.setMemory(RAM_COOLDOWN_TICKS, getCooldownOnFail.applyAsInt(mob))
//!   tick (0..301):
//!        0  if ramCandidate.isEmpty(): return
//!       11  brain.setMemory(WALK_TARGET,
//!               new WalkTarget(ramCandidate.get().getStartPosition(), walkSpeed, 0))
//!       46  brain.setMemory(LOOK_TARGET,
//!               new EntityTracker(ramCandidate.get().getTarget(), true))
//!       77  boolean moved = !ramCandidate.get().getTarget().blockPosition()
//!                              .equals(ramCandidate.get().getTargetPosition())
//!      119  if moved:
//!      124     level.broadcastEntityEvent(mob, (byte) 59)
//!      131     mob.getNavigation().stop()
//!      138     chooseRamPosition(mob, ramCandidate.get().target)
//!      159  else:
//!      159     BlockPos here = mob.blockPosition()
//!      165     if here.equals(ramCandidate.get().getStartPosition()):
//!      186        level.broadcastEntityEvent(mob, (byte) 58)
//!      193        if reachedRamPositionTimestamp.isEmpty():
//!                    reachedRamPositionTimestamp = Optional.of(time)
//!      214        if time - reachedRamPositionTimestamp.get() >= ramPrepareTime:
//!      238           brain.setMemory(RAM_TARGET,
//!                       getEdgeOfBlock(here, ramCandidate.get().getTargetPosition()))
//!      267           level.playSound(null, mob, getPrepareRamSound.apply(mob),
//!                       NEUTRAL, 1.0f, mob.getVoicePitch())
//!      294           ramCandidate = Optional.empty()
//!
//!   getEdgeOfBlock(from, to)  (0..56):
//!        0  double unused = 0.5d                       // stored, never read
//!        4  double x = 0.5d * Mth.sign(to.getX() - from.getX())
//!       24  double z = 0.5d * Mth.sign(to.getZ() - from.getZ())
//!       44  return Vec3.atBottomCenterOf(to).add(x, 0.0, z)
//!
//!   calculateRammingStartPosition(mob, target)  (0..194):
//!        0  BlockPos t = target.blockPosition()
//!        5  if !isWalkableBlock(mob, t): return Optional.empty()
//!       18  List<BlockPos> list = Lists.newArrayList()
//!       23  MutableBlockPos m = t.mutable()
//!       29  for Direction d : Direction.Plane.HORIZONTAL:      // NORTH, EAST, SOUTH, WEST
//!       59     m.set(t)
//!       66     for (int i = 0; i < maxRamDistance; i++):
//!       78        if !isWalkableBlock(mob, m.move(d)): { m.move(d.getOpposite()); break }
//!      113     if m.distManhattan(t) >= minRamDistance: list.add(m.immutable())
//!      142  PathNavigation nav = mob.getNavigation()
//!      148  return list.stream()
//!               .sorted(Comparator.comparingDouble(mob.blockPosition()::distSqr))
//!               .filter(p -> { Path path = nav.createPath(p, 0);
//!                              return path != null && path.canReach(); })
//!               .findFirst()
//!
//!   isWalkableBlock(mob, pos)  (0..30):
//!        mob.getNavigation().isStableDestination(pos)
//!        && mob.getPathfindingMalus(WalkNodeEvaluator.getPathTypeStatic(mob, pos)) == 0.0f
//!
//!   chooseRamPosition(mob, target)  (0..26):
//!        reachedRamPositionTimestamp = Optional.empty()
//!        ramCandidate = calculateRammingStartPosition(mob, target)
//!                          .map(pos -> new RamCandidate(pos, target.blockPosition(), target))
//! ```
//!
//! **The inner search advances BEFORE it tests.** `m.move(d)` at offset 84
//! mutates the position and the result of that move is what `isWalkableBlock`
//! is handed; on a failure the loop moves back one with
//! `d.getOpposite()` and breaks. So the candidate for a direction is the
//! last walkable block reached, and a direction blocked immediately yields
//! the target's own position — which then fails the `>= minRamDistance`
//! test and contributes nothing.
//!
//! **`Direction.Plane.HORIZONTAL` iterates NORTH, EAST, SOUTH, WEST**, read
//! from `Direction$Plane`'s `<clinit>` array rather than from the enum's
//! declaration order, and `Stream.sorted` is stable, so two candidates at
//! the same squared distance from the mob keep that order. That is the whole
//! tie-break.
//!
//! **The path filter runs AFTER the sort, one candidate at a time.**
//! `findFirst` short-circuits, so the number of `createPath` calls is the
//! index of the first reachable candidate plus one, not the list length.
//!
//! **`getEdgeOfBlock`'s first local is dead.** `0.5d` is stored to slot 3 at
//! offset 0 and never read; the two `0.5d` values actually used are separate
//! `ldc2_w`s at offsets 4 and 24. The result is the *bottom centre* of the
//! target block pushed half a block toward the ram's start, so the goat aims
//! at the near edge rather than the middle.
//!
//! **`stop` pays a cooldown only on FAILURE.** The guard is
//! `!brain.hasMemoryValue(RAM_TARGET)`, so a preparation that succeeded —
//! which is exactly the one that wrote `RAM_TARGET` — pays nothing, and
//! `RamTarget.finishRam` sets the real cooldown later. A preparation that
//! times out at 160 ticks pays `getCooldownOnFail`.
//!
//! **`reachedRamPositionTimestamp` is cleared by `chooseRamPosition`, not by
//! `start`**, so a target that keeps moving resets the prepare timer every
//! time it changes block, and the goat can stand on its start position
//! indefinitely without ever charging.
//!
//! **UNVERIFIABLE FROM THIS CRATE — nothing.** Every branch is expressible
//! against `TestMob`/`GridLevel`; what needs a real `ServerLevel` is only
//! the *golden*, as for every class in this crate.

use crate::ai::direction::Direction;
use crate::ai::mob_ai::MobAi;
use crate::ai::targeting::TargetingConditions;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::nearest_visible_find_closest;
use crate::brain::memory::{
    MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker, WalkTarget,
};
use crate::brain::BrainState;
use crate::host::{MobHost, SoundSpec};
use crate::mth;
use crate::phys::Vec3;
use rustmc_data::BlockPos;

/// `ToIntFunction<E>` over the mob, as `getCooldownOnFail` takes it.
pub type MobIntFn<M> = Box<dyn FnMut(&mut MobAi<M>) -> i32>;

/// `PrepareRamNearestTarget.TIME_OUT_DURATION`.
pub const TIME_OUT_DURATION: i32 = 160;
/// The `broadcastEntityEvent` id a restart or a failure sends.
pub const RAM_RESET_EVENT: u8 = 59;
/// The `broadcastEntityEvent` id standing on the start position sends.
pub const RAM_READY_EVENT: u8 = 58;
/// `Direction.Plane.HORIZONTAL`, in the `<clinit>` array's order.
pub const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// `PrepareRamNearestTarget$RamCandidate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamCandidate {
    /// `startPosition`.
    pub start_position: BlockPos,
    /// `targetPosition`.
    pub target_position: BlockPos,
    /// `target`, by id.
    pub target: i32,
}

/// `getEdgeOfBlock(BlockPos from, BlockPos to)` (0..56).
#[must_use]
pub fn get_edge_of_block(from: BlockPos, to: BlockPos) -> Vec3 {
    let x = 0.5 * f64::from(mth::sign(f64::from(to.x - from.x)));
    let z = 0.5 * f64::from(mth::sign(f64::from(to.z - from.z)));
    Vec3::at_bottom_center_of(to).add3(x, 0.0, z)
}

/// `PrepareRamNearestTarget`'s body.
pub struct PrepareRamNearestTarget<M: MobHost> {
    /// `getCooldownOnFail`. The jar takes a `ToIntFunction<E>`; every call
    /// site hands it a lambda over the mob, so this is that lambda.
    pub cooldown_on_fail: MobIntFn<M>,
    /// `minRamDistance`.
    pub min_ram_distance: i32,
    /// `maxRamDistance`.
    pub max_ram_distance: i32,
    /// `walkSpeed`.
    pub walk_speed: f32,
    /// `ramTargeting`.
    pub ram_targeting: TargetingConditions<M>,
    /// `ramPrepareTime`.
    pub ram_prepare_time: i32,
    /// `getPrepareRamSound`, a `Function<E, SoundEvent>` whose value is
    /// constant per mob at every call site.
    pub prepare_ram_sound: SoundSpec,
    /// `reachedRamPositionTimestamp`.
    pub reached_ram_position_timestamp: Option<i64>,
    /// `ramCandidate`.
    pub ram_candidate: Option<RamCandidate>,
}

impl<M: MobHost + 'static> PrepareRamNearestTarget<M> {
    /// `new PrepareRamNearestTarget(...)`.
    #[must_use]
    pub fn new(
        cooldown_on_fail: MobIntFn<M>,
        min_ram_distance: i32,
        max_ram_distance: i32,
        walk_speed: f32,
        ram_targeting: TargetingConditions<M>,
        ram_prepare_time: i32,
        prepare_ram_sound: SoundSpec,
    ) -> Behavior<PrepareRamNearestTarget<M>> {
        Behavior::with_duration(
            PrepareRamNearestTarget {
                cooldown_on_fail,
                min_ram_distance,
                max_ram_distance,
                walk_speed,
                ram_targeting,
                ram_prepare_time,
                prepare_ram_sound,
                reached_ram_position_timestamp: None,
                ram_candidate: None,
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::RamCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
                (
                    MemoryModuleType::NearestVisibleLivingEntities,
                    MemoryStatus::ValuePresent,
                ),
                (MemoryModuleType::RamTarget, MemoryStatus::ValueAbsent),
            ],
            TIME_OUT_DURATION,
        )
    }

    /// `isWalkableBlock(E, BlockPos)` (0..30).
    fn is_walkable_block(cx: &mut MobAi<M>, pos: BlockPos) -> bool {
        if !cx.navigation.is_stable_destination(&cx.mob, pos) {
            return false;
        }
        let t = cx.mob.path_type_static(pos);
        cx.mob.pathfinding_malus(t) == 0.0
    }

    /// `calculateRammingStartPosition(E, LivingEntity)` (0..194).
    fn calculate_ramming_start_position(
        &mut self,
        cx: &mut MobAi<M>,
        target: i32,
    ) -> Option<BlockPos> {
        let t = cx.mob.entity(target)?.block_position();
        if !Self::is_walkable_block(cx, t) {
            return None;
        }
        let mut list: Vec<BlockPos> = Vec::new();
        for d in HORIZONTAL {
            let mut m = t;
            for _ in 0..self.max_ram_distance {
                m = BlockPos {
                    x: m.x + d.step_x(),
                    y: m.y,
                    z: m.z + d.step_z(),
                };
                if !Self::is_walkable_block(cx, m) {
                    let o = d.opposite();
                    m = BlockPos {
                        x: m.x + o.step_x(),
                        y: m.y,
                        z: m.z + o.step_z(),
                    };
                    break;
                }
            }
            let dist = (m.x - t.x).abs() + (m.y - t.y).abs() + (m.z - t.z).abs();
            if dist >= self.min_ram_distance {
                list.push(m);
            }
        }
        let here = cx.mob.block_position();
        // `Comparator.comparingDouble(here::distSqr)`, and `Stream.sorted`
        // is stable, so equal distances keep the HORIZONTAL order.
        let dist_sqr = |p: &BlockPos| {
            let (dx, dy, dz) = (
                f64::from(p.x - here.x),
                f64::from(p.y - here.y),
                f64::from(p.z - here.z),
            );
            dx * dx + dy * dy + dz * dz
        };
        list.sort_by(|a, b| {
            dist_sqr(a)
                .partial_cmp(&dist_sqr(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        // `findFirst` short-circuits, so the path search stops at the first
        // reachable candidate.
        for p in list {
            let path = cx.navigation.create_path(&mut cx.mob, p, 0);
            if path.is_some_and(|path| path.can_reach()) {
                return Some(p);
            }
        }
        None
    }

    /// `chooseRamPosition(E, LivingEntity)` (0..26).
    fn choose_ram_position(&mut self, cx: &mut MobAi<M>, target: i32) {
        self.reached_ram_position_timestamp = None;
        let target_position = cx.mob.entity(target).map(|e| e.block_position());
        self.ram_candidate = self
            .calculate_ramming_start_position(cx, target)
            .zip(target_position)
            .map(|(start_position, target_position)| RamCandidate {
                start_position,
                target_position,
                target,
            });
    }
}

impl<M: MobHost + 'static> BehaviorBody<M> for PrepareRamNearestTarget<M> {
    fn start(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        let conditions =
            std::mem::replace(&mut self.ram_targeting, TargetingConditions::for_combat());
        let found = nearest_visible_find_closest(brain, cx, |cx, id| {
            cx.mob
                .entity(id)
                .is_some_and(|facts| conditions.test(cx, &facts))
        });
        self.ram_targeting = conditions;
        if let Some(target) = found {
            self.choose_ram_position(cx, target);
        }
    }

    fn can_still_use(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        self.ram_candidate
            .and_then(|c| cx.mob.entity(c.target))
            .is_some_and(|e| e.alive)
    }

    fn stop(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        if !brain.has_memory_value(MemoryModuleType::RamTarget) {
            cx.mob.broadcast_entity_event(RAM_RESET_EVENT);
            let n = (self.cooldown_on_fail)(cx);
            brain.set_memory_value(MemoryModuleType::RamCooldownTicks, MemoryValue::Int(n));
        }
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let Some(candidate) = self.ram_candidate else {
            return;
        };
        brain.set_memory_value(
            MemoryModuleType::WalkTarget,
            MemoryValue::WalkTarget(WalkTarget::at_block(
                candidate.start_position,
                self.walk_speed,
                0,
            )),
        );
        brain.set_memory_value(
            MemoryModuleType::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::entity(candidate.target, true)),
        );
        let target_now = cx.mob.entity(candidate.target).map(|e| e.block_position());
        let moved = target_now != Some(candidate.target_position);
        if moved {
            cx.mob.broadcast_entity_event(RAM_RESET_EVENT);
            cx.navigation.stop();
            self.choose_ram_position(cx, candidate.target);
            return;
        }
        let here = cx.mob.block_position();
        if here != candidate.start_position {
            return;
        }
        cx.mob.broadcast_entity_event(RAM_READY_EVENT);
        if self.reached_ram_position_timestamp.is_none() {
            self.reached_ram_position_timestamp = Some(time);
        }
        let since = time - self.reached_ram_position_timestamp.unwrap_or(time);
        if since >= i64::from(self.ram_prepare_time) {
            brain.set_memory_value(
                MemoryModuleType::RamTarget,
                MemoryValue::Vec3(get_edge_of_block(here, candidate.target_position)),
            );
            let sound = self.prepare_ram_sound;
            let pitch = cx.mob.voice_pitch();
            cx.mob.level_play_sound(&sound, "neutral", 1.0, pitch);
            self.ram_candidate = None;
        }
    }

    fn name(&self) -> &'static str {
        "PrepareRamNearestTarget"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{BlockFacts, EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    const PREPARE: SoundSpec = SoundSpec("entity.goat.prepare_ram");

    fn behavior() -> Behavior<PrepareRamNearestTarget<TestMob>> {
        PrepareRamNearestTarget::new(
            Box::new(|_| 100),
            4,
            7,
            1.25,
            TargetingConditions::for_combat(),
            20,
            PREPARE,
        )
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.fill(-12, 0, -12, 12, 0, 12, BlockFacts::STONE);
        let mut mob = TestMob::new(level, 0.5, 1.0, 0.5, 23);
        mob.on_ground = true;
        mob.others
            .push((EntityClass::Living, EntityFacts::living(9, 6.5, 1.0, 0.5)));
        let cx = MobAi::new(mob);
        let brain = Brain::<TestMob>::new(
            &[
                T::LookTarget,
                T::RamCooldownTicks,
                T::NearestVisibleLivingEntities,
                T::RamTarget,
                T::WalkTarget,
            ],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn the_edge_of_block_is_the_bottom_centre_pushed_half_a_block() {
        let from = BlockPos { x: 0, y: 1, z: 0 };
        let to = BlockPos { x: 6, y: 1, z: 0 };
        // sign(+6) is 1, sign(0) is 0.
        assert_eq!(get_edge_of_block(from, to), Vec3::new(7.0, 1.0, 0.5));
        // The other way round the push is negative: bottom centre of
        // `from` is (0.5, 1.0, 0.5) and the push is -0.5 on x.
        assert_eq!(get_edge_of_block(to, from), Vec3::new(0.0, 1.0, 0.5));
        // Equal coordinates get no push at all.
        assert_eq!(
            get_edge_of_block(from, from),
            Vec3::new(0.5, 1.0, 0.5),
            "Mth.sign(0.0) is 0"
        );
    }

    #[test]
    fn the_horizontal_plane_order_is_the_jars_clinit_array() {
        assert_eq!(
            HORIZONTAL,
            [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West
            ]
        );
    }

    #[test]
    fn tick_writes_a_walk_target_and_an_eye_height_entity_look_target() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        b.body.ram_candidate = Some(RamCandidate {
            start_position: BlockPos { x: 11, y: 1, z: 0 },
            target_position: BlockPos { x: 6, y: 1, z: 0 },
            target: 9,
        });
        b.body.tick(&mut brain, &mut cx, 10);
        let MemoryValue::WalkTarget(w) = brain.get_memory(T::WalkTarget).unwrap() else {
            panic!("walk target")
        };
        assert_eq!(w.speed_modifier, 1.25);
        assert_eq!(
            brain.get_memory(T::LookTarget),
            Some(&MemoryValue::PositionTracker(PositionTracker::entity(
                9, true
            ))),
            "new EntityTracker(target, true) — the eye-height flag is set"
        );
    }

    #[test]
    fn a_target_that_moved_restarts_the_search_and_stops_the_navigation() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        b.body.ram_candidate = Some(RamCandidate {
            start_position: BlockPos { x: 11, y: 1, z: 0 },
            // The target is really at block (6, 1, 0), so this is stale.
            target_position: BlockPos { x: 5, y: 1, z: 0 },
            target: 9,
        });
        b.body.reached_ram_position_timestamp = Some(1);
        b.body.tick(&mut brain, &mut cx, 10);
        assert!(cx.mob.calls.iter().any(|c| c == "broadcastEntityEvent(59)"));
        assert_eq!(
            b.body.reached_ram_position_timestamp, None,
            "chooseRamPosition clears the prepare timer"
        );
    }

    #[test]
    fn standing_on_the_start_position_arms_the_timer_and_then_the_ram_target() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        let start = cx.mob.block_position();
        b.body.ram_candidate = Some(RamCandidate {
            start_position: start,
            target_position: BlockPos { x: 6, y: 1, z: 0 },
            target: 9,
        });
        b.body.tick(&mut brain, &mut cx, 100);
        assert_eq!(b.body.reached_ram_position_timestamp, Some(100));
        assert!(!brain.has_memory_value(T::RamTarget), "0 < ramPrepareTime");
        assert!(cx.mob.calls.iter().any(|c| c == "broadcastEntityEvent(58)"));

        // 19 ticks later is still short; 20 is the threshold, `>=`.
        b.body.tick(&mut brain, &mut cx, 119);
        assert!(!brain.has_memory_value(T::RamTarget));
        b.body.tick(&mut brain, &mut cx, 120);
        assert_eq!(
            brain.get_memory(T::RamTarget),
            Some(&MemoryValue::Vec3(get_edge_of_block(
                start,
                BlockPos { x: 6, y: 1, z: 0 }
            )))
        );
        assert!(b.body.ram_candidate.is_none(), "cleared on success");
        assert!(cx
            .mob
            .calls
            .iter()
            .any(|c| c == "level_play_sound entity.goat.prepare_ram neutral 1 1"));
    }

    #[test]
    fn stop_pays_the_cooldown_only_when_no_ram_target_was_written() {
        // Failure: no RAM_TARGET.
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
        assert_eq!(
            brain.get_memory(T::RamCooldownTicks),
            Some(&MemoryValue::Int(100))
        );
        assert!(cx.mob.calls.iter().any(|c| c == "broadcastEntityEvent(59)"));

        // Success: RAM_TARGET present, nothing paid.
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::RamTarget, MemoryValue::Vec3(Vec3::new(1.0, 1.0, 1.0)));
        let mut b = behavior();
        BehaviorControl::do_stop(&mut b, &mut brain, &mut cx, 1);
        assert!(!brain.has_memory_value(T::RamCooldownTicks));
        assert!(cx.mob.calls.is_empty());
    }

    #[test]
    fn can_still_use_needs_a_live_candidate_target() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        assert!(
            !b.body.can_still_use(&mut brain, &mut cx, 1),
            "no candidate"
        );
        b.body.ram_candidate = Some(RamCandidate {
            start_position: BlockPos { x: 11, y: 1, z: 0 },
            target_position: BlockPos { x: 6, y: 1, z: 0 },
            target: 9,
        });
        assert!(b.body.can_still_use(&mut brain, &mut cx, 1));
        cx.mob.others[0].1.alive = false;
        assert!(!b.body.can_still_use(&mut brain, &mut cx, 1));
    }
}
