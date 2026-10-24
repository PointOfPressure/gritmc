//! A counterfactual server, for asking a check what would have made it red.
//!
//! # The question this exists to answer
//!
//! `ci/corpus-falsifiable.py` mutates a **recording** and asks whether the
//! comparator noticed. That is a real question and the ledger it produces is
//! real evidence, but it is evidence about the comparator, not about the
//! world: every recording has bytes at every step, so mutating those bytes
//! moves the verdict of very nearly anything. The ledger reads 108 of 108 and
//! the module docstring of the sweep says so in as many words — "axis A finds
//! no live defect in the corpus today".
//!
//! The question nobody had asked the corpus is the one `docs/AGENT-BRIEF.md`
//! asks everything else: **what recorded difference would turn this check
//! red, and can this server produce it?** A mutated byte is not an answer,
//! because no implementation emits it. A *wrong implementation* is.
//!
//! So this module is a small set of named wrong implementations, each one the
//! shape of a subsystem that was never written:
//!
//! ```text
//!   placement    `getStateForPlacement` returns the block's default state
//!   use          `useWithoutItem` passes instead of acting
//!   neighbors    `updateNeighborsAt` visits nobody
//!   sched        a scheduled tick never comes due
//!   blockentity  the block entity is created but never described
//!   menu         the screen never opens
//!   click        a container click changes nothing
//! ```
//!
//! Each is what the server looked like before somebody ported that subsystem,
//! which is exactly the difference a check claiming to cover it must be able
//! to see. A check that stays green with its subsystem sabotaged is a check
//! that would have passed before the feature existed.
//!
//! # Scoping, and why it is by position
//!
//! One recording is the budget — recordings queue several deep and the gate
//! contends for the same lock — so every control has to ride in the same run.
//! Sabotaging a subsystem globally would redden most of the corpus at once
//! and confound every reading with every other.
//!
//! `tools/oracle-runner/src/record.rs` pins scenario *n* to the workspace at
//! absolute `x = -(n + 1) * 32`, deliberately, so that position-dependent
//! packet order is comparable between servers. That pinning is also an
//! address: a block position, or an acting player's position, names the
//! scenario it belongs to and nothing else. So each sabotage is armed for an
//! explicit list of workspace indices and is inert in every other, and a
//! single run carries as many independent controls as there are scenarios.
//!
//! # The firing counter is half the instrument, not diagnostics
//!
//! A check that stays green under its own sabotage has failed for one of
//! three reasons, and they are not the same finding:
//!
//! * the sabotaged code **never ran** — the scenario's steps do not reach it,
//!   so no implementation of it could have changed the recording. `UNVISITED`.
//! * it ran, and the world it ran in could not express the difference —
//!   a daylight detector on a superflat measures `power = 0` whether or not
//!   anything recomputes it. `SUBSTRATE`.
//! * it ran, the difference reached the wire, and the check's projection
//!   discarded it. `VACUOUS`.
//!
//! Only the first is distinguishable without a second recording, and it is
//! distinguishable exactly because this module counts. [`report`] prints one
//! line per armed sabotage with the number of times its predicate matched;
//! `fired = 0` on an armed sabotage is the `UNVISITED` verdict and needs no
//! further evidence.
//!
//! # Why this cannot ship
//!
//! It is behind the `corpus-sabotage` cargo feature, off by default, and
//! every entry point below compiles to a constant `false` without it. A
//! server built the ordinary way has no branch to reach. `ci/t0.sh` asserts
//! the feature is absent from the default set, because a control that can be
//! turned on in production is not a control, it is a backdoor.

/// The subsystems a control can replace with a not-yet-written one.
///
/// Names are the `RUSTMC_SABOTAGE` spelling. Kept as a closed enum rather
/// than free strings so that a typo in the env var is a refusal at parse time
/// and not a control that silently never arms — which would read as
/// `UNVISITED` for every check it was pointed at, and is precisely the
/// unfalsifiable shape this whole module exists to measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// `getStateForPlacement` returns the default state.
    Placement,
    /// `useWithoutItem` returns `Pass`.
    Use,
    /// `updateNeighborsAt` visits nobody.
    Neighbors,
    /// A scheduled block tick never comes due.
    Sched,
    /// The block entity's description is never sent.
    BlockEntity,
    /// `openScreen` never reaches the client.
    Menu,
    /// A container click changes nothing.
    Click,
    /// The second block of a multi-block is never written, on the way up or
    /// on the way down — `setPlacedBy` writes nothing and
    /// `playerWillDestroy` takes nothing with it.
    ///
    /// **Separate from [`Kind::Neighbors`] because they are separate
    /// mechanisms that look alike in a recording.** A door's upper half and a
    /// lamp beside a lever are both "a block update at a position the step did
    /// not touch"; the first is one `setBlock` inside the placement and the
    /// second is power crossing between blocks. A control that could not tell
    /// them apart would attribute every two-block family's evidence to the
    /// redstone subsystem.
    Structure,
}

impl Kind {
    /// The `RUSTMC_SABOTAGE` spelling.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Use => "use",
            Self::Neighbors => "neighbors",
            Self::Sched => "sched",
            Self::BlockEntity => "blockentity",
            Self::Menu => "menu",
            Self::Click => "click",
            Self::Structure => "structure",
        }
    }

    /// Every kind, for `report` and for the parser's error message.
    #[must_use]
    pub const fn all() -> [Self; 8] {
        [
            Self::Placement,
            Self::Use,
            Self::Neighbors,
            Self::Sched,
            Self::BlockEntity,
            Self::Menu,
            Self::Click,
            Self::Structure,
        ]
    }

    /// Parse one name. `None` for anything else — see the type's note on why
    /// a typo must not read as an unarmed control.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Self::all().into_iter().find(|k| k.name() == s)
    }
}

/// The workspace a block position belongs to.
///
/// `record.rs` puts scenario *n*'s player at `x = -(n + 1) * 32` and every
/// position a scenario names is a small positive offset from there, so the
/// nearest multiple of 32 recovers *n*. Rounding rather than truncating,
/// because `-29` and `-32` are both workspace 0 and integer division sends
/// them to different answers.
///
/// `None` for `x >= 0`: the spawn workspace is nobody's scenario, and the
/// walk to the first one crosses it.
#[must_use]
pub fn workspace_of(x: i32) -> Option<u32> {
    if x >= 0 {
        return None;
    }
    let n = (-x + 16) / 32 - 1;
    u32::try_from(n).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_is_the_nearest_multiple_of_thirty_two() {
        // scenario 0 sits at -32 and writes at -32..-28
        assert_eq!(workspace_of(-32), Some(0));
        assert_eq!(workspace_of(-29), Some(0));
        assert_eq!(workspace_of(-28), Some(0));
        assert_eq!(workspace_of(-64), Some(1));
        assert_eq!(workspace_of(-61), Some(1));
        assert_eq!(workspace_of(-96), Some(2));
        // the spawn workspace belongs to no scenario
        assert_eq!(workspace_of(0), None);
        assert_eq!(workspace_of(5), None);
    }

    #[test]
    fn a_typo_in_a_kind_is_not_an_unarmed_control() {
        assert_eq!(Kind::parse("placement"), Some(Kind::Placement));
        assert_eq!(Kind::parse("Placement"), None);
        assert_eq!(Kind::parse("placements"), None);
        assert_eq!(Kind::parse(""), None);
    }

    #[test]
    fn every_kind_round_trips_through_its_name() {
        for k in Kind::all() {
            assert_eq!(Kind::parse(k.name()), Some(k), "{}", k.name());
        }
    }
}

// --------------------------------------------------------------------------
// the armed set
// --------------------------------------------------------------------------

/// One armed control: a subsystem and the workspaces it is wrong in.
#[derive(Debug, Clone)]
pub struct Armed {
    pub kind: Kind,
    /// Workspace indices, or empty for "every workspace".
    pub workspaces: Vec<u32>,
}

impl Armed {
    /// Is this control armed for `ws`? An empty list is every workspace.
    #[must_use]
    pub fn covers(&self, ws: u32) -> bool {
        self.workspaces.is_empty() || self.workspaces.contains(&ws)
    }
}

/// Parse `RUSTMC_SABOTAGE`.
///
/// ```text
///   placement:3,7,12;use:45,46;neighbors:*
/// ```
///
/// `Err` on anything unrecognised, and the caller refuses to start. A control
/// that fails to arm is indistinguishable from a scenario that never reaches
/// its code, and the whole point of the run is to tell those two apart.
///
/// # Errors
/// A kind that is not a [`Kind`], a workspace that is not a number, or a
/// clause with no colon.
pub fn parse(spec: &str) -> Result<Vec<Armed>, String> {
    let mut out = Vec::new();
    for clause in spec.split(';').map(str::trim).filter(|c| !c.is_empty()) {
        let (name, list) = clause
            .split_once(':')
            .ok_or_else(|| format!("clause {clause:?} has no `:`"))?;
        let kind = Kind::parse(name.trim())
            .ok_or_else(|| format!("unknown sabotage {:?}", name.trim()))?;
        let list = list.trim();
        let workspaces = if list == "*" {
            Vec::new()
        } else {
            list.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<u32>().map_err(|_| format!("bad workspace {s:?}")))
                .collect::<Result<Vec<_>, _>>()?
        };
        if list != "*" && workspaces.is_empty() {
            return Err(format!("clause {clause:?} arms no workspace"));
        }
        out.push(Armed { kind, workspaces });
    }
    Ok(out)
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn a_clause_names_a_kind_and_its_workspaces() {
        let a = parse("placement:3,7;use:45").expect("parses");
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].kind, Kind::Placement);
        assert_eq!(a[0].workspaces, vec![3, 7]);
        assert_eq!(a[1].kind, Kind::Use);
        assert_eq!(a[1].workspaces, vec![45]);
    }

    #[test]
    fn a_star_is_every_workspace() {
        let a = parse("neighbors:*").expect("parses");
        assert!(a[0].workspaces.is_empty());
        assert!(a[0].covers(0) && a[0].covers(999));
    }

    #[test]
    fn an_explicit_list_covers_only_itself() {
        let a = parse("placement:3").expect("parses");
        assert!(a[0].covers(3));
        assert!(!a[0].covers(4));
    }

    /// **The refusals are the load-bearing half.** A control that fails to
    /// arm fires zero times, and zero firings is this instrument's spelling
    /// of `UNVISITED`. A typo that armed nothing would therefore report every
    /// check it was pointed at as testing dead code — the exact false green
    /// the module exists to find.
    #[test]
    fn anything_unrecognised_is_a_refusal_and_not_an_unarmed_control() {
        assert!(parse("placemnt:3").is_err());
        assert!(parse("placement").is_err());
        assert!(parse("placement:").is_err());
        assert!(parse("placement:three").is_err());
        assert!(parse("placement:-1").is_err());
    }

    #[test]
    fn an_empty_spec_arms_nothing_and_is_not_an_error() {
        assert!(parse("").expect("parses").is_empty());
        assert!(parse("  ;  ").expect("parses").is_empty());
    }
}

// --------------------------------------------------------------------------
// the live control
// --------------------------------------------------------------------------

/// The armed set, read once from `RUSTMC_SABOTAGE`.
static ARMED: std::sync::OnceLock<Vec<Armed>> = std::sync::OnceLock::new();

/// `(kind, workspace) -> times the predicate matched`. See the module note on
/// why the count is half the instrument.
static FIRED: std::sync::Mutex<Option<std::collections::BTreeMap<(&'static str, u32), u64>>> =
    std::sync::Mutex::new(None);

// The workspace the connection being served is standing in.
//
// Several sabotage sites are reached from code that has no block position —
// `useWithoutItem` takes a state and nothing else, a container click names a
// slot — and plumbing a position through them to serve a test would change
// the shape of the thing under test. The dispatch loop sets this from the
// acting player's own `x` before it handles a frame, which is exact: the
// recorder walks both players into the workspace before the scenario starts
// and neither leaves it.
thread_local! {
    static HERE: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// Arm from the environment. Called once at startup.
///
/// # Errors
/// A `RUSTMC_SABOTAGE` that does not parse. The caller must refuse to start:
/// see [`parse`]'s note on why an unarmed control is worse than no run.
pub fn arm_from_env() -> Result<usize, String> {
    let spec = std::env::var("RUSTMC_SABOTAGE").unwrap_or_default();
    if spec.trim().is_empty() {
        let _ = ARMED.set(Vec::new());
        return Ok(0);
    }
    if !cfg!(feature = "corpus-sabotage") {
        return Err("RUSTMC_SABOTAGE is set but this binary was not built with \
                    --features corpus-sabotage — every control would be inert \
                    and every check would read UNVISITED"
            .into());
    }
    let armed = parse(&spec)?;
    let n = armed.len();
    *FIRED.lock().expect("sabotage counter") = Some(std::collections::BTreeMap::new());
    let _ = ARMED.set(armed);
    Ok(n)
}

/// Record where the connection being served is standing.
#[inline]
pub fn here_is(x: f64) {
    if cfg!(feature = "corpus-sabotage") {
        #[allow(clippy::cast_possible_truncation)]
        let ws = workspace_of(x.floor() as i32);
        HERE.with(|h| h.set(ws));
    }
}

/// Is `kind` armed for the workspace containing `x`, and count it if so.
#[inline]
#[must_use]
pub fn fires_at(kind: Kind, x: i32) -> bool {
    fires_in(kind, workspace_of(x))
}

/// Is `kind` armed for the workspace this thread is serving, and count it?
#[inline]
#[must_use]
pub fn fires_here(kind: Kind) -> bool {
    fires_in(kind, HERE.with(std::cell::Cell::get))
}

#[inline]
fn fires_in(kind: Kind, ws: Option<u32>) -> bool {
    if !cfg!(feature = "corpus-sabotage") {
        return false;
    }
    let Some(ws) = ws else { return false };
    let Some(armed) = ARMED.get() else {
        return false;
    };
    if !armed.iter().any(|a| a.kind == kind && a.covers(ws)) {
        return false;
    }
    // **The first firing is announced, and that line is the evidence.** A
    // corpus run ends by killing the server, so a report printed at shutdown
    // is a report nobody reads; and the reading that matters is binary —
    // did this control ever reach live code in this workspace? The absence of
    // a `SABOTAGE-FIRST` line for an armed `(kind, workspace)` is the
    // `UNVISITED` verdict, and it needs no count to be true.
    if let Ok(mut g) = FIRED.lock() {
        if let Some(m) = g.as_mut() {
            let n = m.entry((kind.name(), ws)).or_insert(0);
            *n += 1;
            if *n == 1 {
                eprintln!("SABOTAGE-FIRST {} ws={ws}", kind.name());
            }
        }
    }
    true
}

/// One line per armed `(kind, workspace)`, firing count included.
///
/// **`fired=0` on an armed control is a verdict, not a warning.** It says the
/// scenario in that workspace never reaches the subsystem the check claims to
/// cover, so no implementation of it could have moved the recording. That is
/// `UNVISITED`, and it is the one of the three green-and-meaningless classes
/// that a single run can settle on its own.
#[must_use]
pub fn report() -> Vec<String> {
    let Some(armed) = ARMED.get() else {
        return Vec::new();
    };
    let counts = FIRED
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default();
    let mut out = Vec::new();
    for a in armed {
        let mut seen: Vec<u32> = counts
            .keys()
            .filter(|(k, _)| *k == a.kind.name())
            .map(|(_, w)| *w)
            .collect();
        seen.extend(a.workspaces.iter().copied());
        seen.sort_unstable();
        seen.dedup();
        for ws in seen.iter().filter(|w| a.covers(**w)) {
            let n = counts.get(&(a.kind.name(), *ws)).copied().unwrap_or(0);
            out.push(format!("SABOTAGE {} ws={ws} fired={n}", a.kind.name()));
        }
        if seen.is_empty() {
            out.push(format!("SABOTAGE {} ws=* fired=0", a.kind.name()));
        }
    }
    out
}
