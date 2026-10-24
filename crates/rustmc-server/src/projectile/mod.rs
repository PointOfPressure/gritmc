//! Vanilla 26.2's thrown and shot entities on the server.
//!
//! Every constant, branch and RNG draw here is read out of
//! `oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar` with
//! `javap -p -c -constants` under JDK 25.
//!
//! # What is here
//!
//! | module | vanilla |
//! |---|---|
//! | [`math`] | the `Vec3`/`AABB` operations `crate::collide` does not carry, including `AABB.clip` |
//! | [`shoot`] | `Projectile.getMovementToShoot`/`shoot`/`shootFromRotation`, `lerpRotation`, `updateRotation` |
//! | [`clip`] | `BlockGetter.clip`, `traverseBlocks`, `VoxelShape.clip` |
//! | [`hit`] | `ProjectileUtil.computeMargin`/`getEntityHitResult`/`getManyEntityHitResult`, `canHitEntity` |
//! | [`arrow`] | `AbstractArrow.tick` and the `inGround` state machine |
//! | [`bow`] | `BowItem.releaseUsing`, `CrossbowItem`'s charge and shot pitch |
//! | [`wire`] | the `add_entity` `data` field and the six `EntityKind` rows |
//! | [`pickup`] | `AbstractArrow.playerTouch` and `tryPickup` |
//! | [`throwable`] | `ThrowableProjectile.tick` and the five item projectiles' `onHit` |
//!
//! # What is deliberately absent, with the class that owns it
//!
//! * **Damage.** `AbstractArrow.onHitEntity` ends in
//!   `target.hurtServer(level, damageSources().arrow(this, owner), damage)`
//!   and nothing in this server can apply damage to a mob yet. The hit is
//!   reported to the caller as a `Hit::Entity` and what the caller does with
//!   it is `mob-combat`'s.
//! * **Tipped-arrow effects.** `Arrow.doPostHurtEffects` walks the potion
//!   contents component; `data/vanilla` carries no potion table, so a port
//!   here would be a guess at the effect list.
//! * **The ender pearl's teleport.** `ThrownEnderpearl.onHit` calls
//!   `ServerPlayer.teleportTo` plus `resetFallDistance` and a five-damage
//!   `hurtServer`; both are player state the player-systems lanes own.
//! * **The per-player broadcast view.** Which clients receive an
//!   `add_entity` is `entity-tracker`'s and is not decided here.
//! * **`BlockState.onProjectileHit`.** `Projectile.onHitBlock`'s whole body
//!   is `state.onProjectileHit(level, state, hit, this)`, whose
//!   `BlockBehaviour` default is a bare `return`. **Ten blocks override it**,
//!   swept by name rather than counted from memory:
//!
//!   ```text
//!   javap -p -cp server-26.2.jar $(unzip -l ... net/minecraft/world/level/block/*.class ...)
//!       AbstractCandleBlock  AmethystBlock   BellBlock      BigDripleafBlock
//!       CampfireBlock        ChorusFlowerBlock              DecoratedPotBlock
//!       SpeleothemBlock      TargetBlock     TntBlock
//!   ```
//!
//!   325 classes scanned. None of the ten is reachable by a projectile in
//!   this server today — nothing spawns one — and each wants a different
//!   lane (`TargetBlock` is redstone's, `TntBlock` is explosions', the candle
//!   and campfire arms are the fire lane's). It is a dispatch this module
//!   deliberately does not make, not a branch it forgot.

pub mod arrow;
pub mod bow;
pub mod clip;
pub mod hit;
pub mod math;
pub mod pickup;
pub mod shoot;
pub mod throwable;
pub mod wire;
