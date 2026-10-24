//! `PathfindingContext`, `PathTypeCache` and the static block
//! classification of `WalkNodeEvaluator`, from the 26.2 jar.
//!
//! # `PathTypeCache`
//!
//! A 4096-slot direct-mapped cache on `ServerLevel`, keyed by
//! `BlockPos.asLong` through `HashCommon.mix`, invalidated by
//! `ServerLevel` whenever a block changes. With that invalidation it is
//! transparent — `getOrCompute` answers what `getPathTypeFromState` would —
//! so a level that has none (`cache == null`, the non-server branch of the
//! constructor) behaves the same and a test world need not carry one.
//!
//! # The classification chain
//!
//! `getPathTypeFromState` is a chain of `is(...)`/`instanceof` tests whose
//! **order** is the behaviour: a lily pad is `TRAPDOOR` before it could be
//! anything else, a closed wooden door is `DOOR_WOOD_CLOSED` before the
//! `isPathfindable` test that would call it `BLOCKED`, water is tested
//! last. The tests are answered by [`BlockFacts`]; the order is here.

use std::cell::RefCell;

use rustmc_data::BlockPos;

use super::path_type::PathType;
use crate::host::{BlockFacts, MobHost, PathLevel};
use crate::phys::block_pos_as_long;

/// `it.unimi.dsi.fastutil.HashCommon.mix(long)` (fastutil 8.5.18):
/// `x *= 0x9E3779B97F4A7C15L; x ^= x >>> 32; return x ^ (x >>> 16)`.
#[must_use]
pub fn hash_common_mix(x: i64) -> i64 {
    let mut h = x.wrapping_mul(-7_046_029_254_386_353_131);
    h ^= ((h as u64) >> 32) as i64;
    h ^ (((h as u64) >> 16) as i64)
}

/// `PathTypeCache`.
pub struct PathTypeCache {
    positions: Box<[i64; 4096]>,
    path_types: Box<[Option<PathType>; 4096]>,
}

impl Default for PathTypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl PathTypeCache {
    /// `PathTypeCache()`: `positions = new long[4096]`, `pathTypes = new PathType[4096]`.
    ///
    /// A fresh cache has slot 0 keyed by position 0 with a `null` type, so
    /// `get` on `asLong(0,0,0)` sees a matching key and returns `null`,
    /// which `getOrCompute` then computes — same answer as a miss.
    #[must_use]
    pub fn new() -> Self {
        PathTypeCache {
            positions: Box::new([0; 4096]),
            path_types: Box::new([None; 4096]),
        }
    }

    /// `index(long)`: `(int) HashCommon.mix(l) & 4095`.
    #[must_use]
    pub fn index(packed: i64) -> usize {
        #[allow(clippy::cast_possible_truncation)]
        let i = hash_common_mix(packed) as i32;
        (i & 4095) as usize
    }

    /// `getOrCompute(BlockGetter, BlockPos)`.
    pub fn get_or_compute<L: PathLevel>(&mut self, level: &L, pos: BlockPos) -> PathType {
        let packed = block_pos_as_long(pos.x, pos.y, pos.z);
        let i = Self::index(packed);
        if self.positions[i] == packed {
            if let Some(t) = self.path_types[i] {
                return t;
            }
        }
        let t = get_path_type_from_state(level, pos);
        self.positions[i] = packed;
        self.path_types[i] = Some(t);
        t
    }

    /// `invalidate(BlockPos)`: null the type if the slot holds this position.
    pub fn invalidate(&mut self, pos: BlockPos) {
        let packed = block_pos_as_long(pos.x, pos.y, pos.z);
        let i = Self::index(packed);
        if self.positions[i] == packed {
            self.path_types[i] = None;
        }
    }
}

/// `NodeEvaluator.isBurningBlock(BlockState)`:
/// `is(FIRE) || is(LAVA) || is(MAGMA_BLOCK) || isLitCampfire || is(LAVA_CAULDRON)`.
#[must_use]
pub fn is_burning_block(f: &BlockFacts) -> bool {
    f.is_fire || f.is_lava_block || f.is_magma_block || f.is_lit_campfire || f.is_lava_cauldron
}

/// `WalkNodeEvaluator.getPathTypeFromState(BlockGetter, BlockPos)`, in the
/// jar's test order (offsets 13..335).
#[must_use]
pub fn get_path_type_from_state<L: PathLevel>(level: &L, pos: BlockPos) -> PathType {
    let f = level.block_facts(pos);
    if f.is_air {
        return PathType::Open;
    }
    if f.is_trapdoor || f.is_lily_pad || f.is_big_dripleaf {
        return PathType::Trapdoor;
    }
    if f.is_powder_snow {
        return PathType::PowderSnow;
    }
    if f.is_cactus || f.is_sweet_berry_bush {
        return PathType::Damaging;
    }
    if f.is_honey_block {
        return PathType::StickyHoney;
    }
    if f.is_cocoa {
        return PathType::Cocoa;
    }
    if f.is_wither_rose || f.is_speleothem {
        return PathType::DamageCautious;
    }
    if f.fluid_is_lava {
        return PathType::Lava;
    }
    if is_burning_block(&f) {
        return PathType::Fire;
    }
    if let Some(door) = f.door {
        if door.open {
            return PathType::DoorOpen;
        }
        return if door.can_open_by_hand {
            PathType::DoorWoodClosed
        } else {
            PathType::DoorIronClosed
        };
    }
    if f.is_rail {
        return PathType::Rail;
    }
    if f.is_leaves {
        return PathType::Leaves;
    }
    if f.is_fence || f.is_wall || matches!(f.fence_gate_open, Some(false)) {
        return PathType::Fence;
    }
    if !f.is_pathfindable_land {
        return PathType::Blocked;
    }
    if f.fluid_is_water {
        return PathType::Water;
    }
    PathType::Open
}

/// `PathfindingContext`: the `CollisionGetter` the search reads, the
/// level's `PathTypeCache` if it has one, and the mob's block position at
/// `prepare` time. The mob itself is carried too because every evaluator
/// method in the jar reads `this.mob` beside `this.currentContext`.
pub struct PathfindingContext<'a, M: MobHost, L: PathLevel> {
    pub mob: &'a M,
    pub level: &'a L,
    pub cache: Option<&'a RefCell<PathTypeCache>>,
    /// `mobPosition`: `mob.blockPosition()` at construction.
    pub mob_position: BlockPos,
}

impl<'a, M: MobHost, L: PathLevel> PathfindingContext<'a, M, L> {
    /// `PathfindingContext(CollisionGetter, Mob)`.
    #[must_use]
    pub fn new(mob: &'a M, level: &'a L, cache: Option<&'a RefCell<PathTypeCache>>) -> Self {
        PathfindingContext {
            mob,
            level,
            cache,
            mob_position: mob.block_position(),
        }
    }

    /// `getPathTypeFromState(int, int, int)`: through the cache when the
    /// level has one.
    #[must_use]
    pub fn path_type_from_state(&self, x: i32, y: i32, z: i32) -> PathType {
        let pos = BlockPos { x, y, z };
        match self.cache {
            None => get_path_type_from_state(self.level, pos),
            Some(c) => c.borrow_mut().get_or_compute(self.level, pos),
        }
    }

    /// `getBlockState(BlockPos)`, as facts.
    #[must_use]
    pub fn block_facts(&self, pos: BlockPos) -> BlockFacts {
        self.level.block_facts(pos)
    }
}

/// `WalkNodeEvaluator.checkNeighbourBlocks(ctx, x, y, z, fallback)`: the
/// 3x3x3 shell minus the vertical column, `dx` outer, `dy` middle, `dz`
/// inner, first hit wins.
#[must_use]
pub fn check_neighbour_blocks<M: MobHost, L: PathLevel>(
    ctx: &PathfindingContext<'_, M, L>,
    x: i32,
    y: i32,
    z: i32,
    fallback: PathType,
) -> PathType {
    for dx in -1..=1 {
        for dy in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }
                let t = ctx.path_type_from_state(x + dx, y + dy, z + dz);
                match t {
                    PathType::Damaging => return PathType::DamagingInNeighbor,
                    PathType::Fire | PathType::Lava => return PathType::FireInNeighbor,
                    PathType::Water => return PathType::WaterBorder,
                    PathType::DamageCautious => return PathType::DamageCautious,
                    _ => {}
                }
            }
        }
    }
    fallback
}

/// `WalkNodeEvaluator.getPathTypeStatic(ctx, pos)`: the block's own type,
/// and when that is `OPEN` above the world floor, what the block below
/// makes of it (the `tableswitch` at offset 69 through `WalkNodeEvaluator$1`).
#[must_use]
pub fn get_path_type_static<M: MobHost, L: PathLevel>(
    ctx: &PathfindingContext<'_, M, L>,
    x: i32,
    y: i32,
    z: i32,
) -> PathType {
    let t = ctx.path_type_from_state(x, y, z);
    if t != PathType::Open || y < ctx.level.min_y() + 1 {
        return t;
    }
    match ctx.path_type_from_state(x, y - 1, z) {
        PathType::Open | PathType::Water | PathType::Lava | PathType::Walkable => PathType::Open,
        PathType::Fire => PathType::Fire,
        PathType::Damaging => PathType::Damaging,
        PathType::StickyHoney => PathType::StickyHoney,
        PathType::PowderSnow => PathType::OnTopOfPowderSnow,
        PathType::DamageCautious => PathType::DamageCautious,
        PathType::Trapdoor => PathType::OnTopOfTrapdoor,
        _ => check_neighbour_blocks(ctx, x, y, z, PathType::Walkable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::DoorFacts;

    #[test]
    fn mix_matches_fastutil_on_known_values() {
        // mix(0) = 0; mix(1) = the multiplier folded twice.
        assert_eq!(hash_common_mix(0), 0);
        let m: i64 = -7_046_029_254_386_353_131;
        let mut h = m;
        h ^= ((h as u64) >> 32) as i64;
        h ^= ((h as u64) >> 16) as i64;
        assert_eq!(hash_common_mix(1), h);
        assert!(PathTypeCache::index(hash_common_mix(12345)) < 4096);
    }

    #[test]
    fn classification_order() {
        let f = BlockFacts {
            is_lily_pad: true,
            fluid_is_water: true,
            is_pathfindable_land: true,
            is_pathfindable_water: false,
            block_id: None,
            is_bed: false,
            bed_part_head: false,
            chest_open_count: None,
            furnace_lit: None,
            is_edible_for_sheep: false,
            is_grass_block: false,
            blocks_motion: false,
            ..BlockFacts::NONE
        };
        // A lily pad over water is TRAPDOOR, tested before water.
        struct One(BlockFacts);
        impl PathLevel for One {
            fn block_facts(&self, _: BlockPos) -> BlockFacts {
                self.0
            }
            fn min_y(&self) -> i32 {
                -64
            }
            fn max_y(&self) -> i32 {
                319
            }
            fn collision_shape_max_y(&self, _: BlockPos) -> Option<f64> {
                None
            }
            fn game_time(&self) -> i64 {
                0
            }
            fn can_see_sky(&self, _: BlockPos) -> bool {
                true
            }
            fn is_chunk_loaded(&self, _: i32, _: i32) -> bool {
                true
            }
            fn no_collision(&self, _: i32, _: &crate::phys::Aabb) -> bool {
                true
            }
            fn clip_misses(
                &self,
                _: i32,
                _: crate::phys::Vec3,
                _: crate::phys::Vec3,
                _: bool,
            ) -> bool {
                true
            }
        }
        let p = BlockPos { x: 0, y: 0, z: 0 };
        assert_eq!(get_path_type_from_state(&One(f), p), PathType::Trapdoor);
        assert_eq!(
            get_path_type_from_state(&One(BlockFacts::AIR), p),
            PathType::Open
        );
        assert_eq!(
            get_path_type_from_state(&One(BlockFacts::STONE), p),
            PathType::Blocked
        );
        assert_eq!(
            get_path_type_from_state(&One(BlockFacts::WATER), p),
            PathType::Water
        );
        let closed_wood = BlockFacts {
            door: Some(DoorFacts {
                open: false,
                can_open_by_hand: true,
            }),
            ..BlockFacts::NONE
        };
        assert_eq!(
            get_path_type_from_state(&One(closed_wood), p),
            PathType::DoorWoodClosed
        );
        let open_iron = BlockFacts {
            door: Some(DoorFacts {
                open: true,
                can_open_by_hand: false,
            }),
            ..BlockFacts::NONE
        };
        assert_eq!(
            get_path_type_from_state(&One(open_iron), p),
            PathType::DoorOpen
        );
        let open_gate = BlockFacts {
            fence_gate_open: Some(true),
            is_pathfindable_land: true,
            is_pathfindable_water: false,
            block_id: None,
            is_bed: false,
            bed_part_head: false,
            chest_open_count: None,
            furnace_lit: None,
            is_edible_for_sheep: false,
            is_grass_block: false,
            blocks_motion: false,
            ..BlockFacts::NONE
        };
        assert_eq!(get_path_type_from_state(&One(open_gate), p), PathType::Open);
        let closed_gate = BlockFacts {
            fence_gate_open: Some(false),
            ..BlockFacts::NONE
        };
        assert_eq!(
            get_path_type_from_state(&One(closed_gate), p),
            PathType::Fence
        );
        // Lava fluid beats the burning test; magma is FIRE.
        let magma = BlockFacts {
            is_magma_block: true,
            ..BlockFacts::NONE
        };
        assert_eq!(get_path_type_from_state(&One(magma), p), PathType::Fire);
        let lava = BlockFacts {
            fluid_is_lava: true,
            is_lava_block: true,
            ..BlockFacts::NONE
        };
        assert_eq!(get_path_type_from_state(&One(lava), p), PathType::Lava);
    }

    #[test]
    fn cache_is_transparent_and_invalidates() {
        struct Flip(std::cell::Cell<bool>);
        impl PathLevel for Flip {
            fn block_facts(&self, _: BlockPos) -> BlockFacts {
                if self.0.get() {
                    BlockFacts::STONE
                } else {
                    BlockFacts::AIR
                }
            }
            fn min_y(&self) -> i32 {
                0
            }
            fn max_y(&self) -> i32 {
                255
            }
            fn collision_shape_max_y(&self, _: BlockPos) -> Option<f64> {
                None
            }
            fn game_time(&self) -> i64 {
                0
            }
            fn can_see_sky(&self, _: BlockPos) -> bool {
                true
            }
            fn is_chunk_loaded(&self, _: i32, _: i32) -> bool {
                true
            }
            fn no_collision(&self, _: i32, _: &crate::phys::Aabb) -> bool {
                true
            }
            fn clip_misses(
                &self,
                _: i32,
                _: crate::phys::Vec3,
                _: crate::phys::Vec3,
                _: bool,
            ) -> bool {
                true
            }
        }
        let level = Flip(std::cell::Cell::new(false));
        let mut cache = PathTypeCache::new();
        let p = BlockPos { x: 3, y: 4, z: 5 };
        assert_eq!(cache.get_or_compute(&level, p), PathType::Open);
        level.0.set(true);
        // Stale until invalidated, as the jar: the server invalidates on
        // every block change.
        assert_eq!(cache.get_or_compute(&level, p), PathType::Open);
        cache.invalidate(p);
        assert_eq!(cache.get_or_compute(&level, p), PathType::Blocked);
        // Slot 0 of a fresh cache is keyed by position 0 with a null type:
        // a miss, not a stale OPEN.
        let mut fresh = PathTypeCache::new();
        let origin = BlockPos { x: 0, y: 0, z: 0 };
        assert_eq!(fresh.get_or_compute(&level, origin), PathType::Blocked);
    }
}
