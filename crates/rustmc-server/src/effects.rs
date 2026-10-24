//! `MobEffectInstance` and `MobEffect` — how an active effect ticks down, when
//! it fires, and what happens when a second copy of it is applied.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.world.effect.MobEffectInstance` (`tickServer`,
//! `hasRemainingDuration`, `tickDownDuration`, `mapDuration`,
//! `downgradeToHiddenEffect`, `isShorterDurationThan`, `update`,
//! `isInfiniteDuration`, `endsWithin`, `withScaledDuration`, and its three
//! `public static final int`s), `net.minecraft.world.effect.MobEffect`
//! (`shouldApplyEffectTickThisTick`, `applyEffectTick`, `isInstantaneous`) and
//! its eleven subclasses that override either — `AbsorptionMobEffect`,
//! `BadOmenMobEffect`, `HealOrHarmMobEffect`, `HungerMobEffect`,
//! `InstantaneousMobEffect`, `PoisonMobEffect`, `RaidOmenMobEffect`,
//! `RegenerationMobEffect`, `SaturationMobEffect`, `WitherMobEffect`.
//!
//! # What main had before this file
//!
//! Nothing. `docs/SERVER.md`'s assessment census counts **40 status effects
//! (11 named)** and **46 potions (7 named)** — those are counts of what the
//! *registry* declares, taken from the jar, not of anything this server does.
//! No effect could be applied, held or ticked, and `update_mob_effect` is not
//! even a decoded channel in `tools/oracle-runner`.
//!
//! # The census is 40 effects and exactly **10** of them ever tick
//!
//! `MobEffect.shouldApplyEffectTickThisTick` returns **`false`** in the base
//! class, so an effect that does not override it never reaches
//! `applyEffectTick` at all — its whole contribution is its attribute
//! modifiers and the fact of being present.
//!
//! The 40 effects are 14 distinct classes, and **five of those fourteen never
//! fire**: `MobEffect` itself (26 effects) plus `InfestedMobEffect`,
//! `OozingMobEffect`, `WeavingMobEffect` and `WindChargedMobEffect`, which
//! override `onMobRemoved` rather than the tick. That leaves 10 effects across
//! 9 classes, carrying five distinct rules:
//!
//! ```text
//!   MobEffect              false          26 effects
//!   Infested/Oozing/Weaving/WindCharged   false, 1 effect each
//!   AbsorptionMobEffect    true
//!   BadOmenMobEffect       true
//!   HungerMobEffect        true
//!   HealOrHarmMobEffect    duration >= 1  (extends InstantaneousMobEffect)
//!   SaturationMobEffect    duration >= 1  (likewise)
//!   RaidOmenMobEffect      duration == 1
//!   PoisonMobEffect        i = 25 >> amp; i > 0 ? d % i == 0 : true
//!   RegenerationMobEffect  i = 50 >> amp; i > 0 ? d % i == 0 : true
//!   WitherMobEffect        i = 40 >> amp; i > 0 ? d % i == 0 : true
//! ```
//!
//! **Those counts are measured, not read.** They come from
//! `data/vanilla/mob_effects.json`, which probes every effect in the registry
//! over durations 0..=120 and amplifiers 0..=6 plus 32, and
//! `crates/rustmc-server/tests/mob_effects_table.rs` checks [`TickRule`]
//! against that probe cell by cell. This paragraph said "eight of them ever
//! tick" before the probe existed and the probe said 10.
//!
//! **The `i > 0` fallback is not decoration.** `25 >> 5` is 0, and at that
//! amplifier the effect fires *every tick* rather than never — the arm at
//! offset 21 of each of the three returns `true`. Poison V and up therefore
//! deals a point of damage twenty times a second, and an implementation that
//! wrote `d % (25 >> amp) == 0` would divide by zero instead.
//!
//! # The countdown runs on the *duration*, and the duration is the phase
//!
//! `tickServer` reads `this.duration` **before** `tickDownDuration()`, so the
//! first tick of a 900-tick Regeneration sees `d = 900` and `900 % 50 == 0`
//! fires immediately. The effect fires on the tick it is applied, not fifty
//! ticks later, and the phase of every later fire is fixed by the initial
//! duration rather than by a counter.
//!
//! An **infinite** effect (duration `-1`) substitutes the entity's `tickCount`
//! for the duration, so its cadence is the world's clock and not its own.
//!
//! # `applyEffectTick` returning false ends the effect
//!
//! `tickServer` offsets 53..78: if the rule fires and `applyEffectTick`
//! returns `false`, `tickServer` returns `false` **without ticking the
//! duration down** — the effect is removed. Only two effects use it, and both
//! use it as "I have done my job": `BadOmenMobEffect` when it has converted
//! itself into a Raid Omen, `RaidOmenMobEffect` when it has started the raid.
//!
//! # `update` is where two potions of the same kind meet
//!
//! Vanilla does not add a second instance; it merges into the first, and the
//! loser is stashed in a `hiddenEffect` chain that is restored when the winner
//! runs out. A stronger-but-shorter effect pushes the weaker one down the
//! chain and takes over; a weaker one is only kept if it is *longer*, and then
//! only as a hidden entry. `downgradeToHiddenEffect` pops the chain when the
//! duration hits exactly zero.

use crate::javamath;

/// Where the effect registry extract lives, relative to the server's working
/// directory.
///
/// **This literal is load-bearing outside Rust.** `ci/gen-gameplay-tables.sh`
/// derives the set of tables a tree needs by grepping `crates`, `tools` and
/// `plugins` for `"data/vanilla/<name>.json"`. Spelling it any other way makes
/// the gate stop regenerating it and it goes stale in every tree at once.
pub const TABLE_PATH: &str = "data/vanilla/mob_effects.json";

/// `MobEffectInstance.INFINITE_DURATION` — `-1`.
pub const INFINITE_DURATION: i32 = -1;
/// `MobEffectInstance.MIN_AMPLIFIER` — `0`.
pub const MIN_AMPLIFIER: i32 = 0;
/// `MobEffectInstance.MAX_AMPLIFIER` — `255`.
pub const MAX_AMPLIFIER: i32 = 255;

/// `MobEffect.shouldApplyEffectTickThisTick(int, int)`, as the closed set of
/// answers the jar's ten overriding classes give.
///
/// This is an enum rather than a boolean-returning trait object because the
/// set really is closed: `ci/mob-effects-golden.sh` probes every effect in the
/// registry over a grid of durations and amplifiers, and
/// `crates/rustmc-server/tests/mob_effects_table.rs` checks this enum against
/// that probe, cell by cell, rather than against a second reading of the same
/// nine classes by the same pair of eyes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickRule {
    /// `MobEffect` — the base. Thirty-odd of the forty.
    Never,
    /// `AbsorptionMobEffect`, `BadOmenMobEffect`, `HungerMobEffect`.
    Always,
    /// `InstantaneousMobEffect` — `duration >= 1`.
    AtLeastOne,
    /// `RaidOmenMobEffect` — `duration == 1`, on the last tick only.
    ExactlyOne,
    /// `PoisonMobEffect` (25), `RegenerationMobEffect` (50),
    /// `WitherMobEffect` (40). The base is shifted right by the amplifier and
    /// a shift to zero means *every* tick.
    Periodic {
        /// The pre-shift period.
        base: i32,
    },
}

impl TickRule {
    /// `shouldApplyEffectTickThisTick(duration, amplifier)`.
    #[must_use]
    pub const fn fires(self, duration: i32, amplifier: i32) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::AtLeastOne => duration >= 1,
            Self::ExactlyOne => duration == 1,
            Self::Periodic { base } => {
                // `bipush base; iload_2; ishr` — a Java `int` shift masks the
                // count to its low five bits, so `25 >> 32` is 25 and not 0.
                // An amplifier that large is reachable: MAX_AMPLIFIER is 255.
                let i = base >> (amplifier & 31);
                if i > 0 {
                    duration % i == 0
                } else {
                    true
                }
            }
        }
    }
}

/// The vanilla `MobEffect` subclass a registry entry is an instance of, mapped
/// to its rule.
///
/// The key is the `class` column of `data/vanilla/mob_effects.json`, which is
/// the JVM's own `getSimpleName()`. Returning `None` for an unknown name is
/// deliberate: a class added in a later version must surface as a refusal
/// rather than fall back to [`TickRule::Never`] and be silently inert, which
/// is exactly what the majority answer would hide.
#[must_use]
pub fn rule_for_class(class: &str) -> Option<TickRule> {
    Some(match class {
        // The base, and the four that override `onMobRemoved` instead of the
        // tick. All five answer false to every (duration, amplifier).
        "MobEffect"
        | "InfestedMobEffect"
        | "OozingMobEffect"
        | "WeavingMobEffect"
        | "WindChargedMobEffect" => TickRule::Never,
        "AbsorptionMobEffect" | "BadOmenMobEffect" | "HungerMobEffect" => TickRule::Always,
        // Both extend `InstantaneousMobEffect`, whose rule is `duration >= 1`.
        "HealOrHarmMobEffect" | "SaturationMobEffect" | "InstantaneousMobEffect" => {
            TickRule::AtLeastOne
        }
        "RaidOmenMobEffect" => TickRule::ExactlyOne,
        "PoisonMobEffect" => TickRule::Periodic { base: 25 },
        "WitherMobEffect" => TickRule::Periodic { base: 40 },
        "RegenerationMobEffect" => TickRule::Periodic { base: 50 },
        _ => return None,
    })
}

/// What `MobEffect.applyEffectTick` did, for the effects that have one.
///
/// The boolean vanilla returns is `keep`: `false` removes the effect. It is
/// carried here alongside the action so a caller cannot apply one without
/// honouring the other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectTick {
    /// `MobEffect.applyEffectTick` — the base, `return true`. No action.
    Nothing,
    /// `RegenerationMobEffect` — `heal(1.0f)`, but only when
    /// `getHealth() < getMaxHealth()`.
    Regenerate,
    /// `PoisonMobEffect` — `hurtServer(magic, 1.0f)`, but only when
    /// `getHealth() > 1.0f`. Poison alone cannot kill.
    Poison,
    /// `WitherMobEffect` — `hurtServer(wither, 1.0f)`, unconditionally. Wither
    /// **can** kill, and that asymmetry with poison is the whole difference
    /// between the two classes' `applyEffectTick`.
    Wither,
    /// `HungerMobEffect` — `causeFoodExhaustion(0.005f * (amplifier + 1))`.
    Hunger {
        /// The exhaustion to add.
        exhaustion: f32,
    },
    /// `SaturationMobEffect` — `getFoodData().eat(amplifier + 1, 1.0f)`, the
    /// *modifier* overload, so the saturation added is
    /// `(amplifier + 1) * 1.0f * 2.0f`.
    Saturate {
        /// The nutrition argument.
        nutrition: i32,
    },
    /// `AbsorptionMobEffect` — no action; returns `getAbsorptionAmount() >
    /// 0.0f`, so the effect ends the moment the shield is gone.
    AbsorptionCheck,
    /// `BadOmenMobEffect` / `RaidOmenMobEffect` — raid machinery this server
    /// has none of. Both return `true` (keep) on every path this server can
    /// reach, because every path that returns `false` needs a village or a
    /// raid.
    RaidMachinery,
}

/// `HungerMobEffect`'s per-tick exhaustion, `ldc 0.005f`. The same number as
/// `FoodConstants.EXHAUSTION_MINE` and a separate literal in the bytecode.
pub const HUNGER_EXHAUSTION_PER_LEVEL: f32 = 0.005;

/// `HealOrHarmMobEffect.applyInstantaneousEffect`'s two multipliers, `4 << amp`
/// for the heal and `6 << amp` for the harm.
pub const HEAL_SHIFT_BASE: i32 = 4;
/// See [`HEAL_SHIFT_BASE`].
pub const HARM_SHIFT_BASE: i32 = 6;

/// `HealOrHarmMobEffect.applyInstantaneousEffect(level, source, indirect,
/// target, amplifier, health)`.
///
/// Returns the integer amount, which vanilla computes in **double**
/// arithmetic and then narrows with `d2i` — a truncation toward zero, not a
/// round — after adding `0.5`. The `+ 0.5` and the truncation together are a
/// round-half-up, and writing `round()` instead differs on the negatives that
/// a negative `health` multiplier can produce.
#[must_use]
pub fn heal_or_harm_amount(harm: bool, amplifier: i32, health_multiplier: f64) -> i32 {
    let base = if harm {
        HARM_SHIFT_BASE
    } else {
        HEAL_SHIFT_BASE
    };
    let scaled = health_multiplier * f64::from(base << (amplifier & 31)) + 0.5;
    // `d2i` — truncate toward zero, saturating at the i32 bounds and mapping
    // NaN to 0, which is exactly what the JVM instruction does.
    if scaled.is_nan() {
        0
    } else if scaled >= f64::from(i32::MAX) {
        i32::MAX
    } else if scaled <= f64::from(i32::MIN) {
        i32::MIN
    } else {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the JVM's own d2i, with its saturating bounds handled above"
        )]
        {
            scaled as i32
        }
    }
}

/// `net.minecraft.world.effect.MobEffectInstance`.
///
/// The `blendState` field is client-side rendering and is not carried; every
/// other field is here, `hiddenEffect` included, because the hidden chain is
/// observable through the duration a client is shown.
#[derive(Debug, Clone, PartialEq)]
pub struct MobEffectInstance {
    /// The effect's registry id, e.g. `minecraft:regeneration`.
    pub effect: String,
    duration: i32,
    amplifier: i32,
    ambient: bool,
    visible: bool,
    show_icon: bool,
    hidden_effect: Option<Box<MobEffectInstance>>,
}

impl MobEffectInstance {
    /// The six-argument constructor, minus the hidden effect.
    ///
    /// The shorter constructors are `(effect)` -> duration 0, amplifier 0;
    /// `(effect, duration)` -> amplifier 0; `(effect, duration, amplifier)` ->
    /// `ambient = false, visible = true, showIcon = visible`.
    #[must_use]
    pub fn new(effect: impl Into<String>, duration: i32, amplifier: i32) -> Self {
        Self {
            effect: effect.into(),
            duration,
            amplifier,
            ambient: false,
            visible: true,
            show_icon: true,
            hidden_effect: None,
        }
    }

    /// The full form.
    #[must_use]
    pub fn with_flags(
        effect: impl Into<String>,
        duration: i32,
        amplifier: i32,
        ambient: bool,
        visible: bool,
        show_icon: bool,
    ) -> Self {
        Self {
            effect: effect.into(),
            duration,
            amplifier,
            ambient,
            visible,
            show_icon,
            hidden_effect: None,
        }
    }

    /// `getDuration()`.
    #[must_use]
    pub const fn duration(&self) -> i32 {
        self.duration
    }

    /// `getAmplifier()`.
    #[must_use]
    pub const fn amplifier(&self) -> i32 {
        self.amplifier
    }

    /// `isAmbient()`.
    #[must_use]
    pub const fn is_ambient(&self) -> bool {
        self.ambient
    }

    /// `isVisible()`.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// `showIcon()`.
    #[must_use]
    pub const fn show_icon(&self) -> bool {
        self.show_icon
    }

    /// The `hiddenEffect` chain, for a caller that has to serialise it.
    #[must_use]
    pub fn hidden(&self) -> Option<&Self> {
        self.hidden_effect.as_deref()
    }

    /// `isInfiniteDuration()` — `duration == -1`, and **only** `-1`. A
    /// duration of `-2` is not infinite and not remaining either, so it is a
    /// dead effect.
    #[must_use]
    pub const fn is_infinite_duration(&self) -> bool {
        self.duration == INFINITE_DURATION
    }

    /// `hasRemainingDuration()` — `isInfiniteDuration() || duration > 0`.
    #[must_use]
    pub const fn has_remaining_duration(&self) -> bool {
        self.is_infinite_duration() || self.duration > 0
    }

    /// `endsWithin(int)` — `!isInfiniteDuration() && duration <= ticks`.
    #[must_use]
    pub const fn ends_within(&self, ticks: i32) -> bool {
        !self.is_infinite_duration() && self.duration <= ticks
    }

    /// `withScaledDuration(float)` — `max(floor(duration * scale), 1)`,
    /// leaving an infinite or zero duration alone.
    #[must_use]
    pub fn with_scaled_duration(&self, scale: f32) -> Self {
        let mut copy = self.clone();
        copy.duration = self.map_duration(|d| {
            #[expect(
                clippy::cast_precision_loss,
                reason = "vanilla's own i2f at lambda$withScaledDuration$0 offset 0"
            )]
            let scaled = d as f32 * scale;
            javamath::max_i(mth_floor(scaled), 1)
        });
        copy
    }

    /// `mapDuration(Int2IntFunction)` — the function is **not** called for an
    /// infinite or zero duration, which is what stops `tickDownDuration`
    /// walking an expired effect down to `-1` and making it infinite.
    #[must_use]
    pub fn map_duration(&self, f: impl FnOnce(i32) -> i32) -> i32 {
        if self.is_infinite_duration() || self.duration == 0 {
            self.duration
        } else {
            f(self.duration)
        }
    }

    /// `tickDownDuration()` — the hidden chain first, then this instance.
    fn tick_down_duration(&mut self) {
        if let Some(hidden) = self.hidden_effect.as_mut() {
            hidden.tick_down_duration();
        }
        self.duration = self.map_duration(|d| d - 1);
    }

    /// `downgradeToHiddenEffect()` — at exactly zero duration, restore the
    /// stashed instance and pop the chain.
    fn downgrade_to_hidden_effect(&mut self) -> bool {
        if self.duration != 0 {
            return false;
        }
        let Some(hidden) = self.hidden_effect.take() else {
            return false;
        };
        let hidden = *hidden;
        // `setDetailsFrom` copies duration, amplifier and the three flags but
        // NOT the hidden pointer, which is then set to the restored
        // instance's own hidden effect.
        self.duration = hidden.duration;
        self.amplifier = hidden.amplifier;
        self.ambient = hidden.ambient;
        self.visible = hidden.visible;
        self.show_icon = hidden.show_icon;
        self.hidden_effect = hidden.hidden_effect;
        true
    }

    /// `isShorterDurationThan(other)` — `!isInfiniteDuration() && (duration <
    /// other.duration || other.isInfiniteDuration())`.
    fn is_shorter_duration_than(&self, other: &Self) -> bool {
        !self.is_infinite_duration()
            && (self.duration < other.duration || other.is_infinite_duration())
    }

    /// `tickServer(ServerLevel, LivingEntity, Runnable)`.
    ///
    /// `tick_count` is the entity's, and is read only for an infinite effect.
    /// `apply` is `MobEffect.applyEffectTick`, which the caller supplies
    /// because it reaches the whole world; it returns vanilla's `keep`.
    ///
    /// Returns `hasRemainingDuration()` — `false` meaning the effect is over
    /// and should be removed — and reports whether the `Runnable` (which
    /// re-sends the effect to the client) fired.
    pub fn tick_server(
        &mut self,
        rule: TickRule,
        tick_count: i32,
        apply: impl FnOnce(i32, i32) -> bool,
    ) -> TickServer {
        if !self.has_remaining_duration() {
            return TickServer {
                keep: false,
                fired: false,
                downgraded: false,
            };
        }
        let effective_duration = if self.is_infinite_duration() {
            tick_count
        } else {
            self.duration
        };
        let mut fired = false;
        if rule.fires(effective_duration, self.amplifier) {
            fired = true;
            if !apply(effective_duration, self.amplifier) {
                // Removed WITHOUT ticking the duration down.
                return TickServer {
                    keep: false,
                    fired,
                    downgraded: false,
                };
            }
        }
        self.tick_down_duration();
        let downgraded = self.downgrade_to_hidden_effect();
        TickServer {
            keep: self.has_remaining_duration(),
            fired,
            downgraded,
        }
    }

    /// `update(MobEffectInstance)` — merge `other` into this one, returning
    /// whether anything a client can see changed.
    ///
    /// The caller has already established that the two name the same effect;
    /// vanilla logs a warning and carries on if they do not.
    pub fn update(&mut self, other: &Self) -> bool {
        let mut changed = false;
        if other.amplifier > self.amplifier {
            if other.is_shorter_duration_than(self) {
                // Push a copy of the current state down the chain, keeping the
                // existing chain under it.
                let existing = self.hidden_effect.take();
                let mut stash = self.clone();
                stash.hidden_effect = existing;
                self.hidden_effect = Some(Box::new(stash));
            }
            self.amplifier = other.amplifier;
            self.duration = other.duration;
            changed = true;
        } else if self.is_shorter_duration_than(other) {
            if other.amplifier == self.amplifier {
                self.duration = other.duration;
                changed = true;
            } else if self.hidden_effect.is_none() {
                let mut stash = other.clone();
                stash.hidden_effect = None;
                self.hidden_effect = Some(Box::new(stash));
            } else if let Some(hidden) = self.hidden_effect.as_mut() {
                hidden.update(other);
            }
        }

        // 156..183. Note the shape: the assignment runs when the *other* is
        // not ambient and this one is, OR when something already changed. So
        // an ambient effect can be un-ambient-ed by a non-ambient one, and a
        // change elsewhere in the method drags the ambient flag along with it.
        if (!other.ambient && self.ambient) || changed {
            self.ambient = other.ambient;
            changed = true;
        }
        if other.visible != self.visible {
            self.visible = other.visible;
            changed = true;
        }
        if other.show_icon != self.show_icon {
            self.show_icon = other.show_icon;
            changed = true;
        }
        changed
    }
}

/// What one [`MobEffectInstance::tick_server`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickServer {
    /// `tickServer`'s return value: keep the effect.
    pub keep: bool,
    /// The rule fired and `applyEffectTick` ran.
    pub fired: bool,
    /// `downgradeToHiddenEffect` popped the chain, and the `Runnable` — which
    /// re-sends the effect to the client — was run.
    pub downgraded: bool,
}

/// `Mth.floor(float)` — `(int) Math.floor(v)`.
fn mth_floor(v: f32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.floor's own narrowing; the callers here are durations"
    )]
    {
        v.floor() as i32
    }
}

/// One row of `data/vanilla/mob_effects.json`, as a running server needs it.
///
/// Two columns and no more: the **registry id**, which is the varint
/// `ClientboundUpdateMobEffectPacket` and `ClientboundRemoveMobEffectPacket`
/// both carry, and the [`TickRule`] derived from the `class` column.
/// Everything else in that file — the colour, the description id, the
/// attribute modifiers, the 38,720-cell tick probe — is read by
/// `crates/rustmc-server/tests/mob_effects_table.rs` and by nothing at
/// runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectRow {
    /// `BuiltInRegistries.MOB_EFFECT.getId(effect)` — the number on the wire.
    pub registry_id: i32,
    /// Which of the five rules this effect's class implements.
    pub rule: TickRule,
}

/// `data/vanilla/mob_effects.json`, indexed by effect id.
#[derive(Debug, Default)]
pub struct EffectRegistry {
    by_id: std::collections::BTreeMap<String, EffectRow>,
}

impl EffectRegistry {
    /// Read the table at an explicit path.
    ///
    /// # Errors
    ///
    /// The file being unreadable, not JSON, or not shaped like the table. A
    /// row whose `class` has no [`TickRule`] is an **error, not a skipped
    /// row**: a new `MobEffect` subclass in a later version has to be a
    /// refusal here rather than an effect that silently never ticks.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Self::from_json(&doc)
    }

    /// Parse an already-loaded document.
    ///
    /// # Errors
    ///
    /// A missing or wrongly typed field, or an unrecognised `class`.
    pub fn from_json(doc: &serde_json::Value) -> std::io::Result<Self> {
        let bad = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
        let rows = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("mob_effects.json has no `table` array".into()))?;
        let mut by_id = std::collections::BTreeMap::new();
        for row in rows {
            let id = row
                .get("id")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a mob_effects row has no `id`".into()))?;
            let registry_id = row
                .get("registryId")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| bad(format!("{id} has no `registryId`")))?;
            let class = row
                .get("class")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad(format!("{id} has no `class`")))?;
            let rule = rule_for_class(class)
                .ok_or_else(|| bad(format!("{id}: no TickRule for class {class}")))?;
            let registry_id = i32::try_from(registry_id)
                .map_err(|_| bad(format!("{id}: registryId {registry_id} does not fit")))?;
            by_id.insert(id.to_string(), EffectRow { registry_id, rule });
        }
        if by_id.is_empty() {
            return Err(bad("mob_effects.json has an empty table".into()));
        }
        Ok(Self { by_id })
    }

    /// The process-wide registry, or `None` without the table.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        static REG: std::sync::OnceLock<Option<EffectRegistry>> = std::sync::OnceLock::new();
        REG.get_or_init(|| {
            let cwd = std::path::Path::new(TABLE_PATH);
            let path = if cwd.exists() {
                cwd.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(TABLE_PATH)
            };
            Self::load(&path).ok()
        })
        .as_ref()
    }

    /// One effect's row.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<EffectRow> {
        self.by_id.get(id).copied()
    }

    /// How many effects the table carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether the table carried none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{heal_or_harm_amount, MobEffectInstance, TickRule, INFINITE_DURATION};

    const REGEN: TickRule = TickRule::Periodic { base: 50 };
    const POISON: TickRule = TickRule::Periodic { base: 25 };
    const WITHER: TickRule = TickRule::Periodic { base: 40 };

    #[test]
    fn the_base_rule_never_fires_and_that_is_most_of_the_registry() {
        for d in [0, 1, 50, 600] {
            for a in [0, 1, 5] {
                assert!(!TickRule::Never.fires(d, a));
            }
        }
    }

    #[test]
    fn regeneration_fires_on_the_tick_it_is_applied() {
        // duration 900 is `/effect give @s regeneration 45`; 900 % 50 == 0.
        assert!(REGEN.fires(900, 0));
    }

    #[test]
    fn the_period_halves_with_each_amplifier() {
        assert!(REGEN.fires(50, 0) && !REGEN.fires(49, 0));
        assert!(REGEN.fires(25, 1) && !REGEN.fires(24, 1));
        assert!(REGEN.fires(12, 2) && !REGEN.fires(11, 2));
        assert!(POISON.fires(25, 0) && !POISON.fires(24, 0));
        assert!(WITHER.fires(40, 0) && !WITHER.fires(39, 0));
    }

    #[test]
    fn a_period_shifted_to_zero_fires_every_tick_rather_than_never() {
        // 25 >> 5 == 0. This is the arm at offset 21 and the one a naive
        // `d % (base >> amp) == 0` would divide by zero on.
        for d in 1..40 {
            assert!(POISON.fires(d, 5), "poison V at {d}");
        }
        for d in 1..40 {
            assert!(REGEN.fires(d, 6), "regen VII at {d}");
        }
    }

    #[test]
    fn the_shift_count_is_masked_to_five_bits_like_javas() {
        // `25 >> 32` is 25 in Java, not 0, so amplifier 32 behaves exactly
        // like amplifier 0 rather than firing every tick.
        assert!(POISON.fires(25, 32));
        assert!(!POISON.fires(24, 32));
    }

    #[test]
    fn the_raid_omen_rule_fires_only_on_the_last_tick() {
        assert!(TickRule::ExactlyOne.fires(1, 0));
        assert!(!TickRule::ExactlyOne.fires(2, 0));
        assert!(!TickRule::ExactlyOne.fires(0, 0));
    }

    #[test]
    fn the_instantaneous_rule_fires_from_one_upwards() {
        assert!(!TickRule::AtLeastOne.fires(0, 0));
        assert!(TickRule::AtLeastOne.fires(1, 0));
        assert!(TickRule::AtLeastOne.fires(600, 0));
    }

    #[test]
    fn a_finite_effect_counts_down_and_ends() {
        let mut e = MobEffectInstance::new("minecraft:speed", 3, 0);
        let mut ticks = 0;
        loop {
            let out = e.tick_server(TickRule::Never, 0, |_, _| true);
            if !out.keep {
                break;
            }
            ticks += 1;
            assert!(ticks < 10);
        }
        assert_eq!(e.duration(), 0);
        assert_eq!(ticks, 2);
    }

    #[test]
    fn the_first_tick_reads_the_duration_before_decrementing_it() {
        let mut e = MobEffectInstance::new("minecraft:regeneration", 50, 0);
        let mut seen = None;
        e.tick_server(REGEN, 0, |d, _| {
            seen = Some(d);
            true
        });
        assert_eq!(seen, Some(50));
        assert_eq!(e.duration(), 49);
    }

    #[test]
    fn an_infinite_effect_never_ticks_down_and_uses_the_entity_clock() {
        let mut e = MobEffectInstance::new("minecraft:regeneration", INFINITE_DURATION, 0);
        let mut fires = 0;
        for tick in 0..200 {
            let out = e.tick_server(REGEN, tick, |_, _| true);
            assert!(out.keep);
            if out.fired {
                fires += 1;
            }
        }
        assert_eq!(e.duration(), INFINITE_DURATION);
        // tickCount 0, 50, 100, 150 — four in the first two hundred.
        assert_eq!(fires, 4);
    }

    #[test]
    fn apply_returning_false_removes_the_effect_without_ticking_it_down() {
        let mut e = MobEffectInstance::new("minecraft:bad_omen", 600, 0);
        let out = e.tick_server(TickRule::Always, 0, |_, _| false);
        assert!(!out.keep);
        assert!(out.fired);
        assert_eq!(e.duration(), 600);
    }

    #[test]
    fn map_duration_leaves_zero_and_infinite_alone() {
        let zero = MobEffectInstance::new("minecraft:speed", 0, 0);
        assert_eq!(zero.map_duration(|d| d - 1), 0);
        let inf = MobEffectInstance::new("minecraft:speed", INFINITE_DURATION, 0);
        assert_eq!(inf.map_duration(|d| d - 1), INFINITE_DURATION);
        let one = MobEffectInstance::new("minecraft:speed", 1, 0);
        assert_eq!(one.map_duration(|d| d - 1), 0);
    }

    #[test]
    fn ends_within_and_infinite_are_not_the_same_question() {
        let inf = MobEffectInstance::new("minecraft:speed", INFINITE_DURATION, 0);
        assert!(!inf.ends_within(100_000));
        let short = MobEffectInstance::new("minecraft:speed", 5, 0);
        assert!(short.ends_within(5));
        assert!(!short.ends_within(4));
    }

    #[test]
    fn with_scaled_duration_floors_and_keeps_at_least_one_tick() {
        let e = MobEffectInstance::new("minecraft:speed", 100, 0);
        assert_eq!(e.with_scaled_duration(0.5).duration(), 50);
        assert_eq!(e.with_scaled_duration(0.001).duration(), 1);
        assert_eq!(e.with_scaled_duration(0.0).duration(), 1);
        let inf = MobEffectInstance::new("minecraft:speed", INFINITE_DURATION, 0);
        assert_eq!(inf.with_scaled_duration(0.5).duration(), INFINITE_DURATION);
    }

    #[test]
    fn a_stronger_shorter_effect_hides_the_weaker_longer_one_and_it_comes_back() {
        // Regeneration I for 200 ticks, then Regeneration II for 5.
        let mut held = MobEffectInstance::new("minecraft:regeneration", 200, 0);
        let incoming = MobEffectInstance::new("minecraft:regeneration", 5, 1);
        assert!(held.update(&incoming));
        assert_eq!(held.amplifier(), 1);
        assert_eq!(held.duration(), 5);
        assert!(held.hidden().is_some());
        // The hidden copy ticks down alongside the visible one.
        assert_eq!(held.hidden().map(MobEffectInstance::duration), Some(200));

        for _ in 0..5 {
            let out = held.tick_server(TickRule::Never, 0, |_, _| true);
            assert!(out.keep);
        }
        // Five ticks later the strong one is gone and the weak one is back,
        // five ticks shorter than it was.
        assert_eq!(held.amplifier(), 0);
        assert_eq!(held.duration(), 195);
        assert!(held.hidden().is_none());
    }

    #[test]
    fn a_stronger_longer_effect_simply_replaces_the_weaker_one() {
        let mut held = MobEffectInstance::new("minecraft:regeneration", 100, 0);
        let incoming = MobEffectInstance::new("minecraft:regeneration", 400, 1);
        assert!(held.update(&incoming));
        assert_eq!(held.amplifier(), 1);
        assert_eq!(held.duration(), 400);
        assert!(held.hidden().is_none());
    }

    #[test]
    fn a_longer_effect_of_the_same_strength_just_extends_it() {
        let mut held = MobEffectInstance::new("minecraft:speed", 100, 0);
        let incoming = MobEffectInstance::new("minecraft:speed", 400, 0);
        assert!(held.update(&incoming));
        assert_eq!(held.duration(), 400);
        assert!(held.hidden().is_none());
    }

    #[test]
    fn a_weaker_shorter_effect_changes_nothing() {
        let mut held = MobEffectInstance::new("minecraft:regeneration", 400, 1);
        let incoming = MobEffectInstance::new("minecraft:regeneration", 100, 0);
        assert!(!held.update(&incoming));
        assert_eq!(held.amplifier(), 1);
        assert_eq!(held.duration(), 400);
        assert!(held.hidden().is_none());
    }

    #[test]
    fn a_weaker_but_longer_effect_is_stashed_and_reported_as_no_change() {
        // The visible state does not change, so `update` returns false, but
        // the weaker effect is kept for later.
        let mut held = MobEffectInstance::new("minecraft:regeneration", 100, 1);
        let incoming = MobEffectInstance::new("minecraft:regeneration", 400, 0);
        assert!(!held.update(&incoming));
        assert_eq!(held.amplifier(), 1);
        assert_eq!(held.duration(), 100);
        assert_eq!(held.hidden().map(MobEffectInstance::amplifier), Some(0));
        assert_eq!(held.hidden().map(MobEffectInstance::duration), Some(400));
    }

    #[test]
    fn an_infinite_incoming_effect_is_longer_than_anything() {
        let mut held = MobEffectInstance::new("minecraft:speed", i32::MAX, 0);
        let incoming = MobEffectInstance::new("minecraft:speed", INFINITE_DURATION, 0);
        assert!(held.update(&incoming));
        assert!(held.is_infinite_duration());
    }

    #[test]
    fn the_ambient_flag_is_dragged_along_by_any_other_change() {
        let mut held = MobEffectInstance::with_flags("minecraft:speed", 100, 0, true, true, true);
        let incoming = MobEffectInstance::with_flags("minecraft:speed", 400, 0, true, true, true);
        // Both ambient, but the duration change sets `changed`, which then
        // reassigns ambient anyway.
        assert!(held.update(&incoming));
        assert!(held.is_ambient());

        // A non-ambient incoming clears the flag even with nothing else
        // changing.
        let mut held2 = MobEffectInstance::with_flags("minecraft:speed", 400, 0, true, true, true);
        let incoming2 = MobEffectInstance::with_flags("minecraft:speed", 100, 0, false, true, true);
        assert!(held2.update(&incoming2));
        assert!(!held2.is_ambient());
        assert_eq!(held2.duration(), 400);
    }

    #[test]
    fn the_visible_and_icon_flags_each_count_as_a_change_on_their_own() {
        let mut held = MobEffectInstance::with_flags("minecraft:speed", 400, 0, false, true, true);
        let incoming = MobEffectInstance::with_flags("minecraft:speed", 100, 0, false, false, true);
        assert!(held.update(&incoming));
        assert!(!held.is_visible());

        let mut held2 = MobEffectInstance::with_flags("minecraft:speed", 400, 0, false, true, true);
        let incoming2 =
            MobEffectInstance::with_flags("minecraft:speed", 100, 0, false, true, false);
        assert!(held2.update(&incoming2));
        assert!(!held2.show_icon());
    }

    #[test]
    fn instant_health_and_harming_scale_by_different_shifts() {
        // Instant Health I: 4 << 0 = 4, times the 1.0 multiplier, + 0.5 -> 4.
        assert_eq!(heal_or_harm_amount(false, 0, 1.0), 4);
        assert_eq!(heal_or_harm_amount(false, 1, 1.0), 8);
        // Instant Damage I: 6 << 0 = 6.
        assert_eq!(heal_or_harm_amount(true, 0, 1.0), 6);
        assert_eq!(heal_or_harm_amount(true, 1, 1.0), 12);
    }

    #[test]
    fn the_instantaneous_amount_is_a_round_half_up_and_not_a_round() {
        // 0.5 multiplier on Instant Health I: 4 * 0.5 = 2.0, + 0.5 = 2.5,
        // truncated to 2. `round()` would give 3.
        assert_eq!(heal_or_harm_amount(false, 0, 0.5), 2);
        // And on the negative side the two differ in the other direction:
        // -4 * 0.5 = -2.0, + 0.5 = -1.5, `d2i` truncates toward zero -> -1,
        // where `round()` gives -2.
        assert_eq!(heal_or_harm_amount(false, 0, -0.5), -1);
    }

    /// The positive control. Three misreadings of the tick machinery, each
    /// written out and shown to disagree with the transcription.
    #[test]
    fn each_misreading_of_the_effect_tick_is_separable() {
        // 1. The `i > 0` fallback dropped: Poison V would never fire (or
        //    divide by zero). The transcription fires every tick.
        assert!(POISON.fires(7, 5));
        let naive_period = 25_i32 >> 5;
        assert_eq!(naive_period, 0, "the shift really does reach zero");

        // 2. `tickDownDuration` before the rule rather than after: a
        //    900-tick Regeneration would first fire at duration 850 rather
        //    than at 900, and every later fire would be off by one.
        let mut real = MobEffectInstance::new("minecraft:regeneration", 900, 0);
        let mut first = None;
        real.tick_server(REGEN, 0, |d, _| {
            first = Some(d);
            true
        });
        assert_eq!(first, Some(900));
        assert!(
            !REGEN.fires(899, 0),
            "the decrement-first reading misses here"
        );

        // 3. `applyEffectTick` returning false read as "skip this tick"
        //    rather than "remove the effect".
        let mut removed = MobEffectInstance::new("minecraft:bad_omen", 600, 0);
        let out = removed.tick_server(TickRule::Always, 0, |_, _| false);
        assert!(!out.keep);
        assert_eq!(removed.duration(), 600, "and it did not tick down either");
    }
}
