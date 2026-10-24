//! `ServerPlayer.doTick` — the per-player state machines, on one real tick.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.server.level.ServerPlayer.doTick()` offsets 0..583 and
//! `ServerPlayer.tickRegeneration()` (the `Difficulty.PEACEFUL` block reached
//! from `Player.tick`).
//!
//! # What this file is for
//!
//! `origin/player-systems` ported nine state machines and wired **none** of
//! them, on the stated ground that no scenario could tell a correct wiring
//! from a wrong one. Every one of them is a function from what vanilla reads
//! to what vanilla does; what was missing is the thing that calls them in
//! vanilla's order, holds the fields between calls, and turns the result into
//! packets. That is this file, and the order is not a detail:
//!
//! ```text
//!   0    if (isSpectator() && touchingUnloadedChunk()) return
//!   15   super.tick()                      // Player.tick
//!   19   containerMenu.stillValid          // not modelled here
//!   42   foodData.tick(this)               // crate::food
//!   49   awardStat(PLAY_TIME) x4           // no stats system
//!   107  synchronizeSpecialItemUpdates     // maps only
//!   145  the set_health change detector    // crate::playersync
//!   268  updateScoreForCriteria x6         // no scoreboard
//!   487  the set_experience change detector
//!   533  tickCount == 0 -> TICK trigger    // no advancements
//! ```
//!
//! **`foodData.tick` runs BEFORE the change detector, in the same call.** So
//! a food level that drops on tick *n* is on the wire on tick *n*, not *n+1*.
//! A port that ticks food from one place and syncs from another gets a
//! one-tick lag that no unit test of either half can see.
//!
//! # What `Player.tick()` at offset 15 reaches that this server does not
//!
//! Named rather than dropped, because a reader who finds `super.tick()`
//! missing from this file needs to know whether that is a decision:
//! `Player.tick` runs the item-use countdown ([`crate::itemuse`] owns the
//! packet half), `LivingEntity.tick` -> `aiStep`, `baseTick`'s air branch
//! ([`crate::breath`], joined here), the `invulnerableTime` decrement that
//! `ServerPlayer.tick` and not `baseTick` performs for a player
//! ([`crate::breath::tick_server_player_counters`], joined here), and the
//! sleep counter ([`crate::sleep`]). Everything else in it — attributes,
//! brain, equipment, portals, riding — has no field on this server.
//!
//! # Why the packets are returned rather than sent
//!
//! `doTick` is called from a socket loop that owns the connection, and a
//! function that both computes and sends cannot be unit-tested without one.
//! [`Emit`] is the whole of what one tick puts on the wire, in order, and
//! [`crate::world_send`] is the only thing that writes it.

use crate::breath::{self, BreathContext};
use crate::damage_types::DamageTypeRegistry;
use crate::effects::{EffectRegistry, MobEffectInstance};
use crate::experience::Experience;
use crate::food::{Difficulty, FoodAction, FoodData, TickInputs};
use crate::health::{DamageTags, Health, HurtContext};
use crate::hurt::{self, HurtOutcome, HurtServerContext};
use crate::playersync::{SetExperience, SetHealth, SyncInputs, SyncState};

/// `Player.MAX_HEALTH`'s attribute default, and `Attributes.MAX_ABSORPTION`'s.
///
/// Read from `data/vanilla/attributes.json` by [`LivePlayer::new`] when the
/// table is present; these are the fallback for a session with no tables, and
/// they are the values that table carries for `minecraft:player`.
pub const DEFAULT_MAX_HEALTH: f32 = 20.0;
/// `Attributes.MAX_ABSORPTION`'s default is 0.0 — absorption is entirely a
/// modifier effect.
pub const DEFAULT_MAX_ABSORPTION: f32 = 0.0;
/// `Entity.getMaxAirSupply()` — `TOTAL_AIR_SUPPLY`, 300.
pub const DEFAULT_MAX_AIR_SUPPLY: i32 = 300;

/// The three damage types this tick can deal, resolved out of
/// `data/vanilla/damage_types.json`.
///
/// **Not constants, and not `DamageTags::default()`.** A default claims a
/// damage type bypasses nothing, and all three of these bypass armour;
/// `minecraft:starve` also bypasses effects, and `minecraft:drown` carries
/// `no_impact` and `no_knockback`. Every one of those is a behaviour
/// difference a hard-coded default would get wrong silently, which is why
/// [`crate::falldamage::fall_tags`] refuses to be a constant too.
///
/// **None of the three carries `minecraft:bypasses_cooldown`**, so all three
/// go through `hurtServer`'s ten-tick difference rule: a 7-damage fall two
/// ticks after a 4-damage one deals 3, not 7, and the second is silent on the
/// wire. Checked against the table rather than assumed — see
/// `damage_tags_are_read_from_the_table_and_none_bypasses_the_cooldown`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamageTagSet {
    /// `minecraft:fall`.
    pub fall: DamageTags,
    /// `minecraft:drown`.
    pub drown: DamageTags,
    /// `minecraft:starve`.
    pub starve: DamageTags,
}

/// The ids [`DamageTagSet`] resolves, in one place so a grep finds them.
pub const DAMAGE_TYPE_IDS: [&str; 3] = ["minecraft:fall", "minecraft:drown", "minecraft:starve"];

impl DamageTagSet {
    /// Resolve all three, or refuse.
    ///
    /// `None` when the registry is missing any of the three, which is the
    /// same refusal [`crate::damage_types::DamageTypeRegistry::tags_for`]
    /// makes for an unknown id. A partial set is not returned: a player who
    /// can drown but cannot fall is a worse state to debug than one who can
    /// do neither.
    #[must_use]
    pub fn from_registry(registry: &DamageTypeRegistry) -> Option<Self> {
        Some(Self {
            fall: registry.tags_for("minecraft:fall")?,
            drown: registry.tags_for("minecraft:drown")?,
            starve: registry.tags_for("minecraft:starve")?,
        })
    }

    /// The process-wide set, read once from `data/vanilla/damage_types.json`.
    ///
    /// `None` without the table, and a session with `None` behaves exactly as
    /// this server did before this file existed: no damage of any of the
    /// three kinds is ever dealt, and [`LivePlayer::hurts_refused`] counts how
    /// often that mattered so the silence is not mistaken for a player who
    /// was never hurt.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        static SET: std::sync::OnceLock<Option<DamageTagSet>> = std::sync::OnceLock::new();
        SET.get_or_init(|| {
            let registry = DamageTypeRegistry::load(&table_path("damage_types.json")).ok()?;
            Self::from_registry(&registry)
        })
        .as_ref()
    }
}

/// `data/vanilla/<name>` relative to the process cwd, falling back to the
/// crate directory — the same two-step every other loader in this crate uses.
pub(crate) fn table_path(name: &str) -> std::path::PathBuf {
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
/// `"data/vanilla/<name>.json"`, so the literal has to appear where a grep
/// finds it even though [`table_path`] joins the name on.
const _DERIVED_TABLES: [&str; 1] = ["data/vanilla/damage_types.json"];

/// The difficulty both corpus substrates set, and the one this server acts on.
///
/// **This server has no difficulty.** Nothing reads `server.properties`'
/// `difficulty`, no packet carries it, and `ServerLevel.getDifficulty()` has
/// no counterpart here. `ci/t2-gameplay.sh` and `ci/t2-gameplay-survival.sh`
/// both write `difficulty=peaceful`, so this is the value under which every
/// measurement of this server has been taken — named rather than defaulted,
/// because guessing `NORMAL` would starve a player vanilla does not
/// (`FoodData.tick` offset 335 is `difficulty != PEACEFUL`) and the
/// difference would look like a hunger bug rather than a missing field.
pub const SUBSTRATE_DIFFICULTY: Difficulty = Difficulty::Peaceful;

/// The breath context for a player this server cannot tell is in water.
///
/// `Entity.isEyeInFluid(FluidTags.WATER)` needs the fluid state at the eye
/// position, and [`crate::playermove`] tracks a fluid state for the body but
/// exposes no eye query. So the air never falls, no player ever drowns, and
/// that is a **stated** gap: the drowning arm of [`LivePlayer::do_tick`] is
/// reachable and unit-tested, and what is missing is the predicate that would
/// open it. Everything else in this constant is the absent case.
pub const DRY_BREATH: BreathContext = BreathContext {
    eye_in_water: false,
    eye_in_bubble_column: false,
    can_breathe_underwater: false,
    water_breathing: false,
    invulnerable_player: false,
    effects_refill_air: false,
    oxygen_bonus: 0.0,
    max_air_supply: DEFAULT_MAX_AIR_SUPPLY,
};

/// One thing a tick put on this player's connection.
///
/// The order of a `Vec<Emit>` is the order `doTick` sends in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Emit {
    /// `ClientboundSetHealthPacket` — `doTick` offsets 196..224.
    Health(SetHealth),
    /// `ClientboundSetExperiencePacket` — `doTick` offsets 507..529.
    Experience(SetExperience),
    /// `level.broadcastEntityEvent(this, (byte) 67)` — the drowning bubble
    /// burst, `LivingEntity.baseTick` offset 318.
    EntityEvent(u8),
    /// `ClientboundUpdateMobEffectPacket` — `ServerPlayer.onEffectAdded`
    /// offsets 7..23 and `onEffectUpdated` offsets 8..24.
    UpdateMobEffect(EffectPacket),
    /// `ClientboundRemoveMobEffectPacket` — `ServerPlayer.onEffectsRemoved`
    /// offsets 32..50, one per removed instance.
    RemoveMobEffect {
        /// The effect's `BuiltInRegistries.MOB_EFFECT` id.
        registry_id: i32,
    },
    /// `LivingEntity.tickDeath` offsets 11..50, on the twentieth tick of
    /// death: `level.broadcastEntityEvent(this, 60)` then
    /// `remove(RemovalReason.KILLED)`. The caller owns both — the event goes
    /// to tracking players and self, the removal takes the entity off every
    /// tracker.
    DeathRemoval,
    /// `ServerPlayer.onEnterCombat` / `onLeaveCombat` — see
    /// [`crate::combat_tracker`].
    Combat(crate::combat_tracker::CombatPacket),
}

/// `ClientboundUpdateMobEffectPacket`'s five wire fields, minus the entity id
/// which belongs to the connection.
///
/// **`blend` is the constructor's third argument and it is not derived from
/// the instance.** `ServerPlayer.onEffectAdded` passes `iconst_1` at offset
/// 19 and `onEffectUpdated` passes `iconst_0` at offset 20, so the same
/// effect goes out with bit 8 set when it is *added* and clear when it is
/// *updated*. A port that computes the flags from the `MobEffectInstance`
/// alone gets one of the two wrong, always, and the difference is a visual
/// fade the client does or does not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectPacket {
    /// The effect's `BuiltInRegistries.MOB_EFFECT` id.
    pub registry_id: i32,
    /// `MobEffectInstance.getAmplifier()`.
    pub amplifier: i32,
    /// `MobEffectInstance.getDuration()`.
    pub duration: i32,
    /// The packet's own `flags` byte: `AMBIENT = 1`, `VISIBLE = 2`,
    /// `SHOW_ICON = 4`, `BLEND = 8`.
    pub flags: u8,
}

/// The packet's four flag bits, from its `<init>` offsets 33..93.
pub mod effect_flags {
    /// `FLAG_AMBIENT` — `MobEffectInstance.isAmbient()`.
    pub const AMBIENT: u8 = 1;
    /// `FLAG_VISIBLE` — `MobEffectInstance.isVisible()`.
    pub const VISIBLE: u8 = 2;
    /// `FLAG_SHOW_ICON` — `MobEffectInstance.showIcon()`.
    pub const SHOW_ICON: u8 = 4;
    /// `FLAG_BLEND` — the constructor's third argument, NOT a property of the
    /// instance. See [`super::EffectPacket`].
    pub const BLEND: u8 = 8;
}

/// The `% 600` in `LivingEntity.tickEffects` offsets 104..111 — how often a
/// surviving effect is re-announced to the client.
pub const EFFECT_RESEND_PERIOD: i32 = 600;

/// The level-side facts one `doTick` reads.
///
/// Every field is a `getDifficulty`, a `getGameRules().get(...)` or an
/// `isEyeInFluid` in the methods this file joins. Defaults are a normal-mode
/// overworld with natural regeneration on, which is a vanilla server's own
/// default and the corpus substrate's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelFacts {
    /// `ServerLevel.getDifficulty()`.
    pub difficulty: Difficulty,
    /// `GameRules.get(NATURAL_HEALTH_REGENERATION)` — the 26.2 registry id is
    /// `minecraft:natural_health_regeneration`.
    pub natural_regeneration: bool,
    /// Everything `LivingEntity.baseTick`'s water branch reads.
    pub breath: BreathContext,
    /// `abilities.invulnerable` — creative mode. It gates
    /// [`crate::movestats::cause_food_exhaustion`] and the drowning arm.
    pub abilities_invulnerable: bool,
}

impl Default for LevelFacts {
    fn default() -> Self {
        Self {
            difficulty: Difficulty::Normal,
            natural_regeneration: true,
            breath: DRY_BREATH,
            abilities_invulnerable: false,
        }
    }
}

/// `i2f`.
#[expect(clippy::cast_precision_loss, reason = "vanilla's own i2f")]
const fn f32_from_i32(v: i32) -> f32 {
    v as f32
}

/// The per-player fields `doTick` and its callees read and write.
///
/// One of these lives on each play-loop session. It is deliberately a plain
/// aggregate of the already-ported state machines rather than a new model:
/// nothing here re-derives a number, and every field is named for the vanilla
/// field it holds.
#[derive(Debug, Clone)]
pub struct LivePlayer {
    /// `LivingEntity`'s health, absorption and the four cooldown fields.
    pub health: Health,
    /// `Player.foodData`.
    pub food: FoodData,
    /// `Player.experienceLevel`/`experienceProgress`/`totalExperience`.
    pub experience: Experience,
    /// `Entity.getAirSupply()` — `DATA_AIR_SUPPLY_ID`.
    pub air_supply: i32,
    /// `Entity.tickCount`.
    pub tick_count: i32,
    /// `LivingEntity.combatTracker`.
    pub combat: crate::combat_tracker::CombatTracker,
    /// Combat packets `hurt_player` produced, drained by its caller so they
    /// leave before the `damage_event` the level half sends.
    pub combat_out: Vec<crate::combat_tracker::CombatPacket>,
    /// `Player.isSleeping()`. Read by `hurtServer` and by the sleep ladder.
    pub sleeping: bool,
    /// The `lastSent*` change detector. Private because a caller that wrote
    /// it could suppress a packet vanilla sends.
    sync: SyncState,
    /// The three damage types, or `None` without
    /// `data/vanilla/damage_types.json`.
    damage: Option<DamageTagSet>,
    /// How many hurts this player refused for want of that table.
    ///
    /// **This is the whole reason the refusal is not silent.** A server with
    /// no table and no counter looks exactly like a server whose player never
    /// fell, and that is the shape `docs/AGENT-BRIEF.md` opens with.
    hurts_refused: u32,
    /// `LivingEntity.activeEffects`, which is a `Map<Holder<MobEffect>,
    /// MobEffectInstance>` — insertion-ordered in vanilla only by accident of
    /// `HashMap`, and iterated by `tickEffects`. Ordered here because
    /// `clippy::iter_over_hash_type` is denied in this workspace for exactly
    /// the reason ORACLE.md §6 gives: a hash order that reaches the wire is a
    /// divergence nothing can reproduce.
    active_effects: std::collections::BTreeMap<String, MobEffectInstance>,
    /// `data/vanilla/mob_effects.json`, or `None` without it. Effects cannot
    /// be added at all when it is absent, because their registry id is the
    /// only thing either packet carries.
    effect_registry: Option<&'static EffectRegistry>,
    /// `LivingEntity.deathTime`, counted by `tickDeath` while
    /// `isDeadOrDying()`.
    pub death_time: i32,
    /// `Entity.isRemoved()` — set by `tickDeath`'s `remove(KILLED)`.
    pub removed: bool,
    /// `LivingEntity.lastHurtByPlayer`, as the attacker's entity id and
    /// name, and `lastHurtByPlayerMemoryTime`. Written by
    /// `resolvePlayerResponsibleForDamage` (100 ticks), aged by `baseTick`
    /// offsets 513..535, read by `getKillCredit` for a death message.
    pub last_hurt_by_player: Option<(i32, String)>,
    pub last_hurt_by_player_memory: i32,
    /// `Player.lastDeathLocation`'s block position, set by `ServerPlayer.die`
    /// offset 358 and carried into the respawn's spawn info.
    pub death_location: Option<(i32, i32, i32)>,
}

impl Default for LivePlayer {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_HEALTH, DEFAULT_MAX_ABSORPTION)
    }
}

impl LivePlayer {
    /// A freshly joined player: full health, `FoodData.<init>`, no experience,
    /// full air, and the four `lastSent*` sentinels.
    #[must_use]
    pub fn new(max_health: f32, max_absorption: f32) -> Self {
        Self {
            health: Health::full(max_health, max_absorption),
            food: FoodData::new(),
            experience: Experience::new(),
            air_supply: DEFAULT_MAX_AIR_SUPPLY,
            tick_count: 0,
            combat: crate::combat_tracker::CombatTracker::default(),
            combat_out: Vec::new(),
            sleeping: false,
            sync: SyncState::new(),
            damage: DamageTagSet::shared().copied(),
            hurts_refused: 0,
            active_effects: std::collections::BTreeMap::new(),
            effect_registry: EffectRegistry::shared(),
            death_time: 0,
            removed: false,
            last_hurt_by_player: None,
            last_hurt_by_player_memory: 0,
            death_location: None,
        }
    }

    /// `LivingEntity.addEffect(MobEffectInstance, Entity)`, narrowed to the
    /// arms this server can reach.
    ///
    /// Returns the packet the client is owed, or `None` when nothing changed.
    /// **`blend` is `true` here** — `ServerPlayer.onEffectAdded` offset 19 is
    /// `iconst_1` — and `false` on the resend path; see [`EffectPacket`].
    ///
    /// # There is no caller for this on the wire yet, and that is stated
    ///
    /// Vanilla reaches `addEffect` from `Item.finishUsingItem` (a potion or a
    /// food's `Consumable` component), from `/effect`, from a beacon and from
    /// a mob's attack. **This server has none of the four.** The use ladder
    /// that would supply the first is `Item.use`'s `Consumable` arm on
    /// `origin/items-2`, which needs that branch's `item_props` extract;
    /// `/effect` is permission level 2 and the corpus players are not
    /// operators; there is no beacon and no hostile mob. So this method is
    /// called by [`LivePlayer::do_tick`]'s own resend and removal paths, by
    /// this module's tests, and by nothing else — which is written down here
    /// rather than left for a reader to discover, because a writer with no
    /// caller is this project's central defect and the honest version says so.
    pub fn add_effect(&mut self, instance: MobEffectInstance) -> Option<Emit> {
        let row = self.effect_registry?.get(instance.effect.as_str())?;
        let packet = EffectPacket {
            registry_id: row.registry_id,
            amplifier: instance.amplifier(),
            duration: instance.duration(),
            flags: Self::flags_of(&instance) | effect_flags::BLEND,
        };
        match self.active_effects.get_mut(instance.effect.as_str()) {
            // `addEffect` offsets 26..49: an existing instance is `update`d
            // and the packet goes out only when something visible changed.
            Some(existing) => {
                if existing.update(&instance) {
                    let packet = EffectPacket {
                        amplifier: existing.amplifier(),
                        duration: existing.duration(),
                        flags: Self::flags_of(existing) | effect_flags::BLEND,
                        ..packet
                    };
                    return Some(Emit::UpdateMobEffect(packet));
                }
                None
            }
            None => {
                self.active_effects
                    .insert(instance.effect.as_str().to_string(), instance);
                Some(Emit::UpdateMobEffect(packet))
            }
        }
    }

    /// `LivingEntity.removeEffect(Holder<MobEffect>)` — offsets 0..16, which
    /// is `removeEffectNoUpdate` plus `onEffectsRemoved`.
    pub fn remove_effect(&mut self, effect: &str) -> Option<Emit> {
        let instance = self.active_effects.remove(effect)?;
        let row = self.effect_registry?.get(instance.effect.as_str())?;
        Some(Emit::RemoveMobEffect {
            registry_id: row.registry_id,
        })
    }

    /// Whether an effect is active — `hasEffect(Holder)`.
    #[must_use]
    pub fn has_effect(&self, effect: &str) -> bool {
        self.active_effects.contains_key(effect)
    }

    /// One active instance, for a caller reading an amplifier.
    #[must_use]
    pub fn effect(&self, effect: &str) -> Option<&MobEffectInstance> {
        self.active_effects.get(effect)
    }

    /// The packet's first three flag bits, which DO come off the instance.
    fn flags_of(instance: &MobEffectInstance) -> u8 {
        let mut flags = 0_u8;
        if instance.is_ambient() {
            flags |= effect_flags::AMBIENT;
        }
        if instance.is_visible() {
            flags |= effect_flags::VISIBLE;
        }
        if instance.show_icon() {
            flags |= effect_flags::SHOW_ICON;
        }
        flags
    }

    /// `LivingEntity.tickEffects()`, server side — offsets 10..129.
    ///
    /// ```text
    ///   for (Holder k : activeEffects.keySet()) {          // iterator
    ///     MobEffectInstance e = activeEffects.get(k);
    ///     if (!e.tickServer(level, this, () -> onEffectUpdated(e, true, null))) {
    ///       it.remove();  onEffectsRemoved(List.of(e));
    ///     } else if (e.getDuration() % 600 == 0) {
    ///       onEffectUpdated(e, false, null);
    ///     }
    ///   }
    /// ```
    ///
    /// Three readings a port loses:
    ///
    /// * **the resend is `duration % 600 == 0`, on the duration and not on
    ///   the tick count**, so two effects with different durations re-announce
    ///   on different ticks and an effect whose duration never hits a multiple
    ///   of 600 is never re-announced at all;
    /// * **the removal branch and the resend branch are `else`-exclusive**, so
    ///   an effect that expires on a multiple of 600 sends `remove` and not
    ///   `update`;
    /// * **`downgradeToHiddenEffect` runs the SAME `Runnable`**, with
    ///   `blend = true`, which is why a Strength II wearing off to Strength I
    ///   arrives as an add rather than as an update.
    fn tick_effects(&mut self, out: &mut Vec<Emit>) {
        let Some(registry) = self.effect_registry else {
            return;
        };
        let tick_count = self.tick_count;
        let mut removed = Vec::new();
        let mut resent = Vec::new();
        let mut downgraded = Vec::new();
        let health = &mut self.health;
        for (id, instance) in &mut self.active_effects {
            let Some(row) = registry.get(id) else {
                continue;
            };
            // `applyEffectTick` reaches the whole world and none of the ten
            // effects that use it has a subsystem here — `keep` is its return
            // value and `true` is "do not remove me", which is what every one
            // of them answers on the paths this server can reach.
            //
            // The one exception is `minecraft:instant_health`, whose
            // `HealOrHarmMobEffect.applyEffectTick` (offsets 0..23, not
            // inverted for a player) is `heal(Math.max(4 << amplifier, 0))`
            // and touches nothing but this entity. `/effect give` reaches it,
            // and the survival corpus restores health with it.
            // `instant_damage` is the same method's other arm and is a
            // `hurtServer(magic)` — a level-half hurt this loop cannot emit
            // from here — so it stays unmodelled, as before.
            let heals = id == "minecraft:instant_health";
            let outcome = instance.tick_server(row.rule, tick_count, |_, amplifier| {
                if heals {
                    health.heal(f32_from_i32((4_i32 << (amplifier & 31)).max(0)));
                }
                true
            });
            if !outcome.keep {
                removed.push((id.clone(), row.registry_id));
            } else if instance.duration() % EFFECT_RESEND_PERIOD == 0 {
                resent.push(EffectPacket {
                    registry_id: row.registry_id,
                    amplifier: instance.amplifier(),
                    duration: instance.duration(),
                    flags: Self::flags_of(instance),
                });
            }
            if outcome.downgraded {
                downgraded.push(EffectPacket {
                    registry_id: row.registry_id,
                    amplifier: instance.amplifier(),
                    duration: instance.duration(),
                    flags: Self::flags_of(instance) | effect_flags::BLEND,
                });
            }
        }
        for packet in downgraded {
            out.push(Emit::UpdateMobEffect(packet));
        }
        for packet in resent {
            out.push(Emit::UpdateMobEffect(packet));
        }
        for (id, registry_id) in removed {
            self.active_effects.remove(&id);
            out.push(Emit::RemoveMobEffect { registry_id });
        }
    }

    /// How many hurts were refused for want of `damage_types.json`.
    #[must_use]
    pub const fn hurts_refused(&self) -> u32 {
        self.hurts_refused
    }

    /// The resolved damage tags, or `None`.
    #[must_use]
    pub const fn damage_tags(&self) -> Option<DamageTagSet> {
        self.damage
    }

    /// Override the tag set, for a test that wants the refusal path or a
    /// caller that has already loaded the registry.
    pub const fn set_damage_tags(&mut self, tags: Option<DamageTagSet>) {
        self.damage = tags;
    }

    /// Record that a caller declined to hurt this player because
    /// `data/vanilla/damage_types.json` was absent.
    ///
    /// Public because the fall path lives in [`crate::world_send`]: the
    /// refusal has to be counted wherever it is taken, or the count measures
    /// only the two arms inside this file.
    pub const fn refuse_hurt(&mut self) {
        self.hurts_refused = self.hurts_refused.saturating_add(1);
    }

    /// Read `minecraft:player`'s maxima out of `data/vanilla/attributes.json`
    /// when the table is loaded, and fall back to the constants when it is
    /// not.
    ///
    /// The fallback is not a silent one: [`DEFAULT_MAX_HEALTH`] is documented
    /// as the table's own value for a player, and a table that disagrees is a
    /// finding rather than a crash.
    #[must_use]
    pub fn from_attributes(attrs: Option<&crate::attributes::Attributes>) -> Self {
        let read = |id: &str, fallback: f32| -> f32 {
            attrs
                .and_then(|a| a.base("minecraft:player", id))
                .map_or(fallback, |v| {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "getAttributeValue's own d2f in getMaxHealth/getMaxAbsorption"
                    )]
                    {
                        v as f32
                    }
                })
        };
        Self::new(
            read("minecraft:max_health", DEFAULT_MAX_HEALTH),
            read("minecraft:max_absorption", DEFAULT_MAX_ABSORPTION),
        )
    }

    /// `ServerPlayer.doTick()`, in its own order.
    ///
    /// `next_double` is the entity's `RandomSource.nextDouble()`, which
    /// [`crate::breath::decrease_air_supply`] draws from when the player
    /// carries an `OXYGEN_BONUS`. It is a closure for the reason
    /// [`crate::weather`] takes one: the stream is shared and a private
    /// generator here would produce a plausible answer off a stream nothing
    /// else agrees with.
    pub fn do_tick(
        &mut self,
        level: LevelFacts,
        next_double: &mut impl FnMut() -> f64,
    ) -> Vec<Emit> {
        let mut out = Vec::new();

        // --- offset 15, `super.tick()`. Only the parts this server has ---
        //
        // `LivingEntity.baseTick`'s water branch, then the two counters. The
        // `invulnerableTime` decrement is `ServerPlayer.tick` offsets 21..38
        // and NOT `baseTick` offset 475, which skips it for a player; running
        // both halves gives a five-tick hurt cooldown instead of ten.
        let air = breath::tick_air(self.air_supply, level.breath, next_double);
        self.air_supply = air.air_supply;
        if air.drowned {
            // 307..328 — the broadcast happens whether or not the hurt lands,
            // because `broadcastEntityEvent` is at offset 318 and
            // `hurtServer` at 328. A port that gates the bubble burst on the
            // damage loses it inside the ten-tick cooldown, where vanilla
            // still sends it.
            out.push(Emit::EntityEvent(breath::DROWN_ENTITY_EVENT));
            match self.damage.map(|d| d.drown) {
                Some(tags) => {
                    self.hurt(breath::DROWN_DAMAGE, tags, level, false);
                }
                None => self.hurts_refused = self.hurts_refused.saturating_add(1),
            }
        }
        breath::tick_counters(
            &mut self.health.hurt_time,
            &mut self.health.invulnerable_time,
            true,
        );
        breath::tick_server_player_counters(&mut self.health.invulnerable_time);

        // `LivingEntity.baseTick` offsets 492..510, after the two counters:
        // `if (isDeadOrDying() && level.shouldTickDeath(this)) tickDeath()`.
        // `tickDeath` is `++deathTime`, then at 20 — and only once, because
        // `isRemoved()` is checked — the poof event and the removal.
        if self.health.is_dead_or_dying() {
            self.death_time = self.death_time.saturating_add(1);
            if self.death_time >= 20 && !self.removed {
                self.removed = true;
                out.push(Emit::DeathRemoval);
            }
        }
        // 513..535: the kill-credit memory ages, and is forgotten once spent.
        if self.last_hurt_by_player_memory > 0 {
            self.last_hurt_by_player_memory -= 1;
        } else {
            self.last_hurt_by_player = None;
        }

        // `LivingEntity.baseTick` offset 604, which is AFTER the air branch
        // (207..419) and after the two counters (451..491). The order is why
        // this is here and not at the top: an effect removed this tick has
        // already had the air and the cooldown ticked under it.
        self.tick_effects(&mut out);
        // `LivingEntity.tick` offsets 129-142, after `super.tick()`:
        // `if (tickCount % 20 == 0) combatTracker.recheckStatus()`.
        if self.tick_count % 20 == 0 {
            let alive = !self.health.is_dead_or_dying();
            if let Some(p) = self.combat.recheck(self.tick_count, alive) {
                out.push(Emit::Combat(p));
            }
        }

        // --- offset 42, `foodData.tick(this)` ---
        let action = self.food.tick(TickInputs {
            difficulty: level.difficulty,
            natural_regeneration: level.natural_regeneration,
            is_hurt: self.health.is_hurt(),
            health: self.health.health(),
        });
        match action {
            FoodAction::None => {}
            FoodAction::Heal(amount) => self.health.heal(amount),
            FoodAction::Starve => match self.damage.map(|d| d.starve) {
                Some(tags) => {
                    self.hurt(crate::food::consts::STARVE_DAMAGE, tags, level, false);
                }
                None => self.hurts_refused = self.hurts_refused.saturating_add(1),
            },
        }

        // --- offsets 145..267, the set_health change detector ---
        let inputs = self.sync_inputs();
        if let Some(packet) = self.sync.tick_health(inputs) {
            out.push(Emit::Health(packet));
        }

        // --- offsets 487..532, the set_experience change detector ---
        // Not here: the player's experience is the connection's one store
        // (`crate::experience`, written by `/experience` from the console's
        // thread), and the play loop sends from it with its own
        // `lastSentExp`. A second detector on a copy sent the join's packet
        // twice.

        self.tick_count = self.tick_count.wrapping_add(1);
        out
    }

    /// `LivingEntity.hurtServer(level, source, amount)`, with this player's
    /// own fields.
    ///
    /// `fall` is not a flag on the damage — it selects nothing here — it is
    /// the caller telling us it has already resolved the damage type's tags
    /// out of `data/vanilla/damage_types.json`. See
    /// [`LivePlayer::cause_fall_damage`].
    pub fn hurt(
        &mut self,
        amount: f32,
        tags: DamageTags,
        level: LevelFacts,
        invulnerable: bool,
    ) -> HurtOutcome {
        let ctx = HurtServerContext {
            sleeping: self.sleeping,
            hurt: HurtContext {
                invulnerable: invulnerable || level.abilities_invulnerable,
                ..HurtContext::default()
            },
            ..HurtServerContext::default()
        };
        let outcome = hurt::hurt_server(&mut self.health, amount, tags, ctx);
        if outcome.stopped_sleeping {
            self.sleeping = false;
        }
        outcome
    }

    /// `ServerPlayer.hurtServer` -> `Player.hurtServer` -> `LivingEntity.hurtServer`,
    /// the whole chain a `/damage` against a player runs.
    ///
    /// [`Self::hurt`] is the LivingEntity arm alone, which is what the three
    /// tick-driven hurts (fall, drown, starve) were wired to. For those three
    /// the layers above it answer the same — none of them scales on
    /// difficulty and none has a cause — but a console hit can be anything,
    /// and the layers are where `explosion` becomes nothing on peaceful.
    ///
    /// ```text
    ///   ServerPlayer.hurtServer
    ///      0  isInvulnerableTo                       -> false
    ///     11  source.getEntity() instanceof Player
    ///            && !canHarmPlayer(it)               -> false   (pvp rule, teams)
    ///     91  Player.hurtServer
    ///            0  isInvulnerableTo                 -> false
    ///           11  abilities.invulnerable
    ///                && !BYPASSES_INVULNERABILITY   -> false
    ///           33  noActionTime = 0
    ///           38  isDeadOrDying                    -> false
    ///           51  scalesWithDifficulty -> PEACEFUL 0 / EASY min(d/2+1, d) / HARD d*3/2
    ///          108  damage == 0.0f                   -> false
    ///          116  LivingEntity.hurtServer
    /// ```
    ///
    /// `None` is a `false` returned above `LivingEntity` — **nothing** moved,
    /// no cooldown started — which is different in kind from a refusal inside
    /// it, and `crate::damagecmd`'s peaceful check exists to see the
    /// difference.
    ///
    /// Then `Player.actuallyHurt` offsets 89..101: a hit that took any health
    /// charges `source.getFoodExhaustion()` through `causeFoodExhaustion`.
    pub fn hurt_player(
        &mut self,
        amount: f32,
        ty: &crate::damage_types::DamageType,
        caused_by_player: bool,
        causing_is_living_non_player: bool,
        level: LevelFacts,
        rules: &crate::gamerules::GameRules,
    ) -> Option<HurtOutcome> {
        // `isInvulnerableTo`, both layers: the four damage game rules. The
        // base answer (removed, `/data` invulnerable, fire-immune type) is
        // false for a live survival player.
        if crate::damagecmd::blocked_by_game_rule(ty, rules) {
            return None;
        }
        // `canHarmPlayer`: `ServerLevel.isPvpAllowed()` is the `pvp` game
        // rule in 26.2, then `Player.canHarmPlayer`'s team test, which with
        // no teams on this server is true.
        if caused_by_player
            && matches!(
                rules.get("pvp"),
                Some(crate::gamerules::GameRuleValue::Bool(false))
            )
        {
            return None;
        }
        let tags = ty.tags();
        if level.abilities_invulnerable && !ty.tags.contains("minecraft:bypasses_invulnerability") {
            return None;
        }
        self.health.no_action_time = 0;
        if self.health.is_dead_or_dying() {
            return None;
        }
        let mut amount = amount;
        if crate::damagecmd::scales_with_difficulty(ty, causing_is_living_non_player) {
            amount = crate::damagecmd::scale_for_difficulty(amount, level.difficulty);
        }
        if amount == 0.0 {
            return None;
        }
        // `LivingEntity.hurtServer`'s own `isInvulnerableTo` is the base
        // answer above, false here — NOT `abilities.invulnerable`, which is
        // `Player.hurtServer`'s test and has already been passed. So a
        // creative player killed by `generic_kill` reaches health, where
        // [`Self::hurt`] (which folds the abilities flag in for the three
        // tick-driven hurts) would refuse it.
        // The two effects `hurtServer` and `getDamageAfterMagicAbsorb` read:
        // Fire Resistance at offset 20, Resistance's amplifier in the absorb.
        let ctx = HurtServerContext {
            sleeping: self.sleeping,
            fire_resistance: self.has_effect("minecraft:fire_resistance"),
            hurt: HurtContext {
                resistance_amplifier: self
                    .effect("minecraft:resistance")
                    .map(MobEffectInstance::amplifier),
                ..HurtContext::default()
            },
            ..HurtServerContext::default()
        };
        let outcome = hurt::hurt_server(&mut self.health, amount, tags, ctx);
        if outcome.stopped_sleeping {
            self.sleeping = false;
        }
        // `LivingEntity.actuallyHurt` 111-124: `recordDamage` when the damage
        // after absorption is not zero — before `setHealth`, so `isAlive()`
        // reads the health before this hit.
        if outcome.health_lost != 0.0 {
            let packets = self.combat.record_damage(
                self.tick_count,
                true,
                caused_by_player || causing_is_living_non_player,
            );
            self.combat_out.extend(packets);
        }
        if outcome.health_lost != 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "DamageType.exhaustion is a float in the codec; the table widened it"
            )]
            let exhaustion = ty.exhaustion as f32;
            crate::movestats::cause_food_exhaustion(
                &mut self.food,
                level.abilities_invulnerable,
                exhaustion,
            );
        }
        Some(outcome)
    }

    /// The six fields the two change detectors compare.
    fn sync_inputs(&self) -> SyncInputs {
        SyncInputs {
            health: self.health.health(),
            food_level: self.food.food_level(),
            saturation: self.food.saturation_level(),
            experience_progress: self.experience.progress(),
            total_experience: self.experience.total(),
            experience_level: self.experience.level(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Emit, LevelFacts, LivePlayer};
    use crate::effects::MobEffectInstance;
    use crate::food::Difficulty;

    fn no_random() -> impl FnMut() -> f64 {
        || panic!("the air tick drew from the entity random with no OXYGEN_BONUS")
    }

    #[test]
    fn the_first_tick_after_a_join_sends_both_packets_and_the_second_sends_neither() {
        let mut p = LivePlayer::default();
        let out = p.do_tick(LevelFacts::default(), &mut no_random());
        assert!(matches!(out.first(), Some(Emit::Health(_))), "{out:?}");
        // The experience half is the play loop's, off the connection's store.
        assert_eq!(out.len(), 1);

        // The sentinels are spent; nothing has moved.
        let out = p.do_tick(LevelFacts::default(), &mut no_random());
        assert_eq!(out, vec![], "a quiet tick is silent");
    }

    #[test]
    fn set_health_carries_the_food_that_the_same_tick_changed() {
        // The join-order claim in this module's docs, tested: food is ticked
        // at offset 42 and the detector reads it at offset 145, so a level
        // that drops on tick n is on the wire on tick n.
        //
        // NORMAL and not the substrate's PEACEFUL: the food-level decrement
        // at `FoodData.tick` offsets 10..78 is guarded by
        // `difficulty != PEACEFUL`. On peaceful this test would measure
        // nothing at all.
        let normal = LevelFacts {
            difficulty: Difficulty::Normal,
            natural_regeneration: false,
            ..LevelFacts::default()
        };
        let mut p = LivePlayer::default();
        p.food.set_food_level(3);
        p.food.set_saturation(0.0);
        // Two ticks to flush: the first spends the join sentinels, the second
        // settles the manual writes above. Without them the packet below
        // would be caused by `set_food_level` and not by the drop, which is a
        // check that measures its own input.
        p.do_tick(normal, &mut no_random());
        p.do_tick(normal, &mut no_random());

        // `> EXHAUSTION_DROP`, strictly — exactly 4.0 does not spend.
        p.food.add_exhaustion(4.5);
        let out = p.do_tick(normal, &mut no_random());
        let Some(Emit::Health(h)) = out.first() else {
            panic!("no set_health: {out:?}");
        };
        assert_eq!(h.food, 2, "the packet carries the post-tick level");
        assert_eq!(p.food.food_level(), 2);

        // The positive control on the flush: with no exhaustion added, the
        // same tick is silent.
        assert_eq!(p.do_tick(normal, &mut no_random()), vec![]);
    }

    fn damage_type(
        scaling: &str,
        exhaustion: f64,
        tags: &[&str],
    ) -> crate::damage_types::DamageType {
        crate::damage_types::DamageType {
            message_id: "x".into(),
            scaling: scaling.into(),
            effects: "hurt".into(),
            death_message_type: "default".into(),
            exhaustion,
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    #[test]
    fn an_always_scaling_hit_on_peaceful_returns_above_living_entity_and_starts_no_cooldown() {
        let rules = crate::gamerules::GameRules::fresh();
        let mut p = LivePlayer::default();
        let explosion = damage_type("always", 0.1, &[]);
        // `LevelFacts::default()` is NORMAL; the corpus substrate is peaceful.
        let peaceful = LevelFacts {
            difficulty: Difficulty::Peaceful,
            ..LevelFacts::default()
        };
        let out = p.hurt_player(4.0, &explosion, false, false, peaceful, &rules);
        assert!(out.is_none(), "Player.hurtServer offset 115 returns false");
        assert_eq!(p.health.invulnerable_time, 0, "no cooldown started");
        assert!((p.health.health() - 20.0).abs() < f32::EPSILON);
        // The positive control: the same hit on NORMAL lands, full.
        let normal = LevelFacts {
            difficulty: Difficulty::Normal,
            ..LevelFacts::default()
        };
        let out = p
            .hurt_player(4.0, &explosion, false, false, normal, &rules)
            .expect("lands");
        assert!(out.full_hit && out.broadcast_damage_event);
        assert!((p.health.health() - 16.0).abs() < f32::EPSILON);
        assert!(
            (p.food.exhaustion_level() - 0.1).abs() < 1e-6,
            "Player.actuallyHurt charges the type's exhaustion"
        );
    }

    #[test]
    fn resistance_takes_a_fifth_per_level_and_fire_resistance_refuses_fire() {
        let rules = crate::gamerules::GameRules::fresh();
        let generic = damage_type("when_caused_by_living_non_player", 0.0, &[]);
        let mut p = LivePlayer::default();
        if p.add_effect(MobEffectInstance::new("minecraft:resistance", 600, 0))
            .is_none()
        {
            // No `mob_effects.json`: the effect cannot be added, and saying
            // so beats a pass that measured nothing.
            panic!("data/vanilla/mob_effects.json is required for this test");
        }
        let out = p
            .hurt_player(5.0, &generic, false, false, LevelFacts::default(), &rules)
            .expect("lands");
        // `getDamageAfterMagicAbsorb`: (amp + 1) * 5 = 5, 25 - 5 = 20,
        // 5 * 20 / 25 = 4.
        assert!((out.health_lost - 4.0).abs() < 1e-6, "{out:?}");
        assert!((p.health.health() - 16.0).abs() < 1e-6);

        let fire = damage_type("never", 0.0, &["minecraft:is_fire"]);
        let mut q = LivePlayer::default();
        q.add_effect(MobEffectInstance::new("minecraft:fire_resistance", 600, 0))
            .expect("added");
        let out = q
            .hurt_player(3.0, &fire, false, false, LevelFacts::default(), &rules)
            .expect("reaches LivingEntity");
        assert_eq!(out.refused, Some(crate::hurt::HurtRefused::FireResistant));
    }

    #[test]
    fn instant_health_heals_four_shifted_by_the_amplifier_on_its_one_tick() {
        let mut p = LivePlayer::default();
        p.health.set_health(3.0);
        p.add_effect(MobEffectInstance::with_flags(
            "minecraft:instant_health",
            1,
            2,
            false,
            false,
            false,
        ))
        .expect("added — needs data/vanilla/mob_effects.json");
        let out = p.do_tick(LevelFacts::default(), &mut no_random());
        assert!((p.health.health() - 19.0).abs() < 1e-6, "3 + (4 << 2)");
        assert!(
            out.iter()
                .any(|e| matches!(e, Emit::RemoveMobEffect { .. })),
            "{out:?}"
        );
        // Amplifier 4 is 64, capped at max health — the corpus restore.
        p.health.set_health(1.0);
        p.add_effect(MobEffectInstance::with_flags(
            "minecraft:instant_health",
            1,
            4,
            false,
            false,
            false,
        ));
        p.do_tick(LevelFacts::default(), &mut no_random());
        assert!((p.health.health() - 20.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_hit_is_refused_by_player_and_a_game_rule_blocks_its_tag() {
        let rules = crate::gamerules::GameRules::fresh();
        let peaceful = LevelFacts {
            difficulty: Difficulty::Peaceful,
            ..LevelFacts::default()
        };
        let mut p = LivePlayer::default();
        let generic = damage_type("when_caused_by_living_non_player", 0.0, &[]);
        assert!(p
            .hurt_player(0.0, &generic, false, false, peaceful, &rules)
            .is_none());
        assert_eq!(p.health.invulnerable_time, 0);
        // Not scaled: no causing entity, so peaceful leaves it alone.
        assert!(p
            .hurt_player(2.0, &generic, false, false, peaceful, &rules)
            .is_some());
        let fire = damage_type("never", 0.0, &["minecraft:is_fire"]);
        rules.set("fire_damage", "false").expect("rule");
        let mut q = LivePlayer::default();
        assert!(q
            .hurt_player(2.0, &fire, false, false, peaceful, &rules)
            .is_none());
        rules.set("pvp", "false").expect("rule");
        assert!(
            q.hurt_player(2.0, &generic, true, false, peaceful, &rules)
                .is_none(),
            "canHarmPlayer reads the pvp rule"
        );
    }

    #[test]
    fn natural_regeneration_heals_and_the_gamerule_can_stop_it() {
        let mut p = LivePlayer::default();
        p.health.set_health(10.0);
        p.food.set_food_level(20);
        p.food.set_saturation(20.0);
        for _ in 0..12 {
            p.do_tick(LevelFacts::default(), &mut no_random());
        }
        let healed = p.health.health();
        assert!(
            healed > 10.0,
            "saturated regeneration did nothing: {healed}"
        );

        // The positive control: the same twelve ticks with the gamerule off.
        let mut q = LivePlayer::default();
        q.health.set_health(10.0);
        q.food.set_food_level(20);
        q.food.set_saturation(20.0);
        for _ in 0..12 {
            q.do_tick(
                LevelFacts {
                    natural_regeneration: false,
                    ..LevelFacts::default()
                },
                &mut no_random(),
            );
        }
        assert!(
            (q.health.health() - 10.0).abs() < f32::EPSILON,
            "the gamerule did not stop it: {}",
            q.health.health()
        );
    }

    #[test]
    fn starving_on_peaceful_never_reaches_the_hurt_and_on_hard_it_does() {
        let run = |difficulty: Difficulty| {
            let mut p = LivePlayer::default();
            p.food.set_food_level(0);
            p.food.set_saturation(0.0);
            for _ in 0..4_000 {
                p.do_tick(
                    LevelFacts {
                        difficulty,
                        natural_regeneration: false,
                        ..LevelFacts::default()
                    },
                    &mut no_random(),
                );
            }
            p.health.health()
        };
        // 4,000 ticks is fifty starve hits at one every eighty.
        assert!(
            (run(Difficulty::Hard) - 0.0).abs() < f32::EPSILON,
            "hard has no floor"
        );
        let peaceful = run(Difficulty::Peaceful);
        assert!(
            (peaceful - 10.0).abs() < f32::EPSILON,
            "peaceful should stop at exactly ten, got {peaceful}"
        );
        assert!(
            (run(Difficulty::Easy) - 10.0).abs() < f32::EPSILON,
            "and so should easy"
        );
        // NORMAL's third disjunct takes it one further, to 1.0.
        let normal = run(Difficulty::Normal);
        assert!(
            (normal - 1.0).abs() < f32::EPSILON,
            "normal should stop at one, got {normal}"
        );
    }

    #[test]
    fn the_invulnerable_time_is_decremented_exactly_once_per_tick() {
        // `baseTick` offset 475 skips it for a ServerPlayer and
        // `ServerPlayer.tick` offsets 21..38 does it instead. Running both
        // halves halves the hurt cooldown; running neither makes a hit
        // permanent.
        let mut p = LivePlayer::default();
        p.health.invulnerable_time = 20;
        p.do_tick(LevelFacts::default(), &mut no_random());
        assert_eq!(p.health.invulnerable_time, 19);
        p.do_tick(LevelFacts::default(), &mut no_random());
        assert_eq!(p.health.invulnerable_time, 18);
    }

    #[test]
    fn damage_tags_are_read_from_the_table_and_none_bypasses_the_cooldown() {
        let Some(set) = super::DamageTagSet::shared() else {
            // The table is gitignored and generated; a tree without it is the
            // documented `None` path, which the next test exercises.
            return;
        };
        for (id, tags) in [
            ("minecraft:fall", set.fall),
            ("minecraft:drown", set.drown),
            ("minecraft:starve", set.starve),
        ] {
            assert!(tags.bypasses_armor, "{id} should bypass armour");
            assert!(
                !tags.bypasses_cooldown,
                "{id} bypasses the cooldown, which changes the difference rule"
            );
        }
        // The three are not the same row: `starve` bypasses effects and the
        // other two do not, `drown` has `no_impact` and the other two do not.
        // Without this the test above would pass against a set built from one
        // row copied three times.
        assert!(set.starve.bypasses_effects);
        assert!(!set.fall.bypasses_effects);
        assert!(set.drown.no_impact);
        assert!(!set.fall.no_impact);
    }

    #[test]
    fn without_the_damage_table_a_starving_player_is_not_hurt_and_the_refusal_is_counted() {
        let mut p = LivePlayer::default();
        p.set_damage_tags(None);
        p.food.set_food_level(0);
        p.food.set_saturation(0.0);
        for _ in 0..200 {
            p.do_tick(
                LevelFacts {
                    difficulty: Difficulty::Hard,
                    ..LevelFacts::default()
                },
                &mut no_random(),
            );
        }
        assert!((p.health.health() - 20.0).abs() < f32::EPSILON);
        assert_eq!(
            p.hurts_refused(),
            2,
            "200 ticks is two starve hits at one every eighty, and a refusal \
             that is not counted is indistinguishable from a player who was \
             never hurt"
        );
        // The vacuity guard belongs to the creative test below, not here:
        // this one has deliberately set the tag set to `None`, so
        // `damage_tags()` is `None` by construction and asserting otherwise
        // would be asserting the opposite of the arm under test.
    }

    #[test]
    fn a_creative_player_cannot_be_hurt_by_anything_this_tick_produces() {
        let mut p = LivePlayer::default();
        p.food.set_food_level(0);
        p.food.set_saturation(0.0);
        let creative = LevelFacts {
            difficulty: Difficulty::Hard,
            abilities_invulnerable: true,
            ..LevelFacts::default()
        };
        for _ in 0..200 {
            p.do_tick(creative, &mut no_random());
        }
        assert!((p.health.health() - 20.0).abs() < f32::EPSILON);
        // **The vacuity guard.** Without it this test passes on a tree where
        // `data/vanilla/damage_types.json` did not load, because then EVERY
        // hurt is refused and creative has nothing to do with it. The two
        // candidate paths are named so a failure says which one was looked at
        // rather than "no table".
        assert!(
            p.damage_tags().is_some(),
            "the damage table did not load, so this test would pass for the wrong reason. \
             Tried {:?} then {:?}",
            std::path::Path::new("data/vanilla/damage_types.json"),
            super::table_path("damage_types.json"),
        );
        assert_eq!(
            p.hurts_refused(),
            0,
            "refused by invulnerability, not by the table"
        );
        // The positive control is `starvation_stops_at_ten_health_...`
        // above, which takes the same ticks on Hard in survival and dies.
    }

    #[test]
    fn the_effect_registry_reads_forty_effects_and_every_class_has_a_rule() {
        let Some(reg) = crate::effects::EffectRegistry::shared() else {
            panic!(
                "data/vanilla/mob_effects.json did not load; tried {:?} then {:?}",
                std::path::Path::new(crate::effects::TABLE_PATH),
                super::table_path("mob_effects.json"),
            );
        };
        // The count the table's own generator printed. A registry that parsed
        // one row would satisfy every other assertion in this file.
        assert_eq!(reg.len(), 40);
        // Two effects with different registry ids, so an implementation that
        // returned a constant id would red here rather than on the wire.
        let regen = reg.get("minecraft:regeneration").expect("regeneration");
        let poison = reg.get("minecraft:poison").expect("poison");
        assert_ne!(regen.registry_id, poison.registry_id);
        assert_eq!(reg.get("minecraft:not_an_effect"), None);
    }

    #[test]
    fn an_added_effect_is_announced_with_blend_and_a_resend_is_not() {
        let mut p = LivePlayer::default();
        assert!(
            p.effect_registry.is_some(),
            "no mob_effects table; this test would be vacuous"
        );
        // 600 ticks so the first `duration % 600 == 0` resend is reachable
        // inside a short loop. `tickServer` decrements first, so the duration
        // is 599 after one tick and 0 after 600.
        let added = p
            .add_effect(MobEffectInstance::new("minecraft:regeneration", 1_200, 0))
            .expect("the add is announced");
        let super::Emit::UpdateMobEffect(packet) = added else {
            panic!("wrong emit: {added:?}");
        };
        assert_eq!(packet.duration, 1_200);
        assert_eq!(
            packet.flags & super::effect_flags::BLEND,
            super::effect_flags::BLEND,
            "onEffectAdded passes iconst_1 for blend"
        );
        assert_eq!(
            packet.flags & super::effect_flags::VISIBLE,
            super::effect_flags::VISIBLE
        );

        // Drive to the first resend: the duration reaches 600 after 600
        // ticks, and `duration % 600 == 0` fires then.
        let mut resends = Vec::new();
        for _ in 0..600 {
            for emit in p.do_tick(LevelFacts::default(), &mut no_random()) {
                if let super::Emit::UpdateMobEffect(e) = emit {
                    resends.push(e);
                }
            }
        }
        assert_eq!(resends.len(), 1, "one resend in 600 ticks: {resends:?}");
        assert_eq!(resends[0].duration, 600);
        assert_eq!(
            resends[0].flags & super::effect_flags::BLEND,
            0,
            "onEffectUpdated passes iconst_0 for blend — this is the bit a \
             port that derives the flags from the instance gets wrong"
        );
    }

    #[test]
    fn an_effect_that_runs_out_is_removed_and_the_removal_is_not_an_update() {
        let mut p = LivePlayer::default();
        assert!(p.effect_registry.is_some(), "no mob_effects table");
        // **A duration of exactly 600 gets NO resend, and that is the
        // reading.** `tickServer` decrements before `tickEffects` tests
        // `duration % 600 == 0`, so the only tick on which this effect's
        // duration is a multiple of 600 is the tick it reaches ZERO — and
        // zero means `hasRemainingDuration()` is false, which takes the
        // removal branch instead. The two branches are `else`-exclusive at
        // `tickEffects` offsets 81..122 and the removal wins.
        //
        // The consequence a port gets wrong in the other direction: an effect
        // given `/effect give @s poison 30` (600 ticks) is announced once and
        // never re-announced, while one given 1,200 is re-announced exactly
        // once, at duration 600. `an_added_effect_is_announced_with_blend_...`
        // above is that second case.
        p.add_effect(MobEffectInstance::new("minecraft:poison", 600, 0));
        let mut removals = 0;
        let mut updates = 0;
        for _ in 0..700 {
            for emit in p.do_tick(LevelFacts::default(), &mut no_random()) {
                match emit {
                    super::Emit::RemoveMobEffect { .. } => removals += 1,
                    super::Emit::UpdateMobEffect(_) => updates += 1,
                    _ => {}
                }
            }
        }
        assert_eq!(removals, 1, "exactly one removal");
        assert_eq!(
            updates, 0,
            "and NO resend: the decrement runs first, so duration 600 is only \
             ever a multiple of 600 on the tick it reaches zero"
        );
        assert!(!p.has_effect("minecraft:poison"));

        // The positive control: an effect long enough to survive the same 700
        // ticks is never removed.
        let mut q = LivePlayer::default();
        q.add_effect(MobEffectInstance::new("minecraft:poison", 6_000, 0));
        let mut removals = 0;
        for _ in 0..700 {
            for emit in q.do_tick(LevelFacts::default(), &mut no_random()) {
                if matches!(emit, super::Emit::RemoveMobEffect { .. }) {
                    removals += 1;
                }
            }
        }
        assert_eq!(removals, 0);
        assert!(q.has_effect("minecraft:poison"));
    }

    #[test]
    fn removing_an_effect_that_is_not_there_says_nothing() {
        let mut p = LivePlayer::default();
        assert_eq!(p.remove_effect("minecraft:regeneration"), None);
        // The positive control.
        p.add_effect(MobEffectInstance::new("minecraft:regeneration", 100, 0));
        assert!(matches!(
            p.remove_effect("minecraft:regeneration"),
            Some(super::Emit::RemoveMobEffect { .. })
        ));
        assert!(!p.has_effect("minecraft:regeneration"));
    }
}
