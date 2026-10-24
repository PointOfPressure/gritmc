//! `SignBlockEntity` — the two texts a sign carries, and the one edit it
//! accepts per opening.
//!
//! # Why a sign is not a container with words in it
//!
//! Every other block entity this server models is a [`crate::item`] store,
//! and the whole of [`crate::blockentity`] was written around that. A sign
//! keeps no items at all. What it keeps is two [`SignText`]s — the front and
//! the back, which 26.2 signs both have — a waxed flag, and a **transient**
//! permission token naming the one player who is currently allowed to type
//! into it.
//!
//! That token is the part worth reading twice, because it is the difference
//! between a faithful implementation and the obvious wrong one.
//!
//! # The token, and the packet it refuses
//!
//! `SignBlockEntity.updateSignText(player, isFront, lines)` opens with a
//! three-part guard at offsets 0-42 and the whole of its failure path is one
//! `LOGGER.warn` — no packet, no error, nothing on the wire:
//!
//! ```text
//!    0: isWaxed()                  ifne  -> warn
//!    7: player.getUUID()
//!   12: getPlayerWhoMayEdit()
//!   15: equals                     ifeq  -> warn
//!   21: level                      ifnull -> warn
//!   43: updateText(lambda, isFront)
//!   57: setAllowedPlayerEditor(null)
//!   62: level.sendBlockUpdated(pos, state, state, 3)
//! ```
//!
//! **`setAllowedPlayerEditor(null)` runs at 57, before the broadcast at 62.**
//! The token is spent by the write that used it, so a second `sign_update` at
//! the same position — from the same player, in the same tick — fails the
//! `equals` at offset 18 and produces nothing. Vanilla answers two identical
//! sign edits with **one** `block_entity_data`, and
//! `571-a-second-edit-of-the-same-sign-is-ignored` exists to catch the
//! implementation that answers both.
//!
//! # Who sets the token, and the brief that was wrong about it
//!
//! Every account this port was handed said `SignBlock.setPlacedBy` ->
//! `openTextEdit`. **`SignBlock` has no `setPlacedBy`.** Binary-grepping the
//! pinned jar's constant pools for `openTextEdit` returns four classes, and
//! reading them gives two callers rather than one:
//!
//! * `SignItem.updateCustomBlockEntityTag` offsets 13-84 — the placement
//!   route. `BlockItem.place` calls it at offset 143, **before**
//!   `setPlacedBy` at 171, and it opens the editor on `iconst_1`, the
//!   **front**, unconditionally. It is also gated on its own super call
//!   returning false at offset 20: a sign item carrying saved text writes
//!   that text and does *not* open an editor;
//! * `SignBlock.useWithoutItem` offset 164 — right-clicking a sign that is
//!   already there, behind `otherPlayerIsEditingSign`, `mayBuild` and
//!   `hasEditableText`. Not ported: nothing in the corpus right-clicks a
//!   sign, and `use_without_item.json` already records that the click writes
//!   no state.
//!
//! The behaviour the old account described is right — placing a sign opens
//! its editor and hands the placer the token — but it is worth correcting in
//! place, because a reader looking for `setPlacedBy` in `SignBlock` finds
//! nothing and has no way to tell whether the mechanism is missing or the
//! name is.
//!
//! # The update tag is byte-compared, so its shape is not a matter of taste
//!
//! `SignBlockEntity.getUpdatePacket()` is
//! `ClientboundBlockEntityDataPacket.create(this)`, whose tag comes from
//! `getUpdateTag` -> `saveCustomOnly` -> `saveAdditional` **only** —
//! `BlockEntity.saveAdditional` is a bare `return` and `saveMetadata` is not
//! called, so the packet's tag carries no `id`, `x`, `y` or `z`. What it does
//! carry is three keys, and each one was read out of the codec rather than
//! recalled:
//!
//! ```text
//!   front_text  SignText.DIRECT_CODEC
//!   back_text   SignText.DIRECT_CODEC
//!   is_waxed    ValueOutput.putBoolean  -> TAG_Byte
//! ```
//!
//! and `SignText.DIRECT_CODEC`'s own group, in `lambda$static$3`:
//!
//! ```text
//!   messages           LINES_CODEC.fieldOf              required, 4 entries
//!   filtered_messages  LINES_CODEC.lenientOptionalFieldOf   see below
//!   color              optionalAlwaysPresentFieldOf(BLACK)  always written
//!   has_glowing_text   optionalAlwaysPresentFieldOf(false)  always written
//! ```
//!
//! **`filtered_messages` is absent from everything this server writes, and
//! that is a derivation rather than a simplification.** `SignText.
//! filteredMessages()` returns `Optional.empty()` unless some filtered line
//! differs from its plain one, and `setMessages`' non-filtering branch at
//! offsets 74-106 writes `literal(raw)` and `literal(filteredOrEmpty())` with
//! the same style — equal components — for a server with no chat filter. An
//! `optionalFieldOf` given an empty `Optional` writes no key at all.
//!
//! Each message is a `Component` through `ComponentSerialization.CODEC`,
//! which encodes via `tryCollapseToString()`: a `PlainTextContents` with no
//! siblings and an empty style becomes a bare `TAG_String`. Every line this
//! server can produce is exactly that, so `messages` is a `TAG_List` of four
//! strings and never a list of compounds.
//!
//! # And the key order is part of the bytes
//!
//! `CompoundTag` wraps a `java.util.HashMap` and `write` iterates
//! `keySet()`, so the wire order of `front_text`/`back_text`/`is_waxed` is
//! bucket order and not alphabetical. That is already handled — see
//! `rustmc_world::nbt`'s `java_key_order` — and it is named here because it
//! is the reason a "tidier" writer would fail
//! `570-typing-on-a-sign-you-placed-sets-its-text` with an NBT payload that
//! decodes perfectly.

use crate::interact::BlockPos;
use rustmc_world::nbt::Nbt;
use std::collections::BTreeMap;
use std::io;

/// A decoded `ServerboundSignUpdatePacket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignUpdate {
    pub pos: BlockPos,
    /// `isFrontText`. Passed straight through to `updateSignText` and never
    /// re-derived from where the player is standing — `isFacingFrontText` is
    /// what the *right-click* path uses, and this is not that path.
    pub front: bool,
    pub lines: [String; LINES],
}

/// Decode `sign_update`, packet 61.
///
/// ```text
///   long                  the packed BlockPos
///   bool                  isFrontText
///   string(384) x 4       the lines, unconditionally
/// ```
///
/// **Four strings and no count.** The decoder is `new String[4]` filled by a
/// loop that consults no length field, so a packet carrying three lines is
/// not a short sign — it is a stream that has lost synchronisation, and the
/// `read_exact` below is what turns that into an error rather than into a
/// line made of the next packet's header.
///
/// # Errors
/// If the body is short or a line is not UTF-8.
pub fn decode_sign_update(body: &[u8]) -> io::Result<SignUpdate> {
    let mut cur = io::Cursor::new(body);
    let pos = crate::interact::unpack_pos(rustmc_proto::wire::read_i64(&mut cur)?);
    let front = rustmc_proto::wire::read_bool(&mut cur)?;
    let mut lines: [String; LINES] = std::array::from_fn(|_| String::new());
    for line in &mut lines {
        *line = rustmc_proto::wire::read_string(&mut cur)?;
    }
    Ok(SignUpdate { pos, front, lines })
}

/// How many lines a sign has. `SignText.LINES`, `iconst_4` in `loadLines`
/// and in `emptyMessages`.
pub const LINES: usize = 4;

/// The `minecraft:block_entity_type` name a sign registers under. **Not the
/// block's name**: 24 different blocks share this one type — every
/// `<wood>_sign` and `<wood>_wall_sign` — and the hanging signs do not, they
/// are `minecraft:hanging_sign`.
pub const TYPE_ID: &str = "minecraft:sign";

/// One face of a sign.
///
/// `color` and `has_glowing_text` are carried rather than assumed constant
/// even though nothing in this server can change them yet: both are written
/// unconditionally by `optionalAlwaysPresentFieldOf`, so a sign loaded from a
/// world where somebody dyed it must save back the colour it had. Dropping
/// the fields would round-trip through this server perfectly and blacken
/// every dyed sign in a vanilla world on first save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignText {
    messages: [String; LINES],
    color: String,
    has_glowing_text: bool,
}

impl Default for SignText {
    /// `SignText()`, the no-argument constructor: four `CommonComponents.
    /// EMPTY` messages, `DyeColor.BLACK`, `iconst_0` for the glow.
    fn default() -> Self {
        Self {
            messages: std::array::from_fn(|_| String::new()),
            color: "black".to_string(),
            has_glowing_text: false,
        }
    }
}

impl SignText {
    /// The four lines, in order.
    #[must_use]
    pub fn messages(&self) -> &[String; LINES] {
        &self.messages
    }

    /// `SignText.setMessage(i, component)` for every line the packet carried.
    ///
    /// **The loop is over the packet's list, not over `LINES`.**
    /// `SignBlockEntity.setMessages` runs `i < list.size()` at offsets 3-11,
    /// so a shorter list leaves the remaining lines alone rather than
    /// clearing them. `ServerboundSignUpdatePacket` always decodes exactly
    /// four, so the two agree today — this follows the method rather than the
    /// packet so that it still agrees if they ever stop agreeing.
    fn with_messages(&self, lines: &[String]) -> Self {
        let mut next = self.clone();
        for (i, line) in lines.iter().enumerate().take(LINES) {
            next.messages[i] = line.clone();
        }
        next
    }

    /// The `SignText.DIRECT_CODEC` compound.
    fn to_nbt(&self) -> Nbt {
        let mut c = BTreeMap::new();
        c.insert(
            "messages".to_string(),
            Nbt::List(
                self.messages
                    .iter()
                    .map(|m| Nbt::String(m.clone()))
                    .collect(),
            ),
        );
        c.insert("color".to_string(), Nbt::String(self.color.clone()));
        c.insert(
            "has_glowing_text".to_string(),
            Nbt::Byte(i8::from(self.has_glowing_text)),
        );
        Nbt::Compound(c)
    }

    /// Rebuild from a saved compound.
    ///
    /// Every field falls back to the codec's own default rather than making
    /// the parse fail: `color` and `has_glowing_text` are
    /// `optionalAlwaysPresentFieldOf`, so their defaults are `BLACK` and
    /// `false`, and a missing `messages` is an empty sign rather than no
    /// sign. A line that is a compound rather than a string is a styled
    /// component this server cannot model; it reads as empty, which loses the
    /// text and keeps the sign, and is written down here because it is a
    /// real gap rather than an impossible one.
    fn from_nbt(nbt: &Nbt) -> Self {
        let mut out = Self::default();
        let Some(c) = nbt.as_compound() else {
            return out;
        };
        if let Some(list) = c.get("messages").and_then(Nbt::as_list) {
            for (i, entry) in list.iter().enumerate().take(LINES) {
                out.messages[i] = entry.as_str().unwrap_or_default().to_string();
            }
        }
        if let Some(color) = c.get("color").and_then(Nbt::as_str) {
            out.color = color.to_string();
        }
        out.has_glowing_text = matches!(c.get("has_glowing_text"), Some(Nbt::Byte(b)) if *b != 0);
        out
    }
}

/// A sign's whole block-entity state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sign {
    front: SignText,
    back: SignText,
    is_waxed: bool,
    /// `playerWhoMayEdit`. **Not saved** — it is absent from
    /// `saveAdditional`, so a sign reloaded from disk is editable by nobody
    /// until somebody places or right-clicks it again.
    ///
    /// A `u128` rather than a `Uuid` type because that is what
    /// `rustmc_proto::wire::offline_uuid` produces and what the login writes
    /// on the wire; the only operation vanilla performs on it is `equals`.
    editor: Option<u128>,
}

/// What [`Sign::update_sign_text`] decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// The text was written and the token spent. The caller must run
    /// `sendBlockUpdated(pos, state, state, 3)`.
    Accepted,
    /// One of the three guards refused. Vanilla's entire effect here is a
    /// `LOGGER.warn`, so the caller sends nothing at all — which is the whole
    /// of what `571` measures.
    Refused,
}

impl Sign {
    /// `SignBlock.openTextEdit`'s first line —
    /// `setAllowedPlayerEditor(player.getUUID())`.
    ///
    /// The packets that follow it are the caller's, because they go to one
    /// connection rather than to the world; see
    /// [`crate::world_send::open_sign_editor`].
    pub fn set_allowed_player_editor(&mut self, uuid: Option<u128>) {
        self.editor = uuid;
    }

    /// `getPlayerWhoMayEdit()`.
    #[must_use]
    pub const fn player_who_may_edit(&self) -> Option<u128> {
        self.editor
    }

    #[must_use]
    pub const fn is_waxed(&self) -> bool {
        self.is_waxed
    }

    /// `getText(isFront)`.
    #[must_use]
    pub fn text(&self, front: bool) -> &SignText {
        if front {
            &self.front
        } else {
            &self.back
        }
    }

    /// `SignBlockEntity.updateSignText`, offsets 0-82, entire.
    ///
    /// The `level != null` third of the guard is not represented: this server
    /// has no route to a sign block entity that is not in a world, so a
    /// parameter for it would be a branch no test could reach. Said out loud
    /// rather than left as a silently missing condition.
    pub fn update_sign_text(&mut self, by: u128, front: bool, lines: &[String]) -> Edit {
        // Offsets 0-18. `equals` against a null `playerWhoMayEdit` is false,
        // which is why the `None` case refuses rather than accepting anyone.
        if self.is_waxed || self.editor != Some(by) {
            return Edit::Refused;
        }
        // Offsets 43-56: `updateText(op, isFront)` -> `setText(op.apply(
        // getText(isFront)), isFront)`. `setMessage` allocates a new
        // `SignText` per line, so `setFrontText`'s `if_acmpeq` at offset 5
        // never takes the equal branch and an edit that changes nothing still
        // counts as a change. That matters: it is why re-typing the same
        // words still broadcasts.
        let next = self.text(front).with_messages(lines);
        if front {
            self.front = next;
        } else {
            self.back = next;
        }
        // Offsets 57-61, **before** the broadcast at 62.
        self.editor = None;
        Edit::Accepted
    }

    /// `getUpdateTag` -> `saveCustomOnly` -> `saveAdditional`: the three keys
    /// `ClientboundBlockEntityDataPacket` carries, and no metadata.
    #[must_use]
    pub fn update_tag(&self) -> Nbt {
        let mut c = BTreeMap::new();
        c.insert("front_text".to_string(), self.front.to_nbt());
        c.insert("back_text".to_string(), self.back.to_nbt());
        c.insert("is_waxed".to_string(), Nbt::Byte(i8::from(self.is_waxed)));
        Nbt::Compound(c)
    }

    /// The same three keys plus `saveMetadata`'s `id`/`x`/`y`/`z`, which is
    /// what a chunk on disk carries. `BlockEntity.saveWithFullMetadata` is
    /// `saveMetadata` after `saveCustomOnly`, and the metadata is exactly
    /// what the update packet leaves out.
    #[must_use]
    pub fn to_nbt(&self, pos: BlockPos) -> Nbt {
        let mut c = match self.update_tag() {
            Nbt::Compound(c) => c,
            _ => BTreeMap::new(),
        };
        c.insert("id".to_string(), Nbt::String(TYPE_ID.to_string()));
        c.insert("x".to_string(), Nbt::Int(pos.0));
        c.insert("y".to_string(), Nbt::Int(pos.1));
        c.insert("z".to_string(), Nbt::Int(pos.2));
        Nbt::Compound(c)
    }

    /// `loadAdditional`, offsets 0-88. A missing side is a fresh `SignText`,
    /// which is `Optional.orElseGet(SignText::new)` at offsets 26-34.
    #[must_use]
    pub fn from_nbt(nbt: &Nbt) -> Self {
        let front = nbt
            .get("front_text")
            .map(SignText::from_nbt)
            .unwrap_or_default();
        let back = nbt
            .get("back_text")
            .map(SignText::from_nbt)
            .unwrap_or_default();
        Self {
            front,
            back,
            is_waxed: matches!(nbt.get("is_waxed"), Some(Nbt::Byte(b)) if *b != 0),
            // Deliberately not read: `playerWhoMayEdit` is not written by
            // `saveAdditional`, so a save that carried one came from
            // somewhere else and honouring it would let a restart hand out an
            // edit token nobody asked for.
            editor: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: u128 = 0x1111_2222_3333_4444_5555_6666_7777_8888;
    const BOB: u128 = 0x9999_aaaa_bbbb_cccc_dddd_eeee_ffff_0000;

    fn lines(first: &str) -> Vec<String> {
        vec![
            first.to_string(),
            String::new(),
            String::new(),
            String::new(),
        ]
    }

    /// The happy path, and the token going with it. **The second assertion is
    /// the one that matters**: an implementation that wrote the text and left
    /// the token alone passes every other test in this module.
    #[test]
    fn an_edit_from_the_permitted_player_lands_and_spends_the_token() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("hello")),
            Edit::Accepted
        );
        assert_eq!(sign.text(true).messages()[0], "hello");
        assert_eq!(
            sign.player_who_may_edit(),
            None,
            "setAllowedPlayerEditor(null) runs at offset 57, before the broadcast"
        );
    }

    /// `571`'s mechanism, in a unit test: the second edit is refused and
    /// leaves the first one standing.
    #[test]
    fn a_second_edit_of_the_same_sign_is_refused_and_changes_nothing() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("first")),
            Edit::Accepted
        );
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("second")),
            Edit::Refused
        );
        assert_eq!(
            sign.text(true).messages()[0],
            "first",
            "the refused edit still wrote its text"
        );
    }

    /// The `equals` at offset 15 is against the *editor*, not against "some
    /// player". Two different players, so a check that only ever used one
    /// could not tell the difference.
    #[test]
    fn an_edit_from_a_player_who_is_not_the_editor_is_refused() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(BOB, true, &lines("nope")),
            Edit::Refused
        );
        assert_eq!(sign.text(true).messages()[0], "");
        assert_eq!(
            sign.player_who_may_edit(),
            Some(ALICE),
            "a refusal must not spend somebody else's token"
        );
    }

    /// A sign nobody has opened refuses everyone. `uuid.equals(null)` is
    /// false, so this is the same branch as the wrong-player one and not a
    /// separate guard — but an implementation using `Option::is_none_or`
    /// would let the *first* packet through from anyone.
    #[test]
    fn a_sign_with_no_editor_refuses_every_player() {
        let mut sign = Sign::default();
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("nope")),
            Edit::Refused
        );
        assert_eq!(sign.text(true).messages()[0], "");
    }

    /// The waxed branch, which is the first of the three and short-circuits
    /// before the token is even read.
    #[test]
    fn a_waxed_sign_refuses_the_player_who_may_edit_it() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        sign.is_waxed = true;
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("nope")),
            Edit::Refused
        );
        assert_eq!(
            sign.player_who_may_edit(),
            Some(ALICE),
            "a waxed refusal does not spend the token either"
        );
    }

    /// `572`'s mechanism. The boolean comes off the packet and is passed
    /// straight through; the permission test above it does **not** consult
    /// the face, so a token opened for the front authorises the back.
    #[test]
    fn the_front_flag_decides_which_side_is_written_and_not_the_permission() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(ALICE, false, &lines("behind")),
            Edit::Accepted
        );
        assert_eq!(sign.text(false).messages()[0], "behind");
        assert_eq!(
            sign.text(true).messages()[0],
            "",
            "the front was written by a packet that named the back"
        );
    }

    /// Re-typing the same words still counts as a change, because
    /// `setMessage` allocates and `setFrontText`'s comparison is reference
    /// equality. An implementation that compared the text and skipped the
    /// broadcast would be silent where vanilla speaks.
    #[test]
    fn writing_identical_text_is_still_an_accepted_edit() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("same")),
            Edit::Accepted
        );
        sign.set_allowed_player_editor(Some(ALICE));
        assert_eq!(
            sign.update_sign_text(ALICE, true, &lines("same")),
            Edit::Accepted
        );
    }

    /// A list shorter than four lines leaves the rest alone — the loop is
    /// `i < list.size()`, not `i < LINES`.
    #[test]
    fn a_short_line_list_leaves_the_untouched_lines_alone() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        sign.update_sign_text(ALICE, true, &lines("full"));
        sign.front.messages[3] = "keep".to_string();
        sign.set_allowed_player_editor(Some(ALICE));
        sign.update_sign_text(ALICE, true, &["one".to_string()]);
        assert_eq!(sign.text(true).messages()[0], "one");
        assert_eq!(sign.text(true).messages()[3], "keep");
    }

    /// **The update tag's key set, which is byte-compared against vanilla.**
    /// Three keys and no metadata: an `id`, `x`, `y` or `z` here would be
    /// four extra tags in a packet body that a check digests whole.
    #[test]
    fn the_update_tag_carries_three_keys_and_no_position() {
        let sign = Sign::default();
        let Nbt::Compound(c) = sign.update_tag() else {
            panic!("compound")
        };
        assert_eq!(c.len(), 3);
        assert!(c.contains_key("front_text"));
        assert!(c.contains_key("back_text"));
        assert!(matches!(c.get("is_waxed"), Some(Nbt::Byte(0))));
        for absent in ["id", "x", "y", "z", "Items"] {
            assert!(
                !c.contains_key(absent),
                "`{absent}` belongs to saveMetadata, which getUpdateTag does not call"
            );
        }
    }

    /// The `SignText` compound: `messages` as four bare strings,
    /// `color` and `has_glowing_text` written even at their defaults, and
    /// **no `filtered_messages`**.
    #[test]
    fn a_sign_text_writes_four_bare_strings_and_never_filtered_messages() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        sign.update_sign_text(ALICE, true, &lines("hello"));
        let tag = sign.update_tag();
        let front = tag.get("front_text").expect("front_text");
        let Nbt::Compound(c) = front else {
            panic!("compound")
        };
        assert_eq!(
            c.len(),
            3,
            "messages, color, has_glowing_text — and no more"
        );
        assert!(
            !c.contains_key("filtered_messages"),
            "filteredMessages() is Optional.empty when no line was filtered, \
             and an empty optionalFieldOf writes no key"
        );
        let msgs = c.get("messages").and_then(Nbt::as_list).expect("messages");
        assert_eq!(msgs.len(), LINES);
        assert_eq!(msgs[0].as_str(), Some("hello"));
        assert_eq!(msgs[1].as_str(), Some(""));
        assert_eq!(
            msgs[0].tag(),
            8,
            "a plain literal collapses to TAG_String, not to a component compound"
        );
        assert_eq!(c.get("color").and_then(Nbt::as_str), Some("black"));
        assert!(matches!(c.get("has_glowing_text"), Some(Nbt::Byte(0))));
    }

    /// The disk form is the update tag **plus** metadata, and both texts
    /// survive the trip. A dyed sign keeps its colour, which is the field a
    /// writer that hardcoded black would lose.
    #[test]
    fn the_disk_form_round_trips_both_faces_and_the_colour() {
        let mut sign = Sign::default();
        sign.set_allowed_player_editor(Some(ALICE));
        sign.update_sign_text(ALICE, true, &lines("front"));
        sign.set_allowed_player_editor(Some(ALICE));
        sign.update_sign_text(ALICE, false, &lines("back"));
        sign.back.color = "lime".to_string();
        sign.back.has_glowing_text = true;
        sign.is_waxed = true;

        let nbt = sign.to_nbt((3, 64, -7));
        assert_eq!(nbt.get("id").and_then(Nbt::as_str), Some("minecraft:sign"));
        assert_eq!(nbt.get("x").and_then(Nbt::as_i32), Some(3));
        assert_eq!(nbt.get("y").and_then(Nbt::as_i32), Some(64));
        assert_eq!(nbt.get("z").and_then(Nbt::as_i32), Some(-7));

        let back = Sign::from_nbt(&nbt);
        assert_eq!(back.text(true).messages()[0], "front");
        assert_eq!(back.text(false).messages()[0], "back");
        assert_eq!(back.text(false).color, "lime");
        assert!(back.text(false).has_glowing_text);
        assert!(back.is_waxed());
        assert_eq!(
            back.player_who_may_edit(),
            None,
            "the edit token is not persisted and must not come back"
        );
    }

    /// A saved sign that carries a `playerWhoMayEdit` — which vanilla never
    /// writes — must not be honoured. Without this, a hand-written chunk
    /// could hand out an edit token that survives a restart.
    #[test]
    fn a_saved_editor_token_is_ignored_on_load() {
        let mut c = BTreeMap::new();
        c.insert("is_waxed".to_string(), Nbt::Byte(0));
        c.insert("playerWhoMayEdit".to_string(), Nbt::Long(1));
        assert_eq!(
            Sign::from_nbt(&Nbt::Compound(c)).player_who_may_edit(),
            None
        );
    }

    /// A compound with neither face is a blank sign rather than a parse
    /// failure — `Optional.orElseGet(SignText::new)` at offsets 26-34.
    #[test]
    fn a_compound_with_no_texts_loads_as_a_blank_sign() {
        let sign = Sign::from_nbt(&Nbt::Compound(BTreeMap::new()));
        assert_eq!(sign, Sign::default());
    }
}
