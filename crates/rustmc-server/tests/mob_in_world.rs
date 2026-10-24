//! A pig and a bat, end to end, on a superflat tile: spawned by egg and by
//! the `/summon`-shaped call, paired to a fake client, ticked through
//! `mob::tick_all` and broadcast through the real tracker.
//!
//! This is the instrument that lets each claim in `crate::mob` be checked
//! against the bytes a client receives without booting vanilla; the corpus
//! (`ci/checks/gameplay/081-*`, `082-*`) is the one that checks the same
//! bytes against vanilla's.
//!
//! Every table it needs is a `data/vanilla` extract; absence panics
//! (`rustmc_data::fixture`), it never skips.
use rustmc_proto::ids::play::cb;
use rustmc_server::entities::{Entities, Entity, PLAYER};
use rustmc_server::mob;
use rustmc_server::tracker::{TrackedPlayer, Tracker};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// `server.properties`' default `view-distance`, which `mob::tick_all` reads
/// for `ServerLevel.entityTickList` membership (`Mth.clamp(viewDistance, 2, 32)`).
const VIEW_DISTANCE: i32 = 10;

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Everything a client sent, as raw frames.
#[derive(Clone, Default)]
struct Recorder(Arc<Mutex<Vec<u8>>>);

impl Write for Recorder {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("recorder").extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Recorder {
    fn frames(&self) -> Vec<(i32, Vec<u8>)> {
        let bytes = self.0.lock().expect("recorder").clone();
        let mut cur = &bytes[..];
        let mut out = Vec::new();
        while let Ok((id, body)) = rustmc_proto::frame::read_frame(&mut cur, false) {
            out.push((id, body));
        }
        out
    }
    fn clear(&self) {
        self.0.lock().expect("recorder").clear();
    }
    fn count(&self, id: i32) -> usize {
        self.frames().iter().filter(|(i, _)| *i == id).count()
    }
    fn first(&self, id: i32) -> Option<Vec<u8>> {
        self.frames()
            .into_iter()
            .find(|(i, _)| *i == id)
            .map(|(_, b)| b)
    }
}

struct Rig {
    tiles: &'static rustmc_server::tiles::Tiles,
    entities: &'static Entities,
    tracker: &'static Tracker,
    kinds: &'static rustmc_server::entities::KindIds,
    recorder: Recorder,
    players: Vec<TrackedPlayer>,
    surface: f64,
}

/// One flat tile, one player standing on it at the origin, one client
/// recording what it is sent.
fn rig() -> Rig {
    std::env::set_current_dir(repo("")).expect("the repo root");
    for (what, by) in [
        ("data/vanilla/path_facts.json", "ci/path-facts-golden.sh"),
        ("data/vanilla/mob_variants.json", "ci/dump-mob-variants.sh"),
        ("data/vanilla/attributes.json", "ci/attributes-golden.sh"),
        ("data/vanilla/protocol_ids.json", "ci/dump-protocol-ids.sh"),
        ("data/vanilla/registries.bin", "ci/dump-registries.sh"),
        ("data/vanilla/entity_types.json", "ci/dump-entity-types.sh"),
    ] {
        assert!(
            rustmc_data::fixture::available(&repo(what), what, by),
            "fixture rule: absent means panic, not skip"
        );
    }
    rustmc_server::flat::select(std::path::Path::new("data/vanilla/flat_preset.json"))
        .expect("the flat preset");
    let tiles = rustmc_server::tiles::Tiles::prepare(0, 2).expect("a flat spawn tile");
    let entities = Entities::shared();
    let tracker = Tracker::shared();
    let kinds = rustmc_server::entities::shared_kind_ids().expect("entity_types.json");
    let surface = tiles.surface_at(0, 0);
    let recorder = Recorder::default();
    let entity_id = entities.next_id();
    let conn = Arc::new(rustmc_server::players::Connection::with_entity(
        1,
        "oracle",
        Box::new(recorder.clone()),
        None,
        entity_id,
        4,
        // The player stands at (0.5, surface, 0.5): chunk (0, 0), which is
        // the centre `with_entity`'s chunk view opens on (entity-tracker's
        // seventh argument; see `ChunkView::announced_center`).
        (0, 0),
    ));
    // The join's own two steps, as `plugin_tick_lock.rs` and `session_joint.rs`
    // do them: `enter_world` sends the spawn blob and tells the view
    // (`mark_sent`), then `updateChunkTracking` runs. Since entity-tracker,
    // `updatePlayer`'s third limb is `isChunkTracked`, so a view that has had
    // neither is EMPTY and no `add_entity` reaches this client — which is what
    // this test read as "no pig" on train-c until this block was added.
    conn.with_chunk_view(|v| {
        for dx in -4..=4 {
            for dz in -4..=4 {
                v.mark_sent((dx, dz));
            }
        }
        v.update_tracking((0, 0));
    });
    let me = Entity {
        id: entity_id,
        uuid: 1,
        kind: PLAYER,
        x: 0.5,
        y: surface,
        z: 0.5,
        y_rot: 0.0,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: (0.0, 0.0, 0.0),
        on_ground: true,
        data: 0,
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        synched: rustmc_server::synched::Synched::EntityOnly,
    };
    entities.insert(me);
    let players = vec![TrackedPlayer {
        conn,
        entity_id,
        pos: (0.5, surface, 0.5),
        chunk: (0, 0),
        view_distance: 4,
    }];
    Rig {
        tiles,
        entities,
        tracker,
        kinds,
        recorder,
        players,
        surface,
    }
}

fn read_varint(b: &[u8]) -> (i32, usize) {
    let mut cur = b;
    let v = rustmc_proto::varint::read_varint(&mut cur).expect("varint");
    (v, b.len() - cur.len())
}

/// One process, one shared entity list and mob store, so the three parts
/// run in order inside one test rather than racing each other's ticks.
#[test]
fn mobs_in_a_flat_world() {
    let r = rig();
    a_pig_spawns_ticks_stands_and_syncs_its_speed_on_its_next_gated_tick(&r);
    a_bat_past_128_blocks_from_every_player_is_discarded_and_a_pig_is_not(&r);
    a_summoned_pig_keeps_its_constructor_yaw_and_plays_no_ambient_sound(&r);
    // Before the feeding: the silent pig's 400 ticks count every sound frame,
    // and the fed pigs and their baby sound on their own ambient rolls.
    an_egg_carrying_silent_entity_data_spawns_a_pig_that_never_sounds(&r);
    fed_pigs_fall_in_love_breed_and_a_spawn_egg_hatches_a_baby(&r);
    the_four_passive_kinds_spawn_with_their_own_data(&r);
    a_bucket_milks_a_cow_and_a_bowl_a_mooshroom(&r);
    shears_turn_a_mooshroom_into_a_cow_and_five_mushrooms(&r);
    a_sheep_is_dyed_sheared_once_and_regrows_by_eating(&r);
    a_chicken_falls_at_six_tenths_and_lays_an_egg(&r);
    // Last: it hatches two more mobs, and so draws on the level random every
    // check above depends on.
    an_egg_carrying_left_handed_overrides_the_spawn_roll(&r);
}

/// `EntityType.create` applies the egg's tag (javap 179) AFTER
/// `Mob.finalizeSpawn`'s handedness roll (javap 168; `finalizeSpawn` 63-83,
/// `nextFloat() < 0.05`), so `LeftHanded` decides `DATA_MOB_FLAGS_ID` bit 2
/// whatever the roll said. `true` is the arm that can fail: the roll alone
/// leaves the bit clear nineteen times in twenty, so only an applied tag sets
/// it. `false` must then clear it, and it reaches the pairing's
/// `set_entity_data` either way.
fn an_egg_carrying_left_handed_overrides_the_spawn_roll(r: &Rig) {
    let ground = r.surface as i32 - 1;
    for (left, x) in [(true, 12), (false, 14)] {
        let data = rustmc_server::item::EntityData {
            entity: "minecraft:pig".into(),
            silent: Some(true),
            left_handed: Some(left),
        };
        let id = mob::use_spawn_egg(
            r.tiles,
            "minecraft:pig_spawn_egg",
            Some(&data),
            rustmc_data::BlockPos { x, y: ground, z: 6 },
            true,
            r.entities,
            r.tracker,
            &r.players,
            r.kinds,
        )
        .expect("a pig egg spawns")
        .expect("an egg");
        let flags = mob::Mobs::shared()
            .with(id, |s| s.ai.mob.data.get_byte(mob::data::id::MOB_FLAGS))
            .expect("the pig is live");
        assert_eq!(
            flags & mob::host::MOB_FLAG_LEFT_HANDED != 0,
            left,
            "LeftHanded:{left} on the egg decides the mob flag (flags {flags:#04x})"
        );
    }
}

/// `EntityType.updateCustomEntityTag` → `Entity.load`'s `Silent`: the egg's
/// `minecraft:entity_data` makes the pig silent, `DATA_SILENT` (accessor 4)
/// leads its first `set_entity_data` as vanilla's index-ordered
/// `getNonDefaultValues` would put it, and `Entity.playSound`'s `isSilent`
/// gate then refuses the spawn sound and every ambient roll after it — four
/// hundred ticks of them, where an unsilenced pig's per-tick chance reaches
/// certainty long before. Data for another type is not applied.
fn an_egg_carrying_silent_entity_data_spawns_a_pig_that_never_sounds(r: &Rig) {
    let ground = r.surface as i32 - 1;
    let silent = rustmc_server::item::EntityData {
        entity: "minecraft:pig".into(),
        silent: Some(true),
        left_handed: None,
    };
    r.recorder.clear();
    let id = mob::use_spawn_egg(
        r.tiles,
        "minecraft:pig_spawn_egg",
        Some(&silent),
        rustmc_data::BlockPos {
            x: 6,
            y: ground,
            z: 2,
        },
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a pig egg spawns")
    .expect("an egg");
    assert_eq!(
        mob::Mobs::shared().with(id, |s| s.ai.mob.data.get_bool(mob::data::id::SILENT)),
        Some(true)
    );
    let data = r
        .recorder
        .first(cb::SET_ENTITY_DATA)
        .expect("set_entity_data in the pairing");
    let (_, n) = read_varint(&data);
    assert_eq!(data[n], 4, "DATA_SILENT leads the non-default values");
    // Every OTHER mob in the rig is silenced through the same `DATA_SILENT`
    // the egg sets, so any sound at all in these 400 ticks is this pig's.
    // The check used to tell the pigs apart by where a sound landed, on the
    // premise that the rig's first pig walks along z = 0.5; it wanders on the
    // level random (`RandomSource.create()`, unseeded), and crossed the
    // window x 5..8, z 1.5..3.5 in 4 of 20 runs of this binary (land-batch-1
    // on main 4032, 2026-10-01: `entity.pig.step` at (5.75, -60, 2.875)).
    // Silencing them through the gate under test keeps the control: with the
    // egg's apply removed this pig sounds, and with `play_sound`'s gate
    // removed every pig does.
    for other in mob::Mobs::shared().ids() {
        if other != id {
            mob::Mobs::shared().with_mut(other, |s| {
                s.ai.mob.data.set_bool(mob::data::id::SILENT, true);
            });
        }
    }
    r.recorder.clear();
    for _ in 0..400 {
        mob::tick_all(1, 1, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    }
    let sounds = r
        .recorder
        .frames()
        .into_iter()
        .filter(|(i, _)| *i == cb::SOUND)
        .count();
    assert_eq!(sounds, 0, "a silent pig never sounds");

    let for_a_bat = rustmc_server::item::EntityData {
        entity: "minecraft:bat".into(),
        silent: Some(true),
        left_handed: None,
    };
    let other = mob::use_spawn_egg(
        r.tiles,
        "minecraft:pig_spawn_egg",
        Some(&for_a_bat),
        rustmc_data::BlockPos {
            x: 8,
            y: ground,
            z: 2,
        },
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a pig egg spawns")
    .expect("an egg");
    assert_eq!(
        mob::Mobs::shared().with(other, |s| s.ai.mob.data.get_bool(mob::data::id::SILENT)),
        Some(false),
        "updateCustomEntityTag returns at 24-27 when the data is for another type"
    );
}

fn a_pig_spawns_ticks_stands_and_syncs_its_speed_on_its_next_gated_tick(r: &Rig) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a flat surface is an integer"
    )]
    let ground = r.surface as i32 - 1;
    let clicked = rustmc_data::BlockPos {
        x: 3,
        y: ground,
        z: 0,
    };
    let id = mob::use_spawn_egg(
        r.tiles,
        "minecraft:pig_spawn_egg",
        None,
        clicked,
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a pig egg spawns")
    .expect("it is an egg");

    // The pairing bundle: add_entity, then set_entity_data with health 10
    // at accessor 9 — and NO update_attributes (no syncable instance yet).
    let add = r.recorder.first(cb::ADD_ENTITY).expect("add_entity");
    let (eid, _) = read_varint(&add);
    assert_eq!(eid, id);
    let data = r
        .recorder
        .first(cb::SET_ENTITY_DATA)
        .expect("set_entity_data in the pairing");
    let (_, n) = read_varint(&data);
    assert_eq!(data[n], 9, "health is accessor 9");
    assert_eq!(data[n + 1], 3, "FLOAT serializer");
    assert_eq!(
        &data[n + 2..n + 6],
        &10.0_f32.to_be_bytes(),
        "a pig's max health"
    );
    assert_eq!(
        r.recorder.count(cb::UPDATE_ATTRIBUTES),
        0,
        "no attribute in the pairing"
    );
    // The spawn: pos.5, on the block above the clicked one, pitch 0.
    let pig = r.entities.get(id).expect("in the entity list");
    assert_eq!((pig.x, pig.z), (3.5, 0.5));
    assert_eq!(pig.y, f64::from(ground + 1));
    assert_eq!(pig.x_rot, 0.0);
    assert_eq!(
        pig.y_head_rot, pig.y_rot,
        "yHeadRot = yBodyRot = yRot at spawn"
    );

    // Tick 1 in the level order: sendChanges (nothing dirty yet — the
    // pairing already sent the non-default values; the dirty flush
    // re-sends them), then the entity tick.
    r.recorder.clear();
    r.tracker.tick(r.entities, &r.players, r.kinds);
    assert_eq!(
        r.recorder.count(cb::SET_ENTITY_DATA),
        1,
        "packDirty re-sends the values set in the constructor"
    );
    assert_eq!(r.recorder.count(cb::UPDATE_ATTRIBUTES), 0);
    mob::tick_all(1, 1, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    // The MOVEMENT_SPEED instance removeFrost created goes out at
    // `sendDirtyEntityData` (offset 1093), which is INSIDE the
    // `tickCount % updateInterval == 0 || needsSync || entityData.isDirty()`
    // body — and an attribute is not `entityData`, so it waits for the pig's
    // next gated tick: `ServerEntity.tickCount` 3 against `updateInterval` 3,
    // the third sendChanges after the pairing tick (train-e's
    // `401d13445`, read off `javap` and vanilla's recording of `777`). Ticks
    // 2 and 3 send nothing for it. Until train-g this asserted tick 2, which
    // was main's ungated send and went red the day the gate was ported.
    for t in 2..4 {
        r.recorder.clear();
        r.tracker.tick(r.entities, &r.players, r.kinds);
        assert_eq!(
            r.recorder.count(cb::UPDATE_ATTRIBUTES),
            0,
            "tick {t} is off the interval and nothing forces the body"
        );
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    }
    r.recorder.clear();
    r.tracker.tick(r.entities, &r.players, r.kinds);
    let attrs = r
        .recorder
        .first(cb::UPDATE_ATTRIBUTES)
        .expect("update_attributes on the next gated tick");
    let (eid, n) = read_varint(&attrs);
    assert_eq!(eid, id);
    let mut want = vec![1, 26];
    want.extend_from_slice(&0.25_f64.to_be_bytes());
    want.push(0);
    assert_eq!(
        &attrs[n..],
        &want[..],
        "one attribute: movement_speed 0.25, no modifiers"
    );
    assert_eq!(
        r.recorder.count(cb::SET_ENTITY_DATA),
        0,
        "nothing else changed"
    );

    // A standing pig: on the ground at the surface, with the resting
    // downward velocity `(0 - 0.08) * 0.98f` travelInAir leaves after the
    // collision zeroed the fall.
    for t in 4..40 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
        r.tracker.tick(r.entities, &r.players, r.kinds);
    }
    let pig = r.entities.get(id).expect("still there");
    assert!(pig.on_ground, "on the ground");
    assert_eq!(pig.y, f64::from(ground + 1), "standing on the surface");
    let resting_dy = (0.0 - 0.08) * f64::from(0.98_f32);
    assert_eq!(
        pig.movement.1, resting_dy,
        "the gravity-then-drag resting velocity"
    );
    assert!(mob::Mobs::shared().with(id, |_| ()).is_some());
}

fn a_bat_past_128_blocks_from_every_player_is_discarded_and_a_pig_is_not(r: &Rig) {
    r.recorder.clear();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a flat surface is an integer"
    )]
    let ground = r.surface as i32 - 1;
    let far = rustmc_data::BlockPos {
        x: 0,
        y: ground,
        z: 150,
    };
    let bat = mob::use_spawn_egg(
        r.tiles,
        "minecraft:bat_spawn_egg",
        None,
        far,
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a bat egg spawns")
    .expect("an egg");
    let pig = mob::use_spawn_egg(
        r.tiles,
        "minecraft:pig_spawn_egg",
        None,
        rustmc_data::BlockPos { x: 2, ..far },
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a pig egg spawns")
    .expect("an egg");
    assert!(r.entities.get(bat).is_some() && r.entities.get(pig).is_some());
    // Both are 150 blocks from the only player: the despawn distance is
    // 128 for AMBIENT and CREATURE alike, and only the bat's
    // removeWhenFarAway says yes. checkDespawn runs before the ticking
    // range gate, so the bat's chunk being out of simulation range does
    // not save it.
    mob::tick_all(1, 1, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    assert!(
        r.entities.get(bat).is_none(),
        "the bat is discarded on its first tick"
    );
    assert!(mob::Mobs::shared().with(bat, |_| ()).is_none());
    assert!(
        r.entities.get(pig).is_some(),
        "Animal.removeWhenFarAway is false"
    );
    // Nobody tracked either (150 > 64 blocks), so no remove_entities went out.
    assert_eq!(r.recorder.count(cb::REMOVE_ENTITIES), 0);
}

fn a_summoned_pig_keeps_its_constructor_yaw_and_plays_no_ambient_sound(r: &Rig) {
    r.recorder.clear();
    let id = mob::summon(
        r.tiles,
        mob::kind_by_name("minecraft:pig").expect("pig"),
        -3.5,
        r.surface,
        0.5,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("summon");
    let pig = r.entities.get(id).expect("summoned");
    // `LivingEntity.<init>`: `nextFloat() * 6.2831855f` — radians used as
    // degrees, never wrapped, so it lies in [0, 2π).
    assert!(pig.y_rot >= 0.0 && pig.y_rot < 6.283_186, "{}", pig.y_rot);
    assert_eq!(pig.y_head_rot, pig.y_rot);
    assert_eq!((pig.x, pig.y, pig.z), (-3.5, r.surface, 0.5));
    assert_eq!(r.recorder.count(cb::ADD_ENTITY), 1);
    assert_eq!(
        r.recorder.count(cb::SOUND),
        0,
        "addFreshEntity plays nothing"
    );
}

/// `Animal.mobInteract` → `setInLove` → `BreedGoal` → `spawnChildFromBreeding`,
/// and `Mob.checkAndHandleImportantInteractions`' spawn egg and
/// `AgeableMob.mobInteract`'s golden dandelion, through the real store and
/// the real tracker.
fn fed_pigs_fall_in_love_breed_and_a_spawn_egg_hatches_a_baby(r: &Rig) {
    use mob::interact::{interact, Interactor, Outcome, Swing};
    use rustmc_entity::host::MobHost;
    r.recorder.clear();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a flat surface is an integer"
    )]
    let ground = r.surface as i32 - 1;
    let at = rustmc_data::BlockPos {
        x: 8,
        y: ground,
        z: 8,
    };
    let egg = |r: &Rig| {
        mob::use_spawn_egg(
            r.tiles,
            "minecraft:pig_spawn_egg",
            None,
            at,
            true,
            r.entities,
            r.tracker,
            &r.players,
            r.kinds,
        )
        .expect("a pig egg spawns")
        .expect("an egg")
    };
    let a = egg(r);
    let b = egg(r);
    let player = r.players[0].entity_id;
    let carrot = Interactor {
        entity_id: player,
        item: Some("minecraft:carrot"),
        secondary: false,
        creative: true,
    };
    let event_18 = |r: &Rig, id: i32| {
        let mut want = id.to_be_bytes().to_vec();
        want.push(18);
        r.recorder
            .frames()
            .iter()
            .filter(|(i, b)| *i == cb::ENTITY_EVENT && *b == want)
            .count()
    };
    // Nothing to eat with: PASS.
    let bare = Interactor {
        item: None,
        ..carrot
    };
    assert_eq!(
        interact(a, &bare, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );
    // A carrot: in love, the hearts, SUCCESS_SERVER.
    let first = interact(a, &carrot, &r.players).expect("a mob");
    assert_eq!(first.outcome, Outcome::Success(Swing::Server));
    assert_eq!(first.used, 1, "usePlayerItem");
    assert_eq!(event_18(r, a), 1, "setInLove broadcasts event 18");
    // Again while in love: PASS, no second event.
    assert_eq!(
        interact(a, &carrot, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );
    assert_eq!(event_18(r, a), 1);
    assert_eq!(
        interact(b, &carrot, &r.players).expect("a mob").outcome,
        Outcome::Success(Swing::Server)
    );
    // Thirty goal ticks of BreedGoal, at most sixty-two server ticks.
    let pigs_before = mob::Mobs::shared().len();
    let orbs_before = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::EXPERIENCE_ORB)
        .count();
    for t in 100..180 {
        r.tracker.tick(r.entities, &r.players, r.kinds);
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    }
    assert_eq!(
        mob::Mobs::shared().len(),
        pigs_before + 1,
        "one baby from one breeding"
    );
    let baby = *mob::Mobs::shared().ids().last().expect("the baby");
    let age = mob::Mobs::shared()
        .with(baby, |s| s.ai.mob.get_age())
        .expect("in the store");
    assert!(
        age < 0 && age > -24_000,
        "a baby that has aged since: {age}"
    );
    for parent in [a, b] {
        let (age, love) = mob::Mobs::shared()
            .with(parent, |s| (s.ai.mob.get_age(), s.ai.mob.in_love))
            .expect("in the store");
        assert!(age > 0 && age <= 6000, "a parent's cooldown: {age}");
        assert_eq!(love, 0, "resetLove");
    }
    let orbs_after = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::EXPERIENCE_ORB)
        .count();
    assert_eq!(orbs_after, orbs_before + 1, "mob_drops is on: one orb");
    // A parent on its 6000-tick cooldown will not eat.
    assert_eq!(
        interact(a, &carrot, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );

    // A pig spawn egg on a pig: a baby at the pig's position, SUCCESS_SERVER.
    let egg_in_hand = Interactor {
        item: Some("minecraft:pig_spawn_egg"),
        ..carrot
    };
    let before = mob::Mobs::shared().len();
    assert_eq!(
        interact(a, &egg_in_hand, &r.players)
            .expect("a mob")
            .outcome,
        Outcome::Success(Swing::Server)
    );
    assert_eq!(mob::Mobs::shared().len(), before + 1);
    let hatched = *mob::Mobs::shared().ids().last().expect("hatched");
    let (pos, age) = mob::Mobs::shared()
        .with(hatched, |s| {
            (
                (s.ai.mob.x(), s.ai.mob.y(), s.ai.mob.z()),
                s.ai.mob.get_age(),
            )
        })
        .expect("in the store");
    let parent = mob::Mobs::shared()
        .with(a, |s| (s.ai.mob.x(), s.ai.mob.y(), s.ai.mob.z()))
        .expect("in the store");
    assert_eq!(pos, parent, "snapTo(mob.position())");
    assert_eq!(age, -24_000);
    // A bat egg on a pig: not its type, PASS.
    let bat_egg = Interactor {
        item: Some("minecraft:bat_spawn_egg"),
        ..carrot
    };
    assert_eq!(
        interact(a, &bat_egg, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );

    // A carrot on the baby: it ages up by a tenth of its whole seconds, a
    // client swing.
    let fed = interact(hatched, &carrot, &r.players).expect("a mob");
    assert_eq!(fed.outcome, Outcome::Success(Swing::Client));
    let age = mob::Mobs::shared()
        .with(hatched, |s| s.ai.mob.get_age())
        .expect("in the store");
    assert_eq!(age, -24_000 + 120 * 20, "ageUp(120, true)");

    // A golden dandelion on the baby locks its age (accessor 17) and resets
    // it; again inside the 40-tick cooldown does nothing; after it, unlocks.
    let dandelion = Interactor {
        item: Some("minecraft:golden_dandelion"),
        ..carrot
    };
    let locked = |id| {
        mob::Mobs::shared()
            .with(id, |s| s.ai.mob.data.get_bool(mob::data::id::AGE_LOCKED))
            .expect("in the store")
    };
    assert_eq!(
        interact(hatched, &dandelion, &r.players)
            .expect("a mob")
            .outcome,
        Outcome::Success(Swing::Client)
    );
    assert!(locked(hatched));
    assert_eq!(
        mob::Mobs::shared()
            .with(hatched, |s| s.ai.mob.get_age())
            .expect("in the store"),
        -24_000
    );
    assert_eq!(
        interact(hatched, &dandelion, &r.players)
            .expect("a mob")
            .outcome,
        Outcome::Pass,
        "the cooldown"
    );
    for t in 200..240 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    }
    assert_eq!(
        interact(hatched, &dandelion, &r.players)
            .expect("a mob")
            .outcome,
        Outcome::Success(Swing::Client)
    );
    assert!(!locked(hatched), "the second dandelion unlocks");
    // On an adult: PASS.
    assert_eq!(
        interact(b, &dandelion, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );

    // A lead: tied (SUCCESS_SERVER, the link to the player), untied by a
    // creative player (SUCCESS, the link to nobody), tied again and snapped
    // once the holder is past twelve blocks (a lead item falls, the link is
    // cut).
    let lead = Interactor {
        item: Some("minecraft:lead"),
        ..carrot
    };
    let link = |r: &Rig, holder: i32| {
        let mut want = b.to_be_bytes().to_vec();
        want.extend_from_slice(&holder.to_be_bytes());
        r.recorder
            .frames()
            .iter()
            .filter(|(i, body)| *i == cb::SET_ENTITY_LINK && *body == want)
            .count()
    };
    // The pig has strolled for eighty-odd ticks on its own unseeded random
    // (`RandomStrollGoal`) and may stand past the lead's twelve blocks from
    // the player — measured 14.7 on 2026-10-01 — so put it back in reach
    // before asking anything of the lead.
    mob::Mobs::shared().with_mut(b, |s| {
        let y = s.ai.mob.y();
        s.ai.mob.snap_to(3.5, y, 3.5, 0.0, 0.0);
    });
    r.recorder.clear();
    assert_eq!(
        interact(b, &lead, &r.players).expect("a mob").outcome,
        Outcome::Success(Swing::Server)
    );
    assert_eq!(link(r, player), 1, "set_entity_link(pig, player)");
    assert_eq!(
        interact(b, &lead, &r.players).expect("a mob").outcome,
        Outcome::Success(Swing::Client),
        "the holder unties"
    );
    assert_eq!(link(r, 0), 1, "set_entity_link(pig, null)");
    assert!(mob::Mobs::shared()
        .with(b, |s| s.ai.mob.leash.is_none())
        .expect("in the store"));
    assert_eq!(
        interact(b, &lead, &r.players).expect("a mob").outcome,
        Outcome::Success(Swing::Server)
    );
    let items = |r: &Rig| {
        r.entities
            .all()
            .iter()
            .filter(|e| e.kind == rustmc_server::entities::ITEM)
            .count()
    };
    let items_before = items(r);
    let home = r.entities.get(player).expect("the player");
    r.entities.update(player, |e| e.x += 20.0);
    mob::tick_all(300, 300, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    r.entities.update(player, |e| e.x = home.x);
    assert!(
        mob::Mobs::shared()
            .with(b, |s| s.ai.mob.leash.is_none())
            .expect("in the store"),
        "snapped past 12 blocks"
    );
    assert_eq!(link(r, 0), 2, "the snap cuts the link");
    assert_eq!(items(r), items_before + 1, "dropLeash spawns the lead");
}

fn passive_egg(r: &Rig, item: &str, x: i32, z: i32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a flat surface is an integer"
    )]
    let ground = r.surface as i32 - 1;
    mob::use_spawn_egg(
        r.tiles,
        item,
        None,
        rustmc_data::BlockPos { x, y: ground, z },
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("an egg of a kind this server has")
    .expect("an egg")
}

fn creative_with<'a>(r: &Rig, item: Option<&'a str>) -> mob::interact::Interactor<'a> {
    mob::interact::Interactor {
        entity_id: r.players[0].entity_id,
        item,
        secondary: false,
        creative: true,
    }
}

/// `Cow`/`Chicken.defineSynchedData` end at accessor 19, `Sheep`'s and
/// `MushroomCow`'s at 18; `finalizeSpawn` moved what it draws.
fn the_four_passive_kinds_spawn_with_their_own_data(r: &Rig) {
    for (item, kind) in [
        ("minecraft:cow_spawn_egg", "minecraft:cow"),
        ("minecraft:mooshroom_spawn_egg", "minecraft:mooshroom"),
        ("minecraft:sheep_spawn_egg", "minecraft:sheep"),
        ("minecraft:chicken_spawn_egg", "minecraft:chicken"),
    ] {
        let id = passive_egg(r, item, 20, 20);
        assert_eq!(mob::mob_kind_name(id), Some(kind));
        let (colour, health) = mob::Mobs::shared()
            .with(id, |s| {
                let m = &s.ai.mob;
                let colour = (kind == "minecraft:sheep").then(|| mob::sheep::color(m));
                (colour, m.data.get_f32(mob::data::id::HEALTH))
            })
            .expect("in the store");
        // `createAttributes`: cow and mooshroom 10, sheep 8, chicken 4.
        let want = match kind {
            "minecraft:sheep" => 8.0,
            "minecraft:chicken" => 4.0,
            _ => 10.0,
        };
        assert_eq!(health, want, "{kind}'s MAX_HEALTH");
        if let Some(c) = colour {
            // Plains is temperate: black, gray, light gray, brown, white, pink.
            assert!([15, 7, 8, 12, 0, 6].contains(&c), "{c}");
        }
        let chicken = mob::Mobs::shared()
            .with(id, |s| s.ai.mob.chicken.map(|c| c.egg_time))
            .expect("in the store");
        if kind == "minecraft:chicken" {
            let t = chicken.expect("a chicken's own fields");
            assert!(
                (6000..12000).contains(&t),
                "eggTime = nextInt(6000) + 6000: {t}"
            );
        } else {
            assert!(chicken.is_none());
        }
    }
}

fn a_bucket_milks_a_cow_and_a_bowl_a_mooshroom(r: &Rig) {
    use mob::interact::{interact, Outcome, Swing};
    let cow = passive_egg(r, "minecraft:cow_spawn_egg", 12, 4);
    let a = interact(cow, &creative_with(r, Some("minecraft:bucket")), &r.players).expect("a mob");
    assert_eq!(a.outcome, Outcome::Success(Swing::Client));
    assert_eq!(
        a.fill,
        Some(mob::cow::Fill {
            result: "minecraft:milk_bucket",
            limit_creative: true
        })
    );
    // Wheat is cow food: in love, not milk.
    let w = interact(cow, &creative_with(r, Some("minecraft:wheat")), &r.players).expect("a mob");
    assert_eq!(w.outcome, Outcome::Success(Swing::Server));
    assert_eq!(w.fill, None);
    let moo = passive_egg(r, "minecraft:mooshroom_spawn_egg", 12, 8);
    r.recorder.clear();
    let b = interact(moo, &creative_with(r, Some("minecraft:bowl")), &r.players).expect("a mob");
    assert_eq!(b.outcome, Outcome::Success(Swing::Client));
    assert_eq!(
        b.fill,
        Some(mob::cow::Fill {
            result: "minecraft:mushroom_stew",
            limit_creative: false
        })
    );
    assert_eq!(
        r.recorder.count(cb::SOUND),
        1,
        "Entity.playSound(MOOSHROOM_MILK) reaches the player too"
    );
    // A bucket on a mooshroom is AbstractCow's milk.
    let c = interact(moo, &creative_with(r, Some("minecraft:bucket")), &r.players).expect("a mob");
    assert_eq!(c.fill.map(|f| f.result), Some("minecraft:milk_bucket"));
}

fn shears_turn_a_mooshroom_into_a_cow_and_five_mushrooms(r: &Rig) {
    use mob::interact::{interact, Outcome, Swing};
    use rustmc_entity::host::MobHost;
    let moo = passive_egg(r, "minecraft:mooshroom_spawn_egg", 4, 12);
    let (x, y, z, yaw) = mob::Mobs::shared()
        .with(moo, |s| {
            (s.ai.mob.x(), s.ai.mob.y(), s.ai.mob.z(), s.ai.mob.y_rot)
        })
        .expect("in the store");
    let items_before = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .count();
    r.recorder.clear();
    let a = interact(moo, &creative_with(r, Some("minecraft:shears")), &r.players).expect("a mob");
    assert_eq!(a.outcome, Outcome::Success(Swing::Client));
    assert!(!mob::is_mob(moo), "discarded");
    assert!(r.entities.get(moo).is_none());
    let cow = *mob::Mobs::shared().ids().last().expect("the cow");
    assert_eq!(mob::mob_kind_name(cow), Some("minecraft:cow"));
    let (cx, cy, cz, cyaw) = mob::Mobs::shared()
        .with(cow, |s| {
            (s.ai.mob.x(), s.ai.mob.y(), s.ai.mob.z(), s.ai.mob.y_rot)
        })
        .expect("in the store");
    assert_eq!((cx, cy, cz, cyaw), (x, y, z, yaw), "copyPosition");
    let items: Vec<_> = r
        .entities
        .all()
        .into_iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .collect();
    assert_eq!(items.len(), items_before + 5, "rolls: 5");
    let mushrooms: Vec<_> = items.iter().filter(|e| e.id > cow).collect();
    assert_eq!(mushrooms.len(), 5, "made after the cow's id was drawn");
    assert!(mushrooms
        .iter()
        .all(|e| e.x == x && e.z == z && (e.y - (y + 1.4)).abs() < 1e-6));
    assert_eq!(
        r.recorder.count(cb::SOUND_ENTITY),
        1,
        "MOOSHROOM_SHEAR on the entity"
    );
    assert_eq!(r.recorder.count(cb::REMOVE_ENTITIES), 1, "discard, at once");
    // The order on the wire: the shear sound, the items, the cow, the removal.
    let ids: Vec<i32> = r.recorder.frames().iter().map(|(i, _)| *i).collect();
    let snd = ids
        .iter()
        .position(|i| *i == cb::SOUND_ENTITY)
        .expect("sound");
    let rem = ids
        .iter()
        .position(|i| *i == cb::REMOVE_ENTITIES)
        .expect("remove");
    assert!(snd < rem);
}

fn a_sheep_is_dyed_sheared_once_and_regrows_by_eating(r: &Rig) {
    use mob::interact::{interact, Outcome, Swing};
    use rustmc_entity::host::MobHost;
    let sheep = passive_egg(r, "minecraft:sheep_spawn_egg", 12, 0);
    let lime = creative_with(r, Some("minecraft:lime_dye"));
    let a = interact(sheep, &lime, &r.players).expect("a mob");
    assert_eq!(
        a.outcome,
        Outcome::Success(Swing::Client),
        "DyeItem: SUCCESS"
    );
    assert_eq!(
        a.used, 1,
        "stack.shrink(1), skipped by the caller in creative"
    );
    let wool = |r: &Rig| {
        let _ = r;
        mob::Mobs::shared()
            .with(sheep, |s| s.ai.mob.data.get_byte(mob::data::id::SHEEP_WOOL))
            .expect("in the store")
    };
    assert_eq!(wool(r), 5, "lime, not sheared");
    assert_eq!(
        interact(sheep, &lime, &r.players).expect("a mob").outcome,
        Outcome::Pass,
        "the same colour: PASS"
    );
    let shears = creative_with(r, Some("minecraft:shears"));
    let items_before = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .count();
    r.recorder.clear();
    let b = interact(sheep, &shears, &r.players).expect("a mob");
    assert_eq!(b.outcome, Outcome::Success(Swing::Server));
    assert_eq!(wool(r), 5 | 16, "sheared, still lime");
    let dropped = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .count()
        - items_before;
    // `shearing/sheep`'s one draw, on the level seed's stream for that id.
    let mut twin = rustmc_entity::trading::draw::sequence_source(
        rustmc_server::status::level_seed(),
        Some("minecraft:shearing/sheep"),
    );
    use rustmc_worldgen::rng::RandomSource;
    let want = usize::try_from(twin.next_i32_bound(3) + 1).expect("1..=3");
    assert_eq!(dropped, want, "uniform(1, 3) from the sequence");
    assert_eq!(
        r.recorder.count(cb::SOUND_ENTITY),
        1,
        "SHEEP_SHEAR on the entity"
    );
    // Sheared: CONSUME, nothing dropped, no sound.
    r.recorder.clear();
    let c = interact(sheep, &shears, &r.players).expect("a mob");
    assert_eq!(c.outcome, Outcome::Success(Swing::None));
    assert_eq!(r.recorder.count(cb::SOUND_ENTITY), 0);
    // A sheared sheep cannot be dyed.
    let red = creative_with(r, Some("minecraft:red_dye"));
    assert_eq!(
        interact(sheep, &red, &r.players).expect("a mob").outcome,
        Outcome::Pass
    );
    // `Sheep.ate`: the wool grows back.
    mob::Mobs::shared().with_mut(sheep, |s| s.ai.mob.ate());
    assert_eq!(wool(r), 5);
}

fn a_chicken_falls_at_six_tenths_and_lays_an_egg(r: &Rig) {
    use rustmc_entity::host::MobHost;
    let top = r.surface + 6.0;
    let chicken = mob::summon(
        r.tiles,
        &mob::chicken::CHICKEN,
        24.5,
        top,
        24.5,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a chicken");
    let y = |c: i32| {
        mob::Mobs::shared()
            .with(c, |s| (s.ai.mob.y(), s.ai.mob.body.movement.y))
            .expect("in the store")
    };
    // `travelInAir`: `(v - 0.08) * 0.98F`, then `Chicken.aiStep`'s `* 0.6`
    // while falling. The move uses the velocity the last tick left.
    let mut v = 0.0_f64;
    let mut want_y = top;
    for t in 0..6 {
        mob::tick_all(
            400 + t,
            400 + t,
            r.entities,
            r.tracker,
            &r.players,
            VIEW_DISTANCE,
        );
        want_y += v;
        // `travelInAir`: `0.98F`, a float widened to double.
        v = (v - 0.08) * f64::from(0.98_f32);
        if v < 0.0 {
            v *= 0.6;
        }
        let (got_y, got_v) = y(chicken);
        assert!(
            (got_y - want_y).abs() < 1e-9,
            "tick {t}: y {got_y} vs {want_y}"
        );
        assert!((got_v - v).abs() < 1e-9, "tick {t}: v {got_v} vs {v}");
    }
    // The egg: one tick left on the timer.
    mob::Mobs::shared().with_mut(chicken, |s| {
        if let Some(c) = s.ai.mob.chicken.as_mut() {
            c.egg_time = 1;
        }
    });
    let items_before = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .count();
    mob::tick_all(410, 410, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    let items_after = r
        .entities
        .all()
        .iter()
        .filter(|e| e.kind == rustmc_server::entities::ITEM)
        .count();
    assert_eq!(items_after, items_before + 1, "one egg");
    let t = mob::Mobs::shared()
        .with(chicken, |s| s.ai.mob.chicken.map(|c| c.egg_time))
        .flatten()
        .expect("a chicken");
    assert!((6000..12000).contains(&t), "re-drawn: {t}");
    let _ = mob::Mobs::shared().with(chicken, |s| s.ai.mob.is_baby());
}
