//! Player chat: `ServerboundChatPacket` in, `ClientboundPlayerChatPacket`
//! out.
//!
//! Before this module there was no serverbound chat packet in this server at
//! all — not unhandled, *unnameable*: `ids::play::sb` had no constant for id
//! 9, so a plugin could not have observed a chat message even in principle.
//!
//! Everything below is read from the pinned 26.2 jar. The full derivation,
//! with bytecode offsets, is in `docs/intel/chat-and-commands.md`; this header
//! carries the conclusions a reader of this file needs.
//!
//! # The crypto path is out of scope, and that is a measurement
//!
//! `ServerGamePacketListenerImpl`'s constructor installs
//! `SignedMessageChain.Decoder.unsigned(playerUuid, server::enforceSecureProfile)`,
//! and `DedicatedServer.enforceSecureProfile()` returns false unless
//! `enforce-secure-profile` **and** `online-mode` **and**
//! `services.canValidateProfileKeys()` all hold. This server sends
//! `online_mode: false` in its join packet, so the supplier is always false,
//! and `Decoder.unsigned` then **discards the signature argument entirely**
//! and returns `PlayerChatMessage.unsigned(uuid, content)`.
//!
//! So the unsigned path is not a simplification of vanilla — on this
//! configuration it *is* vanilla. What an online-mode server would
//! additionally need is listed in the intel doc §2 and is a sub-project:
//! `ProfilePublicKey` and its Yggdrasil signature, `SignedMessageChain` link
//! chaining, `MessageSignatureCache`, and `LastSeenMessagesValidator`'s
//! pending set. None of it is here.
//!
//! Two consequences follow and both are proved rather than assumed:
//!
//! * `sendPlayerChatMessage` returns at offset 66 on a null signature,
//!   **before** `MessageSignatureCache.push` and
//!   `LastSeenMessagesValidator.addPending`. Nothing is ever tracked, so the
//!   validator accepts every update a client can send and `chat_ack` has
//!   nothing to acknowledge. The last-seen machinery is *inert*, not skipped.
//! * `SignedMessageBody.unsigned` substitutes `Instant.now()` and salt `0`
//!   for the client's timestamp and salt. **The outgoing packet therefore
//!   carries the server's wall clock**, and no golden over these bytes can be
//!   stable across runs — see [`encode_player_chat`]'s note.
//!
//! # The clientbound packet is `player_chat`, not `disguised_chat`
//!
//! The intuitive reading — "unsigned messages go out as disguised chat" — is
//! wrong. `OutgoingChatMessage.create` branches on `PlayerChatMessage.isSystem()`,
//! which compares the *sender* against `SYSTEM_SENDER`, and
//! `SignedMessageLink.unsigned(uuid)` is `root(uuid, NIL_UUID)` — the sender
//! is the player. So an offline server sends `player_chat` (65) with the
//! signature field absent. `disguised_chat` is for messages whose sender is
//! the system and is not ported.
//!
//! # What is NOT covered
//!
//! * **Signed chat, in every part.** See above.
//! * **`chat_ack` (6), `chat_session_update` (10), `chat_command_signed` (8),
//!   `delete_chat` (31).** The ids exist so the packets can be recognised and
//!   dropped deliberately rather than falling through to the movement
//!   decoder; none is handled.
//! * **Chat spam throttling.** `TickThrottler(20, 20 *
//!   getChatSpamThresholdSeconds())` and `detectChatRateSpam`.
//! * **`TextFilter`.** `DUMMY` on a dedicated server, so the filter mask is
//!   always `PASS_THROUGH` and `isFullyFiltered` is always false. Ported as
//!   the constant it is, not as a filter.
//! * **`ChatDecorator`.** `MinecraftServer.getChatDecorator()` returns
//!   `PLAIN`, whose `decorate` is `aload_1; areturn`. Ported as its identity.
//! * **Chat types other than `minecraft:chat`** — `/say`, `/msg`, team chat.
//!
//! # The plugin hook is NOT called here, and that is a fact about `main`
//!
//! The brief for this branch said the server-side plugin hook was live. **On
//! `main` at `fbf0ae6` it is not.** `crates/rustmc-server` declares
//! `rustmc-api` as a dependency and no file under `src/` mentions it; there is
//! no event dispatch in the server crate at all. The hook lives on branch
//! `plugin-hook` (`1423e72`), merged into `storage-dir` (`d3fb805`), neither
//! of which is an ancestor of this branch.
//!
//! That branch's API has also moved past the one `main`'s `rustmc-api`
//! publishes: it has a `Session::plugins` field, a `Host`, a `Ctx`, interned
//! state handles and `deny_reason`, where `main`'s `rustmc_api::Event` still
//! has the flat `PlayerChat { player, message }` variant. Writing a call
//! against either would be writing against an API this branch cannot compile,
//! so nothing is written.
//!
//! **What this branch delivers instead is the thing the hook needs and did not
//! have: a chat message that exists at all.** Before this, no amount of plugin
//! wiring could have produced a `PlayerChat` event, because the packet had no
//! id, no decoder and no call site.
//!
//! The integration point, so the rebase is a one-liner rather than a
//! rediscovery: in [`handle`], after `is_illegal` and **before** [`broadcast`]
//! — vanilla's own cancellation point is the same place, between
//! `tryHandleChat`'s validation and `broadcastChatMessage`. By analogy with
//! `plugins::block_break_denied` it wants the shape
//! `plugins::chat_denied(s, &incoming.message) -> Option<String>`, returning
//! early without broadcasting when a plugin cancels.

use crate::commandtree::Level;
use crate::world_send::Session;
use rustmc_proto::{ids, varint::write_varint, wire};
use rustmc_world::nbt::Nbt;
use std::io;

/// `readUtf(256)` — the message length cap `ServerboundChatPacket` reads with.
pub const MAX_MESSAGE_CHARS: usize = 256;

/// `LastSeenMessages$Update`'s fixed bitset width, in bits.
///
/// `readFixedBitSet(20)`, which is `Mth.positiveCeilDiv(20, 8)` = **3** bytes.
const LAST_SEEN_BITS: usize = 20;

/// The serverbound packet, decoded.
///
/// The signature is deliberately kept as "present or not" rather than as its
/// 256 bytes: nothing on this configuration can check it, and holding bytes
/// we will never verify would invite a later reader to think we do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingChat {
    pub message: String,
    /// Epoch millis, as the client sent them. **Discarded** on the way out —
    /// `SignedMessageBody.unsigned` substitutes the server's own clock — and
    /// kept here only so a plugin can see what the client claimed.
    pub timestamp: i64,
    pub salt: i64,
    /// Whether a `MessageSignature` was present. Always false from a client
    /// with no chat session.
    pub signed: bool,
}

/// Read a `ServerboundChatPacket` body.
///
/// The whole body is consumed, including the last-seen block, because a
/// partial read here is a desynchronised frame reader: the fields after the
/// message are not optional and the trailing `checksum` byte is the one most
/// easily forgotten.
///
/// # Errors
/// If the body is truncated or the string is not valid UTF-8.
pub fn read_chat(body: &[u8]) -> io::Result<IncomingChat> {
    let mut r = body;
    let message = wire::read_string(&mut r)?;
    let timestamp = wire::read_i64(&mut r)?;
    let salt = wire::read_i64(&mut r)?;
    let signed = wire::read_bool(&mut r)?;
    if signed {
        let mut sig = [0u8; 256];
        io::Read::read_exact(&mut r, &mut sig)?;
    }
    // `LastSeenMessages$Update`: varint offset, fixed bitset, i8 checksum.
    let _offset = rustmc_proto::varint::read_varint(&mut r)?;
    let mut bits = [0u8; LAST_SEEN_BITS.div_ceil(8)];
    io::Read::read_exact(&mut r, &mut bits)?;
    let _checksum = wire::read_u8(&mut r)?;
    Ok(IncomingChat {
        message,
        timestamp,
        salt,
        signed,
    })
}

/// `StringUtil.isAllowedChatCharacter`, and so `isChatMessageIllegal`.
///
/// Vanilla disconnects a player whose message contains a disallowed
/// character, before anything else happens
/// (`tryHandleChat` offsets 0..17). The rule is: not the section sign, and
/// not a control character below 0x20 or 0x7F.
#[must_use]
pub fn is_allowed_chat_character(c: char) -> bool {
    c != '\u{a7}' && c >= ' ' && c != '\u{7f}'
}

/// Whether vanilla would refuse this message outright.
#[must_use]
pub fn is_illegal(message: &str) -> bool {
    !message.chars().all(is_allowed_chat_character)
}

/// `ChatVisiblity`, which decides whether a recipient is sent chat at all.
///
/// `ServerPlayer.acceptsChatMessages()` is `chatVisibility == FULL` and
/// nothing else, so a recipient on `SYSTEM` or `HIDDEN` receives **no
/// packet** — not a filtered one. Ordinals are the enum's: FULL 0, SYSTEM 1,
/// HIDDEN 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Visibility {
    #[default]
    Full,
    System,
    Hidden,
}

impl Visibility {
    /// From the varint in `ServerboundClientInformationPacket`.
    ///
    /// An unknown value is `Full`, matching `ByteBufCodecs`' behaviour of
    /// refusing rather than silencing — we cannot refuse a whole login over
    /// it, and silencing a player's chat because of an unrecognised setting
    /// is the worse of the two failures.
    #[must_use]
    pub const fn from_ordinal(n: i32) -> Self {
        match n {
            1 => Self::System,
            2 => Self::Hidden,
            _ => Self::Full,
        }
    }

    /// `ServerPlayer.acceptsChatMessages`.
    #[must_use]
    pub fn accepts_chat(self) -> bool {
        self == Self::Full
    }
}

/// The `minecraft:chat_type` registry id of `minecraft:chat`.
///
/// Entry **0** of the registry in `data/vanilla/registries.bin`, and the
/// holder codec writes `id + 1` for a registry reference (`ByteBufCodecs$30`,
/// offsets 40..59), so the byte on the wire is 1. Written as the id plus the
/// codec's own arithmetic rather than as a literal 1, so that the two facts
/// stay separable.
const CHAT_TYPE_ID: i32 = 0;

/// The `minecraft:chat_type` registry, in the order the client receives it —
/// which **is not** `ChatType.bootstrap`'s registration order.
///
/// # This array was written in bootstrap order and the capture refused it
///
/// `ChatType.bootstrap` registers chat, say_command, msg_command_incoming,
/// msg_command_outgoing, team_msg_command_incoming, team_msg_command_outgoing,
/// emote_command, in that order, and that is the obvious thing to transcribe.
/// The `registry_data` packet a real client is sent carries them **sorted by
/// key**, so `say_command` is id 4 rather than 1 and `emote_command` is 1
/// rather than 6.
///
/// The difference is invisible everywhere except on the wire: the packet is
/// well-formed either way and renders as the wrong kind of message. Registered
/// order is what a reading of the jar gives you, sorted order is what the
/// client numbers against, and only the capture can tell you which one you
/// have. `minecraft:chat` happens to be first under both, which is why the
/// existing `player_chat` work never had to find this out.
///
/// Each entry's `chat` translation key is in the comment beside it, from
/// `bootstrap`, so a reader can see WHICH message a number is without opening
/// the jar. The keys are the client's business; the server sends only the id.
pub const CHAT_TYPES: [&str; 7] = [
    "minecraft:chat",                      // chat.type.text
    "minecraft:emote_command",             // chat.type.emote
    "minecraft:msg_command_incoming",      // commands.message.display.incoming
    "minecraft:msg_command_outgoing",      // commands.message.display.outgoing
    "minecraft:say_command",               // chat.type.announcement
    "minecraft:team_msg_command_incoming", // chat.type.team.text
    "minecraft:team_msg_command_outgoing", // chat.type.team.sent
];

/// The registry id of a chat type, or `None` for a name 26.2 does not
/// register.
#[must_use]
pub fn chat_type_id(name: &str) -> Option<i32> {
    CHAT_TYPES
        .iter()
        .position(|n| *n == name)
        .and_then(|i| i32::try_from(i).ok())
}

/// Build a `ClientboundDisguisedChatPacket` body.
///
/// # Why a command's chat is a different packet from a player's chat
///
/// `PlayerList.broadcastChatMessage` sends whatever
/// `OutgoingChatMessage.create(message)` returns, and that switches on
/// `PlayerChatMessage.isSystem()`. A command's message comes from
/// `MessageArgument.resolveChatMessage`, which on an offline server takes the
/// `resolveDisguisedMessage` branch — `getSigningContext().getArgument(name)`
/// is null, because nothing here ever sends `chat_command_signed` — and builds
/// `PlayerChatMessage.system(text)`. `system` is `unsigned(SYSTEM_SENDER, …)`,
/// so `isSystem()` is true, so the outgoing message is
/// `OutgoingChatMessage.Disguised` and the packet is `disguised_chat`.
///
/// That packet is far smaller than `player_chat`: no signature, no salt, no
/// last-seen block, no per-recipient index. Just the component and the bound
/// chat type.
///
/// `sender_display_name` is `ChatType.bind`'s second field —
/// `CommandSourceStack.getDisplayName()`, which for a player source is the
/// same component [`crate::commands::display_name_component`] builds for
/// `/list`.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_disguised_chat(
    component: &Nbt,
    chat_type: i32,
    sender_display_name: &Nbt,
    target_name: Option<&Nbt>,
) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    crate::commands::write_network_nbt(&mut o, component)?;
    // `ChatType$Bound.STREAM_CODEC`, the same three fields `player_chat` ends
    // with: the holder as `id + 1`, the name, and an optional target name.
    //
    // `ChatType.bind` leaves the target empty and only `withTargetName` fills
    // it — which `MsgCommand` calls for the SENDER's copy and not the
    // recipient's, so the same message produces two different bounds. That is
    // the whole reason this parameter exists.
    write_varint(&mut o, chat_type + 1)?;
    crate::commands::write_network_nbt(&mut o, sender_display_name)?;
    match target_name {
        None => wire::write_bool(&mut o, false)?,
        Some(t) => {
            wire::write_bool(&mut o, true)?;
            crate::commands::write_network_nbt(&mut o, t)?;
        }
    }
    Ok(o)
}

/// One `disguised_chat` to one connection.
///
/// `ServerPlayer.sendChatMessage` gates every outgoing chat message on
/// `acceptsChatMessages()`, disguised or not, so the visibility filter is here
/// rather than in the caller.
pub fn send_disguised(
    conn: &crate::players::Connection,
    component: &Nbt,
    chat_type: i32,
    sender_display_name: &Nbt,
    target_name: Option<&Nbt>,
) {
    if !conn.chat_visibility().accepts_chat() {
        return;
    }
    match encode_disguised_chat(component, chat_type, sender_display_name, target_name) {
        Ok(b) => drop(conn.send(ids::play::cb::DISGUISED_CHAT, &b)),
        Err(e) => eprintln!("disguised chat encode failed for {}: {e}", conn.name()),
    }
}

/// `PlayerList.broadcastChatMessage` for a disguised message: every player who
/// accepts chat, in list order.
///
/// **The `acceptsChatMessages` filter applies here too**, and it is easy to
/// assume it does not, because `sendDisguisedChatMessage` has no check of its
/// own. The gate is one level up, in `ServerPlayer.sendChatMessage`, which
/// wraps every `OutgoingChatMessage` — disguised or not. So a player on
/// `SYSTEM` or `HIDDEN` visibility sees no `/say`.
///
/// Unlike [`broadcast`] there is no per-recipient index, so every recipient
/// gets **byte-identical** bytes and the body is encoded once.
pub fn broadcast_disguised(
    players: &crate::players::Players,
    component: &Nbt,
    chat_type: i32,
    sender_display_name: &Nbt,
) {
    let body = match encode_disguised_chat(component, chat_type, sender_display_name, None) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("disguised chat encode failed: {e}");
            return;
        }
    };
    for c in players.connections() {
        if !c.chat_visibility().accepts_chat() {
            continue;
        }
        drop(c.send(ids::play::cb::DISGUISED_CHAT, &body));
    }
}

/// Build a `ClientboundPlayerChatPacket` body.
///
/// `global_index` is the **recipient's** `nextChatIndex++`, not the sender's:
/// `sendPlayerChatMessage` increments a counter on the connection it is
/// writing to, so two players who join at different times see different
/// indices for the same message.
///
/// `timestamp_millis` is the server's `Instant.now()` at broadcast time. It
/// is a parameter rather than read here so that a test can pin it — **the
/// packet is otherwise not reproducible**, which is a property of vanilla's
/// design and not of this port.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_player_chat(
    global_index: i32,
    sender_uuid: u128,
    message: &str,
    timestamp_millis: i64,
    sender_display_name: &Nbt,
) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    write_varint(&mut o, global_index)?;
    wire::write_u128(&mut o, sender_uuid)?;
    // `SignedMessageLink.root` is built with `iconst_0`, so an unsigned
    // message's index is always 0. Not the same field as `globalIndex`.
    write_varint(&mut o, 0)?;
    // `writeNullable(signature, ...)` — absent.
    wire::write_bool(&mut o, false)?;

    // SignedMessageBody$Packed.write
    wire::write_string(&mut o, message)?;
    wire::write_i64(&mut o, timestamp_millis)?;
    // `SignedMessageBody.unsigned` is built with `lconst_0`.
    wire::write_i64(&mut o, 0)?;
    // `LastSeenMessages.EMPTY.pack(...)` is an empty collection.
    write_varint(&mut o, 0)?;

    // `writeNullable(unsignedContent, TRUSTED_STREAM_CODEC)` — absent,
    // because `withUnsignedContent` stores null when the decorated component
    // equals `Component.literal(signedContent())`, and `ChatDecorator.PLAIN`
    // guarantees it does.
    wire::write_bool(&mut o, false)?;

    // `FilterMask.write`: the enum ordinal as a varint. `PASS_THROUGH` is 0,
    // and only `PARTIALLY_FILTERED` (2) is followed by a bitset.
    write_varint(&mut o, 0)?;

    // ChatType$Bound.STREAM_CODEC: holder, name, optional targetName.
    write_varint(&mut o, CHAT_TYPE_ID + 1)?;
    crate::commands::write_network_nbt(&mut o, sender_display_name)?;
    // `TRUSTED_OPTIONAL_STREAM_CODEC` — `ChatType.bind` leaves targetName
    // empty.
    wire::write_bool(&mut o, false)?;
    Ok(o)
}

/// Handle a `chat` packet: validate, then broadcast to everyone who accepts
/// chat.
///
/// Returns whether the message was broadcast. `false` means vanilla would
/// have refused it — an illegal character — and the caller should treat the
/// connection as one vanilla would have disconnected.
///
/// # Errors
/// If the packet cannot be read.
pub fn handle(s: &Session<'_>, body: &[u8]) -> io::Result<bool> {
    let incoming = read_chat(body)?;
    if is_illegal(&incoming.message) {
        // Vanilla disconnects with `multiplayer.disconnect.illegal_characters`.
        // We refuse the message and say so; the disconnect component is not
        // ported, for the same reason the command error is not.
        eprintln!(
            "chat from {} held an illegal character — refused",
            s.conn.name()
        );
        return Ok(false);
    }
    // THE PLUGIN HOOK GOES HERE. See the module header: `main` has no event
    // dispatch in this crate, so there is nothing to call yet. This is the
    // point at which vanilla could still cancel — after validation, before
    // any packet is sent — and it is where `plugins::chat_denied` belongs
    // when `plugin-hook` lands.
    //
    // `SignedMessageBody.unsigned` -> `Instant.now()`, taken once for the
    // whole broadcast so every recipient sees the same timestamp, which is
    // what a single `PlayerChatMessage` gives vanilla.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let display = crate::commands::display_name_component(s.conn.name());
    let uuid = wire::offline_uuid(s.conn.name());
    broadcast(s.players, uuid, &incoming.message, now, &display);
    Ok(true)
}

/// `PlayerList.broadcastChatMessage`: every player in list order, including
/// the speaker, each with its own `globalIndex`.
///
/// A send failure is dropped rather than propagated, matching
/// [`crate::players::Players::broadcast`]: one player's dead socket must not
/// stop the message reaching everyone else.
pub fn broadcast(
    players: &crate::players::Players,
    sender_uuid: u128,
    message: &str,
    timestamp_millis: i64,
    sender_display_name: &Nbt,
) {
    for c in players.connections() {
        if !c.chat_visibility().accepts_chat() {
            continue;
        }
        let index = c.next_chat_index();
        match encode_player_chat(
            index,
            sender_uuid,
            message,
            timestamp_millis,
            sender_display_name,
        ) {
            Ok(b) => drop(c.send(ids::play::cb::PLAYER_CHAT, &b)),
            Err(e) => eprintln!("chat encode failed for {}: {e}", c.name()),
        }
    }
}

/// The permission level a chat message is subject to.
///
/// **There is none, and this returns so.** 26.2 declares
/// `Permissions.CHAT_SEND_MESSAGES` and three siblings, and a sweep of all
/// 7,434 `net/minecraft` classes in the pinned jar on 2026-08-06 — with a
/// positive control on `COMMANDS_ADMIN`, which hits `Player` and `Commands` —
/// found **no class outside `Permissions` itself referencing any of the
/// four**. They are declared and unused. This function exists so that the
/// finding is in the code rather than only in a doc, and so that the day one
/// of them is wired up there is a single place to change.
#[must_use]
pub const fn required_level_to_chat() -> Level {
    Level::All
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// A full serverbound body: the five fields, with the last-seen block's
    /// trailing checksum byte that is the easiest one to drop.
    fn serverbound(message: &str, signed: bool) -> Vec<u8> {
        let mut b = Vec::new();
        wire::write_string(&mut b, message).expect("buf");
        wire::write_i64(&mut b, 1_700_000_000_000).expect("buf");
        wire::write_i64(&mut b, 0x0123_4567_89ab_cdef).expect("buf");
        wire::write_bool(&mut b, signed).expect("buf");
        if signed {
            b.extend_from_slice(&[0xAB; 256]);
        }
        write_varint(&mut b, 0).expect("buf"); // offset
        b.extend_from_slice(&[0, 0, 0]); // readFixedBitSet(20) -> 3 bytes
        wire::write_u8(&mut b, 0).expect("buf"); // checksum
        b
    }

    /// The serverbound packet round-trips, signed or not, and **consumes the
    /// whole body** either way. The length assertion is the point: a reader
    /// that stopped after the salt would pass a field test and desynchronise
    /// the frame stream.
    #[test]
    fn the_serverbound_packet_is_read_whole() {
        for signed in [false, true] {
            let body = serverbound("hello", signed);
            let got = read_chat(&body).expect("read");
            assert_eq!(got.message, "hello");
            assert_eq!(got.timestamp, 1_700_000_000_000);
            assert_eq!(got.salt, 0x0123_4567_89ab_cdef);
            assert_eq!(got.signed, signed);
            // Exactly the bytes the layout predicts: 1+5 string, 8, 8, 1,
            // [256], 1 varint, 3 bitset, 1 checksum.
            let want = 6 + 8 + 8 + 1 + usize::from(signed) * 256 + 1 + 3 + 1;
            assert_eq!(body.len(), want, "signed={signed}");
        }
    }

    /// A truncated body is an error, not a silently short read. The
    /// `checksum` byte specifically: drop it and the packet still parses
    /// every named field.
    #[test]
    fn a_body_missing_its_checksum_byte_is_refused() {
        let mut body = serverbound("hi", false);
        body.pop();
        assert!(
            read_chat(&body).is_err(),
            "the trailing checksum is not optional"
        );
    }

    /// The 20-bit fixed bitset is three bytes, not two and not four.
    #[test]
    fn the_last_seen_bitset_is_three_bytes() {
        assert_eq!(LAST_SEEN_BITS.div_ceil(8), 3);
    }

    /// `isChatMessageIllegal`: the section sign and control characters.
    #[test]
    fn illegal_characters_are_the_section_sign_and_controls() {
        assert!(!is_illegal("hello world"));
        assert!(!is_illegal("héllo ☃"));
        assert!(is_illegal("colour \u{a7}c"));
        assert!(is_illegal("new\nline"));
        assert!(is_illegal("tab\there"));
        assert!(is_illegal("del\u{7f}"));
    }

    /// Only `FULL` receives chat, and the ordinals are the enum's.
    #[test]
    fn only_full_visibility_receives_chat() {
        assert_eq!(Visibility::from_ordinal(0), Visibility::Full);
        assert_eq!(Visibility::from_ordinal(1), Visibility::System);
        assert_eq!(Visibility::from_ordinal(2), Visibility::Hidden);
        assert!(Visibility::Full.accepts_chat());
        assert!(!Visibility::System.accepts_chat());
        assert!(!Visibility::Hidden.accepts_chat());
        // An unrecognised value must not silence a player.
        assert!(Visibility::from_ordinal(7).accepts_chat());
    }

    /// **The clientbound packet, field by field**, against the layout read
    /// from `ClientboundPlayerChatPacket.write`.
    ///
    /// The display name is a trivial component here rather than the real
    /// styled one so that the assertion is about *this* packet's framing;
    /// the styled component's own bytes are gated in `crate::commands`
    /// against 539 bytes of real vanilla wire.
    #[test]
    fn the_clientbound_packet_has_vanillas_layout() {
        let name = Nbt::String("oracle".to_string());
        let body = encode_player_chat(
            3,
            wire::offline_uuid("oracle"),
            "hi",
            0x0000_0100_0000_0000,
            &name,
        )
        .expect("encode");
        let got = hex(&body);
        let want = concat!(
            "03",                               // globalIndex 3
            "501b21fe0ed23c3c9399e5f69a092dcb", // sender uuid, 16 bytes
            "00",                               // index 0
            "00",                               // signature absent
            "026869",                           // "hi"
            "0000010000000000",                 // timestamp
            "0000000000000000",                 // salt 0
            "00",                               // lastSeen: 0 entries
            "00",                               // unsignedContent absent
            "00",                               // filterMask PASS_THROUGH
            "01",                               // chat type holder: id 0 + 1
            "0800066f7261636c65",               // name: TAG_String, len 0x0006, "oracle"
            "00",                               // targetName absent
        );
        assert_eq!(got, want);
    }

    /// The chat-type holder is `id + 1`, so it can never be the `0` that
    /// means "an inline value follows". A port that wrote the bare id would
    /// make the client read the next component as a `ChatType`.
    #[test]
    fn the_chat_type_holder_is_one_more_than_the_id() {
        assert_eq!(CHAT_TYPE_ID, 0, "minecraft:chat is entry 0");
        let body = encode_player_chat(0, 0, "x", 0, &Nbt::String(String::new())).expect("encode");
        // globalIndex(1) + uuid(16) + index(1) + sig(1) + "x"(2) + ts(8)
        // + salt(8) + lastSeen(1) + unsigned(1) + mask(1) = 40
        assert_eq!(body[40], 1, "holder byte is id + 1, never 0");
    }

    /// Two recipients get two different `globalIndex` values from their own
    /// counters, not from the sender's — the property that makes this field
    /// per-connection.
    #[test]
    fn the_global_index_is_per_recipient() {
        let a = encode_player_chat(0, 0, "m", 0, &Nbt::String(String::new())).expect("a");
        let b = encode_player_chat(1, 0, "m", 0, &Nbt::String(String::new())).expect("b");
        assert_ne!(a, b);
        assert_eq!(a[0], 0);
        assert_eq!(b[0], 1);
    }

    /// **The chat-type registry, against the capture the client is sent.**
    ///
    /// [`CHAT_TYPES`] is a constant because it cannot change within a pinned
    /// version, and it is asserted against `data/vanilla/registries.bin`
    /// because a registry id is the one number in a chat packet that nothing
    /// else in the packet constrains. A wrong id is a well-formed packet that
    /// renders as the wrong kind of message, and no other test here would see
    /// it.
    ///
    /// Both directions: every name in the constant is at that index in the
    /// capture, and the capture holds no eighth entry.
    #[test]
    fn the_chat_type_registry_is_the_one_the_client_receives() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("data/vanilla/registries.bin");
        let caps = crate::configuration::CapturedRegistries::load(&p)
            .unwrap_or_else(|e| panic!("{}: {e} — run ci/dump-registries.sh", p.display()));
        let ids = caps
            .entry_ids("minecraft:chat_type")
            .expect("the capture parses")
            .expect("the capture holds minecraft:chat_type");
        assert_eq!(
            ids,
            CHAT_TYPES.to_vec(),
            "the capture and CHAT_TYPES disagree about the chat-type registry"
        );
        assert_eq!(chat_type_id("minecraft:chat"), Some(CHAT_TYPE_ID));
        // The two this branch sends, and both are where the CAPTURE puts
        // them rather than where `ChatType.bootstrap` does — 4 and 1, not 1
        // and 6. See [`CHAT_TYPES`].
        assert_eq!(chat_type_id("minecraft:say_command"), Some(4));
        assert_eq!(chat_type_id("minecraft:emote_command"), Some(1));
        // And the registry is key-sorted, which is where the difference comes
        // from. Asserted so that a capture which stopped being sorted is a red
        // here rather than a wrong id somewhere else.
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "the capture is no longer key-sorted");
        assert_eq!(chat_type_id("minecraft:not_a_chat_type"), None);
    }

    /// A `disguised_chat` body, field by field.
    ///
    /// The shape is the whole assertion: a component, then `ChatType$Bound`'s
    /// three fields with the holder as `id + 1`. There is no signature, no
    /// salt, no last-seen block and **no per-recipient index** — which is why
    /// [`broadcast_disguised`] encodes once and sends the same bytes to
    /// everyone, and `player_chat` cannot.
    #[test]
    fn the_disguised_chat_body_is_a_component_and_a_bound_chat_type() {
        let msg = Nbt::String("hello".into());
        let who = Nbt::String("oracle".into());
        let body = encode_disguised_chat(&msg, 1, &who, None).expect("encode");
        let want = concat!(
            "080005 68656c6c6f",   // component: TAG_String, len 5, "hello"
            "02",                  // chat type holder: say_command id 1, + 1
            "080006 6f7261636c65", // name: TAG_String, len 6, "oracle"
            "00",                  // targetName absent
        );
        let want: String = want.split_whitespace().collect();
        assert_eq!(hex(&body), want);
        // The holder byte can never be 0, which is the value that means "an
        // inline ChatType follows" — the same property `player_chat` has a
        // test for.
        assert_ne!(body[7], 0);

        // And with a target name, which is what `/msg` sends to the SENDER.
        // The optional flips from `00` to `01` and the component follows.
        let to = Nbt::String("bob".into());
        let with_target = encode_disguised_chat(&msg, 3, &who, Some(&to)).expect("encode");
        let want_target = concat!(
            "080005 68656c6c6f",
            "04",                  // msg_command_outgoing id 3, + 1
            "080006 6f7261636c65", // name
            "01",                  // targetName present
            "080003 626f62",       // "bob"
        );
        let want_target: String = want_target.split_whitespace().collect();
        assert_eq!(hex(&with_target), want_target);
    }

    /// The message cap vanilla reads with.
    #[test]
    fn the_message_cap_is_256() {
        assert_eq!(MAX_MESSAGE_CHARS, 256);
    }

    /// Chat is not permission-gated in 26.2. Pinned so that if someone wires
    /// `CHAT_SEND_MESSAGES` up later, this test is what tells them the sweep
    /// that justified the constant needs re-running.
    #[test]
    fn chat_requires_no_permission() {
        assert_eq!(required_level_to_chat(), Level::All);
    }
}
