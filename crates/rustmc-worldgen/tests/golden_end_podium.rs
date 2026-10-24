//! The exit portal, against `EndPodiumFeature.place` itself.
//!
//! `data/vanilla/end_podium.json` (`ci/end-podium-golden.sh`) records every
//! `setBlock` the feature makes on a level that answers air to every read, for
//! `active` false and true at two origins. No chunk harvest can hold this
//! structure: `EnderDragonFight` builds it only once a player is in the arena.
//!
//! The inactive case is compared write for write, in order. The active case
//! places through `dropPreviousAndSetBlock`, which skips a write when the block
//! is already there, so on an all-air level vanilla records fewer writes; it is
//! compared as the world both leave behind.

use std::collections::BTreeMap;
use std::path::Path;

use rustmc_worldgen::feature::end::podium_writes;

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/vanilla/end_podium.json"
);

type Pos = (i32, i32, i32);

fn apply(writes: &[(Pos, String)]) -> BTreeMap<Pos, String> {
    let mut world = BTreeMap::new();
    for (p, s) in writes {
        world.insert(*p, s.clone());
    }
    world.retain(|_, s| s != "minecraft:air");
    world
}

#[test]
fn the_exit_portal_matches_vanillas_feature() {
    let path = Path::new(VECTORS);
    assert!(
        rustmc_data::fixture::available(path, "end podium vectors", "ci/end-podium-golden.sh"),
        "missing {VECTORS}"
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let cases = doc["cases"].as_array().expect("cases");
    assert_eq!(cases.len(), 4, "two origins x two `active` values");
    let mut portals = 0;
    for case in cases {
        let o: Vec<i32> = case["origin"]
            .as_array()
            .expect("origin")
            .iter()
            .map(|v| i32::try_from(v.as_i64().expect("int")).expect("i32"))
            .collect();
        let origin = (o[0], o[1], o[2]);
        let active = case["active"].as_bool().expect("active");
        let vanilla: Vec<(Pos, String)> = case["writes"]
            .as_array()
            .expect("writes")
            .iter()
            .map(|w| {
                let n = |i: usize| i32::try_from(w[i].as_i64().expect("int")).expect("i32");
                (
                    (n(0), n(1), n(2)),
                    w[3].as_str().expect("state").to_string(),
                )
            })
            .collect();
        let ours: Vec<(Pos, String)> = podium_writes(origin, active)
            .into_iter()
            .map(|(p, s)| (p, s.to_string()))
            .collect();
        if active {
            let (v, o) = (apply(&vanilla), apply(&ours));
            assert_eq!(
                v, o,
                "active podium at {origin:?}: the world each leaves behind"
            );
            portals += v.values().filter(|s| *s == "minecraft:end_portal").count();
        } else {
            assert_eq!(
                vanilla.len(),
                ours.len(),
                "inactive podium at {origin:?}: write count"
            );
            for (i, (v, o)) in vanilla.iter().zip(&ours).enumerate() {
                assert_eq!(v, o, "inactive podium at {origin:?}: write {i}");
            }
        }
    }
    // The 21 cells of the y=origin layer within 2.5 of the centre, less the
    // bedrock pillar's own: 20 portal blocks per active podium, read off
    // vanilla's list. A zero would mean the active case reached nothing.
    assert_eq!(
        portals,
        2 * 20,
        "each active podium holds 20 end_portal blocks"
    );
}

/// `data/vanilla/end_gateway_ring.json` (`ci/end-gateway-ring-golden.sh`):
/// the jar's own `Util.shuffle` on the fight's random at eight seeds, and
/// `spawnNewGateway`'s position for all 20 indices in Java's `Math`.
#[test]
fn the_gateway_ring_matches_the_jar() {
    use rustmc_worldgen::feature::end::{gateway_ring_position, gateway_spawn_order};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rustmc_worldgen::feature::end::GATEWAY_RING_TABLE);
    let path = path.as_path();
    assert!(
        rustmc_data::fixture::available(
            path,
            "gateway ring vectors",
            "ci/end-gateway-ring-golden.sh"
        ),
        "missing {}",
        path.display()
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json");
    let positions = json["positions"].as_array().expect("positions");
    assert_eq!(positions.len(), 20);
    for (i, p) in positions.iter().enumerate() {
        let want: Vec<i64> = p
            .as_array()
            .expect("xyz")
            .iter()
            .map(|v| v.as_i64().expect("int"))
            .collect();
        let (x, y, z) = gateway_ring_position(i32::try_from(i).expect("20 fits"));
        assert_eq!(
            vec![i64::from(x), i64::from(y), i64::from(z)],
            want,
            "ring index {i}"
        );
    }
    let cases = json["spawn_order"].as_array().expect("spawn_order");
    assert_eq!(cases.len(), 8);
    for case in cases {
        let seed: i64 = case["seed"].as_str().expect("seed").parse().expect("i64");
        let want: Vec<i32> = case["order"]
            .as_array()
            .expect("order")
            .iter()
            .map(|v| i32::try_from(v.as_i64().expect("int")).expect("fits"))
            .collect();
        assert_eq!(gateway_spawn_order(seed), want, "seed {seed}");
    }
}
