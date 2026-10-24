//! The level's entities, and the packets that describe one to a client.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `ClientboundAddEntityPacket`, `ClientboundRemoveEntitiesPacket`,
//! `ClientboundMoveEntityPacket$Pos/$PosRot/$Rot`,
//! `ClientboundRotateHeadPacket`, `net.minecraft.network.LpVec3`,
//! `net.minecraft.network.protocol.game.VecDeltaCodec`, `net.minecraft.util.Mth`
//! and `net.minecraft.world.entity.EntityTypes`.
//!
//! # Why this exists at all
//!
//! Until this module, **no `add_entity` broadcast existed anywhere in the
//! server**. Not a weak one — none. A client was told about the world and
//! about block changes and about nothing else, so a second player was
//! invisible to the first, a dropped item did not appear, and five of the
//! twelve `absent` checks in `ci/checks/gameplay/` were absent for the same
//! single reason. This is the floor that everything entity-shaped stands on.
//!
//! # What is here and what is deliberately not
//!
//! Here: the entity record, the level-wide registry that hands out ids, and
//! the wire encoders. Not here: which players are told about which entity,
//! which is `crate::tracker` (`ChunkMap.TrackedEntity`), and per-kind
//! behaviour, which is not written yet and must come from the bytecode rather
//! than from a plausible guess.

use crate::openhash::IntOpenHashMap;
use rustmc_proto::varint::write_varint;
use rustmc_proto::wire;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};

/// The three per-type numbers `ChunkMap.addEntity` reads, and nothing else.
///
/// `addEntity` consults exactly `clientTrackingRange()`, `updateInterval()`
/// and `trackDeltas()` before constructing a `TrackedEntity`, so those are
/// what a kind has to carry for tracking to be faithful. Everything else
/// about an entity type — its size, its category, its factory — belongs to
/// behaviour that is not written yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityKind {
    /// The registry name, resolved to a numeric id against the same
    /// `minecraft:entity_type` registry the client was sent.
    pub name: &'static str,
    /// `EntityType.clientTrackingRange()`, **in chunks**. `addEntity`
    /// multiplies it by 16 to get the range in blocks, and refuses to track
    /// an entity whose product is zero.
    pub tracking_range_chunks: i32,
    /// `EntityType.updateInterval()`. `sendChanges` runs its body only on
    /// ticks where `tickCount % updateInterval == 0` (or when something is
    /// dirty), so this is the cadence of everything positional.
    pub update_interval: i32,
    /// `EntityType.trackDeltas()`.
    ///
    /// **This is a deny-list method, not a builder field**, which is why it
    /// is stored per kind rather than read from a builder call that does not
    /// exist. From the bytecode it returns `false` for exactly `PLAYER`,
    /// `LLAMA_SPIT`, `WITHER`, `BAT`, `ITEM_FRAME`, `GLOW_ITEM_FRAME`,
    /// `LEASH_KNOT`, `PAINTING`, `END_CRYSTAL` and `EVOKER_FANGS`, and `true`
    /// for every other type.
    pub track_deltas: bool,
}

/// `EntityTypes.PLAYER`.
///
/// `createNothing(MISC).noSave().noSummon().sized(0.6, 1.8).eyeHeight(1.62)
///  .vehicleAttachment(..).clientTrackingRange(32).updateInterval(2)`, and
/// `trackDeltas()` names PLAYER in its false list. So a player is tracked to
/// **512 blocks**, its position is considered every **2** ticks, and its
/// velocity is not tracked.
pub const PLAYER: EntityKind = EntityKind {
    name: "minecraft:player",
    tracking_range_chunks: 32,
    update_interval: 2,
    track_deltas: false,
};

/// `EntityTypes.ITEM`.
///
/// `.noLootTable().sized(0.25, 0.25).eyeHeight(0.2125).clientTrackingRange(6)
///  .updateInterval(20)`, and it is not in `trackDeltas`' false list. So a
/// dropped stack is tracked to **96 blocks** and reconsidered once a second.
///
/// The 20-tick interval is worth saying out loud because it is the thing that
/// makes an item look different from a player on the wire: an item that is
/// still falling sends nothing at all for up to a second at a time.
pub const ITEM: EntityKind = EntityKind {
    name: "minecraft:item",
    tracking_range_chunks: 6,
    update_interval: 20,
    track_deltas: true,
};

/// `EntityTypes.FALLING_BLOCK`.
///
/// `.noLootTable().sized(0.98, 0.98).clientTrackingRange(10)
///  .updateInterval(20)`, read from `EntityTypes`' static initialiser at
/// offsets 2234-2270, and it is not in `trackDeltas`' false list. So a falling
/// block is tracked to **160 blocks** and reconsidered once a second.
///
/// The registry id is **51**, which is worth writing down because it is the
/// one number here that was confirmed twice from different directions: the
/// `minecraft:entity_type` extract answers 51, and scenario `070`'s recorded
/// vanilla `add_entity` carries `kind_id 51`.
///
/// **The 20-tick `updateInterval` is not the cadence of the fall.** A falling
/// block moves every tick and lands in about seven — see
/// [`crate::physics::Profile::FALLING_BLOCK`] — so the interval only decides
/// how often a *positional* packet is considered for one that is still in the
/// air. The two `block_update`s at the landing are not on this clock at all.
pub const FALLING_BLOCK: EntityKind = EntityKind {
    name: "minecraft:falling_block",
    tracking_range_chunks: 10,
    update_interval: 20,
    track_deltas: true,
};

/// One entity in the level.
///
/// Positions are `f64` and rotations are `f32`, which is what vanilla stores;
/// the byte-packed forms only exist on the wire and are produced by
/// [`pack_degrees`] at the moment of sending, never held.
#[derive(Debug, Clone)]
pub struct Entity {
    pub id: i32,
    pub uuid: u128,
    pub kind: EntityKind,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// `Entity.getYRot()` — yaw.
    pub y_rot: f32,
    /// `Entity.getXRot()` — pitch.
    pub x_rot: f32,
    /// `Entity.getYHeadRot()`. Equal to `y_rot` for anything that is not a
    /// `LivingEntity` with an independent head.
    pub y_head_rot: f32,
    /// `Entity.getDeltaMovement()`.
    pub movement: (f64, f64, f64),
    pub on_ground: bool,
    /// `ClientboundAddEntityPacket`'s trailing `data` varint. Zero for
    /// everything reached through the two-argument constructor, which is
    /// every entity this server creates so far.
    pub data: i32,
    /// `LivingEntity.swinging`.
    pub swinging: bool,
    /// `LivingEntity.swingTime`.
    ///
    /// **Starts at `-1` after a swing, not at 0**, and the distinction is the
    /// whole of the swallow: `swing`'s third guard is `swingTime >= 0`, so a
    /// second swing in the *same* tick as the first is let through and one a
    /// tick later is not.
    pub swing_time: i32,
    /// `LivingEntity.swingingArm`, as the hand's ordinal. `None` is the null
    /// the field starts as, which `getCurrentSwingDuration` reads as
    /// `MAIN_HAND`.
    pub swinging_arm: Option<u8>,
    /// `Entity.getEntityData()` — the accessors that are away from the value
    /// they were defined with, which is all a client is ever sent.
    ///
    /// Held on the entity rather than derived at send time because that is
    /// where vanilla holds it: `SynchedEntityData` is a field of `Entity`,
    /// `ServerEntity` only snapshots it, and a value recomputed at send time
    /// could not model an accessor whose default the entity has been set
    /// *back* to.
    pub synched: crate::synched::Synched,
}

/// `SwingAnimation.DEFAULT.duration()` — `bipush 6` in
/// `SwingAnimation`'s static initialiser.
///
/// `getCurrentSwingDuration()` is this, adjusted by haste and mining fatigue.
/// **Neither is modelled and neither can be reached**: this server has no
/// mob effects at all, so `hasDigSpeed` and `hasEffect(MINING_FATIGUE)` are
/// both false and the method reduces to the component's duration. Written as
/// a constant with that reasoning rather than as a method with two dead
/// branches.
pub const DEFAULT_SWING_DURATION: i32 = 6;

/// `LivingEntity.swing(hand, false)`'s decision, offsets 0-42.
///
/// Returns whether an `animate` goes out. `false` is the swallow, and it is
/// the branch worth having a name for: vanilla sends **nothing** for a swing
/// that arrives inside the first half of the animation already running, so a
/// server that echoed every `swing` packet would put packets on the wire that
/// vanilla does not.
///
/// ```text
///    0: swinging          ifeq 27
///    7: swingTime
///   11: getCurrentSwingDuration() / 2
///   17: if_icmpge 27
///   20: swingTime         ifge 106   <- return, no packet
///   27: swingTime = -1; swinging = true; swingingArm = hand
/// ```
///
/// The division is Java integer division of the duration, so at the default 6
/// the half is 3 and `swingTime` 0, 1 and 2 all swallow.
pub fn swing(e: &mut Entity, hand: u8) -> bool {
    let half = DEFAULT_SWING_DURATION / 2;
    if e.swinging && e.swing_time < half && e.swing_time >= 0 {
        return false;
    }
    e.swing_time = -1;
    e.swinging = true;
    e.swinging_arm = Some(hand);
    true
}

/// `LivingEntity.updateSwingTime`, offsets 0-47.
///
/// The `attackAnim` at offsets 48-59 is a client-side render value and is not
/// carried: nothing on a server reads it and nothing puts it on the wire.
pub fn update_swing_time(e: &mut Entity) {
    if e.swinging {
        e.swing_time += 1;
        if e.swing_time >= DEFAULT_SWING_DURATION {
            e.swing_time = 0;
            e.swinging = false;
        }
    } else {
        e.swing_time = 0;
    }
}

/// `ClientboundAnimatePacket.SWING_MAIN_HAND` and `SWING_OFF_HAND`, from the
/// `if_acmpne` at offset 61 of `swing`.
///
/// **0 and 3, not 0 and 1.** The gap holds `WAKE_UP` at 2, and the hand's own
/// ordinal on the serverbound packet *is* 0 and 1 — so the two numbers are
/// adjacent in the code and different on the wire, which is exactly the shape
/// of an off-by-something nobody notices.
#[must_use]
pub const fn animate_action_for_hand(hand: u8) -> u8 {
    if hand == 0 {
        0
    } else {
        3
    }
}

impl Entity {
    /// `Entity.chunkPosition()` — floor division, so block −1 is chunk −1.
    #[must_use]
    pub fn chunk(&self) -> (i32, i32) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a block coordinate inside the world's extent"
        )]
        let (bx, bz) = (self.x.floor() as i32, self.z.floor() as i32);
        (bx >> 4, bz >> 4)
    }

    /// `SectionPos.of(entity)` — the section, which is what `ChunkMap.tick`
    /// compares against `lastSectionPos` to decide whether to re-run
    /// `updatePlayers`.
    #[must_use]
    pub fn section(&self) -> (i32, i32, i32) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a block coordinate inside the world's extent"
        )]
        let by = self.y.floor() as i32;
        let (cx, cz) = self.chunk();
        (cx, by >> 4, cz)
    }
}

/// Every `EntityKind` this server can construct.
///
/// One list rather than a call-site literal, because
/// `the_broadcast_walk_assumes_every_live_entity_is_tracked` has to be able to
/// enumerate them. A kind added to the server and not to this list makes that
/// test pass by not looking, which is why the list is here and not beside a
/// `KindIds::resolve` call.
/// `EntityTypes.PIG`.
///
/// `of(Pig::new, CREATURE).sized(0.9F, 0.9F).passengerAttachments(0.86875F)
///  .clientTrackingRange(10)`; `updateInterval` is the builder's default 3
/// and `trackDeltas()` does not name PIG. Tracked to **160 blocks**,
/// considered every **3** ticks, velocity tracked. The mob itself is
/// `crate::mob::pig`.
pub const PIG: EntityKind = EntityKind {
    name: "minecraft:pig",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.BAT`.
///
/// `of(Bat::new, AMBIENT).sized(0.5F, 0.9F).eyeHeight(0.45F)
///  .clientTrackingRange(5)`; `updateInterval` 3 by default, and
/// `trackDeltas()` names BAT in its false list. Tracked to **80 blocks**.
/// The mob itself is `crate::mob::bat`.
pub const BAT: EntityKind = EntityKind {
    name: "minecraft:bat",
    tracking_range_chunks: 5,
    update_interval: 3,
    track_deltas: false,
};

/// `EntityTypes.EXPERIENCE_ORB`.
///
/// `of(ExperienceOrb::new, MISC).noLootTable().sized(0.5F, 0.5F)
///  .clientTrackingRange(6).updateInterval(20)` — `EntityTypes`' static
/// initialiser, offsets 2151..2188 — and `trackDeltas()` does not name it.
/// So an orb is tracked to **96 blocks**, reconsidered once a second, and its
/// velocity IS tracked, which is the one field of the three that differs from
/// [`ITEM`]'s otherwise identical numbers. The orb itself is
/// [`crate::xporb`].
pub const EXPERIENCE_ORB: EntityKind = EntityKind {
    name: "minecraft:experience_orb",
    tracking_range_chunks: 6,
    update_interval: 20,
    track_deltas: true,
};

/// `EntityTypes.IRON_GOLEM`: `sized(1.4F, 2.7F).clientTrackingRange(10)`,
/// interval 3 by default, deltas tracked. The mob is `crate::mob::golem`.
pub const IRON_GOLEM: EntityKind = EntityKind {
    name: "minecraft:iron_golem",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.COW`: `of(Cow::new, CREATURE).sized(0.9F, 1.4F)
/// .eyeHeight(1.3F).passengerAttachments(1.36875F).clientTrackingRange(10)`;
/// `updateInterval` 3 by default, not in `trackDeltas()`' false list.
pub const COW: EntityKind = EntityKind {
    name: "minecraft:cow",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.SNOW_GOLEM`: `clientTrackingRange(8)`.
pub const SNOW_GOLEM: EntityKind = EntityKind {
    name: "minecraft:snow_golem",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.MINECART`.
///
/// `of(Minecart::new, MISC).noLootTable().sized(0.98F, 0.7F)
///  .passengerAttachments(0.1875F).clientTrackingRange(8)` (offsets
/// 3915-3943 of `EntityTypes.<clinit>`); `updateInterval` is the builder's
/// default 3 and `EntityType.trackDeltas()` does not name it. Tracked to
/// **128 blocks**, considered every **3** ticks, velocity tracked.
pub const MINECART: EntityKind = EntityKind {
    name: "minecraft:minecart",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.COPPER_GOLEM`: `clientTrackingRange(10)`.
pub const COPPER_GOLEM: EntityKind = EntityKind {
    name: "minecraft:copper_golem",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.ARMOR_STAND`: `clientTrackingRange(10)`, interval 3,
/// deltas tracked. The entity is `crate::mob::armor_stand`.
pub const ARMOR_STAND: EntityKind = EntityKind {
    name: "minecraft:armor_stand",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

// The twenty `AbstractBoat` types: every one `sized(1.375F, 0.5625F)
// .eyeHeight(0.5625F).clientTrackingRange(10)`, the default interval 3, and
// `trackDeltas()` does not name them. Spelled out one constant each so the
// `ci/entity-id-allocator.py` and `ci/mob-tick-census.py` census of this
// file sees the whole entity vocabulary.

/// `EntityTypes.OAK_BOAT` — see [`crate::boat`].
pub const OAK_BOAT: EntityKind = EntityKind {
    name: "minecraft:oak_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.OAK_CHEST_BOAT` — see [`crate::boat`].
pub const OAK_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:oak_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.SPRUCE_BOAT` — see [`crate::boat`].
pub const SPRUCE_BOAT: EntityKind = EntityKind {
    name: "minecraft:spruce_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.SPRUCE_CHEST_BOAT` — see [`crate::boat`].
pub const SPRUCE_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:spruce_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.BIRCH_BOAT` — see [`crate::boat`].
pub const BIRCH_BOAT: EntityKind = EntityKind {
    name: "minecraft:birch_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.BIRCH_CHEST_BOAT` — see [`crate::boat`].
pub const BIRCH_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:birch_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.JUNGLE_BOAT` — see [`crate::boat`].
pub const JUNGLE_BOAT: EntityKind = EntityKind {
    name: "minecraft:jungle_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.JUNGLE_CHEST_BOAT` — see [`crate::boat`].
pub const JUNGLE_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:jungle_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.ACACIA_BOAT` — see [`crate::boat`].
pub const ACACIA_BOAT: EntityKind = EntityKind {
    name: "minecraft:acacia_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.ACACIA_CHEST_BOAT` — see [`crate::boat`].
pub const ACACIA_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:acacia_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.MOOSHROOM`: the cow's builder values exactly.
pub const MOOSHROOM: EntityKind = EntityKind {
    name: "minecraft:mooshroom",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.CHERRY_BOAT` — see [`crate::boat`].
pub const CHERRY_BOAT: EntityKind = EntityKind {
    name: "minecraft:cherry_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.SHEEP`: `.sized(0.9F, 1.3F).eyeHeight(1.235F)
/// .passengerAttachments(1.2375F).clientTrackingRange(10)`.
pub const SHEEP: EntityKind = EntityKind {
    name: "minecraft:sheep",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.CHERRY_CHEST_BOAT` — see [`crate::boat`].
pub const CHERRY_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:cherry_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.CHICKEN`: `.sized(0.4F, 0.7F).eyeHeight(0.644F)
/// .passengerAttachments(new Vec3(0, 0.7, -0.1)).clientTrackingRange(10)`.
pub const CHICKEN: EntityKind = EntityKind {
    name: "minecraft:chicken",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.DARK_OAK_BOAT` — see [`crate::boat`].
pub const DARK_OAK_BOAT: EntityKind = EntityKind {
    name: "minecraft:dark_oak_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.DARK_OAK_CHEST_BOAT` — see [`crate::boat`].
pub const DARK_OAK_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:dark_oak_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.PALE_OAK_BOAT` — see [`crate::boat`].
pub const PALE_OAK_BOAT: EntityKind = EntityKind {
    name: "minecraft:pale_oak_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.PALE_OAK_CHEST_BOAT` — see [`crate::boat`].
pub const PALE_OAK_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:pale_oak_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.MANGROVE_BOAT` — see [`crate::boat`].
pub const MANGROVE_BOAT: EntityKind = EntityKind {
    name: "minecraft:mangrove_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.MANGROVE_CHEST_BOAT` — see [`crate::boat`].
pub const MANGROVE_CHEST_BOAT: EntityKind = EntityKind {
    name: "minecraft:mangrove_chest_boat",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.BAMBOO_RAFT` — see [`crate::boat`].
pub const BAMBOO_RAFT: EntityKind = EntityKind {
    name: "minecraft:bamboo_raft",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.BAMBOO_CHEST_RAFT` — see [`crate::boat`].
pub const BAMBOO_CHEST_RAFT: EntityKind = EntityKind {
    name: "minecraft:bamboo_chest_raft",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.END_CRYSTAL`.
///
/// `of(EndCrystal::new, MISC).noLootTable().fireImmune().sized(2.0F, 2.0F)
///  .clientTrackingRange(16).updateInterval(Integer.MAX_VALUE)` (offsets
/// 1975-2015); `EntityType.trackDeltas()` names END_CRYSTAL in its false
/// list. Tracked to **256 blocks**, never re-sent on a timer — only when its
/// data is dirty.
pub const END_CRYSTAL: EntityKind = EntityKind {
    name: "minecraft:end_crystal",
    tracking_range_chunks: 16,
    update_interval: i32::MAX,
    track_deltas: false,
};

/// `EntityTypes.CHEST_MINECART`: `sized(0.98F, 0.7F)`,
/// `.clientTrackingRange(8)`, the default interval 3. A mineshaft's.
pub const CHEST_MINECART: EntityKind = EntityKind {
    name: "minecraft:chest_minecart",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.DROWNED`: `sized(0.6F, 1.95F)`, `.clientTrackingRange(8)`.
pub const DROWNED: EntityKind = EntityKind {
    name: "minecraft:drowned",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.WITCH`: `sized(0.6F, 1.95F)`, `.clientTrackingRange(8)`.
pub const WITCH: EntityKind = EntityKind {
    name: "minecraft:witch",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.CAT`: `sized(0.6F, 0.7F)`, `.clientTrackingRange(8)`.
pub const CAT: EntityKind = EntityKind {
    name: "minecraft:cat",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.EVOKER`: `.clientTrackingRange(8)`, the builder's default
/// `updateInterval` of 3, and not in `trackDeltas`' false list.
pub const EVOKER: EntityKind = EntityKind {
    name: "minecraft:evoker",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.VINDICATOR`: `.clientTrackingRange(8)`, interval 3.
pub const VINDICATOR: EntityKind = EntityKind {
    name: "minecraft:vindicator",
    tracking_range_chunks: 8,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.ALLAY`: `.clientTrackingRange(8).updateInterval(2)`.
pub const ALLAY: EntityKind = EntityKind {
    name: "minecraft:allay",
    tracking_range_chunks: 8,
    update_interval: 2,
    track_deltas: true,
};

/// `EntityTypes.ELDER_GUARDIAN`: `.clientTrackingRange(10)`, interval 3.
pub const ELDER_GUARDIAN: EntityKind = EntityKind {
    name: "minecraft:elder_guardian",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.SHULKER`: `.clientTrackingRange(10)`, interval 3.
pub const SHULKER: EntityKind = EntityKind {
    name: "minecraft:shulker",
    tracking_range_chunks: 10,
    update_interval: 3,
    track_deltas: true,
};

/// `EntityTypes.ITEM_FRAME`.
///
/// `of(ItemFrame::new, MISC).noLootTable().sized(0.5F, 0.5F).eyeHeight(0.0F)
///  .clientTrackingRange(10).updateInterval(Integer.MAX_VALUE)` (offsets
/// 3360-3394); `trackDeltas()` names ITEM_FRAME in its false list.
///
/// A hanging entity's `add_entity` is not built from its position: see
/// [`hanging_block_pos`].
pub const ITEM_FRAME: EntityKind = EntityKind {
    name: "minecraft:item_frame",
    tracking_range_chunks: 10,
    update_interval: i32::MAX,
    track_deltas: false,
};

/// `EntityTypes.EGG`: `of(ThrownEgg::new, MISC).noLootTable().sized(0.25F,
/// 0.25F).clientTrackingRange(4).updateInterval(10)`; not in `trackDeltas()`'
/// false list.
pub const EGG: EntityKind = EntityKind {
    name: "minecraft:egg",
    tracking_range_chunks: 4,
    update_interval: 10,
    track_deltas: true,
};

pub const KINDS: &[EntityKind] = &[
    PLAYER,
    ITEM,
    FALLING_BLOCK,
    PIG,
    BAT,
    EXPERIENCE_ORB,
    IRON_GOLEM,
    SNOW_GOLEM,
    COPPER_GOLEM,
    ARMOR_STAND,
    OAK_BOAT,
    OAK_CHEST_BOAT,
    SPRUCE_BOAT,
    SPRUCE_CHEST_BOAT,
    BIRCH_BOAT,
    BIRCH_CHEST_BOAT,
    JUNGLE_BOAT,
    JUNGLE_CHEST_BOAT,
    ACACIA_BOAT,
    ACACIA_CHEST_BOAT,
    CHERRY_BOAT,
    CHERRY_CHEST_BOAT,
    DARK_OAK_BOAT,
    DARK_OAK_CHEST_BOAT,
    PALE_OAK_BOAT,
    PALE_OAK_CHEST_BOAT,
    MANGROVE_BOAT,
    MANGROVE_CHEST_BOAT,
    BAMBOO_RAFT,
    BAMBOO_CHEST_RAFT,
    MINECART,
    END_CRYSTAL,
    ITEM_FRAME,
    COW,
    MOOSHROOM,
    SHEEP,
    CHICKEN,
    EGG,
    EVOKER,
    VINDICATOR,
    ALLAY,
    ELDER_GUARDIAN,
    SHULKER,
    CHEST_MINECART,
    WITCH,
    CAT,
    DROWNED,
];

/// `ItemFrame.createBoundingBox`'s centre offset: `Vec3.atCenterOf(pos)
/// .relative(direction, -0.46875)`, a frame sitting 1/32 off the block it
/// hangs on.
pub const ITEM_FRAME_INSET: f64 = 0.46875;

/// The block position a hanging entity hangs in — `BlockAttachedEntity.pos`
/// — recovered from its position and `direction` (its `data`, the
/// `get3DDataValue`), for the `add_entity` packet.
///
/// `HangingEntity.getAddEntityPacket` is `new ClientboundAddEntityPacket(
/// this, direction.get3DDataValue(), getPos())` — **the block position, not
/// the entity's**, so the packet says `(1, 0, -1)` for a frame whose centre
/// is at `(1.96875, 0.5, -0.5)`. Measured on `an-item-frame-hangs-on-a-
/// blocks-side`.
#[must_use]
pub fn hanging_block_pos(e: &Entity) -> Option<(f64, f64, f64)> {
    if e.kind != ITEM_FRAME {
        return None;
    }
    let face = crate::interact::Face::from_3d(e.data)?;
    let (sx, sy, sz) = face.relative((0, 0, 0));
    let c = (
        e.x + f64::from(sx) * ITEM_FRAME_INSET,
        e.y + f64::from(sy) * ITEM_FRAME_INSET,
        e.z + f64::from(sz) * ITEM_FRAME_INSET,
    );
    Some((c.0.floor(), c.1.floor(), c.2.floor()))
}

/// `EntityType.getDimensions()` — `(width, height)` from each kind's
/// `sized(...)` — for the kinds this server has, and `None` for a name it
/// does not model. Player 0.6 x 1.8, item 0.25 x 0.25, falling block
/// 0.98 x 0.98, pig 0.9 x 0.9, bat 0.5 x 0.9, minecart 0.98 x 0.7, end
/// crystal 2.0 x 2.0, each the `sized` call in `EntityTypes.<clinit>`.
#[must_use]
pub fn dimensions(kind: EntityKind) -> Option<(f64, f64)> {
    Some(match kind.name {
        "minecraft:player" => (0.6, 1.8),
        "minecraft:item" => (0.25, 0.25),
        "minecraft:falling_block" => (0.98, 0.98),
        "minecraft:pig" => (0.9, 0.9),
        "minecraft:bat" => (0.5, 0.9),
        "minecraft:minecart" => (0.98, 0.7),
        "minecraft:end_crystal" => (2.0, 2.0),
        // `sized(0.5F, 0.5F)`, which a frame's real box — 0.75 x 0.75 x
        // 0.0625 about its centre, `ItemFrame.createBoundingBox` — is not.
        "minecraft:item_frame" => (0.5, 0.5),
        // `EntityType.Builder.sized` for the generated structure mobs.
        "minecraft:evoker" | "minecraft:vindicator" => (0.6, 1.95),
        "minecraft:allay" => (0.35, 0.6),
        "minecraft:elder_guardian" => (1.9975, 1.9975),
        "minecraft:shulker" => (1.0, 1.0),
        "minecraft:chest_minecart" => (0.98, 0.7),
        "minecraft:witch" | "minecraft:drowned" => (0.6, 1.95),
        "minecraft:cat" => (0.6, 0.7),
        _ => return None,
    })
}

/// Every entity in the level, and the allocator for their ids.
///
/// # Ids are unique across kinds, and they were not
///
/// `JoinInfo::default().player_id` is 1 and nothing ever overrode it, so
/// **every player joining this server was told its own entity id was 1**.
/// With one player that is invisible; with two it is a collision that no
/// amount of correct tracking can survive, because the second player's
/// `add_entity` names the id the first client is already using for itself.
/// The allocator lives here rather than beside the connection registry for
/// exactly that reason: an item and a player must draw from one sequence.
/// `ServerLevel`'s TWO entity containers, under one lock.
///
/// # Vanilla holds the same entities in two orders and they are not the same
///
/// This used to be one `BTreeMap`, and one map was enough while the walk was
/// ascending id, because ascending id and insertion order **coincide** here:
/// the allocator is monotonic and an entity is inserted in the tick it draws
/// its id. Porting `ChunkMap.entityMap`'s walk broke that coincidence, and
/// nearly broke a place where we already agreed with vanilla:
///
/// * `ChunkMap.entityMap` is an `Int2ObjectOpenHashMap` — KEYHASH — and
///   `ChunkMap.tick` walks it for the broadcast. That is [`Level::by_id`].
/// * `ServerLevel.entityTickList` is an `EntityTickList`, whose `active` and
///   `passive` are both `Int2ObjectLinkedOpenHashMap` — INSERTION — and the
///   entity tick walks it. That is [`Level::tick_order`].
///
/// One container serving both would have to pick an order, and either choice
/// is wrong for one of them. So there are two, which is also how many vanilla
/// has.
#[derive(Debug, Default)]
struct Level {
    /// `ChunkMap.entityMap`. KEYHASH, and the order every per-entity packet
    /// leaves in.
    by_id: IntOpenHashMap<Entity>,
    /// `ServerLevel.entityTickList.active`. INSERTION, and the order the
    /// entity tick runs in.
    ///
    /// A `Vec` rather than a second ported map: a linked open hash map's walk
    /// IS insertion order, re-putting a live key does not move it, and a
    /// remove takes it out of the chain — all three of which a `Vec` with a
    /// push-if-absent and a `retain` reproduce exactly. Porting the linked map
    /// as well would add a second container to be wrong in and buy no order
    /// this does not already give.
    tick_order: Vec<i32>,
}

#[derive(Debug, Default)]
pub struct Entities {
    next_id: AtomicI32,
    live: Mutex<Level>,
}

impl Entities {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The process-wide level entity list, so a connection thread and the
    /// clock thread are talking about the same entities.
    #[must_use]
    pub fn shared() -> &'static Self {
        static SHARED: OnceLock<Entities> = OnceLock::new();
        SHARED.get_or_init(Entities::new)
    }

    /// A fresh entity id.
    ///
    /// # Vanilla's allocator is not on `Entity`, and it is not one line
    ///
    /// This comment used to name `Entity.ENTITY_COUNTER`. There is no such
    /// field in 26.2. `Entity`'s constructor calls `Level.getNextEntityId()`,
    /// the base `Level` returns 0, and `ServerLevel` overrides it with
    ///
    /// ```java
    /// int i = 0;
    /// while (i == 0 || this.chunkSource.hasEntityWithId(i))
    ///     i = ENTITY_COUNTER.incrementAndGet();
    /// return i;
    /// ```
    ///
    /// where `ENTITY_COUNTER` is a private static final `AtomicInteger` on
    /// `ServerLevel`, built with the no-argument constructor. So the first id
    /// in a session is 1 — same as here, and for the same reason ours must
    /// never hand out 0: `Entity.getId` throws
    /// `Tried to access entity ID before ID assignment` on a zero id, and a
    /// client treats 0 as "no entity" in several packets.
    ///
    /// Two properties of that loop are NOT reproduced here, both deliberately
    /// and both written down in `ci/entity-id-allocator.py`'s ledger:
    ///
    /// * the skip-if-live retry. Under a monotonic counter it is unreachable
    ///   until the counter wraps at 2^31, so it is a wraparound guard we do
    ///   not need yet. It stops being one the moment either side reuses an id;
    /// * **the draw happens in `Entity.<init>`**, so an entity vanilla
    ///   constructs and then throws away still burns a number.
    ///   `PlayerList.respawn` is exactly that — it builds a second
    ///   `ServerPlayer`, which draws an id, then stamps the old id over it.
    ///   We have no respawn. Whoever writes one has to burn an id, or every
    ///   entity created afterwards carries the opposite parity to vanilla's,
    ///   and `Mob.serverAiStep` reads that parity: its goal phase is
    ///   `(tickCount + getId()) % 2`.
    pub fn next_id(&self) -> i32 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// # Panics
    /// If the lock is poisoned.
    pub fn insert(&self, e: Entity) {
        let mut l = self.live.lock().expect("entities");
        let id = e.id;
        if l.by_id.insert(id, e).is_none() {
            // `Int2ObjectLinkedOpenHashMap.put` leaves a live key where it
            // was, so only a genuinely new entity joins the tick order.
            l.tick_order.push(id);
        }
    }

    /// # Panics
    /// If the lock is poisoned.
    pub fn remove(&self, id: i32) -> Option<Entity> {
        let mut l = self.live.lock().expect("entities");
        let out = l.by_id.remove(id);
        if out.is_some() {
            l.tick_order.retain(|&k| k != id);
        }
        out
    }

    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn get(&self, id: i32) -> Option<Entity> {
        self.live.lock().expect("entities").by_id.get(id).cloned()
    }

    /// Read every entity, in `ChunkMap.entityMap`'s walk order.
    ///
    /// # This used to say "ascending id, and that is a stated deferral"
    ///
    /// `ChunkMap.entityMap` is a plain `Int2ObjectOpenHashMap` — `ChunkMap`
    /// has four map fields and the other three are
    /// `Long2ObjectLinkedOpenHashMap` — and `ChunkMap.tick()` walks its
    /// `values()` into `TrackedEntity.updatePlayers` and
    /// `ServerEntity.sendChanges`. So the order every per-entity packet leaves
    /// the server in, within one tick, is a hash of the entity id values and
    /// of nothing else. Insertion order does not enter it: {5,4,3,2,1} walks
    /// the same as {1,2,3,4,5}.
    ///
    /// Roadmap §101.5 measured that and deliberately did not act on it. §103
    /// is the port: [`crate::openhash::IntOpenHashMap`] reproduces the ten
    /// inputs that decide the walk, each read from the shipped fastutil's
    /// bytecode and pinned by `ci/fastutil-open-hash-walk.py`, and its replay
    /// test agrees with 948 measured vanilla histories.
    ///
    /// ```text
    /// ids {1,2}    vanilla  2 1      ours  2 1
    /// ids {1,2,3}  vanilla  2 1 3    ours  2 1 3
    /// ```
    ///
    /// # What this is NOT evidence for, which is the half worth reading
    ///
    /// **No scenario in the corpus can see this order.** `ci/order-blind.py`
    /// files all 108 gameplay checks and finds 0 that can grade a per-tick
    /// entity order; `ci/order-selfconsistency.py` returns UNMEASURABLE for
    /// this site from the recordings' side. The only proxy the recordings
    /// offer mixes packets emitted *during* the entity tick, which leave in
    /// `EntityTickList` insertion order, with this broadcast, which leaves in
    /// hash order — 62 of 87 for the hash walk against 53 for ascending id,
    /// which is neither, and §101.6 declined to report 71.3% as support.
    ///
    /// So the evidence for this walk is the golden and the unit tests, and
    /// nothing on the corpus moved when it landed. §101.8's item 1 — a
    /// scenario with two entities producing packets in one tick — is still
    /// open, and is what would let a check grade this rather than replay it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn all(&self) -> Vec<Entity> {
        self.live
            .lock()
            .expect("entities")
            .by_id
            .values()
            .cloned()
            .collect()
    }

    /// Every live entity in **insertion** order — `EntityTickList`'s walk,
    /// which is `ServerLevel.players()`'s order for the players in it.
    ///
    /// # Panics
    /// If the lock is poisoned.
    #[must_use]
    pub fn all_in_tick_order(&self) -> Vec<Entity> {
        let l = self.live.lock().expect("entities");
        l.tick_order
            .iter()
            .filter_map(|&id| l.by_id.get(id).cloned())
            .collect()
    }

    /// Mutate one entity in place, if it is still alive.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn update(&self, id: i32, f: impl FnOnce(&mut Entity)) {
        let _: Option<()> = self.modify(id, f);
    }

    /// Mutate **every** live entity, under one acquisition of the lock.
    ///
    /// The per-tick shape, and it is not the same as `all()` followed by an
    /// `update` each: that clones every entity and then takes the lock once
    /// per id, which is a full copy of the level's entity list and `n` lock
    /// round trips to do something that touches three `i32`s. `ServerLevel`'s
    /// own entity tick is one pass over one list.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn update_all(&self, mut f: impl FnMut(&mut Entity)) {
        let mut l = self.live.lock().expect("entities");
        let Level { by_id, tick_order } = &mut *l;
        for &id in tick_order.iter() {
            if let Some(e) = by_id.get_mut(id) {
                f(e);
            }
        }
    }

    /// The same, for a caller that needs the closure's answer.
    ///
    /// `None` means the entity is gone, which is not the same as a closure
    /// that returned nothing: a swing whose player disconnected mid-packet
    /// must send no `animate`, and folding the two into a `bool` would make
    /// that indistinguishable from the swallow.
    ///
    /// # Panics
    /// If the lock is poisoned.
    pub fn modify<T>(&self, id: i32, f: impl FnOnce(&mut Entity) -> T) -> Option<T> {
        self.live.lock().expect("entities").by_id.get_mut(id).map(f)
    }
}

/// `Mth.packDegrees(float)`, the rotation byte.
///
/// ```java
/// public static byte packDegrees(float f) {
///     return (byte) Mth.floor(f * 256.0F / 360.0F);
/// }
/// public static int floor(float f) { return (int) Math.floor((double) f); }
/// ```
///
/// Three things a rewrite gets wrong and this does not: the arithmetic is in
/// **`f32`** (both the multiply and the divide) and only then widened, the
/// rounding is **floor and not truncation** — which differ for every negative
/// angle, so a player looking north-west packs one step off — and the result
/// is narrowed to a **signed** byte.
#[must_use]
pub fn pack_degrees(deg: f32) -> i8 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2i then i2b, reproduced"
    )]
    let v = f64::from(deg * 256.0f32 / 360.0f32).floor() as i32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`i2b` — the packed rotation is one byte by definition"
    )]
    let b = v as i8;
    b
}

/// `VecDeltaCodec.encode(double)`: `Math.round(d * 4096.0)`.
///
/// **`Math.round` is floor-of-plus-a-half, not round-half-to-even**, and Rust's
/// `f64::round` is half-away-from-zero. The two differ at exactly the negative
/// halves — `-0.5` rounds to `0` in Java and to `-1` in Rust — so this is
/// written as Java's own definition rather than as a call to `round`.
#[must_use]
pub fn encode_pos(v: f64) -> i64 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Java's d2l, reproduced; the domain is a world coordinate"
    )]
    let r = (v * 4096.0 + 0.5).floor() as i64;
    r
}

/// `net.minecraft.network.LpVec3.write` — the low-precision vector 26.2
/// introduced, and the encoding `add_entity` carries its movement in.
///
/// # The format, from the bytecode
///
/// Every component is first `sanitize`d — NaN becomes 0 and everything else
/// is clamped to ±17179869183/10^-… precisely ±1.7179869183E10 — and then the
/// largest absolute component decides the shape:
///
/// * if it is `< 3.051944088384301E-5`, the whole vector is **one zero byte**
///   and nothing else. This is the case that matters most in practice: a
///   stationary entity's `add_entity` carries exactly one byte of movement.
/// * otherwise a scale `s = ceilLong(absMax)` is chosen, each component is
///   divided by it and `pack`ed to 15 bits, and the three fields are laid into
///   a `long` at bit offsets 3, 18 and 33. The low three bits hold
///   `s & 3`, with bit 2 set when `s` did not fit in two bits, in which case
///   `s >> 2` follows as a varint.
///
/// The long is written **little-endian in pieces** — `writeByte(v)`,
/// `writeByte(v >> 8)`, `writeInt(v >> 16)` — where that last `writeInt` is
/// Netty's big-endian 32-bit write of the *high* 32 bits. That mixture is not
/// a mistake in the reading: it is what the bytecode does, and a tidy
/// little-endian `i64` write produces different bytes.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn write_lp_vec3(out: &mut Vec<u8>, v: (f64, f64, f64)) -> std::io::Result<()> {
    /// `sanitize`: NaN to zero, then `Math.clamp` to the representable range.
    fn sanitize(d: f64) -> f64 {
        if d.is_nan() {
            0.0
        } else {
            d.clamp(-1.7179869183E10, 1.7179869183E10)
        }
    }
    /// `pack`: `Math.round((d * 0.5 + 0.5) * 32766.0)`, and `Math.round` is
    /// again floor-of-plus-a-half rather than Rust's half-away-from-zero.
    fn pack(d: f64) -> i64 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "Java's d2l on a value bounded by 32766"
        )]
        let r = ((d * 0.5 + 0.5) * 32766.0 + 0.5).floor() as i64;
        r
    }

    let (x, y, z) = (sanitize(v.0), sanitize(v.1), sanitize(v.2));
    // `Mth.absMax(Mth.absMax(x, y), z)` — associated left, which does not
    // matter for a max but is reproduced anyway.
    let abs_max = x.abs().max(y.abs()).max(z.abs());
    if abs_max < 3.051944088384301E-5 {
        out.push(0);
        return Ok(());
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "Mth.ceilLong is Math.ceil then d2l"
    )]
    let scale = abs_max.ceil() as i64;
    // `(scale & 3) != scale` — true exactly when the scale needs more than
    // two bits, which is what the continuation bit announces.
    let continues = (scale & 3) != scale;
    #[expect(clippy::cast_precision_loss, reason = "vanilla's own l2d")]
    let s = scale as f64;
    let head = if continues { (scale & 3) | 4 } else { scale };
    let packed = head | (pack(x / s) << 3) | (pack(y / s) << 18) | (pack(z / s) << 33);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's l2i then i2b on each byte"
    )]
    {
        out.push(packed as u8);
        out.push((packed >> 8) as u8);
        // `writeInt` — big-endian, of the high 32 bits.
        out.extend_from_slice(&((packed >> 16) as i32).to_be_bytes());
    }
    if continues {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's l2i before VarInt.write"
        )]
        let tail = (scale >> 2) as i32;
        write_varint(out, tail)?;
    }
    Ok(())
}

/// `ClientboundAddEntityPacket`'s body.
///
/// ```text
///   varint  entity id
///   uuid    16 bytes
///   varint  entity type, as a registry id
///   double  x, y, z
///   LpVec3  movement
///   byte    xRot
///   byte    yRot
///   byte    yHeadRot
///   varint  data
/// ```
///
/// **`xRot` comes before `yRot`.** Every other rotation pair on this wire is
/// the other way round, including `ClientboundMoveEntityPacket`'s, so the
/// natural transcription of this packet is wrong by one field and produces an
/// entity facing a plausible but incorrect direction — the kind of difference
/// that reads as "close enough" and is not.
///
/// # What the fields are read from, which is not the entity
///
/// The two-argument constructor takes its position from
/// `serverEntity.getPositionBase()` and its rotations and movement from
/// `getLastSentXRot/YRot/YHeadRot/LastSentMovement`. That is the tracker's
/// **last-sent** state, not the entity's live state, so an entity that has
/// moved since its last position broadcast is announced at the older
/// position — deliberately, so the client's dead-reckoning starts from the
/// same base the subsequent deltas are measured against.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn add_entity_body(a: &Arrival) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(64);
    write_varint(&mut b, a.id)?;
    wire::write_u128(&mut b, a.uuid)?;
    write_varint(&mut b, a.kind_id)?;
    wire::write_f64(&mut b, a.at.0)?;
    wire::write_f64(&mut b, a.at.1)?;
    wire::write_f64(&mut b, a.at.2)?;
    write_lp_vec3(&mut b, a.movement)?;
    #[expect(clippy::cast_sign_loss, reason = "writeByte of a signed byte")]
    {
        wire::write_u8(&mut b, a.x_rot as u8)?;
        wire::write_u8(&mut b, a.y_rot as u8)?;
        wire::write_u8(&mut b, a.y_head_rot as u8)?;
    }
    write_varint(&mut b, a.data)?;
    Ok(b)
}

/// Everything `ClientboundAddEntityPacket` carries, named.
///
/// A record rather than nine positional arguments, and not only to satisfy a
/// lint: **three of the nine are single bytes of rotation whose order is the
/// one trap in this packet**, and three more are an interchangeable triple of
/// `f64`. A call site with nine bare values cannot be read for correctness at
/// all, which is precisely the condition under which the `xRot`/`yRot`
/// transposition survives review.
#[derive(Debug, Clone, Copy)]
pub struct Arrival {
    pub id: i32,
    pub uuid: u128,
    /// The numeric `minecraft:entity_type` id, from [`KindIds`].
    pub kind_id: i32,
    /// `ServerEntity.getPositionBase()`, not the entity's live position.
    pub at: (f64, f64, f64),
    /// `ServerEntity.getLastSentMovement()`.
    pub movement: (f64, f64, f64),
    pub x_rot: i8,
    pub y_rot: i8,
    pub y_head_rot: i8,
    pub data: i32,
}

/// `ClientboundRemoveEntitiesPacket`: a varint count then a varint each.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn remove_entities_body(ids: &[i32]) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(1 + ids.len());
    write_varint(&mut b, i32::try_from(ids.len()).unwrap_or(i32::MAX))?;
    for &id in ids {
        write_varint(&mut b, id)?;
    }
    Ok(b)
}

/// `ClientboundMoveEntityPacket$Pos`: `(varint id, short dx, short dy,
/// short dz, bool onGround)`.
///
/// The deltas are in **1/4096 of a block** and are the difference of two
/// separately rounded positions — see [`encode_pos`] — never the rounding of
/// a difference.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn move_entity_pos_body(
    id: i32,
    d: (i16, i16, i16),
    on_ground: bool,
) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(12);
    write_varint(&mut b, id)?;
    wire::write_i16(&mut b, d.0)?;
    wire::write_i16(&mut b, d.1)?;
    wire::write_i16(&mut b, d.2)?;
    wire::write_bool(&mut b, on_ground)?;
    Ok(b)
}

/// `ClientboundMoveEntityPacket$PosRot`: the same, plus `yRot` then `xRot`
/// before the ground flag.
///
/// **Here the pair is `yRot, xRot`**, the opposite order to
/// [`add_entity_body`]'s. Both are from the bytecode; the wire is simply not
/// self-consistent, and that is exactly why neither can be inferred from the
/// other.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn move_entity_pos_rot_body(
    id: i32,
    d: (i16, i16, i16),
    y_rot: i8,
    x_rot: i8,
    on_ground: bool,
) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(14);
    write_varint(&mut b, id)?;
    wire::write_i16(&mut b, d.0)?;
    wire::write_i16(&mut b, d.1)?;
    wire::write_i16(&mut b, d.2)?;
    #[expect(clippy::cast_sign_loss, reason = "writeByte of a signed byte")]
    {
        wire::write_u8(&mut b, y_rot as u8)?;
        wire::write_u8(&mut b, x_rot as u8)?;
    }
    wire::write_bool(&mut b, on_ground)?;
    Ok(b)
}

/// `ClientboundMoveEntityPacket$Rot`: `(varint id, byte yRot, byte xRot,
/// bool onGround)`.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn move_entity_rot_body(
    id: i32,
    y_rot: i8,
    x_rot: i8,
    on_ground: bool,
) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(8);
    write_varint(&mut b, id)?;
    #[expect(clippy::cast_sign_loss, reason = "writeByte of a signed byte")]
    {
        wire::write_u8(&mut b, y_rot as u8)?;
        wire::write_u8(&mut b, x_rot as u8)?;
    }
    wire::write_bool(&mut b, on_ground)?;
    Ok(b)
}

/// `ClientboundRotateHeadPacket`: `(varint id, byte yHeadRot)`.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn rotate_head_body(id: i32, y_head_rot: i8) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(6);
    write_varint(&mut b, id)?;
    #[expect(clippy::cast_sign_loss, reason = "writeByte of a signed byte")]
    wire::write_u8(&mut b, y_head_rot as u8)?;
    Ok(b)
}

/// The numeric `minecraft:entity_type` ids.
///
/// **Not a table of constants**, and **not read from `registries.bin`.** The
/// first attempt resolved these against the captured `registry_data` packets,
/// where `minecraft:world_clock` comes from. That capture does not hold
/// `minecraft:entity_type` and never will: only datapack-driven registries
/// are synced to a client, and the built-in ones are compiled into it.
///
/// The failure was silent in shape and loud only because the caller printed a
/// line: every kind resolved to nothing, every pairing bundle was empty, and
/// the server behaved exactly like one with no tracking at all. Defaulting a
/// missing id to zero would have been worse — entity type 0 is a real type,
/// so every player would have been announced as one and the result would have
/// read as a behaviour defect rather than a missing table.
///
/// The ids come from `data/vanilla/entity_types.json`, generated by
/// `ci/dump-entity-types.sh` out of vanilla's own registry report.
#[derive(Debug, Default, Clone)]
pub struct KindIds(BTreeMap<&'static str, i32>);

impl KindIds {
    /// The entity type name a registry id belongs to, among the kinds this
    /// server can create.
    #[must_use]
    pub fn name_of(&self, id: i32) -> Option<&'static str> {
        self.0.iter().find(|(_, v)| **v == id).map(|(k, _)| *k)
    }

    /// Resolve the kinds this server can create against a name-to-id map.
    ///
    /// Absent for a name the table does not hold, never zero.
    #[must_use]
    pub fn resolve(entity_types: &BTreeMap<String, i32>, kinds: &[EntityKind]) -> Self {
        let mut m = BTreeMap::new();
        for k in kinds {
            if let Some(id) = entity_types.get(k.name) {
                m.insert(k.name, *id);
            }
        }
        Self(m)
    }

    /// Parse `ci/dump-entity-types.sh`'s output: `{"entity_types": {name: id}}`.
    ///
    /// # Errors
    /// If the file is missing, unparseable, or does not hold the object.
    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path)?).map_err(std::io::Error::other)?;
        let table = doc
            .get("entity_types")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| std::io::Error::other("no `entity_types` object"))?;
        let mut names = BTreeMap::new();
        for (name, id) in table {
            if let Some(id) = id.as_i64().and_then(|v| i32::try_from(v).ok()) {
                names.insert(name.clone(), id);
            }
        }
        Ok(Self::resolve(&names, KINDS))
    }

    #[must_use]
    pub fn get(&self, kind: EntityKind) -> Option<i32> {
        self.0.get(kind.name).copied()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The kind ids for every type this server can create, resolved once against
/// the registry capture the clients are sent.
///
/// `None` means the capture is missing or holds none of our names, which the
/// caller says out loud: without it no entity can be announced at all, and
/// "entities are not implemented" and "the extract is missing" would
/// otherwise look identical from a client.
///
/// # Path resolution
///
/// Tried first relative to the process's cwd, unchanged from before this
/// grew a fallback; only when that file is absent does it fall back to a
/// `CARGO_MANIFEST_DIR`-anchored path, which is what makes this reachable
/// under `cargo test` (whose cwd is the package directory, not the repo
/// root). Same shape and the same carried risk as
/// `crate::protocol_ids::shared()`: `docs/PARITY-ROADMAP.md` §34.
pub fn shared_kind_ids() -> Option<&'static KindIds> {
    static SHARED: OnceLock<Option<KindIds>> = OnceLock::new();
    SHARED
        .get_or_init(|| {
            const REL: &str = "data/vanilla/entity_types.json";
            let cwd_relative = std::path::Path::new(REL);
            let path: std::path::PathBuf = if cwd_relative.exists() {
                cwd_relative.to_path_buf()
            } else {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join(REL)
            };
            let ids = KindIds::load(&path).ok()?;
            if ids.is_empty() {
                None
            } else {
                Some(ids)
            }
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A living entity at rest, for the swing state machine.
    fn resting() -> Entity {
        Entity {
            id: 1,
            uuid: 0,
            kind: PLAYER,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: true,
            data: 0,
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            synched: crate::synched::Synched::Avatar {
                health: 20.0,
                model_customisation: 0,
                score: 0,
            },
        }
    }

    /// The first swing always goes out, and it leaves `swingTime` at **-1**.
    /// A `0` there would make the very next swing swallowed instead of the
    /// one after it, which is one tick's difference and invisible in any test
    /// that only swings once.
    #[test]
    fn a_swing_from_rest_broadcasts_and_arms_the_animation() {
        let mut e = resting();
        assert!(swing(&mut e, 0));
        assert!(e.swinging);
        assert_eq!(e.swing_time, -1, "swing sets iconst_m1, not iconst_0");
        assert_eq!(e.swinging_arm, Some(0));
    }

    /// **The swallow, tick by tick, which is what `581` measures.**
    ///
    /// The guard is `swinging && swingTime < duration/2 && swingTime >= 0`,
    /// so at the default duration of 6 the swallowed values are exactly
    /// 0, 1 and 2 — and `-1` is *not* one of them.
    #[test]
    fn the_swallow_covers_swing_times_zero_to_two_and_not_minus_one() {
        for (swing_time, expected) in [(-1, true), (0, false), (1, false), (2, false), (3, true)] {
            let mut e = resting();
            e.swinging = true;
            e.swing_time = swing_time;
            assert_eq!(
                swing(&mut e, 0),
                expected,
                "swingTime {swing_time} against a half of {}",
                DEFAULT_SWING_DURATION / 2
            );
        }
    }

    /// One swing, then a tick, then another: the second is swallowed. Two
    /// packets from this pair is what a server forwarding every `swing`
    /// produces.
    #[test]
    fn a_second_swing_one_tick_later_sends_nothing() {
        let mut e = resting();
        assert!(swing(&mut e, 0));
        update_swing_time(&mut e);
        assert_eq!(e.swing_time, 0, "-1 incremented once");
        assert!(!swing(&mut e, 0), "the second swing is swallowed");
        // And three ticks in it is past the half and swings again.
        update_swing_time(&mut e);
        update_swing_time(&mut e);
        assert_eq!(e.swing_time, 2);
        assert!(!swing(&mut e, 0), "still inside the first half");
        update_swing_time(&mut e);
        assert_eq!(e.swing_time, 3);
        assert!(swing(&mut e, 0), "past the half, so it swings again");
    }

    /// The animation ends at the duration and resets both fields — otherwise
    /// `swinging` stays true for ever and every later swing is decided by a
    /// `swingTime` that keeps climbing.
    ///
    /// **It takes `duration + 1` ticks, not `duration`**, and this test was
    /// written asserting the wrong one of those and caught it. `swing` sets
    /// `swingTime = -1`, so the walk is -1, 0, 1, 2, 3, 4, 5, 6 and only the
    /// **seventh** increment reaches `>= 6`. The extra tick is the same
    /// `iconst_m1` the swallow depends on; a version that reset to 0 would
    /// shorten the animation by a tick *and* let a same-tick second swing be
    /// swallowed.
    #[test]
    fn the_animation_ends_one_tick_after_its_duration_and_resets() {
        let mut e = resting();
        swing(&mut e, 0);
        for _ in 0..DEFAULT_SWING_DURATION {
            update_swing_time(&mut e);
        }
        assert!(
            e.swinging,
            "six ticks from -1 reaches 5, which is still under the duration"
        );
        assert_eq!(e.swing_time, DEFAULT_SWING_DURATION - 1);
        update_swing_time(&mut e);
        assert!(!e.swinging, "the seventh finishes it");
        assert_eq!(e.swing_time, 0);
        // And a non-swinging entity is pinned at zero rather than drifting.
        update_swing_time(&mut e);
        assert_eq!(e.swing_time, 0);
    }

    /// `SWING_MAIN_HAND` is 0 and `SWING_OFF_HAND` is **3**, while the hand's
    /// own ordinal is 0 and 1. A server forwarding the ordinal would send
    /// action 1, which is not one of the packet's five constants.
    #[test]
    fn the_off_hand_action_is_three_and_not_the_hands_ordinal() {
        assert_eq!(animate_action_for_hand(0), 0);
        assert_eq!(animate_action_for_hand(1), 3);
        assert_ne!(
            animate_action_for_hand(1),
            1,
            "forwarding the ordinal names no animation at all"
        );
        assert_ne!(
            animate_action_for_hand(1),
            2,
            "2 is WAKE_UP, which lays the player down"
        );
    }

    /// The three properties a truncating implementation gets wrong.
    #[test]
    fn packing_degrees_floors_and_wraps_like_vanilla() {
        assert_eq!(pack_degrees(0.0), 0);
        assert_eq!(pack_degrees(90.0), 64);
        assert_eq!(pack_degrees(180.0), -128, "wraps through the signed byte");
        // **Floor, not truncation.** `-90 * 256 / 360` is exactly -64, so
        // that one agrees either way; -1 degree is -0.711, which floors to
        // -1 and truncates to 0.
        assert_eq!(pack_degrees(-90.0), -64);
        assert_eq!(pack_degrees(-1.0), -1, "truncation would give 0");
        assert_eq!(pack_degrees(-0.1), -1, "truncation would give 0");
        // A yaw the placement corpus actually uses.
        assert_eq!(pack_degrees(22.5), 16);
    }

    /// A delta is the difference of two rounded positions, not the rounding
    /// of a difference — and the two disagree wherever the halves land
    /// differently.
    #[test]
    fn a_position_delta_is_a_difference_of_rounded_values() {
        // 1/8192 of a block: each endpoint rounds to a different integer even
        // though the difference is half a unit.
        let a = encode_pos(0.5 / 4096.0);
        let b = encode_pos(0.0);
        assert_eq!(a - b, 1, "0.5 rounds up, as Java's Math.round does");
        // Java's Math.round(-0.5) is 0, where Rust's f64::round(-0.5) is -1.
        assert_eq!(
            encode_pos(-0.5 / 4096.0),
            0,
            "floor(x + 0.5), not away from zero"
        );
        assert_eq!(encode_pos(-1.5 / 4096.0), -1);
        assert_eq!(encode_pos(1.0), 4096);
        assert_eq!(encode_pos(-1.0), -4096);
    }

    /// The stationary case, which is the one every `add_entity` in this
    /// corpus takes: strictly one zero byte, no long, no varint.
    #[test]
    fn a_still_entity_encodes_its_movement_in_one_byte() {
        let mut b = Vec::new();
        write_lp_vec3(&mut b, (0.0, 0.0, 0.0)).expect("buf");
        assert_eq!(b, vec![0]);

        // The threshold is exclusive-below: just under is still one byte.
        let mut b = Vec::new();
        write_lp_vec3(&mut b, (3.0E-5, 0.0, 0.0)).expect("buf");
        assert_eq!(b, vec![0], "3.0E-5 < 3.051944088384301E-5");

        // NaN sanitises to zero rather than propagating into the scale.
        let mut b = Vec::new();
        write_lp_vec3(&mut b, (f64::NAN, f64::NAN, f64::NAN)).expect("buf");
        assert_eq!(b, vec![0]);
    }

    /// Above the threshold the body is six bytes, and a scale that does not
    /// fit in two bits adds a varint — the continuation bit being bit 2 of
    /// the first byte.
    #[test]
    fn a_moving_entity_encodes_a_scale_and_three_packed_fields() {
        let mut small = Vec::new();
        write_lp_vec3(&mut small, (0.1, 0.0, 0.0)).expect("buf");
        assert_eq!(small.len(), 6, "scale 1 fits in two bits: no varint");
        assert_eq!(small[0] & 3, 1, "ceil(0.1) == 1");
        assert_eq!(small[0] & 4, 0, "no continuation");

        // ceil(9.0) == 9, which is 0b1001 — three low bits cannot hold it, so
        // the continuation bit is set and 9 >> 2 == 2 follows as a varint.
        let mut big = Vec::new();
        write_lp_vec3(&mut big, (9.0, 0.0, 0.0)).expect("buf");
        assert_eq!(big.len(), 7, "six bytes plus the scale's varint");
        assert_eq!(big[0] & 4, 4, "continuation set");
        assert_eq!(big[0] & 3, 1, "9 & 3 == 1");
        assert_eq!(big[6], 2, "9 >> 2 == 2");
    }

    /// The field order, asserted where a transposition would show: `xRot`
    /// before `yRot` in this packet and nowhere else.
    #[test]
    fn add_entity_writes_pitch_before_yaw() {
        let body = add_entity_body(&Arrival {
            id: 7,
            uuid: 0,
            kind_id: 3,
            at: (1.0, 2.0, 3.0),
            movement: (0.0, 0.0, 0.0),
            x_rot: 11,
            y_rot: 22,
            y_head_rot: 33,
            data: 0,
        })
        .expect("buf");
        // varint 7, 16 bytes of uuid, varint 3, three doubles, one zero byte
        // of movement — then the three rotations.
        let at = 1 + 16 + 1 + 24 + 1;
        assert_eq!(body[at], 11, "xRot first");
        assert_eq!(body[at + 1], 22, "then yRot");
        assert_eq!(body[at + 2], 33, "then yHeadRot");
        assert_eq!(body[at + 3], 0, "then the data varint");
        assert_eq!(body.len(), at + 4);
    }

    /// ...and the move packet is the other way round, which is the whole
    /// reason both are asserted.
    #[test]
    fn move_entity_pos_rot_writes_yaw_before_pitch() {
        let body = move_entity_pos_rot_body(7, (1, 2, 3), 44, 55, true).expect("buf");
        assert_eq!(body[7], 44, "yRot first here");
        assert_eq!(body[8], 55, "then xRot");
        assert_eq!(body[9], 1, "onGround");
    }

    /// The heartbeat's exact bytes: a zero delta and a ground flag. This is
    /// what `520-a-standing-player-is-rebroadcast-on-a-timer` compares, and
    /// the harness digests everything after the deltas — so the trailing byte
    /// is load-bearing, not decoration.
    #[test]
    fn a_heartbeat_is_three_zero_deltas_and_a_ground_flag() {
        let body = move_entity_pos_body(28, (0, 0, 0), true).expect("buf");
        assert_eq!(body, vec![28, 0, 0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn removing_entities_is_a_count_then_the_ids() {
        assert_eq!(remove_entities_body(&[5]).expect("buf"), vec![1, 5]);
        assert_eq!(remove_entities_body(&[]).expect("buf"), vec![0]);
    }

    /// The entity TICK runs in insertion order, and the BROADCAST does not.
    ///
    /// Vanilla holds its entities in two containers with different orders and
    /// this is the test that keeps ours from collapsing them back into one.
    /// `ServerLevel.entityTickList`'s `active` is an
    /// `Int2ObjectLinkedOpenHashMap` — insertion — and `ChunkMap.entityMap` is
    /// a plain `Int2ObjectOpenHashMap` — a hash of the ids.
    ///
    /// **This nearly went the other way.** While `Entities` was one
    /// `BTreeMap`, the tick walked ascending id, which for a monotonic
    /// allocator inserting in allocation order IS insertion order — so we
    /// agreed with vanilla there by coincidence. Porting the broadcast walk
    /// into that one container would have moved the tick to hash order and
    /// broken an agreement while closing a divergence, and nothing in the
    /// corpus would have said so: `update_swing_time` touches one `i32` per
    /// entity and no ordering of it is observable today.
    #[test]
    fn the_tick_order_and_the_broadcast_order_are_not_the_same_order() {
        let es = Entities::new();
        for id in [1, 2, 3, 4, 5] {
            es.insert(walk_probe_entity(id));
        }
        let mut ticked = Vec::new();
        es.update_all(|e| ticked.push(e.id));
        assert_eq!(
            ticked,
            vec![1, 2, 3, 4, 5],
            "the tick walks insertion order"
        );
        assert_eq!(
            es.all().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 4, 1, 3, 5],
            "the broadcast walks the hash of the ids"
        );

        // Inserted in a different order, the tick follows the insertions and
        // the broadcast does not move at all. That is the whole difference
        // between the linked map and the open one, on one entity set.
        let rev = Entities::new();
        for id in [5, 4, 3, 2, 1] {
            rev.insert(walk_probe_entity(id));
        }
        let mut ticked = Vec::new();
        rev.update_all(|e| ticked.push(e.id));
        assert_eq!(ticked, vec![5, 4, 3, 2, 1]);
        assert_eq!(
            rev.all().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 4, 1, 3, 5]
        );

        // A removal leaves the tick order of the survivors alone, and
        // re-inserting a live id does not move it to the back — both are
        // `Int2ObjectLinkedOpenHashMap.put`'s behaviour on an existing key.
        rev.remove(3);
        rev.insert(walk_probe_entity(5));
        let mut ticked = Vec::new();
        rev.update_all(|e| ticked.push(e.id));
        assert_eq!(ticked, vec![5, 4, 2, 1]);
    }

    /// The TICK order, graded against the jar's own linked map.
    ///
    /// `the_tick_order_and_the_broadcast_order_are_not_the_same_order` asserts
    /// that our tick walks insertion order, from first principles. This one
    /// asserts it against a **measurement**: `ci/probes/IterOrder.java` runs
    /// both fastutil maps over each id set and
    /// `ci/checks/entity-broadcast-order.json` carries the second column,
    /// `linked`, which is what `EntityTickList`'s
    /// `Int2ObjectLinkedOpenHashMap` yields. So this compares our entity tick
    /// against the container vanilla actually ticks with, on the same 41 sets
    /// the broadcast is graded on, rather than against a sentence about what
    /// insertion order means.
    ///
    /// The rows where the two columns differ are the ones that carry the
    /// weight — `{2 1}` and `{5 4 3 2 1}` walk their insertion order in the
    /// linked map and a hash of the ids in the open one — so a tick that had
    /// been quietly moved onto the broadcast container fails here.
    #[test]
    fn the_entity_tick_agrees_with_the_measured_linked_map() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ci/checks/entity-broadcast-order.json"
        );
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("the broadcast pin is unreadable at {path}: {e}"));
        let pin: serde_json::Value =
            serde_json::from_str(&text).expect("the broadcast pin is not json");
        let golden = pin["golden"]
            .as_object()
            .expect("the broadcast pin has no golden table");

        let mut checked = 0usize;
        let mut differing = 0usize;
        for (key, row) in golden {
            let ids: Vec<i32> = key
                .split_whitespace()
                .map(|s| s.parse().expect("a golden key holds a non-integer"))
                .collect();
            let want: Vec<i32> = row["linked"]
                .as_array()
                .expect("a golden row has no linked column")
                .iter()
                .map(|v| i32::try_from(v.as_i64().expect("linked is not an int")).expect("i32"))
                .collect();

            let es = Entities::new();
            for &id in &ids {
                es.insert(walk_probe_entity(id));
            }
            let mut ticked = Vec::new();
            es.update_all(|e| ticked.push(e.id));
            assert_eq!(ticked, want, "the entity tick diverged on {{{key}}}");

            checked += 1;
            let open: Vec<i32> = row["open"]
                .as_array()
                .expect("a golden row has no open column")
                .iter()
                .map(|v| i32::try_from(v.as_i64().expect("open is not an int")).expect("i32"))
                .collect();
            if open != want {
                differing += 1;
            }
        }
        assert!(checked >= 40, "only {checked} id sets graded");
        assert!(
            differing >= 15,
            "only {differing} of {checked} sets have the two containers \
             disagreeing, so this test would mostly pass on either walk"
        );
    }

    /// A READER for `ci/entity-broadcast-order.py`. It asserts nothing.
    ///
    /// # Why the check runs a test instead of reading the source
    ///
    /// Before §103 that check could state our walk from a source read: the
    /// container was a `BTreeMap` and "a BTreeMap walks ascending" is the
    /// language's contract, not an observation about this tree. That is gone.
    /// The container is now ours, so "it walks fastutil's order" is a claim
    /// about code we wrote, and a check that reads the source and then states
    /// the walk would be asserting the very thing it is supposed to grade —
    /// the unfalsifiable shape this tree files as a defect class.
    ///
    /// So the check executes this instead, and computes the divergence itself.
    /// **Nothing here may become an assertion.** If this test failed on a
    /// wrong walk, the check would get an empty stdout and refuse, where what
    /// it must do is go RED — `ci/entity-broadcast-order-falsify.sh` arm 5
    /// depends on exactly that difference.
    ///
    /// The id sets come from the pin, so there is one list rather than a copy
    /// on each side of the comparison.
    #[test]
    fn dump_broadcast_walks() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../ci/checks/entity-broadcast-order.json"
        );
        let Ok(text) = std::fs::read_to_string(path) else {
            // No pin, nothing to dump. The check refuses on a short answer,
            // which is the right verdict when its own pin is missing.
            return;
        };
        let Ok(pin) = serde_json::from_str::<serde_json::Value>(&text) else {
            return;
        };
        let Some(golden) = pin.get("golden").and_then(serde_json::Value::as_object) else {
            return;
        };
        let mut keys: Vec<&String> = golden.keys().collect();
        keys.sort();
        for key in keys {
            let ids: Vec<i32> = key
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            let es = Entities::new();
            for &id in &ids {
                es.insert(walk_probe_entity(id));
            }
            let walk: Vec<String> = es.all().iter().map(|e| e.id.to_string()).collect();
            println!("BCAST-WALK {}|{}", key, walk.join(" "));
        }
    }

    /// The entity the dump inserts. Its kind and position are irrelevant to
    /// the walk — only its id reaches the container — and saying so here is
    /// cheaper than a reader wondering whether a PLAYER and an ITEM would
    /// order differently. They would not: `ChunkMap.entityMap` is keyed on the
    /// id and on nothing else.
    fn walk_probe_entity(id: i32) -> Entity {
        Entity {
            id,
            uuid: u128::from(id.unsigned_abs()),
            kind: ITEM,
            x: 0.0,
            y: 0.0,
            z: 0.0,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: true,
            data: 0,
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            synched: crate::synched::Synched::EntityOnly,
        }
    }

    /// The registry itself walks `ChunkMap.entityMap`'s order, not ascending id.
    ///
    /// The unit tests in [`crate::openhash`] grade the container against 948
    /// measured vanilla histories. This one grades the **production path**:
    /// that `Entities` is actually backed by it and that `all()` — the call
    /// `Tracker::tick` makes — returns the walk rather than sorting it back
    /// into ascending order somewhere on the way out. A correct container
    /// behind a `sort_by_key` would pass every test in `openhash` and change
    /// nothing on the wire, which is the failure this covers.
    ///
    /// {1, 2} is one player and one dropped item, the smallest entity set this
    /// server can build and the one §101.5 named.
    #[test]
    fn the_level_entity_list_walks_in_vanillas_broadcast_order() {
        fn at(id: i32, kind: EntityKind) -> Entity {
            Entity {
                id,
                uuid: u128::from(id.unsigned_abs()),
                kind,
                x: 0.0,
                y: 0.0,
                z: 0.0,
                y_rot: 0.0,
                x_rot: 0.0,
                y_head_rot: 0.0,
                movement: (0.0, 0.0, 0.0),
                on_ground: true,
                data: 0,
                swinging: false,
                swing_time: 0,
                swinging_arm: None,
                synched: crate::synched::Synched::EntityOnly,
            }
        }
        let es = Entities::new();
        es.insert(at(1, PLAYER));
        es.insert(at(2, ITEM));
        assert_eq!(
            es.all().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 1],
            "a player and a dropped item broadcast in the order 2 1"
        );

        // And the row that shows insertion order does not reach it, which is
        // what separates this container from `EntityTickList`'s linked map.
        let rev = Entities::new();
        for id in [5, 4, 3, 2, 1] {
            rev.insert(at(id, ITEM));
        }
        assert_eq!(
            rev.all().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 4, 1, 3, 5]
        );

        // A removal takes the entity out of the walk and leaves the rest of
        // the order alone, which is `shiftKeys`' contract reaching the level.
        rev.remove(4);
        assert_eq!(
            rev.all().iter().map(|e| e.id).collect::<Vec<_>>(),
            vec![2, 1, 3, 5]
        );
    }

    /// Ids start at 1, never 0 — a client reads 0 as "no entity".
    #[test]
    fn entity_ids_start_at_one_and_do_not_repeat() {
        let e = Entities::new();
        assert_eq!(e.next_id(), 1);
        assert_eq!(e.next_id(), 2);
        assert_ne!(e.next_id(), e.next_id());
    }

    /// The section a negative coordinate is in, which floor division gets
    /// right and `as i32` does not.
    #[test]
    fn an_entity_south_west_of_the_origin_is_in_a_negative_section() {
        let e = Entity {
            id: 1,
            uuid: 0,
            kind: PLAYER,
            x: -0.5,
            y: -1.0,
            z: -0.5,
            y_rot: 0.0,
            x_rot: 0.0,
            y_head_rot: 0.0,
            movement: (0.0, 0.0, 0.0),
            on_ground: true,
            data: 0,
            // `LivingEntity`'s own initial values: not swinging, and a null
            // `swingingArm`, which `getCurrentSwingDuration` reads as the main
            // hand.
            swinging: false,
            swing_time: 0,
            swinging_arm: None,
            synched: crate::synched::Synched::Avatar {
                health: 20.0,
                model_customisation: 0,
                score: 0,
            },
        };
        assert_eq!(e.chunk(), (-1, -1));
        assert_eq!(e.section(), (-1, -1, -1));
    }

    /// A kind id is resolved by name against the capture, and a name the
    /// capture does not hold resolves to nothing rather than to zero.
    #[test]
    fn kind_ids_come_from_the_report_and_never_default() {
        // The real 26.2 numbers, from `generated/reports/registries.json`.
        // They are nothing like an alphabetical index, which is why the id
        // has to come from the report and cannot be derived from a position.
        let types: BTreeMap<String, i32> = [
            ("minecraft:item".to_string(), 71),
            ("minecraft:pig".to_string(), 100),
            ("minecraft:player".to_string(), 156),
        ]
        .into_iter()
        .collect();
        let ids = KindIds::resolve(&types, &[PLAYER, ITEM]);
        assert_eq!(ids.get(PLAYER), Some(156));
        assert_eq!(ids.get(ITEM), Some(71));

        let empty = KindIds::resolve(&BTreeMap::new(), &[PLAYER]);
        assert_eq!(empty.get(PLAYER), None, "absent, not 0");
        assert!(empty.is_empty());
    }
}
