//! `net.minecraft.world.entity.ai.behavior.InteractWithDoor`, from the 26.2
//! jar. A `BehaviorBuilder` one-shot over `group(present(PATH),
//! registered(DOORS_TO_CLOSE), registered(NEAREST_LIVING_ENTITIES))` with two
//! captured mutables, `lastCheckedNode` (a `MutableObject<Node>`, null) and
//! `remainingCooldown` (a `MutableInt`, 0). `lambda$create$2`, 0..271:
//!
//! ```text
//! path = PATH; doors = DOORS_TO_CLOSE.tryGet()
//! if path.notStarted() || path.isDone(): return false
//! if Objects.equals(lastCheckedNode, path.getNextNode()): remainingCooldown = 20     (COOLDOWN_BEFORE_RERUNNING_IN_SAME_NODE)
//! else if --remainingCooldown > 0: return false
//! lastCheckedNode = path.getNextNode()
//! prev = path.getPreviousNode(); next = path.getNextNode()
//! for pos in [prev.asBlockPos(), next.asBlockPos()]:
//!     state = level.getBlockState(pos)
//!     if state.is(MOB_INTERACTABLE_DOORS, b -> b instanceof DoorBlock):
//!         if !door.isOpen(state): door.setOpen(mob, level, state, pos, true)
//!         doors = rememberDoorToClose(doorsAccessor, doors, level, pos)
//! doors.ifPresent(set -> closeDoorsThatIHaveOpenedOrPassedThrough(level, mob, prev, next, set, NEAREST_LIVING_ENTITIES.tryGet()))
//! return true
//! rememberDoorToClose(accessor, doors, level, pos):
//!     gp = GlobalPos.of(level.dimension(), pos)
//!     set = doors.map(s -> { s.add(gp); s })                 <- mutates the memory's own set in place
//!               .orElseGet(() -> { s = newHashSet(gp); accessor.set(s); s })
//!     return Optional.of(set)
//! closeDoorsThatIHaveOpenedOrPassedThrough(level, mob, prev, next, doors, nearestLiving) (0..203), iterator with remove:
//!     for gp in doors:
//!         pos = gp.pos()
//!         if prev != null && prev.asBlockPos().equals(pos): continue
//!         if next != null && next.asBlockPos().equals(pos): continue
//!         if isDoorTooFarAway(level, mob, gp): remove; continue
//!         state = level.getBlockState(pos)
//!         if !state.is(MOB_INTERACTABLE_DOORS, DoorBlock): remove; continue
//!         if !door.isOpen(state): remove; continue
//!         if areOtherMobsComingThroughDoor(mob, pos, nearestLiving): remove; continue
//!         door.setOpen(mob, level, state, pos, false); remove
//! isDoorTooFarAway(level, mob, gp):   gp.dimension() != level.dimension() || !gp.pos().closerToCenterThan(mob.position(), 3.0)
//! areOtherMobsComingThroughDoor(mob, pos, opt):
//!     opt.isEmpty() ? false : opt.get().stream().filter(e -> e.getType() == mob.getType())
//!                                    .filter(e -> pos.closerToCenterThan(e.position(), 2.0))
//!                                    .anyMatch(e -> isMobComingThroughDoor(e.getBrain(), pos))
//! isMobComingThroughDoor(brain, pos):  PATH present && !path.isDone() && prev != null && (pos == prev.asBlockPos() || pos == next.asBlockPos())
//! ```
//!
//! `prev` here is `path.getPreviousNode()`, which is `null` when
//! `nextNodeIndex == 0` — excluded by `notStarted()` above, so the
//! `prev.asBlockPos()` call at offset 93 never NPEs.
//!
//! # `DOORS_TO_CLOSE` is a `HashSet<GlobalPos>` and its order is not reproducible
//!
//! `GlobalPos` is a record over a `ResourceKey<Level>` and a `BlockPos`; the
//! record hash mixes `ResourceKey.hashCode()`, which is `Object`'s identity
//! hash. So the order the close loop walks doors in — and therefore which
//! door closes first when two are pending — varies between JVM runs of the
//! same world. This port keeps insertion order and records the finding.
//! Nothing else reads the set.

use rustmc_data::BlockPos;

use crate::ai::mob_ai::MobAi;
use crate::brain::behavior::{OneShot, Trigger};
use crate::brain::memory::{GlobalPos, MemoryModuleType, MemoryValue};
use crate::brain::BrainState;
use crate::host::{MobHost, PathLevel};
use crate::phys::closer_to_center_than;

/// `COOLDOWN_BEFORE_RERUNNING_IN_SAME_NODE`.
pub const COOLDOWN_BEFORE_RERUNNING_IN_SAME_NODE: i32 = 20;
/// `SKIP_CLOSING_DOOR_IF_FURTHER_AWAY_THAN`.
pub const SKIP_CLOSING_DOOR_IF_FURTHER_AWAY_THAN: f64 = 3.0;
/// `MAX_DISTANCE_TO_HOLD_DOOR_OPEN_FOR_OTHER_MOBS`.
pub const MAX_DISTANCE_TO_HOLD_DOOR_OPEN_FOR_OTHER_MOBS: f64 = 2.0;

/// `InteractWithDoor.create()`'s trigger with its two captured mutables.
#[derive(Debug, Clone, Default)]
pub struct InteractWithDoor {
    /// `lastCheckedNode`, compared by position (`Node.equals`).
    last_checked_node: Option<BlockPos>,
    /// `remainingCooldown`.
    remaining_cooldown: i32,
}

impl InteractWithDoor {
    /// `create()`.
    #[must_use]
    pub fn create() -> OneShot<Self> {
        OneShot::new(InteractWithDoor::default())
    }

    /// `remainingCooldown`, for tests.
    #[must_use]
    pub fn remaining_cooldown(&self) -> i32 {
        self.remaining_cooldown
    }

    /// `state.is(BlockTags.MOB_INTERACTABLE_DOORS, b -> b instanceof DoorBlock)`.
    fn is_interactable_door<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
        let f = mob.level().block_facts(pos);
        f.is_mob_interactable_door && f.door.is_some()
    }

    /// `DoorBlock.isOpen(state)`.
    fn door_is_open<M: MobHost>(mob: &M, pos: BlockPos) -> bool {
        mob.level().block_facts(pos).door.is_some_and(|d| d.open)
    }

    /// `rememberDoorToClose(accessor, doors, level, pos)`. The set lives in
    /// the memory; a present set is grown in place, an absent one is
    /// created and stored.
    fn remember_door_to_close<M: MobHost>(brain: &mut BrainState, mob: &M, pos: BlockPos) {
        let gp = GlobalPos {
            dimension: mob.dimension(),
            pos,
        };
        match brain.memory_mut(MemoryModuleType::DoorsToClose) {
            Some(MemoryValue::GlobalPosList(set)) => {
                if !set.contains(&gp) {
                    set.push(gp);
                }
            }
            _ => brain.set_memory_value(
                MemoryModuleType::DoorsToClose,
                MemoryValue::GlobalPosList(vec![gp]),
            ),
        }
    }

    /// `isDoorTooFarAway(level, mob, gp)`.
    fn is_door_too_far_away<M: MobHost>(mob: &M, gp: GlobalPos) -> bool {
        gp.dimension != mob.dimension()
            || !closer_to_center_than(
                gp.pos,
                mob.position(),
                SKIP_CLOSING_DOOR_IF_FURTHER_AWAY_THAN,
            )
    }

    /// `areOtherMobsComingThroughDoor(mob, pos, Optional<List<LivingEntity>>)`.
    fn are_other_mobs_coming_through_door<M: MobHost>(
        mob: &M,
        pos: BlockPos,
        nearest_living: Option<&[i32]>,
    ) -> bool {
        let Some(list) = nearest_living else {
            return false;
        };
        list.iter().any(|id| {
            mob.entity_type_matches_mine(*id)
                && mob.entity(*id).is_some_and(|e| {
                    closer_to_center_than(
                        pos,
                        e.position(),
                        MAX_DISTANCE_TO_HOLD_DOOR_OPEN_FOR_OTHER_MOBS,
                    )
                })
                && mob.entity_is_coming_through_door(*id, pos)
        })
    }

    /// `closeDoorsThatIHaveOpenedOrPassedThrough(level, mob, prev, next, doors, nearestLiving)`.
    pub fn close_doors_that_i_have_opened_or_passed_through<M: MobHost>(
        brain: &mut BrainState,
        cx: &mut MobAi<M>,
        prev: Option<BlockPos>,
        next: Option<BlockPos>,
    ) {
        let Some(doors) = brain
            .get_memory_if_registered(MemoryModuleType::DoorsToClose)
            .and_then(MemoryValue::as_global_pos_list)
            .map(<[GlobalPos]>::to_vec)
        else {
            return;
        };
        let nearest: Option<Vec<i32>> = brain
            .get_memory_if_registered(MemoryModuleType::NearestLivingEntities)
            .and_then(MemoryValue::as_entities)
            .map(<[i32]>::to_vec);
        let mut kept: Vec<GlobalPos> = Vec::new();
        for gp in doors {
            let pos = gp.pos;
            if prev == Some(pos) || next == Some(pos) {
                kept.push(gp);
                continue;
            }
            if Self::is_door_too_far_away(&cx.mob, gp) {
                continue;
            }
            if !Self::is_interactable_door(&cx.mob, pos) {
                continue;
            }
            if !Self::door_is_open(&cx.mob, pos) {
                continue;
            }
            if Self::are_other_mobs_coming_through_door(&cx.mob, pos, nearest.as_deref()) {
                continue;
            }
            cx.mob.set_door_open(pos, false);
        }
        if let Some(MemoryValue::GlobalPosList(set)) =
            brain.memory_mut(MemoryModuleType::DoorsToClose)
        {
            *set = kept;
        }
    }
}

impl<M: MobHost> Trigger<M> for InteractWithDoor {
    fn trigger(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>, _time: i64) -> bool {
        let Some(Some(path)) = brain.memory_internal(MemoryModuleType::Path) else {
            return false;
        };
        let Some(path) = path.as_path().cloned() else {
            return false;
        };
        if brain
            .memory_internal(MemoryModuleType::DoorsToClose)
            .is_none()
            || brain
                .memory_internal(MemoryModuleType::NearestLivingEntities)
                .is_none()
        {
            return false;
        }
        if path.not_started() || path.is_done() {
            return false;
        }
        let next = path.next_node().as_block_pos();
        if self.last_checked_node == Some(next) {
            self.remaining_cooldown = COOLDOWN_BEFORE_RERUNNING_IN_SAME_NODE;
        } else {
            self.remaining_cooldown -= 1;
            if self.remaining_cooldown > 0 {
                return false;
            }
        }
        self.last_checked_node = Some(next);
        let prev = path.previous_node().map(|n| n.as_block_pos());
        let prev_pos = prev.expect("notStarted() excluded nextNodeIndex == 0");
        for pos in [prev_pos, next] {
            if Self::is_interactable_door(&cx.mob, pos) {
                if !Self::door_is_open(&cx.mob, pos) {
                    cx.mob.set_door_open(pos, true);
                }
                Self::remember_door_to_close(brain, &cx.mob, pos);
            }
        }
        Self::close_doors_that_i_have_opened_or_passed_through(brain, cx, prev, Some(next));
        true
    }

    fn required_memories(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::Path,
            MemoryModuleType::DoorsToClose,
            MemoryModuleType::NearestLivingEntities,
        ]
    }

    fn name(&self) -> &'static str {
        "InteractWithDoor"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::behavior::BehaviorControl;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::Brain;
    use crate::host::{BlockFacts, DoorFacts};
    use crate::pathfinder::{Node, Path};
    use crate::testing::{GridLevel, TestMob};
    use rustmc_data::Dimension;

    fn door_facts(open: bool) -> BlockFacts {
        let mut f = BlockFacts::AIR;
        f.door = Some(DoorFacts {
            open,
            can_open_by_hand: true,
        });
        f.is_door_tag = true;
        f.is_mob_interactable_door = true;
        f
    }

    fn path_through(x_from: i32, x_to: i32) -> Path {
        let nodes: Vec<Node> = (x_from..=x_to).map(|x| Node::new(x, 1, 0)).collect();
        let mut p = Path::new(
            nodes,
            BlockPos {
                x: x_to,
                y: 1,
                z: 0,
            },
            true,
        );
        p.set_next_node_index(1);
        p
    }

    fn setup() -> (MobAi<TestMob>, BrainState) {
        let mut level = GridLevel::new();
        level.floor(-4, -4, 12, 4, 0);
        level.set(2, 1, 0, door_facts(false));
        let cx = MobAi::new(TestMob::new(level, 0.5, 1.0, 0.5, 1));
        let brain = Brain::<TestMob>::new(
            &[T::Path, T::DoorsToClose, T::NearestLivingEntities],
            vec![],
        )
        .state;
        (cx, brain)
    }

    #[test]
    fn opens_the_door_ahead_and_remembers_it() {
        let (mut cx, mut brain) = setup();
        // prev = (1,1,0), next = (2,1,0): the door is the next node.
        brain.set_memory_value(T::Path, MemoryValue::Path(path_through(1, 4)));
        let mut b = InteractWithDoor::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.calls, vec!["setOpen(2, 1, 0, true)"]);
        assert_eq!(
            brain
                .get_memory(T::DoorsToClose)
                .unwrap()
                .as_global_pos_list(),
            Some(
                &[GlobalPos {
                    dimension: Dimension::Overworld,
                    pos: BlockPos { x: 2, y: 1, z: 0 }
                }][..]
            )
        );
        assert_eq!(
            b.trigger.remaining_cooldown(),
            -1,
            "first pass decrements from 0"
        );
    }

    #[test]
    fn the_same_next_node_resets_the_cooldown_to_twenty_and_a_new_one_waits_it_out() {
        let (mut cx, mut brain) = setup();
        brain.set_memory_value(T::Path, MemoryValue::Path(path_through(1, 4)));
        let mut b = InteractWithDoor::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(
            b.try_start(&mut brain, &mut cx, 1),
            "same next node: cooldown = 20 and it runs"
        );
        assert_eq!(b.trigger.remaining_cooldown(), 20);
        let mut p = path_through(1, 4);
        p.set_next_node_index(2);
        brain.set_memory_value(T::Path, MemoryValue::Path(p));
        for t in 2..21 {
            assert!(!b.try_start(&mut brain, &mut cx, t), "counting down at {t}");
        }
        assert!(b.try_start(&mut brain, &mut cx, 21));
    }

    #[test]
    fn closes_a_remembered_open_door_once_it_is_behind_and_nobody_follows() {
        let (mut cx, mut brain) = setup();
        cx.mob.level.set(2, 1, 0, door_facts(true));
        cx.mob.x = 4.5;
        brain.set_memory_value(
            T::DoorsToClose,
            MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 2, y: 1, z: 0 },
            }]),
        );
        // prev = (4,1,0), next = (5,1,0): the door at 2 is neither.
        let mut p = path_through(1, 8);
        p.set_next_node_index(4);
        brain.set_memory_value(T::Path, MemoryValue::Path(p));
        let mut b = InteractWithDoor::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert_eq!(cx.mob.calls, vec!["setOpen(2, 1, 0, false)"]);
        assert_eq!(
            brain
                .get_memory(T::DoorsToClose)
                .unwrap()
                .as_global_pos_list(),
            Some(&[][..])
        );
    }

    #[test]
    fn a_door_too_far_away_is_forgotten_not_closed() {
        let (mut cx, mut brain) = setup();
        cx.mob.level.set(2, 1, 0, door_facts(true));
        cx.mob.x = 7.5;
        brain.set_memory_value(
            T::DoorsToClose,
            MemoryValue::GlobalPosList(vec![GlobalPos {
                dimension: Dimension::Overworld,
                pos: BlockPos { x: 2, y: 1, z: 0 },
            }]),
        );
        let mut p = path_through(1, 10);
        p.set_next_node_index(7);
        brain.set_memory_value(T::Path, MemoryValue::Path(p));
        let mut b = InteractWithDoor::create();
        assert!(b.try_start(&mut brain, &mut cx, 0));
        assert!(cx.mob.calls.is_empty());
        assert_eq!(
            brain
                .get_memory(T::DoorsToClose)
                .unwrap()
                .as_global_pos_list(),
            Some(&[][..])
        );
    }
}
