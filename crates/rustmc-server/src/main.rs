//! rustmc binary — subcommand surface pinned by ORACLE.md (see lib.rs::cli).

/// `--flag <value>`, parsed, or `default` if the flag is absent.
///
/// # A FLAG THAT DID NOT PARSE USED TO BE A FLAG THAT WAS NOT THERE
///
/// Found 2026-08-16 by making this target assert; it had executed zero tests
/// on every gate this project has run. Every option below was
/// `.and_then(|v| v.parse().ok()).unwrap_or(DEFAULT)`, so
/// `--view-distance twelve`, `--port 99999` (out of `u16`) and
/// `--radius ""` each ran the server on the **default** and said nothing.
///
/// That is not a cosmetic difference here. `ci/t2-gameplay.sh` sets
/// `--view-distance` and `--radius` on this server to the same values it sets
/// on the vanilla one, because a differential trace whose two sides load
/// different amounts of world is not a comparison — the comment on
/// `--view-distance` below says exactly that. A typo, a renamed flag or a
/// value out of range silently reverted our side to vanilla's *default* while
/// the harness's log recorded the value it had asked for, and every trace
/// would agree with every other trace while all of them were wrong.
///
/// So: absent is the default, present-and-unreadable is a refusal. Unknown
/// flags stay ignored, which is a separate and deliberate decision recorded
/// in the `run` arm.
fn flag<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    match value_of(args, name) {
        None => default,
        Some(raw) => match raw.parse() {
            Ok(v) => v,
            Err(_) => {
                eprintln!(
                    "{name}: expected a {}, got `{raw}`. It is refused rather than \
                     defaulted — a run on a value nobody asked for is worse than no run.",
                    std::any::type_name::<T>()
                );
                std::process::exit(2);
            }
        },
    }
}

/// The token after `name`, or `None` if the flag is absent or is last.
///
/// Split out from [`flag`] so it can be tested without a `process::exit` in
/// the way, and so `--world`, which takes a path rather than a parsed value,
/// reads its argument by the same rule.
fn value_of<'a>(args: &'a [String], name: &str) -> Option<&'a String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        // Oracle harness argv form: --genregions <seed> <radius> <cx> <cz> <dim>
        Some("--genregions") => {
            eprintln!("--genregions: not yet implemented (wave 2; contract in ORACLE.md §3)");
            std::process::exit(2);
        }
        Some("run") => {
            // `--port <p>`; anything else is ignored for now rather than
            // rejected, so adding flags later does not break a caller.
            let port: u16 = flag(&args, "--port", 25565);
            // `--view-distance <n>`: `server.properties`' setting of the same
            // name, in chunks, defaulting to vanilla's own 10. It is not
            // `--radius`, which is how big a generated tile is; conflating
            // the two is exactly the bug this flag exists to make
            // unrepeatable. Anything comparing this server against a vanilla
            // one has to set both to the same thing on both sides —
            // `ci/t2-gameplay.sh` does.
            // `server.properties`, which nothing in this workspace opened
            // until 2026-09-05: `view-distance` arrived only on argv and
            // `simulation-distance` existed nowhere, so the join packet
            // carried a literal 10 whatever the file said.
            //
            // Precedence is argv over file over default, which is the order
            // a flag is useful in. A malformed value is named rather than
            // quietly defaulted — see `rustmc_server::properties`.
            let props = rustmc_server::properties::Properties::load(std::path::Path::new(
                "server.properties",
            ))
            .unwrap_or_else(|e| {
                eprintln!("server.properties: {e} — running on defaults");
                rustmc_server::properties::Properties::default()
            });
            for key in &props.bad_values {
                eprintln!("server.properties: `{key}` is not an integer — using the default");
            }
            if props.unparsed_lines > 0 {
                eprintln!(
                    "server.properties: {} line(s) were neither a comment nor `key=value`",
                    props.unparsed_lines
                );
            }
            // Present-and-unreadable on argv is a refusal, as for every flag
            // here (see [`flag`]); only an ABSENT flag falls through to the
            // file. The `0` is never used: `flag` is reached only when
            // `value_of` found the flag.
            let arg_i32 = |name: &str| -> Option<i32> {
                value_of(&args, name).map(|_| flag::<i32>(&args, name, 0))
            };
            let view_distance: i32 = arg_i32("--view-distance")
                .or(props.view_distance)
                .unwrap_or(rustmc_server::status::DEFAULT_VIEW_DISTANCE);
            // `--simulation-distance <n>`: how far the world *ticks*, which
            // is not how far it renders. See
            // `rustmc_server::status::StatusInfo::simulation_distance`.
            let simulation_distance: i32 = arg_i32("--simulation-distance")
                .or(props.simulation_distance)
                .unwrap_or(rustmc_server::status::DEFAULT_SIMULATION_DISTANCE);
            // `entity-broadcast-range-percentage`, which arrives already
            // clamped to `10..=1000` because that is where vanilla clamps it
            // — in the property's own parse function, not at the point of
            // use. No argv flag: vanilla has none either, and a flag for a
            // setting `DedicatedServer` only ever reads out of the file
            // would be a knob this server has and vanilla does not.
            let entity_broadcast_range_percentage: i32 = props
                .entity_broadcast_range_percentage
                .unwrap_or(rustmc_server::tracker::DEFAULT_BROADCAST_RANGE_PERCENTAGE);
            let info = rustmc_server::status::StatusInfo {
                view_distance,
                simulation_distance,
                entity_broadcast_range_percentage,
                ..rustmc_server::status::StatusInfo::default()
            };

            // Vanilla's "Preparing spawn area", and for the same reason: the
            // world takes tens of seconds and a client's read timeout is
            // twenty. Generating it during login makes every join fail at
            // exactly that mark, blaming whatever packet happened to be in
            // flight. So it is built before the socket is open, and a player
            // who can connect can already be sent a world.
            //
            // A failure here is reported and not fatal — the server still
            // serves the noise stage, which is a real if smaller world.
            // `--radius <n>`: how big a **tile** is. Worth a flag rather
            // than a constant because the cost is quadratic and the right
            // trade — a bigger tile against a longer startup and a longer
            // pause when a player walks into a new one — is the operator's,
            // not ours. It is no longer how big the world is: the world grows
            // a tile at a time as players walk (`crate::tiles`).
            let radius: i32 = flag(&args, "--radius", rustmc_server::status::WORLD_RADIUS);
            // `--level-type flat`: vanilla's superflat, the substrate the
            // gameplay oracle measures in (crate::flat). Chosen before any
            // world is generated, because `Tiles` is a singleton and the
            // first tile decides what every later one is.
            //
            // A missing extract is fatal here rather than a fallback to the
            // noise world: a differential check that silently ran against
            // terrain would report every scenario as a divergence, and the
            // one thing worse than a red gate is a red gate that blames the
            // wrong thing.
            let level_type = value_of(&args, "--level-type").map_or("normal", String::as_str);
            if level_type == "flat" {
                if let Err(e) = rustmc_server::flat::select(std::path::Path::new(
                    "data/vanilla/flat_preset.json",
                )) {
                    eprintln!("--level-type flat: {e}");
                    std::process::exit(1);
                }
            } else if level_type != "normal" {
                eprintln!("--level-type: expected `normal` or `flat`, got `{level_type}`");
                std::process::exit(2);
            }

            // `--gamemode <creative|survival>`: the mode joining players
            // are put in, defaulting to creative — the substrate every check
            // in `ci/checks/gameplay` was measured under.
            //
            // It is a flag rather than a constant because creative makes four
            // whole mechanics **ungradeable**, not merely unported: vanilla's
            // own `Player.hurtServer` returns before it sends anything when
            // `Abilities.invulnerable` is set, so no amount of work on this
            // server could ever have produced a `damage_event` to compare.
            // `ci/substrate-gate-audit.py` reads the gates out of the pinned
            // jar and `ci/t2-gameplay-survival.sh` is the pass that uses this.
            //
            // Rejected rather than ignored, unlike the flags above: a
            // misspelled mode that silently fell back to creative would make
            // a survival pass record a creative server and report the whole
            // difference as this server's divergence.
            if let Some(i) = args.iter().position(|a| a == "--gamemode") {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("--gamemode: expected `creative` or `survival`");
                    std::process::exit(2);
                };
                if let Err(bad) = rustmc_server::status::set_game_type(name) {
                    eprintln!("--gamemode: expected `creative` or `survival`, got `{bad}`");
                    std::process::exit(2);
                }
            }

            // The sky. Fatal on a missing extract for the same reason the
            // flat preset is: a server that silently had no sun would answer
            // every daylight detector `power = 0`, which is a plausible
            // number and therefore the worst possible failure.
            if let Err(e) = rustmc_server::sky::select(
                std::path::Path::new("data/vanilla/timelines.json"),
                "minecraft:overworld",
            ) {
                eprintln!("sky: {e}");
                std::process::exit(1);
            }
            // The other half of `getEffectiveSkyBrightness`. Without it
            // `crate::light::sky_light` answers `None` at every position and
            // every daylight detector declines — which is a *quiet* version
            // of having no sky at all, and is exactly the failure this pair
            // of loads is fatal to prevent. Fatal here for the same reason
            // the sky is: a detector that never moves looks like a detector
            // whose power happens not to have changed.
            if let Err(e) =
                rustmc_server::light::select(std::path::Path::new("data/vanilla/block_props.json"))
            {
                eprintln!("light: {e}");
                std::process::exit(1);
            }

            // `--day-time <n>` and `--advance-time <true|false>`: this
            // level's day time and whether it moves. See `crate::sky` for
            // why a differential harness has to pin both, and note that
            // `advance-time false` stops only the *day* time — the game
            // clock keeps ticking, so scheduled ticks are unaffected.
            let day_time: i64 = match args
                .iter()
                .position(|a| a == "--day-time")
                .and_then(|i| args.get(i + 1))
            {
                None => 0,
                Some(v) => match v.parse() {
                    Ok(n) => n,
                    Err(_) => {
                        eprintln!("--day-time: expected an integer, got `{v}`");
                        std::process::exit(2);
                    }
                },
            };
            let advance_time = match args
                .iter()
                .position(|a| a == "--advance-time")
                .and_then(|i| args.get(i + 1))
            {
                None => true,
                Some(v) if v == "true" => true,
                Some(v) if v == "false" => false,
                Some(v) => {
                    eprintln!("--advance-time: expected `true` or `false`, got `{v}`");
                    std::process::exit(2);
                }
            };
            rustmc_server::sky::set_day_time(day_time, advance_time);

            // The worldgen block-entity side channel on, unless the operator
            // set it either way. A generated container's loot table and its
            // `setBlockEntityLootTable` draw reach the block-entity store only
            // through it (`crate::lootroll::adopt_generated`); with it off a
            // structure's chest is a chest-shaped block with nothing behind it.
            // `ci/be-channel-identical.sh` is the gate that recording leaves
            // every generated block state unchanged.
            if std::env::var_os("RUSTMC_RECORD_BLOCK_ENTITIES").is_none() {
                std::env::set_var("RUSTMC_RECORD_BLOCK_ENTITIES", "1");
            }

            // `--seed <long>`: `WorldOptions.seed()`, which this server reads
            // in exactly one place — `MinecraftServer.getRandomSequence`,
            // where it seeds every loot table's named random sequence. The
            // gameplay harness passes the seed it boots vanilla with; without
            // it the sequences seed from 0 and a seedless loot chest rolls
            // what vanilla would roll in a different world.
            if let Some(v) = args
                .iter()
                .position(|a| a == "--seed")
                .and_then(|i| args.get(i + 1))
            {
                match v.parse::<i64>() {
                    Ok(n) => rustmc_server::lootroll::set_world_seed(n),
                    Err(_) => {
                        eprintln!("--seed: expected a 64-bit integer, got `{v}`");
                        std::process::exit(2);
                    }
                }
            }

            // `--difficulty <name>`: `server.properties`' `difficulty`, read
            // by `EntityType.canSpawn`, `Mob.checkDespawn` and
            // `Mob.getMaxFallDistance`. Vanilla's default is `easy`; the
            // corpus's vanilla runs `peaceful`.
            if let Some(i) = args.iter().position(|a| a == "--difficulty") {
                let Some(name) = args.get(i + 1) else {
                    eprintln!("--difficulty: expected peaceful, easy, normal or hard");
                    std::process::exit(2);
                };
                if let Err(bad) = rustmc_server::mob::set_difficulty(name) {
                    eprintln!("--difficulty: expected peaceful, easy, normal or hard, got `{bad}`");
                    std::process::exit(2);
                }
            }

            // `--level-seed <long>`: `ServerLevel.getSeed()` for the random
            // sequences loot tables draw from — `server.properties`'
            // `level-seed`, which the corpus's vanilla is booted with. See
            // `status::level_seed` for what does not read it yet.
            if let Some(i) = args.iter().position(|a| a == "--level-seed") {
                let Some(v) = args.get(i + 1) else {
                    eprintln!("--level-seed: expected a long");
                    std::process::exit(2);
                };
                match v.parse::<i64>() {
                    Ok(n) => rustmc_server::status::set_level_seed(n),
                    Err(_) => {
                        eprintln!("--level-seed: expected a long, got `{v}`");
                        std::process::exit(2);
                    }
                }
            }

            // `--world <dir>`: where the world is saved and loaded from.
            //
            // Off by default, and deliberately. Every gate in this repo boots
            // this server in a scratch directory and measures it against a
            // vanilla one; a server that silently wrote a world folder would
            // have the *second* run of any gate load the first run's edits,
            // and a differential harness whose subject remembers the previous
            // trial is measuring the wrong thing. So persistence is something
            // an operator asks for.
            //
            // Named before the first tile is generated, because that is when
            // saved chunks are read back in.
            let world_dir = value_of(&args, "--world");
            if let Some(dir) = world_dir {
                rustmc_server::tiles::Tiles::persist_to(std::path::PathBuf::from(dir));
                // The world directory itself — `level.dat`, `session.lock`
                // and the nine `data/minecraft/*.dat` files. Before this,
                // `--world` wrote region files into a directory with no
                // `level.dat`, which `LevelStorageSource` treats as a world
                // that does not exist.
                //
                // **A refusal, not a warning.** The lock this takes is
                // vanilla's own, so a second server — ours or vanilla's —
                // pointed at the same directory is stopped here rather than
                // allowed to interleave writes into one region file.
                if let Err(e) =
                    rustmc_server::levelstore::open_shared(std::path::PathBuf::from(dir))
                {
                    eprintln!("--world {dir}: {e}");
                    std::process::exit(1);
                }
                // The loot tables' random sequences, continued rather than
                // restarted — before anything can roll.
                if let Some(store) = rustmc_server::levelstore::shared() {
                    match store.saved_random_sequences() {
                        Ok(Some(data)) => {
                            rustmc_server::lootroll::restore_sequences(&data);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("--world {dir}: random_sequences.dat: {e}");
                            std::process::exit(1);
                        }
                    }
                }
            }
            // `--console <fifo>`: the missing half of
            // `tools/oracle-runner`'s `--console`, documented at
            // `tools/oracle-runner/src/main.rs`'s `cmd_record` — that flag
            // opens a fifo as the *reference's* stdin; this is what lets the
            // subject be told through one too (`crate::console`). Off by
            // default, same as `--world`: a fifo that never gets created is
            // a fifo this server should not block trying to open, and every
            // gate that does not pass this flag must boot exactly as it did
            // before this existed.
            if let Some(path) = args
                .iter()
                .position(|a| a == "--console")
                .and_then(|i| args.get(i + 1))
            {
                if let Err(e) = rustmc_server::console::start(path) {
                    eprintln!("--console {path}: {e}");
                    std::process::exit(1);
                }
            }
            // `--autosave-seconds <n>`: the interval, expressed the way an
            // operator thinks about it and converted to the ticks vanilla
            // actually counts.
            //
            // **The default is not 300 written down again.** It is
            // `computeNextAutosaveInterval(20.0)` — 300 seconds' worth of
            // ticks at the default rate, floored at 100 — which is 6000, and
            // saying it that way is what stops the number being a constant
            // this server copied. `crate::autosave` has the bytecode.
            //
            // An explicit `<n>` is `n * 20`, and it is deliberately NOT put
            // through the floor: `MIMINUM_AUTOSAVE_TICKS` guards the
            // *derivation from a tick rate*, so that a server crawling at
            // 0.3 Hz does not autosave every third tick. An operator naming
            // an interval is not deriving one, and flooring their 2 seconds
            // up to 5 would be this server overruling a flag it offers.
            //
            // Present-and-unreadable is a refusal, as for every flag here
            // (see [`flag`]): the `0` below is never used, because the arm
            // is only reached when the flag is present.
            let autosave_ticks: i32 = match value_of(&args, "--autosave-seconds") {
                None => rustmc_server::autosave::next_interval(
                    rustmc_server::autosave::DEFAULT_TICKRATE,
                ),
                Some(_) => flag::<i32>(&args, "--autosave-seconds", 0).saturating_mul(20),
            };

            let started = std::time::Instant::now();
            match rustmc_server::tiles::Tiles::prepare(0, radius) {
                Ok(tiles) => {
                    let area = tiles.spawn_area();
                    // Both counts, because they answer different questions:
                    // the inner square is what a player can walk on, the
                    // grown one is what was paid for — and the difference is
                    // the price of tiling, paid again by every tile.
                    let grown = area.grown(rustmc_worldgen::pipeline::DECORATION_MARGIN);
                    eprintln!(
                        "spawn tile: {} chunks ({} with the margin) in {:.1}s; \
                         more are generated as players walk into them. not generated: {}",
                        area.chunk_count(),
                        grown.chunk_count(),
                        started.elapsed().as_secs_f64(),
                        tiles.caveats().join("; ")
                    );
                }
                Err(e) => eprintln!("no decorated world ({e}) — serving the noise stage only"),
            }

            // The autosave is armed here and RUN BY THE TICK LOOP, which is
            // where vanilla runs it: `MinecraftServer.tickServer` decrements
            // `ticksUntilAutosave` once per tick and calls `autoSave()` at
            // zero. What used to be here was a thread that slept N seconds,
            // and the difference is not cosmetic — a wall clock keeps
            // schedule while a saturated server's world falls behind it, so
            // the two intervals diverge in exactly the case where the save
            // matters most.
            //
            // `crate::autosave` carries the constants, the interval
            // computation and `saveEverything`'s order. Nothing is saved
            // without `--world`: the clock is simply not armed, and
            // `autosave::due_now()` answers false forever.
            // The shutdown hook, and it is armed only WITH `--world`: it
            // changes what SIGTERM does to this process, and doing that for a
            // server with nowhere to save would be a change to how the
            // process dies in exchange for nothing. `crate::shutdown` has the
            // async-signal-safety argument and the watchdog that keeps
            // SIGTERM meaning "stop" even if the tick thread wedges.
            if world_dir.is_some() {
                rustmc_server::shutdown::install();
            }
            if world_dir.is_some() && autosave_ticks > 0 {
                rustmc_server::autosave::arm(autosave_ticks);
                eprintln!(
                    "autosave every {autosave_ticks} ticks ({:.0}s at {:.0} Hz), \
                     first in {}",
                    f64::from(autosave_ticks) / 20.0,
                    rustmc_server::autosave::DEFAULT_TICKRATE,
                    rustmc_server::autosave::shared()
                        .map_or(0, rustmc_server::autosave::AutosaveClock::remaining),
                );
            }

            // The one line anything outside this process is entitled to read
            // as "the server is up", and it is printed by `status::listen`
            // calling back, after the bind, from immediately before the first
            // `accept`.
            //
            // It used to be printed here, ahead of the call. A failed bind
            // then produced a log reading `listening on 0.0.0.0:26783` and
            // then `listen: Address already in use (os error 98)`, and three
            // harness wait loops that grep for the first string broke out of
            // their wait and recorded a corpus against a port this process
            // does not hold. The exit status was always 1; it was simply
            // never the thing anyone looked at, because the log had already
            // said the word.
            //
            // The address comes from the callback rather than from `port` for
            // the same reason: it is the socket's own answer, so a proof that
            // reads the port back out of this line is reading a bound port.
            // Arm the corpus positive controls, or refuse to start.
            //
            // **A refusal, not a warning.** A control that fails to arm fires
            // zero times, and zero firings is this instrument's spelling of
            // "the scenario never reaches that code" — so a typo in
            // `RUSTMC_SABOTAGE`, or a binary built without the feature, would
            // report every check it was pointed at as covering dead code.
            // That is the exact false green `crate::sabotage` exists to find.
            match rustmc_server::sabotage::arm_from_env() {
                Ok(0) => {}
                Ok(n) => eprintln!("rustmc: {n} sabotage control(s) armed — THIS IS NOT A SERVER"),
                Err(e) => {
                    eprintln!("rustmc: {e}");
                    std::process::exit(1);
                }
            }
            let announce = |addr: &std::net::SocketAddr| {
                eprintln!(
                    "rustmc {} listening on {addr}{}",
                    rustmc_proto::GAME_VERSION,
                    world_dir.map_or_else(
                        || " (not saving: pass --world <dir> to persist)".to_string(),
                        // Ticks, because ticks are what is counted now.
                        // The seconds are kept beside them because an
                        // operator set the flag in seconds and a line that
                        // answered in a unit they did not use would read as
                        // the flag not having taken.
                        |d| format!(
                            ", saving to {d} every {autosave_ticks} ticks ({}s at 20 Hz)",
                            autosave_ticks / 20
                        ),
                    )
                );
            };
            if let Err(e) =
                rustmc_server::status::listen(("0.0.0.0", port), &info, announce, |peer, e| {
                    eprintln!("{peer}: {e}")
                })
            {
                eprintln!("listen: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            eprintln!("rustmc {} (Phase 0 scaffold)", rustmc_proto::GAME_VERSION);
            eprintln!("usage: rustmc <genregions|run> [args]");
            std::process::exit(2);
        }
    }
}

// ---------------------------------------------------------------------------

/// **This target executed zero assertions until 2026-08-16.**
///
/// It is a thin `main` over a library carrying 588, which is why nobody
/// noticed — but "thin" is not "no logic": everything the gameplay corpus
/// tells this server about the world it must load passes through the argv
/// reader below, and until this commit a value it could not read was
/// indistinguishable from a value nobody passed.
#[cfg(test)]
mod tests {
    use super::value_of;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn a_flags_value_is_the_token_after_it() {
        let a = argv("rustmc run --port 26783 --view-distance 12");
        assert_eq!(value_of(&a, "--port").map(String::as_str), Some("26783"));
        assert_eq!(
            value_of(&a, "--view-distance").map(String::as_str),
            Some("12")
        );
    }

    #[test]
    fn an_absent_flag_has_no_value_and_that_is_how_a_default_is_reached() {
        assert_eq!(value_of(&argv("rustmc run"), "--port"), None);
    }

    /// A flag at the very end of argv has no value. It must read as absent
    /// rather than index past the end.
    #[test]
    fn a_trailing_flag_has_no_value_rather_than_panicking() {
        assert_eq!(value_of(&argv("rustmc run --port"), "--port"), None);
    }

    /// **`--radius` and `--view-distance` are different quantities**, and the
    /// comment in the `run` arm says conflating them is "exactly the bug this
    /// flag exists to make unrepeatable". Nothing had ever executed a line
    /// that told them apart.
    #[test]
    fn radius_and_view_distance_are_read_separately() {
        let a = argv("rustmc run --radius 4 --view-distance 10");
        assert_eq!(value_of(&a, "--radius").map(String::as_str), Some("4"));
        assert_eq!(
            value_of(&a, "--view-distance").map(String::as_str),
            Some("10")
        );
        // And a run that sets only one of them leaves the other unset, rather
        // than letting the reader of one pick up the other's operand.
        let b = argv("rustmc run --view-distance 10");
        assert_eq!(value_of(&b, "--radius"), None);
    }

    /// The first occurrence wins, which is what `position` does; pinned so a
    /// later rewrite to `rposition` is a visible decision rather than a
    /// silent change of which value a duplicated flag runs on.
    #[test]
    fn a_repeated_flag_takes_its_first_value() {
        assert_eq!(
            value_of(&argv("rustmc run --port 1 --port 2"), "--port").map(String::as_str),
            Some("1")
        );
    }
}
