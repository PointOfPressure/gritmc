//! `FallingBlock` — the block half of a gravity block, up to the entity it
//! creates.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `FallingBlock.onPlace`, `FallingBlock.updateShape`, `FallingBlock.tick`,
//! `FallingBlock.getDelayAfterPlace`, `FallingBlock.isFree`,
//! `FallingBlockEntity.fall`, and
//! `BlockBehaviour$BlockStateBase.liquid`/`.canBeReplaced`.
//!
//! # What this module is, said before anything else
//!
//! **The block half only.** `FallingBlock.tick` ends in
//! `FallingBlockEntity.fall`, and everything from that call onwards — the
//! entity, its gravity, its landing — belongs to [`crate::physics`] and the
//! entity list. This module's output is a *request* to spawn one, in the same
//! spirit as [`crate::shapes::TickRequest`]: the rule that decides a block
//! falls is reached from six connection handlers and from chunk generation,
//! none of which own the entity id allocator.
//!
//! `docs/PARITY-ROADMAP.md` §95 carries the measurement this exists to close
//! — scenario `070`, `diverge` on a missing `add_entity`, reference 10
//! observations against subject 2 — and the five-observation-per-client trace
//! that is the target.
//!
//! # The schedule is unconditional and the test is in the tick
//!
//! This is the part a plausible reimplementation gets backwards, so it is
//! stated before the code. Both hooks are one instruction apart from being
//! the same method:
//!
//! ```java
//! // FallingBlock.onPlace, offsets 0-10
//! level.scheduleTick(pos, this, this.getDelayAfterPlace());
//! // FallingBlock.updateShape, offsets 0-13
//! tickAccess.scheduleTick(pos, this, this.getDelayAfterPlace());
//! return super.updateShape(...);
//! ```
//!
//! **Neither asks whether the block is unsupported.** There is no `isFree`
//! call, no `canSurvive`, no test of any kind: a gravity block schedules a
//! tick every time anything touches it, and the tick is where the question
//! gets asked. A port that filters at schedule time is a different function —
//! it drops the tick that fires *after* the support has gone, because the
//! support went away while the tick was already booked.
//!
//! `getDelayAfterPlace()` is `iconst_2`, so the delay is **2** and not the 1
//! that a "next tick" reading would give.
//!
//! `updateShape` is the neighbour hook and it returns `super`'s answer, so it
//! is a side effect on the way through rather than a state change. That makes
//! it the same shape as `ObserverBlock.updateShape`'s `startSignal`, which is
//! why [`update_shape`] sits beside `crate::power::observer_update_shape` in
//! `crate::shapes::execute_shape_update` and not in the shape table: an
//! extracted table records the state a block answers with, and this block
//! answers with its own.
//!
//! # `isFree` is four tests and only one of them is "air"
//!
//! ```java
//! public static boolean isFree(BlockState state) {
//!     return state.isAir()
//!         || state.is(BlockTags.FIRE)
//!         || state.liquid()
//!         || state.canBeReplaced();
//! }
//! ```
//!
//! Both of the two-field ones are plain `getfield`s on `BlockStateBase` and
//! neither means what its name suggests:
//!
//! * **`canBeReplaced()` is the no-argument one**, `getfield replaceable:Z`,
//!   and *not* `canBeReplaced(BlockPlaceContext)`. The context-taking overload
//!   is `replaceable && !item.is(asItem())` and is what
//!   `crate::edits::Blocks::replaceable` serves; using it here would make sand
//!   fall through a slab it is holding. This module takes the flag.
//! * **`liquid()` is `getfield liquid:Z`, which is `Properties.liquid()` at
//!   registration and NOT the fluid state.** A waterlogged slab has a
//!   non-empty `getFluidState()` and `liquid == false`, so reading the fluid
//!   state here would let sand fall straight through every waterlogged block
//!   in the game. `Properties.liquid()` is called only by the `LiquidBlock`
//!   registrations in 26.2, so it is derived here from the class map — the
//!   same `set_placed_by.json` classes the tick dispatcher uses to ask whether
//!   a block is a `ButtonBlock` — rather than from a flag `block_props.json`
//!   does not carry. [`is_free`]'s tests pin water, lava and a waterlogged
//!   slab against each other so a wrong derivation cannot pass quietly.
//!
//! The fire arm is the **tag**, not the class: `#minecraft:fire` is
//! `[minecraft:fire, minecraft:soul_fire]` in the pinned extract, and a tag
//! and a class hierarchy are different objects even when they agree today.

use crate::interact::BlockPos;
use crate::shapes::{Effects, TickRequest};

/// `FallingBlock.getDelayAfterPlace()` — `iconst_2`.
pub const DELAY_AFTER_PLACE: i64 = 2;

/// Vanilla's class for a gravity block. Every gravity block in 26.2 is a
/// subclass of it — `ColoredFallingBlock`, `ConcretePowderBlock`,
/// `SandBlock` — so the test is [`crate::placedby::PlacedByTable::is_a`] and
/// never a string compare, for the reason that method's own docs give.
pub const FALLING_BLOCK_CLASS: &str = "FallingBlock";

/// Vanilla's class for water and lava.
///
/// Used only to derive `BlockStateBase.liquid`. See the module docs for why
/// this is a class test and not a fluid-state read.
pub const LIQUID_BLOCK_CLASS: &str = "LiquidBlock";

/// `BlockTags.FIRE`.
pub const FIRE_TAG: &str = "minecraft:fire";

/// A `data/vanilla/<name>` path that works from the repo root and from a
/// package directory.
///
/// One function rather than the same six lines in each loader below, and the
/// same shape and carried risk as `crate::power::shared()`:
/// `docs/PARITY-ROADMAP.md` §34. `cargo test`'s cwd is the package directory,
/// not the repo root, which is the whole reason for the fallback.
fn extract_path(name: &str) -> std::path::PathBuf {
    let rel = std::path::PathBuf::from("data/vanilla").join(name);
    if rel.exists() {
        return rel;
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

/// The block id of a state — `minecraft:sand[x=1]` -> `minecraft:sand`.
fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

/// Whether `block` is a `FallingBlock`.
///
/// `false` when there is no class map, which is the same convention
/// [`crate::placedby::PlacedByTable::class_of`] documents: a block we cannot
/// ask about is "not that class", never "assume it is".
#[must_use]
pub fn is_gravity_block(classes: Option<&crate::placedby::PlacedByTable>, block: &str) -> bool {
    classes.is_some_and(|c| c.is_a(block, FALLING_BLOCK_CLASS))
}

/// Everything [`is_free`] needs to answer, borrowed rather than owned.
///
/// A struct because the four arms read three different tables and threading
/// three `Option<&_>`s through `crate::ticks::run` and
/// `crate::shapes::execute_shape_update` would put the same three arguments on
/// every function between here and them. Same shape as `crate::rails::Rails`.
#[derive(Clone, Copy)]
pub struct Support<'a> {
    /// `BlockStateBase.isAir` and `.canBeReplaced` — `block_props.json`.
    pub props: Option<&'a rustmc_behavior::block_props::BlockPropsTable>,
    /// `BlockStateBase.liquid`, via `LiquidBlock` — `set_placed_by.json`.
    pub classes: Option<&'a crate::placedby::PlacedByTable>,
    /// `BlockTags.FIRE` — `tags_block.json`.
    pub tags: Option<&'a rustmc_worldgen::tags::BlockTags>,
}

/// `FallingBlock.isFree(BlockState)`.
///
/// # What a missing table does, and why it is not "false"
///
/// Each arm answers `false` when the table it needs is absent, so a tree with
/// no extracts reads every state as **not free** and nothing falls. That is
/// the safe direction — a block that does not fall is a visible divergence,
/// whereas a block that falls through the world floor because a table was
/// missing is a corrupted world — and it is the same convention as
/// `is_gravity_block` above. It is *not* silent: the loader prints, and
/// [`Support::complete`] says whether the answer can be trusted at all.
#[must_use]
pub fn is_free(support: &Support<'_>, state: &str) -> bool {
    if crate::shapes::is_air(state) {
        return true;
    }
    let block = block_of(state);
    if support.tags.is_some_and(|t| t.contains(FIRE_TAG, block)) {
        return true;
    }
    if support
        .classes
        .is_some_and(|c| c.is_a(block, LIQUID_BLOCK_CLASS))
    {
        return true;
    }
    support
        .props
        .and_then(|p| p.get(state))
        .is_some_and(|p| p.air || p.replaceable)
}

impl Support<'_> {
    /// Whether every arm of [`is_free`] has the table it reads.
    ///
    /// A caller that is about to let a block fall can refuse instead, which is
    /// what `crate::ticks` does: a partial `isFree` is a different predicate
    /// and the difference is silent in exactly the direction that loses
    /// blocks.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.props.is_some() && self.classes.is_some() && self.tags.is_some()
    }
}

/// `FallingBlock.updateShape` and `FallingBlock.onPlace` — the schedule, which
/// is the whole of both.
///
/// Called for the block **at** `pos`, whatever changed next to it, because
/// that is what vanilla does: no direction is consulted and no support is
/// tested. See the module docs.
pub fn update_shape(is_gravity: bool, pos: BlockPos, state: &str, updates: &mut Effects) {
    if !is_gravity {
        return;
    }
    updates.ticks.push(TickRequest {
        pos,
        // The **block**, not the state — `ScheduledTick.type` is the `Block`
        // and the queue's uniqueness is keyed on it, so two shape updates in
        // one tick book one tick and not two. That deduplication is vanilla's
        // and it is why the unconditional schedule above is not a flood.
        block: block_of(state).to_string(),
        delay: DELAY_AFTER_PLACE,
        priority: crate::ticks::PRIORITY_NORMAL,
    });
}

/// `FallingBlock.onPlace(state, level, pos, oldState, isMoving)`.
///
/// The same one line as [`update_shape`] — `level.scheduleTick(pos, this,
/// getDelayAfterPlace())` — and a separate function only because the two are
/// separate overrides reached from separate places, so a reader looking for
/// either finds it. Placing a gravity block and disturbing one both book a
/// tick at delay 2, and the tick decides.
pub fn on_place(is_gravity: bool, pos: BlockPos, state: &str, updates: &mut Effects) {
    update_shape(is_gravity, pos, state, updates);
}

/// What [`tick`] decided.
///
/// A request and not a spawn, for the reason in the module docs: the rule runs
/// where there is no entity id allocator. `crate::ticks::Effects` carries it
/// to the drain site, which has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallRequest {
    /// The position the block leaves, as `crate::interact::BlockPos` —
    /// the `(x, y, z)` tuple every block rule in this crate speaks. The entity goes to its centre in x/z and
    /// its **floor** in y — `FallingBlockEntity.fall` is
    /// `(getX() + 0.5, getY(), getZ() + 0.5)`, so y is not centred.
    pub pos: BlockPos,
    /// The state the entity carries and will try to place on landing, with
    /// `WATERLOGGED` cleared if it had it. `fall` does that before the
    /// constructor, so the entity never carries a waterlogged state.
    pub state: String,
}

impl FallRequest {
    /// `FallingBlockEntity.fall`'s spawn position — `(x + 0.5, y, z + 0.5)`.
    #[must_use]
    pub fn spawn_at(&self) -> (f64, f64, f64) {
        (
            f64::from(self.pos.0) + 0.5,
            f64::from(self.pos.1),
            f64::from(self.pos.2) + 0.5,
        )
    }
}

/// `new FallingBlockEntity(level, x, y, z, state)` plus `addFreshEntity`'s
/// half that this crate owns: allocate the id, build the record, insert it.
///
/// Telling the players is the caller's next call — `crate::tracker::Tracker::add`,
/// which is the port of `ChunkMap.addEntity`. The split is not arbitrary: this
/// function needs the entity store and that one needs the player list, and
/// keeping them apart is what lets the drain site put both **inside** the
/// scheduled-tick phase, which is what makes `add_entity` precede the block
/// change. See [`crate::shapes::Effects::falls`].
///
/// `state_id` is `Block.getId(blockState)`, which travels in
/// `ClientboundAddEntityPacket`'s trailing `data` varint — scenario `070`
/// records `data: 118` for sand. It is the caller's because only the world
/// knows the id.
///
/// # What is deliberately not carried, said rather than left silent
///
/// **`DATA_START_POS`.** `FallingBlockEntity`'s constructor sets it to the
/// block position, so it is away from its default from the first tick and
/// vanilla sends a `set_entity_data` for every falling block — the recorded
/// `070` trace has two of them. This server sends none, exactly as it sends
/// none for an `ItemEntity`'s `DATA_ITEM` (`crate::dropitem`'s own note). It
/// is invisible to `070`, whose `observe` list is
/// `["block_update", "section_blocks_update", "add_entity"]`, so **a green
/// there is not evidence this is right** — which is the reason it is written
/// down here instead of being discovered later.
///
/// # Panics
/// If the entity-store lock is poisoned.
pub fn spawn(
    entities: &crate::entities::Entities,
    fall: &FallRequest,
    state_id: i32,
) -> crate::entities::Entity {
    let (x, y, z) = fall.spawn_at();
    let mut rng = crate::dropitem::unique_random();
    let e = crate::entities::Entity {
        id: entities.next_id(),
        // `Entity`'s constructor: `this.uuid = Mth.createInsecureUUID(this.random)`
        // on an unseeded generator, so this is unreproducible in the same way
        // and for the same reason a dropped item's is. The oracle consumes the
        // sixteen bytes and never digests them.
        uuid: crate::dropitem::insecure_uuid(&mut rng),
        kind: crate::entities::FALLING_BLOCK,
        x,
        y,
        z,
        // **All three rotations are zero and that is not a default, it is the
        // measurement.** An `ItemEntity` takes a random spawn yaw and a falling
        // block does not — `FallingBlockEntity` has no `setYRot` anywhere in
        // its constructor — so unlike a dropped item this entity's angles are
        // fully deterministic and `070`'s recorded `add_entity` carries
        // `y_rot 0, x_rot 0, y_head_rot 0`. If these ever disagree with a
        // recording, it is our defect and not vanilla's noise.
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        // `Entity.deltaMovement` starts at `Vec3.ZERO`; the first `applyGravity`
        // runs on the entity's *next* tick, not at construction. So the spawn
        // packet describes something stationary, which is why `070`'s
        // `add_entity` carries one byte of movement and its first
        // `move_entity_heartbeat` is `d = [0, 0, 0]`.
        movement: (0.0, 0.0, 0.0),
        on_ground: false,
        data: state_id,
        // Not a `LivingEntity`. Same three fields as a dropped item and for
        // the same reason.
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: crate::synched::Synched::EntityOnly,
    };
    entities.insert(e.clone());
    e
}

/// `FallingBlock.tick(state, level, pos, random)`.
///
/// ```java
///   if (isFree(level.getBlockState(pos.below())) && pos.getY() >= level.getMinY()) {
///       FallingBlockEntity e = FallingBlockEntity.fall(level, pos, state);
///       this.falling(e);          // Block's own is `return`
///   }
/// ```
///
/// `min_y` is the level's, not a constant: `getMinY()` is
/// `-64` in the overworld and `0` in the other two, and a hard-coded `-64`
/// would let a block fall out of the nether.
///
/// Returns `None` when the block stays put, which includes the case where
/// [`Support::complete`] is false — see [`is_free`].
#[must_use]
pub fn tick(
    support: &Support<'_>,
    below: &str,
    pos: BlockPos,
    state: &str,
    min_y: i32,
) -> Option<FallRequest> {
    if !support.complete() {
        return None;
    }
    if !is_free(support, below) || pos.1 < min_y {
        return None;
    }
    Some(FallRequest {
        pos,
        state: clear_waterlogged(state),
    })
}

/// `state.hasProperty(WATERLOGGED) ? state.setValue(WATERLOGGED, false) : state`
/// — `FallingBlockEntity.fall` offsets 28-56.
#[must_use]
pub fn clear_waterlogged(state: &str) -> String {
    if !state.contains("waterlogged=true") {
        return state.to_string();
    }
    state.replace("waterlogged=true", "waterlogged=false")
}

/// What a spawned falling-block entity will place back on landing, keyed by
/// entity id.
///
/// **Why this exists rather than a field on [`crate::entities::Entity`].**
/// `Entity` carries only what a client is told — `data`, the network state
/// id — and `DATA_START_POS` is already the one field `spawn`'s docs say this
/// server does not send. The state *string* `FallingBlockEntity.blockState`
/// holds is never on the wire at all; vanilla keeps it as a private Java
/// field, and this is that field's port. [`crate::movement`] is the only
/// reader, at the tick that finds the entity on the ground.
#[derive(Debug, Default)]
pub struct Cargo(std::sync::Mutex<std::collections::BTreeMap<i32, String>>);

impl Cargo {
    /// The process-wide table, alongside [`crate::entities::Entities::shared`].
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: std::sync::OnceLock<Cargo> = std::sync::OnceLock::new();
        SHARED.get_or_init(Cargo::default)
    }

    /// Record what entity `id` is carrying, from [`spawn`]'s own request.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn set(&self, id: i32, state: String) {
        self.0.lock().expect("cargo").insert(id, state);
    }

    /// Take and forget what `id` was carrying — the landing tick's read, and
    /// the only way an entry leaves this table, so a falling block that never
    /// lands leaks one `String` for its lifetime and no longer than it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn take(&self, id: i32) -> Option<String> {
        self.0.lock().expect("cargo").remove(&id)
    }
}

/// `block_props.json`, for the two `isFree` arms that read it.
///
/// Path resolution is [`extract_path`]'s.
#[must_use]
pub fn props() -> Option<&'static rustmc_behavior::block_props::BlockPropsTable> {
    static TABLE: std::sync::OnceLock<Option<rustmc_behavior::block_props::BlockPropsTable>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let path = extract_path("block_props.json");
            if !rustmc_data::fixture::available(
                &path,
                "block_props extract",
                "ci/gen-gameplay-tables.sh",
            ) {
                return None;
            }
            match rustmc_behavior::block_props::BlockPropsTable::load(&path) {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!(
                        "block_props extract at {} did not load: {e}",
                        path.display()
                    );
                    None
                }
            }
        })
        .as_ref()
}

/// `tags_block.json`, resolved, for the `BlockTags.FIRE` arm.
///
/// The extract is one file keyed by unprefixed tag stem with a `values` array,
/// where `BlockTags::load` wants a directory of those files — so this maps the
/// one into the other rather than adding a second on-disk convention.
#[must_use]
pub fn tags() -> Option<&'static rustmc_worldgen::tags::BlockTags> {
    static TABLE: std::sync::OnceLock<Option<rustmc_worldgen::tags::BlockTags>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let path = extract_path("tags_block.json");
            if !rustmc_data::fixture::available(
                &path,
                "tags_block extract",
                "ci/gen-gameplay-tables.sh",
            ) {
                return None;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("tags_block extract at {} did not read: {e}", path.display());
                    return None;
                }
            };
            let json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("tags_block.json is not JSON ({e})");
                    return None;
                }
            };
            let Some(map) = json.as_object() else {
                eprintln!("tags_block.json is not an object");
                return None;
            };
            let mut raw: std::collections::BTreeMap<String, Vec<String>> =
                std::collections::BTreeMap::new();
            for (stem, body) in map {
                let Some(values) = body.get("values").and_then(serde_json::Value::as_array) else {
                    continue;
                };
                let names = values
                    .iter()
                    .filter_map(|v| match v {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(_) => v
                            .get("id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string),
                        _ => None,
                    })
                    .collect();
                raw.insert(format!("minecraft:{stem}"), names);
            }
            match rustmc_worldgen::tags::BlockTags::from_entries(raw) {
                Ok(t) => Some(t),
                Err(e) => {
                    eprintln!("tags_block.json will not resolve ({e})");
                    None
                }
            }
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classes() -> Option<&'static crate::placedby::PlacedByTable> {
        crate::placedby::PlacedByTable::shared(&extract_path("set_placed_by.json")).ok()
    }

    #[test]
    fn the_delay_is_two_not_one() {
        // `getDelayAfterPlace` is `iconst_2`. A "next tick" reading gives 1 and
        // moves every `add_entity` in scenario 070 one tick earlier.
        assert_eq!(DELAY_AFTER_PLACE, 2);
    }

    #[test]
    fn air_is_free_without_any_table_at_all() {
        // The first arm is `isAir()` and `crate::shapes::is_air` answers it
        // from the state string, so a tree with no extracts still gets this
        // one right. Everything else needs a table.
        let none = Support {
            props: None,
            classes: None,
            tags: None,
        };
        assert!(is_free(&none, "minecraft:air"));
        assert!(!is_free(&none, "minecraft:stone"));
    }

    #[test]
    fn a_missing_table_reads_as_not_free_and_the_tick_refuses() {
        // The safe direction, and it must be a refusal rather than a `false`
        // that looks like a measurement: a partial `isFree` would let sand
        // fall through anything it could not classify.
        let none = Support {
            props: None,
            classes: None,
            tags: None,
        };
        assert!(!none.complete());
        assert_eq!(
            tick(&none, "minecraft:air", (0, 5, 0), "minecraft:sand", -64),
            None,
            "an incomplete Support must not spawn a falling block"
        );
    }

    #[test]
    fn waterlogged_is_cleared_and_nothing_else_is_touched() {
        assert_eq!(
            clear_waterlogged("minecraft:oak_slab[type=bottom,waterlogged=true]"),
            "minecraft:oak_slab[type=bottom,waterlogged=false]"
        );
        // Not waterlogged, and a state that merely contains the word.
        assert_eq!(clear_waterlogged("minecraft:sand"), "minecraft:sand");
        assert_eq!(
            clear_waterlogged("minecraft:oak_slab[waterlogged=false]"),
            "minecraft:oak_slab[waterlogged=false]"
        );
    }

    #[test]
    fn the_spawn_position_centres_x_and_z_but_not_y() {
        // `FallingBlockEntity.fall` is `(getX() + 0.5, getY(), getZ() + 0.5)`.
        // Scenario 070's recorded `add_entity` is at [1.5, 1.0, 0.5] for a
        // sand at block [1, 1, 0], which is the whole of this assertion.
        let r = FallRequest {
            pos: (1, 1, 0),
            state: "minecraft:sand".to_string(),
        };
        assert_eq!(r.spawn_at(), (1.5, 1.0, 0.5));
    }

    #[test]
    fn update_shape_schedules_for_a_gravity_block_and_nothing_else() {
        // `fixture::available` panics in the default `required` mode, so a tree
        // without the extract fails this test instead of passing it silently.
        if !rustmc_data::fixture::available(
            &extract_path("set_placed_by.json"),
            "set_placed_by extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let classes = classes().expect("set_placed_by extract is present and did not load");
        // The runtime gravity test is `ShapeTable::is_gravity_block`, built
        // from this same class map by `with_classes`; the predicate is passed
        // in so this rule needs neither table threaded through it.
        let mut updates = Effects::new();
        update_shape(
            is_gravity_block(Some(classes), "minecraft:sand"),
            (1, 1, 0),
            "minecraft:sand[foo=bar]",
            &mut updates,
        );
        assert_eq!(updates.ticks.len(), 1, "sand is a FallingBlock");
        assert_eq!(updates.ticks[0].delay, 2);
        assert_eq!(
            updates.ticks[0].block, "minecraft:sand",
            "the queue is keyed on the block, not the state"
        );
        assert_eq!(updates.ticks[0].pos, (1, 1, 0));

        // Stone is not, and the schedule must not be "everything".
        let mut updates = Effects::new();
        update_shape(
            is_gravity_block(Some(classes), "minecraft:stone"),
            (1, 1, 0),
            "minecraft:stone",
            &mut updates,
        );
        assert!(updates.ticks.is_empty());
    }

    #[test]
    fn gravel_and_concrete_powder_are_gravity_blocks_through_their_subclasses() {
        // The reason this is `is_a` and not a string compare: gravel is a
        // `ColoredFallingBlock` and concrete powder a `ConcretePowderBlock`,
        // and neither class name is `FallingBlock`.
        // `fixture::available` panics in the default `required` mode, so a tree
        // without the extract fails this test instead of passing it silently.
        if !rustmc_data::fixture::available(
            &extract_path("set_placed_by.json"),
            "set_placed_by extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let classes = classes().expect("set_placed_by extract is present and did not load");
        for block in [
            "minecraft:sand",
            "minecraft:red_sand",
            "minecraft:gravel",
            "minecraft:white_concrete_powder",
            "minecraft:anvil",
            "minecraft:dragon_egg",
        ] {
            assert!(
                is_gravity_block(Some(classes), block),
                "{block} should be a FallingBlock"
            );
        }
        // **`suspicious_sand` is a `BrushableBlock`, not a `FallingBlock`**, and
        // this test asserted the opposite until the class map refused it. It is
        // the best negative case there is here: a block whose *name* is sand
        // and whose behaviour is a block entity being brushed. Nothing about
        // "sand" in an id implies gravity, which is the whole argument for
        // reading the class map instead of the id.
        for block in [
            "minecraft:stone",
            "minecraft:oak_slab",
            "minecraft:water",
            "minecraft:suspicious_sand",
            "minecraft:suspicious_gravel",
        ] {
            assert!(
                !is_gravity_block(Some(classes), block),
                "{block} should not be a FallingBlock"
            );
        }
    }

    #[test]
    fn there_are_twenty_three_gravity_blocks_in_the_registry() {
        // A count, so that a class map that stopped resolving superclasses —
        // or one that started matching everything — cannot pass this file.
        // `is_a` walking the chain is load-bearing: `SandBlock` extends
        // `ColoredFallingBlock` extends `FallingBlock`, three levels, and a
        // simple-name compare finds **zero** of the 23.
        // `fixture::available` panics in the default `required` mode, so a tree
        // without the extract fails this test instead of passing it silently.
        if !rustmc_data::fixture::available(
            &extract_path("set_placed_by.json"),
            "set_placed_by extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let classes = classes().expect("set_placed_by extract is present and did not load");
        let n = classes
            .class_pairs()
            .keys()
            .filter(|b| is_gravity_block(Some(classes), b))
            .count();
        assert_eq!(n, 23, "FallingBlock instances in the 26.2 registry");
    }

    #[test]
    fn liquid_is_the_registration_flag_and_a_waterlogged_slab_is_not_free() {
        // **The test this module exists to not get wrong.** Reading
        // `getFluidState()` instead of the `liquid` field passes on water and
        // lava and then lets sand fall through every waterlogged block in the
        // game. A waterlogged slab is the separating case: non-empty fluid
        // state, `liquid == false`, `replaceable == false`.
        // `fixture::available` panics in the default `required` mode, so a tree
        // without the extract fails this test instead of passing it silently.
        if !rustmc_data::fixture::available(
            &extract_path("set_placed_by.json"),
            "set_placed_by extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let classes = classes().expect("set_placed_by extract is present and did not load");
        // `props()` gates on `fixture::available`, which panics in the default
        // `required` mode, so `None` here is a table that is present and did not load.
        let props = props().expect("block_props extract is present and did not load");
        // `tags()` gates on `fixture::available`, which panics in the default
        // `required` mode, so `None` here is a table that is present and did not load.
        let tags = tags().expect("tags_block extract is present and did not load");
        let s = Support {
            props: Some(props),
            classes: Some(classes),
            tags: Some(tags),
        };
        assert!(s.complete());
        assert!(is_free(&s, "minecraft:water[level=0]"), "water is liquid()");
        assert!(is_free(&s, "minecraft:lava[level=0]"), "lava is liquid()");
        assert!(
            !is_free(&s, "minecraft:oak_slab[type=bottom,waterlogged=true]"),
            "a waterlogged slab has a fluid state and liquid == false"
        );
        assert!(!is_free(&s, "minecraft:stone"));
    }

    #[test]
    fn the_fire_tag_is_free_and_it_is_the_tag() {
        // `tags()` gates on `fixture::available`, which panics in the default
        // `required` mode, so `None` here is a table that is present and did not load.
        let tags = tags().expect("tags_block extract is present and did not load");
        let s = Support {
            props: None,
            classes: None,
            tags: Some(tags),
        };
        assert!(is_free(&s, "minecraft:fire[age=0]"));
        assert!(is_free(&s, "minecraft:soul_fire"));
        // A campfire is in `#campfires`, not `#fire`, and is not free.
        assert!(!is_free(&s, "minecraft:campfire"));
    }

    #[test]
    fn the_tick_tests_below_and_the_world_floor() {
        // `fixture::available` panics in the default `required` mode, so a tree
        // without the extract fails this test instead of passing it silently.
        if !rustmc_data::fixture::available(
            &extract_path("set_placed_by.json"),
            "set_placed_by extract",
            "ci/gen-gameplay-tables.sh",
        ) {
            return;
        }
        let classes = classes().expect("set_placed_by extract is present and did not load");
        // `props()` gates on `fixture::available`, which panics in the default
        // `required` mode, so `None` here is a table that is present and did not load.
        let props = props().expect("block_props extract is present and did not load");
        // `tags()` gates on `fixture::available`, which panics in the default
        // `required` mode, so `None` here is a table that is present and did not load.
        let tags = tags().expect("tags_block extract is present and did not load");
        let s = Support {
            props: Some(props),
            classes: Some(classes),
            tags: Some(tags),
        };
        // Scenario 070 exactly: sand at [1,1,0], air below after the dig.
        assert_eq!(
            tick(&s, "minecraft:air", (1, 1, 0), "minecraft:sand", -64),
            Some(FallRequest {
                pos: (1, 1, 0),
                state: "minecraft:sand".to_string()
            })
        );
        // Supported: stone below.
        assert_eq!(
            tick(&s, "minecraft:stone", (1, 1, 0), "minecraft:sand", -64),
            None
        );
        // `pos.getY() >= level.getMinY()` — at the floor it still falls, below
        // it does not. The comparison is `>=` and an off-by-one here is a
        // block that vanishes at the bottom of the world.
        assert!(tick(&s, "minecraft:air", (1, -64, 0), "minecraft:sand", -64).is_some());
        assert!(tick(&s, "minecraft:air", (1, -65, 0), "minecraft:sand", -64).is_none());
        // A nether floor of 0 is why `min_y` is an argument.
        assert!(tick(&s, "minecraft:air", (1, -1, 0), "minecraft:sand", 0).is_none());
    }

    #[test]
    fn cargo_is_taken_exactly_once() {
        // A fresh table per test — `Cargo::shared()` is one process-wide
        // instance and a second test using it would see the first one's ids.
        let cargo = Cargo::default();
        cargo.set(7, "minecraft:sand".to_string());
        assert_eq!(cargo.take(7), Some("minecraft:sand".to_string()));
        assert_eq!(cargo.take(7), None, "take removes the entry");
    }
}
