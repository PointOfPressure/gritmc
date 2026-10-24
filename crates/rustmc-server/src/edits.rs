//! The mutable half of the world.
//!
//! # Why an overlay rather than a mutable canvas
//!
//! The generated world is [`rustmc_worldgen::canvas::Canvas`], and it is the
//! thing this project measures parity against. Letting a player write into it
//! would mean the canvas a client stands in and the canvas a region diff
//! reads are the same object, and the first time anyone dug a hole the number
//! would move. Vanilla draws the same line — a chunk is *generated* once and
//! *saved* thereafter — and the reason is the same: a generator has to be
//! reproducible and a world has to change.
//!
//! So the canvas stays exactly what the pipeline produced, and every player
//! write lands here. A read is "the overlay, else the canvas", which is one
//! branch on a hash lookup and is only paid where somebody has actually
//! changed something.
//!
//! # Why it is keyed by chunk first
//!
//! Encoding a chunk asks "what has changed in *this* chunk", and a flat map
//! keyed by position answers that by scanning every edit in the world. Two
//! levels turns it into a lookup plus a walk over that chunk's own edits.
//!
//! Both levels are `BTreeMap`s. Nothing here iterates into a packet today,
//! but the moment something does — a save format, a "what changed" dump — a
//! hash order would make it non-reproducible, and that is the class of bug
//! this project treats as a defect by definition (ORACLE.md §6).

use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::interact::BlockPos;

/// What the play loop may do to blocks.
///
/// A trait rather than a `&World` so the send loop keeps working for the
/// noise-stage fallback, which has no world to write into: the call site
/// passes `None` and a client there can still walk around, it simply cannot
/// change anything. That is also the honest shape — "this connection has a
/// mutable world" is a property of the connection, not an assumption of the
/// loop.
pub trait Blocks: Send + Sync {
    /// The state at a position — the overlay if a player has changed it, the
    /// generated canvas otherwise.
    fn state_at(&self, pos: BlockPos) -> String;

    /// Write a block, returning the new global state id when something
    /// actually changed.
    ///
    /// `None` covers three cases that are the same to a caller: the position
    /// is outside the served world, the state has no network id, or the
    /// block was already that state. In none of them is there anything to
    /// tell a client about, and a client told about a change that did not
    /// happen will happily render it.
    fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32>;

    /// The default state of a block by name — what placing its item puts
    /// down — or `None` if there is no such block.
    fn default_state(&self, block: &str) -> Option<String>;

    /// Whether a state string is one the block table knows.
    ///
    /// The guard on a computed placement state: an unknown state has no
    /// network id, and writing one would fail the *next* encode of the whole
    /// chunk rather than the one placement that caused it.
    fn has_state(&self, state: &str) -> bool;

    /// The global network id of a state, or `None` if the table does not
    /// know it.
    ///
    /// Needed by the resyncs vanilla sends after `use_item_on`, which name a
    /// position's *current* state rather than something just written — so
    /// there is no `set_block` return value to take the id from.
    fn state_id(&self, state: &str) -> Option<i32>;

    /// `BlockState#canBeReplaced(BlockPlaceContext)` at a position: whether
    /// this placement may occupy it.
    ///
    /// **Item-aware, and that is the whole of why it takes a context.** It
    /// used to be the no-argument `canBeReplaced()` — a property of the
    /// position alone — which is the wrong one of vanilla's two overloads for
    /// both of its call sites: `BlockPlaceContext`'s constructor and
    /// `canPlace()` each pass the context. The default rule is
    /// `replaceable && !item.is(asItem())`, so tall grass clicked while
    /// holding tall grass is **not** replaceable and the placement steps along
    /// the face; and `SlabBlock` and `SnowLayerBlock` override it outright,
    /// which is how a second slab merges into the first instead of stacking
    /// on top of it. See [`crate::placement::can_be_replaced`].
    fn replaceable(&self, pos: BlockPos, ctx: &crate::placement::Replacing<'_>) -> bool;

    /// `getFluidState(pos).is(FluidTags.WATER)` — whether the fluid at a
    /// position is water, source or flowing.
    ///
    /// Not "is the block water": 11,792 of the 32,366 states have a water
    /// fluid state and are not water — every waterlogged fence, slab and
    /// stair. `CoralBlock.scanForWater` asks this one.
    /// `getBlockState(pos).isFaceSturdy(level, pos, face)` — whether the
    /// block at `pos` presents a face solid enough to hang something on.
    ///
    /// `block_props.json` carries it as six booleans per state in
    /// `Face::to_3d` order, so this is a lookup rather than a shape
    /// computation. It is what most `canSurvive` rules reduce to, and the
    /// first caller is `LadderBlock.getStateForPlacement`.
    ///
    /// **Deliberately not defaulted.** A default returning `false` would make
    /// every ported placement rule refuse silently in any implementation that
    /// forgot to override it, which is the "check that cannot fail" shape this
    /// corpus has found sixteen of.
    fn face_sturdy(&self, pos: BlockPos, face: crate::interact::Face) -> bool;

    /// `getBlockState(pos).isRedstoneConductor(level, pos)`.
    ///
    /// A per-block `StatePredicate` on `BlockBehaviour.Properties`, carried
    /// as its own column in `block_props.json` since 2026-08-05 because
    /// **none of the other eight can stand in for it**: `glass` has a full
    /// collision face on all six sides and is not a conductor, which is why
    /// redstone dust does not climb it.
    ///
    /// `RedStoneWireBlock.getConnectingSide` asks it at offset 106 to decide
    /// whether a wire is blocked, and the three-argument overload asks it
    /// about `pos.above()` to decide whether a wire may climb at all.
    ///
    /// **Deliberately not defaulted**, for the reason [`Self::face_sturdy`]
    /// gives. A default `false` here would make every wire connect upwards
    /// through solid stone and look like a working rule.
    fn redstone_conductor(&self, pos: BlockPos) -> bool;

    /// `getBlockState(pos).isSignalSource()`.
    ///
    /// The only column `RedStoneWireBlock.shouldConnectTo` needs: the wire,
    /// the repeater and the observer are decided there by block identity and
    /// their own `facing`, and a comparator has no facing rule at all.
    ///
    /// **Deliberately not defaulted**, same reason.
    fn signal_source(&self, pos: BlockPos) -> bool;

    /// `NoteBlock.setInstrument(level, pos, state)` — the instrument a note
    /// block at `pos` would carry, as a serialized name.
    ///
    /// **Takes the note block's own position and reads two others**, which is
    /// the whole reason it is a method here rather than a cell in
    /// `placement.json`: the answer is `pos.above()`'s instrument if that one
    /// works above a note block, and otherwise `pos.below()`'s — inverted, so
    /// that a *head* below gives `harp` rather than that head's sound. A
    /// table keyed by the clicked face cannot express either read.
    ///
    /// Defaulted to `None` for the same reason [`Self::level_min_y`] is: most
    /// impls in this crate are test fakes with no `block_props.json` behind
    /// them, and a fake that answered would be inventing an instrument. A
    /// caller that gets `None` leaves the state alone rather than guessing,
    /// which keeps a stale table visible instead of silently harp.
    fn note_block_instrument(&self, pos: BlockPos) -> Option<String> {
        let _ = pos;
        None
    }

    /// The whole `block_props.json` table, for the rules that need columns
    /// this trait has no accessor for.
    ///
    /// **Defaulted to `None`, and that is the opposite decision from
    /// [`Self::face_sturdy`] three methods up, for a stated reason.** The
    /// consumer is
    /// [`rustmc_behavior::place_survive::vanilla_would_place`], and `None`
    /// there means *no `canSurvive` gate at all* — every placement allowed,
    /// which is exactly what this server did before the gate existed. A
    /// defaulted `false`-shaped answer would instead make every torch, sign,
    /// rail and door **refuse** to be placed in any implementation that had
    /// not overridden it, and a placement that silently does not happen is
    /// indistinguishable on the wire from a client that never clicked.
    ///
    /// So the default here fails **open** where `face_sturdy`'s would fail
    /// closed, and that is why one is defaulted and the other is not. The
    /// nine fake worlds in this crate's tests take the default and are
    /// ungated by it, which is the pre-existing behaviour they were written
    /// against; only [`crate::world::World`] answers it.
    fn place_props(&self) -> Option<&rustmc_behavior::block_props::BlockPropsTable> {
        None
    }

    fn water_at(&self, pos: BlockPos) -> bool;

    /// `getFluidState(pos).createLegacyBlock()` — what removing the block at
    /// `pos` leaves behind.
    ///
    /// **Not air.** `Level.removeBlock` and `Level.destroyBlock` both write
    /// this, which is why breaking a waterlogged fence leaves water and
    /// breaking water leaves water — the same state, so nothing changes and
    /// no client is told anything. A dig that wrote air instead would drain
    /// the ocean one block at a time.
    fn legacy_fluid_at(&self, pos: BlockPos) -> String;

    /// `BlockState.getFluidState()` — the whole fluid, not the boolean
    /// [`Self::water_at`] answers: `FlowingFluid` needs the amount, the
    /// falling flag and whether it is a source.
    ///
    /// Defaulted to [`crate::fluid::of_state`] **without** the props column,
    /// which is the pure-string reading: right for water, lava and anything
    /// carrying `waterlogged`, and dry for kelp, seagrass and a bubble column,
    /// which are wet through their own `getFluidState` override. The default
    /// exists for the test doubles; [`crate::world::World`] and
    /// [`crate::tiles::Tiles`] answer from `block_props.json`'s per-state
    /// fluid id and get those three right.
    fn fluid_of_state(&self, state: &str) -> crate::fluid::FluidState {
        crate::fluid::of_state(state, None)
    }

    /// `BlockState.isSolid()` — `block_props.json`'s `solid` column, **not**
    /// [`Self::face_sturdy`] and not `blocksMotion`. `FlowingFluid.
    /// getNewLiquid` asks it about the block below a position with two source
    /// neighbours, which is the whole of infinite water.
    ///
    /// Defaulted to "anything that is not air" for the test doubles, the same
    /// approximation they already make for `face_sturdy`;
    /// [`crate::world::World`] answers from the table.
    fn solid_of_state(&self, state: &str) -> bool {
        !crate::shapes::is_air(state)
    }

    /// `LavaFluid.isFastLava(level)` — the level's dimension type carries
    /// `EnvironmentAttributes.FAST_LAVA`. In 26.2's generated dimension types
    /// only `the_nether` sets it. Defaulted to the overworld's `false`;
    /// [`crate::world::World`] answers from its own dimension.
    fn fast_lava(&self) -> bool {
        false
    }

    /// `level.environmentAttributes().getValue(WATER_EVAPORATES, pos)` — also
    /// only `the_nether` in 26.2. `BucketItem.emptyContents` reads it.
    fn water_evaporates(&self) -> bool {
        false
    }

    /// [`Self::fluid_of_state`] at a position.
    fn fluid_at(&self, pos: BlockPos) -> crate::fluid::FluidState {
        self.fluid_of_state(&self.state_at(pos))
    }

    /// `LevelHeightAccessor.getMinY()` — the lowest y this level has.
    ///
    /// **`None` means this world does not know, and a caller must refuse
    /// rather than assume.** `FallingBlock.tick`'s second conjunct is
    /// `pos.getY() >= level.getMinY()`, so a gravity block needs the real
    /// floor; it is `-64` in the overworld and `0` in the other two, and a
    /// transcribed `-64` here would let a block fall out of the nether.
    ///
    /// Defaulted rather than required because the great majority of the impls
    /// in this crate are test fakes with no height model at all, and making
    /// them all answer a question they have no opinion on would be inventing
    /// the number in ten places instead of one. A fake that wants to exercise
    /// a fall overrides it; one that does not gets a refusal, which is the
    /// same convention as [`crate::fallable::Support::complete`].
    fn level_min_y(&self) -> Option<i32> {
        None
    }

    /// `LevelChunkSection.isRandomlyTicking()` for every section of one chunk
    /// column, bottom section first, plus the section-Y of the first entry.
    ///
    /// Vanilla keeps `tickingBlockCount` / `tickingFluidCount` incrementally
    /// in `LevelChunkSection.setBlockState` and `ServerLevel.tickChunk` skips
    /// a section whose two counts are both zero **before** it draws a
    /// position, so a quiet section costs the position stream nothing. This
    /// is the same predicate computed over the merged canvas-plus-edits view.
    ///
    /// **Defaulted to `None`, and `None` means "no section random ticks".**
    /// That is the failure mode `crate::tiles`'s delegation comment names, so
    /// `Tiles` delegates it and `ci/check-blocks-delegation.sh` holds it to
    /// that. A fake that wants random ticks overrides it.
    fn random_ticking_sections(&self, chunk_x: i32, chunk_z: i32) -> Option<(i32, Vec<bool>)> {
        let _ = (chunk_x, chunk_z);
        None
    }

    /// A `y` at or above which every position in the column is air: the
    /// generated `WORLD_SURFACE` heightmap raised past any edit in the
    /// column. An upper bound, not the heightmap — an edit that dug a block
    /// out leaves it where it was, which is still true of the claim.
    ///
    /// `crate::brightness` starts its sky-column walk here instead of at the
    /// top of the world: the walk is the per-pick cost of every grass block's
    /// random tick, and 384 reads per pick is the difference between a tick
    /// and a stall.
    ///
    /// Defaulted to `None`, which `crate::brightness` reads as "walk from the
    /// top of the world" — slower, never wrong.
    fn column_top(&self, x: i32, z: i32) -> Option<i32> {
        let _ = (x, z);
        None
    }
}

/// Player writes on top of the generated canvas.
///
/// Shared by every connection, so the interior mutability is the point: the
/// world itself is handed out as `&'static`, and a block a player breaks has
/// to be broken for the next player to join as well as for the one who broke
/// it.
#[derive(Debug, Default)]
pub struct BlockEdits {
    /// Chunk to (position to state). Read far more often than written — a
    /// chunk encode takes the read lock once — so `RwLock` rather than
    /// `Mutex`.
    by_chunk: RwLock<BTreeMap<(i32, i32), BTreeMap<BlockPos, String>>>,
}

impl BlockEdits {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The state at `pos`, if a player has changed it.
    ///
    /// # Panics
    /// If the lock is poisoned, which means a thread died holding it and the
    /// map's contents are not trustworthy.
    #[must_use]
    pub fn get(&self, pos: BlockPos) -> Option<String> {
        let map = self.by_chunk.read().expect("edits lock");
        map.get(&(pos.0 >> 4, pos.2 >> 4))
            .and_then(|c| c.get(&pos))
            .cloned()
    }

    /// Record a write.
    ///
    /// The caller decides whether it is a *change* — this map cannot, because
    /// it does not know what the canvas underneath says.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn set(&self, pos: BlockPos, state: &str) {
        let mut map = self.by_chunk.write().expect("edits lock");
        map.entry((pos.0 >> 4, pos.2 >> 4))
            .or_default()
            .insert(pos, state.to_string());
    }

    /// Every edit inside one chunk, in position order.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn in_chunk(&self, chunk_x: i32, chunk_z: i32) -> Vec<(BlockPos, String)> {
        let map = self.by_chunk.read().expect("edits lock");
        map.get(&(chunk_x, chunk_z))
            .map(|c| c.iter().map(|(p, s)| (*p, s.clone())).collect())
            .unwrap_or_default()
    }

    /// [`Self::in_chunk`] without the copy, for the random-tick pass, which
    /// reads every touched chunk in range on every tick and needs only to
    /// look.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn for_each_in_chunk(&self, chunk_x: i32, chunk_z: i32, mut f: impl FnMut(BlockPos, &str)) {
        let map = self.by_chunk.read().expect("edits lock");
        if let Some(c) = map.get(&(chunk_x, chunk_z)) {
            for (p, s) in c {
                f(*p, s);
            }
        }
    }

    /// Every chunk that carries at least one edit, in chunk-coordinate order.
    ///
    /// What a save walks. Chunks with an empty map are excluded rather than
    /// returned and skipped later: an edit that was set and then set back to
    /// the generated block leaves the key behind, and saving that chunk would
    /// write a file identical to what generation produces — which is not
    /// wrong, but it makes "how many chunks did the player change" a number
    /// that only grows.
    ///
    /// `BTreeMap` order, so two runs that made the same edits save the same
    /// chunks in the same order (ORACLE.md §6).
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn edited_chunks(&self) -> Vec<(i32, i32)> {
        self.by_chunk
            .read()
            .expect("edits lock")
            .iter()
            .filter(|(_, c)| !c.is_empty())
            .map(|(k, _)| *k)
            .collect()
    }

    /// Whether this chunk has been touched at all — the cheap question, asked
    /// once per chunk encode so the untouched majority pays a lookup and
    /// nothing else.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn touched(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.by_chunk
            .read()
            .expect("edits lock")
            .get(&(chunk_x, chunk_z))
            .is_some_and(|c| !c.is_empty())
    }

    /// How many blocks players have changed.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_chunk
            .read()
            .expect("edits lock")
            .values()
            .map(BTreeMap::len)
            .sum()
    }

    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_reads_back_where_it_was_written() {
        let edits = BlockEdits::new();
        assert_eq!(edits.get((1, 2, 3)), None);
        edits.set((1, 2, 3), "minecraft:air");
        assert_eq!(edits.get((1, 2, 3)).as_deref(), Some("minecraft:air"));
        assert_eq!(edits.len(), 1);
    }

    /// Chunk coordinates are `>> 4`, not `/ 16`: block −1 is in chunk −1, and
    /// truncating division would file it under chunk 0 — where `in_chunk`
    /// would never find it again.
    #[test]
    fn negative_coordinates_land_in_the_negative_chunk() {
        let edits = BlockEdits::new();
        edits.set((-1, 64, -1), "minecraft:stone");
        assert!(edits.touched(-1, -1));
        assert!(!edits.touched(0, 0));
        assert_eq!(edits.in_chunk(-1, -1).len(), 1);
        // And it is still findable by position.
        assert_eq!(edits.get((-1, 64, -1)).as_deref(), Some("minecraft:stone"));
    }

    #[test]
    fn a_chunks_edits_come_back_in_position_order() {
        let edits = BlockEdits::new();
        for pos in [(3, 70, 5), (1, 64, 2), (1, 64, 1)] {
            edits.set(pos, "minecraft:air");
        }
        let got: Vec<_> = edits.in_chunk(0, 0).into_iter().map(|(p, _)| p).collect();
        assert_eq!(got, vec![(1, 64, 1), (1, 64, 2), (3, 70, 5)]);
        assert!(edits.in_chunk(9, 9).is_empty());
    }

    /// Overwriting a position replaces it rather than accumulating — a player
    /// who fills a hole back in must not leave two entries behind.
    #[test]
    fn writing_twice_leaves_one_edit() {
        let edits = BlockEdits::new();
        edits.set((0, 0, 0), "minecraft:air");
        edits.set((0, 0, 0), "minecraft:stone");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits.get((0, 0, 0)).as_deref(), Some("minecraft:stone"));
    }
}
