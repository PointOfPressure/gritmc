//! **`/weather` end to end: a line down a real fifo moves the real tick.**
//!
//! Everything between the console and the wire is unit-tested in
//! `crate::weather` against a caller-supplied level and a scripted
//! generator. What no unit test in that module can see is whether the three
//! pieces are actually *connected* — whether `crate::console::dispatch_line`
//! reaches `crate::weather::dispatch_console`, whether that writes the state
//! `crate::weather::tick` reads, and whether the packets that come back
//! describe the transition the console asked for.
//!
//! That is the class of defect this project keeps paying for: a mechanism
//! derived correctly, tested thoroughly, and wired to nothing. `ACCEPT_
//! TELEPORTATION` was decoded into a field read by nothing; `crate::
//! randomtick` sat gated and unwired for a fortnight while its unit tests
//! were green. So this file asks the only question those tests cannot: does
//! a line of text arriving on a fifo change what the next tick broadcasts.
//!
//! # Why it is its own test binary
//!
//! `crate::weather::shared()` and `crate::weather::level_random()` are
//! process-wide singletons, exactly as `MinecraftServer.getWeatherData()` and
//! `Level.random` are one object each in vanilla. `cargo test` runs a
//! module's tests on parallel threads in one process, so a unit test that
//! drove the shared state would be racing every other test that touched it
//! — and would sometimes pass for the wrong reason, which is the
//! contamination `tests/plugin_tick_lock.rs`'s header documents catching in
//! this same workspace.
//!
//! An integration test is its own binary. This file therefore holds exactly
//! **one** test, and the shared weather in this process is touched by
//! nothing else. Do not add a second.
//!
//! # What turns it red
//!
//! Deleting the `Some("weather")` arm from `crate::console::dispatch_line`:
//! the console then answers "not understood" on stderr, the shared state
//! never moves, and the deadline below expires naming the state it saw.
//! Changing `LEVEL_STEP`, the `> 0.2` threshold, the packet ids or the
//! transition arm's send list all move the assertions further down.

use std::io::Write;
use std::time::{Duration, Instant};

use rustmc_server::weather::{self, Audience, Event};

/// How long to wait for the console thread to pick a line off the fifo.
///
/// A bound, not a sleep: the loop below exits the instant the state moves,
/// and reaching this deadline is a **failure** with the observed state in
/// the message. A test that waited forever would be indistinguishable from
/// a test that hung, and one that waited a fixed time and then asserted
/// would be flaky on a loaded box.
const DEADLINE: Duration = Duration::from_secs(10);

#[test]
fn a_console_weather_line_reaches_the_next_tick_and_the_wire() {
    let dir = std::env::temp_dir().join(format!("rustmc-weather-console-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let fifo = dir.join("console.fifo");
    let _ = std::fs::remove_file(&fifo);
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo runs");
    assert!(made.success(), "mkfifo {}", fifo.display());

    // The reading end, which is what `crate::main`'s `--console <path>`
    // starts. It opens read-write, so the write below cannot block.
    weather::shared()
        .lock()
        .map(|mut w| *w = weather::Weather::new(true))
        .expect("a fresh level");
    rustmc_server::console::start(fifo.to_str().expect("utf-8")).expect("console starts");

    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(&fifo)
        .expect("the writing end");

    // ---- rain -------------------------------------------------------------

    writeln!(writer, "weather rain 300").expect("written");
    writer.flush().expect("flushed");

    let deadline = Instant::now() + DEADLINE;
    loop {
        let data = weather::shared().lock().expect("not poisoned").data;
        if data.raining {
            assert_eq!(data.rain_time, 300, "the duration argument, not a sample");
            assert_eq!(data.thunder_time, 300, "setWeatherParameters aliases it");
            assert!(!data.thundering, "`rain` is raining=true thundering=false");
            assert_eq!(data.clear_weather_time, 0);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the console never set the weather: {data:?} after {DEADLINE:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    // ---- the tick that follows it -----------------------------------------
    //
    // `advance_weather` is at its vanilla default of true here, so the
    // timers run too; the transition is driven by the level ramp either way.
    let mut ticks = 0_u32;
    let mut transition = None;
    while ticks < 200 {
        ticks += 1;
        let sends = weather::tick();
        if sends.iter().any(|s| s.event == Event::StartRaining) {
            transition = Some(sends);
            break;
        }
    }
    let sends = transition.unwrap_or_else(|| panic!("no START_RAINING in {ticks} ticks"));
    assert_eq!(ticks, 20, "twenty f32 steps of 0.01f cross 0.2");
    assert_eq!(
        sends.iter().map(|s| s.event).collect::<Vec<_>>(),
        vec![
            Event::RainLevelChange,
            Event::StartRaining,
            Event::RainLevelChange,
            Event::ThunderLevelChange,
        ],
        "advanceWeatherCycle offsets 372, 460, 509, 533"
    );
    assert_eq!(sends[0].audience, Audience::ThisDimension);
    assert!(sends[1..]
        .iter()
        .all(|s| s.audience == Audience::EveryPlayer));

    // The bodies are what a client would receive: one byte, one big-endian
    // float, five bytes each.
    let bodies: Vec<Vec<u8>> = sends.iter().map(|s| s.body()).collect();
    assert_eq!(bodies[1], vec![1, 0, 0, 0, 0], "START_RAINING, param 0.0f");
    assert_eq!(bodies[3][0], 8, "THUNDER_LEVEL_CHANGE");
    assert!(bodies.iter().all(|b| b.len() == 5));

    // And a player joining now is told about it, which is the other half of
    // the wiring — `PlayerList.sendLevelInfo`'s arm.
    let join = weather::shared()
        .lock()
        .expect("not poisoned")
        .join_events();
    assert_eq!(
        join.iter().map(|e| e.0).collect::<Vec<_>>(),
        vec![
            Event::StartRaining,
            Event::RainLevelChange,
            Event::ThunderLevelChange
        ]
    );

    // ---- clear ------------------------------------------------------------
    //
    // The control for everything above: the same route in the other
    // direction. Without it, "the state moved" could be read as "this
    // process started raining", which it did not — it started dry, and the
    // first loop's timeout is the proof.

    writeln!(writer, "weather clear 300").expect("written");
    writer.flush().expect("flushed");

    let deadline = Instant::now() + DEADLINE;
    loop {
        let data = weather::shared().lock().expect("not poisoned").data;
        if !data.raining {
            assert_eq!(data.clear_weather_time, 300);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the console never cleared the weather: {data:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }

    let mut ticks = 0_u32;
    let mut stopped = false;
    while ticks < 400 {
        ticks += 1;
        if weather::tick()
            .iter()
            .any(|s| s.event == Event::StopRaining)
        {
            stopped = true;
            break;
        }
    }
    assert!(stopped, "the level never ramped back under 0.2");

    let _ = std::fs::remove_file(&fifo);
    let _ = std::fs::remove_dir(&dir);
}
