//! `crate::effects::TickRule` against vanilla's own answers.
//!
//! # What this is, and what makes it able to fail
//!
//! `shouldApplyEffectTickThisTick` is a program, so it is ported rather than
//! tabled — see `crates/rustmc-server/src/effects.rs`. A port checked only by
//! unit tests written from the same `javap` dump is checked against a second
//! reading by the same pair of eyes, which is the defect class
//! `docs/AGENT-BRIEF.md` opens with.
//!
//! So `ci/mob-effects-golden.sh` **probes** the real method — every effect in
//! the registry, durations 0..=120, amplifiers 0..=6 and 32 — and this file
//! compares the port to that probe cell by cell. 40 effects x 8 amplifiers x
//! 121 durations is 38,720 individual answers, and a disagreement in any one
//! of them names the effect, the amplifier and the duration.
//!
//! The two halves are independent in the way that matters: one is a Java call
//! into the pinned jar, the other is Rust written from bytecode. Neither
//! consults the other.
//!
//! # Three things that stop this passing vacuously
//!
//! A cell-by-cell comparison against an empty table passes. Against a table
//! where every effect answers `false` it also passes, because most of them do.
//! So the assertions below additionally require that:
//!
//! * the table holds the number of effects it says it does, and that number is
//!   not zero;
//! * **more than one distinct rule** is exercised — a table in which every
//!   effect never fired would agree with a port that had been deleted;
//! * the two periodic effects whose behaviour differs from every other row
//!   (`minecraft:poison` at amplifier 6, which fires every tick, and at
//!   amplifier 32, which fires every 25th) are both present and both
//!   distinguished.
//!
//! Skipped without the table on disk, like every other golden-backed test in
//! this crate.

use std::collections::BTreeSet;
use std::path::PathBuf;

use rustmc_server::effects::{rule_for_class, TickRule, TABLE_PATH};

fn repo(relative: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn table() -> Option<serde_json::Value> {
    let path = repo(TABLE_PATH);
    if !rustmc_data::fixture::available(&path, "mob effect table", "ci/mob-effects-golden.sh") {
        return None;
    }
    let raw = std::fs::read_to_string(&path).expect("the mob effect table is readable");
    Some(serde_json::from_str(&raw).expect("the mob effect table is JSON"))
}

#[test]
fn every_probed_cell_agrees_with_the_transcribed_rule() {
    let Some(doc) = table() else {
        return;
    };
    let rows = doc["table"].as_array().expect("a table array");
    let declared = doc["effects"].as_u64().expect("an effect count");
    assert_eq!(
        rows.len() as u64,
        declared,
        "the table's own count disagrees with its rows"
    );
    assert!(declared > 0, "an empty table would pass every comparison");

    let mut rules_seen: BTreeSet<String> = BTreeSet::new();
    let mut cells = 0_u64;
    let mut fired = 0_u64;

    for row in rows {
        let id = row["id"].as_str().expect("an effect id");
        let class = row["class"].as_str().expect("a class name");
        let rule = rule_for_class(class).unwrap_or_else(|| {
            panic!(
                "{id} is a {class}, which crate::effects::rule_for_class does not know. \
                 A new MobEffect subclass must be read and given a TickRule variant; \
                 falling back to Never would make it silently inert."
            )
        });
        rules_seen.insert(format!("{rule:?}"));

        for probe in row["tick"].as_array().expect("a tick probe array") {
            let amplifier = probe["amplifier"].as_i64().expect("an amplifier") as i32;
            let hits: BTreeSet<i32> = probe["firesAt"]
                .as_array()
                .expect("a firesAt array")
                .iter()
                .map(|v| v.as_i64().expect("a duration") as i32)
                .collect();
            let max = doc["maxProbedDuration"].as_i64().expect("a probe bound") as i32;
            for duration in 0..=max {
                let vanilla = hits.contains(&duration);
                let ours = rule.fires(duration, amplifier);
                assert_eq!(
                    ours, vanilla,
                    "{id} ({class}, {rule:?}): shouldApplyEffectTickThisTick(\
                     duration={duration}, amplifier={amplifier}) is {vanilla} in the jar \
                     and {ours} here"
                );
                cells += 1;
                if vanilla {
                    fired += 1;
                }
            }
        }
    }

    // The vacuity guards. Reported rather than merely asserted, so a run that
    // is thinner than it should be is visible in the log.
    eprintln!(
        "mob effect tick rules: {cells} cells compared over {} effects, \
         {fired} of them firing, {} distinct rules exercised",
        rows.len(),
        rules_seen.len()
    );
    assert!(cells > 10_000, "only {cells} cells — the probe is too thin");
    assert!(
        fired > 0,
        "no cell in the whole table fires — the port would agree with anything"
    );
    assert!(
        rules_seen.len() >= 4,
        "only {} distinct rules exercised ({rules_seen:?}) — this cannot separate \
         a real port from one that always answers the majority",
        rules_seen.len()
    );
}

/// The count in `effects.rs`'s module docs, taken from the table rather than
/// from a reading. It said eight before the probe existed.
#[test]
fn ten_of_the_forty_effects_ever_reach_apply_effect_tick() {
    let Some(doc) = table() else {
        return;
    };
    assert_eq!(doc["effects"].as_u64(), Some(40));
    assert_eq!(doc["everTicks"].as_u64(), Some(10));
    assert_eq!(doc["instantaneous"].as_u64(), Some(3));
}

/// The two cells that separate a correct periodic rule from both plausible
/// wrong ones, named so a thinner table cannot lose them silently.
#[test]
fn poison_distinguishes_the_shift_to_zero_arm_from_the_masked_shift() {
    let Some(doc) = table() else {
        return;
    };
    let row = doc["table"]
        .as_array()
        .expect("a table array")
        .iter()
        .find(|r| r["id"] == "minecraft:poison")
        .expect("minecraft:poison is in the registry");

    let hits = |amplifier: i64| -> BTreeSet<i32> {
        row["tick"]
            .as_array()
            .expect("a tick probe array")
            .iter()
            .find(|p| p["amplifier"] == amplifier)
            .unwrap_or_else(|| panic!("no probe at amplifier {amplifier}"))["firesAt"]
            .as_array()
            .expect("a firesAt array")
            .iter()
            .map(|v| v.as_i64().expect("a duration") as i32)
            .collect()
    };

    // 25 >> 6 == 0, so every tick.
    assert_eq!(hits(6).len(), 121, "poison VII does not fire every tick");
    // 25 >> 32 == 25 in Java, so exactly amplifier 0's answer.
    assert_eq!(
        hits(32),
        hits(0),
        "Java's five-bit shift mask is not reproduced by the probe"
    );
    assert_ne!(hits(6), hits(0), "the two arms are indistinguishable here");

    let rule = TickRule::Periodic { base: 25 };
    assert!(rule.fires(7, 6));
    assert!(!rule.fires(7, 32));
}
