//! `fallDistance`, and the damage it becomes when a player lands.
//!
//! Reference (26.2): `Entity.doCheckFallDamage`, `Entity.checkFallDamage`,
//! `Entity.resetFallDistance`, `Entity.setOnGroundWithMovement`,
//! `LivingEntity.checkFallDamage`, `LivingEntity.causeFallDamage`,
//! `LivingEntity.calculateFallDamage`, `LivingEntity.calculateFallPower`,
//! `Block.fallOn`.
//!
//! # Why this could not be written before
//!
//! `origin/player-systems` ported `causeFallDamage` and `calculateFallDamage`
//! and then declined to wire them, because nothing on the server knew the
//! player had fallen: the move packets were decoded, the position was
//! written onto the entity, and no `fallDistance` was ever accumulated.
//! `handleMovePlayer` is where vanilla accumulates it — twice, at offsets
//! 926-966 on the rubber-band path and 1163-1188 on the accepted one — so
//! this module is the missing input and [`crate::move_player`] is what
//! produces its arguments.
//!
//! # The chain, and the two narrowings in it
//!
//! ```text
//!   doCheckFallDamage(dx, dy, dz, onGround):
//!     0-7    if (touchingUnloadedChunk()) return
//!     8-24   checkSupportingBlock(onGround, new Vec3(dx, dy, dz))
//!     25-40  BlockPos p = getOnPosLegacy(); BlockState s = level.getBlockState(p)
//!     42-53  checkFallDamage(dy, onGround, s, p)
//!
//!   Entity.checkFallDamage(dy, onGround, state, pos):
//!     0-24   if (!isInWater() && dy < 0.0) fallDistance -= (double)(float)dy
//!     25-98  if (onGround) {
//!              if (fallDistance > 0.0) { state.getBlock().fallOn(...); gameEvent(HIT_GROUND) }
//!              resetFallDistance()
//!            }
//! ```
//!
//! **`fallDistance -= (double)(float)dy` narrows through `float` and back**
//! — `d2f` then `f2d` at offsets 19-20 — while `fallDistance` itself is a
//! `double` field (it was a `float` field before 26.x, and the narrowing is
//! what survives of that). A port that subtracts the `double` directly
//! accumulates a different number over a long fall, and since
//! `calculateFallDamage` ends in `Mth.floor` the difference is a whole heart
//! at the boundaries.
//!
//! **`calculateFallPower` adds `1.0E-6` before subtracting the safe
//! distance.** So a fall of exactly the safe distance yields `+1e-6`, which
//! floors to 0 damage, and a fall one ulp longer still floors to 0 — the
//! epsilon is there to make `fallDistance == safe` land on the safe side of
//! a floating-point comparison, not to add damage. Dropping it changes
//! nothing visible and is still wrong, because `Math.max(0, floor(power))`
//! is also the particle count in `LivingEntity.checkFallDamage`.
//!
//! # What is here and what is not
//!
//! `Block.fallOn`'s **default** is ported: `entity.causeFallDamage(dist,
//! 1.0f, damageSources().fall())`. The overrides are not — hay bales, slime,
//! beds and powder snow each override `fallOn` with their own multiplier or
//! bounce, and none of them is a block this server places or ticks. They are
//! named here rather than left as a silent gap, and
//! [`FallDamage::block_overrides_fall_on`] is the list a caller checks
//! before trusting the number.

use crate::attributes::Attributes;

/// `Attributes.SAFE_FALL_DISTANCE`'s registry id.
pub const SAFE_FALL_DISTANCE: &str = "minecraft:safe_fall_distance";
/// `Attributes.FALL_DAMAGE_MULTIPLIER`'s registry id.
pub const FALL_DAMAGE_MULTIPLIER: &str = "minecraft:fall_damage_multiplier";

/// `Entity.fallDistance`, and the ground flags that decide when it is spent.
///
/// One per player. `Entity.<init>` leaves `fallDistance` at `0.0` and
/// `onGround` at `false`, and both are reproduced by [`Fall::default`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Fall {
    fall_distance: f64,
    on_ground: bool,
    horizontal_collision: bool,
}

/// What `Entity.checkFallDamage` did, for a caller that has to act on it.
///
/// A struct rather than a bare `f64` because "landed and took no damage" and
/// "did not land" are different events — the first plays a block sound and
/// fires `HIT_GROUND`, the second does nothing — and a port that returns
/// `0.0` for both cannot tell them apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landing {
    /// `fallDistance` at the moment `fallOn` was called, which is the
    /// argument `causeFallDamage` receives. Zero when the entity did not
    /// land this tick.
    pub distance: f64,
    /// Whether offsets 25-98's `onGround` branch ran at all.
    pub landed: bool,
    /// Whether `fallOn` was reached — `landed && fallDistance > 0.0`. This
    /// is the `GameEvent.HIT_GROUND` gate as well as the damage one.
    pub hit_ground: bool,
}

impl Fall {
    /// `Entity.fallDistance`.
    #[must_use]
    pub const fn distance(&self) -> f64 {
        self.fall_distance
    }

    /// `Entity.onGround()`.
    #[must_use]
    pub const fn on_ground(&self) -> bool {
        self.on_ground
    }

    /// `Entity.horizontalCollision`.
    #[must_use]
    pub const fn horizontal_collision(&self) -> bool {
        self.horizontal_collision
    }

    /// `Entity.resetFallDistance()` — `fallDistance = 0.0`.
    pub const fn reset(&mut self) {
        self.fall_distance = 0.0;
    }

    /// `Entity.setOnGroundWithMovement(boolean, boolean, Vec3)`.
    ///
    /// The three-argument form, which is the one `handleMovePlayer` calls at
    /// offsets 1146-1162 with the packet's two flags. The two-argument form
    /// reuses the entity's existing `horizontalCollision`, and a caller that
    /// has a packet must not use it: the packet carries its own bit and the
    /// stale field is what a walking-into-a-wall scenario disagrees on.
    ///
    /// `checkSupportingBlock` is not modelled — see
    /// [`crate::step`]'s header for the `mainSupportingBlockPos` gap and its
    /// one consequence, which is the block a step sound and a landing read
    /// on a slab.
    pub const fn set_on_ground_with_movement(&mut self, on_ground: bool, horizontal: bool) {
        self.on_ground = on_ground;
        self.horizontal_collision = horizontal;
    }

    /// `Entity.checkFallDamage(double, boolean, BlockState, BlockPos)`.
    ///
    /// `in_water` is `isInWater()` and `landing_on_air` is
    /// `state.isAir()` at the legacy on-pos — vanilla passes the state
    /// itself, and the only thing `Entity`'s implementation asks of it is
    /// which `Block` to call `fallOn` on. A caller with no block table
    /// passes `landing_on_air = true` and gets `hit_ground = false`, which
    /// is what an entity landing on nothing does.
    pub fn check_fall_damage(&mut self, dy: f64, on_ground: bool, landing_on_air: bool) -> Landing {
        // 0-24. The narrowing is vanilla's `d2f; f2d`.
        if !self.in_water_stub() && dy < 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "`d2f` at offset 19; the narrowing IS the behaviour"
            )]
            let narrowed = f64::from(dy as f32);
            self.fall_distance -= narrowed;
        }
        if !on_ground {
            return Landing {
                distance: 0.0,
                landed: false,
                hit_ground: false,
            };
        }
        // 25-98.
        let distance = self.fall_distance;
        let hit_ground = distance > 0.0 && !landing_on_air;
        self.fall_distance = 0.0;
        Landing {
            distance,
            landed: true,
            hit_ground,
        }
    }

    /// `Entity.isInWater()`, which this module does not track.
    ///
    /// **Always false, and the consequence is stated rather than hidden:** a
    /// player descending through water accumulates fall distance here and
    /// none in vanilla. The fluid state that would answer it is
    /// `Entity.updateFluidHeightAndDoFluidPushing`, which no ported mechanic
    /// runs for a player. Nothing calls this with a swimming player today —
    /// no corpus scenario enters water — and when one does, this is the line
    /// it fails on.
    #[expect(
        clippy::unused_self,
        reason = "a method so that the day it reads real state is a one-line change here"
    )]
    const fn in_water_stub(&self) -> bool {
        false
    }
}

/// `LivingEntity`'s two fall-damage arithmetic methods, over one entity's
/// attribute values.
///
/// The attributes are read from `data/vanilla/attributes.json` rather than
/// written down: `safe_fall_distance` is 3.0 and `fall_damage_multiplier`
/// 1.0 for a player in 26.2, but both are per-entity-type table entries and
/// a constant here would be a second place for them to live. Same argument
/// as [`crate::attributes::MAX_HEALTH`]'s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FallDamage {
    safe_fall_distance: f64,
    fall_damage_multiplier: f64,
}

impl FallDamage {
    /// Read one entity type's two attributes out of the loaded table.
    ///
    /// Returns `None` for a type with no `AttributeSupplier`, which is a
    /// fact about the type rather than a gap — an item or a falling block
    /// legitimately has none, and neither takes fall damage.
    #[must_use]
    pub fn for_type(attrs: &Attributes, entity_type: &str) -> Option<Self> {
        Some(Self {
            safe_fall_distance: attrs.base(entity_type, SAFE_FALL_DISTANCE)?,
            fall_damage_multiplier: attrs.base(entity_type, FALL_DAMAGE_MULTIPLIER)?,
        })
    }

    /// `LivingEntity.calculateFallPower(double)` —
    /// `d + 1.0E-6 - getAttributeValue(SAFE_FALL_DISTANCE)`.
    #[must_use]
    pub fn fall_power(&self, distance: f64) -> f64 {
        distance + 1.0E-6 - self.safe_fall_distance
    }

    /// `LivingEntity.calculateFallDamage(double, float)` —
    /// `Mth.floor(calculateFallPower(d) * (double)multiplier *
    /// getAttributeValue(FALL_DAMAGE_MULTIPLIER))`.
    ///
    /// `multiplier` is `Block.fallOn`'s, which is `1.0f` for every block
    /// that does not override it. The `EntityTypeTags.FALL_DAMAGE_IMMUNE`
    /// early return is not reproduced: no entity this server ticks is in
    /// that tag, and a caller that adds one has to add the tag lookup here.
    #[must_use]
    pub fn damage(&self, distance: f64, multiplier: f32) -> i32 {
        let raw = self.fall_power(distance) * f64::from(multiplier) * self.fall_damage_multiplier;
        mth_floor(raw)
    }

    /// The blocks whose `fallOn` override changes the answer, for a caller
    /// that wants to know whether [`FallDamage::damage`] applies.
    ///
    /// Read from the jar's `fallOn` overrides. None is a block this server
    /// places or ticks; the list exists so that the gap is a name rather
    /// than a surprise.
    #[must_use]
    pub const fn block_overrides_fall_on() -> &'static [&'static str] {
        &[
            "minecraft:hay_block",
            "minecraft:honey_block",
            "minecraft:slime_block",
            "minecraft:powder_snow",
            "minecraft:bed",
            "minecraft:farmland",
            "minecraft:turtle_egg",
            "minecraft:sniffer_egg",
        ]
    }
}

/// `Mth.floor(double)` — `(int)Math.floor(d)` with the cast clamping.
#[expect(
    clippy::cast_possible_truncation,
    reason = "vanilla's own `d2i` after `Math.floor`"
)]
fn mth_floor(d: f64) -> i32 {
    d.floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player() -> FallDamage {
        FallDamage {
            safe_fall_distance: 3.0,
            fall_damage_multiplier: 1.0,
        }
    }

    /// The vanilla table every player knows: three blocks is free, four is
    /// one heart's worth, ten is seven.
    #[test]
    fn the_damage_table_matches_the_one_players_know() {
        let p = player();
        assert_eq!(p.damage(3.0, 1.0), 0, "a three-block fall is free");
        assert_eq!(p.damage(4.0, 1.0), 1);
        assert_eq!(p.damage(10.0, 1.0), 7);
        assert_eq!(p.damage(23.0, 1.0), 20, "twenty hearts");
        // Positive control on the multiplier: hay bales pass 0.2f.
        assert_eq!(p.damage(23.0, 0.2), 4);
    }

    /// The epsilon puts an exact safe-distance fall on the safe side.
    #[test]
    fn the_epsilon_makes_the_boundary_land_safe() {
        let p = player();
        assert!(p.fall_power(3.0) > 0.0, "power is positive by 1e-6");
        assert_eq!(p.damage(3.0, 1.0), 0, "and still floors to no damage");
        // One block over is one damage, so the epsilon has not shifted the
        // table by a whole point.
        assert_eq!(p.damage(4.0, 1.0), 1);
    }

    /// The `double -> float -> double` narrowing, on a delta that has no
    /// exact float. Falling 0.1 a hundred times accumulates a different
    /// number from `0.1 * 100` and from the unnarrowed sum.
    #[test]
    fn the_fall_distance_narrows_through_float() {
        let mut f = Fall::default();
        let mut unnarrowed = 0.0_f64;
        for _ in 0..100 {
            f.check_fall_damage(-0.1, false, true);
            unnarrowed += 0.1;
        }
        assert!(
            (f.distance() - unnarrowed).abs() > 1.0e-9,
            "narrowed {} against unnarrowed {}",
            f.distance(),
            unnarrowed
        );
        // And it agrees with an explicitly narrowed sum, which is the
        // positive control that this is the f32 path and not just noise.
        let narrowed: f64 = -(0..100).map(|_| f64::from(-0.1_f64 as f32)).sum::<f64>();
        assert!((f.distance() - narrowed).abs() < f64::EPSILON);
    }

    /// Landing spends the distance, and rising does not accumulate it.
    #[test]
    fn landing_spends_the_distance_and_rising_accumulates_nothing() {
        let mut f = Fall::default();
        f.check_fall_damage(-1.0, false, true);
        f.check_fall_damage(-2.0, false, true);
        assert!((f.distance() - 3.0).abs() < 1.0e-9);
        // Rising: dy > 0 is not subtracted.
        f.check_fall_damage(0.5, false, true);
        assert!((f.distance() - 3.0).abs() < 1.0e-9);

        let landed = f.check_fall_damage(-0.5, true, false);
        assert!(landed.landed);
        assert!(landed.hit_ground);
        assert!((landed.distance - 3.5).abs() < 1.0e-9);
        assert!((f.distance() - 0.0).abs() < f64::EPSILON, "spent");

        // Landing on air reaches the reset but not `fallOn` — the branch
        // that separates "landed" from "hit something".
        f.check_fall_damage(-4.0, false, true);
        let on_air = f.check_fall_damage(0.0, true, true);
        assert!(on_air.landed);
        assert!(!on_air.hit_ground);
        assert!((f.distance() - 0.0).abs() < f64::EPSILON);
    }

    /// A player who never leaves the ground never accumulates anything, and
    /// a player still in the air never lands. Both are the null control for
    /// the walking scenarios in the corpus.
    #[test]
    fn a_walking_player_neither_accumulates_nor_lands() {
        let mut f = Fall::default();
        for _ in 0..20 {
            let l = f.check_fall_damage(0.0, true, false);
            assert!(l.landed);
            assert!(!l.hit_ground, "nothing to spend");
            assert!((f.distance() - 0.0).abs() < f64::EPSILON);
        }
    }

    /// The two attributes come out of the real table, not out of this file.
    ///
    /// Needs `data/vanilla/attributes.json`, which `ci/gen-gameplay-tables.sh`
    /// produces and which `crates/rustmc-data/src/fixture.rs` makes a PANIC
    /// rather than a skip when it is absent. A tree without the extract fails
    /// here loudly, which is the point: a silently skipped fixture test is
    /// the defect class `docs/AGENT-BRIEF.md` opens with.
    #[test]
    fn the_players_attributes_are_read_from_the_table() {
        let attrs = Attributes::load_default().expect("data/vanilla/attributes.json");
        let p =
            FallDamage::for_type(&attrs, "minecraft:player").expect("the player has a supplier");
        assert!(
            (p.safe_fall_distance - 3.0).abs() < f64::EPSILON,
            "safe_fall_distance from the table, got {}",
            p.safe_fall_distance
        );
        assert!((p.fall_damage_multiplier - 1.0).abs() < f64::EPSILON);
        assert_eq!(p.damage(10.0, 1.0), 7);
        // A type with no supplier is None rather than a default, which is
        // the control that this reads the table and does not invent rows.
        assert!(FallDamage::for_type(&attrs, "minecraft:item").is_none());
    }

    #[test]
    fn the_ground_flags_come_from_the_packet_not_from_the_field() {
        let mut f = Fall::default();
        assert!(!f.on_ground());
        f.set_on_ground_with_movement(true, true);
        assert!(f.on_ground());
        assert!(f.horizontal_collision());
        f.set_on_ground_with_movement(false, false);
        assert!(!f.on_ground());
        assert!(!f.horizontal_collision());
    }
}
