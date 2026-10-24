//! What happens when a player dies: `ServerPlayer.die`, the packet it sends,
//! the message it broadcasts, the loot it drops and the experience it awards.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `ServerPlayer.die`, `LivingEntity.die`, `Player.die`,
//! `LivingEntity.dropAllDeathLoot`, `Player.dropEquipment`,
//! `Player.destroyVanishingCursedItems`, `Inventory.dropAll`,
//! `LivingEntity.createItemStackToDrop`, `Player.getBaseExperienceReward`,
//! `CombatTracker.getDeathMessage`, `DamageSource.getLocalizedDeathMessage`.
//!
//! # `ServerPlayer.die` does NOT call `super.die`, and that is load-bearing
//!
//! `Player.die` and `LivingEntity.die` both exist and both drop loot, so the
//! obvious reading is that `ServerPlayer.die` extends one of them. It does
//! not: offset 0 is `gameEvent(ENTITY_DIE)` and there is no `invokespecial`
//! to any superclass `die` anywhere in the method. A port that called a
//! ported `LivingEntity::die` first would drop the inventory **twice** — once
//! from `Player.dropEquipment` under the super call and once from
//! `ServerPlayer.die`'s own `dropAllDeathLoot` at offset 221 — and the
//! duplication is invisible to any check that only asserts "the items are on
//! the floor".
//!
//! # What this module is, and what it is not
//!
//! It is the **decision half** of the death path, written as functions over
//! plain values so that every branch the bytecode has is a branch a test can
//! take. It is not wired into a live death, and it cannot be on this tree:
//! see [`crate::respawn`]'s module header for the two independent reasons the
//! corpus cannot reach any of it, measured rather than assumed.

use std::io::{self, Write};

use rustmc_proto::{ids, varint::write_varint};
use rustmc_world::nbt::Nbt;
use rustmc_worldgen::math;
use rustmc_worldgen::rng::RandomSource;

/// The experience a dying player awards — `Player.getBaseExperienceReward`.
///
/// ```text
///   Player.getBaseExperienceReward(ServerLevel), javap -p -c -constants
///      0: level.getGameRules().get(KEEP_INVENTORY).booleanValue()
///     16: ifne 26
///     19: isSpectator()      ifeq 28
///     26: iconst_0  / 27: ireturn
///     28: getfield experienceLevel
///     32: bipush 7   / 34: imul
///     35: bipush 100 / 37: Math.min(II)I
///     40: ireturn
/// ```
///
/// So the cap is `min(level * 7, 100)` and **the multiply happens before the
/// clamp**, which matters at level 15: `15 * 7` is 105 and the answer is 100,
/// not 105. The clamp is on the product, never on the level.
///
/// `LivingEntity.getExperienceReward` then wraps this in
/// `EnchantmentHelper.processMobExperience`, which for a player killed by
/// anything without an experience-modifying enchantment is the identity. That
/// wrapper is `enchantments`' to port, not this module's, and this function
/// deliberately returns the **base** reward so the join is a named one rather
/// than a silent assumption.
#[must_use]
pub fn base_experience_reward(experience_level: i32, keep_inventory: bool, spectator: bool) -> i32 {
    if keep_inventory || spectator {
        return 0;
    }
    (experience_level.saturating_mul(7)).min(100)
}

/// The join packet's `showDeathScreen` field, from the `immediate_respawn`
/// game rule.
///
/// **It is the negation**, and it is the one field of
/// `ClientboundLoginPacket` where a rule and its wire field have opposite
/// senses:
///
/// ```text
///   PlayerList.placeNewPlayer, javap -p -c -constants
///     211: immediateRespawn = rules.get(GameRules.IMMEDIATE_RESPAWN)
///     ...
///     297: iload 13          (immediateRespawn)
///     299: ifne 306
///     302: iconst_1          -> showDeathScreen = true
///     303: goto 307
///     306: iconst_0          -> showDeathScreen = false
///     307: (next argument)
/// ```
///
/// This is the client-side half of the whole death path: with the field
/// clear, a dying client skips its death screen and sends `PERFORM_RESPAWN`
/// immediately, so the respawn packets follow the combat-kill packet with no
/// human in between. A server that hardcodes it `true` — as this one did
/// until 2026-09-06 — accepts `/gamerule immediate_respawn true` and then
/// behaves as though it had not.
#[must_use]
pub fn show_death_screen(immediate_respawn: bool) -> bool {
    !immediate_respawn
}

/// Whether a stack is destroyed rather than dropped —
/// `Player.destroyVanishingCursedItems`.
///
/// The whole method is a walk of `inventory.getContainerSize()` calling
/// `EnchantmentHelper.has(stack, PREVENT_EQUIPMENT_DROP)` and, on a hit,
/// `inventory.removeItemNoUpdate(i)`. The component is
/// `minecraft:prevent_equipment_drop` on the *enchantment*, not on the stack,
/// so the predicate is "any enchantment on this stack carries that effect
/// component" — which is why this takes the answer rather than computing it.
/// Deciding it needs the enchantment registry, and `enchantments` owns that.
///
/// It runs **before** [`Inventory::drop_all`] and only under
/// `Player.dropEquipment`'s `!keepInventory` arm, so a cursed item under
/// `keepInventory` survives. That ordering is the whole behaviour: reversed,
/// every cursed item would already be on the floor before anything looked at
/// it.
#[must_use]
pub fn destroyed_before_drop(has_prevent_equipment_drop: bool) -> bool {
    has_prevent_equipment_drop
}

/// The velocity a death-dropped stack leaves with —
/// `LivingEntity.createItemStackToDrop`'s `dropAround = true` branch.
///
/// # This is a different branch from the one `crate::dropitem` ports
///
/// `dropitem` ports the **aimed** branch (`dropAround = false`), which is what
/// pressing Q sends: four draws, the player's pitch and yaw, a forward push.
/// `Inventory.dropAll` calls `Player.drop(stack, true, false)` — `dropAround`
/// true, `includeThrowerName` false — and that is a completely different
/// arithmetic with a different draw count:
///
/// ```text
///   LivingEntity.createItemStackToDrop, offsets 60..122 (dropAround arm)
///     64: f  = random.nextFloat() * 0.5F
///     79: f1 = random.nextFloat() * 6.2831855F
///     93: itemEntity.setDeltaMovement(
///           (double)(-Mth.sin(f1) * f),
///           0.20000000298023224d,
///           (double)( Mth.cos(f1) * f))
/// ```
///
/// **Two draws, magnitude first and angle second.** Reversing them is a legal
/// program that produces the same distribution shape and a different stream
/// position for everything drawn afterwards, which is exactly the class of
/// defect a value-comparing test cannot see. The order is asserted below.
///
/// The `y` term is the `double` `0.20000000298023224`, which is the `float`
/// `0.2f` widened, and is **not** `0.2`. Same trap as
/// [`crate::dropitem::DROP_EYE_OFFSET`], written as the exact double for the
/// same reason.
///
/// `Mth.sin`/`Mth.cos` are table lookups in vanilla and not `f32::sin`; the
/// caller passes whichever it has, and `crate::dropitem` already carries that
/// distinction. The multiply `-sin(f1) * f` is done in `f32` and widened
/// once (`fmul` then `f2d`), so rounding twice through `f64` is wrong.
#[must_use]
pub fn death_drop_velocity(rng: &mut impl RandomSource) -> (f64, f64, f64) {
    let f = rng.next_f32() * 0.5;
    let f1 = rng.next_f32() * 6.283_185_5;
    (
        f64::from(-math::sin(f64::from(f1)) * f),
        0.200_000_002_980_232_24,
        f64::from(math::cos(f64::from(f1)) * f),
    )
}

/// The translation key a death message is built from, and how many arguments
/// it takes.
///
/// The three suffixes are string-concat bootstrap constants in
/// `DamageSource.getLocalizedDeathMessage`, read out of the class file's
/// `BootstrapMethods` table rather than guessed:
///
/// ```text
///   BootstrapMethods #1   death.attack.
///   BootstrapMethods #2   .item
///   BootstrapMethods #3   .player
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathMessage {
    /// The full translation key, e.g. `death.attack.player`.
    pub key: String,
    /// How many arguments `Component.translatable` is handed. Vanilla builds
    /// the array with `anewarray` of exactly this length, so the count is a
    /// property of the branch and not of the language file.
    pub args: usize,
}

/// `CombatTracker.getDeathMessage` -> `DamageSource.getLocalizedDeathMessage`,
/// for every arm that does not need a fall history or an entity's display
/// name.
///
/// # The ladder, from the bytecode
///
/// ```text
///   CombatTracker.getDeathMessage
///      0: entries.isEmpty()        -> "death.attack.generic", 1 arg
///     75: deathMessageType == FALL_VARIANTS && mostSignificantFall != null
///                                  -> getFallMessage(...)
///     97: deathMessageType == INTENTIONAL_GAME_DESIGN
///                                  -> bracketed wiki link, 2 args
///    167: source.getLocalizedDeathMessage(mob)
///
///   DamageSource.getLocalizedDeathMessage(LivingEntity victim)
///      0: key = "death.attack." + type().msgId()
///     13: causingEntity != null || directEntity != null ?
///           attacker = causingEntity ?? directEntity
///           weapon   = causingEntity instanceof LivingEntity
///                        ? mainHandItem : ItemStack.EMPTY
///     86: !weapon.isEmpty() && weapon.has(CUSTOM_NAME)
///                                  -> key + ".item",   3 args
///    138:                          -> key,             2 args
///    158: : killCredit = victim.getKillCredit()
///    171: killCredit != null       -> key + ".player", 2 args
///    199:                          -> key,             1 arg
/// ```
///
/// **The `.player` suffix is on the no-entity path, not the killed-by-a-player
/// path**, which reads backwards and is the reason this is transcribed rather
/// than remembered: a player who drowns after being shoved gets
/// `death.attack.drown.player`, and a player shot by another player gets the
/// bare `death.attack.arrow` with two arguments.
///
/// `FALL_VARIANTS` and `INTENTIONAL_GAME_DESIGN` are refused here rather than
/// approximated — the first needs a `CombatTracker` entry list this server has
/// no damage history to fill, and the second needs a styled bracketed link.
/// Both return `None`.
#[must_use]
pub fn localized_death_message(
    msg_id: &str,
    has_causing_or_direct_entity: bool,
    weapon_has_custom_name: bool,
    has_kill_credit: bool,
) -> DeathMessage {
    let key = format!("death.attack.{msg_id}");
    if has_causing_or_direct_entity {
        if weapon_has_custom_name {
            return DeathMessage {
                key: format!("{key}.item"),
                args: 3,
            };
        }
        return DeathMessage { key, args: 2 };
    }
    if has_kill_credit {
        return DeathMessage {
            key: format!("{key}.player"),
            args: 2,
        };
    }
    DeathMessage { key, args: 1 }
}

/// The message a player with no recorded damage at all dies to.
///
/// `CombatTracker.getDeathMessage` offset 12, before any source is consulted.
#[must_use]
pub fn generic_death_message() -> DeathMessage {
    DeathMessage {
        key: "death.attack.generic".into(),
        args: 1,
    }
}

/// Who is told, once `show_death_messages` is on — `ServerPlayer.die`
/// offsets 68..154.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAudience {
    /// `playerList.broadcastSystemMessage(msg, false)` — everyone. The
    /// `false` is `overlay`, so it is a chat line and not an action bar.
    Everyone,
    /// `playerList.broadcastSystemToTeam(player, msg)`.
    OwnTeamOnly,
    /// `playerList.broadcastSystemToAllExceptTeam(player, msg)`.
    EveryoneExceptOwnTeam,
    /// The `showDeathMessages = false` arm at offset 157: no broadcast at all,
    /// and the dying player's own packet carries `CommonComponents.EMPTY`
    /// rather than the message.
    NobodyAndAnEmptyPacket,
    /// `Team.Visibility.NEVER`, which has **no arm at all** — the three
    /// `if_acmpne` tests fall through to offset 154. Nobody is broadcast to,
    /// but the dying player's own combat-kill packet still carries the real
    /// message, so this is a different outcome from
    /// [`MessageAudience::NobodyAndAnEmptyPacket`] and collapsing the two
    /// would lose the one packet that is actually sent.
    NobodyButTheRealPacket,
}

/// A team's `deathMessageVisibility`, in vanilla's own order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamVisibility {
    Always,
    HideForOtherTeams,
    HideForOwnTeam,
    Never,
}

/// The fan-out `ServerPlayer.die` performs.
///
/// ```text
///   ServerPlayer.die, offsets 7..178
///      7: showDeathMessages = level.getGameRules().get(SHOW_DEATH_MESSAGES)
///     27: ifeq 157                        -> the EMPTY-message arm
///     31: msg = getCombatTracker().getDeathMessage()
///     43: send(new ClientboundPlayerCombatKillPacket(getId(), msg),
///              PacketSendListener.exceptionallySend(...))
///     68: team = getTeam()
///     74: team == null || vis == ALWAYS   -> broadcastSystemMessage(msg,false)
///    105: vis == HIDE_FOR_OTHER_TEAMS     -> broadcastSystemToTeam
///    131: vis == HIDE_FOR_OWN_TEAM        -> broadcastSystemToAllExceptTeam
///    154: (NEVER falls through: nobody is told)
///    157: send(new ClientboundPlayerCombatKillPacket(getId(),
///                                                   CommonComponents.EMPTY))
/// ```
///
/// Two things a reading of the field names would get wrong and the bytecode
/// does not:
///
/// * **`HIDE_FOR_OTHER_TEAMS` sends to the player's OWN team** and
///   `HIDE_FOR_OWN_TEAM` sends to everyone else. The names describe who is
///   hidden from; the calls describe who is told, and they are opposites.
/// * **`NEVER` has no arm.** It is the fall-through at offset 154, so the
///   combat-kill packet still goes to the dying player with the real message
///   in it and nobody else hears anything. That is not the same as the
///   `showDeathMessages = false` case, where the packet itself carries
///   `EMPTY`.
#[must_use]
pub fn death_message_audience(
    show_death_messages: bool,
    team: Option<TeamVisibility>,
) -> MessageAudience {
    if !show_death_messages {
        return MessageAudience::NobodyAndAnEmptyPacket;
    }
    match team {
        None | Some(TeamVisibility::Always) => MessageAudience::Everyone,
        Some(TeamVisibility::HideForOtherTeams) => MessageAudience::OwnTeamOnly,
        Some(TeamVisibility::HideForOwnTeam) => MessageAudience::EveryoneExceptOwnTeam,
        // Offset 154 is the fall-through past all three `if_acmpne` tests:
        // NEVER broadcasts to nobody, but the packet at offset 43 has
        // already gone with the real message in it.
        Some(TeamVisibility::Never) => MessageAudience::NobodyButTheRealPacket,
    }
}

/// Encode `minecraft:player_combat_kill` (68).
///
/// Body is `VAR_INT playerId` then the message as NBT through
/// `ComponentSerialization.TRUSTED_STREAM_CODEC`. The component bytes are
/// handed in already encoded, because building them is `crate::chat`'s job
/// and duplicating a component encoder here is how two encoders drift.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_player_combat_kill(player_id: i32, message_nbt: &[u8]) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    write_varint(&mut o, player_id)?;
    o.write_all(message_nbt)?;
    Ok(o)
}

/// Send it.
///
/// # Errors
/// On I/O failure.
pub fn send_player_combat_kill(
    stream: &mut impl Write,
    comp: Option<i32>,
    player_id: i32,
    message_nbt: &[u8],
) -> io::Result<()> {
    rustmc_proto::frame::write_frame(
        stream,
        comp,
        ids::play::cb::PLAYER_COMBAT_KILL,
        &encode_player_combat_kill(player_id, message_nbt)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one field whose sense is inverted. A port that copied the rule
    /// straight through would show a death screen exactly when vanilla
    /// hides it.
    #[test]
    fn show_death_screen_is_the_negation_of_immediate_respawn() {
        assert!(
            show_death_screen(false),
            "the default rule shows the screen"
        );
        assert!(!show_death_screen(true));
        assert_ne!(show_death_screen(true), show_death_screen(false));
    }

    /// The clamp is on the product. Level 14 is under it, level 15 is over.
    #[test]
    fn the_experience_cap_clamps_seven_times_the_level_at_a_hundred() {
        assert_eq!(base_experience_reward(0, false, false), 0);
        assert_eq!(base_experience_reward(1, false, false), 7);
        assert_eq!(base_experience_reward(14, false, false), 98);
        assert_eq!(
            base_experience_reward(15, false, false),
            100,
            "15 * 7 is 105 and the cap is on the product"
        );
        assert_eq!(base_experience_reward(1000, false, false), 100);
    }

    /// The positive control for the two gates: the same level pays out when
    /// neither is set and pays nothing when either is.
    #[test]
    fn keep_inventory_and_spectator_each_zero_the_reward() {
        assert_eq!(base_experience_reward(10, false, false), 70);
        assert_eq!(base_experience_reward(10, true, false), 0);
        assert_eq!(base_experience_reward(10, false, true), 0);
        assert_eq!(base_experience_reward(10, true, true), 0);
    }

    /// `min(level*7, 100)` on a huge level must not overflow into a negative
    /// reward. `saturating_mul` is why; `i32::MAX * 7` wraps.
    #[test]
    fn an_absurd_level_saturates_rather_than_wrapping_negative() {
        assert_eq!(base_experience_reward(i32::MAX, false, false), 100);
    }

    /// Every arm of `getLocalizedDeathMessage`, with the suffix and the
    /// argument count the bytecode's `anewarray` sizes.
    #[test]
    fn the_death_message_ladder_takes_each_of_its_four_arms() {
        // An entity did it, plain weapon: bare key, two arguments.
        assert_eq!(
            localized_death_message("player", true, false, false),
            DeathMessage {
                key: "death.attack.player".into(),
                args: 2
            }
        );
        // An entity did it with a named weapon: `.item`, three arguments.
        assert_eq!(
            localized_death_message("player", true, true, false),
            DeathMessage {
                key: "death.attack.player.item".into(),
                args: 3
            }
        );
        // No entity, but a kill credit: `.player`, two arguments. This is the
        // arm whose name reads backwards.
        assert_eq!(
            localized_death_message("drown", false, false, true),
            DeathMessage {
                key: "death.attack.drown.player".into(),
                args: 2
            }
        );
        // Nothing at all: bare key, one argument.
        assert_eq!(
            localized_death_message("drown", false, false, false),
            DeathMessage {
                key: "death.attack.drown".into(),
                args: 1
            }
        );
    }

    /// `weapon_has_custom_name` is only consulted on the entity path — a
    /// named weapon with no attacker cannot produce `.item`, because
    /// `weapon` is `ItemStack.EMPTY` on that branch by construction.
    #[test]
    fn a_named_weapon_without_an_attacker_cannot_reach_the_item_arm() {
        let m = localized_death_message("drown", false, true, true);
        assert_eq!(m.key, "death.attack.drown.player");
        assert_eq!(m.args, 2);
    }

    /// The two visibility constants that read as opposites of what they do.
    #[test]
    fn hide_for_other_teams_tells_the_own_team_and_vice_versa() {
        assert_eq!(
            death_message_audience(true, Some(TeamVisibility::HideForOtherTeams)),
            MessageAudience::OwnTeamOnly
        );
        assert_eq!(
            death_message_audience(true, Some(TeamVisibility::HideForOwnTeam)),
            MessageAudience::EveryoneExceptOwnTeam
        );
    }

    /// Positive control for the game rule: the same teamless player reaches
    /// two different audiences depending on one boolean.
    #[test]
    fn the_show_death_messages_rule_moves_the_audience() {
        assert_eq!(
            death_message_audience(true, None),
            MessageAudience::Everyone
        );
        assert_eq!(
            death_message_audience(false, None),
            MessageAudience::NobodyAndAnEmptyPacket
        );
        // And it wins over the team, which is offset 27's `ifeq 157` jumping
        // over the whole team ladder.
        assert_eq!(
            death_message_audience(false, Some(TeamVisibility::HideForOwnTeam)),
            MessageAudience::NobodyAndAnEmptyPacket
        );
    }

    /// `NEVER` and `showDeathMessages = false` are not the same outcome, and
    /// this is the assertion that keeps them apart: one still sends the real
    /// message down the dying player's own connection.
    #[test]
    fn team_visibility_never_is_not_the_same_as_the_rule_being_off() {
        assert_eq!(
            death_message_audience(true, Some(TeamVisibility::Never)),
            MessageAudience::NobodyButTheRealPacket
        );
        assert_ne!(
            death_message_audience(true, Some(TeamVisibility::Never)),
            death_message_audience(false, None)
        );
    }

    /// Two draws, magnitude then angle. Swapping them would keep the
    /// distribution and move every later draw, so the control is a
    /// fixed-stream generator whose two values are known apart.
    #[test]
    fn the_death_drop_takes_two_draws_magnitude_first() {
        use rustmc_worldgen::rng::legacy::LegacyRandomSource;
        let mut a = LegacyRandomSource::new(1234);
        let before = a.next_f32();
        let mut b = LegacyRandomSource::new(1234);
        let (x, y, z) = death_drop_velocity(&mut b);

        // The horizontal magnitude is bounded by `f <= 0.5`, and `f` is the
        // FIRST draw: with this seed that is `before`, so the radius is
        // exactly `before * 0.5` up to f32 rounding.
        let radius = (x * x + z * z).sqrt();
        let expected = f64::from(before * 0.5);
        assert!(
            (radius - expected).abs() < 1e-6,
            "radius {radius} should be the first draw halved, {expected}"
        );
        assert!(radius <= 0.5, "the scatter is bounded by 0.5, got {radius}");

        // The vertical term is the widened 0.2f and takes no draw at all.
        assert_eq!(y.to_bits(), f64::from(0.2_f32).to_bits());
        assert_ne!(
            y.to_bits(),
            0.2_f64.to_bits(),
            "0.2 and the widened 0.2f are not the same number"
        );
    }

    /// The vertical term must not be the aimed branch's `0.1 + spread`.
    /// A port that reached for `crate::dropitem::throw_velocity` here would
    /// pass every "the items are on the floor" check and take four draws
    /// where vanilla takes two.
    #[test]
    fn the_death_drop_is_not_the_aimed_throw() {
        use rustmc_worldgen::rng::legacy::LegacyRandomSource;
        let mut r = LegacyRandomSource::new(7);
        let (_, y, _) = death_drop_velocity(&mut r);
        assert_eq!(y, 0.200_000_002_980_232_24);

        let mut r = LegacyRandomSource::new(7);
        let (_, ay, _) = crate::dropitem::throw_velocity(0.0, 0.0, &mut r);
        assert_ne!(
            y, ay,
            "the aimed branch's vertical term is 0.1 plus a random difference"
        );
    }

    #[test]
    fn a_team_on_always_is_the_same_as_no_team() {
        assert_eq!(
            death_message_audience(true, Some(TeamVisibility::Always)),
            death_message_audience(true, None)
        );
    }

    /// The packet is a varint id and then the component bytes verbatim.
    #[test]
    fn the_combat_kill_packet_is_a_varint_then_the_component() {
        let body = encode_player_combat_kill(28, &[0x0a, 0x00]).expect("encode");
        assert_eq!(body, vec![28, 0x0a, 0x00]);
        // A two-byte varint id, to prove the id is not written as a fixed
        // width the way the join packet's is.
        let body = encode_player_combat_kill(300, &[0x01]).expect("encode");
        assert_eq!(body, vec![0xac, 0x02, 0x01]);
    }

    /// The registry id, against the report rather than against this module.
    #[test]
    fn player_combat_kill_is_sixty_eight_and_not_combat_death() {
        assert_eq!(ids::play::cb::PLAYER_COMBAT_KILL, 68);
    }
}

/// One step of `ServerPlayer.die`, in the order the method performs them.
///
/// The order is a property of the method body and nothing else states it.
/// Three of these are observable on the wire and the rest are state, and a
/// port that gets the *order* wrong produces every packet correctly and still
/// diverges: the entity event at offset 282 must arrive after the combat-kill
/// packet, and the loot at offset 221 must be dropped before the scoreboard
/// and the stats are touched, because `dropAllDeathLoot` reads the inventory
/// that `awardStat` does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DieStep {
    /// 0: `gameEvent(GameEvent.ENTITY_DIE)`.
    GameEventEntityDie,
    /// 43 or 161: `ClientboundPlayerCombatKillPacket`. At 43 it carries the
    /// real message and goes through
    /// `PacketSendListener.exceptionallySend`; at 161 it carries
    /// `CommonComponents.EMPTY` and goes through the plain `send`.
    CombatKillPacket,
    /// 90..151: one of the three broadcast calls, or none.
    BroadcastDeathMessage,
    /// 179: `removeEntitiesOnShoulder()`.
    RemoveEntitiesOnShoulder,
    /// 204: `tellNeutralMobsThatIDied()`, gated on the `forgive_dead_players`
    /// game rule. The AABB is `new AABB(blockPosition()).inflate(32, 10, 32)`
    /// — read from offsets 11..20, and *not* a cube.
    TellNeutralMobs,
    /// 221: `dropAllDeathLoot(level, source)`, gated on `!isSpectator()`.
    DropAllDeathLoot,
    /// 240: `scoreboard.forAllObjectives(DEATH_COUNT, this, ScoreAccess::increment)`.
    IncrementDeathCountObjective,
    /// 263..274: only when `getKillCredit()` is non-null —
    /// `awardStat(ENTITY_KILLED_BY, killer.getType())`,
    /// `killer.awardKillScore(this, source)`, `createWitherRose(killer)`.
    CreditTheKiller,
    /// 282: `level.broadcastEntityEvent(this, 3)` — the death animation.
    /// **Status byte 3**, `iconst_3` at offset 282.
    EntityEventDeath,
    /// 290: `awardStat(Stats.DEATHS)`.
    AwardDeathStat,
    /// 303 and 316: `resetStat(TIME_SINCE_DEATH)` then
    /// `resetStat(TIME_SINCE_REST)`, in that order.
    ResetTimeSinceStats,
    /// 320..330: `clearFire()`, `setTicksFrozen(0)`,
    /// `setSharedFlagOnFire(false)`.
    ClearFireAndFreeze,
    /// 337: `getCombatTracker().recheckStatus()`.
    RecheckCombatStatus,
    /// 358: `setLastDeathLocation(Optional.of(GlobalPos.of(level.dimension(),
    /// blockPosition())))`.
    RecordLastDeathLocation,
    /// 365: `connection.markClientUnloadedAfterDeath()`, which is one
    /// `putfield waitingForRespawn = true`. Eight of
    /// `ServerGamePacketListenerImpl`'s handlers gate on `hasClientLoaded()`,
    /// so this is what stops a dead player's movement packets being acted on.
    MarkClientUnloadedAfterDeath,
}

/// Build the ordered list for one death.
///
/// `show_death_messages` false takes the offset-157 arm, which sends the
/// packet with an empty message and performs **no** broadcast — so the list
/// is shorter by exactly one step, and the step it is shorter by is the
/// broadcast rather than the packet.
#[must_use]
pub fn die_sequence(
    show_death_messages: bool,
    broadcasts: bool,
    forgive_dead_players: bool,
    spectator: bool,
    has_kill_credit: bool,
) -> Vec<DieStep> {
    let mut v = vec![DieStep::GameEventEntityDie, DieStep::CombatKillPacket];
    if show_death_messages && broadcasts {
        v.push(DieStep::BroadcastDeathMessage);
    }
    v.push(DieStep::RemoveEntitiesOnShoulder);
    if forgive_dead_players {
        v.push(DieStep::TellNeutralMobs);
    }
    if !spectator {
        v.push(DieStep::DropAllDeathLoot);
    }
    v.push(DieStep::IncrementDeathCountObjective);
    if has_kill_credit {
        v.push(DieStep::CreditTheKiller);
    }
    v.extend([
        DieStep::EntityEventDeath,
        DieStep::AwardDeathStat,
        DieStep::ResetTimeSinceStats,
        DieStep::ClearFireAndFreeze,
        DieStep::RecheckCombatStatus,
        DieStep::RecordLastDeathLocation,
        DieStep::MarkClientUnloadedAfterDeath,
    ]);
    v
}

/// The `entity_event` status byte for a death animation: **3**, from
/// `ServerPlayer.die` offset 282 (`iconst_3`).
pub const ENTITY_EVENT_DEATH: u8 = 3;

/// The box `tellNeutralMobsThatIDied` searches, from
/// `AABB.inflate(32.0d, 10.0d, 32.0d)` at offsets 11..20.
///
/// It is inflated around `new AABB(blockPosition())`, which is the one-block
/// box at the player's feet, and it is **not a cube** — the vertical reach is
/// 10 and the horizontal 32. Reading it as 32 in all three axes searches
/// 3.2x the volume vanilla does.
pub const NEUTRAL_MOB_FORGIVENESS_BOX: (f64, f64, f64) = (32.0, 10.0, 32.0);

/// What `dropAllDeathLoot` does for a **player**, in order.
///
/// ```text
///   LivingEntity.dropAllDeathLoot(ServerLevel, DamageSource)
///      0: recentlyHit = lastHurtByPlayerMemoryTime > 0
///     13: shouldDropLoot(level) ? dropFromLootTable(...);
///                                 dropCustomDeathLoot(...)
///     35: dropEquipment(level)
///     40: dropExperience(level, source.getEntity())
///
///   Player.dropEquipment(ServerLevel)
///      0: super.dropEquipment(level)          (Avatar's: nothing for a player)
///      5: !keepInventory ->
///     24:   destroyVanishingCursedItems()
///     28:   inventory.dropAll()
///
///   Inventory.dropAll()
///      0: for i in 0..items.size():
///           if !items[i].isEmpty():
///     32:     player.drop(items[i], true, false)
///     43:     items[i] = ItemStack.EMPTY
///     61: equipment.dropAll(player)
/// ```
///
/// So for a player: **the loot table and the custom-loot hooks run first and
/// both do nothing** (`Player` has no death loot table and
/// `dropCustomDeathLoot` is an empty method), then the inventory, then the
/// experience. The experience is last, which is why an inventory drop that
/// throws would leave no orb.
///
/// `Inventory.dropAll` walks the main `items` list **in index order** and
/// clears each slot as it goes, then drops the equipment as a separate pass.
/// The index order is the draw order for [`death_drop_velocity`], so a port
/// that iterated the inventory any other way produces the same items at
/// different velocities.
///
/// This module refuses to model the loot-table arms: a player's are empty,
/// and anything else's belongs to `mob-combat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathLootStep {
    /// `dropFromLootTable` — empty for a player.
    LootTable,
    /// `dropCustomDeathLoot` — an empty method on `LivingEntity` and not
    /// overridden by `Player`.
    CustomDeathLoot,
    /// `destroyVanishingCursedItems`, only under `!keepInventory`.
    DestroyVanishingCursed,
    /// `inventory.dropAll()`, only under `!keepInventory`.
    DropInventory,
    /// `equipment.dropAll(player)`, the second pass inside `dropAll`.
    DropEquipment,
    /// `dropExperience(level, source.getEntity())`.
    DropExperience,
}

/// The ordered loot steps for a dying player.
#[must_use]
pub fn death_loot_sequence(should_drop_loot: bool, keep_inventory: bool) -> Vec<DeathLootStep> {
    let mut v = Vec::new();
    if should_drop_loot {
        v.push(DeathLootStep::LootTable);
        v.push(DeathLootStep::CustomDeathLoot);
    }
    if !keep_inventory {
        v.push(DeathLootStep::DestroyVanishingCursed);
        v.push(DeathLootStep::DropInventory);
        v.push(DeathLootStep::DropEquipment);
    }
    v.push(DeathLootStep::DropExperience);
    v
}

#[cfg(test)]
mod sequence_tests {
    use super::*;

    /// The full order, and the two ends of it: the game event is first and
    /// `markClientUnloadedAfterDeath` is last.
    #[test]
    fn the_die_sequence_starts_with_the_game_event_and_ends_unloaded() {
        let s = die_sequence(true, true, true, false, true);
        assert_eq!(s[0], DieStep::GameEventEntityDie);
        assert_eq!(s[s.len() - 1], DieStep::MarkClientUnloadedAfterDeath);
        assert_eq!(
            s,
            vec![
                DieStep::GameEventEntityDie,
                DieStep::CombatKillPacket,
                DieStep::BroadcastDeathMessage,
                DieStep::RemoveEntitiesOnShoulder,
                DieStep::TellNeutralMobs,
                DieStep::DropAllDeathLoot,
                DieStep::IncrementDeathCountObjective,
                DieStep::CreditTheKiller,
                DieStep::EntityEventDeath,
                DieStep::AwardDeathStat,
                DieStep::ResetTimeSinceStats,
                DieStep::ClearFireAndFreeze,
                DieStep::RecheckCombatStatus,
                DieStep::RecordLastDeathLocation,
                DieStep::MarkClientUnloadedAfterDeath,
            ]
        );
    }

    /// The combat-kill packet comes BEFORE the entity event, and the loot
    /// comes before both the scoreboard and the death stat.
    #[test]
    fn the_packet_precedes_the_entity_event_and_the_loot_precedes_the_stats() {
        let s = die_sequence(true, true, false, false, false);
        let at = |x| s.iter().position(|y| *y == x).expect("step present");
        assert!(at(DieStep::CombatKillPacket) < at(DieStep::EntityEventDeath));
        assert!(at(DieStep::DropAllDeathLoot) < at(DieStep::IncrementDeathCountObjective));
        assert!(at(DieStep::DropAllDeathLoot) < at(DieStep::AwardDeathStat));
        assert!(at(DieStep::RecordLastDeathLocation) < at(DieStep::MarkClientUnloadedAfterDeath));
    }

    /// `showDeathMessages = false` loses the broadcast and keeps the packet.
    /// That is the whole difference, and it is one step.
    #[test]
    fn turning_the_rule_off_drops_the_broadcast_and_not_the_packet() {
        let on = die_sequence(true, true, false, false, false);
        let off = die_sequence(false, true, false, false, false);
        assert_eq!(on.len(), off.len() + 1);
        assert!(off.contains(&DieStep::CombatKillPacket));
        assert!(!off.contains(&DieStep::BroadcastDeathMessage));
    }

    /// A spectator dies without dropping anything — offset 209's
    /// `isSpectator ifne 224` jumps the whole call.
    #[test]
    fn a_spectator_drops_no_loot_but_still_dies() {
        let s = die_sequence(true, true, false, true, false);
        assert!(!s.contains(&DieStep::DropAllDeathLoot));
        assert!(s.contains(&DieStep::EntityEventDeath));
        assert!(s.contains(&DieStep::AwardDeathStat));
    }

    /// `forgive_dead_players` and the kill credit each add exactly one step
    /// and neither reorders anything else.
    #[test]
    fn the_two_optional_steps_are_each_one_step() {
        let base = die_sequence(true, true, false, false, false);
        assert_eq!(
            die_sequence(true, true, true, false, false).len(),
            base.len() + 1
        );
        assert_eq!(
            die_sequence(true, true, false, false, true).len(),
            base.len() + 1
        );
        assert_eq!(
            die_sequence(true, true, true, false, true).len(),
            base.len() + 2
        );
    }

    /// The forgiveness box is 32 x 10 x 32 and not a 32-cube.
    #[test]
    fn the_neutral_mob_box_is_not_a_cube() {
        assert_eq!(NEUTRAL_MOB_FORGIVENESS_BOX, (32.0, 10.0, 32.0));
        assert_ne!(
            NEUTRAL_MOB_FORGIVENESS_BOX.1, NEUTRAL_MOB_FORGIVENESS_BOX.0,
            "reading the vertical as 32 searches 3.2x the volume"
        );
    }

    #[test]
    fn the_death_animation_is_entity_event_three() {
        assert_eq!(ENTITY_EVENT_DEATH, 3);
    }

    /// Experience is last, and `keepInventory` removes the three middle
    /// steps without removing it.
    #[test]
    fn the_loot_order_puts_experience_last_under_both_rules() {
        let drops = death_loot_sequence(true, false);
        assert_eq!(drops[drops.len() - 1], DeathLootStep::DropExperience);
        assert_eq!(
            drops,
            vec![
                DeathLootStep::LootTable,
                DeathLootStep::CustomDeathLoot,
                DeathLootStep::DestroyVanishingCursed,
                DeathLootStep::DropInventory,
                DeathLootStep::DropEquipment,
                DeathLootStep::DropExperience,
            ]
        );

        let kept = death_loot_sequence(true, true);
        assert_eq!(kept[kept.len() - 1], DeathLootStep::DropExperience);
        assert!(!kept.contains(&DeathLootStep::DropInventory));
        assert!(!kept.contains(&DeathLootStep::DestroyVanishingCursed));
        assert_eq!(drops.len(), kept.len() + 3);
    }

    /// The cursed-item destruction is strictly before the drop. Reversed,
    /// every cursed item is already on the floor before anything looks at it.
    #[test]
    fn cursed_items_are_destroyed_before_the_inventory_is_dropped() {
        let s = death_loot_sequence(true, false);
        let at = |x| s.iter().position(|y| *y == x).expect("step present");
        assert!(at(DeathLootStep::DestroyVanishingCursed) < at(DeathLootStep::DropInventory));
        assert!(at(DeathLootStep::DropInventory) < at(DeathLootStep::DropEquipment));
        assert!(destroyed_before_drop(true));
        assert!(!destroyed_before_drop(false));
    }
}

/// A death message as a component, ready for [`encode_player_combat_kill`].
///
/// # THE ARGUMENT IS NOT THE PLAYER'S NAME
///
/// Every arm of `DamageSource.getLocalizedDeathMessage` passes
/// `victim.getDisplayName()`, and for a player that is not a string:
///
/// ```text
///   Player.getDisplayName()
///      0: PlayerTeam.formatNameForTeam(getTeam(), getName())
///     12: decorateDisplayNameComponent(...)
///
///   Player.decorateDisplayNameComponent(MutableComponent)
///      0: name = getGameProfile().name()
///      8: withStyle(lambda$decorateDisplayNameComponent$0)
///
///   lambda$decorateDisplayNameComponent$0(String name, Style s)
///      1: s.withClickEvent(new ClickEvent.SuggestCommand("/tell " + name))
///     17:  .withHoverEvent(createHoverEvent())
///     24:  .withInsertion(name)
///
///   Entity.createHoverEvent()
///      0: new HoverEvent.ShowEntity(
///           new HoverEvent.EntityTooltipInfo(getType(), getUUID(), getName()))
/// ```
///
/// So the argument is `Component.literal(name)` carrying a **three-field
/// style**: a `suggest_command` click event whose payload comes from the
/// concat constant in `Player`'s pool, a `show_entity` hover event holding
/// the entity type, the UUID and the name, and an `insertion` of the bare
/// name.
///
/// A port that writes a bare string argument therefore produces a *different
/// component* from vanilla's, and `tools/oracle-runner`'s `chat` projection
/// digests the whole packet body — there is no partial credit. This is the
/// same shape as the `/list` reply's component, which `docs/SERVER.md`
/// records as byte-identical only because it was captured rather than
/// reasoned about.
///
/// **So this function takes the display name as an already-built [`Nbt`] and
/// refuses to invent it.** That component is not this area's: it is the same
/// one `player_chat`, `tab_list` and `disguised_chat` need, it needs the
/// entity type id and the UUID, and its style's key order goes through
/// `rustmc_world::nbt`'s Java-`HashMap`-bucket writer, whose own doc states
/// where that reproduction is exact and where it is merely legal. Whoever
/// builds it should build it once, beside `crate::chat`.
#[must_use]
pub fn death_message_component(message: &DeathMessage, args: Vec<Nbt>) -> Nbt {
    debug_assert_eq!(
        args.len(),
        message.args,
        "the ladder's arm decides the argument count"
    );
    crate::commands::translatable(
        &message.key,
        &args
            .into_iter()
            .map(crate::commands::Arg::Component)
            .collect::<Vec<_>>(),
    )
}

/// The click-event command a player's display name carries.
///
/// From the `StringConcatFactory` recipe in `Player`'s constant pool
/// (`#2944` -> Utf8 `#2945`). **The recipe is eight bytes, `/tell \x01 `,
/// and the placeholder is NOT its last character**: a space follows it, so
/// the result is `/tell <name> ` with a trailing space — the space that lets
/// a player type the message straight after clicking. `javap -v` prints the
/// constant without it visible; the Utf8 length field (`00 08`) and the byte
/// after `\x01` settle it. `crate::commands::display_name_component` writes
/// the same string from recorded bytes, and this function said otherwise
/// until 2026-09-30.
#[must_use]
pub fn display_name_suggest_command(name: &str) -> String {
    format!("/tell {name} ")
}

/// A whole `player_combat_kill` body, component and all.
///
/// # Errors
/// If the component cannot be encoded.
pub fn encode_player_combat_kill_message(player_id: i32, message: &Nbt) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    write_varint(&mut body, player_id)?;
    crate::commands::write_network_nbt(&mut body, message)?;
    Ok(body)
}

#[cfg(test)]
mod component_tests {
    use super::*;

    /// The click command HAS a trailing space: the concat recipe is the
    /// eight bytes `/tell \x01 `, and it agrees with the component
    /// `crate::commands` wrote from recorded bytes.
    #[test]
    fn the_display_name_suggests_tell_with_a_trailing_space() {
        assert_eq!(display_name_suggest_command("Steve"), "/tell Steve ");
        let from_bytes = crate::commands::display_name_component("Steve");
        let rustmc_world::nbt::Nbt::Compound(m) = &from_bytes else {
            panic!("a compound");
        };
        let rustmc_world::nbt::Nbt::Compound(click) = &m["click_event"] else {
            panic!("a click event");
        };
        assert_eq!(
            click["command"],
            rustmc_world::nbt::Nbt::String(display_name_suggest_command("Steve"))
        );
    }

    /// A no-entity, no-credit death is one argument, and the packet is a
    /// varint id followed by the network component with no root name — tag
    /// byte, then payload, and no empty-name pair between them.
    #[test]
    fn a_generic_death_encodes_as_a_varint_and_a_nameless_component() {
        let msg = generic_death_message();
        assert_eq!(msg.args, 1);
        let comp = death_message_component(&msg, vec![Nbt::String("Steve".into())]);
        let body = encode_player_combat_kill_message(28, &comp).expect("encode");

        assert_eq!(body[0], 28, "the entity id is a varint, not an i32");
        // TAG_Compound, then straight into the payload.
        assert_eq!(body[1], 0x0a);
        assert_ne!(
            &body[2..4],
            &[0x00, 0x00],
            "the network form drops the root name"
        );
        let s = String::from_utf8_lossy(&body);
        assert!(s.contains("death.attack.generic"), "body: {s:?}");
        assert!(s.contains("Steve"));
    }

    /// The argument count is the ladder's, not the caller's guess: the same
    /// key on a different arm takes a different number of arguments and
    /// produces different bytes.
    #[test]
    fn the_arm_decides_the_argument_count_and_the_bytes() {
        let two = localized_death_message("player", true, false, false);
        assert_eq!(two.args, 2);
        let a = death_message_component(
            &two,
            vec![Nbt::String("Steve".into()), Nbt::String("Alex".into())],
        );
        let three = localized_death_message("player", true, true, false);
        assert_eq!(three.args, 3);
        let b = death_message_component(
            &three,
            vec![
                Nbt::String("Steve".into()),
                Nbt::String("Alex".into()),
                Nbt::String("Sword".into()),
            ],
        );
        let ea = encode_player_combat_kill_message(1, &a).expect("encode");
        let eb = encode_player_combat_kill_message(1, &b).expect("encode");
        assert_ne!(ea, eb);
        assert!(String::from_utf8_lossy(&eb).contains("death.attack.player.item"));
    }

    /// A no-argument translatable has no `with` key at all — the rule
    /// `crate::commands` gates against its own golden — so a death-message
    /// arm taking zero arguments would encode differently again. None of them
    /// do, and that is worth asserting: every arm passes at least the victim.
    #[test]
    fn every_death_message_arm_carries_at_least_the_victim() {
        for m in [
            generic_death_message(),
            localized_death_message("x", true, false, false),
            localized_death_message("x", true, true, false),
            localized_death_message("x", false, false, true),
            localized_death_message("x", false, false, false),
        ] {
            assert!(m.args >= 1, "{} took {} arguments", m.key, m.args);
        }
    }
}
