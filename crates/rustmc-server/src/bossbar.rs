//! `/bossbar`'s state: `CustomBossEvents`, `CustomBossEvent` and the
//! `ServerBossEvent` under it, and the `boss_event` packet they send.
//!
//! # Who is sent what
//!
//! A bar keeps TWO player sets, and they are different things:
//!
//! * `CustomBossEvent.players` — the UUIDs the bar is *for*. `/bossbar set
//!   <id> players` writes it, and `CustomBossEvents.onPlayerConnect` re-adds
//!   a returning player from it;
//! * `ServerBossEvent.players` — the connections that currently hold the bar
//!   on their screen. Only this set is ever sent a packet.
//!
//! `ServerBossEvent.broadcast` sends an update only while the bar is
//! `visible`, `addPlayer` sends ADD only while visible, and `setVisible`
//! sends every holder ADD or REMOVE as the bar appears or disappears. Every
//! setter returns without a packet when the value is unchanged.
//!
//! # The UUID is a level-random draw
//!
//! `BossBarCommands.createBar` passes `source.getLevel().getRandom()` to
//! `CustomBossEvents.create`, which takes `Mth.createInsecureUUID(random)` —
//! two `nextLong`s. So creating a bar advances the level random, and the
//! UUID differs run to run; the oracle drops it (`boss_event.id`).

use crate::commands::{compound, write_network_nbt};
use crate::players::Connection;
use rustmc_proto::{ids, varint::write_varint, wire};
use rustmc_world::nbt::Nbt;
use std::io;
use std::sync::{Arc, Mutex};

/// `BossEvent.BossBarColor`, by serialized name, in ordinal order.
pub const COLORS: [&str; 7] = ["pink", "blue", "red", "green", "yellow", "purple", "white"];

/// `BossBarColor.getFormatting()`, per ordinal — the second constructor
/// argument of each constant in `BossBarColor.<clinit>`. Not the name: pink
/// is `RED`, red is `DARK_RED`, purple is `DARK_BLUE`.
pub const COLOR_FORMATTING: [&str; 7] = [
    "red",
    "blue",
    "dark_red",
    "green",
    "yellow",
    "dark_blue",
    "white",
];

/// `BossEvent.BossBarOverlay`, by serialized name, in ordinal order.
pub const OVERLAYS: [&str; 5] = [
    "progress",
    "notched_6",
    "notched_10",
    "notched_12",
    "notched_20",
];

/// `ClientboundBossEventPacket.OperationType` ordinals.
pub mod op {
    pub const ADD: i32 = 0;
    pub const REMOVE: i32 = 1;
    pub const UPDATE_PROGRESS: i32 = 2;
    pub const UPDATE_NAME: i32 = 3;
    pub const UPDATE_STYLE: i32 = 4;
}

/// One `CustomBossEvent`.
pub struct Bar {
    /// `customId`, namespaced.
    pub id: String,
    /// `BossEvent.id`, from [`insecure_uuid`].
    pub uuid: u128,
    /// `BossEvent.name`.
    pub name: Nbt,
    /// `BossEvent.color`, an index into [`COLORS`]. `WHITE` on creation.
    pub color: usize,
    /// `BossEvent.overlay`, an index into [`OVERLAYS`]. `PROGRESS` on creation.
    pub overlay: usize,
    /// `BossEvent.progress`.
    pub progress: f32,
    /// `ServerBossEvent.visible`, true on creation.
    pub visible: bool,
    /// `CustomBossEvent.value`, 0 on creation.
    pub value: i32,
    /// `CustomBossEvent.max`, `DEFAULT_MAX` 100 on creation.
    pub max: i32,
    /// `ServerBossEvent.players`: who holds the bar now.
    pub holders: Vec<Arc<Connection>>,
    /// `CustomBossEvent.players`: whose bar it is, by UUID.
    pub for_players: Vec<u128>,
}

/// `Mth.createInsecureUUID(random)`: two longs, masked into a version-4,
/// IETF-variant UUID.
pub fn insecure_uuid(mut next_long: impl FnMut() -> i64) -> u128 {
    #[expect(
        clippy::cast_sign_loss,
        reason = "the bits, as `new UUID(most, least)` keeps them"
    )]
    let most = ((next_long() & -61_441) | 16_384) as u64;
    #[expect(clippy::cast_sign_loss, reason = "the bits, as above")]
    let least = ((next_long() & 0x3FFF_FFFF_FFFF_FFFF) | i64::MIN) as u64;
    (u128::from(most) << 64) | u128::from(least)
}

fn uuid_bytes(b: &mut Vec<u8>, uuid: u128) {
    b.extend_from_slice(&uuid.to_be_bytes());
}

/// `ClientboundBossEventPacket.createAddPacket`: the name, the progress, the
/// colour and overlay as `writeEnum` varints, then `encodeProperties` — the
/// darken/music/fog flags, never set on a command bar.
///
/// # Errors
/// Never, for a `Vec`.
pub fn add_body(bar: &Bar) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    uuid_bytes(&mut b, bar.uuid);
    write_varint(&mut b, op::ADD)?;
    write_network_nbt(&mut b, &bar.name)?;
    wire::write_f32(&mut b, bar.progress)?;
    write_varint(&mut b, i32::try_from(bar.color).unwrap_or(0))?;
    write_varint(&mut b, i32::try_from(bar.overlay).unwrap_or(0))?;
    b.push(0);
    Ok(b)
}

/// `createRemovePacket(id)`: the UUID and the ordinal, no body.
///
/// # Errors
/// Never, for a `Vec`.
pub fn remove_body(uuid: u128) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    uuid_bytes(&mut b, uuid);
    write_varint(&mut b, op::REMOVE)?;
    Ok(b)
}

/// `createUpdateProgressPacket`.
///
/// # Errors
/// Never, for a `Vec`.
pub fn progress_body(bar: &Bar) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    uuid_bytes(&mut b, bar.uuid);
    write_varint(&mut b, op::UPDATE_PROGRESS)?;
    wire::write_f32(&mut b, bar.progress)?;
    Ok(b)
}

/// `createUpdateNamePacket`.
///
/// # Errors
/// Never, for a `Vec`.
pub fn name_body(bar: &Bar) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    uuid_bytes(&mut b, bar.uuid);
    write_varint(&mut b, op::UPDATE_NAME)?;
    write_network_nbt(&mut b, &bar.name)?;
    Ok(b)
}

/// `createUpdateStylePacket`.
///
/// # Errors
/// Never, for a `Vec`.
pub fn style_body(bar: &Bar) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    uuid_bytes(&mut b, bar.uuid);
    write_varint(&mut b, op::UPDATE_STYLE)?;
    write_varint(&mut b, i32::try_from(bar.color).unwrap_or(0))?;
    write_varint(&mut b, i32::try_from(bar.overlay).unwrap_or(0))?;
    Ok(b)
}

fn send(to: &Connection, body: &io::Result<Vec<u8>>) {
    if let Ok(body) = body {
        let _ = to.send(ids::play::cb::BOSS_EVENT, body);
    }
}

impl Bar {
    /// `CustomBossEvent(uuid, id, name, dirty)`: white, progress overlay,
    /// visible, value 0, max 100, and `setProgress(0)`.
    #[must_use]
    pub fn new(id: &str, uuid: u128, name: Nbt) -> Self {
        Self {
            id: id.to_string(),
            uuid,
            name,
            color: 6,
            overlay: 0,
            progress: 0.0,
            visible: true,
            value: 0,
            max: 100,
            holders: Vec::new(),
            for_players: Vec::new(),
        }
    }

    /// `CustomBossEvent.getDisplayName()`:
    /// `ComponentUtils.wrapInSquareBrackets(getName())` styled with the
    /// colour's formatting, a `show_text` hover of the id, and the id as the
    /// insertion.
    #[must_use]
    pub fn display_name(&self) -> Nbt {
        compound([
            ("with", Nbt::List(vec![self.name.clone()])),
            ("translate", Nbt::String("chat.square_brackets".into())),
            ("color", Nbt::String(COLOR_FORMATTING[self.color].into())),
            (
                "hover_event",
                compound([
                    ("action", Nbt::String("show_text".into())),
                    ("value", Nbt::String(self.id.clone())),
                ]),
            ),
            ("insertion", Nbt::String(self.id.clone())),
        ])
    }

    /// `ServerBossEvent.broadcast(fn)`: every holder, only while visible.
    fn broadcast(&self, body: &io::Result<Vec<u8>>) {
        if self.visible {
            for h in &self.holders {
                send(h, body);
            }
        }
    }

    /// `ServerBossEvent.setProgress`: unchanged is silent.
    #[allow(clippy::float_cmp)]
    pub fn set_progress(&mut self, p: f32) {
        if p != self.progress {
            self.progress = p;
            self.broadcast(&progress_body(self));
        }
    }

    /// `CustomBossEvent.setValue`: store, then
    /// `setProgress(Mth.clamp((float) value / max, 0, 1))`.
    #[expect(clippy::cast_precision_loss, reason = "`i2f`, as vanilla does")]
    pub fn set_value(&mut self, value: i32) {
        self.value = value;
        self.set_progress((value as f32 / self.max as f32).clamp(0.0, 1.0));
    }

    /// `CustomBossEvent.setMax`: as [`Self::set_value`], the other operand.
    #[expect(clippy::cast_precision_loss, reason = "`i2f`, as vanilla does")]
    pub fn set_max(&mut self, max: i32) {
        self.max = max;
        self.set_progress((self.value as f32 / max as f32).clamp(0.0, 1.0));
    }

    /// `ServerBossEvent.setName`: `Objects.equal` against the old name.
    pub fn set_name(&mut self, name: Nbt) {
        if name != self.name {
            self.name = name;
            self.broadcast(&name_body(self));
        }
    }

    /// `ServerBossEvent.setColor`.
    pub fn set_color(&mut self, color: usize) {
        if color != self.color {
            self.color = color;
            self.broadcast(&style_body(self));
        }
    }

    /// `ServerBossEvent.setOverlay`.
    pub fn set_overlay(&mut self, overlay: usize) {
        if overlay != self.overlay {
            self.overlay = overlay;
            self.broadcast(&style_body(self));
        }
    }

    /// `ServerBossEvent.setVisible`: each holder is sent ADD or REMOVE.
    pub fn set_visible(&mut self, visible: bool) {
        if visible == self.visible {
            return;
        }
        self.visible = visible;
        let body = if visible {
            add_body(self)
        } else {
            remove_body(self.uuid)
        };
        for h in &self.holders {
            send(h, &body);
        }
    }

    /// `ServerBossEvent.addPlayer`: ADD to it if it was not a holder and
    /// the bar is visible.
    pub fn add_holder(&mut self, p: &Arc<Connection>) {
        if self.holders.iter().any(|h| h.id() == p.id()) {
            return;
        }
        self.holders.push(Arc::clone(p));
        if self.visible {
            send(p, &add_body(self));
        }
    }

    /// `ServerBossEvent.removePlayer`: REMOVE to it if it was a holder and
    /// the bar is visible.
    pub fn remove_holder(&mut self, conn_id: u64) {
        let Some(i) = self.holders.iter().position(|h| h.id() == conn_id) else {
            return;
        };
        let p = self.holders.remove(i);
        if self.visible {
            send(&p, &remove_body(self.uuid));
        }
    }

    /// `CustomBossEvent.addPlayer`: the holder, then the UUID.
    pub fn add_player(&mut self, p: &Arc<Connection>) {
        self.add_holder(p);
        let u = rustmc_proto::wire::offline_uuid(p.name());
        if !self.for_players.contains(&u) {
            self.for_players.push(u);
        }
    }

    /// `CustomBossEvent.removePlayer`: the holder, then the UUID.
    pub fn remove_player(&mut self, p: &Connection) {
        self.remove_holder(p.id());
        let u = rustmc_proto::wire::offline_uuid(p.name());
        self.for_players.retain(|x| *x != u);
    }

    /// `ServerBossEvent.removeAllPlayers`.
    pub fn remove_all_holders(&mut self) {
        let ids: Vec<u64> = self.holders.iter().map(|h| h.id()).collect();
        for id in ids {
            self.remove_holder(id);
        }
    }

    /// `CustomBossEvent.setPlayers(players)`, offsets 0-396:
    ///
    /// ```text
    ///   toRemove = the stored UUIDs no given player has
    ///   toAdd    = the given players whose UUID is not stored
    ///   for uuid in toRemove:
    ///       a holder with that UUID -> removePlayer(holder) (UUID too)
    ///       this.players.remove(uuid)
    ///   for player in toAdd: addPlayer(player)
    ///   return !toRemove.isEmpty() || !toAdd.isEmpty()
    /// ```
    pub fn set_players(&mut self, players: &[Arc<Connection>]) -> bool {
        let given: Vec<u128> = players
            .iter()
            .map(|p| rustmc_proto::wire::offline_uuid(p.name()))
            .collect();
        let to_remove: Vec<u128> = self
            .for_players
            .iter()
            .copied()
            .filter(|u| !given.contains(u))
            .collect();
        let to_add: Vec<Arc<Connection>> = players
            .iter()
            .filter(|p| {
                !self
                    .for_players
                    .contains(&rustmc_proto::wire::offline_uuid(p.name()))
            })
            .cloned()
            .collect();
        for u in &to_remove {
            if let Some(h) = self
                .holders
                .iter()
                .find(|h| rustmc_proto::wire::offline_uuid(h.name()) == *u)
                .cloned()
            {
                self.remove_player(&h);
            }
            self.for_players.retain(|x| x != u);
        }
        for p in &to_add {
            self.add_player(p);
        }
        !to_remove.is_empty() || !to_add.is_empty()
    }
}

/// `CustomBossEvents.events`, a `HashMap<Identifier, CustomBossEvent>` —
/// kept in `HashMap`'s own iteration order, because `/bossbar list` prints
/// the bars in it.
///
/// Java's `HashMap` iterates bucket by bucket, each bucket in insertion
/// order, and its bucket is `(h ^ h >>> 16) & (capacity - 1)` with `h` the
/// key's `hashCode`. The table starts at 16 and doubles when the size passes
/// three quarters of it, or when a bucket reaches eight while the table is
/// under 64 (`treeifyBin`'s resize); it never shrinks. A resize keeps each
/// bucket's relative order. Treeified buckets (eight in one bucket at 64 or
/// more) iterate in the same linked order and need no case here.
#[derive(Default)]
pub struct Bars {
    entries: Vec<(i32, Bar)>,
    capacity: usize,
}

/// `Identifier.hashCode()`: `31 * namespace.hashCode() + path.hashCode()`.
#[must_use]
pub fn identifier_hash(id: &str) -> i32 {
    fn java(s: &str) -> i32 {
        s.encode_utf16()
            .fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c)))
    }
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    java(ns).wrapping_mul(31).wrapping_add(java(path))
}

/// `HashMap.hash(key)`: `h ^ (h >>> 16)`, as an unsigned index source.
#[must_use]
pub fn spread(hash: i32) -> usize {
    let h = u32::from_ne_bytes(hash.to_ne_bytes());
    (h ^ (h >> 16)) as usize
}

impl Bars {
    fn bucket(&self, hash: i32) -> usize {
        spread(hash) & (self.capacity - 1)
    }

    /// `HashMap.get`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Bar> {
        self.entries
            .iter()
            .find(|(_, b)| b.id == id)
            .map(|(_, b)| b)
    }

    /// `HashMap.get`, mutably.
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Bar> {
        self.entries
            .iter_mut()
            .find(|(_, b)| b.id == id)
            .map(|(_, b)| b)
    }

    /// `HashMap.put` of a new key.
    pub fn insert(&mut self, bar: Bar) {
        if self.capacity == 0 {
            self.capacity = 16;
        }
        let h = identifier_hash(&bar.id);
        let in_bucket = self
            .entries
            .iter()
            .filter(|(k, _)| self.bucket(*k) == self.bucket(h))
            .count();
        self.entries.push((h, bar));
        // `putVal`: `binCount >= TREEIFY_THRESHOLD - 1` -> `treeifyBin`, which
        // resizes instead under `MIN_TREEIFY_CAPACITY`; then `++size >
        // threshold` -> `resize`.
        if in_bucket >= 7 && self.capacity < 64 {
            self.capacity *= 2;
        }
        if self.entries.len() > self.capacity * 3 / 4 {
            self.capacity *= 2;
        }
    }

    /// `HashMap.remove`.
    pub fn remove(&mut self, id: &str) -> Option<Bar> {
        let i = self.entries.iter().position(|(_, b)| b.id == id)?;
        Some(self.entries.remove(i).1)
    }

    /// `values()` in iteration order.
    #[must_use]
    pub fn iter(&self) -> Vec<&Bar> {
        let mut order: Vec<(usize, usize)> = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, (h, _))| (self.bucket(*h), i))
            .collect();
        order.sort_unstable();
        order.into_iter().map(|(_, i)| &self.entries[i].1).collect()
    }

    /// `getEvents().size()`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `CustomBossEvents.onPlayerConnect`: every bar that is for this player
    /// takes it back as a holder.
    pub fn on_player_connect(&mut self, p: &Arc<Connection>) {
        let u = rustmc_proto::wire::offline_uuid(p.name());
        for (_, bar) in &mut self.entries {
            if bar.for_players.contains(&u) {
                bar.add_holder(p);
            }
        }
    }

    /// `CustomBossEvents.onPlayerDisconnect`: every bar drops the
    /// connection as a holder and keeps the UUID.
    pub fn on_player_disconnect(&mut self, conn_id: u64) {
        for (_, bar) in &mut self.entries {
            if let Some(i) = bar.holders.iter().position(|h| h.id() == conn_id) {
                // `ServerBossEvent.removePlayer` on a connection that is
                // going away: the REMOVE is written to it as vanilla writes
                // it, and a closed socket drops it.
                let p = bar.holders.remove(i);
                if bar.visible {
                    send(&p, &remove_body(bar.uuid));
                }
            }
        }
    }
}

/// The server's one `CustomBossEvents`.
pub fn shared() -> &'static Mutex<Bars> {
    static BARS: std::sync::OnceLock<Mutex<Bars>> = std::sync::OnceLock::new();
    BARS.get_or_init(|| Mutex::new(Bars::default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_hash_is_javas() {
        // "minecraft".hashCode() = 695073197, "a".hashCode() = 97.
        assert_eq!(
            identifier_hash("minecraft:a"),
            695_073_197i32.wrapping_mul(31).wrapping_add(97)
        );
    }

    #[test]
    fn the_insecure_uuid_is_version_4_ietf() {
        let mut v = [-1i64, -1i64].into_iter();
        let u = insecure_uuid(|| v.next().unwrap());
        assert_eq!((u >> 76) & 0xF, 4, "version nibble");
        assert_eq!((u >> 62) & 0b11, 0b10, "IETF variant");
    }

    #[test]
    fn list_order_is_bucket_order_not_insertion_order() {
        let mut bars = Bars::default();
        for id in ["minecraft:b", "minecraft:a", "minecraft:c"] {
            bars.insert(Bar::new(id, 0, Nbt::String(String::new())));
        }
        let mut want: Vec<(usize, &str)> = ["minecraft:b", "minecraft:a", "minecraft:c"]
            .iter()
            .map(|id| (spread(identifier_hash(id)) & 15, *id))
            .collect();
        want.sort_by_key(|(b, _)| *b);
        let got: Vec<&str> = bars.iter().iter().map(|b| b.id.as_str()).collect();
        let want: Vec<&str> = want.into_iter().map(|(_, id)| id).collect();
        assert_eq!(got, want);
    }
}
