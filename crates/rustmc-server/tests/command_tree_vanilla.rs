//! Vanilla's whole command tree through our encoder, byte for byte.
//!
//! # What this gates, and the thing it emphatically does not
//!
//! `ci/commands-golden.sh` gates the bytes of the tree *this server*
//! registers. That tree is small, and `crates/rustmc-server/src/commandtree.rs`
//! says in its own header exactly how small: one branching parent, two levels
//! deep, no argument nodes, no redirects, no node whose parent is usable and
//! whose own level is not. Every property of the encoder that our tree cannot
//! distinguish is ungated by it.
//!
//! This test hands the same encoder vanilla's entire dispatcher — 90 roots,
//! 2,085 nodes, 1,084 argument nodes over 55 parsers, 112 redirects, 126
//! custom-suggestion nodes — and requires it to reproduce the exact
//! `ClientboundCommandsPacket` body vanilla's own stream codec emits, at each
//! of the five permission levels. `ci/command-tree-extract.sh` produces both
//! halves and `tools/goldengen/vanilla/CommandTreeExtract.java` explains why
//! the semantic half carries no wire encoding at all.
//!
//! **It says nothing whatever about executors, and must never be quoted as
//! if it did.** `executable: true` in the extract means a `Command` object
//! exists on that node; nothing outside a lambda can see what it does. A
//! server that shipped this tree would autocomplete perfectly and do nothing —
//! `docs/EXTRACTABILITY.md` §2.1 makes exactly that point, and this file is
//! the instrument that makes the *other* half of §2.1 falsifiable: that the
//! shape and the serialisers are a table.
//!
//! # Why the strings are leaked
//!
//! [`GraphNode`] holds `&'static str` because production registrations are
//! literals and a command tree built once at boot should not allocate a name
//! per node. The extract's names arrive as owned `String`s, so this test leaks
//! them. That is bounded — one leak per node of one fixture, in a test binary
//! that then exits — and it is the alternative to making the production type
//! carry a `Cow` for the sake of a test.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rustmc_server::commandtree::{
    parser_id, Graph, GraphNode, Level, Parser, Props, StringKind, PARSER_IDS,
};
use serde_json::Value;

/// The extract, or a loud failure.
///
/// **Not a skip.** `ci/gen-gameplay-tables.sh` generates this table in every
/// tree, so its absence is a broken setup rather than a legitimate checkout
/// state, and a test that returned early here would pass while measuring
/// nothing — the defect `crates/rustmc-data/src/fixture.rs` exists to remove.
fn extract() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data/vanilla/command_tree_full.json");
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e} — run ci/command-tree-extract.sh", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// `serializeToJson`'s output for one parser, turned back into the properties
/// its `serializeToNetwork` writes.
///
/// The two are deliberately different documents. `serializeToJson` **omits a
/// bound that is the type's sentinel**, which is precisely the has-min /
/// has-max flag bit that reaches the wire, so `Option::None` here is not
/// "unknown" but "is `Integer.MIN_VALUE`". Anything that turned an absent key
/// into a zero would produce a well-formed packet with the wrong bounds and
/// the wrong length.
fn props_of(parser: &str, props: &Value) -> Props {
    let n = |k: &str| props.get(k);
    match parser {
        "brigadier:integer" => Props::Int(
            n("min").map(|v| i32::try_from(v.as_i64().expect("min")).expect("i32 min")),
            n("max").map(|v| i32::try_from(v.as_i64().expect("max")).expect("i32 max")),
        ),
        "brigadier:long" => Props::Long(
            n("min").map(|v| v.as_i64().expect("min")),
            n("max").map(|v| v.as_i64().expect("max")),
        ),
        #[allow(clippy::cast_possible_truncation)]
        "brigadier:float" => Props::Float(
            n("min").map(|v| v.as_f64().expect("min") as f32),
            n("max").map(|v| v.as_f64().expect("max") as f32),
        ),
        "brigadier:double" => Props::Double(
            n("min").map(|v| v.as_f64().expect("min")),
            n("max").map(|v| v.as_f64().expect("max")),
        ),
        "brigadier:string" => Props::Str(match props["type"].as_str().expect("type") {
            "word" => StringKind::Word,
            "phrase" => StringKind::Phrase,
            "greedy" => StringKind::Greedy,
            other => panic!("unknown brigadier:string type {other:?}"),
        }),
        "minecraft:entity" => Props::Entity {
            single: props["amount"] == "single",
            players_only: props["type"] == "players",
        },
        "minecraft:score_holder" => Props::ScoreHolder {
            multiple: props["amount"] == "multiple",
        },
        "minecraft:resource"
        | "minecraft:resource_key"
        | "minecraft:resource_or_tag"
        | "minecraft:resource_or_tag_key"
        | "minecraft:resource_selector" => {
            Props::Registry(leak(props["registry"].as_str().expect("registry")))
        }
        "minecraft:time" => {
            Props::TimeMin(i32::try_from(props["min"].as_i64().expect("min")).expect("i32 min"))
        }
        // The other 47. Their `serializeToNetwork` writes nothing, so an
        // empty `properties` is the claim — and it is asserted rather than
        // assumed, because a parser that grew a property in a later version
        // would otherwise encode short and silently.
        other => {
            assert!(
                props.as_object().is_some_and(serde_json::Map::is_empty),
                "{other} carries properties {props} and this reader writes none for it"
            );
            Props::Empty
        }
    }
}

/// Vanilla's graph, rebuilt in our types.
fn vanilla_graph(doc: &Value) -> Graph {
    let raw = doc["nodes"].as_array().expect("nodes");
    let nodes = raw
        .iter()
        .map(|n| {
            let kind = n["kind"].as_str().expect("kind");
            let parser = (kind == "argument").then(|| Parser {
                registry: leak(n["parser"].as_str().expect("parser")),
                props: props_of(
                    n["parser"].as_str().expect("parser"),
                    n.get("properties").unwrap_or(&Value::Null),
                ),
                suggestions: n.get("suggestions").map(|v| leak(v.as_str().expect("id"))),
            });
            GraphNode {
                name: n
                    .get("name")
                    .map_or("", |v| leak(v.as_str().expect("name"))),
                parser,
                is_root: kind == "root",
                level: Level::from_ordinal(
                    i32::try_from(n["level"].as_i64().expect("level")).expect("level fits"),
                ),
                executable: n["executable"].as_bool().expect("executable"),
                children: n["children"]
                    .as_array()
                    .expect("children")
                    .iter()
                    .map(|c| usize::try_from(c.as_u64().expect("child")).expect("index"))
                    .collect(),
                redirect: n
                    .get("redirect")
                    .map(|r| usize::try_from(r.as_u64().expect("redirect")).expect("index")),
            }
        })
        .collect();
    Graph {
        nodes,
        root: usize::try_from(doc["root"].as_u64().expect("root")).expect("index"),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// **The gate.** All five levels, byte for byte, against vanilla's own codec.
#[test]
fn every_level_of_vanillas_whole_tree_matches_vanillas_own_encoder() {
    let doc = extract();
    let g = vanilla_graph(&doc);
    let levels = doc["levels"].as_array().expect("levels");
    assert_eq!(levels.len(), 5, "five permission levels");

    for (row, level) in levels.iter().zip(Level::ALL) {
        let want = row["packetHex"].as_str().expect("packetHex");
        let got = g.encode(level).expect("encode");
        assert_eq!(
            hex(&got),
            want,
            "level {level:?} ({}): {} bytes ours against {} vanilla's",
            row["level"],
            got.len(),
            want.len() / 2,
        );
    }
}

/// The extract must be large enough to be the tree it claims to be.
///
/// A dispatcher built without `FeatureFlags.VANILLA_SET` still produces a
/// well-formed dump that our encoder would still reproduce — the pair would
/// agree about a fraction of the surface and the test above would be green.
/// The dumper's own python guard checks this too; it is repeated here because
/// a check that lives only in the generator is absent from every tree that
/// received the table by copy.
#[test]
fn the_extract_is_the_whole_dispatcher_and_not_a_fragment() {
    let doc = extract();
    let c = &doc["counts"];
    assert!(c["roots"].as_u64().expect("roots") >= 88, "roots: {c}");
    assert!(c["nodes"].as_u64().expect("nodes") >= 2000, "nodes: {c}");
    assert!(
        c["arguments"].as_u64().expect("arguments") >= 1000,
        "argument nodes: {c}"
    );
    assert!(
        c["redirects"].as_u64().expect("redirects") >= 100,
        "redirects: {c}"
    );
    assert!(
        c["custom_suggestions"].as_u64().expect("suggestions") >= 100,
        "custom-suggestion nodes: {c}"
    );

    // And every one of those four features must actually appear in `nodes`,
    // not merely in a count the dumper wrote. A count is a claim about the
    // input; these are the objects the encoder is handed.
    let raw = doc["nodes"].as_array().expect("nodes");
    assert!(raw.iter().any(|n| n.get("redirect").is_some()));
    assert!(raw.iter().any(|n| n.get("suggestions").is_some()));
    assert!(raw.iter().any(|n| n["kind"] == "argument"));
    assert!(raw.iter().any(|n| n["kind"] == "literal"));
}

/// The array order is not the wire order, and the wire is *shorter*.
///
/// `CommandTreeExtract` dumps depth first; `enumerateNodes` numbers breadth
/// first and merges structurally equal subtrees. Both properties are asserted
/// because both are named in that dumper's header as things the Rust side
/// checks, and because either one silently ceasing to hold would leave the
/// byte test green while it stopped measuring the thing it was written for:
/// a later dumper that walked breadth first would make the numbering
/// untested, and a Brigadier whose `CommandNode.equals` became identity would
/// make the merge untested.
///
/// The merge is 68 nodes at `OWNERS` — 2,085 in the dispatcher, 2,017 in the
/// packet — and every one of them is in `/execute store`. See
/// `Graph::class_of` for why.
#[test]
fn node_order_is_not_wire_order() {
    let doc = extract();
    let g = vanilla_graph(&doc);
    // Owners sees the whole tree, so the wire order here is the numbering of
    // everything rather than of a filtered subset.
    let wire: Vec<&str> = g
        .wire_order(Level::Owners)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    let dumped: Vec<&str> = doc["nodes"]
        .as_array()
        .expect("nodes")
        .iter()
        .map(|n| n.get("name").and_then(Value::as_str).unwrap_or(""))
        .collect();
    assert_eq!(dumped.len(), 2085, "the dispatcher's node count");
    assert_eq!(wire.len(), 2017, "the packet's entry count at OWNERS");
    assert_ne!(
        wire[..wire.len().min(dumped.len())],
        dumped[..wire.len().min(dumped.len())],
        "the dump's depth-first order and the packet's breadth-first numbering \
         must differ, or this test has stopped separating them"
    );
}

/// The merge is not an artefact of the filter: it happens at the level that
/// filters nothing.
///
/// `OWNERS` may use every node in the dispatcher, so the 68-node difference
/// there is entirely `enumerateNodes`' structural deduplication. Asserted
/// separately from the byte test because the byte test would also be green on
/// an encoder that dropped 68 nodes for the wrong reason.
#[test]
fn the_merge_is_structural_and_not_the_permission_filter() {
    let doc = extract();
    let g = vanilla_graph(&doc);
    let reachable = doc["nodes"].as_array().expect("nodes").len();
    let emitted = g.wire_order(Level::Owners).len();
    assert_eq!(
        reachable - emitted,
        68,
        "68 structurally equal nodes merge at OWNERS, where nothing is filtered"
    );
    // And the surviving `/execute store` shape is pointed at from more than
    // one place, which is the observable consequence: some node's child index
    // is another node's child index.
    let order = g.wire_order(Level::Owners);
    let mut seen = std::collections::BTreeSet::new();
    let mut shared = 0usize;
    for (_, kids) in &order {
        for k in kids {
            if !seen.insert(*k) {
                shared += 1;
            }
        }
    }
    assert!(
        shared > 0,
        "a merged node must be referenced by more than one parent"
    );
}

/// [`PARSER_IDS`] is the registry vanilla declares — in both directions.
///
/// One direction alone is not enough and the asymmetry is the usual one: a
/// missing entry is invisible to "every id we have is right", and an extra
/// entry is invisible to "every id vanilla has is present". The ids are
/// registration order in `ArgumentTypeInfos.bootstrap` and nothing in a node
/// says what they are, so a drift here is a packet that names the wrong
/// serialiser and still decodes.
#[test]
fn the_parser_registry_is_the_one_vanilla_declares() {
    let doc = extract();
    let vanilla: BTreeMap<String, i32> = doc["parsers"]
        .as_object()
        .expect("parsers")
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                i32::try_from(v.as_i64().expect("id")).expect("id fits"),
            )
        })
        .collect();
    let ours: BTreeMap<String, i32> = PARSER_IDS
        .iter()
        .enumerate()
        .map(|(i, name)| ((*name).to_string(), i32::try_from(i).expect("id fits")))
        .collect();
    assert_eq!(ours, vanilla, "PARSER_IDS against vanilla's own registry");
    assert_eq!(parser_id("brigadier:integer"), Some(3));
    assert_eq!(parser_id("minecraft:not_a_parser"), None);
}

/// Every parser vanilla's tree actually uses is one we can encode.
///
/// The test above compares the registry; this one compares the *demand*. They
/// come apart if a parser is registered and unused, which four of the 57 are.
#[test]
fn every_parser_the_tree_uses_has_an_id_and_a_property_reader() {
    let doc = extract();
    let mut used: BTreeMap<&str, usize> = BTreeMap::new();
    for n in doc["nodes"].as_array().expect("nodes") {
        if n["kind"] == "argument" {
            let p = n["parser"].as_str().expect("parser");
            *used.entry(p).or_default() += 1;
            assert!(parser_id(p).is_some(), "{p} has no id in PARSER_IDS");
            // `props_of` panics on a parser it does not read but whose
            // properties are non-empty, so calling it here is the check.
            let _ = props_of(p, n.get("properties").unwrap_or(&Value::Null));
        }
    }
    assert_eq!(
        used.len(),
        55,
        "vanilla 26.2's tree uses 55 of its 57 registered parsers; got {used:?}"
    );
}

/// The five bodies must not be five copies of one, or the level loop above
/// would pass on a filter that filtered nothing.
///
/// Levels 0 and 1 **are** identical in 26.2 — no node in the dispatcher
/// requires `MODERATORS` — so the assertion is on the pair that must differ,
/// and the identical pair is asserted identical so that its being identical is
/// a recorded fact rather than an unnoticed one.
#[test]
fn the_five_levels_are_not_one_level_five_times() {
    let doc = extract();
    let h: Vec<&str> = doc["levels"]
        .as_array()
        .expect("levels")
        .iter()
        .map(|l| l["packetHex"].as_str().expect("hex"))
        .collect();
    assert_eq!(h[0], h[1], "26.2 registers no MODERATORS-only node");
    assert_ne!(h[1], h[2], "GAMEMASTERS opens most of the tree");
    assert_ne!(h[2], h[3], "ADMINS opens more");
    assert_ne!(h[3], h[4], "OWNERS opens more still");
}

/// `/gamerule`, ours against vanilla's, **order-insensitively** — because
/// vanilla has no stable order to compare against.
///
/// `ci/commands-golden.sh` already gates every byte of our `/gamerule`
/// subtree, but it gates it against a golden built from our own list in our
/// own order, so it cannot see a rule we invented, a rule we dropped, or a
/// bound we copied wrong. This is the half that reads vanilla: the same 58
/// rules, each registered under both names, each with the same parser and the
/// same declared bounds, and the whole subtree at the same permission level.
///
/// The order is deliberately not compared. `ci/command-tree-compare.py` has
/// the measurement: sixteen runs of the dumper against one jar produced two
/// orders, 160 of 2,085 nodes permuted and all of them here. A test that
/// compared order would fail about one run in sixteen for no defect, which is
/// worse than not testing it — it would train a reader to re-run the gate.
#[test]
fn our_gamerule_subtree_is_vanillas_rules_with_vanillas_bounds() {
    use rustmc_server::commands;

    let doc = extract();
    let nodes = doc["nodes"].as_array().expect("nodes");
    let root = usize::try_from(doc["root"].as_u64().expect("root")).expect("index");
    let gi = nodes[root]["children"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|c| usize::try_from(c.as_u64().expect("child")).expect("index"))
        .find(|&c| nodes[c]["name"] == "gamerule")
        .expect("vanilla registers /gamerule");

    assert_eq!(nodes[gi]["level"], 2, "vanilla's /gamerule is GAMEMASTERS");
    assert_eq!(
        nodes[gi]["executable"], false,
        "bare /gamerule runs nothing in vanilla"
    );

    // vanilla: literal name -> (parser, properties json)
    let mut vanilla: BTreeMap<String, (String, String)> = BTreeMap::new();
    for c in nodes[gi]["children"].as_array().expect("children") {
        let ch = &nodes[usize::try_from(c.as_u64().expect("child")).expect("index")];
        assert_eq!(ch["level"], 0, "a rule literal is level ALL");
        assert_eq!(ch["executable"], true, "a rule literal queries");
        let kids = ch["children"].as_array().expect("value");
        assert_eq!(kids.len(), 1, "one value argument per rule");
        let v = &nodes[usize::try_from(kids[0].as_u64().expect("v")).expect("index")];
        assert_eq!(v["name"], "value", "the argument is named `value`");
        assert_eq!(v["executable"], true, "the value argument sets");
        vanilla.insert(
            ch["name"].as_str().expect("name").to_owned(),
            (
                v["parser"].as_str().expect("parser").to_owned(),
                serde_json::to_string(v.get("properties").unwrap_or(&Value::Null)).expect("json"),
            ),
        );
    }

    // ours, read off the tree the server really builds
    let tree = commands::tree();
    let gr = tree
        .roots
        .iter()
        .find(|n| n.name == "gamerule")
        .expect("this server registers /gamerule");
    let mut ours: BTreeMap<String, (String, String)> = BTreeMap::new();
    for ch in &gr.children {
        let v = &ch.children[0];
        let p = v.parser.as_ref().expect("value is an argument");
        // Our `Props` back into `serializeToJson`'s shape, which is what the
        // extract carries. Going this direction rather than the other is
        // deliberate: it forces the sentinel rule to be stated on our side
        // too, so a bound we stored as `Some(i32::MIN)` would not compare
        // equal to vanilla's omitted key.
        let props = match p.props {
            Props::Empty => "{}".to_owned(),
            Props::Int(min, max) => {
                let mut m = serde_json::Map::new();
                if let Some(x) = max {
                    m.insert("max".into(), x.into());
                }
                if let Some(n) = min {
                    m.insert("min".into(), n.into());
                }
                serde_json::to_string(&Value::Object(m)).expect("json")
            }
            ref other => panic!("/gamerule carries {other:?}, which this test does not read"),
        };
        ours.insert(ch.name.to_owned(), (p.registry.to_owned(), props));
    }

    assert_eq!(ours.len(), 116, "58 rules under two names each");
    assert_eq!(
        ours, vanilla,
        "our /gamerule subtree disagrees with vanilla's on a name, a parser or a bound"
    );
}
