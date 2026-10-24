//! Command executors, shared by the two sources a command can come from.
//!
//! [`crate::commands`] owns the tree and the replies that were there first
//! (`/list`, `/seed`, `/gamerule`, `/version`, `/say`, `/me`, `/msg`, `/help`,
//! `/worldborder`, `/scoreboard`). This module owns every root ported after
//! them, and the one thing those earlier roots never had: a **source**.
//!
//! # Why a source, and what it is in vanilla
//!
//! An executor in vanilla never writes to a socket. It calls
//! `CommandSourceStack.sendSuccess(supplier, broadcastToAdmins)`,
//! `sendFailure(component)` or `sendSystemMessage(component)`, and the stack
//! decides where that goes. The two stacks this server builds are:
//!
//! * a **player's** — `ServerPlayer.createCommandSourceStack()`, whose
//!   `CommandSource` is the player: `acceptsSuccess` is the
//!   `send_command_feedback` game rule, `acceptsFailure` is `true`, and
//!   `shouldInformAdmins` is `true`;
//! * the **console's** — `MinecraftServer.createCommandSourceStack()`, whose
//!   `CommandSource` is the server: everything it is sent goes to the log.
//!   Its position is the level's respawn point and it has no entity, which is
//!   why `@s` from the console finds nobody.
//!
//! So the same executor answers a typed `/difficulty hard` with a
//! `system_chat` and a console `difficulty hard` with a log line, and the
//! packets it causes — `change_difficulty` to every player — are identical.
//! That symmetry is what makes the console a fair route into the corpus for
//! a level-2 command: the corpus's players are not operators
//! (`tools/oracle-runner`'s `record::Console` header), so the console is the
//! only source that can run these at all, and what a scenario can grade is
//! the state change and whatever the executor sends to someone *other* than
//! its source.
//!
//! # `sendSuccess`, read off `CommandSourceStack` offsets 0..102
//!
//! ```text
//!   ok    = source.acceptsSuccess() && !silent
//!   admin = broadcastToAdmins && source.shouldInformAdmins() && !silent
//!   if !ok && !admin: return            // the supplier is never called
//!   c = supplier.get()
//!   if ok:    source.sendSystemMessage(c)
//!   if admin: broadcastToAdmins(c)
//! ```
//!
//! `broadcastToAdmins` sends `chat.type.admin` (the source's display name and
//! the component, gray italic) to every **op** that is not the source, under
//! `send_command_feedback`; and logs it under `log_admin_commands` unless the
//! source is the server. With no `ops.json` there are no ops, so on the
//! corpus it sends nothing — but it is ported rather than assumed away,
//! because a server with an op list is the ordinary case outside the corpus.
//!
//! # `sendFailure`
//!
//! `source.acceptsFailure() && !silent`, then
//! `Component.empty().append(c).withStyle(RED)` — the same wrapper
//! [`crate::commands`]' parse failures use. An executor raises a
//! `CommandSyntaxException`; `ExecutionCommandSource.handleError` turns its
//! raw message into that one red line. Only one line: the second, context,
//! line belongs to `finishParsing` and a command that parsed has none.

use crate::commands::{compound, send_system_chat, translatable, Arg};
use crate::commandtree::{Level, Resolved};
use crate::players::{Connection, Players};
use rustmc_proto::{ids, wire};
use rustmc_world::nbt::Nbt;
use std::io;
use std::sync::Arc;

/// Where a command's replies go — the stack's `CommandSource`.
///
/// Kept apart from the stack's ENTITY ([`Ctx::entity`]) because vanilla keeps
/// them apart: `execute as <player> run ...` from the console changes the
/// entity, the name and the position, and the replies still go to the
/// console's log (`CommandSourceStack.withEntity` copies `source` unchanged).
#[derive(Clone, Copy)]
pub enum Origin<'a> {
    /// The server's own console: full permission, logs what it is sent.
    Console,
    /// A player, at the permission level its op entry gives it.
    Player { conn: &'a Connection, level: Level },
}

/// Everything an executor may read about where it is running — the parts of
/// `CommandSourceStack` this server models.
#[derive(Clone)]
pub struct Ctx<'a> {
    pub origin: Origin<'a>,
    /// The player list. `None` only on a console line that arrives before the
    /// first connection was ever accepted (`crate::console`'s registration
    /// hook); an executor that needs it fails rather than pretending.
    pub players: Option<&'a Players>,
    /// `getEntity()`: the player a player source is, the entity an
    /// `execute as` put there, or nobody for the console.
    pub entity: Option<Arc<Connection>>,
    /// `getPosition()`: a player's feet, the level's respawn point for the
    /// console, or wherever an `execute at`/`positioned` moved it.
    pub pos: [f64; 3],
    /// `getRotation()`, `(xRot, yRot)` — pitch first, as `Vec2` holds it.
    pub rot: [f32; 2],
    /// Whether an `execute` modifier put this stack here — any modifier at
    /// all. The roots `crate::commands` answers predate sources and know only
    /// the plain player, so a chained one of theirs is not answered.
    pub chained: bool,
    /// Whether one of those modifiers FORKED (`as`, `at`, `positioned as`):
    /// `ContextChain` marks the run forked, and a forked run's exceptions are
    /// not sent (`handleError`'s `!forked`).
    pub forked: bool,
}

impl<'a> Ctx<'a> {
    /// The console's stack: `MinecraftServer.createCommandSourceStack()`.
    #[must_use]
    pub fn console(players: Option<&'a Players>) -> Self {
        let (x, y, z) = LEVEL_SPAWN;
        Self {
            origin: Origin::Console,
            players,
            entity: None,
            pos: [f64::from(x), f64::from(y), f64::from(z)],
            rot: [0.0, 0.0],
            chained: false,
            forked: false,
        }
    }

    /// A player's stack: `ServerPlayer.createCommandSourceStack()`.
    #[must_use]
    pub fn player(
        conn: &'a Connection,
        level: Level,
        players: &'a Players,
        pos: [f64; 3],
        rot: [f32; 2],
    ) -> Self {
        Self {
            origin: Origin::Player { conn, level },
            players: Some(players),
            entity: players
                .connections()
                .into_iter()
                .find(|c| c.id() == conn.id()),
            pos,
            rot,
            chained: false,
            forked: false,
        }
    }

    /// `CommandSourceStack.getDisplayName()`: the entity's display name, or
    /// the console's `Component.literal("Server")`.
    #[must_use]
    pub fn display_name(&self) -> Nbt {
        match &self.entity {
            Some(e) => crate::commands::display_name_component(e.name()),
            None => Nbt::String(CONSOLE_NAME.into()),
        }
    }

    /// The entity's name, for `@s`, `getPlayerOrException` and "is the source
    /// the target".
    #[must_use]
    pub fn source_name(&self) -> Option<&str> {
        self.entity.as_deref().map(Connection::name)
    }

    /// The name of the player whose `CommandSource` this is, which is what
    /// `broadcastToAdmins` compares against: not the entity.
    fn sink_name(&self) -> Option<&str> {
        match &self.origin {
            Origin::Console => None,
            Origin::Player { conn, .. } => Some(conn.name()),
        }
    }
}

/// `MinecraftServer.createCommandSourceStack()`'s text name and display name:
/// both are the literal `"Server"` (`COMMAND_SOURCE_NAME`).
pub const CONSOLE_NAME: &str = "Server";

/// One thing an executor said, in the order it said it.
#[derive(Debug, Clone, PartialEq)]
pub enum Feedback {
    /// `sendSuccess(() -> component, to_admins)`.
    Success { component: Nbt, to_admins: bool },
    /// `sendSystemMessage(component)` — to the source, unconditionally.
    System(Nbt),
}

/// A `CommandSyntaxException` raised by an executor: its raw message.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure(pub Nbt);

impl Failure {
    /// `SimpleCommandExceptionType`/`Dynamic*CommandExceptionType` with a
    /// translatable message, which is every executor exception this module
    /// raises.
    #[must_use]
    pub fn translatable(key: &str, args: &[Arg]) -> Self {
        Self(translatable(key, args))
    }

    /// Not a vanilla exception: this server cannot answer the command at all
    /// (an `@e` it has no entities for, an `@r` it has no draw for), so
    /// nothing is sent, and the console says why. The empty list is the
    /// marker because no component encodes as one.
    #[must_use]
    pub fn unanswered(why: &str) -> Self {
        eprintln!("{why} — not answered");
        Self(Nbt::List(Vec::new()))
    }

    /// Whether this is [`Self::unanswered`].
    #[must_use]
    pub fn is_unanswered(&self) -> bool {
        matches!(&self.0, Nbt::List(v) if v.is_empty())
    }
}

/// What running one command produced.
pub type Outcome = Result<Vec<Feedback>, Failure>;

/// Whether an executable path of the tree is one [`run`] answers.
///
/// The executor coverage test in `crate::commands` asks this for every
/// executable node, so a node registered in the tree with no executor here
/// fails a test rather than reaching a player as silence.
#[must_use]
pub fn owns(path: &[&str]) -> bool {
    matches!(
        path,
        ["difficulty"]
            | ["difficulty", _]
            | ["gamemode", "gamemode"]
            | ["gamemode", "gamemode", "target"]
            | ["defaultgamemode", "gamemode"]
            | ["fill", "from", "to", "block"]
            | [
                "fill",
                "from",
                "to",
                "block",
                "outline" | "hollow" | "destroy" | "strict" | "replace" | "keep"
            ]
            | ["playsound", "sound"]
            | ["playsound", "sound", _]
            | ["playsound", "sound", _, "targets"]
            | ["playsound", "sound", _, "targets", "pos"]
            | ["playsound", "sound", _, "targets", "pos", "volume"]
            | ["playsound", "sound", _, "targets", "pos", "volume", "pitch"]
            | [
                "playsound",
                "sound",
                _,
                "targets",
                "pos",
                "volume",
                "pitch",
                "minVolume"
            ]
            | ["stopsound", "targets"]
            | ["stopsound", "targets", _]
            | ["stopsound", "targets", _, "sound"]
            | ["clone", "begin", "end", "destination"]
            | [
                "clone",
                "begin",
                "end",
                "destination",
                "replace" | "masked" | "strict"
            ]
            | [
                "clone",
                "begin",
                "end",
                "destination",
                "replace" | "masked",
                "force" | "move" | "normal"
            ]
            | [
                "clone",
                "begin",
                "end",
                "destination",
                "strict",
                "replace" | "masked"
            ]
            | [
                "clone",
                "begin",
                "end",
                "destination",
                "strict",
                "replace" | "masked",
                "force" | "move" | "normal"
            ]
            | ["random", "value" | "roll", "range"]
            | ["setblock", "pos", "block"]
            | [
                "setblock",
                "pos",
                "block",
                "destroy" | "keep" | "replace" | "strict"
            ]
            | ["time", "set" | "add", "time"]
            | ["time", "query", "time" | "gametime"]
            | ["title", "targets", "clear" | "reset"]
            | [
                "title",
                "targets",
                "title" | "subtitle" | "actionbar",
                "title"
            ]
            | ["tellraw", "targets", "message"]
            | ["title", "targets", "times", "fadeIn", "stay", "fadeOut"]
            | ["teleport", "location" | "destination"]
            | ["teleport", "targets", "location" | "destination"]
            | ["teleport", "targets", "location", "rotation"]
            | ["rotate", "target", "rotation"]
            | ["swing"]
            | ["swing", "targets"]
            | ["swing", "targets", "mainhand" | "offhand"]
            | ["teammsg", "message"]
            | ["dialog", "clear", "targets"]
            | ["bossbar", "add", "id", "name"]
            | ["bossbar", "remove", "id"]
            | ["bossbar", "list"]
            | ["bossbar", "set", "id", "name", "name"]
            | ["bossbar", "set", "id", "color" | "style", _]
            | ["bossbar", "set", "id", "value", "value"]
            | ["bossbar", "set", "id", "max", "max"]
            | ["bossbar", "set", "id", "visible", "visible"]
            | ["bossbar", "set", "id", "players"]
            | ["bossbar", "set", "id", "players", "targets"]
            | [
                "bossbar",
                "get",
                "id",
                "value" | "max" | "visible" | "players"
            ]
            | ["particle", "name"]
            | ["particle", "name", "pos"]
            | ["particle", "name", "pos", "delta", "speed", "count"]
            | [
                "particle",
                "name",
                "pos",
                "delta",
                "speed",
                "count",
                "force" | "normal"
            ]
            | [
                "particle",
                "name",
                "pos",
                "delta",
                "speed",
                "count",
                "force" | "normal",
                "viewers"
            ]
            | ["weather", "clear" | "rain" | "thunder"]
            | ["weather", "clear" | "rain" | "thunder", "duration"]
            | ["experience", "add" | "set", "target", "amount"]
            | [
                "experience",
                "add" | "set",
                "target",
                "amount",
                "points" | "levels"
            ]
            | ["experience", "query", "target", "points" | "levels"]
    )
}

/// Whether `/say`, `/me` and `/msg` are this module's to answer: from the
/// console, or under an `execute` modifier. A plain player's are answered by
/// `crate::commands`, whose replies were gated against recorded bytes before
/// this module existed.
fn speaks_for_itself(ctx: &Ctx<'_>) -> bool {
    matches!(ctx.origin, Origin::Console) || ctx.chained
}

/// Run a resolved command against a source, and deliver what it said.
///
/// `None` means "not this module's" — the caller answers it. This is the one
/// entry point both sources use, because it is where `execute` lives: the
/// [`Resolved::chain`] a redirect left behind is applied here, modifier by
/// modifier, to a list of source stacks (a fork can make several, or none),
/// and the command then runs once per stack, each delivering to its own
/// `CommandSource`.
///
/// # Errors
/// A write failure to the source's own connection.
pub fn dispatch(resolved: &Resolved, base: &Ctx<'_>) -> Option<io::Result<()>> {
    let modifiers: Vec<_> = resolved
        .chain
        .iter()
        .filter(|(path, _)| path.first() == Some(&"execute"))
        .collect();
    if modifiers.is_empty() {
        let out = run(resolved, base)?;
        return Some(deliver(base, &out));
    }
    let mut stacks = vec![base.clone()];
    for (path, args) in modifiers {
        let mut next = Vec::new();
        for st in &stacks {
            match modify(st, path, args) {
                Ok(v) => next.extend(v),
                Err(f) => {
                    // A modifier that cannot be applied fails the whole
                    // command, and a modifier's own exception is reported
                    // like an executor's.
                    return Some(deliver(st, &Err(f)));
                }
            }
        }
        stacks = next;
    }
    for st in &stacks {
        let out = match run(resolved, st) {
            Some(out) => out,
            None => Err(Failure::unanswered(&format!(
                "{:?} under execute: a root answered only for a plain player",
                resolved.path
            ))),
        };
        if let Err(e) = deliver(st, &out) {
            return Some(Err(e));
        }
    }
    Some(Ok(()))
}

/// One `execute` redirect applied to one source stack, from
/// `ExecuteCommand.register`'s modifiers:
///
/// ```text
///   run                       redirect(root), no modifier
///   as <targets>              fork: one stack per entity, source.withEntity(e)
///   at <targets>              fork: withLevel/withPosition/withRotation of e
///   positioned <pos>          redirect: withPosition(pos.getPosition(source))
///                             .withAnchor(FEET)
///   positioned as <targets>   fork: withPosition(e.position())
/// ```
///
/// `as` and `at` read `EntityArgument.getOptionalEntities`, so no match is an
/// empty fork — the command runs zero times and says nothing — and not
/// `NO_ENTITIES_FOUND`.
fn modify<'a>(
    st: &Ctx<'a>,
    path: &[&str],
    args: &[(&'static str, String)],
) -> Result<Vec<Ctx<'a>>, Failure> {
    let arg = |n: &str| {
        args.iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default()
    };
    let chained = |mut c: Ctx<'a>, fork: bool| {
        c.chained = true;
        c.forked |= fork;
        c
    };
    match path {
        ["execute", "run"] => Ok(vec![chained(st.clone(), false)]),
        ["execute", "as", "targets"] => Ok(select_players(st, arg("targets"))?
            .into_iter()
            .map(|e| {
                let mut c = chained(st.clone(), true);
                c.entity = Some(e);
                c
            })
            .collect()),
        ["execute", "at", "targets"] | ["execute", "positioned", "as", "targets"] => {
            let rotate = path[1] == "at";
            Ok(select_players(st, arg("targets"))?
                .into_iter()
                .filter_map(|e| {
                    let (pos, y_rot, x_rot) = entity_pose(&e)?;
                    let mut c = chained(st.clone(), true);
                    c.pos = pos;
                    if rotate {
                        c.rot = [x_rot, y_rot];
                    }
                    Some(c)
                })
                .collect())
        }
        ["execute", "positioned", "pos"] => {
            let raw = arg("pos");
            let mut r = crate::cmdreader::Reader::new(raw);
            let Ok(Some(coords)) = crate::commandtree::vec3_world(&mut r, true) else {
                return Err(Failure::unanswered(&format!(
                    "execute positioned {raw:?}: local coordinates are not resolved here"
                )));
            };
            let mut c = chained(st.clone(), false);
            c.pos = [
                coords[0].get(st.pos[0]),
                coords[1].get(st.pos[1]),
                coords[2].get(st.pos[2]),
            ];
            Ok(vec![c])
        }
        other => Err(Failure::unanswered(&format!(
            "execute modifier {other:?} is not ported"
        ))),
    }
}

/// Run `resolved` if it is a path this module owns.
///
/// `None` means "not mine" — the caller falls through to the roots
/// [`crate::commands`] answers itself. A path this module claims and cannot
/// complete is a [`Failure`], never a `None`.
#[must_use]
pub fn run(resolved: &Resolved, ctx: &Ctx<'_>) -> Option<Outcome> {
    Some(match resolved.path.as_slice() {
        ["difficulty"] => Ok(query_difficulty()),
        ["difficulty", name] => set_difficulty(ctx, name),
        ["gamemode", "gamemode"] => {
            let mode = resolved.arg("gamemode").unwrap_or_default();
            // `lambda$register$0`: `getPlayerOrException()`, a singleton.
            match self_player(ctx) {
                Ok(me) => set_mode(ctx, &[me], mode),
                Err(e) => Err(e),
            }
        }
        ["gamemode", "gamemode", "target"] => {
            let mode = resolved.arg("gamemode").unwrap_or_default();
            match find_players(ctx, resolved.arg("target").unwrap_or_default()) {
                Ok(targets) => set_mode(ctx, &targets, mode),
                Err(e) => Err(e),
            }
        }
        // `/say`, `/me` and `/msg` from the CONSOLE. A player's are answered
        // by `crate::commands`, which predates this module; the console's
        // differ only in the sender — `Component.literal("Server")` — and in
        // the outgoing echo, which for a non-player source is
        // `source.sendSystemMessage`, i.e. the log.
        ["say", "message"] if speaks_for_itself(ctx) => console_broadcast(
            ctx,
            resolved.arg("message").unwrap_or_default(),
            "minecraft:say_command",
        ),
        ["me", "action"] if speaks_for_itself(ctx) => console_broadcast(
            ctx,
            resolved.arg("action").unwrap_or_default(),
            "minecraft:emote_command",
        ),
        ["msg", "targets", "message"] if speaks_for_itself(ctx) => console_msg(
            ctx,
            resolved.arg("targets").unwrap_or_default(),
            resolved.arg("message").unwrap_or_default(),
        ),
        ["experience", op @ ("add" | "set"), "target", "amount", ..] => {
            let kind = resolved.path.get(4).copied().unwrap_or("points");
            let amount: i32 = resolved
                .arg("amount")
                .and_then(|a| a.parse().ok())
                .unwrap_or(0);
            match find_players(ctx, resolved.arg("target").unwrap_or_default()) {
                Ok(t) if *op == "add" => add_experience(&t, amount, kind),
                Ok(t) => set_experience(&t, amount, kind),
                Err(e) => Err(e),
            }
        }
        ["experience", "query", "target", kind] => {
            match find_players(ctx, resolved.arg("target").unwrap_or_default()) {
                Ok(t) => Ok(query_experience(&t[0], kind)),
                Err(e) => Err(e),
            }
        }
        ["defaultgamemode", "gamemode"] => {
            default_game_mode(resolved.arg("gamemode").unwrap_or_default())
        }
        ["title", "targets", which @ ("title" | "subtitle" | "actionbar"), "title"] => {
            match (
                find_players(ctx, resolved.arg("targets").unwrap_or_default()),
                component_arg(resolved.arg("title").unwrap_or_default()),
            ) {
                (Err(e), _) | (_, Err(e)) => Err(e),
                (Ok(t), Ok(c)) => Ok(show_title(&t, which, &c)),
            }
        }
        ["tellraw", "targets", "message"] => {
            match (
                find_players(ctx, resolved.arg("targets").unwrap_or_default()),
                component_arg(resolved.arg("message").unwrap_or_default()),
            ) {
                (Err(e), _) | (_, Err(e)) => Err(e),
                (Ok(t), Ok(c)) => {
                    // `TellRawCommand`: `sendSystemMessage(resolved, false)` to
                    // each target, and no reply to the source at all.
                    for target in &t {
                        let _ = send_system_chat(target, &c);
                    }
                    Ok(Vec::new())
                }
            }
        }
        ["title", "targets", sub @ ("clear" | "reset")] => {
            match find_players(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => Ok(clear_titles(&t, *sub == "reset")),
                Err(e) => Err(e),
            }
        }
        ["title", "targets", "times", "fadeIn", "stay", "fadeOut"] => {
            let ticks = |n| {
                resolved
                    .arg(n)
                    .and_then(|v| {
                        let mut r = crate::cmdreader::Reader::new(v);
                        crate::commandtree::time_ticks(&mut r, 0).ok()
                    })
                    .unwrap_or(0)
            };
            match find_players(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => Ok(title_times(
                    &t,
                    ticks("fadeIn"),
                    ticks("stay"),
                    ticks("fadeOut"),
                )),
                Err(e) => Err(e),
            }
        }
        ["time", "set", "time"] => Ok(set_time(arg_ticks(resolved, "time", 0))),
        ["time", "add", "time"] => Ok(add_time(arg_ticks(resolved, "time", i32::MIN))),
        ["time", "query", "time"] => Ok(query_time()),
        ["time", "query", "gametime"] => Ok(query_game_time()),
        ["setblock", "pos", "block", ..] => setblock(
            ctx,
            resolved.arg("pos").unwrap_or_default(),
            resolved.arg("block").unwrap_or_default(),
            resolved.path.get(3).copied().unwrap_or("replace"),
        ),
        ["fill", "from", "to", "block", ..] => fill(
            ctx,
            resolved.arg("from").unwrap_or_default(),
            resolved.arg("to").unwrap_or_default(),
            resolved.arg("block").unwrap_or_default(),
            resolved.path.get(4).copied().unwrap_or("replace"),
        ),
        ["playsound", "sound", rest @ ..] => playsound(ctx, resolved, rest),
        ["stopsound", "targets", rest @ ..] => {
            match find_players(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => Ok(stop_sound(
                    &t,
                    rest.first().copied().filter(|s| *s != "*" && *s != "sound"),
                    resolved.arg("sound"),
                )),
                Err(e) => Err(e),
            }
        }
        ["clone", "begin", "end", "destination", rest @ ..] => clone(
            ctx,
            resolved.arg("begin").unwrap_or_default(),
            resolved.arg("end").unwrap_or_default(),
            resolved.arg("destination").unwrap_or_default(),
            rest,
        ),
        ["random", how @ ("value" | "roll"), "range"] => random_sample(
            ctx,
            resolved.arg("range").unwrap_or_default(),
            *how == "roll",
        ),
        ["weather", arm] => weather(arm, None),
        ["weather", arm, "duration"] => weather(arm, resolved.arg("duration")),
        ["teleport", "location"] => match self_entity(ctx) {
            Ok(me) => teleport_to_pos(
                ctx,
                &[me],
                resolved.arg("location").unwrap_or_default(),
                None,
            ),
            Err(e) => Err(e),
        },
        ["teleport", "destination"] => match self_entity(ctx) {
            Ok(me) => {
                teleport_to_entity(ctx, &[me], resolved.arg("destination").unwrap_or_default())
            }
            Err(e) => Err(e),
        },
        ["teleport", "targets", "location", ..] => {
            match find_entities(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => teleport_to_pos(
                    ctx,
                    &t,
                    resolved.arg("location").unwrap_or_default(),
                    resolved.arg("rotation"),
                ),
                Err(e) => Err(e),
            }
        }
        // `TeamMsgCommand.lambda$register$0`, offsets 0-28:
        // `getEntityOrException()`, then `entity.getTeam()` and
        // `ERROR_NOT_ON_TEAM` when it is null. This server registers no
        // `team` root, so no scoreboard team can exist and every entity's
        // `getTeam()` is null: the refusal is the whole of the command here,
        // not an approximation of it. The day `team` is ported this arm has
        // to read it.
        ["teammsg", "message"] => match self_entity(ctx) {
            Ok(_) => Err(teammsg_no_team()),
            Err(e) => Err(e),
        },
        ["bossbar", rest @ ..] => bossbar(ctx, resolved, rest),
        ["dialog", "clear", "targets"] => {
            match find_players(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => Ok(dialog_clear(&t)),
                Err(e) => Err(e),
            }
        }
        ["particle", "name", rest @ ..] => particle(ctx, resolved, rest),
        ["swing"] => match self_entity(ctx) {
            Ok(me) => swing(ctx, &[me], 0),
            Err(e) => Err(e),
        },
        ["swing", "targets", rest @ ..] => {
            let hand = u8::from(rest.first() == Some(&"offhand"));
            match find_entities(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => swing(ctx, &t, hand),
                Err(e) => Err(e),
            }
        }
        ["rotate", "target", "rotation"] => rotate(
            ctx,
            resolved.arg("target").unwrap_or_default(),
            resolved.arg("rotation").unwrap_or_default(),
        ),
        ["teleport", "targets", "destination"] => {
            match find_entities(ctx, resolved.arg("targets").unwrap_or_default()) {
                Ok(t) => {
                    teleport_to_entity(ctx, &t, resolved.arg("destination").unwrap_or_default())
                }
                Err(e) => Err(e),
            }
        }
        _ => return None,
    })
}

/// Send what an executor said to where its source sends it.
///
/// # Errors
/// On a write failure to the source's own connection. A failure to reach an
/// admin is not the source's error and is dropped, as `broadcast` drops it.
pub fn deliver(ctx: &Ctx<'_>, outcome: &Outcome) -> io::Result<()> {
    let rules = crate::gamerules::GameRules::shared();
    let feedback_rule = matches!(
        rules.get("send_command_feedback"),
        Some(crate::gamerules::GameRuleValue::Bool(true))
    );
    match outcome {
        Ok(lines) => {
            for line in lines {
                match line {
                    Feedback::Success {
                        component,
                        to_admins,
                    } => {
                        match &ctx.origin {
                            // `ServerPlayer.acceptsSuccess` is the game rule.
                            Origin::Player { conn, .. } => {
                                if feedback_rule {
                                    send_system_chat(conn, component)?;
                                }
                            }
                            // `MinecraftServer.acceptsSuccess` is true: it
                            // logs.
                            Origin::Console => println!("console: {}", plain(component)),
                        }
                        if *to_admins {
                            broadcast_to_admins(ctx, component, feedback_rule);
                        }
                    }
                    Feedback::System(component) => match &ctx.origin {
                        Origin::Player { conn, .. } => send_system_chat(conn, component)?,
                        Origin::Console => println!("console: {}", plain(component)),
                    },
                }
            }
        }
        Err(f) if f.is_unanswered() => {}
        // `ExecutionCommandSource.handleError`: `if (!forked) sendFailure(...)`
        // — a command run under an `execute as`/`at` fork fails in silence.
        Err(_) if ctx.forked => {}
        Err(Failure(raw)) => match &ctx.origin {
            Origin::Player { conn, .. } => send_system_chat(conn, &failure_line(raw))?,
            Origin::Console => eprintln!("console: {}", plain(raw)),
        },
    }
    Ok(())
}

/// `sendFailure`'s wrapper: `Component.empty().append(c).withStyle(RED)`.
#[must_use]
pub fn failure_line(raw: &Nbt) -> Nbt {
    compound([
        ("color", Nbt::String("red".into())),
        ("extra", Nbt::List(vec![raw.clone()])),
        ("text", Nbt::String(String::new())),
    ])
}

/// `CommandSourceStack.broadcastToAdmins`, offsets 0..143.
///
/// `chat.type.admin` with the source's display name and the component,
/// `withStyle(GRAY, ITALIC)`, to every op whose command source is not this
/// one — gated on `send_command_feedback`, which is read inside the method and
/// not by `sendSuccess`.
fn broadcast_to_admins(ctx: &Ctx<'_>, component: &Nbt, feedback_rule: bool) {
    if !feedback_rule {
        return;
    }
    let Some(players) = ctx.players else { return };
    let line = admin_line(&ctx.display_name(), component);
    for conn in players.connections() {
        if Some(conn.name()) == ctx.sink_name() || !crate::ops::is_op(conn.name()) {
            continue;
        }
        let _ = send_system_chat(&conn, &line);
    }
}

/// `Component.translatable("chat.type.admin", name, c).withStyle(GRAY, ITALIC)`.
#[must_use]
pub fn admin_line(name: &Nbt, component: &Nbt) -> Nbt {
    let mut c = translatable(
        "chat.type.admin",
        &[
            Arg::Component(name.clone()),
            Arg::Component(component.clone()),
        ],
    );
    if let Nbt::Compound(m) = &mut c {
        m.insert("color".into(), Nbt::String("gray".into()));
        m.insert("italic".into(), Nbt::Byte(1));
    }
    c
}

/// A component as one line of console text: its translation key and
/// arguments, or its literal text. Not a renderer — the console log is not on
/// any wire this project compares, and an English rendering would need the
/// language file.
fn plain(c: &Nbt) -> String {
    match c {
        Nbt::String(s) => s.clone(),
        Nbt::Compound(m) => {
            let key = match m.get("translate") {
                Some(Nbt::String(k)) => k.clone(),
                _ => match m.get("text") {
                    Some(Nbt::String(t)) => t.clone(),
                    _ => String::new(),
                },
            };
            let args: Vec<String> = match m.get("with") {
                Some(Nbt::List(items)) => items.iter().map(plain).collect(),
                _ => Vec::new(),
            };
            let extra: String = match m.get("extra") {
                Some(Nbt::List(items)) => items.iter().map(plain).collect(),
                _ => String::new(),
            };
            if args.is_empty() {
                format!("{key}{extra}")
            } else {
                format!("{key}[{}]{extra}", args.join(", "))
            }
        }
        Nbt::Int(v) => v.to_string(),
        other => format!("{other:?}"),
    }
}

/// A console line, if it names a root this module owns.
///
/// Resolved through the **same** tree a player's command is, at
/// `Level::Owners` — the console stack's permission set is
/// `LevelBasedPermissionSet.OWNER`. Returns whether the line was this
/// module's, so [`crate::console`] can fall through to the roots it still
/// dispatches by hand.
pub fn console_line(line: &str, players: Option<&Players>) -> bool {
    let text = line.trim_end_matches(['\r', '\n']);
    let Ok(resolved) = crate::commands::tree().resolve(text, Level::Owners) else {
        return false;
    };
    let ctx = Ctx::console(players);
    // The console's sink is stdout; nothing here can fail to write.
    dispatch(&resolved, &ctx).is_some()
}

// ===========================================================================
// Entity selectors, for player arguments
// ===========================================================================

/// `CommandSourceStack.ERROR_NOT_PLAYER` — `permissions.requires.player`.
#[must_use]
pub fn not_player() -> Failure {
    Failure::translatable("permissions.requires.player", &[])
}

/// `EntityArgument.NO_PLAYERS_FOUND` — `argument.entity.notfound.player`.
#[must_use]
pub fn no_players_found() -> Failure {
    Failure::translatable("argument.entity.notfound.player", &[])
}

/// `getPlayerOrException()`: the source's own player, or
/// [`not_player`] from the console.
fn self_player(ctx: &Ctx<'_>) -> Result<Arc<Connection>, Failure> {
    ctx.entity.clone().ok_or_else(not_player)
}

/// `EntityArgument.getPlayers` for an argument that has already parsed:
/// `EntitySelector.findPlayers`, then `NO_PLAYERS_FOUND` on an empty result.
///
/// # What is resolved, read off `EntitySelectorParser.parseSelector`
///
/// ```text
///   @p   maxResults 1, ORDER_NEAREST, players       no isAlive filter
///   @a   maxResults MAX, ORDER_ARBITRARY, players   no isAlive filter
///   @r   maxResults 1, ORDER_RANDOM, players        no isAlive filter
///   @s   the source entity                          no isAlive filter
///   @e   entities, isAlive                          refused at parse for players()
///   @n   nearest entity, isAlive                    refused at parse for players()
/// ```
///
/// A bare token is a name (`PlayerList.getPlayerByName`, which is
/// `equalsIgnoreCase` over the list in join order) or, when it reads as one,
/// a dashed UUID. An options block never reaches here: the parser refuses
/// `[` as unsupported, so the command never resolved.
///
/// **`@r` is refused here, not approximated.** `ORDER_RANDOM` is
/// `Util.shuffle(list, source.getLevel().random)` — a draw from the level's
/// own random — and picking any player without that draw would be a choice
/// vanilla does not make. It fails with the same exception an empty match
/// raises, and the console names why.
///
/// # Errors
/// [`no_players_found`] for no match, and for `@r`.
pub fn find_players(ctx: &Ctx<'_>, raw: &str) -> Result<Vec<Arc<Connection>>, Failure> {
    let found = select_players(ctx, raw)?;
    if found.is_empty() {
        return Err(no_players_found());
    }
    Ok(found)
}

/// `EntitySelector.findPlayers` with no empty-result check — what
/// `EntityArgument.getOptionalEntities` (`execute as`/`at`) reads.
///
/// # Errors
/// [`Failure::unanswered`] for a selector this server cannot resolve.
pub fn select_players(ctx: &Ctx<'_>, raw: &str) -> Result<Vec<Arc<Connection>>, Failure> {
    let all = ctx.players.map(Players::connections).unwrap_or_default();
    Ok(match raw.strip_prefix('@') {
        Some("a") => all,
        Some("s") => match ctx.source_name() {
            Some(me) => all.into_iter().filter(|c| c.name() == me).collect(),
            None => Vec::new(),
        },
        Some("p") => {
            let at = source_position(ctx);
            let mut best: Option<(f64, Arc<Connection>)> = None;
            for c in all {
                let Some(e) = crate::entities::Entities::shared().get(c.entity_id()) else {
                    continue;
                };
                let d = (e.x - at[0]).powi(2) + (e.y - at[1]).powi(2) + (e.z - at[2]).powi(2);
                // `ORDER_NEAREST` is a stable sort by distance, so the first
                // of two equidistant players in list order wins.
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, c));
                }
            }
            best.into_iter().map(|(_, c)| c).collect()
        }
        Some(other) => {
            return Err(Failure::unanswered(&format!(
                "selector @{other}: not resolved by this server"
            )));
        }
        None => {
            let by_uuid = parse_uuid(raw);
            all.into_iter()
                .find(|c| match by_uuid {
                    Some(u) => wire::offline_uuid(c.name()) == u,
                    None => c.name().eq_ignore_ascii_case(raw),
                })
                .into_iter()
                .collect()
        }
    })
}

/// `UUID.fromString` on the 36-character dashed form, which is the only form
/// `EntitySelectorParser.parseNameOrUUID` tries.
fn parse_uuid(raw: &str) -> Option<u128> {
    let parts: Vec<&str> = raw.split('-').collect();
    if parts.len() != 5 || raw.len() != 36 {
        return None;
    }
    let hex: String = parts.concat();
    u128::from_str_radix(&hex, 16).ok()
}

/// `CommandSourceStack.getPosition()`.
#[must_use]
pub fn source_position(ctx: &Ctx<'_>) -> [f64; 3] {
    ctx.pos
}

/// The level's respawn point as this server records it — the `spawn` that
/// `crate::status`'s autosave writes into `level.dat`. It is the console
/// source's position, which only `@p` and a relative coordinate typed at the
/// console ever read.
pub const LEVEL_SPAWN: (i32, i32, i32) = (8, 100, 8);

/// `GameType`'s four constants by `getName()`, in id order.
pub const GAME_TYPES: [&str; 4] = ["survival", "creative", "adventure", "spectator"];

/// The modes this server's mechanics can honour — see [`set_mode`].
pub const SERVED_GAME_TYPES: [&str; 2] = ["survival", "creative"];

/// `lambda$setMode$0` of `DefaultGameModeCommands`:
/// `commands.defaultgamemode.success` with `GameType.getLongDisplayName()`,
/// which is the same `gameMode.<name>` component `/gamemode` uses.
#[must_use]
pub fn default_game_mode_success(mode: &str) -> Nbt {
    translatable(
        "commands.defaultgamemode.success",
        &[Arg::Component(game_mode_name(mode))],
    )
}

/// `DefaultGameModeCommands.setMode`: `server.setDefaultGameType(type)`, then
/// `enforceGameTypeForPlayers(getForcedGameType())` — and a dedicated server
/// with `force-gamemode=false` has no forced type, so no online player
/// changes; only players who join later start in the new mode. Then
/// `sendSuccess(success, true)`. Same two-mode limit as [`set_mode`].
fn default_game_mode(mode: &str) -> Outcome {
    if crate::status::set_game_type(mode).is_err() {
        eprintln!("defaultgamemode {mode}: not a mode this server serves — not answered");
        return Ok(Vec::new());
    }
    Ok(vec![Feedback::Success {
        component: default_game_mode_success(mode),
        to_admins: true,
    }])
}

/// The `Component.translatable("gameMode." + type.getName())` that
/// `logGamemodeChange` builds once and passes to every message.
#[must_use]
pub fn game_mode_name(mode: &str) -> Nbt {
    translatable(&format!("gameMode.{mode}"), &[])
}

/// `lambda$logGamemodeChange$0` — `commands.gamemode.success.self`.
#[must_use]
pub fn gamemode_success_self(mode: &str) -> Nbt {
    translatable(
        "commands.gamemode.success.self",
        &[Arg::Component(game_mode_name(mode))],
    )
}

/// `lambda$logGamemodeChange$1` — `commands.gamemode.success.other`, the
/// target's display name then the mode.
#[must_use]
pub fn gamemode_success_other(target: &str, mode: &str) -> Nbt {
    translatable(
        "commands.gamemode.success.other",
        &[
            Arg::Component(crate::commands::display_name_component(target)),
            Arg::Component(game_mode_name(mode)),
        ],
    )
}

/// The line the TARGET is sent when somebody else changed its mode:
/// `gameMode.changed` with the mode.
#[must_use]
pub fn game_mode_changed(mode: &str) -> Nbt {
    translatable("gameMode.changed", &[Arg::Component(game_mode_name(mode))])
}

/// `GameModeCommand.setMode`: per target, `setGameMode(source, player, type)`,
/// counting the players whose mode actually changed. A target already in the
/// mode is skipped without a word, and so is a command that changed nobody —
/// vanilla raises nothing for it and returns 0.
///
/// `isSingleplayerOwner` is false for every player of a dedicated server, so
/// the `setDefaultGameType` arm never runs here.
///
/// # Two of the four modes are not answered
///
/// The mechanics know two modes: `Session::creative` is true for creative and
/// false otherwise. Adventure would play as survival (no `can_break`/
/// `can_place_on` checks exist here) and spectator would be a player every
/// interaction still serves — the ability model `crate::status::set_game_type`
/// already names as the reason it accepts only these two. So `adventure` and
/// `spectator` are **not answered**: no packet, no reply, and a console line
/// saying why. Sending vanilla's packets for them would tell a client it is a
/// spectator on a server that treats it as a survival player, which is the
/// partial implementation that reads complete from the call site.
fn set_mode(ctx: &Ctx<'_>, targets: &[Arc<Connection>], mode: &str) -> Outcome {
    let Some(id) = GAME_TYPES.iter().position(|m| *m == mode) else {
        return Ok(Vec::new());
    };
    if !SERVED_GAME_TYPES.contains(&mode) {
        eprintln!(
            "gamemode {mode}: this server has no {mode} ability model (see \
             `crate::status::set_game_type`) — not answered rather than half-applied"
        );
        return Ok(Vec::new());
    }
    let id = u8::try_from(id).unwrap_or(0);
    let mut out = Vec::new();
    for t in targets {
        if !change_game_mode(ctx, t, id) {
            continue;
        }
        // `logGamemodeChange`.
        if ctx.source_name() == Some(t.name()) {
            out.push(Feedback::Success {
                component: gamemode_success_self(mode),
                to_admins: true,
            });
        } else {
            if matches!(
                crate::gamerules::GameRules::shared().get("send_command_feedback"),
                Some(crate::gamerules::GameRuleValue::Bool(true))
            ) {
                // `player.sendSystemMessage` — to the target, unconditionally
                // once the rule allows it.
                let _ = send_system_chat(t, &game_mode_changed(mode));
            }
            out.push(Feedback::Success {
                component: gamemode_success_other(t.name(), mode),
                to_admins: true,
            });
        }
    }
    Ok(out)
}

/// `ServerPlayer.setGameMode(type)` and the `changeGameModeForPlayer` it
/// opens with, in their wire order:
///
/// ```text
///   changeGameModeForPlayer
///     same mode                         -> false, nothing sent
///     setGameModeForPlayer               (GameType.updatePlayerAbilities)
///     player.onUpdateAbilities()        -> player_abilities        (target)
///     broadcastAll(PlayerInfoUpdate(UPDATE_GAME_MODE))
///                                       -> player_info_update      (everyone)
///   game_event CHANGE_GAME_MODE, id     -> game_event 3            (target)
///   setCamera(this)                        nothing: it is its own camera
///   onUpdateAbilities()                 -> player_abilities        (target)
///   updateEffectVisibility()               nothing: no effect changed
/// ```
///
/// The abilities are a pure function of the mode here: this server tracks no
/// flight, so `abilities.flying` is false outside spectator, which is what
/// `changeGameModeForPlayer`'s own `flying && !SPECTATOR && isInRangeOfGround`
/// test leaves it as for a player standing on the ground.
fn change_game_mode(ctx: &Ctx<'_>, target: &Connection, id: u8) -> bool {
    if target.game_type() == id {
        return false;
    }
    target.set_game_type(id);
    let abilities = abilities_body(id);
    let _ = target.send(ids::play::cb::PLAYER_ABILITIES, &abilities);
    if let Some(players) = ctx.players {
        players.broadcast(
            ids::play::cb::PLAYER_INFO_UPDATE,
            &player_info_game_mode_body(target.name(), id),
        );
    }
    let mut ev = Vec::new();
    ev.push(GAME_EVENT_CHANGE_GAME_MODE);
    ev.extend_from_slice(&f32::from(id).to_be_bytes());
    let _ = target.send(ids::play::cb::GAME_EVENT, &ev);
    let _ = target.send(ids::play::cb::PLAYER_ABILITIES, &abilities);
    true
}

/// `ClientboundGameEventPacket.CHANGE_GAME_MODE` — `new Type(3)`.
pub const GAME_EVENT_CHANGE_GAME_MODE: u8 = 3;

/// `ClientboundPlayerAbilitiesPacket` for a player in mode `id`, as
/// `GameType.updatePlayerAbilities` leaves `Abilities`:
///
/// ```text
///   creative   mayfly instabuild invulnerable           flags 1|4|8 = 13
///   spectator  mayfly flying invulnerable               flags 1|2|4 = 7
///   otherwise  none of them                             flags 0
/// ```
///
/// then `flyingSpeed` 0.05f and `walkingSpeed` 0.1f, the `Abilities` field
/// initialisers, which nothing on this server changes. Flag bits from
/// `write`: invulnerable 1, flying 2, canFly 4, instabuild 8.
#[must_use]
pub fn abilities_body(id: u8) -> Vec<u8> {
    let flags: u8 = match id {
        1 => 1 | 4 | 8,
        3 => 1 | 2 | 4,
        _ => 0,
    };
    let mut b = vec![flags];
    b.extend_from_slice(&0.05f32.to_be_bytes());
    b.extend_from_slice(&0.1f32.to_be_bytes());
    b
}

/// `ClientboundPlayerInfoUpdatePacket(UPDATE_GAME_MODE, player)`.
///
/// `writeEnumSet` is a fixed bitset over `Action`'s eight constants — one
/// byte — with `UPDATE_GAME_MODE`'s ordinal, 2, set; then one entry: the
/// profile id as two longs and that action's writer, a varint `GameType` id.
#[must_use]
pub fn player_info_game_mode_body(name: &str, id: u8) -> Vec<u8> {
    let mut b = vec![1u8 << 2];
    let _ = rustmc_proto::varint::write_varint(&mut b, 1);
    b.extend_from_slice(&wire::offline_uuid(name).to_be_bytes());
    let _ = rustmc_proto::varint::write_varint(&mut b, i32::from(id));
    b
}

// ===========================================================================
// /say, /me, /msg from the console
// ===========================================================================

/// The message argument's component, or `None` where this server cannot
/// build it.
///
/// `MessageArgument.resolveChatMessage` on a source with no signing context
/// is `PlayerChatMessage.system(text)` with the resolved component as its
/// unsigned content, and for a message with no selector parts that component
/// is `Component.literal(text)` — a bare `TAG_String`. The console holds
/// `COMMANDS_ENTITY_SELECTORS`, so a message WITH selector parts would be
/// resolved into names, which this server does not do: it is not answered.
fn console_message(text: &str) -> Option<Nbt> {
    let mut r = crate::cmdreader::Reader::new(text);
    match crate::commandtree::message(&mut r) {
        Ok(0) => Some(Nbt::String(text.to_owned())),
        Ok(n) => {
            eprintln!("console message {text:?}: {n} selector(s) to resolve — not answered");
            None
        }
        Err(e) => {
            eprintln!("console message {text:?} reparsed as {e:?} — not answered");
            None
        }
    }
}

/// `SayCommand`/`EmoteCommands` with the console as source:
/// `PlayerList.broadcastChatMessage(message, source, ChatType.bind(type,
/// source))`, which with no player behind the source reaches every player
/// as a `disguised_chat` whose sender is the source's display name.
fn console_broadcast(ctx: &Ctx<'_>, text: &str, chat_type: &str) -> Outcome {
    let (Some(component), Some(id)) = (console_message(text), crate::chat::chat_type_id(chat_type))
    else {
        return Ok(Vec::new());
    };
    if let Some(players) = ctx.players {
        crate::chat::broadcast_disguised(players, &component, id, &ctx.display_name());
    }
    Ok(Vec::new())
}

/// `MsgCommand.sendMessage` with the console as source. Each target is sent
/// `msg_command_incoming` with the console as sender; the
/// `msg_command_outgoing` echo goes through `CommandSourceStack.
/// sendChatMessage`, which for a source with no player is
/// `sendSystemMessage(bound.decorate(content))` — the log.
fn console_msg(ctx: &Ctx<'_>, targets: &str, text: &str) -> Outcome {
    let targets = find_players(ctx, targets)?;
    let (Some(component), Some(incoming)) = (
        console_message(text),
        crate::chat::chat_type_id("minecraft:msg_command_incoming"),
    ) else {
        return Ok(Vec::new());
    };
    let outgoing = crate::chat::chat_type_id("minecraft:msg_command_outgoing");
    let sender = ctx.display_name();
    for t in &targets {
        // `source.sendChatMessage(outgoing, ...)`: `getPlayer()` is the
        // stack's ENTITY when it is a player — so under `execute as` the echo
        // goes to that player — and the log otherwise.
        if let (Some(me), Some(out)) = (&ctx.entity, outgoing) {
            let target_name = crate::commands::display_name_component(t.name());
            crate::chat::send_disguised(me, &component, out, &sender, Some(&target_name));
        }
        crate::chat::send_disguised(t, &component, incoming, &sender, None);
    }
    Ok(Vec::new())
}

// ===========================================================================
// /experience and /xp — `ExperienceCommand`
// ===========================================================================

/// `commands.experience.<add|set>.<type>.success.single` — the amount, then
/// the player's display name.
#[must_use]
pub fn experience_single(op: &str, kind: &str, amount: i32, target: &str) -> Nbt {
    translatable(
        &format!("commands.experience.{op}.{kind}.success.single"),
        &[
            Arg::Int(amount),
            Arg::Component(crate::commands::display_name_component(target)),
        ],
    )
}

/// `commands.experience.<add|set>.<type>.success.multiple` — the amount, then
/// the count.
#[must_use]
pub fn experience_multiple(op: &str, kind: &str, amount: i32, count: i32) -> Nbt {
    translatable(
        &format!("commands.experience.{op}.{kind}.success.multiple"),
        &[Arg::Int(amount), Arg::Int(count)],
    )
}

/// `ERROR_SET_POINTS_INVALID` — `commands.experience.set.points.invalid`.
#[must_use]
pub fn set_points_invalid() -> Failure {
    Failure::translatable("commands.experience.set.points.invalid", &[])
}

fn experience_reply(
    op: &str,
    kind: &str,
    amount: i32,
    targets: &[Arc<Connection>],
) -> Vec<Feedback> {
    let component = if let [only] = targets {
        experience_single(op, kind, amount, only.name())
    } else {
        experience_multiple(
            op,
            kind,
            amount,
            i32::try_from(targets.len()).unwrap_or(i32::MAX),
        )
    };
    vec![Feedback::Success {
        component,
        to_admins: true,
    }]
}

/// `ExperienceCommand.addExperience`: `Type.add` per target —
/// `giveExperiencePoints` or `giveExperienceLevels` — then one reply.
fn add_experience(targets: &[Arc<Connection>], amount: i32, kind: &str) -> Outcome {
    for t in targets {
        let ups = t.with_experience(|e| {
            if kind == "levels" {
                e.give_levels(amount).into_iter().collect()
            } else {
                e.give_points(amount)
            }
        });
        // `giveExperienceLevels`' sound, played where the console's own `xp`
        // plays its: see `crate::experience::queue_levelup`.
        for up in ups {
            crate::experience::queue_levelup(t.entity_id(), up);
        }
    }
    Ok(experience_reply("add", kind, amount, targets))
}

/// `ExperienceCommand.setExperience`: `Type.set` per target, counting the
/// ones it accepted. `POINTS` refuses an amount at or above the target's
/// `getXpNeededForNextLevel()` without changing it; if NO target accepted,
/// `ERROR_SET_POINTS_INVALID`. `LEVELS` always accepts.
fn set_experience(targets: &[Arc<Connection>], amount: i32, kind: &str) -> Outcome {
    let mut accepted = 0;
    for t in targets {
        let ok = t.with_experience(|e| {
            if kind == "levels" {
                e.set_levels(amount);
                true
            } else if amount >= e.needed() {
                false
            } else {
                e.set_points(amount);
                true
            }
        });
        if ok {
            accepted += 1;
        }
    }
    if accepted == 0 {
        return Err(set_points_invalid());
    }
    Ok(experience_reply("set", kind, amount, targets))
}

/// `commands.experience.query.<type>` — the display name, then the value.
#[must_use]
pub fn experience_query(target: &str, kind: &str, value: i32) -> Nbt {
    translatable(
        &format!("commands.experience.query.{kind}"),
        &[
            Arg::Component(crate::commands::display_name_component(target)),
            Arg::Int(value),
        ],
    )
}

/// `ExperienceCommand.queryExperience`: `sendSuccess(query, false)`.
fn query_experience(target: &Connection, kind: &str) -> Vec<Feedback> {
    let value = target.with_experience(|e| {
        if kind == "levels" {
            e.level
        } else {
            e.points()
        }
    });
    vec![Feedback::Success {
        component: experience_query(target.name(), kind, value),
        to_admins: false,
    }]
}

// ===========================================================================
// /title — `TitleCommand`, the subset with no component argument
// ===========================================================================

/// `commands.title.<what>.single` with the target's display name, or
/// `.multiple` with the count — the shape all three of `clear`, `reset` and
/// `times` reply in.
#[must_use]
pub fn title_reply(what: &str, targets: &[&str]) -> Nbt {
    if let [only] = targets {
        translatable(
            &format!("commands.title.{what}.single"),
            &[Arg::Component(crate::commands::display_name_component(
                only,
            ))],
        )
    } else {
        translatable(
            &format!("commands.title.{what}.multiple"),
            &[Arg::Int(i32::try_from(targets.len()).unwrap_or(i32::MAX))],
        )
    }
}

fn names(targets: &[Arc<Connection>]) -> Vec<&str> {
    targets.iter().map(|c| c.name()).collect()
}

/// A `minecraft:component` argument re-read from its token.
fn component_arg(raw: &str) -> Result<Nbt, Failure> {
    let mut r = crate::cmdreader::Reader::new(raw);
    crate::componentarg::parse(&mut r)
        .map_err(|_| Failure::unanswered(&format!("component {raw:?} did not re-parse")))
}

/// `TitleCommand.showTitle(source, targets, component, type, factory)`: per
/// target, `ComponentUtils.resolve` (a no-op on plain text) and one
/// `set_title_text` / `set_subtitle_text` / `set_action_bar_text`; then
/// `commands.title.show.<type>.single|multiple`.
fn show_title(targets: &[Arc<Connection>], which: &str, component: &Nbt) -> Vec<Feedback> {
    let id = match which {
        "title" => ids::play::cb::SET_TITLE_TEXT,
        "subtitle" => ids::play::cb::SET_SUBTITLE_TEXT,
        _ => ids::play::cb::SET_ACTION_BAR_TEXT,
    };
    let mut body = Vec::new();
    if crate::commands::write_network_nbt(&mut body, component).is_ok() {
        for t in targets {
            let _ = t.send(id, &body);
        }
    }
    vec![Feedback::Success {
        component: title_reply(&format!("show.{which}"), &names(targets)),
        to_admins: true,
    }]
}

/// `TitleCommand.clearTitle` / `resetTitle`: ONE
/// `ClientboundClearTitlesPacket(resetTimes)` built before the loop and sent
/// to every target, then `commands.title.cleared.*` / `commands.title.reset.*`.
fn clear_titles(targets: &[Arc<Connection>], reset: bool) -> Vec<Feedback> {
    for t in targets {
        let _ = t.send(ids::play::cb::CLEAR_TITLES, &[u8::from(reset)]);
    }
    vec![Feedback::Success {
        component: title_reply(if reset { "reset" } else { "cleared" }, &names(targets)),
        to_admins: true,
    }]
}

/// `TitleCommand.setTimes`: one `ClientboundSetTitlesAnimationPacket(fadeIn,
/// stay, fadeOut)` — three `writeInt`s — to every target, then
/// `commands.title.times.*`. Each argument is `TimeArgument.time()`, so a
/// unit suffix (`s`, `d`) is already ticks by the time it arrives here.
fn title_times(
    targets: &[Arc<Connection>],
    fade_in: i32,
    stay: i32,
    fade_out: i32,
) -> Vec<Feedback> {
    let mut b = Vec::with_capacity(12);
    for v in [fade_in, stay, fade_out] {
        b.extend_from_slice(&v.to_be_bytes());
    }
    for t in targets {
        let _ = t.send(ids::play::cb::SET_TITLES_ANIMATION, &b);
    }
    vec![Feedback::Success {
        component: title_reply("times", &names(targets)),
        to_admins: true,
    }]
}

// ===========================================================================
// /time — `TimeCommand`, the default-clock subset
// ===========================================================================

static CLOCK: std::sync::OnceLock<crate::tick::GameClock> = std::sync::OnceLock::new();

/// Give the executors the game clock, once. `crate::status::listen` does,
/// right after it makes it.
pub fn set_clock(clock: &crate::tick::GameClock) {
    let _ = CLOCK.set(clock.clone());
}

/// `Level.getGameTime()`, or 0 before a clock exists.
fn game_time() -> i64 {
    CLOCK.get().map_or(0, crate::tick::GameClock::ticks)
}

/// The overworld's default clock — `DimensionType.defaultClock()` — by
/// `Holder.getRegisteredName()`. This server's one level is the overworld.
pub const DEFAULT_CLOCK: &str = "minecraft:overworld";

/// A `minecraft:time` argument's ticks, re-read from its token.
fn arg_ticks(resolved: &Resolved, name: &str, min: i32) -> i32 {
    resolved
        .arg(name)
        .and_then(|v| {
            let mut r = crate::cmdreader::Reader::new(v);
            crate::commandtree::time_ticks(&mut r, min).ok()
        })
        .unwrap_or(0)
}

/// `lambda$setTotalTicks$0`: `commands.time.set.absolute` with the clock's
/// registered name and the ticks as an **int**.
#[must_use]
pub fn time_set_reply(ticks: i32) -> Nbt {
    translatable(
        "commands.time.set.absolute",
        &[Arg::Text(DEFAULT_CLOCK.into()), Arg::Int(ticks)],
    )
}

/// `lambda$addTime$0`: the same key with the new total as a **long**.
#[must_use]
pub fn time_add_reply(total: i64) -> Nbt {
    translatable(
        "commands.time.set.absolute",
        &[Arg::Text(DEFAULT_CLOCK.into()), Arg::Long(total)],
    )
}

/// `lambda$queryTime$0`: `commands.time.query.absolute`, name and a long.
#[must_use]
pub fn time_query_reply(total: i64) -> Nbt {
    translatable(
        "commands.time.query.absolute",
        &[Arg::Text(DEFAULT_CLOCK.into()), Arg::Long(total)],
    )
}

/// `lambda$queryGameTime$0`: `commands.time.query.gametime` with a long.
#[must_use]
pub fn time_query_game_time_reply(ticks: i64) -> Nbt {
    translatable("commands.time.query.gametime", &[Arg::Long(ticks)])
}

/// `TimeCommand.setTotalTicks`: `clockManager.setTotalTicks(clock, ticks)`,
/// whose `modifyClock` broadcasts the clocks, then `sendSuccess(..., true)`.
fn set_time(ticks: i32) -> Vec<Feedback> {
    crate::sky::set_total_ticks(i64::from(ticks), game_time());
    vec![Feedback::Success {
        component: time_set_reply(ticks),
        to_admins: true,
    }]
}

/// `TimeCommand.addTime`: `addTicks`, then the reply carries the clock's new
/// total, read back with `getTotalTicks`.
fn add_time(ticks: i32) -> Vec<Feedback> {
    let now = game_time();
    let total = crate::sky::day_time(now).wrapping_add(i64::from(ticks));
    crate::sky::set_total_ticks(total, now);
    vec![Feedback::Success {
        component: time_add_reply(total),
        to_admins: true,
    }]
}

/// `TimeCommand.queryTime`: `sendSuccess(query.absolute, false)`.
fn query_time() -> Vec<Feedback> {
    vec![Feedback::Success {
        component: time_query_reply(crate::sky::day_time(game_time())),
        to_admins: false,
    }]
}

/// `TimeCommand.queryGameTime`: `sendSuccess(query.gametime, false)`.
fn query_game_time() -> Vec<Feedback> {
    vec![Feedback::Success {
        component: time_query_game_time_reply(game_time()),
        to_admins: false,
    }]
}

// ===========================================================================
// /setblock — `SetBlockCommand`
// ===========================================================================

/// `commands.setblock.success` with the position's three ints.
#[must_use]
pub fn setblock_success(pos: (i32, i32, i32)) -> Nbt {
    translatable(
        "commands.setblock.success",
        &[Arg::Int(pos.0), Arg::Int(pos.1), Arg::Int(pos.2)],
    )
}

/// `SetBlockCommand.ERROR_FAILED` — `commands.setblock.failed`.
#[must_use]
pub fn setblock_failed() -> Failure {
    Failure::translatable("commands.setblock.failed", &[])
}

/// `BlockPosArgument.ERROR_NOT_LOADED` and `ERROR_OUT_OF_WORLD`.
#[must_use]
pub fn pos_unloaded() -> Failure {
    Failure::translatable("argument.pos.unloaded", &[])
}

/// See [`pos_unloaded`].
#[must_use]
pub fn pos_out_of_world() -> Failure {
    Failure::translatable("argument.pos.outofworld", &[])
}

/// `BlockPosArgument.getLoadedBlockPos`: `BlockPos.containing` of the
/// coordinates resolved against the source — `WorldCoordinate.parseInt`, so
/// no centre correction — then `hasChunkAt` (`argument.pos.unloaded`) and
/// `isInWorldBounds` (`argument.pos.outofworld`), in that order.
fn loaded_block_pos(
    ctx: &Ctx<'_>,
    world: &crate::tiles::Tiles,
    raw: &str,
) -> Result<(i32, i32, i32), Failure> {
    let mut r = crate::cmdreader::Reader::new(raw);
    let Ok(Some(c)) = crate::commandtree::vec3_world(&mut r, false) else {
        return Err(Failure::unanswered(&format!(
            "block position {raw:?}: local coordinates are not resolved here"
        )));
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`Mth.floor`, as `containing` does"
    )]
    let pos = (
        c[0].get(ctx.pos[0]).floor() as i32,
        c[1].get(ctx.pos[1]).floor() as i32,
        c[2].get(ctx.pos[2]).floor() as i32,
    );
    if world.tile(pos.0 >> 4, pos.2 >> 4).is_none() {
        return Err(pos_unloaded());
    }
    let min_y = crate::edits::Blocks::level_min_y(world).unwrap_or(-64);
    let top = crate::sky::active().map_or(320, crate::sky::SkyModel::top_y);
    let horizontal =
        (-30_000_000..30_000_000).contains(&pos.0) && (-30_000_000..30_000_000).contains(&pos.2);
    if !(horizontal && (min_y..top).contains(&pos.1)) {
        return Err(pos_out_of_world());
    }
    Ok(pos)
}

/// `Block.updateFromNeighbourShapes(state, level, pos)`: the state as each
/// neighbour would reshape it, asked in `UPDATE_SHAPE_ORDER`.
fn shaped_by_neighbours(
    world: &crate::tiles::Tiles,
    shapes: &crate::shapes::ShapeTable,
    pos: (i32, i32, i32),
    state: &str,
    effects: &mut crate::shapes::Effects,
) -> String {
    let mut state = state.to_string();
    for d in crate::shapes::UPDATE_SHAPE_ORDER {
        let np = d.relative(pos);
        let ns = crate::edits::Blocks::state_at(world, np);
        let nid = crate::edits::Blocks::state_id(world, &ns);
        if let Some(next) =
            crate::shapes::shape_after(world, shapes, pos, &state, d, np, &ns, nid, effects)
        {
            state = next;
        }
    }
    state
}

/// `SetBlockCommand.setBlock`.
///
/// ```text
///   keep:    BlockInWorld(level, pos).getState().isAir() or ERROR_FAILED
///   destroy: level.destroyBlock(pos, true); success = !input.isAir() || !now.isAir()
///   old = level.getBlockState(pos)
///   success && !input.place(level, pos, 2 | (strict ? 816 : 256)) -> ERROR_FAILED
///   !strict: level.updateNeighboursOnBlockSet(pos, old)
///   sendSuccess(commands.setblock.success, true)
/// ```
///
/// `BlockInput.place` is `updateFromNeighbourShapes` unless the flags carry
/// `UPDATE_KNOWN_SHAPE` (strict does), falling back to the input state if
/// that came out air, then `overwriteWithDefinedProperties`, then
/// `level.setBlock(pos, state, flags)` — this server's
/// [`crate::shapes::set_block`], the same write a player's placement goes
/// through. The written positions join the tick's changed-block set, which is
/// where the `block_update` comes from; the scheduled ticks join the level's
/// queue.
///
/// **`destroy` over a block that is not air is not answered**: it is
/// `Level.destroyBlock(pos, dropItems = true)`, whose drop half is a loot
/// roll this executor does not make. Over air it is exactly `replace`.
fn setblock(ctx: &Ctx<'_>, pos_raw: &str, block_raw: &str, mode: &str) -> Outcome {
    let Ok(world) =
        crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS)
    else {
        return Err(Failure::unanswered("setblock: no world"));
    };
    let (Some(table), Some(shapes)) = (
        crate::blockarg::shared(),
        crate::shapes::shared_with_classes(std::path::Path::new(".")),
    ) else {
        return Err(Failure::unanswered("setblock: no block or shape table"));
    };
    let pos = loaded_block_pos(ctx, world, pos_raw)?;
    let mut r = crate::cmdreader::Reader::new(block_raw);
    let Ok(input) = crate::blockarg::parse(&mut r, table) else {
        return Err(Failure::unanswered(&format!(
            "setblock: {block_raw:?} did not re-parse"
        )));
    };
    let now = crate::edits::Blocks::state_at(world, pos);
    if mode == "keep" && !crate::shapes::is_air(&now) {
        return Err(setblock_failed());
    }
    if mode == "destroy" && !crate::shapes::is_air(&now) {
        return Err(Failure::unanswered(
            "setblock destroy: Level.destroyBlock's drops are not ported",
        ));
    }
    let strict = mode == "strict";
    let flags = crate::shapes::UPDATE_CLIENTS | if strict { 816 } else { 256 };
    // `destroy` over air: `success = !input.isAir() || !level.getBlockState(pos).isAir()`.
    let success = mode != "destroy" || !crate::shapes::is_air(&input.state);
    let old = now;
    let mut effects = crate::shapes::Effects::new();
    if success {
        let shaped = if flags & crate::shapes::UPDATE_KNOWN_SHAPE != 0 {
            input.state.clone()
        } else {
            shaped_by_neighbours(world, shapes, pos, &input.state, &mut effects)
        };
        let shaped = if crate::shapes::is_air(&shaped) {
            input.state.clone()
        } else {
            shaped
        };
        let state = input.overwrite_defined(&shaped);
        if !crate::shapes::set_block(
            world,
            Some(shapes),
            pos,
            &state,
            flags,
            crate::shapes::RECURSION_LIMIT,
            &mut effects,
        ) {
            return Err(setblock_failed());
        }
    }
    if !strict {
        update_neighbours_on_block_set(world, shapes, pos, &old, &mut effects);
    }
    apply_effects(ctx, &effects);
    Ok(vec![Feedback::Success {
        component: setblock_success(pos),
        to_admins: true,
    }])
}

/// `commands.fill.success` with the count.
#[must_use]
pub fn fill_success(count: i32) -> Nbt {
    translatable("commands.fill.success", &[Arg::Int(count)])
}

/// `FillCommand.ERROR_FAILED` — `commands.fill.failed`.
#[must_use]
pub fn fill_failed() -> Failure {
    Failure::translatable("commands.fill.failed", &[])
}

/// `FillCommand.ERROR_AREA_TOO_LARGE` — `commands.fill.toobig` with the
/// limit as an `Integer` and the volume as a `Long`.
#[must_use]
pub fn fill_too_big(max: i32, volume: i64) -> Failure {
    Failure::translatable("commands.fill.toobig", &[Arg::Int(max), Arg::Long(volume)])
}

/// `FillCommand.fillBlocks`, for the modes this root registers.
///
/// ```text
///   volume = xspan * yspan * zspan (a long); > max_block_modifications
///                                            -> ERROR_AREA_TOO_LARGE
///   for pos in BlockPos.betweenClosed(min, max)   x fastest, then y, then z
///     keep: !BlockInWorld(pos).getState().isAir() -> skip, uncounted
///     old = getBlockState(pos)
///     changed = mode.affector.affect(level, pos)    (DESTROY's destroyBlock)
///     input = mode.filter(box, pos, block)          OUTLINE: edges or null
///                                                   HOLLOW: edges or air
///     input == null or !input.place(level, pos, 2 | strict ? 816 : 256)
///       -> count it if `changed`, next
///     !strict -> remember (pos, old)
///     count++
///   updateNeighboursOnBlockSet for every remembered (pos, old), in order
///   count == 0 -> ERROR_FAILED; else sendSuccess(commands.fill.success, true)
/// ```
///
/// Every neighbour update runs after every write, which is the difference
/// between a fill and N setblocks, and is why the remembered list exists.
/// `destroy` over a non-air block is not answered, for `/setblock`'s reason.
fn fill(ctx: &Ctx<'_>, from_raw: &str, to_raw: &str, block_raw: &str, mode: &str) -> Outcome {
    let Ok(world) =
        crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS)
    else {
        return Err(Failure::unanswered("fill: no world"));
    };
    let (Some(table), Some(shapes)) = (
        crate::blockarg::shared(),
        crate::shapes::shared_with_classes(std::path::Path::new(".")),
    ) else {
        return Err(Failure::unanswered("fill: no block or shape table"));
    };
    let a = loaded_block_pos(ctx, world, from_raw)?;
    let b = loaded_block_pos(ctx, world, to_raw)?;
    let mut r = crate::cmdreader::Reader::new(block_raw);
    let Ok(input) = crate::blockarg::parse(&mut r, table) else {
        return Err(Failure::unanswered(&format!(
            "fill: {block_raw:?} did not re-parse"
        )));
    };
    let min = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
    let max = (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2));
    let span = |lo: i32, hi: i32| i64::from(hi) - i64::from(lo) + 1;
    let volume = span(min.0, max.0) * span(min.1, max.1) * span(min.2, max.2);
    let limit = match crate::gamerules::GameRules::shared().get("max_block_modifications") {
        Some(crate::gamerules::GameRuleValue::Int(n)) => n,
        _ => 32768,
    };
    if volume > i64::from(limit) {
        return Err(fill_too_big(limit, volume));
    }
    let strict = mode == "strict";
    let flags = crate::shapes::UPDATE_CLIENTS | if strict { 816 } else { 256 };
    let mut effects = crate::shapes::Effects::new();
    let mut remembered: Vec<((i32, i32, i32), String)> = Vec::new();
    let mut count: i32 = 0;
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let pos = (x, y, z);
                let old = crate::edits::Blocks::state_at(world, pos);
                if mode == "keep" && !crate::shapes::is_air(&old) {
                    continue;
                }
                if mode == "destroy" && !crate::shapes::is_air(&old) {
                    return Err(Failure::unanswered(
                        "fill destroy: Level.destroyBlock's drops are not ported",
                    ));
                }
                let edge = x == min.0
                    || x == max.0
                    || y == min.1
                    || y == max.1
                    || z == min.2
                    || z == max.2;
                let target = match mode {
                    "outline" if !edge => continue,
                    "hollow" if !edge => crate::blockarg::BlockInput {
                        block: "minecraft:air".into(),
                        state: "minecraft:air".into(),
                        explicit: Vec::new(),
                    },
                    _ => input.clone(),
                };
                let shaped = if flags & crate::shapes::UPDATE_KNOWN_SHAPE != 0 {
                    target.state.clone()
                } else {
                    shaped_by_neighbours(world, shapes, pos, &target.state, &mut effects)
                };
                let shaped = if crate::shapes::is_air(&shaped) {
                    target.state.clone()
                } else {
                    shaped
                };
                let state = target.overwrite_defined(&shaped);
                if !crate::shapes::set_block(
                    world,
                    Some(shapes),
                    pos,
                    &state,
                    flags,
                    crate::shapes::RECURSION_LIMIT,
                    &mut effects,
                ) {
                    continue;
                }
                if !strict {
                    remembered.push((pos, old));
                }
                count += 1;
            }
        }
    }
    for (pos, old) in &remembered {
        update_neighbours_on_block_set(world, shapes, *pos, old, &mut effects);
    }
    apply_effects(ctx, &effects);
    if count == 0 {
        return Err(fill_failed());
    }
    Ok(vec![Feedback::Success {
        component: fill_success(count),
        to_admins: true,
    }])
}

/// `commands.clone.success` with the count.
#[must_use]
pub fn clone_success(count: i32) -> Nbt {
    translatable("commands.clone.success", &[Arg::Int(count)])
}

/// `CloneCommands.ERROR_OVERLAP` — `commands.clone.overlap`.
#[must_use]
pub fn clone_overlap() -> Failure {
    Failure::translatable("commands.clone.overlap", &[])
}

/// `CloneCommands.ERROR_FAILED` — `commands.clone.failed`.
#[must_use]
pub fn clone_failed() -> Failure {
    Failure::translatable("commands.clone.failed", &[])
}

/// `CloneCommands.ERROR_AREA_TOO_LARGE` — `commands.clone.toobig`, the limit
/// as an `Integer` and the volume as a `Long`.
#[must_use]
pub fn clone_too_big(max: i32, volume: i64) -> Failure {
    Failure::translatable("commands.clone.toobig", &[Arg::Int(max), Arg::Long(volume)])
}

/// One `CloneBlockInfo`: where it goes, what it is, and what was there.
struct CloneBlock {
    dest: (i32, i32, i32),
    state: String,
    previous: String,
}

/// `CloneCommands.clone` for the same-level forms without a filter.
///
/// ```text
///   dest box = destination + (end - begin) spans
///   !mode.canOverlap (NORMAL) && boxes intersect -> ERROR_OVERLAP
///   volume > max_block_modifications            -> ERROR_AREA_TOO_LARGE
///   both boxes loaded, else BlockPosArgument.ERROR_NOT_LOADED
///   z, y, x over the source: masked skips air; each block is filed as
///     block entity  -> blockEntities, deque.addLast(src)
///     solid render or full collision -> full, deque.addLast(src)
///     otherwise     -> other, deque.addFirst(src)
///   flags = 2 | (strict ? 816 : 0)
///   MOVE: every deque position -> barrier (flags | 816), then air
///         (strict ? flags : 3)
///   all = full + blockEntities + other
///   reversed(all): destination -> barrier (flags | 816)
///   all: setBlock(dest, state, flags), counting the writes that changed it
///   !strict: reversed(all): updateNeighboursOnBlockSet(dest, previous)
///   count == 0 -> ERROR_FAILED
/// ```
///
/// Two halves are not answered and say so: a source block with a BLOCK
/// ENTITY (its tag and components would have to be copied, and this executor
/// does not), and scheduled ticks inside the source box, which
/// `LevelTicks.copyAreaFrom` carries to the destination and this server's
/// queue has no area copy for.
fn clone(ctx: &Ctx<'_>, begin: &str, end: &str, dest: &str, rest: &[&str]) -> Outcome {
    let Ok(world) =
        crate::tiles::Tiles::shared(crate::status::WORLD_SEED, crate::status::WORLD_RADIUS)
    else {
        return Err(Failure::unanswered("clone: no world"));
    };
    let (Some(table), Some(shapes)) = (
        crate::blockarg::shared(),
        crate::shapes::shared_with_classes(std::path::Path::new(".")),
    ) else {
        return Err(Failure::unanswered("clone: no block or shape table"));
    };
    let a = loaded_block_pos(ctx, world, begin)?;
    let b = loaded_block_pos(ctx, world, end)?;
    let d = loaded_block_pos(ctx, world, dest)?;
    let strict = rest.first() == Some(&"strict");
    let masked = rest.contains(&"masked");
    let mode = rest
        .iter()
        .copied()
        .find(|m| matches!(*m, "force" | "move" | "normal"))
        .unwrap_or("normal");
    let min = (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2));
    let max = (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2));
    let len = (max.0 - min.0, max.1 - min.1, max.2 - min.2);
    let dmax = (d.0 + len.0, d.1 + len.1, d.2 + len.2);
    let intersects = min.0 <= dmax.0
        && max.0 >= d.0
        && min.1 <= dmax.1
        && max.1 >= d.1
        && min.2 <= dmax.2
        && max.2 >= d.2;
    if mode == "normal" && intersects {
        return Err(clone_overlap());
    }
    let volume = (i64::from(len.0) + 1) * (i64::from(len.1) + 1) * (i64::from(len.2) + 1);
    let limit = match crate::gamerules::GameRules::shared().get("max_block_modifications") {
        Some(crate::gamerules::GameRuleValue::Int(n)) => n,
        _ => 32768,
    };
    if volume > i64::from(limit) {
        return Err(clone_too_big(limit, volume));
    }
    for (lo, hi) in [(min, max), (d, dmax)] {
        for cx in (lo.0 >> 4)..=(hi.0 >> 4) {
            for cz in (lo.2 >> 4)..=(hi.2 >> 4) {
                if world.tile(cx, cz).is_none() {
                    return Err(pos_unloaded());
                }
            }
        }
    }
    let mut full = Vec::new();
    let mut other = Vec::new();
    let mut deque: std::collections::VecDeque<(i32, i32, i32)> = std::collections::VecDeque::new();
    for z in min.2..=max.2 {
        for y in min.1..=max.1 {
            for x in min.0..=max.0 {
                let src = (x, y, z);
                let state = crate::edits::Blocks::state_at(world, src);
                if masked && crate::shapes::is_air(&state) {
                    continue;
                }
                if crate::blockentity::BlockEntities::shared()
                    .get(src)
                    .is_some()
                {
                    return Err(Failure::unanswered(
                        "clone: a source block has a block entity, whose copy is not ported",
                    ));
                }
                let to = (x - min.0 + d.0, y - min.1 + d.1, z - min.2 + d.2);
                let info = CloneBlock {
                    dest: to,
                    previous: crate::edits::Blocks::state_at(world, to),
                    state: state.clone(),
                };
                if table.is_full_block(&state) {
                    full.push(info);
                    deque.push_back(src);
                } else {
                    other.push(info);
                    deque.push_front(src);
                }
            }
        }
    }
    let flags = crate::shapes::UPDATE_CLIENTS | if strict { 816 } else { 0 };
    let barrier = table
        .default_state("minecraft:barrier")
        .unwrap_or_else(|| "minecraft:barrier".into());
    let mut effects = crate::shapes::Effects::new();
    let write = |pos, state: &str, f, effects: &mut crate::shapes::Effects| {
        crate::shapes::set_block(
            world,
            Some(shapes),
            pos,
            state,
            f,
            crate::shapes::RECURSION_LIMIT,
            effects,
        )
    };
    if mode == "move" {
        for &pos in &deque {
            write(pos, &barrier, flags | 816, &mut effects);
        }
        let air_flags = if strict { flags } else { 3 };
        for &pos in &deque {
            write(pos, "minecraft:air", air_flags, &mut effects);
        }
    }
    let all: Vec<CloneBlock> = full.into_iter().chain(other).collect();
    for info in all.iter().rev() {
        write(info.dest, &barrier, flags | 816, &mut effects);
    }
    let mut count: i32 = 0;
    for info in &all {
        if write(info.dest, &info.state, flags, &mut effects) {
            count += 1;
        }
    }
    if !strict {
        for info in all.iter().rev() {
            update_neighbours_on_block_set(world, shapes, info.dest, &info.previous, &mut effects);
        }
    }
    apply_effects(ctx, &effects);
    if count == 0 {
        return Err(clone_failed());
    }
    Ok(vec![Feedback::Success {
        component: clone_success(count),
        to_admins: true,
    }])
}

/// `ServerLevel.updateNeighboursOnBlockSet(pos, oldState)`:
///
/// ```text
///   now = getBlockState(pos)
///   !oldState.is(now.getBlock()) -> oldState.affectNeighborsAfterRemoval(level, pos, false)
///   updateNeighborsAt(pos, now.getBlock())
///   now.hasAnalogOutputSignal() -> updateNeighbourForOutputSignal(pos, block)
/// ```
///
/// The analog-output half is the comparator's, and this server has no
/// per-state `hasAnalogOutputSignal` column to ask; it is skipped and said so.
fn update_neighbours_on_block_set(
    world: &crate::tiles::Tiles,
    shapes: &crate::shapes::ShapeTable,
    pos: (i32, i32, i32),
    old: &str,
    effects: &mut crate::shapes::Effects,
) {
    let now = crate::edits::Blocks::state_at(world, pos);
    let block = |s: &str| s.split('[').next().unwrap_or(s).to_string();
    if block(old) != block(&now) {
        crate::removal::affect_neighbors_after_removal(
            world,
            crate::power::shared(),
            Some(shapes),
            crate::placedby::PlacedByTable::shared(std::path::Path::new(
                "data/vanilla/set_placed_by.json",
            ))
            .ok(),
            pos,
            old,
            false,
            effects,
        );
    }
    crate::power::update_neighbors_at(
        world,
        crate::power::shared(),
        Some(shapes),
        pos,
        &block(&now),
        effects,
    );
}

/// Hand a command's block cascade to the level: written positions to the
/// tick's changed-block set, scheduled ticks to the queue at this game time,
/// level events to every player in range — the three things
/// `world_send`'s `record_updates` does for a player's own placement.
fn apply_effects(ctx: &Ctx<'_>, effects: &crate::shapes::Effects) {
    let changed = crate::broadcast::ChangedBlocks::shared();
    for &(p, _) in &effects.blocks {
        changed.record(p);
    }
    crate::ticks::drain_requests(
        crate::ticks::ScheduledTicks::shared(),
        &effects.ticks,
        game_time(),
    );
    if let Some(players) = ctx.players {
        let conns = players.connections();
        for &(event, pos, data) in &effects.level_events {
            if let Ok(body) = crate::interact::level_event_body(event, pos, data, false) {
                crate::playerlist::broadcast_within(
                    &conns,
                    &crate::playerlist::entity_locator(Some(crate::entities::Entities::shared())),
                    None,
                    (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2)),
                    crate::playerlist::LEVEL_EVENT_RANGE,
                    ids::play::cb::LEVEL_EVENT,
                    &body,
                );
            }
        }
    }
}

// ===========================================================================
// /playsound — `PlaySoundCommand`
// ===========================================================================

/// `SoundSource`'s eleven names, in ordinal order — the literals `/playsound`
/// registers, one per constant.
pub const SOUND_SOURCES: [&str; 11] = [
    "master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient",
    "voice", "ui",
];

/// `lambda$playSound$0` — `commands.playsound.success.single`: the sound id
/// (`Component.translationArg(Identifier)`, a literal) and the one player.
#[must_use]
pub fn playsound_single(sound: &str, target: &str) -> Nbt {
    translatable(
        "commands.playsound.success.single",
        &[
            Arg::Text(sound.to_string()),
            Arg::Component(crate::commands::display_name_component(target)),
        ],
    )
}

/// `lambda$playSound$1` — `commands.playsound.success.multiple`: the id and
/// how many heard it.
#[must_use]
pub fn playsound_multiple(sound: &str, count: i32) -> Nbt {
    translatable(
        "commands.playsound.success.multiple",
        &[Arg::Text(sound.to_string()), Arg::Int(count)],
    )
}

/// `PlaySoundCommand.ERROR_TOO_FAR` — `commands.playsound.failed`.
#[must_use]
pub fn playsound_failed() -> Failure {
    Failure::translatable("commands.playsound.failed", &[])
}

/// One of `/playsound`'s seven executable depths, with the defaults its
/// `executes` lambdas pass for what was not typed: the calling player as the
/// only target (`getCallingPlayerAsCollection`, empty from the console), the
/// source's position, volume 1, pitch 1, minimum volume 0, source `master`.
fn playsound(ctx: &Ctx<'_>, resolved: &Resolved, rest: &[&str]) -> Outcome {
    let sound_raw = resolved.arg("sound").unwrap_or_default();
    let sound = match sound_raw.find(':') {
        None => format!("minecraft:{sound_raw}"),
        Some(0) => format!("minecraft:{}", &sound_raw[1..]),
        Some(_) => sound_raw.to_string(),
    };
    let source = rest.first().copied().unwrap_or("master");
    let Some(source_id) = SOUND_SOURCES.iter().position(|s| *s == source) else {
        return Err(Failure::unanswered(&format!(
            "playsound: no source {source:?}"
        )));
    };
    let targets = if rest.len() >= 2 {
        find_players(ctx, resolved.arg("targets").unwrap_or_default())?
    } else {
        ctx.entity.clone().into_iter().collect()
    };
    let pos = match resolved.arg("pos") {
        Some(raw) => {
            let mut r = crate::cmdreader::Reader::new(raw);
            let Ok(Some(c)) = crate::commandtree::vec3_world(&mut r, true) else {
                return Err(Failure::unanswered(&format!(
                    "playsound {raw:?}: local coordinates are not resolved here"
                )));
            };
            [
                c[0].get(ctx.pos[0]),
                c[1].get(ctx.pos[1]),
                c[2].get(ctx.pos[2]),
            ]
        }
        None => ctx.pos,
    };
    let float = |name: &str, default: f32| {
        resolved
            .arg(name)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(default)
    };
    let volume = float("volume", 1.0);
    let pitch = float("pitch", 1.0);
    let min_volume = float("minVolume", 0.0);
    let heard = play_sound(&targets, &sound, source_id, pos, volume, pitch, min_volume);
    if heard.is_empty() {
        return Err(playsound_failed());
    }
    let component = if targets.len() == 1 {
        playsound_single(&sound, heard[0].name())
    } else {
        playsound_multiple(&sound, i32::try_from(heard.len()).unwrap_or(i32::MAX))
    };
    Ok(vec![Feedback::Success {
        component,
        to_admins: true,
    }])
}

/// `PlaySoundCommand.playSound`, offsets 0-298:
///
/// ```text
///   holder = Holder.direct(SoundEvent.createVariableRangeEvent(id))
///   d = Mth.square(event.getRange(volume))          16, or 16*volume above 1
///   seed = level.getRandom().nextLong()             ONE draw for every target
///   per target in the source's level:
///     dist2 = |pos - player|^2
///     too far: minVolume <= 0 -> skip
///              else at player + (pos - player)/dist * 2, volume minVolume
///     send ClientboundSoundPacket(holder, source, at, volume, pitch, seed)
/// ```
///
/// The holder is DIRECT, so the packet carries the id inline (`0`, the
/// identifier, no fixed range) for a registered sound as much as for any
/// other — which is why this does not go through `crate::sounds`, whose
/// packets name the registry id. The seed's one draw comes from the same
/// level random every other level sound uses.
fn play_sound(
    targets: &[Arc<Connection>],
    sound: &str,
    source: usize,
    pos: [f64; 3],
    volume: f32,
    pitch: f32,
    min_volume: f32,
) -> Vec<Arc<Connection>> {
    use rustmc_worldgen::rng::RandomSource;
    let range = if volume > 1.0 { 16.0 * volume } else { 16.0 };
    let limit = f64::from(range * range);
    let seed = crate::mob::level_random(RandomSource::next_i64);
    let mut heard = Vec::new();
    for t in targets {
        let Some((at, _, _)) = entity_pose(t) else {
            continue;
        };
        let d = [pos[0] - at[0], pos[1] - at[1], pos[2] - at[2]];
        let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let (here, vol) = if dist2 > limit {
            if min_volume <= 0.0 {
                continue;
            }
            let dist = dist2.sqrt();
            (
                [
                    at[0] + d[0] / dist * 2.0,
                    at[1] + d[1] / dist * 2.0,
                    at[2] + d[2] / dist * 2.0,
                ],
                min_volume,
            )
        } else {
            (pos, volume)
        };
        if let Ok(body) = inline_sound_body(sound, source, here, vol, pitch, seed) {
            let _ = t.send(ids::play::cb::SOUND, &body);
        }
        heard.push(Arc::clone(t));
    }
    heard
}

/// `ClientboundSoundPacket` with a DIRECT holder: varint 0, then
/// `SoundEvent.DIRECT_STREAM_CODEC` (the identifier and an absent fixed
/// range), the source, the position in eighths (`(int)(x * 8.0)`), volume,
/// pitch and the seed.
///
/// # Errors
/// Never, for a `Vec`.
pub fn inline_sound_body(
    sound: &str,
    source: usize,
    at: [f64; 3],
    volume: f32,
    pitch: f32,
    seed: i64,
) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    rustmc_proto::varint::write_varint(&mut b, 0)?;
    wire::write_string(&mut b, sound)?;
    wire::write_bool(&mut b, false)?;
    rustmc_proto::varint::write_varint(&mut b, i32::try_from(source).unwrap_or(0))?;
    let e = crate::sounds::Eighths::exact(at[0], at[1], at[2]);
    for v in [e.0, e.1, e.2] {
        wire::write_i32(&mut b, v)?;
    }
    wire::write_f32(&mut b, volume)?;
    wire::write_f32(&mut b, pitch)?;
    wire::write_i64(&mut b, seed)?;
    Ok(b)
}

/// `StopSoundCommand.stopSound`'s four replies, by which of the source and
/// the sound were named.
#[must_use]
pub fn stopsound_reply(source: Option<&str>, sound: Option<&str>) -> Nbt {
    match (source, sound) {
        (Some(src), Some(id)) => translatable(
            "commands.stopsound.success.source.sound",
            &[Arg::Text(id.to_string()), Arg::Text(src.to_string())],
        ),
        (Some(src), None) => translatable(
            "commands.stopsound.success.source.any",
            &[Arg::Text(src.to_string())],
        ),
        (None, Some(id)) => translatable(
            "commands.stopsound.success.sourceless.sound",
            &[Arg::Text(id.to_string())],
        ),
        (None, None) => translatable("commands.stopsound.success.sourceless.any", &[]),
    }
}

/// `ClientboundStopSoundPacket(name, source)`'s body: a flag byte — 1 for a
/// source, 2 for a name — then the source's ordinal and the identifier.
#[must_use]
pub fn stop_sound_body(source: Option<usize>, sound: Option<&str>) -> Vec<u8> {
    let mut b = vec![u8::from(source.is_some()) | (u8::from(sound.is_some()) << 1)];
    if let Some(src) = source {
        let _ = rustmc_proto::varint::write_varint(&mut b, i32::try_from(src).unwrap_or(0));
    }
    if let Some(id) = sound {
        let _ = wire::write_string(&mut b, id);
    }
    b
}

/// `StopSoundCommand.stopSound(source, targets, category, name)`: ONE packet
/// sent to every target, then the reply naming what was stopped. `*` is the
/// `null` category with a name; a bare `targets` is both `null`.
fn stop_sound(
    targets: &[Arc<Connection>],
    source: Option<&str>,
    sound: Option<&str>,
) -> Vec<Feedback> {
    let sound = sound.map(|raw| match raw.find(':') {
        None => format!("minecraft:{raw}"),
        Some(0) => format!("minecraft:{}", &raw[1..]),
        Some(_) => raw.to_string(),
    });
    let ordinal = source.and_then(|s| SOUND_SOURCES.iter().position(|x| *x == s));
    let body = stop_sound_body(ordinal, sound.as_deref());
    for t in targets {
        let _ = t.send(ids::play::cb::STOP_SOUND, &body);
    }
    vec![Feedback::Success {
        component: stopsound_reply(source, sound.as_deref()),
        to_admins: true,
    }]
}

// ===========================================================================
// /random — `RandomCommand`, the level-0 half
// ===========================================================================

/// `lambda$randomSample$0` — `commands.random.sample.success` with the value.
#[must_use]
pub fn random_sample_success(value: i32) -> Nbt {
    translatable("commands.random.sample.success", &[Arg::Int(value)])
}

/// The roll line: `commands.random.roll` with the source's display name, the
/// value, and the bounds the draw used.
#[must_use]
pub fn random_roll(name: &Nbt, value: i32, min: i32, max: i32) -> Nbt {
    translatable(
        "commands.random.roll",
        &[
            Arg::Component(name.clone()),
            Arg::Int(value),
            Arg::Int(min),
            Arg::Int(max),
        ],
    )
}

/// `ERROR_RANGE_TOO_SMALL` — `commands.random.error.range_too_small`.
#[must_use]
pub fn random_too_small() -> Failure {
    Failure::translatable("commands.random.error.range_too_small", &[])
}

/// `ERROR_RANGE_TOO_LARGE` — `commands.random.error.range_too_large`.
#[must_use]
pub fn random_too_large() -> Failure {
    Failure::translatable("commands.random.error.range_too_large", &[])
}

/// `RandomCommand.randomSample(source, range, null, broadcast)`:
///
/// ```text
///   random = level.getRandom()                (no sequence: the level's)
///   min = range.min or MIN_VALUE; max = range.max or MAX_VALUE
///   (long) max - min == 0                     -> ERROR_RANGE_TOO_SMALL
///   (long) max - min >= Integer.MAX_VALUE     -> ERROR_RANGE_TOO_LARGE
///   value = Mth.randomBetweenInclusive(random, min, max)
///           = random.nextInt(max - min + 1) + min
///   roll:  playerList.broadcastSystemMessage(commands.random.roll, false)
///   value: sendSuccess(commands.random.sample.success, false)
/// ```
///
/// The `sequence` argument and `reset` are level 2 and use named random
/// sequences this server does not keep; they are not registered.
fn random_sample(ctx: &Ctx<'_>, raw: &str, roll: bool) -> Outcome {
    use rustmc_worldgen::rng::RandomSource;
    let mut r = crate::cmdreader::Reader::new(raw);
    let Ok((lo, hi)) = crate::commandtree::int_range_value(&mut r) else {
        return Err(Failure::unanswered(&format!(
            "random range {raw:?} did not re-parse"
        )));
    };
    let min = lo.unwrap_or(i32::MIN);
    let max = hi.unwrap_or(i32::MAX);
    let span = i64::from(max) - i64::from(min);
    if span == 0 {
        return Err(random_too_small());
    }
    if span >= i64::from(i32::MAX) {
        return Err(random_too_large());
    }
    let value = crate::mob::level_random(|rng| rng.next_i32_bound(max - min + 1)) + min;
    if roll {
        let line = random_roll(&ctx.display_name(), value, min, max);
        if let Some(players) = ctx.players {
            for p in players.connections() {
                let _ = send_system_chat(&p, &line);
            }
        }
        return Ok(Vec::new());
    }
    Ok(vec![Feedback::Success {
        component: random_sample_success(value),
        to_admins: false,
    }])
}

// ===========================================================================
// /weather — `WeatherCommand`
// ===========================================================================

/// `WeatherCommand.setClear`/`setRain`/`setThunder`, through the same
/// [`crate::weather::dispatch`] the console used before this module, so the
/// duration draw and the state change are one implementation; the reply is
/// `sendSuccess(translatable(commands.weather.set.<arm>), true)`.
fn weather(arm: &str, duration: Option<&str>) -> Outcome {
    let line = match duration {
        Some(d) => format!("{arm} {d}"),
        None => arm.to_string(),
    };
    match crate::weather::dispatch_console(&mut line.split_whitespace()) {
        Ok(key) => Ok(vec![Feedback::Success {
            component: translatable(&key, &[]),
            to_admins: true,
        }]),
        // A refusal here is a disagreement between the tree and the parser
        // `dispatch` re-runs, not a command vanilla would refuse.
        Err(e) => {
            eprintln!("weather {line:?}: {e} — not answered");
            Ok(Vec::new())
        }
    }
}

// ===========================================================================
// /teleport and /tp — `TeleportCommand`
// ===========================================================================

/// `CommandSourceStack.ERROR_NOT_ENTITY` — `permissions.requires.entity`.
#[must_use]
pub fn not_entity() -> Failure {
    Failure::translatable("permissions.requires.entity", &[])
}

/// `TeleportCommand.INVALID_POSITION` — `commands.teleport.invalidPosition`.
#[must_use]
pub fn invalid_position() -> Failure {
    Failure::translatable("commands.teleport.invalidPosition", &[])
}

/// `getEntityOrException()`. Every entity that can be a command source on
/// this server is a player, so it is [`self_player`] with the entity's own
/// exception.
fn self_entity(ctx: &Ctx<'_>) -> Result<Arc<Connection>, Failure> {
    self_player(ctx).map_err(|_| not_entity())
}

/// `EntityArgument.getEntities` for an `entities()` argument, as far as this
/// server can answer it: a name, a UUID, `@s`, `@p`, `@a`. `@e` and `@n`
/// select non-player entities too, which this module has no handle on, so
/// they are [`Failure::unanswered`] rather than answered with the players
/// alone, which would be a different command.
///
/// # Errors
/// `argument.entity.notfound.entity` for no match.
fn find_entities(ctx: &Ctx<'_>, raw: &str) -> Result<Vec<Arc<Connection>>, Failure> {
    let found = select_players(ctx, raw)?;
    if found.is_empty() {
        return Err(Failure::translatable(
            "argument.entity.notfound.entity",
            &[],
        ));
    }
    Ok(found)
}

/// `TeleportCommand.formatDouble`: `String.format(Locale.ROOT, "%f", d)` —
/// six places, Java's HALF_UP.
#[must_use]
pub fn format_double(v: f64) -> String {
    crate::bordercmd::java_format_f(v, 6)
}

/// `lambda$teleportToPos$0` — `commands.teleport.success.location.single`.
#[must_use]
pub fn teleport_location_single(target: &Nbt, at: [f64; 3]) -> Nbt {
    translatable(
        "commands.teleport.success.location.single",
        &[
            Arg::Component(target.clone()),
            Arg::Text(format_double(at[0])),
            Arg::Text(format_double(at[1])),
            Arg::Text(format_double(at[2])),
        ],
    )
}

/// `lambda$teleportToPos$1` — `commands.teleport.success.location.multiple`.
#[must_use]
pub fn teleport_location_multiple(count: i32, at: [f64; 3]) -> Nbt {
    translatable(
        "commands.teleport.success.location.multiple",
        &[
            Arg::Int(count),
            Arg::Text(format_double(at[0])),
            Arg::Text(format_double(at[1])),
            Arg::Text(format_double(at[2])),
        ],
    )
}

/// `lambda$teleportToEntity$0` — `commands.teleport.success.entity.single`.
#[must_use]
pub fn teleport_entity_single(target: &Nbt, destination: &Nbt) -> Nbt {
    translatable(
        "commands.teleport.success.entity.single",
        &[
            Arg::Component(target.clone()),
            Arg::Component(destination.clone()),
        ],
    )
}

/// `lambda$teleportToEntity$1` — `commands.teleport.success.entity.multiple`.
#[must_use]
pub fn teleport_entity_multiple(count: i32, destination: &Nbt) -> Nbt {
    translatable(
        "commands.teleport.success.entity.multiple",
        &[Arg::Int(count), Arg::Component(destination.clone())],
    )
}

/// `Relative`'s bits, `1 << bit` — the second constructor argument of each
/// constant in `Relative.<clinit>`.
pub mod relative {
    pub const X: i32 = 1 << 0;
    pub const Y: i32 = 1 << 1;
    pub const Z: i32 = 1 << 2;
    pub const Y_ROT: i32 = 1 << 3;
    pub const X_ROT: i32 = 1 << 4;
    pub const DELTA_X: i32 = 1 << 5;
    pub const DELTA_Y: i32 = 1 << 6;
    pub const DELTA_Z: i32 = 1 << 7;
}

/// `Mth.wrapDegrees(float)`.
#[must_use]
pub fn wrap_degrees(v: f32) -> f32 {
    let mut f = v % 360.0;
    if f >= 180.0 {
        f -= 360.0;
    }
    if f < -180.0 {
        f += 360.0;
    }
    f
}

/// A target's current position and rotation, from the entity store the play
/// loop writes every movement into.
fn entity_pose(c: &Connection) -> Option<([f64; 3], f32, f32)> {
    crate::entities::Entities::shared()
        .get(c.entity_id())
        .map(|e| ([e.x, e.y, e.z], e.y_rot, e.x_rot))
}

/// `TeleportCommand.performTeleport`, up to `teleportTo`: the spawnable-bounds
/// check, the relative-axis subtraction, `wrapDegrees`, then the teleport —
/// queued for the target's own loop, which is where `teleportTo` lands.
///
/// What follows `teleportTo` in vanilla is server-side only: the downward
/// motion zeroed and `onGround` set (for anything not fall-flying), and a
/// pathfinder's navigation stopped. No packet comes of either.
fn perform_teleport(
    target: &Connection,
    pos: [f64; 3],
    relatives: i32,
    y_rot: f32,
    x_rot: f32,
) -> Result<(), Failure> {
    // `Level.isInSpawnableBounds(BlockPos.containing(x, y, z))`.
    let b = [pos[0].floor(), pos[1].floor(), pos[2].floor()];
    let inside = (-20_000_000.0..20_000_000.0).contains(&b[1])
        && (-30_000_000.0..30_000_000.0).contains(&b[0])
        && (-30_000_000.0..30_000_000.0).contains(&b[2]);
    if !inside {
        return Err(invalid_position());
    }
    let (cur, cur_y, cur_x) = entity_pose(target).unwrap_or(([0.0; 3], 0.0, 0.0));
    let axis = |bit: i32, v: f64, c: f64| if relatives & bit != 0 { v - c } else { v };
    let x = axis(relative::X, pos[0], cur[0]);
    let y = axis(relative::Y, pos[1], cur[1]);
    let z = axis(relative::Z, pos[2], cur[2]);
    let yr = if relatives & relative::Y_ROT != 0 {
        y_rot - cur_y
    } else {
        y_rot
    };
    let xr = if relatives & relative::X_ROT != 0 {
        x_rot - cur_x
    } else {
        x_rot
    };
    target.queue_teleport(crate::players::PendingTeleport {
        pos: [x, y, z],
        y_rot: wrap_degrees(yr),
        x_rot: wrap_degrees(xr),
        relatives,
        rotation_only: false,
    });
    Ok(())
}

/// `TeleportCommand.teleportToPos` without `facing`.
///
/// `getRelatives(position, rotation, sameDimension)`: the axes typed with `~`
/// get both their `DELTA_*` and their position bit; then, with no rotation,
/// `Relative.ROTATION` (both rotation bits), so the target keeps its facing —
/// `performTeleport` is handed the target's own rotation and the relative
/// subtraction turns it into a zero delta; with a rotation,
/// `Relative.rotation(yRelative, xRelative)` and `getRotation(source)` —
/// resolved against the SOURCE's rotation — as the angles.
///
/// A `^` triple resolves against the source's facing, which no executor here
/// reads; it is not answered rather than answered as if it were `~`.
fn teleport_to_pos(
    ctx: &Ctx<'_>,
    targets: &[Arc<Connection>],
    raw: &str,
    rotation: Option<&str>,
) -> Outcome {
    let mut r = crate::cmdreader::Reader::new(raw);
    let Ok(Some(coords)) = crate::commandtree::vec3_world(&mut r, true) else {
        return Err(Failure::unanswered(&format!(
            "teleport {raw:?}: local coordinates are not resolved here"
        )));
    };
    let rot = match rotation {
        Some(text) => {
            let mut r = crate::cmdreader::Reader::new(text);
            match crate::commandtree::rotation_value(&mut r) {
                Ok(v) => Some(v),
                Err(_) => {
                    return Err(Failure::unanswered(&format!(
                        "teleport rotation {text:?} did not re-parse"
                    )))
                }
            }
        }
        None => None,
    };
    let src = source_position(ctx);
    let at = [
        coords[0].get(src[0]),
        coords[1].get(src[1]),
        coords[2].get(src[2]),
    ];
    let mut relatives = match rot {
        None => relative::Y_ROT | relative::X_ROT,
        Some([yaw, pitch]) => {
            (if yaw.relative { relative::Y_ROT } else { 0 })
                | (if pitch.relative { relative::X_ROT } else { 0 })
        }
    };
    for (i, (pos_bit, delta_bit)) in [
        (relative::X, relative::DELTA_X),
        (relative::Y, relative::DELTA_Y),
        (relative::Z, relative::DELTA_Z),
    ]
    .into_iter()
    .enumerate()
    {
        if coords[i].relative {
            relatives |= pos_bit | delta_bit;
        }
    }
    for t in targets {
        #[expect(clippy::cast_possible_truncation, reason = "`Vec2` holds floats")]
        fn narrow(v: f64) -> f32 {
            v as f32
        }
        let (y_rot, x_rot) = match rot {
            // `(float) y.get(source.yRot)`, `(float) x.get(source.xRot)`.
            Some([yaw, pitch]) => (
                narrow(yaw.get(f64::from(ctx.rot[1]))),
                narrow(pitch.get(f64::from(ctx.rot[0]))),
            ),
            None => {
                let (_, y, x) = entity_pose(t).unwrap_or(([0.0; 3], 0.0, 0.0));
                (y, x)
            }
        };
        perform_teleport(t, at, relatives, y_rot, x_rot)?;
    }
    let component = if let [only] = targets {
        teleport_location_single(&crate::commands::display_name_component(only.name()), at)
    } else {
        teleport_location_multiple(i32::try_from(targets.len()).unwrap_or(i32::MAX), at)
    };
    Ok(vec![Feedback::Success {
        component,
        to_admins: true,
    }])
}

/// `TeleportCommand.teleportToEntity`: every target to the destination's
/// position and rotation, absolutely — `EnumSet.noneOf(Relative.class)`.
fn teleport_to_entity(ctx: &Ctx<'_>, targets: &[Arc<Connection>], raw: &str) -> Outcome {
    // `EntityArgument.getEntity` -> `findSingleEntity`: no match is
    // `NO_ENTITIES_FOUND`.
    let dest = find_entities(ctx, raw)?.remove(0);
    let Some((pos, y_rot, x_rot)) = entity_pose(&dest) else {
        return Err(Failure::unanswered(&format!(
            "teleport: {} has no entity",
            dest.name()
        )));
    };
    for t in targets {
        perform_teleport(t, pos, 0, y_rot, x_rot)?;
    }
    let dest_name = crate::commands::display_name_component(dest.name());
    let component = if let [only] = targets {
        teleport_entity_single(
            &crate::commands::display_name_component(only.name()),
            &dest_name,
        )
    } else {
        teleport_entity_multiple(i32::try_from(targets.len()).unwrap_or(i32::MAX), &dest_name)
    };
    Ok(vec![Feedback::Success {
        component,
        to_admins: true,
    }])
}

// ===========================================================================
// /bossbar — `BossBarCommands`, over `crate::bossbar`
// ===========================================================================

/// `ERROR_ALREADY_EXISTS` — `commands.bossbar.create.failed`, the id's
/// `toString()`.
#[must_use]
pub fn bossbar_already_exists(id: &str) -> Failure {
    Failure::translatable(
        "commands.bossbar.create.failed",
        &[Arg::Text(id.to_string())],
    )
}

/// `ERROR_DOESNT_EXIST` — `commands.bossbar.unknown`, the id's `toString()`.
#[must_use]
pub fn bossbar_unknown(id: &str) -> Failure {
    Failure::translatable("commands.bossbar.unknown", &[Arg::Text(id.to_string())])
}

/// The eight no-argument refusals, by their `BossBarCommands` field name.
#[must_use]
pub fn bossbar_unchanged(field: &str) -> Failure {
    let key = match field {
        "ERROR_NO_PLAYER_CHANGE" => "commands.bossbar.set.players.unchanged",
        "ERROR_NO_NAME_CHANGE" => "commands.bossbar.set.name.unchanged",
        "ERROR_NO_COLOR_CHANGE" => "commands.bossbar.set.color.unchanged",
        "ERROR_NO_STYLE_CHANGE" => "commands.bossbar.set.style.unchanged",
        "ERROR_NO_VALUE_CHANGE" => "commands.bossbar.set.value.unchanged",
        "ERROR_NO_MAX_CHANGE" => "commands.bossbar.set.max.unchanged",
        "ERROR_ALREADY_HIDDEN" => "commands.bossbar.set.visibility.unchanged.hidden",
        _ => "commands.bossbar.set.visibility.unchanged.visible",
    };
    Failure::translatable(key, &[])
}

/// A `commands.bossbar.*` reply whose first argument is the bar's display
/// name and whose others follow it.
#[must_use]
pub fn bossbar_reply(key: &str, bar: &crate::bossbar::Bar, more: &[Arg]) -> Nbt {
    let mut args = vec![Arg::Component(bar.display_name())];
    args.extend(more.iter().cloned());
    translatable(&format!("commands.bossbar.{key}"), &args)
}

/// `ComponentUtils.formatList(items, DEFAULT_SEPARATOR)` — the gray `", "`
/// separator, the single element unwrapped, `Component.empty()` around more.
fn format_list(items: Vec<Nbt>) -> Nbt {
    match items.len() {
        0 => crate::commands::compound([("text", Nbt::String(String::new()))]),
        1 => items
            .into_iter()
            .next()
            .unwrap_or(Nbt::String(String::new())),
        _ => {
            let mut extra = Vec::new();
            for (i, item) in items.into_iter().enumerate() {
                if i > 0 {
                    extra.push(crate::commands::compound([
                        ("color", Nbt::String("gray".into())),
                        ("text", Nbt::String(", ".into())),
                    ]));
                }
                extra.push(item);
            }
            crate::commands::compound([
                ("extra", Nbt::List(extra)),
                ("text", Nbt::String(String::new())),
            ])
        }
    }
}

/// `commands.bossbar.list.bars.*`.
#[must_use]
pub fn bossbar_list(bars: &[&crate::bossbar::Bar]) -> Nbt {
    if bars.is_empty() {
        return translatable("commands.bossbar.list.bars.none", &[]);
    }
    translatable(
        "commands.bossbar.list.bars.some",
        &[
            Arg::Int(i32::try_from(bars.len()).unwrap_or(i32::MAX)),
            Arg::Component(format_list(bars.iter().map(|b| b.display_name()).collect())),
        ],
    )
}

/// The players-list replies, `set.players.success.*` and `get.players.*`:
/// the display name alone for none, then the count and the holders'
/// display names (`ServerBossEvent.getPlayers()`, the holder set).
fn bossbar_players_reply(stem: &str, bar: &crate::bossbar::Bar) -> Nbt {
    if bar.holders.is_empty() {
        return bossbar_reply(&format!("{stem}.none"), bar, &[]);
    }
    let names = bar
        .holders
        .iter()
        .map(|h| crate::commands::display_name_component(h.name()))
        .collect();
    bossbar_reply(
        &format!("{stem}.some"),
        bar,
        &[
            Arg::Int(i32::try_from(bar.holders.len()).unwrap_or(i32::MAX)),
            Arg::Component(format_list(names)),
        ],
    )
}

/// `BossBarCommands`' executors.
///
/// ```text
///   add <id> <name>    get(id) != null -> ERROR_ALREADY_EXISTS
///                      create(level.getRandom(), id, resolve(name))
///   remove <id>        removeAllPlayers; remove
///   set ... / get ...  getBossBar: absent -> ERROR_DOESNT_EXIST
///   set value|max      unchanged -> ERROR_NO_VALUE/MAX_CHANGE, then
///                      setValue/setMax -> setProgress(clamp(v / max, 0, 1))
///   set name           resolve(name).equals(getName()) -> ERROR_NO_NAME_CHANGE
///   set color|style    equal -> ERROR_NO_COLOR/STYLE_CHANGE
///   set visible        equal -> ERROR_ALREADY_VISIBLE / _HIDDEN
///   set players [t]    !setPlayers(t or empty) -> ERROR_NO_PLAYER_CHANGE
/// ```
///
/// Every success reply carries the bar's display name AFTER the change, so a
/// recoloured bar is named in its new colour. The packets go out from inside
/// the setters, as `ServerBossEvent`'s do.
fn bossbar(ctx: &Ctx<'_>, resolved: &Resolved, rest: &[&str]) -> Outcome {
    let id = namespaced(resolved.arg("id").unwrap_or_default());
    let mut bars = crate::bossbar::shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let ok = |component: Nbt, to_admins: bool| {
        Ok(vec![Feedback::Success {
            component,
            to_admins,
        }])
    };
    match rest {
        ["add", "id", "name"] => {
            if bars.get(&id).is_some() {
                return Err(bossbar_already_exists(&id));
            }
            let name = component_arg(resolved.arg("name").unwrap_or_default())?;
            let uuid = crate::mob::level_random(|rng| {
                use rustmc_worldgen::rng::RandomSource;
                crate::bossbar::insecure_uuid(|| rng.next_i64())
            });
            let bar = crate::bossbar::Bar::new(&id, uuid, name);
            let reply = bossbar_reply("create.success", &bar, &[]);
            bars.insert(bar);
            ok(reply, true)
        }
        ["list"] => ok(bossbar_list(&bars.iter()), false),
        ["remove", "id"] => {
            let Some(mut bar) = bars.remove(&id) else {
                return Err(bossbar_unknown(&id));
            };
            bar.remove_all_holders();
            ok(bossbar_reply("remove.success", &bar, &[]), true)
        }
        ["set", "id", what @ ..] => {
            let Some(bar) = bars.get_mut(&id) else {
                return Err(bossbar_unknown(&id));
            };
            let int = |n: &str| -> i32 {
                resolved
                    .arg(n)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_default()
            };
            match what {
                ["name", "name"] => {
                    let name = component_arg(resolved.arg("name").unwrap_or_default())?;
                    if name == bar.name {
                        return Err(bossbar_unchanged("ERROR_NO_NAME_CHANGE"));
                    }
                    bar.set_name(name);
                    ok(bossbar_reply("set.name.success", bar, &[]), true)
                }
                ["color", c] => {
                    let i = crate::bossbar::COLORS
                        .iter()
                        .position(|x| x == c)
                        .unwrap_or_default();
                    if i == bar.color {
                        return Err(bossbar_unchanged("ERROR_NO_COLOR_CHANGE"));
                    }
                    bar.set_color(i);
                    ok(bossbar_reply("set.color.success", bar, &[]), true)
                }
                ["style", o] => {
                    let i = crate::bossbar::OVERLAYS
                        .iter()
                        .position(|x| x == o)
                        .unwrap_or_default();
                    if i == bar.overlay {
                        return Err(bossbar_unchanged("ERROR_NO_STYLE_CHANGE"));
                    }
                    bar.set_overlay(i);
                    ok(bossbar_reply("set.style.success", bar, &[]), true)
                }
                ["value", "value"] => {
                    let v = int("value");
                    if v == bar.value {
                        return Err(bossbar_unchanged("ERROR_NO_VALUE_CHANGE"));
                    }
                    bar.set_value(v);
                    ok(
                        bossbar_reply("set.value.success", bar, &[Arg::Int(v)]),
                        true,
                    )
                }
                ["max", "max"] => {
                    let v = int("max");
                    if v == bar.max {
                        return Err(bossbar_unchanged("ERROR_NO_MAX_CHANGE"));
                    }
                    bar.set_max(v);
                    ok(bossbar_reply("set.max.success", bar, &[Arg::Int(v)]), true)
                }
                ["visible", "visible"] => {
                    let v = resolved.arg("visible") == Some("true");
                    if v == bar.visible {
                        return Err(bossbar_unchanged(if v {
                            "ERROR_ALREADY_VISIBLE"
                        } else {
                            "ERROR_ALREADY_HIDDEN"
                        }));
                    }
                    bar.set_visible(v);
                    let key = if v {
                        "set.visible.success.visible"
                    } else {
                        "set.visible.success.hidden"
                    };
                    ok(bossbar_reply(key, bar, &[]), true)
                }
                ["players", ..] => {
                    let targets = match resolved.arg("targets") {
                        // `EntityArgument.getOptionalPlayers`: no match is an
                        // empty collection, not NO_PLAYERS_FOUND.
                        Some(raw) => select_players(ctx, raw)?,
                        None => Vec::new(),
                    };
                    if !bar.set_players(&targets) {
                        return Err(bossbar_unchanged("ERROR_NO_PLAYER_CHANGE"));
                    }
                    ok(bossbar_players_reply("set.players.success", bar), true)
                }
                _ => Err(Failure::unanswered(&format!("bossbar set {what:?}"))),
            }
        }
        ["get", "id", what] => {
            let Some(bar) = bars.get(&id) else {
                return Err(bossbar_unknown(&id));
            };
            match *what {
                "value" => ok(
                    bossbar_reply("get.value", bar, &[Arg::Int(bar.value)]),
                    true,
                ),
                "max" => ok(bossbar_reply("get.max", bar, &[Arg::Int(bar.max)]), true),
                "visible" => {
                    let key = if bar.visible {
                        "get.visible.visible"
                    } else {
                        "get.visible.hidden"
                    };
                    ok(bossbar_reply(key, bar, &[]), true)
                }
                _ => ok(bossbar_players_reply("get.players", bar), true),
            }
        }
        _ => Err(Failure::unanswered(&format!("bossbar {rest:?}"))),
    }
}

// ===========================================================================
// /dialog clear — `DialogCommand.clearDialog`
// ===========================================================================

/// `lambda$clearDialog$0` — `commands.dialog.clear.single`.
#[must_use]
pub fn dialog_clear_single(target: &Nbt) -> Nbt {
    translatable(
        "commands.dialog.clear.single",
        &[Arg::Component(target.clone())],
    )
}

/// `lambda$clearDialog$1` — `commands.dialog.clear.multiple`.
#[must_use]
pub fn dialog_clear_multiple(count: i32) -> Nbt {
    translatable("commands.dialog.clear.multiple", &[Arg::Int(count)])
}

/// `DialogCommand.clearDialog`, offsets 0-78: `ClientboundClearDialogPacket.
/// INSTANCE` — an empty body — to each target, then the single reply for one
/// target and the multiple for more. The packet goes whether or not a dialog
/// is open: the server does not track one.
fn dialog_clear(targets: &[Arc<Connection>]) -> Vec<Feedback> {
    for t in targets {
        let _ = t.send(ids::play::cb::CLEAR_DIALOG, &[]);
    }
    let component = if let [only] = targets {
        dialog_clear_single(&crate::commands::display_name_component(only.name()))
    } else {
        dialog_clear_multiple(i32::try_from(targets.len()).unwrap_or(i32::MAX))
    };
    vec![Feedback::Success {
        component,
        to_admins: true,
    }]
}

/// `TeamMsgCommand.ERROR_NOT_ON_TEAM` — `commands.teammsg.failed.noteam`.
#[must_use]
pub fn teammsg_no_team() -> Failure {
    Failure::translatable("commands.teammsg.failed.noteam", &[])
}

// ===========================================================================
// /particle — `ParticleCommand`
// ===========================================================================

/// `lambda$sendParticles$0` — `commands.particle.success`, the type's
/// registry key as a string.
#[must_use]
pub fn particle_success(name: &str) -> Nbt {
    translatable("commands.particle.success", &[Arg::Text(name.to_string())])
}

/// `ParticleCommand.ERROR_FAILED` — `commands.particle.failed`.
#[must_use]
pub fn particle_failed() -> Failure {
    Failure::translatable("commands.particle.failed", &[])
}

/// The `minecraft:` form of an identifier, as `Identifier.toString` prints
/// the one `Identifier.read` returned.
fn namespaced(id: &str) -> String {
    match id.split_once(':') {
        None => format!("minecraft:{id}"),
        Some(("", path)) => format!("minecraft:{path}"),
        Some(_) => id.to_string(),
    }
}

/// `ClientboundLevelParticlesPacket.write` for a simple type: the two flags,
/// the position as doubles, the spreads and the speed as floats, the count,
/// then `ParticleTypes.STREAM_CODEC` — the registry id and, for a
/// `SimpleParticleType`, nothing after it.
///
/// # Errors
/// Never, for a `Vec`.
#[allow(clippy::too_many_arguments)]
pub fn level_particles_body(
    override_limiter: bool,
    always_show: bool,
    pos: [f64; 3],
    dist: [f32; 3],
    speed: f32,
    count: i32,
    particle: i32,
) -> io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(48);
    b.push(u8::from(override_limiter));
    b.push(u8::from(always_show));
    for v in pos {
        wire::write_f64(&mut b, v)?;
    }
    for v in dist {
        wire::write_f32(&mut b, v)?;
    }
    wire::write_f32(&mut b, speed)?;
    wire::write_i32(&mut b, count)?;
    rustmc_proto::varint::write_varint(&mut b, particle)?;
    Ok(b)
}

/// `ParticleCommand.sendParticles`, offsets 0-110, and the
/// `ServerLevel.sendParticles(ServerPlayer, ...)` it calls per viewer:
///
/// ```text
///   for viewer in viewers:
///     packet = new ClientboundLevelParticlesPacket(particle, force, false,
///                  pos, (float) delta, (float) speed, count)
///     if viewer.level() == this
///        && viewer.blockPosition().closerToCenterThan(pos, force ? 512 : 32):
///         send; n++
///   n == 0 -> ERROR_FAILED
///   sendSuccess(commands.particle.success, true)
/// ```
///
/// `force` is `overrideLimiter` in the packet AND the long range; the
/// packet's `alwaysShow` is always false from this command. Without a
/// `viewers` argument every player on the server is a viewer. The position
/// and the delta are both `Vec3Argument.getVec3` — resolved against the
/// source's position — and only the position is centre-corrected.
fn particle(ctx: &Ctx<'_>, resolved: &Resolved, rest: &[&str]) -> Outcome {
    let name_raw = resolved.arg("name").unwrap_or_default();
    let ty = {
        let mut r = crate::cmdreader::Reader::new(name_raw);
        match crate::commandtree::particle_value(&mut r) {
            Ok(t) => t,
            Err(_) => {
                return Err(Failure::unanswered(&format!(
                    "particle {name_raw:?} did not re-parse"
                )))
            }
        }
    };
    let src = source_position(ctx);
    let vec3 = |name: &str, center: bool| -> Result<Option<[f64; 3]>, Failure> {
        let Some(raw) = resolved.arg(name) else {
            return Ok(None);
        };
        let mut r = crate::cmdreader::Reader::new(raw);
        match crate::commandtree::vec3_world(&mut r, center) {
            Ok(Some(c)) => Ok(Some([c[0].get(src[0]), c[1].get(src[1]), c[2].get(src[2])])),
            _ => Err(Failure::unanswered(&format!(
                "particle {name} {raw:?}: local coordinates are not resolved here"
            ))),
        }
    };
    let pos = vec3("pos", true)?.unwrap_or(src);
    let delta = vec3("delta", false)?.unwrap_or([0.0; 3]);
    let speed: f32 = resolved
        .arg("speed")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let count: i32 = resolved
        .arg("count")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let force = rest.contains(&"force");
    let viewers = match resolved.arg("viewers") {
        // `EntityArgument.getPlayers`: an empty match is NO_PLAYERS_FOUND.
        Some(raw) => find_players(ctx, raw)?,
        None => ctx.players.map(|p| p.connections()).unwrap_or_default(),
    };
    #[expect(clippy::cast_possible_truncation, reason = "`d2f`, as vanilla does")]
    let dist = [delta[0] as f32, delta[1] as f32, delta[2] as f32];
    let body = level_particles_body(force, false, pos, dist, speed, count, ty.id)
        .map_err(|e| Failure::unanswered(&e.to_string()))?;
    let range = if force { 512.0 } else { 32.0 };
    let mut sent = 0;
    for v in &viewers {
        let Some((at, _, _)) = entity_pose(v) else {
            continue;
        };
        // `BlockPos.closerToCenterThan(pos, range)`: the CENTRE of the
        // viewer's block, squared distance strictly below the square.
        let d: f64 = (0..3)
            .map(|i| {
                let c = at[i].floor() + 0.5 - pos[i];
                c * c
            })
            .sum();
        if d < range * range && v.send(ids::play::cb::LEVEL_PARTICLES, &body).is_ok() {
            sent += 1;
        }
    }
    if sent == 0 {
        return Err(particle_failed());
    }
    Ok(vec![Feedback::Success {
        component: particle_success(&namespaced(name_raw)),
        to_admins: true,
    }])
}

// ===========================================================================
// /swing — `SwingCommand`
// ===========================================================================

/// `lambda$swing$0` — `commands.swing.success.single`.
#[must_use]
pub fn swing_single(target: &Nbt) -> Nbt {
    translatable(
        "commands.swing.success.single",
        &[Arg::Component(target.clone())],
    )
}

/// `lambda$swing$1` — `commands.swing.success.multiple`.
#[must_use]
pub fn swing_multiple(count: i32) -> Nbt {
    translatable("commands.swing.success.multiple", &[Arg::Int(count)])
}

/// `SwingCommand.ERROR_NO_LIVING_ENTITY` — `commands.swing.failed.notliving`.
/// Unreachable here — every entity a selector can name on this server is a
/// player, and a player is living — and kept so the reply is gated.
#[must_use]
pub fn swing_not_living() -> Failure {
    Failure::translatable("commands.swing.failed.notliving", &[])
}

/// `SwingCommand.swing`, offsets 0-106: for each target that is a
/// `LivingEntity`, `swing(hand, true)` and count it; none counted is
/// `ERROR_NO_LIVING_ENTITY`; then the single reply (with the first target's
/// name) for a count of one, the multiple otherwise.
///
/// `LivingEntity.swing(hand, true)` is [`crate::entities::swing`]'s swallow
/// and then — `updateSelf` being true — `sendToTrackingPlayersAndSelf`: the
/// `animate` goes to everyone tracking the target AND to the target itself,
/// which a player's own arm swing (`swing(hand)`, `false`) never does. Under
/// the tick lock, because the swallow reads `swingTime`, which the tick loop
/// writes.
fn swing(ctx: &Ctx<'_>, targets: &[Arc<Connection>], hand: u8) -> Outcome {
    let entities = crate::entities::Entities::shared();
    let count = {
        let _tick = crate::broadcast::tick_lock();
        let tracked = ctx
            .players
            .map(|p| crate::status::tracked_players(p, entities))
            .unwrap_or_default();
        let mut count = 0i32;
        for t in targets {
            let id = t.entity_id();
            let Some(swung) = entities.modify(id, |e| crate::entities::swing(e, hand)) else {
                continue;
            };
            count += 1;
            if !swung {
                continue;
            }
            let Ok(body) =
                crate::tracker::animate_body(id, crate::entities::animate_action_for_hand(hand))
            else {
                continue;
            };
            // `ChunkMap.TrackedEntity.sendToTrackingPlayersAndSelf`:
            // `broadcast(packet)` first, then the entity's own connection.
            crate::tracker::Tracker::shared().send_to_tracking_players(
                id,
                &tracked,
                ids::play::cb::ANIMATE,
                &body,
            );
            let _ = t.send(ids::play::cb::ANIMATE, &body);
        }
        count
    };
    if count == 0 {
        return Err(swing_not_living());
    }
    let component = if count == 1 {
        swing_single(&crate::commands::display_name_component(targets[0].name()))
    } else {
        swing_multiple(count)
    };
    Ok(vec![Feedback::Success {
        component,
        to_admins: true,
    }])
}

// ===========================================================================
// /rotate — `RotateCommand`
// ===========================================================================

/// `lambda$rotate$0` — `commands.rotate.success`, the target's display name.
#[must_use]
pub fn rotate_success(target: &Nbt) -> Nbt {
    translatable("commands.rotate.success", &[Arg::Component(target.clone())])
}

/// `RotateCommand.rotate(source, entity, Coordinates)`, offsets 0-94:
///
/// ```text
///   rot = coords.getRotation(source)       // against the SOURCE's rotation
///   y   = coords.isYRelative() ? rot.y - entity.getYRot() : rot.y
///   x   = coords.isXRelative() ? rot.x - entity.getXRot() : rot.x
///   entity.forceSetRotation(y, isYRelative, x, isXRelative)
///   sendSuccess(commands.rotate.success, true)
/// ```
///
/// So a relative axis resolves `~` against the source — the console's is
/// `Vec2.ZERO` — and then subtracts the target's own angle, and the client is
/// sent that difference with the relative flag set. No `wrapDegrees`, unlike
/// `/teleport`. The rotation itself lands in the target's play loop, as a
/// teleport does; see `world_send::command_rotation`.
fn rotate(ctx: &Ctx<'_>, target_raw: &str, rotation_raw: &str) -> Outcome {
    #[expect(clippy::cast_possible_truncation, reason = "`Vec2` holds floats")]
    fn narrow(v: f64) -> f32 {
        v as f32
    }
    // `EntityArgument.getEntity` -> `findSingleEntity`.
    let target = find_entities(ctx, target_raw)?.remove(0);
    let mut r = crate::cmdreader::Reader::new(rotation_raw);
    let Ok([yaw, pitch]) = crate::commandtree::rotation_value(&mut r) else {
        return Err(Failure::unanswered(&format!(
            "rotate rotation {rotation_raw:?} did not re-parse"
        )));
    };
    let ry = narrow(yaw.get(f64::from(ctx.rot[1])));
    let rx = narrow(pitch.get(f64::from(ctx.rot[0])));
    let (_, cur_y, cur_x) = entity_pose(&target).unwrap_or(([0.0; 3], 0.0, 0.0));
    let y = if yaw.relative { ry - cur_y } else { ry };
    let x = if pitch.relative { rx - cur_x } else { rx };
    let relatives = (if yaw.relative { relative::Y_ROT } else { 0 })
        | (if pitch.relative { relative::X_ROT } else { 0 });
    target.queue_teleport(crate::players::PendingTeleport {
        pos: [0.0; 3],
        y_rot: y,
        x_rot: x,
        relatives,
        rotation_only: true,
    });
    Ok(vec![Feedback::Success {
        component: rotate_success(&crate::commands::display_name_component(target.name())),
        to_admins: true,
    }])
}

// ===========================================================================
// /difficulty — `DifficultyCommand`
// ===========================================================================

/// `Difficulty`'s four constants, in ordinal (= id) order, by
/// `getSerializedName()` — which is also the literal each is registered under.
pub const DIFFICULTIES: [&str; 4] = ["peaceful", "easy", "normal", "hard"];

/// `Difficulty.getDisplayName()`: `Component.translatable("options.difficulty."
/// + key)`, `key` being the serialized name.
#[must_use]
pub fn difficulty_display_name(name: &str) -> Nbt {
    translatable(&format!("options.difficulty.{name}"), &[])
}

/// `lambda$register$2`: `commands.difficulty.query` with the display name.
#[must_use]
pub fn difficulty_query(name: &str) -> Nbt {
    translatable(
        "commands.difficulty.query",
        &[Arg::Component(difficulty_display_name(name))],
    )
}

/// `lambda$setDifficulty$0`: `commands.difficulty.success` with the display
/// name.
#[must_use]
pub fn difficulty_success(name: &str) -> Nbt {
    translatable(
        "commands.difficulty.success",
        &[Arg::Component(difficulty_display_name(name))],
    )
}

/// `ERROR_ALREADY_SAME_DIFFICULTY.create(d.getDisplayName())` — a
/// `DynamicCommandExceptionType` whose message is
/// `commands.difficulty.failure` with that one argument.
#[must_use]
pub fn difficulty_failure(name: &str) -> Failure {
    Failure::translatable(
        "commands.difficulty.failure",
        &[Arg::Component(difficulty_display_name(name))],
    )
}

/// The bare-literal executor (`lambda$register$1`):
/// `sendSuccess(query, false)`, returning the id.
fn query_difficulty() -> Vec<Feedback> {
    let current = DIFFICULTIES[usize::try_from(crate::mob::difficulty_id()).unwrap_or(0)];
    vec![Feedback::Success {
        component: difficulty_query(current),
        to_admins: false,
    }]
}

/// `DifficultyCommand.setDifficulty`.
///
/// ```text
///    0-28  d == WorldData.getDifficulty()  -> ERROR_ALREADY_SAME_DIFFICULTY
///   29-32  server.setDifficulty(d, true)
///   35-43  sendSuccess(commands.difficulty.success, true)
/// ```
///
/// `MinecraftServer.setDifficulty(d, forced = true)` skips the lock test,
/// stores `HARD` instead on a hardcore world (this server has none), updates
/// the spawning flags and sends `change_difficulty` to **every** player. The
/// spawning flags are not a separate store here: `crate::mob` reads
/// [`crate::mob::difficulty`] where vanilla reads the flags it derives from it.
fn set_difficulty(ctx: &Ctx<'_>, name: &str) -> Outcome {
    let Some(id) = DIFFICULTIES.iter().position(|d| *d == name) else {
        // Unreachable through the tree: the four literals are these four names.
        return Err(difficulty_failure(name));
    };
    if usize::try_from(crate::mob::difficulty_id()).ok() == Some(id) {
        return Err(difficulty_failure(name));
    }
    crate::mob::set_difficulty(name).map_err(|_| difficulty_failure(name))?;
    if let Some(players) = ctx.players {
        // `getPlayers().forEach(this::sendDifficultyUpdate)`.
        if let Ok(body) = change_difficulty_body(id, false) {
            players.broadcast(ids::play::cb::CHANGE_DIFFICULTY, &body);
        }
    }
    Ok(vec![Feedback::Success {
        component: difficulty_success(name),
        to_admins: true,
    }])
}

/// `ClientboundChangeDifficultyPacket`: `Difficulty.STREAM_CODEC`, which is
/// `ByteBufCodecs.idMapper(byId, getId)` — a varint id — then `BOOL` for
/// `isDifficultyLocked`. A dedicated server's world is unlocked unless
/// something locks it, and nothing on this server can.
///
/// # Errors
/// Never, for a `Vec`.
pub fn change_difficulty_body(id: usize, locked: bool) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    rustmc_proto::varint::write_varint(&mut b, i32::try_from(id).unwrap_or(0))?;
    wire::write_bool(&mut b, locked)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A writer a test can read back after the connection has used it.
    #[derive(Clone, Default)]
    struct Recorder(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Recorder {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("recorder").extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Recorder {
        /// The packet ids written so far, in order.
        fn ids(&self) -> Vec<i32> {
            let bytes = self.0.lock().expect("recorder").clone();
            let mut cur = &bytes[..];
            let mut out = Vec::new();
            while let Ok((id, _)) = rustmc_proto::frame::read_frame(&mut cur, false) {
                out.push(id);
            }
            out
        }
    }

    /// Two players on one registry, each recording what it is sent.
    fn two_players() -> (Players, [Arc<Connection>; 2], [Recorder; 2]) {
        let players = Players::new();
        let (ra, rb) = (Recorder::default(), Recorder::default());
        let a = Arc::new(Connection::new(1, "oracle", Box::new(ra.clone()), None));
        let b = Arc::new(Connection::new(2, "oracle_eye", Box::new(rb.clone()), None));
        players.join(&a);
        players.join(&b);
        (players, [a, b], [ra, rb])
    }

    fn console_run(players: &Players, line: &str) {
        let resolved = crate::commands::tree()
            .resolve(line, Level::Owners)
            .expect("resolves");
        dispatch(&resolved, &Ctx::console(Some(players)))
            .expect("this module's")
            .expect("delivered");
    }

    /// `ServerPlayer.setGameMode` from the console: the target gets
    /// abilities, the tab-list update, the game event, abilities again and
    /// `gameMode.changed`; the other player gets the tab-list update only.
    #[test]
    fn a_console_gamemode_reaches_the_target_in_set_game_modes_order() {
        let (players, [a, _], [ra, rb]) = two_players();
        a.set_game_type(1);
        console_run(&players, "gamemode survival oracle");
        let cb = ids::play::cb::PLAYER_ABILITIES;
        assert_eq!(
            ra.ids(),
            vec![
                cb,
                ids::play::cb::PLAYER_INFO_UPDATE,
                ids::play::cb::GAME_EVENT,
                cb,
                ids::play::cb::SYSTEM_CHAT,
            ]
        );
        assert_eq!(rb.ids(), vec![ids::play::cb::PLAYER_INFO_UPDATE]);
        assert_eq!(a.game_type(), 0);
        // A second run changes nothing and says nothing.
        console_run(&players, "gamemode survival oracle");
        assert_eq!(ra.ids().len(), 5);
    }

    /// `execute as` changes the entity and not the `CommandSource`, so the
    /// self form's reply goes to the console and the target is sent no chat.
    #[test]
    fn execute_as_moves_the_entity_and_not_the_replies() {
        let (players, [a, _], [ra, _]) = two_players();
        a.set_game_type(1);
        console_run(&players, "execute as oracle run gamemode survival");
        assert!(!ra.ids().contains(&ids::play::cb::SYSTEM_CHAT));
        assert!(ra.ids().contains(&ids::play::cb::GAME_EVENT));
    }

    /// A forked run fails in silence; the same failure unforked is one red
    /// line to the player who typed it.
    #[test]
    fn a_forked_failure_is_not_sent() {
        let (players, [a, _], [ra, _]) = two_players();
        let run = |line: &str| {
            let resolved = crate::commands::tree()
                .resolve(line, Level::Owners)
                .expect("resolves");
            let ctx = Ctx::player(&a, Level::Owners, &players, [0.0; 3], [0.0; 2]);
            dispatch(&resolved, &ctx).expect("ours").expect("delivered");
        };
        // 7 points is level 0's whole bar: `ERROR_SET_POINTS_INVALID`.
        run("execute as oracle run xp set oracle 7 points");
        assert!(ra.ids().is_empty(), "forked: nothing");
        run("xp set oracle 7 points");
        assert_eq!(
            ra.ids(),
            vec![ids::play::cb::SYSTEM_CHAT],
            "unforked: one red line"
        );
    }

    /// `tp <targets> <location> <rotation>`: an absolute yaw and a relative
    /// pitch set `X_ROT` and not `Y_ROT`, integer x/z are centred, and the
    /// queued angles are what `performTeleport` hands `teleportTo`.
    #[test]
    fn a_rotation_sets_only_its_relative_bits() {
        let (players, [a, _], _) = two_players();
        console_run(&players, "tp oracle 1 2 3 90 ~5");
        let t = a.take_teleports();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].pos, [1.5, 2.0, 3.5]);
        assert_eq!(t[0].relatives, relative::X_ROT);
        assert!((t[0].y_rot - 90.0).abs() < f32::EPSILON);
        assert!((t[0].x_rot - 5.0).abs() < f32::EPSILON);
        // Without a rotation both rotation bits are relative and the angles
        // are a zero delta.
        console_run(&players, "tp oracle 1 2 3");
        let t = a.take_teleports();
        assert_eq!(t[0].relatives, relative::X_ROT | relative::Y_ROT);
        assert!(t[0].y_rot.abs() < f32::EPSILON && t[0].x_rot.abs() < f32::EPSILON);
    }

    /// Names match case-insensitively, `@a` is the list in join order, and
    /// `@s` from the console is nobody.
    #[test]
    fn player_selectors_resolve_against_the_registry() {
        let (players, _, _) = two_players();
        let ctx = Ctx::console(Some(&players));
        let names = |raw: &str| -> Vec<String> {
            select_players(&ctx, raw)
                .expect("resolvable")
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        };
        assert_eq!(names("ORACLE"), vec!["oracle"]);
        assert_eq!(names("@a"), vec!["oracle", "oracle_eye"]);
        assert!(names("@s").is_empty());
        assert!(select_players(&ctx, "@r")
            .expect_err("unanswered")
            .is_unanswered());
        assert_eq!(
            find_players(&ctx, "nobody").expect_err("none"),
            no_players_found()
        );
    }

    /// `execute` resolves into the command after `run`, and every context a
    /// redirect left behind is kept, in order, with its own arguments — the
    /// `targets` of `execute as` must not shadow a later command's.
    #[test]
    fn an_execute_chain_keeps_each_contexts_arguments() {
        let t = crate::commands::tree();
        let r = t
            .resolve(
                "execute as oracle at @s run tp oracle_eye ~ ~1 ~",
                Level::Owners,
            )
            .expect("resolves");
        assert_eq!(r.path, vec!["teleport", "targets", "location"]);
        assert_eq!(r.arg("targets"), Some("oracle_eye"));
        assert_eq!(r.arg("location"), Some("~ ~1 ~"));
        assert_eq!(
            r.chain,
            vec![
                (
                    vec!["execute", "as", "targets"],
                    vec![("targets", "oracle".to_string())]
                ),
                (
                    vec!["execute", "at", "targets"],
                    vec![("targets", "@s".to_string())]
                ),
                (vec!["execute", "run"], vec![]),
                // `tp` is itself a redirect into `teleport`.
                (vec!["tp"], vec![]),
            ]
        );
        assert!(owns(&r.path));
    }

    /// The plain alias redirects still land where they did, with one
    /// argument-less parent context.
    #[test]
    fn an_alias_redirect_leaves_one_empty_parent() {
        let r = crate::commands::tree()
            .resolve("tell oracle hi", Level::All)
            .expect("resolves");
        assert_eq!(r.path, vec!["msg", "targets", "message"]);
        assert_eq!(r.chain, vec![(vec!["tell"], vec![])]);
    }

    /// `execute` is level 2: a level-0 source is told nothing about it.
    #[test]
    fn execute_is_pruned_below_gamemasters() {
        assert!(crate::commands::tree()
            .resolve("execute run list", Level::All)
            .is_err());
        assert!(crate::commands::tree()
            .resolve("execute run list", Level::Gamemasters)
            .is_ok());
    }

    #[test]
    fn the_difficulty_packet_is_a_varint_id_then_the_lock() {
        assert_eq!(change_difficulty_body(3, false).unwrap(), vec![3, 0]);
        assert_eq!(change_difficulty_body(0, true).unwrap(), vec![0, 1]);
    }

    #[test]
    fn the_admin_line_is_gray_italic_around_the_name_and_the_message() {
        let line = admin_line(
            &Nbt::String(CONSOLE_NAME.into()),
            &translatable("commands.difficulty.success", &[]),
        );
        let Nbt::Compound(m) = &line else {
            panic!("not a compound")
        };
        assert_eq!(m.get("color"), Some(&Nbt::String("gray".into())));
        assert_eq!(m.get("italic"), Some(&Nbt::Byte(1)));
        assert_eq!(
            m.get("translate"),
            Some(&Nbt::String("chat.type.admin".into()))
        );
    }

    #[test]
    fn a_failure_is_one_red_line_around_the_raw_message() {
        let raw = translatable("commands.difficulty.failure", &[]);
        let Nbt::Compound(m) = failure_line(&raw) else {
            panic!("not a compound")
        };
        assert_eq!(m.get("extra"), Some(&Nbt::List(vec![raw])));
        assert_eq!(m.get("color"), Some(&Nbt::String("red".into())));
    }
}
