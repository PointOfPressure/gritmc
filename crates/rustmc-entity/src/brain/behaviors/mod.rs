//! The `net.minecraft.world.entity.ai.behavior` classes, one file each, and
//! the helpers they share (`PositionTracker.isVisibleBy`,
//! `NearestVisibleLivingEntities.contains/findClosest`), which need the
//! brain, the mob and the memo at once and so live here rather than on the
//! memory value.

pub mod acquire_poi;
pub mod animal_make_love;
pub mod animal_panic;
pub mod armadillo_ai;
pub mod assign_profession_from_job_site;
pub mod baby_follow_adult;
pub mod back_up_if_too_close;
pub mod become_passive_if_memory_present;
pub mod behavior_utils;
pub mod breeze_long_jump;
pub mod breeze_shoot;
pub mod breeze_shoot_when_stuck;
pub mod breeze_slide;
pub mod breeze_util;
pub mod camel_ai;
pub mod celebrate_villagers_survived_raid;
pub mod copy_memory_with_expiry;
pub mod count_down_cooldown_ticks;
pub mod croak;
pub mod crossbow_attack;
pub mod dismount_or_skip_mounting;
pub mod do_nothing;
pub mod erase_memory_if;
pub mod follow_temptation;
pub mod give_gift_to_hero;
pub mod go_and_give_items_to_target;
pub mod go_to_closest_village;
pub mod go_to_potential_job_site;
pub mod go_to_target_location;
pub mod go_to_wanted_item;
pub mod inside_brownian_walk;
pub mod interact_with;
pub mod interact_with_door;
pub mod jump_on_bed;
pub mod locate_hiding_place;
pub mod long_jump_mid_jump;
pub mod long_jump_to_random_pos;
pub mod long_jump_util;
pub mod look_and_follow_trading_player_sink;
pub mod look_at_target_sink;
pub mod melee_attack;
pub mod mount;
pub mod move_to_sky_seeing_spot;
pub mod move_to_target_sink;
pub mod play_dead;
pub mod play_tag_with_other_kids;
pub mod poi_competitor_scan;
pub mod prepare_ram_nearest_target;
pub mod ram_target;
pub mod random_look_around;
pub mod random_stroll;
pub mod react_to_bell;
pub mod remember_if_hoglin_was_killed;
pub mod reset_profession;
pub mod reset_raid_status;
pub mod ring_bell;
pub mod set_closest_home_as_walk_target;
pub mod set_entity_look_target;
pub mod set_entity_look_target_sometimes;
pub mod set_hidden_state;
pub mod set_look_and_interact;
pub mod set_raid_status;
pub mod set_roar_target;
pub mod set_walk_target_away_from;
pub mod set_walk_target_from_attack_target_if_target_out_of_reach;
pub mod set_walk_target_from_block_memory;
pub mod set_walk_target_from_look_target;
pub mod set_warden_look_target;
pub mod shoot_tongue;
pub mod show_trades_to_player;
pub mod sleep_in_bed;
pub mod sniffer_ai_overrides;
pub mod sniffer_digging;
pub mod sniffer_searching;
pub mod sniffer_sniffing;
pub mod socialize_at_bell;
pub mod spear_approach;
pub mod spear_attack;
pub mod spear_retreat;
pub mod start_admiring_item_if_seen;
pub mod start_attacking;
pub mod start_celebrating_if_target_dead;
pub mod start_hunting_hoglin;
pub mod stay_close_to_target;
pub mod stop_admiring_if_item_too_far_away;
pub mod stop_admiring_if_tired_of_trying_to_reach_item;
pub mod stop_attacking_if_target_invalid;
pub mod stop_being_angry_if_target_dead;
pub mod stop_holding_item_if_no_longer_admiring;
pub mod stroll_around_poi;
pub mod stroll_to_poi;
pub mod swim;
pub mod trade_with_villager;
pub mod transport_items_between_containers;
pub mod try_find_land;
pub mod try_lay_spawn_on_fluid_near_land;
pub mod try_to_sniff;
pub mod update_activity_from_schedule;
pub mod validate_nearby_poi;
pub mod village_bound_random_stroll;
pub mod villager_calm_down;
pub mod villager_make_love;
pub mod villager_panic_trigger;
pub mod wake_up;
pub mod warden_digging;
pub mod warden_emerging;
pub mod warden_force_unmount;
pub mod warden_roar;
pub mod warden_sonic_boom;
pub mod yield_job_site;

pub use acquire_poi::AcquirePoi;
pub use animal_make_love::AnimalMakeLove;
pub use animal_panic::AnimalPanic;
pub use assign_profession_from_job_site::AssignProfessionFromJobSite;
pub use baby_follow_adult::BabyFollowAdult;
pub use back_up_if_too_close::BackUpIfTooClose;
pub use become_passive_if_memory_present::BecomePassiveIfMemoryPresent;
pub use celebrate_villagers_survived_raid::CelebrateVillagersSurvivedRaid;
pub use copy_memory_with_expiry::CopyMemoryWithExpiry;
pub use count_down_cooldown_ticks::CountDownCooldownTicks;
pub use croak::Croak;
pub use crossbow_attack::CrossbowAttack;
pub use dismount_or_skip_mounting::DismountOrSkipMounting;
pub use do_nothing::DoNothing;
pub use erase_memory_if::EraseMemoryIf;
pub use follow_temptation::FollowTemptation;
pub use give_gift_to_hero::GiveGiftToHero;
pub use go_to_closest_village::GoToClosestVillage;
pub use go_to_potential_job_site::GoToPotentialJobSite;
pub use go_to_target_location::GoToTargetLocation;
pub use go_to_wanted_item::GoToWantedItem;
pub use inside_brownian_walk::InsideBrownianWalk;
pub use interact_with::InteractWith;
pub use interact_with_door::InteractWithDoor;
pub use jump_on_bed::JumpOnBed;
pub use locate_hiding_place::LocateHidingPlace;
pub use long_jump_mid_jump::LongJumpMidJump;
pub use long_jump_to_random_pos::LongJumpToRandomPos;
pub use look_and_follow_trading_player_sink::LookAndFollowTradingPlayerSink;
pub use look_at_target_sink::LookAtTargetSink;
pub use melee_attack::MeleeAttack;
pub use mount::Mount;
pub use move_to_sky_seeing_spot::MoveToSkySeeingSpot;
pub use move_to_target_sink::MoveToTargetSink;
pub use play_dead::{PlayDead, ValidatePlayDead};
pub use play_tag_with_other_kids::PlayTagWithOtherKids;
pub use poi_competitor_scan::PoiCompetitorScan;
pub use random_look_around::RandomLookAround;
pub use random_stroll::RandomStroll;
pub use react_to_bell::ReactToBell;
pub use remember_if_hoglin_was_killed::RememberIfHoglinWasKilled;
pub use reset_profession::ResetProfession;
pub use reset_raid_status::ResetRaidStatus;
pub use ring_bell::RingBell;
pub use set_closest_home_as_walk_target::SetClosestHomeAsWalkTarget;
pub use set_entity_look_target::SetEntityLookTarget;
pub use set_entity_look_target_sometimes::SetEntityLookTargetSometimes;
pub use set_hidden_state::SetHiddenState;
pub use set_look_and_interact::SetLookAndInteract;
pub use set_raid_status::SetRaidStatus;
pub use set_roar_target::SetRoarTarget;
pub use set_walk_target_away_from::SetWalkTargetAwayFrom;
pub use set_walk_target_from_attack_target_if_target_out_of_reach::SetWalkTargetFromAttackTargetIfTargetOutOfReach;
pub use set_walk_target_from_block_memory::SetWalkTargetFromBlockMemory;
pub use set_walk_target_from_look_target::SetWalkTargetFromLookTarget;
pub use set_warden_look_target::SetWardenLookTarget;
pub use show_trades_to_player::ShowTradesToPlayer;
pub use sleep_in_bed::SleepInBed;
pub use socialize_at_bell::SocializeAtBell;
pub use spear_approach::SpearApproach;
pub use spear_attack::SpearAttack;
pub use spear_retreat::SpearRetreat;
pub use start_admiring_item_if_seen::StartAdmiringItemIfSeen;
pub use start_attacking::StartAttacking;
pub use start_celebrating_if_target_dead::StartCelebratingIfTargetDead;
pub use start_hunting_hoglin::StartHuntingHoglin;
pub use stay_close_to_target::StayCloseToTarget;
pub use stop_admiring_if_item_too_far_away::StopAdmiringIfItemTooFarAway;
pub use stop_admiring_if_tired_of_trying_to_reach_item::StopAdmiringIfTiredOfTryingToReachItem;
pub use stop_attacking_if_target_invalid::StopAttackingIfTargetInvalid;
pub use stop_being_angry_if_target_dead::StopBeingAngryIfTargetDead;
pub use stop_holding_item_if_no_longer_admiring::StopHoldingItemIfNoLongerAdmiring;
pub use stroll_around_poi::StrollAroundPoi;
pub use stroll_to_poi::StrollToPoi;
pub use swim::Swim;
pub use trade_with_villager::TradeWithVillager;
pub use try_find_land::TryFindLand;
pub use try_lay_spawn_on_fluid_near_land::TryLaySpawnOnFluidNearLand;
pub use try_to_sniff::TryToSniff;
pub use update_activity_from_schedule::UpdateActivityFromSchedule;
pub use validate_nearby_poi::ValidateNearbyPoi;
pub use village_bound_random_stroll::VillageBoundRandomStroll;
pub use villager_calm_down::VillagerCalmDown;
pub use villager_make_love::VillagerMakeLove;
pub use villager_panic_trigger::VillagerPanicTrigger;
pub use wake_up::WakeUp;
pub use warden_digging::Digging;
pub use warden_emerging::Emerging;
pub use warden_force_unmount::ForceUnmount;
pub use warden_roar::Roar;
pub use warden_sonic_boom::SonicBoom;
pub use yield_job_site::YieldJobSite;

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{
    MemoryModuleType, MemoryValue, NearestVisibleLivingEntities, PositionTracker,
};
use crate::brain::sensor::is_entity_targetable;
use crate::brain::BrainState;
use crate::host::MobHost;

/// `EntityTracker.isVisibleBy(LivingEntity)` / `BlockPosTracker.isVisibleBy`
/// (always true). For an entity: not a `LivingEntity` → `true`; dead →
/// `false`; else the `NEAREST_VISIBLE_LIVING_ENTITIES` memory is present
/// and `contains(entity)`.
pub fn tracker_is_visible_by<M: MobHost>(
    tracker: &PositionTracker,
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
) -> bool {
    match tracker {
        PositionTracker::Block { .. } => true,
        PositionTracker::Entity { id, .. } => {
            if !cx.mob.entity_is_living(*id) {
                return true;
            }
            if !cx.mob.entity(*id).is_some_and(|e| e.alive) {
                return false;
            }
            nearest_visible_contains(brain, cx, *id)
        }
    }
}

/// The `lineOfSightTest` memo of a `NearestVisibleLivingEntities`:
/// `Object2BooleanOpenHashMap.computeIfAbsent(e, e -> Sensor.isEntityTargetable(level, mob, e))`.
/// `EMPTY`'s test is `e -> false`.
fn line_of_sight_test<M: MobHost>(
    nv: &mut NearestVisibleLivingEntities,
    brain: &BrainState,
    cx: &mut MobAi<M>,
    id: i32,
) -> bool {
    if nv.is_empty_instance {
        return false;
    }
    if let Some((_, b)) = nv.memo.iter().find(|(k, _)| *k == id) {
        return *b;
    }
    let b = cx
        .mob
        .entity(id)
        .is_some_and(|e| is_entity_targetable(brain, cx, &e));
    nv.memo.push((id, b));
    b
}

/// Runs `f` on the `NEAREST_VISIBLE_LIVING_ENTITIES` memory value with the
/// brain and mob beside it, writing the memo back (the value's own
/// `Object2BooleanOpenHashMap` is mutated in place in vanilla; the slot's
/// expiry is untouched). `None` when the memory is absent.
pub fn with_nearest_visible<M: MobHost, R>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    f: impl FnOnce(&mut NearestVisibleLivingEntities, &BrainState, &mut MobAi<M>) -> R,
) -> Option<R> {
    let mut nv = brain
        .get_memory_if_registered(MemoryModuleType::NearestVisibleLivingEntities)?
        .as_nearest_visible()?
        .clone();
    let r = f(&mut nv, brain, cx);
    if let Some(MemoryValue::NearestVisible(slot)) =
        brain.memory_mut(MemoryModuleType::NearestVisibleLivingEntities)
    {
        slot.memo = nv.memo;
    }
    Some(r)
}

/// `NearestVisibleLivingEntities.contains(LivingEntity)` on the brain's
/// memory: `nearbyEntities.contains(e) && lineOfSightTest.test(e)`; an
/// absent memory answers `false` (the callers guard with `hasMemoryValue`
/// or `Optional.isPresent` and this is what each of those reduces to).
pub fn nearest_visible_contains<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    id: i32,
) -> bool {
    with_nearest_visible(brain, cx, |nv, brain, cx| {
        nv.nearby.contains(&id) && line_of_sight_test(nv, brain, cx, id)
    })
    .unwrap_or(false)
}

/// `NearestVisibleLivingEntities.findClosest(Predicate)`: the first entity
/// in the sensor's order passing `pred` **and then** the line-of-sight
/// memo — `pred` is asked first, so its own side effects (none in vanilla)
/// and the memo's fill order follow the list.
pub fn nearest_visible_find_closest<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    mut pred: impl FnMut(&mut MobAi<M>, i32) -> bool,
) -> Option<i32> {
    with_nearest_visible(brain, cx, |nv, brain, cx| {
        let ids = nv.nearby.clone();
        ids.into_iter()
            .find(|id| pred(cx, *id) && line_of_sight_test(nv, brain, cx, *id))
    })
    .flatten()
}

/// `NearestVisibleLivingEntities.findAll(Predicate)`: every entity in
/// order passing `pred` and then the memo.
pub fn nearest_visible_find_all<M: MobHost>(
    brain: &mut BrainState,
    cx: &mut MobAi<M>,
    mut pred: impl FnMut(&mut MobAi<M>, i32) -> bool,
) -> Vec<i32> {
    with_nearest_visible(brain, cx, |nv, brain, cx| {
        let ids = nv.nearby.clone();
        ids.into_iter()
            .filter(|id| pred(cx, *id) && line_of_sight_test(nv, brain, cx, *id))
            .collect()
    })
    .unwrap_or_default()
}

/// A `Predicate<E>` over the mob, as the factories take it.
pub type MobPredicate<M> = Box<dyn FnMut(&mut MobAi<M>) -> bool>;
/// A `Function<E, Float>` over the mob (a speed modifier).
pub type MobSpeed<M> = Box<dyn FnMut(&mut MobAi<M>) -> f32>;

/// `MobCategory`, for `SetEntityLookTarget.create(MobCategory, float)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MobCategory {
    Monster,
    Creature,
    Ambient,
    Axolotls,
    UndergroundWaterCreature,
    WaterCreature,
    WaterAmbient,
    Misc,
}

/// The `Predicate<LivingEntity>` shapes the look-target behaviours take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitySelector {
    /// `e -> true`.
    Any,
    /// `e -> e.getType().getCategory() == category`.
    Category(MobCategory),
    /// `e -> e.is(type)`, by registry name (`minecraft:` implied).
    Kind(&'static str),
}

impl EntitySelector {
    /// The predicate, answered by the host.
    pub fn test<M: MobHost>(self, cx: &MobAi<M>, id: i32) -> bool {
        match self {
            EntitySelector::Any => true,
            EntitySelector::Category(c) => cx.mob.entity_category_is(id, c),
            EntitySelector::Kind(k) => cx.mob.entity_kind_is(id, k),
        }
    }
}
