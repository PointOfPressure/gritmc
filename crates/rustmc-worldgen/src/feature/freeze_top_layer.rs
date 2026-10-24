//! The freeze feature — ice on the water, snow on the ground.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `SnowAndFreezeFeature`, `Biome#shouldFreeze`, `Biome#shouldSnow`.
//!
//! The last decoration step is one feature and it runs over the **whole
//! chunk**, not a placement chain's positions: sixteen by sixteen columns
//! from the origin, each taking its own `MOTION_BLOCKING` height. So its
//! position is only ever a chunk corner, and it draws nothing.
//!
//! # It writes ice one block lower than it looks
//!
//! ```text
//! y      = getHeight(MOTION_BLOCKING, x, z)
//! cursor = (x, y, z)          <- snow goes here
//! below  = cursor.below()     <- ice goes here
//! ```
//!
//! The height is the first *free* block above the surface, so snow lands on
//! it and ice replaces the water underneath. Freezing at `cursor` would put
//! ice in the air above a lake.
//!
//! # The two predicates disagree about which position they read
//!
//! `shouldFreeze` is asked about `below` and `shouldSnow` about `cursor` —
//! and both are asked of the biome at **`cursor`**. One biome lookup, two
//! positions.
//!
//! # And the snowy property is a third write
//!
//! After snow lands, the block under it is re-read and, if it has the
//! `snowy` property, written again with `snowy=true`. That is the grass block
//! turning white under a snow layer, and it is why the surface gate's palette
//! contains `grass_block[snowy=false]` — the *other* value of a property that
//! this feature is the only thing to set.

use super::block_predicate::PredicateWorld;

/// The world this feature reads and writes.
pub trait FreezeWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);

    /// `getHeight(MOTION_BLOCKING, x, z)` — the first free block above the
    /// surface, fluids included.
    fn motion_blocking_height(&mut self, x: i32, z: i32) -> i32;

    /// `Biome#coldEnoughToSnow`, i.e. `!warmEnoughToRain`. Proved at 31,548
    /// lookups by `golden_temperature`.
    fn cold_enough_to_snow(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `Biome#getPrecipitationAt(pos, seaLevel) == SNOW`.
    fn precipitation_is_snow(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `LevelReader#isWaterAt`.
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool;

    /// `BlockState#canSurvive` for snow at this position, or `None` when the
    /// port has no rule. `None` refuses rather than guessing.
    fn snow_can_survive(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;
}

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `SnowAndFreezeFeature#place`. Always returns true — vanilla does.
///
/// # Errors
/// If the world cannot answer whether snow survives somewhere.
pub fn place<W: FreezeWorld + ?Sized>(
    world: &mut W,
    origin: (i32, i32, i32),
) -> Result<bool, NeedsRules> {
    let (ox, _, oz) = origin;
    for dx in 0..16 {
        for dz in 0..16 {
            let x = ox + dx;
            let z = oz + dz;
            let y = world.motion_blocking_height(x, z);
            // Snow at `y`, ice at `y - 1`. See the module docs.
            if should_freeze(world, x, y - 1, z, (x, y, z)) {
                world.set_block(x, y - 1, z, "minecraft:ice");
            }
            if should_snow(world, x, y, z)? {
                world.set_block(x, y, z, "minecraft:snow[layers=1]");
                // The third write: the block below turns snowy.
                let below = world.block_at(x, y - 1, z);
                if let Some(snowy) = with_snowy_true(&below) {
                    world.set_block(x, y - 1, z, &snowy);
                }
            }
        }
    }
    Ok(true)
}

/// `Biome#shouldFreeze(level, pos, mustBeAtEdge = false)`.
///
/// The biome is the one at `biome_pos` — the column's top — while every block
/// question is about `pos`, one lower.
fn should_freeze<W: FreezeWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
    biome_pos: (i32, i32, i32),
) -> bool {
    if !world.cold_enough_to_snow(biome_pos.0, biome_pos.1, biome_pos.2) {
        return false;
    }
    if !inside_build_height(world, y) {
        return false;
    }
    // `getBrightness(LightLayer.BLOCK, pos) >= 10` bails. No light has been
    // computed when features run — `initializeLight` is a later chunk status
    // — so this is 0 on a real server too, and the branch is always taken.
    let block = world.block_at(x, y, z);
    let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
    // `fluidState.is(Fluids.WATER)` and `state.getBlock() == Blocks.WATER`:
    // a *source* block, level 0. Flowing water freezes nowhere.
    if plain != "minecraft:water" || block.contains("level=") && !block.contains("level=0") {
        return false;
    }
    // `mustBeAtEdge` is false here, so the four-neighbour check vanilla does
    // for ice *spreading* is skipped. Left as a note rather than as code
    // nobody calls: it is the difference between this and the tick-time path.
    true
}

/// `LevelHeightAccessor#isInsideBuildHeight`, from the shared
/// [`PredicateWorld`] question rather than a second one of this trait's own —
/// two traits declaring the same method name on the same world is an
/// ambiguity the compiler will not resolve for you.
fn inside_build_height<W: FreezeWorld + ?Sized>(world: &mut W, y: i32) -> bool {
    <W as PredicateWorld>::inside_build_height(world, y)
        .expect("a freezing world must know its build height")
}

/// `Biome#shouldSnow(level, pos)`.
fn should_snow<W: FreezeWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, NeedsRules> {
    if !world.cold_enough_to_snow(x, y, z) {
        return Ok(false);
    }
    if !inside_build_height(world, y) {
        return Ok(false);
    }
    if !world.precipitation_is_snow(x, y, z) {
        return Ok(false);
    }
    let block = world.block_at(x, y, z);
    let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
    if plain != "minecraft:air" && plain != "minecraft:cave_air" {
        return Ok(false);
    }
    world
        .snow_can_survive(x, y, z)
        .ok_or_else(|| NeedsRules("canSurvive(snow)".to_string()))
}

/// `state.hasProperty(SNOWY) ? state.setValue(SNOWY, true) : null`.
///
/// String surgery on the canonical spelling rather than a state registry,
/// which is what the rest of this port speaks. Only blocks that already carry
/// the property are touched — adding it to one that does not would invent a
/// state.
fn with_snowy_true(state: &str) -> Option<String> {
    if state.contains("snowy=true") {
        return None;
    }
    state
        .contains("snowy=false")
        .then(|| state.replace("snowy=false", "snowy=true"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    struct Tundra {
        blocks: BTreeMap<(i32, i32, i32), String>,
        height: i32,
        cold: bool,
        snows: bool,
    }

    impl PredicateWorld for Tundra {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn inside_build_height(&self, y: i32) -> Option<bool> {
            Some((-64..320).contains(&y))
        }
    }

    impl FreezeWorld for Tundra {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn motion_blocking_height(&mut self, _: i32, _: i32) -> i32 {
            self.height
        }
        fn cold_enough_to_snow(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.cold
        }
        fn precipitation_is_snow(&mut self, _: i32, _: i32, _: i32) -> bool {
            self.snows
        }
        fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
            self.block_at(x, y, z).starts_with("minecraft:water")
        }
        fn snow_can_survive(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(!self.block_at(x, y - 1, z).starts_with("minecraft:air"))
        }
    }

    fn lake(cold: bool) -> Tundra {
        let mut blocks = BTreeMap::new();
        for x in 0..16 {
            for z in 0..16 {
                blocks.insert((x, 62, z), "minecraft:water[level=0]".to_string());
            }
        }
        Tundra {
            blocks,
            height: 63,
            cold,
            snows: true,
        }
    }

    /// Ice replaces the water **below** the height, not the air at it.
    #[test]
    fn ice_lands_one_block_below_the_motion_blocking_height() {
        let mut world = lake(true);
        place(&mut world, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.blocks.get(&(3, 62, 3)).map(String::as_str),
            Some("minecraft:ice"),
            "the water at y=62 freezes"
        );
        assert_eq!(
            world.blocks.get(&(3, 63, 3)).map(String::as_str),
            Some("minecraft:snow[layers=1]"),
            "and snow lands on top of it"
        );
    }

    #[test]
    fn a_warm_biome_freezes_nothing() {
        let mut world = lake(false);
        place(&mut world, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.blocks.get(&(3, 62, 3)).map(String::as_str),
            Some("minecraft:water[level=0]")
        );
    }

    /// Flowing water does not freeze — only a source block.
    #[test]
    fn flowing_water_does_not_freeze() {
        let mut world = lake(true);
        world
            .blocks
            .insert((5, 62, 5), "minecraft:water[level=3]".to_string());
        place(&mut world, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.blocks.get(&(5, 62, 5)).map(String::as_str),
            Some("minecraft:water[level=3]")
        );
    }

    /// The third write: the block under fresh snow turns snowy.
    #[test]
    fn a_grass_block_under_snow_turns_snowy() {
        let mut world = Tundra {
            blocks: BTreeMap::new(),
            height: 64,
            cold: true,
            snows: true,
        };
        for x in 0..16 {
            for z in 0..16 {
                world
                    .blocks
                    .insert((x, 63, z), "minecraft:grass_block[snowy=false]".to_string());
            }
        }
        place(&mut world, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.blocks.get(&(2, 63, 2)).map(String::as_str),
            Some("minecraft:grass_block[snowy=true]")
        );
    }

    #[test]
    fn only_a_block_that_has_the_property_gets_it() {
        assert_eq!(
            with_snowy_true("minecraft:grass_block[snowy=false]").as_deref(),
            Some("minecraft:grass_block[snowy=true]")
        );
        assert_eq!(with_snowy_true("minecraft:stone"), None);
        assert_eq!(
            with_snowy_true("minecraft:grass_block[snowy=true]"),
            None,
            "already snowy: vanilla's hasProperty branch writes, but the value \
             is unchanged, so skipping the write changes no block"
        );
    }
}
