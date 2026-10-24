//! The block-column feature — a stack of layers grown in one direction.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.BlockColumnFeature`,
//! `configurations.BlockColumnConfiguration`.
//!
//! A cactus is three layers: one to three cactus blocks, then a possible
//! flower, then nothing. Kelp and dripleaf are the same shape.
//!
//! # Every layer's height is drawn, before anything is placed
//!
//! ```text
//! int[] heights = new int[layers.size()];
//! for (i : layers) heights[i] = layers.get(i).height().sample(random);
//! ```
//!
//! All of them, in order, and **then** the column is walked. A layer whose
//! height comes out zero still drew for it, and a column that turns out to
//! have no room at all has already paid for every layer. Sampling lazily as
//! the column is walked would take the same draws in a different order and
//! move everything after.
//!
//! # Truncation is not "stop when you run out"
//!
//! When the column does not fit, `truncate` removes blocks from one end —
//! and which end depends on `prioritizeTip`:
//!
//! ```text
//! prioritizeTip ? walk layers forward : walk them backward
//! ```
//!
//! A cactus prioritises its tip, so a cactus with room for two blocks keeps
//! the flower and drops a body segment rather than the other way round. The
//! *draws* are unaffected — they already happened — so this changes the
//! blocks and nothing else, which is exactly the kind of difference a
//! draw-count check cannot see.

use crate::rng::RandomSource;
use crate::tags::BlockTags;

use super::block_predicate::{BlockPredicate, PredicateWorld};
use super::int_provider::IntProvider;
use super::state_provider::StateProvider;

/// One layer of the column.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub height: IntProvider,
    pub provider: StateProvider,
}

/// `BlockColumnConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockColumnConfig {
    pub layers: Vec<Layer>,
    /// `Direction` as an index into `Direction.values()`.
    pub direction: usize,
    pub allowed_placement: BlockPredicate,
    pub prioritize_tip: bool,
}

impl BlockColumnConfig {
    /// # Errors
    /// If a field is missing or names something unimplemented.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        Ok(Self {
            layers: config
                .get("layers")
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| "block_column config has no `layers`".to_string())?
                .iter()
                .map(|layer| {
                    Ok(Layer {
                        height: IntProvider::parse(
                            layer
                                .get("height")
                                .ok_or_else(|| "layer has no `height`".to_string())?,
                        )?,
                        provider: StateProvider::parse(
                            layer
                                .get("provider")
                                .ok_or_else(|| "layer has no `provider`".to_string())?,
                            states,
                        )?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
            direction: match config
                .get("direction")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| "block_column config has no `direction`".to_string())?
            {
                "down" => 0,
                "up" => 1,
                "north" => 2,
                "south" => 3,
                "west" => 4,
                "east" => 5,
                other => return Err(format!("block_column: unknown direction `{other}`")),
            },
            allowed_placement: BlockPredicate::parse(
                config
                    .get("allowed_placement")
                    .ok_or_else(|| "block_column config has no `allowed_placement`".to_string())?,
            )?,
            prioritize_tip: config
                .get("prioritize_tip")
                .and_then(serde_json::Value::as_bool)
                .ok_or_else(|| "block_column config has no `prioritize_tip`".to_string())?,
        })
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        self.allowed_placement.is_supported()
            && self.layers.iter().all(|l| l.provider.is_supported())
    }
}

/// The world a column reads and writes.
pub trait ColumnWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);
}

/// One step in a `Direction.values()` direction.
const fn step(direction: usize) -> (i32, i32, i32) {
    match direction {
        0 => (0, -1, 0),
        1 => (0, 1, 0),
        2 => (0, 0, -1),
        3 => (0, 0, 1),
        4 => (-1, 0, 0),
        _ => (1, 0, 0),
    }
}

/// `BlockColumnFeature#place`. Returns whether anything was placed.
///
/// # Errors
/// If a predicate or provider needs behaviour the port lacks.
pub fn place<W: ColumnWorld + ?Sized, R: RandomSource>(
    world: &mut W,
    tags: &BlockTags,
    config: &BlockColumnConfig,
    random: &mut R,
    origin: (i32, i32, i32),
) -> Result<bool, super::block_predicate::NeedsBehaviour> {
    // Every height first, in order. See the module docs.
    let mut heights: Vec<i32> = config
        .layers
        .iter()
        .map(|layer| layer.height.sample(random))
        .collect();
    let total: i32 = heights.iter().sum();
    if total == 0 {
        return Ok(false);
    }

    let (dx, dy, dz) = step(config.direction);

    // The fit walk starts **one step in**, not at the origin.
    //
    // Vanilla keeps two cursors: `pos` at the origin, which is what the write
    // loop below uses, and a copy that is moved once in `direction` *before*
    // the loop is entered (`pos.mutable().move(direction)`), which is what
    // `allowedPlacement` is tested against. So the origin block is written
    // without ever being tested — the caller has already vouched for it —
    // and `room` counts the blocks *after* it.
    //
    // Starting the walk at the origin instead makes `room` one larger
    // whenever the origin is allowed, which is invisible until the column is
    // short enough for `truncate` to bind. Then it keeps one block too many
    // and spends one extra `getState` draw, which moves every later draw in
    // the chunk. That is the whole of the lush-caves defect: 219 draws where
    // vanilla spends 218.
    let (mut x, mut y, mut z) = (origin.0 + dx, origin.1 + dy, origin.2 + dz);

    // How far the column can actually go before something blocks it.
    let mut room = 0;
    while room < total {
        if !config.allowed_placement.test(world, tags, (x, y, z))? {
            break;
        }
        x += dx;
        y += dy;
        z += dz;
        room += 1;
    }
    if room == 0 {
        return Ok(false);
    }
    truncate(&mut heights, total, room, config.prioritize_tip);

    // Walk again from the origin, writing.
    let (mut x, mut y, mut z) = origin;
    let mut placed = false;
    for (layer, height) in config.layers.iter().zip(&heights) {
        for _ in 0..*height {
            let state = layer.provider.state(world, tags, random, (x, y, z))?;
            world.set_block(x, y, z, &state);
            placed = true;
            x += dx;
            y += dy;
            z += dz;
        }
    }
    Ok(placed)
}

/// `BlockColumnFeature#truncate`.
///
/// Removes `total - room` blocks, walking the layers from one end. Forward
/// when the tip is *not* prioritised, backward when it is — so the layers
/// nearest the tip are the last to be given up.
fn truncate(heights: &mut [i32], total: i32, room: i32, prioritize_tip: bool) {
    let mut excess = total - room;
    if excess <= 0 {
        return;
    }
    let indices: Vec<usize> = if prioritize_tip {
        (0..heights.len()).collect()
    } else {
        (0..heights.len()).rev().collect()
    };
    for index in indices {
        if excess <= 0 {
            break;
        }
        let taken = excess.min(heights[index]);
        heights[index] -= taken;
        excess -= taken;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    struct Desert(BTreeMap<(i32, i32, i32), String>);

    impl PredicateWorld for Desert {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl ColumnWorld for Desert {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.0.insert((x, y, z), name.to_string());
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(
            [(
                "minecraft:air".to_string(),
                vec!["minecraft:air".to_string()],
            )]
            .into_iter()
            .collect(),
        )
        .expect("tags")
    }

    /// Real default-state spellings, copied out of the block-props extract.
    fn states() -> super::super::state_spec::DefaultStateTable {
        super::super::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:cactus", "minecraft:cactus[age=0]"),
            ("minecraft:cactus_flower", "minecraft:cactus_flower"),
        ])
    }

    fn config(prioritize_tip: bool) -> BlockColumnConfig {
        BlockColumnConfig::parse(
            &serde_json::from_str(&format!(
                r#"{{"direction":"up","prioritize_tip":{prioritize_tip},
                     "allowed_placement":{{"type":"minecraft:matching_block_tag",
                                           "tag":"minecraft:air"}},
                     "layers":[
                        {{"height":2,"provider":{{"type":"minecraft:simple_state_provider",
                                                  "state":{{"Name":"minecraft:cactus"}}}}}},
                        {{"height":1,"provider":{{"type":"minecraft:simple_state_provider",
                                                  "state":{{"Name":"minecraft:cactus_flower"}}}}}}
                     ]}}"#
            ))
            .expect("json"),
            &states(),
        )
        .expect("parse")
    }

    #[test]
    fn a_column_with_room_places_every_layer() {
        let mut world = Desert(BTreeMap::new());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        assert!(place(&mut world, &tags(), &config(true), &mut random, (0, 0, 0)).expect("ok"));
        assert_eq!(
            world.0.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:cactus[age=0]")
        );
        assert_eq!(
            world.0.get(&(0, 1, 0)).map(String::as_str),
            Some("minecraft:cactus[age=0]")
        );
        assert_eq!(
            world.0.get(&(0, 2, 0)).map(String::as_str),
            Some("minecraft:cactus_flower")
        );
    }

    /// One block of room for a three-block column. With the tip prioritised
    /// the flower survives and both body segments go.
    ///
    /// One block, not two, because the fit walk starts at `origin + direction`
    /// — vanilla writes the origin without testing it and counts the room
    /// *after* it. Stone at y 2 therefore leaves only y 1 to test, and the
    /// column truncates to a single block at the origin.
    #[test]
    fn prioritizing_the_tip_keeps_the_tip() {
        let mut world = Desert(BTreeMap::new());
        world.0.insert((0, 2, 0), "minecraft:stone".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        place(&mut world, &tags(), &config(true), &mut random, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.0.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:cactus_flower"),
            "one block of room and the tip prioritised, so the tip is the one \
             block written — `truncate` walks from layer 0 and takes the body \
             first"
        );
        assert_eq!(
            world.0.get(&(0, 1, 0)),
            None,
            "only one block fits, so nothing is written above the origin"
        );
    }

    /// The same column without the flag drops the tip instead.
    #[test]
    fn not_prioritizing_the_tip_drops_it() {
        let mut world = Desert(BTreeMap::new());
        world.0.insert((0, 2, 0), "minecraft:stone".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(1));
        place(&mut world, &tags(), &config(false), &mut random, (0, 0, 0)).expect("ok");
        assert_eq!(
            world.0.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:cactus[age=0]")
        );
        assert_eq!(
            world.0.get(&(0, 1, 0)),
            None,
            "only one block fits; the flag decides which layer keeps it"
        );
    }

    /// A blocked column has already paid for every layer's height.
    ///
    /// Blocked at `origin + direction`, not at the origin: the fit walk never
    /// tests the origin, so a block *there* does not stop the column. That is
    /// vanilla's shape — the caller has already vouched for the origin — and
    /// putting the obstruction one higher is what makes this a blocked column
    /// at all.
    #[test]
    fn a_blocked_column_still_drew_for_every_layer() {
        let mut world = Desert(BTreeMap::new());
        world.0.insert((0, 1, 0), "minecraft:stone".to_string());
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert!(!place(&mut world, &tags(), &config(true), &mut random, (0, 0, 0)).expect("ok"));

        // Both layers here are constants, so the reference stream is
        // untouched — the point is that the *walk* did not consume anything
        // extra either.
        let mut reference = WorldgenRandom::new(LegacyRandomSource::new(4));
        assert_eq!(random.next_i64(), reference.next_i64());
    }

    #[test]
    fn every_block_column_config_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        let Some(extract) = crate::feature::state_spec::extract_or_skip() else {
            return;
        };
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:block_column" {
                continue;
            }
            BlockColumnConfig::parse(&json["config"], &extract)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            seen += 1;
        }
        assert!(seen >= 4, "only {seen} block_column features parsed");
    }
}
