//! What a gameplay check *is*: a scripted scenario, in TOML.
//!
//! A scenario says what to do and which observations count. It does **not**
//! say what should happen — that comes from running the same scenario
//! against real vanilla, which is the only source of truth this project
//! accepts. Adding a check is therefore writing down a sequence of actions
//! and nothing else, which is the property that makes a corpus affordable.
//!
//! # Coordinates are anchor-relative
//!
//! Every position is an offset from the **anchor**: the block the player is
//! standing in when the scenario starts. Vanilla and rustmc choose their own
//! spawn points and there is no reason for them to agree, so an absolute
//! coordinate would compare two different places. On the superflat substrate
//! every column is identical, so the same offset is the same situation on
//! both servers by construction.
//!
//! # Setup is not measured
//!
//! `[[setup]]` steps run first and their observations are discarded. That is
//! how a scenario builds a rig — clearing a space, standing a block up to
//! put a torch on — without the rig's own block updates drowning the thing
//! being measured. `[[step]]` is the measured part.

use serde::Deserialize;

/// One check.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    /// Stable identifier. It is what the corpus report counts and what a
    /// regression is named by, so it must not change once published.
    pub name: String,
    /// What the check is for, in a sentence. Reported on failure, because a
    /// diff of two packet lists does not explain what was being asked.
    pub about: String,
    /// Which observation kinds this scenario is *about*. Everything else the
    /// server said is still recorded — see [`crate::trace`] — and is
    /// compared only under the strict verdict.
    pub observe: Vec<String>,
    /// How to treat wall-clock timing when comparing. Default: ignore it.
    #[serde(default)]
    pub timing: Timing,
    /// Which client's stream the comparison reads. Default: both.
    #[serde(default)]
    pub observe_from: From,
    /// How long to keep listening after the last step, for the things that
    /// arrive on their own — a scheduled tick, a mob moving, an item
    /// despawning. Nothing unprompted can be observed without it.
    #[serde(default = "default_settle")]
    pub settle_ms: u64,
    /// How far from the workspace an observation has to be to still be this
    /// scenario's business, in blocks, Chebyshev. Default 16.
    ///
    /// # Why a scenario has a radius at all
    ///
    /// Scenarios share one world and march 32 blocks apart, and until
    /// 2026-08-03 that was enough to isolate them because the harness never
    /// acknowledged a chunk batch and so held **nine chunks** for the whole
    /// session. With chunk delivery working, a client at one workspace can
    /// see several previous ones, and the corpus immediately recorded two
    /// `block_update`s for a `grass_block` turning to `dirt` 21 and 71 blocks
    /// away — positions no scenario ever touched, in a world with
    /// `randomTickSpeed 0`. Unexplained, and not the slab check's business
    /// either way.
    ///
    /// Widening the spacing instead would cost the walk: `goto` hops half a
    /// block at a time, so putting workspaces outside a view distance would
    /// multiply the corpus's running time by four to isolate scenarios that
    /// a bounded projection isolates for free.
    ///
    /// This does not throw anything away. It is a *projection*, applied at
    /// compare time exactly like `observe` and `observe_from`, and the
    /// **strict** denominator still reads every observation recorded — so
    /// whatever this hides is still counted against us in the gap between
    /// the two numbers.
    ///
    /// # `-1` means unbounded, and it exists for packets with no position
    ///
    /// Some clientbound packets carry an entity and **no coordinates at
    /// all** — `animate` is one, and `entity_event`, `hurt_animation` and
    /// `set_entity_motion` are the same shape. The only spatial information
    /// available for them is where the entity was last *seen* to be, which
    /// [`crate::compare`]'s placement table remembers.
    ///
    /// For an item or a mob that memory is true: it was announced at a
    /// position and it stays there. **For one of the two clients it is false
    /// by construction**, because `record::run_one` walks them to the next
    /// workspace *between* scenarios, outside every measured window, so no
    /// packet ever says they moved. A client re-announced by an `add_entity`
    /// inside some earlier scenario's window therefore carries a placement
    /// that is stale by one workspace spacing for every scenario after it.
    ///
    /// That is not hypothetical. `530-walking-away-drops-a-standing-player-
    /// from-view` exists to make a player leave a tracking range and come
    /// back, so it emits exactly that `add_entity` — and measured on the
    /// corpus run of 2026-08-06, it placed the actor at absolute
    /// `(-1984, -60, 73)` and left it there. Twelve scenarios later the
    /// `animate` checks were being bounded against a position **384 blocks**
    /// from their own anchor, so vanilla's projection came out empty and all
    /// three went `vacuous` — gate-red, and a check that could not have
    /// passed however right the server was.
    ///
    /// The bounding of *leftovers* is deliberate and stays — see
    /// `an_entity_placed_by_an_earlier_scenario_is_still_placed`, which pins
    /// that a neighbouring workspace's dropped item is not this scenario's
    /// business. What `-1` says is narrower and is a property of the
    /// **packet**, not of the world: *this check's observations carry no
    /// position, so there is nothing to bound them by.* A check that sets it
    /// is asserting that, and nothing else.
    #[serde(default = "default_within")]
    pub within: i32,
    /// Observation kinds of which **only the first of each distinct value is
    /// compared**, in both denominators.
    ///
    /// # What this removes, and what it deliberately does not
    ///
    /// It removes two dimensions, and they are one dimension wearing two hats:
    /// *how many times* the same observation arrived, and *which step* each
    /// arrival landed in. Everything else a projected observation carries is
    /// still compared — position, payload, `who` — because two observations
    /// only collapse when they are equal in everything but those two. A wrong
    /// `b1`, a lid at the wrong position, a packet on the wrong stream and an
    /// observation that never arrives at all are all exactly as visible as
    /// before.
    ///
    /// # Why the step index goes too, which is the 2026-08-07 correction
    ///
    /// The count and the step attribution are the same coin. A timer with a
    /// fixed period and a free-running phase puts N or N+1 edges inside a
    /// window, *and* puts each edge on one side or the other of every step
    /// boundary inside it. Removing the count while keeping the step index
    /// leaves the second half of the same non-determinism: measured on P2's
    /// `560`, vanilla and this server emit **eight lid events each** and the
    /// entire difference is that vanilla's second edge (260 ms) is stamped
    /// step 1 while ours (220 ms) is stamped step 0. Six of the seven chest
    /// scenarios in that run disagreed on step attribution alone.
    ///
    /// # Why a check would ever want it
    ///
    /// Some mechanisms repeat on a server-side timer for as long as a state
    /// holds. An open chest is one: `ContainerOpenersCounter.recheckOpeners`
    /// re-arms every `CHECK_TICK_DELAY` = 5 ticks and calls
    /// `openerCountChanged` unconditionally, so the same `block_event(1, 1)`
    /// goes out every 250 ms forever. How many of those land inside a check's
    /// window is decided by where the window edge falls against that cadence,
    /// which is a property of the harness's sampling and not of either
    /// server. `crate::compare::verdict_for` is exact sequence equality, so
    /// without this the count *is* the check — and vanilla, sampled twice,
    /// disagrees with itself.
    ///
    /// That makes `match` a verdict no single run can support, and a ratchet
    /// floor that rests on a coin toss goes red at random. The alternative
    /// families were considered and rejected in `docs/WORKLIST.md`; this one
    /// is the only one that removes the randomness rather than tolerating,
    /// widening or re-sampling it.
    ///
    /// # The precedent, which is in the tree already
    ///
    /// `crate::trace::decode` decodes `ClientboundSoundPacket` **purely to
    /// throw its `long seed` away**, because `Level.playSound` fills it from
    /// `soundSeedGenerator.nextLong()` and digesting it made every check
    /// naming `sound` unmatchable for everybody, vanilla included. Same move:
    /// a dimension no check claims to test, removed from the comparison
    /// rather than declared unstable.
    ///
    /// # The cost, named rather than discovered later
    ///
    /// **A check that declares this stops testing the cadence of the kinds it
    /// names.** A server that emitted the lid once and never repeated would
    /// pass. `CHECK_TICK_DELAY` is a real mechanism and that coverage has to
    /// be replaced deliberately, by something that measures the **gap** and
    /// not the count — which is timing, which this corpus excludes from
    /// comparison on purpose, so its honest home is a unit test. See
    /// `rustmc_server::containers`'s
    /// `the_wire_cadence_is_one_signal_every_check_tick_delay_ticks`.
    ///
    /// # Two limits, both of which used to be defects and are now gone
    ///
    /// * **It was adjacency-keyed**, so a run was broken by anything at all
    ///   arriving between two repeats — which meant the same declaration
    ///   collapsed everything in the declared projection, where the
    ///   interleaving kinds have been filtered out, and nothing whatsoever in
    ///   the strict one, where they have not. It is keyed on the value now.
    ///   See [`crate::compare::collapse_repeats`].
    /// * **It did not apply to `strict`.** It does now. That is not a
    ///   widening of `strict`'s definition: `strict` ignores `observe` because
    ///   it refuses to *narrow* to the kinds a check named, and it has always
    ///   honoured the check's other statements about the instrument — `within`
    ///   and `observe_from`. This is one of those. It is also the only
    ///   denominator where the declaration does any work for a container
    ///   check, because none of them observes `block_event`.
    ///
    /// # A name outside `observe` is legal, and is the normal case
    ///
    /// [`Scenario::load`] refuses only a name that no decoder arm can ever
    /// produce, because that one can never bite in either denominator.
    #[serde(default)]
    pub collapse_repeats: Vec<String>,
    /// What the **measured step** — the last `[[step]]` — is expected to make
    /// the reference do. Default: speak.
    ///
    /// # Why this cannot be derived, which is the whole argument for a field
    ///
    /// `608-container-click-with-an-out-of-range-slot-changes-nothing` and
    /// `607-container-click-pickup-all-gathers-matching-stacks` end with the
    /// **same action on the same rig**: a `container_click` on a chest menu.
    /// One is asserting that nothing happens and the other is asserting that
    /// eight stacks are swept onto the cursor. Nothing in the actions, the
    /// rig, the `observe` list or the recording distinguishes them, because
    /// the difference is not in what the scenario *does* — it is in what the
    /// scenario *claims*, and that is information only the author has. A
    /// harness that guessed would be inventing the check's subject, which is
    /// the one thing this corpus never does: a scenario says what to do and
    /// vanilla says what should happen.
    ///
    /// # The polarity is the safety argument
    ///
    /// **Absent means "must speak".** Every scenario written before this
    /// field existed is therefore opted in to the strict reading, and a new
    /// check that forgets it goes red, never green: omission cannot
    /// manufacture a pass. That is the opposite of the hazard
    /// [`crate::compare::PROJECTION_SOURCES`] names about hand-written
    /// manifests, where the branch that adds the tenth axis is exactly the
    /// branch that forgets to declare it and the omission certifies a lie.
    /// Here the omission refuses.
    ///
    /// So this is not a list of names with a schema around it. A list of
    /// names exempts by enumeration and is silent about everything not on it;
    /// this makes a per-scenario claim, in the scenario, that the reader of
    /// that scenario can check against its own `about` line, and that costs a
    /// deliberate line of TOML to make.
    ///
    /// # What it does not do
    ///
    /// It does not exempt the step from comparison. `silent` scenarios still
    /// compare their measured step on both sides: a subject that speaks where
    /// the reference was silent is a `Diverge` exactly as before, which is
    /// what keeps `561`, `571`, `581` and `608` able to fail. All it
    /// suppresses is [`crate::compare::Verdict::Mute`], whose claim — "this
    /// check did not measure anything at the step it is named for" — is
    /// false for a check whose point is the silence.
    #[serde(default)]
    pub measured_step: MeasuredStep,
    /// Game rules to turn on for **this scenario's measured window only**.
    ///
    /// See [`GameRule`]. Empty for every check written before 2026-08-12 and
    /// for every check that does not need one, which is the safe default: a
    /// scenario that says nothing here runs on exactly the substrate
    /// `ci/t2-gameplay.sh` booted.
    #[serde(default)]
    pub gamerule: Vec<GameRule>,
    /// Console lines that put back whatever a [`Step::Console`] changed, sent
    /// **after** the measured window and whether or not it succeeded.
    ///
    /// # This exists because its absence cost ninety checks
    ///
    /// Measured 2026-09-05, and `record`'s own comment on the game-rule
    /// restore had predicted the exact shape: *"it would arrive as ninety-two
    /// checks going red for a reason none of them names."* Ninety did.
    /// `console-worldborder-set-reaches-a-client` set the world border to 137
    /// blocks and nothing put it back; every rig in this corpus sits 32 blocks
    /// further out in -x than the last, so **scenario 18 onwards was outside
    /// the border**. Vanilla answered every placement with `build.tooHigh` and
    /// every container right-click with silence, and the report read as ninety
    /// regressions in a branch that had touched none of them.
    ///
    /// A console step is global server state by construction — that is the
    /// whole reason it exists, since the only commands it can reach are the
    /// permission-2 ones — so this is required rather than optional:
    /// [`Scenario::load`] refuses a scenario carrying a console step and no
    /// restore.
    ///
    /// **The values are declared, not derived**, for the reason
    /// [`GameRule::restore`] gives: deriving them would be a second copy of
    /// the server's defaults living somewhere else, which is this project's
    /// definition of a caveat list. Writing them in the scenario puts them
    /// next to the line they undo.
    #[serde(default)]
    pub console_restore: Vec<String>,
    /// Rig building. Observations here are discarded.
    #[serde(default)]
    pub setup: Vec<Step>,
    /// The measured part.
    pub step: Vec<Step>,
}

/// One game rule, set for the duration of one scenario's measured window.
///
/// # Why this is a scenario field and not a harness setting
///
/// `ci/t2-gameplay.sh` sets four game rules on the reference before it
/// records anything — `advance_time false`, `spawn_mobs false`,
/// `random_tick_speed 0`, `advance_weather false` — and every check in the
/// corpus was measured under them. Three of those four shut **eight of
/// vanilla's 120 per-tick profiler sections** outright
/// (`docs/PARITY-ROADMAP.md` §21.2), so the corpus has no evidence about mob
/// spawning, random ticks or weather for a reason that is ours rather than
/// vanilla's.
///
/// Turning any of them on globally is not the fix. They were set to make the
/// corpus reproducible: random ticks fire off a per-world generator seeded
/// afresh on every boot, and the first thing they do to a superflat rig is
/// turn the grass under it into dirt at an unpredictable moment. That is not
/// a hypothesis — it is what happened for the whole month before 2026-08-03,
/// when the *old* camelCase rule ids were being silently refused and random
/// ticks were running through every measurement.
///
/// So the rule is set **per scenario**, applied after that scenario's rig is
/// built and quiet, and put back before the next scenario starts. Every check
/// that does not ask for one keeps the substrate it was measured under, which
/// is the property that lets a new scenario reach a dark system without
/// re-baselining the other ninety-two.
///
/// # It reaches the reference through the console, and only the reference
///
/// The four global rules already go to `$VAN_DIR` alone: `gamerule` is a
/// vanilla console command and this server implements no game rules at all,
/// so the harness has always been quieting one of the two servers and
/// relying on the other having nothing to quiet. This is the same asymmetry,
/// declared instead of assumed — `record --console <fifo>` is passed for the
/// vanilla pass and not for ours, and a run that has scenarios declaring a
/// rule and no console says so on stderr, once per scenario, rather than
/// applying nothing in silence.
///
/// **What that costs is stated rather than discovered later**: a check
/// declaring a game rule compares a reference running the mechanism against a
/// subject that is not, so its honest verdict is `absent` until this server
/// grows both the mechanism and a way to be told. That is what an `absent`
/// baseline entry is for. What it buys is that vanilla's section *runs*,
/// which is the precondition every other row of §21.5's work order lacks.
///
/// # `restore` is required, and is not defaulted to vanilla's default
///
/// The value to go back to is the value **the harness set**, which is not
/// vanilla's default for any of the four — `random_tick_speed` is 0 here and
/// 3 in vanilla. Deriving it would be a second copy of `ci/t2-gameplay.sh`'s
/// table living somewhere else, which is this project's definition of a
/// caveat list. Writing it in the scenario puts it next to the value it
/// undoes, where the person reading the check can see both.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameRule {
    /// The 26.2 registry id, without the namespace: `random_tick_speed`,
    /// `advance_weather`, `spawn_mobs`. **Not the pre-26.2 camelCase name** —
    /// vanilla answers `Incorrect argument for command` to those and carries
    /// on, which is how this project lost a month of measurements.
    pub rule: String,
    /// What to set it to for the measured window.
    pub value: String,
    /// What to set it back to afterwards — the value `ci/t2-gameplay.sh` set.
    pub restore: String,
}

impl GameRule {
    /// The console command that applies it.
    #[must_use]
    pub fn apply_command(&self) -> String {
        format!("gamerule {} {}", self.rule, self.value)
    }

    /// The console command that undoes it.
    #[must_use]
    pub fn restore_command(&self) -> String {
        format!("gamerule {} {}", self.rule, self.restore)
    }

    /// Nothing here may carry a newline, a space or a shell metacharacter.
    ///
    /// The strings are written into a server's console fifo verbatim and one
    /// line is one command, so a newline in a value is a second command
    /// nobody declared. Refused at load rather than escaped, because a
    /// scenario has no legitimate reason to hold one.
    fn well_formed(&self) -> bool {
        let ok = |s: &str, allow_minus: bool| {
            !s.is_empty()
                && s.len() <= 64
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || (allow_minus && c == '-'))
        };
        ok(&self.rule, false) && ok(&self.value, true) && ok(&self.restore, true)
    }
}

/// What the last `[[step]]` is expected to provoke from the reference.
///
/// See [`Scenario::measured_step`] for why this is declared rather than
/// derived, and [`crate::compare::Verdict::Mute`] for what reads it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasuredStep {
    /// The measured step is expected to produce at least one observation of a
    /// kind the scenario declared. The default, so that a scenario which says
    /// nothing about this is held to it.
    #[default]
    Speaks,
    /// The measured step is expected to produce **nothing**, and that silence
    /// is the assertion. A negative check: "clicking an empty slot changes
    /// nothing", "a second swing inside the animation is swallowed".
    Silent,
}

const fn default_settle() -> u64 {
    750
}

/// Chebyshev blocks from the workspace. Every rig in the corpus is built
/// within three blocks of the player, so 16 is roomy enough that a cascade
/// or a falling entity stays inside it and tight enough to exclude the
/// neighbouring workspace 32 blocks away.
const fn default_within() -> i32 {
    16
}

/// What to do about the wall clock.
///
/// Ignored by default, and that default is a statement rather than
/// convenience: the interval between two packets from two different servers
/// on two different code paths is not a behavioural property, and treating
/// it as one produces failures that mean nothing. A scenario whose *point*
/// is timing — a scheduled tick firing in vanilla's own window — asks for
/// [`Timing::Bucket`] and says what window it means.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timing {
    #[default]
    Ignore,
    /// Compare arrival times **rounded to the nearest** multiple of this many
    /// milliseconds. `at_ms` is measured from the start of the scenario.
    ///
    /// # Rounded, not floored, and that is the whole design
    ///
    /// This said "rounded" and the code floored, which is not a synonym here.
    /// The arrivals a timed check is about cluster at integer multiples of a
    /// tick-derived delay — a button un-presses at 20 or 30 ticks, 1000 ms or
    /// 1500 ms — and any bucket that divides that difference then puts a
    /// boundary **on top of a cluster**. Measured on the Threadripper
    /// 2026-08-03 across four samples, the stone button's un-press landed at
    /// 956, 972, 986 and 1002 ms: a floor bucket of 250, 500 or 1000 ms splits
    /// those four samples across two buckets and the check flaps for reasons
    /// that have nothing to do with the server.
    ///
    /// Rounding to the nearest multiple moves the boundary to the half-bucket,
    /// which is the furthest point from the cluster it can be. The same four
    /// samples then all land on 1000 with 250 ms of margin, against a measured
    /// spread of 46 ms.
    ///
    /// So the rule for choosing `ms` is: **make it the difference the check
    /// must detect.** Then each behaviour rounds to its own multiple and the
    /// margin either side is `ms / 2`. Choosing it smaller does not make the
    /// check sharper, it makes it flakier.
    Bucket { ms: u64 },
}

/// Which client's observations a check is about.
///
/// # Why this is not merely a filter
///
/// The observer channel spent a day looking untrustworthy: with no change to
/// either server between runs, vanilla sent a nearby non-acting client a
/// `block_update` for a dig in one run and nothing at all in the next. **That
/// is understood and fixed, and it was never about who was watching.**
///
/// The harness never sent `chunk_batch_received`, so vanilla's
/// `PlayerChunkSender` — constructed with `maxUnacknowledgedBatches = 1` —
/// delivered nine chunks and stopped. `ChunkMap.isChunkTracked` is
/// `getChunkTrackingView().contains(x, z) && !chunkSender.isPending(...)`,
/// and it is the *only* filter on `ChunkHolder.broadcastChanges`'s recipient
/// list, so a chunk still queued for delivery is a chunk whose block changes
/// are withheld from that client. Both clients were affected, the acting one
/// included. See `ci/chunk-batch-probe.sh`: 9 chunks without the
/// acknowledgement, 117 with it.
///
/// So the split was never actor-against-observer. It was *direct sends*
/// against *broadcasts*, and the broadcasts were being withheld from
/// everybody. A check may now read either stream on its merits; what
/// [`From::Actor`] still buys is a smaller projection, not a safer one.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum From {
    #[default]
    Both,
    /// Only the client that performed the steps.
    Actor,
    /// Only the client that did nothing.
    Observer,
    /// Only the expendable third connection — see [`crate::trace::Who::Probe`]
    /// and [`Step::ProbeJoin`].
    ///
    /// **`Both` does not include the probe**, and that is deliberate rather
    /// than an omission. A probe joins and leaves inside the measured window,
    /// so its login sequence and its removal are visible to the actor and the
    /// observer as well; a check that read every stream would be comparing a
    /// join it did not ask about alongside the one packet it did. A probe
    /// check states `observe_from = "probe"` and reads the socket the
    /// mechanism is on.
    Probe,
}

/// One entry of the `DataComponentPatch` a [`Step::Hold`] puts on its stack.
///
/// # Only one stream codec is written, and that is deliberate
///
/// A component's payload carries **no length**: `DataComponentPatch$3.encode`
/// writes the type id and then hands the buffer to that type's own
/// `networkSynchronized` stream codec. So a harness that wants to write a
/// component has to reimplement that codec, and 111 of them exist.
///
/// This writes exactly one — `ByteBufCodecs.VAR_INT`, verified out of
/// `DataComponents.lambda$static$3` on the pinned jar:
///
/// ```text
///    1: getstatic  ExtraCodecs.NON_NEGATIVE_INT
///    4: DataComponentType$Builder.persistent
///    7: DataComponentType$Builder.ignoreSwapAnimation
///   10: getstatic  ByteBufCodecs.VAR_INT
///   13: DataComponentType$Builder.networkSynchronized
/// ```
///
/// and refuses every other type by name. **The refusal is the point.** A
/// scenario naming `minecraft:custom_name` with an integer payload would put
/// a malformed `Component` on the wire, vanilla's decoder would throw inside
/// the packet read, and the client would be disconnected mid-scenario — which
/// arrives in the recording as a reference that stopped answering, i.e. as a
/// server defect. A named refusal at parse time cannot be misread that way.
///
/// Adding the second codec is the job of the first scenario that needs it,
/// and it is the encoding-side twin of the null `trace-itemstack-legible`
/// recorded against speculative *decoders*: a codec nothing exercises is
/// untested wire format that only ever fires when something else is already
/// going wrong.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldComponent {
    /// The `minecraft:data_component_type` registry name, e.g.
    /// `minecraft:damage`. Resolved against `Tables::component_types`, which
    /// `ci/dump-items.sh` reads out of vanilla's own registry report.
    #[serde(rename = "type")]
    pub kind: String,
    /// The payload of a `minecraft:damage`, written with `ByteBufCodecs.VAR_INT`.
    #[serde(default)]
    pub varint: Option<i32>,
    /// The block-entity type of a `minecraft:block_entity_data`, e.g.
    /// `minecraft:chest` — `TypedEntityData.type()`, written through
    /// `ByteBufCodecs.registry(BLOCK_ENTITY_TYPE)`.
    #[serde(default)]
    pub block_entity: Option<String>,
    /// String-valued keys of a `minecraft:block_entity_data` tag
    /// (`StringTag`), e.g. `LootTable`.
    #[serde(default)]
    pub strings: std::collections::BTreeMap<String, String>,
    /// Long-valued keys of a `minecraft:block_entity_data` tag (`LongTag`),
    /// e.g. `LootTableSeed` — which vanilla reads with `getLongOr`, so an int
    /// there would read as 0 and must not be what an integer in TOML becomes.
    #[serde(default)]
    pub longs: std::collections::BTreeMap<String, i64>,
    /// The already-encoded value of a `minecraft:stored_enchantments` or
    /// `minecraft:enchantments` — `ItemEnchantments.STREAM_CODEC`, which is
    /// `ByteBufCodecs.map(…, Enchantment.STREAM_CODEC, VAR_INT)`: a count,
    /// then `(enchantment registry id, level)` varints. The ids are the
    /// synchronised registry's order in the capture both servers send, which
    /// this harness does not load; a scenario names them in a comment.
    #[serde(default)]
    pub bytes: Option<Vec<u8>>,
    /// [`ENTITY_DATA_COMPONENT`] only: the entity type the data is for,
    /// e.g. `minecraft:pig`. `EntityType.updateCustomEntityTag` (javap 17-27)
    /// applies nothing unless it is the type the egg spawns.
    #[serde(default)]
    pub entity: Option<String>,
    /// [`ENTITY_DATA_COMPONENT`] only: the `Silent` byte of the compound.
    #[serde(default)]
    pub silent: Option<bool>,
    /// [`ENTITY_DATA_COMPONENT`] only: a `LeftHanded` byte, written after
    /// `Silent` when present and omitted when not.
    ///
    /// `Mob.finalizeSpawn` (javap 63-83) sets the mob's handedness from
    /// `level.getRandom().nextFloat() < 0.05`, and `ServerLevel`'s random is
    /// unseeded, so one egg-spawned mob in twenty pairs with `DATA_MOB_FLAGS_ID`
    /// bit 2 set and its first `set_entity_data` reads differently from every
    /// other run's. The egg's tag is applied by `EntityType.create` (javap
    /// 179, `PostSpawnProcessor.apply`) AFTER `finalizeSpawn` (javap 168), so
    /// `LeftHanded:0b` overrides the roll without removing the draw: the
    /// level random's sequence is the same either way, and a check that
    /// observes a spawned mob's data sees one value on every run.
    #[serde(default)]
    pub left_handed: Option<bool>,
    /// `minecraft:custom_name` only: a plain literal. `ComponentSerialization
    /// .CODEC` encodes a component with no style and no siblings as its bare
    /// text, so the value is one network NBT string tag (type 8, a u16
    /// length, the bytes). ASCII only: the length is modified UTF-8's.
    #[serde(default)]
    pub text: Option<String>,
    /// `minecraft:container` only: the slots, in order, each `{ item, count }`
    /// or `{}` for an empty one. Written as `ItemContainerContents
    /// .STREAM_CODEC`: a count, then per slot a presence byte and an
    /// `ItemStackTemplate` — item id, then count, then the empty trusted
    /// patch.
    #[serde(default)]
    pub container: Option<Vec<ContainerSlot>>,
    /// `minecraft:block_state` only: the `BlockItemStateProperties` map,
    /// written as `ByteBufCodecs.map(HashMap::new, STRING_UTF8,
    /// STRING_UTF8)` — a count, then each key and value as a varint length
    /// and UTF-8.
    #[serde(default)]
    pub properties: Option<std::collections::BTreeMap<String, String>>,
    /// `minecraft:pot_decorations` only: up to four item names, back, left,
    /// right, front — `ByteBufCodecs.registry(ITEM)` under `list(4)`.
    #[serde(default)]
    pub sherds: Option<Vec<String>>,
}

/// One slot of a held `minecraft:container`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ContainerSlot {
    pub item: Option<String>,
    #[serde(default = "one")]
    pub count: i32,
}

const fn one() -> i32 {
    1
}

impl HeldComponent {
    /// Whether this entry is one this harness has a stream codec for, with
    /// exactly the fields that codec reads. `None` if so, otherwise why not.
    #[must_use]
    pub fn refusal(&self) -> Option<String> {
        let tag_fields = !self.strings.is_empty() || !self.longs.is_empty();
        let raw = self.bytes.is_some();
        let egg = self.entity.is_some() || self.silent.is_some() || self.left_handed.is_some();
        if self.text.is_some() != (self.kind == "minecraft:custom_name") {
            return Some(format!(
                "{}: `text` is minecraft:custom_name's field and that component's only one",
                self.kind
            ));
        }
        if self.sherds.is_some() != (self.kind == "minecraft:pot_decorations") {
            return Some(format!(
                "{}: `sherds` is minecraft:pot_decorations' field and that component's only one",
                self.kind
            ));
        }
        if self.properties.is_some() != (self.kind == "minecraft:block_state") {
            return Some(format!(
                "{}: `properties` is minecraft:block_state's field and that component's only one",
                self.kind
            ));
        }
        if self.container.is_some() != (self.kind == "minecraft:container") {
            return Some(format!(
                "{}: `container` is minecraft:container's field and that component's only one",
                self.kind
            ));
        }
        if self
            .text
            .as_deref()
            .is_some_and(|t| !t.is_ascii() || t.len() > 255)
        {
            return Some("minecraft:custom_name: `text` must be ASCII and short".into());
        }
        if egg && self.kind != ENTITY_DATA_COMPONENT {
            return Some(format!(
                "{} takes no `entity`/`silent`/`left_handed`; those are {ENTITY_DATA_COMPONENT}'s",
                self.kind
            ));
        }
        match self.kind.as_str() {
            "minecraft:damage" | "minecraft:max_stack_size"
                if self.varint.is_some() && self.block_entity.is_none() && !tag_fields && !raw =>
            {
                None
            }
            "minecraft:block_entity_data"
                if self.block_entity.is_some() && self.varint.is_none() && !raw =>
            {
                None
            }
            "minecraft:stored_enchantments" | "minecraft:enchantments"
                if raw && self.varint.is_none() && self.block_entity.is_none() && !tag_fields =>
            {
                None
            }
            "minecraft:pot_decorations"
                if self.varint.is_none()
                    && self.block_entity.is_none()
                    && !tag_fields
                    && !raw
                    && !egg
                    && self.sherds.as_ref().is_some_and(|v| v.len() <= 4) =>
            {
                None
            }
            "minecraft:block_state"
                if self.varint.is_none()
                    && self.block_entity.is_none()
                    && !tag_fields
                    && !raw
                    && !egg =>
            {
                None
            }
            "minecraft:container"
                if self.varint.is_none()
                    && self.block_entity.is_none()
                    && !tag_fields
                    && !raw
                    && !egg
                    && self.container.as_ref().is_some_and(|c| {
                        c.len() <= 256 && c.iter().all(|s| s.item.is_none() || s.count > 0)
                    }) =>
            {
                None
            }
            "minecraft:custom_name"
                if self.varint.is_none()
                    && self.block_entity.is_none()
                    && !tag_fields
                    && !raw
                    && !egg =>
            {
                None
            }
            ENTITY_DATA_COMPONENT
                if self.entity.is_some()
                    && self.silent.is_some()
                    && self.varint.is_none()
                    && self.block_entity.is_none()
                    && !tag_fields
                    && !raw =>
            {
                None
            }
            k if ENCODABLE_COMPONENTS.contains(&k) => Some(format!(
                "{k} is encodable but these fields are not its codec's: \
                 `minecraft:damage` takes `varint` only, `minecraft:block_entity_data` \
                 takes `block_entity` plus `strings`/`longs`, the two enchantment \
                 components take `bytes` only, `minecraft:entity_data` takes `entity` \
                 and `silent`, and optionally `left_handed`"
            )),
            k => Some(format!("{k} has no stream codec in this harness")),
        }
    }
}

/// The data component types this harness can encode, by registry name.
///
/// See [`HeldComponent`] for why the list is short rather than generated, and
/// why naming anything outside it is a parse-time refusal.
///
/// **`minecraft:block_entity_data` is the second**, added for the loot lane:
/// `DataComponents.lambda$static$60` on the pinned jar is
/// `persistent(TypedEntityData.codec(BLOCK_ENTITY_TYPE.byNameCodec()))
/// .networkSynchronized(TypedEntityData.streamCodec(ByteBufCodecs.registry(
/// Registries.BLOCK_ENTITY_TYPE)))`, and `TypedEntityData.streamCodec` is
/// `StreamCodec.composite(<the registry codec>, type, ByteBufCodecs
/// .COMPOUND_TAG, tag)` — a varint, then nameless network NBT. It is the only
/// route a client has to a chest that carries a `LootTable`: `container_loot`
/// is `persistent` alone (`lambda$static$80`) and has no stream codec at all.
///
/// **The two enchantment components are the third and fourth**, added for
/// the item-components lane, as pre-encoded `bytes`: see
/// [`HeldComponent::bytes`].
pub const ENCODABLE_COMPONENTS: &[&str] = &[
    "minecraft:damage",
    "minecraft:block_entity_data",
    "minecraft:stored_enchantments",
    "minecraft:enchantments",
    ENTITY_DATA_COMPONENT,
    "minecraft:custom_name",
    "minecraft:container",
    "minecraft:max_stack_size",
    "minecraft:block_state",
    "minecraft:pot_decorations",
];

/// `minecraft:entity_data`, the fifth codec this harness writes, and only
/// the `Silent` key of it. `DataComponents.lambda$static$58` (javap 0-19)
/// registers it with `TypedEntityData.streamCodec(EntityType.STREAM_CODEC)`,
/// which is `StreamCodec.composite(EntityType.STREAM_CODEC, type,
/// ByteBufCodecs.COMPOUND_TAG, tag)`: the entity type's registry id as a
/// VarInt (`ByteBufCodecs.registry(ENTITY_TYPE)`), then a network compound.
/// Added for `084`, whose pig must not roll `Mob.baseTick`'s ambient sound
/// inside a check about its head.
pub const ENTITY_DATA_COMPONENT: &str = "minecraft:entity_data";

/// One action. `do` picks the variant.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// Put an item in a hotbar slot and select it.
    ///
    /// The slot travels as an `InventoryMenu` index — hotbar 0 is menu slot
    /// 36 (`InventoryMenu.USE_ROW_SLOT_START`) — which is the trap this
    /// action exists to keep on one code path.
    Hold {
        item: String,
        #[serde(default)]
        slot: i32,
        /// Data components to put on the stack, as a `DataComponentPatch`
        /// with these added and none removed. Empty by default, which is
        /// byte-for-byte the `0, 0` this action wrote before 2026-08-13.
        ///
        /// **This is the corpus's only route to a component-bearing stack,
        /// and its absence is why there was none.** `set_creative_mode_slot`
        /// is the only packet that puts an item into a harness player's
        /// inventory — `Step::Command` cannot run `/give`, which is
        /// permission level 2, and the harness's players are not operators
        /// (`ci/t2-gameplay.sh`) — so a literal `0 added` in this arm made
        /// every stack in every recording componentless by construction.
        /// `docs/PARITY-ROADMAP.md` §44.5.
        #[serde(default)]
        components: Vec<HeldComponent>,
    },
    /// Face a direction. The server has no other way to know: `use_item_on`
    /// says where the cursor was and never which way the placer looked.
    Look { yaw: f32, pitch: f32 },
    /// Move to an anchor-relative position.
    ///
    /// `on_ground` is the flag byte of every `MOVE_PLAYER_POS` the walk
    /// sends, and it defaults to **true** so that every check written before
    /// it existed keeps sending exactly what it sent.
    ///
    /// # Why one bit and not a physics model
    ///
    /// A client that jumps sends the arc: 0.42 up, gravity 0.08, drag 0.98,
    /// one packet a tick, with the flag clear for the airborne part. It looks
    /// as though a harness must reproduce that, because `handleMovePlayer`
    /// runs the claimed delta through `player.move` and rejects a
    /// disagreement over 0.0625. **It does not, in this substrate.** That
    /// comparison is skipped outright for a creative player:
    ///
    /// ```text
    ///   handleMovePlayer, offsets 785..837
    ///     787: ldc2_w 0.0625d / 791: ifle 855
    ///     798: isSleeping  : ifne 855
    ///     808: isCreative  : ifne 855      <-- the corpus substrate
    ///     818: isSpectator : ifne 855
    ///     837: LOGGER "{} moved wrongly!"
    /// ```
    ///
    /// and the corpus is creative by construction. So the flag can be sent
    /// clear on an otherwise ordinary walk and vanilla accepts the position.
    ///
    /// What that reaches is `ServerEntity.sendChanges`, whose forced-sync
    /// disjunction ends `|| wasOnGround != entity.onGround()` (offsets
    /// 680..691) — an arm no scenario could exercise while `hop_to` wrote a
    /// literal 1 into every movement packet.
    Walk {
        to: [f64; 3],
        #[serde(default = "on_ground")]
        on_ground: bool,
    },
    /// Claim a position in **one** `MOVE_PLAYER_POS`, however far it is.
    ///
    /// # Why the corpus needed a verb that can be refused
    ///
    /// [`Step::Walk`]'s `hop_to` is capped at four blocks a packet, and the
    /// cap is there precisely so that a walk is never rubber-banded — its
    /// own header explains that a sixteen-block hop made "the whole of a walk
    /// past about 16 blocks fiction". The consequence is that **no scenario
    /// in this corpus could ever make either server refuse a move**, so the
    /// entire refusal half of `handleMovePlayer` — the speed check, the
    /// correction teleport, `awaitingPositionFromClient`, the teleport
    /// counter — had no instrument pointed at it.
    ///
    /// This verb is that instrument, and it is the *speed* check it reaches
    /// rather than the geometry one. That distinction is load-bearing:
    ///
    /// ```text
    ///   handleMovePlayer, offsets 451..489
    ///     451: shouldCheckPlayerMovement(isFallFlying)
    ///     460: f2 = isFallFlying ? 300.0f : 100.0f
    ///     476: if (distanceSq - deltaMovementLengthSq > (double)(f2 * (float)i))
    ///     541:     LOGGER.warn("{} moved too quickly! {},{},{}")
    ///     577:     teleport(getX(), getY(), getZ(), f, f1)
    /// ```
    ///
    /// `shouldCheckPlayerMovement` is `isSingleplayerOwner`,
    /// `isChangingDimension` and two game rules — **`isCreative` is not among
    /// them**. The "moved wrongly" check at offsets 804-821 *is* skipped for
    /// a creative player, and this corpus is creative by construction (see
    /// [`Step::Walk`]'s own note), so the geometry branch is unreachable here
    /// and the speed branch is not. A scenario written against the wrong one
    /// records two servers agreeing that nothing happened.
    ///
    /// # It reads the socket, unlike a walk
    ///
    /// A refused move is answered with `player_position`, and the server then
    /// swallows every later move until the client acknowledges it
    /// (`updateAwaitingTeleport`). `hop_to` deliberately never reads, so a
    /// scenario built out of walks would hang the connection at the first
    /// refusal — which is exactly what `hop_to`'s header records happening.
    /// This verb pumps afterwards so the acknowledgement goes back.
    Lurch {
        to: [f64; 3],
        #[serde(default = "on_ground")]
        on_ground: bool,
    },
    /// `START_DESTROY_BLOCK`, which is what a creative client sends and what
    /// destroys the block for one.
    Dig {
        at: [i32; 3],
        #[serde(default = "face_up")]
        face: String,
    },
    /// `STOP_DESTROY_BLOCK` — the packet a *survival* client sends when its
    /// own mining progress reaches one. Acting on both makes survival mining
    /// instant; acting on neither looks exactly like a broken world.
    FinishDig {
        at: [i32; 3],
        #[serde(default = "face_up")]
        face: String,
    },
    /// `DROP_ITEM`, or `DROP_ALL_ITEMS` with `all = true` — what pressing Q
    /// sends. The first inventory *action* in the corpus: everything before
    /// it could only watch the inventory change as a side effect of a block.
    Drop {
        #[serde(default)]
        all: bool,
    },
    /// `SWAP_ITEM_WITH_OFFHAND`. Worth its own action because the result is
    /// visible on two channels at once — `container_set_slot` down the
    /// acting connection and `set_equipment` broadcast to everyone tracking
    /// the player.
    SwapOffhand,
    /// Right-click a block face with whatever is held.
    Place {
        on: [i32; 3],
        #[serde(default = "face_up")]
        face: String,
        /// Where on the face, 0..1. Defaults to the middle of it; a slab or
        /// a stair reads the y component against 0.5 and nothing else does.
        #[serde(default)]
        cursor: Option<[f32; 3]>,
    },
    /// Right-click holding nothing in particular — eating, drawing a bow,
    /// throwing.
    UseItem,
    /// Let go of the key `use_item` pressed: `RELEASE_USE_ITEM`, action 5 of
    /// `ServerboundPlayerActionPacket`.
    ///
    /// # There was no third verb missing, there was half of the second one
    ///
    /// [`Self::UseItem`] is the press and it was the whole vocabulary. A bow,
    /// a crossbow, a trident, a spyglass, food and a shield are all
    /// hold-and-release items: `Item.use` calls `startUsingItem`, and nothing
    /// happens until the key comes back up. `player_action::RELEASE_USE_ITEM`
    /// has been in this file's ordinal table since the table was written and
    /// **the only line that referenced it was the test asserting its
    /// ordinal** — no arm of `Client::act` ever sent it. So the corpus could
    /// draw a bow and never loose it, and every recording of one was the
    /// drawing.
    ///
    /// # What it reaches, and the two doors that send nothing
    ///
    /// `handlePlayerAction`'s `tableswitch` at offset 45 sends this case to
    /// 260, which is `player.releaseUsingItem()` and a `return` — no
    /// spectator test, no block position read, so this is a
    /// [`Client::bare_player_action`], not a dig.
    ///
    /// `LivingEntity.releaseUsingItem` then has two guards, and both of them
    /// are a silence rather than an error:
    ///
    /// ```text
    ///    9: useItem
    ///   13: ItemStack.isEmpty
    ///   16: ifne 65                <-- nothing was being used: nothing happens
    ///   19: getItemInHand(getUsedItemHand())
    ///   24: ItemStack.isSameItem(inHand, useItem)
    ///   27: ifeq 65                <-- the hand changed: nothing happens
    ///   48: useItem.releaseUsing(level, this, getUseItemRemainingTicks())
    /// ```
    ///
    /// Both are decidable from the scenario file, so both are refused in
    /// `Scenario::load` rather than left to be discovered as a check that
    /// passed. A release with no `use_item` before it, or with the used hand
    /// changed in between, records silence from vanilla and silence from this
    /// server and reads as agreement.
    ///
    /// # `use_item` is the only thing that can start a use
    ///
    /// Checked rather than assumed, because the refusal above depends on it:
    /// `ServerPlayerGameMode.useItemOn` calls `BlockState.useItemOn` (offset
    /// 145) and `ItemStack.useOn` (275, 292) and **never `ItemStack.use`**,
    /// which is the only route to `startUsingItem`. `ItemStack.use` is called
    /// from `ServerPlayerGameMode.useItem` at offset 46, and the only packet
    /// that reaches it is `USE_ITEM` — [`Self::Place`] cannot begin a use.
    ///
    /// # How long the key was held is the `wait` between the two steps
    ///
    /// The packet carries no duration. `releaseUsing` is handed
    /// `getUseItemRemainingTicks()`, which counts *down* from the item's own
    /// `getUseDuration` one per tick, so what an item does on release is a
    /// function of the wall-clock gap between the two steps. A scenario that
    /// means a particular draw strength puts a [`Self::Wait`] between them and
    /// says so; this verb takes no `ms` of its own, because a duration
    /// attached to the release would read as the hold and be neither.
    ReleaseUse,
    /// Left-click an **entity**: `ServerboundAttackPacket`, packet 1.
    ///
    /// # The first verb in this enum that names something other than a block
    ///
    /// Every action above this line acts on the world or on the player's own
    /// inventory. Hitting something is, by the corpus's own ranking, the first
    /// thing a player tries that none of them could express — 33 mechanics had
    /// zero checks and the top of that list was not an unwritten check but an
    /// unwritten verb. Combat, and everything downstream of it (`damage_event`,
    /// `set_health`, `entity_event`, `animate`, death, respawn), was not
    /// untested; it was *untestable*.
    ///
    /// # `target` is a type, and resolution is deliberately strict
    ///
    /// The packet carries a raw entity id, which is a per-run number no
    /// scenario can know — vanilla and this server assign different ones for
    /// the same situation, which is the whole reason `trace::ENTITY_REF`
    /// exists. So a scenario names an entity **type** and the harness resolves
    /// it against what this client has actually been told exists, taking the
    /// most recently added live entity of that type.
    ///
    /// **A target that does not resolve is an error, never a packet.** This is
    /// the eighteenth place in this project where the tempting version of a
    /// check cannot fail: sending id 0 for an unresolved target is a legal
    /// packet that hits nothing, both servers would say nothing back, and the
    /// comparison would report `match` on a scenario that never attacked
    /// anything. `Client::act` refuses instead, and reports it as a harness
    /// bug, which is what its own contract already says an unknown item or an
    /// unknown face is.
    Attack {
        /// An entity type id, e.g. `minecraft:player`. Resolved through
        /// `data/vanilla/entity_types.json`.
        target: String,
    },
    /// Right-click an **entity**: `ServerboundInteractPacket`, packet 26.
    ///
    /// # The verb every animal mechanic sits behind
    ///
    /// Feeding, breeding, a spawn egg used on a mob, a lead, a name tag, a
    /// saddle, taming: in vanilla every one of them is
    /// `ServerGamePacketListenerImpl.handleInteract` -> `Player.interactOn` ->
    /// `Mob.interact` -> `mobInteract`, and before this verb existed no
    /// scenario could send the packet at all. So none of them was untested; all
    /// of them were untestable — the same position `attack` was in.
    ///
    /// # The wire form, from the pinned jar
    ///
    /// `ServerboundInteractPacket.STREAM_CODEC` is
    /// `composite(VAR_INT entityId, InteractionHand.STREAM_CODEC hand,
    /// Vec3.LP_STREAM_CODEC location, BOOL usingSecondaryAction)`, and
    /// `InteractionHand.STREAM_CODEC` is `ByteBufCodecs.idMapper` over the
    /// enum's own `id` — a varint, 0 for `MAIN_HAND`. See
    /// `rustmc_proto::ids::play::sb::INTERACT` for why there is no action
    /// discriminant any more.
    ///
    /// `location` is the hit point relative to the entity. The harness sends
    /// the zero vector (`LpVec3`'s one-byte form), which is a point a real
    /// client can send and which nothing on a mob's path reads: only
    /// `ArmorStand.interact` consults it. A check against an armour stand would
    /// have to name a real hit point, and this field would then stop being
    /// fixed.
    ///
    /// `usingSecondaryAction` is **the sneak state this client last sent**,
    /// not a field of the step: `handleInteract` offset 52 writes it straight
    /// into `setShiftKeyDown`, so a hard-coded `false` would silently release
    /// a sneak a scenario had pressed — the bug [`Step::Sneak`] exists to keep
    /// out of the corpus.
    ///
    /// # No swing packet
    ///
    /// Unlike `attack`, nothing else is sent. Whether a real client swings
    /// after an interact depends on its own prediction of the result
    /// (`InteractionResult.SwingSource.CLIENT`), which this harness does not
    /// model; the server's own swing (`SwingSource.SERVER`, offsets 199-217 of
    /// `handleInteract`) is broadcast to everyone including the actor and is
    /// therefore what a check can see.
    ///
    /// # `target` resolves as `attack`'s does, and `pick` chooses among several
    ///
    /// Two pigs in one rig is the shape breeding needs, and `attack`'s "the
    /// most recently added live entity of that type" can only ever reach the
    /// second. `pick = "oldest"` reaches the first. Anything that does not
    /// resolve is refused, never sent — for `attack`'s reason.
    Interact {
        /// An entity type id, e.g. `minecraft:pig`. Required.
        target: String,
        /// `"newest"` (the default, `attack`'s rule) or `"oldest"`.
        #[serde(default = "newest")]
        pick: String,
        /// `"main"` (the default) or `"off"`.
        #[serde(default = "main_hand")]
        hand: String,
    },
    /// Hold or release the sneak key: `ServerboundPlayerInputPacket`, id 43.
    ///
    /// # Sneak is a state, not an event, and this verb is shaped like one
    ///
    /// There is no "sneak once". `handlePlayerInput` stores the whole input
    /// and then sets the flag from it:
    ///
    /// ```text
    ///   handlePlayerInput, javap -p -c -constants on the pinned jar
    ///     16: player.setLastClientInput(packet.input())
    ///     24: hasClientLoaded : ifeq 51 -> 51: return
    ///     34: player.resetLastActionTime()
    ///     42: packet.input().shift()
    ///     48: player.setShiftKeyDown(...)
    /// ```
    ///
    /// So the flag holds until another input packet clears it, and a scenario
    /// that sneaks and then does three things is sneaking for all three. A
    /// real client sends this packet every tick; the harness sends it only on
    /// change, which is equivalent because the server does nothing with it but
    /// store it and read the one bit.
    ///
    /// # What it unlocks
    ///
    /// `Player.isSecondaryUseActive()` reads `isShiftKeyDown()`, and that is
    /// the branch deciding whether a right-click reaches the block's own `use`
    /// or falls through to placing what is in hand. `docs/SERVER.md` names the
    /// resulting divergence outright — a sneaking player holding a block flips
    /// a lever on this server and places the block on vanilla — and until this
    /// verb existed there was no way to write a check against it recurring.
    Sneak {
        /// `true` presses the key, `false` releases it.
        on: bool,
    },
    /// Click a slot inside the menu that is currently open:
    /// `ServerboundContainerClickPacket`, packet 18.
    ///
    /// # The verb that makes storing, retrieving and crafting expressible
    ///
    /// Until this existed, every container in the corpus could only ever
    /// *open*. Four checks put a screen on the wire and not one of them could
    /// put an item anywhere, so "storing or crafting anything" — a
    /// first-ten-minutes action for every player — had zero checks, and
    /// `docs/SERVER.md`'s claim that "the first thing that will tell the
    /// difference is an item staying where it was put" was prose with no
    /// instrument behind it.
    ///
    /// # `slot` is an index into the **menu**, not into the inventory
    ///
    /// `AbstractContainerMenu` numbers the container's own slots first and
    /// then appends the player's, via `addStandardInventorySlots`: three rows
    /// of nine, then the hotbar's nine. So the hotbar's first slot is
    /// `container_slots + 27`, which is **54** in a 63-slot chest menu and
    /// **37** in a 46-slot crafting menu — and those two numbers are what a
    /// scenario writes when it means "the thing I am holding". This is a
    /// different numbering from [`Step::Hold`]'s, which is an `InventoryMenu`
    /// index where the same hotbar slot is 36; the two are only equal when no
    /// container is open.
    ///
    /// There is **no default**, for [`Step::Attack`]'s reason: `do =
    /// "container_click"` on its own would be a legal step clicking slot 0,
    /// and slot 0 of a chest is empty, so the scenario would record two
    /// servers agreeing about a click that did nothing.
    ///
    /// # What the harness claims about its own inventory, and why it claims
    /// nothing
    ///
    /// `changedSlots` and `carriedItem` are the *client's prediction* of the
    /// result, hashed. `handleContainerClick` writes them into the menu's
    /// remote-belief table and then calls `broadcastChanges`, which sends a
    /// `container_set_slot` **only for slots where the truth differs from what
    /// the client is believed to think**. This harness models no inventory, so
    /// it sends an empty map and an empty carried item — the honest claim, and
    /// also the load-bearing one:
    ///
    /// **a harness that predicted correctly would make this entire family of
    /// checks vacuous.** Vanilla would answer a correct prediction with
    /// silence, both servers would say nothing, and every storing and crafting
    /// check would report `match` against a server that has no containers at
    /// all. The prediction is empty on purpose, so that the server's answer is
    /// maximal and a check can see it.
    ///
    /// # It is state-shaped, and the state is reset structurally
    ///
    /// A click can leave a menu open and an item on the cursor, both of which
    /// outlive the scenario on one connection — the shape that made `sneak`
    /// silently disarm the check next to it. [`Step::CloseContainer`] is in
    /// `crate::record`'s per-scenario reset beside the sneak release for that
    /// reason.
    ContainerClick {
        /// The menu slot index. Required.
        slot: i32,
        /// `buttonNum`. 0 is a left click for `pickup`; for `swap` it is the
        /// destination hotbar index and for `throw` it is 0 or 1, which is why
        /// it is a plain number here rather than a name.
        #[serde(default)]
        button: i8,
        /// A `ContainerInput` name, lowercased: `pickup`, `quick_move`,
        /// `swap`, `clone`, `throw`, `quick_craft`, `pickup_all`.
        #[serde(default = "pickup")]
        input: String,
    },
    /// `ServerboundContainerClosePacket`, packet 19 — shut whatever menu is
    /// open.
    ///
    /// Used by `crate::record`'s reset before every scenario, and available to
    /// a scenario that wants to measure what closing does. Vanilla's handler
    /// checks nothing at all, so this is safe to send when nothing is open.
    CloseContainer,
    /// Press one of the open menu's buttons:
    /// `ServerboundContainerButtonClickPacket`, packet 17.
    ///
    /// `handleContainerButtonClick` drops the packet unless its container id
    /// names the open menu, then calls `clickMenuButton(player, button)` and
    /// broadcasts only when that returns `true`. What the number means is the
    /// menu's own business: an enchanting table's offer 0..2, a stonecutter's
    /// index into its recipe list, a loom's index into its pattern list.
    ///
    /// Refused with no menu open, for [`Step::ContainerClick`]'s reason — a
    /// button press naming no menu is dropped in silence and would record two
    /// servers agreeing about nothing.
    ContainerButtonClick {
        /// The button id. Required: no menu has a meaningful default.
        button: i32,
    },
    /// Type a name into an anvil: `ServerboundRenameItemPacket`, packet 48.
    ///
    /// One `writeUtf`. `handleRenameItem` acts only when the open menu is an
    /// `AnvilMenu`, through `AnvilMenu.setItemName`, which recomputes the
    /// result. Refused with no menu open, for the same reason as
    /// [`Step::ContainerButtonClick`].
    RenameItem { name: String },
    /// Type text into a sign: `ServerboundSignUpdatePacket`, packet 61.
    ///
    /// # What a real client is doing when it sends this
    ///
    /// A player never sends this out of nowhere. They place a sign; vanilla's
    /// `SignBlock.setPlacedBy` calls `openTextEdit`, which both records them
    /// as the only permitted editor and sends them a
    /// `ClientboundOpenSignEditorPacket`; they type; closing the screen sends
    /// this. So a scenario that means to measure signs **must place the sign
    /// itself, in the same scenario, as the same player** — see the position
    /// note below, which is the whole design of the verb.
    ///
    /// # The silence trap, which is worse here than it was for the container
    ///
    /// `Step::ContainerClick`'s documentation records that a harness which
    /// predicted correctly would have met silence and reported `match`
    /// against a server with no containers. Signs have the same shape and
    /// **two** doors instead of one, both of which return without sending
    /// anything at all:
    ///
    ///   * no `SignBlockEntity` at the position — which is *every* position on
    ///     this server, because it has no sign block entities;
    ///   * a sender who is not `getPlayerWhoMayEdit()`, or a waxed sign. The
    ///     entire effect is one `LOGGER.warn` on the server's console.
    ///
    /// Either way vanilla says nothing, this server says nothing, and a check
    /// projecting only what a sign edit *should* produce would call that
    /// agreement. **The green would be the bug**, exactly as it would have
    /// been for storing. The corpus answers it structurally rather than by
    /// care: the checks beside this verb project `open_sign_editor` and
    /// `block_entity_data`, both of which vanilla emits **unconditionally**
    /// once a sign is placed by the player driving it, so the reference is
    /// never silent and the runner's `vacuous` verdict is not what is holding
    /// the family up.
    ///
    /// # `pos` is the sign's own position, not the clicked one
    ///
    /// The packet carries the sign's `BlockPos`, so a scenario writes where
    /// the sign *is*: for a standing sign placed with `on = [1,-1,0]`, that is
    /// `[1,0,0]` — one **above** the block that was clicked. Anchor-relative
    /// like every other position in this file.
    ///
    /// Getting it wrong is not an error and does not look like one. It names a
    /// position with no sign block entity, `updateSignText` returns at offset
    /// 56, and the scenario records silence from both servers.
    SignUpdate {
        /// The sign's own position, anchor-relative.
        pos: [i32; 3],
        /// `isFrontText`. Vanilla defaults a freshly placed sign's edit to the
        /// front, so this defaults to `true`.
        #[serde(default = "front")]
        front: bool,
        /// The four lines. Fewer than four is padded with empty strings and
        /// more is refused, because the packet's array is `new String[4]` and
        /// the decoder's loop is unconditional — a fifth line has nowhere to
        /// go and silently vanishing one would be a scenario that measured
        /// something other than what it says.
        #[serde(default)]
        lines: Vec<String>,
    },
    /// Swing an arm: `ServerboundSwingPacket`, id 63.
    ///
    /// # The observation belongs to the *other* player, and that is the whole
    /// shape of this verb
    ///
    /// `handleAnimate` is three lines — `resetLastActionTime`, then
    /// `player.swing(packet.getHand())`. `LivingEntity.swing(hand)` is
    /// `swing(hand, false)`, and the `false` selects
    /// `ServerChunkCache.sendToTrackingPlayers` at offset 99 rather than
    /// `sendToTrackingPlayersAndSelf` at 89. `ChunkMap.TrackedEntity.
    /// updatePlayer` returns immediately when `player == entity`, so the
    /// swinging player is never in `seenBy` and **never receives their own
    /// `animate`**.
    ///
    /// A check that swung and watched the actor would therefore record
    /// silence against silence and report `match` for a server with no swing
    /// handling at all. Every check using this verb sets
    /// `observe_from = "observer"`.
    ///
    /// # The swallow, which is what makes a check here able to fail
    ///
    /// `swing(hand, boolean)` opens with
    ///
    /// ```text
    ///    0: swinging          ifeq 27   -> not swinging: swing
    ///    7: swingTime
    ///   11: getCurrentSwingDuration() / 2
    ///   17: if_icmpge 27                -> past halfway: swing again
    ///   20: swingTime         ifge 106  -> otherwise: return, sending nothing
    ///   27: swingTime = -1; swinging = true; swingingArm = hand
    /// ```
    ///
    /// so a second swing inside the first half of the animation produces **no
    /// packet at all**. `SwingAnimation.DEFAULT.duration()` is `bipush 6`, so
    /// half is 3 ticks — 150 ms. `updateSwingTime` increments `swingTime`
    /// once per tick from the `-1` the swing set, so the first tick after a
    /// swing puts it at 0, which is `>= 0` and `< 3`: swallowed.
    ///
    /// **That is the branch a check has to exercise**, because "the server
    /// echoed my swing" is satisfied by four lines of code that cannot be
    /// wrong. `ms` is how long to wait afterwards, so a pair of steps can sit
    /// either side of the halfway point.
    Swing {
        /// `main` or `off`. `InteractionHand`'s two constants, sent as the
        /// enum's ordinal varint — `MAIN_HAND` 0, `OFF_HAND` 1 — which the
        /// packet's `readEnum` reads back. The resulting `animate` action is
        /// **not** the same number: it is 0 for the main hand and **3** for
        /// the off hand, from the `if_acmpne` at offset 61 of `swing`.
        #[serde(default = "main_hand")]
        hand: String,
        /// How long to wait after sending it, in milliseconds. Defaults to
        /// one tick over the halfway point of the default animation — 200 ms
        /// against the 150 ms the swallow covers — so two consecutive
        /// `swing` steps with no `ms` produce two animations rather than
        /// silently measuring the swallowed case.
        #[serde(default = "past_the_swallow")]
        ms: u64,
    },
    /// `ServerboundClientCommandPacket`, packet 12 — respawn, and the two
    /// requests.
    ///
    /// # The packet the corpus could not send
    ///
    /// `rustmc_proto::ids::play::sb::CLIENT_COMMAND` has been declared since
    /// the id table was written and **nothing in this tree has ever
    /// constructed one**: zero hits across `tools/oracle-runner` and `ci/`
    /// before this commit. What that costs is not a missing check, it is a
    /// missing *end*: vanilla puts a dead player on a death screen and leaves
    /// them there until `PERFORM_RESPAWN` arrives, so every second of a
    /// recording after a death was vanilla waiting and this server waiting,
    /// and a check written over that window compares two silences.
    ///
    /// # The wire is one varint and the action is named, never numbered
    ///
    /// The packet is a single field written with `FriendlyByteBuf.writeEnum`,
    /// which is `writeVarInt(value.ordinal())` and nothing else (offsets
    /// 0..8). So the whole body is one varint, and [`client_command`] holds
    /// all three ordinals in the enum's own declaration order. A scenario
    /// writes the **name**: an unrecognised one is refused for
    /// [`Step::Swing`]'s reason, sharpened — the three actions do three
    /// unrelated things, so a typo silently defaulted would measure a
    /// mechanism the scenario never named and agree with itself.
    ///
    /// # What each of the three actually does, and the two silent doors
    ///
    /// `handleClientCommand` switches on the action
    /// (`ServerGamePacketListenerImpl`, `javap -p -c -constants` on the
    /// pinned jar, `tableswitch` at offset 32):
    ///
    /// ```text
    ///   PERFORM_RESPAWN            -> 60
    ///      67: wonGame ifeq 127          -> the credits path, unreachable here
    ///     127: player.getHealth()
    ///     135: fcmpl / 136: ifle 140
    ///     139: return                    <-- ALIVE: sends nothing at all
    ///     140: PlayerList.respawn(player, false, RemovalReason.KILLED)
    ///   REQUEST_STATS              -> 194
    ///     198: getStats() / 205: ServerStatsCounter.sendStats(player)
    ///   REQUEST_GAMERULE_VALUES    -> 211
    ///     212: sendGameRuleValues()
    /// ```
    ///
    /// **`perform_respawn` on a living player is a silence**, at offset 139,
    /// and it is the trap this verb is most likely to be used into: a
    /// scenario that respawns without having died records nothing from
    /// vanilla, nothing from this server, and reads as agreement. It is not
    /// refused here because it cannot be decided from the file — a creative
    /// harness player reaches zero health through a permission-2 console
    /// command *or* by walking below `getMinY() - 64`, and both are ordinary
    /// steps. The runner's **vacuous** verdict is what refuses it: a check
    /// whose reference produced nothing under its own projection can neither
    /// pass nor fail, which is exactly this case.
    ///
    /// **`request_gamerule_values` is a silence unconditionally, for these
    /// players, and that one IS decidable** — so it is refused at load. See
    /// `Scenario::load`.
    ///
    /// **`request_stats` is the one action with no door.**
    /// `ServerStatsCounter.sendStats` builds a map from the dirty set and
    /// sends a `ClientboundAwardStatsPacket` at its own offsets 56..71 with
    /// no branch in front of the send — an empty map is still a packet. It is
    /// therefore the anchor a scenario can use to prove this arm fires at all,
    /// independently of whether anything died.
    ClientCommand {
        /// `perform_respawn`, `request_stats` or `request_gamerule_values`.
        action: String,
    },
    /// A chat command, without the leading slash.
    Command { text: String },
    /// Say something in chat — `ServerboundChatPacket`.
    ///
    /// Distinct from [`Self::Command`] because they are different packets
    /// (9 against 7) taking different bodies, and because only this one
    /// produces a `player_chat` back. The timestamp and salt the client
    /// sends are fixed here rather than taken from the clock: vanilla
    /// **discards both** (`SignedMessageBody.unsigned` substitutes
    /// `Instant.now()` and `0`), so varying them would vary nothing
    /// observable while making the recording irreproducible.
    Say { text: String },
    /// Do nothing and keep listening.
    Wait { ms: u64 },
    /// Open the expendable third connection — see [`crate::trace::Who::Probe`].
    ///
    /// It joins exactly as the actor and observer do, so by the time this step
    /// returns it has answered the join teleport and is a normal, healthy
    /// client. Everything interesting happens to it afterwards.
    ///
    /// **The join itself is loud** — a login sequence, chunks, a player
    /// position — so a scenario that wants to measure one packet puts this in
    /// `setup`, whose observations are discarded, and keeps the measured steps
    /// for the mechanism. A probe left open at the end of a scenario is closed
    /// by the runner regardless, so there is no verb to close one and no way
    /// to leak one into the next scenario.
    ProbeJoin,
    /// Re-send the probe's last `ACCEPT_TELEPORTATION` — the same teleport id
    /// it has already acknowledged once.
    ///
    /// `handleAcceptTeleportPacket` never clears `awaitingTeleport` (only
    /// `awaitingPositionFromClient`, at offset 125), so the second
    /// acknowledgement matches the id at offset 20, finds nothing held at 27,
    /// and vanilla disconnects with
    /// `multiplayer.disconnect.invalid_player_movement`.
    ///
    /// This is the cheapest discriminator the serverbound census found: one
    /// packet, no world state, no dependence on anything else in the scenario,
    /// and a maximal divergence — a server that merely decodes the packet says
    /// nothing at all.
    ProbeAckTeleportAgain,
    /// A line written into the **server's own console**, not sent by a client.
    ///
    /// # Why this exists beside [`Self::Command`]
    ///
    /// `Step::Command` cannot reach any command above permission level 0. The
    /// harness's two players are `oracle` and `oracle_eye`, neither server has
    /// an `ops.json`, and op-ing them was considered and rejected — see
    /// [`crate::record::Console`]'s header: `sendPlayerPermissionLevel` sends
    /// an `entity_event` carrying `24 + level` **and** re-sends the whole
    /// `ClientboundCommandsPacket` filtered by the new level, so op-ing
    /// changes join traffic for every check in the corpus to buy a facility a
    /// handful need.
    ///
    /// The console needs none of that: `MinecraftServer.createCommandSourceStack()`
    /// is unconditionally full permission, because a process's own stdin has
    /// nothing to check a `PermissionCheck` against.
    ///
    /// # It reaches BOTH servers, unlike [`GameRule`]
    ///
    /// `GameRule`'s header describes a one-sided facility, and that stopped
    /// being true: `ci/t2-gameplay.sh` passes `--console` on **both** record
    /// passes — the reference's `$VAN_DIR/console.fifo` and the subject's
    /// `$OUR_DIR/console.fifo` — and `crate::console` is the reading end on
    /// our side. So a console step is symmetric and a check built on one is a
    /// real comparison rather than a reference running a mechanism the
    /// subject is not.
    ///
    /// A run with no `--console` **refuses** rather than skipping the line:
    /// a console step that quietly did nothing would leave a scenario whose
    /// measured window contains only what the step did not cause, which is
    /// this project's definition of a check that cannot fail.
    ///
    /// The text carries no leading slash, exactly like a line typed at a real
    /// server's console, and no newline — one line is one command, so a
    /// newline here would be a second command nobody declared.
    ///
    /// # `settle_ms`, for the checks that are about how close two lines are
    ///
    /// Without it a console step costs about 800 ms: [`crate::record`] sleeps
    /// `CONSOLE_SETTLE` inside the write and then drains for it again. That is
    /// sixteen ticks, and `LivingEntity.hurtServer`'s cooldown arm is live
    /// only while `invulnerableTime > 10.0f` (offsets 184..193) — the first
    /// ten of the twenty ticks a full hit sets. So two `damage` lines sent the
    /// default way can never land inside one cooldown, and the arm that deals
    /// the *difference* between two hits could not be measured at all.
    ///
    /// With `settle_ms` the write does not sleep and the step's whole wait is
    /// this figure, spent listening. The default is unchanged for every check
    /// that does not say it.
    Console {
        text: String,
        #[serde(default)]
        settle_ms: Option<u64>,
    },
}

fn main_hand() -> String {
    "main".into()
}

/// [`Step::Interact`]'s default `pick`: [`Step::Attack`]'s own rule.
fn newest() -> String {
    "newest".into()
}

/// 200 ms: four ticks, one past the three that `getCurrentSwingDuration() / 2`
/// covers at the default duration of 6.
const fn past_the_swallow() -> u64 {
    200
}

fn face_up() -> String {
    "up".into()
}

/// [`Step::ContainerClick`]'s default `input`: a plain left click.
fn pickup() -> String {
    "pickup".into()
}

/// [`Step::SignUpdate`]'s default `front`: vanilla opens a freshly placed
/// sign's editor on the front face.
const fn front() -> bool {
    true
}

/// `Step::Walk`'s default: a walk is on the ground unless it says otherwise.
const fn on_ground() -> bool {
    true
}

/// `Direction.get3DDataValue` — the ordinal the wire carries for all six.
///
/// Read from `Direction`'s own declaration order rather than assigned here:
/// down, up, north, south, west, east.
#[must_use]
pub fn face_value(name: &str) -> Option<u8> {
    Some(match name {
        "down" => 0,
        "up" => 1,
        "north" => 2,
        "south" => 3,
        "west" => 4,
        "east" => 5,
        _ => return None,
    })
}

/// The step from a block to its neighbour across a named face.
///
/// `Direction`'s normals, in the same declaration order `face_value` reads.
#[must_use]
pub fn face_offset(name: &str) -> Option<[i32; 3]> {
    Some(match name {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => return None,
    })
}

/// `ServerboundPlayerActionPacket$Action`, in the enum's own declaration
/// order, which is what `FriendlyByteBuf.readEnum` reads off the wire.
///
/// Written out as named constants rather than computed, because computing
/// them is what went wrong. [`Step::FinishDig`] used to send
/// `i32::from(matches!(step, Step::FinishDig { .. }))` — one, on the
/// assumption that the finish of a dig is the action after its start. It is
/// not: one is `ABORT_DESTROY_BLOCK` and `STOP_DESTROY_BLOCK` is two, so
/// every `finish_dig` cancelled the dig it claimed to complete. No check in
/// the corpus used the action, so nothing could have caught it, and a
/// survival-mining check written on top of it would have measured an abort
/// and reported it as vanilla's behaviour. Verified against the jar
/// 2026-08-03; `Action` gained an eighth entry, `STAB`, in 26.2.
///
/// The whole enum is written out, including the four entries nothing sends.
/// A partial transcription is how the ordinals go wrong: the value of this
/// table is that the gap between two constants is visible, and an entry
/// omitted because it was not needed is exactly the entry that makes the
/// next one silently off by one.
#[allow(dead_code)]
pub mod player_action {
    pub const START_DESTROY_BLOCK: i32 = 0;
    pub const ABORT_DESTROY_BLOCK: i32 = 1;
    pub const STOP_DESTROY_BLOCK: i32 = 2;
    pub const DROP_ALL_ITEMS: i32 = 3;
    pub const DROP_ITEM: i32 = 4;
    pub const RELEASE_USE_ITEM: i32 = 5;
    pub const SWAP_ITEM_WITH_OFFHAND: i32 = 6;
    pub const STAB: i32 = 7;
}

/// `ServerboundClientCommandPacket$Action` — all three, in the declaration
/// order the class file carries them in.
///
/// The wire value is the **ordinal**, because the packet's `write` is a bare
/// `FriendlyByteBuf.writeEnum` and `writeEnum` is
/// `writeVarInt(value.ordinal())` (offsets 0..8). There is no `id()` field to
/// prefer over the ordinal, which is the opposite of [`container_input`] —
/// and the two are written out separately here rather than sharing a helper
/// so that the difference is visible rather than assumed.
///
/// All three, including the one [`Scenario::load`] refuses, for the reason
/// [`player_action`] gives: a gap in a complete table is visible and a partial
/// transcription is how the next constant ends up off by one. 26.2 has three;
/// `REQUEST_GAMERULE_VALUES` is not in any account older than it, so a
/// transcription from memory would have had two and put nothing at 2.
pub mod client_command {
    pub const PERFORM_RESPAWN: i32 = 0;
    pub const REQUEST_STATS: i32 = 1;
    pub const REQUEST_GAMERULE_VALUES: i32 = 2;
}

/// The wire ordinal for a [`Step::ClientCommand`] action name, or `None`.
///
/// `None` is a refusal at the call site and never a default. Sending
/// `PERFORM_RESPAWN` for a misspelt `request_stats` is a legal packet doing
/// something else entirely, and on a living player it does nothing at all —
/// so a defaulted typo would record silence and report `match`.
#[must_use]
pub fn client_command_action(name: &str) -> Option<i32> {
    Some(match name {
        "perform_respawn" => client_command::PERFORM_RESPAWN,
        "request_stats" => client_command::REQUEST_STATS,
        "request_gamerule_values" => client_command::REQUEST_GAMERULE_VALUES,
        _ => return None,
    })
}

/// `net.minecraft.world.entity.player.Input`'s flag bits — the whole byte
/// `ServerboundPlayerInputPacket` carries.
///
/// **All seven, including the six nothing sends**, for exactly the reason
/// [`player_action`] gives: a partial transcription is how a bit ends up in
/// the wrong place, and the value of a complete table is that a gap in it is
/// visible. These are `private static final byte` fields on `Input` itself,
/// read from the class rather than inferred from `Input$1.encode`'s
/// `iconst_`/`bipush` sequence — though the two agree, and `encode` ors them
/// in this order before a single `writeByte`.
///
/// Sneak is `SHIFT`. It is not in `ServerboundPlayerCommandPacket$Action`,
/// which is where every account older than 26.2 puts it; see
/// `rustmc_proto::ids::play::sb::PLAYER_INPUT`.
#[allow(dead_code)]
pub mod input_flags {
    pub const FORWARD: u8 = 1;
    pub const BACKWARD: u8 = 2;
    pub const LEFT: u8 = 4;
    pub const RIGHT: u8 = 8;
    pub const JUMP: u8 = 16;
    pub const SHIFT: u8 = 32;
    pub const SPRINT: u8 = 64;
}

/// `net.minecraft.world.inventory.ContainerInput` — the seven values
/// `ServerboundContainerClickPacket` can carry, by their **declared `id`**.
///
/// # Not `ClickType`, and not an ordinal
///
/// `ClickType` is gone from the pinned jar. `ContainerInput` replaces it and
/// is not a bare enum: each constant is built with an explicit `int id` —
/// `<init>(String, int ordinal, int id)` — and the codec is
///
/// ```text
///   BY_ID        = ByIdMap.continuous(ContainerInput::id, values(),
///                                     OutOfBoundsStrategy.ZERO)
///   STREAM_CODEC = ByteBufCodecs.idMapper(BY_ID, ContainerInput::id)
/// ```
///
/// so the wire carries `id()` as a varint. The static initialiser passes the
/// ordinal and the id as the same literal for all seven, so the two agree
/// today — which is precisely why this is written out from the `id` argument
/// rather than counted off the field list. A future entry inserted with a
/// non-matching id would break an ordinal-based transcription silently, and
/// `OutOfBoundsStrategy.ZERO` means an out-of-range value decodes as
/// **`PICKUP`** rather than being refused: a wrong number here is a legal
/// click of a different kind, never a decode error.
///
/// All seven, including the four nothing sends, for the reason
/// [`player_action`] gives: a gap in a complete table is visible and a partial
/// transcription is how the next constant ends up off by one.
#[allow(dead_code)]
pub mod container_input {
    pub const PICKUP: i32 = 0;
    pub const QUICK_MOVE: i32 = 1;
    pub const SWAP: i32 = 2;
    pub const CLONE: i32 = 3;
    pub const THROW: i32 = 4;
    pub const QUICK_CRAFT: i32 = 5;
    pub const PICKUP_ALL: i32 = 6;
}

/// A `ContainerInput` name as a scenario writes it, to the id on the wire.
///
/// `None` for anything else, which [`crate::client::Client::act`] turns into a
/// harness error rather than a packet — the same contract an unknown face or
/// an unknown item already has. Defaulting an unrecognised name to `PICKUP`
/// would be the natural thing and is exactly wrong: `OutOfBoundsStrategy.ZERO`
/// means the *server* would do that too, so a typo would click, do something
/// plausible, and never be reported.
#[must_use]
pub fn container_input_value(name: &str) -> Option<i32> {
    Some(match name {
        "pickup" => container_input::PICKUP,
        "quick_move" => container_input::QUICK_MOVE,
        "swap" => container_input::SWAP,
        "clone" => container_input::CLONE,
        "throw" => container_input::THROW,
        "quick_craft" => container_input::QUICK_CRAFT,
        "pickup_all" => container_input::PICKUP_ALL,
        _ => return None,
    })
}

/// An `InteractionHand` name as a scenario writes it, to the ordinal on the
/// wire.
///
/// `None` for anything else, and [`crate::client::Client::act`] turns that
/// into a harness error rather than a packet. **Defaulting a typo to the main
/// hand would be worse here than in most places**: the two hands produce
/// *different* `animate` actions — 0 and 3 — so a scenario meaning to check
/// the off hand and misspelling it would assert the main hand's number,
/// agree with vanilla, and record `match` for a mechanism it never reached.
#[must_use]
pub fn hand_ordinal(name: &str) -> Option<i32> {
    Some(match name {
        // `InteractionHand`'s declaration order, which is what `readEnum`
        // indexes: MAIN_HAND then OFF_HAND.
        "main" | "main_hand" => 0,
        "off" | "off_hand" => 1,
        _ => return None,
    })
}

/// The middle of a face, in the face's own plane — what a client sends when
/// a player clicks the centre of a block side.
#[must_use]
pub fn face_cursor(face: u8) -> [f32; 3] {
    match face {
        0 => [0.5, 0.0, 0.5],
        1 => [0.5, 1.0, 0.5],
        2 => [0.5, 0.5, 0.0],
        3 => [0.5, 0.5, 1.0],
        4 => [0.0, 0.5, 0.5],
        _ => [1.0, 0.5, 0.5],
    }
}

impl Scenario {
    /// Load one scenario file.
    ///
    /// # Errors
    /// If the file is unreadable or is not a scenario.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut s: Self = toml::from_str(&text)
            .map_err(|e| std::io::Error::other(format!("{}: {e}", path.display())))?;
        if s.name.is_empty() {
            s.name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        // **A declaration that cannot bite is a declaration nobody will notice
        // is dead**, and the test for "cannot bite" changed when the collapse
        // started running on both denominators.
        //
        // It used to be "must appear in `observe`", because the collapse only
        // ever saw kinds that had survived the `observe` filter. That is no
        // longer true: the strict projection keeps every kind, so a name
        // outside `observe` now does real work there — and that is precisely
        // the case the container checks need, since not one of them observes
        // `block_event`.
        //
        // What stays refused is the case the old rule was actually protecting
        // against: a name **no packet can ever carry**. A typo, or a kind
        // renamed in `trace::decode` and not renamed here, still collapses
        // nothing in either denominator for ever, and still leaves the check
        // flapping while its file says it does not.
        //
        // `trace::DECODED_KINDS` is the list, and its failure direction is the
        // safe one: a decoder arm added without updating it makes a check that
        // names the new kind **refuse to load**, which is loud, rather than
        // silently doing nothing, which is not.
        if let Some(bad) = s
            .collapse_repeats
            .iter()
            .find(|k| !s.observe.contains(k) && !crate::trace::DECODED_KINDS.contains(&k.as_str()))
        {
            return Err(std::io::Error::other(format!(
                "{}: collapse_repeats names {bad:?}, which is neither in observe \
                 nor a kind any decoder arm produces — it would have no effect",
                path.display()
            )));
        }
        // **A scenario with no `[[step]]` has no measured step**, and
        // [`crate::compare::Verdict::Mute`] is then a question with no
        // subject: there is no action for the reference to have been silent
        // about. Refused here rather than handled there, because it is a
        // static property of the file and the alternative is a check that
        // sits outside the third verdict's reach for ever without saying so.
        //
        // Nothing in the corpus has ever done this. The refusal exists so
        // that "every scenario has a measured step" is a fact the comparator
        // may rely on rather than a coincidence it must re-check.
        if s.step.is_empty() {
            return Err(std::io::Error::other(format!(
                "{}: no [[step]] — a scenario with no measured part cannot pass or fail, \
                 and has no measured step for `measured_step` to be about",
                path.display()
            )));
        }
        // **A component type this harness cannot encode is refused here, at
        // load, and not at the point of encoding it.**
        //
        // `Client::hold` refuses it too, and that check stays — it is the one
        // guarding the buffer, and it must hold whoever calls it. But by the
        // time it fires, `ci/t2-gameplay.sh` holds the corpus lock, both
        // servers are up, and some number of scenarios have already been
        // recorded; the run then dies partway with a message about a stack.
        // The typo was a static property of the file the whole time.
        //
        // The list is short (`ENCODABLE_COMPONENTS`) because each entry is a
        // stream codec written out of the jar, and every other type is a refusal rather
        // than a guess: writing the wrong bytes for a component throws inside
        // vanilla's own packet read, and what arrives in the recording is a
        // reference that stopped answering rather than a divergence anybody
        // can read. `docs/PARITY-ROADMAP.md` §44.5.
        // A console line is written into a fifo verbatim and one line is one
        // command, so a newline in one is a second command nobody declared —
        // the same rule `GameRule::validate` applies, refused at load for the
        // same reason: the file's content is a static property of the file.
        // A console step without a restore is the ninety-check defect of
        // 2026-09-05 and it is refused here so that it cannot be re-landed.
        //
        // **A death is the one console step whose restore is not a console
        // line, and it is interlocked here rather than exempted.**
        //
        // The only route to zero health for a creative harness player is a
        // permission-2 command: `LivingEntity.kill(ServerLevel)` overrides
        // `Entity.kill` with `hurtServer(damageSources().genericKill(),
        // Float.MAX_VALUE)` (offsets 3..11), and `minecraft:generic_kill` is
        // one of the two entries in the `bypasses_invulnerability` damage-type
        // tag, so `Player.isInvulnerableTo` does not stop it. `ServerPlayer`
        // does not override `kill`.
        //
        // What that leaves behind is not a world setting anybody can put back
        // with a second console line. A dead player stays dead and stays on
        // the death screen, and every LATER scenario would then be recorded
        // against a corpse — the 2026-09-05 worldborder defect exactly, in the
        // one shape a `console_restore` cannot express, because only the
        // *client* can respawn. So the restore for a death is a
        // `do = "client_command"` / `perform_respawn` step, and it is required
        // to come after the death rather than merely to exist.
        // `kill` only. A `damage` line can kill too, but whether it does
        // depends on the health the player arrived with, which no static
        // check can know; a `damage` step is therefore an ordinary console
        // step — it must declare a `console_restore`, and the one that puts
        // health back exactly is `kill <player>`, after which
        // `record::respawn_if_dead` answers the death with the client's own
        // respawn whenever the server has reported zero health.
        let death_at = |st: &Step| match st {
            Step::Console { text, .. } => {
                matches!(text.split_whitespace().next(), Some("kill"))
            }
            _ => false,
        };
        let all: Vec<&Step> = s.setup.iter().chain(s.step.iter()).collect();
        let last_death = all.iter().rposition(|st| death_at(st));
        let last_respawn = all.iter().rposition(
            |st| matches!(st, Step::ClientCommand { action } if action == "perform_respawn"),
        );
        if let Some(d) = last_death {
            if last_respawn.is_none_or(|r| r < d) {
                return Err(std::io::Error::other(format!(
                    "{}: step {d} kills a harness player through the console and no \
                     later step respawns them. `LivingEntity.kill` is \
                     `hurtServer(genericKill(), MAX_VALUE)` and `generic_kill` bypasses \
                     creative invulnerability, so the player really dies — and stays \
                     dead, on the death screen, for every LATER scenario in the corpus. \
                     No console line can put that back; only the client can. Add \
                     `do = \"client_command\"` with `action = \"perform_respawn\"` after it.",
                    path.display()
                )));
            }
        }
        // A death-only scenario needs no `console_restore`: the respawn above
        // IS its restore, and demanding a console line as well would be
        // demanding a lie. Any OTHER console step still needs one.
        let has_console = all
            .iter()
            .any(|st| matches!(st, Step::Console { .. }) && !death_at(st));
        if has_console && s.console_restore.is_empty() {
            return Err(std::io::Error::other(format!(
                "{}: has a `do = \"console\"` step and no `console_restore`. A console \
                 step is global server state by construction — the only commands it can \
                 reach are the permission-2 ones — so leaving it set runs every LATER \
                 scenario on a substrate nobody declared. That is not hypothetical: on \
                 2026-09-05 an unrestored `worldborder set 137` put ninety rigs outside \
                 the world border and vanilla answered every one with `build.tooHigh`, \
                 reported as ninety regressions naming none of it. Declare the lines \
                 that put it back.",
                path.display()
            )));
        }
        for line in &s.console_restore {
            if line.contains(['\n', '\r']) || line.trim().is_empty() {
                return Err(std::io::Error::other(format!(
                    "{}: console_restore line {line:?} is empty or holds a newline — \
                     one line is one command",
                    path.display()
                )));
            }
        }
        // Walked in send order across setup and step alike, because a use
        // opened in `[[setup]]` is still open when `[[step]]` starts —
        // `[[setup]]` is unmeasured, not unsent.
        let mut held: Option<String> = None;
        let mut using: Option<String> = None;
        for (i, step) in s.setup.iter().chain(s.step.iter()).enumerate() {
            match step {
                Step::Hold { item, .. } => held = Some(item.clone()),
                // Only an item whose `use` calls `startUsingItem` opens a use.
                // `BoatItem.use` overrides `Item.use` without calling it or
                // `super` (javap of the pinned jar: no `startUsingItem` in the
                // class), so a boat or raft placed by `use_item` leaves
                // `useItem` empty and there is nothing to release. Without this
                // the guard refused main's `776-a-boat-is-placed-by-right-
                // clicking-the-world`, which ends on exactly that use.
                Step::UseItem => {
                    using = held.clone().filter(|item| !opens_no_use(item));
                }
                // `handlePlayerAction`'s SWAP_ITEM_WITH_OFFHAND case sets both
                // hands and then calls `stopUsingItem` at offset 218, so a
                // swap really does end an open use — this is read from the
                // handler, not guessed at from what moving a stack ought to
                // do.
                Step::SwapOffhand => {
                    held = None;
                    using = None;
                }
                // A drop does not. `player.drop(Z)` at offsets 237 and 256 is
                // the whole of those two cases, so the use stays open with the
                // stack gone from the hand: a release then fails `isSameItem`
                // and the refusal below is the one that fires.
                Step::Drop { .. } => held = None,
                _ => {}
            }

            if let Step::Console { text, .. } = step {
                if text.contains(['\n', '\r']) {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} is a console line carrying a newline ({text:?}) — \
                         one line is one command, so this would run a command the \
                         scenario does not declare",
                        path.display()
                    )));
                }
                if text.trim().is_empty() {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} is an empty console line, which every server \
                         ignores — a step that cannot do anything is a step whose \
                         check cannot fail",
                        path.display()
                    )));
                }
            }
            // `release_use`'s two silent doors, both decidable here. See
            // `Step::ReleaseUse` for the bytecode: `releaseUsingItem` returns
            // having sent nothing when `useItem` is empty (offset 16) or when
            // the stack in the used hand is no longer the same item (27).
            //
            // `held` is what the last `hold` selected and `using` is what was
            // held when the last `use_item` went out. `None` for `using` means
            // no use is open.
            if let Step::ReleaseUse = step {
                let Some(open) = using.clone() else {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} is `do = \"release_use\"` with no earlier \
                         `do = \"use_item\"`. `LivingEntity.releaseUsingItem` returns at \
                         its offset 16 on an empty `useItem`, so this sends a packet that \
                         does nothing — on both servers — and the check reports `match` \
                         for a mechanism neither ran. A release is the second half of a \
                         use, never a step of its own.",
                        path.display()
                    )));
                };
                if held.as_deref() != Some(open.as_str()) {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} releases a use of {open:?} but the used hand now \
                         holds {held:?}. `releaseUsingItem` tests \
                         `ItemStack.isSameItem(inHand, useItem)` at its offset 24 and \
                         returns at 27 when it fails, sending nothing. Do not change \
                         hands between a `use_item` and its `release_use`.",
                        path.display()
                    )));
                }
                // Closed only after both checks have read it. Clearing it in
                // the tracker at the top of the loop would clear it before the
                // checks ran, and every honest release would refuse itself.
                using = None;
            }
            if let Step::ClientCommand { action } = step {
                let Some(ordinal) = crate::scenario::client_command_action(action) else {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} is `do = \"client_command\"` with action {action:?}, \
                         which is not one of the three. They are `perform_respawn`, \
                         `request_stats` and `request_gamerule_values`. Defaulting an \
                         unknown one would send a legal packet for a different action.",
                        path.display()
                    )));
                };
                // **`REQUEST_GAMERULE_VALUES` is unreachable for this corpus and
                // refused here rather than sent.**
                //
                // `handleClientCommand` sends it to `sendGameRuleValues`, whose
                // first act is a permission test:
                //
                // ```text
                //     7: Permissions.COMMANDS_GAMEMASTER
                //    10: PermissionSet.hasPermission
                //    15: ifne 40
                //    21: LOGGER.warn "Player {} tried to request game rule values
                //                    without required permissions"
                //    39: return                <-- sends nothing
                //    40: ... ClientboundGameRuleValuesPacket
                // ```
                //
                // The harness's two players are `oracle` and `oracle_eye`, neither
                // server has an `ops.json`, and op-ing them is rejected for the
                // reason `crate::record::Console`'s header gives. So this action
                // is a `LOGGER.warn` on both servers' consoles and a silence on
                // both wires, and a scenario using it would report `match` for a
                // mechanism neither server ran. This is `Step::Console`'s whole
                // reason for existing, arriving one packet later.
                if ordinal == client_command::REQUEST_GAMERULE_VALUES {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} requests gamerule values, which needs \
                         COMMANDS_GAMEMASTER. The harness's players are never operators, \
                         so `sendGameRuleValues` returns at its offset 39 having sent \
                         nothing — on BOTH servers. The check would report `match` for a \
                         packet neither server wrote. There is no scenario-level route to \
                         this action; the values themselves are readable through \
                         `do = \"console\"`.",
                        path.display()
                    )));
                }
            }
            if let Step::Hold { components, .. } = step {
                if let Some((c, why)) = components.iter().find_map(|c| c.refusal().map(|w| (c, w)))
                {
                    return Err(std::io::Error::other(format!(
                        "{}: step {i} holds a stack with component {:?}, which this harness \
                         has no stream codec for ({why}). Encodable: {:?}. Writing a guessed \
                         payload throws inside vanilla's packet read, and the recording then \
                         shows a reference that stopped answering rather than a divergence",
                        path.display(),
                        c.kind,
                        crate::scenario::ENCODABLE_COMPONENTS,
                    )));
                }
            }
        }
        // **A use left open outlives the scenario, and that is the 552/553
        // defect in a second shape.**
        //
        // Every scenario in a run shares one connection, so state a scenario
        // leaves on the server is inherited by the next one —
        // `crate::record`'s per-scenario reset exists for exactly that and its
        // sneak release is the precedent. `startUsingItem` sets `useItem` and
        // nothing clears it: a bow's `getUseDuration` is 72000 ticks, so a
        // scenario that draws one and stops leaves **every later scenario in
        // the corpus drawing a bow**. 552/553 is what that costs — two checks
        // that were the two halves of one pair, producing identical vanilla
        // traces because the first silently disarmed the second.
        //
        // Refused here rather than fixed in the recorder, deliberately. An
        // unconditional release in the reset would be inert by the same
        // argument the sneak release uses — `releaseUsingItem` returns at
        // offset 16 on an empty `useItem`, and the `stopUsingItem` it falls
        // through to sets an already-false `setLivingEntityFlag(1, false)`,
        // which `SynchedEntityData` does not broadcast — but it would put one
        // more packet on every scenario in the corpus to fix the ones that
        // draw, which is the trade `Step::CloseContainer` is conditional for.
        // Making the author say where the release goes costs nothing and is
        // visible in the file.
        if using.is_some() {
            return Err(std::io::Error::other(format!(
                "{}: ends with a `use_item` never released. `startUsingItem` sets \
                 `useItem` and nothing clears it — a bow's `getUseDuration` is 72000 \
                 ticks — and every scenario in a run shares one connection, so this \
                 leaves EVERY LATER scenario in the corpus mid-use. That is the \
                 552/553 defect: a check made unable to fail by the check before it. \
                 Add a `do = \"release_use\"`, or a `do = \"swap_offhand\"`, whose \
                 handler calls `stopUsingItem` at offset 218.",
                path.display()
            )));
        }
        // A game rule reaches the reference as a line in its console fifo, so
        // a newline or a space in any of the three strings is a command the
        // scenario did not declare. Refused here, where it is a static
        // property of the file, rather than escaped at the point of writing.
        if let Some(bad) = s.gamerule.iter().find(|g| !g.well_formed()) {
            return Err(std::io::Error::other(format!(
                "{}: [[gamerule]] rule={:?} value={:?} restore={:?} — each must be a \
                 non-empty run of [A-Za-z0-9_] (value and restore may also carry '-'), \
                 because they are written into a server console one line at a time",
                path.display(),
                bad.rule,
                bad.value,
                bad.restore
            )));
        }
        // Two declarations of the same rule in one scenario have no defined
        // order and their `restore`s would race. Nothing needs it.
        for i in 1..s.gamerule.len() {
            if s.gamerule[..i].iter().any(|g| g.rule == s.gamerule[i].rule) {
                return Err(std::io::Error::other(format!(
                    "{}: [[gamerule]] names {:?} twice",
                    path.display(),
                    s.gamerule[i].rule
                )));
            }
        }
        Ok(s)
    }

    /// Every scenario in a directory, by name.
    ///
    /// # Errors
    /// If the directory cannot be read, a file will not parse, or two
    /// scenarios share a name — a duplicate would silently halve the
    /// denominator, which is the one number this harness exists to publish.
    pub fn load_dir(dir: &std::path::Path) -> std::io::Result<Vec<Self>> {
        let mut out = Vec::new();
        let mut paths: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        paths.sort();
        for path in paths {
            out.push(Self::load(&path)?);
        }
        for i in 1..out.len() {
            if out[i - 1..]
                .iter()
                .skip(1)
                .any(|s| s.name == out[i - 1].name)
            {
                return Err(std::io::Error::other(format!(
                    "two scenarios are called {:?}",
                    out[i - 1].name
                )));
            }
        }
        Ok(out)
    }
}

/// Items whose `use` never calls `startUsingItem`, so a `use_item` with one
/// in hand opens nothing for the unreleased-use guard to refuse: `BoatItem`
/// (every `*_boat`, `*_chest_boat`, `*_raft` and `*_chest_raft`),
/// `BucketItem` for the empty, water and lava buckets (fluids' scenarios —
/// `use` places or picks up and returns; the milk bucket is a `consumable`
/// and does start a use, so it is not here) and `EggItem` (passive-mobs'
/// throw). Those are the items a scenario uses today; an item missing from
/// here is refused, which is the safe direction.
fn opens_no_use(item: &str) -> bool {
    item.ends_with("_boat")
        || item.ends_with("_raft")
        || matches!(
            item,
            "minecraft:bucket"
                | "minecraft:water_bucket"
                | "minecraft:lava_bucket"
                | "minecraft:egg"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scenario TOML carrying `components` parses, and one naming a type
    /// with no codec is refused **at load**.
    ///
    /// The parse half is not ceremony. Nothing else in this file tests that
    /// `components` survives deserialization at all: the encode-side tests
    /// build `Step::Hold` in Rust and never go through `toml`, so a wrong
    /// `#[serde(rename)]` on `HeldComponent::kind` would have left every
    /// scenario silently componentless — which is the exact failure
    /// `PARITY-ROADMAP` §44.5 is about, arriving a second time by a different
    /// door.
    #[test]
    fn hold_components_parse_and_unknown_types_are_refused_at_load() {
        let dir = std::env::temp_dir().join(format!("scen-comp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let good = dir.join("good.toml");
        std::fs::write(
            &good,
            "name = \"good\"\n\
             about = \"a fixture for the component parse test\"\n\
             observe = [\"container_set_slot\"]\n\
             [[step]]\ndo = \"hold\"\nslot = 0\nitem = \"minecraft:diamond_sword\"\n\
             components = [{ type = \"minecraft:damage\", varint = 5 }]\n",
        )
        .unwrap();
        let s = Scenario::load(&good).expect("a damage component must load");
        match &s.step[0] {
            Step::Hold { components, .. } => {
                assert_eq!(components.len(), 1, "the component must survive the parse");
                assert_eq!(components[0].kind, "minecraft:damage");
                assert_eq!(
                    components[0].varint,
                    Some(5),
                    "`varint` must not silently default"
                );
            }
            other => panic!("wanted a Hold, got {other:?}"),
        }

        let bad = dir.join("bad.toml");
        std::fs::write(
            &bad,
            "name = \"bad\"\n\
             about = \"a fixture for the component parse test\"\n\
             observe = [\"container_set_slot\"]\n\
             [[step]]\ndo = \"hold\"\nslot = 0\nitem = \"minecraft:diamond_sword\"\n\
             components = [{ type = \"minecraft:custom_name\", varint = 1 }]\n",
        )
        .unwrap();
        let e = Scenario::load(&bad).expect_err("a type with no codec must be refused at load");
        let msg = e.to_string();
        assert!(
            msg.contains("minecraft:custom_name") && msg.contains("no stream codec"),
            "the refusal must name the offending type and why: {msg}"
        );

        // The same refusal must reach a `hold` in `setup`, which is a
        // different vector into the same encoder.
        let setup = dir.join("setup.toml");
        std::fs::write(
            &setup,
            "name = \"setup\"\n\
             about = \"a fixture for the component parse test\"\n\
             observe = [\"container_set_slot\"]\n\
             [[setup]]\ndo = \"hold\"\nslot = 0\nitem = \"minecraft:diamond_sword\"\n\
             components = [{ type = \"minecraft:custom_name\", varint = 1 }]\n\
             [[step]]\ndo = \"look\"\nyaw = 0.0\npitch = 0.0\n",
        )
        .unwrap();
        Scenario::load(&setup).expect_err("a setup-phase hold must be refused the same way");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn faces_use_the_three_d_data_value() {
        // The ordinal is what the wire carries, so an off-by-one here places
        // blocks on the wrong side of the one they were aimed at.
        assert_eq!(face_value("down"), Some(0));
        assert_eq!(face_value("up"), Some(1));
        assert_eq!(face_value("east"), Some(5));
        assert_eq!(face_value("sideways"), None);
    }

    #[test]
    fn a_face_cursor_lies_on_its_own_face() {
        for face in 0..6u8 {
            let c = face_cursor(face);
            // `Direction`'s ordinal groups the axes in the order Y, Z, X —
            // DOWN, UP, NORTH, SOUTH, WEST, EAST — so the pair index is
            // **not** the coordinate index. This test asserted `face / 2`
            // directly and so demanded that DOWN's cursor lie on the *x*
            // face; it had never run, because `ci/t0.sh` ran
            // `cargo test --workspace --lib` and `oracle-runner` is a binary
            // crate whose tests are `--bins`. `face_cursor` itself was always
            // right.
            let axis = [1usize, 2, 0][usize::from(face) / 2];
            let want = if face % 2 == 0 { 0.0 } else { 1.0 };
            assert!(
                (c[axis] - want).abs() < f32::EPSILON,
                "face {face} cursor {c:?} is not on the face"
            );
        }
    }

    #[test]
    fn a_scenario_parses_from_toml() {
        let s: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["block_update"]
[[step]]
do = "dig"
at = [0, -1, 0]
"#,
        )
        .expect("parses");
        assert_eq!(s.step.len(), 1);
        assert_eq!(s.settle_ms, default_settle());
    }

    /// The ordinals are the whole content of `player_action`, so the test is
    /// the only thing standing between a transcription slip and a scenario
    /// that measures a different action from the one it names. It is
    /// deliberately written as the literal declaration order of
    /// `ServerboundPlayerActionPacket$Action` rather than as arithmetic on
    /// the constants, because arithmetic on the constants is what produced
    /// the bug: `finish_dig` sent `START + 1` and `START + 1` is
    /// `ABORT_DESTROY_BLOCK`.
    #[test]
    fn player_action_ordinals_are_the_enums_declaration_order() {
        use player_action as pa;
        assert_eq!(
            [
                pa::START_DESTROY_BLOCK,
                pa::ABORT_DESTROY_BLOCK,
                pa::STOP_DESTROY_BLOCK,
                pa::DROP_ALL_ITEMS,
                pa::DROP_ITEM,
                pa::RELEASE_USE_ITEM,
                pa::SWAP_ITEM_WITH_OFFHAND,
                pa::STAB,
            ],
            [0, 1, 2, 3, 4, 5, 6, 7],
        );
        assert_ne!(
            pa::STOP_DESTROY_BLOCK,
            pa::ABORT_DESTROY_BLOCK,
            "finishing a dig is not aborting one"
        );
    }

    /// A console death with no respawn after it is refused, and a death that
    /// respawns needs no `console_restore`.
    ///
    /// This is the interlock between the two verbs added on 2026-09-06 and the
    /// guard the 2026-09-05 worldborder defect left behind. `kill` is the only
    /// route to a dead creative harness player — `LivingEntity.kill` is
    /// `hurtServer(genericKill(), MAX_VALUE)` and `generic_kill` is one of the
    /// two entries in the `bypasses_invulnerability` tag — and what it leaves
    /// behind cannot be undone by a second console line, because only the
    /// client respawns. Without this, a death-respawn scenario that forgot its
    /// respawn would leave every LATER scenario in the corpus recorded against
    /// a corpse on a death screen, which is the ninety-check shape in the one
    /// form `console_restore` cannot express.
    #[test]
    fn a_console_death_must_respawn_and_a_respawn_is_its_own_restore() {
        let dir = std::env::temp_dir().join(format!("scen-death-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let head = "about = \"a fixture for the death/respawn interlock\"\n\
                    observe = [\"set_health\"]\n";
        let write = |n: &str, body: &str| {
            let p = dir.join(format!("{n}.toml"));
            std::fs::write(&p, format!("name = \"{n}\"\n{head}{body}")).unwrap();
            p
        };
        let kill = "[[step]]\ndo = \"console\"\ntext = \"kill oracle\"\n";
        let respawn = "[[step]]\ndo = \"client_command\"\naction = \"perform_respawn\"\n";

        // No `console_restore` and it must still load: the respawn is the
        // restore, and demanding a console line as well would be demanding a
        // lie.
        Scenario::load(&write("killed", &format!("{kill}{respawn}")))
            .expect("a death followed by a respawn is complete without a console_restore");

        let e = Scenario::load(&write("no-respawn", kill))
            .expect_err("a death with no respawn poisons every later scenario");
        assert!(
            e.to_string().contains("perform_respawn"),
            "the refusal must name the step that fixes it: {e}"
        );

        // Order is the whole point: a respawn BEFORE the death restores
        // nothing.
        Scenario::load(&write("wrong-order", &format!("{respawn}{kill}")))
            .expect_err("a respawn before the death leaves the player dead");

        // `damage` is a console step like any other: it is refused with NO
        // restore, and accepted with the `kill` restore that puts health back
        // exactly (the recorder then respawns the dead actor itself). Before
        // 2026-09-30 it demanded a trailing `perform_respawn`, which for a
        // non-lethal hit does nothing and made every damage check `mute`,
        // its measured step being the no-op.
        let e = Scenario::load(&write(
            "damaged",
            "[[step]]\ndo = \"console\"\ntext = \"damage oracle 100\"\n",
        ))
        .expect_err("a damage with no restore leaves the player hurt for every later check");
        assert!(e.to_string().contains("console_restore"), "{e}");
        Scenario::load(&write(
            "damaged-restored",
            "console_restore = [\"kill oracle\"]\n[[step]]\ndo = \"console\"\ntext = \"damage oracle 2\"\n",
        ))
        .expect("a damage restored by a kill loads");

        // A NON-death console step still needs its console_restore — the
        // exemption is for deaths only and must not have widened.
        Scenario::load(&write(
            "border",
            "[[step]]\ndo = \"console\"\ntext = \"worldborder set 137\"\n",
        ))
        .expect_err("a worldborder set still needs a console_restore");

        // And a death in `[[setup]]` counts: setup is unmeasured, not unsent.
        Scenario::load(&write(
            "setup-death",
            "[[setup]]\ndo = \"console\"\ntext = \"kill oracle\"\n[[step]]\ndo = \"wait\"\nms = 1\n",
        ))
        .expect_err("a setup-phase death leaves the player just as dead");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `release_use`'s two silent doors are refused at load, and the shape
    /// that actually fires one is not.
    ///
    /// This is the falsifier for the verb. `RELEASE_USE_ITEM` was in the
    /// ordinal table for a month with no arm sending it; adding the arm
    /// without these refusals would have bought a verb whose commonest
    /// spelling — a bare `release_use`, or a `hold` between the press and the
    /// release — sends a well-formed packet that `LivingEntity.releaseUsingItem`
    /// discards at offset 16 or 27. Both servers would then say nothing and
    /// the check would report `match`. Delete either refusal and this test is
    /// red.
    #[test]
    fn a_release_with_no_use_or_a_changed_hand_is_refused_at_load() {
        let dir = std::env::temp_dir().join(format!("scen-rel-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let head = "about = \"a fixture for the release_use load test\"\n\
                    observe = [\"add_entity\"]\n";
        let bow = "[[step]]\ndo = \"hold\"\nitem = \"minecraft:bow\"\n\
                   [[step]]\ndo = \"use_item\"\n";

        let write = |n: &str, body: String| {
            let p = dir.join(format!("{n}.toml"));
            std::fs::write(&p, format!("name = \"{n}\"\n{head}{body}")).unwrap();
            p
        };

        let good = write(
            "good",
            format!("{bow}[[step]]\ndo = \"wait\"\nms = 1200\n[[step]]\ndo = \"release_use\"\n"),
        );
        let s = Scenario::load(&good).expect("hold, use, wait, release must load");
        assert!(matches!(s.step.last(), Some(Step::ReleaseUse)));

        let bare = write("bare", "[[step]]\ndo = \"release_use\"\n".into());
        let e = Scenario::load(&bare).expect_err("a release with no use must be refused");
        assert!(
            e.to_string().contains("no earlier"),
            "the refusal must say what is missing: {e}"
        );

        let swapped = write(
            "swapped",
            format!("{bow}[[step]]\ndo = \"hold\"\nitem = \"minecraft:stick\"\n[[step]]\ndo = \"release_use\"\n"),
        );
        let e = Scenario::load(&swapped)
            .expect_err("changing the held item between the press and the release must be refused");
        assert!(
            e.to_string().contains("isSameItem"),
            "the refusal must name the guard it is about: {e}"
        );

        let offhand = write(
            "offhand",
            format!("{bow}[[step]]\ndo = \"swap_offhand\"\n[[step]]\ndo = \"release_use\"\n"),
        );
        Scenario::load(&offhand)
            .expect_err("a swap moves the stack out of the used hand and voids the release");

        // A use opened in `[[setup]]` is still open when the measured part
        // starts — setup is unmeasured, not unsent — so this must LOAD.
        let across = write(
            "across",
            "[[setup]]\ndo = \"hold\"\nitem = \"minecraft:bow\"\n\
             [[setup]]\ndo = \"use_item\"\n[[step]]\ndo = \"release_use\"\n"
                .into(),
        );
        Scenario::load(&across).expect(
            "a use opened in setup is still open in step — setup is unmeasured, not unsent",
        );

        // Re-holding the SAME item does not fail `isSameItem`, which compares
        // the Item and not the stack, so this must load too. A refusal here
        // would be the guard measuring its own strictness rather than
        // vanilla's.
        let rehold = write(
            "rehold",
            format!("{bow}[[step]]\ndo = \"hold\"\nitem = \"minecraft:bow\"\n[[step]]\ndo = \"release_use\"\n"),
        );
        Scenario::load(&rehold).expect("re-holding the same item leaves isSameItem true");

        // A use left open at the end of the scenario is refused: it would
        // leave every later scenario in the corpus mid-use, which is 552/553
        // in a second shape.
        let e = Scenario::load(&write("dangling", bow.into()))
            .expect_err("an unreleased use must be refused");
        assert!(
            e.to_string().contains("never released"),
            "the refusal must say what was left open: {e}"
        );

        // A boat's `use` never calls `startUsingItem`, so ending on one is
        // not an open use: this is 776's shape and it must load.
        Scenario::load(&write(
            "boat",
            "[[setup]]\ndo = \"hold\"\nitem = \"minecraft:oak_boat\"\n\
             [[step]]\ndo = \"use_item\"\n"
                .into(),
        ))
        .expect("BoatItem.use opens no use, so there is nothing to release");

        // `swap_offhand` ends a use — `handlePlayerAction` calls
        // `stopUsingItem` at offset 218 — so it closes the scenario honestly
        // and must NOT be refused.
        Scenario::load(&write(
            "swap-closes",
            format!("{bow}[[step]]\ndo = \"swap_offhand\"\n"),
        ))
        .expect("a swap calls stopUsingItem, so it closes an open use");

        // A drop does not: `player.drop(Z)` at offsets 237 and 256 is the
        // whole of those two cases, so the use is still open.
        Scenario::load(&write(
            "drop-does-not",
            format!("{bow}[[step]]\ndo = \"drop\"\n"),
        ))
        .expect_err("a drop does not stop a use, so this still leaks one");

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `ServerboundClientCommandPacket$Action`'s three ordinals, written as
    /// the class file's declaration order rather than as arithmetic, for
    /// `player_action_ordinals_are_the_enums_declaration_order`'s reason.
    ///
    /// The third one is the load-bearing entry. Every account of this packet
    /// older than 26.2 has **two** actions, so a transcription from memory
    /// would have had nothing at 2 — and because the wire value is
    /// `writeEnum`'s bare ordinal, a two-entry table is not a compile error
    /// anywhere, it is a `request_gamerule_values` that arrives as something
    /// else or not at all.
    #[test]
    fn client_command_ordinals_are_the_enums_declaration_order() {
        use client_command as cc;
        assert_eq!(
            [
                cc::PERFORM_RESPAWN,
                cc::REQUEST_STATS,
                cc::REQUEST_GAMERULE_VALUES
            ],
            [0, 1, 2]
        );
        assert_eq!(client_command_action("perform_respawn"), Some(0));
        assert_eq!(client_command_action("request_stats"), Some(1));
        assert_eq!(client_command_action("request_gamerule_values"), Some(2));
    }

    /// An action name this harness does not know is `None`, and `None` is a
    /// refusal at both call sites rather than a default.
    ///
    /// Written as a case list because the failure this guards against is not
    /// a wrong ordinal, it is a *fallback*: `PERFORM_RESPAWN` is ordinal 0, so
    /// any `unwrap_or(0)` anywhere on this path turns every typo into a
    /// respawn, and a respawn on a living player sends nothing at all
    /// (`handleClientCommand` offset 139). The whole family would then read as
    /// agreement.
    #[test]
    fn an_unknown_client_command_action_is_refused_not_defaulted() {
        for name in [
            "",
            "respawn",
            "PERFORM_RESPAWN",
            "perform respawn",
            "0",
            "request_stat",
        ] {
            assert_eq!(
                client_command_action(name),
                None,
                "{name:?} must not resolve — ordinal 0 is PERFORM_RESPAWN, so a default \
                 here turns a typo into a packet that does nothing and reads as a match"
            );
        }
    }

    /// The two decidable halves of `client_command`'s contract, at load:
    /// `perform_respawn` parses, and `request_gamerule_values` is refused.
    ///
    /// The refusal is the falsifier for the arm's own honesty. `oracle` and
    /// `oracle_eye` never hold `COMMANDS_GAMEMASTER`, so
    /// `sendGameRuleValues` returns at offset 39 having sent nothing on
    /// **both** servers; the scenario would then be a comparison of two
    /// silences that reports `match`. Deleting the refusal makes this test
    /// red, which is the property the whole file is written for.
    #[test]
    fn a_gamerule_values_request_is_refused_at_load_and_a_respawn_is_not() {
        let dir = std::env::temp_dir().join(format!("scen-cc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let head = "about = \"a fixture for the client_command load test\"\n\
                    observe = [\"respawn\"]\n";

        let good = dir.join("good.toml");
        std::fs::write(
            &good,
            format!("name = \"good\"\n{head}[[step]]\ndo = \"client_command\"\naction = \"perform_respawn\"\n"),
        )
        .unwrap();
        let s = Scenario::load(&good).expect("perform_respawn must load");
        assert!(matches!(
            s.step.as_slice(),
            [Step::ClientCommand { action }] if action == "perform_respawn"
        ));

        let bad = dir.join("bad.toml");
        std::fs::write(
            &bad,
            format!("name = \"bad\"\n{head}[[step]]\ndo = \"client_command\"\naction = \"request_gamerule_values\"\n"),
        )
        .unwrap();
        let e = Scenario::load(&bad)
            .expect_err("a gamerule-values request must be refused: neither server answers it");
        assert!(
            e.to_string().contains("COMMANDS_GAMEMASTER"),
            "the refusal must name the permission it is about, not just decline: {e}"
        );

        let typo = dir.join("typo.toml");
        std::fs::write(
            &typo,
            format!(
                "name = \"typo\"\n{head}[[step]]\ndo = \"client_command\"\naction = \"respawn\"\n"
            ),
        )
        .unwrap();
        Scenario::load(&typo).expect_err("an unknown action must be refused at load, not sent");

        let setup = dir.join("setup.toml");
        std::fs::write(
            &setup,
            format!("name = \"setup\"\n{head}[[setup]]\ndo = \"client_command\"\naction = \"request_gamerule_values\"\n[[step]]\ndo = \"wait\"\nms = 1\n"),
        )
        .unwrap();
        Scenario::load(&setup).expect_err(
            "a setup-phase request must be refused the same way — setup is unmeasured, not unsent",
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// `deny_unknown_fields` plus a tagged enum means a typo in `do` is a
    /// load error rather than a silently skipped step, and the two new
    /// inventory actions take no required fields — so this is the cheapest
    /// place for them to go wrong unnoticed.
    #[test]
    fn the_inventory_actions_parse() {
        let s: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["add_entity"]
[[step]]
do = "drop"
[[step]]
do = "drop"
all = true
[[step]]
do = "swap_offhand"
"#,
        )
        .expect("parses");
        assert!(matches!(s.step[0], Step::Drop { all: false }));
        assert!(matches!(s.step[1], Step::Drop { all: true }));
        assert!(matches!(s.step[2], Step::SwapOffhand));
    }

    /// The input byte is one field of seven flags, so the only way to get it
    /// wrong is to name the wrong bit — and the wrong bit is a legal packet
    /// that says the player pressed something else. `SHIFT` at 32 is the whole
    /// of what `sneak` sends; the gaps either side are what makes a
    /// transposition visible.
    #[test]
    fn the_input_flags_are_the_seven_distinct_bits_of_one_byte() {
        use super::input_flags as f;
        let all = [
            f::FORWARD,
            f::BACKWARD,
            f::LEFT,
            f::RIGHT,
            f::JUMP,
            f::SHIFT,
            f::SPRINT,
        ];
        // Powers of two, in declaration order, with none repeated: the three
        // ways a hand-transcribed bitmask goes wrong.
        assert_eq!(all, [1, 2, 4, 8, 16, 32, 64]);
        assert_eq!(
            all.iter().fold(0u8, |a, b| a | b),
            127,
            "seven flags must occupy seven distinct bits of one byte"
        );
        // Sneak is not jump. They are adjacent bits in the same byte and this
        // is the transposition that would put a scenario in the air.
        assert_ne!(f::SHIFT, f::JUMP);
    }

    /// `sneak` is a state and its field is required, because "sneak" with no
    /// direction is the reading that leaves a scenario guessing. A default of
    /// `on = true` would also make the recorder's own release step — which
    /// resets the input state before every scenario — unwritable.
    #[test]
    fn sneak_parses_both_ways() {
        let s: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["block_update"]
[[step]]
do = "sneak"
on = true
[[step]]
do = "sneak"
on = false
"#,
        )
        .expect("parses");
        assert!(matches!(s.step[0], Step::Sneak { on: true }));
        assert!(matches!(s.step[1], Step::Sneak { on: false }));
    }

    /// `attack` carries a **required** field, which is the first step variant
    /// with one that has no default, so the two ways it can be got wrong are
    /// both worth a line: a missing `target` must not parse into some default
    /// entity, and a `target` that is not a string must not parse at all.
    ///
    /// The first is the one that matters. A default target — `minecraft:player`
    /// is the obvious candidate — would make `do = "attack"` on its own a
    /// legal step, and a scenario that meant to attack a mob and forgot to say
    /// so would silently attack a player and be recorded as agreeing with
    /// vanilla about a situation neither server was in.
    #[test]
    fn attack_requires_its_target() {
        let ok: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["sound"]
[[step]]
do = "attack"
target = "minecraft:player"
"#,
        )
        .expect("parses");
        assert!(matches!(&ok.step[0], Step::Attack { target } if target == "minecraft:player"));

        let bare = toml::from_str::<Scenario>(
            r#"
name = "x"
about = "y"
observe = ["sound"]
[[step]]
do = "attack"
"#,
        );
        assert!(
            bare.is_err(),
            "`attack` with no target parsed — a defaulted target would send \
             this scenario at whatever entity happened to fit"
        );
    }

    /// `interact` needs its target for `attack`'s reason, and its two optional
    /// fields default to `attack`'s rule and the main hand.
    #[test]
    fn interact_requires_its_target_and_defaults_the_rest() {
        let ok: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["entity_event"]
[[step]]
do = "interact"
target = "minecraft:pig"
[[step]]
do = "interact"
target = "minecraft:pig"
pick = "oldest"
hand = "off"
"#,
        )
        .expect("parses");
        assert!(matches!(&ok.step[0],
            Step::Interact { target, pick, hand }
                if target == "minecraft:pig" && pick == "newest" && hand == "main"));
        assert!(matches!(&ok.step[1],
            Step::Interact { pick, hand, .. } if pick == "oldest" && hand == "off"));
        let bare = toml::from_str::<Scenario>(
            r#"
name = "x"
about = "y"
observe = ["entity_event"]
[[step]]
do = "interact"
"#,
        );
        assert!(bare.is_err(), "`interact` with no target parsed");
    }

    /// The seven `ContainerInput` ids, as the enum's own `id` arguments.
    ///
    /// Written as the literal declaration order rather than as arithmetic, for
    /// `player_action_ordinals_are_the_enums_declaration_order`'s reason. The
    /// stake is higher here: `ByIdMap.continuous(..., OutOfBoundsStrategy.ZERO)`
    /// resolves an unknown id to the **first** entry, so a transcription slip
    /// does not produce a decode error, it produces a `PICKUP`. Every wrong
    /// value in this table is a legal click of some other kind.
    #[test]
    fn container_input_ids_are_the_enums_own_id_arguments() {
        use container_input as ci;
        assert_eq!(
            [
                ci::PICKUP,
                ci::QUICK_MOVE,
                ci::SWAP,
                ci::CLONE,
                ci::THROW,
                ci::QUICK_CRAFT,
                ci::PICKUP_ALL,
            ],
            [0, 1, 2, 3, 4, 5, 6],
        );
        // The two that a storing check and a crafting check actually send.
        // `PICKUP` is zero, which is also what an unrecognised id decodes to,
        // so the pair below is the only thing distinguishing "sent pickup" from
        // "sent nonsense".
        assert_eq!(container_input_value("pickup"), Some(ci::PICKUP));
        assert_eq!(container_input_value("quick_move"), Some(ci::QUICK_MOVE));
        assert_ne!(ci::QUICK_MOVE, ci::PICKUP);
        // A name the enum does not have must not resolve. If this returned
        // `Some(PICKUP)` the harness would send a well-formed click on a typo
        // and both servers would answer it.
        assert_eq!(container_input_value("shift_click"), None);
        assert_eq!(container_input_value("PICKUP"), None);
    }

    /// `container_click`'s `slot` is required and its two optional fields have
    /// the documented defaults.
    ///
    /// The required half is the one that matters, for the reason
    /// `attack_requires_its_target` gives: a defaulted slot makes
    /// `do = "container_click"` a legal step that clicks slot 0, and slot 0 of
    /// a freshly placed chest is empty on both servers — so a scenario that
    /// meant to click something and forgot to say what would record `match`
    /// for a click that moved nothing.
    #[test]
    fn container_click_requires_its_slot() {
        let ok: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["container_set_slot"]
[[step]]
do = "container_click"
slot = 54
[[step]]
do = "container_click"
slot = 1
button = 3
input = "quick_move"
[[step]]
do = "close_container"
"#,
        )
        .expect("parses");
        assert!(matches!(
            &ok.step[0],
            Step::ContainerClick { slot: 54, button: 0, input } if input == "pickup"
        ));
        assert!(matches!(
            &ok.step[1],
            Step::ContainerClick { slot: 1, button: 3, input } if input == "quick_move"
        ));
        assert!(matches!(ok.step[2], Step::CloseContainer));

        let bare = toml::from_str::<Scenario>(
            r#"
name = "x"
about = "y"
observe = ["container_set_slot"]
[[step]]
do = "container_click"
"#,
        );
        assert!(
            bare.is_err(),
            "`container_click` with no slot parsed — a defaulted slot would \
             click slot 0, which is empty, and record agreement"
        );
    }

    /// `sign_update` parses, defaults `front` to the front, and **requires its
    /// position**.
    ///
    /// There is no default position for the reason the whole verb is
    /// dangerous: a `sign_update` naming a place with no `SignBlockEntity` is
    /// dropped by vanilla at offset 56 of `updateSignText` with no reply and
    /// no log, so a defaulted `[0,0,0]` — the block the player is standing in,
    /// which is never a sign — would record silence against silence and report
    /// agreement.
    #[test]
    fn sign_update_needs_a_position_and_defaults_to_the_front() {
        let ok: Scenario = toml::from_str(
            r#"
name = "x"
about = "y"
observe = ["block_entity_data"]
[[step]]
do = "sign_update"
pos = [1, 0, 0]
lines = ["hello"]
[[step]]
do = "sign_update"
pos = [1, 0, 0]
front = false
lines = ["a", "b", "c", "d"]
"#,
        )
        .expect("parses");
        assert!(matches!(
            &ok.step[0],
            Step::SignUpdate { pos: [1, 0, 0], front: true, lines } if lines == &["hello"]
        ));
        assert!(matches!(
            &ok.step[1],
            Step::SignUpdate { front: false, lines, .. } if lines.len() == 4
        ));

        let bare = toml::from_str::<Scenario>(
            r#"
name = "x"
about = "y"
observe = ["block_entity_data"]
[[step]]
do = "sign_update"
"#,
        );
        assert!(
            bare.is_err(),
            "`sign_update` with no position parsed — vanilla drops an edit at a \
             position holding no sign without answering, so the default would \
             have been a scenario that measured nothing and called it a match"
        );
    }

    /// **A `collapse_repeats` entry that cannot bite is refused, and one that
    /// bites only in `strict` is not.**
    ///
    /// The first half is the old rule and the reason for it is unchanged: a
    /// name no packet can carry does nothing for ever while the check's own
    /// file says it does something, which is a stale claim in a tracked file.
    ///
    /// The second half is R1's correction. The collapse now runs on both
    /// denominators, so a kind outside `observe` is live in the strict one —
    /// and that is the normal case, not an edge: not one container check
    /// observes `block_event`, which is the kind whose repeats decide their
    /// strict verdicts.
    #[test]
    fn a_collapse_declaration_that_cannot_bite_is_refused() {
        // Distinct directories by an explicit label, not by anything derived
        // from the content: two bodies that happened to be the same length
        // would have shared a path and the second write would have decided
        // both assertions.
        let write = |label: &str, body: &str| {
            let dir = std::env::temp_dir()
                .join(format!("rustmc-scenario-{}-{label}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("tmpdir");
            let path = dir.join("check.toml");
            std::fs::write(&path, body).expect("write");
            path
        };
        let good = write(
            "observed",
            r#"
name = "x"
about = "y"
observe = ["block_event", "open_screen"]
collapse_repeats = ["block_event"]
[[step]]
do = "swing"
"#,
        );
        assert!(Scenario::load(&good).is_ok());

        let bad = write(
            "typo",
            r#"
name = "x"
about = "y"
observe = ["open_screen"]
collapse_repeats = ["block_evemt"]
[[step]]
do = "swing"
"#,
        );
        let err = Scenario::load(&bad).expect_err("a typo must not be silent");
        assert!(err.to_string().contains("block_evemt"), "{err}");

        // A real kind the check does not observe: legal, because `strict`
        // keeps every kind and that is where the container checks need it.
        let strict_only = write(
            "strict-only",
            r#"
name = "x"
about = "y"
observe = ["open_screen"]
collapse_repeats = ["block_event"]
[[step]]
do = "swing"
"#,
        );
        assert!(
            Scenario::load(&strict_only).is_ok(),
            "560 and its eleven siblings are exactly this shape"
        );
    }

    /// A scenario with no `[[step]]` is refused at load.
    ///
    /// It cannot pass or fail, and it has no measured step for
    /// [`MeasuredStep`] to be about — so `compare` would have to carry a
    /// special case for a file that has never existed. Refusing makes "every
    /// scenario has a measured step" a fact the comparator may rely on.
    #[test]
    fn a_scenario_with_no_measured_part_is_refused() {
        let dir = std::env::temp_dir().join(format!("scen-nostep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x.toml");
        std::fs::write(&p, "name = \"x\"\nabout = \"y\"\nobserve = []\nstep = []\n").unwrap();
        let e = Scenario::load(&p).expect_err("a scenario with no [[step]] must not load");
        assert!(
            e.to_string().contains("no [[step]]"),
            "the refusal must say what is wrong: {e}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The two spellings the corpus uses, and the refusal of a third.
    ///
    /// A mistyped value must not fall back to the default — the default is
    /// "must speak", so a typo in a check that meant `silent` would go red,
    /// which is survivable; but a typo in the other direction would be an
    /// exemption nobody wrote, and `deny_unknown_fields` plus this enum's
    /// closed set is what stops it.
    #[test]
    fn measured_step_parses_exactly_two_names() {
        let head = "name = \"x\"\nabout = \"y\"\nobserve = []\n\
                    [[step]]\ndo = \"dig\"\nat = [1, -1, 0]\nface = \"up\"\n";
        let of = |v: &str| -> Option<MeasuredStep> {
            toml::from_str::<Scenario>(&format!("measured_step = \"{v}\"\n{head}"))
                .ok()
                .map(|s| s.measured_step)
        };
        assert_eq!(of("speaks"), Some(MeasuredStep::Speaks));
        assert_eq!(of("silent"), Some(MeasuredStep::Silent));
        assert_eq!(of("Silent"), None);
        assert_eq!(of("quiet"), None);
        assert_eq!(of(""), None);
    }

    /// A game rule reaches a server console as one line, so nothing in it may
    /// carry a newline — and the refusal has to be at load, where it is a
    /// static property of the file.
    ///
    /// **This is the test that makes the refusal a gate.** `Scenario::load`
    /// is the only place that runs it, so a validator nobody exercises would
    /// pass this project's usual failure mode: it would be there, it would
    /// look right, and it would never have been asked a question it could get
    /// wrong.
    #[test]
    fn a_gamerule_that_could_carry_a_second_command_is_refused() {
        let dir = std::env::temp_dir().join(format!("scen-gr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let head = "name = \"x\"\nabout = \"y\"\nobserve = []\n";
        let tail = "[[step]]\ndo = \"wait\"\nms = 10\n";
        let write = |body: &str| {
            let p = dir.join("g.toml");
            std::fs::write(&p, format!("{head}{body}{tail}")).unwrap();
            Scenario::load(&p)
        };

        // The one the corpus ships.
        let ok = write(
            "[[gamerule]]\nrule = \"random_tick_speed\"\nvalue = \"1000\"\nrestore = \"0\"\n",
        )
        .expect("a well-formed rule must load");
        assert_eq!(ok.gamerule.len(), 1);
        assert_eq!(
            ok.gamerule[0].apply_command(),
            "gamerule random_tick_speed 1000"
        );
        assert_eq!(
            ok.gamerule[0].restore_command(),
            "gamerule random_tick_speed 0"
        );

        // A second console command smuggled through the value. This is the
        // case the whole validator exists for.
        let e = write(
            "[[gamerule]]\nrule = \"random_tick_speed\"\n\
             value = \"\"\"1000\nop oracle\"\"\"\nrestore = \"0\"\n",
        )
        .expect_err("a newline in a value must be refused");
        assert!(e.to_string().contains("[[gamerule]]"), "{e}");

        // A space does the same job as a newline for an argument list.
        write("[[gamerule]]\nrule = \"a b\"\nvalue = \"1\"\nrestore = \"0\"\n")
            .expect_err("a space in a rule id must be refused");
        // Empty is not a rule.
        write("[[gamerule]]\nrule = \"\"\nvalue = \"1\"\nrestore = \"0\"\n")
            .expect_err("an empty rule id must be refused");
        // Two declarations of one rule have no defined restore order.
        write(
            "[[gamerule]]\nrule = \"r\"\nvalue = \"1\"\nrestore = \"0\"\n\
             [[gamerule]]\nrule = \"r\"\nvalue = \"2\"\nrestore = \"0\"\n",
        )
        .expect_err("the same rule twice must be refused");
        // And a scenario that says nothing keeps the harness's substrate.
        assert!(write("").unwrap().gamerule.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
