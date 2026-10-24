//! `minecraft:block_state` — `BlockStateArgument.parse`, which is
//! `BlockStateParser.parseForBlock(blocks, reader, allowNbt = true)`.
//!
//! # From the jar
//!
//! ```text
//!   parse()            forTesting is false for a command, so no `#tag` arm:
//!                      readBlock(); if peek '[' readProperties();
//!                      if allowNbt && peek '{' readNbt()
//!   readBlock()        i = cursor; id = Identifier.read(reader)
//!                      unknown block -> setCursor(i); ERROR_UNKNOWN_BLOCK
//!   readProperties()   skip '['; skipWhitespace
//!                      while canRead && peek != ']':
//!                        skipWhitespace; i = cursor; name = readString()
//!                        no such property -> setCursor(i); ERROR_UNKNOWN_PROPERTY
//!                        already set      -> setCursor(i); ERROR_DUPLICATE_PROPERTY
//!                        skipWhitespace; !(canRead && peek '=') -> ERROR_EXPECTED_VALUE
//!                        skip; skipWhitespace; j = cursor; value = readString()
//!                        property.getValue(value) empty -> setCursor(j); ERROR_INVALID_VALUE
//!                        skipWhitespace
//!                        canRead && peek ','  -> skip, next property
//!                        canRead && peek ']'  -> leave the loop
//!                        canRead              -> ERROR_EXPECTED_END_OF_PROPERTIES
//!                      canRead -> skip ']' ; else ERROR_EXPECTED_END_OF_PROPERTIES
//! ```
//!
//! `Property.getValue(String)` is per type: a boolean property accepts
//! exactly `true`/`false`; an integer property is `Integer.parseInt` and a
//! range test, so `+1` and `01` are the value 1; an enum property is its
//! serialized names. The type is recovered from the values the state table
//! holds, which is every value the property has.
//!
//! # What is not read
//!
//! A `{...}` block-entity tag (`readNbt`, `TagParser`) is
//! [`ArgError::Unsupported`]: the SNBT grammar is not transcribed, and a
//! state with a tag this server dropped would be a different block.
//!
//! A `{...}` tag makes the whole argument unread even on a block with no
//! block entity, where vanilla parses the tag and ignores it
//! (`stone{Foo:1b}` accepts at 13): declared partial in
//! `tests/command_parse_vanilla.rs`.
//!
//! The exceptions are raised as [`Syntax::Unmapped`] — accept/reject and the
//! cursor are vanilla's, gated row by row against `CommandParseGolden`'s
//! `minecraft:block_state` cases; the translation keys and their arguments are
//! not transcribed, so a refusal renders as silence, like every other
//! `net.minecraft.commands.arguments` exception here.

use crate::cmdreader::{Reader, Syntax};
use crate::commandtree::ArgError;
use std::collections::BTreeMap;
use std::io;

/// One block's properties: the default state's values in the state string's
/// own (alphabetical) order, and every value each property takes.
#[derive(Debug, Clone, Default)]
struct BlockDef {
    default: Vec<(String, String)>,
    values: BTreeMap<String, Vec<String>>,
}

/// Every block's [`BlockDef`], from `data/vanilla/block_props.json`.
#[derive(Debug, Default)]
pub struct BlockStates {
    blocks: BTreeMap<String, BlockDef>,
    /// Per state: `isSolidRender()` and `isCollisionShapeFullBlock()` (the
    /// table's `collisionFullFaces` all six) — the two tests `/clone` sorts
    /// its blocks by.
    solidity: BTreeMap<String, (bool, bool)>,
}

/// A parsed `minecraft:block_state`: the block id and the full state string,
/// in the form the state table and the world use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockInput {
    pub block: String,
    pub state: String,
    /// The properties the input NAMED, in the order it named them —
    /// `BlockStateParser.properties`, which `BlockInput.place` re-applies
    /// over the neighbour-shaped state (`overwriteWithDefinedProperties`).
    pub explicit: Vec<(String, String)>,
}

impl BlockInput {
    /// `BlockInput.overwriteWithDefinedProperties(state)`: `state` with every
    /// explicitly named property put back to the value the input gave it.
    #[must_use]
    pub fn overwrite_defined(&self, state: &str) -> String {
        let (name, mut props) = split_state(state);
        for (k, v) in &self.explicit {
            if let Some(slot) = props.iter_mut().find(|(pk, _)| pk == k) {
                slot.1.clone_from(v);
            }
        }
        BlockStates::state_string(name, &props)
    }
}

/// `name[a=1,b=2]` -> `(name, [(a, 1), (b, 2)])`.
fn split_state(state: &str) -> (&str, Vec<(String, String)>) {
    match state.split_once('[') {
        None => (state, Vec::new()),
        Some((name, rest)) => {
            let body = rest.strip_suffix(']').unwrap_or(rest);
            let props = body
                .split(',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            (name, props)
        }
    }
}

impl BlockStates {
    /// Load from `block_props.json`'s `table`: each entry's `block`, `state`
    /// and `default`.
    ///
    /// # Errors
    /// If the file is missing or holds no states.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path)?).map_err(io::Error::other)?;
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("block_props.json has no `table`"))?;
        let mut blocks: BTreeMap<String, BlockDef> = BTreeMap::new();
        let mut solidity = BTreeMap::new();
        for e in table {
            let Some(state) = e.get("state").and_then(serde_json::Value::as_str) else {
                continue;
            };
            solidity.insert(
                state.to_string(),
                (
                    e.get("solidRender").and_then(serde_json::Value::as_bool) == Some(true),
                    e.get("collisionFullFaces")
                        .and_then(serde_json::Value::as_str)
                        == Some("111111"),
                ),
            );
            let (name, props) = split_state(state);
            let def = blocks.entry(name.to_string()).or_default();
            for (k, v) in &props {
                let vs = def.values.entry(k.clone()).or_default();
                if !vs.contains(v) {
                    vs.push(v.clone());
                }
            }
            if e.get("default").and_then(serde_json::Value::as_bool) == Some(true) {
                def.default = props;
            }
        }
        if blocks.is_empty() {
            return Err(io::Error::other("no states in block_props.json"));
        }
        Ok(Self { blocks, solidity })
    }

    /// `state.isSolidRender() || state.isCollisionShapeFullBlock(level, pos)`
    /// — `/clone`'s "full block" test. An unknown state is not full.
    #[must_use]
    pub fn is_full_block(&self, state: &str) -> bool {
        self.solidity.get(state).is_some_and(|(r, c)| *r || *c)
    }

    /// A block's default state string.
    #[must_use]
    pub fn default_state(&self, block: &str) -> Option<String> {
        self.blocks
            .get(block)
            .map(|d| Self::state_string(block, &d.default))
    }

    /// The state string for `block` with `props` — every property, in the
    /// table's order.
    fn state_string(block: &str, props: &[(String, String)]) -> String {
        if props.is_empty() {
            return block.to_string();
        }
        let body: Vec<String> = props.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("{block}[{}]", body.join(","))
    }
}

/// The table, loaded once from the server's working directory or, for a test,
/// from the repository root.
#[must_use]
pub fn shared() -> Option<&'static BlockStates> {
    static TABLE: std::sync::OnceLock<Option<BlockStates>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            let rel = std::path::Path::new("data/vanilla/block_props.json");
            let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/vanilla/block_props.json");
            BlockStates::load(rel)
                .or_else(|_| BlockStates::load(&repo))
                .ok()
        })
        .as_ref()
}

/// `Property.getValue(String)` against the values a property takes.
fn property_value(values: &[String], text: &str) -> Option<String> {
    let is_bool = values.iter().all(|v| v == "true" || v == "false");
    if is_bool {
        return values.iter().find(|v| *v == text).cloned();
    }
    let ints: Option<Vec<i32>> = values.iter().map(|v| v.parse::<i32>().ok()).collect();
    if let Some(ints) = ints {
        // `Integer.parseInt`: an optional sign, then decimal digits, nothing
        // else — so `+1` and `01` parse and ` 1` does not.
        let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let n: i32 = text.parse().ok()?;
        return ints.contains(&n).then(|| n.to_string());
    }
    values.iter().find(|v| *v == text).cloned()
}

/// `BlockStateParser.parseForBlock(blocks, reader, true)` — see the module
/// header for the walk.
///
/// # Errors
/// [`ArgError::Syntax`] where vanilla raises (unmapped), and
/// [`ArgError::Unsupported`] for a block-entity tag.
pub fn parse(r: &mut Reader<'_>, table: &BlockStates) -> Result<BlockInput, ArgError> {
    // `parseForBlock(lookup, reader, allowNbt)` saves the cursor and puts it
    // BACK on any `CommandSyntaxException` before rethrowing: the exception
    // carries the position of the fault, the reader is left where the
    // argument began. Measured against vanilla's own rows —
    // `stone[facing=north]` leaves the reader at 0, not at the property.
    let start = r.cursor();
    let out = parse_at(r, table);
    if matches!(out, Err(ArgError::Syntax(_))) {
        r.set_cursor(start);
    }
    out
}

fn parse_at(r: &mut Reader<'_>, table: &BlockStates) -> Result<BlockInput, ArgError> {
    let fail = || ArgError::Syntax(Syntax::Unmapped);
    // `readBlock` — `Identifier.read`, then the lookup.
    let start = r.cursor();
    let run = r.read_identifier_run();
    let id = match run.find(':') {
        None => format!("minecraft:{run}"),
        Some(0) => format!("minecraft:{}", &run[1..]),
        Some(_) => run.to_string(),
    };
    if !crate::commandtree::identifier_is_valid(run) {
        r.set_cursor(start);
        return Err(fail());
    }
    let Some(def) = table.blocks.get(&id) else {
        r.set_cursor(start);
        return Err(fail());
    };
    let mut props = def.default.clone();
    let mut explicit: Vec<(String, String)> = Vec::new();
    if r.peek() == Some('[') {
        let mut seen: Vec<String> = Vec::new();
        r.skip();
        r.skip_whitespace();
        while r.can_read() && r.peek() != Some(']') {
            r.skip_whitespace();
            let i = r.cursor();
            let name = r.read_string().map_err(ArgError::Syntax)?;
            let Some(values) = def.values.get(&name) else {
                r.set_cursor(i);
                return Err(fail());
            };
            if seen.contains(&name) {
                r.set_cursor(i);
                return Err(fail());
            }
            r.skip_whitespace();
            if r.peek() != Some('=') {
                return Err(fail());
            }
            r.skip();
            r.skip_whitespace();
            let j = r.cursor();
            let text = r.read_string().map_err(ArgError::Syntax)?;
            let Some(value) = property_value(values, &text) else {
                r.set_cursor(j);
                return Err(fail());
            };
            if let Some(slot) = props.iter_mut().find(|(k, _)| *k == name) {
                slot.1.clone_from(&value);
            }
            explicit.push((name.clone(), value));
            seen.push(name);
            r.skip_whitespace();
            match r.peek() {
                Some(',') => r.skip(),
                Some(']') => break,
                Some(_) => return Err(fail()),
                None => {}
            }
        }
        if !r.can_read() {
            return Err(fail());
        }
        r.skip();
    }
    if r.peek() == Some('{') {
        return Err(ArgError::Unsupported);
    }
    Ok(BlockInput {
        state: BlockStates::state_string(&id, &props),
        block: id,
        explicit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> BlockStates {
        let mut blocks = BTreeMap::new();
        blocks.insert("minecraft:stone".to_string(), BlockDef::default());
        let mut values = BTreeMap::new();
        values.insert(
            "facing".to_string(),
            vec!["north".into(), "south".into(), "west".into(), "east".into()],
        );
        values.insert("lit".to_string(), vec!["true".into(), "false".into()]);
        values.insert(
            "power".to_string(),
            (0..16).map(|n| n.to_string()).collect(),
        );
        blocks.insert(
            "minecraft:thing".to_string(),
            BlockDef {
                default: vec![
                    ("facing".into(), "north".into()),
                    ("lit".into(), "false".into()),
                    ("power".into(), "0".into()),
                ],
                values,
            },
        );
        BlockStates {
            blocks,
            solidity: BTreeMap::new(),
        }
    }

    fn run(input: &str) -> (Result<String, bool>, usize) {
        let t = table();
        let mut r = Reader::new(input);
        let out = parse(&mut r, &t)
            .map(|b| b.state)
            .map_err(|e| e == ArgError::Unsupported);
        (out, r.cursor())
    }

    #[test]
    fn a_bare_id_is_the_default_state() {
        assert_eq!(run("stone"), (Ok("minecraft:stone".into()), 5));
        assert_eq!(
            run("thing"),
            (
                Ok("minecraft:thing[facing=north,lit=false,power=0]".into()),
                5
            )
        );
    }

    #[test]
    fn properties_override_the_default_in_table_order() {
        assert_eq!(
            run("thing[ power = +7 , facing=east]"),
            (
                Ok("minecraft:thing[facing=east,lit=false,power=7]".into()),
                32
            )
        );
    }

    #[test]
    fn every_refusal_puts_the_reader_back_where_the_argument_began() {
        for input in [
            "nope",
            "stone[facing=north]",
            "thing[lit=true,lit=true]",
            "thing[power=16]",
            "thing[lit]",
            "thing[lit=true",
        ] {
            assert_eq!(run(input), (Err(false), 0), "{input}");
        }
    }

    #[test]
    fn a_block_entity_tag_is_unread() {
        assert_eq!(run("stone{x:1}").0, Err(true));
    }
}
