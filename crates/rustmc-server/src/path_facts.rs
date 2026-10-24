//! The per-state block facts a mob reads, keyed by global block-state id.
//!
//! Reference (26.2): `BlockBehaviour.BlockStateBase.isPathfindable`,
//! `blocksMotion`, `isAir`, `getFluidState`, `FlowingFluid.getOwnHeight`,
//! `Block.getJumpFactor`, and the block tags `WalkNodeEvaluator`,
//! `EatBlockGoal` and `InteractWithDoor` test.
//!
//! `crates/rustmc-entity/src/host.rs::BlockFacts` is the list of tests the
//! transcribed AI makes against a block. Every one of them is a virtual
//! method with per-block overrides (`isPathfindable` has fourteen), a tag
//! bound at datapack reload, or a `Block` field set from `Properties`; none
//! is a function of the state's name. So they come out of the jar through
//! `tools/goldengen/vanilla/PathFactsGolden.java`, de-duplicated into rows
//! with an int per state, and this module only reads them.
//!
//! `block_props.json` is not reused for the columns it shares (`air`,
//! `blocksMotion`, `solid`, `solidRender`, `redstoneConductor`) because that
//! table is keyed by state *string* and the pathfinder asks by state id from
//! inside a node scan; one table, one lookup.
use std::collections::BTreeMap;
use std::io;

use rustmc_entity::host::{BlockFacts, DoorFacts};

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping `crates/rustmc-server/src` for
/// `"data/vanilla/<name>.json"`.
pub const TABLE_PATH: &str = "data/vanilla/path_facts.json";

/// One distinct fact tuple. Many states share one row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Index into [`PathFacts::blocks`].
    pub block: u32,
    pub air: bool,
    pub blocks_motion: bool,
    pub solid: bool,
    pub solid_render: bool,
    pub redstone_conductor: bool,
    pub path_land: bool,
    pub path_water: bool,
    pub path_air: bool,
    /// 0 none, 1 water, 2 lava.
    pub fluid_kind: u8,
    pub fluid_source: bool,
    pub fluid_amount: i32,
    /// `FlowingFluid.getOwnHeight` — `amount / 9f`. Read as bits.
    pub fluid_own_height: f32,
    /// `Block.getJumpFactor()`. Read as bits.
    pub jump_factor: f32,
    pub tag_trapdoors: bool,
    pub tag_fences: bool,
    pub tag_walls: bool,
    pub tag_fire: bool,
    pub tag_beds: bool,
    pub tag_speleothems: bool,
    pub tag_edible_for_sheep: bool,
    pub tag_cauldrons: bool,
    pub tag_doors: bool,
    pub tag_mob_interactable_doors: bool,
    pub door: Option<DoorFacts>,
    pub fence_gate_open: Option<bool>,
    pub leaves: bool,
    pub rail: bool,
    pub lit_campfire: bool,
    pub bed_head: bool,
}

/// The table.
#[derive(Debug)]
pub struct PathFacts {
    /// Block registry ids, in registry order, leaked once so a `BlockFacts`
    /// can carry a `&'static str`.
    blocks: Vec<&'static str>,
    rows: Vec<Row>,
    state_rows: Vec<u32>,
    /// The blocks a `BlockFacts` names individually, resolved once.
    by_name: BTreeMap<&'static str, u32>,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("path_facts.json: {what}"),
    )
}

fn bits_f32(v: Option<&serde_json::Value>) -> Option<f32> {
    let bits = v?.as_i64()?;
    #[expect(clippy::cast_possible_truncation, reason = "raw IEEE-754 bits")]
    #[expect(clippy::cast_sign_loss, reason = "raw IEEE-754 bits")]
    Some(f32::from_bits(bits as u32))
}

impl PathFacts {
    /// Load the table from the working directory, or from the workspace
    /// root when run as a test.
    ///
    /// # Errors
    /// On a missing or malformed file.
    pub fn load_default() -> io::Result<Self> {
        let cwd_relative = std::path::Path::new(TABLE_PATH);
        let path: std::path::PathBuf = if cwd_relative.exists() {
            cwd_relative.to_path_buf()
        } else {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(TABLE_PATH)
        };
        Self::load(&path)
    }

    /// # Errors
    /// On a missing or malformed file.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Self::from_json(&doc)
    }

    /// # Errors
    /// On a malformed document.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let blocks: Vec<&'static str> = doc["blocks"]
            .as_array()
            .ok_or_else(|| bad("no blocks"))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(|s| &*Box::leak(s.to_string().into_boxed_str()))
                    .ok_or_else(|| bad("a block name is not a string"))
            })
            .collect::<io::Result<_>>()?;
        let rows_json = doc["rows"].as_array().ok_or_else(|| bad("no rows"))?;
        let mut rows = Vec::with_capacity(rows_json.len());
        for r in rows_json {
            let b = |k: &str| r[k].as_bool();
            let block = u32::try_from(r["block"].as_u64().ok_or_else(|| bad("row.block"))?)
                .map_err(|_| bad("row.block range"))?;
            if usize::try_from(block).map_err(|_| bad("row.block"))? >= blocks.len() {
                return Err(bad("row.block out of range"));
            }
            let door = match (b("doorOpen"), b("doorByHand")) {
                (Some(open), Some(can_open_by_hand)) => Some(DoorFacts {
                    open,
                    can_open_by_hand,
                }),
                (None, None) => None,
                _ => return Err(bad("half a door row")),
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "0, 1 or 2 by the generator's own check"
            )]
            let fluid_kind = r["fluidKind"]
                .as_u64()
                .ok_or_else(|| bad("row.fluidKind"))? as u8;
            #[expect(clippy::cast_possible_truncation, reason = "a fluid amount is 0..=8")]
            let fluid_amount = r["fluidAmount"]
                .as_i64()
                .ok_or_else(|| bad("row.fluidAmount"))? as i32;
            rows.push(Row {
                block,
                air: b("air").ok_or_else(|| bad("row.air"))?,
                blocks_motion: b("blocksMotion").ok_or_else(|| bad("row.blocksMotion"))?,
                solid: b("solid").ok_or_else(|| bad("row.solid"))?,
                solid_render: b("solidRender").ok_or_else(|| bad("row.solidRender"))?,
                redstone_conductor: b("redstoneConductor")
                    .ok_or_else(|| bad("row.redstoneConductor"))?,
                path_land: b("pathLand").ok_or_else(|| bad("row.pathLand"))?,
                path_water: b("pathWater").ok_or_else(|| bad("row.pathWater"))?,
                path_air: b("pathAir").ok_or_else(|| bad("row.pathAir"))?,
                fluid_kind,
                fluid_source: b("fluidSource").ok_or_else(|| bad("row.fluidSource"))?,
                fluid_amount,
                fluid_own_height: bits_f32(r.get("fluidOwnHeightBits"))
                    .ok_or_else(|| bad("row.fluidOwnHeightBits"))?,
                jump_factor: bits_f32(r.get("jumpFactorBits"))
                    .ok_or_else(|| bad("row.jumpFactorBits"))?,
                tag_trapdoors: b("tag_trapdoors").ok_or_else(|| bad("row.tag_trapdoors"))?,
                tag_fences: b("tag_fences").ok_or_else(|| bad("row.tag_fences"))?,
                tag_walls: b("tag_walls").ok_or_else(|| bad("row.tag_walls"))?,
                tag_fire: b("tag_fire").ok_or_else(|| bad("row.tag_fire"))?,
                tag_beds: b("tag_beds").ok_or_else(|| bad("row.tag_beds"))?,
                tag_speleothems: b("tag_speleothems").ok_or_else(|| bad("row.tag_speleothems"))?,
                tag_edible_for_sheep: b("tag_edible_for_sheep")
                    .ok_or_else(|| bad("row.tag_edible_for_sheep"))?,
                tag_cauldrons: b("tag_cauldrons").ok_or_else(|| bad("row.tag_cauldrons"))?,
                tag_doors: b("tag_doors").ok_or_else(|| bad("row.tag_doors"))?,
                tag_mob_interactable_doors: b("tag_mob_interactable_doors")
                    .ok_or_else(|| bad("row.tag_mob_interactable_doors"))?,
                door,
                fence_gate_open: b("fenceGateOpen"),
                leaves: b("leaves").ok_or_else(|| bad("row.leaves"))?,
                rail: b("rail").ok_or_else(|| bad("row.rail"))?,
                lit_campfire: b("litCampfire").ok_or_else(|| bad("row.litCampfire"))?,
                bed_head: b("bedHead").ok_or_else(|| bad("row.bedHead"))?,
            });
        }
        let state_rows: Vec<u32> = doc["stateRows"]
            .as_array()
            .ok_or_else(|| bad("no stateRows"))?
            .iter()
            .map(|v| {
                let i = v.as_u64().ok_or_else(|| bad("stateRows entry"))?;
                let i = u32::try_from(i).map_err(|_| bad("stateRows range"))?;
                if usize::try_from(i).map_err(|_| bad("stateRows range"))? >= rows.len() {
                    return Err(bad("stateRows entry out of range"));
                }
                Ok(i)
            })
            .collect::<io::Result<_>>()?;
        let declared = doc["states"]
            .as_u64()
            .ok_or_else(|| bad("no states count"))?;
        if declared != state_rows.len() as u64 {
            return Err(bad("states count disagrees with stateRows"));
        }
        let by_name = blocks
            .iter()
            .enumerate()
            .map(|(i, n)| (*n, u32::try_from(i).unwrap_or(u32::MAX)))
            .collect();
        Ok(Self {
            blocks,
            rows,
            state_rows,
            by_name,
        })
    }

    /// The process-wide table, loaded once; `None` when the extract is absent.
    pub fn shared() -> Option<&'static Self> {
        static TABLE: std::sync::OnceLock<Option<PathFacts>> = std::sync::OnceLock::new();
        TABLE.get_or_init(|| Self::load_default().ok()).as_ref()
    }

    /// The row for a global state id, or `None` past the table.
    #[must_use]
    pub fn row(&self, state_id: u32) -> Option<&Row> {
        let i = *self.state_rows.get(usize::try_from(state_id).ok()?)?;
        self.rows.get(usize::try_from(i).ok()?)
    }

    /// The block's registry id (`minecraft:stone`) for a state.
    #[must_use]
    pub fn block_name(&self, state_id: u32) -> Option<&'static str> {
        let row = self.row(state_id)?;
        self.blocks.get(usize::try_from(row.block).ok()?).copied()
    }

    /// `(states, distinct rows)`.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.state_rows.len(), self.rows.len())
    }

    fn is_block(&self, row: &Row, name: &str) -> bool {
        self.by_name.get(name).is_some_and(|&i| i == row.block)
    }

    /// The AI's view of a state: every `BlockFacts` field the table can
    /// answer. The two block-entity fields (`chest_open_count`,
    /// `furnace_lit`) are the level's to fill; they are `None` here.
    #[must_use]
    pub fn block_facts(&self, state_id: u32) -> Option<BlockFacts> {
        let r = self.row(state_id)?;
        let name = self.blocks.get(usize::try_from(r.block).ok()?).copied()?;
        let short = name.strip_prefix("minecraft:").unwrap_or(name);
        Some(BlockFacts {
            is_air: r.air,
            is_trapdoor: r.tag_trapdoors,
            is_lily_pad: self.is_block(r, "minecraft:lily_pad"),
            is_big_dripleaf: self.is_block(r, "minecraft:big_dripleaf"),
            is_powder_snow: self.is_block(r, "minecraft:powder_snow"),
            is_cactus: self.is_block(r, "minecraft:cactus"),
            is_sweet_berry_bush: self.is_block(r, "minecraft:sweet_berry_bush"),
            is_honey_block: self.is_block(r, "minecraft:honey_block"),
            is_cocoa: self.is_block(r, "minecraft:cocoa"),
            is_wither_rose: self.is_block(r, "minecraft:wither_rose"),
            is_speleothem: r.tag_speleothems,
            fluid_is_lava: r.fluid_kind == 2,
            fluid_is_water: r.fluid_kind == 1,
            // `Fluids.WATER.getSource(false)`: a water source, and a source
            // state never carries the falling bit.
            fluid_is_water_source: r.fluid_kind == 1 && r.fluid_source,
            is_water_block: self.is_block(r, "minecraft:water"),
            is_fire: r.tag_fire,
            is_lava_block: self.is_block(r, "minecraft:lava"),
            is_magma_block: self.is_block(r, "minecraft:magma_block"),
            is_lit_campfire: r.lit_campfire,
            is_lava_cauldron: self.is_block(r, "minecraft:lava_cauldron"),
            door: r.door,
            is_rail: r.rail,
            is_leaves: r.leaves,
            is_fence: r.tag_fences,
            is_wall: r.tag_walls,
            fence_gate_open: r.fence_gate_open,
            is_pathfindable_land: r.path_land,
            is_pathfindable_water: r.path_water,
            block_id: Some(short),
            is_bed: r.tag_beds,
            bed_part_head: r.bed_head,
            chest_open_count: None,
            furnace_lit: None,
            is_edible_for_sheep: r.tag_edible_for_sheep,
            is_grass_block: self.is_block(r, "minecraft:grass_block"),
            blocks_motion: r.blocks_motion,
            is_solid: r.solid,
            is_solid_render: r.solid_render,
            is_cauldron: r.tag_cauldrons,
            is_door_tag: r.tag_doors,
            is_mob_interactable_door: r.tag_mob_interactable_doors,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> PathFacts {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(TABLE_PATH);
        assert!(
            rustmc_data::fixture::available(&path, "path_facts.json", "ci/path-facts-golden.sh"),
            "fixture rule: absent means panic, not skip"
        );
        PathFacts::load(&path).expect("path_facts.json parses")
    }

    #[test]
    fn every_state_has_a_row_and_air_is_walkable_where_stone_is_not() {
        let t = table();
        let (states, rows) = t.counts();
        assert!(states > 30_000, "{states} states");
        assert!(rows > 1_000 && rows < states, "{rows} rows");
        let air = (0..states)
            .map(|i| u32::try_from(i).unwrap())
            .find(|&i| t.block_name(i) == Some("minecraft:air"))
            .unwrap();
        let stone = (0..states)
            .map(|i| u32::try_from(i).unwrap())
            .find(|&i| t.block_name(i) == Some("minecraft:stone"))
            .unwrap();
        let a = t.block_facts(air).unwrap();
        let s = t.block_facts(stone).unwrap();
        assert!(a.is_air && a.is_pathfindable_land && !a.blocks_motion);
        assert!(!s.is_air && !s.is_pathfindable_land && s.blocks_motion && s.is_solid);
        assert_eq!(s.block_id, Some("stone"));
    }

    #[test]
    fn a_water_source_reads_as_the_still_source_and_flowing_water_does_not() {
        let t = table();
        let (states, _) = t.counts();
        let mut source = None;
        let mut flowing = None;
        for i in 0..states {
            let i = u32::try_from(i).unwrap();
            if t.block_name(i) == Some("minecraft:water") {
                let r = t.row(i).unwrap();
                if r.fluid_source {
                    source = Some(i);
                } else {
                    flowing = Some(i);
                }
            }
        }
        let s = t.block_facts(source.unwrap()).unwrap();
        let f = t.block_facts(flowing.unwrap()).unwrap();
        assert!(s.fluid_is_water && s.fluid_is_water_source && s.is_water_block);
        assert!(f.fluid_is_water && !f.fluid_is_water_source);
        assert_eq!(t.row(source.unwrap()).unwrap().fluid_own_height, 8.0 / 9.0);
    }

    #[test]
    fn a_door_row_carries_its_open_bit_and_a_stone_row_does_not() {
        let t = table();
        let (states, _) = t.counts();
        let mut seen = (false, false);
        for i in 0..states {
            let i = u32::try_from(i).unwrap();
            if t.block_name(i) == Some("minecraft:oak_door") {
                let d = t.block_facts(i).unwrap().door.expect("a door");
                assert!(d.can_open_by_hand);
                if d.open {
                    seen.0 = true;
                } else {
                    seen.1 = true;
                }
            } else if t.block_name(i) == Some("minecraft:stone") {
                assert!(t.block_facts(i).unwrap().door.is_none());
            }
        }
        assert_eq!(seen, (true, true));
    }
}
