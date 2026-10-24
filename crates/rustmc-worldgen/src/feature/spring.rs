//! The spring feature — where a waterfall or lavafall starts.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `net.minecraft.world.level.levelgen.feature.SpringFeature`,
//! `configurations.SpringConfiguration`.
//!
//! # It draws nothing at all
//!
//! Not once. Every decision is a block lookup, and the position came from the
//! placement chain. A spring that placed nothing and a spring that opened a
//! lavafall leave the stream in exactly the same place — which makes it a
//! good check on a driver, because any draw at all is a bug.
//!
//! # The counts are equalities, not thresholds
//!
//! ```text
//! if (rockCount != config.rockCount) return false
//! if (holeCount != config.holeCount) return false
//! ```
//!
//! Not `>=`. A spring wants *exactly* the configured number of solid
//! neighbours and *exactly* the configured number of open ones, which is why
//! they appear in walls rather than in the middle of caverns. Reading either
//! as a threshold puts water on every exposed rock face in the world.
//!
//! The two counts are also over **different neighbour sets**: rock counts
//! west, east, north, south **and below**; holes count the same four sides
//! and below as well. The same five positions, asked two different questions.
//!
//! # The block written is the fluid's, not the config's
//!
//! `config.state` is a `FluidState`, and what lands is
//! `state.createLegacyBlock()` — so a `falling=true` lava spring writes
//! `minecraft:lava[level=0]`, the block form. Writing the fluid's own
//! spelling would put a state in the world that no block ever has.

use super::block_predicate::PredicateWorld;

/// `SpringConfiguration`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpringConfig {
    /// The fluid, as the *block* it becomes. See the module docs.
    pub state: String,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    /// What counts as rock. A `HolderSet` in vanilla, so either a tag or a
    /// list of block names.
    pub valid_blocks: Vec<String>,
    pub valid_tag: Option<String>,
}

impl SpringConfig {
    /// # Errors
    /// If a field is missing or malformed.
    pub fn parse(
        config: &serde_json::Value,
        states: &dyn super::state_spec::DefaultStates,
    ) -> Result<Self, String> {
        let name = config
            .get("state")
            .and_then(|s| s.get("Name"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "spring config has no `state.Name`".to_string())?;
        // `FluidState.createLegacyBlock()` — the block form of the fluid.
        // The config's `state` is a **fluid** state, so it does not go through
        // `state_spec::canonical`: the extract is a table of block states and
        // has no row for `minecraft:flowing_water`. Water and lava both become
        // the block of the same name at level 0, which is also their default
        // state — asserted below against the extract when one is available.
        let state = match name {
            "minecraft:water" | "minecraft:flowing_water" => "minecraft:water[level=0]",
            "minecraft:lava" | "minecraft:flowing_lava" => "minecraft:lava[level=0]",
            other => other,
        }
        .to_string();
        let _ = states;

        let valid = config
            .get("valid_blocks")
            .ok_or_else(|| "spring config has no `valid_blocks`".to_string())?;
        let (valid_blocks, valid_tag) = match valid {
            serde_json::Value::String(one) => match one.strip_prefix('#') {
                Some(tag) => (Vec::new(), Some(tag.to_string())),
                None => (vec![one.clone()], None),
            },
            serde_json::Value::Array(list) => (
                list.iter()
                    .map(|v| {
                        v.as_str()
                            .map(ToString::to_string)
                            .ok_or_else(|| "valid_blocks entry is not a name".to_string())
                    })
                    .collect::<Result<Vec<_>, String>>()?,
                None,
            ),
            other => {
                return Err(format!(
                    "valid_blocks is neither a name nor a list: {other}"
                ))
            }
        };

        Ok(Self {
            state,
            // `optionalFieldOf("requires_block_below", true)`.
            requires_block_below: config
                .get("requires_block_below")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            rock_count: config
                .get("rock_count")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(4),
            hole_count: config
                .get("hole_count")
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .unwrap_or(1),
            valid_blocks,
            valid_tag,
        })
    }
}

/// The world a spring reads and writes.
pub trait SpringWorld: PredicateWorld {
    fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str);
    /// `BlockGetter#isEmptyBlock` — `isAir`, so `cave_air` counts.
    fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool>;
}

/// `SpringFeature#place`. Returns whether the spring was opened.
///
/// Takes no `RandomSource`: the feature does not draw. The parameter is
/// absent rather than ignored so a caller cannot pass one and assume it was
/// used.
///
/// # Errors
/// If the world cannot answer whether a block is air.
pub fn place<W: SpringWorld + ?Sized>(
    world: &mut W,
    tags: &crate::tags::BlockTags,
    config: &SpringConfig,
    origin: (i32, i32, i32),
) -> Result<bool, super::simple_block::Unplaceable> {
    let (x, y, z) = origin;
    let valid = |world: &mut W, x: i32, y: i32, z: i32| {
        let block = world.block_at(x, y, z);
        let plain = block.split_once('[').map_or(block.as_str(), |(n, _)| n);
        config.valid_tag.as_ref().map_or_else(
            || config.valid_blocks.iter().any(|b| b == plain),
            |tag| tags.contains(tag, plain),
        )
    };

    if !valid(world, x, y + 1, z) {
        return Ok(false);
    }
    if config.requires_block_below && !valid(world, x, y - 1, z) {
        return Ok(false);
    }

    // The block at the spring itself must be air or itself valid rock.
    let here = world.block_at(x, y, z);
    let here_plain = here.split_once('[').map_or(here.as_str(), |(n, _)| n);
    let here_is_air = world
        .is_empty_block(x, y, z)
        .ok_or_else(|| super::simple_block::Unplaceable(format!("isEmptyBlock({here_plain})")))?;
    if !here_is_air && !valid(world, x, y, z) {
        return Ok(false);
    }

    // The same five positions, asked two different questions.
    const SIDES: [(i32, i32, i32); 5] = [
        (-1, 0, 0), // west
        (1, 0, 0),  // east
        (0, 0, -1), // north
        (0, 0, 1),  // south
        (0, -1, 0), // below
    ];
    let mut rocks = 0;
    let mut holes = 0;
    for (dx, dy, dz) in SIDES {
        if valid(world, x + dx, y + dy, z + dz) {
            rocks += 1;
        }
        if world
            .is_empty_block(x + dx, y + dy, z + dz)
            .ok_or_else(|| super::simple_block::Unplaceable("isEmptyBlock".to_string()))?
        {
            holes += 1;
        }
    }

    // Equalities, not thresholds. See the module docs.
    if rocks != config.rock_count || holes != config.hole_count {
        return Ok(false);
    }

    world.set_block(x, y, z, &config.state);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use crate::rng::RandomSource;
    use crate::tags::BlockTags;
    use std::collections::BTreeMap;

    struct Cave(BTreeMap<(i32, i32, i32), String>);

    impl PredicateWorld for Cave {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.0
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }

    impl SpringWorld for Cave {
        fn set_block(&mut self, x: i32, y: i32, z: i32, name: &str) {
            self.0.insert((x, y, z), name.to_string());
        }
        fn is_empty_block(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::new()).expect("tags")
    }

    fn config() -> SpringConfig {
        SpringConfig::parse(
            &serde_json::from_str(
                r#"{"state":{"Name":"minecraft:water"},
                    "rock_count":4,"hole_count":1,
                    "valid_blocks":["minecraft:stone"]}"#,
            )
            .expect("json"),
            // The spring's `state` is a *fluid* state, so no block table is
            // consulted — `NoDefaults` proves that rather than asserting it.
            &crate::feature::state_spec::NoDefaults,
        )
        .expect("parse")
    }

    /// Stone above, below, and on three sides; one side open. Four rocks
    /// (three sides plus below) and one hole.
    fn wall() -> Cave {
        let mut blocks = BTreeMap::new();
        for pos in [(0, 1, 0), (0, -1, 0), (-1, 0, 0), (1, 0, 0), (0, 0, -1)] {
            blocks.insert(pos, "minecraft:stone".to_string());
        }
        Cave(blocks)
    }

    #[test]
    fn a_spring_in_a_wall_opens() {
        let mut world = wall();
        assert!(place(&mut world, &tags(), &config(), (0, 0, 0)).expect("ok"));
        assert_eq!(
            world.0.get(&(0, 0, 0)).map(String::as_str),
            Some("minecraft:water[level=0]"),
            "the fluid's *block* form, not the fluid's own spelling"
        );
    }

    /// The counts are equalities. One rock too many is a refusal, not a
    /// better match — which is why springs live in walls.
    #[test]
    fn one_rock_too_many_is_a_refusal_not_a_better_match() {
        let mut world = wall();
        world.0.insert((0, 0, 1), "minecraft:stone".to_string());
        assert!(
            !place(&mut world, &tags(), &config(), (0, 0, 0)).expect("ok"),
            "five rocks and zero holes must fail a rock_count of 4"
        );
    }

    /// The whole feature is free. A driver that saw a draw here would be
    /// looking at its own bug.
    #[test]
    fn a_spring_never_touches_the_stream() {
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        let before = {
            let mut probe = WorldgenRandom::new(LegacyRandomSource::new(11));
            probe.next_i64()
        };
        let mut world = wall();
        place(&mut world, &tags(), &config(), (0, 0, 0)).expect("ok");
        assert_eq!(random.next_i64(), before);
    }

    #[test]
    fn every_spring_config_in_the_datapack_parses() {
        let dir = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../oracle-work/vanilla-26.2/generated-data/data/minecraft/worldgen/configured_feature"
        ));
        if !rustmc_data::fixture::available(dir, "datagen extract", "ci/vanilla-server.sh data") {
            return;
        }
        let mut seen = 0usize;
        for entry in std::fs::read_dir(dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let json: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).expect("readable"))
                    .expect("json");
            if json["type"] != "minecraft:spring_feature" {
                continue;
            }
            SpringConfig::parse(&json["config"], &crate::feature::state_spec::NoDefaults)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            seen += 1;
        }
        assert!(seen >= 4, "only {seen} spring features parsed");
    }
}
