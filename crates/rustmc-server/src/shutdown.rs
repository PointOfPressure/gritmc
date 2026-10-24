//! `MinecraftServer.stopServer` — the save a clean stop does, and the two
//! signals that ask for one.
//!
//! Reference (26.2), read from the jar with `javap -p -c -constants`:
//! `MinecraftServer.stopServer` and `MinecraftServer.saveAllChunks`.
//!
//! # Why this is not just "the autosave, again"
//!
//! `docs/cold-start/persistence.md` refused a shutdown save on two grounds,
//! and one of them was right and remains right:
//!
//! > A timer bounds the loss to one interval no matter how the process ends,
//! > which is the guarantee vanilla gives and for the same reason.
//!
//! True, and it is why the autosave is not replaced by this. `kill -9` and a
//! power cut reach no handler in any language, so a shutdown hook can only
//! ever be the *better* case, never the guarantee. What it buys is that the
//! ordinary case — an operator stopping a server, systemd stopping a unit,
//! a container being drained — loses **nothing** rather than up to one
//! interval, which at the default is five minutes of everybody's building.
//!
//! The other ground was that the dependency went through the orchestrator.
//! It now has: `libc` is the one crate added, it has no dependencies of its
//! own, and it is the smallest thing that can install a handler.
//!
//! # Async-signal-safety, and why there is a whole thread for it
//!
//! **A signal handler may not save a world.** It runs on whatever thread the
//! kernel picks, at whatever instruction that thread had reached, and almost
//! nothing is safe to call from it — not `malloc`, not `println!`, not
//! anything that takes a lock the interrupted thread might already hold.
//! Saving a world does all three.
//!
//! So the handler does the one thing that *is* async-signal-safe: a relaxed
//! store into a `static AtomicBool`. Everything else happens on threads that
//! were running anyway:
//!
//! * the **tick thread** notices the flag at the top of its next tick and
//!   runs the save. That is where vanilla runs it — `stopServer` executes on
//!   the server thread once `runServer`'s loop has left — and it means the
//!   save cannot interleave with a tick, which is the property that makes it
//!   a save of a consistent world rather than of a half-applied one;
//! * a **watchdog thread**, started with the handler, force-exits if the tick
//!   thread has not finished within [`GRACE_SECONDS`].
//!
//! # The watchdog is not belt-and-braces, it is the thing that keeps SIGTERM
//! meaning what it meant
//!
//! Installing a handler changes SIGTERM's disposition from "terminate" to
//! "whatever this program does". A server whose tick thread is wedged — a
//! deadlock, a pathological chunk, a disk that has stopped answering — would
//! then ignore SIGTERM entirely, and an operator who could previously stop it
//! with `kill` would need `kill -9`. That is a regression this file would
//! have introduced silently, so the watchdog exits with a status and a
//! sentence naming what did not finish.
//!
//! `SIGKILL` cannot be caught and is deliberately not attempted; it is the
//! control this gate's negative arm uses.
//!
//! # The order, and what of it survives the reduction
//!
//! ```text
//! stopServer():
//!   packetProcessor.close()
//!   LOGGER.info("Stopping server")
//!   getConnection().stop()                     <- stop accepting
//!   isSaving = true
//!   if (playerList != null):
//!       LOGGER.info("Saving players")
//!       playerList.saveAll()                   <- PLAYERS FIRST
//!       playerList.removeAll()
//!   LOGGER.info("Saving worlds")
//!   for level: level.noSave = false            <- a `/save-off` cannot skip a shutdown save
//!   while (any level still has chunks to unload):
//!       for level: chunkSource.deactivateTicketsOnClosing()
//!                  chunkSource.tick(() -> true, false)
//!       waitUntilNextTick()
//!   saveAllChunks(false, true, false)          <- suppressLogs=false, FLUSH=true
//!   for level: level.close()
//!   isSaving = false
//!   savedDataStorage.close(); resources.close(); storageSource.close()
//! ```
//!
//! What this server has of that: the players, the chunks, `level.dat`, and
//! the release of `session.lock`. What it does not:
//!
//! * **the unload drain loop.** `ChunkMap.processUnloads` is asynchronous in
//!   vanilla — `scheduleUnload` chains a save onto a `CompletableFuture` and
//!   the loop is what waits for the queue to empty. `crate::tiles`'
//!   `retain_around` saves synchronously before it evicts, so there is no
//!   queue to drain; a loop here would spin on an empty condition.
//! * **`noSave = false`.** There is no `/save-off`, so nothing can have set
//!   it. Noted rather than implemented, because the moment `/save-off` lands
//!   this line is required and a reader needs to know it was left out on
//!   purpose.
//! * **`flush = true`'s meaning.** In vanilla it is what turns
//!   `savedDataStorage.scheduleSave()` into `saveAndJoin()` and makes the
//!   "All chunks are saved" line honest. Every write on this path is already
//!   synchronous, so the flag has nothing to change — which is why the log
//!   line it gates is printed unconditionally here.

use std::sync::atomic::{AtomicBool, Ordering};

/// How long the watchdog gives the tick thread to finish the save.
///
/// Long enough that a real save of a large resident world completes — the
/// measured cost of `Tiles::save_all` is well under a second per region — and
/// short enough that an operator or an init system does not conclude the
/// process is hung. systemd's own `DefaultTimeoutStopSec` is 90 s, so this
/// sits an order of magnitude inside the window before anything else starts
/// sending `SIGKILL`.
pub const GRACE_SECONDS: u64 = 10;

/// Set by the signal handler, read by the tick thread. The only thing the
/// handler touches.
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set by the tick thread when the save has finished, read by the watchdog.
static COMPLETED: AtomicBool = AtomicBool::new(false);

/// The handler. **Everything in here must be async-signal-safe**, which is
/// why it is one relaxed store and nothing else — no allocation, no lock, no
/// formatting, no I/O.
///
/// `Relaxed` is sufficient: the tick thread only needs to observe the flag
/// eventually, and it re-reads it every tick. Nothing is published alongside
/// it.
extern "C" fn handler(_sig: libc::c_int) {
    REQUESTED.store(true, Ordering::Relaxed);
}

/// Install the handler for `SIGTERM` and `SIGINT`, and start the watchdog.
///
/// Idempotent in effect — installing the same handler twice is harmless — but
/// it is called once, from `main`, and only when `--world` was given: a
/// server with nowhere to save has nothing for a shutdown hook to do, and
/// changing SIGTERM's disposition for no gain would be a change to how the
/// process dies in exchange for nothing.
///
/// # Safety of the `unsafe` block
///
/// `libc::signal` is `unsafe` because a handler that is not async-signal-safe
/// is undefined behaviour. [`handler`] is one atomic store; see its doc
/// comment. `SIG_ERR` is checked rather than ignored, because a failed
/// install leaves SIGTERM at its default disposition — the process still dies
/// on `kill`, which is the safe direction, but it dies *without saving* while
/// the log claims a hook is armed.
pub fn install() {
    for sig in [libc::SIGTERM, libc::SIGINT] {
        // `libc::SIG_ERR` is a `sighandler_t` on unix and a `c_int` (-1) on
        // Windows, where `signal` still returns a `sighandler_t`. Compared
        // bare, the Windows build does not type-check (`ci/cross-check.sh`,
        // x86_64-pc-windows-gnu, E0308). The cast is C's own
        // `(void (*)(int))-1`; on unix it would be a no-op cast clippy
        // refuses, hence the cfg rather than one `as` everywhere.
        #[cfg(windows)]
        let sig_err = libc::SIG_ERR as libc::sighandler_t;
        #[cfg(not(windows))]
        let sig_err = libc::SIG_ERR;
        // SAFETY: `handler` performs a single relaxed atomic store and calls
        // nothing. That is async-signal-safe on every platform this builds
        // for.
        let prev = unsafe { libc::signal(sig, handler as *const () as libc::sighandler_t) };
        if prev == sig_err {
            eprintln!(
                "shutdown: could not install a handler for signal {sig} — a clean \
                 stop will NOT save; the autosave is still the bound on loss"
            );
            return;
        }
    }
    std::thread::spawn(watchdog);
    eprintln!("shutdown: SIGTERM and SIGINT save before exiting ({GRACE_SECONDS}s grace)");
}

/// Has a stop been asked for? Read once per tick.
#[must_use]
pub fn requested() -> bool {
    REQUESTED.load(Ordering::Relaxed)
}

/// Called by the tick thread when its save has finished and it is about to
/// exit. The watchdog reads it to tell "finished" from "wedged".
pub fn completed() {
    COMPLETED.store(true, Ordering::Relaxed);
}

/// Force the process down if the tick thread does not.
///
/// Polls rather than blocking on a condvar because the thing it is waiting
/// for is set from a signal handler, and a condvar notify is not
/// async-signal-safe. A 100 ms poll costs ten wakeups a second and is
/// invisible beside a 20 Hz tick.
fn watchdog() {
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if !requested() {
            continue;
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(GRACE_SECONDS);
        while std::time::Instant::now() < deadline {
            if COMPLETED.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        eprintln!(
            "shutdown: the tick thread did not finish saving within {GRACE_SECONDS}s — \
             exiting anyway. The world on disk is whatever the last autosave left."
        );
        std::process::exit(1);
    }
}

/// `stopServer`'s body, reduced to what this server has, run on the tick
/// thread. Does not return: it exits the process.
///
/// The two counts are printed because they fail separately, which is the same
/// reason [`crate::autosave::save_everything`] returns them apart.
pub fn stop_server(tiles: &crate::tiles::Tiles) -> ! {
    eprintln!("Stopping server");
    let Some(store) = crate::levelstore::shared() else {
        // No `--world`, so nothing to save. Reaching here means `install`
        // was called without one, which it is not — but exiting cleanly is
        // the right answer either way and is cheaper than a panic.
        completed();
        std::process::exit(0);
    };
    let state = crate::levelstore::WorldState {
        seed: 0,
        ticks: crate::levelstore::ticks(),
        game_type: crate::status::game_type(),
        epoch_millis: crate::levelstore::epoch_millis(),
        spawn: (8, 100, 8),
        brand: format!("rustmc {}", rustmc_proto::GAME_VERSION),
    };
    eprintln!("Saving players");
    eprintln!("Saving worlds");
    let (players, chunks) =
        crate::autosave::save_everything(store, crate::entities::Entities::shared(), tiles, &state);
    // `saveAllChunks`'s own line, printed unconditionally because `flush` has
    // nothing to change here — see the module docs.
    eprintln!("ThreadedAnvilChunkStorage: All dimensions are saved");
    eprintln!("shutdown: saved {chunks} chunk(s) and {players} player(s)");
    completed();
    std::process::exit(0);
}
