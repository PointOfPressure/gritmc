//! The configuration state — the exchange between login and play.
//!
//! Reference (26.2): the `configuration` protocol state, ids from
//! [`rustmc_proto::ids`], which come from Mojang's own packet report.
//!
//! ```text
//!   C->S  client_information       language, view distance, skin parts, ...
//!   S->C  select_known_packs       which datapacks the server has
//!   C->S  select_known_packs       which of those the client also has
//!   S->C  registry_data            one per registry
//!   S->C  finish_configuration
//!   C->S  finish_configuration     -> play state
//! ```
//!
//! # What `select_known_packs` is actually for
//!
//! It is not a version handshake. It is a *bandwidth* negotiation: for every
//! pack both sides know, the server may send a registry entry with **no
//! payload** and the client fills it in from its own copy. Claim a pack the
//! client does not have and it renders nothing for those entries; claim none
//! and every entry must carry full NBT.
//!
//! This port claims `minecraft:core` at [`rustmc_proto::GAME_VERSION`], which
//! is what a vanilla server claims, and then still sends payloads. That is
//! deliberate: sending data the client could have inferred is wasteful but
//! correct, while omitting data on a mistaken assumption is silently wrong.
//! Cheap to tighten later once entries are gated against vanilla's own bytes.
//!
//! # Status
//!
//! The exchange is implemented and gated. The registry **contents** are not:
//! [`RegistryData::entries`] is whatever a caller supplies, and nothing yet
//! builds the dimension-type, biome and damage-type registries a client needs
//! before it will render a world. That is the next piece of work and it is
//! named here rather than left to be discovered from a client that hangs.

use std::io::{self, Read, Write};

use rustmc_proto::{
    frame, ids,
    varint::{read_varint, write_varint},
    wire,
};

/// One entry of a registry as the client received it: its id, and the payload
/// that came with it — `None` where the server sent none, which tells the
/// client to use the entry from the pack it already has.
pub type RegistryEntry = (String, Option<rustmc_world::nbt::Nbt>);

/// One registry sent to the client.
#[derive(Debug, Clone)]
pub struct RegistryData {
    /// e.g. `minecraft:dimension_type`.
    pub registry: String,
    /// Each entry's id, and its NBT payload when the client cannot infer it.
    pub entries: Vec<(String, Option<rustmc_world::nbt::Nbt>)>,
}

impl RegistryData {
    /// Encode the packet body.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        wire::write_string(&mut out, &self.registry)?;
        write_varint(
            &mut out,
            i32::try_from(self.entries.len()).unwrap_or(i32::MAX),
        )?;
        for (id, payload) in &self.entries {
            wire::write_string(&mut out, id)?;
            match payload {
                Some(nbt) => {
                    wire::write_bool(&mut out, true)?;
                    // Network NBT is *nameless* at the root: the tag byte and
                    // payload with no name string. `write_named_root` with an
                    // empty name would emit a two-byte length prefix the
                    // client does not expect, and the failure is a
                    // disconnect blaming a completely unrelated field.
                    write_network_nbt(&mut out, nbt)?;
                }
                None => wire::write_bool(&mut out, false)?,
            }
        }
        Ok(out)
    }
}

/// Network NBT: a nameless root, unlike the on-disk form.
fn write_network_nbt(out: &mut impl Write, value: &rustmc_world::nbt::Nbt) -> io::Result<()> {
    let mut named = Vec::new();
    rustmc_world::nbt::write_named_root(&mut named, "", value)?;
    // `write_named_root` emits: tag byte, u16 name length, name, payload.
    // Strip the two zero length bytes that follow the tag.
    if named.len() < 3 || named[1] != 0 || named[2] != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "named root was not the expected tag + empty-name shape",
        ));
    }
    out.write_all(&named[..1])?;
    out.write_all(&named[3..])
}

/// Registry payloads captured from a real vanilla server.
///
/// # Why captured rather than built
///
/// A client will not render a world until it has all 29 synchronised
/// registries — biomes, dimension types, damage types, and twenty-odd variant
/// registries that exist only to name a cow's colour. Their contents are
/// **datapack data**, not code: the same category as everything else under
/// `data/vanilla/`, which this project extracts rather than reimplements.
///
/// Building them from the datapack JSON means re-deriving NBT types that JSON
/// does not carry — whether `ambient_light` is a float or a double, whether
/// `has_skylight` is a byte or a bool — for hundreds of fields. Every one of
/// those is a guess unless read from a codec, and a wrong one is a client
/// that disconnects blaming an unrelated field.
///
/// So the bytes come from vanilla. `ci/dump-registries.sh` connects to a real
/// server, completes login and configuration, and writes every
/// `registry_data` body verbatim. The file is gitignored like the rest of the
/// extract and regenerating it is a one-liner on a version bump.
///
/// The format is deliberately trivial — `RMCREG01`, a count, then
/// length-prefixed bodies — because it is an intermediate, and a format worth
/// parsing carefully is a format worth not inventing.
pub struct CapturedRegistries {
    bodies: Vec<Vec<u8>>,
}

impl CapturedRegistries {
    /// Read a capture.
    ///
    /// # Errors
    /// If the file is missing, truncated, or does not start with the magic.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let raw = std::fs::read(path)?;
        if raw.len() < 12 || &raw[..8] != b"RMCREG01" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a registry capture (bad magic)",
            ));
        }
        let count = u32::from_be_bytes(raw[8..12].try_into().expect("4 bytes")) as usize;
        let mut bodies = Vec::with_capacity(count);
        let mut at = 12usize;
        for i in 0..count {
            let end = at
                .checked_add(4)
                .filter(|e| *e <= raw.len())
                .ok_or_else(|| truncated(i))?;
            let len = u32::from_be_bytes(raw[at..end].try_into().expect("4 bytes")) as usize;
            at = end;
            let body_end = at
                .checked_add(len)
                .filter(|e| *e <= raw.len())
                .ok_or_else(|| truncated(i))?;
            bodies.push(raw[at..body_end].to_vec());
            at = body_end;
        }
        Ok(Self { bodies })
    }

    /// How many registries the capture holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    /// Whether the capture is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }

    /// The entry ids of one registry, in the order the client received them.
    ///
    /// That order *is* the numbering. A chunk's biome container carries
    /// indices into this list, so a biome name only means something against
    /// the same capture the client was sent. Deriving the numbering from
    /// anywhere else — a sorted name list, the datapack directory — produces
    /// a world whose biomes are quietly wrong everywhere, which reads as a
    /// worldgen bug rather than a registry one.
    ///
    /// `None` if the capture has no such registry.
    ///
    /// # Errors
    /// If the body is malformed.
    pub fn entry_ids(&self, registry: &str) -> io::Result<Option<Vec<String>>> {
        for body in &self.bodies {
            let mut cur = &body[..];
            if wire::read_string(&mut cur)? != registry {
                continue;
            }
            let count = rustmc_proto::varint::read_varint(&mut cur)?;
            let mut out = Vec::new();
            for _ in 0..count.max(0) {
                out.push(wire::read_string(&mut cur)?);
                // The payload is parsed rather than skipped: network NBT is
                // self-describing but carries no length, so there is no way
                // to step over it without reading it.
                if wire::read_bool(&mut cur)? {
                    rustmc_world::nbt::read_network_tag(&mut cur)?;
                }
            }
            return Ok(Some(out));
        }
        Ok(None)
    }

    /// The entries of one registry with their payloads, in receive order.
    ///
    /// The stronger form of [`Self::entry_ids`]: a `dimension_type` entry's
    /// payload is what tells the client how tall the dimension is and whether
    /// it has a sky, and a server that serves a second dimension has to agree
    /// with the bytes it sent rather than with a constant somebody typed.
    /// `None` in the second half of the pair is an entry sent with no payload
    /// — legal, and means "use the one from the pack you already have".
    ///
    /// `None` if the capture has no such registry.
    ///
    /// # Errors
    /// If the body is malformed.
    pub fn entries(&self, registry: &str) -> io::Result<Option<Vec<RegistryEntry>>> {
        for body in &self.bodies {
            let mut cur = &body[..];
            if wire::read_string(&mut cur)? != registry {
                continue;
            }
            let count = rustmc_proto::varint::read_varint(&mut cur)?;
            let mut out = Vec::new();
            for _ in 0..count.max(0) {
                let id = wire::read_string(&mut cur)?;
                let tag = if wire::read_bool(&mut cur)? {
                    rustmc_world::nbt::read_network_tag(&mut cur)?
                } else {
                    None
                };
                out.push((id, tag));
            }
            return Ok(Some(out));
        }
        Ok(None)
    }

    /// Send every registry, in capture order.
    ///
    /// Order is preserved because a client applies them in the order it
    /// receives them and a later registry may reference an earlier one.
    ///
    /// # Errors
    /// On I/O failure.
    pub fn send(&self, stream: &mut impl Write, comp: Option<i32>) -> io::Result<()> {
        for body in &self.bodies {
            frame::write_frame(stream, comp, ids::configuration::cb::REGISTRY_DATA, body)?;
        }
        Ok(())
    }
}

fn truncated(i: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::UnexpectedEof,
        format!("registry capture truncated at entry {i}"),
    )
}

/// What the client told us about itself.
#[derive(Debug, Clone)]
pub struct ClientInformation {
    pub language: String,
    pub view_distance: i8,
    /// `ChatVisiblity`'s ordinal, straight from the packet.
    ///
    /// Kept as the raw ordinal rather than decoded here so this module keeps
    /// knowing nothing about chat. It decides whether the player is ever sent
    /// a `player_chat` at all — `ServerPlayer.acceptsChatMessages()` is
    /// `chatVisibility == FULL` — so a server that dropped it would talk to
    /// players who asked for silence. See [`crate::chat::Visibility`].
    pub chat_visibility: i32,
    /// `ServerboundClientInformationPacket`'s displayed-skin-parts bitmask.
    ///
    /// **This is not a preference the server may ignore.**
    /// `ServerPlayer.updateOptions` writes it straight into
    /// `Avatar.DATA_PLAYER_MODE_CUSTOMISATION`, an accessor whose definition
    /// is `0`, so any client that displays any skin part is away from its
    /// default from the moment it configures — and every `add_entity` for
    /// that player is followed by a `set_entity_data` carrying this byte. A
    /// server that parsed the packet and dropped the field sends a shorter
    /// payload than vanilla for every player pairing in the corpus.
    pub model_customisation: u8,
}

/// Run the configuration exchange.
///
/// `registries` are sent in the order given; the client applies them in that
/// order and a later one may reference an earlier.
///
/// # Errors
/// On I/O failure or an unexpected packet.
pub fn configure(
    stream: &mut (impl Read + Write),
    registries: &[RegistryData],
    comp: Option<i32>,
) -> io::Result<ClientInformation> {
    configure_inner(stream, comp, |s| {
        for reg in registries {
            frame::write_frame(
                s,
                comp,
                ids::configuration::cb::REGISTRY_DATA,
                &reg.encode()?,
            )?;
        }
        Ok(())
    })
}

/// The exchange itself, with the registry-sending step left to the caller —
/// the only thing that differs between built and captured registries.
fn configure_inner<S: Read + Write>(
    stream: &mut S,
    comp: Option<i32>,
    send_registries: impl FnOnce(&mut S) -> io::Result<()>,
) -> io::Result<ClientInformation> {
    let mut info = ClientInformation {
        language: String::new(),
        view_distance: 0,
        // FULL. Overwritten from the packet below; the initialiser only has
        // to be a value the struct can hold.
        chat_visibility: 0,
        model_customisation: 0,
    };

    // The client speaks first in this state.
    let (id, body) = frame::read_frame(stream, comp.is_some())?;
    if id != ids::configuration::sb::CLIENT_INFORMATION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected client_information, got {id:#04x}"),
        ));
    }
    let mut cur = &body[..];
    info.language = wire::read_string(&mut cur)?;
    // View distance is a signed byte; there is no `read_i8`, and a `u8`
    // reinterpreted is the same eight bits.
    #[allow(clippy::cast_possible_wrap)]
    {
        info.view_distance = wire::read_u8(&mut cur)? as i8;
    }
    // `ServerboundClientInformationPacket`'s remaining fields, in order:
    // chat visibility (varint enum), chat colours (bool), displayed skin
    // parts (byte), main hand (varint enum), text filtering (bool), allows
    // listing (bool), particle status (varint enum). Chat visibility and the
    // skin parts are the two that reach somewhere else — the former gates
    // `player_chat` (see [`crate::chat::Visibility`]), the latter reaches the
    // wire again through `Avatar.DATA_PLAYER_MODE_CUSTOMISATION` — so both are
    // kept; chat colours in between is read rather than skipped because a
    // varint is not a fixed width and a cursor advanced by a guessed length
    // would silently read the wrong byte.
    //
    // A short packet is not an error here past the field this server already
    // required: chat visibility is read with `?` because every client that
    // reaches this server sends at least that much (see
    // `the_exchange_runs_in_order_and_carries_the_registry`'s fixture note),
    // but the fields after it are read for their side effect on the cursor
    // and a client that sent fewer leaves the remainder at its default, which
    // is the same outcome as before this parsed anything at all.
    info.chat_visibility = read_varint(&mut cur)?;
    let _chat_colours = wire::read_u8(&mut cur);
    if let Ok(parts) = wire::read_u8(&mut cur) {
        info.model_customisation = parts;
    }

    // Known packs, then whatever the client keeps of them.
    let mut packs = Vec::new();
    write_varint(&mut packs, 1)?;
    wire::write_string(&mut packs, "minecraft")?;
    wire::write_string(&mut packs, "core")?;
    wire::write_string(&mut packs, rustmc_proto::GAME_VERSION)?;
    frame::write_frame(
        stream,
        comp,
        ids::configuration::cb::SELECT_KNOWN_PACKS,
        &packs,
    )?;
    let (id, _) = frame::read_frame(stream, comp.is_some())?;
    if id != ids::configuration::sb::SELECT_KNOWN_PACKS {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected select_known_packs, got {id:#04x}"),
        ));
    }

    send_registries(stream)?;

    frame::write_frame(
        stream,
        comp,
        ids::configuration::cb::FINISH_CONFIGURATION,
        &[],
    )?;
    let (id, _) = frame::read_frame(stream, comp.is_some())?;
    if id != ids::configuration::sb::FINISH_CONFIGURATION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected finish_configuration, got {id:#04x}"),
        ));
    }
    Ok(info)
}

/// The same exchange, sending a capture instead of built registries.
///
/// # Errors
/// On I/O failure or an unexpected packet.
pub fn configure_with(
    stream: &mut (impl Read + Write),
    registries: &CapturedRegistries,
    comp: Option<i32>,
) -> io::Result<ClientInformation> {
    configure_inner(stream, comp, |s| registries.send(s, comp))
}

/// Refuse during configuration, with a reason the client shows.
///
/// # Errors
/// On I/O failure.
pub fn disconnect(stream: &mut impl Write, reason: &str, comp: Option<i32>) -> io::Result<()> {
    // Unlike the login-state disconnect, this one carries **NBT**, not JSON —
    // the configuration and play states use the component codec. Sending JSON
    // here shows the player a parse error instead of the reason.
    let mut out = Vec::new();
    let mut c = std::collections::BTreeMap::new();
    c.insert(
        "text".to_string(),
        rustmc_world::nbt::Nbt::String(reason.to_string()),
    );
    write_network_nbt(&mut out, &rustmc_world::nbt::Nbt::Compound(c))?;
    frame::write_frame(stream, comp, ids::configuration::cb::DISCONNECT, &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_world::nbt::Nbt;

    struct Pipe {
        input: std::io::Cursor<Vec<u8>>,
        output: Vec<u8>,
    }
    impl Read for Pipe {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.input.read(b)
        }
    }
    impl Write for Pipe {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.output.write(b)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn client_side() -> Vec<u8> {
        let mut ci = Vec::new();
        wire::write_string(&mut ci, "en_gb").expect("buf");
        #[allow(clippy::cast_sign_loss)]
        wire::write_u8(&mut ci, 10i8 as u8).expect("buf");
        // Chat visibility, FULL. **This fixture used to stop at the view
        // distance**, which passed only because the server read no further;
        // the first field added after it turned the fixture into a truncated
        // packet. A fixture that is not a whole packet is a fixture that
        // stops testing the moment the decoder grows.
        write_varint(&mut ci, 0).expect("buf");
        let mut out = Vec::new();
        frame::write_frame(
            &mut out,
            None,
            ids::configuration::sb::CLIENT_INFORMATION,
            &ci,
        )
        .expect("buf");
        frame::write_frame(
            &mut out,
            None,
            ids::configuration::sb::SELECT_KNOWN_PACKS,
            &[],
        )
        .expect("buf");
        frame::write_frame(
            &mut out,
            None,
            ids::configuration::sb::FINISH_CONFIGURATION,
            &[],
        )
        .expect("buf");
        out
    }

    #[test]
    fn the_exchange_runs_in_order_and_carries_the_registry() {
        let mut d = std::collections::BTreeMap::new();
        d.insert("height".to_string(), Nbt::Int(384));
        let regs = vec![RegistryData {
            registry: "minecraft:dimension_type".into(),
            entries: vec![
                ("minecraft:overworld".into(), Some(Nbt::Compound(d))),
                ("minecraft:the_nether".into(), None),
            ],
        }];

        let mut pipe = Pipe {
            input: std::io::Cursor::new(client_side()),
            output: Vec::new(),
        };
        let info = configure(&mut pipe, &regs, None).expect("configured");
        assert_eq!(info.language, "en_gb");
        assert_eq!(info.view_distance, 10);

        let mut cur = &pipe.output[..];
        let (id, _) = frame::read_frame(&mut cur, false).expect("packs");
        assert_eq!(id, ids::configuration::cb::SELECT_KNOWN_PACKS);

        let (id, body) = frame::read_frame(&mut cur, false).expect("registry");
        assert_eq!(id, ids::configuration::cb::REGISTRY_DATA);
        let mut b = &body[..];
        assert_eq!(
            wire::read_string(&mut b).expect("reg"),
            "minecraft:dimension_type"
        );

        let (id, _) = frame::read_frame(&mut cur, false).expect("finish");
        assert_eq!(id, ids::configuration::cb::FINISH_CONFIGURATION);

        // `client_side()` stops after chat visibility, so the skin-parts
        // byte is absent and the field stays at the accessor's definition.
        // That is the control for the test below: a short packet must not
        // invent one.
        assert_eq!(info.model_customisation, 0);
    }

    /// The displayed-skin-parts byte, read from the position a real client
    /// puts it in and not from the one a fixed offset would guess.
    ///
    /// It is the fifth field of `ServerboundClientInformationPacket` and the
    /// two before it are a **varint** enum and a boolean, so a cursor advanced
    /// by an assumed width lands on the wrong byte and puts a plausible wrong
    /// value into `DATA_PLAYER_MODE_CUSTOMISATION` — which then goes on the
    /// wire in every pairing bundle. The two values here are chosen so that
    /// reading one byte early gives `0x01` and one byte late gives `0x01`
    /// again, and neither is `0x7f`.
    #[test]
    fn the_configuration_exchange_keeps_the_displayed_skin_parts() {
        let mut ci = Vec::new();
        wire::write_string(&mut ci, "en_gb").expect("buf");
        wire::write_u8(&mut ci, 10).expect("buf");
        write_varint(&mut ci, 0).expect("buf"); // chat visibility: full
        wire::write_u8(&mut ci, 1).expect("buf"); // chat colours: true
        wire::write_u8(&mut ci, 0x7f).expect("buf"); // displayed skin parts
        write_varint(&mut ci, 1).expect("buf"); // main hand: right
        wire::write_u8(&mut ci, 0).expect("buf"); // text filtering
        wire::write_u8(&mut ci, 1).expect("buf"); // allows listing
        write_varint(&mut ci, 0).expect("buf"); // particle status

        let mut out = Vec::new();
        for (id, body) in [
            (ids::configuration::sb::CLIENT_INFORMATION, &ci[..]),
            (ids::configuration::sb::SELECT_KNOWN_PACKS, &[][..]),
            (ids::configuration::sb::FINISH_CONFIGURATION, &[][..]),
        ] {
            frame::write_frame(&mut out, None, id, body).expect("buf");
        }

        let mut pipe = Pipe {
            input: std::io::Cursor::new(out),
            output: Vec::new(),
        };
        let info = configure(&mut pipe, &[], None).expect("configured");
        assert_eq!(info.view_distance, 10);
        assert_eq!(info.model_customisation, 0x7f);

        // And it reaches the payload it exists for.
        let s = crate::synched::Synched::Avatar {
            health: 20.0,
            model_customisation: info.model_customisation,
            score: 0,
        };
        let v = s.non_default_values().expect("two values");
        assert_eq!(v[1].id, 16);
        assert_eq!(v[1].value, vec![0x7f]);
    }

    /// Network NBT has no root name. Getting this wrong costs two bytes and
    /// produces a disconnect that blames an unrelated field.
    #[test]
    fn network_nbt_has_a_nameless_root() {
        let mut c = std::collections::BTreeMap::new();
        c.insert("a".to_string(), Nbt::Int(1));
        let mut out = Vec::new();
        write_network_nbt(&mut out, &Nbt::Compound(c)).expect("buf");

        assert_eq!(out[0], 10, "TAG_Compound");
        // The next bytes must be the first field's tag, not a name length.
        assert_eq!(out[1], 3, "TAG_Int for the first field, not a u16 length");

        // The same value both ways: the nameless form is exactly the named
        // form with the two-byte empty-name length removed, and nothing else.
        let mut c2 = std::collections::BTreeMap::new();
        c2.insert("a".to_string(), Nbt::Int(1));
        let value = Nbt::Compound(c2);
        let mut named = Vec::new();
        rustmc_world::nbt::write_named_root(&mut named, "", &value).expect("buf");
        assert_eq!(
            named.len(),
            out.len() + 2,
            "exactly the u16 name length shorter"
        );
        assert_eq!(named[0], out[0], "same root tag");
        assert_eq!(&named[3..], &out[1..], "same payload");
    }
}
