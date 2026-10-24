//! `level-probe` — write and read a world directory's `level.dat`, its
//! `data/minecraft/*.dat` files and its `players/data/<uuid>.dat`, from
//! outside the server.
//!
//! # Why this exists
//!
//! `ci/t2-vanilla-opens-our-world.sh` needs a world directory built by *our*
//! writer and handed to a real vanilla server. Booting the whole server to
//! make one would put the server's own behaviour inside a gate that is about
//! the file format, and would make a red ambiguous between the two. This
//! writes the files and nothing else.
//!
//! ```text
//! level-probe write   <world-dir> <seed> [level-name]
//! level-probe dump    <world-dir>
//! level-probe player  <world-dir> <uuid-hex> <x> <y> <z>
//! level-probe getplayer <world-dir> <uuid-hex>
//! ```

use rustmc_world::level::{self, saved, LevelData, SavedDataFile, Spawn};
use rustmc_world::playerdat;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = || -> ! {
        eprintln!(
            "usage:\n  \
             level-probe write <world-dir> <seed> [level-name]\n  \
             level-probe dump <world-dir>\n  \
             level-probe player <world-dir> <uuid-hex> <x> <y> <z>\n  \
             level-probe getplayer <world-dir> <uuid-hex>"
        );
        std::process::exit(2)
    };
    let Some(cmd) = args.first().map(String::as_str) else {
        usage()
    };
    let result = match (cmd, args.len()) {
        ("write", 3 | 4) => write_world(
            Path::new(&args[1]),
            args[2].parse().unwrap_or_else(|_| usage()),
            args.get(3).map_or("world", String::as_str),
        ),
        ("dump", 2) => dump(Path::new(&args[1])),
        ("player", 6) => write_player(
            Path::new(&args[1]),
            parse_uuid(&args[2]).unwrap_or_else(|| usage()),
            [
                args[3].parse().unwrap_or_else(|_| usage()),
                args[4].parse().unwrap_or_else(|_| usage()),
                args[5].parse().unwrap_or_else(|_| usage()),
            ],
        ),
        ("getplayer", 3) => get_player(
            Path::new(&args[1]),
            parse_uuid(&args[2]).unwrap_or_else(|| usage()),
        ),
        _ => usage(),
    };
    if let Err(e) = result {
        eprintln!("level-probe: {e}");
        std::process::exit(1);
    }
}

/// A uuid with or without hyphens.
fn parse_uuid(text: &str) -> Option<u128> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    (hex.len() == 32).then(|| u128::from_str_radix(&hex, 16).ok())?
}

/// Write the whole set. `initialized` is left **false**, which is what the
/// server writes and why: nothing here picks a spawn point, so vanilla must.
fn write_world(dir: &Path, seed: i64, name: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir.join("data/minecraft"))?;
    std::fs::create_dir_all(dir.join("players/data"))?;
    std::fs::create_dir_all(dir.join("datapacks"))?;
    let mut data = LevelData::new(name);
    data.initialized = false;
    data.spawn = Spawn::default();
    data.server_brands = vec!["rustmc".into()];
    level::write_dat_safely(&dir.join("level.dat"), "", &data.to_file_nbt())?;

    let target = dir.join("data/minecraft");
    let write = |file: &str, payload: rustmc_world::nbt::Nbt| -> std::io::Result<()> {
        level::write_dat_safely(
            &target.join(file),
            "",
            &SavedDataFile::new(payload).to_file_nbt(),
        )
    };
    write(
        "world_gen_settings.dat",
        saved::world_gen_settings(seed, true, false),
    )?;
    write("weather.dat", saved::weather(false, 0, false, 0, 1_000_000))?;
    write("world_clocks.dat", saved::world_clocks(0, 0))?;
    write(
        "random_sequences.dat",
        saved::random_sequences(0, saved::empty()),
    )?;
    write("scheduled_events.dat", saved::scheduled_events())?;
    write("stopwatches.dat", saved::stopwatches())?;
    write("scoreboard.dat", saved::empty())?;
    write("custom_boss_events.dat", saved::empty())?;
    // Deliberately not `game_rules.dat`: the rule table lives in the server
    // crate and this tool does not depend on it. Vanilla writes its own
    // defaults for a world that has none, which is the correct behaviour to
    // be exercising here anyway.
    println!("wrote {} (seed {seed}, LevelName {name})", dir.display());
    Ok(())
}

/// Print the fields a gate greps for, one `key=value` per line.
fn dump(dir: &Path) -> std::io::Result<()> {
    let level = LevelData::from_file_nbt(&level::read_dat(&dir.join("level.dat"))?)?;
    println!("LevelName={}", level.level_name);
    println!("DataVersion={}", level.data_version);
    println!("Time={}", level.time);
    println!("GameType={}", level.game_type);
    println!("initialized={}", level.initialized);
    println!("ServerBrands={}", level.server_brands.join(","));
    let gen = level::read_dat(&dir.join("data/minecraft/world_gen_settings.dat"))?;
    let seed = match gen.get("data").and_then(|d| d.get("seed")) {
        Some(rustmc_world::nbt::Nbt::Long(v)) => *v,
        _ => return Err(std::io::Error::other("world_gen_settings.dat has no seed")),
    };
    println!("seed={seed}");
    Ok(())
}

fn write_player(dir: &Path, uuid: u128, pos: [f64; 3]) -> std::io::Result<()> {
    let player_dir = dir.join("players/data");
    std::fs::create_dir_all(&player_dir)?;
    let mut player = playerdat::read(&player_dir, uuid)?
        .unwrap_or_else(|| playerdat::PlayerData::fresh(uuid, pos, 1, level::DATA_VERSION));
    player.pos = pos;
    playerdat::write(&player_dir, &player)?;
    println!(
        "wrote {}",
        player_dir.join(playerdat::file_name(uuid)).display()
    );
    Ok(())
}

fn get_player(dir: &Path, uuid: u128) -> std::io::Result<()> {
    match playerdat::read(&dir.join("players/data"), uuid)? {
        None => {
            println!("absent");
            Ok(())
        }
        Some(p) => {
            // `{:?}` and not `{}`: Display prints an integral f64 as `14`
            // and Debug as `14.0`, and the wire protocol's own doubles are
            // read back by gates that compare the two as strings. Getting
            // this wrong cost one red on
            // `ci/t2-player-survives-a-restart.sh` that named a disagreement
            // between two identical numbers.
            println!("pos={:?},{:?},{:?}", p.pos[0], p.pos[1], p.pos[2]);
            println!("dimension={}", p.dimension);
            println!("keys={}", p.extra.len() + 24);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_uuid;

    /// The two persistence gates hand this binary a player's uuid as text,
    /// hyphenated or not, and read back `players/data/<uuid>.dat`. A uuid
    /// that parses to the wrong value reads some other file — or none — and
    /// the gate's verdict is then about a player nobody joined as. This is
    /// the one piece of the binary that is not a pass-through to
    /// `rustmc_world`, and until train-c the target held no test at all
    /// (`ci/check-test-targets.py`: EXECUTES NO ASSERTION).
    #[test]
    fn a_uuid_parses_the_same_with_or_without_hyphens_and_refuses_the_wrong_length() {
        let plain = parse_uuid("0123456789abcdef0123456789abcdef");
        assert_eq!(plain, Some(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef));
        assert_eq!(parse_uuid("01234567-89ab-cdef-0123-456789abcdef"), plain);
        assert_eq!(parse_uuid("0123456789abcdef0123456789abcde"), None);
        assert_eq!(parse_uuid("0123456789abcdef0123456789abcdef0"), None);
        assert_eq!(parse_uuid("0123456789abcdef0123456789abcdeg"), None);
    }
}
