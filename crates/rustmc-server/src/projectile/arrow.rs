//! `AbstractArrow.tick` — the whole flight, from launch to `inGround` to
//! despawn — transcribed from
//! `net.minecraft.world.entity.projectile.arrow.AbstractArrow` in the pinned
//! 26.2 jar.
//!
//! # The constants, and where each is read
//!
//! ```text
//!   ARROW_BASE_DAMAGE  2.0d       AbstractArrow, the field
//!   SHAKE_TIME         7          onHitBlock sets shakeTime to it
//!   INERTIA            0.99f      getAirDrag()
//!   WATER_INERTIA      0.6f       getWaterInertia()
//!   gravity            0.05d      getDefaultGravity(), NOT 0.03 and not an attribute
//!   despawn            1200       tickDespawn(), on `life` and not on tickCount
//!   FLAG_CRIT          1          the ID_FLAGS byte, bit 0
//!   FLAG_NOPHYSICS     2          the ID_FLAGS byte, bit 1
//! ```
//!
//! **The drag is applied before the gravity, and only one of the two drags
//! runs.** `applyInertia(getWaterInertia())` fires at the top of the moving
//! branch when `isInWater()`, and `applyInertia(getAirDrag())` fires near the
//! bottom when `!isInWater()`. An arrow in water is never given 0.99, and one
//! in air is never given 0.6; a port applying both to a swimming arrow is out
//! by a factor of 0.594 per tick, which looks like a plausible drag.
//!
//! # The tick order, offset by offset
//!
//! ```text
//!   0    boolean physics = !isNoPhysics();
//!   13   Vec3 vec = getDeltaMovement();
//!   18   BlockPos bp = blockPosition(); BlockState st = level.getBlockState(bp);
//!   33   if (!st.isAir() && physics) {                      <- the STICK test
//!            VoxelShape s = st.getCollisionShape(level, bp);
//!            if (!s.isEmpty())
//!              for (AABB b : s.toAabbs())
//!                if (b.move(bp).contains(position())) {
//!                    setDeltaMovement(ZERO); setInGround(true); break; }
//!        }
//!   137  if (shakeTime > 0) shakeTime--;
//!   154  if (isInWaterOrRain()) clearFire();
//!   165  if (isInGround() && physics) {                     <- the GROUNDED arm
//!            if (lastState != st && shouldFall()) startFalling();
//!            else tickDespawn();
//!            inGroundTime++;
//!            if (isAlive()) applyEffectsFromBlocks();
//!            setSharedFlagOnFire(getRemainingFireTicks() > 0);
//!            return;                                        <- NO gravity, NO drag
//!        }
//!   261  inGroundTime = 0;
//!   266  Vec3 pos = position();
//!   272  if (isInWater()) { applyInertia(getWaterInertia()); addBubbleParticles(pos); }
//!   293  if (isCritArrow()) 4 x CRIT particles
//!   398  yaw   = atan2(physics ? vec.x : -vec.x, physics ? vec.z : -vec.z) * 57.2957763671875
//!   443  pitch = atan2(vec.y, vec.horizontalDistance())     * 57.2957763671875
//!   461  setXRot(lerpRotation(getXRot(), pitch));
//!   474  setYRot(lerpRotation(getYRot(), yaw));
//!   487  checkLeftOwner();
//!   491  if (physics) stepMoveAndHit(level.clipIncludingBorder(
//!            new ClipContext(pos, pos.add(vec), COLLIDER, NONE, this)));
//!        else { setPos(pos.add(vec)); applyEffectsFromBlocks(); }
//!   549  if (!isInWater()) applyInertia(getAirDrag());
//!   564  if (physics && !isInGround()) applyGravity();
//!   579  super.tick();                                      <- Projectile.tick
//! ```
//!
//! Three things a reading gets wrong. **The rotation is taken from the
//! delta the tick STARTED with**, captured at offset 13, so it lags the
//! motion by one tick and is unaffected by this tick's drag. **The grounded
//! arm returns**, so a stuck arrow gets neither drag nor gravity and its
//! `life` counter is the only thing moving. And **`lastState != st` is a
//! reference comparison on `BlockState`**, which for an immutable interned
//! state is state-id equality, so it fires when the block under the arrow
//! is *replaced*, not when the arrow moves.
//!
//! # What is refused, and by whom
//!
//! * `onHitEntity`'s damage — `target.hurtServer(level, arrow(this, owner),
//!   damage)` — is `mob-combat`'s. [`Tick::hits`] reports the target and the
//!   damage vanilla would have applied and applies none.
//! * `doPostHurtEffects` (the tipped arrow's potion list) needs a potion
//!   table `data/vanilla` does not carry.
//! * `applyEffectsFromBlocks`, `isInWaterOrRain` and the fire flag need the
//!   fluid and weather state; they are inputs to this module, supplied by
//!   the caller through [`Env`], so a caller that cannot answer them says so
//!   rather than this module guessing.
//! * `setBaseDamageFromMob(float f)` is the skeleton's path and no mob here
//!   shoots. It is recorded rather than ported because it is **two RNG
//!   draws** a mob-combat lane will have to place exactly:
//!
//!   ```text
//!     setBaseDamage(f * 2.0f
//!         + random.triangle(level.getDifficulty().getId() * 0.11, 0.57425));
//!   ```
//!
//!   `triangle` is two `nextDouble`s, the difficulty id multiplies the
//!   CENTRE and not the spread, and `f * 2.0f` is a float multiply widened
//!   afterwards.
//!
//! # The plain arrow overrides none of the physics constants
//!
//! Checked rather than assumed: `net.minecraft.world.entity.projectile.arrow
//! .Arrow` declares only `doPostHurtEffects`, `getDefaultPickupItem` and one
//! lambda. It does **not** override `getAirDrag`, `getWaterInertia` or
//! `getDefaultGravity`, so 0.99, 0.6 and 0.05 are the values a plain arrow
//! actually flies with. `SpectralArrow` and `ThrownTrident` are not ported
//! and would each want this checked again.

use crate::collide::{Aabb, Axis, CollisionBlocks, CollisionShapes, NoShapeFor, Vec3};
use crate::projectile::clip::{self, Clip};
use crate::projectile::hit::{self, EntityHit, ProjectileTargets, Target};
use crate::projectile::math;
use crate::projectile::shoot::{lerp_rotation, RAD_TO_DEG};
use rustmc_data::BlockPos;
use rustmc_entity::mth::atan2;
use rustmc_worldgen::rng::RandomSource;

/// `AbstractArrow.ARROW_BASE_DAMAGE`.
pub const ARROW_BASE_DAMAGE: f64 = 2.0;
/// `AbstractArrow.SHAKE_TIME`.
pub const SHAKE_TIME: i32 = 7;
/// `AbstractArrow.INERTIA` — `getAirDrag()`.
pub const INERTIA: f32 = 0.99;
/// `AbstractArrow.WATER_INERTIA` — `getWaterInertia()`.
pub const WATER_INERTIA: f32 = 0.6;
/// `AbstractArrow.getDefaultGravity()` — `ldc2_w 0.05d`.
pub const ARROW_GRAVITY: f64 = 0.05;
/// `tickDespawn`'s ceiling, on the `life` counter.
pub const DESPAWN_LIFE: i32 = 1200;
/// `EntityTypes.ARROW.sized(0.5f, 0.5f)`.
pub const ARROW_WIDTH: f64 = 0.5;
/// `EntityTypes.ARROW.sized(0.5f, 0.5f)`.
pub const ARROW_HEIGHT: f64 = 0.5;

/// `AbstractArrow.FLAG_CRIT`.
pub const FLAG_CRIT: u8 = 1;
/// `AbstractArrow.FLAG_NOPHYSICS`.
pub const FLAG_NOPHYSICS: u8 = 2;

/// `AbstractArrow.Pickup`, in ordinal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pickup {
    Disallowed,
    Allowed,
    CreativeOnly,
}

/// The world facts `AbstractArrow.tick` reads that are not blocks.
///
/// Passed in rather than queried so that a caller which cannot answer one
/// has to say so at the call: `isInWater` decides which of the two drags
/// runs, and defaulting it to `false` would silently give every arrow the
/// air drag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Env {
    /// `Entity.isInWater()`.
    pub in_water: bool,
    /// `Entity.isInWaterOrRain()`.
    pub in_water_or_rain: bool,
}

/// What one tick produced, for the caller to put on the wire.
///
/// Particles are absent and that is a fact rather than an omission:
/// `Level.addParticle` is a no-op on `ServerLevel` (the override is empty),
/// so the four CRIT particles and the four BUBBLE particles this tick can
/// emit never reach a client from the server side at all. The **sound** in
/// `onHitBlock` does go out, through `Entity.playSound`, and is reported.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tick {
    /// `Entity.playSound(SoundEvent, float volume, float pitch)` calls, in
    /// order.
    pub sounds: Vec<Sound>,
    /// Entities this tick struck, in the order `hitTargetsOrDeflectSelf`
    /// reached them, each with the damage vanilla would have applied.
    pub hits: Vec<Struck>,
    /// The block this tick stuck into, if any.
    pub stuck_in: Option<BlockPos>,
    /// `Entity.discard()` was called — the arrow is gone.
    pub discarded: bool,
    /// `Entity.needsSync`, which is what makes `ServerEntity.sendChanges`
    /// emit outside its `updateInterval`.
    pub needs_sync: bool,
}

/// One `Entity.playSound` call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    /// The `SoundEvents` constant's registry name.
    pub name: &'static str,
    pub volume: f32,
    pub pitch: f32,
}

/// One entity hit, with the damage the port declines to apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Struck {
    pub target_id: i32,
    pub at: Vec3,
    /// `Mth.ceil(Mth.clamp((float) getDeltaMovement().length() * baseDamage,
    /// 0.0, 2.147483647E9))`, plus the crit bonus when `isCritArrow()`.
    ///
    /// **Reported, not applied.** `mob-combat` owns `hurtOrSimulate`. The
    /// figure is an `int` because vanilla's is: `hurtOrSimulate` takes the
    /// `i2f` of it and nothing between here and there is fractional.
    pub damage: i32,
}

/// One arrow in flight.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrow {
    pub pos: Vec3,
    pub movement: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    /// `Entity.tickCount`, incremented by `ServerLevel.tickNonPassenger`
    /// **before** `tick()` runs, which is why [`Arrow::tick`] increments it
    /// first. `ProjectileUtil.computeMargin` reads it.
    pub tick_count: i32,
    /// The synched `IN_GROUND` boolean.
    pub in_ground: bool,
    pub in_ground_time: i32,
    pub shake_time: i32,
    /// `AbstractArrow.life`, which `tickDespawn` raises and `startFalling`
    /// resets. Not `tickCount`.
    pub life: i32,
    pub crit: bool,
    pub pierce_level: i8,
    pub no_physics: bool,
    /// `AbstractArrow.lastState`, as a block-state id. `null` until the
    /// arrow has hit something.
    pub last_state: Option<u32>,
    pub base_damage: f64,
    pub pickup: Pickup,
    /// The owner's entity id — `Projectile.owner`, and the value
    /// `ClientboundAddEntityPacket`'s trailing `data` varint carries.
    pub owner: Option<i32>,
    /// `Projectile.leftOwner`. Latched: `checkLeftOwner` only ever sets it.
    pub left_owner: bool,
    /// `Projectile.leftOwnerChecked`, cleared at the end of `Projectile.tick`
    /// so the check runs at most once per tick.
    pub left_owner_checked: bool,
    pub has_been_shot: bool,
    /// `AbstractArrow.piercingIgnoreEntityIds`.
    pub piercing_ignore: Vec<i32>,
    pub removed: bool,
}

impl Arrow {
    /// A fresh arrow at a position, with the fields vanilla's constructor
    /// leaves at their defaults.
    #[must_use]
    pub fn at(pos: Vec3) -> Self {
        Self {
            pos,
            movement: Vec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            tick_count: 0,
            in_ground: false,
            in_ground_time: 0,
            shake_time: 0,
            life: 0,
            crit: false,
            pierce_level: 0,
            no_physics: false,
            last_state: None,
            base_damage: ARROW_BASE_DAMAGE,
            pickup: Pickup::Disallowed,
            owner: None,
            left_owner: false,
            left_owner_checked: false,
            has_been_shot: false,
            piercing_ignore: Vec::new(),
            removed: false,
        }
    }

    /// `Entity.getBoundingBox()` — `EntityDimensions.makeBoundingBox`, which
    /// halves the width and puts the box's floor at the entity's y.
    #[must_use]
    pub fn bounding_box(&self) -> Aabb {
        let h = ARROW_WIDTH / 2.0;
        Aabb::new(
            self.pos.x - h,
            self.pos.y,
            self.pos.z - h,
            self.pos.x + h,
            self.pos.y + ARROW_HEIGHT,
            self.pos.z + h,
        )
    }

    /// The `ID_FLAGS` byte a client is sent.
    #[must_use]
    pub const fn flags(&self) -> u8 {
        let mut b = 0;
        if self.crit {
            b |= FLAG_CRIT;
        }
        if self.no_physics {
            b |= FLAG_NOPHYSICS;
        }
        b
    }

    /// `AbstractArrow.tick()`, plus the `tickCount` increment
    /// `ServerLevel.tickNonPassenger` performs immediately before it.
    ///
    /// # Errors
    ///
    /// [`NoShapeFor`] when the flight path or the block under the arrow has
    /// no state-keyed collision shape.
    pub fn tick<W, T, R>(
        &mut self,
        shapes: &CollisionShapes,
        world: &W,
        targets: &T,
        rng: &mut R,
        env: Env,
    ) -> Result<Tick, NoShapeFor>
    where
        W: CollisionBlocks,
        T: ProjectileTargets,
        R: RandomSource,
    {
        self.tick_count += 1;
        let mut out = Tick::default();

        let physics = !self.no_physics;
        let vec = self.movement;
        let bp = containing(self.pos);
        let state_id = world.state_id_at(bp);
        let shape = shapes
            .get(state_id)
            .ok_or(NoShapeFor { pos: bp, state_id })?;

        // Offsets 33-136: the arrow is inside a solid, so it stops dead.
        //
        // Vanilla's guard is `!state.isAir() && physics` and only then
        // `!shape.isEmpty()`. The `isAir` half is dropped here with an
        // argument rather than because it was inconvenient: no air state has
        // a non-empty collision shape, so `!isAir() && !isEmpty()` and
        // `!isEmpty()` select the same states. If a future version gave an
        // air state a collision shape the two would part, which is why this
        // is written down instead of being silently equivalent.
        if physics && !shape.is_empty() {
            for cell in occupied_boxes(shape) {
                if math::contains(&math::moved_by_block(&cell, bp), self.pos) {
                    self.movement = Vec3::ZERO;
                    self.in_ground = true;
                    break;
                }
            }
        }

        if self.shake_time > 0 {
            self.shake_time -= 1;
        }
        // `clearFire()` — this server tracks no fire ticks on a projectile.
        let _ = env.in_water_or_rain;

        // Offsets 165-260: the grounded arm, which RETURNS.
        if self.in_ground && physics {
            if self.last_state != Some(state_id) && self.should_fall(shapes, world)? {
                self.start_falling(rng);
            } else {
                self.life += 1;
                if self.life >= DESPAWN_LIFE {
                    self.removed = true;
                    out.discarded = true;
                }
            }
            self.in_ground_time += 1;
            return Ok(out);
        }

        self.in_ground_time = 0;
        let pos = self.pos;

        if env.in_water {
            self.movement = math::scale(self.movement, f64::from(WATER_INERTIA));
        }

        // Offsets 398-486: the rotation, from the delta captured at the TOP.
        let (yx, yz) = if physics {
            (vec.x, vec.z)
        } else {
            (-vec.x, -vec.z)
        };
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
        let yaw = (atan2(yx, yz) * RAD_TO_DEG) as f32;
        #[expect(clippy::cast_possible_truncation, reason = "vanilla's d2f")]
        let pitch = (atan2(vec.y, math::horizontal_distance(vec)) * RAD_TO_DEG) as f32;
        self.x_rot = lerp_rotation(self.x_rot, pitch);
        self.y_rot = lerp_rotation(self.y_rot, yaw);

        self.check_left_owner(targets);

        if physics {
            let to = math::add(pos, vec);
            let clipped = clip::clip(shapes, world, pos, to)?;
            self.step_move_and_hit(shapes, world, targets, rng, &clipped, &mut out)?;
        } else {
            self.pos = math::add(pos, vec);
        }

        if !env.in_water {
            self.movement = math::scale(self.movement, f64::from(INERTIA));
        }
        if physics && !self.in_ground {
            // `Entity.applyGravity()`: `if (g != 0) delta = delta.add(0, -g, 0)`.
            self.movement = Vec3::new(
                self.movement.x,
                self.movement.y - ARROW_GRAVITY,
                self.movement.z,
            );
        }

        // `Projectile.tick()`: the PROJECTILE_SHOOT game event on the first
        // tick, `checkLeftOwner` again, `Entity.tick()`, then
        // `leftOwnerChecked = false`.
        self.has_been_shot = true;
        self.check_left_owner(targets);
        self.left_owner_checked = false;

        Ok(out)
    }

    /// `AbstractArrow.stepMoveAndHit(BlockHitResult)`, offsets 0-195.
    ///
    /// A loop, and the loop is the piercing path: an arrow with
    /// `pierceLevel > 0` that struck something and was not deflected
    /// re-clips from its new position and keeps going in the same tick.
    fn step_move_and_hit<W, T, R>(
        &mut self,
        shapes: &CollisionShapes,
        world: &W,
        targets: &T,
        rng: &mut R,
        block_hit: &Clip,
        out: &mut Tick,
    ) -> Result<(), NoShapeFor>
    where
        W: CollisionBlocks,
        T: ProjectileTargets,
        R: RandomSource,
    {
        while !self.removed {
            let from = self.pos;
            let margin = hit::compute_margin(self.tick_count);
            let search = hit::search_box(self.bounding_box(), self.movement);
            let owned = self.owner.is_some();
            let left = self.left_owner;
            let pierced = self.piercing_ignore.clone();
            let mut hits = hit::many_entity_hit_result(
                shapes,
                world,
                targets,
                from,
                block_hit.location(),
                search,
                margin,
                false,
                &|t: &Target| hit::can_hit_entity(t, owned, left, &pierced),
            )?;
            // `hits.sort(comparingDouble(h -> pos.distanceToSqr(
            //     h.getEntity().position())))` — the ENTITY's position, not
            // the hit point, and a stable sort.
            hits.sort_by(|a, b| {
                math::distance_to_sqr(from, a.target.pos)
                    .partial_cmp(&math::distance_to_sqr(from, b.target.pos))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            let target_at = hits
                .first()
                .map_or_else(|| block_hit.location(), |h| h.location);
            self.pos = target_at;

            if hits.is_empty() {
                if !self.removed && !block_hit.is_miss() {
                    self.on_hit_block(shapes, world, block_hit, rng, out)?;
                    out.needs_sync = true;
                }
                return Ok(());
            }

            if self.removed {
                // Vanilla's `goto 0` back to a loop head that tests
                // `isAlive()`, which then returns.
                continue;
            }
            if self.no_physics {
                // Vanilla loops here forever, and the branch is unreachable:
                // `stepMoveAndHit` is only called under `physics`, and
                // `physics` is `!isNoPhysics()`, whose flag `setNoPhysics`
                // writes in the same call as this field. Returning is the
                // only defensible thing to do with a branch that cannot be
                // taken and would hang if it were.
                return Ok(());
            }
            self.hit_targets(&hits, rng, out);
            out.needs_sync = true;
            if self.pierce_level > 0 {
                continue;
            }
            return Ok(());
        }
        Ok(())
    }

    /// `AbstractArrow.hitTargetsOrDeflectSelf(Collection)` reduced to the
    /// no-deflection case.
    ///
    /// **`ProjectileDeflection` is not ported.** The only deflections a
    /// vanilla arrow can suffer are `AIM_DEFLECT` from being hit by another
    /// redirectable projectile and the wind charge's; neither exists in this
    /// server. The loop below therefore runs to the end of the collection,
    /// which is exactly what vanilla does when every result is `NONE`.
    fn hit_targets<R: RandomSource>(&mut self, hits: &[EntityHit], rng: &mut R, out: &mut Tick) {
        for h in hits {
            let damage = self.damage_for_hit(rng);
            out.hits.push(Struck {
                target_id: h.target.id,
                at: h.location,
                damage,
            });
            if self.pierce_level > 0 {
                self.piercing_ignore.push(h.target.id);
            } else {
                // `AbstractArrow.onHitEntity` ends in `discard()` for a
                // non-piercing arrow that did not bounce.
                self.removed = true;
                out.discarded = true;
                return;
            }
        }
    }

    /// The damage figure `onHitEntity` computes, before `hurtOrSimulate`.
    ///
    /// ```text
    ///   10: float f = (float) getDeltaMovement().length();     <- d2f, NARROWED
    ///   19: double d = baseDamage;
    ///   52: if (getWeaponItem() != null && level instanceof ServerLevel s)
    ///           d = EnchantmentHelper.modifyDamage(s, weapon, target, src, (float) d);
    ///   98: int i = Mth.ceil(Mth.clamp((double) f * d, 0.0, 2.147483647E9));
    ///  115: if (getPierceLevel() > 0) { ... piercingIgnoreEntityIds ... }
    ///  192: if (isCritArrow()) {
    ///           long j = random.nextInt(i / 2 + 2);
    ///           i = (int) Math.min(j + (long) i, 2147483647L);
    ///       }
    /// ```
    ///
    /// **The speed is narrowed to `float` before it multiplies the base
    /// damage** — `d2f` at offset 17 and `f2d` back at 98 — so the product is
    /// taken on a float-precision speed and a double-precision base. Using
    /// the `double` length gives a different `Mth.ceil` for a band of speeds
    /// either side of every integer boundary, which is a whole point of
    /// damage.
    ///
    /// **The crit bonus IS drawn here and this port takes it.** It is
    /// `random.nextInt(i / 2 + 2)` on the entity's own RNG, it happens before
    /// `hurtOrSimulate` and it does not depend on the damage landing, so
    /// skipping it would displace every later draw in the tick — which is the
    /// same defect this lane wrote down for `getMovementToShoot`'s six draws
    /// and would then have committed one module over. Integer division:
    /// `i = 8` gives `nextInt(6)`, `i = 9` gives `nextInt(6)` as well.
    ///
    /// `EnchantmentHelper.modifyDamage` is absent, with the same argument the
    /// rest of this module uses for enchantments: there is no enchantment
    /// table in `data/vanilla` and a no-op is what an unenchanted bow gets.
    fn damage_for_hit<R: RandomSource>(&self, rng: &mut R) -> i32 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's d2f at offset 17"
        )]
        let speed = math::length(self.movement) as f32;
        let product = f64::from(speed) * self.base_damage;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's Mth.ceil of an already-clamped value"
        )]
        let mut dmg = product.clamp(0.0, 2_147_483_647.0).ceil() as i32;
        if self.crit {
            let bonus = i64::from(rng.next_i32_bound(dmg / 2 + 2));
            #[expect(
                clippy::cast_possible_truncation,
                reason = "vanilla's l2i after Math.min against 2147483647L"
            )]
            let capped = bonus.saturating_add(i64::from(dmg)).min(2_147_483_647) as i32;
            dmg = capped;
        }
        dmg
    }

    /// `AbstractArrow.onHitBlock(BlockHitResult)`, offsets 0-183.
    fn on_hit_block<W, R>(
        &mut self,
        shapes: &CollisionShapes,
        world: &W,
        hit: &Clip,
        rng: &mut R,
        out: &mut Tick,
    ) -> Result<(), NoShapeFor>
    where
        W: CollisionBlocks,
        R: RandomSource,
    {
        let Clip::Block(b) = hit else {
            return Ok(());
        };
        let state_id = world.state_id_at(b.pos);
        // `shapes.get` is not needed for the state itself, but a position
        // whose shape is unknown must refuse here too rather than be recorded
        // as `lastState` and then compared against later.
        shapes.get(state_id).ok_or(NoShapeFor {
            pos: b.pos,
            state_id,
        })?;
        self.last_state = Some(state_id);

        let d = self.movement;
        let step = math::scale(
            Vec3::new(d.x.signum(), d.y.signum(), d.z.signum()),
            // `ldc2_w 0.05000000074505806d` — the float 0.05f widened, not
            // the double 0.05 that `getDefaultGravity` uses.
            0.050_000_000_745_058_06,
        );
        self.pos = math::subtract(self.pos, step);
        self.movement = Vec3::ZERO;
        out.sounds.push(Sound {
            name: "minecraft:entity.arrow.hit",
            volume: 1.0,
            pitch: 1.2 / (rng.next_f32() * 0.2 + 0.9),
        });
        self.in_ground = true;
        self.shake_time = SHAKE_TIME;
        self.crit = false;
        self.pierce_level = 0;
        self.piercing_ignore.clear();
        out.stuck_in = Some(b.pos);
        Ok(())
    }

    /// `AbstractArrow.shouldFall()`:
    /// `isInGround() && level.noCollision(new AABB(position(),
    /// position()).inflate(0.06))`.
    ///
    /// **A degenerate box inflated by 0.06**, so the probe is 0.12 on a side
    /// centred on the arrow's position — not on its bounding box.
    fn should_fall<W: CollisionBlocks>(
        &self,
        shapes: &CollisionShapes,
        world: &W,
    ) -> Result<bool, NoShapeFor> {
        if !self.in_ground {
            return Ok(false);
        }
        let probe = math::inflate(
            &Aabb::new(
                self.pos.x, self.pos.y, self.pos.z, self.pos.x, self.pos.y, self.pos.z,
            ),
            0.06,
        );
        no_collision(shapes, world, &probe)
    }

    /// `AbstractArrow.startFalling()`, offsets 0-65.
    ///
    /// ```text
    ///   setInGround(false);
    ///   setDeltaMovement(getDeltaMovement().multiply(
    ///       random.nextFloat() * 0.2f, random.nextFloat() * 0.2f,
    ///       random.nextFloat() * 0.2f));
    ///   life = 0;
    /// ```
    ///
    /// Three `nextFloat` draws, in x, y, z order, each scaled in **float**
    /// and then widened by the `f2d` before `multiply`.
    fn start_falling<R: RandomSource>(&mut self, rng: &mut R) {
        self.in_ground = false;
        let fx = rng.next_f32() * 0.2;
        let fy = rng.next_f32() * 0.2;
        let fz = rng.next_f32() * 0.2;
        self.movement = math::multiply(self.movement, f64::from(fx), f64::from(fy), f64::from(fz));
        self.life = 0;
    }

    /// `Projectile.checkLeftOwner()` and `isOutsideOwnerCollisionRange()`.
    ///
    /// ```text
    ///   if (!leftOwner && !leftOwnerChecked) {
    ///       leftOwner = isOutsideOwnerCollisionRange();
    ///       leftOwnerChecked = true;
    ///   }
    ///   isOutsideOwnerCollisionRange():
    ///       Entity o = getOwner();
    ///       if (o == null) return true;
    ///       AABB probe = getBoundingBox().expandTowards(getDeltaMovement())
    ///                                    .inflate(1.0);
    ///       return o.getRootVehicle().getSelfAndPassengers()
    ///               .filter(CAN_BE_PICKED)
    ///               .noneMatch(e -> probe.intersects(e.getBoundingBox()));
    /// ```
    ///
    /// **A null owner leaves immediately.** So an arrow the server spawned
    /// with no shooter can hit anything from its first tick, which is the
    /// behaviour a `/summon`ed arrow has and the reason the owner field is
    /// `Option` here rather than defaulted.
    fn check_left_owner<T: ProjectileTargets>(&mut self, targets: &T) {
        if self.left_owner || self.left_owner_checked {
            return;
        }
        self.left_owner = match self.owner {
            None => true,
            Some(id) => {
                let probe = hit::search_box(self.bounding_box(), self.movement);
                // Vanilla walks the owner's vehicle tree; nothing in this
                // server rides anything, so the tree is the owner alone.
                !targets
                    .targets_in(probe)
                    .iter()
                    .any(|t| t.id == id && probe.intersects(&t.box_))
            }
        };
        self.left_owner_checked = true;
    }
}

/// `Level.noCollision(AABB)` reduced to blocks: no entity in this server
/// takes part in a projectile's `shouldFall` probe, and vanilla's own
/// `noCollision(AABB)` overload passes `null` for the entity, so the entity
/// arm is absent in the bytecode too rather than skipped here.
fn no_collision<W: CollisionBlocks>(
    shapes: &CollisionShapes,
    world: &W,
    probe: &Aabb,
) -> Result<bool, NoShapeFor> {
    #[expect(clippy::cast_possible_truncation, reason = "block coordinates")]
    let (x0, y0, z0) = (
        probe.min_x.floor() as i32,
        probe.min_y.floor() as i32,
        probe.min_z.floor() as i32,
    );
    #[expect(clippy::cast_possible_truncation, reason = "block coordinates")]
    let (x1, y1, z1) = (
        probe.max_x.floor() as i32,
        probe.max_y.floor() as i32,
        probe.max_z.floor() as i32,
    );
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                let pos = BlockPos { x, y, z };
                let state_id = world.state_id_at(pos);
                let shape = shapes.get(state_id).ok_or(NoShapeFor { pos, state_id })?;
                for cell in occupied_boxes(shape) {
                    if math::moved_by_block(&cell, pos).intersects(probe) {
                        return Ok(false);
                    }
                }
            }
        }
    }
    Ok(true)
}

/// `VoxelShape.toAabbs()` reduced to one box per occupied cell. See
/// [`crate::projectile::clip::shape_clip`] for why the un-merged list is
/// equivalent everywhere this module uses it.
fn occupied_boxes(shape: &crate::collide::Shape) -> Vec<Aabb> {
    let xs = shape.coords(Axis::X);
    let ys = shape.coords(Axis::Y);
    let zs = shape.coords(Axis::Z);
    let mut out = Vec::new();
    for x in 0..shape.size(Axis::X) {
        for y in 0..shape.size(Axis::Y) {
            for z in 0..shape.size(Axis::Z) {
                #[expect(clippy::cast_possible_wrap, reason = "grid indices are small")]
                if shape.is_full_wide(x as i64, y as i64, z as i64) {
                    out.push(Aabb::new(
                        xs[x],
                        ys[y],
                        zs[z],
                        xs[x + 1],
                        ys[y + 1],
                        zs[z + 1],
                    ));
                }
            }
        }
    }
    out
}

fn containing(v: Vec3) -> BlockPos {
    BlockPos {
        x: rustmc_entity::mth::floor(v.x),
        y: rustmc_entity::mth::floor(v.y),
        z: rustmc_entity::mth::floor(v.z),
    }
}
