//! The redstone half of a block update: `updateNeighborsAt` and everything a
//! neighbour does when it hears.
//!
//! [`crate::shapes`] implements `updateNeighbourShapes`, which asks a
//! neighbour *what it becomes*. This is the other branch of the same `if` in
//! `Level.setBlock` — `updateNeighborsAt`, gated on `UPDATE_NEIGHBORS`
//! (flag 1) — which tells a neighbour that something changed and lets the
//! neighbour decide. Until this module existed `shapes.rs` carried the flag
//! all the way down to an empty branch, so a torch beside dust changed the
//! dust's *shape* and never its *power*.
//!
//! # The four layers, and why they are four
//!
//!   1. [`SignalTable`] — measured, from `data/vanilla/signal.json`. How much
//!      signal a state emits in each direction, weak and strong. 1,372 states
//!      carried, 1,250 refused, 29,744 measured silent — re-derived by
//!      `ci/refusal-ledger.py`, which also refuses if the three stop summing
//!      to 32,366.
//!
//!      **Those 1,250 are the worked example of why a table's refusal count is
//!      not the server's debt.** 1,215 of them are `redstone_wire` — the whole
//!      wire, bar the 81 states the table does answer — and this module hand
//!      ports every one of them below, including the `shouldSignal` field the
//!      table structurally cannot carry. The other 35 are the block-entity
//!      family and are the only part of the 1,250 that is genuinely owed.
//!      Quoting 1,250 as redstone debt counts 1,250 where 35 are owed.
//!      `ci/unported-inventory.py` counts the crate's debt and this counts the
//!      table's; `docs/cold-start/refusal-ledger.md` is why neither is a proxy
//!      for the other.
//!   2. [`Redstone`] — `SignalGetter`, vanilla's interface of default methods
//!      that turn "what does that state emit" into "how much signal is at this
//!      position". Ported whole; it is where the conductor rule lives.
//!   3. [`update_neighbors_at`] — `CollectingNeighborUpdater`, the queue that
//!      decides *when* each neighbour hears.
//!   4. The two blocks that react: the wire's power evaluator and the torch.
//!
//! # `REDSTONE_EXPERIMENTS` is off, and that deletes a whole parameter
//!
//! Every method in this family carries an `Orientation`, and
//! `ExperimentalRedstoneUtils.initialOrientation` returns **null** unless
//! `FeatureFlags.REDSTONE_EXPERIMENTS` is in the level's enabled set. A
//! vanilla world without that experimental pack therefore threads `null`
//! through every call, `RedStoneWireBlock.useExperimentalEvaluator` is false,
//! and `DefaultRedstoneWireEvaluator` — which reads neither the orientation
//! nor the boolean beside it — is the evaluator. Both parameters are dropped
//! here rather than carried as always-`None`, and this paragraph is the reason
//! they are missing. The experimental evaluator is a different algorithm, not
//! a variation on this one, and porting it is a separate item.
//!
//! # The two update orders are different and it is worth saying twice
//!
//! ```text
//! BlockBehaviour.UPDATE_SHAPE_ORDER   WEST, EAST, NORTH, SOUTH, DOWN, UP
//! NeighborUpdater.UPDATE_ORDER        WEST, EAST, DOWN, UP, NORTH, SOUTH
//! ```
//!
//! Neither is `Direction.values()`, they are not each other, and the two
//! branches of one `if` in `setBlock` use one each. See [`UPDATE_ORDER`].
//!
//! # What a wrong order can and cannot do here
//!
//! It cannot change the bytes on the wire, and knowing why is what keeps this
//! module honest about which of its details are load-bearing.
//! `ChunkHolder.broadcastChanges` accumulates a tick's changed positions in a
//! per-section `ShortOpenHashSet` and reads the *current* state back at the
//! tick boundary — see [`crate::broadcast`] and [`crate::shortset`]. So two
//! writes to one position collapse, and the order two different positions were
//! written in is discarded and replaced by the set's own iteration order. What
//! an order *can* change is the fixpoint, in any arrangement where a wire's
//! answer depends on a neighbour that has not been updated yet. The corpus has
//! no such arrangement today. The orders are reproduced anyway, because the
//! first contraption that needs them will not announce itself.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;

use crate::edits::Blocks;
use crate::interact::{BlockPos, Face};
use crate::placement::Around;
use crate::shapes::{ShapeTable, RECURSION_LIMIT, UPDATE_CLIENTS};

/// `NeighborUpdater.UPDATE_ORDER`.
///
/// **Not** `BlockBehaviour.UPDATE_SHAPE_ORDER`, which is
/// `WEST, EAST, NORTH, SOUTH, DOWN, UP`, and not `Direction.values()`, which
/// is `DOWN, UP, NORTH, SOUTH, WEST, EAST`. Three six-element orders live in
/// this codebase and no two of them are equal; the vertical pair sits third
/// here and fifth there, which is exactly the kind of difference that survives
/// a careless read.
pub const UPDATE_ORDER: [Face; 6] = [
    Face::West,
    Face::East,
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
];

/// `Direction.values()` — the order `getBestNeighborSignal`, `getDirectSignal`
/// and the wire evaluator's neighbour set all walk.
pub const DIRECTIONS: [Face; 6] = [
    Face::Down,
    Face::Up,
    Face::North,
    Face::South,
    Face::West,
    Face::East,
];

/// `Direction.Plane.VERTICAL` — `{UP, DOWN}`, in that order.
pub const VERTICAL: [Face; 2] = [Face::Up, Face::Down];

/// `MinecraftServer.getMaxChainedNeighborUpdates`, which
/// `CollectingNeighborUpdater` takes as its cap.
///
/// The integrated server hardcodes 1,000,000; the dedicated server reads
/// `max-chained-neighbor-updates` from `server.properties`. **Measured, not
/// assumed**: the reference server the gameplay oracle boots writes
/// `max-chained-neighbor-updates=1000000` into its own properties file, so
/// this is the number the thing we are compared against is running.
const MAX_CHAINED_NEIGHBOR_UPDATES: u32 = 1_000_000;

fn block_of(state: &str) -> &str {
    state.split('[').next().unwrap_or(state)
}

// ---------------------------------------------------------------------------
// 1. The measured table
// ---------------------------------------------------------------------------

/// What `data/vanilla/signal.json` says about one state.
#[derive(Debug, Clone, Copy)]
struct Row {
    source: bool,
    /// `getSignal`, indexed by [`Face::to_3d`].
    signal: [u8; 6],
    /// `getDirectSignal`, indexed by [`Face::to_3d`].
    direct: [u8; 6],
}

/// One lookup's outcome.
///
/// **`Refused` is not zero and must never be collapsed into it.** That
/// distinction is the whole reason this is an enum: `BlockBehaviour` defaults
/// all three signal methods to the falsy answer, so a block nobody ported and
/// a block that genuinely emits nothing are the same value and different
/// facts. `update_shape.json`'s 1,296 wire rows — every one of them the same
/// `DOWN -> air` branch — is what happens when that difference is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Vanilla was asked and answered.
    Measured(u8),
    /// Vanilla's rule read more of the world than its own state, so the table
    /// has no answer and a hand port must.
    Refused,
}

/// `BlockState.getSignal` and `getDirectSignal`, measured for every state.
///
/// # Keyed by the state string, deliberately
///
/// Every other extracted table in this crate is keyed by the global state id,
/// and this one is not. The reason is defect class #20: a test double whose
/// `state_id` returned `Some(1)` for every state made a table's id-keying
/// inert and two injected bugs survived on that alone. Every input to this
/// table arrives as a state string from [`Blocks::state_at`], so keying on the
/// string removes the round trip and with it the only place a double could
/// flatten the key. The ids are carried in the file and checked for
/// injectivity at load, so nothing is lost by not indexing on them.
///
/// # A state in neither map is a measurement, not a default
///
/// `SignalGolden` emits three disjoint sets — carried, refused, and *quiet*,
/// the 29,744 states it asked and which answered zero to all twelve questions.
/// Only the first two are written to the file, because the third is what the
/// port's fallthrough already does. [`Self::load`] checks
/// `carried + refused + quiet == states` so that "not in the table" cannot
/// quietly start meaning "the dump lost it".
#[derive(Debug)]
pub struct SignalTable {
    rows: BTreeMap<String, Row>,
    /// State string to why the harness could not answer it.
    refused: BTreeMap<String, Refusal>,
    states: usize,
    quiet: usize,
}

/// A state whose `getSignal` reads more of the world than itself.
///
/// `source` is carried even though `signal`/`direct` are not, because
/// `isSignalSource` takes no level and so never refuses — see
/// [`SignalTable::is_signal_source`].
#[derive(Debug, Clone)]
struct Refusal {
    reads: String,
    source: bool,
}

/// The one signal table this process serves.
///
/// `None` means no table was loaded, and the message says what that costs —
/// which is not "redstone is a bit wrong" but "nothing is a signal source", a
/// world in which a lever does nothing and looks exactly like a lever nobody
/// flipped.
///
/// # Path resolution
///
/// Tried first relative to the process's cwd, unchanged from before this
/// grew a fallback; only when that file is absent does it fall back to a
/// `CARGO_MANIFEST_DIR`-anchored path, which is what makes this reachable
/// under `cargo test` (whose cwd is the package directory, not the repo
/// root). Same shape and the same carried risk as
/// `crate::protocol_ids::shared()`: `docs/PARITY-ROADMAP.md` §34.
#[must_use]
pub fn shared() -> Option<&'static SignalTable> {
    static TABLE: std::sync::OnceLock<Option<SignalTable>> = std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| {
            const REL: &str = "data/vanilla/signal.json";
            let cwd_relative = std::path::Path::new(REL);
            let path: std::path::PathBuf = if cwd_relative.exists() {
                cwd_relative.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(REL)
            };
            match SignalTable::load(&path) {
                Ok(t) => {
                    let (carried, refused, quiet) = t.gaps();
                    eprintln!(
                        "signal table: {carried} states emit, {refused} refused to the hand \
                         ports, {quiet} measured silent"
                    );
                    Some(t)
                }
                Err(e) => {
                    eprintln!(
                        "no signal table ({e}) — no block is a redstone signal source; a lever \
                         will look exactly like a lever nobody flipped"
                    );
                    None
                }
            }
        })
        .as_ref()
}

impl SignalTable {
    /// Load `data/vanilla/signal.json`.
    ///
    /// # Errors
    /// If the file is missing, is not the expected shape, was dumped in a
    /// direction order this build does not index by, or does not account for
    /// every state it claims to have looked at.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        // Checked, not assumed — the same guard `ShapeTable::load` carries,
        // for the same reason: a transposed column order is silent everywhere
        // except in which face a thing powers.
        let order: Vec<&str> = doc
            .get("directionOrder")
            .and_then(serde_json::Value::as_array)
            .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_default();
        if order != ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"] {
            return Err(io::Error::other(format!(
                "signal.json is in direction order {order:?}, not Direction.values()"
            )));
        }
        let states = usize::try_from(
            doc.get("states")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| io::Error::other("signal.json has no state count"))?,
        )
        .map_err(io::Error::other)?;
        let quiet = usize::try_from(
            doc.get("quiet")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| io::Error::other("signal.json has no quiet count"))?,
        )
        .map_err(io::Error::other)?;

        let six = |v: Option<&serde_json::Value>| -> io::Result<[u8; 6]> {
            let a = v
                .and_then(serde_json::Value::as_array)
                .ok_or_else(|| io::Error::other("a signal row is not an array"))?;
            if a.len() != 6 {
                return Err(io::Error::other(format!(
                    "a signal row has {} columns, not six",
                    a.len()
                )));
            }
            let mut out = [0u8; 6];
            for (slot, value) in out.iter_mut().zip(a) {
                let n = value
                    .as_u64()
                    .ok_or_else(|| io::Error::other("a signal value is not a number"))?;
                if n > 15 {
                    return Err(io::Error::other(format!("a signal value is {n}, over 15")));
                }
                *slot = u8::try_from(n).map_err(io::Error::other)?;
            }
            Ok(out)
        };

        let mut rows = BTreeMap::new();
        let mut ids = BTreeSet::new();
        for entry in doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("signal.json has no table"))?
        {
            let state = entry
                .get("state")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("a signal row has no state"))?;
            let id = entry
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| io::Error::other("a signal row has no id"))?;
            if !ids.insert(id) {
                return Err(io::Error::other(format!(
                    "signal.json gives state id {id} twice — the key is not injective"
                )));
            }
            rows.insert(
                state.to_string(),
                Row {
                    source: entry
                        .get("source")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    signal: six(entry.get("signal"))?,
                    direct: six(entry.get("direct"))?,
                },
            );
        }

        let mut refused = BTreeMap::new();
        for entry in doc
            .get("unanswerable")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| io::Error::other("signal.json has no refusal list"))?
        {
            let state = entry
                .get("state")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| io::Error::other("a refusal has no state"))?;
            let id = entry
                .get("id")
                .and_then(serde_json::Value::as_i64)
                .ok_or_else(|| io::Error::other("a refusal has no id"))?;
            if !ids.insert(id) {
                return Err(io::Error::other(format!(
                    "signal.json gives state id {id} twice — the key is not injective"
                )));
            }
            refused.insert(
                state.to_string(),
                Refusal {
                    reads: entry
                        .get("reads")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unrecorded")
                        .to_string(),
                    source: entry
                        .get("source")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                },
            );
        }

        // The arithmetic that stops "absent from the table" drifting from
        // "measured silent" into "lost in the dump".
        let total = rows.len() + refused.len() + quiet;
        if total != states {
            return Err(io::Error::other(format!(
                "signal.json accounts for {total} states, not the {states} it claims to have \
                 looked at"
            )));
        }
        Ok(Self {
            rows,
            refused,
            states,
            quiet,
        })
    }

    /// `BlockState.getSignal(level, pos, dir)`, for a state whose answer does
    /// not depend on the world.
    #[must_use]
    pub fn signal(&self, state: &str, dir: Face) -> Signal {
        self.pick(state, dir, |row| row.signal)
    }

    /// `BlockState.getDirectSignal(level, pos, dir)`, same terms.
    #[must_use]
    pub fn direct_signal(&self, state: &str, dir: Face) -> Signal {
        self.pick(state, dir, |row| row.direct)
    }

    fn pick(&self, state: &str, dir: Face, of: impl Fn(&Row) -> [u8; 6]) -> Signal {
        let index = usize::try_from(dir.to_3d()).unwrap_or(0);
        if let Some(row) = self.rows.get(state) {
            return Signal::Measured(of(row)[index]);
        }
        if self.refused.contains_key(state) {
            return Signal::Refused;
        }
        // Measured silent. See the type docs: this is the third of the three
        // disjoint sets, and `load` has already checked the arithmetic.
        Signal::Measured(0)
    }

    /// `BlockState.isSignalSource()`, as this table measured it.
    ///
    /// Answered for refused states too — the refusal is about `getSignal`
    /// reading the world, and `isSignalSource` takes no level and never
    /// refuses. So this is total over the registry where the two signal
    /// lookups are not, and saying so is the point of it being its own method.
    ///
    /// [`Blocks::signal_source`] answers the same question from
    /// `block_props.json`. Two independently generated columns that must agree
    /// is a gate and one column is not — see
    /// [`Self::disagreements_with_block_props`].
    #[must_use]
    pub fn is_signal_source(&self, state: &str) -> bool {
        self.rows.get(state).map_or_else(
            || self.refused.get(state).is_some_and(|r| r.source),
            |r| r.source,
        )
    }

    /// `(carried, refused, quiet)`, for the startup line.
    #[must_use]
    pub fn gaps(&self) -> (usize, usize, usize) {
        (self.rows.len(), self.refused.len(), self.quiet)
    }

    /// How many states this table and `block_props.json` disagree about being
    /// a signal source, and the first few names.
    ///
    /// A cross-check rather than a use: the two columns come out of two
    /// separate harnesses over the same registry, so a disagreement is a
    /// transposition or a stale dump in one of them and nothing else. Zero is
    /// the only acceptable answer and the test asserts it.
    #[must_use]
    pub fn disagreements_with_block_props(&self, of: &dyn Fn(&str) -> Option<bool>) -> Vec<String> {
        let mut out = Vec::new();
        for (state, row) in &self.rows {
            if of(state).is_some_and(|props| props != row.source) {
                out.push(state.clone());
            }
        }
        out
    }

    /// The states the table could not answer, each with the reason. What a
    /// hand port owes.
    #[must_use]
    pub fn refusals(&self) -> BTreeMap<&str, &str> {
        self.refused
            .iter()
            .map(|(s, r)| (s.as_str(), r.reads.as_str()))
            .collect()
    }

    /// Every state the table answered, for a test that wants to sweep them.
    pub fn carried(&self) -> impl Iterator<Item = (&str, [u8; 6], [u8; 6])> {
        self.rows
            .iter()
            .map(|(s, r)| (s.as_str(), r.signal, r.direct))
    }

    #[must_use]
    pub const fn states(&self) -> usize {
        self.states
    }
}

// ---------------------------------------------------------------------------
// 2. `SignalGetter`
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.SignalGetter`, which is an interface of default
/// methods over `getBlockState`.
///
/// # `should_signal` is a field on the block, not a parameter
///
/// `RedStoneWireBlock` carries `private boolean shouldSignal`, initialised
/// `true` in its constructor and set `false` for exactly the length of
/// `getBlockSignal`:
///
/// ```java
/// public int getBlockSignal(Level level, BlockPos pos) {
///     this.shouldSignal = false;
///     int best = level.getBestNeighborSignal(pos);
///     this.shouldSignal = true;
///     return best;
/// }
/// ```
///
/// While it is false the wire's `getSignal`, `getDirectSignal` and
/// `isSignalSource` all answer the falsy value — and because it is a field on
/// the singleton block, that is true of **every wire in the world**, not just
/// the one being evaluated. That is the mechanism that stops a wire counting
/// another wire as a *source*: wire-to-wire propagation is done separately by
/// [`incoming_wire_signal`], which reads the `power` property directly and
/// subtracts one. Lose the flag and every dust in a line reads its neighbour
/// twice — once as a source at full strength and once as a wire at strength
/// minus one — and a line of dust never decays.
///
/// It is a [`Cell`] here rather than a parameter so that the reentrancy is the
/// same shape as vanilla's: `get_block_signal` flips it, everything reached
/// from inside sees the flipped value, and it is restored on the way out.
pub struct Redstone<'a> {
    blocks: &'a dyn Blocks,
    signals: &'a SignalTable,
    shapes: Option<&'a ShapeTable>,
    should_signal: Cell<bool>,
}

impl<'a> Redstone<'a> {
    #[must_use]
    pub fn new(
        blocks: &'a dyn Blocks,
        signals: &'a SignalTable,
        shapes: Option<&'a ShapeTable>,
    ) -> Self {
        Self {
            blocks,
            signals,
            shapes,
            should_signal: Cell::new(true),
        }
    }

    /// Hand a [`Around`] to `f`.
    ///
    /// A callback rather than a returned value because `Around` holds
    /// `&dyn Fn`s: the four closures are temporaries and cannot outlive the
    /// statement that makes them, so a function returning one does not
    /// compile. Every other construction site in this crate builds it inline
    /// as a local for the same reason.
    fn with_around<R>(&self, pos: BlockPos, f: impl FnOnce(&Around<'_>) -> R) -> R {
        let at = |p| self.blocks.state_at(p);
        let sturdy = |p, face| self.blocks.face_sturdy(p, face);
        let conductor = |p| self.blocks.redstone_conductor(p);
        let source = |p| self.blocks.signal_source(p);
        f(&Around {
            pos,
            at: &at,
            sturdy: &sturdy,
            conductor: &conductor,
            source: &source,
            instrument: &|_| None,
            state_id: &|_| None,
        })
    }

    /// `BlockState.getSignal(level, pos, dir)`.
    ///
    /// # The dispatch is on the block, and the table is a measurement of it
    ///
    /// Vanilla dispatches on the block *class*: `RedStoneWireBlock.getSignal`
    /// is the wire's method whatever the state. So the wire is asked its own
    /// rule here and the table is asked about everything else — rather than
    /// "table first, hand port on refusal", which was the first shape of this
    /// function and is wrong in a way that would have been invisible.
    ///
    /// The table refuses 1,215 of the wire's 1,296 states and answers the
    /// other 81 — the ones with `power=0`, which return before reading the
    /// world. Under a refusal-driven dispatch those 81 would take the measured
    /// path and the rest the ported one, so the hand port would be exercised
    /// only by the states the harness could not check and never by one it
    /// could. Dispatching on the block instead makes those 81 rows a **golden
    /// vector for the port**, which is what
    /// `the_ported_wire_rule_reproduces_every_measured_wire_row` spends them
    /// on.
    ///
    /// A refusal with no hand port is **reported**, once per distinct state,
    /// rather than silently answered zero. That is the difference between "we
    /// have not ported the comparator" and "the comparator outputs nothing",
    /// and the two are the same number.
    fn state_signal(&self, state: &str, pos: BlockPos, dir: Face) -> u8 {
        if crate::wire::is_wire(state) {
            return self.wire_get_signal(state, pos, dir);
        }
        // The one state family `signal.json` refuses: a comparator's answer
        // is its block entity's `output`, which lives in `crate::comparator`.
        if crate::comparator::is_comparator(state) {
            return crate::comparator::own_signal(pos, state, dir);
        }
        match self.signals.signal(state, dir) {
            Signal::Measured(v) => v,
            Signal::Refused => {
                unported(state, "getSignal");
                0
            }
        }
    }

    fn state_direct_signal(&self, state: &str, pos: BlockPos, dir: Face) -> u8 {
        if crate::wire::is_wire(state) {
            // `RedStoneWireBlock.getDirectSignal` is
            // `shouldSignal ? state.getSignal(level, pos, dir) : 0`, which
            // dispatches straight back into the wire's own `getSignal`. The
            // second `shouldSignal` test is vanilla's and is redundant;
            // reproduced rather than tidied.
            return if self.should_signal.get() {
                self.wire_get_signal(state, pos, dir)
            } else {
                0
            };
        }
        // `DiodeBlock.getDirectSignal` is `getSignal`, comparator included.
        if crate::comparator::is_comparator(state) {
            return crate::comparator::own_signal(pos, state, dir);
        }
        match self.signals.direct_signal(state, dir) {
            Signal::Measured(v) => v,
            Signal::Refused => {
                unported(state, "getDirectSignal");
                0
            }
        }
    }

    /// `RedStoneWireBlock.getSignal`, offsets 0-80 — the one rule the table
    /// cannot hold, because it asks what the wire would connect to and that is
    /// a function of thirteen positions.
    ///
    /// ```java
    /// if (!shouldSignal || dir == DOWN) return 0;          //  0-16
    /// int own = ownSignal(state, level, pos);              // 17-24
    /// if (own == 0) return 0;                              // 26-32
    /// return dir == UP                                     // 33-38
    ///     || getConnectionState(level, state, pos)
    ///          .getValue(PROPERTY_BY_DIRECTION.get(dir.getOpposite()))
    ///          .isConnected()                              // 41-73
    ///     ? own : 0;                                       // 76 / 79
    /// ```
    ///
    /// **`dir.getOpposite()`, and it is the whole of why the argument is not
    /// backwards.** `SignalGetter.getSignal(pos, dir)` is called as
    /// `getSignal(pos.relative(d), d)`, so by the time it reaches the block the
    /// direction points *away* from whoever asked. The wire therefore has to
    /// flip it to find the side facing the asker. Reading it as `dir` makes a
    /// line of dust power the wrong end of itself.
    ///
    /// The 81 wire states with `power=0` return at the `own == 0` line before
    /// touching the world, so `SignalGolden` could answer them and did. They
    /// are the golden vector this function is checked against rather than a
    /// second code path — see [`Self::state_signal`].
    #[must_use]
    pub fn wire_get_signal(&self, state: &str, pos: BlockPos, dir: Face) -> u8 {
        if !self.should_signal.get() || dir == Face::Down {
            return 0;
        }
        let own = crate::placement::property_of(state, "power")
            .and_then(|p| p.parse::<u8>().ok())
            .unwrap_or(0);
        if own == 0 {
            return 0;
        }
        if dir == Face::Up {
            return own;
        }
        let connected = self.with_around(pos, |a| crate::wire::connection_state(a, pos, state));
        if crate::wire::connects(&connected, dir.opposite()) {
            own
        } else {
            0
        }
    }

    /// `SignalGetter.getSignal(pos, dir)`.
    ///
    /// ```java
    /// BlockState s = getBlockState(pos);
    /// int signal = s.getSignal(this, pos, dir);
    /// return s.isRedstoneConductor(this, pos)
    ///      ? Math.max(signal, getDirectSignalTo(pos)) : signal;
    /// ```
    ///
    /// The conductor branch is the whole of "a block can carry a strong
    /// signal": a stone block beside dust is not itself a source, but if
    /// something is *strongly* powering that stone then the dust reads it.
    #[must_use]
    pub fn get_signal(&self, pos: BlockPos, dir: Face) -> u8 {
        let state = self.blocks.state_at(pos);
        let signal = self.state_signal(&state, pos, dir);
        if self.blocks.redstone_conductor(pos) {
            signal.max(self.get_direct_signal_to(pos))
        } else {
            signal
        }
    }

    /// `SignalGetter.getDirectSignal(pos, dir)`.
    #[must_use]
    pub fn get_direct_signal(&self, pos: BlockPos, dir: Face) -> u8 {
        let state = self.blocks.state_at(pos);
        self.state_direct_signal(&state, pos, dir)
    }

    /// `SignalGetter.getDirectSignalTo(pos)` — the six faces, each asked about
    /// the block on the other side of it, short-circuiting at 15.
    ///
    /// **Hand-unrolled in vanilla and in a fixed order**: DOWN, UP, NORTH,
    /// SOUTH, WEST, EAST, which is `Direction.values()`. It is not a loop over
    /// `DIRECTIONS` in the bytecode; it is six copies of the same four
    /// instructions with an early return after each. The order is
    /// unobservable — `max` is commutative and the early return only skips
    /// work — so this is written as the loop it means.
    #[must_use]
    pub fn get_direct_signal_to(&self, pos: BlockPos) -> u8 {
        let mut best = 0;
        for d in DIRECTIONS {
            best = best.max(self.get_direct_signal(d.relative(pos), d));
            if best >= 15 {
                return best;
            }
        }
        best
    }

    /// `SignalGetter.getBestNeighborSignal(pos)`.
    ///
    /// ```java
    /// int best = 0;
    /// for (Direction d : DIRECTIONS) {
    ///     int at = getSignal(pos.relative(d), d);
    ///     if (at >= 15) return 15;
    ///     if (at > best) best = at;
    /// }
    /// return best;
    /// ```
    ///
    /// `DIRECTIONS` here is `SignalGetter`'s own static field, initialised
    /// from `Direction.values()`.
    ///
    /// **The early return is `return 15`, not `return at`.** They are the same
    /// value only because the guard is `>=` and nothing emits more than 15;
    /// written as vanilla wrote it so that a future signal above 15 would be
    /// clamped here exactly as vanilla clamps it.
    #[must_use]
    pub fn get_best_neighbor_signal(&self, pos: BlockPos) -> u8 {
        let mut best = 0;
        for d in DIRECTIONS {
            let at = self.get_signal(d.relative(pos), d);
            if at >= 15 {
                return 15;
            }
            if at > best {
                best = at;
            }
        }
        best
    }

    /// `SignalGetter.hasSignal(pos, dir)`.
    #[must_use]
    pub fn has_signal(&self, pos: BlockPos, dir: Face) -> bool {
        self.get_signal(pos, dir) > 0
    }

    /// `SignalGetter.hasNeighborSignal(pos)` — six faces, any of them.
    ///
    /// Not `getBestNeighborSignal(pos) > 0` even though it always agrees with
    /// it: vanilla writes them separately and this one stops at the first
    /// non-zero rather than at the first 15.
    #[must_use]
    pub fn has_neighbor_signal(&self, pos: BlockPos) -> bool {
        DIRECTIONS
            .into_iter()
            .any(|d| self.has_signal(d.relative(pos), d))
    }

    /// `RedStoneWireBlock.getBlockSignal(level, pos)` — the best neighbour
    /// signal **with every wire in the world silenced**.
    #[must_use]
    pub fn get_block_signal(&self, pos: BlockPos) -> u8 {
        let was = self.should_signal.replace(false);
        let best = self.get_best_neighbor_signal(pos);
        self.should_signal.set(was);
        best
    }
}

/// Say once, per state, that a rule the table refused has no hand port.
///
/// The alternative is answering zero, which is what `BlockBehaviour` defaults
/// to and is therefore indistinguishable from the block working correctly and
/// emitting nothing. Thirty-five states reach this today — comparator,
/// jukebox, trapped chest and test block, all four of which keep their output
/// in a block entity — and every one of them is a real divergence that no
/// check in the corpus can currently see.
fn unported(state: &str, call: &str) {
    static SAID: std::sync::OnceLock<std::sync::Mutex<BTreeSet<String>>> =
        std::sync::OnceLock::new();
    let said = SAID.get_or_init(|| std::sync::Mutex::new(BTreeSet::new()));
    let mut said = said
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if said.insert(format!("{call}:{state}")) {
        eprintln!("redstone: no ported {call} for {state} — answering 0, which may be wrong");
    }
}

// ---------------------------------------------------------------------------
// 3. `CollectingNeighborUpdater`
// ---------------------------------------------------------------------------

/// One queued neighbour update — `MultiNeighborUpdate`.
///
/// # The other two arms are named rather than omitted
///
/// `CollectingNeighborUpdater` has three implementations of `NeighborUpdates`
/// and this ports one. The two that are not here are not oversights and their
/// callers say why:
///
/// * **`SimpleNeighborUpdate`** — one position, told once. Reached only
///   through the three-argument `Level.neighborChanged`, whose callers are
///   `Level.updateNeighbourForOutputSignal` (the comparator, whose 35 states
///   `signal.json` refuses) and the piston family.
/// * **`FullNeighborUpdate`** — the five-argument form, carrying a captured
///   state and a `movedByPiston` flag. Pistons only.
///
/// **A piston has now landed and neither of them arrived with it** — the line
/// that used to stand here said they would, and it was wrong about why they
/// were missing. [`crate::piston`] ports `PistonBaseBlock.neighborChanged`,
/// which is the *receiving* half: the piston is told about a change and
/// queues a block event. Both unported arms are on the *sending* half, which
/// vanilla reaches from `moveBlocks`'s `updateNeighborsAt` after the
/// structure has moved. That part of `moveBlocks` is outside this port's
/// boundary, so the arms are still unreached — but for a different and
/// narrower reason than "nothing here calls either", and the day
/// `moveBlocks`'s neighbour half lands they arrive with it.
#[derive(Debug, Clone)]
struct Update {
    source: BlockPos,
    block: String,
    skip: Option<Face>,
    idx: usize,
    /// `SimpleNeighborUpdate` rather than `MultiNeighborUpdate` — one
    /// `executeUpdate` at [`Self::source`] itself, and then done. [`skip`] and
    /// [`idx`] are unread when this is set.
    ///
    /// [`skip`]: Self::skip
    /// [`idx`]: Self::idx
    simple: bool,
}

/// `CollectingNeighborUpdater`'s three fields.
///
/// # Why a stack and not recursion
///
/// The traversal order the queue produces is the same order plain recursion
/// would: `runUpdates` resumes the item under the one it just finished, so an
/// update that spawns another runs the child to completion and then continues
/// its own remaining directions. The difference is *where the depth lives*.
/// The cap is [`MAX_CHAINED_NEIGHBOR_UPDATES`] — a million — and it counts
/// total updates rather than depth, so a contraption can legitimately be
/// thousands deep. On the JVM that is heap; as Rust recursion it is the
/// process stack, and a redstone clock would be a stack overflow rather than a
/// server that keeps its promises. Vanilla's shape is also simply the honest
/// one.
#[derive(Debug, Default)]
struct Updater {
    /// Vanilla's `ArrayDeque` used as a stack — `push`/`peek`/`pop` are all
    /// `addFirst`/`peekFirst`/`removeFirst`. A `Vec` working at its back is
    /// the same structure with the ends swapped.
    stack: Vec<Update>,
    added_this_layer: Vec<Update>,
    count: u32,
}

thread_local! {
    /// `Level.neighborUpdater`, which is a field on the level.
    ///
    /// A thread local rather than a process global because
    /// [`crate::broadcast::tick_lock`] already serialises every block-action
    /// handler, so at most one thread is ever inside this — and a per-thread
    /// queue cannot be corrupted by a thread that is not holding the lock,
    /// where a shared one could. The observable behaviour is the same as
    /// vanilla's single field for as long as that lock is held across a whole
    /// handler, which is the invariant `crate::broadcast` documents.
    static UPDATER: std::cell::RefCell<Updater> = std::cell::RefCell::new(Updater::default());
}

/// `Level.updateNeighborsAt(pos, block)`.
///
/// `ServerLevel` routes it to
/// `neighborUpdater.updateNeighborsAtExceptFromFacing(pos, block, null, null)`
/// — the skip direction is `null`, so all six run.
///
/// **The block argument is the OLD state's block.** `Level.setBlock` calls
/// this at offsets 146-153 with `aload 7`, which is `LevelChunk.setBlockState`'s
/// return value and therefore the state that *was* there — not `aload_2`, the
/// state just written, and not `aload 6`, the new block it computed four
/// instructions earlier and uses on the very next line for
/// `updateNeighbourForOutputSignal`. Both are in scope and it picks the old
/// one. Nothing in this server reads the argument today, because the only
/// receiver that compares it is behind the experimental evaluator; it is
/// carried correctly so that the first receiver that does is not wrong.
pub fn update_neighbors_at(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    block: &str,
    updates: &mut crate::shapes::Effects,
) {
    update_neighbors_at_except_from_facing(blocks, signals, shapes, pos, block, None, updates);
}

/// `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection(state)` —
/// the direction the block's own face points, which is **away from** the
/// surface it is stuck to.
///
/// ```text
///   $SwitchMap[CEILING.ordinal()] = 1 -> DOWN
///   $SwitchMap[FLOOR.ordinal()]   = 2 -> UP
///   default (WALL)                     -> state.getValue(FACING)
/// ```
///
/// The name reads backwards until the switch map is resolved: a **FLOOR**
/// lever's connected direction is **UP**, not DOWN. That is what makes
/// `LeverBlock.getDirectSignal(.., UP)` answer 15 for a floor lever, and so
/// what lets `getDirectSignalTo` on the block underneath find it.
#[must_use]
fn connected_direction(state: &str) -> Option<Face> {
    match crate::placement::property_of(state, "face")? {
        "ceiling" => Some(Face::Down),
        "floor" => Some(Face::Up),
        // `wall`, and any face this server has not heard of, is the default
        // arm — which reads FACING and is `None` when there is none.
        _ => crate::placement::property_of(state, "facing").and_then(Face::from_name),
    }
}

/// `LeverBlock.updateNeighbours` and `ButtonBlock.updateNeighbours`, which
/// are the same five instructions in two classes.
///
/// ```java
/// Direction back = getConnectedDirection(state).getOpposite();
/// level.updateNeighborsAt(pos, this, o);                  // 44-49
/// level.updateNeighborsAt(pos.relative(back), this, o);   // 52-62
/// ```
///
/// **Two calls, and only the first is what `Level.setBlock`'s
/// `UPDATE_NEIGHBORS` flag already gives you.** The second notifies the
/// neighbours of the block the lever is *attached to*, which is how a
/// strongly-powered block passes a signal on to things that are nowhere near
/// the lever.
///
/// That second call is the whole of scenario 400. Its rig is a floor lever
/// on a stone block with a wall torch on the stone's east face: the torch is
/// two positions from the lever and is never a neighbour of it, so the first
/// call cannot reach it. `360-a-lever-lights-a-lamp` passes today because
/// its lamp *is* a direct neighbour — which is exactly the shape of a check
/// that cannot distinguish one call from two.
///
/// `ExperimentalRedstoneUtils.initialOrientation` at offset 39 builds the
/// `Orientation` both calls carry. Nothing in this server reads one — see
/// [`update_neighbors_at`]'s note on the same argument — so it is not
/// reconstructed here.
pub fn face_attached_update_neighbours(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let Some(back) = connected_direction(state).map(Face::opposite) else {
        return;
    };
    // The first call is `Level.setBlock`'s, made for us by the
    // `UPDATE_NEIGHBORS` bit in the flags the use table carries. Repeating it
    // here would double every neighbour update a lever makes, so only the
    // second is issued and this comment is why.
    update_neighbors_at(
        blocks,
        signals,
        shapes,
        back.relative(pos),
        block_of(state),
        updates,
    );
}

/// Whether a block's `useWithoutItem` ends in
/// [`face_attached_update_neighbours`].
///
/// `LeverBlock.pull` and `ButtonBlock.press` each call their own copy and the
/// two bodies are identical, so this is a two-class question.
///
/// **The button half is `instanceof ButtonBlock` out of the class extract,
/// never "the name ends in `_button`"** — the same rule `crate::ticks` uses
/// to decide whether a scheduled tick is a button's and `crate::world_send`
/// uses to decide whether a click sounds like one. A suffix test is wrong in
/// both directions at once: it would take in anything a future version named
/// `*_button` without making it a `ButtonBlock`, and it silently answers
/// `false` for every button if a version ever renames one. `LeverBlock` is
/// its own class with one block, so that half is a name.
///
/// `classes` is `None` in a tree with no class extract, and then only the
/// lever is served. That is a visible under-answer rather than a guess: a
/// button would stop updating its support's neighbours, which is a
/// divergence, where a suffix guess would look right until it was not.
#[must_use]
pub fn attaches_and_updates_twice(
    classes: Option<&crate::placedby::PlacedByTable>,
    state: &str,
) -> bool {
    let block = block_of(state);
    block == "minecraft:lever" || classes.is_some_and(|c| c.is_a(block, crate::ticks::BUTTON_BLOCK))
}

/// `Level.updateNeighborsAtExceptFromFacing(pos, block, skip, orientation)`.
///
/// Nothing in this server passes a skip direction — `updateNeighborsAt` is the
/// `null` case and is the only caller today. It is ported with the skip
/// because the skip is not a filter applied at the end: it is handled *twice*,
/// once in `MultiNeighborUpdate`'s constructor for the case where the skipped
/// direction is at index 0 and once in `runNext` for every other index. Two
/// places is exactly how a skip gets half implemented, and the block that will
/// need this — the repeater, which does not tell the neighbour it is facing —
/// would have found out by being subtly wrong rather than by not compiling.
pub fn update_neighbors_at_except_from_facing(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    block: &str,
    skip: Option<Face>,
    updates: &mut crate::shapes::Effects,
) {
    // The corpus positive control: `updateNeighborsAt` visiting nobody, which
    // is the server before power crossed from one block to another. A lamp
    // that does not light, an inverter that does not invert, a piston that
    // does not hear its lever. Scoped by the position being updated rather
    // than by the acting thread, because a neighbour update can be reached
    // from the tick loop as well as from a packet. See `crate::sabotage`.
    if crate::sabotage::fires_at(crate::sabotage::Kind::Neighbors, pos.0) {
        return;
    }

    let Some(signals) = signals else { return };
    // `MultiNeighborUpdate`'s constructor, offsets 30-53: if the *first*
    // entry of UPDATE_ORDER is the skipped direction, step past it before the
    // update is ever queued. `runNext` handles every later index.
    let idx = usize::from(skip == Some(UPDATE_ORDER[0]));
    add_and_run(
        blocks,
        signals,
        shapes,
        Update {
            source: pos,
            block: block.to_string(),
            skip,
            idx,
            simple: false,
        },
        updates,
    );
}

/// `Level.neighborChanged(pos, block, orientation)` ->
/// `CollectingNeighborUpdater.neighborChanged`, offsets 0-15, which wraps a
/// `SimpleNeighborUpdate` and hands it to the same `addAndRun` every other
/// update goes through.
///
/// **One position, told once** — not that position's six neighbours. This is
/// the arm `crate::power`'s own header used to name as unported; the block
/// that needed it is the diode, whose `updateNeighborsInFront` calls this and
/// `updateNeighborsAtExceptFromFacing` at the same position, in that order.
pub fn neighbor_changed(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    block: &str,
    updates: &mut crate::shapes::Effects,
) {
    let Some(signals) = signals else { return };
    add_and_run(
        blocks,
        signals,
        shapes,
        Update {
            source: pos,
            block: block.to_string(),
            skip: None,
            idx: 0,
            simple: true,
        },
        updates,
    );
}

/// `CollectingNeighborUpdater.addAndRun`, offsets 0-117.
///
/// ```java
/// boolean nested = this.count > 0;
/// boolean tooMany = this.maxChainedNeighborUpdates >= 0
///                && this.count >= this.maxChainedNeighborUpdates;
/// this.count++;
/// if (!tooMany) {
///     if (nested) this.addedThisLayer.add(update);
///     else        this.stack.push(update);
/// } else if (this.count - 1 == this.maxChainedNeighborUpdates) {
///     LOGGER.error("Too many chained neighbor updates. Skipping the rest. ...");
/// }
/// if (!nested) this.runUpdates();
/// ```
///
/// `count` is incremented **before** the cap is acted on and the log line
/// fires on exactly the update after the cap, which is why the test is
/// `count - 1 == max` rather than `>=`: one message, not one per dropped
/// update.
fn add_and_run(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    update: Update,
    updates: &mut crate::shapes::Effects,
) {
    let nested = UPDATER.with_borrow_mut(|u| {
        let nested = u.count > 0;
        let too_many = u.count >= MAX_CHAINED_NEIGHBOR_UPDATES;
        let at = update.source;
        u.count += 1;
        if too_many {
            if u.count - 1 == MAX_CHAINED_NEIGHBOR_UPDATES {
                eprintln!(
                    "Too many chained neighbor updates. Skipping the rest. First skipped \
                     position: {at:?}"
                );
            }
        } else if nested {
            u.added_this_layer.push(update);
        } else {
            u.stack.push(update);
        }
        nested
    });
    if !nested {
        run_updates(blocks, signals, shapes, updates);
    }
}

/// `CollectingNeighborUpdater.runUpdates`, offsets 0-188.
///
/// ```java
/// while (!(stack.isEmpty() && addedThisLayer.isEmpty())) {
///     for (int i = addedThisLayer.size() - 1; i >= 0; i--)
///         stack.push(addedThisLayer.get(i));
///     addedThisLayer.clear();
///     NeighborUpdates top = stack.peek();
///     while (addedThisLayer.isEmpty()) {
///         if (!top.runNext(level)) { stack.pop(); break; }
///     }
/// }
/// stack.clear(); addedThisLayer.clear(); count = 0;
/// ```
///
/// **The reverse push is what keeps the children in the order they were
/// added.** `push` is `addFirst`, so walking `addedThisLayer` backwards leaves
/// its element 0 on top. A forward loop would run a block's own six neighbour
/// updates back to front.
///
/// The clearing tail is vanilla's `finally`, and it is why one exception does
/// not leave a level with a half-drained queue for ever.
fn run_updates(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    updates: &mut crate::shapes::Effects,
) {
    loop {
        let empty = UPDATER.with_borrow_mut(|u| {
            if u.stack.is_empty() && u.added_this_layer.is_empty() {
                return true;
            }
            while let Some(item) = u.added_this_layer.pop() {
                u.stack.push(item);
            }
            false
        });
        if empty {
            break;
        }
        // `peek` then `runNext` then write the advanced item back, which is
        // what the JVM gets for free by mutating the object the deque holds.
        // Taking the item out and putting it back is the same thing as long as
        // nothing else touches the top of the stack while it runs — and
        // nothing does: a nested update goes to `added_this_layer`.
        while let Some(mut top) = UPDATER.with_borrow_mut(|u| u.stack.pop()) {
            let more = run_next(blocks, signals, shapes, &mut top, updates);
            let stalled = UPDATER.with_borrow_mut(|u| {
                if more {
                    u.stack.push(top);
                }
                !u.added_this_layer.is_empty()
            });
            if !more || stalled {
                break;
            }
        }
    }
    // Vanilla's `finally`, offsets 140-161 and again at 164-187 on the throw
    // path: clear both and zero the count. Unconditional, and that is the
    // point — the loop above already leaves both empty, so the only case this
    // covers is the one where something threw halfway through, and a level
    // whose queue kept a half-drained update would carry it into every later
    // tick.
    UPDATER.with_borrow_mut(|u| {
        u.stack.clear();
        u.added_this_layer.clear();
        u.count = 0;
    });
}

/// `NeighborUpdates.runNext(level)` for both arms — returns whether the item
/// has more to do.
///
/// `MultiNeighborUpdate.runNext` reads `UPDATE_ORDER[idx++]`, tells the block
/// one step that way, then skips the next entry if it is the skip direction,
/// and answers whether `idx` is still in range. The constructor does the same
/// skip once up front, which is how a skip direction sitting at index 0 is
/// handled.
fn run_next(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    update: &mut Update,
    updates: &mut crate::shapes::Effects,
) -> bool {
    // `SimpleNeighborUpdate.runNext`, offsets 0-28: one `executeUpdate` at the
    // update's **own** position — not at a neighbour of it — and `iconst_0`,
    // so it is never resumed. That is the whole arm.
    if update.simple {
        let block = update.block.clone();
        execute_update(blocks, signals, shapes, update.source, &block, updates);
        return false;
    }
    if update.idx >= UPDATE_ORDER.len() {
        return false;
    }
    let d = UPDATE_ORDER[update.idx];
    update.idx += 1;
    let at = d.relative(update.source);
    let block = update.block.clone();
    // `executeUpdate` at offset 103, and the skip advance at 106-142 **after**
    // it. Nothing reads `idx` from inside the update, so the two orders cannot
    // be told apart today; written in vanilla's order because "cannot be told
    // apart today" is a property of what is ported, not of the method.
    execute_update(blocks, signals, shapes, at, &block, updates);
    if update.idx < UPDATE_ORDER.len() && Some(UPDATE_ORDER[update.idx]) == update.skip {
        update.idx += 1;
    }
    update.idx < UPDATE_ORDER.len()
}

/// `NeighborUpdater.executeUpdate` -> `BlockState.handleNeighborChanged` ->
/// the block's own `neighborChanged`.
///
/// `BlockBehaviour.neighborChanged` is empty, so a block with no override
/// hears nothing and that is the correct answer rather than a gap.
///
/// # Twenty-eight blocks override it and two of them are here
///
/// Grepping the constant pool of every class under
/// `net/minecraft/world/level/block/` for the name returns 28. The two ported
/// are the ones checks `100` and `370` are about. The other 26 are
/// `AbstractSkullBlock`, `BigDripleafBlock`, `BaseRailBlock`, `BellBlock`,
/// `CactusBlock`, `DiodeBlock`, `DispenserBlock`, `DoorBlock`,
/// `FrostedIceBlock`, `HopperBlock`, `IceBlock`, `ObserverBlock`,
/// `CrafterBlock`, `FenceGateBlock`, `SpongeBlock`, `StructureBlock`,
/// `LiquidBlock`, `TestBlock`, `TrapDoorBlock`, `CopperBulbBlock`,
/// `CommandBlock`, `DetectorRailBlock`, `NoteBlock`, `RedstoneLampBlock`,
/// `ShelfBlock` and `TntBlock`.
///
/// **Not reaching them is not a regression and it is a new kind of gap.**
/// Before this module the whole path was dead, so all 28 did nothing; now two
/// work and 26 still do nothing. What changed is that the difference is now
/// *reachable*: a door beside a lever is one branch away from being right
/// rather than a whole mechanism away, and the list above is what that branch
/// costs. `RedstoneLampBlock` and `DoorBlock` are the two the corpus could see
/// today.
fn execute_update(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    source_block: &str,
    updates: &mut crate::shapes::Effects,
) {
    let state = blocks.state_at(pos);
    if crate::wire::is_wire(&state) {
        wire_neighbor_changed(blocks, signals, shapes, pos, &state, updates);
    } else if is_torch(&state) {
        torch_neighbor_changed(blocks, signals, pos, &state, updates);
    } else if crate::comparator::is_comparator(&state) {
        crate::comparator::neighbor_changed(blocks, signals, shapes, pos, &state, updates);
    } else if is_diode(&state) {
        diode_neighbor_changed(blocks, signals, pos, &state, updates);
    } else if block_of(&state) == "minecraft:redstone_lamp" {
        lamp_neighbor_changed(blocks, signals, shapes, pos, &state, updates);
    } else if crate::dispenser::is_dispenser(&state) {
        let power = Redstone::new(blocks, signals, shapes);
        crate::dispenser::neighbor_changed(&power, blocks, shapes, pos, &state, updates);
    } else if crate::dispenser::is_crafter(&state) {
        let power = Redstone::new(blocks, signals, shapes);
        crate::dispenser::crafter_neighbor_changed(&power, blocks, shapes, pos, &state, updates);
    } else if block_of(&state) == "minecraft:hopper" {
        let power = Redstone::new(blocks, signals, shapes);
        crate::dispenser::hopper_check_powered_state(&power, blocks, shapes, pos, &state, updates);
    } else if crate::piston::is_piston(block_of(&state)) {
        // `PistonBaseBlock.neighborChanged` — queue a block event, change
        // nothing. See `crate::piston::neighbor_changed`.
        crate::piston::neighbor_changed(
            blocks,
            Some(signals),
            pos,
            &state,
            crate::piston::WORLD_MIN_Y,
            crate::piston::WORLD_MAX_Y,
        );
    } else if block_of(&state) == "minecraft:piston_head" {
        crate::pistonmove::head_neighbor_changed(
            blocks,
            Some(signals),
            shapes,
            pos,
            &state,
            updates,
        );
    }
    // `LiquidBlock.neighborChanged` — the same lines as its `onPlace`, and
    // reached from the same `Level.updateNeighborsAt`. A separate statement
    // rather than another `else if`: a `LiquidBlock` is none of the blocks
    // above, so the dispatch is exclusive by block anyway, and keeping it out
    // of the chain stops a later redstone arm from swallowing it. **Without
    // it a pool never notices a wall being broken**, and lava never notices
    // the water that turns it to cobblestone.
    crate::fluid::on_place_or_neighbor_changed(blocks, shapes, pos, &state, updates);
    // The source block is carried to the receiver in vanilla and read by
    // exactly one of them — `RedStoneWireBlock.neighborChanged`'s
    // `block == this && useExperimentalEvaluator(level)`, whose right-hand
    // side is false here. It is threaded to this point rather than dropped at
    // the queue, because the receiver that reads it is one flag away.
    let _ = source_block;
}

/// `RedstoneTorchBlock` and its wall form. Both, because
/// `RedstoneWallTorchBlock extends RedstoneTorchBlock` and inherits
/// `neighborChanged` unchanged — only `hasNeighborSignal` and `getSignal`
/// differ, and the second of those is in the measured table.
fn is_torch(state: &str) -> bool {
    matches!(
        block_of(state),
        "minecraft:redstone_torch" | "minecraft:redstone_wall_torch"
    )
}

// ---------------------------------------------------------------------------
// 4a. The wire's power evaluator
// ---------------------------------------------------------------------------

/// `RedstoneWireEvaluator.getWireSignal(pos, state)` — the `power` property,
/// or 0 for anything that is not a wire.
///
/// **Read straight off the state and never through `getSignal`**, which is why
/// `should_signal` does not reach it and why a wire can see a wire at all
/// while `getBlockSignal` has every wire silenced.
fn wire_signal(state: &str) -> u8 {
    if crate::wire::is_wire(state) {
        crate::placement::property_of(state, "power")
            .and_then(|p| p.parse::<u8>().ok())
            .unwrap_or(0)
    } else {
        0
    }
}

/// `RedstoneWireEvaluator.getIncomingWireSignal(level, pos)`, offsets 0-165.
///
/// ```java
/// int best = 0;
/// for (Direction d : Plane.HORIZONTAL) {
///     BlockPos npos = pos.relative(d);                       //  32-38
///     BlockState n = level.getBlockState(npos);              //  40-46
///     best = Math.max(best, getWireSignal(npos, n));         //  48-60
///     BlockPos above = pos.above();                          //  61-65
///     if (n.isRedstoneConductor(level, npos)                 //  67-75
///         && !level.getBlockState(above)
///                  .isRedstoneConductor(level, above)) {     //  78-90
///         best = Math.max(best,
///             getWireSignal(npos.above(), ...));             //  93-116
///     } else if (!n.isRedstoneConductor(level, npos)) {      // 120-128
///         best = Math.max(best,
///             getWireSignal(npos.below(), ...));             // 131-154
///     }
/// }
/// return Math.max(0, best - 1);                              // 158-165
/// ```
///
/// # Three details a summary loses
///
/// * **`above` is `pos.above()`, the subject's, not `npos.above()`.** Offset
///   61 is `aload_2`, which is the method's own `pos`. The block over the
///   *wire* is what decides whether a signal may come down from a step up —
///   the same `canGoUp` gate `RedStoneWireBlock.getConnectingSide` applies to
///   the connection shape, so power and shape agree about when dust climbs.
///   Reading it as the neighbour's `above` puts the gate on the wrong column
///   and lets dust draw power through a ceiling it cannot connect through.
/// * **the two branches are exclusive and their condition is the same test
///   twice.** A conductor beside the wire blocks the *lower* path and opens
///   the *upper* one; a non-conductor does the reverse. There is no branch in
///   which both are read.
/// * **the subtraction is at the end and it is clamped at zero.** One decay
///   per hop, applied once to the best of everything seen, not once per
///   neighbour.
fn incoming_wire_signal(power: &Redstone<'_>, pos: BlockPos) -> u8 {
    let mut best = 0u8;
    for d in crate::wire::HORIZONTAL {
        let npos = d.relative(pos);
        let n = power.blocks.state_at(npos);
        best = best.max(wire_signal(&n));
        let above = (pos.0, pos.1 + 1, pos.2);
        let n_conducts = power.blocks.redstone_conductor(npos);
        if n_conducts {
            if !power.blocks.redstone_conductor(above) {
                let up = (npos.0, npos.1 + 1, npos.2);
                best = best.max(wire_signal(&power.blocks.state_at(up)));
            }
        } else {
            let down = (npos.0, npos.1 - 1, npos.2);
            best = best.max(wire_signal(&power.blocks.state_at(down)));
        }
    }
    best.saturating_sub(1)
}

/// `DefaultRedstoneWireEvaluator.calculateTargetStrength(level, pos)`,
/// offsets 0-25.
///
/// ```java
/// int block = getBlockSignal(level, pos);
/// if (block == 15) return block;
/// return Math.max(block, getIncomingWireSignal(level, pos));
/// ```
///
/// **The early return is `== 15`, not `>= 15`**, and it is what stops a wire
/// beside a full-strength source from paying for the twelve reads
/// [`incoming_wire_signal`] costs. Behaviourally identical here because
/// nothing exceeds 15; written as vanilla wrote it.
fn calculate_target_strength(power: &Redstone<'_>, pos: BlockPos) -> u8 {
    let block = power.get_block_signal(pos);
    if block == 15 {
        return block;
    }
    block.max(incoming_wire_signal(power, pos))
}

/// `DefaultRedstoneWireEvaluator.updatePowerStrength`, offsets 0-162.
///
/// ```java
/// int target = calculateTargetStrength(level, pos);
/// if (state.getValue(POWER) != target) {
///     if (level.getBlockState(pos) == state)                        // 26-31
///         level.setBlock(pos, state.setValue(POWER, target), 2);    // 35-56
///     Set<BlockPos> set = Sets.newHashSet();                        // 57-60
///     set.add(pos);                                                 // 62-70
///     for (Direction d : Direction.values()) set.add(pos.relative(d));
///     for (BlockPos p : set) level.updateNeighborsAt(p, wireBlock); // 118-159
/// }
/// ```
///
/// # Three things worth naming
///
/// * **the whole body is inside `power != target`.** A wire whose strength did
///   not change tells nobody anything, which is what stops a ring of dust
///   updating for ever.
/// * **the write is flag 2, `UPDATE_CLIENTS` alone.** No `UPDATE_NEIGHBORS`,
///   because this method is about to do the neighbour updates itself and with
///   a set that is not the six faces. `UPDATE_KNOWN_SHAPE` is *not* set
///   either, so a power change still runs a full shape cascade.
/// * **`level.getBlockState(pos) == state` is a reference comparison on an
///   interned state**, and it is a real guard: by the time a queued update
///   runs, the position may hold something else entirely, and vanilla then
///   updates the neighbours without writing. Reproduced as string equality,
///   which is the same predicate because block states are interned and equal
///   states are one object.
///
/// # The neighbour set is a `java.util.HashSet` and its order is reproduced
///
/// See [`hash_set_order`]. Seven positions, never resized, and the order they
/// come out in is a function of `BlockPos.hashCode`.
fn update_power_strength(
    power: &Redstone<'_>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let target = calculate_target_strength(power, pos);
    let own = wire_signal(state);
    if own == target {
        return;
    }
    if power.blocks.state_at(pos) == state {
        let to = crate::placement::with_property(state, "power", &target.to_string());
        crate::shapes::set_block(
            power.blocks,
            power.shapes,
            pos,
            &to,
            UPDATE_CLIENTS,
            RECURSION_LIMIT,
            updates,
        );
    }
    let mut set = vec![pos];
    for d in DIRECTIONS {
        set.push(d.relative(pos));
    }
    for p in hash_set_order(&set) {
        update_neighbors_at(
            power.blocks,
            Some(power.signals),
            power.shapes,
            p,
            "minecraft:redstone_wire",
            updates,
        );
    }
}

/// `Vec3i.hashCode()` — `(y + z * 31) * 31 + x`, in Java's wrapping `int`.
#[must_use]
pub const fn block_pos_hash((x, y, z): BlockPos) -> i32 {
    y.wrapping_add(z.wrapping_mul(31))
        .wrapping_mul(31)
        .wrapping_add(x)
}

/// The order a `java.util.HashSet<BlockPos>` yields the seven positions
/// `updatePowerStrength` puts in it.
///
/// # Why a hash set's internal order is reproduced here too
///
/// The same reason [`crate::shortset`] reproduces fastutil's: it is not an
/// implementation detail if something downstream can see it. Here the
/// something is the *fixpoint* rather than the packet — the seven positions
/// are told in this order, each of them can rewrite a wire, and a wire that is
/// told before its neighbour settles gets a different answer from one told
/// after. The corpus has no arrangement that separates the two orders, so this
/// is fidelity paid forward rather than a fix.
///
/// # The algorithm, and why it is exactly three lines
///
/// `HashMap` spreads a key with `h ^ (h >>> 16)` and indexes with
/// `(n - 1) & spread`, appending collisions to the end of a bucket's list.
/// Iteration walks buckets 0 upward and each bucket front to back. The default
/// table is 16 slots and resizes at `16 * 0.75 = 12`; `updatePowerStrength`
/// inserts a position and its six neighbours, which is **seven distinct
/// entries and cannot resize**, so the table stays 16 and the treeify
/// threshold of 8 in one bucket is out of reach as well. All of that is why
/// this needs no `HashMap` — only its indexing.
#[must_use]
pub fn hash_set_order(positions: &[BlockPos]) -> Vec<BlockPos> {
    const CAPACITY: usize = 16;
    debug_assert!(
        positions.len() <= 12,
        "more than twelve entries resizes the table and this is not that function"
    );
    let mut buckets: Vec<Vec<BlockPos>> = vec![Vec::new(); CAPACITY];
    for &p in positions {
        let h = block_pos_hash(p);
        #[expect(
            clippy::cast_sign_loss,
            reason = "Java's `(n - 1) & h` masks the sign off; the cast is the mask"
        )]
        let spread = (h ^ ((h as u32) >> 16) as i32) as u32;
        let index = (spread as usize) & (CAPACITY - 1);
        let bucket = &mut buckets[index];
        if !bucket.contains(&p) {
            bucket.push(p);
        }
    }
    buckets.into_iter().flatten().collect()
}

/// `RedStoneWireBlock.neighborChanged`, offsets 0-57.
///
/// ```java
/// if (level.isClientSide) return;
/// if (block == this && useExperimentalEvaluator(level)) return;  //  8-21
/// if (state.canSurvive(level, pos))                              // 22-28
///     updatePowerStrength(level, pos, state, orientation, false);
/// else { dropResources(state, level, pos); level.removeBlock(pos, false); }
/// ```
///
/// The second line is dead with the experimental flag off — `useExperimental`
/// is the whole of its right-hand side — so a wire told about itself still
/// re-evaluates here. That is not an oversight to optimise away: it is what
/// makes the seven-position set in [`update_power_strength`] converge in one
/// pass instead of needing a second one.
fn wire_neighbor_changed(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, shapes);
    if wire_can_survive(blocks, pos) {
        update_power_strength(&power, pos, state, updates);
    } else {
        crate::shapes::remove_block(blocks, shapes, pos, updates);
    }
}

/// `RedStoneWireBlock.canSurvive`, offsets 0-25 — `canSurviveOn` about the
/// block below, with the state read from the world rather than handed in.
///
/// The `updateShape` path passes the *neighbour state argument* to the same
/// helper; this one reads. The difference is [`crate::wire`]'s to document and
/// it is why the helper takes a state.
fn wire_can_survive(blocks: &dyn Blocks, pos: BlockPos) -> bool {
    let below = (pos.0, pos.1 - 1, pos.2);
    let state = blocks.state_at(below);
    blocks.face_sturdy(below, Face::Up) || block_of(&state) == "minecraft:hopper"
}

/// `RedStoneWireBlock.onPlace`, offsets 0-79.
///
/// ```java
/// if (oldState.is(this) || level.isClientSide) return;           //  0-19
/// updatePowerStrength(level, pos, state, null, true);            // 20-26
/// for (Direction d : Plane.VERTICAL)                             // 29-70
///     level.updateNeighborsAt(pos.relative(d), this);
/// updateNeighborsOfNeighboringWires(level, pos);                 // 73-78
/// ```
///
/// **The guard is on the OLD block, and it is `is(this)` — a wire replacing a
/// wire does nothing here.** That is what stops
/// [`update_power_strength`]'s own `setBlock` from re-entering this method for
/// every power change: the write goes wire-to-wire and returns at line one.
/// Drop the guard and a single dust placement recurses until the chain cap
/// stops it.
///
/// `Plane.VERTICAL` is `{UP, DOWN}` — up first. Only the two vertical
/// neighbours, because the four horizontal ones are what
/// `updateNeighborsOfNeighboringWires` is for and it reaches further than they
/// do.
/// The redstone half of `RedStoneWireBlock.affectNeighborsAfterRemoval`,
/// offsets 54-62: `updatePowerStrength(level, pos, state, null, false)` then
/// `updateNeighborsOfNeighboringWires(level, pos)`, with `state` the wire
/// that was just removed. `updatePowerStrength` guards its own write on
/// `getBlockState(pos) == state`, which is now false, so what survives of it
/// is the seven-position `updateNeighborsAt` sweep when the removed wire's
/// power differs from what the empty position would carry.
pub(crate) fn wire_after_removal(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, shapes);
    update_power_strength(&power, pos, state, updates);
    update_neighbors_of_neighboring_wires(blocks, signals, shapes, pos, updates);
}

pub fn wire_on_place(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    old: &str,
    updates: &mut crate::shapes::Effects,
) {
    if crate::wire::is_wire(old) {
        return;
    }
    let power = Redstone::new(blocks, signals, shapes);
    update_power_strength(&power, pos, state, updates);
    for d in VERTICAL {
        update_neighbors_at(
            blocks,
            Some(signals),
            shapes,
            d.relative(pos),
            "minecraft:redstone_wire",
            updates,
        );
    }
    update_neighbors_of_neighboring_wires(blocks, signals, shapes, pos, updates);
}

/// `RedStoneWireBlock.updateNeighborsOfNeighboringWires`, offsets 0-117.
///
/// ```java
/// for (Direction d : Plane.HORIZONTAL) checkCornerChangeAt(level, pos.relative(d));
/// for (Direction d : Plane.HORIZONTAL) {
///     BlockPos p = pos.relative(d);
///     if (level.getBlockState(p).isRedstoneConductor(level, p))
///          checkCornerChangeAt(level, p.above());
///     else checkCornerChangeAt(level, p.below());
/// }
/// ```
///
/// **Two separate loops over the same four directions, not one loop doing
/// both.** All four orthogonals are done before any diagonal is, and merging
/// them changes the order in which corners hear. The diagonal chosen is the
/// same up-or-down choice [`incoming_wire_signal`] makes, which is what keeps
/// "who is told" and "who is read" the same set.
fn update_neighbors_of_neighboring_wires(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut crate::shapes::Effects,
) {
    for d in crate::wire::HORIZONTAL {
        check_corner_change_at(blocks, signals, shapes, d.relative(pos), updates);
    }
    for d in crate::wire::HORIZONTAL {
        let p = d.relative(pos);
        let corner = if blocks.redstone_conductor(p) {
            (p.0, p.1 + 1, p.2)
        } else {
            (p.0, p.1 - 1, p.2)
        };
        check_corner_change_at(blocks, signals, shapes, corner, updates);
    }
}

/// `RedStoneWireBlock.checkCornerChangeAt`, offsets 0-60.
///
/// ```java
/// if (!level.getBlockState(pos).is(this)) return;
/// level.updateNeighborsAt(pos, this);
/// for (Direction d : Direction.values()) level.updateNeighborsAt(pos.relative(d), this);
/// ```
///
/// **It refuses anything that is not a wire, and that is the whole gate.** A
/// corner with no dust in it costs one state read; a corner with dust in it
/// costs seven `updateNeighborsAt` calls, which is 42 `neighborChanged`s.
/// Reading the guard as "if there is a block here" instead of "if there is a
/// wire here" turns a placement into a cascade over the whole neighbourhood.
fn check_corner_change_at(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut crate::shapes::Effects,
) {
    if !crate::wire::is_wire(&blocks.state_at(pos)) {
        return;
    }
    let wire = "minecraft:redstone_wire";
    update_neighbors_at(blocks, Some(signals), shapes, pos, wire, updates);
    for d in DIRECTIONS {
        update_neighbors_at(
            blocks,
            Some(signals),
            shapes,
            d.relative(pos),
            wire,
            updates,
        );
    }
}

// ---------------------------------------------------------------------------
// 4b. The torch
// ---------------------------------------------------------------------------

/// `RedstoneTorchBlock.hasNeighborSignal` and its wall override.
///
/// ```java
/// RedstoneTorchBlock:      level.hasSignal(pos.below(), DOWN)
/// RedstoneWallTorchBlock:  Direction back = state.getValue(FACING).getOpposite();
///                          level.hasSignal(pos.relative(back), back)
/// ```
///
/// One position each, and it is the block the torch is *attached to* — the
/// floor under a standing torch, the wall behind a wall torch. A torch is
/// switched off by the thing holding it and by nothing else, which is why a
/// torch on top of a powered block goes out and a torch beside one does not.
fn torch_has_neighbor_signal(power: &Redstone<'_>, pos: BlockPos, state: &str) -> bool {
    if block_of(state) == "minecraft:redstone_wall_torch" {
        let back = crate::placement::property_of(state, "facing")
            .and_then(Face::from_name)
            .map(Face::opposite);
        return back.is_some_and(|b| power.has_signal(b.relative(pos), b));
    }
    power.has_signal(Face::Down.relative(pos), Face::Down)
}

/// `RedstoneTorchBlock.neighborChanged`, offsets 0-44.
///
/// ```java
/// if (state.getValue(LIT) == hasNeighborSignal(level, pos, state)   //  0-20
///     && !level.getBlockTicks().willTickThisTick(pos, this))        // 23-34
///     level.scheduleTick(pos, this, 2);                             // 37-41
/// ```
///
/// # The test is `==` and reading it as `!=` inverts the whole block
///
/// Offset 20 is `if_icmpne 44`, and 44 is the `return`: the two branch **past**
/// the schedule when they differ, so the schedule fires when they are the
/// same. That is the right way round and it takes a moment to see, because
/// `LIT` and `hasNeighborSignal` mean opposite things — a torch is on when the
/// block holding it is *not* powered. So `lit == hasNeighborSignal` is
/// precisely the two states that need to change: lit on a powered block, and
/// unlit on an unpowered one. The steady state is `lit != hasNeighborSignal`
/// and schedules nothing.
///
/// This was written as `!=` first and every torch in the corpus asked for a
/// tick it did not need. It was caught by the diagnostic below firing in a
/// test that was about something else, which is the argument for the
/// diagnostic.
///
/// **It schedules and never writes.** The torch's `LIT` changes two ticks
/// later in `tick`, which is where the burnout counter lives as well. So a
/// torch that has just been placed onto an unpowered block is already `lit`
/// and this method does nothing at all — which is the whole of check `100`'s
/// torch and the reason that check is about the *wire* rather than about the
/// torch.
///
/// # The scheduled tick is now wired in — 2026-08-11
///
/// It used to only *record* that a torch had asked to be woken, because
/// nothing drained a request. `crate::shapes::TickRequest` is that drain, and
/// `crate::ticks::run` grew the `RedstoneTorchBlock.tick` arm on the same
/// branch. `400-a-redstone-torch-turns-off-when-its-block-is-powered` is the
/// check this pair exists for.
fn torch_neighbor_changed(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, None);
    if torch_should_schedule(&power, pos, state) {
        // `level.scheduleTick(pos, this, 2)`, offsets 37-41. No priority
        // argument, so the three-argument overload's `TickPriority.NORMAL`.
        updates.ticks.push(crate::shapes::TickRequest {
            pos,
            block: block_of(state).to_string(),
            delay: TORCH_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
    }
}

/// `level.scheduleTick(pos, this, 2)` — `iconst_2` at offset 40 of
/// `RedstoneTorchBlock.neighborChanged`, and the same constant again at
/// `RedstoneTorchBlock.tick`'s `TOGGLE_DELAY`.
pub const TORCH_DELAY: i64 = 2;

/// The condition alone, so it can be tested without a tick scheduler to
/// observe.
///
/// `willTickThisTick` is the second half of vanilla's `&&`, and it is left out
/// here rather than ported: see `crate::shapes::TickRequest`'s own docs for
/// why it cannot change the queue's state. It is an early-out around a
/// `scheduleTick` whose only effect on an already-scheduled position is to be
/// dropped.
fn torch_should_schedule(power: &Redstone<'_>, pos: BlockPos, state: &str) -> bool {
    let lit = crate::placement::property_of(state, "lit") == Some("true");
    lit == torch_has_neighbor_signal(power, pos, state)
}

/// `RedstoneTorchBlock.onPlace` -> `notifyNeighbors`, offsets 0-59.
///
/// ```java
/// for (Direction d : Direction.values())
///     level.updateNeighborsAt(pos.relative(d), this, withFront(orientation, d));
/// ```
///
/// **All six, and each one is `updateNeighborsAt` rather than
/// `neighborChanged`** — so a torch tells its six neighbours' six neighbours,
/// which is 36 updates and reaches two blocks away. That is why placing a
/// torch lights dust that is not touching it, and it is what makes check `100`
/// move: the dust the check watches is a diagonal step from the torch and only
/// the second ring reaches it.
///
/// `RedstoneTorchBlock` has no `oldState.is(this)` guard, unlike the wire.
pub fn torch_on_place(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    updates: &mut crate::shapes::Effects,
) {
    for d in DIRECTIONS {
        update_neighbors_at(
            blocks,
            Some(signals),
            shapes,
            d.relative(pos),
            "minecraft:redstone_torch",
            updates,
        );
    }
}

// ---------------------------------------------------------------------------
// 4bb. The diode — `RepeaterBlock` only, and the exclusion is the point
// ---------------------------------------------------------------------------

/// `DiodeBlock` has exactly two subclasses and this ports one of them.
///
/// **`ComparatorBlock` is excluded deliberately, not left over.** It overrides
/// **five** of the methods below — `tick`, `checkTickOnNeighbor`,
/// `shouldTurnOn`, `getOutputSignal` and `getDelay` — and three of those read
/// a `ComparatorBlockEntity` this server has no analogue for. Dispatching a
/// comparator through the repeater's rule would therefore be wrong on every
/// one of them rather than silent, and
/// `420-a-comparator-switches-to-subtract-mode` **matches today**: it tests
/// the right-click that cycles MODE, which is `use_without_item.json`'s and
/// not this module's. A rule that made it tick would be a regression bought
/// with no check.
///
/// So the test is the block id and not `instanceof DiodeBlock`, and that is
/// the one place in this file where a name beats the class table.
fn is_repeater(state: &str) -> bool {
    block_of(state) == "minecraft:repeater"
}

/// `DiodeBlock.isDiode` — `getBlock() instanceof DiodeBlock`.
///
/// Read-only, and used by [`diode_should_prioritize`] alone, which asks what
/// is in **front** of the repeater rather than what to run. Both subclasses
/// answer yes to vanilla, so both answer yes here even though only one of
/// them has a ported rule: narrowing this to the repeater would change a
/// priority, and a priority is the drain order.
fn is_diode(state: &str) -> bool {
    matches!(
        block_of(state),
        "minecraft:repeater" | "minecraft:comparator"
    )
}

/// `RepeaterBlock.getDelay` — `state.getValue(DELAY) * 2`, offsets 0-15.
///
/// The property is 1..4 and the delay is therefore 2, 4, 6 or 8 game ticks.
/// `410-a-repeater-delays-the-signal-it-passes-on` cycles it to 2, so it is
/// asking for **4** ticks, 200 ms.
fn repeater_delay(state: &str) -> i64 {
    crate::placement::property_of(state, "delay")
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(1)
        * 2
}

/// `DiodeBlock.getInputSignal(level, pos, state)`, offsets 0-82.
///
/// ```java
/// Direction facing = state.getValue(FACING);
/// BlockPos   at    = pos.relative(facing);
/// int        i     = level.getSignal(at, facing);
/// if (i >= 15) return i;
/// BlockState s     = level.getBlockState(at);
/// return Math.max(i, s.is(REDSTONE_WIRE) ? s.getValue(POWER) : 0);
/// ```
///
/// **`FACING` names the input, not the output.** `pos.relative(FACING)` is
/// where the signal is read and
/// [`diode_update_neighbors_in_front`] writes to `FACING.getOpposite()` — so
/// vanilla's "in front" is behind the arrow a player sees. `410`'s repeater
/// faces north at `[1,0,1]`, reads `[1,0,0]` and drives the wire at `[1,0,2]`,
/// which is exactly this way round.
pub(crate) fn diode_input_signal(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    pos: BlockPos,
    state: &str,
) -> u8 {
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return 0;
    };
    let at = facing.relative(pos);
    let signal = power.get_signal(at, facing);
    if signal >= 15 {
        return signal;
    }
    // `s.is(REDSTONE_WIRE) ? s.getValue(POWER) : 0`, offsets 48-79. Read off
    // the state and **not** through `getSignal`, for the same reason
    // `RedstoneWireEvaluator.getWireSignal` is: a wire's signal towards a
    // block that is not dust is attenuated, and a repeater reading its own
    // input is not asking what the dust exports.
    let neighbour = blocks.state_at(at);
    let wire = if crate::wire::is_wire(&neighbour) {
        crate::placement::property_of(&neighbour, "power")
            .and_then(|p| p.parse::<u8>().ok())
            .unwrap_or(0)
    } else {
        0
    };
    signal.max(wire)
}

/// `DiodeBlock.shouldTurnOn` — `getInputSignal(...) > 0`, offsets 0-15.
pub(crate) fn diode_should_turn_on(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    pos: BlockPos,
    state: &str,
) -> bool {
    diode_input_signal(blocks, power, pos, state) > 0
}

/// `RepeaterBlock.isLocked` — `getAlternateSignal(...) > 0`.
///
/// `DiodeBlock.getAlternateSignal` reads the two **sides**
/// (`facing.getClockWise()` and `getCounterClockWise()`) through
/// `SignalGetter.getControlInputSignal(pos, dir, diodesOnly)`, with
/// `sideInputDiodesOnly()` — `iconst_1` for `RepeaterBlock`, so a repeater is
/// locked only by another diode beside it and never by dust.
/// `RepeaterBlock.updateShape`, offsets 2-96, read from the jar:
///
/// ```java
/// if (dir == Direction.DOWN && !canSurviveOn(level, neighborPos, neighborState)) return AIR;
/// if (!level.isClientSide() && dir.getAxis() != state.getValue(FACING).getAxis())
///     return state.setValue(LOCKED, isLocked(level, pos, state));
/// return super.updateShape(...);
/// ```
///
/// The `DOWN` arm is `update_shape.json`'s row and is left to the table;
/// this answers the middle arm — **every** neighbour change across the
/// repeater's axis rewrites `LOCKED` from `isLocked`, whether or not it
/// changed, and `updateOrDestroy` drops the write when it is equal. It is
/// how a repeater powered from the side shows `locked=true` on the wire:
/// the side repeater's own tick writes `POWERED` with flags 2, whose shape
/// updates reach here.
///
/// `getStateForPlacement` sets `LOCKED` the same way and is **not** ported:
/// placement has no level handle, so a repeater placed beside an already
/// active side diode lands `locked=false` and corrects itself on the next
/// shape update across its axis. Named rather than hidden.
#[must_use]
pub fn repeater_update_shape(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    direction: Face,
) -> Option<String> {
    if !is_repeater(state) || direction == Face::Down {
        return None;
    }
    let facing = crate::placement::property_of(state, "facing").and_then(Face::from_name)?;
    if direction.axis_is_horizontal() == facing.axis_is_horizontal()
        && (direction == facing || direction == facing.opposite())
    {
        return None;
    }
    if direction == Face::Up {
        return None;
    }
    let signals = signals?;
    let power = Redstone::new(blocks, signals, shapes);
    let locked = repeater_is_locked(blocks, &power, pos, state);
    Some(crate::placement::with_property(
        state,
        "locked",
        if locked { "true" } else { "false" },
    ))
}

fn repeater_is_locked(
    blocks: &dyn Blocks,
    power: &Redstone<'_>,
    pos: BlockPos,
    state: &str,
) -> bool {
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return false;
    };
    let sides = [facing.clockwise(), facing.counter_clockwise()];
    sides.into_iter().flatten().any(|d| {
        // `SignalGetter.getControlInputSignal(pos, dir, true)`, offsets 9-33:
        // with `diodesOnly` set, a state that is not a diode contributes
        // **nothing at all** — not its wire power, not its signal source. The
        // three lower branches of that method are the `diodesOnly == false`
        // arm and are not reachable from a repeater.
        let at = d.relative(pos);
        is_diode(&blocks.state_at(at)) && power.get_direct_signal(at, d) > 0
    })
}

/// `DiodeBlock.shouldPrioritize`, offsets 0-55.
///
/// ```java
/// Direction  back = state.getValue(FACING).getOpposite();
/// BlockState s    = level.getBlockState(pos.relative(back));
/// return isDiode(s) && s.getValue(FACING) != back;
/// ```
///
/// The block **in front** — `FACING.getOpposite()`, per
/// [`diode_input_signal`]'s note — being a diode that is not pointing the same
/// way. That is the "diode facing into our side" case, and it is what buys
/// `EXTREMELY_HIGH` instead of `HIGH`.
pub(crate) fn diode_should_prioritize(blocks: &dyn Blocks, pos: BlockPos, state: &str) -> bool {
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return false;
    };
    let back = facing.opposite();
    let front = blocks.state_at(back.relative(pos));
    is_diode(&front)
        && crate::placement::property_of(&front, "facing").and_then(Face::from_name) != Some(back)
}

/// `DiodeBlock.checkTickOnNeighbor`, offsets 0-102, reached from
/// `neighborChanged` once `canSurvive` holds.
///
/// ```java
/// if (isLocked(level, pos, state)) return;
/// boolean powered  = state.getValue(POWERED);
/// boolean turnOn   = shouldTurnOn(level, pos, state);
/// if (powered != turnOn && !level.getBlockTicks().willTickThisTick(pos, this)) {
///     TickPriority p = TickPriority.HIGH;
///     if (shouldPrioritize(level, pos, state)) p = TickPriority.EXTREMELY_HIGH;
///     else if (powered)                        p = TickPriority.VERY_HIGH;
///     level.scheduleTick(pos, this, getDelay(state), p);
/// }
/// ```
///
/// Note the priority ladder's middle rung: a repeater **turning off** is
/// `VERY_HIGH` and one turning on is `HIGH`, which is how vanilla makes a
/// falling edge beat a rising one within the same game tick.
///
/// `canSurvive` is not tested here. `DiodeBlock.neighborChanged` branches on
/// it into a drop-and-remove path, and this server's placement layer refuses a
/// repeater without a floor in the first place, so the false arm is
/// unreachable from anything the corpus does. It is named rather than ported
/// because reaching it needs `dropResources`, which is a different item.
fn diode_neighbor_changed(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    if !is_repeater(state) {
        return;
    }
    let power = Redstone::new(blocks, signals, None);
    if repeater_is_locked(blocks, &power, pos, state) {
        return;
    }
    let powered = crate::placement::property_of(state, "powered") == Some("true");
    if powered == diode_should_turn_on(blocks, &power, pos, state) {
        return;
    }
    // `checkTickOnNeighbor` offsets 43-53: `getBlockTicks().willTickThisTick`
    // — a repeater whose tick was collected this tick and has not run yet
    // books nothing, because the collection already emptied
    // `ticksPerPosition` and the uniqueness guard alone would let a second
    // tick in behind the first.
    if crate::ticks::ScheduledTicks::shared().will_tick_this_tick(pos, block_of(state)) {
        return;
    }
    let priority = if diode_should_prioritize(blocks, pos, state) {
        PRIORITY_EXTREMELY_HIGH
    } else if powered {
        PRIORITY_VERY_HIGH
    } else {
        PRIORITY_HIGH
    };
    updates.ticks.push(crate::shapes::TickRequest {
        pos,
        block: block_of(state).to_string(),
        delay: repeater_delay(state),
        priority,
    });
}

/// `TickPriority.EXTREMELY_HIGH`, `VERY_HIGH` and `HIGH` by their ordinals in
/// the enum's own `$values` — see [`crate::ticks::PRIORITY_NORMAL`], which is
/// the fourth.
pub const PRIORITY_EXTREMELY_HIGH: u8 = 0;
/// See [`PRIORITY_EXTREMELY_HIGH`].
pub const PRIORITY_VERY_HIGH: u8 = 1;
/// See [`PRIORITY_EXTREMELY_HIGH`].
pub const PRIORITY_HIGH: u8 = 2;

/// `DiodeBlock.updateNeighborsInFront`, offsets 0-57 — and the same method on
/// `ObserverBlock`, offsets 0-55, which differs only in the `initialOrientation`
/// argument that this server does not carry.
///
/// ```java
/// Direction  facing = state.getValue(FACING);
/// BlockPos   front  = pos.relative(facing.getOpposite());
/// level.neighborChanged(front, this, orientation);
/// level.updateNeighborsAtExceptFromFacing(front, this, facing, orientation);
/// ```
///
/// So the block in front is told **twice over**: once directly, and once as
/// part of its own five other faces. `updateNeighborsAtExceptFromFacing` skips
/// `facing`, which is the direction back towards the diode — vanilla does not
/// tell itself.
pub fn diode_update_neighbors_in_front(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return;
    };
    let front = facing.opposite().relative(pos);
    let block = block_of(state).to_string();
    neighbor_changed(blocks, Some(signals), shapes, front, &block, updates);
    update_neighbors_at_except_from_facing(
        blocks,
        Some(signals),
        shapes,
        front,
        &block,
        Some(facing),
        updates,
    );
}

// ---------------------------------------------------------------------------
// 4bc. The observer
// ---------------------------------------------------------------------------

/// `ObserverBlock`, which has no subclasses in the pinned jar.
fn is_observer(state: &str) -> bool {
    block_of(state) == "minecraft:observer"
}

/// `ObserverBlock.startSignal`, offsets 0-34, reached from
/// [`observer_update_shape`].
///
/// ```java
/// if (!level.isClientSide() && !tickAccess.getBlockTicks().hasScheduledTick(pos, this))
///     tickAccess.scheduleTick(pos, this, 2);
/// ```
///
/// `iconst_2` — two game ticks, 100 ms. `440`'s `about` records that the
/// corpus believed one tick until this constant was counted.
fn observer_start_signal(pos: BlockPos, updates: &mut crate::shapes::Effects) {
    updates.ticks.push(crate::shapes::TickRequest {
        pos,
        block: "minecraft:observer".to_string(),
        delay: OBSERVER_DELAY,
        priority: crate::ticks::PRIORITY_NORMAL,
    });
}

/// `iconst_2`, in both `ObserverBlock.startSignal` (offset 28) and the
/// reschedule inside `ObserverBlock.tick` (offset 64).
pub const OBSERVER_DELAY: i64 = 2;

/// `ObserverBlock.updateShape`, offsets 0-33, called from
/// [`crate::shapes::execute_shape_update`] where every other shape update is
/// answered.
///
/// `direction` is the direction from the observer **towards the neighbour that
/// changed**, which is the same convention `execute_shape_update` documents,
/// so the test is a plain equality against `FACING` with no opposite anywhere.
pub fn observer_update_shape(
    state: &str,
    direction: Face,
    pos: BlockPos,
    updates: &mut crate::shapes::Effects,
) {
    if !is_observer(state) {
        return;
    }
    let facing = crate::placement::property_of(state, "facing").and_then(Face::from_name);
    let powered = crate::placement::property_of(state, "powered") == Some("true");
    if facing == Some(direction) && !powered {
        observer_start_signal(pos, updates);
    }
}

/// `ObserverBlock.onPlace`, offsets 0-84.
///
/// ```java
/// if (state.is(oldState.getBlock())) return;
/// if (!level.isClientSide() && state.getValue(POWERED)
///     && !level.getBlockTicks().hasScheduledTick(pos, this)) {
///     BlockState off = state.setValue(POWERED, false);
///     level.setBlock(pos, off, 18);
///     updateNeighborsInFront(level, pos, off);
/// }
/// ```
///
/// **The guard is `is(oldState.getBlock())` and it is the opposite way round
/// from the diode's**: this arm runs only when the *block* changed, so it
/// cannot fire on the POWERED writes `tick` makes. It exists to un-stick an
/// observer that was saved mid-pulse, and flags **18** — `UPDATE_CLIENTS |
/// UPDATE_KNOWN_SHAPE` — are what stop it cascading a shape update back into
/// [`observer_update_shape`] and rescheduling itself for ever.
fn observer_on_place(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    old: &str,
    updates: &mut crate::shapes::Effects,
) {
    if block_of(state) == block_of(old) {
        return;
    }
    if crate::placement::property_of(state, "powered") != Some("true") {
        return;
    }
    let off = crate::placement::with_property(state, "powered", "false");
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &off,
        crate::shapes::UPDATE_CLIENTS | crate::shapes::UPDATE_KNOWN_SHAPE,
        crate::shapes::RECURSION_LIMIT,
        updates,
    );
    observer_update_neighbors_in_front(blocks, signals, shapes, pos, &off, updates);
}

/// `ObserverBlock.updateNeighborsInFront`, offsets 0-55.
///
/// ```java
/// Direction facing = state.getValue(FACING);
/// BlockPos  behind = pos.relative(facing.getOpposite());
/// level.neighborChanged(behind, this, orientation);
/// level.updateNeighborsAtExceptFromFacing(behind, this, facing, orientation);
/// ```
///
/// Structurally the diode's method, and the two are written separately rather
/// than shared because they are separate overrides on unrelated classes and
/// their `initialOrientation` arguments differ (`(facing.getOpposite(), null)`
/// here against `(facing.getOpposite(), UP)` on the diode). This server
/// carries no orientation, so today they would fold into one function — and
/// folding them would make the first orientation-aware change touch a shared
/// helper two blocks depend on.
pub fn observer_update_neighbors_in_front(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let Some(facing) = crate::placement::property_of(state, "facing").and_then(Face::from_name)
    else {
        return;
    };
    let behind = facing.opposite().relative(pos);
    let block = block_of(state).to_string();
    neighbor_changed(blocks, Some(signals), shapes, behind, &block, updates);
    update_neighbors_at_except_from_facing(
        blocks,
        Some(signals),
        shapes,
        behind,
        &block,
        Some(facing),
        updates,
    );
}

// ---------------------------------------------------------------------------
// 4bd. The three `Block.tick` bodies, which `crate::ticks` dispatches to
// ---------------------------------------------------------------------------

/// `RedstoneTorchBlock.RECENT_TOGGLES` — vanilla's
/// `Map<BlockGetter, List<Toggle>>`, with one level.
///
/// A `Vec` and not a set: the burnout counter is "how many of my last few
/// toggles were at this position", so duplicates are the measurement.
static RECENT_TOGGLES: std::sync::Mutex<Vec<(BlockPos, i64)>> = std::sync::Mutex::new(Vec::new());

/// `RedstoneTorchBlock.RECENT_TOGGLE_TIMER`, `MAX_RECENT_TOGGLES` and
/// `RESTART_DELAY` — 60, 8 and 160, read off the class's own constant fields.
pub const TORCH_RECENT_TOGGLE_TIMER: i64 = 60;
/// See [`TORCH_RECENT_TOGGLE_TIMER`].
pub const TORCH_MAX_RECENT_TOGGLES: usize = 8;
/// See [`TORCH_RECENT_TOGGLE_TIMER`].
pub const TORCH_RESTART_DELAY: i64 = 160;
/// `Level.levelEvent(1502, pos, 0)` — the burnout smoke and fizz.
pub const LEVEL_EVENT_REDSTONE_TORCH_BURNOUT: i32 = 1502;

/// `RedstoneTorchBlock.isToggledTooFrequently(level, pos, logging)`, offsets
/// 0-105.
///
/// ```java
/// List<Toggle> list = RECENT_TOGGLES.computeIfAbsent(level, l -> new ArrayList<>());
/// if (logging) list.add(new Toggle(pos.immutable(), level.getGameTime()));
/// int n = 0;
/// for (Toggle t : list)
///     if (t.pos.equals(pos) && ++n >= 8) return true;
/// return false;
/// ```
///
/// The counter is incremented *before* the comparison, so the eighth toggle at
/// one position is the one that burns out.
fn is_toggled_too_frequently(pos: BlockPos, now: i64, logging: bool) -> bool {
    let mut list = RECENT_TOGGLES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if logging {
        list.push((pos, now));
    }
    list.iter().filter(|(p, _)| *p == pos).count() >= TORCH_MAX_RECENT_TOGGLES
}

/// `RedstoneTorchBlock.tick`, offsets 0-190.
///
/// ```java
/// boolean hasSignal = hasNeighborSignal(level, pos, state);              //   0-7
/// List<Toggle> list = RECENT_TOGGLES.get(level);                         //   9-21
/// while (list != null && !list.isEmpty()                                 //  23-73
///        && level.getGameTime() - list.get(0).when > 60)
///     list.remove(0);
/// if (state.getValue(LIT)) {                                             //  76-89
///     if (hasSignal) {                                                   //  92-94
///         level.setBlock(pos, state.setValue(LIT, false), 3);            //  97-117
///         if (isToggledTooFrequently(level, pos, true)) {                // 118-124
///             level.levelEvent(1502, pos, 0);                            // 127-135
///             level.scheduleTick(pos, level.getBlockState(pos).getBlock(), 160);
///         }
///     }
/// } else if (!hasSignal && !isToggledTooFrequently(level, pos, false))   // 155-166
///     level.setBlock(pos, state.setValue(LIT, true), 3);                 // 169-189
/// ```
///
/// Flags **3** on both writes — `UPDATE_NEIGHBORS | UPDATE_CLIENTS` — so the
/// torch's own six neighbours are told, which is what carries the change into
/// the dust. That is a different answer from the button's 3 and the coral's 2
/// for a reason: a torch is a signal source and the other two are not.
///
/// The prune loop is `> 60`, strictly, and it runs on the *front* of the list
/// only — the list is append-ordered by time, so a head that is young means
/// every entry is.
pub fn torch_tick(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    now: i64,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, None);
    let has_signal = torch_has_neighbor_signal(&power, pos, state);
    {
        let mut list = RECENT_TOGGLES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while list
            .first()
            .is_some_and(|(_, when)| now - when > TORCH_RECENT_TOGGLE_TIMER)
        {
            list.remove(0);
        }
    }
    let lit = crate::placement::property_of(state, "lit") == Some("true");
    if lit {
        if !has_signal {
            return;
        }
        let off = crate::placement::with_property(state, "lit", "false");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &off,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
        if is_toggled_too_frequently(pos, now, true) {
            updates
                .level_events
                .push((LEVEL_EVENT_REDSTONE_TORCH_BURNOUT, pos, 0));
            // `level.getBlockState(pos).getBlock()` — re-read, not the block
            // this tick was scheduled for. They are the same today; vanilla
            // re-reads and so does this.
            updates.ticks.push(crate::shapes::TickRequest {
                pos,
                block: block_of(&blocks.state_at(pos)).to_string(),
                delay: TORCH_RESTART_DELAY,
                priority: crate::ticks::PRIORITY_NORMAL,
            });
        }
    } else if !has_signal && !is_toggled_too_frequently(pos, now, false) {
        let on = crate::placement::with_property(state, "lit", "true");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &on,
            crate::shapes::UPDATE_ALL,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
    }
}

/// `ObserverBlock.tick`, offsets 0-75.
///
/// ```java
/// if (state.getValue(POWERED)) {
///     level.setBlock(pos, state.setValue(POWERED, false), 2);
/// } else {
///     level.setBlock(pos, state.setValue(POWERED, true), 2);
///     level.scheduleTick(pos, this, 2);
/// }
/// updateNeighborsInFront(level, pos, state);
/// ```
///
/// **Two writes from one shape update**, which is the whole of what `440`
/// tests: the first tick powers the observer and books the second, the second
/// unpowers it and books nothing. Flags **2** on both — `UPDATE_CLIENTS`
/// alone, no neighbour cascade — because the notification is
/// `updateNeighborsInFront`'s job and it runs on *both* branches, outside the
/// `if`.
///
/// Note the last argument: `aload_1` is the state as it was *entering* the
/// tick, not the state just written. Vanilla notifies in front using the old
/// state, and only `FACING` is read from it, which is the one property neither
/// branch changes.
pub fn observer_tick(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let powered = crate::placement::property_of(state, "powered") == Some("true");
    let next =
        crate::placement::with_property(state, "powered", if powered { "false" } else { "true" });
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &next,
        crate::shapes::UPDATE_CLIENTS,
        crate::shapes::RECURSION_LIMIT,
        updates,
    );
    if !powered {
        updates.ticks.push(crate::shapes::TickRequest {
            pos,
            block: "minecraft:observer".to_string(),
            delay: OBSERVER_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
    }
    observer_update_neighbors_in_front(blocks, signals, shapes, pos, state, updates);
}

/// `DiodeBlock.tick`, offsets 0-114, for the repeater.
///
/// ```java
/// if (isLocked(level, pos, state)) return;
/// boolean powered = state.getValue(POWERED);
/// boolean turnOn  = shouldTurnOn(level, pos, state);
/// if (powered && !turnOn)      level.setBlock(pos, state.setValue(POWERED, false), 2);
/// else if (!powered) {
///     level.setBlock(pos, state.setValue(POWERED, true), 2);
///     if (!turnOn) level.scheduleTick(pos, this, getDelay(state), TickPriority.VERY_HIGH);
/// }
/// ```
///
/// Read the `else if` carefully: a repeater that is **already on and still
/// should be** falls through both arms and writes nothing, and one that is
/// **off and should stay off** still turns itself on for one delay and books
/// the tick that turns it back off. That second case is the repeater's pulse
/// extension and it is easy to lose by writing the obvious `powered !=
/// turnOn`.
///
/// Flags **2** and no cascade — the dust in front hears about it through
/// `onPlace` -> `updateNeighborsInFront`, which
/// `LevelChunk.setBlockState` runs on this very write. See
/// [`on_place`]'s repeater arm.
pub fn repeater_tick(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, None);
    if repeater_is_locked(blocks, &power, pos, state) {
        return;
    }
    let powered = crate::placement::property_of(state, "powered") == Some("true");
    let turn_on = diode_should_turn_on(blocks, &power, pos, state);
    if powered && !turn_on {
        let off = crate::placement::with_property(state, "powered", "false");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &off,
            crate::shapes::UPDATE_CLIENTS,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
    } else if !powered {
        let on = crate::placement::with_property(state, "powered", "true");
        crate::shapes::set_block(
            blocks,
            shapes,
            pos,
            &on,
            crate::shapes::UPDATE_CLIENTS,
            crate::shapes::RECURSION_LIMIT,
            updates,
        );
        if !turn_on {
            updates.ticks.push(crate::shapes::TickRequest {
                pos,
                block: block_of(state).to_string(),
                delay: repeater_delay(state),
                priority: PRIORITY_VERY_HIGH,
            });
        }
    }
}

// ---------------------------------------------------------------------------
// 4c. The lamp
// ---------------------------------------------------------------------------

/// `RedstoneLampBlock.neighborChanged`, offsets 0-65.
///
/// ```java
/// if (level.isClientSide) return;
/// boolean lit = state.getValue(LIT);                          //  8-21
/// if (lit != level.hasNeighborSignal(pos)) {                  // 23-30
///     if (lit) level.scheduleTick(pos, this, 4);              // 33-45
///     else     level.setBlock(pos, state.cycle(LIT), 2);      // 48-64
/// }
/// ```
///
/// # The two directions are not symmetric, and that is why this is here
///
/// **Lighting is immediate and unlighting is four ticks late.** The `else` arm
/// writes straight away; the `if` arm only asks to be woken. So the half of
/// this block that needs a tick scheduler is the half that turns *off*, and
/// the half that turns on needs nothing this server does not already have.
///
/// That asymmetry is the whole reason the lamp landed in the same link as
/// `updateNeighborsAt` rather than waiting for the torch tick.
/// `360-a-lever-lights-a-lamp` flips a lever **on**, and its `about` field
/// says it is "`neighborChanged` rather than `updateShape`, and the half of a
/// block update this server carries a flag for and does not yet fill in" —
/// which is precisely this. It is also the first thing to exercise
/// [`Redstone::has_neighbor_signal`] against real vanilla: the torch's use of
/// it ends in a tick nobody schedules, so until now the function had unit
/// tests and no measurement.
///
/// Note the test is `lit != hasNeighborSignal`, the opposite sense from
/// [`torch_should_schedule`]'s `==`, and both are right: a lamp is on when
/// something powers it and a torch is on when nothing powers what holds it.
///
/// `state.cycle(LIT)` on a two-value property is `!lit`, and in this arm `lit`
/// is already known false, so it writes `lit=true`. Written as the cycle
/// vanilla wrote rather than as the literal, because the two stop being the
/// same thing the moment anybody gives this block a third state.
///
/// **Flag 2**, `UPDATE_CLIENTS` alone: a lamp tells the players and does not
/// tell its neighbours, which is what stops two adjacent lamps updating each
/// other for ever.
fn lamp_neighbor_changed(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    let power = Redstone::new(blocks, signals, shapes);
    let lit = crate::placement::property_of(state, "lit") == Some("true");
    if lit == power.has_neighbor_signal(pos) {
        return;
    }
    if lit {
        // Offsets 35-42: `scheduleTick(pos, this, 4)` — the three-argument
        // form, so `TickPriority.NORMAL`. The lamp goes dark on the tick,
        // never here, and the tick re-reads `hasNeighborSignal` for itself.
        updates.ticks.push(crate::shapes::TickRequest {
            pos,
            block: block_of(state).to_string(),
            delay: LAMP_DELAY,
            priority: crate::ticks::PRIORITY_NORMAL,
        });
        return;
    }
    let to = crate::placement::with_property(state, "lit", "true");
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &to,
        UPDATE_CLIENTS,
        RECURSION_LIMIT,
        updates,
    );
}

/// `RedstoneLampBlock.getStateForPlacement`, offsets 0-27 — the whole method.
///
/// ```java
/// return defaultBlockState().setValue(LIT, level.hasNeighborSignal(clickedPos));
/// ```
///
/// A lamp placed into an already-powered spot is lit the moment it appears,
/// with no neighbour update involved at all.
///
/// **Not wired into `crate::placement` and that is deliberate.** The rule
/// needs a `SignalTable`, which `computed_with_neighbours` is not handed, and
/// threading one through the placement path is surgery in a file two other
/// links are also in. What it costs until then is exactly one case — a lamp
/// placed into an already-powered spot reads `lit=false` until something else
/// updates it — and no check in the corpus builds that, because `360` places
/// the lamp before the lever. Tested here rather than left as an untested
/// `pub fn`: a rule with nothing behind it has never been observed doing
/// anything, and its constants are then guesses with the authority of code.
#[must_use]
pub fn lamp_placement(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    pos: BlockPos,
    default: &str,
) -> String {
    let power = Redstone::new(blocks, signals, None);
    let lit = power.has_neighbor_signal(pos);
    crate::placement::with_property(default, "lit", if lit { "true" } else { "false" })
}

/// `RedstoneLampBlock.neighborChanged` offset 41: `iconst_4`.
pub const LAMP_DELAY: i64 = 4;

/// `RedstoneLampBlock.tick`, offsets 1-41, read from the jar:
///
/// ```java
/// if (state.getValue(LIT) && !level.hasNeighborSignal(pos))
///     level.setBlock(pos, state.cycle(LIT), 2);
/// ```
///
/// Flags **2**: no neighbour update goes out when a lamp goes dark, which
/// is why a lamp cannot drive anything — and the re-check of
/// `hasNeighborSignal` is the whole point of the four-tick delay: a lever
/// flipped off and on again inside it leaves the lamp lit.
pub fn lamp_tick(
    blocks: &dyn Blocks,
    signals: &SignalTable,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    updates: &mut crate::shapes::Effects,
) {
    if crate::placement::property_of(state, "lit") != Some("true") {
        return;
    }
    let power = Redstone::new(blocks, signals, shapes);
    if power.has_neighbor_signal(pos) {
        return;
    }
    let to = crate::placement::with_property(state, "lit", "false");
    crate::shapes::set_block(
        blocks,
        shapes,
        pos,
        &to,
        UPDATE_CLIENTS,
        RECURSION_LIMIT,
        updates,
    );
}

/// `BlockState.onPlace` for the blocks this module owns, called from inside
/// [`crate::shapes::set_block`] where `LevelChunk.setBlockState` calls it —
/// **before** `sendBlockUpdated` and before the six-face cascade.
///
/// The guard `LevelChunk.setBlockState` applies at offsets 514-542 is
/// `!isClientSide && (flags & 512) == 0`; the caller already tests it for the
/// rail, and this rides the same test rather than repeating it.
pub fn on_place(
    blocks: &dyn Blocks,
    signals: Option<&SignalTable>,
    shapes: Option<&ShapeTable>,
    pos: BlockPos,
    state: &str,
    old: &str,
    updates: &mut crate::shapes::Effects,
) {
    let Some(signals) = signals else { return };
    if crate::wire::is_wire(state) {
        wire_on_place(blocks, signals, shapes, pos, state, old, updates);
    } else if is_torch(state) {
        torch_on_place(blocks, signals, shapes, pos, updates);
    } else if crate::comparator::is_comparator(state) {
        crate::comparator::on_place(blocks, signals, shapes, pos, state, old, updates);
    } else if is_repeater(state) {
        // `DiodeBlock.onPlace`, offsets 0-7: `updateNeighborsInFront` and
        // nothing else — **no `oldState.is(this)` guard**, unlike the wire and
        // unlike `ObserverBlock.onPlace`. That absence is what carries the
        // repeater's output to the dust in front when `DiodeBlock.tick`
        // rewrites only the POWERED property with flags 2: the write does not
        // change the block, so nothing else on the path would notify anybody,
        // and `LevelChunk.setBlockState` calls `onPlace` on every write whose
        // `UPDATE_KNOWN_SHAPE` is clear (offsets 514-542). It is the whole
        // mechanism behind `410`'s `section_blocks_update` carrying the wire
        // as well as the repeater.
        diode_update_neighbors_in_front(blocks, signals, shapes, pos, state, updates);
    } else if is_observer(state) {
        observer_on_place(blocks, signals, shapes, pos, state, old, updates);
    } else if block_of(state) == "minecraft:hopper" {
        // `HopperBlock.onPlace`, offsets 0-20: `if (oldState.is(this))
        // return; checkPoweredState(...)`. A hopper set down beside a lever
        // that is already on is written `enabled=false` straight away.
        if block_of(old) != block_of(state) {
            let power = Redstone::new(blocks, signals, shapes);
            crate::dispenser::hopper_check_powered_state(
                &power, blocks, shapes, pos, state, updates,
            );
        }
    } else if crate::piston::is_piston(block_of(state)) {
        // `PistonBaseBlock.onPlace`, offsets 0-35, read from the jar:
        //
        // ```java
        // if (oldState.is(state.getBlock())) return;
        // if (!level.isClientSide && level.getBlockEntity(pos) == null)
        //     checkIfExtend(level, pos, state);
        // ```
        //
        // The `getBlockEntity` test is at the piston's **own** position, not
        // the one in front — it is the half-extended case, where a
        // `moving_piston` and its `PistonMovingBlockEntity` already sit here.
        // This server places no such block entity, so the test is
        // constant-true; named rather than silently dropped, because it stops
        // being constant the day `PistonMovingBlockEntity` lands.
        //
        // `setPlacedBy` is the same `checkIfExtend` call with no guard at
        // all, and it runs on the same placement, which is why one arm serves
        // both: `BlockEvents::queue` is a set, so the second call collapses
        // into the first rather than queueing a duplicate event.
        if block_of(old) != block_of(state) {
            crate::piston::neighbor_changed(
                blocks,
                Some(signals),
                pos,
                state,
                crate::piston::WORLD_MIN_Y,
                crate::piston::WORLD_MAX_Y,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A world whose predicates are decided by block name, and whose writes
    /// stick.
    ///
    /// # What this double deliberately does not flatten
    ///
    /// Defect class #20 was a `Fake::state_id` that answered `Some(1)` for
    /// every state, which made an id-keyed table inert and let two injected
    /// bugs through. `state_id` here is a real function of the string — every
    /// distinct state gets a distinct id — so a caller that keys on it is
    /// exercised rather than short-circuited.
    ///
    /// `sturdy`, `conductor` and `source` are three *separate* predicates over
    /// the same names, because the rules under test turn on their differences:
    /// `glass` is sturdy on every face and is **not** a conductor;
    /// `redstone_block` is sturdy and a source and **not** a conductor;
    /// `stone` is sturdy and a conductor and **not** a source. A double that
    /// answered one boolean for all three would pass every test here and
    /// prove none of them.
    struct World {
        states: Mutex<BTreeMap<BlockPos, String>>,
    }

    impl World {
        fn new(initial: &[(BlockPos, &str)]) -> Self {
            Self {
                states: Mutex::new(
                    initial
                        .iter()
                        .map(|(p, s)| (*p, (*s).to_string()))
                        .collect(),
                ),
            }
        }

        fn at(&self, pos: BlockPos) -> String {
            self.states
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }

        fn power_at(&self, pos: BlockPos) -> Option<u8> {
            let s = self.at(pos);
            crate::wire::is_wire(&s)
                .then(|| crate::placement::property_of(&s, "power").and_then(|p| p.parse().ok()))
                .flatten()
        }
    }

    fn solid(state: &str) -> bool {
        matches!(
            block_of(state),
            "minecraft:stone" | "minecraft:glass" | "minecraft:redstone_block"
        )
    }

    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at(pos)
        }

        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            let mut map = self.states.lock().unwrap();
            let old = map.get(&pos).cloned().unwrap_or_default();
            if old == state {
                return None;
            }
            map.insert(pos, state.to_string());
            self.state_id(state)
        }

        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }

        fn has_state(&self, _state: &str) -> bool {
            true
        }

        /// A real hash of the state string. See the type docs: an id that is
        /// the same for every state is a key that has been collapsed.
        fn state_id(&self, state: &str) -> Option<i32> {
            let mut h: i32 = 17;
            for b in state.bytes() {
                h = h.wrapping_mul(31).wrapping_add(i32::from(b));
            }
            Some(h & 0x7fff_ffff)
        }

        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.at(pos))
        }

        fn face_sturdy(&self, pos: BlockPos, _face: Face) -> bool {
            solid(&self.at(pos))
        }

        /// `glass` is full on every face and is not a conductor, which is the
        /// whole reason `block_props.json` grew a column of its own.
        fn redstone_conductor(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:stone"
        }

        fn signal_source(&self, pos: BlockPos) -> bool {
            matches!(
                block_of(&self.at(pos)),
                "minecraft:redstone_block"
                    | "minecraft:lever"
                    | "minecraft:redstone_torch"
                    | "minecraft:redstone_wall_torch"
                    | "minecraft:redstone_wire"
            )
        }

        fn water_at(&self, _pos: BlockPos) -> bool {
            false
        }

        fn legacy_fluid_at(&self, _pos: BlockPos) -> String {
            "minecraft:air".to_string()
        }
    }

    /// A signal table holding exactly the states these tests use.
    ///
    /// The numbers are **copied from `data/vanilla/signal.json`**, not
    /// invented: `redstone_torch[lit=true]` really is 15 everywhere but UP and
    /// strong only DOWN, and the wall torch really is silent towards the wall
    /// it hangs on. Writing plausible ones instead would make every test below
    /// a test of my expectations.
    fn table() -> SignalTable {
        let json = r#"{
          "rng": "signal",
          "states": 22,
          "quiet": 12,
          "sources": 9,
          "refusedStates": 1,
          "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
          "refused": [],
          "unanswerable": [
            {"state": "minecraft:comparator[facing=north,mode=compare,powered=false]",
             "id": 900, "source": true, "reads": "getSignal read a block entity"}
          ],
          "table": [
            {"state": "minecraft:redstone_block", "id": 1, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,0,0,0,0]},
            {"state": "minecraft:redstone_torch[lit=true]", "id": 2, "source": true,
             "signal": [15,0,15,15,15,15], "direct": [15,0,0,0,0,0]},
            {"state": "minecraft:redstone_torch[lit=false]", "id": 3, "source": true,
             "signal": [0,0,0,0,0,0], "direct": [0,0,0,0,0,0]},
            {"state": "minecraft:redstone_wall_torch[facing=north,lit=true]", "id": 4, "source": true,
             "signal": [15,15,0,15,15,15], "direct": [15,0,0,0,0,0]},
            {"state": "minecraft:redstone_wall_torch[facing=south,lit=true]", "id": 5, "source": true,
             "signal": [15,15,15,0,15,15], "direct": [15,0,0,0,0,0]},
            {"state": "minecraft:lever[face=floor,facing=north,powered=true]", "id": 6, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,15,0,0,0,0]},
            {"state": "minecraft:lever[face=wall,facing=east,powered=true]", "id": 7, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,0,0,0,15]},
            {"state": "minecraft:lever[face=wall,facing=north,powered=true]", "id": 8, "source": true,
             "signal": [15,15,15,15,15,15], "direct": [0,0,15,0,0,0]},
            {"state": "minecraft:lever[face=wall,facing=north,powered=false]", "id": 9, "source": true,
             "signal": [0,0,0,0,0,0], "direct": [0,0,0,0,0,0]}
          ]
        }"#;
        write_and_load(json).expect("the test table should load")
    }

    fn write_and_load(json: &str) -> io::Result<SignalTable> {
        let dir = std::env::temp_dir().join(format!(
            "rustmc-signal-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("signal.json");
        std::fs::write(&path, json)?;
        let out = SignalTable::load(&path);
        let _ = std::fs::remove_file(&path);
        out
    }

    fn dust(power: u8) -> String {
        format!("minecraft:redstone_wire[east=side,north=none,power={power},south=none,west=side]")
    }

    // ---- the table itself ------------------------------------------------

    /// **A refusal is not a zero**, and the type exists to stop them being the
    /// same value. The degenerate is a state the harness measured as silent,
    /// which really is zero and must not read as a refusal.
    #[test]
    fn a_refusal_and_a_measured_zero_are_different_answers() {
        let t = table();
        assert_eq!(
            t.signal(
                "minecraft:comparator[facing=north,mode=compare,powered=false]",
                Face::North
            ),
            Signal::Refused
        );
        assert_eq!(
            t.signal("minecraft:redstone_torch[lit=false]", Face::North),
            Signal::Measured(0)
        );
        assert_eq!(
            t.signal("minecraft:stone", Face::North),
            Signal::Measured(0),
            "a state in neither map is the measured-quiet set, not a refusal"
        );
    }

    /// `isSignalSource` is total where the two signal lookups are not: a
    /// refused state still answers it.
    #[test]
    fn a_refused_state_still_answers_whether_it_is_a_source() {
        let t = table();
        assert!(t.is_signal_source("minecraft:comparator[facing=north,mode=compare,powered=false]"));
        assert!(t.is_signal_source("minecraft:redstone_block"));
        assert!(!t.is_signal_source("minecraft:stone"));
    }

    /// **The arithmetic guard.** A dump that lost rows still parses, still
    /// answers, and is wrong only where the missing rows were — which is
    /// exactly the failure a `states` count exists to catch.
    #[test]
    fn a_table_that_does_not_account_for_every_state_is_refused() {
        let bad = r#"{"rng":"signal","states":99,"quiet":0,
          "directionOrder":["DOWN","UP","NORTH","SOUTH","WEST","EAST"],
          "refused":[],"unanswerable":[],
          "table":[{"state":"minecraft:redstone_block","id":1,"source":true,
                    "signal":[15,15,15,15,15,15],"direct":[0,0,0,0,0,0]}]}"#;
        let err = write_and_load(bad).expect_err("99 states cannot be one row");
        assert!(
            err.to_string()
                .contains("accounts for 1 states, not the 99"),
            "{err}"
        );
    }

    /// A transposed column order is silent everywhere except in which face a
    /// thing powers, so it is refused at load rather than found later.
    #[test]
    fn a_table_in_another_direction_order_is_refused() {
        let bad = r#"{"rng":"signal","states":0,"quiet":0,
          "directionOrder":["NORTH","SOUTH","WEST","EAST","DOWN","UP"],
          "refused":[],"unanswerable":[],"table":[]}"#;
        let err = write_and_load(bad).expect_err("a rotated order must not load");
        assert!(err.to_string().contains("direction order"), "{err}");
    }

    /// The ids are not the key, but they are checked, because a dump that
    /// repeats one has been generated twice into the same buffer.
    #[test]
    fn a_repeated_state_id_is_refused() {
        let bad = r#"{"rng":"signal","states":2,"quiet":0,
          "directionOrder":["DOWN","UP","NORTH","SOUTH","WEST","EAST"],
          "refused":[],"unanswerable":[],
          "table":[{"state":"minecraft:redstone_block","id":1,"source":true,
                    "signal":[15,15,15,15,15,15],"direct":[0,0,0,0,0,0]},
                   {"state":"minecraft:lever[face=floor,facing=north,powered=true]","id":1,
                    "source":true,"signal":[0,0,0,0,0,0],"direct":[0,0,0,0,0,0]}]}"#;
        let err = write_and_load(bad).expect_err("a repeated id must not load");
        assert!(err.to_string().contains("not injective"), "{err}");
    }

    // ---- SignalGetter ----------------------------------------------------

    /// **The conductor branch of `SignalGetter.getSignal`.** Stone is not a
    /// source, but a torch under it strongly powers it, so dust beside the
    /// stone reads 15 *through* it.
    ///
    /// Its degenerate is glass in the same place with the same torch under it:
    /// glass is full on every face and is **not** a redstone conductor, so the
    /// strong signal stops there and the reading is 0. One block name apart,
    /// and a port that answered `isRedstoneConductor` from the sturdy column
    /// gets the second one wrong and nothing else in this file notices.
    #[test]
    fn a_conductor_carries_a_strong_signal_and_glass_does_not() {
        let t = table();
        let carried = World::new(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&carried, &t, None);
        assert_eq!(p.get_signal((1, 0, 0), Face::East), 15);

        let blocked = World::new(&[
            ((1, 0, 0), "minecraft:glass"),
            ((1, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&blocked, &t, None);
        assert_eq!(
            p.get_signal((1, 0, 0), Face::East),
            0,
            "glass is not a redstone conductor, so nothing is carried through it"
        );
    }

    /// **`getDirectSignalTo` asks each face about the block on the other side
    /// of it.** A torch's strong signal is `DOWN` only, which reaches the
    /// block *above* the torch. Beside it there is nothing strong at all.
    #[test]
    fn a_torch_powers_the_block_above_it_strongly_and_the_one_beside_it_not_at_all() {
        let t = table();
        let above = World::new(&[
            ((0, 0, 0), "minecraft:stone"),
            ((0, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&above, &t, None);
        assert_eq!(p.get_direct_signal_to((0, 0, 0)), 15);

        let beside = World::new(&[
            ((0, 0, 0), "minecraft:stone"),
            ((1, 0, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&beside, &t, None);
        assert_eq!(
            p.get_direct_signal_to((0, 0, 0)),
            0,
            "a torch beside a block does not strongly power it"
        );
    }

    /// **`shouldSignal`, the field, and the whole reason it exists.**
    ///
    /// `getBlockSignal` must not see a wire — that is what stops a dust
    /// counting its neighbour at full strength as a *source* when the decay
    /// rule is about to count it at strength minus one. The pair is the same
    /// geometry with the neighbour swapped: a wire at 15 must read 0, a
    /// redstone block at 15 must read 15.
    ///
    /// A port that dropped the flag passes the second assertion and fails the
    /// first, and a line of dust would then never decay.
    #[test]
    fn get_block_signal_cannot_see_a_wire_and_can_see_everything_else() {
        let t = table();
        let wire = World::new(&[((1, 0, 0), dust(15).as_str())]);
        let p = Redstone::new(&wire, &t, None);
        assert_eq!(p.get_block_signal((0, 0, 0)), 0);
        assert_eq!(
            p.get_best_neighbor_signal((0, 0, 0)),
            15,
            "outside getBlockSignal the same wire is visible, which is what makes the flag a flag"
        );

        let block = World::new(&[((1, 0, 0), "minecraft:redstone_block")]);
        let p = Redstone::new(&block, &t, None);
        assert_eq!(p.get_block_signal((0, 0, 0)), 15);
    }

    /// The flag is restored on the way out, so one `getBlockSignal` does not
    /// silence every later read.
    #[test]
    fn get_block_signal_puts_the_flag_back() {
        let t = table();
        let w = World::new(&[((1, 0, 0), dust(9).as_str())]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(p.get_block_signal((0, 0, 0)), 0);
        assert_eq!(p.get_signal((1, 0, 0), Face::East), 9);
    }

    /// **`RedStoneWireBlock.getSignal` flips the direction before asking about
    /// its own side**, and the sides it asks about are **re-derived from the
    /// world** rather than read off the state it was handed.
    ///
    /// The second half of that refuted the first version of this test, which
    /// built two states with different stored sides in an empty world and
    /// expected them to answer differently. They do not: `getConnectionState`
    /// starts from an all-`none` base and computes all four, so in an empty
    /// world every powered dust re-derives to a **cross** and signals in all
    /// four horizontal directions whatever its stored sides say. That is the
    /// `lone` case below, kept as the control.
    ///
    /// The discriminating arrangement needs a genuinely asymmetric shape, and
    /// getting one takes three blocks: dust north, dust west, conductor east.
    /// The conductor blocks east; north and west are connected, so `noNS` and
    /// `noEW` are both false and none of the four straightening `if`s fires.
    /// The wire settles at `north=side, west=side, south=none, east=none`.
    ///
    /// Asked from the east — `getSignal(pos, EAST)`, which is how the block at
    /// `pos.east()` asks — the side facing the asker is `WEST`, which is
    /// connected, so the answer is 9. A port reading `dir` instead of
    /// `dir.getOpposite()` would look at `east`, find `none`, and answer 0.
    /// Every one of the four directions separates the two here, which is what
    /// the symmetric shapes could not do.
    #[test]
    fn a_powered_wire_signals_only_along_the_sides_it_connects_to() {
        let t = table();
        let stored = "minecraft:redstone_wire[east=none,north=side,power=9,south=none,west=side]";

        let shaped = World::new(&[
            ((0, 0, -1), dust(0).as_str()),
            ((-1, 0, 0), dust(0).as_str()),
            ((1, 0, 0), "minecraft:stone"),
        ]);
        let p = Redstone::new(&shaped, &t, None);
        assert_eq!(
            p.wire_get_signal(stored, (0, 0, 0), Face::East),
            9,
            "asked from the east, the side facing the asker is WEST, which is connected"
        );
        assert_eq!(
            p.wire_get_signal(stored, (0, 0, 0), Face::West),
            0,
            "asked from the west, the side facing the asker is EAST, which the conductor blocks"
        );
        assert_eq!(
            p.wire_get_signal(stored, (0, 0, 0), Face::South),
            9,
            "asked from the south, the facing side is NORTH, where the other dust is"
        );
        assert_eq!(
            p.wire_get_signal(stored, (0, 0, 0), Face::North),
            0,
            "asked from the north, the facing side is SOUTH, which is empty"
        );

        // The control, and the refutation: the same stored state alone in the
        // world re-derives to a cross and answers in every direction.
        let lone = World::new(&[]);
        let p = Redstone::new(&lone, &t, None);
        for d in [Face::North, Face::South, Face::East, Face::West] {
            assert_eq!(
                p.wire_get_signal(stored, (0, 0, 0), d),
                9,
                "the stored sides do not decide; getConnectionState re-derives them"
            );
        }
    }

    /// `DOWN` is answered before anything else is read, and `UP` skips the
    /// connection test entirely.
    #[test]
    fn a_wire_never_signals_downwards_and_always_signals_upwards() {
        let t = table();
        let w = World::new(&[]);
        let p = Redstone::new(&w, &t, None);
        let dot = "minecraft:redstone_wire[east=none,north=none,power=7,south=none,west=none]";
        assert_eq!(p.wire_get_signal(dot, (0, 0, 0), Face::Down), 0);
        assert_eq!(
            p.wire_get_signal(dot, (0, 0, 0), Face::Up),
            7,
            "UP returns before the connection test, so a dot still signals upwards"
        );
    }

    // ---- the measured table against the hand port ------------------------
    //
    // These two read `data/vanilla/signal.json`, which is gitignored and
    // generated per tree by `ci/gen-gameplay-tables.sh`. That is the same
    // dependency the three worldgen reference tests already have and it is
    // deliberate: a check that skipped when the data was missing would be
    // green on exactly the trees where nobody had generated the thing it
    // checks.

    /// A path relative to the repo root.
    ///
    /// A lib test's working directory is the *package* directory, not the
    /// workspace root, so the bare `data/vanilla/...` the server itself uses
    /// resolves to nothing here. `crates/rustmc-server/tests/*` already carry
    /// this helper for the same reason.
    fn repo(relative: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    fn real_table() -> SignalTable {
        SignalTable::load(&repo("data/vanilla/signal.json")).expect(
            "data/vanilla/signal.json — run ci/gen-gameplay-tables.sh, as the worldgen \
             reference tests need their own data too",
        )
    }

    /// **The 81 wire rows vanilla could answer, spent on the port that answers
    /// the other 1,215.**
    ///
    /// `RedStoneWireBlock.getSignal` returns at `own == 0` before it reads
    /// anything, so `SignalGolden`'s one-position level could measure every
    /// `power=0` state and refused every other. Those 81 rows are therefore
    /// the only measurement that exists of the function [`Redstone::wire_get_signal`]
    /// reimplements, and this is what they are for. Under a dispatch that sent
    /// the answerable states to the table and only the refused ones to the
    /// port — which is what this module did first — they would have been spent
    /// on nothing.
    ///
    /// **The world here panics if it is read at all**, which makes this an
    /// assertion about the access shape as well as about the values: a port
    /// that computed the connection state before testing `own == 0` would give
    /// the same 81 zeros and fail here.
    ///
    /// The count is asserted so that a table which lost its wire rows makes
    /// this fail rather than pass vacuously.
    #[test]
    fn the_ported_wire_rule_reproduces_every_measured_wire_row() {
        struct Untouchable;
        impl Blocks for Untouchable {
            fn state_at(&self, pos: BlockPos) -> String {
                panic!("a power=0 wire must answer getSignal without reading {pos:?}")
            }
            fn set_block(&self, _p: BlockPos, _s: &str) -> Option<i32> {
                unreachable!()
            }
            fn default_state(&self, _b: &str) -> Option<String> {
                unreachable!()
            }
            fn has_state(&self, _s: &str) -> bool {
                unreachable!()
            }
            fn state_id(&self, _s: &str) -> Option<i32> {
                unreachable!()
            }
            fn replaceable(&self, _p: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
                unreachable!()
            }
            fn face_sturdy(&self, p: BlockPos, _f: Face) -> bool {
                panic!("a power=0 wire must not ask about the face at {p:?}")
            }
            fn redstone_conductor(&self, p: BlockPos) -> bool {
                panic!("a power=0 wire must not ask whether {p:?} conducts")
            }
            fn signal_source(&self, p: BlockPos) -> bool {
                panic!("a power=0 wire must not ask whether {p:?} is a source")
            }
            fn water_at(&self, _p: BlockPos) -> bool {
                unreachable!()
            }
            fn legacy_fluid_at(&self, _p: BlockPos) -> String {
                unreachable!()
            }
        }

        let t = real_table();
        let world = Untouchable;
        let p = Redstone::new(&world, &t, None);
        let mut checked = 0;
        for (state, signal, direct) in t.carried() {
            if !crate::wire::is_wire(state) {
                continue;
            }
            checked += 1;
            for d in DIRECTIONS {
                let i = usize::try_from(d.to_3d()).unwrap();
                assert_eq!(
                    p.wire_get_signal(state, (0, 64, 0), d),
                    signal[i],
                    "getSignal disagrees with vanilla for {state} towards {d:?}"
                );
                assert_eq!(
                    p.state_direct_signal(state, (0, 64, 0), d),
                    direct[i],
                    "getDirectSignal disagrees with vanilla for {state} towards {d:?}"
                );
            }
        }
        assert_eq!(
            checked, 81,
            "the 1,296 wire states hold 81 with power=0, and those are the answerable ones"
        );
    }

    /// **Two harnesses, one column, and they must agree.**
    ///
    /// `isSignalSource` is dumped by `BlockPropsGolden` into
    /// `block_props.json` and again by `SignalGolden` into `signal.json`, from
    /// two separate sweeps of the same registry. One column is a value; two
    /// independently generated columns that must agree is a gate, and it is
    /// the cheapest one available for catching a stale dump or a transposed
    /// row on either side.
    ///
    /// `block_props.json` is keyed by state and `signal.json` carries the
    /// states it answered, so the intersection is every state this table
    /// carries. Zero disagreements is the only acceptable answer.
    #[test]
    fn the_two_dumps_agree_about_which_states_are_signal_sources() {
        let t = real_table();
        let props = rustmc_behavior::block_props::BlockPropsTable::load(&repo(
            "data/vanilla/block_props.json",
        ))
        .expect("data/vanilla/block_props.json");
        let disagreements =
            t.disagreements_with_block_props(&|state| props.get(state).map(|p| p.signal_source));
        assert!(
            disagreements.is_empty(),
            "signal.json and block_props.json disagree about isSignalSource for {} states, \
             first few: {:?}",
            disagreements.len(),
            &disagreements[..disagreements.len().min(5)]
        );
    }

    // ---- getIncomingWireSignal -------------------------------------------

    /// One hop, one less.
    #[test]
    fn a_line_of_dust_loses_one_per_step() {
        let t = table();
        let w = World::new(&[((-1, 0, 0), dust(15).as_str())]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(incoming_wire_signal(&p, (0, 0, 0)), 14);
    }

    /// **The decay is clamped at zero, not wrapped.** A `u8` subtraction that
    /// was not saturating would give 255 here and a `i32` one would give -1;
    /// vanilla's `Math.max(0, best - 1)` gives 0.
    #[test]
    fn the_decay_is_clamped_at_zero() {
        let t = table();
        let w = World::new(&[]);
        let p = Redstone::new(&w, &t, None);
        assert_eq!(incoming_wire_signal(&p, (0, 0, 0)), 0);
    }

    /// **The ceiling that blocks a climb is over the WIRE, not over the
    /// neighbour.** Offset 61 is `aload_2` — the method's own `pos`.
    ///
    /// Geometry: a conductor to the east, dust on top of it, and a conductor
    /// directly over the subject. Vanilla reads the ceiling over the subject,
    /// finds a conductor, and does not take the climb: 0. A port that read
    /// `npos.above()` instead would find the *dust* there, which is not a
    /// conductor, and would take the climb: 14.
    ///
    /// The degenerate is the same arrangement with the subject's ceiling
    /// removed, which must give 14. Without it a port that simply never climbs
    /// passes the first assertion.
    #[test]
    fn the_ceiling_that_blocks_a_climb_is_over_the_wire_and_not_over_the_neighbour() {
        let t = table();
        let roofed = World::new(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, 1, 0), dust(15).as_str()),
            ((0, 1, 0), "minecraft:stone"),
        ]);
        let p = Redstone::new(&roofed, &t, None);
        assert_eq!(
            incoming_wire_signal(&p, (0, 0, 0)),
            0,
            "a conductor over the wire itself blocks the climb"
        );

        let open = World::new(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, 1, 0), dust(15).as_str()),
        ]);
        let p = Redstone::new(&open, &t, None);
        assert_eq!(
            incoming_wire_signal(&p, (0, 0, 0)),
            14,
            "with nothing overhead the same dust one step up is read"
        );
    }

    /// **The else branch**: a neighbour that is not a conductor lets a signal
    /// come *up* from the step below it. Glass is the sharp case — full on
    /// every face and not a conductor — so the port cannot answer this from
    /// the sturdy column.
    ///
    /// Its degenerate is stone in the same place, which is a conductor, takes
    /// the other arm of the `if`, and reads nothing.
    #[test]
    fn dust_reads_the_step_below_a_neighbour_that_is_not_a_conductor() {
        let t = table();
        let through_glass = World::new(&[
            ((1, 0, 0), "minecraft:glass"),
            ((1, -1, 0), dust(15).as_str()),
        ]);
        let p = Redstone::new(&through_glass, &t, None);
        assert_eq!(incoming_wire_signal(&p, (0, 0, 0)), 14);

        let through_stone = World::new(&[
            ((1, 0, 0), "minecraft:stone"),
            ((1, -1, 0), dust(15).as_str()),
        ]);
        let p = Redstone::new(&through_stone, &t, None);
        assert_eq!(
            incoming_wire_signal(&p, (0, 0, 0)),
            0,
            "a conductor beside the wire takes the other arm and the step below is never read"
        );
    }

    /// `calculateTargetStrength` takes the larger of the two mechanisms, and
    /// they are genuinely different numbers: a redstone block gives 15 through
    /// `getBlockSignal`, a wire at 15 gives 14 through the decay rule.
    #[test]
    fn the_target_strength_is_the_better_of_the_block_and_the_wire() {
        let t = table();
        let from_block = World::new(&[((1, 0, 0), "minecraft:redstone_block")]);
        let p = Redstone::new(&from_block, &t, None);
        assert_eq!(calculate_target_strength(&p, (0, 0, 0)), 15);

        let from_wire = World::new(&[((1, 0, 0), dust(15).as_str())]);
        let p = Redstone::new(&from_wire, &t, None);
        assert_eq!(calculate_target_strength(&p, (0, 0, 0)), 14);
    }

    // ---- the HashSet order -----------------------------------------------

    /// **Measured against a real JDK, not derived from the docs.**
    ///
    /// A seven-element `java.util.HashSet<BlockPos>` was built and iterated on
    /// the Threadripper's OpenJDK 25 for each of the three positions below,
    /// and these are the orders it printed. They are hardcoded because the
    /// point is to reproduce a specific structure, so a test that recomputed
    /// the expectation from this file's own algorithm would agree with any bug
    /// in it.
    ///
    /// The last two assertions are what stop this passing under the two
    /// obvious wrong answers: it is not insertion order and it is not sorted.
    #[test]
    fn the_neighbour_set_comes_out_in_java_hash_order() {
        let set_for = |p: BlockPos| {
            let mut v = vec![p];
            for d in DIRECTIONS {
                v.push(d.relative(p));
            }
            v
        };

        assert_eq!(
            hash_set_order(&set_for((0, 0, 0))),
            vec![
                (0, 0, 0),
                (0, 0, -1),
                (-1, 0, 0),
                (0, 0, 1),
                (1, 0, 0),
                (0, -1, 0),
                (0, 1, 0)
            ]
        );
        assert_eq!(
            hash_set_order(&set_for((2, 0, 0))),
            vec![
                (2, 1, 0),
                (1, 0, 0),
                (2, 0, 0),
                (2, 0, 1),
                (3, 0, 0),
                (2, -1, 0),
                (2, 0, -1)
            ]
        );
        assert_eq!(
            hash_set_order(&set_for((-3, 70, 5))),
            vec![
                (-3, 71, 5),
                (-3, 70, 4),
                (-4, 70, 5),
                (-3, 70, 5),
                (-3, 69, 5),
                (-3, 70, 6),
                (-2, 70, 5)
            ]
        );

        let insertion = set_for((2, 0, 0));
        let mut sorted = insertion.clone();
        sorted.sort_unstable();
        let got = hash_set_order(&insertion);
        assert_ne!(
            got, insertion,
            "insertion order would pass a do-nothing port"
        );
        assert_ne!(got, sorted, "sorted order would pass a tidy-minded port");
        assert_eq!(got.len(), 7, "seven distinct positions, none lost");
    }

    /// `Vec3i.hashCode` is `(y + z * 31) * 31 + x` in wrapping `int`
    /// arithmetic, and the sign matters: the negative cases are the ones a
    /// `usize` port gets wrong.
    #[test]
    fn the_position_hash_is_javas() {
        assert_eq!(block_pos_hash((0, 0, 0)), 0);
        assert_eq!(block_pos_hash((1, 0, 0)), 1);
        assert_eq!(block_pos_hash((0, 1, 0)), 31);
        assert_eq!(block_pos_hash((0, 0, 1)), 961);
        assert_eq!(block_pos_hash((-1, 0, 0)), -1);
        assert_eq!(block_pos_hash((0, -1, 0)), -31);
        assert_eq!(block_pos_hash((-3, 70, 5)), (70 + 5 * 31) * 31 - 3);
    }

    // ---- the queue -------------------------------------------------------

    /// **`NeighborUpdater.UPDATE_ORDER` is `WEST, EAST, DOWN, UP, NORTH,
    /// SOUTH`** and it is not the shape order. Observed rather than asserted
    /// on the constant: the wires record the order they were told in.
    #[test]
    fn the_neighbour_update_order_is_west_east_down_up_north_south() {
        assert_eq!(
            UPDATE_ORDER,
            [
                Face::West,
                Face::East,
                Face::Down,
                Face::Up,
                Face::North,
                Face::South
            ]
        );
        assert_ne!(
            UPDATE_ORDER,
            crate::shapes::UPDATE_SHAPE_ORDER,
            "the two branches of one `if` in setBlock use different orders"
        );
        assert_ne!(UPDATE_ORDER, DIRECTIONS);
    }

    /// **The skip direction is handled in two places and this is the pair that
    /// proves both.** `WEST` sits at index 0, so only the constructor can skip
    /// it; `DOWN` sits at index 2, so only `runNext` can.
    ///
    /// Written against the traversal directly, because the only caller passes
    /// `None` and a test that went through it would exercise neither.
    #[test]
    fn a_skipped_direction_is_skipped_wherever_it_sits_in_the_order() {
        let walk = |skip: Option<Face>| {
            let mut u = Update {
                source: (0, 0, 0),
                block: "minecraft:redstone_wire".to_string(),
                skip,
                idx: usize::from(skip == Some(UPDATE_ORDER[0])),
                simple: false,
            };
            let mut seen = Vec::new();
            loop {
                if u.idx >= UPDATE_ORDER.len() {
                    break;
                }
                seen.push(UPDATE_ORDER[u.idx]);
                u.idx += 1;
                if u.idx < UPDATE_ORDER.len() && Some(UPDATE_ORDER[u.idx]) == u.skip {
                    u.idx += 1;
                }
            }
            seen
        };
        assert_eq!(walk(None).len(), 6);
        assert!(!walk(Some(Face::West)).contains(&Face::West));
        assert_eq!(walk(Some(Face::West)).len(), 5);
        assert!(!walk(Some(Face::Down)).contains(&Face::Down));
        assert_eq!(walk(Some(Face::Down)).len(), 5);
        assert!(!walk(Some(Face::South)).contains(&Face::South));
        assert_eq!(walk(Some(Face::South)).len(), 5);
    }

    // ---- the torch -------------------------------------------------------

    /// **A torch reads the block holding it and nothing else.** Powered stone
    /// under a standing torch switches it; the same stone beside it does not.
    #[test]
    fn a_standing_torch_reads_the_block_below_it_only() {
        let t = table();
        let below = World::new(&[
            ((0, 0, 0), "minecraft:redstone_torch[lit=true]"),
            ((0, -1, 0), "minecraft:stone"),
            ((0, -2, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&below, &t, None);
        assert!(torch_has_neighbor_signal(
            &p,
            (0, 0, 0),
            "minecraft:redstone_torch[lit=true]"
        ));

        let beside = World::new(&[
            ((0, 0, 0), "minecraft:redstone_torch[lit=true]"),
            ((1, 0, 0), "minecraft:stone"),
            ((1, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&beside, &t, None);
        assert!(
            !torch_has_neighbor_signal(&p, (0, 0, 0), "minecraft:redstone_torch[lit=true]"),
            "a powered block beside a torch is not the block holding it"
        );
    }

    /// **A wall torch reads the block BEHIND it**, which is
    /// `facing.getOpposite()` and not `facing`. A torch with `facing=north`
    /// points north and hangs on the block to its south.
    ///
    /// The degenerate is the same powered block put to the north instead,
    /// which must not switch it. A port that dropped the `getOpposite` passes
    /// the second and fails the first.
    #[test]
    fn a_wall_torch_reads_the_block_behind_it() {
        let t = table();
        let torch = "minecraft:redstone_wall_torch[facing=north,lit=true]";
        let behind = World::new(&[
            ((0, 0, 1), "minecraft:stone"),
            ((0, -1, 1), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&behind, &t, None);
        assert!(
            torch_has_neighbor_signal(&p, (0, 0, 0), torch),
            "facing=north hangs on the block to the SOUTH"
        );

        let in_front = World::new(&[
            ((0, 0, -1), "minecraft:stone"),
            ((0, -1, -1), "minecraft:redstone_torch[lit=true]"),
        ]);
        let p = Redstone::new(&in_front, &t, None);
        assert!(!torch_has_neighbor_signal(&p, (0, 0, 0), torch));
    }

    /// **The schedule condition is `lit == hasNeighborSignal`, and this test
    /// exists because it was written the other way round.**
    ///
    /// A lit torch on an unpowered block is the steady state and must ask for
    /// nothing; a lit torch on a *powered* block is the one that has to go
    /// out. The inverted version gets both of those backwards and passes every
    /// other test in this file, which is how it survived being written — it
    /// was found by its own diagnostic firing inside an unrelated test.
    #[test]
    fn a_torch_asks_for_a_tick_only_when_it_agrees_with_the_block_holding_it() {
        let t = table();
        let torch = "minecraft:redstone_torch[lit=true]";

        let settled = World::new(&[((0, 0, 0), torch), ((0, -1, 0), "minecraft:stone")]);
        let p = Redstone::new(&settled, &t, None);
        assert!(
            !torch_should_schedule(&p, (0, 0, 0), torch),
            "lit on an unpowered block is the steady state"
        );

        // The same stone, now strongly powered by a torch under it.
        let doomed = World::new(&[
            ((0, 0, 0), torch),
            ((0, -1, 0), "minecraft:stone"),
            ((0, -2, 0), torch),
        ]);
        let p = Redstone::new(&doomed, &t, None);
        assert!(
            torch_should_schedule(&p, (0, 0, 0), torch),
            "lit on a powered block has to go out"
        );

        // And the unlit mirror, which is the other way the two can agree.
        let unlit = "minecraft:redstone_torch[lit=false]";
        let relight = World::new(&[((0, 0, 0), unlit), ((0, -1, 0), "minecraft:stone")]);
        let p = Redstone::new(&relight, &t, None);
        assert!(
            torch_should_schedule(&p, (0, 0, 0), unlit),
            "unlit on an unpowered block has to come back on"
        );
    }

    // ---- updatePowerStrength ---------------------------------------------

    /// **The whole body is inside `power != target`.** A wire told about a
    /// change that did not move it writes nothing and tells nobody, which is
    /// what stops a ring of dust updating for ever.
    #[test]
    fn a_wire_whose_strength_did_not_change_writes_nothing() {
        let t = table();
        let w = World::new(&[
            ((0, 0, 0), dust(0).as_str()),
            ((0, -1, 0), "minecraft:stone"),
        ]);
        let p = Redstone::new(&w, &t, None);
        let mut updates = crate::shapes::Effects::new();
        update_power_strength(&p, (0, 0, 0), &dust(0), &mut updates);
        assert!(
            updates.blocks.is_empty(),
            "nothing changed, so nothing was told"
        );
        assert_eq!(w.power_at((0, 0, 0)), Some(0));
    }

    /// And the other arm: a wire that *did* move writes its new power.
    #[test]
    fn a_wire_whose_strength_changed_writes_it() {
        let t = table();
        let w = World::new(&[
            ((0, 0, 0), dust(0).as_str()),
            ((0, -1, 0), "minecraft:stone"),
            ((1, 0, 0), "minecraft:redstone_block"),
        ]);
        let p = Redstone::new(&w, &t, None);
        let mut updates = crate::shapes::Effects::new();
        update_power_strength(&p, (0, 0, 0), &dust(0), &mut updates);
        assert_eq!(w.power_at((0, 0, 0)), Some(15));
    }

    // ---- the lamp --------------------------------------------------------

    /// **Check `360`, at the level of the mechanism, and the two arms are not
    /// symmetric.**
    ///
    /// A lever on the lamp's north face, flipped on: the lamp lights
    /// **immediately**, in this call, with no tick involved. Flipped off it
    /// only asks to be woken in four, which this server cannot do — so the
    /// second assertion is that the state does *not* change, and it is the
    /// honest expression of a gap rather than a passing test.
    ///
    /// The third case is the guard: a lamp already agreeing with its
    /// neighbours writes nothing. Without it two lamps beside each other would
    /// keep telling one another.
    #[test]
    fn a_lamp_lights_immediately_and_only_asks_to_go_out() {
        let t = table();
        let on = "minecraft:lever[face=wall,facing=north,powered=true]";
        let off = "minecraft:lever[face=wall,facing=north,powered=false]";
        let unlit = "minecraft:redstone_lamp[lit=false]";
        let lit = "minecraft:redstone_lamp[lit=true]";

        let lighting = World::new(&[((2, 0, 1), unlit), ((2, 0, 0), on)]);
        let mut updates = crate::shapes::Effects::new();
        lamp_neighbor_changed(&lighting, &t, None, (2, 0, 1), unlit, &mut updates);
        assert_eq!(
            lighting.state_at((2, 0, 1)),
            lit,
            "the `else` arm writes straight away — no tick, no delay"
        );

        // Lit, and the lever now off. Vanilla schedules four ticks; this
        // server has no lamp tick, so the lamp stays on and says so.
        let going_out = World::new(&[((2, 0, 1), lit), ((2, 0, 0), off)]);
        let mut updates = crate::shapes::Effects::new();
        lamp_neighbor_changed(&going_out, &t, None, (2, 0, 1), lit, &mut updates);
        assert_eq!(
            going_out.state_at((2, 0, 1)),
            lit,
            "unlighting is four ticks late and the tick is not ported"
        );
        assert!(updates.blocks.is_empty());

        // The guard: nothing changed, so nothing is written.
        let settled = World::new(&[((2, 0, 1), lit), ((2, 0, 0), on)]);
        let mut updates = crate::shapes::Effects::new();
        lamp_neighbor_changed(&settled, &t, None, (2, 0, 1), lit, &mut updates);
        assert!(
            updates.blocks.is_empty(),
            "a lamp that agrees with its neighbours is silent"
        );
    }

    /// The whole check `360` rig, through `updateNeighborsAt` — a lever placed
    /// on the lamp's north face, which is what `LeverBlock.pull` ends in.
    ///
    /// The lamp is one step north of the lever, so it is reached by
    /// `setBlock`'s own six-face update and not by anything deeper. That makes
    /// this the shortest path in the module and the first one measured against
    /// vanilla end to end.
    #[test]
    fn a_lever_beside_a_lamp_lights_it_through_the_neighbour_update() {
        let t = table();
        let on = "minecraft:lever[face=wall,facing=north,powered=true]";
        let w = World::new(&[
            ((2, 0, 1), "minecraft:redstone_lamp[lit=false]"),
            (
                (2, 0, 0),
                "minecraft:lever[face=wall,facing=north,powered=false]",
            ),
        ]);
        place(&w, &t, (2, 0, 0), on);
        assert_eq!(w.state_at((2, 0, 1)), "minecraft:redstone_lamp[lit=true]");
    }

    /// `getStateForPlacement` is `hasNeighborSignal` and nothing else. Both
    /// arms, because a rule that answered one constant would pass either
    /// alone.
    #[test]
    fn a_lamp_placed_into_a_powered_spot_arrives_lit() {
        let t = table();
        let powered = World::new(&[(
            (2, 0, 0),
            "minecraft:lever[face=wall,facing=north,powered=true]",
        )]);
        assert_eq!(
            lamp_placement(
                &powered,
                &t,
                (2, 0, 1),
                "minecraft:redstone_lamp[lit=false]"
            ),
            "minecraft:redstone_lamp[lit=true]"
        );
        let quiet = World::new(&[]);
        assert_eq!(
            lamp_placement(&quiet, &t, (2, 0, 1), "minecraft:redstone_lamp[lit=false]"),
            "minecraft:redstone_lamp[lit=false]"
        );
    }

    // ---- end to end, at the geometry the two checks build ----------------

    /// `Level.setBlock(pos, state, 3)`, restricted to the two things this
    /// module owns and **in vanilla's order**: `onPlace` from inside
    /// `LevelChunk.setBlockState`, then `updateNeighborsAt` from `setBlock`
    /// itself, with the OLD block as the source.
    ///
    /// Written out rather than routed through [`crate::shapes::set_block`]
    /// because that reads the process-wide table through [`shared`], which in
    /// a test is the real `data/vanilla/signal.json` or nothing at all —
    /// neither of which is the table these tests are about.
    ///
    /// **The second call is not decoration.** The first version of the torch
    /// test called `torch_on_place` alone and the dust beside the torch stayed
    /// at 0, because `onPlace` tells the torch's neighbours' *neighbours* and
    /// never the neighbours themselves. It is `setBlock`'s own line that tells
    /// the block next door.
    fn place(w: &World, t: &SignalTable, pos: BlockPos, state: &str) {
        let old = w.state_at(pos);
        w.set_block(pos, state);
        let mut updates = crate::shapes::Effects::new();
        on_place(w, Some(t), None, pos, state, &old, &mut updates);
        update_neighbors_at(w, Some(t), None, pos, block_of(&old), &mut updates);
    }

    /// **Check `370`, at the level of the mechanism.** Three dust in a line on
    /// stone, then a redstone block at the far end. The line must read 15, 14,
    /// 13 outward from the source.
    ///
    /// The block is placed last so the whole line re-powers in one pass, which
    /// is what the check's own comment says it is for.
    #[test]
    fn a_redstone_block_beside_a_line_of_dust_reads_fifteen_fourteen_thirteen() {
        let t = table();
        let w = World::new(&[
            ((1, -1, 0), "minecraft:stone"),
            ((2, -1, 0), "minecraft:stone"),
            ((3, -1, 0), "minecraft:stone"),
            ((4, -1, 0), "minecraft:stone"),
            ((1, 0, 0), dust(0).as_str()),
            ((2, 0, 0), dust(0).as_str()),
            ((3, 0, 0), dust(0).as_str()),
        ]);
        place(&w, &t, (4, 0, 0), "minecraft:redstone_block");
        assert_eq!(w.power_at((3, 0, 0)), Some(15), "next to the source");
        assert_eq!(w.power_at((2, 0, 0)), Some(14), "one hop");
        assert_eq!(w.power_at((1, 0, 0)), Some(13), "two hops");
    }

    /// **Check `100`, at the level of the mechanism.** Two dust, then a torch
    /// beside them. The torch is a source at 15 and the dust decays from it.
    ///
    /// This runs through `torch_on_place`, which is the path that matters:
    /// the torch tells its six neighbours' six neighbours, and the far dust is
    /// two steps away and is reached by nothing else.
    #[test]
    fn a_torch_placed_beside_dust_powers_it() {
        let t = table();
        let w = World::new(&[
            ((1, -1, 0), "minecraft:stone"),
            ((2, -1, 0), "minecraft:stone"),
            ((3, -1, 0), "minecraft:stone"),
            ((1, 0, 0), dust(0).as_str()),
            ((2, 0, 0), dust(0).as_str()),
        ]);
        place(&w, &t, (3, 0, 0), "minecraft:redstone_torch[lit=true]");
        assert_eq!(w.power_at((2, 0, 0)), Some(15));
        assert_eq!(w.power_at((1, 0, 0)), Some(14));
    }

    /// **The `oldState.is(this)` guard on the wire's `onPlace`.** A wire
    /// replacing a wire does nothing, which is what stops
    /// `updatePowerStrength`'s own write re-entering the method for every
    /// power change.
    ///
    /// Observed as "the second call changes nothing", with the first call as
    /// the control that shows the arrangement is live at all.
    #[test]
    fn a_wire_replacing_a_wire_does_not_re_enter_on_place() {
        let t = table();
        let w = World::new(&[
            ((0, -1, 0), "minecraft:stone"),
            ((0, 0, 0), dust(0).as_str()),
            ((1, 0, 0), "minecraft:redstone_block"),
        ]);
        let mut updates = crate::shapes::Effects::new();
        wire_on_place(
            &w,
            &t,
            None,
            (0, 0, 0),
            &dust(0),
            "minecraft:air",
            &mut updates,
        );
        assert_eq!(
            w.power_at((0, 0, 0)),
            Some(15),
            "the control: placing works"
        );

        let seen = updates.blocks.len();
        wire_on_place(&w, &t, None, (0, 0, 0), &dust(15), &dust(0), &mut updates);
        assert_eq!(
            updates.blocks.len(),
            seen,
            "wire replacing wire returns at the first line and touches nothing"
        );
    }

    // ---- the three scheduled-tick rules ----------------------------------
    //
    // Each of these is a rule that did not exist before 2026-08-11, so each
    // fails against the tree that preceded it by construction: delete the
    // matching arm and the assertion below reads the unchanged state. The
    // corpus subset run is the other half of the evidence — `400`, `410` and
    // `440` were all `diverge` on the gate of 2026-08-11 18:48 and are
    // measured after these landed.

    /// **The torch goes out on its tick and not before**, and the geometry is
    /// the one `a_torch_powers_the_block_above_it_strongly_and_the_one_beside_it_not_at_all`
    /// already established: a torch strongly powers the block above it, so
    /// stone at `(0,0,0)` with a torch beneath is a powered support.
    ///
    /// The subject torch stands on that stone. `hasNeighborSignal` reads
    /// `level.hasSignal(pos.below(), DOWN)`, which is the stone, so it is
    /// `true` and `RedstoneTorchBlock.tick` takes the `LIT && hasSignal` arm.
    #[test]
    fn a_lit_torch_on_a_powered_block_goes_out_when_its_tick_fires() {
        let t = table();
        let w = World::new(&[
            ((0, 1, 0), "minecraft:redstone_torch[lit=true]"),
            ((0, 0, 0), "minecraft:stone"),
            ((0, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let mut updates = crate::shapes::Effects::new();
        torch_tick(
            &w,
            &t,
            None,
            (0, 1, 0),
            "minecraft:redstone_torch[lit=true]",
            0,
            &mut updates,
        );
        assert_eq!(
            w.at((0, 1, 0)),
            "minecraft:redstone_torch[lit=false]",
            "LIT and hasNeighborSignal both true is the arm that writes"
        );
        assert!(
            updates.level_events.is_empty(),
            "one toggle is not eight, so no burnout event"
        );
        assert!(
            updates.ticks.is_empty(),
            "and no 160-tick restart either — that is behind the burnout"
        );
    }

    /// The degenerate, and it is the one that separates the rule from
    /// "always turn the torch off": the same torch with the source removed
    /// reaches the same `tick` and writes nothing, because `hasNeighborSignal`
    /// is false and the `LIT` arm is guarded on it.
    #[test]
    fn a_lit_torch_on_an_unpowered_block_is_left_alone_by_its_tick() {
        let t = table();
        let w = World::new(&[
            ((0, 1, 0), "minecraft:redstone_torch[lit=true]"),
            ((0, 0, 0), "minecraft:stone"),
        ]);
        let mut updates = crate::shapes::Effects::new();
        torch_tick(
            &w,
            &t,
            None,
            (0, 1, 0),
            "minecraft:redstone_torch[lit=true]",
            0,
            &mut updates,
        );
        assert_eq!(w.at((0, 1, 0)), "minecraft:redstone_torch[lit=true]");
        assert!(updates.blocks.is_empty());
    }

    /// `neighborChanged`'s condition is `==`, not `!=`, and this is the pair
    /// that proves it both ways round: a lit torch over a powered block asks
    /// for a tick, and the steady states do not.
    #[test]
    fn only_a_torch_disagreeing_with_its_support_asks_for_a_tick() {
        let t = table();
        let powered = World::new(&[
            ((0, 1, 0), "minecraft:redstone_torch[lit=true]"),
            ((0, 0, 0), "minecraft:stone"),
            ((0, -1, 0), "minecraft:redstone_torch[lit=true]"),
        ]);
        let mut updates = crate::shapes::Effects::new();
        torch_neighbor_changed(
            &powered,
            &t,
            (0, 1, 0),
            "minecraft:redstone_torch[lit=true]",
            &mut updates,
        );
        assert_eq!(
            updates.ticks,
            vec![crate::shapes::TickRequest {
                pos: (0, 1, 0),
                block: "minecraft:redstone_torch".to_string(),
                delay: TORCH_DELAY,
                priority: crate::ticks::PRIORITY_NORMAL,
            }],
            "lit == hasNeighborSignal, so it schedules"
        );

        let quiet = World::new(&[
            ((0, 1, 0), "minecraft:redstone_torch[lit=true]"),
            ((0, 0, 0), "minecraft:stone"),
        ]);
        let mut updates = crate::shapes::Effects::new();
        torch_neighbor_changed(
            &quiet,
            &t,
            (0, 1, 0),
            "minecraft:redstone_torch[lit=true]",
            &mut updates,
        );
        assert!(
            updates.ticks.is_empty(),
            "a lit torch on an unpowered block is the steady state and asks for nothing"
        );
    }

    /// **The observer's two writes, in order, from one shape update** — which
    /// is the whole of what `440` tests.
    ///
    /// `updateShape` from the faced direction books the first tick; that tick
    /// powers the observer and books the second; the second unpowers it and
    /// books nothing. A port that wrote once, or that rescheduled from both
    /// branches, fails here.
    #[test]
    fn an_observer_pulses_powered_on_then_off_and_stops() {
        let t = table();
        let off = "minecraft:observer[facing=south,powered=false]";
        let w = World::new(&[((0, 0, 0), off)]);

        let mut updates = crate::shapes::Effects::new();
        observer_update_shape(off, Face::South, (0, 0, 0), &mut updates);
        assert_eq!(
            updates.ticks.len(),
            1,
            "the block it FACES changed, so startSignal runs"
        );
        assert_eq!(updates.ticks[0].delay, OBSERVER_DELAY);

        let mut updates = crate::shapes::Effects::new();
        observer_tick(&w, &t, None, (0, 0, 0), off, &mut updates);
        assert_eq!(
            w.at((0, 0, 0)),
            "minecraft:observer[facing=south,powered=true]"
        );
        assert_eq!(
            updates.ticks.len(),
            1,
            "the unpowered arm books the second half of the pulse"
        );

        let on = "minecraft:observer[facing=south,powered=true]";
        let mut updates = crate::shapes::Effects::new();
        observer_tick(&w, &t, None, (0, 0, 0), on, &mut updates);
        assert_eq!(w.at((0, 0, 0)), off);
        assert!(
            updates.ticks.is_empty(),
            "the powered arm has no scheduleTick, so the pulse stops at two"
        );
    }

    /// A change on any face that is **not** the one it faces books nothing.
    /// Without this the observer would answer its own output side and pulse
    /// for ever.
    #[test]
    fn an_observer_ignores_a_change_behind_it() {
        let off = "minecraft:observer[facing=south,powered=false]";
        let mut updates = crate::shapes::Effects::new();
        for d in [Face::North, Face::Up, Face::Down, Face::East, Face::West] {
            observer_update_shape(off, d, (0, 0, 0), &mut updates);
        }
        assert!(updates.ticks.is_empty());
    }

    /// **The repeater's delay is `DELAY * 2` and its priority is the middle
    /// rung**, and both come out of `checkTickOnNeighbor` rather than out of
    /// `tick`.
    ///
    /// `410` cycles DELAY to 2, so the request is for 4 ticks. The repeater is
    /// off and should turn on, which is `HIGH` — `VERY_HIGH` is the falling
    /// edge and `EXTREMELY_HIGH` needs a diode pointing into its side.
    #[test]
    fn a_repeater_fed_from_behind_books_a_tick_at_twice_its_delay() {
        let t = table();
        let rep = "minecraft:repeater[delay=2,facing=north,locked=false,powered=false]";
        let w = World::new(&[((0, 0, 0), rep), ((0, 0, -1), "minecraft:redstone_block")]);
        let mut updates = crate::shapes::Effects::new();
        diode_neighbor_changed(&w, &t, (0, 0, 0), rep, &mut updates);
        assert_eq!(
            updates.ticks,
            vec![crate::shapes::TickRequest {
                pos: (0, 0, 0),
                block: "minecraft:repeater".to_string(),
                delay: 4,
                priority: PRIORITY_HIGH,
            }]
        );

        let mut updates = crate::shapes::Effects::new();
        repeater_tick(&w, &t, None, (0, 0, 0), rep, &mut updates);
        assert_eq!(
            w.at((0, 0, 0)),
            "minecraft:repeater[delay=2,facing=north,locked=false,powered=true]"
        );
        assert!(
            updates.ticks.is_empty(),
            "shouldTurnOn is still true, so the on arm books nothing"
        );
    }

    /// **`FACING` names the input.** The same rig with the redstone block on
    /// the other side — the side vanilla calls "in front" — books nothing,
    /// because `getInputSignal` reads `pos.relative(FACING)` and that is now
    /// air. A port that read the opposite direction passes the test above and
    /// fails this one.
    #[test]
    fn a_repeater_does_not_read_the_block_it_outputs_into() {
        let t = table();
        let rep = "minecraft:repeater[delay=2,facing=north,locked=false,powered=false]";
        let w = World::new(&[((0, 0, 0), rep), ((0, 0, 1), "minecraft:redstone_block")]);
        let mut updates = crate::shapes::Effects::new();
        diode_neighbor_changed(&w, &t, (0, 0, 0), rep, &mut updates);
        assert!(updates.ticks.is_empty());
    }

    /// **A comparator is not dispatched through the repeater's rule.** It
    /// overrides five of the methods `diode_neighbor_changed` calls, so
    /// answering for it would be wrong rather than silent, and
    /// `420-a-comparator-switches-to-subtract-mode` matches today.
    #[test]
    fn a_comparator_is_left_to_a_port_that_does_not_exist_yet() {
        let t = table();
        let cmp = "minecraft:comparator[facing=north,mode=compare,powered=false]";
        let w = World::new(&[((0, 0, 0), cmp), ((0, 0, -1), "minecraft:redstone_block")]);
        let mut updates = crate::shapes::Effects::new();
        diode_neighbor_changed(&w, &t, (0, 0, 0), cmp, &mut updates);
        assert!(
            updates.ticks.is_empty(),
            "the same rig that books a tick for a repeater must book none for a comparator"
        );
    }

    /// `SimpleNeighborUpdate` tells **one** position, where
    /// `MultiNeighborUpdate` tells six. The two arms share `addAndRun`, so the
    /// difference has to be visible from the outside or it is not ported.
    #[test]
    fn a_simple_neighbour_update_reaches_one_position_and_a_full_one_reaches_six() {
        let t = table();
        // Six torches around the origin, each on its own stone, each lit and
        // each over an unpowered support — so none of them schedules. Then
        // one torch directly at the origin's north, sitting on a powered
        // block, which is the only one that can ask for a tick.
        let w = World::new(&[
            ((0, 0, -1), "minecraft:redstone_torch[lit=true]"),
            ((0, -1, -1), "minecraft:stone"),
            ((0, -2, -1), "minecraft:redstone_torch[lit=true]"),
        ]);
        let mut simple = crate::shapes::Effects::new();
        neighbor_changed(
            &w,
            Some(&t),
            None,
            (0, 0, -1),
            "minecraft:stone",
            &mut simple,
        );
        assert_eq!(
            simple.ticks.len(),
            1,
            "told the torch itself, so the torch's own rule ran"
        );

        let mut multi = crate::shapes::Effects::new();
        neighbor_changed(&w, Some(&t), None, (0, 0, 0), "minecraft:stone", &mut multi);
        assert!(
            multi.ticks.is_empty(),
            "a simple update at the origin tells the ORIGIN, which is air — not its neighbours"
        );

        let mut multi = crate::shapes::Effects::new();
        update_neighbors_at(&w, Some(&t), None, (0, 0, 0), "minecraft:stone", &mut multi);
        assert_eq!(
            multi.ticks.len(),
            1,
            "the six-face form from the same origin does reach the torch"
        );
    }

    /// **Scenario 400's rig, exactly as `ci/checks/gameplay/400-*.toml`
    /// builds it, and this is a diagnostic rather than a rule test.**
    ///
    /// The 2026-08-17 recordings have vanilla emitting a fourth
    /// `block_update` that this server does not emit at all:
    ///
    /// ```text
    ///   vanilla  [2,0,0] minecraft:redstone_wall_torch[facing=east,lit=false]  6894
    ///   rustmc   -
    /// ```
    ///
    /// The check reads `diverge` with an **empty** `missing_kinds`, because
    /// `block_update` is a kind this server does emit — for the lever. So
    /// the corpus row reads "content, not coverage" while the thing actually
    /// missing is a whole block change. This test asks the rules in
    /// isolation whether they know the torch should go out, which separates
    /// a rule defect from a plumbing one.
    ///
    /// Geometry, all relative to the workspace anchor:
    ///
    /// ```text
    ///   [1, 1, 0]  lever, face=floor — the step flips this
    ///   [1, 0, 0]  stone — what the lever strongly powers
    ///   [2, 0, 0]  redstone_wall_torch, facing=east — hangs on the stone
    /// ```
    #[test]
    fn scenario_400s_rig_knows_the_torch_should_go_out() {
        let t = table();
        let torch = "minecraft:redstone_wall_torch[facing=east,lit=true]";
        let lit = World::new(&[
            (
                (1, 1, 0),
                "minecraft:lever[face=floor,facing=north,powered=true]",
            ),
            ((1, 0, 0), "minecraft:stone"),
            ((2, 0, 0), torch),
        ]);
        let p = Redstone::new(&lit, &t, None);

        // The chain, one link at a time, so a failure names its own link.
        assert_eq!(
            p.get_direct_signal((1, 1, 0), Face::Up),
            15,
            "a floor lever's connected direction is UP, so it strongly powers \
             the block below it"
        );
        assert_eq!(
            p.get_direct_signal_to((1, 0, 0)),
            15,
            "getDirectSignalTo asks the block above with UP"
        );
        assert!(
            p.has_signal((1, 0, 0), Face::East),
            "the stone is a conductor, so getSignal maxes in the direct signal"
        );
        assert!(
            torch_has_neighbor_signal(&p, (2, 0, 0), torch),
            "facing=east hangs on the block to the WEST, which is the stone"
        );
        assert!(
            torch_should_schedule(&p, (2, 0, 0), torch),
            "lit == hasNeighborSignal is the arm that schedules"
        );

        // And the tick itself writes the state the recording is missing.
        let mut updates = crate::shapes::Effects::new();
        torch_tick(&lit, &t, None, (2, 0, 0), torch, 0, &mut updates);
        assert_eq!(
            lit.at((2, 0, 0)),
            "minecraft:redstone_wall_torch[facing=east,lit=false]",
            "the block update vanilla emits and this server does not"
        );

        // The control: with the lever off, nothing in the chain fires. A
        // rig that turned the torch off either way would pass everything
        // above and mean nothing.
        let dark = World::new(&[
            (
                (1, 1, 0),
                "minecraft:lever[face=floor,facing=north,powered=false]",
            ),
            ((1, 0, 0), "minecraft:stone"),
            ((2, 0, 0), torch),
        ]);
        let p = Redstone::new(&dark, &t, None);
        assert!(!torch_has_neighbor_signal(&p, (2, 0, 0), torch));
        assert!(!torch_should_schedule(&p, (2, 0, 0), torch));
    }

    /// `getConnectedDirection`, whose name reads backwards.
    #[test]
    fn a_floor_levers_connected_direction_is_up() {
        let f = |s: &str| connected_direction(s);
        assert_eq!(
            f("minecraft:lever[face=floor,facing=south,powered=true]"),
            Some(Face::Up),
            "FLOOR is the switch map's case 2, which is UP"
        );
        assert_eq!(
            f("minecraft:lever[face=ceiling,facing=south,powered=true]"),
            Some(Face::Down)
        );
        // WALL is the default arm and reads FACING, so all four are distinct.
        for (facing, want) in [
            ("north", Face::North),
            ("south", Face::South),
            ("east", Face::East),
            ("west", Face::West),
        ] {
            assert_eq!(
                f(&format!(
                    "minecraft:lever[face=wall,facing={facing},powered=true]"
                )),
                Some(want)
            );
        }
    }

    /// **The second `updateNeighborsAt`, which is the whole of scenario
    /// 400.**
    ///
    /// The rig is the check's own: a floor lever at `[1,1,0]` on a stone at
    /// `[1,0,0]`, with a wall torch on the stone's east face at `[2,0,0]`.
    /// The torch is two positions from the lever and is a neighbour of the
    /// **stone**, never of the lever — so `Level.setBlock`'s
    /// `UPDATE_NEIGHBORS` bit, which updates the lever's own neighbours,
    /// cannot reach it however many times it runs.
    ///
    /// The control below is what makes this test mean anything: updating the
    /// lever's own position schedules nothing, and updating the attached
    /// block's schedules the torch. A port that issued the same call twice
    /// would pass an assertion that only looked at the second.
    #[test]
    fn the_second_neighbour_update_is_what_reaches_scenario_400s_torch() {
        let t = table();
        let torch = "minecraft:redstone_wall_torch[facing=east,lit=true]";
        let lever = "minecraft:lever[face=floor,facing=north,powered=true]";
        let rig = || {
            World::new(&[
                ((1, 1, 0), lever),
                ((1, 0, 0), "minecraft:stone"),
                ((2, 0, 0), torch),
            ])
        };

        // The control: the first call, which is the one we already had.
        let w = rig();
        let mut first = crate::shapes::Effects::new();
        update_neighbors_at(&w, Some(&t), None, (1, 1, 0), "minecraft:lever", &mut first);
        assert!(
            !first.ticks.iter().any(|s| s.pos == (2, 0, 0)),
            "the torch is not a neighbour of the lever, so one call cannot \
             reach it — this is why 400 is red and 360 is green"
        );

        // The second call, at `pos.relative(back)` — the stone.
        let w = rig();
        let mut second = crate::shapes::Effects::new();
        face_attached_update_neighbours(&w, Some(&t), None, (1, 1, 0), lever, &mut second);
        assert!(
            second.ticks.iter().any(|s| s.pos == (2, 0, 0)),
            "updating the attached block's neighbours reaches the torch"
        );
    }

    /// Which blocks take that path. A lever and every button, and nothing
    /// else — the two classes whose `updateNeighbours` bodies are identical.
    /// A class extract holding exactly the rows a test names.
    ///
    /// Written to a file and loaded, rather than constructed, so the test
    /// exercises the same parser production does — the shape
    /// `crate::ticks::tests` already uses for the same reason.
    fn classes_for(rows: &[(&str, &str)]) -> crate::placedby::PlacedByTable {
        let pairs: Vec<String> = rows
            .iter()
            .map(|(block, class)| format!(r#""{block}": "{class}""#))
            .collect();
        let p = std::env::temp_dir().join(format!(
            "rustmc-power-classes-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &p,
            format!(r#"{{"classes": {{{}}}, "table": []}}"#, pairs.join(",")),
        )
        .expect("write");
        let t = crate::placedby::PlacedByTable::load(&p).expect("load");
        let _ = std::fs::remove_file(&p);
        t
    }

    #[test]
    fn only_a_lever_and_a_button_update_their_support_too() {
        let classes = classes_for(&[
            ("minecraft:stone_button", "ButtonBlock"),
            ("minecraft:birch_button", "ButtonBlock"),
            ("minecraft:lever", "LeverBlock"),
            ("minecraft:stone", "Block"),
            ("minecraft:redstone_torch", "RedstoneTorchBlock"),
        ]);
        let yes = |s: &str| attaches_and_updates_twice(Some(&classes), s);
        assert!(yes("minecraft:lever[face=floor,facing=north,powered=true]"));
        assert!(yes(
            "minecraft:stone_button[face=wall,facing=north,powered=true]"
        ));
        assert!(yes(
            "minecraft:birch_button[face=floor,facing=north,powered=false]"
        ));
        assert!(!yes("minecraft:stone"));
        assert!(!yes("minecraft:redstone_torch[lit=true]"));
        // **The suffix trap, both ways round.** A block whose name merely ends
        // in the word is not a `ButtonBlock` and must not be taken in...
        assert!(!yes("minecraft:button_block_lookalike"));
        // ...and a `ButtonBlock` whose name does not end in `_button` must
        // still be served, which no suffix test can do.
        let renamed = classes_for(&[("minecraft:clicky_thing", "ButtonBlock")]);
        assert!(attaches_and_updates_twice(
            Some(&renamed),
            "minecraft:clicky_thing[face=wall,facing=north,powered=true]"
        ));

        // With no class extract only the lever is served, and that is a
        // visible under-answer rather than a guess.
        assert!(attaches_and_updates_twice(
            None,
            "minecraft:lever[face=floor,facing=north,powered=true]"
        ));
        assert!(!attaches_and_updates_twice(
            None,
            "minecraft:stone_button[face=wall,facing=north,powered=true]"
        ));
    }
}

#[cfg(test)]
mod repeater_lock_tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct World(Mutex<BTreeMap<BlockPos, String>>);
    impl World {
        fn at(&self, pos: BlockPos) -> String {
            self.0
                .lock()
                .unwrap()
                .get(&pos)
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
    }
    impl Blocks for World {
        fn state_at(&self, pos: BlockPos) -> String {
            self.at(pos)
        }
        fn set_block(&self, pos: BlockPos, state: &str) -> Option<i32> {
            self.0.lock().unwrap().insert(pos, state.to_string());
            Some(1)
        }
        fn default_state(&self, block: &str) -> Option<String> {
            Some(block.to_string())
        }
        fn has_state(&self, _: &str) -> bool {
            true
        }
        fn state_id(&self, _: &str) -> Option<i32> {
            Some(1)
        }
        fn replaceable(&self, pos: BlockPos, _: &crate::placement::Replacing<'_>) -> bool {
            crate::shapes::is_air(&self.at(pos))
        }
        fn face_sturdy(&self, _: BlockPos, _: Face) -> bool {
            false
        }
        fn redstone_conductor(&self, _: BlockPos) -> bool {
            false
        }
        fn signal_source(&self, pos: BlockPos) -> bool {
            block_of(&self.at(pos)) == "minecraft:repeater"
        }
        fn water_at(&self, _: BlockPos) -> bool {
            false
        }
        fn legacy_fluid_at(&self, _: BlockPos) -> String {
            "minecraft:air".to_string()
        }
    }

    fn table() -> SignalTable {
        let json = r#"{
          "rng": "signal", "states": 1, "quiet": 0, "sources": 1, "refusedStates": 0,
          "directionOrder": ["DOWN", "UP", "NORTH", "SOUTH", "WEST", "EAST"],
          "refused": [], "unanswerable": [],
          "table": [
            {"state": "minecraft:repeater[delay=1,facing=east,locked=false,powered=true]", "id": 3, "source": true,
             "signal": [0,0,0,0,0,15], "direct": [0,0,0,0,0,15]}
          ]
        }"#;
        let dir = std::env::temp_dir().join(format!(
            "rustmc-replock-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("signal.json");
        std::fs::write(&path, json).unwrap();
        let out = SignalTable::load(&path).expect("fixture loads");
        let _ = std::fs::remove_file(&path);
        out
    }

    /// A north-facing repeater with a powered east-facing repeater on its
    /// east side — `FACING` is a diode's INPUT side, so that one takes its
    /// input from further east and outputs west, into this one: a shape
    /// update from the east (across its axis) writes `locked=true`; one
    /// from the north (along its axis) or below answers nothing, which is
    /// the table's business.
    #[test]
    fn a_repeater_locks_on_a_shape_update_across_its_axis() {
        let t = table();
        let a = "minecraft:repeater[delay=1,facing=north,locked=false,powered=false]";
        let w = World(Mutex::new(
            [
                ((1, 0, 0), a.to_string()),
                (
                    (2, 0, 0),
                    "minecraft:repeater[delay=1,facing=east,locked=false,powered=true]".to_string(),
                ),
            ]
            .into_iter()
            .collect(),
        ));
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), a, Face::East).as_deref(),
            Some("minecraft:repeater[delay=1,facing=north,locked=true,powered=false]")
        );
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), a, Face::West).as_deref(),
            Some("minecraft:repeater[delay=1,facing=north,locked=true,powered=false]"),
            "the west side is also across the axis; isLocked reads both sides"
        );
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), a, Face::North),
            None
        );
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), a, Face::Down),
            None
        );
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), a, Face::Up),
            None
        );
        assert_eq!(
            repeater_update_shape(&w, Some(&t), None, (1, 0, 0), "minecraft:stone", Face::East),
            None
        );
    }
}
