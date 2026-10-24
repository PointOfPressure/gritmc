//! `EndCityPieces` — the End city's recursive template walk.
//!
//! Reference (26.2 bytecode, behaviour ported — never transcribed, CLAUDE.md
//! rule 4): `EndCityPieces`, `EndCityPieces$EndCityPiece`,
//! `EndCityPieces$SectionGenerator` and its four anonymous implementations,
//! `EndCityStructure.findGenerationPoint`,
//! `Structure.getLowestYIn5by5BoxOffset7Blocks`, `Structure.getCornerHeights`,
//! `StructureTemplate.calculateConnectedPosition`,
//! `StructurePiece.findCollisionPiece`.
//!
//! # This is not a jigsaw, and the assembler cannot express it
//!
//! `worldgen/structure/end_city.json` is four lines and names
//! `minecraft:end_city`, not `minecraft:jigsaw`. There is no
//! `worldgen/template_pool/end_city` anywhere in the datagen extract — no
//! start pool, no `size`, no `max_distance_from_center`. So none of
//! [`crate::structure::jigsaw`] applies: there are no jigsaw blocks, no
//! `canAttach`, no free-space `VoxelShape`, no weight-expanded element lists
//! and — the part that matters most for the draw sequence — **no shuffles at
//! all**. What is shared with the jigsaw path is everything below the
//! assembler: [`Template`], [`Rotation`], `place::place_piece` and the two
//! `BlockIgnoreProcessor` ports.
//!
//! # The one integer that gates the whole walk
//!
//! `recursiveChildren` draws a raw **unbounded** `nextInt()` *after* its
//! generator returns and stamps it over every piece of its own sublist. The
//! outermost call therefore overwrites every nested one, so the `GD` a region
//! file records for the 66 recursive pieces is the stream value at position
//! `N`, where `N` is the total number of draws the entire walk consumed. One
//! integer gates element order, every rotation and every branch taken, and it
//! can be checked against a vanilla world before a single block is placed.
//!
//! # The argument-evaluation traps
//!
//! Java evaluates constructor arguments left to right, so `new BlockPos(3 +
//! random.nextInt(2), -3, 3 + random.nextInt(2))` draws **x before z**.
//! Getting that backwards is a silent two-draw swap that produces a
//! well-formed city in the wrong place. The same shape appears in the ship
//! offset. Both are written here as separate statements so the order is
//! visible rather than implied.

use std::sync::Arc;

use crate::rng::RandomSource;
use crate::structure::pieces::BoundingBox;
use crate::structure::template::{transform, Mirror, Rotation, Template};

/// `EndCityPieces.MAX_GEN_DEPTH`.
const MAX_GEN_DEPTH: i32 = 8;

/// One placed `EndCityPiece`, in the order `startHouseTower` builds it.
///
/// `gen_depth` is the `GD` a region file records. It is not a depth: the
/// constructor passes a literal `0`, `TOWER_BRIDGE` sets `-1` on its bridge
/// ends, and `recursiveChildren` overwrites the lot with a raw `nextInt()`.
#[derive(Clone, Debug)]
pub struct Piece {
    /// The template's name, without the `end_city/` prefix.
    pub template: String,
    /// `TemplateStructurePiece.templatePosition`, after `move`.
    pub pos: (i32, i32, i32),
    pub rotation: Rotation,
    /// `makeSettings`' flag: `true` places with `BlockIgnoreProcessor
    /// .STRUCTURE_BLOCK` (air *is* written), `false` with `STRUCTURE_AND_AIR`.
    pub overwrite: bool,
    pub bbox: BoundingBox,
    pub gen_depth: i32,
}

/// `StructureTemplateManager.getOrCreate`, narrowed to what the walk needs.
pub trait Templates {
    /// The named template, or `None` if the extract does not hold it.
    ///
    /// A missing template is not skippable: vanilla throws, and a walk that
    /// silently dropped a piece would still produce a plausible city with a
    /// different draw count.
    fn get(&mut self, name: &str) -> Option<Arc<Template>>;
}

impl<F> Templates for F
where
    F: FnMut(&str) -> Option<Arc<Template>>,
{
    fn get(&mut self, name: &str) -> Option<Arc<Template>> {
        self(name)
    }
}

/// `EndCityPieces.TOWER_BRIDGES`, in declaration order.
///
/// Read by `List.iterator()` and never shuffled, so the order is part of the
/// draw sequence: one `nextBoolean` per entry, in this order.
const TOWER_BRIDGES: [(Rotation, (i32, i32, i32)); 4] = [
    (Rotation::None, (1, -1, 0)),
    (Rotation::Clockwise90, (6, -1, 1)),
    (Rotation::Counterclockwise90, (0, -1, 5)),
    (Rotation::Clockwise180, (5, -1, 6)),
];

/// `EndCityPieces.FAT_TOWER_BRIDGES`, in declaration order.
const FAT_TOWER_BRIDGES: [(Rotation, (i32, i32, i32)); 4] = [
    (Rotation::None, (4, -1, 0)),
    (Rotation::Clockwise90, (12, -1, 4)),
    (Rotation::Counterclockwise90, (0, -1, 8)),
    (Rotation::Clockwise180, (8, -1, 12)),
];

/// Which of the four `SectionGenerator` implementations to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    /// `EndCityPieces$1`.
    HouseTower,
    /// `EndCityPieces$2`.
    Tower,
    /// `EndCityPieces$3` — the only stateful one.
    TowerBridge,
    /// `EndCityPieces$4`.
    FatTower,
}

/// The walk's mutable state.
///
/// `shipCreated` is a field on the shared `TOWER_BRIDGE_GENERATOR` object and
/// `startHouseTower` calls `init()` on it once, so it persists across the
/// whole city rather than per bridge. That is exactly why a city has **at
/// most one ship** — and, because the `nextInt(10 - depth)` sits behind
/// `!shipCreated` in a short-circuited `&&`, every later bridge **skips that
/// draw entirely**. Treating the flag as per-bridge would place several ships
/// *and* shift the stream for everything after the first.
struct Walk<'t, T: Templates + ?Sized> {
    templates: &'t mut T,
    ship_created: bool,
    /// Set when a template the walk asked for is not in the extract. The walk
    /// runs to completion so the caller sees one refusal rather than a panic,
    /// and the caller must discard the city.
    missing: Option<String>,
}

/// `StructureTemplate.getBoundingBox(settings, pos)` for a piece that has no
/// mirror and no rotation pivot — which is every End city piece, because
/// `makeSettings` sets only `ignoreEntities`, one processor and the rotation.
fn piece_bbox(template: &Template, pos: (i32, i32, i32), rotation: Rotation) -> BoundingBox {
    template.bounding_box(pos, rotation)
}

impl<T: Templates + ?Sized> Walk<'_, T> {
    /// `new EndCityPiece(...)` — the constructor, before any `move`.
    fn make(
        &mut self,
        name: &str,
        pos: (i32, i32, i32),
        rotation: Rotation,
        overwrite: bool,
    ) -> Option<Piece> {
        let Some(template) = self.templates.get(name) else {
            if self.missing.is_none() {
                self.missing = Some(name.to_string());
            }
            return None;
        };
        Some(Piece {
            template: name.to_string(),
            pos,
            rotation,
            overwrite,
            bbox: piece_bbox(&template, pos, rotation),
            gen_depth: 0,
        })
    }

    /// `EndCityPieces.addPiece`.
    ///
    /// The new piece is built **at the parent's `templatePosition`** and then
    /// moved by `calculateConnectedPosition(parentSettings, offset,
    /// ownSettings, BlockPos.ZERO)`.
    ///
    /// That call is `calculateRelativePosition(parent, offset) -
    /// calculateRelativePosition(own, ZERO)`, and `calculateRelativePosition`
    /// is `transform(pos, mirror, rotation, rotationPivot)`. Every End city
    /// piece leaves the pivot at `BlockPos.ZERO`, so the second term is
    /// always zero and the whole expression collapses to the offset turned by
    /// the **parent's** rotation. It is *not*
    /// `getZeroPositionWithTransform`, which is size-dependent and would put
    /// every rotated piece somewhere else.
    fn add_piece(
        &mut self,
        parent: &Piece,
        offset: (i32, i32, i32),
        name: &str,
        rotation: Rotation,
        overwrite: bool,
    ) -> Option<Piece> {
        let mut piece = self.make(name, parent.pos, rotation, overwrite)?;
        let connected = transform(offset, Mirror::None, parent.rotation, (0, 0));
        piece.pos = (
            piece.pos.0 + connected.0,
            piece.pos.1 + connected.1,
            piece.pos.2 + connected.2,
        );
        piece.bbox = BoundingBox::new(
            piece.bbox.min_x + connected.0,
            piece.bbox.min_y + connected.1,
            piece.bbox.min_z + connected.2,
            piece.bbox.max_x + connected.0,
            piece.bbox.max_y + connected.1,
            piece.bbox.max_z + connected.2,
        );
        Some(piece)
    }

    /// `EndCityPieces.recursiveChildren`.
    ///
    /// Returns vanilla's `boolean`: `false` means the sublist was discarded
    /// whole, either because the generator refused or because one of its
    /// pieces collided with an already-placed piece of a different gen depth.
    fn recursive_children<R: RandomSource + ?Sized>(
        &mut self,
        section: Section,
        depth: i32,
        piece: &Piece,
        pos: Option<(i32, i32, i32)>,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        // Not `>=`: depth 8 still generates. And this returns before any
        // draw, so an over-deep branch costs the stream nothing.
        if depth > MAX_GEN_DEPTH {
            return false;
        }
        let mut sub: Vec<Piece> = Vec::new();
        if !self.generate(section, depth, piece, pos, &mut sub, random) {
            return false;
        }
        // The stamp. Unbounded, and drawn once per successful generator call
        // — *after* everything that call recursed into.
        let stamp = random.next_i32();
        let mut bad = false;
        for q in &mut sub {
            q.gen_depth = stamp;
            // `findCollisionPiece` scans the **outer** accumulated list, not
            // the sublist, and returns the first box that intersects.
            let hit = out.iter().find(|p| p.bbox.intersects(q.bbox));
            if let Some(hit) = hit {
                if hit.gen_depth != piece.gen_depth {
                    bad = true;
                    break;
                }
            }
        }
        if bad {
            return false;
        }
        out.append(&mut sub);
        true
    }

    fn generate<R: RandomSource + ?Sized>(
        &mut self,
        section: Section,
        depth: i32,
        piece: &Piece,
        pos: Option<(i32, i32, i32)>,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        match section {
            Section::HouseTower => self.house_tower(depth, piece, pos, out, random),
            Section::Tower => self.tower(depth, piece, out, random),
            Section::TowerBridge => self.tower_bridge(depth, piece, out, random),
            Section::FatTower => self.fat_tower(depth, piece, out, random),
        }
    }

    /// `EndCityPieces$1` — the house tower. The only generator that reads its
    /// `pos` argument, and the only caller that passes one is `TOWER_BRIDGE`.
    fn house_tower<R: RandomSource + ?Sized>(
        &mut self,
        depth: i32,
        piece: &Piece,
        pos: Option<(i32, i32, i32)>,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        if depth > MAX_GEN_DEPTH {
            return false;
        }
        let rot = piece.rotation;
        let offset = pos.unwrap_or((0, 0, 0));
        let Some(p) = self.add_piece(piece, offset, "base_floor", rot, true) else {
            return false;
        };
        out.push(p);
        let mut p = out[out.len() - 1].clone();
        let choice = random.next_i32_bound(3);
        match choice {
            0 => {
                let Some(next) = self.add_piece(&p, (-1, 4, -1), "base_roof", rot, true) else {
                    return false;
                };
                out.push(next);
            }
            1 => {
                let Some(next) = self.add_piece(&p, (-1, 0, -1), "second_floor_2", rot, false)
                else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                let Some(next) = self.add_piece(&p, (-1, 8, -1), "second_roof", rot, false) else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                self.recursive_children(Section::Tower, depth + 1, &p, None, out, random);
            }
            _ => {
                let Some(next) = self.add_piece(&p, (-1, 0, -1), "second_floor_2", rot, false)
                else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                let Some(next) = self.add_piece(&p, (-1, 4, -1), "third_floor_2", rot, false)
                else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                let Some(next) = self.add_piece(&p, (-1, 8, -1), "third_roof", rot, true) else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                self.recursive_children(Section::Tower, depth + 1, &p, None, out, random);
            }
        }
        true
    }

    /// `EndCityPieces$2` — the plain tower.
    fn tower<R: RandomSource + ?Sized>(
        &mut self,
        depth: i32,
        piece: &Piece,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        let rot = piece.rotation;
        // The x-before-z trap: two separate statements, because they are two
        // separate draws and the order is fixed by Java's argument
        // evaluation, not by the geometry.
        let ox = 3 + random.next_i32_bound(2);
        let oz = 3 + random.next_i32_bound(2);
        let Some(next) = self.add_piece(piece, (ox, -3, oz), "tower_base", rot, true) else {
            return false;
        };
        out.push(next);
        let mut p = out[out.len() - 1].clone();
        let Some(next) = self.add_piece(&p, (0, 7, 0), "tower_piece", rot, true) else {
            return false;
        };
        out.push(next);
        p = out[out.len() - 1].clone();
        // Drawn unconditionally — the result decides whether this floor
        // anchors the bridges, and skipping the draw when it does not would
        // shift everything after it.
        let mut anchor = if random.next_i32_bound(3) == 0 {
            Some(p.clone())
        } else {
            None
        };
        let floors = 1 + random.next_i32_bound(3);
        for j in 0..floors {
            let Some(next) = self.add_piece(&p, (0, 4, 0), "tower_piece", rot, true) else {
                return false;
            };
            out.push(next);
            p = out[out.len() - 1].clone();
            // The short circuit: on the **last** floor the `nextBoolean` is
            // not reached at all.
            if j < floors - 1 && random.next_bool() {
                anchor = Some(p.clone());
            }
        }
        if let Some(anchor) = anchor {
            for (turn, offset) in TOWER_BRIDGES {
                if random.next_bool() {
                    let Some(next) =
                        self.add_piece(&anchor, offset, "bridge_end", rot.get_rotated(turn), true)
                    else {
                        return false;
                    };
                    out.push(next);
                    let child = out[out.len() - 1].clone();
                    self.recursive_children(
                        Section::TowerBridge,
                        depth + 1,
                        &child,
                        None,
                        out,
                        random,
                    );
                }
            }
            let Some(next) = self.add_piece(&p, (-1, 4, -1), "tower_top", rot, true) else {
                return false;
            };
            out.push(next);
            return true;
        }
        if depth == 7 {
            let Some(next) = self.add_piece(&p, (-1, 4, -1), "tower_top", rot, true) else {
                return false;
            };
            out.push(next);
            return true;
        }
        self.recursive_children(Section::FatTower, depth + 1, &p, None, out, random)
    }

    /// `EndCityPieces$3` — the bridge, and the only stateful generator.
    fn tower_bridge<R: RandomSource + ?Sized>(
        &mut self,
        depth: i32,
        piece: &Piece,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        let rot = piece.rotation;
        let spans = 1 + random.next_i32_bound(4);
        let Some(next) = self.add_piece(piece, (0, 0, -4), "bridge_piece", rot, true) else {
            return false;
        };
        out.push(next);
        let last = out.len() - 1;
        out[last].gen_depth = -1;
        let mut p = out[last].clone();
        let mut rise = 0;
        for _ in 0..spans {
            if random.next_bool() {
                let Some(next) = self.add_piece(&p, (0, rise, -4), "bridge_piece", rot, true)
                else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                rise = 0;
            } else {
                // The second draw only happens when the first came up false,
                // which is what makes the two stair kinds cost a different
                // number of draws from a flat span.
                let name = if random.next_bool() {
                    "bridge_steep_stairs"
                } else {
                    "bridge_gentle_stairs"
                };
                let offset = if name == "bridge_steep_stairs" {
                    (0, rise, -4)
                } else {
                    (0, rise, -8)
                };
                let Some(next) = self.add_piece(&p, offset, name, rot, true) else {
                    return false;
                };
                out.push(next);
                p = out[out.len() - 1].clone();
                rise = 4;
            }
        }
        // `!shipCreated && random.nextInt(10 - depth) == 0` — short
        // circuited, so once a ship exists this draw is never made again.
        if !self.ship_created && random.next_i32_bound(10 - depth) == 0 {
            let sx = -8 + random.next_i32_bound(8);
            let sz = -70 + random.next_i32_bound(10);
            let Some(next) = self.add_piece(&p, (sx, rise, sz), "ship", rot, true) else {
                return false;
            };
            // `p` is deliberately **not** reassigned: the closing bridge end
            // hangs off the last bridge piece, not off the ship.
            out.push(next);
            self.ship_created = true;
        } else if !self.recursive_children(
            Section::HouseTower,
            depth + 1,
            &p,
            Some((-3, rise + 1, -11)),
            out,
            random,
        ) {
            return false;
        }
        let Some(next) = self.add_piece(
            &p,
            (4, rise, 0),
            "bridge_end",
            rot.get_rotated(Rotation::Clockwise180),
            true,
        ) else {
            return false;
        };
        out.push(next);
        let last = out.len() - 1;
        out[last].gen_depth = -1;
        true
    }

    /// `EndCityPieces$4` — the fat tower.
    fn fat_tower<R: RandomSource + ?Sized>(
        &mut self,
        depth: i32,
        piece: &Piece,
        out: &mut Vec<Piece>,
        random: &mut R,
    ) -> bool {
        let rot = piece.rotation;
        let Some(next) = self.add_piece(piece, (-3, 4, -3), "fat_tower_base", rot, true) else {
            return false;
        };
        out.push(next);
        let mut p = out[out.len() - 1].clone();
        let Some(next) = self.add_piece(&p, (0, 4, 0), "fat_tower_middle", rot, true) else {
            return false;
        };
        out.push(next);
        p = out[out.len() - 1].clone();
        for _ in 0..2 {
            // The break test is at the **top** of the body, so it is drawn
            // before the piece it guards and a loop that runs zero times
            // still costs one draw.
            if random.next_i32_bound(3) == 0 {
                break;
            }
            let Some(next) = self.add_piece(&p, (0, 8, 0), "fat_tower_middle", rot, true) else {
                return false;
            };
            out.push(next);
            p = out[out.len() - 1].clone();
            for (turn, offset) in FAT_TOWER_BRIDGES {
                if random.next_bool() {
                    let Some(next) =
                        self.add_piece(&p, offset, "bridge_end", rot.get_rotated(turn), true)
                    else {
                        return false;
                    };
                    out.push(next);
                    let child = out[out.len() - 1].clone();
                    self.recursive_children(
                        Section::TowerBridge,
                        depth + 1,
                        &child,
                        None,
                        out,
                        random,
                    );
                }
            }
        }
        let Some(next) = self.add_piece(&p, (-2, 8, -2), "fat_tower_top", rot, true) else {
            return false;
        };
        out.push(next);
        true
    }
}

/// `EndCityPieces.startHouseTower` — the whole city.
///
/// `pos` is the `GenerationStub` position: `(chunkX*16 + 7, h, chunkZ*16 + 7)`
/// where `h` came from `getLowestYIn5by5BoxOffset7Blocks`. `rotation` is the
/// `Rotation.getRandom(random)` drawn **first**, before the height is taken.
///
/// Returns `Err` naming the first template the extract could not supply — a
/// city built without one of its twenty NBTs is not a shorter city, it is a
/// different draw sequence, so it is refused rather than truncated.
///
/// # Errors
/// If a template named by the walk is missing from the extract.
pub fn start_house_tower<T: Templates + ?Sized, R: RandomSource + ?Sized>(
    templates: &mut T,
    pos: (i32, i32, i32),
    rotation: Rotation,
    random: &mut R,
) -> Result<Vec<Piece>, String> {
    let mut walk = Walk {
        templates,
        // `TOWER_BRIDGE_GENERATOR.init()` — one of the four `init()` calls
        // `startHouseTower` opens with, and the only one that does anything.
        ship_created: false,
        missing: None,
    };
    let mut out: Vec<Piece> = Vec::new();

    let Some(base) = walk.make("base_floor", pos, rotation, true) else {
        return Err(walk.missing.unwrap_or_else(|| "base_floor".to_string()));
    };
    out.push(base);
    let mut p = out[0].clone();
    for (offset, name, overwrite) in [
        ((-1, 0, -1), "second_floor_1", false),
        ((-1, 4, -1), "third_floor_1", false),
        ((-1, 8, -1), "third_roof", true),
    ] {
        let Some(next) = walk.add_piece(&p, offset, name, rotation, overwrite) else {
            return Err(walk.missing.unwrap_or_else(|| name.to_string()));
        };
        out.push(next);
        p = out[out.len() - 1].clone();
    }
    // The result is discarded: a refused tower leaves the four-piece stub,
    // which is a legal — if bare — city.
    walk.recursive_children(Section::Tower, 1, &p, None, &mut out, random);
    if let Some(missing) = walk.missing {
        return Err(missing);
    }
    Ok(out)
}

/// `Structure.getLowestYIn5by5BoxOffset7Blocks` then the `y < 60` gate of
/// `EndCityStructure.findGenerationPoint`.
///
/// `height_at` is `ChunkGenerator.getFirstOccupiedHeight(x, z,
/// WORLD_SURFACE_WG, ...)`. The four samples are the corners of a 5x5 box
/// whose sign follows the rotation, and the result is their **minimum** —
/// `getLowestY`, not `getMeanCornerHeight`, which is the same four samples
/// averaged and is what several other structures use.
///
/// Returns the stub position, or `None` when the gate rejects the chunk.
pub fn generation_point<H: FnMut(i32, i32) -> Option<i32>>(
    chunk_x: i32,
    chunk_z: i32,
    rotation: Rotation,
    mut height_at: H,
) -> Option<(i32, i32, i32)> {
    let (dx, dz) = match rotation {
        Rotation::None => (5, 5),
        Rotation::Clockwise90 => (-5, 5),
        Rotation::Clockwise180 => (-5, -5),
        Rotation::Counterclockwise90 => (5, -5),
    };
    let bx = chunk_x * 16 + 7;
    let bz = chunk_z * 16 + 7;
    let h = [
        height_at(bx, bz)?,
        height_at(bx, bz + dz)?,
        height_at(bx + dx, bz)?,
        height_at(bx + dx, bz + dz)?,
    ]
    .into_iter()
    .min()?;
    if h < 60 {
        return None;
    }
    Some((bx, h, bz))
}
