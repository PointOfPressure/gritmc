//! `net.minecraft.world.entity.ai.memory` from the 26.2 jar: `MemoryModuleType`,
//! `MemoryStatus`, `MemorySlot`, `ExpirableValue`, `WalkTarget`,
//! `NearestVisibleLivingEntities`, plus the two `PositionTracker`s
//! (`BlockPosTracker`, `EntityTracker`) from `ai.behavior`.
//!
//! # `MemoryModuleType` is a registry, and the brain keys a `HashMap` on it
//!
//! `Brain.memories` is `Maps.newHashMap()` keyed on `MemoryModuleType`, whose
//! `hashCode` is `Object`'s identity hash, so its iteration order is not a
//! function of anything this port can see. **Nothing observable depends on
//! it**: the three places vanilla walks it — `forgetOutdatedMemories`
//! (every slot ticks independently), `clearMemories` and `forEach` (the
//! `pack()` serialiser) — are order-independent in effect. This port keys a
//! dense array on the registration ordinal and says so here rather than
//! pretending to a hash walk.
//!
//! The 116 variants below are `MemoryModuleType.<clinit>`'s `register` calls
//! in bytecode order, with the string each passes; the doc comment on each
//! variant is the static field's declared type parameter, which fixes the
//! [`MemoryValue`] variant a writer is allowed to store there.
//!
//! # `MemorySlot`, offset by offset
//!
//! ```text
//! tick():           if hasValue() && canExpire(): if hasExpired(): clear() else timeToLive--
//! canExpire():      timeToLive != Long.MAX_VALUE
//! hasExpired():     timeToLive <= 0
//! set(v, ttl):      value = v; timeToLive = ttl
//! set(v):           set(v, Long.MAX_VALUE)
//! clear():          value = null; timeToLive = Long.MAX_VALUE
//! ```
//!
//! Note the tick order: a memory set with expiry `n` is cleared on the
//! `n+1`th `forgetOutdatedMemories` after it was set — `hasExpired` is
//! asked *before* the decrement, so `ttl = 1` survives one tick at `1`,
//! then reads `0` and is cleared on the next.

use rustmc_data::{BlockPos, Dimension};

use crate::host::MobHost;
use crate::pathfinder::Path;
use crate::phys::{block_pos_containing_vec, Vec3};

/// `MemorySlot.NEVER_EXPIRE` — `Long.MAX_VALUE`.
pub const NEVER_EXPIRE: i64 = i64::MAX;

macro_rules! memory_types {
    ($( $(#[$m:meta])* $name:ident = $s:literal ),* $(,)?) => {
        /// `MemoryModuleType`, every registered instance in `<clinit>` order.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(u8)]
        pub enum MemoryModuleType {
            $( $(#[$m])* $name, )*
        }

        impl MemoryModuleType {
            /// Every type, in registration order.
            pub const ALL: &'static [MemoryModuleType] = &[ $( MemoryModuleType::$name, )* ];

            /// The registry name (`minecraft:` implied).
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $( MemoryModuleType::$name => $s, )*
                }
            }
        }
    };
}

memory_types! {
    /// `Void`.
    Dummy = "dummy",
    /// `GlobalPos`.
    Home = "home",
    /// `GlobalPos`.
    JobSite = "job_site",
    /// `GlobalPos`.
    PotentialJobSite = "potential_job_site",
    /// `GlobalPos`.
    MeetingPoint = "meeting_point",
    /// `List<GlobalPos>`.
    SecondaryJobSite = "secondary_job_site",
    /// `List<LivingEntity>`.
    NearestLivingEntities = "mobs",
    /// `NearestVisibleLivingEntities`.
    NearestVisibleLivingEntities = "visible_mobs",
    /// `List<LivingEntity>`.
    VisibleVillagerBabies = "visible_villager_babies",
    /// `List<Player>`.
    NearestPlayers = "nearest_players",
    /// `Player`.
    NearestVisiblePlayer = "nearest_visible_player",
    /// `Player`.
    NearestVisibleAttackablePlayer = "nearest_visible_targetable_player",
    /// `List<Player>`.
    NearestVisibleAttackablePlayers = "nearest_visible_targetable_players",
    /// `WalkTarget`.
    WalkTarget = "walk_target",
    /// `PositionTracker`.
    LookTarget = "look_target",
    /// `LivingEntity`.
    AttackTarget = "attack_target",
    /// `Boolean`.
    AttackCoolingDown = "attack_cooling_down",
    /// `LivingEntity`.
    InteractionTarget = "interaction_target",
    /// `AgeableMob`.
    BreedTarget = "breed_target",
    /// `Entity`.
    RideTarget = "ride_target",
    /// `Path`.
    Path = "path",
    /// `Set<GlobalPos>`.
    DoorsToClose = "doors_to_close",
    /// `BlockPos`.
    NearestBed = "nearest_bed",
    /// `DamageSource`.
    HurtBy = "hurt_by",
    /// `LivingEntity`.
    HurtByEntity = "hurt_by_entity",
    /// `LivingEntity`.
    AvoidTarget = "avoid_target",
    /// `LivingEntity`.
    NearestHostile = "nearest_hostile",
    /// `LivingEntity`.
    NearestAttackable = "nearest_attackable",
    /// `GlobalPos`.
    HidingPlace = "hiding_place",
    /// `Long`.
    HeardBellTime = "heard_bell_time",
    /// `Long`.
    CantReachWalkTargetSince = "cant_reach_walk_target_since",
    /// `Boolean`.
    GolemDetectedRecently = "golem_detected_recently",
    /// `Boolean`.
    DangerDetectedRecently = "danger_detected_recently",
    /// `Long`.
    LastSlept = "last_slept",
    /// `Long`.
    LastWoken = "last_woken",
    /// `Long`.
    LastWorkedAtPoi = "last_worked_at_poi",
    /// `LivingEntity`.
    NearestVisibleAdult = "nearest_visible_adult",
    /// `ItemEntity`.
    NearestVisibleWantedItem = "nearest_visible_wanted_item",
    /// `Mob`.
    NearestVisibleNemesis = "nearest_visible_nemesis",
    /// `Integer`.
    PlayDeadTicks = "play_dead_ticks",
    /// `Player`.
    TemptingPlayer = "tempting_player",
    /// `Integer`.
    TemptationCooldownTicks = "temptation_cooldown_ticks",
    /// `Integer`.
    GazeCooldownTicks = "gaze_cooldown_ticks",
    /// `Boolean`.
    IsTempted = "is_tempted",
    /// `Integer`.
    LongJumpCooldownTicks = "long_jump_cooling_down",
    /// `Boolean`.
    LongJumpMidJump = "long_jump_mid_jump",
    /// `Boolean`.
    HasHuntingCooldown = "has_hunting_cooldown",
    /// `Integer`.
    RamCooldownTicks = "ram_cooldown_ticks",
    /// `Vec3`.
    RamTarget = "ram_target",
    /// `Unit`.
    IsInWater = "is_in_water",
    /// `Unit`.
    IsPregnant = "is_pregnant",
    /// `Boolean`.
    IsPanicking = "is_panicking",
    /// `List<UUID>`.
    UnreachableTongueTargets = "unreachable_tongue_targets",
    /// `Set<GlobalPos>`.
    VisitedBlockPositions = "visited_block_positions",
    /// `Set<GlobalPos>`.
    UnreachableTransportBlockPositions = "unreachable_transport_block_positions",
    /// `Integer`.
    TransportItemsCooldownTicks = "transport_items_cooldown_ticks",
    /// `Integer`.
    ChargeCooldownTicks = "charge_cooldown_ticks",
    /// `Integer`.
    AttackTargetCooldown = "attack_target_cooldown",
    /// `Integer`.
    SpearFleeingTime = "spear_fleeing_time",
    /// `Vec3`.
    SpearFleeingPosition = "spear_fleeing_position",
    /// `Vec3`.
    SpearChargePosition = "spear_charge_position",
    /// `Integer`.
    SpearEngageTime = "spear_engage_time",
    /// `SpearAttack.SpearStatus`.
    SpearStatus = "spear_status",
    /// `UUID`.
    AngryAt = "angry_at",
    /// `Boolean`.
    UniversalAnger = "universal_anger",
    /// `Boolean`.
    AdmiringItem = "admiring_item",
    /// `Integer`.
    TimeTryingToReachAdmireItem = "time_trying_to_reach_admire_item",
    /// `Boolean`.
    DisableWalkToAdmireItem = "disable_walk_to_admire_item",
    /// `Boolean`.
    AdmiringDisabled = "admiring_disabled",
    /// `Boolean`.
    HuntedRecently = "hunted_recently",
    /// `BlockPos`.
    CelebrateLocation = "celebrate_location",
    /// `Boolean`.
    Dancing = "dancing",
    /// `Hoglin`.
    NearestVisibleHuntableHoglin = "nearest_visible_huntable_hoglin",
    /// `Hoglin`.
    NearestVisibleBabyHoglin = "nearest_visible_baby_hoglin",
    /// `Player`.
    NearestTargetablePlayerNotWearingGold = "nearest_targetable_player_not_wearing_gold",
    /// `List<AbstractPiglin>`.
    NearbyAdultPiglins = "nearby_adult_piglins",
    /// `List<AbstractPiglin>`.
    NearestVisibleAdultPiglins = "nearest_visible_adult_piglins",
    /// `List<Hoglin>`.
    NearestVisibleAdultHoglins = "nearest_visible_adult_hoglins",
    /// `AbstractPiglin`.
    NearestVisibleAdultPiglin = "nearest_visible_adult_piglin",
    /// `LivingEntity`.
    NearestVisibleZombified = "nearest_visible_zombified",
    /// `Integer`.
    VisibleAdultPiglinCount = "visible_adult_piglin_count",
    /// `Integer`.
    VisibleAdultHoglinCount = "visible_adult_hoglin_count",
    /// `Player`.
    NearestPlayerHoldingWantedItem = "nearest_player_holding_wanted_item",
    /// `Boolean`.
    AteRecently = "ate_recently",
    /// `BlockPos`.
    NearestRepellent = "nearest_repellent",
    /// `Boolean`.
    Pacified = "pacified",
    /// `LivingEntity`.
    RoarTarget = "roar_target",
    /// `BlockPos`.
    DisturbanceLocation = "disturbance_location",
    /// `Unit`.
    RecentProjectile = "recent_projectile",
    /// `Unit`.
    IsSniffing = "is_sniffing",
    /// `Unit`.
    IsEmerging = "is_emerging",
    /// `Unit`.
    RoarSoundDelay = "roar_sound_delay",
    /// `Unit`.
    DigCooldown = "dig_cooldown",
    /// `Unit`.
    RoarSoundCooldown = "roar_sound_cooldown",
    /// `Unit`.
    SniffCooldown = "sniff_cooldown",
    /// `Unit`.
    TouchCooldown = "touch_cooldown",
    /// `Unit`.
    VibrationCooldown = "vibration_cooldown",
    /// `Unit`.
    SonicBoomCooldown = "sonic_boom_cooldown",
    /// `Unit`.
    SonicBoomSoundCooldown = "sonic_boom_sound_cooldown",
    /// `Unit`.
    SonicBoomSoundDelay = "sonic_boom_sound_delay",
    /// `UUID`.
    LikedPlayer = "liked_player",
    /// `GlobalPos`.
    LikedNoteblockPosition = "liked_noteblock",
    /// `Integer`.
    LikedNoteblockCooldownTicks = "liked_noteblock_cooldown_ticks",
    /// `Integer`.
    ItemPickupCooldownTicks = "item_pickup_cooldown_ticks",
    /// `List<GlobalPos>`.
    SnifferExploredPositions = "sniffer_explored_positions",
    /// `BlockPos`.
    SnifferSniffingTarget = "sniffer_sniffing_target",
    /// `Boolean`.
    SnifferDigging = "sniffer_digging",
    /// `Boolean`.
    SnifferHappy = "sniffer_happy",
    /// `Unit`.
    BreezeJumpCooldown = "breeze_jump_cooldown",
    /// `Unit`.
    BreezeShoot = "breeze_shoot",
    /// `Unit`.
    BreezeShootCharging = "breeze_shoot_charging",
    /// `Unit`.
    BreezeShootRecovering = "breeze_shoot_recover",
    /// `Unit`.
    BreezeShootCooldown = "breeze_shoot_cooldown",
    /// `Unit`.
    BreezeJumpInhaling = "breeze_jump_inhaling",
    /// `BlockPos`.
    BreezeJumpTarget = "breeze_jump_target",
    /// `Unit`.
    BreezeLeavingWater = "breeze_leaving_water",
}

/// `MemoryStatus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemoryStatus {
    ValuePresent,
    ValueAbsent,
    Registered,
}

/// `GlobalPos` — a dimension and a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalPos {
    pub dimension: Dimension,
    pub pos: BlockPos,
}

/// `SpearAttack.SpearStatus`: `APPROACH`, `CHARGING`, `RETREAT`, in
/// ordinal order (`$values`, 0..21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpearStatus {
    Approach,
    Charging,
    Retreat,
}

/// `DamageSource`, as much of it as a brain memory reads: the direct and
/// causing entities and the damage type's registry name.
#[derive(Debug, Clone, PartialEq)]
pub struct DamageSourceFacts {
    /// `getEntity()` — the causing entity.
    pub entity: Option<i32>,
    /// `getDirectEntity()`.
    pub direct_entity: Option<i32>,
    /// `type()`'s registry key, `minecraft:` implied.
    pub kind: &'static str,
}

/// `PositionTracker`: `BlockPosTracker` or `EntityTracker`.
#[derive(Debug, Clone, PartialEq)]
pub enum PositionTracker {
    /// `BlockPosTracker(BlockPos)` (`center = Vec3.atCenterOf(pos)`) or
    /// `BlockPosTracker(Vec3)` (`pos = BlockPos.containing(v)`).
    Block { pos: BlockPos, center: Vec3 },
    /// `EntityTracker(entity, trackEyeHeight, targetEyeHeight)`.
    Entity {
        id: i32,
        track_eye_height: bool,
        target_eye_height: bool,
    },
}

impl PositionTracker {
    /// `new BlockPosTracker(BlockPos)`.
    #[must_use]
    pub fn block(pos: BlockPos) -> Self {
        PositionTracker::Block {
            pos,
            center: Vec3::at_center_of(pos),
        }
    }

    /// `new BlockPosTracker(Vec3)`.
    #[must_use]
    pub fn block_at(v: Vec3) -> Self {
        PositionTracker::Block {
            pos: block_pos_containing_vec(v),
            center: v,
        }
    }

    /// `new EntityTracker(entity, trackEyeHeight)` — `targetEyeHeight`
    /// false.
    #[must_use]
    pub fn entity(id: i32, track_eye_height: bool) -> Self {
        PositionTracker::Entity {
            id,
            track_eye_height,
            target_eye_height: false,
        }
    }

    /// `new EntityTracker(entity, trackEyeHeight, targetEyeHeight)`.
    #[must_use]
    pub fn entity_full(id: i32, track_eye_height: bool, target_eye_height: bool) -> Self {
        PositionTracker::Entity {
            id,
            track_eye_height,
            target_eye_height,
        }
    }

    /// `EntityTracker.getEntity()` when this is one.
    #[must_use]
    pub fn entity_id(&self) -> Option<i32> {
        match self {
            PositionTracker::Entity { id, .. } => Some(*id),
            PositionTracker::Block { .. } => None,
        }
    }

    /// `currentPosition()`. For an `EntityTracker`:
    /// `trackEyeHeight ? entity.position().add(0, eyeHeight, 0) : entity.position()`.
    /// An entity the host no longer knows answers `None`, where vanilla
    /// would read a stale object; callers say what they do with it.
    #[must_use]
    pub fn current_position<M: MobHost>(&self, mob: &M) -> Option<Vec3> {
        match self {
            PositionTracker::Block { center, .. } => Some(*center),
            PositionTracker::Entity {
                id,
                track_eye_height,
                ..
            } => {
                let e = mob.entity(*id)?;
                Some(if *track_eye_height {
                    Vec3::new(e.x, e.y + (e.eye_y - e.y), e.z)
                } else {
                    Vec3::new(e.x, e.y, e.z)
                })
            }
        }
    }

    /// `currentBlockPosition()`. For an `EntityTracker`:
    /// `targetEyeHeight ? BlockPos.containing(entity.getEyePosition()) : entity.blockPosition()`.
    #[must_use]
    pub fn current_block_position<M: MobHost>(&self, mob: &M) -> Option<BlockPos> {
        match self {
            PositionTracker::Block { pos, .. } => Some(*pos),
            PositionTracker::Entity {
                id,
                target_eye_height,
                ..
            } => {
                let e = mob.entity(*id)?;
                Some(if *target_eye_height {
                    block_pos_containing_vec(Vec3::new(e.x, e.eye_y, e.z))
                } else {
                    block_pos_containing_vec(Vec3::new(e.x, e.y, e.z))
                })
            }
        }
    }
}

/// `WalkTarget`.
#[derive(Debug, Clone, PartialEq)]
pub struct WalkTarget {
    pub target: PositionTracker,
    pub speed_modifier: f32,
    pub close_enough_dist: i32,
}

impl WalkTarget {
    /// `WalkTarget(PositionTracker, float, int)`.
    #[must_use]
    pub fn new(target: PositionTracker, speed_modifier: f32, close_enough_dist: i32) -> Self {
        WalkTarget {
            target,
            speed_modifier,
            close_enough_dist,
        }
    }

    /// `WalkTarget(BlockPos, float, int)`.
    #[must_use]
    pub fn at_block(pos: BlockPos, speed_modifier: f32, close_enough_dist: i32) -> Self {
        Self::new(
            PositionTracker::block(pos),
            speed_modifier,
            close_enough_dist,
        )
    }

    /// `WalkTarget(Vec3, float, int)` — a `BlockPosTracker(BlockPos.containing(v))`,
    /// so the centre is the block's centre, not `v`.
    #[must_use]
    pub fn at_vec(v: Vec3, speed_modifier: f32, close_enough_dist: i32) -> Self {
        Self::new(
            PositionTracker::block(block_pos_containing_vec(v)),
            speed_modifier,
            close_enough_dist,
        )
    }

    /// `WalkTarget(Entity, float, int)` — an `EntityTracker(entity, false)`.
    #[must_use]
    pub fn at_entity(id: i32, speed_modifier: f32, close_enough_dist: i32) -> Self {
        Self::new(
            PositionTracker::entity(id, false),
            speed_modifier,
            close_enough_dist,
        )
    }
}

/// `NearestVisibleLivingEntities`: the sensed list plus a memo of
/// `Sensor.isEntityTargetable(level, mob, e)` per entity
/// (`Object2BooleanOpenHashMap.computeIfAbsent`). The memo is keyed by
/// entity id, which is what `Entity.hashCode()` returns.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NearestVisibleLivingEntities {
    /// `nearbyEntities`, in the sensor's order.
    pub nearby: Vec<i32>,
    /// The `lineOfSightTest` memo; `EMPTY`'s test is `e -> false`, which
    /// [`Self::empty`] marks with `is_empty_instance`.
    pub memo: Vec<(i32, bool)>,
    /// True for `NearestVisibleLivingEntities.empty()`, whose predicate
    /// answers `false` for everything.
    pub is_empty_instance: bool,
}

impl NearestVisibleLivingEntities {
    /// `NearestVisibleLivingEntities.empty()`.
    #[must_use]
    pub fn empty() -> Self {
        NearestVisibleLivingEntities {
            nearby: Vec::new(),
            memo: Vec::new(),
            is_empty_instance: true,
        }
    }

    /// `new NearestVisibleLivingEntities(level, mob, list)`.
    #[must_use]
    pub fn new(nearby: Vec<i32>) -> Self {
        NearestVisibleLivingEntities {
            nearby,
            memo: Vec::new(),
            is_empty_instance: false,
        }
    }
}

/// A memory's value. The variant a slot may hold is fixed by the
/// [`MemoryModuleType`]'s declared type (see the variant docs above).
#[derive(Debug, Clone, PartialEq)]
pub enum MemoryValue {
    /// `Unit.INSTANCE` / `Void`.
    Unit,
    Bool(bool),
    Int(i32),
    Long(i64),
    /// An `Entity`/`LivingEntity`/`Player`/... by id.
    Entity(i32),
    /// A `List<? extends Entity>` in the sensor's order.
    Entities(Vec<i32>),
    BlockPos(BlockPos),
    GlobalPos(GlobalPos),
    /// `List<GlobalPos>` or `Set<GlobalPos>` (see the module doc on
    /// `DOORS_TO_CLOSE`'s set order), in insertion order.
    GlobalPosList(Vec<GlobalPos>),
    Vec3(Vec3),
    WalkTarget(WalkTarget),
    PositionTracker(PositionTracker),
    Path(Path),
    NearestVisible(NearestVisibleLivingEntities),
    DamageSource(DamageSourceFacts),
    Uuid(u128),
    Uuids(Vec<u128>),
    SpearStatus(SpearStatus),
}

impl MemoryValue {
    /// `Brain.isEmptyCollection(Object)`: `o instanceof Collection c && c.isEmpty()`.
    /// `NearestVisibleLivingEntities` is not a `Collection`.
    #[must_use]
    pub fn is_empty_collection(&self) -> bool {
        match self {
            MemoryValue::Entities(v) => v.is_empty(),
            MemoryValue::GlobalPosList(v) => v.is_empty(),
            MemoryValue::Uuids(v) => v.is_empty(),
            _ => false,
        }
    }

    /// The id when this is an entity memory.
    #[must_use]
    pub fn as_entity(&self) -> Option<i32> {
        match self {
            MemoryValue::Entity(id) => Some(*id),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_entities(&self) -> Option<&[i32]> {
        match self {
            MemoryValue::Entities(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_walk_target(&self) -> Option<&WalkTarget> {
        match self {
            MemoryValue::WalkTarget(w) => Some(w),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_position_tracker(&self) -> Option<&PositionTracker> {
        match self {
            MemoryValue::PositionTracker(p) => Some(p),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_block_pos(&self) -> Option<BlockPos> {
        match self {
            MemoryValue::BlockPos(p) => Some(*p),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_global_pos(&self) -> Option<GlobalPos> {
        match self {
            MemoryValue::GlobalPos(p) => Some(*p),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_global_pos_list(&self) -> Option<&[GlobalPos]> {
        match self {
            MemoryValue::GlobalPosList(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_long(&self) -> Option<i64> {
        match self {
            MemoryValue::Long(v) => Some(*v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        match self {
            MemoryValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            MemoryValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_vec3(&self) -> Option<Vec3> {
        match self {
            MemoryValue::Vec3(v) => Some(*v),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            MemoryValue::Path(p) => Some(p),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_nearest_visible(&self) -> Option<&NearestVisibleLivingEntities> {
        match self {
            MemoryValue::NearestVisible(n) => Some(n),
            _ => None,
        }
    }
}

/// `MemorySlot<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct MemorySlot {
    value: Option<MemoryValue>,
    time_to_live: i64,
}

impl Default for MemorySlot {
    fn default() -> Self {
        Self::create()
    }
}

impl MemorySlot {
    /// `MemorySlot.create()`: `new MemorySlot(null, Long.MAX_VALUE)`.
    #[must_use]
    pub fn create() -> Self {
        MemorySlot {
            value: None,
            time_to_live: NEVER_EXPIRE,
        }
    }

    /// `tick()`.
    pub fn tick(&mut self) {
        if self.has_value() && self.can_expire() {
            if self.has_expired() {
                self.clear();
            } else {
                self.time_to_live -= 1;
            }
        }
    }

    /// `set(T, long)`.
    pub fn set_with_expiry(&mut self, value: MemoryValue, time_to_live: i64) {
        self.value = Some(value);
        self.time_to_live = time_to_live;
    }

    /// `set(T)`.
    pub fn set(&mut self, value: MemoryValue) {
        self.set_with_expiry(value, NEVER_EXPIRE);
    }

    /// `clear()`.
    pub fn clear(&mut self) {
        self.value = None;
        self.time_to_live = NEVER_EXPIRE;
    }

    /// `hasValue()`.
    #[must_use]
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// `value()`.
    #[must_use]
    pub fn value(&self) -> Option<&MemoryValue> {
        self.value.as_ref()
    }

    /// The value, mutably (see `BrainState::memory_mut`).
    pub fn value_mut(&mut self) -> Option<&mut MemoryValue> {
        self.value.as_mut()
    }

    /// `canExpire()`.
    #[must_use]
    pub fn can_expire(&self) -> bool {
        self.time_to_live != NEVER_EXPIRE
    }

    /// `hasExpired()`.
    #[must_use]
    pub fn has_expired(&self) -> bool {
        self.time_to_live <= 0
    }

    /// `timeToLive()`.
    #[must_use]
    pub fn time_to_live(&self) -> i64 {
        self.time_to_live
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_types_are_the_jars_116_in_order() {
        assert_eq!(MemoryModuleType::ALL.len(), 116);
        assert_eq!(MemoryModuleType::ALL[0], MemoryModuleType::Dummy);
        assert_eq!(MemoryModuleType::ALL[13].name(), "walk_target");
        assert_eq!(MemoryModuleType::ALL[115].name(), "breeze_leaving_water");
        assert_eq!(
            MemoryModuleType::LongJumpCooldownTicks.name(),
            "long_jump_cooling_down"
        );
        assert_eq!(
            MemoryModuleType::BreezeShootRecovering.name(),
            "breeze_shoot_recover"
        );
    }

    #[test]
    fn slot_expiry_clears_on_the_tick_after_it_reads_zero() {
        let mut s = MemorySlot::create();
        assert!(!s.can_expire());
        s.set_with_expiry(MemoryValue::Unit, 2);
        s.tick();
        assert_eq!(s.time_to_live(), 1);
        assert!(s.has_value());
        s.tick();
        assert_eq!(s.time_to_live(), 0);
        assert!(
            s.has_value(),
            "ttl 0 is cleared on the NEXT tick, not this one"
        );
        s.tick();
        assert!(!s.has_value());
        assert_eq!(s.time_to_live(), NEVER_EXPIRE);
    }

    #[test]
    fn empty_collection_is_only_the_list_variants() {
        assert!(MemoryValue::Entities(vec![]).is_empty_collection());
        assert!(!MemoryValue::Entities(vec![1]).is_empty_collection());
        assert!(
            !MemoryValue::NearestVisible(NearestVisibleLivingEntities::new(vec![]))
                .is_empty_collection()
        );
        assert!(!MemoryValue::Unit.is_empty_collection());
    }

    #[test]
    fn walk_target_from_vec_snaps_to_block_centre() {
        let w = WalkTarget::at_vec(Vec3::new(1.7, 2.2, -0.3), 0.5, 1);
        match w.target {
            PositionTracker::Block { pos, center } => {
                assert_eq!(pos, BlockPos { x: 1, y: 2, z: -1 });
                assert_eq!(center, Vec3::new(1.5, 2.5, -0.5));
            }
            PositionTracker::Entity { .. } => panic!(),
        }
    }
}
