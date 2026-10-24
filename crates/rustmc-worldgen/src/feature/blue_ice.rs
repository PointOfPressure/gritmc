//! Blue ice, which grows out of packed ice at the bottom of frozen oceans.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `BlueIceFeature`.
//!
//! # The survey, and now the body too
//!
//! Like [`super::monster_room`], the feature is a cheap test followed by an
//! expensive body. The test is three conditions, all block reads and **no
//! draws**:
//!
//! ```text
//! y <= seaLevel - 1
//! this block or the one below it is water
//! some neighbour other than DOWN is packed ice
//! ```
//!
//! **Until 2026-08-10 only the survey was ported**, on the evidence that
//! every blue-ice attempt in the decoration oracle failed one of those
//! conditions, so the body could never run. That evidence was true and the
//! conclusion drawn from it was not, for a reason worth keeping: the survey
//! needs *packed ice beside it*, and the r=1 oracle window contains none
//! because it is not a frozen ocean. The condition that made the body
//! unreachable was a property of the window, not of the feature — and the
//! seed-12345 r=16 window, which does contain frozen ocean, is the one that
//! could see it. A refusal that never fires is indistinguishable from a
//! feature that never applies.
//!
//! It is also the second half of a defect whose first half hid it. The
//! packed ice this survey looks for is mostly *iceberg* packed ice, and
//! [`super::iceberg`] was itself missing — so even a fully ported body would
//! have found far less to grow from than vanilla does. The two had to be
//! fixed together to measure either.
//!
//! # The body is two hundred rounds of one block each
//!
//! Not an ellipsoid sweep: each round picks a single offset near the origin
//! and, if it is air, water, packed ice or ice **and** already touches blue
//! ice on any of its six faces, converts it. So the deposit grows only by
//! accretion onto itself, one block per round at most, which is why it comes
//! out as a compact floor rather than a sphere.
//!
//! The vertical offset is `nextInt(5) - nextInt(6)`, biased downward, and it
//! also sets the horizontal spread: `k = 3`, plus `j / 2` when `j < 2`. When
//! that leaves `k < 1` the round is skipped **and draws nothing more** — so
//! the per-round cost is two draws or six, never a constant, and a port that
//! always drew four horizontal offsets would desynchronise on every deep
//! round.
//!
//! # `DOWN` is skipped, and that is the whole shape of the feature
//!
//! Blue ice grows *sideways and upward* from packed ice. A deposit under the
//! origin does not seed one, which is why blue ice appears as a floor rather
//! than as a column.

use super::block_predicate::PredicateWorld;

/// The world this feature reads and writes.
pub trait BlueIceWorld: PredicateWorld {
    /// `WorldGenLevel#getSeaLevel`.
    fn sea_level(&mut self) -> i32;
    /// `WorldGenLevel#setBlock(pos, state, 2)`. Flag 2 rather than the usual
    /// 3, which changes nothing during worldgen — nothing is listening for
    /// the neighbour update either way.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// The number of accretion rounds. Fixed in vanilla, named here because the
/// draw budget is `2 * ROUNDS` at minimum and the gate reads it.
const ROUNDS: i32 = 200;

/// `BlueIceFeature#place`.
///
/// # Errors
/// If the world cannot answer `isAir` for a position the growth reads.
pub fn place<W: BlueIceWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn crate::rng::RandomSource,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let (x, y, z) = origin;
    if y > world.sea_level() - 1 {
        return Ok(false);
    }
    if !is_water(world, x, y, z) && !is_water(world, x, y - 1, z) {
        return Ok(false);
    }

    // `Direction.values()` order with DOWN (index 0) skipped.
    let seeded = (1..6).any(|face| {
        let (nx, ny, nz) = super::multiface_growth::offset_of(origin, face);
        let state = world.block_at(nx, ny, nz);
        plain(&state) == "minecraft:packed_ice"
    });
    if !seeded {
        return Ok(false);
    }

    // The seed block, written before the first draw.
    world.set_block(x, y, z, BLUE_ICE);

    for _ in 0..ROUNDS {
        // Biased downward: the mean of `nextInt(5) - nextInt(6)` is -0.5.
        let dy = random.next_i32_bound(5) - random.next_i32_bound(6);
        let mut reach = 3;
        if dy < 2 {
            // Java integer division truncates toward zero, so a `dy` of -3
            // gives -1 and not -2.
            reach += dy / 2;
        }
        if reach < 1 {
            // No further draws on a skipped round. This is the whole reason
            // the per-round cost is not a constant.
            continue;
        }
        // Left to right: both x draws, then both z draws.
        let ox = random.next_i32_bound(reach) - random.next_i32_bound(reach);
        let oz = random.next_i32_bound(reach) - random.next_i32_bound(reach);
        let pos = (x + ox, y + dy, z + oz);

        let state = world.block_at(pos.0, pos.1, pos.2);
        let name = plain(&state);
        let takeable = world
            .air_at(pos.0, pos.1, pos.2)
            .ok_or_else(|| NeedsRules("blue ice: the world cannot answer `isAir`".to_string()))?
            || name == "minecraft:water"
            || name == "minecraft:packed_ice"
            || name == "minecraft:ice";
        if !takeable {
            continue;
        }

        // All six faces this time, DOWN included — the exclusion is on the
        // *survey*, not on the growth.
        for face in 0..6 {
            let (nx, ny, nz) = super::multiface_growth::offset_of(pos, face);
            let neighbour = world.block_at(nx, ny, nz);
            if plain(&neighbour) == BLUE_ICE {
                world.set_block(pos.0, pos.1, pos.2, BLUE_ICE);
                break;
            }
        }
    }

    Ok(true)
}

const BLUE_ICE: &str = "minecraft:blue_ice";

fn is_water<W: BlueIceWorld + ?Sized>(world: &mut W, x: i32, y: i32, z: i32) -> bool {
    let state = world.block_at(x, y, z);
    plain(&state) == "minecraft:water"
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Sea(BTreeMap<(i32, i32, i32), String>);

    impl PredicateWorld for Sea {
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }

        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl BlueIceWorld for Sea {
        fn sea_level(&mut self) -> i32 {
            63
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.0.insert((x, y, z), state.to_string());
        }
    }

    fn rng(seed: i64) -> crate::rng::WorldgenRandom<crate::rng::XoroshiroRandomSource> {
        crate::rng::WorldgenRandom::new(crate::rng::XoroshiroRandomSource::from_seed(seed))
    }

    fn blue(world: &Sea) -> usize {
        world.0.values().filter(|v| *v == BLUE_ICE).count()
    }

    #[test]
    fn above_the_sea_is_a_refusal() {
        let mut world = Sea(BTreeMap::new());
        assert!(!place(&mut world, &mut rng(1), (0, 63, 0)).expect("ok"));
    }

    #[test]
    fn dry_ground_is_a_refusal() {
        let mut world = Sea(BTreeMap::new());
        assert!(!place(&mut world, &mut rng(1), (0, 40, 0)).expect("ok"));
    }

    /// Packed ice *below* does not seed a deposit — DOWN is skipped.
    #[test]
    fn packed_ice_below_does_not_count() {
        let mut world = Sea(BTreeMap::from([
            ((0, 40, 0), "minecraft:water[level=0]".to_string()),
            ((0, 39, 0), "minecraft:packed_ice".to_string()),
        ]));
        assert!(!place(&mut world, &mut rng(1), (0, 40, 0)).expect("ok"));
    }

    /// A survey that fails must not draw. Everything after this feature in
    /// the chunk depends on it.
    #[test]
    fn a_failed_survey_draws_nothing() {
        let mut world = Sea(BTreeMap::new());
        let mut random = rng(11);
        assert!(!place(&mut world, &mut random, (0, 40, 0)).expect("ok"));
        let mut fresh = rng(11);
        assert_eq!(
            crate::rng::RandomSource::next_i64(&mut random),
            crate::rng::RandomSource::next_i64(&mut fresh)
        );
    }

    #[test]
    fn packed_ice_beside_it_seeds_a_deposit() {
        let mut world = Sea(BTreeMap::from([
            ((0, 40, 0), "minecraft:water[level=0]".to_string()),
            ((1, 40, 0), "minecraft:packed_ice".to_string()),
        ]));
        assert!(place(&mut world, &mut rng(3), (0, 40, 0)).expect("ok"));
        assert!(
            blue(&world) > 1,
            "only the seed block grew: {}",
            blue(&world)
        );
    }

    /// Growth is accretion: a block that touches no blue ice is left alone
    /// however many rounds land on it.
    #[test]
    fn a_detached_block_is_never_converted() {
        let mut world = Sea(BTreeMap::from([
            ((0, 40, 0), "minecraft:water[level=0]".to_string()),
            ((1, 40, 0), "minecraft:packed_ice".to_string()),
        ]));
        place(&mut world, &mut rng(3), (0, 40, 0)).expect("ok");
        for ((bx, by, bz), state) in world.0.clone() {
            if state != BLUE_ICE {
                continue;
            }
            if (bx, by, bz) == (0, 40, 0) {
                continue;
            }
            let touches = (0..6).any(|face| {
                let (nx, ny, nz) = super::super::multiface_growth::offset_of((bx, by, bz), face);
                world.0.get(&(nx, ny, nz)).map(String::as_str) == Some(BLUE_ICE)
            });
            assert!(touches, "orphan blue ice at {bx},{by},{bz}");
        }
    }
}
