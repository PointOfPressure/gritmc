//! A live player's death and respawn: `LivingEntity.kill`, the parts of
//! `ServerPlayer.die` a harness player reaches, `LivingEntity.tickDeath`'s
//! removal, and `PlayerList.respawn`.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `KillCommand`, `LivingEntity.kill`, `ServerPlayer.die`,
//! `CombatTracker.getDeathMessage`, `DamageSource.getLocalizedDeathMessage`,
//! `LivingEntity.getKillCredit`, `LivingEntity.tickDeath`,
//! `ServerGamePacketListenerImpl.handleClientCommand`, `PlayerList.respawn`,
//! `PlayerList.sendLevelInfo`, `PlayerList.sendPlayerPermissionLevel`.
//!
//! # Why this is wired now
//!
//! `crate::death` and `crate::respawn` were ported as decision halves with
//! nothing live to call them, because no player on this server could reach
//! zero health. `crate::damagecmd` can reach it, and the corpus's restore for
//! a damage check is `kill <player>` followed by the client's respawn — so a
//! server whose death or respawn is wrong now records every later survival
//! check against a dead player. That dependency is deliberate: it is the only
//! exact way to put health back, and it makes a broken respawn loud.
//!
//! # What is refused, by name
//!
//! * `FALL_VARIANTS` and `INTENTIONAL_GAME_DESIGN` death messages (see
//!   [`crate::death::localized_death_message`]): printed and not sent;
//! * dropping a non-empty inventory or awarding experience on death —
//!   printed and not done (no harness player carries either);
//! * `ClientboundChangeDifficultyPacket` in the respawn run: this server has
//!   no id for it (`crate::respawn`'s notes), so the respawn sends none;
//! * `initInventoryMenu`'s `container_set_content`: the join sends none
//!   either, so the respawn does not start.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use rustmc_world::nbt::Nbt;

/// Parse one `kill` console line into its player names.
///
/// `KillCommand.register`: bare `kill` targets the source entity, which the
/// console does not have (`CommandSourceStack.getEntityOrException` throws
/// `permissions.requires.entity`); `kill <targets>` takes an `EntityArgument`
/// of any count. Only player names resolve here, the same narrowing
/// [`crate::damagecmd::parse`] makes.
///
/// # Errors
/// The message the console prints.
pub fn parse_kill(line: &str) -> Result<String, String> {
    let line = line.strip_prefix('/').unwrap_or(line);
    let mut it = line.split_whitespace();
    if it.next() != Some("kill") {
        return Err(format!("not a kill command: {line:?}"));
    }
    let Some(target) = it.next() else {
        return Err("An entity is required to run this command here".into());
    };
    if target.starts_with('@') || target.contains('-') {
        return Err(format!(
            "kill: {target:?} refused — this console resolves player names only"
        ));
    }
    if let Some(extra) = it.next() {
        return Err(format!("kill: trailing argument {extra:?}"));
    }
    Ok(target.to_string())
}

/// `LivingEntity.kill(ServerLevel)` offsets 3..11:
/// `hurtServer(damageSources().genericKill(), Float.MAX_VALUE)`.
///
/// `generic_kill` is in `bypasses_invulnerability`, so even a creative player
/// dies; `ServerPlayer` does not override `kill`.
///
/// # Errors
/// The message the console prints.
pub fn dispatch_kill(
    line: &str,
    players: &crate::players::Players,
    now: i64,
) -> Result<(), String> {
    let name = parse_kill(line)?;
    let conn = players
        .connections()
        .into_iter()
        .find(|c| c.name() == name)
        .ok_or_else(|| "No entity was found".to_string())?;
    crate::damagecmd::submit(
        conn.id(),
        crate::damagecmd::Pending {
            amount: f32::MAX,
            damage_type: GENERIC_KILL.to_string(),
            source: crate::damagecmd::Source::Plain,
            at_tick: now,
        },
    );
    Ok(())
}

/// `DamageTypes.GENERIC_KILL`.
pub const GENERIC_KILL: &str = "minecraft:generic_kill";

/// `ServerPlayer.die` offset 282 and `tickDeath` offset 41: the two
/// `broadcastEntityEvent` bytes.
pub const ENTITY_EVENT_DEATH: u8 = crate::death::ENTITY_EVENT_DEATH;
/// `EntityEvent.POOF` — `bipush 60` at `tickDeath` offset 41.
pub const ENTITY_EVENT_POOF: u8 = 60;
/// `ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START`, which
/// `sendLevelInfo` ends with.
pub const GAME_EVENT_LEVEL_CHUNKS_LOAD_START: u8 = 13;

/// `CombatTracker.getDeathMessage` for a player whose last recorded hit was
/// `ty`, as a component — or `None` for the two arms this server refuses.
///
/// `attacker` is `causingEntity ?? directEntity`'s display name when the
/// source had one; `kill_credit` is `getKillCredit()`'s (the
/// `lastHurtByPlayer` of the last 100 ticks) when it did not. The weapon arm
/// (`.item`) needs a named main-hand stack, which no entity this console can
/// name holds, so it is passed as `false` by the only caller and the argument
/// exists so that a future one is a change at the call site.
#[must_use]
pub fn death_message(
    victim: &str,
    ty: &crate::damage_types::DamageType,
    attacker: Option<&str>,
    weapon_has_custom_name: bool,
    kill_credit: Option<&str>,
) -> Option<Nbt> {
    // `getDeathMessage` offsets 75..167: the two special types are consulted
    // before the source is asked.
    if matches!(
        ty.death_message_type.as_str(),
        "fall_variants" | "intentional_game_design"
    ) {
        return None;
    }
    let msg = crate::death::localized_death_message(
        &ty.message_id,
        attacker.is_some(),
        weapon_has_custom_name,
        kill_credit.is_some(),
    );
    let mut args = vec![crate::commands::display_name_component(victim)];
    if let Some(a) = attacker.or(kill_credit) {
        if msg.args >= 2 {
            args.push(crate::commands::display_name_component(a));
        }
    }
    if args.len() != msg.args {
        return None;
    }
    Some(crate::death::death_message_component(&msg, args))
}

/// `CommonComponents.EMPTY` — `Component.empty()`, a literal with no text,
/// which the network form writes as a bare empty string.
#[must_use]
pub fn empty_component() -> Nbt {
    Nbt::String(String::new())
}

/// `ClientboundEntityEventPacket.write`: a big-endian `int` entity id — not a
/// varint — then the byte.
///
/// # Errors
/// Only from the in-memory writer.
pub fn entity_event_body(entity: i32, event: u8) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(5);
    rustmc_proto::wire::write_i32(&mut b, entity)?;
    rustmc_proto::wire::write_u8(&mut b, event)?;
    Ok(b)
}

/// The `CommonPlayerSpawnInfo` each connection was sent at login, keyed by
/// entity id, for the respawn that has to send it again.
///
/// `PlayerList.respawn` builds it from the new player with
/// `createCommonSpawnInfo`, which reads the same level and game mode the join
/// read; the one field that differs is `lastDeathLocation`, which
/// [`respawn_info`] fills in.
fn infos() -> &'static Mutex<BTreeMap<i32, crate::respawn::CommonSpawnInfo>> {
    static M: OnceLock<Mutex<BTreeMap<i32, crate::respawn::CommonSpawnInfo>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Remember the spawn info a join sent.
pub fn remember_join(join: &crate::play::JoinInfo) {
    let info = crate::respawn::CommonSpawnInfo {
        dimension_type: join.dimension_type,
        dimension: join.dimension.clone(),
        seed: join.seed,
        game_type: join.game_type,
        previous_game_type: join.previous_game_type,
        is_debug: join.is_debug,
        is_flat: join.is_flat,
        last_death_location: None,
        portal_cooldown: join.portal_cooldown,
        sea_level: join.sea_level,
    };
    infos()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(join.player_id, info);
}

/// The spawn info a respawn sends: the join's, with the death recorded.
///
/// `ServerPlayer.die` offsets 340..358 set `lastDeathLocation` to
/// `GlobalPos.of(level.dimension(), blockPosition())` and `restoreFrom`
/// copies it to the new player unconditionally.
#[must_use]
pub fn respawn_info(
    entity: i32,
    died_at: (i32, i32, i32),
) -> Option<crate::respawn::CommonSpawnInfo> {
    let mut info = infos()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&entity)
        .cloned()?;
    info.last_death_location = Some((info.dimension.clone(), died_at.0, died_at.1, died_at.2));
    Some(info)
}

/// `Mth.floor` of a position — `Entity.blockPosition()`.
#[must_use]
pub fn block_position(x: f64, y: f64, z: f64) -> (i32, i32, i32) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.floor's own (int) cast after Math.floor"
    )]
    let f = |v: f64| v.floor() as i32;
    (f(x), f(y), f(z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(message_id: &str, death: &str) -> crate::damage_types::DamageType {
        crate::damage_types::DamageType {
            message_id: message_id.into(),
            scaling: "never".into(),
            effects: "hurt".into(),
            death_message_type: death.into(),
            exhaustion: 0.0,
            tags: std::collections::BTreeSet::new(),
        }
    }

    #[test]
    fn kill_needs_a_target_from_the_console() {
        assert!(
            parse_kill("kill").is_err(),
            "the console has no entity of its own"
        );
        assert_eq!(parse_kill("kill oracle").as_deref(), Ok("oracle"));
        assert_eq!(parse_kill("/kill oracle").as_deref(), Ok("oracle"));
        assert!(parse_kill("kill @a").is_err());
    }

    #[test]
    fn the_death_message_ladder_decides_the_key_and_the_arguments() {
        let generic = ty("generic", "default");
        let m = death_message("oracle", &generic, None, false, None).expect("default arm");
        let s = format!("{m:?}");
        assert!(s.contains("death.attack.generic"), "{s}");
        assert!(!s.contains("death.attack.generic.player"), "{s}");
        // A kill credit with no entity in the source is the `.player` arm.
        let m = death_message("oracle", &generic, None, false, Some("oracle_eye")).expect("arm");
        let s = format!("{m:?}");
        assert!(
            s.contains("death.attack.generic.player") && s.contains("oracle_eye"),
            "{s}"
        );
        // An attacker in the source is two arguments and no suffix.
        let mob = ty("mob", "default");
        let m = death_message("oracle", &mob, Some("oracle_eye"), false, None).expect("arm");
        let s = format!("{m:?}");
        assert!(
            s.contains("death.attack.mob") && s.contains("oracle_eye"),
            "{s}"
        );
        // The two refused arms.
        assert!(death_message("oracle", &ty("fall", "fall_variants"), None, false, None).is_none());
        assert!(death_message(
            "oracle",
            &ty("badRespawnPoint", "intentional_game_design"),
            None,
            false,
            None
        )
        .is_none());
    }

    #[test]
    fn the_entity_event_id_is_an_int_and_not_a_varint() {
        assert_eq!(entity_event_body(300, 3).expect("buf"), [0, 0, 1, 44, 3]);
    }

    #[test]
    fn a_respawn_carries_where_the_player_died() {
        let join = crate::play::JoinInfo {
            player_id: 4242,
            ..crate::play::JoinInfo::default()
        };
        remember_join(&join);
        let info = respawn_info(4242, (1, -60, 2)).expect("remembered");
        assert_eq!(
            info.last_death_location,
            Some((join.dimension.clone(), 1, -60, 2))
        );
        assert!(respawn_info(4243, (0, 0, 0)).is_none());
    }
}
