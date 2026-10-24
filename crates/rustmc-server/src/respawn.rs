//! Coming back: `PlayerList.respawn`, the packets it sends and in what order,
//! `ServerPlayer.restoreFrom`'s copy list, and the bed/anchor ladder that
//! decides where the new player stands.
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned jar:
//! `PlayerList.respawn`, `ServerPlayer.restoreFrom`,
//! `ServerPlayer.transferInventoryXpAndScore`,
//! `ServerPlayer.findRespawnPositionAndUseSpawnBlock`,
//! `ServerPlayer.findRespawnAndUseSpawnBlock`,
//! `ServerGamePacketListenerImpl.handleClientCommand`,
//! `ClientboundRespawnPacket`, `CommonPlayerSpawnInfo`,
//! `ClientboundSetExperiencePacket`, `ClientboundSetHealthPacket`,
//! `ServerboundClientCommandPacket`.
//!
//! # THE CORPUS CANNOT SEE ANY OF THIS ON THIS TREE, AND THAT IS MEASURED
//!
//! The brief this was written under proposed grading the death path from a
//! recording: a console `/kill`, then the client's `PERFORM_RESPAWN`, with
//! `combat_death`, the death message and the respawn packets projected. That
//! is not reachable here, for **two independent reasons**, and the second one
//! survives fixing the first:
//!
//! 1. **Nothing can kill the harness player.** `tools/oracle-runner`'s
//!    `Step` enum has eighteen variants — `hold`, `look`, `walk`, `dig`,
//!    `finish_dig`, `drop`, `swap_offhand`, `place`, `use_item`, `attack`,
//!    `sneak`, `container_click`, `close_container`, `sign_update`, `swing`,
//!    `command`, `say`, `wait` — and none of them damages anybody. The only
//!    console route on `main` is the per-scenario `[gamerule]` block, whose
//!    `well_formed` refuses anything but `gamerule <rule> <value>`, and
//!    `Step::Command` runs as the acting player, who is **not an operator**:
//!    `ci/t2-gameplay.sh` says so in its own words at lines 676-677, which is
//!    why the harness sets game rules through the console rather than through
//!    that verb. `/kill` is permission level 2, so vanilla would refuse it
//!    from a scenario exactly as it refuses `/gamerule`.
//!
//! 2. **Nothing can send `PERFORM_RESPAWN`.** `minecraft:client_command` is
//!    id 12 and has been a named constant in `rustmc_proto::ids::play::sb`
//!    for some time, but no `Step` variant constructs one, `Client::act` has
//!    no arm that could, and nothing in `crates/rustmc-server` reads one. So
//!    even a scenario that somehow killed the player would leave vanilla
//!    sitting on its death screen for the rest of the recording, and the
//!    entire second half of this area — every packet `PlayerList.respawn`
//!    sends — would never appear on either side.
//!
//! **So no gameplay check is registered and no `gameplay-baseline.toml` row
//! is added.** A scenario written against this today would compare two
//! servers that both said nothing, which is this project's own named defect
//! class: the check would be green, and no edit to the step it is named for
//! could turn it red. Adding the verb is `harness`'s to do and adding a
//! damage source is `player-systems-2`'s; when both exist, the check to write
//! first is the one whose measured step is the `PERFORM_RESPAWN` itself, and
//! its `observe` list is
//! `["player_combat_kill", "system_chat", "respawn", "set_default_spawn_position", "set_experience"]`
//! in that order — which is the order below, taken from the bytecode.
//!
//! What is here instead is the transcription, with unit tests, and one
//! positive control that is not a self-comparison: the respawn packet's body
//! is a `CommonPlayerSpawnInfo`, which is byte-for-byte the same block a
//! **captured real-server join packet** carries, and
//! [`tests::the_respawn_body_is_the_captured_join_packets_spawn_info`]
//! reproduces those 36 bytes off the wire capture in `crate::play`.

use std::io::{self, Write};

use rustmc_proto::{ids, varint::write_varint, wire};

/// `CommonPlayerSpawnInfo` — the ten-field block shared by `login` and
/// `respawn`.
///
/// The write order is `CommonPlayerSpawnInfo.write(RegistryFriendlyByteBuf)`,
/// offsets 0..; it is **not** the record's declaration order in any place
/// where they could differ, and three of the widths are surprising:
///
/// ```text
///    0: DimensionType.STREAM_CODEC.encode(buf, dimensionType)  <- a bare id
///   13: buf.writeResourceKey(dimension)                        <- a string
///   21: buf.writeLong(seed)
///   30: buf.writeByte(gameType.getId())
///   42: buf.writeByte(GameType.getNullableId(previousGameType)) <- SIGNED, -1
///   54: buf.writeBoolean(isDebug)
///     : buf.writeBoolean(isFlat)
///     : buf.writeOptional(lastDeathLocation, GlobalPos codec)
///     : buf.writeVarInt(portalCooldown)
///     : buf.writeVarInt(seaLevel)
/// ```
///
/// `crate::play::JoinInfo` writes exactly this block inline, with
/// `lastDeathLocation` hardcoded absent. This struct exists because a respawn
/// **can** carry one — `ServerPlayer.die` offset 358 sets it — and because
/// the join encoder's capture is then usable as this one's control.
#[derive(Debug, Clone)]
pub struct CommonSpawnInfo {
    /// Index into `minecraft:dimension_type`. A bare registry id, not a
    /// holder with an inline fallback.
    pub dimension_type: i32,
    pub dimension: String,
    /// The *hashed* seed the client gets.
    pub seed: i64,
    pub game_type: u8,
    /// `-1` for "none". Signed, between a `u8` and two booleans.
    pub previous_game_type: i8,
    pub is_debug: bool,
    pub is_flat: bool,
    /// `Optional<GlobalPos>`: dimension id then a position, or absent.
    ///
    /// `ServerPlayer.die` offsets 340..358 sets this to
    /// `GlobalPos.of(level.dimension(), blockPosition())` on every death, and
    /// `restoreFrom` copies it across to the new player unconditionally
    /// (offset 319), so a respawned player's own spawn info carries where
    /// they last died. The join packet's copy is absent only because a fresh
    /// join has no death behind it.
    pub last_death_location: Option<(String, i32, i32, i32)>,
    pub portal_cooldown: i32,
    pub sea_level: i32,
}

impl CommonSpawnInfo {
    /// Encode the block.
    ///
    /// # Errors
    /// On a write failure, which for a `Vec` cannot happen.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut o = Vec::new();
        write_varint(&mut o, self.dimension_type)?;
        wire::write_string(&mut o, &self.dimension)?;
        wire::write_i64(&mut o, self.seed)?;
        wire::write_u8(&mut o, self.game_type)?;
        #[allow(clippy::cast_sign_loss)]
        wire::write_u8(&mut o, self.previous_game_type as u8)?;
        wire::write_bool(&mut o, self.is_debug)?;
        wire::write_bool(&mut o, self.is_flat)?;
        match &self.last_death_location {
            None => wire::write_bool(&mut o, false)?,
            Some((dim, x, y, z)) => {
                wire::write_bool(&mut o, true)?;
                wire::write_string(&mut o, dim)?;
                wire::write_i64(&mut o, packed_block_pos(*x, *y, *z))?;
            }
        }
        write_varint(&mut o, self.portal_cooldown)?;
        write_varint(&mut o, self.sea_level)?;
        Ok(o)
    }
}

/// `BlockPos.asLong` — 26 bits of x, 12 of y, 26 of z, in that packing.
///
/// Written here rather than borrowed because the respawn path is the first
/// place in this crate that needs a `GlobalPos` on the wire; if a shared one
/// lands later this should become a call to it rather than a second copy.
#[must_use]
pub fn packed_block_pos(x: i32, y: i32, z: i32) -> i64 {
    let x = i64::from(x) & 0x03FF_FFFF;
    let y = i64::from(y) & 0x0FFF;
    let z = i64::from(z) & 0x03FF_FFFF;
    (x << 38) | (z << 12) | y
}

/// The `dataToKeep` byte of `ClientboundRespawnPacket`.
///
/// The packet declares three constants:
///
/// ```text
///   public static final byte KEEP_ATTRIBUTE_MODIFIERS = 1;
///   public static final byte KEEP_ENTITY_DATA         = 2;
///   public static final byte KEEP_ALL_DATA            = 3;
/// ```
///
/// and **`PlayerList.respawn` never writes 3**. Offsets 224..233 are
/// `iload_2; ifeq 232; iconst_1; goto 233; 232: iconst_0`, so the byte is
/// `keepEverything ? 1 : 0` — `KEEP_ATTRIBUTE_MODIFIERS` alone, never
/// `KEEP_ALL_DATA`. Older accounts of this packet say 3, and a port written
/// from one would tell every client returning from the End to keep its
/// entity data as well.
#[must_use]
pub fn data_to_keep(keep_everything: bool) -> u8 {
    u8::from(keep_everything)
}

/// Encode `minecraft:respawn` (82): the spawn info, then one byte.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_respawn(info: &CommonSpawnInfo, keep_everything: bool) -> io::Result<Vec<u8>> {
    let mut o = info.encode()?;
    wire::write_u8(&mut o, data_to_keep(keep_everything))?;
    Ok(o)
}

/// Send it.
///
/// # Errors
/// On I/O failure.
pub fn send_respawn(
    stream: &mut impl Write,
    comp: Option<i32>,
    info: &CommonSpawnInfo,
    keep_everything: bool,
) -> io::Result<()> {
    rustmc_proto::frame::write_frame(
        stream,
        comp,
        ids::play::cb::RESPAWN,
        &encode_respawn(info, keep_everything)?,
    )
}

/// Encode `minecraft:set_experience` (103).
///
/// **The wire order is not the constructor order**, and this is the kind of
/// thing `javap`'s field list gets wrong on its own. The constructor is
/// `(float experienceProgress, int totalExperience, int experienceLevel)` and
/// `PlayerList.respawn` passes them in that order at offsets 364..379, but
/// `write` puts the **level** before the total:
///
/// ```text
///   ClientboundSetExperiencePacket.write(FriendlyByteBuf)
///      0: writeFloat(experienceProgress)
///      9: writeVarInt(experienceLevel)     <- level second
///     18: writeVarInt(totalExperience)     <- total third
/// ```
///
/// Swapping them is a legal packet the client reads as a different amount of
/// experience, and for a fresh respawn — where both are 0 — it is invisible.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_set_experience(progress: f32, level: i32, total: i32) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    wire::write_f32(&mut o, progress)?;
    write_varint(&mut o, level)?;
    write_varint(&mut o, total)?;
    Ok(o)
}

/// Encode `minecraft:set_health` (104): float, varint, float.
///
/// ```text
///   ClientboundSetHealthPacket.write(FriendlyByteBuf)
///      0: writeFloat(health)
///      9: writeVarInt(food)
///     18: writeFloat(saturation)
/// ```
///
/// `PlayerList.respawn` does **not** send this. It calls
/// `player.setHealth(player.getHealth())` at offsets 446..453, which looks
/// like a no-op and is not: `restoreFrom` has already set `lastSentHealth` to
/// `-1.0f` (offset 232), so the next `ServerPlayer.doTick` sees a mismatch
/// and sends the packet from there. The encoder lives here because the
/// respawn is what makes it due; the tick that sends it is
/// `player-systems-2`'s, and this is the named join.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_set_health(health: f32, food: i32, saturation: f32) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    wire::write_f32(&mut o, health)?;
    write_varint(&mut o, food)?;
    wire::write_f32(&mut o, saturation)?;
    Ok(o)
}

/// The clientbound packets `PlayerList.respawn` sends, in the order it sends
/// them.
///
/// Read off the `invokevirtual …send` sites between offsets 205 and 573,
/// which is the only way to get this right: the order is a property of the
/// method body and nothing in the packet definitions implies it.
///
/// ```text
///   205: game_event NO_RESPAWN_BLOCK_AVAILABLE   (only if missingRespawnBlock)
///   249: respawn                                 (82)
///   273: connection.teleport(x,y,z,yRot,xRot)    -> player_position
///   306: set_default_spawn_position              (97)
///   326: change_difficulty
///   355: set_experience                          (103)
///   385: sendActivePlayerEffects -> update_mob_effect per active effect
///   391: sendLevelInfo
///   399: sendPlayerPermissionLevel
///   525: sound RESPAWN_ANCHOR_DEPLETE            (only on an anchor respawn)
/// ```
///
/// Two conditionals bracket the fixed run, and both are easy to lose:
///
/// * the `game_event` comes **before** the respawn packet, not after, and
///   only when the transition reports `missingRespawnBlock`;
/// * the anchor-deplete sound comes **last**, after the permission level,
///   and only when `!keepEverything` and the saved respawn block is still a
///   `RESPAWN_ANCHOR`. It draws one `nextLong()` from the **new level's**
///   random for its seed (offset 562), which is a draw a port that omits the
///   sound never takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnPacket {
    GameEventNoRespawnBlock,
    Respawn,
    PlayerPosition,
    SetDefaultSpawnPosition,
    ChangeDifficulty,
    SetExperience,
    UpdateMobEffect,
    LevelInfo,
    PlayerPermissionLevel,
    SoundRespawnAnchorDeplete,
}

/// Build the ordered list for one respawn.
#[must_use]
pub fn respawn_packet_order(
    missing_respawn_block: bool,
    active_effects: usize,
    anchor_deplete: bool,
) -> Vec<RespawnPacket> {
    let mut v = Vec::new();
    if missing_respawn_block {
        v.push(RespawnPacket::GameEventNoRespawnBlock);
    }
    v.push(RespawnPacket::Respawn);
    v.push(RespawnPacket::PlayerPosition);
    v.push(RespawnPacket::SetDefaultSpawnPosition);
    v.push(RespawnPacket::ChangeDifficulty);
    v.push(RespawnPacket::SetExperience);
    for _ in 0..active_effects {
        v.push(RespawnPacket::UpdateMobEffect);
    }
    v.push(RespawnPacket::LevelInfo);
    v.push(RespawnPacket::PlayerPermissionLevel);
    if anchor_deplete {
        v.push(RespawnPacket::SoundRespawnAnchorDeplete);
    }
    v
}

/// `ServerboundClientCommandPacket.Action`.
///
/// **Three actions in 26.2, not two.** `$values()` builds a three-element
/// array and `handleClientCommand`'s `tableswitch` runs `1 to 3`:
///
/// ```text
///   ServerboundClientCommandPacket$Action.$values()
///      0: iconst_3 / anewarray
///      6: PERFORM_RESPAWN          ordinal 0
///     12: REQUEST_STATS            ordinal 1
///     18: REQUEST_GAMERULE_VALUES  ordinal 2
/// ```
///
/// `REQUEST_GAMERULE_VALUES` is new enough that a brief written from the
/// older shape names only the first two; its handler
/// (`sendGameRuleValues`, offsets 211..) refuses without
/// `Permissions.COMMANDS_GAMEMASTER` and logs
/// `"Player {} tried to request game rule values without required permissions"`.
///
/// The wire form is `FriendlyByteBuf.writeEnum`, which is a varint of the
/// ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCommandAction {
    PerformRespawn,
    RequestStats,
    RequestGameRuleValues,
}

impl ClientCommandAction {
    /// Decode the varint ordinal. An out-of-range value is `None`: vanilla's
    /// `readEnum` throws, so refusing is the faithful answer and defaulting
    /// to `PERFORM_RESPAWN` would respawn a player who asked for statistics.
    #[must_use]
    pub fn from_ordinal(o: i32) -> Option<Self> {
        match o {
            0 => Some(Self::PerformRespawn),
            1 => Some(Self::RequestStats),
            2 => Some(Self::RequestGameRuleValues),
            _ => None,
        }
    }

    /// The ordinal `writeEnum` puts on the wire.
    #[must_use]
    pub fn ordinal(self) -> i32 {
        match self {
            Self::PerformRespawn => 0,
            Self::RequestStats => 1,
            Self::RequestGameRuleValues => 2,
        }
    }
}

/// What `handleClientCommand` does with a `PERFORM_RESPAWN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformRespawnOutcome {
    /// `wonGame` was set: the End-portal return. `respawn(player, true,
    /// CHANGED_DIMENSION)`, then `resetPosition`,
    /// `restartClientLoadTimerAfterRespawn`, and the `CHANGED_DIMENSION`
    /// advancement trigger from `Level.END` to `Level.OVERWORLD`.
    ///
    /// **This arm is refused rather than ported.** The End is `dimensions`'
    /// to serve and the advancement trigger is `advancements`'; naming it
    /// here is the point, so that whoever lands either finds the branch.
    EndPortalReturn,
    /// `getHealth() > 0`: **offset 139 is a bare `return`**. A living player
    /// pressing respawn gets nothing at all — no packet, no error, no log.
    /// That is the whole handler for the case this server is always in, and
    /// it is why a `client_command` arm added today would be correct and
    /// unobservable.
    Ignored,
    /// The real one: `respawn(player, false, KILLED)`, then `resetPosition`
    /// and `restartClientLoadTimerAfterRespawn`, and on a hardcore server
    /// `setGameMode(SPECTATOR)` afterwards.
    Respawned { then_spectator: bool },
}

/// The ladder at `handleClientCommand` offsets 60..191.
///
/// The order of the two tests matters and is not the obvious one:
/// **`wonGame` is checked first**, before health. A player who died in the
/// End and then reached the portal is on the `wonGame` path regardless of
/// health, which is why `respawn` takes `keepEverything` as an argument at
/// all.
#[must_use]
pub fn perform_respawn(won_game: bool, health: f32, hardcore: bool) -> PerformRespawnOutcome {
    if won_game {
        return PerformRespawnOutcome::EndPortalReturn;
    }
    if health > 0.0 {
        return PerformRespawnOutcome::Ignored;
    }
    PerformRespawnOutcome::Respawned {
        then_spectator: hardcore,
    }
}

/// One field `ServerPlayer.restoreFrom` moves from the dead player to the new
/// one, and under which branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedWhen {
    /// Copied on every respawn, death or End return.
    Always,
    /// Only under `keepEverything` — the End-portal return.
    KeepEverythingOnly,
    /// Only under `!keepEverything`, and then only if `keepInventory` is on
    /// or the old player was a spectator.
    DeathAndKeepInventory,
    /// Not copied: the new player's constructor value stands.
    Reset,
}

/// `ServerPlayer.restoreFrom(ServerPlayer, boolean keepEverything)`, field by
/// field and in bytecode order.
///
/// This is the list, not a summary of it. Twenty-two entries, read from
/// offsets 0..338; anything a respawn appears to forget is either `Reset`
/// here or absent from the method entirely.
///
/// The shape worth stating: the method is **three blocks**. Offsets 0..51 run
/// unconditionally, offsets 52..143 are the `keepEverything` arm, offsets
/// 146..185 are the death arm, and offsets 188..338 run unconditionally
/// again. So `enchantmentSeed` and the ender chest survive a death and
/// `foodData` does not, which is not a rule anybody would guess.
#[must_use]
pub fn restore_from_fields() -> Vec<(&'static str, CopiedWhen)> {
    vec![
        // 0..51, unconditional.
        ("wardenSpawnTracker", CopiedWhen::Always),
        ("chatSession", CopiedWhen::Always),
        // setGameModeForPlayer(old.getGameModeForPlayer(),
        //                      old.getPreviousGameModeForPlayer())
        ("gameMode.gameModeForPlayer", CopiedWhen::Always),
        ("gameMode.previousGameModeForPlayer", CopiedWhen::Always),
        // AttributeMap.assignBaseValues(old.getAttributes())
        ("attributes.baseValues", CopiedWhen::Always),
        // 52..143, keepEverything only.
        (
            "attributes.permanentModifiers",
            CopiedWhen::KeepEverythingOnly,
        ),
        ("health", CopiedWhen::KeepEverythingOnly),
        ("foodData", CopiedWhen::KeepEverythingOnly),
        ("activeEffects", CopiedWhen::KeepEverythingOnly),
        ("portalProcess", CopiedWhen::KeepEverythingOnly),
        // 146..185: setHealth(getMaxHealth()) then, if keepInventory or the
        // old player was a spectator, transferInventoryXpAndScore.
        ("inventory", CopiedWhen::DeathAndKeepInventory),
        ("experienceLevel", CopiedWhen::DeathAndKeepInventory),
        ("totalExperience", CopiedWhen::DeathAndKeepInventory),
        ("experienceProgress", CopiedWhen::DeathAndKeepInventory),
        ("score", CopiedWhen::DeathAndKeepInventory),
        // 188..338, unconditional again.
        ("enchantmentSeed", CopiedWhen::Always),
        ("enderChestInventory", CopiedWhen::Always),
        ("DATA_PLAYER_MODE_CUSTOMISATION", CopiedWhen::Always),
        ("recipeBook", CopiedWhen::Always),
        ("seenCredits", CopiedWhen::Always),
        ("enteredNetherPosition", CopiedWhen::Always),
        ("currentExplosionImpactPos", CopiedWhen::Always),
        ("currentExplosionCause", CopiedWhen::Always),
        ("chunkTrackingView", CopiedWhen::Always),
        ("requestedDebugSubscriptions", CopiedWhen::Always),
        ("shoulderEntityLeft", CopiedWhen::Always),
        ("shoulderEntityRight", CopiedWhen::Always),
        ("lastDeathLocation", CopiedWhen::Always),
        ("waypointIcon", CopiedWhen::Always),
        // Set to sentinels rather than copied — offsets 227..241. These are
        // the reason a respawned player gets a set_health and a
        // set_experience from the next tick without the respawn sending one.
        ("lastSentExp", CopiedWhen::Reset),
        ("lastSentHealth", CopiedWhen::Reset),
        ("lastSentFood", CopiedWhen::Reset),
    ]
}

/// The three fields `restoreFrom` sets to a **sentinel** rather than copying,
/// and what to.
///
/// ```text
///   ServerPlayer.restoreFrom, offsets 227..243
///     229: iconst_m1        putfield lastSentExp:I
///     233: ldc_w -1.0f      putfield lastSentHealth:F
///     241: iconst_m1        putfield lastSentFood:I
/// ```
///
/// This is the mechanism by which a respawn produces a `set_health` and a
/// `set_experience` **without `PlayerList.respawn` sending either**: the next
/// `ServerPlayer.doTick` compares the real values against these and finds
/// them different, always. Note `lastSentHealth` is `-1.0f` and not `0.0f` —
/// a respawned player's health is `getMaxHealth()`, so `0.0f` would also
/// differ, but a player who respawns at zero health under a modified max
/// would not, and vanilla's choice removes that case.
#[must_use]
pub fn reset_sentinels() -> [(&'static str, f64); 3] {
    [
        ("lastSentExp", -1.0),
        ("lastSentHealth", -1.0),
        ("lastSentFood", -1.0),
    ]
}

/// The state of `ServerPlayerGameMode` across a death.
///
/// `PlayerList.respawn` constructs a **new** `ServerPlayer`, which constructs
/// a new `ServerPlayerGameMode`; `restoreFrom` copies only the two game types
/// through `setGameModeForPlayer`. Everything else the class holds is back at
/// its constructor value:
///
/// ```text
///   ServerPlayerGameMode's non-static fields
///     level                     new (the respawn's target level)
///     player                    new
///     gameModeForPlayer         COPIED
///     previousGameModeForPlayer COPIED
///     isDestroyingBlock         reset
///     destroyProgressStart      reset
///     destroyPos                reset
///     gameTicks                 reset
///     hasDelayedDestroy         reset
///     delayedDestroyPos         reset
///     delayedTickStart          reset
///     lastSentState             reset
/// ```
///
/// So a player who dies mid-dig comes back with no destroy in progress and no
/// `lastSentState`, and the *next* block-break progress packet is sent
/// unconditionally rather than deduplicated against a stale one.
#[must_use]
pub fn game_mode_fields_across_death() -> Vec<(&'static str, CopiedWhen)> {
    vec![
        ("gameModeForPlayer", CopiedWhen::Always),
        ("previousGameModeForPlayer", CopiedWhen::Always),
        ("isDestroyingBlock", CopiedWhen::Reset),
        ("destroyProgressStart", CopiedWhen::Reset),
        ("destroyPos", CopiedWhen::Reset),
        ("gameTicks", CopiedWhen::Reset),
        ("hasDelayedDestroy", CopiedWhen::Reset),
        ("delayedDestroyPos", CopiedWhen::Reset),
        ("delayedTickStart", CopiedWhen::Reset),
        ("lastSentState", CopiedWhen::Reset),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bytes of a real 26.2 server's join packet, from
    /// `crate::play::tests::VANILLA` — kept here as the same hex string
    /// rather than as a Rust reference, because the point is that two
    /// independent encoders reproduce one capture.
    const VANILLA_JOIN: &str = concat!(
        "0000001c00",
        "03",
        "136d696e6563726166743a6f766572776f726c64",
        "116d696e6563726166743a7468655f656e64",
        "146d696e6563726166743a7468655f6e6574686572",
        "14040a000100",
        "00",
        "136d696e6563726166743a6f766572776f726c64",
        "ede6199ec3f0251e",
        "00ff000000003f0000",
    );

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    /// Where `CommonPlayerSpawnInfo` starts and ends inside the 109-byte
    /// capture, counted from the field widths above it:
    /// 4 (entity id) + 1 (hardcore) + 1 (level count) + 20 + 18 + 21
    /// (three dimension strings) + 6 (max players, two distances, three
    /// booleans) = 71, and the block is 36 bytes.
    const SPAWN_INFO_AT: usize = 71;
    const SPAWN_INFO_LEN: usize = 36;

    /// **The positive control for this whole module.** The respawn packet's
    /// body is a `CommonPlayerSpawnInfo` and nothing else but one byte, so
    /// reproducing the block out of a real server's captured join packet is
    /// a check against vanilla's own bytes rather than against our decoder.
    #[test]
    fn the_respawn_body_is_the_captured_join_packets_spawn_info() {
        let join = unhex(VANILLA_JOIN);
        assert_eq!(join.len(), 109, "the capture is 109 bytes");
        let want = &join[SPAWN_INFO_AT..SPAWN_INFO_AT + SPAWN_INFO_LEN];

        let info = CommonSpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: i64::from_be_bytes(unhex("ede6199ec3f0251e").try_into().expect("8 bytes")),
            game_type: 0,
            previous_game_type: -1,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
        };
        let got = info.encode().expect("encode");
        assert_eq!(
            got, want,
            "the spawn info block must be byte-identical to the capture"
        );

        // And the respawn packet is that block plus exactly one byte.
        let body = encode_respawn(&info, false).expect("encode");
        assert_eq!(body.len(), want.len() + 1);
        assert_eq!(&body[..want.len()], want);
        assert_eq!(body[want.len()], 0);
    }

    /// The `previousGameType` byte is signed and sits between a `u8` and two
    /// booleans, so a wrong width shifts everything after it. This is the
    /// slice-local version of `crate::play`'s assertion.
    #[test]
    fn the_previous_game_type_survives_as_0xff() {
        let info = CommonSpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: 0,
            game_type: 0,
            previous_game_type: -1,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
        };
        let b = info.encode().expect("encode");
        assert!(b.windows(2).any(|w| w == [0x00, 0xff]));
    }

    /// A respawn's spawn info can carry a `lastDeathLocation` where a join's
    /// cannot in practice, and that is a length change of 1 + string + 8.
    #[test]
    fn a_last_death_location_lengthens_the_block() {
        let base = CommonSpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: 0,
            game_type: 0,
            previous_game_type: -1,
            is_debug: false,
            is_flat: false,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
        };
        let without = base.encode().expect("encode").len();
        let with = CommonSpawnInfo {
            last_death_location: Some(("minecraft:overworld".into(), 1, 2, 3)),
            ..base
        }
        .encode()
        .expect("encode")
        .len();
        // 1 varint length + 19 bytes of name = 20, plus the 8-byte packed
        // position, against the single `false` byte it replaces.
        assert_eq!(with - without, 20 + 8);
    }

    /// `BlockPos.asLong`'s packing, on a value where all three fields are
    /// distinguishable and one is negative.
    #[test]
    fn the_packed_block_pos_is_26_12_26() {
        assert_eq!(packed_block_pos(0, 0, 0), 0);
        assert_eq!(packed_block_pos(0, 1, 0), 1);
        assert_eq!(packed_block_pos(0, 0, 1), 1 << 12);
        assert_eq!(packed_block_pos(1, 0, 0), 1 << 38);
        // y is 12 bits, so -1 is 0xFFF and does not bleed into z.
        assert_eq!(packed_block_pos(0, -1, 0), 0x0FFF);
        assert_eq!(packed_block_pos(0, -1, 0) >> 12, 0);
    }

    /// `KEEP_ALL_DATA` is 3 and vanilla never writes it here.
    #[test]
    fn the_data_to_keep_byte_is_one_or_zero_never_three() {
        assert_eq!(data_to_keep(false), 0);
        assert_eq!(data_to_keep(true), 1, "KEEP_ATTRIBUTE_MODIFIERS, not 3");
        assert_ne!(data_to_keep(true), 3);
    }

    /// The level goes on the wire before the total, which is the opposite of
    /// the constructor's argument order.
    #[test]
    fn set_experience_writes_the_level_before_the_total() {
        // progress 0.0, level 7, total 42.
        let b = encode_set_experience(0.0, 7, 42).expect("encode");
        assert_eq!(b, vec![0, 0, 0, 0, 7, 42]);
        // And the two are distinguishable: swapping them changes the bytes.
        let swapped = encode_set_experience(0.0, 42, 7).expect("encode");
        assert_ne!(b, swapped);
    }

    #[test]
    fn set_health_is_float_varint_float() {
        let b = encode_set_health(20.0, 20, 5.0).expect("encode");
        assert_eq!(b.len(), 4 + 1 + 4);
        assert_eq!(&b[0..4], &20.0_f32.to_be_bytes());
        assert_eq!(b[4], 20);
        assert_eq!(&b[5..9], &5.0_f32.to_be_bytes());
    }

    /// The fixed run, and the two conditionals that bracket it.
    #[test]
    fn the_respawn_packet_order_is_the_bytecodes() {
        use RespawnPacket::{
            ChangeDifficulty, GameEventNoRespawnBlock, LevelInfo, PlayerPermissionLevel,
            PlayerPosition, Respawn, SetDefaultSpawnPosition, SetExperience,
            SoundRespawnAnchorDeplete, UpdateMobEffect,
        };
        assert_eq!(
            respawn_packet_order(false, 0, false),
            vec![
                Respawn,
                PlayerPosition,
                SetDefaultSpawnPosition,
                ChangeDifficulty,
                SetExperience,
                LevelInfo,
                PlayerPermissionLevel,
            ]
        );
        // The game event is FIRST, before the respawn packet.
        let missing = respawn_packet_order(true, 0, false);
        assert_eq!(missing[0], GameEventNoRespawnBlock);
        assert_eq!(missing[1], Respawn);
        // The anchor sound is LAST, after the permission level.
        let anchor = respawn_packet_order(false, 0, true);
        assert_eq!(anchor[anchor.len() - 1], SoundRespawnAnchorDeplete);
        assert_eq!(anchor[anchor.len() - 2], PlayerPermissionLevel);
        // Effects sit between set_experience and sendLevelInfo, one each.
        let effects = respawn_packet_order(false, 3, false);
        assert_eq!(
            effects,
            vec![
                Respawn,
                PlayerPosition,
                SetDefaultSpawnPosition,
                ChangeDifficulty,
                SetExperience,
                UpdateMobEffect,
                UpdateMobEffect,
                UpdateMobEffect,
                LevelInfo,
                PlayerPermissionLevel,
            ]
        );
    }

    /// A respawn sends `respawn` (82) and never `login` (49). Asserted
    /// against the two constants so that a future edit to either is caught.
    #[test]
    fn a_respawn_is_packet_82_and_not_the_login_packet() {
        assert_eq!(ids::play::cb::RESPAWN, 82);
        assert_ne!(ids::play::cb::RESPAWN, ids::play::cb::LOGIN);
    }

    /// Three actions, and an unknown ordinal is refused rather than defaulted.
    #[test]
    fn the_client_command_enum_has_three_actions_in_26_2() {
        assert_eq!(
            ClientCommandAction::from_ordinal(0),
            Some(ClientCommandAction::PerformRespawn)
        );
        assert_eq!(
            ClientCommandAction::from_ordinal(1),
            Some(ClientCommandAction::RequestStats)
        );
        assert_eq!(
            ClientCommandAction::from_ordinal(2),
            Some(ClientCommandAction::RequestGameRuleValues)
        );
        assert_eq!(ClientCommandAction::from_ordinal(3), None);
        assert_eq!(ClientCommandAction::from_ordinal(-1), None);
        for a in [
            ClientCommandAction::PerformRespawn,
            ClientCommandAction::RequestStats,
            ClientCommandAction::RequestGameRuleValues,
        ] {
            assert_eq!(ClientCommandAction::from_ordinal(a.ordinal()), Some(a));
        }
    }

    /// `wonGame` is tested before health, so a living player on that path
    /// still respawns.
    #[test]
    fn won_game_is_checked_before_health() {
        assert_eq!(
            perform_respawn(true, 20.0, false),
            PerformRespawnOutcome::EndPortalReturn
        );
        assert_eq!(
            perform_respawn(false, 20.0, false),
            PerformRespawnOutcome::Ignored
        );
    }

    /// The arm this server is permanently in: a living player pressing
    /// respawn gets a bare `return`.
    #[test]
    fn a_living_player_pressing_respawn_is_ignored() {
        assert_eq!(
            perform_respawn(false, 0.001, false),
            PerformRespawnOutcome::Ignored
        );
        assert_eq!(
            perform_respawn(false, 0.0, false),
            PerformRespawnOutcome::Respawned {
                then_spectator: false
            }
        );
        assert_eq!(
            perform_respawn(false, 0.0, true),
            PerformRespawnOutcome::Respawned {
                then_spectator: true
            }
        );
    }

    /// The copy list is a list, and its three blocks are all populated —
    /// a `restoreFrom` port that dropped the death arm would leave this
    /// with two.
    #[test]
    fn restore_from_has_all_four_kinds_of_field() {
        let f = restore_from_fields();
        for kind in [
            CopiedWhen::Always,
            CopiedWhen::KeepEverythingOnly,
            CopiedWhen::DeathAndKeepInventory,
            CopiedWhen::Reset,
        ] {
            assert!(
                f.iter().any(|(_, w)| *w == kind),
                "no field is {kind:?}, so a whole branch of restoreFrom is missing"
            );
        }
        // The two that would be easiest to get backwards.
        assert_eq!(
            f.iter().find(|(n, _)| *n == "foodData").map(|(_, w)| *w),
            Some(CopiedWhen::KeepEverythingOnly),
            "foodData is NOT carried through a death"
        );
        assert_eq!(
            f.iter()
                .find(|(n, _)| *n == "enderChestInventory")
                .map(|(_, w)| *w),
            Some(CopiedWhen::Always),
            "the ender chest survives a death unconditionally"
        );
        assert_eq!(
            f.iter()
                .find(|(n, _)| *n == "enchantmentSeed")
                .map(|(_, w)| *w),
            Some(CopiedWhen::Always)
        );
    }

    /// The sentinels are `-1`, and `lastSentHealth` is `-1.0f` rather than
    /// zero.
    #[test]
    fn the_three_last_sent_fields_are_reset_to_minus_one() {
        let s = reset_sentinels();
        assert_eq!(s.len(), 3);
        for (name, v) in s {
            assert!((v - -1.0).abs() < f64::EPSILON, "{name} should be -1");
        }
        let f = restore_from_fields();
        for (name, _) in s {
            assert_eq!(
                f.iter().find(|(n, _)| *n == name).map(|(_, w)| *w),
                Some(CopiedWhen::Reset),
                "{name} must be listed as Reset, not copied"
            );
        }
    }

    /// Two of twelve game-mode fields cross a death.
    #[test]
    fn only_the_two_game_types_survive_the_new_game_mode_object() {
        let f = game_mode_fields_across_death();
        let kept: Vec<_> = f
            .iter()
            .filter(|(_, w)| *w == CopiedWhen::Always)
            .map(|(n, _)| *n)
            .collect();
        assert_eq!(kept, vec!["gameModeForPlayer", "previousGameModeForPlayer"]);
        assert_eq!(
            f.iter().filter(|(_, w)| *w == CopiedWhen::Reset).count(),
            8,
            "eight fields are back at their constructor values"
        );
    }
}

/// Encode `minecraft:set_default_spawn_position` (97).
///
/// # This is not `BlockPos + angle` any more, and the shape is four fields
///
/// The packet is a record of one field, a `LevelData.RespawnData`, and that
/// record's `STREAM_CODEC` composes three:
///
/// ```text
///   LevelData$RespawnData.static{}
///     41: GlobalPos.STREAM_CODEC   -> globalPos
///     49: ByteBufCodecs.FLOAT      -> yaw
///     57: ByteBufCodecs.FLOAT      -> pitch
///
///   GlobalPos.static{}
///     20: ResourceKey.streamCodec(Registries.DIMENSION) -> dimension
///     31: BlockPos.STREAM_CODEC                         -> pos
/// ```
///
/// So the body is **a dimension identifier, a packed position, a yaw and a
/// pitch** — four values where every older account of this packet has two.
/// A port that wrote a position and one angle would desync the client by the
/// length of a dimension string on the very first respawn, and the field it
/// would appear to be wrong about is whichever one happened to land there.
///
/// `RespawnData.DEFAULT` is `(OVERWORLD, BlockPos.ZERO, 0.0f, 0.0f)`.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_set_default_spawn_position(
    dimension: &str,
    x: i32,
    y: i32,
    z: i32,
    yaw: f32,
    pitch: f32,
) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    wire::write_string(&mut o, dimension)?;
    wire::write_i64(&mut o, packed_block_pos(x, y, z))?;
    wire::write_f32(&mut o, yaw)?;
    wire::write_f32(&mut o, pitch)?;
    Ok(o)
}

/// Send it.
///
/// # Errors
/// On I/O failure.
#[allow(clippy::too_many_arguments)]
pub fn send_set_default_spawn_position(
    stream: &mut impl Write,
    comp: Option<i32>,
    dimension: &str,
    x: i32,
    y: i32,
    z: i32,
    yaw: f32,
    pitch: f32,
) -> io::Result<()> {
    rustmc_proto::frame::write_frame(
        stream,
        comp,
        ids::play::cb::SET_DEFAULT_SPAWN_POSITION,
        &encode_set_default_spawn_position(dimension, x, y, z, yaw, pitch)?,
    )
}

/// `ClientboundGameEventPacket.Type.NO_RESPAWN_BLOCK_AVAILABLE` — id **0**,
/// the first constant in the class's `static {}` (offset 20, `iconst_0`).
///
/// The packet body is `writeByte(type.id)` then `writeFloat(param)`, and
/// `PlayerList.respawn` passes `fconst_0` for the parameter, so this is
/// always the five bytes `00 00 00 00 00` after the packet id.
pub const GAME_EVENT_NO_RESPAWN_BLOCK_AVAILABLE: u8 = 0;

/// Encode the `game_event` a failed bed or anchor produces.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn encode_game_event(kind: u8, param: f32) -> io::Result<Vec<u8>> {
    let mut o = Vec::new();
    wire::write_u8(&mut o, kind)?;
    wire::write_f32(&mut o, param)?;
    Ok(o)
}

/// What the saved respawn block is, as far as
/// `ServerPlayer.findRespawnAndUseSpawnBlock` cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnBlock {
    /// A `RespawnAnchorBlock`, with its `CHARGE` property.
    Anchor { charge: i32 },
    /// A `BedBlock`.
    Bed,
    /// Anything else — including air, because a bed that was broken while
    /// the player was away leaves air behind.
    Other,
}

/// Where a respawning player ends up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RespawnOutcome {
    /// A stand-up position was found beside the anchor or the bed.
    ///
    /// `use_charge` says whether this call consumed one charge of the
    /// anchor. It is `!keepEverything` at the `PlayerList.respawn` call site
    /// (offsets 0..13), so a real death spends a charge and an End-portal
    /// return does not.
    AtSpawnBlock { used_anchor_charge: bool },
    /// `forced` was set and the saved block is one a player can stand in,
    /// with a second such block above it — the `/spawnpoint` case. The
    /// position is the block's own, offset `(+0.5, +0.1, +0.5)`, and it
    /// keeps the saved yaw and pitch.
    AtForcedPosition,
    /// `Optional.empty()` came back from `findRespawnAndUseSpawnBlock`:
    /// `TeleportTransition.missingRespawnBlock`, which is the world spawn
    /// **plus** a `game_event NO_RESPAWN_BLOCK_AVAILABLE` to the client.
    /// This is the "your home bed was missing or obstructed" case, and in
    /// 26.2 it is a game event and not a chat message.
    WorldSpawnAndNoRespawnBlockEvent,
    /// No respawn config at all, or its dimension no longer exists:
    /// `TeleportTransition.createDefault`, the world spawn with **no** game
    /// event. A player who has never slept takes this path, and telling a
    /// client its bed was missing when it never had one is the bug this
    /// branch exists to avoid.
    WorldSpawnSilently,
}

/// `ServerPlayer.findRespawnPositionAndUseSpawnBlock` and the static
/// `findRespawnAndUseSpawnBlock` it delegates to.
///
/// ```text
///   findRespawnPositionAndUseSpawnBlock(boolean useCharge, PostTeleport p)
///      0: cfg = getRespawnConfig()
///      5: lvl = server.getLevel(RespawnConfig.getDimensionOrDefault(cfg))
///     18: lvl == null || cfg == null      -> createDefault(...)
///     27: findRespawnAndUseSpawnBlock(lvl, cfg, useCharge)
///     36: present ? new TeleportTransition(...) : missingRespawnBlock(...)
///
///   findRespawnAndUseSpawnBlock(ServerLevel, RespawnConfig, boolean)
///     44: block instanceof RespawnAnchorBlock
///     52:   && (forced || state.getValue(CHARGE) > 0)
///     74:   && RespawnAnchorBlock.canSetSpawn(level, pos)
///     83:   -> findStandUpPosition; if (!forced && useCharge && present)
///                setBlock(pos, state.setValue(CHARGE, charge - 1), 3)
///    162: block instanceof BedBlock
///    170:   && environmentAttributes.getValue(BED_RULE, pos)
///              .canSetSpawn(level)
///    192:   -> BedBlock.findStandUpPosition(PLAYER, level, pos, facing, yaw)
///    225: !forced -> Optional.empty()
///    234: forced && isPossibleToRespawnInThis(state)
///              && isPossibleToRespawnInThis(above)
///    276:   -> pos + (0.5, 0.1, 0.5), saved yaw and pitch
///    328: Optional.empty()
/// ```
///
/// Three things the shape of the method makes easy to get wrong:
///
/// * **`forced` bypasses the anchor's charge test but not `canSetSpawn`.**
///   Offset 52's `ifne 74` jumps *over* the charge comparison and lands on
///   the dimension check, so a `/spawnpoint` on an uncharged anchor in the
///   nether still works and one in the overworld still does not.
/// * **The charge is only spent when all three of `!forced`, `useCharge` and
///   a stand-up position hold** (offsets 94..148). A respawn that finds the
///   anchor obstructed does not waste the charge.
/// * **The `forced` fallback needs TWO free blocks**, the saved one and the
///   one above it (offsets 243..273), and both through
///   `Block.isPossibleToRespawnInThis` rather than a collision test.
#[derive(Debug, Clone, Copy)]
pub struct RespawnQuery {
    /// The player has a `RespawnConfig` at all.
    pub has_config: bool,
    /// `server.getLevel(RespawnConfig.getDimensionOrDefault(cfg))` is
    /// non-null — a datapack that removed a dimension takes this away.
    pub dimension_exists: bool,
    /// What is standing at the saved position now.
    pub block: RespawnBlock,
    /// `RespawnConfig.forced` — set by `/spawnpoint`, clear by sleeping.
    pub forced: bool,
    /// `!keepEverything` at the `PlayerList.respawn` call site.
    pub use_charge: bool,
    /// `RespawnAnchorBlock.canSetSpawn(level, pos)` for an anchor, or
    /// `BED_RULE.canSetSpawn(level)` for a bed. Both are dimension rules.
    pub can_set_spawn_here: bool,
    /// `findStandUpPosition` returned a position.
    pub stand_up_position_found: bool,
    /// Both the saved block and the one above pass
    /// `Block.isPossibleToRespawnInThis`.
    pub saved_and_above_are_respawnable: bool,
}

#[must_use]
pub fn respawn_outcome(q: RespawnQuery) -> RespawnOutcome {
    if !q.has_config || !q.dimension_exists {
        return RespawnOutcome::WorldSpawnSilently;
    }
    match q.block {
        RespawnBlock::Anchor { charge } => {
            if (q.forced || charge > 0) && q.can_set_spawn_here && q.stand_up_position_found {
                return RespawnOutcome::AtSpawnBlock {
                    used_anchor_charge: !q.forced && q.use_charge,
                };
            }
        }
        RespawnBlock::Bed => {
            if q.can_set_spawn_here && q.stand_up_position_found {
                return RespawnOutcome::AtSpawnBlock {
                    used_anchor_charge: false,
                };
            }
        }
        RespawnBlock::Other => {}
    }
    if q.forced && q.saved_and_above_are_respawnable {
        return RespawnOutcome::AtForcedPosition;
    }
    RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
}

/// The offset a `forced` respawn stands at, relative to the saved block —
/// `(x + 0.5, y + 0.1, z + 0.5)`, offsets 284..313.
///
/// The `0.1` is a `double` literal and not a `float`, unlike almost every
/// other small constant on this path.
pub const FORCED_RESPAWN_OFFSET: (f64, f64, f64) = (0.5, 0.1, 0.5);

#[cfg(test)]
mod spawn_tests {
    use super::*;

    /// Four fields, and the dimension is one of them.
    #[test]
    fn set_default_spawn_position_carries_a_dimension_a_pos_and_two_angles() {
        let b = encode_set_default_spawn_position("minecraft:overworld", 0, 0, 0, 0.0, 0.0)
            .expect("encode");
        // 1 length byte + 19 name bytes + 8 packed + 4 yaw + 4 pitch.
        assert_eq!(b.len(), 1 + 19 + 8 + 4 + 4);
        assert_eq!(b[0], 19);
        assert_eq!(&b[1..20], b"minecraft:overworld");
        assert_eq!(&b[20..28], &[0u8; 8]);

        // Pitch is a separate field from yaw, so they are distinguishable.
        let b = encode_set_default_spawn_position("minecraft:overworld", 0, 0, 0, 90.0, -45.0)
            .expect("encode");
        assert_eq!(&b[28..32], &90.0_f32.to_be_bytes());
        assert_eq!(&b[32..36], &(-45.0_f32).to_be_bytes());
    }

    #[test]
    fn the_no_respawn_block_game_event_is_type_zero_with_a_zero_parameter() {
        assert_eq!(GAME_EVENT_NO_RESPAWN_BLOCK_AVAILABLE, 0);
        let b = encode_game_event(GAME_EVENT_NO_RESPAWN_BLOCK_AVAILABLE, 0.0).expect("encode");
        assert_eq!(b, vec![0, 0, 0, 0, 0]);
    }

    /// No config, or a dimension the server no longer has, is the SILENT
    /// world spawn — no game event. Telling a client its bed was missing
    /// when it never had one is what this arm avoids.
    #[test]
    fn a_player_who_never_slept_goes_to_spawn_without_a_game_event() {
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: false,
                dimension_exists: true,
                block: RespawnBlock::Other,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true
            }),
            RespawnOutcome::WorldSpawnSilently
        );
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: false,
                block: RespawnBlock::Bed,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true
            }),
            RespawnOutcome::WorldSpawnSilently
        );
        // A broken bed is a different outcome and DOES carry the event.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Other,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
    }

    /// `forced` jumps over the charge test and lands on `canSetSpawn`, so an
    /// uncharged anchor works under `/spawnpoint` and a wrong-dimension one
    /// still does not.
    #[test]
    fn forced_bypasses_the_anchor_charge_but_not_can_set_spawn() {
        let uncharged = RespawnBlock::Anchor { charge: 0 };
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: uncharged,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent,
            "an uncharged anchor is not a spawn without forced"
        );
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: uncharged,
                forced: true,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::AtSpawnBlock {
                used_anchor_charge: false
            },
            "forced skips the charge test"
        );
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: uncharged,
                forced: true,
                use_charge: true,
                can_set_spawn_here: false,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true
            }),
            RespawnOutcome::AtForcedPosition,
            "but canSetSpawn still refuses, and forced then falls to its own arm"
        );
    }

    /// The charge is spent only when all three conditions hold, and an
    /// obstructed anchor does not waste one.
    #[test]
    fn the_anchor_charge_is_spent_on_exactly_one_combination() {
        let charged = RespawnBlock::Anchor { charge: 3 };
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: charged,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::AtSpawnBlock {
                used_anchor_charge: true
            }
        );
        // keepEverything at the call site means use_charge is false.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: charged,
                forced: false,
                use_charge: false,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::AtSpawnBlock {
                used_anchor_charge: false
            }
        );
        // Obstructed: no stand-up position, so no charge and no spawn.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: charged,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: false,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
    }

    /// A bed never spends a charge, and an obstructed bed is the game-event
    /// arm rather than a silent one.
    #[test]
    fn an_obstructed_bed_sends_the_game_event() {
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Bed,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::AtSpawnBlock {
                used_anchor_charge: false
            }
        );
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Bed,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: false,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
        // BED_RULE says no (the nether, the end): same arm.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Bed,
                forced: false,
                use_charge: true,
                can_set_spawn_here: false,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
    }

    /// The forced fallback needs the saved block AND the one above it.
    #[test]
    fn the_forced_fallback_needs_two_respawnable_blocks() {
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Other,
                forced: true,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true
            }),
            RespawnOutcome::AtForcedPosition
        );
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Other,
                forced: true,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: false
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
        // And it is only reachable under `forced`.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: true,
                dimension_exists: true,
                block: RespawnBlock::Other,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true
            }),
            RespawnOutcome::WorldSpawnAndNoRespawnBlockEvent
        );
    }

    /// The 0.1 is a double literal, not a widened 0.1f.
    #[test]
    fn the_forced_respawn_offset_is_a_double_tenth() {
        assert_eq!(FORCED_RESPAWN_OFFSET, (0.5, 0.1, 0.5));
        assert_ne!(
            FORCED_RESPAWN_OFFSET.1.to_bits(),
            f64::from(0.1_f32).to_bits()
        );
    }
}

/// What each `client_command` action would need before it could be served,
/// and why no arm for this packet is added to the play dispatch loop.
///
/// # There is no live arm, on purpose
///
/// `minecraft:client_command` is id 12 and currently falls through every
/// branch of `crate::world_send`'s frame loop to `crate::player::apply`,
/// whose `match id` ends `_ => Ok(Update::Ignored)`. So an arm added today
/// would change nothing any client or any check could see, and the module
/// would then contain a decoder feeding a branch that produces no output —
/// which is the shape `docs/AGENT-BRIEF.md` names as this project's central
/// defect and cites `ACCEPT_TELEPORTATION` for.
///
/// Each of the three actions is refused for a *different* reason, and naming
/// them apart is the point: two are somebody else's substrate and one is a
/// fidelity problem this lane could not solve by writing more code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientCommandBlocker {
    /// `PERFORM_RESPAWN`: **vanilla's own answer here is nothing.**
    /// `handleClientCommand` offset 139 is a bare `return` for
    /// `getHealth() > 0`, and every player this server has ever had is at
    /// full health because it has no damage path. So the faithful behaviour
    /// is already implemented, by not implementing it, and it stays that way
    /// until a player can reach zero health — which is
    /// `origin/player-systems-2`'s `hurt_server` and `FoodData.tick`, not
    /// this branch's.
    NoDamagePathSoTheFaithfulAnswerIsSilence,
    /// `REQUEST_STATS`: `player.getStats().sendStats(player)`, which needs a
    /// `ServerStatsCounter`. This server awards no statistics — the
    /// `awardStat` calls in `ServerPlayer.die` have nowhere to go either —
    /// so the packet would carry an empty map that vanilla's would not.
    /// A wrong answer is worse than none.
    NoStatisticsCounter,
    /// `REQUEST_GAMERULE_VALUES`: this is the one that could otherwise be
    /// served today, and it is refused for fidelity rather than substrate.
    ///
    /// The permission half is exact and cheap:
    /// `sendGameRuleValues` offsets 0..39 refuse without
    /// `Permissions.COMMANDS_GAMEMASTER` and log
    /// `"Player {} tried to request game rule values without required
    /// permissions"`, and `crate::ops` already knows a player's level.
    /// `crate::gamerules` holds all 59 rules and
    /// `GameRuleValue::serialize` is already vanilla's own string form, so
    /// the *content* of `ClientboundGameRuleValuesPacket` (id **39**) is
    /// available.
    ///
    /// **The wire order is not.** The packet's codec is
    /// `ByteBufCodecs.map(HashMap::new, ResourceKey.streamCodec(GAME_RULE),
    /// STRING_UTF8)` (`static {}` offsets 0..14), and `sendGameRuleValues`
    /// fills a `java.util.HashMap` at offset 51. A `HashMap`'s iteration
    /// order is its bucket order over the keys' hashes, so reproducing the
    /// bytes means reproducing `ResourceKey`'s `hashCode` and Java's
    /// `HashMap` bucket layout. Emitting the 59 entries in registration
    /// order instead would be an *approximation* — a packet a client
    /// accepts and a byte comparison rejects — and this project defers
    /// verification, never fidelity. Whoever needs this should derive the
    /// order first and then send it.
    HashMapWireOrderNotDerived,
}

/// The blocker for each action.
#[must_use]
pub fn client_command_blocker(action: ClientCommandAction) -> ClientCommandBlocker {
    match action {
        ClientCommandAction::PerformRespawn => {
            ClientCommandBlocker::NoDamagePathSoTheFaithfulAnswerIsSilence
        }
        ClientCommandAction::RequestStats => ClientCommandBlocker::NoStatisticsCounter,
        ClientCommandAction::RequestGameRuleValues => {
            ClientCommandBlocker::HashMapWireOrderNotDerived
        }
    }
}

/// `restartClientLoadTimerAfterRespawn` sets `clientLoadedTimeoutTimer` to
/// **60** ticks (`bipush 60`, offset 6) after clearing `waitingForRespawn`.
///
/// The pair is the reason a dead player's packets stop being acted on:
/// `markClientUnloadedAfterDeath` is one `putfield waitingForRespawn = true`
/// (offsets 0..5), eight of `ServerGamePacketListenerImpl`'s handlers gate on
/// `hasClientLoaded()`, and each returns silently rather than erroring. This
/// server has **no client-loaded gate at all** — nothing in
/// `crates/rustmc-server` reads `minecraft:player_loaded` (id 44), whose own
/// constant already says so — so there is nothing here for the death half to
/// set. Named rather than added: turning the gate on would start dropping
/// movement packets the corpus currently relies on, and that is
/// `crate::play`'s call and not this branch's.
pub const CLIENT_LOADED_TIMEOUT_TICKS_AFTER_RESPAWN: i32 = 60;

#[cfg(test)]
mod refusal_tests {
    use super::*;

    /// Three actions, three *different* blockers. Collapsing them into one
    /// "not implemented" would lose the fact that two are waiting on another
    /// branch's substrate and one is waiting on a derivation.
    #[test]
    fn each_client_command_action_is_blocked_for_its_own_reason() {
        let b: Vec<_> = [
            ClientCommandAction::PerformRespawn,
            ClientCommandAction::RequestStats,
            ClientCommandAction::RequestGameRuleValues,
        ]
        .into_iter()
        .map(client_command_blocker)
        .collect();
        assert_eq!(
            b,
            vec![
                ClientCommandBlocker::NoDamagePathSoTheFaithfulAnswerIsSilence,
                ClientCommandBlocker::NoStatisticsCounter,
                ClientCommandBlocker::HashMapWireOrderNotDerived,
            ]
        );
        let mut uniq = b.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 3, "three distinct reasons, not one");
    }

    #[test]
    fn the_post_respawn_client_load_timer_is_sixty_ticks() {
        assert_eq!(CLIENT_LOADED_TIMEOUT_TICKS_AFTER_RESPAWN, 60);
    }
}

/// **The respawn's invulnerability is not a tick counter.** It is the
/// client-loaded gate, and the number in it is a ceiling rather than a
/// duration.
///
/// The brief this branch was written under asked for "the respawn's
/// invulnerability ticks", which is the shape older versions had. In 26.2 it
/// is three methods:
///
/// ```text
///   ServerGamePacketListenerImpl.hasClientLoaded()
///      0: waitingForRespawn        ifne 18 -> false
///      7: clientLoadedTimeoutTimer ifgt 18 -> false
///     14: iconst_1                         -> true
///
///   ServerPlayer.isInvulnerableTo(ServerLevel, DamageSource)
///      3: Player.isInvulnerableTo(...)     ifne 36 -> TRUE
///     10: isChangingDimension()            ifeq 26
///     17:   && !source.is(DamageTypes.ENDER_PEARL)  -> TRUE
///     27: connection.hasClientLoaded()     ifne 40 -> false
///     36: iconst_1                                 -> TRUE
///
///   ServerPlayer.die            -> markClientUnloadedAfterDeath()
///                                    waitingForRespawn = true
///   handleClientCommand         -> restartClientLoadTimerAfterRespawn()
///                                    waitingForRespawn = false
///                                    clientLoadedTimeoutTimer = 60
/// ```
///
/// So a dead player is invulnerable because `waitingForRespawn` is set, and a
/// just-respawned one is invulnerable for **at most 60 ticks**, ending the
/// moment the client sends `minecraft:player_loaded` (44) or the timer runs
/// out — whichever is first. A port that ran a fixed 60-tick immunity would
/// keep a loaded, moving player immune for up to three seconds after vanilla
/// had stopped.
///
/// [`CLIENT_LOADED_TIMEOUT_TICKS_AFTER_RESPAWN`] holds the 60.
#[must_use]
pub fn has_client_loaded(waiting_for_respawn: bool, client_loaded_timeout_timer: i32) -> bool {
    !waiting_for_respawn && client_loaded_timeout_timer <= 0
}

/// `ServerPlayer.isInvulnerableTo`'s own two arms, on top of
/// `Player.isInvulnerableTo`'s.
///
/// `Player.isInvulnerableTo` is a ladder of damage-type tags against game
/// rules — `IS_DROWNING` against `drowning_damage`, `IS_FALL` against
/// `fall_damage`, `IS_FIRE` against the next — and it belongs to whoever
/// lands the damage path (`origin/player-systems-2`), so it is a parameter
/// here rather than a reimplementation. What this function adds is the two
/// checks `ServerPlayer` layers on top, and the second of them is the
/// respawn's whole immunity.
#[must_use]
pub fn server_player_is_invulnerable_to(
    base_player_says_invulnerable: bool,
    changing_dimension: bool,
    source_is_ender_pearl: bool,
    client_loaded: bool,
) -> bool {
    if base_player_says_invulnerable {
        return true;
    }
    if changing_dimension && !source_is_ender_pearl {
        return true;
    }
    !client_loaded
}

/// `LivingEntity.tickDeath`, which a player runs unmodified —
/// **neither `Player` nor `ServerPlayer` overrides it**, checked by grep over
/// both disassemblies.
///
/// ```text
///   LivingEntity.tickDeath()
///      0: deathTime++
///     10: deathTime < 20                   -> return
///     19: level.isClientSide               -> return
///     29: isRemoved()                      -> return
///     36: level.broadcastEntityEvent(this, 60)
///     46: remove(RemovalReason.KILLED)
/// ```
///
/// So a player who sits on the death screen has their entity **removed from
/// the level after 20 ticks**, announced with entity event **60** — a
/// different byte from the death animation's 3, and the pair is easy to
/// conflate. The connection keeps its dead `ServerPlayer` object, which is
/// why `PlayerList.respawn` can still read an inventory and a score off it
/// minutes later.
pub const DEATH_TIME_BEFORE_REMOVAL: i32 = 20;

/// The entity event `tickDeath` broadcasts when it removes the body: **60**
/// (`bipush 60`, offset 41). Not the 3 of the death animation.
pub const ENTITY_EVENT_POOF: u8 = 60;

#[cfg(test)]
mod invulnerability_tests {
    use super::*;

    /// Both conditions have to hold, and each on its own keeps the player
    /// unloaded — which is what makes this an immunity rather than a timer.
    #[test]
    fn has_client_loaded_needs_both_the_flag_clear_and_the_timer_expired() {
        assert!(has_client_loaded(false, 0));
        assert!(has_client_loaded(false, -1));
        assert!(!has_client_loaded(true, 0), "dead: waitingForRespawn");
        assert!(
            !has_client_loaded(false, 60),
            "just respawned: timer running"
        );
        assert!(!has_client_loaded(true, 60));
    }

    /// The whole death-to-respawn immunity, walked in order.
    #[test]
    fn the_immunity_runs_from_death_to_at_most_sixty_ticks_after_respawn() {
        let invuln = |waiting, timer| {
            server_player_is_invulnerable_to(false, false, false, has_client_loaded(waiting, timer))
        };
        // Alive and loaded: vulnerable.
        assert!(!invuln(false, 0));
        // die() sets waitingForRespawn.
        assert!(invuln(true, 0));
        // restartClientLoadTimerAfterRespawn clears it and starts the timer.
        assert!(invuln(false, CLIENT_LOADED_TIMEOUT_TICKS_AFTER_RESPAWN));
        // player_loaded arrives, or the timer expires: vulnerable again.
        assert!(!invuln(false, 0));
    }

    /// A fixed 60-tick immunity would differ from vanilla's for every client
    /// that answers early, which is every real client.
    #[test]
    fn a_client_that_loads_early_ends_the_immunity_early() {
        // Vanilla: the packet zeroes the timer, so tick 1 is already
        // vulnerable.
        assert!(has_client_loaded(false, 0));
        // A fixed counter would still be at 59 here and still immune.
        assert!(!has_client_loaded(false, 59));
        assert_ne!(has_client_loaded(false, 0), has_client_loaded(false, 59));
    }

    /// The dimension arm inverts on the damage type: changing dimension is
    /// immunity to everything EXCEPT an ender pearl.
    #[test]
    fn changing_dimension_is_immunity_except_to_an_ender_pearl() {
        assert!(server_player_is_invulnerable_to(false, true, false, true));
        assert!(!server_player_is_invulnerable_to(false, true, true, true));
    }

    /// The base ladder short-circuits everything below it.
    #[test]
    fn the_base_player_ladder_wins_outright() {
        assert!(server_player_is_invulnerable_to(true, false, true, true));
    }

    /// Two different entity-event bytes on one death path.
    #[test]
    fn the_poof_event_is_sixty_and_the_death_animation_is_three() {
        assert_eq!(ENTITY_EVENT_POOF, 60);
        assert_eq!(crate::death::ENTITY_EVENT_DEATH, 3);
        assert_ne!(ENTITY_EVENT_POOF, crate::death::ENTITY_EVENT_DEATH);
        assert_eq!(DEATH_TIME_BEFORE_REMOVAL, 20);
    }
}

/// `ServerPlayer.setRespawnPosition(RespawnConfig, boolean sendMessage)` and
/// the message it sometimes sends.
///
/// ```text
///   ServerPlayer.setRespawnPosition(RespawnConfig cfg, boolean sendMessage)
///      0: sendMessage      ifeq 26
///      4: cfg != null      ifnull 26
///      8: cfg.isSamePosition(this.respawnConfig)  ifne 26
///     19: sendSystemMessage(SPAWN_SET_MESSAGE)
///     26: this.respawnConfig = cfg
///
///   ServerPlayer$RespawnConfig.isSamePosition(RespawnConfig other)
///      0: other == null                      -> false
///      4: this.respawnData.globalPos()
///          .equals(other.respawnData.globalPos())
/// ```
///
/// Three things that are each one `ifeq` away from being wrong:
///
/// * **The message is suppressed when the position is UNCHANGED**, not when
///   it changes — offset 16's `ifne 26` jumps *over* the send. Sleeping twice
///   in the same bed says "Respawn point set" once.
/// * **`isSamePosition` compares only the `GlobalPos`**, so re-sleeping in a
///   bed whose facing changed under you is still "same" and still silent; the
///   yaw and pitch are not part of the comparison even though they are part
///   of the record.
/// * **`copyRespawnPosition` passes `sendMessage = false`** (offset 5,
///   `iconst_0`), which is why a respawn does not tell the player their
///   spawn point was set every time they die.
///
/// The message key is `block.minecraft.set_spawn`, a constant-pool string in
/// `ServerPlayer` backing the static `SPAWN_SET_MESSAGE`. **It is a
/// `system_chat`, not part of the respawn sequence** — it is sent when the
/// spawn point is *set*, which is a bed or `/spawnpoint`, and never by
/// `PlayerList.respawn`.
pub const SPAWN_SET_MESSAGE_KEY: &str = "block.minecraft.set_spawn";

/// Whether `setRespawnPosition` sends `SPAWN_SET_MESSAGE`.
#[must_use]
pub fn announces_spawn_set(
    send_message: bool,
    new_config_present: bool,
    same_global_pos_as_current: bool,
) -> bool {
    send_message && new_config_present && !same_global_pos_as_current
}

/// `RespawnConfig.getDimensionOrDefault` — a **null** config answers
/// `Level.OVERWORLD` rather than refusing.
///
/// ```text
///   getDimensionOrDefault(RespawnConfig cfg)
///      0: cfg == null  ifnull 14
///      4: cfg.respawnData().dimension()
///     14: Level.OVERWORLD
/// ```
///
/// This is why `findRespawnPositionAndUseSpawnBlock` still has a **second**
/// null check on the config at its offset 23: the level lookup at offset 13
/// succeeds for a player with no respawn config at all, so a lookup that
/// returned non-null proves nothing about the config. Dropping either check
/// is a null dereference or a wrong world.
#[must_use]
pub fn respawn_dimension_or_default(configured: Option<&str>) -> &str {
    configured.unwrap_or("minecraft:overworld")
}

#[cfg(test)]
mod spawn_set_tests {
    use super::*;

    /// The message fires on a *change*, and the same bed twice is silent.
    #[test]
    fn the_spawn_set_message_is_suppressed_when_the_position_is_unchanged() {
        assert!(
            announces_spawn_set(true, true, false),
            "a new bed announces"
        );
        assert!(
            !announces_spawn_set(true, true, true),
            "the same bed twice is silent"
        );
        assert!(
            !announces_spawn_set(true, false, false),
            "clearing is silent"
        );
    }

    /// `copyRespawnPosition` passes false, which is why dying does not
    /// announce a spawn point every time.
    #[test]
    fn copying_a_respawn_position_never_announces() {
        for present in [false, true] {
            for same in [false, true] {
                assert!(!announces_spawn_set(false, present, same));
            }
        }
    }

    /// A null config answers OVERWORLD rather than refusing, which is what
    /// makes the second null check in findRespawnPositionAndUseSpawnBlock
    /// necessary rather than redundant.
    #[test]
    fn a_missing_respawn_config_still_names_the_overworld() {
        assert_eq!(respawn_dimension_or_default(None), "minecraft:overworld");
        assert_eq!(
            respawn_dimension_or_default(Some("minecraft:the_nether")),
            "minecraft:the_nether"
        );
        // And the outcome ladder still takes the silent arm for it: the level
        // resolves, the config does not.
        assert_eq!(
            respawn_outcome(RespawnQuery {
                has_config: false,
                dimension_exists: true,
                block: RespawnBlock::Bed,
                forced: false,
                use_charge: true,
                can_set_spawn_here: true,
                stand_up_position_found: true,
                saved_and_above_are_respawnable: true,
            }),
            RespawnOutcome::WorldSpawnSilently
        );
    }

    #[test]
    fn the_spawn_set_message_key_is_a_block_key_not_a_command_one() {
        assert_eq!(SPAWN_SET_MESSAGE_KEY, "block.minecraft.set_spawn");
    }
}

/// `ServerPlayer.createCommonSpawnInfo(ServerLevel)` — where each of the ten
/// fields comes from.
///
/// ```text
///   ServerPlayer.createCommonSpawnInfo(ServerLevel lvl)
///      4: lvl.dimensionTypeRegistration()
///      8: lvl.dimension()
///     12: BiomeManager.obfuscateSeed(lvl.getSeed())
///     19: gameMode.getGameModeForPlayer()
///     26: gameMode.getPreviousGameModeForPlayer()
///     33: lvl.isDebug()
///     37: lvl.isFlat()
///     41: getLastDeathLocation()
///     45: getPortalCooldown()
///     49: lvl.getSeaLevel()
/// ```
///
/// Three of those are not what a join packet's copy holds, and they are the
/// whole difference between the two uses of this record:
///
/// * **`lastDeathLocation` is present on a death respawn and absent on a
///   fresh join.** `ServerPlayer.die` sets it at offset 358 and
///   `restoreFrom` copies it unconditionally at offset 319, so by the time
///   `PlayerList.respawn` calls this the new player already has one. A port
///   that hardcodes the `Optional` absent — as `crate::play::JoinInfo` does,
///   correctly, for a join — sends a respawn packet 20-plus bytes short.
/// * **`previousGameType` is the pre-death one**, not `-1`:
///   `restoreFrom` replays both game types through `setGameModeForPlayer` at
///   offsets 16..34 before this is called.
/// * **the seed is `obfuscateSeed`d**, which this tree already has as
///   [`rustmc_worldgen::biome::manager::obfuscate_seed`] with its own golden
///   test. It is not the world seed, and sending the world seed hands every
///   client the thing the `/seed` permission exists to gate.
///
/// The table below is that list as a thing a test can walk rather than a
/// paragraph.
#[must_use]
pub fn spawn_info_sources() -> [(&'static str, &'static str); 10] {
    [
        ("dimensionType", "lvl.dimensionTypeRegistration()"),
        ("dimension", "lvl.dimension()"),
        ("seed", "BiomeManager.obfuscateSeed(lvl.getSeed())"),
        ("gameType", "gameMode.getGameModeForPlayer()"),
        (
            "previousGameType",
            "gameMode.getPreviousGameModeForPlayer()",
        ),
        ("isDebug", "lvl.isDebug()"),
        ("isFlat", "lvl.isFlat()"),
        ("lastDeathLocation", "getLastDeathLocation()"),
        ("portalCooldown", "getPortalCooldown()"),
        ("seaLevel", "lvl.getSeaLevel()"),
    ]
}

/// The seed the client is told: `BiomeManager.obfuscateSeed(level.getSeed())`
/// at offset 16.
///
/// Not the world seed. Sending the world seed hands every client the thing
/// `/seed`'s permission level exists to gate. This tree already has the
/// function, with its own golden test, so this is a one-line delegation and
/// deliberately not a second implementation.
#[must_use]
pub fn spawn_info_seed(world_seed: i64) -> i64 {
    rustmc_worldgen::biome::manager::obfuscate_seed(world_seed)
}

#[cfg(test)]
mod spawn_info_tests {
    use super::*;

    fn info(previous_game_type: i8, last: Option<(String, i32, i32, i32)>) -> CommonSpawnInfo {
        CommonSpawnInfo {
            dimension_type: 0,
            dimension: "minecraft:overworld".into(),
            seed: spawn_info_seed(1),
            game_type: 1,
            previous_game_type,
            is_debug: false,
            is_flat: false,
            last_death_location: last,
            portal_cooldown: 0,
            sea_level: 63,
        }
    }

    /// Ten fields, ten named sources, and no duplicates — the list is the
    /// thing that keeps a respawn packet from being built out of a join
    /// packet's defaults.
    #[test]
    fn the_spawn_info_has_ten_fields_from_ten_distinct_sources() {
        let t = spawn_info_sources();
        assert_eq!(t.len(), 10);
        let mut srcs: Vec<&str> = t.iter().map(|(_, s)| *s).collect();
        srcs.sort_unstable();
        srcs.dedup();
        assert_eq!(srcs.len(), 10);
    }

    /// The seed on the wire is the obfuscated one, and it is not the world
    /// seed — which is the point of the field.
    #[test]
    fn the_spawn_info_seed_is_obfuscated_not_the_world_seed() {
        assert_ne!(spawn_info_seed(20_260_722), 20_260_722);
        assert_eq!(
            spawn_info_seed(20_260_722),
            rustmc_worldgen::biome::manager::obfuscate_seed(20_260_722)
        );
    }

    /// A respawn's spawn info carries a death location where a join's does
    /// not, and the packet is longer by exactly that.
    #[test]
    fn a_respawns_spawn_info_is_longer_than_a_joins_by_its_death_location() {
        let a = encode_respawn(&info(-1, None), false).expect("encode");
        let b = encode_respawn(
            &info(0, Some(("minecraft:overworld".into(), 100, 64, -200))),
            false,
        )
        .expect("encode");
        assert!(
            b.len() > a.len() + 20,
            "a present GlobalPos costs a name and eight bytes: {} vs {}",
            a.len(),
            b.len()
        );
        // The position survives the packing round trip. Trailing bytes:
        // 8 packed pos, then portalCooldown, seaLevel, dataToKeep.
        assert_eq!(
            packed_block_pos(100, 64, -200),
            i64::from_be_bytes(b[b.len() - 11..b.len() - 3].try_into().expect("8 bytes"))
        );
    }

    /// `previousGameType` is the pre-death one on a respawn, so it is a
    /// different byte from the join packet's habitual -1.
    #[test]
    fn the_previous_game_type_is_carried_across_a_death() {
        let b = info(0, None).encode().expect("encode");
        // dimension_type varint, length byte, 19-byte name, 8-byte seed.
        let at = 1 + 1 + 19 + 8;
        assert_eq!(b[at], 1, "gameType");
        assert_eq!(b[at + 1], 0, "previousGameType, not 0xff");
    }
}
