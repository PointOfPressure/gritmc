//! The scoreboard's wire surface — objectives, scores, display slots and
//! teams, and the five clientbound packets that carry them. Transcribed from
//! 26.2 bytecode.
//!
//! # The enums are id-mapped, not ordinal-mapped, and one of them is not
//!
//! `DisplaySlot`, `TeamColor`, `Team.Visibility` and `Team.CollisionRule` all
//! reach the wire through `ByteBufCodecs.idMapper(BY_ID, e -> e.id)`, which
//! writes a **varint of an `id` field** set in the enum's constructor.
//! `ObjectiveCriteria.RenderType` does not: it goes through
//! `FriendlyByteBuf.writeEnum`, which writes the declaration **ordinal**.
//!
//! Today every one of those tables happens to be `0..n` in declaration order,
//! so the distinction costs nothing — which is exactly why it has to be
//! written down rather than left to be rediscovered when one of them stops
//! being. `crates/rustmc-server/golden/scoreboard.json` carries all five
//! tables as vanilla reports them and [`tests`] compares the constants below
//! against it, so a future version that renumbers one reddens a test instead
//! of mis-encoding a packet.
//!
//! # `set_player_team`'s parameter order changed and the old order still parses
//!
//! `ClientboundSetPlayerTeamPacket.Parameters.STREAM_CODEC` in 26.2 is
//!
//! ```text
//!   displayName, playerPrefix, playerSuffix,
//!   nameTagVisibility, collisionRule, optional(color), options:byte
//! ```
//!
//! The `options` bitfield is **last**, the colour is an `Optional<TeamColor>`
//! (a presence byte then a varint) rather than a bare formatting id, and the
//! two rule enums are varints rather than strings. A body written in the older
//! shape is the same length for a team with no colour, so it decodes without
//! error and puts the flags in the visibility field. The capture at
//! `set_player_team/create` in the golden is the byte-level statement of the
//! order; `set_player_team/create-defaults` is the same team with the other
//! value of both flags, so `options` is not a constant across the capture.
//!
//! `deathMessageVisibility` is a `PlayerTeam` field in 26.2 and is **not** in
//! `Parameters`, so it never reaches a client. It is absent here rather than
//! carried and unwritten: a field on a struct that no encoder reads is the
//! `ACCEPT_TELEPORTATION` shape.
//!
//! # What this module does not do
//!
//! It carries the container and the wire, not the *dispatch*. `ServerScoreboard`
//! decides which of these packets to send to whom when a score changes, and
//! that decision needs a player list this module does not own. The rules are
//! transcribed as doc comments on [`Scoreboard`]'s mutators and are checked by
//! reading only — said plainly rather than implied by a green test.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};

use rustmc_proto::{
    varint::{read_varint, write_varint},
    wire,
};
use rustmc_world::nbt::Nbt;

/// `net.minecraft.world.scores.DisplaySlot` — name and wire id.
///
/// Nineteen slots: the three real ones and sixteen team-colour sidebars. The
/// serialized names are what commands take; the ids are what the wire carries.
pub const DISPLAY_SLOTS: [(&str, i32); 19] = [
    ("list", 0),
    ("sidebar", 1),
    ("below_name", 2),
    ("sidebar.team.black", 3),
    ("sidebar.team.dark_blue", 4),
    ("sidebar.team.dark_green", 5),
    ("sidebar.team.dark_aqua", 6),
    ("sidebar.team.dark_red", 7),
    ("sidebar.team.dark_purple", 8),
    ("sidebar.team.gold", 9),
    ("sidebar.team.gray", 10),
    ("sidebar.team.dark_gray", 11),
    ("sidebar.team.blue", 12),
    ("sidebar.team.green", 13),
    ("sidebar.team.aqua", 14),
    ("sidebar.team.red", 15),
    ("sidebar.team.light_purple", 16),
    ("sidebar.team.yellow", 17),
    ("sidebar.team.white", 18),
];

/// `net.minecraft.world.scores.TeamColor` — name and wire id.
///
/// A class of its own in 26.2 rather than a `ChatFormatting` subset, so the
/// ids are 0..15 with no gaps and no `reset` entry. A transcription that
/// reaches for `ChatFormatting`'s ids gets `reset = 21` and writes a colour
/// the client cannot map.
pub const TEAM_COLORS: [(&str, i32); 16] = [
    ("black", 0),
    ("dark_blue", 1),
    ("dark_green", 2),
    ("dark_aqua", 3),
    ("dark_red", 4),
    ("dark_purple", 5),
    ("gold", 6),
    ("gray", 7),
    ("dark_gray", 8),
    ("blue", 9),
    ("green", 10),
    ("aqua", 11),
    ("red", 12),
    ("light_purple", 13),
    ("yellow", 14),
    ("white", 15),
];

/// `Team.Visibility` — the serialized names are camelCase, unlike everything
/// else in this file, because they are the strings `/team modify` takes.
pub const VISIBILITIES: [(&str, i32); 4] = [
    ("always", 0),
    ("never", 1),
    ("hideForOtherTeams", 2),
    ("hideForOwnTeam", 3),
];

/// `Team.CollisionRule`.
pub const COLLISION_RULES: [(&str, i32); 4] = [
    ("always", 0),
    ("never", 1),
    ("pushOtherTeams", 2),
    ("pushOwnTeam", 3),
];

/// `ObjectiveCriteria.RenderType` — **ordinals**, see the module docs.
pub const RENDER_TYPES: [(&str, i32); 2] = [("integer", 0), ("hearts", 1)];

/// `ClientboundSetObjectivePacket.METHOD_ADD`.
pub const METHOD_ADD: u8 = 0;
/// `ClientboundSetObjectivePacket.METHOD_REMOVE`.
pub const METHOD_REMOVE: u8 = 1;
/// `ClientboundSetObjectivePacket.METHOD_CHANGE`.
pub const METHOD_CHANGE: u8 = 2;

/// `ClientboundSetPlayerTeamPacket`'s method byte.
///
/// The two predicates that read it, `shouldHaveParameters` and
/// `shouldHavePlayerList`, are what make three of these five shapes distinct
/// on the wire.
pub mod team_method {
    /// Parameters **and** a player list.
    pub const CREATE: u8 = 0;
    /// Neither: the name and the method byte, and nothing else.
    pub const REMOVE: u8 = 1;
    /// Parameters only.
    pub const UPDATE: u8 = 2;
    /// Player list only.
    pub const JOIN: u8 = 3;
    /// Player list only.
    pub const LEAVE: u8 = 4;
}

fn id_of(table: &[(&str, i32)], name: &str, what: &str) -> io::Result<i32> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, i)| *i)
        .ok_or_else(|| io::Error::other(format!("no {what} named {name}")))
}

/// A `Component` on the wire: the network NBT form, tag byte then payload with
/// no root name.
///
/// The fourth copy of these lines in this crate, and deliberately not shared —
/// `crate::commands::write_network_nbt` states the reason: a registry payload,
/// a menu title, a chat line and a scoreboard label are four callers, and a
/// shared helper would make a later divergence between them read as a bug in
/// whichever one noticed first.
fn write_component(out: &mut impl Write, value: &Nbt) -> io::Result<()> {
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", value)?;
    if named.len() < 3 || named[1] != 0 || named[2] != 0 {
        return Err(io::Error::other(
            "named root was not the expected tag + empty-name shape",
        ));
    }
    out.write_all(&named[..1])?;
    out.write_all(&named[3..])
}

/// An unstyled `Component.literal`, which serialises as a bare `TAG_String`.
#[must_use]
pub fn literal(text: &str) -> Nbt {
    Nbt::String(text.to_string())
}

/// `net.minecraft.world.scores.Objective`, minus its back-pointer to the
/// scoreboard and its criteria object.
#[derive(Debug, Clone, PartialEq)]
pub struct Objective {
    pub name: String,
    /// The criterion's own name (`dummy`, `health`, …). Not on the wire — the
    /// client is told only the render type — but it is what
    /// `forAllObjectives` dispatches on.
    pub criteria: String,
    pub display_name: Nbt,
    /// One of [`RENDER_TYPES`].
    pub render_type: String,
    pub display_auto_update: bool,
}

/// `net.minecraft.world.scores.PlayerTeam`.
///
/// The two booleans are the whole of the `options` byte: bit 0 is
/// `allowFriendlyFire` and bit 1 is `seeFriendlyInvisibles`. Six further bits
/// exist on the wire and vanilla never sets one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerTeam {
    pub name: String,
    pub display_name: Nbt,
    pub player_prefix: Nbt,
    pub player_suffix: Nbt,
    /// One of [`VISIBILITIES`].
    pub name_tag_visibility: String,
    /// One of [`COLLISION_RULES`].
    pub collision_rule: String,
    /// One of [`TEAM_COLORS`], or `None`.
    pub color: Option<String>,
    pub allow_friendly_fire: bool,
    pub see_friendly_invisibles: bool,
    /// Insertion-ordered, because the wire carries the order and a set would
    /// not.
    pub players: Vec<String>,
}

impl PlayerTeam {
    /// `new PlayerTeam(scoreboard, name)` — the field initialisers.
    ///
    /// `allowFriendlyFire` starts **true** and `seeFriendlyInvisibles` starts
    /// **true**, so a brand-new team's `options` byte is `3` and not `0`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: literal(name),
            player_prefix: literal(""),
            player_suffix: literal(""),
            name_tag_visibility: "always".into(),
            collision_rule: "always".into(),
            color: None,
            allow_friendly_fire: true,
            see_friendly_invisibles: true,
            players: Vec::new(),
        }
    }

    /// The `options` byte.
    #[must_use]
    pub fn options(&self) -> u8 {
        u8::from(self.allow_friendly_fire) | (u8::from(self.see_friendly_invisibles) << 1)
    }
}

/// `net.minecraft.world.scores.Scoreboard` — the container, without the
/// broadcast side that `ServerScoreboard` adds.
#[derive(Debug, Clone, Default)]
pub struct Scoreboard {
    objectives: BTreeMap<String, Objective>,
    /// `playerScores`: owner -> objective name -> score.
    scores: BTreeMap<String, BTreeMap<String, i32>>,
    display: BTreeMap<String, String>,
    teams: BTreeMap<String, PlayerTeam>,
    /// `teamsByPlayer` — vanilla keeps this index beside `teamsByName` and
    /// `addPlayerToTeam` writes both. A single map would make
    /// `getPlayersTeam` a scan and, worse, would let a player sit on two
    /// teams: vanilla's `addPlayerToTeam` removes them from their current one
    /// first, and that removal is only findable through this index.
    team_by_player: BTreeMap<String, String>,
}

impl Scoreboard {
    #[must_use]
    pub fn objective(&self, name: &str) -> Option<&Objective> {
        self.objectives.get(name)
    }

    /// `getObjectives()`, by name.
    ///
    /// A `BTreeMap`'s order, which is **not** vanilla's: `Scoreboard.objectives`
    /// is a `HashMap` and `getObjectives()` returns `objectives.values()`, so
    /// vanilla's order is its own bucket order. That is only observable
    /// through `/scoreboard objectives list`, whose reply joins the names, and
    /// it is written down here rather than left to be discovered from a
    /// two-objective list disagreeing.
    #[must_use]
    pub fn objective_names(&self) -> Vec<String> {
        self.objectives.keys().cloned().collect()
    }

    /// `addObjective`. Vanilla throws on a duplicate name rather than
    /// replacing, so this returns an error instead of overwriting.
    ///
    /// Broadcast: `ServerScoreboard.onObjectiveAdded` sends nothing — an
    /// objective becomes visible to clients only when it is put in a display
    /// slot, which is what adds it to `trackedObjectives`.
    ///
    /// # Errors
    /// If an objective of that name already exists.
    pub fn add_objective(&mut self, o: Objective) -> io::Result<()> {
        if self.objectives.contains_key(&o.name) {
            return Err(io::Error::other(format!(
                "an objective named {} already exists",
                o.name
            )));
        }
        self.objectives.insert(o.name.clone(), o);
        Ok(())
    }

    /// `removeObjective` — drops the objective, every score under it, and
    /// every display slot pointing at it.
    ///
    /// Broadcast: `set_objective` with `METHOD_REMOVE`, and a
    /// `set_display_objective` with a null objective for each slot cleared.
    pub fn remove_objective(&mut self, name: &str) -> bool {
        if self.objectives.remove(name).is_none() {
            return false;
        }
        for per_owner in self.scores.values_mut() {
            per_owner.remove(name);
        }
        self.scores.retain(|_, m| !m.is_empty());
        self.display.retain(|_, v| v != name);
        true
    }

    /// `getOrCreatePlayerScore(...).set(value)`.
    ///
    /// Broadcast: `set_score`, but only if the objective is tracked.
    pub fn set_score(&mut self, owner: &str, objective: &str, value: i32) {
        self.scores
            .entry(owner.to_string())
            .or_default()
            .insert(objective.to_string(), value);
    }

    #[must_use]
    pub fn score(&self, owner: &str, objective: &str) -> Option<i32> {
        self.scores.get(owner)?.get(objective).copied()
    }

    /// `resetSinglePlayerScore`. Broadcast: `reset_score` with the objective
    /// name present.
    pub fn reset_score(&mut self, owner: &str, objective: &str) {
        if let Some(m) = self.scores.get_mut(owner) {
            m.remove(objective);
            if m.is_empty() {
                self.scores.remove(owner);
            }
        }
    }

    /// `resetAllPlayerScores`. Broadcast: `reset_score` with a **null**
    /// objective name, which is the one-byte form.
    pub fn reset_all_scores(&mut self, owner: &str) {
        self.scores.remove(owner);
    }

    /// `setDisplayObjective(slot, objective)`; `None` clears the slot.
    ///
    /// # Errors
    /// If the slot is not one of [`DISPLAY_SLOTS`].
    pub fn set_display_objective(&mut self, slot: &str, objective: Option<&str>) -> io::Result<()> {
        id_of(&DISPLAY_SLOTS, slot, "display slot")?;
        match objective {
            Some(o) => self.display.insert(slot.to_string(), o.to_string()),
            None => self.display.remove(slot),
        };
        Ok(())
    }

    #[must_use]
    pub fn display_objective(&self, slot: &str) -> Option<&str> {
        self.display.get(slot).map(String::as_str)
    }

    /// `addPlayerTeam`.
    ///
    /// # Errors
    /// If a team of that name already exists.
    pub fn add_team(&mut self, t: PlayerTeam) -> io::Result<()> {
        if self.teams.contains_key(&t.name) {
            return Err(io::Error::other(format!(
                "a team named {} already exists",
                t.name
            )));
        }
        for p in &t.players {
            self.team_by_player.insert(p.clone(), t.name.clone());
        }
        self.teams.insert(t.name.clone(), t);
        Ok(())
    }

    #[must_use]
    pub fn team(&self, name: &str) -> Option<&PlayerTeam> {
        self.teams.get(name)
    }

    #[must_use]
    pub fn team_of_player(&self, player: &str) -> Option<&PlayerTeam> {
        self.teams.get(self.team_by_player.get(player)?)
    }

    /// `addPlayerToTeam` — **removes the player from their current team
    /// first**, which is the step a one-map implementation cannot take.
    /// Returns false when the player was already on this team.
    pub fn add_player_to_team(&mut self, player: &str, team: &str) -> bool {
        if !self.teams.contains_key(team) {
            return false;
        }
        if self.team_by_player.get(player).map(String::as_str) == Some(team) {
            return false;
        }
        self.remove_player_from_team(player);
        self.team_by_player
            .insert(player.to_string(), team.to_string());
        if let Some(t) = self.teams.get_mut(team) {
            t.players.push(player.to_string());
        }
        true
    }

    /// `removePlayerFromTeam(String)`. Returns false when they were on none.
    pub fn remove_player_from_team(&mut self, player: &str) -> bool {
        let Some(name) = self.team_by_player.remove(player) else {
            return false;
        };
        if let Some(t) = self.teams.get_mut(&name) {
            t.players.retain(|p| p != player);
        }
        true
    }

    /// `removePlayerTeam` — the team goes, and every member's index entry with
    /// it. Broadcast: `set_player_team` with `REMOVE`.
    pub fn remove_team(&mut self, name: &str) -> bool {
        let Some(t) = self.teams.remove(name) else {
            return false;
        };
        for p in &t.players {
            self.team_by_player.remove(p);
        }
        true
    }
}

/// The five clientbound packets.
pub mod packets {
    use super::Objective;
    use super::{
        id_of, io, literal, read_varint, wire, write_component, write_varint, Nbt, PlayerTeam,
        Read, Write, COLLISION_RULES, DISPLAY_SLOTS, RENDER_TYPES, TEAM_COLORS, VISIBILITIES,
    };

    /// `minecraft:set_objective` — 106.
    ///
    /// `objectiveName`, the method as a **plain byte** (not a varint), then —
    /// only for `ADD` and `CHANGE` — the display name, the render type and an
    /// optional number format. `REMOVE` stops after the byte.
    ///
    /// # Errors
    /// If the render type is not one of [`RENDER_TYPES`], or the sink refuses.
    pub fn set_objective(out: &mut impl Write, o: &Objective, method: u8) -> io::Result<()> {
        wire::write_string(out, &o.name)?;
        wire::write_u8(out, method)?;
        if method == super::METHOD_ADD || method == super::METHOD_CHANGE {
            write_component(out, &o.display_name)?;
            write_varint(out, id_of(&RENDER_TYPES, &o.render_type, "render type")?)?;
            // `NumberFormatTypes.OPTIONAL_STREAM_CODEC`. Only the absent case
            // is written here: a present format is a registry-dispatched
            // payload, and a rounded-off encoding of it would be a wrong row
            // in the table that is supposed to say we are wrong.
            wire::write_bool(out, false)?;
        }
        Ok(())
    }

    /// `minecraft:set_score` — 110.
    ///
    /// The score is `ByteBufCodecs.VAR_INT`, which is a **plain** varint of a
    /// signed int, not a zigzag: `-3` is the five bytes `fd ff ff ff 0f`. The
    /// golden's `set_score/negative` is that case and nothing else in the
    /// capture would catch a zigzag.
    ///
    /// # Errors
    /// If the sink refuses a write.
    pub fn set_score(
        out: &mut impl Write,
        owner: &str,
        objective: &str,
        score: i32,
        display: Option<&Nbt>,
    ) -> io::Result<()> {
        wire::write_string(out, owner)?;
        wire::write_string(out, objective)?;
        write_varint(out, score)?;
        match display {
            Some(d) => {
                wire::write_bool(out, true)?;
                write_component(out, d)?;
            }
            None => wire::write_bool(out, false)?,
        }
        wire::write_bool(out, false)
    }

    /// `minecraft:reset_score` — 79.
    ///
    /// `objectiveName` is `writeNullable`, not an optional codec — the same
    /// one byte on the wire, but `None` means *every* objective, which is a
    /// different operation rather than a missing field.
    ///
    /// # Errors
    /// If the sink refuses a write.
    pub fn reset_score(
        out: &mut impl Write,
        owner: &str,
        objective: Option<&str>,
    ) -> io::Result<()> {
        wire::write_string(out, owner)?;
        match objective {
            Some(o) => {
                wire::write_bool(out, true)?;
                wire::write_string(out, o)
            }
            None => wire::write_bool(out, false),
        }
    }

    /// `minecraft:set_display_objective` — 98.
    ///
    /// # Errors
    /// If the slot is not one of [`DISPLAY_SLOTS`].
    pub fn set_display_objective(
        out: &mut impl Write,
        slot: &str,
        objective: &str,
    ) -> io::Result<()> {
        write_varint(out, id_of(&DISPLAY_SLOTS, slot, "display slot")?)?;
        wire::write_string(out, objective)
    }

    /// `minecraft:set_player_team` — 109.
    ///
    /// See the module docs for the parameter order, which changed in 26.2 and
    /// whose old form still parses.
    ///
    /// # Errors
    /// If a rule or colour name is unknown, or the sink refuses.
    pub fn set_player_team(
        out: &mut impl Write,
        t: &PlayerTeam,
        method: u8,
        players: &[String],
    ) -> io::Result<()> {
        wire::write_string(out, &t.name)?;
        wire::write_u8(out, method)?;
        if should_have_parameters(method) {
            write_component(out, &t.display_name)?;
            write_component(out, &t.player_prefix)?;
            write_component(out, &t.player_suffix)?;
            write_varint(
                out,
                id_of(&VISIBILITIES, &t.name_tag_visibility, "visibility")?,
            )?;
            write_varint(
                out,
                id_of(&COLLISION_RULES, &t.collision_rule, "collision rule")?,
            )?;
            match &t.color {
                Some(c) => {
                    wire::write_bool(out, true)?;
                    write_varint(out, id_of(&TEAM_COLORS, c, "team colour")?)?;
                }
                None => wire::write_bool(out, false)?,
            }
            wire::write_u8(out, t.options())?;
        }
        if should_have_player_list(method) {
            write_varint(out, i32::try_from(players.len()).map_err(io::Error::other)?)?;
            for p in players {
                wire::write_string(out, p)?;
            }
        }
        Ok(())
    }

    /// `ClientboundSetPlayerTeamPacket.shouldHaveParameters` — `method == 0 ||
    /// method == 2`.
    #[must_use]
    pub fn should_have_parameters(method: u8) -> bool {
        method == super::team_method::CREATE || method == super::team_method::UPDATE
    }

    /// `ClientboundSetPlayerTeamPacket.shouldHavePlayerList` — `method == 0 ||
    /// method >= 3`.
    #[must_use]
    pub fn should_have_player_list(method: u8) -> bool {
        method == super::team_method::CREATE || method >= super::team_method::JOIN
    }

    /// Decode `reset_score`, the one packet in this set whose body is entirely
    /// this crate's own primitives — no component, no registry.
    ///
    /// # Errors
    /// If the stream ends early.
    pub fn read_reset_score(r: &mut impl Read) -> io::Result<(String, Option<String>)> {
        let owner = wire::read_string(r)?;
        let objective = if wire::read_bool(r)? {
            Some(wire::read_string(r)?)
        } else {
            None
        };
        Ok((owner, objective))
    }

    /// Decode `set_display_objective`, likewise.
    ///
    /// # Errors
    /// If the stream ends early or the slot id is not in [`DISPLAY_SLOTS`].
    pub fn read_set_display_objective(r: &mut impl Read) -> io::Result<(String, String)> {
        let id = read_varint(r)?;
        let slot = DISPLAY_SLOTS
            .iter()
            .find(|(_, i)| *i == id)
            .map(|(n, _)| (*n).to_string())
            .ok_or_else(|| io::Error::other(format!("no display slot with id {id}")))?;
        Ok((slot, wire::read_string(r)?))
    }

    /// An objective whose display name is a plain literal, for callers that
    /// have no styled component to offer.
    #[must_use]
    pub fn plain_objective(name: &str, criteria: &str, render_type: &str) -> Objective {
        Objective {
            name: name.to_string(),
            criteria: criteria.to_string(),
            display_name: literal(name),
            render_type: render_type.to_string(),
            display_auto_update: false,
        }
    }
}
