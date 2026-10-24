//! The two huge mushrooms — a stem, a cap, and two draws spent before either.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `AbstractHugeMushroomFeature`, `HugeBrownMushroomFeature`,
//! `HugeRedMushroomFeature`, `configurations.HugeMushroomFeatureConfiguration`,
//! `HugeMushroomBlock`.
//!
//! # The draws are spent before the position is judged
//!
//! ```text
//! place():
//!     height = getTreeHeight(random)      nextInt(3) + 4, then nextInt(12)
//!     if (!isValidPosition(...)) return false          <- no further draws
//!     makeCap(...); placeTrunk(...)
//! ```
//!
//! `getTreeHeight` is **two draws, unconditionally**, and `isValidPosition`
//! reads the world without drawing. So a huge mushroom that cannot stand
//! anywhere still costs the stream `nextInt(3)` and `nextInt(12)` — and this
//! is exactly how the feature was found. `dark_forest_vegetation`'s draw
//! stream diverged from vanilla's at draw 793 by that pair and stayed two
//! apart from there: vanilla was spending a mushroom's height and abandoning
//! it, while this port declined the feature and spent nothing.
//!
//! > A feature that always declines is not free. It is free only if vanilla
//! > also declines *before its first draw*.
//!
//! # `getTreeRadiusForHeight` is called with vanilla's own dead arguments
//!
//! `isValidPosition` calls it as `getTreeRadiusForHeight(-1, -1, radius, y)`.
//! For the brown mushroom that yields `y <= 3 ? 0 : radius`, a real widening
//! of the check above the fourth block. For the red one the body is
//!
//! ```text
//!   if (y < height && y >= height - 3) return radius;
//!   if (y == height)                   return radius;
//!   return 0;
//! ```
//!
//! with `height` bound to the **second** argument, which is the constant
//! `-1`. `y` runs `0..=height`, so neither branch can fire and the red
//! mushroom's validity check is a single column, forever. That is vanilla's
//! behaviour and it is reproduced rather than corrected: widening it would
//! refuse mushrooms vanilla places.
//!
//! # The provider is asked at the origin, not at the block being written
//!
//! Both `makeCap` and `placeTrunk` pass `pos` — the feature's origin — to
//! `BlockStateProvider.getState`, while writing through a mutable cursor at
//! some offset from it. For the `simple_state_provider` both mushrooms use
//! that cannot matter; for a noise or dual-noise provider it decides the
//! answer, so the origin is what this port passes too.

use crate::rng::RandomSource;
use crate::tags::BlockTags;

use super::block_predicate::{BlockPredicate, NeedsBehaviour, PredicateWorld};
use super::simple_block::Unplaceable;
use super::state_provider::StateProvider;
use super::state_spec::DefaultStates;

/// Which of the two features this is. They share everything except the cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Brown,
    Red,
}

/// `HugeMushroomFeatureConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct HugeMushroomConfig {
    pub cap_provider: StateProvider,
    pub stem_provider: StateProvider,
    /// `Codec.intRange(0, 12).optionalFieldOf("foliage_radius", 2)`.
    pub foliage_radius: i32,
    pub can_place_on: BlockPredicate,
}

impl HugeMushroomConfig {
    /// # Errors
    /// If a field is missing or names something this port does not implement.
    pub fn parse(config: &serde_json::Value, states: &dyn DefaultStates) -> Result<Self, String> {
        let provider = |name: &str| -> Result<StateProvider, String> {
            let raw = config
                .get(name)
                .ok_or_else(|| format!("huge mushroom config has no `{name}`"))?;
            StateProvider::parse(raw, states)
        };
        Ok(Self {
            cap_provider: provider("cap_provider")?,
            stem_provider: provider("stem_provider")?,
            foliage_radius: config
                .get("foliage_radius")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(2),
            can_place_on: BlockPredicate::parse(
                config
                    .get("can_place_on")
                    .ok_or_else(|| "huge mushroom config has no `can_place_on`".to_string())?,
            )?,
        })
    }
}

/// What the feature needs of the world.
pub trait HugeMushroomWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `LevelHeightAccessor#getMinY`.
    fn min_y(&self) -> i32;
    /// `LevelHeightAccessor#getMaxY` — **inclusive**, as the nether feature
    /// already reads it.
    fn max_y(&self) -> i32;
}

/// `AbstractHugeMushroomFeature#getTreeHeight` — the two draws.
///
/// ```java
///   int i = random.nextInt(3) + 4;
///   if (random.nextInt(12) == 0) i *= 2;
///   return i;
/// ```
///
/// Both draws happen every time; the second is not guarded by the first.
fn tree_height<R: RandomSource + ?Sized>(random: &mut R) -> i32 {
    let mut height = random.next_i32_bound(3) + 4;
    if random.next_i32_bound(12) == 0 {
        height *= 2;
    }
    height
}

/// `HugeBrownMushroomFeature`/`HugeRedMushroomFeature#getTreeRadiusForHeight`,
/// as `isValidPosition` calls it: `(-1, -1, foliage_radius, y)`.
///
/// See the module docs — the red one's `height` parameter is the literal
/// `-1`, so it can never return anything but zero from this call site.
const fn tree_radius_for_height(variant: Variant, radius: i32, y: i32) -> i32 {
    match variant {
        Variant::Brown => {
            if y > 3 {
                radius
            } else {
                0
            }
        }
        // `height` is the constant -1 here and `y >= 0`, so both branches of
        // vanilla's body are unreachable.
        //
        // **The two arms really do both return `radius`, in the jar.** The
        // module header quotes the body: `if (y < height && y >= height - 3)
        // return radius; if (y == height) return radius; return 0;` — two
        // separate tests with the same result, and `height` bound to the
        // second argument, the literal `-1`, at the only call site
        // (`isValidPosition`). Merging them into `y >= -4 && y <= -1` would be
        // arithmetically equal and would erase which of vanilla's two tests a
        // future `y` fell through, which is the thing this transcription is
        // for. Allowed rather than rewritten, for the same reason the whole
        // dead branch is reproduced rather than corrected.
        #[allow(clippy::if_same_then_else)]
        Variant::Red => {
            if y < -1 && y >= -4 {
                radius
            } else if y == -1 {
                radius
            } else {
                0
            }
        }
    }
}

/// `AbstractHugeMushroomFeature#placeMushroomBlock` — write only into air or
/// something the mushroom is allowed to replace.
fn place_mushroom_block<W: HugeMushroomWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: (i32, i32, i32),
    state: &str,
) {
    let there = world.block_at(pos.0, pos.1, pos.2);
    let plain = there.split('[').next().unwrap_or(&there);
    if plain == "minecraft:air"
        || plain == "minecraft:cave_air"
        || plain == "minecraft:void_air"
        || tags.contains("minecraft:replaceable_by_mushrooms", plain)
    {
        world.set_block(pos.0, pos.1, pos.2, state);
    }
}

/// `AbstractHugeMushroomFeature#isValidPosition`. Reads the world, draws
/// nothing.
fn is_valid_position<W: HugeMushroomWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeMushroomConfig,
    variant: Variant,
    origin: (i32, i32, i32),
    height: i32,
) -> Result<bool, NeedsBehaviour> {
    let (x, y, z) = origin;
    if y < world.min_y() + 1 || y + height + 1 > world.max_y() {
        return Ok(false);
    }
    if !config.can_place_on.test(world, tags, (x, y - 1, z))? {
        return Ok(false);
    }
    for dy in 0..=height {
        let radius = tree_radius_for_height(variant, config.foliage_radius, dy);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let state = world.block_at(x + dx, y + dy, z + dz);
                let plain = state.split('[').next().unwrap_or(&state);
                let is_air = plain == "minecraft:air"
                    || plain == "minecraft:cave_air"
                    || plain == "minecraft:void_air";
                if !is_air && !tags.contains("minecraft:leaves", plain) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// `HugeBrownMushroomFeature#makeCap` — one flat disc with its corners cut.
fn make_cap_brown<W: HugeMushroomWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeMushroomConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    height: i32,
) -> Result<(), NeedsBehaviour> {
    let radius = config.foliage_radius;
    for x in -radius..=radius {
        for z in -radius..=radius {
            let west_edge = x == -radius;
            let east_edge = x == radius;
            let north_edge = z == -radius;
            let south_edge = z == radius;
            let x_edge = west_edge || east_edge;
            let z_edge = north_edge || south_edge;
            // The four corners of the square are not part of the cap.
            if x_edge && z_edge {
                continue;
            }
            let pos = (origin.0 + x, origin.1 + height, origin.2 + z);
            // The provider is asked at the ORIGIN. See the module docs.
            let state = config.cap_provider.state(world, tags, random, origin)?;
            let state = with_cap_faces(
                &state,
                CapFaces {
                    up: None,
                    west: west_edge || (z_edge && x == 1 - radius),
                    east: east_edge || (z_edge && x == radius - 1),
                    north: north_edge || (x_edge && z == 1 - radius),
                    south: south_edge || (x_edge && z == radius - 1),
                },
            );
            place_mushroom_block(world, tags, pos, &state);
        }
    }
    Ok(())
}

/// `HugeRedMushroomFeature#makeCap` — four layers, the top one a block
/// narrower, and only the rim below it.
fn make_cap_red<W: HugeMushroomWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeMushroomConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    height: i32,
) -> Result<(), NeedsBehaviour> {
    for y in (height - 3)..=height {
        let radius = if y < height {
            config.foliage_radius
        } else {
            config.foliage_radius - 1
        };
        let inner = config.foliage_radius - 2;
        for x in -radius..=radius {
            for z in -radius..=radius {
                let west_edge = x == -radius;
                let east_edge = x == radius;
                let north_edge = z == -radius;
                let south_edge = z == radius;
                let x_edge = west_edge || east_edge;
                let z_edge = north_edge || south_edge;
                // Below the top layer only the rim is drawn — a position is
                // on the rim when exactly one of the two axes is at its
                // extreme, which is `xEdge != zEdge`.
                if y < height && x_edge == z_edge {
                    continue;
                }
                let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                let state = config.cap_provider.state(world, tags, random, origin)?;
                let state = with_cap_faces(
                    &state,
                    CapFaces {
                        up: Some(y >= height - 1),
                        west: x < -inner,
                        east: x > inner,
                        north: z < -inner,
                        south: z > inner,
                    },
                );
                place_mushroom_block(world, tags, pos, &state);
            }
        }
    }
    Ok(())
}

/// The faces `HugeMushroomBlock` carries, as the two `makeCap`s set them.
/// `up` is `None` for the brown mushroom, which does not touch it.
struct CapFaces {
    up: Option<bool>,
    west: bool,
    east: bool,
    north: bool,
    south: bool,
}

/// `state.setValue(WEST, ..).setValue(EAST, ..)...`, guarded by vanilla's own
/// `hasProperty` chain: a provider that hands back something which is not a
/// `HugeMushroomBlock` is written through untouched.
fn with_cap_faces(state: &str, faces: CapFaces) -> String {
    let needed: &[&str] = if faces.up.is_some() {
        &["west", "east", "north", "south", "up"]
    } else {
        &["west", "east", "north", "south"]
    };
    if !needed.iter().all(|p| state.contains(&format!("{p}="))) {
        return state.to_string();
    }
    let mut out = state.to_string();
    let mut set = |name: &str, value: bool| {
        out = super::multiface_growth::with_property(
            &out,
            name,
            if value { "true" } else { "false" },
        );
    };
    if let Some(up) = faces.up {
        set("up", up);
    }
    set("west", faces.west);
    set("east", faces.east);
    set("north", faces.north);
    set("south", faces.south);
    out
}

/// `AbstractHugeMushroomFeature#placeTrunk`.
fn place_trunk<W: HugeMushroomWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeMushroomConfig,
    random: &mut R,
    origin: (i32, i32, i32),
    height: i32,
) -> Result<(), NeedsBehaviour> {
    for i in 0..height {
        let pos = (origin.0, origin.1 + i, origin.2);
        // Again the origin, not the cursor.
        let state = config.stem_provider.state(world, tags, random, origin)?;
        place_mushroom_block(world, tags, pos, &state);
    }
    Ok(())
}

/// `AbstractHugeMushroomFeature#place`.
///
/// # Errors
/// If a provider or the placement predicate needs behaviour this port does
/// not model.
pub fn place<W: HugeMushroomWorld + ?Sized, R: RandomSource + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &HugeMushroomConfig,
    variant: Variant,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, Unplaceable> {
    // Unconditional, and before the position is judged.
    let height = tree_height(random);
    if !is_valid_position(world, tags, config, variant, origin, height)
        .map_err(|NeedsBehaviour(what)| Unplaceable(what))?
    {
        return Ok(false);
    }
    let result = match variant {
        Variant::Brown => make_cap_brown(world, tags, config, random, origin, height),
        Variant::Red => make_cap_red(world, tags, config, random, origin, height),
    };
    result.map_err(|NeedsBehaviour(what)| Unplaceable(what))?;
    place_trunk(world, tags, config, random, origin, height)
        .map_err(|NeedsBehaviour(what)| Unplaceable(what))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A flat world of air over one block of the given ground.
    struct Flat {
        ground: String,
        written: BTreeMap<(i32, i32, i32), String>,
    }

    impl PredicateWorld for Flat {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            if let Some(state) = self.written.get(&(x, y, z)) {
                return state.clone();
            }
            if y <= 0 {
                self.ground.clone()
            } else {
                "minecraft:air".to_string()
            }
        }
    }

    impl HugeMushroomWorld for Flat {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.written.insert((x, y, z), state.to_string());
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            319
        }
    }

    /// The three tags this feature reads, out of the real extract rather
    /// than invented here: `replaceable_by_mushrooms` nests
    /// `#minecraft:leaves` and `#minecraft:small_flowers`, and a hand-written
    /// stand-in would not.
    fn tags() -> BlockTags {
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/vanilla/tags_block.json"
        ));
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("the tag extract"))
                .expect("json");
        let mut raw = BTreeMap::new();
        for (name, body) in json.as_object().expect("an object") {
            let values = body
                .get("values")
                .and_then(serde_json::Value::as_array)
                .expect("a tag has values");
            raw.insert(
                format!("minecraft:{name}"),
                values
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect(),
            );
        }
        BlockTags::from_entries(raw).expect("tags")
    }

    fn config(name: &str) -> Option<(HugeMushroomConfig, Variant)> {
        let extract = super::super::state_spec::extract_or_skip()?;
        let path = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/vanilla/configured_features.json"
        ));
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
        let raw = json.get(name)?.get("config")?;
        let variant = if name.contains("brown") {
            Variant::Brown
        } else {
            Variant::Red
        };
        Some((
            HugeMushroomConfig::parse(raw, &extract).expect("parses"),
            variant,
        ))
    }

    /// The two draws are unconditional and in this order. A mushroom that
    /// cannot stand anywhere still spends them, which is the whole reason
    /// this feature had to be ported rather than left refused.
    #[test]
    fn get_tree_height_always_spends_both_draws() {
        struct Counting {
            inner: crate::rng::LegacyRandomSource,
            draws: usize,
        }
        impl crate::rng::RandomSource for Counting {
            fn set_seed(&mut self, seed: i64) {
                self.inner.set_seed(seed);
            }
            fn next_gaussian(&mut self) -> f64 {
                self.draws += 1;
                self.inner.next_gaussian()
            }
            fn next_i32(&mut self) -> i32 {
                self.draws += 1;
                self.inner.next_i32()
            }
            fn next_i32_bound(&mut self, bound: i32) -> i32 {
                self.draws += 1;
                self.inner.next_i32_bound(bound)
            }
            fn next_i64(&mut self) -> i64 {
                self.draws += 1;
                self.inner.next_i64()
            }
            fn next_f32(&mut self) -> f32 {
                self.draws += 1;
                self.inner.next_f32()
            }
            fn next_f64(&mut self) -> f64 {
                self.draws += 1;
                self.inner.next_f64()
            }
            fn next_bool(&mut self) -> bool {
                self.draws += 1;
                self.inner.next_bool()
            }
        }
        for seed in 0..64 {
            let mut r = Counting {
                inner: crate::rng::LegacyRandomSource::new(seed),
                draws: 0,
            };
            let h = tree_height(&mut r);
            assert_eq!(r.draws, 2, "seed {seed} spent {} draws", r.draws);
            // `nextInt(3) + 4` is 4..=6, doubled at most once.
            assert!(
                (4..=6).contains(&h) || (8..=12).contains(&h),
                "seed {seed} gave height {h}"
            );
        }
    }

    /// Vanilla's brown cap is one layer of `(2r+1)^2` with the four corners
    /// cut — 45 blocks at the datapack's radius of 3. Measured in a real
    /// vanilla world before it was asserted here: the 3x3 around chunk
    /// (-1056,-672) at seed 20260722 holds exactly 45 `brown_mushroom_block`
    /// and 4 `mushroom_stem`, which is one mushroom of height 4.
    #[test]
    fn the_brown_cap_is_a_square_with_its_corners_cut() {
        let Some((config, variant)) = config("minecraft:huge_brown_mushroom") else {
            return;
        };
        assert_eq!(variant, Variant::Brown);
        assert_eq!(config.foliage_radius, 3);
        let mut world = Flat {
            ground: "minecraft:podzol".to_string(),
            written: BTreeMap::new(),
        };
        let tags = tags();
        let mut random = crate::rng::LegacyRandomSource::new(1);
        let placed = place(&mut world, &tags, &config, variant, &mut random, (0, 1, 0))
            .expect("no unported behaviour");
        assert!(placed, "a mushroom on podzol under open air should stand");
        let caps = world
            .written
            .values()
            .filter(|s| s.starts_with("minecraft:brown_mushroom_block"))
            .count();
        let stems = world
            .written
            .values()
            .filter(|s| s.starts_with("minecraft:mushroom_stem"))
            .count();
        let radius = config.foliage_radius;
        let expected = ((2 * radius + 1) * (2 * radius + 1) - 4) as usize;
        assert_eq!(caps, expected, "brown cap should be {expected} blocks");
        assert!(stems >= 4, "the stem should be at least the minimum height");
    }

    /// The red mushroom's `foliage_radius` is absent from the datapack, so
    /// the codec's default of 2 is what decides its whole shape. A parser
    /// that defaulted it to the brown one's 3 would build a cap a block wider
    /// in every direction and still look plausible.
    #[test]
    fn the_red_mushroom_takes_the_codec_default_radius() {
        let Some((config, variant)) = config("minecraft:huge_red_mushroom") else {
            return;
        };
        assert_eq!(variant, Variant::Red);
        assert_eq!(config.foliage_radius, 2);
    }

    /// Vanilla calls `getTreeRadiusForHeight(-1, -1, radius, y)` from
    /// `isValidPosition`, and the red mushroom binds `height` to that literal
    /// `-1`. With `y >= 0` neither branch can fire, so the red mushroom's
    /// validity check is a single column however wide its cap is. Reproduced
    /// rather than corrected — widening it would refuse mushrooms vanilla
    /// places.
    #[test]
    fn the_red_validity_check_is_one_column_and_the_brown_one_is_not() {
        for y in 0..=12 {
            assert_eq!(
                tree_radius_for_height(Variant::Red, 2, y),
                0,
                "red at y={y} should never widen"
            );
        }
        assert_eq!(tree_radius_for_height(Variant::Brown, 3, 3), 0);
        assert_eq!(tree_radius_for_height(Variant::Brown, 3, 4), 3);
    }

    /// `placeMushroomBlock` writes into air or a member of
    /// `replaceable_by_mushrooms`, and nothing else — a mushroom does not eat
    /// stone.
    #[test]
    fn a_mushroom_does_not_write_through_stone() {
        let Some((config, variant)) = config("minecraft:huge_brown_mushroom") else {
            return;
        };
        let tags = tags();
        let mut world = Flat {
            ground: "minecraft:podzol".to_string(),
            written: BTreeMap::new(),
        };
        // A stone slab right where the stem wants to be.
        world.set_block(0, 2, 0, "minecraft:stone");
        let mut random = crate::rng::LegacyRandomSource::new(1);
        let _ = place(&mut world, &tags, &config, variant, &mut random, (0, 1, 0));
        assert_eq!(
            world.written.get(&(0, 2, 0)).map(String::as_str),
            Some("minecraft:stone"),
            "the stem must not overwrite stone"
        );
    }
}
