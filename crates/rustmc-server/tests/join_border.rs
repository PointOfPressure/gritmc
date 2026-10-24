//! The border a joining client actually receives.
//!
//! `crates/rustmc-server/src/worldborder.rs` has been byte-exact against
//! vanilla since it landed, and until this test existed **nothing in the
//! server sent any of it**. That is the gap this file closes and the reason
//! it is a separate target: a library that encodes correctly and is never
//! called produces a client with no border, which is indistinguishable from a
//! border at its default size and so cannot be noticed by looking.
//!
//! # What is compared, and against what
//!
//! Not "an `initialize_border` frame was sent". The **body bytes**, against
//! `crates/rustmc-server/golden/world_border.json`'s `fresh` state, which is
//! the output of vanilla's own `ClientboundInitializeBorderPacket`
//! `STREAM_CODEC` on a `new WorldBorder()` (`ci/border-golden.sh`). So this
//! target fails if the packet is absent, if it is in the wrong place in the
//! join sequence, if the border it reads is not the one this process serves,
//! or if any of its eight fields moves.
//!
//! # The position in the sequence is vanilla's, read from bytecode
//!
//! `PlayerList.placeNewPlayer` calls `sendLevelInfo` once, and `sendLevelInfo`
//! sends `ClientboundInitializeBorderPacket` **first** — offset 9, ahead of
//! the clock full-sync at 31, the default spawn position at 41, the three
//! weather game events at 66..118 and `LEVEL_CHUNKS_LOAD_START` at 128. Of
//! the four in between this server sends only the weather events at join, and
//! only into a wet world, so the border sits directly after the login packet,
//! ahead of any weather event and of the game event, and
//! `world_send::tests::the_packets_go_out_in_the_order_a_client_needs` pins
//! that order.
//!
//! # The positive control is in the same test, on purpose
//!
//! [`crate::worldborder::shared`] is one process-wide cell, so a control that
//! mutates it cannot live in a second `#[test]` — cargo runs the tests in one
//! binary on parallel threads and the two would race. It is one test that
//! sends, mutates, sends again, and demands the second send differ from the
//! first and equal the mutated border's own encoding. Without it this file
//! would pass just as well against a hard-coded constant.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use rustmc_proto::{frame, ids};
use rustmc_server::worldborder;

/// A sink that is also a reader returning EOF, which is all `enter_world`
/// asks of its stream.
struct Sink(Vec<u8>);
impl Read for Sink {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn golden_fresh_bytes() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/world_border.json");
    assert!(
        rustmc_data::fixture::available(&p, "world border golden", "ci/border-golden.sh"),
        "no world border golden"
    );
    let d: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden");
    let states = d["states"].as_array().expect("states");
    let fresh = states
        .iter()
        .find(|s| s["name"] == "fresh")
        .expect("the golden's first state is `fresh`");
    fresh["packets"]["initialize_border"]
        .as_str()
        .expect("captured initialize_border bytes")
        .to_owned()
}

/// Send a join and return the `(id, body)` pairs a client would read.
fn join_frames() -> Vec<(i32, Vec<u8>)> {
    let mut sink = Sink(Vec::new());
    rustmc_server::world_send::enter_world(
        &mut sink,
        &rustmc_server::play::JoinInfo::default(),
        &[],
        (8.0, 65.0, 8.0),
        None,
        &[],
    )
    .expect("join");
    let mut cur = &sink.0[..];
    let mut out = Vec::new();
    while let Ok((id, body)) = frame::read_frame(&mut cur, false) {
        out.push((id, body));
    }
    out
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn a_joining_client_receives_vanillas_own_border_bytes() {
    let frames = join_frames();
    let (id, body) = frames
        .get(1)
        .expect("a join is more than one packet")
        .clone();
    assert_eq!(
        id,
        ids::play::cb::INITIALIZE_BORDER,
        "`sendLevelInfo` sends the border first, and this server sends nothing \
         between the login packet and it; frame ids were {:?}",
        frames.iter().map(|(i, _)| *i).collect::<Vec<_>>()
    );
    assert_eq!(
        hex(&body),
        golden_fresh_bytes(),
        "the border a client receives is not the one vanilla's own STREAM_CODEC \
         writes for a fresh border"
    );

    // The control. If this file compared against a constant rather than
    // reading `worldborder::shared`, everything above would still pass.
    {
        let mut b = worldborder::shared().lock().expect("border");
        b.set_center(137.0, -42.0);
        b.set_size(64.0);
    }
    let after = join_frames();
    let (id2, body2) = after.get(1).expect("still a join").clone();
    assert_eq!(id2, ids::play::cb::INITIALIZE_BORDER);
    assert_ne!(
        hex(&body2),
        golden_fresh_bytes(),
        "the join packet did not follow the border it is supposed to read"
    );

    let mut want = Vec::new();
    let b = worldborder::shared().lock().expect("border");
    worldborder::packets::InitializeBorder::of(&b)
        .write(&mut want)
        .expect("encode");
    assert_eq!(
        hex(&body2),
        hex(&want),
        "the join packet is not this process's border"
    );
}
