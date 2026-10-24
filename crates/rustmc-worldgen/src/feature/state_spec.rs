//! Turning a datapack's block-state spec into the state vanilla writes.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `BlockState.CODEC`, built from `Block.defaultBlockState()` and
//! `StateDefinition`; `BlockStateParser.serialize`.
//!
//! # A name is not a state
//!
//! A datapack writes
//!
//! ```json
//! {"Name": "minecraft:redstone_ore"}
//! ```
//!
//! and vanilla's codec resolves it by taking the block's **default state** and
//! applying whatever `Properties` the spec listed on top. Redstone ore's
//! default state is `minecraft:redstone_ore[lit=false]`, so that is what lands
//! in the world — the property is there whether or not the spec mentioned it.
//!
//! A port that carries the bare name around writes the right *block* with the
//! wrong *state*, and every gate that compares block names agrees with it.
//! Six ore features were doing exactly that until the decoration oracle
//! started recording `BlockStateParser.serialize` instead of the block's
//! registry key (ORACLE.md fault 7).
//!
//! # Properties are sorted
//!
//! `StateDefinition` holds its properties in an `ImmutableSortedMap` keyed by
//! property name, and `serialize` walks that map — so the canonical spelling
//! is alphabetical by property name, not the order the JSON happened to list
//! them in and not the order the block class declared them.

use std::collections::BTreeMap;

use serde_json::Value;

/// A block's default state, which only a vanilla extract can supply.
///
/// A trait rather than a table so this crate keeps its no-data-dependency
/// shape: worldgen asks the question, the caller answers it. See the note in
/// `Cargo.toml`.
pub trait DefaultStates {
    /// The `BlockStateParser` spelling of `block`'s default state, or `None`
    /// if the block is unknown.
    fn default_state(&self, block: &str) -> Option<&str>;
}

/// A [`DefaultStates`] that knows nothing, for parses that cannot reach a
/// vanilla extract.
///
/// It answers `None` for everything, so [`canonical`] refuses rather than
/// guessing — a spec that names a block this cannot resolve is an error, not
/// a bare name passed through.
pub struct NoDefaults;

impl DefaultStates for NoDefaults {
    fn default_state(&self, _block: &str) -> Option<&str> {
        None
    }
}

/// Every block's default state, read from the `block_props` extract.
///
/// The extract is keyed by state and flags the one row per block that vanilla
/// hands back from `defaultBlockState()`; this keeps only that column. It is a
/// parser, not data — the file lives under `data/vanilla` and only the gates
/// hand it over.
#[derive(Clone, Debug, Default)]
pub struct DefaultStateTable {
    by_block: BTreeMap<String, String>,
}

impl DefaultStateTable {
    /// # Errors
    /// If the document has no `table`, or a block claims two default states.
    pub fn parse(json: &Value) -> Result<Self, String> {
        let mut by_block: BTreeMap<String, String> = BTreeMap::new();
        for row in json["table"]
            .as_array()
            .ok_or_else(|| "block props document has no `table`".to_string())?
        {
            if !row["default"].as_bool().unwrap_or(false) {
                continue;
            }
            let block = row["block"]
                .as_str()
                .ok_or_else(|| "a default row has no `block`".to_string())?;
            let state = row["state"]
                .as_str()
                .ok_or_else(|| format!("{block} has no `state`"))?;
            if let Some(other) = by_block.insert(block.to_string(), state.to_string()) {
                return Err(format!(
                    "{block} has two default states, {other} and {state}"
                ));
            }
        }
        if by_block.is_empty() {
            return Err(
                "no row in the block props extract is flagged default — regenerate it \
                 with ci/block-props-golden.sh"
                    .to_string(),
            );
        }
        Ok(Self { by_block })
    }

    /// A table from literal pairs, for tests that must not depend on an
    /// extract being on disk.
    ///
    /// The right-hand side is the block's real default-state spelling — copy
    /// it out of the extract rather than inventing it, or the test proves the
    /// port agrees with the test.
    #[must_use]
    pub fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        Self {
            by_block: pairs
                .iter()
                .map(|(block, state)| ((*block).to_string(), (*state).to_string()))
                .collect(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_block.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_block.is_empty()
    }
}

impl DefaultStates for DefaultStateTable {
    fn default_state(&self, block: &str) -> Option<&str> {
        self.by_block.get(block).map(String::as_str)
    }
}

/// The extract, for tests and gates. Panics rather than returning an error:
/// every caller is a test that cannot run without it.
///
/// **The paragraph above described this function for the whole of its life and
/// the code did the opposite.** A missing extract returned `None`, and all
/// twenty-three callers spelled that `else { eprintln!("skipping: ..."); return
/// }` — so the tests passed, cargo swallowed the line because a passing test's
/// stderr is never printed, and nothing anywhere went red. The doc comment was
/// the only place the intended behaviour was written down, which is the same
/// shape as an arm whose comment claimed a guard the code did not have.
///
/// It now panics, through `rustmc_data::fixture`, so the contract and the code
/// agree. `RUSTMC_FIXTURES=optional` still returns `None` for a clean clone,
/// and `ci/t1.sh` prints a different sentinel under it.
///
/// # Panics
/// If the extract is missing (in the default `required` mode) or malformed.
#[must_use]
pub fn extract_or_skip() -> Option<DefaultStateTable> {
    let path = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../data/vanilla/block_props.json"
    ));
    if !rustmc_data::fixture::available(path, "block-props extract", "ci/block-props-golden.sh") {
        return None;
    }
    let json: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("json");
    Some(DefaultStateTable::parse(&json).expect("block props extract"))
}

/// `{"Name": "...", "Properties": {...}}` to the state vanilla writes.
///
/// # Errors
/// If `Name` is missing, if the block is not in `states`, or if `Properties`
/// is malformed.
pub fn canonical(spec: &Value, states: &dyn DefaultStates) -> Result<String, String> {
    let name = spec
        .get("Name")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("block state spec has no `Name`: {spec}"))?;
    let base = states.default_state(name).ok_or_else(|| {
        format!("{name} is not in the block-state extract — its default state is unknown")
    })?;
    let mut properties = split(base);

    if let Some(given) = spec.get("Properties") {
        let given = given
            .as_object()
            .ok_or_else(|| format!("{name}: Properties is not an object"))?;
        for (key, value) in given {
            let rendered = match value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            // Vanilla's codec looks the property up on the block's own
            // `StateDefinition` and fails the decode if it is absent, so a
            // spec naming a property the block does not have is a broken
            // datapack rather than something to carry along.
            if !properties.contains_key(key.as_str()) {
                return Err(format!(
                    "{name} has no property `{key}` — its states are spelled {base}"
                ));
            }
            properties.insert(key.clone(), rendered);
        }
    }

    Ok(join(name, &properties))
}

/// `BlockStateParser.parseForBlock` — a **state string** resolved against the
/// block's default state.
///
/// [`canonical`] does this for a `{Name, Properties}` NBT spec; this does it
/// for the `minecraft:name[k=v,...]` spelling that a jigsaw block's
/// `final_state` is written in.
///
/// The distinction is the whole point and it was worth 19 blocks. Vanilla
/// starts from `defaultBlockState()` and applies only the properties the
/// string names, so `minecraft:grass_block` becomes
/// `minecraft:grass_block[snowy=false]` and
/// `minecraft:acacia_stairs[facing=north]` acquires `half`, `shape` and
/// `waterlogged` at their defaults. Writing the string through verbatim —
/// which is what `place.rs` did while its own comment claimed otherwise —
/// leaves a state that is missing properties the block really has, and a
/// diff reads that as "the same block in a different state".
///
/// # Errors
/// If the block has no default state in `states`, or if the string names a
/// property the block does not have — vanilla's parser fails the same way
/// rather than carrying an unknown property along.
pub fn parse_for_block(state: &str, states: &dyn DefaultStates) -> Result<String, String> {
    let name = state.split_once('[').map_or(state, |(n, _)| n);
    let base = states.default_state(name).ok_or_else(|| {
        format!("{name} is not in the block-state extract — its default state is unknown")
    })?;
    let mut properties = split(base);
    for (key, value) in split(state) {
        if !properties.contains_key(&key) {
            return Err(format!(
                "{name} has no property `{key}` — its states are spelled {base}"
            ));
        }
        properties.insert(key, value);
    }
    Ok(join(name, &properties))
}

/// `name[a=1,b=2]` to its property map. A bare name has none.
fn split(state: &str) -> BTreeMap<String, String> {
    let Some((_, rest)) = state.split_once('[') else {
        return BTreeMap::new();
    };
    rest.trim_end_matches(']')
        .split(',')
        .filter_map(|pair| {
            pair.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

/// A name and its properties, spelled the way `BlockStateParser` spells them.
fn join(name: &str, properties: &BTreeMap<String, String>) -> String {
    if properties.is_empty() {
        return name.to_string();
    }
    let rendered = properties
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("{name}[{rendered}]")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `parseForBlock` starts from the default state, so a bare block name
    /// gains every property the block has and a partial one gains the rest.
    ///
    /// This is the 19-block defect in `place.rs`'s jigsaw replacement: the
    /// `final_state` string was written through verbatim while the comment
    /// above it claimed this function.
    #[test]
    fn parse_for_block_fills_in_the_default_properties() {
        let states = Extract(&[
            ("minecraft:grass_block", "minecraft:grass_block[snowy=false]"),
            (
                "minecraft:acacia_stairs",
                "minecraft:acacia_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]",
            ),
            ("minecraft:air", "minecraft:air"),
        ]);

        // A bare name is not a state: it acquires `snowy`.
        assert_eq!(
            parse_for_block("minecraft:grass_block", &states).expect("resolves"),
            "minecraft:grass_block[snowy=false]"
        );
        // A named property wins over the default; the rest are filled in.
        assert_eq!(
            parse_for_block("minecraft:acacia_stairs[facing=east]", &states).expect("resolves"),
            "minecraft:acacia_stairs[facing=east,half=bottom,shape=straight,waterlogged=false]"
        );
        // A propertyless block stays propertyless.
        assert_eq!(
            parse_for_block("minecraft:air", &states).expect("resolves"),
            "minecraft:air"
        );
        // A property the block does not have is refused rather than carried,
        // which is what vanilla's parser does.
        assert!(parse_for_block("minecraft:grass_block[facing=east]", &states).is_err());
        // An unknown block is refused rather than defaulted.
        assert!(parse_for_block("minecraft:nonsense", &states).is_err());
    }

    struct Extract(&'static [(&'static str, &'static str)]);

    impl DefaultStates for Extract {
        fn default_state(&self, block: &str) -> Option<&str> {
            self.0
                .iter()
                .find(|(name, _)| *name == block)
                .map(|(_, state)| *state)
        }
    }

    fn extract() -> Extract {
        Extract(&[
            (
                "minecraft:redstone_ore",
                "minecraft:redstone_ore[lit=false]",
            ),
            ("minecraft:sand", "minecraft:sand"),
            (
                "minecraft:cave_vines",
                "minecraft:cave_vines[age=0,berries=false]",
            ),
        ])
    }

    /// The whole reason this module exists.
    #[test]
    fn a_bare_name_gains_the_blocks_default_properties() {
        let spec = serde_json::json!({"Name": "minecraft:redstone_ore"});
        assert_eq!(
            canonical(&spec, &extract()).expect("ok"),
            "minecraft:redstone_ore[lit=false]"
        );
    }

    #[test]
    fn a_block_with_no_properties_stays_bare() {
        let spec = serde_json::json!({"Name": "minecraft:sand"});
        assert_eq!(canonical(&spec, &extract()).expect("ok"), "minecraft:sand");
    }

    /// Given properties override the default, and the ones not mentioned
    /// survive.
    #[test]
    fn given_properties_land_on_top_of_the_default() {
        let spec = serde_json::json!({
            "Name": "minecraft:cave_vines",
            "Properties": {"berries": "true"}
        });
        assert_eq!(
            canonical(&spec, &extract()).expect("ok"),
            "minecraft:cave_vines[age=0,berries=true]"
        );
    }

    #[test]
    fn properties_come_out_sorted_whatever_order_they_went_in() {
        let spec: Value = serde_json::from_str(
            r#"{"Name":"minecraft:cave_vines","Properties":{"berries":"true","age":"25"}}"#,
        )
        .expect("json");
        assert_eq!(
            canonical(&spec, &extract()).expect("ok"),
            "minecraft:cave_vines[age=25,berries=true]"
        );
    }

    /// Refusing beats guessing: an unknown block has no default state, and
    /// passing the bare name through is how the wrong state gets written.
    #[test]
    fn an_unknown_block_is_an_error_not_a_bare_name() {
        let spec = serde_json::json!({"Name": "minecraft:not_a_block"});
        assert!(canonical(&spec, &extract()).is_err());
        assert!(canonical(&spec, &NoDefaults).is_err());
    }

    #[test]
    fn a_property_the_block_does_not_have_is_an_error() {
        let spec = serde_json::json!({
            "Name": "minecraft:sand",
            "Properties": {"lit": "true"}
        });
        assert!(canonical(&spec, &extract()).is_err());
    }
}
