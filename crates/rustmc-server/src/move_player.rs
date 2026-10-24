//! `ServerGamePacketListenerImpl.handleMovePlayer` — everything the server
//! decides about a move packet before and after it runs the player through
//! [`crate::collide`].
//!
//! Reference (26.2), `javap -p -c -constants` on the pinned server jar:
//! `ServerGamePacketListenerImpl.handleMovePlayer` (1,332 bytes of code),
//! `.containsInvalidValues`, `.clampHorizontal`, `.clampVertical`,
//! `.updateAwaitingTeleport`, `.shouldCheckPlayerMovement`, `.resetPosition`,
//! `.teleport(PositionMoveRotation, Set)`, `.handleAcceptTeleportPacket`,
//! `.isEntityCollidingWithAnythingNew`, `.handlePlayerKnownMovement`,
//! `Mth.wrapDegrees(float)`, `Mth.clamp(double, double, double)`.
//!
//! # Why this is a state machine and not a handler
//!
//! `handleMovePlayer` is one method, but it reaches the world twice — once
//! through `player.move(MoverType.PLAYER, …)` and once through
//! `isEntityCollidingWithAnythingNew` — and everything between those two
//! reaches is arithmetic on fields this module owns. Splitting it at the
//! move means the arithmetic can be tested against vanilla's constants
//! without a world, a chunk source or a socket, and the two halves still run
//! in vanilla's order because the caller cannot run them any other way:
//! [`Tracker::before_move`] hands out a [`BeforeMove`] the caller must feed
//! back to [`Tracker::after_move`].
//!
//! # The two "good" positions are different quantities
//!
//! This is the single thing a plausible port gets wrong, because both names
//! sound like "where the server last agreed the player was":
//!
//! * **`firstGood{X,Y,Z}`** is set only by `resetPosition()`, which
//!   `handleMovePlayer` calls when `tickCount == 0` and which the tick loop
//!   calls once per tick out of `ServerGamePacketListenerImpl.tick`. It is
//!   the anchor of the *speed* check (offsets 275-300), and the whole point
//!   of it being a per-tick anchor is that the check is a budget for a
//!   tick's worth of packets rather than for one packet.
//! * **`lastGood{X,Y,Z}`** is rewritten at the *end* of every accepted move
//!   (offsets 1298-1331) and by `handleAcceptTeleportPacket`. It is the
//!   anchor of the *delta* the player is actually moved by (offsets
//!   590-615).
//!
//! Using one for both makes a walking player fail the speed check on the
//! second packet of a tick, or pass it forever, depending on which one you
//! picked.
//!
//! # The move limit is a budget, in blocks squared per packet
//!
//! ```text
//!   460-474  float limit = isFallFlying ? 300.0f : 100.0f
//!   476-489  if (distanceSq - deltaMovementLengthSq > (double)(limit * (float)packets))
//! ```
//!
//! `packets` is `receivedMovePacketCount - knownMovePacketCount`, clamped to
//! **1** — not to 5 — when it exceeds 5 (offsets 419-449): the `> 5` branch
//! logs "sending move packets too frequently" and then *assigns* `1`, so a
//! client that floods gets the budget of a single packet rather than of the
//! flood. A port that reads that branch as a clamp to 5, or as log-only,
//! gives a flooding client five times the allowance vanilla gives it.
//!
//! The multiply `limit * (float)packets` is done in **`f32`** and only then
//! widened; `distanceSq - deltaMovementLengthSq` is `f64`. Both are
//! reproduced.
//!
//! # What is refused rather than approximated
//!
//! [`Gates`] carries the six predicates this module cannot answer for
//! itself — `wonGame`, `hasClientLoaded`, `isPassenger`, `isSleeping`,
//! `isFallFlying`, `isChangingDimension` — plus `isSingleplayerOwner` and
//! the two game rules. The caller supplies them. None is guessed here: a
//! `false` invented for `isSleeping` would silently delete the
//! sleep branch at offsets 333-378, and this project has paid for exactly
//! that shape of default four times.

/// `Mth.wrapDegrees(float)` — `f % 360`, then `-360` if `>= 180` and `+360`
/// if `< -180`.
///
/// Java's `frem` is IEEE remainder-truncated (the sign follows the
/// dividend), which is Rust's `%` on floats, so this is a transcription and
/// not a re-derivation.
#[must_use]
pub fn wrap_degrees(f: f32) -> f32 {
    let mut g = f % 360.0;
    if g >= 180.0 {
        g -= 360.0;
    }
    if g < -180.0 {
        g += 360.0;
    }
    g
}

/// `Mth.clamp(double, double, double)`.
///
/// **Not `d.clamp(min, max)`.** Vanilla is `d < min ? min : Math.min(d, max)`
/// with a `dcmpg` on the first compare, so a NaN takes the `min` branch;
/// Rust's `f64::clamp` panics on a NaN bound and returns NaN for a NaN
/// input. Nothing reaches here with a NaN — [`contains_invalid_values`] runs
/// first — but the two differ and the difference is written rather than
/// relied on.
#[must_use]
fn mth_clamp(d: f64, min: f64, max: f64) -> f64 {
    if d < min {
        min
    } else {
        d.min(max)
    }
}

/// `clampHorizontal` — `Mth.clamp(d, -3.0E7, 3.0E7)`.
#[must_use]
pub fn clamp_horizontal(d: f64) -> f64 {
    mth_clamp(d, -3.0E7, 3.0E7)
}

/// `clampVertical` — `Mth.clamp(d, -2.0E7, 2.0E7)`.
///
/// **A different limit from the horizontal one**, and the asymmetry is
/// vanilla's: 2e7 against 3e7.
#[must_use]
pub fn clamp_vertical(d: f64) -> f64 {
    mth_clamp(d, -2.0E7, 2.0E7)
}

/// `containsInvalidValues(double, double, double, float, float)`.
///
/// **The three doubles are tested with `isNaN` and the two floats with
/// `Floats.isFinite`.** So an infinite *position* passes this check and an
/// infinite *rotation* does not — which reads like an oversight and is what
/// the bytecode does (offsets 0-35). A port that tests all five the same way
/// disconnects a client vanilla keeps, or keeps one vanilla drops.
///
/// **And the two floats are read in the order pitch, then yaw** — `fload 7`
/// before `fload 6` at offsets 22-32. Argument order is `(x, y, z, yaw,
/// pitch)`; the check evaluates `isFinite(pitch) && isFinite(yaw)`. With
/// `&&` short-circuiting over a side-effect-free predicate the result is the
/// same either way, which is why this is a comment and not a swap.
#[must_use]
pub fn contains_invalid_values(x: f64, y: f64, z: f64, yaw: f32, pitch: f32) -> bool {
    x.is_nan() || y.is_nan() || z.is_nan() || !pitch.is_finite() || !yaw.is_finite()
}

/// The predicates `handleMovePlayer` asks of things this module does not
/// own, supplied by the caller once per packet.
///
/// Every field is a question vanilla asks; none has a default here. See the
/// module header for why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gates {
    /// `ServerPlayer.wonGame` — offsets 62-72, an early `return`.
    pub won_game: bool,
    /// `hasClientLoaded()` — offsets 84-91, an early `return`.
    pub client_loaded: bool,
    /// `ServerPlayer.isPassenger()` — offsets 189-247. A passenger's packet
    /// snaps rotation onto the current position and returns.
    pub passenger: bool,
    /// `ServerPlayer.isSleeping()` — offsets 333-378 and again at 794-801
    /// and 865-872.
    pub sleeping: bool,
    /// `ServerPlayer.isFallFlying()` — read once at offset 379 into a local
    /// and used three times, so a state change mid-handler cannot split the
    /// branches. Reproduced by taking it as one field.
    pub fall_flying: bool,
    /// `ServerPlayer.isChangingDimension()` — offsets 775-782, and again
    /// inside `shouldCheckPlayerMovement`.
    pub changing_dimension: bool,
    /// `isSingleplayerOwner()` — the first clause of
    /// `shouldCheckPlayerMovement`. A dedicated server's is always false.
    pub singleplayer_owner: bool,
    /// `GameRules.PLAYER_MOVEMENT_CHECK`.
    pub player_movement_check: bool,
    /// `GameRules.ELYTRA_MOVEMENT_CHECK`, consulted only when
    /// [`Gates::fall_flying`].
    pub elytra_movement_check: bool,
    /// `TickRateManager.runsNormally()` — offsets 388-395. False freezes the
    /// whole speed check, packet counting included.
    pub runs_normally: bool,
    /// `ServerPlayer.isCreative()` and `.isSpectator()` — two of the five
    /// exemptions from the "moved wrongly" check at offsets 804-821.
    pub creative: bool,
    /// `ServerPlayer.isSpectator()`.
    pub spectator: bool,
    /// `ServerPlayer.isInPostImpulseGraceTime()` — offsets 824-831.
    pub post_impulse_grace: bool,
    /// `ServerPlayer.noPhysics` — offsets 855-862.
    pub no_physics: bool,
}

impl Default for Gates {
    /// A living, loaded, walking survival player on a dedicated server with
    /// both game rules on — the state the gameplay corpus drives.
    ///
    /// **A default is provided for tests and for the corpus, not as an
    /// answer.** A caller that has a real player must fill these from it;
    /// see [`Gates`]'s own doc for why nothing here is guessed.
    fn default() -> Self {
        Self {
            won_game: false,
            client_loaded: true,
            passenger: false,
            sleeping: false,
            fall_flying: false,
            changing_dimension: false,
            singleplayer_owner: false,
            player_movement_check: true,
            elytra_movement_check: true,
            runs_normally: true,
            creative: false,
            spectator: false,
            post_impulse_grace: false,
            no_physics: false,
        }
    }
}

/// One `ServerboundMovePlayerPacket`, after the four subclasses' defaults
/// have been applied.
///
/// `getX(double)`, `getY(double)`, `getZ(double)`, `getYRot(float)` and
/// `getXRot(float)` each return the packet's value if it carries one and the
/// argument if it does not — which is why `handleMovePlayer` passes the
/// player's own current value in at offsets 92-121 and `0.0`/`0.0f` in at
/// offsets 12-37. The two call sites pass *different* defaults into the same
/// getters and a port that resolves the defaults once gets the
/// `containsInvalidValues` check wrong: a rotation-only packet's `x` is
/// `0.0` there and the player's `x` here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovePacket {
    /// Present on `move_player_pos` and `move_player_pos_rot`.
    pub pos: Option<(f64, f64, f64)>,
    /// Present on `move_player_rot` and `move_player_pos_rot`.
    pub rot: Option<(f32, f32)>,
    /// `isOnGround()` — bit 0 of the trailing flags byte. Present on all
    /// four.
    pub on_ground: bool,
    /// `horizontalCollision()` — bit 1.
    pub horizontal_collision: bool,
}

impl MovePacket {
    /// The `containsInvalidValues` arguments: the getters with `0.0` and
    /// `0.0f` defaults, exactly as offsets 12-37 pass them.
    #[must_use]
    fn validation_values(self) -> (f64, f64, f64, f32, f32) {
        let (x, y, z) = self.pos.unwrap_or((0.0, 0.0, 0.0));
        let (yaw, pitch) = self.rot.unwrap_or((0.0, 0.0));
        (x, y, z, yaw, pitch)
    }
}

/// What the pre-move half of `handleMovePlayer` decided.
#[derive(Debug, Clone, PartialEq)]
pub enum Before {
    /// `disconnect(Component.translatable("multiplayer.disconnect.invalid_player_movement"))`
    /// — offsets 43-53.
    Disconnect,
    /// One of the four early `return`s that do nothing: `wonGame`,
    /// `!hasClientLoaded`, and the two arms of the passenger branch that
    /// this server cannot reach (nothing here is rideable).
    Ignore,
    /// `absSnapRotationTo(yRot, xRot)` and return — the
    /// `updateAwaitingTeleport()` branch at offsets 123-140, and the
    /// passenger branch's `absSnapTo` at 199-247 with the position
    /// unchanged.
    SnapRotation { yaw: f32, pitch: f32 },
    /// `teleport(player.getX(), getY(), getZ(), yRot, xRot)` and return —
    /// the sleep branch at 350-378 and the "moved too quickly" branch at
    /// 541-580. The position is the player's *current* one, so this is a
    /// rubber-band to where the server already had them.
    Correct {
        yaw: f32,
        pitch: f32,
        reason: CorrectReason,
    },
    /// Proceed: run `player.move(MoverType.PLAYER, delta)` with
    /// [`Accepted::delta`], then call [`Tracker::after_move`].
    Accept(Accepted),
}

/// Which of `handleMovePlayer`'s three rubber-bands fired.
///
/// Separate variants because they are separate findings: a corpus that only
/// ever sees `TooQuick` has not exercised the collision path, and a green
/// number that cannot say which one it saw is the kind this project refuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectReason {
    /// Offsets 343-378: sleeping and `distanceSq > 1.0`.
    Sleeping,
    /// Offsets 476-580: the speed budget.
    TooQuick,
    /// Offsets 875-925: `movedWrongly` with no collision at the old box, or
    /// `isEntityCollidingWithAnythingNew`.
    CollidingNew,
}

/// The pre-move half's output when the move is to be run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accepted {
    /// The clamped target the client asked for — `clampHorizontal(x)`,
    /// `clampVertical(y)`, `clampHorizontal(z)`.
    pub target: (f64, f64, f64),
    /// `Mth.wrapDegrees` of the packet's rotation, defaulted to the
    /// player's.
    pub yaw: f32,
    /// Wrapped pitch.
    pub pitch: f32,
    /// The vector to hand `Entity.move`: `target - lastGood`, offsets
    /// 590-615.
    pub delta: (f64, f64, f64),
    /// `d1 > 0.0` where `d1` is the *lastGood*-relative dy, taken at offsets
    /// 617-629 **before** the move. It survives the move in a local and is
    /// what resets fall distance at offsets 1195-1206, so it is a fact about
    /// what the client asked for and not about what the server allowed.
    pub asked_to_rise: bool,
    /// True when `player.onGround() && !packet.isOnGround() && asked_to_rise`
    /// — offsets 631-660, the one place a server-side jump is started.
    pub jump_from_ground: bool,
    /// Where the player was before the move, for the after-move deltas and
    /// for the rubber-band target. Offsets 248-273.
    pub was_at: (f64, f64, f64),
    /// `d15` — the lastGood-relative dy, saved at offsets 692-695 *before*
    /// the after-move recomputation overwrites `d11`. Only `clientIsFloating`
    /// reads it.
    pub pre_move_dy: f64,
    /// `bl3`, read from the player at offsets 660-668 — that is, from the
    /// PREVIOUS tick's move, before this packet's move overwrites it.
    pub vertical_collision_below: bool,
}

/// What the post-move half decided.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum After {
    /// `teleport(d4, d5, d6, f, f1)` back to `was_at` and return — offsets
    /// 913-974. `doCheckFallDamage` still runs, with the delta the *server*
    /// achieved, before the return.
    Correct {
        to: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        fall_delta: (f64, f64, f64),
    },
    /// `absSnapTo(d4, d5, d6, f, f1)` — offsets 975-988 — and everything
    /// after it.
    Accept {
        /// The clamped target, which is where the player ends up. Note that
        /// vanilla snaps to the **client's** position here and not to where
        /// `move` put them: the collision result was only ever used to decide
        /// whether to rubber-band.
        to: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        /// `new Vec3(getX() - d7, getY() - d8, getZ() - d9)` at offsets
        /// 1107-1144, taken **after** the snap, so it is the accepted
        /// movement and not the swept one.
        movement: (f64, f64, f64),
        /// Offsets 1195-1206: `if (bl2) player.resetFallDistance()`.
        reset_fall_distance: bool,
        /// `packet.isOnGround()`, carried through because offsets 1146-1188
        /// hand it to BOTH `setOnGroundWithMovement` and
        /// `doCheckFallDamage` and a caller that re-derives it from the
        /// server's own flag would use the wrong one.
        on_ground: bool,
        /// `packet.horizontalCollision()`, the second argument of
        /// `setOnGroundWithMovement`.
        horizontal_collision: bool,
        /// Offsets 1207-1260: `onGround || landedInLiquid || onClimbable ||
        /// spectator || fallFlying || autoSpinAttack`, of which this module
        /// can answer three. The two it cannot are left to the caller, which
        /// is why this is the partial disjunction and not the answer — see
        /// [`After::Accept::try_reset_impulse_unless`].
        try_reset_impulse_unless: TryResetImpulse,
    },
}

/// The three terms of offsets 1207-1260 that `handleMovePlayer` itself
/// supplies, for a caller that has the other three.
///
/// `hasLandedInLiquid()` and `onClimbable()` are `LivingEntity` state this
/// module does not hold, and `isAutoSpinAttack()` is a trident enchantment
/// this server has no field for. Returning a partial disjunction with the
/// missing terms named is the alternative to inventing `false` for them,
/// which would make `tryResetCurrentImpulseContext` fire in cases vanilla
/// skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryResetImpulse {
    /// `packet.isOnGround() || isSpectator() || isFallFlying()`.
    pub known: bool,
}

/// `ServerGamePacketListenerImpl`'s movement fields, and the methods that
/// read them.
#[derive(Debug, Clone, PartialEq)]
pub struct Tracker {
    first_good: (f64, f64, f64),
    last_good: (f64, f64, f64),
    received_move_packet_count: i32,
    known_move_packet_count: i32,
    awaiting_position_from_client: Option<(f64, f64, f64)>,
    awaiting_teleport: i32,
    awaiting_teleport_time: i32,
    tick_count: i32,
    client_is_floating: bool,
}

impl Tracker {
    /// A tracker for a player who has just been placed at `pos`.
    ///
    /// `tickCount` starts at **0**, which is load-bearing: offsets 73-83
    /// call `resetPosition()` on the first packet of the connection because
    /// of it.
    #[must_use]
    pub const fn new(pos: (f64, f64, f64)) -> Self {
        Self {
            first_good: pos,
            last_good: pos,
            received_move_packet_count: 0,
            known_move_packet_count: 0,
            awaiting_position_from_client: None,
            awaiting_teleport: 0,
            awaiting_teleport_time: 0,
            tick_count: 0,
            client_is_floating: false,
        }
    }

    /// Adopt a teleport that was sent before this tracker existed.
    ///
    /// **The join's teleport is one of `handleMovePlayer`'s, and the counter
    /// has to know it.** `PlayerList.placeNewPlayer` reaches the same
    /// `teleport(PositionMoveRotation, Set)` this module models, so vanilla's
    /// `awaitingTeleport` is already **1** and `awaitingPositionFromClient`
    /// is already the spawn when the first move packet arrives. In this
    /// server `world_send::enter_world` sends that packet before the play
    /// loop constructs a tracker, so the tracker is told about it here.
    ///
    /// Getting this wrong is not cosmetic: the id is a **compared field** of
    /// `player_position` in the gameplay corpus (`trace.rs`'s
    /// `PLAYER_POSITION` arm), so a tracker that starts at 0 puts `1` on the
    /// wire where vanilla puts `2` and every correction check diverges on the
    /// counter rather than on the correction.
    pub const fn adopt_teleport(&mut self, id: i32, to: (f64, f64, f64)) {
        self.awaiting_teleport = id;
        self.awaiting_position_from_client = Some(to);
        self.awaiting_teleport_time = self.tick_count;
    }

    /// `resetPosition()` — both anchors to the player's current position.
    pub const fn reset_position(&mut self, pos: (f64, f64, f64)) {
        self.first_good = pos;
        self.last_good = pos;
    }

    /// The speed check's anchor.
    #[must_use]
    pub const fn first_good(&self) -> (f64, f64, f64) {
        self.first_good
    }

    /// The delta's anchor.
    #[must_use]
    pub const fn last_good(&self) -> (f64, f64, f64) {
        self.last_good
    }

    /// `clientIsFloating` — **always `false`, and deliberately so.**
    ///
    /// See the refusal written into [`Tracker::after_move`]: five of the nine
    /// terms of vanilla's conjunction are subsystems this server does not
    /// have, and the one consumer (`tick()`'s `aboveGroundTickCount` kick) is
    /// not ported. The accessor exists so that the field's meaning is
    /// documented in one place rather than rediscovered.
    #[must_use]
    pub const fn client_is_floating(&self) -> bool {
        self.client_is_floating
    }

    /// The id the next correction teleport will carry, for a caller that
    /// needs to match an acknowledgement.
    #[must_use]
    pub const fn awaiting_teleport(&self) -> i32 {
        self.awaiting_teleport
    }

    /// Whether a teleport is outstanding — `awaitingPositionFromClient !=
    /// null`.
    #[must_use]
    pub const fn awaiting_position(&self) -> Option<(f64, f64, f64)> {
        self.awaiting_position_from_client
    }

    /// `ServerGamePacketListenerImpl.tickPlayer()`'s movement bookkeeping.
    ///
    /// ```text
    ///   tickPlayer, offsets 0..104
    ///      0: resetPosition()
    ///      4: xo = getX(); yo = getY(); zo = getZ()
    ///     50: player.doTick()
    ///     53: absSnapTo(firstGoodX, firstGoodY, firstGoodZ, yRot, xRot)
    ///     86: tickCount++
    ///     96: knownMovePacketCount = receivedMovePacketCount
    /// ```
    ///
    /// **`resetPosition()` is the FIRST thing it does, and it is what makes
    /// the speed budget a per-tick budget.** An earlier draft of this module
    /// asserted the opposite — that `firstGood` moves only at a teleport —
    /// which is what `handleMovePlayer` alone suggests, and it is wrong: with
    /// `firstGood` pinned at the join, a player walking 11 blocks from spawn
    /// would be rubber-banded for ever, and `530-walking-away-drops-a-
    /// standing-player-from-view` walks 85. The bytecode above settles it.
    ///
    /// `pos` is the player's position at the top of the tick, i.e. wherever
    /// the last accepted packet left them — `resetPosition` runs before
    /// `doTick`.
    pub const fn tick(&mut self, pos: (f64, f64, f64)) {
        self.reset_position(pos);
        self.tick_count = self.tick_count.wrapping_add(1);
        self.known_move_packet_count = self.received_move_packet_count;
    }

    /// `teleport(PositionMoveRotation, Set)` — the bookkeeping half.
    ///
    /// Returns the teleport id to put on the wire. The counter is
    /// pre-incremented and wraps to **0** at `Integer.MAX_VALUE` (offsets
    /// 8-29), which is not the same as `i32::MAX.wrapping_add(1)`.
    pub const fn begin_teleport(&mut self, to: (f64, f64, f64)) -> i32 {
        self.awaiting_teleport_time = self.tick_count;
        self.awaiting_teleport = self.awaiting_teleport.wrapping_add(1);
        if self.awaiting_teleport == i32::MAX {
            self.awaiting_teleport = 0;
        }
        self.awaiting_position_from_client = Some(to);
        self.awaiting_teleport
    }

    /// `handleAcceptTeleportPacket`.
    ///
    /// Returns `Err(())` when vanilla disconnects — an acknowledgement whose
    /// id matches while no teleport is outstanding (offsets 23-40). An id
    /// that does *not* match is silently ignored, which is the common case
    /// for a stale acknowledgement and must not be an error.
    ///
    /// # Errors
    /// When the id matches and `awaitingPositionFromClient` is null.
    #[expect(
        clippy::result_unit_err,
        reason = "the caller's only response is `disconnect`, which carries vanilla's own \
                  translation key and nothing this function could add"
    )]
    pub const fn accept_teleport(&mut self, id: i32) -> Result<Option<(f64, f64, f64)>, ()> {
        if id != self.awaiting_teleport {
            return Ok(None);
        }
        let Some(pos) = self.awaiting_position_from_client else {
            return Err(());
        };
        self.last_good = pos;
        self.awaiting_position_from_client = None;
        Ok(Some(pos))
    }

    /// `updateAwaitingTeleport()`.
    ///
    /// Re-sends the teleport when the client has been silent for **more
    /// than 20 ticks** — `tickCount - awaitingTeleportTime > 20`, a strict
    /// `>` on `if_icmple` at offset 18. Returns whether a move packet should
    /// be swallowed.
    fn update_awaiting_teleport(&mut self, yaw: f32, pitch: f32) -> AwaitingTeleport {
        let Some(pos) = self.awaiting_position_from_client else {
            self.awaiting_teleport_time = self.tick_count;
            return AwaitingTeleport::NotAwaiting;
        };
        if self.tick_count - self.awaiting_teleport_time > 20 {
            self.awaiting_teleport_time = self.tick_count;
            let id = self.begin_teleport(pos);
            return AwaitingTeleport::Resend {
                to: pos,
                yaw,
                pitch,
                id,
            };
        }
        AwaitingTeleport::Awaiting
    }

    /// `shouldCheckPlayerMovement(boolean isFallFlying)`.
    #[must_use]
    fn should_check_player_movement(g: Gates) -> bool {
        if g.singleplayer_owner || g.changing_dimension || !g.player_movement_check {
            return false;
        }
        // 50-71, as an early return rather than a negated conjunction:
        // the elytra rule is consulted only while fall-flying.
        if g.fall_flying && !g.elytra_movement_check {
            return false;
        }
        true
    }
}

/// What the caller knows about the player at the moment the packet arrived.
///
/// All six are read by `handleMovePlayer` from the `ServerPlayer` and none
/// is derivable from the packet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Now {
    /// `getX()`, `getY()`, `getZ()`.
    pub pos: (f64, f64, f64),
    /// `getYRot()`, used as the default for a packet with no rotation.
    pub yaw: f32,
    /// `getXRot()`.
    pub pitch: f32,
    /// `onGround()` — the *server's* flag, not the packet's. The two are
    /// compared at offsets 631-645 and disagreeing is what starts a jump.
    pub on_ground: bool,
    /// `getDeltaMovement().lengthSqr()` — the allowance the speed check
    /// subtracts, so a player the server itself launched is not punished for
    /// arriving where it sent them.
    pub delta_movement_len_sq: f64,
    /// `verticalCollisionBelow`, as the previous tick's `move` left it.
    /// Feeds `clientIsFloating` only.
    pub vertical_collision_below: bool,
}

impl Tracker {
    /// `handleMovePlayer` up to and including the decision to run
    /// `player.move(MoverType.PLAYER, …)`.
    ///
    /// The caller runs the move itself — this module has no world — and then
    /// must call [`Tracker::after_move`] with the [`Accepted`] returned here.
    pub fn before_move(&mut self, pkt: MovePacket, g: Gates, now: Now) -> Before {
        // 0-53. The getters default to zero HERE and to the player's own
        // values sixty instructions later; see `MovePacket::validation_values`.
        let (vx, vy, vz, vyaw, vpitch) = pkt.validation_values();
        if contains_invalid_values(vx, vy, vz, vyaw, vpitch) {
            return Before::Disconnect;
        }
        // 62-72.
        if g.won_game {
            return Before::Ignore;
        }
        // 73-83. The first packet of a connection anchors both goods.
        if self.tick_count == 0 {
            self.reset_position(now.pos);
        }
        // 84-91.
        if !g.client_loaded {
            return Before::Ignore;
        }
        // 92-121. Wrapped, and defaulted to the player's own rotation.
        let (pyaw, ppitch) = pkt.rot.unwrap_or((now.yaw, now.pitch));
        let yaw = wrap_degrees(pyaw);
        let pitch = wrap_degrees(ppitch);
        // 123-140.
        match self.update_awaiting_teleport(yaw, pitch) {
            AwaitingTeleport::NotAwaiting => {}
            AwaitingTeleport::Awaiting | AwaitingTeleport::Resend { .. } => {
                return Before::SnapRotation { yaw, pitch };
            }
        }
        // 141-187.
        let (px, py, pz) = pkt.pos.unwrap_or(now.pos);
        let target = (
            clamp_horizontal(px),
            clamp_vertical(py),
            clamp_horizontal(pz),
        );
        // 189-247. A passenger's own position is never taken from the packet.
        if g.passenger {
            return Before::SnapRotation { yaw, pitch };
        }
        // 248-273.
        let was_at = now.pos;
        // 275-331. The SPEED check is anchored on firstGood.
        let f = (
            target.0 - self.first_good.0,
            target.1 - self.first_good.1,
            target.2 - self.first_good.2,
        );
        let distance_sq = f.0.mul_add(f.0, f.1.mul_add(f.1, f.2 * f.2));
        // 333-378.
        if g.sleeping {
            if distance_sq > 1.0 {
                return Before::Correct {
                    yaw,
                    pitch,
                    reason: CorrectReason::Sleeping,
                };
            }
            return Before::Ignore;
        }
        // 388-580. Packet counting and the speed budget, both frozen when
        // the tick rate manager is not running normally.
        if g.runs_normally {
            self.received_move_packet_count = self.received_move_packet_count.wrapping_add(1);
            let mut packets = self.received_move_packet_count - self.known_move_packet_count;
            // 419-449: a flood is clamped to ONE packet's budget, not five.
            if packets > 5 {
                packets = 1;
            }
            if Self::should_check_player_movement(g) {
                let limit: f32 = if g.fall_flying { 300.0 } else { 100.0 };
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "vanilla's own `i2f` at offset 485"
                )]
                let budget = f64::from(limit * packets as f32);
                if distance_sq - now.delta_movement_len_sq > budget {
                    return Before::Correct {
                        yaw,
                        pitch,
                        reason: CorrectReason::TooQuick,
                    };
                }
            }
        }
        // 590-629. The DELTA is anchored on lastGood, and `bl2` is taken
        // from it before the move.
        let delta = (
            target.0 - self.last_good.0,
            target.1 - self.last_good.1,
            target.2 - self.last_good.2,
        );
        let asked_to_rise = delta.1 > 0.0;
        // 631-659.
        let jump_from_ground = now.on_ground && !pkt.on_ground && asked_to_rise;
        Before::Accept(Accepted {
            target,
            yaw,
            pitch,
            delta,
            asked_to_rise,
            jump_from_ground,
            was_at,
            pre_move_dy: delta.1,
            vertical_collision_below: now.vertical_collision_below,
        })
    }

    /// `handleMovePlayer` from offset 696 to the end.
    ///
    /// `now_at` is where `player.move` actually left the player.
    /// `old_box_free` is `level.noCollision(player, aABB)` on the box the
    /// player had **before** the move, and `colliding_new` is
    /// `isEntityCollidingWithAnythingNew(level, player, aABB, d4, d5, d6)`.
    /// Both are closures so that vanilla's short-circuit is reproduced rather
    /// than approximated: `noCollision` is asked only when the move was
    /// wrong, and neither is asked at all when the player has `noPhysics` or
    /// is asleep.
    pub fn after_move<F, G>(
        &mut self,
        a: Accepted,
        pkt: MovePacket,
        g: Gates,
        now_at: (f64, f64, f64),
        old_box_free: F,
        colliding_new: G,
    ) -> After
    where
        F: FnOnce() -> bool,
        G: FnOnce() -> bool,
    {
        // 696-751. The residual between where the client asked to be and
        // where collision put them.
        let rx = a.target.0 - now_at.0;
        // 720-740 — AND THIS IS A TAUTOLOGY IN VANILLA, NOT A BAND.
        //
        //     720: d11 > -0.5  -> goto 738 (zero it)
        //     729: d11 >= 0.5  -> goto 741 (skip)
        //     738: d11 = 0.0
        //
        // which is `if (ry > -0.5 || ry < 0.5) ry = 0.0`, true for every
        // finite double. So the vertical residual NEVER contributes to the
        // "moved wrongly" distance. A port that reads the obvious `&&` there
        // rubber-bands a player whose vertical residual exceeds half a block
        // — a legitimate step up a slab under lag — and vanilla does not.
        let ry = 0.0_f64;
        let rz = a.target.2 - now_at.2;
        let residual_sq = rx.mul_add(rx, ry.mul_add(ry, rz * rz));
        // 772-853.
        let moved_wrongly = !g.changing_dimension
            && residual_sq > 0.0625
            && !g.sleeping
            && !g.creative
            && !g.spectator
            && !g.post_impulse_grace;
        // 855-974.
        if !g.no_physics && !g.sleeping && ((moved_wrongly && old_box_free()) || colliding_new()) {
            let fall_delta = (
                now_at.0 - a.was_at.0,
                now_at.1 - a.was_at.1,
                now_at.2 - a.was_at.2,
            );
            return After::Correct {
                to: a.was_at,
                yaw: a.yaw,
                pitch: a.pitch,
                fall_delta,
            };
        }
        // 975-988: the player is snapped to what the CLIENT asked for, not
        // to what collision produced. The swept position only ever decided
        // whether to rubber-band.
        let movement = (
            a.target.0 - a.was_at.0,
            a.target.1 - a.was_at.1,
            a.target.2 - a.was_at.2,
        );
        // 1000-1087 — `clientIsFloating` — is REFUSED, not approximated.
        //
        // Vanilla's conjunction is nine terms: `d15 >= -0.03125`, `!bl3`,
        // `!isSpectator()`, `!server.allowFlight()`, `!abilities.mayfly`,
        // `!hasEffect(LEVITATION)`, `!bl`, `!bl5` and `noBlocksAround()`.
        // Five of those — the server's allow-flight setting, the player's
        // `mayfly` ability, mob effects, auto-spin-attack, and a
        // `getBlockStates` sweep over the inflated box — are fields and
        // subsystems this server does not have. Dropping them turns a
        // conjunction that is almost always FALSE into one that is almost
        // always TRUE, and the only thing it feeds is the
        // `aboveGroundTickCount` kick in `tick()`, which is not ported
        // either. A partial answer here would be a field written by this
        // module and read by nothing, behind a doc comment describing a gate
        // no line implements — the exact shape `docs/AGENT-BRIEF.md` names as
        // this project's central defect. `Tracker::client_is_floating` stays
        // at its initial `false` and says so.
        //
        // 1298-1331.
        self.last_good = a.target;
        After::Accept {
            to: a.target,
            yaw: a.yaw,
            pitch: a.pitch,
            movement,
            reset_fall_distance: a.asked_to_rise,
            on_ground: pkt.on_ground,
            horizontal_collision: pkt.horizontal_collision,
            try_reset_impulse_unless: TryResetImpulse {
                known: pkt.on_ground || g.spectator || g.fall_flying,
            },
        }
    }
}

/// The three outcomes of `updateAwaitingTeleport`, kept apart because the
/// resend has a packet to send and the other two do not.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AwaitingTeleport {
    NotAwaiting,
    Awaiting,
    Resend {
        to: (f64, f64, f64),
        yaw: f32,
        pitch: f32,
        id: i32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_at(pos: (f64, f64, f64)) -> Now {
        Now {
            pos,
            yaw: 0.0,
            pitch: 0.0,
            on_ground: true,
            delta_movement_len_sq: 0.0,
            vertical_collision_below: true,
        }
    }

    fn walk_to(x: f64, y: f64, z: f64) -> MovePacket {
        MovePacket {
            pos: Some((x, y, z)),
            rot: None,
            on_ground: true,
            horizontal_collision: false,
        }
    }

    /// The two anchors are different quantities, and this is the assertion
    /// that fails for a port using one for both.
    ///
    /// Ten packets of 0.2 blocks in one tick: the delta of each is 0.2 from
    /// `lastGood`, but the speed check sees the whole 2.0 from `firstGood`,
    /// which is what makes the budget a tick's budget.
    #[test]
    fn the_speed_check_anchors_on_first_good_and_the_delta_on_last_good() {
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good()); // out of the tickCount == 0 special case
        let g = Gates::default();
        let mut at = (0.0, 64.0, 0.0);
        for i in 1..=10 {
            let target = (f64::from(i) * 0.2, 64.0, 0.0);
            let Before::Accept(a) = t.before_move(walk_to(target.0, 64.0, 0.0), g, now_at(at))
            else {
                panic!("packet {i} should be accepted");
            };
            assert!(
                (a.delta.0 - 0.2).abs() < 1.0e-12,
                "packet {i}: the delta is lastGood-relative, got {}",
                a.delta.0
            );
            let after = t.after_move(
                a,
                walk_to(target.0, 64.0, 0.0),
                g,
                target,
                || false,
                || false,
            );
            let After::Accept { to, .. } = after else {
                panic!("packet {i} should land");
            };
            at = to;
        }
        assert!(
            (t.first_good().0 - 0.0).abs() < f64::EPSILON,
            "firstGood did not move"
        );
        assert!(
            (t.last_good().0 - 2.0).abs() < 1.0e-12,
            "lastGood tracked every packet"
        );
    }

    /// The budget: one packet's allowance is 100 blocks squared, so a 9-block
    /// step passes and an 11-block one does not.
    #[test]
    fn the_speed_budget_is_a_hundred_per_packet_and_a_flood_gets_one() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        assert!(matches!(
            t.before_move(walk_to(9.0, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0))),
            Before::Accept(_)
        ));

        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        assert_eq!(
            t.before_move(walk_to(11.0, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0))),
            Before::Correct {
                yaw: 0.0,
                pitch: 0.0,
                reason: CorrectReason::TooQuick
            }
        );

        // Six packets in one tick clamps the count to 1, so the budget is
        // still 100 and not 600. A port that clamps to 5 accepts 20 blocks.
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        for _ in 0..6 {
            let _ = t.before_move(walk_to(0.1, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0)));
        }
        assert_eq!(
            t.before_move(walk_to(20.0, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0))),
            Before::Correct {
                yaw: 0.0,
                pitch: 0.0,
                reason: CorrectReason::TooQuick
            },
            "the flood clamp is to one packet's budget"
        );
    }

    /// A player walking into a wall: the client asks for a position the
    /// server's collision refuses, the residual is under 0.0625, and vanilla
    /// accepts the CLIENT's position anyway. This is the scenario the
    /// gameplay corpus records no correction for.
    #[test]
    fn walking_into_a_wall_is_accepted_at_the_clients_position() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let pkt = walk_to(0.2, 64.0, 0.0);
        let Before::Accept(a) = t.before_move(pkt, g, now_at((0.0, 64.0, 0.0))) else {
            panic!("accepted");
        };
        // Collision stopped the player 0.05 short — under the 0.25-block
        // (0.0625 squared) tolerance.
        let after = t.after_move(
            a,
            pkt,
            g,
            (0.15, 64.0, 0.0),
            || panic!("not asked"),
            || false,
        );
        assert_eq!(
            after,
            After::Accept {
                to: (0.2, 64.0, 0.0),
                yaw: 0.0,
                pitch: 0.0,
                movement: (0.2, 0.0, 0.0),
                reset_fall_distance: false,
                on_ground: true,
                horizontal_collision: false,
                try_reset_impulse_unless: TryResetImpulse { known: true },
            },
            "the accepted position is the client's, not the swept one"
        );
        // The positive control: the same shape with a residual over the
        // tolerance and a free old box IS corrected.
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let pkt = walk_to(2.0, 64.0, 0.0);
        let Before::Accept(a) = t.before_move(pkt, g, now_at((0.0, 64.0, 0.0))) else {
            panic!("accepted");
        };
        assert!(matches!(
            t.after_move(a, pkt, g, (0.15, 64.0, 0.0), || true, || false),
            After::Correct { .. }
        ));
    }

    /// The vertical residual is dropped unconditionally, so a two-block
    /// vertical disagreement does not rubber-band while a two-block
    /// horizontal one does.
    #[test]
    fn the_vertical_residual_never_counts() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let pkt = walk_to(0.0, 66.0, 0.0);
        let Before::Accept(a) = t.before_move(pkt, g, now_at((0.0, 64.0, 0.0))) else {
            panic!("accepted");
        };
        // Collision left the player two blocks below where the client asked.
        assert!(matches!(
            t.after_move(
                a,
                pkt,
                g,
                (0.0, 64.0, 0.0),
                || panic!("not asked"),
                || false
            ),
            After::Accept { .. }
        ));
    }

    /// A jump is the only thing the server starts for itself here, and it
    /// needs all three terms.
    #[test]
    fn a_jump_needs_the_server_on_ground_the_packet_airborne_and_a_rise() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let airborne = MovePacket {
            pos: Some((0.0, 64.42, 0.0)),
            rot: None,
            on_ground: false,
            horizontal_collision: false,
        };
        let Before::Accept(a) = t.before_move(airborne, g, now_at((0.0, 64.0, 0.0))) else {
            panic!("accepted");
        };
        assert!(a.jump_from_ground);
        assert!(a.asked_to_rise);

        // Same rise, but the packet says still on the ground.
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let Before::Accept(a) =
            t.before_move(walk_to(0.0, 64.42, 0.0), g, now_at((0.0, 64.0, 0.0)))
        else {
            panic!("accepted");
        };
        assert!(!a.jump_from_ground, "the packet's flag is one of the three");
        assert!(a.asked_to_rise);

        // Same packet, but the server had them airborne already.
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        let mut now = now_at((0.0, 64.0, 0.0));
        now.on_ground = false;
        let Before::Accept(a) = t.before_move(airborne, g, now) else {
            panic!("accepted");
        };
        assert!(!a.jump_from_ground);
    }

    /// While a teleport is outstanding, a move packet moves nothing.
    #[test]
    fn an_outstanding_teleport_swallows_every_move() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        t.tick(t.last_good());
        t.begin_teleport((0.0, 64.0, 0.0));
        assert_eq!(
            t.before_move(walk_to(5.0, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0))),
            Before::SnapRotation {
                yaw: 0.0,
                pitch: 0.0
            }
        );
        assert!((t.last_good().0 - 0.0).abs() < f64::EPSILON);
        // Positive control: acknowledge it and the next move lands.
        let id = t.awaiting_teleport();
        t.accept_teleport(id).expect("acknowledged");
        assert!(matches!(
            t.before_move(walk_to(5.0, 64.0, 0.0), g, now_at((0.0, 64.0, 0.0))),
            Before::Accept(_)
        ));
    }

    /// A rotation-only packet keeps the position, and the validation getters
    /// still default its position to zero rather than to the player's.
    #[test]
    fn a_rotation_only_packet_holds_the_position_and_validates_against_zero() {
        let g = Gates::default();
        let mut t = Tracker::new((100.0, 64.0, 100.0));
        t.tick(t.last_good());
        let pkt = MovePacket {
            pos: None,
            rot: Some((370.0, -12.0)),
            on_ground: true,
            horizontal_collision: false,
        };
        let Before::Accept(a) = t.before_move(pkt, g, now_at((100.0, 64.0, 100.0))) else {
            panic!("accepted");
        };
        assert!((a.target.0 - 100.0).abs() < f64::EPSILON);
        assert!((a.delta.0).abs() < f64::EPSILON, "no movement");
        assert!((a.yaw - 10.0).abs() < 1.0e-4, "the yaw is wrapped");
    }

    #[test]
    fn wrap_degrees_folds_both_ways() {
        assert!((wrap_degrees(0.0) - 0.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(179.0) - 179.0).abs() < f32::EPSILON);
        // 180 itself folds: the compare is `>= 180`, not `> 180`.
        assert!((wrap_degrees(180.0) + 180.0).abs() < f32::EPSILON);
        assert!((wrap_degrees(370.0) - 10.0).abs() < 1.0e-4);
        assert!((wrap_degrees(-190.0) - 170.0).abs() < 1.0e-4);
        // And -180 stays: the second compare is `< -180`.
        assert!((wrap_degrees(-180.0) + 180.0).abs() < f32::EPSILON);
    }

    /// The positive control for the clamps: a value inside the range comes
    /// back bit-identical, so a test that only checks the ends cannot pass
    /// against a function that clamps everything.
    #[test]
    fn the_clamps_are_asymmetric_and_pass_the_middle_through() {
        assert!((clamp_horizontal(1234.5) - 1234.5).abs() < f64::EPSILON);
        assert!((clamp_vertical(1234.5) - 1234.5).abs() < f64::EPSILON);
        assert!((clamp_horizontal(4.0e7) - 3.0e7).abs() < f64::EPSILON);
        assert!((clamp_horizontal(-4.0e7) + 3.0e7).abs() < f64::EPSILON);
        // The vertical limit is 2e7, so 2.5e7 clamps vertically and does not
        // clamp horizontally. A port using one constant fails exactly here.
        assert!((clamp_vertical(2.5e7) - 2.0e7).abs() < f64::EPSILON);
        assert!((clamp_horizontal(2.5e7) - 2.5e7).abs() < f64::EPSILON);
    }

    /// An infinite position passes and an infinite rotation does not. This
    /// is the assertion that a port testing all five the same way fails.
    #[test]
    fn infinity_is_invalid_for_rotation_and_valid_for_position() {
        assert!(!contains_invalid_values(f64::INFINITY, 0.0, 0.0, 0.0, 0.0));
        assert!(contains_invalid_values(f64::NAN, 0.0, 0.0, 0.0, 0.0));
        assert!(contains_invalid_values(0.0, 0.0, 0.0, f32::INFINITY, 0.0));
        assert!(contains_invalid_values(0.0, 0.0, 0.0, 0.0, f32::NAN));
        // Positive control: an ordinary move is valid.
        assert!(!contains_invalid_values(8.5, 65.0, -3.25, 90.0, -12.0));
    }

    #[test]
    fn the_teleport_id_wraps_to_zero_at_int_max() {
        let mut t = Tracker::new((0.0, 0.0, 0.0));
        assert_eq!(t.begin_teleport((1.0, 2.0, 3.0)), 1);
        assert_eq!(t.begin_teleport((1.0, 2.0, 3.0)), 2);
        t.awaiting_teleport = i32::MAX - 1;
        assert_eq!(t.begin_teleport((1.0, 2.0, 3.0)), 0, "MAX becomes 0");
        assert_eq!(t.begin_teleport((1.0, 2.0, 3.0)), 1);
    }

    #[test]
    fn a_stale_acknowledgement_is_ignored_and_a_matching_one_with_no_teleport_disconnects() {
        let mut t = Tracker::new((0.0, 0.0, 0.0));
        let id = t.begin_teleport((5.0, 65.0, 5.0));
        assert_eq!(t.accept_teleport(id - 1), Ok(None), "stale id, no error");
        assert_eq!(t.accept_teleport(id), Ok(Some((5.0, 65.0, 5.0))));
        assert_eq!(t.last_good(), (5.0, 65.0, 5.0));
        // Now nothing is outstanding, and the same id arriving again is the
        // disconnect case.
        assert_eq!(t.accept_teleport(id), Err(()));
    }

    /// The per-tick reset is what makes the budget survive a long walk.
    ///
    /// Twenty ticks of four blocks each is eighty blocks from the join, and
    /// no packet is ever refused — because `firstGood` moves with the player
    /// every tick. With `firstGood` pinned at the join (the reading
    /// `handleMovePlayer` alone suggests) the eleventh block is a
    /// rubber-band, and `530-walking-away-drops-a-standing-player-from-view`
    /// walks 85.
    #[test]
    fn a_long_walk_is_never_refused_because_the_anchor_moves_every_tick() {
        let g = Gates::default();
        let mut t = Tracker::new((0.0, 64.0, 0.0));
        let mut at = (0.0, 64.0, 0.0);
        for step in 1..=20 {
            t.tick(at);
            let target = (0.0, 64.0, f64::from(step) * 4.0);
            let pkt = walk_to(target.0, target.1, target.2);
            let Before::Accept(a) = t.before_move(pkt, g, now_at(at)) else {
                panic!("step {step} at z={} was refused", target.2);
            };
            let After::Accept { to, .. } = t.after_move(a, pkt, g, target, || false, || false)
            else {
                panic!("step {step} corrected after the move");
            };
            at = to;
        }
        assert!((at.2 - 80.0).abs() < 1.0e-9, "eighty blocks from the join");
    }

    #[test]
    fn the_resend_waits_more_than_twenty_ticks() {
        let mut t = Tracker::new((0.0, 0.0, 0.0));
        t.begin_teleport((1.0, 2.0, 3.0));
        for _ in 0..20 {
            t.tick(t.last_good());
            assert_eq!(
                t.update_awaiting_teleport(0.0, 0.0),
                AwaitingTeleport::Awaiting
            );
        }
        t.tick(t.last_good());
        assert!(matches!(
            t.update_awaiting_teleport(0.0, 0.0),
            AwaitingTeleport::Resend { .. }
        ));
    }

    #[test]
    fn the_movement_check_is_off_for_the_owner_and_for_elytra_by_rule() {
        let g = Gates::default();
        assert!(Tracker::should_check_player_movement(g));
        assert!(!Tracker::should_check_player_movement(Gates {
            singleplayer_owner: true,
            ..g
        }));
        assert!(!Tracker::should_check_player_movement(Gates {
            player_movement_check: false,
            ..g
        }));
        // The elytra rule only bites while fall-flying.
        assert!(Tracker::should_check_player_movement(Gates {
            elytra_movement_check: false,
            ..g
        }));
        assert!(!Tracker::should_check_player_movement(Gates {
            fall_flying: true,
            elytra_movement_check: false,
            ..g
        }));
    }
}
