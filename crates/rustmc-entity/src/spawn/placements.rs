//! `SpawnPlacements` and `SpawnPlacementTypes` — where a type may stand.

use rustmc_data::BlockPos;

use super::host::{Difficulty, EntitySpawnReason, SpawnLevel, SpawnRandom};
use super::rules::{self, PredicateVerdict};
use super::tables::{SpawnPlacementType, SpawnTables};

/// `SpawnPlacementType.isSpawnPositionOk(level, pos, type)`.
///
/// # Three of the four share a prefix and the fourth does not
///
/// `IN_WATER`, `IN_LAVA` and `ON_GROUND` all open with
/// `type != null && level.getWorldBorder().isWithinBounds(pos)`.
/// **`NO_RESTRICTIONS` does not**: its lambda is `iconst_1; ireturn`, two
/// instructions, unconditionally true. So a type registered
/// `NO_RESTRICTIONS` — and the ten that are include `ghast`,
/// `magma_cube` and `phantom` — is not world-border-checked here at all,
/// and neither is any **unregistered** type, because `getPlacementType`
/// defaults to `NO_RESTRICTIONS`. The border still gates them elsewhere;
/// it does not gate them here, and a port that hoisted the shared prefix
/// out of the three would be adding a check.
#[must_use]
pub fn is_spawn_position_ok<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    pos: BlockPos,
    tables: &SpawnTables,
) -> bool {
    match tables.placement_type(kind) {
        SpawnPlacementType::NoRestrictions => true,
        SpawnPlacementType::InWater => {
            if !level.is_within_world_border(pos) {
                return false;
            }
            let here = level.block_facts(pos);
            let above = level.block_facts(BlockPos {
                y: pos.y + 1,
                ..pos
            });
            here.fluid_is_water && !above.is_redstone_conductor
        }
        SpawnPlacementType::InLava => {
            if !level.is_within_world_border(pos) {
                return false;
            }
            level.block_facts(pos).fluid_is_lava
        }
        SpawnPlacementType::OnGround => {
            if !level.is_within_world_border(pos) {
                return false;
            }
            let below = BlockPos {
                y: pos.y - 1,
                ..pos
            };
            if !level.is_valid_spawn(below, kind) {
                return false;
            }
            // `isValidEmptySpawnBlock` at the position AND at the one
            // above it — a two-block column, evaluated in that order.
            empty_spawn_block(kind, level, pos)
                && empty_spawn_block(
                    kind,
                    level,
                    BlockPos {
                        y: pos.y + 1,
                        ..pos
                    },
                )
        }
    }
}

/// `NaturalSpawner.isValidEmptySpawnBlock(level, pos, state, fluid, type)`.
///
/// Five refusals in this order, and the order is worth keeping because
/// each is cheaper than the next: full collision shape, signal source,
/// non-empty fluid, `PREVENT_MOB_SPAWNING_INSIDE`, then the type's own
/// `immuneTo` tag.
#[must_use]
pub fn empty_spawn_block<L: SpawnLevel + ?Sized>(kind: &str, level: &L, pos: BlockPos) -> bool {
    let f = level.block_facts(pos);
    if f.is_collision_shape_full_block {
        return false;
    }
    if f.is_signal_source {
        return false;
    }
    if !f.fluid_is_empty {
        return false;
    }
    if f.prevents_mob_spawning_inside {
        return false;
    }
    !level.is_block_dangerous(pos, kind)
}

/// `SpawnPlacementTypes.ON_GROUND.adjustSpawnPosition(level, pos)` — the
/// only override; the other three inherit the interface default, which
/// returns `pos` unchanged.
///
/// Not called by `NaturalSpawner`. It is here because the placement table
/// carries it and a reader looking for it should find it named rather than
/// absent.
#[must_use]
pub fn adjust_spawn_position<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    pos: BlockPos,
    tables: &SpawnTables,
) -> BlockPos {
    if tables.placement_type(kind) != SpawnPlacementType::OnGround {
        return pos;
    }
    let below = BlockPos {
        y: pos.y - 1,
        ..pos
    };
    if level.block_facts(below).is_pathfindable_land {
        below
    } else {
        pos
    }
}

/// `SpawnPlacements.checkSpawnRules(type, level, reason, pos, random)`.
///
/// Two gates, in this order:
///
/// 1. `!type.isAllowedInPeaceful() && level.getDifficulty() == PEACEFUL`
///    -> false. **Before** the map lookup, so it applies to every type
///    including unregistered ones;
/// 2. the registered `SpawnPredicate`, or **true** when the type has no
///    row at all — `data == null` takes the `iconst_1` arm at offset 59.
///
/// Returns a [`PredicateVerdict`] rather than a bool so that a predicate
/// this crate has not transcribed is a named refusal at the call site
/// instead of a `false` that reads as "the position was wrong".
pub fn check_spawn_rules<L: SpawnLevel + ?Sized>(
    kind: &str,
    level: &L,
    reason: EntitySpawnReason,
    pos: BlockPos,
    rng: &mut dyn SpawnRandom,
    tables: &SpawnTables,
) -> PredicateVerdict {
    let allowed_in_peaceful = tables.facts(kind).is_none_or(|f| f.allowed_in_peaceful);
    if !allowed_in_peaceful && level.difficulty() == Difficulty::Peaceful {
        return PredicateVerdict::Refused;
    }
    match tables.placement(kind) {
        None => PredicateVerdict::Allowed,
        Some(row) => rules::evaluate(&row.predicate, kind, level, reason, pos, rng),
    }
}
