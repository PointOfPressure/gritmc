//! What a scenario *saw*, recorded so two servers can be compared.
//!
//! # The recording rule
//!
//! **Record at full fidelity; be lenient only when comparing.** A trace
//! keeps exact values, exact order and exact arrival times, and keeps every
//! packet it was not asked about. Leniency is a projection applied later, by
//! [`crate::compare`], from what the scenario declared. The reason is the
//! one this project keeps relearning: a check that was written down loosely
//! can be tightened at any time, but an instrument that threw the data away
//! has to be rebuilt and every earlier result re-run.
//!
//! # How a packet becomes an observation
//!
//! Decode exactly the leading fields that need **normalising** — entity ids,
//! which are arbitrary and differ between servers, and world positions,
//! which depend on where each server put its spawn — and take a digest of
//! everything after them. Positions are normalised here, against the anchor,
//! because the anchor is a property of the recording. Entity ids are only
//! *extracted* here — kept raw, out of the digest, and renumbered by
//! [`crate::compare::project`] over the observations a check actually looks
//! at, which is the one place that knows what the scope is. See
//! [`entity_ref`]. The digest is bit-exact by construction, so the
//! parts nobody has written a decoder for are still compared exactly rather
//! than ignored. That is the opposite of the usual trade: the *undecoded*
//! tail is the strictest part of the comparison, and writing a decoder for
//! it later only makes failures easier to read.
//!
//! # What is deliberately not *compared* — and nothing is left unrecorded
//!
//! This paragraph used to list twelve packet types as "deliberately not
//! recorded", and it was the leniency rule above being broken by the file
//! that states it. They are recorded now. What survives is a *projection*
//! decision, it lives in one declared table — [`FAMILIES`] — and every row
//! carries the measurement that put it there. [`Class::Ambient`] means
//! [`crate::compare::project`] drops the kind unless a scenario names it in
//! its own `observe` list; [`Class::Graded`] means it is compared like
//! anything else.
//!
//! `ci/ambient-falsify.py` is what keeps that honest: it refuses a tree in
//! which a packet is dropped with no row, a row's stated reason is not the
//! mechanism that drops it, or a `Graded` row reaches no projection at all.

use rustmc_proto::ids;
use rustmc_proto::varint::{read_varint, read_varlong};
use rustmc_proto::wire::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Which client heard it.
///
/// # The finding this exists for, and the one it was wrongly built on
///
/// A second, non-acting client is recorded because some of what a server
/// does is only visible from outside the action: an entity appearing, a
/// block change reaching the rest of the world. Both streams are kept,
/// because some things are only ever sent to the actor — a screen opening,
/// its own inventory, the ack itself. A scenario does not choose which; it
/// names the observation kinds it cares about and both streams are searched.
///
/// **What this doc used to say was wrong and is worth keeping written down.**
/// It claimed vanilla does not tell the acting player what changed, on a
/// measurement in which breaking a block produced nothing on the actor's
/// socket and placing one produced an ack and nothing else. The cause was
/// that the harness never acknowledged a chunk batch, so vanilla had stopped
/// sending it chunks after the first nine and
/// `ChunkMap.isChunkTracked` — the only filter on
/// `ChunkHolder.broadcastChanges`'s recipient list — was false wherever the
/// scenarios worked. Re-measured 2026-08-03 with a conforming client, a
/// placement puts **four** things on the actor's socket: two resyncs, the
/// broadcast, then the ack.
///
/// The lesson generalises past this harness: a client that is not being told
/// things looks exactly like a server that is not saying them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    /// The client that performed the steps.
    Actor,
    /// A client standing nearby that did nothing at all.
    Observer,
    /// A third connection, opened and closed inside one scenario, that exists
    /// to be treated badly.
    ///
    /// The actor and the observer are logged in once and replay **every**
    /// scenario, so a check whose whole content is "vanilla disconnects this
    /// client" cannot be written on either of them: the disconnect would
    /// destroy every scenario after it. That is not a small class — the
    /// serverbound census found that several handlers' only observable effect
    /// is a `ClientboundDisconnectPacket`, so before this existed those
    /// packets were unfalsifiable for a reason on our side of the wire rather
    /// than in the protocol.
    ///
    /// A probe is therefore expendable by construction: it joins during the
    /// scenario that wants it, is measured on its own socket, and is dropped
    /// at the end of that scenario whether or not the server hung up first.
    Probe,
}

impl Who {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Actor => "actor",
            Self::Observer => "observer",
            Self::Probe => "probe",
        }
    }
}

/// One thing a server said.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Obs {
    /// Which of the two clients heard it.
    pub who: Who,
    /// Index of the last measured step issued before this arrived. This,
    /// not the clock, is what "when" means by default: it survives two
    /// servers running at different speeds and still says which action
    /// caused what.
    pub step: usize,
    /// Milliseconds since the first measured step. Recorded always,
    /// compared only when a scenario asks — see [`crate::scenario::Timing`].
    pub at_ms: u64,
    /// `block_update`, `add_entity`, or `packet:0x63` for one with no
    /// decoder yet.
    pub kind: String,
    /// The decoded leading fields, plus `rest` — a digest of the bytes after
    /// them.
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// One scenario's run against one server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioTrace {
    pub name: String,
    /// `None` when the scenario ran to completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The block the player was standing in. Everything positional is
    /// relative to it.
    pub anchor: [i32; 3],
    pub spawn: [f64; 3],
    pub obs: Vec<Obs>,
    /// The root commands the *actor's* connection was told the server has.
    ///
    /// A join-time fact reported per scenario, like `anchor` and `spawn`, and
    /// for the same reason: the packet that carries it arrives before any
    /// scenario's measured window opens, so it can never appear in `obs`.
    ///
    /// Empty on a recording taken before this field existed, which is what
    /// `skip_serializing_if` keeps out of those files' diffs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declared_commands: Vec<String>,
    /// Whether this scenario ran on a connection that had just been made.
    ///
    /// # The counters are per-connection, so this is where they reset
    ///
    /// `record::record` drops the party whenever a scenario errors, and the
    /// next scenario logs in fresh. That restarts `Session::seq` at 1 and, on
    /// the server's side, `ServerPlayer.containerCounter` at 0. **The two
    /// sides of a comparison are two independent runs**, so a failure on one
    /// of them resets that side's counters and not the other's, and every
    /// later scenario is then compared against numbering that moved for a
    /// reason having nothing to do with the scenario.
    ///
    /// That is what happened on the `worldgen-iceberg-912` gate of
    /// 2026-08-10 and it cost 38 false divergences.
    /// `compare::rebase_session_counters` makes the *values* survive it, but
    /// rebasing is a repair and not a detector: the next per-connection
    /// quantity somebody decodes will not be in its list, and the failure
    /// would look exactly the same — a wall of divergences naming healthy
    /// checks. So the fact of the reconnect is now recorded, and
    /// `compare::session_path_divergence` says out loud when the two runs did
    /// not take the same path through the corpus.
    ///
    /// `None` means "not recorded" and never "no". Traces written before this
    /// field existed carry it, and an absent measurement must not be read as
    /// a negative one — the comparison is simply skipped for that scenario.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fresh_session: Option<bool>,
}

/// A whole corpus run against one server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Which server this is, in its own words: the version and protocol it
    /// reported at status, and the address it was reached at.
    pub target: String,
    pub version: String,
    pub protocol: i64,
    /// What produced it, so a trace cannot be quoted without saying what
    /// build it came from.
    pub recorder: String,
    pub scenarios: Vec<ScenarioTrace>,
}

/// A 64-bit FNV-1a **variant**, and the word variant is load-bearing.
///
/// The multiplier below is `0x1000_0000_01b3`. FNV-1a's 64-bit prime is
/// `0x100_0000_01b3` — **one hex digit shorter**. The offset basis is the real
/// one. So this is not FNV-1a and never has been, and it said it was until
/// 2026-08-05.
///
/// **The constant is deliberately left alone and only the name is corrected.**
/// Nothing here needs FNV's properties: this is a digest, not a hash table, and
/// its only job is to make two different byte strings differ, which any odd
/// multiplier does. Changing it would invalidate every digest recorded in
/// `docs/`, in `ci/checks/`, and in every saved trace on the Threadripper —
/// a large, purely cosmetic churn with a real chance of orphaning a
/// measurement somebody later wants to reproduce.
///
/// What the wrong name *does* cost is exactly what it cost the day it was
/// found: reproducing a digest outside this tree, to work out which field of a
/// packet had moved under `530-walking-away-drops-a-standing-player-from-view`,
/// gave an answer that disagreed in the top three bytes and agreed in the
/// bottom five — which reads like a subtle bug in the thing being investigated
/// rather than like a mislabelled constant. Anyone doing that again should
/// copy the multiplier from this line and not from a reference.
#[must_use]
pub fn digest(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}

/// Pure liveness: the packets that say nothing about the world at all.
///
/// Distinct from the [`FAMILIES`] classification, and the distinction matters
/// for [`crate::client::Session::settle`], which waits for the world to stop
/// arriving before a measured window opens. Chunk streaming is *not*
/// liveness — a client whose chunks are still landing has not finished
/// arriving anywhere — so treating it as such let a window open too early.
///
/// **It is deliberately not derived from the table and must stay that way.**
/// `settle` asks a question about the socket, not about the comparison: it
/// wants to know whether bytes are still arriving, and that never depended on
/// whether the bytes were also being written down. Wiring it to `Class::Ambient`
/// would silently re-open the window-too-early defect the moment a chunk
/// family was reclassified.
#[must_use]
pub fn is_liveness(id: i32) -> bool {
    use ids::play::cb;
    matches!(id, x if x == cb::KEEP_ALIVE || x == cb::PING || x == cb::SET_TIME)
}

/// How the projection treats a packet family.
///
/// The **whole** classification lives in [`FAMILIES`]. Nothing else in this
/// workspace may drop a packet, and `ci/ambient-falsify.py` refuses a tree in
/// which anything does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Recorded and compared like any other packet: it enters the `strict`
    /// projection, and a scenario may name it in `observe`.
    Graded,
    /// Recorded, and dropped by [`crate::compare::project`] **unless the
    /// scenario names the kind in its own `observe` list**.
    ///
    /// This is leniency at *comparison* time, which is where the module docs
    /// say leniency belongs. It is not a decision to throw the bytes away.
    Ambient,
}

/// One classified packet family, and the measurement that decided its class.
///
/// # Why this is a table and not twelve conditions
///
/// Until 2026-09-06 the same decision was spread over five sites — a
/// module-doc paragraph listing the twelve, `is_housekeeping`'s `matches!`,
/// `observe`'s early `return None`, and two `.filter` lines in
/// `compare::project` naming [`HEARTBEAT`] and [`ENTITY_POSITION`] — and no
/// two of them agreed about what the rule was. `is_housekeeping` dropped
/// packets at **record** time, three stages before the projection that owns
/// leniency, so "did the server send this" was a question the recording could
/// not answer for a fifth of the clientbound protocol. The two `.filter`
/// lines did the same job in the right place and by a different mechanism.
///
/// The `reason` is not decoration. Every row's class is a claim about a
/// measurement — that vanilla emits this on a timer, or that its order is not
/// self-consistent between two runs of vanilla — and a claim with no
/// measurement behind it is the defect this project keeps paying for. The
/// numbers are in `docs/cold-start/recorder-housekeeping.md`.
pub struct Family {
    /// The clientbound id, when the family *is* a packet id. `None` for a
    /// kind this recorder synthesises from a packet it also records under
    /// another kind — [`HEARTBEAT`] is a `move_entity` that carried no
    /// movement, so it has no id of its own.
    pub id: Option<i32>,
    /// The `Obs::kind` this family is recorded under.
    pub kind: &'static str,
    pub class: Class,
    /// **Which measured mechanism decided the class**, from a closed
    /// vocabulary that `ci/ambient-falsify.py` cross-checks against
    /// `ci/checks/ambient-families.toml`:
    ///
    /// | tag | what the evidence file must show |
    /// |---|---|
    /// | `graded` | self-consistent across two vanilla runs, emitted by both servers, and actually reaching a projection |
    /// | `timer` | not self-consistent — it lands inside a window on the window's phase |
    /// | `client-driven` | not self-consistent — its trigger is a bit the client sent |
    /// | `order-not-self-consistent` | not self-consistent — two vanilla runs disagree about its order or its values |
    /// | `no-emitter` | vanilla emitted none of it AND its class has no `new` site anywhere under `net/` in the jar |
    /// | `unexercised` | vanilla emitted none of it over the censused set, but its class HAS a construction site — a statement about the corpus, not about the packet |
    /// | `rare-stray` | at most ONE observation per census, never in both vanilla runs, only at a site pinned in `ci/ambient-falsify.py`'s `RARE_STRAY_SITES`, and a construction site in the jar |
    /// | `not-self-delimiting` | no decoder arm exists, because the wire form cannot be split without a reader this harness does not have |
    ///
    /// A tag is the difference between "we do not compare this" and "we do
    /// not compare this **because**", and the second one can be shown wrong.
    pub tag: &'static str,
    /// Whether [`decode`] has an arm for this family.
    ///
    /// A column rather than a comment because `observe` has to tell three
    /// outcomes apart that would otherwise all read as one: an arm that ran
    /// off the end (`decode = failed`, a harness bug), a family nobody has
    /// written an arm for (`decode = undecoded`, a stated gap), and an
    /// unclassified id with no arm (`packet:0x..`, a mechanic this harness
    /// has never been told about). `families_declare_their_own_arms` puts a
    /// body through `decode` for every row and refuses a stale column.
    pub arm: bool,
    /// What was measured, and what it decided.
    pub reason: &'static str,
}

/// **The classification.** Every packet not named here is [`Class::Graded`].
///
/// A row is a declaration in both directions: an `Ambient` row says the
/// projection drops this kind and why, and a `Graded` row says it does not —
/// which `ci/ambient-falsify.py` checks by asserting the kind reaches a
/// projection at all, because a family "opened" and then compared by nothing
/// is the unfalsifiable-check class wearing a different hat.
pub const FAMILIES: &[Family] = &[
    Family {
        id: Some(ids::play::cb::KEEP_ALIVE),
        kind: "keep_alive",
        class: Class::Ambient,
        tag: "timer",
        arm: false,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 7 observations in one vanilla run and 8 in the other, 16 of 21 scenarios agreeing. `ServerGamePacketListenerImpl.tick` sends one every 15 s off the server clock, so whether one lands inside a measured window is a function of the window's phase and not of anything the scenario did.",
    },
    Family {
        id: Some(ids::play::cb::PING),
        kind: "ping",
        class: Class::Ambient,
        tag: "no-emitter",
        arm: false,
        reason: "registered in `GameProtocols` and constructed nowhere under `net/` — ZERO `new` sites across 7,445 classes (`ci/packet-emitter-sites.py`) and zero observations in the census. Vanilla cannot emit it, so a check on it could only ever be vacuous.",
    },
    Family {
        id: Some(ids::play::cb::SET_TIME),
        kind: "set_time",
        class: Class::Ambient,
        tag: "timer",
        arm: false,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 115 observations in one vanilla run and 122 in the other, and 0 of 21 scenarios agree once the step is included. `ServerLevel.tickTime` broadcasts on a 20-tick timer, so whether one lands inside a measured window, and after which step, is the window's phase.",
    },
    Family {
        id: Some(ids::play::cb::CHUNK_BATCH_START),
        kind: "chunk_batch_start",
        class: Class::Ambient,
        tag: "order-not-self-consistent",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 36 observations in one vanilla run and 38 in the other, 19 of 21 scenarios agreeing. `PlayerChunkSender.batchQuota` is driven by the client's own reported milliseconds-per-chunk, so how many batches a window is split into is a property of the harness's scheduling. A check on it would flap twice in twenty-one.",
    },
    Family {
        id: Some(ids::play::cb::CHUNK_BATCH_FINISHED),
        kind: "chunk_batch_finished",
        class: Class::Ambient,
        tag: "order-not-self-consistent",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): the same 36-against-38 and the same 19 of 21 as `chunk_batch_start`, for the same reason: the batch SIZE is `floor(batchQuota)` and the quota is the client's reported milliseconds-per-chunk. The value is the thing the mechanism makes unstable.",
    },
    Family {
        id: Some(ids::play::cb::FORGET_LEVEL_CHUNK),
        kind: "forget_level_chunk",
        class: Class::Graded,
        tag: "graded",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 253 observations in each vanilla run against 207 from this server, over 5 scenarios, and 21 of 21 agree on values AND on the step. It is `ChunkTrackingView.difference` over two views that are pure functions of position, so both the set and the order are determined and a drop is not queued behind a send. The 253-against-207 gap is the Chebyshev square this server walks against vanilla's padded disc, and it is what `ci/checks/gameplay/547-*` exists to measure.",
    },
    Family {
        id: Some(ids::play::cb::LIGHT_UPDATE),
        kind: "light_update",
        class: Class::Ambient,
        tag: "unexercised",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): vanilla emitted ZERO of it, and `ClientboundLightUpdatePacket` has a construction site (`ChunkHolder`), so this is a statement about the corpus and not about the packet: nothing in it changes a light section outside a chunk body. It has a decoder arm and is nameable; it is not gradeable until a scenario provokes one.",
    },
    Family {
        id: Some(ids::play::cb::SET_CHUNK_CACHE_CENTER),
        kind: "set_chunk_cache_center",
        class: Class::Graded,
        tag: "graded",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 23 observations in each vanilla run and 23 from this server, over 5 scenarios, and 21 of 21 agree on values AND on the step. `ChunkMap.applyChunkTrackingView` offsets 43-56 gate the send on the centre CHUNK having changed and the centre is the player's own chunk, so nothing about it depends on view distance, on which chunks have generated, or on the client's acknowledgement rate. `ci/checks/gameplay/546-*` names it.",
    },
    Family {
        id: Some(ids::play::cb::PLAYER_INFO_UPDATE),
        kind: "player_info_update",
        class: Class::Ambient,
        tag: "client-driven",
        arm: false,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 4 observations in one vanilla run and 8 in the other, 16 of 21 scenarios agreeing, and ZERO from this server. It carries a latency EWMA sampled off the client's own round-trip, and two of its eight action arms (`UPDATE_DISPLAY_NAME`, `INITIALIZE_CHAT`) are not self-delimiting, so the packet cannot be split.",
    },
    Family {
        id: Some(ids::play::cb::TAB_LIST),
        kind: "tab_list",
        class: Class::Ambient,
        tag: "no-emitter",
        arm: false,
        reason: "the other of the only two clientbound classes in this classification with ZERO `new` sites under `net/` (`ci/packet-emitter-sites.py`), and zero observations in the census. Same standing as `ping`.",
    },
    Family {
        id: Some(ids::play::cb::UPDATE_ADVANCEMENTS),
        kind: "update_advancements",
        class: Class::Ambient,
        tag: "rare-stray",
        arm: true,
        reason: "Four censuses of the 20-21 `5*` scenarios, two independent vanilla runs each (`ci/checks/ambient-families.toml`): 1 / 0 at 6dd091ab9, 0 / 0 at 9b89855ac, 0 / 1 at 90f2a3ed4, 0 / 0 at 3e65f5e87 — two observations in eight vanilla runs, never both runs of one census, and both in the FIRST scenario of a fresh world (`500-dropping-an-item-throws-it-on-the-floor`, step 0, at 2027 and 2090 ms). ZERO from this server. `PlayerAdvancements.flushDirty` runs per tick and sends when the set is dirty, so the whole tree goes out at join, before any measured window, and a late flush into the first window is the stray. It was `timer` on the censuses that caught one and `unexercised` on those that did not, and the row flipped T0 on about half the re-takes; `rare-stray` is exact instead and goes red if it appears twice, in both runs, or anywhere but that site. It cannot be graded: the body is undecoded and carries each criterion's `obtained` `Instant` (`CriterionProgress.serializeToNetwork` offsets 2-10), which two runs can never agree on. `advancements` (train-b) gave it a `decode` arm — `reset`, the `added` count and the body length, the rest consumed on purpose — which is what a scenario that names it in `observe` grades; the class stays `Ambient` because the census above is about vanilla, and nothing about it moved.",
    },
    Family {
        id: None,
        kind: CHUNK_NO_BLOCK_ENTITIES,
        class: Class::Graded,
        tag: "graded",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 253 observations in each vanilla run against 207 from this server, over 5 scenarios, and 21 of 21 agree on values AND on the step; `548`, the scenario that names it, contributes 22 of each run's 253. The 9b89855ac census, before `548` existed, read the same 20-of-20 at 231 / 231 / 189. Main dropped it at record time as chunk streaming — which batch lands inside the window — and that does not hold here: the batch packets flap (`chunk_batch_start` 19 of 21 on the step) because `PlayerChunkSender.batchQuota` splits the same chunks differently, while the chunks and the step they land after do not move. Every crossing sends 11 on vanilla and 9 here, the padded disc against this server's Chebyshev square; `ci/checks/gameplay/548-*` names it.",
    },
    Family {
        id: None,
        kind: HEARTBEAT,
        class: Class::Ambient,
        tag: "timer",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 50 observations in one vanilla run and 47 in the other, and only 1 of 21 scenarios agrees once the step is included. `ServerEntity.sendChanges` re-broadcasts every tracked entity on `tickCount % 60 == 0`, so it lands inside a window on a phase coin flip. This reproduces the 2026-08-04 finding that put its filter line in `compare.rs` — the least unstable nine checks at exactly 5 of 11, and vanilla reproducing itself under strict on 0 of 55 — from a different direction.",
    },
    Family {
        id: None,
        kind: ENTITY_POSITION,
        class: Class::Ambient,
        tag: "client-driven",
        arm: true,
        reason: "Measured over 21 scenarios, two independent vanilla runs (`ci/checks/ambient-families.toml`, census at 3e65f5e87): 33 observations in one vanilla run and 31 in the other, 13 of 21 scenarios agreeing. `ServerEntity.sendChanges` offsets 680-691 fire it on `wasOnGround != onGround()`, a bit the client sends, and the reference and the subject are two separate client sessions whose on-ground bit and tick phase are independent draws.",
    },
];

/// The row for a clientbound id, if that id is classified at all.
#[must_use]
pub fn family_of_id(id: i32) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.id == Some(id))
}

/// The row for a recorded kind, if that kind is classified at all.
#[must_use]
pub fn family_of_kind(kind: &str) -> Option<&'static Family> {
    FAMILIES.iter().find(|f| f.kind == kind)
}

/// **The one predicate the projection may drop on.**
///
/// `compare::project` calls this and nothing else; a `.filter` naming a kind
/// literal anywhere else in this workspace is what `ci/ambient-falsify.py`
/// refuses.
#[must_use]
pub fn is_ambient_kind(kind: &str) -> bool {
    family_of_kind(kind).is_some_and(|f| f.class == Class::Ambient)
}

/// `BlockPos.asLong` — x:26 | z:26 | y:12, signed on all three. Derived in
/// `BlockPos`'s static initialiser from the world's size, and the same
/// packing every position on the wire uses.
#[must_use]
pub fn unpack_pos(v: i64) -> [i32; 3] {
    let sext = |n: i64, bits: u32| -> i32 {
        let m = n & ((1i64 << bits) - 1);
        let s = if m >> (bits - 1) != 0 {
            m - (1i64 << bits)
        } else {
            m
        };
        i32::try_from(s).unwrap_or(0)
    };
    [sext(v >> 38, 26), sext(v, 12), sext(v >> 12, 26)]
}

/// The reserved key that marks a decoded field as a **raw entity id awaiting
/// renumbering**, rather than a number to be compared as it stands.
///
/// A recorded trace keeps the server's own id. The comparable index is
/// produced later, by [`crate::compare::project`], over the observations a
/// check actually looks at. The marker is what makes that possible without
/// `compare` having to know which field of which packet kind holds an entity:
/// any value of the form `{"$entity": <raw id>}`, at any depth, is one, so a
/// decoder arm added later participates by construction instead of by being
/// remembered.
pub const ENTITY_REF: &str = "$entity";

/// Record a raw entity id for renumbering at projection time.
///
/// # Why the id cannot be compared raw, and why it is not numbered here
///
/// Raw ids are allocated by a counter neither server has any reason to agree
/// about — `ServerLevel.ENTITY_COUNTER` is a **static** `AtomicInteger` and
/// `getNextEntityId` loops `incrementAndGet` until `hasEntityWithId` is false,
/// so the number depends on every entity the process has ever spawned, in
/// every dimension, and is handed out again once one is removed. It has to be
/// replaced by the order it was seen in, which preserves the thing a check
/// cares about — that the item taken is the item that was dropped — and
/// discards the number it must not depend on.
///
/// **Where that renumbering happens is what this project got wrong, and it
/// was a live defect for as long as the harness existed.** It used to run in
/// `decode`, on every non-housekeeping packet, while a scenario's `observe`
/// list is not applied until [`crate::compare::project`]. So a packet the
/// check filters away had already consumed an index, and one extra unobserved
/// entity packet from either server shifted every later index by one — the
/// check then diverged on the one axis its `observe` list exists to say it
/// does not watch.
///
/// It sprang on 2026-08-03, vanilla against vanilla, on
/// `sand-falls-when-unsupported` at 1 of 11 comparisons: same position, same
/// `kind_id` 51, byte-identical digest of the rest of the packet, `entity` 0
/// in one run and 1 in the other. The stray index belonged to a `move_entity`
/// for the other player that the check does not observe and that arrived
/// before the falling block in one run only.
///
/// Numbering at projection time closes a second hole in the same stroke.
/// `record::pump_both` alternates the two sessions in 25 ms slices through one
/// shared `Normalizer`, so a decode-time index was also a function of which
/// socket the harness happened to read first — forty lines above the
/// `sort_by_key(|o| o.who)` in `record::run_one` that discards socket
/// interleaving as harness noise, on the ground that it is a property of the
/// scheduling and not of the server. The sort took the interleaving out of the
/// *order* while the numbering had already put it into the *value*. Projection
/// runs on the sorted, filtered sequence, so it is now out of both.
#[must_use]
pub fn entity_ref(id: i32) -> serde_json::Value {
    serde_json::json!({ ENTITY_REF: id })
}

/// Turns positions into ones that can be compared.
///
/// Every position on the wire is absolute, and the two servers do not put
/// their spawn in the same place — vanilla picks a superflat spawn that moves
/// from world to world. So each is recorded relative to the anchor, the block
/// the player was standing in when the window opened.
///
/// It used to number entities as well; see [`entity_ref`] for what that cost
/// and why the numbering now happens in [`crate::compare::project`].
#[derive(Default)]
pub struct Normalizer {
    anchor: [i32; 3],
}

impl Normalizer {
    #[must_use]
    pub fn new(anchor: [i32; 3]) -> Self {
        Self { anchor }
    }

    /// A **chunk** coordinate, relative to the anchor's own chunk.
    ///
    /// The chunk-view family carries chunk coordinates, not block ones, and
    /// subtracting the anchor's block position from a chunk index would be
    /// wrong by up to fifteen sixteenths of a chunk in each axis. `>> 4` is
    /// an arithmetic shift, which is floor division on a negative index and
    /// is what `ChunkPos.<init>(BlockPos)` does (`getX() >> 4`).
    fn rel_chunk(&self, cx: i32, cz: i32) -> (i32, i32) {
        (cx - (self.anchor[0] >> 4), cz - (self.anchor[2] >> 4))
    }

    fn rel(&self, pos: [i32; 3]) -> [i32; 3] {
        [
            pos[0] - self.anchor[0],
            pos[1] - self.anchor[1],
            pos[2] - self.anchor[2],
        ]
    }

    fn rel_f(&self, pos: [f64; 3]) -> [f64; 3] {
        [
            pos[0] - f64::from(self.anchor[0]),
            pos[1] - f64::from(self.anchor[1]),
            pos[2] - f64::from(self.anchor[2]),
        ]
    }
}

fn num(v: impl Into<serde_json::Value>) -> serde_json::Value {
    v.into()
}

fn pos_value(p: [i32; 3]) -> serde_json::Value {
    serde_json::json!([p[0], p[1], p[2]])
}

/// Every kind name a decoder **arm** produces.
///
/// **Not every kind `decode` can produce**, and the difference is deliberate.
/// Since 2026-09-06 `decode`'s catch-all also names the [`FAMILIES`] rows that
/// have no arm — `keep_alive`, `ping`, `set_time`, `level_chunk_with_light`,
/// `player_info_update`, `tab_list`, `update_advancements` — and those are
/// left out of this list on purpose: `ci/decoded-kind-reach.py` treats a name
/// here that no recording contains as a **dead decoder arm** and ratchets the
/// count, and `ping` and `tab_list` have no construction site anywhere in the
/// jar (`ci/packet-emitter-sites.py`), so they would sit in that ratchet for
/// ever describing a decoder nobody wrote.
///
/// It exists for one caller — [`crate::scenario::Scenario::load`], which has to
/// tell a `collapse_repeats` typo from a deliberate declaration about a kind
/// the check does not `observe` — and it is a hand-kept list, which is worth
/// being honest about. **The staleness failure is the loud one**: a decoder arm
/// added without a line here makes a check naming that kind refuse to load,
/// with the name in the message. The reverse — a name here that no arm
/// produces — costs a `collapse_repeats` entry that quietly does nothing, which
/// is why `a_collapse_declaration_that_cannot_bite_is_refused` pins the shape
/// rather than trusting the list to stay true on its own.
///
/// `packet:0x..` is deliberately **not** admitted. An undecoded packet's whole
/// content is a digest, so a declaration that its repeats do not matter is a
/// declaration about bytes nobody has read; the honest move is to write the
/// decoder arm, which is what R2 did for `rotate_head`.
pub const DECODED_KINDS: &[&str] = &[
    "add_entity",
    "animate",
    "block_changed_ack",
    "block_destruction",
    "block_entity_data",
    "block_event",
    "block_update",
    "boss_event",
    "change_difficulty",
    "chat",
    "chunk_batch_finished",
    "chunk_batch_start",
    "chunk_block_entities",
    "chunk_no_block_entities",
    "clear_dialog",
    "clear_titles",
    "commands",
    "container_set_content",
    "container_set_data",
    "container_set_slot",
    "damage_event",
    "disconnect",
    "entity_event",
    "entity_position",
    "forget_level_chunk",
    "game_event",
    "initialize_border",
    "level_event",
    "level_particles",
    "light_update",
    "move_entity",
    "move_entity_heartbeat",
    "move_entity_rot",
    "open_screen",
    "open_sign_editor",
    "player_abilities",
    "player_position",
    "player_rotation",
    "remove_entities",
    "rotate_head",
    "section_blocks_update",
    "set_action_bar_text",
    "set_border_center",
    "set_border_lerp_size",
    "set_border_size",
    "set_border_warning_delay",
    "set_border_warning_distance",
    "set_chunk_cache_center",
    "set_cursor_item",
    "set_default_spawn_position",
    "set_display_objective",
    "set_entity_data",
    "set_entity_link",
    "set_entity_motion",
    "set_equipment",
    "set_experience",
    "set_health",
    "set_objective",
    "set_player_inventory",
    "set_subtitle_text",
    "set_title_text",
    "set_titles_animation",
    "sound",
    "stop_sound",
    "take_item_entity",
    "update_advancements",
    "update_attributes",
    "waypoint",
];

/// The kind a `move_entity` gets when it carries no movement.
///
/// It is a separate kind rather than a flag because the projection has to be
/// able to drop it by name, and because a trace that says `move_entity` for a
/// packet that reports no movement is lying to whoever reads it.
pub const HEARTBEAT: &str = "move_entity_heartbeat";

/// The block-entity section of `ClientboundLevelChunkWithLightPacket`.
///
/// Named as a constant so [`observe`] can drop the empty case by identity
/// rather than by string, and so a check that wants chunk-borne block
/// entities names the same string this file produces.
pub const CHUNK_BLOCK_ENTITIES: &str = "chunk_block_entities";

/// A `ClientboundLevelChunkWithLightPacket` whose block-entity list is empty.
///
/// Main dropped this case at **record** time (`observe` returned `None`),
/// because one observation per chunk entering view was argued to be chunk
/// streaming — the harness's scheduling against the measured window — and
/// not either server's behaviour. This branch refuses the place that was
/// applied, and the census refuses the argument: over the twenty `5*`
/// scenarios two vanilla runs agree on this kind's count AND step in all
/// twenty (231 each; 189 from this server). So [`FAMILIES`] grades it, and
/// `ci/checks/gameplay/548-*` is the scenario that names it.
pub const CHUNK_NO_BLOCK_ENTITIES: &str = "chunk_no_block_entities";

/// `ClientboundEntityPositionSyncPacket`, named so the projection can drop it
/// by name the way it drops [`HEARTBEAT`].
///
/// # It is a resync about a client, driven by a bit the client sent
///
/// `ServerEntity.sendChanges` offsets 648-694 emit this packet when any of
/// five things holds, and the fifth is `wasOnGround != entity.onGround()`
/// (offsets 680-691). `onGround` is not server state that a scenario drives —
/// it arrives in the player's own movement packets. See
/// [`crate::compare::project`] for why that makes a stray one evidence about
/// the harness rather than about either server.
pub const ENTITY_POSITION: &str = "entity_position";

/// `ServerEntity.sendChanges`'s own movement test, in wire units.
///
/// # Derivation, from bytecode and not from a round number
///
/// `sendChanges` computes
///
/// ```text
///   moved   = positionCodec.delta(trackingPosition()).lengthSqr() >= 7.62939453125E-6
///   sendPos = moved || tickCount % 60 == 0
/// ```
///
/// (`ldc2_w double 7.62939453125E-6d; dcmpl; iflt` — so the test is `>=`, and
/// the constant is exactly 2⁻¹⁷.) `sendPos` alone decides whether a `Pos` or
/// `PosRot` packet is built at all, and the deltas it carries are
/// `VecDeltaCodec.encodeX/Y/Z`, which are the position in **1/4096 of a
/// block** truncated to `short`.
///
/// So vanilla's own test, expressed in the units that actually reach the wire,
/// is `dx² + dy² + dz² >= 2⁻¹⁷ · 4096² = 2⁻¹⁷ · 2²⁴ = 2⁷ = 128`. Exact in
/// integers, no floating point, no tolerance chosen by us.
///
/// # What that makes a sub-threshold packet
///
/// **A `move_entity` below this cleared no movement test.** The only other way
/// to reach the packet is `tickCount % 60 == 0`, so such a packet exists
/// *because a counter wrapped*, and it says so about an entity that vanilla
/// itself has just decided did not move. Two servers agreeing about movement
/// still disagree about it whenever the 3 s counter lands on a different side
/// of a window boundary — which is every window shorter than 3 s, which is
/// every window in this corpus.
///
/// # The margin, so nobody has to trust that this is safe
///
/// The boundary is a sphere of radius √128 = 11.31 wire units — **0.00276 of
/// a block**. One walk hop in this corpus is half a block, 2048 wire units:
/// 181× outside it. Real movement does not come near this, and cannot, because
/// vanilla would not have sent the packet.
///
/// # The one place this is not exactly vanilla's test, stated rather than left
///
/// `moved` is computed on the **unrounded** `Vec3` delta, and what reaches the
/// wire is that delta quantised to whole 1/4096ths. The two can therefore
/// disagree inside a band of about one quantisation step per axis — roughly
/// `dsq_wire` in `[103, 128)` — where vanilla decided the entity moved and the
/// truncated deltas say it did not. This exempts those.
///
/// It is worth being precise about what that costs, because "close enough" is
/// not a phrase this project accepts. The band corresponds to movement of
/// about **0.0025 of a block in one tick**, 0.05 blocks a second. It cannot
/// hide a movement defect anybody could see, and it cannot make two servers
/// *disagree*, because the predicate is applied identically to both sides of
/// every comparison — the failure mode it admits is a missed detection, never
/// a false one. What it would hide is a server that sent no packet at all
/// where vanilla sent one carrying a 2.5-thousandth-of-a-block delta.
///
/// The alternative is to reconstruct the pre-quantisation delta, which is not
/// recoverable from the wire: the packet carries the truncated value and the
/// codec base is server state the harness never sees. So this is the exact
/// test the wire can express, and the residue is named here rather than
/// rounded away.
const MOVED_WIRE_SQ: i64 = 128;

/// `RUSTMC_DUMP_PACKET=<kind>[,<kind>...]` — print a decoded packet's bytes as
/// hex, to stderr. `RUSTMC_DUMP_PACKET=all` dumps everything.
///
/// # A digest says two runs differ; it cannot say where
///
/// Every decoded observation carries `rest`, an FNV digest of the bytes the
/// decoder did not consume. That is what makes the comparison exact without
/// needing a decoder for every field — and it is also what makes a failure
/// unreadable, because the report can only ever say `vanilla
/// ec94da22c1882da2, ours 449516d9252be94f`. Without this the only route from
/// a digest to a cause is to guess a field and pay for a run per guess.
///
/// It began as `RUSTMC_DUMP_CHAT` on 2026-08-05 and paid for itself the same
/// hour: `chat`'s arm consumes zero bytes, so `rest` is the whole body, and
/// the dump showed that `commands.list.players` takes **three** translation
/// arguments in 26.2 rather than the two `ListPlayersCommand` reads like. It
/// was generalised within the day, when `add_entity`'s tail — a low-precision
/// movement vector, then three rotation bytes and `data`, none of them named
/// by the decoder — moved under
/// `530-walking-away-drops-a-standing-player-from-view` and the same question
/// came round again about a different packet.
///
/// **That sentence said "three rotation bytes, `data`, and three velocity
/// shorts" until 2026-08-05 and was wrong in both the order and the encoding.**
/// `javap -c` on `ClientboundAddEntityPacket.write` puts the movement
/// *before* the rotations and encodes it with `Vec3.LP_STREAM_CODEC`, not as
/// shorts. It is recorded here because the false version was believed twice —
/// once in this file and once in `ci/checks/gameplay-unstable.toml` — and
/// because a wrong byte order in a comment is indistinguishable from a right
/// one until somebody reproduces a digest by hand.
///
/// Env-gated and off by default, like every other instrument here. Run it
/// through `ci/t2-gameplay-subset.sh`, which records the same scenario against
/// real vanilla and then against this server, so the two servers' dumps appear
/// in order and can be diffed directly. **`bytes` is the whole body**, not the
/// undigested tail: the offset the decoder stopped at is printed beside it, so
/// both halves are recoverable and neither has to be reconstructed.
fn dump_packet(kind: &str, body: &[u8], consumed: usize) {
    use std::sync::OnceLock;
    static WANTED: OnceLock<Option<Vec<String>>> = OnceLock::new();
    let wanted = WANTED.get_or_init(|| {
        std::env::var("RUSTMC_DUMP_PACKET")
            .ok()
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
    });
    let Some(wanted) = wanted else { return };
    if !wanted.iter().any(|w| w == "all" || w == kind) {
        return;
    }
    let hex: String = body.iter().map(|b| format!("{b:02x}")).collect();
    eprintln!(
        "packet {kind} {} bytes, decoder consumed {consumed}, rest={}: {hex}",
        body.len(),
        digest(&body[consumed.min(body.len())..])
    );
}

/// Printable ASCII runs of three characters or more, joined by `|`.
///
/// Chat and titles travel as NBT components and there is no NBT decoder in
/// this harness yet. This is recorded **beside** a digest of the whole body,
/// never instead of it: the digest is what the strict comparison uses, and
/// this is so a human reading a failure can see which message it was.
fn ascii_runs(body: &[u8]) -> String {
    let mut out = Vec::new();
    let mut run = String::new();
    for &b in body {
        if (0x20..0x7f).contains(&b) {
            run.push(b as char);
        } else {
            if run.len() >= 3 {
                out.push(std::mem::take(&mut run));
            } else {
                run.clear();
            }
        }
    }
    if run.len() >= 3 {
        out.push(run);
    }
    out.join("|")
}

/// The root command names in a `ClientboundCommandsPacket`, and where it stopped.
///
/// Returns the roots in the server's own registration order, a reason if the
/// packet could not be read that far, and how many bytes were consumed.
///
/// # One layer, deliberately
///
/// A node body ends, for an argument node, with that parser's own properties
/// — one of ~50 `ArgumentTypeInfo` shapes — so without those readers the
/// cursor cannot advance past the first argument node. This does not try.
///
/// It does not need to. `ClientboundCommandsPacket.enumerateNodes` is a
/// breadth-first walk from an empty map: the root is polled first and takes
/// index 0, then its children are enqueued whole and take 1..=count before
/// any grandchild is numbered. Every root command is a literal. So the roots
/// are exactly the nodes immediately following the root node, they cost a
/// flags byte, a child array, an optional redirect index and a string each,
/// and they are reachable without decoding one argument.
///
/// The redirect index is not optional to read even though nothing in this
/// repo's own tree has one: `tell`, `w` and `tm` are redirect nodes on a real
/// server, and skipping the index desynchronises everything after them.
/// Deleting that read makes this find five roots instead of ten, which is the
/// falsification `tests::the_command_arm_reads_a_real_tree_and_declines_the_rest`
/// runs on purpose.
pub fn command_roots(body: &[u8]) -> (Vec<String>, Option<&'static str>, usize) {
    let mut r = body;
    let stop = |r: &[u8]| body.len() - r.len();
    let Ok(total) = read_varint(&mut r) else {
        return (Vec::new(), Some("truncated before the node count"), stop(r));
    };
    let (Ok(root_flags), Ok(kids)) = (read_u8(&mut r), read_varint(&mut r)) else {
        return (Vec::new(), Some("truncated in the root node"), stop(r));
    };
    for _ in 0..kids {
        if read_varint(&mut r).is_err() {
            return (
                Vec::new(),
                Some("truncated in the root's child array"),
                stop(r),
            );
        }
    }
    if root_flags & 3 != 0 {
        return (Vec::new(), Some("node 0 is not the root"), stop(r));
    }
    // Both counts are read from bytes a server sent, so neither may be trusted
    // to be sane. A negative varint would make `0..kids` an *empty* range and
    // this would report zero roots with no refusal -- a malformed packet
    // silently indistinguishable from a server that registered nothing. And
    // `total - 1` on `i32::MIN` overflows, which is a panic in a debug build,
    // in a decoder, on input from the far end of a socket.
    if total < 0 || kids < 0 {
        return (Vec::new(), Some("a negative node count"), stop(r));
    }
    if kids > total.saturating_sub(1) {
        return (
            Vec::new(),
            Some("the root claims more children than the packet has nodes"),
            stop(r),
        );
    }
    let mut roots = Vec::new();
    for _ in 0..kids {
        let (Ok(flags), Ok(n)) = (read_u8(&mut r), read_varint(&mut r)) else {
            return (roots, Some("truncated in a root child"), stop(r));
        };
        if n < 0 {
            return (roots, Some("a negative child count"), stop(r));
        }
        for _ in 0..n {
            if read_varint(&mut r).is_err() {
                return (roots, Some("truncated in a child array"), stop(r));
            }
        }
        if flags & 8 != 0 && read_varint(&mut r).is_err() {
            return (roots, Some("truncated in a redirect index"), stop(r));
        }
        if flags & 3 != 1 {
            return (roots, Some("an argument node at root depth"), stop(r));
        }
        match read_string(&mut r) {
            Ok(name) => roots.push(name),
            Err(_) => return (roots, Some("truncated in a root name"), stop(r)),
        }
    }
    (roots, None, stop(r))
}

/// Decode one clientbound play packet into an observation.
///
/// **Every clientbound play packet produces an observation.** Packets with no
/// decoder become `packet:0x..` with a digest, which is what makes "vanilla
/// said something here and we said nothing" visible for a mechanic nobody has
/// taught this harness about yet; a [`FAMILIES`] row with `arm: false` becomes
/// its declared kind with the same whole-body digest.
///
/// # It used to return `None` for twelve of them and that was the defect
///
/// `is_housekeeping` dropped a fifth of the clientbound protocol here, at
/// **record** time, and the module docs one screen above say leniency belongs
/// at comparison time. What that cost is not a softer comparison — every
/// declared verdict was identical either way — it is that the *recording* had
/// no answer to "did vanilla send this", so two branches whose whole subject
/// is the chunk stream could not write a scenario at all. The exclusion is
/// now [`crate::compare::project`]'s, driven by [`FAMILIES`], and a scenario
/// that names one of those kinds in `observe` gets it.
///
/// `states` maps a global block-state id to its state string, so a failure
/// reads `minecraft:oak_stairs[facing=east,...]` instead of `19334`.
/// `items` is the same for the item registry, and it is not only cosmetic:
/// [`read_item_stack`] refuses an id it does not hold, which is where vanilla
/// throws, so this table is what makes the stack decoder able to fail.
pub fn observe(
    norm: &mut Normalizer,
    states: &BTreeMap<i32, String>,
    items: &BTreeMap<i32, String>,
    id: i32,
    body: &[u8],
) -> Option<(String, BTreeMap<String, serde_json::Value>)> {
    if let Some((kind, mut f, consumed)) = decode(norm, states, items, id, body) {
        // **A chunk that carries no block entities is recorded under its
        // own kind.**
        //
        // Not a failed decode: the payload was read and found to hold none.
        // Its class is [`CHUNK_NO_BLOCK_ENTITIES`]'s `FAMILIES` row, decided
        // by the census and applied at projection time like every other one;
        // the argument is on the `LEVEL_CHUNK_WITH_LIGHT` arm of [`decode`].
        let kind = if kind == CHUNK_BLOCK_ENTITIES
            && f.get("block_entities")
                .and_then(serde_json::Value::as_array)
                .is_none_or(Vec::is_empty)
        {
            CHUNK_NO_BLOCK_ENTITIES.to_owned()
        } else {
            kind
        };
        let tail = &body[consumed.min(body.len())..];
        raw_dump(&kind, tail);
        f.insert("rest".into(), num(digest(tail)));
        return Some((kind, f));
    }
    // A decoder that ran off the end must never make a packet **vanish**.
    // Dropping it would turn a bug in this harness into "the server said
    // nothing", which is the single conclusion the whole design exists to
    // protect — and it did: vanilla's own `add_entity` went unrecorded on
    // 2026-08-02 and a mob that had demonstrably spawned looked like a
    // mob that had not.
    let mut f = BTreeMap::new();
    // Only an arm that ran off the end reaches here: `decode`'s catch-all
    // names every other packet, classified or not. So `decode = failed` in a
    // recording always means a decoder read past its body — a harness bug —
    // and never "nobody has written this one yet", which is `undecoded`.
    f.insert("decode".into(), num("failed"));
    let kind = match family_of_id(id) {
        Some(fam) => fam.kind.to_owned(),
        None => format!("packet:0x{id:02X}"),
    };
    raw_dump(&kind, body);
    f.insert("rest".into(), num(digest(body)));
    Some((kind, f))
}

/// [`observe`], with a heartbeat that carried a rotation recorded as both.
///
/// # A rotation must not depend on the heartbeat's phase
///
/// `ServerEntity.sendChanges` picks the packet from two booleans: `pos`
/// (moved, **or** `tickCount % 60 == 0`) and `rot` (the packed yaw or pitch
/// moved by at least one step). `pos && rot` is `MoveEntityPacket.PosRot`,
/// `rot` alone is `MoveEntityPacket.Rot`. So a player turned by `/rotate` on
/// the one tick in sixty that its counter fires goes out as a `PosRot` with
/// zero deltas, and on any other tick as a `Rot`. The tail after the deltas
/// is the same three bytes either way — `yRot`, `xRot`, `onGround`, read
/// from both `write` methods on the pinned jar.
///
/// [`decode`] files a `PosRot` with sub-threshold deltas as [`HEARTBEAT`],
/// which the projection drops unless a check names it. **The rotation went
/// with it.** Measured 2026-10-03 on the `structures-2` gate:
/// `136-console-rotate-turns-each-target-to-the-angles-given` recorded this
/// server's turn of `oracle_eye` as a heartbeat whose `rest` was
/// `d94b2b1b7cfa262b` — byte for byte vanilla's `move_entity_rot` `rest`
/// for the same turn — and the check went from match to diverge on nothing
/// but where in its 60-tick cycle the shard's clock had drifted. Vanilla is
/// exposed to the same coin flip; it just did not land there that run.
///
/// The packet is therefore recorded as what it carries: a
/// `move_entity_rot` with the rotation's three bytes as its `rest` (exactly
/// what a `Rot` on that tick records), then the heartbeat, with its deltas
/// and the `onGround` byte that a `Pos` heartbeat's `rest` is taken over.
/// Both halves keep every byte compared; a server that never turned the
/// entity, turned it wrong, or sent a heartbeat off its counter still moves
/// the recording. A `PosRot` that really moved (`move_entity`) is untouched:
/// vanilla sends that one on every phase.
pub fn observe_all(
    norm: &mut Normalizer,
    states: &BTreeMap<i32, String>,
    items: &BTreeMap<i32, String>,
    id: i32,
    body: &[u8],
) -> Vec<(String, BTreeMap<String, serde_json::Value>)> {
    let Some((kind, f)) = observe(norm, states, items, id, body) else {
        return Vec::new();
    };
    if id != ids::play::cb::MOVE_ENTITY_POS_ROT || kind != HEARTBEAT || f.contains_key("decode") {
        return vec![(kind, f)];
    }
    let mut r = body;
    if read_varint(&mut r).is_err() || r.len() != 6 + 3 {
        return vec![(kind, f)];
    }
    let tail = &r[6..];
    let mut rot = BTreeMap::new();
    if let Some(e) = f.get("entity") {
        rot.insert("entity".to_owned(), e.clone());
    }
    rot.insert("rest".to_owned(), num(digest(tail)));
    let mut beat = f;
    beat.insert("rest".to_owned(), num(digest(&tail[2..])));
    vec![("move_entity_rot".to_owned(), rot), (kind, beat)]
}

/// `RUSTMC_TRACE_RAW` — print the bytes a `rest` digest was taken over.
///
/// **Why this exists.** `rest` is a one-way digest, and that is deliberate:
/// it compares every undecoded byte exactly without this harness having to
/// understand any of them. The cost is that a divergence in an undecoded
/// tail reads as two different hex strings and says nothing about *which*
/// field moved — and the same opacity applies before there is a divergence
/// at all, when the question is "what exactly does vanilla put here, so that
/// this server can put the same thing".
///
/// Deriving those bytes from the jar and confirming them against a recorded
/// digest works and is cheap when the payload has few unknowns; it stops
/// working the moment the payload is a list whose length you do not know.
/// `ClientboundUpdateAttributesPacket` for a player is exactly that case.
///
/// So: set `RUSTMC_TRACE_RAW` to a comma-separated list of kinds — or to
/// `all` — and every matching observation prints its tail as hex beside the
/// digest that will be recorded for it, on stderr, as it is recorded.
///
/// It is **off unless the variable is set**, it writes to stderr only, and it
/// touches neither the recording nor the projection: the digest is computed
/// from the same slice either way. A gate cannot be affected by it, which is
/// the property that lets it stay in the tree instead of being a patch
/// somebody re-applies.
fn raw_dump(kind: &str, tail: &[u8]) {
    use std::sync::OnceLock;
    static WANTED: OnceLock<Option<Vec<String>>> = OnceLock::new();
    let wanted = WANTED.get_or_init(|| {
        std::env::var("RUSTMC_TRACE_RAW")
            .ok()
            .map(|v| v.split(',').map(|s| s.trim().to_owned()).collect())
    });
    let Some(wanted) = wanted.as_ref() else {
        return;
    };
    if !wanted.iter().any(|w| w == "all" || w == kind) {
        return;
    }
    let mut hex = String::with_capacity(tail.len() * 2);
    for b in tail {
        use std::fmt::Write as _;
        let _ = write!(hex, "{b:02x}");
    }
    eprintln!("raw {kind} {} {hex}", digest(tail));
}

/// Decode the fields that need normalising, returning how many bytes were
/// consumed. `None` means a read ran off the end.
/// `net.minecraft.network.LpVec3`, the low-precision vector 26.2 introduced.
///
/// Transcribed from `javap -c` on `LpVec3.read`. A leading zero byte is the
/// whole encoding of `Vec3.ZERO` — which is why a stopped entity costs one
/// byte and a moving one costs six. Otherwise: two more bytes and a
/// big-endian `u32` assemble a 48-bit word, the low three bits carry the
/// integer scale (with bit 2 flagging a `VarInt` continuation holding the
/// scale's upper bits), and three 15-bit fields at offsets 3, 18 and 33 each
/// unpack to `[-1, 1]` before being multiplied by the scale.
///
/// **Six is the unscaled case only**, which is the one the corpus produces.
/// `hasContinuationBit` is `(b0 & 4) == 4`, and when it is set a `VarInt`
/// follows the six, so the encoding is 7-11 bytes. Re-verified against the
/// pinned jar 2026-08-05 — `readUnsignedByte`, then `readUnsignedByte` and
/// `readUnsignedInt`, shifts 3/18/33 — after a brief reported this count as
/// off by one. It is not; the count, the bit offsets and the continuation
/// test all match the bytecode.
///
/// **`None` on a short read, and the caller then records nothing rather than
/// a wrong vector.** The digest still covers the bytes either way.
fn read_lp_vec3(r: &mut &[u8]) -> Option<serde_json::Value> {
    let b0 = i64::from(read_u8(r).ok()?);
    if b0 == 0 {
        return Some(serde_json::json!([0.0, 0.0, 0.0]));
    }
    let b1 = i64::from(read_u8(r).ok()?);
    let hi = i64::from(read_i32(r).ok()?) & 0xFFFF_FFFF;
    let word = (hi << 16) | (b1 << 8) | b0;
    let mut scale = b0 & 3;
    if b0 & 4 == 4 {
        scale |= (i64::from(read_varint(r).ok()?) & 0xFFFF_FFFF) << 2;
    }
    // `min(.., 32766)` is vanilla's, and it is the reason this decode is not
    // allowed to replace the digest: it maps two distinct wire values onto
    // one double.
    let unpack = |shifted: i64| -> f64 {
        let q = (shifted & 32767).min(32766);
        #[allow(clippy::cast_precision_loss)]
        let v = q as f64 * 2.0 / 32766.0 - 1.0;
        #[allow(clippy::cast_precision_loss)]
        let s = scale as f64;
        v * s
    };
    Some(serde_json::json!([
        unpack(word >> 3),
        unpack(word >> 18),
        unpack(word >> 33),
    ]))
}

/// What one [`read_item_stack`] call decided about the bytes in front of it.
///
/// **Three outcomes and not two.** "These bytes are not an `ItemStack`" and
/// "this is as much of this `ItemStack` as anything can read without a codec
/// it does not have" are different findings, and a caller that conflates them
/// is the exact defect this whole item exists to remove: a decoder that
/// answers *something* for every input cannot fail, and a decoder that cannot
/// fail is worse than the digest it replaced, because the digest was honest
/// about being opaque.
#[derive(Debug)]
enum StackRead {
    /// A whole stack was consumed. `r` now points at the next field of the
    /// packet, and the caller may go on reading.
    Done(serde_json::Value),
    /// The head parsed and a data-component patch this decoder cannot walk
    /// begins at `r`. Everything from here to the end of the packet stays in
    /// `rest`, so the comparison keeps every byte; the caller must stop.
    Opaque(serde_json::Value),
    /// These bytes are not an `ItemStack`. Vanilla's own decoder throws on
    /// each of the cases below, so the packet is not decodable at all and the
    /// caller must report it as undecoded rather than as a stack.
    Refused,
}

/// `ItemStack.OPTIONAL_STREAM_CODEC` — the form every clientbound stack takes.
///
/// # The wire form, transcribed from the pinned jar
///
/// `ItemStack$1.decode` (the anonymous `StreamCodec` built in `ItemStack`'s
/// static initialiser and handed `DataComponentPatch.STREAM_CODEC`):
///
/// ```text
///   VarInt count                 ; <= 0 is the WHOLE encoding of an empty
///                                ; stack — one byte, nothing follows
///   VarInt item                  ; Item.STREAM_CODEC = ByteBufCodecs
///                                ; .holderRegistry(Registries.ITEM), which is
///                                ; ByteBufCodecs$29: VarInt.read, then
///                                ; IdMap.byIdOrThrow — a plain registry id
///   VarInt n_added               ; DataComponentPatch$3.decode
///   VarInt n_removed             ; both zero -> EMPTY patch, stack ends here
///   n_added   x { VarInt type ; <payload, by that type's own stream codec> }
///   n_removed x { VarInt type }
/// ```
///
/// `DataComponentType.STREAM_CODEC` is `ByteBufCodecs.registry(Registries
/// .DATA_COMPONENT_TYPE)` — the same plain-id shape — so both loops start with
/// a self-delimiting VarInt.
///
/// # Why the added components are where this stops, and it is not laziness
///
/// **An added component's payload carries no length.** It is written by that
/// component type's own stream codec and can only be skipped by running that
/// codec, so a decoder without the codec cannot advance past it — not by
/// guessing, not by scanning. The removals can be read, because a removal is
/// nothing but its type id; the additions cannot.
///
/// That leaves exactly one honest option and it is [`StackRead::Opaque`]: read
/// the head, read the two counts, read the first added type id (which is
/// unambiguously a VarInt at a known offset, and names the component the next
/// agent would have to write a codec for), and stop. Everything from that
/// point is still digested into `rest` by [`observe`], so **no byte leaves the
/// comparison** — the projection gains fields and loses nothing.
///
/// Writing codecs for component types the corpus has never produced would be
/// the opposite trade: speculative arms nothing exercises, each one a new
/// place to be silently wrong, in the part of a stack most likely to carry
/// per-run variance. When a real recording stops on a component, its type id
/// is in the trace and that is the moment to write that one codec.
///
/// # The three refusals, each of them somewhere vanilla throws
///
/// * a short read of any of the VarInts — `readVarInt` on an exhausted buffer;
/// * an item id the registry does not hold — `IdMap.byIdOrThrow`;
/// * a negative patch count, which `Reference2ObjectArrayMap`'s constructor
///   rejects and no encoder can produce.
///
/// **`count <= 0` is not one of them.** It is vanilla's own encoding of an
/// empty stack and the one case where an empty result is the right answer;
/// it is recorded as `{"count": 0}` so that a genuine empty stack and a
/// refusal can never read alike.
///
/// # What consuming a VarInt costs the digest, stated rather than skipped
///
/// A VarInt is not a canonical encoding: the value one is `0x01`, and it is
/// also `0x81 0x00`. While these bytes sat inside `rest` the two would have
/// compared different; now they compare the same. Both writers in this corpus are
/// `VarInt.write` and `varint::write_varint`, which emit the shortest form
/// only, so nothing here can produce the second — but the strictness is real
/// and it is spent, and this is the note that says so.
fn read_item_stack(r: &mut &[u8], items: &BTreeMap<i32, String>) -> StackRead {
    let Ok(count) = read_varint(r) else {
        return StackRead::Refused;
    };
    if count <= 0 {
        return StackRead::Done(serde_json::json!({ "count": 0 }));
    }
    let Ok(item_id) = read_varint(r) else {
        return StackRead::Refused;
    };
    // `IdMap.byIdOrThrow`. An id this table does not hold is a stack neither
    // server could have meant, and naming it `""` the way `states` names an
    // unknown block state would put a decoded-looking observation into the
    // trace for bytes nobody parsed.
    let Some(name) = items.get(&item_id) else {
        return StackRead::Refused;
    };
    let mut v = serde_json::Map::new();
    v.insert("count".into(), num(count));
    v.insert("item_id".into(), num(item_id));
    v.insert("item".into(), num(name.clone()));

    let (Ok(n_added), Ok(n_removed)) = (read_varint(r), read_varint(r)) else {
        return StackRead::Refused;
    };
    if n_added < 0 || n_removed < 0 {
        return StackRead::Refused;
    }
    if n_added == 0 && n_removed == 0 {
        return StackRead::Done(serde_json::Value::Object(v));
    }
    v.insert("comp_added".into(), num(n_added));
    v.insert("comp_removed".into(), num(n_removed));
    if n_added == 0 {
        // Removals are self-delimiting, so this whole stack is legible.
        let mut removed = Vec::new();
        for _ in 0..n_removed {
            let Ok(t) = read_varint(r) else {
                return StackRead::Refused;
            };
            removed.push(num(t));
        }
        v.insert("removed".into(), serde_json::Value::Array(removed));
        return StackRead::Done(serde_json::Value::Object(v));
    }
    let Ok(first) = read_varint(r) else {
        return StackRead::Refused;
    };
    v.insert("opaque_component".into(), num(first));
    StackRead::Opaque(serde_json::Value::Object(v))
}

/// Advance past `n` bytes, or `None` if there are not that many.
fn skip(r: &mut &[u8], n: usize) -> Option<()> {
    if r.len() < n {
        return None;
    }
    *r = &r[n..];
    Some(())
}

/// Vanilla's own limit on how deeply an NBT tag may nest.
///
/// `FriendlyByteBuf.readNbt` builds `NbtAccounter.defaultQuota()`, which is
/// `new NbtAccounter(2097152L, 512)` — a 2 MiB byte quota and a **512** depth
/// cap, enforced by `pushDepth`. Read off the pinned jar rather than chosen,
/// because the only reason this constant exists is to bound recursion on a
/// payload this harness did not write, and a cap of our own invention would
/// refuse a tag vanilla considers legal.
const NBT_MAX_DEPTH: u32 = 512;

/// Skip the *payload* of one nameless NBT tag of type `ty`, returning `None`
/// on a short read or a type the format does not define.
///
/// # Why a skipper and not a reader
///
/// The block-entity list is a sequence of variable-length records and the tag
/// is the last field of each, so reaching record *n + 1* requires knowing
/// exactly where record *n*'s tag ends. Nothing else needs the tag's
/// *contents*: the observation carries [`digest`] of its bytes, which is the
/// same treatment `block_entity_data` gives the same tag, and which compares
/// it exactly without an NBT model in this file.
///
/// # Layout, all of it read off the pinned jar
///
/// `NbtIo.readAnyTag` reads a type byte and returns `EndTag` on 0; every
/// other type is a **nameless** payload. Widths come from each
/// `TagType.size()`/`skip()`: byte 1, short 2, int 4, long 8, float 4,
/// double 8. `ByteArrayTag` is `readInt()` then that many bytes,
/// `IntArrayTag` `readInt() * 4`, `LongArrayTag` `readInt() * 8`.
/// `StringTag.skipString` is `readUnsignedShort()` then that many bytes —
/// **`DataInput.readUTF`'s modified UTF-8, so the count is bytes and not
/// chars**, which is the whole reason this can be skipped without decoding
/// it. `ListTag.loadList` reads the element type byte, then `readListCount`
/// (a signed `readInt`, refused below zero), and refuses a non-empty list of
/// type 0. `CompoundTag.loadCompound` loops: type byte, stop on 0, then
/// `readString` (the same `readUTF`) for the name, then the payload.
///
/// The tag ids themselves are `TagTypes.<clinit>`'s array, indices 0..12.
fn nbt_skip_payload(r: &mut &[u8], ty: u8, depth: u32) -> Option<()> {
    if depth > NBT_MAX_DEPTH {
        return None;
    }
    match ty {
        1 => skip(r, 1),
        2 => skip(r, 2),
        3 | 5 => skip(r, 4),
        4 | 6 => skip(r, 8),
        7 => {
            let n = usize::try_from(read_i32(r).ok()?).ok()?;
            skip(r, n)
        }
        8 => {
            let n = read_u16(r).ok()?;
            skip(r, n as usize)
        }
        9 => {
            let elem = read_u8(r).ok()?;
            let n = read_i32(r).ok()?;
            if n < 0 || (elem == 0 && n > 0) {
                // `readListCount` throws on a negative count and `loadList`
                // throws on "Missing type on ListTag". Refusing here is the
                // same refusal: a body vanilla would not have accepted is a
                // short read as far as this decoder is concerned.
                return None;
            }
            for _ in 0..n {
                nbt_skip_payload(r, elem, depth + 1)?;
            }
            Some(())
        }
        10 => loop {
            let t = read_u8(r).ok()?;
            if t == 0 {
                return Some(());
            }
            let name = read_u16(r).ok()?;
            skip(r, name as usize)?;
            nbt_skip_payload(r, t, depth + 1)?;
        },
        11 => {
            let n = usize::try_from(read_i32(r).ok()?).ok()?;
            skip(r, n.checked_mul(4)?)
        }
        12 => {
            let n = usize::try_from(read_i32(r).ok()?).ok()?;
            skip(r, n.checked_mul(8)?)
        }
        _ => None,
    }
}

fn decode(
    norm: &mut Normalizer,
    states: &BTreeMap<i32, String>,
    items: &BTreeMap<i32, String>,
    id: i32,
    body: &[u8],
) -> Option<(String, BTreeMap<String, serde_json::Value>, usize)> {
    use ids::play::cb;
    let mut f: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    let mut r = body;

    // Every arm decodes the fields that need normalising and leaves `r`
    // pointing at the rest, which is digested below. A decode that runs off
    // the end leaves the whole body as the digest rather than failing: a
    // truncated read is a harness bug and must not be reported as a
    // gameplay divergence.
    let kind: String = match id {
        x if x == cb::BLOCK_UPDATE => {
            let at = unpack_pos(read_i64(&mut r).ok()?);
            let state = read_varint(&mut r).ok()?;
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("state_id".into(), num(state));
            f.insert(
                "state".into(),
                num(states.get(&state).cloned().unwrap_or_default()),
            );
            "block_update".into()
        }
        x if x == cb::SECTION_BLOCKS_UPDATE => {
            // `ClientboundSectionBlocksUpdatePacket`: the packed section
            // position, a count, then one varlong per change carrying
            // `(state << 12) | sectionRelativePos`.
            //
            // Decoding this is not cosmetic. Vanilla sends it whenever more
            // than one block changes in one section in one tick — a door, a
            // bed, a pair of chests, a fence connecting, a torch falling off
            // its support — and it sent 18 of them across one corpus run
            // while this harness recorded every one as `packet:0x54` and
            // every check that wanted them read as having nothing to compare.
            let packed = read_i64(&mut r).ok()?;
            let (sx, sy, sz) = (
                (packed >> 42) as i32,
                ((packed << 44) >> 44) as i32,
                ((packed << 22) >> 42) as i32,
            );
            let n = read_varint(&mut r).ok()?;
            let mut blocks = Vec::new();
            for _ in 0..n.max(0) {
                let v = read_varlong(&mut r).ok()?;
                let state = (v >> 12) as i32;
                let p = (v & 0xFFF) as i32;
                let at = [
                    (sx << 4) | ((p >> 8) & 15),
                    (sy << 4) | (p & 15),
                    (sz << 4) | ((p >> 4) & 15),
                ];
                let rel = norm.rel(at);
                blocks.push(serde_json::json!({
                    "at": [rel[0], rel[1], rel[2]],
                    "state_id": state,
                    "state": states.get(&state).cloned().unwrap_or_default(),
                }));
            }
            // `at` is the first change's position, so the workspace radius in
            // `compare::near` has a real block to test and a section update
            // from a neighbouring workspace is excluded like any other.
            if let Some(first) = blocks.first().and_then(|b| b.get("at")).cloned() {
                f.insert("at".into(), first);
            }
            f.insert("blocks".into(), serde_json::Value::Array(blocks));
            "section_blocks_update".into()
        }
        x if x == cb::OPEN_SIGN_EDITOR => {
            let at = unpack_pos(read_i64(&mut r).ok()?);
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert(
                "front".into(),
                num(i64::from(r.first().copied().unwrap_or(0))),
            );
            "open_sign_editor".into()
        }
        x if x == cb::BLOCK_ENTITY_DATA => {
            // Position and type decoded; the NBT payload after them is left
            // to the digest, which makes it exact without needing an NBT
            // reader here.
            let at = unpack_pos(read_i64(&mut r).ok()?);
            let kind_id = read_varint(&mut r).ok()?;
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("block_entity_type".into(), num(kind_id));
            "block_entity_data".into()
        }
        // **The block entities inside a chunk payload, and nothing else in
        // it.**
        //
        // # What this buys, measured rather than assumed
        //
        // `docs/PARITY-ROADMAP.md` §25 measured the block-entity unit's
        // reachable ceiling at **20 of 49**, from two directions: a rig placed
        // chest, barrel, hopper and lectern controls and none of them said
        // anything about itself, and the jar says why —
        // `BlockEntity.getUpdatePacket()` is `aconst_null; areturn`, 29 of the
        // 49 registered types do not override it, and
        // `ChunkHolder.broadcastBlockEntity` returns on null. So
        // `block_entity_data` is a channel those 29 are never on.
        //
        // They are on this one. `ClientboundLevelChunkPacketData`'s
        // constructor walks `LevelChunk.getBlockEntities()` **whole** and
        // calls `BlockEntityInfo.create` on every entry, with no filter of any
        // kind. Nothing about a chest keeps it out of its own chunk's payload.
        //
        // # And what it does NOT buy — the brief for this arm was wrong here
        //
        // It does not make a chest's *contents* observable, and it was written
        // down that it would. `BlockEntityInfo.create` calls
        // `BlockEntity.getUpdateTag(provider)` and then `tag.isEmpty() ? null
        // : tag`. The base `getUpdateTag` is `new CompoundTag(); areturn`, and
        // of the block entity classes in the jar only seventeen override it —
        // banner, beacon, brushable, campfire, conduit, creaking heart,
        // decorated pot, jigsaw, shelf, sign, skull, spawner, structure, test,
        // test instance, end gateway and trial spawner. **Chest, hopper,
        // barrel, lectern and furnace are not among them**, so their entry
        // carries a null tag and this channel reports their *existence, place
        // and type* and no more.
        //
        // That is still the whole of what the census's block-entity unit
        // counts, and it is a real assertion: a server that puts a chest block
        // down without creating a `ChestBlockEntity` behind it now differs
        // from vanilla where before both were silent.
        //
        // # Wire order, from `ClientboundLevelChunkWithLightPacket`'s own
        // `RegistryFriendlyByteBuf` constructor
        //
        // ```text
        //   int   x                      readInt, big-endian
        //   int   z
        //   -- ClientboundLevelChunkPacketData --
        //   varint  heightmap count       ByteBufCodecs.map -> readCount
        //     varint type                 Heightmap$Types.STREAM_CODEC = idMapper
        //     varint len ; len * i64      ByteBufCodecs.LONG_ARRAY = readLongArray
        //   varint  section blob length   refused above 2097152
        //   bytes   section blob
        //   varint  block entity count    BlockEntityInfo.LIST_STREAM_CODEC
        //     byte  packedXZ              sectionRelative(x) << 4 | sectionRelative(z)
        //     short y                     readShort, signed
        //     varint type                 ByteBufCodecs.registry -> plain VarInt id
        //     nbt   tag                   readNbt, 0x00 meaning null
        //   -- ClientboundLightUpdatePacketData --
        // ```
        //
        // # What is deliberately dropped, and what each would have cost
        //
        // `consumed` is `body.len()`, so nothing this arm did not name reaches
        // a field **or** the trailing `rest` digest. This is the shape the
        // `sound` arm uses for its `long seed`, widened from one field to
        // three sections, and deliberately not the shape `container_set_slot`
        // uses — a rebase is for a counter whose *differences* are behaviour
        // and whose origin is session history, and none of what is dropped
        // here is a counter or has a relative form.
        //
        // * **the heightmaps** — four `long[]` per chunk, recomputed by
        //   whichever of the two servers' worldgen produced the column. They
        //   are a restatement of the terrain, so keeping them would put the
        //   whole of worldgen into the comparison of every gameplay scenario
        //   that happens to be sent a chunk;
        // * **the section blob** — per-section block counts, block-state
        //   palettes and biome palettes for the full column height. Same
        //   objection, an order of magnitude larger, and it carries the
        //   *palette encoding* as well as the values: two servers that agree
        //   about every block can still disagree about whether a section is
        //   single-valued or indirect, which is a wire-format difference no
        //   scenario in this corpus was written to assert;
        // * **the whole light payload** — `ClientboundLightUpdatePacketData`,
        //   two `BitSet` pairs and up to 26 half-byte arrays per chunk. It is
        //   the output of a lighting engine that no check here measures, and
        //   it is the one section that would flip on a *timing* difference:
        //   light is computed asynchronously and a chunk sent before the
        //   engine has caught up carries different bytes for the same world.
        //
        // Projecting any of the three would make every scenario in the corpus
        // compare a surface it was never written to assert, which is the
        // definition of a check that starts failing for reasons its author
        // cannot read.
        //
        // The tag **is** kept, as a digest, because that is exactly what
        // `block_entity_data` does with the same bytes six arms above: the
        // same block entity seen through two channels must be compared on the
        // same fields, or a divergence would mean different things depending
        // on which packet carried it.
        //
        // # An empty list is recorded under its own kind — see [`observe`]
        //
        // Every chunk that enters a client's view sends one of these, and in
        // this corpus's superflat world almost all of them are empty. `main`
        // dropped them on the argument that the count is chunk streaming —
        // which chunk batch landed inside the measured window. The census
        // over the `5*` scenarios refutes that: two vanilla runs agree on the
        // count and the step in 20 of 20, and it is the BATCH packets that
        // flap, not the chunks. So they are recorded as
        // [`CHUNK_NO_BLOCK_ENTITIES`], a graded kind with no `at`: what a
        // check on it grades is how many chunks the view gained per step.
        x if x == cb::LEVEL_CHUNK_WITH_LIGHT => {
            let cx = read_i32(&mut r).ok()?;
            let cz = read_i32(&mut r).ok()?;
            let maps = read_varint(&mut r).ok()?;
            for _ in 0..maps.max(0) {
                // DROP: chunk_block_entities.heightmaps — the heightmaps, the
                // section blob and the light tail are this chunk's terrain,
                // which the per-block kinds already compare where it changes
                let _kind = read_varint(&mut r).ok()?;
                let longs = usize::try_from(read_varint(&mut r).ok()?).ok()?;
                skip(&mut r, longs.checked_mul(8)?)?;
            }
            let blob = usize::try_from(read_varint(&mut r).ok()?).ok()?;
            skip(&mut r, blob)?;
            let n = read_varint(&mut r).ok()?;
            let mut list = Vec::new();
            for _ in 0..n.max(0) {
                let packed = read_u8(&mut r).ok()?;
                let y = i32::from(read_i16(&mut r).ok()?);
                let kind_id = read_varint(&mut r).ok()?;
                // The tag's bytes, from its type byte to its last, so the
                // digest covers exactly what vanilla wrote for this entry.
                let before = r;
                let ty = read_u8(&mut r).ok()?;
                if ty != 0 {
                    nbt_skip_payload(&mut r, ty, 0)?;
                }
                let tag = &before[..before.len() - r.len()];
                let at = [
                    (cx << 4) | i32::from((packed >> 4) & 15),
                    y,
                    (cz << 4) | i32::from(packed & 15),
                ];
                let rel = norm.rel(at);
                list.push(serde_json::json!({
                    "at": [rel[0], rel[1], rel[2]],
                    "block_entity_type": kind_id,
                    "tag": digest(tag),
                }));
            }
            // `at` is the first block entity's position, for the same reason
            // `section_blocks_update` above uses the first change's: the
            // workspace radius in `compare::near` needs a real block to test,
            // and workspaces are 32 blocks apart — two chunks — so no chunk
            // ever holds block entities from two of them.
            if let Some(first) = list.first().and_then(|b| b.get("at")).cloned() {
                f.insert("at".into(), first);
            }
            f.insert("block_entities".into(), serde_json::Value::Array(list));
            // Consumed to the end on purpose: the light payload after the
            // block-entity list is dropped rather than digested.
            return Some((CHUNK_BLOCK_ENTITIES.into(), f, body.len()));
        }
        x if x == cb::BLOCK_CHANGED_ACK => {
            f.insert("seq".into(), num(read_varint(&mut r).ok()?));
            "block_changed_ack".into()
        }
        x if x == cb::BLOCK_DESTRUCTION => {
            let e = read_varint(&mut r).ok()?;
            let at = unpack_pos(read_i64(&mut r).ok()?);
            f.insert("entity".into(), entity_ref(e));
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("progress".into(), num(read_u8(&mut r).ok()?));
            "block_destruction".into()
        }
        x if x == cb::BLOCK_EVENT => {
            let at = unpack_pos(read_i64(&mut r).ok()?);
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("b0".into(), num(read_u8(&mut r).ok()?));
            f.insert("b1".into(), num(read_u8(&mut r).ok()?));
            f.insert("block".into(), num(read_varint(&mut r).ok()?));
            "block_event".into()
        }
        x if x == cb::LEVEL_EVENT => {
            f.insert("event".into(), num(read_i32(&mut r).ok()?));
            let at = unpack_pos(read_i64(&mut r).ok()?);
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("data".into(), num(read_i32(&mut r).ok()?));
            "level_event".into()
        }
        x if x == cb::ADD_ENTITY => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            // DROP: add_entity.uuid — a fresh 128-bit value on both servers
            for _ in 0..16 {
                read_u8(&mut r).ok()?;
            }
            // The uuid is a random 128-bit value on both servers and can never
            // match, so it is **consumed and discarded**: the sixteen bytes
            // advance `r` and therefore never reach `rest`. What still checks
            // it is present is the read itself — a body too short to hold it
            // fails to decode and is recorded as `packet:0x..`, never dropped.
            //
            // (This comment used to say the uuid was "digested out" and that
            // its existence was checked "because the digest covers a constant
            // number of bytes". Both halves were false — consuming the bytes
            // is precisely what keeps them *out* of the digest. Corrected
            // 2026-08-05.)
            f.insert("kind_id".into(), num(read_varint(&mut r).ok()?));
            let p = [
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
            ];
            let rel = norm.rel_f(p);
            f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            // **The movement is consumed and discarded, like the uuid above
            // and like `sound`'s seed.** ORACLE rule 26.
            //
            // `ClientboundAddEntityPacket.write` is, from `javap -c` on 26.2:
            // `id`, `uuid`, `type`, `x`, `y`, `z`, then
            // `Vec3.LP_STREAM_CODEC` for **movement**, then `xRot`, `yRot`,
            // `yHeadRot` as bytes, then `data` as a varint. The movement sits
            // *before* the rotations, and there are no velocity shorts in the
            // tail — this file's own module docs said there were, and so did
            // `ci/checks/gameplay-unstable.toml`'s entry for
            // `dropping-an-item-throws-it-on-the-floor`. Both were wrong and
            // both are corrected in this commit.
            //
            // The field is `ServerEntity.getLastSentMovement()`. For a
            // standing player it is a coin toss vanilla makes — it depends on
            // which tick vanilla's last motion packet went out on — which is
            // what put `530-walking-away-drops-a-standing-player-from-view` on
            // a `diverge` floor it should never have needed. For a thrown item
            // it is four fresh `nextFloat()` draws from an unseeded
            // `RandomSource`, which is what put
            // `dropping-an-item-throws-it-on-the-floor` in the unstable file
            // at 25/25. Neither is a property of any server, and no check in
            // this corpus claims to test a velocity.
            //
            // **What claims the field instead**, because an exemption with
            // nothing behind it manufactures a check that cannot fail:
            //   * `tracker::add_entity_announces_the_last_sent_movement`
            //     asserts this server puts `ServerEntity.lastSentMovement`
            //     into the packet rather than zero — the plumbing the corpus
            //     was incidentally covering;
            //   * `an_add_entitys_movement_is_not_part_of_the_observation`
            //     below asserts the exemption is exactly this wide: a body
            //     differing only in movement compares equal, a body differing
            //     in a rotation still differs.
            // **What stays unmeasured, written down rather than assumed
            // closed:** no check compares an entity's movement value *as
            // `add_entity` carries it* any more. Closing that needs a check on
            // a player's velocity, which needs server-side player physics.
            //
            // **The paragraph this replaced said something stronger and it was
            // false. Corrected 2026-08-09.** It said "no check compares an
            // entity's movement value against vanilla's on the wire any more"
            // and justified it with "`EntityType.trackDeltas()` is false for
            // only ten types (PLAYER, LLAMA_SPIT, WITHER, BAT, ITEM_FRAME,
            // GLOW_ITEM_FRAME, LEASH_KNOT, PAINTING, END_CRYSTAL,
            // EVOKER_FANGS) and the corpus's only observer of that kind is
            // check 500, whose entity is an ITEM, for which vanilla
            // structurally never emits one."
            //
            // The list is right and the conclusion inverts it: `ITEM` is *not*
            // one of the ten, so its `trackDeltas` is **true** and vanilla
            // emits `set_entity_motion` for a dropped item every time. The
            // corpus says so directly — in the 2026-08-09 corpus run
            // `dropping-an-item-throws-it-on-the-floor` reports
            // `we never sent: set_entity_motion` with vanilla's first
            // difference `{"entity":0,"v":[-0.00647012146737469,0.0,
            // 0.138985533785021]}`, and `605`/`606` carry four of them each.
            // So `set_entity_motion.v` **is** compared, by exactly one check,
            // and it is four fresh `nextFloat()` draws — which is one of the
            // reasons that check is declared in
            // `ci/checks/gameplay-unstable.toml`. A claim of structural
            // impossibility that the reference recording refutes on its own
            // first line is the same defect as a stale caveat list.
            //
            // **THE DISCARD MOVED FROM `decode` TO `project` ON 2026-08-18 AND
            // THE COMPARISON IS UNCHANGED BY IT.** Everything above stays true
            // of what is *compared*; what is no longer true is that the field
            // is gone. `compare::project` now removes it, and this arm names
            // it — the same order `y_rot` was taken in, and for the same
            // reason.
            //
            // A field consumed here is invisible to every instrument this
            // repository has. `ci/trace_vocabulary.py` cannot see it, so a
            // floor's `vocabulary` cannot record it. `ci/determinism-gap.py`
            // cannot count it, so nothing can say whether vanilla varies in it
            // and we do not. `ci/hidden-stream-audit.py` cannot read it out of
            // the client 102 of 115 checks discard. The projection ledger
            // classified this drop `armed` — real behaviour, compared by
            // nothing — and `armed` was as far as any instrument could get
            // while the bytes were being eaten one layer below the one that
            // looks.
            //
            // **And the drop is wider than its own justification, which is the
            // reason it had to move.** The two sentences above establish a
            // draw for `minecraft:player` and for `minecraft:item`. The corpus
            // reaches nine other entity types (`ci/checks/entity-reach.toml`),
            // and for none of them has anybody read a draw — a golem is put in
            // the world by `CarvedPumpkinBlock.spawnGolemInWorld` with a
            // literal zero rotation and no `finalizeSpawn`, and a
            // `falling_block` starts at rest. The field may well be reproducible
            // for most of what this corpus spawns. Named, that becomes a
            // question a recording can answer; consumed, it was not a question
            // at all.
            f.insert("movement".into(), read_lp_vec3(&mut r)?);
            // **The three rotations and `data` are named, and until
            // 2026-08-09 they were four anonymous bytes inside `rest`.**
            //
            // `ClientboundAddEntityPacket.write`, `javap -p -c -constants` on
            // the pinned jar, ends `xRot` (byte, offset 74-82), `yRot` (byte,
            // 83-91), `yHeadRot` (byte, 92-100), `data` (varint, 101-109) and
            // then `return`. So this consumes the whole body and `rest`
            // becomes the digest of an empty tail for a well-formed packet —
            // which is not a loss: a body with anything after `data` still
            // digests differently, and a body too short to hold them fails to
            // decode and is recorded as `packet:0x..` rather than dropped.
            //
            // # Why naming them is the narrow way to exempt the item yaw
            //
            // `ItemEntity(EntityType, Level)` offsets 39-52 are
            // `setYRot(this.random.nextFloat() * 360.0F)`, from an unseeded
            // `RandomSource`. **One** of these four bytes is therefore
            // unreproducible for an item, and it is unreproducible by vanilla
            // against itself as much as by this server —
            // `docs/PARITY-ROADMAP.md` §11 measured `605` and `606` at 2/2 on
            // that byte alone. While all four lived in one digest, the only
            // exemption available was "drop `rest` for an item `add_entity`",
            // which throws away `xRot`, `yHeadRot` and `data` — three values
            // that are constant `0` for every item vanilla has ever spawned —
            // to reach one byte that is not. Named, the exemption in
            // `compare::project` removes exactly the one draw and leaves the
            // other three under comparison. See that function's `y_rot` arm.
            //
            // The angles stay in wire units (256ths of a turn, signed) rather
            // than being converted to degrees: the comparison is of bytes on
            // the wire and a float conversion would introduce a rounding
            // question the packet does not have.
            f.insert("x_rot".into(), num(read_u8(&mut r).ok()?.cast_signed()));
            f.insert("y_rot".into(), num(read_u8(&mut r).ok()?.cast_signed()));
            f.insert(
                "y_head_rot".into(),
                num(read_u8(&mut r).ok()?.cast_signed()),
            );
            f.insert("data".into(), num(read_varint(&mut r).ok()?));
            "add_entity".into()
        }
        x if x == cb::REMOVE_ENTITIES => {
            let n = read_varint(&mut r).ok()?;
            let mut ids = Vec::new();
            for _ in 0..n.max(0) {
                ids.push(entity_ref(read_varint(&mut r).ok()?));
            }
            // Recorded in wire order, **not** sorted. The list a server sends
            // here is a set with no order the server chose, so it has to be
            // canonicalised — but sorting raw ids and sorting the indices they
            // become are different orders, and it is the indices that get
            // compared. `compare::project` sorts it once the numbering exists.
            f.insert("entities".into(), serde_json::json!(ids));
            "remove_entities".into()
        }
        // `ClientboundSetEntityLinkPacket.write`: `writeInt(sourceId)`,
        // `writeInt(destId)` — plain big-endian ints, not varints, and the
        // holder is `0` when the lead was cut. `0` is kept as the number it
        // is rather than made an entity reference: no entity has id 0, and
        // renumbering it would give a cut lead an index like a real holder.
        x if x == cb::SET_ENTITY_LINK => {
            let e = read_i32(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            let holder = read_i32(&mut r).ok()?;
            f.insert(
                "holder".into(),
                if holder == 0 {
                    num(0)
                } else {
                    entity_ref(holder)
                },
            );
            "set_entity_link".into()
        }
        x if x == cb::SET_ENTITY_DATA => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            "set_entity_data".into()
        }
        x if x == cb::SET_ENTITY_MOTION => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            // **Read beside the digest, never instead of it.** `r` is not
            // advanced past the velocity, so `rest` still digests every byte
            // of it and no comparison anywhere changes. That is the whole
            // safety argument for adding this, and it is not the argument
            // `move_entity`'s deltas were lifted out on: those *replaced*
            // digest coverage, which was safe only because three `i16`s map
            // to three integers injectively. `LpVec3` does not — `unpack`
            // clamps the packed field to 32766, so 32766 and 32767 are
            // different bytes that decode to the same double — so a decode
            // that consumed these bytes would make two different wire
            // messages compare equal. Leaving the digest in place makes the
            // question moot.
            //
            // Why it is worth having at all: `set_entity_motion` was the
            // largest single source of strict irreproducibility in the
            // corpus and every instance of it read as an opaque changed
            // hash, which is the same condition `move_entity` was in before
            // its deltas became readable. See [`read_lp_vec3`].
            if let Some(v) = read_lp_vec3(&mut { r }) {
                f.insert("v".into(), v);
            }
            "set_entity_motion".into()
        }
        x if x == cb::SET_EQUIPMENT => {
            // `ClientboundSetEquipmentPacket.write`: the entity, then one
            // entry per changed slot as `EquipmentSlot.ordinal()` with bit 7
            // set on every entry **except the last** (offsets 51-88 write
            // `ordinal | -128` while `i != size - 1`), each followed by its
            // stack. The loop is bounded because every iteration consumes at
            // least the marker byte.
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            let mut slots = Vec::new();
            loop {
                let marker = read_u8(&mut r).ok()?;
                let stack = match read_item_stack(&mut r, items) {
                    StackRead::Done(v) => v,
                    StackRead::Opaque(v) => {
                        slots.push(serde_json::json!({ "slot": marker & 0x7F, "item": v }));
                        f.insert("truncated".into(), num(true));
                        break;
                    }
                    StackRead::Refused => return None,
                };
                slots.push(serde_json::json!({ "slot": marker & 0x7F, "item": stack }));
                if marker & 0x80 == 0 {
                    break;
                }
            }
            f.insert("slots".into(), serde_json::Value::Array(slots));
            "set_equipment".into()
        }
        x if x == cb::ENTITY_EVENT => {
            let e = read_i32(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            f.insert("event".into(), num(read_u8(&mut r).ok()?));
            "entity_event".into()
        }
        x if x == cb::ANIMATE => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            f.insert("action".into(), num(read_u8(&mut r).ok()?));
            "animate".into()
        }
        x if x == cb::TAKE_ITEM_ENTITY => {
            let item = read_varint(&mut r).ok()?;
            let taker = read_varint(&mut r).ok()?;
            f.insert("item".into(), entity_ref(item));
            f.insert("taker".into(), entity_ref(taker));
            f.insert("count".into(), num(read_varint(&mut r).ok()?));
            "take_item_entity".into()
        }
        x if x == cb::MOVE_ENTITY_POS || x == cb::MOVE_ENTITY_POS_ROT => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            // Deltas are already relative, so they need no anchor and are
            // compared exactly, which is the point: how far a thing moved in
            // one tick is a behaviour, where it started is an accident.
            //
            // They used to be left in the trailing digest, which compared
            // them exactly too. Lifting them out changes no comparison; it
            // makes the one predicate below expressible, and it makes a
            // failure readable instead of a changed hash.
            let d = [
                i64::from(read_i16(&mut r).ok()?),
                i64::from(read_i16(&mut r).ok()?),
                i64::from(read_i16(&mut r).ok()?),
            ];
            f.insert("d".into(), serde_json::json!(d));
            if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] >= MOVED_WIRE_SQ {
                "move_entity".into()
            } else {
                HEARTBEAT.into()
            }
        }
        // **Three arms that exist to make `compare::near` able to see these
        // packets at all.** ORACLE: a filter that cannot parse a thing admits
        // it.
        //
        // `compare::place` reads `fields["entity"]` and `fields["at"]`; an
        // undecoded packet has neither, `place` returns `None`, and
        // `compare::near` returns `true` on its "nobody ever told me where this
        // is" branch. **An undecoded packet is exempt from the spatial bound by
        // construction** — so the corpus's one defence against one scenario's
        // leftovers landing in the next scenario's window was off for exactly
        // the packets that leak.
        //
        // Measured: `080-spawn-egg-makes-a-mob` spawns a mob, never removes it,
        // and goes on rotating its head. `090-chest-opens-a-screen` runs next,
        // 32 blocks away against a default `within` of 16, and seven
        // `packet:0x53` land in its strict projection — six of them at
        // `at_ms == 0`, before its first step ran. One of them,
        // `3aaeca07b4dd0020`, is byte-identical to a packet inside 080's own
        // window. The mob was in the placement table the whole time, seeded by
        // 080's `add_entity`; nothing could read it out.
        //
        // **Decoding a kind does not silence it, it bounds it.** Only the
        // leading varint is consumed; every other byte still reaches the
        // trailing digest, so a `rotate_head` about the actor — who is at the
        // anchor — is compared exactly as before, rotation included. That is
        // what `180`, `235` and `270` assert by staying `match`.
        //
        // Wire formats read with `javap -p -c -constants` on the pinned jar,
        // never recalled:
        //   * `ClientboundRotateHeadPacket.write` — `writeVarInt(entityId)`
        //     then `writeByte(yHeadRot)`.
        //   * `ClientboundMoveEntityPacket$Rot.write` — `writeVarInt(entityId)`,
        //     `writeByte(yRot)`, `writeByte(xRot)`, `writeBoolean(onGround)`.
        //   * `ClientboundUpdateAttributesPacket`'s `STREAM_CODEC` is
        //     `composite(ByteBufCodecs.VAR_INT, …, AttributeSnapshot.STREAM_CODEC
        //     .apply(list(128)), …)`, so the entity id is the leading varint and
        //     the snapshots follow.
        //
        // `move_entity_rot` gets **no** `MOVED_WIRE_SQ` treatment. That test is
        // `sendChanges`'s own *position* test and this packet carries no
        // position; a rotation-only broadcast is not a heartbeat and must not
        // be filtered as one.
        x if x == cb::ROTATE_HEAD => {
            f.insert("entity".into(), entity_ref(read_varint(&mut r).ok()?));
            "rotate_head".into()
        }
        x if x == cb::MOVE_ENTITY_ROT => {
            f.insert("entity".into(), entity_ref(read_varint(&mut r).ok()?));
            "move_entity_rot".into()
        }
        x if x == cb::UPDATE_ATTRIBUTES => {
            f.insert("entity".into(), entity_ref(read_varint(&mut r).ok()?));
            "update_attributes".into()
        }
        x if x == cb::TELEPORT_ENTITY || x == cb::ENTITY_POSITION_SYNC => {
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            let p = [
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
            ];
            let rel = norm.rel_f(p);
            f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            ENTITY_POSITION.into()
        }
        x if x == cb::DAMAGE_EVENT => {
            // **Every field, because the two this used to stop at left two raw
            // entity ids in the digest.**
            //
            // `ClientboundDamageEventPacket.write` puts five things on the
            // wire: `entityId` as a varint, the damage type through
            // `DamageType.STREAM_CODEC` (`ByteBufCodecs.holderRegistry`, so one
            // BARE varint of the registry id — `holderRegistry` is
            // `registry(key, asHolderIdMap)` and has no inline arm to reserve
            // 0 for; recorded 2026-09-30, `generic` arrives as 18, its index
            // in the captured `registry_data`), then `sourceCauseId` and
            // `sourceDirectId` through `writeOptionalEntityId`, then an
            // `Optional<Vec3>` position.
            //
            // Reading only the first two left the two optional ids to fall into
            // the trailing FNV digest **unnormalised**, and an entity id is a
            // counter no two servers agree about and no two runs of one server
            // agree about either. Every other entity-id-bearing packet here goes
            // through `entity_ref`; this one did not, so the first check
            // ever written that observed `damage_event` would have been
            // permanently unmatchable for a reason with nothing to do with
            // damage. No check observes it today, which is the only reason this
            // cost nothing — the trap was armed, not sprung.
            //
            // `writeOptionalEntityId` writes `id + 1`, with -1 meaning "no
            // entity", so the wire value 0 is the absent case and must not be
            // normalised as if it were entity 0.
            let e = read_varint(&mut r).ok()?;
            f.insert("entity".into(), entity_ref(e));
            f.insert("source".into(), num(read_varint(&mut r).ok()?));
            for name in ["cause", "direct"] {
                let raw = read_varint(&mut r).ok()? - 1;
                let v = if raw < 0 {
                    serde_json::Value::Null
                } else {
                    entity_ref(raw)
                };
                f.insert(name.into(), v);
            }
            // The source position is optional and absolute, so it gets the same
            // anchor subtraction every other position in this file gets.
            if read_bool(&mut r).ok()? {
                let p = [
                    read_f64(&mut r).ok()?,
                    read_f64(&mut r).ok()?,
                    read_f64(&mut r).ok()?,
                ];
                let rel = norm.rel_f(p);
                f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            }
            "damage_event".into()
        }
        x if x == cb::SET_HEALTH => {
            f.insert("health".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("food".into(), num(read_varint(&mut r).ok()?));
            f.insert("saturation".into(), num(f64::from(read_f32(&mut r).ok()?)));
            "set_health".into()
        }
        x if x == cb::SET_EXPERIENCE => {
            f.insert("progress".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("level".into(), num(read_varint(&mut r).ok()?));
            f.insert("total".into(), num(read_varint(&mut r).ok()?));
            "set_experience".into()
        }
        x if x == cb::CONTAINER_SET_SLOT => {
            // `ClientboundContainerSetSlotPacket`: containerId, stateId, a
            // signed short slot, then the stack it is being set to. The stack
            // is the entire content of this packet and it was inside `rest`.
            f.insert("window".into(), num(read_varint(&mut r).ok()?));
            f.insert("state".into(), num(read_varint(&mut r).ok()?));
            f.insert("slot".into(), num(i32::from(read_u16(&mut r).ok()? as i16)));
            match read_item_stack(&mut r, items) {
                StackRead::Done(v) | StackRead::Opaque(v) => {
                    f.insert("item".into(), v);
                }
                StackRead::Refused => return None,
            }
            "container_set_slot".into()
        }
        x if x == cb::CONTAINER_SET_CONTENT => {
            // `ClientboundContainerSetContentPacket`: containerId, stateId,
            // `ItemStack.OPTIONAL_LIST_STREAM_CODEC` — a VarInt length then
            // that many stacks — and then the single carried stack.
            //
            // **One opaque stack blinds every later field of this packet**,
            // and that is stated in the trace rather than hidden: `truncated`
            // says the list stopped early and `carried` is then simply
            // absent. The bytes are all still in `rest`.
            f.insert("window".into(), num(read_varint(&mut r).ok()?));
            f.insert("state".into(), num(read_varint(&mut r).ok()?));
            let n = read_varint(&mut r).ok()?;
            if n < 0 {
                return None;
            }
            let mut slots = Vec::new();
            let mut stopped = false;
            for _ in 0..n {
                match read_item_stack(&mut r, items) {
                    StackRead::Done(v) => slots.push(v),
                    StackRead::Opaque(v) => {
                        slots.push(v);
                        stopped = true;
                        break;
                    }
                    StackRead::Refused => return None,
                }
            }
            f.insert("slots".into(), serde_json::Value::Array(slots));
            if stopped {
                f.insert("truncated".into(), num(true));
            } else {
                match read_item_stack(&mut r, items) {
                    StackRead::Done(v) => {
                        f.insert("carried".into(), v);
                    }
                    StackRead::Opaque(v) => {
                        f.insert("carried".into(), v);
                        f.insert("truncated".into(), num(true));
                    }
                    StackRead::Refused => return None,
                }
            }
            "container_set_content".into()
        }
        x if x == cb::SET_CURSOR_ITEM => {
            // `ClientboundSetCursorItemPacket` is one composite over
            // `ItemStack.OPTIONAL_STREAM_CODEC` and nothing else, so the whole
            // packet is a stack. It was `packet:0x60` — observed by two checks
            // and compared as an opaque digest, which is what check 720's own
            // header says to fix by writing this arm.
            match read_item_stack(&mut r, items) {
                StackRead::Done(v) | StackRead::Opaque(v) => {
                    f.insert("item".into(), v);
                }
                StackRead::Refused => return None,
            }
            "set_cursor_item".into()
        }
        x if x == cb::SET_PLAYER_INVENTORY => {
            // `ClientboundSetPlayerInventoryPacket`: composite of
            // `ByteBufCodecs.VAR_INT` (the slot) and one stack.
            f.insert("slot".into(), num(read_varint(&mut r).ok()?));
            match read_item_stack(&mut r, items) {
                StackRead::Done(v) | StackRead::Opaque(v) => {
                    f.insert("item".into(), v);
                }
                StackRead::Refused => return None,
            }
            "set_player_inventory".into()
        }
        x if x == cb::OPEN_SCREEN => {
            f.insert("window".into(), num(read_varint(&mut r).ok()?));
            f.insert("menu".into(), num(read_varint(&mut r).ok()?));
            "open_screen".into()
        }
        x if x == cb::CONTAINER_SET_DATA => {
            // `ClientboundContainerSetDataPacket.write` (javap offsets 0..25):
            // `writeContainerId` — a VarInt — then two `writeShort`s, the data
            // slot and its value. Decoded so the container id is a `window`
            // field `compare::rebase_session_counters` can rebase: while it
            // sat inside `rest`, a furnace's four data packets digested the
            // per-connection menu counter, so the same scenario recorded on a
            // connection that had opened a different number of menus before
            // it — a shard's first scenario against the serial run's — was a
            // strict divergence on both servers (`470`, 2026-09-24 recordings).
            f.insert("window".into(), num(read_varint(&mut r).ok()?));
            f.insert("id".into(), num(i32::from(read_u16(&mut r).ok()? as i16)));
            f.insert(
                "value".into(),
                num(i32::from(read_u16(&mut r).ok()? as i16)),
            );
            "container_set_data".into()
        }
        x if x == cb::WAYPOINT => {
            // `ClientboundTrackedWaypointPacket.STREAM_CODEC`: the
            // `Operation` (`ByteBufCodecs.idMapper`, a VarInt), then
            // `TrackedWaypoint.write`: `writeEither(UUIDUtil.STREAM_CODEC,
            // writeUtf)` — a boolean, `true` for the left (a UUID, two longs)
            // — then `Waypoint.Icon.STREAM_CODEC` (the style's `Identifier`
            // string, then `optional(RGB_COLOR)`: a boolean and three bytes),
            // then `writeEnum(Type)` and the type's own `writeContents`:
            // nothing for EMPTY, three VarInts for VEC3I, two for CHUNK, a
            // float for AZIMUTH (javap on the pinned jar, each class's
            // `write`/`writeContents`).
            //
            // **Decoded so the position is anchor-relative.** VEC3I and CHUNK
            // carry ABSOLUTE coordinates; while the body was a digest, the
            // same scenario recorded at a different workspace — a shard's,
            // whose index restarts at the first workspace — was a strict
            // divergence on both servers (`540`, 2026-09-24 recordings).
            // Deliberately NOT named `at`: `compare::place` bounds anything
            // carrying `at` to the scenario's box, and a waypoint is about a
            // player who may be anywhere. Undecoded, it was admitted
            // unbounded; decoded, it still is.
            f.insert("op".into(), num(read_varint(&mut r).ok()?));
            if read_bool(&mut r).ok()? {
                let (hi, lo) = (read_i64(&mut r).ok()?, read_i64(&mut r).ok()?);
                f.insert("uuid".into(), num(format!("{hi:016x}{lo:016x}")));
            } else {
                f.insert("name".into(), num(read_string(&mut r).ok()?));
            }
            f.insert("style".into(), num(read_string(&mut r).ok()?));
            if read_bool(&mut r).ok()? {
                let rgb = [
                    read_u8(&mut r).ok()?,
                    read_u8(&mut r).ok()?,
                    read_u8(&mut r).ok()?,
                ];
                f.insert("color".into(), serde_json::json!(rgb));
            }
            let ty = read_varint(&mut r).ok()?;
            f.insert("type".into(), num(ty));
            match ty {
                1 => {
                    let v = [
                        read_varint(&mut r).ok()?,
                        read_varint(&mut r).ok()?,
                        read_varint(&mut r).ok()?,
                    ];
                    f.insert("vec".into(), pos_value(norm.rel(v)));
                }
                2 => {
                    let (cx, cz) = (read_varint(&mut r).ok()?, read_varint(&mut r).ok()?);
                    let (cx, cz) = norm.rel_chunk(cx, cz);
                    f.insert("chunk_x".into(), num(cx));
                    f.insert("chunk_z".into(), num(cz));
                }
                3 => {
                    f.insert("azimuth".into(), num(f64::from(read_f32(&mut r).ok()?)));
                }
                _ => {}
            }
            "waypoint".into()
        }
        x if x == cb::SOUND || x == cb::SOUND_ENTITY => {
            // **Decoded, so that the seed can be thrown away.**
            //
            // Every `ClientboundSoundPacket` carries a `long seed`, and
            // `Level.playSound` fills it from `soundSeedGenerator.nextLong()`
            // — a fresh 64-bit draw per sound. Digesting the body whole put
            // that seed into the comparison, so **any check whose projection
            // named `sound` could never match** — not for us, and not for
            // vanilla against itself. That is the same shape as the mob
            // spawn yaw and the dropped item's offset recorded in
            // `ci/checks/gameplay-unstable.toml`, except that this one is
            // fixable: the seed is the only random field in the packet and
            // everything else in it is behaviour.
            //
            // Wire order, from the packet's own `RegistryFriendlyByteBuf`
            // constructor: the sound as a `Holder`, the source enum, then
            // either a position as three **big-endian ints in eighths of a
            // block** (`SOUND`) or an entity id (`SOUND_ENTITY`), then volume
            // and pitch as floats, then the seed.
            let id = read_varint(&mut r).ok()?;
            if id == 0 {
                // An INLINE sound event — `Holder.direct(SoundEvent)`, which
                // `ByteBufCodecs.holder` writes as 0 and then
                // `SoundEvent.DIRECT_STREAM_CODEC`: the identifier, then an
                // optional fixed range. `/playsound` is the producer: it builds
                // `Holder.direct(createVariableRangeEvent(id))` for every sound
                // it plays, registered or not. Read through, so the fields
                // after it — and the seed, which is a level-random draw — are
                // decoded and dropped exactly as for a registry holder, rather
                // than left in a digest no second run could reproduce.
                let name = read_string(&mut r).ok()?;
                f.insert("sound".into(), serde_json::json!(name));
                if read_u8(&mut r).ok()? != 0 {
                    f.insert("range".into(), num(f64::from(read_f32(&mut r).ok()?)));
                }
            } else {
                f.insert("sound".into(), num(id - 1));
            }
            f.insert("source".into(), num(read_varint(&mut r).ok()?));
            if x == cb::SOUND {
                let p = [
                    f64::from(read_i32(&mut r).ok()?) / 8.0,
                    f64::from(read_i32(&mut r).ok()?) / 8.0,
                    f64::from(read_i32(&mut r).ok()?) / 8.0,
                ];
                let rel = norm.rel_f(p);
                f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            } else {
                let e = read_varint(&mut r).ok()?;
                f.insert("entity".into(), entity_ref(e));
            }
            f.insert("volume".into(), num(f64::from(read_f32(&mut r).ok()?)));
            // **The pitch is read and classified, not dropped — 2026-08-17.**
            //
            // `569323e` dropped it, beside the seed, and the reason was sound:
            // `ChestBlockEntity`'s open sound is
            // `level.getRandom().nextFloat() * 0.1f + 0.9f`, a fresh draw every
            // time, and the corpus run of 2026-08-07 02:19 recorded **five
            // different pitches for one event** — 0.9120 at `090`, 0.9268 at
            // `560`, 0.9339 at `490`, 0.9552 at `561`, 0.9952 at `602`. A
            // reference that disagrees with itself run to run cannot be matched
            // by any implementation.
            //
            // What that cost was named at the time and is now bought back: the
            // sounds whose pitch **is** behaviour stopped being compared on it,
            // and for the lever and the comparator the pitch is the *only*
            // field that differs between the two directions of the event. So a
            // `sound` observation of a lever said nothing about which way the
            // lever went.
            //
            // [`crate::pitch`] compares what the call site determines instead
            // of the float: a literal pitch exactly, a drawn pitch as the range
            // it is drawn from, and an unclassified event not at all — which is
            // `569323e`'s behaviour and is the default, so no green scenario
            // can redden for want of a table entry.
            //
            // Volume is **kept**, as it always was. Every volume in the corpus
            // is a literal in the calling class — 0.3 for a lever, 0.5 for a
            // chest, 1.0 for a button — and nothing draws it.
            let pitch = read_f32(&mut r).ok()?;
            if id == 0 {
                // An inline sound's pitch is the command's argument, not a
                // draw: compared as it stands.
                f.insert("pitch".into(), num(f64::from(pitch)));
            } else if let Some(v) = crate::pitch::observed_by_id(id - 1, pitch) {
                f.insert("pitch".into(), v);
            }
            // The seed is read and dropped, deliberately. Reading it keeps it
            // out of the trailing digest; dropping it keeps it out of the
            // comparison.
            // DROP: sound.seed — `soundSeedGenerator.nextLong()`, one fresh draw per sound
            let _seed = read_i64(&mut r).ok()?;
            "sound".into()
        }
        // `ClientboundChangeDifficultyPacket`: the difficulty's varint id
        // and the lock flag. Decoded rather than digested so a check on
        // `/difficulty` says WHICH difficulty arrived, not merely that the
        // bytes differ.
        x if x == cb::CHANGE_DIFFICULTY => {
            f.insert("difficulty".into(), num(read_varint(&mut r).ok()?));
            f.insert(
                "locked".into(),
                serde_json::Value::Bool(read_u8(&mut r).ok()? != 0),
            );
            "change_difficulty".into()
        }
        // `ClientboundStopSoundPacket`: a flag byte (1 = source present,
        // 2 = sound present), the source's ordinal, then the sound id.
        x if x == cb::STOP_SOUND => {
            let flags = read_u8(&mut r).ok()?;
            if flags & 1 != 0 {
                f.insert("source".into(), num(read_varint(&mut r).ok()?));
            }
            if flags & 2 != 0 {
                f.insert("sound".into(), serde_json::json!(read_string(&mut r).ok()?));
            }
            "stop_sound".into()
        }
        // `ClientboundSetTitleTextPacket` and its two siblings: one
        // component and nothing else. Named so a check can observe them; the
        // component itself stays in `rest` and is compared byte for byte, as
        // `chat`'s is.
        x if x == cb::SET_TITLE_TEXT => {
            // The component is the whole body, compared in `rest`.
            "set_title_text".into()
        }
        x if x == cb::SET_SUBTITLE_TEXT => {
            // The component is the whole body, compared in `rest`.
            "set_subtitle_text".into()
        }
        x if x == cb::SET_ACTION_BAR_TEXT => {
            // The component is the whole body, compared in `rest`.
            "set_action_bar_text".into()
        }
        // `ClientboundClearTitlesPacket`: `resetTimes` and nothing else.
        x if x == cb::CLEAR_TITLES => {
            f.insert(
                "reset".into(),
                serde_json::Value::Bool(read_u8(&mut r).ok()? != 0),
            );
            "clear_titles".into()
        }
        // `ClientboundSetTitlesAnimationPacket`: three ints, in ticks.
        x if x == cb::SET_TITLES_ANIMATION => {
            f.insert("fade_in".into(), num(read_i32(&mut r).ok()?));
            f.insert("stay".into(), num(read_i32(&mut r).ok()?));
            f.insert("fade_out".into(), num(read_i32(&mut r).ok()?));
            "set_titles_animation".into()
        }
        // `ClientboundPlayerAbilitiesPacket`: a flag byte (invulnerable 1,
        // flying 2, canFly 4, instabuild 8) and the two speeds.
        x if x == cb::PLAYER_ABILITIES => {
            f.insert("flags".into(), num(read_u8(&mut r).ok()?));
            f.insert(
                "flying_speed".into(),
                num(f64::from(read_f32(&mut r).ok()?)),
            );
            f.insert(
                "walking_speed".into(),
                num(f64::from(read_f32(&mut r).ok()?)),
            );
            "player_abilities".into()
        }
        x if x == cb::GAME_EVENT => {
            f.insert("event".into(), num(read_u8(&mut r).ok()?));
            f.insert("value".into(), num(f64::from(read_f32(&mut r).ok()?)));
            "game_event".into()
        }
        // The six world-border packets.
        //
        // Written as decoder arms rather than left as `packet:0x2B` etc. for
        // the reason this file's own `DECODED_KINDS` doc gives: an undecoded
        // packet's whole content is a digest, so a check that observes one
        // reports "the bytes differ" and nothing more. These carry doubles
        // whose last bits are the interesting part — the size mid-lerp, the
        // centre after a `centerCorrect` half-block — and a reader of a
        // failing check wants to see WHICH field moved.
        //
        // Every field is compared: nothing is consumed and left, so `rest`
        // stays empty and the comparison is still total.
        x if x == cb::INITIALIZE_BORDER => {
            f.insert("center_x".into(), num(read_f64(&mut r).ok()?));
            f.insert("center_z".into(), num(read_f64(&mut r).ok()?));
            f.insert("old_size".into(), num(read_f64(&mut r).ok()?));
            f.insert("new_size".into(), num(read_f64(&mut r).ok()?));
            // A varLONG. A varint here decodes every long lerp as the wrong
            // number without ever failing to parse.
            f.insert("lerp_time".into(), num(read_varlong(&mut r).ok()?));
            f.insert("absolute_max_size".into(), num(read_varint(&mut r).ok()?));
            f.insert("warning_blocks".into(), num(read_varint(&mut r).ok()?));
            f.insert("warning_time".into(), num(read_varint(&mut r).ok()?));
            "initialize_border".into()
        }
        x if x == cb::SET_BORDER_CENTER => {
            f.insert("center_x".into(), num(read_f64(&mut r).ok()?));
            f.insert("center_z".into(), num(read_f64(&mut r).ok()?));
            "set_border_center".into()
        }
        x if x == cb::SET_BORDER_LERP_SIZE => {
            f.insert("old_size".into(), num(read_f64(&mut r).ok()?));
            f.insert("new_size".into(), num(read_f64(&mut r).ok()?));
            f.insert("lerp_time".into(), num(read_varlong(&mut r).ok()?));
            "set_border_lerp_size".into()
        }
        x if x == cb::SET_BORDER_SIZE => {
            f.insert("size".into(), num(read_f64(&mut r).ok()?));
            "set_border_size".into()
        }
        // `set_border_warning_delay` carries `warningTime` and
        // `set_border_warning_distance` carries `warningBlocks`. The names
        // cross over, which is why each field here is named for what it holds
        // rather than for its packet.
        x if x == cb::SET_BORDER_WARNING_DELAY => {
            f.insert("warning_time".into(), num(read_varint(&mut r).ok()?));
            "set_border_warning_delay".into()
        }
        x if x == cb::SET_BORDER_WARNING_DISTANCE => {
            f.insert("warning_blocks".into(), num(read_varint(&mut r).ok()?));
            "set_border_warning_distance".into()
        }
        // The two scoreboard packets `/scoreboard objectives` produces.
        //
        // `set_objective`'s tail is conditional on the method — the display
        // name, render type and number format are present for ADD and CHANGE
        // and absent for REMOVE — so the decoder stops after the method and
        // lets the caller digest the rest. That is deliberate rather than
        // lazy: decoding a component here would put this file in the business
        // of NBT, and the digest compares those bytes exactly anyway. What the
        // two named fields buy is that a failing check says WHICH objective and
        // WHICH method rather than "the bytes differ".
        x if x == cb::SET_OBJECTIVE => {
            if let Ok(name) = read_string(&mut r) {
                f.insert("objective".into(), num(name));
                if let Ok(method) = read_u8(&mut r) {
                    f.insert("method".into(), num(method));
                }
            }
            "set_objective".into()
        }
        // `set_display_objective` is a varint slot and a string, and the whole
        // packet is read. An EMPTY objective name is the cleared form —
        // `ClientboundSetDisplayObjectivePacket` writes `""` for a null
        // objective — so a check comparing this field sees clearing and
        // setting as different values rather than as a present/absent flag.
        x if x == cb::SET_DISPLAY_OBJECTIVE => {
            if let Ok(slot) = read_varint(&mut r) {
                f.insert("slot".into(), num(slot));
            }
            if let Ok(name) = read_string(&mut r) {
                f.insert("objective".into(), num(name));
            }
            "set_display_objective".into()
        }
        // **A `LevelData.RespawnData`, not a bare position — 2026-10-01.**
        // `GlobalPos.STREAM_CODEC` (the dimension's identifier, then the
        // packed position), then `yaw` and `pitch` as floats. This arm read
        // the first eight bytes as the position until the day a scenario
        // named the kind: those bytes are the length prefix and
        // `minecraft:overw`, the same on every server whatever the spawn, so
        // `at` was a constant and the real position sat in the `rest` digest
        // as an absolute coordinate no two worlds share.
        x if x == cb::SET_DEFAULT_SPAWN_POSITION => {
            f.insert("dimension".into(), num(read_string(&mut r).ok()?));
            let at = unpack_pos(read_i64(&mut r).ok()?);
            f.insert("at".into(), pos_value(norm.rel(at)));
            f.insert("yaw".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("pitch".into(), num(f64::from(read_f32(&mut r).ok()?)));
            "set_default_spawn_position".into()
        }
        // `ClientboundBossEventPacket.write`: the bar's UUID, then the
        // `OperationType` ordinal — ADD 0, REMOVE 1, UPDATE_PROGRESS 2,
        // UPDATE_NAME 3, UPDATE_STYLE 4, UPDATE_PROPERTIES 5 — then the
        // operation's own body. The progress is decoded; a name's text is
        // read as runs; everything after the ordinal stays in the `rest`
        // digest, byte for byte.
        x if x == cb::BOSS_EVENT => {
            // DROP: boss_event.id — `CustomBossEvents.create` draws it from
            // `Mth.createInsecureUUID(random)`, a fresh unseeded draw per bar
            let _id = (read_i64(&mut r).ok()?, read_i64(&mut r).ok()?);
            let op = read_varint(&mut r).ok()?;
            f.insert("operation".into(), num(op));
            match op {
                2 => {
                    f.insert("progress".into(), num(f64::from(read_f32(&mut r).ok()?)));
                }
                0 | 3 => {
                    f.insert("text".into(), num(ascii_runs(r)));
                }
                _ => {}
            }
            "boss_event".into()
        }
        // `ClientboundClearDialogPacket.INSTANCE` — a unit codec, no body.
        // A block rather than a one-line arm: `ci/projection-drops.py` reads
        // the emittable kinds off arm-tail literals standing on their own
        // line, and read this one as "no arm emits `clear_dialog`".
        x if x == cb::CLEAR_DIALOG => {
            // Nothing to read: the packet is its id.
            "clear_dialog".into()
        }
        // `ClientboundPlayerRotationPacket`: `yRot` float, `relativeY` bool,
        // `xRot` float, `relativeX` bool — its `STREAM_CODEC` composite, in
        // that order. `/rotate` sends it through `forceSetRotation`.
        x if x == cb::PLAYER_ROTATION => {
            f.insert("yaw".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("yaw_relative".into(), num(read_u8(&mut r).ok()? != 0));
            f.insert("pitch".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("pitch_relative".into(), num(read_u8(&mut r).ok()? != 0));
            "player_rotation".into()
        }
        // `ClientboundLevelParticlesPacket.write`: `overrideLimiter`,
        // `alwaysShow`, x/y/z as doubles, the three spreads and the speed as
        // floats, the count as an int, then `ParticleTypes.STREAM_CODEC` —
        // the type's registry id as a varint and the type's own options,
        // which a simple type has none of. Options are left to the `rest`
        // digest, byte for byte.
        x if x == cb::LEVEL_PARTICLES => {
            f.insert("override_limiter".into(), num(read_u8(&mut r).ok()? != 0));
            f.insert("always_show".into(), num(read_u8(&mut r).ok()? != 0));
            let p = [
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
            ];
            let rel = norm.rel_f(p);
            f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            let d = [
                f64::from(read_f32(&mut r).ok()?),
                f64::from(read_f32(&mut r).ok()?),
                f64::from(read_f32(&mut r).ok()?),
            ];
            f.insert("dist".into(), serde_json::json!(d));
            f.insert("speed".into(), num(f64::from(read_f32(&mut r).ok()?)));
            f.insert("count".into(), num(read_i32(&mut r).ok()?));
            f.insert("particle".into(), num(read_varint(&mut r).ok()?));
            "level_particles".into()
        }
        x if x == cb::PLAYER_POSITION => {
            f.insert("teleport".into(), num(read_varint(&mut r).ok()?));
            let p = [
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
                read_f64(&mut r).ok()?,
            ];
            // **A relative teleport's coordinates are DELTAS — 2026-10-01.**
            // The body ends in the `Relative` bitmask (one big-endian int;
            // X, Y, Z are bits 0..2). On an axis whose bit is set the field
            // is an offset from wherever the player is, so normalising it
            // against the rig as if it were a place turned `~ ~ ~` into a
            // point hundreds of blocks off and the projection's
            // `near(within)` filter dropped it on BOTH sides: a `/tp ~ ~ ~`
            // check read `vacuous`. Such a teleport is recorded as `delta`,
            // un-normalised; the mask stays in `rest` with the rest of the
            // body. Every teleport the corpus had before command teleports
            // is absolute and decodes exactly as it did.
            let mask = body
                .len()
                .checked_sub(4)
                .and_then(|i| body.get(i..))
                .map_or(0, |t| i32::from_be_bytes([t[0], t[1], t[2], t[3]]));
            if mask & 0b111 != 0 {
                f.insert("delta".into(), serde_json::json!([p[0], p[1], p[2]]));
            } else {
                let rel = norm.rel_f(p);
                f.insert("at".into(), serde_json::json!([rel[0], rel[1], rel[2]]));
            }
            "player_position".into()
        }
        // **`player_chat` is its own kind, and it is the one packet in this
        // corpus that cannot be digested whole.**
        //
        // `SignedMessageBody.unsigned` substitutes `Instant.now()` for the
        // client's timestamp (and `0` for its salt), so the body carries the
        // *sending server's wall clock*. Two runs of the same scenario
        // against the same jar produce different bytes, and vanilla against
        // ours always would. Digesting it whole — which is what the `chat`
        // arm below does, consuming nothing — would make every chat check a
        // permanent divergence with no cause to find.
        //
        // So this arm consumes exactly as far as the timestamp and stops.
        // Everything after it — salt, the empty last-seen list, the absent
        // unsigned content, the filter mask, the chat-type holder and the
        // sender's display-name component — is stable and stays in `rest`,
        // where it is still compared byte for byte.
        //
        // The timestamp is consumed and **not** exposed as a field: a field
        // is compared, and this one carries no information about whether the
        // port is right. That is the opposite of the choice
        // `set_entity_motion` makes with its velocity, which is kept in
        // `rest` on purpose because two different velocities *are* two
        // different messages. A wall clock is not.
        x if x == cb::PLAYER_CHAT => {
            f.insert("global_index".into(), num(read_varint(&mut r).ok()?));
            let mut uuid = [0u8; 16];
            std::io::Read::read_exact(&mut r, &mut uuid).ok()?;
            // The speaker's offline uuid, in full rather than digested: it is
            // sixteen bytes and it is the field that says *who* spoke, which
            // is the first thing a reader of a failing chat check wants.
            f.insert(
                "sender".into(),
                num(uuid.iter().map(|b| format!("{b:02x}")).collect::<String>()),
            );
            f.insert("index".into(), num(read_varint(&mut r).ok()?));
            let signed = read_bool(&mut r).ok()?;
            f.insert("signed".into(), serde_json::json!(signed));
            if signed {
                let mut sig = [0u8; 256];
                // DROP: player_chat.signature — a signature over a wall clock cannot repeat
                std::io::Read::read_exact(&mut r, &mut sig).ok()?;
            }
            f.insert("text".into(), num(read_string(&mut r).ok()?));
            // Consumed, deliberately unnamed. See above.
            // DROP: player_chat.timestamp — `Instant.now()`, the sending server's clock
            let _wall_clock = read_i64(&mut r).ok()?;
            "player_chat".into()
        }
        // `ClientboundCommandsPacket` — the Brigadier tree the server tells
        // this player it has. Undecoded until 2026-08-18, which meant the
        // corpus had never once looked at the command registry: the only gate
        // on it was `ci/commands-golden.sh`, and that compares our tree
        // against vanilla's encoder applied to *our own* registration. It
        // cannot see a command vanilla has and we do not, because our
        // registration is what it is given. This arm is the first thing in
        // the project that asks a real vanilla server what it registers.
        //
        // # It decodes one layer, and it stops where it stops being able to
        //
        // A node body is: flags, a varint array of child indices, a redirect
        // index if `flags & 8`, then for a literal its name and for an
        // argument its name, a parser id and *the parser's own properties* —
        // one of ~50 shapes. Without those readers the cursor cannot advance
        // past the first argument node, so a full decode is the ~50
        // `ArgumentTypeInfo` deserialisers, which is not what this is.
        //
        // It does not need to be. `enumerateNodes` is breadth-first from an
        // empty map, so the root takes index 0 and its children take
        // 1..=count *contiguously*, before any grandchild is numbered. Every
        // root command is a literal. So the roots are exactly the nodes
        // immediately following the root node, they are all cheap, and they
        // are reachable without decoding a single argument.
        //
        // Everything past them stays in `rest` and is digested, so the part
        // this arm declines to read is still compared byte for byte. The
        // fields it does expose are the ones a reader of a failing check
        // wants first: how many commands the server admits to, and which.
        // ---- the chunk-view family -------------------------------------
        //
        // Five arms, all read off `javap -p -c -constants` on the pinned
        // 26.2 jar, and all of them exist so that the two branches whose
        // subject *is* the chunk stream can write a scenario at all. Only
        // the coordinates and the batch size are decoded; everything after
        // stays in `rest` and is compared byte-exactly, so the projection
        // gains fields and loses no bytes.
        //
        // `LEVEL_CHUNK_WITH_LIGHT` is not among them: it has its own arm
        // further down, which reads the block-entity list and nothing else,
        // and it has no `FAMILIES` row because a decoded packet is graded by
        // default like every other one.
        x if x == cb::SET_CHUNK_CACHE_CENTER => {
            // `ClientboundSetChunkCacheCenterPacket(FriendlyByteBuf)`:
            // `readVarInt` x, `readVarInt` z. Chunk coordinates, so they are
            // normalised against the anchor's chunk rather than against the
            // anchor: a block-relative subtraction here would be wrong by up
            // to fifteen sixteenths of a chunk.
            let (cx, cz) = (read_varint(&mut r).ok()?, read_varint(&mut r).ok()?);
            let (cx, cz) = norm.rel_chunk(cx, cz);
            f.insert("chunk_x".into(), num(cx));
            f.insert("chunk_z".into(), num(cz));
            "set_chunk_cache_center".into()
        }
        x if x == cb::FORGET_LEVEL_CHUNK => {
            // `readChunkPos` is `ChunkPos.unpack(readLong())`, and
            // `ChunkPos.pack(int, int)` is `(x & 0xFFFFFFFF) | (z << 32)` —
            // **x in the low half**, which is the opposite of `BlockPos`'s
            // packing one packet away and the thing to get wrong here.
            let v = read_i64(&mut r).ok()?;
            // `ChunkPos.unpack` is `new ChunkPos((int) v, (int) (v >> 32))`
            // — a narrowing cast, so both halves keep their sign.
            let (cx, cz) = norm.rel_chunk(v as i32, (v >> 32) as i32);
            f.insert("chunk_x".into(), num(cx));
            f.insert("chunk_z".into(), num(cz));
            "forget_level_chunk".into()
        }
        x if x == cb::LIGHT_UPDATE => {
            // `ClientboundLightUpdatePacket(FriendlyByteBuf)`: `readVarInt`
            // x, `readVarInt` z, then `ClientboundLightUpdatePacketData`.
            // The two coordinates are VarInts here and plain `i32`s inside
            // `LEVEL_CHUNK_WITH_LIGHT`, one packet apart.
            let (cx, cz) = (read_varint(&mut r).ok()?, read_varint(&mut r).ok()?);
            let (cx, cz) = norm.rel_chunk(cx, cz);
            f.insert("chunk_x".into(), num(cx));
            f.insert("chunk_z".into(), num(cz));
            "light_update".into()
        }
        x if x == cb::CHUNK_BATCH_START => {
            // `STREAM_CODEC = StreamCodec.unit(INSTANCE)` — it writes zero
            // bytes, so the frame length **is** the whole assertion. A
            // non-empty body cannot be this packet, and saying so is the only
            // way this arm can fail.
            if !body.is_empty() {
                f.insert("decode".into(), num("failed"));
                f.insert("refused".into(), num("a body for a unit codec"));
                r = body;
            }
            "chunk_batch_start".into()
        }
        x if x == cb::CHUNK_BATCH_FINISHED => {
            // One `readVarInt`, the batch size. **`size = 0` is not a
            // refusal** — an empty batch is a value vanilla produces — so a
            // real empty batch and a decode failure can never read alike.
            // A negative one cannot be: `PlayerChunkSender` writes a count.
            let n = read_varint(&mut r).ok()?;
            if n < 0 {
                f.insert("decode".into(), num("failed"));
                f.insert("refused".into(), num("a negative batch count"));
                r = body;
            } else {
                f.insert("size".into(), num(n));
            }
            "chunk_batch_finished".into()
        }
        x if x == cb::COMMANDS => {
            let (roots, refused, consumed) = command_roots(body);
            f.insert("root_count".into(), num(roots.len().to_string()));
            f.insert("roots".into(), num(roots.join(" ")));
            if let Some(why) = refused {
                f.insert("refused".into(), num(why.to_string()));
            }
            r = &body[consumed.min(body.len())..];
            "commands".into()
        }
        // `ClientboundUpdateAdvancementsPacket`: `boolean reset`,
        // `AdvancementHolder.LIST_STREAM_CODEC added`, `Set<Identifier>
        // removed`, `Map<Identifier, AdvancementProgress> progress`,
        // `boolean showAdvancements`. Five fields, and this arm reads the
        // first one and a half.
        //
        // # WHY THE TAIL IS NOT DIGESTED, WHICH IS OTHERWISE THIS FILE'S
        // WHOLE DESIGN
        //
        // Every other undecoded region is compared as an FNV digest, so a
        // field nobody wrote a decoder for is still compared exactly. Here
        // that would produce a check that is red while this server is right,
        // for two measured reasons:
        //
        //   * `DisplayInfo` ends with two floats written by
        //     `TreeNodePosition.run`, which lays a tree out by walking
        //     `AdvancementNode.children` — a fastutil
        //     `ReferenceOpenHashSet`, **identity** hashed. Permuting only the
        //     resource-scan order moves 122 of vanilla's 126 displayed `y`
        //     values, and two *identical* JVM runs of the same jar have been
        //     seen to disagree (`ci/advancements-golden.sh`, 2026-09-05).
        //   * `CriterionProgress.grant()` is `obtained = Instant.now()` and
        //     `serializeToNetwork` writes it, so every granted criterion on
        //     the wire carries a wall clock.
        //
        // Neither is skippable: `DisplayInfo` is two NBT component trees and
        // an `ItemStackTemplate`, and this harness has no NBT decoder, so
        // there is no way to walk past one holder to reach the next.
        //
        // What survives is exact rather than approximate. Both moving
        // quantities are FIXED WIDTH — two `float`s and one `long` — so the
        // body's LENGTH is a function of exactly which advancements the
        // server put in `added`, which ids are in `removed`, and which
        // criteria are in `progress`. A server that sends the wrong set, or
        // sends nothing, moves `len`; a server that lays its tree out
        // differently does not. `added` is read out as its own field because
        // it is the count a reader wants first.
        x if x == cb::UPDATE_ADVANCEMENTS => {
            f.insert("reset".into(), num(i32::from(read_bool(&mut r).ok()?)));
            f.insert("added".into(), num(read_varint(&mut r).ok()?));
            f.insert("len".into(), num(i32::try_from(body.len()).ok()?));
            // Consume the rest deliberately, so `rest` digests an empty
            // slice. Declining to compare is stated here; it is not the
            // decoder quietly running off the end, which the caller would
            // otherwise turn into a `packet:0x82` observation.
            r = &body[body.len()..];
            "update_advancements".into()
        }
        x if x == cb::SYSTEM_CHAT || x == cb::DISGUISED_CHAT => {
            f.insert("text".into(), num(ascii_runs(body)));
            "chat".into()
        }
        x if x == cb::DISCONNECT => {
            f.insert("text".into(), num(ascii_runs(body)));
            "disconnect".into()
        }
        // **A classified family with no decoder arm keeps its declared name.**
        //
        // This arm is why `observe`'s own fallback was not enough and the
        // first version of this change recorded `packet:0x2C` for `keep_alive`
        // over a whole census: `decode` never returns `None` for an unknown
        // id — it names it and consumes nothing — so `observe`'s
        // `family_of_id` lookup, which only runs when `decode` returns `None`,
        // was unreachable for exactly the seven families that have no arm.
        // `every_classified_family_records_under_its_declared_kind` caught it.
        //
        // Nothing about the comparison changes: the whole body still goes
        // into the `rest` digest, byte for byte, exactly as it did under
        // `packet:0x..`. What changes is the name a reader sees and, more to
        // the point, the name a scenario can put in its own `observe` list.
        other => match family_of_id(other) {
            Some(fam) => {
                f.insert("decode".into(), num("undecoded"));
                fam.kind.to_owned()
            }
            None => format!("packet:0x{other:02X}"),
        },
    };

    // How much was consumed. Whatever is left is digested by the caller, so
    // the part nobody has written a decoder for is still compared exactly.
    let consumed = body.len() - r.len();
    // Here rather than in an arm, so every kind is dumpable without a decoder
    // arm having to remember to ask. An instrument that covers only the
    // packets somebody already suspected is an instrument you have to be
    // right to use.
    dump_packet(&kind, body, consumed);
    Some((kind, f, consumed))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for `data/vanilla/items.json` inverted, as [`observe`] gets
    /// it from `client::Tables`.
    ///
    /// Three entries and a **deliberate gap**: `9999` is an id no registry
    /// holds, and the tests that feed it exist to show the decoder refusing
    /// rather than naming it.
    fn test_items() -> BTreeMap<i32, String> {
        [
            (0, "minecraft:air"),
            (1, "minecraft:stone"),
            (155, "minecraft:chest"),
        ]
        .into_iter()
        .map(|(k, v)| (k, v.to_owned()))
        .collect()
    }

    /// One canonical VarInt, written by the encoder the servers use.
    fn vi(v: i32) -> Vec<u8> {
        let mut out = Vec::new();
        rustmc_proto::varint::write_varint(&mut out, v).expect("vec write");
        out
    }

    /// A plain stack: count, item, and the two-zero empty component patch.
    fn stack(count: i32, item: i32) -> Vec<u8> {
        let mut b = vi(count);
        if count <= 0 {
            return b;
        }
        b.extend(vi(item));
        b.extend(vi(0));
        b.extend(vi(0));
        b
    }

    /// A stack carrying one **added** component, whose payload is opaque to
    /// this decoder by construction — which is the whole point of it.
    fn stack_with_component(count: i32, item: i32, type_id: i32, payload: &[u8]) -> Vec<u8> {
        let mut b = vi(count);
        b.extend(vi(item));
        b.extend(vi(1));
        b.extend(vi(0));
        b.extend(vi(type_id));
        b.extend_from_slice(payload);
        b
    }

    /// `ClientboundContainerSetSlotPacket`'s body around a given stack.
    fn slot_body(slot: i16, tail: &[u8]) -> Vec<u8> {
        let mut b = vi(1);
        b.extend(vi(7));
        b.extend((slot as u16).to_be_bytes());
        b.extend_from_slice(tail);
        b
    }

    /// One decoded packet, put through the projection unfiltered.
    ///
    /// The entity numbering lives there now, so a decoder assertion about an
    /// entity field has to go through it or it is asserting the raw id.
    fn projected(o: (String, BTreeMap<String, serde_json::Value>)) -> serde_json::Value {
        let obs = Obs {
            who: Who::Actor,
            step: 0,
            at_ms: 0,
            kind: o.0,
            fields: o.1,
        };
        crate::compare::project(
            &[obs],
            None,
            &std::collections::BTreeSet::new(),
            crate::scenario::Timing::Ignore,
            crate::scenario::From::Both,
            i32::MAX,
            [0, 0, 0],
            &crate::compare::Places::new(),
        )
        .swap_remove(0)
    }

    /// **Vanilla's movement test, at its own boundary, in integers.**
    ///
    /// `>= 128` and not `> 128`, because `sendChanges` compiles to
    /// `dcmpl; iflt`, so a delta landing exactly on `2⁻¹⁷` counts as movement.
    /// `[8, 8, 0]` is that case exactly — 64 + 64 — and it is the one a
    /// transcription of the constant as `>` would get wrong.
    ///
    /// `[-1, 0, 0]` is the other trap and is why the deltas are read as
    /// **`i16`**: read as `u16` it becomes 65535, whose square is far above
    /// any threshold, so every backwards heartbeat would be misfiled as
    /// movement and the exemption would silently cover only half the axes.
    /// **A light nibble must move the comparison.** `rest` digests only what
    /// a decoder did not consume, and this arm consumes only the two chunk
    /// coordinates, so every mask and every array byte stays in `rest`. That
    /// is the property light-engine-2's arm broke by reading the arrays and
    /// dropping them (found by `ci/projection-drops.py`, 2026-09-29); this
    /// pins it for the arm main landed instead. Two bodies one byte apart in
    /// one sky array must part on `rest` and agree on the coordinates.
    #[test]
    fn a_light_update_that_differs_in_one_nibble_is_a_different_observation() {
        let states = BTreeMap::new();
        let items = test_items();
        let body = |nibble: u8| {
            let mut b = vec![0x00, 0x00]; // x, z as varints
            for _ in 0..4 {
                b.push(0x01); // one long per BitSet
                b.extend_from_slice(&1i64.to_be_bytes());
            }
            b.push(0x01); // one sky array
            b.extend_from_slice(&[0x80, 0x10]); // of 2048 bytes
            let mut arr = vec![0u8; 2048];
            arr[1000] = nibble;
            b.extend_from_slice(&arr);
            b.push(0x00); // no block arrays
            b
        };
        let go = |nibble: u8| {
            let mut n = Normalizer::new([0, 0, 0]);
            observe(
                &mut n,
                &states,
                &items,
                ids::play::cb::LIGHT_UPDATE,
                &body(nibble),
            )
            .expect("recorded")
        };
        let (kind, a) = go(0x0F);
        let (_, b) = go(0x0E);
        assert_eq!(kind, "light_update");
        assert_eq!(a["chunk_x"], b["chunk_x"]);
        assert_ne!(a["rest"], b["rest"], "one nibble must move rest");
    }

    #[test]
    fn a_move_entity_below_vanillas_own_movement_test_is_a_heartbeat() {
        let mut norm = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        let mut kind = |d: [i16; 3]| {
            let mut body = vec![42u8]; // entity id, one-byte varint
            for v in d {
                body.extend_from_slice(&v.to_be_bytes());
            }
            body.push(1); // onGround, left in the digest
            observe(
                &mut norm,
                &states,
                &items,
                ids::play::cb::MOVE_ENTITY_POS,
                &body,
            )
            .expect("a move packet always decodes")
            .0
        };

        // Standing still: the only way this packet exists is the counter.
        assert_eq!(kind([0, 0, 0]), HEARTBEAT);
        // Just inside the boundary sphere, radius sqrt(128) = 11.31 units.
        assert_eq!(kind([11, 0, 0]), HEARTBEAT);
        assert_eq!(kind([8, 7, 0]), HEARTBEAT); // 113
        assert_eq!(kind([-1, 0, 0]), HEARTBEAT);
        // On it, and outside it.
        assert_eq!(kind([8, 8, 0]), "move_entity"); // 128 exactly
        assert_eq!(kind([12, 0, 0]), "move_entity"); // 144
                                                     // One walk hop in this corpus: half a block, 2048 units, 181x clear.
        assert_eq!(kind([-2048, 0, 0]), "move_entity");
    }

    /// A turn that lands on the heartbeat tick records exactly what the same
    /// turn records on any other tick, plus the heartbeat a `Pos` would have
    /// carried. Every half is checked against the packet vanilla sends in the
    /// other phase, byte for byte, so the split cannot invent or drop a field.
    #[test]
    fn a_heartbeat_that_carried_a_rotation_records_the_rotation() {
        use ids::play::cb;
        let states = BTreeMap::new();
        let items = test_items();
        let go = |id: i32, body: &[u8]| {
            let mut n = Normalizer::new([0, 0, 0]);
            observe_all(&mut n, &states, &items, id, body)
        };
        let (yaw, pitch, ground) = (0x40u8, 0xE9u8, 1u8);
        let move_body = |d: [i16; 3], rot: bool| {
            let mut b = vec![42u8];
            for v in d {
                b.extend_from_slice(&v.to_be_bytes());
            }
            if rot {
                b.extend_from_slice(&[yaw, pitch]);
            }
            b.push(ground);
            b
        };
        let rot_only = go(cb::MOVE_ENTITY_ROT, &[42, yaw, pitch, ground]);
        let pos_beat = go(cb::MOVE_ENTITY_POS, &move_body([0, 0, 0], false));
        assert_eq!(rot_only.len(), 1);
        assert_eq!(pos_beat.len(), 1);
        assert_eq!(pos_beat[0].0, HEARTBEAT);

        let split = go(cb::MOVE_ENTITY_POS_ROT, &move_body([0, 0, 0], true));
        assert_eq!(split, vec![rot_only[0].clone(), pos_beat[0].clone()]);

        // A different turn on the heartbeat tick is a different rotation.
        let other = go(cb::MOVE_ENTITY_POS_ROT, &{
            let mut b = move_body([0, 0, 0], true);
            b[7] = yaw.wrapping_add(1);
            b
        });
        assert_ne!(other[0], rot_only[0]);
        assert_eq!(other[1], pos_beat[0]);

        // A real move with a turn is one observation on every phase.
        let walk = go(cb::MOVE_ENTITY_POS_ROT, &move_body([-2048, 0, 0], true));
        assert_eq!(walk.len(), 1);
        assert_eq!(walk[0].0, "move_entity");
        // So is a heartbeat that carried no turn.
        let beat = go(cb::MOVE_ENTITY_POS, &move_body([11, 0, 0], false));
        assert_eq!(beat.len(), 1);
    }

    /// The deltas are **out of the digest and in a field**, which changes no
    /// comparison — they were compared exactly either way — and is what makes
    /// the predicate above expressible and a failure readable.
    #[test]
    fn a_move_entitys_deltas_are_recorded_rather_than_digested() {
        let mut norm = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        let body = [42u8, 0xF8, 0x00, 0x00, 0x00, 0x00, 0x0C, 1];
        let (kind, f) = observe(
            &mut norm,
            &states,
            &items,
            ids::play::cb::MOVE_ENTITY_POS,
            &body,
        )
        .unwrap();
        assert_eq!(kind, "move_entity");
        assert_eq!(f["d"], serde_json::json!([-2048, 0, 12]));
        // Only `onGround` is left for the trailing digest.
        assert_eq!(f["rest"], serde_json::json!(digest(&[1])));
    }

    /// **The velocity is read *and* still digested, and that is the point.**
    ///
    /// The comparison this harness performs on `set_entity_motion` must be
    /// byte-for-byte what it was before the decode existed, or a
    /// readability change would have quietly moved a corpus number. The
    /// third assertion is the one that states it: `rest` is the digest of
    /// the **whole** velocity payload, not of what is left after it.
    #[test]
    fn a_motion_velocity_is_read_beside_its_digest_and_not_instead_of_it() {
        let mut norm = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();

        // `Vec3.ZERO` is one byte. A stopped entity is the cheap case, and
        // it is the case `sendChanges` reaches through
        // `d > 0 && lengthSqr == 0`.
        let stopped = [42u8, 0x00];
        let (kind, f) = observe(
            &mut norm,
            &states,
            &items,
            ids::play::cb::SET_ENTITY_MOTION,
            &stopped,
        )
        .expect("a motion packet always decodes");
        assert_eq!(kind, "set_entity_motion");
        assert_eq!(f["v"], serde_json::json!([0.0, 0.0, 0.0]));
        assert_eq!(f["rest"], serde_json::json!(digest(&[0x00])));

        // Scale 1, x and z packed at 16383 — which unpacks to exactly zero,
        // 32766/32766 - 1 — and y at 15099, vanilla's packing of one tick of
        // gravity. 48 bits: scale in 0..3, x at 3, y at 18, z at 33.
        let word: i64 = 1 | (16383 << 3) | (15099 << 18) | (16383i64 << 33);
        #[allow(clippy::cast_possible_truncation)]
        let mut moving = vec![42u8, (word & 0xff) as u8, ((word >> 8) & 0xff) as u8];
        #[allow(clippy::cast_possible_truncation)]
        moving.extend_from_slice(&(((word >> 16) & 0xFFFF_FFFF) as u32).to_be_bytes());
        let (_, f) = observe(
            &mut norm,
            &states,
            &items,
            ids::play::cb::SET_ENTITY_MOTION,
            &moving,
        )
        .expect("a motion packet always decodes");
        let v = f["v"].as_array().expect("three components");
        assert!((v[0].as_f64().expect("x") - 0.0).abs() < 1e-12);
        assert!((v[1].as_f64().expect("y") + 0.0784).abs() < 1e-4);
        assert!((v[2].as_f64().expect("z") - 0.0).abs() < 1e-12);
        assert_eq!(f["rest"], serde_json::json!(digest(&moving[1..])));
    }

    #[test]
    fn positions_round_trip_through_the_long_packing() {
        for p in [[0, 0, 0], [1, -64, -1], [-30_000_000, 319, 29_999_999]] {
            let packed = (i64::from(p[0]) & 0x3FF_FFFF) << 38
                | (i64::from(p[2]) & 0x3FF_FFFF) << 12
                | (i64::from(p[1]) & 0xFFF);
            assert_eq!(unpack_pos(packed), p, "for {p:?}");
        }
    }

    /// **A recorded trace keeps the server's own id, unnumbered.**
    ///
    /// The numbering is `compare::project`'s, over the observations a check
    /// looks at; the recording's job is only to keep the id out of the digest
    /// and mark it as one. Asserting the raw value here is what stops the
    /// renumbering from creeping back to record time, where its scope is
    /// wrong by construction.
    #[test]
    fn a_recorded_entity_id_is_raw_and_marked() {
        assert_eq!(entity_ref(4231), serde_json::json!({ ENTITY_REF: 4231 }));
        assert_ne!(entity_ref(4231), entity_ref(99));
        // Not a bare number: a bare number would be compared as it stands.
        assert!(entity_ref(0).is_object());
    }

    #[test]
    fn positions_are_relative_to_the_anchor() {
        let n = Normalizer::new([100, -60, -20]);
        assert_eq!(n.rel([101, -60, -20]), [1, 0, 0]);
        let f = n.rel_f([100.5, -60.0, -20.0]);
        assert!((f[0] - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn the_digest_separates_different_bytes() {
        assert_ne!(digest(b"a"), digest(b"b"));
        assert_eq!(digest(b"abc"), digest(b"abc"));
        assert_ne!(digest(b""), digest(b"\0"));
    }

    #[test]
    fn an_undecoded_packet_still_carries_its_bytes() {
        let mut n = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        let (kind, f) = observe(&mut n, &states, &items, 0x7E, &[1, 2, 3]).expect("recorded");
        assert_eq!(kind, "packet:0x7E");
        assert_eq!(f["rest"], serde_json::json!(digest(&[1, 2, 3])));
    }

    /// **The three arms bound rather than delete, and the assertion is that
    /// the tail still moves the comparison.**
    ///
    /// Only the leading varint entity id is consumed. Everything after it —
    /// the head rotation, the two rotations and the ground flag, the attribute
    /// snapshots — still reaches the trailing digest, so two servers that agree
    /// about the entity and disagree about the rotation still part. Without
    /// this half the arms would be a way of deleting three kinds from the
    /// strict denominator, dressed as a way of bounding them.
    #[test]
    fn the_rotation_arms_lift_out_the_id_and_digest_everything_else() {
        let states = BTreeMap::new();
        let items = test_items();
        let go = |id: i32, body: &[u8]| {
            let mut n = Normalizer::new([0, 0, 0]);
            observe(&mut n, &states, &items, id, body).expect("recorded")
        };

        // `writeVarInt(entityId)` then `writeByte(yHeadRot)`.
        let (kind, f) = go(ids::play::cb::ROTATE_HEAD, &[0x2A, 0x40]);
        assert_eq!(kind, "rotate_head");
        assert_eq!(f["entity"], entity_ref(42));
        assert_eq!(f["rest"], serde_json::json!(digest(&[0x40])));
        // Same entity, different rotation: still a different observation.
        assert_ne!(
            go(ids::play::cb::ROTATE_HEAD, &[0x2A, 0x41]).1["rest"],
            f["rest"],
            "the rotation is bounded, not deleted"
        );

        // `writeVarInt`, `writeByte(yRot)`, `writeByte(xRot)`,
        // `writeBoolean(onGround)`.
        let (kind, f) = go(ids::play::cb::MOVE_ENTITY_ROT, &[0x07, 0x10, 0x20, 0x01]);
        assert_eq!(kind, "move_entity_rot");
        assert_eq!(f["entity"], entity_ref(7));
        assert_eq!(f["rest"], serde_json::json!(digest(&[0x10, 0x20, 0x01])));

        // `composite(VAR_INT, …, list(128) of AttributeSnapshot, …)`.
        let (kind, f) = go(ids::play::cb::UPDATE_ATTRIBUTES, &[0x05, 0x00]);
        assert_eq!(kind, "update_attributes");
        assert_eq!(f["entity"], entity_ref(5));
        assert_eq!(f["rest"], serde_json::json!(digest(&[0x00])));

        // A rotation-only broadcast is **not** a heartbeat. `MOVED_WIRE_SQ` is
        // `sendChanges`'s position test and this packet carries no position;
        // classifying it as one would delete it from every projection that did
        // not name it.
        assert_ne!(kind, HEARTBEAT);
    }

    /// **Real vanilla's declare-commands packet, byte for byte.**
    ///
    /// Dumped by `ci/command-surface-dump.sh` from a real
    /// `new Commands(CommandSelection.DEDICATED, ctx)`, filtered through
    /// `fillUsableCommands` for a `PermissionLevel.ALL` source — which is what
    /// the gameplay corpus's players hold — and encoded with vanilla's own
    /// `ClientboundCommandsPacket.STREAM_CODEC`. 265 bytes, 26 nodes, 10
    /// roots.
    ///
    /// It is embedded rather than read because the point of it is to be a
    /// tree this repo could not have produced. Our own packet has two roots,
    /// one branching parent, no redirects and no argument nodes, so a decoder
    /// checked only against ours is a decoder checked against nothing: every
    /// branch this arm has that ours cannot reach would be free to be wrong.
    /// This packet reaches all of them — `tell`, `w` and `tm` are redirect
    /// nodes with `flags & 8` set and a redirect index before their name, and
    /// node 11 onward are the argument nodes the arm declines to read.
    const VANILLA_COMMANDS_AT_LEVEL_ALL: &str = "\
1a000a0102030405060708090a01010b026d6505010c0468656c7005010d046c69737401010e036d\
73670900040474656c6c090004017701020f100672616e646f6d010111077465616d6d7367090008\
02746d0101120774726967676572060006616374696f6e14060007636f6d6d616e64050205000575\
75696473020113077461726765747306020101140576616c756501011504726f6c6c0600076d6573\
736167651416021617096f626a65637469766518146d696e6563726166743a61736b5f7365727665\
720600076d6573736167651406000572616e67652706000572616e67652701011803616464010119\
0373657406000576616c7565030006000576616c7565030000";

    /// The command arm reads the roots out of a real tree, and stops there.
    ///
    /// The two claims worth separating: that it gets the right ten names in
    /// vanilla's own registration order, and that it does **not** pretend to
    /// have read the rest. `consumed` is short of the body, so everything from
    /// the first grandchild on stays in the caller's digest and is still
    /// compared byte for byte. An arm that consumed the whole body would be
    /// declaring the argument nodes equal without having looked at them.
    #[test]
    fn the_command_arm_reads_a_real_tree_and_declines_the_rest() {
        let states = BTreeMap::new();
        let items = test_items();
        let body: Vec<u8> = (0..VANILLA_COMMANDS_AT_LEVEL_ALL.len() / 2)
            .map(|i| {
                u8::from_str_radix(&VANILLA_COMMANDS_AT_LEVEL_ALL[i * 2..i * 2 + 2], 16)
                    .expect("hex")
            })
            .collect();
        assert_eq!(body.len(), 265, "the dumped packet");

        let mut n = Normalizer::new([0, 0, 0]);
        let (kind, f) =
            observe(&mut n, &states, &items, ids::play::cb::COMMANDS, &body).expect("recorded");
        assert_eq!(kind, "commands");
        assert_eq!(f["root_count"], serde_json::json!("10"));
        assert_eq!(
            f["roots"],
            serde_json::json!("me help list msg tell w random teammsg tm trigger"),
            "vanilla's registration order, which is LinkedHashMap insertion order"
        );
        assert!(
            !f.contains_key("refused"),
            "nothing to refuse in a well-formed tree"
        );
        // `observe` digests whatever the arm did not consume. A non-empty
        // tail is the proof that the twenty-one nodes past the roots are
        // still being compared byte for byte rather than declared equal.
        assert_ne!(
            f["rest"],
            serde_json::json!(digest(&[])),
            "the arm must leave the argument nodes to the digest, not swallow them"
        );

        // And the substrate can express a difference: this server's own tree
        // through the same arm is a different observation, not the same one
        // twice. A check on a channel where both sides produce the same thing
        // agrees for the wrong reason.
        let ours = [
            0x03u8, 0x00, 0x01, 0x01, 0x05, 0x01, 0x02, 0x04, b'l', b'i', b's', b't', 0x05, 0x00,
            0x05, b'u', b'u', b'i', b'd', b's', 0x00,
        ];
        let (kind2, f2) =
            observe(&mut n, &states, &items, ids::play::cb::COMMANDS, &ours).expect("recorded");
        assert_eq!(kind2, "commands");
        assert_eq!(f2["root_count"], serde_json::json!("1"));
        assert_ne!(f["roots"], f2["roots"]);
    }

    /// A malformed tree is refused by name rather than decoded into a lie.
    ///
    /// Both arms matter and neither can be reached from a recording of either
    /// server, which is why they are built here: a root claiming more children
    /// than the packet holds, and an argument node where the contiguity
    /// argument says only literals can be.
    #[test]
    fn the_command_arm_refuses_a_tree_it_cannot_read() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut n = Normalizer::new([0, 0, 0]);

        // Two nodes, but the root claims five children.
        let overclaim = [0x02u8, 0x00, 0x05, 0x01, 0x02, 0x03, 0x04, 0x05];
        let (_, f) = observe(&mut n, &states, &items, ids::play::cb::COMMANDS, &overclaim)
            .expect("recorded");
        assert_eq!(
            f["refused"],
            serde_json::json!("the root claims more children than the packet has nodes")
        );
        assert_eq!(f["root_count"], serde_json::json!("0"));

        // An argument node directly under the root. `enumerateNodes` cannot
        // produce this for a real registry -- every root command is a literal
        // -- so if it is ever seen the assumption this arm rests on is wrong,
        // and it must say so rather than read a name out of the wrong shape.
        // A negative node count, which `0..kids` would turn into an empty
        // range: zero roots and no refusal, indistinguishable from a server
        // that registered nothing. The varint 0xFFFFFFFF0F is -1.
        let negative = [0xFFu8, 0xFF, 0xFF, 0xFF, 0x0F, 0x00, 0x00];
        let (roots, why, _) = command_roots(&negative);
        assert_eq!(why, Some("a negative node count"));
        assert!(roots.is_empty());

        // `total = i32::MIN`, where `total - 1` would overflow and panic in a
        // debug build -- in a decoder, on bytes from the far end of a socket.
        // The varint 0x8080808008 is i32::MIN.
        let min = [0x80u8, 0x80, 0x80, 0x80, 0x08, 0x00, 0x00];
        assert_eq!(command_roots(&min).1, Some("a negative node count"));

        let arg_root = [
            0x02u8, 0x00, 0x01, 0x01, 0x02, 0x00, 0x04, b'n', b'a', b'm', b'e',
        ];
        let (_, f) =
            observe(&mut n, &states, &items, ids::play::cb::COMMANDS, &arg_root).expect("recorded");
        assert_eq!(
            f["refused"],
            serde_json::json!("an argument node at root depth")
        );
    }

    /// Every kind in [`DECODED_KINDS`] must be sorted and unique, so the
    /// membership test `scenario::Scenario::load` runs is a set test and a
    /// human can find a name in it.
    #[test]
    fn the_decoded_kind_list_is_sorted_and_unique() {
        let mut sorted = DECODED_KINDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.as_slice(), DECODED_KINDS);
        // The three R2 arms are in it, which is what lets a check declare a
        // collapse on them without observing them.
        for k in ["rotate_head", "move_entity_rot", "update_attributes"] {
            assert!(DECODED_KINDS.contains(&k), "{k}");
        }
    }

    #[test]
    fn a_packet_whose_decoder_runs_off_the_end_is_still_recorded() {
        // The failure this protects against: a truncated read making a
        // packet disappear, so a server that spoke looks like one that
        // did not.
        let mut n = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        let (kind, f) = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::ADD_ENTITY,
            &[1, 2, 3],
        )
        .expect("recorded");
        assert_eq!(kind, "packet:0x01");
        assert_eq!(f["decode"], serde_json::json!("failed"));
        assert_eq!(f["rest"], serde_json::json!(digest(&[1, 2, 3])));
    }

    /// **Every classified family is recorded, under the kind its row names.**
    ///
    /// This test replaced `housekeeping_is_not_recorded`, which asserted the
    /// opposite and was the only mechanised statement of the defect: twelve
    /// packet types thrown away at record time, three stages before the
    /// projection that owns leniency.
    #[test]
    fn every_classified_family_records_under_its_declared_kind() {
        let mut n = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        for fam in FAMILIES {
            let Some(id) = fam.id else { continue };
            let (kind, f) = observe(&mut n, &states, &items, id, &[0; 16])
                .unwrap_or_else(|| panic!("{} was dropped at record time", fam.kind));
            assert_eq!(kind, fam.kind, "{} recorded under the wrong kind", fam.kind);
            assert!(f.contains_key("rest"), "{} kept no digest", fam.kind);
        }
    }

    /// **`Family::arm` is a claim about `decode` and this is the check.**
    ///
    /// A stale column would make a genuine decoder refusal read as "nobody
    /// has written an arm for this yet", which is the difference between a
    /// harness bug and a stated gap.
    ///
    /// It cannot be written as `decode(..).is_some()`: since `decode`'s
    /// catch-all names classified families too, every row returns `Some`.
    /// The observable difference is the `decode = "undecoded"` marker, which
    /// only the catch-all writes and no arm does.
    #[test]
    fn families_declare_their_own_arms() {
        let mut n = Normalizer::new([0, 0, 0]);
        let states = BTreeMap::new();
        let items = test_items();
        for fam in FAMILIES {
            let Some(id) = fam.id else { continue };
            // A 16-byte body of zeroes satisfies every arm this table names
            // (all of them read varints or fixed-width fields).
            let (kind, f, _) = decode(&mut n, &states, &items, id, &[0; 16])
                .unwrap_or_else(|| panic!("{} produced no kind at all", fam.kind));
            assert_eq!(kind, fam.kind, "{} decoded under the wrong kind", fam.kind);
            let undecoded =
                f.get("decode").and_then(serde_json::Value::as_str) == Some("undecoded");
            assert_eq!(
                undecoded,
                !fam.arm,
                "{}: table says arm = {}, decode {} the undecoded marker",
                fam.kind,
                fam.arm,
                if undecoded { "wrote" } else { "did not write" }
            );
        }
    }

    /// The classification is a partition: no id and no kind appears twice.
    #[test]
    fn the_classification_has_no_duplicate_rows() {
        let mut kinds: Vec<&str> = FAMILIES.iter().map(|f| f.kind).collect();
        kinds.sort_unstable();
        let n = kinds.len();
        kinds.dedup();
        assert_eq!(kinds.len(), n, "a kind is classified twice");
        let mut ids: Vec<i32> = FAMILIES.iter().filter_map(|f| f.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "an id is classified twice");
    }

    /// **A row with no reason is a classification with no measurement.**
    #[test]
    fn every_row_states_why() {
        for fam in FAMILIES {
            assert!(
                fam.reason.len() > 40,
                "{} carries no stated reason",
                fam.kind
            );
        }
    }

    #[test]
    fn ascii_runs_pick_out_a_message() {
        assert_eq!(ascii_runs(b"\x08\x00hello\x00\x01ab"), "hello");
    }

    /// One entry of a chunk payload's block-entity list.
    struct Be {
        packed: u8,
        y: i16,
        kind_id: i32,
        /// The tag, already encoded: `vec![0]` for absent.
        tag: Vec<u8>,
    }

    /// A `LEVEL_CHUNK_WITH_LIGHT` body with every section under test's control.
    ///
    /// `heightmaps`, `blob` and `light` are the three the arm drops; the
    /// block-entity list is the one it keeps.
    fn chunk_body(
        cx: i32,
        cz: i32,
        heightmaps: &[(i32, Vec<i64>)],
        blob: &[u8],
        bes: &[Be],
        light: &[u8],
    ) -> Vec<u8> {
        use rustmc_proto::varint::write_varint;
        let mut b = Vec::new();
        b.extend_from_slice(&cx.to_be_bytes());
        b.extend_from_slice(&cz.to_be_bytes());
        write_varint(&mut b, i32::try_from(heightmaps.len()).expect("small")).expect("buf");
        for (kind, longs) in heightmaps {
            write_varint(&mut b, *kind).expect("buf");
            write_varint(&mut b, i32::try_from(longs.len()).expect("small")).expect("buf");
            for l in longs {
                b.extend_from_slice(&l.to_be_bytes());
            }
        }
        write_varint(&mut b, i32::try_from(blob.len()).expect("small")).expect("buf");
        b.extend_from_slice(blob);
        write_varint(&mut b, i32::try_from(bes.len()).expect("small")).expect("buf");
        for be in bes {
            b.push(be.packed);
            b.extend_from_slice(&be.y.to_be_bytes());
            write_varint(&mut b, be.kind_id).expect("buf");
            b.extend_from_slice(&be.tag);
        }
        b.extend_from_slice(light);
        b
    }

    /// A nameless `CompoundTag` holding one `TAG_String` under `k`.
    fn nbt_one_string(k: &str, v: &str) -> Vec<u8> {
        let mut t = vec![10u8]; // TAG_Compound, nameless root
        t.push(8); // TAG_String
        t.extend_from_slice(&u16::try_from(k.len()).expect("short").to_be_bytes());
        t.extend_from_slice(k.as_bytes());
        t.extend_from_slice(&u16::try_from(v.len()).expect("short").to_be_bytes());
        t.extend_from_slice(v.as_bytes());
        t.push(0); // end of compound
        t
    }

    /// **The three sections this arm drops are out of the comparison, and the
    /// one it keeps is in it.**
    ///
    /// The first half alone would pass on a decoder that read nothing at all
    /// — an arm returning a constant observation makes every body identical —
    /// so the second half is what makes the first mean something. Each kept
    /// field is moved on its own: a block entity's position, its registry
    /// type, its tag, and the presence of a second entry.
    #[test]
    fn a_chunks_light_heightmaps_and_sections_are_not_part_of_the_observation() {
        let states = BTreeMap::new();
        let one_be = || {
            vec![Be {
                packed: 0x35, // x = 3, z = 5 inside the chunk
                y: 70,
                kind_id: 7,
                tag: vec![0],
            }]
        };
        let base = chunk_body(
            2,
            -3,
            &[(0, vec![1, 2, 3]), (2, vec![4])],
            &[0xAA; 32],
            &one_be(),
            &[0x11; 24],
        );
        let seen = |body: &[u8]| {
            let mut n = Normalizer::new([0, 0, 0]);
            observe(
                &mut n,
                &states,
                &BTreeMap::new(),
                ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
                body,
            )
        };
        let want = seen(&base).expect("recorded");
        assert_eq!(want.0, CHUNK_BLOCK_ENTITIES);

        // --- dropped: same observation ------------------------------------
        assert_eq!(
            want,
            seen(&chunk_body(
                2,
                -3,
                &[(1, vec![9, 9, 9, 9]), (3, vec![0])],
                &[0xAA; 32],
                &one_be(),
                &[0x11; 24],
            ))
            .expect("recorded"),
            "heightmaps must not reach the comparison"
        );
        assert_eq!(
            want,
            seen(&chunk_body(
                2,
                -3,
                &[(0, vec![1, 2, 3]), (2, vec![4])],
                &[0x5C; 61],
                &one_be(),
                &[0x11; 24],
            ))
            .expect("recorded"),
            "the section blob must not reach the comparison"
        );
        assert_eq!(
            want,
            seen(&chunk_body(
                2,
                -3,
                &[(0, vec![1, 2, 3]), (2, vec![4])],
                &[0xAA; 32],
                &one_be(),
                &[0x77; 400],
            ))
            .expect("recorded"),
            "the light payload must not reach the comparison, nor its digest"
        );

        // --- kept: a different observation, one field at a time -----------
        let moved = |bes: Vec<Be>| {
            seen(&chunk_body(
                2,
                -3,
                &[(0, vec![1, 2, 3]), (2, vec![4])],
                &[0xAA; 32],
                &bes,
                &[0x11; 24],
            ))
            .expect("recorded")
        };
        assert_ne!(
            want,
            moved(vec![Be {
                packed: 0x36, // z = 6
                y: 70,
                kind_id: 7,
                tag: vec![0],
            }]),
            "where the block entity is, is behaviour"
        );
        assert_ne!(
            want,
            moved(vec![Be {
                packed: 0x35,
                y: 71,
                kind_id: 7,
                tag: vec![0],
            }]),
            "and how high"
        );
        assert_ne!(
            want,
            moved(vec![Be {
                packed: 0x35,
                y: 70,
                kind_id: 8,
                tag: vec![0],
            }]),
            "and which type it is — the whole point of the arm"
        );
        assert_ne!(
            want,
            moved(vec![Be {
                packed: 0x35,
                y: 70,
                kind_id: 7,
                tag: nbt_one_string("front_text", "hello"),
            }]),
            "and what its update tag says"
        );
        assert_ne!(
            want,
            moved(vec![
                Be {
                    packed: 0x35,
                    y: 70,
                    kind_id: 7,
                    tag: vec![0],
                },
                Be {
                    packed: 0x00,
                    y: 65,
                    kind_id: 2,
                    tag: vec![0],
                },
            ]),
            "and how many there are"
        );

        // The chunk's own coordinates are kept too, because the position is
        // absolute before it is normalised.
        assert_ne!(
            want,
            seen(&chunk_body(
                3,
                -3,
                &[(0, vec![1, 2, 3]), (2, vec![4])],
                &[0xAA; 32],
                &one_be(),
                &[0x11; 24],
            ))
            .expect("recorded"),
            "a chunk one column east is a different place"
        );
    }

    /// The position arithmetic, against a hand-computed absolute block.
    ///
    /// `BlockEntityInfo.create` packs `sectionRelative(x) << 4 |
    /// sectionRelative(z)`, so the high nibble is x. Getting the two the wrong
    /// way round is invisible to the test above, which only asks whether two
    /// bodies differ.
    #[test]
    fn a_chunk_block_entitys_position_is_the_chunk_plus_the_packed_nibbles() {
        let states = BTreeMap::new();
        let mut n = Normalizer::new([32, 64, -48]);
        let body = chunk_body(
            2,
            -3,
            &[],
            &[],
            &[Be {
                packed: 0x35,
                y: 70,
                kind_id: 7,
                tag: vec![0],
            }],
            &[],
        );
        let (kind, f) = observe(
            &mut n,
            &states,
            &BTreeMap::new(),
            ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
            &body,
        )
        .expect("recorded");
        assert_eq!(kind, CHUNK_BLOCK_ENTITIES);
        // absolute (2*16 + 3, 70, -3*16 + 5) = (35, 70, -43), anchor (32, 64, -48)
        assert_eq!(f["at"], serde_json::json!([3, 6, 5]));
        assert_eq!(
            f["block_entities"],
            serde_json::json!([{
                "at": [3, 6, 5],
                "block_entity_type": 7,
                "tag": digest(&[0]),
            }])
        );
    }

    /// **A chunk with no block entities is recorded under its own kind.**
    ///
    /// Every chunk entering view sends one of these and in a superflat world
    /// nearly all are empty. It is recorded — nothing vanishes at record
    /// time — as [`CHUNK_NO_BLOCK_ENTITIES`], which `FAMILIES` grades: the
    /// census found it self-consistent on the step across two vanilla runs,
    /// and `548` names it.
    #[test]
    fn a_chunk_carrying_no_block_entities_is_recorded_under_its_own_kind() {
        let states = BTreeMap::new();
        let mut n = Normalizer::new([0, 0, 0]);
        let (kind, _) = observe(
            &mut n,
            &states,
            &BTreeMap::new(),
            ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
            &chunk_body(0, 0, &[(0, vec![1])], &[0; 8], &[], &[0x11; 24]),
        )
        .expect("an empty chunk must not vanish at record time");
        assert_eq!(kind, CHUNK_NO_BLOCK_ENTITIES);
        assert_eq!(
            family_of_kind(CHUNK_NO_BLOCK_ENTITIES).map(|f| f.class),
            Some(Class::Graded)
        );
        // But a chunk that carries one is graded, so the split above is about
        // the list being empty and not about the arm being dead.
        assert!(
            observe(
                &mut n,
                &states,
                &BTreeMap::new(),
                ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
                &chunk_body(
                    0,
                    0,
                    &[(0, vec![1])],
                    &[0; 8],
                    &[Be {
                        packed: 0,
                        y: 0,
                        kind_id: 0,
                        tag: vec![0],
                    }],
                    &[0x11; 24],
                ),
            )
            .map(|(k, _)| k)
            .as_deref()
                == Some(CHUNK_BLOCK_ENTITIES)
        );
    }

    /// Every NBT type the skipper claims to handle, nested, skipped exactly.
    ///
    /// The failure this guards against is silent: a width that is wrong by a
    /// byte does not fail here, it desynchronises the *next* block entity in
    /// the list and reports a plausible-looking wrong one.
    #[test]
    fn the_nbt_skipper_lands_exactly_on_the_end_of_every_tag_type() {
        // A nameless compound holding one of each scalar, each array, a list
        // of compounds, and a nested compound.
        let mut t: Vec<u8> = vec![10];
        let named = |t: &mut Vec<u8>, ty: u8, name: &str| {
            t.push(ty);
            t.extend_from_slice(&u16::try_from(name.len()).expect("short").to_be_bytes());
            t.extend_from_slice(name.as_bytes());
        };
        named(&mut t, 1, "b");
        t.push(7);
        named(&mut t, 2, "s");
        t.extend_from_slice(&(-2i16).to_be_bytes());
        named(&mut t, 3, "i");
        t.extend_from_slice(&7i32.to_be_bytes());
        named(&mut t, 4, "l");
        t.extend_from_slice(&9i64.to_be_bytes());
        named(&mut t, 5, "f");
        t.extend_from_slice(&1.5f32.to_be_bytes());
        named(&mut t, 6, "d");
        t.extend_from_slice(&1.5f64.to_be_bytes());
        named(&mut t, 7, "ba");
        t.extend_from_slice(&3i32.to_be_bytes());
        t.extend_from_slice(&[1, 2, 3]);
        named(&mut t, 8, "str");
        t.extend_from_slice(&2u16.to_be_bytes());
        t.extend_from_slice(b"hi");
        named(&mut t, 11, "ia");
        t.extend_from_slice(&2i32.to_be_bytes());
        t.extend_from_slice(&[0; 8]);
        named(&mut t, 12, "la");
        t.extend_from_slice(&1i32.to_be_bytes());
        t.extend_from_slice(&[0; 8]);
        named(&mut t, 9, "list");
        t.push(10); // list of compounds
        t.extend_from_slice(&2i32.to_be_bytes());
        for _ in 0..2 {
            t.push(3);
            t.extend_from_slice(&1u16.to_be_bytes());
            t.push(b'x');
            t.extend_from_slice(&1i32.to_be_bytes());
            t.push(0);
        }
        named(&mut t, 10, "nested");
        t.push(0);
        t.push(0); // end of root
        let n = t.len();
        t.extend_from_slice(b"SENTINEL");

        let mut r: &[u8] = &t;
        let ty = read_u8(&mut r).expect("type");
        assert_eq!(ty, 10);
        nbt_skip_payload(&mut r, ty, 0).expect("skipped");
        assert_eq!(r, b"SENTINEL", "the skipper stopped {n} bytes in, exactly");

        // An empty list of type 0 is legal; a non-empty one is not, and
        // `loadList` throws on it rather than reading anything.
        let mut ok: &[u8] = &[0u8, 0, 0, 0, 0];
        assert!(nbt_skip_payload(&mut ok, 9, 0).is_some());
        let mut bad: &[u8] = &[0u8, 0, 0, 0, 1];
        assert!(nbt_skip_payload(&mut bad, 9, 0).is_none());
        // And a type the format does not define is a refusal, not a guess.
        let mut unknown: &[u8] = &[0u8; 8];
        assert!(nbt_skip_payload(&mut unknown, 13, 0).is_none());
    }

    /// A chunk payload that runs off the end is still recorded, as a packet.
    ///
    /// The rule the whole design rests on: a harness bug must never surface
    /// as "the server said nothing".
    #[test]
    fn a_truncated_chunk_payload_is_still_recorded() {
        let states = BTreeMap::new();
        let mut n = Normalizer::new([0, 0, 0]);
        let mut body = chunk_body(
            0,
            0,
            &[],
            &[],
            &[Be {
                packed: 0,
                y: 0,
                kind_id: 3,
                tag: nbt_one_string("k", "v"),
            }],
            &[],
        );
        body.truncate(body.len() - 4);
        let (kind, f) = observe(
            &mut n,
            &states,
            &BTreeMap::new(),
            ids::play::cb::LEVEL_CHUNK_WITH_LIGHT,
            &body,
        )
        .expect("a short read must not make the packet vanish");
        assert_eq!(
            kind,
            format!("packet:0x{:02X}", ids::play::cb::LEVEL_CHUNK_WITH_LIGHT)
        );
        assert_eq!(f["decode"], serde_json::json!("failed"));
    }

    /// **Two sounds that differ only in their seed or their pitch are the
    /// same observation.**
    ///
    /// `Level.playSound` fills the packet's `long seed` from
    /// `soundSeedGenerator.nextLong()`, a fresh draw every time, so a trace
    /// that digested the body whole made every check naming `sound`
    /// permanently unmatchable — for us, and for vanilla against
    /// itself. This is the assertion that the seed is out of the comparison,
    /// and it is written as two bodies rather than as one because a decoder
    /// that merely *reads* the seed and leaves it in the digest would pass a
    /// single-body test.
    ///
    /// The pitch joined it on 2026-08-07 and **left again on 2026-08-17**,
    /// which is what this test now measures. It was never random for every
    /// event — a lever's is `powered ? 0.6 : 0.5` and a button's is 1.0 — but
    /// a chest's is `nextFloat() * 0.1 + 0.9`, and the corpus run of
    /// 2026-08-07 recorded **five different pitches for the single event
    /// `block.chest.open`** (0.9120, 0.9268, 0.9339, 0.9552, 0.9952). Dropping
    /// the field fixed that and cost the other half: for a lever the pitch is
    /// the *only* field that differs between the two directions of the event,
    /// so a lever's `sound` observation stopped saying which way it went.
    /// [`crate::pitch`] classifies the event instead of dropping the field.
    ///
    /// **This test would pass vacuously without the registry, so it installs
    /// one.** `pitch::observed_by_id` answers `None` for every event when the
    /// `minecraft:sound_event` registry has not been loaded — the deliberate
    /// safe default — and a test that asserted "no `pitch` key" against an
    /// empty registry would be asserting that it had not set anything up. That
    /// is the exact defect class `docs/AGENT-BRIEF.md` opens with, so the
    /// three classes are exercised against real names in one body.
    #[test]
    fn a_sounds_seed_is_dropped_and_its_pitch_is_classified() {
        // Ids chosen freely: the registry is injected, so these stand for
        // whatever the pinned version happens to number them.
        crate::pitch::install_registry(BTreeMap::from([
            (42, "minecraft:block.chest.open".to_string()),
            (50, "minecraft:block.lever.click".to_string()),
            (60, "minecraft:block.note_block.harp".to_string()),
        ]));

        fn sound_body(sound: i32, seed: i64, pitch: f32) -> Vec<u8> {
            let mut b = Vec::new();
            // The holder is `registry id + 1`, with 0 meaning inline.
            rustmc_proto::varint::write_varint(&mut b, sound + 1).expect("buf");
            rustmc_proto::varint::write_varint(&mut b, 3).expect("buf"); // source
            b.extend_from_slice(&(8_i32 * 4).to_be_bytes()); // x = 4.0
            b.extend_from_slice(&(8_i32 * 64).to_be_bytes()); // y = 64.0
            b.extend_from_slice(&(8_i32 * -2).to_be_bytes()); // z = -2.0
            b.extend_from_slice(&1.0_f32.to_be_bytes()); // volume
            b.extend_from_slice(&pitch.to_be_bytes());
            b.extend_from_slice(&seed.to_be_bytes());
            b
        }
        let states = BTreeMap::new();
        let items = test_items();
        let see = |sound: i32, seed: i64, pitch: f32| {
            let mut n = Normalizer::new([0, 0, 0]);
            observe(
                &mut n,
                &states,
                &items,
                ids::play::cb::SOUND,
                &sound_body(sound, seed, pitch),
            )
            .expect("recorded")
        };

        // ---- the seed, unchanged since 569323e -----------------------------
        let chest = see(42, 1, 0.9120);
        assert_eq!(chest.0, "sound");
        assert_eq!(
            chest,
            see(42, i64::MIN, 0.9120),
            "the seed must not reach the comparison"
        );

        // ---- a DRAWN pitch still collapses, which is why the field could be
        //      kept at all. All five of that morning's readings are one
        //      observation.
        for p in [0.9120_f32, 0.9268, 0.9339, 0.9552, 0.9952] {
            assert_eq!(chest, see(42, 7, p), "{p} is inside the chest's draw");
        }
        // ...but the RANGE is a check, which a blanket drop could not give.
        assert_ne!(
            chest,
            see(42, 1, 1.0),
            "a chest-open at a constant 1.0 is outside [0.9, 1.0) and must show"
        );

        // ---- **the lever now says which way it went.** This is the assertion
        //      the whole change exists for: before it, these two bodies
        //      decoded to byte-identical observations.
        let on = see(50, 1, 0.6);
        let off = see(50, 1, 0.5);
        assert_ne!(on, off, "a lever's two directions are two observations");
        assert_eq!(on.1["pitch"], serde_json::json!(f64::from(0.6_f32)));

        // ---- an unclassified event keeps the old behaviour exactly ---------
        let note = see(60, 1, 0.75);
        assert_eq!(note, see(60, 1, 1.5), "an unclassified pitch is dropped");
        assert!(
            !note.1.contains_key("pitch"),
            "the field is absent, not zeroed"
        );

        // ---- and the rest of the packet is still decoded, so none of the
        //      above is passing by decoding nothing.
        assert_eq!(chest.1["sound"], serde_json::json!(42));
        assert_eq!(chest.1["source"], serde_json::json!(3));
        assert_eq!(chest.1["volume"], serde_json::json!(1.0));
        assert_eq!(chest.1["at"], serde_json::json!([4.0, 64.0, -2.0]));
        let mut quieter = sound_body(42, 1, 0.9120);
        // volume, pitch, seed are the last 4 + 4 + 8 bytes.
        let at = quieter.len() - 16;
        quieter[at..at + 4].copy_from_slice(&0.5_f32.to_be_bytes());
        let mut n = Normalizer::new([0, 0, 0]);
        let quieter =
            observe(&mut n, &states, &items, ids::play::cb::SOUND, &quieter).expect("recorded");
        assert_ne!(chest, quieter, "volume is behaviour and stays compared");
    }

    /// **Two `add_entity`s that differ only in their movement are the same
    /// observation, and two that differ in a rotation are not.** ORACLE
    /// rule 26.
    ///
    /// `ClientboundAddEntityPacket` carries the entity's
    /// `ServerEntity.lastSentMovement`, and that value is nobody's behaviour
    /// at the moment an entity is *announced*: for a standing player it
    /// depends on which tick vanilla's last motion packet happened to go out
    /// on, and for a thrown item it is four fresh `nextFloat()` draws from an
    /// unseeded `RandomSource`. It cost `530` a `match` floor and put
    /// `dropping-an-item-throws-it-on-the-floor` in the unstable file at
    /// 25/25.
    ///
    /// **The second half of this test is the half that matters.** An
    /// exemption is only honest if it is exactly as wide as it claims, and a
    /// decoder that simply stopped reading after the position — or one that
    /// consumed the wrong number of bytes for a `LpVec3` — would pass the
    /// first assertion and destroy the rest of the packet. So a body
    /// differing in `yRot` alone must still compare different.
    ///
    /// The movement bodies are chosen to be different *lengths*: `Vec3.ZERO`
    /// is a single zero byte and anything else is six or more. A decoder that
    /// skipped a fixed width would shift the rotations under one of them and
    /// fail the third assertion.
    #[test]
    fn an_add_entitys_movement_is_named_and_the_rest_of_the_body_survives_it() {
        // `id`, `uuid`, `type`, `x`, `y`, `z`, movement, xRot, yRot,
        // yHeadRot, `data` — the order `javap -c` gives for
        // `ClientboundAddEntityPacket.write` in 26.2.
        fn add_body(movement: &[u8], y_rot: u8) -> Vec<u8> {
            let mut b = Vec::new();
            rustmc_proto::varint::write_varint(&mut b, 9).expect("buf"); // entity id
            b.extend_from_slice(&[0xAB; 16]); // uuid
            rustmc_proto::varint::write_varint(&mut b, 148).expect("buf"); // type
            b.extend_from_slice(&4.0_f64.to_be_bytes());
            b.extend_from_slice(&64.0_f64.to_be_bytes());
            b.extend_from_slice(&(-2.0_f64).to_be_bytes());
            b.extend_from_slice(movement);
            b.push(0); // xRot
            b.push(y_rot);
            b.push(0); // yHeadRot
            rustmc_proto::varint::write_varint(&mut b, 0).expect("buf"); // data
            b
        }
        // `Vec3.ZERO`: the whole encoding is one zero byte.
        let still: &[u8] = &[0];
        // A moving entity: `b0` with the low three bits carrying scale 1 and
        // bit 2 clear (so no varint continuation), then `b1` and a
        // big-endian `u32`. Six bytes, five more than `still`.
        let moving: &[u8] = &[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB];

        let states = BTreeMap::new();
        let items = test_items();
        let mut a = Normalizer::new([0, 0, 0]);
        let mut c = Normalizer::new([0, 0, 0]);
        let one = observe(
            &mut a,
            &states,
            &items,
            ids::play::cb::ADD_ENTITY,
            &add_body(still, 0),
        )
        .expect("recorded");
        let two = observe(
            &mut c,
            &states,
            &items,
            ids::play::cb::ADD_ENTITY,
            &add_body(moving, 0),
        )
        .expect("recorded");
        assert_eq!(one.0, "add_entity");
        // **This assertion inverted on 2026-08-18 and the comparison did not
        // change.** It read `assert_eq!(one, two)` — "the movement must not
        // reach the comparison" — while the arm above consumed the three
        // packed shorts and emitted nothing. The removal now happens in
        // `compare::drop_add_entity_movement`, so the *recording* carries the
        // field and the *comparison* still does not. The half that used to be
        // asserted here is asserted there, by
        // `compare::tests::an_add_entity_movement_is_dropped_by_the_projection`.
        //
        // What is asserted here instead is the half nothing asserted before:
        // that the two bodies decode to two different values. A decoder that
        // read the variable-length movement wrongly and produced one value for
        // both would have passed the old assertion **for the wrong reason** —
        // it was an equality, and every way of failing to decode satisfies it.
        assert_ne!(
            one.1["movement"], two.1["movement"],
            "a still entity and a moving one must decode to different movements",
        );
        assert_eq!(one.1["movement"], serde_json::json!([0.0, 0.0, 0.0]));

        // Not passing by decoding nothing: the fields that are behaviour
        // survive.
        assert_eq!(one.1["kind_id"], serde_json::json!(148));
        assert_eq!(one.1["at"], serde_json::json!([4.0, 64.0, -2.0]));

        // And the exemption is exactly this wide. Both movement encodings are
        // tried, so a decoder that got the *variable* length wrong is caught
        // as well as one that read past the movement entirely.
        for m in [still, moving] {
            let mut x = Normalizer::new([0, 0, 0]);
            let mut y = Normalizer::new([0, 0, 0]);
            let straight = observe(
                &mut x,
                &states,
                &items,
                ids::play::cb::ADD_ENTITY,
                &add_body(m, 0),
            )
            .expect("recorded");
            let turned = observe(
                &mut y,
                &states,
                &items,
                ids::play::cb::ADD_ENTITY,
                &add_body(m, 64),
            )
            .expect("recorded");
            assert_ne!(
                straight,
                turned,
                "a rotation must still be compared, movement length {}",
                m.len()
            );
            // The rest of the body must survive both movement encodings
            // intact. This is what catches a variable-length read that is
            // wrong by a byte: the reader would then be misaligned and every
            // field after the movement would be rubbish, which the equality
            // above cannot see because it is comparing two rubbish reads.
            assert_eq!(straight.1["x_rot"], serde_json::json!(0));
            assert_eq!(straight.1["data"], serde_json::json!(0));
            assert_eq!(straight.1["at"], serde_json::json!([4.0, 64.0, -2.0]));
            assert_eq!(straight.1["rest"], serde_json::json!("cbf29ce484222325"));
        }

        // **The rotations and `data` are named, and the tail is then empty.**
        //
        // Added with the item-yaw exemption in `compare::project`, which can
        // only be one field wide if the fields exist. Two assertions, and the
        // second is the one that would catch a decoder reading one byte too
        // few: `rest` is the digest of an empty slice for a body that ends at
        // `data`, so anything left over shows up here rather than silently
        // sitting in the digest where the exemption cannot reach it.
        let mut z = Normalizer::new([0, 0, 0]);
        let named = observe(
            &mut z,
            &states,
            &items,
            ids::play::cb::ADD_ENTITY,
            &add_body(moving, 64),
        )
        .expect("recorded");
        assert_eq!(named.1["x_rot"], serde_json::json!(0));
        assert_eq!(named.1["y_rot"], serde_json::json!(64));
        assert_eq!(named.1["y_head_rot"], serde_json::json!(0));
        assert_eq!(named.1["data"], serde_json::json!(0));
        assert_eq!(
            named.1["rest"],
            serde_json::json!(digest(&[])),
            "the decoder consumes the whole body, so nothing is left to digest"
        );

        // The angles are **signed** bytes, as `writeByte` on a `B` field
        // writes them: 192 on the wire is -64, not 192. A decoder that read
        // them unsigned would compare equal on both sides and would still put
        // a wrong number in every failure report.
        let mut w = Normalizer::new([0, 0, 0]);
        let back = observe(
            &mut w,
            &states,
            &items,
            ids::play::cb::ADD_ENTITY,
            &add_body(moving, 192),
        )
        .expect("recorded");
        assert_eq!(back.1["y_rot"], serde_json::json!(-64));
    }

    /// **The two entity ids a damage event carries beyond its victim are
    /// normalised, not digested.**
    ///
    /// Written as two bodies with the same *shape* and different raw ids,
    /// which is the only form that fails against the old decoder: it read
    /// `entityId` and the damage type and stopped, so `sourceCauseId` and
    /// `sourceDirectId` went into the trailing digest raw. Entity ids are a
    /// counter no two runs agree about, so the first check ever written that
    /// observed `damage_event` would have been unmatchable for a reason with
    /// nothing to do with damage.
    #[test]
    fn a_damage_events_cause_and_direct_entities_are_normalised() {
        fn damage_body(victim: i32, cause: i32, direct: i32) -> Vec<u8> {
            let mut b = Vec::new();
            let w = |b: &mut Vec<u8>, v: i32| {
                rustmc_proto::varint::write_varint(b, v).expect("buf");
            };
            w(&mut b, victim);
            w(&mut b, 8); // damage type holder, registry id 7
            w(&mut b, cause + 1); // writeOptionalEntityId
            w(&mut b, direct + 1);
            b.push(0); // Optional<Vec3>: absent
            b
        }
        let states = BTreeMap::new();
        let items = test_items();
        let mut a = Normalizer::new([0, 0, 0]);
        let mut c = Normalizer::new([0, 0, 0]);
        let one = observe(
            &mut a,
            &states,
            &items,
            ids::play::cb::DAMAGE_EVENT,
            &damage_body(11, 12, 12),
        )
        .expect("recorded");
        let two = observe(
            &mut c,
            &states,
            &items,
            ids::play::cb::DAMAGE_EVENT,
            &damage_body(900, 901, 901),
        )
        .expect("recorded");
        assert_eq!(one.0, "damage_event");
        // Compared **as projected**, which is where the numbering happens
        // now. Raw ids differ in the recording, by design; what must not
        // differ is what the comparison sees.
        let (pone, ptwo) = (projected(one.clone()), projected(two));
        assert_eq!(pone, ptwo, "raw entity ids must not reach the comparison");
        // Order of first sighting, not the raw ids — and **within one packet
        // that order is by field name**, because `Obs::fields` is a
        // `BTreeMap` and insertion order is not something a recording keeps.
        // So `cause` is numbered before `entity` here even though the victim
        // comes first on the wire. That is a fixed rule rather than a wire
        // accident, both servers apply it identically, and the property the
        // numbering exists for is untouched: the direct entity is the same
        // object as the cause, so it carries the same number.
        let pf = &pone["fields"];
        assert_eq!(pf["cause"], serde_json::json!(0));
        assert_eq!(pf["direct"], serde_json::json!(0));
        assert_eq!(pf["entity"], serde_json::json!(1));
        assert_eq!(pf["source"], serde_json::json!(8));

        // -1 on the wire is `writeOptionalEntityId(-1)` = varint 0, and means
        // "no entity". It must not be normalised as if it were entity 0.
        let mut d = Normalizer::new([0, 0, 0]);
        let none = observe(
            &mut d,
            &states,
            &items,
            ids::play::cb::DAMAGE_EVENT,
            &damage_body(5, -1, -1),
        )
        .expect("recorded");
        assert_eq!(none.1["cause"], serde_json::Value::Null);
        assert_eq!(none.1["direct"], serde_json::Value::Null);

        // And the position, when present, is anchored like every other one.
        let mut e = Normalizer::new([10, 0, 0]);
        let mut body = damage_body(5, -1, -1);
        body.pop();
        body.push(1);
        for v in [12.0_f64, 64.0, -2.0] {
            body.extend_from_slice(&v.to_be_bytes());
        }
        let at =
            observe(&mut e, &states, &items, ids::play::cb::DAMAGE_EVENT, &body).expect("recorded");
        assert_eq!(at.1["at"], serde_json::json!([2.0, 64.0, -2.0]));
    }

    // ======================================================================
    // ItemStack. The tests below are mostly about the decoder REFUSING.
    //
    // A stack decoder that answers something for every input would be worse
    // than the digest it replaced: `rest` was opaque and said so, whereas an
    // empty stack invented for bytes nobody parsed reads exactly like a
    // server that sent an empty stack. So the falsification comes first and
    // the happy path is the control.
    // ======================================================================

    /// `container_set_data` names its menu in a `window` field, so the
    /// per-connection menu counter is rebased rather than digested, and its two
    /// shorts are read signed as `readShort` reads them.
    #[test]
    fn container_set_data_names_its_window_and_reads_two_signed_shorts() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut b = vi(37);
        b.extend(2_u16.to_be_bytes());
        b.extend((-5_i16).to_be_bytes());
        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_DATA,
            &b,
        )
        .expect("recorded");
        assert_eq!(o.0, "container_set_data");
        assert_eq!(o.1["window"], serde_json::json!(37));
        assert_eq!(o.1["id"], serde_json::json!(2));
        assert_eq!(o.1["value"], serde_json::json!(-5));
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[])));
    }

    /// A waypoint's position is anchor-relative, block or chunk, so one
    /// scenario recorded at two different workspaces is one observation; and
    /// the whole body is read, so nothing is left in `rest`.
    #[test]
    fn a_waypoint_position_is_relative_to_the_anchor() {
        let states = BTreeMap::new();
        let items = test_items();
        let body = |ty: i32, tail: &[u8]| {
            let mut b = vi(0); // TRACK
            b.push(1); // Either.left: a UUID
            b.extend(0x0123_i64.to_be_bytes());
            b.extend(0x4567_i64.to_be_bytes());
            b.extend(vi(17));
            b.extend_from_slice(b"minecraft:default");
            b.push(1); // a colour
            b.extend([1, 2, 3]);
            b.extend(vi(ty));
            b.extend_from_slice(tail);
            b
        };
        let decode = |anchor: [i32; 3], b: &[u8]| {
            let mut n = Normalizer::new(anchor);
            observe(&mut n, &states, &items, ids::play::cb::WAYPOINT, b).expect("recorded")
        };
        let vec_at = |x: i32| {
            let mut t = vi(x);
            t.extend(vi(-60));
            t.extend(vi(5));
            t
        };
        // The same waypoint 32 * 90 blocks further west, from an anchor moved
        // with it, is the same observation.
        let near = decode([-32, -60, 0], &body(1, &vec_at(-31)));
        let far = decode([-2912, -60, 0], &body(1, &vec_at(-2911)));
        assert_eq!(near.0, "waypoint");
        assert_eq!(near.1, far.1);
        assert_eq!(near.1["vec"], serde_json::json!([1, 0, 5]));
        assert_eq!(
            near.1["uuid"],
            serde_json::json!("00000000000001230000000000004567")
        );
        assert_eq!(near.1["color"], serde_json::json!([1, 2, 3]));
        assert_eq!(near.1["rest"], serde_json::json!(digest(&[])));
        // CHUNK: relative to the anchor's chunk.
        let mut t = vi(-2);
        t.extend(vi(0));
        let c = decode([-32, -60, 0], &body(2, &t));
        assert_eq!(
            (c.1["chunk_x"].clone(), c.1["chunk_z"].clone()),
            (serde_json::json!(0), serde_json::json!(0))
        );
        // AZIMUTH: a float, and no `at` — the scenario box must not bound it.
        let a = decode([-32, -60, 0], &body(3, &1.5_f32.to_be_bytes()));
        assert_eq!(a.1["azimuth"], serde_json::json!(1.5));
        assert!(!a.1.contains_key("at") && !near.1.contains_key("at"));
    }

    /// An empty stack and an unreadable one must never look alike.
    ///
    /// This is the failure the whole design is built against: `count <= 0` is
    /// vanilla's own encoding of empty, and a decoder that also answers
    /// "empty" when it runs out of bytes has made the one observation nobody
    /// can act on.
    #[test]
    fn an_empty_stack_and_an_unreadable_one_are_not_the_same_observation() {
        let states = BTreeMap::new();
        let items = test_items();

        let mut n = Normalizer::default();
        let empty = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &stack(0, 0)),
        )
        .expect("recorded");
        assert_eq!(empty.0, "container_set_slot");
        assert_eq!(empty.1["item"], serde_json::json!({ "count": 0 }));
        // Fully legible: nothing was left for the digest to cover.
        assert_eq!(empty.1["rest"], serde_json::json!(digest(&[])));

        // The same packet with the stack cut off after its count byte.
        let mut n = Normalizer::default();
        let truncated = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &vi(1)),
        )
        .expect("recorded");
        assert_eq!(truncated.0, "packet:0x14");
        assert_eq!(truncated.1["decode"], serde_json::json!("failed"));
        assert!(!truncated.1.contains_key("item"), "{:?}", truncated.1);
    }

    /// `IdMap.byIdOrThrow` throws; so does this.
    ///
    /// The positive control is the same packet with a real id, so a failure
    /// here cannot be read as "the decoder refuses everything".
    #[test]
    fn an_item_id_no_registry_holds_is_refused_rather_than_named() {
        let states = BTreeMap::new();
        let items = test_items();

        let mut n = Normalizer::default();
        let bad = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &stack(1, 9999)),
        )
        .expect("recorded");
        assert_eq!(bad.0, "packet:0x14");
        assert_eq!(bad.1["decode"], serde_json::json!("failed"));

        let mut n = Normalizer::default();
        let good = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &stack(1, 155)),
        )
        .expect("recorded");
        assert_eq!(good.0, "container_set_slot");
        assert_eq!(
            good.1["item"],
            serde_json::json!({ "count": 1, "item_id": 155, "item": "minecraft:chest" })
        );
    }

    /// A negative patch count is refused, and it is not reachable by encoding.
    #[test]
    fn a_negative_component_count_is_refused() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut tail = vi(1);
        tail.extend(vi(155));
        tail.extend(vi(-1));
        tail.extend(vi(0));
        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &tail),
        )
        .expect("recorded");
        assert_eq!(o.1["decode"], serde_json::json!("failed"));
    }

    /// **The bytes this decoder cannot read are still compared.**
    ///
    /// Stopping at an added component is only honest if the payload stays in
    /// `rest`. Two stacks differing in nothing but that payload must produce
    /// two different observations — otherwise the stop has quietly deleted
    /// a comparison the digest used to make.
    #[test]
    fn an_opaque_component_payload_is_still_in_the_digest() {
        let states = BTreeMap::new();
        let items = test_items();
        let one = slot_body(3, &stack_with_component(1, 155, 12, &[0xAA, 0xBB]));
        let two = slot_body(3, &stack_with_component(1, 155, 12, &[0xAA, 0xBC]));

        let mut a = Normalizer::default();
        let mut b = Normalizer::default();
        let o = observe(
            &mut a,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &one,
        )
        .expect("recorded");
        let p = observe(
            &mut b,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &two,
        )
        .expect("recorded");

        assert_eq!(o.0, "container_set_slot");
        assert_eq!(o.1["item"]["opaque_component"], serde_json::json!(12));
        assert_eq!(o.1["item"]["comp_added"], serde_json::json!(1));
        // Same head, same blocking component, different payload.
        assert_eq!(o.1["item"], p.1["item"]);
        assert_ne!(o.1["rest"], p.1["rest"]);
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[0xAA, 0xBB])));
    }

    /// Removals are self-delimiting, so a removal-only patch is fully legible.
    #[test]
    fn a_removal_only_patch_is_read_to_the_end() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut tail = vi(2);
        tail.extend(vi(1));
        tail.extend(vi(0));
        tail.extend(vi(2));
        tail.extend(vi(41));
        tail.extend(vi(6));
        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(3, &tail),
        )
        .expect("recorded");
        assert_eq!(o.1["item"]["removed"], serde_json::json!([41, 6]));
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[])));
    }

    /// One opaque stack blinds the rest of its packet, and the trace says so.
    #[test]
    fn an_opaque_stack_truncates_the_content_list_visibly() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut body = vi(1);
        body.extend(vi(7));
        body.extend(vi(3));
        body.extend(stack(1, 1));
        body.extend(stack_with_component(2, 155, 12, &[0x01]));
        body.extend(stack(4, 1));
        body.extend(stack(1, 155)); // the carried item, now unreachable

        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_CONTENT,
            &body,
        )
        .expect("recorded");
        assert_eq!(o.0, "container_set_content");
        assert_eq!(o.1["slots"].as_array().expect("array").len(), 2);
        assert_eq!(o.1["truncated"], serde_json::json!(true));
        assert!(!o.1.contains_key("carried"), "{:?}", o.1);
        assert_ne!(o.1["rest"], serde_json::json!(digest(&[])));

        // The control: with no component anywhere, the same packet is read
        // whole — list, carried item and all.
        let mut body = vi(1);
        body.extend(vi(7));
        body.extend(vi(2));
        body.extend(stack(1, 1));
        body.extend(stack(4, 1));
        body.extend(stack(1, 155));
        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_CONTENT,
            &body,
        )
        .expect("recorded");
        assert_eq!(o.1["slots"].as_array().expect("array").len(), 2);
        assert_eq!(o.1["carried"]["item"], serde_json::json!("minecraft:chest"));
        assert!(!o.1.contains_key("truncated"), "{:?}", o.1);
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[])));
    }

    /// `set_equipment` walks its chain to the entry with the high bit clear.
    #[test]
    fn set_equipment_reads_every_entry_in_its_chain() {
        let states = BTreeMap::new();
        let items = test_items();
        let mut body = vi(41);
        body.push(0x80 | 2);
        body.extend(stack(1, 155));
        body.push(5);
        body.extend(stack(0, 0));
        let mut n = Normalizer::default();
        let o = observe(&mut n, &states, &items, ids::play::cb::SET_EQUIPMENT, &body)
            .expect("recorded");
        assert_eq!(o.0, "set_equipment");
        let slots = o.1["slots"].as_array().expect("array");
        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0]["slot"], serde_json::json!(2));
        assert_eq!(
            slots[0]["item"]["item"],
            serde_json::json!("minecraft:chest")
        );
        assert_eq!(slots[1]["slot"], serde_json::json!(5));
        assert_eq!(slots[1]["item"], serde_json::json!({ "count": 0 }));
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[])));
    }

    /// The two packets that were wholly opaque now have names.
    ///
    /// `packet:0x60` and `packet:0x6C` are the literal kinds two checks in
    /// `ci/checks/gameplay/` observe today; this is the arm check 720's own
    /// header says to write.
    #[test]
    fn the_cursor_and_player_inventory_packets_are_no_longer_opaque() {
        let states = BTreeMap::new();
        let items = test_items();

        let mut n = Normalizer::default();
        let cursor = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::SET_CURSOR_ITEM,
            &stack(3, 155),
        )
        .expect("recorded");
        assert_eq!(cursor.0, "set_cursor_item");
        assert_eq!(cursor.1["item"]["count"], serde_json::json!(3));
        assert_eq!(cursor.1["rest"], serde_json::json!(digest(&[])));

        let mut body = vi(9);
        body.extend(stack(1, 1));
        let mut n = Normalizer::default();
        let inv = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::SET_PLAYER_INVENTORY,
            &body,
        )
        .expect("recorded");
        assert_eq!(inv.0, "set_player_inventory");
        assert_eq!(inv.1["slot"], serde_json::json!(9));
        assert_eq!(inv.1["item"]["item"], serde_json::json!("minecraft:stone"));
    }

    /// **The wire form, checked against a real vanilla packet — no boot, no
    /// jar, and not one byte written by this project.**
    ///
    /// `ci/checks/gameplay/350-a-dropped-item-goes-into-your-inventory.toml`
    /// recorded, on 2026-08-03, that vanilla's `container_set_slot` in that
    /// scenario carried `rest = d2fbb5fb1fa81421`. That measurement was taken
    /// when the arm consumed only `window`, `state` and `slot`, so the
    /// digested tail **is** the `ItemStack` and nothing else — which makes
    /// that hex string a golden vector for this decoder, written down by a
    /// recorder that had no idea it was one.
    ///
    /// Searching every `(count, item id)` under the form transcribed from
    /// `ItemStack$1.decode` — count, item, then the two-zero empty patch —
    /// gives exactly one preimage of that digest: **count 2, item 350**. Item
    /// 350 in the pinned registry is `minecraft:torch`, and the scenario's own
    /// `[[setup]]` holds `minecraft:torch`. Three independent things — the
    /// bytecode, a digest recorded nine days before this decoder existed, and
    /// a scenario's declared item — agree, and a wrong field order or a
    /// holder-with-tag id would have produced no preimage at all.
    #[test]
    fn the_stack_form_reproduces_a_digest_recorded_by_vanilla() {
        // count 2; item 350 as a VarInt; empty component patch.
        let recorded = [0x02u8, 0xDE, 0x02, 0x00, 0x00];
        assert_eq!(digest(&recorded), "d2fbb5fb1fa81421");
        assert_eq!(&recorded[..], &stack(2, 350)[..]);

        let states = BTreeMap::new();
        let items: BTreeMap<i32, String> =
            [(350, "minecraft:torch".to_owned())].into_iter().collect();
        let mut n = Normalizer::default();
        let o = observe(
            &mut n,
            &states,
            &items,
            ids::play::cb::CONTAINER_SET_SLOT,
            &slot_body(0, &recorded),
        )
        .expect("recorded");
        assert_eq!(
            o.1["item"],
            serde_json::json!({ "count": 2, "item_id": 350, "item": "minecraft:torch" })
        );
        assert_eq!(o.1["rest"], serde_json::json!(digest(&[])));
    }

    /// Every kind the stack arms can produce is declared, or a check naming
    /// it refuses to load. The list is hand-kept; this is what pins it.
    #[test]
    fn the_new_stack_kinds_are_declared() {
        for k in ["set_cursor_item", "set_player_inventory"] {
            assert!(DECODED_KINDS.contains(&k), "{k} missing from DECODED_KINDS");
        }
    }

    /// **The opaque arm is reachable through the harness's own encoder, and
    /// goes unreachable again the moment `Step::Hold` stops writing
    /// components.**
    ///
    /// This is the half of the 860/861/862 experiment that needs no vanilla
    /// and no corpus lock. `PARITY-ROADMAP` §44.5 says `0 of 539 opaque` was a
    /// property of the harness rather than a reading of vanilla — that
    /// `Step::Hold` wrote a literal `0 added`, so no recording could ever have
    /// contained a component-bearing stack. The claim is falsifiable without a
    /// server: chain the real writer to the real reader and watch the count
    /// move.
    ///
    /// Intact, a `[damage 5]` patch decodes to `Opaque` carrying
    /// `opaque_component` — the field `ci/vanilla-reach-census.py` tallies as
    /// `item_opaque`. Force the components to empty, which is the pre-2026-08-13
    /// behaviour exactly, and the same stack decodes to `Done` with no such
    /// field, so the census would count zero however many times it ran.
    ///
    /// **What this does NOT establish is agreement with vanilla**, and it
    /// cannot: both ends here are ours, so a shared misreading of
    /// `DataComponentPatch$3.encode` would pass. That is what recording 861
    /// against the reference is for. Reachability is provable here; agreement
    /// is not.
    #[test]
    fn the_opaque_arm_is_reachable_through_hold_and_only_because_of_components() {
        use crate::client::Tables;
        use rustmc_proto::varint::write_varint;

        let tables = Tables {
            items: BTreeMap::from([("minecraft:diamond_sword".to_string(), 909)]),
            item_names: BTreeMap::from([(909, "minecraft:diamond_sword".to_string())]),
            states: BTreeMap::new(),
            entity_types: BTreeMap::new(),
            component_types: BTreeMap::from([("minecraft:damage".to_string(), 3)]),
            block_entity_types: BTreeMap::new(),
        };

        // **CLIENTBOUND shape, built here rather than with
        // `client::write_component_patch`.**
        //
        // The first version of this test chained that writer to this reader
        // and passed. It was wrong: the writer emits the SERVERBOUND
        // *delimited* patch, where every added value carries a VarInt
        // byte-length, and this reader reads the CLIENTBOUND plain patch,
        // where none do. Both ends were ours, so the mismatch was invisible
        // until vanilla refused the packet (`docs/PARITY-ROADMAP.md` §44.7).
        // The writer is now pinned against vanilla's own bytes in
        // `client.rs::write_component_patch_matches_vanillas_delimited_bytes`;
        // what is tested here is the reader, and only the reader.
        let stack = |added: &[(i32, i32)]| {
            let mut b = Vec::new();
            write_varint(&mut b, 1).unwrap(); // count
            write_varint(&mut b, 909).unwrap(); // minecraft:diamond_sword
            write_varint(&mut b, i32::try_from(added.len()).unwrap()).unwrap();
            write_varint(&mut b, 0).unwrap(); // none removed
            for (id, value) in added {
                write_varint(&mut b, *id).unwrap();
                write_varint(&mut b, *value).unwrap();
            }
            b
        };

        let carried = stack(&[(3, 5)]);
        let mut r = carried.as_slice();
        match read_item_stack(&mut r, &tables.item_names) {
            StackRead::Opaque(v) => {
                assert_eq!(
                    v.get("opaque_component")
                        .and_then(serde_json::Value::as_i64),
                    Some(3),
                    "the census tallies `opaque_component`; it must name the damage type"
                );
                assert_eq!(
                    v.get("comp_added").and_then(serde_json::Value::as_i64),
                    Some(1)
                );
            }
            other => panic!("a component-bearing stack must stop the decoder, got {other:?}"),
        }

        // The mutation: `Step::Hold` as it behaved until 2026-08-13.
        let bare = stack(&[]);
        let mut r = bare.as_slice();
        match read_item_stack(&mut r, &tables.item_names) {
            StackRead::Done(v) => assert!(
                v.get("opaque_component").is_none(),
                "with no components the arm must be unreachable, which is why the \
                 count was 0 for 539 stacks: {v:?}"
            ),
            other => panic!("a bare stack must decode whole, got {other:?}"),
        }

        // And the two differ only in the patch, so nothing else explains it.
        assert_eq!(
            &carried[..bare.len() - 2],
            &bare[..bare.len() - 2],
            "the two stacks must share a head; only the patch may differ"
        );
    }
}
