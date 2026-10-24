//! The answer to every command that does not run, against vanilla's own.
//!
//! `ci/commands-golden.sh` runs `Commands.finishParsing` — vanilla's, by
//! reflection, on a real `CommandSourceStack` whose `CommandSource` records
//! what it is sent — over eleven failing inputs at each of the five permission
//! levels, against vanilla's copy of the tree **this server** registers. The
//! recorded components are `system_chat` bodies through vanilla's own stream
//! codec. This replays the same 55 rows through
//! [`rustmc_server::commands::failure_components`] and compares bytes.
//!
//! # Why this reply is worth its own gate
//!
//! It is the observable answer for every typo, every root this server has not
//! ported, and every command a player lacks permission for — one reply shape
//! whose reach is the whole unported surface. Until it landed the answer to
//! all of those was silence, which on the wire is indistinguishable from a
//! command that ran and printed nothing, and `tools/oracle-runner`'s `chat`
//! projection digests the whole packet body, so there is no partial credit.
//!
//! # Three buckets again, and the middle one is the failure
//!
//! * **rendered** — vanilla sent two components and so did we, byte for byte;
//! * **silent** — vanilla sent nothing (the command succeeded) and neither did
//!   we;
//! * **unrendered** — vanilla sent two components and this server sent none,
//!   because the exception belongs to an argument type whose translation keys
//!   this server has not transcribed. **Currently zero**, and ratcheted at
//!   zero by [`UNRENDERED_CEILING`] so that the first one is a reviewed
//!   change.
//!
//! A disagreement in the first bucket is the only failure, and there are none:
//! 72 rendered, 8 silent, 0 unrendered, 0 disagreements.

use rustmc_server::commands::{failure_components, system_chat_body, tree};
use rustmc_server::commandtree::{Level, Refusal};
use serde_json::Value;
use std::path::PathBuf;

/// How many of the golden's 120 rows this server answers with silence where
/// vanilla answers with two components.
///
/// **Zero, and it is a ceiling rather than a measurement.** It was 6 when the
/// dispatcher's three keys landed without Brigadier's argument-level ones; the
/// `BrigadierExceptions` table closed those, and every failing input the
/// corpus carries is now byte-exact.
///
/// It stays because the surface is not: a `net.minecraft.commands.arguments`
/// type raises exceptions with keys of its own, and the moment this server's
/// tree registers an argument that can produce one, this goes to 1 and the
/// test reds. That is the reviewed change it exists to force, and it is why a
/// zero here is a ratchet rather than a claim of completeness.
const UNRENDERED_CEILING: usize = 0;

fn golden() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("data/vanilla/commands_tree.json");
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{}: {e} — run ci/commands-golden.sh", p.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn level_of(name: &str) -> Level {
    match name {
        "ALL" => Level::All,
        "MODERATORS" => Level::Moderators,
        "GAMEMASTERS" => Level::Gamemasters,
        "ADMINS" => Level::Admins,
        "OWNERS" => Level::Owners,
        other => panic!("unknown permission level {other}"),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Our two components against vanilla's, on every row that has any.
#[test]
fn every_failure_row_answers_the_way_vanilla_answered() {
    let doc = golden();
    let rows = doc["failures"]
        .as_array()
        .expect("no `failures` in the golden — run ci/commands-golden.sh");
    // 16 inputs x 5 levels. Asserted so that a golden that silently shrank
    // cannot leave this test green over four rows.
    assert_eq!(rows.len(), 120, "the golden holds {} rows", rows.len());

    let t = tree();
    let mut rendered = 0usize;
    let mut silent = 0usize;
    let mut unrendered: Vec<String> = Vec::new();
    let mut disagreements = Vec::new();
    for row in rows {
        let input = row["input"].as_str().expect("input");
        let level = level_of(row["level"].as_str().expect("level"));
        let want: Vec<&str> = row["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .map(|v| v.as_str().expect("hex"))
            .collect();
        let refusal = t.resolve(input, level).err();
        match (refusal, want.is_empty()) {
            // Vanilla succeeded and so did we.
            (None, true) => silent += 1,
            (None, false) => disagreements.push(format!(
                "{input:?} at {level:?}: vanilla failed with {} components, we accepted it",
                want.len()
            )),
            (Some(r), true) => disagreements.push(format!(
                "{input:?} at {level:?}: vanilla accepted it, we refused with {r:?}"
            )),
            (Some(r), false) => match failure_components(input, r) {
                None => unrendered.push(format!("{input:?} at {level:?} ({r:?})")),
                Some(got) => {
                    assert_eq!(want.len(), 2, "{input:?}: vanilla sent {}", want.len());
                    rendered += 1;
                    for (k, (line, expected)) in got.iter().zip(&want).enumerate() {
                        let body = system_chat_body(line).expect("buf");
                        if hex(&body) != *expected {
                            disagreements.push(format!(
                                "{input:?} at {level:?} line {k}:\n      vanilla {expected}\n      ours    {}",
                                hex(&body)
                            ));
                        }
                    }
                }
            },
        }
    }
    println!(
        "command-failure: {rendered} rendered, {silent} silent, {} unrendered \
         (ceiling {UNRENDERED_CEILING}), {} disagreements",
        unrendered.len(),
        disagreements.len()
    );
    assert!(
        disagreements.is_empty(),
        "{} of 120 rows disagree:\n  {}",
        disagreements.len(),
        disagreements.join("\n  ")
    );
    // Written as a subtraction rather than `<=` because clippy refuses a
    // comparison against a type's minimum as always-true, and `== 0` would
    // lose the ratchet the moment somebody raises the constant. This is the
    // same test at any value and survives the constant moving.
    assert_eq!(
        unrendered.len().saturating_sub(UNRENDERED_CEILING),
        0,
        "{} rows unrendered, ceiling is {UNRENDERED_CEILING}:\n  {}",
        unrendered.len(),
        unrendered.join("\n  ")
    );
    // Without this, a `failure_components` that returned `None` for everything
    // would leave zero disagreements and satisfy the ceiling, and this test
    // would be green over nothing.
    assert!(
        rendered >= 60,
        "only {rendered} rows were actually rendered and compared"
    );
}

/// **The positive control.** The comparison above is only a gate if a wrong
/// component fails it, and the failure mode this reply has is a wrong CURSOR:
/// the context window, the ellipsis threshold and the red-underlined remainder
/// are all derived from it, and every one of them still produces a
/// well-formed, plausible-looking component.
///
/// This replays every rendered row with the cursor moved by one and requires
/// the byte comparison to catch all of them.
#[test]
fn a_cursor_wrong_by_one_is_caught() {
    let doc = golden();
    let t = tree();
    let mut compared = 0usize;
    let mut caught = 0usize;
    for row in doc["failures"].as_array().expect("failures") {
        let input = row["input"].as_str().expect("input");
        let level = level_of(row["level"].as_str().expect("level"));
        let want: Vec<&str> = row["lines"]
            .as_array()
            .expect("lines")
            .iter()
            .map(|v| v.as_str().expect("hex"))
            .collect();
        let Some(refusal) = t.resolve(input, level).err() else {
            continue;
        };
        let Some(cursor) = refusal.cursor() else {
            continue;
        };
        // The injection: the same refusal, one character further on. Where the
        // cursor is already at the end, one character back — the point is that
        // it MOVED, not which way.
        let moved = if cursor < input.len() {
            cursor + 1
        } else if cursor > 0 {
            cursor - 1
        } else {
            continue;
        };
        let bent = match refusal {
            Refusal::UnknownCommand { .. } => Refusal::UnknownCommand { cursor: moved },
            Refusal::UnknownArgument { .. } => Refusal::UnknownArgument { cursor: moved },
            Refusal::Argument { syntax, .. } => Refusal::Argument {
                cursor: moved,
                syntax,
            },
            other => other,
        };
        let (Some(good), Some(bad)) = (
            failure_components(input, refusal),
            failure_components(input, bent),
        ) else {
            continue;
        };
        assert_eq!(want.len(), 2);
        compared += 1;
        let good_hex: Vec<String> = good
            .iter()
            .map(|c| hex(&system_chat_body(c).expect("buf")))
            .collect();
        let bad_hex: Vec<String> = bad
            .iter()
            .map(|c| hex(&system_chat_body(c).expect("buf")))
            .collect();
        assert_eq!(good_hex, want, "{input:?}: the control's baseline is wrong");
        if bad_hex != want {
            caught += 1;
        }
    }
    assert!(
        compared >= 30,
        "only {compared} rows had a movable cursor; the control is not \
         exercising the corpus"
    );
    assert_eq!(
        caught,
        compared,
        "{} of {compared} moved-cursor rows were NOT caught",
        compared - caught
    );
}

/// Anything this server leaves unrendered is a refusal it says it cannot
/// render, and no other kind.
///
/// [`UNRENDERED_CEILING`] is a number and a number can stay still while what it
/// counts moves. This asserts the KIND. The corpus currently leaves **nothing**
/// unrendered, which is why the assertion is a subset check rather than an
/// equality: an empty residue is the good state and must not have to be
/// declared as one.
///
/// The two kinds that may appear are `Argument` carrying a `Syntax::Unmapped`
/// — a `net.minecraft.commands.arguments` type's own exception, whose key this
/// server has not transcribed — and `Unparsed`, a parser it cannot read at
/// all. **An `UnknownCommand` or `UnknownArgument` here would be a defect**:
/// those three keys are ported and there is no reason for one to come back
/// unrendered.
#[test]
fn nothing_unrendered_is_a_refusal_this_server_claims_to_render() {
    use rustmc_server::commandtree::RefusalKind;
    let doc = golden();
    let t = tree();
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for row in doc["failures"].as_array().expect("failures") {
        let input = row["input"].as_str().expect("input");
        let level = level_of(row["level"].as_str().expect("level"));
        if row["lines"].as_array().expect("lines").is_empty() {
            continue;
        }
        let Some(r) = t.resolve(input, level).err() else {
            continue;
        };
        if failure_components(input, r).is_none() {
            *kinds.entry(format!("{:?}", r.kind())).or_default() += 1;
        }
    }
    let allowed = [
        format!("{:?}", RefusalKind::Argument),
        format!("{:?}", RefusalKind::Unparsed),
    ];
    for k in kinds.keys() {
        assert!(
            allowed.contains(k),
            "a {k} row came back unrendered, and this server claims to render \
             every one of those: {kinds:?}"
        );
    }
    println!("command-failure: unrendered by kind {kinds:?}");
}
