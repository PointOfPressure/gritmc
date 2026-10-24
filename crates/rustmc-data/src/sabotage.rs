//! A counterfactual worldgen, for asking a T1 gate what would have made it red.
//!
//! # The question this exists to answer
//!
//! `docs/PARITY-ROADMAP.md` §73.7 counted, over T1's 80 test binaries, how
//! many have a positive control — anything ever shown to make them fail — and
//! found **74 with neither a falsifier in `ci/` nor a control in the file**.
//! §73.7 is careful about what that does and does not mean: not that the 74 are
//! worthless, but that "for 74 binaries nobody has ever demonstrated a red, so
//! nobody knows which of them are in the same state
//! `can_survive_matches_vanilla_where_both_sides_answer` was in an hour ago".
//!
//! Most of the 74 are forced by moving or perturbing the fixture they read —
//! `ci/t1-force.py`, and that is the cheap axis. It runs out at the binaries
//! that read no fixture at all: `golden_rng` carries its vectors inline,
//! `plugin_storage` builds its world in memory. For those, the only thing that
//! can force a red is **a wrong implementation**, which is what this module is.
//!
//! # Why a wrong implementation and not a mutated input
//!
//! §73.9 measured the two mutations that matter and the result is the argument
//! for this file:
//!
//! ```text
//!   broken xoroshiro state update   794 of 795 lib tests still pass   8 of 9 golden binaries red
//!   inverted ore discard rule       795 of 795 lib tests pass         both goldens red
//! ```
//!
//! A defect in the RNG under every worldgen stream is invisible to 794 of 795
//! unit tests. The layer that sees it is the golden-vector layer, which lives
//! entirely in T1. So the arms below are the shapes of subsystems that were
//! never written correctly, and a gate that stays green with its subsystem
//! sabotaged is a gate that would have passed before the subsystem existed.
//!
//! ```text
//!   xoroshiro    the Xoroshiro128++ state update rotates by the wrong amount
//!   legacy-lcg   the legacy LCG returns low-order bits instead of high
//!   palette      a paletted container packs one bit narrower than it needs
//!   storage       a plugin's storage capability is published unconditionally
//!   path-accuracy the A* reaches a target only ON it, ignoring `accuracy`
//! ```
//!
//! # The firing counter is half the instrument, not diagnostics
//!
//! A gate that stays green under its own sabotage has failed for one of two
//! reasons and they are not the same finding: either the sabotaged code
//! **never ran**, so no implementation of it could have moved the result, or it
//! ran and the assertion did not look. Only the first is distinguishable
//! without a second run, and it is distinguishable exactly because this module
//! counts. [`report`] prints one line per armed arm with the number of times it
//! fired, and `fired=0` on an armed arm is the `UNVISITED` verdict and needs no
//! further evidence.
//!
//! This is the same design as `crates/rustmc-server/src/sabotage.rs` on
//! `corpus-falsifiability-audit`, deliberately: that module scopes its arms by
//! workspace index because one corpus recording has to carry every control at
//! once. A T1 arm needs no scoping — a test binary is already the unit of
//! isolation, and `ci/t1-sabotage.sh` runs one arm per invocation.
//!
//! # Why this cannot ship
//!
//! Behind the `t1-sabotage` cargo feature, off by default, and every entry
//! point below compiles to a constant `false` without it. A server built the
//! ordinary way has no branch to reach. `ci/t0.sh` asserts the feature is
//! absent from the default set of every crate that forwards it, because a
//! control that can be turned on in production is not a control, it is a
//! backdoor.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// The subsystems an arm can replace with a wrong one.
///
/// A closed enum rather than free strings so that a typo in the env var is a
/// refusal at parse time and not an arm that silently never arms — which would
/// read as `UNVISITED` for every gate it was pointed at, and is precisely the
/// unfalsifiable shape this module exists to measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Arm {
    /// `Xoroshiro128PlusPlus.nextLong`'s state update rotates `s0` by 48
    /// instead of 49 — §73.9's mutation A, moved behind a feature so it can be
    /// re-run instead of being a one-off edit in a scratch tree.
    Xoroshiro,
    /// `BitRandomSource#next` returns the LOW `bits` of the LCG state instead
    /// of the high ones. Every value stays in range and the stream is still
    /// deterministic, which is what makes it a fair test of a golden vector:
    /// nothing but a comparison against real Java output can see it.
    LegacyLcg,
    /// A paletted container packs at one bit narrower than the palette needs,
    /// so the top palette index aliases to 0.
    Palette,
    /// A plugin's storage capability is published whether or not a world
    /// directory was configured — the shape of the code before the capability
    /// gate was written.
    Storage,
    /// `PathFinder.findPath` treats a target as reached only when a node lands
    /// ON it — `accuracy` forced to 0 — which is the shape of that loop before
    /// the argument was ported.
    ///
    /// It is the arm for `ground_path` and `navigation`, and it is this one
    /// rather than a tie-break because the tie-break was tried and measured.
    /// The first attempt relaxed `BinaryHeap.upHeap`'s strict `<` to `<=`,
    /// the defect that file's own header says "decides which of two equally
    /// good paths a mob takes". It **fired and moved nothing**: `fired=1`,
    /// both binaries green, 8 and 7 tests passing (2026-09-05, `ci/t1-sabotage.sh
    /// --build` in `~/rustmc-stackci2`). Every grid in those two files is a
    /// straight line or a single detour whose optimal path is unique in shape,
    /// so no equal-`f` choice reaches an assertion, and `fired=1` with a green
    /// binary is this module's evidence for exactly that — the code ran and
    /// the gate did not look. **Do not re-run it.**
    ///
    /// Accuracy does reach them: `ground_path::accuracy_stops_short` asserts
    /// the path ends at `(4, 64, 0)` for a target at `(5, 64, 0)`, and every
    /// `move_to` in `navigation` paths at accuracy 1 and counts 5 nodes. The
    /// one-node difference is a live parity claim rather than a synthetic one
    /// — `docs/FINDINGS.md` records that `moveTo` "paths with accuracy 1, so a
    /// mob told to walk to a block stops one node short of it" and that "a
    /// corpus scenario that expects the mob on the block will diverge by one".
    PathAccuracy,
    /// `ServerLevel.advanceWeatherCycle`'s rain and thunder ramps step by
    /// `0.02f` instead of the `ldc_w #1260 // float 0.01f` the bytecode loads,
    /// so a transition crosses `0.2` in ten ticks rather than twenty.
    ///
    /// The arm for `weather_console`, which reads no fixture and so could not
    /// be forced by `ci/t1-positive-control.py` (`unforced`, 0 candidate
    /// fixtures, 2026-09-24). Its `a_console_weather_line_reaches_the_next_
    /// tick_and_the_wire` asserts the START_RAINING tick is the 20th.
    WeatherStep,
}

impl Arm {
    /// The `RUSTMC_T1_SABOTAGE` spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Xoroshiro => "xoroshiro",
            Self::LegacyLcg => "legacy-lcg",
            Self::Palette => "palette",
            Self::Storage => "storage",
            Self::PathAccuracy => "path-accuracy",
            Self::WeatherStep => "weather-step",
        }
    }

    /// Every arm, for [`report`] and for the parser's error message.
    #[must_use]
    pub const fn all() -> [Self; 6] {
        [
            Self::Xoroshiro,
            Self::LegacyLcg,
            Self::Palette,
            Self::Storage,
            Self::PathAccuracy,
            Self::WeatherStep,
        ]
    }

    /// Parse one name. `None` for anything else — see the type's note on why a
    /// typo must not read as an unarmed control.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::all().into_iter().find(|a| a.name() == s)
    }
}

/// The environment variable that arms this module. Named as a constant because
/// `ci/t1-sabotage.sh` and `ci/t0.sh` both mention it and a third copy of the
/// string is how the two drift apart.
pub const ENV: &str = "RUSTMC_T1_SABOTAGE";

/// Where the firing evidence goes. A FILE, and that is the whole point.
///
/// # This module got the §73.2 defect wrong on its first run
///
/// The firing counter originally announced itself with `eprintln!`, copying
/// `crates/rustmc-server/src/sabotage.rs`, where it is correct: that module
/// runs inside a long-lived *server process* whose stderr goes to a log
/// somebody greps. This one runs inside `cargo test`, which **captures a
/// test's output**, and the first sweep proved it: the `xoroshiro` arm turned
/// `golden_rng` from 6 passed to 3 failed, and `ci/t1-sabotage.sh` reported
/// `fired=0`, which this module defines as `UNVISITED` — "the gate never
/// reaches the arm's code". The arm had fired three times. The instrument
/// reported the exact opposite of what happened, for precisely the reason
/// §73.2 gives for the skip guard it replaced: the bytes never leave the
/// harness.
///
/// So the evidence takes a route `cargo` does not sit on. An append to a file
/// named by this variable needs no capture flag, no `--nocapture`, and no
/// cooperation from the harness — the same reasoning
/// `crates/rustmc-data/src/fixture.rs` used to choose a panic over a print.
/// Unset, it falls back to `eprintln!`, which is right for a server and wrong
/// for a test, and the runner always sets it.
pub const LOG_ENV: &str = "RUSTMC_T1_SABOTAGE_LOG";

/// Parse `RUSTMC_T1_SABOTAGE`, a comma-separated list of arm names.
///
/// # Errors
/// Any name that is not an [`Arm`]. The caller refuses rather than arming
/// nothing: an arm that fails to arm is indistinguishable from a gate that
/// never reaches its code, and telling those two apart is the whole point.
pub fn parse(spec: &str) -> Result<Vec<Arm>, String> {
    let mut out = Vec::new();
    for word in spec.split(',').map(str::trim).filter(|w| !w.is_empty()) {
        let arm = Arm::parse(word).ok_or_else(|| {
            let known: Vec<&str> = Arm::all().iter().map(|a| a.name()).collect();
            format!(
                "unknown sabotage arm {word:?}; known arms are {}",
                known.join(", ")
            )
        })?;
        out.push(arm);
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

static ARMED: OnceLock<Vec<Arm>> = OnceLock::new();
static FIRED: Mutex<Option<BTreeMap<&'static str, u64>>> = Mutex::new(None);

/// Arm from the environment. Idempotent; called from [`fires`] on first use so
/// that a test binary needs no setup hook.
///
/// # Panics
/// On a `RUSTMC_T1_SABOTAGE` that does not parse, and on one that is set while
/// the `t1-sabotage` feature is off — in which case every arm would be inert
/// and every gate would read as unforceable. A silent no-op there would make
/// this module claim exactly the kind of false green it exists to find.
fn armed() -> &'static Vec<Arm> {
    ARMED.get_or_init(|| {
        let spec = std::env::var(ENV).unwrap_or_default();
        if spec.trim().is_empty() {
            return Vec::new();
        }
        // **Not `assert!(cfg!(...))`, and not `const { assert!(..) }` either.**
        // `clippy::assertions_on_constants` is right that the condition is a
        // compile-time constant and wrong about the repair it suggests: a
        // `const` assertion would fail the BUILD whenever the feature is off,
        // which is every ordinary build of this server. What has to fail is
        // the RUN, and only the run that reached here — which is to say the
        // one where somebody set the variable. The early return above is the
        // other half of that: an unset spec is not an error.
        if !cfg!(feature = "t1-sabotage") {
            panic!(
                "{ENV}={spec:?} is set but this binary was not built with \
                 --features t1-sabotage. Every arm would be inert, so every \
                 gate run against it would report green and mean nothing. \
                 Build with `cargo test --features rustmc-data/t1-sabotage` \
                 or unset {ENV}."
            );
        }
        let arms = parse(&spec).unwrap_or_else(|e| panic!("{ENV}: {e}"));
        *FIRED.lock().expect("sabotage counter") = Some(BTreeMap::new());
        arms
    })
}

/// Is `arm` armed, and count it if so.
///
/// Compiles to a constant `false` without the `t1-sabotage` feature, so a
/// release build has no branch and no environment read.
#[inline]
#[must_use]
pub fn fires(arm: Arm) -> bool {
    if !cfg!(feature = "t1-sabotage") {
        return false;
    }
    if !armed().contains(&arm) {
        return false;
    }
    // **The first firing is announced, and that line is the evidence.** The
    // absence of a `T1-SABOTAGE-FIRST` line for an armed arm is the
    // `UNVISITED` verdict, and it needs no count to be true.
    if let Ok(mut g) = FIRED.lock() {
        if let Some(m) = g.as_mut() {
            let n = m.entry(arm.name()).or_insert(0);
            *n += 1;
            if *n == 1 {
                announce(arm);
            }
        }
    }
    true
}

/// The first-firing line, to a file if one was named. See [`LOG_ENV`].
fn announce(arm: Arm) {
    let line = format!("T1-SABOTAGE-FIRST {}\n", arm.name());
    if let Ok(path) = std::env::var(LOG_ENV) {
        if !path.trim().is_empty() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = f.write_all(line.as_bytes());
                return;
            }
        }
    }
    eprint!("{line}");
}

/// One line per armed arm, firing count included.
///
/// **`fired=0` on an armed arm is a verdict, not a warning.** It says the gate
/// never reaches the subsystem it claims to cover, so no implementation of it
/// could have moved the result.
#[must_use]
pub fn report() -> Vec<String> {
    let arms = armed();
    let counts = FIRED
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    arms.iter()
        .map(|a| {
            let n = counts.get(a.name()).copied().unwrap_or(0);
            format!("T1-SABOTAGE {} fired={n}", a.name())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_arm_round_trips_through_its_name() {
        for a in Arm::all() {
            assert_eq!(Arm::parse(a.name()), Some(a), "{}", a.name());
        }
    }

    /// **The refusals are the load-bearing half.** An arm that fails to arm
    /// fires zero times, and zero firings is this module's spelling of
    /// `UNVISITED`. A typo that armed nothing would therefore report every
    /// gate it was pointed at as testing dead code — the exact false green the
    /// module exists to find.
    #[test]
    fn anything_unrecognised_is_a_refusal_and_not_an_unarmed_arm() {
        assert!(parse("xoroshiro").is_ok());
        assert!(parse("Xoroshiro").is_err());
        assert!(parse("xoroshiroo").is_err());
        assert!(parse("xoroshiro,nosuch").is_err());
        assert!(parse("legacy_lcg").is_err(), "the spelling is legacy-lcg");
    }

    #[test]
    fn an_empty_spec_arms_nothing_and_is_not_an_error() {
        assert_eq!(parse("").expect("parses").len(), 0);
        assert_eq!(parse("  , ,").expect("parses").len(), 0);
    }

    #[test]
    fn a_list_arms_each_name_once() {
        let a = parse("palette,xoroshiro,palette").expect("parses");
        assert_eq!(a.len(), 2);
        assert!(a.contains(&Arm::Palette) && a.contains(&Arm::Xoroshiro));
    }

    /// The default build has no arms and cannot be given any. This is the test
    /// that would fail if the feature were ever added to a default set.
    #[test]
    #[cfg(not(feature = "t1-sabotage"))]
    fn without_the_feature_nothing_fires() {
        for a in Arm::all() {
            assert!(
                !fires(a),
                "{} fired in a build without the feature",
                a.name()
            );
        }
    }
}
