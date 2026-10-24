//! The command registry: `ServerboundChatCommandPacket` in, `system_chat`
//! out, plus the `commands` packet that tells a client the tree exists.
//!
//! [`crate::commandtree`] holds the *shape* — what a node is, the permission
//! model, the breadth-first numbering, the packet body. This module holds the
//! *content*: which commands exist, who may run them, and what each answers.
//!
//! # What replaced what, 2026-08-06
//!
//! Until this commit there was no registry. There was one `if text != "list"`
//! against the whole command text, no tree, no `commands` packet, and no
//! permission level anywhere in the server. A real client therefore offered
//! no completions, `/list uuids` answered as though it were `/list` or not at
//! all, and there was no concept a plugin could hang a command on.
//!
//! The whole-text match is gone rather than wrapped: [`handle`] resolves
//! against [`tree`] now, and `list uuids` is its own node with its own reply.
//!
//! # What is here
//!
//! **This list was the whole registry on 2026-08-06 and is a fraction of it
//! now.** The roots answered in THIS file are `list`, `seed`, `gamerule`,
//! `version`, `say`, `me`, `msg`/`tell`/`w`, `help`, `worldborder` and
//! `scoreboard`; every root added after them is executed by
//! [`crate::cmdexec`], which gives an executor a source (console or player)
//! and is where `execute` lives. [`tree`] is the one registry for both, and
//! `docs/generated/command-surface.tsv` is the generated list of what it
//! holds — read that, not this paragraph, for a count. What follows is the
//! original description of the first two roots.
//!
//! * `list` — level 0, executable, with the child `uuids`, also executable.
//! * `seed` — level 2 (`GAMEMASTERS`), executable.
//!
//! Three nodes and a root, at the time. That is a small registry and it is a **complete**
//! one: every node declared is dispatchable, every node dispatchable answers
//! with bytes gated against vanilla, and the permission level attached to
//! each is the level vanilla attaches. A fourth command is a `Node` and a
//! reply function, not a redesign.
//!
//! # What is deliberately NOT here
//!
//! Named, because a reader will otherwise assume the opposite of each — and
//! because this project's most expensive defect is a partial implementation
//! that reads complete from the call site.
//!
//! * **Argument nodes.** See [`crate::commandtree`]: [`Node`] has no argument
//!   variant, so an argument command does not compile rather than
//!   mis-serialising. Every command here is literals.
//! * **An unknown command IS answered now — 2026-09-05.** This bullet used to
//!   read "still answered with silence", and it was the most expensive entry
//!   in this list: silence is the observable answer for every typo, every root
//!   this server has not ported and every command a player lacks permission
//!   for, and on the wire it is indistinguishable from a command that ran and
//!   printed nothing. [`failure_components`] is
//!   `Commands.finishParsing`'s two `system_chat` messages, gated byte for
//!   byte against vanilla's own by
//!   `crates/rustmc-server/tests/command_failure_vanilla.rs`.
//!
//!   The argument-level half followed: `Commands.getParseException`'s
//!   "exactly one candidate raised" arm returns that candidate's own
//!   Brigadier exception, and `net.minecraft.commands.BrigadierExceptions`'
//!   twenty-odd translation keys are transcribed beside the readers that
//!   raise them. **72 of the golden's 80 rows are byte-exact, 8 are silent
//!   where vanilla is silent, and none is unrendered.** What is not covered
//!   is the `net.minecraft.commands.arguments` types' own exceptions, and
//!   the ratchet is at zero so the first one is a reviewed change.
//! * **`chat_command_signed`** (packet 8) is a different packet and is not
//!   handled; see `docs/intel/chat-and-commands.md` §2 for why nothing on an
//!   offline server sends it.
//! * **Command spam throttling.** `TickThrottler(20, 20 *
//!   getCommandSpamThresholdSeconds())` and `detectCommandSpam`.
//! * **Suggestions (`command_suggestion`/`command_suggestions`) and command
//!   blocks.** The console source, redirects and an `/execute` subset
//!   (`run`, `as`, `at`, `positioned`) exist now — see [`crate::cmdexec`].
//!
//! # Why the replies are a byte-for-byte affair
//!
//! `chat`'s projection in `tools/oracle-runner` consumes zero bytes, so its
//! `rest` field is a digest of the **whole** packet body. There is no partial
//! credit: a component that renders identically but encodes differently is a
//! divergence. That is why this module builds NBT rather than a string, why
//! it leans on `rustmc_world::nbt`'s Java-hash key ordering, and why every
//! shape below was read off vanilla's own bytes rather than derived.
//!
//! The three replies are gated against **two different** vanilla sources, on
//! purpose:
//!
//! * `/list` — the 539 bytes a real 26.2 server put on a real socket,
//!   recorded 2026-08-05 with `RUSTMC_DUMP_PACKET=chat`. Strongest possible:
//!   it is the wire.
//! * `/list uuids` and `/seed` — bytes produced by vanilla's own
//!   `ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC` in
//!   `ci/commands-golden.sh`. `/list`'s reply needs a live `ServerPlayer` for
//!   `getDisplayName()` and so cannot be built that way; these two need only
//!   static helpers (`Player.getName()` is `Component.literal(profile.name())`
//!   and `Component.translationArg(UUID)` is `Component.literal(uuid.toString())`),
//!   so they can.
//!
//! **The golden corrected a wrong guess before a line of this was written.**
//! An unstyled literal component does **not** encode as `{text: "..."}` — it
//! encodes as a bare `TAG_String`. `commands.list.nameAndId`'s two arguments
//! are therefore plain strings in the `with` list, and the uuid is its
//! *dashed* `UUID.toString()` form, not the four-int array `/list` uses for
//! its hover event. Both would have been written the other way from a
//! reading of `ListPlayersCommand` alone.
//!
//! # The reply shapes, as recorded
//!
//! ```text
//! /list        { with: [ {"":2}, {"":20}, { extra: [dn, {color:gray,text:", "}, dn],
//!                                           text: "" } ],
//!                translate: "commands.list.players" }
//! /list uuids  same, but each element is
//!              { with: ["<name>", "<dashed-uuid>"],
//!                translate: "commands.list.nameAndId" }
//! /seed        { with: [ { with: [ { click_event: {action:"copy_to_clipboard",
//!                                                  value:"<seed>"},
//!                                    color: "green", insertion: "<seed>",
//!                                    text: "<seed>",
//!                                    hover_event: {action:"show_text",
//!                                                  value:{translate:"chat.copy.click"}} } ],
//!                          translate: "chat.square_brackets" } ],
//!                translate: "commands.seed.success" }
//! ```
//!
//! **`commands.list.players` takes three arguments in 26.2, not two**: the
//! online count, the *maximum* player count, and the joined names. A version
//! that took two would produce a component one argument short and a wrong
//! digest, and no amount of reading `ListPlayersCommand` alone would have said
//! so — `format` passes `list.size()` and the joined component, and the max
//! comes from the translation string's own arity.
//!
//! A **primitive** translation argument encodes as a compound whose single key
//! is the empty string. That is the wire form and it is not a guess.
//!
//! The key order in every one of those compounds is `java.util.HashMap`
//! bucket order, which `rustmc_world::nbt` writes; see
//! [`rustmc_data::java_hash_set`]. It is checked against the recorded bytes by
//! this module's own tests rather than assumed.

use crate::cmdreader::{Syntax, SyntaxArgs};
use crate::commandtree::{Level, Node, Parser, Props, Refusal, StringKind, Tree};
use crate::world_send::Session;
// `PlayerList.getMaxPlayers` — `server.properties`' `max-players`. Taken from
// `crate::status` rather than written again here: it reaches the wire twice,
// once in a status ping's JSON and once as a translation argument in this
// reply, and one number with two sources is one number that will eventually
// disagree with itself.
use crate::status::MAX_PLAYERS;
use rustmc_proto::{ids, wire};
use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;
use std::io;

/// The registry.
///
/// **Registration order is wire-visible** and must match vanilla's, because
/// Brigadier's `children` is a `LinkedHashMap` and `enumerateNodes` numbers
/// nodes in that order. `ci/commands-golden.sh` registers
/// `ListPlayersCommand` then `SeedCommand`, in that order, and the golden is
/// vanilla's bytes for it — so swapping these two lines fails the gate rather
/// than silently shipping a different packet.
#[must_use]
pub fn tree() -> &'static Tree {
    static TREE: std::sync::OnceLock<Tree> = std::sync::OnceLock::new();
    TREE.get_or_init(build_tree)
}

/// The registry, built once.
///
/// Separate from [`tree`] so the `OnceLock` has something to call and so a
/// test can build a fresh one without disturbing the shared instance.
fn build_tree() -> Tree {
    Tree {
        roots: vec![
            // `literal("list").executes(...).then(literal("uuids").executes(...))`
            // — no `requires`, so `LEVEL_ALL`.
            Node::leaf("list").then(Node::leaf("uuids")),
            // `literal("seed").requires(hasPermission(LEVEL_GAMEMASTERS))`
            // on a dedicated server. `SeedCommand.register`'s boolean is
            // `CommandSelection.DEDICATED`'s, and passing the other one makes
            // `/seed` level 0 — which is the mistake that would turn the
            // permission golden into a gate that cannot fail.
            Node::leaf("seed").requires(Level::Gamemasters),
            // `GameRuleCommand.register` — the first root this server carries
            // that is not a bare literal tree. See [`GAME_RULES`].
            gamerule_root(),
            // `VersionCommand.register(d, true)`. The boolean is
            // `CommandSelection.DEDICATED.includeDedicated` again, and it does
            // the same thing here as it does for `/seed`: `true` picks
            // `LEVEL_GAMEMASTERS`, `false` picks `LEVEL_ALL`. Passing the
            // wrong one is the mistake that makes a permission golden
            // unable to fail.
            //
            // `ci/commands-golden.sh` deliberately did NOT register this and
            // said why: "a client offered a command that does nothing is worse
            // on the wire than a client not offered it". It registers it now
            // because the executor exists — see [`VERSION_LINES`].
            Node::leaf("version").requires(Level::Gamemasters),
            // `SayCommand.register`: `literal("say").requires(GAMEMASTERS)`
            // then one `MessageArgument` named `message`. The root itself is
            // NOT executable — bare `/say` is a usage error, not an empty
            // announcement.
            say_root(),
            // `EmoteCommands.register`: the same shape with no `requires` and
            // the argument named `action`. The name is wire-visible, so it is
            // not interchangeable with `message`.
            me_root(),
            // `MsgCommand.register` registers THREE roots: `msg`, then `tell`
            // and `w` as pure redirects into it. All three are level 0.
            msg_root(),
            Node::redirect_to("tell", &["msg"]),
            Node::redirect_to("w", &["msg"]),
            help_root(),
            // `WorldBorderCommand.register`. Appended last so that every node
            // index already gated by `ci/commands-golden.sh` keeps the number
            // it has: `enumerateNodes` is breadth-first over insertion order,
            // so inserting a root anywhere but the end renumbers the redirect
            // edge `tell`/`w` carry.
            worldborder_root(),
            // `ScoreboardCommand.register`, PARTIALLY — see
            // [`scoreboard_root`] for exactly which branches and why the rest
            // are absent rather than present and inert.
            scoreboard_root(),
            // Every root from here on is executed by `crate::cmdexec`, which
            // gives it a source — console or player — rather than a socket.
            // Appended, never inserted, for `worldborder_root`'s reason.
            difficulty_root(),
            gamemode_root(),
            teleport_root(),
            // `TeleportCommand.register`'s second root:
            // `literal("tp").requires(LEVEL_GAMEMASTERS).redirect(node)`.
            Node::redirect_to("tp", &["teleport"]).requires(Level::Gamemasters),
            weather_root(),
            experience_root(),
            // `ExperienceCommand.register`'s second root:
            // `literal("xp").requires(LEVEL_GAMEMASTERS).redirect(node)`.
            Node::redirect_to("xp", &["experience"]).requires(Level::Gamemasters),
            defaultgamemode_root(),
            title_root(),
            time_root(),
            execute_root(),
            setblock_root(),
            fill_root(),
            playsound_root(),
            stopsound_root(),
            clone_root(),
            tellraw_root(),
            random_root(),
            rotate_root(),
            swing_root(),
            particle_root(),
            teammsg_root(),
            // `TeamMsgCommand.register`'s second root:
            // `literal("tm").redirect(node)`, no `requires` — level 0.
            Node::redirect_to("tm", &["teammsg"]),
            dialog_root(),
            bossbar_root(),
        ],
    }
}

/// `ExperienceCommand.register` — the whole root:
///
/// ```text
/// experience                requires(LEVEL_GAMEMASTERS)
///   add  target players() amount integer()   points (exec)  levels (exec)
///   set  target players() amount integer(0)  points (exec)  levels (exec)
///   query target player()                    points (exec)  levels (exec)
/// ```
///
/// `amount` is executable as well as its two literals: `/xp add @s 5` is
/// `Type.POINTS` (the `executes` at offset 44, before `then(points)`). `query`'s
/// `target` is not — a query must name what it asks for.
fn experience_root() -> Node {
    fn arg(name: &'static str, registry: &'static str, props: Props) -> Node {
        Node {
            name,
            level: Level::All,
            executable: name == "amount",
            parser: Some(Parser {
                registry,
                props,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    // The root literal first: `ci/command-surface.py` names a root by the
    // first literal leaf call in its builder's body.
    let mut root = Node::leaf("experience").requires(Level::Gamemasters);
    root.executable = false;
    let kinds = |n: Node| n.then(Node::leaf("points")).then(Node::leaf("levels"));
    let players = |single| {
        arg(
            "target",
            "minecraft:entity",
            Props::Entity {
                single,
                players_only: true,
            },
        )
    };
    let mut add = Node::leaf("add");
    add.executable = false;
    let mut set = Node::leaf("set");
    set.executable = false;
    let mut query = Node::leaf("query");
    query.executable = false;
    root.then(add.then(players(false).then(kinds(arg(
        "amount",
        "brigadier:integer",
        Props::Int(None, None),
    )))))
    .then(set.then(players(false).then(kinds(arg(
        "amount",
        "brigadier:integer",
        Props::Int(Some(0), None),
    )))))
    .then(query.then(kinds(players(true))))
}

/// `DefaultGameModeCommands.register`: `literal("defaultgamemode")
/// .requires(LEVEL_GAMEMASTERS)` and one executable
/// `argument("gamemode", GameModeArgument.gameMode())`. The whole root.
fn defaultgamemode_root() -> Node {
    let mut root = Node::leaf("defaultgamemode").requires(Level::Gamemasters);
    root.executable = false;
    root.then(Node {
        name: "gamemode",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:gamemode",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    })
}

/// `TitleCommand.register`:
///
/// ```text
/// title                    requires(LEVEL_GAMEMASTERS)
///   targets  players()
///     clear                                              exec
///     reset                                              exec
///     times  fadeIn time()  stay time()  fadeOut time()  exec (last only)
/// ```
///
/// plus `title`, `subtitle` and `actionbar <title: component>` since
/// `crate::componentarg` (2026-10-01): the whole root now, so
/// `CommandsGolden` registers it with vanilla's own `TitleCommand.register`.
/// A component outside that module's subset is not answered.
fn title_root() -> Node {
    fn arg(name: &'static str, registry: &'static str, props: Props, executable: bool) -> Node {
        Node {
            name,
            level: Level::All,
            executable,
            parser: Some(Parser {
                registry,
                props,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    let time = |name, exec| arg(name, "minecraft:time", Props::TimeMin(0), exec);
    // `title|subtitle|actionbar <title: component>`: the literal is not
    // executable, the component is.
    let text = |name: &'static str| {
        let mut n = Node::leaf(name);
        n.executable = false;
        n.then(arg("title", "minecraft:component", Props::Empty, true))
    };
    let mut root = Node::leaf("title").requires(Level::Gamemasters);
    root.executable = false;
    let mut times = Node::leaf("times");
    times.executable = false;
    let targets = arg(
        "targets",
        "minecraft:entity",
        Props::Entity {
            single: false,
            players_only: true,
        },
        false,
    )
    .then(Node::leaf("clear"))
    .then(Node::leaf("reset"))
    .then(text("title"))
    .then(text("subtitle"))
    .then(text("actionbar"))
    .then(times.then(time("fadeIn", false).then(time("stay", false).then(time("fadeOut", true)))));
    root.then(targets)
}

/// One argument node, at `Level::All` (an argument inherits its root's
/// requirement), with no children.
fn arg_node(name: &'static str, registry: &'static str, props: Props, executable: bool) -> Node {
    Node {
        name,
        level: Level::All,
        executable,
        parser: Some(Parser {
            registry,
            props,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    }
}

/// `RotateCommand.register`, as a declared SUBSET:
///
/// ```text
/// rotate                       requires LEVEL_GAMEMASTERS
///   target  entity (single)
///     rotation  rotation       exec
/// ```
///
/// Absent: `target facing <location>` and `target facing entity <entity>
/// [<anchor>]`, which go through `LookAt.perform` and on a player send
/// `player_look_at` rather than `player_rotation` — a packet no executor
/// here writes. `CommandsGolden.registerRotate` builds the same subset.
fn rotate_root() -> Node {
    let mut root = Node::leaf("rotate").requires(Level::Gamemasters);
    root.executable = false;
    root.then(
        arg_node(
            "target",
            "minecraft:entity",
            Props::Entity {
                single: true,
                players_only: false,
            },
            false,
        )
        .then(arg_node(
            "rotation",
            "minecraft:rotation",
            Props::Empty,
            true,
        )),
    )
}

/// `SwingCommand.register`, whole:
///
/// ```text
/// swing                        requires LEVEL_GAMEMASTERS, exec (@s, main hand)
///   targets  entities          exec (main hand)
///     mainhand                 exec
///     offhand                  exec
/// ```
fn swing_root() -> Node {
    let mut root = Node::leaf("swing").requires(Level::Gamemasters);
    root.executable = true;
    root.then(
        arg_node(
            "targets",
            "minecraft:entity",
            Props::Entity {
                single: false,
                players_only: false,
            },
            true,
        )
        .then(Node::leaf("mainhand"))
        .then(Node::leaf("offhand")),
    )
}

/// `ParticleCommand.register`, whole:
///
/// ```text
/// particle                       requires LEVEL_GAMEMASTERS
///   name  particle               exec (at the source's position)
///     pos  vec3                  exec
///       delta  vec3(false)
///         speed  float(min 0)
///           count  integer(min 0)        exec
///             force                      exec
///               viewers  players         exec
///             normal                     exec
///               viewers  players         exec
/// ```
///
/// `vec3(false)` differs from `vec3()` only in the value — no
/// `centerCorrect` — which `Vec3Argument`'s serialiser does not write, so the
/// two declare identically.
fn particle_root() -> Node {
    let viewers = || {
        arg_node(
            "viewers",
            "minecraft:entity",
            Props::Entity {
                single: false,
                players_only: true,
            },
            true,
        )
    };
    let mut root = Node::leaf("particle").requires(Level::Gamemasters);
    root.executable = false;
    root.then(
        arg_node("name", "minecraft:particle", Props::Empty, true).then(
            arg_node("pos", "minecraft:vec3", Props::Empty, true).then(
                arg_node("delta", "minecraft:vec3", Props::Empty, false).then(
                    arg_node(
                        "speed",
                        "brigadier:float",
                        Props::Float(Some(0.0), None),
                        false,
                    )
                    .then(
                        arg_node(
                            "count",
                            "brigadier:integer",
                            Props::Int(Some(0), None),
                            true,
                        )
                        .then(Node::leaf("force").then(viewers()))
                        .then(Node::leaf("normal").then(viewers())),
                    ),
                ),
            ),
        ),
    )
}

/// `DialogCommand.register`, as a declared SUBSET:
///
/// ```text
/// dialog                       requires LEVEL_GAMEMASTERS
///   clear
///     targets  players         exec
/// ```
///
/// Absent: `show <targets> <dialog>`, whose `ResourceOrIdArgument.dialog`
/// reads either a `minecraft:dialog` registry key or an inline dialog in
/// SNBT, and whose packet carries the whole dialog — none of which this
/// server holds. `CommandsGolden.registerDialog` builds the same subset.
fn dialog_root() -> Node {
    let mut root = Node::leaf("dialog").requires(Level::Gamemasters);
    root.executable = false;
    let mut clear = Node::leaf("clear");
    clear.executable = false;
    root.then(clear.then(arg_node(
        "targets",
        "minecraft:entity",
        Props::Entity {
            single: false,
            players_only: true,
        },
        true,
    )))
}

/// `BossBarCommands.register`, whole:
///
/// ```text
/// bossbar                              requires LEVEL_GAMEMASTERS
///   add     id <resource_location>
///             name <component>         exec
///   remove  id* exec
///   list    exec
///   set     id*
///             name   name <component>  exec
///             color  pink|blue|red|green|yellow|purple|white    exec each
///             style  progress|notched_6|notched_10|notched_12|notched_20  exec each
///             value  value <integer min 0>   exec
///             max    max <integer min 1>     exec
///             visible  visible <bool>        exec
///             players  exec
///               targets <players>      exec
///   get     id*
///             value|max|visible|players  exec each
/// ```
///
/// `id*` is `IdentifierArgument.id()` with `.suggests(SUGGEST_BOSS_BAR)`, a
/// provider `SuggestionProviders` does not name, so the packet declares
/// `minecraft:ask_server` for it.
fn bossbar_root() -> Node {
    let id = |suggests: bool| Node {
        name: "id",
        level: Level::All,
        executable: false,
        parser: Some(Parser {
            registry: "minecraft:resource_location",
            props: Props::Empty,
            suggestions: suggests.then_some("minecraft:ask_server"),
        }),
        children: Vec::new(),
        redirect: None,
    };
    let lit = |name: &'static str, exec: bool| {
        let mut n = Node::leaf(name);
        n.executable = exec;
        n
    };
    let mut set = id(true).then(lit("name", false).then(arg_node(
        "name",
        "minecraft:component",
        Props::Empty,
        true,
    )));
    let mut color = lit("color", false);
    for c in crate::bossbar::COLORS {
        color = color.then(lit(c, true));
    }
    let mut style = lit("style", false);
    for o in crate::bossbar::OVERLAYS {
        style = style.then(lit(o, true));
    }
    set = set
        .then(color)
        .then(style)
        .then(lit("value", false).then(arg_node(
            "value",
            "brigadier:integer",
            Props::Int(Some(0), None),
            true,
        )))
        .then(lit("max", false).then(arg_node(
            "max",
            "brigadier:integer",
            Props::Int(Some(1), None),
            true,
        )))
        .then(lit("visible", false).then(arg_node("visible", "brigadier:bool", Props::Empty, true)))
        .then(lit("players", true).then(arg_node(
            "targets",
            "minecraft:entity",
            Props::Entity {
                single: false,
                players_only: true,
            },
            true,
        )));
    let get = id(true)
        .then(lit("value", true))
        .then(lit("max", true))
        .then(lit("visible", true))
        .then(lit("players", true));
    let mut add_id = id(false);
    add_id = add_id.then(arg_node("name", "minecraft:component", Props::Empty, true));
    let mut remove_id = id(true);
    remove_id.executable = true;
    let mut root = Node::leaf("bossbar").requires(Level::Gamemasters);
    root.executable = false;
    root.then(lit("add", false).then(add_id))
        .then(lit("remove", false).then(remove_id))
        .then(lit("list", true))
        .then(lit("set", false).then(set))
        .then(lit("get", false).then(get))
}

/// `RandomCommand.register`, as a declared SUBSET — the level-0 half:
///
/// ```text
/// random                     (no requires: level 0)
///   value  range int_range   exec
///   roll   range int_range   exec
/// ```
///
/// Absent: each `range`'s `sequence` child and the whole `reset` branch, all
/// `requires(LEVEL_GAMEMASTERS)` and all about named random sequences this
/// server does not keep. **A level-0 player is told exactly vanilla's tree for
/// this root**, because `fillUsableCommands` prunes those level-2 nodes from
/// vanilla's packet too. `CommandsGolden.registerRandom` builds the subset.
fn random_root() -> Node {
    let range = || Node {
        name: "range",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:int_range",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    let mut root = Node::leaf("random");
    root.executable = false;
    let mut value = Node::leaf("value");
    value.executable = false;
    let mut roll = Node::leaf("roll");
    roll.executable = false;
    root.then(value.then(range())).then(roll.then(range()))
}

/// `TellRawCommand.register`: `tellraw <targets: players()> <message:
/// component>`, only the component executable. The component is read for the
/// subset `crate::componentarg` rebuilds; a message outside it is not
/// answered.
fn tellraw_root() -> Node {
    let mut root = Node::leaf("tellraw").requires(Level::Gamemasters);
    root.executable = false;
    root.then(Node {
        name: "targets",
        level: Level::All,
        executable: false,
        parser: Some(Parser {
            registry: "minecraft:entity",
            props: Props::Entity {
                single: false,
                players_only: true,
            },
            suggestions: None,
        }),
        children: vec![Node {
            name: "message",
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:component",
                props: Props::Empty,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }],
        redirect: None,
    })
}

/// `CloneCommands.register`, as a declared SUBSET — the same-level forms
/// without a block predicate:
///
/// ```text
/// clone begin end destination                              exec
///   replace | masked                                       exec
///     force | move | normal                                exec
///   strict                                                 exec
///     replace | masked                                     exec
///       force | move | normal                              exec
/// ```
///
/// Absent: `filtered <filter>` (a `minecraft:block_predicate`), and `from
/// <sourceDimension>` / `to <targetDimension>` (a `minecraft:dimension` this
/// one-level server cannot name a second of). `CommandsGolden.registerClone`
/// builds the same subset.
fn clone_root() -> Node {
    fn pos(name: &'static str, executable: bool, children: Vec<Node>) -> Node {
        Node {
            name,
            level: Level::All,
            executable,
            parser: Some(Parser {
                registry: "minecraft:block_pos",
                props: Props::Empty,
                suggestions: None,
            }),
            children,
            redirect: None,
        }
    }
    // The root literal first: `ci/command-surface.py` names a root by the
    // first literal leaf call in its builder's body.
    let mut root = Node::leaf("clone").requires(Level::Gamemasters);
    root.executable = false;
    let modes = |n: Node| {
        n.then(Node::leaf("force"))
            .then(Node::leaf("move"))
            .then(Node::leaf("normal"))
    };
    let masks = |n: Node| {
        n.then(modes(Node::leaf("replace")))
            .then(modes(Node::leaf("masked")))
    };
    let destination = masks(pos("destination", true, Vec::new())).then(masks(Node::leaf("strict")));
    root.then(pos(
        "begin",
        false,
        vec![pos("end", false, vec![destination])],
    ))
}

/// `StopSoundCommand.register` — the whole root: `targets` (players,
/// executable), then `*` (not executable) and one executable literal per
/// `SoundSource`, each with an executable `sound` carrying the
/// `minecraft:available_sounds` suggestions.
fn stopsound_root() -> Node {
    fn sound() -> Node {
        Node {
            name: "sound",
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:resource_location",
                props: Props::Empty,
                suggestions: Some("minecraft:available_sounds"),
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    let mut root = Node::leaf("stopsound").requires(Level::Gamemasters);
    root.executable = false;
    let mut any = Node::leaf("*");
    any.executable = false;
    let mut targets = Node {
        name: "targets",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:entity",
            props: Props::Entity {
                single: false,
                players_only: true,
            },
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    }
    .then(any.then(sound()));
    for source in crate::cmdexec::SOUND_SOURCES {
        targets = targets.then(Node::leaf(source).then(sound()));
    }
    root.then(targets)
}

/// `PlaySoundCommand.register` — the whole root. `sound` is a
/// `minecraft:resource_location` with the `minecraft:available_sounds`
/// suggestion provider (flag 16 on the wire), executable; under it one
/// executable literal per `SoundSource`, each carrying the same executable
/// chain `targets` -> `pos` -> `volume` (>= 0) -> `pitch` (0..2) ->
/// `minVolume` (0..1).
fn playsound_root() -> Node {
    fn arg(name: &'static str, registry: &'static str, props: Props) -> Node {
        Node {
            name,
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry,
                props,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    let mut root = Node::leaf("playsound").requires(Level::Gamemasters);
    root.executable = false;
    let mut sound = arg("sound", "minecraft:resource_location", Props::Empty);
    if let Some(p) = sound.parser.as_mut() {
        p.suggestions = Some("minecraft:available_sounds");
    }
    for source in crate::cmdexec::SOUND_SOURCES {
        let chain = arg(
            "targets",
            "minecraft:entity",
            Props::Entity {
                single: false,
                players_only: true,
            },
        )
        .then(
            arg("pos", "minecraft:vec3", Props::Empty).then(
                arg("volume", "brigadier:float", Props::Float(Some(0.0), None)).then(
                    arg(
                        "pitch",
                        "brigadier:float",
                        Props::Float(Some(0.0), Some(2.0)),
                    )
                    .then(arg(
                        "minVolume",
                        "brigadier:float",
                        Props::Float(Some(0.0), Some(1.0)),
                    )),
                ),
            ),
        );
        sound = sound.then(Node::leaf(source).then(chain));
    }
    root.then(sound)
}

/// `FillCommand.register`, as a declared SUBSET:
///
/// ```text
/// fill  from block_pos  to block_pos  block block_state   exec
///         outline | hollow | destroy | strict | replace | keep   exec
/// ```
///
/// Absent: `replace <filter>`, whose `minecraft:block_predicate` (block tags,
/// state predicates, NBT) this server does not parse. `replace` alone is
/// present and executable, as it is in vanilla. `CommandsGolden.registerFill`
/// builds the same subset with vanilla's argument types.
fn fill_root() -> Node {
    fn pos(name: &'static str, children: Vec<Node>) -> Node {
        Node {
            name,
            level: Level::All,
            executable: false,
            parser: Some(Parser {
                registry: "minecraft:block_pos",
                props: Props::Empty,
                suggestions: None,
            }),
            children,
            redirect: None,
        }
    }
    let mut root = Node::leaf("fill").requires(Level::Gamemasters);
    root.executable = false;
    let mut block = Node {
        name: "block",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:block_state",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    for mode in ["outline", "hollow", "destroy", "strict", "replace", "keep"] {
        block = block.then(Node::leaf(mode));
    }
    root.then(pos("from", vec![pos("to", vec![block])]))
}

/// `SetBlockCommand.register` — the whole root: `setblock <pos> <block>`,
/// executable, with four executable mode literals after it. `pos` is a
/// `minecraft:block_pos` and is not itself executable. Registered in
/// `CommandsGolden` with vanilla's own `SetBlockCommand.register`.
fn setblock_root() -> Node {
    let mut root = Node::leaf("setblock").requires(Level::Gamemasters);
    root.executable = false;
    let mut block = Node {
        name: "block",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:block_state",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    for mode in ["destroy", "keep", "replace", "strict"] {
        block = block.then(Node::leaf(mode));
    }
    root.then(Node {
        name: "pos",
        level: Level::All,
        executable: false,
        parser: Some(Parser {
            registry: "minecraft:block_pos",
            props: Props::Empty,
            suggestions: None,
        }),
        children: vec![block],
        redirect: None,
    })
}

/// `ExecuteCommand.register`, as a declared SUBSET — the four modifiers that
/// change WHO and WHERE, and `run`:
///
/// ```text
/// execute                          requires(LEVEL_GAMEMASTERS)
///   run                            redirect -> the root
///   as  targets entities()         fork -> execute
///   at  targets entities()         fork -> execute
///   positioned
///     pos vec3()                   redirect -> execute
///     as  targets entities()       fork -> execute
/// ```
///
/// Absent: `if`/`unless`, `store`, `rotated`, `facing`, `align`, `anchored`,
/// `in`, `summon`, `on`, and `positioned over`. Each needs an argument type or
/// a world query this server does not have (block predicates, scores,
/// NBT paths, swizzles, anchors, dimensions, heightmaps). Registered by hand
/// in `CommandsGolden.registerExecute` in the same shape, with vanilla's own
/// `fork`/`redirect` targets.
fn execute_root() -> Node {
    fn redirecting(name: &'static str, registry: &'static str, props: Props) -> Node {
        Node {
            name,
            level: Level::All,
            executable: false,
            parser: Some(Parser {
                registry,
                props,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: Some(&["execute"]),
        }
    }
    fn not_executable(name: &'static str) -> Node {
        let mut n = Node::leaf(name);
        n.executable = false;
        n
    }
    let targets = || {
        redirecting(
            "targets",
            "minecraft:entity",
            Props::Entity {
                single: false,
                players_only: false,
            },
        )
    };
    let mut root = Node::leaf("execute").requires(Level::Gamemasters);
    root.executable = false;
    root.then(Node::redirect_to("run", &[]))
        .then(not_executable("as").then(targets()))
        .then(not_executable("at").then(targets()))
        .then(
            not_executable("positioned")
                .then(redirecting("pos", "minecraft:vec3", Props::Empty))
                .then(not_executable("as").then(targets())),
        )
}

/// `TimeCommand.register`, as a declared SUBSET of the default-clock half:
///
/// ```text
/// time                   requires(LEVEL_GAMEMASTERS)
///   set   time time(0)                      exec
///   add   time time(Integer.MIN_VALUE)      exec
///   query time                              exec
///         gametime                          exec
/// ```
///
/// Absent, one reason each: `set <timemarker>` resolves a clock time marker
/// (`day`, `noon`, ...) through `ServerClockManager`'s marker table, which
/// this server does not load; `pause`, `resume` and `rate` change a clock's
/// paused flag and rate, which this server's one day-time model does not
/// have (it has `advance_time` only); `query <timeline>` and `of <clock>`
/// take `minecraft:resource` arguments over the `timeline` and `world_clock`
/// registries. `CommandsGolden.registerTime` builds the same subset.
fn time_root() -> Node {
    fn time(min: i32) -> Node {
        Node {
            name: "time",
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:time",
                props: Props::TimeMin(min),
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    fn not_executable(name: &'static str) -> Node {
        let mut n = Node::leaf(name);
        n.executable = false;
        n
    }
    let mut root = Node::leaf("time").requires(Level::Gamemasters);
    root.executable = false;
    root.then(not_executable("set").then(time(0)))
        .then(not_executable("add").then(time(i32::MIN)))
        .then(
            not_executable("query")
                .then(Node::leaf("time"))
                .then(Node::leaf("gametime")),
        )
}

/// `WeatherCommand.register`: `literal("weather").requires(LEVEL_GAMEMASTERS)`
/// with three executable literals — `clear`, `rain`, `thunder` — each with one
/// executable `duration` argument, `TimeArgument.time(1)`. The root itself has
/// no `executes`. The whole root, so `CommandsGolden` registers it with
/// vanilla's own `WeatherCommand.register`.
fn weather_root() -> Node {
    let mut root = Node::leaf("weather").requires(Level::Gamemasters);
    root.executable = false;
    for arm in ["clear", "rain", "thunder"] {
        root = root.then(Node::leaf(arm).then(Node {
            name: "duration",
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:time",
                props: Props::TimeMin(1),
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }));
    }
    root
}

/// `TeleportCommand.register`, as a declared SUBSET:
///
/// ```text
/// teleport                 requires(LEVEL_GAMEMASTERS), not executable
///   location     vec3                         exec
///   destination  entity (single)              exec
///   targets      entities                     not executable
///     location     vec3                       exec
///     destination  entity (single)            exec
/// ```
///
/// with `targets location rotation` (executable) under `location`.
///
/// Absent: `targets location facing ...`, whose `LookAt` ends in a
/// `ClientboundPlayerLookAtPacket` this server has no writer for. A node
/// offered and not answered is worse than a node not offered, so the subset is
/// registered by hand in `CommandsGolden.registerTeleport` exactly as the
/// scoreboard subset is.
fn teleport_root() -> Node {
    fn arg(name: &'static str, registry: &'static str, props: Props, executable: bool) -> Node {
        Node {
            name,
            level: Level::All,
            executable,
            parser: Some(Parser {
                registry,
                props,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    let entity = |name, single, exec| {
        arg(
            name,
            "minecraft:entity",
            Props::Entity {
                single,
                players_only: false,
            },
            exec,
        )
    };
    let mut root = Node::leaf("teleport").requires(Level::Gamemasters);
    root.executable = false;
    root.then(arg("location", "minecraft:vec3", Props::Empty, true))
        .then(entity("destination", true, true))
        .then(
            entity("targets", false, false)
                .then(
                    arg("location", "minecraft:vec3", Props::Empty, true).then(arg(
                        "rotation",
                        "minecraft:rotation",
                        Props::Empty,
                        true,
                    )),
                )
                .then(entity("destination", true, true)),
        )
}

/// `GameModeCommand.register`: `literal("gamemode").requires(PERMISSION_CHECK)`
/// — `Require(COMMANDS_GAMEMASTER)`, level 2 — then
/// `argument("gamemode", GameModeArgument.gameMode())`, executable, then
/// `argument("target", EntityArgument.players())`, executable. The literal
/// itself has no `executes`. All three nodes are vanilla's whole root.
fn gamemode_root() -> Node {
    let mut root = Node::leaf("gamemode").requires(Level::Gamemasters);
    root.executable = false;
    let target = Node {
        name: "target",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:entity",
            props: Props::Entity {
                single: false,
                players_only: true,
            },
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    root.then(Node {
        name: "gamemode",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:gamemode",
            props: Props::Empty,
            suggestions: None,
        }),
        children: vec![target],
        redirect: None,
    })
}

/// `DifficultyCommand.register`: `literal("difficulty")`, one executable
/// literal per `Difficulty` in ordinal order (`getSerializedName()`), then
/// `requires(LEVEL_GAMEMASTERS)` and an `executes` on the root itself — the
/// query. Every node is a literal, so the whole root is portable as it
/// stands, which is why it is registered with vanilla's own
/// `DifficultyCommand.register` in `CommandsGolden` rather than rebuilt.
fn difficulty_root() -> Node {
    let mut root = Node::leaf("difficulty").requires(Level::Gamemasters);
    for name in crate::cmdexec::DIFFICULTIES {
        root = root.then(Node::leaf(name));
    }
    root
}

/// `/scoreboard`, the seven nodes of it this server can answer.
///
/// # This root is a SUBSET and that is a deliberate, declared thing
///
/// `ScoreboardCommand.register` is ~1,100 bytes of builder calls across
/// `objectives` (list/add/modify/remove/setdisplay) and `players`
/// (list/set/get/add/remove/reset/enable/display/operation), plus number
/// formats and render types. What is here is:
///
/// ```text
/// scoreboard              requires(LEVEL_GAMEMASTERS), NOT executable
///   objectives            NOT executable
///     list                                                        exec
///     add       objective  brigadier:string word
///                 criteria  minecraft:objective_criteria          exec
///     remove    objective  minecraft:objective                    exec
///     setdisplay slot      minecraft:scoreboard_slot              exec
///                 objective minecraft:objective                   exec
/// ```
///
/// **Every node offered is executable and answers with vanilla's own bytes.**
/// That is the bar `ci/commands-golden.sh`'s own header sets — "a client
/// offered a command that does nothing is worse on the wire than a client not
/// offered it" — and it is why the absent branches are absent rather than
/// registered and silent.
///
/// What is missing, and the one reason each:
///
/// * **`objectives add <objective> <criteria> <displayName>`** and
///   **`objectives modify … displayname`** take `minecraft:component`, which
///   is `ComponentArgument.textComponent(ctx)` — a whole SNBT-and-registry
///   parse this server has no reader for. It is the same class as
///   `minecraft:block_state`: the registry decides the accept.
/// * **`objectives modify … rendertype` and `… numberformat`** are
///   `createRenderTypeModify()` and `addNumberFormats(...)`, whose effects
///   reach the wire inside `ClientboundSetObjectivePacket`'s optional number
///   format — a field `crate::scoreboard` writes as absent.
/// * **every `players` branch** takes `minecraft:score_holder`, which is
///   `EntitySelectorParser` plus a wildcard and a name form;
///   `crate::commandtree::entity_selector` refuses an options block, so a
///   `score_holder` node would offer completions for input this server cannot
///   read.
///
/// `CommandsGolden.registerScoreboard` builds this same subtree with vanilla's
/// own builders, exactly as `registerGameRules` does for `/gamerule`, so the
/// `commands` packet is still compared byte for byte.
fn scoreboard_root() -> Node {
    fn not_executable(name: &'static str) -> Node {
        let mut n = Node::leaf(name);
        n.executable = false;
        n
    }
    fn arg(name: &'static str, registry: &'static str, executable: bool) -> Node {
        Node {
            name,
            level: Level::All,
            executable,
            parser: Some(Parser {
                registry,
                props: Props::Empty,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }

    let mut root = Node::leaf("scoreboard").requires(Level::Gamemasters);
    root.executable = false;

    // `objectives add` names its first argument `objective` and parses it as
    // `StringArgumentType.word()` — NOT `ObjectiveArgument`, because the
    // objective does not exist yet and `ObjectiveArgument`'s suggestions list
    // the ones that do. The `remove` and `setdisplay` nodes carry the same
    // argument NAME with the other parser, and an argument name is
    // wire-visible, so the two are not interchangeable.
    let mut add_objective = Node {
        name: "objective",
        level: Level::All,
        executable: false,
        parser: Some(Parser {
            registry: "brigadier:string",
            props: Props::Str(StringKind::Word),
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    add_objective = add_objective.then(arg("criteria", "minecraft:objective_criteria", true));

    let objectives = not_executable("objectives")
        .then(Node::leaf("list"))
        .then(not_executable("add").then(add_objective))
        .then(not_executable("remove").then(arg("objective", "minecraft:objective", true)))
        .then(not_executable("setdisplay").then(
            arg("slot", "minecraft:scoreboard_slot", true).then(arg(
                "objective",
                "minecraft:objective",
                true,
            )),
        ));
    root = root.then(objectives);
    root
}

/// `/worldborder` — `WorldBorderCommand.register`, transcribed from the
/// pinned jar's `register` method offsets 0..329.
///
/// # The shape, and the four constants a reading would guess wrong
///
/// ```text
/// worldborder            requires(LEVEL_GAMEMASTERS), NOT executable
///   add     distance  brigadier:double [-5.9999968E7, 5.9999968E7]  exec
///             time    minecraft:time min=0                          exec
///   set     distance  brigadier:double [-5.9999968E7, 5.9999968E7]  exec
///             time    minecraft:time min=0                          exec
///   center  pos       minecraft:vec2                                exec
///   damage  amount    damagePerBlock  brigadier:float min=0         exec
///           buffer    distance        brigadier:float min=0         exec
///   get                                                             exec
///   warning distance  distance  brigadier:integer min=0             exec
///           time      time      minecraft:time min=0                exec
/// ```
///
/// * **the double bounds are `WorldBorder.MAX_SIZE` and its negation**, not
///   the border's own `absoluteMaxSize` and not `MAX_CENTER_COORDINATE`
///   (offsets 25/28 and 84/87, `ldc2_w -5.9999968E7d` / `5.9999968E7d`). A
///   negative bound on a *distance* is not a typo — `/worldborder add -20`
///   shrinks — and `set` carries the same negative minimum even though a
///   negative absolute size is rejected later by the executor, not the parser.
/// * **`damage amount` and `damage buffer` are two literals under one
///   `damage`, in that order** (offsets 171 and 202, with the three
///   `LiteralArgumentBuilder.then` at 224/227/230 attaching buffer, then
///   damage). Their argument names differ — `damagePerBlock` and `distance` —
///   and an argument name is wire-visible.
/// * **`warning` has children `distance` then `time`**, and `warning distance`
///   nests a *second* literal-then-argument pair whose argument is also called
///   `distance`. The inner one is `brigadier:integer` with min 0 and no
///   maximum (offset 267 `iconst_0`, `IntegerArgumentType.integer(I)`), while
///   the two `add`/`set` distances are doubles.
/// * **only the root is not executable.** `get` is (offset 246,
///   `LiteralArgumentBuilder.executes`); every argument leaf is; and the
///   intermediate literals `add`, `set`, `center`, `damage`, `amount`,
///   `buffer`, `warning` are not, because `register` calls `executes` on none
///   of them.
fn worldborder_root() -> Node {
    /// `argument(name, DoubleArgumentType.doubleArg(-MAX_SIZE, MAX_SIZE))`.
    fn distance_double(name: &'static str) -> Node {
        Node {
            name,
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "brigadier:double",
                props: Props::Double(
                    Some(-crate::worldborder::MAX_SIZE),
                    Some(crate::worldborder::MAX_SIZE),
                ),
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    /// `argument("time", TimeArgument.time(0))`.
    fn time_arg() -> Node {
        Node {
            name: "time",
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:time",
                props: Props::TimeMin(0),
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    /// `argument(name, FloatArgumentType.floatArg(0.0F))` — a minimum and no
    /// maximum, which is the one-argument factory and writes flag byte 1.
    fn float_min_zero(name: &'static str) -> Node {
        Node {
            name,
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "brigadier:float",
                props: Props::Float(Some(0.0), None),
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }
    fn not_executable(name: &'static str) -> Node {
        let mut n = Node::leaf(name);
        n.executable = false;
        n
    }

    let mut root = Node::leaf("worldborder").requires(Level::Gamemasters);
    root.executable = false;

    root = root.then(not_executable("add").then(distance_double("distance").then(time_arg())));
    root = root.then(not_executable("set").then(distance_double("distance").then(time_arg())));
    root = root.then(not_executable("center").then(Node {
        name: "pos",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:vec2",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    }));
    root = root.then(
        not_executable("damage")
            .then(not_executable("amount").then(float_min_zero("damagePerBlock")))
            .then(not_executable("buffer").then(float_min_zero("distance"))),
    );
    root = root.then(Node::leaf("get"));
    root = root.then(
        not_executable("warning")
            .then(not_executable("distance").then(Node {
                name: "distance",
                level: Level::All,
                executable: true,
                parser: Some(Parser {
                    registry: "brigadier:integer",
                    props: Props::Int(Some(0), None),
                    suggestions: None,
                }),
                children: Vec::new(),
                redirect: None,
            }))
            .then(not_executable("time").then(time_arg())),
    );
    root
}

/// The value a rule's `/gamerule` argument node accepts.
///
/// Two variants and not fifty-eight: `GameRuleCommand$1.visitBoolean` and
/// `visitInteger` are the only two arms of `GameRuleTypeVisitor` the jar
/// declares, so the whole 233-node subtree is one of these per rule.
#[derive(Debug, Clone, Copy)]
enum RuleArg {
    /// `BoolArgumentType.bool()` — no properties on the wire.
    Bool,
    /// `IntegerArgumentType.integer(min[, max])`, with vanilla's own declared
    /// bounds. `None` is the type's sentinel and is what makes the flag byte
    /// omit that bound; it is not "no bound was recorded".
    Int(Option<i32>, Option<i32>),
}

/// Every rule `/gamerule` offers, with both names it registers for each and
/// its value argument's type and bounds.
///
/// # The order here is OURS, and that is not a shortcut
///
/// `GameRuleCommand.register` builds `new GameRules(ctx.enabledFeatures())`
/// and walks it with `visitGameRuleTypes`, so vanilla's child order is the
/// **registry's** iteration order — and that iteration **is not stable across
/// JVM runs**. Measured 2026-09-05 over sixteen runs of the same dumper
/// against the same jar: fifteen produced one order and one produced another,
/// 160 of 2,085 nodes permuted, all of them here and nothing else in the tree
/// moving. Two vanilla dedicated servers on this jar can send two different
/// `ClientboundCommandsPacket` byte sequences and both are correct.
///
/// So there is no vanilla order to reproduce. This table is in
/// `crate::gamerules::DEFAULTS`' order, which is the jar's `<clinit>`
/// registration order: a real vanilla order, stable, and already transcribed
/// and gated. `ci/commands-golden.sh` registers the same 58 rules in the same
/// order through vanilla's own builders and compares bytes, so a wrong bound,
/// a wrong type or a missing rule is still a red gate — what it cannot gate,
/// because vanilla has no answer to gate against, is the order.
/// `ci/command-tree-compare.py` is where that measurement lives.
///
/// # 58 here against 59 in `DEFAULTS`, and it is not an error in either
///
/// `max_minecart_speed` is in the game-rule registry and **not** in a
/// dispatcher built with `FeatureFlags.VANILLA_SET`, because
/// `new GameRules(enabledFeatures)` filters by feature flag. So a vanilla
/// dedicated server has the rule and does not offer it on `/gamerule`. Every
/// other one of `DEFAULTS`' 59 names appears in vanilla's own subtree, which
/// is a cross-check on both tables that neither had before.
///
/// Each rule is registered **twice**: bare and `minecraft:`-prefixed. That is
/// 116 children, 232 nodes under the root, 233 with it.
const GAME_RULES: [(&str, &str, RuleArg); 58] = [
    ("advance_time", "minecraft:advance_time", RuleArg::Bool),
    (
        "advance_weather",
        "minecraft:advance_weather",
        RuleArg::Bool,
    ),
    (
        "allow_entering_nether_using_portals",
        "minecraft:allow_entering_nether_using_portals",
        RuleArg::Bool,
    ),
    ("block_drops", "minecraft:block_drops", RuleArg::Bool),
    (
        "block_explosion_drop_decay",
        "minecraft:block_explosion_drop_decay",
        RuleArg::Bool,
    ),
    (
        "command_blocks_work",
        "minecraft:command_blocks_work",
        RuleArg::Bool,
    ),
    (
        "command_block_output",
        "minecraft:command_block_output",
        RuleArg::Bool,
    ),
    (
        "drowning_damage",
        "minecraft:drowning_damage",
        RuleArg::Bool,
    ),
    (
        "elytra_movement_check",
        "minecraft:elytra_movement_check",
        RuleArg::Bool,
    ),
    (
        "ender_pearls_vanish_on_death",
        "minecraft:ender_pearls_vanish_on_death",
        RuleArg::Bool,
    ),
    ("entity_drops", "minecraft:entity_drops", RuleArg::Bool),
    ("fall_damage", "minecraft:fall_damage", RuleArg::Bool),
    ("fire_damage", "minecraft:fire_damage", RuleArg::Bool),
    (
        "fire_spread_radius_around_player",
        "minecraft:fire_spread_radius_around_player",
        RuleArg::Int(Some(-1), None),
    ),
    (
        "forgive_dead_players",
        "minecraft:forgive_dead_players",
        RuleArg::Bool,
    ),
    ("freeze_damage", "minecraft:freeze_damage", RuleArg::Bool),
    (
        "global_sound_events",
        "minecraft:global_sound_events",
        RuleArg::Bool,
    ),
    (
        "immediate_respawn",
        "minecraft:immediate_respawn",
        RuleArg::Bool,
    ),
    ("keep_inventory", "minecraft:keep_inventory", RuleArg::Bool),
    (
        "lava_source_conversion",
        "minecraft:lava_source_conversion",
        RuleArg::Bool,
    ),
    (
        "limited_crafting",
        "minecraft:limited_crafting",
        RuleArg::Bool,
    ),
    ("locator_bar", "minecraft:locator_bar", RuleArg::Bool),
    (
        "log_admin_commands",
        "minecraft:log_admin_commands",
        RuleArg::Bool,
    ),
    (
        "max_block_modifications",
        "minecraft:max_block_modifications",
        RuleArg::Int(Some(1), None),
    ),
    (
        "max_command_forks",
        "minecraft:max_command_forks",
        RuleArg::Int(Some(0), None),
    ),
    (
        "max_command_sequence_length",
        "minecraft:max_command_sequence_length",
        RuleArg::Int(Some(0), None),
    ),
    (
        "max_entity_cramming",
        "minecraft:max_entity_cramming",
        RuleArg::Int(Some(0), None),
    ),
    (
        "max_snow_accumulation_height",
        "minecraft:max_snow_accumulation_height",
        RuleArg::Int(Some(0), Some(8)),
    ),
    ("mob_drops", "minecraft:mob_drops", RuleArg::Bool),
    (
        "mob_explosion_drop_decay",
        "minecraft:mob_explosion_drop_decay",
        RuleArg::Bool,
    ),
    ("mob_griefing", "minecraft:mob_griefing", RuleArg::Bool),
    (
        "natural_health_regeneration",
        "minecraft:natural_health_regeneration",
        RuleArg::Bool,
    ),
    (
        "player_movement_check",
        "minecraft:player_movement_check",
        RuleArg::Bool,
    ),
    (
        "players_nether_portal_creative_delay",
        "minecraft:players_nether_portal_creative_delay",
        RuleArg::Int(Some(0), None),
    ),
    (
        "players_nether_portal_default_delay",
        "minecraft:players_nether_portal_default_delay",
        RuleArg::Int(Some(0), None),
    ),
    (
        "players_sleeping_percentage",
        "minecraft:players_sleeping_percentage",
        RuleArg::Int(Some(0), None),
    ),
    (
        "projectiles_can_break_blocks",
        "minecraft:projectiles_can_break_blocks",
        RuleArg::Bool,
    ),
    ("pvp", "minecraft:pvp", RuleArg::Bool),
    ("raids", "minecraft:raids", RuleArg::Bool),
    (
        "random_tick_speed",
        "minecraft:random_tick_speed",
        RuleArg::Int(Some(0), None),
    ),
    (
        "reduced_debug_info",
        "minecraft:reduced_debug_info",
        RuleArg::Bool,
    ),
    (
        "respawn_radius",
        "minecraft:respawn_radius",
        RuleArg::Int(Some(0), None),
    ),
    (
        "send_command_feedback",
        "minecraft:send_command_feedback",
        RuleArg::Bool,
    ),
    (
        "show_advancement_messages",
        "minecraft:show_advancement_messages",
        RuleArg::Bool,
    ),
    (
        "show_death_messages",
        "minecraft:show_death_messages",
        RuleArg::Bool,
    ),
    (
        "spawner_blocks_work",
        "minecraft:spawner_blocks_work",
        RuleArg::Bool,
    ),
    ("spawn_mobs", "minecraft:spawn_mobs", RuleArg::Bool),
    ("spawn_monsters", "minecraft:spawn_monsters", RuleArg::Bool),
    ("spawn_patrols", "minecraft:spawn_patrols", RuleArg::Bool),
    ("spawn_phantoms", "minecraft:spawn_phantoms", RuleArg::Bool),
    (
        "spawn_wandering_traders",
        "minecraft:spawn_wandering_traders",
        RuleArg::Bool,
    ),
    ("spawn_wardens", "minecraft:spawn_wardens", RuleArg::Bool),
    (
        "spectators_generate_chunks",
        "minecraft:spectators_generate_chunks",
        RuleArg::Bool,
    ),
    ("spread_vines", "minecraft:spread_vines", RuleArg::Bool),
    ("tnt_explodes", "minecraft:tnt_explodes", RuleArg::Bool),
    (
        "tnt_explosion_drop_decay",
        "minecraft:tnt_explosion_drop_decay",
        RuleArg::Bool,
    ),
    (
        "universal_anger",
        "minecraft:universal_anger",
        RuleArg::Bool,
    ),
    (
        "water_source_conversion",
        "minecraft:water_source_conversion",
        RuleArg::Bool,
    ),
];

/// `SayCommand.register`. See [`message_root`].
///
/// A zero-argument wrapper rather than a call with arguments, because
/// `ci/command-surface.py` names each root by reading this `vec!` and can only
/// resolve a literal `Node::leaf("x")` or a builder it can call by name —
/// which it refuses to guess at rather than silently dropping.
fn say_root() -> Node {
    message_root(Node::leaf("say").requires(Level::Gamemasters), "message")
}

/// `TeamMsgCommand.register`: `literal("teammsg")` with no `requires`, so a
/// level-0 root — one of the ten a non-operator is told about — then
/// `argument("message", MessageArgument.message())`, executable. See
/// [`message_root`].
fn teammsg_root() -> Node {
    message_root(Node::leaf("teammsg"), "message")
}

/// `EmoteCommands.register`. Same shape, no `requires`, and the argument is
/// named `action` rather than `message` — the name is wire-visible, so the two
/// are not interchangeable.
fn me_root() -> Node {
    message_root(Node::leaf("me"), "action")
}

/// `HelpCommand.register`: `literal("help").executes(...)` with one optional
/// greedy-string child, also executable. No `requires`, so level 0.
fn help_root() -> Node {
    Node::leaf("help").then(Node {
        name: "command",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "brigadier:string",
            props: Props::Str(StringKind::Greedy),
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    })
}

/// `MsgCommand.register`'s first root.
///
/// `literal("msg").then(argument("targets", players()).then(argument("message",
/// message()).executes(...)))` — no `requires`, so level 0, and only the
/// deepest node is executable.
fn msg_root() -> Node {
    let mut root = Node::leaf("msg");
    root.executable = false;
    let mut targets = Node {
        name: "targets",
        level: Level::All,
        executable: false,
        parser: Some(Parser {
            // `EntityArgument.players()` — `single = false`, `playersOnly =
            // true`, which is the `amount`/`type` pair the wire carries as two
            // flag bits.
            registry: "minecraft:entity",
            props: Props::Entity {
                single: false,
                players_only: true,
            },
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    };
    targets = targets.then(Node {
        name: "message",
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:message",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    });
    root.then(targets)
}

/// A root that is one literal and one `minecraft:message` argument.
///
/// `/say` and `/me` are exactly this and differ only in the literal, the
/// argument name and the permission. `minecraft:message` writes no properties
/// and carries no suggestion provider, so the argument node is the parser name
/// and nothing else.
///
/// It takes the built root rather than the literal and level, so that each
/// caller carries its own visible `Node::leaf("…")`: `ci/command-surface.py`
/// names a root by finding that call in the builder's own body and refuses a
/// builder where it cannot.
fn message_root(root: Node, arg: &'static str) -> Node {
    let mut root = root;
    // Neither `SayCommand` nor `EmoteCommands` calls `executes` on the
    // literal; only the argument is executable.
    root.executable = false;
    root.then(Node {
        name: arg,
        level: Level::All,
        executable: true,
        parser: Some(Parser {
            registry: "minecraft:message",
            props: Props::Empty,
            suggestions: None,
        }),
        children: Vec::new(),
        redirect: None,
    })
}

/// `/gamerule` — `literal("gamerule").requires(LEVEL_GAMEMASTERS)`, then a
/// pair of literals per rule, each executable (query) and each carrying one
/// executable `value` argument (set).
fn gamerule_root() -> Node {
    let mut root = Node::leaf("gamerule").requires(Level::Gamemasters);
    // `literal("gamerule")` has no `executes`: bare `/gamerule` is a usage
    // error in vanilla, not a listing.
    root.executable = false;
    for (short, namespaced, arg) in GAME_RULES {
        for name in [short, namespaced] {
            root = root.then(Node::leaf(name).then(Node {
                name: "value",
                level: Level::All,
                executable: true,
                parser: Some(Parser {
                    registry: match arg {
                        RuleArg::Bool => "brigadier:bool",
                        RuleArg::Int(..) => "brigadier:integer",
                    },
                    props: match arg {
                        RuleArg::Bool => Props::Empty,
                        RuleArg::Int(min, max) => Props::Int(min, max),
                    },
                    suggestions: None,
                }),
                children: Vec::new(),
                redirect: None,
            }));
        }
    }
    root
}

/// `ServerboundChatCommandPacket` — one string, the command **without** its
/// leading slash.
///
/// # Errors
/// If the body is not a well-formed string.
pub fn read_command(body: &[u8]) -> io::Result<String> {
    let mut r = body;
    wire::read_string(&mut r)
}

/// Tell one player its permission level and the command tree, in vanilla's
/// order.
///
/// `PlayerList.sendPlayerPermissionLevel` sends
/// `ClientboundEntityEventPacket(player, 24 + level)` **first** and then
/// `Commands.sendCommands(player)`. The order is observable — the client uses
/// the level to decide what its own UI offers — and the entity event goes to
/// the player's own connection only, never to trackers.
///
/// # Errors
/// On I/O failure.
pub fn send_commands(conn: &crate::players::Connection, level: Level) -> io::Result<()> {
    let mut ev = Vec::new();
    // `ClientboundEntityEventPacket.write`: a plain big-endian i32 entity id
    // — **not** a varint, unlike most ids on this wire — then one byte.
    wire::write_i32(&mut ev, conn.entity_id())?;
    wire::write_u8(&mut ev, level.entity_event_byte())?;
    conn.send(ids::play::cb::ENTITY_EVENT, &ev)?;
    conn.send(ids::play::cb::COMMANDS, &tree().encode(level)?)
}

/// Handle a `chat_command`.
///
/// Returns whether anything was dispatched, which is what a caller needs to
/// tell "answered" from "this server has no such command" without reading the
/// socket.
///
/// # Errors
/// If the packet cannot be read or the reply cannot be written.
pub fn handle(s: &Session<'_>, body: &[u8]) -> io::Result<bool> {
    let text = read_command(body)?;
    let level = crate::ops::level_for(s.conn.name());
    let resolved = match tree().resolve(&text, level) {
        Ok(r) => r,
        Err(refusal) => {
            // `Commands.finishParsing`'s two components, for the three
            // refusals this server can render; silence for the other two,
            // which is what the module header describes and which is now the
            // residue rather than the whole answer.
            let Some(lines) = failure_components(&text, refusal) else {
                return Ok(false);
            };
            for line in lines {
                send_system_chat(s.conn, &line)?;
            }
            return Ok(true);
        }
    };
    // The roots `crate::cmdexec` owns, run against a player source.
    {
        let ctx = crate::cmdexec::Ctx::player(
            s.conn,
            level,
            s.players,
            [s.player.x, s.player.y, s.player.z],
            [s.player.pitch, s.player.yaw],
        );
        if let Some(done) = crate::cmdexec::dispatch(&resolved, &ctx) {
            done?;
            return Ok(true);
        }
    }
    let names: Vec<String> = s
        .players
        .connections()
        .iter()
        .map(|c| c.name().to_string())
        .collect();
    let component = match resolved.path.as_slice() {
        ["list"] => list_players_component(&names),
        ["list", "uuids"] => list_uuids_component(&names),
        ["seed"] => seed_component(s.seed),
        // `/version` is TEN `system_chat` packets, not one, so it returns
        // early rather than falling through to the single-component send at
        // the bottom of this function.
        ["version"] => {
            for line in version_components() {
                send_system_chat(s.conn, &line)?;
            }
            return Ok(true);
        }
        // `/say` and `/me`. Both broadcast rather than answering the caller,
        // so both return early: there is no component for the send at the
        // bottom of this function to make.
        ["say", "message"] => {
            let text = resolved.arg("message").unwrap_or_default();
            return broadcast_command_message(s, text, "minecraft:say_command", level);
        }
        ["me", "action"] => {
            let text = resolved.arg("action").unwrap_or_default();
            return broadcast_command_message(s, text, "minecraft:emote_command", level);
        }
        // `/help`, both forms. Like `/version` it is many `system_chat`
        // packets, so it returns early.
        ["help"] => {
            for line in help_lines(level) {
                send_system_chat(s.conn, &Nbt::String(line))?;
            }
            return Ok(true);
        }
        ["help", "command"] => {
            let query = resolved.arg("command").unwrap_or_default();
            let Some(lines) = help_lines_for(query, level) else {
                // `HelpCommand.ERROR_FAILED` — `commands.help.failed`, raised
                // as a `CommandSyntaxException`, so its message goes through
                // the same unported path as every other Brigadier error.
                eprintln!("/help {query:?}: no such command — not answered");
                return Ok(false);
            };
            for line in lines {
                send_system_chat(s.conn, &Nbt::String(line))?;
            }
            return Ok(true);
        }
        // `/msg`, and therefore `/tell` and `/w`: `Tree::resolve` follows the
        // redirect, so all three arrive here with the same path.
        ["msg", "targets", "message"] => {
            let targets = resolved.arg("targets").unwrap_or_default();
            let text = resolved.arg("message").unwrap_or_default();
            return private_message(s, targets, text, level);
        }
        // `/gamerule <rule>` and `/gamerule <rule> <value>`. `GameRule.id()`
        // is the bare name for both registered literals, so the namespaced
        // form answers about the same rule under the same name.
        ["gamerule", rule] => {
            let bare = rule.strip_prefix("minecraft:").unwrap_or(rule);
            let Some(value) = crate::gamerules::GameRules::shared().get(bare) else {
                // Unreachable through `tree()`: every literal under
                // `gamerule` is one of GAME_RULES' 58 names and every one of
                // those is in `gamerules::DEFAULTS` — asserted by
                // `every_registered_rule_has_a_default`. A refusal rather
                // than a panic, for the connection-thread reason below.
                eprintln!("/gamerule {bare}: in the tree with no default — not answered");
                return Ok(false);
            };
            gamerule_query_component(bare, &value.serialize())
        }
        ["gamerule", rule, "value"] => {
            let bare = rule.strip_prefix("minecraft:").unwrap_or(rule);
            let token = resolved.arg("value").unwrap_or_default();
            match crate::gamerules::GameRules::shared().set(bare, token) {
                Ok(v) => gamerule_set_component(bare, &v.serialize()),
                // `Tree::resolve` already ran `Parser::accepts`, so a token
                // that reaches here parsed against the declared bounds. A
                // `SetError` is therefore a disagreement between GAME_RULES
                // and `gamerules::DEFAULTS` about a rule's type, which is a
                // programming error and is reported as one rather than
                // answered with an invented message.
                Err(e) => {
                    eprintln!("/gamerule {bare} {token}: {e} — not answered");
                    return Ok(false);
                }
            }
        }
        // `/worldborder`'s nine executable paths. All of them go through one
        // arm because they share the lock, the broadcast and the reply send;
        // `worldborder_dispatch` is where the path becomes an executor call.
        ["worldborder", ..] => return worldborder_dispatch(s, &resolved),
        // `/scoreboard objectives`' five executable paths, through one arm for
        // the reason `/worldborder`'s gives.
        ["scoreboard", ..] => return scoreboard_dispatch(s, &resolved),
        // Unreachable by construction: `resolve` only returns paths through
        // `tree()`, and every executable node in it is matched above. An
        // `unreachable!()` here would be a panic in a connection thread —
        // which reaches a reader as a dropped socket — so it is a refusal
        // instead, and a loud one.
        other => {
            eprintln!("command {other:?} is in the tree with no reply — not answered");
            return Ok(false);
        }
    };
    send_system_chat(s.conn, &component)?;
    Ok(true)
}

/// Every `/worldborder` path, from a resolved command to the packets and the
/// reply.
///
/// The path-to-executor mapping itself is [`crate::bordercmd::run`], because
/// [`crate::console`] dispatches the same commands with no connection to
/// answer — `MinecraftServer.createCommandSourceStack()` is unconditionally
/// full permission and has no player behind it. What is left here is the three
/// things only a player's command does: take the border lock **once** around
/// the read-modify-write, broadcast to every connection, and send the reply to
/// the caller alone.
///
/// # The refusals are silent, and that is the honest answer rather than the
/// complete one
///
/// A `SimpleCommandExceptionType` raised inside an executor reaches a client
/// through `Commands.executeCommand`'s catch — a different path from the
/// `finishParsing` one [`failure_components`] renders, and one this server has
/// not ported. Answering it with a guess would put bytes on the channel
/// `tools/oracle-runner` digests whole. So a refusal returns `Ok(false)`,
/// which is `absent`, and names itself on the console.
fn worldborder_dispatch(
    s: &Session<'_>,
    resolved: &crate::commandtree::Resolved,
) -> io::Result<bool> {
    let mut border = crate::worldborder::shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let outcome = crate::bordercmd::run(
        resolved,
        &mut border,
        (s.player.x, s.player.z),
        // `level.getGameTime()`, which `lerpSizeBetween` stores as `lerpBegin`
        // and which only `getLerpSpeed` reads back.
        s.clock.ticks(),
    );
    // The lock is dropped before anything touches a socket: a slow client must
    // not hold the border against the tick thread.
    drop(border);
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            eprintln!("/worldborder: {e} — not answered");
            return Ok(false);
        }
    };
    crate::bordercmd::broadcast(s.players, &outcome.packets);
    send_system_chat(s.conn, &outcome.reply)?;
    Ok(true)
}

/// Every `/scoreboard` path, from a resolved command to the packets and the
/// reply.
///
/// The path-to-executor mapping is [`crate::scoreboardcmd::run`], shared with
/// [`crate::console`]. What is here is the lock, the broadcast and the reply,
/// exactly as [`worldborder_dispatch`] is.
fn scoreboard_dispatch(
    s: &Session<'_>,
    resolved: &crate::commandtree::Resolved,
) -> io::Result<bool> {
    let mut board = crate::scoreboardcmd::shared()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let outcome = crate::scoreboardcmd::run(resolved, &mut board);
    drop(board);
    let outcome = match outcome {
        Ok(o) => o,
        Err(e) => {
            eprintln!("/scoreboard: {e} — not answered");
            return Ok(false);
        }
    };
    crate::bordercmd::broadcast(s.players, &outcome.packets);
    send_system_chat(s.conn, &outcome.reply)?;
    Ok(true)
}

/// `/say` and `/me`: `MessageArgument.resolveChatMessage` then
/// `PlayerList.broadcastChatMessage` under a bound chat type.
///
/// # The component is the message text, and that is not the simplification it
/// looks like
///
/// `resolveChatMessage` on an offline server takes the
/// `resolveDisguisedMessage` branch (`getSigningContext().getArgument(name)`
/// is null, because nothing here sends `chat_command_signed`), building
/// `PlayerChatMessage.system(text).withUnsignedContent(resolveComponent(...))`.
/// `withUnsignedContent` stores **null** when the component equals
/// `Component.literal(signedContent())`, and for a message with no selector
/// parts `Message.toComponent` returns exactly that. So `decoratedContent()`
/// falls back to `Component.literal(text)`, which an unstyled literal
/// serialises as a bare `TAG_String`.
///
/// # Selector resolution, and the permission that decides it
///
/// `Message.resolveComponent` passes
/// `permissions.hasPermission(Permissions.COMMANDS_ENTITY_SELECTORS)` as
/// `allowSelectors`, and `LevelBasedPermissionSet.hasPermission` answers that
/// one atom specially: **`level >= GAMEMASTERS`**. `toComponent` then returns
/// `Component.literal(text)` unchanged when there are no parts **or** when
/// selectors are not allowed.
///
/// That gives three cases and this server is exact on two of them:
///
/// * no `@` selector in the text — the raw text, at any level. Exact.
/// * a selector, below GAMEMASTERS — vanilla does **not** resolve it and
///   sends the raw text with the `@a` still in it. Exact, and reachable only
///   through `/me`, since `/say` requires GAMEMASTERS.
/// * a selector, at or above GAMEMASTERS — vanilla resolves it into the
///   matched names. **Not ported**, and refused rather than approximated:
///   `crate::commandtree::message` counts the parts and this declines when the
///   count is non-zero and the level allows resolution. Inventing the resolved
///   component would put wrong bytes on the channel `tools/oracle-runner`
///   digests whole.
///
/// # Errors
/// On a write failure. A parse that produced parts is not an error — it is a
/// refusal, reported as `Ok(false)` exactly as an unknown command is.
fn broadcast_command_message(
    s: &Session<'_>,
    text: &str,
    chat_type: &str,
    level: Level,
) -> io::Result<bool> {
    let mut r = crate::cmdreader::Reader::new(text);
    let parts = match crate::commandtree::message(&mut r) {
        Ok(n) => n,
        // Unreachable through `tree()`: `Tree::resolve` already ran this same
        // parser to accept the argument. A refusal rather than a panic, for
        // the connection-thread reason the `/gamerule` arms give.
        Err(e) => {
            eprintln!("message argument {text:?} reparsed as {e:?} — not answered");
            return Ok(false);
        }
    };
    // `LevelBasedPermissionSet.hasPermission(COMMANDS_ENTITY_SELECTORS)`.
    let selectors_allowed = level.allows(Level::Gamemasters);
    if parts > 0 && selectors_allowed {
        eprintln!(
            "{chat_type}: {parts} entity selector(s) in {text:?} — this server \
             cannot resolve them, so the message is not sent"
        );
        return Ok(false);
    }
    let Some(id) = crate::chat::chat_type_id(chat_type) else {
        eprintln!("{chat_type} is not in the chat_type registry — not answered");
        return Ok(false);
    };
    let display = display_name_component(s.conn.name());
    crate::chat::broadcast_disguised(s.players, &Nbt::String(text.to_owned()), id, &display);
    Ok(true)
}

/// The two components `Commands.finishParsing` sends when a parse fails.
///
/// # Why this one reply is worth more than any command
///
/// It is the observable answer for **every** typo, **every** root this server
/// has not ported, and **every** command a player lacks permission for.
/// `crate::commands`' header called it "one job whose reach is the whole
/// unported surface, and porting `/me` is one job whose reach is `/me`", and
/// until this landed the answer to all of them was silence, which is
/// indistinguishable on the wire from a command that ran and printed nothing.
///
/// # The shape, and the three places it is not what it looks like
///
/// ```text
/// line 1  { color: red, extra: [ { translate: <key> } ],       text: "" }
/// line 2  { color: red, extra: [ { click_event: {suggest_command "/"+input},
///                                  color: gray,
///                                  extra: [ …, red-underline, here ],
///                                  text: "" } ],               text: "" }
/// ```
///
/// * **The red is `sendFailure`'s, not the message's.**
///   `CommandSourceStack.sendFailure` wraps whatever it is given in
///   `Component.empty().append(c).withStyle(RED)`, so BOTH lines carry the
///   same outer wrapper and neither inner component is red itself.
/// * **The prefix run is omitted when it is empty**, because
///   `MutableComponent.append(String)` returns early on an empty string rather
///   than adding an empty sibling. `nope` fails at cursor 0 and its context
///   component therefore starts with the red part, with no `{"": ""}` before
///   it.
/// * **The context elements are wrapped and the styled ones are not**, which
///   is `ListTag.write`'s rule and not a component rule — see
///   [`translation_args`]. The ellipsis and the prefix are bare strings in a
///   list that also holds compounds, so the writer wraps each of those two and
///   leaves the compounds alone.
///
/// Returns `None` for the two refusals this server cannot render: an
/// argument-level Brigadier exception, whose key and arguments are its own,
/// and a parser this server could not read at all, where our exception count
/// and matched range are not vanilla's and neither would be any exception
/// derived from them.
#[must_use]
pub fn failure_components(input: &str, refusal: Refusal) -> Option<[Nbt; 2]> {
    let Syntax::Known { key, args } = refusal.syntax()? else {
        // `Syntax::Unmapped` — an exception raised by a
        // `net.minecraft.commands.arguments` type, whose translation key this
        // server has not transcribed. Rendered as nothing rather than as a
        // guess.
        return None;
    };
    let cursor = refusal.cursor()?;
    let message = failure_wrapper(translatable_syntax(input, key, args));

    // `int i = Math.min(e.getInput().length(), e.getCursor())`.
    let i = cursor.min(input.len());
    let mut extra: Vec<Arg> = Vec::new();
    // `if (i > 10) ctx.append(CommonComponents.ELLIPSIS)`. Ten CHARACTERS in
    // Java, which counts UTF-16 units; this counts bytes. They agree on every
    // command a client can send that is not a mojibake test, and the
    // difference is recorded rather than hidden — see the module header.
    if i > CONTEXT_WINDOW {
        extra.push(Arg::Text(ELLIPSIS.to_string()));
    }
    // `ctx.append(input.substring(Math.max(0, i - 10), i))`, and an empty
    // substring appends nothing at all.
    let from = i.saturating_sub(CONTEXT_WINDOW);
    if from < i {
        extra.push(Arg::Text(input[from..i].to_string()));
    }
    // `if (i < input.length())` — the offending remainder, red and underlined.
    if i < input.len() {
        extra.push(Arg::Component(compound([
            ("color", Nbt::String("red".into())),
            ("underlined", Nbt::Byte(1)),
            ("text", Nbt::String(input[i..].to_string())),
        ])));
    }
    extra.push(Arg::Component(compound([
        ("color", Nbt::String("red".into())),
        ("italic", Nbt::Byte(1)),
        ("translate", Nbt::String("command.context.here".into())),
    ])));
    let context = compound([
        (
            "click_event",
            compound([
                ("action", Nbt::String("suggest_command".into())),
                // `new ClickEvent.SuggestCommand("/" + input)` — the slash
                // `Commands.trimOptionalPrefix` took off, put back.
                ("command", Nbt::String(format!("/{input}"))),
            ]),
        ),
        ("color", Nbt::String("gray".into())),
        ("extra", translation_args(&extra)),
        ("text", Nbt::String(String::new())),
    ]);
    Some([message, failure_wrapper(context)])
}

/// One Brigadier exception as a component: its translation key and whatever
/// arguments it carries.
///
/// The argument shapes come straight off `BrigadierExceptions`' lambdas: a
/// span renders as the input text it names, a bound pair renders as two
/// numbers **bound first** (see [`SyntaxArgs::Ints`]). Two numbers of one type
/// are a homogeneous list, so they stay bare on the wire — the `/version`
/// `commands.version.data` case, arriving from a third direction.
fn translatable_syntax(input: &str, key: &str, args: SyntaxArgs) -> Nbt {
    let args: Vec<Arg> = match args {
        SyntaxArgs::None => Vec::new(),
        SyntaxArgs::Span { start, end } => vec![Arg::Text(input[start..end].to_string())],
        SyntaxArgs::Ints(a, b) => vec![Arg::Int(a), Arg::Int(b)],
        // `Component.translationArg(Number)` for a `Long`, `Float` or `Double`
        // is not an `int`, and this server has no command that reaches one:
        // `/gamerule`'s value nodes are the only bounded arguments it
        // registers and every one of them is `brigadier:integer`. Rendering
        // them as text would be a guess with no vector behind it, so they are
        // refused the way an unmapped key is.
        SyntaxArgs::Longs(..) | SyntaxArgs::Floats(..) | SyntaxArgs::Doubles(..) => {
            return compound([("translate", Nbt::String(key.to_string()))])
        }
    };
    translatable(key, &args)
}

/// `CommandSourceStack.sendFailure`'s wrapper:
/// `Component.empty().append(c).withStyle(RED)`.
///
/// `Component.empty()` is a literal with empty text, so the wrapper is
/// `{color: "red", extra: [c], text: ""}` — a one-element `extra` whose single
/// member is a compound, so no `ListTag` wrapping applies.
fn failure_wrapper(inner: Nbt) -> Nbt {
    compound([
        ("color", Nbt::String("red".into())),
        ("extra", Nbt::List(vec![inner])),
        ("text", Nbt::String(String::new())),
    ])
}

/// `CommonComponents.ELLIPSIS`'s text.
const ELLIPSIS: &str = "...";

/// How much of the input `finishParsing` shows before the error, and the
/// threshold above which it prefixes an ellipsis. The same literal `10` twice
/// in vanilla, `Math.max(0, i - 10)` and `if (i > 10)`.
const CONTEXT_WINDOW: usize = 10;

/// `/help` with no argument: one line per usable root.
///
/// `getSmartUsage(dispatcher.getRoot(), source)` and then
/// `Component.literal("/" + usage)` per value. The root has no `Command`, so
/// the top-level `optional` is false and the roots are unbracketed.
///
/// **The component is a bare literal**, which serialises as a `TAG_String` —
/// the same shape `/say`'s message takes and not the `{"text": …}` a port
/// would write by hand.
#[must_use]
pub fn help_lines(level: Level) -> Vec<String> {
    let t = tree();
    // The dispatcher's root is not executable, which is the `optional = false`
    // this passes. Writing `true` here brackets every root.
    t.smart_usage(&t.roots, false, level)
        .into_iter()
        .map(|u| format!("/{u}"))
        .collect()
}

/// `/help <command>`: the usages under the node that command names.
///
/// Three things that are easy to get wrong and are all in
/// `HelpCommand.lambda$register$1`:
///
/// * the prefix is `parse.getReader().getString()` — the **whole** argument as
///   typed, not the node's own name — and the line is `prefix + " " + usage`
///   with no leading slash;
/// * the map is `getSmartUsage(lastNode, source)`, which enumerates that
///   node's CHILDREN. A leaf command therefore prints **nothing** and returns
///   0, which is not an error;
/// * the failure arm is `getNodes().isEmpty()`, i.e. nothing matched at all.
///
/// `None` is that failure arm. An empty `Vec` is the leaf case and is a
/// success with no output.
#[must_use]
pub fn help_lines_for(query: &str, level: Level) -> Option<Vec<String>> {
    let t = tree();
    let (node, _matched) = t.usage_target(query, level)?;
    Some(
        t.smart_usage(&node.children, node.executable, level)
            .into_iter()
            .map(|u| format!("{query} {u}"))
            .collect(),
    )
}

/// `/msg`, `/tell` and `/w`: `MsgCommand.sendMessage`.
///
/// # It is TWO packets per target, with two different chat types
///
/// `sendMessage` binds `MSG_COMMAND_INCOMING` once, outside the loop, and then
/// for each target sends **the sender** a copy bound to `MSG_COMMAND_OUTGOING`
/// **with `withTargetName(target.getDisplayName())`** and the target a copy
/// bound to the incoming type with no target name. So `/msg Bob hi` puts two
/// `disguised_chat` packets on the wire carrying the same component and
/// different bounds, and `/msg @a hi` puts out two per recipient.
///
/// The order inside the loop is the sender's copy first, then the target's.
///
/// # Which selectors resolve here, and which are refused
///
/// `EntityArgument.players()` has already refused anything that can name a
/// non-player, so what reaches this is a plain name, a UUID, `@s`, `@a`, or
/// `@p`/`@r`. Of those:
///
/// * a **name** is looked up in the connection list, and a **UUID** against
///   `wire::offline_uuid` — this server has no online-mode profiles, so a
///   uuid only ever names a player whose name hashes to it;
/// * **`@s`** is the caller;
/// * **`@a`** is every connection, in join order, which is `PlayerList`'s own
///   iteration order;
/// * **`@p` and `@r` are refused.** `@p` is `ORDER_NEAREST` over the source's
///   position and this server tracks no position for a connection other than
///   the session's own; `@r` is a draw from vanilla's RNG and reproducing a
///   draw we cannot see is worse than not answering. Both are named rather
///   than silently treated as `@s`.
///
/// An empty result is `EntityArgument.NO_PLAYERS_FOUND`, whose message is not
/// ported, so it is silence — the same answer `crate::commands`' header
/// describes for every unported reply.
///
/// # Errors
/// On a write failure.
fn private_message(s: &Session<'_>, targets: &str, text: &str, level: Level) -> io::Result<bool> {
    let mut r = crate::cmdreader::Reader::new(targets);
    let Ok(sel) = crate::commandtree::entity_selector(&mut r, false, true) else {
        eprintln!("/msg targets {targets:?} reparsed as a refusal — not answered");
        return Ok(false);
    };
    let conns = s.players.connections();
    let chosen: Vec<std::sync::Arc<crate::players::Connection>> = if sel.used_selector {
        if sel.self_selector {
            // Compared by NAME rather than by connection id, because this
            // server is offline-mode and a name identifies one connection —
            // `crate::players::Connection` exposes no id and this does not
            // need one.
            conns
                .iter()
                .filter(|c| c.name() == s.conn.name())
                .cloned()
                .collect()
        } else if sel.max_results > 1 {
            conns.clone()
        } else {
            eprintln!(
                "/msg {targets:?}: @p and @r need a position or an RNG draw this \
                 server does not have — not answered"
            );
            return Ok(false);
        }
    } else {
        // A name, or the dashed uuid form `parseNameOrUUID` would have taken.
        // `dashed_uuid` is the same rendering `/list uuids` puts on the wire,
        // so the two agree by construction.
        conns
            .iter()
            .filter(|c| c.name() == targets || dashed_uuid(c.name()) == targets)
            .cloned()
            .collect()
    };
    if chosen.is_empty() {
        // `EntityArgument.NO_PLAYERS_FOUND`. Vanilla answers with a message;
        // this server does not have it and says nothing rather than inventing
        // one.
        eprintln!("/msg {targets:?}: no players found — not answered");
        return Ok(false);
    }
    let mut r = crate::cmdreader::Reader::new(text);
    let Ok(parts) = crate::commandtree::message(&mut r) else {
        eprintln!("/msg message {text:?} reparsed as a refusal — not answered");
        return Ok(false);
    };
    if parts > 0 && level.allows(Level::Gamemasters) {
        eprintln!(
            "/msg: {parts} entity selector(s) in {text:?} — this server cannot \
             resolve them, so the message is not sent"
        );
        return Ok(false);
    }
    let (Some(incoming), Some(outgoing)) = (
        crate::chat::chat_type_id("minecraft:msg_command_incoming"),
        crate::chat::chat_type_id("minecraft:msg_command_outgoing"),
    ) else {
        eprintln!("the msg_command chat types are not in the registry — not answered");
        return Ok(false);
    };
    let component = Nbt::String(text.to_owned());
    let sender = display_name_component(s.conn.name());
    for target in &chosen {
        let target_name = display_name_component(target.name());
        // The sender's copy first, with the target's display name in the
        // bound. `sendMessage`'s loop body order.
        crate::chat::send_disguised(s.conn, &component, outgoing, &sender, Some(&target_name));
        crate::chat::send_disguised(target, &component, incoming, &sender, None);
    }
    Ok(true)
}

/// `CommandSourceStack.sendSuccess` for a player source: a `system_chat` with
/// `overlay` false.
///
/// # Errors
/// On I/O failure.
pub fn send_system_chat(conn: &crate::players::Connection, component: &Nbt) -> io::Result<()> {
    let mut body = Vec::new();
    write_network_nbt(&mut body, component)?;
    // `ClientboundSystemChatPacket`'s second field: `overlay`, false for
    // anything that is not an action-bar message. `sendSuccess` sends a chat
    // line.
    wire::write_bool(&mut body, false)?;
    conn.send(ids::play::cb::SYSTEM_CHAT, &body)
}

/// A `Component` on the wire: the network form, with no root name.
///
/// The third copy of these three lines, and deliberately not shared — see
/// `crate::menu`'s note. One is a registry payload, one a menu title, one a
/// chat line; a shared helper would make a later divergence between them look
/// like a bug in whichever caller noticed first.
pub(crate) fn write_network_nbt(out: &mut impl io::Write, value: &Nbt) -> io::Result<()> {
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", value)?;
    // The shape check `crate::configuration`'s copy carries, and it is the one
    // difference from `crate::menu`'s worth keeping: `&named[3..]` on a shorter
    // buffer is a panic in a connection thread, which reaches a reader as a
    // dropped socket and no message at all. An error says which layer.
    if named.len() < 3 || named[1] != 0 || named[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "named root was not the expected tag + empty-name shape",
        ));
    }
    out.write_all(&named[..1])?;
    out.write_all(&named[3..])
}

/// A `system_chat` body carrying one plain line of text.
///
/// Here rather than in [`crate::plugins`], which is the only caller, because
/// this is the second thing in this server that writes a chat component and
/// the encoding belongs beside the first. `{"text": ...}` is the whole
/// component: plain text and not a rich one, which is the API's own decision
/// (`grit-plugins/docs/DECISIONS.md`) — a component type in a plugin API
/// freezes the chat-component encoding, and that is a protocol detail that
/// changes with the game.
///
/// # Errors
/// If the component cannot be encoded.
pub(crate) fn plain_text_body(text: &str) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_network_nbt(
        &mut body,
        &compound([("text", Nbt::String(text.to_owned()))]),
    )?;
    // `overlay`, false for anything that is not an action-bar message.
    wire::write_bool(&mut body, false)?;
    Ok(body)
}

/// One translation argument, before `ListTag`'s write-time wrapping.
///
/// The variants are the three shapes `Component.translationArg` can produce
/// for the arguments this server builds: a `String`, an `int`, and a nested
/// `Component`. A `String` and a `Component.literal(String)` are the same
/// bytes — an unstyled literal serialises as a bare `TAG_String` — so they
/// share a variant.
#[derive(Debug, Clone)]
pub enum Arg {
    /// A `String` argument, or equivalently `Component.literal(s)`.
    Text(String),
    /// An `int` argument.
    Int(i32),
    /// A `long` argument — `Long.valueOf`, which `/time`'s replies pass and
    /// which serialises as a `TAG_Long`, not an int.
    Long(i64),
    /// A `Component` argument, already built.
    Component(Nbt),
}

impl Arg {
    /// The raw tag this argument becomes before wrapping.
    fn raw(&self) -> Nbt {
        match self {
            Self::Text(s) => Nbt::String(s.clone()),
            Self::Int(v) => Nbt::Int(*v),
            Self::Long(v) => Nbt::Long(*v),
            Self::Component(c) => c.clone(),
        }
    }
}

/// A `with` list, built the way `ListTag.write` builds one.
///
/// # This is an NBT rule and it had been applied by hand at three call sites
///
/// `ListTag.write` in 26.2 is three steps and only the first is obvious:
///
/// 1. `identifyRawElementType()` scans **every** element. All the same tag id
///    gives that id; **any disagreement gives 10 (`TAG_Compound`)**; an empty
///    list gives 0.
/// 2. that byte and the count are written;
/// 3. every element goes through `wrapIfNeeded(type, tag)` — which, when the
///    list type is 10, replaces a non-compound with `{"": tag}`.
///
/// So the `{"": 2}` in `/list`'s recorded reply is not something
/// `ListPlayersCommand` built. It is an `IntTag(2)` that the **writer** wrapped
/// because a `Component` sat beside it in the same list. `/version`'s
/// `commands.version.data` line, whose one argument is also an int, is a bare
/// `TAG_Int` for exactly the same reason read the other way: nothing else is in
/// that list to disagree with it.
///
/// A port that treated "primitive translation argument" as always-wrapped gets
/// `/list` right and `/version` wrong; one that treated it as never-wrapped
/// gets the opposite. Both are in the golden.
///
/// # The re-wrap arm, which looks like a bug and is not
///
/// `wrapIfNeeded` re-wraps a compound that IS already a wrapper —
/// `isWrapper(c)` is `size() == 1 && contains("")` — so a genuine single-empty-
/// key component beside a `TAG_Int` becomes `{"": {"": ...}}`. That is how the
/// reader knows which layer to unwrap. Nothing this server builds hits it
/// today; it is transcribed because the alternative is a helper that is right
/// for the current call sites and silently wrong for the next one.
#[must_use]
pub fn translation_args(args: &[Arg]) -> Nbt {
    let raw: Vec<Nbt> = args.iter().map(Arg::raw).collect();
    let first = raw.first().map_or(0, Nbt::tag);
    let heterogeneous = raw.iter().any(|t| t.tag() != first);
    if !heterogeneous {
        return Nbt::List(raw);
    }
    Nbt::List(
        raw.into_iter()
            .map(|t| {
                // `wrapIfNeeded(10, tag)`: a compound stays as it is **unless**
                // it is itself a wrapper, in which case it is wrapped again.
                let already = match &t {
                    Nbt::Compound(m) => m.len() != 1 || !m.contains_key(""),
                    _ => false,
                };
                if already {
                    t
                } else {
                    compound([("", t)])
                }
            })
            .collect(),
    )
}

/// `Component.translatable(key, args...)`.
#[must_use]
pub fn translatable(key: &str, args: &[Arg]) -> Nbt {
    if args.is_empty() {
        // A no-argument translatable has no `with` key at all, not an empty
        // one. `commands.version.header` and `commands.version.stable.yes` are
        // both this shape and both are in the golden.
        return compound([("translate", Nbt::String(key.to_string()))]);
    }
    compound([
        ("with", translation_args(args)),
        ("translate", Nbt::String(key.to_string())),
    ])
}

/// `/version`'s ten lines, as `(translation key, arguments)`.
///
/// # Every value is a constant, and that is a property of the command
///
/// `VersionCommand.dumpVersion` reads `SharedConstants.getCurrentVersion()`,
/// which is the pinned jar's own `version.json` and cannot change without the
/// version changing. So `/version` needs no world, no player and no server
/// state — it is the cheapest real root this server can answer, and the ten
/// bodies below are gated byte for byte against
/// `data/vanilla/commands_tree.json`'s `versionLines`, which
/// `ci/commands-golden.sh` takes from `dumpVersion` itself.
///
/// # The header is line 0 and `dumpVersion` does not emit it
///
/// `VersionCommand`'s executor is `sendSystemMessage(HEADER)` **then**
/// `dumpVersion(source::sendSystemMessage)`. A port gated only against
/// `dumpVersion` would send nine of vanilla's ten packets and pass; the golden
/// carries the header explicitly now for that reason.
///
/// # `build_time` is rendered in UTC, and that is a stated divergence
///
/// `Component.translationArg(Date)` is `Component.literal(date.toString())`,
/// and `java.util.Date.toString` renders in the **JVM's default time zone**.
/// A vanilla server in Asia/Tokyo prints a different string for the same
/// build. This server has no such notion, so it prints UTC — which is what
/// `ci/commands-golden.sh` pins its dump to, and what its TZ control asserts
/// is the *only* zone-dependent line in the reply.
const VERSION_LINES: [(&str, &[VersionArg]); 10] = [
    ("commands.version.header", &[]),
    ("commands.version.id", &[VersionArg::Text("26.2")]),
    ("commands.version.name", &[VersionArg::Text("26.2")]),
    // `DataVersion.version()` is an `int` and the only argument in its list,
    // so it is a bare `TAG_Int` on the wire. See [`translation_args`].
    ("commands.version.data", &[VersionArg::Int(4903)]),
    ("commands.version.series", &[VersionArg::Text("main")]),
    // Two arguments of different types, which is what makes this the line
    // that proves the wrapping rule: `protocolVersion()` and
    // `"0x" + Integer.toHexString(protocolVersion())`.
    (
        "commands.version.protocol",
        &[VersionArg::Int(776), VersionArg::Text("0x308")],
    ),
    (
        "commands.version.build_time",
        &[VersionArg::Text("Tue Jun 16 12:01:27 UTC 2026")],
    ),
    (
        "commands.version.pack.resource",
        &[VersionArg::Text("88.0")],
    ),
    ("commands.version.pack.data", &[VersionArg::Text("107.1")]),
    // `STABLE` / `UNSTABLE`, chosen by `WorldVersion.stable()`. 26.2 is
    // stable, so `commands.version.stable.no` is never sent and is not here —
    // a port carrying both would carry one line no gate can reach.
    ("commands.version.stable.yes", &[]),
];

/// A `VERSION_LINES` argument, which must be `const`-constructible and so
/// cannot be [`Arg`] (whose `Text` owns a `String`).
#[derive(Debug, Clone, Copy)]
enum VersionArg {
    Text(&'static str),
    Int(i32),
}

/// The ten components `/version` answers with, in vanilla's order.
#[must_use]
pub fn version_components() -> Vec<Nbt> {
    VERSION_LINES
        .iter()
        .map(|(key, args)| {
            let args: Vec<Arg> = args
                .iter()
                .map(|a| match a {
                    VersionArg::Text(t) => Arg::Text((*t).to_string()),
                    VersionArg::Int(v) => Arg::Int(*v),
                })
                .collect();
            translatable(key, &args)
        })
        .collect()
}

/// A component compound, with the key order `rustmc_world::nbt` writes.
///
/// `pub` since 2026-09-05 so [`crate::scoreboardcmd`] can build
/// `getFormattedDisplayName`'s nested hover event with the same helper rather
/// than a second one; the key ordering rule is the whole reason there must be
/// only one.
#[must_use]
pub fn compound<const N: usize>(entries: [(&str, Nbt); N]) -> Nbt {
    let mut m = BTreeMap::new();
    for (k, v) in entries {
        m.insert(k.to_string(), v);
    }
    Nbt::Compound(m)
}

/// A translation argument that is not a component.
///
/// `{"": <value>}` — a compound whose only key is the empty string. Read off
/// vanilla's own bytes; see the module header.
fn primitive_arg(value: i32) -> Nbt {
    compound([("", Nbt::Int(value))])
}

/// `ServerPlayer.getDisplayName()` for a player on no team.
///
/// `PlayerTeam.formatNameForTeam(null, name)` returns the name unchanged, and
/// `ServerPlayer.getDisplayName` then adds the click, hover and insertion.
#[must_use]
pub fn display_name_component(name: &str) -> Nbt {
    compound([
        (
            "click_event",
            compound([
                ("action", Nbt::String("suggest_command".into())),
                // The trailing space is vanilla's, so the player can type
                // straight after it. It is inside the recorded bytes.
                ("command", Nbt::String(format!("/tell {name} "))),
            ]),
        ),
        ("insertion", Nbt::String(name.to_string())),
        ("text", Nbt::String(name.to_string())),
        (
            "hover_event",
            compound([
                ("name", Nbt::String(name.to_string())),
                ("action", Nbt::String("show_entity".into())),
                ("id", Nbt::String("minecraft:player".into())),
                ("uuid", uuid_ints(name)),
            ]),
        ),
    ])
}

/// `commands.list.nameAndId` for one player — the `/list uuids` element.
///
/// **Both arguments are bare `TAG_String`s, not compounds.** `Player.getName()`
/// is `Component.literal(profile.name())` and `Component.translationArg(UUID)`
/// is `Component.literal(uuid.toString())`; an unstyled literal component
/// serialises as a plain string. Read off the golden's bytes, having first
/// been written the other way.
///
/// **The uuid here is the dashed `UUID.toString()` form**, which is a
/// different encoding of the same number from the four-int array
/// [`display_name`] puts in its hover event. Same uuid, two wire forms, in
/// two replies to the same command.
fn name_and_id(name: &str) -> Nbt {
    compound([
        (
            "with",
            Nbt::List(vec![
                Nbt::String(name.to_string()),
                Nbt::String(dashed_uuid(name)),
            ]),
        ),
        ("translate", Nbt::String("commands.list.nameAndId".into())),
    ])
}

/// The offline uuid in `java.util.UUID.toString()` form: 8-4-4-4-12 lowercase
/// hex.
fn dashed_uuid(name: &str) -> String {
    let u = wire::offline_uuid(name);
    let hi = (u >> 64) as u64;
    let lo = u as u64;
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        (hi >> 32) as u32,
        ((hi >> 16) & 0xffff) as u16,
        (hi & 0xffff) as u16,
        ((lo >> 48) & 0xffff) as u16,
        lo & 0xffff_ffff_ffff
    )
}

/// A UUID as `TAG_Int_Array`, most significant word first.
fn uuid_ints(name: &str) -> Nbt {
    let uuid = wire::offline_uuid(name);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the four 32-bit halves of a u128, taken deliberately"
    )]
    let word = |shift: u32| ((uuid >> shift) as u32) as i32;
    Nbt::IntArray(vec![word(96), word(64), word(32), word(0)])
}

/// `commands.gamerule.query` — `/gamerule <rule>`'s reply.
///
/// `GameRuleCommand.queryRule` is
/// `Component.translatable("commands.gamerule.query", rule.id(), rule.serialize(value))`.
/// **Both arguments are plain strings**, not components:
/// `Component.translatable(String, Object...)` wraps a non-`Component`
/// argument with `Component.translationArg`, which for a `String` is a
/// literal component, and an unstyled literal serialises as a bare
/// `TAG_String` — the same reason `commands.list.nameAndId`'s two arguments
/// are bare strings.
///
/// `rule.id()` is the **bare** name, so `/gamerule minecraft:keep_inventory`
/// answers about `keep_inventory`. That is not a normalisation this server
/// applies: it is what `GameRule.id()` returns for both registered literals.
#[must_use]
pub fn gamerule_query_component(rule: &str, value: &str) -> Nbt {
    translated_pair("commands.gamerule.query", rule, value)
}

/// `commands.gamerule.set` — `/gamerule <rule> <value>`'s reply, in the same
/// shape as [`gamerule_query_component`] and with the same two arguments.
///
/// **`sendSuccess`'s second argument is `true` here**, which in vanilla also
/// logs the change to every other operator when `send_command_feedback` is
/// on. That broadcast is not ported: this server answers the caller only. It
/// is named here rather than in a changelog because a reader of the caller's
/// correct reply has no way to see the recipients that are missing.
#[must_use]
pub fn gamerule_set_component(rule: &str, value: &str) -> Nbt {
    translated_pair("commands.gamerule.set", rule, value)
}

/// `Component.translatable(key, a, b)` where both arguments are strings.
fn translated_pair(key: &str, a: &str, b: &str) -> Nbt {
    compound([
        (
            "with",
            Nbt::List(vec![Nbt::String(a.to_string()), Nbt::String(b.to_string())]),
        ),
        ("translate", Nbt::String(key.to_string())),
    ])
}

/// `ComponentUtils.formatList(players, DEFAULT_SEPARATOR, element)`.
///
/// `Component.empty()` with the names and separators appended, which
/// serialises as `{ extra: [...], text: "" }`. The separator is
/// `Component.literal(", ").withStyle(ChatFormatting.GRAY)`.
///
/// **One name is not a one-element list.** `formatList` returns the single
/// element's own component rather than wrapping it, so a one-player server
/// sends a bare element here. For `/list` nothing in the corpus exercises
/// that — the harness always joins two — but `/list uuids` now gates it: the
/// golden is taken at two players and the single-player branch is asserted
/// against `formatList`'s own shape in [`tests`].
fn joined(names: &[String], element: impl Fn(&str) -> Nbt) -> Nbt {
    match names {
        [] => compound([("text", Nbt::String(String::new()))]),
        [only] => element(only),
        many => {
            let mut extra = Vec::new();
            for (i, name) in many.iter().enumerate() {
                if i > 0 {
                    extra.push(compound([
                        ("color", Nbt::String("gray".into())),
                        ("text", Nbt::String(", ".into())),
                    ]));
                }
                extra.push(element(name));
            }
            compound([
                ("extra", Nbt::List(extra)),
                ("text", Nbt::String(String::new())),
            ])
        }
    }
}

/// `commands.list.players` around whichever element form the variant uses.
fn list_component(names: &[String], element: impl Fn(&str) -> Nbt) -> Nbt {
    let online = i32::try_from(names.len()).unwrap_or(i32::MAX);
    compound([
        (
            "with",
            Nbt::List(vec![
                primitive_arg(online),
                primitive_arg(MAX_PLAYERS),
                joined(names, element),
            ]),
        ),
        ("translate", Nbt::String("commands.list.players".into())),
    ])
}

/// The whole `/list` reply component.
///
/// `Component.translatable("commands.list.players", online, max, names)` —
/// **three** arguments in 26.2.
#[must_use]
pub fn list_players_component(names: &[String]) -> Nbt {
    list_component(names, display_name_component)
}

/// The whole `/list uuids` reply component.
///
/// Same outer translation, different element: `commands.list.nameAndId`
/// rather than a display name.
#[must_use]
pub fn list_uuids_component(names: &[String]) -> Nbt {
    list_component(names, name_and_id)
}

/// The whole `/seed` reply component.
///
/// `Component.translatable("commands.seed.success",
/// ComponentUtils.copyOnClickText(String.valueOf(seed)))`, where
/// `copyOnClickText` is a green literal with a copy-to-clipboard click, a
/// `chat.copy.click` hover and an insertion, **wrapped in
/// `chat.square_brackets`** — the brackets are a translation, not two literal
/// characters, and a port that appended `[` and `]` would render the same and
/// encode differently.
#[must_use]
pub fn seed_component(seed: i64) -> Nbt {
    let text = seed.to_string();
    let inner = compound([
        (
            "click_event",
            compound([
                ("action", Nbt::String("copy_to_clipboard".into())),
                ("value", Nbt::String(text.clone())),
            ]),
        ),
        ("color", Nbt::String("green".into())),
        ("insertion", Nbt::String(text.clone())),
        ("text", Nbt::String(text)),
        (
            "hover_event",
            compound([
                ("action", Nbt::String("show_text".into())),
                (
                    "value",
                    compound([("translate", Nbt::String("chat.copy.click".into()))]),
                ),
            ]),
        ),
    ]);
    let bracketed = compound([
        ("with", Nbt::List(vec![inner])),
        ("translate", Nbt::String("chat.square_brackets".into())),
    ]);
    compound([
        ("with", Nbt::List(vec![bracketed])),
        ("translate", Nbt::String("commands.seed.success".into())),
    ])
}

/// Encode a whole `system_chat` body, for the tests and for anything that
/// needs the bytes rather than the send.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn system_chat_body(component: &Nbt) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_network_nbt(&mut body, component)?;
    wire::write_bool(&mut body, false)?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commandtree::RefusalKind;

    /// The two names the gameplay corpus connects with, in join order.
    fn corpus_players() -> Vec<String> {
        vec!["oracle".to_string(), "oracle_eye".to_string()]
    }

    /// **Real vanilla's answer, byte for byte.**
    ///
    /// Recorded 2026-08-05 on the Threadripper with `RUSTMC_DUMP_PACKET=chat
    /// ./ci/t2-gameplay-subset.sh 110-server-answers-a-command.toml` against
    /// the pinned 26.2 server, corpus players `oracle` and `oracle_eye`. 539
    /// bytes: 538 of component and the `overlay` boolean.
    ///
    /// This is the check. `chat`'s projection digests the whole body, so
    /// anything short of these exact bytes is a divergence — and every key
    /// order in it is `java.util.HashMap` bucket order, which is what
    /// `rustmc_world::nbt`'s writer and `rustmc_data::java_hash_set` are for.
    /// A test that compared a decoded structure instead would pass on bytes
    /// vanilla would never send.
    const VANILLA_LIST_REPLY: &str = "\
0a090004776974680a000000030300000000000200030000000000140009000565787472610a0000\
00030a000b636c69636b5f6576656e74080006616374696f6e000f737567676573745f636f6d6d61\
6e64080007636f6d6d616e64000d2f74656c6c206f7261636c652000080009696e73657274696f6e\
00066f7261636c650800047465787400066f7261636c650a000b686f7665725f6576656e74080004\
6e616d6500066f7261636c65080006616374696f6e000b73686f775f656e74697479080002696400\
106d696e6563726166743a706c617965720b00047575696400000004501b21fe0ed23c3c9399e5f6\
9a092dcb0000080005636f6c6f720004677261790800047465787400022c20000a000b636c69636b\
5f6576656e74080006616374696f6e000f737567676573745f636f6d6d616e64080007636f6d6d61\
6e6400112f74656c6c206f7261636c655f6579652000080009696e73657274696f6e000a6f726163\
6c655f65796508000474657874000a6f7261636c655f6579650a000b686f7665725f6576656e7408\
00046e616d65000a6f7261636c655f657965080006616374696f6e000b73686f775f656e74697479\
080002696400106d696e6563726166743a706c617965720b00047575696400000004e8faa23a3617\
396d92f60b6e0838bec80000080004746578740000000800097472616e736c6174650015636f6d6d\
616e64732e6c6973742e706c61796572730000";

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// A path relative to the repo root — a lib test's cwd is the package
    /// directory. See `crate::power`'s copy.
    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// Every command a player may not use is answered with silence, and
    /// vanilla answers it with a message.
    ///
    /// This pins a **known divergence**, not a behaviour to keep. It is here
    /// because the gap is invisible from anywhere else: `handle` returning
    /// `false` looks identical to a command that was handled and produced
    /// nothing, and no scenario invokes a command it may not use, so nothing
    /// in the corpus has ever asked.
    ///
    /// `/seed` is the case that needs no unported command to demonstrate it.
    /// We *have* `/seed`; it requires GAMEMASTERS on a dedicated server, the
    /// corpus's players hold `Level::All`, and `Tree::resolve` refuses at the
    /// parent with `Unknown` exactly as `fillUsableCommands` prunes. So the
    /// same input, from the same player, gets a message from vanilla and
    /// nothing from us.
    ///
    /// # The size of it, which is not one command
    ///
    /// Read from `assets/minecraft/lang/en_us.json` in the pinned jar:
    ///
    /// ```text
    ///   command.unknown.command = "Unknown or incomplete command. See below for error"
    ///   command.context.here    = "<--[HERE]"
    /// ```
    ///
    /// and Brigadier's `dispatcherUnknownCommand` is what raises it. That is
    /// **one component pair**, and it is the observable answer for every one
    /// of the 89 roots on `docs/generated/command-surface.tsv` we have not
    /// ported, for every root a player lacks permission for, and for every
    /// typo. `docs/PARITY-ROADMAP.md` §82.6 has the leverage argument; the
    /// short version is that porting it is one job whose reach is the whole
    /// unported surface, and porting `/me` is one job whose reach is `/me`.
    ///
    /// It is still the expensive one. The message and the styled context
    /// component have to go byte-exact through `rest`, and inventing them
    /// would be worse than the silence, which is why the silence is
    /// deliberate and documented in the module header rather than accidental.
    #[test]
    fn a_command_the_player_may_not_use_is_answered_with_silence() {
        // The whole tree, at the level the corpus's players hold.
        let t = build_tree();
        assert!(
            t.resolve("list", Level::All).is_ok(),
            "/list requires nothing and must resolve"
        );
        assert!(
            t.resolve("seed", Level::All).is_err(),
            "/seed requires GAMEMASTERS; a level-0 source must not reach it"
        );
        assert!(
            t.resolve("seed", Level::Gamemasters).is_ok(),
            "and must reach it at the level that may"
        );
        // The same refusal, and it is `Unknown` rather than a permission
        // error: vanilla prunes the node out of the parse rather than
        // reporting it as forbidden, so telling the two apart on the wire
        // would itself be a divergence.
        assert_eq!(
            t.resolve("seed", Level::All).unwrap_err().kind(),
            RefusalKind::UnknownCommand,
            "pruned, not forbidden"
        );
        // An unported command reaches the identical state, which is why the
        // reply that closes this closes all of them at once.
        assert_eq!(
            t.resolve("gamerule", Level::All).unwrap_err().kind(),
            RefusalKind::UnknownCommand
        );
        assert_eq!(
            t.resolve("stop", Level::Owners).unwrap_err().kind(),
            RefusalKind::UnknownCommand
        );
    }

    /// The generated surface names exactly the roots this server registers.
    ///
    /// `ci/command-surface.py` derives the 90 roots a dedicated vanilla server
    /// registers and marks which of them we have. Its view of *our* side is
    /// read out of this file's `roots: vec![...]`, and a script that parses
    /// Rust is a script that can be wrong about Rust — the first version of it
    /// reported `uuids`, a child of `list`, as a root.
    ///
    /// So the direction that matters is checked here instead, against the
    /// tree the server actually builds. Adding a command to [`build_tree`]
    /// without regenerating the surface fails this test, which is the failure
    /// worth having: the alternative is a worklist that quietly stops
    /// counting the thing it exists to count.
    #[test]
    fn the_generated_surface_names_our_actual_roots() {
        let path = repo("docs/generated/command-surface.tsv");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} — run ci/command-surface.py --write ({e})",
                path.display()
            )
        });
        // By column *name*, not position. The first version of this read the
        // fourth field, and adding the cost columns to the generator silently
        // moved `ported` one to the right -- so the test failed for a reason
        // that had nothing to do with the claim it exists to make.
        let mut rows = text.lines().filter(|l| !l.starts_with('#'));
        let header: Vec<&str> = rows.next().expect("a header row").split('\t').collect();
        let ported = header
            .iter()
            .position(|c| *c == "ported")
            .expect("the surface has a `ported` column");
        let mut claimed: Vec<&str> = rows
            .filter_map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f.get(ported).copied() == Some("yes")).then(|| f[0])
            })
            .collect();
        claimed.sort_unstable();
        let mut actual: Vec<&str> = build_tree().roots.iter().map(|n| n.name).collect();
        actual.sort_unstable();
        assert_eq!(
            claimed, actual,
            "docs/generated/command-surface.tsv disagrees with build_tree(); \
             run ci/command-surface.py --write"
        );
    }

    /// The golden, or a loud failure. **Not a skip**: a skip is a gate that
    /// cannot fail, and these two tests are the only thing standing between
    /// `/list uuids` and a plausible-looking wrong component.
    fn golden() -> serde_json::Value {
        let path = repo("data/vanilla/commands_tree.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
        serde_json::from_str(&text).expect("the golden is json")
    }

    /// **`/version`, all ten lines, against vanilla's own components.**
    ///
    /// Nine of them are `VersionCommand.dumpVersion`'s own output, handed over
    /// by the consumer it feeds; the tenth is `commands.version.header`, which
    /// the executor sends before calling `dumpVersion` and which therefore
    /// never appeared in that dump. `ci/commands-golden.sh` carries both now.
    ///
    /// This is the strongest kind of vector available for a reply: the
    /// components are vanilla's objects through vanilla's stream codec, not a
    /// reading of the command.
    #[test]
    fn the_version_reply_matches_vanillas_codec() {
        let g = golden();
        let want: Vec<String> = g["versionLines"]
            .as_array()
            .expect("versionLines")
            .iter()
            .map(|v| v.as_str().expect("hex").to_string())
            .collect();
        assert_eq!(want.len(), 10, "the golden holds {} lines", want.len());
        let got = version_components();
        assert_eq!(got.len(), want.len());
        for (i, (line, expected)) in got.iter().zip(&want).enumerate() {
            // The golden's `encodeComponent` appends the `overlay` boolean, so
            // these are whole `system_chat` bodies and not bare components —
            // the same shape `replies` carries and the same shape this server
            // puts on the socket.
            let body = system_chat_body(line).expect("buf");
            assert_eq!(
                hex(&body),
                *expected,
                "line {i} ({}) does not match vanilla's bytes",
                VERSION_LINES[i].0
            );
        }
    }

    /// **`/worldborder`'s nine replies against vanilla's own components.**
    ///
    /// This is where [`crate::bordercmd::java_format_f`] is actually gated.
    /// The Java side builds each component with the *same*
    /// `String.format(Locale.ROOT, "%.Nf", …)` expression `WorldBorderCommand`
    /// uses, so a row disagrees when the formatter disagrees — and three of
    /// the nine carry values (`0.15` at one place, `2.5` at zero, `0.125` at
    /// two) where Rust's `{:.N}` gives a different string from Java's `%f`.
    /// A port that reached for the obvious formatting fails those three and
    /// passes the other six, which is exactly the shape a golden has to catch.
    #[test]
    fn the_worldborder_replies_match_vanillas_codec() {
        use crate::bordercmd as bc;
        let g = golden();
        let mut b = crate::worldborder::WorldBorder::default();

        // Each row is (golden key, the component this server would send).
        let rows: Vec<(&str, Nbt)> = vec![
            (
                "worldborderSetImmediate",
                translatable(
                    "commands.worldborder.set.immediate",
                    &[Arg::Text(bc::java_format_f(100.0, 1))],
                ),
            ),
            (
                "worldborderSetImmediateTie",
                translatable(
                    "commands.worldborder.set.immediate",
                    &[Arg::Text(bc::java_format_f(0.15, 1))],
                ),
            ),
            (
                "worldborderSetShrink",
                translatable(
                    "commands.worldborder.set.shrink",
                    &[
                        Arg::Text(bc::java_format_f(12.0, 1)),
                        Arg::Text(bc::format_ticks_to_seconds(20)),
                    ],
                ),
            ),
            (
                "worldborderSetGrow",
                translatable(
                    "commands.worldborder.set.grow",
                    &[
                        Arg::Text(bc::java_format_f(200.0, 1)),
                        Arg::Text(bc::format_ticks_to_seconds(7)),
                    ],
                ),
            ),
            // Through the executor, so the reply and the packet come from one
            // call rather than from a rebuild of the reply alone.
            (
                "worldborderCenter",
                bc::set_center(&mut b, 137.5, -41.5).expect("changed").reply,
            ),
            ("worldborderGet", {
                let mut probe = crate::worldborder::WorldBorder::default();
                probe.set_size(2.5);
                bc::get_size(&probe).reply
            }),
            (
                "worldborderWarningDistance",
                bc::set_warning_distance(&mut b, 9).expect("changed").reply,
            ),
            (
                "worldborderWarningTime",
                bc::set_warning_time(&mut b, 37).expect("changed").reply,
            ),
            (
                "worldborderDamageAmount",
                bc::set_damage_amount(&mut b, 0.125).expect("changed").reply,
            ),
            (
                "worldborderDamageBuffer",
                bc::set_damage_buffer(&mut b, 1.25).expect("changed").reply,
            ),
        ];
        assert_eq!(
            rows.len(),
            10,
            "nine reply shapes plus one tie-breaking row"
        );
        for (key, component) in rows {
            let want = g["replies"][key]
                .as_str()
                .unwrap_or_else(|| panic!("the golden has no `{key}` — run ci/commands-golden.sh"));
            let body = system_chat_body(&component).expect("buf");
            assert_eq!(hex(&body), want, "{key} does not match vanilla's bytes");
        }
    }

    /// **`/scoreboard objectives`' six replies against vanilla's own components.**
    ///
    /// Built on the Java side from a REAL `Objective` on a real `Scoreboard`,
    /// so `getFormattedDisplayName()` is vanilla's object rather than a
    /// reconstruction — which is the one of the four arguments a reading gets
    /// wrong. It is not the display name: it is
    /// `wrapInSquareBrackets(displayName.copy().withStyle(hover(name)))`, and
    /// a port that passed the plain name renders almost right and encodes
    /// nothing like it.
    ///
    /// `display.set` is the other vector worth naming: its two arguments are a
    /// `String` and a `Component`, so `ListTag.write`'s heterogeneous rule
    /// wraps both, while `display.cleared`'s lone `String` is not wrapped.
    #[test]
    fn the_scoreboard_replies_match_vanillas_codec() {
        use crate::scoreboardcmd as sb;
        let g = golden();
        let mut b = sb::ServerScoreboard::default();

        let added = sb::add_objective(&mut b, "kills", "dummy").expect("added");
        let shown = sb::set_display_slot(&mut b, "sidebar", "kills").expect("shown");
        let listed_one = sb::list_objectives(&b);
        let cleared = sb::clear_display_slot(&mut b, "sidebar").expect("cleared");
        let removed = sb::remove_objective(&mut b, "kills").expect("removed");
        let listed_none = sb::list_objectives(&b);

        for (key, component) in [
            ("scoreboardObjectiveAdd", added.reply),
            ("scoreboardDisplaySet", shown.reply),
            ("scoreboardListOne", listed_one.reply),
            ("scoreboardDisplayCleared", cleared.reply),
            ("scoreboardObjectiveRemove", removed.reply),
            ("scoreboardListEmpty", listed_none.reply),
        ] {
            let want = g["replies"][key]
                .as_str()
                .unwrap_or_else(|| panic!("the golden has no `{key}` — run ci/commands-golden.sh"));
            let body = system_chat_body(&component).expect("buf");
            assert_eq!(hex(&body), want, "{key} does not match vanilla's bytes");
        }
    }

    /// **The wrapping rule, from both sides, on the two lines that prove it.**
    ///
    /// `commands.version.data` has one `int` argument and is a bare `TAG_Int`
    /// on the wire; `commands.version.protocol` has an `int` and a `String`
    /// and both are wrapped in `{"": ...}`. A port that always wrapped gets
    /// the first wrong and a port that never wrapped gets the second wrong,
    /// and the byte comparison above would say so — this test says WHICH rule
    /// it is, so a future reader does not have to re-derive it from a hex
    /// diff.
    #[test]
    fn a_translation_argument_is_wrapped_only_in_a_mixed_list() {
        let homogeneous = translation_args(&[Arg::Int(4903)]);
        assert_eq!(
            homogeneous,
            Nbt::List(vec![Nbt::Int(4903)]),
            "a lone int argument is a bare TAG_Int"
        );
        let mixed = translation_args(&[Arg::Int(776), Arg::Text("0x308".into())]);
        assert_eq!(
            mixed,
            Nbt::List(vec![
                compound([("", Nbt::Int(776))]),
                compound([("", Nbt::String("0x308".into()))]),
            ]),
            "an int beside a string is two wrapped compounds"
        );
        // `wrapIfNeeded`'s re-wrap arm: a compound that is itself a wrapper is
        // wrapped again, so the reader knows which layer to strip. Nothing
        // this server builds hits it, which is exactly why it needs a test.
        let rewrapped =
            translation_args(&[Arg::Component(compound([("", Nbt::Int(1))])), Arg::Int(2)]);
        assert_eq!(
            rewrapped,
            Nbt::List(vec![
                compound([("", compound([("", Nbt::Int(1))]))]),
                compound([("", Nbt::Int(2))]),
            ])
        );
        // And a real component beside a primitive is NOT re-wrapped, which is
        // the `/list` reply's own shape.
        let real = compound([("text", Nbt::String("x".into()))]);
        assert_eq!(
            translation_args(&[Arg::Int(2), Arg::Component(real.clone())]),
            Nbt::List(vec![compound([("", Nbt::Int(2))]), real])
        );
    }

    /// A no-argument translatable has **no `with` key**, not an empty one.
    ///
    /// Two of `/version`'s ten lines are this shape and the byte comparison
    /// covers them; this names the rule.
    #[test]
    fn a_no_argument_translatable_has_no_with_key() {
        assert_eq!(
            translatable("commands.version.stable.yes", &[]),
            compound([(
                "translate",
                Nbt::String("commands.version.stable.yes".into())
            )])
        );
    }

    /// `/version` is registered, requires GAMEMASTERS, and resolves.
    ///
    /// The permission half matters: `VersionCommand.register`'s boolean is
    /// `CommandSelection.DEDICATED.includeDedicated` and passing the other one
    /// makes it `LEVEL_ALL`, which is the same mistake `/seed` has a test for.
    #[test]
    fn version_needs_gamemasters() {
        let t = tree();
        assert_eq!(
            t.resolve("version", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        assert_eq!(
            t.resolve("version", Level::Gamemasters).expect("gm").path,
            vec!["version"]
        );
    }

    /// `/tell` and `/w` reach `/msg`'s executor through the redirect, and the
    /// resolved path is `msg`'s — which is what lets [`handle`] have one arm
    /// for three roots.
    ///
    /// The three-token form is the one that matters: a redirect followed with
    /// only one argument left would be a `parse_nodes` bug that this
    /// notices and the tree golden does not, because the golden gates the
    /// packet and never runs a parse.
    #[test]
    fn tell_and_w_resolve_through_the_redirect_into_msg() {
        let t = tree();
        for text in [
            "msg oracle hi there",
            "tell oracle hi there",
            "w oracle hi there",
        ] {
            let r = t
                .resolve(text, Level::All)
                .unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
            assert_eq!(
                r.path,
                vec!["msg", "targets", "message"],
                "{text:?} did not land on /msg's executor"
            );
            assert_eq!(r.arg("targets"), Some("oracle"));
            assert_eq!(r.arg("message"), Some("hi there"));
        }
        // A redirect literal runs nothing of its own, and the refusal is
        // `Incomplete` rather than `Unknown`: `tell` is a real node with no
        // `Command`, which is `execute`'s `context.getCommand() == null` arm.
        assert_eq!(
            t.resolve("tell", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        // `EntityArgument.players()` refuses a selector that can name a
        // non-player, at parse time, before any executor sees it — and since
        // `targets` is the only candidate there, the refusal is that
        // argument's own `ERROR_ONLY_PLAYERS_ALLOWED` rather than an
        // unknown-argument.
        assert_eq!(
            t.resolve("msg @e hi", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::Argument),
            "@e names entities and /msg takes players only"
        );
        // And `single = false` means `@a` is allowed here, unlike a
        // single-target argument.
        assert!(t.resolve("msg @a hi", Level::All).is_ok());
    }

    /// **`/help`, against vanilla's own `getSmartUsage` over our own tree, at
    /// all five permission levels.**
    ///
    /// This is the strongest available check on a usage transcription: the
    /// expected strings are produced by `CommandDispatcher.getSmartUsage`
    /// itself, running on vanilla's copy of the tree this server registers, so
    /// nothing in the comparison is a restatement of the algorithm under test.
    ///
    /// Our tree happens to exercise every branch of that algorithm, which is
    /// not luck so much as what a mixed tree looks like:
    ///
    /// * `list [uuids]` — one child, and the parent is executable, so the
    ///   child is bracketed;
    /// * `me <action>` — one child, parent NOT executable, so it is not;
    /// * `gamerule (a|minecraft:a|b|…)` — many children with distinct deep
    ///   usages, parent not executable, so the alternation is parenthesised;
    /// * `tell -> msg` — the redirect arm;
    /// * `help [<command>]` — the `deep` recursion, which returns the child's
    ///   own text and stops;
    /// * `seed`, `version` — a leaf, which is just its own text.
    ///
    /// The permission filter is in it too: `say`, `seed`, `version` and
    /// `gamerule` are absent below GAMEMASTERS, so a `canUse` that had
    /// stopped filtering would show as five identical lists.
    #[test]
    fn the_help_lines_match_vanillas_own_usage_generator() {
        let g = golden();
        let rows = g["levels"].as_array().expect("levels");
        assert_eq!(rows.len(), 5);
        let mut seen: Vec<Vec<String>> = Vec::new();
        for (row, level) in rows.iter().zip(Level::ALL) {
            let want: Vec<String> = row["help"]
                .as_array()
                .unwrap_or_else(|| panic!("no help lines at {level:?}"))
                .iter()
                .map(|v| format!("/{}", v.as_str().expect("usage")))
                .collect();
            assert_eq!(
                help_lines(level),
                want,
                "/help at {level:?} does not match vanilla's getSmartUsage"
            );
            seen.push(want);
        }
        // A filter that stopped filtering would make all five equal, and every
        // assertion above would still pass.
        assert_ne!(
            seen[0], seen[2],
            "ALL and GAMEMASTERS produced the same /help, so the permission \
             filter is not being exercised"
        );
    }

    /// `/help <command>` enumerates that node's CHILDREN, so a leaf prints
    /// nothing and is still a success — and a redirect ends the walk, so
    /// `/help tell` is `tell`'s children, which is nothing at all.
    #[test]
    fn help_for_a_command_enumerates_its_children() {
        // `[uuids]` and not `uuids`: the public `getSmartUsage(node, source)`
        // passes `optional = node.getCommand() != null`, and `list` runs, so
        // everything under it is optional.
        assert_eq!(
            help_lines_for("list", Level::All),
            Some(vec!["list [uuids]".to_string()])
        );
        // A leaf: `getSmartUsage` over no children is an empty map, which
        // `HelpCommand` reports as 0 and prints nothing. Not an error.
        assert_eq!(help_lines_for("list uuids", Level::All), Some(Vec::new()));
        assert_eq!(help_lines_for("seed", Level::Gamemasters), Some(Vec::new()));
        // `ParseResults.getContext().getNodes()` is the ROOT context's list, so
        // a redirect ends the walk and the node is `tell` — which has no
        // children of its own.
        assert_eq!(help_lines_for("tell", Level::All), Some(Vec::new()));
        // The failure arm: nothing matched at all.
        assert_eq!(help_lines_for("nope", Level::All), None);
        // And the permission filter reaches here too.
        assert_eq!(help_lines_for("seed", Level::All), None);
        // The prefix is the whole argument as typed, not the node's name.
        assert_eq!(
            help_lines_for("msg", Level::All),
            Some(vec!["msg <targets> <message>".to_string()])
        );
    }

    #[test]
    fn the_list_reply_is_byte_identical_to_vanillas() {
        let body = system_chat_body(&list_players_component(&corpus_players())).expect("buf");
        let want: String = VANILLA_LIST_REPLY.split_whitespace().collect();
        assert_eq!(hex(&body), want);
        assert_eq!(body.len(), 539);
    }

    /// **`/list uuids`, against bytes vanilla's own component codec produced.**
    ///
    /// The inputs are fixed on the Java side — players `oracle` and
    /// `oracle_eye`, `max-players` 20 — and must be the same two this test
    /// passes, or the comparison is between two different questions.
    #[test]
    fn the_list_uuids_reply_matches_vanillas_codec() {
        let g = golden();
        assert_eq!(
            g["replyInputs"]["maxPlayers"].as_i64(),
            Some(i64::from(MAX_PLAYERS)),
            "the golden was taken at a different max-players"
        );
        let want = g["replies"]["listUuids"].as_str().expect("listUuids");
        let body = system_chat_body(&list_uuids_component(&corpus_players())).expect("buf");
        assert_eq!(hex(&body), want);
    }

    /// **`/seed`, against bytes vanilla's own component codec produced**, at
    /// both seeds the golden carries. Two seeds rather than one because a
    /// reply that ignored its argument would pass on one.
    #[test]
    fn the_seed_reply_matches_vanillas_codec() {
        let g = golden();
        for (key, seed_key) in [("seed", "seed"), ("seedAlt", "seedAlt")] {
            let want = g["replies"][key].as_str().expect("reply");
            let seed = g["replyInputs"][seed_key].as_i64().expect("seed");
            let body = system_chat_body(&seed_component(seed)).expect("buf");
            assert_eq!(hex(&body), want, "seed {seed}");
        }
        assert_ne!(
            g["replies"]["seed"], g["replies"]["seedAlt"],
            "the golden's two seeds must differ or this test cannot fail"
        );
    }

    /// `/gamerule`'s two replies, against vanilla's own component codec.
    ///
    /// Four vectors and not one: query and set, over a boolean rule and an
    /// integer one. `GameRule.serialize` is per type, and a golden taken on
    /// one rule would not see the other's; query and set differ only in the
    /// translation key, which is exactly the pair a copy-paste gets wrong.
    #[test]
    fn the_gamerule_replies_match_vanillas_codec() {
        let g = golden();
        /// (golden key, builder, rule, value).
        type Case = (
            &'static str,
            fn(&str, &str) -> Nbt,
            &'static str,
            &'static str,
        );
        let cases: [Case; 4] = [
            (
                "gameruleQueryBool",
                gamerule_query_component,
                "keep_inventory",
                "false",
            ),
            (
                "gameruleSetBool",
                gamerule_set_component,
                "keep_inventory",
                "true",
            ),
            (
                "gameruleQueryInt",
                gamerule_query_component,
                "random_tick_speed",
                "3",
            ),
            (
                "gameruleSetInt",
                gamerule_set_component,
                "random_tick_speed",
                "7",
            ),
        ];
        for (key, build, rule, value) in cases {
            let want = g["replies"][key].as_str().expect("reply");
            let body = system_chat_body(&build(rule, value)).expect("buf");
            assert_eq!(hex(&body), want, "{key}");
        }
        assert_ne!(
            g["replies"]["gameruleQueryBool"], g["replies"]["gameruleSetBool"],
            "query and set must differ, or the key is not being read"
        );
        assert_ne!(
            g["replies"]["gameruleSetBool"], g["replies"]["gameruleSetInt"],
            "two rules must differ, or the arguments are not being read"
        );
    }

    /// Every rule the tree offers is one `crate::gamerules` can answer for.
    ///
    /// The dispatch in [`handle`] refuses rather than panicking on a rule with
    /// no default, which is right for a connection thread and wrong as the
    /// only statement of the invariant: a refusal that fires is a command
    /// that silently stopped working. This is where the two tables are made
    /// to agree, in both directions.
    #[test]
    fn every_registered_rule_has_a_default_and_the_right_type() {
        use crate::gamerules::{GameRuleType, DEFAULTS};
        let defaults: std::collections::BTreeMap<&str, GameRuleType> =
            DEFAULTS.iter().map(|(n, v)| (*n, v.kind())).collect();
        for (short, namespaced, arg) in GAME_RULES {
            assert_eq!(
                namespaced,
                format!("minecraft:{short}"),
                "the namespaced form must be the bare one prefixed"
            );
            let kind = defaults
                .get(short)
                .unwrap_or_else(|| panic!("{short} is in the command tree and not in DEFAULTS"));
            let wanted = match arg {
                RuleArg::Bool => GameRuleType::Bool,
                RuleArg::Int(..) => GameRuleType::Int,
            };
            assert_eq!(*kind, wanted, "{short}");
        }
        // The other direction, with the one known exception named. If a rule
        // is dropped from GAME_RULES this fails; if vanilla adds one to the
        // vanilla feature set, `command_tree_vanilla` fails.
        let registered: std::collections::BTreeSet<&str> =
            GAME_RULES.iter().map(|(s, _, _)| *s).collect();
        let missing: Vec<&str> = DEFAULTS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !registered.contains(n))
            .collect();
        assert_eq!(
            missing,
            vec!["max_minecart_speed"],
            "the only rule in the registry and not on /gamerule is the \
             feature-flagged one"
        );
    }

    /// The offline UUID reaches the wire two ways in two replies, and both are
    /// pinned. Vanilla's own bytes for `oracle` are
    /// `501b21fe 0ed23c3c 9399e5f6 9a092dcb` — version nibble 3 and variant 9,
    /// which is `UUID.nameUUIDFromBytes`.
    #[test]
    fn the_uuid_has_two_wire_forms_and_both_are_vanillas() {
        let Nbt::IntArray(words) = uuid_ints("oracle") else {
            panic!("uuid is an int array");
        };
        assert_eq!(
            words,
            vec![
                0x501b_21fe_u32 as i32,
                0x0ed2_3c3c,
                0x9399_e5f6_u32 as i32,
                0x9a09_2dcb_u32 as i32
            ]
        );
        // The same number, as `UUID.toString()`.
        assert_eq!(
            dashed_uuid("oracle"),
            "501b21fe-0ed2-3c3c-9399-e5f69a092dcb"
        );
        assert_eq!(
            dashed_uuid("oracle_eye"),
            "e8faa23a-3617-396d-92f6-0b6e0838bec8"
        );
    }

    /// **`commands.list.players` takes three arguments in 26.2.** Asserted
    /// because it is the one thing about this reply that reading
    /// `ListPlayersCommand` would have got wrong.
    #[test]
    fn the_translation_takes_online_max_and_names() {
        for c in [
            list_players_component(&corpus_players()),
            list_uuids_component(&corpus_players()),
        ] {
            let with = c.get("with").and_then(Nbt::as_list).expect("with");
            assert_eq!(with.len(), 3);
            assert_eq!(with[0].get("").and_then(Nbt::as_i32), Some(2), "online");
            assert_eq!(with[1].get("").and_then(Nbt::as_i32), Some(20), "max");
            assert!(with[2].get("extra").is_some(), "the joined names");
        }
    }

    /// `formatList` does not wrap a single element, so one player is a bare
    /// element with no `extra` — in both variants.
    #[test]
    fn one_player_is_a_bare_element() {
        let one = joined(&["oracle".to_string()], display_name_component);
        assert!(one.get("extra").is_none());
        assert_eq!(one.get("text").and_then(Nbt::as_str), Some("oracle"));
        let one_uuid = joined(&["oracle".to_string()], name_and_id);
        assert!(one_uuid.get("extra").is_none());
        assert_eq!(
            one_uuid.get("translate").and_then(Nbt::as_str),
            Some("commands.list.nameAndId")
        );
        assert_eq!(
            joined(&[], display_name_component)
                .get("text")
                .and_then(Nbt::as_str),
            Some("")
        );
    }

    /// The element of `/list uuids` is two **bare strings**, not two
    /// compounds. This is the shape the golden corrected, so it is pinned
    /// separately from the byte test — if the byte test ever has to be
    /// regenerated, this one still says what the shape was.
    #[test]
    fn name_and_id_arguments_are_bare_strings() {
        let e = name_and_id("oracle");
        let with = e.get("with").and_then(Nbt::as_list).expect("with");
        assert_eq!(with.len(), 2);
        assert_eq!(with[0].as_str(), Some("oracle"));
        assert_eq!(
            with[1].as_str(),
            Some("501b21fe-0ed2-3c3c-9399-e5f69a092dcb")
        );
        assert!(with[0].get("text").is_none(), "not {{text: ...}}");
    }

    /// The registry replaced the whole-text match: `list uuids` is now its
    /// own node with its own answer, and the two answers differ.
    #[test]
    fn list_and_list_uuids_are_different_commands() {
        let t = tree();
        assert_eq!(
            t.resolve("list", Level::All).expect("list").path,
            vec!["list"]
        );
        assert_eq!(
            t.resolve("list uuids", Level::All).expect("uuids").path,
            vec!["list", "uuids"]
        );
        assert_ne!(
            list_players_component(&corpus_players()),
            list_uuids_component(&corpus_players())
        );
        assert_eq!(read_command(&packet("list")).expect("read"), "list");
        assert_eq!(
            read_command(&packet("list uuids")).expect("read"),
            "list uuids"
        );
    }

    fn packet(text: &str) -> Vec<u8> {
        let mut b = Vec::new();
        wire::write_string(&mut b, text).expect("buf");
        b
    }

    /// Every executable node in the tree has a reply arm. This is the test
    /// that would have caught a `Node` added without one — the defect the
    /// `other =>` arm in [`handle`] exists to survive.
    /// The executable paths of the roots `crate::cmdexec` answers:
    /// `/difficulty` 5 (the query and four literals), `/gamemode` 2,
    /// `/teleport` 4 (`tp` is a redirect and adds none), `/weather` 6, and
    /// `/experience` 8 (`add` and `set` each three — `amount`, `points`,
    /// `levels` — and `query` two; `xp` is a redirect), `/defaultgamemode` 1,
    /// `/title` 6 (`clear`, `reset`, `times … fadeOut`, three component
    /// forms), `/tellraw` 1, `/random` 2, `/rotate` 1, `/swing` 4,
    /// `/particle` 7, `/teammsg` 1, `/dialog clear` 1, `/bossbar` 25 (add,
    /// remove, list, set's 18 — name, seven colours, five styles, value,
    /// max, visible, players with and without targets — and get's four),
    /// and
    /// `/time`'s subset 4 (`set`, `add`, `query time`, `query gametime`), and
    /// `/setblock` 5 (`block` and its four modes), and `/fill` 7 (`block` and
    /// its six modes), and `/playsound` 67 (`sound`, then six depths under
    /// each of the eleven sources), and `/stopsound` 24 (`targets`, `* sound`,
    /// and each source with and without a sound), and `/clone`'s subset 18.
    const CMDEXEC_PATHS: usize = 5
        + 2
        + 5
        + 6
        + 8
        + 1
        + 6
        + 4
        + 5
        + 7
        + 1
        + 11 * 6
        + 1
        + 1
        + 11 * 2
        + 18
        + 1
        + 2
        + 1
        + 4
        + 7
        + 1
        + 1
        + 25;
    // `/execute` adds none: every node of its subset is a redirect or a
    // non-executable literal; what runs is the command after `run`.

    #[test]
    fn every_executable_node_has_a_reply() {
        fn walk(nodes: &[Node], prefix: &mut Vec<&'static str>, out: &mut Vec<Vec<&'static str>>) {
            for n in nodes {
                prefix.push(n.name);
                if n.executable {
                    out.push(prefix.clone());
                }
                walk(&n.children, prefix, out);
                prefix.pop();
            }
        }
        let mut paths = Vec::new();
        walk(&tree().roots, &mut Vec::new(), &mut paths);
        assert_eq!(
            paths.len(),
            9 + 116 * 2 + 10 + 5 + CMDEXEC_PATHS,
            "list, list uuids, seed, version, say, me, msg, help and help \
             command, a query and a set for each of the 116 /gamerule \
             literals, and /worldborder's TEN executable paths — `add` and \
             `set` each with and without a time, `center pos`, `damage \
             amount`, `damage buffer`, `get`, `warning distance` and `warning \
             time`. `tell` and `w` are NOT here: a redirect literal has no \
             `Command` of its own, so the walk finds nothing executable under \
             either, and neither is the `worldborder` root, which `register` \
             never calls `executes` on. And /scoreboard's FIVE: `objectives \
             list`, `objectives add … criteria`, `objectives remove …`, and \
             `objectives setdisplay` with and without an objective."
        );
        for p in paths {
            let answered = matches!(
                p.as_slice(),
                ["list"]
                    | ["list", "uuids"]
                    | ["seed"]
                    | ["version"]
                    | ["say", "message"]
                    | ["me", "action"]
                    | ["msg", "targets", "message"]
                    | ["help"]
                    | ["help", "command"]
                    | ["gamerule", _]
                    | ["gamerule", _, "value"]
                    | ["worldborder", "add" | "set", "distance"]
                    | ["worldborder", "add" | "set", "distance", "time"]
                    | ["worldborder", "center", "pos"]
                    | ["worldborder", "damage", "amount", "damagePerBlock"]
                    | ["worldborder", "damage", "buffer", "distance"]
                    | ["worldborder", "get"]
                    | ["worldborder", "warning", "distance", "distance"]
                    | ["worldborder", "warning", "time", "time"]
                    | ["scoreboard", "objectives", "list"]
                    | ["scoreboard", "objectives", "add", "objective", "criteria"]
                    | ["scoreboard", "objectives", "remove", "objective"]
                    | ["scoreboard", "objectives", "setdisplay", "slot"]
                    | [
                        "scoreboard",
                        "objectives",
                        "setdisplay",
                        "slot",
                        "objective"
                    ]
            );
            assert!(
                answered || crate::cmdexec::owns(&p),
                "{p:?} is declared executable with no reply"
            );
        }
    }

    /// A refusal is a refusal, not a wrong answer: nothing is sent.
    #[test]
    fn an_unknown_command_resolves_to_nothing() {
        let t = tree();
        assert_eq!(
            t.resolve("nope", Level::Owners).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        // `list uuids` matched and `x` did not, so this is
        // `command.unknown.argument` — the same input as `nope` in every
        // respect except that a prefix of it is a real command.
        assert_eq!(
            t.resolve("list uuids x", Level::Owners)
                .map_err(Refusal::kind),
            Err(RefusalKind::UnknownArgument)
        );
    }
}
