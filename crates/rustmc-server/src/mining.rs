//! Survival block breaking: `ServerPlayerGameMode`'s dig state machine and
//! the arithmetic that times it.
//!
//! Every constant and branch below is from `javap -p -c -constants` on the
//! pinned 26.2 server jar. The offsets cited are that listing's.
//!
//! # The three packets and the one tick
//!
//! A survival client digging a block sends `START_DESTROY_BLOCK`, then —
//! when its *own* progress reaches one — `STOP_DESTROY_BLOCK`, or
//! `ABORT_DESTROY_BLOCK` if it looks away. The server keeps its own clock and
//! does not believe the client:
//!
//! ```text
//!   handleBlockBreakAction(pos, action, dir, maxY, seq)
//!      0-22   !isWithinBlockInteractionRange(pos, 1.0)       -> return
//!     23-68   pos.y > maxY  -> send block_update(pos)         -> return
//!   START (73):
//!     76-170  spawn protection, mayInteract                   (not modelled)
//!    171-194  abilities.instabuild -> destroyAndAck           -> return
//!    195-251  blockActionRestricted (adventure)               (not modelled)
//!    252-257  destroyProgressStart = gameTicks
//!    260-349  f = 1.0; if !air: onHitBlock; state.attack; f = getDestroyProgress
//!    351-376  !air && f >= 1  -> destroyAndAck("insta mine")
//!    379-426  else: if isDestroyingBlock -> send block_update(destroyPos)
//!    429-481       isDestroyingBlock = true; destroyPos = pos;
//!                  destroyBlockProgress(id, pos, (int)(f*10)); lastSentState = that
//!   STOP (487):
//!    494-502  pos.equals(destroyPos) else -> nothing
//!    505-564  ticks = gameTicks - destroyProgressStart;
//!             !air && getDestroyProgress * (ticks + 1) >= 0.7F:
//!    568-599    isDestroyingBlock = false; progress(-1); destroyAndAck; return
//!    600-627  else if !hasDelayedDestroy: isDestroyingBlock = false;
//!               hasDelayedDestroy = true; delayedDestroyPos = pos;
//!               delayedTickStart = destroyProgressStart
//!   ABORT (644):
//!    651-653  isDestroyingBlock = false
//!    656-699  destroyPos != pos -> warn; progress(-1) at destroyPos
//!    713-726  progress(-1) at pos
//!
//!   tick()
//!      0-7    gameTicks++
//!     10-78   hasDelayedDestroy: air -> clear; else
//!               incrementDestroyProgress(state, delayedPos, delayedTickStart) >= 1
//!                 -> hasDelayedDestroy = false; destroyBlock(delayedPos)
//!     81-152  else isDestroyingBlock: air -> progress(-1), lastSentState = -1,
//!               isDestroyingBlock = false; else incrementDestroyProgress(...)
//!
//!   incrementDestroyProgress(state, pos, start)
//!      0-30   f = getDestroyProgress * (gameTicks - start + 1)
//!     32-69   i = (int)(f * 10); i != lastSentState -> progress(i); lastSentState = i
//! ```
//!
//! # Four things a reading gets wrong, each one a recorded divergence
//!
//! * **The tick never destroys a block a client is still digging.** The
//!   `isDestroyingBlock` arm only *displays* progress — it has no threshold
//!   and no `destroyBlock`. The stage keeps climbing past 9 with no cap;
//!   `002-mining-a-block-reports-its-progress` recorded stage **48** on the
//!   observer's stream on 2026-09-30 for grass dug bare-handed for 4.3 s,
//!   with the block still standing.
//! * **An early STOP breaks the block later, not never.** Below 0.7 the STOP
//!   arms `hasDelayedDestroy`, and the tick destroys the block when the
//!   server's own clock reaches 1.0 — `004` recorded the drop and the
//!   `block_update` ~750 ms after its STOP. The ack for the STOP goes out at
//!   once and the block goes later: the order on the actor's stream is ack
//!   then update, the reverse of an on-time dig.
//! * **Progress never reaches the digger.** `ServerLevel.destroyBlockProgress`
//!   offsets 48-57 skip the player whose id is the breaker's, then send to
//!   everyone else within `dx²+dy²+dz² < 1024`, measured from the block's
//!   *corner* (`pos.getX()`, no `+0.5`) to the listener's feet.
//! * **A dig left unfinished poisons the next START.** `isDestroyingBlock`
//!   stays true until a STOP, an ABORT, or the block turning to air, so the
//!   next START anywhere sends a `block_update` for the old position first
//!   (offsets 379-426). The corpus shares one session across scenarios, and
//!   `003`'s recording opens with exactly that packet for `002`'s block.
//!
//! # What is not here
//!
//! Spawn protection and `mayInteract` (a harness with no operators has
//! neither — `DedicatedServer.isUnderSpawnProtection` offsets 21-34 return
//! false on an empty op list; past that it needs `spawn-protection` from
//! `server.properties` and the level's respawn position, and this server
//! reads the one from its command line and places the other by a
//! placeholder), and adventure mode. Haste, conduit power and mining fatigue
//! enter [`destroy_speed`] as inputs; this server has no effect store on
//! `main`, so the caller passes none.
//!
//! `EnchantmentHelper.onHitBlock` at START (offsets 260-349) is absent and
//! that is exact, not a gap: the one `minecraft:hit_block` effect in 26.2's
//! datapack is Channeling's, whose requirements include `entity_properties
//! this = minecraft:trident`, which a digging player never is. The three
//! `BlockState.attack` overrides — `NoteBlock`, `RedStoneOreBlock`,
//! `DragonEggBlock` — are `crate::dig`'s.

use std::collections::HashMap;
use std::io;
use std::sync::OnceLock;

use crate::interact::BlockPos;

/// Where the per-block facts live. See `ci/mining-golden.sh`.
pub const TABLE_PATH: &str = "data/vanilla/block_mining.json";

/// `ServerPlayerGameMode.handleBlockBreakAction` offset 561: `ldc 0.7f`.
pub const STOP_THRESHOLD: f32 = 0.7;

/// `ServerLevel.destroyBlockProgress` offset 116: `ldc2_w 1024.0d` — the
/// square of the 32-block radius a crack overlay reaches.
pub const PROGRESS_RANGE_SQR: f64 = 1024.0;

/// `Attributes.BLOCK_INTERACTION_RANGE`'s default, and the `1.0` buffer
/// `handleBlockBreakAction` offset 5 passes to `isWithinBlockInteractionRange`.
pub const INTERACTION_BUFFER: f64 = 1.0;

/// One block's row of `data/vanilla/block_mining.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// `BlockState.getDestroySpeed` — the destroy time. `-1` is unbreakable,
    /// `0` breaks on the first hit.
    pub destroy_time: f32,
    /// `BlockState.requiresCorrectToolForDrops`.
    pub requires_correct_tool: bool,
    /// `block instanceof GameMasterBlock`.
    pub game_master: bool,
    /// The class declaring `playerWillDestroy`, when it is not `Block`'s own.
    pub will_destroy: Option<String>,
    /// The class declaring `playerDestroy`, when it is not `Block`'s own.
    pub player_destroy: Option<String>,
    /// The class declaring `spawnAfterBreak`, when it is not the default.
    pub spawn_after_break: Option<String>,
}

/// The whole table, keyed by block id.
#[derive(Debug, Clone, Default)]
pub struct BlockMining {
    rows: HashMap<String, Row>,
}

impl BlockMining {
    /// Parse the table at `path`.
    ///
    /// # Errors
    /// If the file is missing or is not the shape `MiningGolden` writes. A
    /// row without a `destroy_time` is an error rather than a default: every
    /// block vanilla registers has one, and a default would be a silently
    /// wrong dig time for every block the extract failed on.
    pub fn load(path: &std::path::Path) -> io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let doc: serde_json::Value = serde_json::from_str(&text).map_err(io::Error::other)?;
        Self::parse(&doc)
    }

    /// Parse an already-read document.
    ///
    /// # Errors
    /// See [`Self::load`].
    pub fn parse(doc: &serde_json::Value) -> io::Result<Self> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
        let table = doc
            .get("table")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| bad("block_mining: no table"))?;
        let mut rows = HashMap::with_capacity(table.len());
        for r in table {
            let block = r
                .get("block")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| bad("block_mining: row without a block"))?;
            #[allow(clippy::cast_possible_truncation)]
            let destroy_time = r
                .get("destroy_time")
                .and_then(serde_json::Value::as_f64)
                .ok_or_else(|| bad("block_mining: row without a destroy_time"))?
                as f32;
            let flag = |k: &str| {
                r.get(k)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            };
            let class = |k: &str| {
                r.get(k)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            };
            rows.insert(
                block.to_string(),
                Row {
                    destroy_time,
                    requires_correct_tool: flag("requires_correct_tool"),
                    game_master: flag("game_master"),
                    will_destroy: class("will_destroy"),
                    player_destroy: class("player_destroy"),
                    spawn_after_break: class("spawn_after_break"),
                },
            );
        }
        if rows.is_empty() {
            return Err(bad("block_mining: empty table"));
        }
        Ok(Self { rows })
    }

    /// The process-wide table, loaded once from [`TABLE_PATH`].
    ///
    /// `None` on a checkout with no extract. The caller then treats every
    /// block as vanilla's `Properties` default — destroy time 0, no tool
    /// needed — which is exactly the creative behaviour this server had
    /// before the table existed, and is named at the call site.
    #[must_use]
    pub fn shared() -> Option<&'static Self> {
        static TABLE: OnceLock<Option<BlockMining>> = OnceLock::new();
        TABLE
            .get_or_init(|| {
                Self::load(std::path::Path::new(TABLE_PATH))
                    .map_err(|e| eprintln!("no block mining table ({e}) — survival digging is off"))
                    .ok()
            })
            .as_ref()
    }

    /// The row for a block id (`minecraft:stone`), or for a full state
    /// string, whose properties are ignored — the table is per block.
    #[must_use]
    pub fn row(&self, block_or_state: &str) -> Option<&Row> {
        let block = block_or_state.split('[').next().unwrap_or(block_or_state);
        self.rows.get(block)
    }

    /// How many blocks the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table is empty (never true for a loaded one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// What `Player.getDestroySpeed(state)` reads, one field per load.
///
/// The defaults are an unenchanted, effect-free player standing on the
/// ground in air, holding something with no `minecraft:tool` component —
/// `Default::default()` is bare hands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Digger {
    /// `ItemStack.getDestroySpeed(state)` — the held tool's speed for this
    /// block, 1.0 for anything without a matching `tool` rule.
    pub tool_speed: f32,
    /// `Attributes.MINING_EFFICIENCY` — Efficiency's `levels_squared + 1`,
    /// added only when `tool_speed > 1`.
    pub mining_efficiency: f64,
    /// `MobEffectUtil.getDigSpeedAmplification` when `hasDigSpeed` — the
    /// larger of Haste's and Conduit Power's amplifier. `None` without either.
    pub dig_speed_amplifier: Option<i32>,
    /// Mining Fatigue's amplifier, `None` without it.
    pub fatigue_amplifier: Option<i32>,
    /// `Attributes.BLOCK_BREAK_SPEED`, base 1.0.
    pub block_break_speed: f64,
    /// `isEyeInFluid(FluidTags.WATER)`.
    pub eye_in_water: bool,
    /// `Attributes.SUBMERGED_MINING_SPEED`, base 0.2; Aqua Affinity adds.
    pub submerged_mining_speed: f64,
    /// `Entity.onGround()`.
    pub on_ground: bool,
    /// `ItemStack.isCorrectToolForDrops(state)` for the held stack.
    pub item_correct_for_drops: bool,
}

impl Default for Digger {
    fn default() -> Self {
        Self {
            tool_speed: 1.0,
            mining_efficiency: 0.0,
            dig_speed_amplifier: None,
            fatigue_amplifier: None,
            block_break_speed: 1.0,
            eye_in_water: false,
            submerged_mining_speed: 0.2,
            on_ground: true,
            item_correct_for_drops: false,
        }
    }
}

/// `Player.getDestroySpeed(state)`, offsets 0-175, in `float` exactly where
/// the listing is in `float`.
///
/// Every attribute is a `double` read with `d2f` before it touches the
/// accumulator (offsets 26, 134, 158), so the arithmetic is single precision
/// throughout and done in the listing's order. A port in `f64` that rounded
/// once at the end would disagree in the last bit for most tools, and the
/// last bit decides which tick a threshold is crossed on.
#[must_use]
pub fn destroy_speed(d: &Digger) -> f32 {
    let mut f = d.tool_speed;
    // 12-28: `fload_2; fconst_1; fcmpl; ifle` — strictly greater than one.
    if f > 1.0 {
        #[allow(clippy::cast_possible_truncation)]
        let add = d.mining_efficiency as f32;
        f += add;
    }
    // 29-51: `f *= 1.0F + (float)(amp + 1) * 0.2F`.
    if let Some(amp) = d.dig_speed_amplifier {
        #[allow(clippy::cast_precision_loss)]
        let level = amp.wrapping_add(1) as f32;
        f *= 1.0 + level * 0.2;
    }
    // 52-125: the tableswitch. Amplifier 3 and above, and a negative one,
    // take the default arm — `8.1E-4F`.
    if let Some(amp) = d.fatigue_amplifier {
        let scale: f32 = match amp {
            0 => 0.3,
            1 => 0.09,
            2 => 0.0027,
            _ => 8.1e-4,
        };
        f *= scale;
    }
    // 126-136.
    #[allow(clippy::cast_possible_truncation)]
    let break_speed = d.block_break_speed as f32;
    f *= break_speed;
    // 137-160.
    if d.eye_in_water {
        #[allow(clippy::cast_possible_truncation)]
        let submerged = d.submerged_mining_speed as f32;
        f *= submerged;
    }
    // 161-173: `f / 5.0F`, a division and not a multiplication by 0.2 —
    // the two differ in the last bit.
    if !d.on_ground {
        f /= 5.0;
    }
    f
}

/// `Player.hasCorrectToolForDrops(state)`, offsets 0-26: the block's own
/// flag is read first, so a block that does not require a tool is always
/// "correct" whatever is held.
#[must_use]
pub fn has_correct_tool_for_drops(row: &Row, item_correct: bool) -> bool {
    !row.requires_correct_tool || item_correct
}

/// Which `Block.playerDestroy` a block runs, read off the class
/// `block_mining.json` names as declaring it.
///
/// Four classes override it in 26.2 (`MiningGolden`'s census, 15 blocks):
/// `DoublePlantBlock` (10), `IceBlock` (`ice`, and `frosted_ice` through
/// `FrostedIceBlock extends IceBlock`), `BeehiveBlock` (2) and
/// `TurtleEggBlock` (1). Dispatching on the declaring class rather than on
/// block names is what keeps `frosted_ice` in the ice arm without a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerDestroy {
    /// `Block.playerDestroy`: `awardStat`, `causeFoodExhaustion(0.005F)` and
    /// `dropResources(state, level, pos, be, player, tool)`.
    Default,
    /// `DoublePlantBlock.playerDestroy`, offsets 0-17: `super.playerDestroy`
    /// with `Blocks.AIR.defaultBlockState()` in place of the state, so the
    /// roll is the air table's — empty, and no draw. A survival double plant
    /// drops from `playerWillDestroy` instead (see [`will_destroy_drops`]).
    DoublePlant,
    /// `IceBlock.playerDestroy` — see [`ice_after_destroy`].
    Ice,
    /// `TurtleEggBlock.playerDestroy` — `super`, then `decreaseEggs`. See
    /// [`turtle_eggs_after_destroy`].
    TurtleEgg,
    /// `BeehiveBlock.playerDestroy`: `super`, then the hive releases its
    /// occupants and angers bees nearby. This server has no bees, so an
    /// occupant-free hive is all it can hold and the tail does nothing a
    /// packet carries; the default's drop is the whole of it.
    Beehive,
}

const BLOCK_PKG: &str = "net.minecraft.world.level.block.";

/// [`PlayerDestroy`] for a block's row; a block with no row runs `Block`'s.
#[must_use]
pub fn player_destroy(row: Option<&Row>) -> PlayerDestroy {
    match row
        .and_then(|r| r.player_destroy.as_deref())
        .and_then(|c| c.strip_prefix(BLOCK_PKG))
    {
        Some("DoublePlantBlock") => PlayerDestroy::DoublePlant,
        Some("IceBlock") => PlayerDestroy::Ice,
        Some("TurtleEggBlock") => PlayerDestroy::TurtleEgg,
        Some("BeehiveBlock") => PlayerDestroy::Beehive,
        _ => PlayerDestroy::Default,
    }
}

/// The class declaring a block's `playerWillDestroy`, package stripped, when
/// it is not `Block`'s own — `DoorBlock`, `BaseFireBlock`, `PistonHeadBlock`
/// and eight more in 26.2.
#[must_use]
pub fn will_destroy_class(row: Option<&Row>) -> Option<&str> {
    row.and_then(|r| r.will_destroy.as_deref())
        .and_then(|c| c.strip_prefix(BLOCK_PKG))
}

/// Whether `playerWillDestroy` itself rolls the block's drops — the one
/// override that does is `DoublePlantBlock`'s, offsets 0-40:
///
/// ```text
///   if (!level.isClientSide())
///       if (player.preventsBlockDrops()) preventDropFromBottomPart(...)
///       else dropResources(state, level, pos, null, player, player.getMainHandItem())
///   return super.playerWillDestroy(...)          <- spawnDestroyParticles (2001)
/// ```
///
/// So a survival double plant drops **before** its `2001` and before
/// `removeBlock`, and **ungated** by `hasCorrectToolForDrops` — the gate at
/// `ServerPlayerGameMode.destroyBlock` 244 guards `playerDestroy`, which for
/// this class rolls air.
#[must_use]
pub fn will_destroy_drops(row: Option<&Row>, prevents_block_drops: bool) -> bool {
    !prevents_block_drops && will_destroy_class(row) == Some("DoublePlantBlock")
}

/// What `IceBlock.playerDestroy` leaves at the dug position, after its
/// `super.playerDestroy` drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IceAfter {
    /// Offsets 21 or 75: the tool prevents melting, or nothing below holds
    /// water — the position stays what `removeBlock` left.
    Nothing,
    /// Offsets 44-51: `WATER_EVAPORATES` here — `level.removeBlock(pos, false)`.
    Remove,
    /// Offsets 78-86: `level.setBlockAndUpdate(pos, IceBlock.meltsInto())`,
    /// which is `Blocks.WATER.defaultBlockState()`.
    Water,
}

/// `IceBlock.playerDestroy`, offsets 13-87:
///
/// ```text
///   if (EnchantmentHelper.hasTag(tool, PREVENTS_ICE_MELTING)) return
///   if (level.environmentAttributes().getValue(WATER_EVAPORATES, pos))
///       { level.removeBlock(pos, false); return }
///   below = level.getBlockState(pos.below())
///   if (below.blocksMotion() || below.liquid())
///       level.setBlockAndUpdate(pos, meltsInto())
/// ```
#[must_use]
pub fn ice_after_destroy(
    prevents_melting: bool,
    water_evaporates: bool,
    below_blocks_motion: bool,
    below_liquid: bool,
) -> IceAfter {
    if prevents_melting {
        IceAfter::Nothing
    } else if water_evaporates {
        IceAfter::Remove
    } else if below_blocks_motion || below_liquid {
        IceAfter::Water
    } else {
        IceAfter::Nothing
    }
}

/// `BlockState.liquid()` — the `liquid` flag `BlockBehaviour.Properties.liquid()`
/// sets. `Blocks.<clinit>` calls it three times, for `WATER`, `LAVA` and
/// `BUBBLE_COLUMN` (`javap -p -c -constants`, the `putstatic` following each
/// `Properties.liquid` call), and on nothing else.
#[must_use]
pub fn is_liquid_block(block: &str) -> bool {
    matches!(
        block,
        "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column"
    )
}

/// `TurtleEggBlock.decreaseEggs`, offsets 30-108, given the dug state's
/// `eggs`: `None` is `eggs <= 1` -> `level.destroyBlock(pos, false)`, which
/// on the air `removeBlock` already left is a no-op; `Some(n)` is
/// `setBlock(pos, state.setValue(EGGS, n), 2)` followed by
/// `levelEvent(2001, pos, Block.getId(state))` — the three-argument overload,
/// so the digger is told, and the id is the **dug** state's.
#[must_use]
pub fn turtle_eggs_after_destroy(eggs: i32) -> Option<i32> {
    (eggs > 1).then_some(eggs - 1)
}

/// `TurtleEggBlock.decreaseEggs`, offsets 0-26: `TURTLE_EGG_BREAK` at
/// volume 0.7, pitch `0.9F + random.nextFloat() * 0.2F`.
pub const TURTLE_EGG_BREAK_VOLUME: f32 = 0.7;

/// `DragonEggBlock.teleport`, offsets 0-356, server side: up to a thousand
/// tries at `pos.offset(nextInt(16) - nextInt(16), nextInt(8) - nextInt(8),
/// nextInt(16) - nextInt(16))` — six draws a try, x then y then z, each
/// pair's first draw the minuend — for a position that `fits`: air, not over
/// air, inside the world border and inside the build height (offsets 89-129,
/// in that order, which the caller's predicate must keep). `None` after a
/// thousand misses, and the egg stays.
pub fn dragon_egg_target(
    pos: BlockPos,
    random: &mut impl rustmc_worldgen::rng::RandomSource,
    mut fits: impl FnMut(BlockPos) -> bool,
) -> Option<BlockPos> {
    for _ in 0..1000 {
        let dx = random.next_i32_bound(16) - random.next_i32_bound(16);
        let dy = random.next_i32_bound(8) - random.next_i32_bound(8);
        let dz = random.next_i32_bound(16) - random.next_i32_bound(16);
        let target = (pos.0 + dx, pos.1 + dy, pos.2 + dz);
        if fits(target) {
            return Some(target);
        }
    }
    None
}

/// `BlockBehaviour.getDestroyProgress(state, player, level, pos)`,
/// offsets 0-49 — one tick's worth of progress.
///
/// ```text
///   f = state.getDestroySpeed(level, pos)
///   if (f == -1.0F) return 0.0F
///   i = player.hasCorrectToolForDrops(state) ? 30 : 100
///   return player.getDestroySpeed(state) / f / (float) i
/// ```
///
/// A destroy time of zero divides by zero, and that is the mechanism: the
/// result is `+Infinity` (or `NaN` for a zero speed), and `+Infinity >= 1`
/// is what makes a torch break on the first hit.
#[must_use]
pub fn destroy_progress(row: &Row, d: &Digger) -> f32 {
    let f = row.destroy_time;
    if f == -1.0 {
        return 0.0;
    }
    let i: f32 = if has_correct_tool_for_drops(row, d.item_correct_for_drops) {
        30.0
    } else {
        100.0
    };
    destroy_speed(d) / f / i
}

/// `(int)(f * 10.0F)` — Java's `f2i`: truncation toward zero, `NaN` to 0,
/// and saturation at the `int` range. Rust's `as` has had exactly those
/// semantics since 1.45, which is why this is a cast and not a helper with
/// a branch per case.
#[must_use]
pub fn stage(progress: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let i = (progress * 10.0) as i32;
    i
}

/// `ServerboundPlayerActionPacket.Action`'s three digging arms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dig {
    /// `START_DESTROY_BLOCK`.
    Start,
    /// `ABORT_DESTROY_BLOCK`.
    Abort,
    /// `STOP_DESTROY_BLOCK`.
    Stop,
}

/// What the state machine asks its caller to do, in the order it asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `connection.send(new ClientboundBlockUpdatePacket(pos, level.getBlockState(pos)))`
    /// — to the digger only, carrying whatever is at `pos` now.
    ResendBlock(BlockPos),
    /// `level.destroyBlockProgress(player.getId(), pos, stage)` — to everyone
    /// else within 32 blocks. `-1` clears the overlay.
    Progress(BlockPos, i32),
    /// `state.attack(level, pos, player)` — START offsets 318-329, before the
    /// progress is read. Three blocks override it: a note block plays, a
    /// redstone ore lights, a dragon egg teleports.
    Attack(BlockPos),
    /// `destroyAndAck(pos, seq, why)`: `destroyBlock(pos)`, and on `false`
    /// a [`Effect::ResendBlock`] of `pos`. The ack itself is
    /// `handlePlayerAction`'s, after this returns, and is not an effect.
    DestroyAndAck(BlockPos),
    /// `destroyBlock(pos)` from the tick — no resend on failure.
    Destroy(BlockPos),
}

/// What the machine reads from the world and the player, supplied by the
/// caller so the machine itself stays a pure function of its fields.
pub trait World {
    /// Whether the block at `pos` is air (`BlockState.isAir`).
    fn is_air(&self, pos: BlockPos) -> bool;
    /// `state.getDestroyProgress(player, level, pos)` for the block at `pos`.
    fn progress(&self, pos: BlockPos) -> f32;
}

/// `ServerPlayerGameMode`'s dig fields, with the constructor's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameMode {
    /// `isDestroyingBlock`.
    pub is_destroying_block: bool,
    /// `destroyProgressStart`.
    pub destroy_progress_start: i32,
    /// `destroyPos` — `BlockPos.ZERO` at construction (offset 12).
    pub destroy_pos: BlockPos,
    /// `gameTicks` — this player's own tick counter, not the level's.
    pub game_ticks: i32,
    /// `hasDelayedDestroy`.
    pub has_delayed_destroy: bool,
    /// `delayedDestroyPos` — `BlockPos.ZERO` at construction (offset 19).
    pub delayed_destroy_pos: BlockPos,
    /// `delayedTickStart`.
    pub delayed_tick_start: i32,
    /// `lastSentState` — `-1` at construction (offset 26).
    pub last_sent_state: i32,
}

impl Default for GameMode {
    fn default() -> Self {
        Self {
            is_destroying_block: false,
            destroy_progress_start: 0,
            destroy_pos: (0, 0, 0),
            game_ticks: 0,
            has_delayed_destroy: false,
            delayed_destroy_pos: (0, 0, 0),
            delayed_tick_start: 0,
            last_sent_state: -1,
        }
    }
}

impl GameMode {
    /// A fresh player's.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `handleBlockBreakAction` from offset 69 — after the range and height
    /// refusals, which the caller applies because they need the player's
    /// position and the level's height.
    ///
    /// `instabuild` is `Abilities.instabuild`: a creative player's START is
    /// `destroyAndAck` and nothing else, which is why no creative dig ever
    /// touches a field here.
    pub fn handle(
        &mut self,
        action: Dig,
        pos: BlockPos,
        instabuild: bool,
        world: &dyn World,
    ) -> Vec<Effect> {
        let mut out = Vec::new();
        match action {
            Dig::Start => {
                if instabuild {
                    out.push(Effect::DestroyAndAck(pos));
                    return out;
                }
                self.destroy_progress_start = self.game_ticks;
                let air = world.is_air(pos);
                if !air {
                    out.push(Effect::Attack(pos));
                }
                let f = if air { 1.0 } else { world.progress(pos) };
                // 351-376: `!air && f >= 1.0F` — `fcmpl; iflt`, so NaN is
                // not an insta-mine.
                if !air && f >= 1.0 {
                    out.push(Effect::DestroyAndAck(pos));
                    return out;
                }
                if self.is_destroying_block {
                    out.push(Effect::ResendBlock(self.destroy_pos));
                }
                self.is_destroying_block = true;
                self.destroy_pos = pos;
                let s = stage(f);
                out.push(Effect::Progress(pos, s));
                self.last_sent_state = s;
            }
            Dig::Stop => {
                if pos != self.destroy_pos {
                    return out;
                }
                let ticks = self.game_ticks.wrapping_sub(self.destroy_progress_start);
                if world.is_air(pos) {
                    return out;
                }
                #[allow(clippy::cast_precision_loss)]
                let f = world.progress(pos) * ticks.wrapping_add(1) as f32;
                if f >= STOP_THRESHOLD {
                    self.is_destroying_block = false;
                    out.push(Effect::Progress(pos, -1));
                    out.push(Effect::DestroyAndAck(pos));
                    return out;
                }
                if !self.has_delayed_destroy {
                    self.is_destroying_block = false;
                    self.has_delayed_destroy = true;
                    self.delayed_destroy_pos = pos;
                    self.delayed_tick_start = self.destroy_progress_start;
                }
            }
            Dig::Abort => {
                self.is_destroying_block = false;
                if self.destroy_pos != pos {
                    out.push(Effect::Progress(self.destroy_pos, -1));
                }
                out.push(Effect::Progress(pos, -1));
            }
        }
        out
    }

    /// `ServerPlayerGameMode.tick()`, offsets 0-153.
    pub fn tick(&mut self, world: &dyn World) -> Vec<Effect> {
        let mut out = Vec::new();
        self.game_ticks = self.game_ticks.wrapping_add(1);
        if self.has_delayed_destroy {
            let pos = self.delayed_destroy_pos;
            if world.is_air(pos) {
                self.has_delayed_destroy = false;
            } else {
                let f = self.increment(pos, self.delayed_tick_start, world, &mut out);
                if f >= 1.0 {
                    self.has_delayed_destroy = false;
                    out.push(Effect::Destroy(pos));
                }
            }
        } else if self.is_destroying_block {
            let pos = self.destroy_pos;
            if world.is_air(pos) {
                out.push(Effect::Progress(pos, -1));
                self.last_sent_state = -1;
                self.is_destroying_block = false;
            } else {
                self.increment(pos, self.destroy_progress_start, world, &mut out);
            }
        }
        out
    }

    /// `incrementDestroyProgress(state, pos, start)`, offsets 0-74.
    fn increment(
        &mut self,
        pos: BlockPos,
        start: i32,
        world: &dyn World,
        out: &mut Vec<Effect>,
    ) -> f32 {
        let ticks = self.game_ticks.wrapping_sub(start);
        #[allow(clippy::cast_precision_loss)]
        let f = world.progress(pos) * ticks.wrapping_add(1) as f32;
        let s = stage(f);
        if s != self.last_sent_state {
            out.push(Effect::Progress(pos, s));
            self.last_sent_state = s;
        }
        f
    }
}

/// `Player.isWithinBlockInteractionRange(pos, buffer)`, offsets 0-37:
/// `new AABB(pos).distanceToSqr(getEyePosition()) < (range + buffer)²`.
///
/// Written with three separate multiplies and two adds, the `dmul; dmul;
/// dadd; dmul; dadd` of `AABB.distanceToSqr`, and never as a fused
/// multiply-add — see `crate::playerlist::within` for why the rounding is the
/// point.
#[must_use]
pub fn within_interaction_range(pos: BlockPos, eye: (f64, f64, f64), range: f64) -> bool {
    fn axis(min: f64, max: f64, v: f64) -> f64 {
        f64::max(f64::max(min - v, v - max), 0.0)
    }
    let (x, y, z) = (f64::from(pos.0), f64::from(pos.1), f64::from(pos.2));
    let dx = axis(x, x + 1.0, eye.0);
    let dy = axis(y, y + 1.0, eye.1);
    let dz = axis(z, z + 1.0, eye.2);
    let d2 = dx * dx;
    let d2 = d2 + dy * dy;
    let d2 = d2 + dz * dz;
    let r = range + INTERACTION_BUFFER;
    d2 < r * r
}

/// `ClientboundBlockDestructionPacket.write`: `writeVarInt(id)`,
/// `writeBlockPos(pos)`, `writeByte(progress)` — so `-1` is `0xFF`.
///
/// # Errors
/// If writing to the buffer fails.
pub fn block_destruction_body(breaker: i32, pos: BlockPos, stage: i32) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    rustmc_proto::varint::write_varint(&mut out, breaker)?;
    rustmc_proto::wire::write_i64(&mut out, crate::interact::pack_pos(pos))?;
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    rustmc_proto::wire::write_u8(&mut out, stage as u8)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
