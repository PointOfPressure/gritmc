//! Shipwrecks — `ShipwreckStructure` and `ShipwreckPieces`.
//!
//! Reference (26.2 bytecode, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.structure.structures.
//! Shipwreck{Structure,Pieces}`, `TemplateStructurePiece`, `StructureStart`.
//!
//! A shipwreck is the cheapest interesting template structure in the
//! overworld: exactly **one** piece, no jigsaw walk, no children, and the
//! whole of its randomness is three draws — a rotation, a template, and (for
//! the beached variant only) a sink depth.
//!
//! # The two variants, and why only one of them is here
//!
//! `shipwrecks.json` is a two-entry structure set: `shipwreck` (ocean) and
//! `shipwreck_beached`. They differ in three ways, and the third is what
//! keeps the beached one out of this port:
//!
//! * the template array — 20 entries against 11;
//! * the heightmap — `OCEAN_FLOOR_WG` against `WORLD_SURFACE_WG`;
//! * the y. An ocean wreck sits at the **mean** ground height under its
//!   footprint. A beached one sits at `lowest - size.y/2 - nextInt(3)`, and
//!   that `nextInt(3)` comes out of `postProcess`'s random, which is
//!   `applyBiomeDecoration`'s `setFeatureSeed(populationSeed, index, 4)` —
//!   the decoration stream at the `SURFACE_STRUCTURES` structure index.
//!
//! **That index is now known and this paragraph used to say it could not
//! be.** It read "not readable from a registry whose iteration order is
//! unstable (ORACLE fault 9)"; the bytecode says otherwise.
//! `ResourceManagerRegistryLoadTask` sorts by resource identifier before it
//! registers anything, so the order is total and the same on every run — see
//! [`crate::structure::step_index`], which derives it and is corroborated by
//! the independently *measured* mineshaft index. `shipwreck_beached` is
//! **index 18 of the 26 structures at step 4**.
//!
//! So what keeps the beached variant out is now only the port itself — the
//! 11-entry template array, `OCEAN_FLOOR_WG` against `WORLD_SURFACE_WG`, and
//! the sink-depth draw — and no longer an unknown number. An ocean wreck
//! spends **nothing** from that stream, which is why it was portable before
//! the index was.
//!
//! # The branch that is dead with vanilla data
//!
//! `generatePieces` asks `isTooBigToFitInWorldGenRegion()` and, when true,
//! takes the height at start time from `Structure.getMeanFirstOccupiedHeight`
//! or `getLowestY` instead of from `postProcess`. The test is
//! `size.x > 32 || size.y > 32` — note it never reads `z` — and the largest
//! shipwreck template is `with_mast` at 9x21x28. So the branch never runs,
//! and [`too_big`] exists to keep the *shape* of the port honest rather than
//! to be true.

use crate::rng::RandomSource;
use crate::structure::template::Rotation;

/// `ShipwreckPieces.PIVOT` = `new BlockPos(4, 0, 15)`, in x and z.
///
/// Identity under `Rotation::NONE`, so a window whose only wreck is unrotated
/// cannot tell a right pivot from a missing one. It is threaded through
/// anyway.
pub const PIVOT: (i32, i32) = (4, 15);

/// `ShipwreckPieces.STRUCTURE_LOCATION_OCEAN`, in declaration order.
///
/// The order is the whole point: the template is
/// `Util.getRandom(array, random)` = `array[random.nextInt(array.length)]`,
/// so index 8 being `rightsideup_fronthalf` is a load-bearing fact and not a
/// listing convenience.
pub const OCEAN: [&str; 20] = [
    "shipwreck/with_mast",
    "shipwreck/upsidedown_full",
    "shipwreck/upsidedown_fronthalf",
    "shipwreck/upsidedown_backhalf",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/upsidedown_full_degraded",
    "shipwreck/upsidedown_fronthalf_degraded",
    "shipwreck/upsidedown_backhalf_degraded",
    "shipwreck/sideways_full_degraded",
    "shipwreck/sideways_fronthalf_degraded",
    "shipwreck/sideways_backhalf_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// `ShipwreckPieces.STRUCTURE_LOCATION_BEACHED`, in declaration order.
///
/// Eleven entries, and **not** a subsequence of [`OCEAN`] read at the same
/// indices: `sideways_full` is index 1 here and index 4 there. Kept even
/// though nothing places a beached wreck yet, because the two arrays being
/// different lengths is exactly what makes the branch observable in a
/// template name.
pub const BEACHED: [&str; 11] = [
    "shipwreck/with_mast",
    "shipwreck/sideways_full",
    "shipwreck/sideways_fronthalf",
    "shipwreck/sideways_backhalf",
    "shipwreck/rightsideup_full",
    "shipwreck/rightsideup_fronthalf",
    "shipwreck/rightsideup_backhalf",
    "shipwreck/with_mast_degraded",
    "shipwreck/rightsideup_full_degraded",
    "shipwreck/rightsideup_fronthalf_degraded",
    "shipwreck/rightsideup_backhalf_degraded",
];

/// The y the piece is built at before `postProcess` adjusts it.
///
/// `generatePieces` ignores the stub's height entirely and builds at
/// `new BlockPos(chunkPos.getMinBlockX(), 90, chunkPos.getMinBlockZ())` — the
/// chunk's **minimum** corner, not its centre, and a hardcoded 90. The stub's
/// own position is used for the biome gate and nothing else.
pub const START_Y: i32 = 90;

/// One wreck's two draws, in vanilla's order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub rotation: Rotation,
    pub template: String,
    pub beached: bool,
}

/// `ShipwreckStructure.generatePieces` — the rotation, then the template.
///
/// The order matters and is not the order the two are declared in: the
/// rotation is drawn in `generatePieces` before `addRandomPiece` is entered,
/// and the template is drawn inside it. Both come from the structure's own
/// `WorldgenRandom`, seeded by `setLargeFeatureSeed(seed, cx, cz)`.
///
/// `Rotation.getRandom` is `Util.getRandom(values(), random)` and
/// `Rotation.values()` is `NONE, CLOCKWISE_90, CLOCKWISE_180,
/// COUNTERCLOCKWISE_90` — declaration order, which is *not* turn count order.
pub fn plan<R: RandomSource + ?Sized>(random: &mut R, beached: bool) -> Plan {
    const ROTATIONS: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ];
    let rotation = ROTATIONS[usize::try_from(random.next_i32_bound(4)).expect("0..4 is a usize")];
    let names: &[&str] = if beached { &BEACHED } else { &OCEAN };
    let i = usize::try_from(
        random.next_i32_bound(i32::try_from(names.len()).expect("a template count fits an i32")),
    )
    .expect("a bounded draw is non-negative");
    Plan {
        rotation,
        template: names[i].to_string(),
        beached,
    }
}

/// `ShipwreckPiece.isTooBigToFitInWorldGenRegion` — `x > 32 || y > 32`.
///
/// `z` is never tested. That omission is real, and it is dead here: no
/// shipwreck template reaches 32 on any axis.
#[must_use]
pub const fn too_big(size: (i32, i32, i32)) -> bool {
    size.0 > 32 || size.1 > 32
}

/// `ShipwreckPiece.postProcess`'s walk: the mean and the lowest ground height
/// under the piece's footprint.
///
/// Three details, each of which alone moves the whole wreck:
///
/// * the footprint is the **unrotated** `size.x` by `size.z` rectangle from
///   `templatePosition`, because `postProcess` offsets by `size - 1` without
///   passing it through `transform`;
/// * `getHeight` is the raw heightmap value with **no** `-1` — the `-1` in
///   `getFirstOccupiedHeight` belongs to the *stub*, which decides the biome
///   and nothing else;
/// * the mean is an integer division of the sum by `size.x * size.z`, taken
///   after the whole walk, so it truncates once rather than per column.
///
/// The `lowest` half is returned even though only a beached wreck reads it:
/// dropping it would leave a caller free to invent one later.
pub fn ground_height<H: FnMut(i32, i32) -> i32>(
    origin: (i32, i32),
    size: (i32, i32, i32),
    mut height_at: H,
) -> (i32, i32) {
    // `int i = level.getMaxY() + 1` — a starting minimum above every real
    // height, not a sentinel that could survive into the result.
    let mut lowest = i32::MAX;
    let mut sum = 0i32;
    let columns = size.0 * size.2;
    if columns == 0 {
        // `if (i1 == 0) j = level.getHeight(type, tp.x, tp.z)` — a zero-sized
        // template takes the single column under its origin and skips the
        // walk, mean and lowest alike.
        let h = height_at(origin.0, origin.1);
        return (h, h);
    }
    for x in origin.0..(origin.0 + size.0) {
        for z in origin.1..(origin.1 + size.2) {
            let h = height_at(x, z);
            sum += h;
            lowest = lowest.min(h);
        }
    }
    (sum / columns, lowest)
}

/// `ShipwreckPiece.calculateBeachedPosition` — `lowest - size.y/2 -
/// random.nextInt(3)`.
///
/// Present and unused. It is here so that the one thing a beached wreck needs
/// is visibly a *stream index*, not a missing formula: the draw is on the
/// decoration random, which is why nothing calls this.
pub fn beached_position<R: RandomSource + ?Sized>(lowest: i32, size_y: i32, random: &mut R) -> i32 {
    lowest - size_y / 2 - random.next_i32_bound(3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;

    /// The whole cheap gate for the reference wreck, with no world and no
    /// blocks: seed 20260722, placement chunk (57, 65).
    ///
    /// Vanilla's own dump names `shipwreck/rightsideup_fronthalf` at
    /// `rot=NONE` there, so this pins the array order *and* the draw order at
    /// once — a wrong array and a swapped draw order both come out as a
    /// different template name.
    #[test]
    fn the_reference_wreck_draws_rightsideup_fronthalf_unrotated() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
        random.set_large_feature_seed(20_260_722, 57, 65);
        let plan = plan(&mut random, false);
        assert_eq!(plan.rotation, Rotation::None);
        assert_eq!(plan.template, "shipwreck/rightsideup_fronthalf");
    }

    /// The mean is one integer division at the end, not a running average.
    /// Two columns at 54 and one at 55 is 163/3 = 54, and a per-column mean
    /// would say 54 too — three at 54, 55, 56 is where they part.
    #[test]
    fn the_mean_truncates_once() {
        let heights = [54, 55, 56, 54, 55, 56, 54, 55, 56];
        let (mean, lowest) = ground_height((0, 0), (3, 9, 3), |x, z| {
            heights[usize::try_from(x * 3 + z).expect("a small index")]
        });
        assert_eq!(mean, 55);
        assert_eq!(lowest, 54);
    }

    /// No vanilla shipwreck template is too big, on either tested axis.
    #[test]
    fn the_too_big_branch_is_dead_for_every_shipwreck() {
        // The largest template in the extract is `with_mast`, 9x21x28.
        assert!(!too_big((9, 21, 28)));
        // And the axis the test omits would not save it if it were bigger.
        assert!(!too_big((9, 9, 64)));
        assert!(too_big((33, 9, 9)));
    }
}
