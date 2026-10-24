//! Gravity and landing for the entities this server locally simulates.
//!
//! `Profile::of` answers `Some` for `minecraft:item` and
//! `minecraft:falling_block` — the two kinds whose `isClientAuthoritative()`
//! is `false` — and this module is [`crate::physics::move_entity`]'s first
//! production caller. **Only `minecraft:falling_block` is stepped here.** An
//! item's tick — gravity, friction, merging, the pickup delay, the despawn —
//! is `crate::itementity::tick`, called beside this one, because it carries
//! state (`ItemData`) this loop has no reason to know about.
//!
//! # `FallingBlockEntity.tick()`, read from the pinned jar
//!
//! ```java
//! void tick() {
//!     if (blockState.isAir()) { discard(); return; }
//!     time++;
//!     applyGravity();
//!     move(MoverType.SELF, getDeltaMovement());
//!     // applyEffectsFromBlocks(), handlePortal() — not reachable, not ported
//!     if (level() instanceof ServerLevel serverLevel && isAlive()) {
//!         BlockPos pos = blockPosition();
//!         if (!onGround()) { /* fall through to the air-drag scale below */ }
//!         else {
//!             BlockState atPos = level.getBlockState(pos);
//!             setDeltaMovement(getDeltaMovement().multiply(0.7, -0.5, 0.7));
//!             if (!atPos.is(Blocks.MOVING_PISTON)) {
//!                 boolean canBeReplaced = atPos.canBeReplaced(
//!                     new DirectionalPlaceContext(level, pos, DOWN, ItemStack.EMPTY, UP));
//!                 boolean canFallFurther = FallingBlock.isFree(level.getBlockState(pos.below()));
//!                 boolean shouldPlace = /* canSurvive, FallingBlock doesn't override it */ true
//!                     && !canFallFurther;
//!                 if (canBeReplaced && shouldPlace) {
//!                     if (level.setBlock(pos, blockState, 3)) {
//!                         chunkMap.sendToTrackingPlayers(this,
//!                             new ClientboundBlockUpdatePacket(pos, level.getBlockState(pos)));
//!                         discard();
//!                         // Fallable.onLand, block-entity NBT copy — not ported
//!                     } else { discard(); /* dropItem path — not ported */ }
//!                 } else { discard(); /* dropItem path — not ported */ }
//!             }
//!         }
//!     }
//!     setDeltaMovement(getDeltaMovement().scale(getAirDrag()));
//! }
//! ```
//!
//! # What is here and what is deliberately not, named rather than left silent
//!
//! **Ported:** the gravity/move step every tick; the landing test
//! (`onGround()`); the `canBeReplaced` / `isFree(below)` pair that decides
//! whether to place; the write, keyed on [`crate::fallable::Cargo`] for the
//! state string `Entity` itself never carries; and the **two-update landing**
//! — [`crate::tracker::Tracker::send_to_tracking_players`] immediately,
//! [`crate::broadcast::ChangedBlocks`] queued, in that order, matching
//! `ChunkMap.sendToTrackingPlayers` running inside the entity tick while the
//! queued broadcast waits for `ServerChunkCache.tick`'s next pass — which in
//! this server's own measured tick order is the second `changed.flush` call
//! in `crate::status::listen`, immediately after this function returns.
//!
//! **Not ported, each because nothing has measured it yet:**
//! * `ConcretePowderBlock`'s mid-air water conversion — the raycast between
//!   the previous and current position that lets a fast-moving concrete
//!   powder block turn to concrete without landing exactly on a water block.
//!   `isConcretePowder` and `waterHit` are both `false` unconditionally here,
//!   which also means the `onGround() == false` early-continue is the whole
//!   of the "still falling" case — vanilla's `waterHit` branch of that `||`
//!   never fires.
//! * `Fallable.onLand` (`AnvilBlock`'s break chance, `DragonEggBlock`'s
//!   teleport) and the copied block-entity NBT (`blockData`) — neither of the
//!   23 `FallingBlock`s a plain sand or gravel drop reaches needs either.
//! * `cancelDrop`, the `Blocks.MOVING_PISTON` guard, and the 100/600-tick
//!   despawn-and-drop timeout for a block that never lands. Landing on top of
//!   a moving piston, or falling for 30 real seconds, are both left as a
//!   block that keeps falling rather than one that vanishes correctly.
//! * The "cannot place" arms (`!canBeReplaced || !shouldPlace`, and
//!   `setBlock` returning `false`) simply discard the entity here with no
//!   dropped item. Vanilla spawns one when `dropItem && ENTITY_DROPS`; that
//!   needs `crate::dropitem`'s item-form-of-a-block conversion, which nothing
//!   here has wired up.
//! * `canSurvive` is read as unconditionally `true`: `FallingBlock` itself
//!   does not override `Block.canSurvive`'s default, and none of the 23
//!   `FallingBlock` subclasses in 26.2's registry does either (checked
//!   against the same `javap` pass this module cites). A future subclass
//!   that does would need this revisited.
//! * `Entity.mainSupportingBlockPos` / `onGroundNoBlocks` are not carried
//!   across ticks — a fresh [`crate::physics::Body`] is built from the
//!   entity's own persisted `x`/`y`/`z`/`movement`/`on_ground` each tick, so
//!   `check_supporting_block`'s "shifted probe" fallback never has a previous
//!   `on_ground_no_blocks` to read. This cannot affect landing detection
//!   (`on_ground` itself comes from this tick's collision, not from that
//!   fallback); it can only affect the friction/`block_speed_factor` input to
//!   [`crate::physics::move_entity`] on a tick where the direct probe finds
//!   nothing, which is not the falling-straight-down case any scenario here
//!   exercises.
//! * The `Replacing` context this builds for `canBeReplaced` passes
//!   `table: None`, so a falling block landing exactly on a slab or a snow
//!   layer gets the flag-only default rule
//!   (`crate::placement::can_be_replaced`) rather than either override. Both
//!   overrides only diverge from the default when the falling block's own
//!   item matches the block being landed on, which a `FallingBlock` never is
//!   (`DirectionalPlaceContext` is built with `ItemStack.EMPTY`), **except**
//!   `SnowLayerBlock.canBeReplaced` returns `layers == 1` outright for an
//!   empty item — a single-layer snow is replaceable by a landing block and
//!   the flag-only fallback may disagree. Named rather than fixed: no
//!   recording has exercised it.

use crate::collide::{CollisionBlocks, CollisionShapes, Vec3};
use crate::edits::Blocks;
use crate::entities::Entities;
use crate::fallable::{Cargo, Support};
use crate::interact::Face;
use crate::movement_props::MovementProps;
use crate::physics::{self, MoverType, Profile};
use crate::placement::Replacing;
use crate::tracker::{TrackedPlayer, Tracker};

/// [`CollisionBlocks`] over anything that implements [`Blocks`] — the bridge
/// between the two position types the two halves of this crate use:
/// `crate::edits::Blocks` speaks `crate::interact::BlockPos`, the `(i32, i32,
/// i32)` tuple every block rule in this crate takes; `crate::collide` speaks
/// `rustmc_data::BlockPos`, the struct `crate::physics::move_entity` and
/// everything under it were ported against. Neither module depends on the
/// other's position type, so the conversion lives here, at the one place they
/// meet.
pub(crate) struct WorldCollision<'a, B: Blocks + ?Sized>(pub(crate) &'a B);

impl<B: Blocks + ?Sized> CollisionBlocks for WorldCollision<'_, B> {
    /// `Blocks::state_at` then `Blocks::state_id`. `0` — air's own global id —
    /// for a position [`Blocks::state_id`] does not know, which is the same
    /// "treat the unreadable as the safest wrong answer" convention
    /// [`crate::fallable::is_free`] documents for its own missing tables: a
    /// position this server cannot resolve does not stop an entity as if it
    /// were solid.
    fn state_id_at(&self, pos: rustmc_data::BlockPos) -> u32 {
        let state = self.0.state_at((pos.x, pos.y, pos.z));
        self.0
            .state_id(&state)
            .and_then(|id| u32::try_from(id).ok())
            .unwrap_or(0)
    }
}

/// Everything a landing needs beyond the world and the entity list — the
/// tables `crate::status::listen` already loads once at boot, threaded
/// through rather than re-read per entity per tick.
pub struct Tables<'a> {
    pub shapes: Option<&'a CollisionShapes>,
    pub props: Option<&'a MovementProps>,
    pub support: Support<'a>,
}

/// One tick for every live `minecraft:falling_block`: gravity, the move, and
/// a landing if this tick produced one.
///
/// Call once per server tick, after the scheduled block ticks and before
/// `changed.flush` is called again — see the module docs for why the second
/// flush is what makes the queued half of a landing go out in the same tick
/// as the immediate half.
///
/// # Panics
/// If `entities`' or `tracker`'s lock is poisoned — the same precondition
/// every other caller of either already carries.
#[allow(clippy::too_many_arguments)]
pub fn tick<W: Blocks + ?Sized>(
    entities: &Entities,
    world: &W,
    tables: &Tables<'_>,
    changed: &crate::broadcast::ChangedBlocks,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let (Some(shapes), Some(props)) = (tables.shapes, tables.props) else {
        return;
    };
    // Snapshot the ids first: `move_entity` needs no lock, but `entities`'
    // own is not re-entrant across the loop body, which also calls
    // `entities.update` and, on a landing, `entities.remove`.
    let falling: Vec<i32> = entities
        .all()
        .into_iter()
        .filter(|e| e.kind == crate::entities::FALLING_BLOCK)
        .map(|e| e.id)
        .collect();
    let wrapped = WorldCollision(world);
    for id in falling {
        let Some(e) = entities.get(id) else { continue };
        let mut body = physics::Body::at(Vec3::new(e.x, e.y, e.z));
        body.movement = Vec3::new(e.movement.0, e.movement.1, e.movement.2);
        body.on_ground = e.on_ground;
        let profile = Profile::FALLING_BLOCK;
        physics::apply_gravity(&mut body, profile.gravity);
        let vec = body.movement;
        let moved = physics::move_entity(
            shapes,
            props,
            &wrapped,
            &profile,
            &mut body,
            MoverType::SelfMove,
            vec,
        );
        // `NoShapeFor` — a state this server's collision table has no answer
        // for, `docs/AGENT-BRIEF.md`'s "raise rather than guess". Left where
        // it is rather than moved: a block that cannot be swept correctly
        // stops rather than falling through or standing on air.
        if moved.is_err() {
            continue;
        }
        entities.update(id, |ent| {
            ent.x = body.pos.x;
            ent.y = body.pos.y;
            ent.z = body.pos.z;
            ent.movement = (body.movement.x, body.movement.y, body.movement.z);
            ent.on_ground = body.on_ground;
        });
        if !body.on_ground {
            continue;
        }
        land(
            id, &body, world, tables, entities, changed, tracker, players,
        );
    }
}

/// `FallingBlockEntity.tick()`'s landing branch, reached only once `on_ground`
/// is true this tick. Always ends by removing the entity — see the module
/// docs for the "cannot place" arms this collapses into a bare discard.
#[allow(clippy::too_many_arguments)]
fn land<W: Blocks + ?Sized>(
    id: i32,
    body: &physics::Body,
    world: &W,
    tables: &Tables<'_>,
    entities: &Entities,
    changed: &crate::broadcast::ChangedBlocks,
    tracker: &Tracker,
    players: &[TrackedPlayer],
) {
    let Some(state) = Cargo::shared().take(id) else {
        // No cargo recorded — cannot place anything faithfully. Discard
        // rather than guess a state, the same refusal `fallable::is_free`
        // takes on a missing table.
        entities_discard(id, entities, tracker, players);
        return;
    };
    // `Entity.blockPosition()`: `Mth.floor` on each axis of the entity's own
    // (feet) position, taken *after* this tick's move.
    #[allow(clippy::cast_possible_truncation)]
    let pos: crate::interact::BlockPos = (
        body.pos.x.floor() as i32,
        body.pos.y.floor() as i32,
        body.pos.z.floor() as i32,
    );
    let below = (pos.0, pos.1 - 1, pos.2);
    let is_free_below = crate::fallable::is_free(&tables.support, &world.state_at(below));
    // `canSurvive` is `true` unconditionally — see the module docs.
    let should_place = !is_free_below;
    // `DirectionalPlaceContext(level, pos, DOWN, ItemStack.EMPTY, UP)`:
    // `getClickedFace()` reads the *last* Direction argument, `UP` — see the
    // module docs' bytecode reading of the constructor. `item: None` is
    // `ItemStack.EMPTY`; `face`/`replacing_clicked`/`cursor_y` beyond that are
    // inert for an empty item on every path but the snow-layer one the module
    // docs already name as a gap.
    let ctx = Replacing {
        item: None,
        face: Face::Up,
        cursor_y: 0.0,
        replacing_clicked: false,
        secondary_use: false,
        table: None,
    };
    let can_be_replaced = world.replaceable(pos, &ctx);
    if can_be_replaced && should_place {
        if let Some(new_id) = world.set_block(pos, &state) {
            if let Ok(body_bytes) = crate::interact::block_update_body(pos, new_id) {
                tracker.send_to_tracking_players(
                    id,
                    players,
                    rustmc_proto::ids::play::cb::BLOCK_UPDATE,
                    &body_bytes,
                );
            }
            changed.record(pos);
        }
    }
    entities_discard(id, entities, tracker, players);
}

fn entities_discard(id: i32, entities: &Entities, tracker: &Tracker, players: &[TrackedPlayer]) {
    entities.remove(id);
    tracker.remove(id, players);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A world whose every column is air except a named floor, for the
    /// landing arithmetic — the physics engine itself is `move_entity`'s own
    /// suite, and `crates/rustmc-server/tests/entity_move.rs`'s.
    #[derive(Default)]
    struct Fake {
        floor_y: i32,
        writes: Mutex<HashMap<crate::interact::BlockPos, String>>,
    }

    impl Blocks for Fake {
        fn state_at(&self, pos: crate::interact::BlockPos) -> String {
            if let Some(s) = self.writes.lock().expect("writes").get(&pos) {
                return s.clone();
            }
            if pos.1 <= self.floor_y {
                "minecraft:stone".to_string()
            } else {
                "minecraft:air".to_string()
            }
        }
        fn set_block(&self, pos: crate::interact::BlockPos, state: &str) -> Option<i32> {
            self.writes
                .lock()
                .expect("writes")
                .insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, _block: &str) -> Option<String> {
            None
        }
        fn has_state(&self, _state: &str) -> bool {
            true
        }
        fn state_id(&self, _state: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: crate::interact::BlockPos, _ctx: &Replacing<'_>) -> bool {
            self.state_at(pos) == "minecraft:air"
        }
        fn face_sturdy(&self, _pos: crate::interact::BlockPos, _face: Face) -> bool {
            false
        }
        fn redstone_conductor(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn signal_source(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn water_at(&self, _pos: crate::interact::BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _pos: crate::interact::BlockPos) -> String {
            "minecraft:air".to_string()
        }
        fn level_min_y(&self) -> Option<i32> {
            Some(-64)
        }
    }

    fn support() -> Support<'static> {
        Support {
            props: None,
            classes: None,
            tags: None,
        }
    }

    #[test]
    fn world_collision_reads_through_state_id() {
        let world = Fake {
            floor_y: 0,
            ..Fake::default()
        };
        let wrapped = WorldCollision(&world);
        // Both arms of `Fake::state_id` answer `Some(1)`, so every position —
        // air included — reads back as global id 1. The point of this test
        // is only that the bridge does not panic and does not silently
        // invent a different id than the world's own answer.
        assert_eq!(
            wrapped.state_id_at(rustmc_data::BlockPos { x: 0, y: 0, z: 0 }),
            1
        );
    }

    #[test]
    fn a_missing_cargo_entry_discards_without_writing() {
        // `land` reached with nothing in `Cargo` — must not guess a state.
        let world = Fake {
            floor_y: -1,
            ..Fake::default()
        };
        let body = physics::Body::at(Vec3::new(0.5, 1.0, 0.5));
        let tables = Tables {
            shapes: None,
            props: None,
            support: support(),
        };
        let entities = Entities::new();
        let changed = crate::broadcast::ChangedBlocks::default();
        let tracker = Tracker::new();
        land(
            999,
            &body,
            &world,
            &tables,
            &entities,
            &changed,
            &tracker,
            &[],
        );
        assert!(world.writes.lock().expect("writes").is_empty());
    }

    #[test]
    fn landing_writes_the_carried_state_when_the_ground_is_solid() {
        Cargo::shared().set(-12345, "minecraft:sand".to_string());
        // Floor at y = -1, so the block at y = 0 sits on solid ground and
        // `is_free(below)` is false — `should_place` is true.
        let world = Fake {
            floor_y: -1,
            ..Fake::default()
        };
        let body = physics::Body::at(Vec3::new(1.5, 0.0, 0.5));
        let tables = Tables {
            shapes: None,
            props: None,
            support: support(),
        };
        let entities = Entities::new();
        let changed = crate::broadcast::ChangedBlocks::default();
        let tracker = Tracker::new();
        land(
            -12345,
            &body,
            &world,
            &tables,
            &entities,
            &changed,
            &tracker,
            &[],
        );
        assert_eq!(
            world.writes.lock().expect("writes").get(&(1, 0, 0)),
            Some(&"minecraft:sand".to_string())
        );
        // The cargo entry is gone either way — `take`, not a read.
        assert_eq!(Cargo::shared().take(-12345), None);
    }

    #[test]
    fn landing_over_a_free_below_does_not_place() {
        // No floor at all: `is_free(below)` is true (air), so `should_place`
        // is false and the entity is discarded without a write — the "still
        // has somewhere to go" arm `FallingBlock.isFree` exists to catch,
        // even though `on_ground` being true here is itself an input this
        // test asserts rather than derives from `move_entity`.
        Cargo::shared().set(-12346, "minecraft:sand".to_string());
        let world = Fake {
            floor_y: -100,
            ..Fake::default()
        };
        let body = physics::Body::at(Vec3::new(1.5, 0.0, 0.5));
        let tables = Tables {
            shapes: None,
            props: None,
            support: support(),
        };
        let entities = Entities::new();
        let changed = crate::broadcast::ChangedBlocks::default();
        let tracker = Tracker::new();
        land(
            -12346,
            &body,
            &world,
            &tables,
            &entities,
            &changed,
            &tracker,
            &[],
        );
        assert!(world.writes.lock().expect("writes").is_empty());
    }
}
