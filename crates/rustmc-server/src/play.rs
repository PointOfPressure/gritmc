//! The play state — the join packet.
//!
//! Reference (26.2): `ClientboundLoginPacket` and `CommonPlayerSpawnInfo`,
//! field lists read from the jar and the **wire layout confirmed against a
//! real server's bytes** rather than inferred from the record order.
//!
//! # Why the bytes and not just the field list
//!
//! `javap` gives the fields of a record, not the order or the widths its
//! codec writes them in, and this packet has twenty of them across a nested
//! record. Two are the same type and adjacent (`isDebug`, `isFlat`); one is a
//! `Holder` that turns out to be a bare registry id; `previousGameType` is a
//! *signed* byte holding −1 where every neighbouring boolean is a zero byte.
//! Guessing any of those produces a client that disconnects several fields
//! later, blaming whichever field happened to land on a plausible value.
//!
//! So a real server was asked. [`tests::matches_vanillas_join_packet`] holds
//! its 109 bytes and this encoder must reproduce them exactly, which is a far
//! stronger claim than "the fields are all present".

use std::io::{self, Write};

use rustmc_proto::{ids, varint::write_varint, wire};

/// What a joining player is told about the world.
#[derive(Debug, Clone)]
pub struct JoinInfo {
    pub player_id: i32,
    pub hardcore: bool,
    /// Every dimension the server has, by id.
    pub levels: Vec<String>,
    pub max_players: i32,
    /// **The server's** view distance, in chunks — `server.properties`'
    /// `view-distance`, which vanilla reads out of `playerList.getViewDistance()`
    /// when it builds `ClientboundLoginPacket`.
    ///
    /// This doc used to say "the client's own view distance, echoed back",
    /// which is not what any call site did and is not what vanilla does, and
    /// it survived into a plan: a check written against it predicted this
    /// server would track entities at 160 blocks — `oracle-runner` asks for
    /// 10 — when in fact `status.rs` was filling this field with the radius
    /// of the generated spawn tile and tracking at 32.
    pub chunk_radius: i32,
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub show_death_screen: bool,
    pub do_limited_crafting: bool,
    /// Index into the `minecraft:dimension_type` registry — a bare id, not a
    /// holder with an inline fallback.
    pub dimension_type: i32,
    pub dimension: String,
    /// The *hashed* seed, which is what the client gets; it drives client-side
    /// biome noise for foliage colour and nothing authoritative.
    pub seed: i64,
    pub game_type: u8,
    /// `-1` for "none", which is why this is signed where its neighbours are
    /// booleans.
    pub previous_game_type: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    pub portal_cooldown: i32,
    pub sea_level: i32,
    pub online_mode: bool,
    pub enforces_secure_chat: bool,
}

impl Default for JoinInfo {
    fn default() -> Self {
        Self {
            player_id: 1,
            hardcore: false,
            levels: vec![
                "minecraft:overworld".into(),
                "minecraft:the_end".into(),
                "minecraft:the_nether".into(),
            ],
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 10,
            reduced_debug_info: false,
            show_death_screen: true,
            do_limited_crafting: false,
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: 0,
            game_type: 0,
            previous_game_type: -1,
            is_debug: false,
            is_flat: false,
            portal_cooldown: 0,
            sea_level: 63,
            online_mode: false,
            enforces_secure_chat: false,
        }
    }
}

impl JoinInfo {
    /// Encode `play/login`.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut o = Vec::new();
        // Not a varint: the entity id is a plain big-endian i32 here, unlike
        // most ids on the wire.
        wire::write_i32(&mut o, self.player_id)?;
        wire::write_bool(&mut o, self.hardcore)?;
        write_varint(&mut o, i32::try_from(self.levels.len()).unwrap_or(i32::MAX))?;
        for l in &self.levels {
            wire::write_string(&mut o, l)?;
        }
        write_varint(&mut o, self.max_players)?;
        write_varint(&mut o, self.chunk_radius)?;
        write_varint(&mut o, self.simulation_distance)?;
        wire::write_bool(&mut o, self.reduced_debug_info)?;
        wire::write_bool(&mut o, self.show_death_screen)?;
        wire::write_bool(&mut o, self.do_limited_crafting)?;

        // CommonPlayerSpawnInfo, inline.
        self.write_spawn_info(&mut o)?;

        wire::write_bool(&mut o, self.online_mode)?;
        wire::write_bool(&mut o, self.enforces_secure_chat)?;
        Ok(o)
    }

    /// `CommonPlayerSpawnInfo.write`, the ten fields in vanilla's order.
    ///
    /// Factored out of [`Self::encode`] because `ClientboundRespawnPacket` is
    /// *this and one more byte*. Read from
    /// `CommonPlayerSpawnInfo.write(RegistryFriendlyByteBuf)` in 26.2, whose
    /// bytecode is a straight run of eleven calls with no branches:
    ///
    /// ```text
    ///   0  DimensionType.STREAM_CODEC.encode(buf, dimensionType)  -> varint
    ///  13  buf.writeResourceKey(dimension)                        -> string
    ///  21  buf.writeLong(seed)
    ///  30  buf.writeByte(gameType.getId())
    ///  42  buf.writeByte(GameType.getNullableId(previousGameType))
    ///  54  buf.writeBoolean(isDebug)
    ///  63  buf.writeBoolean(isFlat)
    ///  72  buf.writeOptional(lastDeathLocation, ...)
    ///  85  buf.writeVarInt(portalCooldown)
    ///  94  buf.writeVarInt(seaLevel)
    /// ```
    ///
    /// The `previousGameType` byte is `getNullableId`, which is **−1** for
    /// `null` — a signed byte among booleans, which is why the field is `i8`.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn write_spawn_info(&self, o: &mut impl Write) -> io::Result<()> {
        write_varint(o, self.dimension_type)?;
        wire::write_string(o, &self.dimension)?;
        wire::write_i64(o, self.seed)?;
        wire::write_u8(o, self.game_type)?;
        #[allow(clippy::cast_sign_loss)]
        wire::write_u8(o, self.previous_game_type as u8)?;
        wire::write_bool(o, self.is_debug)?;
        wire::write_bool(o, self.is_flat)?;
        // `Optional<GlobalPos> lastDeathLocation` — absent.
        wire::write_bool(o, false)?;
        write_varint(o, self.portal_cooldown)?;
        write_varint(o, self.sea_level)?;
        Ok(())
    }

    /// `ClientboundRespawnPacket` for the dimension this `JoinInfo` describes.
    ///
    /// `dataToKeep` is the packet's second and last field, a raw byte —
    /// [`KEEP_ATTRIBUTE_MODIFIERS`], [`KEEP_ENTITY_DATA`] and [`KEEP_ALL_DATA`]
    /// are its three named values, read off the class's own `static final`
    /// constants. It is a mask: `shouldKeep(b)` is `(dataToKeep & b) != 0`.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn encode_respawn(&self, data_to_keep: u8) -> io::Result<Vec<u8>> {
        let mut o = Vec::new();
        self.write_spawn_info(&mut o)?;
        wire::write_u8(&mut o, data_to_keep)?;
        Ok(o)
    }

    /// Send it.
    ///
    /// # Errors
    /// On I/O failure.
    pub fn send(&self, stream: &mut impl Write, comp: Option<i32>) -> io::Result<()> {
        rustmc_proto::frame::write_frame(stream, comp, ids::play::cb::LOGIN, &self.encode()?)
    }

    /// Send `play/respawn` — the packet that moves a client between worlds.
    ///
    /// # Errors
    /// On I/O failure.
    pub fn send_respawn(
        &self,
        stream: &mut impl Write,
        comp: Option<i32>,
        data_to_keep: u8,
    ) -> io::Result<()> {
        rustmc_proto::frame::write_frame(
            stream,
            comp,
            ids::play::cb::RESPAWN,
            &self.encode_respawn(data_to_keep)?,
        )
    }
}

/// Refuse a connection that is already in the play state.
///
/// `ServerCommonPacketListenerImpl.disconnect(Component)` — a
/// `ClientboundDisconnectPacket` carrying the component, and then the channel
/// closes. The play and configuration states share the component codec, so
/// this is NBT and not JSON; the login state is the odd one out.
///
/// **`translate`, not `text`.** Every caller in `handleMovePlayer` and
/// `handleAcceptTeleportPacket` passes `Component.translatable(key)`, which
/// serialises to a compound holding a single `translate` string. A `text`
/// compound carrying the same key is a different packet on the wire, and
/// would be a divergence introduced by the fix for one.
///
/// # Errors
/// On I/O failure.
pub fn disconnect_translatable_body(key: &str) -> io::Result<Vec<u8>> {
    let mut c = std::collections::BTreeMap::new();
    c.insert(
        "translate".to_string(),
        rustmc_world::nbt::Nbt::String(key.to_string()),
    );
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", &rustmc_world::nbt::Nbt::Compound(c))?;
    if named.len() < 3 || named[1] != 0 || named[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "named root was not the expected tag + empty-name shape",
        ));
    }
    let mut out = Vec::new();
    out.extend_from_slice(&named[..1]);
    out.extend_from_slice(&named[3..]);
    Ok(out)
}

/// `ClientboundRespawnPacket.KEEP_ATTRIBUTE_MODIFIERS` — 1.
pub const KEEP_ATTRIBUTE_MODIFIERS: u8 = 1;
/// `ClientboundRespawnPacket.KEEP_ENTITY_DATA` — 2.
pub const KEEP_ENTITY_DATA: u8 = 2;
/// `ClientboundRespawnPacket.KEEP_ALL_DATA` — 3, which is the other two ored.
pub const KEEP_ALL_DATA: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    /// The 109 bytes a real 26.2 server sent, captured by `ci/regdump.py`.
    ///
    /// This is the whole point of the module: a field list from `javap` does
    /// not tell you the order, the widths, or that `previousGameType` is a
    /// signed byte. These bytes do.
    const VANILLA: &str = concat!(
        "0000001c00",
        "03",
        "136d696e6563726166743a6f766572776f726c64",
        "116d696e6563726166743a7468655f656e64",
        "146d696e6563726166743a7468655f6e6574686572",
        "14040a000100",
        "00",
        "136d696e6563726166743a6f766572776f726c64",
        "ede6199ec3f0251e",
        "00ff000000003f0000",
    );

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn matches_vanillas_join_packet() {
        let want = unhex(VANILLA);
        assert_eq!(want.len(), 109, "the capture is 109 bytes");

        let info = JoinInfo {
            player_id: 28,
            max_players: 20,
            chunk_radius: 4,
            simulation_distance: 10,
            seed: i64::from_be_bytes(unhex("ede6199ec3f0251e").try_into().expect("8 bytes")),
            ..JoinInfo::default()
        };
        let got = info.encode().expect("encode");

        // Compared byte for byte, and the first difference is reported with
        // its offset — "the packets differ" is not a diagnosis.
        if got != want {
            let at = got
                .iter()
                .zip(&want)
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| got.len().min(want.len()));
            panic!(
                "first difference at byte {at}: ours {:#04x}, vanilla {:#04x}\n ours: {}\n van : {}",
                got.get(at).copied().unwrap_or(0),
                want.get(at).copied().unwrap_or(0),
                hex(&got),
                hex(&want)
            );
        }
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// The **spawn-info slice of the vanilla capture**, sliced out of the 109
    /// bytes above rather than retyped: `dimension_type` varint, the dimension
    /// id, the seed, the two game-type bytes, `isDebug`, `isFlat`, the absent
    /// `lastDeathLocation`, `portalCooldown`, `seaLevel`. Thirty-six bytes.
    ///
    /// The two bytes after it in the login packet (`online_mode`,
    /// `enforces_secure_chat`) are `ClientboundLoginPacket`'s own and are not
    /// part of `CommonPlayerSpawnInfo` — a respawn packet that carried them
    /// would be two bytes long and the client would read `dataToKeep` out of
    /// `online_mode`.
    const VANILLA_SPAWN_INFO: &str = concat!(
        "00",
        "136d696e6563726166743a6f766572776f726c64",
        "ede6199ec3f0251e",
        "00ff000000003f",
    );

    /// `ClientboundRespawnPacket` is `CommonPlayerSpawnInfo` then one byte.
    ///
    /// Scored against bytes vanilla actually sent, not against our own
    /// encoder: the vector is a slice of the login capture, so a change to
    /// any of the ten field widths turns this red at a named offset.
    #[test]
    fn the_respawn_packet_is_the_spawn_info_and_one_byte() {
        let info = JoinInfo {
            seed: i64::from_be_bytes(unhex("ede6199ec3f0251e").try_into().expect("8 bytes")),
            ..JoinInfo::default()
        };
        let spawn = unhex(VANILLA_SPAWN_INFO);
        assert_eq!(spawn.len(), 36, "the spawn-info slice is 36 bytes");

        // It really is a slice of the join packet, not a hand-copy of it.
        let join = info.encode().expect("encode");
        assert_eq!(
            &join[join.len() - 38..join.len() - 2],
            &spawn[..],
            "the spawn info sits 2 bytes from the end of the login packet"
        );

        for keep in [
            0u8,
            KEEP_ATTRIBUTE_MODIFIERS,
            KEEP_ENTITY_DATA,
            KEEP_ALL_DATA,
        ] {
            let mut want = spawn.clone();
            want.push(keep);
            let got = info.encode_respawn(keep).expect("encode");
            assert_eq!(got.len(), 37, "36 + the dataToKeep byte");
            if got != want {
                let at = got
                    .iter()
                    .zip(&want)
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| got.len().min(want.len()));
                panic!(
                    "keep={keep}: first difference at byte {at}\n ours: {}\n van : {}",
                    hex(&got),
                    hex(&want)
                );
            }
        }
    }

    /// The positive control for the test above: a respawn packet built for a
    /// *different* dimension must not equal the overworld one. Without this,
    /// an `encode_respawn` that ignored its own fields and returned a
    /// constant would pass.
    #[test]
    fn a_respawn_into_another_dimension_differs() {
        let over = JoinInfo::default().encode_respawn(0).expect("encode");
        let nether = JoinInfo {
            dimension_type: 3,
            dimension: "minecraft:the_nether".into(),
            sea_level: 32,
            ..JoinInfo::default()
        }
        .encode_respawn(0)
        .expect("encode");
        assert_ne!(over, nether);
        assert_eq!(over[0], 0, "overworld is dimension_type 0");
        assert_eq!(nether[0], 3, "the_nether is dimension_type 3");
    }

    /// `previousGameType` is −1 and must survive as `0xff`, not saturate or
    /// wrap to zero — it sits between two booleans, so a wrong width shifts
    /// every field after it.
    #[test]
    fn the_previous_game_type_is_a_signed_byte() {
        let info = JoinInfo {
            previous_game_type: -1,
            ..JoinInfo::default()
        };
        let bytes = info.encode().expect("encode");
        assert!(
            bytes.windows(2).any(|w| w == [0x00, 0xff]),
            "game type 0 then previous -1 as 0xff"
        );
    }
}
