//! `buried_treasure` — one chest, and the sand it sits in.
//!
//! Reference (26.2 bytecode, transcribed — CLAUDE.md rule 4):
//! `BuriedTreasureStructure`, `BuriedTreasurePieces$BuriedTreasurePiece`,
//! `StructurePiece.reorient` and `StructurePiece.createChest`.
//!
//! # Why this one
//!
//! It is the only structure set vanilla places in the seed-20260722 r7
//! harvest that this server did not, and its whole cost there is **one
//! block**: `minecraft:chest[facing=east,type=single,waterlogged=false]` at
//! (1177,53,745), where we served `minecraft:sand`.
//! `crates/rustmc-server/tests/served_vs_vanilla.rs` is where that was
//! measured and it is where the close is read off.
//!
//! # The shape of it
//!
//! `findGenerationPoint` is `onTopOfChunkCenter(context, OCEAN_FLOOR_WG, ..)`
//! and **never rejects**: it is `Optional.of(..)` unconditionally, so the
//! placement chunk test and the biome filter are the only two gates. The
//! piece itself is constructed at `(chunkPos.getBlockX(9), 90, chunkPos.getBlockZ(9))`
//! — a literal 90, which is not a height and is never used as one. The scan
//! below starts from `level.getHeight(OCEAN_FLOOR_WG, ..)` instead, and the
//! 90 survives only in vanilla's saved `Children[0].BB` until `postProcess`
//! overwrites the bounding box with where the chest actually went.
//!
//! # It spends nothing from the decoration stream
//!
//! The only `nextLong` in the whole path is `ChestBlockEntity.setLootTable`,
//! inside an `instanceof ChestBlockEntity` guard, and it is a draw on the
//! structure's own per-chunk random rather than on the feature stream. So
//! placing a buried treasure moves no other feature — the same argument the
//! ocean ruin's call site already records for itself. Nothing here needs a
//! measured step index.
//!
//! The loot table and its seed are recorded by the caller,
//! `worldgen_assembly::generate::place_buried_treasure_in_chunk`, on the
//! block-entity channel: `buried_treasure`'s step-3 stream, first `nextLong`.

use crate::structure::pieces::BoundingBox;
use crate::structure::place::PieceWorld;

/// `Direction.values()` — `DOWN, UP, NORTH, SOUTH, WEST, EAST`, read off
/// `Direction.$values()` rather than remembered.
///
/// The order is behaviour: the six writes below happen in it, and two of them
/// can land on the same position only through the `UP` special case, so a
/// reordering would be invisible in most worlds and wrong in some.
const VALUES: [(i32, i32, i32); 6] = [
    (0, -1, 0), // DOWN
    (0, 1, 0),  // UP
    (0, 0, -1), // NORTH
    (0, 0, 1),  // SOUTH
    (-1, 0, 0), // WEST
    (1, 0, 0),  // EAST
];
/// The index of `UP` in [`VALUES`]. `postProcess` treats it specially.
const UP: usize = 1;

/// `Direction.Plane.HORIZONTAL` — `NORTH, EAST, SOUTH, WEST`, in that order,
/// read off `Direction$Plane.<clinit>`.
///
/// **Not** `Direction.values()` filtered, which would give NORTH, SOUTH,
/// WEST, EAST. Where the two orders differ is narrower than it looks and
/// worth stating exactly, because the obvious reading is wrong: the
/// solid-neighbour half of `reorient` is order-INDEPENDENT — one solid
/// neighbour picks itself and two or more abandon the search, whichever order
/// they are met in. What the order decides is the **chest short-circuit**,
/// which returns the moment a neighbouring chest is seen. A chest to the west
/// with solid blocks north and east is answered by this order (break at east,
/// the chest never reached) and differently by the other (chest reached
/// first). `two_solids_before_a_chest_never_reach_the_chest` is that case.
const HORIZONTAL: [(&str, i32, i32); 4] = [
    ("north", 0, -1),
    ("east", 1, 0),
    ("south", 0, 1),
    ("west", -1, 0),
];

/// The five blocks `postProcess` will bury a chest on top of, in the order it
/// tests them.
const FLOOR: [&str; 5] = [
    "minecraft:sandstone",
    "minecraft:stone",
    "minecraft:andesite",
    "minecraft:granite",
    "minecraft:diorite",
];

/// A start: the column the chest is looked for in.
///
/// No `y`. Vanilla's piece carries one — the literal 90 — and never reads it;
/// carrying it here would invite the same mistake the ruined portal's own
/// notes warn about, where a saved coordinate is mistaken for a height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    /// `chunkPos.getBlockX(9)`, i.e. `chunk_x * 16 + 9`.
    pub x: i32,
    /// `chunkPos.getBlockZ(9)`.
    pub z: i32,
}

impl Piece {
    /// The piece vanilla builds for a start in this chunk.
    #[must_use]
    pub const fn for_chunk(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            x: chunk_x * 16 + 9,
            z: chunk_z * 16 + 9,
        }
    }
}

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `BlockState.isAir()` — the three air blocks and nothing else.
fn is_air(state: &str) -> bool {
    matches!(
        block_of(state),
        "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

/// `BuriedTreasurePieces.isLiquid` — `is(WATER) || is(LAVA)`, on the block
/// and so at any level, flowing included.
fn is_liquid(state: &str) -> bool {
    matches!(block_of(state), "minecraft:water" | "minecraft:lava")
}

fn opposite(dir: usize) -> usize {
    match dir {
        0 => 2,
        1 => 3,
        2 => 0,
        _ => 1,
    }
}

fn clockwise(dir: usize) -> usize {
    (dir + 1) % 4
}

/// `StructurePiece.reorient(BlockGetter, BlockPos, BlockState)`, for a chest.
///
/// Returns the `facing` the chest should take, or `None` when a neighbouring
/// chest short-circuits the walk and vanilla returns the state unchanged —
/// which for a default chest state means `facing=north`, but the caller says
/// that rather than this function inventing it.
///
/// The two halves are different searches and the second one is only reached
/// when the first finds no single solid neighbour:
///
/// * walk `HORIZONTAL`; a neighbouring chest returns immediately; the first
///   solid-render neighbour is remembered and a **second** one abandons the
///   whole search (`found = null; break`);
/// * otherwise start from the state's own facing and turn away from solid
///   faces: opposite, then clockwise, then opposite again.
fn reorient<W: PieceWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
    solid_render: &dyn Fn(&str) -> bool,
) -> Option<&'static str> {
    let mut found: Option<usize> = None;
    for (i, (_, dx, dz)) in HORIZONTAL.iter().enumerate() {
        let side = world.block_at(x + dx, y, z + dz);
        if block_of(&side) == "minecraft:chest" {
            return None;
        }
        if solid_render(&side) {
            if found.is_none() {
                found = Some(i);
            } else {
                found = None;
                break;
            }
        }
    }
    if let Some(i) = found {
        return Some(HORIZONTAL[opposite(i)].0);
    }
    // `Blocks.CHEST.defaultBlockState()`'s `facing` is `north`, which is
    // index 0 of `HORIZONTAL`.
    let mut dir = 0usize;
    let at = |world: &mut W, d: usize| {
        let (_, dx, dz) = HORIZONTAL[d];
        world.block_at(x + dx, y, z + dz)
    };
    if solid_render(&at(world, dir)) {
        dir = opposite(dir);
    }
    if solid_render(&at(world, dir)) {
        dir = clockwise(dir);
    }
    if solid_render(&at(world, dir)) {
        dir = opposite(dir);
    }
    Some(HORIZONTAL[dir].0)
}

/// What `postProcess` did, for a caller that wants to say so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placed {
    /// A chest went in at this world position.
    Chest(i32, i32, i32),
    /// The floor was found and the sand written, but `createChest` refused:
    /// the position is outside the box this chunk may write, or a chest is
    /// already there.
    NoChest,
    /// The scan reached the bottom of the world without finding a floor
    /// block. Vanilla writes nothing at all in that case.
    NoFloor,
}

/// `BuriedTreasurePiece.postProcess`.
///
/// `top_y` is `level.getHeight(OCEAN_FLOOR_WG, bb.minX, bb.minZ)` — the
/// **live** `_WG` fossil off the canvas, which is `Heightmap.getFirstAvailable`
/// and therefore one above the highest matching block. `min_y` is
/// `level.getMinY()`; the loop condition is `> min_y`, so the bottom row is
/// never tested.
///
/// `chunk_box` is the `BoundingBox` parameter `placeInChunk` hands the piece,
/// and it gates **only** the chest. The six neighbour writes are plain
/// `WorldGenLevel.setBlock` calls with no box test at all, which is safe here
/// for a reason worth stating rather than relying on: the piece sits at
/// `x = chunk_x * 16 + 9` and `z = chunk_z * 16 + 9`, so no neighbour of it
/// can leave the chunk.
pub fn post_process<W: PieceWorld + ?Sized>(
    world: &mut W,
    piece: &Piece,
    top_y: i32,
    min_y: i32,
    chunk_box: BoundingBox,
    solid_render: &dyn Fn(&str) -> bool,
) -> Placed {
    let (x, z) = (piece.x, piece.z);
    let mut y = top_y;
    while y > min_y {
        let state = world.block_at(x, y, z);
        let below = world.block_at(x, y - 1, z);
        if !FLOOR.contains(&block_of(&below)) {
            y -= 1;
            continue;
        }
        // `state.isAir() || isLiquid(state) ? SAND.defaultBlockState() : state`
        let replacement = if is_air(&state) || is_liquid(&state) {
            world
                .default_state("minecraft:sand")
                .unwrap_or_else(|| "minecraft:sand".to_string())
        } else {
            state.clone()
        };
        for (dir, (dx, dy, dz)) in VALUES.into_iter().enumerate() {
            let (sx, sy, sz) = (x + dx, y + dy, z + dz);
            let side = world.block_at(sx, sy, sz);
            if !(is_air(&side) || is_liquid(&side)) {
                continue;
            }
            let under = world.block_at(sx, sy - 1, sz);
            if (is_air(&under) || is_liquid(&under)) && dir != UP {
                // Nothing to stand on: the floor material goes in instead, so
                // the chest is not left hanging over a cave.
                world.set_block(sx, sy, sz, &below);
            } else {
                world.set_block(sx, sy, sz, &replacement);
            }
        }
        // `createChest(level, box, random, pos, BURIED_TREASURE, null)`.
        if !chunk_box.is_inside(x, y, z) {
            return Placed::NoChest;
        }
        let here = world.block_at(x, y, z);
        if block_of(&here) == "minecraft:chest" {
            return Placed::NoChest;
        }
        let Some(facing) = reorient(world, x, y, z, solid_render) else {
            // A neighbouring chest: `reorient` hands the state back
            // untouched, so the default `facing=north` is what is written.
            let chest = with_facing(world, "north");
            world.set_block(x, y, z, &chest);
            return Placed::Chest(x, y, z);
        };
        let chest = with_facing(world, facing);
        world.set_block(x, y, z, &chest);
        return Placed::Chest(x, y, z);
    }
    Placed::NoFloor
}

/// `Blocks.CHEST.defaultBlockState().setValue(FACING, dir)`.
///
/// Built from the extract's own default state so `type` and `waterlogged`
/// come from the table rather than from this file's memory of them; the
/// literal is the fallback for a toy world in a unit test, exactly as
/// `PieceWorld::default_state` documents.
fn with_facing<W: PieceWorld + ?Sized>(world: &mut W, facing: &str) -> String {
    let default = world.default_state("minecraft:chest").unwrap_or_else(|| {
        "minecraft:chest[facing=north,type=single,waterlogged=false]".to_string()
    });
    let Some((block, rest)) = default.split_once('[') else {
        return format!("{default}[facing={facing}]");
    };
    let props: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|kv| {
            if kv.starts_with("facing=") {
                format!("facing={facing}")
            } else {
                kv.to_string()
            }
        })
        .collect();
    format!("{block}[{}]", props.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A world of explicit blocks over an implicit sea of air.
    struct Toy {
        blocks: BTreeMap<(i32, i32, i32), String>,
        writes: Vec<((i32, i32, i32), String)>,
    }
    impl crate::block_entity::BlockEntityRecorder for Toy {
        fn record_block_entity(
            &mut self,
            _x: i32,
            _y: i32,
            _z: i32,
            _record: crate::block_entity::Record,
        ) {
        }
    }

    impl PieceWorld for Toy {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
            self.writes.push(((x, y, z), state.to_string()));
        }
        fn world_surface_wg(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
        fn block_in_tag(&self, _tag: &str, _block: &str) -> bool {
            false
        }
        fn default_state(&self, block: &str) -> Option<String> {
            match block {
                "minecraft:chest" => {
                    Some("minecraft:chest[facing=north,type=single,waterlogged=false]".to_string())
                }
                "minecraft:sand" => Some("minecraft:sand".to_string()),
                _ => None,
            }
        }
    }
    fn toy(blocks: &[((i32, i32, i32), &str)]) -> Toy {
        Toy {
            blocks: blocks
                .iter()
                .map(|((x, y, z), s)| ((*x, *y, *z), (*s).to_string()))
                .collect(),
            writes: Vec::new(),
        }
    }
    fn whole_world() -> BoundingBox {
        BoundingBox::new(-1000, -1000, -1000, 1000, 1000, 1000)
    }
    fn solid(state: &str) -> bool {
        !is_air(state) && !is_liquid(state) && state != "minecraft:chest"
    }

    /// The piece is at chunk-local (9,9), never on an edge, which is what
    /// makes the unclamped neighbour writes safe.
    #[test]
    fn the_column_is_always_nine_nine_inside_its_chunk() {
        for (cx, cz) in [(0, 0), (-1, -1), (73, 46), (1_000_000, -1_000_000)] {
            let p = Piece::for_chunk(cx, cz);
            assert_eq!(p.x.rem_euclid(16), 9);
            assert_eq!(p.z.rem_euclid(16), 9);
        }
    }

    /// A stone floor under water: the chest goes on the stone and the water
    /// around it becomes sand.
    ///
    /// And the facing is `east`, which is not obvious and is the point. The
    /// six sand writes run BEFORE `createChest`, so `reorient` sees the sand
    /// this piece has just placed: all four horizontal neighbours are solid,
    /// the two-solids arm abandons the search, and the fallback walk turns
    /// north -> south -> west -> east and runs out of turns. A port that
    /// reordered those two halves would answer `north` here.
    #[test]
    fn a_chest_lands_on_the_first_floor_block_under_water() {
        let mut w = toy(&[
            ((9, 59, 9), "minecraft:stone"),
            ((8, 59, 9), "minecraft:stone"),
            ((10, 59, 9), "minecraft:stone"),
            ((9, 59, 8), "minecraft:stone"),
            ((9, 59, 10), "minecraft:stone"),
            ((9, 60, 9), "minecraft:water[level=0]"),
        ]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            62,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::Chest(9, 60, 9));
        assert_eq!(
            w.block_at(9, 60, 9),
            "minecraft:chest[facing=east,type=single,waterlogged=false]"
        );
        // DOWN is stone already, so it is skipped; UP and the four sides were
        // water or air and become sand.
        assert_eq!(w.block_at(9, 61, 9), "minecraft:sand");
        assert_eq!(w.block_at(8, 60, 9), "minecraft:sand");
    }

    /// `dir != UP` is the whole of the hanging-chest guard, and this is the
    /// case that separates it: the side is air AND the block under the side
    /// is air, so the FLOOR material goes in rather than the replacement —
    /// except upward, where nothing can hold anything up anyway.
    #[test]
    fn a_side_over_a_void_gets_the_floor_material_and_up_never_does() {
        let mut w = toy(&[((9, 40, 9), "minecraft:granite")]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            41,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::Chest(9, 41, 9));
        // Every horizontal side hangs over air, so each takes the granite.
        assert_eq!(w.block_at(8, 41, 9), "minecraft:granite");
        assert_eq!(w.block_at(9, 41, 8), "minecraft:granite");
        // UP hangs over the chest position, whose own block is air at the
        // time it is read — and UP is excluded, so it takes the replacement.
        assert_eq!(w.block_at(9, 42, 9), "minecraft:sand");
    }

    /// A single solid horizontal neighbour turns the chest to face away from
    /// it. West is solid, so `facing=east` — which is the state vanilla
    /// actually saved at (1177,53,745).
    #[test]
    fn one_solid_neighbour_faces_the_chest_away_from_it() {
        let mut w = toy(&[
            ((9, 52, 9), "minecraft:sandstone"),
            ((8, 53, 9), "minecraft:stone"),
        ]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            53,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::Chest(9, 53, 9));
        assert_eq!(
            w.block_at(9, 53, 9),
            "minecraft:chest[facing=east,type=single,waterlogged=false]"
        );
    }

    /// Two solid horizontal neighbours abandon the search entirely — the
    /// `found = null; break` arm — and the fallback walk runs instead.
    ///
    /// North and east are stone, so they survive the sand pass untouched;
    /// south and west are air over air and take the floor material, so by the
    /// time `reorient` runs all four are solid and the walk exhausts its
    /// three turns at `east`.
    #[test]
    fn two_solid_neighbours_fall_through_to_the_facing_walk() {
        let mut w = toy(&[
            ((9, 52, 9), "minecraft:diorite"),
            ((8, 53, 9), "minecraft:stone"),
            ((9, 53, 8), "minecraft:stone"),
        ]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            53,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::Chest(9, 53, 9));
        assert_eq!(
            w.block_at(9, 53, 9),
            "minecraft:chest[facing=east,type=single,waterlogged=false]"
        );
    }

    /// The one case `Direction.Plane.HORIZONTAL`'s order actually decides.
    ///
    /// A chest to the WEST and solid blocks NORTH and EAST. Under the real
    /// order — NORTH, EAST, SOUTH, WEST — the second solid neighbour breaks
    /// the loop at index 1 and the chest at index 3 is never seen, so the
    /// fallback walk runs and answers `east`. Under `Direction.values()`
    /// filtered — NORTH, SOUTH, WEST, EAST — north would be remembered, south
    /// skipped, and the chest reached at index 2, returning the state
    /// unchanged at `facing=north`. Nothing else in this file separates the
    /// two orders, which is why this test exists rather than a comment.
    #[test]
    fn two_solids_before_a_chest_never_reach_the_chest() {
        let mut w = toy(&[
            ((9, 52, 9), "minecraft:stone"),
            ((9, 53, 8), "minecraft:stone"),
            ((10, 53, 9), "minecraft:stone"),
            (
                (8, 53, 9),
                "minecraft:chest[facing=north,type=single,waterlogged=false]",
            ),
            // South must not become solid in the sand pass, or the fallback
            // walk would stop somewhere else and hide the difference.
            ((9, 53, 10), "minecraft:dirt"),
        ]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            53,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::Chest(9, 53, 9));
        assert_eq!(
            w.block_at(9, 53, 9),
            "minecraft:chest[facing=east,type=single,waterlogged=false]",
            "the chest to the west is never reached; the other order would give north"
        );
    }

    /// No floor block anywhere in the column: vanilla writes nothing at all.
    #[test]
    fn a_column_with_no_floor_writes_nothing() {
        let mut w = toy(&[((9, 40, 9), "minecraft:dirt")]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            60,
            -64,
            whole_world(),
            &solid,
        );
        assert_eq!(placed, Placed::NoFloor);
        assert!(w.writes.is_empty());
    }

    /// The box gates the chest and NOT the sand. Both halves are asserted,
    /// because a port that clamped everything would pass a chest-only check.
    #[test]
    fn the_chunk_box_gates_the_chest_but_not_the_sand() {
        let mut w = toy(&[((9, 52, 9), "minecraft:stone")]);
        let placed = post_process(
            &mut w,
            &Piece::for_chunk(0, 0),
            53,
            -64,
            BoundingBox::new(0, -64, 0, 15, 52, 15),
            &solid,
        );
        assert_eq!(placed, Placed::NoChest);
        assert_eq!(w.block_at(9, 53, 9), "minecraft:air", "no chest");
        assert_eq!(
            w.block_at(8, 53, 9),
            "minecraft:stone",
            "but the sand pass ran"
        );
    }
}
