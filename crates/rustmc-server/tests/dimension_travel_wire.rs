//! What a client actually receives when it travels between dimensions.
//!
//! `crates/rustmc-server/src/dimension.rs` has carried the portal timer, the
//! coordinate scale, the border clamp and the End's constants since before
//! anything could send a `respawn` packet — the same shape `join_border.rs`
//! was written to close for the world border. A library that encodes
//! correctly and is never called produces a client that stays in the
//! overworld, which looks exactly like a portal that did not trigger.
//!
//! # What is compared, and against what
//!
//! Not "a respawn frame was sent". Three things, each of which can go wrong on
//! its own:
//!
//! * **the frame ids and their order**, against `ServerPlayer.teleport`'s own
//!   bytecode order — and in particular that `respawn` is FIRST and the border
//!   is LAST, which is the opposite of the join sequence;
//! * **the respawn body**, which must equal the target dimension's own
//!   `CommonPlayerSpawnInfo` plus the `dataToKeep` byte `iconst_3` at offset
//!   128 — so the dimension id, its registry index and its sea level are all
//!   under test rather than merely present;
//! * **the chunk batch**, which must carry the new dimension's section count.
//!   A batch of overworld-shaped chunks sent after a respawn into the nether
//!   desyncs the client's decoder rather than looking wrong.
//!
//! # The positive control
//!
//! Every assertion below is taken for BOTH targets — the nether and the End —
//! and the two must differ in the respawn body. Without that, a
//! `change_dimension` that ignored its `JoinInfo` and sent a constant would
//! satisfy every ordering and every length check in the file.

use std::io::{self, Write};

use rustmc_proto::{frame, ids};
use rustmc_server::dimension::Dimension;

struct Sink(Vec<u8>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The `JoinInfo` describing a dimension, as `createCommonSpawnInfo` would
/// fill it from that level.
fn info_for(dim: Dimension) -> rustmc_server::play::JoinInfo {
    rustmc_server::play::JoinInfo {
        dimension_type: dim.registry_index(),
        dimension: dim.id().to_string(),
        sea_level: dim.sea_level(),
        ..rustmc_server::play::JoinInfo::default()
    }
}

/// A chunk of the right shape for a dimension: every section empty, the
/// dimension's own count of them.
fn empty_chunk(dim: Dimension, cx: i32, cz: i32) -> rustmc_server::chunk::LevelChunkWithLight {
    use rustmc_server::chunk::{ContainerKind, PalettedContainer, Section};
    let sections = (0..dim.sections())
        .map(|_| Section {
            non_empty_blocks: 0,
            fluid_count: 0,
            states: PalettedContainer::from_values(ContainerKind::BlockStates, &[0; 4096]),
            biomes: PalettedContainer::from_values(ContainerKind::Biomes, &[0; 64]),
        })
        .collect();
    rustmc_server::chunk::LevelChunkWithLight {
        chunk_x: cx,
        chunk_z: cz,
        heightmaps: Vec::new(),
        sections,
        block_entities: Vec::new(),
        light: rustmc_server::chunk::LightData::default(),
    }
}

/// Travel to `dim` and return the `(id, body)` pairs a client would read.
fn travel_frames(dim: Dimension, chunks: usize) -> Vec<(i32, Vec<u8>)> {
    let cs: Vec<_> = (0..chunks)
        .map(|i| empty_chunk(dim, i32::try_from(i).unwrap_or(0), 0))
        .collect();
    let mut sink = Sink(Vec::new());
    rustmc_server::world_send::change_dimension(
        &mut sink,
        &info_for(dim),
        &cs,
        (100.5, 49.0, 0.5),
        (90.0, 0.0),
        None,
    )
    .expect("travel");
    let mut cur = &sink.0[..];
    let mut out = Vec::new();
    while let Ok((id, body)) = frame::read_frame(&mut cur, false) {
        out.push((id, body));
    }
    out
}

/// The order is `ServerPlayer.teleport`'s: respawn first, border last.
#[test]
fn the_travel_packets_go_out_in_vanillas_order() {
    for dim in [Dimension::Nether, Dimension::TheEnd] {
        let frames = travel_frames(dim, 2);
        let ids: Vec<i32> = frames.iter().map(|(i, _)| *i).collect();
        assert_eq!(
            ids,
            vec![
                ids::play::cb::RESPAWN,
                ids::play::cb::GAME_EVENT,
                ids::play::cb::SET_CHUNK_CACHE_CENTER,
                ids::play::cb::CHUNK_BATCH_START,
                ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
                ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
                ids::play::cb::CHUNK_BATCH_FINISHED,
                ids::play::cb::PLAYER_POSITION,
                ids::play::cb::INITIALIZE_BORDER,
            ],
            "{}: the dimension-change sequence",
            dim.id()
        );

        // Said again as a property rather than as a list, because the list
        // would still read as correct if somebody reordered it to match the
        // join: on a dimension change the border is LAST and at join it is
        // second.
        assert_eq!(ids[0], ids::play::cb::RESPAWN, "respawn is first");
        assert_eq!(
            *ids.last().expect("frames"),
            ids::play::cb::INITIALIZE_BORDER,
            "sendLevelInfo, and its border, is at offset 314 — after the teleport"
        );
        assert!(
            ids.iter()
                .position(|i| *i == ids::play::cb::PLAYER_POSITION)
                > ids
                    .iter()
                    .position(|i| *i == ids::play::cb::CHUNK_BATCH_FINISHED),
            "the teleport comes after the chunks that cover it"
        );
    }
}

/// The respawn body is the target dimension's spawn info plus `KEEP_ALL_DATA`.
///
/// This is where the dimension actually travels: the varint at the front is
/// the `minecraft:dimension_type` index (2 for the End, 3 for the nether) and
/// the string after it is the `ResourceKey<Level>`. A client that received the
/// overworld's would decode 24 sections out of the 16-section chunks that
/// follow.
#[test]
fn the_respawn_carries_the_target_dimension() {
    let mut bodies = Vec::new();
    for dim in [Dimension::Nether, Dimension::TheEnd, Dimension::Overworld] {
        let frames = travel_frames(dim, 0);
        let (id, body) = &frames[0];
        assert_eq!(*id, ids::play::cb::RESPAWN);

        let want = info_for(dim)
            .encode_respawn(rustmc_server::play::KEEP_ALL_DATA)
            .expect("encode");
        assert_eq!(body, &want, "{}: the respawn body", dim.id());
        assert_eq!(
            *body.last().expect("dataToKeep"),
            3,
            "{}: dataToKeep is KEEP_ALL_DATA — iconst_3 at offset 128",
            dim.id()
        );

        // The registry index really is on the wire, as the first varint.
        assert_eq!(
            i32::from(body[0]),
            dim.registry_index(),
            "{}: the dimension_type index",
            dim.id()
        );
        bodies.push(body.clone());
    }

    // The positive control: three dimensions, three different bodies. An
    // encoder that ignored its argument would pass every other assertion in
    // this file.
    assert_ne!(bodies[0], bodies[1]);
    assert_ne!(bodies[1], bodies[2]);
    assert_ne!(bodies[0], bodies[2]);
}

/// The chunks that follow the respawn are the new dimension's shape.
///
/// Decoded at the **dimension type's** section count, which is where the
/// desync would show up: a decoder handed the wrong number reads past the end
/// of the section buffer rather than returning a short chunk, so this either
/// decodes cleanly at 16 or fails.
#[test]
fn the_first_chunk_batch_is_the_new_dimensions_shape() {
    for dim in [Dimension::Nether, Dimension::TheEnd, Dimension::Overworld] {
        let frames = travel_frames(dim, 3);
        let batch: Vec<&Vec<u8>> = frames
            .iter()
            .filter(|(i, _)| *i == ids::play::cb::LEVEL_CHUNK_WITH_LIGHT)
            .map(|(_, b)| b)
            .collect();
        assert_eq!(batch.len(), 3, "{}: three chunks", dim.id());

        for body in &batch {
            let c = rustmc_server::chunk::LevelChunkWithLight::decode(body, dim.sections())
                .unwrap_or_else(|e| {
                    panic!("{}: decode at {} sections: {e}", dim.id(), dim.sections())
                });
            assert_eq!(c.sections.len(), dim.sections());
        }

        // And the count in `chunk_batch_finished` agrees, which is what the
        // client throttles on.
        let (_, fin) = frames
            .iter()
            .find(|(i, _)| *i == ids::play::cb::CHUNK_BATCH_FINISHED)
            .expect("batch finished");
        assert_eq!(fin, &vec![3u8], "{}: the batch says 3", dim.id());
    }

    // The control: a nether-shaped chunk must NOT decode as an overworld one.
    // Without this the section count could be ignored everywhere and every
    // assertion above would still hold.
    let nether = travel_frames(Dimension::Nether, 1);
    let (_, body) = nether
        .iter()
        .find(|(i, _)| *i == ids::play::cb::LEVEL_CHUNK_WITH_LIGHT)
        .expect("a chunk");
    assert!(
        rustmc_server::chunk::LevelChunkWithLight::decode(body, Dimension::Overworld.sections())
            .is_err(),
        "a 16-section chunk must not decode as 24 — if it does, the section \
         count is not being read and the wrong one would go unnoticed"
    );
}

/// The teleport carries the arrival position and rotation the End's own
/// constants give, rather than the join's zeroes.
#[test]
fn the_teleport_carries_the_arrival() {
    let frames = travel_frames(Dimension::TheEnd, 0);
    let (_, body) = frames
        .iter()
        .find(|(i, _)| *i == ids::play::cb::PLAYER_POSITION)
        .expect("a teleport");
    // varint teleport id, then 3 doubles of position.
    let mut cur = &body[..];
    let _id = rustmc_proto::varint::read_varint(&mut cur).expect("teleport id");
    let f = |c: &mut &[u8]| {
        let mut b = [0u8; 8];
        std::io::Read::read_exact(c, &mut b).expect("f64");
        f64::from_be_bytes(b)
    };
    let (x, y, z) = (f(&mut cur), f(&mut cur), f(&mut cur));
    let a = rustmc_server::dimension::end_portal::player_end_arrival();
    assert!((x - a.x).abs() < f64::EPSILON, "x {x} != {}", a.x);
    assert!((y - a.y).abs() < f64::EPSILON, "y {y} != {}", a.y);
    assert!((z - a.z).abs() < f64::EPSILON, "z {z} != {}", a.z);

    // Then the three delta doubles, then yaw and pitch.
    for _ in 0..3 {
        let _ = f(&mut cur);
    }
    let mut b = [0u8; 4];
    std::io::Read::read_exact(&mut cur, &mut b).expect("yaw");
    let yaw = f32::from_be_bytes(b);
    assert!(
        (yaw - a.y_rot).abs() < f32::EPSILON,
        "yaw {yaw} != Direction.WEST.toYRot() {}",
        a.y_rot
    );
}
