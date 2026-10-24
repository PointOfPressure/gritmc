//! One logged-in play session, and the actions a scenario drives it with.
//!
//! Synchronous, single-threaded, and id-agnostic where it can be: packets
//! the harness has no interest in are skipped by frame, so a version bump
//! that adds a packet does not break the client. The protocol number is read
//! off the status ping rather than compiled in, for the same reason.
//!
//! Every serverbound layout here cites the packet it reproduces. The layouts
//! are not guesses: each one is exercised against **real vanilla** by
//! `ci/t2-gameplay.sh` before it is ever pointed at rustmc, and a wrong
//! field width against vanilla shows up immediately as a disconnect rather
//! than as a quietly wrong check.

use crate::scenario::{face_cursor, face_value, Step};
use crate::trace::{self, Normalizer, Obs};
use rustmc_proto::frame::Conn;
use rustmc_proto::ids;
use rustmc_proto::varint::{read_varint, write_varint};
use rustmc_proto::wire::*;
use std::collections::BTreeMap;
use std::io;
use std::time::{Duration, Instant};

/// The extracted tables a session needs to speak about blocks and items.
pub struct Tables {
    /// Item name to registry id. `Item.STREAM_CODEC` is `holderRegistry`, so
    /// the wire carries a plain varint id and nothing else.
    pub items: BTreeMap<String, i32>,
    /// The same registry the other way round, for the recorder rather than
    /// for the scenarios: [`crate::trace::read_item_stack`] turns the varint
    /// on the wire into a name, and **refuses an id this map does not hold**,
    /// which is where `IdMap.byIdOrThrow` throws. A registry is one-to-one, so
    /// this is an exact inverse and not a lossy one.
    pub item_names: BTreeMap<i32, String>,
    /// Global block-state id to state string, for readable failures.
    pub states: BTreeMap<i32, String>,
    /// Entity type name to registry id, for the verbs that name an entity.
    ///
    /// `ClientboundAddEntityPacket` writes the type through
    /// `ByteBufCodecs.registry(Registries.ENTITY_TYPE)` — a plain varint of
    /// the registry id — so this is what turns `minecraft:player` in a
    /// scenario into the number that arrives on the wire.
    pub entity_types: BTreeMap<String, i32>,
    /// `minecraft:data_component_type` name to registry id — a **different
    /// registry from `minecraft:item`**, 111 entries against 1,537.
    ///
    /// A `DataComponentPatch` names each entry by this id, so without the
    /// table [`crate::scenario::Step::Hold`] could only write the literal
    /// `0 added` it wrote until 2026-08-13, and no scenario could put a
    /// component on a stack. See [`write_component_patch`] for why that made
    /// the item census's `0 opaque` a reading of this harness.
    pub component_types: BTreeMap<String, i32>,
    /// `minecraft:block_entity_type` name to registry id, from
    /// `protocol_ids.json` (`ci/dump-protocol-ids.sh`). Read by the
    /// `minecraft:block_entity_data` component writer only, and empty when the
    /// file is absent — which refuses that component at write time, by name.
    pub block_entity_types: BTreeMap<String, i32>,
}

impl Tables {
    /// Load both from `data/vanilla/`.
    ///
    /// # Errors
    /// If either extract is missing. Named rather than defaulted: a harness
    /// that cannot name an item would place nothing and report it as the
    /// server's fault.
    pub fn load(dir: &std::path::Path) -> io::Result<Self> {
        let items_path = dir.join("items.json");
        let raw = std::fs::read_to_string(&items_path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: {e} — run ci/dump-items.sh", items_path.display()),
            )
        })?;
        let v_items: serde_json::Value = serde_json::from_str(&raw).map_err(io::Error::other)?;
        let mut items = BTreeMap::new();
        let mut item_names = BTreeMap::new();
        for (k, id) in v_items["items"].as_object().into_iter().flatten() {
            if let Some(n) = id.as_i64() {
                let id = i32::try_from(n).unwrap_or(0);
                items.insert(k.clone(), id);
                item_names.insert(id, k.clone());
            }
        }

        let props_path = dir.join("block_props.json");
        let raw = std::fs::read_to_string(&props_path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!("{}: {e} — run ci/dump-registries.sh", props_path.display()),
            )
        })?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(io::Error::other)?;
        let mut states = BTreeMap::new();
        for row in v["table"].as_array().into_iter().flatten() {
            if let (Some(id), Some(state)) = (row["id"].as_i64(), row["state"].as_str()) {
                states.insert(i32::try_from(id).unwrap_or(0), state.to_string());
            }
        }
        let types_path = dir.join("entity_types.json");
        let raw = std::fs::read_to_string(&types_path).map_err(|e| {
            io::Error::new(
                e.kind(),
                format!(
                    "{}: {e} — run ci/dump-entity-types.sh",
                    types_path.display()
                ),
            )
        })?;
        let v: serde_json::Value = serde_json::from_str(&raw).map_err(io::Error::other)?;
        let mut entity_types = BTreeMap::new();
        for (k, id) in v["entity_types"].as_object().into_iter().flatten() {
            if let Some(n) = id.as_i64() {
                entity_types.insert(k.clone(), i32::try_from(n).unwrap_or(0));
            }
        }

        // Named, not defaulted, and the third table is here for the same
        // reason as the first two. A harness that cannot name an entity type
        // would resolve every `attack` target to nothing — and the failure
        // that follows is a scenario that hits nobody while both servers agree
        // it hit nobody, which reports `match`.
        // The same file, because it comes off the same report in the same
        // `ci/dump-items.sh` run. **Not defaulted to empty**, for the reason
        // the three above are not: an absent table would make every
        // `components = [...]` in a scenario refuse at run time with a
        // registry error, and a scenario that refuses looks like a scenario
        // whose server said no.
        let mut component_types = BTreeMap::new();
        for (k, id) in v_items["data_component_types"]
            .as_object()
            .into_iter()
            .flatten()
        {
            if let Some(n) = id.as_i64() {
                component_types.insert(k.clone(), i32::try_from(n).unwrap_or(0));
            }
        }

        // The `minecraft:sound_event` registry, for [`crate::pitch`].
        //
        // **Absence is not an error here, and that is the one place this table
        // differs from the four above.** Those are load-bearing for what the
        // harness *does* — a missing item table places nothing, a missing
        // entity-type table resolves every `attack` to nobody — so they are
        // named rather than defaulted. This one only widens what a `sound`
        // observation compares, and without it `pitch::observed_by_id` answers
        // `None` for every event, which is exactly the behaviour every
        // recording before 2026-08-17 was taken under. Refusing to run would
        // trade a real loss of coverage for no gain in honesty; the coverage
        // is printed instead, so a tree missing the file says so.
        let ids_path = dir.join("protocol_ids.json");
        let mut block_entity_types = BTreeMap::new();
        if let Ok(raw) = std::fs::read_to_string(&ids_path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                for (k, id) in v["block_entity_types"].as_object().into_iter().flatten() {
                    if let Some(n) = id.as_i64() {
                        block_entity_types.insert(k.clone(), i32::try_from(n).unwrap_or(0));
                    }
                }
                let mut sound_names = BTreeMap::new();
                for (k, id) in v["sounds"].as_object().into_iter().flatten() {
                    if let Some(n) = id.as_i64() {
                        sound_names.insert(i32::try_from(n).unwrap_or(0), k.clone());
                    }
                }
                crate::pitch::install_registry(sound_names);
            }
        }

        if items.is_empty() || states.is_empty() || entity_types.is_empty() {
            return Err(io::Error::other(
                "the item, block-state or entity-type extract is empty — the \
                 harness would report every placement as unimplemented and \
                 resolve every entity target to nothing",
            ));
        }
        if component_types.is_empty() {
            return Err(io::Error::other(
                "items.json carries no `data_component_types` — it was written \
                 by a dumper older than 2026-08-13. Re-run ci/dump-items.sh: \
                 without it no scenario can put a data component on a stack, \
                 and `trace::read_item_stack`'s opaque arm is unreachable",
            ));
        }
        Ok(Self {
            items,
            item_names,
            states,
            entity_types,
            component_types,
            block_entity_types,
        })
    }
}

/// `ByteBufCodecs.COMPOUND_TAG` for a flat compound of strings and longs:
/// `TAG_Compound` (10) with no name, then each entry as its tag type, a
/// `writeUTF` name and the payload, then `TAG_End`. Modified UTF-8 and plain
/// UTF-8 agree for every key and value a scenario can sensibly write (no NUL,
/// nothing outside the BMP); anything else is refused rather than mis-encoded.
fn write_flat_compound(
    b: &mut Vec<u8>,
    strings: &BTreeMap<String, String>,
    longs: &BTreeMap<String, i64>,
) -> io::Result<()> {
    fn utf(b: &mut Vec<u8>, s: &str) -> io::Result<()> {
        if s.chars().any(|c| c == '\0' || u32::from(c) > 0xFFFF) {
            return Err(io::Error::other(format!(
                "{s:?} needs modified UTF-8 this harness does not write"
            )));
        }
        let n = u16::try_from(s.len()).map_err(io::Error::other)?;
        b.extend_from_slice(&n.to_be_bytes());
        b.extend_from_slice(s.as_bytes());
        Ok(())
    }
    b.push(10);
    for (k, v) in strings {
        b.push(8);
        utf(b, k)?;
        utf(b, v)?;
    }
    for (k, v) in longs {
        b.push(4);
        utf(b, k)?;
        b.extend_from_slice(&v.to_be_bytes());
    }
    b.push(0);
    Ok(())
}

/// Write a `DataComponentPatch` — `n` added, none removed — onto `b`.
///
/// `DataComponentPatch$3.encode` writes the two counts as VarInts and then,
/// for each addition, the type's registry id followed by that type's own
/// `networkSynchronized` stream codec applied to the value. Nothing carries a
/// length, which is the same fact that stops `trace::read_item_stack` on the
/// decoding side: a writer needs the codec, not just the id.
///
/// **An empty patch is byte-for-byte what this call site wrote before this
/// function existed** — `0, 0` — so every scenario in the corpus that does
/// not name a component sends the identical packet it always did, and no
/// baseline moves for the existence of this feature. That was checked by
/// `an_empty_patch_is_the_two_zero_bytes_the_old_call_site_wrote`, below,
/// rather than argued.
///
/// # Errors
/// If a named type is not in the registry extract, or is one this harness has
/// no stream codec for. Both are harness bugs and are reported as such — see
/// [`crate::scenario::HeldComponent`] for why they must not be allowed to
/// reach the wire.
pub fn write_component_patch(
    b: &mut Vec<u8>,
    components: &[crate::scenario::HeldComponent],
    tables: &Tables,
) -> io::Result<()> {
    // **Resolved before a byte is written**, so a refusal leaves the caller's
    // buffer exactly as it found it. Written inline, the two patch counts
    // reached `b` before the first bad type was even looked at, and a caller
    // that logged the refusal and carried on would have sent a stack whose
    // patch promised entries that are not there — a truncation vanilla reads
    // as a malformed packet. Caught by
    // `a_component_with_no_codec_here_is_refused_rather_than_written`, which
    // asserted the buffer rather than only the error.
    let mut resolved = Vec::with_capacity(components.len());
    for c in components {
        if let Some(why) = c.refusal() {
            return Err(io::Error::other(format!(
                "hold names data component {} — {why}; this harness writes only {:?}. \
                 Add its stream codec (read `networkSynchronized` off \
                 `DataComponents` in the pinned jar) before naming it in a \
                 scenario; sending the wrong bytes disconnects the client and \
                 arrives in the recording as a dead reference",
                c.kind,
                crate::scenario::ENCODABLE_COMPONENTS,
            )));
        }
        let id = *tables.component_types.get(&c.kind).ok_or_else(|| {
            io::Error::other(format!(
                "no data component type called {} in the extract — \
                 run ci/dump-items.sh",
                c.kind
            ))
        })?;
        // The value's own stream codec, written into its own buffer because
        // the untrusted patch length-prefixes it (below).
        let mut payload = Vec::new();
        if let Some(v) = c.varint {
            write_varint(&mut payload, v)?;
        } else if let Some(raw) = &c.bytes {
            payload.extend_from_slice(raw);
        } else if let Some(be) = &c.block_entity {
            let be_id = *tables.block_entity_types.get(be).ok_or_else(|| {
                io::Error::other(format!(
                    "no block entity type called {be} in protocol_ids.json — \
                     run ci/dump-protocol-ids.sh"
                ))
            })?;
            write_varint(&mut payload, be_id)?;
            write_flat_compound(&mut payload, &c.strings, &c.longs)?;
        } else if let Some(sherds) = &c.sherds {
            write_varint(
                &mut payload,
                i32::try_from(sherds.len()).map_err(io::Error::other)?,
            )?;
            for item in sherds {
                let id = *tables.items.get(item).ok_or_else(|| {
                    io::Error::other(format!("no item called {item} in the extract"))
                })?;
                write_varint(&mut payload, id)?;
            }
        } else if let Some(props) = &c.properties {
            write_varint(
                &mut payload,
                i32::try_from(props.len()).map_err(io::Error::other)?,
            )?;
            for (k, v) in props {
                for s in [k, v] {
                    write_varint(
                        &mut payload,
                        i32::try_from(s.len()).map_err(io::Error::other)?,
                    )?;
                    payload.extend_from_slice(s.as_bytes());
                }
            }
        } else if let Some(slots) = &c.container {
            write_varint(
                &mut payload,
                i32::try_from(slots.len()).map_err(io::Error::other)?,
            )?;
            for slot in slots {
                match &slot.item {
                    None => payload.push(0),
                    Some(item) => {
                        let id = *tables.items.get(item).ok_or_else(|| {
                            io::Error::other(format!("no item called {item} in the extract"))
                        })?;
                        payload.push(1);
                        // `ItemStackTemplate`: the item BEFORE the count.
                        write_varint(&mut payload, id)?;
                        write_varint(&mut payload, slot.count)?;
                        write_varint(&mut payload, 0)?;
                        write_varint(&mut payload, 0)?;
                    }
                }
            }
        } else if let Some(text) = &c.text {
            // `ComponentSerialization.CODEC` on a plain literal: the bare
            // string, so one network NBT string tag.
            payload.push(8); // TAG_String, the unnamed network root
            payload.extend_from_slice(
                &u16::try_from(text.len())
                    .map_err(io::Error::other)?
                    .to_be_bytes(),
            );
            payload.extend_from_slice(text.as_bytes());
        } else if let Some(entity) = &c.entity {
            // `TypedEntityData.streamCodec`: the entity type's registry id,
            // then `ByteBufCodecs.COMPOUND_TAG` — a network compound, whose
            // root is the tag type byte (10) with NO name, then its entries
            // and an end byte. One entry: `Silent`, a byte tag (1), which
            // `Entity.load` reads with `getBooleanOr("Silent", false)`.
            let ty = *tables.entity_types.get(entity).ok_or_else(|| {
                io::Error::other(format!("no entity type called {entity} in the extract"))
            })?;
            write_varint(&mut payload, ty)?;
            payload.push(10); // TAG_Compound, the unnamed network root
            payload.push(1); // TAG_Byte
            payload.extend_from_slice(&6u16.to_be_bytes());
            payload.extend_from_slice(b"Silent");
            payload.push(u8::from(c.silent.unwrap_or(false)));
            // `LeftHanded`, read by `Mob.readAdditionalSaveData` (javap 111-120,
            // `getBooleanOr("LeftHanded", false)`). Only when declared, so every
            // egg that does not name it keeps the bytes it has always sent.
            if let Some(left) = c.left_handed {
                payload.push(1); // TAG_Byte
                payload.extend_from_slice(&10u16.to_be_bytes());
                payload.extend_from_slice(b"LeftHanded");
                payload.push(u8::from(left));
            }
            payload.push(0); // TAG_End
        }
        resolved.push((id, payload));
    }
    write_varint(b, i32::try_from(resolved.len()).unwrap_or(0))?;
    write_varint(b, 0)?; // components to remove
    for (id, payload) in resolved {
        write_varint(b, id)?;
        // **The value is LENGTH-PREFIXED, and this one byte is the whole
        // difference between a stack vanilla reads and a disconnect.**
        //
        // `ServerboundSetCreativeModeSlotPacket.STREAM_CODEC` composes
        // `ItemStack.validatedStreamCodec(ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC)`,
        // and the *untrusted* one is built on
        // `DataComponentPatch.DELIMITED_STREAM_CODEC` rather than on
        // `DataComponentPatch.STREAM_CODEC`. The delimited variant wraps each
        // added value in `ByteBufCodecs.registryFriendlyLengthPrefixed`, which
        // writes a VarInt byte-count and then the bytes; the trusted variant
        // does not. Type ids, added and removed, stay bare in both.
        //
        // Serverbound is the only direction that uses it — no other class in
        // the jar names `OPTIONAL_UNTRUSTED_STREAM_CODEC` — which is why
        // `trace::read_item_stack`, which reads CLIENTBOUND stacks, correctly
        // expects no prefix and must not grow one.
        //
        // Without the prefix vanilla reads the value's first byte as a length
        // and slices past the end of the packet: `IndexOutOfBoundsException`,
        // wrapped by netty as `DecoderException: Failed to decode packet
        // 'serverbound/minecraft:set_creative_mode_slot'`. An empty patch
        // never reaches this loop, which is exactly why every scenario written
        // before 2026-08-13 was unaffected and 861 was the first to notice.
        // `docs/PARITY-ROADMAP.md` §44.7.
        write_varint(b, i32::try_from(payload.len()).unwrap_or(0))?;
        b.extend_from_slice(&payload);
    }
    Ok(())
}

/// The most recently added live entity of a given type, or `None`.
///
/// Split out of [`Client::act`] so the resolution rule is reachable from a
/// test without a socket, a server or a login. The rule itself is "the newest
/// one", which is what a scenario means when it says `minecraft:player` in a
/// world that contains the actor and one observer: the actor never sees an
/// `add_entity` for itself, so the newest player it has been told about is the
/// other one.
#[must_use]
pub fn newest_of_type(entities: &[(i32, i32)], type_id: i32) -> Option<i32> {
    entities
        .iter()
        .rev()
        .find(|(_, t)| *t == type_id)
        .map(|(e, _)| *e)
}

/// The oldest live entity of `type_id` this client knows of — the first of
/// two pigs a breeding rig hatched. [`newest_of_type`]'s mirror.
#[must_use]
pub fn oldest_of_type(entities: &[(i32, i32)], type_id: i32) -> Option<i32> {
    entities
        .iter()
        .find(|(_, t)| *t == type_id)
        .map(|(e, _)| *e)
}

/// `ServerboundInteractPacket.STREAM_CODEC`: `VAR_INT` entity id,
/// `InteractionHand.STREAM_CODEC` (an `idMapper` varint, `MAIN_HAND` 0),
/// `Vec3.LP_STREAM_CODEC` location, `BOOL` secondary action.
///
/// The location is the zero vector, whose `LpVec3.write` form is the single
/// byte 0 (`LpVec3.write` offsets 26-54: the largest component magnitude
/// under `ABS_MIN`, 3.05e-5, writes `writeByte(0)` and returns). See [`Step::Interact`] for
/// why zero.
///
/// # Errors
/// Only a write to a `Vec` failing, which it cannot.
pub fn interact_body(entity: i32, hand: i32, secondary: bool) -> io::Result<Vec<u8>> {
    let mut b = Vec::new();
    write_varint(&mut b, entity)?;
    write_varint(&mut b, hand)?;
    write_u8(&mut b, 0)?;
    write_u8(&mut b, u8::from(secondary))?;
    Ok(b)
}

/// How long to wait for a frame to *start* before giving the other client a
/// turn. Short, because two sockets are read in alternation.
const POLL: Duration = Duration::from_millis(15);
/// How long to wait for the *rest* of a frame once one has started. Long,
/// because a partially-read frame is unrecoverable — see `Conn::recv_poll`.
const PATIENCE: Duration = Duration::from_secs(15);

/// The `f32` payload of `chunk_batch_received`, big-endian: `64.0`.
///
/// `PlayerChunkSender.onChunkBatchReceivedByClient` clamps the value into
/// `[MIN_CHUNKS_PER_TICK, MAX_CHUNKS_PER_TICK]` = `[0.01, 64.0]` and stores
/// it as `desiredChunksPerTick`, which is the whole of the next batch's
/// quota. A real client reports how many chunks it managed to process, so
/// the number is a bandwidth estimate and varies. **This harness reports the
/// maximum on purpose**: pacing is not what any check measures, and a
/// varying quota would put chunk arrivals in different ticks on different
/// runs, which is precisely the kind of run-to-run variation this whole
/// investigation exists to remove.
const DESIRED_CHUNKS_PER_TICK: [u8; 4] = [0x42, 0x80, 0x00, 0x00];

/// Escape hatch for the measurement that found the stall, and for nothing
/// else. With `ORACLE_NO_CHUNK_ACK=1` the harness goes back to never
/// acknowledging a chunk batch, which is what every trace recorded before
/// 2026-08-03 was taken under.
fn chunk_ack_enabled() -> bool {
    std::env::var_os("ORACLE_NO_CHUNK_ACK").is_none()
}

/// A logged-in play-state session.
pub struct Session {
    pub conn: Conn,
    /// Where the server first put the player. Fixed for the session.
    pub spawn: [f64; 3],
    /// Where the player is now — updated by walking, and by any correction
    /// the server sends back.
    pub pos: [f64; 3],
    /// The block the player is standing in — every scenario coordinate is
    /// relative to this.
    pub anchor: [i32; 3],
    /// The next sequence number for a block action. The harness owns the
    /// counter so both servers see the same number for the same step, which
    /// is what makes a `block_changed_ack` comparable at all.
    pub seq: i32,
    /// How many `level_chunk_with_light` packets this session has been sent,
    /// and how many chunk batches it has been told finished.
    ///
    /// Counted because they are the instrument that measured the chunk-batch
    /// stall: a client that does not acknowledge a batch is sent exactly one
    /// of them, and every broadcast about a chunk it never received is
    /// silently withheld. See `liveness`.
    pub chunks_seen: u32,
    pub batches_seen: u32,
    /// Where the player is looking. Tracked because `use_item` **carries the
    /// rotation in the packet** and vanilla ray-traces from it: this used to
    /// be hardcoded to `(0.0, 0.0)`, so a `look` step was silently ignored by
    /// every `use_item` and anything aimed at the ground could not work. It
    /// is why `water-spreads` sat in `open/` — a bucket used while looking
    /// dead ahead on flat terrain hits nothing within reach and places
    /// nothing, and two servers then agree to do nothing.
    pub yaw: f32,
    /// The root command names this connection was told the server has, in the
    /// server's own registration order, from `ClientboundCommandsPacket`.
    ///
    /// # Why it lives on the session and not in a scenario's observations
    ///
    /// The packet arrives during the join, and `record::record` logs both
    /// players in and then `settle`s them before the first scenario's
    /// measured window opens. Everything the join says is therefore *drained*
    /// — so a scenario that named `commands` in its `observe` list would
    /// record nothing, and the check built on it would pass by being empty.
    /// That is the shape this corpus already has 61 of.
    ///
    /// `anchor` and `spawn` are join-time facts kept here for the same
    /// reason, and reported per scenario the same way.
    ///
    /// Vanilla filters this tree through `fillUsableCommands` for the
    /// receiving player, so what lands here is not the registry: it is the
    /// part of the registry a `PermissionLevel.ALL` source may use. The
    /// corpus's players are not operators, which makes this ten roots on
    /// vanilla rather than ninety.
    pub declared_commands: Vec<String>,
    pub pitch: f32,
    /// Every entity this client has been told exists, as `(entity id, type
    /// id)` in arrival order, with removals applied.
    ///
    /// # Why the harness has to keep this, and why it keeps the *raw* ids
    ///
    /// `ServerboundAttackPacket` names its target by entity id, and an entity
    /// id is a per-run number: it depends on every entity the process has ever
    /// spawned. That is exactly why observations renumber them through
    /// `trace::ENTITY_REF` before comparing. But the *scenario* has to send
    /// one, and it can only send the number this server actually assigned —
    /// so the raw id is kept here, un-normalised, and only ever travels back
    /// out to the server that issued it. The two servers will send different
    /// numbers and each client will attack the right entity on its own server,
    /// which is the whole point.
    ///
    /// Populated in [`Session::liveness`], which every read path already runs.
    /// Putting it there rather than in `trace::observe` is deliberate: an
    /// observation is filtered by the scenario's `observe` list and may be
    /// dropped, and a target that exists only when the scenario happens to be
    /// watching for it is a target that resolves intermittently.
    pub entities: Vec<(i32, i32)>,
    /// The menu this client has been told is open, if any.
    ///
    /// # Why a click cannot be written down without this
    ///
    /// `ServerboundContainerClickPacket` names its menu by `containerId`, and
    /// `handleContainerClick`'s **first** branch is
    ///
    /// ```text
    ///   player.containerMenu.containerId != packet.containerId()  ->  return
    /// ```
    ///
    /// — a silent return, no log, no reply. That number is
    /// `ServerPlayer.nextContainerCounter`, `containerCounter % 100 + 1` on a
    /// field that lives as long as the player, so it is a per-session running
    /// count and no scenario can know it. It is exactly the situation
    /// [`Session::entities`] is in: the harness has to send back the number
    /// *this* server issued, so it reads it off `open_screen` and keeps it raw.
    ///
    /// The two servers will be on different counts if they ever disagree about
    /// which blocks open a menu, and that is fine and even desirable: each
    /// client clicks the right menu on its own server, and the *divergence in
    /// the counter* stays visible where it belongs, in `open_screen`'s own
    /// `window` field, which the corpus already compares.
    ///
    /// `state_id` is tracked for the same reason and has a sharper
    /// consequence: a mismatch is not refused, it makes the server answer with
    /// `broadcastFullState()` — the whole menu, every slot — instead of
    /// `broadcastChanges()`. That would drown a targeted correction in a
    /// packet whose payload includes the player's entire inventory, which
    /// accumulates across a corpus run, and would make every container check
    /// depend on which other checks ran before it.
    ///
    /// Tracked in [`Session::liveness`], for `entities`' reason. Reset
    /// structurally by `crate::record` before every scenario — see
    /// [`Step::CloseContainer`].
    pub menu: Option<OpenMenu>,
    /// The teleport id of the most recent `player_position` this connection
    /// was sent, kept raw for [`Session::ack_teleport_again`]'s reason.
    ///
    /// It is the id of a teleport this client has **already acknowledged**:
    /// both the join loop and [`Session::liveness`] answer a `player_position`
    /// the moment they see one, and record the id here as they do. Nothing
    /// re-sends it by itself.
    ///
    /// The two servers issue their own ids and each client answers its own,
    /// which is the same arrangement [`Session::entities`] and
    /// [`Session::menu`] are in.
    pub last_teleport: i32,
    /// Set once this connection has been hung up on, by either side.
    ///
    /// **Only a scenario that declared a probe can reach this**, and that is
    /// the whole safety of it: an unexpected disconnect anywhere else is still
    /// an `io::Error` that fails the scenario loudly. A flag that made every
    /// socket failure survivable would turn a server that crashed halfway
    /// through the corpus into ninety checks quietly reporting that it had
    /// nothing to say.
    pub closed: bool,
    /// True only for a probe — the connection a scenario is allowed to lose.
    ///
    /// Set by `crate::record::act_on` at `probe_join` and by nothing else, so
    /// the actor and the observer keep every one of their loud failures.
    pub expendable: bool,
    /// Set by a `respawn` packet and spent by the next `player_position`.
    ///
    /// # A respawned player has to say it has loaded, exactly as a joined one does
    ///
    /// `ServerPlayer.die` ends with `connection.markClientUnloadedAfterDeath()`
    /// and `PlayerList.respawn` restarts the load timer, so after a respawn
    /// `hasClientLoaded()` is false until the client sends
    /// `ServerboundPlayerLoadedPacket` — or sixty ticks pass. While it is
    /// false `ServerPlayer.isInvulnerableTo` answers true (offsets 26..36),
    /// eight packet handlers drop their packet, and a hit is refused. The
    /// join path already answers the first `player_position` with
    /// `accept_teleportation` then `player_loaded`, because that is what a
    /// real client does when its level screen closes; this is the same
    /// answer to the same screen after a death. Without it every scenario
    /// recorded in the three seconds after a respawn runs against a player
    /// the server still treats as loading.
    pub awaiting_loaded: bool,
    /// The last `set_health` this client was sent, or `None` before any.
    ///
    /// Read by [`crate::record`] after a scenario's restores: a player the
    /// server has told it is at zero is dead, and the one restore for that is
    /// the client's own `perform_respawn`. See `record::respawn_if_dead`.
    pub health: Option<f32>,
    /// The sneak state this client last sent in a `player_input`.
    ///
    /// Kept because `ServerboundInteractPacket` carries it again as
    /// `usingSecondaryAction`, and `handleInteract` offset 52 writes that
    /// straight into `setShiftKeyDown` — so an interact sending anything but
    /// the client's real state would press or release sneak behind the
    /// scenario's back. See [`Step::Interact`].
    pub sneaking: bool,
}

/// What the harness knows about the menu the server says is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenMenu {
    /// `AbstractContainerMenu.containerId`, as this server issued it.
    pub container_id: i32,
    /// The last `stateId` this server stamped on a container packet.
    pub state_id: i32,
}

/// Server list ping with one bounded retry — a cold-booted server can drop
/// the very first connection after "Done", and a corpus cold-boots targets
/// constantly.
pub fn status(addr: &str) -> io::Result<serde_json::Value> {
    match status_once(addr) {
        Ok(v) => Ok(v),
        Err(first) => {
            std::thread::sleep(Duration::from_millis(750));
            status_once(addr).map_err(|second| {
                io::Error::other(format!("status failed twice: {first}; then: {second}"))
            })
        }
    }
}

fn status_once(addr: &str) -> io::Result<serde_json::Value> {
    let (host, port) = split_addr(addr)?;
    let mut conn = Conn::connect(addr, Duration::from_secs(8))?;
    conn.set_read_timeout(Some(Duration::from_secs(8)))?;

    conn.send(ids::handshake::sb::INTENTION, &{
        let mut b = Vec::new();
        write_varint(&mut b, -1)?; // -1 = "just asking", for status
        write_string(&mut b, host)?;
        write_u16(&mut b, port)?;
        write_varint(&mut b, 1)?;
        b
    })?;
    conn.send(ids::status::sb::STATUS_REQUEST, &[])?;

    let (id, body) = conn.recv()?;
    if id != ids::status::cb::STATUS_RESPONSE {
        return Err(io::Error::other(format!(
            "expected status_response, got 0x{id:02X}"
        )));
    }
    let json_str = read_string(&mut body.as_slice())?;
    serde_json::from_str(&json_str).map_err(io::Error::other)
}

/// Full offline-mode login, through configuration, into play, and on until
/// the server says where the player is.
///
/// Returning at `play/login` is not enough: the join packet does not carry a
/// position, and a scenario with no anchor has no coordinates. So this waits
/// for the teleport that actually places the player, which is also the point
/// at which a real client stops showing a loading screen.
///
/// # Errors
/// On disconnect at any stage, or if the world never arrives.
pub fn login_to_play(addr: &str, name: &str) -> io::Result<Session> {
    let protocol = status(addr)?["version"]["protocol"]
        .as_i64()
        .ok_or_else(|| io::Error::other("status JSON missing version.protocol"))?
        as i32;

    let (host, port) = split_addr(addr)?;
    let mut conn = Conn::connect(addr, Duration::from_secs(8))?;
    conn.set_read_timeout(Some(Duration::from_secs(20)))?;

    conn.send(ids::handshake::sb::INTENTION, &{
        let mut b = Vec::new();
        write_varint(&mut b, protocol)?;
        write_string(&mut b, host)?;
        write_u16(&mut b, port)?;
        write_varint(&mut b, 2)?;
        b
    })?;
    conn.send(ids::login::sb::HELLO, &{
        let mut b = Vec::new();
        write_string(&mut b, name)?;
        write_u128(&mut b, offline_uuid(name))?;
        b
    })?;

    #[derive(PartialEq)]
    enum State {
        Login,
        Config,
        Play,
    }
    let mut state = State::Login;
    let mut chunks_seen: u32 = 0;
    let mut batches_seen: u32 = 0;
    let mut declared_commands: Vec<String> = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(90);

    while Instant::now() < deadline {
        let (id, body) = conn.recv()?;
        match state {
            State::Login => {
                if id == ids::login::cb::LOGIN_COMPRESSION {
                    conn.compression = Some(read_varint(&mut body.as_slice())?);
                } else if id == ids::login::cb::LOGIN_DISCONNECT {
                    let reason = read_string(&mut body.as_slice()).unwrap_or_default();
                    return Err(io::Error::other(format!("login disconnect: {reason}")));
                } else if id == ids::login::cb::LOGIN_FINISHED {
                    conn.send(ids::login::sb::LOGIN_ACKNOWLEDGED, &[])?;
                    // `client_information`, which a real client sends the
                    // instant it enters configuration. Vanilla tolerates its
                    // absence and rustmc does not, and the failure is
                    // silent from the client's side: configuration simply
                    // never finishes, the harness times out on its own read,
                    // and the server logs a truncated frame from the socket
                    // being closed under it. Two servers disagreeing about
                    // whether a packet is optional is exactly the kind of
                    // thing this harness exists to find, and the fix is to
                    // behave like a client rather than like the minimum one
                    // server happened to accept.
                    //
                    // Reference: `ServerboundClientInformationPacket` —
                    // language, view distance, chat visibility, chat colours,
                    // displayed skin parts, main hand, text filtering,
                    // allows listing, particle status.
                    let mut b = Vec::new();
                    write_string(&mut b, "en_gb")?;
                    write_u8(&mut b, 10)?; // view distance
                    write_varint(&mut b, 0)?; // chat visibility: full
                    write_bool(&mut b, true)?; // chat colours
                    write_u8(&mut b, 0x7f)?; // displayed skin parts
                    write_varint(&mut b, 1)?; // main hand: right
                    write_bool(&mut b, false)?; // text filtering
                    write_bool(&mut b, true)?; // allows listing
                    write_varint(&mut b, 0)?; // particle status: all
                    conn.send(ids::config::sb::CLIENT_INFORMATION, &b)?;
                    state = State::Config;
                }
            }
            State::Config => {
                if id == ids::config::cb::SELECT_KNOWN_PACKS {
                    // Claim zero known packs: the server then sends the full
                    // registry data inline, which we skip by frame, and the
                    // entry layout never has to be understood here.
                    let mut b = Vec::new();
                    write_varint(&mut b, 0)?;
                    conn.send(ids::config::sb::SELECT_KNOWN_PACKS, &b)?;
                } else if id == ids::config::cb::KEEP_ALIVE {
                    conn.send(ids::config::sb::KEEP_ALIVE, &body)?;
                } else if id == ids::config::cb::DISCONNECT {
                    return Err(io::Error::other(format!(
                        "disconnect during configuration: {}",
                        String::from_utf8_lossy(&body[..body.len().min(160)])
                    )));
                } else if id == ids::config::cb::FINISH_CONFIGURATION {
                    conn.send(ids::config::sb::FINISH_CONFIGURATION, &[])?;
                    state = State::Play;
                }
            }
            State::Play => {
                if id == ids::play::cb::COMMANDS {
                    declared_commands = crate::trace::command_roots(&body).0;
                }
                if id == ids::play::cb::PLAYER_POSITION {
                    let mut r = body.as_slice();
                    let tp = read_varint(&mut r)?;
                    let spawn = [read_f64(&mut r)?, read_f64(&mut r)?, read_f64(&mut r)?];
                    let mut b = Vec::new();
                    write_varint(&mut b, tp)?;
                    conn.send(ids::play::sb::ACCEPT_TELEPORTATION, &b)?;
                    // "I have finished loading." An empty packet, and
                    // without it vanilla silently drops movement, block
                    // actions and item use — eight handlers return early on
                    // `hasClientLoaded()`. Omitting it is why this harness
                    // spent a run measuring a player who was never where it
                    // thought and interactions that were never processed,
                    // and why it looked like vanilla declining to speak
                    // rather than like a packet we never sent.
                    conn.send(ids::play::sb::PLAYER_LOADED, &[])?;
                    #[allow(clippy::cast_possible_truncation)]
                    let anchor = [
                        spawn[0].floor() as i32,
                        spawn[1].floor() as i32,
                        spawn[2].floor() as i32,
                    ];
                    return Ok(Session {
                        conn,
                        spawn,
                        pos: spawn,
                        anchor,
                        seq: 1,
                        chunks_seen,
                        batches_seen,
                        yaw: 0.0,
                        declared_commands: std::mem::take(&mut declared_commands),
                        pitch: 0.0,
                        entities: Vec::new(),
                        menu: None,
                        last_teleport: tp,
                        closed: false,
                        expendable: false,
                        awaiting_loaded: false,
                        health: None,
                        sneaking: false,
                    });
                }
                if id == ids::play::cb::KEEP_ALIVE {
                    conn.send(ids::play::sb::KEEP_ALIVE, &body)?;
                } else if id == ids::play::cb::PING {
                    conn.send(ids::play::sb::PONG, &body)?;
                } else if id == ids::play::cb::LEVEL_CHUNK_WITH_LIGHT {
                    chunks_seen += 1;
                } else if id == ids::play::cb::CHUNK_BATCH_FINISHED {
                    // The very first batch is normally sent before the
                    // player position packet that ends this loop, so the
                    // acknowledgement has to exist here as well as in
                    // `liveness` — otherwise the stall begins before the
                    // session object does.
                    batches_seen += 1;
                    if chunk_ack_enabled() {
                        conn.send(
                            ids::play::sb::CHUNK_BATCH_RECEIVED,
                            &DESIRED_CHUNKS_PER_TICK,
                        )?;
                    }
                } else if id == ids::play::cb::DISCONNECT {
                    return Err(io::Error::other(format!(
                        "disconnected in play state: {}",
                        String::from_utf8_lossy(&body[..body.len().min(160)])
                    )));
                }
            }
        }
    }
    Err(io::Error::other(
        "timed out before the server placed the player",
    ))
}

impl Session {
    /// Answer the packets that keep a connection alive, and nothing else.
    ///
    /// A server correction is honoured here rather than ignored: if the
    /// server rubber-bands the player, the harness's idea of where it is
    /// standing has to follow, or every coordinate after that names a place
    /// the player is not and the whole scenario silently measures nothing.
    fn liveness(&mut self, id: i32, body: &[u8]) -> io::Result<()> {
        if id == ids::play::cb::COMMANDS {
            // Here rather than in the join loop alone. `sendCommands` runs
            // after the teleport in `PlayerList.placeNewPlayer`, so the join
            // loop returns on `PLAYER_POSITION` before this packet arrives
            // and the field would be empty on every real server -- which is
            // the failure mode a check built on it could not distinguish from
            // "the server declared nothing". Every drain routes through here,
            // so wherever in the join it lands, it is seen.
            //
            // Last one wins: vanilla re-sends the whole tree on a permission
            // change, and the current tree is the one worth reporting.
            self.declared_commands = crate::trace::command_roots(body).0;
        }
        if id == ids::play::cb::KEEP_ALIVE {
            self.conn.send(ids::play::sb::KEEP_ALIVE, body)?;
        } else if id == ids::play::cb::LEVEL_CHUNK_WITH_LIGHT {
            self.chunks_seen += 1;
        } else if id == ids::play::cb::CHUNK_BATCH_FINISHED {
            self.batches_seen += 1;
            if chunk_ack_enabled() {
                self.conn.send(
                    ids::play::sb::CHUNK_BATCH_RECEIVED,
                    &DESIRED_CHUNKS_PER_TICK,
                )?;
            }
        } else if id == ids::play::cb::PING {
            self.conn.send(ids::play::sb::PONG, body)?;
        } else if id == ids::play::cb::PLAYER_POSITION {
            let mut r = body;
            let tp = read_varint(&mut r)?;
            if let (Ok(x), Ok(y), Ok(z)) = (read_f64(&mut r), read_f64(&mut r), read_f64(&mut r)) {
                // `PositionMoveRotation.calculateAbsolute`, the position half:
                // an axis whose `Relative` bit (0..2, in the int that ends the
                // body) is set is an OFFSET from where the client is. Taking
                // it as a place put the client hundreds of blocks from its
                // body after a `/tp ~ ~ ~`, and every later walk was refused
                // by both servers — 2026-10-01, the first relative teleport
                // this harness was ever sent.
                let mask = body
                    .len()
                    .checked_sub(4)
                    .and_then(|i| body.get(i..))
                    .map_or(0, |t| i32::from_be_bytes([t[0], t[1], t[2], t[3]]));
                let v = [x, y, z];
                for (axis, value) in v.into_iter().enumerate() {
                    if mask & (1 << axis) != 0 {
                        self.pos[axis] += value;
                    } else {
                        self.pos[axis] = value;
                    }
                }
            }
            self.last_teleport = tp;
            let mut b = Vec::new();
            write_varint(&mut b, tp)?;
            self.conn.send(ids::play::sb::ACCEPT_TELEPORTATION, &b)?;
            if std::mem::take(&mut self.awaiting_loaded) {
                self.conn.send(ids::play::sb::PLAYER_LOADED, &[])?;
            }
        } else if id == ids::play::cb::RESPAWN {
            self.awaiting_loaded = true;
        } else if id == ids::play::cb::SET_HEALTH {
            let mut r = body;
            if let Ok(h) = read_f32(&mut r) {
                self.health = Some(h);
            }
        } else if id == ids::play::cb::ADD_ENTITY {
            // The leading two fields only, and then stop reading. The layout
            // is `ClientboundAddEntityPacket`'s own — id, then a 16-byte uuid,
            // then the type as a registry varint — and it is transcribed once
            // more here rather than shared with `trace::observe`'s arm because
            // that arm's job is to *normalise* the id and this one's is to
            // keep it raw. Sharing a reader would mean one of the two callers
            // getting the other's answer.
            //
            // A body too short to hold both fields is dropped rather than
            // guessed at: `trace::observe` records such a frame as
            // `packet:0x..` and a check comparing those still compares
            // something, whereas an entity recorded from a misread body would
            // be a target id nothing on the server answers to.
            let mut r = body;
            if let Ok(e) = read_varint(&mut r) {
                if r.len() >= 16 {
                    r = &r[16..];
                    if let Ok(t) = read_varint(&mut r) {
                        self.entities.push((e, t));
                    }
                }
            }
        } else if id == ids::play::cb::REMOVE_ENTITIES {
            // Removals are applied rather than ignored, so `newest_of_type`
            // cannot hand a scenario an entity the server has already told it
            // is gone. Check 530 walks a player out of tracking range and this
            // packet is how that arrives; without this, an `attack` after such
            // a walk would name a stale id and hit nothing, and both servers
            // hitting nothing compares equal.
            let mut r = body;
            if let Ok(n) = read_varint(&mut r) {
                let mut gone = Vec::new();
                for _ in 0..n.max(0) {
                    match read_varint(&mut r) {
                        Ok(e) => gone.push(e),
                        Err(_) => break,
                    }
                }
                self.entities.retain(|(e, _)| !gone.contains(e));
            }
        } else if id == ids::play::cb::OPEN_SCREEN {
            // `ClientboundOpenScreenPacket` is (containerId, menuType, title)
            // and only the first field is read here. `state_id` starts at 0
            // rather than being carried over from a previous menu: a fresh
            // `AbstractContainerMenu` is constructed with `stateId = 0`, and
            // the first `container_set_content` that follows will overwrite
            // this with whatever the server actually stamped.
            let mut r = body;
            if let Ok(container_id) = read_varint(&mut r) {
                self.menu = Some(OpenMenu {
                    container_id,
                    state_id: 0,
                });
            }
        } else if id == ids::play::cb::CONTAINER_SET_CONTENT
            || id == ids::play::cb::CONTAINER_SET_SLOT
        {
            // Both start `varint containerId, varint stateId`, which is all
            // that is read. The state is what the next click has to echo to
            // avoid provoking a whole-menu resend, so it is followed rather
            // than assumed to stay put.
            //
            // **Window 0 is skipped.** It is the player's own `InventoryMenu`,
            // which is never opened and never closed and whose state counter
            // runs for the life of the player — the corpus already sees it, in
            // `torch-falls-with-its-support` and the offhand swap. Letting it
            // overwrite an open container's state would make a click echo the
            // wrong menu's counter, and the punishment for a wrong state is not
            // an error but a silent switch to `broadcastFullState`.
            let mut r = body;
            if let (Ok(window), Ok(state)) = (read_varint(&mut r), read_varint(&mut r)) {
                if let Some(m) = self.menu.as_mut() {
                    if window == m.container_id && window != 0 {
                        m.state_id = state;
                    }
                }
            }
        } else if id == ids::play::cb::DISCONNECT {
            // **For a probe this packet IS the measurement.**
            //
            // `pump` calls this method before `trace::observe`, so returning
            // `Ok` here is what lets the disconnect be recorded as an
            // observation rather than thrown as a fault. The socket closes
            // immediately afterwards and `pump_expendable` absorbs the
            // hang-up.
            //
            // Gated on a flag set only by `record::act_on`, not on the packet:
            // an unexpected disconnect on the actor or the observer is still
            // the loud failure below, and it has earned that — see the note on
            // `printable_reason`, which exists because a whole brief was
            // written about this message while the reason was unreadable.
            if self.expendable {
                self.closed = true;
                return Ok(());
            }
            return Err(io::Error::other(format!(
                "the server disconnected us in play state: {}",
                printable_reason(body)
            )));
        }
        Ok(())
    }

    /// Walk to an absolute position and make it the new anchor.
    ///
    /// # Why scenarios do not share a place
    ///
    /// Nothing in this server or in vanilla undoes what a scenario did: a
    /// hole dug by one check is still a hole when the next one runs. Running
    /// every scenario at spawn would therefore make the corpus
    /// order-dependent, which is the one property a corpus must not have —
    /// and it would fail *intermittently*, which is worse than failing.
    ///
    /// So each scenario gets its own workspace, far enough apart that
    /// neither blocks nor entities from one can reach another. On superflat
    /// every column is identical, so moving costs nothing in fidelity: the
    /// hundredth workspace is the same situation as the first.
    ///
    /// The walk is broken into short hops because vanilla range-checks
    /// movement — a single long jump is "moved too quickly", and the
    /// rubber-band that follows would leave the player somewhere the
    /// scenario did not intend.
    ///
    /// # Errors
    /// On a socket error.
    pub fn goto(&mut self, target: [f64; 3]) -> io::Result<()> {
        // Half a block per packet, twenty packets a second — about ten
        // blocks a second, which is a speed vanilla accepts from a player.
        //
        // It used to be four blocks every 40ms, roughly a hundred blocks a
        // second, and **vanilla simply did not move the player**. It did not
        // always say so either: the correction, when it came at all, arrived
        // after this function had already decided it had arrived and taken
        // the anchor from its own belief. Vanilla's own console, asked where
        // the player was, answered spawn while the harness was reporting a
        // position 32 blocks away — and every scenario had been digging and
        // placing into empty air a workspace from the player ever since.
        //
        // That one bug invalidated a whole run's worth of conclusions. It is
        // the reason this function now walks at a legal speed, reads the
        // socket between every step so a correction is applied *during* the
        // walk rather than after it, and refuses to return until the server
        // agrees where the player is.
        const HOP: f64 = 0.5;
        // Enough for a long walk and not enough to hide a stuck one. At 50ms
        // a step this is thirty seconds; the previous 4000 was three
        // minutes of looking like slow progress rather than like a failure.
        const MAX_STEPS: usize = 600;

        // Convergence is measured in x and z only, and every step is sent at
        // whatever y the server currently believes.
        //
        // Insisting on a y is a fight the harness cannot win. A scenario digs
        // a hole beside the player; the walk to the next workspace crosses
        // it; the player falls one block; the walk keeps demanding the
        // original y, vanilla keeps putting it back on the ground, and the
        // loop runs until its step budget is gone — three minutes a
        // scenario, against twelve seconds for the same scenario run on
        // untouched ground. Ground height is the server's business.
        let mut steps = 0;
        loop {
            let d = [target[0] - self.pos[0], 0.0, target[2] - self.pos[2]];
            let dist = (d[0] * d[0] + d[2] * d[2]).sqrt();
            if dist < 1e-6 {
                break;
            }
            steps += 1;
            if steps > MAX_STEPS {
                return Err(io::Error::other(format!(
                    "gave up walking to ({:.1}, {:.1}, {:.1}) after {MAX_STEPS} steps —                      still {dist:.1} blocks away at ({:.1}, {:.1}, {:.1});                      the server is refusing the movement",
                    target[0], target[1], target[2], self.pos[0], self.pos[1], self.pos[2]
                )));
            }
            let f = (HOP / dist).min(1.0);
            let next = [self.pos[0] + d[0] * f, self.pos[1], self.pos[2] + d[2] * f];
            let mut b = Vec::new();
            for v in next {
                write_f64(&mut b, v)?;
            }
            write_u8(&mut b, 1)?; // on ground
            self.conn.send(ids::play::sb::MOVE_PLAYER_POS, &b)?;
            self.pos = next;
            // Read between steps, so a correction moves `pos` back and the
            // loop walks from where the server actually put us. Without
            // this the walk is open-loop and its own idea of position is
            // fiction.
            self.drain(Duration::from_millis(50))?;
        }

        // Settle **before** deciding we arrived. A correction that lands
        // after the check is a correction the check did not see, which is
        // exactly how the old version passed while being 32 blocks wrong.
        self.settle(Duration::from_millis(250), Duration::from_millis(3000))?;

        // x and z only, for the same reason the walk ignores y: where the
        // ground is, is the server's answer and not the scenario's.
        let drift = [0usize, 2]
            .into_iter()
            .map(|i| (target[i] - self.pos[i]).abs())
            .fold(0.0_f64, f64::max);
        if drift > 0.75 {
            return Err(io::Error::other(format!(
                "walked to ({:.1}, {:.1}, {:.1}) but the server put us at \
                 ({:.1}, {:.1}, {:.1}) — {drift:.1} blocks adrift",
                target[0], target[1], target[2], self.pos[0], self.pos[1], self.pos[2]
            )));
        }
        #[allow(clippy::cast_possible_truncation)]
        {
            self.anchor = [
                self.pos[0].floor() as i32,
                self.pos[1].floor() as i32,
                self.pos[2].floor() as i32,
            ];
        }
        Ok(())
    }

    /// Walk to an absolute position **without reading the socket**.
    ///
    /// Returns how many hops it took. The caller is expected to pump
    /// afterwards; until it does, inbound frames queue up, which is exactly
    /// what is wanted — none of them are lost, they are merely late.
    ///
    /// # This walked too fast for vanilla until 2026-08-04, and [`Self::goto`]
    /// beside it already knew
    ///
    /// `HOP` was **8.0**, and the comment where the sleep is said a hop was
    /// "under its per-packet bound". It is not, and vanilla says so in its
    /// own log:
    ///
    /// ```text
    ///   [Server thread/WARN]: oracle moved too quickly! 0.0,0.0,16.0
    /// ```
    ///
    /// Sixteen, not eight: the packets leave here every 30 ms and a tick is
    /// 50, so two hops routinely reach the server against one `lastGood`
    /// position and the delta it tests is their sum.
    /// `ServerGamePacketListenerImpl.handleMovePlayer` compares the claimed
    /// squared delta against `100.0f * i` — `i` being the packets received
    /// since the last tick, **and reset to 1 whenever it exceeds 5** — so
    /// `16² = 256` against `100 * 2` is refused, the player is teleported
    /// back, and every later hop is refused too because a measured step never
    /// reads the socket and so never sends the `accept_teleportation` the
    /// server is waiting for.
    ///
    /// The whole of a walk past about 16 blocks was therefore fiction. It
    /// cost `080-other-player-is-an-entity` two rewrites — its comment blames
    /// "vanilla rubber-bands a player who covers ground that fast" without
    /// the cause — and it made the first run of
    /// `530-walking-away-drops-a-standing-player-from-view` report that
    /// vanilla does not stop tracking a player 80 blocks away, when vanilla
    /// had never seen the actor further than 24.
    ///
    /// **[`Self::goto`] had already been through this and fixed it at 0.5
    /// blocks a packet**, with the story written out above its own `HOP`. The
    /// correction lived in the function it corrected and did not travel to
    /// the one next to it, which is the same shape as the gamerule ids in
    /// `docs/AGENT-BRIEF.md`.
    ///
    /// # Why four and not one, which is the second half of the same lesson
    ///
    /// The bound gives a **window**, not a point, and the first repair took
    /// the small end of it — one block a packet — which made the walk legal
    /// and broke `080-other-player-is-an-entity`, a check that had been
    /// `match` since the tracker landed:
    ///
    /// ```text
    ///   move_entity  vanilla d = [12288, 0, 0]   ours d = [8192, 0, 0]
    /// ```
    ///
    /// Three blocks against two. 080 walks four blocks; at `HOP = 8` that
    /// was **one** packet and neither server could split it, and at
    /// `HOP = 1` it is four packets that the two servers' `sendChanges`
    /// happened to divide differently. Nothing about either server changed.
    /// **More packets for the same walk buys no fidelity and makes every
    /// observation of it phase-sensitive**, so within the legal window the
    /// hop wants to be as large as it can be, not as small.
    ///
    /// Four is the largest that keeps the routine folds comfortable. `n`
    /// folded hops claim `4n` blocks against `100 * i`:
    ///
    /// ```text
    ///   n = 2   64 vs 200      n = 3   144 vs 300     n = 5   400 vs 500
    ///   n = 6   576 vs 100     — the clamp, and 180 ms of server stall
    /// ```
    ///
    /// where eight blocks a packet failed at `n = 2`, which is routine at 30
    /// ms a packet against a 50 ms tick. It also leaves 080 at a single hop,
    /// which is the state it was measured `match` in.
    ///
    /// # Errors
    /// On a socket error.
    /// `on_ground` is the flag byte every packet of the walk carries. See
    /// `crate::scenario::Step::Walk` for why one bit is the whole cost of a
    /// jump here, and `ServerEntity.sendChanges` offsets 680..691 for the arm
    /// it reaches.
    pub fn hop_to(&mut self, target: [f64; 3], on_ground: bool) -> io::Result<u64> {
        const HOP: f64 = 4.0;
        let mut hops = 0;
        loop {
            let d = [
                target[0] - self.pos[0],
                target[1] - self.pos[1],
                target[2] - self.pos[2],
            ];
            let dist = d.iter().map(|v| v * v).sum::<f64>().sqrt();
            if dist < 1e-6 {
                return Ok(hops);
            }
            let f = (HOP / dist).min(1.0);
            let next = [
                self.pos[0] + d[0] * f,
                self.pos[1] + d[1] * f,
                self.pos[2] + d[2] * f,
            ];
            let mut b = Vec::new();
            for v in next {
                write_f64(&mut b, v)?;
            }
            // `ServerboundMovePlayerPacket`'s trailing byte is **flags**,
            // not a bool: bit 0 is `FLAG_ON_GROUND` and bit 1 is
            // `FLAG_HORIZONTAL_COLLISION`. A walk never claims a collision,
            // so clearing the ground bit leaves the byte at 0.
            write_u8(&mut b, u8::from(on_ground))?;
            self.conn.send(ids::play::sb::MOVE_PLAYER_POS, &b)?;
            self.pos = next;
            hops += 1;
            // Spacing alone cannot make a walk legal — see the header. The
            // hop size is what does it; this only keeps the queue short.
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Read and discard for a while, answering liveness.
    ///
    /// # Errors
    /// On a socket error.
    pub fn drain(&mut self, how_long: Duration) -> io::Result<()> {
        let until = Instant::now() + how_long;
        while Instant::now() < until {
            if let Some((id, body)) = self.conn.recv_poll(POLL, PATIENCE)? {
                self.liveness(id, &body)?;
            }
        }
        Ok(())
    }

    /// Read and discard until the server has been silent for a while.
    ///
    /// A fixed sleep is a guess about how long a server takes, and the two
    /// servers here do not take the same time for anything. Waiting for
    /// *quiet* instead means the measured window starts when the world has
    /// actually settled, so a packet caused by rig-building cannot land
    /// inside it and be attributed to the first real step. That has already
    /// happened once: a scenario whose setup placed a block showed the
    /// block's own sounds and updates among its measured observations.
    ///
    /// # Errors
    /// On a socket error.
    pub fn settle(&mut self, quiet: Duration, cap: Duration) -> io::Result<()> {
        let deadline = Instant::now() + cap;
        let mut last = Instant::now();
        while Instant::now() < deadline {
            if let Some((id, body)) = self.conn.recv_poll(POLL, PATIENCE)? {
                self.liveness(id, &body)?;
                // Only true liveness is discounted. Chunk streaming counts
                // as the world still arriving: discounting it let a measured
                // window open while the client's chunks were still landing,
                // and a client that has not received a chunk is told nothing
                // about the blocks in it — which reads exactly like a server
                // that said nothing.
                if !crate::trace::is_liveness(id) {
                    last = Instant::now();
                }
            } else if last.elapsed() >= quiet {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Absolute position of an anchor-relative offset.
    #[must_use]
    pub const fn at(&self, rel: [i32; 3]) -> [i32; 3] {
        [
            self.anchor[0] + rel[0],
            self.anchor[1] + rel[1],
            self.anchor[2] + rel[2],
        ]
    }

    /// `BlockPos.asLong`, the packing every position on the wire uses.
    fn pack(pos: [i32; 3]) -> i64 {
        (i64::from(pos[0]) & 0x3FF_FFFF) << 38
            | (i64::from(pos[2]) & 0x3FF_FFFF) << 12
            | (i64::from(pos[1]) & 0xFFF)
    }

    /// Listen until `until`, recording what arrives.
    ///
    /// Recording and liveness are the same loop on purpose: a harness that
    /// stops answering keep-alives while it waits for a scheduled tick gets
    /// disconnected at exactly the interesting moment, which is how the
    /// coral check failed the first time it was written anywhere.
    #[allow(clippy::too_many_arguments)]
    pub fn pump(
        &mut self,
        who: crate::trace::Who,
        until: Instant,
        norm: &mut Normalizer,
        states: &BTreeMap<i32, String>,
        items: &BTreeMap<i32, String>,
        step: usize,
        started: Instant,
        out: &mut Vec<Obs>,
    ) -> io::Result<()> {
        while Instant::now() < until {
            // `recv_poll`, never `recv` with a short timeout: the poll
            // interval applies to the first byte of a frame only, so a big
            // packet cannot be cut in half and misread as the next one.
            if let Some((id, body)) = self.conn.recv_poll(POLL, PATIENCE)? {
                self.liveness(id, &body)?;
                let at_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                for (kind, fields) in trace::observe_all(norm, states, items, id, &body) {
                    out.push(Obs {
                        who,
                        step,
                        at_ms,
                        kind,
                        fields,
                    });
                }
            }
        }
        Ok(())
    }

    /// True for the errors a socket produces when the far side has hung up.
    ///
    /// Named and narrow. A blanket "any error means the server disconnected
    /// us" would swallow a decode failure or a short read and report a broken
    /// harness as a measured divergence, which is this project's signature
    /// defect wearing a network hat.
    fn is_hangup(e: &io::Error) -> bool {
        matches!(
            e.kind(),
            io::ErrorKind::UnexpectedEof
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::ConnectionAborted
                | io::ErrorKind::BrokenPipe
        )
    }

    /// [`Session::pump`] for a connection the scenario is allowed to lose.
    ///
    /// The `ClientboundDisconnectPacket` itself arrives *before* the socket
    /// closes and is recorded by the `pump` inside — so the measurement is the
    /// packet, and the hang-up that follows is only how the sentence ends.
    /// A server that says nothing and keeps the connection open records
    /// nothing, which is exactly the divergence a probe check is written to
    /// see.
    ///
    /// # Errors
    /// On any socket error that is not a hang-up.
    #[allow(clippy::too_many_arguments)]
    pub fn pump_expendable(
        &mut self,
        who: crate::trace::Who,
        until: Instant,
        norm: &mut Normalizer,
        states: &BTreeMap<i32, String>,
        items: &BTreeMap<i32, String>,
        step: usize,
        started: Instant,
        out: &mut Vec<Obs>,
    ) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        match self.pump(who, until, norm, states, items, step, started, out) {
            Err(e) if Self::is_hangup(&e) => {
                self.closed = true;
                Ok(())
            }
            other => other,
        }
    }

    /// [`Session::settle`] for a connection the scenario is allowed to lose.
    ///
    /// # Errors
    /// On any socket error that is not a hang-up.
    pub fn settle_expendable(&mut self, quiet: Duration, cap: Duration) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        match self.settle(quiet, cap) {
            Err(e) if Self::is_hangup(&e) => {
                self.closed = true;
                Ok(())
            }
            other => other,
        }
    }

    /// Acknowledge the last teleport a second time.
    ///
    /// `handleAcceptTeleportPacket` clears `awaitingPositionFromClient` at
    /// offset 125 and leaves `awaitingTeleport` alone, so this packet matches
    /// the id at offset 20, finds the position already taken at 27, and
    /// vanilla disconnects. See [`crate::scenario::Step::ProbeAckTeleportAgain`].
    ///
    /// A hang-up here is recorded rather than raised for
    /// [`Session::pump_expendable`]'s reason: the server may already have gone
    /// on a previous step.
    ///
    /// # Errors
    /// On a socket error that is not a hang-up.
    pub fn ack_teleport_again(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        let mut b = Vec::new();
        write_varint(&mut b, self.last_teleport)?;
        match self.conn.send(ids::play::sb::ACCEPT_TELEPORTATION, &b) {
            Err(e) if Self::is_hangup(&e) => {
                self.closed = true;
                Ok(())
            }
            other => other,
        }
    }

    /// Listen until `until` for exactly one packet of `id`, answering
    /// liveness on everything else that arrives meanwhile.
    ///
    /// For `ci/dump-recipe-unlocks.sh`: a `hold` can sit for over a second
    /// waiting for `recipe_book_add`, and this connection is reused for
    /// hundreds of holds in one run, so keep-alives have to be answered
    /// between them — `drain`'s own loop, aimed at one packet id instead of
    /// everything. Returns `None` on timeout, never an error: silence is the
    /// expected answer for a candidate item that unlocks nothing.
    ///
    /// # Errors
    /// On a socket error.
    pub fn capture(&mut self, id_wanted: i32, until: Instant) -> io::Result<Option<Vec<u8>>> {
        while Instant::now() < until {
            if let Some((id, body)) = self.conn.recv_poll(POLL, PATIENCE)? {
                self.liveness(id, &body)?;
                if id == id_wanted {
                    return Ok(Some(body));
                }
            }
        }
        Ok(None)
    }

    /// Put this player back into the state a fresh login leaves it in: every
    /// `InventoryMenu` slot empty, hotbar slot 0 selected, facing yaw 0 and
    /// pitch 0. Sent unmeasured, before a scenario's setup.
    ///
    /// # Why every scenario, and why all of it
    ///
    /// Every scenario in a run shares one connection, so whatever a scenario
    /// leaves on the player is inherited by the next one. `run_one` already
    /// resets the open menu, the held sneak key and the position; the
    /// inventory, the selected slot and the facing were not reset, so a
    /// scenario's situation depended on which scenarios ran before it.
    /// Measured 2026-09-30 on the 2026-09-24 corpus-parallel recordings
    /// (`~/rustmc-cpar-a` serial against `~/rustmc-cpar-b` 4 shards):
    /// `607-container-click-pickup-all-gathers-matching-stacks` saw a stone
    /// in `InventoryMenu` slot 44 left by an earlier scenario when run
    /// serially and an empty slot when it opened a shard, and its
    /// `container_set_content` differed on **both** servers; `530` and `788`
    /// saw a different `update_attributes` digest on both, which is what a
    /// different item in the main hand is. A corpus split into shards gives
    /// each shard a different predecessor set, so this is what makes a
    /// sharded run measure the same thing as a serial one — and a serial
    /// run's scenario stop depending on the order of its neighbours.
    ///
    /// # Inert where there is nothing to reset
    ///
    /// `ServerGamePacketListenerImpl.handleSetCreativeModeSlot` (javap
    /// offsets 59..157) accepts slots 1..45 and an empty stack, calls
    /// `Slot.setByPlayer`, then `setRemoteSlot` with the same stack and
    /// `broadcastChanges` — so the client is already believed to hold what
    /// it sent and an empty slot set empty echoes nothing. Slot 0, the
    /// crafting result, is outside that range and is never sent.
    pub fn reset_player_state(&mut self) -> io::Result<()> {
        for slot in 1u16..=45 {
            let mut b = Vec::new();
            write_u16(&mut b, slot)?;
            write_varint(&mut b, 0)?; // `ItemStack.OPTIONAL_*_STREAM_CODEC`: count 0 is EMPTY
            self.conn.send(ids::play::sb::SET_CREATIVE_MODE_SLOT, &b)?;
        }
        let mut b = Vec::new();
        write_u16(&mut b, 0)?;
        self.conn.send(ids::play::sb::SET_CARRIED_ITEM, &b)?;
        self.yaw = 0.0;
        self.pitch = 0.0;
        let mut b = Vec::new();
        write_f32(&mut b, 0.0)?;
        write_f32(&mut b, 0.0)?;
        write_u8(&mut b, 1)?;
        self.conn.send(ids::play::sb::MOVE_PLAYER_ROT, &b)
    }

    /// Perform one step. Returns how long to keep listening afterwards.
    ///
    /// # Errors
    /// On a socket error, or a step naming something the extract does not
    /// have — an unknown item or an unknown face are harness bugs and are
    /// reported as such, never as a server divergence.
    pub fn act(&mut self, step: &Step, tables: &Tables) -> io::Result<Duration> {
        match step {
            Step::Hold {
                item,
                slot,
                components,
            } => {
                let id = *tables.items.get(item).ok_or_else(|| {
                    io::Error::other(format!("no item called {item} in the extract"))
                })?;
                // `set_creative_mode_slot`: the slot is an InventoryMenu
                // index, so hotbar 0 is 36 (`USE_ROW_SLOT_START`). Reading it
                // as a hotbar index puts the item somewhere the player can
                // never select, and every later placement silently does
                // nothing.
                let mut b = Vec::new();
                write_u16(&mut b, (36 + slot) as u16)?;
                write_varint(&mut b, 1)?; // count
                write_varint(&mut b, id)?;
                write_component_patch(&mut b, components, tables)?;
                self.conn.send(ids::play::sb::SET_CREATIVE_MODE_SLOT, &b)?;

                let mut b = Vec::new();
                write_u16(&mut b, u16::try_from(*slot).unwrap_or(0))?;
                self.conn.send(ids::play::sb::SET_CARRIED_ITEM, &b)?;
                // **250 ms, not 60 — a step's window has to outlast the
                // reference's tick.** This was 60 ms, and 60 ms is shorter
                // than one vanilla tick plus a round trip, so *anything*
                // vanilla says in answer to a hold was a coin flip between
                // this step and the next one.
                //
                // Measured, 2026-08-09, over the six full-corpus recordings
                // sitting on the box, all of `stairs-and-furnace-face-apart`,
                // all carrying the same `packet:0x4A` digest
                // `7a679a94549f118e`: the hold is issued at t = 321 ms and
                // vanilla's `recipe_book_add` arrives at 322, 333, 335, 345,
                // 381 and 383 ms — latencies of 1, 12, 14, 24, 60 and 62 ms,
                // a 61 ms spread that is one 50 ms tick of phase plus the
                // wire. The old boundary sat at 381 ms, *inside* that spread:
                // five of the six landed in the hold's own step and the sixth
                // landed in the next one, and which you got was the phase of
                // vanilla's tick loop against the recorder's clock. Our own
                // side answers on the network thread and came in at 317, 329,
                // 329 and 330 ms every time, so the disagreement was entirely
                // the reference's jitter crossing a boundary drawn too close.
                //
                // **No server-side change could have fixed it.** Vanilla's
                // tick phase is neither observable to the client nor
                // controlled by it, so even a subject that quantised its own
                // answers to 50 ms would race the reference. The fragility is
                // in the window, which is the only place it can be removed.
                //
                // 250 ms is what `place`, `dig`, `use` and the rest already
                // wait, so this makes `hold` ordinary rather than special: it
                // is five ticks against a measured worst case of 62 ms, with
                // enough left over to survive a loaded box. It does not widen
                // any projection — the packet must still appear, at the same
                // step, with the same digest, on both sides. It costs 146
                // hold steps x 190 ms = ~28 s per recording.
                //
                // `Step::Look` below carried the identical 60 ms and was left
                // alone here, because no scenario had been measured failing
                // on it and this change was gated on one corpus run. It has
                // since been measured — `ci/step-window-audit.py`, nine
                // recordings, 2026-08-09 — and repaired to 250 ms for a
                // reason of its own rather than by analogy with this one. The
                // reading that settles it is in that arm.
                Ok(Duration::from_millis(250))
            }
            Step::Look { yaw, pitch } => {
                self.yaw = *yaw;
                self.pitch = *pitch;
                // `ServerboundMovePlayerPacket.Rot` — yaw, pitch, flags.
                let mut b = Vec::new();
                write_f32(&mut b, *yaw)?;
                write_f32(&mut b, *pitch)?;
                write_u8(&mut b, 1)?;
                self.conn.send(ids::play::sb::MOVE_PLAYER_ROT, &b)?;
                // **250 ms, not 60 — measured, not copied.** This was 60 ms,
                // the same constant `Step::Hold` carried, and
                // `docs/PARITY-ROADMAP.md` §12 left it open on the stated
                // ground that nobody had measured what it should be.
                //
                // Measured 2026-08-09 with `ci/step-window-audit.py` over the
                // nine full-corpus recordings on the box, every one of them
                // recorded at `look = 60` so the ensemble spans no change to
                // this constant (the script checks that itself and refuses
                // otherwise). The corpus holds six look steps across five
                // scenarios; four of them answer at all, and the two that
                // do not are looks to yaw 0.0 -- the yaw the player already
                // holds, a zero-degree change, below the one-step deadzone
                // in `Mth.packDegrees` and never sent. Two
                // answer kinds each: the reference's answer arrives between
                // **32 ms and 192 ms** after the look, and **45 of 71
                // arrivals landed outside the 60 ms window that was supposed
                // to contain them**. Six of the eight cells flip between the
                // look's own step and the next one from run to run; the other
                // two — `a-bed-facing-north`'s `rotate_head` and the second
                // look in `stairs-make-a-corner-when-two-meet` — are late in
                // **nine runs out of nine**, which no stability test can see.
                //
                // The bytecode says why, and bounds it at five ticks.
                // `handleMovePlayer` goes through
                // `PacketUtils.ensureRunningOnSameThread`, so the rotation is
                // applied at the head of a tick, not on the netty thread:
                // up to 1 tick. `ServerEntity.sendChanges` gates the
                // broadcast — `$Rot` and `RotateHead` alike, both inside it —
                // behind `tickCount % updateInterval == 0`, and
                // `EntityType.PLAYER` declares `updateInterval(2)`: up to 2
                // more ticks. `RotateHead` is measured a further ~100 ms
                // behind `$Rot`, one more gated interval, because the head
                // rotation has to pass its own packed-degree deadzone against
                // `lastSentYHeadRot`. 1 + 2 + 2 = 5 ticks = 250 ms, and the
                // measured worst case of 192 ms sits inside that.
                //
                // So 250 ms is not a round number borrowed from `hold`; it is
                // the mechanism's own bound, and it happens to be what
                // `place`, `dig`, `use` and the rest already return, which
                // makes `look` ordinary rather than special.
                //
                // **This moves no corpus score today and that is measured,
                // not hoped.** All five look-bearing scenarios declare
                // `observe_from = "actor"`, and a pure rotation reaches the
                // actor not at all: `ChunkMap$TrackedEntity.updatePlayer`
                // returns early on `player == this.entity`, so the mover is
                // never in `seenBy`. Every flipping cell is therefore outside
                // the declared *and* the strict projection — `strict` keeps
                // `observe_from` too. The fragility is real, measured, and
                // currently unable to fail a gate; it becomes able to the
                // first time a look scenario observes the observer, and seven
                // scenarios in the corpus already do observe one.
                Ok(Duration::from_millis(250))
            }
            Step::Walk { to, on_ground } => {
                // Anchor-relative, and the anchor deliberately does **not**
                // move: a scenario that walks away from its rig still names
                // the rig by the same coordinates.
                //
                // This sends and never reads, unlike `goto`. A measured step
                // must not consume the socket, because whatever it consumed
                // it would also **discard** — and the packets a walk causes
                // are exactly the ones a walk is usually there to provoke.
                // That bug made `other-player-is-an-entity` report vacuous
                // twice: the player was removed and re-added on schedule and
                // the harness threw both away, then reported that vanilla
                // had said nothing.
                let target = [
                    to[0] + f64::from(self.anchor[0]),
                    to[1] + f64::from(self.anchor[1]),
                    to[2] + f64::from(self.anchor[2]),
                ];
                let hops = self.hop_to(target, *on_ground)?;
                // Long enough for the walk itself plus what it caused; the
                // caller pumps for this, so nothing is lost meanwhile —
                // inbound frames simply queue in the socket.
                Ok(Duration::from_millis(400 + 20 * hops))
            }
            Step::Lurch { to, on_ground } => {
                // One packet, at whatever distance the scenario asked for.
                // Deliberately not `hop_to`: the point is to hand the server
                // a delta its own speed check can refuse, and `hop_to`'s cap
                // exists to guarantee it never does.
                let target = [
                    to[0] + f64::from(self.anchor[0]),
                    to[1] + f64::from(self.anchor[1]),
                    to[2] + f64::from(self.anchor[2]),
                ];
                let mut b = Vec::new();
                for v in target {
                    write_f64(&mut b, v)?;
                }
                // Flags, not a bool — bit 0 is `FLAG_ON_GROUND`.
                write_u8(&mut b, u8::from(*on_ground))?;
                self.conn.send(ids::play::sb::MOVE_PLAYER_POS, &b)?;
                // The client believes it is there. If the server refuses,
                // `liveness` overwrites this from the correction packet and
                // acknowledges it, which is what a real client does and what
                // keeps the connection usable for the next step.
                self.pos = target;
                // Two ticks of server time plus the round trip. Shorter than
                // a walk's because there is one packet, longer than a look's
                // because the answer has to come back.
                Ok(Duration::from_millis(500))
            }
            Step::Dig { at, face } | Step::FinishDig { at, face } => {
                // Named, not computed. `STOP_DESTROY_BLOCK` is 2; this used
                // to send 1, which is `ABORT_DESTROY_BLOCK` — see
                // `scenario::player_action`.
                let action = if matches!(step, Step::FinishDig { .. }) {
                    crate::scenario::player_action::STOP_DESTROY_BLOCK
                } else {
                    crate::scenario::player_action::START_DESTROY_BLOCK
                };
                let face = face_value(face)
                    .ok_or_else(|| io::Error::other(format!("no face called {face}")))?;
                let mut b = Vec::new();
                write_varint(&mut b, action)?;
                write_i64(&mut b, Self::pack(self.at(*at)))?;
                write_u8(&mut b, face)?;
                write_varint(&mut b, self.seq)?;
                self.seq += 1;
                self.conn.send(ids::play::sb::PLAYER_ACTION, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::Place { on, face, cursor } => {
                let face = face_value(face)
                    .ok_or_else(|| io::Error::other(format!("no face called {face}")))?;
                let cursor = cursor.unwrap_or_else(|| face_cursor(face));
                let mut b = Vec::new();
                write_varint(&mut b, 0)?; // main hand
                write_i64(&mut b, Self::pack(self.at(*on)))?;
                write_varint(&mut b, i32::from(face))?;
                for v in cursor {
                    write_f32(&mut b, v)?;
                }
                write_bool(&mut b, false)?; // inside block
                write_bool(&mut b, false)?; // hit world border
                write_varint(&mut b, self.seq)?;
                self.seq += 1;
                self.conn.send(ids::play::sb::USE_ITEM_ON, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::UseItem => {
                let mut b = Vec::new();
                write_varint(&mut b, 0)?; // main hand
                write_varint(&mut b, self.seq)?;
                self.seq += 1;
                // The rotation the last `look` set, not zero. Vanilla
                // ray-traces `use_item` from these.
                write_f32(&mut b, self.yaw)?;
                write_f32(&mut b, self.pitch)?;
                self.conn.send(ids::play::sb::USE_ITEM, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::ReleaseUse => {
                // `handlePlayerAction` case 5 -> offset 260 ->
                // `player.releaseUsingItem()` -> `return`. It never reaches
                // `handleBlockBreakAction` at 268, so it names no block, is
                // not acknowledged, and must not consume a sequence number —
                // which is exactly `bare_player_action`'s contract.
                self.bare_player_action(crate::scenario::player_action::RELEASE_USE_ITEM)?;
                Ok(Duration::from_millis(250))
            }
            Step::Drop { all } => {
                let action = if *all {
                    crate::scenario::player_action::DROP_ALL_ITEMS
                } else {
                    crate::scenario::player_action::DROP_ITEM
                };
                self.bare_player_action(action)?;
                Ok(Duration::from_millis(250))
            }
            Step::SwapOffhand => {
                self.bare_player_action(crate::scenario::player_action::SWAP_ITEM_WITH_OFFHAND)?;
                Ok(Duration::from_millis(250))
            }
            Step::Attack { target } => {
                let type_id = *tables.entity_types.get(target).ok_or_else(|| {
                    io::Error::other(format!("no entity type called {target} in the extract"))
                })?;
                // **Refuse rather than send.** An unresolved target sent as
                // entity 0 is a well-formed packet that hits nothing; vanilla
                // would say nothing, this server would say nothing, and the
                // comparison would call that a match on a check that never
                // attacked anything. So this is an error, and `act`'s contract
                // already says a step naming something the harness cannot
                // resolve is a harness bug and never a server divergence.
                let entity = newest_of_type(&self.entities, type_id).ok_or_else(|| {
                    io::Error::other(format!(
                        "attack names {target} (type {type_id}) but this client has been \
                         told of no live entity of that type — it knows {:?}. An attack \
                         on nothing would look exactly like agreement.",
                        self.entities
                    ))
                })?;
                // A real client left-clicking an entity sends the arm swing
                // too, and vanilla's `handleAttack` does not send it for you.
                // Omitting it makes every combat check compare a world in
                // which nobody visibly swung.
                let mut b = Vec::new();
                write_varint(&mut b, 0)?; // main hand
                self.conn.send(ids::play::sb::SWING, &b)?;

                // `ServerboundAttackPacket` is one varint and nothing else.
                let mut b = Vec::new();
                write_varint(&mut b, entity)?;
                self.conn.send(ids::play::sb::ATTACK, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::Interact { target, pick, hand } => {
                let type_id = *tables.entity_types.get(target).ok_or_else(|| {
                    io::Error::other(format!("no entity type called {target} in the extract"))
                })?;
                let hand_id = crate::scenario::hand_ordinal(hand)
                    .ok_or_else(|| io::Error::other(format!("no InteractionHand called {hand}")))?;
                // Refused rather than sent, for `attack`'s reason: an interact
                // on an entity id nothing has is a well-formed packet that
                // `getEntityOrPart` answers with null, and both servers would
                // agree in silence.
                let entity = match pick.as_str() {
                    "newest" => newest_of_type(&self.entities, type_id),
                    "oldest" => oldest_of_type(&self.entities, type_id),
                    other => {
                        return Err(io::Error::other(format!(
                            "interact pick {other:?} — the two are \"newest\" and \"oldest\""
                        )))
                    }
                }
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "interact names {target} (type {type_id}) but this client has been \
                         told of no live entity of that type — it knows {:?}. An interact \
                         with nothing would look exactly like agreement.",
                        self.entities
                    ))
                })?;
                self.conn.send(
                    ids::play::sb::INTERACT,
                    &interact_body(entity, hand_id, self.sneaking)?,
                )?;
                Ok(Duration::from_millis(250))
            }
            Step::Sneak { on } => {
                // `Input.STREAM_CODEC` (`Input$1.encode`) ors seven flags into
                // one byte and calls `writeByte` once. Only `FLAG_SHIFT` is
                // ever set here, and the other six stay clear because they are
                // honestly clear: this harness walks by sending
                // `MOVE_PLAYER_POS` directly, never by holding a movement key,
                // so a set `FLAG_FORWARD` would be a claim about the client
                // that nothing else in the harness backs up.
                //
                //   FLAG_FORWARD  = 1     FLAG_JUMP   = 16
                //   FLAG_BACKWARD = 2     FLAG_SHIFT  = 32
                //   FLAG_LEFT     = 4     FLAG_SPRINT = 64
                //   FLAG_RIGHT    = 8
                let mut b = Vec::new();
                write_u8(
                    &mut b,
                    if *on {
                        crate::scenario::input_flags::SHIFT
                    } else {
                        0
                    },
                )?;
                self.conn.send(ids::play::sb::PLAYER_INPUT, &b)?;
                self.sneaking = *on;
                // Long enough for the flag to be set before the next step
                // reads it. `setShiftKeyDown` happens on the packet thread via
                // `ensureRunningOnSameThread`, so this is a tick's grace and
                // not a guess at a network delay.
                Ok(Duration::from_millis(120))
            }
            Step::ContainerClick {
                slot,
                button,
                input,
            } => {
                let input_id = crate::scenario::container_input_value(input).ok_or_else(|| {
                    io::Error::other(format!(
                        "no ContainerInput called {input} — the seven are pickup, \
                         quick_move, swap, clone, throw, quick_craft, pickup_all. \
                         Refused rather than defaulted: vanilla's own id mapper \
                         resolves an unknown value to PICKUP, so a typo would \
                         click and be answered."
                    ))
                })?;
                // **Refuse rather than send**, for `attack`'s reason. A click
                // at a `containerId` no menu is using is silently dropped by
                // `handleContainerClick`'s first branch — no reply, no log —
                // so a scenario that clicked before opening anything would
                // record two servers saying nothing and report `match`.
                let menu = self.menu.ok_or_else(|| {
                    io::Error::other(format!(
                        "container_click names slot {slot} but this client has \
                         been told of no open menu. A click carries the menu's \
                         own containerId and vanilla drops a click that names \
                         the wrong one without answering, so this would have \
                         been a scenario that measured silence against silence."
                    ))
                })?;
                let slot_num = i16::try_from(*slot).map_err(|_| {
                    io::Error::other(format!("slot {slot} does not fit the packet's i16"))
                })?;

                let mut b = Vec::new();
                write_varint(&mut b, menu.container_id)?;
                write_varint(&mut b, menu.state_id)?;
                // `ByteBufCodecs.SHORT` is `ByteBuf.writeShort` — two bytes,
                // big-endian, signed. Not a varint, and not the four bytes
                // `ByteBufCodecs.INT` writes, which is the neighbouring codec
                // in the same static initialiser.
                write_u16(&mut b, slot_num as u16)?;
                write_u8(&mut b, *button as u8)?;
                write_varint(&mut b, input_id)?;
                // `changedSlots`: an empty map, one varint. **Deliberate, and
                // it is what makes this family of checks able to fail** — see
                // `Step::ContainerClick`'s own documentation. A correct
                // prediction is answered with silence.
                write_varint(&mut b, 0)?;
                // `carriedItem`: `HashedStack.EMPTY`, which is
                // `ByteBufCodecs.optional`'s absent form — a single false.
                write_bool(&mut b, false)?;
                self.conn.send(ids::play::sb::CONTAINER_CLICK, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::ContainerButtonClick { button } => {
                let menu = self.menu.ok_or_else(|| {
                    io::Error::other(format!(
                        "container_button_click names button {button} but this \
                         client has been told of no open menu; vanilla drops a \
                         press naming the wrong container without answering"
                    ))
                })?;
                let mut b = Vec::new();
                // `ByteBufCodecs.CONTAINER_ID`, then `ByteBufCodecs.VAR_INT`.
                write_varint(&mut b, menu.container_id)?;
                write_varint(&mut b, *button)?;
                self.conn.send(ids::play::sb::CONTAINER_BUTTON_CLICK, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::RenameItem { name } => {
                if self.menu.is_none() {
                    return Err(io::Error::other(
                        "rename_item with no open menu: handleRenameItem acts only \
                         on an open AnvilMenu, so this would measure silence",
                    ));
                }
                let mut b = Vec::new();
                // `FriendlyByteBuf.writeUtf`: a varint byte length, then UTF-8.
                write_varint(&mut b, i32::try_from(name.len()).map_err(io::Error::other)?)?;
                b.extend_from_slice(name.as_bytes());
                self.conn.send(ids::play::sb::RENAME_ITEM, &b)?;
                Ok(Duration::from_millis(250))
            }
            Step::CloseContainer => {
                // Sent with the last known container id, or 0 when nothing has
                // been opened. `handleContainerClose` reads the id and does
                // nothing with it — `doCloseContainer()` is called
                // unconditionally — so the number cannot make this wrong; it
                // is sent honestly rather than as a constant because a real
                // client sends the menu it is closing.
                let id = self.menu.map_or(0, |m| m.container_id);
                let mut b = Vec::new();
                write_varint(&mut b, id)?;
                self.conn.send(ids::play::sb::CONTAINER_CLOSE, &b)?;
                // Cleared here and not only in the recorder: the harness's own
                // belief that a menu is open is state of exactly the shape
                // this verb exists to reset, and a later `container_click`
                // must refuse rather than click a menu that has been shut.
                self.menu = None;
                Ok(Duration::from_millis(120))
            }
            Step::SignUpdate { pos, front, lines } => {
                // **Refused rather than truncated**, for `attack`'s and
                // `container_click`'s reason. The packet's array is
                // `new String[4]`; a fifth line has nowhere to go, and
                // dropping it quietly would make the scenario measure
                // something other than what it says.
                if lines.len() > 4 {
                    return Err(io::Error::other(format!(
                        "sign_update carries exactly four lines and this step has {}. \
                         The packet's decoder reads `new String[4]` in an \
                         unconditional loop, so the extra one cannot be sent and \
                         silently dropping it would leave a scenario whose text \
                         is not the text it asserts.",
                        lines.len()
                    )));
                }
                // 384 is `MAX_STRING_LENGTH` on the packet itself, and
                // `readUtf(384)` throws on the decoding side rather than
                // truncating — which disconnects the client mid-corpus and
                // costs every later scenario on the connection.
                //
                // **Bytes here, characters there**, and deliberately not
                // reconciled: `Utf8String.read` caps the encoded form at
                // `384 * 3` bytes and the decoded form at 384 *chars*, so this
                // guard refuses a little sooner than vanilla would for
                // multi-byte text. Refusing early is the safe direction — the
                // failure it prevents is a dropped connection partway through
                // a corpus — and every line in the corpus is ASCII, where the
                // two are the same number.
                if let Some(long) = lines.iter().find(|l| l.len() > 384) {
                    return Err(io::Error::other(format!(
                        "sign line is {} bytes and the packet's MAX_STRING_LENGTH \
                         is 384; `readUtf(384)` throws on the server rather than \
                         truncating, which drops the connection and costs every \
                         later scenario on it",
                        long.len()
                    )));
                }
                let mut b = Vec::new();
                write_i64(&mut b, Self::pack(self.at(*pos)))?;
                write_bool(&mut b, *front)?;
                // Always four, padded. Not `lines.len()` and no count prefix:
                // the decoder's loop is `for (i = 0; i < 4; i++)`, so a short
                // write desynchronises the rest of the stream rather than
                // producing a short sign.
                for i in 0..4 {
                    write_string(&mut b, lines.get(i).map_or("", String::as_str))?;
                }
                self.conn.send(ids::play::sb::SIGN_UPDATE, &b)?;
                // `handleSignUpdate` does not act on the packet thread: it
                // hands the lines to `filterTextPacket` and resumes in
                // `thenAcceptAsync(..., server)`, so the edit lands on a later
                // tick and the `block_entity_data` after that. 250 ms is the
                // same grace `place` takes and covers both.
                Ok(Duration::from_millis(250))
            }
            Step::Swing { hand, ms } => {
                // **Refused rather than defaulted**, for `container_click`'s
                // reason and with a sharper edge: `main` and `off` produce
                // different `animate` actions, so a typo silently checked
                // would assert the wrong number against both servers at once
                // and agree with itself.
                let Some(ordinal) = crate::scenario::hand_ordinal(hand) else {
                    return Err(io::Error::other(format!(
                        "`{hand}` is not an InteractionHand. The two are `main` and \
                         `off`; defaulting an unknown one would send a legal packet \
                         for the other hand and record a match for a mechanism this \
                         scenario never reached."
                    )));
                };
                let mut b = Vec::new();
                write_varint(&mut b, ordinal)?;
                self.conn.send(ids::play::sb::SWING, &b)?;
                Ok(Duration::from_millis(*ms))
            }
            Step::ClientCommand { action } => {
                // Refused, never defaulted — `Scenario::load` refuses the same
                // thing at load, and this one guards the buffer for whoever
                // else calls `act`. `Step::Swing`'s reason, with three doors
                // instead of two.
                let Some(ordinal) = crate::scenario::client_command_action(action) else {
                    return Err(io::Error::other(format!(
                        "`{action}` is not a ServerboundClientCommandPacket$Action. The \
                         three are `perform_respawn`, `request_stats` and \
                         `request_gamerule_values`; defaulting an unknown one would send \
                         a legal packet for a different action and record a match for a \
                         mechanism this scenario never reached."
                    )));
                };
                // `FriendlyByteBuf.writeEnum` is `writeVarInt(ordinal())` and
                // the packet's `write` is that call and a `pop`. One varint is
                // the entire body.
                let mut b = Vec::new();
                write_varint(&mut b, ordinal)?;
                self.conn.send(ids::play::sb::CLIENT_COMMAND, &b)?;
                // A respawn is the largest single answer in this harness's
                // vocabulary — `PlayerList.respawn` tears the player down and
                // rebuilds them, and the chunk stream follows. 400 ms is
                // `Step::Command`'s window, which is the longest this file
                // already grants anything, and it is the floor rather than a
                // fitted value: a scenario measuring a respawn should say so
                // with a `wait` after it rather than trusting this number.
                Ok(Duration::from_millis(400))
            }
            Step::Command { text } => {
                let mut b = Vec::new();
                write_string(&mut b, text)?;
                self.conn.send(ids::play::sb::CHAT_COMMAND, &b)?;
                Ok(Duration::from_millis(400))
            }
            Step::Say { text } => {
                // `ServerboundChatPacket`: the message, an epoch-millis
                // timestamp, a salt, a nullable 256-byte signature, then
                // `LastSeenMessages$Update` — a varint offset, a 20-bit
                // fixed bitset (**three** bytes), and a trailing i8
                // checksum. The checksum is the field a hand-written client
                // forgets, and forgetting it desynchronises the server's
                // frame reader rather than failing this packet.
                let mut b = Vec::new();
                write_string(&mut b, text)?;
                write_i64(&mut b, 0)?; // timeStamp — vanilla discards it
                write_i64(&mut b, 0)?; // salt — vanilla discards it too
                write_bool(&mut b, false)?; // no signature
                write_varint(&mut b, 0)?; // lastSeen offset
                b.extend_from_slice(&[0, 0, 0]); // the 20-bit bitset
                write_u8(&mut b, 0)?; // LastSeenMessages.Update.IGNORE_CHECKSUM
                self.conn.send(ids::play::sb::CHAT, &b)?;
                Ok(Duration::from_millis(400))
            }
            Step::Wait { ms } => Ok(Duration::from_millis(*ms)),
            // Dispatched by `crate::record::act_on`, which owns the probe
            // connection; this method only ever holds one session and it is
            // the actor's. Refused rather than ignored: performing a probe
            // verb on the actor would disconnect the connection every later
            // scenario runs on, which is the exact failure the probe exists
            // to avoid.
            Step::ProbeJoin | Step::ProbeAckTeleportAgain => Err(io::Error::other(
                "a probe verb reached Session::act — it must be dispatched by \
                 record::act_on, which owns the probe connection",
            )),
            // Handled by `record::act_or_console`, which has the console this
            // client does not. Reachable only from a second caller added
            // without that interception, so it names what is missing rather
            // than doing nothing — a console step silently skipped would
            // record a window holding only what it did not cause.
            Step::Console { text, .. } => Err(io::Error::other(format!(
                "console step {text:?} reached Client::act, which has no console — \
                 route it through record::act_or_console"
            ))),
        }
    }

    /// The `player_action`s that name no block: the two drops and the
    /// offhand swap.
    ///
    /// What a real client sends for these is `BlockPos.ZERO`,
    /// `Direction.DOWN` and the three-argument constructor, which leaves
    /// `sequence` at zero. None of them reaches `handleBlockBreakAction`, so
    /// none is acknowledged and none consumes a sequence number —
    /// incrementing ours here would put every later ack in the scenario one
    /// out, which is the kind of drift that shows up three checks away from
    /// its cause.
    fn bare_player_action(&mut self, action: i32) -> io::Result<()> {
        let mut b = Vec::new();
        write_varint(&mut b, action)?;
        write_i64(&mut b, 0)?;
        write_u8(&mut b, 0)?;
        write_varint(&mut b, 0)?;
        self.conn.send(ids::play::sb::PLAYER_ACTION, &b)
    }
}

fn split_addr(addr: &str) -> io::Result<(&str, u16)> {
    let (h, p) = addr
        .rsplit_once(':')
        .ok_or_else(|| io::Error::other("address must be host:port"))?;
    Ok((h, p.parse().map_err(io::Error::other)?))
}

/// The printable runs inside a packet body, for the one error nobody can act
/// on without them.
///
/// `ClientboundDisconnectPacket` carries its reason as an NBT component, and
/// there is no NBT decoder in this crate — see [`crate::trace`]'s note on
/// chat and titles — so this does not claim to parse one. It pulls out the
/// ASCII runs, which for a disconnect are the translation key and any literal
/// text. A reason that comes back `[no printable text in the reason]` is
/// still worth more than the bare sentence, because it separates an empty
/// reason from a decoder that never looked.
///
/// Written on 2026-08-10, after the `worldgen-iceberg-912` merge gate spent a
/// whole brief on `the server disconnected us in play state` while the reason
/// sat unread in the packet that carried it. A four-character floor drops the
/// one- and two-byte runs that NBT tags and varint lengths produce, which
/// would otherwise bury the key in punctuation.
fn printable_reason(body: &[u8]) -> String {
    let mut runs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for &b in body {
        if (0x20..0x7f).contains(&b) {
            cur.push(b as char);
        } else if cur.chars().count() >= 4 {
            runs.push(std::mem::take(&mut cur));
        } else {
            cur.clear();
        }
    }
    if cur.chars().count() >= 4 {
        runs.push(cur);
    }
    if runs.is_empty() {
        "[no printable text in the reason]".to_string()
    } else {
        runs.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::{
        interact_body, newest_of_type, oldest_of_type, write_component_patch, Tables,
        DESIRED_CHUNKS_PER_TICK,
    };
    use crate::scenario::HeldComponent;
    use std::collections::BTreeMap;

    const PLAYER: i32 = 156;
    const ITEM: i32 = 71;

    /// The interact body is the four fields of `STREAM_CODEC` in order, and
    /// the secondary-action byte is the sneak state passed in rather than a
    /// constant: a body that always wrote 0 would release a sneak.
    #[test]
    fn an_interact_body_is_id_hand_zero_location_and_the_sneak_bit() {
        assert_eq!(interact_body(5, 0, false).unwrap(), vec![5, 0, 0, 0]);
        assert_eq!(
            interact_body(300, 1, true).unwrap(),
            vec![0xAC, 0x02, 1, 0, 1]
        );
    }

    /// `oldest` and `newest` pick the two ends of the arrival order, and both
    /// skip entities of another type.
    #[test]
    fn oldest_and_newest_are_the_two_ends_of_one_type() {
        let seen = [(7, ITEM), (8, PLAYER), (9, ITEM), (10, PLAYER)];
        assert_eq!(oldest_of_type(&seen, PLAYER), Some(8));
        assert_eq!(newest_of_type(&seen, PLAYER), Some(10));
        assert_eq!(oldest_of_type(&seen, 3), None);
    }

    // ---------------------------------------------------------------------
    // `DataComponentPatch` on the way OUT.
    //
    // These are the encoding-side twin of `trace::read_item_stack`'s tests,
    // and they exist because this patch is the corpus's only route to a
    // component-bearing stack. If they go, `hold` silently returns to writing
    // `0, 0` and the item census's opaque count returns to being a reading of
    // this harness rather than of vanilla.
    // ---------------------------------------------------------------------

    fn tables() -> Tables {
        Tables {
            items: BTreeMap::from([("minecraft:diamond_sword".into(), 900)]),
            item_names: BTreeMap::from([(900, "minecraft:diamond_sword".into())]),
            states: BTreeMap::new(),
            entity_types: BTreeMap::new(),
            // `minecraft:damage` is protocol id 3 in the pinned jar's
            // `minecraft:data_component_type` registry.
            component_types: BTreeMap::from([("minecraft:damage".into(), 3)]),
            block_entity_types: BTreeMap::new(),
        }
    }

    /// **The compatibility assertion, and the reason no baseline moves.**
    ///
    /// Every scenario in the corpus that names no component must put exactly
    /// the bytes on the wire that the old literal `0, 0` put there. Asserted
    /// rather than reasoned about: a patch writer that emitted, say, a
    /// length-prefixed empty list would change 146 hold steps per recording
    /// and every one of the resulting divergences would look like a server
    /// defect.
    #[test]
    fn an_empty_patch_is_the_two_zero_bytes_the_old_call_site_wrote() {
        let mut b = Vec::new();
        write_component_patch(&mut b, &[], &tables()).expect("wrote");
        assert_eq!(b, vec![0x00, 0x00]);
    }

    /// **Vanilla's own bytes, taken by running vanilla's own codec.**
    ///
    /// Added count, no removals, the type's registry id, then the value
    /// **length-prefixed** — `DataComponentPatch.DELIMITED_STREAM_CODEC`,
    /// which is what `ServerboundSetCreativeModeSlotPacket` composes through
    /// `ItemStack.OPTIONAL_UNTRUSTED_STREAM_CODEC`.
    ///
    /// This test asserted `[0x01, 0x00, 0x03, 0x05]` until 2026-08-13 and was
    /// green, because it was written from the same misreading as the code it
    /// tested — that serverbound stacks use the plain
    /// `DataComponentPatch.STREAM_CODEC`. **A test derived from the
    /// implementation's own assumption cannot falsify that assumption.** What
    /// caught it was vanilla refusing the packet
    /// (`docs/PARITY-ROADMAP.md` §44.7); what fixed it was loading the pinned
    /// jar and running `DataComponentPatch.DELIMITED_STREAM_CODEC.encode`
    /// against a real `RegistryFriendlyByteBuf`, which emits:
    ///
    /// ```text
    ///   trusted  (clientbound):   01 01 03    05 02
    ///   delimited (serverbound):  01 01 03 01 05 02
    ///                                      ^^ the byte we were missing
    /// ```
    ///
    /// (that pair is added=`damage 5`, removed=`max_damage`; ours below is
    /// added=`damage 5`, no removals.) The clientbound form is still the
    /// right one for `trace::read_item_stack`, which must not grow a prefix.
    /// A pre-encoded enchantment map goes out length-prefixed like any
    /// other value, and `bytes` on a type that does not take it is refused.
    #[test]
    fn stored_enchantments_go_out_as_their_bytes_length_prefixed() {
        let mut t = tables();
        t.component_types
            .insert("minecraft:stored_enchantments".into(), 34);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:stored_enchantments".into(),
            bytes: Some(vec![1, 21, 1]),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        assert_eq!(b, vec![1, 0, 34, 3, 1, 21, 1]);
        let bad = [HeldComponent {
            kind: "minecraft:damage".into(),
            varint: Some(1),
            bytes: Some(vec![1]),
            ..Default::default()
        }];
        let mut b = Vec::new();
        assert!(write_component_patch(&mut b, &bad, &t).is_err());
        assert!(b.is_empty());
    }

    #[test]
    fn write_component_patch_matches_vanillas_delimited_bytes() {
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:damage".into(),
            varint: Some(5),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &tables()).expect("wrote");
        assert_eq!(
            b,
            vec![0x01, 0x00, 0x03, 0x01, 0x05],
            "added=1, removed=0, type=3 (minecraft:damage), len=1, value=5"
        );
    }

    /// `minecraft:entity_data` with only `Silent`: the type's registry id,
    /// then an UNNAMED network compound — and all of it inside the delimited
    /// length prefix, like every serverbound component value.
    #[test]
    fn an_entity_data_component_is_a_type_id_and_an_unnamed_compound() {
        let mut t = tables();
        t.component_types.insert("minecraft:entity_data".into(), 71);
        t.entity_types.insert("minecraft:pig".into(), 100);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:entity_data".into(),
            entity: Some("minecraft:pig".into()),
            silent: Some(true),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        let mut want = vec![0x01, 0x00, 71, 13, 100, 10, 1, 0, 6];
        want.extend_from_slice(b"Silent");
        want.extend_from_slice(&[1, 0]);
        assert_eq!(
            b, want,
            "added=1, removed=0, type, len=13, pig, compound{{Silent:1b}}"
        );
        // And it is refused with a varint, or without its entity, and the
        // egg fields are refused on any other component.
        for bad in [
            HeldComponent {
                kind: "minecraft:entity_data".into(),
                varint: Some(1),
                entity: Some("minecraft:pig".into()),
                silent: Some(true),
                ..Default::default()
            },
            HeldComponent {
                kind: "minecraft:entity_data".into(),
                silent: Some(true),
                ..Default::default()
            },
            HeldComponent {
                kind: "minecraft:damage".into(),
                varint: Some(1),
                silent: Some(true),
                ..Default::default()
            },
        ] {
            let mut b = Vec::new();
            assert!(write_component_patch(&mut b, &[bad], &t).is_err());
            assert!(b.is_empty());
        }
    }

    /// `left_handed` appends a `LeftHanded` byte after `Silent`, inside the
    /// same compound and the same length prefix, and only when declared.
    #[test]
    fn an_entity_data_component_carries_left_handed_when_declared() {
        let mut t = tables();
        t.component_types.insert("minecraft:entity_data".into(), 71);
        t.entity_types.insert("minecraft:bat".into(), 10);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:entity_data".into(),
            entity: Some("minecraft:bat".into()),
            silent: Some(false),
            left_handed: Some(false),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        let mut want = vec![0x01, 0x00, 71, 27, 10, 10, 1, 0, 6];
        want.extend_from_slice(b"Silent");
        want.extend_from_slice(&[0, 1, 0, 10]);
        want.extend_from_slice(b"LeftHanded");
        want.extend_from_slice(&[0, 0]);
        assert_eq!(b, want, "len=27, bat, compound{{Silent:0b,LeftHanded:0b}}");
        // `true` is the byte 1, so the field is read rather than assumed.
        let mut b1 = Vec::new();
        let c1 = [HeldComponent {
            left_handed: Some(true),
            ..c[0].clone()
        }];
        write_component_patch(&mut b1, &c1, &t).expect("wrote");
        assert_eq!(b1[b1.len() - 2], 1);
        // Refused on any other component.
        let bad = HeldComponent {
            kind: "minecraft:damage".into(),
            varint: Some(1),
            left_handed: Some(false),
            ..Default::default()
        };
        assert!(write_component_patch(&mut Vec::new(), &[bad], &t).is_err());
    }

    /// `minecraft:container`: a slot count, then presence, item, count and
    /// an empty patch per slot — item before count, unlike `ItemStack`.
    #[test]
    fn a_container_is_a_list_of_optional_templates() {
        let mut t = tables();
        t.component_types.insert("minecraft:container".into(), 70);
        t.items.insert("minecraft:stone".into(), 1);
        let stone = 1u8;
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:container".into(),
            container: Some(vec![
                crate::scenario::ContainerSlot {
                    item: Some("minecraft:stone".into()),
                    count: 5,
                },
                crate::scenario::ContainerSlot::default(),
            ]),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        // stone is id 1, one varint byte.
        let value = vec![2u8, 1, stone, 5, 0, 0, 0];
        let mut want = vec![1u8, 0, 70, u8::try_from(value.len()).expect("short")];
        want.extend_from_slice(&value);
        assert_eq!(b, want);
    }

    /// `minecraft:block_state`: a count, then length-prefixed UTF-8 keys and
    /// values.
    #[test]
    fn a_block_state_is_a_map_of_strings() {
        let mut t = tables();
        t.component_types.insert("minecraft:block_state".into(), 64);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:block_state".into(),
            properties: Some(BTreeMap::from([("axis".to_string(), "z".to_string())])),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        assert_eq!(b, vec![1, 0, 64, 8, 1, 4, b'a', b'x', b'i', b's', 1, b'z']);
    }

    /// `minecraft:custom_name` from `text`: one unnamed string tag, inside
    /// the delimited length prefix.
    #[test]
    fn a_custom_name_is_one_unnamed_string_tag() {
        let mut t = tables();
        t.component_types.insert("minecraft:custom_name".into(), 6);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:custom_name".into(),
            text: Some("Loot".into()),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        assert_eq!(b, vec![1, 0, 6, 7, 8, 0, 4, b'L', b'o', b'o', b't']);
        // `text` on anything else, or custom_name without it, is refused.
        for bad in [
            HeldComponent {
                kind: "minecraft:damage".into(),
                varint: Some(1),
                text: Some("x".into()),
                ..Default::default()
            },
            HeldComponent {
                kind: "minecraft:custom_name".into(),
                ..Default::default()
            },
        ] {
            let mut b = Vec::new();
            assert!(write_component_patch(&mut b, &[bad], &t).is_err());
        }
    }

    /// `minecraft:block_entity_data`: the component's own bytes are the
    /// block-entity type varint and a nameless compound, and the whole value
    /// is length-prefixed like any other serverbound component.
    #[test]
    fn block_entity_data_is_a_type_varint_and_a_nameless_compound() {
        let mut t = tables();
        t.component_types
            .insert("minecraft:block_entity_data".into(), 60);
        t.block_entity_types.insert("minecraft:chest".into(), 1);
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:block_entity_data".into(),
            block_entity: Some("minecraft:chest".into()),
            strings: BTreeMap::from([("A".to_string(), "b".to_string())]),
            longs: BTreeMap::from([("S".to_string(), 2)]),
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &t).expect("wrote");
        let value = vec![
            0x01, // block entity type 1
            10,   // TAG_Compound, no name
            8, 0, 1, b'A', 0, 1, b'b', // "A": "b"
            4, 0, 1, b'S', 0, 0, 0, 0, 0, 0, 0, 2, // "S": 2L
            0, // TAG_End
        ];
        let mut want = vec![0x01, 0x00, 60, u8::try_from(value.len()).unwrap()];
        want.extend_from_slice(&value);
        assert_eq!(b, want);
    }

    /// A two-byte value must move the length prefix, or the prefix is a
    /// constant `1` that happens to be right for small damage values and
    /// wrong for everything else — which is the same class of coincidence
    /// that kept the missing prefix hidden.
    #[test]
    fn the_length_prefix_is_the_payload_length_and_not_a_constant() {
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:damage".into(),
            varint: Some(300), // two VarInt bytes: 0xac 0x02
            ..Default::default()
        }];
        write_component_patch(&mut b, &c, &tables()).expect("wrote");
        assert_eq!(b, vec![0x01, 0x00, 0x03, 0x02, 0xac, 0x02]);
    }

    /// **The refusal, which is the half that can fail.** A type with no
    /// stream codec here is named and rejected before it reaches the socket.
    /// Without this the harness would write a VarInt where vanilla's decoder
    /// expects something else, vanilla would throw inside the packet read,
    /// and the recording would show a reference that stopped answering.
    #[test]
    fn a_component_with_no_codec_here_is_refused_rather_than_written() {
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:lore".into(),
            varint: Some(5),
            ..Default::default()
        }];
        let e = write_component_patch(&mut b, &c, &tables()).expect_err("refused");
        assert!(e.to_string().contains("minecraft:lore"), "{e}");
        assert!(b.is_empty(), "nothing reached the buffer");
    }

    /// A codec this harness *has* but the extract does not name is a
    /// different failure from one it does not have, and says so: the fix is
    /// to re-run the dumper, not to write a codec.
    #[test]
    fn a_known_codec_with_no_registry_entry_names_the_dumper() {
        let mut t = tables();
        t.component_types.clear();
        let mut b = Vec::new();
        let c = [HeldComponent {
            kind: "minecraft:damage".into(),
            varint: Some(1),
            ..Default::default()
        }];
        let e = write_component_patch(&mut b, &c, &t).expect_err("refused");
        assert!(e.to_string().contains("dump-items.sh"), "{e}");
    }

    /// The **degenerate** half. One entity, of the wanted type, and it
    /// resolves. This passes under every wrong rule worth worrying about —
    /// "the last entity", "the first entity", "any entity" — which is why it
    /// is not on its own.
    #[test]
    fn the_only_player_is_the_target() {
        assert_eq!(newest_of_type(&[(42, PLAYER)], PLAYER), Some(42));
    }

    /// The **discriminating** half, and the pair the ladder-port lesson asks
    /// for. A scenario that drops an item and then attacks a player has an
    /// `add_entity` for the item *after* the one for the player, so "the last
    /// entity this client heard about" — the obvious implementation — sends
    /// the item's id and attacks a dropped stack. Both servers would refuse it
    /// identically and the check would report a match.
    #[test]
    fn a_newer_entity_of_another_type_does_not_become_the_target() {
        let seen = [(42, PLAYER), (43, ITEM)];
        assert_eq!(newest_of_type(&seen, PLAYER), Some(42));
        assert_eq!(newest_of_type(&seen, ITEM), Some(43));
    }

    /// Newest wins *within* a type, which is what "the other player" means in
    /// a world the actor has walked around: the actor never sees an
    /// `add_entity` for itself, and a player that left and came back is added
    /// again with a new id.
    #[test]
    fn the_newest_of_several_of_one_type_wins() {
        assert_eq!(
            newest_of_type(&[(42, PLAYER), (43, ITEM), (44, PLAYER)], PLAYER),
            Some(44)
        );
    }

    /// Nothing of the type is `None` — which `Client::act` turns into an
    /// error rather than into entity 0. An attack on nothing is a packet both
    /// servers ignore identically, so it is the shape of check that cannot
    /// fail.
    #[test]
    fn no_entity_of_that_type_resolves_to_nothing() {
        assert_eq!(newest_of_type(&[(43, ITEM)], PLAYER), None);
        assert_eq!(newest_of_type(&[], PLAYER), None);
    }

    /// The literal is a hand-written big-endian `f32`, so it is worth one
    /// assertion that it is the number the comment claims.
    #[test]
    fn desired_chunks_per_tick_is_the_protocol_maximum() {
        assert_eq!(DESIRED_CHUNKS_PER_TICK, 64.0_f32.to_be_bytes());
    }

    #[test]
    fn a_disconnect_reason_is_carried_out_of_the_packet() {
        // An NBT string payload: a length-prefixed run of ASCII with binary
        // tags around it, which is the shape the reason actually arrives in.
        let body = [
            0x0a, 0x08, 0x00, 0x00, 0x00, 0x1a, b'm', b'u', b'l', b't', b'i', b'p', b'l', b'a',
            b'y', b'e', b'r', b'.', b'd', b'i', b's', b'c', b'o', b'n', b'n', b'e', b'c', b't',
            0x00,
        ];
        assert_eq!(
            super::printable_reason(&body),
            "multiplayer.disconnect",
            "the run that carries the reason must survive the tags around it"
        );

        // Short runs are noise from the binary framing, not text.
        assert_eq!(
            super::printable_reason(&[0x0a, b'a', b'b', 0x00, 0xff]),
            "[no printable text in the reason]"
        );
    }
}
