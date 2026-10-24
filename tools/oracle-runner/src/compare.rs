//! Diffing two traces, and the corpus number that comes out of it.
//!
//! # The verdict that matters
//!
//! From a client, "the server did something different" and "the server has
//! never heard of this" look identical: in both cases the packet you were
//! waiting for did not arrive. Telling them apart is the whole reason this
//! is a *differential* harness rather than a list of assertions.
//!
//! * [`Verdict::Match`] — same observations, same order.
//! * [`Verdict::Diverge`] — the subject produced observations of the kinds
//!   the reference did, and they differ. The mechanism exists and is wrong.
//! * [`Verdict::Absent`] — the subject produced **nothing** where the
//!   reference produced something. The mechanism is not implemented.
//! * [`Verdict::Inert`] — the *reference* produced only what the scenario's
//!   own actions force it to produce, **and the subject agreed**. The check
//!   cannot fail. Both halves are load-bearing: the reference decides whether
//!   anything informative was on offer, and the agreement is what makes
//!   "cannot fail" a claim rather than a guess. A subject that disagreed has
//!   just demonstrated the opposite.
//! * [`Verdict::Mute`] — the scenario's **measured step**, the last
//!   `[[step]]` and the one the file is named for, contributed nothing on
//!   either side. The window as a whole was informative and the assertion
//!   was not in it, so the check is green whatever the server does. The
//!   third of the three broken-check verdicts, and the one that catches a
//!   scenario passing by comparing two silences.
//! * [`Verdict::Vacuous`] — the *reference* produced nothing. The scenario
//!   proves nothing about anything and is a fault in the check, not in the
//!   server. This is the rule that has burned this project twice, mechanised:
//!   a negative result is only evidence if the mechanism was demonstrably
//!   live, and vanilla's own trace is the proof that it was.
//! * [`Verdict::Error`] — one side could not be run.
//!
//! # Two denominators, always
//!
//! Every run reports the corpus twice. **Declared** applies the projection
//! the scenario asked for — the kinds it is about, and its timing rule.
//! **Strict** compares everything recorded, including the packets no decoder
//! has been written for, whose digests make them exact by construction.
//!
//! The gap between the two is the honest measure of how much has been
//! deferred: a check can pass under its own projection while the strict
//! comparison still shows the server saying something vanilla did not. That
//! number is free to compute and would be invisible if only one were
//! reported — which is exactly how deferred verification turns into
//! forgotten verification.

use crate::scenario::{Scenario, Timing};
use crate::trace::{Obs, Trace};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// --- the identity of the projection ----------------------------------------
//
// **A floor is a number taken under a comparator, and until 2026-08-09 it did
// not say which one.** `ci/t2-gameplay.sh` checked the floor's *denominator*
// against the run's — 88 against 88 — and called that staleness. It did not
// check the projection, and `gameplay-container-throw` was the first branch
// ever to change one: [`exempt_item_spawn_yaw`] drops an item entity's spawn
// yaw, and two of the nine names in the standing floor
// (`container-click-throw-drops-one-item` and `-drops-the-whole-stack`) are
// scenarios that exemption converts. The denominator stayed 88, the staleness
// test passed, the floor file was internally consistent, and the figure
// described a comparator that no longer existed.
//
// That is a **check that could not fail**, which is this project's oldest
// defect wearing a measurement's costume, and it is the same one
// `ci/provenance.sh` was written for one directory over: a gate must never
// quote an artefact it cannot attribute. This is that mechanism, copied rather
// than reinvented — a digest of what produced the number, stamped beside the
// number, recomputed by the consumer, and a loud refusal on a mismatch.
//
// # Why the sources, and not a hand-written manifest of the axes
//
// A declared list of axes ("drops `y_rot` from an item `add_entity`", "deletes
// the heartbeat unless the scenario names it") reads better and is what a
// human wants beside the floor. It is also **omittable**: the branch that adds
// the tenth axis is exactly the branch that forgets to declare it, and the
// stamp would then certify a projection that had changed. `prov_gen_digest`
// makes the same choice for the same reason — it digests the generator script
// whole, not a description of what the generator does.
//
// So the identity is mechanical. What a human needs is served by
// `oracle-runner projection-id --explain`, which prints the per-source line
// and byte counts a mismatch has to be diagnosed from.
//
// # The three sources, and why not all six
//
// `compare.rs` is the projection. `scenario.rs` defines what a scenario may
// ask for — [`Timing`], `observe_from`, `within`, `collapse_repeats` — so it
// decides the projection's parameters. `trace.rs` is the decoder, and it
// decides which fields exist to be compared at all: naming `y_rot` was a
// `trace.rs` change, and the exemption above could not have been written
// narrowly without it.
//
// `client.rs`, `record.rs` and `main.rs` are the **recorder**, whose identity
// is already tracked separately as `recorder_digest` in the ensemble manifest.
// Conflating the two would make every floor stale on every recorder change and
// tell a reader nothing about which of the two moved.
//
// A **new** module is the hole a fixed list leaves, and it is closed by a test
// rather than by a comment: `every_oracle_source_is_declared_in_or_out_of_the_projection`
// reads `src/` and fails until the new file is placed in one list or the
// other. T0 runs `--lib --bins`, so it reaches this crate.
//
// # What is stripped, and the two residuals that leaves
//
// Comment-only lines, blank lines, and everything from the `#[cfg(test)]`
// module to the end of the file. Measured over the 54 commits touching these
// three files in the 21 days to 2026-08-09: the raw text moved on **53** of
// them and the stripped text on **41**. Neither is small, and that is correct
// rather than tolerable — a branch that edits the comparator is precisely the
// population that must re-measure, and `ci/corpus-floor.sh` re-measures over
// recordings already on disk in seconds. What stripping buys is that a floor
// does not go stale because a doc comment was corrected, which is the same
// judgement `ci/provenance.sh` already makes about `commit=`.
//
//   1. a line inside a raw string whose first non-space characters are `//` is
//      stripped as though it were a comment. `scenario.rs` holds twelve raw
//      strings, all of them inside the test module and therefore already cut;
//      there are none in the kept text of any of the three today.
//   2. the digest is [`crate::trace::digest`], 64 bits, and not a cryptographic
//      hash. It answers "did these bytes change", which is the question, and
//      the payload carries each source's kept byte and line counts beside its
//      text so a collision would have to match those too. A new dependency was
//      the alternative and the workspace `Cargo.toml` says additions go
//      through the orchestrator.
/// The sources whose text defines what a comparison looks at, in a fixed
/// order. Embedded at compile time, so the identity describes **the binary
/// that ran** and not the tree it is standing in — which is the half that
/// matters on a box where `rsync -a` can leave sources older than the
/// artefacts built from them.
pub const PROJECTION_SOURCES: [(&str, &str); 4] = [
    ("compare.rs", include_str!("compare.rs")),
    // **`pitch.rs` is a projection source and adding it moves `proj:`.** That
    // is the correct price and not an accident of listing it: the module
    // decides whether a recorded `sound.pitch` is compared exactly, compared
    // as the range it is drawn from, or dropped, and those are three different
    // comparisons of the same recording. A floor measured before it existed
    // was measured by a comparator that could not see a pitch at all.
    //
    // The test below is what caught it. `pitch.rs` landed as a new file in
    // `tools/oracle-runner/src` and was in neither list, which is precisely
    // the shape that test exists to refuse — a file that changes what a
    // comparison looks at, arriving without anyone deciding whether the
    // standing floors survive it. They do not; `ci/checks/gameplay-floor.toml`
    // reads STALE PROJECTION until the floor is re-measured, and that is the
    // honest state rather than a cost to be avoided.
    ("pitch.rs", include_str!("pitch.rs")),
    ("scenario.rs", include_str!("scenario.rs")),
    ("trace.rs", include_str!("trace.rs")),
];

// **THE RECORDER'S DECLARATION USED TO LIVE HERE, AND THAT WAS THE BUG.**
//
// `pub const RECORDER_SOURCES: [&str; 3]` sat in this file until 2026-08-09,
// three lines under `PROJECTION_SOURCES`, with a comment saying the two were
// tracked separately because "conflating them would make every floor stale on
// every recorder change". The comment was right about the hazard and the code
// did not have the property: this file **is** a projection source, so declaring
// a recorder module — or embedding the recorder's text to digest it — edited
// the projection's own bytes and moved `proj:`. Every standing floor went stale
// on a recorder change, which is the sentence the comment wrote to forbid.
//
// Measured on this branch, which is how it was found rather than argued:
// adding `recorder_identity()` here took `projection-id` from
// `proj:58380c9e4cbeae81` to `proj:e72858dd73a06e16` for a function that
// changes nothing about what a comparison looks at, and `ci/checks/gameplay-floor.toml`
// would have gone STALE PROJECTION for it.
//
// The list is now [`crate::record::RECORDER_SOURCES`]. Moving it costs one
// bump of `proj:` — this commit's, and it is paid once.
//
// **AND THE SENTENCE THAT USED TO FOLLOW WAS WRONG — corrected 2026-09-05.**
// It read "after which no change to the recorder can move the projection
// identity again", and that is true only of the three FILES named in
// `RECORDER_SOURCES`. The separation this list buys is by file, not by role,
// and `trace.rs` is on both sides of it: it is a `PROJECTION_SOURCES` entry,
// and it also holds `observe`/`decode`, the recorder's whole packet decoder.
// `trace::observe` is called from exactly one place in this crate,
// `client.rs:1091` — a recorder source — and from nothing in `compare.rs` or
// `scenario.rs`. So decoder work is recorder work by call graph and moves
// `proj:` every time.
//
// Measured rather than argued, over the 14 commits touching a projection
// source between `53cf64244` (where `ci/checks/gameplay-floor.toml` was
// stamped at `proj:7cefde0fb56d9f3e`) and main `1a31e0a56`
// (`proj:1277e43a7e157183`) — **10 of the 14 moved the identity**:
//
//     scenario.rs  3 moves, +76 kept lines
//     trace.rs     6 moves, +200 kept lines
//     compare.rs   1 move,   -5 kept lines
//
// and four of trace.rs's six are one item, the `ClientboundCommandsPacket`
// decoder (`8d4eb93c4`, `a0eba090b`, `7bc0637a2`, `7d3a1d98d`, +70 kept lines
// between them), which changes nothing about what `project` looks at and
// everything about what a recording contains.
//
// **The code is right and the promise was too wide.** A new decoder arm turns
// a packet that reached a recording as `packet:0x10` plus a digest into a
// structured observation with named fields, and a comparison of two later
// recordings does look at different things because of it — so `trace.rs`
// belongs in `PROJECTION_SOURCES` and a standing floor genuinely does go stale
// on a decoder change. What does not follow, and what the old sentence
// promised, is that a floor is safe from recorder work. It is safe from work
// in those three files. Re-measuring is cheap and `ci/corpus-floor.sh` does it
// over recordings already on disk; the sentence that said it would not be
// needed is what cost a standing floor its quotability for the whole of
// 2026-08-18 to 2026-09-05.
//
// [`projection_body`] is shared rather than copied: two implementations of one
// strip are two answers waiting for an occasion, and the identity would be
// worth nothing if the recorder's stripping rule could drift from the
// projection's.

/// The attribute that opens the test module. Exactly one per source, and more
/// than one is a refusal rather than a guess about which one ends the file.
const TEST_MODULE: &str = "#[cfg(test)]";

/// The text of one source with comments, blank lines and tests removed.
///
/// Panics on a source carrying more than one `#[cfg(test)]`, because the cut
/// is "from the marker to the end of the file" and a second marker means that
/// sentence has stopped being true. Refusing beats silently digesting half a
/// file, which would move the identity for no reason a reader could see.
pub(crate) fn projection_body(name: &str, src: &str) -> String {
    let markers = src
        .lines()
        .filter(|l| l.trim_start().starts_with(TEST_MODULE))
        .count();
    assert!(
        markers <= 1,
        "{name}: {markers} `{TEST_MODULE}` markers — the projection identity cuts the test \
         module by finding exactly one. Split the tests into one module or teach \
         `projection_body` the new shape."
    );
    let kept: Vec<&str> = src
        .lines()
        .take_while(|l| !l.trim_start().starts_with(TEST_MODULE))
        .filter(|l| {
            let t = l.trim_start();
            !t.is_empty() && !t.starts_with("//")
        })
        .collect();
    assert!(
        !kept.is_empty(),
        "{name}: nothing survived the strip — the projection identity would be a digest of \
         the empty string, which is the same value for every tree"
    );
    kept.join("\n")
}

/// `proj:<16 hex>` — the identity of the projection this binary applies.
///
/// Stamped into `ci/checks/gameplay-floor.toml` by `ci/corpus-floor.py update`
/// and re-derived by `ci/t2-gameplay.sh`, which prints `STALE PROJECTION` and
/// withdraws the floor's quotability when the two disagree.
#[must_use]
pub fn projection_identity() -> String {
    let mut payload = String::from("oracle-runner projection identity v1\n");
    for (name, src) in PROJECTION_SOURCES {
        let body = projection_body(name, src);
        payload.push_str(&format!(
            "== {name} {} bytes {} lines\n{body}\n",
            body.len(),
            body.lines().count()
        ));
    }
    format!("proj:{}", crate::trace::digest(payload.as_bytes()))
}

/// The per-source breakdown a mismatch has to be diagnosed from.
///
/// A bare identity says two projections differ and nothing about where, and
/// the first thing anybody does with a mismatch is ask which file moved.
#[must_use]
pub fn projection_explain() -> String {
    let mut out = format!("projection {}\n", projection_identity());
    for (name, src) in PROJECTION_SOURCES {
        let body = projection_body(name, src);
        out.push_str(&format!(
            "  {name:<14} {:>6} lines  {:>7} bytes  {}\n",
            body.lines().count(),
            body.len(),
            crate::trace::digest(body.as_bytes())
        ));
    }
    out.push_str(&crate::record::recorder_explain());
    out
}

/// How a scenario came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Ranked lowest on purpose: a broken check and an unrunnable target are
    /// never an acceptable resting state, so a baseline can never bless one.
    Error,
    Vacuous,
    /// The scenario's **measured step** — its last `[[step]]`, the action it
    /// is named for — contributed **zero declared observations on both
    /// sides**, and the scenario did not declare that it should
    /// ([`crate::scenario::MeasuredStep::Silent`]). The check is green
    /// whatever the server does at the only step it claims to be about.
    ///
    /// # The third verdict, and why the two that exist could not catch this
    ///
    /// [`Self::Vacuous`] asks whether the *reference* said anything at all,
    /// and [`Self::Inert`] asks whether what it said was anything beyond what
    /// the scenario's own actions force. Both are questions about the
    /// scenario **as a whole**, and a scenario can pass both while its
    /// measured step contributes nothing: the rig steps before it speak, the
    /// totals are healthy, and the assertion at the end is compared against
    /// an empty sequence on each side.
    ///
    /// That is not hypothetical and it is the finding this verdict closes.
    /// `607-container-click-pickup-all-gathers-matching-stacks` was
    /// `declared match` with `reference_obs: 7` and `subject_obs: 7`, and
    /// **all seven observations on each side came from steps 0..5**. Its
    /// step-6 `pickup_all` — the click the whole scenario exists to make —
    /// produced one packet on vanilla's side, `packet:0x60`, which `607` does
    /// not `observe`; so under its own projection both sides said nothing and
    /// the two silences were compared and found equal. The check was green
    /// whatever `crates/rustmc-server/src/click.rs` did, and implementing
    /// `PICKUP_ALL` would have closed nothing.
    ///
    /// # Only a `Match` may be relabelled
    ///
    /// The same rule [`Self::Inert`] was corrected into on 2026-08-03, for
    /// the same reason, and it is load-bearing twice over here.
    ///
    /// *It cannot swallow a real divergence.* Both sides being silent at the
    /// measured step does not make the rest of the scenario agree, so a
    /// `Diverge` or an `Absent` elsewhere in the window survives untouched.
    ///
    /// *And it is why the test reads both sides rather than the reference
    /// alone.* A subject that spoke at the measured step where the reference
    /// did not has demonstrated that the projection can see that step, which
    /// refutes the claim this verdict makes; the sequences then differ and
    /// the ordinary verdict is `Diverge`, which is the honest description —
    /// the mirror of the one-sided case `verdict_for` was corrected for.
    ///
    /// # Counted after `Inert`, not before
    ///
    /// A scenario that is inert is already saying the stronger thing — that
    /// nothing *anywhere* in it survives subtracting its own actions — and
    /// relabelling those would move an existing number for no new finding.
    /// So this fires only on what `Inert` let through, and its claim is
    /// exactly: **this check is informative, and not at the step it is named
    /// for.**
    Mute,
    /// The reference *spoke*, and said only what the scenario's own actions
    /// force it to say: the acknowledgement, and the two `handleUseItemOn`
    /// resyncs at the positions the step clicked — **and the subject said the
    /// same thing.** Nothing here could have come out differently if the
    /// mechanic had not fired, so the check can never fail and is as broken as
    /// a vacuous one.
    ///
    /// The second half was missing until 2026-08-03 and the omission was not
    /// cosmetic. A forced resync carries the actual block state at that
    /// position, so a server that placed the wrong block differs from the
    /// reference on the one axis the position-based allowance does not look
    /// at — and that `Diverge` was being relabelled as a dead check.
    ///
    /// This exists because `vacuous` stopped being sufficient. Before the
    /// unconditional resyncs were reproduced, a scenario that provoked
    /// nothing produced an empty vanilla trace and was caught. Afterwards
    /// `water-spreads` went `absent` -> `match` on two servers agreeing to
    /// do nothing, with three observations on the reference side, and no
    /// verdict noticed.
    Inert,
    Absent,
    Diverge,
    Match,
}

impl Verdict {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Vacuous => "vacuous",
            Self::Mute => "mute",
            Self::Inert => "inert",
            Self::Absent => "absent",
            Self::Diverge => "diverge",
            Self::Match => "match",
        }
    }

    /// Whether this verdict may ever be recorded as an accepted state.
    ///
    /// `Absent` and `Diverge` may: they are honest descriptions of a server
    /// that is not finished, and a gate that refused them would mean nothing
    /// could ever land. `Vacuous`, `Mute`, `Inert` and `Error` may not,
    /// because none of the four says anything about the server at all — the
    /// first because the reference was silent, the second because the step
    /// the check is named for was, the third because it said only what the
    /// scenario made it say, the fourth because a side would not run.
    ///
    /// **This is a whitelist and not a blacklist, and that is deliberate**: a
    /// verdict added later is unbaselineable until somebody decides
    /// otherwise, which is the direction an omission should fail in.
    #[must_use]
    pub const fn baselineable(self) -> bool {
        matches!(self, Self::Absent | Self::Diverge | Self::Match)
    }
}

/// One scenario's comparison.
#[derive(Debug, Clone, Serialize)]
pub struct Outcome {
    pub name: String,
    pub about: String,
    pub verdict: Verdict,
    /// The same comparison with nothing projected away.
    pub strict: Verdict,
    pub reference_obs: usize,
    pub subject_obs: usize,
    /// Index of the measured step — the last `[[step]]`. `None` for a
    /// scenario with no measured steps at all.
    pub measured_step: Option<usize>,
    /// How many of `reference_obs` are attributed to the measured step.
    ///
    /// Reported unconditionally, not only when [`Verdict::Mute`] fires,
    /// because the interesting reading is the *ratio*: `607` was `7` and `0`,
    /// and that pair says on its face that seven observations of rig-building
    /// were being compared and none of the assertion. A quantity that appears
    /// only when a gate is already red is a quantity nobody sees coming.
    pub measured_reference_obs: usize,
    /// See [`Self::measured_reference_obs`].
    pub measured_subject_obs: usize,
    /// The distinct kinds the reference produced **at the measured step**.
    ///
    /// The count above says how much evidence there is; this says what it is
    /// made of, and the pair is what a reader needs to tell a claim from its
    /// name. `550-attacking-another-player` declares `sound`, `damage_event`,
    /// `set_health` and `entity_event`, is `match`, and reports
    /// `measured_reference_obs = 1` — from which nobody could tell that the
    /// one observation is a **sound**, that vanilla emits none of the other
    /// three for a player hitting a player, and that the check therefore says
    /// nothing whatever about damage.
    ///
    /// It took `ci/observe-coverage.py` and a recording pair to find that.
    /// With this field it is on the face of every report, for every check, at
    /// no cost — which is the difference between a property somebody audits
    /// and a property everybody can see.
    pub measured_reference_kinds: Vec<String>,
    /// Kinds the reference produced and the subject produced none of.
    pub missing_kinds: Vec<String>,
    /// Kinds the subject produced and the reference produced none of.
    pub extra_kinds: Vec<String>,
    /// The first place the two projected sequences part company, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_difference: Option<String>,
    /// The declared projection of each side, as compared.
    ///
    /// `#[serde(skip)]` on purpose: this is what `oracle-runner explain`
    /// prints and it must never enter `report.json`, which is a gate artefact
    /// read by scripts and diffed by people. Carried on the outcome rather
    /// than re-derived by the caller because a second copy of the projection
    /// is a second instrument, and two instruments that can disagree about
    /// what a check observed is the exact defect this harness exists to find.
    #[serde(skip)]
    pub reference_projection: Vec<serde_json::Value>,
    /// See [`Self::reference_projection`].
    #[serde(skip)]
    pub subject_projection: Vec<serde_json::Value>,
    /// The **strict** projection of each side, on the same terms.
    ///
    /// Carried for the same reason the declared pair is, and it was missing
    /// for longer than it should have been. `explain` could say where a
    /// declared verdict came apart and had nothing at all to say about a
    /// strict one — so every strict divergence had to be diagnosed by reading
    /// raw traces by hand, with a second reading of the projection rules in
    /// somebody's head. Three of the corpus's open items live entirely in the
    /// strict column; an instrument that cannot report its own second
    /// denominator makes every claim about them unauditable.
    #[serde(skip)]
    pub reference_strict_projection: Vec<serde_json::Value>,
    /// See [`Self::reference_strict_projection`].
    #[serde(skip)]
    pub subject_strict_projection: Vec<serde_json::Value>,
}

/// The raw entity id inside a recorded field, if that field is one.
///
/// A single-key object under [`crate::trace::ENTITY_REF`] and nothing else.
/// Deliberately structural rather than a list of field names: `entity`,
/// `item`, `taker`, `cause`, `direct` and the array in `remove_entities` are
/// all entity references today, and the next decoder arm that adds one should
/// not have to be remembered here to be renumbered.
fn entity_ref_id(v: &serde_json::Value) -> Option<i64> {
    let m = v.as_object()?;
    if m.len() != 1 {
        return None;
    }
    m.get(crate::trace::ENTITY_REF)?.as_i64()
}

/// Replace raw entity ids with **the order they appear in this projection**.
///
/// # This is the whole of the entity-scope fix, and it belongs here
///
/// It used to happen in `trace::decode`, which runs on every non-housekeeping
/// packet — before `observe`, before `within`, before `observe_from`, and
/// before `record::run_one` sorts the two sockets apart. An entity mentioned
/// by a packet the check filters away therefore took an index anyway and
/// pushed every later one along, so a check could diverge on the numbering of
/// an entity it never claimed to watch. Measured, vanilla against vanilla, on
/// `sand-falls-when-unsupported`: the same falling block at the same position
/// with a byte-identical tail, numbered 0 in one run and 1 in the next,
/// because an unobserved `move_entity` for the other player arrived first in
/// one of them.
///
/// Running here makes the index mean "the n-th entity **this projection**
/// saw", which is what every scenario's `about` already reads as, and it is
/// the only scope under which the two sides are numbering the same population.
/// It also runs after the sort by `who`, so the 25 ms socket interleaving
/// `record::run_one` already discards from the *order* is now out of the
/// *value* too.
///
/// Both denominators get their own numbering, and that is correct rather than
/// incidental: `declared` numbers over the kinds the check named, `strict`
/// over everything inside the scenario's box. A packet only strict looks at
/// can no longer move a number declared compares.
fn renumber_entities(projected: &mut [serde_json::Value]) {
    fn walk(v: &mut serde_json::Value, seen: &mut BTreeMap<i64, usize>) {
        if let Some(raw) = entity_ref_id(v) {
            let next = seen.len();
            *v = serde_json::json!(*seen.entry(raw).or_insert(next));
            return;
        }
        match v {
            serde_json::Value::Object(m) => {
                // `serde_json::Map` iterates in key order, so the sequence
                // this assigns indices in does not depend on which order a
                // decoder arm happened to insert its fields.
                //
                // That is `serde_json`'s **default** `Map`. Turning on its
                // `preserve_order` feature — which a sibling crate could pull
                // in without anyone here noticing — would make it insertion
                // order instead and silently change what every recorded
                // comparison means. `trace::tests::
                // a_damage_events_cause_and_direct_entities_are_normalised`
                // asserts the key-order outcome for exactly that reason: it
                // goes red rather than letting the semantics move.
                for (_, x) in m.iter_mut() {
                    walk(x, seen);
                }
            }
            serde_json::Value::Array(a) => {
                let all_refs = !a.is_empty() && a.iter().all(|x| entity_ref_id(x).is_some());
                for x in a.iter_mut() {
                    walk(x, seen);
                }
                if all_refs {
                    // `remove_entities` carries a set, in whatever order the
                    // server's own collection iterated. Canonicalise it —
                    // after numbering, because sorting raw ids and sorting
                    // the indices they become are different orders.
                    a.sort_by_key(|x| x.as_i64().unwrap_or(i64::MAX));
                }
            }
            _ => {}
        }
    }
    let mut seen: BTreeMap<i64, usize> = BTreeMap::new();
    for o in projected {
        if let Some(f) = o.get_mut("fields") {
            walk(f, &mut seen);
        }
    }
}

/// Rebase the session counters onto **this scenario**.
///
/// # One error on one side moved 38 checks, and not one of them was the error
///
/// `block_changed_ack.seq` and the container `window` are both counters that
/// advance with the **session**, not with the scenario. Nothing resets them
/// between checks, so their value at scenario N is a function of everything
/// the run did before N — and the two sides of a comparison are two
/// independent runs. Any event that makes one run's history differ from the
/// other's moves every later value on that side, and the comparator, which
/// has no way to know that happened, reports a wall of divergences naming
/// checks that did nothing wrong.
///
/// Measured on the `worldgen-iceberg-912` gate of 2026-08-10, whose two
/// traces are the case this function exists for. `water-spreads` failed on
/// the **vanilla** side only. [`crate::record::record`] drops the party on any
/// error — correctly; a half-read frame would desync everything after it —
/// and the next scenario logs in fresh, which restarts `Session::seq` at one.
/// Ours never failed, so ours never reconnected and its counter kept
/// climbing. From the very next scenario every `seq` on our side read exactly
/// **31** higher than vanilla's: 33 against 2, 35 against 4, 38 against 7,
/// unbroken to the end of the corpus. Twenty-six checks went `match` ->
/// `diverge` on that constant, and twelve more went the same way on `window`,
/// which vanilla's reconnect had reset for exactly the same reason.
///
/// **Nothing consumed 31 sequence numbers.** Both sides stood at 31 when
/// `water-spreads` began; the offset is the distance between a counter
/// twenty-one scenarios old and a brand new one, and every check it convicted
/// was innocent. A constant offset across unrelated scenarios never is a
/// count of defects.
///
/// So what gets compared is each observation's distance from the first one
/// **in its own projection**, per connection. Every gap inside a scenario
/// survives — a side that acknowledges 7 then 9 still differs from one that
/// acknowledges 7 then 8, and an acknowledgement that is missing outright
/// still shortens the sequence — while the part that only ever encoded run
/// history is gone. What is given up is a uniform additive offset within one
/// scenario, and that is the quantity no scenario could ever have
/// adjudicated: the harness owns the `seq` counter and hands both servers the
/// same number for the same step, so a uniform offset can only have come from
/// history.
///
/// # What it does NOT repair, and `strict` is where that shows
///
/// This rebases *values*. It cannot repair a side that emitted a **different
/// set of packets** because it reconnected — a fresh login re-sends the
/// player's own inventory, and those are observations one side has and the
/// other does not. `declared` is unaffected, because it only projects the
/// kinds a check named; `strict` projects everything inside the scenario's
/// box and so still sees the difference. Measured on the same iceberg-912
/// traces: after rebasing, `declared` is clean and one scenario
/// (`a-furnace-opens-a-screen-a-chest-does-not`) is still `strict: diverge`
/// while its declared projection is two observations that agree field for
/// field. So a one-sided reconnect is bounded here, not abolished, and the
/// bound is exactly the gated number.
///
/// # It cannot merge two observations that were distinct
///
/// The transform is `raw - base` on `seq` and `raw - base + 1` on an
/// allocated `window`, which is injective on each connection's counter, and
/// the reserved `window` 0 is excluded from the domain and cannot be landed
/// on. So nothing this does can make two different observations compare
/// equal, and in particular it cannot feed [`collapse_repeats`] a pair it
/// would not otherwise have seen. That is the whole of the safety argument
/// for running it inside the projection rather than after it.
///
/// # This also closes the hazard `menu.rs` was already warning about
///
/// [`crate::menu`]'s note on the eighteen unimplemented containers says that
/// a scenario opening a menu on vanilla and none here leaves "*every* later
/// menu check in the run reporting a window id one too low — pointing at four
/// healthy checks instead of the one missing block". Rebasing deletes that
/// false blame without hiding the defect: the scenario that opens no menu
/// still has no `open_screen` to compare and still fails on its own.
///
/// # Window 0 is a name, not a draw
///
/// [`crate::menu::next_container_counter`] runs `1..=100` and never returns 0
/// — `menu`'s own
/// `the_container_counter_runs_one_to_a_hundred_and_never_zero` holds that —
/// because 0 is the id of the player's own inventory and is never allocated.
/// It is a constant rather than a position in a sequence, and rebasing it
/// would collapse "the server answered on the inventory" and "the server
/// answered on the menu it just opened" into the same observation. It is left
/// exactly as it is.
///
/// # The third counter is the container state id, and it is scoped to a menu
///
/// Added 2026-08-11. `container_set_slot.state` is the same class of quantity
/// as the two above and was the last one still compared raw. It cost three
/// entries in `ci/checks/gameplay-unstable.toml` —
/// `swapping-to-the-offhand-shows-on-both-channels` at 11/25 (N = 26),
/// `closing-a-crafting-table-returns-the-grid-contents` at 8/15 and
/// `a-flower-pot-empties-when-right-clicked` at 9/15 (both N = 6) — every one
/// of them naming this field and every one of them naming this fix.
///
/// Read off the pinned 26.2 jar rather than taken from those entries, because
/// three entries agreeing is three readings of code and ORACLE rule 9 says
/// readings of code do not move suspects. `AbstractContainerMenu` holds
/// `private int stateId`; `incrementStateId` is
/// `stateId = (stateId + 1) & 32767` and **returns the incremented value**;
/// `ServerPlayer$1.sendSlotChange` — the `ContainerSynchronizer` — builds
/// `ClientboundContainerSetSlotPacket(containerId, incrementStateId(), slot,
/// stack)`, and `sendInitialData` calls the same method for
/// `container_set_content`. So the number on the wire counts **packets
/// actually sent on that menu since it was opened**, not changes made.
///
/// The two are not the same count, and that is the whole instability.
/// `broadcastChanges` walks every slot once a tick into
/// `synchronizeSlotToRemote`, which sends only where `RemoteSlot.matches` is
/// false — so whether an earlier write produces one packet or two is decided
/// by which 50 ms tick sweep saw which intermediate state, and every later
/// packet on that menu inherits the difference. Vanilla cannot reproduce it
/// against vanilla.
///
/// # It is scoped per menu, where `seq` and `window` are scoped per connection
///
/// `stateId` is an instance field of `AbstractContainerMenu`, so each menu
/// counts on its own and a fresh menu starts from zero. The base is therefore
/// keyed by the `window` the observation names as well as by `who`. Two
/// menus open in one scenario keep two independent bases, which is what lets
/// a check still say that the container answered twice while the inventory
/// answered once.
///
/// # Window 0 has no `window` rebase and it is the one that most needs a
/// # `state` rebase
///
/// These are two different fields and the paragraph above is about the other
/// one. The `window` **field** of the player's own inventory is the constant
/// `0` and is left alone. The `state` **counter** on that same window 0 is
/// rebased exactly like any other, and it is the case that pays: `window`
/// 0's `InventoryMenu` is created with the player and never closes, so it is
/// the only menu whose counter accumulates across the *whole corpus run*.
/// `ci/checks/gameplay/README.md` is about this — `790` must stay the
/// highest-numbered check because `FlowerPotBlock.useWithoutItem` reaches
/// `Player.addItem` and *advances* the window-0 counter for everything after
/// it. All three retired entries land on window 0.
///
/// # Rebased, NOT dropped, and the difference is the deliverable
///
/// Dropping the field would stop all three flapping too, and it would be
/// wrong: how far the counter moves **between two packets on one menu** is
/// real behaviour a check may legitimately assert, and a server that sent one
/// packet where vanilla sent two would stop being caught. Subtracting a base
/// keeps every gap and discards only the history in front of the first
/// observation. The pair
/// `two_streams_differing_only_in_session_history_project_the_same` and
/// `two_streams_differing_in_the_gap_between_two_packets_project_differently`
/// separates the two, and the second is the one a dropping decoder fails.
///
/// # The subtraction is done in the counter's own modulus
///
/// `& 32767` means the counter wraps at 32768, so a base near the top and an
/// observation just past the wrap would otherwise project as a large
/// negative. `rem_euclid` is the wire's own arithmetic. It is still injective
/// on one menu's counter — two raw values in `0..=32767` differ by less than
/// the modulus, so they cannot be congruent — which is what the "cannot merge
/// two observations that were distinct" argument above needs.
fn rebase_session_counters(projected: &mut [serde_json::Value]) {
    // Matched on the field name rather than on the kind. `window` is written
    // by `open_screen`, `container_set_slot` and `container_set_content`
    // today and they must share one base to go on referring to one menu, and
    // the next decoder arm that carries one should not have to be remembered
    // here to be rebased — the same argument `entity_ref_id` makes for being
    // structural rather than a list.
    const COUNTERS: [&str; 3] = ["seq", "window", "state"];

    // `AbstractContainerMenu.incrementStateId` is `(stateId + 1) & 32767`, so
    // the state id lives in `0..MODULUS` and a difference across the wrap has
    // to be taken in that ring rather than in the integers.
    const STATE_MODULUS: i64 = 32768;

    // `seq` and `window` are allocated once per connection; `state` is
    // allocated once per **menu**. `CONNECTION` is the scope the first two
    // share, and it is outside the range of any window id so it can never
    // collide with one.
    const CONNECTION: i64 = i64::MIN;

    // Keyed by `who` as well as by counter: the actor and the observer are
    // two separate connections, so they are two separate counters, and one
    // base shared between them would be the same conflation this function
    // exists to undo. The third element is the counter's scope — see
    // `CONNECTION`.
    let mut base: BTreeMap<(String, &str, i64), i64> = BTreeMap::new();
    for o in projected {
        let Some(who) = o
            .get("who")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let Some(fields) = o
            .get_mut("fields")
            .and_then(serde_json::Value::as_object_mut)
        else {
            continue;
        };
        // Read before the loop rewrites anything, so the menu a `state`
        // belongs to is named by the same number on every observation
        // whatever order `COUNTERS` happens to be in.
        let window = fields.get("window").and_then(serde_json::Value::as_i64);
        for name in COUNTERS {
            // A `state` on an observation that does not say which menu it is
            // on cannot be scoped, and a counter rebased against the wrong
            // menu's base is worse than one left raw. Nothing in the corpus
            // produces one — `container_set_slot` and `container_set_content`
            // both write `window` — so this is a refusal rather than a path,
            // and a recording that predates `window` being named passes
            // through untouched, which is the same honest behaviour
            // `exempt_item_spawn_yaw` has on a digest it cannot reach.
            let scope = if name == "state" {
                let Some(w) = window else { continue };
                w
            } else {
                CONNECTION
            };
            let Some(slot) = fields.get_mut(name) else {
                continue;
            };
            let Some(raw) = slot.as_i64() else {
                continue;
            };
            // Window 0 is the inventory's name, not a draw, and an allocated
            // id must not be able to land on it: rebasing the first menu of
            // the scenario to 0 would make "answered on the inventory" and
            // "answered on the menu it just opened" the same observation.
            // So allocated ids rebase into `1..`, which is also the range
            // `menu::next_container_counter` itself produces. This is the
            // `window` FIELD; the `state` counter *on* window 0 is rebased
            // like every other and is the case all three retired entries
            // land on.
            if name == "window" && raw == 0 {
                continue;
            }
            let b = *base.entry((who.clone(), name, scope)).or_insert(raw);
            *slot = serde_json::json!(match name {
                "state" => (raw - b).rem_euclid(STATE_MODULUS),
                "window" => raw - b + 1,
                _ => raw - b,
            });
        }
    }
}

/// The scenarios where the two runs did **not** take the same path through
/// the corpus — one reconnected and the other did not.
///
/// # A repair is not a detector, and this is the detector
///
/// [`rebase_session_counters`] makes the known session counters — `seq`,
/// `window` and, since 2026-08-11, the per-menu `state` — survive a one-sided
/// reconnect. That is a repair of the quantities we know about, and it is
/// worth exactly as much as the completeness of that list. The fault is not
/// "`seq` was wrong": it is **one side silently restarting every session
/// counter it has while the other keeps counting**, and the next such quantity
/// somebody decodes will not be in `COUNTERS`. That the list has already grown
/// once, from two names to three, is the argument for this paragraph rather
/// than against it. The failure would then look precisely as it did on
/// 2026-08-10 — a wall of divergences naming healthy checks, every trace
/// agreeing with every other trace while all of them are wrong.
///
/// So the fact of the reconnect is recorded by the recorder and compared here
/// on its own, independently of any counter. If this returns a non-empty list
/// the two recordings are not strictly comparable from that scenario onward,
/// whatever the verdicts say, and the reader is told so in those words rather
/// than left to infer it from a constant offset.
///
/// A scenario whose flag is `None` on either side is skipped: traces written
/// before the field existed carry no measurement, and an absent measurement
/// is not a negative one.
#[must_use]
pub fn session_path_divergence(reference: &Trace, subject: &Trace) -> Vec<String> {
    let mut out = Vec::new();
    for r in &reference.scenarios {
        let Some(s) = subject.scenarios.iter().find(|s| s.name == r.name) else {
            continue;
        };
        if let (Some(a), Some(b)) = (r.fresh_session, s.fresh_session) {
            if a != b {
                let (fresh, kept) = if a {
                    ("reference", "subject")
                } else {
                    ("subject", "reference")
                };
                out.push(format!(
                    "{}: {fresh} reconnected here and {kept} did not — every per-connection \
                     counter restarted on one side only",
                    r.name
                ));
            }
        }
    }
    out
}

/// Where every entity this trace has placed currently is, in **absolute**
/// world coordinates, by raw entity id.
///
/// Absolute and not anchor-relative because the whole point is to compare an
/// entity against a *different* scenario's anchor. `ScenarioTrace` records
/// its own anchor, so absolute positions are recoverable from what is already
/// on disk and nothing has to be re-recorded.
pub(crate) type Places = BTreeMap<i64, [f64; 3]>;

/// The position an observation should be judged by, relative to `anchor` —
/// and the position updates that observation implies.
///
/// # The bound `near` could not apply
///
/// `near` answers "is this the scenario's business" by reading the `at` field,
/// and **twenty recorded kinds do not have one**: `set_entity_motion`,
/// `move_entity`, `set_entity_data`, `set_equipment`, `entity_event`,
/// `take_item_entity` and the rest of entity tracking. Vanilla puts the
/// entity's id on those packets, not its position, because the client already
/// knows where the entity is. So every one of them was kept unconditionally,
/// from anywhere in the world.
///
/// That is not a theoretical gap. Workspaces march 32 blocks apart and
/// `within` defaults to 16 — `scenario::within` says in as many words that 16
/// is "tight enough to exclude the neighbouring workspace 32 blocks away" —
/// but an item's client tracking range covers the neighbour easily. A dropped
/// item from `500-dropping-an-item-throws-it-on-the-floor` supplied **12 of
/// 12** of `520-a-standing-player-is-rebroadcast-on-a-timer`'s non-heartbeat
/// observations in one corpus run and **34 of 42** of
/// `060-coral-dries-out-unprompted`'s in another, and which run got which is
/// the leftover's lifetime, not anything either server decided.
///
/// # Why this cannot manufacture a check that cannot fail
///
/// **An entity whose position is unknown is never bounded**, and since
/// 2026-08-09 a **player is never placed**, so the two clients are that case
/// by rule rather than by luck. Every observation about a player survives this
/// unchanged, and `520`, whose whole content is the players' heartbeats,
/// measures precisely what it measured before. The rule only ever fires on an
/// entity this trace watched arrive somewhere else, which is the case it
/// exists for.
///
/// It used to be luck, and the luck had already run out: `530` announces a
/// player with `add_entity` inside a measured window and `540` was being
/// emptied by it. The measurement is in the body of this function.
///
/// An observation that carries its own `at` keeps exactly its old bound: the
/// relative position is returned untouched. It also *teaches* the table where
/// its entity is, which is how `add_entity` and `entity_position` seed this.
fn place(o: &Obs, anchor: [i32; 3], places: &mut Places) -> Option<[f64; 3]> {
    let ent = o
        .fields
        .get("entity")
        .and_then(entity_ref_id)
        .or_else(|| o.fields.get("entity").and_then(|v| v.as_i64()));
    let rel_of = |abs: [f64; 3]| -> [f64; 3] {
        [
            abs[0] - f64::from(anchor[0]),
            abs[1] - f64::from(anchor[1]),
            abs[2] - f64::from(anchor[2]),
        ]
    };
    let abs_of = |rel: [f64; 3]| -> [f64; 3] {
        [
            rel[0] + f64::from(anchor[0]),
            rel[1] + f64::from(anchor[1]),
            rel[2] + f64::from(anchor[2]),
        ]
    };

    if let Some(at) = o.fields.get("at").and_then(|v| v.as_array()) {
        let mut rel = [0.0f64; 3];
        for (i, s) in rel.iter_mut().enumerate() {
            *s = at.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0);
        }
        if let Some(e) = ent {
            // **Only an arrival may put an entity into the table.**
            //
            // `add_entity` is vanilla saying "this entity came into your view,
            // here" — the one packet that states a position with no prior
            // context. Everything else *updates* an entity already placed.
            //
            // Seeding from `entity_position` as well is what the first
            // version of this did, and prediction 7 refuted it on the
            // measurement: the two clients get an `entity_position` inside
            // each scenario's window, and then `record::run_one` walks them
            // 32 blocks to the next workspace **outside any measured
            // window**, so the trace contains no packet saying they moved.
            // Their placement went stale by exactly one spacing, they were
            // bounded out of the following scenario, and
            // `520-a-standing-player-is-rebroadcast-on-a-timer` — whose whole
            // declared content is their heartbeats — went `vacuous`, which is
            // gate-red.
            //
            // # AND `add_entity` WAS NOT SAFE EITHER. Measured 2026-08-09.
            //
            // This comment used to end "a player is never announced by
            // `add_entity` inside a measured window, because it joined at
            // login, so this rule leaves every client unplaced and therefore
            // unbounded". **That sentence was false when it was written and
            // `530-walking-away-drops-a-standing-player-from-view` is why**:
            // it exists to walk the actor out of the observer's tracking
            // range and back, and coming back is an `add_entity` — for a
            // player, inside a measured window, at that workspace.
            //
            // Read out of `twin-homog-20260809`'s r5, scenario 63 of 88, at
            // anchor `[-2048, -60, 0]`: `add_entity entity 1 kind_id 156` —
            // `minecraft:player` — at relative `[0, 0, 77]`. `540` is the very
            // next scenario, at anchor `[-2080, -60, 0]`, so the actor is
            // remembered 32 blocks and 77 blocks away from it and `within` is
            // 16. Every observation about the actor that does not carry its
            // own `at` is then bounded out of `540`.
            //
            // That is exactly what made `540` look SILENT rather than
            // unstable. In four of the six recordings vanilla forces a
            // position sync, the `entity_position` carries an `at`, the stale
            // placement is corrected on the spot and the step-1 delta
            // survives. In the other two vanilla sends the outbound walk as a
            // **delta** — `move_entity d = [0, 0, 12288]` at 64 ms in r5 and
            // 87 ms in r6, which is the packet the check is about — the stale
            // placement is never corrected, and BOTH deltas are bounded out.
            // The projection comes out empty, `ci/corpus-floor.py` reports
            // "2 of 6 runs recorded nothing at all", and the evidence line
            // reads "the other said nothing more" about a recording that had
            // said plenty. An instrument describing its own filter as the
            // reference's silence is the failure mode this file is a
            // catalogue of.
            //
            // So a player is never placed, which is what the old sentence
            // claimed was true for free. It is narrow in the right direction:
            // the harness walks the clients between workspaces with no packet
            // on the wire, so a client's placement is stale by exactly one
            // `record::WORKSPACE_SPACING` at every scenario boundary — 32
            // blocks against a `within` of 16 — and it can therefore only ever
            // subtract observations that belong. Nothing is leaked back in by
            // it either: a client is always *in* the workspace being measured,
            // which is the property the leakage rule was protecting against
            // for items.
            //
            // What it costs, stated rather than hidden: an entity that
            // arrived in view at one workspace and then genuinely travelled
            // more than `within` without a positioned packet would be judged
            // by where it arrived. Nothing in this corpus does — a dropped
            // item lands within a block or two of the player and a spawn-egg
            // mob does not cross 16 blocks in a five-second window — and a
            // check that ever needs it has `entity_position` available to say
            // so.
            // `is_some_and`, so the table is read only when the packet
            // actually carries a type. A recording that predates `kind_id`
            // being decoded is passed through as it always was, and a unit
            // test that hand-builds an `add_entity` does not acquire a
            // dependency on `data/vanilla/entity_types.json` by standing
            // next to this line.
            let is_player = o
                .fields
                .get("kind_id")
                .and_then(serde_json::Value::as_i64)
                .is_some_and(|k| k == i64::from(player_entity_type_id()));
            if o.kind == "add_entity" {
                if !is_player {
                    places.insert(e, abs_of(rel));
                }
            } else if let Some(p) = places.get_mut(&e) {
                *p = abs_of(rel);
            }
        }
        return Some(rel);
    }

    // A delta says how far, not where. It only helps for an entity already
    // placed — which is the point: it keeps a tracked entity's position
    // current across the walk without ever inventing one. Wire units are
    // 1/4096 of a block, the same encoding `MOVED_WIRE_SQ` is expressed in.
    if let (Some(e), Some(d)) = (ent, o.fields.get("d").and_then(|v| v.as_array())) {
        if let Some(p) = places.get_mut(&e) {
            for (i, s) in p.iter_mut().enumerate() {
                *s += d.get(i).and_then(serde_json::Value::as_f64).unwrap_or(0.0) / 4096.0;
            }
        }
    }

    let bound = ent.and_then(|e| places.get(&e)).copied().map(rel_of);

    // Forgetting is as load-bearing as remembering: an entity that left this
    // client's tracking range and came back somewhere else must not be judged
    // by where it used to be. Applied *after* the bound, so the packet that
    // announces the removal is still judged by where the entity was.
    if let Some(list) = o.fields.get("entities").and_then(|v| v.as_array()) {
        for e in list.iter().filter_map(entity_ref_id) {
            places.remove(&e);
        }
    }
    bound
}

/// Walk a scenario's observations for their position information alone.
///
/// `project` runs twice per scenario — once declared, once strict — so it
/// cannot be what advances the table, or the second call would see the first
/// call's movement applied twice.
pub(crate) fn advance(obs: &[Obs], anchor: [i32; 3], places: &mut Places) {
    for o in obs {
        place(o, anchor, places);
    }
}

/// A named entity type's numeric id, read from
/// `"data/vanilla/entity_types.json"`.
///
/// Two callers, and they want opposite things from the same table:
/// [`exempt_item_spawn_yaw`] wants `minecraft:item` so it can drop one byte
/// from an item's spawn, and [`place`] wants `minecraft:player` so it can
/// refuse to remember where a client was.
///
/// **Read, not written down.** The experiment `docs/PARITY-ROADMAP.md` §11
/// describes hardcoded `71`, and §11 says in as many words why a landed
/// version must not: the registry renumbers whenever an entity type is added
/// or removed, and a hardcoded id does not fail when that happens — it
/// silently stops firing, and the exemption below then quietly stops applying
/// to items and starts applying to whatever inherited the number.
///
/// # It refuses rather than defaulting
///
/// A missing or malformed table panics with the path in the message. The
/// alternative — resolve to "no type matches" and carry on — is the shape of
/// defect this project keeps paying for: every instrument would agree, the
/// corpus number would move, and nothing would say why. `ci/t2-gameplay.sh`'s
/// preflight already requires this exact table to be present and stamped for
/// the tree being measured (it is in the `REQUIRED` grep), so the panic is
/// unreachable from the gate; it exists for the hand-run case.
///
/// The parent walk is for `cargo test`, whose working directory is the
/// package root rather than the tree root. It tries the working directory and
/// four parents, which is `tools/oracle-runner` plus slack. Every shipped
/// caller — `ci/t2-gameplay.sh` and `ci/selfcheck-recompare.sh` — `cd`s to the
/// tree root first, so they hit it on the first try.
fn entity_type_id(name: &str) -> i32 {
    use std::sync::OnceLock;
    static TABLE: OnceLock<serde_json::Value> = OnceLock::new();
    let v = TABLE.get_or_init(|| {
        const REL: &str = "data/vanilla/entity_types.json";
        let mut dir = std::path::PathBuf::from(".");
        for _ in 0..5 {
            let path = dir.join(REL);
            if path.is_file() {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                return serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            }
            dir.push("..");
        }
        panic!(
            "{REL} not found from {:?} or its parents — run ci/gen-gameplay-tables.sh",
            std::env::current_dir()
        );
    });
    v["entity_types"][name]
        .as_i64()
        .and_then(|n| i32::try_from(n).ok())
        .unwrap_or_else(|| {
            panic!("data/vanilla/entity_types.json: no {name} — run ci/gen-gameplay-tables.sh")
        })
}

/// `minecraft:item`. See [`entity_type_id`].
fn item_entity_type_id() -> i32 {
    entity_type_id("minecraft:item")
}

/// `minecraft:player`. See [`entity_type_id`] and [`place`].
fn player_entity_type_id() -> i32 {
    entity_type_id("minecraft:player")
}

/// DROP: `add_entity.movement` — `ServerEntity.getLastSentMovement()`, which
/// for a standing player depends on which tick vanilla's last motion packet
/// went out on and for a thrown item is four fresh `nextFloat()` draws.
///
/// # This moved here from `trace::decode` and the comparison did not change
///
/// Until 2026-08-18 the three packed shorts were read and thrown away inside
/// `crate::trace`'s `add_entity` arm, so `movement` was never a field and this
/// function had nothing to remove. Moving the removal one layer up leaves the
/// projected observation byte-identical — `an_add_entity_movement_is_dropped_
/// by_the_projection` asserts exactly that — and makes the field **recorded**,
/// which is the whole of the change.
///
/// A field consumed at decode is invisible to every instrument this repository
/// has: `ci/trace_vocabulary.py` cannot list it, so a floor's `vocabulary`
/// cannot record it; `ci/determinism-gap.py` cannot count it, so nothing can
/// say whether vanilla varies in it where we do not; `ci/hidden-stream-audit.py`
/// cannot read it out of the client that 102 of 115 checks discard. That is why
/// `ci/checks/projection-drops.toml` could get no further than `armed` for this
/// row — real behaviour, compared by nothing — and why `armed` was a ceiling
/// rather than a verdict.
///
/// # It is TWO TYPES wide, and the width was measured rather than argued
///
/// `ci/entity-spawn-probe.sh`, N = 6 cold vanilla servers, seed 20260803,
/// superflat, `difficulty=peaceful`, 2026-08-18, one recorder. Ten
/// `(scenario, kind_id)` rows carried an `add_entity` in all six runs:
///
/// ```text
///   DRAWN     minecraft:item          6 distinct   dropping-an-item-throws-it-on-the-floor
///   CONSTANT  minecraft:oak_boat      [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:copper_golem  [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:iron_golem    [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:minecart      [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:armor_stand   [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:end_crystal   [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:item_frame    [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:pig           [0.0, 0.0, 0.0]
///   CONSTANT  minecraft:falling_block [0.0, 0.0, 0.0]
/// ```
///
/// So the ledger's reason was right about the thrown item and wrong about
/// everything else, and the drop was nine tenths unjustified. `minecraft:pig`
/// is the row that settles the shape: its **yaw** is drawn — that is what puts
/// `080-spawn-egg-makes-a-mob` on the floor at 25 flaps in 25 — and its
/// **movement** is a constant zero in all six. The draw is per FIELD and not
/// per entity.
///
/// # WHY NINE CONSTANTS AT N = 6 IS ENOUGH HERE AND WOULD NOT BE IN GENERAL
///
/// `ci/entity-movement-report.py` prints `p_hide` on every run and at N = 6 it
/// is 45.1%: a field varying less often than that can read constant six times.
/// That is a real limit and it is the reason its header refuses to call
/// `constant` "reproducible".
///
/// The narrowing does not rest on the count. It rests on the **value**. Nine
/// rows across **four different spawn routes** — item-placed, carved pumpkin,
/// gravity, spawn egg — all read exactly `[0.0, 0.0, 0.0]`, which is what
/// `ServerEntity.getLastSentMovement()` returns for an entity whose velocity
/// nothing has set. Six identical draws from a continuous distribution would be
/// a coincidence; nine types agreeing on the identity element is a mechanism.
/// Had they agreed on some arbitrary shared vector, this would still be
/// dropped.
///
/// # `minecraft:player` STAYS DROPPED AND IS NOT MEASURED HERE
///
/// The probe has no scenario that announces a player, so the ledger's other
/// draw — a tick-phase coin toss on which tick vanilla's last motion packet
/// went out — is untested by this run and the type keeps its exemption. Do not
/// remove it on the strength of the table above; it is not in the table.
///
/// # WHAT THIS CANNOT DO TODAY, WHICH IS THE POINT
///
/// It cannot move a verdict. Every type this now compares is a type for which
/// `crates/rustmc-server` emits no `add_entity` at all — measured in
/// `ci/entity-broadcast-symmetry.py`'s `WE EMIT NOTHING AT ALL` list — so the
/// checks are `absent` and stay `absent`. What changes is that the field is
/// **armed for the entity system when it lands**, instead of being a green
/// nobody could ever have earned. A narrowing that silently flipped a check to
/// `match` would be the thing this repository keeps catching.
fn drop_add_entity_movement<'a>(
    kind: &str,
    fields: &'a BTreeMap<String, serde_json::Value>,
) -> std::borrow::Cow<'a, BTreeMap<String, serde_json::Value>> {
    use std::borrow::Cow;
    if kind != "add_entity" || !fields.contains_key("movement") {
        return Cow::Borrowed(fields);
    }
    // The two types the ledger's reason actually establishes a draw for. Read
    // from the generated registry rather than written down, for the reason
    // `the_item_entity_type_id_comes_from_the_generated_table` gives: a loader
    // silently answering 0 would exempt `minecraft:acacia_boat` and nothing
    // else, and every test that only checked "it resolved" would still pass.
    let ty = fields.get("kind_id").and_then(serde_json::Value::as_i64);
    let drawn = [item_entity_type_id(), entity_type_id("minecraft:player")]
        .into_iter()
        .any(|t| ty == Some(i64::from(t)));
    if !drawn {
        return Cow::Borrowed(fields);
    }
    let mut out = fields.clone();
    // DROP: add_entity.movement — `ServerEntity.getLastSentMovement()`: a tick-phase
    // coin toss for a standing player and four unseeded `nextFloat()` draws for a
    // thrown item. Moved here from `trace::decode` on 2026-08-18 so the field is
    // RECORDED and only the comparison loses it; see this function's docs.
    out.remove("movement");
    Cow::Owned(out)
}

/// Drop an item entity's spawn yaw from an `add_entity`, and nothing else.
///
/// # What is removed, and why exactly this
///
/// `ItemEntity(EntityType, Level)` offsets 39-52 are
/// `setYRot(this.random.nextFloat() * 360.0F)`, drawn from the entity's own
/// `RandomSource`, which `Entity.<init>` creates unseeded. So the `yRot` byte
/// of an item's `add_entity` is a fresh random angle in every run of every
/// server, vanilla included. `docs/PARITY-ROADMAP.md` §11 measured it:
/// `605-container-click-throw-drops-one-item` and `606` were both
/// `unstable 2/2` on the **gated** denominator against saved vanilla
/// recordings, and `ci/checks/gameplay-unstable.toml` has named the same call
/// at the same offsets under `dropping-an-item-throws-it-on-the-floor` since
/// 2026-08-05, where it is why a pre-registered 25/25 → 0/25 came back 25/25.
///
/// This is the same move `569323e` made for `sound.pitch` — a field drawn the
/// same way, dropped from the comparison and bought back in unit tests.
///
/// # Why it is one field and not `rest`
///
/// Before `crate::trace`'s `add_entity` arm named them, `xRot`, `yRot`,
/// `yHeadRot` and `data` were four anonymous bytes inside one digest, and the
/// narrowest available exemption was to drop the whole digest. Three of those
/// four are constant `0` for every item vanilla spawns, so dropping `rest`
/// would have stopped comparing three real values to reach one that cannot be
/// compared. Naming them first costs a decoder change and buys the difference.
///
/// # Why this cannot manufacture a check that cannot fail
///
/// An item's `add_entity` still compares its `kind_id`, its position, its
/// normalised entity index, its `xRot`, its `yHeadRot`, its `data`, its step
/// and its `rest`. The two checks this unblocks fail today on **absence** —
/// this server spawns no item entity on a container throw at all — and they
/// would fail again the moment one appeared at the wrong height, on the wrong
/// step, or as the wrong entity type. What stops being tested is that the
/// angle is *drawn*, and that is claimed instead by
/// `rustmc_server::dropitem::tests::every_dropped_item_gets_a_random_spawn_yaw`,
/// which asserts a hundred spawns produce >90 distinct yaws in `[0, 360)` —
/// an assertion a server writing a constant fails.
///
/// # It is narrow in the other direction too
///
/// The type test is on `minecraft:item` alone.
/// `070-sand-falls-when-unsupported` announces a `minecraft:falling_block`,
/// whose `add_entity` is byte-identical across runs (§11 measured all three
/// recordings at `rest dc54fa7f9dce24f3`), and it keeps every field it had.
/// So does `530`'s player. A recording made before the rotations were named
/// has no `y_rot` field and is passed through untouched, which is the honest
/// behaviour: the exemption cannot reach a digest.
fn exempt_item_spawn_yaw<'a>(
    kind: &str,
    fields: &'a BTreeMap<String, serde_json::Value>,
) -> std::borrow::Cow<'a, BTreeMap<String, serde_json::Value>> {
    use std::borrow::Cow;
    if kind != "add_entity" || !fields.contains_key("y_rot") {
        return Cow::Borrowed(fields);
    }
    if fields.get("kind_id").and_then(serde_json::Value::as_i64)
        != Some(i64::from(item_entity_type_id()))
    {
        return Cow::Borrowed(fields);
    }
    let mut out = fields.clone();
    // DROP: add_entity.y_rot_of_an_item — `ItemEntity`'s ctor draws the yaw
    out.remove("y_rot");
    Cow::Owned(out)
}

/// A projected observation: what the comparison actually looks at.
#[expect(
    clippy::too_many_arguments,
    reason = "every argument is one axis of the projection and a struct would \
              hide which of the two calls per scenario differs from the other"
)]
pub(crate) fn project(
    obs: &[Obs],
    kinds: Option<&BTreeSet<String>>,
    observed: &BTreeSet<String>,
    timing: Timing,
    from: crate::scenario::From,
    within: i32,
    anchor: [i32; 3],
    places: &Places,
) -> Vec<serde_json::Value> {
    use crate::trace::Who;
    // Positions are read from **every** observation, before any filter: an
    // `add_entity` the scenario's `observe_from` hides still says where that
    // entity is, and a table built from the surviving half would place
    // entities differently on the two sides of the same comparison.
    let mut seen = places.clone();
    let bounds: Vec<Option<[f64; 3]>> = obs.iter().map(|o| place(o, anchor, &mut seen)).collect();
    let mut out: Vec<serde_json::Value> = obs
        .iter()
        .zip(bounds)
        .filter(|(o, _)| match from {
            crate::scenario::From::Both => true,
            crate::scenario::From::Actor => o.who == Who::Actor,
            crate::scenario::From::Observer => o.who == Who::Observer,
            crate::scenario::From::Probe => o.who == Who::Probe,
        })
        .filter(|(o, _)| kinds.is_none_or(|k| k.contains(&o.kind)))
        // **The timer broadcast is invisible unless a check names it.**
        //
        // `crate::trace::HEARTBEAT` is a `move_entity` that failed vanilla's
        // own movement test — see [`crate::trace::MOVED_WIRE_SQ`] for the
        // derivation — so it is on the wire only because
        // `ServerEntity.sendChanges` re-broadcasts every tracked entity on
        // `tickCount % 60 == 0`. Every scenario here stands a second player
        // five blocks away and no window in the corpus is three seconds long,
        // so that broadcast lands inside the window on a phase coin flip,
        // under every check at once.
        //
        // That is not a small effect and it is not a guess about one. At
        // N = 12 the aggregate compares runs 2..12 against run 1, so one fair
        // coin per run predicts a disagreement rate of 1/2 and a
        // reproduces-in-all-eleven rate of 2⁻¹¹. Measured 2026-08-04, before
        // this line existed: the least unstable nine checks sat at **exactly
        // 5 of 11**, and vanilla reproduced itself under strict on **0 of
        // 55**. The mechanism predicts both numbers, which is why it is worth
        // acting on rather than tolerating.
        //
        // The line runs for `declared` and `strict` alike, and the exception
        // is what buys it back: a check that writes the kind into its own
        // `observe` list gets its heartbeats. Without that clause this fix
        // would be self-defeating — it would make a server that never sends
        // the heartbeat at all indistinguishable from vanilla, which is a
        // check that cannot fail, manufactured by our own hand. See
        // `ci/checks/gameplay/520-a-standing-player-is-rebroadcast-on-a-timer.toml`,
        // which exists for exactly that reason and does not ship separately
        // from this line.
        //
        // # The exception reads `observed`, not `kinds`, and that is the fix
        //
        // It used to read `kinds`, which `strict` passes as `None` — so
        // `is_some_and` was false on every strict projection and the exception
        // could not fire there. For `520`, whose `observe` list is exactly
        // `["move_entity_heartbeat"]` and nothing else, that deleted **both**
        // strict projections outright: an empty reference is `vacuous`, and
        // `520` was strict-`vacuous` in all twelve gate runs that carry
        // per-scenario detail. It was the one member of the declared/strict
        // gap that no server change could ever vacate, because the instrument
        // removed its whole subject before either side was looked at.
        //
        // `observed` is the scenario's own `observe` list, passed to both
        // calls. `kinds` stays what it was — `Some` for declared, `None` for
        // strict — so strict still counts every other kind, including the ones
        // no decoder reads. What changes is only which projections the
        // heartbeat is *deleted* from: now neither, for a check that named it.
        //
        // Deleting a kind from one denominator and not the other was never the
        // intent. `strict`'s definition is "the scenario's own bounds and
        // client, every packet"; a kind the scenario explicitly asked to see is
        // not an exception to that.
        // **One filter, one table.** `crate::trace::FAMILIES` is the whole
        // classification and this is the only line in this workspace that
        // acts on it. The exception clause — a check that writes the kind
        // into its own `observe` list gets every one of them — is what keeps
        // this from manufacturing a check that cannot fail, and it is why
        // the two commentaries above are arguments about *rows* rather than
        // about code.
        //
        // It reads `observed` and not `kinds`: `strict` passes `kinds =
        // None`, so a `kinds`-based exception could not fire there, and for
        // `520` — whose `observe` list is exactly `["move_entity_heartbeat"]`
        // — that deleted both strict projections outright and made it
        // strict-`vacuous` in all twelve gate runs carrying per-scenario
        // detail.
        // DROP: ambient_unless_observed — a `trace::FAMILIES` row of `Class::Ambient`, unless the scenario names the kind
        .filter(|(o, _)| !crate::trace::is_ambient_kind(&o.kind) || observed.contains(&o.kind))
        // **A resync driven by a bit the client sent is not evidence about the
        // server**, and the same exception buys it back.
        //
        // `ServerEntity.sendChanges` offsets 648-694 send
        // `ClientboundEntityPositionSyncPacket` on any of five conditions, and
        // the fifth — `wasOnGround != entity.onGround()`, offsets 680-691 — is
        // decided by the `onGround` flag in the *client's own* movement
        // packets. `crates/rustmc-server/src/tracker.rs:236-242` already ports
        // that test verbatim, so a stray one is not a missing rule on either
        // side.
        //
        // What is missing is a **shared input**. `ci/t2-gameplay.sh` records
        // all scenarios against vanilla and then all of them again against
        // this server, so the reference and the subject are two separate
        // client sessions: the harness's own on-ground bit, and its phase
        // against the server tick, are two independent draws. No server change
        // can align them, and nothing a scenario does can provoke them.
        //
        // Measured on P2 (23 scenarios, actor stream, 2026-08-07 07:10):
        // **3 strays on vanilla's side — `060` at 5310 ms, `290` at 598 ms,
        // `360` at 1296 ms — and 2 on ours — `237` at 1289 ms, `520` at
        // 1175 ms.** Same packet, both directions, never coinciding, about one
        // window in eight. A quantity that appears on whichever side the coin
        // fell is a property of the instrument.
        //
        // # The exception is the whole of the safety argument
        //
        // Exactly as for the heartbeat above: a check that writes
        // `entity_position` into its own `observe` list gets every one of
        // them. Two do — `080-other-player-is-an-entity` and
        // `540-a-player-that-leaves-the-ground-is-resynced`, which exists to
        // provoke this packet by jumping — so a server that stopped sending it
        // fails both and nothing else changes. **If 540 is ever retired this
        // line must be retired with it**, and the rule must not be widened to
        // `move_entity`, which 080 and 520 read and which would leave no
        // positive control at all.
        //
        // # It cannot move a declared verdict, by construction
        //
        // `kinds` is `Some(observe)` for declared, so a kind absent from
        // `observe` has already been dropped one filter above; and a kind
        // present in `observe` survives this one. The line is reachable only
        // on `strict`, where `kinds` is `None`. That is the same asymmetry the
        // heartbeat line has and it is why neither can flatter the gated
        // number.
        .filter(|(_, bound)| near(*bound, within))
        .map(|(o, _)| {
            let bucket = match timing {
                Timing::Ignore => serde_json::Value::Null,
                // Nearest multiple, not the floor. See [`Timing::Bucket`]:
                // the arrivals cluster on multiples of the delay being
                // measured, so a floor puts the boundary where the samples
                // are and the check flaps.
                Timing::Bucket { ms } => {
                    let ms = ms.max(1);
                    serde_json::json!((o.at_ms + ms / 2) / ms)
                }
            };
            // Two field-level exemptions, composed. Both are `Cow`, so an
            // observation that triggers neither is not cloned — which is the
            // common case by a long way, since `add_entity` is 4 kinds in a
            // corpus that records 37.
            let moved = drop_add_entity_movement(&o.kind, &o.fields);
            let fields = exempt_item_spawn_yaw(&o.kind, &moved);
            serde_json::json!({
                "who": o.who.name(),
                "step": o.step,
                "kind": o.kind,
                "fields": fields,
                "when": bucket,
            })
        })
        .collect();
    // Last, on the sequence that survived every filter. See
    // [`renumber_entities`] for why not earlier and not in `decode`.
    renumber_entities(&mut out);
    // Same argument, a different quantity: an identifier that is a function
    // of the session rather than of the scenario cannot be compared across
    // two independently recorded runs. See [`rebase_session_counters`].
    rebase_session_counters(&mut out);
    out
}

/// Drop a projected observation of a named kind when it repeats **the
/// previous observation of that same kind**, ignoring step and timing.
///
/// See [`crate::scenario::Scenario::collapse_repeats`] for what this is for
/// and what it costs. This is only the mechanism; the judgement is there.
///
/// # It was keyed on adjacency in the whole projection, and that is the defect
///
/// The old rule collapsed a run of identical **consecutive** observations of
/// the projection. Adjacency in the whole projection is a property of what
/// sits *between* the repeats — which is decided by which other kinds the
/// projection kept — so one declaration produced opposite behaviour in the two
/// denominators of the same comparison. The declared projection has already
/// deleted every kind outside `observe`, so the repeats end up adjacent and
/// collapse; the strict projection keeps `block_changed_ack`,
/// `container_set_slot` and `packet:0x60` between them, **no two repeats are
/// ever adjacent, and nothing collapses at all.**
///
/// A declaration whose effect depends on which *other* kinds happen to be in
/// the projection is not the declaration the check file says it is making.
///
/// # The key is adjacency within the named kind's own subsequence
///
/// Not "the first of each distinct value, for ever", which is the wider rule
/// `docs/PARITY-ROADMAP.md` §2 R1 proposed. The wider rule collapses
/// `open · shut · open` to `open · shut` and so stops distinguishing a
/// mechanism that resumed from one that never came back — real discriminating
/// power, given up for nothing, because no lid in this corpus alternates and
/// the wider rule collapses not one observation more on any trace on disk.
///
/// This one removes exactly the defect: the dependence on unrelated kinds.
/// `A A A B A A` is still `A B A`, as the old doc comment promised and as
/// `a_mechanism_that_resumes_is_not_a_repeat` pins — and now it is `A B A`
/// whatever sits between the members, in either denominator.
///
/// # `step` and `when` are outside the key, and outside the survivor
///
/// The kinds this is used for repeat on a free-running server timer —
/// `ContainerOpenersCounter.recheckOpeners` re-arms every 5 ticks, and
/// `ServerEntity.sendChanges` re-broadcasts on `tickCount % 60 == 0`. Which
/// step a given repeat lands in is therefore set by where the timer's edges
/// fall against the step boundaries, which is phase and not behaviour.
/// Measured on P2's `560`: vanilla's lid edges land at 61 · 260 · 478 · 728 ·
/// 978 · 1228 · 1494 · 1744 ms and ours at 20 · 220 · 470 · 720 · 970 · 1220 ·
/// 1470 · 1720, the step-1 action fires at ~283 ms, and the *only* consequence
/// is that vanilla's 260 is stamped step 1 while our 220 is stamped step 0 —
/// eight against eight, no packet missing.
///
/// So `step` and the timing bucket are dropped from the key **and blanked on
/// the survivor**. Keeping them in the key would collapse nothing across a
/// boundary and leave the per-step counts being compared, which is the whole
/// defect. Keeping them on the survivor would leave the *first* repeat's step
/// index being compared, which is the same coin with a smaller amplitude —
/// "less non-deterministic", which the old doc comment already admitted was
/// the wrong answer.
///
/// Everything else stays in the key: `who`, `kind` and every field. A lid at
/// the wrong position, with the wrong viewer count, on the wrong stream, or
/// for the wrong block is a different value and does not collapse into the
/// right one.
///
/// # It runs on the projection, and it runs last
///
/// After every filter and after [`renumber_entities`], for two reasons that
/// pull the same way. Equality has to be equality of *the thing being
/// compared* — an observation the `within` bound would have dropped must not
/// be able to mask a duplicate, and two observations that differ only in a
/// field the projection discards must collapse. And numbering before
/// collapsing means an entity index counts the entities the projection saw,
/// which is what it already means everywhere else; a duplicate refers to an
/// entity already numbered, so collapsing never renumbers anything.
fn collapse_repeats(projected: &mut Vec<serde_json::Value>, kinds: &BTreeSet<String>) {
    collapse_repeats_with_times(projected, kinds, None);
}

/// [`collapse_repeats`], keeping a parallel arrival-time list in step.
///
/// The times are not part of the projection and are never compared; they are
/// [`Projected::at_ms`], the evidence a boundary diagnosis rests on. They have
/// to be filtered by the *same* rule or they index against the wrong
/// observation — and thirteen checks in this corpus declare
/// `collapse_repeats`, so "close enough" here would mean a wrong millisecond
/// figure on exactly the container checks the boundary class is most likely to
/// turn up in. Hence one function with an optional second output rather than a
/// second implementation of the rule.
fn collapse_repeats_with_times(
    projected: &mut Vec<serde_json::Value>,
    kinds: &BTreeSet<String>,
    times: Option<&mut Vec<u64>>,
) {
    if kinds.is_empty() {
        return;
    }
    let mut previous: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut keep: Vec<bool> = Vec::with_capacity(projected.len());
    for o in projected.iter_mut() {
        let Some(kind) = o
            .get("kind")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            keep.push(true);
            continue;
        };
        if !kinds.contains(&kind) {
            keep.push(true);
            continue;
        }
        // Blanked on the observation itself and not only in a key, so the
        // thing compared and the thing keyed on can never disagree about what
        // "the same observation" means.
        if let Some(m) = o.as_object_mut() {
            m.insert("step".into(), serde_json::Value::Null);
            m.insert("when".into(), serde_json::Value::Null);
        }
        if previous.get(&kind) == Some(o) {
            keep.push(false);
            continue;
        }
        previous.insert(kind, o.clone());
        keep.push(true);
    }
    let mut i = 0;
    projected.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    if let Some(t) = times {
        // Only when the two lists were built by the same filters. A shorter
        // time list means the caller did not build one for this projection,
        // and silently zipping a mismatched pair is how a millisecond figure
        // ends up naming the wrong packet.
        if t.len() == keep.len() {
            let mut j = 0;
            t.retain(|_| {
                j += 1;
                keep[j - 1]
            });
        } else {
            t.clear();
        }
    }
}

/// Whether an observation happened close enough to the workspace to be this
/// scenario's business.
///
/// An observation with **no position at all** — an acknowledgement, a chat
/// line, a screen opening — has nothing to be far from and is always kept.
/// Only something that can be placed can be somewhere else.
///
/// It used to read the `at` field directly, and so treated "carries no
/// position" and "is about an entity somewhere else entirely" as the same
/// thing. [`place`] is what tells them apart now, and it is passed the answer
/// rather than working it out here, because placing an entity needs the whole
/// preceding trace and this only needs one number.
///
/// # What this bound cannot bound — audited 2026-08-07, and it is a list
///
/// `bound == None` admits, which is right for an acknowledgement and wrong for
/// anything the instrument merely failed to read. **R2 was one instance of the
/// second case and the audit that followed it is written down here rather than
/// left to be rediscovered.** Four remaining routes to "admitted because it
/// could not be parsed", in descending order of how much they matter:
///
/// 1. **A kind with no decoder arm is exempt by construction**, because it has
///    no `entity` and no `at`. After R2 the kinds still in that state in the
///    2026-08-07 02:19 corpus are `packet:0x8A` waypoint (440),
///    `packet:0x4A` recipe_book_add (39), `packet:0x00` bundle_delimiter (32),
///    `packet:0x60` set_cursor_item (13), `packet:0x13` container_set_data (4)
///    and `packet:0x6C` set_player_inventory (1) — and **not one of them can
///    ever be bounded**, because not one puts an entity id or a position on the
///    wire. `javap` on the pinned jar: `ClientboundTrackedWaypointPacket` is
///    `(Operation, TrackedWaypoint)` and `TrackedWaypoint.write` leads with an
///    `Either<UUID, String>`; `ClientboundRecipeBookAddPacket` is
///    `(List<Entry>, boolean)`; `ClientboundSetCursorItemPacket` is one
///    `ItemStack`; `ClientboundContainerSetDataPacket` is three ints of menu
///    state; `ClientboundSetPlayerInventoryPacket` is a slot and a stack;
///    bundle_delimiter has an empty body. So this is a **limit of the bound**,
///    not a missing decoder: `near` can only bound what says where it is.
///    Writing an arm for any of them buys readability, never bounding.
///    **Two of that list have arms since 2026-08-12** — `set_cursor_item` and
///    `set_player_inventory`, on branch `trace-itemstack-legible` — and the
///    sentence above is exactly why that changed nothing here: they still put
///    no entity id and no position on the wire, so they are still exempt from
///    this bound and the audit's conclusion stands unaltered. The list is a
///    dated reading of one corpus, not a live index.
/// 2. **[`place`] reads one field name; [`renumber_entities`] reads a shape.**
///    `place` looks at `fields["entity"]`, while the renumbering walks every
///    `{"$entity": n}` at any depth on purpose, so that a decoder arm added
///    later participates without being remembered. The two disagree today:
///    `take_item_entity` names its entities `item` and `taker`, so it is
///    renumbered and **not** bounded — vanilla sent 6 in that corpus, and one
///    scenario's pickup can therefore land in a later scenario's window.
///    `remove_entities` is the same shape and is deliberately exempt (see
///    [`place`]'s forgetting rule). Making `place` structural would change what
///    is compared and needs its own measurement; it is named, not fixed.
/// 3. **The two kind-keyed exemptions in [`project`] key on a decoded name.**
///    A `move_entity` or an `entity_position` whose arm ran off the end becomes
///    `packet:0x..` with `decode: "failed"` in its fields, and neither
///    exemption can see it. Zero instances in either trace pair on disk; the
///    marker is in the trace when it happens.
/// 4. **`informative` admits a `block_update` whose `at` will not read** — it
///    returns "informative", which is the direction that hides a dead check
///    rather than inventing a live one. Unreachable while the arm always
///    inserts `at`, because a failed decode also changes the kind.
///
/// And one route that is not a defect but has the same effect: a check may
/// declare `within = -1` and turn the bound off outright. Three do —
/// `580`, `581`, `582` — so those three see every leftover in the corpus by
/// their own choice, which is what the negative value is for.
fn near(bound: Option<[f64; 3]>, within: i32) -> bool {
    // **Negative is unbounded**, declared by a check whose observations carry
    // no position of their own — see [`crate::scenario::Scenario::within`]
    // for the measurement that made this necessary. It is deliberately not
    // the same thing as `bound == None`: that is the harness saying "nobody
    // ever told me where this is", and this is the check saying "there is
    // nothing here to bound".
    if within < 0 {
        return true;
    }
    let Some(rel) = bound else {
        return true;
    };
    let limit = f64::from(within);
    // Chebyshev, and horizontal only: `y` is bounded by the world, not by
    // how far the scenario walked, and a cascade straight up or down is
    // exactly the kind of thing a check wants to see.
    [0usize, 2].iter().all(|&i| rel[i].abs() <= limit)
}

fn kinds_of(obs: &[Obs], filter: Option<&BTreeSet<String>>) -> BTreeSet<String> {
    obs.iter()
        .filter(|o| filter.is_none_or(|k| k.contains(&o.kind)))
        .map(|o| o.kind.clone())
        .collect()
}

/// How many `block_update`s a step produces no matter what the world does.
///
/// `handleUseItemOn` ends by resyncing **two** positions by hand — the block
/// that was clicked and the block it would have placed into — unconditionally
/// on every path that reaches the end of the handler, whether or not anything
/// changed. `block_changed_ack` is likewise sent for every sequenced action,
/// and is acknowledged *before* the reach check that can silently drop the
/// interaction.
///
/// So a scenario built on `place` produces a non-empty vanilla trace **by
/// construction**, and `vacuous` — which only fires when the reference said
/// nothing at all — can no longer tell a live check from a dead one. This
/// function is what replaces it: it returns the positions a step would resync
/// even if the server did nothing, so [`informative`] can subtract them.
fn unconditional_positions(step: &crate::scenario::Step) -> Vec<[i32; 3]> {
    match step {
        crate::scenario::Step::Place { on, face, .. } => crate::scenario::face_offset(face)
            .map_or_else(Vec::new, |d| {
                vec![*on, [on[0] + d[0], on[1] + d[1], on[2] + d[2]]]
            }),
        _ => Vec::new(),
    }
}

/// How many observations in a projected trace could only have come from the
/// world actually changing.
///
/// Everything the scenario's own actions force is subtracted: the ack, and
/// one `block_update` at each position the step resyncs by hand. What is left
/// is the `ChunkHolder` broadcast — which `Level.setBlock` only reaches when
/// the state at that position really changed — plus anything unprompted.
fn informative(projected: &[serde_json::Value], steps: &[crate::scenario::Step]) -> usize {
    // One allowance per (step, position), consumed at most once.
    let mut allowance: Vec<Vec<[i32; 3]>> = steps.iter().map(unconditional_positions).collect();
    projected
        .iter()
        .filter(|o| {
            let kind = o
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            // DROP: informative.block_changed_ack — a receipt for the step, not a result of it
            if kind == "block_changed_ack" {
                return false;
            }
            if kind != "block_update" {
                return true;
            }
            let step = o
                .get("step")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as usize;
            let Some(at) = o
                .get("fields")
                .and_then(|f| f.get("at"))
                .and_then(|v| v.as_array())
            else {
                return true;
            };
            let pos: Vec<i64> = at.iter().filter_map(serde_json::Value::as_i64).collect();
            let Some(slot) = allowance.get_mut(step) else {
                return true;
            };
            match slot.iter().position(|p| {
                pos.len() == 3
                    && i64::from(p[0]) == pos[0]
                    && i64::from(p[1]) == pos[1]
                    && i64::from(p[2]) == pos[2]
            }) {
                Some(i) => {
                    // DROP: informative.block_update_allowance — the block the step was always going to place
                    slot.remove(i);
                    false
                }
                None => true,
            }
        })
        .count()
}

/// Which step is the measured one: the **last** `[[step]]`, or `None` when a
/// scenario has no measured steps at all.
///
/// # Why the last one and not every one
///
/// `[[setup]]` is unmeasured by construction — [`crate::scenario`] says so —
/// and among the `[[step]]`s the corpus's own grammar is that everything
/// before the last arranges the state the last one acts on. `607` is the
/// pattern in the open: place a chest, five clicks to get one stone into the
/// cursor and one into a slot, then the `pickup_all` the file is named for.
///
/// Requiring **every** step to speak was the alternative and it is wrong
/// rather than merely stricter: a scenario that opens a chest, clicks a slot
/// and then clicks it back has a middle step whose whole job is to arrange,
/// and reddening it would price arranging out of the corpus. Requiring the
/// last one to speak asks the narrow question that `607` failed, and asks it
/// of the step whose name is on the file.
///
/// **The residual is named rather than hidden**: an assertion made at a step
/// that is not the last one is not covered. `602-container-click-quick-move-
/// on-an-empty-slot-changes-nothing` is the corpus's example of a scenario
/// its own source describes as having two measured steps. Nothing here
/// catches a silent one of those, and a scenario that wants the guarantee can
/// have it by ending on the step it is about.
const fn measured_step_index(scenario: &Scenario) -> Option<usize> {
    scenario.step.len().checked_sub(1)
}

/// How many projected observations are attributed to `step`.
///
/// # Counted before `collapse_repeats`, which is not an implementation detail
///
/// [`collapse_repeats`] blanks `step` on the kinds a scenario names, on
/// purpose: where a free-running timer's edge falls against a step boundary
/// is a property of the harness's sampling and not of either server. A filter
/// whose stated job is to forget which step something landed in must not be
/// the thing that decides whether a step said anything — it would report a
/// step as silent precisely because the check had disclaimed the cadence of
/// what arrived there, and the disclaimer would have manufactured the
/// silence. So [`compare_with`] takes this reading off the raw projection and
/// collapses afterwards.
///
/// An observation carrying no `step` at all is therefore not this step's, and
/// is not counted. That is the same judgement in the other direction.
fn obs_at_step(projected: &[serde_json::Value], step: usize) -> usize {
    projected
        .iter()
        .filter(|o| o.get("step").and_then(serde_json::Value::as_u64) == Some(step as u64))
        .count()
}

/// The distinct kinds attributed to `step`, sorted, for [`Outcome::measured_reference_kinds`].
///
/// Sorted and de-duplicated rather than left in wire order: this is read by a
/// human deciding what a check rests on, and the order two runs happen to
/// produce is not information.
fn kinds_at_step(projected: &[serde_json::Value], step: usize) -> Vec<String> {
    let mut out: Vec<String> = projected
        .iter()
        .filter(|o| o.get("step").and_then(serde_json::Value::as_u64) == Some(step as u64))
        .filter_map(|o| o.get("kind").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn verdict_for(reference: &[serde_json::Value], subject: &[serde_json::Value]) -> Verdict {
    if reference.is_empty() {
        // **`Vacuous` is decided by the reference alone, which is the shape
        // of the defect found in `Inert` on 2026-08-03, and it had the same
        // hole in one direction.**
        //
        // The claim `vacuous` makes is "the scenario proves nothing about
        // anything and is a fault in the check, not in the server". When the
        // subject is *also* silent that is exactly right and is the whole
        // doctrine: a negative result is only evidence if the mechanism was
        // demonstrably live. When the subject **spoke**, it is false. The
        // subject emitting observations inside a window where the reference
        // emitted none is a real, reportable state of a real server — it is
        // the mirror of `Absent` — and it was being labelled a broken check.
        //
        // The hole is narrower than `Inert`'s was, and the difference is
        // worth stating rather than glossing. `Inert` was applied *after*
        // the verdict was computed and overwrote whatever it found, so it
        // could swallow a `Diverge` with both sides populated — the ordinary
        // case, the one that happens when a server is merely wrong. This
        // fires only when the reference side is empty, so it can never touch
        // a two-sided divergence, and it can never touch `Absent` at all
        // (that verdict requires a non-empty reference). Exactly one verdict
        // was reachable through it: the one-sided divergence in the
        // extra-output direction.
        //
        // `Diverge` is the right bucket and not a new one. It is already
        // where an extra-output difference lands the moment the reference
        // says anything at all, and it is baselineable, which an honest
        // description of an unfinished server has to be.
        //
        // **Measured inert on the corpus at `adb7cee` before it landed**: no
        // declared projection has an empty reference, and the single strict
        // `vacuous` — `a-standing-player-is-rebroadcast-on-a-timer` — has an
        // empty subject too, so it stays `vacuous`. This closes a hole
        // rather than moving a number, and the declared 27 is unchanged.
        return if subject.is_empty() {
            Verdict::Vacuous
        } else {
            Verdict::Diverge
        };
    }
    if reference == subject {
        return Verdict::Match;
    }
    if subject.is_empty() {
        return Verdict::Absent;
    }
    Verdict::Diverge
}

fn describe_first_difference(
    reference: &[serde_json::Value],
    subject: &[serde_json::Value],
) -> Option<String> {
    for i in 0..reference.len().max(subject.len()) {
        match (reference.get(i), subject.get(i)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(a), Some(b)) => {
                return Some(format!("at {i}: vanilla {a}, ours {b}"));
            }
            (Some(a), None) => return Some(format!("at {i}: vanilla {a}, ours said nothing more")),
            (None, Some(b)) => return Some(format!("at {i}: vanilla said nothing more, ours {b}")),
            (None, None) => break,
        }
    }
    None
}

/// **Recorded order, not `scenarios` order.** `scenarios` is a directory
/// listing; the entity table has to accumulate in the order the world
/// actually happened in, or a leftover would be placed by a scenario that
/// had not run yet. Each side of a comparison is tracked independently,
/// because the two servers pick their own raw entity ids and the bound is
/// about position.
fn scenario_starts(t: &Trace) -> BTreeMap<String, (Places, [i32; 3])> {
    let mut places = Places::new();
    let mut out = BTreeMap::new();
    for s in &t.scenarios {
        out.insert(s.name.clone(), (places.clone(), s.anchor));
        advance(&s.obs, s.anchor, &mut places);
    }
    out
}

/// Which artefacts of **our own sampling** a projection is allowed to see.
///
/// # This exists to tell two opposite defects apart, and it may never gate
///
/// A scenario that does not reproduce between two runs of the *reference* is
/// either a fact about vanilla — which is to be recorded — or a fact about
/// where this harness put a step boundary — which is a bug in the harness, to
/// be repaired. They read identically in a report and they have opposite
/// fixes, and three separate agents spent part of a run in one night
/// rediscovering that by hand.
///
/// The discriminator is mechanical. A projected observation carries `who`,
/// `step`, `kind`, `fields` and `when`. Of those, **`step` and `when` are
/// ours**: `step` is which of the scenario's own actions was in flight when
/// the packet arrived, and `when` is which bucket of our own timing rule it
/// fell in. Everything else is what vanilla said. So re-run the comparison
/// with those two blanked, and:
///
/// * still different — vanilla said different **things**. Record it.
/// * now identical — vanilla said the same things and we filed them under
///   different headings. Our boundary is inside vanilla's spread. Repair it.
///
/// # `Declared` is the only lens any gate may use
///
/// [`Lens::BoundaryBlind`] deletes real discriminating power: a check that
/// fires on the wrong step is a check that has caught something, and this
/// lens cannot see it. It is a **diagnostic** — the corpus analogue of
/// `ci/decorate-order-intersect.sh`, which decomposes a worldgen residual by
/// traversal order without ever excusing one. `ci/t2-gameplay.sh` does not
/// pass it and `ci/corpus-floor-falsify.sh` case 11 asserts that it does not.
/// The lens is written into every report it produces, so a consumer that
/// reads a report cannot fail to see which one made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lens {
    /// Everything the scenario declared, step attribution included.
    Declared,
    /// `step` and the timing bucket blanked on every projected observation.
    BoundaryBlind,
}

impl Lens {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::BoundaryBlind => "boundary-blind",
        }
    }

    /// Parse the CLI spelling. `None` for anything else, so a typo cannot
    /// silently select the diagnostic lens.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "declared" => Some(Self::Declared),
            "boundary-blind" => Some(Self::BoundaryBlind),
            _ => None,
        }
    }
}

/// Blank the fields [`Lens::BoundaryBlind`] hides.
///
/// Runs on the output of [`project`] and **before** [`collapse_repeats`],
/// which blanks the same two fields for the kinds a check named. Doing it in
/// the other order would leave collapse keying on a step index this lens has
/// already declared meaningless.
fn apply_lens(projected: &mut [serde_json::Value], lens: Lens) {
    if lens == Lens::Declared {
        return;
    }
    for o in projected.iter_mut() {
        if let Some(m) = o.as_object_mut() {
            m.insert("step".into(), serde_json::Value::Null);
            m.insert("when".into(), serde_json::Value::Null);
        }
    }
}

/// One scenario's projection of a **single** trace, under one lens.
///
/// # Why this is not a pairwise comparison
///
/// [`compare`] answers "do these two traces agree". The question a stability
/// measurement asks is "how many distinct things does the reference say", and
/// that is not recoverable from pairwise verdicts: N runs that disagree
/// pairwise everywhere might hold two answers or might hold N, and the two
/// cases justify very different entries in `ci/checks/gameplay-unstable.toml`.
/// `update_attributes` needed the second reading — four recordings, **three**
/// distinct payloads — and it was got by hand.
///
/// It also separates two things pairwise verdicts conflate. Two runs whose
/// projections are both **empty** compare as `vacuous`, which
/// `ci/selfcheck-aggregate.py` counts as unstable and its own header admits is
/// wrong: the reference agreed with itself perfectly and the check is dead.
/// Here that is one distinct projection and zero flaps, reported as a dead
/// check rather than an unstable one.
#[derive(Debug, Clone, Serialize)]
pub struct Projected {
    pub name: String,
    pub about: String,
    /// False when the trace has no scenario of this name — a recording that
    /// predates the check. Not an instability, and not adjudicable either.
    pub present: bool,
    /// The recorded per-scenario error, if the run failed there.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub declared: Vec<serde_json::Value>,
    pub strict: Vec<serde_json::Value>,
    /// `at_ms` of each **declared** observation, in the same order.
    ///
    /// Not part of the projection and never compared. It is the evidence for
    /// a boundary diagnosis: "this packet arrives at 322-383 ms across six
    /// runs and our step boundary is at 381" is a repair order, where "step 2
    /// in one run and step 3 in another" is only a symptom.
    pub at_ms: Vec<u64>,
}

/// Project every scenario of ONE trace, under one lens. See [`Projected`].
#[must_use]
pub fn project_trace(scenarios: &[Scenario], trace: &Trace, lens: Lens) -> Vec<Projected> {
    let by_name: BTreeMap<String, crate::trace::ScenarioTrace> = trace
        .scenarios
        .iter()
        .map(|s| (s.name.clone(), s.clone()))
        .collect();
    let starts = scenario_starts(trace);
    let empty = (Places::new(), [0i32; 3]);
    scenarios
        .iter()
        .map(|scenario| {
            let (places, anchor) = starts.get(&scenario.name).unwrap_or(&empty);
            let Some(t) = by_name.get(&scenario.name) else {
                return Projected {
                    name: scenario.name.clone(),
                    about: scenario.about.clone(),
                    present: false,
                    error: None,
                    declared: Vec::new(),
                    strict: Vec::new(),
                    at_ms: Vec::new(),
                };
            };
            let kinds: BTreeSet<String> = scenario.observe.iter().cloned().collect();
            let collapse: BTreeSet<String> = scenario.collapse_repeats.iter().cloned().collect();
            let mut declared = project(
                &t.obs,
                Some(&kinds),
                &kinds,
                scenario.timing,
                scenario.observe_from,
                scenario.within,
                *anchor,
                places,
            );
            // The arrival times of exactly the observations that survived the
            // declared projection, taken under the same filters so the two
            // lists index together. `project` does not carry `at_ms` because
            // nothing compares it; this re-derives it rather than widening the
            // projection, so no gated comparison can ever see the field.
            let mut timed = project_at_ms(
                &t.obs,
                Some(&kinds),
                &kinds,
                scenario.observe_from,
                scenario.within,
                *anchor,
                places,
            );
            let mut strict = project(
                &t.obs,
                None,
                &kinds,
                Timing::Ignore,
                scenario.observe_from,
                scenario.within,
                *anchor,
                places,
            );
            apply_lens(&mut declared, lens);
            apply_lens(&mut strict, lens);
            collapse_repeats_with_times(&mut declared, &collapse, Some(&mut timed));
            collapse_repeats(&mut strict, &collapse);
            Projected {
                name: scenario.name.clone(),
                about: scenario.about.clone(),
                present: true,
                error: t.error.clone(),
                declared,
                strict,
                at_ms: timed,
            }
        })
        .collect()
}

/// `at_ms` of the observations [`project`] keeps, under the same filters.
///
/// Duplicated rather than returned from `project` on purpose: adding the
/// field to a projected observation would put a wall-clock reading inside the
/// thing every gated comparison diffs, and no amount of care downstream would
/// make that safe.
fn project_at_ms(
    obs: &[Obs],
    kinds: Option<&BTreeSet<String>>,
    observed: &BTreeSet<String>,
    from: crate::scenario::From,
    within: i32,
    anchor: [i32; 3],
    places: &Places,
) -> Vec<u64> {
    use crate::trace::Who;
    let mut seen = places.clone();
    let bounds: Vec<Option<[f64; 3]>> = obs.iter().map(|o| place(o, anchor, &mut seen)).collect();
    obs.iter()
        .zip(bounds)
        .filter(|(o, _)| match from {
            crate::scenario::From::Both => true,
            crate::scenario::From::Actor => o.who == Who::Actor,
            crate::scenario::From::Observer => o.who == Who::Observer,
            crate::scenario::From::Probe => o.who == Who::Probe,
        })
        .filter(|(o, _)| kinds.is_none_or(|k| k.contains(&o.kind)))
        // The same one line [`project`] uses, for the same reason. It was
        // two, naming two kinds, and every kind added to the classification
        // had to be remembered here as well — which is the shape of defect
        // `ci/ambient-falsify.py` now refuses.
        // DROP: ambient_unless_observed — the same filter as `project`, on the time axis
        .filter(|(o, _)| !crate::trace::is_ambient_kind(&o.kind) || observed.contains(&o.kind))
        .filter(|(_, bound)| near(*bound, within))
        .map(|(o, _)| o.at_ms)
        .collect()
}

/// Compare a subject trace against a reference trace, scenario by scenario.
///
/// [`Lens::Declared`], always. Every gate in this tree calls this one.
#[must_use]
pub fn compare(scenarios: &[Scenario], reference: &Trace, subject: &Trace) -> Vec<Outcome> {
    compare_with(scenarios, reference, subject, Lens::Declared)
}

/// [`compare`], under a chosen [`Lens`]. Read that type before using it.
#[must_use]
pub fn compare_with(
    scenarios: &[Scenario],
    reference: &Trace,
    subject: &Trace,
    lens: Lens,
) -> Vec<Outcome> {
    let by_name = |t: &Trace| -> BTreeMap<String, crate::trace::ScenarioTrace> {
        t.scenarios
            .iter()
            .map(|s| (s.name.clone(), s.clone()))
            .collect()
    };
    let refs = by_name(reference);
    let subs = by_name(subject);

    let ref_start = scenario_starts(reference);
    let sub_start = scenario_starts(subject);
    let empty = (Places::new(), [0i32; 3]);

    scenarios
        .iter()
        .map(|scenario| {
            let (rplaces, ranchor) = ref_start.get(&scenario.name).unwrap_or(&empty);
            let (splaces, sanchor) = sub_start.get(&scenario.name).unwrap_or(&empty);
            let r = refs.get(&scenario.name);
            let s = subs.get(&scenario.name);
            let mut out = Outcome {
                name: scenario.name.clone(),
                about: scenario.about.clone(),
                verdict: Verdict::Error,
                strict: Verdict::Error,
                reference_obs: 0,
                subject_obs: 0,
                measured_step: measured_step_index(scenario),
                measured_reference_obs: 0,
                measured_subject_obs: 0,
                measured_reference_kinds: Vec::new(),
                missing_kinds: Vec::new(),
                extra_kinds: Vec::new(),
                first_difference: None,
                reference_projection: Vec::new(),
                subject_projection: Vec::new(),
                reference_strict_projection: Vec::new(),
                subject_strict_projection: Vec::new(),
            };
            let (Some(r), Some(s)) = (r, s) else {
                out.first_difference = Some("the scenario is missing from a trace".into());
                return out;
            };
            if let Some(e) = r.error.as_ref().or(s.error.as_ref()) {
                out.first_difference = Some(format!("the run failed: {e}"));
                return out;
            }

            let kinds: BTreeSet<String> = scenario.observe.iter().cloned().collect();
            let mut pr = project(
                &r.obs,
                Some(&kinds),
                &kinds,
                scenario.timing,
                scenario.observe_from,
                scenario.within,
                *ranchor,
                rplaces,
            );
            let mut ps = project(
                &s.obs,
                Some(&kinds),
                &kinds,
                scenario.timing,
                scenario.observe_from,
                scenario.within,
                *sanchor,
                splaces,
            );
            // The measured step's share of each side, read here rather than
            // below because the two lines that follow deliberately destroy
            // step attribution. See [`obs_at_step`].
            if let Some(m) = out.measured_step {
                out.measured_reference_obs = obs_at_step(&pr, m);
                out.measured_subject_obs = obs_at_step(&ps, m);
                out.measured_reference_kinds = kinds_at_step(&pr, m);
            }
            // **Both denominators, and both sides identically.**
            //
            // This used to say "declared only", on the argument that
            // `collapse_repeats` is a statement about what the check is about,
            // which is the same category as `observe`, and that `strict`
            // ignores that category on purpose. The argument is wrong in the
            // same way the heartbeat exception's was, and for the same reason.
            //
            // `strict` ignoring `observe` means it does not *narrow* to the
            // kinds a check named. It has never meant that a check's
            // statements about the instrument do not apply there — `within`
            // and `observe_from` are exactly such statements and strict honours
            // both. `collapse_repeats` says "this kind arrives on a
            // free-running server timer whose phase against my window is not a
            // property of either server". That is true in the strict
            // projection too, and it is *only* there that it does any work:
            // the lid repeats are not in any container check's `observe` list,
            // so before this line the declaration was written in a check file,
            // validated at load, and could not reach the denominator it was
            // needed in.
            //
            // The cost is stated where the feature is, and it is not made
            // larger by running here: what stops being compared is the repeat
            // count and the step attribution of the named kinds, in both
            // columns rather than one.
            let collapse: BTreeSet<String> = scenario.collapse_repeats.iter().cloned().collect();
            // Before `collapse_repeats`, and a no-op under `Lens::Declared`.
            // See [`Lens`].
            apply_lens(&mut pr, lens);
            apply_lens(&mut ps, lens);
            collapse_repeats(&mut pr, &collapse);
            collapse_repeats(&mut ps, &collapse);
            out.reference_obs = pr.len();
            out.subject_obs = ps.len();
            out.verdict = verdict_for(&pr, &ps);
            // A check has to be able to fail. `vacuous` catches a reference
            // that said nothing; this catches a reference that said only
            // what the scenario's own actions forced it to say, which since
            // the resync rule landed is every placement check by
            // construction. See `informative`.
            //
            // **Only a `Match` may be relabelled, and that is the whole of the
            // rule.** `informative` reads `pr` — the *reference* projection —
            // and nothing else, so before 2026-08-03 this line overwrote the
            // verdict whatever it was, including `Diverge`. That is not
            // cosmetic: the forced resync at a clicked position carries the
            // **actual block state there**, so a server that placed the wrong
            // block produces a resync whose payload differs from vanilla's,
            // and that is a real divergence about a real defect. Relabelling
            // it `inert` reads as "this check is dead" and sends the reader to
            // rewrite the check instead of the server.
            //
            // Restricting it to `Match` also states what `inert` means more
            // exactly than the old form did. The claim is *this check cannot
            // fail*; a check that just failed has refuted that claim by
            // demonstration, whatever the reference said on its own. And
            // nothing is lost by waiting: if the divergence is later fixed the
            // scenario becomes `Match`, this line fires, and the gate goes red
            // at the moment the check actually stops discriminating.
            //
            // `Absent` is kept for the same reason: a subject that produced
            // nothing where the reference produced even a forced resync has
            // failed to send the ack and the resyncs, which is a real and
            // reportable state of the server.
            //
            // The tell was on screen the whole time and nobody had reason to
            // look at it. `strict` below never applies this override, so the
            // constructed case printed `1 inert` on the declared line and
            // `1 diverge` on the strict line **of the same run**. The two
            // denominators exist to expose deferred verification; they turn
            // out to expose this as well, and a declared/strict pair that
            // disagrees in that direction — inert here, diverge there — is
            // worth reading as a signal rather than as noise.
            if out.verdict == Verdict::Match && informative(&pr, &scenario.step) == 0 {
                out.verdict = Verdict::Inert;
            }
            // The third verdict. See [`Verdict::Mute`] for what it claims and
            // [`crate::scenario::Scenario::measured_step`] for why the
            // exception is declared in the check rather than derived here.
            //
            // Placed after `Inert` and guarded on `Match` for the reasons
            // that type documents: a scenario `Inert` already caught is
            // making the stronger statement, and a scenario that did not
            // match has refuted the claim "this check cannot fail" by
            // demonstration.
            // `measured_step.is_some()` is not belt-and-braces: a scenario
            // with no `[[step]]` has zero observations at a step that does not
            // exist, which would satisfy the two counters below and fire on a
            // question with no subject. `Scenario::load` refuses such a file
            // outright, so this guard is what makes that refusal the *only*
            // route — a hand-built `Scenario` in a test cannot get a `Mute`
            // the corpus could never produce.
            if out.verdict == Verdict::Match
                && out.measured_step.is_some()
                && scenario.measured_step == crate::scenario::MeasuredStep::Speaks
                && out.measured_reference_obs == 0
                && out.measured_subject_obs == 0
            {
                out.verdict = Verdict::Mute;
            }
            out.first_difference = describe_first_difference(&pr, &ps);
            // A mute scenario has no first difference — the two sides agreed
            // — so the line a reader gets is `first_difference`'s fallback,
            // "vanilla itself produced nothing to compare against", which is
            // true of the *scenario* only for `Vacuous`. Say the real thing.
            if out.verdict == Verdict::Mute {
                out.first_difference = Some(format!(
                    "step {} is the measured step and contributed 0 of the {} declared \
                     observation(s) on the reference side and 0 of the {} on ours — the check \
                     is green whatever the server does there. Either the step provokes nothing \
                     in vanilla, or its `observe` list cannot see what it provokes. If the \
                     silence IS the assertion, say `measured_step = \"silent\"`.",
                    out.measured_step.map_or(-1, |m| m as i64),
                    out.reference_obs,
                    out.subject_obs,
                ));
            }

            let rk = kinds_of(&r.obs, Some(&kinds));
            let sk = kinds_of(&s.obs, Some(&kinds));
            out.missing_kinds = rk.difference(&sk).cloned().collect();
            out.extra_kinds = sk.difference(&rk).cloned().collect();

            // Strict: the scenario's own bounds, but **no `observe` filter**
            // — so it counts the packet kinds nobody has written a decoder
            // for, whose digests make them exact by construction. That gap
            // is the point of the number: it is how much of what the two
            // servers said is not being looked at.
            //
            // # It used to drop the bounds too, and that made it noise
            //
            // This passed `From::Both` and `i32::MAX` as well, so every
            // check's strict verdict covered the whole session everywhere in
            // the world — every other workspace, every entity, both clients.
            //
            // **`ci/t2-gameplay-selfcheck.sh` measured what that costs the
            // moment it was first run: vanilla scores 3/37 on strict against
            // itself.** A denominator its own reference fails 92% of the
            // time is measuring the reference's nondeterminism, not the
            // subject, and the three names that survived it —
            // `sand-falls-when-unsupported`, `server-answers-a-command`,
            // `digging-a-door-takes-both-halves` — have nothing in common
            // except that nothing random happened near them.
            //
            // `docs/SERVER.md` published the declared/strict gap as "the
            // honest measure of how much verification has been deferred".
            // With an 8% ceiling on the reference it was not that; it was
            // mostly a measure of how much of the world each check happened
            // to be able to see.
            //
            // Keeping `within` and `observe_from` restores the intent. What
            // strict now says is: inside the box this scenario is about, and
            // on the client it is about, did the two servers say the same
            // bytes — including the bytes no decoder reads. Timing is still
            // ignored, and that remains the one thing strict does not cover.
            //
            // **That fixed the scope and did not make the number usable, and
            // the correction is dated 2026-08-04.** The 43% the bounds bought
            // was 16 of 37 checks matching strict in *one* comparison at
            // N = 2. Asked whether a check reproduces under strict in all
            // eleven comparisons of a twelve-run selfcheck, vanilla scores
            // **0 of 55** against itself — the least unstable nine at exactly
            // 5 of 11, which is a coin flip, because
            // `ServerEntity.sendChanges` re-broadcasts every tracked entity
            // on `tickCount % 60 == 0` regardless of movement and no window
            // in this corpus is as long as three seconds. So `strict` is a
            // diagnostic, not a score: it is worth computing, worth reporting
            // beside `declared`, and worth nothing quoted on its own.
            //
            // **The dominant cause was removed the same day and the number is
            // now 35 of 60.** The heartbeat filter in [`project`] drops a
            // `move_entity` that failed vanilla's own movement test, so the
            // coin flip that sat under every check at once is gone. `strict`
            // is still a diagnostic and still not gated — 35 of 60 is not a
            // rate to build a gate on — but it is no longer pinned at zero by
            // one packet, and movement in it is now worth reading.
            //
            // What remained read as `set_entity_motion` failing in 10
            // scenarios and `rotate_head` in 6, both entity tracking, plus a
            // sound *pitch* drawn from `level.getRandom()` that no
            // implementation can match.
            //
            // **The entity-tracking half was not a mechanism at all.** It was
            // [`near`] being unable to bound an observation that carries no
            // position, so one scenario's dropped item sat in every later
            // scenario's window until it despawned. [`place`] bounds them and
            // quantity A moved **35 -> 37 of 60**. A registered floor of 42
            // was missed, so it is a real cause and not the dominant one, and
            // no floor is restated here. Measured with
            // `ci/selfcheck-kind-stability.py`; see `docs/WORKLIST.md`.
            //
            // **Still never a parity claim.** This is vanilla against itself.
            let mut sr = project(
                &r.obs,
                None,
                &kinds,
                Timing::Ignore,
                scenario.observe_from,
                scenario.within,
                *ranchor,
                rplaces,
            );
            let mut ss = project(
                &s.obs,
                None,
                &kinds,
                Timing::Ignore,
                scenario.observe_from,
                scenario.within,
                *sanchor,
                splaces,
            );
            apply_lens(&mut sr, lens);
            apply_lens(&mut ss, lens);
            collapse_repeats(&mut sr, &collapse);
            collapse_repeats(&mut ss, &collapse);
            out.strict = verdict_for(&sr, &ss);
            out.reference_projection = pr;
            out.subject_projection = ps;
            out.reference_strict_projection = sr;
            out.subject_strict_projection = ss;
            out
        })
        .collect()
}

/// The counts a corpus run publishes.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Counts {
    pub total: usize,
    pub matched: usize,
    pub diverged: usize,
    pub absent: usize,
    pub vacuous: usize,
    /// See [`Verdict::Mute`]. Counted separately from `inert` because the two
    /// send a reader to different places: `inert` says the whole window was
    /// forced, `mute` says the window was fine and the assertion was not in
    /// it.
    pub mute: usize,
    pub inert: usize,
    pub errored: usize,
}

impl Counts {
    #[must_use]
    pub fn of(outcomes: &[Outcome], strict: bool) -> Self {
        let mut c = Self {
            total: outcomes.len(),
            ..Self::default()
        };
        for o in outcomes {
            match if strict { o.strict } else { o.verdict } {
                Verdict::Match => c.matched += 1,
                Verdict::Diverge => c.diverged += 1,
                Verdict::Absent => c.absent += 1,
                Verdict::Vacuous => c.vacuous += 1,
                Verdict::Mute => c.mute += 1,
                Verdict::Inert => c.inert += 1,
                Verdict::Error => c.errored += 1,
            }
        }
        c
    }
}

/// The accepted state of the corpus, committed to the repo.
///
/// A ratchet, not a pass list. Most of a young corpus is legitimately
/// [`Verdict::Absent`] — the checks for mechanics nobody has built yet are
/// exactly the ones worth writing early, because they are the worklist — so
/// a gate that demanded `Match` everywhere would mean nothing could land.
/// What the gate refuses is a scenario going **backwards**, and a scenario
/// that is not in the baseline at all: a new check has to say what it
/// currently does, which is the moment somebody reads it.
#[derive(Debug, Default, Deserialize)]
pub struct Baseline {
    #[serde(default)]
    pub expect: BTreeMap<String, String>,
}

impl Baseline {
    /// # Errors
    /// If the file is unreadable or not a baseline.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))
    }

    /// Which scenarios are worse than the baseline allows, and which are
    /// better than it records.
    #[must_use]
    pub fn check(&self, outcomes: &[Outcome]) -> (Vec<String>, Vec<String>) {
        let (mut worse, mut better) = (Vec::new(), Vec::new());
        for o in outcomes {
            let Some(want) = self.expect.get(&o.name) else {
                worse.push(format!(
                    "{}: not in the baseline — a new check must record what it currently does",
                    o.name
                ));
                continue;
            };
            let want = match want.as_str() {
                "match" => Verdict::Match,
                "diverge" => Verdict::Diverge,
                "absent" => Verdict::Absent,
                other => {
                    worse.push(format!(
                        "{}: baseline says {other:?}, which is not a verdict a check may rest at",
                        o.name
                    ));
                    continue;
                }
            };
            if o.verdict < want {
                worse.push(format!(
                    "{}: was {}, is now {} — {}",
                    o.name,
                    want.name(),
                    o.verdict.name(),
                    o.first_difference.as_deref().unwrap_or("no detail")
                ));
            } else if o.verdict > want {
                better.push(format!(
                    "{}: was {}, is now {}",
                    o.name,
                    want.name(),
                    o.verdict.name()
                ));
            }
        }
        (worse, better)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `measured_reference_kinds` is what 550's report could not say.
    ///
    /// The shape reconstructed here is the one measured on the 17 Aug pair:
    /// four kinds declared, three of which vanilla never emits, and one
    /// observation at the measured step. `measured_reference_obs = 1` is what
    /// the report has always carried and it does not distinguish "one sound"
    /// from "one damage event" — which is the whole of the difference between
    /// a check about a noise and a check about damage.
    ///
    /// It also pins the two properties a reader depends on: the list is
    /// de-duplicated, so three sounds at one step read as `["sound"]` and not
    /// as evidence of variety, and it is sorted, so two runs that produced the
    /// same kinds in a different order produce the same list.
    #[test]
    fn the_measured_kinds_say_what_the_count_cannot() {
        let at = |step: u64, kind: &str| serde_json::json!({ "step": step, "kind": kind });
        let projected = vec![
            at(0, "open_screen"),
            at(1, "sound"),
            at(1, "sound"),
            at(1, "block_update"),
            at(2, "block_changed_ack"),
        ];
        assert_eq!(kinds_at_step(&projected, 1), vec!["block_update", "sound"]);
        assert_eq!(kinds_at_step(&projected, 0), vec!["open_screen"]);
        // A step nothing was attributed to is an empty list, not a panic: it
        // is the reading `561`, `571`, `581` and `608` all have, and all four
        // are "changes nothing" claims for which an empty measured step is the
        // claim rather than a hole in it.
        assert!(kinds_at_step(&projected, 9).is_empty());
    }

    /// **The hole a fixed source list leaves, closed by a census.**
    ///
    /// [`PROJECTION_SOURCES`] names four files. A branch that puts a new
    /// comparator axis in a *fifth* would change the projection without
    /// changing its identity, and the floor stamped with that identity would
    /// go on certifying a comparator that no longer existed — which is the
    /// exact defect the identity was added to stop, one level out.
    ///
    /// **It has now caught one, which is the only evidence a guard like this
    /// can offer.** `pitch.rs` arrived on `corpus-sound-pitch` as a new module
    /// deciding whether a recorded pitch is compared exactly, compared as a
    /// range, or dropped — three different comparisons of one recording — and
    /// was declared in neither list. Every test in the crate passed except
    /// this one.
    ///
    /// Nothing in the binary can enumerate its own modules, so this reads the
    /// source directory at test time and requires every `.rs` in it to be
    /// declared as part of the projection or as part of the recorder. Adding a
    /// module is then a decision somebody makes in a diff, not a silence.
    #[test]
    fn every_oracle_source_is_declared_in_or_out_of_the_projection() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut found: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
            .map(|e| e.expect("read_dir entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .map(|p| {
                p.file_name()
                    .expect("file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        found.sort();
        let mut declared: Vec<String> = PROJECTION_SOURCES
            .iter()
            .map(|(n, _)| (*n).to_owned())
            .chain(
                crate::record::RECORDER_SOURCES
                    .iter()
                    .map(|(n, _)| (*n).to_owned()),
            )
            .collect();
        declared.sort();
        assert_eq!(
            found, declared,
            "a source in tools/oracle-runner/src is in neither PROJECTION_SOURCES nor \
             RECORDER_SOURCES. If it can change what a comparison looks at, add it to the \
             first — every standing floor then has to be re-measured, which is the point. \
             If it cannot, add it to the second and say why in the diff."
        );
    }

    /// The identity is a digest of text and would be perfectly stable if that
    /// text were empty, so the two ways it can be vacuous are pinned: a
    /// non-empty body for every declared source, and an identity that actually
    /// depends on all three of them.
    #[test]
    fn the_projection_identity_is_not_a_constant() {
        let id = projection_identity();
        assert!(id.starts_with("proj:"), "{id}");
        assert_eq!(id.len(), "proj:".len() + 16, "{id}");
        let mut bodies = Vec::new();
        for (name, src) in PROJECTION_SOURCES {
            let body = projection_body(name, src);
            assert!(body.len() > 1000, "{name}: kept only {} bytes", body.len());
            assert!(
                !body.contains("\n//"),
                "{name}: a comment line survived the strip"
            );
            bodies.push(body);
        }
        // Three different files, three different bodies: a list that named the
        // same file three times would digest something, pass every other
        // assertion here, and be blind to two thirds of the projection.
        bodies.sort();
        bodies.dedup();
        assert_eq!(bodies.len(), PROJECTION_SOURCES.len());
        // Perturbing any one source moves the identity. Done on a copy of the
        // payload rather than on disk, which is why it is spelled out here
        // instead of being left to `ci/corpus-floor-falsify.sh`.
        let base = crate::trace::digest(bodies.join("\n").as_bytes());
        for i in 0..bodies.len() {
            let mut mutated = bodies.clone();
            mutated[i].push_str("\nlet _ = 1;");
            assert_ne!(base, crate::trace::digest(mutated.join("\n").as_bytes()));
        }
    }

    /// **No projection source may name a recorder source, and this is the
    /// test that keeps it that way.**
    ///
    /// `RECORDER_SOURCES` lived in this file until 2026-08-09 and the coupling
    /// was invisible: declaring a recorder module edited a projection source,
    /// so `proj:` moved and every standing floor went stale for a change that
    /// altered nothing about what a comparison looks at. The list moved to
    /// `record.rs`; without a test, the next person to want the recorder's
    /// filenames in here would put them back.
    ///
    /// It reads the *stripped* text, because a mention in a comment is a
    /// mention nobody's identity depends on — and this file's own header
    /// discusses the recorder at length.
    #[test]
    fn no_projection_source_embeds_a_recorder_source() {
        for (pname, psrc) in PROJECTION_SOURCES {
            let body = projection_body(pname, psrc);
            for (rname, _) in crate::record::RECORDER_SOURCES {
                assert!(
                    !body.contains(&format!("include_str!(\"{rname}\")")),
                    "{pname} embeds {rname}: the projection identity would then move on \
                     every recorder change, which is the coupling removed on 2026-08-09"
                );
            }
        }
    }

    /// Every test written before entity positions existed asked the same
    /// question — "what does this projection contain?" — and none of them is
    /// about where an entity is. This shadows [`super::project`] with an
    /// anchor at the origin and an empty table, which is exactly the state
    /// the first scenario of any run starts in, so those tests keep asserting
    /// what they were written to assert. The tests that *are* about placement
    /// call `super::project` by name.
    fn project(
        obs: &[Obs],
        kinds: Option<&BTreeSet<String>>,
        timing: Timing,
        from: crate::scenario::From,
        within: i32,
    ) -> Vec<serde_json::Value> {
        // `observed` is the scenario's `observe` list, which for a declared
        // projection **is** `kinds`. A strict projection passes `None` for
        // `kinds` and still carries the list, so it is threaded separately
        // here too rather than defaulted to empty — defaulting it would make
        // every test below unable to see a heartbeat under strict, which is
        // the exact bug the split exists to fix.
        let observed: BTreeSet<String> = kinds.cloned().unwrap_or_default();
        super::project(
            obs,
            kinds,
            &observed,
            timing,
            from,
            within,
            [0, 0, 0],
            &Places::new(),
        )
    }

    /// Strict projections pass `kinds = None`, and a check that names the
    /// heartbeat must still see it there.
    ///
    /// This is `520-a-standing-player-is-rebroadcast-on-a-timer`'s whole
    /// problem in one assertion. Its `observe` list is exactly
    /// `["move_entity_heartbeat"]`, so before the `observed` argument existed
    /// the strict projection of *both* sides came back empty and the check was
    /// `vacuous` — not because either server was wrong, but because the
    /// instrument deleted the only kind it was about. It was strict-`vacuous`
    /// in twelve consecutive gate runs and could not have been anything else.
    ///
    /// The negative half is the guard: a check that does **not** name the
    /// heartbeat still never sees one under strict either, which is what keeps
    /// the coin-flip cadence of `ServerEntity.sendChanges` out of every other
    /// scenario's strict verdict.
    #[test]
    fn a_check_that_names_the_heartbeat_sees_it_under_strict_too() {
        let obs = vec![
            obs(crate::trace::HEARTBEAT, 0),
            obs("move_entity", 0),
            obs("block_update", 0),
        ];
        let names_it: BTreeSet<String> =
            [crate::trace::HEARTBEAT.to_string()].into_iter().collect();
        let does_not: BTreeSet<String> = ["block_update".to_string()].into_iter().collect();
        let strict = |observed: &BTreeSet<String>| -> Vec<String> {
            super::project(
                &obs,
                None,
                observed,
                Timing::Ignore,
                crate::scenario::From::Both,
                -1,
                [0, 0, 0],
                &Places::new(),
            )
            .iter()
            .map(|o| o["kind"].as_str().unwrap_or_default().to_string())
            .collect()
        };
        assert_eq!(
            strict(&names_it),
            vec![crate::trace::HEARTBEAT, "move_entity", "block_update"],
            "520's strict projection is no longer empty"
        );
        assert_eq!(
            strict(&does_not),
            vec!["move_entity", "block_update"],
            "and every other scenario's strict verdict is unchanged"
        );
    }

    /// The same shape, for `entity_position`, and the same guard.
    ///
    /// A stray `ClientboundEntityPositionSyncPacket` is emitted when the
    /// client's own `onGround` bit flips, and the reference and the subject
    /// are recorded in two separate client sessions — so it appears on
    /// whichever side the coin fell (3 on vanilla's, 2 on ours, over 23
    /// windows of P2) and never on both. The exception is what stops the rule
    /// from deleting the mechanism outright: **540 and 080 name the kind and
    /// therefore still see every one of them.**
    ///
    /// The negative half is load-bearing in the other direction too: the
    /// projection must keep `move_entity` for a check that named neither, or
    /// the rule has been widened past what the measurement supports.
    #[test]
    fn a_check_that_names_the_position_resync_sees_it_under_strict_too() {
        let obs = vec![
            obs(crate::trace::ENTITY_POSITION, 0),
            obs("move_entity", 0),
            obs("block_update", 0),
        ];
        let names_it: BTreeSet<String> = [crate::trace::ENTITY_POSITION.to_string()]
            .into_iter()
            .collect();
        let does_not: BTreeSet<String> = ["block_update".to_string()].into_iter().collect();
        let strict = |observed: &BTreeSet<String>| -> Vec<String> {
            super::project(
                &obs,
                None,
                observed,
                Timing::Ignore,
                crate::scenario::From::Both,
                -1,
                [0, 0, 0],
                &Places::new(),
            )
            .iter()
            .map(|o| o["kind"].as_str().unwrap_or_default().to_string())
            .collect()
        };
        assert_eq!(
            strict(&names_it),
            vec![crate::trace::ENTITY_POSITION, "move_entity", "block_update"],
            "540 and 080 are the positive controls and must still see the packet"
        );
        assert_eq!(
            strict(&does_not),
            vec!["move_entity", "block_update"],
            "every other scenario stops being scored on a bit the client sent — \
             and `move_entity` stays, because widening the rule to it would \
             leave no control at all"
        );
    }

    /// The declared denominator cannot move under either exception.
    ///
    /// Both filters are reachable only when `kinds` is `None`. For a declared
    /// projection a kind outside `observe` is already gone one filter earlier,
    /// and a kind inside it takes the exception — so neither line can flatter
    /// the number the gate ratchets on. Asserted rather than argued, because
    /// "it cannot affect the gated column" is exactly the kind of claim that
    /// stops being true when somebody reorders the filters.
    #[test]
    fn neither_projection_exception_can_move_the_declared_denominator() {
        let obs = vec![
            obs(crate::trace::HEARTBEAT, 0),
            obs(crate::trace::ENTITY_POSITION, 0),
            obs("block_update", 0),
        ];
        let declared = |names: &[&str]| -> Vec<String> {
            let kinds: BTreeSet<String> = names.iter().map(|s| (*s).to_string()).collect();
            project(
                &obs,
                Some(&kinds),
                Timing::Ignore,
                crate::scenario::From::Both,
                -1,
            )
            .iter()
            .map(|o| o["kind"].as_str().unwrap_or_default().to_string())
            .collect()
        };
        assert_eq!(
            declared(&["block_update"]),
            vec!["block_update"],
            "a scenario that names neither never saw them declared either"
        );
        assert_eq!(
            declared(&[
                crate::trace::HEARTBEAT,
                crate::trace::ENTITY_POSITION,
                "block_update"
            ]),
            vec![
                crate::trace::HEARTBEAT,
                crate::trace::ENTITY_POSITION,
                "block_update"
            ],
            "and a scenario that names both still gets both"
        );
    }

    // --- the third verdict ------------------------------------------------
    //
    // The tests below are the poison. `607` passed for its whole life by
    // comparing two silences at the step it was named for, and a verdict
    // asserting that cannot happen is worth exactly as much as the
    // demonstration that it fires — so each of these makes a measured step go
    // silent and reads the verdict back.

    /// A scenario of `n` dig steps. `Dig` on purpose: it has no
    /// [`unconditional_positions`], so nothing here can be quietly rescued or
    /// condemned by `Inert` while a `Mute` test thinks it is measuring `Mute`.
    fn dig_scenario(
        name: &str,
        n: usize,
        measured_step: crate::scenario::MeasuredStep,
    ) -> Scenario {
        let mut s = one_kind_scenario(name);
        s.measured_step = measured_step;
        s.step = (0..n)
            .map(|i| crate::scenario::Step::Dig {
                at: [1, -1, i as i32],
                face: "up".into(),
            })
            .collect();
        s
    }

    /// One scenario's trace from a list of `(kind, step)` pairs.
    fn trace_of(name: &str, obss: &[(&str, usize)]) -> Trace {
        let mut t = one_step_trace(name, 0, 0, "same");
        t.scenarios[0].obs = obss
            .iter()
            .map(|(k, st)| {
                let mut o = obs(k, *st);
                o.fields
                    .insert("rest".into(), serde_json::Value::String("same".into()));
                o
            })
            .collect();
        t
    }

    /// **`607`, reduced to its arithmetic.**
    ///
    /// A scenario with three steps whose reference and subject both said
    /// plenty — and said all of it while the rig was being built. Step 2 is
    /// the assertion, both sides contributed nothing to it, and the sequences
    /// are equal, so every verdict that existed before this one calls it
    /// `Match`: `Vacuous` sees a non-empty reference, `Inert` sees two
    /// observations a `Dig` does not force.
    ///
    /// If this ever returns to `Match` the corpus is back to being able to
    /// pass by comparing two silences.
    #[test]
    fn a_measured_step_that_says_nothing_on_both_sides_is_mute() {
        let scenarios = vec![dig_scenario("s", 3, crate::scenario::MeasuredStep::Speaks)];
        let live = [("block_update", 0), ("block_update", 1)];
        let out = compare(&scenarios, &trace_of("s", &live), &trace_of("s", &live));
        assert_eq!(out[0].verdict, Verdict::Mute);
        assert_eq!(out[0].measured_step, Some(2));
        assert_eq!(out[0].reference_obs, 2, "the window as a whole spoke");
        assert_eq!(
            out[0].measured_reference_obs, 0,
            "and the assertion did not"
        );
        assert_eq!(out[0].measured_subject_obs, 0);
        assert!(
            !out[0].verdict.baselineable(),
            "a check that cannot fail may never be locked in"
        );
        assert!(
            Verdict::Mute < Verdict::Match,
            "and must read as a regression"
        );

        // The same run with one observation moved onto the measured step is
        // the positive control: nothing else about the fixture changes.
        let spoke = [("block_update", 0), ("block_update", 2)];
        let out = compare(&scenarios, &trace_of("s", &spoke), &trace_of("s", &spoke));
        assert_eq!(out[0].verdict, Verdict::Match);
        assert_eq!(out[0].measured_reference_obs, 1);
    }

    /// The four by-design negative checks, in one assertion: a scenario whose
    /// silence **is** its assertion declares that and is compared exactly as
    /// before.
    ///
    /// The second half is the half that matters. A declaration that merely
    /// excused the scenario would have made `561`, `571`, `581` and `608`
    /// unfailable — the disease, applied deliberately — so the test asserts
    /// that a subject which speaks at the silent step still diverges.
    #[test]
    fn a_declared_silent_measured_step_is_not_mute_and_can_still_fail() {
        let scenarios = vec![dig_scenario("s", 3, crate::scenario::MeasuredStep::Silent)];
        let silent = [("block_update", 0), ("block_update", 1)];
        let out = compare(&scenarios, &trace_of("s", &silent), &trace_of("s", &silent));
        assert_eq!(
            out[0].verdict,
            Verdict::Match,
            "the silence is the assertion"
        );
        assert_eq!(out[0].measured_reference_obs, 0);

        let noisy = [
            ("block_update", 0),
            ("block_update", 1),
            ("block_update", 2),
        ];
        let out = compare(&scenarios, &trace_of("s", &silent), &trace_of("s", &noisy));
        assert_eq!(
            out[0].verdict,
            Verdict::Diverge,
            "a server that answered where vanilla stayed quiet is what this check is for"
        );
    }

    /// **Absent means "must speak".** The whole safety argument for putting
    /// the exception in the scenario rests on this: a check that forgets the
    /// field goes red, so an omission can never manufacture a pass.
    #[test]
    fn the_default_is_that_a_measured_step_must_speak() {
        assert_eq!(
            crate::scenario::MeasuredStep::default(),
            crate::scenario::MeasuredStep::Speaks
        );
        let s: Scenario = toml::from_str(
            "name = \"x\"\nabout = \"y\"\nobserve = [\"block_update\"]\n\
             [[step]]\ndo = \"dig\"\nat = [1, -1, 0]\nface = \"up\"\n",
        )
        .expect("a scenario that says nothing about its measured step must still load");
        assert_eq!(s.measured_step, crate::scenario::MeasuredStep::Speaks);
    }

    /// `Mute` may only relabel a `Match`, which is the rule `Inert` was
    /// corrected into on 2026-08-03 after it was found swallowing a real
    /// divergence.
    ///
    /// Here the two sides disagree *before* the measured step and both are
    /// silent at it. The scenario is broken and the server is also wrong, and
    /// the verdict a reader needs is the one about the server.
    #[test]
    fn mute_never_swallows_a_divergence_elsewhere_in_the_window() {
        let scenarios = vec![dig_scenario("s", 3, crate::scenario::MeasuredStep::Speaks)];
        let mut theirs = trace_of("s", &[("block_update", 0), ("block_update", 1)]);
        theirs.scenarios[0].obs[1]
            .fields
            .insert("rest".into(), serde_json::Value::String("different".into()));
        let out = compare(
            &scenarios,
            &trace_of("s", &[("block_update", 0), ("block_update", 1)]),
            &theirs,
        );
        assert_eq!(out[0].verdict, Verdict::Diverge);
        assert_eq!(
            out[0].measured_reference_obs, 0,
            "the measured step really was silent — this is not a fixture that dodged the test"
        );
    }

    /// A one-sided silence at the measured step is a `Diverge` and not a
    /// `Mute`, for the reason [`verdict_for`] gives about `Vacuous`: a
    /// subject that spoke where the reference did not has demonstrated that
    /// the projection can see the step, which refutes what `Mute` claims.
    #[test]
    fn a_subject_that_speaks_at_a_silent_measured_step_is_a_divergence() {
        let scenarios = vec![dig_scenario("s", 3, crate::scenario::MeasuredStep::Speaks)];
        let out = compare(
            &scenarios,
            &trace_of("s", &[("block_update", 0)]),
            &trace_of("s", &[("block_update", 0), ("block_update", 2)]),
        );
        assert_eq!(out[0].verdict, Verdict::Diverge);
        assert_eq!(out[0].measured_reference_obs, 0);
        assert_eq!(out[0].measured_subject_obs, 1);
    }

    /// Only the **last** step is the measured one. A silent step in the
    /// middle is a rig being arranged, and reddening it would price arranging
    /// out of the corpus — `607` itself spends five of its seven steps doing
    /// exactly that.
    #[test]
    fn an_earlier_silent_step_is_not_the_measured_one() {
        let scenarios = vec![dig_scenario("s", 3, crate::scenario::MeasuredStep::Speaks)];
        let obss = [("block_update", 0), ("block_update", 2)];
        let out = compare(&scenarios, &trace_of("s", &obss), &trace_of("s", &obss));
        assert_eq!(
            out[0].verdict,
            Verdict::Match,
            "step 1 said nothing and may"
        );
    }

    /// **`collapse_repeats` must not be able to manufacture a `Mute`.**
    ///
    /// It blanks `step` on the kinds a check names, on purpose: where a
    /// free-running timer's edge falls against a step boundary is a property
    /// of this harness's sampling. Counting the measured step *after* that
    /// would report a step as silent precisely because the check had
    /// disclaimed the cadence of what arrived there — the disclaimer would
    /// have created the silence. So [`compare_with`] reads the counts off the
    /// raw projection and collapses afterwards, and this is the assertion
    /// that pins the order.
    ///
    /// Every container check in the corpus collapses `block_event`, so the
    /// day one of them adds it to `observe` this is what stands between that
    /// edit and a corpus-wide false `mute`.
    #[test]
    fn a_collapsed_kind_still_counts_towards_its_measured_step() {
        let mut scenario = dig_scenario("s", 2, crate::scenario::MeasuredStep::Speaks);
        scenario.observe = vec!["block_update".into()];
        scenario.collapse_repeats = vec!["block_update".into()];
        let scenarios = vec![scenario];
        let obss = [("block_update", 1), ("block_update", 1)];
        let out = compare(&scenarios, &trace_of("s", &obss), &trace_of("s", &obss));
        assert_eq!(
            out[0].reference_obs, 1,
            "the collapse ran, so this fixture is testing what it says it is"
        );
        assert_eq!(
            out[0].measured_reference_obs, 2,
            "and the measured step was counted before it did"
        );
        assert_eq!(out[0].verdict, Verdict::Match);
    }

    /// A trace of one scenario whose only observation sits in `step`.
    fn one_step_trace(name: &str, step: usize, at_ms: u64, rest: &str) -> Trace {
        let mut o = obs("block_update", step);
        o.at_ms = at_ms;
        o.fields
            .insert("rest".into(), serde_json::Value::String(rest.into()));
        Trace {
            target: "t".into(),
            version: "26.2".into(),
            protocol: 776,
            recorder: "test".into(),
            scenarios: vec![crate::trace::ScenarioTrace {
                name: name.into(),
                error: None,
                anchor: [0, 0, 0],
                spawn: [0.0, 0.0, 0.0],
                declared_commands: Vec::new(),
                obs: vec![o],
                fresh_session: None,
            }],
        }
    }

    fn one_kind_scenario(name: &str) -> Scenario {
        Scenario {
            name: name.into(),
            about: "constructed for the lens tests".into(),
            observe: vec!["block_update".into()],
            timing: Timing::Ignore,
            observe_from: crate::scenario::From::Both,
            settle_ms: 750,
            within: 16,
            collapse_repeats: Vec::new(),
            measured_step: crate::scenario::MeasuredStep::Speaks,
            gamerule: Vec::new(),
            console_restore: Vec::new(),
            setup: Vec::new(),
            step: Vec::new(),
        }
    }

    /// **The whole claim of `Lens::BoundaryBlind`, in one assertion.**
    ///
    /// Two runs of the reference that said the byte-identical thing and had it
    /// filed under different steps must `Diverge` under the gated lens — the
    /// step is real information and no gate may lose it — and `Match` under
    /// the diagnostic one, which is what identifies the disagreement as the
    /// harness's rather than vanilla's.
    ///
    /// If this ever passes in both columns the discriminator has stopped
    /// discriminating and every `boundary` verdict in
    /// `ci/checks/gameplay-floor.toml` is worthless.
    #[test]
    fn the_boundary_lens_sees_through_a_moved_step_and_the_declared_one_does_not() {
        let scenarios = vec![one_kind_scenario("moved-step")];
        let a = one_step_trace("moved-step", 1, 380, "same");
        let b = one_step_trace("moved-step", 2, 381, "same");

        let declared = compare_with(&scenarios, &a, &b, Lens::Declared);
        assert_eq!(declared[0].verdict, Verdict::Diverge);

        let blind = compare_with(&scenarios, &a, &b, Lens::BoundaryBlind);
        assert_eq!(blind[0].verdict, Verdict::Match);

        // And the plain entry point is the gated lens, always.
        assert_eq!(
            compare(&scenarios, &a, &b)[0].verdict,
            declared[0].verdict,
            "compare() must be compare_with(.., Lens::Declared)"
        );
    }

    /// The negative half, and it is the one that keeps the lens honest: a
    /// disagreement about what vanilla *said* survives blanking our own two
    /// fields. Without this the previous test would pass for a lens that
    /// simply deleted the whole projection.
    #[test]
    fn the_boundary_lens_does_not_see_through_a_changed_field() {
        let scenarios = vec![one_kind_scenario("moved-field")];
        let a = one_step_trace("moved-field", 1, 380, "aaaa");
        let b = one_step_trace("moved-field", 1, 380, "bbbb");
        assert_eq!(
            compare_with(&scenarios, &a, &b, Lens::Declared)[0].verdict,
            Verdict::Diverge
        );
        assert_eq!(
            compare_with(&scenarios, &a, &b, Lens::BoundaryBlind)[0].verdict,
            Verdict::Diverge
        );
    }

    /// `project_trace` carries the arrival times of exactly the observations
    /// it projected, and never inside them.
    ///
    /// The second half is the load-bearing one: `at_ms` is wall clock, and a
    /// wall clock inside the thing every gate diffs would make every scenario
    /// unstable by construction.
    #[test]
    fn a_projection_carries_its_arrival_times_beside_it_and_never_inside_it() {
        let scenarios = vec![one_kind_scenario("timed")];
        let t = one_step_trace("timed", 1, 1234, "same");
        let p = project_trace(&scenarios, &t, Lens::Declared);
        assert!(p[0].present);
        assert_eq!(p[0].declared.len(), 1);
        assert_eq!(p[0].at_ms, vec![1234]);
        assert!(
            !serde_json::to_string(&p[0].declared)
                .unwrap()
                .contains("1234"),
            "at_ms must not appear inside a projected observation"
        );
        // And the diagnostic lens blanks exactly the two fields it claims to.
        let b = project_trace(&scenarios, &t, Lens::BoundaryBlind);
        assert_eq!(b[0].declared[0]["step"], serde_json::Value::Null);
        assert_eq!(b[0].declared[0]["when"], serde_json::Value::Null);
        assert_eq!(b[0].declared[0]["kind"], "block_update");
    }

    /// A scenario the trace does not contain is `present = false`, not an
    /// empty projection.
    ///
    /// `ci/corpus-floor.py` excludes those from the floor's denominator, and
    /// the two states have opposite meanings: an empty projection is a
    /// reference that said nothing, which is evidence, and an absent scenario
    /// is a recording that predates the check, which is not.
    #[test]
    fn a_scenario_the_recording_predates_is_absent_and_not_empty() {
        let scenarios = vec![one_kind_scenario("here"), one_kind_scenario("not-here")];
        let t = one_step_trace("here", 0, 10, "same");
        let p = project_trace(&scenarios, &t, Lens::Declared);
        assert!(p[0].present);
        assert!(!p[1].present);
        assert!(p[1].declared.is_empty());
    }

    /// A mistyped lens is refused rather than defaulted. A silent fallback to
    /// `Declared` would produce a boundary-blind measurement labelled
    /// declared, and the label travelling with the number is the whole point.
    #[test]
    fn an_unknown_lens_name_is_refused() {
        assert_eq!(Lens::parse("declared"), Some(Lens::Declared));
        assert_eq!(Lens::parse("boundary-blind"), Some(Lens::BoundaryBlind));
        assert_eq!(Lens::parse("boundary_blind"), None);
        assert_eq!(Lens::parse("blind"), None);
        assert_eq!(Lens::parse(""), None);
    }

    fn obs(kind: &str, step: usize) -> Obs {
        Obs {
            who: crate::trace::Who::Observer,
            step,
            at_ms: 0,
            kind: kind.into(),
            fields: BTreeMap::new(),
        }
    }

    fn at(kind: &str, x: i32, y: i32, z: i32) -> Obs {
        let mut o = obs(kind, 0);
        o.fields.insert("at".into(), serde_json::json!([x, y, z]));
        o
    }

    /// One packet naming one entity by its raw server id.
    fn ent(kind: &str, raw: i32) -> Obs {
        let mut o = obs(kind, 0);
        o.fields
            .insert("entity".into(), crate::trace::entity_ref(raw));
        o
    }

    /// One `add_entity` as [`crate::trace`] now decodes it, for a given
    /// entity type and set of rotations.
    fn spawn(kind_id: i32, x_rot: i32, y_rot: i32) -> Obs {
        let mut o = obs("add_entity", 0);
        o.fields
            .insert("entity".into(), crate::trace::entity_ref(9));
        o.fields
            .insert("kind_id".into(), serde_json::json!(kind_id));
        o.fields
            .insert("at".into(), serde_json::json!([0.0, 1.32, 0.0]));
        o.fields.insert("x_rot".into(), serde_json::json!(x_rot));
        o.fields.insert("y_rot".into(), serde_json::json!(y_rot));
        o.fields.insert("y_head_rot".into(), serde_json::json!(0));
        o.fields.insert("data".into(), serde_json::json!(0));
        o.fields
            .insert("rest".into(), serde_json::json!("cbf29ce484222325"));
        o
    }

    /// **`movement` is recorded and is not compared, for every entity type.**
    ///
    /// This is the half of `trace::tests::an_add_entitys_movement_is_...` that
    /// moved here on 2026-08-18, and it is asserted in three directions
    /// because an equality alone passes for every wrong reason:
    ///
    /// * two spawns differing **only** in `movement` project equal — the
    ///   drop happens at all;
    /// * the projected observation carries **no `movement` key** — it is
    ///   removed rather than zeroed, so nothing downstream can read a zero
    ///   and believe it;
    /// * two spawns differing in `movement` **and** in `x_rot` project
    ///   different — the drop is one FIELD wide;
    /// * and, since 2026-08-18, that it is exactly two TYPES wide: two
    ///   `minecraft:falling_block` spawns differing only in `movement`
    ///   project **different**, because the probe measured that type constant
    ///   at `[0.0, 0.0, 0.0]` in all six runs. That assertion is the whole of
    ///   the narrowing and is the one that fails if somebody widens the
    ///   exemption back to "all entities" for convenience.
    #[test]
    fn an_add_entity_movement_is_dropped_by_the_projection() {
        let item = item_entity_type_id();
        // `minecraft:falling_block`, as `only_an_items_spawn_yaw_is_exempt`
        // reads it, so the two tests cannot disagree about the id.
        let falling = 51;
        assert_ne!(item, falling);
        let p = |o: Obs| project(&[o], None, Timing::Ignore, crate::scenario::From::Both, 16);
        let mv = |kind_id: i32, m: [f64; 3], x_rot: i32| {
            let mut o = spawn(kind_id, x_rot, 0);
            o.fields.insert("movement".into(), serde_json::json!(m));
            o
        };
        let player = entity_type_id("minecraft:player");
        for kind_id in [item, player] {
            let still = mv(kind_id, [0.0, 0.0, 0.0], 0);
            let moving = mv(kind_id, [0.5, -0.25, 0.125], 0);
            assert_eq!(
                p(still.clone()),
                p(moving.clone()),
                "movement must not reach the comparison for kind_id {kind_id}",
            );
            assert!(
                p(still.clone())[0]["fields"].get("movement").is_none(),
                "movement must be absent, not zeroed, for kind_id {kind_id}",
            );
            // Not vacuous: the same two bodies with a rotation between them
            // still differ, so the drop has not eaten the packet.
            assert_ne!(
                p(still),
                p(mv(kind_id, [0.5, -0.25, 0.125], 3)),
                "a rotation must still be compared for kind_id {kind_id}",
            );
        }
        // AND THE OTHER DIRECTION, which is the narrowing. Measured constant
        // at `[0.0, 0.0, 0.0]` over six cold vanilla servers, so it is
        // compared, and a server inventing a velocity for a falling block
        // fails here rather than passing silently.
        assert_ne!(
            p(mv(falling, [0.0, 0.0, 0.0], 0)),
            p(mv(falling, [0.5, -0.25, 0.125], 0)),
            "a falling block's movement is compared — it is constant zero in \
             all six probe runs and the exemption is two types wide",
        );
        assert!(
            p(mv(falling, [0.0, 0.0, 0.0], 0))[0]["fields"]
                .get("movement")
                .is_some(),
            "and it survives into the projection rather than being removed",
        );
        // A recording made before the decoder named the field has no
        // `movement` key and is passed through untouched — the same honest
        // behaviour `exempt_item_spawn_yaw` has, and the reason a floor taken
        // over an older ensemble is a different instrument rather than a
        // broken one.
        let old = spawn(item, 0, 0);
        assert!(!old.fields.contains_key("movement"));
        assert_eq!(
            drop_add_entity_movement(&old.kind, &old.fields).as_ref(),
            &old.fields,
            "a recording from before the field was named is untouched"
        );
    }

    /// **`minecraft:item`'s id is read from the generated table, not written
    /// down**, and the table this tree generated says 71.
    ///
    /// The number is asserted as well as the lookup, because a loader that
    /// silently answered `0` would make the exemption below fire on
    /// `minecraft:acacia_boat` and on nothing else, and every test that only
    /// checked "it resolved" would still pass. 71 is what
    /// `ci/dump-entity-types.sh` produced for 26.2 and what
    /// `docs/PARITY-ROADMAP.md` §11's recordings show on the wire for a
    /// dropped item; the day it changes, this fails and the exemption is
    /// still correct — which is the whole reason it is read.
    #[test]
    fn the_item_entity_type_id_comes_from_the_generated_table() {
        assert_eq!(
            item_entity_type_id(),
            71,
            "data/vanilla/entity_types.json disagrees with the recordings in \
             docs/PARITY-ROADMAP.md §11"
        );
    }

    /// **An item's spawn yaw is exempt; nothing else about it is, and no
    /// other entity type is.**
    ///
    /// Four cases, and the last three are what make the first one honest:
    ///
    /// * two item spawns differing only in `y_rot` project **equal** — the
    ///   `nextFloat() * 360` from `ItemEntity`'s unseeded generator;
    /// * two item spawns differing in `x_rot` project **different** — the
    ///   exemption is one field wide, not "the rotations";
    /// * two `falling_block` spawns differing in `y_rot` project
    ///   **different** — `070-sand-falls-when-unsupported`'s entity, which
    ///   §11 measured byte-identical across three vanilla recordings and
    ///   which must keep every field it had;
    /// * an observation with no `y_rot` at all — a recording made before the
    ///   decoder named the rotations — is passed through untouched, which is
    ///   the only honest answer available: the yaw is inside a digest there
    ///   and no projection can reach it.
    #[test]
    fn only_an_items_spawn_yaw_is_exempt() {
        let item = item_entity_type_id();
        // `minecraft:falling_block`, and read the same way so this test does
        // not become the place a hardcoded id hides.
        let falling = 51;
        assert_ne!(item, falling);

        let p = |o: Obs| project(&[o], None, Timing::Ignore, crate::scenario::From::Both, 16);

        assert_eq!(
            p(spawn(item, 0, 0)),
            p(spawn(item, 0, 91)),
            "an item's yaw is a draw from an unseeded RandomSource and is dropped"
        );
        assert_ne!(
            p(spawn(item, 0, 0)),
            p(spawn(item, 44, 0)),
            "and xRot is not — the exemption is one field wide"
        );
        assert_ne!(
            p(spawn(falling, 0, 0)),
            p(spawn(falling, 0, 91)),
            "070's falling block keeps its yaw"
        );

        // The pre-naming recording: `rest` holds the four bytes and there is
        // no `y_rot` to remove.
        let mut old = spawn(item, 0, 0);
        old.fields.remove("x_rot");
        old.fields.remove("y_rot");
        old.fields.remove("y_head_rot");
        old.fields.remove("data");
        assert_eq!(
            exempt_item_spawn_yaw(&old.kind, &old.fields).as_ref(),
            &old.fields,
            "a recording from before the rotations were named is untouched"
        );
    }

    fn entity_field(v: &serde_json::Value) -> &serde_json::Value {
        &v["fields"]["entity"]
    }

    /// **The exemption, and the clause that keeps it falsifiable.**
    ///
    /// A `move_entity` that failed vanilla's own movement test is dropped
    /// from every projection — including `strict`, which is the one that
    /// passes `None` and therefore looks at everything else. That is the
    /// whole fix for the 3 s re-broadcast.
    ///
    /// The third case is the condition on it. If the drop were
    /// unconditional, a server that never sent the heartbeat at all would be
    /// indistinguishable from vanilla under every check in the corpus, and
    /// nothing would be able to say so. Naming the kind in `observe` buys the
    /// packet back, which is what
    /// `520-a-standing-player-is-rebroadcast-on-a-timer` does and why the two
    /// do not ship separately.
    #[test]
    fn the_timer_broadcast_is_invisible_unless_a_check_names_it() {
        let run = vec![
            ent(crate::trace::HEARTBEAT, 77),
            ent("move_entity", 77),
            ent(crate::trace::HEARTBEAT, 77),
        ];
        let kinds = |k: &str| -> BTreeSet<String> { [k.to_string()].into_iter().collect() };
        let go = |k: Option<&BTreeSet<String>>| {
            project(&run, k, Timing::Ignore, crate::scenario::From::Both, 16)
                .iter()
                .map(|o| o["kind"].as_str().unwrap_or_default().to_string())
                .collect::<Vec<_>>()
        };

        // `strict` passes no kind filter and still must not see it.
        assert_eq!(go(None), ["move_entity"]);
        // A check that names `move_entity` gets real movement and nothing
        // that only a counter produced.
        assert_eq!(go(Some(&kinds("move_entity"))), ["move_entity"]);
        // A check that asks for the heartbeat by name gets both of them, and
        // can therefore fail when a server sends neither.
        assert_eq!(
            go(Some(&kinds(crate::trace::HEARTBEAT))),
            [crate::trace::HEARTBEAT, crate::trace::HEARTBEAT]
        );
    }

    /// **The defect this whole change exists for.**
    ///
    /// A check that observes `add_entity` and nothing else must not have its
    /// entity numbering moved by a `move_entity` it never asked about. This
    /// is `sand-falls-when-unsupported`'s failure written down as an
    /// assertion: two runs of the *same* server, differing only in whether an
    /// unobserved packet for another entity arrived first, used to project to
    /// `entity: 0` and `entity: 1` and to be reported as a divergence.
    #[test]
    fn an_unobserved_entity_cannot_shift_an_observed_index() {
        let kinds: BTreeSet<String> = ["add_entity".to_string()].into_iter().collect();
        // Run A: the falling block, alone.
        let a = vec![ent("add_entity", 4231)];
        // Run B: the same falling block, with a stray broadcast about the
        // other player in front of it. Same server, same mechanic.
        let b = vec![ent("move_entity", 77), ent("add_entity", 4231)];

        let pa = project(
            &a,
            Some(&kinds),
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
        );
        let pb = project(
            &b,
            Some(&kinds),
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
        );
        assert_eq!(pa.len(), 1);
        assert_eq!(pb.len(), 1);
        assert_eq!(entity_field(&pa[0]), &serde_json::json!(0));
        assert_eq!(
            entity_field(&pb[0]),
            &serde_json::json!(0),
            "an unobserved packet must not consume an index"
        );
        assert_eq!(pa, pb, "and so the two runs must compare equal");
    }

    /// The property the numbering exists for, and it has to survive the move.
    ///
    /// `ServerLevel.ENTITY_COUNTER` is static and its ids are reused, so two
    /// servers never agree on a raw id. What they must agree on is the order.
    #[test]
    fn two_servers_with_different_raw_ids_agree_after_projection() {
        let p = |ids: [i32; 3]| {
            project(
                &[
                    ent("add_entity", ids[0]),
                    ent("add_entity", ids[1]),
                    ent("move_entity", ids[2]),
                ],
                None,
                Timing::Ignore,
                crate::scenario::From::Both,
                16,
            )
        };
        // Same three sightings, and the third names the first entity again.
        let vanilla = p([4231, 99, 4231]);
        let ours = p([7, 8, 7]);
        assert_eq!(vanilla, ours);
        assert_eq!(entity_field(&vanilla[0]), &serde_json::json!(0));
        assert_eq!(entity_field(&vanilla[1]), &serde_json::json!(1));
        assert_eq!(entity_field(&vanilla[2]), &serde_json::json!(0));
    }

    /// **`declared` and `strict` number over their own scopes, separately.**
    ///
    /// Not a nicety: this is what stops a packet only `strict` looks at from
    /// moving a number `declared` compares, which is the whole failure. The
    /// same trace projects the observed entity to 0 under the check's own
    /// kinds and to 1 under strict, and both are right for their scope.
    #[test]
    fn the_two_denominators_number_independently() {
        let kinds: BTreeSet<String> = ["add_entity".to_string()].into_iter().collect();
        let trace = vec![ent("move_entity", 77), ent("add_entity", 4231)];
        let declared = project(
            &trace,
            Some(&kinds),
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
        );
        let strict = project(
            &trace,
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
        );
        assert_eq!(entity_field(&declared[0]), &serde_json::json!(0));
        assert_eq!(entity_field(&strict[0]), &serde_json::json!(0));
        assert_eq!(entity_field(&strict[1]), &serde_json::json!(1));
    }

    /// `remove_entities` carries a set, and a set has to be canonicalised —
    /// but by the numbers that get compared, not by the raw ids underneath.
    #[test]
    fn a_removed_entity_set_is_sorted_by_index_not_by_raw_id() {
        let mut o = obs("remove_entities", 0);
        // Seen in the order 9, 4 — so 9 is index 0 and 4 is index 1 — and
        // sent in an order the server's own collection chose.
        o.fields.insert(
            "entities".into(),
            serde_json::json!([crate::trace::entity_ref(9), crate::trace::entity_ref(4)]),
        );
        let p = project(&[o], None, Timing::Ignore, crate::scenario::From::Both, 16);
        assert_eq!(p[0]["fields"]["entities"], serde_json::json!([0, 1]));
    }

    /// A scenario is about its workspace. The neighbouring one is 32 blocks
    /// away and is somebody else's check.
    #[test]
    fn the_projection_is_bounded_to_the_workspace() {
        let all = vec![
            at("block_update", 1, 0, 0),   // the rig
            at("block_update", 0, -40, 0), // straight down: still ours
            at("block_update", 21, -1, 9), // another workspace
            obs("block_changed_ack", 0),   // no position at all
        ];
        let kept = project(&all, None, Timing::Ignore, crate::scenario::From::Both, 16);
        assert_eq!(kept.len(), 3, "only the far one is dropped");
        assert_eq!(
            project(
                &all,
                None,
                Timing::Ignore,
                crate::scenario::From::Both,
                i32::MAX
            )
            .len(),
            4,
            "strict keeps everything, which is what makes the gap honest"
        );
    }

    /// A reference trace that contains only what the scenario's own actions
    /// force cannot tell a working server from a dead one.
    ///
    /// This is the exact shape `water-spreads` had when it went
    /// `absent` -> `match` for the wrong reason: three observations on the
    /// reference side, every one of them unconditional. `vacuous` could not
    /// see it, because the reference was not empty.
    #[test]
    fn a_reference_that_only_echoed_the_scenario_is_inert() {
        let steps = vec![crate::scenario::Step::Place {
            on: [1, -1, 0],
            face: "up".into(),
            cursor: None,
        }];
        let unconditional = vec![
            at("block_update", 1, -1, 0), // resync of the clicked block
            at("block_update", 1, 0, 0),  // resync of the block placed into
            obs("block_changed_ack", 0),
        ];
        let projected = project(
            &unconditional,
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        assert_eq!(
            informative(&projected, &steps),
            0,
            "nothing here could fail"
        );

        // The `ChunkHolder` broadcast is the one that could not have been
        // sent if the block had not really changed, so one of it is enough.
        let mut live = unconditional;
        live.insert(2, at("block_update", 1, 0, 0));
        let projected = project(
            &live,
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        assert_eq!(informative(&projected, &steps), 1);
    }

    /// A dig resyncs nothing, so every `block_update` it produces is a
    /// broadcast and the check is live on its own.
    #[test]
    fn a_dig_is_live_because_it_has_no_unconditional_resyncs() {
        let steps = vec![crate::scenario::Step::Dig {
            at: [1, -1, 0],
            face: "up".into(),
        }];
        let projected = project(
            &[at("block_update", 1, -1, 0), obs("block_changed_ack", 0)],
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        assert_eq!(informative(&projected, &steps), 1);
    }

    /// **`inert` must not be able to swallow a real `Diverge`.**
    ///
    /// The trap, and it was live until 2026-08-03: `informative()` reads the
    /// *reference* projection and nothing else, and the line that used it
    /// overwrote the verdict whatever it was. So a scenario whose vanilla
    /// trace happens to hold nothing but the two forced `handleUseItemOn`
    /// resyncs had **every** verdict rewritten to `inert`, including the one
    /// that says this server is wrong.
    ///
    /// That is not a hypothetical shape. A resync at a forced position carries
    /// the actual block state at that position, so a server that placed the
    /// wrong block — or the right block in the wrong state — produces a resync
    /// whose payload differs while the position, the kind and the count all
    /// match. Everything the old rule looked at was identical.
    ///
    /// The two servers here differ *only* in the state carried by the resync
    /// of the block that was placed into, which is exactly that case.
    #[test]
    fn a_wrong_block_in_a_forced_resync_is_a_diverge_not_inert() {
        fn block_update(x: i32, y: i32, z: i32, state: &str) -> Obs {
            let mut o = at("block_update", x, y, z);
            o.fields.insert("state".into(), serde_json::json!(state));
            o
        }
        // Vanilla: the ack, and the two positions `handleUseItemOn` resyncs by
        // hand. Nothing else — so the reference is uninformative.
        let vanilla = vec![
            block_update(1, -1, 0, "minecraft:stone"),
            block_update(1, 0, 0, "minecraft:torch[facing=up]"),
            obs("block_changed_ack", 0),
        ];
        // Ours: same kinds, same positions, same count, same order. One state
        // is wrong.
        let ours = vec![
            block_update(1, -1, 0, "minecraft:stone"),
            block_update(1, 0, 0, "minecraft:air"),
            obs("block_changed_ack", 0),
        ];

        let scenario = Scenario {
            name: "a-torch-goes-where-it-was-put".into(),
            about: "deliberately constructed: a reference that only resyncs".into(),
            observe: vec!["block_update".into(), "block_changed_ack".into()],
            timing: Timing::Ignore,
            observe_from: crate::scenario::From::Both,
            settle_ms: 750,
            within: 16,
            collapse_repeats: Vec::new(),
            measured_step: crate::scenario::MeasuredStep::Speaks,
            gamerule: Vec::new(),
            console_restore: Vec::new(),
            setup: Vec::new(),
            step: vec![crate::scenario::Step::Place {
                on: [1, -1, 0],
                face: "up".into(),
                cursor: None,
            }],
        };

        // The premise of the test: the reference really is uninformative, so
        // the old rule really would have fired here.
        let kinds: BTreeSet<String> = scenario.observe.iter().cloned().collect();
        let pr = project(
            &vanilla,
            Some(&kinds),
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
        );
        assert_eq!(
            informative(&pr, &scenario.step),
            0,
            "if this is not 0 the test has stopped exercising the trap"
        );

        let trace = |obs: Vec<Obs>| Trace {
            target: "test".into(),
            version: "26.2".into(),
            protocol: 776,
            recorder: "test".into(),
            scenarios: vec![crate::trace::ScenarioTrace {
                name: scenario.name.clone(),
                error: None,
                anchor: [0, 0, 0],
                spawn: [0.0, 0.0, 0.0],
                declared_commands: Vec::new(),
                obs,
                fresh_session: None,
            }],
        };
        let out = compare(
            std::slice::from_ref(&scenario),
            &trace(vanilla.clone()),
            &trace(ours),
        );
        assert_eq!(
            out[0].verdict,
            Verdict::Diverge,
            "a wrong block state in a forced resync is a defect, not a dead check"
        );

        // And the label is still earned when the two servers agree: an
        // uninformative reference plus a matching subject is a check that
        // genuinely could not have failed.
        let out = compare(
            std::slice::from_ref(&scenario),
            &trace(vanilla.clone()),
            &trace(vanilla),
        );
        assert_eq!(out[0].verdict, Verdict::Inert);
    }

    /// **A timing bucket rounds to the nearest multiple, and the four numbers
    /// below are why.**
    ///
    /// 956, 972, 986 and 1002 ms are the four measured arrivals of a stone
    /// button's un-press (Threadripper, 2026-08-03, two selfcheck runs, actor
    /// and observer streams). The nominal delay is `ticksToStayPressed = 20`,
    /// so 1000 ms — and the samples straddle it. Under a floor bucket they
    /// split, and the check that is supposed to measure a 500 ms difference
    /// fails on a 46 ms one.
    #[test]
    fn a_timing_bucket_does_not_split_the_delay_it_is_measuring() {
        let arrivals = |ms: u64, samples: &[u64]| -> Vec<serde_json::Value> {
            samples
                .iter()
                .map(|&t| {
                    let mut o = obs("block_update", 0);
                    o.at_ms = t;
                    project(
                        std::slice::from_ref(&o),
                        None,
                        Timing::Bucket { ms },
                        crate::scenario::From::Both,
                        i32::MAX,
                    )[0]["when"]
                        .clone()
                })
                .collect()
        };
        let stone = arrivals(500, &[956, 972, 986, 1002]);
        assert!(
            stone.iter().all(|b| *b == stone[0]),
            "the four measured stone-button arrivals must be one bucket: {stone:?}"
        );
        // And a wooden button, 30 ticks, lands in the next one — which is the
        // difference the check exists to see.
        let oak = arrivals(500, &[1495, 1511, 1500, 1516]);
        assert!(oak.iter().all(|b| *b == oak[0]));
        assert_ne!(stone[0], oak[0], "20 ticks and 30 ticks must not agree");
        assert_eq!(stone[0], serde_json::json!(2));
        assert_eq!(oak[0], serde_json::json!(3));
    }

    /// **`within` now applies to the observations it was written to apply
    /// to, and to no others.**
    ///
    /// This is not an exemption and nothing is hidden from the corpus by it.
    /// A `set_entity_motion` about the neighbouring workspace's item is not a
    /// signal being deleted — it is observed, in full, by the scenario that
    /// dropped the item, which is where the mechanism actually is. What
    /// changes is that it stops also being counted in every later scenario,
    /// unbounded, on a timetable set by how long it takes to despawn.
    ///
    /// Three cases, and the third is the one that stops this manufacturing a
    /// check that cannot fail.
    #[test]
    fn an_entity_is_bounded_by_where_it_is_and_never_by_a_position_nobody_sent() {
        let anchor = [-1888, 64, 0];
        let motion = |e: i32| {
            let mut o = obs("set_entity_motion", 0);
            o.fields
                .insert("entity".into(), crate::trace::entity_ref(e));
            o
        };
        let arrival = |e: i32, x: f64| {
            let mut o = obs("add_entity", 0);
            o.fields
                .insert("entity".into(), crate::trace::entity_ref(e));
            o.fields
                .insert("at".into(), serde_json::json!([x, 0.0, 0.0]));
            o
        };
        // `observed` is threaded because the strict projection now drops
        // `entity_position` unless the scenario named it, and cases 3a below
        // are `540`'s shape — a check that *does* name it. Passing an empty
        // set there would delete the packet before this test could ask what
        // `place` did with it, which would leave the placement rule asserted
        // by nothing.
        let run = |stream: &[Obs], observed: &[&str]| {
            let observed: BTreeSet<String> = observed.iter().map(|s| (*s).to_string()).collect();
            super::project(
                stream,
                None,
                &observed,
                Timing::Ignore,
                crate::scenario::From::Both,
                16,
                anchor,
                &Places::new(),
            )
            .len()
        };

        // 1. Arrived in this workspace: kept, as it always was.
        assert_eq!(run(&[arrival(7, 2.0), motion(7)], &[]), 2);

        // 2. Arrived at the neighbouring workspace, 32 blocks along the axis
        //    the corpus marches down. Vanilla still broadcasts it, because a
        //    dropped item's client tracking range covers the neighbour, and
        //    it is still not this scenario's business.
        assert_eq!(run(&[arrival(7, -32.0), motion(7)], &[]), 0);

        // 3a. An `entity_position` for an entity nobody announced does NOT
        //     place it. This is the clients' case and prediction 7 refuted
        //     the version without it.
        let sync = |e: i32, x: f64| {
            let mut o = obs("entity_position", 0);
            o.fields
                .insert("entity".into(), crate::trace::entity_ref(e));
            o.fields
                .insert("at".into(), serde_json::json!([x, 0.0, 0.0]));
            o
        };
        // The sync is itself positioned and so is bounded by its own `at`;
        // the motion after it is not bounded at all, because nothing ever
        // announced entity 7 arriving anywhere.
        assert_eq!(
            run(&[sync(7, 2.0), motion(7)], &[crate::trace::ENTITY_POSITION]),
            2
        );
        assert_eq!(
            run(
                &[sync(7, -32.0), motion(7)],
                &[crate::trace::ENTITY_POSITION]
            ),
            1
        );

        // 3. **Never placed at all — kept.** Bounding an entity nobody
        //    announced would empty
        //    `520-a-standing-player-is-rebroadcast-on-a-timer`, whose entire
        //    content is the players' heartbeats, and a check whose reference
        //    is empty by construction is a check that cannot fail. This
        //    assertion is what refuses that.
        assert_eq!(run(&[motion(7)], &[]), 1);

        // 4. **A PLAYER'S ARRIVAL DOES NOT PLACE IT**, and this case is here
        //    because the comment above case 3 used to say the clients reach it
        //    for free — "their `add_entity` lands during login, before any
        //    scenario window". `530-walking-away-drops-a-standing-player-from-view`
        //    walks the actor out of the observer's tracking range and back, so
        //    it announces a player *inside* a measured window, at that
        //    workspace; `540` is the next scenario, 32 blocks on, and every
        //    delta about the actor was bounded out of it. See [`place`] for the
        //    recordings that measure it.
        //
        //    Same shape as case 2 — arrival at the neighbour, then a delta —
        //    and the opposite answer, which is the whole content of the rule.
        let player_arrival = |e: i32, x: f64| {
            let mut o = arrival(e, x);
            o.fields.insert(
                "kind_id".into(),
                serde_json::json!(super::player_entity_type_id()),
            );
            o
        };
        assert_eq!(run(&[player_arrival(1, -32.0), motion(1)], &[]), 1);
        // And the arrival itself is still bounded by its own `at`, so this
        // does not smuggle the neighbouring workspace's packet back in: only
        // the delta survives, not the announcement.
        assert_eq!(run(&[player_arrival(1, 2.0), motion(1)], &[]), 2);
        // An item at the same coordinates is unchanged. The rule is on the
        // type, not on the distance, and case 2 must keep failing.
        assert_eq!(run(&[arrival(1, -32.0), motion(1)], &[]), 0);
    }

    /// A leftover is placed by the scenario that *created* it, which is a
    /// different scenario with a different anchor — so the table has to carry
    /// absolute positions across the whole trace, not this scenario's
    /// relative ones.
    #[test]
    fn an_entity_placed_by_an_earlier_scenario_is_still_placed() {
        let mut places = Places::new();
        let mut arrival = obs("add_entity", 0);
        arrival
            .fields
            .insert("entity".into(), crate::trace::entity_ref(7));
        arrival
            .fields
            .insert("at".into(), serde_json::json!([0.0, 0.0, 0.0]));
        // Scenario 58's workspace.
        advance(&[arrival], [-1888, 64, 0], &mut places);
        assert_eq!(places.get(&7), Some(&[-1888.0, 64.0, 0.0]));

        let mut motion = obs("set_entity_motion", 0);
        motion
            .fields
            .insert("entity".into(), crate::trace::entity_ref(7));
        // Read from scenario 60's workspace, 64 blocks away: gone.
        let seen = super::project(
            &[motion],
            None,
            &BTreeSet::new(),
            Timing::Ignore,
            crate::scenario::From::Both,
            16,
            [-1952, 64, 0],
            &places,
        );
        assert!(seen.is_empty(), "a neighbour's item is not our business");
    }

    #[test]
    fn a_reference_that_did_nothing_is_vacuous_not_a_pass() {
        // The rule this harness exists to mechanise: a check that passes
        // because nothing fired has measured nothing.
        assert_eq!(verdict_for(&[], &[]), Verdict::Vacuous);
    }

    /// **`vacuous` must not be able to swallow a subject that spoke.**
    ///
    /// The `Inert` defect of 2026-08-03 was a bucket chosen from the
    /// reference projection alone, overwriting a real verdict. `Vacuous` is
    /// chosen the same way and had the same hole in exactly one direction:
    /// a subject producing observations where the reference produced none is
    /// a server saying something vanilla did not, and reporting it as "a
    /// fault in the check" sends the reader to rewrite the check.
    ///
    /// This is the assertion that stops that reappearing. The two verdicts
    /// are told apart by the *subject*, which is the half `vacuous` was not
    /// reading.
    #[test]
    fn a_subject_that_spoke_where_the_reference_was_silent_is_not_vacuous() {
        let spoke = project(
            &[obs("block_update", 0)],
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        assert!(!spoke.is_empty(), "the fixture has to be non-empty");
        assert_eq!(verdict_for(&[], &spoke), Verdict::Diverge);
        // And the case it exists for is untouched.
        assert_eq!(verdict_for(&[], &[]), Verdict::Vacuous);
        // The narrowness of the hole, stated as a test: a two-sided
        // divergence was never reachable through `vacuous`, because the
        // reference being non-empty is what `Absent` and `Diverge` are for.
        assert_eq!(verdict_for(&spoke, &[]), Verdict::Absent);
    }

    #[test]
    fn nothing_at_all_is_absent_and_something_wrong_is_diverge() {
        let a = project(
            &[obs("block_update", 0)],
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        let b = project(
            &[obs("block_update", 1)],
            None,
            Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
        );
        assert_eq!(verdict_for(&a, &[]), Verdict::Absent);
        assert_eq!(verdict_for(&a, &b), Verdict::Diverge);
        assert_eq!(verdict_for(&a, &a), Verdict::Match);
    }

    #[test]
    fn verdicts_rank_so_a_ratchet_can_compare_them() {
        assert!(Verdict::Match > Verdict::Diverge);
        assert!(Verdict::Diverge > Verdict::Absent);
        assert!(Verdict::Absent > Verdict::Vacuous);
        assert!(Verdict::Vacuous > Verdict::Error);
        assert!(!Verdict::Vacuous.baselineable());
        assert!(!Verdict::Error.baselineable());
        assert!(Verdict::Absent.baselineable());
    }

    #[test]
    fn the_projection_hides_kinds_a_scenario_did_not_ask_about() {
        let kinds: BTreeSet<String> = ["block_update".to_string()].into_iter().collect();
        let all = vec![obs("block_update", 0), obs("sound", 0)];
        assert_eq!(
            project(
                &all,
                Some(&kinds),
                Timing::Ignore,
                crate::scenario::From::Both,
                i32::MAX
            )
            .len(),
            1
        );
        assert_eq!(
            project(
                &all,
                None,
                Timing::Ignore,
                crate::scenario::From::Both,
                i32::MAX
            )
            .len(),
            2
        );
    }

    #[test]
    fn timing_is_compared_only_when_asked_for() {
        let mut early = obs("block_update", 0);
        early.at_ms = 100;
        let mut late = obs("block_update", 0);
        late.at_ms = 9000;
        assert_eq!(
            project(
                std::slice::from_ref(&early),
                None,
                Timing::Ignore,
                crate::scenario::From::Both,
                i32::MAX
            ),
            project(
                std::slice::from_ref(&late),
                None,
                Timing::Ignore,
                crate::scenario::From::Both,
                i32::MAX
            )
        );
        assert_ne!(
            project(
                std::slice::from_ref(&early),
                None,
                Timing::Bucket { ms: 1000 },
                crate::scenario::From::Both,
                i32::MAX
            ),
            project(
                std::slice::from_ref(&late),
                None,
                Timing::Bucket { ms: 1000 },
                crate::scenario::From::Both,
                i32::MAX
            )
        );
    }

    #[test]
    fn a_baseline_refuses_a_regression_and_notices_an_improvement() {
        let mut b = Baseline::default();
        b.expect.insert("a".into(), "match".into());
        b.expect.insert("c".into(), "absent".into());
        let mk = |name: &str, v: Verdict| Outcome {
            name: name.into(),
            about: String::new(),
            verdict: v,
            strict: v,
            reference_obs: 1,
            subject_obs: 0,
            measured_step: Some(0),
            measured_reference_obs: 0,
            measured_subject_obs: 0,
            measured_reference_kinds: Vec::new(),
            missing_kinds: Vec::new(),
            extra_kinds: Vec::new(),
            first_difference: None,
            reference_projection: Vec::new(),
            subject_projection: Vec::new(),
            reference_strict_projection: Vec::new(),
            subject_strict_projection: Vec::new(),
        };
        let (worse, better) = b.check(&[
            mk("a", Verdict::Absent),
            mk("b", Verdict::Match),
            mk("c", Verdict::Match),
        ]);
        assert_eq!(worse.len(), 2, "a regressed and b is unlisted: {worse:?}");
        assert_eq!(better.len(), 1, "c improved: {better:?}");
    }

    // ---- collapse_repeats -------------------------------------------------
    //
    // The rig is the chest lid, because that is the mechanism the feature
    // exists for: the same `block_event(1, 1)` at the same position, repeated
    // by a 5-tick timer for as long as the chest is open, sampled through a
    // window whose edge decides how many arrive.

    /// One `block_event(1, b1)` at `(1, 0, 0)`, in a given step.
    fn lid(b1: i64, step: usize) -> Obs {
        let mut o = obs("block_event", step);
        o.fields.insert("at".into(), serde_json::json!([1, 0, 0]));
        o.fields.insert("b0".into(), serde_json::json!(1));
        o.fields.insert("b1".into(), serde_json::json!(b1));
        o.fields
            .insert("block".into(), serde_json::json!("minecraft:chest"));
        o
    }

    fn collapsed(obs: &[Obs], kinds: &[&str]) -> Vec<serde_json::Value> {
        let mut p = project(obs, None, Timing::Ignore, crate::scenario::From::Both, 16);
        let set: BTreeSet<String> = kinds.iter().map(|k| (*k).to_string()).collect();
        collapse_repeats(&mut p, &set);
        p
    }

    /// Six repeats and five repeats project to the same thing, which is the
    /// entire point: vanilla emitted six inside a 1,000 ms window and this
    /// server emitted five, with the same `b0`, the same `b1`, the same
    /// position and the same mean gap.
    #[test]
    fn a_run_of_identical_observations_collapses_to_one() {
        let six: Vec<Obs> = (0..6).map(|_| lid(1, 0)).collect();
        let five: Vec<Obs> = (0..5).map(|_| lid(1, 0)).collect();
        let a = collapsed(&six, &["block_event"]);
        assert_eq!(a.len(), 1, "a run is one observation");
        assert_eq!(a, collapsed(&five, &["block_event"]));
        // And the surviving one still carries everything.
        assert_eq!(a[0]["fields"]["b1"], serde_json::json!(1));
        assert_eq!(a[0]["fields"]["at"], serde_json::json!([1, 0, 0]));
    }

    /// **The kind has to be named.** Without this the feature would be a
    /// corpus-wide loosening rather than a per-check declaration, and a
    /// server that sent one `block_update` where vanilla sent four would stop
    /// being caught everywhere at once.
    #[test]
    fn an_undeclared_kind_is_not_collapsed() {
        let six: Vec<Obs> = (0..6).map(|_| lid(1, 0)).collect();
        assert_eq!(collapsed(&six, &[]).len(), 6);
        assert_eq!(collapsed(&six, &["block_update"]).len(), 6);
    }

    /// `A A A B A A` is `A B A`. A mechanism that stops and starts again is
    /// not the same event as one that never stopped, and a check that named
    /// the kind has not asked to stop telling them apart.
    ///
    /// This is the assertion that keeps the fix narrow. The wider rule — first
    /// of each distinct value, for ever — would give `A B` here and stop
    /// distinguishing a chest that was reopened from one that stayed shut, for
    /// no gain: it collapses not one observation more than this on any trace
    /// on disk, because no lid in this corpus alternates.
    #[test]
    fn a_mechanism_that_resumes_is_not_a_repeat() {
        let mixed = vec![
            lid(1, 0),
            lid(1, 0),
            lid(1, 0),
            lid(0, 0),
            lid(1, 0),
            lid(1, 0),
        ];
        let p = collapsed(&mixed, &["block_event"]);
        assert_eq!(p.len(), 3, "open, shut, open");
        let b1 = |i: usize| p[i]["fields"]["b1"].clone();
        assert_eq!(
            (b1(0), b1(1), b1(2)),
            (
                serde_json::json!(1),
                serde_json::json!(0),
                serde_json::json!(1)
            )
        );
    }

    /// **A step boundary no longer breaks a run, and an unnamed kind between
    /// two repeats no longer breaks one either.** These are the two halves of
    /// the defect R1 fixed, and they are the same defect: the collapse was
    /// keyed on things the declaration says are not being tested.
    ///
    /// The step case is `560`'s in miniature. A free-running 250 ms timer puts
    /// its edges wherever the phase puts them, so which step a given repeat is
    /// stamped with is the harness's sampling; leaving `step` in the key left
    /// the per-step counts being compared, which is exactly what six of seven
    /// chest scenarios disagreed on in one run.
    ///
    /// The interleaving case is the reason the declaration could not reach
    /// `strict` at all: there, `block_changed_ack` and `container_set_slot`
    /// sit between every pair of lid events.
    #[test]
    fn neither_a_step_boundary_nor_an_unnamed_kind_breaks_a_repeat() {
        let across = vec![lid(1, 0), lid(1, 0), lid(1, 1), lid(1, 1)];
        assert_eq!(collapsed(&across, &["block_event"]).len(), 1);

        let interleaved = vec![
            lid(1, 0),
            obs("block_changed_ack", 0),
            lid(1, 0),
            obs("container_set_slot", 1),
            lid(1, 1),
        ];
        let p = collapsed(&interleaved, &["block_event"]);
        assert_eq!(
            p.iter().filter(|o| o["kind"] == "block_event").count(),
            1,
            "one lid, whatever arrived between the repeats"
        );
        assert_eq!(p.len(), 3, "and the kinds it was not given are untouched");
    }

    /// The survivor carries a blanked `step` and `when`, and **only for the
    /// kinds the check named**. Pinned because the alternative — blanking the
    /// key but not the observation — reads identically at every call site and
    /// leaves the first repeat's step index being compared, which is the same
    /// coin with a smaller amplitude.
    #[test]
    fn only_the_named_kinds_lose_their_step() {
        let mixed = vec![lid(1, 2), obs("block_changed_ack", 2)];
        let p = collapsed(&mixed, &["block_event"]);
        assert_eq!(p[0]["step"], serde_json::Value::Null);
        assert_eq!(p[1]["step"], serde_json::json!(2));
    }

    /// The full rig, and the four states that matter. **Three of them must
    /// still be red**, or this would be the sixteenth check-that-cannot-fail
    /// in this corpus and it would have been manufactured on purpose.
    #[test]
    fn collapsing_repeats_hides_the_count_and_nothing_else() {
        let scenario = Scenario {
            name: "chest-opens-a-screen".into(),
            about: "the lid repeats on a timer; the repeat count is the harness's sampling".into(),
            observe: vec!["open_screen".into(), "block_event".into()],
            timing: Timing::Ignore,
            observe_from: crate::scenario::From::Both,
            settle_ms: 1000,
            within: 16,
            collapse_repeats: vec!["block_event".into()],
            measured_step: crate::scenario::MeasuredStep::Speaks,
            gamerule: Vec::new(),
            console_restore: Vec::new(),
            setup: Vec::new(),
            step: vec![crate::scenario::Step::Place {
                on: [1, 0, 0],
                face: "up".into(),
                cursor: None,
            }],
        };
        let trace = |o: Vec<Obs>| Trace {
            target: "test".into(),
            version: "26.2".into(),
            protocol: 776,
            recorder: "test".into(),
            scenarios: vec![crate::trace::ScenarioTrace {
                name: scenario.name.clone(),
                error: None,
                anchor: [0, 0, 0],
                spawn: [0.0, 0.0, 0.0],
                declared_commands: Vec::new(),
                obs: o,
                fresh_session: None,
            }],
        };
        let screen = obs("open_screen", 0);
        let van: Vec<Obs> = std::iter::once(screen.clone())
            .chain((0..6).map(|_| lid(1, 0)))
            .collect();
        let verdict = |ours: Vec<Obs>| {
            compare(
                std::slice::from_ref(&scenario),
                &trace(van.clone()),
                &trace(ours),
            )[0]
            .verdict
        };

        // 1. A different number of repeats is the same behaviour.
        let five: Vec<Obs> = std::iter::once(screen.clone())
            .chain((0..5).map(|_| lid(1, 0)))
            .collect();
        assert_eq!(verdict(five), Verdict::Match);

        // 2. A wrong viewer count is still a divergence.
        let wrong_b1: Vec<Obs> = std::iter::once(screen.clone())
            .chain((0..5).map(|_| lid(2, 0)))
            .collect();
        assert_eq!(verdict(wrong_b1), Verdict::Diverge);

        // 3. So is a missing screen — the collapse must not swallow a
        //    neighbouring kind it was not given.
        let no_screen: Vec<Obs> = (0..5).map(|_| lid(1, 0)).collect();
        assert_eq!(verdict(no_screen), Verdict::Diverge);

        // 4. And a lid that never arrives at all is still `absent`, which is
        //    the floor this feature has to leave intact.
        assert_eq!(verdict(Vec::new()), Verdict::Absent);

        // 5. **A lid on the wrong stream is still a divergence.** `who` stays
        //    in the key; only `step` and `when` leave it. Named because
        //    `490-a-bystander-sees-a-chest-open` is entirely about which
        //    client the lid reaches.
        let wrong_stream: Vec<Obs> = std::iter::once(screen.clone())
            .chain((0..5).map(|_| {
                let mut o = lid(1, 0);
                o.who = crate::trace::Who::Actor;
                o
            }))
            .collect();
        assert_eq!(verdict(wrong_stream), Verdict::Diverge);
    }

    /// **`strict` collapses too, and it must still be able to fail.**
    ///
    /// This test replaced `strict_still_sees_every_repeat`, which asserted the
    /// opposite, and the reversal is the whole of R1. The old assertion was
    /// defended as keeping the declared/strict gap honest — but the gap it was
    /// keeping was not deferred verification, it was the phase of a
    /// free-running timer against a window edge, and no server change could
    /// ever close it. What replaces the coverage is not nothing: three of the
    /// four cases below are red, on the strict axis, after the collapse.
    #[test]
    fn strict_collapses_but_still_fails_on_everything_that_is_not_the_count() {
        let scenario = Scenario {
            name: "chest-opens-a-screen".into(),
            about: "strict is not filtered by a check's own declarations".into(),
            observe: vec!["block_event".into()],
            timing: Timing::Ignore,
            observe_from: crate::scenario::From::Both,
            settle_ms: 1000,
            within: 16,
            collapse_repeats: vec!["block_event".into()],
            measured_step: crate::scenario::MeasuredStep::Speaks,
            gamerule: Vec::new(),
            console_restore: Vec::new(),
            setup: Vec::new(),
            step: vec![crate::scenario::Step::Swing {
                hand: "main".into(),
                ms: 200,
            }],
        };
        let trace = |o: Vec<Obs>| Trace {
            target: "test".into(),
            version: "26.2".into(),
            protocol: 776,
            recorder: "test".into(),
            scenarios: vec![crate::trace::ScenarioTrace {
                name: scenario.name.clone(),
                error: None,
                anchor: [0, 0, 0],
                spawn: [0.0, 0.0, 0.0],
                declared_commands: Vec::new(),
                obs: o,
                fresh_session: None,
            }],
        };
        let van: Vec<Obs> = (0..6).map(|_| lid(1, 0)).collect();
        let strict = |ours: Vec<Obs>| {
            compare(
                std::slice::from_ref(&scenario),
                &trace(van.clone()),
                &trace(ours),
            )[0]
            .strict
        };

        // 1. Six against five is the same behaviour, in strict as in declared.
        //    This is the line that used to read `Diverge`.
        assert_eq!(strict((0..5).map(|_| lid(1, 0)).collect()), Verdict::Match);

        // 2. Six against five *at a different position* is not. The collapse
        //    keeps every field, so a lid on the wrong block still parts.
        let elsewhere: Vec<Obs> = (0..5)
            .map(|_| {
                let mut o = lid(1, 0);
                o.fields.insert("at".into(), serde_json::json!([9, 0, 0]));
                o
            })
            .collect();
        assert_eq!(strict(elsewhere), Verdict::Diverge);

        // 3. Nor is a wrong viewer count.
        assert_eq!(
            strict((0..5).map(|_| lid(2, 0)).collect()),
            Verdict::Diverge
        );

        // 4. And a server that never opens the lid is `absent`: one against
        //    zero survives any collapse, which is the floor this must leave
        //    standing on both axes.
        assert_eq!(strict(Vec::new()), Verdict::Absent);
    }

    /// One `block_changed_ack` carrying a raw session sequence number.
    fn ack(who: crate::trace::Who, seq: i64) -> Obs {
        let mut o = obs("block_changed_ack", 0);
        o.who = who;
        o.fields.insert("seq".into(), serde_json::json!(seq));
        o
    }

    /// One `open_screen` carrying a raw container id.
    fn screen(window: i64) -> Obs {
        let mut o = obs("open_screen", 0);
        o.who = crate::trace::Who::Actor;
        o.fields.insert("window".into(), serde_json::json!(window));
        o.fields.insert("menu".into(), serde_json::json!(12));
        o
    }

    /// One `container_set_slot` carrying a raw menu state id.
    ///
    /// The defaults are the measured ones: slot 36 and window 0 are what
    /// `closing-a-crafting-table-returns-the-grid-contents` and
    /// `swapping-to-the-offhand-shows-on-both-channels` both moved on.
    fn set_slot(window: i64, state: i64, slot: i64) -> Obs {
        let mut o = obs("container_set_slot", 0);
        o.who = crate::trace::Who::Actor;
        o.fields.insert("window".into(), serde_json::json!(window));
        o.fields.insert("state".into(), serde_json::json!(state));
        o.fields.insert("slot".into(), serde_json::json!(slot));
        o
    }

    /// The state id of one projected observation, by index.
    fn state_of(projected: &[serde_json::Value], i: usize) -> i64 {
        projected[i]["fields"]["state"]
            .as_i64()
            .expect("the state id must survive the projection, rebased not dropped")
    }

    /// **Half of the control that decides whether this is the right fix.**
    ///
    /// Two streams differing only in the session history *in front of* the
    /// first packet on that window must project the same. This is the
    /// measured signature of all three retired
    /// `ci/checks/gameplay-unstable.toml` entries: `swapping-to-the-offhand`
    /// carried states 3 and 4 in one run and 2 and 3 in the other, a constant
    /// shift of exactly one.
    #[test]
    fn two_streams_differing_only_in_session_history_project_the_same() {
        let early = strict_project(&[set_slot(0, 2, 36), set_slot(0, 3, 45)]);
        let late = strict_project(&[set_slot(0, 3, 36), set_slot(0, 4, 45)]);
        assert_eq!(early.len(), 2, "both slot changes must survive");
        assert_eq!(early, late);

        // And the field is still there, carrying the distance rather than the
        // history. An assertion of equality alone is satisfied by a decoder
        // that deleted the field, which is the wrong fix.
        assert_eq!(state_of(&early, 0), 0);
        assert_eq!(state_of(&early, 1), 1);
    }

    /// **The other half, and the one a decoder that DROPS the field fails.**
    ///
    /// How far the counter moves between two packets on one menu is real
    /// behaviour: a server that sent one packet where vanilla sent two
    /// differs here, and the projection has to keep saying so. Same first
    /// value, different second — the shape
    /// `rebasing_a_sequence_keeps_the_gaps_inside_it` uses for `seq`.
    #[test]
    fn two_streams_differing_in_the_gap_between_two_packets_project_differently() {
        let tight = strict_project(&[set_slot(0, 3, 36), set_slot(0, 4, 45)]);
        let loose = strict_project(&[set_slot(0, 3, 36), set_slot(0, 5, 45)]);
        assert_eq!(tight.len(), 2);
        assert_ne!(
            tight, loose,
            "rebasing must keep the gap between two packets on one menu"
        );
        assert_eq!(state_of(&tight, 1), 1);
        assert_eq!(state_of(&loose, 1), 2);
    }

    /// The single-observation case, which is what `611` and the flower pot
    /// both are.
    ///
    /// A lone state id carries nothing *but* session history — there is no
    /// second packet for it to be a distance from — so it projects to 0 on
    /// both sides and the check stops flapping. What the check still has is
    /// every other field: the control below moves the slot and must diverge.
    #[test]
    fn a_lone_state_id_carries_only_history_and_the_rest_of_the_packet_survives() {
        let nine = strict_project(&[set_slot(0, 9, 38)]);
        let ten = strict_project(&[set_slot(0, 10, 38)]);
        assert_eq!(nine.len(), 1);
        assert_eq!(nine, ten);
        assert_eq!(state_of(&nine, 0), 0);

        // The control. Rebasing the state id must not have made the whole
        // observation unfalsifiable.
        assert_ne!(nine, strict_project(&[set_slot(0, 9, 36)]));
    }

    /// `stateId` is an instance field of `AbstractContainerMenu`, so two open
    /// menus count independently and must rebase independently.
    ///
    /// One base shared across windows would make the container's first answer
    /// a function of how many times the inventory had answered, which is the
    /// conflation this whole function exists to undo — one door further in.
    #[test]
    fn two_menus_rebase_against_their_own_state_counters() {
        // Same inventory history on both sides; the container's own counter
        // differs only in where it started.
        let a = strict_project(&[set_slot(0, 9, 38), set_slot(3, 1, 0), set_slot(3, 2, 1)]);
        let b = strict_project(&[set_slot(0, 4, 38), set_slot(3, 6, 0), set_slot(3, 7, 1)]);
        assert_eq!(a.len(), 3);
        assert_eq!(a, b);

        // The container's own gap is still compared, on its own base.
        let stretched = strict_project(&[set_slot(0, 4, 38), set_slot(3, 6, 0), set_slot(3, 8, 1)]);
        assert_ne!(a, stretched);
    }

    /// Window 0's `window` field is a name and is left alone; window 0's
    /// `state` counter is a draw and is rebased. Both halves, because the two
    /// fields sit on the same observation and the easy mistake is to treat
    /// them the same way.
    ///
    /// This is the case that matters most: `InventoryMenu` is created with
    /// the player and never closes, so window 0's counter is the only one
    /// that accumulates across the whole corpus run.
    #[test]
    fn window_zero_keeps_its_name_and_loses_its_state_history() {
        let p = strict_project(&[set_slot(0, 9, 38)]);
        assert_eq!(
            p[0]["fields"]["window"].as_i64(),
            Some(0),
            "window 0 is the inventory's name and must not be rebased"
        );
        assert_eq!(
            state_of(&p, 0),
            0,
            "window 0's state id is a session counter and must be"
        );
    }

    /// `incrementStateId` is `(stateId + 1) & 32767`, so a projection that
    /// straddles the wrap must not read as a 32767-step jump backwards.
    #[test]
    fn a_state_id_that_wraps_still_projects_its_true_distance() {
        let wrapped = strict_project(&[set_slot(0, 32767, 36), set_slot(0, 0, 45)]);
        assert_eq!(wrapped.len(), 2);
        assert_eq!(
            state_of(&wrapped, 1),
            1,
            "one packet after 32767 is one step, not minus 32767"
        );
        assert_eq!(
            wrapped,
            strict_project(&[set_slot(0, 3, 36), set_slot(0, 4, 45)]),
            "the wrap is where the counter is, not a difference in behaviour"
        );
    }

    /// `From::Both`, deliberately. `From::Actor` drops every observer
    /// observation, and a rebase test that cannot see the observer's stream
    /// is a test that passes whatever the two connections do — which is how
    /// the first draft of `the_two_connections_rebase_against_their_own_counters`
    /// passed while sharing one base. Every caller asserts the length it
    /// expects for the same reason.
    fn strict_project(obs: &[Obs]) -> Vec<serde_json::Value> {
        project(obs, None, Timing::Ignore, crate::scenario::From::Both, 8)
    }

    /// The `worldgen-iceberg-912` cascade, in the smallest form that carries
    /// it: two sides whose sequence numbers differ by a constant because one
    /// of them reconnected a scenario earlier.
    ///
    /// The offsets are the measured ones — vanilla restarted at 2 after its
    /// fresh login while ours was twenty-one scenarios into the same counter
    /// at 33. Before [`super::rebase_session_counters`] this pair was
    /// `diverge` and took twenty-five other checks with it.
    #[test]
    fn a_one_sided_reconnect_no_longer_moves_the_next_scenarios_verdict() {
        use crate::trace::Who::Actor;
        let vanilla = strict_project(&[ack(Actor, 2), ack(Actor, 3)]);
        let ours = strict_project(&[ack(Actor, 33), ack(Actor, 34)]);
        assert_eq!(
            vanilla.len(),
            2,
            "both acknowledgements must survive the projection"
        );
        assert_eq!(vanilla, ours);
    }

    /// And the check has not been hollowed out to buy that.
    ///
    /// A gap inside the scenario is the whole of what a `block_changed_ack`
    /// sequence can say — an action vanilla acknowledged and we did not
    /// leaves one — so rebasing must not be able to absorb one. Same first
    /// value, different second.
    #[test]
    fn rebasing_a_sequence_keeps_the_gaps_inside_it() {
        use crate::trace::Who::Actor;
        let vanilla = strict_project(&[ack(Actor, 2), ack(Actor, 3)]);
        let skipped = strict_project(&[ack(Actor, 33), ack(Actor, 35)]);
        assert_ne!(vanilla, skipped);

        // Nor an acknowledgement that never arrived at all.
        let short = strict_project(&[ack(Actor, 33)]);
        assert_ne!(vanilla, short);
    }

    /// Window 0 is the player's own inventory and is never allocated, so it
    /// is a name and not a position. Rebasing it would make "answered on the
    /// inventory" and "answered on the menu" the same observation.
    #[test]
    fn container_window_zero_is_not_a_counter_and_is_left_alone() {
        let inventory = strict_project(&[screen(0)]);
        assert_eq!(
            inventory[0]["fields"]["window"],
            serde_json::json!(0),
            "window 0 must survive the rebase unchanged"
        );

        // An allocated id rebases to 0 whatever the session had reached, so
        // the menu.rs hazard — one unimplemented container shifting every
        // later menu check by one — can no longer convict a healthy check.
        assert_eq!(strict_project(&[screen(1)]), strict_project(&[screen(2)]));

        // But a server that answers on the inventory where vanilla opened a
        // menu still parts, which is the distinction being protected.
        assert_ne!(strict_project(&[screen(0)]), strict_project(&[screen(1)]));
    }

    /// The actor and the observer are two connections, so they are two
    /// counters. One base shared between them would be the same conflation
    /// this rebase exists to undo — and it would be silent, because the two
    /// streams are sorted together before the projection is built.
    #[test]
    fn the_two_connections_rebase_against_their_own_counters() {
        use crate::trace::Who::{Actor, Observer};
        // The actor's counter is twenty-one scenarios ahead in one run and
        // freshly reset in the other; the observer's is unrelated to both,
        // and is *behind* the actor in one run and ahead of it in the other.
        let a = strict_project(&[
            ack(Actor, 2),
            ack(Observer, 40),
            ack(Observer, 44),
            ack(Actor, 3),
        ]);
        let b = strict_project(&[
            ack(Actor, 33),
            ack(Observer, 7),
            ack(Observer, 11),
            ack(Actor, 34),
        ]);
        assert_eq!(
            a.len(),
            4,
            "the observer's stream must reach the projection"
        );
        // This equality *is* the control for the scoping. One base shared
        // between the two connections would rebase the observer's 40 against
        // the actor's 2 — giving 38 here and -26 there — and the assertion
        // could not hold. A single observation is no control at all: it
        // rebases to 0 whatever its value, which is how the first draft of
        // this test passed while asserting nothing.
        assert_eq!(a, b);

        // And the observer's own gap stays its own: 40 -> 44 is four wide and
        // must not compare equal to a gap of one.
        let narrowed = strict_project(&[
            ack(Actor, 33),
            ack(Observer, 7),
            ack(Observer, 8),
            ack(Actor, 34),
        ]);
        assert_ne!(a, narrowed);
    }

    /// One trace of named scenarios, each carrying a `fresh_session` flag.
    fn pathed(flags: &[(&str, Option<bool>)]) -> crate::trace::Trace {
        crate::trace::Trace {
            target: "t".into(),
            version: "26.2".into(),
            protocol: 776,
            recorder: "test".into(),
            scenarios: flags
                .iter()
                .map(|(n, f)| crate::trace::ScenarioTrace {
                    name: (*n).into(),
                    error: None,
                    anchor: [0, 0, 0],
                    spawn: [0.0, 0.0, 0.0],
                    declared_commands: Vec::new(),
                    obs: Vec::new(),
                    fresh_session: *f,
                })
                .collect(),
        }
    }

    /// The detector for the fault *class*, as opposed to the repair for the
    /// two counters we happen to know about.
    ///
    /// The shape is the iceberg-912 one: both runs log in at the first
    /// scenario, then the reference alone reconnects partway through. Every
    /// per-connection counter restarts on that side and on no other, and
    /// whether the comparator happens to rebase the particular fields
    /// involved is not the question this asks.
    #[test]
    fn a_one_sided_reconnect_is_reported_even_when_no_counter_is_compared() {
        let reference = pathed(&[
            ("first", Some(true)),
            ("water-spreads", Some(false)),
            ("after", Some(true)),
        ]);
        let subject = pathed(&[
            ("first", Some(true)),
            ("water-spreads", Some(false)),
            ("after", Some(false)),
        ]);
        let found = super::session_path_divergence(&reference, &subject);
        assert_eq!(found.len(), 1, "exactly the one scenario that parted");
        assert!(
            found[0].starts_with("after: reference reconnected here"),
            "{found:?}"
        );

        // Two runs that took the same path say nothing at all — a detector
        // that fires on healthy runs is noise and would be turned off.
        assert!(super::session_path_divergence(&reference, &reference).is_empty());
    }

    /// An absent measurement is not a negative one. Traces recorded before
    /// the flag existed carry `None`, and reading that as "did not reconnect"
    /// would make every old-against-new comparison report a divergence at the
    /// first scenario, which is how a useful signal gets ignored.
    #[test]
    fn a_trace_that_never_recorded_the_flag_is_skipped_not_accused() {
        let old = pathed(&[("first", None), ("after", None)]);
        let new = pathed(&[("first", Some(true)), ("after", Some(false))]);
        assert!(super::session_path_divergence(&old, &new).is_empty());
        assert!(super::session_path_divergence(&new, &old).is_empty());
    }

    /// The regression test for the reset itself: a whole side's counters
    /// shifted by a constant must not move a single verdict.
    ///
    /// This is deliberately not a test of `rebase_session_counters`' internals.
    /// It applies the transform the fault applies — add a constant to every
    /// session counter from some scenario onward, exactly as a side that did
    /// **not** reconnect differs from one that did — and asserts the
    /// projection is unchanged. A future counter that is decoded but not
    /// rebased fails here.
    #[test]
    fn shifting_one_sides_counters_by_a_constant_moves_no_verdict() {
        use crate::trace::Who::Actor;
        // Every counter-bearing kind the corpus records, in one scenario.
        // `container_set_slot` joined the list on 2026-08-11: its `state` is
        // the menu's own packet counter and shifts for the same reason.
        let unreset = [
            ack(Actor, 33),
            screen(8),
            set_slot(0, 40, 36),
            ack(Actor, 35),
            screen(9),
            set_slot(0, 41, 45),
        ];
        let reset: Vec<Obs> = unreset
            .iter()
            .cloned()
            .map(|mut o| {
                for name in ["seq", "window", "state"] {
                    if let Some(v) = o.fields.get_mut(name) {
                        let raw = v.as_i64().expect("counter is a number");
                        // Window 0 is the inventory's name, and a reconnect
                        // does not renumber it — `InventoryMenu` is 0 on both
                        // sides of every comparison. Shifting it here would
                        // model a fault that cannot happen. Its `state`,
                        // which is a counter, is shifted like all the others.
                        if name == "window" && raw == 0 {
                            continue;
                        }
                        *v = serde_json::json!(raw - 31);
                    }
                }
                o
            })
            .collect();
        assert_eq!(
            strict_project(&unreset),
            strict_project(&reset),
            "a constant shift of one side's session counters must be invisible"
        );

        // The controls: a shift that is *not* constant is a real difference
        // and must still be caught, or the assertion above is satisfied by a
        // projection that has simply stopped looking at these fields. One per
        // counter, because a control on `seq` alone would pass for a
        // projection that had stopped looking at `state`.
        for (i, name) in [(3, "seq"), (4, "window"), (5, "state")] {
            let mut skewed = reset.clone();
            let v = skewed[i].fields.get_mut(name).expect("counter");
            *v = serde_json::json!(v.as_i64().expect("number") + 1);
            assert_ne!(
                strict_project(&unreset),
                strict_project(&skewed),
                "a non-constant shift of `{name}` must still be visible"
            );
        }
    }
}
