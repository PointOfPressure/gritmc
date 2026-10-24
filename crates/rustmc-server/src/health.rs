//! `LivingEntity`'s health, absorption, and the two absorption stages a
//! damage figure passes through before it reaches either.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.entity.LivingEntity` — `getHealth`, `setHealth`,
//! `heal`, `isDeadOrDying`, `getMaxHealth`, `getMaxAbsorption`,
//! `getAbsorptionAmount`, `setAbsorptionAmount`, `internalSetAbsorptionAmount`,
//! `getDamageAfterArmorAbsorb`, `getDamageAfterMagicAbsorb`, `actuallyHurt`,
//! `isInvulnerableTo` — and `net.minecraft.world.entity.player.Player.isHurt`.
//!
//! # What main had before this file
//!
//! No health of any kind. `docs/SERVER.md` describes the player as "a
//! position, a hotbar, a creative slot, swing, the click surface", and
//! `crate::player::PlayerState` is exactly that. `set_health` has been a
//! decoded channel in `tools/oracle-runner` for as long as the recorder has
//! existed and no scenario could ever make this server emit one.
//!
//! # Health is clamped on the way in, not on the way out
//!
//! `setHealth` is `Mth.clamp(value, 0.0f, getMaxHealth())`, and **every**
//! write goes through it — `heal`, `actuallyHurt`, the `/heal`-shaped paths.
//! So a health field is never out of range and nothing downstream re-clamps.
//! `heal` additionally does nothing at all when current health is already
//! `<= 0`: a dead entity cannot be healed back, which is the check that stops
//! a regeneration effect resurrecting a corpse mid-death-tick.
//!
//! `getMaxHealth` is an **attribute read** — `Attributes.MAX_HEALTH` through
//! `getAttributeValue`, narrowed `d2f` — and so is `getMaxAbsorption`. They
//! are parameters here rather than constants for the reason
//! `crate::attributes` exists: `20.0` is a default in
//! `data/vanilla/attributes.json`, not a number in this file.
//!
//! # The absorption arithmetic in `actuallyHurt` subtracts twice
//!
//! Offsets 27..51 spend the absorption shield:
//!
//! ```text
//!   f4       = damage                                (before absorption)
//!   damage   = max(damage - absorption, 0.0)         (what reaches health)
//!   absorption = absorption - (f4 - damage)          (the shield spent)
//! ```
//!
//! and then, **after** the health write, offsets 137..144 do
//! `setAbsorptionAmount(getAbsorptionAmount() - damage)` — subtracting the
//! *health* damage from the shield as well. That second subtraction is
//! vanilla's and it is not a no-op: it drives the shield negative and
//! `setAbsorptionAmount` clamps it to 0. It is transcribed rather than
//! tidied, and [`ActuallyHurt::absorption_double_subtracted`] records that a
//! caller who "fixes" it has changed behaviour.
//!
//! # Everything this file cannot reach is a named input, never a default
//!
//! `getDamageAfterArmorAbsorb` calls `hurtArmor`; `getDamageAfterMagicAbsorb`
//! reads a Resistance effect instance and calls
//! `EnchantmentHelper.getDamageProtection`; `isInvulnerableTo` composes
//! `isInvulnerableToBase` with `EnchantmentHelper.isImmuneToDamage`;
//! `actuallyHurt` records into a `CombatTracker` and awards three statistics.
//! None of those exists on this server. Each is a field of [`HurtContext`] or
//! an explicit `_` in the transcription, so that when one of them is built the
//! grep that finds every site is `HurtContext`.

use crate::combat_rules::{self, ArmorEffectiveness};
use crate::javamath;

/// `Float.MAX_VALUE`, the sentinel `hurtServer` substitutes for a NaN or
/// infinite damage, and the ceiling the statistics guards compare against —
/// note those compare against `3.4028235E37`, a **tenth** of it, which is a
/// separate constant in the bytecode and not a typo here.
pub const FLOAT_MAX_VALUE: f32 = f32::MAX;

/// `3.4028235E37f` — the bound the `awardStat` guards in `actuallyHurt` and
/// `getDamageAfterMagicAbsorb` use, one order of magnitude below
/// `Float.MAX_VALUE`. Vanilla uses it to keep the `* 10.0f` in the statistic
/// from overflowing.
pub const STAT_DAMAGE_CEILING: f32 = 3.402_823_5e37;

/// What the damage type says about itself, as tag membership.
///
/// Each field is one `DamageSource.is(DamageTypeTags.X)` call on the paths
/// transcribed here. They are booleans rather than a tag set because the
/// damage-type registry is a table (`data/vanilla/damage_types.json`) and this
/// module is the behaviour that reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DamageTags {
    /// `DamageTypeTags.BYPASSES_ARMOR` — skips the whole armour stage.
    pub bypasses_armor: bool,
    /// `DamageTypeTags.BYPASSES_EFFECTS` — skips Resistance *and* Protection.
    pub bypasses_effects: bool,
    /// `DamageTypeTags.BYPASSES_RESISTANCE` — skips Resistance only.
    pub bypasses_resistance: bool,
    /// `DamageTypeTags.BYPASSES_ENCHANTMENTS` — skips Protection only.
    pub bypasses_enchantments: bool,
    /// `DamageTypeTags.BYPASSES_COOLDOWN` — ignores `invulnerableTime`.
    /// Read by `crate::hurt`, not by this module.
    pub bypasses_cooldown: bool,
    /// `DamageTypeTags.IS_FIRE` — the tag Fire Resistance immunises against.
    pub is_fire: bool,
    /// `DamageTypeTags.IS_FREEZING` — quintupled against
    /// `EntityTypeTags.FREEZE_HURTS_EXTRA_TYPES`.
    pub is_freezing: bool,
    /// `DamageTypeTags.DAMAGES_HELMET` — costs a quarter of the damage when
    /// a helmet is worn.
    pub damages_helmet: bool,
    /// `DamageTypeTags.NO_IMPACT` — suppresses `markHurt`.
    pub no_impact: bool,
    /// `DamageTypeTags.NO_KNOCKBACK` — suppresses `dealDefaultKnockback`.
    pub no_knockback: bool,
}

/// The parts of the hurt path this server has no model for.
///
/// Every field is a hook vanilla calls that would need machinery that does not
/// exist here. Defaults are the *absent* case and not a guess: no armour, no
/// toughness, no Resistance, no Protection, not invulnerable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HurtContext {
    /// `isInvulnerableTo(level, source)` — `isInvulnerableToBase` composed
    /// with `EnchantmentHelper.isImmuneToDamage`.
    pub invulnerable: bool,
    /// `getArmorValue()`, as the `i2f` at `getDamageAfterArmorAbsorb`
    /// offset 23 widens it.
    pub armor: f32,
    /// `getAttributeValue(Attributes.ARMOR_TOUGHNESS)`, narrowed `d2f`.
    pub armor_toughness: f32,
    /// `EnchantmentHelper.modifyArmorEffectiveness`, or its absence.
    pub armor_effectiveness: ArmorEffectiveness,
    /// `getEffect(MobEffects.RESISTANCE).getAmplifier()`, or `None` when
    /// `hasEffect(RESISTANCE)` is false. Amplifier 0 is Resistance I.
    pub resistance_amplifier: Option<i32>,
    /// `EnchantmentHelper.getDamageProtection(level, this, source)`; `0.0`
    /// both when there are no enchantments and when the level is not a
    /// `ServerLevel`, which are the same branch in vanilla.
    pub damage_protection: f32,
}

impl Default for HurtContext {
    fn default() -> Self {
        Self {
            invulnerable: false,
            armor: 0.0,
            armor_toughness: 0.0,
            armor_effectiveness: ArmorEffectiveness::Unmodified,
            resistance_amplifier: None,
            damage_protection: 0.0,
        }
    }
}

/// What one `actuallyHurt` did, for a caller that has to broadcast it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActuallyHurt {
    /// The damage that reached health, after both absorb stages and the
    /// absorption shield. Zero means vanilla returned at offset 117 and
    /// emitted no `ENTITY_DAMAGE` game event.
    pub dealt: f32,
    /// `f5` — how much of the hit the absorption shield swallowed.
    pub absorbed: f32,
    /// True when vanilla ran the second, post-health
    /// `setAbsorptionAmount(getAbsorptionAmount() - damage)` at offsets
    /// 137..144. It runs whenever `dealt != 0.0`, and it is recorded because
    /// it is the line a reimplementation deletes as redundant.
    pub absorption_double_subtracted: bool,
    /// `gameEvent(GameEvent.ENTITY_DAMAGE)` fired.
    pub game_event: bool,
}

/// `LivingEntity`'s health and absorption, and the fields the hurt cooldown
/// needs.
///
/// `dead` is `LivingEntity.dead`; `hurtTime`/`hurtDuration`/`invulnerableTime`
/// /`lastHurt` are the four the hurt cooldown reads and writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Health {
    health: f32,
    absorption: f32,
    max_health: f32,
    max_absorption: f32,
    /// `Entity.invulnerableTime`.
    pub invulnerable_time: i32,
    /// `LivingEntity.hurtTime`.
    pub hurt_time: i32,
    /// `LivingEntity.hurtDuration`.
    pub hurt_duration: i32,
    /// `LivingEntity.lastHurt`.
    pub last_hurt: f32,
    /// `LivingEntity.dead`.
    pub dead: bool,
    /// `LivingEntity.noActionTime`.
    pub no_action_time: i32,
}

impl Health {
    /// A living entity at full health, with the two attribute-derived maxima
    /// supplied by the caller.
    ///
    /// `LivingEntity.<init>` calls `setHealth(getMaxHealth())`, so the initial
    /// value is the attribute and never a literal.
    #[must_use]
    pub fn full(max_health: f32, max_absorption: f32) -> Self {
        let mut h = Self {
            health: 0.0,
            absorption: 0.0,
            max_health,
            max_absorption,
            invulnerable_time: 0,
            hurt_time: 0,
            hurt_duration: 0,
            last_hurt: 0.0,
            dead: false,
            no_action_time: 0,
        };
        h.set_health(max_health);
        h
    }

    /// `getHealth()` — the `DATA_HEALTH_ID` synched float.
    #[must_use]
    pub const fn health(&self) -> f32 {
        self.health
    }

    /// `setHealth(float)` — `Mth.clamp(value, 0.0f, getMaxHealth())`.
    pub fn set_health(&mut self, value: f32) {
        self.health = javamath::clamp_f(value, 0.0, self.max_health);
    }

    /// `getMaxHealth()` — `getAttributeValue(Attributes.MAX_HEALTH)`, `d2f`.
    #[must_use]
    pub const fn max_health(&self) -> f32 {
        self.max_health
    }

    /// Re-read the `MAX_HEALTH` attribute. Vanilla has no setter — the value
    /// simply follows the attribute map — so this exists for the caller that
    /// owns the map. It does **not** re-clamp the current health, and neither
    /// does vanilla: health above a freshly lowered maximum survives until the
    /// next `setHealth`.
    pub const fn set_max_health(&mut self, value: f32) {
        self.max_health = value;
    }

    /// `getMaxAbsorption()` — `getAttributeValue(Attributes.MAX_ABSORPTION)`.
    #[must_use]
    pub const fn max_absorption(&self) -> f32 {
        self.max_absorption
    }

    /// Re-read the `MAX_ABSORPTION` attribute; see [`Health::set_max_health`].
    pub const fn set_max_absorption(&mut self, value: f32) {
        self.max_absorption = value;
    }

    /// `getAbsorptionAmount()`.
    #[must_use]
    pub const fn absorption(&self) -> f32 {
        self.absorption
    }

    /// `setAbsorptionAmount(float)` — `internalSetAbsorptionAmount(clamp(v,
    /// 0.0f, getMaxAbsorption()))`. `final` in vanilla, so the clamp is not
    /// overridable and every write is clamped.
    pub fn set_absorption(&mut self, value: f32) {
        self.absorption = javamath::clamp_f(value, 0.0, self.max_absorption);
    }

    /// `heal(float)` — a no-op at or below zero health.
    pub fn heal(&mut self, amount: f32) {
        let current = self.health;
        if current > 0.0 {
            self.set_health(current + amount);
        }
    }

    /// `isDeadOrDying()` — `getHealth() <= 0.0f || this.dead`.
    ///
    /// `fcmpg; ifle` on the health, so a NaN health reads as **dead**.
    #[must_use]
    pub fn is_dead_or_dying(&self) -> bool {
        // `fcmpg; ifle` — "not greater than zero", which is TRUE for NaN.
        // Written through `partial_cmp` rather than as `!(h > 0.0)` because
        // clippy refuses the negated comparison on a partially ordered type,
        // and because spelling it out is the only way the NaN arm is visible.
        !matches!(
            self.health.partial_cmp(&0.0),
            Some(std::cmp::Ordering::Greater)
        ) || self.dead
    }

    /// `Player.isHurt()` — `getHealth() > 0.0f && getHealth() < getMaxHealth()`.
    ///
    /// The gate on `FoodData`'s two healing arms. Note it is *not*
    /// `!isDeadOrDying() && needsHealing`: a dead player is not "hurt" here
    /// and neither is a full one.
    #[must_use]
    pub fn is_hurt(&self) -> bool {
        self.health > 0.0 && self.health < self.max_health
    }

    /// `LivingEntity.getDamageAfterArmorAbsorb(DamageSource, float)`.
    ///
    /// `hurtArmor(source, damage)` runs on the non-bypassing branch and is not
    /// modelled: it damages the four armour pieces, and this server has no
    /// armour slots. Its absence changes nothing about the returned figure.
    #[must_use]
    pub fn damage_after_armor_absorb(damage: f32, tags: DamageTags, ctx: HurtContext) -> f32 {
        if tags.bypasses_armor {
            return damage;
        }
        // hurtArmor(source, damage) — no armour slots on this server.
        combat_rules::damage_after_absorb(
            damage,
            ctx.armor,
            ctx.armor_toughness,
            ctx.armor_effectiveness,
        )
    }

    /// `LivingEntity.getDamageAfterMagicAbsorb(DamageSource, float)`.
    ///
    /// Three stages, in vanilla's order: the `BYPASSES_EFFECTS` early return,
    /// the Resistance reduction, and the Protection reduction — with a
    /// `damage <= 0` early return of **0.0** between the second and the third.
    #[must_use]
    pub fn damage_after_magic_absorb(damage: f32, tags: DamageTags, ctx: HurtContext) -> f32 {
        if tags.bypasses_effects {
            return damage;
        }
        let mut damage = damage;
        if let Some(amplifier) = ctx.resistance_amplifier {
            if !tags.bypasses_resistance {
                // i = (amplifier + 1) * 5;  j = 25 - i;
                // damage = max(damage * (float) j / 25.0f, 0.0f)
                let i = (amplifier + 1) * 5;
                let j = 25 - i;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "vanilla's own i2f at getDamageAfterMagicAbsorb offset 56"
                )]
                let scaled = damage * j as f32;
                damage = javamath::max_f(scaled / 25.0, 0.0);
                // Stats.DAMAGE_RESISTED / DAMAGE_DEALT_RESISTED are awarded
                // here from `f2 - damage`; no statistics on this server.
            }
        }
        // 166: `fcmpg; ifgt 174` — anything not strictly positive, NaN
        // included, returns a hard 0.0 rather than falling through. Spelled
        // through `partial_cmp` so the NaN arm is explicit; `!(d > 0.0)` is
        // the same predicate and clippy refuses it.
        if !matches!(damage.partial_cmp(&0.0), Some(std::cmp::Ordering::Greater)) {
            return 0.0;
        }
        if tags.bypasses_enchantments {
            return damage;
        }
        let protection = ctx.damage_protection;
        if protection > 0.0 {
            damage = combat_rules::damage_after_magic_absorb(damage, protection);
        }
        damage
    }

    /// `LivingEntity.actuallyHurt(ServerLevel, DamageSource, float)`.
    ///
    /// Returns what was done rather than performing the broadcast, for the
    /// reason `crate::food` returns a [`crate::food::FoodAction`]: the game
    /// event and the combat tracker belong to the level, not to the health.
    pub fn actually_hurt(
        &mut self,
        damage: f32,
        tags: DamageTags,
        ctx: HurtContext,
    ) -> ActuallyHurt {
        if ctx.invulnerable {
            return ActuallyHurt::default();
        }
        let mut damage = Self::damage_after_armor_absorb(damage, tags, ctx);
        damage = Self::damage_after_magic_absorb(damage, tags, ctx);

        // 24..51 — spend the shield.
        let before_absorption = damage;
        damage = javamath::max_f(damage - self.absorption, 0.0);
        self.set_absorption(self.absorption - (before_absorption - damage));
        let absorbed = before_absorption - damage;
        // Stats.DAMAGE_DEALT_ABSORBED, guarded by `0 < absorbed <
        // STAT_DAMAGE_CEILING`, is awarded here. No statistics here.
        let _ = STAT_DAMAGE_CEILING;

        // 111..117 — an exact zero returns before the health write, so no
        // game event and no combat-tracker record.
        if damage == 0.0 {
            return ActuallyHurt {
                dealt: 0.0,
                absorbed,
                absorption_double_subtracted: false,
                game_event: false,
            };
        }

        // getCombatTracker().recordDamage(source, damage) — no combat tracker.
        self.set_health(self.health - damage);
        // 137..144 — the second subtraction. See the module docs.
        self.set_absorption(self.absorption - damage);
        ActuallyHurt {
            dealt: damage,
            absorbed,
            absorption_double_subtracted: true,
            game_event: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DamageTags, Health, HurtContext};

    fn player() -> Health {
        // 20.0 is `minecraft:generic.max_health`'s default in
        // data/vanilla/attributes.json; it is supplied here rather than
        // written into the module for exactly that reason.
        Health::full(20.0, 0.0)
    }

    #[test]
    fn a_fresh_entity_starts_at_its_attribute_maximum() {
        assert!((player().health() - 20.0).abs() < f32::EPSILON);
        assert!(!player().is_hurt());
        assert!(!player().is_dead_or_dying());
    }

    #[test]
    fn set_health_clamps_both_ways() {
        let mut h = player();
        h.set_health(1000.0);
        assert!((h.health() - 20.0).abs() < f32::EPSILON);
        h.set_health(-5.0);
        assert!(h.health().abs() < f32::EPSILON);
        assert!(h.is_dead_or_dying());
    }

    #[test]
    fn heal_does_nothing_at_zero_health() {
        let mut h = player();
        h.set_health(0.0);
        h.heal(10.0);
        assert!(h.health().abs() < f32::EPSILON);
    }

    #[test]
    fn heal_is_clamped_to_the_maximum() {
        let mut h = player();
        h.set_health(19.5);
        h.heal(10.0);
        assert!((h.health() - 20.0).abs() < f32::EPSILON);
    }

    #[test]
    fn is_hurt_is_false_at_full_and_at_zero() {
        let mut h = player();
        assert!(!h.is_hurt());
        h.set_health(0.0);
        assert!(!h.is_hurt());
        h.set_health(19.0);
        assert!(h.is_hurt());
    }

    #[test]
    fn absorption_is_clamped_to_the_max_absorption_attribute() {
        let mut h = Health::full(20.0, 4.0);
        h.set_absorption(100.0);
        assert!((h.absorption() - 4.0).abs() < f32::EPSILON);
        h.set_absorption(-1.0);
        assert!(h.absorption().abs() < f32::EPSILON);
    }

    #[test]
    fn plain_damage_reaches_health_intact() {
        let mut h = player();
        let out = h.actually_hurt(6.0, DamageTags::default(), HurtContext::default());
        assert!((out.dealt - 6.0).abs() < f32::EPSILON);
        assert!((h.health() - 14.0).abs() < f32::EPSILON);
        assert!(out.game_event);
    }

    #[test]
    fn an_invulnerable_entity_takes_nothing_and_emits_nothing() {
        let mut h = player();
        let ctx = HurtContext {
            invulnerable: true,
            ..HurtContext::default()
        };
        let out = h.actually_hurt(6.0, DamageTags::default(), ctx);
        assert!((h.health() - 20.0).abs() < f32::EPSILON);
        assert!(!out.game_event);
    }

    #[test]
    fn armour_reduces_and_bypasses_armor_does_not() {
        let ctx = HurtContext {
            armor: 20.0,
            ..HurtContext::default()
        };
        let with = Health::damage_after_armor_absorb(10.0, DamageTags::default(), ctx);
        let bypass = Health::damage_after_armor_absorb(
            10.0,
            DamageTags {
                bypasses_armor: true,
                ..DamageTags::default()
            },
            ctx,
        );
        assert!(with < 10.0, "{with}");
        assert!((bypass - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn resistance_takes_a_fifth_per_level_and_four_levels_takes_everything() {
        let tags = DamageTags::default();
        for (amplifier, expected) in [(0, 8.0_f32), (1, 6.0), (2, 4.0), (3, 2.0)] {
            let ctx = HurtContext {
                resistance_amplifier: Some(amplifier),
                ..HurtContext::default()
            };
            let d = Health::damage_after_magic_absorb(10.0, tags, ctx);
            assert!((d - expected).abs() < 1e-5, "amp {amplifier}: {d}");
        }
        let ctx = HurtContext {
            resistance_amplifier: Some(4),
            ..HurtContext::default()
        };
        assert!(Health::damage_after_magic_absorb(10.0, tags, ctx).abs() < f32::EPSILON);
    }

    #[test]
    fn resistance_five_does_not_heal_the_target() {
        // j = 25 - 30 = -5, so the product is negative and the max(_, 0.0)
        // is what stops a Resistance V hit from adding health.
        let ctx = HurtContext {
            resistance_amplifier: Some(5),
            ..HurtContext::default()
        };
        let d = Health::damage_after_magic_absorb(10.0, DamageTags::default(), ctx);
        assert!(d.abs() < f32::EPSILON, "{d}");
    }

    #[test]
    fn the_two_bypass_tags_skip_different_stages() {
        let ctx = HurtContext {
            resistance_amplifier: Some(3),
            damage_protection: 10.0,
            ..HurtContext::default()
        };
        let both = Health::damage_after_magic_absorb(10.0, DamageTags::default(), ctx);
        let no_effects = Health::damage_after_magic_absorb(
            10.0,
            DamageTags {
                bypasses_effects: true,
                ..DamageTags::default()
            },
            ctx,
        );
        let no_resistance = Health::damage_after_magic_absorb(
            10.0,
            DamageTags {
                bypasses_resistance: true,
                ..DamageTags::default()
            },
            ctx,
        );
        let no_enchantments = Health::damage_after_magic_absorb(
            10.0,
            DamageTags {
                bypasses_enchantments: true,
                ..DamageTags::default()
            },
            ctx,
        );
        // BYPASSES_EFFECTS is the whole method; the other two are one stage
        // each, and all four figures differ.
        assert!((no_effects - 10.0).abs() < f32::EPSILON, "{no_effects}");
        assert!(both < no_resistance, "{both} {no_resistance}");
        assert!(both < no_enchantments, "{both} {no_enchantments}");
        assert!(no_resistance < no_effects && no_enchantments < no_effects);
    }

    #[test]
    fn absorption_swallows_the_hit_and_is_spent() {
        let mut h = Health::full(20.0, 10.0);
        h.set_absorption(4.0);
        let out = h.actually_hurt(3.0, DamageTags::default(), HurtContext::default());
        assert!(out.dealt.abs() < f32::EPSILON, "{}", out.dealt);
        assert!((out.absorbed - 3.0).abs() < f32::EPSILON);
        assert!((h.health() - 20.0).abs() < f32::EPSILON);
        assert!((h.absorption() - 1.0).abs() < f32::EPSILON);
        // A fully absorbed hit returns before the health write, so no game
        // event and no second subtraction.
        assert!(!out.game_event);
        assert!(!out.absorption_double_subtracted);
    }

    #[test]
    fn a_hit_that_outruns_the_shield_spends_it_twice() {
        // 10 damage against a 4-point shield: 6 reaches health, and the
        // shield is then reduced by 6 again from an already-zero value —
        // clamped back to 0. The observable claim is that health took
        // exactly 6 and the shield is empty.
        let mut h = Health::full(20.0, 10.0);
        h.set_absorption(4.0);
        let out = h.actually_hurt(10.0, DamageTags::default(), HurtContext::default());
        assert!((out.dealt - 6.0).abs() < f32::EPSILON);
        assert!((out.absorbed - 4.0).abs() < f32::EPSILON);
        assert!((h.health() - 14.0).abs() < f32::EPSILON);
        assert!(h.absorption().abs() < f32::EPSILON);
        assert!(out.absorption_double_subtracted);
    }

    /// The positive control. Three misreadings of the magic-absorb ladder and
    /// the absorption arithmetic, each written out and shown to disagree.
    #[test]
    fn each_misreading_of_the_hurt_path_is_separable() {
        let tags = DamageTags::default();

        // 1. The Resistance reduction written as `damage * (1 - 0.2 * (amp+1))`
        //    — algebraically the same, but vanilla divides an integer-scaled
        //    product by 25.0f and the float rounding differs. The separable
        //    claim is the clamp, not the rounding: at amplifier 5 the naive
        //    form goes NEGATIVE and vanilla floors at zero.
        let ctx5 = HurtContext {
            resistance_amplifier: Some(5),
            ..HurtContext::default()
        };
        let real = Health::damage_after_magic_absorb(10.0, tags, ctx5);
        let unclamped = 10.0_f32 * (25.0 - 30.0) / 25.0;
        assert!(unclamped < 0.0);
        assert_ne!(real, unclamped);

        // 2. The `damage <= 0` gate read as "fall through" rather than
        //    "return 0.0". With Resistance IV the damage is exactly 0 and a
        //    fall-through would then run the Protection stage on it — which
        //    is harmless here, so the observable form is the NaN case: `!(d >
        //    0.0)` catches NaN and `d <= 0.0` does not.
        let ctx_nan = HurtContext {
            resistance_amplifier: None,
            damage_protection: 10.0,
            ..HurtContext::default()
        };
        let real_nan = Health::damage_after_magic_absorb(f32::NAN, tags, ctx_nan);
        assert!(real_nan.abs() < f32::EPSILON, "{real_nan}");
        let fallthrough = crate::combat_rules::damage_after_magic_absorb(f32::NAN, 10.0);
        assert!(fallthrough.is_nan());

        // 3. The armour stage skipped entirely. 20 points of armour against
        //    a 10-point hit must not reach health intact.
        let ctx_armor = HurtContext {
            armor: 20.0,
            ..HurtContext::default()
        };
        let armoured = Health::damage_after_armor_absorb(10.0, tags, ctx_armor);
        assert_ne!(armoured, 10.0);
    }

    /// A null, recorded so nobody re-runs it: the second, post-health
    /// `setAbsorptionAmount(getAbsorptionAmount() - damage)` at `actuallyHurt`
    /// offsets 137..144 **cannot be observed through the absorption value**.
    ///
    /// By the time it runs the shield has already been reduced by exactly what
    /// it swallowed, so it is either already zero (a hit that outran it) or
    /// the hit reached no health at all and the line never runs. Either way
    /// `setAbsorptionAmount`'s clamp to `[0, maxAbsorption]` lands on the same
    /// number as a single-subtraction implementation.
    ///
    /// It is transcribed anyway, and [`super::ActuallyHurt`] records that it
    /// ran, because "unobservable through this field today" is not
    /// "unobservable": a `MAX_ABSORPTION` modifier or a listener on the
    /// setter would see the difference, and deleting the line would be a
    /// silent change of behaviour with no test able to catch it.
    #[test]
    fn the_second_absorption_subtraction_is_not_observable_through_absorption() {
        let tags = DamageTags::default();
        for (damage, shield) in [(10.0_f32, 4.0_f32), (3.0, 4.0), (4.0, 4.0), (100.0, 10.0)] {
            let mut once = Health::full(20.0, 10.0);
            once.set_absorption(shield);
            let before = damage;
            let dealt = crate::javamath::max_f(before - once.absorption(), 0.0);
            once.set_absorption(once.absorption() - (before - dealt));
            if dealt != 0.0 {
                once.set_health(once.health() - dealt);
            }

            let mut twice = Health::full(20.0, 10.0);
            twice.set_absorption(shield);
            twice.actually_hurt(damage, tags, HurtContext::default());

            assert!(
                (once.absorption() - twice.absorption()).abs() < f32::EPSILON,
                "damage {damage} shield {shield}: {} vs {}",
                once.absorption(),
                twice.absorption()
            );
            assert!((once.health() - twice.health()).abs() < f32::EPSILON);
        }
    }
}
