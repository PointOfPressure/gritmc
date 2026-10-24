//! Vanilla's own `ClientboundExplodePacket.STREAM_CODEC`, against ours.
//!
//! `data/vanilla/explosion_wire.json` is recorded by
//! `ci/explosion-wire-golden.sh`, which builds a real
//! `RegistryFriendlyByteBuf` and calls the codec. So these are the bytes
//! vanilla puts on the socket, byte for byte, and not a reading of the codec
//! composition.
//!
//! The one that matters most is `blockCount`. It goes through
//! `ByteBufCodecs.INT` — `ByteBuf.writeInt`, four fixed bytes — and every
//! other integer on this packet is a varint, so the natural mistake is to
//! reach for `write_varint`. The golden holds two cases differing only in that
//! field, and a varint port gets a shorter packet for one of them.

use rustmc_server::explosion::{explode_body, BlockParticle};
use std::path::{Path, PathBuf};

/// The whole path in one literal, so `ci/gen-gameplay-tables.sh`'s grep sees
/// it and `ci/check-generator-arms.sh` finds the arm reached.
const WIRE_TABLE: &str = "data/vanilla/explosion_wire.json";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn our_explode_packet_is_vanillas_byte_for_byte() {
    let p = repo_root().join(WIRE_TABLE);
    if !rustmc_data::fixture::available(Path::new(&p), WIRE_TABLE, "ci/explosion-wire-golden.sh") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&p).expect("read")).expect("json");

    // The id this packet is sent under, from the same file, so a version bump
    // that moves it fails here rather than silently sending an `entity_event`.
    assert_eq!(
        doc["packetId"].as_i64(),
        Some(i64::from(rustmc_proto::ids::play::cb::EXPLODE)),
        "ids.rs and the golden disagree on the explode packet id"
    );

    let ids = &doc["particleIds"];
    let poof = i32::try_from(ids["minecraft:poof"].as_i64().expect("poof")).expect("fits");
    let smoke = i32::try_from(ids["minecraft:smoke"].as_i64().expect("smoke")).expect("fits");
    let sound = i32::try_from(doc["soundId"].as_i64().expect("soundId")).expect("fits");

    let defaults = rustmc_server::explosion::default_block_particles(poof, smoke);
    let none: [BlockParticle; 0] = [];

    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "no cases — the golden is empty");
    let mut compared = 0usize;
    let mut bytes_seen = 0usize;

    for c in cases {
        let name = c["name"].as_str().expect("name");
        let want = c["bytes"].as_str().expect("bytes");
        let cb = c["centerBits"].as_array().expect("centerBits");
        #[allow(clippy::cast_sign_loss)]
        let center = (
            f64::from_bits(cb[0].as_i64().expect("cx") as u64),
            f64::from_bits(cb[1].as_i64().expect("cy") as u64),
            f64::from_bits(cb[2].as_i64().expect("cz") as u64),
        );
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let radius = f32::from_bits(c["radiusBits"].as_i64().expect("radiusBits") as u32);
        let block_count =
            i32::try_from(c["blockCount"].as_i64().expect("blockCount")).expect("fits");
        #[allow(clippy::cast_sign_loss)]
        let knockback = c["knockbackBits"].as_array().map(|k| {
            (
                f64::from_bits(k[0].as_i64().expect("kx") as u64),
                f64::from_bits(k[1].as_i64().expect("ky") as u64),
                f64::from_bits(k[2].as_i64().expect("kz") as u64),
            )
        });
        let particle = i32::try_from(
            ids[c["particle"].as_str().expect("particle")]
                .as_i64()
                .expect("particle id"),
        )
        .expect("fits");
        let particles: &[BlockParticle] = if c["blockParticleCount"].as_u64() == Some(0) {
            &none
        } else {
            &defaults
        };

        let got = explode_body(
            center,
            radius,
            block_count,
            knockback,
            particle,
            sound,
            particles,
        )
        .expect("encodes");

        assert_eq!(hex(&got), want, "{name}: the encoded body diverged");
        assert_eq!(
            got.len(),
            usize::try_from(c["length"].as_u64().expect("length")).expect("fits"),
            "{name}: length"
        );
        compared += 1;
        bytes_seen += got.len();
    }

    // The instrument's own floor: every assertion above passes vacuously over
    // an empty case list, so the totals are asserted against what
    // `ci/explosion-wire-golden.sh` printed when the table was recorded.
    assert_eq!(compared, 5, "the golden lost a case");
    assert_eq!(bytes_seen, 289, "the golden's total byte count moved");
}

/// `blockCount` is four fixed bytes, so two packets differing only in it are
/// the same length. Asserted here as well as in the generator because this is
/// the check the *port* can fail — the generator's version can only catch
/// vanilla changing.
#[test]
fn block_count_is_four_fixed_bytes_not_a_varint() {
    let long = explode_body((0.5, 65.5, 0.5), 4.0, 1120, None, 29, 699, &[]).expect("encodes");
    let zero = explode_body((0.5, 65.5, 0.5), 4.0, 0, None, 29, 699, &[]).expect("encodes");
    assert_eq!(
        long.len(),
        zero.len(),
        "blockCount changed the packet's length, so it was written as a varint"
    );
    assert_ne!(long, zero, "blockCount did not reach the wire at all");
    // And the four bytes are where the field is: after 24 bytes of centre and
    // 4 of radius.
    assert_eq!(&long[28..32], &1120i32.to_be_bytes());
    assert_eq!(&zero[28..32], &[0, 0, 0, 0]);
}
