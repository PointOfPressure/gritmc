//! `PistonBaseBlock.triggerEvent` and `moveBlocks` with their side effects,
//! `MovingPistonBlock`, and the `PistonMovingBlockEntity` tick — the half of
//! the piston that `crate::piston` names as not ported.
//!
//! # What `crate::piston` already has, and where this starts
//!
//! `crate::piston` produces the block event (`checkIfExtend`), encodes it,
//! resolves the structure (`PistonStructureResolver`, non-sticky arm) and
//! decides delivery (`triggerEvent`'s three refusals). What it *wrote* was a
//! list of raw block writes that `containers::run_piston_event` applied with
//! `Blocks::set_block` — no neighbour update, no shape update, no block
//! entity, no `finalTick`. So a piston extended, its head never arrived, and
//! a retracting piston left a `moving_piston` at its own position for ever.
//! `380-a-piston-extends-when-powered` and `390-a-piston-pushes-the-block-
//! in-front` both diverged at exactly the head's arrival, two ticks after
//! the event.
//!
//! This module runs the whole of `triggerEvent` through
//! [`crate::shapes::set_block`] with vanilla's flags, keeps the
//! `PistonMovingBlockEntity`s in [`MovingPistons`] (insertion order, which is
//! `Level.blockEntityTickers`' order), and ticks them from the game clock
//! after the block events, where `ServerLevel.tick` puts `tickBlockEntities`
//! (offset 484, after `runBlockEvents` at 360).
//!
//! # The tick, from `PistonMovingBlockEntity.tick` (offsets 2-242)
//!
//! ```java
//! be.lastTicked = level.getGameTime();
//! be.progressO = be.progress;
//! if (be.progressO >= 1.0F) {
//!     level.removeBlockEntity(pos); be.setRemoved();
//!     if (level.getBlockState(pos).is(Blocks.MOVING_PISTON)) {
//!         BlockState s = Block.updateFromNeighbourShapes(be.movedState, level, pos);
//!         if (s.isAir()) {
//!             level.setBlock(pos, be.movedState, 340);
//!             Block.updateOrDestroy(be.movedState, s, level, pos, 3);
//!         } else {
//!             if (s.hasProperty(WATERLOGGED) && s.getValue(WATERLOGGED)) s = s.setValue(WATERLOGGED, false);
//!             level.setBlock(pos, s, 67);
//!             level.neighborChanged(pos, s.getBlock(), initialOrientation(level, be.getPushDirection(), null));
//!         }
//!     }
//! } else {
//!     float f = be.progress + 0.5F;
//!     moveCollidedEntities(level, pos, f, be);
//!     moveStuckEntities(level, pos, f, be);
//!     be.progress = f;
//!     if (be.progress >= 1.0F) be.progress = 1.0F;
//! }
//! ```
//!
//! So an entity created at tick T (from a block event) runs at T (0 -> 0.5),
//! T+1 (0.5 -> 1.0) and finalises at T+2 — the "exists for two ticks" of
//! 390's `about`. `finalTick` (offsets 1-148) is the same finalisation
//! forced early, with `AIR` for a source piston's entity.
//!
//! # Not modelled, and named
//!
//! * **`moveCollidedEntities` / `moveStuckEntities`.** They read every
//!   entity whose bounding box meets the moving block's and push it. The
//!   only entities on this server are players and thrown items, neither of
//!   which any scenario stands in front of a piston; the two calls are
//!   empty here and the first scenario that pushes a player will find this
//!   paragraph.
//! * **Drops.** `moveBlocks` calls `dropResources` for every `DESTROY`
//!   block; this server has nothing to drop into, which is the same gap
//!   `destroy_block` already carries.
//! * **The pitch RNG.** `triggerEvent` draws `level.getRandom().nextFloat()`
//!   for the sound's pitch. The level's shared `RandomSource` is not
//!   modelled; the sound is `crate::sounds::piston_move`, whose pitch comes
//!   from `crate::sounds::drawn_pitch`. The oracle's projection drops pitch and seed, so this is
//!   unobservable on the wire and written down rather than hidden.
//! * **`PistonHeadBlock.playerWillDestroy`** (creative-mode double destroy)
//!   and `MovingPistonBlock.useWithoutItem` (a stray `moving_piston` with no
//!   entity is removed on click) are not here.
//!
//! # The sound goes out before the block event, and how that is arranged
//!
//! `triggerEvent` plays `PISTON_EXTEND`/`PISTON_CONTRACT` from inside
//! `runBlockEvents`, before `ServerLevel` broadcasts the
//! `ClientboundBlockEventPacket` — the vanilla recording of 380 carries the
//! sound one observation ahead of the event. The event packet is sent by
//! `crate::containers`, which this lane does not edit, so the sound cannot
//! be queued behind it: [`install_sound_sink`] lets the tick driver hand
//! this module a broadcaster once, and the sound leaves synchronously from
//! inside the event.

use std::sync::Mutex;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::piston::{
    block_of, facing_of, is_pushable, is_sticky_piston, moving_piston_state, resolve, LevelPiston,
    PistonWorld, PushReaction, TRIGGER_CONTRACT, TRIGGER_DROP, TRIGGER_EXTEND, WORLD_MAX_Y,
    WORLD_MIN_Y,
};
use crate::power::{Redstone, SignalTable};
use crate::shapes::{Effects, ShapeTable, RECURSION_LIMIT};

// ---------------------------------------------------------------------------
// PistonMovingBlockEntity
// ---------------------------------------------------------------------------

/// One `PistonMovingBlockEntity`: the fields its six-argument constructor
/// writes, plus the two progress floats and `lastTicked`.
#[derive(Debug, Clone, PartialEq)]
pub struct Moving {
    pub moved_state: String,
    pub direction: Face,
    pub extending: bool,
    pub is_source_piston: bool,
    pub progress: f32,
    pub progress_o: f32,
    pub last_ticked: i64,
}

impl Moving {
    #[must_use]
    pub fn new(
        moved_state: &str,
        direction: Face,
        extending: bool,
        is_source_piston: bool,
    ) -> Self {
        Self {
            moved_state: moved_state.to_string(),
            direction,
            extending,
            is_source_piston,
            progress: 0.0,
            progress_o: 0.0,
            last_ticked: 0,
        }
    }

    /// `getPushDirection`: `extending ? direction : direction.getOpposite()`.
    #[must_use]
    pub const fn push_direction(&self) -> Face {
        if self.extending {
            self.direction
        } else {
            self.direction.opposite()
        }
    }

    /// `getProgress(partialTick)` at partial tick 0, which is `progressO`.
    #[must_use]
    pub const fn progress_at_tick_start(&self) -> f32 {
        self.progress_o
    }
}

/// Every `PistonMovingBlockEntity` in the world, in `Level.blockEntityTickers`
/// order: the order `Level.setBlockEntity` registered them, a replacement at
/// an existing position keeping that position's slot.
#[derive(Debug, Default)]
pub struct MovingPistons {
    inner: Mutex<Vec<(BlockPos, Moving)>>,
}

impl MovingPistons {
    pub fn shared() -> &'static Self {
        static MOVING: std::sync::OnceLock<MovingPistons> = std::sync::OnceLock::new();
        MOVING.get_or_init(Self::default)
    }

    /// `Level.setBlockEntity`.
    pub fn insert(&self, pos: BlockPos, moving: Moving) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = inner.iter_mut().find(|(p, _)| *p == pos) {
            slot.1 = moving;
        } else {
            inner.push((pos, moving));
        }
    }

    #[must_use]
    pub fn get(&self, pos: BlockPos) -> Option<Moving> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .find(|(p, _)| *p == pos)
            .map(|(_, m)| m.clone())
    }

    /// `Level.removeBlockEntity` + `setRemoved`.
    pub fn remove(&self, pos: BlockPos) -> Option<Moving> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let i = inner.iter().position(|(p, _)| *p == pos)?;
        Some(inner.remove(i).1)
    }

    fn with_mut<T>(&self, pos: BlockPos, f: impl FnOnce(&mut Moving) -> T) -> Option<T> {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.iter_mut().find(|(p, _)| *p == pos).map(|(_, m)| f(m))
    }

    /// The positions to tick this tick, snapshotted: `tickBlockEntities`
    /// sets `tickingBlockEntities` and anything registered during the sweep
    /// goes to `pendingBlockEntityTickers` for next tick.
    #[must_use]
    pub fn positions(&self) -> Vec<BlockPos> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(p, _)| *p)
            .collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
}

// ---------------------------------------------------------------------------
// The sound sink
// ---------------------------------------------------------------------------

type SoundSink = Box<dyn Fn(&crate::sounds::Sound) + Send + Sync>;

static SOUND_SINK: std::sync::OnceLock<SoundSink> = std::sync::OnceLock::new();

/// Hand this module the broadcaster its sounds leave through. Installed once
/// by the tick driver; a second install is ignored. Without one the sounds
/// are dropped silently, which is the unit-test configuration.
pub fn install_sound_sink(sink: SoundSink) {
    let _ = SOUND_SINK.set(sink);
}

fn play(sound: crate::sounds::Sound) {
    if let Some(sink) = SOUND_SINK.get() {
        sink(&sound);
    }
}

// ---------------------------------------------------------------------------
// Block.updateFromNeighbourShapes and the finaliser
// ---------------------------------------------------------------------------

/// `Block.updateFromNeighbourShapes(state, level, pos)`, offsets 2-80: fold
/// `updateShape` over `UPDATE_SHAPE_ORDER` without writing.
#[must_use]
pub fn update_from_neighbour_shapes(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    state: &str,
    pos: BlockPos,
    updates: &mut Effects,
) -> String {
    let Some(shapes) = shapes else {
        return state.to_string();
    };
    let mut current = state.to_string();
    for d in crate::shapes::UPDATE_SHAPE_ORDER {
        let neighbour_pos = d.relative(pos);
        let neighbour_state = blocks.state_at(neighbour_pos);
        let neighbour_id = blocks.state_id(&neighbour_state);
        if let Some(next) = crate::shapes::shape_after(
            blocks,
            shapes,
            pos,
            &current,
            d,
            neighbour_pos,
            &neighbour_state,
            neighbour_id,
            updates,
        ) {
            current = next;
        }
    }
    current
}

#[must_use]
fn is_moving_piston(state: &str) -> bool {
    block_of(state) == "minecraft:moving_piston"
}

/// The `progressO >= 1.0F` arm of `tick` — shared with [`final_tick`], which
/// differs only in what it puts down for a source piston and in its flags.
fn finalise(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    moving: &Moving,
    updates: &mut Effects,
) {
    if !is_moving_piston(&blocks.state_at(pos)) {
        return;
    }
    let mut s = update_from_neighbour_shapes(blocks, shapes, &moving.moved_state, pos, updates);
    if crate::shapes::is_air(&s) {
        // 340 = SKIP_BLOCK_ENTITY_SIDEEFFECTS | MOVE_BY_PISTON | KNOWN_SHAPE
        // | INVISIBLE: the moved block lands unseen, then `updateOrDestroy`
        // takes it straight back out with flags 3, which is what the client
        // sees.
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &moving.moved_state,
            340,
            RECURSION_LIMIT,
            updates,
        );
        crate::shapes::update_or_destroy(
            blocks,
            shapes,
            &moving.moved_state,
            &s,
            pos,
            3,
            RECURSION_LIMIT,
            updates,
        );
        return;
    }
    if crate::placement::property_of(&s, "waterlogged") == Some("true") {
        s = crate::placement::with_property(&s, "waterlogged", "false");
    }
    crate::shapes::set_block(blocks, shapes, pos, &s, 67, RECURSION_LIMIT, updates);
    crate::power::neighbor_changed(blocks, signals, shapes, pos, block_of(&s), updates);
}

/// `PistonMovingBlockEntity.tick` for one entity.
fn tick_one(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    now: i64,
    updates: &mut Effects,
) {
    let registry = MovingPistons::shared();
    let Some(moving) = registry.with_mut(pos, |m| {
        m.last_ticked = now;
        m.progress_o = m.progress;
        m.clone()
    }) else {
        return;
    };
    if moving.progress_o >= 1.0 {
        registry.remove(pos);
        finalise(blocks, signals, shapes, pos, &moving, updates);
        return;
    }
    // `moveCollidedEntities` and `moveStuckEntities` go here; see the module
    // doc for why they are empty.
    let f = moving.progress + 0.5;
    registry.with_mut(pos, |m| {
        m.progress = if f >= 1.0 { 1.0 } else { f };
    });
}

/// `Level.tickBlockEntities` restricted to the piston entities: each in
/// registration order, over the set as it stood when the sweep began.
pub fn tick_all(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    now: i64,
    updates: &mut Effects,
) {
    for pos in MovingPistons::shared().positions() {
        tick_one(blocks, signals, shapes, pos, now, updates);
    }
}

/// `PistonMovingBlockEntity.finalTick`, offsets 1-148.
///
/// ```java
/// if (level != null && (progressO < 1.0F || level.isClientSide)) {
///     progress = 1.0F; progressO = progress;
///     level.removeBlockEntity(worldPosition); setRemoved();
///     if (level.getBlockState(worldPosition).is(Blocks.MOVING_PISTON)) {
///         BlockState s = isSourcePiston ? Blocks.AIR.defaultBlockState()
///                                       : Block.updateFromNeighbourShapes(movedState, level, worldPosition);
///         level.setBlock(worldPosition, s, 3);
///         level.neighborChanged(worldPosition, s.getBlock(), initialOrientation(level, getPushDirection(), null));
///     }
/// }
/// ```
pub fn final_tick(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) {
    let registry = MovingPistons::shared();
    let Some(moving) = registry.get(pos) else {
        return;
    };
    if moving.progress_o >= 1.0 {
        return;
    }
    registry.remove(pos);
    if !is_moving_piston(&blocks.state_at(pos)) {
        return;
    }
    let s = if moving.is_source_piston {
        "minecraft:air".to_string()
    } else {
        update_from_neighbour_shapes(blocks, shapes, &moving.moved_state, pos, updates)
    };
    crate::shapes::set_block(blocks, shapes, pos, &s, 3, RECURSION_LIMIT, updates);
    crate::power::neighbor_changed(blocks, signals, shapes, pos, block_of(&s), updates);
}

// ---------------------------------------------------------------------------
// PistonBaseBlock.moveBlocks
// ---------------------------------------------------------------------------

fn head_state(facing: Face, sticky: bool) -> String {
    format!(
        "minecraft:piston_head[facing={},short=false,type={}]",
        crate::piston::face_name(facing),
        if sticky { "sticky" } else { "normal" }
    )
}

/// `PistonBaseBlock.moveBlocks(level, pos, dir, extending)`, offsets 2-923.
/// Every write goes through [`crate::shapes::set_block`] with the flag the
/// jar uses at that site; the entities are registered in the order the jar
/// calls `setBlockEntity`.
#[expect(
    clippy::too_many_lines,
    reason = "one vanilla method, transcribed in its own order"
)]
pub fn move_blocks(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    dir: Face,
    extending: bool,
    updates: &mut Effects,
) -> bool {
    let pos2 = dir.relative(pos);
    if !extending && block_of(&blocks.state_at(pos2)) == "minecraft:piston_head" {
        crate::shapes::set_block(
            blocks,
            shapes,
            pos2,
            "minecraft:air",
            276,
            RECURSION_LIMIT,
            updates,
        );
    }
    let Some(signals) = signals else {
        return false;
    };
    let redstone = Redstone::new(blocks, signals, shapes);
    let world = LevelPiston::new(
        blocks,
        &redstone,
        crate::piston::shared_push_table(),
        WORLD_MIN_Y,
        WORLD_MAX_Y,
    );
    let Ok(resolved) = resolve(&world, pos, dir, extending) else {
        return false;
    };
    let to_push = resolved.to_push;
    let to_destroy = resolved.to_destroy;

    // `Map<BlockPos, BlockState> map = Maps.newHashMap()` — a `HashMap`, so
    // its two iterations below run in `BlockPos.hashCode` bucket order.
    let mut map: Vec<(BlockPos, String)> = Vec::new();
    let list2: Vec<String> = to_push.iter().map(|p| blocks.state_at(*p)).collect();
    for (p, s) in to_push.iter().zip(&list2) {
        map.push((*p, s.clone()));
    }
    let mut states: Vec<String> = Vec::with_capacity(to_destroy.len() + to_push.len());
    let push_dir = if extending { dir } else { dir.opposite() };

    for p in to_destroy.iter().rev() {
        let s = blocks.state_at(*p);
        // `dropResources(s, level, p, be)`: nothing to drop into here. The
        // `levelEvent(2001)` beside it is client-side only (offset 273).
        crate::shapes::set_block(
            blocks,
            shapes,
            *p,
            "minecraft:air",
            18,
            RECURSION_LIMIT,
            updates,
        );
        states.push(s);
    }
    for (k, p) in to_push.iter().enumerate().rev() {
        let s = blocks.state_at(*p);
        let p3 = push_dir.relative(*p);
        map.retain(|(mp, _)| *mp != p3);
        let moving = moving_piston_state(dir, false);
        crate::shapes::set_block(blocks, shapes, p3, &moving, 324, RECURSION_LIMIT, updates);
        MovingPistons::shared().insert(p3, Moving::new(&list2[k], dir, extending, false));
        states.push(s);
    }
    if extending {
        let sticky = is_sticky_piston(&blocks.state_at(pos));
        let head = head_state(dir, sticky);
        let moving = moving_piston_state(dir, sticky);
        map.retain(|(mp, _)| *mp != pos2);
        crate::shapes::set_block(blocks, shapes, pos2, &moving, 324, RECURSION_LIMIT, updates);
        MovingPistons::shared().insert(pos2, Moving::new(&head, dir, true, true));
    }

    let keys: Vec<BlockPos> = map.iter().map(|(p, _)| *p).collect();
    let ordered = crate::power::hash_set_order(&keys);
    for key in &ordered {
        crate::shapes::set_block(
            blocks,
            shapes,
            *key,
            "minecraft:air",
            82,
            RECURSION_LIMIT,
            updates,
        );
    }
    if let Some(shapes) = shapes {
        for key in &ordered {
            let Some((_, value)) = map.iter().find(|(p, _)| p == key) else {
                continue;
            };
            crate::shapes::update_indirect_neighbour_shapes(
                blocks,
                shapes,
                value,
                *key,
                2,
                RECURSION_LIMIT,
                updates,
            );
            crate::shapes::update_neighbour_shapes(
                blocks,
                shapes,
                *key,
                "minecraft:air",
                2,
                RECURSION_LIMIT,
                updates,
            );
            crate::shapes::update_indirect_neighbour_shapes(
                blocks,
                shapes,
                "minecraft:air",
                *key,
                2,
                RECURSION_LIMIT,
                updates,
            );
        }
    }

    let mut j = 0;
    for p in to_destroy.iter().rev() {
        let s = &states[j];
        j += 1;
        crate::removal::affect_neighbors_after_removal(
            blocks,
            Some(signals),
            shapes,
            crate::placedby::PlacedByTable::shared(std::path::Path::new(
                "data/vanilla/set_placed_by.json",
            ))
            .ok(),
            *p,
            s,
            false,
            updates,
        );
        if let Some(shapes) = shapes {
            crate::shapes::update_indirect_neighbour_shapes(
                blocks,
                shapes,
                s,
                *p,
                2,
                RECURSION_LIMIT,
                updates,
            );
        }
        crate::power::update_neighbors_at(blocks, Some(signals), shapes, *p, block_of(s), updates);
    }
    for p in to_push.iter().rev() {
        let s = &states[j];
        j += 1;
        crate::power::update_neighbors_at(blocks, Some(signals), shapes, *p, block_of(s), updates);
    }
    if extending {
        crate::power::update_neighbors_at(
            blocks,
            Some(signals),
            shapes,
            pos2,
            "minecraft:piston_head",
            updates,
        );
    }
    true
}

// ---------------------------------------------------------------------------
// PistonBaseBlock.triggerEvent
// ---------------------------------------------------------------------------

/// `PistonBaseBlock.triggerEvent(state, level, pos, b0, b1)`, offsets 1-573,
/// with every side effect. Returns what vanilla returns: whether the event
/// is delivered to clients.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's five arguments plus the three tables and the effects sink"
)]
pub fn trigger_event(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    b0: i32,
    b1: i32,
    updates: &mut Effects,
) -> bool {
    let Some(facing) = facing_of(state) else {
        return false;
    };
    let extended_state = crate::placement::with_property(state, "extended", "true");
    let Some(signals) = signals else {
        return false;
    };
    let redstone = Redstone::new(blocks, signals, shapes);
    let world = LevelPiston::new(
        blocks,
        &redstone,
        crate::piston::shared_push_table(),
        WORLD_MIN_Y,
        WORLD_MAX_Y,
    );
    let has_signal = crate::piston::get_neighbor_signal(&world, pos, facing);
    if has_signal && (b0 == TRIGGER_CONTRACT || b0 == TRIGGER_DROP) {
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &extended_state,
            2,
            RECURSION_LIMIT,
            updates,
        );
        return false;
    }
    if !has_signal && b0 == TRIGGER_EXTEND {
        return false;
    }

    if b0 == TRIGGER_EXTEND {
        if !move_blocks(blocks, Some(signals), shapes, pos, facing, true, updates) {
            return false;
        }
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &extended_state,
            67,
            RECURSION_LIMIT,
            updates,
        );
        play(crate::sounds::piston_move(pos, true));
        return true;
    }
    if b0 != TRIGGER_CONTRACT && b0 != TRIGGER_DROP {
        return true;
    }

    // Offsets 175-204: an entity still moving in front is finished first.
    final_tick(blocks, Some(signals), shapes, facing.relative(pos), updates);
    let sticky = is_sticky_piston(state);
    let moving = moving_piston_state(facing, sticky);
    crate::shapes::set_block(blocks, shapes, pos, &moving, 276, RECURSION_LIMIT, updates);
    // `this.defaultBlockState().setValue(FACING, Direction.from3DDataValue(b1 & 7))`.
    let base_facing = Face::from_3d(b1 & 7).unwrap_or(facing);
    let base = format!(
        "{}[extended=false,facing={}]",
        block_of(state),
        crate::piston::face_name(base_facing)
    );
    MovingPistons::shared().insert(pos, Moving::new(&base, facing, false, true));
    crate::power::update_neighbors_at(
        blocks,
        Some(signals),
        shapes,
        pos,
        "minecraft:moving_piston",
        updates,
    );
    if let Some(shapes) = shapes {
        crate::shapes::update_neighbour_shapes(
            blocks,
            shapes,
            pos,
            &moving,
            2,
            RECURSION_LIMIT,
            updates,
        );
    }

    if sticky {
        let pos2 = facing.relative(facing.relative(pos));
        let s2 = blocks.state_at(pos2);
        let mut finished = false;
        if is_moving_piston(&s2) {
            if let Some(be) = MovingPistons::shared().get(pos2) {
                if be.direction == facing && be.extending {
                    final_tick(blocks, Some(signals), shapes, pos2, updates);
                    finished = true;
                }
            }
        }
        if !finished {
            let pullable = b0 == TRIGGER_CONTRACT
                && !crate::shapes::is_air(&s2)
                && is_pushable(&world, &s2, pos2, facing.opposite(), false, facing)
                && (world.push_reaction(&s2) == PushReaction::Normal
                    || crate::piston::is_piston(block_of(&s2)));
            if pullable {
                move_blocks(blocks, Some(signals), shapes, pos, facing, false, updates);
            } else {
                remove_block(blocks, shapes, facing.relative(pos), updates);
            }
        }
    } else {
        remove_block(blocks, shapes, facing.relative(pos), updates);
    }
    play(crate::sounds::piston_move(pos, false));
    true
}

/// `Level.removeBlock(pos, false)`: `setBlock(pos, fluid.createLegacyBlock(), 3)`.
fn remove_block(
    blocks: &dyn Blocks,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut Effects,
) {
    let legacy = blocks.legacy_fluid_at(pos);
    crate::shapes::set_block(blocks, shapes, pos, &legacy, 3, RECURSION_LIMIT, updates);
}

/// `PistonHeadBlock.neighborChanged`, offsets 3-50: while the head can
/// survive, forward the change to the base with `PistonHeadBlock` as the
/// source — so the base's `checkIfExtend` runs when something beside the
/// head moves.
pub fn head_neighbor_changed(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut Effects,
) {
    let Some(facing) = facing_of(state) else {
        return;
    };
    let base_pos = facing.opposite().relative(pos);
    let base = blocks.state_at(base_pos);
    // `canSurvive`: a fitting base, or a `moving_piston` facing the same way.
    let survives = crate::removal::is_fitting_base(state, &base)
        || (is_moving_piston(&base)
            && crate::placement::property_of(&base, "facing")
                == crate::placement::property_of(state, "facing"));
    if survives {
        crate::power::neighbor_changed(
            blocks,
            signals,
            shapes,
            base_pos,
            "minecraft:piston_head",
            updates,
        );
    }
}

// ---------------------------------------------------------------------------
// Effects that could not leave through the caller
// ---------------------------------------------------------------------------

/// Effects produced inside a block event, which `crate::containers` drains
/// without a place to put them. The tick driver takes them right after
/// `run_block_events` and after [`tick_all`].
pub struct Pending {
    inner: Mutex<Effects>,
}

impl Pending {
    pub fn shared() -> &'static Self {
        static PENDING: std::sync::OnceLock<Pending> = std::sync::OnceLock::new();
        PENDING.get_or_init(|| Self {
            inner: Mutex::new(Effects::new()),
        })
    }

    pub fn push(&self, effects: Effects) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.blocks.extend(effects.blocks);
        // A drop counts the events before it within its own batch; offset it
        // by what this queue already holds, so it stays between the same two.
        let before = inner.level_events.len();
        inner.drops.extend(
            effects
                .drops
                .into_iter()
                .map(|d| crate::shapes::DropRequest {
                    after_events: d.after_events + before,
                    ..d
                }),
        );
        inner.level_events.extend(effects.level_events);
        inner.sounds.extend(effects.sounds);
        inner.ticks.extend(effects.ticks);
    }

    #[must_use]
    pub fn take(&self) -> Effects {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::replace(&mut *inner, Effects::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap as Map;

    /// [`MovingPistons::shared`] is one registry for the process and
    /// `tick_all` sweeps all of it, so two tests ticking at once would tick
    /// each other's entities against the wrong world. Each test holds this.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    struct World {
        states: Mutex<Map<BlockPos, String>>,
    }

    impl World {
        fn new(initial: &[(BlockPos, &str)]) -> Self {
            Self {
                states: Mutex::new(
                    initial
                        .iter()
                        .map(|(p, s)| (*p, (*s).to_string()))
                        .collect(),
                ),
            }
        }
        fn at(&self, pos: BlockPos) -> String {
            self.states
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at(pos)
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            let mut map = self.states.lock().unwrap();
            if map.get(&pos).map(String::as_str) == Some(state) {
                return None;
            }
            map.insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _state: &str) -> bool {
            true
        }
        fn state_id(&self, _state: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.at(pos))
        }
        fn face_sturdy(&self, pos: BlockPos, _face: Face) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn redstone_conductor(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }
        fn signal_source(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:redstone_block"
        }
        fn water_at(&self, _pos: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _pos: BlockPos) -> String {
            "minecraft:air".to_string()
        }
    }

    fn table() -> SignalTable {
        let json = r#"{
          "rng": "signal", "states": 2, "quiet": 1, "sources": 1, "refusedStates": 0,
          "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
          "refused": [], "unanswerable": [],
          "table": [
            {"state": "minecraft:redstone_block", "id": 1, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,0,0,0,0]}
          ]
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "rustmc-pistonmove-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("signal.json");
        std::fs::write(&path, json).unwrap();
        let out = SignalTable::load(&path).expect("fixture loads");
        let _ = std::fs::remove_file(&path);
        out
    }

    const PISTON_N: &str = "minecraft:piston[extended=false,facing=north]";
    const PISTON_N_EXT: &str = "minecraft:piston[extended=true,facing=north]";
    const STICKY_N: &str = "minecraft:sticky_piston[extended=false,facing=north]";

    fn far(x: i32) -> (BlockPos, BlockPos, BlockPos) {
        ((x, 0, 0), (x, 0, -1), (x, 0, -2))
    }

    /// 380's rig with a stone in front (390's): the event lands the piston
    /// extended and two invisible `moving_piston`s with entities; the head
    /// and the stone arrive two ticks later, in one tick, in the order the
    /// entities were registered — the pushed block's first, the head's second.
    #[test]
    fn an_extend_event_lands_the_head_and_the_pushed_block_two_ticks_later() {
        let _serial = serial();
        let t = table();
        let (piston, front, beyond) = far(100);
        let w = World::new(&[
            (piston, PISTON_N),
            (front, "minecraft:stone"),
            ((101, 0, 0), "minecraft:redstone_block"),
        ]);
        let registry = MovingPistons::shared();
        let mut fx = Effects::new();
        assert!(trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            PISTON_N,
            TRIGGER_EXTEND,
            2,
            &mut fx
        ));
        assert_eq!(w.at(piston), PISTON_N_EXT);
        assert_eq!(
            w.at(front),
            "minecraft:moving_piston[facing=north,type=normal]"
        );
        assert_eq!(
            w.at(beyond),
            "minecraft:moving_piston[facing=north,type=normal]"
        );
        assert!(
            fx.blocks.iter().any(|(p, _)| *p == piston),
            "flag 67 reaches clients"
        );
        assert!(
            !fx.blocks.iter().any(|(p, _)| *p == front || *p == beyond),
            "flag 324 does not: {:?}",
            fx.blocks
        );
        let stone = registry.get(beyond).expect("the pushed stone's entity");
        assert_eq!(stone.moved_state, "minecraft:stone");
        assert!(stone.extending && !stone.is_source_piston);
        let head = registry.get(front).expect("the head's entity");
        assert_eq!(
            head.moved_state,
            "minecraft:piston_head[facing=north,short=false,type=normal]"
        );
        assert!(head.extending && head.is_source_piston);
        assert_eq!(
            registry
                .positions()
                .iter()
                .filter(|p| **p == beyond || **p == front)
                .copied()
                .collect::<Vec<_>>(),
            vec![beyond, front],
            "pushed blocks register before the head"
        );

        // T: 0 -> 0.5. T+1: 0.5 -> 1.0. T+2: finalise.
        let mut fx = Effects::new();
        tick_all(&w, Some(&t), None, 10, &mut fx);
        assert_eq!(registry.get(front).unwrap().progress, 0.5);
        assert!(fx.blocks.is_empty());
        tick_all(&w, Some(&t), None, 11, &mut fx);
        assert_eq!(registry.get(front).unwrap().progress, 1.0);
        assert!(fx.blocks.is_empty());
        tick_all(&w, Some(&t), None, 12, &mut fx);
        assert_eq!(
            w.at(front),
            "minecraft:piston_head[facing=north,short=false,type=normal]"
        );
        assert_eq!(w.at(beyond), "minecraft:stone");
        assert!(registry.get(front).is_none() && registry.get(beyond).is_none());
        let landed: Vec<BlockPos> = fx.blocks.iter().map(|(p, _)| *p).collect();
        assert_eq!(landed, vec![beyond, front], "flag 67, in ticker order");
    }

    /// The retract event: the head goes at once (flags 3), the base becomes
    /// an invisible `moving_piston` whose entity puts the unextended piston
    /// back two ticks later with flags 67.
    #[test]
    fn a_contract_event_removes_the_head_now_and_restores_the_base_later() {
        let _serial = serial();
        let t = table();
        let (piston, front, _) = far(200);
        let w = World::new(&[
            (piston, PISTON_N_EXT),
            (
                front,
                "minecraft:piston_head[facing=north,short=false,type=normal]",
            ),
        ]);
        let mut fx = Effects::new();
        assert!(trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            PISTON_N_EXT,
            TRIGGER_CONTRACT,
            2,
            &mut fx
        ));
        assert_eq!(w.at(front), "minecraft:air");
        assert_eq!(
            w.at(piston),
            "minecraft:moving_piston[facing=north,type=normal]"
        );
        assert!(
            fx.blocks.iter().any(|(p, _)| *p == front),
            "removeBlock is flags 3"
        );
        assert!(
            !fx.blocks.iter().any(|(p, _)| *p == piston),
            "276 is invisible"
        );
        let be = MovingPistons::shared()
            .get(piston)
            .expect("the source entity");
        assert!(be.is_source_piston && !be.extending);
        assert_eq!(be.moved_state, PISTON_N);
        assert_eq!(be.push_direction(), Face::South);

        for now in 20..22 {
            tick_all(&w, Some(&t), None, now, &mut Effects::new());
        }
        let mut fx = Effects::new();
        tick_all(&w, Some(&t), None, 22, &mut fx);
        assert_eq!(w.at(piston), PISTON_N);
        assert_eq!(
            fx.blocks.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
            vec![piston]
        );
    }

    /// A sticky piston retracting pulls the block in front of its head back
    /// by one: `moveBlocks(pos, dir, false)`, whose resolver starts two
    /// blocks out and pushes towards the piston.
    #[test]
    fn a_sticky_piston_pulls_the_block_back() {
        let _serial = serial();
        let t = table();
        let (piston, front, beyond) = far(300);
        let w = World::new(&[
            (
                piston,
                "minecraft:sticky_piston[extended=true,facing=north]",
            ),
            (
                front,
                "minecraft:piston_head[facing=north,short=false,type=sticky]",
            ),
            (beyond, "minecraft:stone"),
        ]);
        let mut fx = Effects::new();
        assert!(trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            "minecraft:sticky_piston[extended=true,facing=north]",
            TRIGGER_CONTRACT,
            2,
            &mut fx
        ));
        assert_eq!(
            w.at(front),
            "minecraft:moving_piston[facing=north,type=normal]"
        );
        assert_eq!(w.at(beyond), "minecraft:air", "vacated with flags 82");
        let pulled = MovingPistons::shared()
            .get(front)
            .expect("the stone's entity");
        assert_eq!(pulled.moved_state, "minecraft:stone");
        assert!(!pulled.extending);
        for now in 30..33 {
            tick_all(&w, Some(&t), None, now, &mut Effects::new());
        }
        assert_eq!(w.at(front), "minecraft:stone");
        assert_eq!(w.at(piston), STICKY_N);
    }

    /// `TRIGGER_DROP` on a sticky piston drops the head without pulling.
    #[test]
    fn a_drop_event_on_a_sticky_piston_pulls_nothing() {
        let _serial = serial();
        let t = table();
        let (piston, front, beyond) = far(400);
        let w = World::new(&[
            (
                piston,
                "minecraft:sticky_piston[extended=true,facing=north]",
            ),
            (
                front,
                "minecraft:piston_head[facing=north,short=false,type=sticky]",
            ),
            (beyond, "minecraft:stone"),
        ]);
        let mut fx = Effects::new();
        trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            "minecraft:sticky_piston[extended=true,facing=north]",
            TRIGGER_DROP,
            2,
            &mut fx,
        );
        assert_eq!(w.at(front), "minecraft:air");
        assert_eq!(w.at(beyond), "minecraft:stone");
    }

    /// A contract event arriving while the extension is still moving finishes
    /// it first (`finalTick`): the source entity puts AIR down, flags 3.
    #[test]
    fn a_contract_during_an_extension_finishes_the_extension_first() {
        let _serial = serial();
        let t = table();
        let (piston, front, _) = far(500);
        let w = World::new(&[
            (piston, PISTON_N),
            ((501, 0, 0), "minecraft:redstone_block"),
        ]);
        let mut fx = Effects::new();
        assert!(trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            PISTON_N,
            TRIGGER_EXTEND,
            2,
            &mut fx
        ));
        assert!(MovingPistons::shared().get(front).is_some());
        // The redstone block leaves; the retract event follows.
        w.set_block((501, 0, 0), "minecraft:air");
        let mut fx = Effects::new();
        assert!(trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            PISTON_N_EXT,
            TRIGGER_CONTRACT,
            2,
            &mut fx
        ));
        assert!(
            MovingPistons::shared().get(front).is_none(),
            "finalTick removed it"
        );
        assert_eq!(
            w.at(front),
            "minecraft:air",
            "a source entity finalises to AIR"
        );
    }

    /// `triggerEvent`'s first refusal: a retract event that finds the piston
    /// re-powered writes `extended=true` with flags 2 and delivers nothing.
    #[test]
    fn a_contract_event_on_a_repowered_piston_is_refused_with_a_flag_two_write() {
        let _serial = serial();
        let t = table();
        let (piston, _, _) = far(600);
        let w = World::new(&[
            (piston, PISTON_N_EXT),
            ((601, 0, 0), "minecraft:redstone_block"),
        ]);
        let mut fx = Effects::new();
        assert!(!trigger_event(
            &w,
            Some(&t),
            None,
            piston,
            PISTON_N_EXT,
            TRIGGER_CONTRACT,
            2,
            &mut fx
        ));
        assert!(fx.ticks.is_empty());
        assert!(MovingPistons::shared().get(piston).is_none());
    }

    #[test]
    fn the_registry_keeps_registration_order_and_replaces_in_place() {
        let _serial = serial();
        let r = MovingPistons::shared();
        let a = (700, 0, 0);
        let b = (700, 0, 1);
        r.insert(a, Moving::new("minecraft:stone", Face::North, true, false));
        r.insert(b, Moving::new("minecraft:dirt", Face::North, true, false));
        r.insert(a, Moving::new("minecraft:sand", Face::North, true, false));
        let order: Vec<_> = r.positions().into_iter().filter(|p| p.0 == 700).collect();
        assert_eq!(order, vec![a, b]);
        assert_eq!(r.get(a).unwrap().moved_state, "minecraft:sand");
        r.remove(a);
        r.remove(b);
    }
}
