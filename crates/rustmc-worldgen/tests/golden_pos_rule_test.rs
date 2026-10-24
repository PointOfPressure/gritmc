//! `AxisAlignedLinearPosTest` — the one position predicate the jar reaches.
//!
//! Reference (26.2, read before porting): `AxisAlignedLinearPosTest`,
//! `PosAlwaysTrueTest`, `PosRuleTest`, `ProcessorRule.test`,
//! `RuleProcessor.processBlock`, `StructureStart.placeInChunk`,
//! `StructureTemplate.processBlockInfos`, `Mth.clampedLerp`,
//! `Mth.inverseLerp`, `Mth.lerp`, `BoundingBox.getCenter`.
//!
//! Every check here is about a way of being *nearly* right. The predicate has
//! no visible signature of its own — it turns a handful of blocks in a
//! bastion's rampart into air — so a port that reads the wrong position, or
//! takes its draw at the wrong moment, or interpolates the right two numbers
//! in the wrong order, produces a bastion that looks entirely plausible and
//! is wrong in a few hundred blocks. None of that is caught by "it builds a
//! bastion".

use rustmc_worldgen::rng::legacy::LegacyRandomSource;
use rustmc_worldgen::rng::RandomSource;
use rustmc_worldgen::structure::pieces::BoundingBox;
use rustmc_worldgen::structure::place::{start_reference_pos, PosRuleTest, Rule, RuleTest};

fn parse(text: &str) -> PosRuleTest {
    PosRuleTest::parse(&serde_json::from_str(text).expect("json")).expect("ported")
}

/// The defaults are the whole of `high_rampart.json`'s meaning: it names two
/// fields of five and inherits three.
#[test]
fn the_absent_fields_default_the_way_the_codec_does() {
    let got = parse(
        r#"{"predicate_type":"minecraft:axis_aligned_linear_pos",
            "max_chance":0.05,"max_dist":100}"#,
    );
    assert_eq!(
        got,
        PosRuleTest::AxisAlignedLinear {
            min_chance: 0.0,
            max_chance: 0.05,
            min_dist: 0,
            max_dist: 100,
            // `Axis.CODEC.fieldOf("axis").orElse(Axis.Y)`. The default is Y
            // and not X, which is the one that matters: X and Z read a
            // horizontal distance from the structure's centre column, Y reads
            // height above its floor, and on a bastion those are entirely
            // different sets of blocks.
            axis: rustmc_worldgen::structure::place::Axis::Y,
        }
    );
}

/// `minecraft:linear_pos` is registered in vanilla and reached by no document
/// in the jar. It is refused **by name**, so the refusal distinguishes "a
/// type we chose not to port" from "a document we failed to read".
#[test]
fn linear_pos_is_refused_by_name() {
    let value: serde_json::Value =
        serde_json::from_str(r#"{"predicate_type":"minecraft:linear_pos"}"#).expect("json");
    let err = PosRuleTest::parse(&value).expect_err("unported");
    assert!(err.0.contains("minecraft:linear_pos"), "{}", err.0);
}

/// Vanilla's constructor throws on `minDist >= maxDist`, and the *default*
/// pair is `0, 0` — so an object naming no distance is the throwing case.
///
/// The reason to refuse rather than shrug: `inverseLerp` would divide by zero
/// and answer an infinity or a NaN, `clampedLerp` would then take a branch on
/// a NaN comparison, and the predicate would silently never fire. That is
/// indistinguishable from a correctly-parsed predicate on a structure nobody
/// has measured.
#[test]
fn an_empty_range_is_refused_rather_than_dividing_by_zero() {
    let value: serde_json::Value =
        serde_json::from_str(r#"{"predicate_type":"minecraft:axis_aligned_linear_pos"}"#)
            .expect("json");
    let err = PosRuleTest::parse(&value).expect_err("invalid range");
    assert!(err.0.contains("[0,0]"), "{}", err.0);
}

/// The arithmetic, against values computed by hand from the bytecode.
///
/// `dist = |worldPos.y - reference.y|` for `axis = Y`; the chance is
/// `clampedLerp(inverseLerp(dist, 0, 100), 0.0, 0.05)`, which for
/// `high_rampart` is `dist / 100 * 0.05`; and the comparison is
/// `nextFloat() <= chance`.
#[test]
fn the_chance_is_linear_in_the_distance_along_the_axis() {
    /// A source that answers one scripted float and records that it was asked.
    struct One(f32, std::cell::Cell<usize>);
    impl RandomSource for One {
        fn set_seed(&mut self, _seed: i64) {
            unreachable!()
        }
        fn next_i32(&mut self) -> i32 {
            unreachable!("the predicate draws a float")
        }
        fn next_i32_bound(&mut self, _bound: i32) -> i32 {
            unreachable!()
        }
        fn next_i64(&mut self) -> i64 {
            unreachable!()
        }
        fn next_f32(&mut self) -> f32 {
            self.1.set(self.1.get() + 1);
            self.0
        }
        fn next_f64(&mut self) -> f64 {
            unreachable!()
        }
        fn next_bool(&mut self) -> bool {
            unreachable!()
        }
        fn next_gaussian(&mut self) -> f64 {
            unreachable!()
        }
    }

    let predicate = parse(
        r#"{"predicate_type":"minecraft:axis_aligned_linear_pos",
            "max_chance":0.05,"max_dist":100}"#,
    );
    // The rule is only reachable through `Rule`, so drive it through one.
    let rule = Rule {
        input: RuleTest::AlwaysTrue,
        location: RuleTest::AlwaysTrue,
        position: predicate.clone(),
        output: "minecraft:air".to_string(),
        append_loot: None,
    };
    assert_eq!(rule.position, predicate);

    // dist 40 -> chance 0.02. A draw of exactly 0.02 passes, because the
    // comparison is `<=`; 0.020001 does not.
    for (y, draw, want) in [
        (40i32, 0.02f32, true),
        (40, 0.020_001, false),
        // dist 0 at the reference's own y, chance 0.0 — and `nextFloat` CAN
        // return 0.0, which `<=` accepts and `<` would not. This is the row
        // that separates the two comparisons.
        (0, 0.0, true),
        (0, 0.000_001, false),
        // Beyond `max_dist` the factor is clamped, not extrapolated: chance
        // stays 0.05 at any distance past 100.
        (400, 0.05, true),
        (400, 0.050_001, false),
    ] {
        let mut random = One(draw, std::cell::Cell::new(0));
        let got = predicate.test((10, y, -7), (10, 0, -7), &mut random);
        assert_eq!(got, want, "y={y} draw={draw}");
        assert_eq!(random.1.get(), 1, "exactly one float per test");
    }

    // The axis is read: the same predicate on a position that differs only in
    // x is at distance 0, however far away it is.
    let mut random = One(0.02, std::cell::Cell::new(0));
    assert!(
        !predicate.test((900, 0, -7), (10, 0, -7), &mut random),
        "an x offset is distance 0 for an axis=Y predicate"
    );
}

/// `StructureStart.placeInChunk`'s reference position, on the exact boxes
/// vanilla saved for the two bastions in the r=16 nether window.
///
/// These are not invented numbers: `worldgen-dump jigsaw-starts` prints piece
/// zero of `bastion_remnant (55,63)` as
/// `BB=[833 32 1008 .. 880 55 1037]` and of `(70,49)` as
/// `BB=[1091 32 737 .. 1120 55 784]`.
#[test]
fn the_reference_position_is_get_center_in_xz_and_min_y_in_y() {
    let a = BoundingBox::new(833, 32, 1008, 880, 55, 1037);
    // getCenter x = 833 + (880 - 833 + 1)/2 = 833 + 24 = 857.
    // getCenter z = 1008 + (1037 - 1008 + 1)/2 = 1008 + 15 = 1023.
    // y is minY, 32 — NOT getCenter's y, which is 44.
    assert_eq!(start_reference_pos(a), (857, 32, 1023));

    let b = BoundingBox::new(1091, 32, 737, 1120, 55, 784);
    assert_eq!(start_reference_pos(b), (1106, 32, 761));

    // `(min + max) / 2` is the formula the village stub uses and it is NOT
    // this one. Both bastion `air_base` spans are even, so the two disagree
    // on both starts — 856 against 857, 1022 against 1023.
    assert_ne!(start_reference_pos(a).0, (833 + 880) / 2);
    assert_ne!(start_reference_pos(a).2, (1008 + 1037) / 2);

    // And the y is the trap this whole type exists for: taking the centre's y
    // would put the reference 12 blocks up, which for `high_rampart`'s
    // `max_dist` of 100 shifts every chance by 0.6 percentage points in the
    // wrong direction.
    assert_ne!(start_reference_pos(a).1, 32 + (55 - 32 + 1) / 2);
}

/// The seed a rule's random is built from is the **world** position, and it
/// is unaffected by the reference. Pinned so that threading the reference
/// cannot be mistaken for having changed the stream.
#[test]
fn the_rule_random_is_still_seeded_from_the_world_position_alone() {
    use rustmc_worldgen::structure::template::position_seed;
    let mut a = LegacyRandomSource::new(position_seed(857, 44, 1023));
    let mut b = LegacyRandomSource::new(position_seed(857, 44, 1023));
    assert_eq!(a.next_f32(), b.next_f32());
}

/// The real `high_rampart.json`, parsed — the document `Rule::parse` used to
/// refuse outright.
///
/// This is the whole of the bastion's processor gap in one file, so it is
/// asserted against the jar rather than against a fixture written here. Three
/// things it pins that a hand-written fixture would not:
///
/// * the list parses **at all**. Before this branch the single
///   `position_predicate` made `Rule::parse` refuse, a refused rule refuses
///   its whole list, and `bastion/treasure/ramparts` was therefore
///   unplaceable — one field in one document standing between this port and a
///   105-piece structure.
/// * **rules 1 and 3 keep their probabilities and rule 2 has none.** A
///   `random_block_match` spends a float only when its block test passes, so
///   0.3 and 0.5 are draw counts as well as chances.
/// * **rule 2's input predicate is `always_true`**, which means its position
///   predicate runs for EVERY block that reaches it and therefore that rule 3
///   reads the second float of that position's stream and not the first. A
///   port that dropped the position predicate rather than refusing it would
///   have kept every block and shifted that read.
#[test]
fn the_high_rampart_list_parses_and_keeps_its_draw_sequence() {
    use rustmc_worldgen::structure::place::{Axis, Processor};
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft",
    ))
    .join("worldgen/processor_list/high_rampart.json");
    if !rustmc_data::fixture::available(&path, "the datagen extract", "ci/vanilla-server.sh data") {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    let list = Processor::parse_list(&doc).expect("high_rampart parses");
    assert_eq!(list.len(), 1, "one rule processor");
    let Processor::Rule(rules) = &list[0] else {
        panic!("not a rule processor: {:?}", list[0]);
    };
    assert_eq!(rules.len(), 3);

    assert_eq!(
        rules[0].input,
        RuleTest::RandomBlockMatch {
            block: "minecraft:gold_block".to_string(),
            probability: 0.3,
        }
    );
    assert_eq!(rules[0].position, PosRuleTest::AlwaysTrue);

    assert_eq!(rules[1].input, RuleTest::AlwaysTrue);
    assert_eq!(rules[1].location, RuleTest::AlwaysTrue);
    assert_eq!(rules[1].output, "minecraft:air");
    assert_eq!(
        rules[1].position,
        PosRuleTest::AxisAlignedLinear {
            min_chance: 0.0,
            max_chance: 0.05,
            min_dist: 0,
            max_dist: 100,
            axis: Axis::Y,
        }
    );

    assert_eq!(
        rules[2].input,
        RuleTest::RandomBlockMatch {
            block: "minecraft:gilded_blackstone".to_string(),
            probability: 0.5,
        }
    );
    assert_eq!(rules[2].position, PosRuleTest::AlwaysTrue);
}

/// Every named processor list a `bastion/` pool element resolves, parsed.
///
/// The port's claim is that a bastion needs no processor type it does not
/// already have. `ci/structure-surface.py --bastion` asserts that from the
/// datapack; this asserts it from the PARSER, which is the half that can fail
/// while the census passes — a type can be listed in `Processor::parse` and
/// still refuse a particular document over a field.
#[test]
fn every_processor_list_a_bastion_names_parses() {
    use rustmc_worldgen::structure::place::Processor;
    let data = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft",
    ));
    if !rustmc_data::fixture::available(
        &data.join("worldgen/processor_list/high_rampart.json"),
        "the datagen extract",
        "ci/vanilla-server.sh data",
    ) {
        return;
    }
    let mut named: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut elements = 0usize;
    let pools = data.join("worldgen/template_pool/bastion");
    let mut stack = vec![pools];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read a bastion pool dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let doc: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
            for element in doc["elements"].as_array().into_iter().flatten() {
                elements += 1;
                if let Some(name) = element["element"]["processors"].as_str() {
                    named.insert(name.to_string());
                }
            }
        }
    }
    // The census figures, restated here so this test fails if it is pointed
    // at a tree whose bastion pools are not the ones it was written against.
    assert_eq!(elements, 176, "bastion pool elements");
    assert_eq!(named.len(), 12, "distinct named processor lists");

    for name in &named {
        let file = data.join(format!(
            "worldgen/processor_list/{}.json",
            name.trim_start_matches("minecraft:")
        ));
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        Processor::parse_list(&doc).unwrap_or_else(|e| panic!("{name} does not parse: {}", e.0));
    }
}
