//! `crate::cmdexec`'s replies against the bytes vanilla's own code produces.
//!
//! The golden side is `data/vanilla/commands_tree.json`'s `replies` map,
//! written by `ci/commands-golden.sh` (`CommandsGolden.appendExecutorReplies`),
//! which builds each component by calling the command class's own private
//! message lambda, raising its own static exception type, and running
//! `CommandSourceStack.sendFailure` on a real stack. So a row here fails when
//! this server's component differs from vanilla's in any byte — key order,
//! list wrapping, a missing style — and not only when it renders differently.
//!
//! A missing golden is a panic, never a skip.

use rustmc_server::cmdexec as cx;
use rustmc_server::commands::system_chat_body;
use rustmc_world::nbt::Nbt;

fn golden() -> serde_json::Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../data/vanilla/commands_tree.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} — run ci/gen-gameplay-tables.sh ({e})", path.display()));
    serde_json::from_str(&text).expect("the golden is json")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn check(rows: Vec<(String, Nbt)>) {
    let g = golden();
    assert!(!rows.is_empty());
    for (key, component) in rows {
        let want = g["replies"][key.as_str()]
            .as_str()
            .unwrap_or_else(|| panic!("the golden has no `{key}` — run ci/commands-golden.sh"));
        let body = system_chat_body(&component).expect("buf");
        assert_eq!(hex(&body), want, "{key} does not match vanilla's bytes");
    }
}

#[test]
fn the_difficulty_replies_are_vanillas() {
    let mut rows = Vec::new();
    for name in cx::DIFFICULTIES {
        rows.push((
            format!("difficultySuccess_{name}"),
            cx::difficulty_success(name),
        ));
        rows.push((
            format!("difficultyQuery_{name}"),
            cx::difficulty_query(name),
        ));
        let cx::Failure(raw) = cx::difficulty_failure(name);
        rows.push((
            format!("difficultyFailureLine_{name}"),
            cx::failure_line(&raw),
        ));
        rows.push((format!("difficultyFailure_{name}"), raw));
    }
    assert_eq!(rows.len(), 16, "four difficulties, four replies each");
    check(rows);
}

#[test]
fn the_admin_broadcast_line_is_vanillas() {
    check(vec![(
        "adminLineConsoleDifficulty".to_string(),
        cx::admin_line(
            &Nbt::String(cx::CONSOLE_NAME.into()),
            &cx::difficulty_success("hard"),
        ),
    )]);
}

#[test]
fn the_gamemode_replies_are_vanillas() {
    let mut rows = Vec::new();
    for mode in cx::GAME_TYPES {
        rows.push((
            format!("gamemodeSelf_{mode}"),
            cx::gamemode_success_self(mode),
        ));
        rows.push((
            format!("gamemodeChanged_{mode}"),
            cx::game_mode_changed(mode),
        ));
    }
    assert_eq!(rows.len(), 8);
    check(rows);
}

#[test]
fn the_selector_and_source_refusals_are_vanillas() {
    let cx::Failure(not_player) = cx::not_player();
    let cx::Failure(none) = cx::no_players_found();
    let rows = vec![
        ("notPlayer".to_string(), not_player),
        ("noPlayersFound".to_string(), none),
    ];
    assert_eq!(rows.len(), 2);
    check(rows);
}

#[test]
fn the_weather_replies_are_vanillas() {
    // The keys `crate::weather::WeatherArm::message` hands the executor,
    // through the same translatable builder the executor uses.
    check(
        ["clear", "rain", "thunder"]
            .into_iter()
            .map(|arm| {
                (
                    format!("weather_{arm}"),
                    rustmc_server::commands::translatable(
                        &format!("commands.weather.set.{arm}"),
                        &[],
                    ),
                )
            })
            .collect(),
    );
}

#[test]
fn the_teleport_refusals_and_count_reply_are_vanillas() {
    let cx::Failure(none) = cx::Failure::translatable("argument.entity.notfound.entity", &[]);
    let cx::Failure(not_entity) = cx::not_entity();
    let cx::Failure(invalid) = cx::invalid_position();
    let rows = vec![
        ("noEntitiesFound".to_string(), none),
        ("notEntity".to_string(), not_entity),
        ("teleportInvalidPosition".to_string(), invalid),
        (
            "teleportLocationMultiple".to_string(),
            cx::teleport_location_multiple(2, [12.5, -60.0, 0.000_000_5]),
        ),
    ];
    assert_eq!(rows.len(), 4);
    check(rows);
}

#[test]
fn the_experience_count_replies_are_vanillas() {
    let mut rows = Vec::new();
    for kind in ["points", "levels"] {
        rows.push((
            format!("experienceAddMultiple_{kind}"),
            cx::experience_multiple("add", kind, 7, 2),
        ));
        rows.push((
            format!("experienceSetMultiple_{kind}"),
            cx::experience_multiple("set", kind, 0, 2),
        ));
    }
    let cx::Failure(invalid) = cx::set_points_invalid();
    rows.push(("experienceSetPointsInvalid".to_string(), invalid));
    check(rows);
}

#[test]
fn the_defaultgamemode_replies_are_vanillas() {
    check(
        cx::GAME_TYPES
            .into_iter()
            .map(|m| {
                (
                    format!("defaultGameMode_{m}"),
                    cx::default_game_mode_success(m),
                )
            })
            .collect(),
    );
}

#[test]
fn the_title_count_replies_are_vanillas() {
    let two = ["oracle", "oracle_eye"];
    check(vec![
        (
            "title_lambda$clearTitle$1".to_string(),
            cx::title_reply("cleared", &two),
        ),
        (
            "title_lambda$resetTitle$1".to_string(),
            cx::title_reply("reset", &two),
        ),
        (
            "title_lambda$setTimes$1".to_string(),
            cx::title_reply("times", &two),
        ),
    ]);
}

#[test]
fn the_time_replies_are_vanillas() {
    check(vec![
        ("timeSet".to_string(), cx::time_set_reply(6000)),
        ("timeAdd".to_string(), cx::time_add_reply(30000)),
        ("timeQuery".to_string(), cx::time_query_reply(6000)),
        (
            "timeQueryGame".to_string(),
            cx::time_query_game_time_reply(1_234_567),
        ),
    ]);
}

#[test]
fn the_setblock_replies_are_vanillas() {
    let cx::Failure(failed) = cx::setblock_failed();
    let cx::Failure(unloaded) = cx::pos_unloaded();
    let cx::Failure(outside) = cx::pos_out_of_world();
    check(vec![
        (
            "setblockSuccess".to_string(),
            cx::setblock_success((-7, 64, 12)),
        ),
        ("setblockFailed".to_string(), failed),
        ("posUnloaded".to_string(), unloaded),
        ("posOutOfWorld".to_string(), outside),
    ]);
}

#[test]
fn the_fill_replies_are_vanillas() {
    let cx::Failure(failed) = cx::fill_failed();
    let cx::Failure(big) = cx::fill_too_big(32768, 50_000);
    check(vec![
        ("fillSuccess".to_string(), cx::fill_success(27)),
        ("fillFailed".to_string(), failed),
        ("fillTooBig".to_string(), big),
    ]);
}

#[test]
fn the_playsound_replies_are_vanillas() {
    let cx::Failure(failed) = cx::playsound_failed();
    check(vec![
        (
            "playsoundMultiple".to_string(),
            cx::playsound_multiple("minecraft:block.note_block.bell", 2),
        ),
        ("playsoundFailed".to_string(), failed),
    ]);
}

#[test]
fn the_stopsound_replies_are_vanillas() {
    let bell = "minecraft:block.note_block.bell";
    check(vec![
        (
            "stopsoundSourceSound".to_string(),
            cx::stopsound_reply(Some("block"), Some(bell)),
        ),
        (
            "stopsoundSourceAny".to_string(),
            cx::stopsound_reply(Some("block"), None),
        ),
        (
            "stopsoundSound".to_string(),
            cx::stopsound_reply(None, Some(bell)),
        ),
        ("stopsoundAny".to_string(), cx::stopsound_reply(None, None)),
    ]);
}

#[test]
fn the_clone_replies_are_vanillas() {
    let cx::Failure(overlap) = cx::clone_overlap();
    let cx::Failure(failed) = cx::clone_failed();
    let cx::Failure(big) = cx::clone_too_big(32768, 40_000);
    check(vec![
        ("cloneSuccess".to_string(), cx::clone_success(9)),
        ("cloneOverlap".to_string(), overlap),
        ("cloneFailed".to_string(), failed),
        ("cloneTooBig".to_string(), big),
    ]);
}

#[test]
fn the_title_show_replies_are_vanillas() {
    let two = ["oracle", "oracle_eye"];
    check(
        ["title", "subtitle", "actionbar"]
            .into_iter()
            .map(|w| {
                (
                    format!("titleShow_{w}"),
                    cx::title_reply(&format!("show.{w}"), &two),
                )
            })
            .collect(),
    );
}

#[test]
fn the_random_replies_are_vanillas() {
    let cx::Failure(small) = cx::random_too_small();
    let cx::Failure(large) = cx::random_too_large();
    check(vec![
        ("randomSample".to_string(), cx::random_sample_success(4)),
        ("randomTooSmall".to_string(), small),
        ("randomTooLarge".to_string(), large),
        (
            "randomRollConsole".to_string(),
            cx::random_roll(&Nbt::String(cx::CONSOLE_NAME.into()), 3, 1, 6),
        ),
    ]);
}

/// `/rotate`, `/swing`, `/particle`, `/teammsg` and `/dialog clear`. The single-target
/// replies are gated over a literal name — the key and the argument shape;
/// the display name itself is `crate::commands`' and gated there.
#[test]
fn the_rotate_swing_particle_teammsg_and_dialog_replies_are_vanillas() {
    let name = Nbt::String("oracle".into());
    let cx::Failure(not_living) = cx::swing_not_living();
    let cx::Failure(particle_failed) = cx::particle_failed();
    let cx::Failure(no_team) = cx::teammsg_no_team();
    check(vec![
        ("rotateSuccess".to_string(), cx::rotate_success(&name)),
        ("swingSingle".to_string(), cx::swing_single(&name)),
        ("swingMultiple".to_string(), cx::swing_multiple(2)),
        ("swingNotLiving".to_string(), not_living),
        (
            "particleSuccess".to_string(),
            cx::particle_success("minecraft:flame"),
        ),
        ("particleFailed".to_string(), particle_failed),
        ("teammsgNoTeam".to_string(), no_team),
        (
            "dialogClearSingle".to_string(),
            cx::dialog_clear_single(&name),
        ),
        (
            "dialogClearMultiple".to_string(),
            cx::dialog_clear_multiple(2),
        ),
    ]);
}

/// `/bossbar`'s replies, over a bar built the way `CustomBossEvent`'s
/// constructor builds one — white, value 0, max 100, visible — and the golden's
/// own `CustomBossEvent` of the same id and name.
#[test]
fn the_bossbar_replies_are_vanillas() {
    use rustmc_server::bossbar::Bar;
    use rustmc_server::commands::Arg;
    let mut bar = Bar::new("minecraft:t", 0, Nbt::String("T".into()));
    let unchanged = |f: &str| {
        let cx::Failure(raw) = cx::bossbar_unchanged(f);
        (format!("bossbar_{f}"), raw)
    };
    let cx::Failure(exists) = cx::bossbar_already_exists("minecraft:t");
    let cx::Failure(unknown) = cx::bossbar_unknown("minecraft:t");
    let mut rows = vec![
        (
            "bossbarCreate".to_string(),
            cx::bossbar_reply("create.success", &bar, &[]),
        ),
        (
            "bossbarRemove".to_string(),
            cx::bossbar_reply("remove.success", &bar, &[]),
        ),
        ("bossbarListNone".to_string(), cx::bossbar_list(&[])),
        ("bossbarListOne".to_string(), cx::bossbar_list(&[&bar])),
        (
            "bossbarGetValue".to_string(),
            cx::bossbar_reply("get.value", &bar, &[Arg::Int(0)]),
        ),
        (
            "bossbarGetMax".to_string(),
            cx::bossbar_reply("get.max", &bar, &[Arg::Int(100)]),
        ),
        (
            "bossbarGetVisible".to_string(),
            cx::bossbar_reply("get.visible.visible", &bar, &[]),
        ),
        (
            "bossbarGetPlayersNone".to_string(),
            cx::bossbar_reply("get.players.none", &bar, &[]),
        ),
        (
            "bossbarSetPlayersNone".to_string(),
            cx::bossbar_reply("set.players.success.none", &bar, &[]),
        ),
        (
            "bossbarSetValue".to_string(),
            cx::bossbar_reply("set.value.success", &bar, &[Arg::Int(50)]),
        ),
        (
            "bossbarSetMax".to_string(),
            cx::bossbar_reply("set.max.success", &bar, &[Arg::Int(200)]),
        ),
        (
            "bossbarSetName".to_string(),
            cx::bossbar_reply("set.name.success", &bar, &[]),
        ),
        ("bossbarExists".to_string(), exists),
        ("bossbarUnknown".to_string(), unknown),
    ];
    bar.color = 2;
    rows.push((
        "bossbarSetColorRed".to_string(),
        cx::bossbar_reply("set.color.success", &bar, &[]),
    ));
    bar.color = 5;
    rows.push((
        "bossbarSetColorPurple".to_string(),
        cx::bossbar_reply("set.color.success", &bar, &[]),
    ));
    rows.push((
        "bossbarSetHidden".to_string(),
        cx::bossbar_reply("set.visible.success.hidden", &bar, &[]),
    ));
    rows.push((
        "bossbarGetHidden".to_string(),
        cx::bossbar_reply("get.visible.hidden", &bar, &[]),
    ));
    for f in [
        "ERROR_NO_PLAYER_CHANGE",
        "ERROR_NO_NAME_CHANGE",
        "ERROR_NO_COLOR_CHANGE",
        "ERROR_NO_STYLE_CHANGE",
        "ERROR_NO_VALUE_CHANGE",
        "ERROR_NO_MAX_CHANGE",
        "ERROR_ALREADY_HIDDEN",
        "ERROR_ALREADY_VISIBLE",
    ] {
        rows.push(unchanged(f));
    }
    check(rows);
}
