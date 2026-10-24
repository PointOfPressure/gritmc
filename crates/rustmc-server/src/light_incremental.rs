//! The incremental light engine — `LightEngine`, `BlockLightEngine` and
//! `SkyLightEngine` as vanilla actually runs them, queues and all.
//!
//! # Why this exists beside [`crate::light_engine`]
//!
//! [`crate::light_engine::light`] computes **converged initial lighting** as a
//! max-plus fixpoint. That is the right shape for a chunk that has just been
//! generated and it is graded against vanilla's own engine by
//! `ci/light-golden.sh`. It cannot do the other half of the job, and the
//! reason is structural rather than a matter of effort: a fixpoint over
//! `max` has no way to *lower* a value. Remove the only torch in a room and
//! the fixpoint's answer is unchanged, because nothing it computes ever
//! shrinks.
//!
//! Vanilla's answer to that is the decrease queue, and the decrease queue is
//! the whole of this file's reason to exist. A block change enqueues the
//! position through `checkBlock`; `runLightUpdates` drains the check set into
//! `checkNode`, which zeroes the position and enqueues a *decrease* carrying
//! the level that was there; `propagateDecrease` walks outward zeroing
//! everything that could only have been lit by what was removed, and
//! re-queues as *increases* the boundary positions that were lit by
//! something else. Then `propagateIncreases` refills.
//!
//! # The order, from `LightEngine.runLightUpdates`
//!
//! ```text
//!   for (long pos : blockNodesToCheck) checkNode(pos);
//!   blockNodesToCheck.clear(); blockNodesToCheck.trim(512);
//!   int n = 0;
//!   n += propagateDecreases();
//!   n += propagateIncreases();
//!   clearChunkCache();
//!   storage.markNewInconsistencies(this);
//!   storage.swapSectionMap();
//!   return n;
//! ```
//!
//! **Decreases run before increases, and both run to exhaustion.** That is
//! not an optimisation: a decrease that ran after an increase would erase
//! light the increase had just placed, and the two queues are drained in
//! separate passes rather than interleaved so that every zeroing is done
//! before any refilling starts.
//!
//! # The one place we are deliberately not vanilla, stated rather than hidden
//!
//! `blockNodesToCheck` is a `LongOpenHashSet` and vanilla iterates it in
//! **hash order**. Ours is a [`BTreeSet`] and iterates in position order. The
//! justification is not taste: `ci/light-golden.sh` runs vanilla's engine
//! twice over the same world and `cmp`s the output, and the results are
//! identical — so the converged answer does not depend on that order, and an
//! ordered set turns a result that merely happens to be reproducible into one
//! that cannot be otherwise. If a future case ever shows vanilla's answer
//! depending on check order, this comment is where to start, and
//! [`Engine::check_order_is_total`] is the property that would have to give.
//!
//! Everything else — the entry packing, the queue disciplines, the skip
//! directions, the emission flag, the empty-shape flag, the sky column
//! shortcut — is transcribed from the bytecode.
//!
//! # Where `runLightUpdates` is called from, and why one drain per batch
//!
//! `ThreadedLevelLightEngine` does not call `runLightUpdates` per edit. It
//! queues `(TaskType, Runnable)` pairs and `runUpdate()` drains them a batch
//! at a time — `DEFAULT_BATCH_SIZE = 1000`, and `taskPerBatch` is that
//! constant:
//!
//! ```text
//!   n = min(lightTasks.size(), 1000);
//!   for (i = 0; it.hasNext() && i < n; i++) if (first == PRE_UPDATE)  run();
//!   it.back(i);
//!   super.runLightUpdates();
//!   for (i = 0; it.hasNext() && i < n; i++) { if (first == POST_UPDATE) run();
//!                                             it.remove(); }
//! ```
//!
//! `checkBlock`, `updateSectionStatus`, `setLightEnabled` and
//! `propagateLightSources` are all `PRE_UPDATE`. So **every edit in a batch
//! is fed to the engine before a single drain runs**, and that is why
//! [`light_region_with_edits`] calls `check_block` for all of its edits and
//! then drains once, rather than draining per edit. It is not a shortcut: a
//! drain per edit would give a different intermediate state to every later
//! `checkNode`, and the `source_moved` golden case — one source removed and
//! another added in the same batch — is the case that can tell the two
//! apart.
//!
//! The threading itself is deliberately absent. The batching above is the
//! part that can change results; a `ConsecutiveExecutor` and a
//! `ChunkTaskDispatcher` cannot, because everything they schedule ends up
//! running on one thread in queue order anyway.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::light::{DataLayer, LightGeometry, LightLayer};
use crate::light_engine::{LightProps, Region, MAX_LEVEL};
use crate::light_queue::{Dir, QueueEntry, PROPAGATION_DIRECTIONS, PULL_LIGHT_IN_ENTRY};

/// A section position: `(chunk x, section y, chunk z)`.
pub type Sec = (i32, i32, i32);

/// A block position, packed exactly as `BlockPos.asLong`.
///
/// `PACKED_HORIZONTAL_LENGTH` is `1 + log2(smallestEncompassingPowerOfTwo(
/// 30000000))` = 26 and `PACKED_Y_LENGTH` is `64 - 2 * 26` = 12, so the
/// layout is X in bits 38..63, Z in 12..37 and Y in 0..11. Read off the
/// static initialiser rather than remembered.
pub type Pos = i64;

const PACKED_HORIZONTAL_LENGTH: u32 = 26;
const PACKED_Y_LENGTH: u32 = 12;
const X_OFFSET: u32 = PACKED_Y_LENGTH + PACKED_HORIZONTAL_LENGTH;
const Z_OFFSET: u32 = PACKED_Y_LENGTH;

/// `BlockPos.asLong(x, y, z)`.
#[must_use]
pub const fn as_long(x: i32, y: i32, z: i32) -> Pos {
    let xm = (1i64 << PACKED_HORIZONTAL_LENGTH) - 1;
    let ym = (1i64 << PACKED_Y_LENGTH) - 1;
    ((x as i64 & xm) << X_OFFSET) | (y as i64 & ym) | ((z as i64 & xm) << Z_OFFSET)
}

/// `BlockPos.getX(long)` — the sign is recovered by shifting left then
/// arithmetic-shifting back, which is what the bytecode does.
#[must_use]
pub const fn pos_x(p: Pos) -> i32 {
    (p << (64 - X_OFFSET - PACKED_HORIZONTAL_LENGTH) >> (64 - PACKED_HORIZONTAL_LENGTH)) as i32
}

/// `BlockPos.getY(long)`.
#[must_use]
pub const fn pos_y(p: Pos) -> i32 {
    (p << (64 - PACKED_Y_LENGTH) >> (64 - PACKED_Y_LENGTH)) as i32
}

/// `BlockPos.getZ(long)`.
#[must_use]
pub const fn pos_z(p: Pos) -> i32 {
    (p << (64 - Z_OFFSET - PACKED_HORIZONTAL_LENGTH) >> (64 - PACKED_HORIZONTAL_LENGTH)) as i32
}

/// `BlockPos.offset(long, Direction)`.
#[must_use]
pub const fn offset(p: Pos, d: Dir) -> Pos {
    let (dx, dy, dz) = d.offset();
    as_long(pos_x(p) + dx, pos_y(p) + dy, pos_z(p) + dz)
}

/// `SectionPos.blockToSectionCoord(int)` — an arithmetic shift, so it floors
/// for negative coordinates. `/ 16` would not.
#[must_use]
pub const fn block_to_section_coord(v: i32) -> i32 {
    v >> 4
}

/// `SectionPos.sectionRelative(int)`.
#[must_use]
pub const fn section_relative(v: i32) -> i32 {
    v & 15
}

/// `SectionPos.blockToSection(long)`.
#[must_use]
pub const fn block_to_section(p: Pos) -> Sec {
    (
        block_to_section_coord(pos_x(p)),
        block_to_section_coord(pos_y(p)),
        block_to_section_coord(pos_z(p)),
    )
}

/// The nibble index inside a section for a block position.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub const fn nibble_index(p: Pos) -> usize {
    DataLayer::index(
        section_relative(pos_x(p)) as u8,
        section_relative(pos_y(p)) as u8,
        section_relative(pos_z(p)) as u8,
    )
}

/// `LayerLightSectionStorage.SectionState`.
///
/// One byte: bit 5 (`HAS_DATA_BIT` = 32) says the section itself is non-air,
/// bits 0..4 (`NEIGHBOR_COUNT_BITS` = 31) count how many of the 26 sections
/// around it are. `EMPTY` is 0, and **a section is stored for lighting
/// exactly when its state is not 0** — `putSectionState` creates the data
/// layer on the transition away from zero and removes it on the transition
/// back.
mod section_state {
    pub const EMPTY: u8 = 0;
    pub const HAS_DATA_BIT: u8 = 32;
    pub const NEIGHBOR_COUNT_BITS: u8 = 31;

    #[must_use]
    pub const fn has_data(state: u8, value: bool) -> u8 {
        if value {
            state | HAS_DATA_BIT
        } else {
            state & !HAS_DATA_BIT
        }
    }

    #[must_use]
    pub const fn neighbor_count(state: u8) -> i32 {
        (state & NEIGHBOR_COUNT_BITS) as i32
    }

    /// Panics outside `[0, 26]`, exactly as vanilla's
    /// `IllegalArgumentException` does — a count that walked out of range is
    /// a bookkeeping defect and must not be silently clamped.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn with_neighbor_count(state: u8, count: i32) -> u8 {
        assert!(
            (0..=26).contains(&count),
            "Neighbor count was not within range [0; 26]"
        );
        (state & !NEIGHBOR_COUNT_BITS) | (count as u8 & NEIGHBOR_COUNT_BITS)
    }
}

/// `ChunkSkyLightSources` for one chunk: the lowest y in each of the 256
/// columns that still sees the sky straight down.
#[derive(Debug, Clone)]
pub struct SkySources {
    /// Indexed `z * 16 + x`, as `ChunkSkyLightSources` indexes its own array.
    lowest: [i32; 256],
}

impl SkySources {
    /// `ChunkSkyLightSources.getLowestSourceY(x, z)`.
    #[must_use]
    #[allow(clippy::cast_sign_loss)]
    pub const fn lowest_source_y(&self, x: i32, z: i32) -> i32 {
        self.lowest[(z * 16 + x) as usize]
    }

    /// `ChunkSkyLightSources.getHighestLowestSourceY()`.
    #[must_use]
    pub fn highest_lowest_source_y(&self) -> i32 {
        let mut m = i32::MIN;
        for v in self.lowest {
            if v > m {
                m = v;
            }
        }
        m
    }
}

/// `LayerLightSectionStorage` plus the two `SkyLightSectionStorage` fields
/// that decide where sky light starts.
#[derive(Debug, Clone)]
pub struct LightStorage {
    layer: LightLayer,
    geometry: LightGeometry,
    /// `sectionStates`. An absent key is `SectionState.EMPTY`.
    section_states: BTreeMap<Sec, u8>,
    /// `updatingSectionData`. A section has an entry exactly while its state
    /// is non-empty, which is what makes `storingLightForSection` a lookup.
    levels: BTreeMap<Sec, Vec<u8>>,
    /// `columnsWithSources` — the chunks `setLightEnabled(_, true)` was
    /// called for. `lightOnInSection` reads it.
    columns_with_sources: BTreeSet<(i32, i32)>,
    /// The sections whose data layer has been **written** through
    /// `DataLayer.set`, as opposed to merely created.
    ///
    /// This is not bookkeeping, it is on the wire. Vanilla's `DataLayer`
    /// carries `byte[] data` and an `int defaultValue`, `createDataLayer`
    /// hands back one with `data == null`, and `getDataLayerToWrite`
    /// materialises the array on the first `set`. `isEmpty()` is
    /// `data == null && defaultValue == 0` — it reads the *representation*.
    ///
    /// So a section that was lit and then darkened again reports NOT empty
    /// and rides in the data mask carrying 2048 zero bytes, while the
    /// byte-identical world reached without the edit rides in the empty mask
    /// carrying nothing. That is the `relit` divergence, and it is the
    /// difference between 459/468 and 468/468 on that case: reconstructing
    /// emptiness from the *values* gets it wrong by exactly the sections an
    /// edit darkened.
    materialised: BTreeSet<Sec>,
    /// `sectionsAffectedByLightUpdates` — every section a `setStoredLevel`
    /// could have made a client's copy stale.
    ///
    /// This is the mask a partial `light_update` is sent with.
    /// `setStoredLevel` feeds it through `SectionPos.aroundAndAtBlockPos`,
    /// which is the section range from `(x-1, y-1, z-1)` to `(x+1, y+1, z+1)`
    /// — one section for a write in the middle of one, up to eight for a
    /// write on a corner. **Not** the section the write landed in: a client
    /// stores light per section and a nibble on a section face changes what
    /// the neighbouring section's edge renders like.
    sections_affected_by_light_updates: BTreeSet<Sec>,
    /// `SkyDataLayerStorageMap.currentLowestY`.
    current_lowest_y: i32,
    /// `SkyDataLayerStorageMap.topSections` — highest stored section Y plus
    /// one, per chunk. Absent means `currentLowestY`, which is the map's
    /// `defaultReturnValue`.
    top_sections: BTreeMap<(i32, i32), i32>,
}

impl LightStorage {
    /// A storage with nothing in it. `currentLowestY` starts above every
    /// section so the first `onNodeAdded` lowers it.
    #[must_use]
    pub fn new(layer: LightLayer, geometry: LightGeometry) -> Self {
        Self {
            layer,
            geometry,
            section_states: BTreeMap::new(),
            levels: BTreeMap::new(),
            materialised: BTreeSet::new(),
            sections_affected_by_light_updates: BTreeSet::new(),
            columns_with_sources: BTreeSet::new(),
            current_lowest_y: geometry.max_light_section() + 1,
            top_sections: BTreeMap::new(),
        }
    }

    /// `storingLightForSection(long)` — `getDataLayer(sec, true) != null`.
    #[must_use]
    pub fn storing_light_for_section(&self, s: Sec) -> bool {
        self.levels.contains_key(&s)
    }

    /// `getStoredLevel(long)`.
    #[must_use]
    pub fn stored_level(&self, p: Pos) -> i32 {
        self.levels
            .get(&block_to_section(p))
            .map_or(0, |d| i32::from(d[nibble_index(p)]))
    }

    /// `setStoredLevel(long, int)`.
    ///
    /// # Panics
    /// If the section is not stored. Vanilla would NPE; every call site here
    /// tests `storingLightForSection` first, and a panic is the honest
    /// reading of "this cannot happen" rather than a silent no-op that would
    /// lose light.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    pub fn set_stored_level(&mut self, p: Pos, level: i32) {
        let s = block_to_section(p);
        let d = self
            .levels
            .get_mut(&s)
            .expect("setStoredLevel on an unstored section");
        d[nibble_index(p)] = level as u8;
        // `getDataLayerToWrite` -> `DataLayer.set` -> `getData()`, which
        // allocates the array. Every write materialises, including a write
        // of zero.
        self.materialised.insert(s);
        around_and_at_block_pos(p, |sec| {
            self.sections_affected_by_light_updates.insert(sec);
        });
    }

    /// `lightOnInSection(long)` — a property of the *column*, not the
    /// section: `columnsWithSources.contains(getZeroNode(sec))`.
    #[must_use]
    pub fn light_on_in_section(&self, s: Sec) -> bool {
        self.columns_with_sources.contains(&(s.0, s.2))
    }

    /// `setLightEnabled(ChunkPos, boolean)`, the storage half.
    pub fn set_light_enabled(&mut self, chunk: (i32, i32), enabled: bool) {
        if enabled {
            self.columns_with_sources.insert(chunk);
        } else {
            self.columns_with_sources.remove(&chunk);
        }
    }

    /// `SkyLightSectionStorage.hasLightDataAtOrBelow(int)`.
    #[must_use]
    pub const fn has_light_data_at_or_below(&self, y: i32) -> bool {
        y >= self.current_lowest_y
    }

    /// `SkyLightSectionStorage.getBottomSectionY()`.
    #[must_use]
    pub const fn bottom_section_y(&self) -> i32 {
        self.current_lowest_y
    }

    /// `SkyLightSectionStorage.getTopSectionY(long)`.
    #[must_use]
    pub fn top_section_y(&self, chunk: (i32, i32)) -> i32 {
        self.top_sections
            .get(&chunk)
            .copied()
            .unwrap_or(self.current_lowest_y)
    }

    /// `SkyLightSectionStorage.isAboveData(long)`.
    #[must_use]
    pub fn is_above_data(&self, s: Sec) -> bool {
        let t = self.top_section_y((s.0, s.2));
        t == self.current_lowest_y || s.1 >= t
    }

    /// `LayerLightSectionStorage.updateSectionStatus(long, boolean)`.
    ///
    /// `is_empty` is `LevelChunkSection.hasOnlyAir()`. The 3x3x3 loop skips
    /// its own centre, so a section's own `hasData` bit and its neighbours'
    /// counts are maintained by different halves of this method — and the
    /// stored set is their union, which is why the neighbourhood is a cube in
    /// section space rather than a column.
    pub fn update_section_status(&mut self, s: Sec, is_empty: bool) {
        let old = self.section_states.get(&s).copied().unwrap_or(0);
        let new = section_state::has_data(old, !is_empty);
        if old == new {
            return;
        }
        self.put_section_state(s, new);
        let delta = if is_empty { -1 } else { 1 };
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = (s.0 + dx, s.1 + dy, s.2 + dz);
                    let st = self.section_states.get(&n).copied().unwrap_or(0);
                    let bumped = section_state::with_neighbor_count(
                        st,
                        section_state::neighbor_count(st) + delta,
                    );
                    self.put_section_state(n, bumped);
                }
            }
        }
    }

    /// `putSectionState(long, byte)`, with `initializeSection` and
    /// `removeSection` folded in.
    fn put_section_state(&mut self, s: Sec, state: u8) {
        if state != section_state::EMPTY {
            let had = self.section_states.insert(s, state).is_some();
            if !had {
                self.initialize_section(s);
            }
        } else if self.section_states.remove(&s).is_some() {
            self.levels.remove(&s);
            self.materialised.remove(&s);
            self.on_node_removed(s);
        }
    }

    /// `initializeSection(long)` — creates the data layer, which is what
    /// makes `storingLightForSection` true, and bookkeeps the sky column
    /// extents.
    fn initialize_section(&mut self, s: Sec) {
        // Clipped to the light section range: a section outside it is never
        // stored, and `LevelLightEngine`'s own `getLightSectionCount` bounds
        // the wire masks to the same range.
        if s.1 < self.geometry.min_light_section() || s.1 >= self.geometry.max_light_section() {
            self.section_states.remove(&s);
            return;
        }
        let initial = self.create_data_layer(s);
        self.levels.insert(s, initial);
        self.on_node_added(s);
    }

    /// `SkyLightSectionStorage.createDataLayer(long)` for the sky layer, and
    /// the inherited all-zero layer for block light.
    ///
    /// The sky case is the **column-fill shortcut** and it is not an
    /// optimisation that could be skipped: a section created *below* existing
    /// data does not start dark, it starts as sixteen copies of the bottom
    /// y-slice of the nearest stored section above it
    /// (`repeatFirstLayer`). A port that starts it dark and lets propagation
    /// fill it gets the same answer only where propagation can reach, and
    /// gets a dark band under a solid floor where vanilla has none.
    fn create_data_layer(&self, s: Sec) -> Vec<u8> {
        if self.layer == LightLayer::Block {
            return vec![0u8; 4096];
        }
        if self.is_above_data(s) {
            return if self.light_on_in_section(s) {
                vec![MAX_LEVEL; 4096]
            } else {
                vec![0u8; 4096]
            };
        }
        let mut up = (s.0, s.1 + 1, s.2);
        loop {
            if let Some(d) = self.levels.get(&up) {
                return repeat_first_layer(d);
            }
            up = (up.0, up.1 + 1, up.2);
            if up.1 > self.geometry.max_light_section() {
                return vec![0u8; 4096];
            }
        }
    }

    /// `SkyLightSectionStorage.onNodeAdded(long)`.
    fn on_node_added(&mut self, s: Sec) {
        if self.layer != LightLayer::Sky {
            return;
        }
        if self.current_lowest_y > s.1 {
            self.current_lowest_y = s.1;
        }
        let e = self.top_sections.entry((s.0, s.2)).or_insert(s.1 + 1);
        if *e < s.1 + 1 {
            *e = s.1 + 1;
        }
    }

    /// `SkyLightSectionStorage.onNodeRemoved(long)`.
    fn on_node_removed(&mut self, s: Sec) {
        if self.layer != LightLayer::Sky {
            return;
        }
        let chunk = (s.0, s.2);
        if self.top_section_y(chunk) != s.1 + 1 {
            return;
        }
        let mut y = s.1;
        let mut cur = s;
        while !self.storing_light_for_section(cur) && self.has_light_data_at_or_below(y) {
            y -= 1;
            cur = (cur.0, cur.1 - 1, cur.2);
        }
        if self.storing_light_for_section(cur) {
            self.top_sections.insert(chunk, y + 1);
        } else {
            self.top_sections.remove(&chunk);
        }
    }

    /// The stored sections, in ascending order — what the wire masks are
    /// built from.
    pub fn stored_sections(&self) -> impl Iterator<Item = &Sec> {
        self.levels.keys()
    }

    /// The sections a client's light for would now be stale —
    /// `sectionsAffectedByLightUpdates`, which is the mask a partial
    /// `light_update` carries.
    pub fn affected_sections(&self) -> impl Iterator<Item = &Sec> {
        self.sections_affected_by_light_updates.iter()
    }

    /// `markNewInconsistencies` clears it once the packets have been built.
    pub fn clear_affected_sections(&mut self) {
        self.sections_affected_by_light_updates.clear();
    }

    /// One section's levels, or `None` if it is not stored.
    #[must_use]
    pub fn section_levels(&self, s: Sec) -> Option<&[u8]> {
        self.levels.get(&s).map(Vec::as_slice)
    }
}

/// `SectionPos.aroundAndAtBlockPos(x, y, z, consumer)` — every section
/// touching the 3x3x3 block cube around `p`.
///
/// The bytecode computes `blockToSectionCoord` of each coordinate minus one
/// and plus one and iterates the inclusive box between them, with a
/// fast path when all three collapse. So it is **one** section for a write
/// strictly inside one, two across a face, four across an edge and eight at a
/// corner — never a fixed 27.
pub fn around_and_at_block_pos(p: Pos, mut f: impl FnMut(Sec)) {
    let (x, y, z) = (pos_x(p), pos_y(p), pos_z(p));
    let (x0, x1) = (block_to_section_coord(x - 1), block_to_section_coord(x + 1));
    let (y0, y1) = (block_to_section_coord(y - 1), block_to_section_coord(y + 1));
    let (z0, z1) = (block_to_section_coord(z - 1), block_to_section_coord(z + 1));
    for sx in x0..=x1 {
        for sy in y0..=y1 {
            for sz in z0..=z1 {
                f((sx, sy, sz));
            }
        }
    }
}

/// `SkyLightSectionStorage.repeatFirstLayer(DataLayer)` — the bottom y-slice,
/// sixteen times.
///
/// In nibble-per-byte terms the bottom slice is the first 256 entries
/// (`y = 0`), and vanilla copies the first 128 *bytes* into each of the
/// sixteen 128-byte blocks, which is the same 256 nibbles.
fn repeat_first_layer(d: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; 4096];
    for y in 0..16 {
        out[y * 256..(y + 1) * 256].copy_from_slice(&d[0..256]);
    }
    out
}

/// Which layer an [`Engine`] is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Block,
    Sky,
}

/// `LightEngine` — the queues, the check set and `runLightUpdates`.
pub struct Engine<'a> {
    layer: Layer,
    region: &'a Region,
    props: &'a LightProps,
    /// `blockNodesToCheck`. Ordered; see the module docs for why.
    block_nodes_to_check: BTreeSet<Pos>,
    /// `increaseQueue` — vanilla's `LongArrayFIFOQueue` holds the position
    /// and the entry as two consecutive longs, which is a pair.
    increase_queue: VecDeque<(Pos, QueueEntry)>,
    /// `decreaseQueue`.
    decrease_queue: VecDeque<(Pos, QueueEntry)>,
    /// `storage`.
    pub storage: LightStorage,
    /// Per chunk, `ChunkSkyLightSources`.
    sky_sources: BTreeMap<(i32, i32), SkySources>,
}

impl<'a> Engine<'a> {
    /// A new engine over `region`. **Nothing is declared yet**: no section
    /// status, no enabled column, no source.
    ///
    /// That is deliberate and it is the order `ThreadedLevelLightEngine`
    /// uses, which [`light_region`] reproduces: per chunk, declare its
    /// non-air sections, then enable it, and only once every chunk has been
    /// through that does anything propagate. Declaring them all in a
    /// constructor would change what `setLightEnabled`'s fill sees, because
    /// that fill reads `topSections`, which the declarations build.
    #[must_use]
    pub fn new(layer: Layer, region: &'a Region, props: &'a LightProps) -> Self {
        let light_layer = match layer {
            Layer::Block => LightLayer::Block,
            Layer::Sky => LightLayer::Sky,
        };
        let storage = LightStorage::new(light_layer, region.geometry);
        let mut sky_sources = BTreeMap::new();
        if layer == Layer::Sky {
            for &(cx, cz) in region.chunks.keys() {
                sky_sources.insert((cx, cz), compute_sky_sources(region, props, cx, cz));
            }
        }
        Self {
            layer,
            region,
            props,
            block_nodes_to_check: BTreeSet::new(),
            increase_queue: VecDeque::new(),
            decrease_queue: VecDeque::new(),
            storage,
            sky_sources,
        }
    }

    /// `LightEngine.updateSectionStatus(SectionPos, boolean)`.
    pub fn update_section_status(&mut self, s: Sec, is_empty: bool) {
        self.storage.update_section_status(s, is_empty);
    }

    /// `LightEngine.setLightEnabled(ChunkPos, boolean)`, dispatched to the
    /// sky override where there is one.
    pub fn set_light_enabled(&mut self, chunk: (i32, i32), enabled: bool) {
        match self.layer {
            Layer::Block => self.storage.set_light_enabled(chunk, enabled),
            Layer::Sky => self.sky_set_light_enabled(chunk, enabled),
        }
    }

    /// `LightEngine.propagateLightSources(ChunkPos)`, dispatched.
    pub fn propagate_light_sources(&mut self, chunk: (i32, i32)) {
        match self.layer {
            Layer::Block => self.block_propagate_light_sources(chunk),
            Layer::Sky => self.sky_propagate_light_sources(chunk),
        }
    }

    /// The property the module docs name: our check set is totally ordered,
    /// so a run is reproducible regardless of hashing.
    #[must_use]
    pub const fn check_order_is_total() -> bool {
        true
    }

    /// `LightEngine.checkBlock(BlockPos)`.
    pub fn check_block(&mut self, p: Pos) {
        self.block_nodes_to_check.insert(p);
    }

    /// `LightEngine.hasLightWork()`.
    #[must_use]
    pub fn has_light_work(&self) -> bool {
        !self.block_nodes_to_check.is_empty()
            || !self.decrease_queue.is_empty()
            || !self.increase_queue.is_empty()
    }

    /// `LightEngine.enqueueDecrease(long, long)`.
    fn enqueue_decrease(&mut self, p: Pos, e: QueueEntry) {
        self.decrease_queue.push_back((p, e));
    }

    /// `LightEngine.enqueueIncrease(long, long)`.
    fn enqueue_increase(&mut self, p: Pos, e: QueueEntry) {
        self.increase_queue.push_back((p, e));
    }

    /// `LightEngine.runLightUpdates()`. Returns the number of queue entries
    /// drained, which is vanilla's own return value.
    pub fn run_light_updates(&mut self) -> usize {
        let nodes: Vec<Pos> = self.block_nodes_to_check.iter().copied().collect();
        for p in nodes {
            self.check_node(p);
        }
        self.block_nodes_to_check.clear();
        let mut n = 0;
        n += self.propagate_decreases();
        n += self.propagate_increases();
        n
    }

    /// `LightEngine.propagateIncreases()`.
    ///
    /// The emission flag is what makes this more than a drain: an entry that
    /// came from `increaseLightFromEmission` may *raise* the stored level to
    /// its own, and only then propagate. Any other entry propagates only if
    /// the stored level still equals the level it was queued with — which is
    /// how a stale entry, overtaken by a later higher one, drops out.
    fn propagate_increases(&mut self) -> usize {
        let mut n = 0;
        while let Some((p, e)) = self.increase_queue.pop_front() {
            let mut stored = self.storage.stored_level(p);
            let from = e.from_level();
            if e.is_increase_from_emission() && stored < from {
                self.storage.set_stored_level(p, from);
                stored = from;
            }
            if stored == from {
                match self.layer {
                    Layer::Block => self.block_propagate_increase(p, e, stored),
                    Layer::Sky => self.sky_propagate_increase(p, e, stored),
                }
            }
            n += 1;
        }
        n
    }

    /// `LightEngine.propagateDecreases()`.
    fn propagate_decreases(&mut self) -> usize {
        let mut n = 0;
        while let Some((p, e)) = self.decrease_queue.pop_front() {
            match self.layer {
                Layer::Block => self.block_propagate_decrease(p, e),
                Layer::Sky => self.sky_propagate_decrease(p, e),
            }
            n += 1;
        }
        n
    }

    /// `LightEngine.getState(BlockPos)`. A position in an unloaded chunk
    /// reads as **bedrock**, not air — vanilla's `getChunkForLighting`
    /// returning null falls back to `Blocks.BEDROCK.defaultBlockState()`, so
    /// light does not leak off the edge of the loaded region.
    fn state(&self, p: Pos) -> u16 {
        let (cx, sy, cz) = block_to_section(p);
        self.region.chunks.get(&(cx, cz)).map_or(BEDROCK, |c| {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            c.sections
                .get(&sy)
                .map_or(self.region.air, |s| s[nibble_index(p)])
        })
    }

    fn check_node(&mut self, p: Pos) {
        match self.layer {
            Layer::Block => self.block_check_node(p),
            Layer::Sky => self.sky_check_node(p),
        }
    }
}

/// The state id this engine uses for a position in an unloaded chunk.
///
/// Vanilla returns `Blocks.BEDROCK.defaultBlockState()`. Its numeric id is
/// not needed: what the engine asks of it is `getOpacity` (15, so light dies)
/// and `isEmptyShape` (false). [`Engine::opacity_of`] and
/// [`Engine::shape_occluding_of`] special-case this sentinel rather than
/// depending on a table lookup that would differ between dimensions.
const BEDROCK: u16 = u16::MAX;

impl Engine<'_> {
    /// `LightEngine.getOpacity(BlockState)` — `max(1, getLightDampening())`.
    fn opacity_of(&self, state: u16) -> i32 {
        if state == BEDROCK {
            return i32::from(MAX_LEVEL);
        }
        i32::from(self.props.opacity(state))
    }

    fn emission_of(&self, state: u16) -> i32 {
        if state == BEDROCK {
            return 0;
        }
        i32::from(self.props.emission(state))
    }

    /// `LightEngine.isEmptyShape(BlockState)`.
    fn is_empty_shape(&self, state: u16) -> bool {
        if state == BEDROCK {
            return false;
        }
        !self.props.shape_occluding(state)
    }

    /// `LightEngine.shapeOccludes(from, to, dir)`, through the per-state face
    /// column — the same reduction, and the same residual, as
    /// `light_engine::shape_occludes`.
    fn shape_occludes(&self, from: u16, to: u16, d: Dir) -> bool {
        if from == BEDROCK || to == BEDROCK {
            return false;
        }
        let (a, da) = (from, d.ordinal());
        let (b, db) = (to, d.opposite().ordinal());
        if !self.props.shape_occluding(a) && !self.props.shape_occluding(b) {
            return false;
        }
        self.props.face_occludes(a, da) || self.props.face_occludes(b, db)
    }
}

/// `ChunkSkyLightSources.findLowestSourceY` for every column of one chunk.
fn compute_sky_sources(region: &Region, props: &LightProps, cx: i32, cz: i32) -> SkySources {
    let mut lowest = [0i32; 256];
    for z in 0..16u8 {
        for x in 0..16u8 {
            lowest[(z as usize) * 16 + x as usize] =
                crate::light_engine::lowest_source_y(region, props, cx, cz, x, z);
        }
    }
    SkySources { lowest }
}

// ---------------------------------------------------------------------------
// BlockLightEngine
// ---------------------------------------------------------------------------

impl Engine<'_> {
    /// `BlockLightEngine.getEmission(long, BlockState)`.
    ///
    /// The `lightOnInSection` guard is not redundant: a chunk whose light has
    /// not been enabled yet contributes no emission at all, so a torch in a
    /// chunk that is still loading does not light its neighbours early.
    fn block_emission(&self, p: Pos, state: u16) -> i32 {
        let e = self.emission_of(state);
        if e > 0 && self.storage.light_on_in_section(block_to_section(p)) {
            e
        } else {
            0
        }
    }

    /// `BlockLightEngine.checkNode(long)`.
    fn block_check_node(&mut self, p: Pos) {
        let sec = block_to_section(p);
        if !self.storage.storing_light_for_section(sec) {
            return;
        }
        let state = self.state(p);
        let emission = self.block_emission(p, state);
        let stored = self.storage.stored_level(p);
        if emission < stored {
            self.storage.set_stored_level(p, 0);
            self.enqueue_decrease(p, QueueEntry::decrease_all_directions(stored));
        } else {
            // The position kept whatever it had, so nothing is removed — but
            // its neighbours are still asked to push their light back in,
            // which is what repairs a position whose *source* moved rather
            // than whose own level fell.
            self.enqueue_decrease(p, PULL_LIGHT_IN_ENTRY);
        }
        if emission > 0 {
            let empty = self.is_empty_shape(state);
            self.enqueue_increase(p, QueueEntry::increase_light_from_emission(emission, empty));
        }
    }

    /// `BlockLightEngine.propagateIncrease(long, long, int)`.
    fn block_propagate_increase(&mut self, p: Pos, e: QueueEntry, level: i32) {
        let mut from_state: Option<u16> = None;
        for d in PROPAGATION_DIRECTIONS {
            if !e.should_propagate_in_direction(d) {
                continue;
            }
            let np = offset(p, d);
            if !self.storage.storing_light_for_section(block_to_section(np)) {
                continue;
            }
            let n_stored = self.storage.stored_level(np);
            // The cheap gate first, exactly as the bytecode orders it: even a
            // fully transparent neighbour costs one level, so `level - 1` is
            // the best any neighbour could reach.
            if level - 1 <= n_stored {
                continue;
            }
            let n_state = self.state(np);
            let new_level = level - self.opacity_of(n_state);
            if new_level <= n_stored {
                continue;
            }
            if from_state.is_none() {
                from_state = Some(if e.is_from_empty_shape() {
                    self.region.air
                } else {
                    self.state(p)
                });
            }
            if self.shape_occludes(from_state.expect("set above"), n_state, d) {
                continue;
            }
            self.storage.set_stored_level(np, new_level);
            if new_level > 1 {
                let empty = self.is_empty_shape(n_state);
                self.enqueue_increase(
                    np,
                    QueueEntry::increase_skip_one_direction(new_level, empty, d.opposite()),
                );
            }
        }
    }

    /// `BlockLightEngine.propagateDecrease(long, long)`.
    ///
    /// Note what is **absent**: there is no `shapeOccludes` test here. A
    /// decrease walks through a face an increase could not have crossed, and
    /// that is correct — it can only reach a position that has light, and if
    /// occlusion stopped the light getting there the position has none and
    /// the `stored == 0` test above ends the walk.
    fn block_propagate_decrease(&mut self, p: Pos, e: QueueEntry) {
        let from_level = e.from_level();
        for d in PROPAGATION_DIRECTIONS {
            if !e.should_propagate_in_direction(d) {
                continue;
            }
            let np = offset(p, d);
            if !self.storage.storing_light_for_section(block_to_section(np)) {
                continue;
            }
            let n_stored = self.storage.stored_level(np);
            if n_stored == 0 {
                continue;
            }
            // The bytecode is `if (stored > fromLevel - 1) goto else`, i.e.
            // this arm runs when `stored <= fromLevel - 1`. Written as `<`
            // because the level field is four bits wide, so `from_level` is
            // in `0..=15` and the two forms cannot differ.
            if n_stored < from_level {
                // This neighbour could only have been lit by what we just
                // removed. Zero it and carry on outward.
                let n_state = self.state(np);
                let n_emission = self.block_emission(np, n_state);
                self.storage.set_stored_level(np, 0);
                if n_emission < n_stored {
                    self.enqueue_decrease(
                        np,
                        QueueEntry::decrease_skip_one_direction(n_stored, d.opposite()),
                    );
                }
                if n_emission > 0 {
                    let empty = self.is_empty_shape(n_state);
                    self.enqueue_increase(
                        np,
                        QueueEntry::increase_light_from_emission(n_emission, empty),
                    );
                }
            } else {
                // Brighter than the removal could account for, so it is lit
                // by something else and becomes a source pushing back the
                // one way we came.
                self.enqueue_increase(
                    np,
                    QueueEntry::increase_only_one_direction(n_stored, false, d.opposite()),
                );
            }
        }
    }

    /// `BlockLightEngine.propagateLightSources(ChunkPos)` — enable the
    /// column, then seed every emitting block in it.
    pub fn block_propagate_light_sources(&mut self, chunk: (i32, i32)) {
        self.storage.set_light_enabled(chunk, true);
        let Some(c) = self.region.chunks.get(&chunk) else {
            return;
        };
        let mut seeds: Vec<(Pos, i32, bool)> = Vec::new();
        for (&sy, sec) in &c.sections {
            for (idx, &state) in sec.iter().enumerate() {
                let e = i32::from(self.props.emission(state));
                if e == 0 {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                let (x, y, z) = (
                    (idx & 15) as i32,
                    ((idx >> 8) & 15) as i32,
                    ((idx >> 4) & 15) as i32,
                );
                let p = as_long(chunk.0 * 16 + x, sy * 16 + y, chunk.1 * 16 + z);
                seeds.push((p, e, !self.props.shape_occluding(state)));
            }
        }
        for (p, e, empty) in seeds {
            self.enqueue_increase(p, QueueEntry::increase_light_from_emission(e, empty));
        }
    }
}

// ---------------------------------------------------------------------------
// SkyLightEngine
// ---------------------------------------------------------------------------

/// `SkyLightEngine.REMOVE_TOP_SKY_SOURCE_ENTRY`.
const REMOVE_TOP_SKY_SOURCE_ENTRY: QueueEntry = QueueEntry::decrease_all_directions(15);
/// `SkyLightEngine.REMOVE_SKY_SOURCE_ENTRY`.
const REMOVE_SKY_SOURCE_ENTRY: QueueEntry = QueueEntry::decrease_skip_one_direction(15, Dir::Up);
/// `SkyLightEngine.ADD_SKY_SOURCE_ENTRY`.
const ADD_SKY_SOURCE_ENTRY: QueueEntry =
    QueueEntry::increase_skip_one_direction(15, false, Dir::Up);

/// `SkyLightEngine.isSourceLevel(int)`.
const fn is_source_level(level: i32) -> bool {
    level == 15
}

/// `SkyLightEngine.crossedSectionEdge(Direction, int, int)`, where the two
/// ints are `sectionRelative(x)` and `sectionRelative(z)`.
///
/// The switch map in `SkyLightEngine$1` is NORTH=1, SOUTH=2, WEST=3, EAST=4,
/// and DOWN and UP fall through to the default. So this is only ever true for
/// a horizontal step that leaves the section.
const fn crossed_section_edge(d: Dir, x: i32, z: i32) -> bool {
    match d {
        Dir::North => z == 15,
        Dir::South => z == 0,
        Dir::West => x == 15,
        Dir::East => x == 0,
        Dir::Down | Dir::Up => false,
    }
}

impl Engine<'_> {
    /// `SkyLightEngine.getLowestSourceY(int, int, int)`.
    fn sky_lowest_source_y(&self, x: i32, z: i32, default: i32) -> i32 {
        self.sky_sources
            .get(&(block_to_section_coord(x), block_to_section_coord(z)))
            .map_or(default, |s| {
                s.lowest_source_y(section_relative(x), section_relative(z))
            })
    }

    /// `SkyLightEngine.checkNode(long)`.
    fn sky_check_node(&mut self, p: Pos) {
        let (x, y, z) = (pos_x(p), pos_y(p), pos_z(p));
        let sec = block_to_section(p);
        let lowest = if self.storage.light_on_in_section(sec) {
            self.sky_lowest_source_y(x, z, i32::MAX)
        } else {
            i32::MAX
        };
        if lowest != i32::MAX {
            self.update_sources_in_column(x, z, lowest);
        }
        if !self.storage.storing_light_for_section(sec) {
            return;
        }
        if y >= lowest {
            // Still a source. Both queues get an entry: the decrease clears
            // whatever the old value was everywhere except upward, and the
            // increase re-establishes 15 here.
            self.enqueue_decrease(p, REMOVE_SKY_SOURCE_ENTRY);
            self.enqueue_increase(p, ADD_SKY_SOURCE_ENTRY);
        } else {
            let stored = self.storage.stored_level(p);
            if stored > 0 {
                self.storage.set_stored_level(p, 0);
                self.enqueue_decrease(p, QueueEntry::decrease_all_directions(stored));
            } else {
                self.enqueue_decrease(p, PULL_LIGHT_IN_ENTRY);
            }
        }
    }

    /// `SkyLightEngine.updateSourcesInColumn(int, int, int)`.
    fn update_sources_in_column(&mut self, x: i32, z: i32, lowest: i32) {
        let bottom = self.storage.bottom_section_y() * 16;
        self.remove_sources_below(x, z, lowest, bottom);
        self.add_sources_above(x, z, lowest, bottom);
    }

    /// `SkyLightEngine.removeSourcesBelow(int, int, int, int)` — walk down
    /// from just under the new lowest source, section by section, zeroing
    /// every position that is still at 15 until one is not.
    fn remove_sources_below(&mut self, x: i32, z: i32, lowest: i32, bottom: i32) {
        if lowest <= bottom {
            return;
        }
        let cx = block_to_section_coord(x);
        let cz = block_to_section_coord(z);
        let top_block = lowest - 1;
        let mut sy = block_to_section_coord(top_block);
        while self.storage.has_light_data_at_or_below(sy) {
            if self.storage.storing_light_for_section((cx, sy, cz)) {
                let base = sy * 16;
                let mut y = (base + 15).min(top_block);
                while y >= base {
                    let p = as_long(x, y, z);
                    if !is_source_level(self.storage.stored_level(p)) {
                        return;
                    }
                    self.storage.set_stored_level(p, 0);
                    // The topmost one removed keeps its UP direction,
                    // because there is nothing above it that is still a
                    // source to protect; every one below skips UP.
                    let e = if y == lowest - 1 {
                        REMOVE_TOP_SKY_SOURCE_ENTRY
                    } else {
                        REMOVE_SKY_SOURCE_ENTRY
                    };
                    self.enqueue_decrease(p, e);
                    y -= 1;
                }
            }
            sy -= 1;
        }
    }

    /// `SkyLightEngine.addSourcesAbove(int, int, int, int)`.
    fn add_sources_above(&mut self, x: i32, z: i32, lowest: i32, bottom: i32) {
        let cx = block_to_section_coord(x);
        let cz = block_to_section_coord(z);
        // The highest of the four horizontal neighbours' own lowest source.
        // A new source below that height has a lit neighbour to push into and
        // must be queued; one above it is surrounded by sources and need not
        // be.
        let neighbour_high = self
            .sky_lowest_source_y(x - 1, z, i32::MIN)
            .max(self.sky_lowest_source_y(x + 1, z, i32::MIN))
            .max(self.sky_lowest_source_y(x, z - 1, i32::MIN))
            .max(self.sky_lowest_source_y(x, z + 1, i32::MIN));
        let start = lowest.max(bottom);
        let mut sec = (cx, block_to_section_coord(start), cz);
        while !self.storage.is_above_data(sec) {
            if self.storage.storing_light_for_section(sec) {
                let base = sec.1 * 16;
                let top = base + 15;
                let mut y = base.max(start);
                while y <= top {
                    let p = as_long(x, y, z);
                    if is_source_level(self.storage.stored_level(p)) {
                        return;
                    }
                    self.storage.set_stored_level(p, 15);
                    if y < neighbour_high || y == lowest {
                        self.enqueue_increase(p, ADD_SKY_SOURCE_ENTRY);
                    }
                    y += 1;
                }
            }
            sec = (sec.0, sec.1 + 1, sec.2);
        }
    }

    /// `SkyLightEngine.countEmptySectionsBelowIfAtBorder(long)`.
    ///
    /// Zero unless the position sits on a section's **bottom** y-slice and on
    /// one of its four horizontal edges; then it counts how many sections
    /// directly below are unstored but still within the data. Those are the
    /// sections a horizontal step at a section corner would otherwise skip
    /// straight past, and [`Engine::propagate_from_empty_sections`] is what
    /// fills them.
    fn count_empty_sections_below_if_at_border(&self, p: Pos) -> i32 {
        let y = pos_y(p);
        if section_relative(y) != 0 {
            return 0;
        }
        let (x, z) = (pos_x(p), pos_z(p));
        let (rx, rz) = (section_relative(x), section_relative(z));
        if !(rx == 0 || rx == 15 || rz == 0 || rz == 15) {
            return 0;
        }
        let (cx, sy, cz) = (
            block_to_section_coord(x),
            block_to_section_coord(y),
            block_to_section_coord(z),
        );
        let mut n = 0;
        while !self.storage.storing_light_for_section((cx, sy - n - 1, cz))
            && self.storage.has_light_data_at_or_below(sy - n - 1)
        {
            n += 1;
        }
        n
    }

    /// `SkyLightEngine.propagateFromEmptySections(long, Direction, int,
    /// boolean, int)`.
    fn propagate_from_empty_sections(
        &mut self,
        p: Pos,
        d: Dir,
        level: i32,
        increase: bool,
        empty_below: i32,
    ) {
        if empty_below == 0 {
            return;
        }
        let (x, z) = (pos_x(p), pos_z(p));
        if !crossed_section_edge(d, section_relative(x), section_relative(z)) {
            return;
        }
        let y = pos_y(p);
        let (cx, cz) = (block_to_section_coord(x), block_to_section_coord(z));
        let mut sy = block_to_section_coord(y) - 1;
        let stop = sy - empty_below + 1;
        while sy >= stop {
            if !self.storage.storing_light_for_section((cx, sy, cz)) {
                sy -= 1;
                continue;
            }
            let base = sy * 16;
            let mut iy = 15i32;
            while iy >= 0 {
                let np = as_long(x, base + iy, z);
                if increase {
                    self.storage.set_stored_level(np, level);
                    if level > 1 {
                        self.enqueue_increase(
                            np,
                            QueueEntry::increase_skip_one_direction(level, true, d.opposite()),
                        );
                    }
                } else {
                    self.storage.set_stored_level(np, 0);
                    self.enqueue_decrease(
                        np,
                        QueueEntry::decrease_skip_one_direction(level, d.opposite()),
                    );
                }
                iy -= 1;
            }
            sy -= 1;
        }
    }

    /// `SkyLightEngine.propagateIncrease(long, long, int)`.
    fn sky_propagate_increase(&mut self, p: Pos, e: QueueEntry, level: i32) {
        let mut from_state: Option<u16> = None;
        let empty_below = self.count_empty_sections_below_if_at_border(p);
        for d in PROPAGATION_DIRECTIONS {
            if !e.should_propagate_in_direction(d) {
                continue;
            }
            let np = offset(p, d);
            if !self.storage.storing_light_for_section(block_to_section(np)) {
                continue;
            }
            let n_stored = self.storage.stored_level(np);
            if level - 1 <= n_stored {
                continue;
            }
            let n_state = self.state(np);
            let new_level = level - self.opacity_of(n_state);
            if new_level <= n_stored {
                continue;
            }
            if from_state.is_none() {
                from_state = Some(if e.is_from_empty_shape() {
                    self.region.air
                } else {
                    self.state(p)
                });
            }
            if self.shape_occludes(from_state.expect("set above"), n_state, d) {
                continue;
            }
            self.storage.set_stored_level(np, new_level);
            if new_level > 1 {
                let empty = self.is_empty_shape(n_state);
                self.enqueue_increase(
                    np,
                    QueueEntry::increase_skip_one_direction(new_level, empty, d.opposite()),
                );
            }
            self.propagate_from_empty_sections(np, d, new_level, true, empty_below);
        }
    }

    /// `SkyLightEngine.propagateDecrease(long, long)`.
    fn sky_propagate_decrease(&mut self, p: Pos, e: QueueEntry) {
        let empty_below = self.count_empty_sections_below_if_at_border(p);
        let from_level = e.from_level();
        for d in PROPAGATION_DIRECTIONS {
            if !e.should_propagate_in_direction(d) {
                continue;
            }
            let np = offset(p, d);
            if !self.storage.storing_light_for_section(block_to_section(np)) {
                continue;
            }
            let n_stored = self.storage.stored_level(np);
            if n_stored == 0 {
                continue;
            }
            // The bytecode is `if (stored > fromLevel - 1) goto else`, i.e.
            // this arm runs when `stored <= fromLevel - 1`. Written as `<`
            // because the level field is four bits wide, so `from_level` is
            // in `0..=15` and the two forms cannot differ.
            if n_stored < from_level {
                self.storage.set_stored_level(np, 0);
                self.enqueue_decrease(
                    np,
                    QueueEntry::decrease_skip_one_direction(n_stored, d.opposite()),
                );
                self.propagate_from_empty_sections(np, d, n_stored, false, empty_below);
            } else {
                self.enqueue_increase(
                    np,
                    QueueEntry::increase_only_one_direction(n_stored, false, d.opposite()),
                );
            }
        }
    }

    /// `SkyLightEngine.setLightEnabled(ChunkPos, boolean)`.
    ///
    /// The override's own half is the fill: every stored section from just
    /// above the chunk's highest source up to its top section is filled with
    /// 15 if it is still empty. That is what makes the sky above the terrain
    /// bright without a single propagation step.
    pub fn sky_set_light_enabled(&mut self, chunk: (i32, i32), enabled: bool) {
        self.storage.set_light_enabled(chunk, enabled);
        if !enabled {
            return;
        }
        let highest = self
            .sky_sources
            .get(&chunk)
            .map_or(self.storage.bottom_section_y() * 16, |s| {
                s.highest_lowest_source_y()
            });
        let above = highest - 1;
        let first = block_to_section_coord(above) + 1;
        let top = self.storage.top_section_y(chunk);
        let bottom = self.storage.bottom_section_y().max(first);
        let mut sy = top - 1;
        while sy >= bottom {
            let s = (chunk.0, sy, chunk.1);
            if let Some(d) = self.storage.levels.get_mut(&s) {
                if d.iter().all(|&v| v == 0) {
                    d.fill(15);
                }
            }
            sy -= 1;
        }
    }

    /// `SkyLightEngine.propagateLightSources(ChunkPos)`.
    ///
    /// Transcribed whole, because every one of its five neighbour lookups is
    /// a *cross-chunk* one and the shape of the loop is the answer to "how
    /// does the sky get lit without propagating 15 through sixteen thousand
    /// air blocks per chunk". It does not propagate: it writes 15 straight
    /// into the data layer for every position at or above the column's
    /// source, and enqueues an increase only at the source itself or where a
    /// horizontal neighbour's source is higher — i.e. only where there is
    /// darkness for the light to flow into.
    ///
    /// The descent stops at the first section in which **no** column's source
    /// lies below the section floor (`anyBelow`), which is what bounds the
    /// work to the terrain rather than to the world height.
    pub fn sky_propagate_light_sources(&mut self, chunk: (i32, i32)) {
        // The storage's own `setLightEnabled`, not the engine override: this
        // method does the filling itself, so the override's blanket fill must
        // not also run here.
        self.storage.set_light_enabled(chunk, true);
        let (cx, cz) = chunk;
        let sources = self.sky_sources.get(&chunk).cloned();
        let north = self.sky_sources.get(&(cx, cz - 1)).cloned();
        let south = self.sky_sources.get(&(cx, cz + 1)).cloned();
        let west = self.sky_sources.get(&(cx - 1, cz)).cloned();
        let east = self.sky_sources.get(&(cx + 1, cz)).cloned();
        // `Objects.requireNonNullElse(_, emptyChunkSources)`: an absent
        // neighbour reads as a chunk whose every column's source is the
        // bottom of the world, so it never asks for light.
        let empty_source = self.storage.bottom_section_y() * 16;
        let at = |s: &Option<SkySources>, x: i32, z: i32| -> i32 {
            s.as_ref().map_or(empty_source, |s| s.lowest_source_y(x, z))
        };
        let top = self.storage.top_section_y(chunk);
        let bottom = self.storage.bottom_section_y();
        let (bx, bz) = (cx * 16, cz * 16);
        let mut queued: Vec<(Pos, QueueEntry)> = Vec::new();
        let mut sy = top - 1;
        while sy >= bottom {
            let s = (cx, sy, cz);
            if !self.storage.storing_light_for_section(s) {
                sy -= 1;
                continue;
            }
            let y0 = sy * 16;
            let y1 = y0 + 15;
            let mut any_below = false;
            // `getDataLayerToWrite` is called once per section here, and the
            // materialisation it causes is unconditional — even if the inner
            // loops then write nothing, which is `roofed`'s top section.
            self.storage.materialised.insert(s);
            for z in 0..16i32 {
                for x in 0..16i32 {
                    let src_y = at(&sources, x, z);
                    if src_y > y1 {
                        continue;
                    }
                    let n_src = if z == 0 {
                        at(&north, x, 15)
                    } else {
                        at(&sources, x, z - 1)
                    };
                    let s_src = if z == 15 {
                        at(&south, x, 0)
                    } else {
                        at(&sources, x, z + 1)
                    };
                    let w_src = if x == 0 {
                        at(&west, 15, z)
                    } else {
                        at(&sources, x - 1, z)
                    };
                    let e_src = if x == 15 {
                        at(&east, 0, z)
                    } else {
                        at(&sources, x + 1, z)
                    };
                    let max_neighbour = n_src.max(s_src).max(w_src.max(e_src));
                    let mut y = y1;
                    while y >= y0.max(src_y) {
                        let d = self
                            .storage
                            .levels
                            .get_mut(&s)
                            .expect("stored, tested above");
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let idx = DataLayer::index(x as u8, section_relative(y) as u8, z as u8);
                        d[idx] = 15;
                        if y == src_y || y < max_neighbour {
                            queued.push((
                                as_long(bx + x, y, bz + z),
                                QueueEntry::increase_sky_source_in_directions(
                                    y == src_y,
                                    y < n_src,
                                    y < s_src,
                                    y < w_src,
                                    y < e_src,
                                ),
                            ));
                        }
                        y -= 1;
                    }
                    if src_y < y0 {
                        any_below = true;
                    }
                }
            }
            if !any_below {
                break;
            }
            sy -= 1;
        }
        for (p, e) in queued {
            self.enqueue_increase(p, e);
        }
    }
}

// ---------------------------------------------------------------------------
// The driver, and the view `crate::light::classify` consumes
// ---------------------------------------------------------------------------

/// A lit region, as the incremental engine leaves it.
#[derive(Debug, Clone, Default)]
pub struct IncrementalLight {
    layers: BTreeMap<(i32, i32, u8, i32), DataLayer>,
    stored: BTreeSet<Sec>,
}

impl IncrementalLight {
    /// A [`crate::light::SectionLightSource`] view of one chunk.
    #[must_use]
    pub fn chunk(&self, cx: i32, cz: i32) -> IncrementalChunkLight<'_> {
        IncrementalChunkLight {
            light: self,
            cx,
            cz,
        }
    }

    /// The sections stored for this chunk, ascending.
    #[must_use]
    pub fn stored_sections(&self, cx: i32, cz: i32) -> Vec<i32> {
        let mut v: Vec<i32> = self
            .stored
            .iter()
            .filter(|(x, _, z)| *x == cx && *z == cz)
            .map(|(_, y, _)| *y)
            .collect();
        v.sort_unstable();
        v
    }

    /// The raw level at a block position, which is what a live
    /// `light_update` needs and what a comparison against harvested nibbles
    /// reads.
    #[must_use]
    pub fn level_at(&self, layer: LightLayer, p: Pos) -> u8 {
        let (cx, sy, cz) = block_to_section(p);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        self.layers.get(&(cx, cz, layer as u8, sy)).map_or(0, |d| {
            d.get(
                section_relative(pos_x(p)) as u8,
                section_relative(pos_y(p)) as u8,
                section_relative(pos_z(p)) as u8,
            )
        })
    }
}

/// One chunk's incremental light.
#[derive(Debug, Clone, Copy)]
pub struct IncrementalChunkLight<'a> {
    light: &'a IncrementalLight,
    cx: i32,
    cz: i32,
}

impl crate::light::SectionLightSource for IncrementalChunkLight<'_> {
    fn data_layer(&self, layer: LightLayer, section_y: i32) -> Option<&DataLayer> {
        self.light
            .layers
            .get(&(self.cx, self.cz, layer as u8, section_y))
    }
}

/// Turn one engine's storage into `DataLayer`s.
///
/// The homogeneity reconstruction is the same as
/// `light_engine::light`'s and for the same reason: vanilla's rule is
/// allocation history, ours is a property of the values, and the two agree on
/// the wire everywhere except the written-then-darkened case already
/// registered as the `relit` divergence.
fn harvest(engine: &Engine<'_>, layer: LightLayer, out: &mut IncrementalLight) {
    let all: Vec<Sec> = engine.storage.levels.keys().copied().collect();
    harvest_sections(engine, layer, out, &all);
}

/// The same, restricted to named sections — `swapSectionMap`'s copy of
/// `changedSections` into `visibleSectionData` rather than a whole rebuild.
///
/// A section not named keeps whatever it had, which is what makes this the
/// incremental form: after one block edit the changed set is one to eight
/// sections and the region is thousands.
fn harvest_sections(
    engine: &Engine<'_>,
    layer: LightLayer,
    out: &mut IncrementalLight,
    sections: &[Sec],
) {
    for &sec in sections {
        let Some(vals) = engine.storage.levels.get(&sec) else {
            continue;
        };
        let (cx, sy, cz) = sec;
        out.stored.insert((cx, sy, cz));
        let first = vals[0];
        // Vanilla's rule is allocation history, and the incremental engine
        // HAS the history — so this reads the history rather than
        // reconstructing it from the values the way `light_engine::light`
        // has to. The two differ in exactly one case, the one an edit
        // creates: written, then darkened back to zero.
        let uniform = !engine.storage.materialised.contains(&(cx, sy, cz));
        debug_assert!(
            !uniform || vals.iter().all(|&v| v == first),
            "an unwritten layer is homogeneous by construction"
        );
        let d = if uniform {
            DataLayer::filled(first)
        } else {
            let mut d = DataLayer::default();
            // Materialise FIRST, unconditionally. Writing only the non-zero
            // nibbles would leave an all-dark written layer un-materialised
            // and report it empty — which is precisely the `relit` state this
            // whole flag exists for.
            d.data_mut();
            for (i, &v) in vals.iter().enumerate() {
                if v != 0 {
                    let data = d.data_mut();
                    let shift = 4 * (i & 1);
                    data[i >> 1] = (data[i >> 1] & !(15 << shift)) | (v << shift);
                }
            }
            d
        };
        out.layers.insert((cx, cz, layer as u8, sy), d);
    }
}

/// Light `region` to convergence with the incremental engine, in
/// `ThreadedLevelLightEngine`'s order.
///
/// ```text
///   for each chunk:
///       for each non-air section: updateSectionStatus(sec, false)
///       setLightEnabled(pos, true)
///   for each chunk: propagateLightSources(pos)
///   while (hasLightWork()) runLightUpdates();
/// ```
///
/// The two loops are separate and that is load-bearing: a chunk's light
/// depends on its neighbours' sections having been declared, so nothing may
/// propagate until every declaration is in.
///
/// # Panics
/// If the drain does not converge within `MAX_DRAIN` calls. A fixed
/// iteration count would silently truncate a large propagation and report a
/// half-lit world as an answer; this says so instead.
#[must_use]
pub fn light_region(region: &Region, props: &LightProps) -> IncrementalLight {
    light_region_dimension(region, props, true)
}

/// The same, for a dimension that may have no sky light.
///
/// `LevelLightEngine(getter, blockLight, skyLight)` builds a `SkyLightEngine`
/// only when the third argument is set, and `ChunkMap` passes
/// `level.dimensionType().hasSkyLight()`. With no sky engine,
/// `getDataLayerData(SKY, _)` returns null for every section, so the packet
/// carries **no sky bit at all** — not an empty one. The nether and the End
/// are both in that state, and a port that lit the sky anyway would put 15s
/// on the wire for a dimension that has no sky.
#[must_use]
pub fn light_region_dimension(
    region: &Region,
    props: &LightProps,
    sky_light: bool,
) -> IncrementalLight {
    let mut out = IncrementalLight::default();
    let layers: &[(Layer, LightLayer)] = if sky_light {
        &[
            (Layer::Block, LightLayer::Block),
            (Layer::Sky, LightLayer::Sky),
        ]
    } else {
        &[(Layer::Block, LightLayer::Block)]
    };
    for &(layer, light_layer) in layers {
        let mut e = Engine::new(layer, region, props);
        for (&(cx, cz), c) in &region.chunks {
            for &sy in c.sections.keys() {
                e.update_section_status((cx, sy, cz), false);
            }
            e.set_light_enabled((cx, cz), true);
        }
        for &(cx, cz) in region.chunks.keys() {
            e.propagate_light_sources((cx, cz));
        }
        drain(&mut e);
        harvest(&e, light_layer, &mut out);
    }
    out
}

/// The same region, lit **one chunk at a time** — declare, enable, propagate,
/// drain — instead of declaring every chunk before propagating any.
///
/// # Why a second driver exists, and what it is for
///
/// [`light_region`] is `ThreadedLevelLightEngine`'s batch shape and it is the
/// right one for a region that arrives whole. A real server does not receive
/// a region whole: chunks load one at a time, and each one's declarations
/// land on a storage whose earlier chunks are **already lit**. That is not a
/// scheduling detail, because `SkyLightSectionStorage.createDataLayer` reads
/// the section above at the moment a section is created:
///
/// ```text
///   if (d.isDefinitelyHomogenous()) return d.copy();
///   byte[] src = d.getData();
///   for (i in 0..16) arraycopy(src, 0, out, i * 128, 128);
/// ```
///
/// A layer created above an *unlit* section is a copy of a homogeneous zero
/// layer, which stays unmaterialised and is never saved. A layer created
/// above a *lit* one is sixteen copies of its bottom y-slice, which is. So
/// the same world lit in two orders leaves two different padding sections,
/// and only one of them is the order a server reaches.
///
/// Measured: the End's harvest residual is 5,609 sky nibbles under
/// [`light_region`], every one in the padding section -1, and vanilla's
/// values there equal **32,768 of 32,768** copies of our own converged bottom
/// slice of section 0 — which is `repeatFirstLayer` of a lit section, exactly.
/// `crates/rustmc-server/tests/light_harvest.rs`, `RUSTMC_LIGHT_HARVEST_PROBE=1`.
///
/// Chunk order here is the region's own `BTreeMap` order, which is
/// `(x, z)`-ascending. A server's is its ticket order and is not reproducible
/// from a saved region file, so this reproduces *that a per-chunk order
/// happened* and not *which one*.
///
/// # AND IT DOES NOT CLOSE THE END'S RESIDUAL — IT MAKES IT WORSE
///
/// This function is kept because the measurement is worth more than the code:
/// under it the End's harvest residual goes from **5,609 to 14,537** of
/// 1,863,680, while the overworld and the nether stay at 0 of 2,093,056 and 0
/// of 2,080,768 — so the driver order is invisible in the two dimensions
/// whose padding section carries no data, and `(x, z)`-ascending is not
/// vanilla's load order.
///
/// What is now known exactly: the target is `repeatFirstLayer` of a **lit**
/// section, our converged bottom slice matches vanilla's whole padding
/// section 32,768 of 32,768 under BOTH drivers, and the disagreement is a
/// layer created at the wrong moment rather than a light value computed
/// wrongly. Writing that copy at harvest time would produce the right bytes
/// by a route vanilla does not take, which this project does not do. **Do not
/// re-run the order sweep**: it is one order and it moved the figure the
/// wrong way, and `docs/AGENT-BRIEF.md` already records what chasing orders
/// costs. The item closes when the harness can reproduce a chunk *load*
/// order, which a saved region file does not record.
///
/// # Panics
/// If a drain does not converge — see [`drain`].
#[must_use]
pub fn light_region_per_chunk(
    region: &Region,
    props: &LightProps,
    sky_light: bool,
) -> IncrementalLight {
    let mut out = IncrementalLight::default();
    let layers: &[(Layer, LightLayer)] = if sky_light {
        &[
            (Layer::Block, LightLayer::Block),
            (Layer::Sky, LightLayer::Sky),
        ]
    } else {
        &[(Layer::Block, LightLayer::Block)]
    };
    for &(layer, light_layer) in layers {
        let mut e = Engine::new(layer, region, props);
        for (&(cx, cz), c) in &region.chunks {
            for &sy in c.sections.keys() {
                e.update_section_status((cx, sy, cz), false);
            }
            e.set_light_enabled((cx, cz), true);
            e.propagate_light_sources((cx, cz));
            drain(&mut e);
        }
        harvest(&e, light_layer, &mut out);
    }
    out
}

/// The drain guard. Vanilla's own golden generator uses 10,000 and asserts
/// rather than truncating; the same number for the same reason.
pub const MAX_DRAIN: usize = 10_000;

/// Run `runLightUpdates` until `hasLightWork` is false.
///
/// # Panics
/// If it has not converged after [`MAX_DRAIN`] calls.
pub fn drain(e: &mut Engine<'_>) -> usize {
    let mut guard = 0;
    while e.has_light_work() {
        e.run_light_updates();
        guard += 1;
        assert!(
            guard <= MAX_DRAIN,
            "light did not converge in {MAX_DRAIN} drains"
        );
    }
    guard
}

/// Everything an [`Engine`] carries between calls, with the borrows dropped.
///
/// `Engine` borrows its `Region` and its `LightProps` for its whole life, and
/// a block edit produces a different `Region`. A long-lived server therefore
/// cannot hold an `Engine`; it holds this, and binds one for the length of a
/// drain. Vanilla has no equivalent because its `LightChunkGetter` reads a
/// mutable level — the split is a consequence of the borrow, not of the
/// algorithm, and nothing about the engine's state lives outside it.
///
/// The one thing NOT carried is `ChunkSkyLightSources`, which
/// [`EngineState::bind`] recomputes: that is
/// `ChunkAccess.initializeLightSources()` running again after the edit, and
/// vanilla's `LevelChunk.setBlockState` maintains the same table on every
/// write. A stale one here would be a port defect and not a shortcut — it is
/// the exact defect that made vanilla's own golden record that an 8x8 stone
/// lid darkens nothing (`docs/FINDINGS.md`, 2026-09-05).
#[derive(Debug, Clone)]
pub struct EngineState {
    layer: Layer,
    block_nodes_to_check: BTreeSet<Pos>,
    increase_queue: VecDeque<(Pos, QueueEntry)>,
    decrease_queue: VecDeque<(Pos, QueueEntry)>,
    /// `storage`.
    pub storage: LightStorage,
}

impl EngineState {
    /// Bind this state to a region and get an engine back.
    #[must_use]
    pub fn bind<'a>(self, region: &'a Region, props: &'a LightProps) -> Engine<'a> {
        let mut sky_sources = BTreeMap::new();
        if self.layer == Layer::Sky {
            for &(cx, cz) in region.chunks.keys() {
                sky_sources.insert((cx, cz), compute_sky_sources(region, props, cx, cz));
            }
        }
        Engine {
            layer: self.layer,
            region,
            props,
            block_nodes_to_check: self.block_nodes_to_check,
            increase_queue: self.increase_queue,
            decrease_queue: self.decrease_queue,
            storage: self.storage,
            sky_sources,
        }
    }

    /// `LightEngine.checkBlock(BlockPos)`, without a region to bind to.
    ///
    /// Vanilla's `checkBlock` is one `blockNodesToCheck.add`; nothing about
    /// it reads the level, so an edit can be recorded against the state and
    /// the drain deferred to the next tick, which is what
    /// `ThreadedLevelLightEngine` does with it anyway.
    pub fn check_block(&mut self, p: Pos) {
        self.block_nodes_to_check.insert(p);
    }

    /// `LightEngine.hasLightWork()`.
    #[must_use]
    pub fn has_light_work(&self) -> bool {
        !self.block_nodes_to_check.is_empty()
            || !self.decrease_queue.is_empty()
            || !self.increase_queue.is_empty()
    }
}

impl<'a> Engine<'a> {
    /// Drop the borrows and keep the state — the inverse of
    /// [`EngineState::bind`].
    #[must_use]
    pub fn into_state(self) -> EngineState {
        EngineState {
            layer: self.layer,
            block_nodes_to_check: self.block_nodes_to_check,
            increase_queue: self.increase_queue,
            decrease_queue: self.decrease_queue,
            storage: self.storage,
        }
    }

    /// Rebind an engine's storage to a **different** region — the same world
    /// after an edit.
    #[must_use]
    pub fn rebind(self, region: &'a Region, props: &'a LightProps) -> Engine<'a> {
        self.into_state().bind(region, props)
    }

    /// Which layer this engine runs.
    #[must_use]
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Harvest the sections this engine's storage names as affected, into
    /// `out`, and clear the affected set — `swapSectionMap`, whose two halves
    /// are the visible-data copy and the `onLightUpdate` callback.
    ///
    /// The affected set is a **superset** of the changed one:
    /// `setStoredLevel` marks `aroundAndAtBlockPos` of the write, which
    /// always includes the write's own section. Harvesting the superset
    /// re-reads a few unchanged neighbours and cannot miss a changed one.
    ///
    /// Returns the sections, which is what `LightChunkGetter.onLightUpdate`
    /// is handed one at a time.
    pub fn swap_section_map(&mut self, layer: LightLayer, out: &mut IncrementalLight) -> Vec<Sec> {
        let affected: Vec<Sec> = self.storage.affected_sections().copied().collect();
        // Vanilla's swap is a **whole-map copy**: `swapSectionMap` replaces
        // `visibleSectionData` with a copy of `updatingSectionData` whenever
        // `changedSections` is non-empty, so a section created but never
        // written — a uniform sky layer above the terrain, which no
        // `setStoredLevel` ever touches and which therefore appears in no
        // affected set — becomes visible with everything else. Copying the
        // whole map on every edit is what this port cannot afford on a
        // world-sized region, so the equivalence is made explicit instead:
        // affected sections, plus every stored section that has never been
        // harvested at all. A section already harvested can only change
        // through `setStoredLevel`, which marks it affected.
        //
        // Getting this wrong is not subtle and was measured: harvesting only
        // the affected set after a from-scratch build left **17 sections of
        // 468** — 69,632 nibbles — absent from the classification, because
        // `propagateLightSources` fills a sky column by creating layers
        // rather than by writing them.
        //
        // The "never harvested" test is **per layer**, not per section:
        // `IncrementalLight.stored` is keyed by section alone and both
        // engines write into it, so testing it would let the block layer's
        // pass hide every sky layer of the same sections. Measured: 17 of
        // 468 section/layer pairs — 69,632 nibbles, sky sections 4 and 5 of
        // all nine chunks — came back absent that way.
        let mut sections = affected.clone();
        sections.extend(
            self.storage
                .levels
                .keys()
                .filter(|&&(cx, sy, cz)| !out.layers.contains_key(&(cx, cz, layer as u8, sy)))
                .copied(),
        );
        harvest_sections(self, layer, out, &sections);
        self.storage.clear_affected_sections();
        affected
    }
}

/// Light `before` from scratch, then apply the edits that turn it into
/// `after` and let the engine repair the light incrementally — which is what
/// a server does when a player places a torch.
///
/// `edits` are the positions that changed, in the order they were applied.
/// `LevelLightEngine.checkBlock` is called for each, then the drain runs
/// once; that is `ServerLevel.setBlock`'s route and it is why a batch of
/// edits produces one repair rather than one repair per block.
///
/// # Panics
/// If either drain fails to converge — see [`drain`].
#[must_use]
pub fn light_region_with_edits(
    before: &Region,
    after: &Region,
    props: &LightProps,
    edits: &[Pos],
) -> IncrementalLight {
    let mut out = IncrementalLight::default();
    for (layer, light_layer) in [
        (Layer::Block, LightLayer::Block),
        (Layer::Sky, LightLayer::Sky),
    ] {
        let mut e = Engine::new(layer, before, props);
        for (&(cx, cz), c) in &before.chunks {
            for &sy in c.sections.keys() {
                e.update_section_status((cx, sy, cz), false);
            }
            e.set_light_enabled((cx, cz), true);
        }
        for &(cx, cz) in before.chunks.keys() {
            e.propagate_light_sources((cx, cz));
        }
        drain(&mut e);

        let mut e = e.rebind(after, props);
        // `LevelChunk.setBlockState` declares a section that has just stopped
        // being all-air, before `checkBlock` runs. A section that has just
        // become all-air is NOT undeclared there — vanilla leaves that to
        // chunk unload — so only the one direction is done here.
        for (&(cx, cz), c) in &after.chunks {
            for &sy in c.sections.keys() {
                e.update_section_status((cx, sy, cz), false);
            }
        }
        for &p in edits {
            e.check_block(p);
        }
        drain(&mut e);
        harvest(&e, light_layer, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BlockPos.asLong`'s three fields, round-tripped across the sign
    /// boundary that a `/ 16` would get wrong.
    #[test]
    fn block_pos_packing_round_trips_negative_coordinates() {
        for (x, y, z) in [
            (0, 0, 0),
            (-1, -1, -1),
            (15, 319, 15),
            (-16, -64, -16),
            (-30_000_000, -64, 30_000_000),
        ] {
            let p = as_long(x, y, z);
            assert_eq!((pos_x(p), pos_y(p), pos_z(p)), (x, y, z));
        }
    }

    /// `SectionPos.blockToSectionCoord` is an arithmetic shift and floors;
    /// `/ 16` truncates toward zero and is wrong for every negative
    /// coordinate above -16.
    #[test]
    fn block_to_section_coord_floors() {
        assert_eq!(block_to_section_coord(-1), -1);
        assert_eq!(block_to_section_coord(-16), -1);
        assert_eq!(block_to_section_coord(-17), -2);
        assert_eq!(block_to_section_coord(0), 0);
        assert_eq!(block_to_section_coord(15), 0);
        assert_ne!(block_to_section_coord(-1), -1 / 16);
    }

    /// `offset` moves exactly one block in `Direction`'s own normal.
    #[test]
    fn offset_matches_directions_normals() {
        let p = as_long(3, 70, -5);
        for d in PROPAGATION_DIRECTIONS {
            let (dx, dy, dz) = d.offset();
            let n = offset(p, d);
            assert_eq!((pos_x(n), pos_y(n), pos_z(n)), (3 + dx, 70 + dy, -5 + dz));
        }
    }

    /// `SectionPos.aroundAndAtBlockPos` is one section in the middle, two
    /// across a face, four across an edge and eight at a corner — never a
    /// fixed 27, which is what a port that copied `updateSectionStatus`'s
    /// cube would produce.
    #[test]
    fn around_and_at_block_pos_is_one_to_eight_sections() {
        let count = |x, y, z| {
            let mut n = 0;
            around_and_at_block_pos(as_long(x, y, z), |_| n += 1);
            n
        };
        assert_eq!(count(8, 8, 8), 1, "strictly inside one section");
        assert_eq!(count(0, 8, 8), 2, "on the -x face");
        assert_eq!(count(15, 8, 8), 2, "on the +x face");
        assert_eq!(count(0, 0, 8), 4, "on an edge");
        assert_eq!(count(0, 0, 0), 8, "at a corner");
        assert_eq!(count(-1, -1, -1), 8, "at the opposite corner");
    }

    /// The sections a write marks are the ones `aroundAndAtBlockPos` names,
    /// and the write's own section is always among them.
    #[test]
    fn a_write_marks_its_own_section_and_the_ones_it_touches() {
        let mut seen = BTreeSet::new();
        around_and_at_block_pos(as_long(0, 0, 0), |s| {
            seen.insert(s);
        });
        assert!(seen.contains(&(0, 0, 0)));
        assert!(seen.contains(&(-1, -1, -1)));
        assert_eq!(seen.len(), 8);
    }

    /// `SectionState`'s two fields do not collide, and the neighbour count
    /// refuses to leave `[0, 26]` rather than wrapping into the data bit.
    #[test]
    fn section_state_packs_has_data_beside_the_neighbour_count() {
        let s = section_state::has_data(section_state::EMPTY, true);
        assert_eq!(section_state::neighbor_count(s), 0);
        let s = section_state::with_neighbor_count(s, 26);
        assert_eq!(section_state::neighbor_count(s), 26);
        // hasData survived the count going in beside it.
        assert_ne!(s & section_state::HAS_DATA_BIT, 0);
        let s = section_state::has_data(s, false);
        assert_eq!(section_state::neighbor_count(s), 26);
        assert_eq!(s & section_state::HAS_DATA_BIT, 0);
    }

    #[test]
    #[should_panic(expected = "Neighbor count was not within range")]
    fn a_neighbour_count_of_27_is_refused_not_wrapped() {
        let _ = section_state::with_neighbor_count(0, 27);
    }

    /// `repeatFirstLayer` copies the BOTTOM y-slice sixteen times, which is
    /// the first 256 nibbles — not the first 256 bytes and not the whole
    /// layer.
    #[test]
    fn repeat_first_layer_repeats_the_bottom_slice() {
        let mut d = vec![0u8; 4096];
        for (i, v) in d.iter_mut().enumerate() {
            *v = u8::try_from(i % 16).expect("0..16");
        }
        let out = repeat_first_layer(&d);
        for y in 0..16usize {
            assert_eq!(
                &out[y * 256..(y + 1) * 256],
                &d[0..256],
                "slice {y} is not a copy of the bottom one"
            );
        }
    }
}
