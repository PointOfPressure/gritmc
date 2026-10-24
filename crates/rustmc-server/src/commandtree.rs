//! The Brigadier command tree, its permission model, and
//! `ClientboundCommandsPacket`.
//!
//! This is the half of the command registry that is *shape* rather than
//! *content*: what a node is, who may use one, how the tree is numbered, and
//! how it reaches the wire. [`crate::commands`] holds the commands themselves
//! and their replies.
//!
//! # What this covers and what it refuses
//!
//! Stated first, because the defect this project repeats is a partial
//! implementation that reads complete from the call site.
//!
//! **Covered:** literal nodes, arbitrary nesting, per-node permission levels,
//! vanilla's `fillUsableCommands` filtering, the `RESTRICTED` flag, the
//! breadth-first node numbering, and the exact `ClientboundCommandsPacket`
//! body. Gated byte-for-byte against bytes produced by *vanilla's own*
//! encoder — see `ci/commands-golden.sh`.
//!
//! **Refused, loudly rather than silently:**
//!
//! * **Argument nodes.** `ClientboundCommandsPacket$ArgumentNodeStub.write`
//!   writes the argument's name, then `serializeCap`, which dispatches to one
//!   of the ~50 `ArgumentTypeInfo` serialisers, then an optional suggestion
//!   id. None of that is implemented. [`Node`] has no argument variant at
//!   all, so this is a type error rather than a wrong byte — the failure mode
//!   is "it will not compile", which is the only kind worth having here.
//! * **Redirects** (`FLAG_REDIRECT`, 8). Vanilla uses them for
//!   `/execute`-style chaining. [`Node`] cannot express one.
//! * **Custom suggestions** (`FLAG_CUSTOM_SUGGESTIONS`, 16). An argument-node
//!   flag, so it is refused with argument nodes.
//!
//! A tree of literals is not a toy: it is exactly what `/list`, `/list uuids`
//! and `/seed` are in vanilla, and it is the whole of what this server
//! registers today.
//!
//! # The permission model in 26.2 is not the one people remember
//!
//! Older accounts of this protocol — and any memory of `hasPermission(int)` —
//! describe a bare integer op level. 26.2 replaced it with
//! `net.minecraft.server.permissions`: `Permission`, `PermissionSet`,
//! `PermissionCheck`. Read from the jar rather than assumed:
//!
//! ```text
//!   Commands.LEVEL_ALL         = PermissionCheck$AlwaysPass.INSTANCE
//!   Commands.LEVEL_MODERATORS  = Require(Permissions.COMMANDS_MODERATOR)
//!   Commands.LEVEL_GAMEMASTERS = Require(Permissions.COMMANDS_GAMEMASTER)
//!   Commands.LEVEL_ADMINS      = Require(Permissions.COMMANDS_ADMIN)
//!   Commands.LEVEL_OWNERS      = Require(Permissions.COMMANDS_OWNER)
//! ```
//!
//! and each of those four `Permissions` constants is
//! `new Permission$HasCommandLevel(PermissionLevel.X)` for the matching
//! `PermissionLevel` — `ALL`=0, `MODERATORS`=1, `GAMEMASTERS`=2, `ADMINS`=3,
//! `OWNERS`=4. So the *observable* model is still the five levels, and that
//! is what [`Level`] is. **The indirection matters for one thing and it is
//! not cosmetic:** `LEVEL_ALL` is `AlwaysPass`, not `Require(level >= 0)`, so
//! it passes for a source holding `PermissionSet.NO_PERMISSIONS` while every
//! other level fails. That is precisely the predicate `isRestricted` uses,
//! and it is why [`Level::All`] is the only level that is not restricted.
//!
//! 26.2 also declares four *chat* permissions — `chat/send_messages`,
//! `chat/send_commands`, `chat/receive_player_messages`,
//! `chat/receive_system_messages`. **They are referenced by no other class in
//! the server jar** (swept 2026-08-06 over all 7,434 `net/minecraft` classes,
//! with a positive control on `COMMANDS_ADMIN`, which hits `Player` and
//! `Commands`). They are declared and unused, so chat is *not*
//! permission-gated in 26.2 and this server does not gate it either.
//!
//! # Numbering, and why child order is load-bearing
//!
//! `ClientboundCommandsPacket.enumerateNodes` is a breadth-first walk with an
//! `ArrayDeque`: the root is polled first and takes index 0, then each node's
//! `getChildren()` is enqueued whole, then its redirect. An index is assigned
//! as `map.size()` at the moment a node is first seen.
//!
//! Brigadier's `CommandNode.children` is a **`LinkedHashMap`** (checked in
//! `brigadier-1.3.10.jar`, ctor offsets 5..12), so `getChildren()` yields
//! *insertion* order — registration order. Registering `seed` before `list`
//! is therefore a different packet, byte for byte, and both are valid. Ours
//! must match vanilla's registration order because the golden was taken from
//! vanilla registering in that order.
//!
//! `createEntries` then writes each entry at its own index, so the entry list
//! is in index order regardless of the hash map it was collected from.
//!
//! # The golden cannot see everything, because our tree is degenerate
//!
//! Worth reading before adding a test here, because it cost two real misses
//! in one falsification sweep on 2026-08-06.
//!
//! `ci/commands-golden.sh` gates the bytes of **our** tree against vanilla's
//! own encoder. That is a strong gate for everything our tree can express and
//! **silent** for everything it cannot — and this server's tree is a
//! remarkably poor exercise of its own encoder:
//!
//! * it has exactly **one branching parent** (`list`), so breadth-first and
//!   depth-first numbering produce identical output and the traversal
//!   discipline is unobservable;
//! * every node is either level 0 with level-0 children or level 2 with **no
//!   children**, so there is no usable parent with an unusable child and the
//!   *recursive* half of the permission filter is never asked a question it
//!   could answer wrongly;
//! * it is two levels deep, has no redirects and no arguments.
//!
//! Both of the first two were injected by the sweep and neither the golden
//! nor the then-existing unit tests caught them, because those tests used
//! fixtures shaped like the production tree. The fixes are
//! [`tests::numbering_is_breadth_first_not_depth_first`] and
//! [`tests::a_usable_parent_does_not_carry_an_unusable_child`], both of which
//! build **synthetic** trees chosen to separate the behaviours.
//!
//! So: a property of this encoder is gated by the golden only if our three
//! nodes happen to distinguish it. Everything else needs a fixture built to
//! falsify, and a fixture that mirrors production is the one shape guaranteed
//! not to.

use crate::cmdreader::{Reader, Syntax, SyntaxArgs};
use rustmc_proto::varint::write_varint;
use rustmc_proto::wire;
use std::io;

/// How many redirect hops one parse may take.
///
/// Vanilla has no such constant: its recursion is bounded by the input
/// running out, because a redirect is only followed after `canRead(1)` and
/// every hop consumes at least the separator. That argument holds here too —
/// this is a belt on the braces, so that a redirect added later which
/// consumes nothing cannot spin a connection thread. Vanilla's own deepest
/// chain is `/execute run execute run …`, which is bounded by the 256-char
/// command limit and cannot approach this.
const MAX_REDIRECT_DEPTH: usize = 64;

/// `net.minecraft.server.permissions.PermissionLevel`.
///
/// The discriminants are the enum's own ordinals, which is not decoration:
/// `PlayerList.sendPlayerPermissionLevel` switches on `level().ordinal()` and
/// emits entity-event byte `24 + ordinal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    All = 0,
    Moderators = 1,
    Gamemasters = 2,
    Admins = 3,
    Owners = 4,
}

impl Level {
    /// Every level, in ordinal order.
    pub const ALL: [Self; 5] = [
        Self::All,
        Self::Moderators,
        Self::Gamemasters,
        Self::Admins,
        Self::Owners,
    ];

    /// The `ClientboundEntityEventPacket` byte that tells a client its own
    /// permission level.
    ///
    /// `PlayerList.sendPlayerPermissionLevel` offsets 66..86: the five arms
    /// push 24, 25, 26, 27, 28 in ordinal order. Sent **before**
    /// `sendCommands`, and sent to the player's own connection only.
    #[must_use]
    pub const fn entity_event_byte(self) -> u8 {
        24 + self as u8
    }

    /// `server.properties`' `op-permission-level`, or a level from an op
    /// entry, clamped into range.
    ///
    /// Out-of-range is clamped rather than refused because that is what a
    /// misconfigured properties file should do to a server: nothing.
    #[must_use]
    pub const fn from_ordinal(n: i32) -> Self {
        match n {
            ..=0 => Self::All,
            1 => Self::Moderators,
            2 => Self::Gamemasters,
            3 => Self::Admins,
            _ => Self::Owners,
        }
    }

    /// Whether a source at this level may use a node requiring `required`.
    ///
    /// `Permission$HasCommandLevel` is satisfied by a command level at or
    /// above the one it names, and `AlwaysPass` by anything.
    #[must_use]
    pub fn allows(self, required: Self) -> bool {
        self >= required
    }
}

/// One literal node.
///
/// There is deliberately no argument variant; see the module header.
#[derive(Debug, Clone)]
pub struct Node {
    /// The literal itself — `LiteralCommandNode.getLiteral()`, which is what
    /// `LiteralNodeStub.write` puts on the wire.
    pub name: &'static str,
    /// The permission this node's Brigadier `requires` predicate demands.
    pub level: Level,
    /// Whether the node has a `Command` attached — `isExecutable`, flag 4.
    ///
    /// Separate from having children: `list` is both executable and a parent,
    /// and vanilla's `/list` is exactly that.
    pub executable: bool,
    /// `None` makes this a `LiteralCommandNode`; `Some` makes it an
    /// `ArgumentCommandNode` whose [`Node::name`] is the argument's name.
    pub parser: Option<Parser>,
    pub children: Vec<Node>,
    /// A redirect target, named as the path of literals from the root that
    /// reaches it — `["execute"]`, not an index.
    ///
    /// A builder tree has no stable indices to point at, and a path is the
    /// only form a registration can write down. [`Tree::lower`] resolves it
    /// against the built graph and **panics on a path that names no node**,
    /// because a redirect that silently became `None` is a packet whose
    /// `/execute`-style chaining stops working with nothing said.
    pub redirect: Option<&'static [&'static str]>,
}

impl Node {
    /// A literal that runs something, at [`Level::All`].
    #[must_use]
    pub fn leaf(literal: &'static str) -> Self {
        Self {
            name: literal,
            level: Level::All,
            executable: true,
            parser: None,
            children: Vec::new(),
            redirect: None,
        }
    }

    /// Raise the permission this node demands.
    #[must_use]
    pub fn requires(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// `LiteralArgumentBuilder.redirect(node)` — a literal that runs nothing
    /// of its own and continues the parse at another root's children.
    ///
    /// **Not executable**, which is what `redirect` produces: `MsgCommand`
    /// registers `tell` and `w` this way and neither has a `Command`. A
    /// redirect that was also executable would be a different node and a
    /// different flag byte.
    #[must_use]
    pub fn redirect_to(literal: &'static str, target: &'static [&'static str]) -> Self {
        let mut n = Self::leaf(literal);
        n.executable = false;
        n.redirect = Some(target);
        n
    }

    /// `LiteralArgumentBuilder.then` — append a child, preserving order.
    #[must_use]
    pub fn then(mut self, child: Self) -> Self {
        self.children.push(child);
        self
    }

    /// `Commands.COMMAND_NODE_INSPECTOR.isRestricted`: the node's requirement
    /// tested against a `PermissionSet.NO_PERMISSIONS` source.
    ///
    /// `LEVEL_ALL` is `AlwaysPass` and passes; every `Require(...)` fails,
    /// because `NO_PERMISSIONS` holds nothing. So the bit is set for every
    /// level above [`Level::All`] — and note this is computed against a fixed
    /// no-permission source, **not** against the receiving player. A node the
    /// player *can* use is still flagged restricted if anyone could not.
    #[must_use]
    pub fn restricted(&self) -> bool {
        self.level != Level::All
    }
}

/// Flag bits, read off `ClientboundCommandsPacket.createEntry`'s own `ior`s
/// rather than from any table: 8 at offsets 9..13, 4 at 42..45, 32 at 56..60,
/// the two-bit type in the `typeSwitch` arms (0 root, 1 literal, 2 argument)
/// and 16 immediately after the argument arm builds its stub.
const FLAG_EXECUTABLE: u8 = 4;
const FLAG_REDIRECT: u8 = 8;
const FLAG_CUSTOM_SUGGESTIONS: u8 = 16;
const FLAG_RESTRICTED: u8 = 32;
const TYPE_ROOT: u8 = 0;
const TYPE_LITERAL: u8 = 1;
const TYPE_ARGUMENT: u8 = 2;

// ===========================================================================
// ARGUMENT NODES — the vocabulary `ClientboundCommandsPacket` needs
// ===========================================================================
//
// Everything below was refused by this module until 2026-09-05. What replaced
// the refusal is not "arguments now work": it is that the *serialisers* now
// work, at vanilla's own scale, and are gated by
// `crates/rustmc-server/tests/command_tree_vanilla.rs` against the bytes
// vanilla's own encoder produces for vanilla's whole 2,085-node tree at all
// five permission levels.
//
// **That gate says nothing about executors and must not be read as if it
// did.** A tree with every node placed and every property byte right
// autocompletes perfectly and does nothing. `docs/EXTRACTABILITY.md` §2.1 is
// the measurement this implements and it says so in the same words.

/// `brigadier:string`'s `StringType`, whose ordinal is what reaches the wire.
///
/// `StringArgumentSerializer.serializeToNetwork` is a bare
/// `buf.writeEnum(template.type)`, and `FriendlyByteBuf.writeEnum` is
/// `writeVarInt(e.ordinal())`. So these three discriminants are the wire
/// values, not a local convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringKind {
    /// `SINGLE_WORD`, which `serializeToJson` names `"word"`.
    Word = 0,
    /// `QUOTABLE_PHRASE`, `"phrase"`.
    Phrase = 1,
    /// `GREEDY_PHRASE`, `"greedy"`.
    Greedy = 2,
}

/// What an argument parser writes after its registry id, if anything.
///
/// # Why this is one enum and not fifty-five implementations
///
/// 55 distinct parsers appear in vanilla's tree (`ci/command-tree-extract.sh`,
/// 2026-09-05) across 1,084 argument nodes. **47 of them serialise nothing at
/// all** — the whole of `minecraft:block_pos`, `minecraft:nbt_path`,
/// `minecraft:component`, `minecraft:item_stack` and forty-three others is
/// their registry id. The remaining eight fall into five shapes:
///
/// ```text
///   number with optional bounds   brigadier:integer/long/float/double   251 nodes
///   registry key                  minecraft:resource{,_key,_or_tag,      34 nodes
///                                 _or_tag_key,_selector}
///   entity selector flags         minecraft:entity                      118 nodes
///   score-holder flags            minecraft:score_holder                 27 nodes
///   string kind                   brigadier:string                       18 nodes
///   a bare i32 minimum            minecraft:time                         16 nodes
/// ```
///
/// That is leverage 6.9 on the parsers that carry properties at all and
/// effectively unbounded on the 47 that do not, which is the claim
/// `docs/EXTRACTABILITY.md` §2.1 makes and this type is the shape of.
///
/// # The sentinel rule, which is the one thing easy to get wrong
///
/// `IntegerArgumentInfo.serializeToNetwork` compares `min` against
/// `Integer.MIN_VALUE` and `max` against `Integer.MAX_VALUE` and sets bit 1 /
/// bit 2 of a flag byte only for a bound that differs. So an argument declared
/// `integer(Integer.MIN_VALUE, 40)` writes **only** the max. `Option::None`
/// here means "is the sentinel", which is exactly the state
/// `ArgumentTypeInfo.serializeToJson` expresses by omitting the key — and the
/// reason the extract dumps JSON rather than bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Props {
    /// The 47 parsers whose `serializeToNetwork` writes nothing.
    Empty,
    /// `brigadier:integer`. `min` `None` is `i32::MIN`, `max` `None` is `i32::MAX`.
    Int(Option<i32>, Option<i32>),
    /// `brigadier:long`. Sentinels `i64::MIN` / `i64::MAX`.
    Long(Option<i64>, Option<i64>),
    /// `brigadier:float`. Sentinels `-f32::MAX` / `f32::MAX` — **not**
    /// infinity, and not `f32::MIN`, which in Rust is the smallest normal
    /// magnitude rather than the most negative value. Java's
    /// `-Float.MAX_VALUE` is `-3.4028235E38`.
    Float(Option<f32>, Option<f32>),
    /// `brigadier:double`. Sentinels `-f64::MAX` / `f64::MAX`.
    Double(Option<f64>, Option<f64>),
    /// `brigadier:string`.
    Str(StringKind),
    /// `minecraft:entity`. Bit 1 is `single`, bit 2 is `playersOnly`.
    Entity { single: bool, players_only: bool },
    /// `minecraft:score_holder`. Bit 1 is `multiple`.
    ScoreHolder { multiple: bool },
    /// The five resource parsers, each of which writes its registry's
    /// identifier through `writeResourceKey` -> `writeIdentifier` -> a String.
    Registry(&'static str),
    /// `minecraft:time`, which writes a bare big-endian i32 minimum with no
    /// flag byte and no sentinel — `{"min": -2147483648}` is written out in
    /// full, and two nodes in vanilla's tree carry exactly that.
    TimeMin(i32),
}

impl Props {
    /// Append this parser's properties, after its registry id.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    fn write(&self, out: &mut Vec<u8>) -> io::Result<()> {
        /// `ArgumentUtils.createNumberFlags(hasMin, hasMax)` — bit 1, bit 2.
        fn number_flags(has_min: bool, has_max: bool) -> u8 {
            u8::from(has_min) | (u8::from(has_max) << 1)
        }
        match self {
            Self::Empty => {}
            Self::Int(min, max) => {
                wire::write_u8(out, number_flags(min.is_some(), max.is_some()))?;
                if let Some(v) = min {
                    out.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Self::Long(min, max) => {
                wire::write_u8(out, number_flags(min.is_some(), max.is_some()))?;
                if let Some(v) = min {
                    out.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Self::Float(min, max) => {
                wire::write_u8(out, number_flags(min.is_some(), max.is_some()))?;
                if let Some(v) = min {
                    out.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Self::Double(min, max) => {
                wire::write_u8(out, number_flags(min.is_some(), max.is_some()))?;
                if let Some(v) = min {
                    out.extend_from_slice(&v.to_be_bytes());
                }
                if let Some(v) = max {
                    out.extend_from_slice(&v.to_be_bytes());
                }
            }
            Self::Str(kind) => write_varint(out, *kind as i32)?,
            Self::Entity {
                single,
                players_only,
            } => wire::write_u8(out, u8::from(*single) | (u8::from(*players_only) << 1))?,
            Self::ScoreHolder { multiple } => wire::write_u8(out, u8::from(*multiple))?,
            Self::Registry(id) => wire::write_string(out, id)?,
            Self::TimeMin(min) => out.extend_from_slice(&min.to_be_bytes()),
        }
        Ok(())
    }
}

/// Why one argument node's parse did not produce a value.
///
/// The two arms are the distinction [`Refusal`] carries one level up and they
/// are kept apart for the same reason: a parser this server has not
/// transcribed is a statement about this server, and reporting it as a syntax
/// error would be a silent claim that the player's input was bad.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArgError {
    /// Brigadier raised a `CommandSyntaxException` here, with the translation
    /// key and arguments the message needs — or [`Syntax::Unmapped`] where the
    /// argument type is one whose keys this server has not transcribed.
    Syntax(Syntax),
    /// This server cannot read this parser at all.
    Unsupported,
}

impl Parser {
    /// Run this parser at the reader's cursor, exactly as vanilla's
    /// `ArgumentType.parse(StringReader)` does.
    ///
    /// On success the cursor is left immediately after the argument — which
    /// is the *whole* point of doing it this way rather than by splitting on
    /// whitespace. `minecraft:block_pos` advances past three coordinates,
    /// `brigadier:string` in `GREEDY_PHRASE` advances to the end of the line,
    /// and `brigadier:integer` advances past a number that may be followed by
    /// no space at all (which the next node then refuses, exactly as
    /// `dispatcherExpectedArgumentSeparator` does).
    ///
    /// On a syntax failure the cursor is wherever the failing reader left it.
    /// Callers must save and restore it themselves; `parse_nodes` does,
    /// because Brigadier does — it hands each candidate node a **copy** of
    /// the reader.
    ///
    /// # The refusal is the coverage statement
    ///
    /// [`ArgError::Unsupported`] is returned for every parser not listed
    /// below, and the list is the only place that coverage is written down.
    /// Adding a parser here without a golden vector is how this server would
    /// acquire a tokeniser that agrees with nothing;
    /// `ci/command-parse-golden.sh` is the gate.
    ///
    /// # Errors
    /// [`ArgError::Syntax`] where vanilla raises, [`ArgError::Unsupported`]
    /// for a parser this server has not transcribed.
    pub fn parse(&self, r: &mut Reader<'_>) -> Result<(), ArgError> {
        match (self.registry, &self.props) {
            // `BoolArgumentType.parse` -> `reader.readBoolean()`.
            ("brigadier:bool", _) => r.read_boolean().map(|_| ()).map_err(ArgError::Syntax),
            // `IntegerArgumentType.parse`: read, then bounds-check against
            // the DECLARED bounds, rewinding to the start before raising.
            ("brigadier:integer", Props::Int(min, max)) => {
                let start = r.cursor();
                let v = r.read_int().map_err(ArgError::Syntax)?;
                // `setCursor(start)` BEFORE raising, and the message carries
                // the BOUND first and the value second — see
                // [`SyntaxArgs::Ints`].
                if let Some(m) = *min {
                    if v < m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.integer.low",
                            args: SyntaxArgs::Ints(m, v),
                        }));
                    }
                }
                if let Some(m) = *max {
                    if v > m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.integer.big",
                            args: SyntaxArgs::Ints(m, v),
                        }));
                    }
                }
                Ok(())
            }
            ("brigadier:long", Props::Long(min, max)) => {
                let start = r.cursor();
                let v = r.read_long().map_err(ArgError::Syntax)?;
                // `setCursor(start)` BEFORE raising, and the message carries
                // the BOUND first and the value second — see
                // [`SyntaxArgs::Ints`].
                if let Some(m) = *min {
                    if v < m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.long.low",
                            args: SyntaxArgs::Longs(m, v),
                        }));
                    }
                }
                if let Some(m) = *max {
                    if v > m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.long.big",
                            args: SyntaxArgs::Longs(m, v),
                        }));
                    }
                }
                Ok(())
            }
            ("brigadier:float", Props::Float(min, max)) => {
                let start = r.cursor();
                let v = r.read_float().map_err(ArgError::Syntax)?;
                // `setCursor(start)` BEFORE raising, and the message carries
                // the BOUND first and the value second — see
                // [`SyntaxArgs::Ints`].
                if let Some(m) = *min {
                    if v < m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.float.low",
                            args: SyntaxArgs::Floats(m, v),
                        }));
                    }
                }
                if let Some(m) = *max {
                    if v > m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.float.big",
                            args: SyntaxArgs::Floats(m, v),
                        }));
                    }
                }
                Ok(())
            }
            ("brigadier:double", Props::Double(min, max)) => {
                let start = r.cursor();
                let v = r.read_double().map_err(ArgError::Syntax)?;
                // `setCursor(start)` BEFORE raising, and the message carries
                // the BOUND first and the value second — see
                // [`SyntaxArgs::Ints`].
                if let Some(m) = *min {
                    if v < m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.double.low",
                            args: SyntaxArgs::Doubles(m, v),
                        }));
                    }
                }
                if let Some(m) = *max {
                    if v > m {
                        r.set_cursor(start);
                        return Err(ArgError::Syntax(Syntax::Known {
                            key: "argument.double.big",
                            args: SyntaxArgs::Doubles(m, v),
                        }));
                    }
                }
                Ok(())
            }
            // `StringArgumentType.parse`'s three arms, which are three
            // different readers and not three flavours of one.
            ("brigadier:string", Props::Str(StringKind::Word)) => {
                r.read_unquoted_string();
                Ok(())
            }
            ("brigadier:string", Props::Str(StringKind::Phrase)) => {
                r.read_string().map(|_| ()).map_err(ArgError::Syntax)
            }
            ("brigadier:string", Props::Str(StringKind::Greedy)) => {
                r.read_remaining();
                Ok(())
            }
            // `GameModeArgument.parse`: `readUnquotedString`, then
            // `GameType.byName(s, null)` — which is `StringRepresentable`'s
            // enum codec and matches the four `getSerializedName`s exactly.
            // No abbreviations and no ids: `/gamemode 0` is a syntax error in
            // 26.2 and `/gamemode s` is too.
            ("minecraft:gamemode", _) => {
                let name = r.read_unquoted_string();
                if GAME_MODE_NAMES.contains(&name) {
                    Ok(())
                } else {
                    Err(ArgError::Syntax(Syntax::Unmapped))
                }
            }
            // `Identifier.read`: the identifier-character run, then
            // `bySeparator(s, ':')`. The cursor is rewound to the start before
            // raising, which is `read`'s own `setCursor(i)` in its catch block
            // and is observable — a candidate tried after this one must see
            // the whole identifier, not its tail.
            // `BlockStateArgument.parse` — see `crate::blockarg`. Unread
            // only when the state table itself is missing, which is a fact
            // about this process and not about the input.
            ("minecraft:block_state", _) => match crate::blockarg::shared() {
                Some(table) => crate::blockarg::parse(r, table).map(|_| ()),
                None => Err(ArgError::Unsupported),
            },
            ("minecraft:resource_location", _) => {
                let start = r.cursor();
                let run = r.read_identifier_run();
                if identifier_is_valid(run) {
                    Ok(())
                } else {
                    r.set_cursor(start);
                    Err(ArgError::Syntax(Syntax::Unmapped))
                }
            }
            // `ParticleArgument.readParticle` — see [`particle_value`].
            ("minecraft:particle", _) => particle_value(r).map(|_| ()),
            // `TimeArgument.parse`: a float, then a unit, then a rounded
            // multiply, then the declared minimum.
            //
            // The bound is checked against the **product**, not the float, so
            // `/time add -1t` and `/time add -1` differ from `/time add -1d`
            // by 24,000 rather than by a unit. `TimeMin` carries the minimum
            // in full (no sentinel: two nodes in vanilla's tree declare
            // `i32::MIN` explicitly and the serialiser writes it), so the
            // comparison is unconditional.
            ("minecraft:time", Props::TimeMin(min)) => time_ticks(r, *min).map(|_| ()),
            // `BlockPosArgument.parse` and `Vec3Argument.parse` differ in
            // exactly one place: which `WorldCoordinates` factory the
            // non-local branch calls. Both dispatch to `LocalCoordinates` on a
            // leading `^`.
            ("minecraft:block_pos", _) => coordinates(r, Coord::Int),
            ("minecraft:vec3", _) => coordinates(r, Coord::Double),
            // `Vec2Argument.parse`, which is NOT `coordinates` with the loop
            // bound changed — see [`vec2`] for the two places it differs.
            ("minecraft:vec2", _) => vec2(r),
            // `ComponentArgument.textComponent` — the subset in
            // `crate::componentarg`; anything outside it is unread.
            ("minecraft:component", _) => crate::componentarg::parse(r).map(|_| ()),
            // `RangeArgument.intRange()` — see [`int_range_value`].
            ("minecraft:int_range", _) => int_range_value(r).map(|_| ()),
            // `RotationArgument.parse` — see [`rotation_value`].
            ("minecraft:rotation", _) => rotation_value(r).map(|_| ()),
            // `ObjectiveArgument.parse` is **two instructions**:
            // `readUnquotedString(); areturn`. No length check, no lookup, no
            // rewind — so it accepts the empty string, and every constraint on
            // an objective name lives in the executor rather than the parser.
            ("minecraft:objective", _) => {
                r.read_unquoted_string();
                Ok(())
            }
            ("minecraft:objective_criteria", _) => objective_criteria(r).map(|_| ()),
            ("minecraft:scoreboard_slot", _) => scoreboard_slot(r).map(|_| ()),
            // `EntityArgument.parse(reader, true)`: the selector parse, then
            // the two shape checks the flag bits stand for.
            (
                "minecraft:entity",
                Props::Entity {
                    single,
                    players_only,
                },
            ) => entity_selector(r, *single, *players_only).map(|_| ()),
            // `MessageArgument.parse` -> `Message.parseText(reader, true)`.
            ("minecraft:message", _) => message(r).map(|_| ()),
            _ => Err(ArgError::Unsupported),
        }
    }
}

/// What one entity selector turned out to be, for the caller that has to
/// decide whether it can act on it.
///
/// The three fields are `EntitySelector`'s own and are exactly the three
/// `EntityArgument.parse` reads back: how many results the selector may
/// produce, whether it can name a non-player, and whether it is `@s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selector {
    /// `EntitySelector.getMaxResults`.
    pub max_results: i32,
    /// `EntitySelector.includesEntities`.
    pub includes_entities: bool,
    /// `EntitySelector.isSelfSelector` — set only by `@s`.
    pub self_selector: bool,
    /// Whether the source text began with `@`. A plain name or UUID is not a
    /// selector, and the distinction is what an executor needs: a name can be
    /// looked up in the player list today, and `@a[...]` cannot.
    pub used_selector: bool,
}

/// The six selector type characters, from `parseSelector`'s own `tableswitch`
/// (97..115, six live arms).
///
/// **`n` is one of them and it is the one a port misses**: `@n` is
/// `SELECTOR_NEAREST_ENTITY`, added beside `@p`/`@a`/`@r`/`@s`/`@e`, and a
/// five-arm transcription refuses a selector vanilla accepts.
const SELECTOR_TYPES: [char; 6] = ['p', 'a', 'r', 's', 'e', 'n'];

/// `EntitySelectorParser.parse` followed by `EntityArgument.parse`'s two
/// checks.
///
/// # What is transcribed and what is refused
///
/// The selector *head* — `@` plus a type character — and the name/UUID form
/// are transcribed. `parseOptions`, the `[...]` block, is **not**: it is
/// `EntitySelectorOptions`' thirty-odd options, each with its own value
/// grammar, and inventing any of them would put an accept/reject verdict on
/// input this server cannot actually read. So a `[` after the type character
/// is [`ArgError::Unsupported`], which is a statement about this server and
/// keeps `ci/command-parse-golden.sh`'s unread ratchet honest about it.
///
/// # Errors
/// [`ArgError::Syntax`] where vanilla raises, [`ArgError::Unsupported`] for an
/// options block.
pub fn entity_selector(
    r: &mut Reader<'_>,
    single: bool,
    players_only: bool,
) -> Result<Selector, ArgError> {
    let sel = if r.peek() == Some('@') {
        r.skip();
        parse_selector_head(r)?
    } else {
        parse_name_or_uuid(r)?
    };
    // `EntityArgument.parse`'s post-checks. **`setCursor(0)`, literally zero**
    // — not the start of the argument — is what vanilla writes at offsets 44
    // and 93. It is unobservable through `Tree::resolve`, which discards a
    // failing candidate's reader, and it is transcribed rather than tidied
    // because the day something does read that cursor, the tidy version would
    // be the surprise.
    if sel.max_results > 1 && single {
        r.set_cursor(0);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    if sel.includes_entities && players_only && !sel.self_selector {
        r.set_cursor(0);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    Ok(sel)
}

/// `EntitySelectorParser.parseSelector`, after the `@` has been skipped.
fn parse_selector_head(r: &mut Reader<'_>) -> Result<Selector, ArgError> {
    if !r.can_read() {
        // ERROR_MISSING_SELECTOR_TYPE — the one `MessageArgument` forgives.
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let at = r.cursor();
    let Some(kind) = r.read() else {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    };
    if !SELECTOR_TYPES.contains(&kind) {
        // ERROR_UNKNOWN_SELECTOR_TYPE, with `parseSelector`'s own rewind to
        // the type character — the other one `MessageArgument` forgives.
        r.set_cursor(at);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let sel = Selector {
        // The four fields as the six arms set them. `@a` and `@e` are the two
        // unbounded ones; `@s` is the only self selector; `@p`, `@a` and `@r`
        // are the three that `limitToType(PLAYER)` and so leave
        // `includesEntities` false.
        max_results: if kind == 'a' || kind == 'e' {
            i32::MAX
        } else {
            1
        },
        includes_entities: kind == 's' || kind == 'e' || kind == 'n',
        self_selector: kind == 's',
        used_selector: true,
    };
    if r.peek() == Some('[') {
        r.skip();
        return Err(ArgError::Unsupported);
    }
    // `finalizePredicates` and `getSelector` read nothing further, so the
    // parse ends here.
    Ok(sel)
}

/// `EntitySelectorParser.parseNameOrUUID`.
///
/// **`UUID.fromString` is tried FIRST** and the player-name branch is the
/// `IllegalArgumentException` handler, which is why the UUID grammar is part
/// of this parser's accept set rather than a detail of resolution. A name is
/// then refused only for being empty or longer than 16, and the cursor is put
/// back to the start of the string before raising.
fn parse_name_or_uuid(r: &mut Reader<'_>) -> Result<Selector, ArgError> {
    let start = r.cursor();
    let s = r.read_string().map_err(ArgError::Syntax)?;
    if java_uuid_is_valid(&s) {
        return Ok(Selector {
            max_results: 1,
            includes_entities: true,
            self_selector: false,
            used_selector: false,
        });
    }
    if s.is_empty() || s.chars().count() > 16 {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    Ok(Selector {
        max_results: 1,
        includes_entities: false,
        self_selector: false,
        used_selector: false,
    })
}

/// `java.util.UUID.fromString`, as a predicate.
///
/// The modern JDK implementation, not the `split("-")` one it replaced: at
/// most 36 characters, **exactly four** dashes with a fifth forbidden, and
/// each of the five components parsed as unsigned hex — which is lenient
/// about component WIDTH. `a-b-c-d-e` is a valid UUID and `1-2-3-4-5-6` is
/// not, and both are in `ci/command-parse-golden.sh`'s corpus so that this is
/// gated rather than reasoned about.
fn java_uuid_is_valid(s: &str) -> bool {
    if s.len() > 36 {
        return false;
    }
    let Some(d1) = s.find('-') else { return false };
    let Some(d2) = s[d1 + 1..].find('-').map(|i| i + d1 + 1) else {
        return false;
    };
    let Some(d3) = s[d2 + 1..].find('-').map(|i| i + d2 + 1) else {
        return false;
    };
    let Some(d4) = s[d3 + 1..].find('-').map(|i| i + d3 + 1) else {
        return false;
    };
    if s[d4 + 1..].contains('-') {
        return false;
    }
    // `Long.parseLong(name, from, to, 16)` on each component: non-empty, hex,
    // and inside a signed long once the sign-free ranges are applied. The
    // widths vanilla's own parse uses are 8/4/4/4/12 but the parser does not
    // enforce them — it only requires each range to be parseable.
    for (from, to) in [
        (0, d1),
        (d1 + 1, d2),
        (d2 + 1, d3),
        (d3 + 1, d4),
        (d4 + 1, s.len()),
    ] {
        let part = &s[from..to];
        if part.is_empty() || i64::from_str_radix(part, 16).is_err() {
            return false;
        }
    }
    true
}

/// `MessageArgument$Message.parseText(reader, true)`.
///
/// Greedy, capped, and it walks the text looking for selectors as it goes.
/// The cap is on `getRemainingLength()` **before** anything is read, so a
/// 257-character message fails without the cursor moving.
///
/// # The two forgiven errors are the whole shape of it
///
/// A `@` is only a selector if the selector parse gets past its type
/// character. `ERROR_MISSING_SELECTOR_TYPE` (a trailing `@`) and
/// `ERROR_UNKNOWN_SELECTOR_TYPE` (`@q`) are both caught, the cursor set to
/// `start + 1`, and the scan continues — so `hello@world` and `mail me @ 6`
/// are plain text. Every other exception propagates and fails the whole
/// message. A port that treated any `@` as a selector would refuse a mail
/// address; one that treated none as a selector would accept `@a[bad` .
///
/// Returns how many selector parts the message holds, because that is what an
/// executor needs to know: this server can send a message with zero parts and
/// cannot resolve one with more.
///
/// # Errors
/// [`ArgError::Syntax`] on the length cap or an unforgiven selector error,
/// [`ArgError::Unsupported`] if a selector carries an options block.
pub fn message(r: &mut Reader<'_>) -> Result<usize, ArgError> {
    if r.remaining_len() > MESSAGE_MAX_LENGTH {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let mut parts = 0usize;
    while r.can_read() {
        if r.peek() != Some('@') {
            r.skip();
            continue;
        }
        let at = r.cursor();
        r.skip();
        match parse_selector_head(r) {
            Ok(_) => parts += 1,
            // The two forgiven arms — `ERROR_MISSING_SELECTOR_TYPE` and
            // `ERROR_UNKNOWN_SELECTOR_TYPE` — restored to `start + 1`, one
            // past the `@`, so the scan cannot loop on it. They are the ONLY
            // two syntax errors `parse_selector_head` raises, which is why one
            // arm covers both; anything else it returns is `Unsupported` and
            // falls through to the `Err(e)` below.
            Err(ArgError::Syntax(Syntax::Unmapped)) => {
                r.set_cursor(at + 1);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(parts)
}

/// `MessageArgument.TOO_LONG`'s bound: `getRemainingLength() > 256`.
///
/// The same 256 as `crate::chat`'s `MAX_MESSAGE_CHARS` and a different fact —
/// that one is `ServerboundChatPacket`'s read limit. They are equal in 26.2
/// and there is no reason they must stay so, which is why this is its own
/// constant rather than a reference to that one.
pub const MESSAGE_MAX_LENGTH: usize = 256;

/// Which of `WorldCoordinates`' two factories a coordinate triple uses.
///
/// The difference is **not** int-versus-double at the reader: a *relative*
/// coordinate reads a `double` under both, so `/setblock ~1.5 ~ ~ stone`
/// parses. It is only the absolute branch that differs, which is why this is
/// one function and a flag rather than two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coord {
    /// `WorldCoordinates.parseInt`.
    Int,
    /// `WorldCoordinates.parseDouble`. The `centerCorrect` flag it also
    /// carries changes the VALUE (`+ 0.5` on a whole absolute number) and
    /// never the cursor, so it has no place in a parse.
    Double,
}

/// `WorldCoordinates.parseInt` / `parseDouble`, or `LocalCoordinates.parse`.
///
/// # The separator rule is stricter than the dispatcher's, and it is here
///
/// After each of the first two components the reader must have **more input
/// AND** be looking at a space — `canRead()` alone is not enough, and reading
/// it as "exhausted is fine, the next component will complain" is wrong by one
/// error and by the whole cursor. Vanilla's own offsets say so:
/// `10: canRead / 14: ifeq 26 / 17: peek / 23: if_icmpeq 39 / 26: setCursor(start); throw`,
/// so `!canRead` jumps to the SAME error branch that a non-space does.
///
/// On failure the cursor goes back to the start of the *whole triple* rather
/// than to the start of the component, and that rewind is observable: it is
/// what lets a later candidate node in `parse_nodes` see `1 2` as its own
/// argument rather than as ` 2`. Measured: `minecraft:block_pos` on `"1 2"`
/// leaves vanilla's cursor at 0, and a port that let the exhausted reader
/// through leaves it at 3.
fn coordinates(r: &mut Reader<'_>, kind: Coord) -> Result<(), ArgError> {
    let start = r.cursor();
    if r.peek() == Some('^') {
        return local_coordinates(r, start);
    }
    for i in 0..3 {
        world_coordinate(r, kind)?;
        if i == 2 {
            break;
        }
        if !r.can_read() || r.peek() != Some(' ') {
            r.set_cursor(start);
            return Err(ArgError::Syntax(Syntax::Unmapped));
        }
        r.skip();
    }
    Ok(())
}

/// The criteria `ObjectiveCriteria.CRITERIA_CACHE` holds with no registry in
/// hand: eleven custom ones and two per `TeamColor`.
///
/// `registerCustom` is called eleven times in `<clinit>` (offsets 33..139) and
/// `registerForEveryTeamColor` twice, with the format strings
/// `killedByTeam.\u0001` and `teamkill.\u0001` — recovered from the
/// `BootstrapMethods` table with `javap -v`, because `makeConcatWithConstants`
/// hides them from `javap -c`. Sixteen colours each, so **43 names**.
const CUSTOM_CRITERIA: [&str; 11] = [
    "dummy",
    "trigger",
    "deathCount",
    "playerKillCount",
    "totalKillCount",
    "health",
    "food",
    "air",
    "armor",
    "xp",
    "level",
];

/// `ObjectiveCriteriaArgument.parse`.
///
/// # The registry decides part of the accept set, and only part
///
/// `ObjectiveCriteria.byName` has three arms and they are three different
/// answers, which is what made this parser transcribable before
/// `minecraft:block_state` was (`crate::blockarg`, 2026-10-01):
///
/// 1. a hit in `CRITERIA_CACHE` — the 43 names [`CUSTOM_CRITERIA`] and the two
///    per-colour families describe — accepts, and needs no registry;
/// 2. **no `:` in the name and no cache hit** returns `Optional.empty()`
///    immediately (offsets 22..36), so it is a definite REJECT that also needs
///    no registry;
/// 3. a `:` sends it to `BuiltInRegistries.STAT_TYPE` and then to that stat
///    type's own registry, which is thousands of names this server does not
///    hold. That arm alone is [`ArgError::Unsupported`].
///
/// So `dummy` accepts, `notacriterion` rejects, and
/// `minecraft.mined:minecraft.stone` is honestly unread — rather than the
/// whole parser being unread, which is what refusing it wholesale would have
/// cost.
///
/// The scan is `while canRead && peek != ' ' { skip }`, so it consumes to the
/// next space and **not** through `readUnquotedString` — a quoted or
/// punctuated criterion is taken whole rather than stopping at the first
/// character `isAllowedInUnquotedString` refuses. On failure the cursor goes
/// back to the start of the name (`lambda$parse$0`'s `setCursor`), which the
/// neighbouring [`scoreboard_slot`] does **not** do.
///
/// # Errors
/// [`ArgError::Syntax`] for arm 2, [`ArgError::Unsupported`] for arm 3.
fn objective_criteria<'a>(r: &mut Reader<'a>) -> Result<&'a str, ArgError> {
    let start = r.cursor();
    while r.can_read() && r.peek() != Some(' ') {
        r.skip();
    }
    let name = r.slice(start, r.cursor());
    if CUSTOM_CRITERIA.contains(&name) {
        return Ok(name);
    }
    let team_family = crate::scoreboard::TEAM_COLORS.iter().any(|(colour, _)| {
        name == format!("killedByTeam.{colour}") || name == format!("teamkill.{colour}")
    });
    if team_family {
        return Ok(name);
    }
    if name.contains(':') {
        r.set_cursor(start);
        return Err(ArgError::Unsupported);
    }
    r.set_cursor(start);
    Err(ArgError::Syntax(Syntax::Unmapped))
}

/// `ScoreboardSlotArgument.parse` — `readUnquotedString()` then
/// `DisplaySlot.CODEC.byName`.
///
/// **It does not rewind on failure.** There is no `setCursor` anywhere in the
/// method (offsets 0..30): the string is read, the lookup fails, and the
/// exception is raised with the cursor already past the name. Its neighbour
/// [`objective_criteria`] does rewind, and the two sit in the same command —
/// so a port that gave both the same treatment is wrong about one of them, and
/// `ci/command-parse-golden.sh` records the cursor on the rejecting rows
/// precisely so that is visible.
///
/// The nineteen names are [`crate::scoreboard::DISPLAY_SLOTS`], which is
/// `DisplaySlot`'s own `StringRepresentable` table and is already gated by
/// `crates/rustmc-server/golden/scoreboard.json`.
///
/// # Errors
/// [`ArgError::Syntax`] when the name is not a display slot.
fn scoreboard_slot<'a>(r: &mut Reader<'a>) -> Result<&'a str, ArgError> {
    let name = r.read_unquoted_string();
    if crate::scoreboard::DISPLAY_SLOTS
        .iter()
        .any(|(slot, _)| *slot == name)
    {
        Ok(name)
    } else {
        Err(ArgError::Syntax(Syntax::Unmapped))
    }
}

/// `TimeArgument.parse`, returning the tick count rather than discarding it.
///
/// The value is what an executor needs — `/worldborder set 100 5s` is 100
/// ticks — and it used to be computed here and thrown away, which is the
/// dead-field shape this project keeps paying for. Now the parse arm calls
/// this and drops the value, and an executor calls it and keeps it, so there
/// is exactly one transcription of the arithmetic.
///
/// `Math.round(float)` is `(int) Math.floor(a + 0.5f)` **in float**, with
/// Java's saturating narrowing. `time_unit` has FOUR entries and not three:
/// the empty suffix is in the map mapped to 1, and `getOrDefault(s, 0)`
/// returning 0 is the only route to `argument.time.invalid_unit` — a map with
/// a default of 1 accepts `/time set 5x` as five ticks.
///
/// # Errors
/// [`ArgError::Syntax`] on a malformed float, an unknown unit, or a result
/// below the node's declared minimum.
pub fn time_ticks(r: &mut Reader<'_>, min: i32) -> Result<i32, ArgError> {
    let f = r.read_float().map_err(ArgError::Syntax)?;
    let unit = r.read_unquoted_string();
    let Some(mul) = time_unit(unit) else {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    };
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        reason = "`Math.round(float)` is `(int) Math.floor(a + 0.5f)`, \
                  in float, with Java's saturating narrowing — which is \
                  exactly what Rust's `as` does on a float"
    )]
    let ticks = (f * mul as f32 + 0.5f32).floor() as i32;
    if ticks < min {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    Ok(ticks)
}

/// `Vec2Argument.parse` — two components, and **no local-coordinate branch**.
///
/// It differs from [`coordinates`] in exactly two places, and both are places a
/// port that reuses the triple with the loop bound changed gets wrong:
///
/// * **there is no `^` dispatch.** `Vec3Argument.parse` opens
///   `canRead && peek == '^' -> LocalCoordinates.parse`; `Vec2Argument.parse`
///   has no such test at all (its offsets 0..28 go straight from the cursor
///   snapshot to `WorldCoordinate.parseDouble`), so a leading `^` falls into
///   the component parser and raises `ERROR_MIXED_TYPE`. `/worldborder center
///   ^1 ^1` is a syntax error on a real server and `/tp ^1 ^1 ^1` is not.
/// * **there is an explicit empty-input check first.** Offsets 5..19 raise
///   `Vec2Argument.ERROR_NOT_COMPLETE` on `!canRead()` before any component is
///   read, where the triple lets `WorldCoordinate` raise `ERROR_EXPECTED_INT`.
///   Only the exception type differs — both leave the cursor at the start —
///   which is why it is transcribed rather than relied on: the day
///   `ArgError::Syntax` carries a mapped type, these two are not the same one.
///
/// The separator rule between the two components is the triple's, including
/// the rewind to the start of the pair rather than of the component
/// (offsets 29..57).
///
/// The `y` this returns in vanilla is a synthesised `new WorldCoordinate(true,
/// 0.0)` wedged between the two read ones, which is why `getVec2` reads `.x`
/// and `.z` of a `Vec3`. Nothing about that is observable through a parse.
fn vec2(r: &mut Reader<'_>) -> Result<(), ArgError> {
    vec2_value(r, 0.0, 0.0).map(|_| ())
}

/// [`vec2`] keeping the pair it read, resolved against the source position the
/// way `WorldCoordinates.getPosition(source)` resolves one.
///
/// # `centerCorrect` adds half a block and nothing in the name says so
///
/// `Vec2Argument.vec2()` is `new Vec2Argument(true)`, and that `true` reaches
/// `WorldCoordinate.parseDouble(reader, centerCorrect)`, whose offsets 111..137
/// add **0.5** when the text held no `.`, the coordinate was not relative, and
/// the flag is set. So `/worldborder center 10 20` centres the border at
/// `(10.5, 20.5)`, `/worldborder center 10.0 20.0` at `(10.0, 20.0)`, and the
/// two commands are not the same command. A port that read the number and
/// stopped is wrong by half a block on every integer argument a player types.
///
/// A **relative** coordinate with no digits after the `~` is
/// `new WorldCoordinate(true, 0.0)` returned early (offsets 89..110), which is
/// why `~ ~` resolves to the source position exactly and never picks up the
/// half-block.
///
/// The result is narrowed to `f32` because `Vec2Argument.getVec2` builds a
/// `Vec2` out of two `d2f` narrowings, and the executor then widens them back —
/// so a centre set from a command carries float precision even though the
/// border stores doubles.
///
/// # Errors
/// As [`vec2`].
pub fn vec2_value(
    r: &mut Reader<'_>,
    source_x: f64,
    source_z: f64,
) -> Result<(f32, f32), ArgError> {
    let start = r.cursor();
    if !r.can_read() {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let x = world_coordinate_value(r, true)?;
    if !r.can_read() || r.peek() != Some(' ') {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    r.skip();
    let z = world_coordinate_value(r, true)?;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`d2f` is exactly this narrowing, and `getVec2` does it twice"
    )]
    Ok((x.get(source_x) as f32, z.get(source_z) as f32))
}

/// `MinMaxBounds.Ints.fromReader` — the `minecraft:int_range` argument, as
/// `(min, max)`.
///
/// ```text
///   Bounds.fromReader
///     !canRead                       -> argument.range.empty
///     min = readNumber               [0-9-]* then Integer.parseInt
///     canRead(2) && ".."             -> skip both, max = readNumber
///     otherwise                      -> max = min
///     both empty                     -> argument.range.empty
///   Ints.fromReader
///     min > max                      -> setCursor(start); argument.range.swapped
/// ```
///
/// `readNumber`'s run is digits, `-`, and any `.` not followed by another —
/// and a run `parseInt` rejects (`1.5`, `--1`, out of range) is
/// `parsing.int.invalid` naming the run. Every refusal leaves the reader at
/// the argument's start.
///
/// # Errors
/// [`ArgError::Syntax`] where vanilla raises.
pub fn int_range_value(r: &mut Reader<'_>) -> Result<(Option<i32>, Option<i32>), ArgError> {
    // `RangeArgument.Ints.parse` puts the reader back at the argument's start
    // on ANY exception — measured against vanilla's own rows: `1.5`, `--1`,
    // `..`, `5..1` all leave the cursor at 0.
    let start = r.cursor();
    let out = int_range_at(r);
    if out.is_err() {
        r.set_cursor(start);
    }
    out
}

fn int_range_at(r: &mut Reader<'_>) -> Result<(Option<i32>, Option<i32>), ArgError> {
    /// `Bounds.isAllowedInputChar`: a digit, `-`, or a `.` that does NOT
    /// start a `..` — so `1.5` is one run, which `parseInt` then rejects.
    fn allowed(r: &Reader<'_>) -> bool {
        match r.peek() {
            Some(c) if c.is_ascii_digit() || c == '-' => true,
            Some('.') => !r.string()[r.cursor() + 1..].starts_with('.'),
            _ => false,
        }
    }
    fn read_number(r: &mut Reader<'_>) -> Result<Option<i32>, ArgError> {
        let start = r.cursor();
        while allowed(r) {
            r.skip();
        }
        let text = r.slice(start, r.cursor());
        if text.is_empty() {
            return Ok(None);
        }
        // `Integer.parseInt`: an optional leading sign then digits.
        let digits = text.strip_prefix('-').unwrap_or(text);
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return Err(ArgError::Syntax(Syntax::span(
                "parsing.int.invalid",
                start,
                r.cursor(),
            )));
        }
        text.parse::<i32>()
            .map(Some)
            .map_err(|_| ArgError::Syntax(Syntax::span("parsing.int.invalid", start, r.cursor())))
    }
    if !r.can_read() {
        return Err(ArgError::Syntax(Syntax::simple("argument.range.empty")));
    }
    let start = r.cursor();
    let min = read_number(r)?;
    let max = if r.can_read_len(2)
        && r.peek() == Some('.')
        && r.string()[r.cursor() + 1..].starts_with('.')
    {
        r.skip();
        r.skip();
        read_number(r)?
    } else {
        min
    };
    if min.is_none() && max.is_none() {
        return Err(ArgError::Syntax(Syntax::simple("argument.range.empty")));
    }
    if let (Some(lo), Some(hi)) = (min, max) {
        if lo > hi {
            r.set_cursor(start);
            return Err(ArgError::Syntax(Syntax::simple("argument.range.swapped")));
        }
    }
    Ok((min, max))
}

/// `RotationArgument.parse`, keeping its two coordinates as `[yaw, pitch]` —
/// the order they are typed in.
///
/// ```text
///    0-19  i = cursor; !canRead -> ERROR_NOT_COMPLETE
///   21-22  y = WorldCoordinate.parseDouble(reader, false)
///   27-54  !(canRead && peek ' ') -> setCursor(i); ERROR_NOT_COMPLETE
///   56-61  skip; x = WorldCoordinate.parseDouble(reader, false)
///   66-82  new WorldCoordinates(x, y, new WorldCoordinate(true, 0.0))
/// ```
///
/// No centre correction on either — a rotation is not a block. The vanilla
/// value puts the SECOND typed coordinate in `x` (pitch) and the first in `y`
/// (yaw); `getRotation(source)` resolves each relative one against the
/// SOURCE's rotation, which is not the target's.
///
/// # Errors
/// [`ArgError::Syntax`] (unmapped) where vanilla raises.
pub fn rotation_value(r: &mut Reader<'_>) -> Result<[WorldCoordinate; 2], ArgError> {
    let start = r.cursor();
    if !r.can_read() {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let yaw = world_coordinate_value(r, false)?;
    if !r.can_read() || r.peek() != Some(' ') {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    r.skip();
    let pitch = world_coordinate_value(r, false)?;
    Ok([yaw, pitch])
}

/// `WorldCoordinates.parseDouble(reader, centerCorrect)`, keeping the three
/// coordinates — `Vec3Argument`'s value for the absolute/relative form.
///
/// The centre correction applies to x and z only: offsets 45 and 85 pass the
/// caller's flag, offset 45's middle call passes a literal `iconst_0` for y.
/// `Vec3Argument.vec3()` is `new Vec3Argument(true)`.
///
/// `Ok(None)` for a local (`^`) triple, which `LocalCoordinates` resolves
/// against the source's facing and which no executor here reads yet; the
/// caller decides what an unread form means for its command.
///
/// # Errors
/// As the accept/reject half, [`Parser::parse`] on `minecraft:vec3`.
pub fn vec3_world(
    r: &mut Reader<'_>,
    center_correct: bool,
) -> Result<Option<[WorldCoordinate; 3]>, ArgError> {
    if r.peek() == Some('^') {
        return Ok(None);
    }
    let start = r.cursor();
    let x = world_coordinate_value(r, center_correct)?;
    if !r.can_read() || r.peek() != Some(' ') {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    r.skip();
    let y = world_coordinate_value(r, false)?;
    if !r.can_read() || r.peek() != Some(' ') {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    r.skip();
    let z = world_coordinate_value(r, center_correct)?;
    Ok(Some([x, y, z]))
}

/// `WorldCoordinate` — `relative` and the value, with `get(double)` on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldCoordinate {
    /// Whether the text began with `~`.
    pub relative: bool,
    /// The parsed number, already `centerCorrect`ed if that applied.
    pub value: f64,
}

impl WorldCoordinate {
    /// `WorldCoordinate.get(double)` — `relative ? value + d : value`.
    #[must_use]
    pub fn get(self, source: f64) -> f64 {
        if self.relative {
            self.value + source
        } else {
            self.value
        }
    }
}

/// `ParticleArgument.readParticle(reader, registries)`:
///
/// ```text
///   readParticleType:  id = Identifier.read(reader)
///                      registry.get(id).orElseThrow(
///                          ERROR_UNKNOWN_PARTICLE.createWithContext(reader, id))
///   readParticle:      tag = canRead() && peek() == '{' ? TagParser.parseAsArgument
///                                                       : emptyMap
///                      type.codec().codec().parse(ops, tag)
///                          .getOrThrow(ERROR_INVALID_OPTIONS)
/// ```
///
/// The unknown-type refusal leaves the cursor AFTER the identifier —
/// `createWithContext` is called with the reader where `Identifier.read`
/// left it — and is [`Syntax::Unmapped`]: its argument is the identifier's
/// `toString()`, namespaced, which is not a span of the input.
///
/// Read for a `SimpleParticleType` with no `{`, which is the whole of it: its
/// codec is a unit and the empty map decodes. A `{` (the SNBT grammar is not
/// transcribed) and a type with options (whose codec decides whether an
/// empty map is enough) are [`ArgError::Unsupported`].
///
/// # Errors
/// As above; `Unsupported` also when the particle table is absent.
pub fn particle_value(r: &mut Reader<'_>) -> Result<crate::particles::ParticleType, ArgError> {
    let Some(table) = crate::particles::shared() else {
        return Err(ArgError::Unsupported);
    };
    let start = r.cursor();
    let run = r.read_identifier_run();
    if !identifier_is_valid(run) {
        r.set_cursor(start);
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let Some(ty) = table.get(run) else {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    };
    if r.peek() == Some('{') || !ty.simple {
        return Err(ArgError::Unsupported);
    }
    Ok(ty)
}

/// `WorldCoordinate.parseDouble(reader, centerCorrect)`, keeping its result.
///
/// [`world_coordinate`] is the accept/reject half of this and stays separate
/// because `parseInt` shares that half and not this one.
fn world_coordinate_value(
    r: &mut Reader<'_>,
    center_correct: bool,
) -> Result<WorldCoordinate, ArgError> {
    if r.peek() == Some('^') {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    if !r.can_read() {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let relative = r.peek() == Some('~');
    if relative {
        r.skip();
    }
    let text_start = r.cursor();
    let value = if r.can_read() && r.peek() != Some(' ') {
        r.read_double().map_err(ArgError::Syntax)?
    } else {
        0.0
    };
    let text = r.slice(text_start, r.cursor());
    // `isRelative && text.isEmpty()` returns a bare relative zero BEFORE the
    // centre correction is considered — offsets 89..110.
    if relative && text.is_empty() {
        return Ok(WorldCoordinate {
            relative: true,
            value: 0.0,
        });
    }
    let corrected = if !text.contains('.') && !relative && center_correct {
        value + 0.5
    } else {
        value
    };
    Ok(WorldCoordinate {
        relative,
        value: corrected,
    })
}

/// `WorldCoordinate.parseInt` / `parseDouble`, minus the value arithmetic.
///
/// Three things a port gets wrong by reading the name instead of the code:
///
/// * a `^` here is `ERROR_MIXED_TYPE` and **not** a local coordinate — mixing
///   the two forms in one triple is refused rather than parsed;
/// * an exhausted reader is `ERROR_EXPECTED_INT`/`_DOUBLE`, so `/setblock` with
///   nothing after it fails inside the coordinate rather than at the tree;
/// * a bare `~` is legal and consumes nothing further. `isRelative` skips the
///   tilde, and the number is then read only if there is one before the next
///   space — so `~ ~ ~` is three complete coordinates and not three empty ones.
fn world_coordinate(r: &mut Reader<'_>, kind: Coord) -> Result<(), ArgError> {
    if r.peek() == Some('^') {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    if !r.can_read() {
        return Err(ArgError::Syntax(Syntax::Unmapped));
    }
    let relative = r.peek() == Some('~');
    if relative {
        r.skip();
    }
    if r.can_read() && r.peek() != Some(' ') {
        // A RELATIVE COORDINATE READS A DOUBLE EVEN UNDER `parseInt`. That is
        // `parseInt`'s own `iload_1 / ifeq` at offset 60: the `readInt` branch
        // is the absolute one only.
        if relative || kind == Coord::Double {
            r.read_double().map_err(ArgError::Syntax)?;
        } else {
            r.read_int().map_err(ArgError::Syntax)?;
        }
    }
    Ok(())
}

/// `LocalCoordinates.parse` — three `^`-prefixed components.
///
/// Every failure inside rewinds to `start`, which is the triple's start and
/// not the component's; that is what `readDouble(reader, start)`'s second
/// parameter is for and it is the only reason that helper takes one.
fn local_coordinates(r: &mut Reader<'_>, start: usize) -> Result<(), ArgError> {
    for i in 0..3 {
        if !r.can_read() {
            return Err(ArgError::Syntax(Syntax::Unmapped));
        }
        if r.peek() != Some('^') {
            r.set_cursor(start);
            return Err(ArgError::Syntax(Syntax::Unmapped));
        }
        r.skip();
        if r.can_read() && r.peek() != Some(' ') {
            r.read_double().map_err(ArgError::Syntax)?;
        }
        if i == 2 {
            break;
        }
        if !r.can_read() || r.peek() != Some(' ') {
            r.set_cursor(start);
            return Err(ArgError::Syntax(Syntax::Unmapped));
        }
        r.skip();
    }
    Ok(())
}

/// `GameType`'s four `getSerializedName`s, in the enum's `$values` order.
///
/// The order is not load-bearing for the parse — `StringRepresentable`'s enum
/// codec is a name lookup — but it is the enum's ordinal order, which is what
/// `game_event`'s payload and `crate::status`'s `SURVIVAL`/`CREATIVE` use, so
/// keeping it means one table can serve both if a caller ever needs the
/// ordinal.
pub const GAME_MODE_NAMES: [&str; 4] = ["survival", "creative", "adventure", "spectator"];

/// `TimeArgument.UNITS`, which has **four** entries and not three: the empty
/// string is in the map, mapped to 1.
///
/// That is what makes a bare `/time set 1000` legal — `readUnquotedString`
/// returns `""` after the float and the lookup finds it. A port with three
/// entries and a `default 1` reads the same on every input in the corpus and
/// is wrong in one place: `getOrDefault(s, 0)` returning 0 is the ONLY route to
/// `argument.time.invalid_unit`, so a default of 1 would accept `/time set 5x`
/// as five ticks.
fn time_unit(unit: &str) -> Option<i32> {
    match unit {
        "d" => Some(24_000),
        "s" => Some(20),
        "t" | "" => Some(1),
        _ => None,
    }
}

/// `Identifier.validPathChar` — `_`, `-`, `a-z`, `0-9`, `/`, `.`.
const fn valid_path_char(c: char) -> bool {
    c == '_' || c == '-' || c.is_ascii_lowercase() || c.is_ascii_digit() || c == '/' || c == '.'
}

/// `Identifier.validNamespaceChar` — the path set **without** `/`.
const fn valid_namespace_char(c: char) -> bool {
    c == '_' || c == '-' || c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.'
}

/// `Identifier.parse` == `bySeparator(s, ':')`, as a predicate.
///
/// The three arms are vanilla's own, and the one that surprises is the middle:
/// a **leading** colon is not an empty namespace, it is
/// `withDefaultNamespace(path)` — `bySeparator` tests `i != 0` before it takes
/// a namespace at all. So `:stone` is `minecraft:stone` and validates only its
/// path.
///
/// **An empty identifier is valid.** `isValidPath("")` is a loop over no
/// characters, so `Identifier.read` on an exhausted reader succeeds and
/// consumes nothing — which is why `ci/command-parse-golden.sh` needed a
/// malformed identifier in its corpus before this parser could reject anything
/// at all.
pub(crate) fn identifier_is_valid(s: &str) -> bool {
    match s.find(':') {
        None => s.chars().all(valid_path_char),
        Some(0) => s[1..].chars().all(valid_path_char),
        Some(i) => {
            s[..i].chars().all(valid_namespace_char) && s[i + 1..].chars().all(valid_path_char)
        }
    }
}

/// One argument node's parser: which serialiser runs, and what it writes.
#[derive(Debug, Clone, PartialEq)]
pub struct Parser {
    /// The `minecraft:command_argument_type` registry name, e.g.
    /// `"brigadier:integer"`. The **name**, not the id: the id is a property
    /// of the registry and lives in [`PARSER_IDS`], so a node cannot carry a
    /// number nobody can check.
    pub registry: &'static str,
    pub props: Props,
    /// `SuggestionProviders.getName(node.getCustomSuggestions())`, when the
    /// node has one. Sets flag 16 and is written last in the stub.
    pub suggestions: Option<&'static str>,
}

/// The `minecraft:command_argument_type` registry, in id order.
///
/// `ArgumentNodeStub.write` calls `serializeCap`, which writes
/// `BuiltInRegistries.COMMAND_ARGUMENT_TYPE.getId(info)` as a varint before
/// the properties. The id is therefore part of the wire format and is not
/// derivable from anything else — it is the registration order of
/// `ArgumentTypeInfos.bootstrap`, and nothing in a node says what it is.
///
/// It is a constant table rather than a runtime one because it is 57 entries
/// that cannot change within a pinned version, and because a constant a test
/// can compare is cheaper than a file every boot must read.
/// `command_tree_vanilla::the_parser_registry_is_the_one_vanilla_declares`
/// asserts this array against `data/vanilla/command_tree_full.json`'s own
/// `parsers` map **in both directions**, so a drift, a reordering or a missing
/// entry is a red test rather than a wrong packet.
pub const PARSER_IDS: [&str; 57] = [
    "brigadier:bool",
    "brigadier:float",
    "brigadier:double",
    "brigadier:integer",
    "brigadier:long",
    "brigadier:string",
    "minecraft:entity",
    "minecraft:game_profile",
    "minecraft:block_pos",
    "minecraft:column_pos",
    "minecraft:vec3",
    "minecraft:vec2",
    "minecraft:block_state",
    "minecraft:block_predicate",
    "minecraft:item_stack",
    "minecraft:item_predicate",
    "minecraft:team_color",
    "minecraft:hex_color",
    "minecraft:component",
    "minecraft:style",
    "minecraft:message",
    "minecraft:nbt_compound_tag",
    "minecraft:nbt_tag",
    "minecraft:nbt_path",
    "minecraft:objective",
    "minecraft:objective_criteria",
    "minecraft:operation",
    "minecraft:particle",
    "minecraft:angle",
    "minecraft:rotation",
    "minecraft:scoreboard_slot",
    "minecraft:score_holder",
    "minecraft:swizzle",
    "minecraft:team",
    "minecraft:item_slot",
    "minecraft:item_slots",
    "minecraft:resource_location",
    "minecraft:function",
    "minecraft:entity_anchor",
    "minecraft:int_range",
    "minecraft:float_range",
    "minecraft:dimension",
    "minecraft:gamemode",
    "minecraft:time",
    "minecraft:resource_or_tag",
    "minecraft:resource_or_tag_key",
    "minecraft:resource",
    "minecraft:resource_key",
    "minecraft:resource_selector",
    "minecraft:template_mirror",
    "minecraft:template_rotation",
    "minecraft:heightmap",
    "minecraft:loot_table",
    "minecraft:loot_predicate",
    "minecraft:loot_modifier",
    "minecraft:dialog",
    "minecraft:uuid",
];

/// The registry id for a parser name, or `None` for a name not in 26.2's
/// registry.
#[must_use]
pub fn parser_id(registry: &str) -> Option<i32> {
    PARSER_IDS
        .iter()
        .position(|p| *p == registry)
        .and_then(|i| i32::try_from(i).ok())
}

// ===========================================================================
// THE GRAPH — vanilla's actual data structure, and the only encoder
// ===========================================================================
//
// [`Tree`] is a builder: nested [`Node`]s, no back edges, one owner per node.
// It is what `crate::commands` registers against and it cannot express a
// redirect, because a redirect is an edge to a node somebody else owns.
//
// Vanilla's dispatcher is not a tree. `CommandNode.redirect` makes it a graph
// with 112 back edges, and `enumerateNodes` walks the graph rather than the
// tree — it enqueues a node's redirect target as well as its children, which
// is why `/execute`'s subtree is numbered once and pointed at from
// everywhere.
//
// So this is the shape the encoder works in, and **there is exactly one
// encoder.** [`Tree::encode`] lowers to a [`Graph`] and calls
// [`Graph::encode`]; the vanilla-scale gate builds a [`Graph`] straight from
// `data/vanilla/command_tree_full.json` and calls the same method. If the two
// had separate encoders the gate would be measuring a second implementation
// and this server's own packet would stay ungated at scale — the shape
// `docs/AGENT-BRIEF.md` opens by naming.

/// One node in the flattened graph. Index 0 is not special; [`Graph::root`]
/// says which node is the root.
#[derive(Debug, Clone)]
pub struct GraphNode {
    /// The literal, or the argument's name. Empty for the root, which writes
    /// no name at all.
    pub name: &'static str,
    /// `None` for a literal or the root.
    pub parser: Option<Parser>,
    /// Set on exactly one node, and it writes flag byte 0.
    pub is_root: bool,
    pub level: Level,
    pub executable: bool,
    /// Child indices, in registration order — Brigadier's `children` is a
    /// `LinkedHashMap`, so this order is the wire order and is load-bearing.
    pub children: Vec<usize>,
    pub redirect: Option<usize>,
}

/// Vanilla's dispatcher graph.
#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub root: usize,
}

impl GraphNode {
    /// `Commands.COMMAND_NODE_INSPECTOR.isRestricted`, as [`Node::restricted`]
    /// — the requirement tested against a `NO_PERMISSIONS` source, so every
    /// level above [`Level::All`] is restricted and the root never is.
    #[must_use]
    pub fn restricted(&self) -> bool {
        !self.is_root && self.level != Level::All
    }
}

/// A node after `fillUsableCommands` has copied it, with its children and
/// redirect resolved into the copy's own index space.
struct Copied {
    src: usize,
    children: Vec<usize>,
    redirect: Option<usize>,
}

impl Graph {
    /// The node a path of literals from the root reaches, or `None`.
    ///
    /// Literals only: an argument node has no literal to name and a path
    /// through one is not expressible. Every redirect in vanilla's tree that
    /// this server could register targets a literal or the root.
    #[must_use]
    pub fn find(&self, path: &[&str]) -> Option<usize> {
        let mut here = self.root;
        for step in path {
            here = *self.nodes[here]
                .children
                .iter()
                .find(|&&c| self.nodes[c].parser.is_none() && self.nodes[c].name == *step)?;
        }
        Some(here)
    }

    /// The filtered graph in wire order: each node's name and its children as
    /// **wire** indices.
    ///
    /// Exposed because two properties of the walk are observable only here
    /// and both have been silently broken before: that the numbering is
    /// breadth first (`ci/chat-commands-falsify.sh` turned the queue's
    /// `push_back` into a `push_front` and nothing caught it) and that the
    /// filter recurses. Reading them out of the encoded bytes instead would
    /// mean a test that fails for the right reason and says the wrong thing.
    #[must_use]
    pub fn wire_order(&self, level: Level) -> Vec<(&'static str, Vec<usize>)> {
        let copy = self.fill_usable(level);
        let (order, index) = self.enumerate(&copy);
        order
            .iter()
            .map(|&c| {
                (
                    self.nodes[copy[c].src].name,
                    copy[c].children.iter().map(|ch| index[*ch]).collect(),
                )
            })
            .collect()
    }

    /// `Commands.fillUsableCommands`, over the whole graph.
    ///
    /// Three properties of vanilla's version are easy to lose and each is
    /// separately observable in the byte comparison:
    ///
    /// 1. **It recurses only into children that survived.** Offsets 33..39
    ///    gate the entire body on `canUse`, so a usable node under an
    ///    unusable parent is unreachable and is never sent.
    /// 2. **It recurses only if the original has children** (offsets
    ///    101..111), which is an optimisation and not a behaviour, but it is
    ///    also why a childless node's copy has no children rather than an
    ///    empty list that later grows.
    /// 3. **A redirect is remapped through the copy map, and a target not yet
    ///    copied becomes no redirect at all.** `builder.redirect(map.get(t))`
    ///    with a `null` from the map is `forward(null, null, false)`, and a
    ///    node with a null target reports `getRedirect() == null`, so flag 8
    ///    is never set. The map is seeded with the root before the walk
    ///    (`sendCommands` offsets 16..30), so a redirect to the root always
    ///    survives; a forward reference into an unvisited subtree does not.
    fn fill_usable(&self, level: Level) -> Vec<Copied> {
        let mut out = vec![Copied {
            src: self.root,
            children: Vec::new(),
            redirect: None,
        }];
        // `map`: source index -> copy index. Seeded with the root, exactly as
        // `sendCommands` seeds it, which is what makes a redirect to the root
        // resolvable on the first node that carries one.
        let mut map: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::from([(self.root, 0usize)]);
        self.fill_into(self.root, 0, level, &mut out, &mut map);
        out
    }

    fn fill_into(
        &self,
        src: usize,
        dst: usize,
        level: Level,
        out: &mut Vec<Copied>,
        map: &mut std::collections::HashMap<usize, usize>,
    ) {
        for &child in &self.nodes[src].children {
            if !level.allows(self.nodes[child].level) {
                continue;
            }
            // The redirect is resolved BEFORE this node is put in the map, so
            // a node that redirects to itself gets no redirect. That is
            // vanilla's order (offsets 49..94) and not an accident of ours.
            let redirect = self.nodes[child]
                .redirect
                .and_then(|t| map.get(&t).copied());
            let idx = out.len();
            out.push(Copied {
                src: child,
                children: Vec::new(),
                redirect,
            });
            map.insert(child, idx);
            out[dst].children.push(idx);
            if !self.nodes[child].children.is_empty() {
                self.fill_into(child, idx, level, out, map);
            }
        }
    }

    /// Brigadier's structural equality class for one copied node.
    ///
    /// # This is the single hardest thing in the packet and nothing names it
    ///
    /// `enumerateNodes` collects into an **`Object2IntOpenHashMap`**, and that
    /// map hashes by `hashCode`/`equals` — not by identity. Brigadier's
    /// `CommandNode.equals` compares **the children map and the `Command`
    /// object, and nothing else**: not the requirement, not the redirect, not
    /// the suggestion provider (`brigadier-1.3.10.jar`,
    /// `CommandNode.equals` offsets 21..71). `LiteralCommandNode` adds the
    /// literal; `ArgumentCommandNode` adds the name and `type.equals`.
    ///
    /// So `if (map.containsKey(node)) continue;` is not a cycle guard. It
    /// **merges every structurally identical subtree in the tree**, and the
    /// merge is large: at `OWNERS`, vanilla's 2,085-node dispatcher becomes a
    /// **2,017-entry** packet. All 68 of the collapsed nodes are in
    /// `/execute store {result,success} {block,entity,storage} … <path>
    /// {byte,short,int,long,float,double} [scale]`, where the six numeric
    /// literals carry one non-executable `scale` argument each, of the same
    /// `DoubleArgumentType`, so the six `scale` nodes are equal, so the six
    /// literals are equal, and the whole shape is written once and pointed at
    /// from everywhere. An encoder that deduplicated by identity produces
    /// 2,085 entries and a packet 694 bytes too long, which is what this one
    /// did until 2026-09-05.
    ///
    /// Two consequences that are easy to get backwards:
    ///
    /// * **`type.equals` is identity for most parsers.** Of Brigadier's six
    ///   primitive argument types only `integer`, `long`, `float` and
    ///   `double` override `equals`; `bool` and `string` do not, and no
    ///   `net.minecraft.commands.arguments` type does. So an argument node
    ///   merges only when its parser is one of those four *and* the bounds
    ///   match. Everything else is its own class.
    /// * **An executable node never merges.** `command` is a lambda and
    ///   `Object.equals` on it is identity, so two nodes carrying two
    ///   separately-constructed `Command`s are unequal even when everything
    ///   else about them matches. Keyed on the source node here, because two
    ///   copies of one source share one lambda and would be equal.
    fn class_of(
        &self,
        copy: &[Copied],
        node: usize,
        cache: &mut Vec<Option<usize>>,
        intern: &mut std::collections::HashMap<String, usize>,
    ) -> usize {
        if let Some(c) = cache[node] {
            return c;
        }
        // Children first: a parent's class is a function of its children's.
        let kids: Vec<(&'static str, usize)> = copy[node]
            .children
            .iter()
            .map(|&c| {
                let cls = self.class_of(copy, c, cache, intern);
                (self.nodes[copy[c].src].name, cls)
            })
            .collect();
        // `Map.equals` is order-independent, so the key must be too.
        let mut sorted = kids;
        sorted.sort_unstable();
        let src = copy[node].src;
        let n = &self.nodes[src];

        let head = if n.executable {
            // Identity on the source's `Command`.
            format!("X{src}")
        } else if n.is_root {
            "R".to_owned()
        } else {
            match &n.parser {
                None => format!("L{}", n.name),
                Some(p) => match p.props {
                    // The four types whose `equals` is by value.
                    Props::Int(..) | Props::Long(..) | Props::Float(..) | Props::Double(..) => {
                        format!("A{}\u{1}{}\u{1}{:?}", n.name, p.registry, p.props)
                    }
                    // Every other `ArgumentType` inherits `Object.equals`.
                    _ => format!("I{src}"),
                },
            }
        };
        let mut key = head;
        for (name, cls) in sorted {
            key.push('\u{2}');
            key.push_str(name);
            key.push('\u{3}');
            key.push_str(&cls.to_string());
        }
        let next = intern.len();
        let id = *intern.entry(key).or_insert(next);
        cache[node] = Some(id);
        id
    }

    /// `ClientboundCommandsPacket.enumerateNodes` over the filtered copy.
    ///
    /// Breadth first with an `ArrayDeque`, an index assigned as the map's
    /// size at first sight, **children enqueued whole and then the redirect
    /// target**, and a node whose equality class has already been numbered
    /// skipped — see [`Graph::class_of`] for what "equality class" means here
    /// and why it is not identity.
    ///
    /// Returns the copies in wire order and, for every copy, the wire index
    /// its class was numbered at. `createEntry` writes `map.getInt(child)`,
    /// so a merged child is written as the index of the node it merged with.
    fn enumerate(&self, copy: &[Copied]) -> (Vec<usize>, Vec<usize>) {
        let mut cache = vec![None; copy.len()];
        let mut intern = std::collections::HashMap::new();
        let classes: Vec<usize> = (0..copy.len())
            .map(|i| self.class_of(copy, i, &mut cache, &mut intern))
            .collect();

        let mut order: Vec<usize> = Vec::new();
        let mut numbered: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        queue.push_back(0);
        while let Some(n) = queue.pop_front() {
            if numbered.contains_key(&classes[n]) {
                continue;
            }
            numbered.insert(classes[n], order.len());
            order.push(n);
            queue.extend(copy[n].children.iter().copied());
            if let Some(r) = copy[n].redirect {
                queue.push_back(r);
            }
        }
        let index = classes.iter().map(|c| numbered[c]).collect();
        (order, index)
    }

    /// The `ClientboundCommandsPacket` body for a source at `level`.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    ///
    /// # Panics
    /// If an argument node names a parser that is not in [`PARSER_IDS`]. That
    /// is a programming error in a registration, not a runtime condition:
    /// there is no id to write and a silently-omitted node would be a packet
    /// the client cannot validate.
    pub fn encode(&self, level: Level) -> io::Result<Vec<u8>> {
        let copy = self.fill_usable(level);
        let (order, index) = self.enumerate(&copy);

        let mut out = Vec::new();
        write_varint(&mut out, i32::try_from(order.len()).unwrap_or(i32::MAX))?;
        for &c in &order {
            let entry = &copy[c];
            let node = &self.nodes[entry.src];

            let mut flags = if node.is_root {
                TYPE_ROOT
            } else if node.parser.is_some() {
                TYPE_ARGUMENT
            } else {
                TYPE_LITERAL
            };
            if entry.redirect.is_some() {
                flags |= FLAG_REDIRECT;
            }
            if node.executable {
                flags |= FLAG_EXECUTABLE;
            }
            if node.restricted() {
                flags |= FLAG_RESTRICTED;
            }
            if node
                .parser
                .as_ref()
                .is_some_and(|p| p.suggestions.is_some())
            {
                flags |= FLAG_CUSTOM_SUGGESTIONS;
            }
            wire::write_u8(&mut out, flags)?;

            // `writeVarIntArray`: the count, then the values.
            write_varint(
                &mut out,
                i32::try_from(entry.children.len()).unwrap_or(i32::MAX),
            )?;
            for ch in &entry.children {
                write_varint(&mut out, i32::try_from(index[*ch]).unwrap_or(i32::MAX))?;
            }
            if let Some(r) = entry.redirect {
                write_varint(&mut out, i32::try_from(index[r]).unwrap_or(i32::MAX))?;
            }

            // The stub. The root has none; a literal writes its name; an
            // argument writes its name, its parser's registry id, the
            // parser's own properties and then its suggestion id.
            if node.is_root {
                continue;
            }
            wire::write_string(&mut out, node.name)?;
            if let Some(p) = &node.parser {
                let id = parser_id(p.registry).unwrap_or_else(|| {
                    panic!(
                        "argument node {:?} names parser {:?}, which is not in \
                         26.2's command_argument_type registry",
                        node.name, p.registry
                    )
                });
                write_varint(&mut out, id)?;
                p.props.write(&mut out)?;
                if let Some(s) = p.suggestions {
                    wire::write_string(&mut out, s)?;
                }
            }
        }
        // `rootIndex` — `map.getInt(root)`. The root is polled first into an
        // empty map so it is always 0, but it is read off the walk rather
        // than written as a literal so the claim is the code's.
        write_varint(&mut out, i32::try_from(index[0]).unwrap_or_default())?;
        Ok(out)
    }
}

/// The whole registry: the root's children, in registration order.
#[derive(Debug, Default, Clone)]
pub struct Tree {
    pub roots: Vec<Node>,
}

impl Tree {
    /// Flatten this builder tree into the [`Graph`] the encoder works in.
    ///
    /// The root is node 0 and every other node is appended in **depth-first
    /// registration order**. That order is not the wire order and is not
    /// meant to be: [`Graph::enumerate`] renumbers breadth-first. It is only
    /// an arena.
    ///
    /// # Panics
    /// On a [`Node::redirect`] path that names no node in this tree.
    #[must_use]
    pub fn lower(&self) -> Graph {
        let mut g = Graph {
            nodes: vec![GraphNode {
                name: "",
                parser: None,
                is_root: true,
                level: Level::All,
                executable: false,
                children: Vec::new(),
                redirect: None,
            }],
            root: 0,
        };
        // Resolved in a second pass: a redirect may name a node that has not
        // been built yet, and registration order is not a topological one.
        let mut pending: Vec<(usize, &'static [&'static str])> = Vec::new();
        Self::lower_into(&self.roots, 0, &mut g, &mut pending);
        for (idx, path) in pending {
            let target = g.find(path).unwrap_or_else(|| {
                panic!("redirect path {path:?} names no node in this command tree")
            });
            g.nodes[idx].redirect = Some(target);
        }
        g
    }

    fn lower_into(
        nodes: &[Node],
        parent: usize,
        g: &mut Graph,
        pending: &mut Vec<(usize, &'static [&'static str])>,
    ) {
        for n in nodes {
            let idx = g.nodes.len();
            g.nodes.push(GraphNode {
                name: n.name,
                parser: n.parser.clone(),
                is_root: false,
                level: n.level,
                executable: n.executable,
                children: Vec::new(),
                redirect: None,
            });
            g.nodes[parent].children.push(idx);
            if let Some(path) = n.redirect {
                pending.push((idx, path));
            }
            Self::lower_into(&n.children, idx, g, pending);
        }
    }

    /// The `ClientboundCommandsPacket` body for a source at `level`.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn encode(&self, level: Level) -> io::Result<Vec<u8>> {
        self.lower().encode(level)
    }

    /// Parse a command against this tree, as Brigadier's
    /// `CommandDispatcher.parseNodes` does.
    ///
    /// # This is a transcription now, not a tokeniser
    ///
    /// Until 2026-09-05 this split on whitespace and gave each argument node
    /// one token. That model cannot express a `minecraft:block_pos` (three
    /// tokens, one node) or a greedy `minecraft:message` (every token, one
    /// node), so every command taking either was unreachable regardless of
    /// whether its executor existed. It now runs [`crate::cmdreader::Reader`]
    /// over the raw text and lets each parser consume what vanilla's own
    /// `ArgumentType.parse` consumes.
    ///
    /// The three structural rules that come with that, all from
    /// `parseNodes`' own bytecode:
    ///
    /// * **`getRelevantNodes`**: if this node has literal children and the
    ///   next space-delimited word is exactly one of them, that literal is
    ///   the *only* candidate. Otherwise every argument child is a candidate
    ///   and no literal is. A token that is both a literal and a valid
    ///   argument therefore takes the literal, and a token that is neither
    ///   does not fall back to a near-miss literal.
    /// * **Every candidate gets its own copy of the reader**, and a
    ///   candidate that raises is recorded and the cursor put back. The next
    ///   candidate sees exactly what the first one did.
    /// * **After a node parses, the input must be exhausted or the next
    ///   character must be a space** (`dispatcherExpectedArgumentSeparator`),
    ///   and descending needs `canRead(2)` — a trailing space with nothing
    ///   after it does not enter the children.
    ///
    /// Where several branches parse, Brigadier sorts them: a result that
    /// consumed the whole input beats one that did not, then a result with no
    /// recorded exception beats one with. The sort is stable, so ties keep
    /// candidate order. [`Tree::resolve`] reproduces that and takes the
    /// first.
    ///
    /// # What is still not Brigadier
    ///
    /// A redirect is followed by **path**, resolved against this tree's own
    /// roots, because [`Node::redirect`] is a path — see its doc comment for
    /// why a builder tree cannot hold an index. Vanilla's fork/modifier
    /// machinery (`/execute`'s `as`, `at`, `if`) has no equivalent here at
    /// all: a redirect that carried a `RedirectModifier` would need a
    /// per-branch source, and this server has one source per command.
    ///
    /// # Errors
    /// [`RefusalKind::UnknownCommand`] where nothing at this point in the tree matched,
    /// or where the level may not use what did — the two are deliberately the
    /// same answer, because `fillUsableCommands` prunes an unusable node
    /// entirely and a client was never told it existed.
    /// [`Refusal::Incomplete`] where a real node parsed and runs nothing.
    /// [`Refusal::Unparsed`] where the only candidate that could have matched
    /// carries a parser this server has not transcribed — a statement about
    /// this server, not about the input.
    pub fn resolve(&self, text: &str, level: Level) -> Result<Resolved, Refusal> {
        let r = Reader::new(text);
        let mut acc = Resolved::default();
        let out = self.parse_nodes(&self.roots, &r, level, &mut acc, 0);
        let (resolved, reader, errors, first_error) =
            (out.resolved, out.reader, out.errors, out.first_error);
        // An unported parser means a candidate vanilla would have TRIED was
        // skipped, so this parse's exception count and matched range are not
        // vanilla's and neither is any exception derived from them. Reported
        // as itself and rendered as nothing; see [`Refusal::Unparsed`].
        if acc.unsupported || resolved.unsupported {
            return Err(Refusal::Unparsed);
        }
        // `Commands.getParseException`, in its order.
        if reader.can_read() {
            if errors == 1 {
                // The one candidate's own exception IS the answer, cursor and
                // all — `getParseException` returns the exception object, not
                // a fresh one made at the reader.
                let (syntax, cursor) = first_error.expect("one error, one record");
                return Err(Refusal::Argument { cursor, syntax });
            }
            return Err(if resolved.path.is_empty() {
                Refusal::UnknownCommand {
                    cursor: reader.cursor(),
                }
            } else {
                Refusal::UnknownArgument {
                    cursor: reader.cursor(),
                }
            });
        }
        // The reader is exhausted, so `validateParseResults` passes and
        // `finishParsing` goes on to `ContextChain.tryFlatten(...)
        // .orElseThrow(dispatcherUnknownCommand)`. A context whose last node
        // has no `Command` fails to flatten, and the cursor is the reader's —
        // the END of the input, which is why `/msg` underlines nothing and
        // shows `msg` as context.
        if resolved.executable {
            Ok(resolved)
        } else {
            Err(Refusal::UnknownCommand {
                cursor: reader.cursor(),
            })
        }
    }

    /// One level of `parseNodes`, returning the winning branch, the reader it
    /// left behind, and **how many candidates raised at the level that
    /// produced that result**.
    ///
    /// That third value is `ParseResults.getExceptions().size()`, and it is
    /// not bookkeeping: `Commands.getParseException` answers "exactly one
    /// candidate raised" differently from "none did", and the difference is
    /// `parsing.bool.invalid` against `command.unknown.argument`. It
    /// propagates the way Brigadier's does — the chosen potential's own map,
    /// or this level's if no potential survived — rather than being summed.
    ///
    /// `unsupported` accumulates into `acc` rather than being returned,
    /// because a branch blocked by an unported parser is not a *result* — it
    /// is the reason there is no result, and only the top level can say so.
    ///
    /// `depth` bounds the redirect walk. A redirect cycle is legal in
    /// vanilla's graph (`/execute run execute run …` is exactly one) and is
    /// bounded there by the input running out; it is bounded here the same
    /// way, since every hop consumes at least the separator, but the counter
    /// is kept as a second bound so that a future redirect that consumes
    /// nothing cannot spin a connection thread.
    fn parse_nodes<'t, 'i>(
        &'t self,
        here: &'t [Node],
        r: &Reader<'i>,
        level: Level,
        acc: &mut Resolved,
        depth: usize,
    ) -> ParseOutcome<'i> {
        if depth > MAX_REDIRECT_DEPTH {
            return ParseOutcome {
                resolved: acc.clone(),
                reader: r.clone(),
                errors: 0,
                first_error: None,
            };
        }
        // `CommandNode.getRelevantNodes`: the next space-delimited word
        // against the literal map first.
        let word = {
            let mut probe = r.clone();
            while probe.can_read() && probe.peek() != Some(' ') {
                probe.skip();
            }
            &r.string()[r.cursor()..probe.cursor()]
        };
        let literal = here
            .iter()
            .find(|n| n.parser.is_none() && n.name == word)
            .map(std::slice::from_ref);
        let candidates: &[Node] = literal.unwrap_or(here);
        let only_literal = literal.is_some();

        // `errors` at THIS level: one entry per candidate that raised. Only
        // its size is ever read, so a count is the whole of it.
        let mut errors = 0usize;
        // The single-exception arm needs the exception itself, not just the
        // count, and `getParseException` reads `values().iterator().next()` —
        // the first inserted, since the map is a `LinkedHashMap`.
        let mut first_error: Option<(Syntax, usize)> = None;
        let record = |errors: &mut usize,
                      first: &mut Option<(Syntax, usize)>,
                      syntax: Syntax,
                      cursor: usize| {
            *errors += 1;
            if first.is_none() {
                *first = Some((syntax, cursor));
            }
        };
        let mut potentials: Vec<ParseOutcome<'i>> = Vec::new();
        for n in candidates {
            // `getRelevantNodes` returns `arguments.values()` when no literal
            // matched, so an unmatched literal is not a candidate at all.
            if !only_literal && n.parser.is_none() {
                continue;
            }
            // `CommandNode.canUse(source)`.
            if !level.allows(n.level) {
                continue;
            }
            let mut cr = r.clone();
            if let Some(p) = &n.parser {
                match p.parse(&mut cr) {
                    Ok(()) => {}
                    Err(ArgError::Unsupported) => {
                        acc.unsupported = true;
                        continue;
                    }
                    Err(ArgError::Syntax(e)) => {
                        record(&mut errors, &mut first_error, e, cr.cursor());
                        continue;
                    }
                }
            } else {
                // `LiteralCommandNode.parse`: the literal, then end-or-space.
                // A mismatch is `literalIncorrect`, which is an exception and
                // is counted — but it is unreachable here, because
                // `getRelevantNodes` only offers a literal whose whole word
                // already matched.
                if !cr.remaining().starts_with(n.name) {
                    record(
                        &mut errors,
                        &mut first_error,
                        Syntax::span("argument.literal.incorrect", cr.cursor(), cr.cursor()),
                        cr.cursor(),
                    );
                    continue;
                }
                cr.set_cursor(cr.cursor() + n.name.len());
                if cr.can_read() && cr.peek() != Some(' ') {
                    record(
                        &mut errors,
                        &mut first_error,
                        Syntax::span("argument.literal.incorrect", r.cursor(), cr.cursor()),
                        cr.cursor(),
                    );
                    continue;
                }
            }
            // `dispatcherExpectedArgumentSeparator`. A candidate that raises
            // here is recorded and dropped, exactly as one that raised inside
            // its own parse is: `parseNodes` puts both in the same exception
            // map and moves to the next relevant node.
            if cr.can_read() && cr.peek() != Some(' ') {
                record(
                    &mut errors,
                    &mut first_error,
                    Syntax::simple("command.expected.separator"),
                    cr.cursor(),
                );
                continue;
            }
            let mut mine = acc.clone();
            mine.path.push(n.name);
            mine.executable = n.executable;
            if n.parser.is_some() {
                mine.args
                    .push((n.name, r.string()[r.cursor()..cr.cursor()].to_owned()));
            }
            // `canRead(redirect == null ? 2 : 1)`: descending needs a
            // separator AND something after it; following a redirect needs
            // only the separator, because the redirect target's own children
            // are what will be asked next.
            let need = if n.redirect.is_some() { 1 } else { 2 };
            if cr.can_read_len(need) {
                cr.skip();
                let next: &[Node] = match n.redirect {
                    // `redirect(dispatcher.getRoot())` — `execute run`. The
                    // root is not a `Node` here, so it is matched by its
                    // empty path rather than looked up.
                    Some([]) => {
                        mine.chain.push((
                            std::mem::take(&mut mine.path),
                            std::mem::take(&mut mine.args),
                        ));
                        &self.roots
                    }
                    Some(path) => match self.at(path) {
                        Some(target) => {
                            // **A REDIRECT RESTARTS THE PATH AT ITS TARGET**,
                            // and that is `parseNodes` rather than a
                            // convenience. Following one builds
                            // `new CommandContextBuilder(dispatcher, source,
                            // node.getRedirect(), cursor)` — a FRESH context
                            // rooted at the target — and attaches it with
                            // `withChild`; `execute` then runs the last child
                            // context's command. So the context that dispatches
                            // `/tell oracle hi` is rooted at `msg` and holds
                            // `targets` and `message`, and `tell` lives only in
                            // the parent context.
                            //
                            // Keeping `tell` at the head instead would give
                            // every redirect root its own executor arm for the
                            // same command, which is exactly the duplication
                            // `redirect` exists to avoid.
                            mine.chain.push((
                                std::mem::replace(&mut mine.path, vec![target.name]),
                                std::mem::take(&mut mine.args),
                            ));
                            &target.children
                        }
                        // A redirect path that names nothing is a build
                        // error; `Tree::lower` panics on one and this is the
                        // read side of the same fact. Refusing is the safe
                        // half: it cannot take a connection thread down.
                        None => {
                            record(&mut errors, &mut first_error, Syntax::Unmapped, cr.cursor());
                            continue;
                        }
                    },
                    None => &n.children,
                };
                let deep = self.parse_nodes(next, &cr, level, &mut mine, depth + 1);
                potentials.push(deep);
            } else {
                // `new ParseResults(context, reader, Collections.emptyMap())`
                // — a leaf potential carries NO exceptions, whatever this
                // level recorded.
                potentials.push(ParseOutcome {
                    resolved: mine,
                    reader: cr,
                    errors: 0,
                    first_error: None,
                });
            }
        }
        // `list.sort(...)`: fully-consumed first, then error-free. A stable
        // sort, so equal candidates keep registration order.
        potentials.sort_by_key(|p| (p.reader.can_read(), p.errors != 0));
        match potentials.into_iter().next() {
            Some(p) => {
                acc.unsupported |= p.resolved.unsupported;
                p
            }
            // `new ParseResults(contextSoFar, originalReader, errors)`: the
            // context as it was, the reader as it was, and THIS level's map.
            None => ParseOutcome {
                resolved: acc.clone(),
                reader: r.clone(),
                errors,
                first_error,
            },
        }
    }

    /// `CommandNode.getUsageText()` — a literal's literal, an argument's
    /// `"<" + name + ">"`.
    ///
    /// Free-standing rather than a `Node` method because it is Brigadier's
    /// vocabulary and not this server's: nothing but usage generation reads
    /// it.
    fn usage_text(n: &Node) -> String {
        if n.parser.is_some() {
            format!("<{}>", n.name)
        } else {
            n.name.to_string()
        }
    }

    /// `CommandDispatcher.getSmartUsage(node, source)` — one usage string per
    /// usable child, in child order.
    ///
    /// The map vanilla returns is a `LinkedHashMap` keyed by child node; only
    /// its **values** are read (`HelpCommand` iterates `values()`), so this
    /// returns the strings in the same order and drops the keys.
    ///
    /// `optional` for the top call is `node.getCommand() != null`, which is
    /// how `/help` on the root — a root has no command — produces bare
    /// `list`, `seed`, … rather than `[list]`, `[seed]`.
    #[must_use]
    pub fn smart_usage(&self, from: &[Node], executable: bool, level: Level) -> Vec<String> {
        from.iter()
            .filter_map(|c| self.smart_usage_of(c, level, executable, false))
            .collect()
    }

    /// `getSmartUsage(node, source, optional, deep)`, transcribed.
    ///
    /// # The two booleans are not the same question and the names hide it
    ///
    /// `optional` decides whether **this** node's own text is bracketed.
    /// `deep` decides whether the recursion looks at children at all: a `true`
    /// returns the node's own text and stops, which is what makes a two-child
    /// alternation render as `(a|b)` rather than as two full sub-usages.
    /// They are passed the same value in one place — the single-child arm,
    /// `getSmartUsage(child, source, childOptional, childOptional)` — and that
    /// coincidence is the single most misreadable line in the method.
    ///
    /// # `childOptional` is the PARENT's executability, not the child's
    ///
    /// `open`/`close` are `[`/`]` when this node has a command and `(`/`)`
    /// when it does not, which reads backwards until you see why: if the
    /// parent can already run, everything below it is optional.
    fn smart_usage_of(&self, n: &Node, level: Level, optional: bool, deep: bool) -> Option<String> {
        // `CommandNode.canUse(source)`.
        if !level.allows(n.level) {
            return None;
        }
        let text = Self::usage_text(n);
        let self_text = if optional { format!("[{text}]") } else { text };
        let child_optional = n.executable;
        let (open, close) = if child_optional {
            ("[", "]")
        } else {
            ("(", ")")
        };
        if deep {
            return Some(self_text);
        }
        if let Some(path) = n.redirect {
            // `node.getRedirect() == root` renders as `...` and is `/execute
            // run`'s, which `Node::redirect` writes as the EMPTY path; every
            // other redirect is the `-> ` form.
            //
            // **THE SPACE AFTER THE ARROW IS REAL AND `javap` DOES NOT SHOW
            // IT.** The constant pool entry renders as `-> ` with its trailing
            // space stripped, exactly as `ARGUMENT_SEPARATOR` renders as
            // nothing at all — javap trims trailing whitespace on a `Utf8`
            // line, confirmed by `od -c` on both. Transcribed from the listing
            // this produces `tell ->msg`; vanilla's own `getSmartUsage` over
            // this very tree produces `tell -> msg`, and the golden is what
            // caught it.
            // `getRedirect() == root` is the `...` form — `/execute run`.
            if path.is_empty() {
                return Some(format!("{self_text} ..."));
            }
            let target = self.at(path)?;
            return Some(format!("{self_text} -> {}", Self::usage_text(target)));
        }
        let usable: Vec<&Node> = n
            .children
            .iter()
            .filter(|c| level.allows(c.level))
            .collect();
        match usable.len() {
            0 => {}
            1 => {
                if let Some(u) =
                    self.smart_usage_of(usable[0], level, child_optional, child_optional)
                {
                    return Some(format!("{self_text} {u}"));
                }
            }
            _ => {
                // A `LinkedHashSet`: distinct strings, insertion-ordered. Two
                // children whose deep usage is the same string collapse to
                // one, which is what makes a set rather than a list the right
                // container and is observable — `/gamerule`'s bare and
                // namespaced literals do NOT collapse (different text) but two
                // arguments of the same name would.
                let mut distinct: Vec<String> = Vec::new();
                for c in &usable {
                    if let Some(u) = self.smart_usage_of(c, level, child_optional, true) {
                        if !distinct.contains(&u) {
                            distinct.push(u);
                        }
                    }
                }
                if distinct.len() == 1 {
                    let u = &distinct[0];
                    let wrapped = if child_optional {
                        format!("[{u}]")
                    } else {
                        u.clone()
                    };
                    return Some(format!("{self_text} {wrapped}"));
                }
                if distinct.len() > 1 {
                    // NOTE the source of the alternation text: it is
                    // `child.getUsageText()` for every usable child, NOT the
                    // deep usages just collected. The set was only used to
                    // decide which arm to take.
                    let mut b = String::from(open);
                    for (i, c) in usable.iter().enumerate() {
                        if i > 0 {
                            b.push('|');
                        }
                        b.push_str(&Self::usage_text(c));
                    }
                    b.push_str(close);
                    return Some(format!("{self_text} {b}"));
                }
            }
        }
        Some(self_text)
    }

    /// `ParseResults.getContext().getNodes()` — the nodes the **root**
    /// context matched, which is what `/help <command>` takes its last node
    /// from.
    ///
    /// **A redirect ends this walk.** Brigadier puts everything past a
    /// redirect in a *child* context, and `HelpCommand` reads
    /// `getContext().getNodes()` — the root one — so `/help tell oracle`
    /// resolves to `tell`, whose children are empty, and vanilla prints
    /// nothing. That is why this is a separate walk from
    /// [`Tree::parse_nodes`], which follows redirects because dispatch must.
    ///
    /// Returns the node reached and the text consumed to reach it, or `None`
    /// if the first token matched nothing — `HelpCommand`'s
    /// `getNodes().isEmpty()` arm, which is `commands.help.failed`.
    #[must_use]
    pub fn usage_target(&self, text: &str, level: Level) -> Option<(&Node, String)> {
        let mut here: &[Node] = &self.roots;
        let mut found;
        let mut consumed = String::new();
        let mut r = Reader::new(text);
        loop {
            let start = r.cursor();
            while r.can_read() && r.peek() != Some(' ') {
                r.skip();
            }
            let word = &text[start..r.cursor()];
            let node = here
                .iter()
                .find(|n| n.parser.is_none() && n.name == word && level.allows(n.level))?;
            if !consumed.is_empty() {
                consumed.push(' ');
            }
            consumed.push_str(word);
            found = Some((node, consumed.clone()));
            if node.redirect.is_some() || !r.can_read() {
                break;
            }
            r.skip();
            here = &node.children;
        }
        found
    }

    /// The node a redirect path names, walked from this tree's roots.
    fn at(&self, path: &[&str]) -> Option<&Node> {
        let mut here = &self.roots;
        let mut found: Option<&Node> = None;
        for seg in path {
            let n = here.iter().find(|n| n.name == *seg)?;
            found = Some(n);
            here = &n.children;
        }
        found
    }
}

/// `ParseResults` — what one level of `parseNodes` hands back.
///
/// A struct rather than a tuple because it is four things now and two of them
/// are only meaningful together: `errors` decides WHICH arm of
/// `Commands.getParseException` fires, and `first_error` is what that arm
/// returns when the count is one.
struct ParseOutcome<'i> {
    resolved: Resolved,
    reader: Reader<'i>,
    /// `ParseResults.getExceptions().size()`.
    errors: usize,
    /// The first exception this level recorded, with the cursor
    /// `createWithContext` stamped on it. `getParseException` reads
    /// `values().iterator().next()` of a `LinkedHashMap`, which is the first
    /// inserted.
    first_error: Option<(Syntax, usize)>,
}

/// A command that resolved to an executable node.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Node names along the matched path: a literal's literal, an argument's
    /// argument name.
    pub path: Vec<&'static str>,
    /// Whether the node this path ends on has a `Command`.
    ///
    /// Carried rather than re-derived because the winning branch is chosen
    /// after the walk, and the node it ended on is no longer in hand.
    pub executable: bool,
    /// Whether any candidate anywhere in this parse was blocked by a parser
    /// this server has not transcribed.
    ///
    /// This is the field that keeps [`Refusal::Unparsed`] honest. Without it
    /// an unported parser is indistinguishable from no such command, and
    /// this server's whole unported-argument surface would report as typos.
    pub unsupported: bool,
    /// The token each argument node consumed, in path order — of the LAST
    /// context only; see [`Self::chain`].
    pub args: Vec<(&'static str, String)>,
    /// The parent contexts a redirect left behind, outermost first: each one's
    /// path and the arguments it consumed.
    ///
    /// `parseNodes` builds a FRESH `CommandContextBuilder` at every redirect
    /// and attaches it with `withChild`, so `execute as oracle run gamemode
    /// survival` is three contexts — `execute as targets`, `execute run`, and
    /// `gamemode gamemode` — and the command runs in the last one with the
    /// source the first two produced. [`Self::path`] and [`Self::args`] are
    /// that last context; this is everything before it, which is where an
    /// `execute` modifier's arguments live. A plain alias redirect (`tell`,
    /// `tp`, `xp`) leaves one argument-less entry here.
    pub chain: Vec<ParentContext>,
}

/// One parent context of a [`Resolved`]: the node names along its path and
/// the tokens its argument nodes consumed.
pub type ParentContext = (Vec<&'static str>, Vec<(&'static str, String)>);

impl Resolved {
    /// The token an argument node with this name consumed.
    #[must_use]
    pub fn arg(&self, name: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// A [`Refusal`] with its cursor dropped.
///
/// Exists because most callers — and nearly every test — care WHICH exception
/// vanilla would raise and not where it points, and comparing whole refusals
/// would make each of those assertions carry a cursor it has no opinion about.
/// The cursor is the second failure component's whole shape, so it is asserted
/// where it matters: `crates/rustmc-server/tests/command_failure_vanilla.rs`
/// compares the rendered bytes, which cannot be right with a wrong cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    UnknownCommand,
    UnknownArgument,
    Argument,
    Unparsed,
}

impl Refusal {
    /// This refusal without its cursor.
    #[must_use]
    pub const fn kind(self) -> RefusalKind {
        match self {
            Self::UnknownCommand { .. } => RefusalKind::UnknownCommand,
            Self::UnknownArgument { .. } => RefusalKind::UnknownArgument,
            Self::Argument { .. } => RefusalKind::Argument,
            Self::Unparsed => RefusalKind::Unparsed,
        }
    }

    /// The Brigadier translation key this refusal renders as, or `None` for
    /// the two this server does not render.
    ///
    /// `BrigadierExceptions` is what supplies these: vanilla replaces
    /// Brigadier's `LiteralMessage`s with `Component.translatable`, so the
    /// first failure line is a translation key and not an English string.
    #[must_use]
    pub const fn syntax(self) -> Option<Syntax> {
        match self {
            Self::UnknownCommand { .. } => Some(Syntax::simple("command.unknown.command")),
            Self::UnknownArgument { .. } => Some(Syntax::simple("command.unknown.argument")),
            Self::Argument { syntax, .. } => Some(syntax),
            Self::Unparsed => None,
        }
    }

    /// The cursor `createWithContext(reader)` stamped, or `None` where this
    /// server has no exception to stamp one on.
    #[must_use]
    pub const fn cursor(self) -> Option<usize> {
        match self {
            Self::UnknownCommand { cursor }
            | Self::UnknownArgument { cursor }
            | Self::Argument { cursor, .. } => Some(cursor),
            Self::Unparsed => None,
        }
    }
}

/// Why a command did not run — and, for the three this server can answer,
/// enough to answer it.
///
/// # These are `Commands.getParseException`'s arms, not a taxonomy
///
/// Each variant is one exception vanilla raises, with the cursor
/// `createWithContext(reader)` would have stamped on it, because the cursor is
/// what the second failure component is built around. The three named ones are
/// the whole of the **dispatcher's** vocabulary and they are the answer for
/// every typo, every root this server has not ported, and every command a
/// player lacks permission for.
///
/// `crate::commands::failure_components` renders them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal {
    /// `dispatcherUnknownCommand` — `command.unknown.command`.
    ///
    /// Two quite different situations reach it and vanilla makes no
    /// distinction: nothing matched at all (the context range is empty), and
    /// everything matched but the node it ended on has no `Command`
    /// (`ContextChain.tryFlatten` fails). The cursor differs — 0 in the first
    /// case, the end of the input in the second — which is why it is carried
    /// rather than recomputed.
    UnknownCommand { cursor: usize },
    /// `dispatcherUnknownArgument` — `command.unknown.argument`. A prefix of
    /// the input matched and the rest did not.
    UnknownArgument { cursor: usize },
    /// `getParseException`'s first arm: exactly one candidate raised, and
    /// **its own** exception is the answer.
    ///
    /// The cursor is that exception's, not the reader's: every argument type
    /// rewinds before raising, so it is the start of the argument rather than
    /// wherever the failed read stopped.
    ///
    /// A [`Syntax::Known`] renders; a [`Syntax::Unmapped`] does not, and is
    /// the residue `crates/rustmc-server/tests/command_failure_vanilla.rs`
    /// ratchets.
    Argument { cursor: usize, syntax: Syntax },
    /// An argument node whose parser this server cannot tokenise. **A
    /// statement about this server, not about the input**, and it suppresses
    /// the reply as well as the command: a candidate vanilla would have tried
    /// was skipped, so this parse's exception count and matched range are not
    /// vanilla's, and neither is any exception derived from them.
    Unparsed,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path relative to the repo root.
    ///
    /// A lib test's working directory is the *package* directory, not the
    /// workspace root, so the bare `data/vanilla/...` the server itself uses
    /// resolves to nothing here. Copied from `crate::power`'s helper, which
    /// exists for the same reason — and **this test skipped silently for one
    /// run before it was adopted**, which is precisely the vacuous-gate
    /// failure the golden was written to avoid.
    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// The golden, or a loud failure.
    ///
    /// **Not a skip.** A skip here is a gate that cannot fail, and the whole
    /// value of this file is that it can. `ci/gen-gameplay-tables.sh`
    /// generates the table in every tree, so its absence is a broken setup,
    /// not a legitimate checkout state.
    fn golden() -> serde_json::Value {
        let path = repo("data/vanilla/commands_tree.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
        serde_json::from_str(&text).expect("the golden is json")
    }

    /// The tree under test, which must be [`crate::commands::tree`] — a test
    /// that built its own would gate nothing.
    fn tree() -> &'static Tree {
        crate::commands::tree()
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// **The gate.** Every level's packet body, byte for byte, against bytes
    /// produced by vanilla's own `ClientboundCommandsPacket` constructor,
    /// vanilla's own `COMMAND_NODE_INSPECTOR` and vanilla's own stream codec,
    /// for a dispatcher holding exactly the commands this server registers.
    ///
    /// Generated by `ci/commands-golden.sh`. See
    /// `tools/goldengen/vanilla/CommandsGolden.java` for why the golden is
    /// built rather than captured.
    #[test]
    fn every_level_matches_vanillas_own_encoder() {
        let doc = golden();
        let levels = doc["levels"].as_array().expect("levels");
        assert_eq!(levels.len(), 5, "five permission levels");

        let t = tree();
        for (row, level) in levels.iter().zip(Level::ALL) {
            let want = row["packetHex"].as_str().expect("packetHex");
            let got = t.encode(level).expect("encode");
            assert_eq!(
                hex(&got),
                want,
                "level {level:?} ({}) disagrees with vanilla",
                row["level"]
            );
            assert_eq!(
                row["entityEventByte"].as_u64(),
                Some(u64::from(level.entity_event_byte())),
                "entity-event byte for {level:?}"
            );
        }
    }

    /// The golden must not be five copies of one row, or the test above
    /// cannot fail. Asserted here rather than trusted: the Java side takes a
    /// boolean that, set wrongly, makes `/seed` level-`ALL` and every row
    /// identical.
    #[test]
    fn the_golden_actually_varies_by_level() {
        let doc = golden();
        let all = doc["levels"][0]["packetHex"].as_str().expect("all");
        let gm = doc["levels"][2]["packetHex"].as_str().expect("gamemasters");
        assert_ne!(all, gm, "ALL and GAMEMASTERS must differ");
    }

    /// The shallow shape: siblings before grandchildren.
    ///
    /// This is the real tree's shape and it is **not enough on its own** —
    /// see [`numbering_is_breadth_first_not_depth_first`] for why.
    #[test]
    fn numbering_puts_siblings_before_grandchildren() {
        let t = Tree {
            roots: vec![
                Node::leaf("list").then(Node::leaf("uuids")),
                Node::leaf("seed").requires(Level::Gamemasters),
            ],
        };
        let flat = t.lower().wire_order(Level::Owners);
        assert_eq!(flat.len(), 4);
        assert_eq!(flat[0].1, vec![1, 2], "root's children in order");
        assert_eq!(flat[1].0, "list");
        assert_eq!(flat[2].0, "seed");
        assert_eq!(flat[3].0, "uuids");
        assert_eq!(flat[1].1, vec![3], "uuids is numbered after seed");
    }

    /// **Breadth first, on a tree that can tell the difference.**
    ///
    /// Added 2026-08-06 because `ci/chat-commands-falsify.sh` turned the
    /// queue's `push_back` into a `push_front` — a depth-first walk — and
    /// **nothing caught it**. Neither the test above nor the vanilla golden
    /// could: this server's real tree has exactly one branching parent
    /// (`list`), and with only one parent to enqueue there is no order for
    /// the two disciplines to disagree about. The property was asserted by a
    /// fixture incapable of falsifying it.
    ///
    /// Two branching parents is the smallest shape that separates them.
    /// Breadth first numbers `a1` before `b1`; depth first numbers `b1`
    /// first, because `b`'s children are pushed last and popped first.
    #[test]
    fn numbering_is_breadth_first_not_depth_first() {
        let t = Tree {
            roots: vec![
                Node::leaf("a").then(Node::leaf("a1")),
                Node::leaf("b").then(Node::leaf("b1")),
            ],
        };
        let flat = t.lower().wire_order(Level::All);
        assert_eq!(flat.len(), 5);
        assert_eq!(flat[1].0, "a");
        assert_eq!(flat[2].0, "b");
        assert_eq!(
            flat[3].0, "a1",
            "breadth first: the FIRST parent's child is numbered first"
        );
        assert_eq!(flat[4].0, "b1");
        assert_eq!(flat[1].1, vec![3]);
        assert_eq!(flat[2].1, vec![4]);
    }

    /// Registration order reaches the wire. Two trees differing only in the
    /// order of two sibling literals must encode differently — if they did
    /// not, `getChildren()` would be being sorted somewhere and the golden
    /// would be passing by luck.
    #[test]
    fn sibling_order_changes_the_bytes() {
        let a = Tree {
            roots: vec![Node::leaf("list"), Node::leaf("seed")],
        };
        let b = Tree {
            roots: vec![Node::leaf("seed"), Node::leaf("list")],
        };
        assert_ne!(
            a.encode(Level::Owners).expect("a"),
            b.encode(Level::Owners).expect("b")
        );
    }

    /// The filter prunes whole subtrees, not just the node that failed.
    #[test]
    fn an_unusable_parent_hides_its_usable_children() {
        let t = Tree {
            roots: vec![Node::leaf("op")
                .requires(Level::Admins)
                .then(Node::leaf("free"))],
        };
        // At `All` the parent is gone, so the child cannot be reached even
        // though the child itself requires nothing.
        let body = t.encode(Level::All).expect("encode");
        assert_eq!(body, vec![0x01, TYPE_ROOT, 0x00, 0x00], "root alone");
        assert!(t.resolve("op free", Level::All).is_err());
        assert_eq!(
            t.resolve("op free", Level::Admins).expect("admin").path,
            vec!["op", "free"]
        );
    }

    /// **The filter recurses, and a usable parent does not carry an
    /// unusable child through with it.**
    ///
    /// Added 2026-08-06 because `ci/chat-commands-falsify.sh` replaced
    /// `Self::usable(&n.children, level)` with `n.children.clone()` — a
    /// filter that prunes only the top level — and **nothing caught it**.
    ///
    /// The reason is the shape of the real tree, and it is the same reason
    /// the depth-first injection went unnoticed: every node this server
    /// registers is either level 0 with level-0 children (`list`, `uuids`) or
    /// level 2 with no children at all (`seed`). There is no *usable parent
    /// with an unusable child* anywhere in it, so the recursive half of the
    /// filter is never asked a question it could answer wrongly.
    /// [`an_unusable_parent_hides_its_usable_children`] tests the other
    /// direction and cannot see this one: it prunes at the top level, where
    /// the non-recursive filter is already correct.
    #[test]
    fn a_usable_parent_does_not_carry_an_unusable_child() {
        let t = Tree {
            roots: vec![Node::leaf("list")
                .then(Node::leaf("uuids"))
                .then(Node::leaf("ips").requires(Level::Admins))],
        };
        // At level 0 the parent survives and only one of its two children
        // does, so the packet is root + list + uuids: three entries.
        let body = t.encode(Level::All).expect("encode");
        assert_eq!(body[0], 3, "root, list, uuids — and NOT ips");
        assert!(
            t.resolve("list ips", Level::All).is_err(),
            "an admin-only child is not reachable at level 0"
        );
        // And at admin level all four are there.
        let admin = t.encode(Level::Admins).expect("encode");
        assert_eq!(admin[0], 4, "root, list, uuids, ips");
        assert_eq!(
            t.resolve("list ips", Level::Admins).expect("admin").path,
            vec!["list", "ips"]
        );
    }

    /// `restricted` is computed against a no-permission source, not against
    /// the recipient — so a node an owner may use is still flagged.
    #[test]
    fn restricted_is_absolute_not_relative_to_the_recipient() {
        let seed = Node::leaf("seed").requires(Level::Gamemasters);
        assert!(seed.restricted());
        assert!(!Node::leaf("list").restricted());
        let t = Tree { roots: vec![seed] };
        let body = t.encode(Level::Owners).expect("encode");
        // root, then the seed entry: flags = literal|executable|restricted.
        assert_eq!(body[1], TYPE_ROOT);
        assert_eq!(
            body[4],
            TYPE_LITERAL | FLAG_EXECUTABLE | FLAG_RESTRICTED,
            "0x25, as in the golden"
        );
    }

    /// The entity-event bytes are 24..28, and they are what tells a client to
    /// enable its own command UI.
    #[test]
    fn permission_bytes_are_24_through_28() {
        let got: Vec<u8> = Level::ALL.iter().map(|l| l.entity_event_byte()).collect();
        assert_eq!(got, vec![24, 25, 26, 27, 28]);
        assert_eq!(Level::from_ordinal(-1), Level::All);
        assert_eq!(Level::from_ordinal(9), Level::Owners);
    }

    /// Resolution is whole-token, so `list` does not match `listen` and a
    /// non-executable node is `Incomplete` rather than silently run.
    ///
    /// # The padded-input case is a CORRECTION, not a regression
    ///
    /// This test asserted, until 2026-09-05, that `"  list   uuids  "`
    /// resolves, with the comment *"extra whitespace is what
    /// `split_whitespace` eats, and Brigadier tolerates it the same way"*.
    /// **Brigadier does not.** `CommandDispatcher.parseNodes` skips exactly
    /// **one** separator character (`skip()`, one call, offset 219) and
    /// `Commands.trimOptionalPrefix` removes a leading `/` and nothing else —
    /// no `trim`, no `strip`, nowhere between the packet and the dispatcher.
    /// So a doubled space, a leading space or a trailing space is
    /// `dispatcherExpectedArgumentSeparator` or an unmatched empty literal on
    /// a real 26.2 server, and the old assertion was a property of the
    /// tokeniser being tested, described as a property of vanilla.
    #[test]
    fn resolution_matches_whole_tokens() {
        let t = tree();
        assert_eq!(
            t.resolve("list", Level::All).expect("list").path,
            vec!["list"]
        );
        assert_eq!(
            t.resolve("list uuids", Level::All).expect("uuids").path,
            vec!["list", "uuids"]
        );
        assert_eq!(
            t.resolve("listen", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        // `list` matched and `oops` did not, so the context range is not
        // empty and the exception is `dispatcherUnknownArgument` — a
        // different message from `listen`, which matched nothing at all.
        assert_eq!(
            t.resolve("list oops", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownArgument)
        );
        assert_eq!(
            t.resolve("", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        // All four are syntax errors in vanilla, and WHICH one differs by
        // where the parse got to: a leading space matches no literal at all
        // (`command.unknown.command`), a trailing or doubled space has
        // `list` behind it (`command.unknown.argument`). The old
        // whitespace-eating tokeniser accepted every one of them.
        for (padded, kind) in [
            ("  list   uuids  ", RefusalKind::UnknownCommand),
            (" list", RefusalKind::UnknownCommand),
            ("list ", RefusalKind::UnknownArgument),
            ("list  uuids", RefusalKind::UnknownArgument),
        ] {
            assert_eq!(
                t.resolve(padded, Level::All).map_err(Refusal::kind),
                Err(kind),
                "{padded:?} is a syntax error in vanilla and must be one here"
            );
        }
    }

    /// A node carrying a parser this server has not transcribed, for the
    /// tests below that need one.
    ///
    /// **This was `minecraft:vec3` and that broke, correctly, the moment
    /// `minecraft:vec3` was ported.** Which is the property to keep: the test
    /// must not be able to start passing for the wrong reason. `nbt_path` is
    /// the replacement because it is the second-commonest parser in vanilla's
    /// tree, needs a whole NBT path grammar, and is on no roadmap this branch
    /// has — so when it too is ported, this will go red again and want a new
    /// name, which is the correct amount of friction.
    fn unported_arg(name: &'static str) -> Node {
        Node {
            name,
            level: Level::All,
            executable: true,
            parser: Some(Parser {
                registry: "minecraft:nbt_path",
                props: Props::Empty,
                suggestions: None,
            }),
            children: Vec::new(),
            redirect: None,
        }
    }

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

    /// A greedy string is **one** argument holding every remaining
    /// character, spaces included. The tokeniser this replaced gave it the
    /// first word and left the rest as unmatched input, so every root taking
    /// a message or a greedy phrase was unreachable.
    #[test]
    fn a_greedy_string_takes_the_whole_remainder() {
        let t = Tree {
            roots: vec![Node::leaf("say").then(arg(
                "message",
                "brigadier:string",
                Props::Str(StringKind::Greedy),
            ))],
        };
        let mut root = t.roots[0].clone();
        root.executable = false;
        let t = Tree { roots: vec![root] };
        let r = t
            .resolve("say hello there  world", Level::All)
            .expect("greedy");
        assert_eq!(r.path, vec!["say", "message"]);
        assert_eq!(r.arg("message"), Some("hello there  world"));
    }

    /// A word argument stops at the first character outside
    /// `isAllowedInUnquotedString`, and the leftover then fails the whole
    /// parse rather than being silently dropped.
    #[test]
    fn a_word_argument_stops_at_a_disallowed_character() {
        let t = Tree {
            roots: vec![Node::leaf("x").then(arg(
                "w",
                "brigadier:string",
                Props::Str(StringKind::Word),
            ))],
        };
        let mut root = t.roots[0].clone();
        root.executable = false;
        let t = Tree { roots: vec![root] };
        assert_eq!(
            t.resolve("x abc", Level::All).expect("word").arg("w"),
            Some("abc")
        );
        // `abc[1]` reads `abc`, leaves `[1]`, and `[` is not a separator —
        // so the one candidate raised `dispatcherExpectedArgumentSeparator`
        // and `getParseException`'s size-one arm returns THAT rather than an
        // unknown-argument.
        assert_eq!(
            t.resolve("x abc[1]", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::Argument)
        );
    }

    /// The declared bounds are enforced, in both directions, and the value
    /// itself still has to be an integer.
    #[test]
    fn integer_bounds_are_the_declared_ones() {
        let t = Tree {
            roots: vec![Node::leaf("n").then(arg(
                "v",
                "brigadier:integer",
                Props::Int(Some(0), Some(8)),
            ))],
        };
        let mut root = t.roots[0].clone();
        root.executable = false;
        let t = Tree { roots: vec![root] };
        assert_eq!(
            t.resolve("n 8", Level::All).expect("in range").arg("v"),
            Some("8")
        );
        // One candidate, and it raised: `argument.integer.big` /
        // `argument.integer.low` / `parsing.int.expected` respectively, all
        // of which `getParseException` returns verbatim.
        for out in ["n 9", "n -1"] {
            assert_eq!(
                t.resolve(out, Level::All).map_err(Refusal::kind),
                Err(RefusalKind::Argument),
                "{out:?}"
            );
        }
        assert_eq!(
            t.resolve("n x", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::Argument)
        );
    }

    /// A parser this server cannot read is [`Refusal::Unparsed`] and not
    /// [`RefusalKind::UnknownCommand`]. That distinction is the only record of how much
    /// of vanilla's argument surface is unreachable, so it is asserted
    /// rather than left to the type.
    #[test]
    fn an_unported_parser_is_unparsed_and_not_unknown() {
        let mut root = Node::leaf("go");
        root.executable = false;
        let t = Tree {
            roots: vec![root.then(unported_arg("where"))],
        };
        assert_eq!(
            t.resolve("go 1 2 3", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::Unparsed)
        );
        // A root that does not exist at all is still `Unknown`, so the two
        // arms are not collapsing into one.
        assert_eq!(
            t.resolve("nope", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
    }

    /// `getRelevantNodes`: a word that matches a literal makes that literal
    /// the **only** candidate, so an argument sibling never sees it. A word
    /// that matches no literal skips every literal and goes to the
    /// arguments.
    #[test]
    fn a_matching_literal_excludes_its_argument_siblings() {
        let mut root = Node::leaf("t");
        root.executable = false;
        let t = Tree {
            roots: vec![root.then(Node::leaf("1")).then(arg(
                "v",
                "brigadier:integer",
                Props::Int(None, None),
            ))],
        };
        // "1" is both a literal and a valid integer; the literal wins.
        assert_eq!(
            t.resolve("t 1", Level::All).expect("literal").path.last(),
            Some(&"1")
        );
        // "2" is not a literal, so the argument is tried.
        assert_eq!(
            t.resolve("t 2", Level::All).expect("arg").path.last(),
            Some(&"v")
        );
    }

    /// A redirect is followed, the target's children are what parses next,
    /// and the resolved path is rooted at the TARGET — `parseNodes` starts a
    /// fresh context builder there and `execute` runs that context's command.
    #[test]
    fn a_redirect_reaches_its_targets_children() {
        let mut msg = Node::leaf("msg");
        msg.executable = false;
        let msg = msg.then(arg("m", "brigadier:string", Props::Str(StringKind::Greedy)));
        let mut tell = Node::leaf("tell");
        tell.executable = false;
        tell.redirect = Some(&["msg"]);
        let t = Tree {
            roots: vec![msg, tell],
        };
        let r = t.resolve("tell hi there", Level::All).expect("redirect");
        assert_eq!(r.path, vec!["msg", "m"], "the path is rooted at the target");
        assert_eq!(r.arg("m"), Some("hi there"));
    }

    /// `readBoolean` goes through `readString`, so vanilla accepts a quoted
    /// boolean and the tokeniser this replaced did not. Asserted against the
    /// **real** registry, since `/gamerule` is where it is reachable.
    #[test]
    fn a_quoted_boolean_reaches_a_real_gamerule() {
        let t = tree();
        for token in ["true", "\"true\"", "'true'"] {
            let text = format!("gamerule keep_inventory {token}");
            let r = t
                .resolve(&text, Level::Gamemasters)
                .unwrap_or_else(|e| panic!("{text:?}: {e:?}"));
            assert_eq!(r.path, vec!["gamerule", "keep_inventory", "value"]);
        }
        // The `value` node is the only candidate and `readBoolean` raised, so
        // this is `parsing.bool.invalid` and not an unknown command — which
        // is exactly the split `data/vanilla/commands_tree.json`'s `failures`
        // rows show between level ALL (where `/gamerule` is pruned entirely)
        // and GAMEMASTERS (where it is reached and the argument is wrong).
        assert_eq!(
            t.resolve("gamerule keep_inventory yes", Level::Gamemasters)
                .map_err(Refusal::kind),
            Err(RefusalKind::Argument)
        );
    }

    /// `/seed` is refused below GAMEMASTERS and runs at or above it. This is
    /// the dispatch half of the permission model; the encoding half is the
    /// golden above.
    #[test]
    fn seed_needs_gamemasters() {
        let t = tree();
        assert_eq!(
            t.resolve("seed", Level::All).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        assert_eq!(
            t.resolve("seed", Level::Moderators).map_err(Refusal::kind),
            Err(RefusalKind::UnknownCommand)
        );
        assert_eq!(
            t.resolve("seed", Level::Gamemasters).expect("gm").path,
            vec!["seed"]
        );
        assert_eq!(
            t.resolve("seed", Level::Owners).expect("owner").path,
            vec!["seed"]
        );
    }
}
