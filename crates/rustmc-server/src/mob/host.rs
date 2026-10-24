//! `ServerMob`: the `Entity` / `LivingEntity` / `Mob` fields a living mob
//! carries in a `ServerLevel`, answering [`MobHost`] for the AI in
//! `rustmc-entity`.
//!
//! Reference (26.2): `Entity.<init>` (the field defaults), `LivingEntity.<init>`,
//! `Mob.<init>`, `PathfinderMob.<init>`, `AgeableMob.<init>`, `Animal.<init>`,
//! `Bat.<init>`; the accessors each `MobHost` method names; `Level.getNearestPlayer`,
//! `EntityGetter.getEntitiesOfClass`, `LivingEntity.hasLineOfSight`,
//! `Animal.getWalkTargetValue`, `TemptGoal`'s item predicate,
//! `Mob.getMaxFallDistance`, `LivingEntity.getComfortableFallDistance`.
//!
//! # What this host does not answer, and where each is named
//!
//! * `entities_of_class_in` walks this server's entity list by ascending
//!   id, not the entity section storage's section-then-insertion order
//!   (`rustmc-entity/src/lib.rs` records the divergence). No goal here
//!   sees two entities in one query on the corpus.
//! * A player's eye height is `1.62` standing; the sneaking pose (1.27) is
//!   not tracked for other players and is not read by any goal this lane
//!   ships.
//! * `visibility_percent_of` is `1.0`: no invisibility, no armour, no head
//!   item on any entity here.
use super::attributes::MobAttributes;
use super::data::{id as data_id, MobData};
use super::level::MobLevel;
use super::{MobKind, PlayerHands};
use crate::attributes::{Attributes, Modifier};
use crate::physics::{Body, Profile};
use rustmc_data::BlockPos;
use rustmc_entity::host::{BlockFacts, Difficulty, EntityClass, EntityFacts, MobHost};
use rustmc_entity::pathfinder::PathType;
use rustmc_entity::phys::{Aabb, Vec3};
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

/// `Attributes` ids the tick reads.
pub const MOVEMENT_SPEED: &str = "minecraft:movement_speed";
pub const FOLLOW_RANGE: &str = "minecraft:follow_range";
pub const TEMPT_RANGE: &str = "minecraft:tempt_range";
pub const MAX_HEALTH: &str = "minecraft:max_health";
pub const JUMP_STRENGTH: &str = "minecraft:jump_strength";
pub const FRICTION_MODIFIER: &str = "minecraft:friction_modifier";
pub const MOVEMENT_EFFICIENCY: &str = "minecraft:movement_efficiency";

/// `Mob.DATA_MOB_FLAGS_ID` bits: `setNoAi` 1, `setLeftHanded` 2,
/// `setAggressive` 4.
pub const MOB_FLAG_NO_AI: u8 = 1;
pub const MOB_FLAG_LEFT_HANDED: u8 = 2;
pub const MOB_FLAG_AGGRESSIVE: u8 = 4;

/// The mob.
pub struct ServerMob {
    pub level: MobLevel,
    pub kind: &'static MobKind,
    pub id: i32,
    pub uuid: u128,
    /// `tickCount`, incremented by the tick loop before `tick()`.
    pub tick_count: i32,
    /// `Entity.random` — `RandomSource.create()`, the mob's own.
    pub random: LegacyRandomSource,
    // ---- Entity ----
    /// `position`, `deltaMovement`, `onGround` and the collision flags,
    /// in the shape `Entity.move` reads and writes.
    pub body: Body,
    /// `xo`, `yo`, `zo`.
    pub old_pos: (f64, f64, f64),
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub first_tick: bool,
    pub removed: bool,
    /// `EntityFluidInteraction`: per fluid (water, lava) the `height`
    /// and `eyesInside` of the tracker.
    pub fluid_height: [f64; 2],
    pub eyes_in_fluid: [bool; 2],
    pub was_touching_water: bool,
    pub was_eye_in_water: bool,
    pub is_in_powder_snow: bool,
    pub was_in_powder_snow: bool,
    pub fall_distance: f64,
    pub remaining_fire_ticks: i32,
    pub needs_sync: bool,
    pub hurt_marked: bool,
    /// `lastKnownPosition` / `lastKnownSpeed`, for `computeSpeed`.
    pub last_known_position: Option<Vec3>,
    pub last_known_speed: Vec3,
    /// `SynchedEntityData`.
    pub data: MobData,
    // ---- LivingEntity ----
    pub y_head_rot: f32,
    pub y_body_rot: f32,
    pub y_head_rot_o: f32,
    pub y_body_rot_o: f32,
    pub xxa: f32,
    pub yya: f32,
    pub zza: f32,
    pub speed: f32,
    pub jumping: bool,
    pub no_jump_delay: i32,
    pub attack_anim: f32,
    pub o_attack_anim: f32,
    pub swing_time: i32,
    pub swinging: bool,
    pub hurt_time: i32,
    /// `LivingEntity.hurtDuration`, copied into `hurtTime` on a full hit.
    pub hurt_duration: i32,
    /// `LivingEntity.lastHurt` — **the damage of the last full hit**, and the
    /// value `hurtServer`'s cooldown arm compares the next one against.
    ///
    /// It was missing until 2026-09-05 and its absence was not visible in any
    /// single hit: `mob::combat::vitals` built a fresh `Health` per call, so
    /// `lastHurt` was 0.0 every time and **every** hit inside the
    /// invulnerability window landed for its whole damage instead of for the
    /// difference. `tests/mob_combat.rs`'s cooldown part is what caught it.
    pub last_hurt: f32,
    /// `LivingEntity.noActionTime`, zeroed by `hurtServer` at offset 53
    /// **before** any of its remaining returns.
    pub no_action_time: i32,
    pub invulnerable_time: i32,
    pub last_hurt_by_mob_timestamp: i32,
    pub last_hurt_by_player_memory_time: i32,
    pub effects_dirty: bool,
    pub dead: bool,
    pub death_time: i32,
    pub fall_fly_ticks: i32,
    pub auto_spin_attack_ticks: i32,
    pub discard_friction: bool,
    pub attributes: MobAttributes,
    /// `LivingEntity.lastPos`.
    pub last_pos: Option<BlockPos>,
    // ---- Mob ----
    pub ambient_sound_time: i32,
    pub persistence_required: bool,
    pub can_pick_up_loot: bool,
    /// `pathfindingMalus`, an `EnumMap`.
    pub pathfinding_malus: Vec<(PathType, f32)>,
    pub target: Option<i32>,
    pub last_hurt_by_mob: Option<i32>,
    pub last_hurt_mob: Option<i32>,
    pub home_radius: i32,
    // ---- AgeableMob / Animal ----
    pub age: i32,
    pub forced_age: i32,
    pub forced_age_timer: i32,
    pub age_lock_particle_timer: i32,
    pub in_love: i32,
    /// `Animal.loveCause`, the player who fed it — an entity id here.
    pub love_cause: Option<i32>,
    // ---- Mob: Leashable, home ----
    /// `Mob.leashData`.
    pub leash: Option<super::leash::LeashData>,
    /// `Mob.homePosition`, `BlockPos.ZERO` until restricted.
    pub home_position: BlockPos,
    /// The `PanicGoal`'s id, for `PathfinderMob.isPanicking()` read from
    /// inside the mob's own tick (the store's copy is out while it ticks).
    pub panic_goal: Option<rustmc_entity::ai::goal_selector::GoalId>,
    // ---- Chicken ----
    /// `Chicken`'s own fields; `None` for every other kind.
    pub chicken: Option<super::chicken::ChickenState>,
    // ---- Bat ----
    pub target_position: Option<BlockPos>,
    /// `Entity.moveDist` / `nextStep`, the step-sound cadence.
    pub walk: crate::step::Walk,
    /// The golem classes' own fields — see [`super::golem::GolemState`].
    pub golem: super::golem::GolemState,
    /// `ArmorStand.lastHit`, a game time, `0L` at construction.
    pub stand_last_hit: i64,
}

impl ServerMob {
    /// `other instanceof Enemy`, read off the other mob's kind. A player,
    /// an item or anything that is not a mob here is not an `Enemy`.
    #[must_use]
    pub fn is_enemy(&self, other: i32) -> bool {
        super::mob_kind_name(other)
            .and_then(super::kind_by_name)
            .is_some_and(|k| k.enemy)
    }

    /// `other instanceof Creeper`.
    #[must_use]
    pub fn is_creeper(&self, other: i32) -> bool {
        super::mob_kind_name(other) == Some("minecraft:creeper")
    }

    /// The constructor chain from `Entity.<init>` down to the kind's own,
    /// with the id already drawn by the caller (`Level.getNextEntityId`)
    /// and the level's tables resolved.
    ///
    /// Draws, in order, from the mob's own `random`: the uuid
    /// (`Mth.createInsecureUUID`, two `nextLong`s), then
    /// `LivingEntity.<init>`'s `nextFloat() * 6.2831855f` yaw — which
    /// `EntityType.create` overwrites for a spawned mob and `/summon` keeps.
    ///
    /// `None` when the type has no attribute supplier, which no mob type
    /// lacks; refused rather than defaulted.
    pub fn new(
        level: MobLevel,
        kind: &'static MobKind,
        id: i32,
        table: &'static Attributes,
    ) -> Option<Self> {
        let mut random = crate::dropitem::unique_random();
        let uuid = crate::dropitem::insecure_uuid(&mut random);
        let attributes = MobAttributes::new(table, kind.name)?;
        let data = match kind.data {
            super::DataLayout::Pig => {
                let (variant_default, sound_variant_default) = super::pig_defaults()?;
                MobData::pig(variant_default, sound_variant_default)
            }
            super::DataLayout::Cow => {
                let (v, s) = super::variants::defaults(&super::variants::COW)?;
                MobData::cow(v, s)
            }
            super::DataLayout::Mooshroom => MobData::mooshroom(),
            super::DataLayout::Sheep => MobData::sheep(),
            super::DataLayout::Chicken => {
                let (v, s) = super::variants::defaults(&super::variants::CHICKEN)?;
                MobData::chicken(v, s)
            }
            super::DataLayout::Bat => MobData::bat(),
            super::DataLayout::IronGolem => MobData::iron_golem(),
            super::DataLayout::SnowGolem => MobData::snow_golem(),
            super::DataLayout::CopperGolem => MobData::copper_golem(),
            super::DataLayout::ArmorStand => MobData::armor_stand(),
        };
        let mut m = Self {
            level,
            kind,
            id,
            uuid,
            tick_count: 0,
            random,
            body: Body::at(crate::collide::Vec3::new(0.0, 0.0, 0.0)),
            old_pos: (0.0, 0.0, 0.0),
            y_rot: 0.0,
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            first_tick: true,
            removed: false,
            fluid_height: [0.0; 2],
            eyes_in_fluid: [false; 2],
            was_touching_water: false,
            was_eye_in_water: false,
            is_in_powder_snow: false,
            was_in_powder_snow: false,
            fall_distance: 0.0,
            remaining_fire_ticks: 0,
            needs_sync: false,
            hurt_marked: false,
            last_known_position: None,
            last_known_speed: Vec3::ZERO,
            data,
            y_head_rot: 0.0,
            y_body_rot: 0.0,
            y_head_rot_o: 0.0,
            y_body_rot_o: 0.0,
            xxa: 0.0,
            yya: 0.0,
            zza: 0.0,
            speed: 0.0,
            jumping: false,
            no_jump_delay: 0,
            attack_anim: 0.0,
            o_attack_anim: 0.0,
            swing_time: 0,
            swinging: false,
            hurt_time: 0,
            hurt_duration: 0,
            last_hurt: 0.0,
            no_action_time: 0,
            invulnerable_time: 0,
            last_hurt_by_mob_timestamp: 0,
            last_hurt_by_player_memory_time: 0,
            effects_dirty: true,
            dead: false,
            death_time: 0,
            fall_fly_ticks: 0,
            auto_spin_attack_ticks: 0,
            discard_friction: false,
            attributes,
            last_pos: None,
            ambient_sound_time: 0,
            persistence_required: false,
            can_pick_up_loot: false,
            pathfinding_malus: Vec::new(),
            target: None,
            last_hurt_by_mob: None,
            last_hurt_mob: None,
            home_radius: -1,
            age: 0,
            forced_age: 0,
            forced_age_timer: 0,
            age_lock_particle_timer: 0,
            in_love: 0,
            love_cause: None,
            leash: None,
            home_position: BlockPos { x: 0, y: 0, z: 0 },
            panic_goal: None,
            chicken: None,
            target_position: None,
            walk: crate::step::Walk::new(),
            golem: super::golem::GolemState::new(),
            stand_last_hit: 0,
        };
        // `LivingEntity.<init>` 149-154: `setHealth(getMaxHealth())`.
        let max = m.max_health();
        m.set_health(max);
        // 170-171: `reapplyPosition()` — `setPos(0, 0, 0)`, already the box.
        // 174-197: the yaw draw, head rotation follows.
        let yaw = m.random.next_f32() * 6.283_185_5_f32;
        m.set_y_rot_raw(yaw);
        m.y_head_rot = m.y_rot;
        // The kind's own constructor.
        match kind.data {
            super::DataLayout::Pig
            | super::DataLayout::Cow
            | super::DataLayout::Mooshroom
            | super::DataLayout::Sheep
            | super::DataLayout::Chicken => {
                // `Animal.<init>` 11-26: two pathfinding maluses.
                m.set_pathfinding_malus_raw(PathType::FireInNeighbor, 16.0);
                m.set_pathfinding_malus_raw(PathType::Fire, -1.0);
                // `Chicken.<init>` 7-46: its fields, the egg timer's draw,
                // and `setPathfindingMalus(WATER, 0F)`.
                if let super::DataLayout::Chicken = kind.data {
                    super::chicken::construct(&mut m);
                }
            }
            super::DataLayout::Bat => {
                // `Bat.<init>` 28-37: `if (!level.isClientSide) setResting(true)`.
                super::bat::set_resting(&mut m, true);
            }
            // `IronGolem.<init>` and `SnowGolem.<init>` are `super(...)` and
            // `return`; `AbstractGolem.<init>` likewise. `ArmorStand.<init>`
            // sets two plain fields (`invisible`, `disabledSlots`) to false
            // and 0, neither synched.
            super::DataLayout::IronGolem
            | super::DataLayout::SnowGolem
            | super::DataLayout::ArmorStand => {}
            // `CopperGolem.<init>` 77-143: navigation path length and door
            // flag (the navigation is the host's), `setPersistenceRequired`,
            // `setState(IDLE)` (the default — no write), three maluses, and
            // the brain's `TRANSPORT_ITEMS_COOLDOWN_TICKS = nextInt(60,
            // 100)`, drawn from the golem's own random even though the brain
            // that would read it is not ported, so the stream stays in step.
            super::DataLayout::CopperGolem => {
                m.persistence_required = true;
                m.set_pathfinding_malus_raw(PathType::FireInNeighbor, 16.0);
                m.set_pathfinding_malus_raw(PathType::DamagingInNeighbor, 16.0);
                m.set_pathfinding_malus_raw(PathType::Fire, -1.0);
                // `nextInt(int, int)` is `nextInt(max - min) + min`.
                let _cooldown = m.random.next_i32_bound(40) + 60;
            }
        }
        Some(m)
    }

    /// `Entity.getEyeHeight()`: the dimensions' eye height (`Pig`'s baby
    /// dimensions have their own).
    #[must_use]
    pub fn eye_height(&self) -> f32 {
        if self.is_baby() {
            self.kind.baby_eye_height.unwrap_or(self.kind.eye_height)
        } else {
            self.kind.eye_height
        }
    }

    /// `EntityDimensions` in force: `getDimensions(pose)` —
    /// `getDefaultDimensions(pose).scale(getScale())`, with `SCALE` at
    /// its default of 1 and `Pig.getDefaultDimensions` substituting the
    /// baby box.
    #[must_use]
    pub fn dimensions(&self) -> (f32, f32) {
        if self.is_baby() {
            self.kind
                .baby_dimensions
                .unwrap_or((self.kind.width, self.kind.height))
        } else {
            (self.kind.width, self.kind.height)
        }
    }

    /// The movement profile for `Entity.move`.
    #[must_use]
    pub fn profile(&self) -> Profile {
        let (w, h) = self.dimensions();
        Profile {
            width: f64::from(w),
            height: f64::from(h),
            max_up_step: self.max_up_step(),
            gravity: self.attribute_value("minecraft:gravity"),
            air_drag: 0.98,
            entity_bounciness: self.attribute_value("minecraft:bounciness"),
            client_authoritative: false,
            // `Entity.canSimulateMovement()` is `isLocalInstanceAuthoritative()`
            // for a mob, which is true on the server.
            can_simulate_movement: true,
            living: true,
        }
    }

    /// `getAttributeValue(holder)`.
    #[must_use]
    pub fn attribute_value(&self, attribute: &str) -> f64 {
        self.attributes
            .value(attribute)
            .unwrap_or_else(|| panic!("{} has no attribute {attribute}", self.kind.name))
    }

    /// `getMaxHealth()`: `(float) getAttributeValue(MAX_HEALTH)`.
    #[must_use]
    pub fn max_health(&self) -> f32 {
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        {
            self.attribute_value(MAX_HEALTH) as f32
        }
    }

    /// `getHealth()`.
    #[must_use]
    pub fn health(&self) -> f32 {
        self.data.get_f32(data_id::HEALTH)
    }

    /// `setHealth(float)`: `entityData.set(DATA_HEALTH_ID, Mth.clamp(v, 0, getMaxHealth()))`.
    pub fn set_health(&mut self, v: f32) {
        let max = self.max_health();
        let clamped = if v < 0.0 {
            0.0
        } else if v > max {
            max
        } else {
            v
        };
        self.data.set_f32(data_id::HEALTH, clamped);
    }

    /// `isDeadOrDying()`: `getHealth() <= 0`.
    #[must_use]
    pub fn is_dead_or_dying(&self) -> bool {
        self.health() <= 0.0
    }

    /// `setYRot(float)`: refused when not finite.
    pub fn set_y_rot_raw(&mut self, v: f32) {
        if v.is_finite() {
            self.y_rot = v;
        }
    }

    /// `setXRot(float)`: `clamp(v % 360, -90, 90)`.
    pub fn set_x_rot_raw(&mut self, v: f32) {
        if v.is_finite() {
            self.x_rot = (v % 360.0).clamp(-90.0, 90.0);
        }
    }

    /// `setPosRaw(x, y, z)`: the position, without the bounding box (which
    /// this host derives on read).
    pub fn set_pos_raw(&mut self, x: f64, y: f64, z: f64) {
        self.body.pos = crate::collide::Vec3::new(x, y, z);
    }

    /// `snapTo(x, y, z, yRot, xRot)`: `setPosRaw`, both rotations,
    /// `setOldPosAndRot`, `reapplyPosition`.
    pub fn snap_to(&mut self, x: f64, y: f64, z: f64, y_rot: f32, x_rot: f32) {
        self.set_pos_raw(x, y, z);
        self.set_y_rot_raw(y_rot);
        self.set_x_rot_raw(x_rot);
        self.set_old_pos_and_rot();
        // `reapplyPosition`: `lastKnownPosition = null; setPos(position)`.
        self.last_known_position = None;
    }

    /// `setOldPosAndRot()`.
    pub fn set_old_pos_and_rot(&mut self) {
        self.old_pos = (self.body.pos.x, self.body.pos.y, self.body.pos.z);
        self.y_rot_o = self.y_rot;
        self.x_rot_o = self.x_rot;
    }

    /// `discard()`.
    pub fn discard(&mut self) {
        self.removed = true;
    }

    /// `Mob.setPathfindingMalus(PathType, float)`.
    pub fn set_pathfinding_malus_raw(&mut self, t: PathType, f: f32) {
        if let Some(e) = self.pathfinding_malus.iter_mut().find(|(k, _)| *k == t) {
            e.1 = f;
        } else {
            self.pathfinding_malus.push((t, f));
        }
    }

    /// `Mob.setLeftHanded(boolean)`: bit 2 of `DATA_MOB_FLAGS_ID`.
    pub fn set_left_handed(&mut self, v: bool) {
        let b = self.data.get_byte(data_id::MOB_FLAGS);
        let n = if v {
            b | MOB_FLAG_LEFT_HANDED
        } else {
            b & !MOB_FLAG_LEFT_HANDED
        };
        self.data.set_byte(data_id::MOB_FLAGS, n);
    }

    /// `AgeableMob.setAge(int)`: the field, `DATA_BABY_ID` when the sign
    /// changes, `ageBoundaryReached()` (a no-op for a mob not in a boat).
    pub fn set_age(&mut self, v: i32) {
        let was_baby = self.age < 0;
        self.age = v;
        let is_baby = v < 0;
        if was_baby != is_baby {
            self.data.set_bool(data_id::BABY, is_baby);
        }
    }

    /// `PathfinderMob.isPanicking()` on this mob's own selector: its
    /// `PanicGoal` is running.
    #[must_use]
    pub fn is_panicking_now(
        &self,
        selector: &rustmc_entity::ai::goal_selector::GoalSelector<
            rustmc_entity::ai::mob_ai::MobAi<ServerMob>,
        >,
    ) -> bool {
        self.panic_goal.is_some_and(|id| {
            selector
                .available_goals()
                .any(|(g, w)| g == id && w.is_running())
        })
    }

    /// `AgeableMob.getAge()` on the server: the field.
    #[must_use]
    pub fn get_age(&self) -> i32 {
        self.age
    }

    /// A snapshot of another entity in this level, as the goals read it.
    #[must_use]
    pub fn facts_of(&self, id: i32) -> Option<EntityFacts> {
        if id == self.id {
            return Some(self.own_facts());
        }
        super::facts_of_entity(id)
    }

    /// This mob's own `EntityFacts`.
    #[must_use]
    pub fn own_facts(&self) -> EntityFacts {
        EntityFacts {
            id: self.id,
            x: self.x(),
            y: self.y(),
            z: self.z(),
            eye_y: self.eye_y(),
            bb: self.bounding_box(),
            x_rot: self.x_rot,
            y_rot: self.y_rot,
            alive: !self.removed && !self.is_dead_or_dying(),
            is_player: false,
            is_spectator: false,
            is_creative: false,
            is_invulnerable: false,
            is_ghast: false,
            vehicle: None,
            age: self.age,
            in_love: self.in_love > 0,
            panicking: false,
            target: self.target,
        }
    }

    /// `EntitySelector.NO_SPECTATORS`-shaped hands lookup for
    /// `TemptGoal`: the player's main and off hand items, or `None` for an
    /// entity that is not a tracked player.
    fn hands_of(id: i32) -> Option<PlayerHands> {
        super::player_hands(id)
    }
}

impl MobHost for ServerMob {
    type Level = MobLevel;

    fn level(&self) -> &MobLevel {
        &self.level
    }

    fn id(&self) -> i32 {
        self.id
    }

    fn tick_count(&self) -> i32 {
        self.tick_count
    }

    fn random(&mut self) -> &mut LegacyRandomSource {
        &mut self.random
    }

    fn x(&self) -> f64 {
        self.body.pos.x
    }
    fn y(&self) -> f64 {
        self.body.pos.y
    }
    fn z(&self) -> f64 {
        self.body.pos.z
    }
    fn xo(&self) -> f64 {
        self.old_pos.0
    }
    fn zo(&self) -> f64 {
        self.old_pos.2
    }

    fn eye_y(&self) -> f64 {
        self.body.pos.y + f64::from(self.eye_height())
    }

    fn bounding_box(&self) -> Aabb {
        let (w, h) = self.dimensions();
        let half = f64::from(w / 2.0);
        let p = self.body.pos;
        Aabb::new(
            p.x - half,
            p.y,
            p.z - half,
            p.x + half,
            p.y + f64::from(h),
            p.z + half,
        )
    }

    fn bb_width(&self) -> f32 {
        self.dimensions().0
    }
    fn bb_height(&self) -> f32 {
        self.dimensions().1
    }

    fn y_rot(&self) -> f32 {
        self.y_rot
    }
    fn set_y_rot(&mut self, v: f32) {
        self.set_y_rot_raw(v);
    }
    fn x_rot(&self) -> f32 {
        self.x_rot
    }
    fn set_x_rot(&mut self, v: f32) {
        self.set_x_rot_raw(v);
    }
    fn y_head_rot(&self) -> f32 {
        self.y_head_rot
    }
    fn set_y_head_rot(&mut self, v: f32) {
        self.y_head_rot = v;
    }
    fn y_body_rot(&self) -> f32 {
        self.y_body_rot
    }
    fn set_y_body_rot(&mut self, v: f32) {
        self.y_body_rot = v;
    }

    fn speed(&self) -> f32 {
        self.speed
    }
    fn set_speed_field(&mut self, v: f32) {
        self.speed = v;
    }
    fn set_zza(&mut self, v: f32) {
        self.zza = v;
    }
    fn set_xxa(&mut self, v: f32) {
        self.xxa = v;
    }
    fn set_yya(&mut self, v: f32) {
        self.yya = v;
    }
    fn set_jumping(&mut self, v: bool) {
        self.jumping = v;
    }

    fn on_ground(&self) -> bool {
        self.body.on_ground
    }

    fn is_in_water(&self) -> bool {
        self.was_touching_water
    }

    fn is_in_lava(&self) -> bool {
        !self.first_tick && self.fluid_height[1] > 0.0
    }

    fn is_in_liquid(&self) -> bool {
        self.is_in_water() || self.is_in_lava()
    }

    fn is_passenger(&self) -> bool {
        false
    }

    fn is_affected_by_fluids(&self) -> bool {
        true
    }

    fn is_on_fire(&self) -> bool {
        // `!fireImmune() && remainingFireTicks > 0` on the server.
        self.remaining_fire_ticks > 0
    }

    fn water_fluid_height(&self) -> f64 {
        self.fluid_height[0]
    }

    fn fluid_jump_threshold(&self) -> f64 {
        // `getEyeHeight() < 0.4F ? 0.0 : 0.4`.
        if self.eye_height() < 0.4 {
            0.0
        } else {
            0.4
        }
    }

    fn max_up_step(&self) -> f32 {
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
        {
            self.attribute_value("minecraft:step_height") as f32
        }
    }

    fn max_fall_distance(&self) -> i32 {
        // `Mob.getMaxFallDistance`, offsets 0-57.
        let health = if self.target.is_none() {
            0.0
        } else {
            #[expect(clippy::cast_possible_truncation, reason = "vanilla's own f2i")]
            let mut i = (self.health() - self.max_health() * 0.33) as i32;
            i -= (3 - super::difficulty_id()) * 4;
            if i < 0 {
                i = 0;
            }
            #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
            {
                i as f32
            }
        };
        // `getComfortableFallDistance(float)`: `Mth.floor(f + 3.0F)`.
        #[expect(clippy::cast_possible_truncation, reason = "Mth.floor")]
        {
            (health + 3.0).floor() as i32
        }
    }

    fn movement_speed(&self) -> f64 {
        self.attribute_value(MOVEMENT_SPEED)
    }

    fn follow_range(&self) -> f64 {
        self.attribute_value(FOLLOW_RANGE)
    }

    fn follow_range_base(&self) -> f64 {
        self.attributes.base_value(FOLLOW_RANGE).unwrap_or(0.0)
    }

    fn pathfinding_malus(&self, t: PathType) -> f32 {
        self.pathfinding_malus
            .iter()
            .find(|(k, _)| *k == t)
            .map_or_else(|| t.malus(), |(_, f)| *f)
    }

    fn set_pathfinding_malus(&mut self, t: PathType, f: f32) {
        self.set_pathfinding_malus_raw(t, f);
    }

    fn controlling_passenger_is_mob(&self) -> bool {
        false
    }
    fn has_controlling_passenger(&self) -> bool {
        false
    }
    fn vehicle_is_boat(&self) -> bool {
        false
    }
    fn first_passenger_is_mob(&self) -> bool {
        false
    }

    fn home_radius(&self) -> i32 {
        self.home_radius
    }

    fn home_position(&self) -> BlockPos {
        self.home_position
    }

    fn walk_target_value(&self, pos: BlockPos) -> f32 {
        match self.kind.data {
            // `Animal.getWalkTargetValue(pos, level)`: 10 on grass, else
            // `level.getPathfindingCostFromLightLevels(pos)`;
            // `MushroomCow.getWalkTargetValue`: the same on mycelium.
            super::DataLayout::Pig
            | super::DataLayout::Cow
            | super::DataLayout::Mooshroom
            | super::DataLayout::Sheep
            | super::DataLayout::Chicken => {
                let below = BlockPos {
                    x: pos.x,
                    y: pos.y - 1,
                    z: pos.z,
                };
                let favoured = if let super::DataLayout::Mooshroom = self.kind.data {
                    "minecraft:mycelium"
                } else {
                    "minecraft:grass_block"
                };
                if self.level.block_name(below) == Some(favoured) {
                    10.0
                } else {
                    super::level::pathfinding_cost_from_light_levels(&self.level, pos)
                }
            }
            // `PathfinderMob.getWalkTargetValue`'s `0.0f` is `Mob`'s too, and
            // no golem overrides it.
            super::DataLayout::Bat
            | super::DataLayout::IronGolem
            | super::DataLayout::SnowGolem
            | super::DataLayout::CopperGolem
            | super::DataLayout::ArmorStand => 0.0,
        }
    }

    fn has_line_of_sight_to(&self, entity_id: i32) -> bool {
        // `LivingEntity.hasLineOfSight(Entity)`, offsets 0-104: same
        // level, eye to eye, refused past 128 blocks, then `Level.clip`
        // with COLLIDER / NONE.
        let Some(other) = self.facts_of(entity_id) else {
            return false;
        };
        let from = Vec3::new(self.x(), self.eye_y(), self.z());
        let to = Vec3::new(other.x, other.eye_y, other.z);
        if to.distance_to(from) > 128.0 {
            return false;
        }
        rustmc_entity::host::PathLevel::clip_misses(&self.level, self.id, from, to, false)
    }

    fn entity(&self, id: i32) -> Option<EntityFacts> {
        self.facts_of(id)
    }

    fn players(&self) -> Vec<i32> {
        super::player_ids_in_join_order()
    }

    fn entities_of_class_in(&self, class: EntityClass, aabb: &Aabb) -> Vec<i32> {
        super::entities_of_class_in(self.id, self.kind, class, aabb)
    }

    fn difficulty_is_peaceful(&self) -> bool {
        super::difficulty() == Difficulty::Peaceful
    }

    fn difficulty(&self) -> Difficulty {
        super::difficulty()
    }

    fn mob_griefing(&self) -> bool {
        crate::gamerules::GameRules::shared()
            .get("mob_griefing")
            .is_some_and(|v| v.serialize() == "true")
    }

    fn target_raw(&self) -> Option<i32> {
        self.target
    }

    fn set_target_raw(&mut self, id: Option<i32>) {
        self.target = id;
    }

    fn last_hurt_by_mob(&self) -> Option<i32> {
        self.last_hurt_by_mob
    }

    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.last_hurt_by_mob_timestamp
    }

    fn is_in_love(&self) -> bool {
        self.in_love > 0
    }

    fn age(&self) -> i32 {
        self.age
    }

    fn can_mate_with(&self, id: i32) -> bool {
        super::breed::can_mate(self, id)
    }

    fn spawn_child_from_breeding(&mut self, partner: i32) {
        super::breed::spawn_child_from_breeding(self, partner);
    }

    fn is_baby(&self) -> bool {
        // `AgeableMob.isBaby()`: `canBeABaby() && getAge() < 0`; a bat is
        // not ageable.
        self.kind.data.is_animal() && self.age < 0
    }

    fn tempt_range(&self) -> f64 {
        self.attribute_value(TEMPT_RANGE)
    }

    fn holds_tempting_item(&self, id: i32) -> bool {
        // `Pig.registerGoals`' two `TemptGoal`s: `stack.is(CARROT_ON_A_STICK)`
        // and `stack.is(ItemTags.PIG_FOOD)`. Both goals share one predicate
        // slot in the host, so the union is what a pig answers; the two
        // goals sit at the same priority and the first that `canUse`s wins
        // in both readings.
        let Some(hands) = Self::hands_of(id) else {
            return false;
        };
        let tempting = |item: &Option<String>| {
            item.as_deref()
                .is_some_and(|i| self.kind.is_tempting_item(i))
        };
        tempting(&hands.main) || tempting(&hands.off)
    }

    fn delta_movement(&self) -> Vec3 {
        Vec3::new(
            self.body.movement.x,
            self.body.movement.y,
            self.body.movement.z,
        )
    }

    fn set_delta_movement(&mut self, x: f64, y: f64, z: f64) {
        self.body.movement = crate::collide::Vec3::new(x, y, z);
    }

    fn move_by_delta_movement(&mut self) {
        super::living::move_self(self);
    }

    fn was_in_powder_snow(&self) -> bool {
        self.was_in_powder_snow
    }

    fn is_in_powder_snow(&self) -> bool {
        self.is_in_powder_snow
    }

    fn is_powder_snow_walkable(&self) -> bool {
        // `PowderSnowBlock.canEntityWalkOnPowderSnow`: leather boots, or
        // a type in `POWDER_SNOW_WALKABLE_MOBS`; neither here.
        false
    }

    fn xxa(&self) -> f32 {
        self.xxa
    }
    fn yya(&self) -> f32 {
        self.yya
    }
    fn zza(&self) -> f32 {
        self.zza
    }

    fn set_fall_distance(&mut self, d: f64) {
        self.fall_distance = d;
    }

    fn x_rot_o(&self) -> f32 {
        self.x_rot_o
    }
    fn y_rot_o(&self) -> f32 {
        self.y_rot_o
    }

    fn is_alive(&self) -> bool {
        !self.removed && self.health() > 0.0
    }

    fn hurt_marked(&self) -> bool {
        self.hurt_marked
    }

    fn day_time(&self) -> i64 {
        self.level.day_time
    }

    fn level_random_next_float(&mut self) -> f32 {
        super::level_random(|r| r.next_f32())
    }

    fn level_random_next_int(&mut self, bound: i32) -> i32 {
        super::level_random(|r| r.next_i32_bound(bound))
    }

    fn can_pick_up_loot(&self) -> bool {
        self.can_pick_up_loot
    }

    fn entity_in_water(&self, id: i32) -> bool {
        super::entity_in_water(id)
    }

    fn entity_is_living(&self, id: i32) -> bool {
        self.facts_of(id).is_some_and(|f| f.is_player) || super::is_mob(id)
    }

    fn entity_in_same_level(&self, id: i32) -> bool {
        self.facts_of(id).is_some()
    }

    fn entity_type_matches_mine(&self, id: i32) -> bool {
        super::mob_kind_name(id) == Some(self.kind.name)
    }

    fn entity_is_exactly(&self, id: i32, class: EntityClass) -> bool {
        super::entity_class_test(id, class, true)
    }

    fn entity_is_assignable_to(&self, id: i32, class: EntityClass) -> bool {
        super::entity_class_test(id, class, false)
    }

    fn can_stand_on_fluid(&self, _facts: &BlockFacts) -> bool {
        false
    }

    fn broadcast_entity_event(&mut self, id: u8) {
        // `ServerLevel.broadcastEntityEvent(this, id)` — `EatBlockGoal.start`'s
        // 10, the sheep lowering its head.
        super::breed::broadcast_entity_event(self.id, id);
    }

    fn ate(&mut self) {
        // `Mob.ate()` is a game event; `Sheep.ate` is the one override here.
        if let super::DataLayout::Sheep = self.kind.data {
            super::sheep::ate(self);
        }
    }

    fn destroy_block(&mut self, pos: BlockPos) {
        super::sheep::destroy_block(self, pos);
    }

    fn turn_grass_to_dirt(&mut self, pos: BlockPos) {
        super::sheep::turn_grass_to_dirt(self, pos);
    }
}

/// A `Modifier` for `Mob.finalizeSpawn`'s follow-range bonus.
#[must_use]
pub fn random_spawn_bonus(amount: f64) -> Modifier {
    Modifier {
        id: super::spawn::RANDOM_SPAWN_BONUS_ID.to_string(),
        amount,
        op: crate::attributes::Operation::AddMultipliedBase,
    }
}
