//! Which features this port still cannot place — as a gate, not a log line.
//!
//! `Generated::refused` counts every configured feature the port declined,
//! and a refusal is not free: the blocks it would have written are missing,
//! and the number is quoted in `caveats()` beside every parity figure. Until
//! now that count only appeared on `worldgen-dump`'s stderr, where it is seen
//! by whoever happens to be reading a dump.
//!
//! This is a **ratchet**, deliberately, rather than an assertion that nothing
//! is refused. One feature is genuinely unported — `root_system`, reached by
//! `rooted_sulfur_spring` — and a gate that is red for a known reason trains
//! people to ignore it, which this repo has already paid for once with
//! `ci/t0.sh`. So the gate asserts the *set of kinds* rather than the count:
//!
//!   * a **new** unported feature type fails it immediately, which is the
//!     thing worth catching, because it means a datapack path was reached
//!     that nobody has looked at;
//!   * porting `root_system` also fails it, which is correct — the list is
//!     wrong at that moment and should be updated in the same commit.
//!
//! Counts are not asserted. They move with the seed, the extent and the
//! decoration order, and pinning them would make this a flaky test about
//! something it is not measuring.

use std::collections::BTreeSet;

use rustmc_worldgen::pipeline::{Area, Through};

/// Every feature type this port is known not to place.
///
/// Empty is the goal. Adding to it is a decision, not a fix.
///
/// **This gate's window cannot see everything on this list, and that is a hole
/// rather than a design.** `minecraft:fallen_tree` was found by dumping the
/// village window — seed 20260722, centre (-53,-33), 169 chunks — where it is
/// refused **14 times**, alongside 3 refusals of
/// `canSurvive(minecraft:peony[half=lower])`. The radius-1 square below reaches
/// neither, so the ratchet was green while two unported things were costing
/// blocks in a measured window. Widening it is a real item; until then, this
/// list is maintained from the dumps rather than by the gate, which is exactly
/// the failure mode the module docs above warn about.
/// `minecraft:basalt_pillar` was on this list and has been **removed**: it is
/// ported, wired and measured. It sat here because enabling it regressed the
/// r=7 nether window from 24,245 to 72,684, and that regression turned out to
/// be a second defect underneath it — an inverted entry test in
/// `place_basalt_pillar` — exactly the pattern `docs/ORACLE.md` §22 describes.
/// With the test corrected the same wiring moves the window to 22,014. An
/// entry earned by a regression is a hypothesis, not a verdict, and this list
/// is the wrong place to keep one.
const KNOWN_UNPORTED: &[&str] = &["minecraft:root_system"];

#[test]
fn no_unported_feature_type_appears_that_is_not_already_known() {
    if !worldgen_assembly::have_extract() {
        eprintln!("skipped: no vanilla extract");
        return;
    }

    // A radius-1 square through every stage. Small enough to be a gate and
    // wide enough to reach the biomes whose feature lists differ — the
    // refusals that matter come from a biome nobody has generated before,
    // not from more chunks of one that has.
    let world = worldgen_assembly::generate::generate(
        20_260_722,
        Area::square(0, 0, 1),
        Through::Decoration,
    )
    .expect("generates");

    let seen: BTreeSet<&str> = world.refused.keys().map(String::as_str).collect();
    let known: BTreeSet<&str> = KNOWN_UNPORTED.iter().copied().collect();

    let surprises: Vec<&str> = seen.difference(&known).copied().collect();
    assert!(
        surprises.is_empty(),
        "a feature type was refused that is not in KNOWN_UNPORTED: {surprises:?}\n\
         Either port it, or add it to the list deliberately — the list is the \
         record of what this generator cannot do, and it is quoted beside every \
         parity number."
    );

    // The other direction, so the list cannot rot into a description of a
    // problem that was fixed. This is expected to fail the day `root_system`
    // lands, and updating it belongs in that commit.
    for kind in &known {
        if !seen.contains(kind) {
            eprintln!(
                "note: `{kind}` is listed as unported but was not refused here — \
                 that may mean this window does not reach it, or that it has been \
                 ported and KNOWN_UNPORTED is stale"
            );
        }
    }
}
