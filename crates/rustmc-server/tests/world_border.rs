//! The world border against vanilla's own numbers and vanilla's own bytes.
//!
//! `crates/rustmc-server/golden/world_border.json` is produced by
//! `ci/border-golden.sh` running `BorderGolden` inside the pinned jar: it
//! drives a real `WorldBorder` through a script of operations and, at each of
//! thirteen states, records every scalar the server has to reproduce **and**
//! the bytes vanilla's own `STREAM_CODEC` writes for all six border packets.
//!
//! # What can turn this red
//!
//! The lerp arithmetic (`shrink-t10` and `grow-t3` are both mid-lerp at
//! non-round sizes), the countdown's end condition (`shrink-t20` is the tick
//! the extent must swap itself back to static), the absolute-max clamp
//! (`clamped-absmax-64`), the previous-size lag in the edge accessors, the
//! choice of `getLerpTarget()` over `getSize()` in `set_border_size`, the
//! varlong in two packets, and every field order. [`positive_control`] shows
//! four of those going red on demand.
//!
//! # What cannot
//!
//! The script is written twice — once in Java, once below — so a state added
//! to one and not the other would silently shorten the comparison. That is why
//! the state *names* are asserted in order before any value is: a script that
//! has drifted fails on the name, naming the state, rather than passing with
//! fewer rows.

use std::path::PathBuf;

use rustmc_server::worldborder::{packets, WorldBorder};

fn golden() -> serde_json::Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/world_border.json");
    assert!(
        rustmc_data::fixture::available(&p, "world border golden", "ci/border-golden.sh"),
        "no world border golden"
    );
    serde_json::from_slice(&std::fs::read(&p).expect("read golden")).expect("parse golden")
}

/// The same script `BorderGolden.main` runs, in the same order.
fn replay() -> Vec<(&'static str, WorldBorder)> {
    let mut out = Vec::new();
    let mut b = WorldBorder::default();
    out.push(("fresh", b.clone()));

    b.set_center(137.0, -42.0);
    out.push(("center-137--42", b.clone()));

    b.set_size(64.0);
    out.push(("size-64", b.clone()));

    b.set_warning_blocks(9);
    b.set_warning_time(37);
    b.set_damage_per_block(0.35);
    b.set_safe_zone(1.25);
    out.push(("warn-9-37-dmg-0.35-safe-1.25", b.clone()));

    b.lerp_size_between(64.0, 12.0, 20, 0);
    out.push(("shrink-t0", b.clone()));
    for _ in 0..10 {
        b.tick();
    }
    out.push(("shrink-t10", b.clone()));
    for _ in 0..9 {
        b.tick();
    }
    out.push(("shrink-t19", b.clone()));
    b.tick();
    out.push(("shrink-t20", b.clone()));
    b.tick();
    out.push(("shrink-t21", b.clone()));

    b.lerp_size_between(12.0, 100.0, 7, 0);
    out.push(("grow-t0", b.clone()));
    b.tick();
    b.tick();
    b.tick();
    out.push(("grow-t3", b.clone()));

    b.lerp_size_between(50.0, 50.0, 40, 0);
    out.push(("degenerate-equal", b.clone()));

    b.set_absolute_max_size(64);
    b.set_size(1000.0);
    out.push(("clamped-absmax-64", b.clone()));

    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every scalar and every packet byte, at every state.
#[test]
fn every_border_state_matches_vanilla() {
    let g = golden();
    let states = g["states"].as_array().expect("states");
    let ours = replay();

    assert_eq!(
        states.len(),
        ours.len(),
        "the Java script has {} states and this one has {} — they have drifted",
        states.len(),
        ours.len()
    );
    for (v, (name, _)) in states.iter().zip(&ours) {
        assert_eq!(v["name"].as_str().unwrap(), *name, "state order differs");
    }

    let probe_x = f64::from_bits(g["probeXBits"].as_i64().unwrap() as u64);
    let probe_z = f64::from_bits(g["probeZBits"].as_i64().unwrap() as u64);

    let mut checked = 0usize;
    for (v, (name, b)) in states.iter().zip(&ours) {
        // Bits, never the decimal. `serde_json`'s default number parser is
        // one ULP out on a 17-significant-digit double — measured on this very
        // golden, whose `shrink-t19` size is `14.600000000000001` and comes
        // back from `as_f64()` as `14.6`. Writing more digits cannot fix that,
        // so the comparison never touches the decimal.
        let f = |k: &str| {
            f64::from_bits(
                v[&format!("{k}Bits")]
                    .as_i64()
                    .unwrap_or_else(|| panic!("{name}: no {k}Bits")) as u64,
            )
        };
        let i = |k: &str| v[k].as_i64().unwrap_or_else(|| panic!("{name}: no {k}"));

        // Bit-exact, not approximate: these are doubles vanilla produced and
        // the same expression must produce the same bits. A tolerance here
        // would hide exactly the association error `lerp` is written out to
        // avoid.
        for (k, got, want) in [
            ("centerX", b.center_x(), f("centerX")),
            ("centerZ", b.center_z(), f("centerZ")),
            ("size", b.size(), f("size")),
            ("lerpTarget", b.lerp_target(), f("lerpTarget")),
            ("lerpSpeed", b.lerp_speed(), f("lerpSpeed")),
            ("minX", b.min_x(0.0), f("minX")),
            ("minZ", b.min_z(0.0), f("minZ")),
            ("maxX", b.max_x(0.0), f("maxX")),
            ("maxZ", b.max_z(0.0), f("maxZ")),
            ("damagePerBlock", b.damage_per_block(), f("damagePerBlock")),
            ("safeZone", b.safe_zone(), f("safeZone")),
            (
                "distanceToProbe",
                b.distance_to_border(probe_x, probe_z),
                f("distanceToProbe"),
            ),
        ] {
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{name}.{k}: {got} != vanilla {want}"
            );
            checked += 1;
        }
        assert_eq!(b.lerp_time(), i("lerpTime"), "{name}.lerpTime");
        assert_eq!(
            i64::from(b.warning_blocks()),
            i("warningBlocks"),
            "{name}.warningBlocks"
        );
        assert_eq!(
            i64::from(b.warning_time()),
            i("warningTime"),
            "{name}.warningTime"
        );
        assert_eq!(
            i64::from(b.absolute_max_size()),
            i("absoluteMaxSize"),
            "{name}.absoluteMaxSize"
        );
        assert_eq!(
            b.status().name(),
            v["status"].as_str().unwrap(),
            "{name}.status"
        );
        assert_eq!(
            b.is_within_bounds(probe_x, probe_z),
            v["probeWithinBounds"].as_bool().unwrap(),
            "{name}.probeWithinBounds"
        );
        checked += 6;

        let p = &v["packets"];
        for (kind, bytes) in encode_all(b) {
            let want = p[kind]
                .as_str()
                .unwrap_or_else(|| panic!("{name}: no captured {kind}"));
            assert_eq!(hex(&bytes), want, "{name}.{kind}");
            checked += 1;
        }
    }
    assert!(
        checked >= 13 * 24,
        "only {checked} comparisons — the loop is not reaching the states"
    );
    eprintln!(
        "world border: {checked} comparisons across {} states",
        ours.len()
    );
}

/// Encode all six packets from a border.
fn encode_all(b: &WorldBorder) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    packets::InitializeBorder::of(b).write(&mut buf).unwrap();
    out.push(("initialize_border", std::mem::take(&mut buf)));
    packets::SetBorderCenter::of(b).write(&mut buf).unwrap();
    out.push(("set_border_center", std::mem::take(&mut buf)));
    packets::SetBorderLerpSize::of(b).write(&mut buf).unwrap();
    out.push(("set_border_lerp_size", std::mem::take(&mut buf)));
    packets::SetBorderSize::of(b).write(&mut buf).unwrap();
    out.push(("set_border_size", std::mem::take(&mut buf)));
    packets::SetBorderWarningDelay::of(b)
        .write(&mut buf)
        .unwrap();
    out.push(("set_border_warning_delay", std::mem::take(&mut buf)));
    packets::SetBorderWarningDistance::of(b)
        .write(&mut buf)
        .unwrap();
    out.push(("set_border_warning_distance", std::mem::take(&mut buf)));
    out
}

/// A captured packet decodes to the fields it was built from, and re-encodes
/// byte-identically.
///
/// The re-encode alone would be worth little — a reader and a writer that
/// share a wrong field order round-trip perfectly. What makes this a check is
/// that the input is *vanilla's* bytes and the decoded values are compared
/// against the state vanilla recorded beside them.
#[test]
fn captured_packets_decode_and_re_encode_identically() {
    let g = golden();
    let mut n = 0usize;
    for v in g["states"].as_array().unwrap() {
        let name = v["name"].as_str().unwrap();
        let p = &v["packets"];
        let raw = |k: &str| -> Vec<u8> {
            let s = p[k].as_str().unwrap();
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };

        let b = raw("initialize_border");
        let d = packets::InitializeBorder::read(&mut &b[..]).unwrap();
        assert_eq!(
            d.new_center_x.to_bits(),
            v["centerXBits"].as_i64().unwrap() as u64
        );
        assert_eq!(d.old_size.to_bits(), v["sizeBits"].as_i64().unwrap() as u64);
        assert_eq!(
            d.new_size.to_bits(),
            v["lerpTargetBits"].as_i64().unwrap() as u64
        );
        assert_eq!(d.lerp_time, v["lerpTime"].as_i64().unwrap());
        assert_eq!(
            i64::from(d.warning_time),
            v["warningTime"].as_i64().unwrap()
        );
        let mut re = Vec::new();
        d.write(&mut re).unwrap();
        assert_eq!(re, b, "{name}.initialize_border re-encode");

        let b = raw("set_border_lerp_size");
        let d = packets::SetBorderLerpSize::read(&mut &b[..]).unwrap();
        assert_eq!(d.lerp_time, v["lerpTime"].as_i64().unwrap());
        let mut re = Vec::new();
        d.write(&mut re).unwrap();
        assert_eq!(re, b, "{name}.set_border_lerp_size re-encode");

        let b = raw("set_border_warning_distance");
        let d = packets::SetBorderWarningDistance::read(&mut &b[..]).unwrap();
        assert_eq!(
            i64::from(d.warning_blocks),
            v["warningBlocks"].as_i64().unwrap()
        );
        let mut re = Vec::new();
        d.write(&mut re).unwrap();
        assert_eq!(re, b, "{name}.set_border_warning_distance re-encode");
        n += 3;
    }
    assert_eq!(
        n, 39,
        "expected three decodes per state over thirteen states"
    );
}

/// Four ways to be wrong, each shown to move a number the golden pins.
///
/// This exists because the two tests above are the kind that can quietly stop
/// measuring: a golden with one state, a loop that never enters, a tolerance
/// wide enough to swallow the defect. Each case below is a real transcription
/// mistake that was available while writing this module.
#[test]
fn positive_control() {
    let g = golden();
    let states = g["states"].as_array().unwrap();
    let by = |n: &str| states.iter().find(|s| s["name"] == n).unwrap();

    // 1. The lerp written the other way round. `from*(1-d) + to*d` is a
    //    different double from `from + d*(to-from)` at `grow-t3`.
    let s = by("grow-t3");
    let (from, to, d) = (12.0_f64, 100.0_f64, 3.0 / 7.0);
    let ours = from + d * (to - from);
    let other = from * (1.0 - d) + to * d;
    let want = s["sizeBits"].as_i64().unwrap() as u64;
    assert_eq!(ours.to_bits(), want);
    assert_ne!(
        other.to_bits(),
        want,
        "the two lerp associations agree here, so this control is inert"
    );

    // 2. The countdown's end condition. A `< 0` instead of `<= 0` leaves the
    //    moving extent alive for one extra tick, and at `shrink-t20` that
    //    reads as SHRINKING rather than STATIONARY.
    assert_eq!(by("shrink-t19")["status"], "SHRINKING");
    assert_eq!(by("shrink-t20")["status"], "STATIONARY");

    // 3. The absolute-max clamp. If it were dropped, the box would be
    //    `size` wide; the golden says it is not.
    let c = by("clamped-absmax-64");
    let (min_x, max_x, size) = (
        f64::from_bits(c["minXBits"].as_i64().unwrap() as u64),
        f64::from_bits(c["maxXBits"].as_i64().unwrap() as u64),
        f64::from_bits(c["sizeBits"].as_i64().unwrap() as u64),
    );
    assert!(
        max_x - min_x < size,
        "clamped box {} is not narrower than size {size}",
        max_x - min_x
    );

    // 4. `set_border_size` reads `getLerpTarget()`, not `getSize()`. On a
    //    mid-lerp state those differ, and the captured bytes say which one
    //    vanilla wrote.
    let s = by("shrink-t10");
    let mut wrong = Vec::new();
    rustmc_proto::wire::write_f64(
        &mut wrong,
        f64::from_bits(s["sizeBits"].as_i64().unwrap() as u64),
    )
    .unwrap();
    let mut right = Vec::new();
    rustmc_proto::wire::write_f64(
        &mut right,
        f64::from_bits(s["lerpTargetBits"].as_i64().unwrap() as u64),
    )
    .unwrap();
    assert_ne!(
        hex(&wrong),
        hex(&right),
        "size and lerpTarget agree at shrink-t10, so this control is inert"
    );
    assert_eq!(
        hex(&right),
        s["packets"]["set_border_size"].as_str().unwrap()
    );
}
