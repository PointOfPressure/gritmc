//! The tree feature.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `TreeFeature`, `TreeConfiguration`, `TrunkPlacer`,
//! `FoliagePlacer`, `FeatureSize`.
//!
//! # The draw order is the tree
//!
//! Four values come off the stream before a single block is read, in this
//! order and no other:
//!
//! ```text
//! treeHeight    = trunkPlacer.getTreeHeight(random)     2 draws
//! foliageHeight = foliagePlacer.foliageHeight(...)      0 or more
//! foliageRadius = foliagePlacer.foliageRadius(random)   an IntProvider
//! offset        = foliagePlacer.offset(random)          an IntProvider, and
//!                                                       only once the
//!                                                       foliage actually runs
//! ```
//!
//! The last two are `IntProvider`s, so whether they draw is the *config's*
//! business: oak's radius is `ConstantInt`, which samples without touching
//! the stream, while a uniform one takes a draw. Counting them as one draw
//! each is as wrong as counting them as none.
//!
//! `getTreeHeight` is `baseHeight + nextInt(a + 1) + nextInt(b + 1)` — **two**
//! draws even when `b` is zero, because `nextInt(1)` still consumes one.
//! Reading it as one draw shifts every tree in the chunk and everything after
//! them in the step.
//!
//! # A tree that does not fit still costs its draws
//!
//! The height checks come after the four draws, so a tree refused for want of
//! headroom leaves the stream four values further on. That is the same shape
//! as the ore feature's discard chance and it is the usual way a port drifts:
//! bail early, save the draws, and every later feature in the step lands
//! somewhere else.
//!
//! # `getMaxFreeTreeHeight` clips rather than refuses
//!
//! It walks up from the trunk origin testing a square of side
//! `2 * sizeAtHeight + 1` at each level, and returns `y - 2` at the first
//! blocked level — so a tree under an overhang grows *shorter* rather than
//! not at all, provided the config's `minimum_size` names a
//! `min_clipped_height` it still clears. The `- 2` is not a fencepost slip:
//! the level that failed is unusable and so is the one below it.
//!
//! # What this module does not do yet
//!
//! Seven of the nine trunk placers and eight of the eleven foliage placers,
//! the root placers, and every decorator. Each is refused by name rather than
//! approximated — a tree placed by the wrong placer is a tree in the right
//! place with the wrong shape, which reads as a noise bug rather than a
//! missing port.

pub mod decorator;
pub mod foliage;
pub mod leaves;
pub mod size;
pub mod trunk;

use super::block_predicate::PredicateWorld;
use super::state_provider::StateProvider;
use super::state_spec::DefaultStates;
use crate::java_hash_set::JavaHashSet;
use crate::rng::RandomSource;
use crate::tags::BlockTags;

pub use foliage::{FoliageAttachment, FoliagePlacer};
pub use size::FeatureSize;
pub use trunk::TrunkPlacer;

/// A position, as this port spells one.
pub type Pos = (i32, i32, i32);

/// The feature could not be evaluated.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `TreeConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeConfig {
    pub trunk_provider: StateProvider,
    pub trunk_placer: TrunkPlacer,
    pub foliage_provider: StateProvider,
    pub foliage_placer: FoliagePlacer,
    /// `dirt_provider` in older versions; the block put *under* the trunk.
    pub below_trunk_provider: StateProvider,
    pub minimum_size: FeatureSize,
    pub decorators: Vec<decorator::TreeDecorator>,
    pub ignore_vines: bool,
    pub force_dirt: bool,
    /// Whether the config named a `root_placer`, which this port has not.
    pub has_root_placer: bool,
}

impl TreeConfig {
    /// # Errors
    /// If a field is missing or malformed.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn DefaultStates,
        resolve_feature: &dyn Fn(
            &str,
        )
            -> Result<crate::feature::configured::ConfiguredFeature, String>,
    ) -> Result<Self, String> {
        let provider = |name: &str| -> Result<StateProvider, String> {
            StateProvider::parse(
                config
                    .get(name)
                    .ok_or_else(|| format!("tree config has no `{name}`"))?,
                states,
            )
        };
        Ok(Self {
            trunk_provider: provider("trunk_provider")?,
            trunk_placer: TrunkPlacer::parse(
                config
                    .get("trunk_placer")
                    .ok_or_else(|| "tree config has no `trunk_placer`".to_string())?,
            )?,
            foliage_provider: provider("foliage_provider")?,
            foliage_placer: FoliagePlacer::parse(
                config
                    .get("foliage_placer")
                    .ok_or_else(|| "tree config has no `foliage_placer`".to_string())?,
            )?,
            below_trunk_provider: provider("below_trunk_provider")?,
            minimum_size: FeatureSize::parse(
                config
                    .get("minimum_size")
                    .ok_or_else(|| "tree config has no `minimum_size`".to_string())?,
            )?,
            decorators: config
                .get("decorators")
                .and_then(serde_json::Value::as_array)
                .map(|list| {
                    list.iter()
                        .map(|d| decorator::TreeDecorator::parse(d, states, resolve_feature))
                        .collect::<Result<Vec<_>, String>>()
                })
                .transpose()?
                .unwrap_or_default(),
            ignore_vines: config
                .get("ignore_vines")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            force_dirt: config
                .get("force_dirt")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            has_root_placer: config.get("root_placer").is_some(),
        })
    }

    /// Whether the port can grow this tree.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.decorators
            .iter()
            .all(decorator::TreeDecorator::is_supported)
            && !self.has_root_placer
            && self.trunk_placer.is_supported()
            && self.foliage_placer.is_supported()
            && self.trunk_provider.is_supported()
            && self.foliage_provider.is_supported()
            && self.below_trunk_provider.is_supported()
    }
}

/// The world a tree reads and writes.
///
/// The leaf-distance pass wants the same two operations under different
/// names, so [`leaves::LeafWorld`] is implemented for anything that is a
/// `TreeWorld` rather than asked for separately.
pub trait TreeWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `level.getMinY()`.
    fn min_y(&self) -> i32;
    /// `level.getMaxY()` — the *inclusive* top, so `minY + height - 1`.
    fn max_y(&self) -> i32;
    /// `getHeightmapPos(MOTION_BLOCKING_NO_LEAVES, pos).getY()`, which only
    /// the `place_on_ground` decorator asks for.
    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32;
}

impl<T: TreeWorld + ?Sized> decorator::DecoratorWorld for T {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        <Self as PredicateWorld>::block_at(self, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <Self as PredicateWorld>::air_at(self, x, y, z)
    }
    fn is_solid_render(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <Self as PredicateWorld>::solid_at(self, x, y, z)
    }
    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32 {
        <Self as TreeWorld>::motion_blocking_no_leaves(self, x, z)
    }
}

impl<T: TreeWorld + ?Sized> leaves::LeafWorld for T {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        <Self as PredicateWorld>::block_at(self, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
}

/// What a tree wrote, before the leaf-distance pass.
///
/// The four sets are `java.util.HashSet`s in vanilla and their iteration
/// order reaches the decorators, so they are [`JavaHashSet`]s here. See
/// `crate::java_hash_set`.
#[derive(Debug, Default)]
pub struct Placed {
    pub roots: JavaHashSet<Pos>,
    pub logs: JavaHashSet<Pos>,
    pub foliage: JavaHashSet<Pos>,
    pub decorations: JavaHashSet<Pos>,
}

/// A world that remembers what it overwrote, so a refusal can put it back.
///
/// # Why an undo log and not a write buffer
///
/// A tree **reads its own writes** while placing: `TrunkPlacer.isFree`
/// accepts a position that already holds a log, `validTreePos` asks whether a
/// block is air or replaceable, and the decorators walk what the trunk left.
/// Buffering the writes and committing at the end would therefore change what
/// the placement *sees*, not only what it leaves — a different feature, not a
/// safer one.
///
/// Writing through and recording the previous state changes nothing on the
/// successful path **by construction**: every read during placement is served
/// by the real world exactly as before, and the log is simply dropped. Only a
/// refusal behaves differently, and only by undoing what it already did.
///
/// The old state of a position is recorded **once**, the first time it is
/// written, so a position written twice still restores to what was there
/// before the tree started.
struct UndoWorld<'a, W: TreeWorld + ?Sized> {
    inner: &'a mut W,
    undo: Vec<(Pos, String)>,
    touched: std::collections::BTreeSet<Pos>,
    /// Block-entity records this tree made, held back until it commits.
    ///
    /// A record is not a block write and `rollback` cannot reach it: the
    /// canvas's `record_block_entity` appends to a per-chunk list with no
    /// remove. So a refused tree that had already recorded one would leave a
    /// block entity behind at a position it had just restored — the block
    /// gone, the record still there. Buffering here and flushing only on
    /// success is what makes the wrapper's "all or nothing" true of records
    /// as well as of blocks.
    block_entities: Vec<(i32, i32, i32, crate::block_entity::Record)>,
}

impl<'a, W: TreeWorld + ?Sized> UndoWorld<'a, W> {
    fn new(inner: &'a mut W) -> Self {
        Self {
            inner,
            undo: Vec::new(),
            touched: std::collections::BTreeSet::new(),
            block_entities: Vec::new(),
        }
    }
    /// Put back everything this tree wrote, newest first.
    fn rollback(&mut self) {
        while let Some((pos, state)) = self.undo.pop() {
            self.inner.set_block(pos.0, pos.1, pos.2, &state);
        }
        self.touched.clear();
        self.block_entities.clear();
    }
}

impl<W: crate::block_entity::BlockEntityRecorder + TreeWorld + ?Sized> UndoWorld<'_, W> {
    /// Hand the buffered records to the real world. Called only where the
    /// tree has already succeeded, so it is the counterpart of `rollback`.
    fn commit_block_entities(&mut self) {
        for (x, y, z, record) in std::mem::take(&mut self.block_entities) {
            self.inner.record_block_entity(x, y, z, record);
        }
    }
}

impl<W: crate::block_entity::BlockEntityRecorder + TreeWorld + ?Sized>
    crate::block_entity::BlockEntityRecorder for UndoWorld<'_, W>
{
    fn record_block_entity(&mut self, x: i32, y: i32, z: i32, record: crate::block_entity::Record) {
        self.block_entities.push((x, y, z, record));
    }
}

impl<W: crate::feature::block_pile::BlockPileWorld + TreeWorld + ?Sized>
    crate::feature::block_pile::BlockPileWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::block_pile::BlockPileWorld>::is_empty_block(&mut *self.inner, x, y, z)
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::block_pile::BlockPileWorld>::min_y(&*self.inner)
    }
}

impl<W: TreeWorld + ?Sized> PredicateWorld for UndoWorld<'_, W> {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.inner.block_at(x, y, z)
    }
    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.inner.fluid_at(x, y, z)
    }
    fn would_survive(&mut self, state: &str, x: i32, y: i32, z: i32) -> Option<bool> {
        self.inner.would_survive(state, x, y, z)
    }
    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.inner.face_is_sturdy(x, y, z, direction)
    }
    fn can_support_center(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.inner.can_support_center(x, y, z, direction)
    }
    fn face_is_collision_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.inner.face_is_collision_full(x, y, z, direction)
    }
    fn inside_build_height(&self, y: i32) -> Option<bool> {
        self.inner.inside_build_height(y)
    }
    fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.inner.is_solid_at(x, y, z)
    }
    fn replaceable_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.inner.replaceable_at(x, y, z)
    }
    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.inner.air_at(x, y, z)
    }
    fn solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.inner.solid_at(x, y, z)
    }
}

impl<W: TreeWorld + ?Sized> TreeWorld for UndoWorld<'_, W> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        let pos = (x, y, z);
        if self.touched.insert(pos) {
            let was = self.inner.block_at(x, y, z);
            self.undo.push((pos, was));
        }
        self.inner.set_block(x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        self.inner.min_y()
    }
    fn max_y(&self) -> i32 {
        self.inner.max_y()
    }
    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32 {
        self.inner.motion_blocking_no_leaves(x, z)
    }
}

// ===== `UndoWorld` as a `FeatureWorld` =====
//
// A tree decorator can place a whole configured feature: `pale_moss` places
// `minecraft:pale_moss_patch` on the ground under the lowest log. The
// decorators run inside this wrapper, so the wrapper has to satisfy
// `FeatureWorld` or the nested feature cannot be placed at all, and the
// wrapper is the only thing holding `&mut inner` so there is nothing else to
// hand the feature instead.
//
// **The one rule that makes this sound: every `set_block` forwards to
// `<Self as TreeWorld>::set_block`, never to `self.inner`.** Twenty of the
// twenty-two traits below declare a `set_block` with an identical signature,
// and each is a write that a refusal must be able to undo. Sending them all
// through the one recording path means the undo log covers the nested
// feature's blocks as well as the tree's, so a decorator that refuses after
// the patch is placed still puts the whole chunk back. Delegating any of them
// straight to `inner` would compile, place correctly, and silently leave that
// feature's blocks behind on a rollback.
//
// Read-only methods delegate to `inner` directly. **Defaulted methods are
// delegated too, not left to their defaults** — `MagmaWorld::is_water`,
// `SimpleBlockWorld::is_water_at` and `PlacementWorld::set_running_feature`
// all have trait defaults that the real `Decoration` world overrides, and
// inheriting the default here would answer a different question inside a tree
// than outside one. The existing `PredicateWorld` impl above already sets that
// precedent for its own eight defaults.
//
// The bound is `W: <that trait> + TreeWorld`, per trait, rather than
// `W: FeatureWorld` on the struct: the wrapper is a `FeatureWorld` exactly
// when the world it wraps is one, and stays usable as a plain `TreeWorld`
// wrapper for anything that only needs the tree half.

impl<W: crate::feature::ore::OreWorld + TreeWorld + ?Sized> crate::feature::ore::OreWorld
    for UndoWorld<'_, W>
{
    fn ocean_floor(&mut self, x: i32, z: i32) -> i32 {
        <W as crate::feature::ore::OreWorld>::ocean_floor(&mut *self.inner, x, z)
    }
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        <W as crate::feature::ore::OreWorld>::block_at(&mut *self.inner, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn outside_build_height(&self, y: i32) -> bool {
        <W as crate::feature::ore::OreWorld>::outside_build_height(&*self.inner, y)
    }
    fn can_write(&self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::ore::OreWorld>::can_write(&*self.inner, x, y, z)
    }
}

impl<W: crate::feature::disk::DiskWorld + TreeWorld + ?Sized> crate::feature::disk::DiskWorld
    for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        <W as crate::feature::disk::DiskWorld>::mark_above_for_post_processing(
            &mut *self.inner,
            x,
            y,
            z,
        );
    }
}

impl<W: crate::feature::underwater_magma::MagmaWorld + TreeWorld + ?Sized>
    crate::feature::underwater_magma::MagmaWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn face_is_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        <W as crate::feature::underwater_magma::MagmaWorld>::face_is_full(
            &mut *self.inner,
            x,
            y,
            z,
            direction,
        )
    }
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::underwater_magma::MagmaWorld>::is_air(&mut *self.inner, x, y, z)
    }
    fn is_water(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::underwater_magma::MagmaWorld>::is_water(&mut *self.inner, x, y, z)
    }
    fn is_water_or_air(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::underwater_magma::MagmaWorld>::is_water_or_air(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
}

impl<W: crate::feature::simple_block::SimpleBlockWorld + TreeWorld + ?Sized>
    crate::feature::simple_block::SimpleBlockWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::simple_block::SimpleBlockWorld>::is_empty_block(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::simple_block::SimpleBlockWorld>::is_water_at(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    // Delegated and not inherited, which is this impl's standing rule for a
    // defaulted method and is load-bearing here in a way the others are not:
    // the default answers `None`, which every caller must read as a refusal,
    // so inheriting it makes a nested feature refuse inside a tree while the
    // same feature places outside one. That is what it did — 85 of window A's
    // 101 pale moss carpet declines survived the level random being wired in,
    // because every one of them was inside a `PaleMossDecorator`.
    fn level_random_next_boolean(&mut self) -> Option<bool> {
        <W as crate::feature::simple_block::SimpleBlockWorld>::level_random_next_boolean(
            &mut *self.inner,
        )
    }
}

impl<W: crate::feature::spring::SpringWorld + TreeWorld + ?Sized>
    crate::feature::spring::SpringWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::spring::SpringWorld>::is_empty_block(&mut *self.inner, x, y, z)
    }
}

impl<W: crate::feature::freeze_top_layer::FreezeWorld + TreeWorld + ?Sized>
    crate::feature::freeze_top_layer::FreezeWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
    fn motion_blocking_height(&mut self, x: i32, z: i32) -> i32 {
        <W as crate::feature::freeze_top_layer::FreezeWorld>::motion_blocking_height(
            &mut *self.inner,
            x,
            z,
        )
    }
    fn cold_enough_to_snow(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::freeze_top_layer::FreezeWorld>::cold_enough_to_snow(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    fn precipitation_is_snow(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::freeze_top_layer::FreezeWorld>::precipitation_is_snow(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::freeze_top_layer::FreezeWorld>::is_water_at(&mut *self.inner, x, y, z)
    }
    fn snow_can_survive(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::freeze_top_layer::FreezeWorld>::snow_can_survive(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
}

impl<W: crate::feature::block_column::ColumnWorld + TreeWorld + ?Sized>
    crate::feature::block_column::ColumnWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, name);
    }
}

impl<W: crate::feature::multiface_growth::MultifaceWorld + TreeWorld + ?Sized>
    crate::feature::multiface_growth::MultifaceWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn mark_pos_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        <W as crate::feature::multiface_growth::MultifaceWorld>::mark_pos_for_post_processing(
            &mut *self.inner,
            x,
            y,
            z,
        );
    }
}

impl<W: crate::feature::aquatic::AquaticWorld + TreeWorld + ?Sized>
    crate::feature::aquatic::AquaticWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn ocean_floor_height(&mut self, x: i32, z: i32) -> i32 {
        <W as crate::feature::aquatic::AquaticWorld>::ocean_floor_height(&mut *self.inner, x, z)
    }
}

impl<W: crate::feature::monster_room::MonsterRoomWorld + TreeWorld + ?Sized>
    crate::feature::monster_room::MonsterRoomWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn can_replace(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::monster_room::MonsterRoomWorld>::can_replace(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::monster_room::MonsterRoomWorld>::min_y(&*self.inner)
    }
}

impl<W: crate::feature::blue_ice::BlueIceWorld + TreeWorld + ?Sized>
    crate::feature::blue_ice::BlueIceWorld for UndoWorld<'_, W>
{
    fn sea_level(&mut self) -> i32 {
        <W as crate::feature::blue_ice::BlueIceWorld>::sea_level(&mut *self.inner)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
}

impl<W: crate::feature::iceberg::IcebergWorld + TreeWorld + ?Sized>
    crate::feature::iceberg::IcebergWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn sea_level(&mut self) -> i32 {
        <W as crate::feature::iceberg::IcebergWorld>::sea_level(&mut *self.inner)
    }
}

impl<W: crate::feature::vines::VineWorld + TreeWorld + ?Sized> crate::feature::vines::VineWorld
    for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
}

impl<W: crate::feature::lake::LakeWorld + TreeWorld + ?Sized> crate::feature::lake::LakeWorld
    for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::lake::LakeWorld>::min_y(&*self.inner)
    }
    fn is_liquid(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::lake::LakeWorld>::is_liquid(&mut *self.inner, x, y, z)
    }
    fn should_freeze(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::lake::LakeWorld>::should_freeze(&mut *self.inner, x, y, z)
    }
    fn mark_above_for_post_processing(&mut self, x: i32, y: i32, z: i32) {
        <W as crate::feature::lake::LakeWorld>::mark_above_for_post_processing(
            &mut *self.inner,
            x,
            y,
            z,
        );
    }
}

impl<W: crate::feature::geode::GeodeWorld + TreeWorld + ?Sized> crate::feature::geode::GeodeWorld
    for UndoWorld<'_, W>
{
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        <W as crate::feature::geode::GeodeWorld>::block_at(&mut *self.inner, x, y, z)
    }
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn is_air(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::geode::GeodeWorld>::is_air(&mut *self.inner, x, y, z)
    }
    fn is_fluid_source(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::geode::GeodeWorld>::is_fluid_source(&mut *self.inner, x, y, z)
    }
    fn has_fluid(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::geode::GeodeWorld>::has_fluid(&mut *self.inner, x, y, z)
    }
    fn world_seed(&mut self) -> i64 {
        <W as crate::feature::geode::GeodeWorld>::world_seed(&mut *self.inner)
    }
}

impl<W: crate::feature::speleothem::SpeleothemWorld + TreeWorld + ?Sized>
    crate::feature::speleothem::SpeleothemWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn is_water_at(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::speleothem::SpeleothemWorld>::is_water_at(&mut *self.inner, x, y, z)
    }
}

impl<W: crate::feature::large_dripstone::LargeDripstoneWorld + TreeWorld + ?Sized>
    crate::feature::large_dripstone::LargeDripstoneWorld for UndoWorld<'_, W>
{
    fn world_surface_wg(&mut self, x: i32, z: i32) -> i32 {
        <W as crate::feature::large_dripstone::LargeDripstoneWorld>::world_surface_wg(
            &mut *self.inner,
            x,
            z,
        )
    }
}

impl<W: crate::feature::huge_mushroom::HugeMushroomWorld + TreeWorld + ?Sized>
    crate::feature::huge_mushroom::HugeMushroomWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::huge_mushroom::HugeMushroomWorld>::min_y(&*self.inner)
    }
    fn max_y(&self) -> i32 {
        <W as crate::feature::huge_mushroom::HugeMushroomWorld>::max_y(&*self.inner)
    }
}

impl<W: crate::feature::vegetation_patch::PatchWorld + TreeWorld + ?Sized>
    crate::feature::vegetation_patch::PatchWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        <W as crate::feature::vegetation_patch::PatchWorld>::is_empty_block(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        <W as crate::feature::vegetation_patch::PatchWorld>::face_is_sturdy(
            &mut *self.inner,
            x,
            y,
            z,
            direction,
        )
    }
}

impl<W: crate::feature::nether::NetherWorld + TreeWorld + ?Sized>
    crate::feature::nether::NetherWorld for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::nether::NetherWorld>::min_y(&*self.inner)
    }
    fn max_y(&self) -> i32 {
        <W as crate::feature::nether::NetherWorld>::max_y(&*self.inner)
    }
    fn sea_level(&mut self) -> i32 {
        <W as crate::feature::nether::NetherWorld>::sea_level(&mut *self.inner)
    }
}

impl<W: crate::feature::end::EndWorld + TreeWorld + ?Sized> crate::feature::end::EndWorld
    for UndoWorld<'_, W>
{
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        <Self as TreeWorld>::set_block(self, x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::end::EndWorld>::min_y(&*self.inner)
    }
    fn world_seed(&mut self) -> i64 {
        <W as crate::feature::end::EndWorld>::world_seed(&mut *self.inner)
    }
}

impl<W: crate::feature::placement::PlacementWorld + TreeWorld + ?Sized>
    crate::feature::placement::PlacementWorld for UndoWorld<'_, W>
{
    fn block_tags(&self) -> &crate::tags::BlockTags {
        <W as crate::feature::placement::PlacementWorld>::block_tags(&*self.inner)
    }
    fn biome_info_noise(&mut self, x: f64, z: f64) -> f64 {
        <W as crate::feature::placement::PlacementWorld>::biome_info_noise(&mut *self.inner, x, z)
    }
    fn min_y(&self) -> i32 {
        <W as crate::feature::placement::PlacementWorld>::min_y(&*self.inner)
    }
    fn gen_depth(&self) -> i32 {
        <W as crate::feature::placement::PlacementWorld>::gen_depth(&*self.inner)
    }
    fn heightmap(&mut self, kind: crate::feature::placement::HeightmapKind, x: i32, z: i32) -> i32 {
        <W as crate::feature::placement::PlacementWorld>::heightmap(&mut *self.inner, kind, x, z)
    }
    fn set_running_feature(&mut self, name: &str) {
        <W as crate::feature::placement::PlacementWorld>::set_running_feature(
            &mut *self.inner,
            name,
        );
    }
    fn biome_lists_this_feature(&mut self, x: i32, y: i32, z: i32) -> bool {
        <W as crate::feature::placement::PlacementWorld>::biome_lists_this_feature(
            &mut *self.inner,
            x,
            y,
            z,
        )
    }
}

/// `TreeFeature#place`, and **all or nothing**.
///
/// Vanilla never refuses a tree, so it writes straight to the level and the
/// question does not arise. This port can still refuse — an unported config,
/// or `java_hash_set` declining a treeified bucket whose spread hashes tie —
/// and a refusal used to propagate out of `do_place` leaving everything
/// already written in the world, with `update_leaves` never reaching it. That
/// is worse than placing nothing: it pollutes a diff from both directions at
/// once, leaving leaves that vanilla decayed *and* missing the ones the tree
/// never got to. Over the village window it was 4,049 blocks of undecayed
/// spruce.
///
/// So the write path is wrapped in an [`UndoWorld`] and a refusal puts the
/// world back.
///
/// # Errors
/// If the config names something unported, or the world cannot answer a
/// block question. In either case the world is unchanged.
pub fn place(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    origin: Pos,
) -> Result<bool, NeedsRules> {
    let mut undo = UndoWorld::new(world);
    match place_all_or_nothing(&mut undo, tags, config, random, origin) {
        Ok(placed) => {
            undo.commit_block_entities();
            Ok(placed)
        }
        Err(e) => {
            undo.rollback();
            Err(e)
        }
    }
}

fn place_all_or_nothing(
    world: &mut (dyn crate::feature::configured::FeatureWorld + '_),
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    origin: Pos,
) -> Result<bool, NeedsRules> {
    if !config.is_supported() {
        return Err(NeedsRules(format!(
            "tree config: {} trunk, {} foliage, decorators {:?}{}",
            config.trunk_placer.name(),
            config.foliage_placer.name(),
            config
                .decorators
                .iter()
                .map(decorator::TreeDecorator::name)
                .collect::<Vec<_>>(),
            if config.has_root_placer {
                ", a root placer"
            } else {
                ""
            }
        )));
    }

    let mut placed = Placed::default();
    // A refusal from inside `do_place` is re-labelled with the placers that
    // were running, because the bare message ("a bucket reached 8 entries")
    // says nothing about *which* tree gave up — and the answer turned out to
    // be the whole finding: every leaf this port leaves undecayed belongs to
    // one kind of tree.
    //
    // A refusal here *is* undone: `place` runs this whole function against an
    // [`UndoWorld`] and rolls it back, so a refused tree is an absent tree
    // rather than a half-built one whose leaves keep the provider's
    // `distance=7`. That was worth 4,049 blocks of undecayed spruce on the
    // village window before the undo existed.
    let ok = do_place(world, tags, config, random, origin, &mut placed).map_err(|e| {
        NeedsRules(format!(
            "{} ({} trunk, {} foliage)",
            e.0,
            config.trunk_placer.name(),
            config.foliage_placer.name()
        ))
    })?;
    if !ok {
        return Ok(false);
    }
    // Vanilla's `!flag || (logs.isEmpty() && foliage.isEmpty())`.
    if placed.logs.is_empty() && placed.foliage.is_empty() {
        return Ok(false);
    }
    // The decorators, in the config's order, over a context built from what
    // the tree wrote.
    if !config.decorators.is_empty() {
        let context = decorator::Context::new(&placed);
        for d in &config.decorators {
            decorator::place(world, tags, d, random, &context, &mut placed)?;
        }
    }

    // `BoundingBox.encapsulatingPositions(concat(roots, logs, foliage,
    // decorations))` — every position the tree touched, in that order. The
    // order does not change the box, but it is the order vanilla walks and
    // the concat is what makes an all-empty tree return false above rather
    // than here.
    let mut all: Vec<Pos> = placed.roots.to_vec();
    all.extend(placed.logs.to_vec());
    all.extend(placed.foliage.to_vec());
    all.extend(placed.decorations.to_vec());
    let Some(box_) = leaves::BoundingBox::encapsulating(&all) else {
        return Ok(false);
    };
    leaves::update_leaves(world, tags, box_, &placed)?;
    Ok(true)
}

/// `TreeFeature#doPlace`.
fn do_place<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    origin: Pos,
    placed: &mut Placed,
) -> Result<bool, NeedsRules> {
    // The four draws, in order, before anything is read. See the module docs.
    let tree_height = config.trunk_placer.tree_height(random);
    let foliage_height = config.foliage_placer.foliage_height(random, tree_height);
    let trunk_height = tree_height - foliage_height;
    let foliage_radius = config.foliage_placer.foliage_radius(random, trunk_height);

    // `rootPlacer.map(...).orElse(origin)` — no root placer, so the trunk
    // starts at the origin.
    let trunk_origin = origin;

    let low = origin.1.min(trunk_origin.1);
    let high = origin.1.max(trunk_origin.1) + tree_height + 1;
    if low < world.min_y() + 1 || high > world.max_y() + 1 {
        return Ok(false);
    }

    let free = max_free_tree_height(world, tags, config, tree_height, trunk_origin)?;
    if free < tree_height {
        match config.minimum_size.min_clipped_height() {
            Some(min) if free >= min => {}
            _ => return Ok(false),
        }
    }

    let attachments =
        config
            .trunk_placer
            .place_trunk(world, tags, config, random, free, trunk_origin, placed)?;
    for attachment in &attachments {
        config.foliage_placer.create_foliage(
            world,
            tags,
            config,
            random,
            attachment,
            foliage_height,
            foliage_radius,
            placed,
        )?;
    }
    Ok(true)
}

/// `TreeFeature#getMaxFreeTreeHeight`.
///
/// Returns the tree height that fits, which is `treeHeight` when nothing is
/// in the way and `y - 2` at the first blocked level otherwise.
fn max_free_tree_height<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    tree_height: i32,
    origin: Pos,
) -> Result<i32, NeedsRules> {
    for y in 0..=tree_height + 1 {
        let size = config.minimum_size.size_at_height(tree_height, y);
        for dx in -size..=size {
            for dz in -size..=size {
                let pos = (origin.0 + dx, origin.1 + y, origin.2 + dz);
                let free = is_free(world, tags, pos)?;
                if !free || (!config.ignore_vines && is_vine(world, pos)) {
                    return Ok(y - 2);
                }
            }
        }
    }
    Ok(tree_height)
}

/// `TrunkPlacer#isFree`, which is `TreeFeature.validTreePos` or a leaf.
fn is_free<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    if valid_tree_pos(world, tags, pos)? {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(tags.contains("minecraft:logs", plain(&state)))
}

/// `TreeFeature#validTreePos` — air, or in `#replaceable_by_trees`.
///
/// `pub(crate)` because `FallenTreeFeature` calls the same static: it is not
/// a tree, but it asks the tree question.
pub(crate) fn valid_tree_pos<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    let air = world
        .air_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| NeedsRules("isAir".to_string()))?;
    if air {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(tags.contains("minecraft:replaceable_by_trees", plain(&state)))
}

/// `TreeFeature#isAirOrLeaves` — air, or anything in `#minecraft:leaves`.
///
/// **Not [`valid_tree_pos`] and not [`is_free`].** `validTreePos` accepts
/// `#replaceable_by_trees` (grass, ferns, flowers) and `isFree` widens that to
/// logs; this one accepts leaves and nothing replaceable. `DarkOakTrunkPlacer`
/// uses it as the gate on a whole 2x2 layer, so borrowing either of the other
/// two would place a dark oak's trunk through a flower and refuse it through
/// its own canopy.
pub(crate) fn is_air_or_leaves<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    let air = world
        .air_at(pos.0, pos.1, pos.2)
        .ok_or_else(|| NeedsRules("isAir".to_string()))?;
    if air {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(tags.contains("minecraft:leaves", plain(&state)))
}

fn is_vine<W: TreeWorld + ?Sized>(world: &mut W, pos: Pos) -> bool {
    let state = world.block_at(pos.0, pos.1, pos.2);
    plain(&state) == "minecraft:vine"
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

/// A `TreeWorld` presented as a [`FeatureWorld`], for unit tests of the tree
/// layer.
///
/// [`place`] requires a `FeatureWorld` because `pale_moss` places a whole
/// configured feature from inside a tree decorator. That is right for the
/// production world, which is one, and it would otherwise force every small
/// unit-test world in this module to answer sixty-six methods about oceans,
/// geodes and the Nether in order to plant an oak in a 5x5 grid.
///
/// **Everything outside `PredicateWorld` and `TreeWorld` panics, naming
/// itself.** That is the point rather than a shortcut: a unit test that starts
/// reaching these has grown a nested feature and wants the real `Decoration`
/// world, and a panic saying `PatchWorld::set_block on a tree-only test world`
/// says exactly that. Answering with a plausible default (`false`, `0`, a
/// dropped write) would let such a test pass while measuring nothing, which is
/// the failure mode this project keeps finding in its own checks.
///
/// [`FeatureWorld`]: crate::feature::configured::FeatureWorld
#[cfg(test)]
pub(crate) struct TreeOnly<'w, W: TreeWorld + ?Sized>(pub &'w mut W);

#[cfg(test)]
impl<W: TreeWorld + ?Sized> PredicateWorld for TreeOnly<'_, W> {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.0.block_at(x, y, z)
    }
    fn fluid_at(&mut self, x: i32, y: i32, z: i32) -> String {
        self.0.fluid_at(x, y, z)
    }
    fn would_survive(&mut self, state: &str, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.would_survive(state, x, y, z)
    }
    fn face_is_sturdy(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.0.face_is_sturdy(x, y, z, direction)
    }
    fn can_support_center(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.0.can_support_center(x, y, z, direction)
    }
    fn face_is_collision_full(&mut self, x: i32, y: i32, z: i32, direction: usize) -> Option<bool> {
        self.0.face_is_collision_full(x, y, z, direction)
    }
    fn inside_build_height(&self, y: i32) -> Option<bool> {
        self.0.inside_build_height(y)
    }
    fn is_solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.is_solid_at(x, y, z)
    }
    fn replaceable_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.replaceable_at(x, y, z)
    }
    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.air_at(x, y, z)
    }
    fn solid_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        self.0.solid_at(x, y, z)
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> TreeWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        self.0.set_block(x, y, z, state);
    }
    fn min_y(&self) -> i32 {
        self.0.min_y()
    }
    fn max_y(&self) -> i32 {
        self.0.max_y()
    }
    fn motion_blocking_no_leaves(&mut self, x: i32, z: i32) -> i32 {
        self.0.motion_blocking_no_leaves(x, z)
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::ore::OreWorld for TreeOnly<'_, W> {
    fn ocean_floor(&mut self, _x: i32, _z: i32) -> i32 {
        unimplemented!("OreWorld::ocean_floor on a tree-only test world")
    }
    fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
        unimplemented!("OreWorld::block_at on a tree-only test world")
    }
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("OreWorld::set_block on a tree-only test world")
    }
    fn outside_build_height(&self, _y: i32) -> bool {
        unimplemented!("OreWorld::outside_build_height on a tree-only test world")
    }
    fn can_write(&self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("OreWorld::can_write on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::disk::DiskWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("DiskWorld::set_block on a tree-only test world")
    }
    fn mark_above_for_post_processing(&mut self, _x: i32, _y: i32, _z: i32) {
        unimplemented!("DiskWorld::mark_above_for_post_processing on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::underwater_magma::MagmaWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("MagmaWorld::set_block on a tree-only test world")
    }
    fn face_is_full(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
        unimplemented!("MagmaWorld::face_is_full on a tree-only test world")
    }
    fn is_air(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("MagmaWorld::is_air on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::simple_block::SimpleBlockWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("SimpleBlockWorld::set_block on a tree-only test world")
    }
    fn is_empty_block(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("SimpleBlockWorld::is_empty_block on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::spring::SpringWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("SpringWorld::set_block on a tree-only test world")
    }
    fn is_empty_block(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("SpringWorld::is_empty_block on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::freeze_top_layer::FreezeWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("FreezeWorld::set_block on a tree-only test world")
    }
    fn motion_blocking_height(&mut self, _x: i32, _z: i32) -> i32 {
        unimplemented!("FreezeWorld::motion_blocking_height on a tree-only test world")
    }
    fn cold_enough_to_snow(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("FreezeWorld::cold_enough_to_snow on a tree-only test world")
    }
    fn precipitation_is_snow(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("FreezeWorld::precipitation_is_snow on a tree-only test world")
    }
    fn is_water_at(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("FreezeWorld::is_water_at on a tree-only test world")
    }
    fn snow_can_survive(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("FreezeWorld::snow_can_survive on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::block_column::ColumnWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("ColumnWorld::set_block on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::multiface_growth::MultifaceWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("MultifaceWorld::set_block on a tree-only test world")
    }
    fn mark_pos_for_post_processing(&mut self, _x: i32, _y: i32, _z: i32) {
        unimplemented!("MultifaceWorld::mark_pos_for_post_processing on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::aquatic::AquaticWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("AquaticWorld::set_block on a tree-only test world")
    }
    fn ocean_floor_height(&mut self, _x: i32, _z: i32) -> i32 {
        unimplemented!("AquaticWorld::ocean_floor_height on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::monster_room::MonsterRoomWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("MonsterRoomWorld::set_block on a tree-only test world")
    }
    fn can_replace(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("MonsterRoomWorld::can_replace on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("MonsterRoomWorld::min_y on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::blue_ice::BlueIceWorld for TreeOnly<'_, W> {
    fn sea_level(&mut self) -> i32 {
        unimplemented!("BlueIceWorld::sea_level on a tree-only test world")
    }
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("BlueIceWorld::set_block on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::iceberg::IcebergWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("IcebergWorld::set_block on a tree-only test world")
    }
    fn sea_level(&mut self) -> i32 {
        unimplemented!("IcebergWorld::sea_level on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::vines::VineWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("VineWorld::set_block on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::lake::LakeWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("LakeWorld::set_block on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("LakeWorld::min_y on a tree-only test world")
    }
    fn is_liquid(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("LakeWorld::is_liquid on a tree-only test world")
    }
    fn should_freeze(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("LakeWorld::should_freeze on a tree-only test world")
    }
    fn mark_above_for_post_processing(&mut self, _x: i32, _y: i32, _z: i32) {
        unimplemented!("LakeWorld::mark_above_for_post_processing on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::geode::GeodeWorld for TreeOnly<'_, W> {
    fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
        unimplemented!("GeodeWorld::block_at on a tree-only test world")
    }
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("GeodeWorld::set_block on a tree-only test world")
    }
    fn is_air(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("GeodeWorld::is_air on a tree-only test world")
    }
    fn is_fluid_source(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("GeodeWorld::is_fluid_source on a tree-only test world")
    }
    fn has_fluid(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("GeodeWorld::has_fluid on a tree-only test world")
    }
    fn world_seed(&mut self) -> i64 {
        unimplemented!("GeodeWorld::world_seed on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::speleothem::SpeleothemWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("SpeleothemWorld::set_block on a tree-only test world")
    }
    fn is_water_at(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("SpeleothemWorld::is_water_at on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::large_dripstone::LargeDripstoneWorld
    for TreeOnly<'_, W>
{
    fn world_surface_wg(&mut self, _x: i32, _z: i32) -> i32 {
        unimplemented!("LargeDripstoneWorld::world_surface_wg on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::huge_mushroom::HugeMushroomWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("HugeMushroomWorld::set_block on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("HugeMushroomWorld::min_y on a tree-only test world")
    }
    fn max_y(&self) -> i32 {
        unimplemented!("HugeMushroomWorld::max_y on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::block_entity::BlockEntityRecorder for TreeOnly<'_, W> {
    fn record_block_entity(
        &mut self,
        _x: i32,
        _y: i32,
        _z: i32,
        _record: crate::block_entity::Record,
    ) {
        unimplemented!("BlockEntityRecorder::record_block_entity on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::block_pile::BlockPileWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _name: &str) {
        unimplemented!("BlockPileWorld::set_block on a tree-only test world")
    }
    fn is_empty_block(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("BlockPileWorld::is_empty_block on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("BlockPileWorld::min_y on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::vegetation_patch::PatchWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("PatchWorld::set_block on a tree-only test world")
    }
    fn is_empty_block(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
        unimplemented!("PatchWorld::is_empty_block on a tree-only test world")
    }
    fn face_is_sturdy(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
        unimplemented!("PatchWorld::face_is_sturdy on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::nether::NetherWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("NetherWorld::set_block on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("NetherWorld::min_y on a tree-only test world")
    }
    fn max_y(&self) -> i32 {
        unimplemented!("NetherWorld::max_y on a tree-only test world")
    }
    fn sea_level(&mut self) -> i32 {
        unimplemented!("NetherWorld::sea_level on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::end::EndWorld for TreeOnly<'_, W> {
    fn set_block(&mut self, _x: i32, _y: i32, _z: i32, _state: &str) {
        unimplemented!("EndWorld::set_block on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("EndWorld::min_y on a tree-only test world")
    }
    fn world_seed(&mut self) -> i64 {
        unimplemented!("EndWorld::world_seed on a tree-only test world")
    }
}

#[cfg(test)]
impl<W: TreeWorld + ?Sized> crate::feature::placement::PlacementWorld for TreeOnly<'_, W> {
    fn block_tags(&self) -> &crate::tags::BlockTags {
        unimplemented!("PlacementWorld::block_tags on a tree-only test world")
    }
    fn biome_info_noise(&mut self, _x: f64, _z: f64) -> f64 {
        unimplemented!("PlacementWorld::biome_info_noise on a tree-only test world")
    }
    fn min_y(&self) -> i32 {
        unimplemented!("PlacementWorld::min_y on a tree-only test world")
    }
    fn gen_depth(&self) -> i32 {
        unimplemented!("PlacementWorld::gen_depth on a tree-only test world")
    }
    fn heightmap(
        &mut self,
        _kind: crate::feature::placement::HeightmapKind,
        _x: i32,
        _z: i32,
    ) -> i32 {
        unimplemented!("PlacementWorld::heightmap on a tree-only test world")
    }
    fn biome_lists_this_feature(&mut self, _x: i32, _y: i32, _z: i32) -> bool {
        unimplemented!("PlacementWorld::biome_lists_this_feature on a tree-only test world")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    fn states() -> crate::feature::state_spec::DefaultStateTable {
        crate::feature::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:oak_log", "minecraft:oak_log[axis=y]"),
            (
                "minecraft:oak_leaves",
                "minecraft:oak_leaves[distance=7,persistent=false,waterlogged=false]",
            ),
            ("minecraft:dirt", "minecraft:dirt"),
        ])
    }

    fn oak() -> TreeConfig {
        TreeConfig::parse(
            &serde_json::from_str(
                r#"{"below_trunk_provider":{"type":"minecraft:simple_state_provider",
                                            "state":{"Name":"minecraft:dirt"}},
                    "decorators":[],
                    "foliage_placer":{"type":"minecraft:blob_foliage_placer",
                                      "height":3,"offset":0,"radius":2},
                    "foliage_provider":{"type":"minecraft:simple_state_provider",
                                        "state":{"Name":"minecraft:oak_leaves"}},
                    "ignore_vines":true,
                    "minimum_size":{"type":"minecraft:two_layers_feature_size"},
                    "trunk_placer":{"type":"minecraft:straight_trunk_placer",
                                    "base_height":4,"height_rand_a":2,"height_rand_b":0},
                    "trunk_provider":{"type":"minecraft:simple_state_provider",
                                      "state":{"Name":"minecraft:oak_log"}}}"#,
            )
            .expect("json"),
            &states(),
            &decorator::no_registry,
        )
        .expect("parse")
    }

    struct Field {
        blocks: BTreeMap<Pos, String>,
    }

    impl PredicateWorld for Field {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    // `Field` answers one of the twenty-two nested traits, so that the undo
    // log can be tested through a `set_block` that is NOT `TreeWorld`'s.
    // `PatchWorld` is the one that matters: `pale_moss_patch` is a
    // `vegetation_patch`, so this is the exact path a pale oak takes.
    impl crate::feature::vegetation_patch::PatchWorld for Field {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            self.air_at(x, y, z)
        }
        fn face_is_sturdy(&mut self, _x: i32, _y: i32, _z: i32, _direction: usize) -> Option<bool> {
            Some(true)
        }
    }

    impl TreeWorld for Field {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            319
        }
        fn motion_blocking_no_leaves(&mut self, _x: i32, _z: i32) -> i32 {
            0
        }
    }

    /// The two tags the tree feature reads. Empty sets, not an empty map:
    /// `BlockTags::contains` panics on a tag that does not exist, which is
    /// the check that stopped 567 carvers touching zero blocks.
    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([
            ("minecraft:replaceable_by_trees".to_string(), Vec::new()),
            ("minecraft:logs".to_string(), Vec::new()),
            (
                "minecraft:prevents_nearby_leaf_decay".to_string(),
                vec!["minecraft:oak_log".to_string()],
            ),
        ]))
        .expect("tags")
    }

    /// `baseHeight + nextInt(a + 1) + nextInt(b + 1)` — two draws even when
    /// `b` is zero, because `nextInt(1)` still consumes one.
    #[test]
    fn the_tree_height_is_two_draws_even_with_a_zero_range() {
        let config = oak();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(12));
        let height = config.trunk_placer.tree_height(&mut random);
        assert!((4..=6).contains(&height), "height {height}");

        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(12));
        probe.next_i32_bound(3);
        probe.next_i32_bound(1);
        assert_eq!(
            random.next_i64(),
            probe.next_i64(),
            "the second nextInt(1) is a draw, not a no-op"
        );
    }

    /// A tree with no headroom still pays for the four values it drew before
    /// it looked at the world.
    #[test]
    fn a_refused_tree_still_costs_its_draws() {
        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        // A ceiling one block up: nothing fits.
        for x in -3..4 {
            for z in -3..4 {
                world
                    .blocks
                    .insert((x, 1, z), "minecraft:stone".to_string());
            }
        }
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(3));
        assert!(!place(
            &mut TreeOnly(&mut world),
            &tags(),
            &oak(),
            &mut random,
            (0, 0, 0)
        )
        .expect("ok"));

        // Two draws, and only two: `getTreeHeight` is `nextInt(3)` and
        // `nextInt(1)`. A blob's foliage height is a constant field, and its
        // radius here is `ConstantInt`, which samples without drawing — so
        // the four *values* the feature computes cost two draws between them.
        let mut probe = WorldgenRandom::new(LegacyRandomSource::new(3));
        probe.next_i32_bound(3);
        probe.next_i32_bound(1);
        assert_eq!(random.next_i64(), probe.next_i64());
    }

    /// An open field grows a trunk of logs with a blob of leaves on top.
    #[test]
    fn an_open_field_grows_a_tree() {
        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(8));
        assert!(place(
            &mut TreeOnly(&mut world),
            &tags(),
            &oak(),
            &mut random,
            (0, 0, 0)
        )
        .expect("ok"));

        let logs = world
            .blocks
            .values()
            .filter(|v| v.starts_with("minecraft:oak_log"))
            .count();
        let leaves = world
            .blocks
            .values()
            .filter(|v| v.starts_with("minecraft:oak_leaves"))
            .count();
        assert!(logs >= 4, "{logs} logs");
        assert!(leaves > 20, "{leaves} leaves");
        // The leaf-distance pass ran: the provider writes `distance=7`, which
        // decays, and everything within six blocks of wood is rewritten.
        let decaying = world
            .blocks
            .values()
            .filter(|v| v.starts_with("minecraft:oak_leaves") && v.contains("distance=7"))
            .count();
        assert_eq!(
            decaying, 0,
            "{decaying} leaves would decay on the first tick"
        );
        assert_eq!(
            world.blocks.get(&(0, -1, 0)).map(String::as_str),
            Some("minecraft:dirt"),
            "and dirt under the trunk"
        );
    }

    /// A config naming a placer the port has not is refused, not
    /// approximated.
    #[test]
    fn an_unported_placer_is_refused() {
        let config = TreeConfig::parse(
            &serde_json::from_str(
                r#"{"below_trunk_provider":{"type":"minecraft:simple_state_provider",
                                            "state":{"Name":"minecraft:dirt"}},
                    "decorators":[],
                    "foliage_placer":{"type":"minecraft:blob_foliage_placer",
                                      "height":3,"offset":0,"radius":2},
                    "foliage_provider":{"type":"minecraft:simple_state_provider",
                                        "state":{"Name":"minecraft:oak_leaves"}},
                    "minimum_size":{"type":"minecraft:two_layers_feature_size"},
                    "trunk_placer":{"type":"minecraft:cherry_trunk_placer",
                                    "base_height":4,"height_rand_a":2,"height_rand_b":0},
                    "trunk_provider":{"type":"minecraft:simple_state_provider",
                                      "state":{"Name":"minecraft:oak_log"}}}"#,
            )
            .expect("json"),
            &states(),
            &decorator::no_registry,
        )
        .expect("parse");
        assert!(!config.is_supported());
        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        let error = place(
            &mut TreeOnly(&mut world),
            &tags(),
            &config,
            &mut random,
            (0, 0, 0),
        )
        .expect_err("refuses");
        assert!(error.0.contains("cherry"), "{}", error.0);
    }

    /// The property the twenty-two delegating impls exist for.
    ///
    /// A nested feature writes through its OWN trait's `set_block`, not
    /// `TreeWorld`'s. If any of those twenty delegated straight to `inner` the
    /// write would land, the tree would look right, and the undo log would not
    /// know about it, so a later refusal would put back everything except the
    /// nested feature's blocks. Nothing else in the suite would notice.
    // `Field` also answers `SimpleBlockWorld`, and the only method that
    // matters here is the level random: it is the one defaulted method on a
    // delegated trait whose default is a *refusal* rather than a value.
    impl crate::feature::simple_block::SimpleBlockWorld for Field {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            <Self as TreeWorld>::set_block(self, x, y, z, name);
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
        fn level_random_next_boolean(&mut self) -> Option<bool> {
            Some(true)
        }
    }

    /// A defaulted method on a delegated trait must be **forwarded**, and the
    /// one that answers `Option` is the one where getting it wrong is silent.
    ///
    /// §85.2 already states the rule — defaulted methods are delegated rather
    /// than inherited — and this is the case that shows why it is not a style
    /// preference. `SimpleBlockWorld::level_random_next_boolean` defaults to
    /// `None`, which every caller reads as a refusal, so an inherited default
    /// makes a feature refuse **inside** a tree and place outside one. It cost
    /// 85 of window A's 101 pale moss carpet declines, every one of them
    /// inside a `PaleMossDecorator`, and the world generated fine and looked
    /// right.
    #[test]
    fn the_wrapper_forwards_the_level_random_rather_than_inheriting_its_default() {
        use crate::feature::simple_block::SimpleBlockWorld;

        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        assert_eq!(
            SimpleBlockWorld::level_random_next_boolean(&mut world),
            Some(true),
            "the inner world has a stream"
        );

        let mut undo = UndoWorld::new(&mut world);
        assert_eq!(
            <UndoWorld<'_, Field> as SimpleBlockWorld>::level_random_next_boolean(&mut undo),
            Some(true),
            "the wrapper inherited the `None` default instead of forwarding, so a \
             nested feature refuses inside a tree and places outside one"
        );
    }

    #[test]
    fn a_rollback_undoes_a_nested_features_writes_too() {
        use crate::feature::vegetation_patch::PatchWorld;

        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        <Field as TreeWorld>::set_block(&mut world, 0, 0, 0, "minecraft:stone");
        <Field as TreeWorld>::set_block(&mut world, 0, 1, 0, "minecraft:dirt");

        let mut undo = UndoWorld::new(&mut world);
        // The tree's own write, through `TreeWorld`.
        <UndoWorld<'_, Field> as TreeWorld>::set_block(
            &mut undo,
            0,
            1,
            0,
            "minecraft:pale_oak_log",
        );
        // The nested feature's write, through `PatchWorld`. Same wrapper, a
        // different trait, and the same log has to catch it.
        PatchWorld::set_block(&mut undo, 0, 0, 0, "minecraft:pale_moss_block");
        PatchWorld::set_block(&mut undo, 0, 2, 0, "minecraft:pale_moss_carpet");

        assert_eq!(
            <UndoWorld<'_, Field> as PredicateWorld>::block_at(&mut undo, 0, 0, 0),
            "minecraft:pale_moss_block",
            "the nested write must land immediately, not be buffered"
        );

        undo.rollback();

        assert_eq!(world.block_at(0, 0, 0), "minecraft:stone");
        assert_eq!(world.block_at(0, 1, 0), "minecraft:dirt");
        assert_eq!(
            world.block_at(0, 2, 0),
            "minecraft:air",
            "a position the nested feature created must go back to air"
        );
    }

    #[test]
    fn every_tree_config_in_the_datapack_parses() {
        let Some(extract) = crate::feature::state_spec::extract_or_skip() else {
            return;
        };
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        // A real registry, because `pale_moss` resolves `pale_moss_patch`
        // through it at parse time. `no_registry` would make every pale oak a
        // parse ERROR rather than an unsupported config, which is a different
        // thing and would hide exactly what this test is for.
        let placed_dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/placed_feature"
        ));
        let read = |base: &std::path::Path, name: &str| -> Option<serde_json::Value> {
            let file = name.strip_prefix("minecraft:")?;
            serde_json::from_str(&std::fs::read_to_string(base.join(format!("{file}.json"))).ok()?)
                .ok()
        };
        let resolve_placed = |name: &str| read(placed_dir, name);
        let resolve_configured = |name: &str| read(dir, name);
        let resolve_feature =
            |name: &str| -> Result<crate::feature::configured::ConfiguredFeature, String> {
                let json = resolve_configured(name)
                    .ok_or_else(|| format!("{name} is not in the extract"))?;
                crate::feature::configured::ConfiguredFeature::parse(
                    &json,
                    &resolve_placed,
                    &resolve_configured,
                    &extract,
                )
            };

        let mut total = 0usize;
        let mut supported = 0usize;
        let mut pale_oaks = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:tree" {
                continue;
            }
            total += 1;
            let config = TreeConfig::parse(&json["config"], &extract, &resolve_feature)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            if config.is_supported() {
                supported += 1;
            }
            // The two entries this lane is for. Both list `pale_moss` and
            // nothing else unported, so both stand or fall with it.
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(stem, "pale_oak" | "pale_oak_creaking") {
                pale_oaks += 1;
                assert!(
                    config.is_supported(),
                    "{stem} is still refused: {:?}",
                    config
                        .decorators
                        .iter()
                        .map(decorator::TreeDecorator::name)
                        .collect::<Vec<_>>()
                );
            }
        }
        assert_eq!(total, 39, "the datapack has thirty-nine tree features");
        assert_eq!(pale_oaks, 2, "both pale oaks are tree features");
        // Ratcheted: straight trunk, blob foliage, and the beehive and
        // place-on-ground decorators. It should only go up. 30 -> 32 when
        // `pale_moss` landed and took both pale oaks with it.
        assert!(
            supported >= 32,
            "only {supported}/{total} tree configs are supported"
        );
        println!("TREE CONFIGS: {supported}/{total} supported");
    }

    /// A spruce config with a **constant** `offset`, so the row geometry can
    /// be pinned without the offset draw moving underneath the assertion.
    fn spruce(offset: i32) -> TreeConfig {
        TreeConfig::parse(
            &serde_json::from_str(&format!(
                r#"{{"below_trunk_provider":{{"type":"minecraft:simple_state_provider",
                                             "state":{{"Name":"minecraft:dirt"}}}},
                    "decorators":[],
                    "foliage_placer":{{"type":"minecraft:spruce_foliage_placer",
                                       "offset":{offset},"radius":2,"trunk_height":1}},
                    "foliage_provider":{{"type":"minecraft:simple_state_provider",
                                         "state":{{"Name":"minecraft:oak_leaves"}}}},
                    "ignore_vines":true,
                    "minimum_size":{{"type":"minecraft:two_layers_feature_size"}},
                    "trunk_placer":{{"type":"minecraft:straight_trunk_placer",
                                     "base_height":5,"height_rand_a":2,"height_rand_b":1}},
                    "trunk_provider":{{"type":"minecraft:simple_state_provider",
                                       "state":{{"Name":"minecraft:oak_log"}}}}}}"#
            ))
            .expect("json"),
            &states(),
            &decorator::no_registry,
        )
        .expect("parse")
    }

    /// The lowest row a placer wrote, relative to the foliage attachment.
    fn lowest_row(config: &TreeConfig, foliage_height: i32, seed: i64) -> i32 {
        let mut world = Field {
            blocks: BTreeMap::new(),
        };
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
        let mut placed = Placed::default();
        config
            .foliage_placer
            .create_foliage(
                &mut world,
                &tags(),
                config,
                &mut random,
                &FoliageAttachment {
                    pos: (0, 64, 0),
                    radius_offset: 0,
                    double_trunk: false,
                },
                foliage_height,
                2,
                &mut placed,
            )
            .expect("no rules needed");
        world
            .blocks
            .keys()
            .map(|&(_, y, _)| y - 64)
            .min()
            .expect("the placer wrote something")
    }

    /// `SpruceFoliagePlacer#createFoliage` stops at `-foliageHeight`. Every
    /// other placer stops at `offset - foliageHeight`.
    ///
    /// This is the gate that did not exist, and its absence cost 1,293 blocks
    /// on the seed-12345 r=16 overworld window. The port had copied the blob
    /// form, so with `spruce.json`'s `offset` of `uniform 0..2` two thirds of
    /// every spruce tree in the world was one or two canopy rows short. In the
    /// 26.2 bytecode the whole difference is one instruction:
    ///
    /// ```text
    ///   BlobFoliagePlacer     6: iload 9 (offset)   8: iload 7   10: isub
    ///   SpruceFoliagePlacer                        28: iload 7   30: ineg
    /// ```
    ///
    /// The old tests pinned the draws and the corner rule and passed
    /// throughout, because neither of them looks at how many rows there are.
    #[test]
    fn a_spruce_canopy_reaches_below_its_attachment_by_the_offset() {
        // The bound does not mention `offset`, so the bottom row is at
        // `-foliage_height` no matter what the offset is. Under the blob
        // form these would read -4, -3 and -2.
        for offset in 0..=2 {
            assert_eq!(
                lowest_row(&spruce(offset), 4, 12345),
                -4,
                "spruce offset {offset}: the bottom row is -foliageHeight, \
                 not offset - foliageHeight"
            );
        }

        // A taller canopy moves the bottom with `foliage_height` and still
        // ignores the offset — so the test cannot pass by the bound having
        // been hard-wired to a constant.
        assert_eq!(lowest_row(&spruce(2), 6, 12345), -6);

        // And the contrast that makes the spruce form meaningful: a blob
        // DOES carry its offset into the bound, so with offset 0 its bottom
        // row is -3 and it is a different function from the one above.
        assert_eq!(lowest_row(&oak(), 3, 12345), -3);
    }
}
