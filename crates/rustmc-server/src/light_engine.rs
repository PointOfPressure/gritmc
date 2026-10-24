//! The light engine: what fills the section data [`crate::light`] classifies.
//!
//! # Scope, stated up front
//!
//! This computes **converged initial lighting** for a loaded region — what
//! `ThreadedLevelLightEngine.initializeLight` + `lightChunk` leave behind
//! before any player edit. It is not the incremental engine: there is no
//! decrease queue, because a decrease only arises from a mutation and the
//! chunk-send path observes the converged state.
//!
//! Converged light is order-independent, and that is measured rather than
//! assumed — `ci/light-golden.sh` runs vanilla's own engine twice and `cmp`s
//! the results, and they are identical.
//!
//! # The three rules that decide the wire bytes, all from bytecode
//!
//! **1. Which sections are stored.** `LayerLightSectionStorage.putSectionState`
//! initialises a section the moment its packed state stops being zero, and
//! that state is `hasData | neighborCount`. `updateSectionStatus` sets
//! `hasData` on the section itself and bumps `neighborCount` on all **26**
//! surrounding sections — `markSectionAndNeighborsAsAffected` is a
//! `-1..=1` cube in *section* space, so it crosses chunk borders.
//!
//! So a section is stored iff it, or any of the 26 sections around it, is a
//! non-air section. That is three chunks by three chunks by three sections,
//! and it is emphatically **not** per-column.
//!
//! This was measured, not inferred: `ci/light-golden.sh`'s `tower` case puts
//! a pillar to y = 120 in the centre chunk only, and the *diagonal* neighbour
//! (1, 1) — whose own only non-air section is its superflat surface — comes
//! back storing sections -5..8, the pillar's full extent plus padding. A
//! per-column rule predicts -5..-3 there and is refuted.
//!
//! **2. Light does not enter an unstored section.** `propagateIncrease` skips
//! a neighbour outright when `storingLightForSection` is false for its
//! section. The stored set is therefore a hard boundary on propagation and
//! not merely a question of what gets serialised.
//!
//! **3. Opacity is `max(1, getLightDampening())`** — `LightEngine.getOpacity`
//! — so even a fully transparent block costs one level per step.
//! `getLightDampeningInto` overrides that with 16 when the two occlusion
//! shapes merge to cover the shared face, but **only when at least one of the
//! two states has a non-empty occlusion shape**, and `isEmptyShape` is
//! `!canOcclude() || !useShapeForLightOcclusion()`.
//!
//! # What this does not do, measured rather than waved at
//!
//! The shape path **is** implemented, through the per-state
//! `lightOcclusionFaces` column `ci/block-props-golden.sh` dumps, and
//! [`shape_occludes`] documents exactly what that reduction is and is not.
//!
//! It had to be. A bottom slab has `lightDampening` **0** and an occlusion
//! shape that closes its DOWN face, so a dampening-only engine lights a room
//! under a slab roof that vanilla leaves dark — the divergence is not
//! marginal, it is the whole room. **6,762 of 32,366 block states (20.9%)**
//! are in that position: the slabs, stairs, shelves, snow, farmland,
//! dirt_path, pistons, sculk sensors, daylight detector, lectern, enchanting
//! table, stonecutter and end portal frame.
//!
//! What remains is narrower and is stated in [`shape_occludes`]: two partial
//! shapes neither of which closes the shared face alone but which together
//! do. [`Region::shape_occluding_states`] still counts the states in a real
//! region, so a caller can ask rather than assume.

// Ordered collections throughout, and not for the lint's sake.
//
// A light engine whose output depended on iteration order would be the worst
// kind of defect this project has: reproducible on the machine that produced
// it and not on the next one. `ci/light-golden.sh` runs vanilla twice and
// `cmp`s precisely because converged light SHOULD be order-independent, and
// the cheapest way to keep our side honest about that is to have no unordered
// iteration in it at all. `BTreeMap`/`BTreeSet` over tuple keys give a total
// order for free.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::light::{DataLayer, LightGeometry, LightLayer, SectionLightSource};

/// `LightEngine.MAX_LEVEL`.
pub const MAX_LEVEL: u8 = 15;

/// Per-state light inputs, as `ci/block-props-golden.sh` dumps them.
#[derive(Debug, Clone, Default)]
pub struct LightProps {
    /// `BlockState.getLightEmission()`, indexed by state id.
    pub emission: Vec<u8>,
    /// `BlockState.getLightDampening()` — 1.21's name for `getLightBlock`.
    pub dampening: Vec<u8>,
    /// `!LightEngine.isEmptyShape(state)`, i.e.
    /// `canOcclude() && useShapeForLightOcclusion()`.
    pub shape_occluding: Vec<bool>,
    /// Six bits per state, in `Direction.values()` order
    /// (DOWN, UP, NORTH, SOUTH, WEST, EAST): set when that face occludes on
    /// its own, i.e. `Shapes.faceShapeOccludes(getOcclusionShape(state, d),
    /// Shapes.empty())`.
    ///
    /// A bottom slab is `100000` — it stops light through its DOWN face and
    /// not its UP one — while its `lightDampening` is **0**. That pair is why
    /// a dampening-only engine lights a room under a slab roof that vanilla
    /// leaves dark, and why this column had to exist.
    pub occlusion_faces: Vec<u8>,
}

impl LightProps {
    /// `LightEngine.getOpacity` — `max(1, getLightDampening())`.
    ///
    /// The `max` is not a guard against bad data. A transparent block costs a
    /// level per step, which is the whole reason light has a range at all.
    #[must_use]
    pub fn opacity(&self, state: u16) -> u8 {
        self.dampening
            .get(state as usize)
            .copied()
            .unwrap_or(0)
            .max(1)
    }

    /// `BlockState.getLightEmission()`.
    #[must_use]
    pub fn emission(&self, state: u16) -> u8 {
        self.emission.get(state as usize).copied().unwrap_or(0)
    }

    /// `BlockState.getLightDampening()`, unclamped — this is the one
    /// `ChunkSkyLightSources.isEdgeOccluded` tests against zero, and clamping
    /// it to 1 there would make every block occlude the sky.
    #[must_use]
    pub fn dampening(&self, state: u16) -> u8 {
        self.dampening.get(state as usize).copied().unwrap_or(0)
    }

    /// Whether this state's occlusion shape is non-empty, i.e. whether the
    /// shape path in `getLightDampeningInto` is live for it.
    #[must_use]
    pub fn shape_occluding(&self, state: u16) -> bool {
        self.shape_occluding
            .get(state as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Whether face `dir` of this state occludes on its own.
    #[must_use]
    pub fn face_occludes(&self, state: u16, dir: usize) -> bool {
        self.occlusion_faces
            .get(state as usize)
            .copied()
            .unwrap_or(0)
            & (1 << dir)
            != 0
    }
}

/// `LightEngine.shapeOccludes(from, to, dir)`:
///
/// ```text
///   s1 = getOcclusionShape(from, dir)
///   s2 = getOcclusionShape(to, dir.getOpposite())
///   return Shapes.faceShapeOccludes(s1, s2)
/// ```
///
/// and `faceShapeOccludes(a, b)` is true when either shape is the full block,
/// false when both are empty, and otherwise asks whether their **union**
/// covers the shared face.
///
/// This reduces it to the per-state column: face `dir` of `from`, or the
/// opposite face of `to`, occluding on its own. `Direction.getOpposite()` is
/// `d ^ 1` for `Direction.values()`' pairing (DOWN/UP, NORTH/SOUTH,
/// WEST/EAST), which is why the index arithmetic is a xor and not a table.
///
/// # What this is not
///
/// It is exact whenever at most one of the two shapes is partial, which
/// covers every full-cube face and every empty one. The residual is the
/// union-of-two-partials case: two shapes neither of which covers the shared
/// face alone, but which together do — two stairs facing each other is the
/// realistic instance. Vanilla says occluded there and this says not.
///
/// The residual is stated rather than hidden because the alternative is a
/// voxel-shape join in the light engine, and the honest position is that this
/// closes the slab and stair *roofs* — which is what a light engine meets —
/// without claiming to be `Shapes.joinUnoptimized`.
pub(crate) fn shape_occludes(props: &LightProps, from: u16, to: u16, dir: usize) -> bool {
    shape_occludes_pair(props, from, dir, to, dir ^ 1)
}

/// One chunk's non-air sections. A section absent from the map is all air,
/// which is exactly `LevelChunkSection.hasOnlyAir()` and is the predicate
/// `initializeLight` tests.
#[derive(Debug, Clone, Default)]
pub struct LitChunk {
    /// section Y -> 4096 state ids in `y << 8 | z << 4 | x` order, the same
    /// index `DataLayer` uses.
    pub sections: BTreeMap<i32, Vec<u16>>,
}

impl LitChunk {
    #[must_use]
    pub fn state(&self, section_y: i32, x: u8, y: u8, z: u8) -> u16 {
        self.sections
            .get(&section_y)
            .map_or(0, |s| s[DataLayer::index(x, y, z)])
    }
}

/// The loaded region being lit. Chunks absent from the map are unloaded, and
/// the engine treats them as vanilla does: `getChunkForLighting` returns null
/// and nothing is stored or propagated for them.
#[derive(Debug, Clone)]
pub struct Region {
    pub geometry: LightGeometry,
    pub chunks: BTreeMap<(i32, i32), LitChunk>,
    /// State id of `minecraft:air`, so a column walk can tell air from a
    /// state whose id happens to be zero in some other table.
    pub air: u16,
}

impl Region {
    /// How many distinct block states in this region have a non-empty
    /// occlusion shape — the states this engine's dampening-only occlusion
    /// can be wrong about.
    ///
    /// A caller that wants to know whether a result is exact asks this rather
    /// than trusting it. Zero means the shape path was never reachable and
    /// the answer is vanilla's.
    #[must_use]
    pub fn shape_occluding_states(&self, props: &LightProps) -> usize {
        let mut seen = BTreeSet::new();
        for c in self.chunks.values() {
            for s in c.sections.values() {
                for &state in s {
                    if props.shape_occluding(state) {
                        seen.insert(state);
                    }
                }
            }
        }
        seen.len()
    }
}

/// The lit result: per chunk, per layer, the stored sections and their data.
#[derive(Debug, Clone, Default)]
pub struct LitRegion {
    layers: BTreeMap<(i32, i32, u8, i32), DataLayer>,
    stored: BTreeSet<(i32, i32, i32)>,
}

impl LitRegion {
    /// A [`SectionLightSource`] view of one chunk, which is what
    /// [`crate::light::classify`] consumes.
    #[must_use]
    pub fn chunk(&self, cx: i32, cz: i32) -> ChunkLight<'_> {
        ChunkLight {
            region: self,
            cx,
            cz,
        }
    }

    /// The sections stored for this chunk, in ascending section-Y order.
    #[must_use]
    pub fn stored_sections(&self, cx: i32, cz: i32) -> Vec<i32> {
        let mut v: Vec<i32> = self
            .stored
            .iter()
            .filter(|(x, z, _)| *x == cx && *z == cz)
            .map(|(_, _, y)| *y)
            .collect();
        v.sort_unstable();
        v
    }
}

/// One chunk's light, addressed the way `prepareSectionData` addresses it.
#[derive(Debug, Clone, Copy)]
pub struct ChunkLight<'a> {
    region: &'a LitRegion,
    cx: i32,
    cz: i32,
}

impl SectionLightSource for ChunkLight<'_> {
    fn data_layer(&self, layer: LightLayer, section_y: i32) -> Option<&DataLayer> {
        self.region
            .layers
            .get(&(self.cx, self.cz, layer as u8, section_y))
    }
}

/// A block position inside the region: chunk, section, and the nibble index.
type Pos = (i32, i32, i32, usize);

/// Light `region` to convergence.
///
/// # Panics
/// Never; the fixpoint is bounded by [`MAX_LEVEL`] decrements per position.
#[must_use]
pub fn light(region: &Region, props: &LightProps) -> LitRegion {
    let stored = stored_sections(region);
    let mut out = LitRegion {
        layers: BTreeMap::new(),
        stored: stored.clone(),
    };

    let sky = propagate_sky(region, props, &stored);
    let block = propagate_block(region, props, &stored);

    for &(cx, cz, sy) in &stored {
        for (layer, levels) in [(LightLayer::Sky, &sky), (LightLayer::Block, &block)] {
            // `createDataLayer` hands back a HOMOGENEOUS layer and
            // `getDataLayerToWrite` materialises it on the first write, so a
            // section nothing wrote a level into stays homogeneous and
            // reports `isEmpty()` — which is why an all-dark stored section
            // rides in the EMPTY mask rather than in the data mask with 2048
            // zero bytes.
            //
            // A uniform section is stored homogeneously here, and that is
            // worth being precise about because it is **not** vanilla's rule.
            // Vanilla's is allocation history: `SkyLightSectionStorage`
            // hands back `new DataLayer(15)` for a section above the data and
            // it stays that way until something writes. Ours is a property of
            // the values. The two agree on the wire for every case the golden
            // covers — `materialised()` of a `filled(v)` is the same 2048
            // bytes either way, and `is_empty()` agrees except in exactly the
            // written-then-darkened case, which is the `relit` divergence
            // already registered in `tests/light_golden.rs`.
            //
            // It is done this way rather than always materialising because
            // always materialising leaves `DataLayer::default_value`
            // unreachable from the engine — dead state behind a faithful
            // port. `ci/light-falsify.sh` case 6 caught exactly that: with
            // every layer materialised, breaking `isEmpty()` to ignore
            // `default_value` changed no output and the comparison stayed
            // green.
            let vals = levels.get(&(cx, cz, sy));
            let uniform = vals.and_then(|v| {
                let first = v[0];
                v.iter().all(|&x| x == first).then_some(first)
            });
            let d = match (vals, uniform) {
                (_, Some(v)) => DataLayer::filled(v),
                (None, _) => DataLayer::default(),
                (Some(vals), None) => {
                    let mut d = DataLayer::default();
                    for (i, &v) in vals.iter().enumerate() {
                        if v != 0 {
                            let data = d.data_mut();
                            let shift = 4 * (i & 1);
                            data[i >> 1] = (data[i >> 1] & !(15 << shift)) | (v << shift);
                        }
                    }
                    d
                }
            };
            out.layers.insert((cx, cz, layer as u8, sy), d);
        }
    }
    out
}

/// Rule 1: the 3x3x3 section neighbourhood, clipped to the light section
/// range.
fn stored_sections(region: &Region) -> BTreeSet<(i32, i32, i32)> {
    let lo = region.geometry.min_light_section();
    let hi = region.geometry.max_light_section();
    let mut has_data: BTreeSet<(i32, i32, i32)> = BTreeSet::new();
    for (&(cx, cz), c) in &region.chunks {
        for &sy in c.sections.keys() {
            has_data.insert((cx, cz, sy));
        }
    }
    let mut stored = BTreeSet::new();
    for &(cx, cz, sy) in &has_data {
        for dx in -1..=1 {
            for dz in -1..=1 {
                for dy in -1..=1 {
                    let (nx, nz, ny) = (cx + dx, cz + dz, sy + dy);
                    // A section is only stored for a chunk that is actually
                    // loaded — `getChunkForLighting` returning null is the
                    // end of it.
                    if region.chunks.contains_key(&(nx, nz)) && ny >= lo && ny < hi {
                        stored.insert((nx, nz, ny));
                    }
                }
            }
        }
    }
    stored
}

/// Every position of every stored section, as a flat map to a level array.
fn blank(stored: &BTreeSet<(i32, i32, i32)>) -> BTreeMap<(i32, i32, i32), Vec<u8>> {
    stored.iter().map(|&k| (k, vec![0u8; 4096])).collect()
}

fn state_at(region: &Region, cx: i32, cz: i32, sy: i32, idx: usize) -> u16 {
    region.chunks.get(&(cx, cz)).map_or(region.air, |c| {
        c.sections.get(&sy).map_or(region.air, |s| s[idx])
    })
}

/// `(x, y, z)` of a nibble index inside a section.
const fn unindex(i: usize) -> (u8, u8, u8) {
    ((i & 15) as u8, ((i >> 8) & 15) as u8, ((i >> 4) & 15) as u8)
}

/// The six `Direction.values()` offsets, as `(dx, dy, dz)`.
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (0, -1, 0),
    (0, 1, 0),
    (0, 0, -1),
    (0, 0, 1),
    (-1, 0, 0),
    (1, 0, 0),
];

/// Move one block from a position, returning `None` when the destination is
/// outside the stored set — rule 2.
fn step(
    stored: &BTreeSet<(i32, i32, i32)>,
    (cx, cz, sy, idx): Pos,
    (dx, dy, dz): (i32, i32, i32),
) -> Option<Pos> {
    let (x, y, z) = unindex(idx);
    let (mut nx, mut ny, mut nz) = (i32::from(x) + dx, i32::from(y) + dy, i32::from(z) + dz);
    let (mut ncx, mut ncz, mut nsy) = (cx, cz, sy);
    if nx < 0 {
        ncx -= 1;
        nx += 16;
    } else if nx > 15 {
        ncx += 1;
        nx -= 16;
    }
    if nz < 0 {
        ncz -= 1;
        nz += 16;
    } else if nz > 15 {
        ncz += 1;
        nz -= 16;
    }
    if ny < 0 {
        nsy -= 1;
        ny += 16;
    } else if ny > 15 {
        nsy += 1;
        ny -= 16;
    }
    if !stored.contains(&(ncx, ncz, nsy)) {
        return None;
    }
    #[allow(clippy::cast_sign_loss)]
    Some((
        ncx,
        ncz,
        nsy,
        DataLayer::index(nx as u8, ny as u8, nz as u8),
    ))
}

/// The max-plus fixpoint both layers share.
fn flood(
    region: &Region,
    props: &LightProps,
    stored: &BTreeSet<(i32, i32, i32)>,
    seeds: Vec<(Pos, u8)>,
) -> BTreeMap<(i32, i32, i32), Vec<u8>> {
    let mut levels = blank(stored);
    let mut queue: VecDeque<Pos> = VecDeque::new();
    for (p, v) in seeds {
        let (cx, cz, sy, idx) = p;
        if let Some(sec) = levels.get_mut(&(cx, cz, sy)) {
            if v > sec[idx] {
                sec[idx] = v;
                queue.push_back(p);
            }
        }
    }
    while let Some(p) = queue.pop_front() {
        let (cx, cz, sy, idx) = p;
        let here = levels[&(cx, cz, sy)][idx];
        if here <= 1 {
            continue;
        }
        let from_state = state_at(region, cx, cz, sy, idx);
        for (di, d) in DIRECTIONS.into_iter().enumerate() {
            let Some(n) = step(stored, p, d) else {
                continue;
            };
            let (ncx, ncz, nsy, nidx) = n;
            let state = state_at(region, ncx, ncz, nsy, nidx);
            // `propagateIncrease` drops the neighbour outright when the two
            // occlusion shapes close the shared face, whatever the opacity
            // arithmetic said. A bottom slab is the case that matters: its
            // `lightDampening` is 0, so without this light walks straight
            // through a slab roof.
            if shape_occludes(props, from_state, state, di) {
                continue;
            }
            let cost = props.opacity(state);
            let Some(candidate) = here.checked_sub(cost) else {
                continue;
            };
            if candidate == 0 {
                continue;
            }
            let sec = levels.get_mut(&(ncx, ncz, nsy)).expect("stored");
            if candidate > sec[nidx] {
                sec[nidx] = candidate;
                queue.push_back(n);
            }
        }
    }
    levels
}

/// `ChunkSkyLightSources.isEdgeOccluded(above, below)`:
///
/// ```text
///   if (below.getLightDampening() != 0) return true;
///   return faceShapeOccludes(getOcclusionShape(above, DOWN),
///                            getOcclusionShape(below, UP));
/// ```
///
/// Note the **unclamped** dampening: `getOpacity`'s `max(1, …)` must not be
/// used here, or every block would occlude the sky and no column would have a
/// source at all.
pub(crate) fn edge_occluded(props: &LightProps, above: u16, below: u16) -> bool {
    if props.dampening(below) != 0 {
        return true;
    }
    // DOWN is 0 and UP is 1 in `Direction.values()` order.
    shape_occludes_pair(props, above, 0, below, 1)
}

/// `Shapes.faceShapeOccludes(getOcclusionShape(a, da), getOcclusionShape(b, db))`
/// through the per-state face column. Same reduction, and same residual, as
/// [`shape_occludes`] — see its docs.
fn shape_occludes_pair(props: &LightProps, a: u16, da: usize, b: u16, db: usize) -> bool {
    if !props.shape_occluding(a) && !props.shape_occluding(b) {
        return false;
    }
    props.face_occludes(a, da) || props.face_occludes(b, db)
}

/// `ChunkSkyLightSources.findLowestSourceY` — the lowest y in a column that
/// still sees the sky. Public because [`crate::light_incremental`] builds a
/// per-chunk `ChunkSkyLightSources` table out of it rather than duplicating
/// the column walk.
/// still sees the sky straight down. Everything at or above it is a source.
pub fn lowest_source_y(region: &Region, props: &LightProps, cx: i32, cz: i32, x: u8, z: u8) -> i32 {
    let g = region.geometry;
    let min_y = g.min_section_y * 16;
    let Some(chunk) = region.chunks.get(&(cx, cz)) else {
        return min_y;
    };
    let Some(&top) = chunk.sections.keys().max() else {
        return min_y;
    };
    // Walk down from the top of the highest filled section.
    let mut y = (top + 1) * 16 - 1;
    let mut above = region.air;
    while y >= min_y {
        let sy = y.div_euclid(16);
        #[allow(clippy::cast_sign_loss)]
        let iy = y.rem_euclid(16) as u8;
        let state = chunk.state(sy, x, iy, z);
        if edge_occluded(props, above, state) {
            return y + 1;
        }
        above = state;
        y -= 1;
    }
    min_y
}

fn propagate_sky(
    region: &Region,
    props: &LightProps,
    stored: &BTreeSet<(i32, i32, i32)>,
) -> BTreeMap<(i32, i32, i32), Vec<u8>> {
    let mut seeds = Vec::new();
    for &(cx, cz) in region.chunks.keys() {
        for x in 0..16u8 {
            for z in 0..16u8 {
                let src = lowest_source_y(region, props, cx, cz, x, z);
                for &(sx, sz, sy) in stored {
                    if sx != cx || sz != cz {
                        continue;
                    }
                    for iy in 0..16u8 {
                        if sy * 16 + i32::from(iy) >= src {
                            seeds.push(((cx, cz, sy, DataLayer::index(x, iy, z)), MAX_LEVEL));
                        }
                    }
                }
            }
        }
    }
    flood(region, props, stored, seeds)
}

fn propagate_block(
    region: &Region,
    props: &LightProps,
    stored: &BTreeSet<(i32, i32, i32)>,
) -> BTreeMap<(i32, i32, i32), Vec<u8>> {
    let mut seeds = Vec::new();
    for (&(cx, cz), c) in &region.chunks {
        for (&sy, sec) in &c.sections {
            if !stored.contains(&(cx, cz, sy)) {
                continue;
            }
            for (idx, &state) in sec.iter().enumerate() {
                let e = props.emission(state);
                if e > 0 {
                    seeds.push(((cx, cz, sy, idx), e));
                }
            }
        }
    }
    flood(region, props, stored, seeds)
}

impl LightProps {
    /// Load the four light columns out of `block_props.json`, where each
    /// row's `id` is the global state id.
    ///
    /// # Errors
    /// If the file is missing or is a table generated before the light
    /// columns existed — which is reported rather than defaulted, because a
    /// table with no `lightEmission` would light a world in which nothing
    /// emits and nothing blocks, and that reads exactly like a port defect.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        let table = doc["table"]
            .as_array()
            .ok_or_else(|| std::io::Error::other(format!("{}: no table", path.display())))?;
        let n = table.len();
        let mut out = Self {
            emission: vec![0; n],
            dampening: vec![0; n],
            shape_occluding: vec![false; n],
            occlusion_faces: vec![0; n],
        };
        for row in table {
            let id = usize::try_from(
                row["id"]
                    .as_u64()
                    .ok_or_else(|| std::io::Error::other("a block_props row has no id"))?,
            )
            .map_err(std::io::Error::other)?;
            let emission = row["lightEmission"].as_u64().ok_or_else(|| {
                std::io::Error::other(format!(
                    "{}: no lightEmission column — this table predates the light \
                     port. Re-run ci/block-props-golden.sh.",
                    path.display()
                ))
            })?;
            let dampening = row["lightDampening"]
                .as_u64()
                .ok_or_else(|| std::io::Error::other("no lightDampening column"))?;
            if id >= n {
                return Err(std::io::Error::other(format!(
                    "state id {id} past the table"
                )));
            }
            out.emission[id] = u8::try_from(emission).map_err(std::io::Error::other)?;
            out.dampening[id] = u8::try_from(dampening).map_err(std::io::Error::other)?;
            out.shape_occluding[id] = row["shapeLightOcclusion"].as_bool().unwrap_or(false);
            // Six characters in `Direction.values()` order. Absent on a table
            // generated before the light columns, which `lightEmission` above
            // has already refused, so `unwrap_or(0)` here cannot silently
            // disable occlusion on a real table.
            if let Some(faces) = row["lightOcclusionFaces"].as_str() {
                let mut bits = 0u8;
                for (d, c) in faces.chars().take(6).enumerate() {
                    if c == '1' {
                        bits |= 1 << d;
                    }
                }
                out.occlusion_faces[id] = bits;
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::light::{classify, SectionLightSource};

    /// Two states: 0 is air (transparent, dark), 1 is stone (opaque), 2 is a
    /// lamp. Small on purpose — these tests are about the rules, and the real
    /// table is what `tests/light_golden.rs` uses.
    fn props() -> LightProps {
        LightProps {
            emission: vec![0, 0, 15],
            dampening: vec![0, 15, 0],
            shape_occluding: vec![false, false, false],
            occlusion_faces: vec![0, 0, 0],
        }
    }

    /// `(chunk position, [(section Y, the state filling it)])`.
    type Fixture<'a> = ((i32, i32), &'a [(i32, u16)]);

    fn region(chunks: &[Fixture<'_>]) -> Region {
        let mut map = BTreeMap::new();
        for &(pos, sections) in chunks {
            let mut c = LitChunk::default();
            for &(sy, fill) in sections {
                c.sections.insert(sy, vec![fill; 4096]);
            }
            map.insert(pos, c);
        }
        Region {
            geometry: LightGeometry::OVERWORLD,
            chunks: map,
            air: 0,
        }
    }

    #[test]
    fn stored_sections_are_a_cube_in_section_space_not_a_column() {
        // One chunk has content at section 3; its neighbour has none of its
        // own. The neighbour must still store 2, 3 and 4.
        //
        // This is the rule the golden's `tower` case measures against
        // vanilla; here it is pinned without needing the jar, so a refactor
        // that quietly makes it per-column fails in T0's unit tests as well
        // as in the comparison.
        let r = region(&[((0, 0), &[(3, 1)]), ((1, 0), &[])]);
        let lit = light(&r, &props());
        assert_eq!(lit.stored_sections(0, 0), vec![2, 3, 4]);
        assert_eq!(
            lit.stored_sections(1, 0),
            vec![2, 3, 4],
            "a chunk with no content of its own still stores its neighbour's \
             sections — markSectionAndNeighborsAsAffected is a cube"
        );
    }

    #[test]
    fn an_unloaded_chunk_stores_nothing() {
        // `getChunkForLighting` returning null is the end of it: the cube
        // does not conjure a chunk that is not loaded.
        let r = region(&[((0, 0), &[(3, 1)])]);
        let lit = light(&r, &props());
        assert!(lit.stored_sections(1, 0).is_empty());
    }

    #[test]
    fn a_uniform_section_is_stored_homogeneously() {
        // `DataLayer::default_value` is only reachable if the engine ever
        // produces a homogeneous non-zero layer. `ci/light-falsify.sh` case 6
        // exists to catch it not being: with every layer materialised,
        // breaking `isEmpty()` to ignore `default_value` changed no output
        // and the comparison stayed green.
        let r = region(&[((0, 0), &[(0, 2)])]); // a section of solid lamps
        let lit = light(&r, &props());
        let chunk = lit.chunk(0, 0);
        let d = chunk
            .data_layer(LightLayer::Block, 0)
            .expect("section 0 is stored");
        assert!(
            d.is_definitely_homogenous(),
            "a section whose every nibble is the same value must not be \
             materialised — vanilla's own createDataLayer hands back a \
             homogeneous layer and nothing writes to it"
        );
        assert!(d.is_definitely_filled_with(15));
        assert!(!d.is_empty(), "homogeneous 15 is not empty");
    }

    #[test]
    fn an_all_dark_stored_section_is_empty_not_zeroed_data() {
        // The other side of the same rule: nothing wrote a level, so the
        // layer stays homogeneous zero and rides in the EMPTY mask rather
        // than the data mask with 2048 zero bytes.
        let r = region(&[((0, 0), &[(0, 1)])]); // solid stone, no source
        let lit = light(&r, &props());
        let out = classify(LightGeometry::OVERWORLD, &lit.chunk(0, 0));
        assert!(out.block.is_empty(), "no arrays for a dark chunk");
        assert!(out.block_mask.is_empty());
        assert!(
            !out.empty_block_mask.is_empty(),
            "the stored dark sections must be in the empty mask"
        );
    }

    #[test]
    fn opacity_is_max_one_so_light_attenuates_through_air() {
        // `LightEngine.getOpacity` is `max(1, getLightDampening())`. Air
        // dampens 0; without the max, light would not fall off at all.
        let p = props();
        assert_eq!(p.opacity(0), 1, "air still costs one level per step");
        assert_eq!(p.opacity(1), 15);
        assert_eq!(p.dampening(0), 0, "and the unclamped value is still 0");
    }

    #[test]
    fn shape_occluding_states_are_countable_before_they_are_wrong() {
        // A caller asks whether a region contains a state this engine's
        // dampening-only occlusion can be wrong about, rather than finding
        // out from a wrong answer.
        let mut p = props();
        p.shape_occluding = vec![false, false, true];
        let r = region(&[((0, 0), &[(0, 1)])]);
        assert_eq!(r.shape_occluding_states(&p), 0);
        let r = region(&[((0, 0), &[(0, 2)])]);
        assert_eq!(r.shape_occluding_states(&p), 1);
    }
}
