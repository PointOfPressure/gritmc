//! A player's experience: the level maths, the per-player state, and the
//! `set_experience` sync.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `Player.giveExperiencePoints`, `Player.giveExperienceLevels`,
//! `Player.getXpNeededForNextLevel`, `Player.increaseScore`, `Player.tick`
//! (offsets 27-41, `takeXpDelay`), `ServerPlayer.giveExperiencePoints`,
//! `ServerPlayer.giveExperienceLevels`, `ServerPlayer.setExperiencePoints`,
//! `ServerPlayer.setExperienceLevels`, `ServerPlayer.<init>` (offset 60,
//! `lastSentExp = -99999999`), `ServerPlayer.doTick` (offsets 488-529),
//! `ExperienceCommand$Type`'s four lambdas.
//!
//! # Where each half runs, and why that is the tick order
//!
//! `ServerPlayer.doTick` is reached from `ServerGamePacketListenerImpl.tick`,
//! the **connection** tick, which `MinecraftServer.tickChildren` runs after
//! every level has ticked: the block-change ack goes out first (offsets
//! 1-25 of the listener's `tick`), then `tickPlayer` -> `doTick`, whose
//! `Player.tick` decrements `takeXpDelay`, whose `aiStep` touches orbs
//! (`crate::xporb::touch`), and whose tail compares `totalExperience` with
//! `lastSentExp` and sends `ClientboundSetExperiencePacket`. So a pickup and
//! the `set_experience` it causes are in the same tick, after the tick's block
//! broadcast and after the ack. [`connection_tick`] is called from
//! `crate::status::listen` immediately after `flush_acks` for that reason.
//!
//! `DedicatedServer.tickConnection` is `super.tickConnection();
//! handleConsoleInputs();` — the console runs **after** every connection has
//! ticked. A console `xp` therefore lands its `set_experience` one tick later,
//! at the next `doTick`, and [`run_console`] is called after
//! [`connection_tick`] to keep that.
//!
//! # The score is synched data and the level is not
//!
//! `Player.increaseScore` writes `DATA_SCORE_ID` (accessor 18, `INT`), so a
//! pickup also dirties the player's `SynchedEntityData` and the next
//! `ServerEntity.sendChanges` broadcasts it **to trackers and to the player
//! itself** (`broadcastAndSend`). `experienceLevel`, `totalExperience` and
//! `experienceProgress` are plain fields and reach only the player's own
//! client, through `set_experience`. Nothing resets the score short of a
//! respawn, so a check that observes `set_entity_data` while a player gains
//! points is a statement about every point gained earlier in the session.
//!
//! # One store for three ports
//!
//! `train-g` made this the player's one experience. `entity-ports` read a copy
//! inside `playertick` and `commands-exec` kept one on the connection for
//! `/experience`; both now reach this [`Registry`] by entity id
//! (`crate::players::Connection::with_experience`), and [`connection_tick`]
//! is the one `set_experience` sender. `/experience`'s level-up sounds are
//! queued ([`queue_levelup`]) and played by [`run_console`].

use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// `ServerPlayer.<init>` offset 60: `ldc -99999999; putfield lastSentExp`.
///
/// Not `-1`, which is what every *setter* writes. The difference is not
/// observable — both differ from any reachable total — but the constructor's
/// value is the one a fresh player has, so it is the one carried.
pub const LAST_SENT_EXP_INITIAL: i32 = -99_999_999;

/// `Player.getXpNeededForNextLevel()`.
///
/// ```text
///   level >= 30 : 112 + (level - 30) * 9
///   level >= 15 :  37 + (level - 15) * 5
///   otherwise   :   7 + level * 2
/// ```
///
/// `int` arithmetic throughout, wrapping as Java's does for a level near
/// `i32::MAX` (reachable: `giveExperienceLevels` saturates rather than wraps).
#[must_use]
pub const fn xp_needed_for_next_level(level: i32) -> i32 {
    if level >= 30 {
        112_i32.wrapping_add(level.wrapping_sub(30).wrapping_mul(9))
    } else if level >= 15 {
        37_i32.wrapping_add(level.wrapping_sub(15).wrapping_mul(5))
    } else {
        7_i32.wrapping_add(level.wrapping_mul(2))
    }
}

/// `SoundEvents.PLAYER_LEVELUP`'s registry name.
pub const LEVELUP_SOUND: &str = "minecraft:entity.player.levelup";

/// One `Level.playSound(null, x, y, z, PLAYER_LEVELUP, getSoundSource(),
/// volume * 0.75f, 1.0f)` that `giveExperienceLevels` asked for.
///
/// Carried as a value rather than played here so the maths stays pure and
/// the caller, which knows where the player stands, builds the packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelUp {
    /// `f * 0.75f`, where `f` is `level > 30 ? 1.0f : level / 30.0f`.
    pub volume: f32,
}

/// The experience fields of one `ServerPlayer`, and the three counters that
/// decide when anything about them is sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Experience {
    /// `Player.experienceLevel`.
    pub level: i32,
    /// `Player.experienceProgress` — a fraction of the current level, `[0, 1)`
    /// after every whole call returns.
    pub progress: f32,
    /// `Player.totalExperience`, clamped to `[0, i32::MAX]` on every add.
    pub total: i32,
    /// `Player.DATA_SCORE_ID`.
    pub score: i32,
    /// Whether [`Self::score`] has changed since the last
    /// `sendDirtyEntityData` — `SynchedEntityData.isDirty` for accessor 18.
    pub score_dirty: bool,
    /// `ServerPlayer.lastSentExp`.
    pub last_sent: i32,
    /// `Player.takeXpDelay`: an orb is taken only while this is 0, and taking
    /// one sets it to 2.
    pub take_xp_delay: i32,
    /// `Player.lastLevelUpTime`, compared against [`Self::tick_count`].
    pub last_level_up_time: i32,
    /// `Entity.tickCount` for this player — `ServerLevel.tickNonPassenger`
    /// increments it once per entity tick.
    pub tick_count: i32,
}

impl Default for Experience {
    fn default() -> Self {
        Self::new()
    }
}

impl Experience {
    /// A fresh `ServerPlayer`: level 0, nothing gained, nothing sent.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            level: 0,
            progress: 0.0,
            total: 0,
            score: 0,
            score_dirty: false,
            last_sent: LAST_SENT_EXP_INITIAL,
            take_xp_delay: 0,
            last_level_up_time: 0,
            tick_count: 0,
        }
    }

    /// `getXpNeededForNextLevel()` at the current level.
    #[must_use]
    pub const fn needed(&self) -> i32 {
        xp_needed_for_next_level(self.level)
    }

    /// `ServerPlayer.giveExperiencePoints(int)`:
    /// `if (amount == 0) return; super.giveExperiencePoints(amount);
    /// lastSentExp = -1;`
    ///
    /// `Player.giveExperiencePoints`, offsets 0-147:
    ///
    /// ```text
    ///   increaseScore(amount)
    ///   progress += (float) amount / (float) getXpNeededForNextLevel()
    ///   total = Mth.clamp(total + amount, 0, Integer.MAX_VALUE)   // int add, wraps first
    ///   while (progress < 0.0f) {
    ///       float f = progress * (float) getXpNeededForNextLevel();
    ///       if (level > 0) { giveExperienceLevels(-1); progress = 1.0f + f / getXpNeededForNextLevel(); }
    ///       else           { giveExperienceLevels(-1); progress = 0.0f; }
    ///   }
    ///   while (progress >= 1.0f) {
    ///       progress = (progress - 1.0f) * (float) getXpNeededForNextLevel();
    ///       giveExperienceLevels(1);
    ///       progress /= (float) getXpNeededForNextLevel();
    ///   }
    /// ```
    ///
    /// **`giveExperienceLevels` inside it is the virtual call**, so on a
    /// server player each step is `ServerPlayer.giveExperienceLevels` — and a
    /// level-up crossed by points plays the level-up sound exactly as a
    /// `/xp add ... levels` does. Every `float` operation is single precision
    /// and in this order; a port in `f64` lands a level boundary on the wrong
    /// side for some amounts.
    pub fn give_points(&mut self, amount: i32) -> Vec<LevelUp> {
        let mut sounds = Vec::new();
        if amount == 0 {
            return sounds;
        }
        self.increase_score(amount);
        #[allow(clippy::cast_precision_loss)]
        {
            self.progress += amount as f32 / self.needed() as f32;
        }
        self.total = self.total.wrapping_add(amount).clamp(0, i32::MAX);
        while self.progress < 0.0 {
            #[allow(clippy::cast_precision_loss)]
            let f = self.progress * self.needed() as f32;
            if self.level > 0 {
                sounds.extend(self.give_levels(-1));
                #[allow(clippy::cast_precision_loss)]
                {
                    self.progress = 1.0 + f / self.needed() as f32;
                }
            } else {
                sounds.extend(self.give_levels(-1));
                self.progress = 0.0;
            }
        }
        while self.progress >= 1.0 {
            #[allow(clippy::cast_precision_loss)]
            {
                self.progress = (self.progress - 1.0) * self.needed() as f32;
            }
            sounds.extend(self.give_levels(1));
            #[allow(clippy::cast_precision_loss)]
            {
                self.progress /= self.needed() as f32;
            }
        }
        self.last_sent = -1;
        sounds
    }

    /// `Player.increaseScore(int)`: `entityData.set(DATA_SCORE_ID, getScore() + amount)`.
    ///
    /// `SynchedEntityData.set` marks the item dirty only when the value
    /// changes, which for an `int` add is exactly `amount != 0` modulo 2^32.
    pub fn increase_score(&mut self, amount: i32) {
        let next = self.score.wrapping_add(amount);
        if next != self.score {
            self.score = next;
            self.score_dirty = true;
        }
    }

    /// `ServerPlayer.giveExperienceLevels(int)`:
    /// `if (amount == 0) return; super.giveExperienceLevels(amount);
    /// lastSentExp = -1;`
    ///
    /// `Player.giveExperienceLevels`, offsets 0-129:
    ///
    /// ```text
    ///   level = IntMath.saturatedAdd(level, amount)
    ///   if (level < 0) { level = 0; progress = 0.0f; total = 0; }
    ///   if (amount > 0 && level % 5 == 0 && (float) lastLevelUpTime < (float) tickCount - 100.0f) {
    ///       float f = level > 30 ? 1.0f : (float) level / 30.0f;
    ///       level().playSound(null, getX(), getY(), getZ(), PLAYER_LEVELUP, getSoundSource(), f * 0.75f, 1.0f);
    ///       lastLevelUpTime = tickCount;
    ///   }
    /// ```
    ///
    /// The reset branch is the only thing in the game that zeroes
    /// `totalExperience` short of a respawn, which is why a corpus scenario's
    /// `console_restore` uses a large negative level add.
    pub fn give_levels(&mut self, amount: i32) -> Option<LevelUp> {
        if amount == 0 {
            return None;
        }
        self.level = self.level.saturating_add(amount);
        if self.level < 0 {
            self.level = 0;
            self.progress = 0.0;
            self.total = 0;
        }
        let mut out = None;
        #[allow(clippy::cast_precision_loss)]
        if amount > 0
            && self.level % 5 == 0
            && (self.last_level_up_time as f32) < (self.tick_count as f32) - 100.0
        {
            let f = if self.level > 30 {
                1.0_f32
            } else {
                self.level as f32 / 30.0
            };
            out = Some(LevelUp { volume: f * 0.75 });
            self.last_level_up_time = self.tick_count;
        }
        self.last_sent = -1;
        out
    }

    /// `ServerPlayer.setExperiencePoints(int)`, offsets 0-45:
    ///
    /// ```text
    ///   float f = getXpNeededForNextLevel();
    ///   float g = (f - 1.0f) / f;
    ///   float p = Mth.clamp(amount / f, 0.0f, g);
    ///   if (p == experienceProgress) return;
    ///   experienceProgress = p; lastSentExp = -1;
    /// ```
    ///
    /// **`totalExperience` is not touched.** `/xp set <p> points` moves the
    /// bar and leaves the total the client is sent as it was.
    pub fn set_points(&mut self, amount: i32) {
        #[allow(clippy::cast_precision_loss)]
        let f = self.needed() as f32;
        let g = (f - 1.0) / f;
        #[allow(clippy::cast_precision_loss)]
        let p = (amount as f32 / f).clamp(0.0, g);
        // `fcmpl; ifne` — a float compare, so `-0.0 == 0.0` returns early.
        #[allow(clippy::float_cmp)]
        if p == self.progress {
            return;
        }
        self.progress = p;
        self.last_sent = -1;
    }

    /// `ServerPlayer.setExperienceLevels(int)`:
    /// `if (amount == experienceLevel) return; experienceLevel = amount;
    /// lastSentExp = -1;`
    pub fn set_levels(&mut self, amount: i32) {
        if amount == self.level {
            return;
        }
        self.level = amount;
        self.last_sent = -1;
    }

    /// `Player.tick` offsets 27-41: `if (takeXpDelay > 0) takeXpDelay--`.
    pub fn tick_take_delay(&mut self) {
        if self.take_xp_delay > 0 {
            self.take_xp_delay -= 1;
        }
    }

    /// `ServerPlayer.doTick` offsets 488-529:
    ///
    /// ```text
    ///   if (totalExperience != lastSentExp) {
    ///       lastSentExp = totalExperience;
    ///       connection.send(new ClientboundSetExperiencePacket(experienceProgress, totalExperience, experienceLevel));
    ///   }
    /// ```
    ///
    /// The comparison is on the **total**, and the setters force it with
    /// `-1`. So `/xp set <n> levels` — which changes neither the total nor
    /// the progress — still sends, while a direct field write that bypassed
    /// the setters would not. Returns `(progress, level, total)` in the
    /// packet's wire order (see `crate::respawn::encode_set_experience`).
    pub fn sync(&mut self) -> Option<(f32, i32, i32)> {
        if self.total == self.last_sent {
            return None;
        }
        self.last_sent = self.total;
        Some((self.progress, self.level, self.total))
    }
}

/// `ExperienceCommand$Type`: which half of the bar a command addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Points,
    Levels,
}

/// `ExperienceCommand`'s three verbs, resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XpCommand {
    /// `xp add <targets> <amount> [points|levels]` — `Type.add`:
    /// `Player::giveExperiencePoints` / `Player::giveExperienceLevels`.
    Add {
        target: String,
        amount: i32,
        unit: Unit,
    },
    /// `xp set <targets> <amount> [points|levels]`. `amount` is
    /// `IntegerArgumentType.integer(0)`.
    Set {
        target: String,
        amount: i32,
        unit: Unit,
    },
    /// `xp query <target> <points|levels>`. Changes nothing.
    Query { target: String, unit: Unit },
}

/// Parse an `experience`/`xp` console line (without the command word).
///
/// The grammar is `ExperienceCommand.register`'s, with `<targets>` narrowed
/// to a **player name** — the one selector form this server's console can
/// resolve without an entity-selector engine. A selector (`@a`, `@p`) is
/// refused by name rather than read as a player called `@a`.
///
/// # Errors
/// A message naming what did not parse.
pub fn parse_xp(rest: &[&str]) -> Result<XpCommand, String> {
    fn unit(s: Option<&&str>, default: Unit) -> Result<Unit, String> {
        match s.copied() {
            None => Ok(default),
            Some("points") => Ok(Unit::Points),
            Some("levels") => Ok(Unit::Levels),
            Some(other) => Err(format!("expected points|levels, got {other:?}")),
        }
    }
    fn target(s: Option<&&str>) -> Result<String, String> {
        match s.copied() {
            Some(t) if t.starts_with('@') => Err(format!(
                "selector {t:?}: this console resolves player names only"
            )),
            Some(t) => Ok(t.to_string()),
            None => Err("missing target".into()),
        }
    }
    match rest.first().copied() {
        Some(verb @ ("add" | "set")) => {
            let target = target(rest.get(1))?;
            let amount: i32 = rest
                .get(2)
                .ok_or("missing amount")?
                .parse()
                .map_err(|e| format!("amount: {e}"))?;
            let unit = unit(rest.get(3), Unit::Points)?;
            if rest.len() > 4 {
                return Err("trailing arguments".into());
            }
            if verb == "add" {
                Ok(XpCommand::Add {
                    target,
                    amount,
                    unit,
                })
            } else if amount < 0 {
                // `IntegerArgumentType.integer(0)` on the set node.
                Err(format!("set amount {amount} is below 0"))
            } else {
                Ok(XpCommand::Set {
                    target,
                    amount,
                    unit,
                })
            }
        }
        Some("query") => {
            let target = target(rest.get(1))?;
            // `query <target> <type>` has no default: the type node is the
            // executable one.
            let unit = match rest.get(2).copied() {
                Some("points") => Unit::Points,
                Some("levels") => Unit::Levels,
                other => return Err(format!("query needs points|levels, got {other:?}")),
            };
            Ok(XpCommand::Query { target, unit })
        }
        other => Err(format!("expected add|set|query, got {other:?}")),
    }
}

/// Apply a parsed command to one player's state, as `ExperienceCommand`'s
/// executors do. Returns the level-up sounds it caused and, for a query,
/// the value read.
///
/// `set ... points` is **refused** when `amount >= getXpNeededForNextLevel()`
/// (`Type` lambda 0, offsets 0-12) — the bar cannot be set to a full level
/// from this command.
pub fn apply(cmd: &XpCommand, xp: &mut Experience) -> (Vec<LevelUp>, Result<i32, String>) {
    match *cmd {
        XpCommand::Add {
            amount,
            unit: Unit::Points,
            ..
        } => (xp.give_points(amount), Ok(amount)),
        XpCommand::Add {
            amount,
            unit: Unit::Levels,
            ..
        } => (xp.give_levels(amount).into_iter().collect(), Ok(amount)),
        XpCommand::Set {
            amount,
            unit: Unit::Points,
            ..
        } => {
            if amount >= xp.needed() {
                return (
                    Vec::new(),
                    Err("commands.experience.set.points.invalid".into()),
                );
            }
            xp.set_points(amount);
            (Vec::new(), Ok(amount))
        }
        XpCommand::Set {
            amount,
            unit: Unit::Levels,
            ..
        } => {
            xp.set_levels(amount);
            (Vec::new(), Ok(amount))
        }
        // `Type` lambda 1: `Mth.floor(experienceProgress * getXpNeededForNextLevel())`;
        // lambda 3: `experienceLevel`.
        XpCommand::Query {
            unit: Unit::Points, ..
        } => {
            #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
            let v = (xp.progress * xp.needed() as f32).floor() as i32;
            (Vec::new(), Ok(v))
        }
        XpCommand::Query {
            unit: Unit::Levels, ..
        } => (Vec::new(), Ok(xp.level)),
    }
}

// ---- the names the other two ports of this file used ----

/// `lastSentExp`'s initial value, by `commands-exec`'s name.
pub const NEVER_SENT: i32 = LAST_SENT_EXP_INITIAL;

/// [`xp_needed_for_next_level`] by `commands-exec`'s name.
#[must_use]
pub const fn needed_for_next_level(level: i32) -> i32 {
    xp_needed_for_next_level(level)
}

impl Experience {
    /// `experienceLevel` (`entity-ports`' accessor).
    #[must_use]
    pub const fn level(&self) -> i32 {
        self.level
    }

    /// `experienceProgress`.
    #[must_use]
    pub const fn progress(&self) -> f32 {
        self.progress
    }

    /// `totalExperience`.
    #[must_use]
    pub const fn total(&self) -> i32 {
        self.total
    }

    /// `DATA_SCORE_ID`.
    #[must_use]
    pub const fn score(&self) -> i32 {
        self.score
    }

    /// `getXpNeededForNextLevel()`.
    #[must_use]
    pub const fn xp_needed_for_next_level(&self) -> i32 {
        self.needed()
    }

    /// `ExperienceCommand.Type.POINTS`' getter: `Mth.floor(progress *
    /// getXpNeededForNextLevel())`.
    #[must_use]
    pub fn points(&self) -> i32 {
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "`i2f`, then `Mth.floor`'s own narrowing"
        )]
        {
            (self.progress * self.needed() as f32).floor() as i32
        }
    }

    /// `ServerPlayer.onEnchantmentPerformed` over `Player`'s: the levels come
    /// off directly (no saturation, no rescale), a negative level wipes the
    /// bar, and `lastSentExp = -1`. The seed reroll belongs to whoever holds
    /// the player's `RandomSource`.
    pub const fn on_enchantment_performed(&mut self, levels: i32) {
        self.level -= levels;
        if self.level < 0 {
            self.level = 0;
            self.progress = 0.0;
            self.total = 0;
        }
        self.last_sent = -1;
    }
}

/// `ClientboundSetExperiencePacket`: `experienceProgress` as a float, then
/// `experienceLevel` and `totalExperience` as varints.
///
/// # Errors
/// Never, for a `Vec`.
pub fn set_experience_body(progress: f32, total: i32, level: i32) -> std::io::Result<Vec<u8>> {
    crate::respawn::encode_set_experience(progress, level, total)
}

/// Level-up sounds a command on another thread earned (`crate::cmdexec`'s
/// `/experience`), for [`run_console`] to play at the player's feet — where
/// this module's own console `xp` plays its, after every connection has
/// ticked.
fn levelup_queue() -> &'static Mutex<Vec<(i32, LevelUp)>> {
    static Q: OnceLock<Mutex<Vec<(i32, LevelUp)>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// Queue a level-up sound for `entity_id`'s player.
pub fn queue_levelup(entity_id: i32, up: LevelUp) {
    if let Ok(mut q) = levelup_queue().lock() {
        q.push((entity_id, up));
    }
}

// ---- the per-player registry ----

/// Every connected player's [`Experience`], by entity id, plus the player's
/// own `Entity.random` for the one draw `aiStep` makes from it.
#[derive(Debug, Default)]
pub struct Registry {
    players: Mutex<HashMap<i32, (Experience, LegacyRandomSource)>>,
    /// Saved state read at login, waiting for the player's first tick.
    /// Separate from `players` so [`Self::retain`] — which runs every tick
    /// against the entity list — cannot drop it in the window between the
    /// read and the entity's insertion.
    pending: Mutex<HashMap<i32, Experience>>,
}

impl Registry {
    /// The process-wide registry: the clock thread writes it, the console
    /// thread only queues ([`queue_console`]).
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Registry> = OnceLock::new();
        SHARED.get_or_init(Registry::default)
    }

    /// Run `f` on a player's state, creating it fresh if this is the first
    /// time the player is seen. `None` only if the lock is poisoned.
    pub fn with<T>(
        &self,
        entity_id: i32,
        f: impl FnOnce(&mut Experience, &mut LegacyRandomSource) -> T,
    ) -> Option<T> {
        let mut map = self.players.lock().ok()?;
        let (xp, rng) = map.entry(entity_id).or_insert_with(|| {
            let loaded = self
                .pending
                .lock()
                .ok()
                .and_then(|mut p| p.remove(&entity_id));
            (loaded.unwrap_or_default(), crate::dropitem::unique_random())
        });
        Some(f(xp, rng))
    }

    /// `Player.readAdditionalSaveData`'s experience half: `XpP`, `XpLevel`,
    /// `XpTotal`, and `setScore(Score)`. `lastSentExp` stays the
    /// constructor's, so the first `doTick` sends the loaded bar.
    pub fn preload(&self, entity_id: i32, level: i32, progress: f32, total: i32, score: i32) {
        let x = Experience {
            level,
            progress,
            total,
            score,
            ..Experience::new()
        };
        if let Ok(mut p) = self.pending.lock() {
            p.insert(entity_id, x);
        }
    }

    /// The score a player joins with — the preloaded one, or 0.
    #[must_use]
    pub fn joining_score(&self, entity_id: i32) -> i32 {
        self.pending
            .lock()
            .ok()
            .and_then(|p| p.get(&entity_id).map(|x| x.score))
            .unwrap_or(0)
    }

    /// A copy of a player's state, if it has one.
    #[must_use]
    pub fn get(&self, entity_id: i32) -> Option<Experience> {
        self.players
            .lock()
            .ok()?
            .get(&entity_id)
            .map(|(x, _)| x.clone())
    }

    /// Forget every player not in `live`. A reconnecting player is a new
    /// `ServerPlayer` and starts from a fresh [`Experience`] unless its saved
    /// data says otherwise.
    pub fn retain(&self, live: &[i32]) {
        if let Ok(mut map) = self.players.lock() {
            map.retain(|id, _| live.contains(id));
        }
    }
}

/// `Player.addAdditionalSaveData`'s experience half, for a save:
/// `(XpLevel, XpP, XpTotal, Score)`, or `None` for a player never ticked.
#[must_use]
pub fn save_fields(entity_id: i32) -> Option<(i32, f32, i32, i32)> {
    Registry::shared()
        .get(entity_id)
        .map(|x| (x.level, x.progress, x.total, x.score))
}

/// `sendDirtyEntityData` for a player's score: the `set_entity_data` body
/// carrying accessor 18, if the score changed since the last call.
#[must_use]
pub fn drain_dirty_score(entity_id: i32) -> Option<Vec<u8>> {
    let score = Registry::shared().with(entity_id, |xp, _| {
        if xp.score_dirty {
            xp.score_dirty = false;
            Some(xp.score)
        } else {
            None
        }
    })??;
    let mut value = Vec::new();
    rustmc_proto::varint::write_varint(&mut value, score).ok()?;
    crate::synched::set_entity_data_body(
        entity_id,
        &[crate::synched::DataValue {
            id: crate::synched::SCORE_ACCESSOR,
            serializer: crate::synched::serializer::INT,
            value,
        }],
    )
    .ok()
}

/// Draw `Util.getRandom(list, player.random)`'s index: `random.nextInt(size)`.
pub fn player_pick(entity_id: i32, size: usize) -> usize {
    if size <= 1 {
        // `nextInt(1)` still draws; the result is 0 either way and the
        // generator is unseeded, so the draw itself is not observable.
        Registry::shared().with(entity_id, |_, r| {
            let _ = r.next_i32_bound(1);
        });
        return 0;
    }
    let bound = i32::try_from(size).unwrap_or(i32::MAX);
    Registry::shared()
        .with(entity_id, |_, r| {
            usize::try_from(r.next_i32_bound(bound)).unwrap_or(0)
        })
        .unwrap_or(0)
}

// ---- the console queue ----

/// A console line this module owns, queued from the console thread and run
/// on the clock thread at the point `handleConsoleInputs` occupies.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsoleAction {
    Xp(XpCommand),
    /// `summon minecraft:experience_orb`, already resolved to a position.
    /// `at` is either absolute or relative to a named player
    /// (`execute at <player> run summon ...`).
    SummonOrb(crate::xporb::SummonOrb),
}

fn console_queue() -> &'static Mutex<Vec<ConsoleAction>> {
    static Q: OnceLock<Mutex<Vec<ConsoleAction>>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(Vec::new()))
}

/// Queue a console action for the next [`run_console`].
pub fn queue_console(a: ConsoleAction) {
    if let Ok(mut q) = console_queue().lock() {
        q.push(a);
    }
}

/// Take everything queued, in arrival order.
#[must_use]
pub fn take_console() -> Vec<ConsoleAction> {
    console_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// A level-up sound at a player's feet: `Level.playSound(null, getX(), getY(),
/// getZ(), PLAYER_LEVELUP, getSoundSource(), volume, 1.0f)`.
///
/// `Player.getSoundSource()` is `SoundSource.PLAYERS`. The `null` source
/// entity means the levelling player hears it too.
#[must_use]
pub fn levelup_sound(at: (f64, f64, f64), up: LevelUp) -> crate::sounds::Sound {
    crate::sounds::Sound {
        event: LEVELUP_SOUND.to_string(),
        source: crate::sounds::SOURCE_PLAYERS,
        at: crate::sounds::Eighths::exact(at.0, at.1, at.2),
        pos: at,
        volume: up.volume,
        pitch: 1.0,
        except: None,
    }
}

// ---- the two clock-thread passes ----

/// What the two passes below need from the clock.
pub struct Ctx<'a> {
    pub players: &'a crate::players::Players,
    pub protocol_ids: Option<&'a crate::protocol_ids::ProtocolIds>,
    pub sound_events: Option<&'a crate::sound_events::SoundEventTable>,
    pub level: crate::xporb::Level<'a>,
}

fn publish_score(entities: &crate::entities::Entities, entity_id: i32, value: i32) {
    entities.update(entity_id, |e| {
        if let crate::synched::Synched::Avatar { score, .. } = &mut e.synched {
            *score = value;
        }
    });
}

fn play(ctx: &Ctx<'_>, at: (f64, f64, f64), ups: &[LevelUp]) {
    if ups.is_empty() {
        return;
    }
    let sounds: Vec<crate::sounds::Sound> = ups.iter().map(|u| levelup_sound(at, *u)).collect();
    crate::sounds::send(
        ctx.players,
        ctx.protocol_ids,
        ctx.sound_events,
        Some(ctx.level.entities),
        &sounds,
    );
}

/// The experience half of every player's `ServerPlayer.doTick`, in
/// connection order: `tickCount` (the entity tick's increment), `takeXpDelay`,
/// the orb touch, and the `set_experience` comparison. See the module docs
/// for why this sits right after the acknowledgements.
pub fn connection_tick(ctx: &Ctx<'_>) {
    let live: Vec<i32> = ctx.level.players.iter().map(|p| p.entity_id).collect();
    Registry::shared().retain(&live);
    for p in ctx.level.players {
        let done = Registry::shared().with(p.entity_id, |xp, rng| {
            xp.tick_count = xp.tick_count.wrapping_add(1);
            xp.tick_take_delay();
            let before = xp.score;
            let ups = crate::xporb::touch(&ctx.level, p, xp, |n| {
                let bound = i32::try_from(n).unwrap_or(i32::MAX);
                usize::try_from(rng.next_i32_bound(bound)).unwrap_or(0)
            });
            let score = (xp.score != before).then_some(xp.score);
            (ups, score, xp.sync())
        });
        let Some((ups, score, sync)) = done else {
            continue;
        };
        if let Some(v) = score {
            publish_score(ctx.level.entities, p.entity_id, v);
        }
        play(ctx, p.pos, &ups);
        if let Some((progress, level, total)) = sync {
            if let Ok(body) = crate::respawn::encode_set_experience(progress, level, total) {
                let _ = p
                    .conn
                    .send(rustmc_proto::ids::play::cb::SET_EXPERIENCE, &body);
            }
        }
    }
}

/// `DedicatedServer.handleConsoleInputs` for the lines this module owns,
/// after every connection has ticked.
pub fn run_console(ctx: &Ctx<'_>) {
    let queued = levelup_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default();
    for (id, up) in queued {
        if let Some(p) = ctx.level.players.iter().find(|p| p.entity_id == id) {
            play(ctx, p.pos, &[up]);
        }
    }
    for action in take_console() {
        match action {
            ConsoleAction::Xp(cmd) => {
                let target = match &cmd {
                    XpCommand::Add { target, .. }
                    | XpCommand::Set { target, .. }
                    | XpCommand::Query { target, .. } => target.clone(),
                };
                let Some(p) = ctx.level.players.iter().find(|p| p.conn.name() == target) else {
                    eprintln!("console: xp: no player named {target:?}");
                    continue;
                };
                let done = Registry::shared().with(p.entity_id, |xp, _| {
                    let before = xp.score;
                    let (ups, r) = apply(&cmd, xp);
                    (ups, r, (xp.score != before).then_some(xp.score))
                });
                let Some((ups, r, score)) = done else {
                    continue;
                };
                if let Some(v) = score {
                    publish_score(ctx.level.entities, p.entity_id, v);
                }
                play(ctx, p.pos, &ups);
                match r {
                    Ok(v) => println!("console: xp {cmd:?} -> {v}"),
                    Err(e) => eprintln!("console: xp {cmd:?}: {e}"),
                }
            }
            ConsoleAction::SummonOrb(s) => {
                let base = match &s.relative_to {
                    Some(name) => {
                        let Some(p) = ctx.level.players.iter().find(|p| p.conn.name() == *name)
                        else {
                            eprintln!("console: execute at: no player named {name:?}");
                            continue;
                        };
                        p.pos
                    }
                    None => (0.0, 0.0, 0.0),
                };
                if s.relative_to.is_none() && s.at.iter().any(|(_, rel)| *rel) {
                    eprintln!("console: summon: `~` needs `execute at <player>` on this console");
                    continue;
                }
                let pick = |i: usize, b: f64| if s.at[i].1 { b + s.at[i].0 } else { s.at[i].0 };
                let at =
                    crate::collide::Vec3::new(pick(0, base.0), pick(1, base.1), pick(2, base.2));
                let id = crate::xporb::summon(&ctx.level, at, &s);
                println!("console: summoned experience_orb {id} at {at:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_level_curve_has_three_arms() {
        assert_eq!(xp_needed_for_next_level(0), 7);
        assert_eq!(xp_needed_for_next_level(14), 35);
        assert_eq!(xp_needed_for_next_level(15), 37);
        assert_eq!(xp_needed_for_next_level(29), 107);
        assert_eq!(xp_needed_for_next_level(30), 112);
        assert_eq!(xp_needed_for_next_level(31), 121);
    }

    /// Ten points from nothing: seven fill level 0, three of level 1's nine
    /// remain. The progress is the `float` vanilla computes, not 1/3.
    #[test]
    fn ten_points_cross_one_level() {
        let mut x = Experience::new();
        let s = x.give_points(10);
        assert_eq!((x.level, x.total, x.score), (1, 10, 10));
        // ((10/7) - 1) * 7 / 9, in f32, in that order.
        let p = ((10.0_f32 / 7.0 - 1.0) * 7.0) / 9.0;
        assert_eq!(x.progress.to_bits(), p.to_bits());
        assert!(s.is_empty(), "level 1 is not a multiple of five");
        assert_eq!(x.sync(), Some((p, 1, 10)));
        assert_eq!(x.sync(), None, "sent once, then quiet");
    }

    #[test]
    fn the_constructor_sends_once_even_at_zero() {
        let mut x = Experience::new();
        assert_eq!(x.sync(), Some((0.0, 0, 0)));
        assert_eq!(x.sync(), None);
    }

    #[test]
    fn a_level_up_to_a_multiple_of_five_is_heard_only_after_a_hundred_ticks() {
        let mut x = Experience::new();
        x.tick_count = 100;
        assert_eq!(x.give_levels(5), None, "0 < 100 - 100 is false");
        let mut y = Experience::new();
        y.tick_count = 101;
        assert_eq!(y.give_levels(5), Some(LevelUp { volume: 0.125 }));
        assert_eq!(y.last_level_up_time, 101);
        assert_eq!(y.give_levels(5), None, "inside the 100-tick window");
    }

    #[test]
    fn a_large_negative_level_add_zeroes_all_three() {
        let mut x = Experience::new();
        x.give_points(500);
        assert!(x.level > 0 && x.total == 500);
        x.give_levels(-1_000_000);
        assert_eq!((x.level, x.total), (0, 0));
        assert_eq!(x.progress.to_bits(), 0.0_f32.to_bits());
        assert_eq!(x.score, 500, "the score survives: only respawn resets it");
    }

    #[test]
    fn set_points_moves_the_bar_and_not_the_total() {
        let mut x = Experience::new();
        x.give_points(3);
        x.sync();
        let (sounds, r) = apply(
            &XpCommand::Set {
                target: "p".into(),
                amount: 5,
                unit: Unit::Points,
            },
            &mut x,
        );
        assert!(sounds.is_empty() && r.is_ok());
        assert_eq!(x.total, 3);
        assert_eq!(x.progress.to_bits(), (5.0_f32 / 7.0).to_bits());
        assert!(x.sync().is_some(), "lastSentExp = -1 forces the send");
        let (_, r) = apply(
            &XpCommand::Set {
                target: "p".into(),
                amount: 7,
                unit: Unit::Points,
            },
            &mut x,
        );
        assert!(r.is_err(), "a full level is refused");
    }

    #[test]
    fn negative_points_walk_down_through_levels() {
        let mut x = Experience::new();
        x.give_points(20); // level 1 (7), level 2 (9) -> 16, 4 into level 2
        assert_eq!(x.level, 2);
        x.give_points(-10);
        assert_eq!(x.level, 1);
        assert_eq!(x.total, 10);
    }

    #[test]
    fn crossing_level_five_by_points_plays_the_sound() {
        let mut x = Experience::new();
        x.tick_count = 1000;
        // Levels 0..4 need 7+9+11+13+15 = 55.
        let s = x.give_points(55);
        assert_eq!(x.level, 5);
        assert_eq!(s, vec![LevelUp { volume: 0.125 }]);
    }

    #[test]
    fn the_console_grammar_defaults_to_points_and_refuses_selectors() {
        assert_eq!(
            parse_xp(&["add", "oracle", "10"]),
            Ok(XpCommand::Add {
                target: "oracle".into(),
                amount: 10,
                unit: Unit::Points
            })
        );
        assert_eq!(
            parse_xp(&["add", "oracle", "-5", "levels"]),
            Ok(XpCommand::Add {
                target: "oracle".into(),
                amount: -5,
                unit: Unit::Levels
            })
        );
        assert!(parse_xp(&["add", "@a", "1"]).is_err());
        assert!(parse_xp(&["set", "oracle", "-1"]).is_err());
        assert!(parse_xp(&["query", "oracle"]).is_err());
    }
}
