//! `/scoreboard objectives`' executors: the packets they broadcast and the six
//! reply components.
//!
//! [`crate::scoreboard`] is the transcription — the objective, the team, the
//! five packet encodings and the 45 enum ids, all byte-gated against
//! `crates/rustmc-server/golden/scoreboard.json`. This module is the wiring.
//!
//! # What is here, and what is refused by name
//!
//! `objectives list`, `objectives add`, `objectives remove` and
//! `objectives setdisplay` in both its forms. Not `objectives modify`, not any
//! `players` branch — [`crate::commands::scoreboard_root`] carries the reason
//! for each, and neither is registered in the tree, so nothing is offered that
//! is not answered.
//!
//! # ADDING AN OBJECTIVE SENDS NOTHING, AND THAT IS THE FINDING HERE
//!
//! `ServerScoreboard.onObjectiveAdded` is three instructions — `super`, then
//! `setDirty()`. It broadcasts **no packet at all**. So
//! `/scoreboard objectives add kills dummy` is invisible on the wire, and a
//! port that answered it with a `set_objective` would be sending a packet
//! vanilla does not send, on a channel `tools/oracle-runner` digests whole.
//!
//! Every scoreboard packet goes through **tracking**, which is
//! `ServerScoreboard`'s own `trackedObjectives` set and nothing in the plain
//! `Scoreboard`:
//!
//! * `setDisplayObjective(slot, objective)`, when the objective is not yet
//!   tracked, calls `startTrackingObjective` — and *that* sends
//!   `getStartTrackingPackets`: `set_objective` **METHOD_ADD** first, then one
//!   `set_display_objective` for every slot already showing it, then its
//!   scores. When it IS already tracked, only the one
//!   `set_display_objective` goes out.
//! * the same call, for the objective being *displaced*, sends
//!   `set_display_objective(slot, new)` if that objective still occupies
//!   another slot, and otherwise `stopTrackingObjective` — `set_objective`
//!   **METHOD_REMOVE** plus a `set_display_objective` per slot it still holds.
//! * `onObjectiveRemoved` calls `stopTrackingObjective` **only if the
//!   objective was tracked**, and `Scoreboard.removeObjective` has already
//!   nulled its display slots by then, so a removal is one packet or none.
//!
//! The consequence a port has to get right: **the first packet a client ever
//! sees for an objective is the `set_objective` that `setdisplay` emits**, not
//! one from `add`. [`ServerScoreboard`] below carries the tracked set for
//! exactly this reason; a `Scoreboard` alone cannot produce the right packets.
//!
//! # `getFormattedDisplayName` is not the display name
//!
//! Three of the six replies carry it and it is
//! `ComponentUtils.wrapInSquareBrackets(displayName.copy().withStyle(s ->
//! s.withHoverEvent(new ShowText(Component.literal(name)))))` — a
//! `chat.square_brackets` translation whose one argument is the display name
//! carrying a hover event of the objective's *internal* name. A port that
//! passed the plain display name renders almost right and encodes nothing
//! like it. `commands::tests::the_scoreboard_replies_match_vanillas_codec`
//! compares against vanilla's own bytes for a real `Objective` on a real
//! `Scoreboard`.

use rustmc_proto::ids;
use rustmc_world::nbt::Nbt;

use crate::commands::{translatable, Arg};
use crate::scoreboard::{packets, Objective, Scoreboard, METHOD_ADD, METHOD_REMOVE};

/// The one scoreboard this process serves.
///
/// `MinecraftServer` holds a single `ServerScoreboard` and every level reads
/// it through `getScoreboard()`, so one cell is vanilla's shape. A `Mutex` for
/// the reason [`crate::worldborder::shared`] gives: an objective add is a map
/// insert plus a display-slot scan, and a reader that caught it half-done
/// would see a display slot pointing at an objective that is not in the map.
pub fn shared() -> &'static std::sync::Mutex<ServerScoreboard> {
    static BOARD: std::sync::OnceLock<std::sync::Mutex<ServerScoreboard>> =
        std::sync::OnceLock::new();
    BOARD.get_or_init(|| std::sync::Mutex::new(ServerScoreboard::default()))
}

/// Why a `/scoreboard` could not run — vanilla's own `ERROR_*`.
///
/// Rendered nowhere, for the reason [`crate::bordercmd::BorderError`] gives:
/// an executor's `SimpleCommandExceptionType` takes
/// `Commands.executeCommand`'s catch and not `finishParsing`'s, and that path
/// is not ported. Carried so the day it is ported the caller knows the message
/// it owes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreboardError {
    /// `ERROR_OBJECTIVE_ALREADY_EXISTS`.
    ObjectiveAlreadyExists,
    /// The objective named is not on the board. Vanilla never raises this from
    /// an executor because `ObjectiveArgument.getObjective` raises
    /// `ERROR_NO_SUCH_OBJECTIVE` during argument *resolution*, which this
    /// server does not do — its `minecraft:objective` parse is
    /// `readUnquotedString` and nothing more, exactly as vanilla's parse is.
    /// So the lookup lands here instead, one layer later, and the variant is
    /// named for where vanilla's message comes from.
    NoSuchObjective,
    /// `ERROR_DISPLAY_SLOT_ALREADY_SET`.
    DisplaySlotAlreadySet,
    /// `ERROR_DISPLAY_SLOT_ALREADY_EMPTY`.
    DisplaySlotAlreadyEmpty,
    /// The criterion or slot did not re-read. Unreachable through
    /// `Tree::resolve`, which ran the same parser to accept it.
    Unreadable(&'static str),
    /// A `scoreboard` path with no executor here.
    NoSuchPath(String),
}

impl std::fmt::Display for ScoreboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// What an executor produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// In `ServerScoreboard`'s own order. Empty for `list`, which only
    /// answers.
    pub packets: Vec<(i32, Vec<u8>)>,
    /// The `sendSuccess` component.
    pub reply: Nbt,
    /// The Brigadier `Command` return value.
    pub result: i32,
}

/// `Objective.getFormattedDisplayName()`.
///
/// `wrapInSquareBrackets(displayName.copy().withStyle(s ->
/// s.withHoverEvent(new HoverEvent.ShowText(Component.literal(name)))))`.
///
/// The hover carries the objective's **internal name**, which for an objective
/// created by `/scoreboard objectives add` is the same string as its display
/// name — `addObjective` passes `Component.literal(name)` when no display name
/// is given — so the two look identical in the rendered text and are in
/// different places in the NBT.
#[must_use]
pub fn formatted_display_name(name: &str, display: &str) -> Nbt {
    let inner = crate::commands::compound([
        ("text", Nbt::String(display.to_owned())),
        (
            "hover_event",
            crate::commands::compound([
                ("action", Nbt::String("show_text".into())),
                // `Component.literal(name)`, unstyled, which serialises as a
                // bare TAG_String and not as `{text: "..."}`.
                ("value", Nbt::String(name.to_owned())),
            ]),
        ),
    ]);
    crate::commands::compound([
        ("with", Nbt::List(vec![inner])),
        ("translate", Nbt::String("chat.square_brackets".into())),
    ])
}

/// `net.minecraft.server.ServerScoreboard` — the plain board plus the one
/// field that decides every packet.
///
/// `trackedObjectives` is a `Set<Objective>` on the server subclass and is
/// absent from `Scoreboard`; without it an objective's `set_objective` has no
/// trigger and `setdisplay` produces the wrong packet count. Kept here rather
/// than added to [`crate::scoreboard::Scoreboard`] because that module is the
/// container's transcription and this is the server's.
#[derive(Debug, Clone, Default)]
pub struct ServerScoreboard {
    /// The container.
    pub board: Scoreboard,
    /// `trackedObjectives`, by objective name.
    tracked: std::collections::BTreeSet<String>,
}

impl ServerScoreboard {
    /// `getStartTrackingPackets(objective)` — `set_objective` METHOD_ADD, then
    /// one `set_display_objective` per slot already showing it, then its
    /// scores.
    ///
    /// The scores are **not** produced: `listPlayerScores` needs the score
    /// map's iteration order and `ClientboundSetScorePacket`'s optional
    /// display name and number format, and no `/scoreboard players` branch is
    /// registered, so nothing here can put a score on the board in the first
    /// place. That is an emptiness with a cause rather than an omission.
    fn start_tracking(&self, o: &Objective) -> Vec<(i32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut body = Vec::new();
        if packets::set_objective(&mut body, o, METHOD_ADD).is_ok() {
            out.push((ids::play::cb::SET_OBJECTIVE, body));
        }
        for (slot, _) in crate::scoreboard::DISPLAY_SLOTS {
            if self.board.display_objective(slot) == Some(o.name.as_str()) {
                let mut b = Vec::new();
                if packets::set_display_objective(&mut b, slot, &o.name).is_ok() {
                    out.push((ids::play::cb::SET_DISPLAY_OBJECTIVE, b));
                }
            }
        }
        out
    }

    /// `getStopTrackingPackets(objective)` — the mirror: `set_objective`
    /// METHOD_REMOVE, then a cleared `set_display_objective` per slot that
    /// still shows it.
    fn stop_tracking(&self, o: &Objective) -> Vec<(i32, Vec<u8>)> {
        let mut out = Vec::new();
        let mut body = Vec::new();
        if packets::set_objective(&mut body, o, METHOD_REMOVE).is_ok() {
            out.push((ids::play::cb::SET_OBJECTIVE, body));
        }
        for (slot, _) in crate::scoreboard::DISPLAY_SLOTS {
            if self.board.display_objective(slot) == Some(o.name.as_str()) {
                let mut b = Vec::new();
                if packets::set_display_objective(&mut b, slot, "").is_ok() {
                    out.push((ids::play::cb::SET_DISPLAY_OBJECTIVE, b));
                }
            }
        }
        out
    }

    /// `getObjectiveDisplaySlotCount(objective)`.
    fn display_slot_count(&self, name: &str) -> usize {
        crate::scoreboard::DISPLAY_SLOTS
            .iter()
            .filter(|(slot, _)| self.board.display_objective(slot) == Some(name))
            .count()
    }
}

/// `addObjective(source, name, criteria, displayName)`.
///
/// `ScoreboardCommand` passes `criteria.getDefaultRenderType()` and
/// `displayAutoUpdate = false`, and the display name a two-argument
/// `/scoreboard objectives add` gets is `Component.literal(name)`.
///
/// **No packet.** `ServerScoreboard.onObjectiveAdded` broadcasts nothing; see
/// the module header.
///
/// # Errors
/// [`ScoreboardError::ObjectiveAlreadyExists`].
pub fn add_objective(
    s: &mut ServerScoreboard,
    name: &str,
    criteria: &str,
) -> Result<Outcome, ScoreboardError> {
    if s.board.objective(name).is_some() {
        return Err(ScoreboardError::ObjectiveAlreadyExists);
    }
    let objective = Objective {
        name: name.to_owned(),
        display_name: crate::scoreboard::literal(name),
        criteria: criteria.to_owned(),
        render_type: default_render_type(criteria).to_owned(),
        display_auto_update: false,
    };
    s.board
        .add_objective(objective)
        .map_err(|_| ScoreboardError::ObjectiveAlreadyExists)?;
    Ok(Outcome {
        packets: Vec::new(),
        reply: translatable(
            "commands.scoreboard.objectives.add.success",
            &[Arg::Component(formatted_display_name(name, name))],
        ),
        // `iconst_0; ireturn`.
        result: 0,
    })
}

/// `ObjectiveCriteria.getDefaultRenderType()`.
///
/// Every criterion registered by the one-argument `registerCustom` takes
/// `RenderType.INTEGER`; the three-argument form is used for `health`, `food`,
/// `air`, `armor`, `xp` and `level`, and only `health` declares `HEARTS` —
/// `<clinit>` offsets 73..139, where the render type is the third pushed
/// argument.
///
/// A wrong render type is one byte in `set_objective` and nothing else, so it
/// is invisible to every check that does not read that packet.
fn default_render_type(criteria: &str) -> &'static str {
    if criteria == "health" {
        "hearts"
    } else {
        "integer"
    }
}

/// `removeObjective(source, objective)`.
///
/// `Scoreboard.removeObjective` clears the display slots first, so by the time
/// `stopTrackingObjective` runs there is nothing left to clear and the packet
/// list is exactly one `set_objective` METHOD_REMOVE — **or none at all**, if
/// the objective was never displayed and so never tracked.
///
/// # Errors
/// [`ScoreboardError::NoSuchObjective`].
pub fn remove_objective(s: &mut ServerScoreboard, name: &str) -> Result<Outcome, ScoreboardError> {
    let objective = s
        .board
        .objective(name)
        .ok_or(ScoreboardError::NoSuchObjective)?
        .clone();
    // `Scoreboard.onObjectiveRemoved` runs through `super` first, and it is
    // what nulls the display slots.
    s.board.remove_objective(name);
    let packets = if s.tracked.remove(name) {
        s.stop_tracking(&objective)
    } else {
        Vec::new()
    };
    Ok(Outcome {
        packets,
        reply: translatable(
            "commands.scoreboard.objectives.remove.success",
            &[Arg::Component(formatted_display_name(name, name))],
        ),
        // `getObjectives().size()` AFTER the removal.
        result: i32::try_from(s.board.objective_names().len()).unwrap_or(i32::MAX),
    })
}

/// `setDisplaySlot(source, slot, objective)`, through
/// `ServerScoreboard.setDisplayObjective`.
///
/// The command's refusal is an **identity** comparison against what the slot
/// already holds, so setting a slot to the objective it already shows is
/// `ERROR_DISPLAY_SLOT_ALREADY_SET` and setting it to a different one is not.
///
/// # Errors
/// [`ScoreboardError::NoSuchObjective`], [`ScoreboardError::DisplaySlotAlreadySet`].
pub fn set_display_slot(
    s: &mut ServerScoreboard,
    slot: &str,
    objective: &str,
) -> Result<Outcome, ScoreboardError> {
    let target = s
        .board
        .objective(objective)
        .ok_or(ScoreboardError::NoSuchObjective)?
        .clone();
    if s.board.display_objective(slot) == Some(objective) {
        return Err(ScoreboardError::DisplaySlotAlreadySet);
    }
    let old = s.board.display_objective(slot).map(str::to_owned);
    s.board
        .set_display_objective(slot, Some(objective))
        .map_err(|_| ScoreboardError::Unreadable("slot"))?;
    let mut out = Vec::new();
    // `if (old != objective && old != null)`, and the arms are not symmetric:
    // an objective still shown somewhere else gets a `set_display_objective`
    // naming the NEW occupant of this slot; one shown nowhere else is untracked.
    if let Some(old) = old.filter(|o| o != objective) {
        if s.display_slot_count(&old) > 0 {
            let mut b = Vec::new();
            if packets::set_display_objective(&mut b, slot, objective).is_ok() {
                out.push((ids::play::cb::SET_DISPLAY_OBJECTIVE, b));
            }
        } else if s.tracked.remove(&old) {
            if let Some(o) = s.board.objective(&old).cloned() {
                out.extend(s.stop_tracking(&o));
            }
        }
    }
    if s.tracked.contains(objective) {
        let mut b = Vec::new();
        if packets::set_display_objective(&mut b, slot, objective).is_ok() {
            out.push((ids::play::cb::SET_DISPLAY_OBJECTIVE, b));
        }
    } else {
        out.extend(s.start_tracking(&target));
        s.tracked.insert(objective.to_owned());
    }
    Ok(Outcome {
        packets: out,
        // The slot is a String and the objective is a COMPONENT — the plain
        // display name, not the formatted one — so `ListTag.write`'s
        // heterogeneous rule wraps both. `display.cleared` below takes one
        // String and wraps nothing.
        reply: translatable(
            "commands.scoreboard.objectives.display.set",
            &[
                Arg::Text(slot.to_owned()),
                Arg::Component(target.display_name.clone()),
            ],
        ),
        result: 0,
    })
}

/// `clearDisplaySlot(source, slot)` — `setDisplayObjective(slot, null)`.
///
/// # Errors
/// [`ScoreboardError::DisplaySlotAlreadyEmpty`].
pub fn clear_display_slot(
    s: &mut ServerScoreboard,
    slot: &str,
) -> Result<Outcome, ScoreboardError> {
    let old = s
        .board
        .display_objective(slot)
        .map(str::to_owned)
        .ok_or(ScoreboardError::DisplaySlotAlreadyEmpty)?;
    s.board
        .set_display_objective(slot, None)
        .map_err(|_| ScoreboardError::Unreadable("slot"))?;
    let mut out = Vec::new();
    // The `objective == null` case skips the second block entirely, so the
    // only packets are the displaced objective's.
    if s.display_slot_count(&old) > 0 {
        let mut b = Vec::new();
        if packets::set_display_objective(&mut b, slot, "").is_ok() {
            out.push((ids::play::cb::SET_DISPLAY_OBJECTIVE, b));
        }
    } else if s.tracked.remove(&old) {
        if let Some(o) = s.board.objective(&old).cloned() {
            out.extend(s.stop_tracking(&o));
        }
    }
    Ok(Outcome {
        packets: out,
        reply: translatable(
            "commands.scoreboard.objectives.display.cleared",
            &[Arg::Text(slot.to_owned())],
        ),
        result: 0,
    })
}

/// `listObjectives(source)` — the one executor that sends
/// `sendSuccess(..., false)` and so never reaches the admin broadcast, and the
/// one that sends no packet.
#[must_use]
pub fn list_objectives(s: &ServerScoreboard) -> Outcome {
    let names = s.board.objective_names();
    let reply = if names.is_empty() {
        translatable("commands.scoreboard.objectives.list.empty", &[])
    } else {
        translatable(
            "commands.scoreboard.objectives.list.success",
            &[
                Arg::Int(i32::try_from(names.len()).unwrap_or(i32::MAX)),
                Arg::Component(format_list(&s.board, &names)),
            ],
        )
    };
    Outcome {
        packets: Vec::new(),
        reply,
        result: i32::try_from(names.len()).unwrap_or(i32::MAX),
    }
}

/// `ComponentUtils.formatList(objectives, Objective::getFormattedDisplayName)`.
///
/// The two-argument overload, whose separator is a gray `", "` and which
/// **returns the single element's own component** for a one-element
/// collection rather than wrapping it — the same rule `/list` obeys and the
/// same trap: a one-objective board is not a one-element list.
fn format_list(board: &Scoreboard, names: &[String]) -> Nbt {
    let element = |n: &String| {
        let display = board
            .objective(n)
            .and_then(|o| match &o.display_name {
                Nbt::String(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap_or_else(|| n.clone());
        formatted_display_name(n, &display)
    };
    match names {
        [] => crate::commands::compound([("text", Nbt::String(String::new()))]),
        [only] => element(only),
        many => {
            let mut extra = Vec::new();
            for (i, n) in many.iter().enumerate() {
                if i > 0 {
                    extra.push(crate::commands::compound([
                        ("color", Nbt::String("gray".into())),
                        ("text", Nbt::String(", ".into())),
                    ]));
                }
                extra.push(element(n));
            }
            crate::commands::compound([
                ("extra", Nbt::List(extra)),
                ("text", Nbt::String(String::new())),
            ])
        }
    }
}

/// Map a resolved `/scoreboard` command onto its executor.
///
/// Shared by [`crate::commands`] and [`crate::console`], for the reason
/// [`crate::bordercmd::run`] gives.
///
/// # Errors
/// [`ScoreboardError`].
pub fn run(
    resolved: &crate::commandtree::Resolved,
    board: &mut ServerScoreboard,
) -> Result<Outcome, ScoreboardError> {
    match resolved.path.as_slice() {
        ["scoreboard", "objectives", "list"] => Ok(list_objectives(board)),
        ["scoreboard", "objectives", "add", "objective", "criteria"] => {
            let name = resolved
                .arg("objective")
                .ok_or(ScoreboardError::Unreadable("objective"))?;
            let criteria = resolved
                .arg("criteria")
                .ok_or(ScoreboardError::Unreadable("criteria"))?;
            add_objective(board, name, criteria)
        }
        ["scoreboard", "objectives", "remove", "objective"] => {
            let name = resolved
                .arg("objective")
                .ok_or(ScoreboardError::Unreadable("objective"))?;
            remove_objective(board, name)
        }
        ["scoreboard", "objectives", "setdisplay", "slot"] => {
            let slot = resolved
                .arg("slot")
                .ok_or(ScoreboardError::Unreadable("slot"))?;
            clear_display_slot(board, slot)
        }
        ["scoreboard", "objectives", "setdisplay", "slot", "objective"] => {
            let slot = resolved
                .arg("slot")
                .ok_or(ScoreboardError::Unreadable("slot"))?;
            let objective = resolved
                .arg("objective")
                .ok_or(ScoreboardError::Unreadable("objective"))?;
            set_display_slot(board, slot, objective)
        }
        other => Err(ScoreboardError::NoSuchPath(format!("{other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_an_objective_twice_is_refused() {
        let mut b = ServerScoreboard::default();
        let o = add_objective(&mut b, "kills", "dummy").expect("added");
        assert!(o.packets.is_empty(), "onObjectiveAdded broadcasts nothing");
        assert_eq!(
            add_objective(&mut b, "kills", "dummy"),
            Err(ScoreboardError::ObjectiveAlreadyExists)
        );
    }

    /// The finding the module header exists for: `add` puts NOTHING on the
    /// wire, and the objective's `set_objective` arrives with the
    /// `setdisplay` that first tracks it.
    #[test]
    fn adding_is_silent_and_setdisplay_carries_the_objective() {
        let mut b = ServerScoreboard::default();
        let added = add_objective(&mut b, "kills", "dummy").expect("added");
        assert!(
            added.packets.is_empty(),
            "onObjectiveAdded broadcasts nothing"
        );
        let shown = set_display_slot(&mut b, "sidebar", "kills").expect("shown");
        assert_eq!(
            shown.packets.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![
                ids::play::cb::SET_OBJECTIVE,
                ids::play::cb::SET_DISPLAY_OBJECTIVE
            ],
            "startTrackingObjective sends the objective FIRST, then the slot"
        );
        // A second slot for the same objective is one packet, not two: it is
        // already tracked.
        let again = set_display_slot(&mut b, "list", "kills").expect("shown");
        assert_eq!(
            again.packets.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![ids::play::cb::SET_DISPLAY_OBJECTIVE]
        );
    }

    /// Removing an objective that was never displayed sends nothing, because
    /// it was never tracked; removing a tracked one sends one packet, because
    /// `Scoreboard.removeObjective` cleared the slots first.
    #[test]
    fn removal_depends_on_whether_it_was_ever_tracked() {
        let mut b = ServerScoreboard::default();
        add_objective(&mut b, "kills", "dummy").expect("added");
        add_objective(&mut b, "deaths", "deathCount").expect("added");
        set_display_slot(&mut b, "sidebar", "kills").expect("shown");

        let untracked = remove_objective(&mut b, "deaths").expect("removed");
        assert!(
            untracked.packets.is_empty(),
            "never displayed, so never tracked, so nothing to say"
        );
        let tracked = remove_objective(&mut b, "kills").expect("removed");
        assert_eq!(
            tracked
                .packets
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![ids::play::cb::SET_OBJECTIVE],
            "the slot was already cleared by Scoreboard.removeObjective"
        );
        assert_eq!(tracked.result, 0, "getObjectives().size() after removal");
    }

    #[test]
    fn a_slot_already_showing_that_objective_is_refused() {
        let mut b = ServerScoreboard::default();
        add_objective(&mut b, "kills", "dummy").expect("added");
        add_objective(&mut b, "deaths", "deathCount").expect("added");
        set_display_slot(&mut b, "sidebar", "kills").expect("shown");
        assert_eq!(
            set_display_slot(&mut b, "sidebar", "kills"),
            Err(ScoreboardError::DisplaySlotAlreadySet)
        );
        // A DIFFERENT objective in the same slot is not refused — the test is
        // identity against what the slot holds, not "is the slot occupied".
        set_display_slot(&mut b, "sidebar", "deaths").expect("replaced");
    }

    #[test]
    fn clearing_an_empty_slot_is_refused() {
        let mut b = ServerScoreboard::default();
        assert_eq!(
            clear_display_slot(&mut b, "sidebar"),
            Err(ScoreboardError::DisplaySlotAlreadyEmpty)
        );
        add_objective(&mut b, "kills", "dummy").expect("added");
        set_display_slot(&mut b, "sidebar", "kills").expect("shown");
        let o = clear_display_slot(&mut b, "sidebar").expect("cleared");
        assert_eq!(o.packets.len(), 1);
    }

    /// `health` is the only criterion whose default render type is `hearts`,
    /// and a wrong render type is one byte in `set_objective` and nothing
    /// else — invisible to every check that does not read that packet.
    #[test]
    fn only_health_defaults_to_hearts() {
        assert_eq!(default_render_type("health"), "hearts");
        for c in ["dummy", "trigger", "deathCount", "food", "air", "level"] {
            assert_eq!(default_render_type(c), "integer", "{c}");
        }
    }

    #[test]
    fn an_empty_board_lists_the_empty_reply() {
        let b = ServerScoreboard::default();
        let o = list_objectives(&b);
        assert!(o.packets.is_empty());
        assert_eq!(o.result, 0);
    }
}
