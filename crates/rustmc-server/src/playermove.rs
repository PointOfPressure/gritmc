//! The player's own `Entity.move`, wired to [`crate::move_player`],
//! [`crate::collide`] and [`crate::fall`].
//!
//! Reference (26.2): `ServerGamePacketListenerImpl.handleMovePlayer` offsets
//! 581-1331, `Entity.move`, `EntityTypes.PLAYER`'s builder
//! (`createNothing(MISC).noSave().noSummon().sized(0.6f, 1.8f)
//! .eyeHeight(1.62f)`, `EntityTypes.<clinit>` offsets 7303-7350).
//!
//! # What this adds that the tree did not have
//!
//! [`crate::collide`]'s header says "Nothing calls this yet", and
//! [`crate::physics::Profile::player`] says the same in more words. This is
//! the caller. Before it, the server wrote the client's position onto the
//! player entity unconditionally: a client could claim any position inside
//! the 3e7 clamp and the server agreed.
//!
//! # The player's box does NOT change with the pose, and that is measured
//!
//! `LivingEntity.getDimensions(Pose)` is
//!
//! ```text
//!    0: pose == Pose.SLEEPING ? SLEEPING_DIMENSIONS
//!   13: : getDefaultDimensions(pose).scale(getScale())
//! ```
//!
//! and `LivingEntity.getDefaultDimensions(Pose)` **discards its argument**:
//! `getType().getDimensions().scale(getAgeScale())`. `EntityDimensions` is a
//! record of `width, height, eyeHeight, attachments, fixed` with no pose
//! component, and `EntityTypes.PLAYER` is registered `sized(0.6f, 1.8f)`.
//!
//! So on the server a crouching player occupies the same 0.6 x 1.8 box as a
//! standing one, `canPlayerFitWithinBlocksAndEntitiesWhen(CROUCHING)` tests
//! the same box as `STANDING`, and **there is no server-side "sneak under a
//! slab"**: the shorter hitbox is the client's. A port that carries a
//! pose-to-dimensions table over from an older version gives a sneaking
//! player a 1.5-block box the server never had, and every gap it then fits
//! through is a divergence.
//!
//! # The state-id bridge costs a string per block per move
//!
//! [`crate::collide::CollisionBlocks`] wants a global state id and
//! [`crate::edits::Blocks`] answers with a `String`, so [`WorldShapes`] goes
//! through `state_at` and `state_id`. `crate::collide`'s own header argues
//! against exactly this, and it is right: a swept player box touches up to
//! a few dozen positions and each costs an allocation. It is done anyway
//! because the alternative is a new method on a trait five other lanes
//! implement, and it is written down here so the fix has a name —
//! `Blocks::state_id_at` — rather than being rediscovered as a profile
//! spike.
//!
//! # A block with no shape stops the move rather than being guessed
//!
//! [`crate::collide::NoShapeFor`] is raised for the 23 `hasDynamicShape`
//! blocks. `CollisionBlocks::state_id_at` cannot return an error, so
//! [`WorldShapes`] records the first unknown *state string* and
//! [`PlayerMovement::on_move`] refuses the move when one was seen. The
//! refusal is [`Outcome::NoShape`], which a caller must not treat as
//! "accepted": that is the difference between a player standing on a
//! shulker box and a player falling through one.

use std::cell::Cell;

use crate::collide::{self, CollisionBlocks, CollisionShapes, Vec3};
use crate::edits::Blocks;
use crate::fall::{Fall, FallDamage, Landing};
use crate::move_player::{After, Before, CorrectReason, Gates, MovePacket, Now, Tracker};
use crate::movement_props::MovementProps;
use crate::physics::{self, Body, MoverType, Profile};
use rustmc_data::BlockPos;

/// A [`CollisionBlocks`] over the live world.
///
/// Not `Copy` and not shared between threads: it holds a
/// [`Cell`] of the first position whose state has no network id, which is
/// per-move state and must not outlive the move it belongs to.
pub struct WorldShapes<'a> {
    blocks: &'a dyn Blocks,
    /// The first position whose state string the block table could not turn
    /// into an id. `Cell` because [`CollisionBlocks::state_id_at`] takes
    /// `&self` — vanilla's `BlockGetter.getBlockState` is equally
    /// non-mutating and equally total.
    unknown: Cell<Option<BlockPos>>,
}

impl<'a> WorldShapes<'a> {
    #[must_use]
    pub fn new(blocks: &'a dyn Blocks) -> Self {
        Self {
            blocks,
            unknown: Cell::new(None),
        }
    }

    /// The first position whose state had no id, if any.
    #[must_use]
    pub fn unknown(&self) -> Option<BlockPos> {
        self.unknown.get()
    }
}

impl CollisionBlocks for WorldShapes<'_> {
    fn state_id_at(&self, pos: BlockPos) -> u32 {
        // `crate::interact::BlockPos` is a bare `(i32, i32, i32)` and
        // `rustmc_data::BlockPos` is a struct; the two are the same quantity
        // on either side of a crate boundary.
        let state = self.blocks.state_at((pos.x, pos.y, pos.z));
        match self.blocks.state_id(&state) {
            #[expect(
                clippy::cast_sign_loss,
                reason = "a global block-state id is non-negative by construction"
            )]
            Some(id) => id as u32,
            None => {
                // Recorded, not guessed. Returning air here would make an
                // unknown block frictionless and the player would walk
                // through it; the caller reads `unknown()` and refuses.
                if self.unknown.get().is_none() {
                    self.unknown.set(Some(pos));
                }
                0
            }
        }
    }
}

/// What one move packet did.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// `disconnect("multiplayer.disconnect.invalid_player_movement")`.
    Disconnect,
    /// One of `handleMovePlayer`'s early returns. Nothing changed.
    Ignored,
    /// Rotation was taken and position was not.
    RotationOnly { yaw: f32, pitch: f32 },
    /// A correction teleport is owed to the client, carrying `id`.
    Corrected {
        to: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        id: i32,
        reason: CorrectReason,
        /// The landing, if the rubber-band's own `doCheckFallDamage` at
        /// offsets 926-966 produced one.
        landing: Landing,
    },
    /// The move was taken.
    Moved {
        to: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        /// `new Vec3(getX() - d7, getY() - d8, getZ() - d9)` — what
        /// `setOnGroundWithMovement`, `doCheckFallDamage`,
        /// `handlePlayerKnownMovement` and `checkMovementStatistics` all
        /// receive.
        movement: (f64, f64, f64),
        landing: Landing,
        /// Whether `player.jumpFromGround()` fired.
        jumped: bool,
    },
    /// A block on the swept path has no collision shape in the table, so the
    /// move was not run. **Not an acceptance.**
    NoShape { at: BlockPos },
}

/// Everything the server holds about one player's movement.
pub struct PlayerMovement {
    tracker: Tracker,
    fall: Fall,
    body: Body,
    profile: Profile,
    damage: FallDamage,
    /// `Entity.getYRot()`. Written only by an accepted or snapped packet,
    /// which is what makes it the default the next packet's getters read.
    yaw: f32,
    /// `Entity.getXRot()`.
    pitch: f32,
}

/// The `9.999999747378752E-6` both `isEntityCollidingWithAnythingNew`
/// deflates by.
///
/// It is `1.0E-5f` widened — the same number `Mth.equal` uses — and it is
/// written here as the widening rather than as a decimal so that a reader
/// cannot mistake it for `1.0E-5`. `crate::collide` holds the same constant
/// NEGATED, for the direction its own caller needs.
const DEFLATE_EPSILON: f64 = 1.0E-5_f32 as f64;

/// `AABB.deflate(double)` — shrink by `d` on every face.
fn deflate(b: collide::Aabb, d: f64) -> collide::Aabb {
    collide::Aabb::new(
        b.min_x + d,
        b.min_y + d,
        b.min_z + d,
        b.max_x - d,
        b.max_y - d,
        b.max_z - d,
    )
}

/// `Level.noCollision(Entity, AABB)` restricted to blocks.
///
/// The entity list is empty in this server's player path — nothing else is
/// collidable — and the world border is 60,000,000 wide with every position
/// far inside it, exactly as `EntityMoveGolden`'s `borderShapes` records for
/// the collision golden. Both are stated rather than assumed: a server that
/// grows either has to widen this.
fn no_collision<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    box_: collide::Aabb,
) -> bool {
    collide::collect_block_collisions(shapes, world, box_).is_ok_and(|c| c.is_empty())
}

/// `ServerGamePacketListenerImpl.isEntityCollidingWithAnythingNew`.
///
/// ```text
///    0-28  AABB newBox = box.move(target - position)
///   30-49  Iterable<VoxelShape> shapes = getPreMoveCollisions(e, newBox.deflate(1e-5), oldBox.getBottomCenter())
///   51-61  VoxelShape old = Shapes.create(oldBox.deflate(1e-5))
///   63-113 for (s : shapes) if (!joinIsNotEmpty(s, old, AND)) return true
///          return false
/// ```
///
/// So it is true when some shape overlapping the box the player is moving
/// INTO does not overlap the box they are moving OUT of — "something new".
/// A port that tests overlap with the new box has written the opposite
/// check and will rubber-band a player who was already standing in a block.
///
/// `getPreMoveCollisions` concatenates the entity collisions (empty here)
/// with `getBlockCollisionsFromContext(CollisionContext.withPosition(e,
/// bottomCenter.y), newBox)`. The context's y only reaches blocks whose
/// shape depends on the entity's position — the same `hasDynamicShape` set
/// [`crate::collide::NoShapeFor`] already refuses — so a plain block
/// collision sweep is the whole of it here, and the day a dynamic shape is
/// added this is the second place it has to be handled.
fn colliding_with_anything_new<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    old_box: collide::Aabb,
    new_box: collide::Aabb,
) -> bool {
    let old = deflate(old_box, DEFLATE_EPSILON);
    let Ok(found) =
        collide::collect_block_collisions(shapes, world, deflate(new_box, DEFLATE_EPSILON))
    else {
        // A shape the table cannot answer. `on_move` has already refused on
        // `WorldShapes::unknown`, and a `NoShapeFor` here would be the same
        // refusal reached a second way; reporting "nothing new" would be the
        // silent-acceptance shape this module exists to avoid, so it reports
        // the opposite and the caller rubber-bands.
        return true;
    };
    found.iter().any(|placed| {
        let off = [
            f64::from(placed.pos.x),
            f64::from(placed.pos.y),
            f64::from(placed.pos.z),
        ];
        !placed.shape.overlaps(old, off)
    })
}

impl PlayerMovement {
    /// A player just placed at `pos`.
    ///
    /// Returns `None` without `data/vanilla/attributes.json`: `step_height`,
    /// `gravity`, `safe_fall_distance` and `fall_damage_multiplier` are all
    /// table reads, and a default for any of them would be a number with
    /// nothing checking it. Same refusal as
    /// [`crate::physics::Profile::player`]'s.
    #[must_use]
    pub fn new(pos: (f64, f64, f64), attributes: &crate::attributes::Attributes) -> Option<Self> {
        Some(Self {
            tracker: Tracker::new(pos),
            fall: Fall::default(),
            body: Body::at(Vec3::new(pos.0, pos.1, pos.2)),
            profile: Profile::player(attributes)?,
            damage: FallDamage::for_type(attributes, "minecraft:player")?,
            yaw: 0.0,
            pitch: 0.0,
        })
    }

    /// Tell the tracker about the teleport `world_send::enter_world` sent at
    /// the join. See [`Tracker::adopt_teleport`] for why the id matters.
    /// `PlayerList.respawn` for this connection's movement state: a new
    /// `ServerPlayer` — a body at rest at `to`, no fall behind it, rotation
    /// zero — on the SAME `ServerGamePacketListenerImpl`, whose teleport
    /// counter carries on. Returns the id `connection.teleport` (offset 303)
    /// awaits.
    pub fn respawn(&mut self, to: (f64, f64, f64)) -> i32 {
        self.body = Body::at(Vec3::new(to.0, to.1, to.2));
        self.fall = Fall::default();
        self.yaw = 0.0;
        self.pitch = 0.0;
        self.tracker.begin_teleport(to)
    }

    pub const fn adopt_join_teleport(&mut self, id: i32, to: (f64, f64, f64)) {
        self.tracker.adopt_teleport(id, to);
    }

    /// `ServerGamePacketListenerImpl.tickPlayer`'s movement bookkeeping,
    /// anchored on where the last accepted packet left the player.
    pub const fn tick(&mut self) {
        self.tracker
            .tick((self.body.pos.x, self.body.pos.y, self.body.pos.z));
    }

    /// The caller accepted a move this tracker answered [`Outcome::NoShape`]
    /// for, by the path that predates it; put the body where that path put
    /// the player.
    ///
    /// **Without this the tracker and the player part company for good.**
    /// `NoShape` resets the body to where the move started, `world_send`'s
    /// fall-through then moves the player anyway, and every later packet is
    /// swept from the stale body — which still touches the same unknown
    /// block, so it is `NoShape` again — until the distance from that body
    /// passes the speed budget and `TooQuick` teleports the player back to
    /// it. Measured on train-b: a `minecraft:moving_piston` left by
    /// `380-a-piston-extends-when-powered` at the head's position produced
    /// 572 `NoShape`s and 28 `TooQuick` corrections to the workspace, and the
    /// corpus's next walk gave up 26 blocks short. [`Self::tick`] re-anchors
    /// `firstGood` on the body, so moving the body is the whole repair.
    pub fn adopt_unchecked_move(&mut self, to: (f64, f64, f64)) {
        self.body.pos = Vec3::new(to.0, to.1, to.2);
    }

    /// The tracker, for a caller that needs the teleport id or the anchors.
    #[must_use]
    pub const fn tracker(&self) -> &Tracker {
        &self.tracker
    }

    /// `Entity.fallDistance`.
    #[must_use]
    pub const fn fall_distance(&self) -> f64 {
        self.fall.distance()
    }

    /// `Entity.onGround()`.
    #[must_use]
    pub const fn on_ground(&self) -> bool {
        self.fall.on_ground()
    }

    /// `LivingEntity.causeFallDamage`'s damage, for a landing this module
    /// produced. Zero for a landing that did not reach `fallOn`.
    ///
    /// The multiplier is `Block.fallOn`'s default `1.0f`; see
    /// [`FallDamage::block_overrides_fall_on`] for the blocks that change it
    /// and are not modelled.
    #[must_use]
    pub fn fall_damage(&self, landing: Landing) -> i32 {
        if landing.hit_ground {
            self.damage.damage(landing.distance, 1.0)
        } else {
            0
        }
    }

    /// `handleAcceptTeleportPacket`.
    ///
    /// # Errors
    /// When vanilla disconnects: the id matches and no teleport is
    /// outstanding.
    #[expect(
        clippy::result_unit_err,
        reason = "the caller's response is `disconnect`, with vanilla's own key"
    )]
    pub fn accept_teleport(&mut self, id: i32) -> Result<(), ()> {
        if let Some(pos) = self.tracker.accept_teleport(id)? {
            self.body.pos = Vec3::new(pos.0, pos.1, pos.2);
        }
        Ok(())
    }

    /// One `ServerboundMovePlayerPacket`, start to finish.
    pub fn on_move(
        &mut self,
        pkt: MovePacket,
        gates: Gates,
        blocks: &dyn Blocks,
        shapes: &CollisionShapes,
        props: &MovementProps,
    ) -> Outcome {
        let now = Now {
            pos: (self.body.pos.x, self.body.pos.y, self.body.pos.z),
            yaw: self.yaw,
            pitch: self.pitch,
            on_ground: self.fall.on_ground(),
            delta_movement_len_sq: self.body.movement.length_sqr(),
            vertical_collision_below: self.body.vertical_collision_below,
        };
        let accepted = match self.tracker.before_move(pkt, gates, now) {
            Before::Disconnect => return Outcome::Disconnect,
            Before::Ignore => return Outcome::Ignored,
            Before::SnapRotation { yaw, pitch } => {
                self.yaw = yaw;
                self.pitch = pitch;
                return Outcome::RotationOnly { yaw, pitch };
            }
            Before::Correct { yaw, pitch, reason } => {
                let to = (self.body.pos.x, self.body.pos.y, self.body.pos.z);
                let id = self.tracker.begin_teleport(to);
                self.yaw = yaw;
                self.pitch = pitch;
                return Outcome::Corrected {
                    to,
                    yaw,
                    pitch,
                    id,
                    reason,
                    landing: NO_LANDING,
                };
            }
            Before::Accept(a) => a,
        };

        // Offsets 669-691. `MoverType.PLAYER`, and the profile's
        // `client_authoritative` is what stops `move` from restituting or
        // running its own fall check — the packet's flags do that instead.
        let world = WorldShapes::new(blocks);
        let before_pos = self.body.pos;
        let moved = physics::move_entity(
            shapes,
            props,
            &world,
            &self.profile,
            &mut self.body,
            MoverType::Player,
            Vec3::new(accepted.delta.0, accepted.delta.1, accepted.delta.2),
        );
        if let Some(at) = world.unknown() {
            // Put the player back: a half-run move through a block with no
            // shape is worse than no move.
            self.body.pos = before_pos;
            return Outcome::NoShape { at };
        }
        let Ok(_) = moved else {
            let at = moved.expect_err("checked Ok above").pos;
            self.body.pos = before_pos;
            return Outcome::NoShape { at };
        };
        let now_at = (self.body.pos.x, self.body.pos.y, self.body.pos.z);

        let after = {
            let world = WorldShapes::new(blocks);
            self.tracker.after_move(
                accepted,
                pkt,
                gates,
                now_at,
                || no_collision(shapes, &world, self.profile.bounding_box(before_pos)),
                || {
                    colliding_with_anything_new(
                        shapes,
                        &world,
                        self.profile.bounding_box(before_pos),
                        self.profile.bounding_box(Vec3::new(
                            accepted.target.0,
                            accepted.target.1,
                            accepted.target.2,
                        )),
                    )
                },
            )
        };

        match after {
            After::Correct {
                to,
                yaw,
                pitch,
                fall_delta,
            } => {
                self.body.pos = Vec3::new(to.0, to.1, to.2);
                self.yaw = yaw;
                self.pitch = pitch;
                // 926-966: the rubber-band still checks fall damage, with the
                // delta the SERVER achieved and the PACKET's ground flag.
                let landing = self.fall.check_fall_damage(
                    fall_delta.1,
                    pkt.on_ground,
                    self.landing_on_air(blocks, to),
                );
                let id = self.tracker.begin_teleport(to);
                Outcome::Corrected {
                    to,
                    yaw,
                    pitch,
                    id,
                    reason: CorrectReason::CollidingNew,
                    landing,
                }
            }
            After::Accept {
                to,
                yaw,
                pitch,
                movement,
                reset_fall_distance,
                on_ground,
                horizontal_collision,
                ..
            } => {
                // 975-988: the CLIENT's position wins.
                self.body.pos = Vec3::new(to.0, to.1, to.2);
                self.yaw = yaw;
                self.pitch = pitch;
                // 1146-1162.
                self.fall
                    .set_on_ground_with_movement(on_ground, horizontal_collision);
                self.body.on_ground = on_ground;
                self.body.horizontal_collision = horizontal_collision;
                // 1163-1188.
                let landing = self.fall.check_fall_damage(
                    movement.1,
                    on_ground,
                    self.landing_on_air(blocks, to),
                );
                // 1195-1206, AFTER the fall check.
                if reset_fall_distance {
                    self.fall.reset();
                }
                Outcome::Moved {
                    to,
                    yaw,
                    pitch,
                    movement,
                    landing,
                    jumped: accepted.jump_from_ground,
                }
            }
        }
    }

    /// `level.getBlockState(getOnPosLegacy()).isAir()` — the one thing
    /// `Entity.checkFallDamage` asks of the state it is handed.
    /// `Entity.position()` of the server's own copy of the player.
    #[must_use]
    pub const fn position(&self) -> (f64, f64, f64) {
        (self.body.pos.x, self.body.pos.y, self.body.pos.z)
    }

    /// `Entity.getDeltaMovement()` of the server's own copy of the player.
    #[must_use]
    pub const fn velocity(&self) -> (f64, f64, f64) {
        (
            self.body.movement.x,
            self.body.movement.y,
            self.body.movement.z,
        )
    }

    /// `Entity.onGround()` as the body holds it — what `LivingEntity.knockback`
    /// reads at offset 135.
    #[must_use]
    pub const fn body_on_ground(&self) -> bool {
        self.body.on_ground
    }

    /// `Entity.setDeltaMovement` from outside the tick — a knockback.
    pub const fn set_velocity(&mut self, v: (f64, f64, f64)) {
        self.body.movement = Vec3::new(v.0, v.1, v.2);
    }

    /// The server's own tick of a `ServerPlayer`'s movement:
    /// `LivingEntity.aiStep`'s velocity prelude and `travel`, which
    /// `ServerPlayer.doTick` reaches through `Player.tick` (offset 15).
    ///
    /// # Why the server simulates a client-authoritative entity at all
    ///
    /// `Player.canSimulateMovement` and `Player.isEffectiveAi` are both
    /// `!level.isClientSide() || isLocalPlayer()` — TRUE on the server — so
    /// `LivingEntity.aiStep` offsets 601..617 call `travel` for every
    /// `ServerPlayer`. What it simulates is a player with no input:
    /// `xxa`/`zza` are written by nothing on the server (`applyInput` only
    /// scales them by `0.98f`), and `jumping` likewise. The client's move
    /// packets then overwrite the position; the VELOCITY this leaves is what
    /// a knockback starts from and what `set_entity_motion` carries.
    ///
    /// ```text
    ///   LivingEntity.aiStep
    ///     93..199  v = getDeltaMovement(); a PLAYER zeroes x and z when
    ///              v.horizontalDistanceSqr() < 9.0E-6; every entity zeroes y
    ///              when |y| < 0.003
    ///    217       applyInput()                         (0 * 0.98f)
    ///    601..617  canSimulateMovement && isEffectiveAi -> travel(new Vec3(xxa, yya, zza))
    ///   Player.travel     not swimming, not flying      -> super.travel
    ///   LivingEntity.travel
    ///      0..23   shouldTravelInFluid(fluid at blockPosition) -> travelInFluid   (REFUSED)
    ///     26..38   isFallFlying                                -> travelFallFlying (no elytra)
    ///     41       travelInAir
    ///   LivingEntity.travelInAir
    ///      0..41   friction = onGround ? computeModifiedFriction(block.getFriction(),
    ///                                     FRICTION_MODIFIER) : 1.0f
    ///     45       v = handleRelativeFrictionAndCalculateMovement(input, friction)
    ///                  moveRelative(speed, ZERO)            -> nothing
    ///                  setDeltaMovement(handleOnClimbable(getDeltaMovement()))
    ///                  move(SELF, getDeltaMovement())
    ///                  (horizontalCollision || jumping) && onClimbable -> v.y = 0.2
    ///     57..161  y = v.y; LEVITATION ? y + (0.05*(amp+1) - y)*0.2 : y - getEffectiveGravity()
    ///    187..260  setDeltaMovement(v.x*f, y*computeModifiedFriction(0.98f, AIR_DRAG),
    ///                               v.z*f),  f = friction * computeModifiedFriction(0.91f, AIR_DRAG)
    /// ```
    ///
    /// # Errors
    /// [`TravelRefused`] names the arm this port does not have — the
    /// velocity is then left exactly as it was rather than guessed at.
    pub fn server_travel(
        &mut self,
        blocks: &dyn Blocks,
        tables: &Tables,
        facts: TravelFacts,
    ) -> Result<Option<TravelStep>, TravelRefused> {
        // aiStep 93..199, the PLAYER arm.
        let v = self.body.movement;
        let (mut x, mut y, mut z) = (v.x, v.y, v.z);
        if x * x + z * z < 9.0E-6 {
            x = 0.0;
            z = 0.0;
        }
        if y.abs() < 0.003 {
            y = 0.0;
        }
        self.body.movement = Vec3::new(x, y, z);

        let feet_state = blocks.state_at(crate::playerdeath::block_position(
            self.body.pos.x,
            self.body.pos.y,
            self.body.pos.z,
        ));
        let feet_block = feet_state.split('[').next().unwrap_or(&feet_state);
        if holds_fluid(&feet_state) {
            return Err(TravelRefused::InFluid);
        }
        let world = WorldShapes::new(blocks);
        let props = &tables.props;
        // 0..41 — the friction under the player.
        let friction = if self.body.on_ground {
            let below = physics::block_pos_below_that_affects_my_movement(
                &self.body,
                |p| props.is_fence(world.state_id_at(p)),
                |p| props.is_wall_or_gate(world.state_id_at(p)),
            );
            physics::compute_modified_friction(
                props.friction(world.state_id_at(below)),
                facts.friction_modifier,
            )
        } else {
            1.0_f32
        };
        // `handleOnClimbable`, then `move(SELF, ...)`.
        let climbable = !facts.flying && facts.climbable(feet_block)?;
        if climbable {
            // `resetFallDistance()`, handleOnClimbable offset 8.
            self.fall.reset();
            let m = self.body.movement;
            let cx =
                m.x.clamp(-0.150_000_005_960_464_48, 0.150_000_005_960_464_48);
            let cz =
                m.z.clamp(-0.150_000_005_960_464_48, 0.150_000_005_960_464_48);
            let mut cy = m.y.max(-0.150_000_005_960_464_48);
            if cy < 0.0 && feet_block != "minecraft:scaffolding" && facts.shift_key_down {
                cy = 0.0;
            }
            self.body.movement = Vec3::new(cx, cy, cz);
        }
        self.body.suppressing_bounce = facts.shift_key_down;
        let before = self.body.pos;
        let delta = self.body.movement;
        let moved = physics::move_entity(
            &tables.shapes,
            props,
            &world,
            &self.profile,
            &mut self.body,
            MoverType::SelfMove,
            delta,
        );
        if let Some(at) = world.unknown() {
            self.body.pos = before;
            return Err(TravelRefused::NoShape(at));
        }
        if let Err(e) = moved {
            self.body.pos = before;
            return Err(TravelRefused::NoShape(e.pos));
        }
        let mut r = self.body.movement;
        if self.body.horizontal_collision && climbable {
            r = Vec3::new(r.x, 0.2, r.z);
        }
        // 57..161.
        let y = match facts.levitation {
            Some(amp) => r.y + (0.05 * f64::from(amp + 1) - r.y) * 0.2,
            None => {
                let g = if r.y <= 0.0 && facts.slow_falling {
                    self.profile.gravity.min(0.01)
                } else {
                    self.profile.gravity
                };
                r.y - g
            }
        };
        // 187..260.
        let air = physics::compute_modified_friction(0.91, facts.air_drag_modifier);
        let f = friction * air;
        let drag = physics::compute_modified_friction(0.98, facts.air_drag_modifier);
        self.body.movement = Vec3::new(r.x * f64::from(f), y * f64::from(drag), r.z * f64::from(f));
        // `ServerGamePacketListenerImpl.tickPlayer` offsets 0..83:
        // `resetPosition()` captures `firstGood*` from the player BEFORE
        // `doTick`, and after it `absSnapTo(firstGoodX, firstGoodY,
        // firstGoodZ, yRot, xRot)` puts the player back. So the server's
        // travel moves the VELOCITY and not the position — the client owns
        // where it is. Refuted the first cut of this port on 2026-09-30: a
        // body that kept its travelled position had fallen most of the way
        // before `006`'s landing packet arrived, and the fall charged nothing.
        let step = (self.body.pos != before).then_some(TravelStep {
            from: (before.x, before.y, before.z),
            to: (self.body.pos.x, self.body.pos.y, self.body.pos.z),
            on_ground: self.body.on_ground,
        });
        self.body.pos = before;
        Ok(step)
    }

    fn landing_on_air(&self, blocks: &dyn Blocks, at: (f64, f64, f64)) -> bool {
        let pos = crate::step::on_pos(at.0, at.1, at.2, crate::step::LEGACY_OFFSET);
        let state = blocks.state_at(pos);
        matches!(
            state.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        )
    }
}

/// The three tables the player path cannot run without, loaded once.
///
/// A `OnceLock` beside [`crate::movement_props::MovementProps::shared`] and
/// [`crate::attributes::Attributes::shared`] rather than a fourth field on
/// `Session`: they are process-wide read-only extracts, and the session
/// struct already carries fourteen of these borrows.
///
/// **`None` means the server keeps its old behaviour**, which is to write
/// the client's position onto the player and believe it. That is the
/// opt-in-until-measured rule: a checkout with no `data/vanilla` gets
/// exactly what it got before this module existed, and says so once rather
/// than moving players by a rule nobody generated the tables for.
/// What [`PlayerMovement::server_travel`] reads that the body does not hold.
pub struct TravelFacts<'a> {
    /// `Player.isShiftKeyDown()` — `isSuppressingSlidingDownLadder` and
    /// `isSuppressingBounce` for a player.
    pub shift_key_down: bool,
    /// `Abilities.flying` — `Player.onClimbable` is false while flying, and
    /// `Player.travel` takes its own arm. This server decodes no
    /// `player_abilities` packet, so a flight the client starts is not seen;
    /// the flag is what the server holds.
    pub flying: bool,
    /// `getEffect(LEVITATION).getAmplifier()`, or `None`.
    pub levitation: Option<i32>,
    /// `hasEffect(SLOW_FALLING)`.
    pub slow_falling: bool,
    /// `getAttributeValue(FRICTION_MODIFIER)`, narrowed.
    pub friction_modifier: f32,
    /// `getAttributeValue(AIR_DRAG_MODIFIER)`, narrowed.
    pub air_drag_modifier: f32,
    /// `BlockTags.CLIMBABLE`, resolved — `None` without the tag table.
    pub climbable_tag: Option<&'a rustmc_worldgen::tags::BlockTags>,
}

impl TravelFacts<'_> {
    /// `LivingEntity.onClimbable`'s block half: the feet block in
    /// `minecraft:climbable`. The open-trapdoor-over-a-ladder arm
    /// (`trapdoorUsableAsLadder`) is refused rather than answered.
    fn climbable(&self, block: &str) -> Result<bool, TravelRefused> {
        if block.ends_with("_trapdoor") {
            return Err(TravelRefused::Trapdoor);
        }
        let tags = self.climbable_tag.ok_or(TravelRefused::NoTagTable)?;
        Ok(tags.contains("minecraft:climbable", block))
    }
}

/// Where one tick of the server's travel moved the body before `absSnapTo`
/// put it back — what `Entity.move`'s movement emission (the step sound)
/// sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TravelStep {
    pub from: (f64, f64, f64),
    pub to: (f64, f64, f64),
    pub on_ground: bool,
}

/// The arms of the server's player travel this port does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TravelRefused {
    /// `travelInFluid` — the feet block holds a fluid.
    InFluid,
    /// `trapdoorUsableAsLadder`.
    Trapdoor,
    /// No `tags_block.json`, so `minecraft:climbable` cannot be read.
    NoTagTable,
    /// A position with a dynamic collision shape the table cannot answer.
    NoShape(BlockPos),
}

/// Whether a block state carries a fluid: a fluid block, a waterlogged
/// state, or one of the blocks whose fluid state is water by construction.
/// Used only to REFUSE `travelInFluid`, never to simulate it.
fn holds_fluid(state: &str) -> bool {
    let block = state.split('[').next().unwrap_or(state);
    matches!(
        block,
        "minecraft:water"
            | "minecraft:lava"
            | "minecraft:bubble_column"
            | "minecraft:kelp"
            | "minecraft:kelp_plant"
            | "minecraft:seagrass"
            | "minecraft:tall_seagrass"
    ) || state.contains("waterlogged=true")
}

pub struct Tables {
    pub attributes: crate::attributes::Attributes,
    pub shapes: CollisionShapes,
    pub props: MovementProps,
}

/// The loaded tables, or `None` when any of the three is missing.
#[must_use]
pub fn shared() -> Option<&'static Tables> {
    use std::sync::OnceLock;
    static TABLES: OnceLock<Option<Tables>> = OnceLock::new();
    TABLES
        .get_or_init(|| {
            let attributes = crate::attributes::Attributes::load_default().ok()?;
            let shapes = CollisionShapes::load(&table_path("collision_shapes.json")).ok()?;
            let props = MovementProps::load(&table_path(MOVEMENT_PROPS_FILE)).ok()?;
            Some(Tables {
                attributes,
                shapes,
                props,
            })
        })
        .as_ref()
}

/// `data/vanilla/<name>` relative to the process cwd, falling back to the
/// crate directory — the same two-step every other loader here uses, and the
/// reason `cargo test` (whose cwd is the package) finds the extract.
fn table_path(name: &str) -> std::path::PathBuf {
    let cwd = std::path::Path::new("data/vanilla").join(name);
    if cwd.exists() {
        return cwd;
    }
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla")
        .join(name)
}

/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping `crates/rustmc-server/src` for
/// `"data/vanilla/<name>.json"`, so these two literals have to appear
/// somewhere a grep finds them even though [`table_path`] joins the name on.
const _DERIVED_TABLES: [&str; 2] = [
    "data/vanilla/collision_shapes.json",
    "data/vanilla/movement_props.json",
];
const MOVEMENT_PROPS_FILE: &str = "movement_props.json";

/// The `Landing` a path that never reached `checkFallDamage` produces.
const NO_LANDING: Landing = Landing {
    distance: 0.0,
    landed: false,
    hit_ground: false,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attributes::Attributes;
    use crate::interact::BlockPos as Pos;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A world of named blocks whose ids come from
    /// `data/vanilla/block_props.json`.
    ///
    /// **The ids are read, not invented.** Every other `Blocks` double in
    /// this crate returns a constant from `state_id`, which is harmless for
    /// a test about redstone and fatal for one about collision: the id is
    /// the key into `collision_shapes.json`, so a made-up id silently tests
    /// the shape of whichever block happens to hold it.
    struct World {
        blocks: Mutex<HashMap<Pos, String>>,
        ids: HashMap<String, i32>,
    }

    impl World {
        fn new(blocks: &[(Pos, &str)]) -> Self {
            let doc: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(repo("data/vanilla/block_props.json"))
                    .expect("data/vanilla/block_props.json — run ci/gen-gameplay-tables.sh"),
            )
            .expect("block_props.json parses");
            let mut ids = HashMap::new();
            for row in doc["table"].as_array().expect("table") {
                if let (Some(state), Some(id)) = (row["state"].as_str(), row["id"].as_i64()) {
                    #[expect(clippy::cast_possible_truncation, reason = "a state id fits i32")]
                    ids.insert(state.to_string(), id as i32);
                }
            }
            Self {
                blocks: Mutex::new(blocks.iter().map(|(p, s)| (*p, (*s).to_string())).collect()),
                ids,
            }
        }
    }

    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    impl Blocks for World {
        fn state_at(&self, pos: Pos) -> String {
            self.blocks
                .lock()
                .expect("world")
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn set_block(&self, _: Pos, _: &str) -> Option<i32> {
            None
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, state: &str) -> bool {
            self.ids.contains_key(state)
        }
        fn state_id(&self, state: &str) -> Option<i32> {
            self.ids.get(state).copied()
        }
        fn replaceable(&self, _: Pos, _: &crate::placement::Replacing<'_>) -> bool {
            true
        }
        fn face_sturdy(&self, pos: Pos, _: crate::interact::Face) -> bool {
            self.state_at(pos) != "minecraft:air"
        }
        fn redstone_conductor(&self, _: Pos) -> bool {
            false
        }
        fn signal_source(&self, _: Pos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _: Pos) -> String {
            "minecraft:air".to_string()
        }
        fn water_at(&self, _: Pos) -> bool {
            false
        }
    }

    struct Tables {
        attrs: Attributes,
        shapes: CollisionShapes,
        props: MovementProps,
    }

    fn tables() -> Tables {
        Tables {
            attrs: Attributes::load_default().expect("data/vanilla/attributes.json"),
            shapes: CollisionShapes::load(&repo("data/vanilla/collision_shapes.json"))
                .expect("data/vanilla/collision_shapes.json"),
            props: MovementProps::load(&repo("data/vanilla/movement_props.json"))
                .expect("data/vanilla/movement_props.json"),
        }
    }

    fn travel_tables(t: &Tables) -> super::Tables {
        super::Tables {
            attributes: t.attrs.clone(),
            shapes: t.shapes.clone(),
            props: t.props.clone(),
        }
    }

    fn still(tags: Option<&rustmc_worldgen::tags::BlockTags>) -> TravelFacts<'_> {
        TravelFacts {
            shift_key_down: false,
            flying: false,
            levitation: None,
            slow_falling: false,
            friction_modifier: 1.0,
            air_drag_modifier: 1.0,
            climbable_tag: tags,
        }
    }

    /// The first `/damage` recording: a standing vanilla player's velocity
    /// is `(0, -0.0784, 0)` — one tick of gravity and drag, restituted to 0
    /// by the floor and pulled down again every tick. A server with no
    /// player travel sends `(0, 0, 0)`.
    #[test]
    fn a_standing_player_settles_at_one_tick_of_gravity_and_does_not_move() {
        let t = tables();
        let tt = travel_tables(&t);
        let world = World::new(&floor());
        let mut mv = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("player profile");
        mv.body.on_ground = true;
        let tags = crate::fallable::tags();
        for _ in 0..5 {
            mv.server_travel(&world, &tt, still(tags))
                .expect("dry floor");
        }
        let v = mv.velocity();
        assert_eq!(v.0, 0.0);
        assert_eq!(v.2, 0.0);
        assert_eq!(v.1, -0.08 * f64::from(0.98_f32), "(0 - gravity) * 0.98f");
        assert_eq!(mv.position(), (0.5, 64.0, 0.5), "the floor holds it");
        assert!(mv.body_on_ground());
    }

    #[test]
    fn a_knocked_player_keeps_its_position_and_loses_speed_to_friction() {
        let t = tables();
        let tt = travel_tables(&t);
        let world = World::new(&floor());
        let mut mv = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("player profile");
        mv.body.on_ground = true;
        mv.set_velocity((0.0, 0.3608, -0.4));
        let step = mv
            .server_travel(&world, &tt, still(crate::fallable::tags()))
            .expect("dry floor")
            .expect("the body moved before the snap-back");
        assert!((step.to.2 - 0.1).abs() < 1e-9, "{step:?}");
        assert_eq!(mv.position(), (0.5, 64.0, 0.5), "tickPlayer's absSnapTo");
        // Stone's friction is 0.6; the ground flag was set when the tick
        // began, so f = 0.6f * 0.91f.
        let v = mv.velocity();
        let f = f64::from(0.6_f32 * 0.91_f32);
        assert!((v.2 - (-0.4 * f)).abs() < 1e-12, "{v:?}");
        assert!(
            (v.1 - (0.3608 - 0.08) * f64::from(0.98_f32)).abs() < 1e-12,
            "{v:?}"
        );
    }

    #[test]
    fn a_player_in_water_is_refused_rather_than_simulated() {
        let t = tables();
        let tt = travel_tables(&t);
        let mut blocks = floor();
        blocks.push(((0, 64, 0), "minecraft:water[level=0]"));
        let world = World::new(&blocks);
        let mut mv = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("player profile");
        mv.set_velocity((0.0, -0.2, 0.0));
        assert_eq!(
            mv.server_travel(&world, &tt, still(crate::fallable::tags())),
            Err(TravelRefused::InFluid)
        );
        assert_eq!(mv.velocity(), (0.0, -0.2, 0.0), "left exactly as it was");
    }

    /// A 5x5 stone floor at y = 63, so a player stands at y = 64.
    fn floor() -> Vec<(Pos, &'static str)> {
        let mut v = Vec::new();
        for x in -2..=6 {
            for z in -2..=2 {
                v.push(((x, 63, z), "minecraft:stone"));
            }
        }
        v
    }

    fn walk(x: f64, y: f64, z: f64) -> MovePacket {
        MovePacket {
            pos: Some((x, y, z)),
            rot: None,
            on_ground: true,
            horizontal_collision: false,
        }
    }

    /// The headline: the server now runs the player through vanilla's own
    /// collision and refuses a move that walks through a wall.
    #[test]
    fn a_player_who_walks_into_a_wall_is_stopped_by_the_wall() {
        let t = tables();
        let mut w = floor();
        // A stone wall at x = 2, two blocks tall.
        for y in 64..=65 {
            w.push(((2, y, 0), "minecraft:stone"));
        }
        let world = World::new(&w);
        let mut pm = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("tables");
        pm.tick();

        // The client claims to be inside the wall.
        let out = pm.on_move(
            walk(2.5, 64.0, 0.5),
            Gates::default(),
            &world,
            &t.shapes,
            &t.props,
        );
        assert!(
            matches!(
                out,
                Outcome::Corrected {
                    reason: CorrectReason::CollidingNew,
                    ..
                }
            ),
            "a move into a wall is corrected, got {out:?}"
        );

        // The positive control: the same step in a world with no wall lands.
        let world = World::new(&floor());
        let mut pm = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("tables");
        pm.tick();
        let out = pm.on_move(
            walk(2.5, 64.0, 0.5),
            Gates::default(),
            &world,
            &t.shapes,
            &t.props,
        );
        assert!(
            matches!(out, Outcome::Moved { .. }),
            "the same step with no wall lands, got {out:?}"
        );
    }

    /// Walking along the floor a short step at a time: no correction, and
    /// the accepted position is the client's every time. This is the shape
    /// of corpus scenario 530.
    #[test]
    fn an_ordinary_walk_is_never_corrected() {
        let t = tables();
        let world = World::new(&floor());
        let mut pm = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("tables");
        for i in 1..=10 {
            pm.tick();
            let x = 0.5 + f64::from(i) * 0.2;
            let out = pm.on_move(
                walk(x, 64.0, 0.5),
                Gates::default(),
                &world,
                &t.shapes,
                &t.props,
            );
            match out {
                Outcome::Moved { to, landing, .. } => {
                    assert!((to.0 - x).abs() < 1.0e-12, "step {i} took the client's x");
                    assert!(landing.landed, "the packet says on-ground every step");
                    assert!(!landing.hit_ground, "nothing to spend");
                }
                other => panic!("step {i}: {other:?}"),
            }
            assert!((pm.fall_distance() - 0.0).abs() < f64::EPSILON);
        }
    }

    /// Falling off a block: the server accumulates the distance the client
    /// reports and turns it into damage on the landing packet. This is the
    /// input `origin/player-systems` said the server could not produce.
    #[test]
    fn falling_off_a_block_accumulates_and_then_hurts() {
        let t = tables();
        let world = World::new(&floor());
        let mut pm = PlayerMovement::new((0.5, 74.0, 0.5), &t.attrs).expect("tables");
        let airborne = |y: f64| MovePacket {
            pos: Some((0.5, y, 0.5)),
            rot: None,
            on_ground: false,
            horizontal_collision: false,
        };
        // Ten blocks of falling, one block per packet.
        for step in 1..=10 {
            pm.tick();
            let y = 74.0 - f64::from(step);
            let out = pm.on_move(airborne(y), Gates::default(), &world, &t.shapes, &t.props);
            assert!(matches!(out, Outcome::Moved { .. }), "step {step}: {out:?}");
            assert!(!pm.on_ground());
        }
        assert!(
            (pm.fall_distance() - 10.0).abs() < 1.0e-9,
            "ten blocks accumulated, got {}",
            pm.fall_distance()
        );
        // The landing packet.
        pm.tick();
        let out = pm.on_move(
            walk(0.5, 64.0, 0.5),
            Gates::default(),
            &world,
            &t.shapes,
            &t.props,
        );
        let Outcome::Moved { landing, .. } = out else {
            panic!("landed: {out:?}");
        };
        assert!(landing.hit_ground);
        assert!((landing.distance - 10.0).abs() < 1.0e-9);
        assert_eq!(pm.fall_damage(landing), 7, "ten blocks is seven damage");
        assert!((pm.fall_distance() - 0.0).abs() < f64::EPSILON, "spent");
        assert!(pm.on_ground());

        // The positive control: a three-block fall lands for nothing.
        let mut pm = PlayerMovement::new((0.5, 67.0, 0.5), &t.attrs).expect("tables");
        for step in 1..=3 {
            pm.tick();
            let out = pm.on_move(
                airborne(67.0 - f64::from(step)),
                Gates::default(),
                &world,
                &t.shapes,
                &t.props,
            );
            assert!(matches!(out, Outcome::Moved { .. }), "step {step}");
        }
        pm.tick();
        let Outcome::Moved { landing, .. } = pm.on_move(
            walk(0.5, 64.0, 0.5),
            Gates::default(),
            &world,
            &t.shapes,
            &t.props,
        ) else {
            panic!("landed");
        };
        assert!(landing.hit_ground, "it did land on stone");
        assert_eq!(pm.fall_damage(landing), 0, "three blocks is free");
    }

    /// A block whose collision shape the table cannot answer stops the move
    /// rather than being walked through.
    #[test]
    fn an_unknown_state_refuses_the_move_rather_than_passing_through_it() {
        let t = tables();
        let mut w = floor();
        w.push(((2, 64, 0), "minecraft:not_a_block[bogus=1]"));
        let world = World::new(&w);
        let mut pm = PlayerMovement::new((0.5, 64.0, 0.5), &t.attrs).expect("tables");
        pm.tick();
        let out = pm.on_move(
            walk(2.5, 64.0, 0.5),
            Gates::default(),
            &world,
            &t.shapes,
            &t.props,
        );
        assert!(
            matches!(out, Outcome::NoShape { .. }),
            "an unknown state is refused, got {out:?}"
        );
        // And the player did not move.
        pm.tick();
        let world = World::new(&floor());
        assert!(
            matches!(
                pm.on_move(
                    walk(0.7, 64.0, 0.5),
                    Gates::default(),
                    &world,
                    &t.shapes,
                    &t.props
                ),
                Outcome::Moved { .. }
            ),
            "and the tracker is not wedged afterwards"
        );
    }

    /// **Walking away from an unknown block, the way `world_send` answers
    /// `NoShape`: the tracker follows the fall-through and the walk gets
    /// clear of it.** Shaped on the corpus defect: a `moving_piston` left at a
    /// piston head's position, diagonally behind a player standing at z = 0,
    /// whose box reaches into that cell. Without [`PlayerMovement::adopt_unchecked_move`]
    /// every step is swept from the stale body, stays `NoShape`, and no step
    /// is ever `Moved` — so the assertion that the walk ends `Moved` at the
    /// client's x is the one the defect fails.
    #[test]
    fn a_walk_away_from_an_unknown_block_is_not_left_behind_by_the_tracker() {
        let t = tables();
        let mut w = floor();
        w.push((
            (0, 64, -1),
            "minecraft:moving_piston[facing=north,type=normal]",
        ));
        let world = World::new(&w);
        let mut pm = PlayerMovement::new((0.5, 64.0, 0.0), &t.attrs).expect("tables");
        let (mut unknown, mut last) = (0, None);
        for i in 1..=11 {
            pm.tick();
            let x = 0.5 + f64::from(i) * 0.5;
            let pkt = walk(x, 64.0, 0.0);
            let out = pm.on_move(pkt, Gates::default(), &world, &t.shapes, &t.props);
            match out {
                Outcome::NoShape { .. } => {
                    unknown += 1;
                    pm.adopt_unchecked_move(pkt.pos.expect("a position packet"));
                }
                Outcome::Corrected { .. } => panic!("step {i} was corrected: {out:?}"),
                _ => {}
            }
            last = Some(out);
        }
        assert!(
            unknown > 0,
            "the first steps must reach the unknown cell, or this measured nothing"
        );
        match last {
            Some(Outcome::Moved { to, .. }) => {
                assert!(
                    (to.0 - 6.0).abs() < 1.0e-12,
                    "the walk ended at the client's x"
                );
            }
            other => panic!("the walk never got clear of the unknown block: {other:?}"),
        }
    }

    /// The pose does not change the box, so a "sneaking under a slab" test
    /// is a test that the box did NOT shrink.
    #[test]
    fn the_players_box_is_the_same_whatever_the_pose() {
        let t = tables();
        let p = Profile::player(&t.attrs).expect("attributes");
        assert!((p.width - 0.6).abs() < f64::EPSILON, "sized(0.6f, 1.8f)");
        assert!((p.height - 1.8).abs() < f64::EPSILON);
        // There is no pose parameter anywhere in this crate's player box, and
        // that is the assertion: `EntityDimensions` is a record of width,
        // height, eyeHeight, attachments and fixed, and
        // `LivingEntity.getDefaultDimensions(Pose)` discards its argument.
        let b = p.bounding_box(Vec3::new(0.0, 64.0, 0.0));
        assert!((b.max_y - b.min_y - 1.8).abs() < 1.0e-12);
        assert!((b.max_x - b.min_x - 0.6).abs() < 1.0e-12);
    }
}
