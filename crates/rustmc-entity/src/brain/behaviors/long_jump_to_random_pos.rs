//! `net.minecraft.world.entity.ai.behavior.LongJumpToRandomPos`, from the
//! 26.2 jar. `Behavior<Mob>` with entry `{LOOK_TARGET: REGISTERED,
//! LONG_JUMP_COOLDOWN_TICKS: VALUE_ABSENT, LONG_JUMP_MID_JUMP:
//! VALUE_ABSENT}` and a fixed duration of **200** (`TIME_OUT_DURATION`,
//! `sipush 200` at ctor 22).
//!
//! ```text
//! FIND_JUMP_TRIES = 20; PREPARE_JUMP_DURATION = 40
//! MIN_PATHFIND_DISTANCE_TO_VALID_JUMP = 8; ALLOWED_ANGLES = [65, 70, 75, 80]
//!
//! checkExtraStartConditions (0..75):
//!   ok = mob.onGround() && !mob.isInWater() && !mob.isInLava()
//!          && !level.getBlockState(mob.blockPosition()).is(Blocks.HONEY_BLOCK)
//!   if !ok: brain.setMemory(LONG_JUMP_COOLDOWN_TICKS, timeBetweenLongJumps.sample(level.random) / 2)
//!   return ok
//! canStillUse (0..129):
//!   ok = initialPosition.isPresent() && initialPosition.get().equals(mob.position())
//!          && findJumpTries > 0 && !mob.isInWater()
//!          && (chosenJump != null || !jumpCandidates.isEmpty())
//!   if !ok && brain.getMemory(LONG_JUMP_MID_JUMP).isEmpty():
//!       brain.setMemory(LONG_JUMP_COOLDOWN_TICKS, timeBetweenLongJumps.sample(level.random) / 2)
//!       brain.eraseMemory(LOOK_TARGET)
//!   return ok
//! start (0..138):
//!   chosenJump = null; findJumpTries = 20; initialPosition = Optional.of(mob.position())
//!   p = mob.blockPosition()
//!   jumpCandidates = BlockPos.betweenClosedStream(p.x-w, p.y-h, p.z-w, p.x+w, p.y+h, p.z+w)
//!                      .filter(q -> !q.equals(p))
//!                      .map(q -> new PossibleJump(q.immutable(), Mth.ceil(p.distSqr(q))))
//!                      .collect(toCollection(ArrayList::new))
//! tick (0..126):
//!   if chosenJump == null: findJumpTries--; pickCandidate(level, mob, time); return
//!   if time - prepareJumpStart < 40: return
//!   mob.setYRot(mob.yBodyRot); mob.setDiscardFriction(true)
//!   d5 = chosenJump.length(); d7 = d5 + mob.getJumpBoostPower()
//!   mob.setDeltaMovement(chosenJump.scale(d7 / d5))
//!   brain.setMemory(LONG_JUMP_MID_JUMP, true)
//!   level.playSound(null, mob, getJumpSound.apply(mob), NEUTRAL, 1.0f, 1.0f)
//! pickCandidate (0..150):  loop while !jumpCandidates.isEmpty():
//!   candidate = getJumpCandidate(level); if empty: continue          <- REMOVES nothing, so this
//!                                                                       only ends when the list empties
//!   target = candidate.targetPos()
//!   if !isAcceptableLandingPosition(level, mob, target): continue
//!   vec = calculateOptimalJumpVector(mob, Vec3.atCenterOf(target)); if null: continue
//!   brain.setMemory(LOOK_TARGET, new BlockPosTracker(target))
//!   path = mob.getNavigation().createPath(target, 0, 8)
//!   if path == null || !path.canReach(): chosenJump = vec; prepareJumpStart = time; return
//!   (else) continue
//! getJumpCandidate (0..36):
//!   o = WeightedRandom.getRandomItem(level.random, jumpCandidates, PossibleJump::weight)
//!   o.ifPresent(jumpCandidates::remove)                              <- List.remove(Object), first equal
//!   return o
//! isAcceptableLandingPosition (0..51):
//!   if mob.blockPosition().x == target.x && .z == target.z: false     (a pure-vertical hop is refused)
//!   return acceptableLandingSpot.test(mob, target)
//! defaultAcceptableLandingSpot (0..40):
//!   level.getBlockState(target.below()).isSolidRender()
//!     && mob.getPathfindingMalus(WalkNodeEvaluator.getPathTypeStatic(mob, target)) == 0.0f
//! calculateOptimalJumpVector (0..93):
//!   angles = Lists.newArrayList(ALLOWED_ANGLES); Collections.shuffle(angles)
//!   f = (float)(mob.getAttributeValue(JUMP_STRENGTH) * maxJumpVelocityMultiplier)
//!   for a in angles: v = LongJumpUtil.calculateJumpVectorForAngle(mob, target, f, a, true)
//!                    if present: return v
//!   return null
//! ```
//!
//! **The path test is inverted from what it reads like.** A candidate is
//! **accepted** when the navigation *cannot* reach it (`path == null ||
//! !path.canReach()`, 122..146) — the whole point is to jump only where
//! walking will not do. A reachable candidate falls through to the next
//! loop iteration with `LOOK_TARGET` already written, so a rejected
//! candidate still leaves its look target behind.
//!
//! **`pickCandidate` is a `while (!jumpCandidates.isEmpty())`, and every
//! `continue` re-enters it.** `getJumpCandidate` is what shrinks the list,
//! by `List.remove(Object)` on the drawn element; the other three
//! `continue`s (empty draw, unacceptable landing, no jump vector) do not
//! remove anything, and the loop terminates because the draw removed the
//! element it returned.
//!
//! **Unverifiable by construction:** `Collections.shuffle(angles)` in
//! `calculateOptimalJumpVector` uses an unseeded `java.util.Random`, so
//! the order the four `ALLOWED_ANGLES` are tried in is not reproducible.
//! A caller-seeded `LegacyRandomSource` stands in, exactly as
//! `InsideBrownianWalk`'s shuffle does, and the port takes the mob random
//! for it so at least a seed-replayed twin agrees with itself.
//!
//! Both cooldown writes take `timeBetweenLongJumps.sample(level.random)`
//! and then **integer-divide by 2**, so an odd sample is rounded towards
//! zero. `UniformInt.sample` is `Mth.randomBetweenInclusive(random, min,
//! max)` = `nextInt(max - min + 1) + min`, one draw on the **level**
//! random.

use rustmc_data::BlockPos;

use crate::ai::goals::try_find_water_goal::between_closed;
use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{Behavior, BehaviorBody};
use crate::brain::behaviors::long_jump_util::{calculate_jump_vector_for_angle, mth_ceil};
use crate::brain::behaviors::set_entity_look_target_sometimes::UniformInt;
use crate::brain::memory::{MemoryModuleType, MemoryStatus, MemoryValue, PositionTracker};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel, SoundSpec};
use crate::phys::Vec3;
use rustmc_worldgen::rng::RandomSource;

/// `FIND_JUMP_TRIES`.
pub const FIND_JUMP_TRIES: i32 = 20;
/// `PREPARE_JUMP_DURATION`.
pub const PREPARE_JUMP_DURATION: i64 = 40;
/// `MIN_PATHFIND_DISTANCE_TO_VALID_JUMP`.
pub const MIN_PATHFIND_DISTANCE_TO_VALID_JUMP: i32 = 8;
/// `TIME_OUT_DURATION`, the behaviour's fixed duration.
pub const TIME_OUT_DURATION: i32 = 200;
/// `ALLOWED_ANGLES`, in the static initialiser's array order.
pub const ALLOWED_ANGLES: [i32; 4] = [65, 70, 75, 80];

/// `LongJumpToRandomPos$PossibleJump`: a landing block and its weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PossibleJump {
    /// `targetPos()`.
    pub target_pos: BlockPos,
    /// `weight()` — `Mth.ceil(start.distSqr(target))`.
    pub weight: i32,
}

/// Which `acceptableLandingSpot` `BiPredicate` the constructor installed.
/// The six-argument constructor takes an arbitrary one, and callers that
/// pass their own (the frog does) name it here; the predicate itself is a
/// host call, because those callers' predicates read the mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LandingSpot {
    /// `LongJumpToRandomPos::defaultAcceptableLandingSpot`, installed by
    /// the five-argument constructor.
    Default,
    /// A caller-supplied `BiPredicate<E, BlockPos>`, by name.
    Host(&'static str),
}

/// `LongJumpToPreferredBlock`'s extra state. The subclass overrides
/// `start` and `getJumpCandidate` only, so it is this struct's optional
/// half rather than a type of its own.
#[derive(Debug, Clone)]
pub struct PreferredBlock {
    /// `preferredBlockTag`, by tag name (`minecraft:` implied).
    pub tag: &'static str,
    /// `preferredBlocksChance`.
    pub chance: f32,
    /// `notPrefferedJumpCandidates` — vanilla's spelling.
    pub not_preferred: Vec<PossibleJump>,
    /// `currentlyWantingPreferredOnes`.
    pub currently_wanting_preferred_ones: bool,
}

/// `LongJumpToRandomPos`'s body.
#[derive(Debug, Clone)]
pub struct LongJumpToRandomPos {
    /// `timeBetweenLongJumps`.
    pub time_between_long_jumps: UniformInt,
    /// `maxLongJumpHeight`.
    pub max_long_jump_height: i32,
    /// `maxLongJumpWidth`.
    pub max_long_jump_width: i32,
    /// `maxJumpVelocityMultiplier`.
    pub max_jump_velocity_multiplier: f32,
    /// `getJumpSound.apply(mob)`, constant per mob in every caller.
    pub jump_sound: SoundSpec,
    /// `acceptableLandingSpot`.
    pub acceptable_landing_spot: LandingSpot,
    /// `LongJumpToPreferredBlock`'s extra state, when this is one.
    pub preferred: Option<PreferredBlock>,
    /// `jumpCandidates`.
    pub jump_candidates: Vec<PossibleJump>,
    /// `initialPosition`.
    pub initial_position: Option<Vec3>,
    /// `chosenJump`.
    pub chosen_jump: Option<Vec3>,
    /// `findJumpTries`.
    pub find_jump_tries: i32,
    /// `prepareJumpStart`.
    pub prepare_jump_start: i64,
}

impl LongJumpToRandomPos {
    /// `LongJumpToRandomPos(UniformInt, int, int, float, Function)` — the
    /// five-argument constructor, which installs
    /// `defaultAcceptableLandingSpot`.
    #[must_use]
    pub fn new(
        time_between_long_jumps: UniformInt,
        max_long_jump_height: i32,
        max_long_jump_width: i32,
        max_jump_velocity_multiplier: f32,
        jump_sound: SoundSpec,
    ) -> Behavior<LongJumpToRandomPos> {
        Self::with_landing_spot(
            time_between_long_jumps,
            max_long_jump_height,
            max_long_jump_width,
            max_jump_velocity_multiplier,
            jump_sound,
            LandingSpot::Default,
        )
    }

    /// The six-argument constructor.
    #[must_use]
    pub fn with_landing_spot(
        time_between_long_jumps: UniformInt,
        max_long_jump_height: i32,
        max_long_jump_width: i32,
        max_jump_velocity_multiplier: f32,
        jump_sound: SoundSpec,
        acceptable_landing_spot: LandingSpot,
    ) -> Behavior<LongJumpToRandomPos> {
        Behavior::with_duration(
            LongJumpToRandomPos {
                time_between_long_jumps,
                max_long_jump_height,
                max_long_jump_width,
                max_jump_velocity_multiplier,
                jump_sound,
                acceptable_landing_spot,
                preferred: None,
                jump_candidates: Vec::new(),
                initial_position: None,
                chosen_jump: None,
                find_jump_tries: 0,
                prepare_jump_start: 0,
            },
            vec![
                (MemoryModuleType::LookTarget, MemoryStatus::Registered),
                (
                    MemoryModuleType::LongJumpCooldownTicks,
                    MemoryStatus::ValueAbsent,
                ),
                (MemoryModuleType::LongJumpMidJump, MemoryStatus::ValueAbsent),
            ],
            TIME_OUT_DURATION,
        )
    }

    /// `LongJumpToPreferredBlock(UniformInt, int, int, float, Function,
    /// TagKey, float, BiPredicate)` — the subclass, which passes the
    /// caller's landing predicate straight through to `super`.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn to_preferred_block(
        time_between_long_jumps: UniformInt,
        max_long_jump_height: i32,
        max_long_jump_width: i32,
        max_jump_velocity_multiplier: f32,
        jump_sound: SoundSpec,
        preferred_block_tag: &'static str,
        preferred_blocks_chance: f32,
        acceptable_landing_spot: LandingSpot,
    ) -> Behavior<LongJumpToRandomPos> {
        let mut b = Self::with_landing_spot(
            time_between_long_jumps,
            max_long_jump_height,
            max_long_jump_width,
            max_jump_velocity_multiplier,
            jump_sound,
            acceptable_landing_spot,
        );
        b.body.preferred = Some(PreferredBlock {
            tag: preferred_block_tag,
            chance: preferred_blocks_chance,
            not_preferred: Vec::new(),
            currently_wanting_preferred_ones: false,
        });
        b
    }

    /// The cooldown write both condition methods share:
    /// `timeBetweenLongJumps.sample(level.random) / 2`, an integer divide.
    fn set_cooldown<M: MobHost>(&self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let n = self.time_between_long_jumps.sample_level(&mut cx.mob) / 2;
        brain.set_memory_value(MemoryModuleType::LongJumpCooldownTicks, MemoryValue::Int(n));
    }

    /// `defaultAcceptableLandingSpot(mob, pos)` and the preferred-block
    /// override, whichever the constructor installed.
    fn acceptable_landing_spot<M: MobHost>(&self, cx: &MobAi<M>, target: BlockPos) -> bool {
        let below = BlockPos {
            x: target.x,
            y: target.y - 1,
            z: target.z,
        };
        match self.acceptable_landing_spot {
            LandingSpot::Default => {
                cx.mob.level().block_facts(below).is_solid_render
                    && cx.mob.pathfinding_malus(cx.mob.path_type_static(target)) == 0.0
            }
            LandingSpot::Host(name) => cx.mob.acceptable_landing_spot(name, target),
        }
    }

    /// `isAcceptableLandingPosition(level, mob, target)`.
    fn is_acceptable_landing_position<M: MobHost>(&self, cx: &MobAi<M>, target: BlockPos) -> bool {
        let p = cx.mob.block_position();
        if p.x == target.x && p.z == target.z {
            return false;
        }
        self.acceptable_landing_spot(cx, target)
    }

    /// `getJumpCandidate(level)`:
    /// `WeightedRandom.getRandomItem(level.random, jumpCandidates,
    /// PossibleJump::weight)`, then `jumpCandidates.remove(drawn)`.
    fn get_jump_candidate<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> Option<PossibleJump> {
        // `LongJumpToPreferredBlock.getJumpCandidate` (0..130): when not
        // currently wanting preferred ones it is exactly `super`; when it
        // is, it drains the whole weighted list, returning the first draw
        // whose block below carries `preferredBlockTag` and parking every
        // other draw in `notPrefferedJumpCandidates`, then falls back to
        // that parked list **in order, from the front** — so the fallback
        // is the draw order, not a fresh weighted draw.
        if let Some(pref) = self.preferred.as_ref() {
            if pref.currently_wanting_preferred_ones {
                let tag = pref.tag;
                while !self.jump_candidates.is_empty() {
                    let Some(c) = self.weighted_draw(cx) else {
                        continue;
                    };
                    let below = BlockPos {
                        x: c.target_pos.x,
                        y: c.target_pos.y - 1,
                        z: c.target_pos.z,
                    };
                    if cx.mob.block_tag_is(below, tag) {
                        return Some(c);
                    }
                    if let Some(p) = self.preferred.as_mut() {
                        p.not_preferred.push(c);
                    }
                }
                let p = self.preferred.as_mut().expect("checked above");
                if p.not_preferred.is_empty() {
                    return None;
                }
                return Some(p.not_preferred.remove(0));
            }
        }
        self.weighted_draw(cx)
    }

    /// `LongJumpToRandomPos.getJumpCandidate(level)` itself.
    fn weighted_draw<M: MobHost>(&mut self, cx: &mut MobAi<M>) -> Option<PossibleJump> {
        let total: i64 = self
            .jump_candidates
            .iter()
            .map(|c| i64::from(c.weight))
            .sum();
        assert!(
            total <= i64::from(i32::MAX),
            "Sum of weights must be <= {}",
            i32::MAX
        );
        #[allow(clippy::cast_possible_truncation)]
        let total = total as i32;
        assert!(total >= 0, "Negative total weight in getRandomItem");
        if total == 0 {
            return None;
        }
        let mut n = cx.mob.level_random_next_int(total);
        let mut found = None;
        for c in &self.jump_candidates {
            n -= c.weight;
            if n < 0 {
                found = Some(*c);
                break;
            }
        }
        if let Some(c) = found {
            // `List.remove(Object)`: the first equal element.
            if let Some(i) = self.jump_candidates.iter().position(|x| *x == c) {
                self.jump_candidates.remove(i);
            }
        }
        found
    }

    /// `calculateOptimalJumpVector(mob, target)`. The shuffle is unseeded
    /// in vanilla; see this file's header.
    fn calculate_optimal_jump_vector<M: MobHost>(
        &self,
        cx: &mut MobAi<M>,
        target: Vec3,
    ) -> Option<Vec3> {
        let mut angles = ALLOWED_ANGLES;
        // `Collections.shuffle(list)` — Fisher-Yates from the end down,
        // which is the algorithm `java.util.Collections.shuffle` uses.
        for i in (1..angles.len()).rev() {
            #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
            let j = cx.mob.random().next_i32_bound(i as i32 + 1) as usize;
            angles.swap(i, j);
        }
        #[allow(clippy::cast_possible_truncation)]
        let f = (cx.mob.jump_strength() * f64::from(self.max_jump_velocity_multiplier)) as f32;
        for a in angles {
            if let Some(v) = calculate_jump_vector_for_angle(cx, target, f, a, true) {
                return Some(v);
            }
        }
        None
    }

    /// `pickCandidate(level, mob, time)`.
    fn pick_candidate<M: MobHost>(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        while !self.jump_candidates.is_empty() {
            let Some(candidate) = self.get_jump_candidate(cx) else {
                continue;
            };
            let target = candidate.target_pos;
            if !self.is_acceptable_landing_position(cx, target) {
                continue;
            }
            let Some(vec) = self.calculate_optimal_jump_vector(cx, Vec3::at_center_of(target))
            else {
                continue;
            };
            brain.set_memory_value(
                MemoryModuleType::LookTarget,
                MemoryValue::PositionTracker(PositionTracker::block(target)),
            );
            let path = cx.navigation.create_path_within(
                &mut cx.mob,
                target,
                0,
                MIN_PATHFIND_DISTANCE_TO_VALID_JUMP,
            );
            let reachable = path.is_some_and(|p| p.can_reach());
            if !reachable {
                self.chosen_jump = Some(vec);
                self.prepare_jump_start = time;
                return;
            }
        }
    }
}

impl<M: MobHost> BehaviorBody<M> for LongJumpToRandomPos {
    fn check_extra_start_conditions(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) -> bool {
        let ok = cx.mob.on_ground()
            && !cx.mob.is_in_water()
            && !cx.mob.is_in_lava()
            && !cx
                .mob
                .level()
                .block_facts(cx.mob.block_position())
                .is_honey_block;
        if !ok {
            self.set_cooldown(brain, cx);
        }
        ok
    }

    fn can_still_use(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let ok = self.initial_position == Some(cx.mob.position())
            && self.find_jump_tries > 0
            && !cx.mob.is_in_water()
            && (self.chosen_jump.is_some() || !self.jump_candidates.is_empty());
        if !ok && !brain.has_memory_value(MemoryModuleType::LongJumpMidJump) {
            self.set_cooldown(brain, cx);
            brain.erase_memory(MemoryModuleType::LookTarget);
        }
        ok
    }

    fn start(&mut self, _brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) {
        self.chosen_jump = None;
        self.find_jump_tries = FIND_JUMP_TRIES;
        self.initial_position = Some(cx.mob.position());
        let p = cx.mob.block_position();
        let (w, h) = (self.max_long_jump_width, self.max_long_jump_height);
        if let Some(p) = self.preferred.as_mut() {
            p.not_preferred.clear();
            p.currently_wanting_preferred_ones = cx.mob.random().next_f32() < p.chance;
        }
        self.jump_candidates = between_closed(p.x - w, p.y - h, p.z - w, p.x + w, p.y + h, p.z + w)
            .into_iter()
            .filter(|q| *q != p)
            .map(|q| PossibleJump {
                target_pos: q,
                weight: mth_ceil(dist_sqr(p, q)),
            })
            .collect();
    }

    fn tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, time: i64) {
        let Some(chosen) = self.chosen_jump else {
            self.find_jump_tries -= 1;
            self.pick_candidate(brain, cx, time);
            return;
        };
        if time - self.prepare_jump_start < PREPARE_JUMP_DURATION {
            return;
        }
        let y_body = cx.mob.y_body_rot();
        cx.mob.set_y_rot(y_body);
        cx.mob.set_discard_friction(true);
        let d5 = chosen.length();
        let d7 = d5 + f64::from(cx.mob.jump_boost_power());
        let v = chosen.scale(d7 / d5);
        cx.mob.set_delta_movement(v.x, v.y, v.z);
        brain.set_memory_value(MemoryModuleType::LongJumpMidJump, MemoryValue::Bool(true));
        let sound = self.jump_sound;
        cx.mob.play_sound(&sound, 1.0, 1.0);
    }

    fn name(&self) -> &'static str {
        "LongJumpToRandomPos"
    }
}

/// `Vec3i.distSqr(Vec3i)`: the squared distance between two block
/// positions, in `double`.
#[must_use]
pub fn dist_sqr(a: BlockPos, b: BlockPos) -> f64 {
    let dx = f64::from(a.x - b.x);
    let dy = f64::from(a.y - b.y);
    let dz = f64::from(a.z - b.z);
    dx * dx + dy * dy + dz * dz
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::testing::{GridLevel, TestMob};

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.floor(-16, -16, 16, 16, 0);
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[T::LookTarget, T::LongJumpCooldownTicks, T::LongJumpMidJump],
            vec![],
        )
        .state;
        (cx, brain)
    }

    fn behavior() -> Behavior<LongJumpToRandomPos> {
        LongJumpToRandomPos::new(
            UniformInt::of(60, 100),
            2,
            4,
            1.5,
            SoundSpec("frog.long_jump"),
        )
    }

    #[test]
    fn a_failed_start_check_writes_half_a_sampled_cooldown_on_the_level_random() {
        let (mut cx, mut brain) = setup();
        cx.mob.on_ground = false;
        let mut b = behavior();
        let mut twin = TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1);
        let expected = UniformInt::of(60, 100).sample_level(&mut twin) / 2;
        assert!(!b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(
            brain.get_memory(T::LongJumpCooldownTicks),
            Some(&MemoryValue::Int(expected))
        );
        assert!(
            (30..=50).contains(&expected),
            "an integer halving of 60..100"
        );
    }

    #[test]
    fn start_builds_the_candidate_box_minus_the_mob_block_and_weights_by_ceil_of_dist_sqr() {
        let (mut cx, mut brain) = setup();
        cx.mob.on_ground = true;
        brain.erase_memory(T::LongJumpCooldownTicks);
        let mut b = behavior();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        // The box is (2w+1)^2 * (2h+1) minus the mob's own block.
        assert_eq!(b.body.jump_candidates.len(), 9 * 9 * 5 - 1);
        assert_eq!(b.body.find_jump_tries, FIND_JUMP_TRIES);
        assert_eq!(b.body.initial_position, Some(cx.mob.position()));
        assert!(b.body.chosen_jump.is_none());
        let p = cx.mob.block_position();
        assert!(
            !b.body.jump_candidates.iter().any(|c| c.target_pos == p),
            "the mob's own block is filtered out"
        );
        let far = b
            .body
            .jump_candidates
            .iter()
            .find(|c| {
                c.target_pos
                    == BlockPos {
                        x: p.x + 4,
                        y: p.y + 2,
                        z: p.z + 4,
                    }
            })
            .expect("a corner");
        assert_eq!(far.weight, mth_ceil(16.0 + 4.0 + 16.0));
    }

    #[test]
    fn the_preferred_block_draw_parks_non_preferred_candidates_and_falls_back_in_draw_order() {
        let (mut cx, mut brain) = setup();
        cx.mob.on_ground = true;
        let mut b = LongJumpToRandomPos::to_preferred_block(
            UniformInt::of(60, 100),
            1,
            1,
            1.5,
            SoundSpec("frog.long_jump"),
            "frog_prefer_jump_to",
            1.0,
            LandingSpot::Default,
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            b.body
                .preferred
                .as_ref()
                .unwrap()
                .currently_wanting_preferred_ones,
            "chance 1.0: nextFloat() < 1.0 for every draw"
        );
        // Nothing carries the tag, so every draw is parked and the
        // fallback returns them from the FRONT of the parked list, in the
        // order they were drawn — not by weight.
        let n = b.body.jump_candidates.len();
        let first = b.body.get_jump_candidate(&mut cx).expect("the parked head");
        assert!(b.body.jump_candidates.is_empty(), "the whole list drained");
        assert_eq!(
            b.body.preferred.as_ref().unwrap().not_preferred.len(),
            n - 1
        );
        let second = b.body.get_jump_candidate(&mut cx).expect("still parked");
        assert_ne!(first, second);
        // With a tagged block below one candidate, that one is returned
        // directly and the list is not drained.
        let (mut cx, mut brain) = setup();
        cx.mob.on_ground = true;
        let mut b = LongJumpToRandomPos::to_preferred_block(
            UniformInt::of(60, 100),
            1,
            1,
            1.5,
            SoundSpec("frog.long_jump"),
            "frog_prefer_jump_to",
            1.0,
            LandingSpot::Default,
        );
        assert!(b.try_start(&mut brain, &mut cx, 0));
        for c in &b.body.jump_candidates.clone() {
            cx.mob.block_tags.push((
                BlockPos {
                    x: c.target_pos.x,
                    y: c.target_pos.y - 1,
                    z: c.target_pos.z,
                },
                "frog_prefer_jump_to",
            ));
        }
        let drawn = b.body.get_jump_candidate(&mut cx).expect("a tagged one");
        assert!(b.body.jump_candidates.iter().all(|c| *c != drawn));
        assert_eq!(b.body.jump_candidates.len(), 26 - 1);
        assert!(b.body.preferred.as_ref().unwrap().not_preferred.is_empty());
    }

    #[test]
    fn can_still_use_writes_the_cooldown_only_when_not_mid_jump() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        b.body.initial_position = Some(cx.mob.position());
        b.body.find_jump_tries = 0; // fails the tries clause
        brain.set_memory_value(
            T::LookTarget,
            MemoryValue::PositionTracker(PositionTracker::block(BlockPos { x: 1, y: 1, z: 1 })),
        );
        brain.set_memory_value(T::LongJumpMidJump, MemoryValue::Bool(true));
        assert!(!BehaviorBody::<TestMob>::can_still_use(
            &mut b.body,
            &mut brain,
            &mut cx,
            0
        ));
        assert!(
            !brain.has_memory_value(T::LongJumpCooldownTicks),
            "mid-jump: no cooldown, no look-target erase"
        );
        assert!(brain.has_memory_value(T::LookTarget));
        brain.erase_memory(T::LongJumpMidJump);
        assert!(!BehaviorBody::<TestMob>::can_still_use(
            &mut b.body,
            &mut brain,
            &mut cx,
            0
        ));
        assert!(brain.has_memory_value(T::LongJumpCooldownTicks));
        assert!(!brain.has_memory_value(T::LookTarget));
    }

    #[test]
    fn the_prepare_delay_is_forty_ticks_and_then_the_jump_scales_by_the_boost() {
        let (mut cx, mut brain) = setup();
        let mut b = behavior();
        b.body.chosen_jump = Some(Vec3 {
            x: 0.6,
            y: 0.8,
            z: 0.0,
        });
        b.body.prepare_jump_start = 10;
        cx.mob.y_body_rot = 42.0;
        cx.mob.jump_boost_power = 0.0;
        BehaviorBody::<TestMob>::tick(&mut b.body, &mut brain, &mut cx, 49);
        assert!(
            !brain.has_memory_value(T::LongJumpMidJump),
            "39 ticks is short of PREPARE_JUMP_DURATION"
        );
        BehaviorBody::<TestMob>::tick(&mut b.body, &mut brain, &mut cx, 50);
        assert_eq!(
            brain.get_memory(T::LongJumpMidJump),
            Some(&MemoryValue::Bool(true))
        );
        assert!(cx.mob.discard_friction);
        assert!((cx.mob.y_rot - 42.0).abs() < 1e-6);
        // length 1.0, boost 0: the vector is unchanged.
        assert!((cx.mob.delta_movement.x - 0.6).abs() < 1e-12);
        assert!((cx.mob.delta_movement.y - 0.8).abs() < 1e-12);
    }
}
