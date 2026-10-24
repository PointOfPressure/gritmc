//! `Level.getBlockRandomPos` against vanilla running it.
//!
//! The vectors in `tests/golden/random_tick_pos.json` are produced by
//! `ci/random-tick-golden.sh`, which calls the method on a real
//! `net.minecraft.world.level.Level` and records what it returned. So this is
//! a comparison against vanilla's bytecode, not against a reading of it.
//!
//! # Why a golden and not a property test
//!
//! Because the defect this exists to catch is invisible to every property the
//! function has. `getBlockRandomPos` builds `y` from bits 16.. and `z` from
//! bits 8.., and a port that swaps them produces positions that are uniform
//! over the section, inside the section, at the right rate, with the right
//! generator advance. `randomtick::tests::every_pick_is_inside_its_own_section`
//! passes with the swap in place — it is written down there saying so.
//!
//! Only the values separate them, and the values only mean something if
//! vanilla produced them.
//!
//! # What a red here means
//!
//! The transcription, or the golden being stale against a new pinned version.
//! In the second case the whole file changes at once, which is easy to tell
//! from the first — a transcription error moves one component of every
//! position and leaves the others alone.

use rustmc_server::randomtick::BlockRandomPos;

/// The golden lives in `crates/rustmc-server/golden/` — the tree's own
/// convention for vectors dumped out of Java — rather than under
/// `data/vanilla/`, and
/// the distinction is load-bearing. `data/vanilla` holds **extracts the
/// server reads at runtime**, which are regenerated per tree and stamped with
/// their generating tree's provenance because a stale one silently changes
/// what the server does. This is a fixture that is only ever compared
/// against: a stale one turns a test red and can do nothing else. So it is
/// committed, and `ci/gen-gameplay-tables.sh` has no business regenerating
/// it.
const GOLDEN: &str = include_str!("../golden/random_tick_pos.json");

#[test]
fn the_position_function_reproduces_vanillas_own_draws() {
    let doc: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden parses");
    assert_eq!(
        doc["rng"].as_str(),
        Some("random_tick_pos"),
        "wrong golden file"
    );
    let cases = doc["cases"].as_array().expect("cases array");
    assert!(!cases.is_empty(), "a golden with no cases cannot fail");

    let mut compared = 0usize;
    for case in cases {
        let x = case["x"].as_i64().expect("x") as i32;
        let y = case["y"].as_i64().expect("y") as i32;
        let z = case["z"].as_i64().expect("z") as i32;
        let mask = case["mask"].as_i64().expect("mask") as i32;
        let seed = case["seed"].as_i64().expect("seed") as i32;

        let mut rng = BlockRandomPos::at(seed);
        let positions = case["positions"].as_array().expect("positions");
        assert!(
            !positions.is_empty(),
            "case seed {seed} has no draws — an empty case compares nothing"
        );
        for (i, want) in positions.iter().enumerate() {
            let w = want.as_array().expect("position triple");
            let want = (
                w[0].as_i64().expect("wx") as i32,
                w[1].as_i64().expect("wy") as i32,
                w[2].as_i64().expect("wz") as i32,
            );
            let got = rng.next(x, y, z, mask);
            assert_eq!(
                got, want,
                "draw {i} of case (x={x}, y={y}, z={z}, mask={mask}, seed={seed})"
            );
            compared += 1;
        }
        // The state the generator is left holding. A port can agree on every
        // position above and still advance `randValue` by the wrong amount —
        // it would show on the draw after the last compared one, which is
        // exactly the draw a golden of fixed length never reaches.
        assert_eq!(
            rng.value(),
            case["endValue"].as_i64().expect("endValue") as i32,
            "randValue after case (x={x}, y={y}, z={z}, mask={mask}, seed={seed})"
        );
    }

    // The golden is a file on disk and a test that silently compared nothing
    // would be green. This is the count the run has to be able to quote.
    assert!(
        compared >= 100,
        "only {compared} positions compared — the golden is thinner than it looks"
    );
    println!(
        "compared {compared} vanilla-drawn positions across {} cases",
        cases.len()
    );
}
