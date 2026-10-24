//! `crate::sky` against vanilla's own answer, bit for bit.
//!
//! Vectors: `crates/rustmc-server/golden/sky_vectors.json`, made by
//! `ci/sky-golden.sh` from `tools/goldengen/vanilla/SkyGolden.java`, which
//! drives vanilla's real `AttributeTrackSampler` over the real
//! `minecraft:timeline` registry.
//!
//! **The comparison is on raw float bits and there is no tolerance**, which
//! is not pedantry. `DaylightDetectorBlock.updateSignalStrength` computes
//! `Math.round(power * Mth.cos(angle))`, so the answer is an integer with
//! fifteen boundaries inside the range, and `Mth.cos` quantises its argument
//! to one of 65,536 table slots. A last-bit disagreement in the angle lands
//! on the other side of a slot boundary often enough to move the rounded
//! result — an epsilon here would hide exactly the defect the file exists to
//! catch.
//!
//! What this does NOT cover is stated in `SkyGolden.java`'s header:
//! `updateSignalStrength` itself is `private static` and needs a real
//! `ServerLevel`, so the six lines of integer arithmetic on top of these
//! inputs are covered by the corpus and not here.

use rustmc_server::sky::{mth_cos, SkyModel};

const GOLDEN: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/golden/sky_vectors.json");

fn repo(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn bits(v: &serde_json::Value, key: &str) -> f32 {
    #[allow(clippy::cast_possible_truncation)]
    let raw = v[key].as_i64().expect("a raw-bits integer") as i32;
    f32::from_bits(raw as u32)
}

/// The extract and the golden are made from the same pinned jar by two
/// unrelated routes — a datagen dump and a live registry walk — so a
/// disagreement is real rather than a transcription slip in one of them.
#[test]
fn sun_angle_and_sky_light_level_match_the_jvm() {
    let table = repo("data/vanilla/timelines.json");
    if !rustmc_data::fixture::available(
        &table,
        "vanilla timelines extract",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let sky = SkyModel::load(&table, "minecraft:overworld")
        .expect("timelines.json is present but did not load");
    let golden: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(GOLDEN).expect("golden vectors"))
            .expect("golden vectors parse");
    let samples = golden["samples"].as_array().expect("samples");
    assert!(
        samples.len() > 400,
        "the vector table shrank to {} — regenerate it rather than weakening the test",
        samples.len()
    );

    let mut angle_bad = Vec::new();
    let mut level_bad = Vec::new();
    let mut darken_bad = Vec::new();
    let mut cos_bad = Vec::new();
    for s in samples {
        let t = s["ticks"].as_i64().expect("ticks");
        let want_angle = bits(s, "sun_angle");
        let want_level = bits(s, "sky_light_level");
        let want_cos = bits(s, "mth_cos");
        let want_pulled = bits(s, "pulled_radians");
        let want_darken = s["sky_darken"].as_i64().expect("sky_darken");

        let got_angle = sky.sun_angle_degrees(t).expect("the overworld has a sun");
        if got_angle.to_bits() != want_angle.to_bits() {
            angle_bad.push((t, want_angle, got_angle));
        }
        let got_level = sky.sky_light_level(t);
        if got_level.to_bits() != want_level.to_bits() {
            level_bad.push((t, want_level, got_level));
        }
        if i64::from(sky.sky_darken(t)) != want_darken {
            darken_bad.push((t, want_darken, sky.sky_darken(t)));
        }
        // The cosine is checked at vanilla's own pulled argument, so a
        // disagreement here is the table and not the pull.
        let got_cos = mth_cos(f64::from(want_pulled));
        if got_cos.to_bits() != want_cos.to_bits() {
            cos_bad.push((t, want_cos, got_cos));
        }
    }
    assert!(
        angle_bad.is_empty(),
        "sun angle differs from the JVM at {} of {} ticks; first: {:?}",
        angle_bad.len(),
        samples.len(),
        &angle_bad[..angle_bad.len().min(5)]
    );
    assert!(
        level_bad.is_empty(),
        "sky light level differs at {} ticks; first: {:?}",
        level_bad.len(),
        &level_bad[..level_bad.len().min(5)]
    );
    assert!(
        darken_bad.is_empty(),
        "skyDarken differs at {} ticks; first: {:?}",
        darken_bad.len(),
        &darken_bad[..darken_bad.len().min(5)]
    );
    assert!(
        cos_bad.is_empty(),
        "Mth.cos differs at {} ticks; first: {:?}",
        cos_bad.len(),
        &cos_bad[..cos_bad.len().min(5)]
    );
}

/// The nether and the end carry no `visual/sun_angle` track, and the honest
/// answer there is "no sun", not zero — zero is midday.
#[test]
fn a_dimension_with_no_day_timeline_has_no_sun_angle() {
    let table = repo("data/vanilla/timelines.json");
    if !rustmc_data::fixture::available(
        &table,
        "vanilla timelines extract",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let nether = SkyModel::load(&table, "minecraft:the_nether")
        .expect("timelines.json is present but did not load");
    assert_eq!(nether.sun_angle_degrees(0), None);
    assert!(!nether.has_skylight());
    // **The base value is the dimension's own attribute map, not the
    // attribute's registered default**, and this assertion was written the
    // other way round and refuted by the extract. `minecraft:the_nether`
    // carries a static `minecraft:gameplay/sky_light_level` of **4.0**, so
    // its `skyDarken` is `(int)(15 - 4) = 11` at every tick of every day — no
    // timeline, no variation. A port that read only the timeline tracks would
    // have answered 0 here and looked entirely reasonable doing it.
    assert_eq!(nether.sky_darken(0), 11);
    assert_eq!(nether.sky_darken(18_000), 11);

    // The end carries no such static, so it falls to the registered default
    // of 15 and its `skyDarken` really is 0.
    let end = SkyModel::load(&table, "minecraft:the_end").expect("the end");
    assert_eq!(end.sun_angle_degrees(0), None);
    assert_eq!(end.sky_darken(0), 0);
    assert_eq!(end.sky_darken(18_000), 0);
}

/// `loopTicks` is `Math.floorMod`, which is not `%`. A negative day time —
/// which `/time add` can produce — must wrap forward, not mirror.
#[test]
fn the_day_wraps_with_floor_mod_not_remainder() {
    let table = repo("data/vanilla/timelines.json");
    if !rustmc_data::fixture::available(
        &table,
        "vanilla timelines extract",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let sky = SkyModel::load(&table, "minecraft:overworld")
        .expect("timelines.json is present but did not load");
    for t in [0_i64, 1, 6000, 12_345, 23_999] {
        assert_eq!(
            sky.sun_angle_degrees(t).unwrap().to_bits(),
            sky.sun_angle_degrees(t - 24_000).unwrap().to_bits(),
            "tick {t} and {} must be the same point of the day",
            t - 24_000
        );
        assert_eq!(
            sky.sun_angle_degrees(t).unwrap().to_bits(),
            sky.sun_angle_degrees(t + 240_000).unwrap().to_bits(),
        );
    }
}
