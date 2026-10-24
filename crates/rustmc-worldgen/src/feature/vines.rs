//! Vines on a cave wall.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `VinesFeature`, `VineBlock#isAcceptableNeighbour`.
//!
//! # The whole feature is free
//!
//! Not one draw, at any point. The position came from the placement chain and
//! everything after it is a block read — which makes this, like `spring`, a
//! useful check on a driver: any draw at all is the driver's bug and not the
//! feature's.
//!
//! # It takes the first face that works and stops
//!
//! `Direction.values()` in order, skipping DOWN — so DOWN, UP, NORTH, SOUTH,
//! WEST, EAST minus the first — and the *first* direction whose neighbour can
//! hold a vine gets one, after which the feature returns. A block that could
//! carry vines on three sides carries one.
//!
//! `VineBlock.isAcceptableNeighbour` is `MultifaceBlock.canAttachTo` and
//! nothing else, so it is borrowed from [`super::multiface_growth`] rather
//! than restated: the block on that side must present a full face towards the
//! vine, on either its support shape or its collision shape.

use super::block_predicate::PredicateWorld;
use super::multiface_growth::{can_attach_to, DIRECTIONS};

/// The world this feature reads and writes.
pub trait VineWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `VinesFeature#place`.
///
/// Takes no `RandomSource`: the feature does not draw. Absent rather than
/// ignored, so a caller cannot pass one and assume it was used.
///
/// `default_state` is `Blocks.VINE.defaultBlockState()` spelled the way the
/// block-props extract spells it — every face off.
///
/// # Errors
/// If the world cannot answer a shape question.
pub fn place<W: VineWorld + ?Sized>(
    world: &mut W,
    default_state: &str,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let (x, y, z) = origin;
    let empty = world
        .air_at(x, y, z)
        .ok_or_else(|| NeedsRules("isEmptyBlock".to_string()))?;
    if !empty {
        return Ok(false);
    }

    // `Direction.values()` order, DOWN skipped. Index 0 is DOWN, so the
    // enumeration starts at 1 and the index is the direction.
    for (face, name) in DIRECTIONS.iter().enumerate().skip(1) {
        let backing = super::multiface_growth::offset_of(origin, face);
        let holds = can_attach_to(world, backing, face).map_err(|e| NeedsRules(e.0))?;
        if holds {
            // `VineBlock.getPropertyForFace(direction)` — the face's own
            // boolean, set true on the default state.
            let state = set_face(default_state, name);
            world.set_block(x, y, z, &state);
            return Ok(true);
        }
    }
    Ok(false)
}

/// `state.setValue(property, true)` on the canonical spelling.
fn set_face(state: &str, face: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some((k, _)) if k == face => format!("{face}=true"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const VINE: &str = "minecraft:vine[east=false,north=false,south=false,up=false,west=false]";

    struct Cave(BTreeMap<(i32, i32, i32), String>);

    impl PredicateWorld for Cave {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, _d: usize) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
        fn face_is_collision_full(&mut self, x: i32, y: i32, z: i32, _d: usize) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:stone")
        }
    }

    impl VineWorld for Cave {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
    }

    /// Stone above and to the north: UP comes first in `Direction.values()`
    /// after DOWN, so the vine hangs from the ceiling.
    #[test]
    fn the_first_face_that_works_wins() {
        let mut world = Cave(BTreeMap::from([
            ((0, 1, 0), "minecraft:stone".to_string()),
            ((0, 0, -1), "minecraft:stone".to_string()),
        ]));
        assert!(place(&mut world, VINE, (0, 0, 0)).expect("ok"));
        let placed = world.0.get(&(0, 0, 0)).expect("placed");
        assert!(placed.contains("up=true"), "{placed}");
        assert!(placed.contains("north=false"), "{placed}");
    }

    /// Stone below only: DOWN is skipped, so nothing grows.
    #[test]
    fn a_floor_is_not_something_to_climb() {
        let mut world = Cave(BTreeMap::from([(
            (0, -1, 0),
            "minecraft:stone".to_string(),
        )]));
        assert!(!place(&mut world, VINE, (0, 0, 0)).expect("ok"));
        assert!(!world.0.contains_key(&(0, 0, 0)));
    }

    #[test]
    fn an_occupied_position_is_refused() {
        let mut world = Cave(BTreeMap::from([
            ((0, 0, 0), "minecraft:stone".to_string()),
            ((0, 1, 0), "minecraft:stone".to_string()),
        ]));
        assert!(!place(&mut world, VINE, (0, 0, 0)).expect("ok"));
    }
}
