//! `crate::crypt` against vanilla's own `Crypt`.
//!
//! `crates/rustmc-proto/golden/crypt.json` is produced by
//! `ci/crypt-golden.sh`, which drives `net.minecraft.util.Crypt` inside the
//! pinned jar with a fixed 1024-bit RSA key and a fixed AES key. Everything
//! here is compared against what the JVM produced, not against a reading of
//! the standard.
//!
//! # What can turn this red
//!
//! The SHA-1 padding at the one/two-block boundary; the AES key schedule and
//! the S-box; the CFB8 feedback direction and its carried register; the DER
//! INTEGER's leading zero; the Montgomery reduction; the PKCS#1 block check;
//! the `serverId || secret || publicKey` hash order; and the signed,
//! zero-stripped base-16 rendering of the digest.
//!
//! # What cannot
//!
//! Key *generation*. Vanilla's `Crypt.generateKeyPair()` is a JCE call and the
//! golden pastes one key in rather than generating one, so nothing here checks
//! that this crate can make a key. It cannot; see `crates/rustmc-server/src/online.rs`
//! for what that costs and what is refused because of it.

use std::path::PathBuf;

use rustmc_proto::crypt::{
    encode_rsa_public_key, parse_pkcs8_rsa_private, rsa_decrypt_pkcs1, server_id_hash, sha1,
    Aes128, Cfb8,
};

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/crypt.json");
    assert!(
        p.exists(),
        "no crypt golden at {} — run ci/crypt-golden.sh",
        p.display()
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

#[test]
fn sha1_matches_the_jvm() {
    let g = golden();
    assert_eq!(hex(&sha1(b"")), g["sha1Empty"].as_str().unwrap());
    assert_eq!(hex(&sha1(b"abc")), g["sha1Abc"].as_str().unwrap());
    // 55 and 56 bytes bracket the padding boundary: at 56 the length no longer
    // fits in the first block and a second one is needed. An implementation
    // that always appends one block passes the 55 case and fails the 56 one.
    let long: Vec<u8> = (0u8..119).collect();
    assert_eq!(hex(&sha1(&long[..55])), g["sha1Len55"].as_str().unwrap());
    assert_eq!(hex(&sha1(&long[..56])), g["sha1Len56"].as_str().unwrap());
    assert_eq!(hex(&sha1(&long)), g["sha1Len119"].as_str().unwrap());
}

#[test]
fn aes_block_matches_the_jvm() {
    let g = golden();
    let key: [u8; 16] = hexs(&g, "secretKey").try_into().unwrap();
    let input: [u8; 16] = hexs(&g, "aesBlockIn").try_into().unwrap();
    let got = Aes128::new(&key).encrypt_block(&input);
    assert_eq!(hex(&got), g["aesBlockOut"].as_str().unwrap());
}

#[test]
fn cfb8_matches_the_jvm_whole_and_in_pieces() {
    let g = golden();
    let key: [u8; 16] = hexs(&g, "secretKey").try_into().unwrap();
    let plain = hexs(&g, "cfb8Plain");
    let want = g["cfb8Cipher"].as_str().unwrap();

    let mut buf = plain.clone();
    Cfb8::new(&key, false).apply(&mut buf);
    assert_eq!(hex(&buf), want, "one call");

    // The same split the golden used: 1, 16, and the rest. A per-call register
    // reset agrees on the first byte and diverges from the second.
    let mut c = Cfb8::new(&key, false);
    let mut split = plain.clone();
    let (a, rest) = split.split_at_mut(1);
    c.apply(a);
    let (b, tail) = rest.split_at_mut(16);
    c.apply(b);
    c.apply(tail);
    assert_eq!(
        hex(&split),
        g["cfb8CipherSplit"].as_str().unwrap(),
        "three calls"
    );
    assert_eq!(hex(&split), want, "the two must agree");

    // And the decrypt direction, on vanilla's ciphertext.
    let mut back = hexs(&g, "cfb8Cipher");
    Cfb8::new(&key, true).apply(&mut back);
    assert_eq!(hex(&back), hex(&plain));
}

#[test]
fn the_public_key_encoding_is_the_jvm_s() {
    let g = golden();
    let got = encode_rsa_public_key(&hexs(&g, "modulusHex"), &hexs(&g, "publicExponentHex"));
    assert_eq!(hex(&got), g["publicKeyEncoded"].as_str().unwrap());
    assert_eq!(g["publicKeyFormat"].as_str().unwrap(), "X.509");
    // The leading zero the signed DER INTEGER needs: an RSA modulus always has
    // its top bit set, so it is always there, and dropping it would shorten
    // the encoding by exactly one byte.
    let der = hexs(&g, "publicKeyEncoded");
    assert!(
        der.windows(2).any(|w| w == [0x02, 0x81]),
        "no long-form INTEGER in the encoding"
    );
}

#[test]
fn rsa_decrypt_matches_the_jvm() {
    let g = golden();
    let key = parse_pkcs8_rsa_private(&hexs(&g, "privateKeyEncoded")).expect("parse pkcs8");
    assert_eq!(key.size, 128, "a 1024-bit modulus is 128 bytes");
    // The parsed modulus must be the one the golden names, or the parse walked
    // to the wrong INTEGER and every decryption below would still be
    // self-consistent.
    assert_eq!(
        hex(&key.modulus.to_be_bytes(128).unwrap()),
        g["modulusHex"].as_str().unwrap()
    );
    assert_eq!(
        hex(&key.exponent.to_be_bytes(128).unwrap()),
        g["privateExponentHex"].as_str().unwrap()
    );

    let secret = rsa_decrypt_pkcs1(&key, &hexs(&g, "fixedCiphertext")).expect("decrypt");
    assert_eq!(hex(&secret), g["secretKey"].as_str().unwrap());
}

#[test]
fn the_server_hash_matches_the_jvm() {
    let g = golden();
    let pubkey = hexs(&g, "publicKeyEncoded");
    let secret = hexs(&g, "secretKey");
    let mut checked = 0usize;
    for (id, key) in [
        ("", "hash_empty"),
        ("a", "hash_a"),
        ("b", "hash_b"),
        ("notch", "hash_notch"),
        ("jeb_", "hash_jeb_"),
        ("-", "hash_dash"),
    ] {
        assert_eq!(
            server_id_hash(id, &pubkey, &secret),
            g[key].as_str().unwrap(),
            "server hash for {id:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 6);
    // The digest itself, so a hash that is right by two compensating errors
    // cannot pass.
    let mut data = secret.clone();
    data.extend_from_slice(&pubkey);
    assert_eq!(hex(&sha1(&data)), g["digest"].as_str().unwrap());
}

/// The three ways `BigInteger.toString(16)` differs from a hex encoder, each
/// demonstrated on a value the golden contains.
#[test]
fn positive_control() {
    let g = golden();

    // 1. Negative. `notch` gives a digest with its top bit set, so the hash
    //    carries a `-` and is the two's complement — not the bytes with a
    //    sign glued on.
    let notch = g["hash_notch"].as_str().unwrap();
    assert!(notch.starts_with('-'), "hash_notch is not negative");
    let digest_bytes = {
        let mut d = hexs(&g, "secretKey");
        d.extend_from_slice(&hexs(&g, "publicKeyEncoded"));
        let mut full = b"notch".to_vec();
        full.extend_from_slice(&d);
        sha1(&full)
    };
    assert!(
        digest_bytes[0] & 0x80 != 0,
        "hash_notch's digest is not negative"
    );
    assert_ne!(
        format!("-{}", hex(&digest_bytes)),
        notch,
        "a sign glued onto the hex would have been right, so this control is inert"
    );

    // 2. Leading zeros are dropped. `-` gives a 38-character hash.
    let dash = g["hash_dash"].as_str().unwrap();
    assert!(
        dash.len() < 40,
        "hash_dash is {} characters, so the leading-zero case is untested",
        dash.len()
    );

    // 3. The hash order. `serverId || publicKey || secret` — the order
    //    `digestData`'s own signature suggests — is a different digest.
    let pubkey = hexs(&g, "publicKeyEncoded");
    let secret = hexs(&g, "secretKey");
    let mut wrong = pubkey.clone();
    wrong.extend_from_slice(&secret);
    assert_ne!(hex(&sha1(&wrong)), g["digest"].as_str().unwrap());

    // 4. The CFB8 feedback direction. An encryptor that fed the plaintext byte
    //    into the register instead of the ciphertext byte agrees on byte 0.
    let key: [u8; 16] = secret.clone().try_into().unwrap();
    let plain = hexs(&g, "cfb8Plain");
    let want = hexs(&g, "cfb8Cipher");
    let mut wrongly = plain.clone();
    let cipher = Aes128::new(&key);
    let mut reg = key;
    for b in wrongly.iter_mut() {
        let ks = cipher.encrypt_block(&reg)[0];
        let feed = *b; // the mistake: plaintext, not ciphertext
        *b ^= ks;
        reg.copy_within(1..16, 0);
        reg[15] = feed;
    }
    assert_eq!(wrongly[0], want[0], "byte 0 cannot separate them");
    assert_ne!(wrongly[1], want[1], "byte 1 must");
}
