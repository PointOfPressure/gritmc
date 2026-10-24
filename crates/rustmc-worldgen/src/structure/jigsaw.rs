//! `JigsawPlacement.addPieces` — the assembler every jigsaw structure uses.
//!
//! Reference (26.2 bytecode, behaviour ported): `JigsawPlacement`,
//! `JigsawPlacement$Placer`, `JigsawPlacement$PieceState`,
//! `StructureTemplatePool`, `SequencedPriorityIterator`, `JigsawBlock.canAttach`,
//! `Util.shuffle`.
//!
//! This is the spine for **six** of the structure sets the owner listed —
//! villages, pillager outposts, bastions, ancient cities, trail ruins and
//! trial chambers — so it is written as the general walk rather than as a
//! village.
//!
//! # What decides what
//!
//! A jigsaw structure is a breadth-first-ish graph walk. One start element is
//! drawn, and thereafter each placed piece offers its jigsaw blocks, each
//! jigsaw names a pool, and the walk tries that pool's elements — shuffled —
//! in every rotation — shuffled — until one of them both *attaches* and fits
//! in the free space. The first that does wins and the walk moves to the next
//! jigsaw of the parent.
//!
//! Everything about that is order-sensitive, and the order is set by four
//! shuffles per jigsaw block:
//!
//! ```text
//!   Rotation.getRandom(random)            once, for the start piece
//!   pool.getRandomTemplate(random)        once, for the start element
//!   element.getShuffledJigsawBlocks(..)   per piece: shuffle, then a stable
//!                                         sort by selection priority
//!   pool.getShuffledTemplates(random)     per jigsaw, TWICE — the pool and
//!                                         then its fallback
//!   Rotation.getShuffled(random)          per candidate element
//! ```
//!
//! `getShuffledTemplates` shuffles the **weight-expanded** list: a pool entry
//! of weight 150 appears 150 times. **Measured** rather than eyeballed:
//! `village/savanna/town_centers` is 8 entries and **459 slots**, so its start
//! draw is `nextInt(459)` and every shuffle of it costs 458 more;
//! `village/plains/town_centers` is 204 and `trial_chambers/hallway` is 1231.
//! Nothing about that is optional — the draw count is the stream position for
//! everything after it, and a cumulative-weight scan would reproduce the
//! *distribution* while spending one draw instead of n-1, which looks right
//! and is wrong.
//!
//! # The free space is integer boxes, not a `VoxelShape`
//!
//! Vanilla carries a `MutableObject<VoxelShape>` and tests candidates with
//! `Shapes.joinIsNotEmpty(free, Shapes.create(AABB.of(box).deflate(0.25)),
//! ONLY_SECOND)`. That is CSG over doubles, and reproducing it would be a
//! large amount of code for an answer that is always an integer one.
//!
//! It reduces exactly, and the reduction is worth writing down because it is
//! the sort of thing a port normally approximates:
//!
//!   * the shape only ever starts as **one** box — the big
//!     `max_distance_from_center` box for the root, or the parent piece's own
//!     box for a jigsaw pointing inward — and is only ever modified by
//!     subtracting placed boxes (`ONLY_FIRST`). So `free = outer \ holes`.
//!   * `AABB.of(BoundingBox)` spans `min .. max+1`, and deflating by 0.25
//!     shrinks each side by a quarter of a block. A deflated candidate meets a
//!     hole's AABB iff `x1 <= px2 && px1 <= x2` on every axis — which is
//!     `BoundingBox.intersects`, inclusive, exactly.
//!   * containment in `outer` likewise reduces to inclusive integer
//!     containment.
//!
//! So the test is `outer.contains(candidate) && !holes.any(intersects)`, with
//! no floating point anywhere. The 0.25 is what makes it exact: it is large
//! enough that two boxes sharing a face do not count as meeting, and small
//! enough that one block of real overlap does.

use std::sync::Arc;

use crate::rng::RandomSource;
use crate::structure::pieces::BoundingBox;
use crate::structure::template::{Facing, JigsawBlock, JigsawNbt, Rotation, Template};
use crate::structure::template_pool::Projection;

/// `Rotation.values()`, in declaration order — which is the order
/// `Util.shuffledCopy` is handed and therefore part of the draw.
pub const ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::Counterclockwise90,
];

/// `Util.shuffle(list, random)` — Fisher-Yates walking **down** from the end.
///
/// The direction is not a detail: walking up draws the same number of values
/// in a different order and produces a different permutation from the same
/// stream.
pub fn shuffle<T, R: RandomSource + ?Sized>(list: &mut [T], random: &mut R) {
    let mut i = list.len();
    while i > 1 {
        let j = random.next_i32_bound(i as i32) as usize;
        list.swap(j, i - 1);
        i -= 1;
    }
}

/// `Util.getRandom(values, random)` over `Rotation.values()`.
pub fn random_rotation<R: RandomSource + ?Sized>(random: &mut R) -> Rotation {
    ROTATIONS[random.next_i32_bound(4) as usize]
}

/// `Rotation.getShuffled(random)`.
pub fn shuffled_rotations<R: RandomSource + ?Sized>(random: &mut R) -> Vec<Rotation> {
    let mut out = ROTATIONS.to_vec();
    shuffle(&mut out, random);
    out
}

/// One entry of a pool, after the weight expansion.
///
/// `StructurePoolElement` has five subtypes and this port needs three of them.
/// `list_pool_element` and `legacy_single_pool_element` are folded in: the
/// legacy element differs from the single one only in which processors it adds
/// at *placement* time, not in anything the walk decides, and no pool this
/// project has reached uses a list element.
#[derive(Clone, Debug)]
pub enum ElementKind {
    /// `single_pool_element` and `legacy_single_pool_element`.
    Single {
        location: String,
        template: Arc<Template>,
        /// The element's processor list, unresolved — a registry name or the
        /// inline document. Kept whole rather than reduced to a name: 375 of
        /// the 1198 pool entries write their list inline, and dropping those
        /// would place a piece with the wrong processors while still looking
        /// like a correct assembly, because the walk never reads them.
        processors: Option<crate::structure::template_pool::Processors>,
        /// `legacy_single_pool_element`, which places with
        /// `BlockIgnoreProcessor.STRUCTURE_BLOCK` instead of
        /// `STRUCTURE_AND_AIR` — so its air *is* written.
        legacy: bool,
    },
    /// `feature_pool_element` — a configured feature, and for the walk a
    /// zero-sized element carrying exactly one synthetic jigsaw block.
    Feature { feature: String },
    /// `empty_pool_element`. Reaching it aborts the element list for the
    /// jigsaw being served, which is a `break` and not a `continue`.
    Empty,
    /// `list_pool_element` — several elements placed at one position and
    /// rotation as one piece. `ListPoolElement` answers the **first**
    /// element's jigsaws, the union of its non-empty elements' boxes, and
    /// places every element in order; its projection is pushed onto each
    /// child (`setProjectionOnEachElement`). The pillager outpost's tower is
    /// the one 26.2 pool this port reaches that uses it: a watchtower and its
    /// overgrown twin, the second under `outpost_rot`.
    List { elements: Vec<Element> },
}

#[derive(Clone, Debug)]
pub struct Element {
    pub kind: ElementKind,
    pub projection: Projection,
}

impl Element {
    /// `StructurePoolElement.getGroundLevelDelta` — 1 for every subtype.
    #[must_use]
    pub const fn ground_level_delta(&self) -> i32 {
        1
    }

    /// `getBoundingBox(stm, pos, rotation)`.
    ///
    /// A feature element's size is `Vec3i.ZERO`, so its box is the single
    /// block at its position — vanilla's `new BoundingBox(x, y, z, x + 0,
    /// y + 0, z + 0)`.
    #[must_use]
    pub fn bounding_box(&self, pos: (i32, i32, i32), rotation: Rotation) -> BoundingBox {
        match &self.kind {
            ElementKind::Single { template, .. } => template.bounding_box(pos, rotation),
            ElementKind::Feature { .. } | ElementKind::Empty => {
                BoundingBox::new(pos.0, pos.1, pos.2, pos.0, pos.1, pos.2)
            }
            ElementKind::List { elements } => {
                let boxes: Vec<BoundingBox> = elements
                    .iter()
                    .filter(|e| !matches!(e.kind, ElementKind::Empty))
                    .map(|e| e.bounding_box(pos, rotation))
                    .collect();
                BoundingBox::encapsulating(&boxes)
                    .unwrap_or(BoundingBox::new(pos.0, pos.1, pos.2, pos.0, pos.1, pos.2))
            }
        }
    }

    /// `getShuffledJigsawBlocks(stm, pos, rotation, random)`.
    ///
    /// `SinglePoolElement` shuffles the template's jigsaw list and then runs a
    /// **stable** sort by selection priority, highest first. Stability is
    /// load-bearing: with every priority equal — which is every village — the
    /// sort must leave the shuffle's order untouched, and an unstable sort
    /// would silently reorder it.
    ///
    /// `FeaturePoolElement` spends no draws at all and answers one synthetic
    /// jigsaw at its own position: front DOWN, top SOUTH, name
    /// `minecraft:bottom`, pool and target empty, joint rollable. It is a leaf
    /// by construction.
    pub fn shuffled_jigsaws<R: RandomSource + ?Sized>(
        &self,
        pos: (i32, i32, i32),
        rotation: Rotation,
        random: &mut R,
    ) -> Vec<JigsawBlock> {
        match &self.kind {
            ElementKind::Single { template, .. } => {
                let mut list = template.jigsaws(pos, rotation);
                shuffle(&mut list, random);
                // `List.sort` is TimSort, which is stable.
                list.sort_by_key(|j| std::cmp::Reverse(j.nbt.selection_priority));
                list
            }
            ElementKind::Feature { .. } => vec![JigsawBlock {
                pos,
                front: Facing::Down,
                top: Facing::South,
                nbt: JigsawNbt {
                    name: "minecraft:bottom".to_string(),
                    pool: "minecraft:empty".to_string(),
                    target: "minecraft:empty".to_string(),
                    rollable: true,
                    final_state: "minecraft:air".to_string(),
                    placement_priority: 0,
                    selection_priority: 0,
                },
                state: "minecraft:jigsaw[orientation=down_south]".to_string(),
            }],
            ElementKind::Empty => Vec::new(),
            ElementKind::List { elements } => elements
                .first()
                .map(|e| e.shuffled_jigsaws(pos, rotation, random))
                .unwrap_or_default(),
        }
    }

    /// The template name, for the piece list a region file would record.
    #[must_use]
    pub fn location(&self) -> Option<&str> {
        match &self.kind {
            ElementKind::Single { location, .. } => Some(location),
            _ => None,
        }
    }

    /// A name for **every** kind, for listings and diagnostics.
    ///
    /// `location()` answers `None` for a `feature_pool_element`, which is
    /// correct — vanilla writes no `location` for one — but it means a piece
    /// listing prints `-` for exactly the elements that are declared
    /// unported, so the declared gap cannot be checked against the blocks it
    /// is supposed to explain. This is deliberately **not** used by
    /// `golden_village_start`'s comparison, which must keep asking vanilla's
    /// own field.
    #[must_use]
    pub fn describe(&self) -> String {
        match &self.kind {
            ElementKind::Single {
                location, legacy, ..
            } => {
                if *legacy {
                    format!("legacy:{location}")
                } else {
                    location.clone()
                }
            }
            ElementKind::Feature { feature } => format!("feature:{feature}"),
            ElementKind::Empty => "<empty>".to_string(),
            ElementKind::List { elements } => format!(
                "list[{}]",
                elements
                    .iter()
                    .map(Element::describe)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
}

/// A pool, expanded the way `StructureTemplatePool`'s constructor expands it.
///
/// `templates` is `rawTemplates` with each element repeated `weight` times, and
/// **that** is the list both `getRandomTemplate` and `getShuffledTemplates`
/// work on. Expanding at parse time rather than sampling a weight is not an
/// optimisation, it is the semantics: the shuffle's length is the expanded
/// length, so the number of draws depends on the weights.
#[derive(Clone, Debug)]
pub struct Pool {
    pub name: String,
    pub fallback: String,
    pub elements: Vec<Arc<Element>>,
}

impl Pool {
    /// `getRandomTemplate` — one draw, or `EMPTY` for an empty pool.
    pub fn random_template<R: RandomSource + ?Sized>(
        &self,
        random: &mut R,
    ) -> Option<Arc<Element>> {
        if self.elements.is_empty() {
            return None;
        }
        let i = random.next_i32_bound(self.elements.len() as i32) as usize;
        Some(self.elements[i].clone())
    }

    /// `getShuffledTemplates` — a shuffled copy of the expanded list.
    pub fn shuffled_templates<R: RandomSource + ?Sized>(
        &self,
        random: &mut R,
    ) -> Vec<Arc<Element>> {
        let mut out = self.elements.clone();
        shuffle(&mut out, random);
        out
    }

    /// `getMaxSize(stm)` — the tallest non-empty element, unrotated.
    #[must_use]
    pub fn max_size(&self) -> i32 {
        self.elements
            .iter()
            .filter(|e| !matches!(e.kind, ElementKind::Empty))
            .map(|e| e.bounding_box((0, 0, 0), Rotation::None).y_span())
            .max()
            .unwrap_or(0)
    }
}

/// Everything the walk has to look up in a registry or a world.
pub trait Library {
    /// The pool by its registry name, already weight-expanded.
    ///
    /// `None` is vanilla's "Empty or non-existent pool" warning, which skips
    /// the jigsaw rather than failing the structure.
    fn pool(&mut self, name: &str) -> Option<Arc<Pool>>;

    /// `ChunkGenerator.getFirstFreeHeight(x, z, WORLD_SURFACE_WG, ..)`.
    ///
    /// Every terrain-matching piece is placed against this and nothing else,
    /// so a height that is one out puts a whole street one block out.
    fn first_free_height(&mut self, x: i32, z: i32) -> i32;
}

/// The free space, as the reduction in the module docs.
#[derive(Clone, Debug)]
struct Free {
    outer: BoundingBox,
    holes: Vec<BoundingBox>,
}

impl Free {
    fn admits(&self, candidate: BoundingBox) -> bool {
        let inside = candidate.min_x >= self.outer.min_x
            && candidate.min_y >= self.outer.min_y
            && candidate.min_z >= self.outer.min_z
            && candidate.max_x <= self.outer.max_x
            && candidate.max_y <= self.outer.max_y
            && candidate.max_z <= self.outer.max_z;
        inside && !self.holes.iter().any(|h| h.intersects(candidate))
    }
}

/// `JigsawJunction`, as saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Junction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
    pub delta_y: i32,
    pub dest_projection: Projection,
}

/// `PoolElementStructurePiece` — one placed piece.
#[derive(Clone, Debug)]
pub struct Piece {
    pub element: Arc<Element>,
    pub pos: (i32, i32, i32),
    pub rotation: Rotation,
    pub ground_level_delta: i32,
    pub bbox: BoundingBox,
    pub junctions: Vec<Junction>,
}

/// `JigsawStructure`'s configuration, as `addPieces` consumes it.
#[derive(Clone, Debug)]
pub struct Settings {
    pub start_pool: String,
    pub start_jigsaw_name: Option<String>,
    pub max_depth: i32,
    /// `(chunkMinX, startHeight, chunkMinZ)`.
    pub start_pos: (i32, i32, i32),
    pub use_expansion_hack: bool,
    /// `project_start_to_heightmap` — present for every village, absent for a
    /// bastion. Absent means the start keeps the y it was given.
    pub project_start_to_heightmap: bool,
    pub max_distance_horizontal: i32,
    pub max_distance_vertical: i32,
    /// `LevelHeightAccessor.getMinY()` and `getMaxY()`, inclusive.
    pub level_min_y: i32,
    pub level_max_y: i32,
    pub padding_bottom: i32,
    pub padding_top: i32,
}

/// One entry of the `SequencedPriorityIterator`.
struct PieceState {
    piece: usize,
    free: usize,
    depth: i32,
    priority: i32,
    /// Insertion order, so the FIFO within a priority is reproduced without a
    /// deque per priority.
    seq: usize,
}

/// `JigsawPlacement.addPieces`.
///
/// Returns the piece list in the order vanilla builds it, which is the order a
/// region file records and the order the pieces are written to the world in.
/// `None` is vanilla's `Optional.empty()`: an empty start pool, a missing
/// named start jigsaw, or a start piece that does not fit the dimension
/// padding.
///
/// # Errors
/// Never — a missing pool is skipped exactly as vanilla skips it.
#[allow(clippy::too_many_lines)]
pub fn add_pieces<L: Library + ?Sized, R: RandomSource + ?Sized>(
    random: &mut R,
    library: &mut L,
    settings: &Settings,
) -> Option<Vec<Piece>> {
    // Two draws before anything else, in this order. Swapping them is a
    // different village from the same seed.
    let rotation = random_rotation(random);
    let start_pool = library.pool(&settings.start_pool)?;
    let start_element = start_pool.random_template(random)?;
    if matches!(start_element.kind, ElementKind::Empty) {
        return None;
    }

    // `getRandomNamedJigsaw` — the ancient city's `minecraft:city_anchor`.
    // It shuffles the start element's jigsaws (spending the shuffle's draws)
    // and takes the first whose name matches; none matching is "no
    // structure". The piece is then built at `startPos - (anchor - startPos)`
    // so the anchor lands on the start position, and the stub — and the
    // free-space box — sit at the projected y plus that offset's y.
    let offset = if let Some(name) = &settings.start_jigsaw_name {
        let jigsaws = start_element.shuffled_jigsaws(settings.start_pos, rotation, random);
        let anchor = jigsaws.iter().find(|j| j.nbt.name == *name)?.pos;
        (
            anchor.0 - settings.start_pos.0,
            anchor.1 - settings.start_pos.1,
            anchor.2 - settings.start_pos.2,
        )
    } else {
        (0, 0, 0)
    };
    let start = (
        settings.start_pos.0 - offset.0,
        settings.start_pos.1 - offset.1,
        settings.start_pos.2 - offset.2,
    );
    let mut pieces = vec![Piece {
        element: start_element.clone(),
        pos: start,
        rotation,
        ground_level_delta: start_element.ground_level_delta(),
        bbox: start_element.bounding_box(start, rotation),
        junctions: Vec::new(),
    }];
    let bbox = pieces[0].bbox;
    let center_x = (bbox.max_x + bbox.min_x) / 2;
    let center_z = (bbox.max_z + bbox.min_z) / 2;
    let projected = if settings.project_start_to_heightmap {
        settings.start_pos.1 + library.first_free_height(center_x, center_z)
    } else {
        start.1
    };
    // `piece.move(0, projected - (bbox.minY + groundLevelDelta), 0)`.
    let dy = projected - (bbox.min_y + pieces[0].ground_level_delta);
    pieces[0].pos.1 += dy;
    pieces[0].bbox.shift(0, dy, 0);
    let start_box = pieces[0].bbox;
    if start_box.min_y < settings.level_min_y + settings.padding_bottom
        || start_box.max_y > settings.level_max_y - settings.padding_top
    {
        // `isStartTooCloseToWorldHeightLimits`.
        return None;
    }
    if settings.max_depth <= 0 {
        return Some(pieces);
    }
    let center_y = projected + offset.1;

    // The root free space: the max-distance box minus the start piece.
    let horizontal = settings.max_distance_horizontal;
    let vertical = settings.max_distance_vertical;
    let outer = BoundingBox::new(
        center_x - horizontal,
        (center_y - vertical).max(settings.level_min_y + settings.padding_bottom),
        center_z - horizontal,
        center_x + horizontal,
        (center_y + vertical).min(settings.level_max_y - settings.padding_top),
        center_z + horizontal,
    );
    let mut frees = vec![Free {
        outer,
        holes: vec![start_box],
    }];

    let mut queue: Vec<PieceState> = Vec::new();
    let mut seq = 0usize;
    let mut current = PieceState {
        piece: 0,
        free: 0,
        depth: 0,
        priority: 0,
        seq,
    };
    loop {
        try_placing_children(
            random,
            library,
            settings,
            &mut pieces,
            &mut frees,
            &mut queue,
            &mut seq,
            current.piece,
            current.free,
            current.depth,
        );
        // `SequencedPriorityIterator.next` — highest priority first, FIFO
        // within a priority.
        let Some(best) = queue
            .iter()
            .enumerate()
            .max_by_key(|(_, s)| (s.priority, std::cmp::Reverse(s.seq)))
            .map(|(i, _)| i)
        else {
            break;
        };
        current = queue.remove(best);
    }
    Some(pieces)
}

/// `Placer.tryPlacingChildren`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn try_placing_children<L: Library + ?Sized, R: RandomSource + ?Sized>(
    random: &mut R,
    library: &mut L,
    settings: &Settings,
    pieces: &mut Vec<Piece>,
    frees: &mut Vec<Free>,
    queue: &mut Vec<PieceState>,
    seq: &mut usize,
    piece_index: usize,
    free_index: usize,
    depth: i32,
) {
    let parent = pieces[piece_index].clone();
    let parent_rigid = parent.element.projection == Projection::Rigid;
    let parent_box = parent.bbox;
    let parent_min_y = parent_box.min_y;
    // The per-piece free space, created on first use by a jigsaw whose target
    // sits inside the parent's own box. Children of such a jigsaw are
    // confined to the parent, which is how an interior room is filled.
    let mut inner_free: Option<usize> = None;

    let jigsaws = parent
        .element
        .shuffled_jigsaws(parent.pos, parent.rotation, random);
    for jigsaw in jigsaws {
        let front = jigsaw.front;
        let jigsaw_pos = jigsaw.pos;
        let step = front.step();
        let target = (
            jigsaw_pos.0 + step.0,
            jigsaw_pos.1 + step.1,
            jigsaw_pos.2 + step.2,
        );
        let above_floor = jigsaw_pos.1 - parent_min_y;
        // `int24` — the heightmap answer, sampled at most once per jigsaw and
        // reused. Vanilla caches it in a local seeded to MIN_VALUE, and the
        // cache is observable: two candidates for one jigsaw ask the same
        // column and must get the same answer.
        let mut height: Option<i32> = None;

        let Some(pool) = library.pool(&jigsaw.nbt.pool) else {
            continue;
        };

        if pool.elements.is_empty() && jigsaw.nbt.pool != "minecraft:empty" {
            continue;
        }
        let Some(fallback) = library.pool(&pool.fallback) else {
            continue;
        };
        if fallback.elements.is_empty() && pool.fallback != "minecraft:empty" {
            continue;
        }

        let inside = parent_box.min_x <= target.0
            && target.0 <= parent_box.max_x
            && parent_box.min_y <= target.1
            && target.1 <= parent_box.max_y
            && parent_box.min_z <= target.2
            && target.2 <= parent_box.max_z;
        let free_slot = if inside {
            *inner_free.get_or_insert_with(|| {
                frees.push(Free {
                    outer: parent_box,
                    holes: Vec::new(),
                });
                frees.len() - 1
            })
        } else {
            free_index
        };

        let mut candidates: Vec<Arc<Element>> = Vec::new();
        if depth != settings.max_depth {
            candidates.extend(pool.shuffled_templates(random));
        }
        candidates.extend(fallback.shuffled_templates(random));
        let priority = jigsaw.nbt.placement_priority;

        // `'jigsaw` is vanilla's `goto 1215`, which is the *next parent
        // jigsaw*. Both the empty element and a successful placement jump
        // there, and neither of them is a `continue` on the element list —
        // reading them as one would keep trying elements after a piece had
        // already been placed.
        'jigsaw: for element in candidates {
            if matches!(element.kind, ElementKind::Empty) {
                break 'jigsaw;
            }
            for child_rotation in shuffled_rotations(random) {
                let child_jigsaws = element.shuffled_jigsaws((0, 0, 0), child_rotation, random);
                let child_box_at_origin = element.bounding_box((0, 0, 0), child_rotation);
                // The expansion hack's height, computed once per rotation and
                // over the *unplaced* child.
                let hack = if settings.use_expansion_hack && child_box_at_origin.y_span() <= 16 {
                    child_jigsaws
                        .iter()
                        .map(|j| expansion_height(library, j, child_box_at_origin))
                        .max()
                        .unwrap_or(0)
                } else {
                    0
                };
                for child_jigsaw in &child_jigsaws {
                    if !jigsaw.can_attach(child_jigsaw) {
                        continue;
                    }
                    let child_pos = child_jigsaw.pos;
                    let offset = (
                        target.0 - child_pos.0,
                        target.1 - child_pos.1,
                        target.2 - child_pos.2,
                    );
                    let raw = element.bounding_box(offset, child_rotation);
                    let child_rigid = element.projection == Projection::Rigid;
                    let child_y = child_pos.1;
                    let delta = above_floor - child_y + step.1;
                    let target_y = if parent_rigid && child_rigid {
                        parent_min_y + delta
                    } else {
                        let h = *height.get_or_insert_with(|| {
                            library.first_free_height(jigsaw_pos.0, jigsaw_pos.2)
                        });
                        h - child_y
                    };
                    let lift = target_y - raw.min_y;
                    let mut placed_box = raw;
                    placed_box.shift(0, lift, 0);
                    let placed_pos = (offset.0, offset.1 + lift, offset.2);
                    if hack > 0 {
                        let grow = (hack + 1).max(placed_box.max_y - placed_box.min_y);
                        placed_box.max_y = placed_box.max_y.max(placed_box.min_y + grow);
                    }
                    if !frees[free_slot].admits(placed_box) {
                        continue;
                    }
                    frees[free_slot].holes.push(placed_box);

                    let parent_delta = parent.ground_level_delta;
                    let child_delta = if child_rigid {
                        parent_delta - delta
                    } else {
                        element.ground_level_delta()
                    };
                    // The junction's ground y, which is the third of the three
                    // ways this walk resolves a height and the only one that
                    // halves the step.
                    let ground_y = if parent_rigid {
                        parent_min_y + above_floor
                    } else if child_rigid {
                        target_y + child_y
                    } else {
                        let h = *height.get_or_insert_with(|| {
                            library.first_free_height(jigsaw_pos.0, jigsaw_pos.2)
                        });
                        h + delta / 2
                    };
                    pieces[piece_index].junctions.push(Junction {
                        source_x: target.0,
                        source_ground_y: ground_y - above_floor + parent_delta,
                        source_z: target.2,
                        delta_y: delta,
                        dest_projection: element.projection,
                    });
                    let child = Piece {
                        element: element.clone(),
                        pos: placed_pos,
                        rotation: child_rotation,
                        ground_level_delta: child_delta,
                        bbox: placed_box,
                        junctions: vec![Junction {
                            source_x: jigsaw_pos.0,
                            source_ground_y: ground_y - child_y + child_delta,
                            source_z: jigsaw_pos.2,
                            delta_y: -delta,
                            dest_projection: parent.element.projection,
                        }],
                    };
                    pieces.push(child);
                    // Vanilla writes `depth + 1 <= maxDepth`; clippy insists on
                    // the equivalent `depth < maxDepth`, which is the same set.
                    if depth < settings.max_depth {
                        *seq += 1;
                        queue.push(PieceState {
                            piece: pieces.len() - 1,
                            free: free_slot,
                            depth: depth + 1,
                            priority,
                            seq: *seq,
                        });
                    }
                    break 'jigsaw;
                }
            }
        }
    }
}

/// `Placer.lambda$tryPlacingChildren$1` — how far the expansion hack may grow
/// a piece for one of its jigsaw blocks.
///
/// Zero unless the jigsaw points **into** the candidate's own box; otherwise
/// the taller of the target pool's and its fallback's `getMaxSize`.
fn expansion_height<L: Library + ?Sized>(
    library: &mut L,
    jigsaw: &JigsawBlock,
    box_at_origin: BoundingBox,
) -> i32 {
    let step = jigsaw.front.step();
    let target = (
        jigsaw.pos.0 + step.0,
        jigsaw.pos.1 + step.1,
        jigsaw.pos.2 + step.2,
    );
    let inside = box_at_origin.min_x <= target.0
        && target.0 <= box_at_origin.max_x
        && box_at_origin.min_y <= target.1
        && target.1 <= box_at_origin.max_y
        && box_at_origin.min_z <= target.2
        && target.2 <= box_at_origin.max_z;
    if !inside {
        return 0;
    }
    let Some(pool) = library.pool(&jigsaw.nbt.pool) else {
        return 0;
    };
    let own = pool.max_size();
    let fallback = library.pool(&pool.fallback).map_or(0, |p| p.max_size());
    own.max(fallback)
}
