//! Online mode's handshake — `ServerLoginPacketListenerImpl`'s state machine,
//! `ClientboundHelloPacket` and `ServerboundKeyPacket`, transcribed from 26.2
//! bytecode.
//!
//! # What this is, given `docs/SERVER.md` §328
//!
//! That paragraph says offline mode only, and gives the reason: online mode is
//! "network plumbing with a hard external dependency". The plumbing half is
//! transcribable and is here — the state machine, both packet bodies, the
//! challenge, the digest, and the two ciphers. The dependency half is a live
//! HTTPS call to `sessionserver.mojang.com`, which the corpus cannot record
//! and no test in this repository could make deterministic, so it is
//! **refused by name** in [`has_joined_server`] rather than stubbed.
//!
//! That distinction is the point. A stub that returned a plausible profile
//! would make every test below green and the server would accept anybody; a
//! refusal makes the boundary a thing a caller has to handle.
//!
//! # The state machine
//!
//! Eight states, and the two that matter here are the first two.
//! `handleHello` asserts `state == HELLO` through
//! `Validate.validState(…, "Unexpected hello packet")` and `handleKey`
//! asserts `state == KEY`; a packet arriving in the wrong state is an
//! exception, not a disconnect. On a server with authentication on and a
//! non-memory connection, `handleHello` sets `KEY` and sends the clientbound
//! hello; otherwise it goes straight to `startClientVerification` with an
//! offline profile, which is the path this server takes today.
//!
//! # Three constants that are not what they look like
//!
//! * **`serverId` is `""`.** It is a `private final String` initialised to the
//!   empty string in the constructor and never assigned again. It is not the
//!   server's name and not configurable; every `digestData` call and every
//!   session lookup uses the empty string.
//! * **`shouldAuthenticate` is always `true`** in the packet the server sends
//!   — `handleHello` passes `iconst_1`. The field exists for a client to read,
//!   not for a server to vary.
//! * **The challenge is four bytes**, `Ints.toByteArray(nextInt())`, a
//!   big-endian `int`. Not sixteen, not random-length.
//!
//! # Evidence
//!
//! `crates/rustmc-proto/golden/crypt.json` carries the encoded
//! `ClientboundHelloPacket` (171 bytes for a 162-byte key), a
//! `ServerboundKeyPacket` body built from a fixed ciphertext, and vanilla's
//! own answers for `isChallengeValid` on the right and the wrong challenge.

use std::io::{self, Read, Write};

use rustmc_proto::{
    crypt::{self, Cfb8, RsaKey},
    varint::{read_varint, write_varint},
    wire,
};

/// `ServerLoginPacketListenerImpl.State`, in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Hello,
    Key,
    Authenticating,
    Negotiating,
    Verifying,
    WaitingForDupeDisconnect,
    ProtocolSwitching,
    Accepted,
}

/// `ServerLoginPacketListenerImpl.MAX_TICKS_BEFORE_LOGIN`.
pub const MAX_TICKS_BEFORE_LOGIN: i32 = 600;

/// `ServerLoginPacketListenerImpl.serverId` — the empty string, always.
pub const SERVER_ID: &str = "";

/// `ClientboundHelloPacket`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientboundHello {
    /// Read back with `readUtf(20)`, so a server id longer than 20 characters
    /// is a decode error on the client. Vanilla's is always empty.
    pub server_id: String,
    /// The X.509 `SubjectPublicKeyInfo`, verbatim from
    /// `getPublic().getEncoded()`.
    pub public_key: Vec<u8>,
    pub challenge: Vec<u8>,
    pub should_authenticate: bool,
}

fn write_byte_array(out: &mut impl Write, b: &[u8]) -> io::Result<()> {
    write_varint(out, i32::try_from(b.len()).map_err(io::Error::other)?)?;
    out.write_all(b)
}

fn read_byte_array(r: &mut impl Read) -> io::Result<Vec<u8>> {
    let n = read_varint(r)?;
    let n = usize::try_from(n).map_err(|_| io::Error::other("negative array length"))?;
    // `FriendlyByteBuf.readByteArray()` caps at the readable bytes; here the
    // cap is explicit so a hostile length cannot ask for a gigabyte.
    if n > 1 << 20 {
        return Err(io::Error::other(format!("byte array of {n} is too large")));
    }
    let mut b = vec![0u8; n];
    r.read_exact(&mut b)?;
    Ok(b)
}

impl ClientboundHello {
    /// # Errors
    /// If the sink refuses a write.
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        wire::write_string(out, &self.server_id)?;
        write_byte_array(out, &self.public_key)?;
        write_byte_array(out, &self.challenge)?;
        wire::write_bool(out, self.should_authenticate)
    }

    /// # Errors
    /// If the stream ends early.
    pub fn read(r: &mut impl Read) -> io::Result<Self> {
        Ok(Self {
            server_id: wire::read_string(r)?,
            public_key: read_byte_array(r)?,
            challenge: read_byte_array(r)?,
            should_authenticate: wire::read_bool(r)?,
        })
    }
}

/// `ServerboundKeyPacket` — two RSA ciphertexts and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerboundKey {
    pub keybytes: Vec<u8>,
    pub encrypted_challenge: Vec<u8>,
}

impl ServerboundKey {
    /// # Errors
    /// If the stream ends early.
    pub fn read(r: &mut impl Read) -> io::Result<Self> {
        Ok(Self {
            keybytes: read_byte_array(r)?,
            encrypted_challenge: read_byte_array(r)?,
        })
    }

    /// # Errors
    /// If the sink refuses a write.
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        write_byte_array(out, &self.keybytes)?;
        write_byte_array(out, &self.encrypted_challenge)
    }

    /// `ServerboundKeyPacket.isChallengeValid(byte[], PrivateKey)`.
    ///
    /// **Catches its own exception and returns false**, so a ciphertext that
    /// is not a valid PKCS#1 block is "wrong challenge" and not a crash. That
    /// is the difference between a failed login and a dropped connection, and
    /// a transcription that propagates the error gets the second.
    #[must_use]
    pub fn is_challenge_valid(&self, expected: &[u8], key: &RsaKey) -> bool {
        crypt::rsa_decrypt_pkcs1(key, &self.encrypted_challenge).is_ok_and(|got| got == expected)
    }

    /// `ServerboundKeyPacket.getSecretKey(PrivateKey)`.
    ///
    /// # Errors
    /// If the ciphertext is not a valid PKCS#1 block.
    pub fn secret_key(&self, key: &RsaKey) -> io::Result<Vec<u8>> {
        crypt::rsa_decrypt_pkcs1(key, &self.keybytes)
    }
}

/// The server's side of the exchange.
pub struct OnlineLogin {
    state: State,
    /// The X.509 encoding sent to the client and hashed into the server id.
    public_key_der: Vec<u8>,
    private_key: RsaKey,
    challenge: [u8; 4],
    requested_username: Option<String>,
}

/// What `handleKey` produces when it succeeds.
///
/// `Debug` deliberately does not print the secret: a connection log that
/// carried the AES key would hand a reader the session.
pub struct KeyAccepted {
    /// The 16-byte AES key, which is also the CFB8 IV.
    pub secret: Vec<u8>,
    /// `new BigInteger(digestData("", pub, secret)).toString(16)`.
    pub server_hash: String,
    /// `Crypt.getCipher(DECRYPT_MODE, secret)`.
    pub decrypt: Cfb8,
    /// `Crypt.getCipher(ENCRYPT_MODE, secret)`.
    pub encrypt: Cfb8,
}

impl std::fmt::Debug for KeyAccepted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyAccepted")
            .field("secret", &format_args!("<{} bytes>", self.secret.len()))
            .field("server_hash", &self.server_hash)
            .finish_non_exhaustive()
    }
}

impl OnlineLogin {
    /// A listener holding a key pair.
    ///
    /// The key is **supplied**, not generated: see [`generate_key_pair`].
    #[must_use]
    pub fn new(public_key_der: Vec<u8>, private_key: RsaKey, challenge: [u8; 4]) -> Self {
        Self {
            state: State::Hello,
            public_key_der,
            private_key,
            challenge,
            requested_username: None,
        }
    }

    #[must_use]
    pub fn state(&self) -> State {
        self.state
    }

    /// `handleHello`'s authenticating branch: record the name, move to `KEY`
    /// and hand back the packet to send.
    ///
    /// Vanilla's `Validate.validState` on `state == HELLO` is an error here
    /// rather than a panic — the assertion is the same, the reporting is not,
    /// because a panic in a connection thread reaches a client as a dropped
    /// socket and says nothing about which packet was unexpected.
    ///
    /// # Errors
    /// If the state is not `HELLO`, or the name is not a valid player name.
    pub fn handle_hello(&mut self, name: &str) -> io::Result<ClientboundHello> {
        if self.state != State::Hello {
            return Err(io::Error::other("Unexpected hello packet"));
        }
        if !is_valid_player_name(name) {
            return Err(io::Error::other("Invalid characters in username"));
        }
        self.requested_username = Some(name.to_string());
        self.state = State::Key;
        Ok(ClientboundHello {
            server_id: SERVER_ID.to_string(),
            public_key: self.public_key_der.clone(),
            challenge: self.challenge.to_vec(),
            // `iconst_1` — see the module docs.
            should_authenticate: true,
        })
    }

    /// `handleKey`, up to the point it starts the authentication thread.
    ///
    /// The order is vanilla's and it matters: the challenge is checked
    /// **before** the secret is decrypted, so a client that cannot answer the
    /// challenge never gets its key material looked at. The hash is then
    /// computed, the state moves to `AUTHENTICATING`, and only then are the
    /// two ciphers built — decrypt first, then encrypt, which is the argument
    /// order `Connection.setEncryptionKey(decrypt, encrypt)` takes.
    ///
    /// # Errors
    /// If the state is not `KEY`, the challenge is wrong ("Protocol error",
    /// vanilla's own message), or the secret is not a 16-byte AES key.
    pub fn handle_key(&mut self, packet: &ServerboundKey) -> io::Result<KeyAccepted> {
        if self.state != State::Key {
            return Err(io::Error::other("Unexpected key packet"));
        }
        if !packet.is_challenge_valid(&self.challenge, &self.private_key) {
            return Err(io::Error::other("Protocol error"));
        }
        let secret = packet.secret_key(&self.private_key)?;
        let key: [u8; 16] = secret
            .clone()
            .try_into()
            .map_err(|_| io::Error::other("Protocol error"))?;
        let server_hash = crypt::server_id_hash(SERVER_ID, &self.public_key_der, &secret);
        self.state = State::Authenticating;
        Ok(KeyAccepted {
            secret,
            server_hash,
            decrypt: Cfb8::new(&key, true),
            encrypt: Cfb8::new(&key, false),
        })
    }
}

/// `StringUtil.isValidPlayerName`.
///
/// Two things it is not. It is **not** `isAllowedChatCharacter`, which sits
/// next to it in the same class and allows `c >= 32` including the space;
/// this predicate rejects anything `<= 32` or `>= 127`, so a space is not a
/// legal player name character and a chat character is not a name character.
/// And there is **no minimum**: the filter finds nothing in the empty string,
/// so `isValidPlayerName("")` is `true`. Both were checked against
/// `lambda$isValidPlayerName$0`, which returns true for a *bad* character and
/// is consumed by `findAny().isEmpty()`.
///
/// The length is `String.length()`, UTF-16 code units. Counting `char`s here
/// rather than bytes: the two disagree only for a name holding a non-ASCII
/// character, which the filter rejects anyway, so the verdict never differs —
/// but the reason it never differs is a property of the *pair* of checks, not
/// of either one, and a byte count would be wrong on its own.
#[must_use]
pub fn is_valid_player_name(name: &str) -> bool {
    name.chars().count() <= 16 && name.chars().all(|c| c > ' ' && (c as u32) < 127)
}

/// **REFUSED.** `MinecraftSessionService.hasJoinedServer(name, hash, address)`.
///
/// This is the one part of online mode that is not transcribable. It is an
/// HTTPS request to Mojang's session server, and the answer depends on what
/// that server knows about a login that happened seconds earlier on a client
/// this repository does not run. The corpus records packet traces; it cannot
/// record a third party's database, and no golden can be taken of it.
///
/// So it is a named refusal rather than a stub. A stub returning a profile
/// would make [`OnlineLogin::handle_key`]'s callers compile, every test green,
/// and the server would admit anyone who could complete a handshake against a
/// key it published — which is exactly the failure the session lookup exists
/// to prevent. Returning an error puts the boundary where a caller has to see
/// it.
///
/// What a caller gets from this module is everything up to the call: the
/// verified challenge, the shared secret, the two ciphers, and the
/// `server_hash` string this function would send. Wiring an HTTP client to it
/// is a decision with an external dependency attached and belongs to whoever
/// takes that decision.
///
/// # Errors
/// Always.
pub fn has_joined_server(
    _username: &str,
    _server_hash: &str,
) -> io::Result<std::convert::Infallible> {
    Err(io::Error::other(
        "hasJoinedServer is refused: it is a live HTTPS call to Mojang's session \
         server, which no recording in this repository can stand in for. \
         crates/rustmc-server/src/online.rs has the whole handshake up to this point.",
    ))
}

/// **NOT IMPLEMENTED.** `Crypt.generateKeyPair()` — RSA-1024.
///
/// Unlike [`has_joined_server`] this one *could* be written and checked: a
/// generated key is verifiable without any oracle (`n == p * q`, both factors
/// pass Miller-Rabin, `e * d ≡ 1 mod λ(n)`). It is not written because it
/// needs a cryptographic random source, which means reading `/dev/urandom` and
/// deciding what a server does when that is not there, and because nothing
/// downstream can use a key yet — [`has_joined_server`] is refused, so a
/// generated key would have nowhere to go.
///
/// Said here rather than left as an absence: a reader looking for key
/// generation should find this, not silence.
///
/// # Errors
/// Always.
pub fn generate_key_pair() -> io::Result<std::convert::Infallible> {
    Err(io::Error::other(
        "RSA key generation is not implemented; supply a key with \
         OnlineLogin::new. See the doc comment for what it would take.",
    ))
}
