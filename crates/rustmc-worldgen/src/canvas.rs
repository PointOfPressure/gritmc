//! A block canvas spanning many chunks, which is what the decoration stage
//! actually needs.
//!
//! # Why a chunk is the wrong unit
//!
//! Every stage before decoration is per-chunk: noise, surface and carvers all
//! read and write one column at a time (carvers *read* a 17x17 neighbourhood
//! but only write their own chunk). Decoration is not. A feature placed near
//! a chunk's edge writes into its neighbours, and vanilla's `WorldGenRegion`
//! hands the feature a window that spans them — so a generator that finishes
//! one chunk before starting the next produces a world with seams, and it
//! produces a *different* world depending on the order it visited chunks in.
//!
//! ORACLE.md §6 is about exactly this: the vibenilla adopt was silently
//! *losing* border writes under parallel generation, which showed up as five
//! different parity numbers from five runs. So this canvas exists to make the
//! shared window explicit and the order a named constant rather than an
//! accident of scheduling.
//!
//! # Blocks are interned, not strings
//!
//! Every other module in this crate speaks block *states* as strings —
//! `minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]` —
//! because that is what the golden dumps carry and comparing anything less
//! specific is ORACLE.md rule 11's mistake. A string per block is fine for a
//! nine-chunk test world and ruinous for a region: 32x32 chunks at 384 high
//! is 100 million of them.
//!
//! So the canvas keeps one palette per chunk and a `u16` per block. The
//! strings at the boundary are unchanged; only the storage is different, and
//! [`Canvas::block_at`] hands back a `&str` into the palette rather than a
//! fresh allocation.
//!
//! # Heightmaps live here too
//!
//! The four heightmaps features read are re-primed immediately before
//! decoration and rise as features write, so they belong to the same
//! structure as the blocks. Keeping them anywhere else is how they drift.

use std::collections::BTreeMap;

/// One of the four heightmaps decoration reads and updates.
///
/// **Not** `OCEAN_FLOOR_WG` or `WORLD_SURFACE_WG`: those are fossils primed
/// when noise filled the chunk and never touched again, and they belong to
/// the chunk rather than to this canvas. These four are re-primed
/// immediately before features run and rise as features write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveHeightmap {
    WorldSurface,
    OceanFloor,
    MotionBlocking,
    MotionBlockingNoLeaves,
}

/// The four, in the order this module stores them.
pub const HEIGHTMAPS: [LiveHeightmap; 4] = [
    LiveHeightmap::WorldSurface,
    LiveHeightmap::OceanFloor,
    LiveHeightmap::MotionBlocking,
    LiveHeightmap::MotionBlockingNoLeaves,
];

/// Whether a block state counts towards a heightmap.
///
/// A trait rather than a table, for the reason `Cargo.toml` gives: this crate
/// asks block-behaviour questions and the caller answers them. The answers
/// live in `rustmc_behavior::block_props`, which is a *dev*-dependency here
/// and must stay one.
pub trait HeightmapPredicates {
    /// `Heightmap.Types.<map>.isOpaque().test(state)`.
    fn matches(&self, state: &str, map: LiveHeightmap) -> bool;
}

/// One chunk's blocks, palette-encoded.
#[derive(Clone, Debug)]
struct ChunkBlocks {
    /// State strings, in the order they were first written.
    palette: Vec<String>,
    /// `palette` index per block, `(y - min_y) * 256 + z * 16 + x`.
    ids: Vec<u16>,
    /// The four heightmaps, each 256 columns of first-available y.
    heightmaps: [Vec<i32>; 4],
    /// `WORLD_SURFACE_WG` and `OCEAN_FLOOR_WG`, the two the **noise stage**
    /// primes and nothing ever touches again.
    ///
    /// They are separate from the four live maps on purpose. A fossil that
    /// rose as features wrote would put the surface rules' `stone_depth` at
    /// the top of a tree, and the placement chains that name `OCEAN_FLOOR_WG`
    /// would follow the canopy rather than the ground.
    wg_heightmaps: [Vec<i32>; 2],
    /// Biome names, interned like the blocks.
    biome_palette: Vec<String>,
    /// One entry per 4x4x4 cell, indexed
    /// `(cell_y * 4 + cell_z) * 4 + cell_x` within a section and sections
    /// bottom-up — vanilla's own order, so the region writer can hand it
    /// straight over.
    biomes: Vec<u16>,
}

/// What the `_WG` heightmaps need to know about a block state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgFacts {
    /// `BlockState#isAir` — the `WORLD_SURFACE_WG` predicate is its negation.
    pub air: bool,
    /// `BlockState#blocksMotion` — the `OCEAN_FLOOR_WG` predicate.
    pub blocks_motion: bool,
}

/// The answer table the live `_WG` heightmaps consult, supplied from
/// outside because this crate deliberately carries no block-property table.
///
/// Held behind an `Arc` so a `Canvas` stays `Clone`, and given a hand-written
/// `Debug` because a closure has none.
#[derive(Clone)]
pub struct WgPredicates(std::sync::Arc<dyn Fn(&str) -> WgFacts + Send + Sync>);

impl WgPredicates {
    pub fn new(facts: impl Fn(&str) -> WgFacts + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(facts))
    }
}

impl std::fmt::Debug for WgPredicates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WgPredicates(..)")
    }
}

/// A counter a `&Canvas` can bump, without costing the canvas its `Sync`.
///
/// `Cell` would be cheaper and is what the first version used; it makes
/// `Canvas` `!Sync`, which the server's `World` — shared behind `&'static` —
/// needs. Relaxed atomics on a path that only runs when a chunk is *missing*
/// cost nothing measurable, and the reach probe behind them is env-gated.
#[derive(Debug, Default)]
struct Counter(std::sync::atomic::AtomicU64);

impl Clone for Counter {
    fn clone(&self) -> Self {
        Self(std::sync::atomic::AtomicU64::new(self.get()))
    }
}

impl Counter {
    fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn bump(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn add(&self, n: u64) {
        self.0.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    }
    fn raise_to(&self, n: u64) {
        self.0.fetch_max(n, std::sync::atomic::Ordering::Relaxed);
    }
}

/// `ProtoChunk`'s `postProcessing` lists, keyed by chunk rather than held as
/// one array per chunk object: chunk position to the marks made in it, in the
/// order they were made.
pub type PostProcessingMarks = BTreeMap<(i32, i32), Vec<(i32, i32, i32)>>;

/// A rectangle of chunks that stages write into.
#[derive(Clone, Debug)]
pub struct Canvas {
    min_y: i32,
    height: i32,
    /// Set while the `_WG` heightmaps are **live**, cleared once they are
    /// fossils. See [`Canvas::set_wg_live`].
    wg_live: Option<WgPredicates>,
    /// `ChunkAccess#markPosForPostProcessing` — positions whose block has to
    /// be re-derived once everything around it exists.
    ///
    /// **Keyed by chunk, and a `Vec` rather than a set, because vanilla's
    /// order is behaviour and its duplicates are behaviour.** `ProtoChunk`
    /// keeps `ShortList[] postProcessing`, one list per *section*, and
    /// `markPosForPostProcessing` does a bare `ShortArrayList.add` — it does
    /// not deduplicate, and `LevelChunk.postProcessGeneration` walks the
    /// sections in ascending index and each list in insertion order,
    /// **applying every write before it reads the next position**.
    ///
    /// That is not a detail. A marked position can be refilled by an earlier
    /// mark's fluid spread and then tick with the refilled state, which is
    /// how one pass builds a two-deep column of flowing water under a single
    /// source. A `BTreeSet<(x,y,z)>` — which this was — sorts by x then y
    /// ascending and collapses duplicates, so it could reproduce neither.
    ///
    /// The `Vec` is in insertion order; [`Self::post_processing`] applies the
    /// per-section grouping when it flattens, so the store stays cheap to
    /// write and the order is imposed once, where it is read.
    post_processing: PostProcessingMarks,
    /// Keyed by chunk position so the store is sparse — a generator working
    /// outward does not have to know its extent in advance.
    ///
    /// A `BTreeMap` rather than a `HashMap`: this is iterated to write
    /// regions, and CLAUDE.md's rule about map iteration at behaviour-visible
    /// sites applies to output order as much as to generation order.
    chunks: BTreeMap<(i32, i32), ChunkBlocks>,
    /// What an unwritten block is. Not `minecraft:air` by accident — a
    /// canvas is filled by the noise stage before anything reads it, and a
    /// read outside the written area is a bug this makes visible.
    void: String,
    /// Every read or write that named a chunk this canvas does not hold.
    ///
    /// It exists for [`Self::detach`]: a detached group is a canvas holding
    /// only the chunks one decorating chunk may touch, and the whole safety
    /// argument is that a feature never reaches further. If it did, the read
    /// would silently answer the void and the write would silently vanish —
    /// which is exactly the failure a byte comparison might or might not
    /// surface, depending on whether the void happened to be the right
    /// answer. So it is counted instead: the *sum* over the detached groups
    /// must equal the count a whole-canvas run of the same square produces,
    /// where the only misses are the true edge of the generated area.
    ///
    /// A [`Counter`], not a `Cell`: [`Self::block_at`] takes `&self`, and a
    /// `Cell` would take `Sync` away from `Canvas` — which the server's
    /// `World` needs, since it is shared behind a `&'static`.
    outside: Counter,
    /// The chunk whose decoration is running, when one is.
    ///
    /// Set by the decoration driver for **both** paths, so the reach probe
    /// below measures the same thing sequentially and in parallel.
    home: Option<(i32, i32)>,
    /// `RUSTMC_READ_REACH=1` — record how far, in chunks, an access strays
    /// from [`Self::home`].
    ///
    /// This is the measurement the parallel decoration pass rests on and it
    /// is deliberately not an argument from the code: a feature is supposed to
    /// reach one chunk out, vanilla's `WorldGenRegion` refuses a *write*
    /// beyond that, and nothing bounds a **read**. Rule 9 — so it is counted.
    reach_probe: bool,
    /// `RUSTMC_READ_REACH_TRAP=<n>` — panic on the first access further than
    /// `n` chunks from the decorating chunk, so the stack names the call site.
    reach_trap: Option<i32>,
    /// The largest Chebyshev chunk distance from `home` any access has used.
    max_reach: Counter,
    /// Every accepted write, in the order it was made, while recording.
    ///
    /// This is the isolated decoration pass's output — see
    /// [`Self::record_writes`]. A chunk decorating into a canvas of its own
    /// produces a *write list*, and the composed world is those lists
    /// replayed onto one canvas in the canonical chunk order. The list is
    /// what makes the composition exact rather than a diff: a write that
    /// changes nothing still counts, because a later write from another
    /// chunk has to land on top of it in the right order, and a diff cannot
    /// see it at all.
    write_log: Option<Vec<(i32, i32, i32, String)>>,
    /// The block-entity side channel — what a placer knew at a position that
    /// the finished block states do not say.
    ///
    /// `docs/DETERMINISM.md` §42. `crate::block_entity::for_block` derives a
    /// payload from the final block state and is exact for the one class that
    /// is a function of it; the other 141 positions of 173 on the census
    /// window were written by whatever placed the block, and this is where
    /// that is carried out of generation.
    ///
    /// **`None` is not recording, and that is the safety property rather than
    /// an optimisation.** The channel is a write path out of a generation pass
    /// and stays sound only while there is no read path back in: nothing on
    /// this canvas exposes what the channel holds to a feature, and
    /// [`Self::record_block_entity`] is the only way in. Switching it off must
    /// therefore change nothing about the world, which is checkable rather
    /// than assertable — see `ci/be-channel-identical.sh`.
    ///
    /// An `Option` rather than a bare map for the same reason
    /// [`Self::write_log`] is one: a `Some(empty)` says "recording, nothing
    /// recorded" and a `None` says "not recording", and the emitter must be
    /// able to tell those apart or a channel nobody switched on reads as a
    /// world with no block entities in it.
    block_entities: Option<crate::block_entity::Records>,
    /// The entities generation adds — a structure's mobs, an end city ship's
    /// item frame — as the compounds `entities/r.X.Z.mca` saves, keyed by the
    /// chunk the entity stands in. Recorded whenever [`Self::block_entities`]
    /// is, and carried by every path that carries it.
    entities: Option<EntityRecords>,
}

/// Generated entities, keyed by chunk, in the order they were added.
pub type EntityRecords = BTreeMap<(i32, i32), Vec<rustmc_world::nbt::Nbt>>;

impl Canvas {
    /// An empty canvas for a world of the given vertical extent.
    #[must_use]
    pub fn new(min_y: i32, height: i32) -> Self {
        Self {
            min_y,
            height,
            chunks: BTreeMap::new(),
            void: "minecraft:air".to_string(),
            wg_live: None,
            post_processing: PostProcessingMarks::new(),
            outside: Counter::default(),
            home: None,
            reach_probe: std::env::var_os("RUSTMC_READ_REACH").is_some(),
            reach_trap: std::env::var("RUSTMC_READ_REACH_TRAP")
                .ok()
                .and_then(|v| v.parse().ok()),
            max_reach: Counter::default(),
            write_log: None,
            block_entities: None,
            entities: None,
        }
    }

    /// Start recording every accepted write, discarding any earlier log.
    ///
    /// The isolated decoration pass turns this on for one chunk's canvas,
    /// decorates, and takes the log back out with [`Self::take_write_log`].
    /// Nothing else in the crate records: the cost is a `String` per write
    /// and it is paid only where the log is the output.
    pub fn record_writes(&mut self) {
        self.write_log = Some(Vec::new());
    }

    /// Take the recorded writes and stop recording.
    ///
    /// `None` if [`Self::record_writes`] was never called, which is a
    /// different statement from an empty log: a chunk that decorated and
    /// wrote nothing returns `Some(vec![])`.
    #[must_use]
    pub fn take_write_log(&mut self) -> Option<Vec<(i32, i32, i32, String)>> {
        self.write_log.take()
    }

    /// Replay a recorded write list onto this canvas, in order.
    ///
    /// This is the composition half of the isolated decoration pass. Writes
    /// go through [`Self::set_block`], so the live heightmaps and the `_WG`
    /// maps see exactly the sequence a whole-canvas run would have shown
    /// them — the isolation changes what each chunk *decides* to write, not
    /// how the writes land.
    pub fn replay_writes(&mut self, writes: &[(i32, i32, i32, String)]) {
        for (x, y, z, state) in writes {
            self.set_block(*x, *y, *z, state);
        }
    }

    /// A canvas holding **copies** of the named chunks.
    ///
    /// [`Self::detach`] moves chunks out; this copies them, which is what an
    /// isolated decoration needs: the chunk it decorates and every chunk it
    /// may read have to stay in the canvas underneath, because the next
    /// chunk's isolated decoration must see them in the same
    /// pre-decoration state.
    ///
    /// Positions this canvas does not hold are skipped rather than
    /// fabricated, exactly as `detach` skips them — but the isolated pass is
    /// arranged so that never happens inside the delivered area, and that is
    /// the whole of why it is extent-independent. See
    /// [`crate::pipeline::Pipeline::decorated`].
    #[must_use]
    pub fn clone_region(&self, positions: impl IntoIterator<Item = (i32, i32)>) -> Self {
        let mut chunks = BTreeMap::new();
        for position in positions {
            if let Some(blocks) = self.chunks.get(&position) {
                chunks.insert(position, blocks.clone());
            }
        }
        Self {
            min_y: self.min_y,
            height: self.height,
            wg_live: self.wg_live.clone(),
            post_processing: PostProcessingMarks::new(),
            chunks,
            void: self.void.clone(),
            outside: Counter::default(),
            home: None,
            reach_probe: self.reach_probe,
            reach_trap: self.reach_trap,
            max_reach: Counter::default(),
            write_log: None,
            // Inherit whether the parent is recording, never what it
            // recorded. A group that did not inherit the switch would drop
            // every payload its own chunk placed, silently and only in the
            // parallel path — the one difference between the two paths that
            // `ci/be-channel-identical.sh` could not see, because it compares
            // block states and this would move none.
            block_entities: self
                .block_entities
                .as_ref()
                .map(|_| crate::block_entity::Records::new()),
            entities: self.entities.as_ref().map(|_| EntityRecords::new()),
        }
    }

    /// The post-processing marks made while decorating, to be merged into
    /// the composed canvas in the same order the write lists are replayed.
    #[must_use]
    pub fn take_post_processing(&mut self) -> PostProcessingMarks {
        std::mem::take(&mut self.post_processing)
    }

    /// Merge marks taken from an isolated decoration back in.
    pub fn absorb_post_processing(&mut self, marks: PostProcessingMarks) {
        for (position, entries) in marks {
            self.post_processing
                .entry(position)
                .or_default()
                .extend(entries);
        }
    }

    /// Start recording the block-entity side channel.
    ///
    /// Opt-in, per the standing rule that new behaviour lands off until it is
    /// measured, and opt-in for a second reason here: the whole gate on this
    /// channel is that the world generates byte-identically with recording on
    /// and off, and a switch is what makes that comparison possible at all.
    ///
    /// Discards anything already recorded, as [`Self::record_writes`] does, so
    /// a caller cannot accidentally accumulate two passes into one channel.
    pub fn record_block_entities(&mut self) {
        self.block_entities = Some(crate::block_entity::Records::new());
        self.entities = Some(EntityRecords::new());
    }

    /// Record an entity generation added, in the chunk it stands in. A no-op
    /// when the channel is off.
    pub fn record_entity(&mut self, chunk_x: i32, chunk_z: i32, entity: rustmc_world::nbt::Nbt) {
        if let Some(records) = self.entities.as_mut() {
            records.entry((chunk_x, chunk_z)).or_default().push(entity);
        }
    }

    /// The entities recorded in one chunk, in the order they were added.
    #[must_use]
    pub fn entity_records(&self, chunk_x: i32, chunk_z: i32) -> &[rustmc_world::nbt::Nbt] {
        self.entities
            .as_ref()
            .and_then(|records| records.get(&(chunk_x, chunk_z)))
            .map_or(&[], Vec::as_slice)
    }

    /// Take the recorded entities, leaving the channel on and empty.
    #[must_use]
    pub fn take_entities(&mut self) -> EntityRecords {
        self.entities
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Merge entities taken from an isolated decoration back in, appending
    /// in call order.
    pub fn absorb_entities(&mut self, records: EntityRecords) {
        let Some(mine) = self.entities.as_mut() else {
            assert!(
                records.is_empty(),
                "absorbing {} chunk(s) of entities into a canvas that is not recording",
                records.len()
            );
            return;
        };
        for (position, entries) in records {
            mine.entry(position).or_default().extend(entries);
        }
    }

    /// Whether the channel is recording.
    ///
    /// Read by nothing in generation — see the field's own note about there
    /// being no read path back in. It exists for the gate and for the
    /// emitter, which has to tell "not recording" from "recorded nothing".
    #[must_use]
    pub const fn recording_block_entities(&self) -> bool {
        self.block_entities.is_some()
    }

    /// Record what a placer knew at a position.
    ///
    /// The only way into the channel. A no-op when recording is off, and a
    /// no-op that costs the caller nothing to write: a producer calls this
    /// unconditionally, so the code path a feature takes is the same either
    /// way and the only difference between recording and not is whether the
    /// payload is stored. If the call were behind a producer-side `if`, the
    /// two paths would differ in shape and the identity gate would be
    /// comparing two different programs.
    pub fn record_block_entity(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        record: crate::block_entity::Record,
    ) {
        if let Some(records) = self.block_entities.as_mut() {
            records
                .entry((x >> 4, z >> 4))
                .or_default()
                .push(((x, y, z), record));
        }
    }

    /// The channel's records for one chunk, in the order they were made.
    ///
    /// Empty when recording is off, which is why the emitter is handed this
    /// rather than the `Option`: a column built from a canvas that never
    /// recorded carries the block entities the block states alone can answer
    /// for, which is exactly what it carried before this channel existed.
    #[must_use]
    pub fn block_entity_records(
        &self,
        chunk_x: i32,
        chunk_z: i32,
    ) -> &[((i32, i32, i32), crate::block_entity::Record)] {
        self.block_entities
            .as_ref()
            .and_then(|records| records.get(&(chunk_x, chunk_z)))
            .map_or(&[], Vec::as_slice)
    }

    /// The records made while decorating, to be merged into the composed
    /// canvas in the same order the write lists are replayed.
    ///
    /// Leaves the channel recording and empty rather than switched off: a
    /// group whose records were taken is still a group that must record.
    #[must_use]
    pub fn take_block_entities(&mut self) -> crate::block_entity::Records {
        self.block_entities
            .as_mut()
            .map(std::mem::take)
            .unwrap_or_default()
    }

    /// Merge records taken from an isolated decoration back in.
    ///
    /// **Order matters here where it does not for post-processing marks.** A
    /// position can be recorded twice and the last record is the one saved,
    /// because the last block written at that position is the one the save
    /// sees. So this appends in call order, and the caller replays groups in
    /// decoration order — the same order it replays writes in, for the same
    /// reason.
    pub fn absorb_block_entities(&mut self, records: crate::block_entity::Records) {
        let Some(mine) = self.block_entities.as_mut() else {
            // Absorbing into a canvas that is not recording would silently
            // discard a group's whole channel. A caller that switched
            // recording on for the group and not for the composed canvas has
            // a bug the gate cannot see, since it moves no block state.
            assert!(
                records.is_empty(),
                "absorbing {} chunk(s) of block-entity records into a canvas that is not recording",
                records.len()
            );
            return;
        };
        for (position, entries) in records {
            mine.entry(position).or_default().extend(entries);
        }
    }

    /// Name the chunk whose decoration is running, for the reach probe.
    pub const fn set_home(&mut self, chunk_x: i32, chunk_z: i32) {
        self.home = Some((chunk_x, chunk_z));
    }

    /// The largest Chebyshev chunk distance from the decorating chunk that any
    /// access has reached, or 0 when `RUSTMC_READ_REACH` is unset.
    #[must_use]
    pub fn max_reach(&self) -> i32 {
        i32::try_from(self.max_reach.get()).unwrap_or(i32::MAX)
    }

    /// Record how far this access strayed from the decorating chunk.
    ///
    /// `RUSTMC_READ_REACH_TRAP=<n>` panics on the first access further than
    /// `n` chunks, which with `RUST_BACKTRACE=1` names the call site. A
    /// maximum says how far something reached and never says what; this
    /// project's standing rule is that a reading of code does not move a
    /// suspect, and a stack does.
    fn note_reach(&self, x: i32, z: i32) {
        if !self.reach_probe {
            return;
        }
        let Some((cx, cz)) = self.home else { return };
        let d = ((x >> 4) - cx).abs().max(((z >> 4) - cz).abs());
        self.max_reach.raise_to(u64::try_from(d).unwrap_or(0));
        if self.reach_trap.is_some_and(|limit| d > limit) {
            let limit = self.reach_trap.unwrap_or_default();
            panic!(
                "read reach {d} > {limit}: decorating chunk ({cx},{cz}) \
                 reached block ({x},{z}) in chunk ({},{})",
                x >> 4,
                z >> 4
            );
        }
    }

    /// How many reads and writes named a chunk this canvas does not hold.
    /// See the field's documentation — this is the instrument that says a
    /// detached group was big enough.
    #[must_use]
    pub fn outside_accesses(&self) -> u64 {
        self.outside.get()
    }

    /// Move a named set of chunks out into a canvas of their own.
    ///
    /// The parallel decoration pass is built on this. Two chunks whose
    /// decoration cannot touch each other's blocks can be decorated at the
    /// same time, and the cheapest way to say that to the borrow checker is
    /// to give each one a canvas holding exactly the chunks it may reach.
    /// Nothing is copied: a `ChunkBlocks` is moved, which is its `Vec`
    /// headers and nothing else.
    ///
    /// Positions this canvas does not hold are skipped rather than
    /// fabricated, so a group at the edge of the generated area sees the same
    /// void a whole-canvas run sees there.
    ///
    /// The detached canvas inherits `min_y`, `height`, `void` and `wg_live`,
    /// because a stage that detaches must see the same world the stage that
    /// did not would have. It does **not** inherit `post_processing`: that
    /// set is write-only during decoration, so each group accumulates its own
    /// and [`Self::merge`] unions them back.
    #[must_use]
    pub fn detach(&mut self, positions: impl IntoIterator<Item = (i32, i32)>) -> Self {
        let mut chunks = BTreeMap::new();
        for position in positions {
            if let Some(blocks) = self.chunks.remove(&position) {
                chunks.insert(position, blocks);
            }
        }
        Self {
            min_y: self.min_y,
            height: self.height,
            wg_live: self.wg_live.clone(),
            post_processing: PostProcessingMarks::new(),
            chunks,
            void: self.void.clone(),
            outside: Counter::default(),
            home: None,
            reach_probe: self.reach_probe,
            reach_trap: self.reach_trap,
            max_reach: Counter::default(),
            write_log: None,
            // Inherit whether the parent is recording, never what it
            // recorded. A group that did not inherit the switch would drop
            // every payload its own chunk placed, silently and only in the
            // parallel path — the one difference between the two paths that
            // `ci/be-channel-identical.sh` could not see, because it compares
            // block states and this would move none.
            block_entities: self
                .block_entities
                .as_ref()
                .map(|_| crate::block_entity::Records::new()),
            entities: self.entities.as_ref().map(|_| EntityRecords::new()),
        }
    }

    /// Put a [`Self::detach`]ed group back.
    ///
    /// The chunk moves back; the group's `post_processing` marks are unioned
    /// in, which is order-independent because it is a set; the group's
    /// outside-access count is added to this canvas's, so the total over a
    /// pass is comparable with a whole-canvas run's.
    ///
    /// # Panics
    /// If a chunk being merged back is already present — which would mean two
    /// groups claimed it and the disjointness argument is wrong.
    pub fn merge(&mut self, group: Self) {
        for (position, blocks) in group.chunks {
            assert!(
                self.chunks.insert(position, blocks).is_none(),
                "two decoration groups both held chunk {position:?}"
            );
        }
        // Appended per chunk rather than unioned, because the marks are a
        // list now. Groups hold disjoint chunks and a group only marks inside
        // its own write radius, so in practice each key comes from one group;
        // appending rather than asserting keeps a stray shared key
        // deterministic under the merge order instead of panicking.
        for (position, marks) in group.post_processing {
            self.post_processing
                .entry(position)
                .or_default()
                .extend(marks);
        }
        // The side channel comes back the same way, and it has to come back
        // here rather than only in the compose path: `detach`/`merge` is the
        // other isolation route, and a group whose records were dropped on
        // merge would produce a world whose block states are right and whose
        // block entities are missing — which no block diff can see.
        if let Some(records) = group.block_entities {
            self.absorb_block_entities(records);
        }
        if let Some(records) = group.entities {
            self.absorb_entities(records);
        }
        self.outside.add(group.outside.get());
        self.max_reach.raise_to(group.max_reach.get());
    }

    /// `ChunkAccess#markPosForPostProcessing` — an append, not an insert.
    /// Vanilla does not deduplicate and a position marked twice ticks twice.
    pub fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        self.post_processing
            .entry((x >> 4, z >> 4))
            .or_default()
            .push((x, y, z));
    }

    /// The marked positions in the order `LevelChunk#postProcessGeneration`
    /// visits them: chunk by chunk, and inside a chunk by ascending **section
    /// index** with insertion order preserved inside each section.
    ///
    /// The sort is `sort_by_key`, which is stable, so grouping by section
    /// cannot disturb the order within one — that stability is the whole
    /// point and is not an implementation accident.
    ///
    /// **The order between chunks is ours, not vanilla's.** Vanilla runs this
    /// pass once per chunk, when that chunk is promoted to FULL, so its
    /// between-chunk order is whatever order the server promoted them in —
    /// unobservable from a saved region, and order noise of exactly the kind
    /// `ci/decorate-order-intersect.sh` exists to subtract. We use the
    /// canvas's own key order and say so rather than implying it is derived.
    #[must_use]
    pub fn post_processing(&self) -> Vec<(i32, i32, i32)> {
        let mut out = Vec::new();
        for marks in self.post_processing.values() {
            let start = out.len();
            out.extend_from_slice(marks);
            out[start..].sort_by_key(|&(_, y, _)| y >> 4);
        }
        out
    }

    /// Keep the two `_WG` heightmaps **live**: every write updates them, as
    /// `ProtoChunk#setBlockState` does.
    ///
    /// This is not an optimisation switch, it is the stage boundary.
    /// `ProtoChunk#setBlockState` updates whatever
    /// `getPersistedStatus().heightmapsAfter()` names, and the persisted
    /// status lags the running step by one:
    ///
    /// ```text
    ///   running SURFACE  -> persisted NOISE    -> WORLDGEN_HEIGHTMAPS
    ///   running CARVERS  -> persisted SURFACE  -> WORLDGEN_HEIGHTMAPS
    ///   running FEATURES -> persisted CARVERS  -> FINAL_HEIGHTMAPS
    /// ```
    ///
    /// where `WORLDGEN_HEIGHTMAPS` is exactly `{OCEAN_FLOOR_WG,
    /// WORLD_SURFACE_WG}`. So the `_WG` maps track surface **and carving**,
    /// and only stop moving when decoration starts — see [`Self::freeze_wg`].
    ///
    /// Getting this wrong is invisible in the blocks and loud in the
    /// features: a carver that lowers the sea floor by four blocks without
    /// lowering `OCEAN_FLOOR_WG` leaves every `heightmap`-anchored placement
    /// hanging in the water above it.
    pub fn set_wg_live(&mut self, predicates: WgPredicates) {
        self.wg_live = Some(predicates);
    }

    /// Stop updating the `_WG` heightmaps — what vanilla does by moving on
    /// to a status whose `heightmapsAfter` no longer names them.
    pub fn freeze_wg(&mut self) {
        self.wg_live = None;
    }

    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// `getMaxY()` — the **inclusive** top.
    #[must_use]
    pub const fn max_y(&self) -> i32 {
        self.min_y + self.height - 1
    }

    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Whether this chunk has been started.
    #[must_use]
    pub fn has_chunk(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.chunks.contains_key(&(chunk_x, chunk_z))
    }

    /// Every chunk position on the canvas, in x-then-z order.
    pub fn positions(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.chunks.keys().copied()
    }

    /// Make room for a chunk, filled with `fill`.
    ///
    /// Idempotent: a second call for the same chunk leaves it alone rather
    /// than wiping what a previous stage wrote, which is the failure mode of
    /// a pipeline that runs a stage twice.
    pub fn ensure_chunk(&mut self, chunk_x: i32, chunk_z: i32, fill: &str) {
        let height = usize::try_from(self.height).expect("a positive height");
        self.chunks
            .entry((chunk_x, chunk_z))
            .or_insert_with(|| ChunkBlocks {
                palette: vec![fill.to_string()],
                ids: vec![0; height * 256],
                heightmaps: [
                    vec![self.min_y; 256],
                    vec![self.min_y; 256],
                    vec![self.min_y; 256],
                    vec![self.min_y; 256],
                ],
                wg_heightmaps: [vec![self.min_y; 256], vec![self.min_y; 256]],
                biome_palette: vec!["minecraft:plains".to_string()],
                biomes: vec![0; height / 4 * 16],
            });
    }

    /// The index of a block within its chunk, or `None` outside the world's
    /// vertical extent.
    fn index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        let local = usize::try_from((y - self.min_y) * 256 + (z & 15) * 16 + (x & 15))
            .expect("a positive index");
        Some(local)
    }

    /// The state at a position. Outside a written chunk this is the void
    /// block, because a stage reading there is asking about a chunk that has
    /// not been generated.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> &str {
        self.note_reach(x, z);
        let state = 'state: {
            let Some(index) = self.index(x, y, z) else {
                break 'state &self.void;
            };
            let Some(chunk) = self.chunks.get(&(x >> 4, z >> 4)) else {
                self.outside.bump();
                break 'state &self.void;
            };
            &chunk.palette[chunk.ids[index] as usize]
        };
        trace_read(x, y, z, state);
        state
    }

    /// Write a block, interning its state.
    ///
    /// Returns whether the block changed, which is what a write log wants —
    /// vanilla's own decoration dumps record a *difference*, so a write that
    /// changes nothing is invisible to them and must be to us.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) -> bool {
        trace_write(x, y, z, state);
        trace_feature(x, y, z, state);
        self.note_reach(x, z);
        let Some(index) = self.index(x, y, z) else {
            return false;
        };
        let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) else {
            self.outside.bump();
            return false;
        };
        let id = match chunk.palette.iter().position(|p| p == state) {
            Some(id) => id,
            None => {
                chunk.palette.push(state.to_string());
                chunk.palette.len() - 1
            }
        };
        let id = u16::try_from(id).expect("a chunk cannot hold 65,536 distinct states");
        let changed = chunk.ids[index] != id;
        chunk.ids[index] = id;
        // After the two rejections above, not before: a write outside the
        // canvas's vertical range or into a chunk it does not hold is a
        // write that did not happen, and replaying it onto a canvas that
        // *does* hold that chunk would fabricate a block the isolated run
        // never wrote.
        if let Some(log) = self.write_log.as_mut() {
            log.push((x, y, z, state.to_string()));
        }
        // Disjoint field borrows: `wg_live` immutably, `chunks` mutably.
        // That is the whole reason `update_wg` is an associated function
        // rather than a `&mut self` method — see its doc comment.
        if let Some(predicates) = self.wg_live.as_ref() {
            Self::update_wg(
                &mut self.chunks,
                predicates,
                self.min_y,
                self.height,
                &self.void,
                x,
                y,
                z,
                state,
            );
        }
        changed
    }

    /// `Heightmap#update` for the two `_WG` maps.
    ///
    /// The shape is vanilla's and it is not simply "raise if higher". When
    /// the block being replaced *was* the top one and the new block does not
    /// match, the map has to walk **down** to find the next one that does —
    /// which is the whole reason carving lowers it rather than leaving it
    /// stranded.
    ///
    /// **An associated function over the fields, not a `&mut self` method.**
    /// It used to open with `self.wg_live.clone()` — an `Arc` refcount bump
    /// per block written — because a borrow of `self.wg_live` cannot coexist
    /// with the `&mut self` this needed. Taking the two fields separately
    /// makes the borrows disjoint and the clone unnecessary. The contended
    /// form of that same clone in `ChunkWriter` was the whole of the parallel
    /// stages' CPU tax (`78f8035`); this is its sequential twin, on the path
    /// the carvers and decoration write through.
    #[allow(clippy::too_many_arguments)]
    fn update_wg(
        chunks: &mut BTreeMap<(i32, i32), ChunkBlocks>,
        predicates: &WgPredicates,
        min_y: i32,
        height: i32,
        void: &str,
        x: i32,
        y: i32,
        z: i32,
        state: &str,
    ) {
        let facts = (predicates.0)(state);
        let column = ((z & 15) * 16 + (x & 15)) as usize;
        for (slot, matches) in [(0usize, !facts.air), (1usize, facts.blocks_motion)] {
            let Some(chunk) = chunks.get(&(x >> 4, z >> 4)) else {
                return;
            };
            let current = chunk.wg_heightmaps[slot][column];
            let new = if matches {
                // `if (y >= firstAvailable) setHeight(y + 1)`.
                if y >= current {
                    Some(y + 1)
                } else {
                    None
                }
            } else if current - 1 == y {
                // The top block was removed: walk down for the next match,
                // and bottom out at `min_y` if there is none.
                let mut found = min_y;
                let mut probe = y - 1;
                while probe >= min_y {
                    // `Canvas::block_at` by hand, on the borrow we hold.
                    // Same answer, including the void: `probe` is inside the
                    // vertical extent by the loop condition, and the chunk
                    // lookup is the same one, so the only way this differs is
                    // if the chunk is missing — and then both give the void.
                    let below = match chunks.get(&(x >> 4, z >> 4)) {
                        Some(chunk) if probe < min_y + height => {
                            let index =
                                usize::try_from((probe - min_y) * 256 + (z & 15) * 16 + (x & 15))
                                    .expect("a positive index");
                            &chunk.palette[chunk.ids[index] as usize]
                        }
                        _ => void,
                    };
                    let facts = (predicates.0)(below);
                    let hit = if slot == 0 {
                        !facts.air
                    } else {
                        facts.blocks_motion
                    };
                    if hit {
                        found = probe + 1;
                        break;
                    }
                    probe -= 1;
                }
                Some(found)
            } else {
                None
            };
            if let Some(new) = new {
                if let Some(chunk) = chunks.get_mut(&(x >> 4, z >> 4)) {
                    chunk.wg_heightmaps[slot][column] = new;
                }
            }
        }
    }

    /// `Heightmap#getFirstAvailable` — one past the highest block matching
    /// the map's predicate.
    #[must_use]
    pub fn height_of(&self, map: LiveHeightmap, x: i32, z: i32) -> i32 {
        self.note_reach(x, z);
        let Some(chunk) = self.chunks.get(&(x >> 4, z >> 4)) else {
            self.outside.bump();
            return self.min_y;
        };
        let column = usize::try_from((z & 15) * 16 + (x & 15)).expect("a positive index");
        let slot = HEIGHTMAPS
            .iter()
            .position(|m| *m == map)
            .expect("one of the four maps this canvas tracks");
        chunk.heightmaps[slot][column]
    }

    /// `Heightmap.primeHeightmaps` — scan every column from the top and
    /// record the first y at which each predicate holds, plus one.
    ///
    /// Run once per chunk immediately before its features, which is where
    /// `generateFeatures` runs it. Not after each stage: the *point* is that
    /// decoration starts from the carved world and then tracks its own
    /// writes.
    pub fn prime_heightmaps<P: HeightmapPredicates + ?Sized>(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        props: &P,
    ) {
        let min_y = self.min_y;
        let height = self.height;
        let Some(chunk) = self.chunks.get_mut(&(chunk_x, chunk_z)) else {
            return;
        };
        for slot in 0..4 {
            chunk.heightmaps[slot].fill(min_y);
        }
        // `props.matches` answered per *block* was 4.3% of a whole pipeline
        // (`perf`, 2026-07-28) — it is a string-keyed lookup, and this walk
        // asks it about `minecraft:air` a few hundred times per column on the
        // way down to the surface. The answer depends only on the block state
        // and the map, and a chunk's palette is tens of entries, so it is
        // memoised per `(palette id, map)`.
        //
        // Lazily, per slot, rather than precomputed for the whole palette:
        // that way the set of `(state, map)` pairs this asks about is exactly
        // the set it asked about before — never a superset — so a
        // `HeightmapPredicates` implementation that is partial, or that
        // panics on a state this chunk happens not to reach below the
        // heightmap, behaves the way it did. Strictly fewer calls, identical
        // answers, identical heightmaps.
        let mut cache: Vec<[Option<bool>; 4]> = vec![[None; 4]; chunk.palette.len()];
        for column in 0..256usize {
            let mut done = [false; 4];
            for y in (min_y..min_y + height).rev() {
                let index = usize::try_from(y - min_y).expect("a positive index") * 256 + column;
                let id = chunk.ids[index] as usize;
                for (slot, map) in HEIGHTMAPS.into_iter().enumerate() {
                    if done[slot] {
                        continue;
                    }
                    let answer = match cache[id][slot] {
                        Some(answer) => answer,
                        None => {
                            let answer = props.matches(&chunk.palette[id], map);
                            cache[id][slot] = Some(answer);
                            answer
                        }
                    };
                    if answer {
                        chunk.heightmaps[slot][column] = y + 1;
                        done[slot] = true;
                    }
                }
                if done.iter().all(|d| *d) {
                    break;
                }
            }
        }
    }

    /// `Heightmap#update`, reduced to the raising case.
    ///
    /// A feature only ever writes, so a block that matches a predicate can
    /// only raise that map. A block that *stops* matching is not something
    /// the decoration stage produces — vanilla's own `Heightmap.update` can
    /// lower a column by rescanning, and the branch is unreachable here.
    pub fn update_heightmaps<P: HeightmapPredicates + ?Sized>(
        &mut self,
        x: i32,
        y: i32,
        z: i32,
        props: &P,
    ) {
        if !self.has_chunk(x >> 4, z >> 4) {
            return;
        }
        let state = self.block_at(x, y, z).to_string();
        let column = usize::try_from((z & 15) * 16 + (x & 15)).expect("a positive index");

        for (slot, map) in HEIGHTMAPS.into_iter().enumerate() {
            let current = self.chunks[&(x >> 4, z >> 4)].heightmaps[slot][column];
            let new = if props.matches(&state, map) {
                (y >= current).then_some(y + 1)
            } else if current - 1 == y {
                // The block that *was* the top no longer matches, so the map
                // walks down for the next one that does. Vanilla does this in
                // `Heightmap#update`; a raise-only port leaves the map
                // stranded above a lake a feature has just dug, and every
                // later feature anchored to that map follows it up there.
                Some(self.first_below(x, y, z, map, props))
            } else {
                None
            };
            if let Some(new) = new {
                if let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) {
                    chunk.heightmaps[slot][column] = new;
                }
            }
        }
    }

    /// One past the highest block strictly below `y` that matches, or
    /// `min_y` when the column has none — `Heightmap#update`'s inner loop.
    fn first_below<P: HeightmapPredicates + ?Sized>(
        &self,
        x: i32,
        y: i32,
        z: i32,
        map: LiveHeightmap,
        props: &P,
    ) -> i32 {
        let mut probe = y - 1;
        while probe >= self.min_y {
            if props.matches(self.block_at(x, probe, z), map) {
                return probe + 1;
            }
            probe -= 1;
        }
        self.min_y
    }

    /// Record the two `_WG` heightmaps for a chunk. The noise stage's job,
    /// and nobody else's.
    pub fn set_wg_heightmaps(
        &mut self,
        chunk_x: i32,
        chunk_z: i32,
        world_surface: &[i32; 256],
        ocean_floor: &[i32; 256],
    ) {
        let Some(chunk) = self.chunks.get_mut(&(chunk_x, chunk_z)) else {
            return;
        };
        chunk.wg_heightmaps[0].copy_from_slice(world_surface);
        chunk.wg_heightmaps[1].copy_from_slice(ocean_floor);
    }

    /// `WORLD_SURFACE_WG` at a column.
    #[must_use]
    pub fn world_surface_wg(&self, x: i32, z: i32) -> i32 {
        self.wg_height(0, x, z)
    }

    /// `OCEAN_FLOOR_WG` at a column — the fossil the ore chains read.
    #[must_use]
    pub fn ocean_floor_wg(&self, x: i32, z: i32) -> i32 {
        self.wg_height(1, x, z)
    }

    fn wg_height(&self, slot: usize, x: i32, z: i32) -> i32 {
        self.note_reach(x, z);
        let Some(chunk) = self.chunks.get(&(x >> 4, z >> 4)) else {
            self.outside.bump();
            return self.min_y;
        };
        let column = usize::try_from((z & 15) * 16 + (x & 15)).expect("a positive index");
        chunk.wg_heightmaps[slot][column]
    }

    /// One chunk's `WORLD_SURFACE_WG` column heights, which is what the
    /// surface stage's oracle wants.
    #[must_use]
    pub fn world_surface_wg_chunk(&self, chunk_x: i32, chunk_z: i32) -> Option<[i32; 256]> {
        let chunk = self.chunks.get(&(chunk_x, chunk_z))?;
        let mut out = [self.min_y; 256];
        out.copy_from_slice(&chunk.wg_heightmaps[0]);
        Some(out)
    }

    /// One [`ChunkWriter`] per chunk on the canvas, in `positions()` order.
    ///
    /// This is the seam a worker pool runs a chunk-local stage through. The
    /// borrows are provably disjoint — `BTreeMap::values_mut` hands out one
    /// `&mut` per entry and no two of them alias — so the writers can be
    /// split across threads without a lock and without `unsafe`.
    ///
    /// # The live `_WG` heightmaps are **not** an obstacle here
    ///
    /// This used to assert `wg_live.is_none()`, on the written-down reason
    /// that a live update is "a read-modify-write of a **neighbour's**
    /// column". That was inferred, not read: [`Canvas::update_wg`] indexes
    /// `self.chunks[(x >> 4, z >> 4)]` — the chunk that owns the block being
    /// written — and its downward walk reads `self.block_at(x, probe, z)`,
    /// the same column. It never touches a neighbour. So a writer can carry
    /// the predicates and keep its own chunk's maps live, which is what
    /// [`ChunkWriter::set_block`] does.
    pub fn chunk_writers(&mut self) -> Vec<ChunkWriter<'_>> {
        let Self {
            min_y,
            height,
            wg_live,
            chunks,
            void,
            ..
        } = self;
        let (min_y, height) = (*min_y, *height);
        // Borrowed once for the whole set. `wg_live` and `chunks` are
        // disjoint fields, so an immutable borrow of one coexists with the
        // `iter_mut` over the other, and no writer touches a refcount.
        let wg = wg_live.as_ref();
        let void: &str = void;
        chunks
            .iter_mut()
            .map(|(&(chunk_x, chunk_z), blocks)| ChunkWriter {
                min_y,
                height,
                chunk_x,
                chunk_z,
                blocks,
                wg,
                void,
                post_processing: Vec::new(),
            })
            .collect()
    }

    /// The index of a 4x4x4 biome cell within its chunk.
    ///
    /// Cells are `y`-major over the whole column, then `z`, then `x` — the
    /// same order `to_chunk_column` reads them back in, which is the only
    /// reason it is spelled here rather than derived at the boundary.
    fn biome_index(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        if y < self.min_y || y >= self.min_y + self.height {
            return None;
        }
        let cell_y = (y - self.min_y) >> 2;
        let cell_z = (z & 15) >> 2;
        let cell_x = (x & 15) >> 2;
        Some(usize::try_from(cell_y * 16 + cell_z * 4 + cell_x).expect("a positive index"))
    }

    /// The biome at a block position, resolved through its 4x4x4 cell.
    #[must_use]
    pub fn biome_at(&self, x: i32, y: i32, z: i32) -> &str {
        let Some(index) = self.biome_index(x, y, z) else {
            return &self.void;
        };
        let Some(chunk) = self.chunks.get(&(x >> 4, z >> 4)) else {
            return &self.void;
        };
        &chunk.biome_palette[chunk.biomes[index] as usize]
    }

    /// Set one 4x4x4 cell's biome. `x`, `y` and `z` are **block**
    /// coordinates anywhere inside the cell.
    pub fn set_biome(&mut self, x: i32, y: i32, z: i32, biome: &str) {
        let Some(index) = self.biome_index(x, y, z) else {
            return;
        };
        let Some(chunk) = self.chunks.get_mut(&(x >> 4, z >> 4)) else {
            return;
        };
        let id = match chunk.biome_palette.iter().position(|p| p == biome) {
            Some(id) => id,
            None => {
                chunk.biome_palette.push(biome.to_string());
                chunk.biome_palette.len() - 1
            }
        };
        chunk.biomes[index] = u16::try_from(id).expect("a chunk cannot hold 65,536 biomes");
    }

    /// Every biome present in a square of chunks' palettes, sorted.
    ///
    /// This is what decides which features a chunk even attempts, and
    /// vanilla collects it from the **region's** section palettes rather
    /// than from the biome source: a biome the source could produce but this
    /// region does not contain contributes nothing. Reading it from the
    /// source instead would attempt every feature in the dimension.
    ///
    /// Sorted and deduplicated because the attempt order is built from it,
    /// and an unordered set at a behaviour-visible site is a bug by
    /// definition (ORACLE.md §6).
    #[must_use]
    pub fn biomes_in(&self, min_chunk_x: i32, min_chunk_z: i32, radius: i32) -> Vec<String> {
        let mut out: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                let Some(chunk) = self.chunks.get(&(min_chunk_x + dx, min_chunk_z + dz)) else {
                    continue;
                };
                // The *palette*, not the cells: a palette entry that no cell
                // uses is still in the palette, and vanilla walks the
                // palette too.
                for biome in &chunk.biome_palette {
                    out.insert(biome.clone());
                }
            }
        }
        out.into_iter().collect()
    }

    /// The biome palette and cell ids of one chunk.
    #[must_use]
    pub fn chunk_biomes(&self, chunk_x: i32, chunk_z: i32) -> Option<(&[String], &[u16])> {
        self.chunks
            .get(&(chunk_x, chunk_z))
            .map(|c| (c.biome_palette.as_slice(), c.biomes.as_slice()))
    }

    /// The palette and block ids of one chunk, for whoever writes it to disk.
    ///
    /// `None` for a chunk that was never started.
    #[must_use]
    pub fn chunk_data(&self, chunk_x: i32, chunk_z: i32) -> Option<(&[String], &[u16])> {
        self.chunks
            .get(&(chunk_x, chunk_z))
            .map(|c| (c.palette.as_slice(), c.ids.as_slice()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read box is closed on every face. A half-open box would drop the
    /// far wall of a scan window, and the far wall is exactly where a
    /// cross-chunk read lands.
    #[test]
    fn read_box_is_closed_on_every_face() {
        let bounds = [1248, 1253, 13, 35, 763, 767];
        assert!(in_read_box(&bounds, 1248, 13, 763), "low corner is inside");
        assert!(in_read_box(&bounds, 1253, 35, 767), "high corner is inside");
        assert!(in_read_box(&bounds, 1250, 24, 765), "the middle is inside");
        // One step out on each axis, in each direction, so a sign error in
        // any one comparison fails this rather than half of them.
        assert!(!in_read_box(&bounds, 1247, 24, 765), "below x");
        assert!(!in_read_box(&bounds, 1254, 24, 765), "above x");
        assert!(!in_read_box(&bounds, 1250, 12, 765), "below y");
        assert!(!in_read_box(&bounds, 1250, 36, 765), "above y");
        assert!(!in_read_box(&bounds, 1250, 24, 762), "below z");
        assert!(!in_read_box(&bounds, 1250, 24, 768), "above z");
    }

    /// A one-block box is a legal box, and it is how the trace is narrowed
    /// once a position is known.
    #[test]
    fn read_box_admits_a_single_block() {
        let bounds = [5, 5, 5, 5, 5, 5];
        assert!(in_read_box(&bounds, 5, 5, 5));
        assert!(!in_read_box(&bounds, 5, 5, 6));
    }

    /// Everything but air counts, which is enough to tell a live map from a
    /// fossil.
    struct Solid;

    impl HeightmapPredicates for Solid {
        fn matches(&self, state: &str, _map: LiveHeightmap) -> bool {
            state != "minecraft:air"
        }
    }

    fn canvas() -> Canvas {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        canvas
    }

    /// The parallel decoration pass moves a chunk out of the canvas, writes
    /// it, and moves it back. The blocks have to survive the round trip and
    /// the outside-access count has to come with them, because that count is
    /// the instrument that says the group was big enough.
    #[test]
    fn a_detached_group_round_trips_its_blocks_and_its_counters() {
        let mut canvas = Canvas::new(-64, 384);
        for x in 0..3 {
            canvas.ensure_chunk(x, 0, "minecraft:air");
        }
        canvas.set_block(20, 5, 3, "minecraft:stone");
        let mut group = canvas.detach([(1, 0)]);
        assert_eq!(group.block_at(20, 5, 3), "minecraft:stone");
        // The canvas no longer holds it, so a read there is outside.
        assert_eq!(canvas.block_at(20, 5, 3), "minecraft:air");
        assert_eq!(canvas.outside_accesses(), 1);
        // And the group does not hold its neighbours, which is the case the
        // count exists to make visible.
        assert_eq!(group.block_at(4, 5, 3), "minecraft:air");
        assert_eq!(group.outside_accesses(), 1);
        group.set_block(20, 6, 3, "minecraft:dirt");
        group.mark_post_processing(20, 6, 3);
        canvas.merge(group);
        assert_eq!(canvas.block_at(20, 5, 3), "minecraft:stone");
        assert_eq!(canvas.block_at(20, 6, 3), "minecraft:dirt");
        assert_eq!(canvas.post_processing(), vec![(20, 6, 3)]);
        assert_eq!(canvas.outside_accesses(), 2);
    }

    /// The disjointness of two groups in one level is an argument; this is
    /// the check that fails loudly if the argument is ever wrong.
    #[test]
    #[should_panic(expected = "two decoration groups both held chunk")]
    fn merging_a_chunk_the_canvas_still_holds_panics() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        let group = canvas.detach([(0, 0)]);
        canvas.ensure_chunk(0, 0, "minecraft:air");
        canvas.merge(group);
    }

    #[test]
    fn a_written_block_reads_back() {
        let mut canvas = canvas();
        assert!(canvas.set_block(3, 70, 5, "minecraft:stone"));
        assert_eq!(canvas.block_at(3, 70, 5), "minecraft:stone");
        assert_eq!(canvas.block_at(4, 70, 5), "minecraft:air");
    }

    /// The return value is a *difference*, which is what a write log needs.
    #[test]
    fn writing_the_same_block_twice_reports_no_change() {
        let mut canvas = canvas();
        assert!(canvas.set_block(0, 0, 0, "minecraft:stone"));
        assert!(!canvas.set_block(0, 0, 0, "minecraft:stone"));
        assert!(canvas.set_block(0, 0, 0, "minecraft:dirt"));
    }

    /// One palette entry per distinct state, however many blocks use it.
    #[test]
    fn the_palette_interns() {
        let mut canvas = canvas();
        for x in 0..16 {
            for z in 0..16 {
                canvas.set_block(x, 70, z, "minecraft:stone");
            }
        }
        canvas.set_block(0, 71, 0, "minecraft:dirt");
        let (palette, ids) = canvas.chunk_data(0, 0).expect("chunk");
        assert_eq!(
            palette,
            ["minecraft:air", "minecraft:stone", "minecraft:dirt"]
        );
        assert_eq!(ids.len(), 384 * 256);
    }

    /// Neighbouring chunks are separate stores and the same local coordinates
    /// do not collide.
    #[test]
    fn chunks_do_not_bleed_into_each_other() {
        let mut canvas = canvas();
        canvas.ensure_chunk(1, 0, "minecraft:air");
        canvas.set_block(0, 70, 0, "minecraft:stone");
        canvas.set_block(16, 70, 0, "minecraft:dirt");
        assert_eq!(canvas.block_at(0, 70, 0), "minecraft:stone");
        assert_eq!(canvas.block_at(16, 70, 0), "minecraft:dirt");
    }

    /// Negative coordinates land in the chunk an arithmetic shift says, not
    /// the one a division would.
    #[test]
    fn negative_coordinates_use_an_arithmetic_shift() {
        let mut canvas = Canvas::new(-64, 384);
        canvas.ensure_chunk(-1, -1, "minecraft:air");
        assert!(canvas.set_block(-1, 70, -1, "minecraft:stone"));
        assert_eq!(canvas.block_at(-1, 70, -1), "minecraft:stone");
        // -1 / 16 is 0 in Rust and -1 with a shift; the shift is right.
        assert!(!canvas.has_chunk(0, 0));
    }

    /// A read outside a generated chunk is the void block, not a panic and
    /// not a plausible-looking stone.
    #[test]
    fn a_read_outside_the_canvas_is_void() {
        let canvas = canvas();
        assert_eq!(canvas.block_at(1000, 70, 1000), "minecraft:air");
        assert_eq!(canvas.block_at(0, 5000, 0), "minecraft:air");
        assert_eq!(canvas.block_at(0, -5000, 0), "minecraft:air");
    }

    /// The biome set spans the region, not the centre chunk, and it is
    /// sorted.
    #[test]
    fn the_biome_set_spans_the_region() {
        let mut canvas = Canvas::new(-64, 384);
        for x in -1..=1 {
            for z in -1..=1 {
                canvas.ensure_chunk(x, z, "minecraft:air");
            }
        }
        canvas.set_biome(0, 0, 0, "minecraft:desert");
        canvas.set_biome(-16, 0, 0, "minecraft:taiga");

        let biomes = canvas.biomes_in(0, 0, 1);
        assert!(biomes.contains(&"minecraft:desert".to_string()));
        assert!(
            biomes.contains(&"minecraft:taiga".to_string()),
            "the neighbour's biome counts: {biomes:?}"
        );
        let mut sorted = biomes.clone();
        sorted.sort();
        assert_eq!(biomes, sorted, "the set is ordered");

        let centre_only = canvas.biomes_in(0, 0, 0);
        assert!(!centre_only.contains(&"minecraft:taiga".to_string()));
    }

    /// The `_WG` maps are the noise stage's and nothing else raises them —
    /// a feature that raises the live maps must leave the fossils alone.
    #[test]
    fn the_wg_heightmaps_are_fossils_only_once_frozen() {
        let mut canvas = canvas();
        let mut surface = [0i32; 256];
        surface[0] = 70;
        canvas.set_wg_heightmaps(0, 0, &surface, &[64; 256]);
        assert_eq!(canvas.world_surface_wg(0, 0), 70);
        assert_eq!(canvas.ocean_floor_wg(0, 0), 64);

        // Frozen — which is decoration onwards — a block written far above
        // does not move them.
        canvas.set_block(0, 200, 0, "minecraft:stone");
        canvas.update_heightmaps(0, 200, 0, &Solid);
        assert_eq!(canvas.world_surface_wg(0, 0), 70, "still the fossil");
        assert_eq!(
            canvas.height_of(LiveHeightmap::WorldSurface, 0, 0),
            201,
            "while the live map moved"
        );
    }

    /// A live heightmap falls when a feature digs the top block away.
    ///
    /// The raise direction is obvious and was always right; this is the one
    /// that was missing. Decoration removes blocks — lakes, monster rooms,
    /// geodes — and every feature that anchors to a heightmap afterwards
    /// reads whatever this left behind.
    #[test]
    fn a_live_heightmap_falls_when_a_feature_digs() {
        let mut canvas = canvas();
        for y in 60..=67 {
            canvas.set_block(0, y, 0, "minecraft:stone");
            canvas.update_heightmaps(0, y, 0, &Solid);
        }
        assert_eq!(canvas.height_of(LiveHeightmap::WorldSurface, 0, 0), 68);

        // A lake digs the top three out.
        for y in (65..=67).rev() {
            canvas.set_block(0, y, 0, "minecraft:air");
            canvas.update_heightmaps(0, y, 0, &Solid);
        }
        assert_eq!(
            canvas.height_of(LiveHeightmap::WorldSurface, 0, 0),
            65,
            "the map walks down to the next solid block"
        );

        // And digging a block that is *not* the top one moves nothing.
        canvas.set_block(0, 62, 0, "minecraft:air");
        canvas.update_heightmaps(0, 62, 0, &Solid);
        assert_eq!(
            canvas.height_of(LiveHeightmap::WorldSurface, 0, 0),
            65,
            "a hole under the surface is not the surface"
        );
    }

    /// While live — surface and carvers — the `_WG` maps track writes, and
    /// crucially they go **down** when the top block is taken away. That is
    /// the direction that matters: a carver lowering a sea floor without
    /// lowering `OCEAN_FLOOR_WG` strands every placement anchored to it.
    #[test]
    fn a_live_wg_heightmap_falls_when_the_top_block_is_carved() {
        let mut canvas = canvas();
        canvas.set_wg_live(WgPredicates::new(|state: &str| WgFacts {
            air: state == "minecraft:air",
            blocks_motion: state != "minecraft:air" && state != "minecraft:water",
        }));

        for y in 60..=67 {
            canvas.set_block(0, y, 0, "minecraft:stone");
        }
        assert_eq!(canvas.ocean_floor_wg(0, 0), 68, "one above the top stone");

        // Carve the top four, as a carver under the sea would.
        for y in (64..=67).rev() {
            canvas.set_block(0, y, 0, "minecraft:water");
        }
        assert_eq!(
            canvas.ocean_floor_wg(0, 0),
            64,
            "the map walks down to the next motion-blocking block"
        );
        assert_eq!(
            canvas.world_surface_wg(0, 0),
            68,
            "water is not air, so WORLD_SURFACE_WG does not move"
        );

        // And once frozen it stops, even though more is removed.
        canvas.freeze_wg();
        for y in 60..=63 {
            canvas.set_block(0, y, 0, "minecraft:air");
        }
        assert_eq!(canvas.ocean_floor_wg(0, 0), 64, "frozen");
    }

    /// A biome cell covers 4x4x4 blocks, and every block in it answers the
    /// same.
    #[test]
    fn a_biome_cell_covers_four_blocks_each_way() {
        let mut canvas = canvas();
        canvas.set_biome(0, 64, 0, "minecraft:desert");
        for x in 0..4 {
            for y in 64..68 {
                for z in 0..4 {
                    assert_eq!(canvas.biome_at(x, y, z), "minecraft:desert", "{x} {y} {z}");
                }
            }
        }
        assert_eq!(
            canvas.biome_at(4, 64, 0),
            "minecraft:plains",
            "the next cell is untouched"
        );
    }

    /// One cell per 4x4x4 over the whole column.
    #[test]
    fn the_biome_grid_is_the_size_vanilla_expects() {
        let canvas = canvas();
        let (_, cells) = canvas.chunk_biomes(0, 0).expect("chunk");
        assert_eq!(cells.len(), 384 / 4 * 16, "96 layers of 4x4 cells");
    }

    /// `ensure_chunk` twice must not wipe what a stage already wrote.
    #[test]
    fn ensuring_a_chunk_twice_keeps_its_blocks() {
        let mut canvas = canvas();
        canvas.set_block(0, 70, 0, "minecraft:stone");
        canvas.ensure_chunk(0, 0, "minecraft:air");
        assert_eq!(canvas.block_at(0, 70, 0), "minecraft:stone");
    }
}

// ---------------------------------------------------------------------------
// One chunk, borrowed on its own
// ---------------------------------------------------------------------------

/// An exclusive borrow of exactly one chunk of a [`Canvas`].
///
/// # What it is for
///
/// A stage that writes only into its own chunk does not need the whole
/// canvas, and asking for the whole canvas is what forces it to run on one
/// thread. [`Canvas::chunk_writers`] hands out one of these per chunk from
/// `BTreeMap::values_mut`, so the borrows are disjoint by construction and a
/// worker pool can take a slice each.
///
/// # Why writes outside the chunk are dropped rather than a panic
///
/// [`Canvas::set_block`] already drops them — a write to a chunk the canvas
/// does not hold returns `false` and changes nothing, which is how the
/// margin's outermost ring behaves today. A writer that panicked instead
/// would be a *different* semantics, and this type exists to be substitutable
/// for the canvas in a stage that has been shown not to need the difference,
/// not to police it. `set_block` returns `false` for a dropped write, exactly
/// as the canvas does, so a caller that checks gets the same answer.
///
/// # Determinism
///
/// The palette is **per chunk** and interned in first-write order within that
/// chunk, so two chunks written on two threads cannot reorder each other's.
/// That is the property that makes a chunk-local stage's output independent
/// of how the work was divided, and it is why this seam does not need a lock.
pub struct ChunkWriter<'a> {
    min_y: i32,
    height: i32,
    chunk_x: i32,
    chunk_z: i32,
    blocks: &'a mut ChunkBlocks,
    /// The canvas's `wg_live`, **borrowed**. `None` for a stage that runs
    /// while the `_WG` maps are fossils (the noise stage); `Some` for one
    /// that runs while they move (surface, carvers). Chunk-local either way
    /// — see [`Canvas::chunk_writers`].
    ///
    /// **A reference, not a clone, and that is a measured 20% of the
    /// pipeline.** It used to be an owned `WgPredicates`, which is an `Arc`,
    /// and [`ChunkWriter::update_wg`] cloned it **once per block written**.
    /// One atomic increment and one decrement, on a single cache line that
    /// every worker shares: at one worker the line sits in L1 and costs
    /// nothing worth measuring, at twelve it ping-pongs across four L3
    /// slices. `ChunkWriter::set_block` went from 1.33% of the run at one
    /// worker to **20.31% at twelve** — 2.9 G cycles to 54.0 G — while the
    /// whole run's instruction count stayed flat to 0.27%. That is the
    /// signature of contention rather than work, and it was the whole of the
    /// surface stage's parallel tax. See BENCH.md.
    wg: Option<&'a WgPredicates>,
    /// [`Canvas::void`], so a read outside this chunk answers exactly as the
    /// canvas would rather than panicking.
    void: &'a str,
    /// Marks this writer's stage made, in insertion order, for the caller to
    /// union into the canvas once the workers have joined.
    ///
    /// Owned rather than a borrow of the canvas's set for the same reason the
    /// blocks are: the writers are handed out in parallel and the canvas's
    /// mark map is one structure. Within-chunk order is the only order that
    /// is behaviour (`docs/ORACLE.md` §19), and one chunk is written by one
    /// writer, so collecting per writer preserves exactly what matters.
    post_processing: Vec<(i32, i32, i32)>,
}

impl ChunkWriter<'_> {
    /// `ChunkAccess#markPosForPostProcessing`, recorded for the caller.
    pub fn mark_post_processing(&mut self, x: i32, y: i32, z: i32) {
        self.post_processing.push((x, y, z));
    }

    /// The marks this writer made, in insertion order, leaving it empty.
    #[must_use]
    pub fn take_post_processing(&mut self) -> Vec<(i32, i32, i32)> {
        std::mem::take(&mut self.post_processing)
    }

    #[must_use]
    pub const fn min_y(&self) -> i32 {
        self.min_y
    }

    #[must_use]
    pub const fn height(&self) -> i32 {
        self.height
    }

    /// The state at a position, exactly as [`Canvas::block_at`] answers it:
    /// the void block outside this chunk or outside the vertical extent.
    #[must_use]
    pub fn block_at(&self, x: i32, y: i32, z: i32) -> &str {
        if (x >> 4) != self.chunk_x || (z >> 4) != self.chunk_z {
            return self.void;
        }
        if y < self.min_y || y >= self.min_y + self.height {
            return self.void;
        }
        let index = usize::try_from((y - self.min_y) * 256 + (z & 15) * 16 + (x & 15))
            .expect("a positive index");
        &self.blocks.palette[self.blocks.ids[index] as usize]
    }

    /// `WORLD_SURFACE_WG` at a column of this chunk — [`Canvas::world_surface_wg`]
    /// restricted to what this writer owns.
    #[must_use]
    pub fn world_surface_wg(&self, x: i32, z: i32) -> i32 {
        if (x >> 4) != self.chunk_x || (z >> 4) != self.chunk_z {
            return self.min_y;
        }
        let column = usize::try_from((z & 15) * 16 + (x & 15)).expect("a positive index");
        self.blocks.wg_heightmaps[0][column]
    }

    /// [`Canvas::update_wg`], restricted to this chunk. The two bodies are
    /// the same walk; keeping them in step is what the byte-identical gate
    /// on the parallel stages checks.
    fn update_wg(&mut self, x: i32, y: i32, z: i32, state: &str) {
        // `Option<&_>` is `Copy`, so this is a pointer read and the borrow it
        // yields is the writer's `'a`, not a borrow of `*self` — which is why
        // the `&mut self` calls below still compile without the `Arc` clone
        // this line used to be.
        let Some(predicates) = self.wg else {
            return;
        };
        let facts = (predicates.0)(state);
        let column = usize::try_from((z & 15) * 16 + (x & 15)).expect("a positive index");
        for (slot, matches) in [(0usize, !facts.air), (1usize, facts.blocks_motion)] {
            let current = self.blocks.wg_heightmaps[slot][column];
            let new = if matches {
                if y >= current {
                    Some(y + 1)
                } else {
                    None
                }
            } else if current - 1 == y {
                let mut found = self.min_y;
                let mut probe = y - 1;
                while probe >= self.min_y {
                    let hit = {
                        let below = self.block_at(x, probe, z);
                        let facts = (predicates.0)(below);
                        if slot == 0 {
                            !facts.air
                        } else {
                            facts.blocks_motion
                        }
                    };
                    if hit {
                        found = probe + 1;
                        break;
                    }
                    probe -= 1;
                }
                Some(found)
            } else {
                None
            };
            if let Some(new) = new {
                self.blocks.wg_heightmaps[slot][column] = new;
            }
        }
    }

    /// The chunk this writer owns, in chunk coordinates.
    #[must_use]
    pub const fn position(&self) -> (i32, i32) {
        (self.chunk_x, self.chunk_z)
    }

    /// Write a block in **absolute** block coordinates.
    ///
    /// Same signature and same return as [`Canvas::set_block`]: whether the
    /// block changed. A position outside this writer's own chunk, or outside
    /// the world's vertical extent, is dropped and reports `false`.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) -> bool {
        trace_write(x, y, z, state);
        trace_feature(x, y, z, state);
        if (x >> 4) != self.chunk_x || (z >> 4) != self.chunk_z {
            return false;
        }
        if y < self.min_y || y >= self.min_y + self.height {
            return false;
        }
        let index = usize::try_from((y - self.min_y) * 256 + (z & 15) * 16 + (x & 15))
            .expect("a positive index");
        let id = match self.blocks.palette.iter().position(|p| p == state) {
            Some(id) => id,
            None => {
                self.blocks.palette.push(state.to_string());
                self.blocks.palette.len() - 1
            }
        };
        let id = u16::try_from(id).expect("a chunk cannot hold 65,536 distinct states");
        let changed = self.blocks.ids[index] != id;
        self.blocks.ids[index] = id;
        if self.wg.is_some() {
            self.update_wg(x, y, z, state);
        }
        changed
    }

    /// Record the two `_WG` heightmaps for this chunk — the noise stage's
    /// job, and nobody else's. [`Canvas::set_wg_heightmaps`] by another name.
    pub fn set_wg_heightmaps(&mut self, world_surface: &[i32; 256], ocean_floor: &[i32; 256]) {
        self.blocks.wg_heightmaps[0].copy_from_slice(world_surface);
        self.blocks.wg_heightmaps[1].copy_from_slice(ocean_floor);
    }
}

// ---------------------------------------------------------------------------
// Write tracing
// ---------------------------------------------------------------------------

thread_local! {
    /// What is writing to the canvas right now — a feature name, a stage, or
    /// "?" when nothing has said.
    static WRITER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Whether any instrument that needs the writer label is set.
///
/// Callers gate label *construction* on this. Building a label costs an
/// allocation and a `format!` per feature placement, which is real work on
/// the generator's hot path for an instrument that is off in every run that
/// matters — so the check has to come first, not inside [`set_writer`].
///
/// `RUSTMC_TRACE_READS` is in this list even though it traces *reads*: a read
/// line whose writer column is blank cannot say which feature asked the
/// question, which is the only thing the trace exists to answer.
#[must_use]
pub fn tracing_writes() -> bool {
    thread_local! {
        static ON: bool = std::env::var_os("RUSTMC_TRACE_WRITE").is_some()
            || std::env::var_os("RUSTMC_TRACE_FEATURE").is_some()
            || std::env::var_os("RUSTMC_TRACE_READS").is_some();
    }
    ON.with(|on| *on)
}

/// `RUSTMC_TRACE_READS=x0,x1,y0,y1,z0,z1` — print every canvas *read* whose
/// position falls inside the closed box, with the feature that asked.
///
/// Every other instrument here answers "who wrote this block". None of them
/// answers "what did this feature see", and the two are different questions
/// whenever a feature's *decision* — not its output — is what diverged. A
/// feature that reads a neighbour's block and then declines to write leaves
/// no trace at all in a write log, yet it has already spent the draws that
/// shift every later feature in the chunk.
///
/// It is a box rather than a single position because the question it was
/// built for is "which of the 276 blocks this scan can see differs between
/// two chunk orders" — 276 single-position runs is not an instrument.
///
/// **A malformed value panics rather than silently disabling the trace.** A
/// knob that quietly does nothing reports a clean run as evidence of absence:
/// the trace prints nothing, and nothing is exactly what a correct world also
/// prints.
fn trace_read(x: i32, y: i32, z: i32, state: &str) {
    thread_local! {
        static BOUNDS: Option<[i32; 6]> = read_box();
    }
    BOUNDS.with(|bounds| {
        let Some(bounds) = bounds else { return };
        if !in_read_box(bounds, x, y, z) {
            return;
        }
        WRITER.with(|w| {
            let w = w.borrow();
            let who = if w.is_empty() {
                "? (no feature running)"
            } else {
                w.as_str()
            };
            println!("READ {x},{y},{z} = {state}   [{who}]");
        });
    });
}

/// Parse `RUSTMC_TRACE_READS`, panicking on a value that is set but unusable.
fn read_box() -> Option<[i32; 6]> {
    let raw = std::env::var("RUSTMC_TRACE_READS").ok()?;
    let parts: Vec<&str> = raw.split(',').map(str::trim).collect();
    assert!(
        parts.len() == 6,
        "RUSTMC_TRACE_READS wants x0,x1,y0,y1,z0,z1 — got {} field(s) in {raw:?}",
        parts.len()
    );
    let mut out = [0_i32; 6];
    for (slot, part) in out.iter_mut().zip(parts) {
        *slot = part
            .parse()
            .unwrap_or_else(|_| panic!("RUSTMC_TRACE_READS field {part:?} is not an integer"));
    }
    assert!(
        out[0] <= out[1] && out[2] <= out[3] && out[4] <= out[5],
        "RUSTMC_TRACE_READS box is empty — each low bound must not exceed its high: {out:?}"
    );
    Some(out)
}

/// Closed on every face, so a one-block box is written `x,x,y,y,z,z`.
const fn in_read_box(bounds: &[i32; 6], x: i32, y: i32, z: i32) -> bool {
    x >= bounds[0]
        && x <= bounds[1]
        && y >= bounds[2]
        && y <= bounds[3]
        && z >= bounds[4]
        && z <= bounds[5]
}

/// Name whatever is about to write, for [`trace_write`].
pub fn set_writer(label: &str) {
    WRITER.with(|w| {
        let mut w = w.borrow_mut();
        w.clear();
        w.push_str(label);
    });
}

/// `RUSTMC_TRACE_WRITE=x,y,z` prints every write to one block, in order,
/// with whoever made it.
///
/// A whole-world diff says a block is the wrong *thing*; the attempt dump
/// says where each feature was *invoked*. Neither says which feature
/// actually wrote the block, and a block can be written several times in one
/// chunk with only the last one visible. That gap is the shape of every
/// remaining fault in the diff, so it is worth an instrument rather than
/// another one-off harness.
/// `RUSTMC_TRACE_FEATURE=<substring>[,<substring>...]` prints **every** write
/// made while a matching feature is running, not just the ones at one
/// position.
///
/// [`trace_write`] answers "who wrote this block". This answers "what did
/// this feature write", which is the question when a whole feature's output
/// is being compared against vanilla's — and comparing four sampled
/// positions and calling it a match is how a premise goes unchecked.
///
/// The list is comma-separated because a fault cluster is usually a *family*
/// — seagrass and kelp share a heightmap, a survival rule and an ocean — and
/// the run that produces the trace is a whole-world generation costing
/// fourteen minutes. Two families in one run is two runs saved, and a trace
/// taken from a *different* run is a trace of a different world.
fn trace_feature(x: i32, y: i32, z: i32, state: &str) {
    thread_local! {
        static WANT: Vec<String> = std::env::var("RUSTMC_TRACE_FEATURE")
            .ok()
            .into_iter()
            .flat_map(|v| {
                v.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
            })
            .collect();
    }
    WANT.with(|want| {
        if want.is_empty() {
            return;
        }
        WRITER.with(|w| {
            let w = w.borrow();
            if !w.is_empty() && want.iter().any(|p| w.contains(p.as_str())) {
                println!("FEATWRITE {x},{y},{z} = {state}   [{w}]");
            }
        });
    });
}

fn trace_write(x: i32, y: i32, z: i32, state: &str) {
    // A thread-local so the check is a pointer read once the miss is cached,
    // rather than a `getenv` per block written.
    thread_local! {
        static TARGET: Option<(i32, i32, i32)> = std::env::var("RUSTMC_TRACE_WRITE")
            .ok()
            .and_then(|v| {
                let mut parts = v.split(',').map(|p| p.trim().parse::<i32>().ok());
                Some((parts.next()??, parts.next()??, parts.next()??))
            });
    }
    TARGET.with(|target| {
        if *target == Some((x, y, z)) {
            WRITER.with(|w| {
                let w = w.borrow();
                // "?" means nothing claimed the write — a stage rather than
                // a feature. It is deliberately not a guess.
                let who = if w.is_empty() {
                    "? (no feature running)"
                } else {
                    w.as_str()
                };
                println!("WRITE ({x},{y},{z}) <- {state}   by {who}");
            });
        }
    });
}
