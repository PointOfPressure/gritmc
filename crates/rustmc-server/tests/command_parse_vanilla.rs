//! `Parser::parse` against vanilla's own `ArgumentType.parse`, on 770 rows.
//!
//! `ci/command-parse-golden.sh` runs Brigadier's six primitives and
//! `brigadier:string`'s three kinds over a fixed input corpus inside a real
//! JVM and records, for each, whether vanilla accepted it and **where its
//! `StringReader` cursor ended up**. This test replays every row through
//! `crates/rustmc-server/src/commandtree.rs`'s parser and demands both fields
//! agree.
//!
//! # The cursor is the assertion, and the verdict is nearly free
//!
//! Agreeing about accept/reject is the easy half and it is not the half that
//! breaks a command. A parser that consumes one character too many leaves the
//! *next* node reading the wrong input, so `/setblock 1 2 3 stone` comes back
//! "unknown command" for a command that is present, correctly spelled, and
//! whose executor works. One character too few is a
//! `dispatcherExpectedArgumentSeparator` and reads identically. Neither is
//! visible to a test that asks only "did this parse".
//!
//! # UTF-16 against UTF-8, which is a real conversion and not a formality
//!
//! `StringReader` counts UTF-16 code units; [`rustmc_server::cmdreader::Reader`]
//! counts UTF-8 bytes, for the reason its own header gives. Those agree on
//! ASCII and disagree the moment a consumed span contains anything else — the
//! corpus's `café au lait` is 12 code units and 13 bytes, so a greedy string
//! over it ends at 12 in the golden and 13 here. The conversion is done
//! explicitly below rather than by comparing ASCII-only rows, because a test
//! that avoided the disagreement would be a test that never checked the claim
//! the reader's header makes.
//!
//! # A row is not a claim that the port implements the parser
//!
//! The golden covers 15 parser names, and `Parser::parse` transcribes fewer
//! than that. Every row therefore lands in one of three buckets:
//!
//! * **agreed** — we parsed it and got vanilla's verdict and vanilla's cursor;
//! * **disagreed** — we parsed it and got something else. Always zero;
//! * **unread** — `ArgError::Unsupported`: this server cannot read this
//!   parser at all.
//!
//! Only the middle bucket is a failure. The third is counted, printed and
//! **ratcheted against [`UNREAD_CEILING`]**, which is what turns this file
//! from a gate into a coverage measurement as well: a parser transcribed is a
//! number that falls, and a parser quietly un-transcribed is a red test. The
//! ceiling is a named constant rather than a data file so that widening it is
//! a reviewed change, which is the same shape `ci/corpus-falsifiable.py` and
//! `ci/liveness-proofs.py` use for their own allowances.

use rustmc_server::cmdreader::Reader;
use rustmc_server::commandtree::{ArgError, Parser, Props, StringKind, PARSER_IDS};
use serde_json::Value;
use std::path::PathBuf;

/// The golden, or a loud failure — never a skip. See
/// `crates/rustmc-data/src/fixture.rs` for why a returning-early test is worse
/// than a missing one.
fn golden() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data/vanilla/command_parse.json");
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e} — run ci/command-parse-golden.sh", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Rebuild the [`Props`] the dumper's `props` object describes.
///
/// An absent bound is `None`, which in [`Props`] means "is the type's
/// sentinel" — the same convention `serializeToJson` uses and the same one
/// `command_tree_vanilla` rebuilds from. It is not "unrecorded".
fn props_of(parser: &str, props: &Value) -> Props {
    let num = |k: &str| props.get(k).and_then(Value::as_f64);
    let int = |k: &str| props.get(k).and_then(Value::as_i64);
    let str_of = |k: &str| props.get(k).and_then(Value::as_str);
    match parser {
        "minecraft:entity" => Props::Entity {
            single: str_of("amount") == Some("single"),
            players_only: str_of("type") == Some("players"),
        },
        "minecraft:time" => {
            Props::TimeMin(i32::try_from(int("min").expect("time min")).expect("i32 time min"))
        }
        // The parsers whose `serializeToNetwork` writes nothing. They are
        // `Props::Empty` on the wire and here.
        "minecraft:message"
        | "minecraft:block_pos"
        | "minecraft:vec3"
        | "minecraft:vec2"
        | "minecraft:objective"
        | "minecraft:objective_criteria"
        | "minecraft:scoreboard_slot"
        | "minecraft:gamemode"
        | "minecraft:resource_location"
        | "minecraft:block_state"
        | "minecraft:item_stack"
        | "minecraft:rotation"
        | "minecraft:int_range"
        | "minecraft:particle"
        | "minecraft:component" => Props::Empty,
        "brigadier:bool" => Props::Empty,
        "brigadier:integer" => Props::Int(
            int("min").map(|v| i32::try_from(v).expect("i32 min")),
            int("max").map(|v| i32::try_from(v).expect("i32 max")),
        ),
        "brigadier:long" => Props::Long(int("min"), int("max")),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the golden writes a float bound through gson's double; \
                      narrowing it back is what the declared type is"
        )]
        "brigadier:float" => {
            Props::Float(num("min").map(|v| v as f32), num("max").map(|v| v as f32))
        }
        "brigadier:double" => Props::Double(num("min"), num("max")),
        "brigadier:string" => Props::Str(match props.get("type").and_then(Value::as_str) {
            Some("word") => StringKind::Word,
            Some("phrase") => StringKind::Phrase,
            Some("greedy") => StringKind::Greedy,
            other => panic!("unknown string kind {other:?}"),
        }),
        other => panic!("the golden carries {other}, which this test cannot rebuild"),
    }
}

/// A byte cursor as the number of UTF-16 code units before it — Java's index.
///
/// # Panics
/// If the cursor is not a char boundary, which would mean the reader split a
/// character and is the thing its own header promises cannot happen.
fn utf16_index(input: &str, byte_cursor: usize) -> usize {
    assert!(
        input.is_char_boundary(byte_cursor),
        "cursor {byte_cursor} is inside a character of {input:?}"
    );
    input[..byte_cursor].encode_utf16().count()
}

/// One row's outcome in the golden's own terms.
fn ours(parser: &str, props: &Props, input: &str) -> Ours {
    let p = Parser {
        // `Parser::registry` is `&'static str` because a node's parser name is
        // a compile-time fact everywhere else in the server. The golden's names
        // are all in `PARSER_IDS`, which is that same set of literals, so this
        // borrows from the constant rather than leaking a string per row.
        registry: PARSER_IDS
            .iter()
            .find(|p| **p == parser)
            .copied()
            .unwrap_or_else(|| panic!("{parser} is not in the 26.2 parser registry")),
        props: props.clone(),
        suggestions: None,
    };
    let mut r = Reader::new(input);
    match p.parse(&mut r) {
        Ok(()) => Ours::Read(true, utf16_index(input, r.cursor())),
        Err(ArgError::Syntax(_)) => Ours::Read(false, utf16_index(input, r.cursor())),
        Err(ArgError::Unsupported) => Ours::Unread,
    }
}

/// What this server made of one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ours {
    /// Parsed, with vanilla's two fields: accepted, and the cursor as a UTF-16
    /// index.
    Read(bool, usize),
    /// `ArgError::Unsupported` — this server cannot read this parser.
    Unread,
}

/// How many of the golden's rows this server may leave unread.
///
/// **A ceiling, not a target, and it may not rise.** It is the count of rows
/// whose parser `Parser::parse` has not transcribed, and it is here rather than
/// in a data file so that raising it is a reviewed change to source rather than
/// an edit to a number. Transcribing a parser lowers it; a parser silently
/// dropped raises it and turns this test red.
///
/// 236 rows, over the parser names the golden covers and this server does not
/// fully read. **Raised 227 -> 236 on 2026-10-01, `commands-exec`, by review:**
/// `minecraft:particle` joined the golden with five particle-shaped inputs
/// (`flame`, `minecraft:flame`, `flame{}`, `dust`, `minecraft:nope`), and the
/// nine new unread rows are exactly two `minecraft:particle` (the `{}` tag and
/// `dust`, a type with options), two `minecraft:objective_criteria` (the two
/// namespaced inputs, a stat form) and five `minecraft:item_stack` (unread
/// whole) — measured by listing every unread row, 0 read rows disagreeing.
/// **Raised 151 -> 227 on 2026-10-01, `commands-exec`, by review,
/// and the rise is coverage, not loss:** `minecraft:component` joined the
/// golden that day with 120 rows, and the 76 this server leaves unread are
/// the inputs outside `crate::componentarg`'s text subset (numbers, booleans,
/// selectors, lists, the half-made strings) — 0 of its read rows disagree.
/// The same day took it 264 -> 151 by transcribing `minecraft:block_state`
/// (`crate::blockarg`), whose one unread row is a `{...}` tag. The arithmetic behind it is in
/// [`the_unread_rows_are_exactly_the_untranscribed_parsers`], which asserts
/// which parsers the residue belongs to as well as how many rows it is — so a
/// ceiling that happened to stay the same while the coverage moved sideways is
/// still a red.
const UNREAD_CEILING: usize = 236;

/// Every row: the verdict and the cursor, bucketed three ways.
#[test]
fn every_golden_row_parses_the_way_vanilla_parsed_it() {
    let doc = golden();
    let cases = doc["cases"].as_array().expect("cases");
    // The document's own header against the array it shipped, twice over. A
    // dumper that died mid-write leaves a short array beside a header that
    // still carries its full count, and every other assertion in this file
    // would pass over the rows that survived. `case_count` is a separate key
    // from `cases` deliberately: writing the count under the array's own name
    // is a gson no-op, which is how this check had nothing to read for its
    // first hour.
    let count = doc["case_count"].as_i64().expect("case_count");
    let specs = doc["specs"].as_i64().expect("specs");
    let inputs = doc["inputs"].as_i64().expect("inputs");
    assert_eq!(
        i64::try_from(cases.len()).expect("case count"),
        count,
        "the header says {count} cases and the array holds {}",
        cases.len()
    );
    assert_eq!(
        count,
        specs * inputs,
        "{specs} specs x {inputs} inputs != {count}"
    );

    let mut agreed = 0usize;
    let mut unread = 0usize;
    let mut unchecked = 0usize;
    let mut disagreements = Vec::new();
    for c in cases {
        let parser = c["parser"].as_str().expect("parser");
        let input = c["input"].as_str().expect("input");
        let props = props_of(parser, &c["props"]);
        let want_accepted = c["accepted"].as_bool().expect("accepted");
        let want_cursor = usize::try_from(c["cursor"].as_i64().expect("cursor")).expect("cursor");
        // **A ROW WHERE VANILLA THREW AN UNCHECKED EXCEPTION IS NOT A ROW
        // WHERE VANILLA REJECTED.** `ObjectiveCriteria.byName` hands any name
        // holding a `:` to `Identifier.bySeparator`, whose `assertValidPath`
        // raises `IdentifierException` — a `RuntimeException` that escapes
        // `ObjectiveCriteriaArgument.parse` entirely, so on a real server the
        // command produces an internal error rather than a syntax error. The
        // dumper marks those rows; this server must answer `Unsupported` on
        // every one, because claiming a verdict where vanilla gave none is a
        // stronger error than having no verdict.
        if let Some(kind) = c["unchecked"].as_str() {
            unchecked += 1;
            assert_eq!(
                ours(parser, &props, input),
                Ours::Unread,
                "{parser} on {input:?}: vanilla threw {kind}, so this server must \
                 not answer with a verdict"
            );
            continue;
        }
        match ours(parser, &props, input) {
            Ours::Unread => unread += 1,
            Ours::Read(got_accepted, got_cursor) => {
                if (got_accepted, got_cursor) == (want_accepted, want_cursor) {
                    agreed += 1;
                } else {
                    disagreements.push(format!(
                        "{parser} {:?} on {input:?}: vanilla {want_accepted}@{want_cursor}, ours {got_accepted}@{got_cursor}",
                        c["props"]
                    ));
                }
            }
        }
    }
    println!(
        "command-parse: {agreed} agreed, {} disagreed, {unread} unread \
         (ceiling {UNREAD_CEILING}), {unchecked} vanilla-threw of {} rows",
        disagreements.len(),
        cases.len()
    );
    assert!(
        unchecked > 0,
        "no row records an unchecked vanilla throw — either the corpus lost \
         `stone{{Foo:1b}}` or the dumper stopped marking them, and either way \
         the assertion above is measuring nothing"
    );
    assert!(
        disagreements.is_empty(),
        "{} of {} read rows disagree:\n  {}",
        disagreements.len(),
        agreed + disagreements.len(),
        disagreements.join("\n  ")
    );
    // THE RATCHET. A parser this server stops reading shows up here and
    // nowhere else: it would otherwise move rows out of `agreed` and into
    // `unread` with every assertion above still green.
    assert!(
        unread <= UNREAD_CEILING,
        "{unread} rows unread, ceiling is {UNREAD_CEILING} — a parser has been dropped, or the ceiling needs a reviewed rise"
    );
    // AND THE OTHER HALF OF IT. Without this, a change that made every parser
    // unsupported would leave `unread <= ceiling` satisfied and every
    // disagreement gone, which is a green test measuring nothing.
    assert!(
        agreed >= 1_000,
        "only {agreed} rows were actually read and compared; this test is not measuring the port"
    );
}

/// The unread residue belongs to exactly the parsers this server has not
/// transcribed, and to no others.
///
/// [`UNREAD_CEILING`] on its own is a number, and a number can stay still while
/// what it counts moves — one parser transcribed and another dropped nets to
/// zero. This asserts the *set*, so that trade is a red.
#[test]
fn the_unread_rows_are_exactly_the_untranscribed_parsers() {
    let doc = golden();
    let mut unread: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    let mut read: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for c in doc["cases"].as_array().expect("cases") {
        let parser = c["parser"].as_str().expect("parser");
        // Skipped for the same reason the main test skips them and with the
        // same accounting: a row where vanilla threw an unchecked exception is
        // not a row this server declined to read, it is a row with no verdict
        // to be graded against.
        if c["unchecked"].as_str().is_some() {
            continue;
        }
        let props = props_of(parser, &c["props"]);
        match ours(parser, &props, c["input"].as_str().expect("input")) {
            Ours::Unread => *unread.entry(parser).or_default() += 1,
            Ours::Read(..) => {
                read.insert(parser);
            }
        }
    }
    // A parser split across the two buckets is either an honest partial
    // transcription or a props shape falling silently through
    // `Parser::parse`'s match, and those look identical from here. So the
    // partial ones are DECLARED, with the construction each stops at, and
    // anything else split is a red.
    //
    //   minecraft:entity   `@a[...]` — `EntitySelectorOptions` is thirty-odd
    //                      options each with its own value grammar
    //   minecraft:message  a message CONTAINING such a selector, for the same
    //                      reason: `parseText` runs the selector parser
    //   minecraft:objective_criteria
    //                      a criterion holding a `:`, which
    //                      `ObjectiveCriteria.byName` sends to
    //                      `BuiltInRegistries.STAT_TYPE` and then to that stat
    //                      type's own registry. The 43 names `CRITERIA_CACHE`
    //                      holds are read and so is every colon-free reject,
    //                      because `byName` answers `Optional.empty()` for
    //                      those without touching a registry.
    //   minecraft:block_state
    //                      a `{...}` block-entity tag (`readNbt`, SNBT),
    //                      which `crate::blockarg` does not read — even on a
    //                      block with no block entity, where vanilla parses
    //                      the tag and drops it
    //   minecraft:component
    //                      anything outside `crate::componentarg`'s subset
    //                      of text components: numbers, booleans, lists,
    //                      translatable/score/selector contents, events, hex
    //                      colours — refused as unread, never answered
    //   minecraft:particle
    //                      a `{...}` options tag (SNBT) and a type that
    //                      carries options, whose codec decides whether the
    //                      empty map is enough; a simple type bare is read
    const PARTIAL: [&str; 6] = [
        "minecraft:block_state",
        "minecraft:component",
        "minecraft:entity",
        "minecraft:message",
        "minecraft:objective_criteria",
        "minecraft:particle",
    ];
    let split: Vec<&&str> = unread
        .keys()
        .filter(|k| read.contains(**k) && !PARTIAL.contains(*k))
        .collect();
    assert!(
        split.is_empty(),
        "{split:?} are read on some rows and unread on others and are not declared partial — a props shape is falling through `Parser::parse`'s match"
    );
    for name in PARTIAL {
        assert!(
            unread.contains_key(name) && read.contains(name),
            "{name} is declared partial and is wholly {} — the declaration is stale",
            if read.contains(name) {
                "read"
            } else {
                "unread"
            }
        );
    }
    assert_eq!(
        unread.keys().copied().collect::<Vec<_>>(),
        vec![
            "minecraft:block_state",
            "minecraft:component",
            "minecraft:entity",
            "minecraft:item_stack",
            "minecraft:message",
            "minecraft:objective_criteria",
            "minecraft:particle",
        ],
        "the set of parsers this server cannot fully read has changed"
    );
    assert_eq!(
        unread.values().sum::<usize>(),
        UNREAD_CEILING,
        "the ceiling and the residue have drifted apart"
    );
}

/// **The positive control.** The comparison above is only a gate if a wrong
/// parser fails it, and "wrong" here means wrong by ONE CHARACTER — the
/// failure mode the whole file exists for, and the one an accept/reject
/// comparison is blind to.
///
/// This replays the same rows through a parser deliberately made to
/// over-consume by one character and asserts the same comparison rejects it. If
/// this ever passes with zero disagreements, the test above is measuring
/// nothing.
#[test]
fn a_parser_wrong_by_one_character_is_caught() {
    let doc = golden();
    let mut caught = 0usize;
    let mut compared = 0usize;
    for c in doc["cases"].as_array().expect("cases") {
        let parser = c["parser"].as_str().expect("parser");
        let input = c["input"].as_str().expect("input");
        let props = props_of(parser, &c["props"]);
        let want_accepted = c["accepted"].as_bool().expect("accepted");
        let want_cursor = usize::try_from(c["cursor"].as_i64().expect("cursor")).expect("cursor");
        let Ours::Read(accepted, cursor) = ours(parser, &props, input) else {
            continue;
        };
        // The injection: one more character than the parser took, where there
        // is one to take. A real over-consuming parser is exactly this.
        let bumped = input[cursor_bytes(input, cursor)..]
            .chars()
            .next()
            .map_or(cursor, |ch| cursor + ch.len_utf16());
        if bumped == cursor {
            continue;
        }
        compared += 1;
        if (accepted, bumped) != (want_accepted, want_cursor) {
            caught += 1;
        }
    }
    assert!(
        compared >= 200,
        "only {compared} rows had a character left to over-consume; the control is not exercising the corpus"
    );
    assert_eq!(
        caught, compared,
        "{} of {compared} over-consuming rows were NOT caught — the comparison above cannot see a one-character error on them",
        compared - caught
    );
}

/// A UTF-16 index back to a byte offset — the inverse of [`utf16_index`], for
/// the control's own arithmetic.
fn cursor_bytes(input: &str, utf16: usize) -> usize {
    let mut units = 0usize;
    for (i, ch) in input.char_indices() {
        if units == utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    input.len()
}

/// Nothing outside the golden's parser set reaches [`Parser::parse`] as a
/// readable parser.
///
/// The registry is 57 names; the golden covers 15. This asserts the port's
/// coverage from the other side — every registry name the golden does NOT
/// carry must come back `Unsupported` — so a parser transcribed without a
/// golden row is an ungated tokeniser and is a red test rather than a reader
/// that agrees with nothing.
#[test]
fn no_parser_is_read_without_a_golden_row() {
    let doc = golden();
    let in_golden: std::collections::BTreeSet<&str> = doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|c| c["parser"].as_str().expect("parser"))
        .collect();
    let mut ungated = Vec::new();
    for name in PARSER_IDS {
        if in_golden.contains(name) {
            continue;
        }
        let p = Parser {
            registry: name,
            props: Props::Empty,
            suggestions: None,
        };
        let mut r = Reader::new("anything at all");
        if p.parse(&mut r) != Err(ArgError::Unsupported) {
            ungated.push(name);
        }
    }
    assert!(
        ungated.is_empty(),
        "{ungated:?} parse but have no golden row — ungated tokenisers"
    );
    assert_eq!(
        in_golden.len(),
        23,
        "the golden covers {in_golden:?}; 23 parser names is the expected set"
    );
}
