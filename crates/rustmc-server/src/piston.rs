//! `PistonBaseBlock` — the block-event behaviour behind `380` and `390`.
//!
//! # Why this module exists at all, and what it is not
//!
//! The block-event **transport** already shipped: [`crate::containers`] holds
//! `Level.blockEvent`'s queue, `ServerLevel.runBlockEvents`'s drain and
//! `ClientboundBlockEventPacket`'s body. What it did not hold is a block that
//! *produces* one. `trigger_event` matched `minecraft:chest` and nothing
//! else, so every piston event queued in this server was queued and then
//! silently discarded at the gate.
//!
//! # The distinction that decides the whole design
//!
//! Read off the pinned jar's `setBlock` flags, which is the only place it is
//! written down. `Block.UPDATE_CLIENTS` is bit 2, and a `setBlock` without
//! it changes the server's world and tells no client. On a piston extension
//! vanilla calls `setBlock` six ways, and they split cleanly:
//!
//! | write | flag | bit 2 | reaches a client |
//! |---|---|---|---|
//! | the piston's own `extended=true` | 67 | yes | **yes** |
//! | a `PushReaction.DESTROY` block cleared | 18 | yes | **yes** |
//! | the vacated source positions | 82 | yes | **yes** |
//! | `finalTick`'s arrival at the destination | 3 | yes | **yes** |
//! | every `moving_piston` placement in `moveBlocks` | 324 | no | **no** |
//! | the retracting piston's own `moving_piston` | 276 | no | **no** |
//!
//! 67 = `UPDATE_MOVE_BY_PISTON|UPDATE_CLIENTS|UPDATE_NEIGHBORS`, 324 =
//! `UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS|UPDATE_MOVE_BY_PISTON|UPDATE_INVISIBLE`,
//! 276 = `…SIDEEFFECTS|UPDATE_KNOWN_SHAPE|UPDATE_INVISIBLE`. Every constant
//! is read from `Block`'s own `static final int`s, not from memory.
//!
//! So: **the `moving_piston` blocks and their `PistonMovingBlockEntity` are
//! server-side bookkeeping that no client is ever told about.** They are
//! created with `Level.setBlockEntity`, which sends nothing, and placed with
//! a flag that has no `UPDATE_CLIENTS`. The client is told exactly one thing
//! — the `block_event` packet carrying `(b0, b1)` — and **predicts** the
//! entire two-tick animation from it locally.
//!
//! That is why this is a **packet-and-ordering** port rather than a
//! block-entity one, and it is worth stating plainly because the opposite
//! reading is the expensive one: an implementation that faithfully models
//! `PistonMovingBlockEntity` and broadcasts it would put packets on the wire
//! that vanilla never sends, and would still not move the piston, because
//! what moves it on screen is the client's own prediction off `b0`.
//!
//! # What is ported here and what is not
//!
//! Ported: the event's **production** (`checkIfExtend`), its **encoding**
//! (`b0`, `b1`), and its **delivery gate** (`triggerEvent`'s three refusals),
//! plus the two world-reading predicates those need —
//! `PistonBaseBlock.getNeighborSignal` and `PistonBaseBlock.isPushable` — and
//! `PistonStructureResolver.resolve` restricted to its non-sticky arm.
//!
//! **Not ported, deliberately, and the boundary is here rather than in a
//! commit message:** `PistonMovingBlockEntity`'s tick, `progress`/`progressO`
//! interpolation and `finalTick`; entity movement
//! (`moveCollidedEntities`, `moveStuckEntities`, `PistonMath`); the sticky
//! pull-back on retraction; `PistonStructureResolver.addBranchingBlocks` and
//! `reorderListAtCollision`, which fire only for `slime_block` and
//! `honey_block`; and `MovingPistonBlock`'s shapes, drops and clone item.
//! A push whose line contains slime or honey is therefore refused by
//! [`resolve`] rather than approximated — see its `Err` arm.

use crate::interact::Face;
use crate::placement::property_of;

/// A block position, matching the rest of this crate.
pub type BlockPos = (i32, i32, i32);

/// `PistonBaseBlock.TRIGGER_EXTEND`.
pub const TRIGGER_EXTEND: i32 = 0;
/// `PistonBaseBlock.TRIGGER_CONTRACT`.
pub const TRIGGER_CONTRACT: i32 = 1;
/// `PistonBaseBlock.TRIGGER_DROP`.
pub const TRIGGER_DROP: i32 = 2;

/// `Level.getMinY()` for the only dimension this server serves.
pub const WORLD_MIN_Y: i32 = -64;
/// `Level.getMaxY()` — **inclusive**: `minY + height - 1`, so 319 and not
/// 320. An exclusive reading makes `isPushable`'s `pushDir == UP && y ==
/// maxY` refusal fire one block too high, which is invisible until something
/// pushes a block at the build limit.
pub const WORLD_MAX_Y: i32 = WORLD_MIN_Y + 384 - 1;

/// `PistonStructureResolver.MAX_PUSH_DEPTH`.
pub const MAX_PUSH_DEPTH: usize = 12;

/// `Direction.values()` — the iteration order `getNeighborSignal` walks, and
/// the order `get3DDataValue` numbers.
pub const DIRECTIONS: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// The two blocks `PistonBaseBlock` is registered as.
#[must_use]
pub fn is_piston(block: &str) -> bool {
    matches!(block, "minecraft:piston" | "minecraft:sticky_piston")
}

/// The world a piston reads, narrowed to the four questions it asks.
///
/// A trait rather than a concrete type because `triggerEvent` **re-reads the
/// world**, and that is the whole reason the chest-shaped
/// `trigger_event(block, b0, b1)` predicate could not express a piston: the
/// chest's answer is a constant, the piston's is a function of the world at
/// the moment the queue drains.
pub trait PistonWorld {
    /// `Level.getBlockState(pos)`, as this server's state string.
    fn state_at(&self, pos: BlockPos) -> String;

    /// `SignalGetter.hasSignal(pos, direction)`.
    fn has_signal(&self, pos: BlockPos, dir: Face) -> bool;

    /// `Level.getMinY()`.
    fn min_y(&self) -> i32;

    /// `Level.getMaxY()`.
    fn max_y(&self) -> i32;

    /// `BlockState.getPistonPushReaction()`.
    fn push_reaction(&self, state: &str) -> PushReaction;

    /// `BlockState.getDestroySpeed(level, pos) == -1.0F` — unbreakable.
    fn is_unbreakable(&self, state: &str) -> bool;

    /// `BlockState.hasBlockEntity()`.
    fn has_block_entity(&self, state: &str) -> bool;

    /// The live level behind this world, when there is one. A `Some` lets
    /// [`trigger_event`] run the whole of `PistonBaseBlock.triggerEvent`
    /// through `crate::pistonmove` — writes, entities, neighbour updates and
    /// all — instead of returning a list of raw writes; the test fakes leave
    /// it `None` and keep the write list.
    fn blocks(&self) -> Option<&dyn crate::edits::Blocks> {
        None
    }
}

/// `net.minecraft.world.level.material.PushReaction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushReaction {
    Normal,
    Destroy,
    Block,
    PushOnly,
}

/// The block name of a state string — everything before the `[`.
#[must_use]
pub fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// `state.getValue(FACING)` for a piston.
#[must_use]
pub fn facing_of(state: &str) -> Option<Face> {
    Face::from_name(property_of(state, "facing")?)
}

/// `state.getValue(EXTENDED)` for a piston.
#[must_use]
pub fn extended_of(state: &str) -> bool {
    property_of(state, "extended") == Some("true")
}

/// `PistonBaseBlock.getNeighborSignal` — the quasi-connectivity rule.
///
/// Three passes, in vanilla's order, and the middle one is the one that gets
/// dropped by anybody porting this from a description rather than the
/// bytecode:
///
/// 1. every direction **except the piston's own facing**, tested at the
///    neighbour position and with that same direction as the argument;
/// 2. `hasSignal(pos, DOWN)` at the piston's **own** position — not a
///    neighbour, and not skipped when `facing == DOWN`;
/// 3. every direction **except `DOWN`**, tested around `pos.above()`.
///
/// Pass 3 is why a piston answers to a redstone block sitting on top of the
/// block above it. `facing` is excluded in pass 1 only, so a downward-facing
/// piston still reads pass 2.
#[must_use]
pub fn get_neighbor_signal(world: &dyn PistonWorld, pos: BlockPos, facing: Face) -> bool {
    for dir in DIRECTIONS {
        if dir != facing && world.has_signal(dir.relative(pos), dir) {
            return true;
        }
    }
    if world.has_signal(pos, Face::Down) {
        return true;
    }
    let above = Face::Up.relative(pos);
    for dir in DIRECTIONS {
        if dir != Face::Down && world.has_signal(dir.relative(above), dir) {
            return true;
        }
    }
    false
}

/// The four blocks `isPushable` names individually.
const IMMOVABLE: [&str; 4] = [
    "minecraft:obsidian",
    "minecraft:crying_obsidian",
    "minecraft:respawn_anchor",
    "minecraft:reinforced_deepslate",
];

/// `PistonBaseBlock.isPushable`.
///
/// The argument order is vanilla's and the two `Direction`s are **not** the
/// same one: `push_dir` is where the structure is travelling, `piston_facing`
/// is where the piston points. They differ on a retraction, and
/// `PushReaction.PUSH_ONLY` compares against `piston_facing`, so folding them
/// into one parameter would be right for every extension and wrong for every
/// pull.
#[must_use]
pub fn is_pushable(
    world: &dyn PistonWorld,
    state: &str,
    pos: BlockPos,
    push_dir: Face,
    allow_destroy: bool,
    piston_facing: Face,
) -> bool {
    if pos.1 < world.min_y() || pos.1 > world.max_y() {
        return false;
    }
    if block_of(state) == "minecraft:air" {
        return true;
    }
    if IMMOVABLE.contains(&block_of(state)) {
        return false;
    }
    if push_dir == Face::Down && pos.1 == world.min_y() {
        return false;
    }
    if push_dir == Face::Up && pos.1 == world.max_y() {
        return false;
    }
    if is_piston(block_of(state)) {
        if extended_of(state) {
            return false;
        }
    } else {
        if world.is_unbreakable(state) {
            return false;
        }
        match world.push_reaction(state) {
            PushReaction::Block => return false,
            PushReaction::Destroy => return allow_destroy,
            PushReaction::PushOnly => return push_dir == piston_facing,
            PushReaction::Normal => {}
        }
    }
    !world.has_block_entity(state)
}

/// `PistonStructureResolver.isSticky` — `slime_block` or `honey_block`.
#[must_use]
pub fn is_sticky(state: &str) -> bool {
    matches!(
        block_of(state),
        "minecraft:slime_block" | "minecraft:honey_block"
    )
}

/// What [`resolve`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// `PistonStructureResolver.getToPush()`, in vanilla's order.
    pub to_push: Vec<BlockPos>,
    /// `PistonStructureResolver.getToDestroy()`.
    pub to_destroy: Vec<BlockPos>,
}

/// Why a resolve did not produce a movable structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// `resolve` returned `false` — vanilla refuses this push too.
    Refused,
    /// The line reached `slime_block` or `honey_block`, whose
    /// `addBranchingBlocks` arm is **not ported**. Vanilla might move this;
    /// this server declines to guess.
    ///
    /// Kept separate from [`Self::Refused`] on purpose: collapsing them would
    /// make an unported branch indistinguishable from a real vanilla refusal,
    /// which is precisely the kind of green that cannot be turned red.
    StickyUnported(BlockPos),
}

/// `PistonStructureResolver.resolve`, restricted to its non-sticky arm.
///
/// Vanilla's body is
///
/// ```text
/// toPush.clear(); toDestroy.clear();
/// state = level.getBlockState(startPos);
/// if (!isPushable(state, level, startPos, pushDirection, false, pistonDirection)) {
///     if (state.getPistonPushReaction() == DESTROY) { toDestroy.add(startPos); return true; }
///     return false;
/// }
/// if (!addBlockLine(startPos, pushDirection)) return false;
/// for (i = 0; i < toPush.size(); i++)
///     if (isSticky(getBlockState(toPush.get(i))) && !addBranchingBlocks(toPush.get(i))) return false;
/// return true;
/// ```
///
/// and the loop at the end is the only part not here — it fires only for the
/// two sticky blocks, which is why refusing on them costs nothing outside
/// slime and honey contraptions and keeps this function honest about its
/// boundary.
///
/// # Errors
/// [`Unresolved::Refused`] when vanilla refuses, [`Unresolved::StickyUnported`]
/// when the line reaches an unported sticky block.
pub fn resolve(
    world: &dyn PistonWorld,
    piston_pos: BlockPos,
    piston_facing: Face,
    extending: bool,
) -> Result<Resolved, Unresolved> {
    let push_direction = if extending {
        piston_facing
    } else {
        piston_facing.opposite()
    };
    let start_pos = if extending {
        piston_facing.relative(piston_pos)
    } else {
        piston_facing.relative(piston_facing.relative(piston_pos))
    };

    let state = world.state_at(start_pos);
    if !is_pushable(
        world,
        &state,
        start_pos,
        push_direction,
        false,
        push_direction,
    ) {
        if world.push_reaction(&state) == PushReaction::Destroy {
            return Ok(Resolved {
                to_push: Vec::new(),
                to_destroy: vec![start_pos],
            });
        }
        return Err(Unresolved::Refused);
    }

    let mut out = Resolved {
        to_push: Vec::new(),
        to_destroy: Vec::new(),
    };
    add_block_line(world, &mut out, piston_pos, push_direction, start_pos)?;

    // `for (i = 0; i < toPush.size(); i++) if (isSticky(...) &&
    // !addBranchingBlocks(...)) return false;` — the branching arm is
    // unported, so a sticky block reaching this loop refuses rather than
    // being treated as an ordinary pushed block.
    for pos in out.to_push.clone() {
        if is_sticky(&world.state_at(pos)) {
            return Err(Unresolved::StickyUnported(pos));
        }
    }
    Ok(out)
}

/// `pos.relative(dir, n)`.
#[must_use]
fn relative_n(pos: BlockPos, dir: Face, n: i32) -> BlockPos {
    let d = dir.normal();
    (pos.0 + d.0 * n, pos.1 + d.1 * n, pos.2 + d.2 * n)
}

/// `PistonStructureResolver.addBlockLine(pos, dir)`, non-sticky arm.
///
/// **The four early returns are `true`, not `false`, and that is the trap.**
/// Air, an unpushable block, the piston's own position and an
/// already-collected position all mean "this line is finished and it is
/// fine", not "this push is refused". Reading them as refusals inverts every
/// piston that has anything solid two blocks ahead of it.
///
/// The two genuine refusals are the depth test — `count + toPush.size() > 12`
/// on the way in and `toPush.size() >= 12` inside the forward walk — and
/// running into the piston itself from in front.
///
/// The forward walk's `isPushable` passes **`allowDestroy = true`** where the
/// entry test passes `false`. That asymmetry is load-bearing: a `DESTROY`
/// block at the head of the line refuses the whole push, while the same block
/// further along is collected into `to_destroy` and broken.
fn add_block_line(
    world: &dyn PistonWorld,
    out: &mut Resolved,
    piston_pos: BlockPos,
    push_direction: Face,
    start: BlockPos,
) -> Result<(), Unresolved> {
    let state = world.state_at(start);
    if block_of(&state) == "minecraft:air" {
        return Ok(());
    }
    if !is_pushable(world, &state, start, push_direction, false, push_direction) {
        return Ok(());
    }
    if start == piston_pos {
        return Ok(());
    }
    if out.to_push.contains(&start) {
        return Ok(());
    }

    // `while (isSticky(state)) { … }` walks the line *backwards* through
    // sticky blocks before anything is collected, so `count` can exceed 1.
    // Unported: with slime and honey refused, `count` is always 1 and the
    // reverse-order emission below degenerates to pushing `start` itself.
    if is_sticky(&state) {
        return Err(Unresolved::StickyUnported(start));
    }
    let count: usize = 1;
    if count + out.to_push.len() > MAX_PUSH_DEPTH {
        return Err(Unresolved::Refused);
    }
    for k in (0..count).rev() {
        out.to_push.push(relative_n(
            start,
            push_direction.opposite(),
            i32::try_from(k).unwrap_or(0),
        ));
    }

    let mut m: i32 = 1;
    loop {
        let ahead = relative_n(start, push_direction, m);
        if out.to_push.contains(&ahead) {
            // `reorderListAtCollision` + the `addBranchingBlocks` sweep.
            // Reachable only once a back-walk or a branch has put a position
            // ahead of the cursor into `toPush`, both of which are sticky-only.
            return Err(Unresolved::StickyUnported(ahead));
        }
        let state = world.state_at(ahead);
        if block_of(&state) == "minecraft:air" {
            return Ok(());
        }
        if !is_pushable(world, &state, ahead, push_direction, true, push_direction) {
            return Ok(());
        }
        if ahead == piston_pos {
            return Err(Unresolved::Refused);
        }
        if world.push_reaction(&state) == PushReaction::Destroy {
            out.to_destroy.push(ahead);
            return Ok(());
        }
        if out.to_push.len() >= MAX_PUSH_DEPTH {
            return Err(Unresolved::Refused);
        }
        out.to_push.push(ahead);
        m += 1;
    }
}

// ---------------------------------------------------------------------------
// The event: production, encoding, delivery
// ---------------------------------------------------------------------------

/// One `Level.setBlock(pos, state, flags)` a piston performs.
///
/// The `flags` are kept as vanilla's raw int rather than decoded into a bool,
/// because [`Write::reaches_clients`] is the only bit this server reads today
/// and the others have to survive until something reads them. A decoded
/// `broadcast: bool` would throw away `UPDATE_MOVE_BY_PISTON`, which is what
/// tells `neighborChanged` that a piston moved the block — the flag whose
/// absence is invisible until the first observer next to a piston.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub pos: BlockPos,
    pub state: String,
    pub flags: i32,
}

impl Write {
    /// `Block.UPDATE_CLIENTS`.
    pub const UPDATE_CLIENTS: i32 = 2;

    /// Whether this write produces a block update on the wire.
    ///
    /// Every `moving_piston` placement answers **false**, which is the single
    /// most important fact in this module.
    #[must_use]
    pub const fn reaches_clients(&self) -> bool {
        self.flags & Self::UPDATE_CLIENTS != 0
    }
}

/// What `PistonBaseBlock.triggerEvent` decided.
///
/// **Not `Eq`, and it cannot become one again.** [`Self::sound`] carries a
/// pitch, which is an `f32` drawn per event — `nextFloat() * 0.25f + 0.6f` for
/// an extension — so two `Outcome`s that describe the same piston firing are
/// not equal and no total order over them exists. `PartialEq` is kept because
/// the tests compare write lists; a derived `Eq` on a type holding a drawn
/// float would be claiming a reflexivity the type does not have.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// `triggerEvent`'s return value — whether `runBlockEvents` sends the
    /// `block_event` packet.
    pub deliver: bool,
    /// The world writes, in vanilla's order. Performed **whether or not**
    /// [`Self::deliver`] is set: the `has_signal && b0 != 0` refusal still
    /// writes `extended=true` with `UPDATE_CLIENTS`, so a client sees the
    /// piston stay out with no event to explain it.
    pub writes: Vec<Write>,
    /// `PISTON_EXTEND` or `PISTON_CONTRACT`, when this event played one.
    ///
    /// `None` on all three refusal arms, and that is a claim rather than an
    /// omission: vanilla's two `playSound` calls sit at offsets 118 and 531,
    /// both **past** every `return false`, so a refused event is silent. A
    /// port that played the sound before the refusal would give a piston that
    /// clicks and does not move.
    ///
    /// Sent by the caller rather than here, immediately and before the
    /// `block_event` packet, because that is where vanilla's is:
    /// `Level.playSound` reaches `PlayerList.broadcast` with no queue, while
    /// `runBlockEvents` broadcasts the event only after `doBlockEvent`
    /// returns true.
    pub sound: Option<crate::sounds::Sound>,
}

/// `PistonBaseBlock.checkIfExtend` — what, if anything, to queue.
///
/// Returns `Some((b0, b1))` for `Level.blockEvent(pos, this, b0, b1)`.
/// `b1` is always `facing.get3DDataValue()`; `b0` selects extend, contract or
/// drop.
///
/// # The drop arm is reachable in vanilla and not here
///
/// `TRIGGER_DROP` is chosen when the block two ahead is a `moving_piston`
/// facing the same way whose `PistonMovingBlockEntity` is still extending and
/// either under half-way, ticked this same game tick, or caught mid
/// `isHandlingTick`. This server places no `moving_piston` with a block
/// entity — see the module header — so that test can never fire and `b0` is
/// always [`TRIGGER_CONTRACT`] on a retraction. **That is a divergence, not a
/// simplification**, and it is named here rather than left to be found: a
/// contraption that retracts a piston into a still-extending one gets `1`
/// from this server where vanilla sends `2`.
#[must_use]
pub fn check_if_extend(world: &dyn PistonWorld, pos: BlockPos, state: &str) -> Option<(i32, i32)> {
    let facing = facing_of(state)?;
    let has_signal = get_neighbor_signal(world, pos, facing);
    let extended = extended_of(state);

    if has_signal && !extended {
        if resolve(world, pos, facing, true).is_ok() {
            return Some((TRIGGER_EXTEND, facing.to_3d()));
        }
        return None;
    }
    if !has_signal && extended {
        // Offsets 94-220: `b = 1`, unless two blocks out sits a
        // `moving_piston` facing this way whose `PistonMovingBlockEntity`
        // is extending and (`getProgress(0) < 0.5 || gameTime == lastTicked
        // || server.isHandlingTick()`) — and the last of those three is
        // true everywhere this can run, so an extension still in flight
        // always makes it `TRIGGER_DROP`. Reachable since
        // `crate::pistonmove` keeps the entities.
        let two_out = facing.relative(facing.relative(pos));
        let ahead = world.state_at(two_out);
        let dropping = block_of(&ahead) == "minecraft:moving_piston"
            && property_of(&ahead, "facing") == Some(face_name(facing))
            && crate::pistonmove::MovingPistons::shared()
                .get(two_out)
                .is_some_and(|be| be.extending);
        let b0 = if dropping {
            TRIGGER_DROP
        } else {
            TRIGGER_CONTRACT
        };
        return Some((b0, facing.to_3d()));
    }
    None
}

/// `PistonBaseBlock.triggerEvent`.
///
/// Three `return false`s and one `return true`, and getting the false arms
/// wrong is what makes a piston "work" while sending packets vanilla does
/// not:
///
/// * a **signal is present** and the event is a contract or a drop — the
///   piston was re-powered between queueing and draining, so it stays out:
///   `setBlock(pos, extended=true, UPDATE_CLIENTS)` and no packet;
/// * **no signal** and the event is an extend — symmetric, and nothing is
///   written at all;
/// * an extend whose `moveBlocks` fails, which is `resolve` failing a second
///   time at drain against a world that has moved since `checkIfExtend`.
///
/// The `!level.isClientSide` guard around the first two is a no-op on a
/// server and is dropped rather than modelled.
#[must_use]
pub fn trigger_event(
    world: &dyn PistonWorld,
    state: &str,
    pos: BlockPos,
    b0: i32,
    b1: i32,
) -> Outcome {
    let refuse = || Outcome {
        deliver: false,
        writes: Vec::new(),
        sound: None,
    };
    if let Some(blocks) = world.blocks() {
        // The live path: `crate::pistonmove` does the writing itself, with
        // vanilla's flags, entities and neighbour updates, and its effects
        // wait in `Pending` for the tick driver. Nothing is left for the
        // caller to write.
        let mut effects = crate::shapes::Effects::new();
        let deliver = crate::pistonmove::trigger_event(
            blocks,
            crate::power::shared(),
            crate::shapes::shared_with_classes(std::path::Path::new(".")),
            pos,
            state,
            b0,
            b1,
            &mut effects,
        );
        crate::pistonmove::Pending::shared().push(effects);
        return Outcome {
            deliver,
            writes: Vec::new(),
            // `crate::pistonmove` plays it through its own sink, from inside
            // the event and ahead of the block-event packet.
            sound: None,
        };
    }
    let Some(facing) = facing_of(state) else {
        return refuse();
    };
    let extended_state = crate::placement::with_property(state, "extended", "true");
    let has_signal = get_neighbor_signal(world, pos, facing);

    if has_signal && (b0 == TRIGGER_CONTRACT || b0 == TRIGGER_DROP) {
        return Outcome {
            deliver: false,
            writes: vec![Write {
                pos,
                state: extended_state,
                flags: 2,
            }],
            sound: None,
        };
    }
    if !has_signal && b0 == TRIGGER_EXTEND {
        return refuse();
    }

    if b0 == TRIGGER_EXTEND {
        let Ok(resolved) = resolve(world, pos, facing, true) else {
            return refuse();
        };
        let mut writes = move_blocks_extend(pos, facing, &resolved);
        // `setBlock(pos, extendedState, 67)` — the piston's own state, the
        // one write on an extension a client is told about besides the
        // vacated positions.
        writes.push(Write {
            pos,
            state: extended_state,
            flags: 67,
        });
        return Outcome {
            deliver: true,
            writes,
            sound: Some(crate::sounds::piston_move(pos, true)),
        };
    }

    // `TRIGGER_CONTRACT` / `TRIGGER_DROP`.
    //
    // First, common to both piston kinds: a `moving_piston` at the piston's
    // own position carrying the head, at flag 276 — **no `UPDATE_CLIENTS`**.
    // The client is told by the packet alone and animates the head coming
    // home itself.
    let mut writes = vec![Write {
        pos,
        state: moving_piston_state(facing, is_sticky_piston(state)),
        flags: 276,
    }];

    if is_sticky_piston(state) {
        // `if (this.isSticky) { … }` is the pull-back: it reads what is two
        // blocks ahead, and either `moveBlocks(level, pos, dir, false)` when
        // that is another piston or `removeBlock(pos.relative(dir), false)`
        // when it is not. **Neither is ported** — the analysis in between is
        // the sticky arm named in this module's header — so nothing is
        // guessed at here and the head is left where it is rather than
        // removed on a coin flip.
        sticky_retract_unported_once(pos);
    } else {
        // `level.removeBlock(pos.relative(dir), false)`.
        //
        // `Level.removeBlock(pos, isMoving)` is
        // `setBlock(pos, getFluidState(pos).createLegacyBlock(),
        //           3 | (isMoving ? 64 : 0))`, and `isMoving` is `false`
        // here — so **flag 3, which is broadcast**. The head really does
        // vanish from every client's world; only its animation is predicted.
        //
        // The fluid's legacy block rather than air is why this reads the
        // world: a waterlogged position retracts to water, not to air.
        writes.push(Write {
            pos: facing.relative(pos),
            state: legacy_fluid_block(&world.state_at(facing.relative(pos))),
            flags: 3,
        });
    }

    Outcome {
        deliver: true,
        writes,
        sound: Some(crate::sounds::piston_move(pos, false)),
    }
}

/// `FluidState.createLegacyBlock()` for what `Level.removeBlock` leaves
/// behind.
///
/// Air unless the position held water or lava. This server carries no
/// waterlogging on a piston head today, so the branch is one line rather than
/// a fluid model — but it is a branch rather than a constant `air`, because
/// the constant is wrong the moment a head is placed in water and nothing
/// would say so.
#[must_use]
fn legacy_fluid_block(state: &str) -> String {
    match block_of(state) {
        "minecraft:water" => "minecraft:water[level=0]".to_string(),
        "minecraft:lava" => "minecraft:lava[level=0]".to_string(),
        _ => "minecraft:air".to_string(),
    }
}

/// Warn once per position that a sticky piston retracted without its
/// pull-back.
fn sticky_retract_unported_once(pos: BlockPos) {
    static SAID: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<BlockPos>>> =
        std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let mut said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.insert(pos) {
        eprintln!(
            "piston: sticky retraction at {pos:?} — the pull-back is not ported, so the head \
             is left in place and nothing is pulled back"
        );
    }
}

/// Whether a piston state is the sticky variant — `PistonBaseBlock.isSticky`,
/// which is a constructor field rather than a block property.
#[must_use]
pub fn is_sticky_piston(state: &str) -> bool {
    block_of(state) == "minecraft:sticky_piston"
}

/// `Blocks.MOVING_PISTON.defaultBlockState().setValue(FACING, …).setValue(TYPE, …)`.
#[must_use]
pub fn moving_piston_state(facing: Face, sticky: bool) -> String {
    let ty = if sticky { "sticky" } else { "normal" };
    format!(
        "minecraft:moving_piston[facing={},type={}]",
        face_name(facing),
        ty
    )
}

/// `Direction`'s lowercase name, as it appears in a state string.
#[must_use]
pub const fn face_name(face: Face) -> &'static str {
    match face {
        Face::Down => "down",
        Face::Up => "up",
        Face::North => "north",
        Face::South => "south",
        Face::West => "west",
        Face::East => "east",
    }
}

/// `PistonBaseBlock.moveBlocks`'s writes for an extension.
///
/// In vanilla's order, and with vanilla's flags. The order matters because
/// the vacated positions are computed from the set of pushed positions minus
/// the positions the structure now occupies, so a write performed early
/// changes what "vacated" means.
///
/// The `to_destroy` entries also `dropResources` and fire `levelEvent(2001)`;
/// neither is modelled here and both are named in the module header's
/// boundary.
#[must_use]
fn move_blocks_extend(piston_pos: BlockPos, facing: Face, resolved: &Resolved) -> Vec<Write> {
    let mut writes = Vec::new();

    // `for (destroy) { dropResources; levelEvent(2001); setBlock(AIR, 18) }`
    for pos in &resolved.to_destroy {
        writes.push(Write {
            pos: *pos,
            state: "minecraft:air".to_string(),
            flags: 18,
        });
    }

    // `for (i = toPush.size() - 1; i >= 0; i--)` — reverse order, each a
    // `moving_piston` at flag 324. **Not broadcast.**
    let mut occupied = Vec::new();
    for pos in resolved.to_push.iter().rev() {
        let dest = facing.relative(*pos);
        occupied.push(dest);
        writes.push(Write {
            pos: dest,
            state: moving_piston_state(facing, false),
            flags: 324,
        });
    }

    // The head: a `moving_piston` at the position in front of the piston,
    // also 324 and also not broadcast.
    let head_pos = facing.relative(piston_pos);
    occupied.push(head_pos);
    writes.push(Write {
        pos: head_pos,
        state: moving_piston_state(facing, false),
        flags: 324,
    });

    // `for (BlockPos p : vacated.keySet()) setBlock(p, air, 82)` — every
    // position the structure left that nothing else now occupies. Broadcast.
    for pos in &resolved.to_push {
        if !occupied.contains(pos) {
            writes.push(Write {
                pos: *pos,
                state: "minecraft:air".to_string(),
                flags: 82,
            });
        }
    }
    writes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// A world of explicit states, recording every `hasSignal` probe.
    ///
    /// The probe log is what makes the `getNeighborSignal` tests falsifiable:
    /// asserting only on the boolean answer would pass for a port that walked
    /// the wrong positions and happened to find the same signal.
    struct Fake {
        states: BTreeMap<BlockPos, String>,
        signals: Vec<(BlockPos, Face)>,
        probes: RefCell<Vec<(BlockPos, Face)>>,
        entities: Vec<&'static str>,
        reactions: BTreeMap<&'static str, PushReaction>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                states: BTreeMap::new(),
                signals: Vec::new(),
                probes: RefCell::new(Vec::new()),
                entities: Vec::new(),
                reactions: BTreeMap::new(),
            }
        }
        fn with(mut self, pos: BlockPos, state: &str) -> Self {
            self.states.insert(pos, state.to_string());
            self
        }
        fn powered(mut self, pos: BlockPos, dir: Face) -> Self {
            self.signals.push((pos, dir));
            self
        }
        fn reaction(mut self, block: &'static str, r: PushReaction) -> Self {
            self.reactions.insert(block, r);
            self
        }
        fn entity(mut self, block: &'static str) -> Self {
            self.entities.push(block);
            self
        }
    }

    impl PistonWorld for Fake {
        fn state_at(&self, pos: BlockPos) -> String {
            self.states
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn has_signal(&self, pos: BlockPos, dir: Face) -> bool {
            self.probes.borrow_mut().push((pos, dir));
            self.signals.contains(&(pos, dir))
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            319
        }
        fn push_reaction(&self, state: &str) -> PushReaction {
            *self
                .reactions
                .get(block_of(state))
                .unwrap_or(&PushReaction::Normal)
        }
        fn is_unbreakable(&self, state: &str) -> bool {
            block_of(state) == "minecraft:bedrock"
        }
        fn has_block_entity(&self, state: &str) -> bool {
            self.entities.contains(&block_of(state))
        }
    }

    const P: BlockPos = (0, 0, 0);
    const PISTON: &str = "minecraft:piston[extended=false,facing=north]";
    const EXTENDED: &str = "minecraft:piston[extended=true,facing=north]";

    // -- getNeighborSignal ---------------------------------------------------

    /// Pass 2 — `hasSignal(pos, DOWN)` at the piston's **own** position.
    ///
    /// Goes red if that line is dropped, which is the single easiest thing to
    /// lose when porting this from a description: every other probe in the
    /// method is at a *neighbour*.
    #[test]
    fn the_own_position_down_probe_happens() {
        let w = Fake::new();
        let _ = get_neighbor_signal(&w, P, Face::North);
        assert!(
            w.probes.borrow().contains(&(P, Face::Down)),
            "getNeighborSignal never probed its own position with DOWN; probes were {:?}",
            w.probes.borrow()
        );
    }

    /// And it answers `true` off that probe alone, with nothing else powered.
    #[test]
    fn a_signal_at_the_pistons_own_position_from_below_extends_it() {
        let w = Fake::new().powered(P, Face::Down);
        assert!(get_neighbor_signal(&w, P, Face::North));
    }

    /// Pass 3 — quasi-connectivity. Only the block **above** the piston is
    /// powered, and the piston still answers.
    #[test]
    fn quasi_connectivity_reads_the_block_above() {
        let above = Face::Up.relative(P);
        let w = Fake::new().powered(Face::East.relative(above), Face::East);
        assert!(
            get_neighbor_signal(&w, P, Face::North),
            "the block above the piston was powered and the piston did not answer — pass 3 is missing"
        );
    }

    /// Pass 3 skips `DOWN`.
    ///
    /// **This cannot be tested through the answer**, and finding that out
    /// cost a red: `pos.above().relative(DOWN)` *is* `pos`, and pass 2 has
    /// already probed `(pos, DOWN)`, so dropping the guard changes no
    /// verdict — it only probes the same pair twice. The observable
    /// difference is the probe count, so that is what is asserted.
    ///
    /// Red if the `dir != Face::Down` guard in pass 3 is dropped.
    #[test]
    fn pass_three_skips_down() {
        let w = Fake::new();
        let _ = get_neighbor_signal(&w, P, Face::North);
        let probes = w.probes.borrow();
        let own_down = probes.iter().filter(|p| **p == (P, Face::Down)).count();
        assert_eq!(
            own_down, 1,
            "(pos, DOWN) was probed {own_down} times; pass 3 must skip DOWN because pass 2 \
             has already asked that exact question. Probes: {probes:?}"
        );
    }

    /// Pass 1 skips the facing direction — a signal in front of the piston
    /// does not power it. Red if the `dir != facing` guard is dropped.
    #[test]
    fn pass_one_skips_the_facing_direction() {
        let w = Fake::new().powered(Face::North.relative(P), Face::North);
        assert!(!get_neighbor_signal(&w, P, Face::North));
        // ... and the same signal on any other side does power it.
        let w = Fake::new().powered(Face::South.relative(P), Face::South);
        assert!(get_neighbor_signal(&w, P, Face::North));
    }

    // -- isPushable ----------------------------------------------------------

    /// **The four named blocks are refused by name, and they have to be.**
    ///
    /// Measured from `data/vanilla/piston_push.json` as generated by
    /// `ci/piston-golden.sh` on 2026-08-17 against the pinned 26.2 jar,
    /// 1,196 blocks: **NORMAL 811, DESTROY 356, PUSH_ONLY 16, BLOCK 13**,
    /// 15 unbreakable, 186 with a block entity — and
    /// **`minecraft:obsidian` is `NORMAL`**.
    ///
    /// So a port that consulted `getPistonPushReaction()` alone would happily
    /// push obsidian. `isPushable` names those four as literals *before* it
    /// reaches the reaction switch precisely because the reaction does not
    /// cover them, and this `Fake` answers `Normal` for everything for the
    /// same reason: it makes the literal list the only thing that can be
    /// refusing them, so deleting the list turns this red.
    #[test]
    fn obsidian_and_friends_refuse_and_air_accepts() {
        let w = Fake::new();
        assert!(is_pushable(
            &w,
            "minecraft:air",
            (0, 0, -1),
            Face::North,
            false,
            Face::North
        ));
        for b in IMMOVABLE {
            assert!(
                !is_pushable(&w, b, (0, 0, -1), Face::North, false, Face::North),
                "{b} was pushable"
            );
        }
    }

    /// `PushReaction.DESTROY` answers `allowDestroy`, which is `false` at the
    /// head of a line and `true` inside the forward walk. Red if the arm is
    /// hard-coded either way.
    #[test]
    fn destroy_answers_the_allow_destroy_argument() {
        let w = Fake::new().reaction("minecraft:torch", PushReaction::Destroy);
        let at = (0, 0, -1);
        assert!(!is_pushable(
            &w,
            "minecraft:torch",
            at,
            Face::North,
            false,
            Face::North
        ));
        assert!(is_pushable(
            &w,
            "minecraft:torch",
            at,
            Face::North,
            true,
            Face::North
        ));
    }

    /// `PUSH_ONLY` compares the **push direction** against the **piston's
    /// facing**, which differ on a retraction. Red if the two are conflated.
    #[test]
    fn push_only_compares_push_direction_to_piston_facing() {
        let w = Fake::new().reaction("minecraft:piston_head", PushReaction::PushOnly);
        let at = (0, 0, -1);
        assert!(is_pushable(
            &w,
            "minecraft:piston_head",
            at,
            Face::North,
            false,
            Face::North
        ));
        assert!(!is_pushable(
            &w,
            "minecraft:piston_head",
            at,
            Face::South,
            false,
            Face::North
        ));
    }

    #[test]
    fn a_block_entity_and_an_extended_piston_both_refuse() {
        let w = Fake::new().entity("minecraft:chest");
        let at = (0, 0, -1);
        assert!(!is_pushable(
            &w,
            "minecraft:chest",
            at,
            Face::North,
            false,
            Face::North
        ));
        assert!(!is_pushable(
            &w,
            EXTENDED,
            at,
            Face::North,
            false,
            Face::North
        ));
        assert!(is_pushable(&w, PISTON, at, Face::North, false, Face::North));
    }

    // -- resolve / addBlockLine ---------------------------------------------

    /// `addBlockLine`'s early returns are **successes**. A stone block in
    /// front with air beyond resolves to a one-block push; the wall behind it
    /// stops the line without refusing the push.
    ///
    /// Red if any of those four `return true`s is read as a refusal, which
    /// inverts every ordinary piston.
    #[test]
    fn a_stopping_condition_is_not_a_refusal() {
        let w = Fake::new()
            .with((0, 0, -1), "minecraft:stone")
            .with((0, 0, -2), "minecraft:stone")
            .with((0, 0, -3), "minecraft:bedrock");
        let r = resolve(&w, P, Face::North, true).expect("push refused");
        assert_eq!(r.to_push, vec![(0, 0, -1), (0, 0, -2)]);
        assert!(r.to_destroy.is_empty());
    }

    /// Nothing in front at all still resolves — the piston extends into air.
    #[test]
    fn an_empty_line_resolves() {
        let w = Fake::new();
        let r = resolve(&w, P, Face::North, true).expect("push into air refused");
        assert!(r.to_push.is_empty());
    }

    /// Twelve move, thirteen refuse. Red if the boundary moves by one.
    #[test]
    fn the_push_depth_boundary_is_twelve() {
        let line = |n: i32| {
            let mut w = Fake::new();
            for i in 1..=n {
                w = w.with((0, 0, -i), "minecraft:stone");
            }
            w
        };
        let twelve = resolve(&line(12), P, Face::North, true).expect("twelve refused");
        assert_eq!(twelve.to_push.len(), 12);
        assert_eq!(
            resolve(&line(13), P, Face::North, true),
            Err(Unresolved::Refused)
        );
    }

    /// A `DESTROY` block at the head refuses the line but reports itself as
    /// destroyable; further along it is collected. The asymmetry is
    /// `allowDestroy`.
    #[test]
    fn destroy_at_the_head_and_destroy_further_along_differ() {
        let w = Fake::new()
            .with((0, 0, -1), "minecraft:torch")
            .reaction("minecraft:torch", PushReaction::Destroy);
        let r = resolve(&w, P, Face::North, true).expect("head destroy refused");
        assert_eq!(r.to_destroy, vec![(0, 0, -1)]);
        assert!(r.to_push.is_empty());

        let w = Fake::new()
            .with((0, 0, -1), "minecraft:stone")
            .with((0, 0, -2), "minecraft:torch")
            .reaction("minecraft:torch", PushReaction::Destroy);
        let r = resolve(&w, P, Face::North, true).expect("trailing destroy refused");
        assert_eq!(r.to_push, vec![(0, 0, -1)]);
        assert_eq!(r.to_destroy, vec![(0, 0, -2)]);
    }

    /// Slime refuses **distinguishably**. Red if `StickyUnported` is
    /// collapsed into `Refused`, which would make an unported branch look
    /// like a vanilla refusal.
    #[test]
    fn slime_refuses_as_unported_not_as_a_vanilla_refusal() {
        let w = Fake::new().with((0, 0, -1), "minecraft:slime_block");
        assert_eq!(
            resolve(&w, P, Face::North, true),
            Err(Unresolved::StickyUnported((0, 0, -1)))
        );
    }

    // -- checkIfExtend -------------------------------------------------------

    /// `b1` is `facing.get3DDataValue()` and nothing else. Red if the
    /// encoding drifts to an ordinal of some other enum.
    #[test]
    fn the_queued_event_carries_extend_and_the_facing_3d_value() {
        for (name, face) in [
            ("north", Face::North),
            ("south", Face::South),
            ("west", Face::West),
            ("east", Face::East),
            ("up", Face::Up),
            ("down", Face::Down),
        ] {
            let state = format!("minecraft:piston[extended=false,facing={name}]");
            // Powered through pass 2 — `hasSignal(pos, DOWN)` at the
            // piston's own position. Pass 1 is skipped for `dir == facing`,
            // so a neighbour-powered fixture silently tests nothing at
            // `facing=down`; that cost a red here.
            let w = Fake::new().powered(P, Face::Down);
            let got = check_if_extend(&w, P, &state);
            assert_eq!(
                got,
                Some((TRIGGER_EXTEND, face.to_3d())),
                "facing={name} queued {got:?}"
            );
        }
    }

    /// Powered but unresolvable queues **nothing**. Red if the `resolve` gate
    /// is dropped from `checkIfExtend`, which would put an event on the wire
    /// for a piston vanilla leaves alone.
    #[test]
    fn an_unresolvable_push_queues_no_event() {
        let mut w = Fake::new().powered(Face::Down.relative(P), Face::Down);
        for i in 1..=13 {
            w = w.with((0, 0, -i), "minecraft:stone");
        }
        assert_eq!(check_if_extend(&w, P, PISTON), None);
    }

    /// Unpowered and extended retracts; unpowered and retracted does nothing.
    #[test]
    fn losing_power_queues_a_contract_and_a_quiet_piston_queues_nothing() {
        let w = Fake::new();
        assert_eq!(
            check_if_extend(&w, P, EXTENDED),
            Some((TRIGGER_CONTRACT, Face::North.to_3d()))
        );
        assert_eq!(check_if_extend(&w, P, PISTON), None);
    }

    // -- triggerEvent --------------------------------------------------------

    /// **The queued-and-discarded case.** A piston event reaches the gate and
    /// is delivered, where before it was dropped because the gate matched
    /// `minecraft:chest` alone.
    #[test]
    fn a_piston_extend_event_is_delivered() {
        let w = Fake::new().powered(Face::Down.relative(P), Face::Down);
        let out = trigger_event(&w, PISTON, P, TRIGGER_EXTEND, Face::North.to_3d());
        assert!(out.deliver, "the piston's extend event was discarded");
    }

    /// Re-powered between queue and drain: the contract event is refused and
    /// the piston is written back out **with** `UPDATE_CLIENTS`. Red if the
    /// refusal forgets its write, which leaves a client rendering a retracted
    /// piston the server thinks is out.
    #[test]
    fn a_contract_overtaken_by_power_refuses_but_still_writes_extended() {
        let w = Fake::new().powered(Face::Down.relative(P), Face::Down);
        let out = trigger_event(&w, EXTENDED, P, TRIGGER_CONTRACT, Face::North.to_3d());
        assert!(!out.deliver);
        assert_eq!(out.writes.len(), 1);
        assert!(out.writes[0].reaches_clients());
        assert_eq!(out.writes[0].flags, 2);
        assert!(extended_of(&out.writes[0].state));
    }

    /// The symmetric refusal: an extend that lost its signal writes nothing
    /// at all.
    #[test]
    fn an_extend_that_lost_its_signal_refuses_silently() {
        let w = Fake::new();
        let out = trigger_event(&w, PISTON, P, TRIGGER_EXTEND, Face::North.to_3d());
        assert!(!out.deliver);
        assert!(out.writes.is_empty());
    }

    /// An extend whose structure will not move refuses.
    #[test]
    fn an_extend_whose_resolve_fails_refuses() {
        let mut w = Fake::new().powered(Face::Down.relative(P), Face::Down);
        for i in 1..=13 {
            w = w.with((0, 0, -i), "minecraft:stone");
        }
        let out = trigger_event(&w, PISTON, P, TRIGGER_EXTEND, Face::North.to_3d());
        assert!(!out.deliver);
        assert!(out.writes.is_empty());
    }

    /// The two sounds, their two different pitch scales, and the three arms
    /// that make none.
    ///
    /// Vanilla's `playSound` calls are at `triggerEvent` offsets 118 and 531,
    /// both **past** every `return false`, so a refused event is silent. That
    /// is the half worth a test: a port that plays the sound before deciding
    /// gives a piston that clicks and does not move, and nothing about the
    /// blocks would say so.
    #[test]
    fn only_a_piston_that_moves_makes_a_noise() {
        let powered = || Fake::new().powered(Face::Down.relative(P), Face::Down);

        let out = trigger_event(
            &powered().with((0, 0, -1), "minecraft:stone"),
            PISTON,
            P,
            TRIGGER_EXTEND,
            Face::North.to_3d(),
        );
        let sound = out.sound.expect("an extension is heard");
        assert_eq!(sound.event, "minecraft:block.piston.extend");
        assert_eq!(sound.source, crate::sounds::SOURCE_BLOCKS);
        assert_eq!(sound.volume, 0.5);
        // `aconst_null` in the entity slot: nobody is excluded.
        assert_eq!(sound.except, None);
        // `nextFloat() * 0.25f + 0.6f`, so 0.6 <= pitch < 0.85. The
        // contraction's range is 0.6..0.75 and is a strict subset, which is
        // why the two are asserted as ranges that do not coincide rather than
        // as one range twice.
        assert!(
            (0.6..0.85).contains(&sound.pitch),
            "extend pitch {} outside 0.6..0.85",
            sound.pitch
        );

        let out = trigger_event(
            &Fake::new(),
            EXTENDED,
            P,
            TRIGGER_CONTRACT,
            Face::North.to_3d(),
        );
        let sound = out.sound.expect("a retraction is heard");
        assert_eq!(sound.event, "minecraft:block.piston.contract");
        assert_eq!(sound.volume, 0.5);
        assert!(
            (0.6..0.75).contains(&sound.pitch),
            "contract pitch {} outside 0.6..0.75",
            sound.pitch
        );

        // The three refusals, silent. Same three worlds as the three tests
        // above, so if one of them stops refusing this test stops testing a
        // refusal and the test above it goes red.
        assert!(
            trigger_event(
                &powered(),
                EXTENDED,
                P,
                TRIGGER_CONTRACT,
                Face::North.to_3d()
            )
            .sound
            .is_none(),
            "a contract overtaken by power is silent"
        );
        assert!(
            trigger_event(&Fake::new(), PISTON, P, TRIGGER_EXTEND, Face::North.to_3d())
                .sound
                .is_none(),
            "an extend that lost its signal is silent"
        );
        let mut w = powered();
        for i in 1..=13 {
            w = w.with((0, 0, -i), "minecraft:stone");
        }
        assert!(
            trigger_event(&w, PISTON, P, TRIGGER_EXTEND, Face::North.to_3d())
                .sound
                .is_none(),
            "an extend whose resolve fails is silent"
        );
    }

    /// **The central claim of this module, as a check that can go red.**
    ///
    /// Every `moving_piston` write is invisible to clients and the piston's
    /// own `extended=true` is not. Flip either flag's `UPDATE_CLIENTS` bit
    /// and this fails.
    #[test]
    fn moving_piston_writes_are_never_broadcast_and_the_piston_state_always_is() {
        let w = Fake::new()
            .powered(Face::Down.relative(P), Face::Down)
            .with((0, 0, -1), "minecraft:stone");
        let out = trigger_event(&w, PISTON, P, TRIGGER_EXTEND, Face::North.to_3d());
        assert!(out.deliver);

        let mut saw_moving = 0;
        let mut saw_piston_state = 0;
        for wr in &out.writes {
            if block_of(&wr.state) == "minecraft:moving_piston" {
                saw_moving += 1;
                assert!(
                    !wr.reaches_clients(),
                    "a moving_piston write at {:?} carried UPDATE_CLIENTS (flags {})",
                    wr.pos,
                    wr.flags
                );
                assert_eq!(wr.flags, 324);
            }
            if is_piston(block_of(&wr.state)) {
                saw_piston_state += 1;
                assert!(wr.reaches_clients());
                assert_eq!(wr.flags, 67);
            }
        }
        // the pushed stone's destination and the head
        assert_eq!(saw_moving, 2, "writes were {:?}", out.writes);
        assert_eq!(saw_piston_state, 1);
    }

    /// The retraction places a `moving_piston` at the piston's own position,
    /// at flag 276, and still delivers the packet. Red if 276 gains bit 2.
    #[test]
    fn a_retraction_is_delivered_and_writes_nothing_a_client_sees() {
        let w = Fake::new();
        let out = trigger_event(&w, EXTENDED, P, TRIGGER_CONTRACT, Face::North.to_3d());
        assert!(out.deliver);
        assert_eq!(out.writes[0].pos, P);
        assert_eq!(out.writes[0].flags, 276);
        assert!(
            !out.writes[0].reaches_clients(),
            "the retracting piston's own moving_piston must stay off the wire"
        );
    }

    /// A sticky piston's `moving_piston` carries `type=sticky`, and its
    /// retraction stops there — the pull-back is unported, so nothing is
    /// written at the head position rather than a guess being made.
    ///
    /// Red if the sticky arm ever starts emitting the plain piston's head
    /// removal, which vanilla only performs on one of two branches it
    /// chooses by reading two blocks ahead.
    #[test]
    fn a_sticky_piston_retracts_as_type_sticky_and_does_not_guess_the_pull_back() {
        let w = Fake::new();
        let sticky = "minecraft:sticky_piston[extended=true,facing=north]";
        let out = trigger_event(&w, sticky, P, TRIGGER_CONTRACT, Face::North.to_3d());
        assert!(out.deliver);
        assert_eq!(out.writes.len(), 1, "writes were {:?}", out.writes);
        assert!(out.writes[0].state.contains("type=sticky"));
    }

    /// **A plain retraction removes the head, and that write IS broadcast.**
    ///
    /// `level.removeBlock(pos.relative(dir), false)` is
    /// `setBlock(pos, fluidState.createLegacyBlock(), 3)` — flag 3 carries
    /// `UPDATE_CLIENTS`. Red if the write is dropped (the head stays forever)
    /// or if its flag loses bit 2 (the head vanishes server-side and every
    /// client keeps rendering it).
    #[test]
    fn a_plain_retraction_removes_the_head_and_tells_the_clients() {
        let w = Fake::new();
        let out = trigger_event(&w, EXTENDED, P, TRIGGER_CONTRACT, Face::North.to_3d());
        let head = Face::North.relative(P);
        let removal = out
            .writes
            .iter()
            .find(|x| x.pos == head)
            .unwrap_or_else(|| panic!("no write at the head position; writes {:?}", out.writes));
        assert_eq!(removal.state, "minecraft:air");
        assert_eq!(removal.flags, 3);
        assert!(removal.reaches_clients());
    }

    /// The head position retracts to the fluid that was there, not to air.
    ///
    /// Red if `createLegacyBlock` is flattened to a constant `air`, which is
    /// right everywhere except in water and says nothing when it is wrong.
    #[test]
    fn a_head_under_water_retracts_to_water() {
        let head = Face::North.relative(P);
        let w = Fake::new().with(head, "minecraft:water[level=0]");
        let out = trigger_event(&w, EXTENDED, P, TRIGGER_CONTRACT, Face::North.to_3d());
        let removal = out.writes.iter().find(|x| x.pos == head).expect("no write");
        assert_eq!(removal.state, "minecraft:water[level=0]");
    }
}

// ---------------------------------------------------------------------------
// The table and the world adapter
// ---------------------------------------------------------------------------

/// `data/vanilla/piston_push.json` — the three per-state facts `isPushable`
/// reads that no other table carries.
///
/// Generated by `ci/piston-golden.sh`. Keyed by block name where every state
/// of that block agrees and by full state string where they do not, which is
/// how the file stays small without inventing a uniformity vanilla does not
/// have.
#[derive(Debug, Default)]
pub struct PushTable {
    blocks: std::collections::BTreeMap<String, Facts>,
    states: std::collections::BTreeMap<String, Facts>,
}

/// One state's answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub reaction: PushReaction,
    pub unbreakable: bool,
    pub block_entity: bool,
}

impl PushTable {
    /// Look a state up — the per-state row first, then the per-block one.
    ///
    /// `None` when the table has never heard of the block, which callers must
    /// treat as a refusal rather than as NORMAL. A missing row is not a
    /// pushable block; it is a table that did not cover this world.
    #[must_use]
    pub fn get(&self, state: &str) -> Option<Facts> {
        self.states
            .get(state)
            .or_else(|| self.blocks.get(block_of(state)))
            .copied()
    }

    /// How many rows the table carries, per kind.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.blocks.len(), self.states.len())
    }

    /// Parse the dumper's JSON.
    ///
    /// # Errors
    /// If the file is missing, is not the expected shape, or carries no
    /// `DESTROY`/`BLOCK`/`PUSH_ONLY` row at all — an all-`NORMAL` table is
    /// what a dump against a broken classpath produces, and it would make
    /// every block pushable while looking like a full table.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        let mut out = Self::default();
        for (key, dest) in [("blocks", true), ("states", false)] {
            let Some(map) = v.get(key).and_then(serde_json::Value::as_object) else {
                return Err(std::io::Error::other(format!(
                    "piston_push.json has no `{key}` object"
                )));
            };
            for (name, row) in map {
                let reaction = match row.get("reaction").and_then(serde_json::Value::as_str) {
                    Some("NORMAL") => PushReaction::Normal,
                    Some("DESTROY") => PushReaction::Destroy,
                    Some("BLOCK") => PushReaction::Block,
                    Some("PUSH_ONLY") => PushReaction::PushOnly,
                    other => {
                        return Err(std::io::Error::other(format!(
                            "piston_push.json: {name} has reaction {other:?}"
                        )));
                    }
                };
                let facts = Facts {
                    reaction,
                    unbreakable: row.get("unbreakable").and_then(serde_json::Value::as_bool)
                        == Some(true),
                    block_entity: row.get("blockEntity").and_then(serde_json::Value::as_bool)
                        == Some(true),
                };
                if dest {
                    out.blocks.insert(name.clone(), facts);
                } else {
                    out.states.insert(name.clone(), facts);
                }
            }
        }
        let mut seen = [false; 3];
        for f in out.blocks.values().chain(out.states.values()) {
            match f.reaction {
                PushReaction::Destroy => seen[0] = true,
                PushReaction::Block => seen[1] = true,
                PushReaction::PushOnly => seen[2] = true,
                PushReaction::Normal => {}
            }
        }
        if !seen.iter().all(|b| *b) {
            return Err(std::io::Error::other(
                "piston_push.json is missing a whole reaction class — an all-NORMAL table \
                 makes every block pushable and would gate green",
            ));
        }
        Ok(out)
    }
}

/// The process-wide push table, or `None` when it could not be loaded.
///
/// Same cwd-then-manifest fallback as [`crate::power::shared`], and the same
/// carried risk.
#[must_use]
pub fn shared_push_table() -> Option<&'static PushTable> {
    static TABLE: std::sync::OnceLock<Option<PushTable>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            const REL: &str = "data/vanilla/piston_push.json";
            let cwd_relative = std::path::Path::new(REL);
            let path: std::path::PathBuf = if cwd_relative.exists() {
                cwd_relative.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(REL)
            };
            match PushTable::load(&path) {
                Ok(t) => {
                    let (blocks, states) = t.counts();
                    eprintln!("piston push table: {blocks} blocks, {states} split states");
                    Some(t)
                }
                Err(e) => {
                    eprintln!(
                        "no piston push table ({e}) — every piston will refuse to move a \
                         structure rather than guess a push reaction"
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// [`PistonWorld`] over this server's block store and redstone view.
pub struct LevelPiston<'a> {
    blocks: &'a dyn crate::edits::Blocks,
    redstone: &'a crate::power::Redstone<'a>,
    table: Option<&'a PushTable>,
    min_y: i32,
    max_y: i32,
}

impl<'a> LevelPiston<'a> {
    #[must_use]
    pub fn new(
        blocks: &'a dyn crate::edits::Blocks,
        redstone: &'a crate::power::Redstone<'a>,
        table: Option<&'a PushTable>,
        min_y: i32,
        max_y: i32,
    ) -> Self {
        Self {
            blocks,
            redstone,
            table,
            min_y,
            max_y,
        }
    }
}

impl PistonWorld for LevelPiston<'_> {
    fn state_at(&self, pos: BlockPos) -> String {
        self.blocks.state_at(pos)
    }
    fn has_signal(&self, pos: BlockPos, dir: Face) -> bool {
        self.redstone.has_signal(pos, dir)
    }
    fn min_y(&self) -> i32 {
        self.min_y
    }
    fn max_y(&self) -> i32 {
        self.max_y
    }
    /// A block the table has never heard of answers [`PushReaction::Block`],
    /// not [`PushReaction::Normal`].
    ///
    /// **The direction of this default is the whole point of the table.**
    /// Defaulting to `Normal` would push every unknown block and produce a
    /// piston that works in every scenario whose blocks happen to be normal —
    /// which is every scenario written first, so the defect would ship behind
    /// a green gate.
    fn push_reaction(&self, state: &str) -> PushReaction {
        self.table
            .and_then(|t| t.get(state))
            .map_or(PushReaction::Block, |f| f.reaction)
    }
    fn is_unbreakable(&self, state: &str) -> bool {
        self.table
            .and_then(|t| t.get(state))
            .is_none_or(|f| f.unbreakable)
    }
    fn has_block_entity(&self, state: &str) -> bool {
        self.table
            .and_then(|t| t.get(state))
            .is_none_or(|f| f.block_entity)
    }
    fn blocks(&self) -> Option<&dyn crate::edits::Blocks> {
        Some(self.blocks)
    }
}

/// Warn once per state string that the block table does not know.
///
/// Once, not per occurrence: a piston writes on every extension and a
/// per-write warning would be a per-tick log line. Same shape as
/// [`crate::power`]'s `unported`.
pub fn unknown_state_once(state: &str) {
    static SAID: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<String>>> =
        std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));
    let mut said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.insert(state.to_string()) {
        eprintln!(
            "piston: the block table has no state {state} — the write is dropped, so this \
             piston will move less than vanilla's"
        );
    }
}

/// `PistonBaseBlock.neighborChanged` and `onPlace` — both are
/// `if (!level.isClientSide) checkIfExtend(level, pos, state);` and nothing
/// else, so one function serves both.
///
/// Queues into [`crate::containers::BlockEvents::shared`]. It does **not**
/// act: `checkIfExtend` only ever calls `Level.blockEvent`, and every visible
/// consequence happens at the tick boundary when the queue drains. That
/// separation is the whole mechanism, and collapsing it — extending the
/// piston here, at neighbour-update time — would put the head out in the same
/// tick as the redstone change instead of the tick after it.
pub fn neighbor_changed(
    blocks: &dyn crate::edits::Blocks,
    signals: Option<&crate::power::SignalTable>,
    pos: BlockPos,
    state: &str,
    min_y: i32,
    max_y: i32,
) {
    let Some(signals) = signals else { return };
    let redstone = crate::power::Redstone::new(blocks, signals, None);
    let world = LevelPiston::new(blocks, &redstone, shared_push_table(), min_y, max_y);
    if let Some((b0, b1)) = check_if_extend(&world, pos, state) {
        crate::containers::BlockEvents::shared().queue(crate::containers::BlockEventData {
            pos,
            block: block_of(state).to_string(),
            b0,
            b1,
        });
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;

    const GOOD: &str = r#"{
      "_": "piston_push",
      "blocks": {
        "minecraft:stone":   {"reaction": "NORMAL",   "unbreakable": false, "blockEntity": false},
        "minecraft:bedrock": {"reaction": "BLOCK",    "unbreakable": true,  "blockEntity": false},
        "minecraft:torch":   {"reaction": "DESTROY",  "unbreakable": false, "blockEntity": false},
        "minecraft:piston_head": {"reaction": "PUSH_ONLY", "unbreakable": false, "blockEntity": false}
      },
      "states": {
        "minecraft:chest[facing=north]": {"reaction": "NORMAL", "unbreakable": false, "blockEntity": true}
      }
    }"#;

    /// Write `text` to a file nothing else in this process will name, and
    /// load it.
    ///
    /// **The counter is not decoration.** This keyed the filename on
    /// `text.len()` at first, and cargo runs these tests in parallel: two
    /// fixtures of equal length shared one path and each deleted the other's
    /// file mid-read, surfacing as `EOF while parsing a value at line 1
    /// column 0` in whichever test lost. A test that fails because of another
    /// test is a check whose red says nothing.
    fn load(text: &str) -> std::io::Result<PushTable> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("piston-table-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("t{}.json", N.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(&p, text).unwrap();
        let r = PushTable::load(&p);
        let _ = std::fs::remove_file(&p);
        r
    }

    #[test]
    fn a_state_row_wins_over_its_blocks_row() {
        let t = load(GOOD).expect("load");
        assert!(t.get("minecraft:chest[facing=north]").unwrap().block_entity);
        assert_eq!(
            t.get("minecraft:stone").unwrap().reaction,
            PushReaction::Normal
        );
        // a state string falls back to its block's row
        assert_eq!(
            t.get("minecraft:stone[foo=bar]").unwrap().reaction,
            PushReaction::Normal
        );
        assert_eq!(t.get("minecraft:nonesuch"), None);
    }

    /// **An all-NORMAL table is refused.**
    ///
    /// That is what a dump against a broken classpath produces, and it is the
    /// same size and shape as a real one. Without this the loader would
    /// accept it and every block in the world would become pushable.
    #[test]
    fn a_table_with_no_destroy_or_block_row_is_refused() {
        let bland = r#"{"blocks": {"minecraft:stone": {"reaction": "NORMAL", "unbreakable": false, "blockEntity": false}}, "states": {}}"#;
        let e = load(bland).expect_err("an all-NORMAL table loaded");
        assert!(e.to_string().contains("reaction class"), "{e}");
    }

    #[test]
    fn an_unknown_reaction_name_is_refused() {
        let bad = r#"{"blocks": {"minecraft:stone": {"reaction": "SQUISH", "unbreakable": false, "blockEntity": false}}, "states": {}}"#;
        assert!(load(bad).is_err());
    }

    /// **The missing-row default refuses rather than pushes.**
    ///
    /// Red if `push_reaction`'s `map_or` default is changed to `Normal`,
    /// which is the change that makes pistons appear to work everywhere and
    /// be silently wrong on obsidian-adjacent blocks the table would have
    /// covered.
    #[test]
    fn a_world_with_no_table_refuses_every_push() {
        struct NoTable;
        impl PistonWorld for NoTable {
            fn state_at(&self, _p: BlockPos) -> String {
                "minecraft:stone".into()
            }
            fn has_signal(&self, _p: BlockPos, _d: Face) -> bool {
                false
            }
            fn min_y(&self) -> i32 {
                WORLD_MIN_Y
            }
            fn max_y(&self) -> i32 {
                WORLD_MAX_Y
            }
            fn push_reaction(&self, _s: &str) -> PushReaction {
                PushReaction::Block
            }
            fn is_unbreakable(&self, _s: &str) -> bool {
                true
            }
            fn has_block_entity(&self, _s: &str) -> bool {
                true
            }
        }
        assert_eq!(
            resolve(&NoTable, (0, 0, 0), Face::North, true),
            Err(Unresolved::Refused)
        );
    }
}
