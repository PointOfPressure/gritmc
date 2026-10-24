//! The server's own console: a line of text in, a real command dispatched
//! against the real per-process game-rule store — no client, no permission
//! check, matching vanilla's own console source.
//!
//! # From the jar
//!
//! `net/minecraft/server/ConsoleInput` is a two-field record (`msg`,
//! `source`); `MinecraftServer.handleConsoleInput`/`handleConsoleInputs`
//! queue lines and drain them once per tick, dispatched through
//! `CommandSourceStack`s built by `MinecraftServer.createCommandSourceStack()`
//! — which is unconditionally full permission, because a process's own
//! stdin has nothing to check a `PermissionCheck` against. That is why
//! [`dispatch_line`] below has no permission argument at all, unlike
//! `crate::commands::handle`, which resolves one per connection
//! (`crate::ops::level_for`).
//!
//! # Why this exists, and what it is not
//!
//! `tools/oracle-runner`'s `record::Console` is the *writing* end of this —
//! it already exists and already drives real vanilla through exactly this
//! mechanism (a fifo opened as the target's stdin by `ci/vanilla-server.sh`).
//! This is the missing *reading* end for `rustmc-server`, which until now
//! had none — `tools/oracle-runner/src/main.rs:239`'s "this server has no
//! console" is the line this module answers.
//!
//! It is a real server capability, not a harness-only backdoor: a dedicated
//! server's console is a first-party vanilla feature, and nothing here is
//! reachable only in a test build. What is scoped down from vanilla's own
//! console is the command grammar it understands — see [`dispatch_line`].
//!
//! # The fifo, and the zero-block open
//!
//! Vanilla's stdin is whatever the process's stdin is; `ci/vanilla-server.sh`
//! makes that a fifo by shell redirection (`exec 3<>"$dir/console.fifo"`,
//! read-write, then `<&3`) rather than by vanilla knowing anything about
//! fifos. This server takes a *path* instead (`--console <path>`, wired in
//! `crate::main`) because it does not inherit its stdin from a wrapper
//! script the way `ci/t2-gameplay.sh` launches it — so it has to open the
//! fifo itself, and it does the same read-write trick directly:
//! `OpenOptions::read(true).write(true)` on a FIFO never blocks in `open(2)`
//! regardless of whether the other end is open, where a read-only open
//! blocks until a writer exists. `ci/t2-gameplay.sh` creates the fifo and
//! starts this server before it starts recording, so the ordering already
//! matches what vanilla's own wrapper gives it.

use crate::gamerules::GameRules;

/// Start reading console commands from `path` on a background thread.
///
/// `path` must already exist as a fifo — this does not create one, the same
/// division of labour as `ci/vanilla-server.sh` (`mkfifo`) and vanilla
/// itself (reads whatever stdin is). The caller is `crate::main`'s `run`
/// subcommand, given `--console <path>` — see `ci/t2-gameplay.sh` for where
/// that path is created and passed.
///
/// # Errors
/// If `path` cannot be opened. Not being a fifo is not checked here, unlike
/// `tools/oracle-runner::record::Console::open` (the writing end, which
/// refuses eagerly): a regular file opened this way reads to EOF once and
/// the thread exits, which is a silent inertness rather than a hang — an
/// acceptable failure mode for an operator mistake on the served side, where
/// hanging the boot itself would not be.
/// The connection list a console command broadcasts to, once there is one.
///
/// # Why a registration hook and not an argument
///
/// `crate::console::start` is called from `crate::main` before
/// `crate::status::listen`, which is where the `Players` is constructed — so
/// there is nothing to pass at the time the console thread starts. Vanilla has
/// the same ordering (`MinecraftServer.runServer` starts the console reader in
/// `DedicatedServer.initServer` and the `PlayerList` already exists) and
/// resolves it the other way round; restructuring `listen` to build its
/// registry earlier is a bigger change than this cell, and one that would
/// touch every caller of `listen`.
///
/// `None` means no connection has ever been accepted, which is exactly when a
/// broadcast has nobody to reach. A console command still takes effect on the
/// border in that state — [`crate::worldborder::shared`] is independent of
/// this — so a `worldborder` set before the first player joins is carried into
/// that player's `initialize_border`, which is what vanilla does too.
static PLAYERS: std::sync::OnceLock<std::sync::Arc<crate::players::Players>> =
    std::sync::OnceLock::new();

/// Give the console the connection list, once.
///
/// A second call is ignored rather than refused: one process serves one
/// registry, and a caller that has one has the right one.
pub fn set_players(players: &std::sync::Arc<crate::players::Players>) {
    let _ = PLAYERS.set(std::sync::Arc::clone(players));
}

/// The server's clock, once — for the console lines whose effect is tied to
/// the tick they arrived in. See [`crate::damagecmd::Pending::at_tick`].
static CLOCK: std::sync::OnceLock<crate::tick::GameClock> = std::sync::OnceLock::new();

/// Give the console the game clock, once. A second call is ignored, as
/// [`set_players`]'s is.
pub fn set_clock(clock: &crate::tick::GameClock) {
    let _ = CLOCK.set(clock.clone());
}

pub fn start(path: &str) -> std::io::Result<()> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    let path = path.to_string();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(f).lines() {
            match line {
                Ok(line) => dispatch_line(&line, GameRules::shared()),
                Err(e) => {
                    eprintln!("console {path}: {e}");
                    break;
                }
            }
        }
        eprintln!("console {path}: closed, no more commands will be read");
    });
    Ok(())
}

/// One console line.
///
/// **First, the tree.** Every line is resolved against
/// `crate::commands::tree()` at `Level::Owners` and handed to
/// `crate::cmdexec::dispatch`, which runs every root added since 2026-10-01
/// (`/difficulty`, `/gamemode`, `/teleport`, `/execute`, `/setblock`, ...)
/// with the console as source — so `weather` below is now reached only by a
/// line the tree refuses. What falls through is matched by hand:
///
/// Two commands are wired here:
///
/// * `gamerule <name> [<value>]` — set with a value, query without one —
///   which is the whole of what `tools/oracle-runner`'s `Console::send` sent
///   before 2026-09-05 and the whole of what a corpus scenario's
///   `[[gamerule]]` needs;
/// * `weather clear|rain|thunder [<duration>]` — `WeatherCommand`'s whole
///   grammar, dispatched in [`crate::weather::dispatch_console`]. It is here
///   rather than in `crate::commands` for the reason vanilla's own
///   `/weather` could not be reached from a corpus scenario over chat:
///   `WeatherCommand.register` gates it on `Commands.LEVEL_GAMEMASTERS`
///   (permission 2) and the harness's players are not operators
///   (`ci/t2-gameplay.sh`'s own note). A console line has no permission to
///   check — `MinecraftServer.createCommandSourceStack()` carries none —
///   which is the same asymmetry `gamerule` already lives under.
///
/// Vanilla's console accepts every command in the tree (`/kill`, `/stop`,
/// `/save-all`, ...); this one does not, and an unrecognised line is
/// answered by naming what happened rather than staying silent — a dropped
/// console line during a corpus run is the 2026-08-03 defect
/// (`gamerule randomTickSpeed 0` refused and unread) one layer further in,
/// and this module exists specifically so that cannot happen quietly again.
fn dispatch_line(line: &str, rules: &GameRules) {
    // `MinecraftServer.handleConsoleInput` hands the line to
    // `Commands.performPrefixedCommand`, whose first act is
    // `Commands.trimOptionalPrefix` — **one** leading slash removed and
    // nothing else, no trim of whitespace. Without it a console line typed
    // with the slash a player would use runs on vanilla and not here, and the
    // two servers disagree for a reason that is entirely the harness's.
    let line = line.strip_prefix('/').unwrap_or(line);
    // `xp`/`experience`, and `summon`/`execute ... summon` of an experience
    // orb, are queued for the clock thread when `crate::experience` can read
    // them, BEFORE `crate::cmdexec` would run them here on the reader thread:
    // `DedicatedServer.handleConsoleInputs` runs after every connection has
    // ticked, so the level-up sound `giveExperienceLevels` plays leaves before
    // the `set_experience` the next `doTick` sends. Run here instead, the
    // registry moves between ticks and the next connection tick sends
    // `set_experience` ahead of the sound (124 and 681 on train-g's first
    // corpus run). A form the queue does not read still goes to `cmdexec`.
    if matches!(
        line.split_whitespace().next(),
        Some("xp" | "experience" | "summon" | "execute")
    ) {
        if let Ok(a) = parse_queued(line) {
            crate::experience::queue_console(a);
            return;
        }
    }
    // Every root `crate::cmdexec` owns goes through the real tree and the
    // real executors, with the console as the source.
    if crate::cmdexec::console_line(line, PLAYERS.get().map(|p| &**p)) {
        return;
    }
    let mut parts = line.split_whitespace();
    match parts.next() {
        Some("gamerule") => {
            let Some(rule) = parts.next() else {
                eprintln!("console: gamerule needs a rule name: {line:?}");
                return;
            };
            match parts.next() {
                Some(value) => match rules.set(rule, value) {
                    Ok(v) => println!("console: gamerule {rule} = {}", v.serialize()),
                    Err(e) => eprintln!("console: {e}"),
                },
                None => match rules.get(rule) {
                    Some(v) => println!("console: gamerule {rule} = {}", v.serialize()),
                    None => eprintln!("console: no such game rule: {rule}"),
                },
            }
        }
        // `damage` — `DamageCommand`, the harness's verb for hurting a
        // player identically on both servers. Resolved and queued here; the
        // hurt itself runs on the target's own loop, which owns its health.
        // See `crate::damagecmd`.
        Some("damage") => {
            let Some(players) = PLAYERS.get() else {
                eprintln!("console: damage — no player registry yet: {line:?}");
                return;
            };
            let now = CLOCK.get().map_or(0, crate::tick::GameClock::ticks);
            if let Err(e) = crate::damagecmd::dispatch(line, players, now) {
                eprintln!("console: {e}");
            }
        }
        // `kill` — `KillCommand`, which is `LivingEntity.kill`: a
        // `generic_kill` hit of `Float.MAX_VALUE`, queued like a `damage`.
        // It is also the corpus's restore for a damage check: see
        // `crate::playerdeath`.
        // `effect give` — `EffectCommands.giveEffect`, queued to the target's
        // loop like a hit; the only `/effect` form this console runs.
        Some("effect") => {
            let Some(players) = PLAYERS.get() else {
                eprintln!("console: effect — no player registry yet: {line:?}");
                return;
            };
            let now = CLOCK.get().map_or(0, crate::tick::GameClock::ticks);
            if let Err(e) = crate::damagecmd::dispatch_effect(line, players, now) {
                eprintln!("console: {e}");
            }
        }
        Some("kill") => {
            let Some(players) = PLAYERS.get() else {
                eprintln!("console: kill — no player registry yet: {line:?}");
                return;
            };
            let now = CLOCK.get().map_or(0, crate::tick::GameClock::ticks);
            if let Err(e) = crate::playerdeath::dispatch_kill(line, players, now) {
                eprintln!("console: {e}");
            }
        }
        Some("weather") => match crate::weather::dispatch_console(&mut parts) {
            Ok(key) => println!("console: {key}"),
            Err(e) => eprintln!("console: {e}"),
        },
        // `worldborder`, dispatched through the SAME tree and the same
        // executors a typed `/worldborder` reaches.
        //
        // # Why the console and not `Step::Command`
        //
        // `/worldborder` requires `LEVEL_GAMEMASTERS` and the gameplay
        // harness's two players are `oracle` and `oracle_eye` with no
        // `ops.json` on either server — a decision `record::Console`'s own
        // header records and argues, because op-ing them changes the
        // `entity_event` permission byte and the whole `commands` packet at
        // join for every scenario in the corpus. The console is the route
        // that needs neither: `MinecraftServer.createCommandSourceStack()` is
        // unconditionally full permission, because a process's own stdin has
        // nothing to check a `PermissionCheck` against.
        //
        // # The reply goes to stdout and NOT to a client, which is also vanilla
        //
        // `CommandSourceStack.sendSuccess` sends to the source — here the
        // server itself, which logs — and then `broadcastToAdmins`, which
        // reaches operators other than the source. With no `ops.json` there
        // are none, so a console `worldborder` puts **no chat packet** on
        // either server's wire and the only observable is the border packet.
        Some("worldborder") => {
            let text = line.trim();
            let level = crate::commandtree::Level::Owners;
            let resolved = match crate::commands::tree().resolve(text, level) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("console: {text:?} did not resolve ({e:?})");
                    return;
                }
            };
            let mut border = crate::worldborder::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // `CommandSourceStack.getPosition()` for a console source is the
            // level's spawn, which this server puts at the origin of its one
            // tile. It is read only by a relative `~` centre.
            // `game_time` is zero and that is not a stub. It is
            // `lerpSizeBetween`'s `lerpBegin`, which `WorldBorder` stores and
            // which **only `getLerpSpeed()` reads back** — no packet carries
            // it, `set_border_lerp_size` carrying `lerpProgress` and not
            // `lerpEnd`. `bordercmd::tests::lerp_begin_reaches_no_packet`
            // asserts that, so the day a packet does read it this line is a
            // failing test rather than a wrong number.
            let outcome = crate::bordercmd::run(&resolved, &mut border, (0.0, 0.0), 0);
            drop(border);
            match outcome {
                Ok(o) => {
                    if let Some(players) = PLAYERS.get() {
                        crate::bordercmd::broadcast(players, &o.packets);
                    }
                    println!("console: {text} -> {} packet(s)", o.packets.len());
                }
                Err(e) => eprintln!("console: {text}: {e}"),
            }
        }
        // `scoreboard`, through the same tree and executors, for the same
        // permission reason `worldborder` is here.
        Some("scoreboard") => {
            let text = line.trim();
            let resolved =
                match crate::commands::tree().resolve(text, crate::commandtree::Level::Owners) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("console: {text:?} did not resolve ({e:?})");
                        return;
                    }
                };
            let mut board = crate::scoreboardcmd::shared()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let outcome = crate::scoreboardcmd::run(&resolved, &mut board);
            drop(board);
            match outcome {
                Ok(o) => {
                    if let Some(players) = PLAYERS.get() {
                        crate::bordercmd::broadcast(players, &o.packets);
                    }
                    println!("console: {text} -> {} packet(s)", o.packets.len());
                }
                Err(e) => eprintln!("console: {text}: {e}"),
            }
        }
        // `give` — `GiveCommand.giveItem`, queued to the target's own loop.
        // See `crate::givecmd` for what of it is here.
        Some("give") => {
            let Some(players) = PLAYERS.get() else {
                eprintln!("console: give — no player registry yet: {line:?}");
                return;
            };
            match crate::givecmd::dispatch(line.trim(), players) {
                Ok(()) => println!("console: {}", line.trim()),
                Err(e) => eprintln!("console: {e}"),
            }
        }
        Some("clear") => {
            let Some(players) = PLAYERS.get() else {
                eprintln!("console: clear — no player registry yet: {line:?}");
                return;
            };
            match crate::givecmd::dispatch_clear(line.trim(), players) {
                Ok(()) => println!("console: {}", line.trim()),
                Err(e) => eprintln!("console: {e}"),
            }
        }
        // `experience`/`xp` and the orb half of `summon`, queued for the clock
        // thread: `handleConsoleInputs` runs inside the server tick, after the
        // connection tick, and a line applied here on the reader thread would
        // land at an arbitrary point in it. See `crate::experience`.
        Some("xp" | "experience" | "summon" | "execute") => match parse_queued(line) {
            Ok(a) => crate::experience::queue_console(a),
            Err(e) => eprintln!("console: {line:?}: {e}"),
        },
        Some(other) => {
            eprintln!(
                "console: not understood — {other:?} (only `gamerule`, `weather`, `worldborder`, \
                 `scoreboard`, `xp`, `summon minecraft:experience_orb` and `execute at <player> \
                 run` are wired here)"
            );
        }
        None => {}
    }
}

/// The console lines `crate::experience` runs, parsed.
///
/// * `xp|experience add|set|query ...` — see `crate::experience::parse_xp`;
/// * `summon [minecraft:]experience_orb <x> <y> <z> [<tag>]` — `Vec3Argument`
///   coordinates: `~n` relative, and an absolute x or z **without a decimal
///   point gets +0.5** (`WorldCoordinate.parseDouble(reader, true)`), y never;
/// * `execute at <player> run <one of the above>` — `at` sets the source's
///   position to the player's, which is what `~` is relative to.
///
/// Any other entity type for `summon` is refused by name: this console does
/// not pretend to summon what it cannot build.
///
/// # Errors
/// A message naming what did not parse.
pub fn parse_queued(line: &str) -> Result<crate::experience::ConsoleAction, String> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    let (relative_to, rest): (Option<String>, &[&str]) = match parts.as_slice() {
        ["execute", "at", who, "run", rest @ ..] => {
            if who.starts_with('@') {
                return Err(format!("selector {who:?}: player names only"));
            }
            (Some((*who).to_string()), rest)
        }
        ["execute", ..] => return Err("only `execute at <player> run ...` is wired".into()),
        all => (None, all),
    };
    match rest {
        ["xp" | "experience", args @ ..] => {
            crate::experience::parse_xp(args).map(crate::experience::ConsoleAction::Xp)
        }
        ["summon", kind, x, y, z, tag @ ..] => {
            if kind.strip_prefix("minecraft:").unwrap_or(kind) != "experience_orb" {
                return Err(format!("summon {kind}: only experience_orb is wired here"));
            }
            let coord = |t: &str, center: bool| -> Result<(f64, bool), String> {
                if let Some(r) = t.strip_prefix('~') {
                    let v = if r.is_empty() {
                        0.0
                    } else {
                        r.parse::<f64>().map_err(|e| format!("{t}: {e}"))?
                    };
                    Ok((v, true))
                } else {
                    let v = t.parse::<f64>().map_err(|e| format!("{t}: {e}"))?;
                    Ok((
                        if center && !t.contains('.') {
                            v + 0.5
                        } else {
                            v
                        },
                        false,
                    ))
                }
            };
            let at = [coord(x, true)?, coord(y, false)?, coord(z, true)?];
            let (value, count, age, health) = crate::xporb::parse_tag(&tag.join(" "))?;
            Ok(crate::experience::ConsoleAction::SummonOrb(
                crate::xporb::SummonOrb {
                    relative_to,
                    at,
                    value,
                    count,
                    age,
                    health,
                },
            ))
        }
        other => Err(format!("not wired: {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamerules::GameRuleValue;

    fn fresh() -> GameRules {
        GameRules::fresh()
    }

    #[test]
    fn gamerule_set_takes_effect() {
        let rules = fresh();
        dispatch_line("gamerule random_tick_speed 1000", &rules);
        assert_eq!(rules.random_tick_speed(), 1000);
    }

    #[test]
    fn gamerule_set_then_restore_round_trips() {
        let rules = fresh();
        dispatch_line("gamerule random_tick_speed 1000", &rules);
        dispatch_line("gamerule random_tick_speed 0", &rules);
        assert_eq!(rules.random_tick_speed(), 0);
    }

    #[test]
    fn a_query_does_not_change_anything() {
        let rules = fresh();
        dispatch_line("gamerule random_tick_speed", &rules);
        assert_eq!(rules.random_tick_speed(), 3, "still vanilla's default");
    }

    /// An unknown rule is refused, not silently accepted — a dropped
    /// `[[gamerule]]` is exactly the defect this module exists to prevent,
    /// so a line that names the wrong rule must not read as success.
    #[test]
    fn an_unknown_rule_does_not_get_created() {
        let rules = fresh();
        dispatch_line("gamerule doFireTick true", &rules);
        assert_eq!(rules.get("doFireTick"), None);
    }

    /// A line naming a command this console does not understand is not
    /// silently dropped from a test's point of view either: nothing panics
    /// and nothing in the store moves.
    #[test]
    fn an_unrecognised_command_touches_nothing() {
        let rules = fresh();
        let before = rules.get("random_tick_speed");
        dispatch_line("stop", &rules);
        assert_eq!(rules.get("random_tick_speed"), before);
    }

    #[test]
    fn a_bool_rule_round_trips_through_dispatch() {
        let rules = fresh();
        dispatch_line("gamerule mob_griefing false", &rules);
        assert_eq!(rules.get("mob_griefing"), Some(GameRuleValue::Bool(false)));
    }

    #[test]
    fn an_orb_summon_parses_relative_to_a_player() {
        let a =
            parse_queued("execute at oracle run summon minecraft:experience_orb ~1 ~ ~ {Value:3}")
                .expect("parses");
        let crate::experience::ConsoleAction::SummonOrb(s) = a else {
            panic!("not a summon")
        };
        assert_eq!(s.relative_to.as_deref(), Some("oracle"));
        assert_eq!(s.at, [(1.0, true), (0.0, true), (0.0, true)]);
        assert_eq!((s.value, s.count), (3, 1));
    }

    #[test]
    fn an_absolute_integer_x_and_z_are_centred_and_y_is_not() {
        let a = parse_queued("summon experience_orb 10 64 -3").expect("parses");
        let crate::experience::ConsoleAction::SummonOrb(s) = a else {
            panic!("not a summon")
        };
        assert_eq!(s.at, [(10.5, false), (64.0, false), (-2.5, false)]);
    }

    #[test]
    fn a_summon_of_anything_else_is_refused() {
        assert!(parse_queued("summon minecraft:pig 0 0 0").is_err());
        assert!(parse_queued("execute as oracle run xp add oracle 1").is_err());
    }
}
