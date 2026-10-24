//! `nether_fossil` — a single template piece dropped on the first ledge
//! below a random height.
//!
//! Reference: `NetherFossilStructure` and `NetherFossilPieces`, read with
//! `javap -p -c -constants`.
//!
//! # Why this structure and not a fortress
//!
//! It is the only bearding structure with a start anywhere in the measured
//! nether window, and it is `terrain_adaptation: beard_thin`. The
//! beardifier has been ported and gated since 2026-08-02 and has had nothing
//! to feed it in this dimension, because no nether structure was ported at
//! all. This is what turns it on.
//!
//! # The shape of it
//!
//! ```text
//!   random = WorldgenRandom(LegacyRandomSource(0))
//!   random.setLargeFeatureSeed(seed, chunkX, chunkZ)
//!   x  = chunkMinX + random.nextInt(16)
//!   z  = chunkMinZ + random.nextInt(16)
//!   y  = height.sample(random, ctx)          uniform [32, 125]
//!   column = chunkGenerator.getBaseColumn(x, z)
//!   while y > seaLevel:
//!       above = column[y];  y -= 1;  below = column[y]
//!       if above.isAir() and (below.is(SOUL_SAND) or below.isFaceSturdy(UP)):
//!           break
//!   if y <= seaLevel: no structure
//!   rotation = Rotation.values()[random.nextInt(4)]
//!   template = FOSSILS[random.nextInt(14)]
//! ```
//!
//! **The rotation is drawn before the template.** `NetherFossilPieces.addPieces`
//! calls `Rotation.getRandom(random)` at offset 0 and `Util.getRandom(FOSSILS,
//! random)` at offset 16, and swapping them gives a different fossil in a
//! different rotation at every site — a world that looks entirely plausible
//! and matches nothing.
//!
//! # The `SOUL_SAND` branch is unreachable here, and that is worth knowing
//!
//! `getBaseColumn` runs `iterateNoiseColumn`, which is the **noise stage**:
//! the material rule list over the density function, before any surface rule
//! has run. In the nether that yields exactly three things — the settings'
//! default block (netherrack), its default fluid (lava, and only below sea
//! level) and air. There is no soul sand in it, in any column, ever.
//!
//! So `below.is(SOUL_SAND) || below.isFaceSturdy(..)` reduces to the sturdy
//! test alone, and the sturdy test in turn reduces to "is the default block":
//! lava is not a full block and air is not a block. Both reductions are
//! recorded rather than relied on silently, because they are true of *this
//! dimension's settings* and not of the method.
//!
//! # What has an oracle here
//!
//! All of it, and unusually so. Vanilla saves each start into its own chunk
//! NBT, and `worldgen-dump jigsaw-starts <ref> nether_fossil` reads it back:
//! the chunk, the template name, the rotation, the template position and the
//! bounding box, for every start in a reference. `tests/golden_nether_fossil.rs`
//! checks this module against all thirty in
//! `oracle-work/van-nether-20260722-r7`.

use crate::density::fill::Substance;
use crate::rng::legacy::LegacyRandomSource;
use crate::rng::worldgen_random::WorldgenRandom;
use crate::rng::RandomSource;
use crate::structure::template::Rotation;
use crate::surface::rule::VerticalAnchor;

/// `NetherFossilPieces.FOSSILS`, in declaration order.
///
/// The order is the whole content of this constant: the template is chosen by
/// `random.nextInt(14)` as an index into it, so a sorted or reordered list
/// picks a different fossil at every site while still picking *a* fossil.
/// `fossil_10` follows `fossil_9`, not `fossil_1`.
pub const FOSSILS: [&str; 14] = [
    "nether_fossils/fossil_1",
    "nether_fossils/fossil_2",
    "nether_fossils/fossil_3",
    "nether_fossils/fossil_4",
    "nether_fossils/fossil_5",
    "nether_fossils/fossil_6",
    "nether_fossils/fossil_7",
    "nether_fossils/fossil_8",
    "nether_fossils/fossil_9",
    "nether_fossils/fossil_10",
    "nether_fossils/fossil_11",
    "nether_fossils/fossil_12",
    "nether_fossils/fossil_13",
    "nether_fossils/fossil_14",
];

/// `Rotation.values()`, which is the order `Rotation.getRandom` indexes.
const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::Counterclockwise90,
];

/// Where one fossil goes, and which one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FossilStart {
    /// `TemplateStructurePiece.templatePosition` — vanilla's `TP` in the
    /// saved NBT.
    pub pos: (i32, i32, i32),
    pub rotation: Rotation,
    /// An entry of [`FOSSILS`], without the `minecraft:` namespace.
    pub template: &'static str,
}

/// The two anchors of `nether_fossil`'s `height` provider, straight out of
/// `worldgen/structure/nether_fossil.json`:
///
/// ```json
/// "height": { "type": "minecraft:uniform",
///             "min_inclusive": { "absolute": 32 },
///             "max_inclusive": { "below_top": 2 } }
/// ```
const HEIGHT_MIN: VerticalAnchor = VerticalAnchor::Absolute(32);
const HEIGHT_MAX: VerticalAnchor = VerticalAnchor::BelowTop(2);

/// The vertical range that provider resolves to for a dimension.
///
/// `VerticalAnchor::resolve` is `min_y + height - 1 - offset` for
/// `below_top`, which is vanilla's `getGenDepth() - 1 + getMinGenY() -
/// offset` with the terms in a different order. For the nether — `min_y` 0,
/// `height` 128 — the range is 32..=125.
///
/// Resolved from the dimension rather than written as a literal, because the
/// same structure in a dimension of a different height wants a different
/// number and a constant would be silently wrong there without ever looking
/// wrong.
#[must_use]
pub const fn height_range(min_y: i32, height: i32) -> (i32, i32) {
    (
        HEIGHT_MIN.resolve(min_y, height),
        HEIGHT_MAX.resolve(min_y, height),
    )
}

/// `NetherFossilStructure.findGenerationPoint`, followed by
/// `NetherFossilPieces.addPieces`.
///
/// `column` is `getBaseColumn`'s output indexed from `min_y`, and
/// `sea_level` is the generator's — 32 in the nether.
///
/// Returns `None` where vanilla returns `Optional.empty()`: the downward
/// walk reached sea level without finding a ledge.
///
/// `min_y` is both the column's index origin and the anchor base. Vanilla
/// splits those across `NoiseColumn.minY` and
/// `WorldGenerationContext.getMinGenY()`; in a `NoiseBasedChunkGenerator`
/// they are the same number.
///
/// The biome test is **not** here. Vanilla applies it in
/// `Structure.findValidGenerationPoint`, *after* this has run and against
/// the position this returns — so it cannot be folded in without changing
/// which draws happen. The caller does it.
#[must_use]
pub fn find_generation_point(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
    column: &dyn Fn(i32, i32) -> Vec<Substance>,
    min_y: i32,
    height: i32,
    sea_level: i32,
) -> Option<FossilStart> {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    random.set_large_feature_seed(seed, chunk_x, chunk_z);

    let x = chunk_x * 16 + random.next_i32_bound(16);
    let z = chunk_z * 16 + random.next_i32_bound(16);

    // `UniformHeight.sample` is `Mth.randomBetweenInclusive(random, lo, hi)`,
    // which `RandomSource` already carries with vanilla's wrapping
    // arithmetic.
    let (lo, hi) = height_range(min_y, height);
    let mut y = random.next_i32_between_inclusive(lo, hi);

    let column = column(x, z);
    let at = |y: i32| -> Substance {
        usize::try_from(y - min_y)
            .ok()
            .and_then(|index| column.get(index).copied())
            .unwrap_or(Substance::Air)
    };

    // The ledge walk. Note it reads `above` at the *current* y, decrements,
    // and then reads `below` — so the pair straddles the decrement and the
    // loop can exit with `y` already one lower than the air block it found.
    while y > sea_level {
        let above = at(y);
        y -= 1;
        let below = at(y);
        // `below.is(SOUL_SAND)` is unreachable on a base column — see the
        // module header — so this is the sturdy test alone, and on a nether
        // base column that is exactly "is the default block".
        if above.is_air() && below.blocks_motion() {
            break;
        }
    }
    if y <= sea_level {
        return None;
    }

    // `addPieces`: rotation first, then the template. Two draws, in this
    // order, from the same stream the position came from.
    let rotation = ROTATIONS[usize::try_from(random.next_i32_bound(4)).expect("0..4")];
    let template = FOSSILS[usize::try_from(random.next_i32_bound(14)).expect("0..14")];

    Some(FossilStart {
        pos: (x, y, z),
        rotation,
        template,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_height_range_is_thirty_two_to_one_hundred_and_twenty_five() {
        // The nether: min_y 0, height 128. `below_top 2` is
        // `128 - 1 + 0 - 2`.
        assert_eq!(height_range(0, 128), (32, 125));
        // And the overworld's numbers, to show the formula is the anchor's
        // and not a nether constant: min_y -64, height 384.
        assert_eq!(height_range(-64, 384), (32, 317));
    }

    #[test]
    fn the_fossil_list_is_in_declaration_order_not_sorted() {
        // `fossil_10` at index 9, immediately after `fossil_9`. A list built
        // by sorting the filenames would put it at index 1 and every site in
        // the world would get a different fossil.
        assert_eq!(FOSSILS[8], "nether_fossils/fossil_9");
        assert_eq!(FOSSILS[9], "nether_fossils/fossil_10");
        assert_eq!(FOSSILS.len(), 14);
    }

    #[test]
    fn a_column_of_solid_rock_finds_no_ledge() {
        // Every block solid means `above.isAir()` is never true, so the walk
        // runs to sea level and refuses. This is the `Optional.empty()` arm.
        let solid = |_x: i32, _z: i32| vec![Substance::Solid; 128];
        assert!(find_generation_point(1, 0, 0, &solid, 0, 128, 32).is_none());
    }

    #[test]
    fn a_column_of_air_finds_no_ledge_either() {
        // `above.isAir()` is always true and `below.blocksMotion()` never is,
        // so this also runs out. The two refusals have different causes and
        // both reach the same `y <= seaLevel` test.
        let air = |_x: i32, _z: i32| vec![Substance::Air; 128];
        assert!(find_generation_point(1, 0, 0, &air, 0, 128, 32).is_none());
    }

    #[test]
    fn lava_is_not_a_ledge() {
        // `isFaceSturdy` is false for a fluid, so a lava lake's surface does
        // not carry a fossil. A port that tested "not air" instead of
        // "blocks motion" would place one on every lava surface in the
        // dimension.
        let mut column = vec![Substance::Air; 128];
        for block in column.iter_mut().take(60) {
            *block = Substance::Lava;
        }
        let lava = move |_x: i32, _z: i32| column.clone();
        assert!(find_generation_point(1, 0, 0, &lava, 0, 128, 32).is_none());
    }
}
