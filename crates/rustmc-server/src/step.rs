//! `Entity.applyMovementEmissionAndPlaySound` — when a walk makes a noise.
//!
//! # The cadence is two floats and nothing else
//!
//! ```text
//!   Entity.applyMovementEmissionAndPlaySound(emission, movement, onPos, onState):
//!     16-25   float h = (float)(movement.horizontalDistance() * 0.6000000238418579d)
//!     52-70   moveDist += climbable ? full : h
//!     84-101  if (moveDist > nextStep && !onState.isAir()) {
//!     112-126     if (vibrationAndSoundEffectsFromBlock(pos, state, emitsSounds, same, movement))
//!     157-162         nextStep = nextStep();          // floor(moveDist) + 1
//!             }
//!   Entity.nextStep():  (int)moveDist + 1
//! ```
//!
//! The `0.6` is a `double` in the constant pool — `0.6000000238418579`, which
//! is `0.6f` widened — so the multiply is done in `f64` from an `f32` constant
//! and the accumulator is an `f32`. Both are reproduced here rather than
//! simplified to `0.6`, because a step's *timing* is what this module exists
//! to get right and the two differ by 2.4e-8 per block, which is a whole
//! sound after enough of them.
//!
//! # What decides whether the step is audible at all
//!
//! `Player.getMovementEmission()` — `NONE` if the player is flying, or if it
//! is on the ground and `isDiscrete()` (sneaking); `ALL` otherwise. And
//! `Entity.vibrationAndSoundEffectsFromBlock` refuses unless
//! `onGround || climbable || (crouching && movement.y == 0) || onRails`, and
//! refuses if swimming.
//!
//! **Three of the clauses are implemented and the rest are named.**
//! `onGround` comes from the move packet's flag byte, the air check reads the
//! block below, and **sneaking is answered** — `Entity.isDiscrete()` is
//! exactly `isShiftKeyDown()`, which `ServerboundPlayerInputPacket` already
//! puts on `crate::player::PlayerState`. See [`emits_sounds`].
//!
//! Still not tracked: `Abilities.flying` (its packet is not decoded),
//! climbing, swimming and rails. The consequence is stated rather than
//! hidden: **a player who flies, climbs, swims or rides a rail will make
//! step sounds here where vanilla makes none or a different one.** That is
//! narrower than it was — until this, every *sneaking* player made them
//! too, and the corpus has a `sneak` verb.
//!
//! # `getOnPos`, and the part of it that is not modelled
//!
//! `Entity.getOnPos(float)` prefers `mainSupportingBlockPos`, which is set by
//! the collision code from the block the entity actually landed on. This
//! server does not run a player through collision — the client's position is
//! taken as given — so there is no supporting block to prefer, and the
//! fallback path is used: `BlockPos(floor(x), floor(y - offset), floor(z))`,
//! offset `0.2f` for the state that is tested and `1.0E-5f` for the position
//! reported. On flat ground with a player standing at an integer `y` the two
//! agree with each other and with the supporting block, which is the whole of
//! the gameplay corpus; on a slab or a stair they can differ from vanilla's
//! and that is a real gap, written down here rather than discovered later.

/// `Player.getMovementEmission().emitsSounds()`.
///
/// ```text
///   Player.getMovementEmission, offsets 0..33
///      0: abilities.flying : ifne 30   -> NONE
///     10: onGround()       : ifeq 24   -> ALL
///     17: isDiscrete()     : ifne 30   -> NONE
///     24: ALL   30: NONE
/// ```
///
/// `Entity.isDiscrete()` is **exactly `isShiftKeyDown()`** — one instruction,
/// no pose involved — which is why this server can answer it: the sneak bit
/// arrives on `ServerboundPlayerInputPacket` and is already tracked on
/// `crate::player::PlayerState`. It is `isCrouching()` that reads the pose,
/// and `isCrouching` appears in `vibrationAndSoundEffectsFromBlock` rather
/// than here.
///
/// `flying` is `Abilities.flying`, set by `ServerboundPlayerAbilitiesPacket`,
/// which this server does not decode — so a creative player who is actually
/// flying makes step sounds here and none in vanilla. That is a smaller gap
/// than the one it replaces (every sneaking player made them) and it is
/// named rather than folded into a default.
#[must_use]
pub const fn emits_sounds(flying: bool, on_ground: bool, shift_key_down: bool) -> bool {
    !(flying || (on_ground && shift_key_down))
}

/// `Entity.moveDist` and `Entity.nextStep`, per player.
///
/// `nextStep` starts at `1.0f` — the field initialiser at `Entity.<init>`
/// offset 39 — which is why the first step of a walk lands after 1.667 blocks
/// rather than immediately.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walk {
    move_dist: f32,
    next_step: f32,
}

impl Default for Walk {
    fn default() -> Self {
        Self::new()
    }
}

/// `Vec3.horizontalDistance() * 0.6000000238418579d`, narrowed to `f32`.
///
/// The constant is the `ldc2_w` at offsets 9 and 20 of
/// `applyMovementEmissionAndPlaySound`, not `0.6`.
#[must_use]
pub fn horizontal_step(dx: f64, dz: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2f at offset 24"
    )]
    let v = (dx.hypot(dz) * 0.600_000_023_841_785_9) as f32;
    v
}

impl Walk {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            move_dist: 0.0,
            next_step: 1.0,
        }
    }

    /// Accumulate one move and say whether a step sound fires.
    ///
    /// `on_ground` is the client's own flag, and `solid_below` is
    /// `!onState.isAir()` — the two clauses of vanilla's gate this server can
    /// answer. Returns `true` exactly when `Entity.walkingStepSound` would be
    /// called.
    ///
    /// **`next_step` is only advanced when the sound fires.** A move that
    /// crosses the threshold over air leaves `moveDist` accumulated and
    /// `nextStep` where it was, so the sound arrives on the first later move
    /// over ground rather than being lost — which is what the bytecode does,
    /// the `nextStep()` assignment sitting inside the `if` at offset 152.
    pub fn advance(&mut self, dx: f64, dz: f64, on_ground: bool, solid_below: bool) -> bool {
        self.advance_emitting(dx, dz, on_ground, solid_below, true)
    }

    /// The same, with `Entity$MovementEmission.emitsSounds()` supplied.
    ///
    /// # Why this is a separate argument and not another clause of the gate
    ///
    /// A sneaking player is `MovementEmission.NONE`, and NONE does **not**
    /// stop the cadence — it only silences it. Three separate places in
    /// `applyMovementEmissionAndPlaySound` say so:
    ///
    /// ```text
    ///    52-70  moveDist += ...            <- before the emission is read
    ///   112-126 vibrationAndSoundEffectsFromBlock(pos, state, EMISSION.emitsSounds(), same, movement)
    ///   157-162 if (it returned true) nextStep = nextStep()
    ///
    ///   vibrationAndSoundEffectsFromBlock, offsets 59-94
    ///     59: if (!emitsSounds) skip walkingStepSound
    ///     69: if (!emitsEvents) skip the STEP game event
    ///     93: return 1                     <- REGARDLESS of either
    /// ```
    ///
    /// So `moveDist` accumulates and `nextStep` advances whether or not a
    /// sound is played. A port that gates the accumulation instead — the
    /// obvious "if silent, do nothing" — freezes the cadence while sneaking
    /// and puts the first step after un-sneaking in the wrong place. The
    /// difference is invisible in a scenario that only sneaks, and shows in
    /// any scenario that sneaks and then walks.
    ///
    /// `Entity$MovementEmission`'s four constants are
    /// `NONE(false, false)`, `SOUNDS(true, false)`, `EVENTS(false, true)`,
    /// `ALL(true, true)` — read from its `<clinit>`, not recalled.
    pub fn advance_emitting(
        &mut self,
        dx: f64,
        dz: f64,
        on_ground: bool,
        solid_below: bool,
        emits_sounds: bool,
    ) -> bool {
        self.move_dist += horizontal_step(dx, dz);
        if self.move_dist <= self.next_step || !solid_below || !on_ground {
            return false;
        }
        // Past the gate: `vibrationAndSoundEffectsFromBlock` returns 1, so
        // `nextStep` moves on even when nothing is heard.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's own f2i at nextStep offset 4"
        )]
        let floored_silent = self.move_dist as i32;
        if !emits_sounds {
            self.next_step = (floored_silent + 1) as f32;
            return false;
        }
        // `Entity.nextStep()`: `(int)moveDist + 1`, a truncating cast, then
        // widened back. Not `ceil` and not `+= 1`: after a single hop of 4
        // blocks `moveDist` is 2.4 and the next threshold is 3, not 3.4.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vanilla's own f2i at nextStep offset 4"
        )]
        let floored = self.move_dist as i32;
        self.next_step = (floored + 1) as f32;
        true
    }

    /// `Entity.moveDist`, for tests and for a log line.
    #[must_use]
    pub const fn move_dist(&self) -> f32 {
        self.move_dist
    }
}

/// `Entity.getOnPos(float)`'s fallback: `floor(x), floor(y - offset), floor(z)`.
///
/// See the module docs for why the `mainSupportingBlockPos` branch is not
/// here. The two offsets vanilla uses on this path are `0.2f`
/// (`getOnPosLegacy`, which supplies the *state* the step sound reads) and
/// `1.0E-5f` (`getOnPos`, which supplies the position compared for the game
/// event); both are passed in rather than baked, because the callers are one
/// either side of a comparison this server may yet have to make.
#[must_use]
pub fn on_pos(x: f64, y: f64, z: f64, offset: f32) -> crate::interact::BlockPos {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a block coordinate inside the world's extent"
    )]
    fn f(v: f64) -> i32 {
        v.floor() as i32
    }
    (f(x), f(y - f64::from(offset)), f(z))
}

/// `getOnPosLegacy()`'s offset — the one that picks the state a step sounds
/// like.
pub const LEGACY_OFFSET: f32 = 0.2;

/// `Entity.blockPosition()` — `BlockPos.containing(position)`, which is
/// `floor` on each axis and **not** `getOnPos`.
///
/// The distinction matters: a step sound reads the block *below* the player
/// and a waypoint carries the block the player is *in*. For 080 the two differ
/// by one in y, and the waypoint digests only reproduce with this one.
#[must_use]
pub fn block_position(x: f64, y: f64, z: f64) -> crate::interact::BlockPos {
    on_pos(x, y, z, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The number this module exists to get right.**
    /// `080-other-player-is-an-entity` walks four blocks out and four back, in
    /// one `hop_to` hop each, and vanilla's recording of 2026-08-08 21:26 has
    /// exactly two `sound` observations. `moveDist` goes 2.4 then 4.8 against
    /// a `nextStep` of 1 then 3.
    #[test]
    fn eighty_walks_four_blocks_twice_and_sounds_twice() {
        let mut w = Walk::new();
        assert!(w.advance(4.0, 0.0, true, true), "first hop");
        assert!((w.move_dist() - 2.4).abs() < 1e-5, "{}", w.move_dist());
        assert!(w.advance(-4.0, 0.0, true, true), "second hop");
        assert!((w.move_dist() - 4.8).abs() < 1e-5, "{}", w.move_dist());
    }

    /// **The step count DEPENDS on how the walk is chopped up, and this test
    /// was written asserting the opposite.**
    ///
    /// It looked as though it could not: `moveDist` is a path length, and
    /// eight blocks is eight blocks however it is delivered. It is
    /// `nextStep()` that breaks the invariance — `floor(moveDist) + 1` sets
    /// the next threshold from where the walk *is*, throwing away however far
    /// past the old threshold one packet carried it. A big hop therefore
    /// skips every threshold it jumps over:
    ///
    /// ```text
    ///   8 x 1 block   0.6  1.2*  1.8  2.4*  3.0  3.6*  4.2*  4.8    4 sounds
    ///   2 x 4 blocks  2.4*                  4.8*                    2 sounds
    /// ```
    ///
    /// This is not a defect in either the model or the harness; it is
    /// vanilla's arithmetic and it is why `client.rs`'s `hop_to` comment says
    /// that more packets for the same walk make every observation of it
    /// phase-sensitive. It does mean `080`'s **two** is a fact about a walk of
    /// two four-block packets and not about a walk of eight blocks: change
    /// `HOP` and 080's sound count changes on both servers together.
    #[test]
    fn the_count_depends_on_how_the_walk_is_chopped() {
        let mut w = Walk::new();
        let n = (0..8).filter(|_| w.advance(1.0, 0.0, true, true)).count();
        assert_eq!(n, 4, "eight blocks in eight hops");
        let mut w = Walk::new();
        let n = (0..2).filter(|_| w.advance(4.0, 0.0, true, true)).count();
        assert_eq!(n, 2, "the same eight blocks in two hops");
    }

    /// A step over air is silent **and does not consume the threshold**. A
    /// version that advanced `nextStep` unconditionally would swallow the
    /// sound of the first ground block after a jump.
    #[test]
    fn air_below_is_silent_and_keeps_the_threshold() {
        let mut w = Walk::new();
        assert!(!w.advance(4.0, 0.0, true, false), "over air");
        assert!(
            w.advance(0.01, 0.0, true, true),
            "the very next ground move"
        );
    }

    /// A player the client says is airborne makes no step sound, which is
    /// vanilla's `onGround()` clause. Half of a jump's arc is otherwise a
    /// stream of footsteps.
    /// A sneaking player is silent, and the cadence carries on underneath.
    ///
    /// The second half is the one a plausible port gets wrong: gating the
    /// accumulation instead of the sound freezes `nextStep`, so the walk
    /// after un-sneaking sounds in the wrong place. Here the silent walk and
    /// the audible one cover the same ground and the audible one is late by
    /// exactly the steps the silent one swallowed.
    #[test]
    fn sneaking_silences_the_step_and_does_not_freeze_the_cadence() {
        // Eight one-block moves, sneaking throughout.
        let mut silent = Walk::new();
        let heard = (0..8)
            .filter(|_| silent.advance_emitting(1.0, 0.0, true, true, false))
            .count();
        assert_eq!(heard, 0, "a sneaking player makes no step sound");

        // The same eight, not sneaking.
        let mut loud = Walk::new();
        let heard = (0..8)
            .filter(|_| loud.advance_emitting(1.0, 0.0, true, true, true))
            .count();
        assert_eq!(heard, 4, "the positive control: four steps in eight blocks");

        // And the two walked the same distance and are at the same threshold,
        // which is what says the cadence was not frozen.
        assert!((silent.move_dist() - loud.move_dist()).abs() < f32::EPSILON);
        assert_eq!(silent.next_step.to_bits(), loud.next_step.to_bits());
    }

    /// `getMovementEmission()`'s three-way gate, each arm separately.
    #[test]
    fn only_a_flyer_or_a_sneaker_on_the_ground_is_silent() {
        assert!(emits_sounds(false, true, false), "walking");
        assert!(!emits_sounds(true, true, false), "flying");
        assert!(!emits_sounds(false, true, true), "sneaking on the ground");
        // Sneaking in mid-air is ALL: the `onGround()` test at offset 10
        // short-circuits to ALL before `isDiscrete()` is reached.
        assert!(emits_sounds(false, false, true), "sneaking in the air");
    }

    #[test]
    fn a_player_in_the_air_does_not_step() {
        let mut w = Walk::new();
        assert!(!w.advance(4.0, 0.0, false, true));
    }

    /// A rotation-only packet moves nothing, so it cannot step. Our movement
    /// handler calls this for every `Update::Moved`, and `MOVE_PLAYER_ROT` is
    /// one of those.
    #[test]
    fn turning_on_the_spot_is_not_a_step() {
        let mut w = Walk::new();
        for _ in 0..100 {
            assert!(!w.advance(0.0, 0.0, true, true));
        }
        assert!((w.move_dist() - 0.0).abs() < f32::EPSILON);
    }

    /// `getOnPos` is the block **below** a player standing on flat ground, and
    /// the y is floored after the offset is taken off — not before.
    #[test]
    fn the_block_a_player_stands_on_is_the_one_below_it() {
        assert_eq!(on_pos(-252.3, -60.0, 0.5, LEGACY_OFFSET), (-253, -61, 0));
        // The corpus's own case: 080's actor at x = -252 exactly.
        assert_eq!(on_pos(-252.0, -60.0, 0.0, LEGACY_OFFSET), (-252, -61, 0));
        // 1.0E-5 is enough to cross the boundary for an integer y, which is
        // why `getOnPos()` and `getOnPosLegacy()` agree on flat ground.
        assert_eq!(on_pos(-252.0, -60.0, 0.0, 1.0e-5), (-252, -61, 0));
    }
}
