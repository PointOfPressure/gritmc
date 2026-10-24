//! `ServerboundAttackPacket` — `handleAttack`, and exactly as much of
//! `Player.attack` as this server's world can reach.
//!
//! # The packet is its own packet in 26.2, and that is not what it used to be
//!
//! Left-clicking an entity is `minecraft:attack`, **protocol id 1**, one
//! varint and nothing else; right-clicking one is `minecraft:interact`, id 26.
//! They were one packet with an `ActionType` in earlier versions and they are
//! two now. The ids here are not written from that memory — they are
//! [`rustmc_proto::ids::play::sb::ATTACK`], which is checked against Mojang's
//! own `reports/packets.json` where every id in this project is.
//!
//! # What the corpus can observe, and it is one packet
//!
//! `550-attacking-another-player` and `551-attacking-from-out-of-range` both
//! project `["sound", "damage_event", "set_health", "entity_event"]` from the
//! **actor**, and in every recorded vanilla run of this corpus the whole of
//! their content is a single `entity.player.attack.nodamage` at the attacker's
//! own feet. `damage_event` and `set_health` occur zero times in seventeen
//! recordings, because both players are creative and a creative player is
//! invulnerable — `docs/PARITY-ROADMAP.md` §20.5 names that as a substrate
//! gap, not a check gap.
//!
//! So this module reaches the sound and stops. See
//! `docs/cold-start/gameplay-player-actions.md` for the boundary written out.
//!
//! # `handleAttack`'s refusal ladder, from `javap -p -c -constants`
//!
//! ```text
//!    12: hasClientLoaded                       ifeq 29 -> return
//!    19: isSpectator                           ifne 30 -> 29: return
//!    38: ServerLevel.getEntityOrPart(entityId)
//!    54: ifnull 72                                     -> 72: return
//!    58: WorldBorder.isWithinBounds(target.blockPosition())
//!    69: ifne 73                                       -> 72: return
//!    79: getMainHandItem()
//!    88: isWithinAttackRange(mainHand, target.getBoundingBox(), 3.0d)
//!   102: ifne 106                                      -> 105: return
//!   106: mainHand.has(PIERCING_WEAPON)         ifeq 118 -> 117: return
//!   118: target instanceof ItemEntity | ExperienceOrb | self
//!        | AbstractArrow !isAttackable()
//!          -> disconnect("multiplayer.disconnect.invalid_entity_attacked")
//!   190: mainHand.isItemEnabled(enabledFeatures) ifne 203 -> 202: return
//!   203: cannotAttackWithItem(mainHand, 5)      ifeq 217 -> 216: return
//!   217: player.attack(target)
//! ```
//!
//! **The `3.0d` at offset 96 is not the reach**, and this project has already
//! paid once for reading it as one — `ci/checks/gameplay/550-…` carries the
//! correction. It is the `d` argument that
//! `AttackRange.isInRange(LivingEntity, ToDoubleFunction, double)` *adds* to
//! the maximum and *subtracts* from the minimum. See [`AttackRange`].
//!
//! # `Player.attack`'s branch, and where this stops
//!
//! ```text
//!     0: cannotAttack(target)                  ifeq 9  -> return
//!    24: getAttributeValue(ATTACK_DAMAGE)               (1.0 for a player)
//!    81: deflectProjectile(target)             ifeq 88 -> return
//!    89: damage <= 0 && enchantedDamage <= 0            -> 351
//!   118: isSprinting() && strongAttack -> PLAYER_ATTACK_KNOCKBACK
//!   242: target.hurtOrSimulate(source, damage)
//!   249: ifeq 344 ------------------------------------> 345: playServerSideSound(
//!                                                            PLAYER_ATTACK_NODAMAGE)
//!   258..341: knockback, sweep, attackVisualEffects, itemAttackInteraction,
//!             damageStatsAndHearts, causeFoodExhaustion   <- NOT PORTED
//!   352: postPiercingAttack()
//! ```
//!
//! `hurtOrSimulate` reaches `Player.hurtServer`, whose offsets 11-32 are
//! `abilities.invulnerable && !source.is(BYPASSES_INVULNERABILITY) -> false`,
//! and `Abilities.invulnerable` is what creative mode sets. Every player this
//! server has is creative, so the `ifeq 344` above always takes the branch it
//! takes here — **as a consequence of the target's abilities, not as an
//! assumption**, which is why [`Target::invulnerable`] is a field the caller
//! computes rather than a constant this module bakes in. The first survival
//! player makes [`Outcome::Hurt`] reachable, and it is deliberately a variant
//! that produces no sound rather than one that silently produces the wrong
//! one.

use rustmc_proto::varint::read_varint;
use std::io;

/// `Attributes.ENTITY_INTERACTION_RANGE`'s default, from its own
/// `RangedAttribute("attribute.name.entity_interaction_range", 3.0, 0.0, 64.0)`
/// in `Attributes`' static initialiser.
///
/// `Player.createAttributes` does **not** add this one — it arrives through
/// `LivingEntity.createLivingAttributes`, which does — so a reading of the
/// player's own builder alone would find no value and conclude wrongly.
pub const ENTITY_INTERACTION_RANGE: f64 = 3.0;

/// `handleAttack`'s literal `3.0d`, offset 96.
///
/// A padding on both ends of the reach, not the reach. Named so that the one
/// place it is a bare number is beside the sentence that says what it is.
pub const ATTACK_RANGE_PADDING: f64 = 3.0;

/// `EntityTypes.PLAYER`'s `.sized(0.6, 1.8)`.
pub const PLAYER_WIDTH: f64 = 0.6;
/// `EntityTypes.PLAYER`'s `.sized(0.6, 1.8)`.
pub const PLAYER_HEIGHT: f64 = 1.8;

/// `SoundEvents.PLAYER_ATTACK_NODAMAGE`.
pub const ATTACK_NODAMAGE_SOUND: &str = "minecraft:entity.player.attack.nodamage";

/// `net.minecraft.world.phys.AABB`, only as far as this file needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: (f64, f64, f64),
    pub max: (f64, f64, f64),
}

impl Aabb {
    /// `Entity.makeBoundingBox` for a `sized(width, height)` type: the
    /// position is the **feet centre**, so the box is half the width either
    /// side in x and z and the whole height upwards in y.
    #[must_use]
    pub fn of_entity(x: f64, y: f64, z: f64, width: f64, height: f64) -> Self {
        let half = width / 2.0;
        Self {
            min: (x - half, y, z - half),
            max: (x + half, y + height, z + half),
        }
    }

    /// A player's box: [`PLAYER_WIDTH`] by [`PLAYER_HEIGHT`].
    #[must_use]
    pub fn of_player(x: f64, y: f64, z: f64) -> Self {
        Self::of_entity(x, y, z, PLAYER_WIDTH, PLAYER_HEIGHT)
    }

    /// `AABB.distanceToSqr(Vec3)`, offsets 0-70:
    ///
    /// ```text
    ///   d = max(max(minX - v.x, v.x - maxX), 0.0)   per axis
    ///   return d*d + e*e + f*f
    /// ```
    ///
    /// Zero when the point is inside, which is the reason for the outer
    /// `max(_, 0.0)` and the reason an attacker standing *in* the target is
    /// always in range.
    #[must_use]
    pub fn distance_to_sqr(&self, p: (f64, f64, f64)) -> f64 {
        fn axis(min: f64, max: f64, v: f64) -> f64 {
            f64::max(f64::max(min - v, v - max), 0.0)
        }
        let dx = axis(self.min.0, self.max.0, p.0);
        let dy = axis(self.min.1, self.max.1, p.1);
        let dz = axis(self.min.2, self.max.2, p.2);
        dx.mul_add(dx, dy.mul_add(dy, dz * dz))
    }
}

/// `net.minecraft.world.item.component.AttackRange`, a six-float record.
///
/// The fields are in the record's own declaration order, which is the order
/// its canonical constructor takes them in and therefore the only order
/// `defaultFor`'s `(0, r, 0, r, 0, 1)` can be read against.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackRange {
    pub min_reach: f32,
    pub max_reach: f32,
    pub min_creative_reach: f32,
    pub max_creative_reach: f32,
    pub hitbox_margin: f32,
    pub mob_factor: f32,
}

impl AttackRange {
    /// `AttackRange.defaultFor(LivingEntity)`:
    ///
    /// ```text
    ///   float r = living.getAttributeValue(ENTITY_INTERACTION_RANGE);
    ///   return new AttackRange(0, r, 0, r, 0, 1);
    /// ```
    ///
    /// The `d2f` at offset 7 is not decoration: the attribute is a `double`
    /// and every field of this record is a `float`, so the value is narrowed
    /// before it is stored and any arithmetic on it happens in `float`.
    #[must_use]
    pub fn default_for(interaction_range: f64) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "`d2f` at defaultFor offset 7 — the narrowing is vanilla's"
        )]
        let r = interaction_range as f32;
        Self {
            min_reach: 0.0,
            max_reach: r,
            min_creative_reach: 0.0,
            max_creative_reach: r,
            hitbox_margin: 0.0,
            mob_factor: 1.0,
        }
    }

    /// `effectiveMinRange(Entity)`, offsets 0-40.
    ///
    /// The `instanceof Player` arm picks the creative pair; the other arm is
    /// `minReach * mobFactor`. **Only the player arm is modelled**: nothing
    /// but a player can send `handleAttack` a packet, and a mob attacking is
    /// a whole AI this server does not have. The parameter is the game mode
    /// rather than an entity so that the unreachable arm is absent instead of
    /// present and untested.
    #[must_use]
    pub const fn effective_min_range(&self, creative: bool) -> f32 {
        if creative {
            self.min_creative_reach
        } else {
            self.min_reach
        }
    }

    /// `effectiveMaxRange(Entity)`, offsets 0-40. See
    /// [`Self::effective_min_range`].
    #[must_use]
    pub const fn effective_max_range(&self, creative: bool) -> f32 {
        if creative {
            self.max_creative_reach
        } else {
            self.max_reach
        }
    }

    /// `AttackRange.isInRange(LivingEntity, ToDoubleFunction<Vec3>, double)`,
    /// the private three-argument one that both public overloads delegate to:
    ///
    /// ```text
    ///   double dist = sqrt(fn.applyAsDouble(living.getEyePosition()));
    ///   double lo   = (effectiveMinRange(living) - hitboxMargin) - d;
    ///   double hi   = (effectiveMaxRange(living) + hitboxMargin) + d;
    ///   return dist >= lo && dist <= hi;
    /// ```
    ///
    /// `fn` for the AABB overload is `aabb::distanceToSqr`, so the distance
    /// is **eye to bounding box**, not eye to eye and not feet to feet. The
    /// `f2d` widenings sit exactly where vanilla's do: both bounds are
    /// computed in `float` and then widened, and `d` is added in `double`.
    ///
    /// With `defaultFor` and a creative attacker this is
    /// `dist >= -3.0 && dist <= r + 3.0` — six blocks at the default reach,
    /// which is why the corpus's near attack at 3 blocks lands and its far
    /// attack at 20 does not.
    #[must_use]
    pub fn is_in_range(&self, eye: (f64, f64, f64), creative: bool, aabb: &Aabb, d: f64) -> bool {
        let dist = aabb.distance_to_sqr(eye).sqrt();
        let lo = f64::from(self.effective_min_range(creative) - self.hitbox_margin) - d;
        let hi = f64::from(self.effective_max_range(creative) + self.hitbox_margin) + d;
        dist >= lo && dist <= hi
    }
}

/// Why an attack produced nothing.
///
/// Every variant is one `return` in the ladder above, in the order the
/// handler tests them, so a reader can line this enum up against the
/// bytecode without holding a mapping in their head.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// `isSpectator` at offset 19.
    Spectator,
    /// `getEntityOrPart` returned null at offset 54.
    NoSuchEntity,
    /// `isWithinAttackRange` was false at offset 102. **This is the whole of
    /// `551-attacking-from-out-of-range`.**
    OutOfRange,
    /// Offset 118's four cases. Vanilla **disconnects** here rather than
    /// returning — see [`Outcome`].
    InvalidEntityAttacked,
}

/// What an attack did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing, and why.
    ///
    /// [`Refusal::InvalidEntityAttacked`] is folded in here rather than given
    /// a kick of its own: vanilla ends the session at offset 161 and this
    /// server does not, which is a difference no corpus scenario reaches (no
    /// check attacks a dropped stack) and which is named in the cold start
    /// rather than silently smoothed over. Refusing the attack is the
    /// conservative half of vanilla's behaviour — it never produces a packet
    /// vanilla would not.
    Refused(Refusal),
    /// `hurtOrSimulate` returned false, so `playServerSideSound(
    /// PLAYER_ATTACK_NODAMAGE)` at offset 348.
    NoDamageSound,
    /// `hurtOrSimulate` returned true.
    ///
    /// **Unreachable today and deliberately silent.** Everything vanilla does
    /// down that branch — knockback, the sweep, `attackVisualEffects`'
    /// crit/strong/weak/sweep sound, the damage tilt, the hearts, food
    /// exhaustion — is unported. A variant that played the *nodamage* sound
    /// anyway would make `550` pass for a server with no damage model at all
    /// and no way to tell, which is this project's most expensive class of
    /// defect. So it produces nothing and says so.
    Hurt,
}

/// One entity as `handleAttack` needs to see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// The entity id, so `target == player` at offset 132 can be tested.
    pub id: i32,
    /// `target instanceof ItemEntity | ExperienceOrb`, or an `AbstractArrow`
    /// that is not attackable — the three kinds at offset 118 that this
    /// server can actually hold. A dropped stack is the only one it creates.
    pub invalid_to_attack: bool,
    /// `Entity.getBoundingBox()`.
    pub aabb: Aabb,
    /// `Player.hurtServer` offsets 11-32 — `abilities.invulnerable` with no
    /// `BYPASSES_INVULNERABILITY` on the source. True for a creative player.
    pub invulnerable: bool,
}

/// The attacker, as `handleAttack` and `Player.attack` need to see it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Attacker {
    pub entity_id: i32,
    /// `Entity.getEyePosition()` — `(getX(), getY() + eyeHeight, getZ())`.
    pub eye: (f64, f64, f64),
    /// `Player.isCreative()`, which picks the creative pair in
    /// [`AttackRange::effective_max_range`].
    pub creative: bool,
    /// `ServerPlayer.isSpectator()`.
    pub spectator: bool,
}

/// `handleAttack` followed by `Player.attack`, over data rather than over a
/// session, so that every arm above can be a unit test rather than a corpus
/// run.
///
/// The three tests this does **not** perform, each because its answer cannot
/// vary in this server's world, and each named so the omission is a decision
/// rather than an oversight:
///
/// * `hasClientLoaded` — every connection that reaches a packet handler here
///   has finished its join sequence.
/// * `WorldBorder.isWithinBounds` — the default border is ±29,999,984 and the
///   gameplay workspace is inside it by seven orders of magnitude.
/// * `PIERCING_WEAPON` / `isItemEnabled` / `cannotAttackWithItem(stack, 5)` —
///   all three read data components off the held stack, and
///   [`crate::item::ItemStack`] carries an item name and a count and no
///   components at all. `cannotAttackWithItem` in particular is
///   `minCharge > 0 && charge < minCharge` with `minCharge` defaulting to
///   `0.0f`, so it is false for every stack that has no component — which is
///   every stack here.
#[must_use]
pub fn resolve(attacker: &Attacker, target: Option<&Target>) -> Outcome {
    if attacker.spectator {
        return Outcome::Refused(Refusal::Spectator);
    }
    let Some(target) = target else {
        return Outcome::Refused(Refusal::NoSuchEntity);
    };
    let range = AttackRange::default_for(ENTITY_INTERACTION_RANGE);
    if !range.is_in_range(
        attacker.eye,
        attacker.creative,
        &target.aabb,
        ATTACK_RANGE_PADDING,
    ) {
        return Outcome::Refused(Refusal::OutOfRange);
    }
    if target.invalid_to_attack || target.id == attacker.entity_id {
        return Outcome::Refused(Refusal::InvalidEntityAttacked);
    }
    // `Player.attack` offset 0's `cannotAttack(target)` is
    // `!target.isAttackable() || target.skipAttackInteraction(this)`.
    // `Entity.isAttackable()` returns true unless overridden and neither of
    // this server's two entity kinds overrides it; `skipAttackInteraction`
    // is true only for `ItemFrame` and `Painting`, which do not exist here.
    // So it is a branch with no reachable true arm, and it is written down
    // rather than given an enum variant that no input could produce.
    if target.invulnerable {
        Outcome::NoDamageSound
    } else {
        Outcome::Hurt
    }
}

/// `ServerboundAttackPacket`'s body: one varint, the target's entity id.
///
/// # Errors
/// If the varint is truncated.
pub fn decode(body: &[u8]) -> io::Result<i32> {
    let mut cur = body;
    read_varint(&mut cur)
}

// ---------------------------------------------------------------------------
// `Player.attack`'s body, past the point `Outcome::Hurt` used to stop at.
// ---------------------------------------------------------------------------

/// `Attributes.ATTACK_DAMAGE`'s value for a player: `Player.createAttributes`
/// adds it explicitly at `dconst_1`, offsets 3..7.
///
/// The attribute's own `RangedAttribute` default is **2048.0** — the maximum,
/// not the base — so reading the attribute's declaration instead of the
/// player's builder gives a number three orders of magnitude wrong.
pub const PLAYER_ATTACK_DAMAGE: f64 = 1.0;

/// `Attributes.ATTACK_SPEED`'s default, `RangedAttribute("attribute.name
/// .attack_speed", 4.0, 0.0, 1024.0)`.
///
/// `Player.createAttributes` adds it with no value, so the attribute's own
/// default is the player's — the opposite of `ATTACK_DAMAGE` directly above,
/// which is why both are written out rather than one rule being applied to
/// both.
pub const PLAYER_ATTACK_SPEED: f64 = 4.0;

/// `Attributes.SWEEPING_DAMAGE_RATIO`'s default for a player, added with no
/// value in `createAttributes`. Zero, so a bare sweep deals
/// `1 + 0 * damage = 1` to each secondary target.
pub const PLAYER_SWEEPING_DAMAGE_RATIO: f64 = 0.0;

/// `Attributes.ATTACK_KNOCKBACK`'s default. Zero for a player with no weapon,
/// which is what makes `Player.attack`'s `causeExtraKnockback` a no-op at
/// offset 0's `fcmpl; ifle`.
pub const PLAYER_ATTACK_KNOCKBACK: f64 = 0.0;

/// `Player.getAttackStrengthScale(float)`'s `0.5f` argument at `attack`
/// offset 45.
pub const ATTACK_STRENGTH_ADJUST: f32 = 0.5;

/// The threshold `Player.attack` offset 103 compares the strength scale
/// against for `strongAttack`.
pub const STRONG_ATTACK_SCALE: f32 = 0.9;

/// `SoundEvents.PLAYER_ATTACK_STRONG`.
pub const ATTACK_STRONG_SOUND: &str = "minecraft:entity.player.attack.strong";
/// `SoundEvents.PLAYER_ATTACK_WEAK`.
pub const ATTACK_WEAK_SOUND: &str = "minecraft:entity.player.attack.weak";
/// `SoundEvents.PLAYER_ATTACK_CRIT`.
pub const ATTACK_CRIT_SOUND: &str = "minecraft:entity.player.attack.crit";
/// `SoundEvents.PLAYER_ATTACK_KNOCKBACK`.
pub const ATTACK_KNOCKBACK_SOUND: &str = "minecraft:entity.player.attack.knockback";
/// `SoundEvents.PLAYER_ATTACK_SWEEP`.
pub const ATTACK_SWEEP_SOUND: &str = "minecraft:entity.player.attack.sweep";

/// `Player.causeFoodExhaustion(0.1f)` at `attack` offset 335.
pub const ATTACK_EXHAUSTION: f32 = 0.1;

/// `Player.getCurrentItemAttackStrengthDelay()`, offsets 0..14:
/// `(float)(1.0 / getAttributeValue(ATTACK_SPEED) * 20.0)`.
///
/// The whole expression is `double` and narrowed once at the end (`d2f` at
/// offset 13), so computing it in `f32` would round twice.
#[must_use]
pub fn current_item_attack_strength_delay(attack_speed: f64) -> f32 {
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    {
        (1.0 / attack_speed * 20.0) as f32
    }
}

/// `Player.getAttackStrengthScale(float)`, offsets 0..17:
/// `Mth.clamp((attackStrengthTicker + adjust) / delay, 0.0f, 1.0f)`.
///
/// `attackStrengthTicker` is an `int` widened by `i2f` and the sum is in
/// `float`, so the whole quotient is single precision — unlike the delay
/// above, which is computed in `double` and narrowed once.
#[must_use]
pub fn attack_strength_scale(ticker: i32, adjust: f32, attack_speed: f64) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
    let t = ticker as f32;
    crate::javamath::clamp_f(
        (t + adjust) / current_item_attack_strength_delay(attack_speed),
        0.0,
        1.0,
    )
}

/// `Player.baseDamageScaleFactor()`, offsets 0..19:
/// `0.2f + scale * scale * 0.8f` with `scale = getAttackStrengthScale(0.5f)`.
///
/// So a fully charged swing multiplies by exactly **1.0** and an uncharged one
/// by **0.2**, and the curve between them is quadratic rather than linear. A
/// port using the scale directly would deal a fifth too little at half charge.
#[must_use]
pub fn base_damage_scale_factor(scale: f32) -> f32 {
    scale.mul_add(scale * 0.8, 0.2)
}

/// `Player.canCriticalAttack(Entity)`, offsets 0..63.
///
/// Seven conditions, all of which must hold, and the sixth is the one a port
/// forgets: the **target must be a `LivingEntity`**. A critical hit on a
/// dropped stack is not a thing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CritConditions {
    /// `fallDistance > 0.0` — a `double` in 26.2, compared with `dcmpl`.
    pub fall_distance: f64,
    pub on_ground: bool,
    pub on_climbable: bool,
    pub in_water: bool,
    /// `isMobilityRestricted()`.
    pub mobility_restricted: bool,
    pub passenger: bool,
    /// `target instanceof LivingEntity`.
    pub target_is_living: bool,
    pub sprinting: bool,
}

impl CritConditions {
    /// The conjunction, in vanilla's order.
    #[must_use]
    pub fn can_crit(&self) -> bool {
        self.fall_distance > 0.0
            && !self.on_ground
            && !self.on_climbable
            && !self.in_water
            && !self.mobility_restricted
            && !self.passenger
            && self.target_is_living
            && !self.sprinting
    }
}

/// `Player.isSweepAttack(boolean, boolean, boolean)`, offsets 0..65.
///
/// `strong && !crit && !knockbackSprint && onGround && movedLessThan &&
///  mainHand.is(ItemTags.SWORDS)`, where `movedLessThan` is
/// `getKnownMovement().horizontalDistanceSqr() < Mth.square(getSpeed() * 2.5)`.
///
/// The comparison is `dcmpg; ifge`, so **equality does not sweep** and a NaN
/// movement does not either.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepConditions {
    pub strong: bool,
    pub crit: bool,
    /// The `knockbackSprint` local, set only by the sprint arm at offsets
    /// 117..145.
    pub knockback_sprint: bool,
    pub on_ground: bool,
    /// `getKnownMovement().horizontalDistanceSqr()`.
    pub known_movement_horizontal_sqr: f64,
    /// `getSpeed()`, the movement-speed field and **not** the attribute.
    pub speed: f32,
    /// `getItemInHand(MAIN_HAND).is(ItemTags.SWORDS)`.
    pub holding_sword: bool,
}

impl SweepConditions {
    /// The conjunction, in vanilla's order.
    #[must_use]
    pub fn is_sweep(&self) -> bool {
        if !self.strong || self.crit || self.knockback_sprint || !self.on_ground {
            return false;
        }
        let limit = f64::from(self.speed) * 2.5;
        if !matches!(
            self.known_movement_horizontal_sqr
                .partial_cmp(&(limit * limit)),
            Some(std::cmp::Ordering::Less)
        ) {
            return false;
        }
        self.holding_sword
    }
}

/// Everything `Player.attack` needs about the attacker beyond
/// [`Attacker`]'s reach fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Swing {
    /// `Player.attackStrengthTicker`, read **before** `onAttack()` resets it
    /// at offset 76.
    pub attack_strength_ticker: i32,
    /// `getAttributeValue(ATTACK_SPEED)`.
    pub attack_speed: f64,
    /// `getAttributeValue(ATTACK_DAMAGE)`, the `d2f` at offset 30.
    pub attack_damage: f64,
    /// `getAttributeValue(ATTACK_KNOCKBACK)`.
    pub attack_knockback: f64,
    /// `Item.getAttackDamageBonus(target, damage, source)` at offset 154 —
    /// a `MaceItem` smash and nothing else in this jar, so zero for every
    /// stack this server can hold.
    pub item_damage_bonus: f32,
    pub sprinting: bool,
    pub crit: CritConditions,
    pub sweep: SweepConditions,
}

/// The plan one landed melee attack produces.
///
/// Every field is one thing `Player.attack` does after `hurtOrSimulate`
/// returned true, so a caller performs them in field order and a reader can
/// line the struct up against offsets 252..341.
#[derive(Debug, Clone, PartialEq)]
pub struct Melee {
    /// `f` at offset 74 — the damage handed to `hurtOrSimulate`, after the
    /// base scale factor, the item bonus and the 1.5x crit.
    pub damage: f32,
    /// `getAttackStrengthScale(0.5f)`, kept because `doSweepAttack` is handed
    /// it and because it is the one number every branch below reads.
    pub strength_scale: f32,
    /// `strongAttack`, the `scale > 0.9f` local at offset 115.
    pub strong: bool,
    /// The `knockbackSprint` local. **Sprinting alone is not enough**: the
    /// arm at 117 requires `isSprinting() && strongAttack`.
    pub knockback_sprint: bool,
    /// `canCriticalAttack` and `strongAttack`.
    pub crit: bool,
    /// `isSweepAttack(strong, crit, knockbackSprint)`.
    pub sweep: bool,
    /// `getKnockback(target, source) + (knockbackSprint ? 0.5f : 0.0f)`,
    /// which `causeExtraKnockback` uses only when it is **strictly positive**.
    pub extra_knockback: f32,
    /// The sounds, in the order `Player.attack` plays them.
    ///
    /// This is a list rather than four booleans because the order is the
    /// observable thing: `PLAYER_ATTACK_KNOCKBACK` at offset 133 comes
    /// **before** `hurtOrSimulate`, and therefore before the target's own hurt
    /// sound; the crit, strong/weak and sweep sounds all come after it.
    pub sounds: Vec<&'static str>,
    /// `causeFoodExhaustion(0.1f)` at offset 335.
    pub exhaustion: f32,
}

/// `Player.attack`'s arithmetic and branch, given a target that is not
/// invulnerable.
///
/// `sounds` is split around the `hurtOrSimulate` call by the caller: every
/// element **before** the first `None` entry is played before the hit and the
/// rest after it. That is not a shape this returns, because vanilla plays
/// exactly one sound before the hit and it is always the sprint knockback —
/// so [`Melee::knockback_sprint`] says whether `sounds[0]` is the early one.
///
/// `getEnchantedDamage` is `LivingEntity`'s own `freturn` of its argument
/// (offsets 0..1) unless an enchantment overrides it, so the `f2` local at
/// `attack` offset 66 — `scale * (enchanted - base)` — is **zero** for every
/// stack this server can hold, and with it goes `magicCrit` at offset 54.
/// Named rather than dropped: it is the one input `attackVisualEffects` takes
/// that nothing else in this file computes.
#[must_use]
pub fn melee(swing: &Swing) -> Melee {
    // 44..51 — read BEFORE `onAttack()` resets the ticker at offset 76.
    let scale = attack_strength_scale(
        swing.attack_strength_ticker,
        ATTACK_STRENGTH_ADJUST,
        swing.attack_speed,
    );
    // 23..31 — `(float) getAttributeValue(ATTACK_DAMAGE)`.
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let mut damage = swing.attack_damage as f32;
    // 68..74 — the base scale multiplies the ATTRIBUTE damage, before the
    // item bonus is added. Reversing those two changes the answer whenever
    // the swing is not fully charged.
    damage *= base_damage_scale_factor(scale);
    // 101..115.
    let strong = scale > STRONG_ATTACK_SCALE;
    // 117..145 — sprinting AND strong. A sprint-hit at low charge plays no
    // sound and gets no extra knockback.
    let knockback_sprint = swing.sprinting && strong;
    let mut sounds = Vec::new();
    if knockback_sprint {
        sounds.push(ATTACK_KNOCKBACK_SOUND);
    }
    // 145..158.
    damage += swing.item_damage_bonus;
    // 159..189.
    let crit = strong && swing.crit.can_crit();
    if crit {
        damage *= 1.5;
    }
    // 196..206.
    let sweep = SweepConditions {
        strong,
        crit,
        knockback_sprint,
        ..swing.sweep
    }
    .is_sweep();

    // 252..283 — `getKnockback` is `getAttributeValue(ATTACK_KNOCKBACK) / 2`,
    // and the `/ 2` is easy to miss: it is `fconst_2; fdiv` on both arms of
    // the `instanceof ServerLevel` test, so it applies whether or not an
    // enchantment modified the value.
    #[expect(clippy::cast_possible_truncation, reason = "vanilla's own d2f")]
    let base_knockback = (swing.attack_knockback / 2.0) as f32;
    let extra_knockback = base_knockback + if knockback_sprint { 0.5 } else { 0.0 };

    // 284..298 — the sweep sound is played first inside `doSweepAttack`,
    // before the secondary targets are hurt.
    if sweep {
        sounds.push(ATTACK_SWEEP_SOUND);
    }
    // 299..312 — `attackVisualEffects(target, crit, sweep, strong, false,
    // enchantedDamage)`. **The fifth argument is `iconst_0` at offset 307,
    // not `knockbackSprint`** (`iload 9; iload 11; iload 7; iconst_0; fload
    // 6`), and it is the one the method's strong/weak gate reads at its own
    // offset 24 (`iload 5`). So the gate is `!crit && !sweep && !false`: a
    // sprinting strong hit plays the knockback sound before the hit AND the
    // strong sound after it. This read `!knockback_sprint` until 2026-09-30.
    if crit {
        sounds.push(ATTACK_CRIT_SOUND);
    }
    if !crit && !sweep {
        sounds.push(if strong {
            ATTACK_STRONG_SOUND
        } else {
            ATTACK_WEAK_SOUND
        });
    }

    Melee {
        damage,
        strength_scale: scale,
        strong,
        knockback_sprint,
        crit,
        sweep,
        extra_knockback,
        sounds,
        exhaustion: ATTACK_EXHAUSTION,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The near attack of `550` and `551`: the actor stands at relative
    /// `(0, 0, 3)` and the target is at the workspace origin. Eye to box is
    /// `3 - 0.3 = 2.7`, comfortably inside `0 + 3.0 + 3.0`.
    #[test]
    fn three_blocks_is_within_reach() {
        let attacker = Attacker {
            entity_id: 1,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, 3.0),
            creative: true,
            spectator: false,
        };
        let target = Target {
            id: 2,
            invalid_to_attack: false,
            aabb: Aabb::of_player(0.0, 0.0, 0.0),
            invulnerable: true,
        };
        assert_eq!(resolve(&attacker, Some(&target)), Outcome::NoDamageSound);
    }

    /// The far attack of `551`: twenty blocks out, which no reading of the
    /// attribute plus its padding reaches. **This is the check.** A server
    /// that acted on the packet the moment it arrived would answer here and
    /// be byte-identical to vanilla on `550` while being wrong.
    #[test]
    fn twenty_blocks_is_refused() {
        let attacker = Attacker {
            entity_id: 1,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, 20.0),
            creative: true,
            spectator: false,
        };
        let target = Target {
            id: 2,
            invalid_to_attack: false,
            aabb: Aabb::of_player(0.0, 0.0, 0.0),
            invulnerable: true,
        };
        assert_eq!(
            resolve(&attacker, Some(&target)),
            Outcome::Refused(Refusal::OutOfRange)
        );
    }

    /// The bound is `maxCreativeReach + hitboxMargin + d` = `3 + 0 + 3`, and
    /// the distance is measured to the **box**, so the last position that
    /// lands is 6 blocks from the box's `z = 0.3` face — `z = 6.3`.
    ///
    /// Pinned on both sides of the edge because a reach read as "3 blocks"
    /// (the `AttackRange` maximum alone) or as "3 blocks" (the handler's
    /// literal alone) both fail here, and both were plausible readings.
    #[test]
    fn the_reach_is_the_attribute_plus_the_padding() {
        let at = |z: f64| Attacker {
            entity_id: 1,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, z),
            creative: true,
            spectator: false,
        };
        let target = Target {
            id: 2,
            invalid_to_attack: false,
            aabb: Aabb::of_player(0.0, 0.0, 0.0),
            invulnerable: true,
        };
        assert_eq!(resolve(&at(6.29), Some(&target)), Outcome::NoDamageSound);
        assert_eq!(
            resolve(&at(6.31), Some(&target)),
            Outcome::Refused(Refusal::OutOfRange)
        );
    }

    /// `distanceToSqr`'s outer `max(_, 0.0)` per axis: a point inside the box
    /// is at distance zero, not at a negative one.
    #[test]
    fn a_point_inside_the_box_is_at_zero() {
        let b = Aabb::of_player(0.0, 0.0, 0.0);
        assert!((b.distance_to_sqr((0.0, 1.0, 0.0)) - 0.0).abs() < f64::EPSILON);
        // And the y axis is not ignored: the box is 1.8 tall from the feet,
        // so a point three blocks above it is three blocks away.
        assert!((b.distance_to_sqr((0.0, 4.8, 0.0)) - 9.0).abs() < 1e-9);
    }

    /// A vulnerable target does **not** get the nodamage sound. The damage
    /// path is unported and this is the assertion that keeps it from being
    /// silently faked.
    #[test]
    fn a_vulnerable_target_takes_the_unported_branch() {
        let attacker = Attacker {
            entity_id: 1,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, 3.0),
            creative: true,
            spectator: false,
        };
        let target = Target {
            id: 2,
            invalid_to_attack: false,
            aabb: Aabb::of_player(0.0, 0.0, 0.0),
            invulnerable: false,
        };
        assert_eq!(resolve(&attacker, Some(&target)), Outcome::Hurt);
    }

    /// `getEntityOrPart` returning null is a `return`, not a kick.
    #[test]
    fn an_unknown_entity_is_refused_quietly() {
        let attacker = Attacker {
            entity_id: 1,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, 3.0),
            creative: true,
            spectator: false,
        };
        assert_eq!(
            resolve(&attacker, None),
            Outcome::Refused(Refusal::NoSuchEntity)
        );
    }

    /// Offset 132: attacking yourself is one of the four cases at 118.
    #[test]
    fn attacking_yourself_is_invalid() {
        let attacker = Attacker {
            entity_id: 7,
            eye: (0.0, crate::dropitem::PLAYER_EYE_HEIGHT, 0.0),
            creative: true,
            spectator: false,
        };
        let target = Target {
            id: 7,
            invalid_to_attack: false,
            aabb: Aabb::of_player(0.0, 0.0, 0.0),
            invulnerable: true,
        };
        assert_eq!(
            resolve(&attacker, Some(&target)),
            Outcome::Refused(Refusal::InvalidEntityAttacked)
        );
    }

    /// `defaultFor` is `(0, r, 0, r, 0, 1)` in the record's declaration
    /// order, and reading that tuple in any other order silently produces a
    /// different reach. Pinned because the order is not recoverable from the
    /// field names at the call site.
    #[test]
    fn default_for_fills_the_record_in_declaration_order() {
        let r = AttackRange::default_for(ENTITY_INTERACTION_RANGE);
        assert_eq!(
            r,
            AttackRange {
                min_reach: 0.0,
                max_reach: 3.0,
                min_creative_reach: 0.0,
                max_creative_reach: 3.0,
                hitbox_margin: 0.0,
                mob_factor: 1.0,
            }
        );
    }

    /// One varint and nothing else.
    #[test]
    fn the_body_is_one_varint() {
        assert_eq!(decode(&[0x2a]).expect("decoded"), 42);
        assert_eq!(decode(&[0x80, 0x01]).expect("decoded"), 128);
    }
}

#[cfg(test)]
mod melee_tests {
    use super::*;

    fn still_bare_handed(ticker: i32) -> Swing {
        Swing {
            attack_strength_ticker: ticker,
            attack_speed: PLAYER_ATTACK_SPEED,
            attack_damage: PLAYER_ATTACK_DAMAGE,
            attack_knockback: PLAYER_ATTACK_KNOCKBACK,
            item_damage_bonus: 0.0,
            sprinting: false,
            crit: CritConditions {
                fall_distance: 0.0,
                on_ground: true,
                on_climbable: false,
                in_water: false,
                mobility_restricted: false,
                passenger: false,
                target_is_living: true,
                sprinting: false,
            },
            sweep: SweepConditions {
                strong: false,
                crit: false,
                knockback_sprint: false,
                on_ground: true,
                known_movement_horizontal_sqr: 0.0,
                speed: 0.1,
                holding_sword: false,
            },
        }
    }

    /// The cooldown is five ticks at the player's own `ATTACK_SPEED` of 4.0,
    /// and the scale is `(ticker + 0.5) / 5` clamped.
    ///
    /// The `0.5` adjust is why a swing is fully charged after **five** ticks
    /// rather than after five and a half: at ticker 5 the quotient is 1.1,
    /// which clamps. At ticker 4 it is 0.9 — **not** greater than 0.9, so a
    /// four-tick swing is not a strong one. That boundary is the whole reason
    /// the comparison is written `>` and not `>=`.
    #[test]
    fn the_cooldown_is_five_ticks_and_four_is_not_yet_strong() {
        assert_eq!(current_item_attack_strength_delay(PLAYER_ATTACK_SPEED), 5.0);
        assert_eq!(attack_strength_scale(0, 0.5, PLAYER_ATTACK_SPEED), 0.1);
        assert_eq!(attack_strength_scale(4, 0.5, PLAYER_ATTACK_SPEED), 0.9);
        assert_eq!(attack_strength_scale(5, 0.5, PLAYER_ATTACK_SPEED), 1.0);
        assert_eq!(attack_strength_scale(500, 0.5, PLAYER_ATTACK_SPEED), 1.0);

        assert!(!melee(&still_bare_handed(4)).strong, "0.9 is not > 0.9");
        assert!(melee(&still_bare_handed(5)).strong);
    }

    /// The curve is quadratic, not linear: `0.2 + s*s*0.8`. At half charge a
    /// linear reading would give 0.5 and vanilla gives 0.4.
    #[test]
    fn the_damage_scale_is_quadratic_and_a_linear_reading_disagrees_at_half() {
        assert_eq!(base_damage_scale_factor(0.0), 0.2);
        assert_eq!(base_damage_scale_factor(1.0), 1.0);
        let half = base_damage_scale_factor(0.5);
        assert!((half - 0.4).abs() < 1e-6, "{half}");
        assert!(
            (half - 0.5).abs() > 1e-3,
            "a linear reading would give 0.5 here"
        );
    }

    /// A rested bare-handed hit on a pig is exactly **1.0** damage — the
    /// `ATTACK_DAMAGE` attribute at full charge, with no item bonus and no
    /// crit — and it plays `entity.player.attack.strong` and nothing else.
    ///
    /// This is the corpus case, so the number is asserted rather than
    /// described.
    #[test]
    fn a_rested_bare_handed_hit_is_one_damage_and_one_strong_sound() {
        let m = melee(&still_bare_handed(20));
        assert_eq!(m.damage, 1.0);
        assert!(m.strong);
        assert!(!m.crit && !m.sweep && !m.knockback_sprint);
        assert_eq!(m.sounds, vec![ATTACK_STRONG_SOUND]);
        assert_eq!(m.extra_knockback, 0.0, "bare hand: ATTACK_KNOCKBACK is 0");
        assert_eq!(m.exhaustion, ATTACK_EXHAUSTION);
    }

    /// The same hit at zero charge and the WEAK sound. Without this half the
    /// test above cannot tell a strong swing from a swing.
    ///
    /// **The floor is 0.208, not 0.2**, and the difference is the `0.5f`
    /// adjust: `getAttackStrengthScale(0.5f)` at `attackStrengthTicker = 0` is
    /// `0.5 / 5 = 0.1`, not zero, so `baseDamageScaleFactor` is
    /// `0.2 + 0.01 * 0.8 = 0.208`. A player's damage never reaches the 0.2 the
    /// curve's constant term suggests, because the scale can never be 0 —
    /// vanilla's `0.2f` is a limit the game does not take.
    #[test]
    fn an_uncharged_hit_is_a_fifth_of_the_damage_and_the_weak_sound() {
        let m = melee(&still_bare_handed(0));
        assert!((m.damage - 0.208).abs() < 1e-6, "{}", m.damage);
        assert!(m.damage > 0.2, "the 0.5 adjust puts the floor above 0.2");
        assert!(!m.strong);
        assert_eq!(m.sounds, vec![ATTACK_WEAK_SOUND]);
    }

    /// Sprinting alone does not make a knockback hit: offset 124's `iload 7;
    /// ifeq 142` requires `strongAttack` as well.
    ///
    /// And the knockback sound comes **first**, before the target is hurt, so
    /// it precedes the mob's own hurt sound on the wire.
    #[test]
    fn the_sprint_knockback_needs_a_strong_swing_too_and_its_sound_comes_first() {
        let mut weak_sprint = still_bare_handed(0);
        weak_sprint.sprinting = true;
        let m = melee(&weak_sprint);
        assert!(!m.knockback_sprint, "sprinting at 0.1 charge is not enough");
        assert_eq!(m.sounds, vec![ATTACK_WEAK_SOUND]);

        let mut strong_sprint = still_bare_handed(20);
        strong_sprint.sprinting = true;
        let m = melee(&strong_sprint);
        assert!(m.knockback_sprint);
        assert_eq!(
            m.sounds,
            vec![ATTACK_KNOCKBACK_SOUND, ATTACK_STRONG_SOUND],
            "attackVisualEffects is handed iconst_0, not knockbackSprint: the strong \
             sound follows the knockback one"
        );
        assert_eq!(m.extra_knockback, 0.5);
    }

    /// A crit multiplies by 1.5 **after** the item bonus is added, and needs a
    /// strong swing. Each of the seven conditions is flipped in turn and each
    /// one alone kills the crit.
    #[test]
    fn every_one_of_the_crit_conditions_alone_prevents_it() {
        let mut falling = still_bare_handed(20);
        falling.crit.fall_distance = 1.0;
        falling.crit.on_ground = false;
        let m = melee(&falling);
        assert!(m.crit);
        assert!((m.damage - 1.5).abs() < 1e-6, "{}", m.damage);
        assert_eq!(m.sounds, vec![ATTACK_CRIT_SOUND]);

        type Flip = (&'static str, fn(&mut Swing));
        let flips: [Flip; 7] = [
            ("no fall distance", |s| s.crit.fall_distance = 0.0),
            ("on ground", |s| s.crit.on_ground = true),
            ("climbing", |s| s.crit.on_climbable = true),
            ("in water", |s| s.crit.in_water = true),
            ("mobility restricted", |s| s.crit.mobility_restricted = true),
            ("riding", |s| s.crit.passenger = true),
            ("target not living", |s| s.crit.target_is_living = false),
        ];
        for (why, flip) in flips {
            let mut s = falling;
            flip(&mut s);
            assert!(!melee(&s).crit, "{why} must prevent the crit");
        }
        // The eighth: sprinting is on `CritConditions` and not read from
        // `Swing.sprinting`, because vanilla reads `isSprinting()` inside
        // `canCriticalAttack` as its own call.
        let mut s = falling;
        s.crit.sprinting = true;
        assert!(!melee(&s).crit, "sprinting must prevent the crit");
        // And a weak swing, which is the gate outside canCriticalAttack.
        let mut s = falling;
        s.attack_strength_ticker = 0;
        assert!(!melee(&s).crit, "an uncharged swing cannot crit");
    }

    /// The sweep's movement gate is `<`, strictly, against the SQUARE of
    /// `speed * 2.5`. At exactly the limit it does not sweep.
    #[test]
    fn the_sweep_gate_is_strict_and_against_the_squared_limit() {
        let mut s = still_bare_handed(20);
        s.sweep.holding_sword = true;
        let limit = f64::from(s.sweep.speed) * 2.5;

        s.sweep.known_movement_horizontal_sqr = limit * limit;
        assert!(!melee(&s).sweep, "equality does not sweep");

        s.sweep.known_movement_horizontal_sqr = limit * limit - 1e-9;
        let m = melee(&s);
        assert!(m.sweep);
        assert_eq!(
            m.sounds,
            vec![ATTACK_SWEEP_SOUND],
            "a sweep suppresses the strong/weak pair"
        );

        // A port comparing the UNsquared movement against the unsquared limit
        // would sweep here, where vanilla does not: 0.0625 > 0.0625 is false
        // but the squares differ.
        s.sweep.known_movement_horizontal_sqr = f64::from(s.sweep.speed) * 2.5 * 0.9;
        assert!(
            !melee(&s).sweep,
            "0.225 is above the squared limit of 0.0625 and must not sweep"
        );

        // No sword, no sweep, whatever the movement.
        s.sweep.holding_sword = false;
        s.sweep.known_movement_horizontal_sqr = 0.0;
        assert!(!melee(&s).sweep);
    }

    /// A crit and a sweep are mutually exclusive by construction —
    /// `isSweepAttack` takes `crit` and returns false for it — so the two
    /// sounds can never both be played.
    #[test]
    fn a_crit_and_a_sweep_never_happen_together() {
        let mut s = still_bare_handed(20);
        s.sweep.holding_sword = true;
        s.crit.fall_distance = 1.0;
        s.crit.on_ground = false;
        s.sweep.on_ground = true;
        let m = melee(&s);
        assert!(m.crit);
        assert!(!m.sweep);
        assert_eq!(m.sounds, vec![ATTACK_CRIT_SOUND]);
    }

    /// `getKnockback` halves the attribute on both arms of its
    /// `instanceof ServerLevel` test — `fconst_2; fdiv` at offsets 42 and 46 —
    /// so a weapon with `ATTACK_KNOCKBACK` 2.0 contributes 1.0, not 2.0.
    #[test]
    fn the_attack_knockback_attribute_is_halved() {
        let mut s = still_bare_handed(20);
        s.attack_knockback = 2.0;
        assert_eq!(melee(&s).extra_knockback, 1.0);
        s.sprinting = true;
        assert_eq!(
            melee(&s).extra_knockback,
            1.5,
            "the sprint 0.5 is added after"
        );
    }
}
