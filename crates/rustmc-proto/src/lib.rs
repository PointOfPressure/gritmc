//! rustmc-proto — packet codecs for the pinned protocol version, framing,
//! compression, encryption. Zero gameplay knowledge.
//!
//! The version-bump surface lives here ALONE: a Mojang protocol bump should
//! touch this crate and rustmc-data's extracted files, nothing else (that
//! property is what makes the 48h-SLA pipeline possible — VISION.md).

pub mod crypt;
pub mod frame;
pub mod ids;
pub mod varint;
pub mod wire;

/// Pinned game + protocol version. Single source of truth.
pub const GAME_VERSION: &str = "26.2";

/// Connection protocol states, vanilla's state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolState {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
}

/// FROZEN INTERFACE: a decoded packet. Concrete packet structs get a
/// serde-style derive (wave 1); this trait is what framing hands upward.
pub trait Packet: Send + std::fmt::Debug {
    fn packet_id(&self) -> i32;
    fn state(&self) -> ProtocolState;
}

/// Minecraft VarInt — the protocol's core primitive.
///
/// ```
/// assert_eq!(rustmc_proto::varint_len(0), 1);
/// assert_eq!(rustmc_proto::varint_len(128), 2);
/// assert_eq!(rustmc_proto::varint_len(-1), 5);
/// ```
pub fn varint_len(value: i32) -> usize {
    let v = value as u32;
    match 32 - v.leading_zeros() {
        0 => 1,
        bits => bits.div_ceil(7) as usize,
    }
}
