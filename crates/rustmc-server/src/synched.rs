//! `SynchedEntityData` — the per-entity values a client is told about, and
//! the `ClientboundSetEntityDataPacket` that carries them.
//!
//! # What is modelled, and what a client actually receives
//!
//! Vanilla gives every entity a fixed array of `DataItem`s, one per
//! `EntityDataAccessor` declared anywhere in its class chain, each holding a
//! value and the value it was **defined** with.
//! `SynchedEntityData.getNonDefaultValues()` walks that array in **accessor
//! id order** and collects only the items whose value differs from their
//! definition, returning `null` — not an empty list — when every one of them
//! matches. `ServerEntity.sendPairingData` sends a
//! `ClientboundSetEntityDataPacket` only when that result is non-null
//! (offsets 48-52 are `getfield trackedDataValues; ifnull 79`), so **an
//! entity at all its defaults produces no packet at all**.
//!
//! This server has two entity kinds, and only one of them is ever at
//! anything but its defaults:
//!
//! * `minecraft:item` — an `ItemEntity`'s stack lives at accessor 8. A block
//!   drop carries it ([`Synched::Item`], since 2026-09-30, `crate::dig`); a
//!   thrown item still does not, so [`Synched::EntityOnly`] is what one gets
//!   and no `set_entity_data` is produced for it. That is a real difference
//!   from vanilla and it is named in `docs/SERVER.md` rather than left to be
//!   found;
//! * `minecraft:player` — a `ServerPlayer` is an `Avatar`, and two of its
//!   accessors are away from their definitions the moment it joins.
//!
//! # The accessor ids, derived rather than recalled
//!
//! `SynchedEntityData.defineId(Class, serializer)` numbers accessors in
//! declaration order, continuing from the count the superclass reached, so
//! the ids are a property of the **class chain** and of the order of the
//! `defineId` calls in each `<clinit>`. Read out of the pinned 26.2 jar with
//! `javap -p -c -constants`, the chain for a player is
//! `Entity -> LivingEntity -> Avatar -> Player -> ServerPlayer`:
//!
//! ```text
//!   Entity        0 shared flags   1 air supply   2 custom name
//!                 3 custom name visible           4 silent
//!                 5 no gravity     6 pose         7 ticks frozen
//!   LivingEntity  8 living flags   9 health      10 effect particles
//!                11 effect ambience              12 arrow count
//!                13 stinger count 14 sleeping pos
//!   Avatar       15 main hand     16 model customisation
//!   Player       17 absorption    18 score       19 shoulder parrot left
//!                20 shoulder parrot right
//!   ServerPlayer  — declares none
//! ```
//!
//! **`Avatar` is the trap and it is a 26.2 one.** `DATA_PLAYER_MAIN_HAND` and
//! `DATA_PLAYER_MODE_CUSTOMISATION` are declared on `Avatar`, which sits
//! between `LivingEntity` and `Player`; a chain that goes straight from
//! `LivingEntity` to `Player` numbers absorption 15 and customisation 17, and
//! every id from 15 up is then wrong. `Player` itself declares exactly four
//! accessors, which is how the mistake is caught: a chain missing `Avatar`
//! has no home for the two that are left over.
//!
//! # Which two are away from their definitions
//!
//! From the `defineSynchedData` bodies in the same jar:
//!
//! * accessor 9 is defined `fconst_1` — **health's default is 1.0f**, and a
//!   player joins at `getMaxHealth()`, so it is always sent;
//! * accessor 16 is defined `iconst_0` and
//!   `ServerPlayer.updateOptions(ClientInformation)` sets it to
//!   `info.modelCustomisation()`, so it is sent for any client that displays
//!   any skin part — which is every real client and every client this
//!   harness drives.
//!
//! Accessor 15 is defined `DEFAULT_MAIN_HAND` and `updateOptions` sets it to
//! whatever the client asked for, so a right-handed client leaves it at its
//! definition and it is **not** sent. That asymmetry is the reason this is
//! written as "differs from its definition" rather than as a list of things
//! to send.
//!
//! # Confirmed against a recording, not only against the bytecode
//!
//! `530-walking-away-drops-a-standing-player-from-view` records vanilla
//! re-adding a standing player and its `set_entity_data` observation carries
//! `rest = 99a4e94964a5df0a`. The bytes this module produces for that player
//! are
//!
//! ```text
//!   09 03 41 A0 00 00   accessor 9, serializer FLOAT (3), 20.0f
//!   10 00 7F            accessor 16, serializer BYTE (0), 0x7f
//!   FF                  EOF marker
//! ```
//!
//! and `crate::synched::tests` digests exactly those ten bytes with the
//! recorder's own constant and reproduces `99a4e94964a5df0a`. A payload with
//! the right shape and the wrong list is the failure this closes: it looks
//! like the packet arrived.

use rustmc_proto::varint::write_varint;

/// `EntityDataSerializers`' registration order, which is the id on the wire.
///
/// `registerSerializer` pushes into a `CrudeIncrementalIntIdentityHashBiMap`,
/// so the id is the position of the call in `<clinit>`, and the **field
/// declaration order is not that order** — `BLOCK_STATE` is declared tenth
/// and registered fifteenth. Only the two this server needs are named; adding
/// a third means reading the registration list again, not counting fields.
pub mod serializer {
    /// `EntityDataSerializers.BYTE`, the first registered.
    pub const BYTE: i32 = 0;
    /// `EntityDataSerializers.INT`, the second registered — a varint on the
    /// wire (`ByteBufCodecs.VAR_INT`). `ExperienceOrb.DATA_VALUE` and
    /// `Player.DATA_SCORE_ID` are both this.
    pub const INT: i32 = 1;
    /// `EntityDataSerializers.FLOAT`, the fourth registered.
    pub const FLOAT: i32 = 3;
    /// `EntityDataSerializers.ITEM_STACK`, the eighth registered — read off
    /// the `registerSerializer` calls in `<clinit>`: BYTE, INT, LONG, FLOAT,
    /// STRING, COMPONENT, OPTIONAL_COMPONENT, ITEM_STACK.
    pub const ITEM_STACK: i32 = 7;
    /// `EntityDataSerializers.BOOLEAN`, the ninth registered — after BYTE,
    /// INT, LONG, FLOAT, STRING, COMPONENT, OPTIONAL_COMPONENT and
    /// ITEM_STACK in `EntityDataSerializers.<clinit>`'s
    /// `registerSerializer` calls.
    pub const BOOLEAN: i32 = 8;
    /// `EntityDataSerializers.DIRECTION`, the thirteenth registered (after
    /// ROTATIONS, BLOCK_POS and OPTIONAL_BLOCK_POS). Its codec is
    /// `Direction.STREAM_CODEC`, a varint of the 3D value, which for 0..6 is
    /// one byte.
    pub const DIRECTION: i32 = 12;
}

/// `ItemEntity.DATA_ITEM`'s accessor id. `ItemEntity` extends `Entity`
/// directly, `Entity.<clinit>` makes eight `defineId` calls (0-7), and
/// `ItemEntity.<clinit>`'s first `defineId` is `DATA_ITEM` with
/// `ITEM_STACK` — so 8.
pub const ITEM_DATA_ITEM: u8 = 8;

/// `Player.DATA_SCORE_ID` — accessor 18 in the chain table above.
pub const SCORE_ACCESSOR: u8 = 18;

/// `ExperienceOrb.DATA_VALUE`: `ExperienceOrb.<clinit>` is one
/// `defineId(ExperienceOrb.class, INT)`, and `Entity` declares eight (0-7)
/// before it, so the orb's only accessor is **8**.
pub const ORB_VALUE_ACCESSOR: u8 = 8;

/// `ClientboundSetEntityDataPacket.EOF_MARKER`, written after the last value.
///
/// It is a bare `writeByte(255)` and not a length prefix, so a packet with no
/// values would still be one byte long — which is why vanilla's decision to
/// send nothing at all lives in `sendPairingData`'s `ifnull` and not here.
const EOF_MARKER: u8 = 0xFF;

/// The synched data an entity carries, named by the vanilla class that
/// declares the accessors away from their definitions.
///
/// A `Vec<DataValue>` per entity would be more general and would also be a
/// place to put a value nothing derives, which is how a synched field ends up
/// being set by a call site instead of by the mechanic that owns it.
#[derive(Debug, Clone, PartialEq)]
pub enum Synched {
    /// Every accessor at its definition. Produces no packet.
    ///
    /// Correct today for `minecraft:item` only because this server does not
    /// yet carry an `ItemEntity`'s stack; vanilla does send one for an item.
    EntityOnly,
    /// An `ExperienceOrb`: `DATA_VALUE` at accessor 8.
    ///
    /// `ExperienceOrb extends Entity`, which defines **0..7**, so its own
    /// first `defineId` takes 8 — not 0, which is what counting only that
    /// class's own calls would give. The serializer is `INT`, whose stream
    /// codec is `ByteBufCodecs.VAR_INT`.
    ///
    /// **The value is the one field of an orb a player can see.** An orb sent
    /// with `EntityOnly` renders and is worth nothing, which is a silent
    /// wrongness rather than a missing entity.
    ExperienceOrb {
        /// `ExperienceOrb.getValue()`. `defineSynchedData` defines it as
        /// `0`, so an orb worth zero — which `award` never makes, since its
        /// loop guard is `amount > 0` — would send no packet at all.
        value: i32,
    },
    /// An `Avatar`: `LivingEntity.DATA_HEALTH_ID` at accessor 9 and
    /// `Avatar.DATA_PLAYER_MODE_CUSTOMISATION` at accessor 16.
    /// An `ItemEntity` — or a `ThrowableItemProjectile`, whose
    /// `DATA_ITEM_STACK` is the same accessor and serializer (an egg) — at
    /// accessor 8. An `ItemEntity`'s `DATA_ITEM` is defined
    /// `ItemStack.EMPTY` and set by `setItem` in the constructor, so a live
    /// item is always away from its definition. `value` is the stack already
    /// encoded with `ItemStack.OPTIONAL_STREAM_CODEC` — the entity holds the
    /// wire bytes rather than the stack because the tracker that sends them
    /// has no item table, and the stack itself is
    /// `crate::dispenser::ItemEntityStacks`'.
    Item { value: Vec<u8> },
    Avatar {
        /// `LivingEntity.getHealth()`. Defined as `1.0f`.
        health: f32,
        /// `ClientInformation.modelCustomisation()`, the displayed-skin-parts
        /// bitmask the client sent in configuration. Defined as `0`.
        model_customisation: u8,
        /// `Player.DATA_SCORE_ID` at accessor 18. Defined as `0`, moved only by
        /// `Player.increaseScore` — that is, by gaining experience points.
        score: i32,
    },
    /// An `EndCrystal`: `DATA_BEAM_TARGET` at accessor 8 (always its
    /// definition, `Optional.empty()`, here) and `DATA_SHOW_BOTTOM` at 9,
    /// defined `true` — the two `defineId`s of `EndCrystal.<clinit>`, after
    /// `Entity`'s eight.
    EndCrystal {
        /// `EndCrystal.showsBottom()`. `EndCrystalItem.useOn` clears it at
        /// offset 173.
        show_bottom: bool,
    },
    /// An `ItemFrame`: `HangingEntity.DATA_DIRECTION` at accessor 8, defined
    /// `Direction.SOUTH` (`DEFAULT_DIRECTION`), then the frame's own
    /// `DATA_ITEM` (9) and `DATA_ROTATION` (10). Rotation is always at its
    /// definition here; the item is set only for a frame generation placed
    /// holding one — an end city ship's elytra.
    ItemFrame {
        /// `getDirection().get3DDataValue()`.
        direction: u8,
        /// `DATA_ITEM`, already encoded with `ItemStack.OPTIONAL_STREAM_CODEC`;
        /// `None` is `ItemStack.EMPTY`, its definition.
        item: Option<Vec<u8>>,
    },
    /// A `LivingEntity` with nothing synched away from its definition but
    /// `DATA_HEALTH_ID` (accessor 9, defined `1.0F`) — a structure's mob as
    /// generation leaves it, at its maximum health.
    Living {
        /// `getHealth()`.
        health: f32,
    },
}

/// `LivingEntity.defineSynchedData`'s `define(DATA_HEALTH_ID, 1.0F)`.
const HEALTH_DEFAULT: f32 = 1.0;

/// `ExperienceOrb.defineSynchedData`'s `define(DATA_VALUE, 0)`.
const ORB_VALUE_DEFAULT: i32 = 0;

/// One entry of the packet, already encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataValue {
    /// The accessor id, written as a single **unsigned** byte —
    /// `DataValue.write` is `writeByte(this.id)` and the reader compares it
    /// against 255, so ids are 0..=254.
    pub id: u8,
    /// `EntityDataSerializers.getSerializedId(serializer)`, a varint.
    pub serializer: i32,
    /// The value, encoded by that serializer's `StreamCodec`.
    pub value: Vec<u8>,
}

impl Synched {
    /// `SynchedEntityData.getNonDefaultValues()`: the entries whose value
    /// differs from the one their accessor was defined with, in ascending
    /// accessor id, or `None` when every one of them matches.
    ///
    /// `None` and `Some(vec![])` are different answers and only the first can
    /// occur — vanilla allocates the list lazily inside the loop, so an empty
    /// list is unreachable there and would be a bug here.
    #[must_use]
    pub fn non_default_values(&self) -> Option<Vec<DataValue>> {
        let mut out: Vec<DataValue> = Vec::new();
        match *self {
            Self::EntityOnly => {}
            Self::Item { ref value } => {
                // `ItemStack.EMPTY` encodes as the single varint 0, and an
                // empty item is at its definition.
                if value.as_slice() != [0] {
                    out.push(DataValue {
                        id: ITEM_DATA_ITEM,
                        serializer: serializer::ITEM_STACK,
                        value: value.clone(),
                    });
                }
            }
            Self::ExperienceOrb { value } => {
                if value != ORB_VALUE_DEFAULT {
                    let mut v = Vec::with_capacity(5);
                    rustmc_proto::varint::write_varint(&mut v, value).expect("a Vec write");
                    out.push(DataValue {
                        id: 8,
                        serializer: serializer::INT,
                        value: v,
                    });
                }
            }
            Self::ItemFrame {
                direction,
                ref item,
            } => {
                // SOUTH is 3D value 3.
                if direction != 3 {
                    out.push(DataValue {
                        id: 8,
                        serializer: serializer::DIRECTION,
                        value: vec![direction],
                    });
                }
                if let Some(item) = item {
                    out.push(DataValue {
                        id: 9,
                        serializer: serializer::ITEM_STACK,
                        value: item.clone(),
                    });
                }
            }
            Self::Living { health } => {
                if health.to_bits() != HEALTH_DEFAULT.to_bits() {
                    out.push(DataValue {
                        id: 9,
                        serializer: serializer::FLOAT,
                        value: health.to_be_bytes().to_vec(),
                    });
                }
            }
            Self::EndCrystal { show_bottom } => {
                if !show_bottom {
                    out.push(DataValue {
                        id: 9,
                        serializer: serializer::BOOLEAN,
                        value: vec![0],
                    });
                }
            }
            Self::Avatar {
                health,
                model_customisation,
                score,
            } => {
                // Ascending accessor id: 9 before 16, because
                // `getNonDefaultValues` walks `itemsById` and not a set.
                //
                // The comparison is vanilla's `DataItem.isSetToDefault`,
                // which is `Objects.equals(initialValue, value)` — for a
                // boxed `Float` that is `Float.equals`, so it compares the
                // *bits* and not the numbers. `f32::to_bits` says the same
                // thing about the only case that separates them, `-0.0`,
                // and `f32 == f32` does not.
                if health.to_bits() != HEALTH_DEFAULT.to_bits() {
                    out.push(DataValue {
                        id: 9,
                        serializer: serializer::FLOAT,
                        value: health.to_be_bytes().to_vec(),
                    });
                }
                if model_customisation != 0 {
                    out.push(DataValue {
                        id: 16,
                        serializer: serializer::BYTE,
                        value: vec![model_customisation],
                    });
                }
                if score != 0 {
                    let mut b = Vec::new();
                    let _ = write_varint(&mut b, score);
                    out.push(DataValue {
                        id: SCORE_ACCESSOR,
                        serializer: serializer::INT,
                        value: b,
                    });
                }
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

impl Synched {
    /// `SynchedEntityData.packDirty()`, against the values last sent: the
    /// entries whose value changed since `last`, in ascending accessor id, or
    /// `None` when nothing did.
    ///
    /// `SynchedEntityData.set` marks an item dirty only when
    /// `ObjectUtils.notEqual(value, current)` — `Float.equals` for a boxed
    /// float, so the comparison is on bits, as [`Self::non_default_values`]'s
    /// is.
    #[must_use]
    pub fn dirty_values(self, last: Self) -> Option<Vec<DataValue>> {
        let mut out = Vec::new();
        if let (
            Self::Avatar {
                health,
                model_customisation,
                ..
            },
            Self::Avatar {
                health: was_health,
                model_customisation: was_model,
                ..
            },
        ) = (self, last)
        {
            if health.to_bits() != was_health.to_bits() {
                out.push(DataValue {
                    id: 9,
                    serializer: serializer::FLOAT,
                    value: health.to_be_bytes().to_vec(),
                });
            }
            if model_customisation != was_model {
                out.push(DataValue {
                    id: 16,
                    serializer: serializer::BYTE,
                    value: vec![model_customisation],
                });
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }
}

/// `ClientboundSetEntityDataPacket`'s body: the entity id, then each value,
/// then the EOF marker.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn set_entity_data_body(id: i32, values: &[DataValue]) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(16);
    write_varint(&mut b, id)?;
    for v in values {
        b.push(v.id);
        write_varint(&mut b, v.serializer)?;
        b.extend_from_slice(&v.value);
    }
    b.push(EOF_MARKER);
    Ok(b)
}

/// Entities whose data is dirty for their first tracker tick.
///
/// `ItemEntity`'s constructor calls `setItem`, which `set`s `DATA_ITEM` and
/// marks it dirty; `sendPairingData` reads `getNonDefaultValues` without
/// clearing that, so the next `ServerEntity.sendChanges` (offset 204,
/// `sendDirtyEntityData`, before the interval gate) sends the same value a
/// second time. Vanilla's recording of a survival dig has exactly two
/// `set_entity_data` for the dropped item on each stream, one in the bundle
/// and one alone, a tick apart.
fn dirty() -> &'static std::sync::Mutex<std::collections::BTreeSet<i32>> {
    static DIRTY: std::sync::OnceLock<std::sync::Mutex<std::collections::BTreeSet<i32>>> =
        std::sync::OnceLock::new();
    DIRTY.get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()))
}

/// Mark an entity's non-default values dirty until its next tracker tick.
pub fn mark_dirty(id: i32) {
    dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id);
}

/// `packDirty` for an entity marked by [`mark_dirty`]: its `set_entity_data`
/// body, once, then `None` until it is marked again.
#[must_use]
pub fn drain_dirty(id: i32, synched: &Synched) -> Option<Vec<u8>> {
    let was = dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id);
    if !was {
        return None;
    }
    let values = synched.non_default_values()?;
    set_entity_data_body(id, &values).ok()
}

/// Whether [`mark_dirty`] has marked `id` and nothing has drained it.
#[must_use]
pub fn is_dirty(id: i32) -> bool {
    dirty()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&id)
}

/// An entity marked by [`mark_dirty`] as `crate::tracker::DirtySource`:
/// data dirty once, never `needsSync`, never hurt.
pub struct DroppedSync<'a>(pub &'a crate::entities::Entity);

impl crate::tracker::DirtySource for DroppedSync<'_> {
    fn flags(&self) -> (bool, bool) {
        (is_dirty(self.0.id), false)
    }
    fn drain(&self) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
        (drain_dirty(self.0.id, &self.0.synched), None)
    }
    fn take_hurt_marked(&self) -> bool {
        false
    }
}

impl Synched {
    /// An `ItemEntity` holding `count` of the item with registry id `item`
    /// and no component patch: `OPTIONAL_STREAM_CODEC`'s count, id and the
    /// empty patch `0, 0` — or `ItemStack.EMPTY`'s single `0`.
    #[must_use]
    pub fn item(item: i32, count: i32) -> Self {
        let mut value = Vec::with_capacity(6);
        if count > 0 {
            // A `Vec` write cannot fail.
            let _ = write_varint(&mut value, count);
            let _ = write_varint(&mut value, item);
            value.extend_from_slice(&[0, 0]);
        } else {
            value.push(0);
        }
        Self::Item { value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `tools/oracle-runner`'s `trace::digest`, copied rather than depended
    /// on: the recorder is a separate crate this one must not link, and the
    /// multiplier is the single thing that has to agree. It is
    /// `0x1000_0000_01b3` — **one hex digit longer than FNV-64's prime**, a
    /// difference that leaves the bottom five bytes of every digest correct
    /// and the top three wrong, which reads exactly like a subtle bug in
    /// whatever is being derived. `trace::digest`'s own header says so; this
    /// copy exists so a change to either is caught here rather than by a
    /// corpus run.
    fn digest(bytes: &[u8]) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        format!("{h:016x}")
    }

    /// The observation `530` records from vanilla, reproduced from the bytes
    /// this module builds.
    ///
    /// The recorder consumes `set_entity_data`'s leading entity varint and
    /// digests everything after it, so the digest is taken over the body
    /// **minus its first byte** — which is also why an entity id cannot
    /// smuggle a difference into this comparison.
    #[test]
    fn the_pairing_payload_reproduces_vanillas_recorded_digest() {
        let s = Synched::Avatar {
            health: 20.0,
            model_customisation: 0x7f,
            score: 0,
        };
        let values = s.non_default_values().expect("two non-default values");
        let body = set_entity_data_body(1, &values).unwrap();
        assert_eq!(
            body,
            vec![0x01, 0x09, 0x03, 0x41, 0xA0, 0x00, 0x00, 0x10, 0x00, 0x7F, 0xFF]
        );
        assert_eq!(digest(&body[1..]), "99a4e94964a5df0a");
    }

    /// The two ways to get the shape right and the list wrong, both of which
    /// would look like the fix worked.
    #[test]
    fn a_wrong_accessor_id_or_a_wrong_order_is_a_different_payload() {
        let right = set_entity_data_body(
            1,
            &[
                DataValue {
                    id: 9,
                    serializer: serializer::FLOAT,
                    value: 20.0f32.to_be_bytes().to_vec(),
                },
                DataValue {
                    id: 16,
                    serializer: serializer::BYTE,
                    value: vec![0x7f],
                },
            ],
        )
        .unwrap();
        // `Player`-numbered rather than `Avatar`-numbered: customisation at
        // 17 instead of 16, which is what a chain without `Avatar` produces.
        let shifted = set_entity_data_body(
            1,
            &[
                DataValue {
                    id: 9,
                    serializer: serializer::FLOAT,
                    value: 20.0f32.to_be_bytes().to_vec(),
                },
                DataValue {
                    id: 17,
                    serializer: serializer::BYTE,
                    value: vec![0x7f],
                },
            ],
        )
        .unwrap();
        let swapped = set_entity_data_body(
            1,
            &[
                DataValue {
                    id: 16,
                    serializer: serializer::BYTE,
                    value: vec![0x7f],
                },
                DataValue {
                    id: 9,
                    serializer: serializer::FLOAT,
                    value: 20.0f32.to_be_bytes().to_vec(),
                },
            ],
        )
        .unwrap();
        assert_ne!(digest(&right[1..]), digest(&shifted[1..]));
        assert_ne!(digest(&right[1..]), digest(&swapped[1..]));
        assert_eq!(digest(&right[1..]), "99a4e94964a5df0a");
    }

    /// `getNonDefaultValues` returns `null` for an entity at its defaults,
    /// and `sendPairingData` then sends no packet — so this must be `None`
    /// and never `Some(vec![])`.
    #[test]
    fn an_entity_at_its_defaults_produces_no_list_at_all() {
        assert_eq!(Synched::EntityOnly.non_default_values(), None);
        assert_eq!(
            Synched::Avatar {
                health: 1.0,
                model_customisation: 0,
                score: 0,
            }
            .non_default_values(),
            None
        );
    }

    /// A client that displays no skin parts sends only its health, and a
    /// right-handed client never puts accessor 15 on the wire at all.
    #[test]
    fn only_the_accessors_away_from_their_definitions_are_sent() {
        let v = Synched::Avatar {
            health: 20.0,
            model_customisation: 0,
            score: 0,
        }
        .non_default_values()
        .expect("health alone");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id, 9);
    }

    /// A dropped item's one value: accessor 8, `ITEM_STACK` (7), then
    /// `OPTIONAL_STREAM_CODEC` — count, id, and the empty patch `0, 0`.
    #[test]
    fn an_item_entity_sends_its_stack_at_accessor_eight() {
        let v = Synched::item(55, 1)
            .non_default_values()
            .expect("a stack is never the EMPTY default");
        let body = set_entity_data_body(3, &v).expect("buf");
        assert_eq!(body, vec![3, 8, 7, 1, 55, 0, 0, EOF_MARKER]);
        assert!(Synched::item(55, 0).non_default_values().is_none());
    }

    /// Dirty once, then clean: the second `set_entity_data` is sent on the
    /// first tracker tick and never again.
    #[test]
    fn a_dirty_mark_is_drained_once() {
        let item = Synched::item(1, 2);
        assert!(drain_dirty(-77, &item).is_none(), "never marked");
        mark_dirty(-77);
        assert!(drain_dirty(-77, &item).is_some());
        assert!(drain_dirty(-77, &item).is_none(), "drained");
    }
}

#[cfg(test)]
mod orb_tests {
    use super::*;

    /// The orb's value is accessor **8** with the `INT` serializer, and the
    /// value is a **varint** — `ByteBufCodecs.VAR_INT`, not four raw bytes.
    ///
    /// Accessor 8 and not 0: `ExperienceOrb extends Entity`, which defines
    /// 0..7, so counting only `ExperienceOrb`'s own `defineId` calls gives the
    /// wrong id and a client would read the value as `Entity`'s shared flags.
    #[test]
    fn an_orbs_value_is_accessor_eight_as_a_varint() {
        let values = Synched::ExperienceOrb { value: 3 }
            .non_default_values()
            .expect("3 is not the default");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].id, 8);
        assert_eq!(values[0].serializer, serializer::INT);
        assert_eq!(values[0].value, vec![3], "a varint, one byte for 3");

        // 2477 is `getExperienceValue`'s top rung and needs two varint bytes,
        // which is what separates a varint from a byte and from a big-endian
        // int.
        let big = Synched::ExperienceOrb { value: 2477 }
            .non_default_values()
            .expect("not the default");
        assert_eq!(big[0].value, vec![0xAD, 0x13]);
    }

    /// `defineSynchedData` defines the value as 0, so an orb worth zero sends
    /// nothing — `None`, not an empty list, for the reason
    /// `non_default_values`' own doc gives.
    ///
    /// `ExperienceOrb.awardWithDirection`'s loop guard is `amount > 0`, so
    /// vanilla never constructs one; the arm exists so that a zero here is a
    /// silence rather than a packet claiming a default.
    #[test]
    fn an_orb_worth_zero_sends_nothing() {
        assert_eq!(
            Synched::ExperienceOrb { value: 0 }.non_default_values(),
            None
        );
    }
}
