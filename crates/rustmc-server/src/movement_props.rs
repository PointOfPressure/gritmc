//! The per-block floats and predicates the movement tick reads, keyed by
//! global block-state id.
//!
//! Reference (26.2): `Block.getFriction`, `Block.getSpeedFactor`,
//! `Block.getBounceRestitution`, `Entity.getBlockBounciness`,
//! `Entity.getBlockSpeedFactor`, `Entity.getOnPos`.
//!
//! `block_props.json` cannot carry these: it is keyed by block **state** and
//! these are `Block` fields, set from `BlockBehaviour.Properties` at
//! construction. There is no rule connecting them to a block's name — ice is
//! 0.98 friction, blue ice 0.989, soul sand 0.4 speed, honey both — so they
//! are data or they are wrong.
//!
//! # Indexed by state id, not by name
//!
//! The lookup happens once per block per entity per tick, inside
//! `Entity.move`, and [`crate::collide::CollisionBlocks`] already answers a
//! position with a **global state id**. Going via a state string would
//! allocate on every one of those, which is the same argument
//! `CollisionBlocks` makes for not using `crate::edits::Blocks::state_at`.
//! `ci/movement-props-golden.sh` therefore emits the block rows once and a
//! flat `stateRows` array from `Block.BLOCK_STATE_REGISTRY.getId(state)` to a
//! row index.
//!
//! **There is no `-1` in that array and a reader must not invent one.** Unlike
//! a collision shape, every block state has all three floats; a missing row
//! could only ever be a dumper fault, and the generator fails rather than
//! emitting it.

use std::collections::BTreeMap;
use std::io;

/// Where the extract lives, relative to the server's working directory.
///
/// **Load-bearing outside Rust**: `ci/gen-gameplay-tables.sh` derives the set
/// of tables a tree needs by grepping `crates/rustmc-server/src` for
/// `"data/vanilla/<name>.json"`.
pub const TABLE_PATH: &str = "data/vanilla/movement_props.json";

/// One block's movement properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// `Block.getFriction()`. Default 0.6.
    pub friction: f32,
    /// `Block.getSpeedFactor()`. Default 1.0.
    pub speed_factor: f32,
    /// `Block.getBounceRestitution()`. Default 0.0.
    pub bounce_restitution: f32,
    /// `BlockTags.FENCES`.
    ///
    /// **Separate from [`Row::wall_or_gate`], and the separation is the whole
    /// point of this field.** `Entity.getOnPos(float offset)` guards the fence
    /// arm with `offset <= 0.5` and tests the other two unconditionally:
    ///
    /// ```text
    ///    38: offset > 0.5   -> skip the fence test
    ///    47: is(FENCES)     -> return the support
    ///    57: is(WALLS)      -> return the support
    ///    67: FenceGateBlock -> return the support
    ///    79: return support.atY(floor(y - offset))
    /// ```
    ///
    /// The two callers in this server pass **0.2** (`getOnPosLegacy`) and
    /// **0.500001** (`getBlockPosBelowThatAffectsMyMovement`) — one either
    /// side of that comparison. A single `tall_support` boolean makes a fence
    /// behave the same at both, which is wrong at both, and that is exactly
    /// what this field looked like until a review pass against the bytecode
    /// caught it.
    pub fence: bool,
    /// `BlockTags.WALLS` or `instanceof FenceGateBlock` — the two arms
    /// `getOnPos` tests without consulting the offset.
    pub wall_or_gate: bool,
    /// `Blocks.WATER` or `Blocks.BUBBLE_COLUMN` —
    /// `Entity.getBlockSpeedFactor`'s short circuit.
    pub waterlike: bool,
}

/// The table: distinct block rows, and one row index per global state id.
#[derive(Debug, Clone)]
pub struct MovementProps {
    rows: Vec<Row>,
    state_rows: Vec<u32>,
    /// Kept for the loader's log line and for tests that name a block.
    by_name: BTreeMap<String, usize>,
}

fn bad(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("movement_props.json: {what}"),
    )
}

fn f32_bits(v: Option<&serde_json::Value>) -> Option<f32> {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    v.and_then(serde_json::Value::as_i64)
        .map(|b| f32::from_bits(b as u32))
}

impl MovementProps {
    /// Load from [`TABLE_PATH`], resolved relative to the process's cwd and,
    /// failing that, relative to `CARGO_MANIFEST_DIR` — which is what makes
    /// this reachable under `cargo test` (whose cwd is the package
    /// directory, not the repo root). Same shape and the same carried risk
    /// as `crate::protocol_ids::shared()`: `docs/PARITY-ROADMAP.md` §34.
    ///
    /// # Errors
    /// If the file is missing or malformed.
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

    /// Load from an explicit path.
    ///
    /// # Errors
    /// If the file is missing or malformed.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| bad(&format!("not json: {e}")))?;
        Self::from_json(&doc)
    }

    /// Parse the dumper's output.
    ///
    /// # Errors
    /// If a row is malformed, or if any state maps to no row — which the
    /// generator refuses to emit, so seeing one here means the file was
    /// edited or truncated.
    pub fn from_json(doc: &serde_json::Value) -> io::Result<Self> {
        let mut rows = Vec::new();
        let mut by_name = BTreeMap::new();
        for r in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `table` array"))?
        {
            let name = r
                .get("block")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("a row with no `block`"))?;
            by_name.insert(name.to_string(), rows.len());
            rows.push(Row {
                friction: f32_bits(r.get("frictionBits"))
                    .ok_or_else(|| bad(&format!("{name} has no `frictionBits`")))?,
                speed_factor: f32_bits(r.get("speedFactorBits"))
                    .ok_or_else(|| bad(&format!("{name} has no `speedFactorBits`")))?,
                bounce_restitution: f32_bits(r.get("bounceRestitutionBits"))
                    .ok_or_else(|| bad(&format!("{name} has no `bounceRestitutionBits`")))?,
                fence: r
                    .get("fence")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| bad(&format!("{name} has no `fence`")))?,
                wall_or_gate: r
                    .get("wallOrGate")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| bad(&format!("{name} has no `wallOrGate`")))?,
                waterlike: r
                    .get("waterlike")
                    .and_then(serde_json::Value::as_bool)
                    .ok_or_else(|| bad(&format!("{name} has no `waterlike`")))?,
            });
        }

        let raw = doc
            .get("stateRows")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("no `stateRows` array"))?;
        let mut state_rows = Vec::with_capacity(raw.len());
        for (id, v) in raw.iter().enumerate() {
            let i = v
                .as_i64()
                .ok_or_else(|| bad(&format!("state {id} has a non-integer row")))?;
            // The refusal the module docs promise. A `-1` reaching this reader
            // would otherwise become a silent default, and a default here is a
            // frictionless block.
            let i = usize::try_from(i)
                .map_err(|_| bad(&format!("state {id} maps to row {i} — the dump is broken")))?;
            if i >= rows.len() {
                return Err(bad(&format!(
                    "state {id} maps to row {i} of {}",
                    rows.len()
                )));
            }
            #[allow(clippy::cast_possible_truncation)]
            state_rows.push(i as u32);
        }

        if rows.is_empty() || state_rows.is_empty() {
            return Err(bad("empty — every read would fall back to a default"));
        }
        Ok(Self {
            rows,
            state_rows,
            by_name,
        })
    }

    /// The row for a global block-state id.
    ///
    /// `None` only for an id outside the table, which is an id from a
    /// different version's registry rather than a block with no properties.
    #[must_use]
    pub fn row(&self, state_id: u32) -> Option<&Row> {
        let i = *self.state_rows.get(state_id as usize)?;
        self.rows.get(i as usize)
    }

    /// `Block.getFriction()` at a state id.
    ///
    /// **Falls back to `0.6` for an unknown id**, and that is the one default
    /// in this file. It is here rather than at the call site because the
    /// alternative — `Option<f32>` on a value multiplied into a velocity every
    /// tick — pushes the same decision into three callers. An unknown id
    /// cannot arise from a world this server generated; it can arise from a
    /// saved region written by a different version, and treating that as
    /// ordinary ground is the least surprising answer.
    #[must_use]
    pub fn friction(&self, state_id: u32) -> f32 {
        self.row(state_id).map_or(0.6, |r| r.friction)
    }

    /// `Block.getSpeedFactor()` at a state id.
    #[must_use]
    pub fn speed_factor(&self, state_id: u32) -> f32 {
        self.row(state_id).map_or(1.0, |r| r.speed_factor)
    }

    /// `Entity.getBlockBounciness(block)`:
    /// `block.getBounceRestitution() * (this instanceof LivingEntity ? 1 : 0.8)`.
    ///
    /// **The 0.8 applies to everything that is not a `LivingEntity`**, which
    /// is both kinds this server simulates, so an item landing on a slime
    /// block bounces at 0.8 of the block's restitution and a player at all of
    /// it. Folding the multiply into the table would make it wrong for one of
    /// the two.
    #[must_use]
    pub fn bounce_restitution_for_entity(&self, state_id: u32, living: bool) -> f64 {
        let r = f64::from(self.row(state_id).map_or(0.0, |r| r.bounce_restitution));
        if living {
            r
        } else {
            r * f64::from(0.8_f32)
        }
    }

    /// `BlockTags.FENCES` — see [`Row::fence`] for why this is not merged
    /// with [`MovementProps::is_wall_or_gate`].
    #[must_use]
    pub fn is_fence(&self, state_id: u32) -> bool {
        self.row(state_id).is_some_and(|r| r.fence)
    }

    /// `BlockTags.WALLS` or `instanceof FenceGateBlock`.
    #[must_use]
    pub fn is_wall_or_gate(&self, state_id: u32) -> bool {
        self.row(state_id).is_some_and(|r| r.wall_or_gate)
    }

    /// `Blocks.WATER` / `Blocks.BUBBLE_COLUMN` — see [`Row::waterlike`].
    #[must_use]
    pub fn is_water_or_bubble_column(&self, state_id: u32) -> bool {
        self.row(state_id).is_some_and(|r| r.waterlike)
    }

    /// The row for a block name, for tests and for the loader's log line.
    #[must_use]
    pub fn by_block(&self, block: &str) -> Option<&Row> {
        self.rows.get(*self.by_name.get(block)?)
    }

    /// Distinct blocks and states, for a log line that can tell an empty parse
    /// from a full one.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        (self.rows.len(), self.state_rows.len())
    }

    /// The process-wide table, loaded once. `None` when the extract is absent.
    pub fn shared() -> Option<&'static Self> {
        static TABLE: std::sync::OnceLock<Option<MovementProps>> = std::sync::OnceLock::new();
        TABLE.get_or_init(|| Self::load_default().ok()).as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> MovementProps {
        let row = |name: &str, fr: f32, sp: f32, bo: f32, fence: bool, wall: bool, water: bool| {
            serde_json::json!({
                "block": name,
                "frictionBits": fr.to_bits() as i64,
                "speedFactorBits": sp.to_bits() as i64,
                "bounceRestitutionBits": bo.to_bits() as i64,
                "fence": fence,
                "wallOrGate": wall,
                "waterlike": water,
            })
        };
        let doc = serde_json::json!({
            "table": [
                row("minecraft:stone", 0.6, 1.0, 0.0, false, false, false),
                row("minecraft:ice", 0.98, 1.0, 0.0, false, false, false),
                row("minecraft:slime_block", 0.8, 1.0, 0.8, false, false, false),
                row("minecraft:oak_fence", 0.6, 1.0, 0.0, true, false, false),
                row("minecraft:water", 0.6, 1.0, 0.0, false, false, true),
                row("minecraft:cobblestone_wall", 0.6, 1.0, 0.0, false, true, false),
            ],
            // 0 stone, 1 ice, 2 slime, 3 fence, 4 water, 5 wall
            "stateRows": [0, 1, 2, 3, 4, 5]
        });
        MovementProps::from_json(&doc).expect("the fixture parses")
    }

    #[test]
    fn a_state_id_resolves_to_its_blocks_row() {
        let p = table();
        assert_eq!(p.friction(0), 0.6);
        assert_eq!(p.friction(1), 0.98);
        assert!(p.is_fence(3));
        assert!(
            !p.is_wall_or_gate(3),
            "a fence is not a wall — getOnPos tests them separately"
        );
        assert!(p.is_wall_or_gate(5));
        assert!(!p.is_fence(5));
        assert!(!p.is_fence(0));
        assert!(p.is_water_or_bubble_column(4));
    }

    /// **The out-of-table fallbacks, every one of them.**
    ///
    /// Added because the falsification sweep found three holes here and they
    /// were the same hole: `friction`, `speed_factor` and `is_fence` all have
    /// a documented answer for a state id the table does not carry, and no
    /// test pinned any of them — so changing 0.6 to 0.98, or 1.0 to 0.0, or
    /// `is_some_and` to `is_none_or`, went green.
    ///
    /// The values are not arbitrary and that is why they are worth pinning:
    /// an unknown id is a block from a *different version's* registry, read
    /// out of a saved region, and the least surprising answer is ordinary
    /// ground — 0.6 friction, speed 1.0, not bouncy, not a fence, not water.
    /// Any other default silently changes how something moves on terrain
    /// nobody looked at.
    #[test]
    fn an_unknown_state_id_reads_as_ordinary_ground() {
        let p = table();
        let unknown = 9_999;
        assert!(p.row(unknown).is_none(), "the fixture has no such state");
        assert_eq!(p.friction(unknown), 0.6, "vanilla's default friction");
        assert_eq!(p.speed_factor(unknown), 1.0, "vanilla's default speed");
        assert_eq!(p.bounce_restitution_for_entity(unknown, false), 0.0);
        assert_eq!(p.bounce_restitution_for_entity(unknown, true), 0.0);
        assert!(!p.is_fence(unknown), "not a fence");
        assert!(!p.is_wall_or_gate(unknown), "not a wall");
        assert!(!p.is_water_or_bubble_column(unknown), "not water");
    }

    /// The 0.8 is the *entity's*, not the block's, and getting it into the
    /// table would make an item and a player bounce identically when vanilla
    /// makes them differ by exactly this factor.
    #[test]
    fn a_non_living_entity_bounces_at_four_fifths() {
        let p = table();
        assert_eq!(p.bounce_restitution_for_entity(2, true), f64::from(0.8_f32));
        assert_eq!(
            p.bounce_restitution_for_entity(2, false),
            f64::from(0.8_f32) * f64::from(0.8_f32)
        );
        // A block that does not bounce is zero for both, so the multiply
        // cannot manufacture a bounce.
        assert_eq!(p.bounce_restitution_for_entity(0, false), 0.0);
    }

    /// The bits path: `0.98f` read through raw bits is the float the bytecode
    /// holds, and `0.98` as a decimal is a different number once widened.
    #[test]
    fn friction_is_the_float_the_bytecode_holds() {
        let p = table();
        assert_eq!(p.friction(1).to_bits(), 0.98_f32.to_bits());
        assert_ne!(f64::from(p.friction(1)), 0.98_f64);
    }

    /// A `-1` row is the dumper failing, and it must be an error rather than a
    /// default — a defaulted row is a frictionless block that behaves almost
    /// correctly.
    #[test]
    fn a_negative_state_row_is_refused() {
        let doc = serde_json::json!({
            "table": [{"block": "a", "frictionBits": 0, "speedFactorBits": 0,
                       "bounceRestitutionBits": 0, "fence": false, "wallOrGate": false,
                       "waterlike": false}],
            "stateRows": [0, -1]
        });
        assert!(MovementProps::from_json(&doc).is_err());
    }

    #[test]
    fn an_out_of_range_state_row_is_refused() {
        let doc = serde_json::json!({
            "table": [{"block": "a", "frictionBits": 0, "speedFactorBits": 0,
                       "bounceRestitutionBits": 0, "fence": false, "wallOrGate": false,
                       "waterlike": false}],
            "stateRows": [0, 7]
        });
        assert!(MovementProps::from_json(&doc).is_err());
    }

    #[test]
    fn an_empty_table_is_refused() {
        let doc = serde_json::json!({"table": [], "stateRows": []});
        assert!(MovementProps::from_json(&doc).is_err());
    }
}
