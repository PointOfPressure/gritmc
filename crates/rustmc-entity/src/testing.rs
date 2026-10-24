//! Test fixtures: a grid level and a plain mob that answer [`PathLevel`]
//! and [`MobHost`] from fields, so the pathfinder, the navigation and the
//! goals can be driven without a server. Public because the crate's
//! integration tests and goldens use them; nothing in the server does.

use std::collections::HashMap;

use rustmc_data::BlockPos;
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

use crate::host::{
    BlockFacts, Difficulty, EntityClass, EntityFacts, ItemSpec, MobHost, PathLevel, SoundSpec,
};
use crate::pathfinder::PathType;
use crate::phys::{Aabb, Vec3};

/// A level made of [`BlockFacts`] at explicit positions, air everywhere
/// else. A solid block is a full cube for collision purposes.
#[derive(Debug, Clone)]
pub struct GridLevel {
    pub blocks: HashMap<BlockPos, BlockFacts>,
    pub min_y: i32,
    pub max_y: i32,
    pub game_time: i64,
    /// Chunks reported as loaded; `None` means every chunk is.
    pub loaded_chunks: Option<Vec<(i32, i32)>>,
    pub bright_outside: bool,
}

impl Default for GridLevel {
    fn default() -> Self {
        Self::new()
    }
}

impl GridLevel {
    #[must_use]
    pub fn new() -> Self {
        GridLevel {
            blocks: HashMap::new(),
            min_y: -64,
            max_y: 319,
            game_time: 0,
            loaded_chunks: None,
            bright_outside: false,
        }
    }

    /// Set one block.
    pub fn set(&mut self, x: i32, y: i32, z: i32, f: BlockFacts) {
        self.blocks.insert(BlockPos { x, y, z }, f);
    }

    /// Fill a box (inclusive) with one block.
    #[allow(clippy::too_many_arguments)]
    pub fn fill(&mut self, x0: i32, y0: i32, z0: i32, x1: i32, y1: i32, z1: i32, f: BlockFacts) {
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    self.set(x, y, z, f);
                }
            }
        }
    }

    /// A flat stone floor at `y = floor_y` over the inclusive x/z box.
    pub fn floor(&mut self, x0: i32, z0: i32, x1: i32, z1: i32, floor_y: i32) {
        self.fill(x0, floor_y, z0, x1, floor_y, z1, BlockFacts::STONE);
    }

    fn is_solid_at(&self, p: BlockPos) -> bool {
        self.blocks.get(&p).is_some_and(|f| f.is_solid)
    }
}

impl PathLevel for GridLevel {
    fn block_facts(&self, pos: BlockPos) -> BlockFacts {
        *self.blocks.get(&pos).unwrap_or(&BlockFacts::AIR)
    }

    fn min_y(&self) -> i32 {
        self.min_y
    }

    fn max_y(&self) -> i32 {
        self.max_y
    }

    fn collision_shape_max_y(&self, pos: BlockPos) -> Option<f64> {
        if self.is_solid_at(pos) {
            Some(1.0)
        } else {
            None
        }
    }

    fn game_time(&self) -> i64 {
        self.game_time
    }
    fn is_bright_outside(&self) -> bool {
        self.bright_outside
    }

    fn can_see_sky(&self, pos: BlockPos) -> bool {
        !self
            .blocks
            .iter()
            .any(|(p, f)| f.is_solid && p.x == pos.x && p.z == pos.z && p.y > pos.y)
    }

    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        match &self.loaded_chunks {
            None => true,
            Some(v) => v.contains(&(chunk_x, chunk_z)),
        }
    }

    /// Any solid full cube whose unit box strictly intersects `aabb` is a
    /// collision.
    fn no_collision(&self, _mob_id: i32, aabb: &Aabb) -> bool {
        use rustmc_worldgen::math::floor;
        let x0 = floor(aabb.min_x) - 1;
        let y0 = floor(aabb.min_y) - 1;
        let z0 = floor(aabb.min_z) - 1;
        let x1 = floor(aabb.max_x) + 1;
        let y1 = floor(aabb.max_y) + 1;
        let z1 = floor(aabb.max_z) + 1;
        for x in x0..=x1 {
            for y in y0..=y1 {
                for z in z0..=z1 {
                    if self.is_solid_at(BlockPos { x, y, z }) {
                        let b = Aabb::new(
                            f64::from(x),
                            f64::from(y),
                            f64::from(z),
                            f64::from(x) + 1.0,
                            f64::from(y) + 1.0,
                            f64::from(z) + 1.0,
                        );
                        if b.intersects(aabb) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }

    /// A straight ray, sampled every 1/8 block, misses when it enters no
    /// solid block — enough for the navigation's clear-movement test.
    fn clip_misses(&self, _mob_id: i32, from: Vec3, to: Vec3, _fluids: bool) -> bool {
        let d = to.subtract(from);
        let len = d.length();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let steps = (len * 8.0).ceil().max(1.0) as usize;
        for i in 0..=steps {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f64 / steps as f64;
            let p = from.add3(d.x * t, d.y * t, d.z * t);
            if self.is_solid_at(crate::phys::block_pos_containing_vec(p)) {
                return false;
            }
        }
        true
    }
}

/// A mob whose every `MobHost` answer is a field.
#[derive(Debug, Clone)]
pub struct TestMob {
    pub level: GridLevel,
    pub id: i32,
    pub tick_count: i32,
    pub random: LegacyRandomSource,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub xo: f64,
    pub zo: f64,
    pub eye_height: f64,
    pub bb_width: f32,
    pub bb_height: f32,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_head_rot: f32,
    pub y_body_rot: f32,
    pub speed: f32,
    pub zza: f32,
    pub xxa: f32,
    pub yya: f32,
    pub jumping: bool,
    pub on_ground: bool,
    pub in_water: bool,
    /// `level.getDayTime()`.
    pub day_time: i64,
    /// `release_poi` calls, in order.
    pub released_pois: Vec<crate::brain::memory::MemoryModuleType>,
    /// Damage type names `damage_type_is(_, PanicCauses)` answers true for.
    pub panic_damage_kinds: Vec<&'static str>,
    /// `entity_by_uuid`: `(id, uuid)` pairs.
    pub uuids: Vec<(i32, u128)>,
    /// `forgive_dead_players()`.
    pub forgive_dead_players: bool,
    /// `can_pick_up_loot()`.
    pub can_pick_up_loot: bool,
    /// Positions `world_border_contains` refuses.
    pub outside_world_border: Vec<BlockPos>,
    /// `last_damage_source()`.
    pub last_damage_source: Option<crate::brain::memory::DamageSourceFacts>,
    /// `usable_projectile_weapon_range()`.
    pub projectile_weapon_range: Option<i32>,
    /// `is_holding_usable_non_melee_weapon()`.
    pub holding_usable_non_melee_weapon: bool,
    /// `hasPassenger(e)` for these ids.
    pub passengers: Vec<i32>,
    /// `entity_type_matches_mine(e)` for these ids.
    pub same_type_entities: Vec<i32>,
    /// `entity_is_coming_through_door(e, pos)` pairs.
    pub coming_through_door: Vec<(i32, BlockPos)>,
    /// `entity_kind_is(e, kind)` pairs.
    pub entity_kinds: Vec<(i32, &'static str)>,
    /// `entity_type_tag_is(e, tag)` pairs.
    pub entity_type_tags: Vec<(i32, &'static str)>,
    /// `cube_mob_size(e)` pairs.
    pub cube_sizes: Vec<(i32, i32)>,
    /// Ids `can_target_entity` answers true for.
    pub targetable: Vec<i32>,
    /// Ids `is_scared_by` answers true for.
    pub scary: Vec<i32>,
    /// `can_stay_rolled_up()`.
    pub can_stay_rolled_up: bool,
    /// `gravity()`.
    pub gravity: f64,
    /// `jump_boost_power()`.
    pub jump_boost_power: f32,
    /// `jump_strength()`.
    pub jump_strength: f64,
    /// `set_discard_friction` writes here.
    pub discard_friction: bool,
    /// `set_pose` writes here.
    pub pose: crate::host::Pose,
    /// Positions `is_hoglin_repellent_at` answers true for.
    pub hoglin_repellents: Vec<rustmc_data::BlockPos>,
    /// `block_tag_is(pos, tag)` pairs.
    pub block_tags: Vec<(rustmc_data::BlockPos, &'static str)>,
    /// Positions every named `acceptable_landing_spot` answers true for.
    pub landing_spots: Vec<rustmc_data::BlockPos>,
    /// Positions whose `getFluidState()` is NOT empty.
    pub wet_blocks: Vec<rustmc_data::BlockPos>,
    /// Positions `is_face_sturdy_up` answers true for.
    pub sturdy_up: Vec<rustmc_data::BlockPos>,
    /// Positions whose up face shape is NOT empty.
    pub solid_face_up: Vec<rustmc_data::BlockPos>,
    /// `fluid_tag_is(pos, tag)` pairs.
    pub fluid_tags: Vec<(rustmc_data::BlockPos, &'static str)>,
    /// `stay_close_target(name)` pairs.
    pub stay_close_targets: Vec<(&'static str, crate::brain::memory::PositionTracker)>,
    /// Names `stay_close_can_travel_to` answers true for.
    pub stay_close_travelable: Vec<&'static str>,
    /// `item_entity_stack(e)` pairs.
    pub item_entities: Vec<(i32, ItemSpec)>,
    /// `offhand_item()`.
    pub offhand: Option<ItemSpec>,
    /// Item ids `item_blocks_attacks` answers true for.
    pub blocks_attacks: Vec<&'static str>,
    /// Item ids `piglin_is_loved_item` answers true for.
    pub loved_items: Vec<&'static str>,
    /// Ids `other_is_angry` answers true for.
    pub angry_others: Vec<i32>,
    /// `warden_entity_angry_at()`.
    pub angry_at: Option<i32>,
    /// `is_removed()` / `remove_discarded()`.
    pub removed: bool,
    /// `entity_category_is(e, category)` pairs.
    pub entity_categories: Vec<(i32, crate::brain::behaviors::MobCategory)>,
    pub in_lava: bool,
    pub in_liquid: bool,
    pub is_passenger: bool,
    pub affected_by_fluids: bool,
    pub on_fire: bool,
    pub water_fluid_height: f64,
    pub fluid_jump_threshold: f64,
    pub max_up_step: f32,
    pub max_fall_distance: i32,
    pub movement_speed: f64,
    pub follow_range: f64,
    pub malus_overrides: HashMap<PathType, f32>,
    pub controlling_passenger_is_mob: bool,
    pub has_controlling_passenger: bool,
    pub vehicle_is_boat: bool,
    pub first_passenger_is_mob: bool,
    /// `getLastDamageSource().is(PANIC_CAUSES)`.
    pub panics: bool,
    /// `homeRadius` / `homePosition`.
    pub home_radius: i32,
    pub home_position: BlockPos,
    /// Entity ids this mob has line of sight to.
    pub visible: Vec<i32>,
    /// Other entities, in `players()` / `getEntitiesOfClass` order, with
    /// the class each belongs to.
    pub others: Vec<(EntityClass, EntityFacts)>,
    /// `Mob.target`.
    pub target_raw: Option<i32>,
    pub aggressive: bool,
    pub hurt: Vec<i32>,
    pub last_hurt_by_mob: Option<i32>,
    pub last_hurt_by_mob_timestamp: i32,
    pub in_love: bool,
    pub age: i32,
    pub mates: Vec<i32>,
    pub bred_with: Vec<i32>,
    pub tempting: Vec<i32>,
    /// Players `player_holding_temptation` answers true for, whatever the
    /// [`Temptations`] kind — the kind reads item tags, which are a host
    /// concern outside this crate.
    pub temptation_holders: Vec<i32>,
    /// `tempt_range()`.
    pub tempt_range: f64,
    pub melee_reach: Vec<i32>,
    pub allies: Vec<i32>,
    pub peaceful: bool,
    pub visibility: HashMap<i32, f64>,
    /// Every `setJumping`, `setSpeed`, `setZza`, `setXxa` call, in order.
    pub calls: Vec<String>,
    // ---- second lane ----
    pub delta_movement: Vec3,
    pub was_in_powder_snow: bool,
    pub in_powder_snow: bool,
    pub powder_snow_walkable: bool,
    pub tame: bool,
    pub ordered_to_sit: bool,
    pub owner: Option<i32>,
    pub owner_in_other_level: bool,
    pub in_sitting_pose: bool,
    pub leashed: bool,
    pub is_vehicle: bool,
    pub first_passenger: Option<i32>,
    pub last_hurt_by: HashMap<i32, i32>,
    pub temper: i32,
    pub max_temper: i32,
    pub horse_tamed: bool,
    pub using_item: bool,
    pub main_hand: Option<ItemSpec>,
    pub snaps: Vec<(f64, f64, f64)>,
    pub universal_anger: bool,
    /// id -> (lastHurtMob, lastHurtMobTimestamp).
    pub last_hurt_mob: HashMap<i32, (Option<i32>, i32)>,
    /// id -> lastHurtByMobTimestamp (the attacker is `last_hurt_by`).
    pub last_hurt_by_ts: HashMap<i32, i32>,
    pub wants_to_attack_denied: Vec<i32>,
    pub active_raid: bool,
    /// (villager, player) -> reputation.
    pub reputation: HashMap<(i32, i32), i32>,
    pub alive: bool,
    pub hurt_marked: bool,
    pub trading_player: Option<i32>,
    pub swell_dir: i32,
    pub head_slot_empty: bool,
    pub x_rot_o: f32,
    pub y_rot_o: f32,
    pub moves: i32,
    pub ambient_stand_sound: Option<SoundSpec>,
    pub ambient_stand_interval: i32,
    pub immobile: bool,
    pub air_supply: i32,
    pub village_sections: Vec<(i32, i32, i32)>,
    pub gift_candidates: Vec<i32>,
    pub accepts_gift: Vec<i32>,
    pub horizontal_collision: bool,
    pub mob_griefing: bool,
    pub difficulty: Difficulty,
    pub baby: bool,
    pub swinging: bool,
    pub interesting_players: Vec<i32>,
    pub holding_bow: bool,
    pub ticks_using_item: i32,
    pub controlled_vehicle_mob: Option<i32>,
    /// id -> (schoolSize, maxSchoolSize, leader).
    pub fish: HashMap<i32, (i32, i32, Option<i32>)>,
    pub can_sit_on_shoulder: bool,
    pub shoulder_accepts: bool,
    pub flying_players: Vec<i32>,
    pub wet: Vec<i32>,
    pub snowy: Vec<i32>,
    /// vehicle id -> controlling player id.
    pub riders: HashMap<i32, i32>,
    pub moved_recently: Vec<i32>,
    pub lying: bool,
    pub raid_over: bool,
    pub raid_active: bool,
    pub raid_center: BlockPos,
    pub raid_recruits: Vec<i32>,
    pub in_caravan: bool,
    pub caravan_head: Option<i32>,
    /// id -> (inCaravan, hasCaravanTail, head, leashed).
    pub llamas: HashMap<i32, (bool, bool, Option<i32>, bool)>,
    pub invisible: Vec<i32>,
    pub other_look_wanted: HashMap<i32, Vec3>,
    pub close_to_village: bool,
    pub sections_to_village: HashMap<(i32, i32, i32), i32>,
    pub village_pois: Vec<BlockPos>,
    pub level_random: LegacyRandomSource,
    pub golem_wanters: Vec<i32>,
    pub occupied_pois: Vec<BlockPos>,
    pub holding_crossbow: bool,
    pub crossbow_charge_duration: i32,
    pub kinetic_weapon: Option<i32>,
    pub root_vehicle_charge_speed: Option<f32>,
    // ---- brains, fourth lane ----
    pub sleeping: bool,
    pub raid: Option<crate::host::RaidFacts>,
    pub villager_data: crate::host::VillagerDataFacts,
    pub villager_xp: i32,
    pub bells: Vec<BlockPos>,
    pub vehicle: Option<i32>,
    pub pois: Vec<(crate::host::PoiRecordFacts, bool)>,
    pub poi_valid_ranges: HashMap<&'static str, i32>,
    pub wanted_items: Vec<i32>,
    pub secondary_poi_blocks: Vec<BlockPos>,
    pub huntable_hoglins: Vec<i32>,
    pub safe_armor: Vec<i32>,
    pub loved_item_holders: Vec<i32>,
    pub repellents: Vec<BlockPos>,
    pub assign_profession_when_spawned: bool,
    pub standable: Vec<BlockPos>,
    pub heightmap_motion_blocking: HashMap<(i32, i32), i32>,
    pub job_site_professions: HashMap<&'static str, &'static str>,
    pub occupied_beds: Vec<BlockPos>,
    pub other_wants_job_site: Vec<(i32, BlockPos)>,
    pub other_job_sites: HashMap<i32, crate::brain::memory::GlobalPos>,
    pub other_holds: Vec<(i32, &'static str)>,
    pub other_xp: HashMap<i32, i32>,
    pub beds: Vec<BlockPos>,
    pub other_interaction_targets: HashMap<i32, i32>,
    pub other_main_hand: HashMap<i32, ItemSpec>,
    pub offers: Vec<crate::host::MerchantOfferFacts>,
    pub drop_chance: Option<f32>,
    pub excess_food: bool,
    pub others_wanting_food: Vec<i32>,
    pub requested: Vec<&'static str>,
    pub other_requested: HashMap<i32, Vec<&'static str>>,
    pub inventory: Vec<Option<ItemSpec>>,
    pub can_breed: bool,
    pub others_can_breed: Vec<i32>,
    pub offspring: Option<i32>,
    pub heroes: Vec<i32>,
    // ---- sixth lane ----
    /// `Sniffer.transitionTo` writes here, and every call is also logged.
    pub sniffer_state: crate::host::SnifferState,
    /// `Sniffer.isTempted()`.
    pub sniffer_tempted: bool,
    /// `Sniffer.canSniff()`.
    pub sniffer_can_sniff: bool,
    /// `Sniffer.canDig()`.
    pub sniffer_can_dig: bool,
    /// `Sniffer.calculateDigPosition()`.
    pub sniffer_dig_position: Option<BlockPos>,
    /// Effect ids `has_effect` answers true for.
    pub effects: Vec<&'static str>,
    /// `other.yHeadRot` pairs.
    pub other_y_head_rot: HashMap<i32, f32>,
    /// Positions `breeze_within_inner_circle_range` answers true for.
    pub breeze_inner_circle: Vec<Vec3>,
    /// `Breeze.getFiringYPosition()`; `None` falls back to `y()`.
    pub breeze_firing_y: Option<f64>,
    /// `snap_to_surface` answers, keyed by the `from` vector.
    pub snap_surfaces: Vec<(Vec3, BlockPos)>,
    /// Positions `entity_type_is_block_dangerous_at` answers true for.
    pub dangerous_blocks: Vec<BlockPos>,
    /// `Armadillo.getState()`; `switch_to_state` writes here.
    pub armadillo_state: crate::host::ArmadilloState,
    /// `Armadillo.isScared()`.
    pub armadillo_scared: bool,
    /// `Armadillo.shouldSwitchToScaredState()`.
    pub armadillo_should_switch_to_scared: bool,
    /// `Camel.isCamelSitting()`.
    pub camel_sitting: bool,
    /// `Camel.canCamelChangePose()`.
    pub camel_can_change_pose: bool,
    /// `Camel.getPoseTime()`.
    pub camel_pose_time: i64,
    /// `Creaking.canMove()`.
    pub creaking_can_move: bool,
    /// `getVoicePitch()`.
    pub voice_pitch: f32,
    /// `getEffect(e).getAmplifier()` pairs.
    pub effect_amplifiers: HashMap<&'static str, i32>,
    /// Ids `hurt_server` answers true for.
    pub hurt_succeeds: Vec<i32>,
    /// `apply_item_blocking` answers, by target id.
    pub item_blocking: HashMap<i32, f32>,
    /// `Goat.dropHorn()`.
    pub goat_horn_drops: bool,
    /// `getAttributeValue(ATTACK_DAMAGE)`.
    pub attack_damage: f64,
    /// `Frog.getTongueTarget()`; the setter writes here.
    pub frog_tongue_target: Option<i32>,
    /// Ids the static `Frog.canEat` answers true for.
    pub frog_edible: Vec<i32>,
    /// Ids `do_hurt_target` leaves dead.
    pub hurt_kills: Vec<i32>,
    // ---- seventh lane: containers ----
    /// `chunk_block_entity_positions`: the chunk's block-entity positions
    /// in the order `LevelChunk.getBlockEntities().values()` yields them.
    /// A chunk absent from this map is `getChunkNow` returning null.
    pub chunk_block_entities: HashMap<(i32, i32), Vec<BlockPos>>,
    /// `block_entity_kind` and `block_entity_identity`: the registry id and
    /// the object identity of the block entity at each position.
    pub block_entities: HashMap<BlockPos, (&'static str, u64)>,
    /// Positions whose `getBlockEntityContainer` is non-null.
    pub containers: HashMap<BlockPos, Vec<Option<ItemSpec>>>,
    /// Positions `container_is_locked` answers true for.
    pub locked_containers: Vec<BlockPos>,
    /// Positions `chest_is_blocked_at` answers true for.
    pub blocked_chests: Vec<BlockPos>,
    /// `chest_connected_pos` pairs; an absent key is `TYPE == SINGLE`.
    pub chest_pairs: HashMap<BlockPos, BlockPos>,
    /// `collision_shape_bounds`; an absent key is the full unit cube.
    pub collision_bounds: HashMap<BlockPos, Aabb>,
    /// `clip_block_hit` answers, keyed by `(from, to)` rounded through
    /// `to_bits` so a `Vec3` can be a key; an absent pair is a MISS.
    pub clip_hits: Vec<(Vec3, Vec3, BlockPos)>,
    /// `set_guaranteed_drop_main_hand()` call count.
    pub guaranteed_drops: i32,
    /// `has_running_panic_goal()`.
    pub running_panic_goal: bool,
}

impl TestMob {
    /// A zombie-shaped mob (0.6 x 1.95) standing at `(x, y, z)` on a
    /// level, with a seeded `LegacyRandomSource`.
    #[must_use]
    pub fn new(level: GridLevel, x: f64, y: f64, z: f64, seed: i64) -> Self {
        TestMob {
            level,
            id: 1,
            tick_count: 0,
            random: LegacyRandomSource::new(seed),
            x,
            y,
            z,
            xo: x,
            zo: z,
            eye_height: 1.74,
            bb_width: 0.6,
            bb_height: 1.95,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            y_body_rot: 0.0,
            speed: 0.0,
            zza: 0.0,
            xxa: 0.0,
            yya: 0.0,
            jumping: false,
            on_ground: true,
            in_water: false,
            day_time: 0,
            released_pois: Vec::new(),
            panic_damage_kinds: Vec::new(),
            uuids: Vec::new(),
            forgive_dead_players: false,
            can_pick_up_loot: false,
            outside_world_border: Vec::new(),
            last_damage_source: None,
            projectile_weapon_range: None,
            holding_usable_non_melee_weapon: false,
            passengers: Vec::new(),
            same_type_entities: Vec::new(),
            coming_through_door: Vec::new(),
            entity_kinds: Vec::new(),
            entity_type_tags: Vec::new(),
            cube_sizes: Vec::new(),
            targetable: Vec::new(),
            scary: Vec::new(),
            can_stay_rolled_up: false,
            gravity: 0.08,
            jump_boost_power: 0.0,
            jump_strength: 0.42,
            discard_friction: false,
            pose: crate::host::Pose::Standing,
            hoglin_repellents: Vec::new(),
            block_tags: Vec::new(),
            landing_spots: Vec::new(),
            wet_blocks: Vec::new(),
            sturdy_up: Vec::new(),
            solid_face_up: Vec::new(),
            fluid_tags: Vec::new(),
            stay_close_targets: Vec::new(),
            stay_close_travelable: Vec::new(),
            item_entities: Vec::new(),
            offhand: None,
            blocks_attacks: Vec::new(),
            loved_items: Vec::new(),
            angry_others: Vec::new(),
            angry_at: None,
            removed: false,
            temptation_holders: Vec::new(),
            tempt_range: 10.0,
            entity_categories: Vec::new(),
            in_lava: false,
            in_liquid: false,
            is_passenger: false,
            affected_by_fluids: true,
            on_fire: false,
            water_fluid_height: 0.0,
            fluid_jump_threshold: 0.0,
            max_up_step: 0.6,
            max_fall_distance: 3,
            movement_speed: 0.23,
            follow_range: 16.0,
            malus_overrides: HashMap::new(),
            controlling_passenger_is_mob: false,
            has_controlling_passenger: false,
            vehicle_is_boat: false,
            first_passenger_is_mob: false,
            panics: false,
            home_radius: -1,
            home_position: BlockPos { x: 0, y: 0, z: 0 },
            visible: Vec::new(),
            others: Vec::new(),
            target_raw: None,
            aggressive: false,
            hurt: Vec::new(),
            last_hurt_by_mob: None,
            last_hurt_by_mob_timestamp: 0,
            in_love: false,
            age: 0,
            mates: Vec::new(),
            bred_with: Vec::new(),
            tempting: Vec::new(),
            melee_reach: Vec::new(),
            allies: Vec::new(),
            peaceful: false,
            visibility: HashMap::new(),
            calls: Vec::new(),
            delta_movement: Vec3::ZERO,
            was_in_powder_snow: false,
            in_powder_snow: false,
            powder_snow_walkable: false,
            tame: false,
            ordered_to_sit: false,
            owner: None,
            owner_in_other_level: false,
            in_sitting_pose: false,
            leashed: false,
            is_vehicle: false,
            first_passenger: None,
            last_hurt_by: HashMap::new(),
            temper: 0,
            max_temper: 100,
            horse_tamed: false,
            using_item: false,
            main_hand: None,
            snaps: Vec::new(),
            universal_anger: false,
            last_hurt_mob: HashMap::new(),
            last_hurt_by_ts: HashMap::new(),
            wants_to_attack_denied: Vec::new(),
            active_raid: false,
            reputation: HashMap::new(),
            alive: true,
            hurt_marked: false,
            trading_player: None,
            swell_dir: 0,
            head_slot_empty: true,
            x_rot_o: 0.0,
            y_rot_o: 0.0,
            moves: 0,
            ambient_stand_sound: None,
            ambient_stand_interval: 80,
            immobile: false,
            air_supply: 300,
            village_sections: Vec::new(),
            gift_candidates: Vec::new(),
            accepts_gift: Vec::new(),
            horizontal_collision: false,
            mob_griefing: true,
            difficulty: Difficulty::Normal,
            baby: false,
            swinging: false,
            interesting_players: Vec::new(),
            holding_bow: false,
            ticks_using_item: 0,
            controlled_vehicle_mob: None,
            fish: HashMap::new(),
            can_sit_on_shoulder: false,
            shoulder_accepts: false,
            flying_players: Vec::new(),
            wet: Vec::new(),
            snowy: Vec::new(),
            riders: HashMap::new(),
            moved_recently: Vec::new(),
            lying: false,
            raid_over: true,
            raid_active: false,
            raid_center: BlockPos { x: 0, y: 0, z: 0 },
            raid_recruits: Vec::new(),
            in_caravan: false,
            caravan_head: None,
            llamas: HashMap::new(),
            invisible: Vec::new(),
            other_look_wanted: HashMap::new(),
            close_to_village: false,
            sections_to_village: HashMap::new(),
            village_pois: Vec::new(),
            level_random: LegacyRandomSource::new(seed ^ 0x5DEECE66D),
            golem_wanters: Vec::new(),
            occupied_pois: Vec::new(),
            holding_crossbow: false,
            crossbow_charge_duration: 25,
            kinetic_weapon: None,
            root_vehicle_charge_speed: None,
            sleeping: false,
            raid: None,
            villager_data: crate::host::VillagerDataFacts {
                profession: "none",
                level: 1,
            },
            villager_xp: 0,
            bells: Vec::new(),
            vehicle: None,
            pois: Vec::new(),
            poi_valid_ranges: HashMap::new(),
            wanted_items: Vec::new(),
            secondary_poi_blocks: Vec::new(),
            huntable_hoglins: Vec::new(),
            safe_armor: Vec::new(),
            loved_item_holders: Vec::new(),
            repellents: Vec::new(),
            assign_profession_when_spawned: false,
            standable: Vec::new(),
            heightmap_motion_blocking: HashMap::new(),
            job_site_professions: HashMap::new(),
            occupied_beds: Vec::new(),
            other_wants_job_site: Vec::new(),
            other_job_sites: HashMap::new(),
            other_holds: Vec::new(),
            other_xp: HashMap::new(),
            beds: Vec::new(),
            other_interaction_targets: HashMap::new(),
            other_main_hand: HashMap::new(),
            offers: Vec::new(),
            drop_chance: None,
            excess_food: false,
            others_wanting_food: Vec::new(),
            requested: Vec::new(),
            other_requested: HashMap::new(),
            inventory: Vec::new(),
            can_breed: false,
            others_can_breed: Vec::new(),
            offspring: None,
            heroes: Vec::new(),
            sniffer_state: crate::host::SnifferState::Idling,
            sniffer_tempted: false,
            sniffer_can_sniff: false,
            sniffer_can_dig: false,
            sniffer_dig_position: None,
            effects: Vec::new(),
            other_y_head_rot: HashMap::new(),
            breeze_inner_circle: Vec::new(),
            breeze_firing_y: None,
            snap_surfaces: Vec::new(),
            dangerous_blocks: Vec::new(),
            armadillo_state: crate::host::ArmadilloState::Idle,
            armadillo_scared: false,
            armadillo_should_switch_to_scared: false,
            camel_sitting: false,
            camel_can_change_pose: true,
            camel_pose_time: 0,
            creaking_can_move: true,
            voice_pitch: 1.0,
            effect_amplifiers: HashMap::new(),
            hurt_succeeds: Vec::new(),
            item_blocking: HashMap::new(),
            goat_horn_drops: false,
            attack_damage: 0.0,
            frog_tongue_target: None,
            frog_edible: Vec::new(),
            hurt_kills: Vec::new(),
            chunk_block_entities: HashMap::new(),
            block_entities: HashMap::new(),
            containers: HashMap::new(),
            locked_containers: Vec::new(),
            blocked_chests: Vec::new(),
            chest_pairs: HashMap::new(),
            collision_bounds: HashMap::new(),
            clip_hits: Vec::new(),
            guaranteed_drops: 0,
            running_panic_goal: false,
        }
    }

    /// Register a chest at `pos` in its chunk, with the given slots.
    /// The identity token is derived from the position so a second call
    /// at the same position is the *same* block entity; pass an explicit
    /// `identity` through [`TestMob::place_container_with_identity`] to
    /// model a break-and-replace.
    pub fn place_chest(&mut self, pos: BlockPos, slots: Vec<Option<ItemSpec>>) {
        #[allow(clippy::cast_sign_loss)]
        let id = crate::phys::block_pos_as_long(pos.x, pos.y, pos.z) as u64;
        self.place_container_with_identity(pos, "minecraft:chest", id, slots);
    }

    /// The same, with the block-entity kind and identity chosen.
    pub fn place_container_with_identity(
        &mut self,
        pos: BlockPos,
        kind: &'static str,
        identity: u64,
        slots: Vec<Option<ItemSpec>>,
    ) {
        self.block_entities.insert(pos, (kind, identity));
        self.containers.insert(pos, slots);
        let chunk = (pos.x >> 4, pos.z >> 4);
        let entry = self.chunk_block_entities.entry(chunk).or_default();
        if !entry.contains(&pos) {
            entry.push(pos);
        }
    }
}

impl MobHost for TestMob {
    type Level = GridLevel;

    fn level(&self) -> &GridLevel {
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
        self.x
    }
    fn y(&self) -> f64 {
        self.y
    }
    fn z(&self) -> f64 {
        self.z
    }
    fn xo(&self) -> f64 {
        self.xo
    }
    fn zo(&self) -> f64 {
        self.zo
    }
    fn eye_y(&self) -> f64 {
        self.y + self.eye_height
    }
    fn bounding_box(&self) -> Aabb {
        let hw = f64::from(self.bb_width) / 2.0;
        Aabb::new(
            self.x - hw,
            self.y,
            self.z - hw,
            self.x + hw,
            self.y + f64::from(self.bb_height),
            self.z + hw,
        )
    }
    fn bb_width(&self) -> f32 {
        self.bb_width
    }
    fn bb_height(&self) -> f32 {
        self.bb_height
    }
    fn y_rot(&self) -> f32 {
        self.y_rot
    }
    fn set_y_rot(&mut self, v: f32) {
        self.y_rot = v;
    }
    fn x_rot(&self) -> f32 {
        self.x_rot
    }
    fn set_x_rot(&mut self, v: f32) {
        self.x_rot = v;
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
        self.calls.push(format!("setSpeed({v})"));
        self.speed = v;
    }
    fn set_zza(&mut self, v: f32) {
        self.calls.push(format!("setZza({v})"));
        self.zza = v;
    }
    fn set_xxa(&mut self, v: f32) {
        self.calls.push(format!("setXxa({v})"));
        self.xxa = v;
    }
    fn set_yya(&mut self, v: f32) {
        self.yya = v;
    }
    fn set_jumping(&mut self, v: bool) {
        self.calls.push(format!("setJumping({v})"));
        self.jumping = v;
    }
    fn on_ground(&self) -> bool {
        self.on_ground
    }
    fn is_in_water(&self) -> bool {
        self.in_water
    }
    fn is_in_lava(&self) -> bool {
        self.in_lava
    }
    fn is_in_liquid(&self) -> bool {
        self.in_liquid
    }
    fn is_passenger(&self) -> bool {
        self.is_passenger
    }
    fn is_affected_by_fluids(&self) -> bool {
        self.affected_by_fluids
    }
    fn is_on_fire(&self) -> bool {
        self.on_fire
    }
    fn water_fluid_height(&self) -> f64 {
        self.water_fluid_height
    }
    fn fluid_jump_threshold(&self) -> f64 {
        self.fluid_jump_threshold
    }
    fn max_up_step(&self) -> f32 {
        self.max_up_step
    }
    fn max_fall_distance(&self) -> i32 {
        self.max_fall_distance
    }
    fn movement_speed(&self) -> f64 {
        self.movement_speed
    }
    fn follow_range(&self) -> f64 {
        self.follow_range
    }
    fn follow_range_base(&self) -> f64 {
        self.follow_range
    }
    fn pathfinding_malus(&self, t: PathType) -> f32 {
        *self.malus_overrides.get(&t).unwrap_or(&t.malus())
    }
    fn controlling_passenger_is_mob(&self) -> bool {
        self.controlling_passenger_is_mob
    }
    fn has_controlling_passenger(&self) -> bool {
        self.has_controlling_passenger
    }
    fn vehicle_is_boat(&self) -> bool {
        self.vehicle_is_boat
    }
    fn first_passenger_is_mob(&self) -> bool {
        self.first_passenger_is_mob
    }
    fn home_radius(&self) -> i32 {
        self.home_radius
    }
    fn home_position(&self) -> BlockPos {
        self.home_position
    }
    fn last_damage_source_causes_panic(&self) -> bool {
        self.panics
    }
    fn has_line_of_sight_to(&self, entity_id: i32) -> bool {
        self.visible.contains(&entity_id)
    }
    fn entity(&self, id: i32) -> Option<EntityFacts> {
        self.others
            .iter()
            .find(|(_, f)| f.id == id)
            .map(|(_, f)| f.clone())
    }
    fn players(&self) -> Vec<i32> {
        self.others
            .iter()
            .filter(|(c, _)| *c == EntityClass::Player)
            .map(|(_, f)| f.id)
            .collect()
    }
    fn entities_of_class_in(&self, class: EntityClass, aabb: &Aabb) -> Vec<i32> {
        self.others
            .iter()
            .filter(|(c, f)| {
                let class_ok = match class {
                    EntityClass::Living => *c != EntityClass::Item,
                    EntityClass::Mob => *c != EntityClass::Player && *c != EntityClass::Item,
                    other => *c == other,
                };
                class_ok && f.bb.intersects(aabb)
            })
            .map(|(_, f)| f.id)
            .collect()
    }
    fn visibility_percent_of(&self, target_id: i32) -> f64 {
        *self.visibility.get(&target_id).unwrap_or(&1.0)
    }
    fn is_allied_to(&self, id: i32) -> bool {
        self.allies.contains(&id)
    }
    fn difficulty_is_peaceful(&self) -> bool {
        self.peaceful
    }
    fn target_raw(&self) -> Option<i32> {
        self.target_raw
    }
    fn set_target_raw(&mut self, id: Option<i32>) {
        self.calls.push(format!("setTarget({id:?})"));
        self.target_raw = id;
    }
    fn set_aggressive(&mut self, v: bool) {
        self.calls.push(format!("setAggressive({v})"));
        self.aggressive = v;
    }
    fn last_damage_source(&self) -> Option<crate::brain::memory::DamageSourceFacts> {
        self.last_damage_source.clone()
    }
    fn entity_by_uuid(&self, uuid: u128) -> Option<i32> {
        self.uuids.iter().find(|(_, u)| *u == uuid).map(|(i, _)| *i)
    }
    fn forgive_dead_players(&self) -> bool {
        self.forgive_dead_players
    }
    fn world_border_contains(&self, pos: BlockPos) -> bool {
        !self.outside_world_border.contains(&pos)
    }
    fn can_pick_up_loot(&self) -> bool {
        self.can_pick_up_loot
    }
    fn release_poi(&mut self, memory: crate::brain::memory::MemoryModuleType) {
        self.released_pois.push(memory);
    }
    fn damage_type_is(&self, kind: &str, tag: crate::host::DamageTypeTag) -> bool {
        tag == crate::host::DamageTypeTag::PanicCauses && self.panic_damage_kinds.contains(&kind)
    }
    fn day_time(&self) -> i64 {
        self.day_time
    }
    fn tempt_range(&self) -> f64 {
        self.tempt_range
    }
    fn player_holding_temptation(&self, player: i32, _t: crate::host::Temptations) -> bool {
        self.temptation_holders.contains(&player)
    }
    fn has_passenger(&self, id: i32) -> bool {
        self.passengers.contains(&id)
    }
    fn is_holding_usable_non_melee_weapon(&self) -> bool {
        self.holding_usable_non_melee_weapon
    }
    fn usable_projectile_weapon_range(&self) -> Option<i32> {
        self.projectile_weapon_range
    }
    fn entity_type_matches_mine(&self, id: i32) -> bool {
        self.same_type_entities.contains(&id)
    }
    fn entity_category_is(&self, id: i32, category: crate::brain::behaviors::MobCategory) -> bool {
        self.entity_categories.contains(&(id, category))
    }
    fn entity_uuid(&self, id: i32) -> u128 {
        self.uuids
            .iter()
            .find(|(i, _)| *i == id)
            .map_or(0, |(_, u)| *u)
    }
    fn can_target_entity(&self, id: i32) -> bool {
        self.targetable.contains(&id)
    }
    fn is_scared_by(&self, id: i32) -> bool {
        self.scary.contains(&id)
    }
    fn can_stay_rolled_up(&self) -> bool {
        self.can_stay_rolled_up
    }
    fn gravity(&self) -> f64 {
        self.gravity
    }
    fn jump_boost_power(&self) -> f32 {
        self.jump_boost_power
    }
    fn jump_strength(&self) -> f64 {
        self.jump_strength
    }
    fn set_discard_friction(&mut self, v: bool) {
        self.discard_friction = v;
    }
    fn set_pose(&mut self, pose: crate::host::Pose) {
        self.pose = pose;
    }
    fn is_hoglin_repellent_at(&self, pos: rustmc_data::BlockPos) -> bool {
        self.hoglin_repellents.contains(&pos)
    }
    fn block_tag_is(&self, pos: rustmc_data::BlockPos, tag: &str) -> bool {
        self.block_tags.iter().any(|(p, t)| *p == pos && *t == tag)
    }
    fn acceptable_landing_spot(&self, _name: &str, pos: rustmc_data::BlockPos) -> bool {
        self.landing_spots.contains(&pos)
    }
    fn item_entity_stack(&self, id: i32) -> Option<ItemSpec> {
        self.item_entities
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, s)| s.clone())
    }
    fn offhand_item(&self) -> Option<ItemSpec> {
        self.offhand.clone()
    }
    fn item_blocks_attacks(&self, item: &ItemSpec) -> bool {
        self.blocks_attacks.contains(&item.id)
    }
    fn piglin_is_loved_item(&self, item: &ItemSpec) -> bool {
        self.loved_items.contains(&item.id)
    }
    fn piglin_stop_holding_off_hand_item(&mut self, should_barter: bool) {
        self.calls
            .push(format!("stop_holding_off_hand {should_barter}"));
    }
    fn piglin_set_anger_target(&mut self, target: i32) {
        self.calls.push(format!("set_anger_target {target}"));
    }
    fn piglin_dont_kill_any_more_hoglins_for_a_while(&mut self, id: i32) {
        self.calls.push(format!("dont_kill_hoglins {id}"));
    }
    fn piglin_broadcast_anger_target(&mut self, target: i32) {
        self.calls.push(format!("broadcast_anger {target}"));
    }
    fn other_is_angry(&self, id: i32) -> bool {
        self.angry_others.contains(&id)
    }
    fn warden_entity_angry_at(&self) -> Option<i32> {
        self.angry_at
    }
    fn un_ride(&mut self) {
        self.calls.push("un_ride".to_string());
    }
    fn has_pose(&self, pose: crate::host::Pose) -> bool {
        self.pose == pose
    }
    fn warden_increase_anger_at(&mut self, id: i32) {
        self.calls.push(format!("increase_anger {id}"));
    }
    fn warden_increase_anger_at_by(&mut self, id: i32, amount: i32, alert_others: bool) {
        self.calls
            .push(format!("increase_anger {id} {amount} {alert_others}"));
    }
    fn warden_set_disturbance_location(&mut self, pos: rustmc_data::BlockPos) {
        self.calls
            .push(format!("set_disturbance {} {} {}", pos.x, pos.y, pos.z));
    }
    fn warden_set_attack_target(&mut self, id: i32) {
        self.calls.push(format!("set_attack_target {id}"));
    }
    fn is_removed(&self) -> bool {
        self.removed
    }
    fn remove_discarded(&mut self) {
        self.removed = true;
        self.calls.push("remove_discarded".to_string());
    }
    fn warden_sonic_boom_at(&mut self, id: i32) {
        self.calls.push(format!("sonic_boom {id}"));
    }
    fn look_control_set_look_at(&mut self, target: Vec3) {
        self.calls
            .push(format!("look_at {} {} {}", target.x, target.y, target.z));
    }
    fn fluid_is_empty_at(&self, pos: rustmc_data::BlockPos) -> bool {
        !self.wet_blocks.contains(&pos)
    }
    fn is_face_sturdy_up(&self, pos: rustmc_data::BlockPos) -> bool {
        self.sturdy_up.contains(&pos)
    }
    fn collision_face_shape_up_empty(&self, pos: rustmc_data::BlockPos) -> bool {
        !self.solid_face_up.contains(&pos)
    }
    fn fluid_tag_is(&self, pos: rustmc_data::BlockPos, tag: &str) -> bool {
        self.fluid_tags.iter().any(|(p, t)| *p == pos && *t == tag)
    }
    fn lay_spawn_block(&mut self, pos: rustmc_data::BlockPos, block: &'static str) {
        self.calls
            .push(format!("lay_spawn {block} {} {} {}", pos.x, pos.y, pos.z));
    }
    fn stay_close_target(&self, name: &str) -> Option<crate::brain::memory::PositionTracker> {
        self.stay_close_targets
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, t)| t.clone())
    }
    fn stay_close_can_travel_to(&self, name: &str) -> bool {
        self.stay_close_travelable.contains(&name)
    }
    fn add_effect(&mut self, effect: &'static str, duration: i32, amplifier: i32) {
        self.calls
            .push(format!("add_effect {effect} {duration} {amplifier}"));
    }
    fn sniffer_transition_to(&mut self, state: crate::host::SnifferState) {
        self.calls.push(format!("transition_to {state:?}"));
        self.sniffer_state = state;
    }
    fn sniffer_is_tempted(&self) -> bool {
        self.sniffer_tempted
    }
    fn sniffer_can_sniff(&self) -> bool {
        self.sniffer_can_sniff
    }
    fn sniffer_can_dig(&self) -> bool {
        self.sniffer_can_dig
    }
    fn sniffer_calculate_dig_position(&mut self) -> Option<BlockPos> {
        self.sniffer_dig_position
    }
    fn sniffer_on_digging_complete(&mut self, complete: bool) {
        self.calls.push(format!("on_digging_complete {complete}"));
    }
    fn has_effect(&self, effect: &'static str) -> bool {
        self.effects.contains(&effect)
    }
    fn pose(&self) -> crate::host::Pose {
        self.pose
    }
    fn entity_y_head_rot(&self, id: i32) -> f32 {
        self.other_y_head_rot.get(&id).copied().unwrap_or(0.0)
    }
    fn look_at_eyes(&mut self, target: Vec3) {
        self.calls.push(format!(
            "look_at_eyes {} {} {}",
            target.x, target.y, target.z
        ));
    }
    fn level_play_sound(
        &mut self,
        sound: &crate::host::SoundSpec,
        source: &'static str,
        volume: f32,
        pitch: f32,
    ) {
        self.calls.push(format!(
            "level_play_sound {} {source} {volume} {pitch}",
            sound.0
        ));
    }
    fn breeze_within_inner_circle_range(&self, v: Vec3) -> bool {
        self.breeze_inner_circle.contains(&v)
    }
    fn breeze_firing_y_position(&self) -> f64 {
        self.breeze_firing_y.unwrap_or(self.y)
    }
    fn breeze_shoot_wind_charge(
        &mut self,
        dx: f64,
        dy: f64,
        dz: f64,
        velocity: f32,
        inaccuracy: f32,
    ) {
        self.calls.push(format!(
            "shoot_wind_charge {dx} {dy} {dz} {velocity} {inaccuracy}"
        ));
    }
    fn snap_to_surface(&self, from: Vec3) -> Option<BlockPos> {
        self.snap_surfaces
            .iter()
            .find(|(v, _)| *v == from)
            .map(|(_, p)| *p)
    }
    fn entity_type_is_block_dangerous_at(&self, pos: BlockPos) -> bool {
        self.dangerous_blocks.contains(&pos)
    }
    fn armadillo_state(&self) -> crate::host::ArmadilloState {
        self.armadillo_state
    }
    fn armadillo_switch_to_state(&mut self, state: crate::host::ArmadilloState) {
        self.calls
            .push(format!("armadillo_switch_to_state {state:?}"));
        self.armadillo_state = state;
    }
    fn armadillo_is_scared(&self) -> bool {
        self.armadillo_scared
    }
    fn armadillo_should_switch_to_scared_state(&self) -> bool {
        self.armadillo_should_switch_to_scared
    }
    fn armadillo_roll_up(&mut self) {
        self.calls.push("armadillo_roll_up".to_string());
    }
    fn armadillo_roll_out(&mut self) {
        self.calls.push("armadillo_roll_out".to_string());
    }
    fn camel_stand_up_instantly(&mut self) {
        self.calls.push("camel_stand_up_instantly".to_string());
    }
    fn camel_stand_up(&mut self) {
        self.calls.push("camel_stand_up".to_string());
        self.camel_sitting = false;
    }
    fn camel_sit_down(&mut self) {
        self.calls.push("camel_sit_down".to_string());
        self.camel_sitting = true;
    }
    fn camel_is_sitting(&self) -> bool {
        self.camel_sitting
    }
    fn camel_can_change_pose(&self) -> bool {
        self.camel_can_change_pose
    }
    fn camel_pose_time(&self) -> i64 {
        self.camel_pose_time
    }
    fn creaking_can_move(&self) -> bool {
        self.creaking_can_move
    }
    fn voice_pitch(&self) -> f32 {
        self.voice_pitch
    }
    fn effect_amplifier(&self, effect: &'static str) -> Option<i32> {
        self.effect_amplifiers.get(effect).copied()
    }
    fn hurt_server(&mut self, target: i32, kind: &'static str, amount: f32) -> bool {
        self.calls
            .push(format!("hurt_server {target} {kind} {amount}"));
        self.hurt_succeeds.contains(&target)
    }
    fn apply_item_blocking(&mut self, target: i32, kind: &'static str, amount: f32) -> f32 {
        self.calls
            .push(format!("apply_item_blocking {target} {kind} {amount}"));
        self.item_blocking.get(&target).copied().unwrap_or(0.0)
    }
    fn knockback(
        &mut self,
        target: i32,
        strength: f64,
        x: f64,
        z: f64,
        kind: &'static str,
        amount: f32,
    ) {
        self.calls.push(format!(
            "knockback {target} {strength} {x} {z} {kind} {amount}"
        ));
    }
    fn goat_drop_horn(&mut self) -> bool {
        self.calls.push("goat_drop_horn".to_string());
        self.goat_horn_drops
    }
    fn attack_damage(&self) -> f64 {
        self.attack_damage
    }
    fn frog_set_tongue_target(&mut self, id: i32) {
        self.frog_tongue_target = Some(id);
    }
    fn frog_erase_tongue_target(&mut self) {
        self.calls.push("frog_erase_tongue_target".to_string());
        self.frog_tongue_target = None;
    }
    fn frog_tongue_target(&self) -> Option<i32> {
        self.frog_tongue_target
    }
    fn frog_can_eat(&self, id: i32) -> bool {
        self.frog_edible.contains(&id)
    }
    fn set_other_delta_movement(&mut self, id: i32, v: Vec3) {
        self.calls.push(format!(
            "set_other_delta_movement {id} ({} {} {})",
            v.x, v.y, v.z
        ));
    }
    fn remove_entity_killed(&mut self, id: i32) {
        self.calls.push(format!("remove_entity_killed {id}"));
    }

    // ---- seventh lane: containers ----

    fn chunk_block_entity_positions(&self, chunk_x: i32, chunk_z: i32) -> Option<Vec<BlockPos>> {
        self.chunk_block_entities.get(&(chunk_x, chunk_z)).cloned()
    }

    fn block_entity_kind(&self, pos: BlockPos) -> Option<&'static str> {
        self.block_entities.get(&pos).map(|(k, _)| *k)
    }

    fn block_entity_identity(&self, pos: BlockPos) -> Option<u64> {
        self.block_entities.get(&pos).map(|(_, i)| *i)
    }

    fn transport_container_exists(&self, pos: BlockPos) -> bool {
        self.containers.contains_key(&pos)
    }

    fn container_is_locked(&self, pos: BlockPos) -> bool {
        self.locked_containers.contains(&pos)
    }

    fn chest_is_blocked_at(&self, pos: BlockPos) -> bool {
        self.blocked_chests.contains(&pos)
    }

    fn chest_connected_pos(&self, pos: BlockPos) -> Option<BlockPos> {
        self.chest_pairs.get(&pos).copied()
    }

    fn collision_shape_bounds(&self, pos: BlockPos) -> Aabb {
        self.collision_bounds
            .get(&pos)
            .copied()
            .unwrap_or_else(|| Aabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0))
    }

    fn clip_block_hit(&self, from: Vec3, to: Vec3) -> Option<BlockPos> {
        self.clip_hits
            .iter()
            .find(|(f, t, _)| *f == from && *t == to)
            .map(|(_, _, p)| *p)
    }

    fn container_is_empty(&self, pos: BlockPos) -> bool {
        self.containers
            .get(&pos)
            .is_none_or(|v| v.iter().all(|s| s.as_ref().is_none_or(|i| i.count <= 0)))
    }

    fn container_items(&self, pos: BlockPos) -> Vec<Option<ItemSpec>> {
        self.containers.get(&pos).cloned().unwrap_or_default()
    }

    fn container_remove_item(
        &mut self,
        pos: BlockPos,
        slot: usize,
        count: i32,
    ) -> Option<ItemSpec> {
        let slots = self.containers.get_mut(&pos)?;
        let stack = slots.get_mut(slot)?;
        let held = stack.as_mut()?;
        let taken = held.count.min(count);
        let mut out = held.clone();
        out.count = taken;
        held.count -= taken;
        if held.count <= 0 {
            *stack = None;
        }
        self.calls
            .push(format!("removeItem({pos:?}, {slot}, {count})"));
        Some(out)
    }

    fn container_set_item(&mut self, pos: BlockPos, slot: usize, stack: Option<ItemSpec>) {
        self.calls.push(format!(
            "setItem({pos:?}, {slot}, {:?})",
            stack.as_ref().map(|s| (s.id, s.count))
        ));
        if let Some(slots) = self.containers.get_mut(&pos) {
            if slot < slots.len() {
                slots[slot] = stack;
            }
        }
    }

    fn container_set_changed(&mut self, pos: BlockPos) {
        self.calls.push(format!("setChanged({pos:?})"));
    }

    fn main_hand_item(&self) -> Option<ItemSpec> {
        self.main_hand.clone()
    }

    fn set_guaranteed_drop_main_hand(&mut self) {
        self.guaranteed_drops += 1;
        self.calls.push("setGuaranteedDrop(MAINHAND)".to_string());
    }

    fn has_running_panic_goal(&self) -> bool {
        self.running_panic_goal
    }

    fn look_at_entity(&mut self, id: i32, y_max_rot: f32, x_max_rot: f32) {
        self.calls
            .push(format!("look_at_entity {id} {y_max_rot} {x_max_rot}"));
    }
    fn cause_extra_knockback(
        &mut self,
        target: i32,
        strength: f32,
        delta: Vec3,
        kind: &'static str,
        amount: f32,
        flag: bool,
    ) {
        self.calls.push(format!(
            "cause_extra_knockback {target} {strength} ({} {} {}) {kind} {amount} {flag}",
            delta.x, delta.y, delta.z
        ));
    }

    fn cube_mob_size(&self, id: i32) -> Option<i32> {
        self.cube_sizes
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, n)| *n)
    }
    fn entity_type_tag_is(&self, id: i32, tag: &str) -> bool {
        self.entity_type_tags
            .iter()
            .any(|(i, s)| *i == id && *s == tag)
    }
    fn entity_kind_is(&self, id: i32, kind: &str) -> bool {
        self.entity_kinds
            .iter()
            .any(|(i, k)| *i == id && *k == kind)
    }
    fn entity_is_coming_through_door(&self, id: i32, door: BlockPos) -> bool {
        self.coming_through_door.contains(&(id, door))
    }
    fn swing_main_hand(&mut self) {
        self.calls.push("swing".to_string());
    }
    fn do_hurt_target(&mut self, id: i32) -> bool {
        self.calls.push(format!("doHurtTarget({id})"));
        self.hurt.push(id);
        if self.hurt_kills.contains(&id) {
            if let Some((_, f)) = self.others.iter_mut().find(|(_, f)| f.id == id) {
                f.alive = false;
            }
        }
        true
    }
    fn is_within_melee_attack_range(&self, id: i32) -> bool {
        self.melee_reach.contains(&id)
    }
    fn last_hurt_by_mob(&self) -> Option<i32> {
        self.last_hurt_by_mob
    }
    fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.last_hurt_by_mob_timestamp
    }
    fn is_in_love(&self) -> bool {
        self.in_love
    }
    fn age(&self) -> i32 {
        self.age
    }
    fn can_mate_with(&self, id: i32) -> bool {
        self.mates.contains(&id)
    }
    fn spawn_child_from_breeding(&mut self, partner: i32) {
        self.bred_with.push(partner);
    }
    fn holds_tempting_item(&self, id: i32) -> bool {
        self.tempting.contains(&id)
    }
    fn entity_is_exactly(&self, id: i32, class: EntityClass) -> bool {
        self.others.iter().any(|(c, f)| f.id == id && *c == class)
    }
    fn entity_is_assignable_to(&self, id: i32, class: EntityClass) -> bool {
        self.others.iter().any(|(c, f)| {
            f.id == id
                && match class {
                    EntityClass::Living => true,
                    EntityClass::Mob => *c != EntityClass::Player,
                    other => *c == other,
                }
        })
    }
    fn alert_other(&mut self, other: i32, attacker: Option<i32>) {
        self.calls
            .push(format!("alertOther({other}, {attacker:?})"));
        if let Some((_, f)) = self.others.iter_mut().find(|(_, f)| f.id == other) {
            f.target = attacker;
        }
    }

    // ---- second lane ----
    fn delta_movement(&self) -> Vec3 {
        self.delta_movement
    }
    fn set_delta_movement(&mut self, x: f64, y: f64, z: f64) {
        self.calls.push(format!("setDeltaMovement({x}, {y}, {z})"));
        self.delta_movement = Vec3::new(x, y, z);
    }
    fn was_in_powder_snow(&self) -> bool {
        self.was_in_powder_snow
    }
    fn is_in_powder_snow(&self) -> bool {
        self.in_powder_snow
    }
    fn is_powder_snow_walkable(&self) -> bool {
        self.powder_snow_walkable
    }
    fn set_pathfinding_malus(&mut self, t: PathType, f: f32) {
        self.calls.push(format!("setPathfindingMalus({t:?}, {f})"));
        self.malus_overrides.insert(t, f);
    }
    fn may_be_leashed(&self) -> bool {
        self.leashed
    }
    fn is_vehicle(&self) -> bool {
        self.is_vehicle
    }
    fn first_passenger(&self) -> Option<i32> {
        self.first_passenger
    }
    fn eject_passengers(&mut self) {
        self.calls.push("ejectPassengers".to_string());
        self.is_vehicle = false;
        self.first_passenger = None;
    }
    fn broadcast_entity_event(&mut self, id: u8) {
        self.calls.push(format!("broadcastEntityEvent({id})"));
    }
    fn is_tame(&self) -> bool {
        self.tame
    }
    fn is_ordered_to_sit(&self) -> bool {
        self.ordered_to_sit
    }
    fn owner(&self) -> Option<i32> {
        self.owner
    }
    fn owner_in_other_level(&self, _owner: i32) -> bool {
        self.owner_in_other_level
    }
    fn set_in_sitting_pose(&mut self, v: bool) {
        self.calls.push(format!("setInSittingPose({v})"));
        self.in_sitting_pose = v;
    }
    fn entity_last_hurt_by_mob(&self, id: i32) -> Option<i32> {
        self.last_hurt_by.get(&id).copied()
    }
    fn snap_to(&mut self, x: f64, y: f64, z: f64) {
        self.calls.push(format!("snapTo({x}, {y}, {z})"));
        self.snaps.push((x, y, z));
        self.x = x;
        self.y = y;
        self.z = z;
    }
    fn perform_ranged_attack(&mut self, target: i32, power: f32) {
        self.calls
            .push(format!("performRangedAttack({target}, {power})"));
    }
    fn is_using_item(&self) -> bool {
        self.using_item
    }
    fn set_main_hand_item(&mut self, item: Option<&ItemSpec>) {
        self.calls
            .push(format!("setItemSlot(MAINHAND, {:?})", item.map(|i| i.id)));
        self.main_hand = item.cloned();
    }
    fn start_using_main_hand(&mut self) {
        self.calls.push("startUsingItem(MAIN_HAND)".to_string());
        self.using_item = true;
    }
    fn play_sound(&mut self, sound: &SoundSpec, volume: f32, pitch: f32) {
        self.calls
            .push(format!("playSound({}, {volume}, {pitch})", sound.0));
    }
    fn horse_is_tamed(&self) -> bool {
        self.horse_tamed
    }
    fn temper(&self) -> i32 {
        self.temper
    }
    fn max_temper(&self) -> i32 {
        self.max_temper
    }
    fn tame_with_name(&mut self, player: i32) -> bool {
        self.calls.push(format!("tameWithName({player})"));
        self.horse_tamed = true;
        true
    }
    fn modify_temper(&mut self, n: i32) -> i32 {
        self.temper = (self.temper + n).clamp(0, self.max_temper);
        self.calls.push(format!("modifyTemper({n})"));
        self.temper
    }
    fn make_mad(&mut self) {
        self.calls.push("makeMad".to_string());
    }
    fn universal_anger(&self) -> bool {
        self.universal_anger
    }
    fn forget_current_target_and_refresh_universal_anger(&mut self) {
        self.calls
            .push("forgetCurrentTargetAndRefreshUniversalAnger".to_string());
        self.target_raw = None;
    }
    fn other_forget_current_target_and_refresh_universal_anger(&mut self, id: i32) {
        self.calls
            .push(format!("forgetCurrentTargetAndRefreshUniversalAnger({id})"));
    }
    fn entity_last_hurt_mob(&self, id: i32) -> Option<i32> {
        self.last_hurt_mob.get(&id).and_then(|v| v.0)
    }
    fn entity_last_hurt_mob_timestamp(&self, id: i32) -> i32 {
        self.last_hurt_mob.get(&id).map_or(0, |v| v.1)
    }
    fn entity_last_hurt_by_mob_timestamp(&self, id: i32) -> i32 {
        *self.last_hurt_by_ts.get(&id).unwrap_or(&0)
    }
    fn wants_to_attack(&self, target: i32, _owner: i32) -> bool {
        !self.wants_to_attack_denied.contains(&target)
    }
    fn has_active_raid(&self) -> bool {
        self.active_raid
    }
    fn villager_player_reputation(&self, villager: i32, player: i32) -> i32 {
        *self.reputation.get(&(villager, player)).unwrap_or(&0)
    }
    fn is_alive(&self) -> bool {
        self.alive
    }
    fn hurt_marked(&self) -> bool {
        self.hurt_marked
    }
    fn trading_player(&self) -> Option<i32> {
        self.trading_player
    }
    fn set_trading_player(&mut self, id: Option<i32>) {
        self.calls.push(format!("setTradingPlayer({id:?})"));
        self.trading_player = id;
    }
    fn swell_dir(&self) -> i32 {
        self.swell_dir
    }
    fn set_swell_dir(&mut self, v: i32) {
        self.calls.push(format!("setSwellDir({v})"));
        self.swell_dir = v;
    }
    fn head_slot_empty(&self) -> bool {
        self.head_slot_empty
    }
    fn x_rot_o(&self) -> f32 {
        self.x_rot_o
    }
    fn y_rot_o(&self) -> f32 {
        self.y_rot_o
    }
    fn move_by_delta_movement(&mut self) {
        self.moves += 1;
        self.calls.push("move(SELF)".to_string());
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
    fn stand_if_possible(&mut self) {
        self.calls.push("standIfPossible".to_string());
    }
    fn ambient_stand_sound(&self) -> Option<SoundSpec> {
        self.ambient_stand_sound
    }
    fn ambient_stand_interval(&self) -> i32 {
        self.ambient_stand_interval
    }
    fn is_immobile(&self) -> bool {
        self.immobile
    }
    fn air_supply(&self) -> i32 {
        self.air_supply
    }
    fn is_village(&self, pos: BlockPos) -> bool {
        self.village_sections
            .contains(&(pos.x >> 4, pos.y >> 4, pos.z >> 4))
    }
    fn find_section_closest_to_village(
        &self,
        section: (i32, i32, i32),
        radius: i32,
    ) -> (i32, i32, i32) {
        let mut best = section;
        let mut best_d = i64::MAX;
        if self.village_sections.contains(&section) {
            return section;
        }
        for v in &self.village_sections {
            let (dx, dy, dz) = (v.0 - section.0, v.1 - section.1, v.2 - section.2);
            if dx.abs() > radius || dy.abs() > radius || dz.abs() > radius {
                continue;
            }
            let d = i64::from(dx * dx + dy * dy + dz * dz);
            if d < best_d {
                best_d = d;
                best = *v;
            }
        }
        best
    }
    fn offer_flower(&mut self, v: bool) {
        self.calls.push(format!("offerFlower({v})"));
    }
    fn iron_golem_gift_candidates_in(&self, aabb: &Aabb) -> Vec<i32> {
        self.gift_candidates
            .iter()
            .copied()
            .filter(|id| self.entity(*id).is_some_and(|f| f.bb.intersects(aabb)))
            .collect()
    }
    fn accepts_iron_golem_gift_with_empty_antenna(&self, id: i32) -> bool {
        self.accepts_gift.contains(&id)
    }
    fn give_iron_golem_gift(&mut self, id: i32) {
        self.calls.push(format!("giveGift({id})"));
    }
    fn horizontal_collision(&self) -> bool {
        self.horizontal_collision
    }
    fn set_door_open(&mut self, pos: BlockPos, open: bool) {
        self.calls
            .push(format!("setOpen({}, {}, {}, {open})", pos.x, pos.y, pos.z));
        if let Some(f) = self.level.blocks.get_mut(&pos) {
            if let Some(d) = f.door.as_mut() {
                d.open = open;
            }
        }
    }
    fn mob_griefing(&self) -> bool {
        self.mob_griefing
    }
    fn difficulty(&self) -> Difficulty {
        self.difficulty
    }
    fn level_event(&mut self, id: i32, pos: BlockPos, data: i32) {
        self.calls.push(format!(
            "levelEvent({id}, {}, {}, {}, {data})",
            pos.x, pos.y, pos.z
        ));
    }
    fn level_event_block_broken(&mut self, pos: BlockPos) {
        self.calls
            .push(format!("levelEvent(2001, {}, {}, {})", pos.x, pos.y, pos.z));
    }
    fn destroy_block_progress(&mut self, pos: BlockPos, progress: i32) {
        self.calls.push(format!(
            "destroyBlockProgress({}, {}, {}, {progress})",
            pos.x, pos.y, pos.z
        ));
    }
    fn remove_block(&mut self, pos: BlockPos) {
        self.calls
            .push(format!("removeBlock({}, {}, {})", pos.x, pos.y, pos.z));
        self.level.blocks.remove(&pos);
    }
    fn destroy_block(&mut self, pos: BlockPos) {
        self.calls
            .push(format!("destroyBlock({}, {}, {})", pos.x, pos.y, pos.z));
        self.level.blocks.remove(&pos);
    }
    fn turn_grass_to_dirt(&mut self, pos: BlockPos) {
        self.calls
            .push(format!("grassToDirt({}, {}, {})", pos.x, pos.y, pos.z));
        if let Some(f) = self.level.blocks.get_mut(&pos) {
            f.is_grass_block = false;
        }
    }
    fn ate(&mut self) {
        self.calls.push("ate".to_string());
    }
    fn is_baby(&self) -> bool {
        self.baby
    }
    fn swinging(&self) -> bool {
        self.swinging
    }
    fn swing_used_item_hand(&mut self) {
        self.calls.push("swing(usedHand)".to_string());
    }
    fn set_fall_distance(&mut self, d: f64) {
        self.calls.push(format!("fallDistance={d}"));
    }
    fn send_egg_particles(&mut self, _x: f64, _y: f64, _z: f64, _dx: f64, _dy: f64, _dz: f64) {
        self.calls.push("eggParticles".to_string());
    }
    fn send_poof_particle(&mut self, _x: f64, _y: f64, _z: f64, _dx: f64, _dy: f64, _dz: f64) {
        self.calls.push("poof".to_string());
    }
    fn play_destroy_progress_sound(&mut self, _pos: BlockPos) {
        self.calls.push("destroyProgressSound".to_string());
    }
    fn play_break_sound(&mut self, _pos: BlockPos) {
        self.calls.push("breakSound".to_string());
    }
    fn player_holding_interesting(&self, player: i32) -> bool {
        self.interesting_players.contains(&player)
    }
    fn set_is_interested(&mut self, v: bool) {
        self.calls.push(format!("setIsInterested({v})"));
    }
    fn is_holding_bow(&self) -> bool {
        self.holding_bow
    }
    fn stop_using_item(&mut self) {
        self.calls.push("stopUsingItem".to_string());
        self.using_item = false;
        self.ticks_using_item = 0;
    }
    fn ticks_using_item(&self) -> i32 {
        self.ticks_using_item
    }
    fn start_using_bow_hand(&mut self) {
        self.calls.push("startUsingItem(bowHand)".to_string());
        self.using_item = true;
    }
    fn controlled_vehicle_mob(&self) -> Option<i32> {
        self.controlled_vehicle_mob
    }
    fn other_mob_look_at(&mut self, mob: i32, _x: f64, _y: f64, _z: f64, _my: f32, _mp: f32) {
        self.calls.push(format!("vehicleLookAt({mob})"));
    }
    fn fish_school_size(&self, id: i32) -> i32 {
        self.fish.get(&id).map_or(1, |f| f.0)
    }
    fn fish_max_school_size(&self, id: i32) -> i32 {
        self.fish.get(&id).map_or(1, |f| f.1)
    }
    fn fish_leader(&self, id: i32) -> Option<i32> {
        self.fish.get(&id).and_then(|f| f.2)
    }
    fn fish_start_following(&mut self, follower: i32, leader: i32) {
        self.calls
            .push(format!("startFollowing({follower}, {leader})"));
        if let Some(f) = self.fish.get_mut(&follower) {
            f.2 = Some(leader);
        }
        if let Some(l) = self.fish.get_mut(&leader) {
            l.0 += 1;
        }
    }
    fn fish_stop_following(&mut self) {
        self.calls.push("stopFollowing".to_string());
        let me = self.id;
        if let Some(l) = self.fish.get(&me).and_then(|f| f.2) {
            if let Some(lf) = self.fish.get_mut(&l) {
                lf.0 -= 1;
            }
        }
        if let Some(f) = self.fish.get_mut(&me) {
            f.2 = None;
        }
    }
    fn can_sit_on_shoulder(&self) -> bool {
        self.can_sit_on_shoulder
    }
    fn is_in_sitting_pose(&self) -> bool {
        self.in_sitting_pose
    }
    fn is_leashed(&self) -> bool {
        self.leashed
    }
    fn set_entity_on_shoulder(&mut self, player: i32) -> bool {
        self.calls.push(format!("setEntityOnShoulder({player})"));
        self.shoulder_accepts
    }
    fn player_is_flying(&self, id: i32) -> bool {
        self.flying_players.contains(&id)
    }
    fn entity_in_water(&self, id: i32) -> bool {
        self.wet.contains(&id)
    }
    fn entity_in_powder_snow(&self, id: i32) -> bool {
        self.snowy.contains(&id)
    }
    fn controlling_passenger_player(&self, id: i32) -> Option<i32> {
        self.riders.get(&id).copied()
    }
    fn player_has_moved_horizontally_recently(&self, id: i32) -> bool {
        self.moved_recently.contains(&id)
    }
    fn is_lying(&self) -> bool {
        self.lying
    }
    fn set_lying(&mut self, v: bool) {
        self.calls.push(format!("setLying({v})"));
        self.lying = v;
    }
    fn raid_is_over(&self) -> bool {
        self.raid_over
    }
    fn raid_is_active(&self) -> bool {
        self.raid_active
    }
    fn raid_center(&self) -> BlockPos {
        self.raid_center
    }
    fn raid_recruits_in(&self, aabb: &Aabb) -> Vec<i32> {
        self.raid_recruits
            .iter()
            .copied()
            .filter(|id| self.entity(*id).is_some_and(|f| f.bb.intersects(aabb)))
            .collect()
    }
    fn raid_join(&mut self, raider: i32) {
        self.calls.push(format!("joinRaid({raider})"));
    }
    fn in_caravan(&self) -> bool {
        self.in_caravan
    }
    fn caravan_head(&self) -> Option<i32> {
        self.caravan_head
    }
    fn join_caravan(&mut self, head: i32) {
        self.calls.push(format!("joinCaravan({head})"));
        self.in_caravan = true;
        self.caravan_head = Some(head);
    }
    fn leave_caravan(&mut self) {
        self.calls.push("leaveCaravan".to_string());
        self.in_caravan = false;
        self.caravan_head = None;
    }
    fn llama_in_caravan(&self, id: i32) -> bool {
        self.llamas.get(&id).is_some_and(|l| l.0)
    }
    fn llama_has_caravan_tail(&self, id: i32) -> bool {
        self.llamas.get(&id).is_some_and(|l| l.1)
    }
    fn llama_caravan_head(&self, id: i32) -> Option<i32> {
        self.llamas.get(&id).and_then(|l| l.2)
    }
    fn llama_is_leashed(&self, id: i32) -> bool {
        self.llamas.get(&id).is_some_and(|l| l.3)
    }
    fn llamas_in(&self, aabb: &Aabb) -> Vec<i32> {
        self.others
            .iter()
            .filter(|(_, f)| self.llamas.contains_key(&f.id) && f.bb.intersects(aabb))
            .map(|(_, f)| f.id)
            .collect()
    }
    fn entity_is_invisible(&self, id: i32) -> bool {
        self.invisible.contains(&id)
    }
    fn other_look_wanted(&self, id: i32) -> Option<Vec3> {
        self.other_look_wanted.get(&id).copied()
    }
    fn is_close_to_village(&self, _pos: BlockPos, _sections: i32) -> bool {
        self.close_to_village
    }
    fn sections_to_village(&self, section: (i32, i32, i32)) -> i32 {
        *self.sections_to_village.get(&section).unwrap_or(&i32::MAX)
    }
    fn find_village_poi(
        &self,
        pos: BlockPos,
        radius: i32,
        visited: &[BlockPos],
    ) -> Option<BlockPos> {
        self.village_pois.iter().copied().find(|p| {
            !visited.contains(p)
                && (p.x - pos.x).abs() <= radius
                && (p.y - pos.y).abs() <= radius
                && (p.z - pos.z).abs() <= radius
        })
    }
    fn level_random_next_float(&mut self) -> f32 {
        self.level_random.next_f32()
    }
    fn level_random_next_int(&mut self, bound: i32) -> i32 {
        self.level_random.next_i32_bound(bound)
    }
    fn villagers_wanting_golem_in(&self, aabb: &Aabb) -> Vec<i32> {
        self.golem_wanters
            .iter()
            .copied()
            .filter(|id| self.entity(*id).is_some_and(|f| f.bb.intersects(aabb)))
            .collect()
    }
    fn village_sections_in_cube(
        &self,
        section: (i32, i32, i32),
        radius: i32,
    ) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for x in section.0 - radius..=section.0 + radius {
            for y in section.1 - radius..=section.1 + radius {
                for z in section.2 - radius..=section.2 + radius {
                    if self.sections_to_village.get(&(x, y, z)) == Some(&0) {
                        out.push((x, y, z));
                    }
                }
            }
        }
        out
    }
    fn occupied_pois_within(&self, center: BlockPos, radius: i32) -> Vec<BlockPos> {
        self.occupied_pois
            .iter()
            .copied()
            .filter(|p| crate::phys::dist_sqr(*p, center) <= f64::from(radius * radius))
            .collect()
    }
    fn is_holding_crossbow(&self) -> bool {
        self.holding_crossbow
    }
    fn set_charging_crossbow(&mut self, v: bool) {
        self.calls.push(format!("setChargingCrossbow({v})"));
    }
    fn start_using_crossbow_hand(&mut self) {
        self.calls.push("startUsingItem(crossbowHand)".to_string());
        self.using_item = true;
    }
    fn release_using_item(&mut self) {
        self.calls.push("releaseUsingItem".to_string());
        self.using_item = false;
    }
    fn crossbow_charge_duration(&self) -> i32 {
        self.crossbow_charge_duration
    }
    fn clear_charged_projectiles(&mut self) {
        self.calls.push("clearChargedProjectiles".to_string());
    }
    fn main_hand_is_kinetic_weapon(&self) -> bool {
        self.kinetic_weapon.is_some()
    }
    fn kinetic_weapon_use_duration(&self) -> i32 {
        self.kinetic_weapon.unwrap_or(0)
    }
    fn root_vehicle_charge_speed_modifier(&self) -> Option<f32> {
        self.root_vehicle_charge_speed
    }
    fn is_sleeping(&self) -> bool {
        self.sleeping
    }
    fn stop_sleeping(&mut self) {
        self.sleeping = false;
        self.calls.push("stop_sleeping".into());
    }
    fn start_sleeping(&mut self, pos: BlockPos) {
        self.sleeping = true;
        self.calls
            .push(format!("start_sleeping {} {} {}", pos.x, pos.y, pos.z));
    }
    fn raid_at(&self, _pos: BlockPos) -> Option<crate::host::RaidFacts> {
        self.raid
    }
    fn villager_data(&self) -> crate::host::VillagerDataFacts {
        self.villager_data
    }
    fn villager_xp(&self) -> i32 {
        self.villager_xp
    }
    fn set_villager_profession(&mut self, profession: &'static str) {
        self.villager_data.profession = profession;
        self.calls.push(format!("set_profession {profession}"));
    }
    fn refresh_brain(&mut self) {
        self.calls.push("refresh_brain".into());
    }
    fn is_bell_at(&self, pos: BlockPos) -> bool {
        self.bells.contains(&pos)
    }
    fn attempt_to_ring_bell(&mut self, pos: BlockPos) {
        self.calls
            .push(format!("ring_bell {} {} {}", pos.x, pos.y, pos.z));
    }
    fn vehicle(&self) -> Option<i32> {
        self.vehicle
    }
    fn start_riding(&mut self, id: i32) {
        self.vehicle = Some(id);
        self.calls.push(format!("start_riding {id}"));
    }
    fn stop_riding(&mut self) {
        self.vehicle = None;
        self.calls.push("stop_riding".into());
    }
    fn pois_in_range(
        &self,
        center: BlockPos,
        radius: i32,
        occupancy: crate::host::PoiOccupancy,
    ) -> Vec<crate::host::PoiRecordFacts> {
        use crate::host::PoiOccupancy;
        let r2 = f64::from(radius * radius);
        self.pois
            .iter()
            .filter(|(_, occupied)| match occupancy {
                PoiOccupancy::Any => true,
                PoiOccupancy::HasSpace => !*occupied,
                PoiOccupancy::IsOccupied => *occupied,
            })
            .filter(|(p, _)| crate::phys::dist_sqr(p.pos, center) < r2)
            .map(|(p, _)| *p)
            .collect()
    }
    fn poi_type_at(&self, pos: BlockPos) -> Option<&'static str> {
        self.pois
            .iter()
            .find(|(p, _)| p.pos == pos)
            .map(|(p, _)| p.poi_type)
    }
    fn poi_valid_range(&self, poi_type: &str) -> i32 {
        self.poi_valid_ranges.get(poi_type).copied().unwrap_or(1)
    }
    fn take_poi(&mut self, pos: BlockPos) {
        if let Some(e) = self.pois.iter_mut().find(|(p, _)| p.pos == pos) {
            e.1 = true;
        }
        self.calls
            .push(format!("take_poi {} {} {}", pos.x, pos.y, pos.z));
    }
    fn item_entities_in(&self, aabb: &Aabb) -> Vec<i32> {
        self.others
            .iter()
            .filter(|(c, f)| *c == EntityClass::Item && aabb.intersects(&f.bb))
            .map(|(_, f)| f.id)
            .collect()
    }
    fn wants_to_pick_up(&self, item_entity: i32) -> bool {
        self.wanted_items.contains(&item_entity)
    }
    fn is_secondary_poi_block_at(&self, pos: BlockPos) -> bool {
        self.secondary_poi_blocks.contains(&pos)
    }
    fn hoglin_can_be_hunted(&self, id: i32) -> bool {
        self.huntable_hoglins.contains(&id)
    }
    fn is_wearing_safe_armor(&self, id: i32) -> bool {
        self.safe_armor.contains(&id)
    }
    fn is_holding_loved_item(&self, id: i32) -> bool {
        self.loved_item_holders.contains(&id)
    }
    fn is_piglin_repellent_at(&self, pos: BlockPos) -> bool {
        self.repellents.contains(&pos)
    }
    fn spawn_golem_if_needed(&mut self, game_time: i64, required_count: i32) {
        self.calls.push(format!(
            "spawn_golem_if_needed {game_time} {required_count}"
        ));
    }
    fn assign_profession_when_spawned(&self) -> bool {
        self.assign_profession_when_spawned
    }
    fn loaded_and_entity_can_stand_on(&self, pos: BlockPos) -> bool {
        self.standable.contains(&pos)
    }
    fn heightmap_pos_motion_blocking(&self, pos: BlockPos) -> BlockPos {
        BlockPos {
            x: pos.x,
            y: self
                .heightmap_motion_blocking
                .get(&(pos.x, pos.z))
                .copied()
                .unwrap_or(0),
            z: pos.z,
        }
    }
    fn profession_holding_job_site(&self, poi_type: &str) -> Option<&'static str> {
        self.job_site_professions.get(poi_type).copied()
    }
    fn release_poi_at_global(&mut self, gp: crate::brain::memory::GlobalPos) {
        if let Some(e) = self.pois.iter_mut().find(|(p, _)| p.pos == gp.pos) {
            e.1 = false;
        }
        self.calls.push(format!(
            "release_poi {} {} {}",
            gp.pos.x, gp.pos.y, gp.pos.z
        ));
    }
    fn bed_occupied_at(&self, pos: BlockPos) -> bool {
        self.occupied_beds.contains(&pos)
    }
    fn other_villager_wants_job_site(&mut self, id: i32, poi_type: &str, pos: BlockPos) -> bool {
        self.calls.push(format!("wants_job_site {id} {poi_type}"));
        self.other_wants_job_site.contains(&(id, pos))
    }
    fn other_villager_job_site(&self, id: i32) -> Option<crate::brain::memory::GlobalPos> {
        self.other_job_sites.get(&id).copied()
    }
    fn other_villager_profession_holds(&self, id: i32, poi_type: &str) -> bool {
        self.other_holds
            .iter()
            .any(|(i, t)| *i == id && *t == poi_type)
    }
    fn other_villager_xp(&self, id: i32) -> i32 {
        self.other_xp.get(&id).copied().unwrap_or(0)
    }
    fn other_villager_erase_job_site(&mut self, id: i32) {
        self.other_job_sites.remove(&id);
        self.calls.push(format!("other_erase_job_site {id}"));
    }
    fn other_set_walk_and_look_target(
        &mut self,
        id: i32,
        pos: BlockPos,
        speed: f32,
        close_enough: i32,
    ) {
        self.calls.push(format!(
            "other_walk_look {id} {} {} {} {speed} {close_enough}",
            pos.x, pos.y, pos.z
        ));
    }
    fn other_set_potential_job_site(&mut self, id: i32, gp: crate::brain::memory::GlobalPos) {
        self.calls.push(format!(
            "other_potential_job_site {id} {} {} {}",
            gp.pos.x, gp.pos.y, gp.pos.z
        ));
    }
    fn is_bed_at(&self, pos: BlockPos) -> bool {
        self.beds.contains(&pos) || self.occupied_beds.contains(&pos)
    }
    fn other_interaction_target(&self, id: i32) -> Option<i32> {
        self.other_interaction_targets.get(&id).copied()
    }
    fn set_drop_chance_main_hand(&mut self, chance: f32) {
        self.drop_chance = Some(chance);
    }
    fn other_main_hand_item(&self, id: i32) -> Option<ItemSpec> {
        self.other_main_hand.get(&id).cloned()
    }
    fn merchant_offers(&self) -> Vec<crate::host::MerchantOfferFacts> {
        self.offers.clone()
    }
    fn has_excess_food(&self) -> bool {
        self.excess_food
    }
    fn other_villager_wants_more_food(&self, id: i32) -> bool {
        self.others_wanting_food.contains(&id)
    }
    fn gossip_with(&mut self, id: i32, game_time: i64) {
        self.calls.push(format!("gossip {id} {game_time}"));
    }
    fn requested_items(&self) -> Vec<&'static str> {
        self.requested.clone()
    }
    fn other_requested_items(&self, id: i32) -> Vec<&'static str> {
        self.other_requested.get(&id).cloned().unwrap_or_default()
    }
    fn inventory_items(&self) -> Vec<Option<ItemSpec>> {
        self.inventory.clone()
    }
    fn shrink_inventory_slot(&mut self, slot: usize, n: i32) {
        if let Some(Some(stack)) = self.inventory.get_mut(slot) {
            stack.count -= n;
            if stack.count <= 0 {
                self.inventory[slot] = None;
            }
        }
    }
    fn throw_item(&mut self, stack: ItemSpec, target: Vec3) {
        self.calls.push(format!(
            "throw {}x{} at {} {} {}",
            stack.count, stack.id, target.x, target.y, target.z
        ));
    }
    fn other_look_at_entity(&mut self, id: i32, target: i32) {
        self.calls.push(format!("other_look {id} {target}"));
    }
    fn other_set_walk_and_look_target_to_entity(
        &mut self,
        id: i32,
        target: i32,
        speed: f32,
        close_enough: i32,
    ) {
        self.calls.push(format!(
            "other_walk_look_entity {id} {target} {speed} {close_enough}"
        ));
    }
    fn can_breed(&self) -> bool {
        self.can_breed
    }
    fn other_can_breed(&self, id: i32) -> bool {
        self.others_can_breed.contains(&id)
    }
    fn eat_and_digest_food(&mut self) {
        self.calls.push("eat".into());
    }
    fn other_eat_and_digest_food(&mut self, id: i32) {
        self.calls.push(format!("other_eat {id}"));
    }
    fn breed_offspring(&mut self, partner: i32) -> Option<i32> {
        self.calls.push(format!("breed_offspring {partner}"));
        self.offspring
    }
    fn set_age(&mut self, age: i32) {
        self.age = age;
        self.calls.push(format!("set_age {age}"));
    }
    fn other_set_age(&mut self, id: i32, age: i32) {
        self.calls.push(format!("other_set_age {id} {age}"));
    }
    fn other_snap_to(&mut self, id: i32, x: f64, y: f64, z: f64, y_rot: f32, x_rot: f32) {
        self.calls
            .push(format!("other_snap_to {id} {x} {y} {z} {y_rot} {x_rot}"));
    }
    fn add_fresh_entity_with_passengers(&mut self, id: i32) {
        self.calls.push(format!("add_entity {id}"));
    }
    fn other_broadcast_entity_event(&mut self, id: i32, event: u8) {
        self.calls.push(format!("other_event {id} {event}"));
    }
    fn other_set_home(&mut self, id: i32, gp: crate::brain::memory::GlobalPos) {
        self.calls.push(format!(
            "other_set_home {id} {} {} {}",
            gp.pos.x, gp.pos.y, gp.pos.z
        ));
    }
    fn other_set_breed_target(&mut self, id: i32, me: i32) {
        self.calls.push(format!("other_set_breed_target {id} {me}"));
    }
    fn other_erase_breed_target(&mut self, id: i32) {
        self.calls.push(format!("other_erase_breed_target {id}"));
    }
    fn has_hero_of_the_village(&self, id: i32) -> bool {
        self.heroes.contains(&id)
    }
    fn drop_from_gift_loot_table(&mut self, table: &'static str, target: Vec3) {
        self.calls.push(format!(
            "gift {table} at {} {} {}",
            target.x, target.y, target.z
        ));
    }
    fn raid_by_id(&self, id: i32) -> Option<crate::host::RaidFacts> {
        self.raid.filter(|r| r.id == id)
    }
    fn play_celebrate_sound(&mut self) {
        self.calls.push("celebrate_sound".into());
    }
    fn spawn_firework_rocket(
        &mut self,
        color: &'static str,
        flight_duration: i32,
        x: f64,
        y: f64,
        z: f64,
    ) {
        self.calls
            .push(format!("firework {color} {flight_duration} {x} {y} {z}"));
    }
}

impl EntityFacts {
    /// A living, non-player entity standing at `(x, y, z)`, 0.6 x 1.8.
    #[must_use]
    pub fn living(id: i32, x: f64, y: f64, z: f64) -> Self {
        EntityFacts {
            id,
            x,
            y,
            z,
            eye_y: y + 1.62,
            bb: Aabb::new(x - 0.3, y, z - 0.3, x + 0.3, y + 1.8, z + 0.3),
            x_rot: 0.0,
            y_rot: 0.0,
            alive: true,
            is_player: false,
            is_spectator: false,
            is_creative: false,
            is_invulnerable: false,
            is_ghast: false,
            vehicle: None,
            age: 0,
            in_love: false,
            panicking: false,
            target: None,
        }
    }

    /// A survival player at `(x, y, z)`.
    #[must_use]
    pub fn player(id: i32, x: f64, y: f64, z: f64) -> Self {
        let mut f = Self::living(id, x, y, z);
        f.is_player = true;
        f
    }
}
