//! What the AI asks of the mob and of the level: the `Mob` / `LivingEntity`
//! / `Entity` calls and the `Level` / `BlockGetter` calls the transcribed
//! classes make, as traits the caller answers.
//!
//! # Why traits, and why these two
//!
//! Every class in `net.minecraft.world.entity.ai` holds a `Mob mob` and
//! reaches through it: `mob.getX()`, `mob.getRandom()`, `mob.level()
//! .getBlockState(pos).is(BlockTags.FENCES)`. There are no mobs in this
//! server and the block-state model lives in other crates, so this crate
//! writes the AI against two traits and lets whoever owns an entity's fields
//! and a level answer them. Every method is one Java call, named for it,
//! and where the Java base class has a body the trait has that body as its
//! default.
//!
//! The split is the jar's own: [`MobHost`] is `Mob` and its superclasses;
//! [`PathLevel`] is the `Level`/`CollisionGetter`/`BlockGetter` surface the
//! pathfinder and the navigation read.
//!
//! # `BlockFacts`
//!
//! `WalkNodeEvaluator.getPathTypeFromState` classifies a block by a chain
//! of `is(Block)`, `is(TagKey)` and `instanceof` tests. That chain is
//! transcribed in the pathfinder; what the level answers is the raw facts
//! it tests, one bit each, in [`BlockFacts`]. A test world answers them
//! from a small grid; the server answers them from its registry.

use rustmc_data::{BlockPos, Dimension};

use crate::brain::activity::Activity;
use crate::brain::behaviors::MobCategory;
use crate::brain::memory::{DamageSourceFacts, GlobalPos, MemoryModuleType};
use crate::brain::schedule::Schedule;
use rustmc_worldgen::rng::LegacyRandomSource;

use crate::pathfinder::PathType;
use crate::phys::{Aabb, Vec3};

/// The block-state facts `getPathTypeFromState`, `isBurningBlock` and the
/// navigation's own block tests ask for, in the order the jar tests them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockFacts {
    /// `BlockState.isAir()`.
    pub is_air: bool,
    /// `is(BlockTags.TRAPDOORS)`.
    pub is_trapdoor: bool,
    /// `is(Blocks.LILY_PAD)`.
    pub is_lily_pad: bool,
    /// `is(Blocks.BIG_DRIPLEAF)`.
    pub is_big_dripleaf: bool,
    /// `is(Blocks.POWDER_SNOW)`.
    pub is_powder_snow: bool,
    /// `is(Blocks.CACTUS)`.
    pub is_cactus: bool,
    /// `is(Blocks.SWEET_BERRY_BUSH)`.
    pub is_sweet_berry_bush: bool,
    /// `is(Blocks.HONEY_BLOCK)`.
    pub is_honey_block: bool,
    /// `is(Blocks.COCOA)`.
    pub is_cocoa: bool,
    /// `is(Blocks.WITHER_ROSE)`.
    pub is_wither_rose: bool,
    /// `is(BlockTags.SPELEOTHEMS)`.
    pub is_speleothem: bool,
    /// `getFluidState().is(FluidTags.LAVA)`.
    pub fluid_is_lava: bool,
    /// `getFluidState().is(FluidTags.WATER)`.
    pub fluid_is_water: bool,
    /// `getFluidState()` is `Fluids.WATER.getSource(false)` — a source
    /// block of still water, which `WalkNodeEvaluator.getStart` tests
    /// separately from `is(Blocks.WATER)`.
    pub fluid_is_water_source: bool,
    /// `is(Blocks.WATER)`.
    pub is_water_block: bool,
    /// `is(BlockTags.FIRE)`.
    pub is_fire: bool,
    /// `is(Blocks.LAVA)`.
    pub is_lava_block: bool,
    /// `is(Blocks.MAGMA_BLOCK)`.
    pub is_magma_block: bool,
    /// `CampfireBlock.isLitCampfire(state)`.
    pub is_lit_campfire: bool,
    /// `is(Blocks.LAVA_CAULDRON)`.
    pub is_lava_cauldron: bool,
    /// `getBlock() instanceof DoorBlock`, with `OPEN` and
    /// `type().canOpenByHand()` when it is.
    pub door: Option<DoorFacts>,
    /// `getBlock() instanceof BaseRailBlock`.
    pub is_rail: bool,
    /// `getBlock() instanceof LeavesBlock`.
    pub is_leaves: bool,
    /// `is(BlockTags.FENCES)`.
    pub is_fence: bool,
    /// `is(BlockTags.WALLS)`.
    pub is_wall: bool,
    /// `getBlock() instanceof FenceGateBlock`, with its `OPEN` value.
    pub fence_gate_open: Option<bool>,
    /// `isPathfindable(PathComputationType.LAND)`.
    pub is_pathfindable_land: bool,
    /// `isPathfindable(PathComputationType.WATER)`.
    pub is_pathfindable_water: bool,
    /// The block's registry id (`minecraft:` dropped), for `is(Block)`
    /// tests against a block a goal is built with (`RemoveBlockGoal`).
    pub block_id: Option<&'static str>,
    /// `is(BlockTags.BEDS)`, and `BedBlock.PART == HEAD` when it is one.
    pub is_bed: bool,
    pub bed_part_head: bool,
    /// `is(Blocks.CHEST)` with `ChestBlockEntity.getOpenCount`.
    pub chest_open_count: Option<i32>,
    /// `is(Blocks.FURNACE)` with `FurnaceBlock.LIT`.
    pub furnace_lit: Option<bool>,
    /// `is(BlockTags.EDIBLE_FOR_SHEEP)`.
    pub is_edible_for_sheep: bool,
    /// `is(Blocks.GRASS_BLOCK)`.
    pub is_grass_block: bool,
    /// `BlockState.blocksMotion()`.
    pub blocks_motion: bool,
    /// `BlockState.isSolid()`.
    pub is_solid: bool,
    /// `BlockState.isSolidRender()`.
    pub is_solid_render: bool,
    /// `is(BlockTags.CAULDRONS)`.
    pub is_cauldron: bool,
    /// `is(BlockTags.DOORS)`.
    pub is_door_tag: bool,
    /// `is(BlockTags.MOB_INTERACTABLE_DOORS)` — the doors a brain mob's
    /// `InteractWithDoor` opens and closes (wooden doors; not iron).
    pub is_mob_interactable_door: bool,
}

/// `DoorBlock` facts: `state.getValue(DoorBlock.OPEN)` and
/// `door.type().canOpenByHand()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoorFacts {
    pub open: bool,
    pub can_open_by_hand: bool,
}

impl BlockFacts {
    /// A plain air block: everything false except `is_air` and
    /// `is_pathfindable_land`.
    pub const AIR: BlockFacts = BlockFacts {
        is_air: true,
        is_pathfindable_land: true,
        ..BlockFacts::NONE
    };

    /// All-false, for building the others from.
    pub const NONE: BlockFacts = BlockFacts {
        is_air: false,
        is_trapdoor: false,
        is_lily_pad: false,
        is_big_dripleaf: false,
        is_powder_snow: false,
        is_cactus: false,
        is_sweet_berry_bush: false,
        is_honey_block: false,
        is_cocoa: false,
        is_wither_rose: false,
        is_speleothem: false,
        fluid_is_lava: false,
        fluid_is_water: false,
        fluid_is_water_source: false,
        is_water_block: false,
        is_fire: false,
        is_lava_block: false,
        is_magma_block: false,
        is_lit_campfire: false,
        is_lava_cauldron: false,
        door: None,
        is_rail: false,
        is_leaves: false,
        is_fence: false,
        is_wall: false,
        fence_gate_open: None,
        is_pathfindable_land: false,
        is_pathfindable_water: false,
        block_id: None,
        is_bed: false,
        bed_part_head: false,
        chest_open_count: None,
        furnace_lit: None,
        is_edible_for_sheep: false,
        is_grass_block: false,
        blocks_motion: false,
        is_solid: false,
        is_solid_render: false,
        is_cauldron: false,
        is_door_tag: false,
        is_mob_interactable_door: false,
    };

    /// A full solid cube (stone): not pathfindable on land, solid, solid
    /// render.
    pub const STONE: BlockFacts = BlockFacts {
        is_solid: true,
        is_solid_render: true,
        ..BlockFacts::NONE
    };

    /// A still-water source block.
    pub const WATER: BlockFacts = BlockFacts {
        fluid_is_water: true,
        fluid_is_water_source: true,
        is_water_block: true,
        is_pathfindable_land: true,
        is_pathfindable_water: true,
        ..BlockFacts::NONE
    };
}

/// The `Class<? extends Entity>` arguments the goals pass to
/// `getEntitiesOfClass` and friends, as the host understands them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityClass {
    /// `Player.class` / `ServerPlayer.class`.
    Player,
    /// `LivingEntity.class`.
    Living,
    /// `Mob.class`.
    Mob,
    /// `Animal.class`.
    Animal,
    /// `mob.getClass()` — the same concrete kind as the asking mob.
    SameKind,
    /// `Villager.class`.
    Villager,
    /// `ItemEntity.class`.
    Item,
}

/// A snapshot of another entity, as the AI reads it. Every field is one
/// `Entity`/`LivingEntity`/`Player`/`Animal` call the goals make on a
/// target, taken at the moment the host is asked.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityFacts {
    /// `getId()`.
    pub id: i32,
    /// `getX()`, `getY()`, `getZ()`.
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `getEyeY()`.
    pub eye_y: f64,
    /// `getBoundingBox()`.
    pub bb: Aabb,
    /// `getXRot()`, `getYRot()`.
    pub x_rot: f32,
    pub y_rot: f32,
    /// `isAlive()`.
    pub alive: bool,
    /// `instanceof Player`.
    pub is_player: bool,
    /// `isSpectator()`.
    pub is_spectator: bool,
    /// `Player.isCreative()`.
    pub is_creative: bool,
    /// `isInvulnerable()`.
    pub is_invulnerable: bool,
    /// `is(EntityTypes.GHAST)`.
    pub is_ghast: bool,
    /// `isPassenger()` and `getVehicle().getId()`.
    pub vehicle: Option<i32>,
    /// `Animal.getAge()`; `0` for a non-animal.
    pub age: i32,
    /// `Animal.isInLove()`.
    pub in_love: bool,
    /// `PathfinderMob.isPanicking()`.
    pub panicking: bool,
    /// `Mob.getTarget()`'s id, for `HurtByTargetGoal.alertOthers`.
    pub target: Option<i32>,
}

impl EntityFacts {
    /// `blockPosition()`.
    #[must_use]
    pub fn block_position(&self) -> BlockPos {
        crate::phys::block_pos_containing(self.x, self.y, self.z)
    }

    /// `position()`.
    #[must_use]
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }

    /// `LivingEntity.canBeSeenByAnyone()`: `!isSpectator() && isAlive()`.
    #[must_use]
    pub fn can_be_seen_by_anyone(&self) -> bool {
        !self.is_spectator && self.alive
    }

    /// `LivingEntity.canBeSeenAsEnemy()`: `!isInvulnerable() && canBeSeenByAnyone()`.
    #[must_use]
    pub fn can_be_seen_as_enemy(&self) -> bool {
        !self.is_invulnerable && self.can_be_seen_by_anyone()
    }

    /// `distanceToSqr(double, double, double)` from this entity.
    #[must_use]
    pub fn distance_to_sqr(&self, x: f64, y: f64, z: f64) -> f64 {
        let dx = self.x - x;
        let dy = self.y - y;
        let dz = self.z - z;
        dx * dx + dy * dy + dz * dz
    }

    /// `EntitySelector.NO_CREATIVE_OR_SPECTATOR`: a non-player, or a
    /// player neither spectating nor creative.
    #[must_use]
    pub fn no_creative_or_spectator(&self) -> bool {
        !self.is_player || (!self.is_spectator && !self.is_creative)
    }

    /// `EntitySelector.NO_SPECTATORS`.
    #[must_use]
    pub fn no_spectators(&self) -> bool {
        !self.is_spectator
    }
}

/// The `Level` / `CollisionGetter` / `BlockGetter` surface the AI reads.
pub trait PathLevel {
    /// `getBlockState(pos)` reduced to the facts the AI tests.
    fn block_facts(&self, pos: BlockPos) -> BlockFacts;

    /// `getMinY()`.
    fn min_y(&self) -> i32;

    /// `getMaxY()`.
    fn max_y(&self) -> i32;

    /// `isOutsideBuildHeight(int)`: `y < minY || y > maxY`.
    fn is_outside_build_height(&self, y: i32) -> bool {
        y < self.min_y() || y > self.max_y()
    }

    /// `getBlockState(pos).getCollisionShape(level, pos)`: `None` when the
    /// shape `isEmpty()`, otherwise its `max(Direction.Axis.Y)`.
    fn collision_shape_max_y(&self, pos: BlockPos) -> Option<f64>;

    /// `getGameTime()`.
    fn game_time(&self) -> i64;

    /// `Level.isBrightOutside()`: `!dimensionType().hasFixedTime() && skyDarken < 4`.
    fn is_bright_outside(&self) -> bool {
        false
    }

    /// `canSeeSky(pos)`.
    fn can_see_sky(&self, pos: BlockPos) -> bool;

    /// `getChunkSource().getChunkNow(cx, cz) != null`.
    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// `CollisionGetter.noCollision(entity, aabb)`.
    fn no_collision(&self, mob_id: i32, aabb: &Aabb) -> bool;

    /// `level.clip(new ClipContext(from, to, COLLIDER, fluids ? ANY : NONE,
    /// mob)).getType() == MISS`.
    fn clip_misses(&self, mob_id: i32, from: Vec3, to: Vec3, fluids: bool) -> bool;

    /// `ServerLevel.getPathTypeCache()`: the level's cache, when it keeps
    /// one. `None` is the non-server branch of `PathfindingContext`'s
    /// constructor and gives the same answers.
    fn path_type_cache(&self) -> Option<&std::cell::RefCell<crate::pathfinder::PathTypeCache>> {
        None
    }
}

/// Which `BiPredicate<PathfinderMob, ItemStack>` a [`TemptingSensor`]
/// carries. The three registered `SensorType`s differ only in this.
///
/// [`TemptingSensor`]: crate::brain::sensors::TemptingSensor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Temptations {
    /// `TemptingSensor.forAnimal()`:
    /// `mob instanceof Animal a && a.isFood(stack)`.
    AnimalFood,
    /// `new TemptingSensor(stack -> stack.is(tag))`, by item-tag name
    /// (`minecraft:` implied) — `frog_food`, `nautilus_food`.
    ItemTag(&'static str),
}

/// The `Mob` / `LivingEntity` / `Entity` surface the AI reads and writes.
pub trait MobHost {
    type Level: PathLevel;

    /// `level()`.
    fn level(&self) -> &Self::Level;

    /// `getId()`.
    fn id(&self) -> i32;

    /// `tickCount`.
    fn tick_count(&self) -> i32;

    /// `getRandom()` — `RandomSource.create()` in `Entity.<init>`, which is
    /// a `LegacyRandomSource`.
    fn random(&mut self) -> &mut LegacyRandomSource;

    /// `getX()`, `getY()`, `getZ()`.
    fn x(&self) -> f64;
    fn y(&self) -> f64;
    fn z(&self) -> f64;

    /// `xo`, `zo` — last tick's position, read by
    /// `BodyRotationControl.isMoving`.
    fn xo(&self) -> f64;
    fn zo(&self) -> f64;

    /// `position()`.
    fn position(&self) -> Vec3 {
        Vec3::new(self.x(), self.y(), self.z())
    }

    /// `getEyeY()`.
    fn eye_y(&self) -> f64;

    /// `getBlockY()`: `blockPosition().getY()`.
    fn block_y(&self) -> i32 {
        self.block_position().y
    }

    /// `blockPosition()`: `BlockPos.containing(x, y, z)`.
    fn block_position(&self) -> BlockPos {
        crate::phys::block_pos_containing(self.x(), self.y(), self.z())
    }

    /// `getBoundingBox()`.
    fn bounding_box(&self) -> Aabb;

    /// `getBbWidth()`, `getBbHeight()`.
    fn bb_width(&self) -> f32;
    fn bb_height(&self) -> f32;

    /// `getYRot()` / `setYRot(float)`.
    fn y_rot(&self) -> f32;
    fn set_y_rot(&mut self, v: f32);

    /// `getXRot()` / `setXRot(float)`.
    fn x_rot(&self) -> f32;
    fn set_x_rot(&mut self, v: f32);

    /// `yHeadRot` field.
    fn y_head_rot(&self) -> f32;
    fn set_y_head_rot(&mut self, v: f32);

    /// `yBodyRot` field.
    fn y_body_rot(&self) -> f32;
    fn set_y_body_rot(&mut self, v: f32);

    /// `LivingEntity.getSpeed()` / `setSpeed(float)` — the `speed` field.
    /// `Mob.setSpeed` also sets `zza`, which [`MobHost::set_speed_and_zza`]
    /// is for.
    fn speed(&self) -> f32;
    fn set_speed_field(&mut self, v: f32);

    /// `Mob.setSpeed(float)`: `super.setSpeed(v); setZza(v)`.
    fn set_speed_and_zza(&mut self, v: f32) {
        self.set_speed_field(v);
        self.set_zza(v);
    }

    /// `setZza(float)`, `setXxa(float)`, `setYya(float)`.
    fn set_zza(&mut self, v: f32);
    fn set_xxa(&mut self, v: f32);
    fn set_yya(&mut self, v: f32);

    /// `setJumping(boolean)`.
    fn set_jumping(&mut self, v: bool);

    /// `onGround()`.
    fn on_ground(&self) -> bool;

    /// `isInWater()`.
    fn is_in_water(&self) -> bool;

    /// `isInLava()`.
    fn is_in_lava(&self) -> bool;

    /// `Entity.getGravity()` — the per-tick downward acceleration.
    fn gravity(&self) -> f64 {
        0.08
    }
    /// `Mob.getJumpBoostPower()`.
    fn jump_boost_power(&self) -> f32 {
        0.0
    }
    /// `getAttributeValue(Attributes.JUMP_STRENGTH)`.
    fn jump_strength(&self) -> f64 {
        0.42
    }
    /// `Mob.getDimensions(Pose.LONG_JUMPING)`, as `(width, height)`.
    fn long_jumping_dimensions(&self) -> (f32, f32) {
        (self.bb_width(), self.bb_height())
    }
    /// `Entity.setDiscardFriction(boolean)`.
    fn set_discard_friction(&mut self, v: bool) {
        let _ = v;
    }
    /// `Entity.setPose(Pose)`.
    fn set_pose(&mut self, pose: Pose) {
        let _ = pose;
    }
    /// `WalkNodeEvaluator.getPathTypeStatic(mob, pos)`.
    fn path_type_static(&self, pos: BlockPos) -> PathType {
        let _ = pos;
        PathType::Blocked
    }
    /// `level.getBlockState(pos).is(TagKey<Block>)` by tag name
    /// (`minecraft:` implied), e.g. `frog_prefer_jump_to`.
    fn block_tag_is(&self, pos: BlockPos, tag: &str) -> bool {
        let _ = (pos, tag);
        false
    }
    /// A caller-supplied `acceptableLandingSpot` `BiPredicate<Mob,
    /// BlockPos>` passed to `LongJumpToRandomPos`'s six-argument
    /// constructor, by name — the callers' predicates read the mob, so
    /// they cannot live in this crate.
    fn acceptable_landing_spot(&self, name: &str, pos: BlockPos) -> bool {
        let _ = (name, pos);
        false
    }

    /// `isInLiquid()`.
    fn is_in_liquid(&self) -> bool;

    /// `isPassenger()`.
    fn is_passenger(&self) -> bool;

    /// `isAffectedByFluids()`.
    fn is_affected_by_fluids(&self) -> bool;

    /// `isOnFire()`.
    fn is_on_fire(&self) -> bool;

    /// `getFluidHeight(FluidTags.WATER)`.
    fn water_fluid_height(&self) -> f64;

    /// `getFluidJumpThreshold()`.
    fn fluid_jump_threshold(&self) -> f64;

    /// `LivingEntity.maxUpStep()`: the `STEP_HEIGHT` attribute, raised to
    /// at least 1 when a player is the controlling passenger.
    fn max_up_step(&self) -> f32;

    /// `Mob.getMaxFallDistance()`.
    fn max_fall_distance(&self) -> i32;

    /// `LivingEntity.canStandOnFluid(FluidState)` — `iconst_0` in the base
    /// class; striders override.
    fn can_stand_on_fluid(&self, facts: &BlockFacts) -> bool {
        let _ = facts;
        false
    }

    /// `getAttributeValue(Attributes.MOVEMENT_SPEED)`.
    fn movement_speed(&self) -> f64;

    /// `getAttributeValue(Attributes.FOLLOW_RANGE)`.
    fn follow_range(&self) -> f64;

    /// `getAttributeBaseValue(Attributes.FOLLOW_RANGE)`.
    fn follow_range_base(&self) -> f64;

    /// `Mob.getPathfindingMalus(PathType)`: the per-mob override map, or
    /// `type.getMalus()` when there is none; read from the controlled
    /// vehicle instead when it is a mob whose passengers inherit.
    fn pathfinding_malus(&self, t: PathType) -> f32 {
        t.malus()
    }

    /// `getControllingPassenger() instanceof Mob`.
    fn controlling_passenger_is_mob(&self) -> bool;

    /// `hasControllingPassenger()`.
    fn has_controlling_passenger(&self) -> bool;

    /// `getVehicle() instanceof AbstractBoat`.
    fn vehicle_is_boat(&self) -> bool;

    /// `getFirstPassenger() instanceof Mob`.
    fn first_passenger_is_mob(&self) -> bool;

    /// `Mob.getMaxHeadXRot()`: `bipush 40`.
    fn max_head_x_rot(&self) -> i32 {
        40
    }

    /// `Mob.getMaxHeadYRot()`: `bipush 75`.
    fn max_head_y_rot(&self) -> i32 {
        75
    }

    /// `Mob.getHeadRotSpeed()`: `bipush 10`.
    fn head_rot_speed(&self) -> i32 {
        10
    }

    /// `Mob.onPathfindingStart()` / `onPathfindingDone()` — bare returns.
    fn on_pathfinding_start(&mut self) {}
    fn on_pathfinding_done(&mut self) {}

    /// `Mob.getHomeRadius()`: `-1` when the mob has no home, which is the
    /// constructor's value.
    fn home_radius(&self) -> i32 {
        -1
    }

    /// `Mob.getHomePosition()`: `BlockPos.ZERO` until restricted.
    fn home_position(&self) -> BlockPos {
        BlockPos { x: 0, y: 0, z: 0 }
    }

    /// `Mob.hasHome()`: `homeRadius != -1`.
    fn has_home(&self) -> bool {
        self.home_radius() != -1
    }

    /// `Mob.isWithinHome(BlockPos)`:
    /// `homeRadius == -1 || homePosition.distSqr(pos) < (double)(homeRadius * homeRadius)`.
    fn is_within_home_pos(&self, pos: BlockPos) -> bool {
        let r = self.home_radius();
        if r == -1 {
            return true;
        }
        crate::phys::dist_sqr(self.home_position(), pos) < f64::from(r * r)
    }

    /// `Mob.isWithinHome(Vec3)`:
    /// `homeRadius == -1 || homePosition.distToCenterSqr(v) < (double)(homeRadius * homeRadius)`.
    fn is_within_home_vec(&self, v: Vec3) -> bool {
        let r = self.home_radius();
        if r == -1 {
            return true;
        }
        crate::phys::dist_to_center_sqr(self.home_position(), v.x, v.y, v.z) < f64::from(r * r)
    }

    /// `PathfinderMob.getWalkTargetValue(BlockPos)`: `0.0f` unless the
    /// kind overrides it (`DEFAULT_WALK_TARGET_VALUE`).
    fn walk_target_value(&self, pos: BlockPos) -> f32 {
        let _ = pos;
        0.0
    }

    /// `getLastDamageSource() != null && getLastDamageSource().is(tag)`
    /// for `PanicGoal`'s damage-type tag (`DamageTypeTags.PANIC_CAUSES` by
    /// default).
    fn last_damage_source_causes_panic(&self) -> bool {
        false
    }

    /// `LivingEntity.hasLineOfSight(Entity)` toward the entity with this
    /// id, for `Sensing`.
    fn has_line_of_sight_to(&self, entity_id: i32) -> bool {
        let _ = entity_id;
        false
    }

    // ---- other entities, as the level and the mob answer about them ----

    /// The entity with this id, or `None` when it is gone
    /// (`EntityReference.getLivingEntity` returning `null`).
    fn entity(&self, id: i32) -> Option<EntityFacts> {
        let _ = id;
        None
    }

    /// `ServerLevel.players()`: every player, in the list's order.
    fn players(&self) -> Vec<i32> {
        Vec::new()
    }

    /// `Level.getEntitiesOfClass(Class, AABB, Predicate)` without the
    /// predicate (which every goal here supplies as `true` or applies
    /// itself): the ids of entities of the class whose bounding box
    /// intersects `aabb`, in the entity section storage's order — the
    /// order this crate's `lib.rs` documents as not yet vanilla's.
    fn entities_of_class_in(&self, class: EntityClass, aabb: &Aabb) -> Vec<i32> {
        let _ = (class, aabb);
        Vec::new()
    }

    /// `target.getVisibilityPercent(this)`: the target's own discretion,
    /// invisibility and armour, and this mob's head item against the
    /// target's kind. `1.0` when nothing reduces it.
    fn visibility_percent_of(&self, target_id: i32) -> f64 {
        let _ = target_id;
        1.0
    }

    /// `isAlliedTo(Entity)`.
    fn is_allied_to(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `getTeam() != null && getTeam() == other.getTeam()`.
    fn on_same_team_as(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `level().getDifficulty() == Difficulty.PEACEFUL`.
    fn difficulty_is_peaceful(&self) -> bool {
        false
    }

    /// `GameRules.UNIVERSAL_ANGER`.
    fn universal_anger(&self) -> bool {
        false
    }

    /// `Mob.canAttack(LivingEntity)`:
    /// `!target.is(GHAST) && super.canAttack(target)`, where
    /// `LivingEntity.canAttack` is
    /// `!(target instanceof Player && peaceful) && target.canBeSeenAsEnemy()`.
    fn can_attack(&self, target: &EntityFacts) -> bool {
        if target.is_ghast {
            return false;
        }
        if target.is_player && self.difficulty_is_peaceful() {
            return false;
        }
        target.can_be_seen_as_enemy()
    }

    /// `Mob.asValidTarget(LivingEntity)`: a creative or spectator player
    /// is `null`, an entity this mob cannot attack is `null`.
    fn as_valid_target(&self, id: Option<i32>) -> Option<i32> {
        let id = id?;
        let facts = self.entity(id)?;
        if facts.is_player && (facts.is_creative || facts.is_spectator) {
            return None;
        }
        if !self.can_attack(&facts) {
            return None;
        }
        Some(id)
    }

    /// The raw `Mob.target` field.
    fn target_raw(&self) -> Option<i32> {
        None
    }
    fn set_target_raw(&mut self, id: Option<i32>) {
        let _ = id;
    }

    /// `Mob.getTarget()`: `asValidTarget(target)`.
    fn target(&self) -> Option<i32> {
        self.as_valid_target(self.target_raw())
    }

    /// `Mob.setTarget(LivingEntity)`: `target = asValidTarget(t)`.
    fn set_target(&mut self, id: Option<i32>) {
        let v = self.as_valid_target(id);
        self.set_target_raw(v);
    }

    /// `Mob.setAggressive(boolean)`.
    fn set_aggressive(&mut self, v: bool) {
        let _ = v;
    }

    /// `swing(InteractionHand.MAIN_HAND)`.
    fn swing_main_hand(&mut self) {}

    /// `Mob.doHurtTarget(ServerLevel, Entity)`.
    fn do_hurt_target(&mut self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `Mob.isWithinMeleeAttackRange(LivingEntity)`.
    fn is_within_melee_attack_range(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `LivingEntity.getLastHurtByMob()`'s id and `getLastHurtByMobTimestamp()`.
    fn last_hurt_by_mob(&self) -> Option<i32> {
        None
    }
    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        0
    }

    /// `Animal.isInLove()`, `Animal.getAge()`, `PathfinderMob.isPanicking()`
    /// on this mob.
    fn is_in_love(&self) -> bool {
        false
    }
    fn age(&self) -> i32 {
        0
    }

    /// `Animal.canMate(Animal)`.
    fn can_mate_with(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `Animal.spawnChildFromBreeding(ServerLevel, Animal)`.
    fn spawn_child_from_breeding(&mut self, partner: i32) {
        let _ = partner;
    }

    /// `TemptingSensor.playerHoldingTemptation(mob, player)` (0..29):
    /// `isTemptation(mob, player.getMainHandItem()) ||
    /// isTemptation(mob, player.getOffhandItem())`, with the sensor's own
    /// `temptations` `BiPredicate`. The predicate reads item tags, which
    /// live outside this crate, so it is a host call rather than a port.
    fn player_holding_temptation(&self, player: i32, t: Temptations) -> bool {
        let _ = (player, t);
        false
    }

    /// `getAttributeValue(Attributes.TEMPT_RANGE)`.
    fn tempt_range(&self) -> f64 {
        10.0
    }

    /// `TemptGoal`'s item predicate against the entity's main and off
    /// hand: `items.test(getMainHandItem()) || items.test(getOffhandItem())`.
    fn holds_tempting_item(&self, id: i32) -> bool {
        let _ = id;
        false
    }

    /// `other.isAlliedTo(attacker)` for another mob, in
    /// `HurtByTargetGoal.alertOthers`.
    fn other_is_allied_to(&self, other: i32, attacker: i32) -> bool {
        let _ = (other, attacker);
        false
    }

    /// `other.getClass() == clazz` — an exact kind match.
    fn entity_is_exactly(&self, id: i32, class: EntityClass) -> bool {
        let _ = (id, class);
        false
    }

    /// `clazz.isAssignableFrom(entity.getClass())`.
    fn entity_is_assignable_to(&self, id: i32, class: EntityClass) -> bool {
        let _ = (id, class);
        false
    }

    /// `this instanceof TamableAnimal && this.getOwner() != other.getOwner()`.
    fn owner_differs_from(&self, other: i32) -> bool {
        let _ = other;
        false
    }

    /// `HurtByTargetGoal.alertOther(Mob, LivingEntity)`: `other.setTarget(attacker)`.
    fn alert_other(&mut self, other: i32, attacker: Option<i32>) {
        let _ = (other, attacker);
    }

    // ---- second lane: the calls the remaining goal classes make ----

    /// `Entity.getDeltaMovement()`.
    fn delta_movement(&self) -> Vec3 {
        Vec3::ZERO
    }
    /// `Entity.setDeltaMovement(double, double, double)`.
    fn set_delta_movement(&mut self, x: f64, y: f64, z: f64) {
        let _ = (x, y, z);
    }
    /// `Entity.wasInPowderSnow` and `Entity.isInPowderSnow` (fields).
    fn was_in_powder_snow(&self) -> bool {
        false
    }
    fn is_in_powder_snow(&self) -> bool {
        false
    }
    /// `getType().is(EntityTypeTags.POWDER_SNOW_WALKABLE_MOBS)`.
    fn is_powder_snow_walkable(&self) -> bool {
        false
    }
    /// `Mob.setPathfindingMalus(PathType, float)`: `pathfindingMalus.put(t, f)`.
    fn set_pathfinding_malus(&mut self, t: PathType, f: f32) {
        let _ = (t, f);
    }
    /// `Mob.isWithinHome()`: `isWithinHome(blockPosition())`.
    fn is_within_home(&self) -> bool {
        self.is_within_home_pos(self.block_position())
    }
    /// `Entity.isPassenger()` is above; `Leashable.mayBeLeashed()`.
    fn may_be_leashed(&self) -> bool {
        false
    }
    /// `Entity.isVehicle()`: `!passengers.isEmpty()`.
    fn is_vehicle(&self) -> bool {
        false
    }
    /// `Entity.getFirstPassenger()`'s id.
    fn first_passenger(&self) -> Option<i32> {
        None
    }
    /// `Entity.ejectPassengers()`.
    fn eject_passengers(&mut self) {}
    /// `Level.broadcastEntityEvent(this, id)`.
    fn broadcast_entity_event(&mut self, id: u8) {
        let _ = id;
    }
    /// `Entity.distanceToSqr(Entity)` for another entity, `NaN`-free only
    /// while it exists: `None` when it does not.
    fn distance_to_sqr_entity(&self, id: i32) -> Option<f64> {
        let f = self.entity(id)?;
        Some(f.distance_to_sqr(self.x(), self.y(), self.z()))
    }

    // ---- TamableAnimal ----

    /// `TamableAnimal.isTame()`: flag bit 4 of `DATA_FLAGS_ID`.
    fn is_tame(&self) -> bool {
        false
    }
    /// `TamableAnimal.isOrderedToSit()`: the `orderedToSit` field.
    fn is_ordered_to_sit(&self) -> bool {
        false
    }
    /// `TamableAnimal.getOwner()`'s id, `None` when there is none or it is
    /// not loaded.
    fn owner(&self) -> Option<i32> {
        None
    }
    /// `owner.level() != this.level()` for `SitWhenOrderedToGoal`; the
    /// default host has one level.
    fn owner_in_other_level(&self, owner: i32) -> bool {
        let _ = owner;
        false
    }
    /// `TamableAnimal.setInSittingPose(boolean)`: flag bit 1.
    fn set_in_sitting_pose(&mut self, v: bool) {
        let _ = v;
    }
    /// `other.getLastHurtByMob()` for another living entity — the owner,
    /// in `SitWhenOrderedToGoal`.
    fn entity_last_hurt_by_mob(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    /// `TamableAnimal.unableToMoveToOwner()`, offsets 0..43:
    /// `isOrderedToSit() || isPassenger() || mayBeLeashed()
    /// || getOwner() != null && getOwner().isSpectator()`.
    fn unable_to_move_to_owner(&self) -> bool {
        if self.is_ordered_to_sit() || self.is_passenger() || self.may_be_leashed() {
            return true;
        }
        match self.owner() {
            Some(o) => self.entity(o).is_some_and(|f| f.is_spectator),
            None => false,
        }
    }
    /// `TamableAnimal.shouldTryTeleportToOwner()`, offsets 0..29:
    /// `getOwner() != null && distanceToSqr(getOwner()) >= 144.0`.
    fn should_try_teleport_to_owner(&self) -> bool {
        match self.owner() {
            Some(o) => self.distance_to_sqr_entity(o).is_some_and(|d| d >= 144.0),
            None => false,
        }
    }
    /// `TamableAnimal.canFlyToOwner()`: `iconst_0`; the parrot overrides.
    fn can_fly_to_owner(&self) -> bool {
        false
    }
    /// `Entity.snapTo(double, double, double, float, float)`, as
    /// `TamableAnimal.maybeTeleportTo` calls it with the mob's own rotation.
    fn snap_to(&mut self, x: f64, y: f64, z: f64) {
        let _ = (x, y, z);
    }

    // ---- RangedAttackMob ----

    /// `RangedAttackMob.performRangedAttack(LivingEntity, float)`.
    fn perform_ranged_attack(&mut self, target: i32, power: f32) {
        let _ = (target, power);
    }

    // ---- item use (UseItemGoal) ----

    /// `LivingEntity.isUsingItem()`.
    fn is_using_item(&self) -> bool {
        false
    }
    /// `setItemSlot(EquipmentSlot.MAINHAND, item.copy())` (`None` is
    /// `ItemStack.EMPTY`).
    fn set_main_hand_item(&mut self, item: Option<&ItemSpec>) {
        let _ = item;
    }
    /// `startUsingItem(InteractionHand.MAIN_HAND)`.
    fn start_using_main_hand(&mut self) {}
    /// `playSound(SoundEvent, float volume, float pitch)`.
    fn play_sound(&mut self, sound: &SoundSpec, volume: f32, pitch: f32) {
        let _ = (sound, volume, pitch);
    }

    // ---- AbstractHorse ----

    /// `AbstractHorse.isTamed()`: flag bit 2 of `DATA_ID_FLAGS`.
    fn horse_is_tamed(&self) -> bool {
        false
    }
    /// `AbstractHorse.isMobControlled()` — `getControllingPassenger()
    /// instanceof Mob`.
    fn is_mob_controlled(&self) -> bool {
        self.controlling_passenger_is_mob()
    }
    /// `AbstractHorse.getTemper()` / `getMaxTemper()` (100 in the base).
    fn temper(&self) -> i32 {
        0
    }
    fn max_temper(&self) -> i32 {
        100
    }
    /// `AbstractHorse.tameWithName(Player)`.
    fn tame_with_name(&mut self, player: i32) -> bool {
        let _ = player;
        false
    }
    /// `AbstractHorse.modifyTemper(int)`: `setTemper(clamp(temper + n, 0, maxTemper))`,
    /// answering the new value.
    fn modify_temper(&mut self, n: i32) -> i32 {
        let _ = n;
        0
    }
    /// `AbstractHorse.makeMad()`.
    fn make_mad(&mut self) {}

    // ---- targets: NeutralMob, owners, raids, villages ----

    /// `NeutralMob.forgetCurrentTargetAndRefreshUniversalAnger()` on this mob.
    fn forget_current_target_and_refresh_universal_anger(&mut self) {}
    /// The same on another mob, in `ResetUniversalAngerTargetGoal.start`.
    fn other_forget_current_target_and_refresh_universal_anger(&mut self, id: i32) {
        let _ = id;
    }
    /// `other.getLastHurtMob()` / `getLastHurtMobTimestamp()` on another
    /// living entity — the owner, in `OwnerHurtTargetGoal`.
    fn entity_last_hurt_mob(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    fn entity_last_hurt_mob_timestamp(&self, id: i32) -> i32 {
        let _ = id;
        0
    }
    /// `other.getLastHurtByMobTimestamp()`, paired with
    /// [`MobHost::entity_last_hurt_by_mob`].
    fn entity_last_hurt_by_mob_timestamp(&self, id: i32) -> i32 {
        let _ = id;
        0
    }
    /// `TamableAnimal.wantsToAttack(LivingEntity target, LivingEntity owner)`:
    /// `iconst_1` in the base; the wolf overrides it.
    fn wants_to_attack(&self, target: i32, owner: i32) -> bool {
        let _ = (target, owner);
        true
    }
    /// `Raider.hasActiveRaid()`.
    fn has_active_raid(&self) -> bool {
        false
    }
    /// `Villager.getPlayerReputation(Player)` for a villager and a player.
    fn villager_player_reputation(&self, villager: i32, player: i32) -> i32 {
        let _ = (villager, player);
        0
    }

    // ---- batch three: villagers, creepers, horses, the sun, air ----

    /// `Entity.isAlive()`.
    fn is_alive(&self) -> bool {
        true
    }
    /// `Entity.hurtMarked` (field).
    fn hurt_marked(&self) -> bool {
        false
    }
    /// `AbstractVillager.getTradingPlayer()`'s id.
    fn trading_player(&self) -> Option<i32> {
        None
    }
    /// `AbstractVillager.setTradingPlayer(Player)`.
    fn set_trading_player(&mut self, id: Option<i32>) {
        let _ = id;
    }
    /// `AbstractVillager.isTrading()`: `tradingPlayer != null`.
    fn is_trading(&self) -> bool {
        self.trading_player().is_some()
    }
    /// `Creeper.getSwellDir()` / `setSwellDir(int)`.
    fn swell_dir(&self) -> i32 {
        0
    }
    fn set_swell_dir(&mut self, v: i32) {
        let _ = v;
    }
    /// `other.isDeadOrDying()`: `getHealth() <= 0 || dead`; a host with
    /// health answers it exactly, the default reads it as `!isAlive()`.
    fn entity_dead_or_dying(&self, id: i32) -> bool {
        self.entity(id).is_none_or(|f| !f.alive)
    }
    /// `getItemBySlot(EquipmentSlot.HEAD).isEmpty()`.
    fn head_slot_empty(&self) -> bool {
        true
    }
    /// `Entity.xRotO` / `yRotO`: last tick's rotation, what
    /// `getXRot(0.0f)` / `getYRot(0.0f)` lerp to.
    fn x_rot_o(&self) -> f32 {
        self.x_rot()
    }
    fn y_rot_o(&self) -> f32 {
        self.y_rot()
    }
    /// `Entity.getViewVector(0.0f)`: `calculateViewVector(getXRot(0), getYRot(0))`,
    /// and at partial tick zero both lerps answer the `..O` field.
    fn view_vector_at_zero(&self) -> Vec3 {
        crate::mth::calculate_view_vector(self.x_rot_o(), self.y_rot_o())
    }
    /// `Entity.moveRelative(float speed, Vec3 input)`:
    /// `setDeltaMovement(getDeltaMovement().add(getInputVector(input, speed, getYRot())))`.
    fn move_relative(&mut self, speed: f32, input: Vec3) {
        let v = crate::mth::get_input_vector(input, speed, self.y_rot());
        let d = self.delta_movement() + (v);
        self.set_delta_movement(d.x, d.y, d.z);
    }
    /// `Entity.move(MoverType.SELF, getDeltaMovement())` — the collision
    /// step, the host's.
    fn move_by_delta_movement(&mut self) {}
    /// `LivingEntity.xxa` / `yya` / `zza`, the input fields.
    fn xxa(&self) -> f32 {
        0.0
    }
    fn yya(&self) -> f32 {
        0.0
    }
    fn zza(&self) -> f32 {
        0.0
    }
    /// `AbstractHorse.standIfPossible()`.
    fn stand_if_possible(&mut self) {}
    /// `AbstractHorse.getAmbientStandSound()` (`getAmbientSound()`).
    fn ambient_stand_sound(&self) -> Option<SoundSpec> {
        None
    }
    /// `AbstractHorse.getAmbientStandInterval()`: `getAmbientSoundInterval()`,
    /// `bipush 80` in `Mob`.
    fn ambient_stand_interval(&self) -> i32 {
        80
    }
    /// `AbstractHorse.isImmobile()`.
    fn is_immobile(&self) -> bool {
        false
    }
    /// `Entity.isSilent()`.
    fn is_silent(&self) -> bool {
        false
    }
    /// `Entity.playSound(SoundEvent)`: `if !isSilent() playSound(s, 1.0f, 1.0f)`.
    fn play_sound_simple(&mut self, sound: &SoundSpec) {
        if !self.is_silent() {
            self.play_sound(sound, 1.0, 1.0);
        }
    }
    /// `Entity.getAirSupply()`.
    fn air_supply(&self) -> i32 {
        300
    }
    /// `ServerLevel.isVillage(BlockPos)`.
    fn is_village(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `BehaviorUtils.findSectionClosestToVillage(level, section, radius)`:
    /// the same section when none within the radius is a village.
    fn find_section_closest_to_village(
        &self,
        section: (i32, i32, i32),
        radius: i32,
    ) -> (i32, i32, i32) {
        let _ = radius;
        section
    }
    /// `IronGolem.offerFlower(boolean)`.
    fn offer_flower(&mut self, v: bool) {
        let _ = v;
    }
    /// `getEntities(EntityTypeTags.CANDIDATE_FOR_IRON_GOLEM_GIFT, box)`'s
    /// ids, in the level's order.
    fn iron_golem_gift_candidates_in(&self, aabb: &Aabb) -> Vec<i32> {
        let _ = aabb;
        Vec::new()
    }
    /// `other instanceof Mob && other.is(ACCEPTS_IRON_GOLEM_GIFT)
    /// && other.getItemBySlot(EQUIPMENT_SLOT_ANTENNA).isEmpty()`.
    fn accepts_iron_golem_gift_with_empty_antenna(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `other.setItemSlot(ANTENNA, POPPY.getDefaultInstance()); other.setGuaranteedDrop(ANTENNA)`.
    fn give_iron_golem_gift(&mut self, id: i32) {
        let _ = id;
    }

    // ---- batch four: doors, blocks, bows, flocks, shoulders ----

    /// `Entity.horizontalCollision` (field).
    fn horizontal_collision(&self) -> bool {
        false
    }
    /// `DoorBlock.setOpen(entity, level, state, pos, open)` on the door at `pos`.
    fn set_door_open(&mut self, pos: BlockPos, open: bool) {
        let _ = (pos, open);
    }
    /// `GameRules.MOB_GRIEFING`.
    fn mob_griefing(&self) -> bool {
        true
    }
    /// `level().getDifficulty()`.
    fn difficulty(&self) -> Difficulty {
        Difficulty::Normal
    }
    /// `Level.levelEvent(int, BlockPos, int)`.
    fn level_event(&mut self, id: i32, pos: BlockPos, data: i32) {
        let _ = (id, pos, data);
    }
    /// `Level.levelEvent(2001, pos, Block.getId(getBlockState(pos)))` — the
    /// break particles for whatever stands at `pos`.
    fn level_event_block_broken(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `Level.destroyBlockProgress(getId(), pos, progress)`.
    fn destroy_block_progress(&mut self, pos: BlockPos, progress: i32) {
        let _ = (pos, progress);
    }
    /// `Level.removeBlock(pos, false)`.
    fn remove_block(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `Level.destroyBlock(pos, false)`.
    fn destroy_block(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `level.setBlock(pos, DIRT.defaultBlockState(), 2)` after
    /// `levelEvent(2001, pos, id(GRASS_BLOCK))` — `EatBlockGoal`'s grass bite.
    fn turn_grass_to_dirt(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `Mob.ate()`.
    fn ate(&mut self) {}
    /// `Mob.isBaby()`.
    fn is_baby(&self) -> bool {
        false
    }
    /// `LivingEntity.swinging` (field) and `swing(getUsedItemHand())`.
    fn swinging(&self) -> bool {
        false
    }
    fn swing_used_item_hand(&mut self) {}
    /// `Entity.fallDistance = d`.
    fn set_fall_distance(&mut self, d: f64) {
        let _ = d;
    }
    /// `ServerLevel.sendParticles(ITEM(EGG), x, y, z, 3, dx, dy, dz, 0.15)`
    /// and `sendParticles(POOF, x, y, z, 1, dx, dy, dz, 0.15)`; the offsets
    /// are the goal's draws, made here in the jar's order.
    fn send_egg_particles(&mut self, x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64) {
        let _ = (x, y, z, dx, dy, dz);
    }
    fn send_poof_particle(&mut self, x: f64, y: f64, z: f64, dx: f64, dy: f64, dz: f64) {
        let _ = (x, y, z, dx, dy, dz);
    }
    /// `RemoveBlockGoal.playDestroyProgressSound` / `playBreakSound` —
    /// bare returns in the base, the zombie's egg goal overrides them.
    fn play_destroy_progress_sound(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    fn play_break_sound(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `BegGoal.playerHoldingInteresting(Player)`: either hand holds a
    /// bone or `wolf.isFood(stack)`.
    fn player_holding_interesting(&self, player: i32) -> bool {
        let _ = player;
        false
    }
    /// `Wolf.setIsInterested(boolean)`.
    fn set_is_interested(&mut self, v: bool) {
        let _ = v;
    }
    /// `Monster.isHolding(Items.BOW)`.
    fn is_holding_bow(&self) -> bool {
        false
    }
    /// `LivingEntity.stopUsingItem()` / `getTicksUsingItem()`.
    fn stop_using_item(&mut self) {}
    fn ticks_using_item(&self) -> i32 {
        0
    }
    /// `startUsingItem(ProjectileUtil.getWeaponHoldingHand(this, Items.BOW))`.
    fn start_using_bow_hand(&mut self) {}
    /// `getControlledVehicle()` when it is a `Mob`, and that mob's
    /// `lookAt(target, maxYaw, maxPitch)`.
    fn controlled_vehicle_mob(&self) -> Option<i32> {
        None
    }
    fn other_mob_look_at(
        &mut self,
        mob: i32,
        x: f64,
        y_for_pitch: f64,
        z: f64,
        max_yaw: f32,
        max_pitch: f32,
    ) {
        let _ = (mob, x, y_for_pitch, z, max_yaw, max_pitch);
    }
    /// `AbstractSchoolingFish`: `hasFollowers()` is `schoolSize > 1`,
    /// `isFollower()` is `leader != null && leader.isAlive()`,
    /// `canBeFollowed()` is `hasFollowers() && schoolSize < getMaxSchoolSize()`,
    /// `inRangeOfLeader()` is `distanceToSqr(leader) <= 121.0`.
    fn fish_school_size(&self, id: i32) -> i32 {
        let _ = id;
        1
    }
    fn fish_max_school_size(&self, id: i32) -> i32 {
        let _ = id;
        1
    }
    fn fish_leader(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    fn fish_has_followers(&self, id: i32) -> bool {
        self.fish_school_size(id) > 1
    }
    fn fish_is_follower(&self, id: i32) -> bool {
        self.fish_leader(id)
            .and_then(|l| self.entity(l))
            .is_some_and(|f| f.alive)
    }
    fn fish_can_be_followed(&self, id: i32) -> bool {
        self.fish_has_followers(id) && self.fish_school_size(id) < self.fish_max_school_size(id)
    }
    /// `follower.startFollowing(leader)`: `follower.leader = leader; leader.addFollower()`.
    fn fish_start_following(&mut self, follower: i32, leader: i32) {
        let _ = (follower, leader);
    }
    /// `stopFollowing()`: `leader.removeFollower(); leader = null` on this mob.
    fn fish_stop_following(&mut self) {}
    /// `ShoulderRidingEntity`: `canSitOnShoulder()`, `isInSittingPose()`,
    /// `isLeashed()`, `setEntityOnShoulder(ServerPlayer)`.
    fn can_sit_on_shoulder(&self) -> bool {
        false
    }
    fn is_in_sitting_pose(&self) -> bool {
        false
    }
    fn is_leashed(&self) -> bool {
        false
    }
    fn set_entity_on_shoulder(&mut self, player: i32) -> bool {
        let _ = player;
        false
    }
    /// A player's `getAbilities().flying`, and any entity's `isInWater()`
    /// / `isInPowderSnow`.
    fn player_is_flying(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    fn entity_in_water(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    fn entity_in_powder_snow(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `other.getControllingPassenger()` when it is a `Player`, and
    /// `Player.hasMovedHorizontallyRecently()`.
    fn controlling_passenger_player(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    fn player_has_moved_horizontally_recently(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Cat.isLying()` / `setLying(boolean)`.
    fn is_lying(&self) -> bool {
        false
    }
    fn set_lying(&mut self, v: bool) {
        let _ = v;
    }
    /// `Creeper`-style swell aside, `Dolphin.getMotionDirection()` and
    /// every `Entity.getMotionDirection()` is `getDirection()`, which is
    /// `Direction.fromYRot(getYRot())`.
    fn motion_direction(&self) -> crate::ai::direction::Direction {
        crate::ai::direction::Direction::from_y_rot(f64::from(self.y_rot()))
    }

    // ---- batch five: raids, caravans, villages, crossbows, spears ----

    /// `Raider.getCurrentRaid().isOver()` / `isActive()`, `getCenter()`,
    /// `getGroupsSpawned()`; only meaningful under `hasActiveRaid()`.
    fn raid_is_over(&self) -> bool {
        true
    }
    fn raid_is_active(&self) -> bool {
        false
    }
    fn raid_center(&self) -> BlockPos {
        BlockPos { x: 0, y: 0, z: 0 }
    }
    /// `level.getEntitiesOfClass(Raider.class, box, r -> !r.hasActiveRaid()
    /// && Raids.canJoinRaid(r))`, in the level's order.
    fn raid_recruits_in(&self, aabb: &Aabb) -> Vec<i32> {
        let _ = aabb;
        Vec::new()
    }
    /// `raid.joinRaid(level, raid.getGroupsSpawned(), raider, null, true)`.
    fn raid_join(&mut self, raider: i32) {
        let _ = raider;
    }
    /// `Mob.isPathFinding()`: `!navigation.isDone()` — answered by the
    /// navigation the goal holds, so no host call.
    /// `Llama.inCaravan()`, `hasCaravanTail()`, `getCaravanHead()`,
    /// `joinCaravan(Llama)`, `leaveCaravan()`, and the same on another llama.
    fn in_caravan(&self) -> bool {
        false
    }
    fn caravan_head(&self) -> Option<i32> {
        None
    }
    fn join_caravan(&mut self, head: i32) {
        let _ = head;
    }
    fn leave_caravan(&mut self) {}
    fn llama_in_caravan(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    fn llama_has_caravan_tail(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    fn llama_caravan_head(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    fn llama_is_leashed(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `level.getEntities(this, box, e -> e.is(LLAMA) || e.is(TRADER_LLAMA))`.
    fn llamas_in(&self, aabb: &Aabb) -> Vec<i32> {
        let _ = aabb;
        Vec::new()
    }
    /// `getLeashHolder() instanceof LeashFenceKnotEntity`.
    fn leash_holder_is_fence_knot(&self) -> bool {
        false
    }
    /// `other.isInvisible()`.
    fn entity_is_invisible(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// Another mob's `getLookControl().getWantedX/Y/Z()`.
    fn other_look_wanted(&self, id: i32) -> Option<Vec3> {
        let _ = id;
        None
    }
    /// `ServerLevel.isCloseToVillage(pos, sections)` and `sectionsToVillage(section)`.
    fn is_close_to_village(&self, pos: BlockPos, sections: i32) -> bool {
        let _ = (pos, sections);
        false
    }
    fn sections_to_village(&self, section: (i32, i32, i32)) -> i32 {
        let _ = section;
        i32::MAX
    }
    /// `PoiManager.find(type.is(VILLAGE), p -> hasNotVisited(p), pos, 10, IS_OCCUPIED)`:
    /// the first occupied village point of interest within `radius` of
    /// `pos` not in `visited`, in the manager's order.
    fn find_village_poi(
        &self,
        pos: BlockPos,
        radius: i32,
        visited: &[BlockPos],
    ) -> Option<BlockPos> {
        let _ = (pos, radius, visited);
        None
    }
    /// `Level.getRandom()` — the level's own source, not the mob's.
    fn level_random_next_float(&mut self) -> f32 {
        0.0
    }
    fn level_random_next_int(&mut self, bound: i32) -> i32 {
        let _ = bound;
        0
    }
    /// `level.getEntities(EntityTypes.VILLAGER, box, v -> v.wantsToSpawnGolem(gameTime))`.
    fn villagers_wanting_golem_in(&self, aabb: &Aabb) -> Vec<i32> {
        let _ = aabb;
        Vec::new()
    }
    /// `SectionPos.cube(section, radius).filter(s -> sectionsToVillage(s) == 0)`,
    /// in `cube`'s order.
    fn village_sections_in_cube(
        &self,
        section: (i32, i32, i32),
        radius: i32,
    ) -> Vec<(i32, i32, i32)> {
        let _ = (section, radius);
        Vec::new()
    }
    /// `poiManager.getInRange(any, center, 8, IS_OCCUPIED).map(PoiRecord::getPos)`.
    fn occupied_pois_within(&self, center: BlockPos, radius: i32) -> Vec<BlockPos> {
        let _ = (center, radius);
        Vec::new()
    }
    /// `Level.getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos)`.
    fn heightmap_pos_motion_blocking_no_leaves(&self, pos: BlockPos) -> BlockPos {
        pos
    }
    /// `Monster.isHolding(Items.CROSSBOW)`, `CrossbowAttackMob.setChargingCrossbow`,
    /// `startUsingItem(getWeaponHoldingHand(CROSSBOW))`, `releaseUsingItem()`,
    /// `CrossbowItem.getChargeDuration(getUseItem(), this)`, and the
    /// `CHARGED_PROJECTILES = EMPTY` reset on `stop`.
    fn is_holding_crossbow(&self) -> bool {
        false
    }
    fn set_charging_crossbow(&mut self, v: bool) {
        let _ = v;
    }
    fn start_using_crossbow_hand(&mut self) {}
    fn release_using_item(&mut self) {}
    fn crossbow_charge_duration(&self) -> i32 {
        25
    }
    fn clear_charged_projectiles(&mut self) {}
    /// `getMainHandItem().has(DataComponents.KINETIC_WEAPON)` and that
    /// component's `useDuration`; `getRootVehicle()` as a `Mob` and its
    /// `chargeSpeedModifier()`.
    fn main_hand_is_kinetic_weapon(&self) -> bool {
        false
    }
    fn kinetic_weapon_use_duration(&self) -> i32 {
        0
    }
    fn root_vehicle_charge_speed_modifier(&self) -> Option<f32> {
        None
    }

    // ---- brains: what the behaviour and sensor classes reach ----

    /// `level.getDayTime()`, which the `minecraft:overworld` clock's
    /// `getTotalTicks` reads for the villager schedule timeline.
    fn day_time(&self) -> i64 {
        0
    }
    /// `level.dimension()`, for `GlobalPos.of(level.dimension(), pos)`.
    fn dimension(&self) -> Dimension {
        Dimension::Overworld
    }
    /// `level.environmentAttributes().getValue(schedule, pos)`: the
    /// default resolves the jar's timeline data alone at `day_time()`
    /// (`crate::brain::schedule`); a level with positional layers on this
    /// attribute answers for itself.
    fn schedule_activity(&self, schedule: Schedule, pos: Vec3) -> Activity {
        let _ = pos;
        schedule.resolve(self)
    }
    /// `getLastDamageSource()`, `null` when none or expired.
    fn last_damage_source(&self) -> Option<DamageSourceFacts> {
        None
    }
    /// `entity.getUUID()` for a loaded entity by id — the reverse of
    /// [`Self::entity_by_uuid`]. `0` for an id this host does not know.
    fn entity_uuid(&self, id: i32) -> u128 {
        let _ = id;
        0
    }
    /// `e instanceof AbstractCubeMob c ? c.getSize() : null`.
    fn cube_mob_size(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    /// `Warden.canTargetEntity(Entity)` (0..99): `instanceof LivingEntity`,
    /// the same level, `EntitySelector.NO_CREATIVE_OR_SPECTATOR`,
    /// `!isAlliedTo`, not `ARMOR_STAND`, not `WARDEN`, `!isInvulnerable`,
    /// `!isDeadOrDying`, and inside the world border. Every conjunct is
    /// outside this crate, so the whole predicate is one host call.
    fn can_target_entity(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Armadillo.isScaredBy(LivingEntity)` and
    /// `Armadillo.canStayRolledUp()`, the `mobTest` and `readyTest` of the
    /// `ARMADILLO_SCARE_DETECTED` `MobSensor`.
    fn is_scared_by(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    fn can_stay_rolled_up(&self) -> bool {
        false
    }
    /// `level.getBlockState(pos).is(BlockTags.HOGLIN_REPELLENTS)` — a
    /// different tag from the piglin's, so a call of its own.
    fn is_hoglin_repellent_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `level.getEntity(UUID)` — the id of the entity with that uuid, if loaded.
    fn entity_by_uuid(&self, uuid: u128) -> Option<i32> {
        let _ = uuid;
        None
    }
    /// `GameRules.FORGIVE_DEAD_PLAYERS`.
    fn forgive_dead_players(&self) -> bool {
        false
    }
    /// `level.getWorldBorder().isWithinBounds(pos)`.
    fn world_border_contains(&self, pos: BlockPos) -> bool {
        let _ = pos;
        true
    }
    /// `canPickUpLoot()`.
    fn can_pick_up_loot(&self) -> bool {
        false
    }
    /// `Villager.releasePoi(MemoryModuleType)`.
    fn release_poi(&mut self, memory: MemoryModuleType) {
        let _ = memory;
    }
    /// `damageSource.is(TagKey<DamageType>)` for the source's type name.
    fn damage_type_is(&self, kind: &str, tag: DamageTypeTag) -> bool {
        let _ = (kind, tag);
        false
    }
    /// `hasPassenger(entity)`.
    fn has_passenger(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `mob.isHolding(mob::canUseNonMeleeWeapon)` — `MeleeAttack.isHoldingUsableNonMeleeWeapon`.
    fn is_holding_usable_non_melee_weapon(&self) -> bool {
        false
    }
    /// `BehaviorUtils.isWithinAttackRange`'s projectile branch: when
    /// `getMainHandItem().getItem()` is a `ProjectileWeaponItem` the mob
    /// `canUseNonMeleeWeapon`, its `getDefaultProjectileRange()`.
    fn usable_projectile_weapon_range(&self) -> Option<i32> {
        None
    }
    /// `entity instanceof LivingEntity`.
    fn entity_is_living(&self, id: i32) -> bool {
        let _ = id;
        true
    }
    /// `entity.level() == this.level()`.
    fn entity_in_same_level(&self, id: i32) -> bool {
        let _ = id;
        true
    }
    /// `entity.getType() == this.getType()`.
    fn entity_type_matches_mine(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `entity.getType().getCategory() == category`.
    fn entity_category_is(&self, id: i32, category: MobCategory) -> bool {
        let _ = (id, category);
        false
    }
    /// `entity.is(EntityType)` by registry name (`minecraft:` implied).
    fn entity_kind_is(&self, id: i32, kind: &str) -> bool {
        let _ = (id, kind);
        false
    }
    /// `entity.getType().is(TagKey<EntityType>)` by tag name
    /// (`minecraft:` implied), e.g. `followable_friendly_mobs`.
    fn entity_type_tag_is(&self, id: i32, tag: &str) -> bool {
        let _ = (id, tag);
        false
    }
    /// `InteractWithDoor.isMobComingThroughDoor(other.getBrain(), pos)`:
    /// the other mob's `PATH` memory is present, not done, has a previous
    /// node, and `pos` is that node's or the next node's block position.
    fn entity_is_coming_through_door(&self, id: i32, door: BlockPos) -> bool {
        let _ = (id, door);
        false
    }

    // ---- brains, fourth lane: villagers, raids, beds, bells, riding ----

    /// `LivingEntity.isSleeping()`.
    fn is_sleeping(&self) -> bool {
        false
    }
    /// `LivingEntity.stopSleeping()`.
    fn stop_sleeping(&mut self) {}
    /// `LivingEntity.startSleeping(pos)`.
    fn start_sleeping(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `ServerLevel.getRaidAt(pos)`: the raid whose village covers `pos`,
    /// with the flags the villager behaviours read off it.
    fn raid_at(&self, pos: BlockPos) -> Option<RaidFacts> {
        let _ = pos;
        None
    }
    /// `Villager.getVillagerData()`: the profession's registry key and
    /// the villager level.
    fn villager_data(&self) -> VillagerDataFacts {
        VillagerDataFacts {
            profession: "none",
            level: 1,
        }
    }
    /// `Villager.getVillagerXp()`.
    fn villager_xp(&self) -> i32 {
        0
    }
    /// `setVillagerData(getVillagerData().withProfession(registryAccess, key))`.
    fn set_villager_profession(&mut self, profession: &'static str) {
        let _ = profession;
    }
    /// `Villager.refreshBrain(level)`.
    fn refresh_brain(&mut self) {}
    /// `level.getBlockState(pos).is(Blocks.BELL)`.
    fn is_bell_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `BellBlock.attemptToRing(entity, level, pos, null)`; the result is
    /// popped by every caller.
    fn attempt_to_ring_bell(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `Entity.getVehicle()`.
    fn vehicle(&self) -> Option<i32> {
        None
    }
    /// `LivingEntity.startRiding(entity)`; the result is popped by every
    /// brain caller.
    fn start_riding(&mut self, id: i32) {
        let _ = id;
    }
    /// `LivingEntity.stopRiding()`.
    fn stop_riding(&mut self) {}
    /// `Entity.closerThan(other, d)`: `distanceToSqr(other) < d * d`, for
    /// an entity this host can see.
    fn entity_closer_than(&self, id: i32, d: f64) -> bool {
        self.distance_to_sqr_entity(id).is_some_and(|sq| sq < d * d)
    }
    /// `PoiManager.getInRange(any, center, radius, occupancy)` as the
    /// brain reads it: every record within `radius` (`distSqr <
    /// radius * radius`, after the square scan) with that occupancy, in
    /// the manager's own order — `ChunkPos.rangeClosed` around the centre
    /// chunk, then each chunk's sections and records — as `(poi type key,
    /// pos)`. The type predicate and the position predicate are applied by
    /// the caller, in that order, because the position predicate has side
    /// effects the manager's order sequences.
    fn pois_in_range(
        &self,
        center: BlockPos,
        radius: i32,
        occupancy: PoiOccupancy,
    ) -> Vec<PoiRecordFacts> {
        let _ = (center, radius, occupancy);
        Vec::new()
    }
    /// `PoiManager.getType(pos)`: the registry key of the point of interest
    /// at `pos`, if any.
    fn poi_type_at(&self, pos: BlockPos) -> Option<&'static str> {
        let _ = pos;
        None
    }
    /// `PoiType.validRange()` for a type key.
    fn poi_valid_range(&self, poi_type: &str) -> i32 {
        let _ = poi_type;
        1
    }
    /// `PoiManager.take(typePredicate, (h, p) -> p.equals(pos), pos, 1)`:
    /// occupy the record at `pos`; the result is popped by every caller.
    fn take_poi(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `level.getEntitiesOfClass(ItemEntity.class, box, e -> true)`, in the
    /// level's order.
    fn item_entities_in(&self, aabb: &Aabb) -> Vec<i32> {
        let _ = aabb;
        Vec::new()
    }
    /// `ItemEntity.getItem()` for an item entity by id.
    fn item_entity_stack(&self, id: i32) -> Option<ItemSpec> {
        let _ = id;
        None
    }
    /// `getOffhandItem()` on this mob; `None` for `ItemStack.EMPTY`.
    fn offhand_item(&self) -> Option<ItemSpec> {
        None
    }
    /// `stack.has(DataComponents.BLOCKS_ATTACKS)` — a shield, in 26.2's
    /// component terms.
    fn item_blocks_attacks(&self, item: &ItemSpec) -> bool {
        let _ = item;
        false
    }
    /// `PiglinAi.isLovedItem(ItemStack)`.
    fn piglin_is_loved_item(&self, item: &ItemSpec) -> bool {
        let _ = item;
        false
    }
    /// `PiglinAi.stopHoldingOffHandItem(level, piglin, shouldBarter)`.
    fn piglin_stop_holding_off_hand_item(&mut self, should_barter: bool) {
        let _ = should_barter;
    }
    /// `PiglinAi.setAngerTarget(level, piglin, target)`.
    fn piglin_set_anger_target(&mut self, target: i32) {
        let _ = target;
    }
    /// `PiglinAi.dontKillAnyMoreHoglinsForAWhile(piglin)`: sets
    /// `HUNTED_RECENTLY` on **every** nearby adult piglin, this one
    /// included, each with its own `TIME_BETWEEN_HUNTS` draw.
    fn piglin_dont_kill_any_more_hoglins_for_a_while(&mut self, id: i32) {
        let _ = id;
    }
    /// `PiglinAi.broadcastAngerTarget(level, piglin, target)`.
    fn piglin_broadcast_anger_target(&mut self, target: i32) {
        let _ = target;
    }
    /// `other.getBrain().hasMemoryValue(ANGRY_AT)` for another piglin, the
    /// predicate `StartHuntingHoglin` runs over `NEAREST_VISIBLE_ADULT_PIGLINS`.
    fn other_is_angry(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Warden.getEntityAngryAt()`: the entity the warden's
    /// `AngerManagement` currently holds the top grudge against, if it is
    /// loaded.
    fn warden_entity_angry_at(&self) -> Option<i32> {
        None
    }
    /// `Entity.unRide()`.
    fn un_ride(&mut self) {}
    /// `Entity.hasPose(Pose)`.
    fn has_pose(&self, pose: Pose) -> bool {
        let _ = pose;
        false
    }
    /// `Warden.increaseAngerAt(Entity)` — the one-argument form, which is
    /// `increaseAngerAt(entity, 35, true)`.
    fn warden_increase_anger_at(&mut self, id: i32) {
        let _ = id;
    }
    /// `Warden.increaseAngerAt(Entity, int, boolean)`.
    fn warden_increase_anger_at_by(&mut self, id: i32, amount: i32, alert_others: bool) {
        let _ = (id, amount, alert_others);
    }
    /// `WardenAi.setDisturbanceLocation(warden, pos)`.
    fn warden_set_disturbance_location(&mut self, pos: BlockPos) {
        let _ = pos;
    }
    /// `Warden.setAttackTarget(LivingEntity)`.
    fn warden_set_attack_target(&mut self, id: i32) {
        let _ = id;
    }
    /// `Warden.getRemovalReason() != null`.
    fn is_removed(&self) -> bool {
        false
    }
    /// `Entity.remove(RemovalReason.DISCARDED)`.
    fn remove_discarded(&mut self) {}
    /// `Warden`'s sonic-boom effect: the particle line, the sound, the
    /// 10.0f `sonicBoom` damage and the knockback push, all of which need
    /// the level. `Roar`/`SonicBoom` name the constants; this call does
    /// the work.
    fn warden_sonic_boom_at(&mut self, id: i32) {
        let _ = id;
    }
    /// `LookControl.setLookAt(Vec3)`.
    fn look_control_set_look_at(&mut self, target: Vec3) {
        let _ = target;
    }
    /// `level.getFluidState(pos).isEmpty()` — no fluid at all, which is
    /// weaker than "not water".
    fn fluid_is_empty_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        true
    }
    /// `level.getBlockState(pos).isFaceSturdy(level, pos, Direction.UP)`.
    fn is_face_sturdy_up(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `level.getBlockState(pos).getCollisionShape(level,
    /// pos).getFaceShape(Direction.UP).isEmpty()`.
    fn collision_face_shape_up_empty(&self, pos: BlockPos) -> bool {
        let _ = pos;
        true
    }
    /// `level.getFluidState(pos).is(TagKey<Fluid>)` by tag name.
    fn fluid_tag_is(&self, pos: BlockPos, tag: &str) -> bool {
        let _ = (pos, tag);
        false
    }
    /// `TryLaySpawnOnFluidNearLand`'s write: `level.setBlock(pos,
    /// block.defaultBlockState(), 3)`, `level.gameEvent(BLOCK_PLACE, pos,
    /// Context.of(mob, state))` and
    /// `level.playSound(null, mob, FROG_LAY_SPAWN, BLOCKS, 1.0f, 1.0f)`,
    /// in that order — three level operations that cannot live here.
    fn lay_spawn_block(&mut self, pos: BlockPos, block: &'static str) {
        let _ = (pos, block);
    }
    /// `StayCloseToTarget`'s caller-supplied
    /// `Function<LivingEntity, Optional<PositionTracker>>`, by name.
    fn stay_close_target(&self, name: &str) -> Option<crate::brain::memory::PositionTracker> {
        let _ = name;
        None
    }
    /// `StayCloseToTarget`'s caller-supplied
    /// `Predicate<LivingEntity>` (`canTravelTo`), by the same name.
    fn stay_close_can_travel_to(&self, name: &str) -> bool {
        let _ = name;
        false
    }
    /// `LivingEntity.addEffect(new MobEffectInstance(effect, duration,
    /// amplifier))`, by effect registry name (`minecraft:` implied).
    fn add_effect(&mut self, effect: &'static str, duration: i32, amplifier: i32) {
        let _ = (effect, duration, amplifier);
    }

    /// `Mob.wantsToPickUp(level, itemEntity.getItem())`.
    fn wants_to_pick_up(&self, item_entity: i32) -> bool {
        let _ = item_entity;
        false
    }
    /// `profession.secondaryPoi().contains(level.getBlockState(pos).getBlock())`
    /// for this villager's profession.
    fn is_secondary_poi_block_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `Hoglin.canBeHunted()` for another entity.
    fn hoglin_can_be_hunted(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `PiglinAi.isWearingSafeArmor(entity)`: any armour slot's item is in
    /// `ItemTags.PIGLIN_SAFE_ARMOR`.
    fn is_wearing_safe_armor(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `entity.isHolding(PiglinAi::isLovedItem)`: either hand's item is in
    /// `ItemTags.PIGLIN_LOVED`.
    fn is_holding_loved_item(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `PiglinSpecificSensor.isValidRepellent(level, pos)`: the block is in
    /// `BlockTags.PIGLIN_REPELLENTS`, and a soul campfire only when lit.
    fn is_piglin_repellent_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `Villager.spawnGolemIfNeeded(level, gameTime, requiredCount)`.
    fn spawn_golem_if_needed(&mut self, game_time: i64, required_count: i32) {
        let _ = (game_time, required_count);
    }
    /// `Villager.assignProfessionWhenSpawned()`.
    fn assign_profession_when_spawned(&self) -> bool {
        false
    }
    /// `level.loadedAndEntityCanStandOn(pos, this)`.
    fn loaded_and_entity_can_stand_on(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `Level.getHeightmapPos(MOTION_BLOCKING, pos)`.
    fn heightmap_pos_motion_blocking(&self, pos: BlockPos) -> BlockPos {
        pos
    }
    /// `server.getLevel(gp.dimension()).getPoiManager().getType(gp.pos())`:
    /// the point of interest at a global position, in any loaded level.
    /// The default answers only for this level.
    fn poi_type_at_global(&self, gp: GlobalPos) -> Option<&'static str> {
        if gp.dimension == self.dimension() {
            self.poi_type_at(gp.pos)
        } else {
            None
        }
    }
    /// `BuiltInRegistries.VILLAGER_PROFESSION.listElements().filter(p ->
    /// p.value().heldJobSite().test(poiType)).findFirst()`, by registry
    /// key, in registry order.
    fn profession_holding_job_site(&self, poi_type: &str) -> Option<&'static str> {
        let _ = poi_type;
        None
    }
    /// `poiManager.exists(pos, h -> true)` in the level `gp` names
    /// (`server.getLevel(gp.dimension())`).
    fn poi_exists_at_global(&self, gp: GlobalPos) -> bool {
        self.poi_type_at_global(gp).is_some()
    }
    /// `poiManager.release(pos)` in the level `gp` names; the result is
    /// popped by every caller.
    fn release_poi_at_global(&mut self, gp: GlobalPos) {
        let _ = gp;
    }
    /// `level.getBlockState(pos).is(BlockTags.BEDS) && state.getValue(BedBlock.OCCUPIED)`.
    fn bed_occupied_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `level.getBlockState(pos).is(BlockTags.BEDS)`.
    fn is_bed_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }
    /// `other.getBrain().getMemory(INTERACTION_TARGET)`, as an entity id.
    fn other_interaction_target(&self, id: i32) -> Option<i32> {
        let _ = id;
        None
    }
    /// `setDropChance(EquipmentSlot.MAINHAND, chance)`.
    fn set_drop_chance_main_hand(&mut self, chance: f32) {
        let _ = chance;
    }
    /// `other.getMainHandItem()`; `None` is `ItemStack.EMPTY`.
    fn other_main_hand_item(&self, id: i32) -> Option<ItemSpec> {
        let _ = id;
        None
    }
    /// `Villager.getOffers()`, in offer order.
    fn merchant_offers(&self) -> Vec<MerchantOfferFacts> {
        Vec::new()
    }
    /// `Villager.hasExcessFood()`.
    fn has_excess_food(&self) -> bool {
        false
    }
    /// `other.wantsMoreFood()`.
    fn other_villager_wants_more_food(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Villager.gossip(level, other, gameTime)`.
    fn gossip_with(&mut self, id: i32, game_time: i64) {
        let _ = (id, game_time);
    }
    /// `getVillagerData().profession().value().requestedItems()`, by item id.
    fn requested_items(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// The same for another villager.
    fn other_requested_items(&self, id: i32) -> Vec<&'static str> {
        let _ = id;
        Vec::new()
    }
    /// `getInventory()`'s slots in order; `None` is an empty stack.
    fn inventory_items(&self) -> Vec<Option<ItemSpec>> {
        Vec::new()
    }
    /// `getInventory().getItem(slot).shrink(n)`.
    fn shrink_inventory_slot(&mut self, slot: usize, n: i32) {
        let _ = (slot, n);
    }
    /// `ItemStack.getMaxStackSize()` for a stack, and
    /// `Item.getDefaultMaxStackSize()` for the item alone.
    fn max_stack_size(&self, item: &ItemSpec) -> i32 {
        let _ = item;
        64
    }
    /// `BehaviorUtils.throwItem(this, stack, target)`.
    fn throw_item(&mut self, stack: ItemSpec, target: Vec3) {
        let _ = (stack, target);
    }
    /// `BehaviorUtils.lookAtEntity(other, target)`: sets the other mob's
    /// `LOOK_TARGET` to an `EntityTracker(target, true)`.
    fn other_look_at_entity(&mut self, id: i32, target: i32) {
        let _ = (id, target);
    }
    /// `BehaviorUtils.setWalkAndLookTargetMemories(other, target, speed, close)`.
    fn other_set_walk_and_look_target_to_entity(
        &mut self,
        id: i32,
        target: i32,
        speed: f32,
        close_enough: i32,
    ) {
        let _ = (id, target, speed, close_enough);
    }
    /// `AgeableMob.canBreed()` for this mob and for another.
    fn can_breed(&self) -> bool {
        false
    }
    fn other_can_breed(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Villager.eatAndDigestFood()` for this mob and for another.
    fn eat_and_digest_food(&mut self) {}
    fn other_eat_and_digest_food(&mut self, id: i32) {
        let _ = id;
    }
    /// `getBreedOffspring(level, partner)`: the new child's id, not yet in
    /// the level, or `None`.
    fn breed_offspring(&mut self, partner: i32) -> Option<i32> {
        let _ = partner;
        None
    }
    /// `setAge(age)` for this mob and for another.
    fn set_age(&mut self, age: i32) {
        let _ = age;
    }
    fn other_set_age(&mut self, id: i32, age: i32) {
        let _ = (id, age);
    }
    /// `other.snapTo(x, y, z, yRot, xRot)`.
    fn other_snap_to(&mut self, id: i32, x: f64, y: f64, z: f64, y_rot: f32, x_rot: f32) {
        let _ = (id, x, y, z, y_rot, x_rot);
    }
    /// `level.addFreshEntityWithPassengers(entity)`.
    fn add_fresh_entity_with_passengers(&mut self, id: i32) {
        let _ = id;
    }
    /// `level.broadcastEntityEvent(other, id)`.
    fn other_broadcast_entity_event(&mut self, id: i32, event: u8) {
        let _ = (id, event);
    }
    /// `other.getBrain().setMemory(HOME, gp)`.
    fn other_set_home(&mut self, id: i32, gp: GlobalPos) {
        let _ = (id, gp);
    }
    /// `other.getBrain().setMemory(BREED_TARGET, me)` and
    /// `other.getBrain().eraseMemory(BREED_TARGET)`, the partner's half of
    /// `AnimalMakeLove`.
    fn other_set_breed_target(&mut self, id: i32, me: i32) {
        let _ = (id, me);
    }
    fn other_erase_breed_target(&mut self, id: i32) {
        let _ = id;
    }
    /// `player.hasEffect(MobEffects.HERO_OF_THE_VILLAGE)`.
    fn has_hero_of_the_village(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `Villager.dropFromGiftLootTable(level, table, (level, stack) ->
    /// BehaviorUtils.throwItem(this, stack, target))`: the table is named
    /// by its `BuiltInLootTables` constant (`armorer_gift`,
    /// `unemployed_gift`, `baby_villager_gift`, …).
    fn drop_from_gift_loot_table(&mut self, table: &'static str, target: Vec3) {
        let _ = (table, target);
    }
    /// The raid `raid_at` answered, read again live by its id.
    fn raid_by_id(&self, id: i32) -> Option<RaidFacts> {
        let _ = id;
        None
    }
    /// `Villager.playCelebrateSound()`.
    fn play_celebrate_sound(&mut self) {}
    /// `Projectile.spawnProjectile(new FireworkRocketEntity(level, this, x,
    /// y, z, firework), level, firework)` for a burst of one `DyeColor`
    /// with `flightDuration`.
    fn spawn_firework_rocket(
        &mut self,
        color: &'static str,
        flight_duration: i32,
        x: f64,
        y: f64,
        z: f64,
    ) {
        let _ = (color, flight_duration, x, y, z);
    }
    /// `YieldJobSite.nearbyWantsJobsite(poiType, other, pos)` on another
    /// villager: its `POTENTIAL_JOB_SITE` absent, its profession's
    /// `heldJobSite` accepting `poiType`, and then either its `JOB_SITE`
    /// equal to `pos` or — with no job site — a path from **its**
    /// navigation (`createPath(pos, poiType.validRange())`) that can
    /// reach. The path search runs on the other mob.
    fn other_villager_wants_job_site(&mut self, id: i32, poi_type: &str, pos: BlockPos) -> bool {
        let _ = (id, poi_type, pos);
        false
    }
    /// `other.getBrain().getMemory(JOB_SITE)`.
    fn other_villager_job_site(&self, id: i32) -> Option<GlobalPos> {
        let _ = id;
        None
    }
    /// `other.getVillagerData().profession().value().heldJobSite().test(poiType)`.
    fn other_villager_profession_holds(&self, id: i32, poi_type: &str) -> bool {
        let _ = (id, poi_type);
        false
    }
    /// `other.getVillagerXp()`.
    fn other_villager_xp(&self, id: i32) -> i32 {
        let _ = id;
        0
    }
    /// `other.getBrain().eraseMemory(JOB_SITE)`.
    fn other_villager_erase_job_site(&mut self, id: i32) {
        let _ = id;
    }
    /// `BehaviorUtils.setWalkAndLookTargetMemories(other, pos, speed, closeEnough)`.
    fn other_set_walk_and_look_target(
        &mut self,
        id: i32,
        pos: BlockPos,
        speed: f32,
        close_enough: i32,
    ) {
        let _ = (id, pos, speed, close_enough);
    }
    /// `other.getBrain().setMemory(POTENTIAL_JOB_SITE, gp)`.
    fn other_set_potential_job_site(&mut self, id: i32, gp: GlobalPos) {
        let _ = (id, gp);
    }

    // ---- sixth lane: the sniffer ----

    /// `Sniffer.transitionTo(State)`. The return is `this`, discarded at
    /// every one of the nine call sites in `SnifferAi`.
    fn sniffer_transition_to(&mut self, state: SnifferState) {
        let _ = state;
    }
    /// `Sniffer.isTempted()`: `brain.getMemory(IS_TEMPTED).orElse(false)`.
    /// It reads the brain through `this.brain`, which this port keeps
    /// beside the mob rather than inside it, so the host answers.
    fn sniffer_is_tempted(&self) -> bool {
        false
    }
    /// `Sniffer.canSniff()` (0..54): `!isTempted() && !isPanicking() &&
    /// !isInWater() && !isInLove() && onGround() && !isPassenger() &&
    /// !isLeashed()`.
    fn sniffer_can_sniff(&self) -> bool {
        false
    }
    /// `Sniffer.canDig()` (0..61): `!isPanicking() && !isTempted() &&
    /// !isBaby() && !isInWater() && onGround() && !isPassenger() &&
    /// canDig(getHeadBlock().below())`, where the private
    /// `canDig(BlockPos)` (0..77) is the `SNIFFER_DIGGABLE_BLOCK` tag,
    /// `getExploredPositions().noneMatch(...)` and a navigation path that
    /// `canReach()`.
    fn sniffer_can_dig(&self) -> bool {
        false
    }
    /// `Sniffer.calculateDigPosition()` — the `Optional<BlockPos>`
    /// `SnifferAi$Sniffing.stop` feeds to its `ifPresent` lambda.
    fn sniffer_calculate_dig_position(&mut self) -> Option<BlockPos> {
        None
    }
    /// `Sniffer.onDiggingComplete(boolean)`; the return is `this`,
    /// discarded.
    fn sniffer_on_digging_complete(&mut self, complete: bool) {
        let _ = complete;
    }

    // ---- sixth lane: shared ----

    /// `LivingEntity.getEffect(effect) != null` / `hasEffect(effect)`, by
    /// the effect's registry id (`levitation`, `jump_boost`).
    fn has_effect(&self, effect: &'static str) -> bool {
        let _ = effect;
        false
    }
    /// `getPose()`. [`MobHost::has_pose`] is the equality test the
    /// behaviours usually want; this is for the ones that compare against
    /// two poses in a row.
    fn pose(&self) -> Pose {
        Pose::Standing
    }
    /// `other.yHeadRot`, the field `BreezeUtil.randomPointBehindTarget`
    /// reads off its target.
    fn entity_y_head_rot(&self, id: i32) -> f32 {
        let _ = id;
        0.0
    }
    /// `mob.lookAt(EntityAnchorArgument.Anchor.EYES, target)`.
    fn look_at_eyes(&mut self, target: Vec3) {
        let _ = target;
    }
    /// `level.playSound(null, mob, sound, source, volume, pitch)` — the
    /// level's own play, which is not `Entity.playSound`; `source` is the
    /// `SoundSource` constant's name (`hostile`).
    fn level_play_sound(
        &mut self,
        sound: &SoundSpec,
        source: &'static str,
        volume: f32,
        pitch: f32,
    ) {
        let _ = (sound, source, volume, pitch);
    }

    // ---- sixth lane: the breeze ----

    /// `Breeze.withinInnerCircleRange(Vec3)`.
    fn breeze_within_inner_circle_range(&self, v: Vec3) -> bool {
        let _ = v;
        false
    }
    /// `Breeze.getFiringYPosition()`.
    fn breeze_firing_y_position(&self) -> f64 {
        self.y()
    }
    /// `Projectile.spawnProjectileUsingShoot(new BreezeWindCharge(this,
    /// level), level, ItemStack.EMPTY, dx, dy, dz, velocity, inaccuracy)`.
    ///
    /// **This hides RNG.** `Projectile.shoot` applies the inaccuracy with
    /// three `nextGaussian()` draws from the projectile's own random, so
    /// the draw count of `Shoot.tick` is not observable from this crate;
    /// a host that spawns a real wind charge takes them.
    fn breeze_shoot_wind_charge(
        &mut self,
        dx: f64,
        dy: f64,
        dz: f64,
        velocity: f32,
        inaccuracy: f32,
    ) {
        let _ = (dx, dy, dz, velocity, inaccuracy);
    }
    /// `LongJump.snapToSurface(LivingEntity, Vec3)` (0..118): clip straight
    /// down 10 blocks from `from` with `ClipContext(COLLIDER, NONE, mob)`
    /// and, on a `BLOCK` hit, answer
    /// `BlockPos.containing(hit.getLocation()).above()`; otherwise clip
    /// straight *up* 10 and do the same; otherwise `null`.
    ///
    /// **Not verifiable from this crate.** It needs `Level.clip`'s exact
    /// hit *location*, which only a real voxel trace against real collision
    /// shapes produces; [`PathLevel::clip_misses`] answers only whether a
    /// ray is clear. So this is a host hook rather than a transcription,
    /// and the bytecode above is the specification the host owes.
    fn snap_to_surface(&self, from: Vec3) -> Option<BlockPos> {
        let _ = from;
        None
    }
    /// `mob.getType().isBlockDangerous(level.getBlockState(pos))`.
    fn entity_type_is_block_dangerous_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }

    // ---- sixth lane: the armadillo ----

    /// `Armadillo.getState()`.
    fn armadillo_state(&self) -> ArmadilloState {
        ArmadilloState::Idle
    }
    /// `Armadillo.switchToState(ArmadilloState)`.
    fn armadillo_switch_to_state(&mut self, state: ArmadilloState) {
        let _ = state;
    }
    /// `Armadillo.isScared()`.
    fn armadillo_is_scared(&self) -> bool {
        false
    }
    /// `Armadillo.shouldSwitchToScaredState()`.
    fn armadillo_should_switch_to_scared_state(&self) -> bool {
        false
    }
    /// `Armadillo.rollUp()`.
    fn armadillo_roll_up(&mut self) {}
    /// `Armadillo.rollOut()`.
    fn armadillo_roll_out(&mut self) {}

    // ---- sixth lane: the camel ----

    /// `Camel.standUpInstantly()`.
    fn camel_stand_up_instantly(&mut self) {}
    /// `Camel.standUp()`.
    fn camel_stand_up(&mut self) {}
    /// `Camel.sitDown()`.
    fn camel_sit_down(&mut self) {}
    /// `Camel.isCamelSitting()`.
    fn camel_is_sitting(&self) -> bool {
        false
    }
    /// `Camel.canCamelChangePose()`.
    fn camel_can_change_pose(&self) -> bool {
        false
    }
    /// `Camel.getPoseTime()` — ticks in the current pose.
    fn camel_pose_time(&self) -> i64 {
        0
    }

    // ---- sixth lane: the creaking ----

    /// `Creaking.canMove()`.
    fn creaking_can_move(&self) -> bool {
        false
    }

    // ---- sixth lane: the goat's ram ----

    /// `LivingEntity.getVoicePitch()`.
    fn voice_pitch(&self) -> f32 {
        1.0
    }
    /// `getEffect(effect).getAmplifier()`, `None` when the effect is
    /// absent — which is how `RamTarget.tick` writes
    /// `hasEffect(e) ? getEffect(e).getAmplifier() + 1 : 0`.
    fn effect_amplifier(&self, effect: &'static str) -> Option<i32> {
        let _ = effect;
        None
    }
    /// `target.hurtServer(level, level.damageSources().<kind>(this), amount)`
    /// followed, on `true`, by
    /// `EnchantmentHelper.doPostAttackEffects(level, target, source)`.
    /// `kind` is the `DamageSources` method name.
    fn hurt_server(&mut self, target: i32, kind: &'static str, amount: f32) -> bool {
        let _ = (target, kind, amount);
        false
    }
    /// `target.applyItemBlocking(level, source, amount)`.
    fn apply_item_blocking(&mut self, target: i32, kind: &'static str, amount: f32) -> f32 {
        let _ = (target, kind, amount);
        0.0
    }
    /// `target.knockback(strength, x, z, source, amount)`.
    fn knockback(
        &mut self,
        target: i32,
        strength: f64,
        x: f64,
        z: f64,
        kind: &'static str,
        amount: f32,
    ) {
        let _ = (target, strength, x, z, kind, amount);
    }
    /// `Goat.dropHorn()`.
    fn goat_drop_horn(&mut self) -> bool {
        false
    }
    /// `mob.getAttributeValue(Attributes.ATTACK_DAMAGE)`.
    fn attack_damage(&self) -> f64 {
        0.0
    }

    // ---- sixth lane: ChargeAttack ----

    /// `mob.lookAt(Entity, float yMaxRot, float xMaxRot)`.
    fn look_at_entity(&mut self, id: i32, y_max_rot: f32, x_max_rot: f32) {
        let _ = (id, y_max_rot, x_max_rot);
    }
    /// `mob.causeExtraKnockback(Entity target, float strength, Vec3 delta,
    /// DamageSource source, float amount, boolean flag)`.
    fn cause_extra_knockback(
        &mut self,
        target: i32,
        strength: f32,
        delta: Vec3,
        kind: &'static str,
        amount: f32,
        flag: bool,
    ) {
        let _ = (target, strength, delta, kind, amount, flag);
    }

    // ---- sixth lane: the frog's tongue ----

    /// `Frog.setTongueTarget(Entity)`.
    fn frog_set_tongue_target(&mut self, id: i32) {
        let _ = id;
    }
    /// `Frog.eraseTongueTarget()`.
    fn frog_erase_tongue_target(&mut self) {}
    /// `Frog.getTongueTarget()`.
    fn frog_tongue_target(&self) -> Option<i32> {
        None
    }
    /// The static `Frog.canEat(LivingEntity)`.
    fn frog_can_eat(&self, id: i32) -> bool {
        let _ = id;
        false
    }
    /// `other.setDeltaMovement(Vec3)` on another entity.
    fn set_other_delta_movement(&mut self, id: i32, v: Vec3) {
        let _ = (id, v);
    }
    /// `other.remove(Entity.RemovalReason.KILLED)`.
    fn remove_entity_killed(&mut self, id: i32) {
        let _ = id;
    }

    // ---- seventh lane: the container host surface ----
    //
    // `TransportItemsBetweenContainers` is the only behaviour in this crate
    // that reads block entities and writes containers, and every call below
    // exists because one of its statements makes it. Nothing here models a
    // block state: the class only ever asks four questions of one
    // (`instanceof ChestBlockEntity`, `getValueOrElse(TYPE, SINGLE)`,
    // `getCollisionShape().bounds()` and the two predicates the provider
    // supplies), so those are the four calls rather than a state type.

    /// `((LevelChunk) level.getChunkSource().getChunkNow(x, z))
    /// .getBlockEntities().values()`, as the positions in that collection's
    /// own iteration order. `None` is `getChunkNow` returning null — an
    /// unloaded chunk, which `getTransportTarget` skips entirely rather
    /// than loading.
    ///
    /// **The order is not insertion order and not sorted.**
    /// `ChunkAccess.blockEntities` is a fastutil
    /// `Object2ObjectOpenHashMap<BlockPos, BlockEntity>`, so `values()`
    /// walks its open-addressed table — a function of each key's hash, the
    /// table's capacity and the chunk's whole insertion and removal
    /// history. The one thing the caller asks of it is the tie-break:
    /// `getTransportTarget` keeps a candidate only on a strict
    /// `distToCenterSqr < best`, so two containers exactly equidistant from
    /// the mob resolve to whichever this collection yields first. A host
    /// that wants that tie to match vanilla has to reproduce that map, not
    /// merely the set of positions.
    fn chunk_block_entity_positions(&self, chunk_x: i32, chunk_z: i32) -> Option<Vec<BlockPos>> {
        let _ = (chunk_x, chunk_z);
        None
    }

    /// `level.getBlockEntity(pos)`'s registry id (`minecraft:chest`,
    /// `minecraft:barrel`, ...), or `None` where there is no block entity.
    /// `getTransportTarget`'s scan filters on `instanceof
    /// ChestBlockEntity`, which this crate expresses as this id being one
    /// of the chest ids.
    fn block_entity_kind(&self, pos: BlockPos) -> Option<&'static str> {
        let _ = pos;
        None
    }

    /// A token identifying **the block entity object** at `pos`, so that
    /// `targetHasNotChanged`'s `target.blockEntity.equals(level
    /// .getBlockEntity(target.pos))` can be answered. `BlockEntity` does
    /// not override `equals`, so that is reference identity: a chest broken
    /// and replaced at the same position is a different object and the
    /// target goes invalid even though the position and the kind match.
    /// `None` where there is no block entity.
    fn block_entity_identity(&self, pos: BlockPos) -> Option<u64> {
        let _ = pos;
        None
    }

    /// `TransportItemTarget.getBlockEntityContainer(be, state, level, pos)`
    /// (0..48): for a `ChestBlock` it is
    /// `ChestBlock.getContainer(block, state, level, pos, false)` — which
    /// is the double-chest `CompoundContainer` when the chest is paired,
    /// and null when the chest is blocked; otherwise the block entity
    /// itself when it is a `Container`, and null when it is not.
    ///
    /// `false` is what makes it null for a blocked chest: the parameter is
    /// `overrideBlockedCheck`.
    ///
    /// Answering `true` is what makes a scanned position a possible target
    /// at all; every read and write below is then keyed on the same `pos`.
    fn transport_container_exists(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }

    /// `blockEntity instanceof BaseContainerBlockEntity b && b.isLocked()`,
    /// the whole of `isContainerLocked`.
    fn container_is_locked(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }

    /// `ChestBlock.isChestBlockedAt(level, pos)` (0..21): the block above
    /// is a redstone conductor, or a cat is sitting on it.
    fn chest_is_blocked_at(&self, pos: BlockPos) -> bool {
        let _ = pos;
        false
    }

    /// `ChestBlock.getConnectedBlockPos(pos, state)` (0..10) —
    /// `pos.relative(getConnectedDirection(state))`, where the direction is
    /// `FACING` turned clockwise for `TYPE == LEFT` and counter-clockwise
    /// otherwise. `None` is `state.getValueOrElse(TYPE, SINGLE) == SINGLE`,
    /// which includes every block that has no `TYPE` property at all:
    /// `getConnectedTargets` then yields the single target unchanged.
    fn chest_connected_pos(&self, pos: BlockPos) -> Option<BlockPos> {
        let _ = pos;
        None
    }

    /// `state.getCollisionShape(level, pos).bounds()`, in the block's own
    /// local coordinates — `isWithinTargetDistance` inflates it and only
    /// then `move`s it to the position.
    fn collision_shape_bounds(&self, pos: BlockPos) -> Aabb {
        let _ = pos;
        Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0)
    }

    /// `level.clip(new ClipContext(from, to, Block.COLLIDER, Fluid.NONE,
    /// mob))`, reduced to the one thing `canSeeAnyTargetSide` asks of the
    /// result: the hit block's position when `getType() == BLOCK`, and
    /// `None` on a `MISS`. This crate's [`Level::clip_misses`] answers only
    /// whether a ray is clear, which cannot distinguish "hit the target" from
    /// "hit something else".
    fn clip_block_hit(&self, from: Vec3, to: Vec3) -> Option<BlockPos> {
        let _ = (from, to);
        None
    }

    /// `container.isEmpty()`: every slot's stack is empty.
    fn container_is_empty(&self, pos: BlockPos) -> bool {
        let _ = pos;
        true
    }

    /// `container.iterator()` — the stacks in slot order, an empty slot as
    /// `None`. `Container` is `Iterable<ItemStack>` and iterates
    /// `0..getContainerSize()`.
    fn container_items(&self, pos: BlockPos) -> Vec<Option<ItemSpec>> {
        let _ = pos;
        Vec::new()
    }

    /// `container.removeItem(slot, count)`: splits at most `count` off the
    /// slot and returns what came out.
    fn container_remove_item(
        &mut self,
        pos: BlockPos,
        slot: usize,
        count: i32,
    ) -> Option<ItemSpec> {
        let _ = (pos, slot, count);
        None
    }

    /// `container.setItem(slot, stack)`.
    fn container_set_item(&mut self, pos: BlockPos, slot: usize, stack: Option<ItemSpec>) {
        let _ = (pos, slot, stack);
    }

    /// `container.setChanged()`.
    fn container_set_changed(&mut self, pos: BlockPos) {
        let _ = pos;
    }

    /// `mob.getMainHandItem()`. [`MobHost::set_main_hand_item`] is the
    /// setter this pairs with.
    fn main_hand_item(&self) -> Option<ItemSpec> {
        None
    }

    /// `mob.setGuaranteedDrop(EquipmentSlot.MAINHAND)` — the only slot
    /// this crate's callers name.
    fn set_guaranteed_drop_main_hand(&mut self) {}

    /// The half of `PathfinderMob.isPanicking()` (0..92) this crate cannot
    /// see: after the brain answers no, it walks
    /// `goalSelector.getAvailableGoals()` and answers true for the first
    /// **running** `WrappedGoal` whose goal is a `PanicGoal`. The brain
    /// half — `!brain.isBrainDead() && brain.hasMemoryValue(IS_PANICKING)
    /// && brain.getMemory(IS_PANICKING).isPresent()` — is read off
    /// `BrainState` by the caller, so only this fallback is a host call.
    fn has_running_panic_goal(&self) -> bool {
        false
    }
}

/// `net.minecraft.world.entity.animal.armadillo.Armadillo$ArmadilloState`,
/// in `<clinit>` order. The private constructor is
/// `(String name, boolean isThreatened, int animationDuration, int id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArmadilloState {
    /// `IDLE("idle", false, 0, 0)`.
    #[default]
    Idle,
    /// `ROLLING("rolling", true, 10, 1)`.
    Rolling,
    /// `SCARED("scared", true, 50, 2)`.
    Scared,
    /// `UNROLLING("unrolling", true, 30, 3)`.
    Unrolling,
}

impl ArmadilloState {
    /// `getSerializedName()`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ArmadilloState::Idle => "idle",
            ArmadilloState::Rolling => "rolling",
            ArmadilloState::Scared => "scared",
            ArmadilloState::Unrolling => "unrolling",
        }
    }
    /// `isThreatened()`.
    #[must_use]
    pub fn is_threatened(self) -> bool {
        self != ArmadilloState::Idle
    }
    /// `animationDuration()`.
    #[must_use]
    pub fn animation_duration(self) -> i32 {
        match self {
            ArmadilloState::Idle => 0,
            ArmadilloState::Rolling => 10,
            ArmadilloState::Scared => 50,
            ArmadilloState::Unrolling => 30,
        }
    }
    /// The `id` the constructor's fourth argument gives.
    #[must_use]
    pub fn id(self) -> i32 {
        match self {
            ArmadilloState::Idle => 0,
            ArmadilloState::Rolling => 1,
            ArmadilloState::Scared => 2,
            ArmadilloState::Unrolling => 3,
        }
    }
}

/// `net.minecraft.world.entity.animal.sniffer.Sniffer$State`, in the jar's
/// declaration order — which is the `id` the private constructor takes and
/// the order `BY_ID` indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SnifferState {
    #[default]
    Idling,
    FeelingHappy,
    Scenting,
    Sniffing,
    Searching,
    Digging,
    Rising,
}

/// `net.minecraft.world.Difficulty`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Difficulty {
    Peaceful,
    Easy,
    Normal,
    Hard,
}

/// An item, named by its registry id, for goals that hold one
/// (`UseItemGoal`'s `ItemStack` is a template it copies into the main hand).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemSpec {
    /// `minecraft:` resource id, e.g. `potion` or `milk_bucket`.
    pub id: &'static str,
    /// `ItemStack.getCount()`; every table so far uses 1.
    pub count: i32,
    /// A potion or other component the table names, verbatim from the
    /// bytecode, or `None`.
    pub component: Option<&'static str>,
}

/// A sound event, named by its registry id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundSpec(pub &'static str);

/// `net.minecraft.world.entity.Pose`, the values the brain behaviours set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pose {
    /// `Pose.STANDING`.
    #[default]
    Standing,
    /// `Pose.LONG_JUMPING`.
    LongJumping,
    /// `Pose.SITTING`.
    Sitting,
    /// `Pose.SNIFFING`.
    Sniffing,
    /// `Pose.DIGGING`.
    Digging,
    /// `Pose.EMERGING`.
    Emerging,
    /// `Pose.ROARING`.
    Roaring,
    /// `Pose.SHOOTING`.
    Shooting,
    /// `Pose.INHALING`.
    Inhaling,
    /// `Pose.SLIDING`.
    Sliding,
    /// `Pose.CROAKING`.
    Croaking,
    /// `Pose.USING_TONGUE`.
    UsingTongue,
}

/// A `TagKey<DamageType>` a brain behaviour tests a `DamageSource` against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DamageTypeTag {
    /// `DamageTypeTags.PANIC_CAUSES`.
    PanicCauses,
    /// `DamageTypeTags.PANIC_ENVIRONMENTAL_CAUSES`.
    PanicEnvironmentalCauses,
}

/// What `ServerLevel.getRaidAt(pos)` answers, as the villager behaviours
/// read it: `Raid.isStopped()`, `isLoss()`, `isVictory()`,
/// `hasFirstWaveSpawned()`, `isBetweenWaves()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RaidFacts {
    /// An identity for `raid_by_id`.
    pub id: i32,
    pub stopped: bool,
    pub loss: bool,
    pub victory: bool,
    pub has_first_wave_spawned: bool,
    pub between_waves: bool,
}

/// `VillagerData` as the brain reads it: `profession()` by registry key
/// (`"none"`, `"nitwit"`, `"farmer"`, …) and `level()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VillagerDataFacts {
    pub profession: &'static str,
    pub level: i32,
}

/// `PoiManager.Occupancy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PoiOccupancy {
    HasSpace,
    IsOccupied,
    Any,
}

/// A `PoiRecord` as the brain reads it: `getPoiType()` by registry key
/// (`"home"`, `"meeting"`, `"farmer"`, …) and `getPos()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoiRecordFacts {
    pub poi_type: &'static str,
    pub pos: BlockPos,
}

/// A `MerchantOffer` as `ShowTradesToPlayer` reads it: `getCostA()`,
/// `getCostB()` (`None` is empty), `assemble()` and `isOutOfStock()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerchantOfferFacts {
    pub cost_a: Option<ItemSpec>,
    pub cost_b: Option<ItemSpec>,
    pub result: ItemSpec,
    pub out_of_stock: bool,
}
