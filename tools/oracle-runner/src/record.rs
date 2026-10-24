//! Running a corpus against one server and writing down what it said.
//!
//! The same code records vanilla and rustmc. That is not tidiness: if the
//! reference were produced by a different program from the subject, every
//! difference between the two programs would arrive as a gameplay
//! divergence, and the harness would be measuring itself.
//!
//! # Two clients, and why
//!
//! An **actor** performs the scenario's steps. An **observer** stands a few
//! blocks away and does nothing whatsoever. Both are recorded.
//!
//! The observer is not redundancy, it is the only way to see the world.
//! Vanilla does not tell the acting player what changed — the client
//! predicted its own dig or placement and vanilla sends only the
//! acknowledgement, because there is nothing to correct. Measured against
//! real vanilla on 2026-08-02: breaking a block produced **nothing at all**
//! on the actor's socket. A client that took no action has no prediction to
//! keep, so the server must tell it everything.
//!
//! The actor's stream is still recorded because some things are only ever
//! sent to it: the acknowledgement itself, a screen opening, its own
//! inventory.

use crate::client::{login_to_play, status, Session, Tables};
use crate::scenario::Scenario;
use crate::trace::{Normalizer, Obs, ScenarioTrace, Trace, Who};
use std::io;
use std::time::{Duration, Instant};

/// How far apart two scenarios work.
///
/// Bigger than a server's view distance on purpose. Blocks 32 apart cannot
/// affect each other, and — the case that actually bites — a dropped item or
/// a mob from one scenario cannot wander into the next one's measurements.
/// On superflat this costs nothing: every column is identical, so the
/// hundredth workspace is the same situation as the first.
const WORKSPACE_SPACING: f64 = 32.0;

/// Where the observer stands relative to the actor: close enough to be
/// tracking the same chunks, far enough not to be in the way of a placement.
const OBSERVER_OFFSET: f64 = 5.0;

/// How long the world has to be quiet before a measured window opens.
const QUIET: Duration = Duration::from_millis(250);
const QUIET_CAP: Duration = Duration::from_millis(4000);

/// How long a console command is given to be read and executed.
///
/// The fifo is the server's stdin and its reader is the main thread, which
/// drains pending console commands once per tick. Five ticks is 250 ms; this
/// is 400, which is two and a half times the margin and still small beside a
/// `settle_ms`. It is spent twice per declared rule per scenario and by no
/// scenario that declares none.
const CONSOLE_SETTLE: Duration = Duration::from_millis(400);

/// A handle on the reference server's console fifo.
///
/// # Why a fifo and not a command
///
/// `Step::Command` exists and would be the obvious way to set a game rule
/// from inside a scenario. It cannot: `gamerule` is permission level 2, the
/// harness's two players are `oracle` and `oracle_eye` with no `ops.json`
/// anywhere in `ci/vanilla-server.sh`, and vanilla answers an unprivileged
/// `gamerule` with a refusal in chat and changes nothing. Op-ing them would
/// work and was rejected: `PlayerList.placeNewPlayer` calls
/// `sendPlayerPermissionLevel`, which sends an `entity_event` carrying
/// `24 + level` **and** re-sends the whole `ClientboundCommandsPacket` tree
/// filtered by the new level, so op-ing changes join traffic for all
/// ninety-two existing checks to buy a facility three of them need.
///
/// The fifo changes nothing for a scenario that does not declare a rule,
/// which is the property that matters here.
///
/// # It is the reference's console, and there is no equivalent for ours
///
/// `ci/t2-gameplay.sh` has always sent its four `gamerule`s to `$VAN_DIR`
/// only. See [`crate::scenario::GameRule`] for what that asymmetry costs and
/// why it is declared rather than hidden.
pub struct Console {
    path: std::path::PathBuf,
}

impl Console {
    /// Open a handle on a console fifo.
    ///
    /// # Errors
    /// If the path is not a fifo, or does not exist. A console that is not
    /// there is refused now rather than discovered as a scenario silently
    /// running on the wrong substrate — which is the failure this whole
    /// mechanism exists to make impossible.
    pub fn open(path: &str) -> io::Result<Self> {
        let p = std::path::PathBuf::from(path);
        let md =
            std::fs::metadata(&p).map_err(|e| io::Error::other(format!("console {path}: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if !md.file_type().is_fifo() {
                return Err(io::Error::other(format!(
                    "console {path} is not a fifo — ci/vanilla-server.sh makes one at \
                     <dir>/console.fifo and the server reads it as stdin"
                )));
            }
        }
        #[cfg(not(unix))]
        let _ = md;
        Ok(Self { path: p })
    }

    /// Send one line, and give the server time to act on it.
    ///
    /// # Errors
    /// If the write fails. A dropped `gamerule` is not survivable: the
    /// scenario after it would measure a substrate nobody declared, which is
    /// exactly the 2026-08-03 defect (`gamerule randomTickSpeed 0` refused
    /// and unread) one level down.
    fn send(&self, line: &str) -> io::Result<()> {
        self.write_line(line)?;
        std::thread::sleep(CONSOLE_SETTLE);
        Ok(())
    }

    /// Write one line and return at once. [`Self::send`] without its sleep,
    /// for a console step that declares its own `settle_ms`.
    ///
    /// # Errors
    /// If the write fails — the same refusal [`Self::send`] makes.
    fn write_line(&self, line: &str) -> io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&self.path)
            .map_err(|e| io::Error::other(format!("console {}: {e}", self.path.display())))?;
        writeln!(f, "{line}")?;
        f.flush()?;
        Ok(())
    }
}

/// Record every scenario against one target.
///
/// A scenario that fails mid-way is written down as an error and the corpus
/// continues on fresh connections. A dead target fails the run rather than
/// producing a trace of empty scenarios, because an empty trace compares
/// equal to another empty trace and would read as a pass.
///
/// # Errors
/// If the target cannot be reached at all, or the trace cannot be built.
pub fn record(
    addr: &str,
    scenarios: &[Scenario],
    tables: &Tables,
    recorder: &str,
    console: Option<&Console>,
) -> io::Result<Trace> {
    // **A scenario that asks for a game rule and cannot get one must say so.**
    // This pass is the whole of the "declared rather than assumed" claim in
    // `crate::scenario::GameRule`: the rustmc pass has no console, so its
    // recording is taken on the substrate the harness booted while vanilla's
    // is taken on the one the scenario asked for, and a difference that size
    // between two recordings has to be printed by the thing that creates it.
    if console.is_none() {
        for s in scenarios.iter().filter(|s| !s.gamerule.is_empty()) {
            for g in &s.gamerule {
                eprintln!(
                    "  !! {}: [[gamerule]] {} = {} NOT APPLIED — no --console for {addr}. \
                     This recording is on the harness's substrate, not the scenario's.",
                    s.name, g.rule, g.value
                );
            }
        }
    }
    let status = status(addr)?;
    let mut out = Trace {
        target: addr.to_string(),
        version: status["version"]["name"]
            .as_str()
            .unwrap_or("?")
            .to_string(),
        protocol: status["version"]["protocol"].as_i64().unwrap_or(-1),
        recorder: recorder.to_string(),
        scenarios: Vec::new(),
    };

    let mut party: Option<(Session, Session)> = None;
    for (index, scenario) in scenarios.iter().enumerate() {
        // Recorded before the party is filled, because this is the moment the
        // per-connection counters either survive from the previous scenario
        // or start again at 1. See `ScenarioTrace::fresh_session`: a run that
        // reconnects here and a run that does not are no longer comparing the
        // same numbering, and that has to be visible rather than inferred.
        let fresh = party.is_none();
        if party.is_none() {
            let mut actor = login_to_play(addr, "oracle")?;
            let mut observer = login_to_play(addr, "oracle_eye")?;
            // Both have just loaded a world; nothing is measured until they
            // have stopped being told about it.
            actor.settle(QUIET, QUIET_CAP)?;
            observer.settle(QUIET, QUIET_CAP)?;
            warm_up(&mut actor, tables)?;
            observer.settle(QUIET, QUIET_CAP)?;
            party = Some((actor, observer));
        }
        let Some((actor, observer)) = party.as_mut() else {
            unreachable!("just filled")
        };

        match run_one(actor, observer, scenario, index, tables, console, addr) {
            Ok(mut t) => {
                t.fresh_session = Some(fresh);
                out.scenarios.push(t);
            }
            Err(e) => {
                out.scenarios.push(ScenarioTrace {
                    name: scenario.name.clone(),
                    error: Some(e.to_string()),
                    anchor: actor.anchor,
                    spawn: actor.spawn,
                    declared_commands: actor.declared_commands.clone(),
                    obs: Vec::new(),
                    fresh_session: Some(fresh),
                });
                // Neither connection is trustworthy after a failure — a
                // half-read frame would desync every scenario after this one
                // and blame them for it.
                party = None;
            }
        }
        println!(
            "  recorded {}/{}  {}",
            index + 1,
            scenarios.len(),
            scenario.name
        );
    }
    Ok(out)
}

/// Perform one step: through the actor's socket, or — for
/// [`crate::scenario::Step::Console`] — through the server's own console.
///
/// # A missing console is a refusal, not a skip
///
/// A console step that quietly did nothing would leave a measured window
/// containing only what the step did **not** cause, on both servers, agreeing
/// perfectly. That is the shape `ci/corpus-falsifiable.py` exists to refuse
/// and it would not be visible in the report, so it is refused here where the
/// message can name the scenario and the missing flag.
///
/// The wait is [`CONSOLE_SETTLE`], the same 400 ms a game rule gets and for
/// the same reason: `MinecraftServer.handleConsoleInputs` drains the queue
/// once per tick, so the line has to survive at least one tick boundary before
/// anything it caused can be observed.
fn act_or_console(
    actor: &mut Session,
    probe: &mut Option<Session>,
    addr: &str,
    console: Option<&Console>,
    step: &crate::scenario::Step,
    tables: &Tables,
    scenario: &str,
) -> io::Result<Duration> {
    let crate::scenario::Step::Console { text, settle_ms } = step else {
        // Every other verb, the probe's included, is `act_on`'s.
        return act_on(actor, probe, addr, step, tables);
    };
    let Some(c) = console else {
        return Err(io::Error::other(format!(
            "{scenario}: a [[step]] with do = \"console\" needs `record --console <fifo>`, \
             and this pass was given none. Skipping the line would record a window \
             holding only what the step did not cause — on both servers, agreeing."
        )));
    };
    // A declared settle replaces BOTH halves of the default wait — the sleep
    // inside the write and the drain after it — so the figure in the file is
    // the whole gap to the next step. See `Step::Console`'s `settle_ms`.
    if let Some(ms) = settle_ms {
        c.write_line(text)?;
        return Ok(Duration::from_millis(*ms));
    }
    c.send(text)?;
    Ok(CONSOLE_SETTLE)
}

/// Whether a console line can take a player to zero health: `kill` is
/// `hurtServer(genericKill(), Float.MAX_VALUE)` and `damage` is any amount.
fn kills(line: &str) -> bool {
    matches!(
        line.trim_start_matches('/').split_whitespace().next(),
        Some("kill" | "damage")
    )
}

/// Respawn the actor if the server has told it it is dead.
///
/// Drains first, for [`CONSOLE_SETTLE`]: a death caused by the last console
/// line reaches the client as `set_health` 0.0 on the target's next
/// `ServerPlayer.doTick`, and `handleClientCommand` refuses a respawn while
/// `getHealth() > 0` (offset 139) — so the respawn is sent only on the
/// evidence, never speculatively. Then drains again so the respawn's own
/// packets are read here and not in the next scenario's window.
fn respawn_if_dead(actor: &mut Session, tables: &Tables) -> io::Result<()> {
    actor.drain(CONSOLE_SETTLE)?;
    if actor.health.is_some_and(|h| h <= 0.0) {
        let respawn = crate::scenario::Step::ClientCommand {
            action: "perform_respawn".into(),
        };
        let wait = actor.act(&respawn, tables)?;
        actor.drain(wait.max(CONSOLE_SETTLE))?;
    }
    Ok(())
}

/// What a measured window is accumulating into.
///
/// Bundled rather than passed as six arguments: the normalizer, the clock
/// and the list are one thing — the recording — and splitting them up made
/// it possible to pass a *different* normalizer to the two clients, which
/// would have anchored the two streams' positions differently.
///
/// It used to number entities too, and sharing it between the two sessions
/// was the load-bearing part of that: two normalizers "would have renumbered
/// the same entity twice". The numbering now happens in
/// `compare::project`, after the sort by `who` below — see
/// `trace::entity_ref` — so what is shared here is only the anchor, which
/// `run_one` copies onto the observer for the same reason.
struct Recording<'a> {
    norm: Normalizer,
    tables: &'a Tables,
    started: Instant,
    obs: Vec<Obs>,
}

/// Spend the first block action of a session, unmeasured.
///
/// **Vanilla ignores it.** Measured 2026-08-02 on superflat: the very first
/// `player_action` of a session produced no `block_update` and — the part
/// that matters — no `block_changed_ack` either, while every action after it
/// was acknowledged normally. Whatever the cause inside vanilla, a harness
/// that lets that land inside a scenario reports the first check of every
/// run as a server that did nothing, and the check would be believed,
/// because "the reference produced nothing" is exactly what a broken check
/// looks like.
///
/// So the first action is spent here, at the spawn workspace, which no
/// scenario uses — scenario `n` works at spawn plus `(n + 1)` spacings. The
/// hole it leaves is never visited again.
fn warm_up(actor: &mut Session, tables: &Tables) -> io::Result<()> {
    let dig = crate::scenario::Step::Dig {
        at: [1, -1, 0],
        face: "up".into(),
    };
    let wait = actor.act(&dig, tables)?;
    actor.drain(wait)?;
    actor.settle(QUIET, QUIET_CAP)
}

/// Listen on every open client at once, in short alternating slices.
///
/// One socket at a time would let another's kernel buffer sit unread while a
/// scheduled tick fired, which is only a latency artefact — the packets still
/// arrive — but it puts them in the wrong `at_ms` bucket. The slices are short
/// so no stream is ever far behind.
///
/// The third connection is the expendable probe and is `None` for all but a
/// handful of checks. It is pumped through [`Session::pump_expendable`], so
/// the hang-up that follows a disconnect ends that connection's recording
/// instead of the scenario.
fn pump_all(
    actor: &mut Session,
    observer: &mut Session,
    mut probe: Option<&mut Session>,
    until: Instant,
    step: usize,
    rec: &mut Recording,
) -> io::Result<()> {
    while Instant::now() < until {
        let slice = (Instant::now() + Duration::from_millis(25)).min(until);
        actor.pump(
            Who::Actor,
            slice,
            &mut rec.norm,
            &rec.tables.states,
            &rec.tables.item_names,
            step,
            rec.started,
            &mut rec.obs,
        )?;
        let slice = (Instant::now() + Duration::from_millis(25)).min(until);
        observer.pump(
            Who::Observer,
            slice,
            &mut rec.norm,
            &rec.tables.states,
            &rec.tables.item_names,
            step,
            rec.started,
            &mut rec.obs,
        )?;
        if let Some(p) = probe.as_deref_mut() {
            let slice = (Instant::now() + Duration::from_millis(25)).min(until);
            p.pump_expendable(
                Who::Probe,
                slice,
                &mut rec.norm,
                &rec.tables.states,
                &rec.tables.item_names,
                step,
                rec.started,
                &mut rec.obs,
            )?;
        }
    }
    Ok(())
}

/// Perform one step on whichever connection it names.
///
/// Every verb but the probe ones goes to the actor, which is what
/// [`Session::act`] already assumes; the probe verbs are dispatched here
/// because they are the only ones that need a connection `act` does not own.
/// Keeping the split at this level rather than inside `act` means a scenario
/// cannot address a probe that was never opened: that is an error naming the
/// scenario, not a panic and not a silent no-op.
fn act_on(
    actor: &mut Session,
    probe: &mut Option<Session>,
    addr: &str,
    step: &crate::scenario::Step,
    tables: &Tables,
) -> io::Result<Duration> {
    use crate::scenario::Step;
    match step {
        Step::ProbeJoin => {
            if probe.is_some() {
                return Err(io::Error::other(
                    "probe_join twice in one scenario — a probe is dropped by the \
                     runner at the end of the scenario and there is only ever one",
                ));
            }
            // A distinct name: the player list is a set and a second `oracle`
            // would collide with the actor on an offline-mode server.
            let mut p = login_to_play(addr, "oracle_probe")?;
            // The one place this is set. Everything that makes a probe
            // survivable — a recorded disconnect instead of a raised one, a
            // tolerated hang-up — keys off it, so a connection that is not a
            // probe cannot accidentally become quiet about being dropped.
            p.expendable = true;
            *probe = Some(p);
            // **A join is loud and none of it is the measurement.** A login
            // sequence, the two players already here, their entity data and
            // the chunk batches all arrive on this socket, and a zero wait
            // here leaves that backlog in the kernel buffer for the first
            // *measured* pump to collect — which is how a check about one
            // packet ends up comparing an `add_entity` it never asked about.
            // Measured: 18 of the 19 observations in the first self-test
            // recording were this backlog.
            //
            // So the join is drained inside the step that caused it. This is
            // only correct because `probe_join` belongs in `setup`, whose
            // observations are discarded; the settle after the setup loop
            // catches whatever is still in flight.
            Ok(Duration::from_millis(600))
        }
        Step::ProbeAckTeleportAgain => {
            let p = probe.as_mut().ok_or_else(|| {
                io::Error::other(
                    "probe_ack_teleport_again without a probe_join — add \
                     `do = \"probe_join\"` to this scenario's setup",
                )
            })?;
            p.ack_teleport_again()?;
            // Long enough for a disconnect to arrive and the socket to close.
            // A server that is going to say nothing says nothing for just as
            // long, so this costs the same either way.
            Ok(Duration::from_millis(400))
        }
        other => actor.act(other, tables),
    }
}

fn run_one(
    actor: &mut Session,
    observer: &mut Session,
    scenario: &Scenario,
    index: usize,
    tables: &Tables,
    console: Option<&Console>,
    addr: &str,
) -> io::Result<ScenarioTrace> {
    // Workspaces march **west** while every scenario builds its rig to the
    // **east** of the player. That opposition is the whole point.
    //
    // They used to march east, into the rigs. A scenario would stand up a
    // stone block, a furnace and a fence at +1..+3 x, and the walk to the
    // next workspace then tried to go straight through them; vanilla
    // refuses to move a player into a solid block, so the walk never
    // arrived and burned its entire step budget — about three minutes per
    // scenario, against twelve seconds for the same scenario run alone.
    // Walking away from what you just built costs nothing and cannot
    // collide.
    // **Absolute, not spawn-relative, and that is load-bearing.**
    //
    // Workspaces used to be measured from wherever each server had put the
    // player. Vanilla picks a superflat spawn that moves from world to world
    // — three consecutive runs landed at (-4.5, 2.5), (-3.5, -2.5) and
    // (3.5, 6.5) — and this server spawns at (8, 8), so the same scenario ran
    // at *different absolute coordinates* on the two servers and on every
    // run.
    //
    // Almost nothing noticed, because the trace normalises positions against
    // the anchor. `section_blocks_update` notices. Its block order is the
    // iteration order of a `ShortOpenHashSet` keyed on
    // `SectionPos.sectionRelativePos`, which is built out of the low four
    // bits of the **absolute** x, y and z — so the byte order of that packet
    // is a function of where in the world the blocks are, and comparing two
    // servers standing 13 blocks apart compares two different situations.
    // Measured 2026-08-03: vanilla emitted a fence pair in one order in one
    // run and the other order in the next, with no change to anything, and
    // `crate::shortset`'s model predicts both correctly from their two
    // spawns.
    //
    // Pinning the workspace to absolute coordinates makes every
    // position-dependent property comparable. The cost is one longer walk at
    // the start, because both players now walk to a place neither spawned at.
    #[allow(clippy::cast_precision_loss)]
    let workspace = [
        -((index + 1) as f64 * WORKSPACE_SPACING),
        actor.spawn[1],
        0.0,
    ];
    // **An open menu is closed here, before the walk, and the order is the
    // point.**
    //
    // A container click is the second state-shaped verb in this language and
    // it leaks in three ways at once: the menu stays open, an item can stay on
    // the cursor, and the *harness's* own idea of which `containerId` is live
    // outlives the scenario that opened it. The third is the worst, because a
    // later `container_click` would then name a menu vanilla has already shut
    // — a click at a wrong `containerId` is dropped silently by
    // `handleContainerClick`, so it would measure nothing and report it as
    // agreement.
    //
    // **Before `goto`, not after**, because vanilla closes a container by
    // itself once the player walks out of range: `stillValid` is a distance
    // test and a workspace is 32 blocks from the last one. Closing after the
    // walk would therefore always land on the *inventory* menu instead — which
    // is window 0, never closes, and carries a state counter two existing
    // checks digest. Closing first hits the menu the scenario actually opened,
    // and on vanilla it only brings forward an auto-close that was going to
    // happen a moment later anyway.
    //
    // **Only when one is open**, which is the difference from the sneak
    // release below. Releasing a key nobody is holding is provably inert;
    // `doCloseContainer` is not — it runs `removed`, drops whatever is
    // carried, and clears the 2x2 crafting grid — and it does that whether or
    // not anything was open, because vanilla's handler checks nothing at all.
    // Sending it unconditionally would put that on all 67 scenarios to reset
    // the four that need it.
    if actor.menu.is_some() {
        actor.act(&crate::scenario::Step::CloseContainer, tables)?;
    }
    if observer.menu.is_some() {
        observer.act(&crate::scenario::Step::CloseContainer, tables)?;
    }

    // **The actor's walk is taken in legs, and the observer is read between
    // them.** `Client::goto` walks at ten blocks a second and reads only its
    // own socket, so for the whole walk the observer answers nothing — and
    // vanilla disconnects a client whose keep-alive has been pending for
    // fifteen seconds (`oracle_eye lost connection: Timed out`). A scenario
    // that leaves the actor 150 or more blocks from the next workspace
    // (`082` walks to z = 150; `083`-`089` build out at z = +/-80 and
    // +/-160) makes that walk longer than fifteen seconds, and the scenario
    // AFTER it then fails with `Broken pipe` for a reason it does not own.
    // Measured 2026-09-24 in `~/rustmc-mobgoal`: one such error in each of
    // two vanilla recordings, each right after a 150-160 block walk, none
    // after a shorter one. Forty-block legs keep every observer silence under
    // five seconds; the last leg ends at `workspace`, so the anchor `goto`
    // takes is the same one a single walk would have taken.
    const LEG: f64 = 40.0;
    loop {
        let d = [workspace[0] - actor.pos[0], workspace[2] - actor.pos[2]];
        let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if dist <= LEG {
            break;
        }
        let f = LEG / dist;
        actor.goto([
            actor.pos[0] + d[0] * f,
            workspace[1],
            actor.pos[2] + d[1] * f,
        ])?;
        observer.drain(Duration::from_millis(50))?;
    }
    actor.goto(workspace)?;
    observer.goto([workspace[0], workspace[1], workspace[2] + OBSERVER_OFFSET])?;
    // The observer's coordinates must mean what the actor's mean, or every
    // position it reports is offset by however far apart they stand.
    observer.anchor = actor.anchor;

    // **Held keys are reset here for the same reason position is.**
    //
    // Every scenario in a run shares one connection — `record` logs in once
    // and reuses the party — so any state a scenario leaves on the *server*
    // is inherited by the next one. Position already had this problem and
    // `goto` above is its answer. `Step::Sneak` gave the problem to the input
    // state: `handlePlayerInput` stores what it is sent and holds it until
    // another packet says otherwise, so a scenario that sneaks and does not
    // release leaves every later scenario sneaking.
    //
    // **Measured, not anticipated.** `552` sneaks and `553` deliberately does
    // not, and they are the two halves of one pair. Run together, vanilla
    // produced *identical* traces for both — it placed the block in each —
    // and the pair silently stopped distinguishing anything. Run alone, 553's
    // vanilla trace flips the lever, which is what it is there to assert. The
    // check that was supposed to catch a server ignoring the sneak flag had
    // been made unable to fail by the check next to it.
    //
    // Fixing it in `552` by releasing at the end is the version that works
    // until somebody writes the next scenario, so it is fixed here: the
    // recorder puts the client back into a known input state before every
    // scenario, and a scenario that never mentions sneak cannot be sneaking.
    // The corpus must not be order-dependent, and this is the whole cost of
    // that for a state-shaped verb.
    actor.act(&crate::scenario::Step::Sneak { on: false }, tables)?;
    observer.act(&crate::scenario::Step::Sneak { on: false }, tables)?;

    // **And the rest of the player's own state, for the same reason** — the
    // inventory, the selected hotbar slot and the facing. Sneak above was
    // measured leaking between two neighbours; these were measured leaking
    // between a serial run and a sharded one, where a scenario's neighbours
    // are different scenarios. See `Session::reset_player_state`. Settled
    // here, not in setup, so a scenario with no setup does not open its
    // measured window on the equipment and attribute packets the reset
    // caused.
    actor.reset_player_state()?;
    observer.reset_player_state()?;
    actor.settle(QUIET, QUIET_CAP)?;
    observer.settle(QUIET, QUIET_CAP)?;

    // Setup runs into a throwaway recording. Its observations are
    // discarded — a rig's own block updates would otherwise drown whatever
    // the scenario is actually asking about — but they are still *read*,
    // because a socket nobody drains fills up and the next step blocks.
    let mut scratch = Recording {
        norm: Normalizer::new(actor.anchor),
        tables,
        started: Instant::now(),
        obs: Vec::new(),
    };
    // The expendable third connection, if this scenario opens one. `None` for
    // every check that does not, which is all but a handful — see
    // [`crate::trace::Who::Probe`].
    let mut probe: Option<Session> = None;
    for step in &scenario.setup {
        let wait = act_or_console(
            actor,
            &mut probe,
            addr,
            console,
            step,
            tables,
            &scenario.name,
        )?;
        pump_all(
            actor,
            observer,
            probe.as_mut(),
            Instant::now() + wait,
            0,
            &mut scratch,
        )?;
    }
    // Wait for quiet rather than for a fixed time: the measured window has
    // to start after the rig has finished being built, and the two servers
    // do not take the same time to finish anything.
    actor.settle(QUIET, QUIET_CAP)?;
    observer.settle(QUIET, QUIET_CAP)?;
    // The probe settles on the same terms, when a scenario opened one. Without
    // this the tail of its join lands in the measured window — see the note in
    // `act_on`'s `ProbeJoin` arm.
    if let Some(p) = probe.as_mut() {
        p.settle_expendable(QUIET, QUIET_CAP)?;
    }

    // **A declared game rule is applied here and nowhere earlier**, and the
    // position is the whole of its safety.
    //
    // Setup is above this line, so the rig is built with the harness's own
    // substrate: a scenario that turns random ticks on to watch the grass
    // under its block die must not have that happen while it is *placing* the
    // block, because setup's observations are discarded and the measured
    // window would then open onto a world where the thing had already
    // happened. `crate::compare::Verdict::Mute` is the verdict for that and
    // it is indistinguishable from a check that does nothing.
    //
    // It is also after the quiet settle, so the rule takes effect on a world
    // that has stopped talking, and every observation it causes belongs to
    // the measured window by construction.
    let mut applied: Vec<&crate::scenario::GameRule> = Vec::new();
    if let Some(c) = console {
        for g in &scenario.gamerule {
            c.send(&g.apply_command())?;
            applied.push(g);
        }
    }

    let mut rec = Recording {
        norm: Normalizer::new(actor.anchor),
        tables,
        started: Instant::now(),
        obs: Vec::new(),
    };
    // **The window is fallible and the restore is not optional.** A step that
    // errors between the apply and the restore would otherwise leave the rule
    // set for every scenario after it, which is the corpus-wide instability
    // this mechanism exists to prevent — and it would arrive as ninety-two
    // checks going red for a reason none of them names. So the measured part
    // runs to a `Result` that is held, the rules are put back either way, and
    // a failure to put one back outranks the original error: a substrate that
    // is wrong from here on invalidates the rest of the run, where a failed
    // scenario invalidates one.
    let windowed = (|| -> io::Result<()> {
        for (i, step) in scenario.step.iter().enumerate() {
            let wait = act_or_console(
                actor,
                &mut probe,
                addr,
                console,
                step,
                tables,
                &scenario.name,
            )?;
            pump_all(
                actor,
                observer,
                probe.as_mut(),
                Instant::now() + wait,
                i,
                &mut rec,
            )?;
        }
        // The settle window is where anything unprompted shows up: a scheduled
        // tick, an item despawning, a mob taking its first step. A scenario
        // about something that happens on its own is entirely this window.
        let last = scenario.step.len().saturating_sub(1);
        pump_all(
            actor,
            observer,
            probe.as_mut(),
            Instant::now() + Duration::from_millis(scenario.settle_ms),
            last,
            &mut rec,
        )
    })();
    // **Dropped here, unconditionally, including after an error.** A probe
    // that outlived its scenario would be a second player standing in the
    // world for every check after it — visible to entity tracking, counted by
    // `/list`, and present on one server and not the other exactly when the
    // mechanism under test worked. There is no verb to close one, so this is
    // the only place it can happen and it cannot be forgotten by a check.
    drop(probe.take());
    for g in applied.iter().rev() {
        if let Some(c) = console {
            c.send(&g.restore_command()).map_err(|e| {
                io::Error::other(format!(
                    "could not restore gamerule {} to {} after {}: {e} — \
                     every scenario after this one would run on the wrong substrate",
                    g.rule, g.restore, scenario.name
                ))
            })?;
        }
    }
    // **The console restore, and it is not optional either.** Same position as
    // the game-rule restore and the same rule: outside the measured window, run
    // whether or not the window succeeded, and a failure to restore outranks
    // the original error — a substrate that is wrong from here on invalidates
    // the rest of the run where a failed scenario invalidates one.
    //
    // The declaration is mandatory (`Scenario::load` refuses a console step
    // with no `console_restore`) because of what its absence did on
    // 2026-09-05: an unrestored `worldborder set 137` left every rig from
    // scenario 18 on OUTSIDE the world border, vanilla answered each with
    // `build.tooHigh`, and ninety checks were reported as regressions naming
    // none of it.
    //
    // The lines land in the NEXT scenario's quiet settle rather than in
    // anybody's measured window, which is the property the game-rule restore
    // relies on too: `actor.settle(QUIET, QUIET_CAP)` runs before the window
    // opens and absorbs whatever they cause.
    for line in &scenario.console_restore {
        let Some(c) = console else {
            return Err(io::Error::other(format!(
                "{}: declares console_restore and this pass has no console — the \
                 step that needed restoring cannot have run either",
                scenario.name
            )));
        };
        c.send(line).map_err(|e| {
            io::Error::other(format!(
                "could not send console restore {line:?} after {}: {e} — every \
                 scenario after this one would run on state nobody declared",
                scenario.name
            ))
        })?;
    }
    // **A death is restored by the client, and only by the client.** A
    // `damage` step or a `kill` restore can leave the actor at zero health,
    // and a dead player stays on the death screen for every later scenario —
    // `handleClientCommand`'s `PERFORM_RESPAWN` is the one way back and no
    // console line can send it. So after the restores, a scenario that could
    // have killed the actor listens for the `set_health` that says so and
    // answers it with the respawn a real client's button sends. Outside the
    // window, like every restore; its packets land in the next settle.
    let could_kill = scenario
        .setup
        .iter()
        .chain(scenario.step.iter())
        .any(|st| matches!(st, crate::scenario::Step::Console { text, .. } if kills(text)))
        || scenario.console_restore.iter().any(|l| kills(l));
    if could_kill {
        respawn_if_dead(actor, tables).map_err(|e| {
            io::Error::other(format!(
                "could not respawn the actor after {}: {e} — every scenario after \
                 this one would record a dead player",
                scenario.name
            ))
        })?;
    }
    if !scenario.console_restore.is_empty() {
        // `MinecraftServer.handleConsoleInputs` drains its queue once per
        // tick, and the next scenario's setup begins the moment this returns.
        // Without this the restore can still be sitting in the queue while
        // that setup places its first block — which for a border restore is
        // the very failure this whole mechanism exists to stop, one scenario
        // narrower. The same 400 ms a console STEP waits, and for the same
        // reason.
        std::thread::sleep(CONSOLE_SETTLE);
    }
    windowed?;

    // Group by client, keeping each client's own order. Interleaving two
    // sockets read in turn is a property of the harness's scheduling and not
    // of the server, so comparing the interleaving would compare noise.
    let mut obs = rec.obs;
    obs.sort_by_key(|o| o.who);

    Ok(ScenarioTrace {
        name: scenario.name.clone(),
        error: None,
        anchor: actor.anchor,
        spawn: actor.spawn,
        declared_commands: actor.declared_commands.clone(),
        obs,
        // Filled in by `record`, which is the only place that knows whether
        // the party it handed us had just been logged in.
        fresh_session: None,
    })
}

// --- the recorder's own identity -------------------------------------------
//
// **This lives here rather than in `compare.rs`, and the move is the point.**
// `RECORDER_SOURCES` was declared three lines under `PROJECTION_SOURCES` with a
// comment explaining that the two are tracked separately so that a recorder
// change does not stale every floor. `compare.rs` is itself a projection
// source, so the declaration had the opposite effect: adding a module to the
// recorder's list, or embedding its text to digest it, edited the projection's
// bytes and moved `proj:`. Measured on the branch that found it —
// `proj:58380c9e4cbeae81` to `proj:e72858dd73a06e16` for adding the function
// below, which changes nothing about what a comparison looks at.
//
// From here, nothing about the recorder can move the projection identity.
// `compare::no_projection_source_embeds_a_recorder_source` keeps it that way.

/// The sources that decide what a recording *contains*, in a fixed order.
///
/// Embedded at compile time for the reason [`crate::compare::PROJECTION_SOURCES`]
/// is: the identity must describe the binary that ran, not the tree it stood
/// in. `ci/t2-gameplay-selfcheck.sh` stamped `recorder=` by hashing
/// `tools/oracle-runner/src/*.rs` **on disk** until 2026-08-09, which answers
/// "what recorder is in that directory now" — a different question, and on a
/// box where `rsync -a` can leave sources older than the artefacts built from
/// them, a differently-answered one. `ci/corpus-floor.sh`'s `tree-now:`
/// fallback is that weaker reading and stays labelled as such.
///
/// This list also exists so that adding a module is a decision somebody makes
/// rather than a silent third category — see
/// `compare::every_oracle_source_is_declared_in_or_out_of_the_projection`.
pub const RECORDER_SOURCES: [(&str, &str); 3] = [
    ("client.rs", include_str!("client.rs")),
    ("main.rs", include_str!("main.rs")),
    ("record.rs", include_str!("record.rs")),
];

/// `rec:<16 hex>` — the identity of the recorder this binary carries.
///
/// Written into the `.prov` beside every recording by `ci/t2-gameplay.sh` and
/// `ci/t2-gameplay-selfcheck.sh`, and read back by `ci/corpus-floor.sh`, which
/// groups an ensemble by decoder to decide whether a disagreement between two
/// recordings could be a decoder difference rather than vanilla's.
///
/// **It is the weaker half of the question and is built anyway.**
/// `docs/PARITY-ROADMAP.md` §15.4: a digest differs between every pair of
/// trees, so it can say "different" and never "comparable" — whereas the
/// ensemble's *vocabulary* is content-derived and can say both. That is why
/// the vocabulary leads and this follows. What this adds is the direction a
/// vocabulary cannot cover: two decoders that happen to emit the same field
/// set over one run have the same vocabulary and are still two decoders.
#[must_use]
pub fn recorder_identity() -> String {
    let mut payload = String::from("oracle-runner recorder identity v1\n");
    for (name, src) in RECORDER_SOURCES {
        let body = crate::compare::projection_body(name, src);
        payload.push_str(&format!(
            "== {name} {} bytes {} lines\n{body}\n",
            body.len(),
            body.lines().count()
        ));
    }
    format!("rec:{}", crate::trace::digest(payload.as_bytes()))
}

/// The per-source breakdown a `recorder=` mismatch has to be diagnosed from.
#[must_use]
pub fn recorder_explain() -> String {
    let mut out = format!("recorder {}\n", recorder_identity());
    for (name, src) in RECORDER_SOURCES {
        let body = crate::compare::projection_body(name, src);
        out.push_str(&format!(
            "  {name:<14} {:>6} lines  {:>7} bytes  {}   -- recorder, not the projection\n",
            body.lines().count(),
            body.len(),
            crate::trace::digest(body.as_bytes())
        ));
    }
    out
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    /// The recorder identity, on [`crate::compare::projection_identity`]'s
    /// terms and with one more.
    ///
    /// The extra one is that it must not equal the projection identity. Both
    /// are `digest` over a payload built the same way, and a copy-paste that
    /// left `PROJECTION_SOURCES` in the loop would give a `rec:` stamp that
    /// tracked the comparator: every recording would look re-recorded whenever
    /// the comparator moved, and identical whenever only the decoder did.
    /// That is worse than no stamp — it is a stamp wrong in both directions
    /// while looking like it works.
    #[test]
    fn the_recorder_identity_is_not_the_projection_identity() {
        let id = recorder_identity();
        assert!(id.starts_with("rec:"), "{id}");
        assert_eq!(id.len(), "rec:".len() + 16, "{id}");
        assert_ne!(
            id["rec:".len()..],
            crate::compare::projection_identity()["proj:".len()..],
            "the recorder identity digests the same payload as the projection — check \
             which constant the loop reads"
        );
        let mut bodies = Vec::new();
        for (name, src) in RECORDER_SOURCES {
            let body = crate::compare::projection_body(name, src);
            assert!(body.len() > 500, "{name}: kept only {} bytes", body.len());
            bodies.push(body);
        }
        // Three files, three bodies: a list naming one file three times would
        // digest something and be blind to two thirds of the recorder.
        bodies.sort();
        bodies.dedup();
        assert_eq!(bodies.len(), RECORDER_SOURCES.len());
    }
}
