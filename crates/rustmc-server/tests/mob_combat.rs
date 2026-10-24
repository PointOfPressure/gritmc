//! A pig hit, killed and cleaned up, against the bytes a fake client
//! receives.
//!
//! The sibling of `mob_in_world.rs` and built on the same rig: a flat tile,
//! one player, one recording connection, the real `Entities`, the real
//! `Tracker` and `mob::tick_all`. What it grades is the wire, not the
//! arithmetic — `crate::mob::combat`'s unit tests own the arithmetic, and a
//! test that only re-asserted those numbers here would be measuring itself.
//!
//! # The three claims, and what would make each go red
//!
//! * **A hit produces `damage_event`, a dirty health `set_entity_data` and a
//!   `set_entity_motion`.** Red if the damage type is written as a bare
//!   registry id (the codec's `+ 1`), if the health lands on the wrong
//!   accessor, or if `hurtMarked` is not read outside the interval gate.
//! * **A kill produces `entity_event 3`, item `add_entity`s and orb
//!   `add_entity`s, in that order.** Red if `dropAllDeathLoot` runs the orbs
//!   before the items, or if a refusal is turned into an empty drop.
//! * **The corpse leaves after twenty ticks, with `entity_event 60` before
//!   `remove_entities`.** Red on an off-by-one in `tickDeath`, and red if the
//!   dying mob keeps ticking its AI.
//!
//! Every table is a `data/vanilla` extract; absence panics
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
    /// The packet ids in the order they were written, which is the only thing
    /// that can grade an ordering claim.
    fn order(&self) -> Vec<i32> {
        self.frames().into_iter().map(|(i, _)| i).collect()
    }
}

struct Rig {
    tiles: &'static rustmc_server::tiles::Tiles,
    entities: &'static Entities,
    tracker: &'static Tracker,
    kinds: &'static rustmc_server::entities::KindIds,
    recorder: Recorder,
    players: Vec<TrackedPlayer>,
    attacker_id: i32,
    surface: f64,
}

fn rig() -> Rig {
    std::env::set_current_dir(repo("")).expect("the repo root");
    for (what, by) in [
        ("data/vanilla/path_facts.json", "ci/path-facts-golden.sh"),
        ("data/vanilla/mob_variants.json", "ci/dump-mob-variants.sh"),
        ("data/vanilla/attributes.json", "ci/attributes-golden.sh"),
        ("data/vanilla/protocol_ids.json", "ci/dump-protocol-ids.sh"),
        ("data/vanilla/registries.bin", "ci/dump-registries.sh"),
        ("data/vanilla/entity_types.json", "ci/dump-entity-types.sh"),
        ("data/vanilla/loot_tables.json", "ci/loot-golden.sh"),
        ("data/vanilla/item_tags.json", "ci/loot-golden.sh"),
        ("data/vanilla/block_loot_map.json", "ci/loot-golden.sh"),
        ("data/vanilla/recipes.json", "ci/dump-recipes.sh"),
    ] {
        assert!(
            rustmc_data::fixture::available(&repo(what), what, by),
            "fixture rule: absent means panic, not skip"
        );
    }
    rustmc_server::flat::select(std::path::Path::new("data/vanilla/flat_preset.json"))
        .expect("the flat preset");
    // The world seed the named random sequences hang off
    // (`crate::lootroll`'s store, 0 unless set — set here explicitly).
    rustmc_server::lootroll::set_world_seed(0);
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
        (0, 0),
    ));
    // The join's two chunk steps, as `mob_in_world.rs` does them: since
    // entity-tracker, `updatePlayer` asks `isChunkTracked`, so a view that has
    // been sent nothing tracks no pig and the hit's `damage_event` reaches
    // nobody.
    conn.with_chunk_view(|v| {
        for dx in -4..=4 {
            for dz in -4..=4 {
                v.mark_sent((dx, dz));
            }
        }
        v.update_tracking((0, 0));
    });
    entities.insert(Entity {
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
    });
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
        attacker_id: entity_id,
        surface,
    }
}

fn read_varint(b: &[u8]) -> (i32, usize) {
    let mut cur = b;
    let v = rustmc_proto::varint::read_varint(&mut cur).expect("varint");
    (v, b.len() - cur.len())
}

/// A swing with the damage the caller wants, everything else at the rested
/// bare-handed values `crate::attack`'s tests pin.
fn swing(damage: f64) -> rustmc_server::attack::Swing {
    rustmc_server::attack::Swing {
        attack_strength_ticker: 5,
        attack_speed: rustmc_server::attack::PLAYER_ATTACK_SPEED,
        attack_damage: damage,
        attack_knockback: rustmc_server::attack::PLAYER_ATTACK_KNOCKBACK,
        item_damage_bonus: 0.0,
        sprinting: false,
        crit: rustmc_server::attack::CritConditions {
            fall_distance: 0.0,
            on_ground: true,
            on_climbable: false,
            in_water: false,
            mobility_restricted: false,
            passenger: false,
            target_is_living: true,
            sprinting: false,
        },
        sweep: rustmc_server::attack::SweepConditions {
            strong: false,
            crit: false,
            knockback_sprint: false,
            on_ground: true,
            known_movement_horizontal_sqr: 0.0,
            speed: 0.0,
            holding_sword: false,
        },
    }
}

fn spawn_pig(r: &Rig, x: i32) -> i32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a flat surface is an integer"
    )]
    let ground = r.surface as i32 - 1;
    mob::use_spawn_egg(
        r.tiles,
        "minecraft:pig_spawn_egg",
        None,
        rustmc_data::BlockPos { x, y: ground, z: 0 },
        true,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    )
    .expect("a pig egg spawns")
    .expect("it is an egg")
}

/// One process, one shared entity list and mob store, so the parts run in
/// order inside one test rather than racing each other's ticks.
#[test]
fn a_pig_is_hurt_killed_and_cleaned_up() {
    let r = rig();
    let damage_type =
        mob::combat::damage_type_id(mob::combat::PLAYER_ATTACK).expect("the capture names it");
    one_hit_sends_a_damage_event_the_health_and_the_knockback(&r, damage_type);
    a_second_hit_inside_the_cooldown_sends_nothing(&r, damage_type);
    a_killing_blow_drops_porkchop_and_an_orb(&r, damage_type);
}

/// A pig hit once for a point of damage.
///
/// Four assertions, and each is a different mistake:
///
/// * the `damage_event` carries the damage type as `registry id + 1` and both
///   entity ids as `id + 1`. Writing either without its offset gives a packet
///   the client parses into a different entity or a different death message.
/// * the health reaches the client as accessor **9** with the FLOAT
///   serializer, on the tick after the hit, through the dirty flush.
/// * a `set_entity_motion` goes out because `hurtMarked` was set, and it goes
///   out on the FIRST tick after the hit rather than on the next multiple of
///   the pig's `updateInterval` of 3.
/// * the health is 9.0 and not 10.0, which is what makes the other three
///   about a hit rather than about a spawn.
fn one_hit_sends_a_damage_event_the_health_and_the_knockback(r: &Rig, damage_type: i32) {
    let id = spawn_pig(r, 3);
    // Let the spawn tick settle so nothing below is the pairing's.
    for t in 1..4 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
        r.tracker.tick(r.entities, &r.players, r.kinds);
    }
    r.recorder.clear();

    let out = mob::attack_mob(
        id,
        r.attacker_id,
        (0.5, r.surface, 0.5),
        &swing(1.0),
        damage_type,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    );
    let mob::MobAttack::Hurt { health, died, .. } = out else {
        panic!("a pig with no invulnerability must take the hit: {out:?}");
    };
    assert!(!died, "one point off ten is not a kill");
    assert!(
        (health - 9.0).abs() < 1e-6,
        "10 - 1 with armour 0 and no absorption: {health}"
    );

    let ev = r
        .recorder
        .first(cb::DAMAGE_EVENT)
        .expect("a damage_event on the hit tick");
    let mut cur = &ev[..];
    let entity = rustmc_proto::varint::read_varint(&mut cur).expect("entity");
    let source = rustmc_proto::varint::read_varint(&mut cur).expect("source");
    let cause = rustmc_proto::varint::read_varint(&mut cur).expect("cause");
    let direct = rustmc_proto::varint::read_varint(&mut cur).expect("direct");
    let has_pos = cur.first().copied();
    assert_eq!(entity, id);
    assert_eq!(
        source,
        damage_type + 1,
        "ByteBufCodecs.holderRegistry writes registry id + 1; 0 is the inline-holder marker"
    );
    assert_eq!(
        cause,
        r.attacker_id + 1,
        "writeOptionalEntityId writes id + 1; 0 is 'no entity'"
    );
    assert_eq!(direct, r.attacker_id + 1);
    assert_eq!(has_pos, Some(0), "no source position on a melee hit");

    // The pig's own hurt sound, at the pig, from inside hurtServer's arm.
    assert!(
        r.recorder.count(cb::SOUND) >= 1,
        "the pig's hurt sound and the attacker's strong sound"
    );

    // The dirty flush is on the NEXT sendChanges, and the pig's
    // updateInterval is 3 — so a health packet that only arrived on a
    // multiple of three would be wrong by up to two ticks.
    r.recorder.clear();
    r.tracker.tick(r.entities, &r.players, r.kinds);
    let data = r
        .recorder
        .first(cb::SET_ENTITY_DATA)
        .expect("the health goes out on the very next sendChanges");
    let (eid, n) = read_varint(&data);
    assert_eq!(eid, id);
    assert_eq!(data[n], 9, "health is accessor 9");
    assert_eq!(data[n + 1], 3, "FLOAT serializer");
    assert_eq!(&data[n + 2..n + 6], &9.0_f32.to_be_bytes());
    // TWO, and that is vanilla's count rather than a defect.
    //
    // `ServerEntity.sendChanges` builds a `ClientboundSetEntityMotionPacket`
    // in two independent places and a knocked-back mob satisfies both on the
    // same tick:
    //
    //   919   the velocity limb, entered on `needsSync || trackDelta`.
    //         `knockback` sets `needsSync` at its offset 21 AND a pig's
    //         `EntityType` is not in `trackDeltas`' false list, so either
    //         disjunct alone would open it; the `dist > 1.0E-7` gate is then
    //         satisfied because the knockback moved the velocity.
    //   1224  the `hurtMarked` limb, outside the interval gate entirely.
    //
    // So a client is told the same velocity twice. Asserting ONE here would
    // have been asserting that this server sends less than vanilla, and the
    // test was written that way first: the count is measured, not assumed.
    //
    // The count is two whatever tick this lands on, and that is worth saying
    // rather than leaving to a reader to work out: `needsSync` opens the
    // interval gate by itself, so the velocity limb runs even on a tick the
    // pig's `updateInterval` of 3 would have skipped, and the `hurtMarked`
    // limb is outside the gate entirely. There is no tick on which this is
    // one — which is why this assertion cannot be made to grade the gate
    // position on its own, and the `set_entity_data` assertion above is the
    // one that does (`sendDirtyEntityData` runs at offset 204, before the
    // gate, so the health arrives on the very next `sendChanges` and not on
    // the next multiple of three).
    assert_eq!(
        r.recorder.count(cb::SET_ENTITY_MOTION),
        2,
        "the velocity limb at 919 and the hurtMarked limb at 1224 both fire"
    );
}

/// The invulnerability cooldown, on the wire.
///
/// `hurtServer` offsets 184..240: inside the first ten of the twenty ticks,
/// a hit no larger than `lastHurt` returns false and takes the `fullHit`
/// arm with it — no `damage_event`, no knockback, no sound, and the health
/// does not move.
///
/// This is the branch main's `crate::attack` could not reach at all, because
/// every player on this server is creative and `Outcome::Hurt` was
/// unreachable. It is also the one that makes `PLAYER_ATTACK_NODAMAGE`
/// reachable for a real reason rather than as a constant.
///
/// The third hit is the positive control and it is a **health** control, not a
/// packet one: a larger hit inside the window lands for the difference and is
/// still silent, because the cooldown arm clears `fullHit` and `fullHit` gates
/// the broadcast, the knockback and every sound. Without it the pair above
/// would pass for an implementation that refuses every second hit.
fn a_second_hit_inside_the_cooldown_sends_nothing(r: &Rig, damage_type: i32) {
    let id = spawn_pig(r, 5);
    for t in 10..14 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
        r.tracker.tick(r.entities, &r.players, r.kinds);
    }

    let first = mob::attack_mob(
        id,
        r.attacker_id,
        (0.5, r.surface, 0.5),
        &swing(2.0),
        damage_type,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    );
    assert!(matches!(first, mob::MobAttack::Hurt { .. }));

    r.recorder.clear();
    let second = mob::attack_mob(
        id,
        r.attacker_id,
        (0.5, r.surface, 0.5),
        &swing(2.0),
        damage_type,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    );
    assert_eq!(
        second,
        mob::MobAttack::NoDamage,
        "an equal hit inside the cooldown returns false from hurtOrSimulate"
    );
    assert_eq!(
        r.recorder.count(cb::DAMAGE_EVENT),
        0,
        "fullHit is false, and it gates the broadcast"
    );

    // The positive control for the two above: a LARGER hit inside the same
    // cooldown DOES land, for the difference. Without this the pair would
    // pass for an implementation that refused every second hit.
    r.recorder.clear();
    let third = mob::attack_mob(
        id,
        r.attacker_id,
        (0.5, r.surface, 0.5),
        &swing(5.0),
        damage_type,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    );
    let mob::MobAttack::Hurt { health, .. } = third else {
        panic!("a larger hit inside the cooldown lands: {third:?}");
    };
    assert!(
        (health - 5.0).abs() < 1e-6,
        "10 - 2 - (5 - 2) = 5, the DIFFERENCE and not the whole five: {health}"
    );
    // **And it broadcasts NOTHING**, which is the reading this assertion had
    // backwards and `crate::hurt`'s own module doc had right all along: "a hit
    // that lands *inside* the cooldown therefore moves health and is otherwise
    // silent — no `damage_event` on the wire, no knockback, no red flash."
    //
    // `fullHit` is the local-8 flag, `true` by default and cleared by the
    // cooldown arm at `hurtServer` offset ~236, and it gates the
    // `broadcastDamageEvent`, `markHurt`, `dealDefaultKnockback` and every
    // sound. A LARGER hit inside the window takes the same arm as an equal
    // one — the difference is only whether `damage > lastHurt` lets it in at
    // all. So the observable difference between the two is the HEALTH and
    // nothing else, which is why the assertion above is the one that
    // separates them and this one is here to say the wire cannot.
    assert_eq!(
        r.recorder.count(cb::DAMAGE_EVENT),
        0,
        "the cooldown arm clears fullHit, and fullHit gates the broadcast"
    );
    assert_eq!(
        r.recorder.count(cb::SET_ENTITY_MOTION),
        0,
        "no knockback either: dealDefaultKnockback is under the same flag"
    );
}

/// The kill, the drops, the orbs and the twenty-tick corpse.
fn a_killing_blow_drops_porkchop_and_an_orb(r: &Rig, damage_type: i32) {
    let id = spawn_pig(r, 7);
    for t in 20..24 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
        r.tracker.tick(r.entities, &r.players, r.kinds);
    }
    let before_entities = r.entities.all().len();
    r.recorder.clear();

    let out = mob::attack_mob(
        id,
        r.attacker_id,
        (0.5, r.surface, 0.5),
        // Twenty damage against ten health: one blow, no cooldown to reason
        // about, and the death arm is the only thing under test.
        &swing(20.0),
        damage_type,
        r.entities,
        r.tracker,
        &r.players,
        r.kinds,
    );
    let mob::MobAttack::Hurt {
        health,
        died,
        drops,
        experience,
        ..
    } = out
    else {
        panic!("the killing blow lands: {out:?}");
    };
    assert!(died);
    assert_eq!(
        health, 0.0,
        "setHealth clamps at zero, it does not go under"
    );
    assert_eq!(
        drops, 1,
        "the pig's table is one pool of one entry: one porkchop stack"
    );
    assert!(
        (1..=3).contains(&experience),
        "Animal.getBaseExperienceReward is 1 + nextInt(3): {experience}"
    );

    // `entity_event 3` — a four-byte int id, not a varint.
    let ev = r
        .recorder
        .first(cb::ENTITY_EVENT)
        .expect("entity_event 3 on the death");
    assert_eq!(ev.len(), 5, "writeInt then writeByte");
    assert_eq!(
        i32::from_be_bytes(ev[0..4].try_into().expect("4 bytes")),
        id,
        "the entity id is a big-endian int; a varint would name a different entity"
    );
    assert_eq!(ev[4], 3);

    // The order: the death event, then the item, then the orb. Vanilla's
    // `dropAllDeathLoot` calls `dropFromLootTable` before `dropExperience`,
    // and `die` broadcasts event 3 before either.
    let order = r.recorder.order();
    let death_at = order
        .iter()
        .position(|i| *i == cb::ENTITY_EVENT)
        .expect("the death event is in the stream");
    let adds: Vec<usize> = order
        .iter()
        .enumerate()
        .filter(|(_, i)| **i == cb::ADD_ENTITY)
        .map(|(n, _)| n)
        .collect();
    assert!(
        adds.len() >= 2,
        "at least the porkchop and one orb: {} add_entity",
        adds.len()
    );
    assert!(
        adds.iter().all(|a| *a > death_at),
        "every drop is announced after entity_event 3"
    );

    // The kinds, read off the wire rather than off the entity list, because
    // the entity list is this server's own bookkeeping and the wire is what a
    // client sees.
    let orb_kind = r
        .kinds
        .get(rustmc_server::entities::EXPERIENCE_ORB)
        .expect("the orb type is in entity_types.json");
    let item_kind = r
        .kinds
        .get(rustmc_server::entities::ITEM)
        .expect("the item type is in entity_types.json");
    let mut saw_item = false;
    let mut saw_orb = false;
    let mut item_at = usize::MAX;
    for (n, (packet, body)) in r.recorder.frames().into_iter().enumerate() {
        if packet != cb::ADD_ENTITY {
            continue;
        }
        let mut cur = &body[..];
        let _id = rustmc_proto::varint::read_varint(&mut cur).expect("id");
        // The uuid is sixteen raw bytes, then the type is a varint.
        let (_, uuid) = cur.split_at(16);
        let mut cur = uuid;
        let kind = rustmc_proto::varint::read_varint(&mut cur).expect("type");
        if kind == item_kind {
            saw_item = true;
            item_at = item_at.min(n);
        }
        if kind == orb_kind {
            saw_orb = true;
            assert!(
                n > item_at,
                "dropFromLootTable runs before dropExperience: the item comes first"
            );
        }
    }
    assert!(saw_item, "a porkchop item entity reached the client");
    assert!(saw_orb, "an experience orb reached the client");

    // The corpse: nineteen ticks of nothing, then the poof and the removal.
    r.recorder.clear();
    for t in 30..49 {
        mob::tick_all(t, t, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
        r.tracker.tick(r.entities, &r.players, r.kinds);
    }
    assert_eq!(
        r.recorder.count(cb::REMOVE_ENTITIES),
        0,
        "nineteen ticks is not twenty"
    );
    assert!(
        r.entities.get(id).is_some(),
        "the corpse is still in the world at deathTime 19"
    );

    r.recorder.clear();
    mob::tick_all(49, 49, r.entities, r.tracker, &r.players, VIEW_DISTANCE);
    let order = r.recorder.order();
    let poof = order
        .iter()
        .position(|i| *i == cb::ENTITY_EVENT)
        .expect("entity_event 60 on the twentieth tick");
    let gone = order
        .iter()
        .position(|i| *i == cb::REMOVE_ENTITIES)
        .expect("remove_entities on the same tick");
    assert!(poof < gone, "the poof precedes the removal");
    let ev = r.recorder.first(cb::ENTITY_EVENT).expect("the poof");
    assert_eq!(ev[4], 60);
    assert!(r.entities.get(id).is_none(), "the corpse is gone");
    assert!(
        r.entities.all().len() > before_entities,
        "the drops outlive the mob"
    );
}
