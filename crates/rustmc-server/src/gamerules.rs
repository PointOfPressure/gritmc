//! The game rule registry: what vanilla's `GameRules` typed store actually
//! is, and the subset of it this server does anything with.
//!
//! # From the jar, clean-room
//!
//! `javap -p -c -constants` against
//! `net/minecraft/world/level/gamerules/{GameRuleType,GameRule,GameRules}.class`
//! in the pinned 26.2 jar (`oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar`).
//!
//! * **The type set is exactly two: `INT` and `BOOL`.** `GameRuleType` is a
//!   two-variant enum (`net/minecraft/world/level/gamerules/GameRuleType`)
//!   and every one of the 59 rules `GameRules`'s static initializer
//!   registers is built by `registerBoolean` or `registerInteger` — no third
//!   kind exists in this version. Older notes describing a `GameType` or
//!   float-valued rule are for a different version and do not apply here.
//! * **Storage is per-level.** `GameRuleCommand.setRule`
//!   (`net/minecraft/server/commands/GameRuleCommand`) reads
//!   `CommandSourceStack.getLevel().getGameRules()` — a `ServerLevel` owns
//!   its own `GameRules`, not the server. [`GameRules`] below is
//!   process-wide rather than per-level because this server serves one
//!   world; the day it serves more than one, this is where that split goes.
//! * **`/gamerule` requires `LEVEL_GAMEMASTERS` (permission level 2).**
//!   `GameRuleCommand.register`'s `requires` clause, confirmed independently
//!   by `tools/oracle-runner/src/record.rs`'s `Console` doc, which is why
//!   the harness never sends `gamerule` as a chat command.
//! * **`[[59 names, type, default]]` are transcribed below** from the
//!   `registerBoolean`/`registerInteger` call sequence in
//!   `GameRules.<clinit>`, in registration order, reading the pushed
//!   constant immediately before each `invokestatic`. `random_tick_speed`'s
//!   default (3) and minimum (0) match `crate::randomtick`'s existing doc
//!   comment, which was derived independently — the two agree.
//!
//! # The subset this server implements, and why the rest is data only
//!
//! **Every rule name and its type/default is in [`DEFAULTS`] and is
//! settable and queryable through [`GameRules::set`]/[`GameRules::get`] —
//! that much is a complete, typed store, because it costs nothing beyond
//! transcribing a table vanilla already publishes.** What is **not** here is
//! any behaviour for 58 of the 59: `mob_griefing` can be set to `false` and
//! read back, but nothing in this server consults it, because there are no
//! mobs to grief. That is the same shape as every other "derived but not
//! wired" system in this codebase (`crate::tick`'s module doc lists several;
//! `rustmc_entity::tick_list` is another) and is named here rather than left
//! to be discovered as a silent gap.
//!
//! **`random_tick_speed` is the one exception**, because it is the one rule
//! a corpus scenario needs: `crate::randomtick::tick` reads it every tick.
//! Wiring a second rule's behaviour is a separate, scoped piece of work with
//! its own bytecode read, not a byproduct of this one.
//!
//! # The console, not `/gamerule`
//!
//! There is deliberately no chat-command surface here. `crate::commands`'s
//! own header explains why a command answered over the network has to match
//! vanilla's wire bytes exactly (`ClientboundCommandsPacket`, a `system_chat`
//! reply built from the same NBT vanilla's Brigadier exception would send) —
//! building that for an argument-taking command is real work with no payoff
//! for this item, because the harness the whole subsystem exists for talks
//! to a fifo, not a client. [`crate::console`] is that surface: a real
//! server capability (a dedicated server's console is a first-party vanilla
//! feature, not a harness backdoor), dispatched with full permission because
//! a process's own stdin has no `PermissionCheck` to fail — matching
//! `MinecraftServer.createCommandSourceStack()`, which vanilla's own
//! `ConsoleInput` is built with.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// `net/minecraft/world/level/gamerules/GameRuleType` — `INT` and `BOOL`,
/// and nothing else exists in this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleType {
    Bool,
    Int,
}

/// One rule's value, carrying its own type so a caller cannot read an int
/// rule as a bool by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameRuleValue {
    Bool(bool),
    Int(i32),
}

impl GameRuleValue {
    #[must_use]
    pub const fn kind(self) -> GameRuleType {
        match self {
            Self::Bool(_) => GameRuleType::Bool,
            Self::Int(_) => GameRuleType::Int,
        }
    }

    #[must_use]
    pub const fn as_int(self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(v),
            Self::Bool(_) => None,
        }
    }

    /// The bool half of [`Self::as_int`], and `None` for an int for the same
    /// reason: reading `random_tick_speed` as a boolean should be a miss and
    /// not a `true`.
    #[must_use]
    pub const fn as_bool(self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(v),
            Self::Int(_) => None,
        }
    }

    /// `GameRule.serialize` — vanilla's own string form, `"true"`/`"false"`
    /// for a bool and the plain decimal for an int. What a console `gamerule
    /// <name>` query with no value would answer.
    #[must_use]
    pub fn serialize(self) -> String {
        match self {
            Self::Bool(b) => b.to_string(),
            Self::Int(i) => i.to_string(),
        }
    }
}

/// name -> (type, vanilla default). See the module doc for where every one
/// of these 59 entries comes from. Registration order, matching the jar's
/// `<clinit>` — not alphabetised, so a diff against a re-run of the same
/// `javap` stays a diff of values and not of reordering.
pub const DEFAULTS: &[(&str, GameRuleValue)] = &[
    ("advance_time", GameRuleValue::Bool(true)),
    ("advance_weather", GameRuleValue::Bool(true)),
    (
        "allow_entering_nether_using_portals",
        GameRuleValue::Bool(true),
    ),
    ("block_drops", GameRuleValue::Bool(true)),
    ("block_explosion_drop_decay", GameRuleValue::Bool(true)),
    ("command_blocks_work", GameRuleValue::Bool(true)),
    ("command_block_output", GameRuleValue::Bool(true)),
    ("drowning_damage", GameRuleValue::Bool(true)),
    ("elytra_movement_check", GameRuleValue::Bool(true)),
    ("ender_pearls_vanish_on_death", GameRuleValue::Bool(true)),
    ("entity_drops", GameRuleValue::Bool(true)),
    ("fall_damage", GameRuleValue::Bool(true)),
    ("fire_damage", GameRuleValue::Bool(true)),
    ("fire_spread_radius_around_player", GameRuleValue::Int(128)),
    ("forgive_dead_players", GameRuleValue::Bool(true)),
    ("freeze_damage", GameRuleValue::Bool(true)),
    ("global_sound_events", GameRuleValue::Bool(true)),
    ("immediate_respawn", GameRuleValue::Bool(false)),
    ("keep_inventory", GameRuleValue::Bool(false)),
    ("lava_source_conversion", GameRuleValue::Bool(false)),
    ("limited_crafting", GameRuleValue::Bool(false)),
    ("locator_bar", GameRuleValue::Bool(true)),
    ("log_admin_commands", GameRuleValue::Bool(true)),
    ("max_block_modifications", GameRuleValue::Int(32768)),
    ("max_command_forks", GameRuleValue::Int(65536)),
    ("max_command_sequence_length", GameRuleValue::Int(65536)),
    ("max_entity_cramming", GameRuleValue::Int(24)),
    ("max_minecart_speed", GameRuleValue::Int(8)),
    ("max_snow_accumulation_height", GameRuleValue::Int(1)),
    ("mob_drops", GameRuleValue::Bool(true)),
    ("mob_explosion_drop_decay", GameRuleValue::Bool(true)),
    ("mob_griefing", GameRuleValue::Bool(true)),
    ("natural_health_regeneration", GameRuleValue::Bool(true)),
    ("player_movement_check", GameRuleValue::Bool(true)),
    (
        "players_nether_portal_creative_delay",
        GameRuleValue::Int(0),
    ),
    (
        "players_nether_portal_default_delay",
        GameRuleValue::Int(80),
    ),
    ("players_sleeping_percentage", GameRuleValue::Int(100)),
    ("projectiles_can_break_blocks", GameRuleValue::Bool(true)),
    ("pvp", GameRuleValue::Bool(true)),
    ("raids", GameRuleValue::Bool(true)),
    ("random_tick_speed", GameRuleValue::Int(3)),
    ("reduced_debug_info", GameRuleValue::Bool(false)),
    ("respawn_radius", GameRuleValue::Int(10)),
    ("send_command_feedback", GameRuleValue::Bool(true)),
    ("show_advancement_messages", GameRuleValue::Bool(true)),
    ("show_death_messages", GameRuleValue::Bool(true)),
    ("spawner_blocks_work", GameRuleValue::Bool(true)),
    ("spawn_mobs", GameRuleValue::Bool(true)),
    ("spawn_monsters", GameRuleValue::Bool(true)),
    ("spawn_patrols", GameRuleValue::Bool(true)),
    ("spawn_phantoms", GameRuleValue::Bool(true)),
    ("spawn_wandering_traders", GameRuleValue::Bool(true)),
    ("spawn_wardens", GameRuleValue::Bool(true)),
    ("spectators_generate_chunks", GameRuleValue::Bool(true)),
    ("spread_vines", GameRuleValue::Bool(true)),
    ("tnt_explodes", GameRuleValue::Bool(true)),
    ("tnt_explosion_drop_decay", GameRuleValue::Bool(false)),
    ("universal_anger", GameRuleValue::Bool(false)),
    ("water_source_conversion", GameRuleValue::Bool(true)),
];

/// The live store. One process, one world, so one instance — see the module
/// doc's "storage is per-level" paragraph for what changes if that stops
/// being true.
pub struct GameRules {
    values: Mutex<HashMap<String, GameRuleValue>>,
}

/// Why a `set` did not take effect. Distinguished so a console caller (and a
/// test) can tell "not a rule this version has" from "wrong shape for the
/// rule it is" rather than reading both as one silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    UnknownRule(String),
    WrongType {
        rule: String,
        wants: GameRuleType,
        got: String,
    },
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownRule(r) => write!(f, "no such game rule: {r}"),
            Self::WrongType { rule, wants, got } => {
                let shape = match wants {
                    GameRuleType::Bool => "true or false",
                    GameRuleType::Int => "an integer",
                };
                write!(f, "{rule} takes {shape}, got {got:?}")
            }
        }
    }
}

impl GameRules {
    /// The process-wide instance, seeded from [`DEFAULTS`] on first use.
    #[must_use]
    pub fn shared() -> &'static Self {
        static INSTANCE: OnceLock<GameRules> = OnceLock::new();
        INSTANCE.get_or_init(Self::fresh)
    }

    /// A fresh, defaults-seeded instance, independent of [`Self::shared`]'s
    /// process-wide singleton.
    ///
    /// `pub` rather than `#[cfg(test)]`: `crate::console`'s own tests live in
    /// a different module and need one that will not interfere with any
    /// other test touching the real singleton — a `#[cfg(test)]` item is not
    /// visible across a crate's module boundary the way a plain `pub` one
    /// is, and gating this behind `cfg(test)` would need every caller built
    /// with `--cfg test`, which `cargo test -p rustmc-server` gives every
    /// module in the crate but which is easy to get wrong one module at a
    /// time. A non-test caller constructing one is harmless: it is simply
    /// another, unconnected store.
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            values: Mutex::new(
                DEFAULTS
                    .iter()
                    .map(|(name, value)| ((*name).to_string(), *value))
                    .collect(),
            ),
        }
    }

    /// The rule's current value, or `None` if `rule` names nothing in
    /// [`DEFAULTS`].
    #[must_use]
    pub fn get(&self, rule: &str) -> Option<GameRuleValue> {
        self.values
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(rule)
            .copied()
    }

    /// Parse `value` against the rule's own type and store it.
    ///
    /// Vanilla's own string forms: `Boolean::parseBoolean`-equivalent
    /// (`"true"` is true, everything else is false — **not** rejected, see
    /// `GameRule.deserialize`'s codec, `Codec.BOOL` via
    /// `ExtraCodecs.validate`, which is exactly `parseBoolean`'s
    /// permissiveness) for a bool rule, `i32::parse` for an int one.
    ///
    /// # Errors
    /// [`SetError::UnknownRule`] if `rule` is not in [`DEFAULTS`];
    /// [`SetError::WrongType`] if `value` does not parse as that rule's type.
    /// Vanilla's own min/max clamps (`registerInteger`'s bounds) are **not**
    /// enforced here — see the module doc's scope note; only
    /// `random_tick_speed` is read back by anything, and any `i32` is a
    /// meaningful tick rate.
    pub fn set(&self, rule: &str, value: &str) -> Result<GameRuleValue, SetError> {
        let mut values = self.values.lock().unwrap_or_else(|e| e.into_inner());
        let Some(existing) = values.get(rule).copied() else {
            return Err(SetError::UnknownRule(rule.to_string()));
        };
        let parsed = match existing.kind() {
            // `"true"` is true, anything else is false — vanilla's
            // `Boolean.parseBoolean`, not a strict `bool::from_str`, so
            // `gamerule keepInventory yes` is a silent `false` in vanilla
            // too and this matches it rather than "improving" on it.
            GameRuleType::Bool => GameRuleValue::Bool(value == "true"),
            GameRuleType::Int => {
                value
                    .parse::<i32>()
                    .map(GameRuleValue::Int)
                    .map_err(|_| SetError::WrongType {
                        rule: rule.to_string(),
                        wants: GameRuleType::Int,
                        got: value.to_string(),
                    })?
            }
        };
        values.insert(rule.to_string(), parsed);
        Ok(parsed)
    }

    /// `random_tick_speed`, as an `i32` — the one rule
    /// `crate::randomtick::tick` reads. `0` if the store somehow has no
    /// entry for it, which cannot happen through [`Self::shared`] but would
    /// through [`Self::set`] on a hand-built instance in a test; a wrong
    /// type there is a programming error in this module, not a runtime
    /// input, so it also falls back to `0` rather than panicking a
    /// connection thread.
    #[must_use]
    pub fn random_tick_speed(&self) -> i32 {
        self.get("random_tick_speed")
            .and_then(GameRuleValue::as_int)
            .unwrap_or(0)
    }

    /// A boolean rule, falling back to `false` for the same reason
    /// [`Self::random_tick_speed`] falls back to `0`: a missing or
    /// wrong-typed entry cannot happen through [`Self::shared`], and where
    /// it can it is a programming error in this module rather than a runtime
    /// input, so it must not panic a connection thread.
    ///
    /// The fallback is deliberately `false` rather than the rule's own
    /// default, because a caller that reaches a name this store does not
    /// have should see the flat, safe answer and not a plausible one.
    #[must_use]
    pub fn boolean(&self, rule: &str) -> bool {
        self.get(rule)
            .and_then(GameRuleValue::as_bool)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> GameRules {
        GameRules::fresh()
    }

    #[test]
    fn defaults_has_fifty_nine_entries_and_no_duplicate_names() {
        assert_eq!(
            DEFAULTS.len(),
            59,
            "the jar's GameRules.<clinit> registers 59 rules"
        );
        let mut names: Vec<&str> = DEFAULTS.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            DEFAULTS.len(),
            "a repeated name would shadow silently"
        );
    }

    #[test]
    fn random_tick_speed_defaults_to_three() {
        assert_eq!(fresh().random_tick_speed(), 3);
    }

    #[test]
    fn set_random_tick_speed_and_read_it_back() {
        let g = fresh();
        assert_eq!(
            g.set("random_tick_speed", "1000"),
            Ok(GameRuleValue::Int(1000))
        );
        assert_eq!(g.random_tick_speed(), 1000);
        assert_eq!(g.set("random_tick_speed", "0"), Ok(GameRuleValue::Int(0)));
        assert_eq!(g.random_tick_speed(), 0);
    }

    #[test]
    fn an_unknown_rule_is_named_in_the_error() {
        let g = fresh();
        assert_eq!(
            g.set("doFireTick", "true"),
            Err(SetError::UnknownRule("doFireTick".to_string()))
        );
    }

    #[test]
    fn a_bool_rule_given_a_non_integer_shaped_string_is_just_false() {
        // Matches vanilla's own permissiveness (`Boolean.parseBoolean`), not
        // a strict parse — see `GameRules::set`'s doc comment.
        let g = fresh();
        assert_eq!(g.set("mob_griefing", "nah"), Ok(GameRuleValue::Bool(false)));
    }

    #[test]
    fn an_int_rule_given_a_non_integer_string_is_a_typed_error() {
        let g = fresh();
        assert_eq!(
            g.set("random_tick_speed", "fast"),
            Err(SetError::WrongType {
                rule: "random_tick_speed".to_string(),
                wants: GameRuleType::Int,
                got: "fast".to_string(),
            })
        );
        // And the store is untouched by the rejected write.
        assert_eq!(g.random_tick_speed(), 3);
    }

    #[test]
    fn serialize_round_trips_vanillas_own_string_forms() {
        assert_eq!(GameRuleValue::Bool(true).serialize(), "true");
        assert_eq!(GameRuleValue::Bool(false).serialize(), "false");
        assert_eq!(GameRuleValue::Int(1000).serialize(), "1000");
    }
}

#[cfg(test)]
mod boolean_tests {
    use super::*;

    /// The three booleans `ClientboundLoginPacket` carries all exist in this
    /// store and are all `false` by default, which is what makes the join
    /// packet's captured bytes reproducible from the rules rather than from
    /// literals.
    #[test]
    fn the_three_login_packet_rules_are_all_present_and_false() {
        let r = GameRules::fresh();
        for name in [
            "immediate_respawn",
            "reduced_debug_info",
            "limited_crafting",
        ] {
            assert_eq!(
                r.get(name),
                Some(GameRuleValue::Bool(false)),
                "{name} must be a bool defaulting to false"
            );
            assert!(!r.boolean(name));
        }
    }

    /// Setting one moves what `boolean` answers — the positive control that
    /// this getter reads the store rather than a constant.
    #[test]
    fn setting_a_boolean_rule_moves_the_getter() {
        let r = GameRules::fresh();
        assert!(!r.boolean("immediate_respawn"));
        r.set("immediate_respawn", "true").expect("set");
        assert!(r.boolean("immediate_respawn"));
        assert!(
            !crate::death::show_death_screen(r.boolean("immediate_respawn")),
            "the join packet's showDeathScreen follows it, inverted"
        );
    }

    /// An int rule read as a bool is a miss, not a `true`. Without this,
    /// `boolean("random_tick_speed")` would answer for a rule of the wrong
    /// kind.
    #[test]
    fn an_int_rule_is_not_a_boolean() {
        assert_eq!(GameRuleValue::Int(3).as_bool(), None);
        assert_eq!(GameRuleValue::Bool(true).as_int(), None);
        let r = GameRules::fresh();
        assert!(!r.boolean("random_tick_speed"));
        assert!(!r.boolean("no_such_rule_exists"));
    }
}
