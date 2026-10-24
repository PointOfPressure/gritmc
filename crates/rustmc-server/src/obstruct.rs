//! `Level.isUnobstructed(state, pos, context)` — the entity half of
//! `BlockItem.canPlace`.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `BlockItem.canPlace` offsets 27-43, `CollisionGetter.isUnobstructed(
//! BlockState, BlockPos, CollisionContext)`, `CommonLevelAccessor
//! .isUnobstructed(Entity, VoxelShape)` (which is `EntityGetter`'s),
//! `EntityGetter.getEntities(Entity, AABB)` (`EntitySelector.NO_SPECTATORS`),
//! `Shapes.joinIsNotEmpty`, and the constructors that set
//! `Entity.blocksBuilding`.
//!
//! ```text
//!   CollisionGetter.isUnobstructed(state, pos, ctx):
//!     shape = state.getCollisionShape(this, pos, ctx)
//!     return shape.isEmpty() || isUnobstructed(null, shape.move(pos))
//!   EntityGetter.isUnobstructed(except = null, shape):
//!     for (e : getEntities(null, shape.bounds()))      // no spectators
//!       if (!e.isRemoved() && e.blocksBuilding
//!           && joinIsNotEmpty(shape, Shapes.create(e.getBoundingBox()), AND))
//!         return false
//!     return true
//! ```
//!
//! # Who blocks building
//!
//! `Entity.blocksBuilding` defaults to `false`. `LivingEntity`'s constructor
//! sets it `true` (offset 166) and so does `FallingBlockEntity`'s; of the
//! kinds this server has, that is the player, the pig, the bat and the
//! falling block — **not the item**, which is why a block can be placed on a
//! dropped stack and the stack then pushed out by `moveTowardsClosestSpace`.
//! (`ArmorStand` sets it back to `false`; there is none here.)
//!
//! # The shape is the placed state's, so a torch can go in your own feet
//!
//! The test runs on the state `getStateForPlacement` chose, after
//! `canSurvive`, with that state's collision shape. A torch, a flower or a
//! rail has an empty one and is never obstructed — so a torch clicked onto
//! the top of the block you stand on **is** placed, which the floor-column
//! test this replaced refused. A full block is refused wherever any part of
//! your box overlaps it, including a neighbouring column when you stand on a
//! block edge — which the floor-column test placed.
//!
//! # Not modelled, named
//!
//! * `CollisionContext.placementContext(player)` — the context only changes
//!   the shape of the blocks whose shape is entity-dependent (scaffolding,
//!   powder snow), and those 23 dynamic-shape blocks have no row in
//!   `collision_shapes.json` at all; a state with no row is **not refused**
//!   here, the same allow-what-cannot-be-adjudicated rule
//!   `place_survive_gate` applies;
//! * another player's pose. This server keeps a player's shift key on that
//!   player's own connection, so everyone but the placer is boxed
//!   `STANDING`;
//! * spectators — this server has none.

use crate::collide::{Aabb, Shape};
use crate::entities::{Entity, BAT, FALLING_BLOCK, PIG, PLAYER};

/// `FallingBlockEntity`'s `sized(0.98f, 0.98f)`.
const FALLING_BLOCK_DIMS: (f32, f32) = (0.98, 0.98);

/// The bounding box of an entity whose `blocksBuilding` is `true`, or `None`
/// for one that never obstructs a placement.
///
/// `crouching` is the pose of `self_id`, the placer, whose shift key is the
/// only one this caller can see.
#[must_use]
pub fn blocking_box(e: &Entity, self_id: i32, crouching: bool) -> Option<Aabb> {
    if e.kind == PLAYER {
        return Some(crate::pickup::player_box(e, e.id == self_id && crouching));
    }
    if e.kind == FALLING_BLOCK {
        return Some(crate::pickup::make_bounding_box(
            FALLING_BLOCK_DIMS,
            e.x,
            e.y,
            e.z,
        ));
    }
    if e.kind == PIG || e.kind == BAT {
        let dims = crate::mob::Mobs::shared().with(e.id, |m| m.ai.mob.dimensions())?;
        return Some(crate::pickup::make_bounding_box(dims, e.x, e.y, e.z));
    }
    None
}

/// `EntityGetter.isUnobstructed(null, shape.move(pos))` for a non-empty
/// shape: no blocking entity's box overlaps any cell of the shape.
///
/// `Shape::overlaps` is `joinIsNotEmpty(Shapes.create(box), shape, AND)`
/// with `IndirectMerger`'s 1e-7 merge, so an overlap thinner than that does
/// not count — a player standing exactly on a block's top face does not
/// obstruct placing that block.
#[must_use]
pub fn is_unobstructed(
    shape: &Shape,
    pos: (i32, i32, i32),
    entities: &[Entity],
    box_of: impl Fn(&Entity) -> Option<Aabb>,
) -> bool {
    if shape.is_empty() {
        return true;
    }
    let off = [f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)];
    !entities
        .iter()
        .filter_map(&box_of)
        .any(|b| shape.overlaps(b, off))
}

/// [`is_unobstructed`] for one box already in hand.
#[must_use]
pub fn is_unobstructed_by_box(shape: &Shape, pos: (i32, i32, i32), b: &Aabb) -> bool {
    shape.is_empty() || !shape.overlaps(*b, [f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::CollisionShapes;

    fn shapes() -> Option<&'static CollisionShapes> {
        CollisionShapes::shared()
    }

    fn player_at(id: i32, x: f64, y: f64, z: f64) -> Entity {
        Entity {
            id,
            uuid: 1,
            kind: PLAYER,
            x,
            y,
            z,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: true,
            data: 0,
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            synched: crate::synched::Synched::EntityOnly,
        }
    }

    fn full_block() -> &'static Shape {
        let s = shapes().expect("collision_shapes.json");
        // `minecraft:stone`'s default state has the full-block shape; the
        // table is indexed by global state id and stone is id 1.
        s.get(1).expect("stone's shape")
    }

    /// The `mining` lane's finding, as a unit: a player standing on the block
    /// corner (x = z = 0) overlaps the column at x = -1 by 0.3, so a block
    /// there is refused — the floor-column test placed it.
    #[test]
    fn a_player_on_a_block_corner_obstructs_the_neighbouring_column() {
        let me = player_at(7, 0.0, 0.0, 0.0);
        let all = [me.clone()];
        let shape = full_block();
        assert!(!is_unobstructed(shape, (-1, 0, 0), &all, |e| blocking_box(
            e, 7, false
        )));
        assert!(!is_unobstructed(shape, (0, 1, 0), &all, |e| blocking_box(
            e, 7, false
        )));
        // Two columns out is clear.
        assert!(is_unobstructed(shape, (-2, 0, 0), &all, |e| blocking_box(
            e, 7, false
        )));
    }

    /// Standing exactly on a block's top face touches it with zero
    /// thickness, and `joinIsNotEmpty` needs a cell wider than 1e-7.
    #[test]
    fn standing_on_a_block_does_not_obstruct_it() {
        let me = player_at(7, 0.5, 1.0, 0.5);
        let all = [me];
        assert!(is_unobstructed(full_block(), (0, 0, 0), &all, |e| {
            blocking_box(e, 7, false)
        }));
    }

    /// Height: 1.8 standing reaches into the block at y + 1, and a crouching
    /// placer at y = 0.4 (box to 1.9) still does; at y = 0.5 crouched the box
    /// tops out at 2.0 exactly and the block at y = 2 is clear, while the
    /// standing box (to 2.3) is not.
    #[test]
    fn the_crouching_box_is_one_and_a_half_tall() {
        let me = player_at(7, 0.5, 0.5, 0.5);
        let all = [me];
        assert!(!is_unobstructed(full_block(), (0, 2, 0), &all, |e| {
            blocking_box(e, 7, false)
        }));
        assert!(is_unobstructed(full_block(), (0, 2, 0), &all, |e| {
            blocking_box(e, 7, true)
        }));
    }

    /// An item does not block building; an empty shape is never obstructed.
    #[test]
    fn items_never_obstruct_and_empty_shapes_never_are() {
        let mut item = player_at(9, 0.5, 0.0, 0.5);
        item.kind = crate::entities::ITEM;
        let all = [item, player_at(7, 0.5, 0.0, 0.5)];
        assert!(blocking_box(&all[0], 7, false).is_none());
        let air = shapes().expect("table").get(0).expect("air's shape");
        assert!(air.is_empty());
        assert!(is_unobstructed(air, (0, 0, 0), &all, |e| blocking_box(
            e, 7, false
        )));
    }
}
