//! The online-mode handshake against vanilla's own packets and answers.
//!
//! `crates/rustmc-proto/golden/crypt.json` carries the encoded
//! `ClientboundHelloPacket` vanilla's `handleHello` would send with a fixed
//! key and challenge, a `ServerboundKeyPacket` body built from a ciphertext
//! the JVM confirmed decrypts, and vanilla's own `isChallengeValid` answers
//! for the right and the wrong challenge.
//!
//! # What can turn this red
//!
//! The hello's field order and widths (a `readUtf` where a byte array belongs
//! is the same first byte for an empty server id); `shouldAuthenticate` being
//! anything but true; the state machine accepting a packet out of order; the
//! challenge being checked after the secret rather than before; and
//! `isChallengeValid` throwing where vanilla returns false.
//!
//! # What cannot
//!
//! `hasJoinedServer`, which is refused, and key generation, which is not
//! implemented. Both are asserted to *fail* below, so the refusals are
//! themselves checked rather than assumed — a stub quietly appearing later
//! would redden this file.

use std::path::PathBuf;

use rustmc_proto::crypt::{parse_pkcs8_rsa_private, RsaKey};
use rustmc_server::online::{
    generate_key_pair, has_joined_server, is_valid_player_name, ClientboundHello, OnlineLogin,
    ServerboundKey, State, MAX_TICKS_BEFORE_LOGIN, SERVER_ID,
};

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rustmc-proto/golden/crypt.json");
    assert!(
        rustmc_data::fixture::available(&p, "crypt golden", "ci/crypt-golden.sh"),
        "no crypt golden"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden")
}

fn hexs(g: &serde_json::Value, k: &str) -> Vec<u8> {
    let s = g[k].as_str().unwrap_or_else(|| panic!("no {k}"));
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn key(g: &serde_json::Value) -> RsaKey {
    parse_pkcs8_rsa_private(&hexs(g, "privateKeyEncoded")).expect("parse pkcs8")
}

/// The whole exchange, from a hello to a pair of ciphers.
#[test]
fn the_handshake_reproduces_vanilla() {
    let g = golden();
    let pubkey = hexs(&g, "publicKeyEncoded");
    let challenge: [u8; 4] = hexs(&g, "challenge").try_into().unwrap();
    let mut login = OnlineLogin::new(pubkey.clone(), key(&g), challenge);
    assert_eq!(login.state(), State::Hello);
    assert_eq!(SERVER_ID, "");
    assert_eq!(MAX_TICKS_BEFORE_LOGIN, 600);

    // handleHello -> the clientbound hello, byte for byte.
    let hello = login.handle_hello("oracle").expect("hello");
    assert_eq!(login.state(), State::Key);
    assert_eq!(hello.server_id, "");
    assert!(hello.should_authenticate);
    assert_eq!(hello.challenge.len(), 4);
    let mut body = Vec::new();
    hello.write(&mut body).unwrap();
    assert_eq!(hex(&body), g["clientboundHello"].as_str().unwrap());

    // And back out of vanilla's own bytes.
    let back = ClientboundHello::read(&mut &body[..]).expect("decode hello");
    assert_eq!(back, hello);

    // handleKey, on a body built from a ciphertext the JVM confirmed decrypts
    // to the secret. Both fields are that same ciphertext, so the challenge
    // check sees a 16-byte plaintext where four bytes belong and must fail —
    // which is the first assertion, because a decryptor that ignored the
    // expected value would pass.
    let both_secret = hexs(&g, "serverboundKeyBody");
    let packet = ServerboundKey::read(&mut &both_secret[..]).expect("decode key");
    assert_eq!(packet.keybytes.len(), 128);
    assert_eq!(packet.encrypted_challenge.len(), 128);
    assert!(
        !packet.is_challenge_valid(&challenge, &key(&g)),
        "a ciphertext of the secret is not a valid answer to the challenge"
    );
    assert_eq!(
        hex(&packet.secret_key(&key(&g)).expect("secret")),
        g["secretKey"].as_str().unwrap()
    );

    // Re-encoding a body vanilla wrote must give the same bytes.
    let mut re = Vec::new();
    packet.write(&mut re).unwrap();
    assert_eq!(re, both_secret);
}

/// The state machine refuses a packet in the wrong state, with vanilla's own
/// message.
#[test]
fn the_state_machine_refuses_out_of_order_packets() {
    let g = golden();
    let challenge: [u8; 4] = hexs(&g, "challenge").try_into().unwrap();
    let body = hexs(&g, "serverboundKeyBody");
    let packet = ServerboundKey::read(&mut &body[..]).unwrap();

    // A key before a hello.
    let mut login = OnlineLogin::new(hexs(&g, "publicKeyEncoded"), key(&g), challenge);
    let err = login.handle_key(&packet).unwrap_err();
    assert!(err.to_string().contains("Unexpected key packet"), "{err}");
    assert_eq!(
        login.state(),
        State::Hello,
        "a refused packet moves nothing"
    );

    // Two hellos.
    login.handle_hello("oracle").expect("first hello");
    let err = login.handle_hello("oracle").unwrap_err();
    assert!(err.to_string().contains("Unexpected hello packet"), "{err}");
    assert_eq!(login.state(), State::Key);

    // A wrong challenge is "Protocol error", vanilla's own string.
    let err = login.handle_key(&packet).unwrap_err();
    assert_eq!(err.to_string(), "Protocol error");
    assert_eq!(
        login.state(),
        State::Key,
        "a failed key packet must not advance the state"
    );
}

/// `StringUtil.isValidPlayerName`, including the two answers that surprise.
#[test]
fn player_names() {
    assert!(is_valid_player_name("oracle"));
    assert!(
        is_valid_player_name("A1234567890_-abc"),
        "sixteen is allowed"
    );
    assert!(
        !is_valid_player_name("A1234567890_-abcd"),
        "seventeen is not"
    );
    // A space is NOT allowed here, though `isAllowedChatCharacter` next door
    // permits it: the name predicate rejects `c <= 32`.
    assert!(!is_valid_player_name("two words"));
    assert!(!is_valid_player_name("caf\u{e9}"), "non-ASCII is rejected");
    assert!(!is_valid_player_name("del\u{7f}"), "127 is rejected");
    // And there is no minimum length.
    assert!(is_valid_player_name(""), "vanilla accepts the empty name");

    let g = golden();
    let challenge: [u8; 4] = hexs(&g, "challenge").try_into().unwrap();
    let mut login = OnlineLogin::new(hexs(&g, "publicKeyEncoded"), key(&g), challenge);
    let err = login.handle_hello("two words").unwrap_err();
    assert!(err.to_string().contains("Invalid characters"), "{err}");
    assert_eq!(login.state(), State::Hello);
}

/// The two refusals, asserted as refusals.
///
/// A named refusal is only worth something if it stays one. Both of these
/// return `Err` today; a later commit that quietly replaced either with a stub
/// would turn this test red rather than turning the server permissive in
/// silence.
#[test]
fn the_refusals_are_refusals() {
    let e = has_joined_server("oracle", "6b94072f2337e37ad05c1e8a249197e7e7edfb04").unwrap_err();
    assert!(
        e.to_string().contains("hasJoinedServer is refused"),
        "the session lookup must refuse by name: {e}"
    );
    let e = generate_key_pair().unwrap_err();
    assert!(
        e.to_string().contains("not implemented"),
        "key generation must say so: {e}"
    );
}

/// Two ways to be wrong, each shown to move a captured byte.
#[test]
fn positive_control() {
    let g = golden();
    let hello = hexs(&g, "clientboundHello");
    let pubkey = hexs(&g, "publicKeyEncoded");

    // 1. The public key's length prefix is a two-byte varint, because 162 is
    //    above 127. A one-byte length would truncate the packet by one byte
    //    and still parse the first field.
    assert_eq!(pubkey.len(), 162);
    assert_eq!(&hello[1..3], &[0xA2, 0x01], "not a two-byte varint length");
    assert_eq!(hello.len(), 1 + 2 + 162 + 1 + 4 + 1);

    // 2. `shouldAuthenticate` is the last byte and it is 1. A server that
    //    passed its own `usesAuthentication()` here would write 0 on an
    //    offline server and the client would skip the session call.
    assert_eq!(*hello.last().unwrap(), 1);

    // 3. The challenge really is four bytes, not sixteen: the packet's length
    //    only works out with a four-byte one.
    let decoded = ClientboundHello::read(&mut &hello[..]).unwrap();
    assert_eq!(decoded.challenge.len(), 4);
    assert_eq!(hex(&decoded.challenge), g["challenge"].as_str().unwrap());
    assert_eq!(decoded.public_key, pubkey);
}
