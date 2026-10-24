//! `crate::place_survive` against `data/vanilla/item_survive.json`.
//!
//! Every state of every block a `BlockItem` can place whose class overrides
//! `canSurvive`, over the 79 neighbourhoods and 2 light levels the dumper
//! swept — 1,042,800 answers, replayed in full.
//!
//! # What makes this able to go red
//!
//! The subject side is this port and the reference side is vanilla's own
//! answer in a world the dumper fully specified, so any disagreement is a
//! disagreement. Three things stop it being a check that measures its own
//! input:
//!
//! * the ported families are asserted **exactly**, one cell at a time, with
//!   the disagreeing (state, neighbourhood) named — no aggregate threshold;
//! * the unported families are ratcheted by **state count and class list**,
//!   so a family quietly dropping out of the dispatch is as red as a wrong
//!   answer, and a family newly ported has to move the number here;
//! * `a_wrong_rule_is_caught` mutates one rule and requires the sweep to
//!   refuse it, so a green run means the comparison happened.

use rustmc_behavior::block_props::BlockPropsTable;
use rustmc_behavior::place_survive::{
    can_survive, unported_classes, Neighbourhood, PlaceWorld, Survives,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// The world one neighbourhood describes: one non-air block at one offset
/// from the origin, air everywhere else, and a fixed raw brightness.
///
/// The origin is `(0, 0, 0)`, matching the dumper's `BlockPos.ZERO`.
struct OneBlock {
    at: Option<((i32, i32, i32), String)>,
    brightness: i32,
}

impl PlaceWorld for OneBlock {
    fn state_at(&self, x: i32, y: i32, z: i32) -> String {
        match &self.at {
            Some((pos, state)) if *pos == (x, y, z) => state.clone(),
            _ => "minecraft:air".to_string(),
        }
    }

    fn raw_brightness(&self, _x: i32, _y: i32, _z: i32) -> i32 {
        self.brightness
    }
}

fn data(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("data/vanilla");
    path.push(name);
    path
}

/// One neighbourhood: the offset of the single non-air block and its name,
/// or `None` for the all-air column.
type Column = Option<((i32, i32, i32), String)>;

struct Golden {
    columns: Vec<Column>,
    lit: i32,
    dark: i32,
    table: serde_json::Value,
}

/// The dumper's own column names, resolved to the world each describes.
fn load() -> Golden {
    let text = std::fs::read_to_string(data("item_survive.json"))
        .expect("data/vanilla/item_survive.json — run ci/item-survive-golden.sh");
    let json: serde_json::Value = serde_json::from_str(&text).expect("item_survive.json is JSON");
    let offsets: HashMap<&str, (i32, i32, i32)> = [
        ("down", (0, -1, 0)),
        ("up", (0, 1, 0)),
        ("north", (0, 0, -1)),
        ("south", (0, 0, 1)),
        ("west", (-1, 0, 0)),
        ("east", (1, 0, 0)),
    ]
    .into_iter()
    .collect();
    let columns = json["columns"]
        .as_array()
        .expect("columns")
        .iter()
        .map(|column| {
            let column = column.as_str().expect("column name");
            if column == "air" {
                return None;
            }
            let (direction, ground) = column.split_once('=').expect("<direction>=<ground>");
            let offset = *offsets.get(direction).expect("a Direction name");
            // The dumper writes the ground's **default** state and the table
            // it is compared against is keyed by state, so the name has to be
            // resolved the same way `BlockItem.place` would.
            Some((offset, ground.to_string()))
        })
        .collect();
    Golden {
        columns,
        lit: json["lit_brightness"].as_i64().expect("lit_brightness") as i32,
        dark: json["dark_brightness"].as_i64().expect("dark_brightness") as i32,
        table: json,
    }
}

/// Replay the sweep. Returns (agreed, disagreements, unported states).
fn sweep(
    props: &BlockPropsTable,
    golden: &Golden,
    break_the_torch: bool,
) -> (u64, Vec<String>, u64) {
    let mut agreed = 0u64;
    let mut disagreements = Vec::new();
    let mut unported = 0u64;
    for row in golden.table["table"].as_array().expect("table") {
        let Some(class) = row["rule"].as_str() else {
            continue;
        };
        let may_place_on = row["mayPlaceOn"].as_str();
        for entry in row["states"].as_array().expect("states") {
            let state = entry["state"].as_str().expect("state");
            let mut counted_unported = false;
            for (answers, brightness) in [
                (entry["lit"].as_str().expect("lit"), golden.lit),
                (entry["dark"].as_str().expect("dark"), golden.dark),
            ] {
                for (i, expected) in answers.chars().enumerate() {
                    let at = golden.columns[i].as_ref().map(|(offset, ground)| {
                        let ground = props
                            .default_state(ground)
                            .unwrap_or(ground.as_str())
                            .to_string();
                        (*offset, ground)
                    });
                    let world = OneBlock { at, brightness };
                    let n = Neighbourhood::new(&world, props, (0, 0, 0));
                    let mut got = can_survive(&n, state, Some(class), may_place_on);
                    if break_the_torch && class.ends_with("BaseTorchBlock") {
                        got = match got {
                            Survives::No => Survives::Yes,
                            other => other,
                        };
                    }
                    match got {
                        Survives::Unported => {
                            if !counted_unported {
                                unported += 1;
                                counted_unported = true;
                            }
                        }
                        Survives::Yes if expected == 'Y' => agreed += 1,
                        Survives::No if expected == 'N' => agreed += 1,
                        other => {
                            if disagreements.len() < 20 {
                                disagreements.push(format!(
                                    "{state} @ {} light {brightness}: vanilla {expected}, port \
                                     {other:?}",
                                    golden.columns[i]
                                        .as_ref()
                                        .map_or("air".to_string(), |(o, g)| format!("{o:?}={g}")),
                                ));
                            } else {
                                disagreements.push(String::new());
                            }
                        }
                    }
                }
            }
        }
    }
    (agreed, disagreements, unported)
}

/// Every ported family agrees with vanilla on every cell.
#[test]
fn ported_families_match_vanilla_exactly() {
    let props = BlockPropsTable::load(&data("block_props.json")).expect("block_props.json");
    let golden = load();
    let (agreed, disagreements, unported) = sweep(&props, &golden, false);
    let shown: Vec<&String> = disagreements.iter().filter(|d| !d.is_empty()).collect();
    assert!(
        disagreements.is_empty(),
        "{} of {} cells disagree with vanilla; first {}:\n{}",
        disagreements.len(),
        agreed + disagreements.len() as u64,
        shown.len(),
        shown
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    // Not a threshold — the exact figure the sweep produces, so a run that
    // compared nothing is as red as one that compared and disagreed. This is
    // the `0 of 539 stacks opaque` guard: an assertion on the denominator.
    assert_eq!(
        agreed, 640_532,
        "the ported families cover {agreed} cells; the number moves when a family is added \
         or removed and is pinned so that cannot happen silently"
    );
    assert_eq!(
        unported, 2546,
        "{unported} states fall in a class this port does not answer"
    );
}

/// The classes this port does not answer, by name, so one dropping out of
/// the dispatch is a named failure rather than a moved total.
#[test]
fn the_unported_classes_are_the_ones_named_here() {
    let golden = load();
    let classes: Vec<(String, Option<String>)> = golden.table["table"]
        .as_array()
        .expect("table")
        .iter()
        .filter_map(|row| {
            let rule = row["rule"].as_str()?.to_string();
            Some((rule, row["mayPlaceOn"].as_str().map(ToString::to_string)))
        })
        .collect();
    let unported = unported_classes(
        classes
            .iter()
            .map(|(class, may)| (class.as_str(), may.as_deref())),
    );
    let short: Vec<&str> = unported
        .iter()
        .map(|c| c.rsplit('.').next().unwrap_or(c))
        .collect();
    assert_eq!(
        short,
        [
            "BambooStalkBlock",
            "BaseCoralPlantTypeBlock",
            "BaseCoralWallFanBlock",
            "BellBlock",
            "BigDripleafBlock",
            "CactusBlock",
            "CakeBlock",
            "ChorusFlowerBlock",
            "ChorusPlantBlock",
            "CocoaBlock",
            "DirtPathBlock",
            "FarmlandBlock",
            "FrogspawnBlock",
            "GrindstoneBlock",
            "GrowingPlantBlock",
            "HangingMossBlock",
            "HangingRootsBlock",
            "LeafLitterBlock",
            "MangrovePropaguleBlock",
            "MossyCarpetBlock",
            "MultifaceBlock",
            "NetherFungusBlock",
            "NetherRootsBlock",
            "PitcherCropBlock",
            "RedStoneWireBlock",
            "ScaffoldingBlock",
            "SeaPickleBlock",
            "SmallDripleafBlock",
            "SpeleothemBlock",
            "SporeBlossomBlock",
            "StemBlock",
            "SugarCaneBlock",
            "TripWireHookBlock",
            "VineBlock",
        ],
        "the unported set moved; port it or record it, but do not let the list drift"
    );
}

/// The positive control: a rule that is wrong must be caught.
///
/// `BaseTorchBlock` is the family the placement gate exists for — a torch on
/// air — and the mutation is the exact defect this server had before the
/// gate: every no becomes a yes. If the sweep still passes, it is not
/// reading the port.
#[test]
fn a_wrong_rule_is_caught() {
    let props = BlockPropsTable::load(&data("block_props.json")).expect("block_props.json");
    let golden = load();
    let (_, disagreements, _) = sweep(&props, &golden, true);
    assert!(
        !disagreements.is_empty(),
        "a BaseTorchBlock that survives on air was not caught — the sweep is not reading the \
         port's answer"
    );
}
