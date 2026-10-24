//! `Level.playSound` — the clientbound sound packet, and the one argument
//! that decides who does *not* hear it.
//!
//! # The exclusion is the whole of the difficulty
//!
//! `Level.playSound(Entity, BlockPos, SoundEvent, SoundSource, float, float)`
//! reaches `ServerLevel.playSound`, which hands its first argument to
//! `PlayerList.broadcast(Player except, …)` — and `broadcast` skips `except`
//! (offsets 33-39: `if (players.get(i) == except) continue;`). So the entity
//! argument is not decoration and it is not the same at every site:
//!
//! ```text
//!   LeverBlock.useWithoutItem   pull(state, level, pos, null)     -> everybody
//!     -> LeverBlock.playSound(null, level, pos, state)
//!   ButtonBlock.playSound(player, level, pos, on), offsets 0-11:
//!     level.playSound(on ? player : null, pos, getSound(on), BLOCKS)
//!                     ^^^^^^^^^^^^^^^^^^
//!                     the PRESS excludes the presser; the RELEASE does not
//!   ChestBlockEntity's ContainerOpenersCounter.onOpen / onClose
//!     -> playSound(null, …)                                       -> everybody
//!   BlockItem.place, offsets 208-240:
//!     level.playSound(player, pos, getPlaceSound(state), BLOCKS, …)
//!                     ^^^^^^ always the placer                    -> everybody else
//!   Entity.playStepSound -> Player.playSound(sound, volume, pitch)
//!     level.playSound(this, getX(), getY(), getZ(), sound, PLAYERS, …)
//!                     ^^^^ Player's OVERRIDE; Entity's passes null
//!                                                                 -> everybody else
//! ```
//!
//! The last of those is the one that is invisible from `Entity` alone.
//! `Entity.playSound` has `aconst_null` in the entity slot, so reading only
//! that class gives "a step sound is heard by everybody" — and `Player`, four
//! classes down the hierarchy, overrides the method with `aload_0`.
//!
//! That asymmetry is not inferred. In the recorded trace pair of 2026-08-07
//! 02:19, `330-button-unpresses-itself` has **one** sound on the actor's
//! stream — `block.stone_button.click_off` at ~1000 ms — and **two** on the
//! observer's: `click_on` at ~49 ms and `click_off` at ~1000 ms. A server that
//! broadcast the press to everybody would buy the observer's stream and lose
//! the actor's, and both of that scenario's declared and strict projections
//! read the actor's.
//!
//! # Volume and pitch come from the call site, and one of them is random
//!
//! `LevelAccessor.playSound(Entity, BlockPos, SoundEvent, SoundSource)` — the
//! four-argument overload the button uses — is volume 1.0 and pitch 1.0. The
//! lever passes 0.3 and `powered ? 0.6f : 0.5f` explicitly. The chest passes
//! 0.5 and `level.random.nextFloat() * 0.1f + 0.9f`, **a fresh draw per
//! sound**: the same event was measured at 0.9120, 0.9268, 0.9339, 0.9552 and
//! 0.9952 within one corpus run. No implementation can reproduce that draw
//! without reproducing the level's random sequence, which is why
//! `tools/oracle-runner`'s trace stopped comparing the pitch field in the same
//! commit that added this module — the same move it had already made for the
//! packet's `long seed`. The pitch is still *produced* faithfully in shape
//! (0.9..1.0 for a chest) rather than pinned to a constant, because a constant
//! would be a claim about vanilla that is false.
//!
//! **The trace compares it again as of 2026-08-17, and what it compares is the
//! call site rather than the float.** `tools/oracle-runner/src/pitch.rs`
//! classifies each event: a literal pitch exactly, a drawn pitch as the range
//! it is drawn from, an unclassified event not at all. So the chest's draw is
//! still not compared as a number — that part of `569323e` stands — while the
//! lever's `powered ? 0.6f : 0.5f` and the comparator's
//! `SUBTRACT ? 0.55f : 0.5f` are compared, which they had to be: for those two
//! events the pitch is the **only** field that differs between the two
//! directions, so their `sound` observations previously said nothing about
//! which way the block went.
//!
//! # The radius, which used to be in the list below
//!
//! `ServerLevel.playSeededSound` offsets 41-46 pass
//! `sound.value().getRange(volume)` to `PlayerList.broadcast` as a distance,
//! and `broadcast` (offsets 58-110) sends only to players inside it:
//! `dx*dx + dy*dy + dz*dz < range*range`, strictly. [`send`] does that now.
//! The rule itself is [`crate::sound_events::range`] and the fixed ranges it
//! needs are a generated table, because `getRange` is
//! `fixedRange.orElse(volume > 1.0f ? 16.0f * volume : 16.0f)` and nothing
//! about a sound's name says which events have one.
//!
//! **What that was costing, measured before the port.** In the corpus run of
//! 2026-08-09, `530-walking-away-drops-a-standing-player-from-view` had
//! vanilla put **22** step sounds on the observer's stream and this server
//! **68** — 51 of ours more than 16 blocks from the workspace where
//! vanilla's stop at 20. The difference was the radius and nothing else.
//! Before the step sound existed it cost nothing observable, because every
//! sound this server played was at a block the actor was standing next to; a
//! walking player is what made it visible.
//!
//! Two things about the port are worth knowing before trusting it:
//!
//! * **an unlocatable listener is sent the sound, not denied it.** A
//!   connection whose entity is not in [`crate::entities::Entities`] — the
//!   window between `players.join` and the `entities.insert` a few lines
//!   later, and every connection in a test that never inserts one — has no
//!   position, and there is no honest distance for it. Vanilla has no such
//!   state: `ServerPlayer.getX()` always answers. Silencing them would make
//!   the failure mode of a missing entity *quiet*, which is the one failure
//!   mode nothing downstream can tell apart from the feature not existing.
//! * **the dimension test is not reproduced**, because this server has one
//!   dimension. It belongs in [`send`] before the distance when there are
//!   two.
//!
//! # What is not here
//!
//! * **`SOUND_ENTITY`**, the entity-relative packet. Nothing here plays one.
//! * **Inline sound events.** The packet can carry a whole `SoundEvent`
//!   instead of a registry id; only datapack sounds need it.
//! * **Break, hit and fall sounds.** [`crate::sound_types`] carries all five
//!   events per block; only `place` and `step` have a call site here. A dig
//!   plays its break sound through `Block.spawnDestroyParticles`' level event
//!   rather than through this packet, which `crate::shapes` already emits.

use crate::interact::BlockPos;
use crate::players::Players;

/// `SoundSource.BLOCKS`'s ordinal, which is what the packet carries.
///
/// From `SoundSource`'s own declaration order — `MASTER, MUSIC, RECORDS,
/// WEATHER, BLOCKS, HOSTILE, NEUTRAL, PLAYERS, AMBIENT, VOICE` — and
/// corroborated by every `sound` observation in the recorded traces, all of
/// which carry `source: 4`.
pub const SOURCE_BLOCKS: i32 = 4;

/// `SoundSource.RECORDS`'s ordinal, from the same declaration order.
///
/// Two *below* `BLOCKS` in the declaration order, and the note block is why
/// it is here:
/// `NoteBlock.triggerEvent` offset 165 is `getstatic SoundSource.RECORDS`,
/// not `BLOCKS`, which is not a guess anybody makes correctly. Corroborated
/// by every `block.note_block.harp` observation in the recordings on this
/// box, all 60 of which carry `source: 2`.
pub const SOURCE_RECORDS: i32 = 2;

/// `SoundSource.PLAYERS`'s ordinal, from the same declaration order.
///
/// A step sound is **not** a `BLOCKS` sound even though it names a block:
/// `Entity.playSound` passes `getSoundSource()`, and `Player.getSoundSource`
/// returns `PLAYERS`. Corroborated by the recording — 080's two step sounds
/// both carry `source: 7` where every block sound in the corpus carries 4.
pub const SOURCE_PLAYERS: i32 = 7;

/// `SoundSource.NEUTRAL`'s ordinal — `Entity.getSoundSource()`'s default,
/// the source an item frame's placement sound goes out on.
pub const SOURCE_NEUTRAL: i32 = 6;

/// A `SoundSource`'s wire ordinal from the name `UseGolden` writes.
///
/// The extract records `SoundSource.getName()` — `"block"`, `"player"` — and
/// the packet carries the ordinal, so a table-driven sound needs the map
/// between them. Read off `SoundSource`'s own `<clinit>` in the pinned 26.2
/// jar, where each constant's `ldc` of its name sits beside its `putstatic`:
///
/// ```text
///   master 0   music 1   record 2   weather 3   block 4    hostile 5
///   neutral 6  player 7  ambient 8  voice 9     ui 10
/// ```
///
/// **`UI` is in this list and was not in the two constants above it**, whose
/// doc comments stop at `VOICE`. That is a stale enumeration rather than a
/// disagreement — nothing reads `UI` here — but a table that mapped a name
/// this function did not know would otherwise get a silent default, so
/// unknown names return `None` and refuse at load instead.
///
/// **Not `as i32` on a Rust enum.** There is no `SoundSource` type in this
/// server; inventing one whose declaration order had to be kept in step with
/// vanilla's by hand is exactly the coupling this table exists to remove.
#[must_use]
pub fn source_ordinal(name: &str) -> Option<i32> {
    Some(match name {
        "master" => 0,
        "music" => 1,
        "record" => 2,
        "weather" => 3,
        "block" => SOURCE_BLOCKS,
        "hostile" => 5,
        "neutral" => 6,
        "player" => SOURCE_PLAYERS,
        "ambient" => 8,
        "voice" => 9,
        "ui" => 10,
        _ => return None,
    })
}

/// A sound's position on the wire: three ints in **eighths of a block**.
///
/// `ClientboundSoundPacket` writes `(int)(x * 8.0)`, and the two overloads
/// that reach it disagree about what `x` is:
///
/// ```text
///   Level.playSound(Entity, BlockPos, …)   x + 0.5   — the block's centre
///   Level.playSound(Entity, D, D, D, …)    x         — wherever the caller says
/// ```
///
/// A block sound therefore lands on a block centre and an entity's sound
/// lands wherever the entity is, which for a walking player is a fraction.
/// Carrying eighths rather than a `BlockPos` is what lets one packet writer
/// serve both, and [`Eighths::block`] keeps the block case in integers so it
/// cannot pick up a rounding of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eighths(pub i32, pub i32, pub i32);

impl Eighths {
    /// A block's centre: `8x + 4` per axis, in integers.
    ///
    /// The multiply-add is deliberate. `((x as f64) + 0.5) * 8.0` is exact for
    /// every coordinate this server can reach, but it is exact by an argument
    /// about doubles rather than by construction, and the axis this got wrong
    /// once was a negative one.
    #[must_use]
    pub const fn block(pos: BlockPos) -> Self {
        Self(
            pos.0.wrapping_mul(8).wrapping_add(4),
            pos.1.wrapping_mul(8).wrapping_add(4),
            pos.2.wrapping_mul(8).wrapping_add(4),
        )
    }

    /// An entity's own position, truncated toward zero exactly as the
    /// packet's `(int)(x * 8.0)` cast does.
    #[must_use]
    pub fn exact(x: f64, y: f64, z: f64) -> Self {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the packet's own narrowing cast, which is what is being reproduced"
        )]
        fn e(v: f64) -> i32 {
            (v * 8.0) as i32
        }
        Self(e(x), e(y), e(z))
    }
}

/// `Level.playSound(Entity, BlockPos, …)`'s position as a double triple —
/// `pos.getX() + 0.5` per axis, from the `i2d`/`dconst_0.5`/`dadd` at its
/// offsets 4-16.
///
/// The eighths form of the same point is [`Eighths::block`]; this is the form
/// `PlayerList.broadcast` measures distances in. They are the same number:
/// `x + 0.5` is exact in an `f64` for every block coordinate this world
/// reaches, and `8x + 4` is exactly eight times it.
#[must_use]
pub fn block_centre(pos: BlockPos) -> (f64, f64, f64) {
    (
        f64::from(pos.0) + 0.5,
        f64::from(pos.1) + 0.5,
        f64::from(pos.2) + 0.5,
    )
}

/// One `ClientboundSoundPacket` waiting to go out.
#[derive(Debug, Clone, PartialEq)]
pub struct Sound {
    /// The sound event's registry **name**. Resolved to a number at send
    /// time through `crate::protocol_ids`, for the reason that module exists:
    /// `minecraft:sound_event` is a built-in registry, never sent to a
    /// client, so its numbering has to come from vanilla's own report.
    ///
    /// A `String` rather than a `&'static str` because the two table-driven
    /// sounds — place and step — take their event from
    /// [`crate::sound_types`], and a table read from a file cannot hand out
    /// `'static` strs without leaking them.
    pub event: String,
    /// `SoundSource`'s ordinal.
    pub source: i32,
    /// Where the sound is, in eighths of a block. See [`Eighths`] for why
    /// this is not a `BlockPos`: a block sound is at the block's **centre**
    /// — the traces show `[1.5, 0.5, 0.5]` for a block at `[1, 0, 0]` — and
    /// an entity's sound is at the entity.
    pub at: Eighths,
    /// The same position in **doubles**, which is what the radius test uses.
    ///
    /// Not derived from [`Self::at`], and that is the point.
    /// `PlayerList.broadcast` compares `dx*dx + dy*dy + dz*dz < range*range`
    /// against the doubles `ServerLevel.playSeededSound` was handed, while
    /// `at` has already been through the packet's `(int)(x * 8.0)`. For a
    /// block the two agree exactly — `8x + 4` is `8 * (x + 0.5)` — but for an
    /// entity the cast truncates toward zero, so recovering the position by
    /// dividing `at` by eight would move a walking player by up to an eighth
    /// of a block and could flip a listener sitting on the boundary. Carrying
    /// both costs three doubles and removes the question.
    pub pos: (f64, f64, f64),
    pub volume: f32,
    pub pitch: f32,
    /// `PlayerList.broadcast`'s `except`: the connection id of the player
    /// this sound is **not** sent to, if any. `None` is vanilla's `null`.
    pub except: Option<u64>,
}

/// `LeverBlock.playSound`, offsets 0-43.
///
/// ```text
///   float pitch = state.getValue(POWERED) ? 0.6f : 0.5f;
///   level.playSound(player, pos, SoundEvents.LEVER_CLICK, SoundSource.BLOCKS, 0.3f, pitch);
/// ```
///
/// `player` is `null` on every path a right-click can reach:
/// `LeverBlock.useWithoutItem` calls `pull(state, level, pos, null)` —
/// `aconst_null` at offset 51 — so the flipper hears its own lever. Measured:
/// `290`, `360` and `553` all carry the same sound on both streams.
///
/// `state` is the state **after** the flip, which is what `pull` passes.
#[must_use]
pub fn lever_click(pos: BlockPos, powered: bool) -> Sound {
    Sound {
        event: "minecraft:block.lever.click".into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: 0.3,
        pitch: if powered { 0.6 } else { 0.5 },
        except: None,
    }
}

/// `ButtonBlock.playSound(player, level, pos, on)` with `ButtonBlock.getSound(on)`.
///
/// `getSound` is `type.buttonClickOn()` / `buttonClickOff()` — a property of
/// the block's `BlockSetType`, not of its name — and `BlockSetType`'s static
/// initialiser carries the whole mapping. Read out of it (`javap -p -c
/// -constants`, the `<clinit>`, matching each `ldc` of the set type's name to
/// the `SoundEvents.*_BUTTON_CLICK_OFF` that follows it):
///
/// ```text
///   STONE_BUTTON_CLICK_*        iron, copper, gold, stone, polished_blackstone
///   CHERRY_WOOD_BUTTON_CLICK_*  cherry
///   NETHER_WOOD_BUTTON_CLICK_*  crimson, warped
///   BAMBOO_WOOD_BUTTON_CLICK_*  bamboo
///   WOODEN_BUTTON_CLICK_*       every other set type — the default the
///                               register helper supplies at offsets 29/32
/// ```
///
/// Three of those set types (`iron`, `copper`, `gold`) have no button block in
/// 26.2, so they cannot arrive here; they are listed because the mapping is
/// the set type's and leaving them out would make the rule look like a
/// statement about which blocks sound stony.
///
/// **`except` is the presser and only on the press.** The entity argument is
/// `on ? player : null` at offsets 0-11.
#[must_use]
pub fn button_click(block: &str, pos: BlockPos, on: bool, presser: Option<u64>) -> Sound {
    let family = match block.strip_prefix("minecraft:").unwrap_or(block) {
        "iron_button"
        | "copper_button"
        | "gold_button"
        | "stone_button"
        | "polished_blackstone_button" => "stone_button",
        "cherry_button" => "cherry_wood_button",
        "crimson_button" | "warped_button" => "nether_wood_button",
        "bamboo_button" => "bamboo_wood_button",
        _ => "wooden_button",
    };
    // `&'static str` without an allocation and without a second table: the
    // ten combinations are the whole space, so they are written out and the
    // compiler checks the match is total.
    let event = match (family, on) {
        ("stone_button", true) => "minecraft:block.stone_button.click_on",
        ("stone_button", false) => "minecraft:block.stone_button.click_off",
        ("cherry_wood_button", true) => "minecraft:block.cherry_wood_button.click_on",
        ("cherry_wood_button", false) => "minecraft:block.cherry_wood_button.click_off",
        ("nether_wood_button", true) => "minecraft:block.nether_wood_button.click_on",
        ("nether_wood_button", false) => "minecraft:block.nether_wood_button.click_off",
        ("bamboo_wood_button", true) => "minecraft:block.bamboo_wood_button.click_on",
        ("bamboo_wood_button", false) => "minecraft:block.bamboo_wood_button.click_off",
        (_, true) => "minecraft:block.wooden_button.click_on",
        (_, false) => "minecraft:block.wooden_button.click_off",
    };
    Sound {
        event: event.into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        // The four-argument `LevelAccessor.playSound(Entity, BlockPos,
        // SoundEvent, SoundSource)` overload, which is 1.0 and 1.0.
        volume: 1.0,
        pitch: 1.0,
        except: if on { presser } else { None },
    }
}

/// `ComparatorBlock.useWithoutItem`, offsets 15-62.
///
/// ```text
///    15-25  state = state.cycle(MODE)
///    26-46  float pitch = state.getValue(MODE) == SUBTRACT ? 0.55f : 0.5f
///    48-62  level.playSound(player, pos, SoundEvents.COMPARATOR_CLICK,
///                           SoundSource.BLOCKS, 0.3f, pitch)
///    65-71  level.setBlock(pos, state, 2)
/// ```
///
/// **Three things here are not the lever's, and each is a way to get this
/// wrong.**
///
/// * `player` is passed, not `null`, so `PlayerList.broadcast` skips the
///   player who clicked and the sound is evidence **only on a bystander's
///   stream**. `LeverBlock.pull` passes `aconst_null` and the flipper hears
///   its own lever; a comparator's clicker does not hear its own comparator.
///   `420` is `observe_from` the observer for exactly this reason, and read
///   from the actor alone it would be inert.
/// * the pitches are 0.55/0.5, not the lever's 0.6/0.5, and they are keyed on
///   `MODE` rather than on `POWERED`.
/// * the pitch is read from the **cycled** state — `astore_1` at 25 replaces
///   the local before `getValue` at 30 — so it describes the mode the
///   comparator is going *to*. `use_sound` is handed the table's write target,
///   which carries the same `mode`: `refreshOutputState` at 85-89 can move
///   `POWERED` afterwards and never touches `MODE`.
///
/// The sound is played **before** `setBlock` here and after it in
/// `LeverBlock` and `ButtonBlock`. That is invisible on the wire and is not
/// reproduced: a `playSound` reaches `PlayerList.broadcast` with no queue on
/// the path, while a block change is a tick-boundary event out of
/// `ServerChunkCache.tick`, so the sound precedes the update either way. Said
/// here because the ordering *looks* like a difference and is not one.
#[must_use]
pub fn comparator_click(pos: BlockPos, subtract: bool, clicker: Option<u64>) -> Sound {
    Sound {
        event: "minecraft:block.comparator.click".into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: 0.3,
        pitch: if subtract { 0.55 } else { 0.5 },
        except: clicker,
    }
}

/// `FenceGateBlock.useWithoutItem`, offsets 102-162 — the fourth family.
///
/// ```text
///  102-115  boolean open = state.getValue(OPEN);          // the WRITTEN state
///  117-143  level.playSound(player, pos,
///                           open ? type.fenceGateOpen() : type.fenceGateClose(),
///                           SoundSource.BLOCKS, 1.0f,
///                           level.getRandom().nextFloat() * 0.1f + 0.9f);
/// ```
///
/// Three things, and two of them are the button's traps by another door:
///
/// * `player` is passed, so `PlayerList.broadcast` skips the clicker and this
///   sound is evidence **only on a bystander's stream**. Both `700` and `701`
///   `observe` `sound` from the *actor*, and both were `match` on the declared
///   projection with `missing_kinds: ["sound"]` printed beside them **before
///   this function existed** — the advisory is one-sided by construction and
///   `701`'s own header says so. Measured, 2026-08-11, `~/rustmc-stack`
///   report: vanilla's `sound` for both checks is on the observer's stream at
///   `at [1.5, 0.5, 0.5]`, `source: 4`, `volume: 1.0`, event id 625.
/// * `open` is read off the state that was just **written**, not the one
///   clicked, so a gate going open sounds `open` — the same ordering the
///   comparator's `MODE` has.
/// * the pitch is a fresh `nextFloat() * 0.1f + 0.9f` draw per sound —
///   character for character the chest's, which is why [`nine_tenths_pitch`]
///   is named after the formula rather than after the chest. It is taken as
///   an argument for the same reason `chest_open` takes one: a function that
///   drew its own would have no deterministic test.
///
/// The wood-type families are `WoodType`'s own `<clinit>`, read off the jar
/// rather than inferred from the button's: **cherry** takes
/// `CHERRY_WOOD_FENCE_GATE_*`, **crimson and warped** share
/// `NETHER_WOOD_FENCE_GATE_*`, **bamboo** takes `BAMBOO_WOOD_FENCE_GATE_*`,
/// and the other eight — oak, spruce, birch, acacia, jungle, dark oak, pale
/// oak, mangrove — take the plain `FENCE_GATE_*`. Note that this is **not**
/// the button's split: a stone button exists and a stone fence gate does not,
/// and pale oak is a plain gate where it is a `wooden_button`.
#[must_use]
pub fn fence_gate(
    block: &str,
    pos: BlockPos,
    open: bool,
    clicker: Option<u64>,
    pitch: f32,
) -> Sound {
    let family = match block.strip_prefix("minecraft:").unwrap_or(block) {
        "cherry_fence_gate" => "cherry_wood_fence_gate",
        "crimson_fence_gate" | "warped_fence_gate" => "nether_wood_fence_gate",
        "bamboo_fence_gate" => "bamboo_wood_fence_gate",
        _ => "fence_gate",
    };
    let event = match (family, open) {
        ("cherry_wood_fence_gate", true) => "minecraft:block.cherry_wood_fence_gate.open",
        ("cherry_wood_fence_gate", false) => "minecraft:block.cherry_wood_fence_gate.close",
        ("nether_wood_fence_gate", true) => "minecraft:block.nether_wood_fence_gate.open",
        ("nether_wood_fence_gate", false) => "minecraft:block.nether_wood_fence_gate.close",
        ("bamboo_wood_fence_gate", true) => "minecraft:block.bamboo_wood_fence_gate.open",
        ("bamboo_wood_fence_gate", false) => "minecraft:block.bamboo_wood_fence_gate.close",
        (_, true) => "minecraft:block.fence_gate.open",
        (_, false) => "minecraft:block.fence_gate.close",
    };
    Sound {
        event: event.into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: 1.0,
        pitch,
        except: clicker,
    }
}

/// `ChestBlockEntity`'s open sound —
/// `ContainerOpenersCounter.onOpen` -> `playSound(state, SoundEvents.CHEST_OPEN)`.
///
/// ```text
///   double x = pos.getX() + 0.5, y = pos.getY() + 0.5, z = pos.getZ() + 0.5;
///   level.playSound(null, x, y, z, sound, SoundSource.BLOCKS,
///                   0.5f, level.getRandom().nextFloat() * 0.1f + 0.9f);
/// ```
///
/// `null`, so the player who opened it hears it — which is what `090`, `490`,
/// `560`, `561` and `602` all show on both streams.
///
/// The double-chest offset (`ChestBlock.getConnectedDirection`, which moves
/// the source half a block towards the other half) is **not** applied: no
/// check in the corpus opens a double chest, and guessing the direction of an
/// offset is worse than not applying one. `240-chest-pairs-with-its-neighbour`
/// places a pair and never opens it.
#[must_use]
pub fn chest_open(pos: BlockPos, pitch: f32) -> Sound {
    Sound {
        event: "minecraft:block.chest.open".into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: 0.5,
        pitch,
        except: None,
    }
}

/// `ShulkerBoxBlockEntity.startOpen`'s sound, offsets 92-137:
/// `playSound(null, worldPosition, SHULKER_BOX_OPEN, BLOCKS, 0.5f,
/// nextFloat() * 0.1f + 0.9f)` — the chest's volume and pitch shape under a
/// different event, and played from the block entity rather than from a
/// `ContainerOpenersCounter`.
#[must_use]
pub fn shulker_box_open(pos: BlockPos, pitch: f32) -> Sound {
    Sound {
        event: "minecraft:block.shulker_box.open".into(),
        ..chest_open(pos, pitch)
    }
}

/// `ShulkerBoxBlockEntity.stopOpen`'s sound — `SHULKER_BOX_CLOSE`, same
/// volume and pitch shape as [`shulker_box_open`], played once the count has
/// fallen to zero or below.
#[must_use]
pub fn shulker_box_close(pos: BlockPos, pitch: f32) -> Sound {
    Sound {
        event: "minecraft:block.shulker_box.close".into(),
        ..chest_open(pos, pitch)
    }
}

/// `BlockItem.place`'s sound, offsets 201-240.
///
/// ```text
///   201: SoundType t = level.getBlockState(pos).getSoundType();
///   208: level.playSound(player, pos, getPlaceSound(state), SoundSource.BLOCKS,
///   223:                 (t.getVolume() + 1.0F) / 2.0F,       // fconst_1 fadd fconst_2 fdiv
///   232:                 t.getPitch() * 0.8F);                // ldc 0.8f fmul
/// ```
///
/// # The exclusion is the whole risk of this function
///
/// `aload 6` at offset 210 is the **placing player**, and
/// `PlayerList.broadcast` skips `except`. In the corpus of 2026-08-08, 32
/// scenarios carry a `*.place` sound on vanilla and in all 32 it is on the
/// *observer's* stream only — zero exceptions. Twenty-one of those read a
/// strict `match`, so a place sound sent to everybody would convert nothing
/// and buy twenty-one scenarios straight back to `diverge`. That is why
/// `placer` is not an `Option` here: there is no path in vanilla that places a
/// block with a null player, and making the caller able to express one would
/// make the mistake expressible.
///
/// The `SoundType` is the one on the state **read back from the level** after
/// the write (`aload 8`), not the state the placement asked for. The two
/// differ for a block whose `updateBlockStateFromTag` changed it, and for a
/// placement the level refused to store as asked.
#[must_use]
pub fn block_place(row: &crate::sound_types::Row, pos: BlockPos, placer: u64) -> Sound {
    Sound {
        event: row.place_sound.clone(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: (row.volume + 1.0) / 2.0,
        pitch: row.pitch * 0.8,
        except: Some(placer),
    }
}

/// `Entity.playStepSound`, offsets 0-25, through `Player.playSound`.
///
/// ```text
///   Entity.playStepSound(pos, state):
///     SoundType t = state.getSoundType();
///     playSound(t.getStepSound(), t.getVolume() * 0.15F, t.getPitch());
///   Player.playSound(sound, volume, pitch):                 // OVERRIDDEN
///     level.playSound(this, getX(), getY(), getZ(), sound, getSoundSource(), …)
///                     ^^^^
/// ```
///
/// **`Player`'s override passes `this` where `Entity`'s passes
/// `aconst_null`.** That one `aload_0` is why a step sound is on every stream
/// except the walker's, and it is the same shape of finding as the button's
/// press/release asymmetry: the sound is right, the position is right, and a
/// version without the exclusion is wrong on every scenario that observes the
/// actor.
///
/// The position is the player's own `x, y, z` — not a block centre — and the
/// source is `PLAYERS`, from `Player.getSoundSource`. Both are visible in
/// 080's recording: `at` is exactly the hop target and `source` is 7.
///
/// The pitch is **not** scaled. Only the volume is, by `0.15f`.
#[must_use]
pub fn player_step(row: &crate::sound_types::Row, x: f64, y: f64, z: f64, walker: u64) -> Sound {
    Sound {
        event: row.step_sound.clone(),
        source: SOURCE_PLAYERS,
        at: Eighths::exact(x, y, z),
        pos: (x, y, z),
        volume: row.volume * 0.15,
        pitch: row.pitch,
        except: Some(walker),
    }
}

/// `level.getRandom().nextFloat() * 0.1f + 0.9f`, from **our** random.
///
/// # This is not vanilla's number and it is not meant to be
///
/// Vanilla draws from `ServerLevel`'s `RandomSource`, whose sequence depends
/// on everything else that has asked it for anything since the world loaded.
/// There is no way to reproduce that draw short of reproducing the whole
/// level's random consumption, and this project's rule is that an
/// unreproducible dimension is removed from the comparison rather than
/// approximated by a constant — the packet's own `long seed` was removed for
/// exactly this reason, and the pitch was removed beside it.
///
/// What is reproduced is the **range**: a chest-open pitch is in `[0.9, 1.0)`
/// here as it is there, so a client hears the same variation and nothing
/// downstream can be written against a fixed value it would then depend on.
///
/// **And since 2026-08-17 that range is what the trace compares**, so this
/// paragraph stopped being a statement of intent and became the thing under
/// test. `tools/oracle-runner/src/pitch.rs` reads a chest-open pitch as its
/// draw only while it lies in `[0.9, 1.0)`; a value outside survives as a
/// number and diverges. Pinning this function to a constant would now fail a
/// gameplay scenario as well as being false.
///
/// Named for the formula and not for the chest, because two unrelated call
/// sites write it character for character —
/// `ContainerOpenersCounter.onOpen`/`onClose` and
/// `FenceGateBlock.useWithoutItem` at offsets 147-161. It was `chest_pitch`
/// until the second one arrived.
#[must_use]
pub fn nine_tenths_pitch() -> f32 {
    drawn_pitch(0.1, 0.9)
}

/// `level.random.nextFloat() * scale + base` — the shape every drawn pitch in
/// vanilla has, with the two constants the call site chose.
///
/// The constants are not decoration and the two piston sounds are the reason
/// this is a function rather than one more copy: `PISTON_EXTEND` is
/// `nextFloat() * 0.25f + 0.6f` at offsets 131-141 and `PISTON_CONTRACT` is
/// `nextFloat() * 0.15f + 0.6f` at 544-555. Same base, different scale, forty
/// bytecode offsets apart, and a port that read one and assumed the other
/// would be wrong about the retraction only.
///
/// `mul_add` rather than `a * b + c` because `Math.fma` is not what vanilla
/// does — the Java is `fmul; fadd`, two roundings — but the *value* here is
/// nobody's parity claim: see [`nine_tenths_pitch`] on why the draw is
/// deliberately not reproducible and why `tools/oracle-runner` stopped
/// comparing this field. What is claimed is the **range**, and a `mul_add`
/// and a `fmul; fadd` agree on that to the last bit at these magnitudes.
#[must_use]
pub fn drawn_pitch(scale: f32, base: f32) -> f32 {
    use std::sync::atomic::{AtomicU64, Ordering};
    // SplitMix64, seeded from nothing in particular. A sound's pitch is the
    // one place in this server where a value is *supposed* to be arbitrary,
    // so it does not go through `crate::ticks`' position hash or any other
    // reproducible source: making it reproducible would invite a check to be
    // written against it.
    static STATE: AtomicU64 = AtomicU64::new(0x9E37_79B9_7F4A_7C15);
    let mut z = STATE
        .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
        .wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    // `nextFloat()` is 24 bits over 2^24, which is what this reproduces.
    #[expect(
        clippy::cast_precision_loss,
        reason = "24 bits into an f32 mantissa, exactly"
    )]
    let unit = ((z >> 40) as f32) / 16_777_216.0;
    unit.mul_add(scale, base)
}

/// `PistonBaseBlock.triggerEvent`'s two sounds.
///
/// ```text
///   extend   (offsets 118-142, after the setBlock that follows a successful moveBlocks)
///     level.playSound(null, pos, PISTON_EXTEND,   BLOCKS, 0.5f, rnd.nextFloat()*0.25f + 0.6f)
///   contract (offsets 531-556, after the head is removed)
///     level.playSound(null, pos, PISTON_CONTRACT, BLOCKS, 0.5f, rnd.nextFloat()*0.15f + 0.6f)
/// ```
///
/// `aconst_null` in the entity slot on both — a piston is not pushed by
/// anybody, so there is nobody to exclude, and the player who powered it
/// hears it like everyone else.
///
/// **The scales differ and the bases do not.** 0.25 for the push, 0.15 for
/// the pull, both onto 0.6. That is the only difference between the two call
/// sites besides the event, and it is the kind of thing a port derives from
/// one of them and gets wrong in the other.
///
/// The volume is 0.5, which matters beyond loudness: `SoundEvent.getRange`
/// takes `volume > 1.0f` before it multiplies, so both of these carry exactly
/// 16 blocks — the flat default — and not eight.
#[must_use]
pub fn piston_move(pos: BlockPos, extending: bool) -> Sound {
    Sound {
        event: if extending {
            "minecraft:block.piston.extend".into()
        } else {
            "minecraft:block.piston.contract".into()
        },
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: 0.5,
        pitch: drawn_pitch(if extending { 0.25 } else { 0.15 }, 0.6),
        except: None,
    }
}

/// `ClientboundSoundPacket`'s body, for the block-positioned form.
///
/// From the packet's own `RegistryFriendlyByteBuf` constructor, in order:
/// the sound as a `Holder` — a varint that is `registry id + 1`, with 0
/// meaning an inline event follows — then the source ordinal as a varint,
/// then the position as **three big-endian ints in eighths of a block**, then
/// volume and pitch as floats, then a `long seed`.
///
/// The eighths are the trap: `ClientboundSoundPacket` writes
/// `(int)(x * 8.0)`, so a block centre at `x + 0.5` travels as `8x + 4` and
/// a position written in whole blocks would be eight times too close to the
/// origin.
///
/// The seed is `soundSeedGenerator.nextLong()` in vanilla and zero here.
/// `tools/oracle-runner`'s trace reads it and throws it away, so it is not
/// compared; writing a fresh random one instead would change nothing anybody
/// can observe and would make this function untestable.
///
/// # Errors
/// On a write failure, which for a `Vec` cannot happen.
pub fn sound_body(sound: &Sound, id: i32) -> std::io::Result<Vec<u8>> {
    use rustmc_proto::varint::write_varint;
    use rustmc_proto::wire;
    let mut out = Vec::new();
    write_varint(&mut out, id + 1)?;
    write_varint(&mut out, sound.source)?;
    for v in [sound.at.0, sound.at.1, sound.at.2] {
        // Already in eighths — see `Eighths`, which is where the `8x + 4` for
        // a block centre and the `(int)(x * 8.0)` for an entity both live.
        wire::write_i32(&mut out, v)?;
    }
    wire::write_f32(&mut out, sound.volume)?;
    wire::write_f32(&mut out, sound.pitch)?;
    wire::write_i64(&mut out, 0)?;
    Ok(out)
}

/// Send a batch of sounds, honouring each one's exclusion and its radius.
///
/// `PlayerList.broadcast(Player except, double x, double y, double z,
/// double range, ResourceKey dimension, Packet)`, offsets 0-129, for every
/// sound in the batch:
///
/// ```text
///   for (i = 0; i < players.size(); i++) {
///       ServerPlayer p = players.get(i);
///       if (p == except) continue;
///       if (p.level().dimension() != dimension) continue;
///       double dx = x - p.getX(), dy = y - p.getY(), dz = z - p.getZ();
///       if (dx*dx + dy*dy + dz*dz < range*range) p.connection.send(packet);
///   }
/// ```
///
/// The three arguments beyond the sounds are the three things that rule needs
/// and `Players` does not carry: the id table to number the event, the
/// event table to look its `fixedRange` up in, and the entity registry to
/// find each listener. `events: None` falls back to
/// [`crate::sound_events::range`]`(None, volume)`, which is right for every
/// event without a fixed range; `entities: None` skips the distance test
/// entirely and broadcasts, which is what this function did before the radius
/// existed and is what a unit test that has built no entities gets.
///
/// Silent when the protocol-id table is missing or the event is not in it —
/// the same rule every other registry-numbered packet here follows, and
/// `crate::protocol_ids::shared` has already printed by then.
///
/// **The table is an argument and not `protocol_ids::shared()`**, which is not
/// a style preference. Every other id-carrying packet in this server takes the
/// table the caller was handed (`Session::protocol_ids`, or the one the tick
/// loop resolved once at startup), and a version of this that reached for the
/// global read a file that does not exist under `cargo test` — so the menu
/// order test asserted a sound that could never be sent and would have gone on
/// asserting it against a server that had stopped sending any.
pub fn send(
    players: &Players,
    ids: Option<&crate::protocol_ids::ProtocolIds>,
    events: Option<&crate::sound_events::SoundEventTable>,
    entities: Option<&crate::entities::Entities>,
    sounds: &[Sound],
) {
    if sounds.is_empty() {
        return;
    }
    let Some(ids) = ids else {
        return;
    };
    // One snapshot for the whole batch, for the reason `Players::connections`
    // gives: two walks of the list under separate locks can disagree about
    // who is connected, and a batch that sent its first sound to one audience
    // and its second to another would be a divergence nobody could
    // reproduce.
    let conns = players.connections();
    let locate = crate::playerlist::entity_locator(entities);
    for s in sounds {
        let Some(id) = ids.sound(&s.event) else {
            continue;
        };
        let Ok(body) = sound_body(s, id) else {
            continue;
        };
        let radius = f64::from(events.map_or_else(
            || crate::sound_events::range(None, s.volume),
            |t| t.range_of(&s.event, s.volume),
        ));
        // The exclusion, the dimension test and the distance all live in
        // `crate::playerlist`, because `ServerLevel.levelEvent` reaches the
        // same `PlayerList.broadcast` with a different radius and a copy here
        // would be two implementations of one loop.
        crate::playerlist::broadcast_within(
            &conns,
            &locate,
            s.except,
            s.pos,
            radius,
            rustmc_proto::ids::play::cb::SOUND,
            &body,
        );
    }
}

/// `BellBlock.attemptToRing`'s sound, offsets 57-68.
///
/// ```text
///   level.playSound(null, pos, SoundEvents.BELL_BLOCK, SoundSource.BLOCKS, 2.0f, 1.0f);
/// ```
///
/// **`fconst_2` and `fconst_1`** — volume 2, pitch 1, which is louder than
/// every other block sound this module builds and is the one number a guess
/// gets wrong. The source entity is `aconst_null`, so the ringing player
/// hears it: `except` is `None` rather than the clicker's id.
#[must_use]
pub fn bell_ring(pos: BlockPos) -> Sound {
    Sound {
        event: "minecraft:block.bell.use".into(),
        source: SOURCE_BLOCKS,
        at: Eighths::block(pos),
        pos: block_centre(pos),
        volume: crate::bell::RING_VOLUME,
        pitch: crate::bell::RING_PITCH,
        except: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::entities::{Entities, Entity};
    use crate::players::{Connection, Players};
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    /// A `protocol_ids` table with the one event these tests send, so `send`
    /// gets past its id lookup. Built here rather than read from
    /// `data/vanilla` for the reason [`send`]'s own doc gives: that file does
    /// not exist under `cargo test`, and a version of this that reached for
    /// the global asserted a sound that could never be sent.
    fn ids_for_test() -> crate::protocol_ids::ProtocolIds {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("rustmc-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tmp");
        // A distinct file per call: `cargo test` runs this module's tests on
        // parallel threads in one process, and two of them writing one path
        // is a flake nobody reproduces.
        let path = dir.join(format!("ids-{}.json", NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::write(
            &path,
            r#"{"menus":{},"blocks":{},"block_entity_types":{},
                "sounds":{"minecraft:block.stone.step":1},"attributes":{}}"#,
        )
        .expect("write");
        crate::protocol_ids::ProtocolIds::load(&path).expect("the fixture parses")
    }

    #[derive(Clone, Default)]
    struct Rec(Arc<Mutex<Vec<u8>>>);
    impl Write for Rec {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("rec").extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Rec {
        fn frames(&self) -> usize {
            let bytes = self.0.lock().expect("rec").clone();
            let mut cur = &bytes[..];
            let mut n = 0;
            while !cur.is_empty() {
                if rustmc_proto::frame::read_frame(&mut cur, false).is_err() {
                    break;
                }
                n += 1;
            }
            n
        }
    }

    fn player_entity(id: i32, x: f64, y: f64, z: f64) -> Entity {
        Entity {
            id,
            uuid: 0,
            kind: crate::entities::PLAYER,
            x,
            y,
            z,
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
                model_customisation: 0x7f,
                score: 0,
            },
        }
    }

    /// A step sound at the origin, and one listener at `at` blocks along x.
    /// Returns how many frames that listener was sent.
    fn frames_for_listener_at(distance: f64, entities_known: bool) -> usize {
        let players = Players::new();
        let entities = Entities::new();
        let rec = Rec::default();
        let entity_id = entities.next_id();
        let conn = Arc::new(Connection::with_entity(
            players.next_id(),
            "listener",
            Box::new(rec.clone()),
            None,
            entity_id,
            32,
            (0, 0),
        ));
        players.join(&conn);
        if entities_known {
            entities.insert(player_entity(entity_id, distance, 0.0, 0.0));
        }
        let sound = Sound {
            event: "minecraft:block.stone.step".into(),
            source: SOURCE_PLAYERS,
            at: Eighths::exact(0.0, 0.0, 0.0),
            pos: (0.0, 0.0, 0.0),
            volume: 0.15,
            pitch: 1.0,
            except: None,
        };
        send(
            &players,
            Some(&ids_for_test()),
            None,
            Some(&entities),
            &[sound],
        );
        rec.frames()
    }

    /// The port itself: a listener inside sixteen blocks hears a step sound
    /// and one outside does not.
    ///
    /// The number this closes is in the module header —
    /// `530-walking-away-drops-a-standing-player-from-view`, corpus run of
    /// 2026-08-09, vanilla 22 step sounds on the observer's stream against
    /// this server's 68, with 51 of ours beyond 16 blocks.
    #[test]
    fn a_step_sound_stops_at_sixteen_blocks() {
        assert_eq!(frames_for_listener_at(0.0, true), 1, "standing on it");
        assert_eq!(frames_for_listener_at(15.9, true), 1, "just inside");
        assert_eq!(frames_for_listener_at(20.0, true), 0, "vanilla's 20 is out");
        assert_eq!(frames_for_listener_at(64.0, true), 0, "far out");
    }

    /// `dcmpg; ifge` — strictly less than, so a player standing at exactly
    /// the radius hears nothing. Volume 0.15 is below 1.0, so the radius here
    /// is the flat 16 rather than `16 * volume`, which is the other half of
    /// the same rule.
    #[test]
    fn the_boundary_is_exclusive() {
        assert_eq!(frames_for_listener_at(16.0, true), 0, "exactly 16 is out");
        // The next representable double below 16 is in. A `<=` would make
        // both of these 1 and the test above would still pass.
        assert_eq!(frames_for_listener_at(16.0_f64.next_down(), true), 1);
    }

    /// The refusal that keeps a missing entity from reading as a missing
    /// feature. A connection with no entity has no position, and vanilla has
    /// no such state — so it is sent the sound rather than silently dropped.
    #[test]
    fn a_listener_with_no_entity_is_still_sent_the_sound() {
        assert_eq!(
            frames_for_listener_at(1000.0, false),
            1,
            "an unlocatable listener hears everything, and that is deliberate"
        );
    }

    /// `entities: None` is the pre-radius behaviour, kept so a unit test that
    /// builds no world is not silently testing a server that has gone quiet.
    #[test]
    fn without_an_entity_registry_send_broadcasts_as_it_always_did() {
        let players = Players::new();
        let rec = Rec::default();
        let conn = Arc::new(Connection::with_entity(
            players.next_id(),
            "listener",
            Box::new(rec.clone()),
            None,
            7,
            32,
            (0, 0),
        ));
        players.join(&conn);
        send(
            &players,
            Some(&ids_for_test()),
            None,
            None,
            &[Sound {
                event: "minecraft:block.stone.step".into(),
                source: SOURCE_PLAYERS,
                at: Eighths::exact(9999.0, 0.0, 0.0),
                pos: (9999.0, 0.0, 0.0),
                volume: 0.15,
                pitch: 1.0,
                except: None,
            }],
        );
        assert_eq!(rec.frames(), 1);
    }

    /// The exclusion still wins over the radius, and it wins *first*: a
    /// presser standing on the button is inside every radius there is.
    #[test]
    fn the_exclusion_outranks_the_radius() {
        let players = Players::new();
        let entities = Entities::new();
        let rec = Rec::default();
        let entity_id = entities.next_id();
        let id = players.next_id();
        let conn = Arc::new(Connection::with_entity(
            id,
            "presser",
            Box::new(rec.clone()),
            None,
            entity_id,
            32,
            (0, 0),
        ));
        players.join(&conn);
        entities.insert(player_entity(entity_id, 0.0, 0.0, 0.0));
        send(
            &players,
            Some(&ids_for_test()),
            None,
            Some(&entities),
            &[Sound {
                event: "minecraft:block.stone.step".into(),
                source: SOURCE_PLAYERS,
                at: Eighths::exact(0.0, 0.0, 0.0),
                pos: (0.0, 0.0, 0.0),
                volume: 0.15,
                pitch: 1.0,
                except: Some(id),
            }],
        );
        assert_eq!(rec.frames(), 0);
    }

    /// A fixed range is what the generated table is for, and this is the one
    /// case where having it changes who hears a sound: at volume 1.0 the
    /// variable rule says 16 and a 64-block fixed range says 64.
    #[test]
    fn a_fixed_range_event_carries_past_sixteen_blocks() {
        let table = crate::sound_events::SoundEventTable::from_json(&serde_json::json!({
            "events": 1,
            "table": [{"id": 1, "name": "minecraft:block.stone.step",
                       "location": "minecraft:block.stone.step",
                       "fixedRange": 64.0, "fixedRangeBits": 0x4280_0000_i64}]
        }))
        .expect("the fixture parses");
        let players = Players::new();
        let entities = Entities::new();
        let rec = Rec::default();
        let entity_id = entities.next_id();
        let conn = Arc::new(Connection::with_entity(
            players.next_id(),
            "listener",
            Box::new(rec.clone()),
            None,
            entity_id,
            32,
            (0, 0),
        ));
        players.join(&conn);
        entities.insert(player_entity(entity_id, 40.0, 0.0, 0.0));
        let sound = Sound {
            event: "minecraft:block.stone.step".into(),
            source: SOURCE_PLAYERS,
            at: Eighths::exact(0.0, 0.0, 0.0),
            pos: (0.0, 0.0, 0.0),
            volume: 1.0,
            pitch: 1.0,
            except: None,
        };
        // The control: without the table, 40 blocks is out.
        send(
            &players,
            Some(&ids_for_test()),
            None,
            Some(&entities),
            std::slice::from_ref(&sound),
        );
        assert_eq!(rec.frames(), 0, "no table: the flat 16 wins and 40 is out");
        send(
            &players,
            Some(&ids_for_test()),
            Some(&table),
            Some(&entities),
            &[sound],
        );
        assert_eq!(rec.frames(), 1, "with the table: 64 blocks, and 40 is in");
    }

    /// The two ways to get the position wrong are whole blocks and the
    /// missing half-block, and this pins both against the recorded value:
    /// the traces show a block at workspace offset `[1, 0, 0]` sounding at
    /// `[1.5, 0.5, 0.5]`, which the harness reads by dividing the wire ints
    /// by eight. A negative coordinate is included because `* 8 + 4` is the
    /// one arithmetic here that a `>> 3` would get wrong on the other side of
    /// zero.
    #[test]
    fn the_position_is_the_block_centre_in_eighths() {
        let body = sound_body(&lever_click((1, 0, 0), true), 934).expect("buf");
        // varint 935 (id + 1) is two bytes, varint 4 is one.
        assert_eq!(&body[..3], &[0xA7, 0x07, 0x04]);
        let ints: Vec<i32> = body[3..15]
            .chunks_exact(4)
            .map(|c| i32::from_be_bytes(c.try_into().expect("4 bytes")))
            .collect();
        assert_eq!(ints, vec![12, 4, 4], "8x + 4 per axis");
        for (x, want) in [(-1, -4), (-2, -12), (0, 4)] {
            let b = sound_body(&lever_click((x, 0, 0), true), 0).expect("buf");
            assert_eq!(
                i32::from_be_bytes(b[2..6].try_into().expect("4 bytes")),
                want,
                "x = {x}"
            );
        }
    }

    /// **The exclusion is the whole point of the button pair.** A press is
    /// not heard by the presser and a release is heard by everybody; a
    /// version that dropped `except` would still produce the right sound at
    /// the right place and would cost `330` and `335` their strict verdicts.
    #[test]
    fn only_the_button_press_excludes_the_presser() {
        let press = button_click("minecraft:stone_button", (1, 0, 0), true, Some(7));
        let release = button_click("minecraft:stone_button", (1, 0, 0), false, Some(7));
        assert_eq!(press.except, Some(7));
        assert_eq!(release.except, None, "the un-press is heard by everybody");
        assert_eq!(press.event, "minecraft:block.stone_button.click_on");
        assert_eq!(release.event, "minecraft:block.stone_button.click_off");
        // The lever's flip is a `pull(…, null)` and is never excluded.
        assert_eq!(lever_click((1, 0, 0), true).except, None);
    }

    /// The `BlockSetType` families, at one block each and at the two that
    /// would be wrong if the rule were "wooden unless the name says stone".
    #[test]
    fn a_buttons_sound_follows_its_block_set_type() {
        let ev = |b: &str| button_click(b, (0, 0, 0), false, None).event;
        assert_eq!(
            ev("minecraft:stone_button"),
            "minecraft:block.stone_button.click_off"
        );
        assert_eq!(
            ev("minecraft:polished_blackstone_button"),
            "minecraft:block.stone_button.click_off",
            "polished blackstone is BlockSetType.POLISHED_BLACKSTONE, which is stone-sounding"
        );
        assert_eq!(
            ev("minecraft:oak_button"),
            "minecraft:block.wooden_button.click_off"
        );
        assert_eq!(
            ev("minecraft:mangrove_button"),
            "minecraft:block.wooden_button.click_off"
        );
        assert_eq!(
            ev("minecraft:cherry_button"),
            "minecraft:block.cherry_wood_button.click_off"
        );
        assert_eq!(
            ev("minecraft:crimson_button"),
            "minecraft:block.nether_wood_button.click_off"
        );
        assert_eq!(
            ev("minecraft:warped_button"),
            "minecraft:block.nether_wood_button.click_off"
        );
        assert_eq!(
            ev("minecraft:bamboo_button"),
            "minecraft:block.bamboo_wood_button.click_off"
        );
    }

    /// The lever's pitch is the one thing about it that is not constant, and
    /// the values are the bytecode's `0.6f` and `0.5f` rather than a guess
    /// that a flip sounds the same both ways.
    /// `ComparatorBlock.useWithoutItem` 26-62, every constant in it.
    ///
    /// The three that separate it from the lever beside it are asserted
    /// against the lever rather than in isolation, because each is a value
    /// the lever also has and a copied-then-edited constructor would keep:
    /// the pitches (0.55/0.5 against 0.6/0.5), the property they are keyed on
    /// (`MODE`, not `POWERED`), and `except` (the clicker, not `null`).
    #[test]
    fn a_comparators_pitch_says_which_mode_it_reached_and_its_clicker_is_deaf() {
        let to_subtract = comparator_click((1, 0, 0), true, Some(7));
        let to_compare = comparator_click((1, 0, 0), false, Some(7));
        assert_eq!(to_subtract.event, "minecraft:block.comparator.click");
        assert!((to_subtract.pitch - 0.55).abs() < 1e-6, "SUBTRACT is 0.55f");
        assert!((to_compare.pitch - 0.5).abs() < 1e-6, "COMPARE is 0.5f");
        assert!((to_subtract.volume - 0.3).abs() < 1e-6);
        assert_eq!(to_subtract.source, SOURCE_BLOCKS);
        // The lever's 0.6 is the value a copy would have kept.
        assert!(
            (to_subtract.pitch - lever_click((1, 0, 0), true).pitch).abs() > 1e-6,
            "0.55f, not the lever's 0.6f — different classes, different constants"
        );
        // `playSound(player, ...)`, not `playSound(null, ...)`.
        assert_eq!(
            to_subtract.except,
            Some(7),
            "the clicker does not hear its own comparator — the entity \
             argument at offset 49 is the player, where LeverBlock.pull \
             passes aconst_null"
        );
        assert_eq!(
            lever_click((1, 0, 0), true).except,
            None,
            "and the lever's is still null, which is what 290 and 553 show on \
             both streams"
        );
    }

    /// The pitch is the **only** thing that says which way the lever went,
    /// and the corpus cannot see the pitch.
    ///
    /// # This test is a warning label, not a requirement
    ///
    /// `the_levers_pitch_says_which_way_it_went` below pins the two constants.
    /// This pins the thing that makes them dangerous: every other field of the
    /// two packets is **identical** — same event, same source, same position,
    /// same volume, same exclusion. So the entire observable difference
    /// between a lever turning on and a lever turning off lives in one `f32`.
    ///
    /// `oracle-runner` deliberately drops `sound.pitch` (`trace.rs`, the
    /// `let _pitch`, with its own test asserting the pitch is not part of the
    /// observation). Therefore **`lever-flips-when-right-clicked` reporting
    /// `match` is not evidence that the lever flipped the right way** — swap
    /// the two constants and that scenario stays green. The same holds for the
    /// comparator's 0.55/0.5, which is what scenario 420 is named for, and
    /// `docs/PARITY-ROADMAP.md`'s "block sounds, random pitch" row is named
    /// for a quantity the corpus cannot observe at all.
    ///
    /// If this assertion ever fails because the two sounds started differing
    /// in some *other* field, that is good news and this comment is stale:
    /// the corpus would then have something to grade.
    #[test]
    fn only_the_pitch_separates_a_lever_going_on_from_one_going_off() {
        let on = lever_click((3, 4, 5), true);
        let off = lever_click((3, 4, 5), false);

        assert_eq!(on.event, off.event);
        assert_eq!(on.source, off.source);
        assert_eq!(on.at, off.at);
        assert!((on.volume - off.volume).abs() < 1e-6);
        assert_eq!(on.except, off.except);
        assert!(
            (on.pitch - off.pitch).abs() > 1e-6,
            "if even the pitch stopped differing, the two would be the same \
             packet and the direction would be unobservable outright"
        );

        // Said as an equality on the whole struct so a field added later is
        // caught here rather than silently joining the invisible half.
        assert_eq!(
            Sound {
                pitch: off.pitch,
                ..on.clone()
            },
            off,
            "a new field that differs by direction would be gradeable, and \
             this test and the roadmap row above it would both need rewriting"
        );
    }

    #[test]
    fn the_levers_pitch_says_which_way_it_went() {
        assert!((lever_click((0, 0, 0), true).pitch - 0.6).abs() < 1e-6);
        assert!((lever_click((0, 0, 0), false).pitch - 0.5).abs() < 1e-6);
        assert!((lever_click((0, 0, 0), true).volume - 0.3).abs() < 1e-6);
    }

    /// A chest's pitch is arbitrary **within a range**, and the range is the
    /// part that is vanilla's. A draw outside it is a defect even though no
    /// check compares the value.
    #[test]
    fn a_chests_pitch_stays_inside_vanillas_range() {
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..200 {
            let p = nine_tenths_pitch();
            assert!((0.9..1.0).contains(&p), "{p} outside [0.9, 1.0)");
            seen.insert(p.to_bits());
        }
        assert!(seen.len() > 100, "a constant would not be a draw at all");
    }
}
