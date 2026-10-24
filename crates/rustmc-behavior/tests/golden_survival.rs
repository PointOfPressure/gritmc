//! `canSurvive` against vanilla's own answers, over a controlled
//! neighbourhood.
//!
//! Vectors: `data/vanilla/survival.json`, from `ci/survival-golden.sh`, which
//! asks vanilla every state `would_survive` names against twenty-seven
//! candidate blocks-below with nothing else nearby.
//!
//! # A refusal on either side is not a pass
//!
//! The oracle records `null` where the rule read more of the world than the
//! harness could show it — a mushroom checking light, sugar cane looking for
//! water beside the block below. This port records [`Survival::Unknown`] for
//! rules it has not implemented.
//!
//! Those are **different refusals** and the gate keeps them apart:
//!
//! - vanilla answered, port answered: compared, and a mismatch fails.
//! - vanilla answered, port refused: counted as unported. Legitimate, and
//!   ratcheted so it can only go down.
//! - vanilla refused: the port must refuse too. **Something can be learned
//!   there after all, and this gate did not learn it until 2026-08-10.**
//!
//! What the gate will not do is let a port refusal stand in for agreement —
//! nor, since 2026-08-10, let a port *answer* stand in where vanilla could
//! not be shown enough of the world to have one.
//!
//! # The third case was a gate that could not fail
//!
//! It used to read "vanilla refused: skipped, and counted. Nothing can be
//! learned there", and that is wrong in one direction. The oracle records
//! `null` precisely when vanilla's rule read something the one-block
//! neighbourhood does not define — a light level, water beside the block
//! below. A port that answers `Yes` or `No` there is claiming to have
//! resolved a read the harness never set up, so its answer cannot be right
//! for vanilla's reasons even when it happens to agree.
//!
//! `brown_mushroom` and `red_mushroom` on `grass_block` were exactly that:
//! the oracle recorded
//! `null / reads: canSurvive read LevelReader.getRawBrightness/2`, this port
//! answered `Yes` because both blocks were filed under the plain
//! `#supports_vegetation` vegetation family, and the gate skipped the row.
//! It cost 33 blocks on the r=16 overworld window at seed 20260722 — every
//! mushroom in it that vanilla does not have — and no gate anywhere could
//! have caught it.

use std::collections::BTreeMap;
use std::path::Path;

use rustmc_behavior::survival::{can_survive, Survival, SurvivalWorld, TagLookup};

const VECTORS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/vanilla/survival.json"
);
const TAGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/tags/block"
);

/// Answers the port must produce. Ratcheted: it may only go up.
///
/// 760 -> 853 when `mangrove_propagule` was ported, then -> 991 with the
/// lily pad, the dry vegetation family (dead_bush, short_dry_grass,
/// tall_dry_grass) and the two blocks whose rule turns out not to exist
/// (pumpkin and melon are plain `Block`s). Unported fell 255 -> 117.
///
/// 991 -> 1014 with `soul_fire`, whose whole rule is
/// `below.is(#soul_fire_base_blocks)`. That family was 293 refusals in one
/// r=7 nether window and 213 blocks of residual against vanilla.
///
/// 1014 -> 1302 on 2026-10-02, from two changes measured together because
/// they landed together: the oracle grew from 23 grounds to 27 (the two
/// nyliums, soul soil and an oak slab) and from 51 subjects to 55 (the four
/// `nether_forest_vegetation` states it had never asked about), and the
/// nether plants got their own families. The grounds are what made the
/// `crimson_roots` defect visible at all; see `survival.rs`.
const ANSWERED_FLOOR: usize = 1302;

/// Cases the port declines that vanilla answers. **May only go down**, which
/// the header has claimed since this file was written and no line enforced
/// until 2026-08-18.
///
/// 117 -> 94 with `soul_fire`. What is left is `fire` (23 grounds), whose
/// rule is `below.isFaceSturdy(UP) || isValidFireLocation(pos)` and needs the
/// per-block ignite-odds table the second clause reads, and three families
/// this port has no rule for at all.
///
/// 94 -> 110 on 2026-10-02, and that is a wider oracle rather than a rule
/// lost: the grounds went 23 -> 27 and four cells decline per ground in this
/// one-block world (94 = 4 x 23 + 2, 110 = 4 x 27 + 2). `fire` is one of the
/// four and has a family now; it declines here only because this world
/// furnishes no shape table, and is checked with one in
/// `fire_matches_vanilla_on_every_ground`.
const UNPORTED_CEILING: usize = 110;

/// One block below the origin, air everywhere else — the same world the
/// oracle showed vanilla.
struct OneBlockBelow(String);

impl SurvivalWorld for OneBlockBelow {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        if (x, y, z) == (0, -1, 0) {
            self.0.clone()
        } else {
            "minecraft:air".to_string()
        }
    }
}

/// The block tags, flattened the way `BlockTags::load` does — reimplemented
/// here rather than depended on, because `rustmc-behavior` must not depend on
/// `rustmc-worldgen`.
struct Tags(BTreeMap<String, Vec<String>>);

impl TagLookup for Tags {
    fn contains(&self, tag: &str, block: &str) -> bool {
        self.0
            .get(tag)
            .is_some_and(|blocks| blocks.iter().any(|b| b == block))
    }
}

fn load_tags() -> Option<Tags> {
    let dir = Path::new(TAGS);
    // The absence used to be returned as a plain `None` and reported by the
    // caller's `eprintln!`, which cargo swallows for a passing test. This
    // panics in the default mode and names the command that makes the extract.
    if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
        return None;
    }
    let mut raw: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("tags dir") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).expect("utf-8");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("readable")).expect("json");
        let entries = json["values"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .map(|v| {
                        v.as_str()
                            .or_else(|| v["id"].as_str())
                            .expect("tag entry")
                            .to_string()
                    })
                    .collect()
            })
            .unwrap_or_default();
        raw.insert(format!("minecraft:{stem}"), entries);
    }

    // Flatten `#tag` references, with a cycle guard.
    fn flatten(
        name: &str,
        raw: &BTreeMap<String, Vec<String>>,
        out: &mut Vec<String>,
        visiting: &mut Vec<String>,
    ) {
        if visiting.iter().any(|v| v == name) {
            panic!("block tag cycle at {name}");
        }
        visiting.push(name.to_string());
        for entry in raw.get(name).unwrap_or(&Vec::new()) {
            if let Some(inner) = entry.strip_prefix('#') {
                flatten(inner, raw, out, visiting);
            } else {
                out.push(entry.clone());
            }
        }
        visiting.pop();
    }

    let mut flat = BTreeMap::new();
    for name in raw.keys() {
        let mut out = Vec::new();
        flatten(name, &raw, &mut out, &mut Vec::new());
        flat.insert(name.clone(), out);
    }
    Some(Tags(flat))
}

#[test]
fn can_survive_matches_vanilla_where_both_sides_answer() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let Some(tags) = load_tags() else { return };
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    let mut answered = 0usize;
    let mut unported = 0usize;
    let mut oracle_refused = 0usize;
    let mut mismatches: Vec<String> = Vec::new();
    // Rows where vanilla could not be shown enough of the world to answer and
    // this port answered anyway. Not a mismatch — there is nothing to
    // mismatch against — but an unfounded claim, and the failure this gate
    // was blind to.
    let mut unfounded: Vec<String> = Vec::new();

    for row in doc["table"].as_array().expect("table") {
        let state = row["state"].as_str().expect("state");
        for ground in row["grounds"].as_array().expect("grounds") {
            let below = ground["ground"].as_str().expect("ground");
            let Some(expected) = ground["survives"].as_bool() else {
                oracle_refused += 1;
                let mut world = OneBlockBelow(below.to_string());
                let answer = can_survive(&mut world, &tags, state, (0, 0, 0));
                if answer != Survival::Unknown {
                    let reads = ground["reads"].as_str().unwrap_or("(no reason recorded)");
                    unfounded.push(format!(
                        "{state} on {below}: port says {}, but vanilla could not be \
                         shown enough to answer — {reads}",
                        answer == Survival::Yes
                    ));
                }
                continue;
            };
            let mut world = OneBlockBelow(below.to_string());
            match can_survive(&mut world, &tags, state, (0, 0, 0)) {
                Survival::Unknown => unported += 1,
                answer => {
                    answered += 1;
                    let got = answer == Survival::Yes;
                    if got != expected {
                        mismatches.push(format!(
                            "{state} on {below}: port says {got}, vanilla says {expected}"
                        ));
                    }
                }
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} of {answered} answered cases disagree with vanilla:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        unfounded.is_empty(),
        "{} of {oracle_refused} cases vanilla could not answer were answered anyway \
         by this port:\n{}",
        unfounded.len(),
        unfounded
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(
        answered >= ANSWERED_FLOOR,
        "the port answered only {answered} cases, floor is {ANSWERED_FLOOR}"
    );
    // The ratchet the header has promised since this file was written. It was
    // a sentence and nothing else until 2026-08-18, so an unported family
    // could grow back silently — the same shape as a doc comment describing a
    // gate no line implements.
    assert!(
        unported <= UNPORTED_CEILING,
        "unported rose to {unported}, ceiling is {UNPORTED_CEILING}. A survival          family that stops answering is a decoration feature that stops placing."
    );

    println!(
        "SURVIVAL: {answered} answered and 100.000000% correct, \
         {unported} unported, {oracle_refused} the oracle could not show \
         and this port correctly declines"
    );
}

/// The same one-block neighbourhood, but able to answer [`SurvivalWorld::air_at`].
///
/// [`OneBlockBelow`] implements `block_at` and nothing else, so every rule
/// that reads air gets `None` and answers [`Survival::Unknown`] no matter what
/// it does. That is fine for the gate above — those rows are the oracle's own
/// refusals — but it means a rule shaped like `!below.isAir()` **cannot be
/// exercised there at all**, and a check that cannot fail is not a check.
///
/// It knows the answer soundly: the world holds exactly one block and the
/// three air blocks are named. `cave_air` and `void_air` are air, which a
/// `== "minecraft:air"` test misses, and that is the mutation this world
/// exists to make catchable.
struct OneBlockBelowKnowingAir(String);

impl SurvivalWorld for OneBlockBelowKnowingAir {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        if (x, y, z) == (0, -1, 0) {
            self.0.clone()
        } else {
            "minecraft:air".to_string()
        }
    }

    fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let block = self.block_at(x, y, z);
        let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
        Some(matches!(
            plain,
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        ))
    }
}

/// `pale_moss_carpet`'s base state, against every ground vanilla answered for.
///
/// This is the row the main gate cannot reach. Vanilla answered all
/// twenty-seven — unlike `moss_carpet`, whose every ground is `null` with
/// `reads: canSurvive read LevelReader.isEmptyBlock/1` — so the two carpets
/// are not the same rule as far as any instrument here can see, and the port
/// keeps them as separate families for that reason.
///
/// The assertion is the whole row and not a sample: vanilla says **true on
/// every ground except `air`**, water included, which is what rules out a
/// sturdy-face reading and leaves the `isAir` one. Asserting `water` and
/// `air` explicitly, because those two are the entire content of the
/// distinction and a loop that happened to skip them would still pass.
#[test]
fn the_base_pale_moss_carpet_matches_vanilla_on_every_ground() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let Some(tags) = load_tags() else {
        return;
    };
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    let row = doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .find(|r| r["block"].as_str() == Some("minecraft:pale_moss_carpet"))
        .expect("the oracle carries a pale_moss_carpet row");
    let state = row["state"].as_str().expect("state");
    assert!(
        state.contains("bottom=true"),
        "the oracle's pale_moss_carpet row is {state}, not the base state this rule is for"
    );

    let mut checked = 0usize;
    let mut seen_water = false;
    let mut seen_air = false;
    for ground in row["grounds"].as_array().expect("grounds") {
        let below = ground["ground"].as_str().expect("ground");
        let expected = ground["survives"]
            .as_bool()
            .unwrap_or_else(|| panic!("vanilla refused {below}, which this rule must not need"));
        let mut world = OneBlockBelowKnowingAir(below.to_string());
        let got = can_survive(&mut world, &tags, state, (0, 0, 0));
        assert_eq!(
            got,
            if expected {
                Survival::Yes
            } else {
                Survival::No
            },
            "{state} on {below}: port says {got:?}, vanilla says {expected}"
        );
        seen_water |= below == "minecraft:water";
        seen_air |= below == "minecraft:air";
        checked += 1;
    }
    assert!(
        seen_water && seen_air,
        "the two decisive grounds are missing"
    );
    assert_eq!(
        checked, 27,
        "the oracle's ground list changed under this test"
    );
}

/// A wall-attached carpet reads its neighbours, so the port declines it —
/// **without reading the block below**, which is the part worth pinning.
///
/// Answering `bottom=false` off the block below would be right roughly as
/// often as the ground happens to be solid, so a test that only asserted
/// `Unknown` would pass against a port that read below and guessed. This one
/// hands it a world where the block below is `stone`: a port that fell
/// through to the base rule would answer `Yes`.
#[test]
fn a_wall_attached_pale_moss_carpet_is_declined_rather_than_guessed() {
    let Some(tags) = load_tags() else {
        return;
    };
    for state in [
        "minecraft:pale_moss_carpet[bottom=false,east=none,north=low,south=none,west=none]",
        "minecraft:pale_moss_carpet[bottom=false,east=tall,north=none,south=none,west=none]",
        "minecraft:pale_moss_carpet[bottom=false,east=none,north=none,south=none,west=none]",
    ] {
        let mut world = OneBlockBelowKnowingAir("minecraft:stone".to_string());
        assert_eq!(
            can_survive(&mut world, &tags, state, (0, 0, 0)),
            Survival::Unknown,
            "{state} was answered off the block below, which its rule never reads"
        );
    }
}

/// Which subjects this port cannot answer for **anywhere**, derived.
///
/// [`family_of`] is private and its list is prose in a doc comment, which is
/// how that comment came to name sixteen blocks as unported when fourteen of
/// them had families. A sentence no line holds to is the defect class this
/// repo keeps finding, and the fix is the same every time: derive it.
///
/// Three subjects answer on no ground at all, and they are **not** three of a
/// kind:
///
/// * `fire` has a rule, and it reads a shape table and an ignite-odds table,
///   neither of which this world furnishes. Both are furnished in
///   `fire_matches_vanilla_on_every_ground`, where it answers every row.
/// * `leaf_litter` and `spore_blossom` **have** rules, and each reads
///   something this world does not furnish: `isFaceSturdy(UP)`, which needs a
///   shape table, and `can_support_center` on the block *above*. Both answer
///   in a real chunk, and `leaf_litter` is checked against the shape table in
///   `leaf_litter_is_the_sturdy_face_and_only_the_sturdy_face`.
///
/// The distinction is the whole point of the test: a count of "answers
/// nowhere" that lumped the causes together would go green if a rule were
/// deleted and replaced by a read this world happens not to serve. It is
/// asserted below, once per cause.
#[test]
fn only_three_subjects_answer_on_no_ground_and_for_two_different_reasons() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let Some(tags) = load_tags() else {
        return;
    };
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    let mut silent: Vec<String> = Vec::new();
    for row in doc["table"].as_array().expect("table") {
        let state = row["state"].as_str().expect("state");
        let block = row["block"].as_str().expect("block");
        let answered_any = row["grounds"].as_array().expect("grounds").iter().any(|g| {
            let below = g["ground"].as_str().expect("ground");
            let mut world = OneBlockBelowKnowingAir(below.to_string());
            can_survive(&mut world, &tags, state, (0, 0, 0)) != Survival::Unknown
        });
        if !answered_any {
            silent.push(block.to_string());
        }
    }
    silent.sort();
    silent.dedup();
    assert_eq!(
        silent,
        vec![
            "minecraft:fire".to_string(),
            "minecraft:leaf_litter".to_string(),
            "minecraft:spore_blossom".to_string(),
        ],
        "the set of subjects this port answers for nowhere has moved. Update \
         `family_of`'s doc comment to match rather than this assertion, unless \
         a rule was really added or lost"
    );

    // The half of the claim the set alone does not carry: `spore_blossom` is
    // silent because of the world, not because of a missing rule. Given a
    // ceiling it answers, and the other two still do not.
    struct WithCeiling(bool);
    impl SurvivalWorld for WithCeiling {
        fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
            "minecraft:stone".to_string()
        }
        fn can_support_center(
            &mut self,
            _x: i32,
            _y: i32,
            _z: i32,
            _direction: usize,
        ) -> Option<bool> {
            Some(self.0)
        }
    }
    assert_eq!(
        can_survive(
            &mut WithCeiling(true),
            &tags,
            "minecraft:spore_blossom",
            (0, 0, 0)
        ),
        Survival::Yes,
        "spore_blossom has a rule and it should answer once the ceiling is furnished"
    );
    assert_eq!(
        can_survive(&mut WithCeiling(true), &tags, "minecraft:fire", (0, 0, 0)),
        Survival::Unknown,
        "fire needs the sturdy face, which this world does not furnish"
    );

    // And the same for `leaf_litter`, whose missing read is the shape table.
    struct WithSturdyFace(bool);
    impl SurvivalWorld for WithSturdyFace {
        fn block_at(&mut self, _x: i32, _y: i32, _z: i32) -> String {
            "minecraft:stone".to_string()
        }
        fn face_is_sturdy_up(&mut self, _x: i32, _y: i32, _z: i32) -> Option<bool> {
            Some(self.0)
        }
    }
    assert_eq!(
        can_survive(
            &mut WithSturdyFace(true),
            &tags,
            "minecraft:leaf_litter[facing=north,segment_amount=1]",
            (0, 0, 0)
        ),
        Survival::Yes,
        "leaf_litter has a rule and it should answer once the face is furnished"
    );
}

/// `isFaceSturdy(UP)` for a ground block, read out of the **other** extract.
///
/// `data/vanilla/block_props.json` records `sturdyFaces` as a six-character
/// bitstring in `directionOrder`, which is `DOWN, UP, NORTH, SOUTH, WEST,
/// EAST`, so `UP` is index 1. Nothing here is derived from `survival.json`,
/// which is what makes the leaf-litter test below a comparison of two
/// independent extracts rather than a table checked against itself.
struct OneBlockBelowWithShapes {
    below: String,
    sturdy_up: BTreeMap<String, bool>,
}

impl OneBlockBelowWithShapes {
    fn load() -> Option<BTreeMap<String, bool>> {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/vanilla/block_props.json"
        ));
        let doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?)
            .expect("block_props.json parses");
        assert_eq!(
            doc["directionOrder"][1].as_str(),
            Some("UP"),
            "sturdyFaces is indexed by directionOrder and UP moved"
        );
        let mut out = BTreeMap::new();
        for row in doc["table"].as_array().expect("table") {
            if row["default"].as_bool() != Some(true) {
                continue;
            }
            let block = row["block"].as_str().expect("block").to_string();
            let faces = row["sturdyFaces"].as_str().expect("sturdyFaces");
            out.insert(block, faces.as_bytes().get(1) == Some(&b'1'));
        }
        Some(out)
    }
}

impl SurvivalWorld for OneBlockBelowWithShapes {
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
        if (x, y, z) == (0, -1, 0) {
            self.below.clone()
        } else {
            "minecraft:air".to_string()
        }
    }

    fn face_is_sturdy_up(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
        let block = self.block_at(x, y, z);
        let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
        self.sturdy_up.get(plain).copied()
    }
}

/// `leaf_litter` against vanilla's answers, over every ground.
///
/// The rule is `level.getBlockState(pos.below()).isFaceSturdy(level, below,
/// UP)` and nothing else — four instructions and a return in
/// `LeafLitterBlock#canSurvive`. Three grounds decide it and the assertion
/// names all three rather than trusting the loop to reach them:
///
/// * **`farmland` is false.** `farmland` is in `#supports_vegetation`, so no
///   reading of this rule as the ordinary vegetation family survives that row.
/// * **`sand`, `gravel` and `snow_block` are true.** An `isSolidRender`
///   reading fails those.
///
/// Both extracts are used and neither is derived from the other:
/// `survival.json` says what vanilla answered, `block_props.json` says which
/// faces are sturdy.
#[test]
fn leaf_litter_is_the_sturdy_face_and_only_the_sturdy_face() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let (Some(tags), Some(sturdy_up)) = (load_tags(), OneBlockBelowWithShapes::load()) else {
        return;
    };
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");

    let row = doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .find(|r| r["block"].as_str() == Some("minecraft:leaf_litter"))
        .expect("the oracle carries a leaf_litter row");
    let state = row["state"].as_str().expect("state");

    let mut checked = 0usize;
    let mut decisive = 0usize;
    for ground in row["grounds"].as_array().expect("grounds") {
        let below = ground["ground"].as_str().expect("ground");
        let expected = ground["survives"]
            .as_bool()
            .unwrap_or_else(|| panic!("vanilla refused {below}, which this rule must not need"));
        let mut world = OneBlockBelowWithShapes {
            below: below.to_string(),
            sturdy_up: sturdy_up.clone(),
        };
        assert_eq!(
            can_survive(&mut world, &tags, state, (0, 0, 0)),
            if expected {
                Survival::Yes
            } else {
                Survival::No
            },
            "{state} on {below}: vanilla says {expected}"
        );
        if matches!(
            below,
            "minecraft:farmland" | "minecraft:sand" | "minecraft:gravel" | "minecraft:snow_block"
        ) {
            decisive += 1;
        }
        checked += 1;
    }
    assert_eq!(
        checked, 27,
        "the oracle's ground list changed under this test"
    );
    assert_eq!(
        decisive, 4,
        "the four grounds that separate this rule from its neighbours are missing"
    );
}

/// `FireBlock#canSurvive` against every ground vanilla answered, with both of
/// the tables it reads furnished: the sturdy face from `block_props.json` and
/// `canBurn` from `fire_odds.json`.
///
/// `oak_slab` is the decisive ground. It is flammable and a bottom slab is
/// not sturdy on top, so it is the only one of the twenty-seven on which the
/// `isValidFireLocation` clause decides the answer — vanilla says yes, and a
/// rule that stopped at the sturdy face would say no. `air` and `water` are
/// the other side: neither sturdy nor flammable, so both clauses must fail.
#[test]
fn fire_matches_vanilla_on_every_ground() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let (Some(tags), Some(sturdy_up)) = (load_tags(), OneBlockBelowWithShapes::load()) else {
        return;
    };
    let odds_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/fire_odds.json"
    ));
    if !rustmc_data::fixture::available(odds_path, "fire odds", "ci/gen-gameplay-tables.sh") {
        return;
    }
    let odds: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(odds_path).expect("readable"))
            .expect("fire_odds.json parses");
    let flammable: std::collections::BTreeSet<String> = odds["odds"]
        .as_object()
        .expect("odds")
        .iter()
        .filter(|(_, cell)| cell["ignite"].as_i64().unwrap_or(0) > 0)
        .map(|(block, _)| block.clone())
        .collect();

    struct FireWorld {
        shapes: OneBlockBelowWithShapes,
        flammable: std::collections::BTreeSet<String>,
    }
    impl SurvivalWorld for FireWorld {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.shapes.block_at(x, y, z)
        }
        fn face_is_sturdy_up(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            self.shapes.face_is_sturdy_up(x, y, z)
        }
        // `canBurn`: a positive ignite odds and not waterlogged. Every block
        // this world holds is a default state, and no default state here is
        // waterlogged, so the odds alone decide it.
        fn fire_can_burn(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            let block = self.block_at(x, y, z);
            let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
            Some(self.flammable.contains(plain))
        }
    }

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let row = doc["table"]
        .as_array()
        .expect("table")
        .iter()
        .find(|r| r["block"].as_str() == Some("minecraft:fire"))
        .expect("the oracle carries a fire row");
    let state = row["state"].as_str().expect("state");
    let mut checked = 0usize;
    let mut by_second_clause = 0usize;
    for ground in row["grounds"].as_array().expect("grounds") {
        let below = ground["ground"].as_str().expect("ground");
        let expected = ground["survives"]
            .as_bool()
            .unwrap_or_else(|| panic!("vanilla refused fire on {below}"));
        let mut world = FireWorld {
            shapes: OneBlockBelowWithShapes {
                below: below.to_string(),
                sturdy_up: sturdy_up.clone(),
            },
            flammable: flammable.clone(),
        };
        let sturdy = world.face_is_sturdy_up(0, -1, 0);
        assert!(
            sturdy.is_some(),
            "{below} has no sturdy-face row in block_props.json"
        );
        assert_eq!(
            can_survive(&mut world, &tags, state, (0, 0, 0)),
            if expected {
                Survival::Yes
            } else {
                Survival::No
            },
            "fire on {below}: vanilla says {expected}"
        );
        if expected && sturdy == Some(false) {
            by_second_clause += 1;
        }
        checked += 1;
    }
    assert_eq!(
        checked, 27,
        "the oracle's ground list changed under this test"
    );
    assert_eq!(
        by_second_clause, 1,
        "exactly one ground (oak_slab) is decided by isValidFireLocation; if \
         that moved, this test no longer reaches the second clause"
    );
}

/// The mushrooms in a DARK world, against vanilla's own answers there.
///
/// `SurvivalGolden` asks every subject a second time with `getRawBrightness`
/// answering 0 — what a decorating chunk in the nether answers, with no sky
/// engine and no block light yet. Only the mushrooms answer differently in
/// the dark: `MushroomBlock.canSurvive` is the override tag, else light below
/// 13 and `mayPlaceOn`, which is `below.isSolidRender()`. This world furnishes
/// the light and the render flag (`block_props.json`'s `solidRender`, a
/// separate extract), and every one of the 2 x 27 dark answers must match.
///
/// Before 2026-10-02 the dark branch returned `Unknown` ("no world reaches
/// here today") and the nether placed no mushroom at all: 3,476 brown and
/// 2,413 red refusals on seed 31415926 c(12,3) r=7.
#[test]
fn mushrooms_in_the_dark_match_vanilla_on_every_ground() {
    let path = Path::new(VECTORS);
    if !rustmc_data::fixture::available(path, "survival extract", "ci/survival-golden.sh") {
        return;
    }
    let Some(tags) = load_tags() else { return };
    let props_path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/block_props.json"
    ));
    let props: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(props_path).expect("block_props.json"))
            .expect("block_props.json parses");
    let solid_render: BTreeMap<String, bool> = props["table"]
        .as_array()
        .expect("table")
        .iter()
        .filter(|row| row["default"].as_bool() == Some(true))
        .map(|row| {
            (
                row["block"].as_str().expect("block").to_string(),
                row["solidRender"].as_bool().expect("solidRender"),
            )
        })
        .collect();

    struct Dark<'a> {
        below: String,
        solid_render: &'a BTreeMap<String, bool>,
    }
    impl SurvivalWorld for Dark<'_> {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            if (x, y, z) == (0, -1, 0) {
                self.below.clone()
            } else {
                "minecraft:air".to_string()
            }
        }
        fn raw_brightness(&mut self, _x: i32, _y: i32, _z: i32) -> Option<i32> {
            Some(0)
        }
        fn is_solid_render(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            let block = self.block_at(x, y, z);
            let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
            self.solid_render.get(plain).copied()
        }
    }

    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    let mut checked = 0usize;
    let mut decided_by_the_dark = 0usize;
    for row in doc["table"].as_array().expect("table") {
        let block = row["block"].as_str().expect("block");
        if !matches!(block, "minecraft:brown_mushroom" | "minecraft:red_mushroom") {
            continue;
        }
        let state = row["state"].as_str().expect("state");
        let dark = row["dark"]
            .as_array()
            .expect("the oracle carries a dark pass — re-run ci/survival-golden.sh");
        let lit = row["grounds"].as_array().expect("grounds");
        for (d, l) in dark.iter().zip(lit) {
            let below = d["ground"].as_str().expect("ground");
            let expected = d["survives"]
                .as_bool()
                .unwrap_or_else(|| panic!("vanilla refused {state} on {below} in the dark"));
            let mut world = Dark {
                below: below.to_string(),
                solid_render: &solid_render,
            };
            assert_eq!(
                can_survive(&mut world, &tags, state, (0, 0, 0)),
                if expected {
                    Survival::Yes
                } else {
                    Survival::No
                },
                "{state} on {below} in the dark: vanilla says {expected}"
            );
            if l["survives"].as_bool() != Some(expected) {
                decided_by_the_dark += 1;
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 54, "two mushrooms x 27 grounds");
    assert!(
        decided_by_the_dark > 0,
        "no ground answers differently in the dark, so this test reaches no light rule"
    );
}
