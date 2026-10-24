//! The login state — offline mode, no encryption.
//!
//! Reference (26.2, read from the jar's bytecode, never transcribed):
//! `ServerboundHelloPacket`, `ClientboundLoginFinishedPacket`,
//! `ByteBufCodecs.GAME_PROFILE`, with ids from [`rustmc_proto::ids`].
//!
//! ```text
//!   C->S  login/hello             name, profile uuid
//!   S->C  login/login_finished    uuid, name, properties[], session uuid
//!   C->S  login/login_acknowledged  (empty)  -> configuration state
//! ```
//!
//! # Offline mode only, and deliberately
//!
//! Online mode needs `login/hello` (clientbound), an RSA key exchange, an
//! `encryption_response`, a session-server call and AES on every subsequent
//! packet. None of that is worldgen or gameplay, all of it is network plumbing
//! with a hard external dependency, and skipping it costs nothing that this
//! project is trying to prove. The UUID therefore comes from
//! [`rustmc_proto::wire::offline_uuid`], which is vanilla's own
//! `OfflinePlayer:<name>` MD5 scheme — so a player keeps the same identity
//! that a vanilla offline server would give them.
//!
//! # The trailing session UUID is new in 26.2
//!
//! `ClientboundLoginFinishedPacket`'s codec is
//! `composite(GAME_PROFILE, ..., UUIDUtil.STREAM_CODEC, ...)` — the profile
//! *and then a second UUID*. Older protocol notes describe this packet as
//! ending after the properties array, which would leave the client waiting on
//! sixteen bytes that never arrive. Read from the bytecode for that reason.

use std::io::{self, Read, Write};

use rustmc_proto::{frame, ids, varint::read_varint, wire};

/// A player as far as login is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub uuid: u128,
}

/// A decoded `login/hello`.
#[derive(Debug, Clone)]
pub struct Hello {
    pub name: String,
    /// What the client claims. Ignored for identity — see [`Profile::offline`]
    /// — but carried so a caller can log the mismatch.
    pub claimed_uuid: u128,
}

impl Profile {
    /// `OfflinePlayer:<name>` MD5, the identity a vanilla offline server
    /// assigns.
    #[must_use]
    pub fn offline(name: &str) -> Self {
        Self {
            name: name.to_string(),
            uuid: wire::offline_uuid(name),
        }
    }
}

/// Read `login/hello`.
///
/// # Errors
/// If the packet is not `hello`, or the stream ends.
pub fn read_hello(r: &mut impl Read, comp: Option<i32>) -> io::Result<Hello> {
    let (id, body) = frame::read_frame(r, comp.is_some())?;
    if id != ids::login::sb::HELLO {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected login/hello, got {id:#04x}"),
        ));
    }
    let mut cur = &body[..];
    let name = wire::read_string(&mut cur)?;
    let claimed_uuid = wire::read_u128(&mut cur)?;
    Ok(Hello { name, claimed_uuid })
}

/// Send `login/login_finished` and wait for the client's acknowledgement.
///
/// Returns once the client has moved to the configuration state.
///
/// # Errors
/// On I/O failure, or if the client sends something other than
/// `login_acknowledged`.
pub fn finish(
    stream: &mut (impl Read + Write),
    profile: &Profile,
    comp: Option<i32>,
) -> io::Result<()> {
    let mut out = Vec::new();
    wire::write_u128(&mut out, profile.uuid)?;
    wire::write_string(&mut out, &profile.name)?;
    // No properties: those carry skin textures, which only a session-server
    // lookup provides and offline mode has none. An empty array is correct
    // rather than a placeholder — a client shown a fabricated texture
    // property renders a broken skin instead of the default one.
    rustmc_proto::varint::write_varint(&mut out, 0)?;
    // The session UUID. See the module docs: this field is easy to omit and
    // the failure is a client that hangs rather than one that errors.
    wire::write_u128(&mut out, profile.uuid)?;
    frame::write_frame(stream, comp, ids::login::cb::LOGIN_FINISHED, &out)?;

    let (id, _) = frame::read_frame(stream, comp.is_some())?;
    if id != ids::login::sb::LOGIN_ACKNOWLEDGED {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected login_acknowledged, got {id:#04x}"),
        ));
    }
    Ok(())
}

/// Turn compression on: `login/login_compression`, then every frame after it
/// — in **both** directions — carries the compressed form.
///
/// Sent *before* `login_finished`, which is vanilla's order and means that
/// packet is itself already compressed. Getting that backwards gives a client
/// a compressed frame it is not yet inflating, which it reads as a length
/// followed by garbage.
///
/// # Errors
/// On I/O failure.
pub fn enable_compression(stream: &mut impl Write, threshold: i32) -> io::Result<()> {
    let mut out = Vec::new();
    rustmc_proto::varint::write_varint(&mut out, threshold)?;
    // Sent uncompressed: it is the packet that turns compression on.
    frame::write_frame(stream, None, ids::login::cb::LOGIN_COMPRESSION, &out)
}

/// Refuse a login with a reason the client will display.
///
/// # Errors
/// On I/O failure.
pub fn disconnect(stream: &mut impl Write, reason: &str, comp: Option<i32>) -> io::Result<()> {
    let mut out = Vec::new();
    // The login-state disconnect carries a *JSON* chat component, unlike the
    // play-state one which carries NBT. Sending the wrong shape here shows the
    // player a parse error instead of the reason they were refused.
    wire::write_string(
        &mut out,
        &format!(r#"{{"text":"{}"}}"#, reason.replace('"', "'")),
    )?;
    frame::write_frame(stream, comp, ids::login::cb::LOGIN_DISCONNECT, &out)
}

/// Read a varint from a slice — exposed for callers driving the state machine.
///
/// # Errors
/// If the slice is too short.
pub fn peek_varint(mut b: &[u8]) -> io::Result<i32> {
    read_varint(&mut b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello_bytes(name: &str) -> Vec<u8> {
        let mut body = Vec::new();
        wire::write_string(&mut body, name).expect("buf");
        wire::write_u128(&mut body, 0).expect("buf");
        let mut out = Vec::new();
        frame::write_frame(&mut out, None, ids::login::sb::HELLO, &body).expect("buf");
        frame::write_frame(&mut out, None, ids::login::sb::LOGIN_ACKNOWLEDGED, &[]).expect("buf");
        out
    }

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

    #[test]
    fn a_login_produces_the_offline_identity_and_the_session_uuid() {
        let mut pipe = Pipe {
            input: std::io::Cursor::new(hello_bytes("Notch")),
            output: Vec::new(),
        };
        let hello = read_hello(&mut pipe, None).expect("hello");
        assert_eq!(hello.name, "Notch");

        let profile = Profile::offline(&hello.name);
        finish(&mut pipe, &profile, None).expect("finished");

        let (id, body) = frame::read_frame(&mut &pipe.output[..], false).expect("login_finished");
        assert_eq!(id, ids::login::cb::LOGIN_FINISHED);

        let mut cur = &body[..];
        assert_eq!(wire::read_u128(&mut cur).expect("uuid"), profile.uuid);
        assert_eq!(wire::read_string(&mut cur).expect("name"), "Notch");
        assert_eq!(
            read_varint(&mut cur).expect("props"),
            0,
            "no properties offline"
        );
        // The field older protocol notes omit. Its absence hangs a client.
        assert_eq!(
            wire::read_u128(&mut cur).expect("session uuid"),
            profile.uuid,
            "login_finished ends with a session UUID in 26.2"
        );
        assert!(cur.is_empty(), "nothing trailing: {} bytes left", cur.len());
    }

    /// The offline UUID is vanilla's, not ours to invent — a player must keep
    /// the identity a vanilla offline server would give them.
    #[test]
    fn the_offline_uuid_is_vanillas_scheme() {
        let a = Profile::offline("Notch");
        let b = Profile::offline("Notch");
        assert_eq!(a.uuid, b.uuid, "stable for a name");
        assert_ne!(a.uuid, Profile::offline("notch").uuid, "case matters");
        // Version 3 (MD5) and RFC 4122 variant, which is what
        // `UUID.nameUUIDFromBytes` produces.
        let bytes = a.uuid.to_be_bytes();
        assert_eq!(bytes[6] >> 4, 3, "version nibble");
        assert_eq!(bytes[8] >> 6, 0b10, "variant bits");
    }
}
