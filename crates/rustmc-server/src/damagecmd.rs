//! `/damage` — `net.minecraft.server.commands.DamageCommand` — read off the
//! server's console, and the layer of `Player.hurtServer` /
//! `ServerPlayer.hurtServer` that sits above `crate::hurt`.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `DamageCommand.register` and its five lambdas, `DamageCommand.damage`,
//! `ServerPlayer.hurtServer`, `ServerPlayer.isInvulnerableTo`,
//! `Player.hurtServer`, `Player.isInvulnerableTo`, `Player.actuallyHurt`,
//! `DamageSource.scalesWithDifficulty`, `DamageSource.getSourcePosition`.
//!
//! # Why this is the harness's damage verb
//!
//! A corpus scenario had no way to hurt a player on both servers by the same
//! cause. `Step::Attack` cannot target its own actor, and on the creative
//! substrate every player is invulnerable. `/damage` is permission level 2
//! (`requires(Commands.LEVEL_GAMEMASTERS)` at offset 6), which the harness's
//! players do not have, but the console does:
//! `MinecraftServer.createCommandSourceStack()` carries full permission. So a
//! survival scenario writes `do = "console"`, `text = "damage oracle 2
//! minecraft:generic"`, and both servers run `Entity.hurtServer` with the same
//! `DamageSource`. That is the whole verb; everything below is making our half
//! of it the same method vanilla's is.
//!
//! # The grammar, and the source each branch builds
//!
//! ```text
//!   damage <target:entity> <amount:float(min 0)>                  lambda$0
//!          -> damageSources().generic()
//!          [<damageType:resource(damage_type)>                     lambda$1
//!          -> new DamageSource(type)
//!             [at <location:vec3>]                                 lambda$2
//!          -> new DamageSource(type, location)
//!             [by <entity:entity>                                  lambda$3
//!          -> new DamageSource(type, entity)            direct = causing = entity
//!                [from <cause:entity>]]]                           lambda$4
//!          -> new DamageSource(type, entity, cause)     direct = entity, causing = cause
//! ```
//!
//! `damage(source, target, amount, damageSource)` is `target.hurtServer(level,
//! damageSource, amount)` and either `sendSuccess` or `ERROR_INVULNERABLE`.
//! Both answers go to the console's own log and to operators; the harness has
//! no operators, so **neither answer reaches a client**, on either server.
//!
//! # What is refused, by name
//!
//! Selectors (`@p`, `@e[...]`), UUIDs, and relative or local coordinates in
//! `at`. The console's own position is the overworld spawn, so `~` would mean
//! something no scenario could see; a selector would need an entity query this
//! server does not have. Each is a parse error naming the form rather than a
//! guess at what was meant.

use std::sync::{Mutex, OnceLock};

use crate::damage_types::{DamageType, DamageTypeRegistry};

/// The registry whose numbering `damage_event`'s `source` field carries.
pub const REGISTRY: &str = "minecraft:damage_type";

/// `DamageSources.generic()` — the type `lambda$register$0` uses when the
/// command names none.
pub const GENERIC: &str = "minecraft:generic";

/// Where a hit comes from, as `DamageCommand` builds it.
#[derive(Debug, Clone, PartialEq)]
pub enum Source<E> {
    /// `new DamageSource(type)`: no entity, no position.
    Plain,
    /// `new DamageSource(type, Vec3)`: a position and no entity.
    At([f64; 3]),
    /// `new DamageSource(type, direct, causing)`. `lambda$3` passes the same
    /// entity twice.
    By { direct: E, causing: E },
}

/// One parsed `/damage` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub target: String,
    pub amount: f32,
    /// Always namespaced: `ResourceArgument` reads `generic` as
    /// `minecraft:generic`.
    pub damage_type: String,
    pub source: Source<String>,
}

/// A hit waiting for its target's next tick, entities resolved to ids.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    pub amount: f32,
    pub damage_type: String,
    /// Entity ids, resolved when the line was read — vanilla resolves
    /// `EntityArgument`s when the command executes, which is the same tick.
    pub source: Source<i32>,
    /// The game time when the console read the line.
    ///
    /// `MinecraftServer` queues console input on its reader thread and
    /// `DedicatedServer.tickConnection` drains it at the end of the tick in
    /// progress, after every player's `doTick`. The clock advances and then
    /// runs the level tick, so a line read while it reads `T` is run at the
    /// end of `T`. The target's loop catches up on ticks in batches, so a hit
    /// is applied in the batch's iteration for this tick — not at whatever
    /// tick the batch happens to start on. (It was `T + 1` until a recording
    /// showed every hit one tick late against vanilla's 50 ms.) Without the tag, three lines read
    /// 100 ms apart landed in ONE simulated tick: one `set_health` where
    /// vanilla sends two, measured on `008` 2026-09-30.
    pub at_tick: i64,
}

/// Parse one console line that starts with `damage`.
///
/// # Errors
/// A message naming what is wrong, in the shape Brigadier's would be; never a
/// default in place of an argument that did not parse.
pub fn parse(line: &str) -> Result<Command, String> {
    let line = line.strip_prefix('/').unwrap_or(line);
    let mut it = line.split_whitespace();
    if it.next() != Some("damage") {
        return Err(format!("not a damage command: {line:?}"));
    }
    let target = entity_name(it.next().ok_or("damage: missing <target>")?)?;
    let amount = float_min_zero(it.next().ok_or("damage: missing <amount>")?)?;
    let Some(ty) = it.next() else {
        return Ok(Command {
            target,
            amount,
            damage_type: GENERIC.to_string(),
            source: Source::Plain,
        });
    };
    let damage_type = resource(ty)?;
    let source = match it.next() {
        None => Source::Plain,
        Some("at") => {
            let mut xyz = [0.0f64; 3];
            for (axis, slot) in xyz.iter_mut().enumerate() {
                let tok = it.next().ok_or("damage: `at` needs three coordinates")?;
                *slot = coordinate(tok, axis != 1)?;
            }
            Source::At(xyz)
        }
        Some("by") => {
            let direct = entity_name(it.next().ok_or("damage: `by` needs an entity")?)?;
            let causing = match it.next() {
                None => direct.clone(),
                Some("from") => entity_name(it.next().ok_or("damage: `from` needs an entity")?)?,
                Some(other) => return Err(format!("damage: expected `from`, found {other:?}")),
            };
            Source::By { direct, causing }
        }
        Some(other) => return Err(format!("damage: expected `at` or `by`, found {other:?}")),
    };
    if let Some(extra) = it.next() {
        return Err(format!("damage: trailing argument {extra:?}"));
    }
    Ok(Command {
        target,
        amount,
        damage_type,
        source,
    })
}

/// A player name, which is the one `EntityArgument` form this server can
/// resolve. `EntitySelectorParser` reads a bare word as a name.
fn entity_name(tok: &str) -> Result<String, String> {
    if tok.starts_with('@') {
        return Err(format!(
            "damage: selector {tok:?} refused — this console resolves player names only"
        ));
    }
    if tok.contains('-') {
        return Err(format!(
            "damage: {tok:?} looks like a UUID — this console resolves player names only"
        ));
    }
    Ok(tok.to_string())
}

/// `FloatArgumentType.floatArg(0.0f)`: Brigadier's `StringReader.readFloat`
/// takes the run of `[0-9.-]` and hands it to `Float.parseFloat`, then the
/// argument type refuses anything below its minimum.
fn float_min_zero(tok: &str) -> Result<f32, String> {
    if tok.is_empty()
        || !tok
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return Err(format!("damage: invalid float {tok:?}"));
    }
    let v: f32 = tok
        .parse()
        .map_err(|_| format!("damage: invalid float {tok:?}"))?;
    if v < 0.0 {
        return Err(format!("Float must not be less than 0.0, found {v}"));
    }
    Ok(v)
}

/// `ResourceArgument`: an `Identifier`, `minecraft:` when unqualified.
fn resource(tok: &str) -> Result<String, String> {
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.' | '/')
            })
    };
    let (ns, path) = tok.split_once(':').unwrap_or(("minecraft", tok));
    if !ok(ns) || !ok(path) {
        return Err(format!("damage: invalid resource location {tok:?}"));
    }
    Ok(format!("{ns}:{path}"))
}

/// One absolute `WorldCoordinate`. `Vec3Argument.vec3()` is
/// `centerCorrect = true`, so an integer x or z is the block's centre
/// (`+0.5`) and an integer y is not.
fn coordinate(tok: &str, center: bool) -> Result<f64, String> {
    if tok.starts_with('~') || tok.starts_with('^') {
        return Err(format!(
            "damage: relative coordinate {tok:?} refused — the console's position is the \
             overworld spawn, which no scenario can see"
        ));
    }
    let v: f64 = tok
        .parse()
        .map_err(|_| format!("damage: invalid coordinate {tok:?}"))?;
    Ok(if center && !tok.contains('.') {
        v + 0.5
    } else {
        v
    })
}

/// Hits read off the console and not yet taken by their target's loop, keyed
/// by connection id.
///
/// The console thread cannot reach a player's `LivePlayer` — it lives on that
/// player's own loop — so the line is resolved here and the hurt runs there,
/// on the next tick boundary. `DedicatedServer.tickConnection` runs
/// `handleConsoleInputs` AFTER `MinecraftServer.tickConnection`, i.e. after
/// every `ServerPlayer.doTick` of the tick; the loop reproduces that order by
/// taking these after its own `do_tick`.
fn queue() -> &'static Mutex<Vec<(u64, Act)>> {
    static Q: OnceLock<Mutex<Vec<(u64, Act)>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// One console line's effect on one player, in the order the console read
/// them — a hit and an effect read in the same tick apply in their own
/// order, as `handleConsoleInputs` runs them.
#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// `/damage` or `/kill`.
    Hurt(Pending),
    /// `/effect give`.
    Effect(PendingEffect),
}

impl Act {
    const fn at_tick(&self) -> i64 {
        match self {
            Self::Hurt(p) => p.at_tick,
            Self::Effect(e) => e.at_tick,
        }
    }
}

/// `EffectCommands.giveEffect`'s `MobEffectInstance`, waiting for its target.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingEffect {
    pub effect: String,
    /// In ticks, already converted — see [`effect_duration`].
    pub duration: i32,
    pub amplifier: i32,
    /// `visible` — the command's `!hideParticles`; `showIcon` follows it
    /// through the five-argument constructor.
    pub visible: bool,
    pub at_tick: i64,
}

/// Queue a hit for connection `conn`.
pub fn submit(conn: u64, hit: Pending) {
    queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((conn, Act::Hurt(hit)));
}

/// Queue an effect for connection `conn`.
pub fn submit_effect(conn: u64, effect: PendingEffect) {
    queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((conn, Act::Effect(effect)));
}

/// `effect give <target> <effect> [<seconds> [<amplifier> [<hideParticles>]]]`.
///
/// Returns (target, effect id, seconds or `None`, amplifier, visible).
/// Seconds `integer(1, 1000000)` or the `infinite` literal (`-1`),
/// amplifier `integer(0, 255)`.
///
/// # Errors
/// The message the console prints.
#[allow(clippy::type_complexity)]
pub fn parse_effect_give(line: &str) -> Result<(String, String, Option<i32>, i32, bool), String> {
    let line = line.strip_prefix('/').unwrap_or(line);
    let mut it = line.split_whitespace();
    if (it.next(), it.next()) != (Some("effect"), Some("give")) {
        return Err(format!("not an effect give: {line:?}"));
    }
    let target = entity_name(it.next().ok_or("effect: missing <targets>")?)?;
    let effect = resource(it.next().ok_or("effect: missing <effect>")?)?;
    let seconds = match it.next() {
        None => None,
        Some("infinite") => Some(-1),
        Some(t) => {
            let v: i32 = t
                .parse()
                .map_err(|_| format!("effect: invalid integer {t:?}"))?;
            if !(1..=1_000_000).contains(&v) {
                return Err(format!("effect: seconds out of range: {v}"));
            }
            Some(v)
        }
    };
    let amplifier = match it.next() {
        None => 0,
        Some(t) => {
            let v: i32 = t
                .parse()
                .map_err(|_| format!("effect: invalid integer {t:?}"))?;
            if !(0..=255).contains(&v) {
                return Err(format!("effect: amplifier out of range: {v}"));
            }
            v
        }
    };
    let visible = match it.next() {
        None | Some("false") => true,
        Some("true") => false,
        Some(t) => return Err(format!("effect: invalid boolean {t:?}")),
    };
    if let Some(extra) = it.next() {
        return Err(format!("effect: trailing argument {extra:?}"));
    }
    Ok((target, effect, seconds, amplifier, visible))
}

/// The duration `EffectCommands.giveEffect` hands the instance, in ticks —
/// offsets 11..75: with `seconds`, an instantaneous effect takes it as TICKS
/// and anything else as seconds times 20, `-1` staying `-1`; without, an
/// instantaneous effect lasts 1 tick and anything else 600.
#[must_use]
pub const fn effect_duration(seconds: Option<i32>, instantaneous: bool) -> i32 {
    match (seconds, instantaneous) {
        (Some(s), true) => s,
        (Some(-1), false) => -1,
        (Some(s), false) => s * 20,
        (None, true) => 1,
        (None, false) => 600,
    }
}

/// The console's handling of an `effect give` line.
///
/// # Errors
/// The message the console prints; nothing is queued on an error.
pub fn dispatch_effect(
    line: &str,
    players: &crate::players::Players,
    now: i64,
) -> Result<(), String> {
    let (target, effect, seconds, amplifier, visible) = parse_effect_give(line)?;
    let conn = players
        .connections()
        .into_iter()
        .find(|c| c.name() == target)
        .ok_or_else(|| "No entity was found".to_string())?;
    let row = crate::effects::EffectRegistry::shared()
        .and_then(|r| r.get(&effect))
        .ok_or_else(|| format!("Can't find element '{effect}' of type 'minecraft:mob_effect'"))?;
    let instantaneous = matches!(row.rule, crate::effects::TickRule::AtLeastOne);
    submit_effect(
        conn.id(),
        PendingEffect {
            effect,
            duration: effect_duration(seconds, instantaneous),
            amplifier,
            visible,
            at_tick: now,
        },
    );
    Ok(())
}

/// Every hit queued for `conn` that is due by the end of tick `tick`, in the
/// order the console read them: those read while tick `tick - 1` or earlier
/// was the latest.
#[must_use]
pub fn take(conn: u64, tick: i64) -> Vec<Act> {
    let mut q = queue()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = Vec::new();
    q.retain(|(c, p)| {
        if *c == conn && p.at_tick() <= tick {
            out.push(p.clone());
            false
        } else {
            true
        }
    });
    out
}

/// The console's whole handling of a `damage` line: parse, resolve every
/// entity against the connection list, check the type exists, queue.
///
/// # Errors
/// The message the console prints. Nothing is queued on an error.
pub fn dispatch(line: &str, players: &crate::players::Players, now: i64) -> Result<(), String> {
    let cmd = parse(line)?;
    let conns = players.connections();
    let find = |name: &str| {
        conns
            .iter()
            .find(|c| c.name() == name)
            .map(std::sync::Arc::clone)
            .ok_or_else(|| "No entity was found".to_string())
    };
    let target = find(&cmd.target)?;
    // `ResourceArgument.getResource` throws `ERROR_UNKNOWN_RESOURCE` for a key
    // the registry does not hold, before `hurtServer` is reached.
    let registry = registry().ok_or("damage: no data/vanilla/damage_types.json")?;
    if registry.get(&cmd.damage_type).is_none() {
        return Err(format!(
            "Can't find element '{}' of type '{REGISTRY}'",
            cmd.damage_type
        ));
    }
    let source = match cmd.source {
        Source::Plain => Source::Plain,
        Source::At(p) => Source::At(p),
        Source::By { direct, causing } => Source::By {
            direct: find(&direct)?.entity_id(),
            causing: find(&causing)?.entity_id(),
        },
    };
    submit(
        target.id(),
        Pending {
            amount: cmd.amount,
            damage_type: cmd.damage_type,
            source,
            at_tick: now,
        },
    );
    Ok(())
}

/// `Entity.hurtMarked` and `Entity.needsSync`, for the entities that have
/// them set, keyed by entity id.
///
/// Held beside [`crate::entities::Entity`] rather than on it because both are
/// one-tick flags that exactly one writer sets (the hurt path, on the
/// target's own loop) and exactly one reader clears (`ServerEntity.sendChanges`,
/// on the level tick), and a field on `Entity` would be a new member on
/// twenty-five struct literals that never touch either.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Marks {
    hurt: bool,
    /// The same `hurtMarked`, for the entity's own player. Split because the
    /// two halves of `sendToTrackingPlayersAndSelf` are sent from two
    /// threads here: the trackers' from the level tick, the player's own from
    /// its loop, ahead of that tick's `doTick` — see
    /// `world_send::send_changes_to_self`.
    hurt_self: bool,
    needs_sync: bool,
}

fn marks() -> &'static Mutex<std::collections::BTreeMap<i32, Marks>> {
    static M: OnceLock<Mutex<std::collections::BTreeMap<i32, Marks>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(std::collections::BTreeMap::new()))
}

fn with_marks<T>(f: impl FnOnce(&mut std::collections::BTreeMap<i32, Marks>) -> T) -> T {
    f(&mut marks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// `Entity.markHurt()` — `hurtMarked = true`.
pub fn mark_hurt(entity: i32) {
    with_marks(|m| {
        let k = m.entry(entity).or_default();
        k.hurt = true;
        k.hurt_self = true;
    });
}

/// `LivingEntity.knockback` offset 21 — `needsSync = true`.
pub fn mark_needs_sync(entity: i32) {
    with_marks(|m| m.entry(entity).or_default().needs_sync = true);
}

/// Whether `needsSync` is set, without clearing it.
#[must_use]
pub fn needs_sync(entity: i32) -> bool {
    with_marks(|m| m.get(&entity).is_some_and(|k| k.needs_sync))
}

/// `ServerEntity.sendChanges` offset 1189 — `entity.needsSync = false`.
pub fn clear_needs_sync(entity: i32) {
    with_marks(|m| {
        if let Some(k) = m.get_mut(&entity) {
            k.needs_sync = false;
            if *k == Marks::default() {
                m.remove(&entity);
            }
        }
    });
}

/// `ServerEntity.sendChanges` offsets 1202..1217: read `hurtMarked` and clear
/// it in one step.
#[must_use]
pub fn take_hurt_mark(entity: i32) -> bool {
    with_marks(|m| {
        let Some(k) = m.get_mut(&entity) else {
            return false;
        };
        let was = k.hurt;
        k.hurt = false;
        if *k == Marks::default() {
            m.remove(&entity);
        }
        was
    })
}

/// The self half of [`take_hurt_mark`].
#[must_use]
pub fn take_self_hurt_mark(entity: i32) -> bool {
    with_marks(|m| {
        let Some(k) = m.get_mut(&entity) else {
            return false;
        };
        let was = k.hurt_self;
        k.hurt_self = false;
        if *k == Marks::default() {
            m.remove(&entity);
        }
        was
    })
}

/// The damage-type table, once per process.
#[must_use]
pub fn registry() -> Option<&'static DamageTypeRegistry> {
    static R: OnceLock<Option<DamageTypeRegistry>> = OnceLock::new();
    R.get_or_init(|| {
        DamageTypeRegistry::load(&crate::playertick::table_path("damage_types.json")).ok()
    })
    .as_ref()
}

/// The `minecraft:damage_type` numbering the client was sent, in
/// `registry_data` order — `ClientboundDamageEventPacket.write` offset 9 puts
/// `id + 1` of this list on the wire (`ByteBufCodecs.holderRegistry`).
///
/// **Read from the capture, never derived.** The datapack directory listing
/// is alphabetical and happens to agree today; `crate::chat` records a
/// registry whose bootstrap order does not, and only the bytes the client was
/// actually sent can say which numbering it holds.
#[must_use]
pub fn type_ids() -> Option<&'static [String]> {
    static IDS: OnceLock<Option<Vec<String>>> = OnceLock::new();
    IDS.get_or_init(|| {
        crate::configuration::CapturedRegistries::load(&crate::playertick::table_path(
            "registries.bin",
        ))
        .ok()?
        .entry_ids(REGISTRY)
        .ok()
        .flatten()
    })
    .as_deref()
}

/// A damage type's wire id, or `None` if the capture does not hold it.
#[must_use]
pub fn wire_id(ids: &[String], damage_type: &str) -> Option<i32> {
    ids.iter()
        .position(|n| n == damage_type)
        .and_then(|i| i32::try_from(i).ok())
}

/// `DamageSource.scalesWithDifficulty()`: a `tableswitch` on
/// `type().scaling()`.
///
/// ```text
///   NEVER                              -> false
///   WHEN_CAUSED_BY_LIVING_NON_PLAYER   -> causingEntity instanceof LivingEntity
///                                         && !(causingEntity instanceof Player)
///   ALWAYS                             -> true
/// ```
///
/// `causing_is_living_non_player` is the caller's: the only entities this
/// server can name as a cause are players, so it is `false` for every hit the
/// console can build today — and the argument exists so that the first mob
/// cause is a change at the call site, not a silent `false` here.
#[must_use]
pub fn scales_with_difficulty(ty: &DamageType, causing_is_living_non_player: bool) -> bool {
    match ty.scaling.as_str() {
        "always" => true,
        "when_caused_by_living_non_player" => causing_is_living_non_player,
        _ => false,
    }
}

/// `Player.hurtServer` offsets 51..107 — the difficulty scaling.
///
/// ```text
///   PEACEFUL -> 0.0f
///   EASY     -> Math.min(damage / 2.0f + 1.0f, damage)
///   HARD     -> damage * 3.0f / 2.0f
/// ```
///
/// Three independent `if_acmpne` tests, not a switch, and NORMAL matches none
/// of them.
#[must_use]
pub fn scale_for_difficulty(damage: f32, difficulty: crate::food::Difficulty) -> f32 {
    use crate::food::Difficulty;
    match difficulty {
        Difficulty::Peaceful => 0.0,
        Difficulty::Easy => (damage / 2.0 + 1.0).min(damage),
        Difficulty::Hard => damage * 3.0 / 2.0,
        Difficulty::Normal => damage,
    }
}

/// `Player.isInvulnerableTo` offsets 11..152: the four damage game rules,
/// each keyed on a damage-type tag. `Avatar`/`LivingEntity`'s own answer
/// (offset 3) is false for a live survival player with no enchantments, and
/// is the caller's.
#[must_use]
pub fn blocked_by_game_rule(ty: &DamageType, rules: &crate::gamerules::GameRules) -> bool {
    let off = |rule: &str| {
        matches!(
            rules.get(rule),
            Some(crate::gamerules::GameRuleValue::Bool(false))
        )
    };
    let has = |tag: &str| ty.tags.contains(tag);
    (has("minecraft:is_drowning") && off("drowning_damage"))
        || (has("minecraft:is_fall") && off("fall_damage"))
        || (has("minecraft:is_fire") && off("fire_damage"))
        || (has("minecraft:is_freezing") && off("freeze_damage"))
}

/// `DamageEffects`' sound, by the `effects` name the damage type's JSON
/// carries — `Player.getHurtSound` is `source.type().effects().sound()`.
///
/// ```text
///   hurt      PLAYER_HURT            thorns    PLAYER_HURT
///   drowning  PLAYER_HURT_DROWN      burning   PLAYER_HURT_ON_FIRE
///   poking    PLAYER_HURT_SWEET_BERRY_BUSH     freezing  PLAYER_HURT_FREEZE
/// ```
///
/// `None` for a name the enum does not have, which the codec would have
/// refused when the datapack loaded.
#[must_use]
pub fn player_hurt_sound(effects: &str) -> Option<&'static str> {
    Some(match effects {
        "hurt" | "thorns" => "minecraft:entity.player.hurt",
        "drowning" => "minecraft:entity.player.hurt_drown",
        "burning" => "minecraft:entity.player.hurt_on_fire",
        "poking" => "minecraft:entity.player.hurt_sweet_berry_bush",
        "freezing" => "minecraft:entity.player.hurt_freeze",
        _ => return None,
    })
}

/// `ClientboundDamageEventPacket.write`: entity id, the damage type's
/// registry id, causing id + 1, direct id + 1 (0 = none,
/// `writeOptionalEntityId`), then an optional source position.
///
/// **The type id is NOT offset by one.** `DamageType.STREAM_CODEC` is
/// `ByteBufCodecs.holderRegistry`, which is `registry(key,
/// Registry::asHolderIdMap)` — a bare `idMap.getIdOrThrow` varint, with no
/// inline-value arm to reserve 0 for (`ByteBufCodecs.holder` is the one with
/// `id + 1`). This wrote `id + 1` first, and the first recording pair read
/// `source` 19 against vanilla's 18 for `generic`, 18 being `generic`'s index
/// in the captured registry.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn damage_event_body(
    entity: i32,
    type_id: i32,
    causing: Option<i32>,
    direct: Option<i32>,
    position: Option<[f64; 3]>,
) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    let mut b = Vec::with_capacity(16);
    write_varint(&mut b, entity)?;
    write_varint(&mut b, type_id)?;
    write_varint(&mut b, causing.map_or(0, |e| e + 1))?;
    write_varint(&mut b, direct.map_or(0, |e| e + 1))?;
    match position {
        None => b.push(0),
        Some(p) => {
            b.push(1);
            for v in p {
                b.extend_from_slice(&v.to_be_bytes());
            }
        }
    }
    Ok(b)
}

/// `ClientboundHurtAnimationPacket`: the entity id and `getHurtDir()` as a
/// float.
///
/// # Errors
/// Only if the buffer write fails.
pub fn hurt_animation_body(entity: i32, yaw: f32) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(9);
    rustmc_proto::varint::write_varint(&mut b, entity)?;
    b.extend_from_slice(&yaw.to_be_bytes());
    Ok(b)
}

/// `LivingEntity.knockback(strength, x, z, source, damage, false)` offsets
/// 0..184, over the entity's current velocity.
///
/// `None` when the resisted strength is not positive (offset 15), in which
/// case vanilla returns before `needsSync` and the velocity is untouched.
///
/// `redraw` is the entity's own `random.nextDouble()`, drawn FOUR times per
/// retry while `x*x + z*z < 1.0E-5` (offsets 30..95) — a hit with no
/// direction gets a random one. That draw comes off an unseeded
/// `RandomSource.create()` in vanilla, so no two vanilla runs agree on it and
/// no scenario grades it; it is taken as a closure so the loop is the loop.
pub fn knockback(
    velocity: (f64, f64, f64),
    on_ground: bool,
    strength: f64,
    knockback_resistance: f64,
    mut x: f64,
    mut z: f64,
    redraw: &mut impl FnMut() -> f64,
) -> Option<(f64, f64, f64)> {
    let strength = strength * (1.0 - knockback_resistance);
    if strength <= 0.0 {
        return None;
    }
    while x * x + z * z < 9.999_999_747_378_752e-6 {
        x = (redraw() - redraw()) * 0.01;
        z = (redraw() - redraw()) * 0.01;
    }
    // `new Vec3(x, 0.0, z).normalize().scale(strength)`. `Vec3.normalize` is
    // `sqrt(x*x + y*y + z*z)`, `< 1.0E-5 -> ZERO`, else each over the length.
    let len = (x * x + 0.0 * 0.0 + z * z).sqrt();
    let (nx, nz) = if len < 1.0e-5 {
        (0.0, 0.0)
    } else {
        (x / len * strength, z / len * strength)
    };
    let y = if on_ground {
        0.4f64.min(velocity.1 / 2.0 + strength)
    } else {
        velocity.1
    };
    Some((velocity.0 / 2.0 - nx, y, velocity.2 / 2.0 - nz))
}

/// `ServerPlayer.indicateDamage(x, z)`: `hurtDir = (float)(Mth.atan2(z, x) *
/// 57.2957763671875 - getYRot())`.
///
/// `Mth.atan2` is `Math.atan2` in 26.2 (a static forward), and the constant is
/// the float `57.295776f` widened, not `180 / PI`.
#[must_use]
pub fn hurt_dir(x: f64, z: f64, y_rot: f32) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2f at indicateDamage offset 16"
    )]
    let d = (z.atan2(x) * 57.295_776_367_187_5 - f64::from(y_rot)) as f32;
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bare_form_is_generic_with_no_source() {
        let c = parse("damage oracle 4").expect("parses");
        assert_eq!(c.damage_type, GENERIC);
        assert_eq!(c.source, Source::Plain);
        assert!((c.amount - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_unqualified_type_is_minecraft_and_by_names_one_entity_twice() {
        let c = parse("damage oracle 1 mob_attack by oracle_eye").expect("parses");
        assert_eq!(c.damage_type, "minecraft:mob_attack");
        assert_eq!(
            c.source,
            Source::By {
                direct: "oracle_eye".into(),
                causing: "oracle_eye".into()
            }
        );
        let c = parse("damage oracle 1 arrow by a from b").expect("parses");
        assert_eq!(
            c.source,
            Source::By {
                direct: "a".into(),
                causing: "b".into()
            }
        );
    }

    #[test]
    fn at_centres_integer_x_and_z_and_not_y() {
        let c = parse("damage oracle 1 generic at 3 64 -2").expect("parses");
        assert_eq!(c.source, Source::At([3.5, 64.0, -1.5]));
        let c = parse("damage oracle 1 generic at 3.0 64 -2.25").expect("parses");
        assert_eq!(c.source, Source::At([3.0, 64.0, -2.25]));
    }

    #[test]
    fn what_brigadier_would_refuse_is_refused() {
        assert!(parse("damage oracle -1").is_err(), "floatArg(0)");
        assert!(
            parse("damage oracle 1e3").is_err(),
            "no exponent in readFloat"
        );
        assert!(
            parse("damage @p 1").is_err(),
            "selectors are named, not guessed"
        );
        assert!(parse("damage oracle 1 generic at ~ ~ ~").is_err());
        assert!(parse("damage oracle 1 generic with x").is_err());
        assert!(
            parse("damage oracle 1 Generic").is_err(),
            "identifiers are lowercase"
        );
        assert!(parse("damage oracle").is_err());
    }

    #[test]
    fn the_queue_hands_each_connection_only_its_own_hits_in_order() {
        let hit = |a: f32, t: i64| Pending {
            amount: a,
            damage_type: GENERIC.into(),
            source: Source::Plain,
            at_tick: t,
        };
        submit(9001, hit(1.0, 10));
        submit(9002, hit(2.0, 10));
        submit(9001, hit(3.0, 10));
        submit(9001, hit(4.0, 12));
        assert!(take(9001, 9).is_empty(), "read in tick 10 is not due at 9");
        let mine = take(9001, 10);
        let amounts: Vec<f32> = mine
            .iter()
            .map(|a| match a {
                Act::Hurt(p) => p.amount,
                Act::Effect(_) => f32::NAN,
            })
            .collect();
        assert_eq!(amounts, [1.0, 3.0]);
        assert!(take(9001, 11).is_empty(), "taken means gone");
        assert_eq!(
            take(9001, 12).len(),
            1,
            "a later line waits for its own tick"
        );
        assert_eq!(take(9002, 10).len(), 1);
    }

    #[test]
    fn effect_give_durations_follow_the_command_not_the_effect() {
        assert_eq!(effect_duration(None, false), 600);
        assert_eq!(effect_duration(None, true), 1);
        assert_eq!(effect_duration(Some(30), false), 600);
        assert_eq!(
            effect_duration(Some(3), true),
            3,
            "an instant effect's seconds are ticks"
        );
        assert_eq!(effect_duration(Some(-1), false), -1);
        let (t, e, s, a, v) =
            parse_effect_give("effect give oracle resistance 30 1 true").expect("parses");
        assert_eq!(
            (t.as_str(), e.as_str(), s, a, v),
            ("oracle", "minecraft:resistance", Some(30), 1, false)
        );
        assert!(parse_effect_give("effect give oracle resistance 0").is_err());
        assert!(parse_effect_give("effect give oracle resistance 1 256").is_err());
    }

    #[test]
    fn difficulty_scaling_is_three_tests_and_normal_is_none_of_them() {
        use crate::food::Difficulty;
        assert!(scale_for_difficulty(4.0, Difficulty::Peaceful).abs() < f32::EPSILON);
        assert!((scale_for_difficulty(4.0, Difficulty::Easy) - 3.0).abs() < f32::EPSILON);
        // `min(d/2 + 1, d)`: a one-point hit stays one point on easy.
        assert!((scale_for_difficulty(1.0, Difficulty::Easy) - 1.0).abs() < f32::EPSILON);
        assert!((scale_for_difficulty(4.0, Difficulty::Normal) - 4.0).abs() < f32::EPSILON);
        assert!((scale_for_difficulty(4.0, Difficulty::Hard) - 6.0).abs() < f32::EPSILON);
    }

    #[test]
    fn knockback_pushes_away_from_the_source_and_lifts_on_the_ground() {
        let mut never = || -> f64 { panic!("a directed knockback drew from the random") };
        // Source 3 blocks along +z: the direction passed is (source - self),
        // and the push is its negation.
        let v = knockback((0.0, 0.0, 0.0), true, 0.4, 0.0, 0.0, 3.0, &mut never).expect("moves");
        assert!(v.0.abs() < 1e-12);
        assert!((v.1 - 0.4).abs() < 1e-12, "min(0.4, 0/2 + 0.4)");
        assert!((v.2 + 0.4).abs() < 1e-12);
        // Airborne keeps the vertical velocity.
        let v = knockback((0.2, -0.3, 0.0), false, 0.4, 0.0, 1.0, 0.0, &mut never).expect("moves");
        assert!((v.0 - (0.1 - 0.4)).abs() < 1e-12);
        assert!((v.1 + 0.3).abs() < 1e-12);
        // Full resistance: nothing at all.
        assert!(knockback((0.0, 0.0, 0.0), true, 0.4, 1.0, 1.0, 0.0, &mut never).is_none());
    }

    #[test]
    fn a_directionless_knockback_draws_four_doubles_per_retry() {
        let mut draws = 0;
        let seq = [0.9, 0.1, 0.3, 0.3];
        let mut rng = || {
            let v = seq[draws % 4];
            draws += 1;
            v
        };
        let v = knockback((0.0, 0.0, 0.0), true, 0.4, 0.0, 0.0, 0.0, &mut rng).expect("moves");
        assert_eq!(draws, 4);
        // x = 0.008, z = 0 -> pushed along -x.
        assert!((v.0 + 0.4).abs() < 1e-9);
    }

    #[test]
    fn the_damage_event_writes_absent_entities_as_zero() {
        let b = damage_event_body(5, 0, None, None, None).expect("buf");
        assert_eq!(b, [5, 0, 0, 0, 0]);
        let b = damage_event_body(5, 18, Some(7), Some(7), None).expect("buf");
        assert_eq!(b, [5, 18, 8, 8, 0], "the type id is bare, the entities +1");
    }

    #[test]
    fn hurt_sound_follows_the_damage_effects_enum() {
        assert_eq!(
            player_hurt_sound("hurt"),
            Some("minecraft:entity.player.hurt")
        );
        assert_eq!(
            player_hurt_sound("thorns"),
            Some("minecraft:entity.player.hurt")
        );
        assert_eq!(
            player_hurt_sound("burning"),
            Some("minecraft:entity.player.hurt_on_fire")
        );
        assert_eq!(player_hurt_sound("nonsense"), None);
    }
}
