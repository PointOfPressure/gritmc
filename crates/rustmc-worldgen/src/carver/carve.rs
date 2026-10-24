//! The shared carving primitive: cut an ellipsoid out of a chunk.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `WorldCarver#carveEllipsoid`, `#carveBlock`, `#getCarveState`,
//! `#canReach`.
//!
//! Every cave, tunnel and ravine is a string of these. The carver decides
//! where the centres go; this decides which blocks inside one actually
//! disappear.
//!
//! # The bounds are asymmetric on purpose
//!
//! `minX`/`minZ` subtract an extra 1 from the floored radius and `maxY` adds
//! one, while `maxX`/`maxZ` do not — and the y loop is `for y = maxY; y >
//! minY; y--`, so **`minY` itself is never visited**. None of that is
//! symmetric and none of it is a rounding convenience: it is the exact set of
//! blocks vanilla touches, and widening it by one carves blocks vanilla
//! leaves, which shows up as caves that are one block deeper at the floor.
//!
//! # `hasGrass` runs down the column, not per block
//!
//! It is set when the walk passes a grass block or mycelium and stays set for
//! every block **below** in the same column. That is what re-grasses the dirt
//! under a cave mouth: the carve reaches a dirt block, remembers it saw grass
//! above, and asks the surface rule for a new top material. Resetting it per
//! block leaves bare dirt rings around every cave entrance.

use super::mask::CarvingMask;
use crate::math::floor;
use crate::tags::BlockTags;

/// The world's vertical extent, standing in for `CarvingContext`.
#[derive(Clone, Copy, Debug)]
pub struct CarveBounds {
    /// `context.getMinGenY()`.
    pub min_gen_y: i32,
    /// `context.getGenDepth()`.
    pub gen_depth: i32,
    pub chunk_x: i32,
    pub chunk_z: i32,
}

impl CarveBounds {
    #[must_use]
    pub const fn min_block_x(&self) -> i32 {
        self.chunk_x * 16
    }

    #[must_use]
    pub const fn min_block_z(&self) -> i32 {
        self.chunk_z * 16
    }

    /// `ChunkPos#getMiddleBlockX` — the chunk's minimum plus eight, *not* its
    /// geometric centre at plus 7.5.
    #[must_use]
    pub const fn middle_block_x(&self) -> i32 {
        self.min_block_x() + 8
    }

    #[must_use]
    pub const fn middle_block_z(&self) -> i32 {
        self.min_block_z() + 8
    }
}

/// The chunk a carver writes into, plus the two things it has to ask the rest
/// of worldgen.
///
/// A trait rather than a concrete chunk so the geometry can be tested without
/// an aquifer or a biome source, which is most of what can go wrong here.
pub trait CarveTarget {
    /// The block name at a world position, e.g. `minecraft:stone`.
    fn block(&self, x: i32, y: i32, z: i32) -> String;

    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);

    /// `Aquifer#computeSubstance(pos, 0.0)` — what fills the hole.
    ///
    /// `None` means the aquifer declined and the block stays as it was. That
    /// is not an error case: it is how caves stop being carved below the
    /// local water table in some columns.
    fn aquifer_substance(&mut self, x: i32, y: i32, z: i32) -> Option<String>;

    /// `CarvingContext#topMaterial` — the surface rule's answer for a block
    /// that has just been exposed.
    fn top_material(&mut self, x: i32, y: i32, z: i32, under_fluid: bool) -> Option<String>;

    /// `Aquifer#shouldScheduleFluidUpdate` — read straight off the aquifer,
    /// **after** the substance call that sets it.
    ///
    /// It is a field on the aquifer and not a second return value because
    /// vanilla reads it in a branch that never called `computeSubstance`:
    /// below the configured lava level `getCarveState` returns lava without
    /// consulting the aquifer at all, and the flag then still holds whatever
    /// the *previous* position left there. Reproducing that staleness is the
    /// whole reason this is a separate method.
    fn should_schedule_fluid_update(&self) -> bool;

    /// `ChunkAccess#markPosForPostProcessing`.
    ///
    /// A carved fluid is scheduled to flow once the chunk reaches FULL. The
    /// mark is invisible in a region file, so nothing about a chunk's bytes
    /// says whether this was called — see `docs/ORACLE.md` §19.
    fn mark_post_processing(&mut self, x: i32, y: i32, z: i32);
}

/// Whether a position inside the ellipsoid's bounding box is skipped.
///
/// Cave and canyon supply different ones — the cave's cuts off a flat floor,
/// the canyon's applies a per-height width factor — and it is called for
/// every candidate block, so it is a closure rather than an enum.
pub type SkipChecker<'a> = dyn Fn(f64, f64, f64, i32) -> bool + 'a;

/// `WorldCarver#carveEllipsoid`. Returns whether anything was carved.
#[allow(clippy::too_many_arguments)]
pub fn carve_ellipsoid<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    replaceable: &str,
    style: CarveStyle,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    x: f64,
    y: f64,
    z: f64,
    horizontal_radius: f64,
    vertical_radius: f64,
    skip: &SkipChecker<'_>,
) -> bool {
    let center_x = f64::from(bounds.middle_block_x());
    let center_z = f64::from(bounds.middle_block_z());
    let max_delta = 16.0 + horizontal_radius * 2.0;
    // Written as vanilla writes it — `!(a > d) && !(b > d)` — so a NaN
    // radius takes the same branch it does there rather than the opposite.
    if (x - center_x).abs() > max_delta || (z - center_z).abs() > max_delta {
        return false;
    }

    let chunk_min_x = bounds.min_block_x();
    let chunk_min_z = bounds.min_block_z();
    let min_x_index = (floor(x - horizontal_radius) - chunk_min_x - 1).max(0);
    let max_x_index = (floor(x + horizontal_radius) - chunk_min_x).min(15);
    let min_y = (floor(y - vertical_radius) - 1).max(bounds.min_gen_y + 1);
    // `chunk.isUpgrading()` is false for a chunk being generated, so the
    // seven protected blocks at the top always apply.
    const PROTECTED_BLOCKS_ON_TOP: i32 = 7;
    let max_y = (floor(y + vertical_radius) + 1)
        .min(bounds.min_gen_y + bounds.gen_depth - 1 - PROTECTED_BLOCKS_ON_TOP);
    let min_z_index = (floor(z - horizontal_radius) - chunk_min_z - 1).max(0);
    let max_z_index = (floor(z + horizontal_radius) - chunk_min_z).min(15);

    let mut carved = false;
    for x_index in min_x_index..=max_x_index {
        let world_x = chunk_min_x + x_index;
        let xd = (f64::from(world_x) + 0.5 - x) / horizontal_radius;

        for z_index in min_z_index..=max_z_index {
            let world_z = chunk_min_z + z_index;
            let zd = (f64::from(world_z) + 0.5 - z) / horizontal_radius;
            if xd * xd + zd * zd >= 1.0 {
                continue;
            }

            // Persists down the column. See the module docs.
            let mut has_grass = false;
            let mut world_y = max_y;
            while world_y > min_y {
                let yd = (f64::from(world_y) - 0.5 - y) / vertical_radius;
                if !skip(xd, yd, zd, world_y) && !mask.get(x_index, world_y, z_index) {
                    mask.set(x_index, world_y, z_index);
                    carved |= carve_block(
                        target,
                        tags,
                        replaceable,
                        style,
                        world_x,
                        world_y,
                        world_z,
                        &mut has_grass,
                    );
                }
                world_y -= 1;
            }
        }
    }

    carved
}

/// Which `carveBlock` a carver runs.
///
/// **`NetherWorldCarver` overrides `carveBlock` outright** — it is not a
/// different configuration of the same method, and reading it as one is why
/// this enum exists rather than a lava-level number. Vanilla, verbatim in
/// structure:
///
/// ```text
///   WorldCarver.carveBlock:        note hasGrass, check canReplaceBlock,
///                                  getCarveState (lava below config.lavaLevel,
///                                  else the aquifer), write, re-grass the
///                                  dirt below through topMaterial.
///   NetherWorldCarver.carveBlock:  check canReplaceBlock, write LAVA at
///                                  y <= getMinGenY() + 31 and CAVE_AIR above.
///                                  No aquifer. No topMaterial. No hasGrass.
///                                  `config.lavaLevel` is never read.
/// ```
///
/// The 31 is a literal in the override, and the nether's own
/// `carvers_nether.json` says `lava_level: {above_bottom: 10}` — a value that
/// is parsed, carried, and then ignored. Using it would put the lava line 21
/// blocks too low.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarveStyle {
    /// `WorldCarver#carveBlock` / `#getCarveState`. Carries the resolved
    /// `config.lavaLevel`.
    Standard { lava_level_y: i32 },
    /// `NetherWorldCarver#carveBlock`. Carries `getMinGenY() + 31`.
    Nether { lava_at_or_below: i32 },
}

/// `WorldCarver#carveBlock`, and `NetherWorldCarver`'s override of it.
#[allow(clippy::too_many_arguments)]
fn carve_block<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    replaceable: &str,
    style: CarveStyle,
    x: i32,
    y: i32,
    z: i32,
    has_grass: &mut bool,
) -> bool {
    let existing = target.block(x, y, z);

    if let CarveStyle::Nether { lava_at_or_below } = style {
        // The whole override. Note what is *absent*: no `hasGrass`, no
        // aquifer, no `topMaterial` pass on the block below.
        if !tags.contains(replaceable, plain_name(&existing)) {
            return false;
        }
        let state = if y <= lava_at_or_below {
            "minecraft:lava[level=0]"
        } else {
            "minecraft:cave_air"
        };
        target.set_block(x, y, z, state);
        return true;
    }

    // `BlockState#is(Block)` compares the **block**, not the state, so
    // `plain_name` is not a convenience here — it is the comparison vanilla
    // makes. This read `existing == "minecraft:grass_block"` until
    // 2026-08-09, and the surface rule writes
    // `minecraft:grass_block[snowy=false]` (vanilla's own
    // `overworld.json` surface rule carries `"Properties": {"snowy":
    // "false"}`), so the arm could never fire in the real generator and the
    // whole re-grassing pass below it was dead code. The golden carver
    // chunk's `topMaterial was asked for 0 times` was measuring that, not
    // measuring a world with no cave mouths in it.
    let existing_block = plain_name(&existing);
    if existing_block == "minecraft:grass_block" || existing_block == "minecraft:mycelium" {
        *has_grass = true;
    }

    // Note the order: `hasGrass` is updated *before* the replaceable check,
    // so a grass block that cannot be replaced still arms the re-grassing
    // for the column below it.
    if !tags.contains(replaceable, plain_name(&existing)) {
        return false;
    }

    let Some(state) = carve_state(target, style, x, y, z) else {
        return false;
    };
    let is_fluid = is_fluid(&state);
    target.set_block(x, y, z, &state);

    // The order is vanilla's: the mark is taken immediately after the write
    // and before the re-grassing pass below, and the flag is tested first so
    // that a non-fluid carve never even asks what the block is.
    if target.should_schedule_fluid_update() && is_fluid {
        target.mark_post_processing(x, y, z);
    }

    if *has_grass && target.block(x, y - 1, z) == "minecraft:dirt" {
        if let Some(top) = target.top_material(x, y - 1, z, is_fluid) {
            target.set_block(x, y - 1, z, &top);
        }
    }

    true
}

/// `WorldCarver#getCarveState`.
fn carve_state<T: CarveTarget>(
    target: &mut T,
    style: CarveStyle,
    x: i32,
    y: i32,
    z: i32,
) -> Option<String> {
    let CarveStyle::Standard { lava_level_y } = style else {
        unreachable!("the nether style never reaches getCarveState")
    };
    if y <= lava_level_y {
        // Below the configured lava level the aquifer is not consulted at
        // all — this is why every deep cave floods rather than opening into
        // air, regardless of what the local water table says.
        //
        // `Blocks.LAVA.defaultBlockState()`, and the `[level=0]` is not
        // decoration. Vanilla writes a *state*; a bare `minecraft:lava` is a
        // different thing to anything that reads the level, and to a region
        // diff against a real world. This gate compared block names for its
        // first three days and could not see the difference — 223 blocks of
        // it, across the carver's chunks.
        return Some("minecraft:lava[level=0]".to_string());
    }
    target.aquifer_substance(x, y, z)
}

/// `canReach` — is this tunnel step still close enough to the target chunk to
/// be worth continuing?
///
/// A cheap early-out that is nonetheless behaviour: it *returns* from the
/// tunnel loop rather than skipping a step, so a wrong answer truncates the
/// rest of the tunnel and everything it would have carved.
#[must_use]
pub fn can_reach(
    bounds: CarveBounds,
    x: f64,
    z: f64,
    current_step: i32,
    total_steps: i32,
    thickness: f32,
) -> bool {
    let xd = x - f64::from(bounds.middle_block_x());
    let zd = z - f64::from(bounds.middle_block_z());
    let remaining = f64::from(total_steps - current_step);
    // `thickness + 2.0F + 16.0F` — float additions, then widened. Doing the
    // sum in double gives a different bound for thicknesses that are not
    // exactly representable.
    let rr = f64::from(thickness + 2.0 + 16.0);
    xd * xd + zd * zd - remaining * remaining <= rr * rr
}

/// Strips a `[k=v]` state suffix, since tags name blocks.
fn plain_name(name: &str) -> &str {
    name.split_once('[').map_or(name, |(n, _)| n)
}

fn is_fluid(name: &str) -> bool {
    let plain = plain_name(name);
    plain == "minecraft:water" || plain == "minecraft:lava"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A chunk of named blocks, with the aquifer stubbed to plain air.
    struct TestChunk {
        blocks: BTreeMap<(i32, i32, i32), String>,
        default: String,
        aquifer: Option<String>,
        top_material: Option<String>,
        top_material_calls: Vec<(i32, i32, i32, bool)>,
        should_schedule_fluid_update: bool,
        marks: Vec<(i32, i32, i32)>,
    }

    impl TestChunk {
        fn new() -> Self {
            Self {
                blocks: BTreeMap::new(),
                default: "minecraft:stone".to_string(),
                aquifer: Some("minecraft:cave_air".to_string()),
                top_material: None,
                top_material_calls: Vec::new(),
                should_schedule_fluid_update: false,
                marks: Vec::new(),
            }
        }

        fn carved(&self) -> usize {
            self.blocks
                .values()
                .filter(|b| b.as_str() == "minecraft:cave_air" || b.as_str() == "minecraft:lava")
                .count()
        }
    }

    impl CarveTarget for TestChunk {
        fn block(&self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| self.default.clone())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.blocks.insert((x, y, z), name.to_string());
        }
        fn aquifer_substance(&mut self, _: i32, _: i32, _: i32) -> Option<String> {
            self.aquifer.clone()
        }
        fn top_material(&mut self, x: i32, y: i32, z: i32, under_fluid: bool) -> Option<String> {
            self.top_material_calls.push((x, y, z, under_fluid));
            self.top_material.clone()
        }
        fn should_schedule_fluid_update(&self) -> bool {
            self.should_schedule_fluid_update
        }
        fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
            self.marks.push((x, y, z));
        }
    }

    fn tags() -> BlockTags {
        // Built in memory rather than through a temp directory: several of
        // these tests run in parallel and a shared directory raced.
        BlockTags::from_entries(
            [(
                "minecraft:overworld_carver_replaceables".to_string(),
                [
                    "minecraft:stone",
                    "minecraft:dirt",
                    "minecraft:grass_block",
                    "minecraft:mycelium",
                    "minecraft:water",
                ]
                .map(String::from)
                .to_vec(),
            )]
            .into_iter()
            .collect(),
        )
        .expect("test tags resolve")
    }

    fn bounds() -> CarveBounds {
        CarveBounds {
            min_gen_y: -64,
            gen_depth: 384,
            chunk_x: 0,
            chunk_z: 0,
        }
    }

    /// The plain sphere check both carvers reduce to when their extra
    /// shaping is neutral.
    fn sphere(xd: f64, yd: f64, zd: f64, _y: i32) -> bool {
        xd * xd + yd * yd + zd * zd >= 1.0
    }

    #[test]
    fn an_ellipsoid_far_from_the_chunk_carves_nothing() {
        // The early-out. Without it a carver seeded eight chunks away would
        // still walk this chunk's whole bounding box.
        let mut chunk = TestChunk::new();
        let mut mask = CarvingMask::new(384, -64);
        let carved = carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -56 },
            bounds(),
            &mut mask,
            200.0,
            40.0,
            8.0,
            4.0,
            4.0,
            &sphere,
        );
        assert!(!carved);
        assert_eq!(mask.count(), 0);
    }

    #[test]
    fn a_sphere_at_the_chunk_centre_carves_a_ball() {
        let mut chunk = TestChunk::new();
        let mut mask = CarvingMask::new(384, -64);
        let carved = carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -56 },
            bounds(),
            &mut mask,
            8.0,
            40.0,
            8.0,
            4.0,
            4.0,
            &sphere,
        );
        assert!(carved);
        assert!(chunk.carved() > 100, "carved {}", chunk.carved());
        // Everything carved is inside the radius, with a block of slack for
        // the half-block offsets.
        for ((bx, by, bz), name) in &chunk.blocks {
            if name == "minecraft:cave_air" {
                let d = (f64::from(*bx) - 8.0).hypot(f64::from(*bz) - 8.0);
                assert!(d < 5.0, "carved {bx},{by},{bz} at distance {d}");
                assert!((f64::from(*by) - 40.0).abs() < 5.0);
            }
        }
    }

    #[test]
    fn the_top_seven_blocks_of_the_world_are_never_carved() {
        // `protectedBlocksOnTop`. A carver reaching the build limit must not
        // open the sky.
        let mut chunk = TestChunk::new();
        let mut mask = CarvingMask::new(384, -64);
        carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -56 },
            bounds(),
            &mut mask,
            8.0,
            330.0,
            8.0,
            8.0,
            8.0,
            &sphere,
        );
        let highest = chunk.blocks.keys().map(|(_, y, _)| *y).max();
        if let Some(highest) = highest {
            assert!(
                highest <= -64 + 384 - 1 - 7,
                "carved at y={highest}, inside the protected top"
            );
        }
    }

    #[test]
    fn the_lowest_row_of_the_bounding_box_is_left_alone() {
        // `for (y = maxY; y > minY; y--)` — strictly greater, so minY itself
        // is never visited. One block, and it is the floor of every cave.
        let mut chunk = TestChunk::new();
        let mut mask = CarvingMask::new(384, -64);
        carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -100 },
            bounds(),
            &mut mask,
            8.0,
            40.0,
            8.0,
            4.0,
            4.0,
            &|_, _, _, _| false,
        );
        let lowest = chunk
            .blocks
            .keys()
            .map(|(_, y, _)| *y)
            .min()
            .expect("carved");
        let bounding_min = floor(40.0 - 4.0) - 1;
        assert_eq!(
            lowest,
            bounding_min + 1,
            "the bottom row of the box should be untouched"
        );
    }

    #[test]
    fn below_the_lava_level_the_aquifer_is_not_consulted() {
        let mut chunk = TestChunk::new();
        chunk.aquifer = None; // the aquifer refuses everything
        let mut mask = CarvingMask::new(384, -64);
        carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -50 },
            bounds(),
            &mut mask,
            8.0,
            -56.0,
            8.0,
            4.0,
            4.0,
            &sphere,
        );
        assert!(
            chunk
                .blocks
                .values()
                .any(|b| b == "minecraft:lava[level=0]"),
            "deep carves should flood even when the aquifer declines — and with \
             the *state* vanilla writes, not a bare block name"
        );
    }

    #[test]
    fn a_block_the_tag_excludes_is_left_but_still_arms_the_regrassing() {
        // `hasGrass` is set before the replaceable check, so a grass block
        // that cannot be replaced still re-grasses the dirt below.
        let mut chunk = TestChunk::new();
        chunk.top_material = Some("minecraft:grass_block".to_string());
        chunk.default = "minecraft:bedrock".to_string(); // not in the tag
        let mut mask = CarvingMask::new(384, -64);
        chunk.set_block(8, 40, 8, "minecraft:grass_block");
        chunk.set_block(8, 39, 8, "minecraft:dirt");

        carve_ellipsoid(
            &mut chunk,
            &tags(),
            "#minecraft:overworld_carver_replaceables",
            CarveStyle::Standard { lava_level_y: -56 },
            bounds(),
            &mut mask,
            8.0,
            40.0,
            8.0,
            2.0,
            2.0,
            &sphere,
        );
        // The grass was replaceable and got carved; the dirt beneath it was
        // then handed to the surface rule.
        assert!(
            chunk
                .top_material_calls
                .iter()
                .any(|(x, y, z, _)| (*x, *y, *z) == (8, 39, 8)),
            "the dirt under the carved grass was never re-surfaced: {:?}",
            chunk.top_material_calls
        );
    }

    #[test]
    fn can_reach_narrows_as_the_tunnel_runs_out_of_steps() {
        // Early in a tunnel almost anything is reachable; near the end only
        // positions close to the chunk are. If this were inverted, tunnels
        // would stop immediately.
        // First written with a position 300 blocks out and it failed at
        // step 0 — correctly: 112 steps of about a block each cannot cover
        // 292 blocks, so `canReach` is false there at *every* step. The
        // radius has to be within reach of the remaining steps for the test
        // to say anything about the narrowing.
        let b = bounds();
        assert!(
            can_reach(b, 50.0, 50.0, 0, 112, 3.0),
            "reachable at the start"
        );
        assert!(
            !can_reach(b, 50.0, 50.0, 110, 112, 3.0),
            "two steps left should not reach 42 blocks"
        );
        assert!(
            can_reach(b, 8.0, 8.0, 110, 112, 3.0),
            "the chunk itself is always reachable"
        );
        assert!(
            !can_reach(b, 300.0, 300.0, 0, 112, 3.0),
            "292 blocks is out of reach even with every step remaining"
        );
    }
}
