//! `SurfaceSystem#frozenOceanExtension` — the icebergs of a frozen ocean.
//!
//! Reference (26.2, behaviour ported — never transcribed, CLAUDE.md rule 4):
//! `net.minecraft.world.level.levelgen.SurfaceSystem#frozenOceanExtension`
//! and its call site in `#buildSurface`. The class is not in `vanilla-src/`,
//! so this was read out of the mapped jar's bytecode.
//!
//! # Why the surface rules could never have done this
//!
//! `buildSurface` applies a rule only where the column holds neither air nor
//! fluid — the walk resets on air and merely records `waterHeight` on fluid.
//! An iceberg is packed ice standing *in the water column*, above the sea
//! floor, so no rule the datapack can express will ever be asked about those
//! positions. Vanilla places them from a second pass that runs after the walk
//! and writes straight into the column.
//!
//! That is why this cost a long investigation. The gate said `packed_ice ->
//! water` and every suspect was a surface-rule input: the biome grid, the
//! preliminary surface, the surface depth, the `packed_ice` noise bands, the
//! `steep` condition — all of them measured exact, because none of them was
//! ever consulted. The tell was in the confusion table the whole time:
//! alongside the ice sat `snow_block -> air`, and no overworld rule places
//! snow blocks over open ocean.
//!
//! # Draw counts are behaviour
//!
//! The loop consumes a `nextDouble` per candidate position, and *which*
//! positions are candidates decides how many. Two traps, both preserved
//! here:
//!
//! - The air branch draws before it knows whether it will place. If the draw
//!   loses (`<= 0.01`), control falls through to the water test rather than
//!   moving to the next block — the draw is spent either way.
//! - The whole method returns before creating its random source when the
//!   column is too thin (`thickness <= 1.8`). A column that generates no
//!   iceberg consumes nothing, so the stream cannot be advanced by "checking
//!   first".

use super::rule::BlockState;
use super::system::BlockColumn;
use crate::rng::RandomSource;

/// The three noise samples and the one climate question the extension needs.
///
/// Taken as data rather than sampled here so this function has no opinion
/// about how a world seeds its noises, and so a test can pin a shape without
/// standing up a `NoiseSeeding`.
#[derive(Clone, Copy, Debug)]
pub struct IcebergInputs {
    /// `icebergSurfaceNoise.getValue(x, 0, z)`.
    pub surface: f64,
    /// `icebergPillarNoise.getValue(x * 1.28, 0, z * 1.28)`.
    pub pillar: f64,
    /// `icebergPillarRoofNoise.getValue(x * 1.17, 0, z * 1.17)`.
    pub pillar_roof: f64,
    /// `Biome#shouldMeltFrozenOceanIcebergSlightly` at `(x, seaLevel, z)`,
    /// which is `getTemperature(pos, seaLevel) > 0.1F`. Warm frozen oceans
    /// get shorter bergs.
    pub melt_slightly: bool,
}

fn plain(state: &str) -> &str {
    // A byte scan, not `split_once('[')`: the `char` pattern goes through
    // `CharSearcher` and UTF-8 decoding, which showed up as 1.5% of the whole
    // pipeline on its own. Block state names are ASCII by construction.
    let cut = state
        .as_bytes()
        .iter()
        .position(|b| *b == b'[')
        .unwrap_or(state.len());
    &state[..cut]
}

fn is_water(state: &str) -> bool {
    // `state.is(Blocks.WATER)` — the block, not the fluid state, so flowing
    // water and lava are both excluded. At this stage the only water on the
    // canvas is what the aquifer placed.
    plain(state) == "minecraft:water"
}

/// Build one column's iceberg, if it has one.
///
/// `start_y` is `getHeight(WORLD_SURFACE_WG, x, z) + 1` — the same value the
/// rule walk starts from. `min_surface_level` is the context's, i.e. the
/// blended preliminary surface, which is where the pass stops descending.
///
/// Returns how many blocks it wrote. A pass that runs and changes nothing is
/// indistinguishable from one that never ran, and this project has been
/// caught by that before.
pub fn frozen_ocean_extension(
    column: &mut dyn BlockColumn,
    random: &mut dyn RandomSource,
    inputs: IcebergInputs,
    min_surface_level: i32,
    start_y: i32,
    sea_level: i32,
) -> usize {
    // How thick the berg is: the surface noise sets a broad envelope and the
    // pillar noise cuts spikes through it, whichever is smaller.
    let thickness = (inputs.surface * 8.25).abs().min(inputs.pillar * 15.0);
    if thickness <= 1.8 {
        return 0;
    }

    let roof = (inputs.pillar_roof * 1.5).abs();
    // Quadratic in thickness, capped by the roof noise. `ceil` before the
    // `+ 14.0`, not after.
    let mut height = (thickness * thickness * 1.2).min((roof * 40.0).ceil() + 14.0);
    if inputs.melt_slightly {
        height -= 2.0;
    }

    // Below this the berg is not worth having; both bounds collapse to zero
    // and the loop below then writes nothing, because `y > 0` fails for the
    // water branch and `y < 0` for the air branch.
    let (top, bottom) = if height > 2.0 {
        (
            height + f64::from(sea_level),
            f64::from(sea_level) - height - 7.0,
        )
    } else {
        (0.0, 0.0)
    };

    #[allow(clippy::cast_possible_truncation)]
    let top_i = top as i32;
    #[allow(clippy::cast_possible_truncation)]
    let bottom_i = bottom as i32;

    // Only now is the stream created — see the module docs.
    let snow_budget = 2 + random.next_i32_bound(4);
    let snow_min_y = sea_level + 18 + random.next_i32_bound(10);
    let mut snow_placed = 0i32;
    let mut written = 0usize;

    let mut y = start_y.max(top_i + 1);
    while y >= min_surface_level {
        let Some(state) = column.get(y) else {
            y -= 1;
            continue;
        };

        // Java's `if / else if`: when the first branch's draw loses, the
        // second condition is still evaluated. It cannot pass — the block is
        // air, so `is_water` is false — but the draw has been spent.
        let place = if column.is_air(state) && y < top_i && random.next_f64() > 0.01 {
            true
        } else {
            is_water(state)
                && y > bottom_i
                && y < sea_level
                && bottom != 0.0
                && random.next_f64() > 0.15
        };

        if place {
            // Snow caps the tallest bergs, and only a few blocks of it.
            if snow_placed <= snow_budget && y > snow_min_y {
                column.set(y, BlockState("minecraft:snow_block".into()));
                snow_placed += 1;
            } else {
                column.set(y, BlockState("minecraft:packed_ice".into()));
            }
            written += 1;
        }
        y -= 1;
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::LegacyRandomSource;

    struct Column {
        min_y: i32,
        blocks: Vec<BlockState>,
    }

    impl Column {
        fn ocean(min_y: i32, height: usize, sea_level: i32) -> Self {
            let blocks = (0..height)
                .map(|i| {
                    let y = min_y + i32::try_from(i).expect("small");
                    if y < 40 {
                        BlockState("minecraft:stone".into())
                    } else if y <= sea_level {
                        BlockState("minecraft:water[level=0]".into())
                    } else {
                        BlockState("minecraft:air".into())
                    }
                })
                .collect();
            Self { min_y, blocks }
        }
        fn count(&self, name: &str) -> usize {
            self.blocks.iter().filter(|b| plain(&b.0) == name).count()
        }
    }

    impl BlockColumn for Column {
        fn get(&self, y: i32) -> Option<&str> {
            usize::try_from(y - self.min_y)
                .ok()
                .and_then(|i| self.blocks.get(i))
                .map(|s| s.0.as_str())
        }
        fn set(&mut self, y: i32, state: BlockState) {
            if let Ok(i) = usize::try_from(y - self.min_y) {
                if let Some(slot) = self.blocks.get_mut(i) {
                    *slot = state;
                }
            }
        }
        fn is_air(&self, state: &str) -> bool {
            state == "minecraft:air" || state == "minecraft:cave_air"
        }
        fn is_fluid(&self, state: &str) -> bool {
            matches!(plain(state), "minecraft:water" | "minecraft:lava")
        }
    }

    fn thin() -> IcebergInputs {
        IcebergInputs {
            surface: 0.05,
            pillar: 0.05,
            pillar_roof: 0.1,
            melt_slightly: false,
        }
    }

    fn thick() -> IcebergInputs {
        IcebergInputs {
            surface: 0.9,
            pillar: 0.9,
            pillar_roof: 0.6,
            melt_slightly: false,
        }
    }

    /// A column too thin for a berg must not touch the random stream at all.
    /// Vanilla returns before creating it, so a port that "checks first"
    /// would desync every later column that shares a factory.
    #[test]
    fn a_thin_column_writes_nothing_and_draws_nothing() {
        let mut column = Column::ocean(-64, 384, 63);
        let mut random = LegacyRandomSource::new(1);
        let before = random.next_i32();
        let mut random = LegacyRandomSource::new(1);
        assert_eq!(
            frozen_ocean_extension(&mut column, &mut random, thin(), 0, 64, 63),
            0
        );
        assert_eq!(random.next_i32(), before, "the stream must be untouched");
    }

    /// And a thick one places ice in the water column — above the sea floor,
    /// where no surface rule is ever consulted.
    #[test]
    fn a_thick_column_places_ice_above_the_floor() {
        let mut column = Column::ocean(-64, 384, 63);
        let mut random = LegacyRandomSource::new(7);
        let written = frozen_ocean_extension(&mut column, &mut random, thick(), 0, 64, 63);
        assert!(written > 0, "a thick column should build something");
        assert!(column.count("minecraft:packed_ice") > 0, "packed ice");
        // The sea floor itself is stone and must survive: the pass only
        // overwrites air and water.
        assert_eq!(column.get(39), Some("minecraft:stone"));
    }

    /// Melting shortens the berg rather than removing it, which is the
    /// difference between a warm frozen ocean and a cold one.
    #[test]
    fn melting_lowers_the_top() {
        let mut cold = Column::ocean(-64, 384, 63);
        let mut r = LegacyRandomSource::new(11);
        let cold_written = frozen_ocean_extension(&mut cold, &mut r, thick(), 0, 64, 63);

        let mut warm = Column::ocean(-64, 384, 63);
        let mut r = LegacyRandomSource::new(11);
        let warm_written = frozen_ocean_extension(
            &mut warm,
            &mut r,
            IcebergInputs {
                melt_slightly: true,
                ..thick()
            },
            0,
            64,
            63,
        );
        assert!(
            warm_written < cold_written,
            "melting should place fewer blocks ({warm_written} vs {cold_written})"
        );
    }
}
