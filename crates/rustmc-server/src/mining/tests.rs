//! The dig arithmetic against hand-derived values, and the state machine
//! against the observer stream vanilla recorded on 2026-09-30.

use super::*;

fn row(destroy_time: f32, requires_correct_tool: bool) -> Row {
    Row {
        destroy_time,
        requires_correct_tool,
        game_master: false,
        will_destroy: None,
        player_destroy: None,
        spawn_after_break: None,
    }
}

fn table() -> Option<BlockMining> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push(TABLE_PATH);
    if !rustmc_data::fixture::available(&path, "block mining table", "ci/mining-golden.sh") {
        return None;
    }
    Some(BlockMining::load(&path).expect("block_mining.json parses"))
}

/// The extract carries the destroy times vanilla's `Blocks.<clinit>` sets,
/// and keys a full state string by its block.
#[test]
fn the_table_reads_the_blocks_the_corpus_digs() {
    let Some(t) = table() else { return };
    assert!(t.len() > 1000, "{} blocks", t.len());
    let grass = t.row("minecraft:grass_block[snowy=false]").expect("grass");
    assert_eq!(grass.destroy_time, 0.6);
    assert!(!grass.requires_correct_tool);
    let stone = t.row("minecraft:stone").expect("stone");
    assert_eq!(stone.destroy_time, 1.5);
    assert!(stone.requires_correct_tool);
    assert_eq!(
        t.row("minecraft:bedrock").expect("bedrock").destroy_time,
        -1.0
    );
    assert_eq!(t.row("minecraft:torch").expect("torch").destroy_time, 0.0);
    assert!(t.row("minecraft:command_block").expect("cb").game_master);
    assert!(t.row("minecraft:not_a_block").is_none());
}

/// Bare hands on grass: `1.0 / 0.6 / 30`, one tick's worth. The recorded
/// observer stream of `002` climbs one stage every 1.8 ticks, which is this.
#[test]
fn bare_hands_on_grass_is_one_over_eighteen_a_tick() {
    let p = destroy_progress(&row(0.6, false), &Digger::default());
    assert_eq!(p, 1.0f32 / 0.6 / 30.0);
    assert_eq!(stage(p), 0, "START shows stage 0");
    assert_eq!(stage(p * 2.0), 1, "the first tick shows stage 1");
}

/// A block that requires a tool, dug without one, divides by **100**: bare
/// hands on stone is `1 / 1.5 / 100`, 150 ticks to break.
#[test]
fn the_wrong_tool_divides_by_a_hundred_not_thirty() {
    let hand = destroy_progress(&row(1.5, true), &Digger::default());
    assert_eq!(hand, 1.0f32 / 1.5 / 100.0);
    let pick = Digger {
        tool_speed: 2.0,
        item_correct_for_drops: true,
        ..Digger::default()
    };
    assert_eq!(
        destroy_progress(&row(1.5, true), &pick),
        2.0f32 / 1.5 / 30.0
    );
    // A block that does NOT require a tool is always /30, whatever is held.
    assert_eq!(
        destroy_progress(&row(0.6, false), &Digger::default()),
        1.0f32 / 0.6 / 30.0
    );
}

/// `-1` is unbreakable and returns 0 before anything divides; `0` divides by
/// zero and that infinity is the insta-mine.
#[test]
fn unbreakable_is_zero_and_instant_is_infinite() {
    assert_eq!(destroy_progress(&row(-1.0, false), &Digger::default()), 0.0);
    let torch = destroy_progress(&row(0.0, false), &Digger::default());
    assert!(torch.is_infinite() && torch >= 1.0);
}

/// Efficiency adds only on top of a tool that is already faster than a hand
/// (`fcmpl; ifle` at offset 15: strictly greater than one).
#[test]
fn mining_efficiency_needs_a_speed_above_one() {
    let with = |speed: f32| Digger {
        tool_speed: speed,
        mining_efficiency: 26.0,
        ..Digger::default()
    };
    assert_eq!(destroy_speed(&with(1.0)), 1.0);
    assert_eq!(destroy_speed(&with(8.0)), 34.0);
}

/// Haste, fatigue, airborne and underwater, each on its own, in `float`.
#[test]
fn each_multiplier_is_the_listings_constant() {
    let d = Digger {
        tool_speed: 8.0,
        ..Digger::default()
    };
    let haste = Digger {
        dig_speed_amplifier: Some(1),
        ..d
    };
    assert_eq!(destroy_speed(&haste), 8.0 * (1.0 + 2.0f32 * 0.2));
    for (amp, scale) in [
        (0, 0.3f32),
        (1, 0.09),
        (2, 0.0027),
        (3, 8.1e-4),
        (9, 8.1e-4),
    ] {
        let tired = Digger {
            fatigue_amplifier: Some(amp),
            ..d
        };
        assert_eq!(destroy_speed(&tired), 8.0 * scale, "fatigue {amp}");
    }
    let airborne = Digger {
        on_ground: false,
        ..d
    };
    assert_eq!(destroy_speed(&airborne), 8.0 / 5.0);
    let wet = Digger {
        eye_in_water: true,
        ..d
    };
    assert_eq!(destroy_speed(&wet), 8.0 * 0.2f64 as f32);
}

/// The stages vanilla's `002` put on the observer's stream: every stage from
/// 0 upward, one at a time, past 9 and with no cap, and the block never
/// destroyed by the tick — only the STOP or the delayed arm destroys.
struct Solid(f32);
impl World for Solid {
    fn is_air(&self, _pos: BlockPos) -> bool {
        false
    }
    fn progress(&self, _pos: BlockPos) -> f32 {
        self.0
    }
}

struct Air;
impl World for Air {
    fn is_air(&self, _pos: BlockPos) -> bool {
        true
    }
    fn progress(&self, _pos: BlockPos) -> f32 {
        1.0
    }
}

const GRASS: f32 = 1.0 / 0.6 / 30.0;

#[test]
fn an_unfinished_dig_climbs_past_nine_and_never_breaks() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    let pos = (1, -1, 0);
    let mut stages = Vec::new();
    for e in gm.handle(Dig::Start, pos, false, &w) {
        match e {
            Effect::Progress(_, s) => stages.push(s),
            Effect::Attack(p) => assert_eq!(p, pos),
            other => panic!("{other:?}"),
        }
    }
    for _ in 0..86 {
        for e in gm.tick(&w) {
            match e {
                Effect::Progress(p, s) => {
                    assert_eq!(p, pos);
                    stages.push(s);
                }
                other => panic!("the tick never destroys a dug block: {other:?}"),
            }
        }
    }
    let want: Vec<i32> = (0..=48).collect();
    assert_eq!(stages, want, "0..=48, the recorded 4.3 s of 002");
    assert!(gm.is_destroying_block);
}

/// `004`: a STOP two ticks in is below 0.7, sends nothing, and arms the
/// delayed destroy, which fires on the tick the server's own progress
/// reaches one — tick 17 for bare-handed grass, `(17 + 1) / 18 >= 1`.
#[test]
fn an_early_stop_breaks_the_block_when_the_servers_clock_says_so() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    let pos = (1, -1, 0);
    gm.handle(Dig::Start, pos, false, &w);
    gm.tick(&w);
    gm.tick(&w);
    assert!(
        gm.handle(Dig::Stop, pos, false, &w).is_empty(),
        "no packet at STOP"
    );
    assert!(gm.has_delayed_destroy && !gm.is_destroying_block);
    let mut destroyed_at = None;
    for t in 3..40 {
        if gm.tick(&w).contains(&Effect::Destroy(pos)) {
            destroyed_at = Some(t);
            break;
        }
    }
    assert_eq!(
        destroyed_at,
        Some(17),
        "gameTicks 17: (17 - 0 + 1) * f >= 1"
    );
    assert!(!gm.has_delayed_destroy);
}

/// A STOP at or past 0.7 clears the overlay and destroys, in that order.
#[test]
fn an_on_time_stop_clears_the_overlay_then_destroys() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    let pos = (1, -1, 0);
    gm.handle(Dig::Start, pos, false, &w);
    for _ in 0..12 {
        gm.tick(&w);
    }
    // (12 + 1) / 18 = 0.72 >= 0.7.
    assert_eq!(
        gm.handle(Dig::Stop, pos, false, &w),
        vec![Effect::Progress(pos, -1), Effect::DestroyAndAck(pos)]
    );
    assert!(!gm.is_destroying_block);
}

/// Eleven ticks is 0.667 — below the threshold — and that is the delayed arm.
#[test]
fn one_tick_short_of_the_threshold_is_the_delayed_arm() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    gm.handle(Dig::Start, (0, 0, 0), false, &w);
    for _ in 0..11 {
        gm.tick(&w);
    }
    assert!(gm.handle(Dig::Stop, (0, 0, 0), false, &w).is_empty());
    assert!(gm.has_delayed_destroy);
}

/// The corpus shares one session across scenarios: a dig left unfinished is
/// re-sent to the digger by the next START, before the new overlay.
#[test]
fn a_dig_left_unfinished_is_resent_by_the_next_start() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    gm.handle(Dig::Start, (-31, -1, 0), false, &w);
    gm.tick(&w);
    assert_eq!(
        gm.handle(Dig::Start, (1, -1, 0), false, &w),
        vec![
            Effect::Attack((1, -1, 0)),
            Effect::ResendBlock((-31, -1, 0)),
            Effect::Progress((1, -1, 0), 0)
        ]
    );
}

/// A STOP for a position that is not `destroyPos` does nothing at all.
#[test]
fn a_stop_somewhere_else_is_ignored() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    gm.handle(Dig::Start, (1, -1, 0), false, &w);
    assert!(gm.handle(Dig::Stop, (2, -1, 0), false, &w).is_empty());
    assert!(gm.is_destroying_block, "the real dig is still going");
    // And with no START at all, `destroyPos` is ZERO: a STOP there on air
    // is still nothing.
    let mut fresh = GameMode::new();
    assert!(fresh.handle(Dig::Stop, (0, 0, 0), false, &Air).is_empty());
}

/// An ABORT for the wrong position clears both overlays, the old one first.
#[test]
fn an_abort_clears_the_overlay_and_a_mismatch_clears_both() {
    let w = Solid(GRASS);
    let mut gm = GameMode::new();
    gm.handle(Dig::Start, (1, -1, 0), false, &w);
    assert_eq!(
        gm.handle(Dig::Abort, (1, -1, 0), false, &w),
        vec![Effect::Progress((1, -1, 0), -1)]
    );
    gm.handle(Dig::Start, (1, -1, 0), false, &w);
    assert_eq!(
        gm.handle(Dig::Abort, (5, -1, 0), false, &w),
        vec![
            Effect::Progress((1, -1, 0), -1),
            Effect::Progress((5, -1, 0), -1)
        ]
    );
    assert!(!gm.is_destroying_block);
}

/// A creative START is `destroyAndAck` and touches no field; a survival START
/// on a torch is too, through the `f >= 1` arm.
#[test]
fn creative_and_insta_mine_both_destroy_on_start() {
    let mut gm = GameMode::new();
    assert_eq!(
        gm.handle(Dig::Start, (1, 0, 0), true, &Solid(GRASS)),
        vec![Effect::DestroyAndAck((1, 0, 0))]
    );
    assert_eq!(gm, GameMode::new());
    assert_eq!(
        gm.handle(Dig::Start, (1, 0, 0), false, &Solid(f32::INFINITY)),
        vec![Effect::Attack((1, 0, 0)), Effect::DestroyAndAck((1, 0, 0))]
    );
    // NaN — a zero speed on a zero destroy time — is not an insta-mine.
    let mut gm = GameMode::new();
    assert_eq!(
        gm.handle(Dig::Start, (1, 0, 0), false, &Solid(f32::NAN)),
        vec![Effect::Attack((1, 0, 0)), Effect::Progress((1, 0, 0), 0)]
    );
}

/// Bedrock: stage 0 at START and never another, however long the dig.
#[test]
fn an_unbreakable_block_shows_stage_zero_once() {
    let w = Solid(0.0);
    let mut gm = GameMode::new();
    assert_eq!(
        gm.handle(Dig::Start, (1, -4, 0), false, &w),
        vec![Effect::Attack((1, -4, 0)), Effect::Progress((1, -4, 0), 0)]
    );
    for _ in 0..200 {
        assert!(gm.tick(&w).is_empty());
    }
}

/// The block turning to air under a dig clears the overlay once.
#[test]
fn a_block_that_vanishes_mid_dig_clears_the_overlay() {
    let mut gm = GameMode::new();
    gm.handle(Dig::Start, (1, -1, 0), false, &Solid(GRASS));
    assert_eq!(gm.tick(&Air), vec![Effect::Progress((1, -1, 0), -1)]);
    assert_eq!(gm.last_sent_state, -1);
    assert!(gm.tick(&Air).is_empty());
}

/// `writeByte(-1)` is `0xFF`, after a varint id and a packed position.
#[test]
fn the_clear_stage_is_the_byte_ff() {
    let body = block_destruction_body(3, (1, -1, 0), -1).expect("body");
    assert_eq!(body.len(), 1 + 8 + 1);
    assert_eq!(body[0], 3);
    assert_eq!(body[9], 0xFF);
    assert_eq!(
        block_destruction_body(3, (1, -1, 0), 48).expect("body")[9],
        48
    );
}

/// The range test is strict and measured from the block's box to the eye:
/// 5.5 blocks is the edge for a default player.
#[test]
fn the_reach_is_five_and_a_half_from_the_eye_to_the_box() {
    let eye = (0.5, 1.62, 0.5);
    assert!(within_interaction_range((1, -1, 0), eye, 4.5));
    // Box face at x = 6.0, eye at 0.5: exactly 5.5 away, and `<` refuses it.
    assert!(!within_interaction_range((6, 1, 0), eye, 4.5));
    assert!(within_interaction_range((5, 1, 0), eye, 4.5));
}

fn with_classes(will: Option<&str>, player: Option<&str>) -> Row {
    Row {
        will_destroy: will.map(|c| format!("net.minecraft.world.level.block.{c}")),
        player_destroy: player.map(|c| format!("net.minecraft.world.level.block.{c}")),
        ..row(0.5, false)
    }
}

/// The dispatch is on the declaring class, so `frosted_ice` — whose row
/// names `IceBlock` because `FrostedIceBlock` inherits it — lands in the ice
/// arm, and a class this server does not know falls back to `Block`'s.
#[test]
fn player_destroy_dispatches_on_the_declaring_class() {
    assert_eq!(player_destroy(None), PlayerDestroy::Default);
    assert_eq!(
        player_destroy(Some(&row(1.5, true))),
        PlayerDestroy::Default
    );
    let of = |c| player_destroy(Some(&with_classes(None, Some(c))));
    assert_eq!(of("IceBlock"), PlayerDestroy::Ice);
    assert_eq!(of("DoublePlantBlock"), PlayerDestroy::DoublePlant);
    assert_eq!(of("TurtleEggBlock"), PlayerDestroy::TurtleEgg);
    assert_eq!(of("BeehiveBlock"), PlayerDestroy::Beehive);
    assert_eq!(of("SomeFutureBlock"), PlayerDestroy::Default);
}

/// Only a double plant drops from `playerWillDestroy`, and only for a player
/// whose drops are not prevented; a creative one runs
/// `preventDropFromBottomPart` instead.
#[test]
fn only_a_survival_double_plant_drops_on_will_destroy() {
    let plant = with_classes(Some("DoublePlantBlock"), Some("DoublePlantBlock"));
    assert!(will_destroy_drops(Some(&plant), false));
    assert!(!will_destroy_drops(Some(&plant), true));
    let door = with_classes(Some("DoorBlock"), None);
    assert!(!will_destroy_drops(Some(&door), false));
    assert!(!will_destroy_drops(None, false));
}

/// `IceBlock.playerDestroy`'s four exits, in the bytecode's order: silk touch
/// first (it beats even an evaporating dimension), then the dimension, then
/// the block below.
#[test]
fn ice_melts_only_over_something_that_holds_water() {
    use IceAfter::{Nothing, Remove, Water};
    assert_eq!(ice_after_destroy(true, true, true, true), Nothing);
    assert_eq!(ice_after_destroy(false, true, true, false), Remove);
    assert_eq!(ice_after_destroy(false, false, true, false), Water);
    assert_eq!(ice_after_destroy(false, false, false, true), Water);
    assert_eq!(ice_after_destroy(false, false, false, false), Nothing);
}

#[test]
fn three_blocks_are_liquid() {
    assert!(is_liquid_block("minecraft:water"));
    assert!(is_liquid_block("minecraft:lava"));
    assert!(is_liquid_block("minecraft:bubble_column"));
    assert!(!is_liquid_block("minecraft:ice"));
    assert!(!is_liquid_block("minecraft:kelp"));
}

#[test]
fn a_clutch_of_turtle_eggs_loses_one() {
    assert_eq!(turtle_eggs_after_destroy(1), None);
    assert_eq!(turtle_eggs_after_destroy(2), Some(1));
    assert_eq!(turtle_eggs_after_destroy(4), Some(3));
}

/// The extract agrees with the census `player_destroy` was written against.
#[test]
fn the_extract_names_the_four_player_destroy_classes() {
    let Some(t) = table() else { return };
    let kind = |b: &str| player_destroy(t.row(b));
    assert_eq!(kind("minecraft:ice"), PlayerDestroy::Ice);
    assert_eq!(kind("minecraft:frosted_ice[age=0]"), PlayerDestroy::Ice);
    assert_eq!(
        kind("minecraft:sunflower[half=lower]"),
        PlayerDestroy::DoublePlant
    );
    assert_eq!(
        kind("minecraft:turtle_egg[eggs=1,hatch=0]"),
        PlayerDestroy::TurtleEgg
    );
    assert_eq!(
        kind("minecraft:beehive[facing=north,honey_level=0]"),
        PlayerDestroy::Beehive
    );
    assert_eq!(kind("minecraft:stone"), PlayerDestroy::Default);
    assert!(will_destroy_drops(
        t.row("minecraft:sunflower[half=upper]"),
        false
    ));
}

/// Six draws a try in x, y, z order, each pair's first draw the minuend;
/// the first position the predicate accepts wins, and a thousand refusals
/// leave the egg where it is.
#[test]
fn a_dragon_egg_lands_on_the_first_spot_that_fits() {
    use rustmc_worldgen::rng::legacy::LegacyRandomSource;
    use rustmc_worldgen::rng::RandomSource;
    let mut a = LegacyRandomSource::new(5);
    let mut b = LegacyRandomSource::new(5);
    let mut tries = 0;
    let got = dragon_egg_target((10, 64, -3), &mut a, |_| {
        tries += 1;
        tries == 3
    });
    let mut want = None;
    for _ in 0..3 {
        let dx = b.next_i32_bound(16) - b.next_i32_bound(16);
        let dy = b.next_i32_bound(8) - b.next_i32_bound(8);
        let dz = b.next_i32_bound(16) - b.next_i32_bound(16);
        want = Some((10 + dx, 64 + dy, -3 + dz));
    }
    assert_eq!(got, want);
    assert_eq!(a.next_i64(), b.next_i64(), "eighteen draws, no more");
    let mut c = LegacyRandomSource::new(5);
    assert_eq!(dragon_egg_target((0, 0, 0), &mut c, |_| false), None);
}

/// The two `playerWillDestroy` overrides `crate::dig` dispatches on by class
/// besides the double plant: fire's extinguish event and the piston head's
/// creative base removal.
#[test]
fn the_extract_names_fire_and_the_piston_head_as_will_destroy_classes() {
    assert_eq!(will_destroy_class(None), None);
    let Some(t) = table() else { return };
    assert_eq!(
        will_destroy_class(t.row("minecraft:fire[age=0]")),
        Some("BaseFireBlock")
    );
    assert_eq!(
        will_destroy_class(t.row("minecraft:soul_fire")),
        Some("BaseFireBlock")
    );
    assert_eq!(
        will_destroy_class(t.row("minecraft:piston_head[facing=north,short=false,type=normal]")),
        Some("piston.PistonHeadBlock")
    );
    assert_eq!(will_destroy_class(t.row("minecraft:stone")), None);
}
