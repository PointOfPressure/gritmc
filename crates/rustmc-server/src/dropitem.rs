//! Throwing an item on the floor — `LivingEntity.createItemStackToDrop`'s
//! aimed branch, and the `ItemEntity` it produces.
//!
//! Reference (26.2), read from the pinned jar: `LivingEntity.drop(ItemStack,
//! boolean, boolean)`, `LivingEntity.createItemStackToDrop` (the private one),
//! `EntityTypes.ITEM`, and `Mth.sin`/`Mth.cos`, which are table lookups and
//! not `f32::sin`.
//!
//! # The one thing here that cannot be reproduced, said first
//!
//! **Ten of this drop's numbers come from two unseeded `RandomSource`s, so no
//! implementation reproduces them — vanilla included.** Six from the item's
//! own generator (a uuid, a bob phase, **a spawn yaw**, and a delta the aimed
//! branch overwrites) and four from the thrower's (the velocity). The yaw was
//! missing from this module until 2026-08-08 and from
//! `docs/PARITY-ROADMAP.md` §3 S4's draw account as well; see §11.
//! `ci/checks/gameplay-unstable.toml` records
//! `dropping-an-item-throws-it-on-the-floor` at **25 flaps in 25 comparisons
//! at N = 26**: twenty-six cold vanilla servers, and the `add_entity` tail
//! digest differed in every single pairing.
//!
//! That is not a licence to approximate the rest. The split is sharp and it
//! is worth stating exactly where it falls, because it is the difference
//! between a check that can never pass and a check that is failing:
//!
//! * the **position** is `(getX(), getEyeY() - 0.30000001192092896, getZ())`,
//!   entirely deterministic, and the same file records it as identical to the
//!   last bit across all twenty-six runs — `[0.0, 1.3199999928474426, 0.0]`
//!   relative, which is a player's 1.62 eye height less 0.3. **This must
//!   match**, and if it does not that is our defect and not vanilla's noise;
//! * the **velocity** takes four `nextFloat()` draws and cannot match;
//! * the **yaw** takes one, from the other generator, and cannot match
//!   either. That entry has named it since 2026-08-05 —
//!   `setYRot(random.nextFloat() * 360.0F)`, offsets 44-52 — and **this
//!   module denied it** until 2026-08-08.
//!
//! So this port uses [`rustmc_worldgen::rng::legacy::LegacyRandomSource`] —
//! vanilla's own `java.util.Random` LCG, already golden-tested in this tree —
//! rather than any convenient generator. The seed is unknowable, so the draws
//! differ; the *arithmetic they feed*, and therefore the distribution and the
//! rounding, is vanilla's. That is the most faithful thing available, and the
//! alternative of inventing a formula would be an approximation of something
//! we can read exactly.
//!
//! # After it is thrown
//!
//! `ItemEntity.tick` — gravity, drag, block collision through
//! `crate::physics::move_entity`, the pickup delay, merging with nearby
//! stacks, the 6000-tick despawn — is [`crate::itementity`], and a player
//! taking it back is [`crate::pickup`]. Until 2026-10-01 neither existed and
//! a thrown item hung in the air where it was announced; this module's header
//! said so and argued, correctly, that faking the fall with a floor-finder
//! would have been a one-way door. The real `Entity.move` was ported first
//! (`crate::physics`), and the item tick stands on it.

use crate::entities::{Entities, Entity, ITEM};
use rustmc_worldgen::math;
use rustmc_worldgen::rng::{legacy::LegacyRandomSource, RandomSource};

/// `LivingEntity.createItemStackToDrop`'s eye offset: the item leaves the
/// thrower's hands `0.30000001192092896` below eye level.
///
/// Written as the exact `double` the bytecode holds rather than as `0.3`,
/// which is a different number: `0.3` is `0.299999999999999988897769753748`
/// and this constant is the `float` `0.3f` widened. The difference is about
/// `1.2e-8` of a block — invisible to a player and *not* invisible to a
/// comparison of two `f64`s on the wire, which is what `add_entity` carries.
pub const DROP_EYE_OFFSET: f64 = 0.300_000_011_920_928_96;

/// `Avatar`'s eye height, from `EntityTypes.PLAYER`'s `.eyeHeight(1.62f)`.
///
/// **The widened `float`, not the `f64` literal `1.62`**, and the difference
/// is the whole of a bug this module's test caught before it shipped.
/// `Entity.getEyeY()` is
///
/// ```text
///   getfield position.y : D
///   getfield eyeHeight  : F
///   f2d
///   dadd
/// ```
///
/// — the height is stored as a `float` and *widened*, so it is
/// `1.6200000047683716` and not `1.62`. Written as `1.62` the drop lands at
/// `1.3199999880790712` above the thrower's feet; vanilla's own recorded
/// position, identical across all twenty-six runs of the N = 26 power test,
/// is `1.3199999928474426`. Four and a half nanometres of a block, invisible
/// to a player, and a guaranteed `add_entity` divergence on a field that is
/// otherwise one of the few things about a thrown item that *can* be
/// compared.
pub const PLAYER_EYE_HEIGHT: f64 = 1.620_000_004_768_371_6;

/// `Mth.DEG_TO_RAD` — `0.017453292f`, a **float**, and the multiply happens in
/// `f32` before being widened for the table lookup.
const DEG_TO_RAD: f32 = 0.017_453_292;

/// The velocity an aimed throw gives an item — `createItemStackToDrop` at
/// offset 125, the `dropAround == false` branch.
///
/// ```java
/// float f  = 0.3F;
/// float f1 = Mth.sin(xRot * 0.017453292F);
/// float f2 = Mth.cos(xRot * 0.017453292F);
/// float f3 = Mth.sin(yRot * 0.017453292F);
/// float f4 = Mth.cos(yRot * 0.017453292F);
/// float f5 = random.nextFloat() * 6.2831855F;
/// float f6 = 0.02F * random.nextFloat();
/// itemEntity.setDeltaMovement(
///     -f3 * f2 * 0.3F + Math.cos(f5) * f6,
///     -f1 * 0.3F + 0.1F + (random.nextFloat() - random.nextFloat()) * 0.1F,
///      f4 * f2 * 0.3F + Math.sin(f5) * f6);
/// ```
///
/// # Three details that a transcription loses
///
/// * `Mth.sin`/`Mth.cos` are **65536-entry table lookups**, not the libm
///   functions — but `Math.cos(f5)` and `Math.sin(f5)` in the *same
///   expression* are the real ones. Four trigonometric calls, two of each
///   kind, and mixing them up changes the answer in the fifth decimal.
/// * The draw order is `f5`, then `f6`, then the two that make the vertical
///   difference — **four draws, and the y term consumes the last two in
///   left-to-right order**. A generator advanced in a different order gives
///   a differently-shaped scatter even with the same seed.
/// * The horizontal terms are computed in `f32` and only widened at the
///   `f2d` before the addition, so `-f3 * f2 * 0.3F` rounds once as a float
///   before it ever meets the `Math.cos` term.
#[must_use]
pub fn throw_velocity(x_rot: f32, y_rot: f32, rng: &mut impl RandomSource) -> (f64, f64, f64) {
    let f1 = math::sin(f64::from(x_rot * DEG_TO_RAD));
    let f2 = math::cos(f64::from(x_rot * DEG_TO_RAD));
    let f3 = math::sin(f64::from(y_rot * DEG_TO_RAD));
    let f4 = math::cos(f64::from(y_rot * DEG_TO_RAD));
    let f5 = rng.next_f32() * 6.283_185_5;
    let f6 = 0.02 * rng.next_f32();
    let spread = rng.next_f32() - rng.next_f32();
    (
        f64::from(-f3 * f2 * 0.3) + f64::from(f5).cos() * f64::from(f6),
        f64::from((-f1 * 0.3) + 0.1 + spread * 0.1),
        f64::from(f4 * f2 * 0.3) + f64::from(f5).sin() * f64::from(f6),
    )
}

/// Spawn the item a player just threw, and return it.
///
/// `Player.drop(stack, false)` reaches `LivingEntity.drop(stack, false, true)`
/// — dropped *aimed*, and traced back to the thrower — which builds the
/// entity, sets a 40-tick pickup delay so the thrower cannot instantly
/// re-collect it, and hands it to `Level.addFreshEntity`.
///
/// The delay itself, the thrower and the stack are the caller's to record in
/// [`crate::itementity::ItemEntities`] — [`crate::itementity::THROWN_PICKUP_DELAY`]
/// is the 40 — because this function has no stack: it builds the entity
/// the tracker announces, and the item tick reads the rest from the store.
pub fn spawn_thrown_item(
    entities: &Entities,
    thrower: &Entity,
    rng: &mut impl RandomSource,
) -> Entity {
    // **The item has its own `RandomSource` and it is not the thrower's.**
    //
    // `Entity.<init>` offsets 43-46 are `this.random = RandomSource.create()`,
    // so every entity carries a generator of its own, and the draws below come
    // out of *that* one. Six of them, in this order, none of them optional:
    // getting the order wrong desynchronises everything after it from vanilla,
    // and getting the *source* wrong desynchronises the thrower as well.
    let mut item_rng = unique_random();
    let uuid = insecure_uuid(&mut item_rng);
    // `ItemEntity(EntityType, Level)` offsets 21-36: `bobOffs` is a client-side
    // bobbing phase that never reaches the wire. **Drawn anyway**, because the
    // draw is what advances the stream that the yaw below reads.
    let _bob_offs = item_rng.next_f32() * std::f32::consts::PI * 2.0;
    // Offsets 39-52: `setYRot(this.random.nextFloat() * 360.0F)`.
    let y_rot = item_rng.next_f32() * 360.0;
    // Offsets 23-61 of the five-argument constructor: the delta the aimed
    // branch overwrites four instructions later. Drawn and discarded for the
    // same reason as `bobOffs` — and unlike `bobOffs` this one is also what a
    // future port of the `dropAround` branch (offsets 64-119, the death drop,
    // which does not overwrite) will need.
    let _ctor_dx = item_rng.next_f64() * 0.2 - 0.1;
    let _ctor_dz = item_rng.next_f64() * 0.2 - 0.1;
    let e = Entity {
        id: entities.next_id(),
        // `Entity.<init>` offsets 94-100: `Mth.createInsecureUUID(this.random)`
        // — the *entity's* random, two `nextLong`s, most significant half
        // first. The oracle digests it out, so no comparison sees it; the draw
        // order still matters because the same stream feeds the yaw.
        uuid,
        kind: ITEM,
        x: thrower.x,
        y: thrower.y + PLAYER_EYE_HEIGHT - DROP_EYE_OFFSET,
        z: thrower.z,
        // **A random spawn yaw, and this comment used to deny it.**
        //
        // It said `ItemEntity`'s constructor "sets a position, an item and a
        // velocity and **no rotation**, so both stay at zero", and that this
        // had been "checked in the bytecode rather than assumed". It had not.
        // `ItemEntity(EntityType, Level)` offsets 39-52 are
        // `setYRot(this.random.nextFloat() * 360.0F)`, so every item entity in
        // the game spins to a fresh angle as it is created — and that angle
        // rides `add_entity`'s rotation byte.
        //
        // **The rest of the tree already knew.** This was not a discovery in
        // 2026-08-08's S4 measurement, and claiming it as one was withdrawn
        // the same night: `ci/checks/gameplay-unstable.toml` has carried the
        // same call at the same offsets since 2026-08-05, and
        // `docs/intel/movement-tick-three-checks.md` counts the spawn rotation
        // as a fifth random field. What was found is only that **this file
        // contradicted them** — which is worse than not knowing, because a
        // comment claiming to have read the bytecode is what stops the next
        // reader looking. `docs/PARITY-ROADMAP.md` §11.
        //
        // Fixing it converts nothing and cannot: vanilla's angle is a draw
        // from an unseeded generator and ours is a draw from a different one.
        // It is here because the alternative is a server that announces every
        // dropped item at yaw zero, which is a thing vanilla never does.
        y_rot,
        x_rot: 0.0,
        y_head_rot: 0.0,
        movement: throw_velocity(thrower.x_rot, thrower.y_rot, rng),
        // Freshly thrown and still in the air, which is what vanilla's
        // `Entity.onGround` is for a just-constructed entity.
        on_ground: false,
        data: 0,
        // `LivingEntity`'s own initial values: not swinging, and a null
        // `swingingArm`, which `getCurrentSwingDuration` reads as the main
        // hand.
        swinging: false,
        swing_time: 0,
        swinging_arm: None,
        // An `ItemEntity` is not a `LivingEntity` and has none of the
        // accessors this server models. It does have one of its own —
        // `DATA_ITEM` — which is away from its default the moment the stack
        // is set, so vanilla sends a `set_entity_data` for a dropped item and
        // this server does not. Named here and in `docs/SERVER.md` rather
        // than left as an unexplained silence; carrying it belongs with the
        // rest of the drop chain.
        synched: crate::synched::Synched::EntityOnly,
    };
    entities.insert(e.clone());
    e
}

/// `Mth.createInsecureUUID(RandomSource)` — two `nextLong`s, high half first.
///
/// ```text
///   msb = nextLong() & -61441L                    | 16384L
///   lsb = nextLong() & 4611686018427387903L       | -9223372036854775808L
/// ```
///
/// The masks are version 4 and variant 2, and they are written as the exact
/// longs the constant pool holds rather than as `0xF000` arithmetic, because
/// the *count* and the *order* of the draws are the part that matters here:
/// this runs on the entity's own generator, immediately before the two
/// `nextFloat`s that decide its spawn yaw.
///
/// `pub(crate)` because it is `Entity`'s constructor and not `ItemEntity`'s:
/// every fresh entity gets its uuid this way, and [`crate::fallable::spawn`]
/// is the second caller. A falling block draws no yaw after it, so for that
/// kind these two longs are the whole of its unseeded draw account. The version bits themselves are
/// invisible — the oracle consumes the sixteen uuid bytes and never digests
/// them — but a port that drew one long, or drew them the other way round,
/// would put a different angle on the wire for every dropped item.
pub(crate) fn insecure_uuid(rng: &mut impl RandomSource) -> u128 {
    let msb = (rng.next_i64() & -61441) | 16384;
    let lsb = (rng.next_i64() & 4_611_686_018_427_387_903) | i64::MIN;
    ((msb as u64 as u128) << 64) | (lsb as u64 as u128)
}

/// A fresh `Entity.random` — `RandomSource.create()` on
/// `RandomSupport.generateUniqueSeed()`.
///
/// **This is vanilla's seed construction and not an approximation of it.**
/// `RandomSupport.generateUniqueSeed` is
///
/// ```text
///   SEED_UNIQUIFIER.updateAndGet(l -> l * 1181783497276652981L) ^ System.nanoTime()
/// ```
///
/// with `SEED_UNIQUIFIER` initialised to `8682522807148012L` in the class
/// initialiser. The clock half is unknowable and the multiplier half is not,
/// and the multiplier half is the half that matters for a server: it is what
/// stops two entities constructed inside the same clock tick from getting the
/// same generator. The version this replaced took the seed from
/// `SystemTime::now()` alone, which on a coarse clock hands two entities
/// created in one millisecond an identical stream — and this function is now
/// called twice per thrown item, once for the item and once for the thrower,
/// so that stopped being hypothetical.
///
/// Everything downstream of the seed is vanilla's: the generator is
/// [`LegacyRandomSource`], `java.util.Random`'s LCG, golden-tested in this
/// tree.
///
/// # One deviation, named rather than glossed
///
/// In vanilla `Entity.random` is a **field**, created once with the entity and
/// advanced by everything that entity ever does. Here the *item's* generator
/// is created once with the item, which matches — but the **thrower's** is
/// created fresh on every drop, because this server keeps no per-player
/// generator. Nothing observable turns on it: both are unseeded, so neither
/// vanilla's stream position nor ours is reproducible either way, and the
/// draws a single drop makes are in vanilla's order within each. It would
/// start to matter the moment a player has a second unseeded mechanic whose
/// draws must interleave with a drop's, and at that point the generator
/// belongs on the player rather than here.
#[must_use]
pub fn unique_random() -> LegacyRandomSource {
    use std::sync::atomic::{AtomicI64, Ordering};
    /// `RandomSupport.SEED_UNIQUIFIER`, from the class initialiser.
    static SEED_UNIQUIFIER: AtomicI64 = AtomicI64::new(8_682_522_807_148_012);
    // `updateAndGet` returns the *new* value; `fetch_update` returns the old
    // one and cannot fail here, because the closure always returns `Some`.
    let previous = SEED_UNIQUIFIER
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |l| {
            Some(l.wrapping_mul(1_181_783_497_276_652_981))
        })
        .unwrap_or(8_682_522_807_148_012);
    let uniquified = previous.wrapping_mul(1_181_783_497_276_652_981);
    // `System.nanoTime()` is monotonic and has no epoch; the wall clock is the
    // nearest thing available and its low bits are what this uses.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla xors the full 64 bits of nanoTime; only the low bits carry entropy"
    )]
    let clock = nanos as i64;
    LegacyRandomSource::new(uniquified ^ clock)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The position, which is the half of this that **must** match vanilla.
    ///
    /// `gameplay-unstable.toml` records the item arriving at
    /// `[0.0, 1.3199999928474426, 0.0]` relative to the thrower on all
    /// twenty-six runs of the N = 26 power test. That number is
    /// `1.62 - 0.30000001192092896` in `f64`, and it is the assertion that
    /// catches a `0.3` written where the widened `0.3f` belongs.
    #[test]
    fn a_thrown_item_leaves_the_hand_at_vanillas_exact_height() {
        let offset = PLAYER_EYE_HEIGHT - DROP_EYE_OFFSET;
        assert!(
            (offset - 1.319_999_992_847_442_6).abs() < f64::EPSILON,
            "got {offset}, vanilla's recorded 1.3199999928474426"
        );
        // The two traps this pins, both of which are "an f32 constant
        // written as an f64 literal" and both of which land within 1e-8 of
        // the right answer:
        assert_ne!(
            (PLAYER_EYE_HEIGHT - 0.3).to_bits(),
            offset.to_bits(),
            "0.3 and the widened 0.3f are not the same number"
        );
        assert_ne!(
            (1.62_f64 - DROP_EYE_OFFSET).to_bits(),
            offset.to_bits(),
            "1.62 and the widened 1.62f are not the same number — this one \
             was written wrong first and the recorded vanilla position \
             caught it"
        );
    }

    /// The velocity is bounded even though it is random: the aimed throw is
    /// 0.3 blocks a tick plus at most 0.02 of scatter, and the vertical term
    /// is 0.1 plus at most 0.1 of difference. A sign error or a missing
    /// `0.3` multiplier leaves this range immediately.
    #[test]
    fn an_aimed_throw_is_bounded_and_mostly_forward() {
        let mut rng = LegacyRandomSource::new(42);
        // Facing due south (yaw 0), level (pitch 0): vanilla's yaw 0 is +z,
        // so the throw goes +z and nowhere else but the scatter.
        for _ in 0..64 {
            let (vx, vy, vz) = throw_velocity(0.0, 0.0, &mut rng);
            assert!(vz > 0.27 && vz < 0.33, "forward along +z, got {vz}");
            assert!(vx.abs() < 0.03, "no sideways component but scatter: {vx}");
            assert!((0.0..=0.21).contains(&vy), "up and gently, got {vy}");
        }
    }

    /// Looking straight down throws the item down; straight up throws it up.
    /// This is the assertion a swapped `sin`/`cos` or a lost negation fails.
    #[test]
    fn pitch_decides_whether_the_item_goes_up_or_down() {
        let mut rng = LegacyRandomSource::new(7);
        // xRot +90 is looking straight down in Minecraft's convention.
        let (_, down, _) = throw_velocity(90.0, 0.0, &mut rng);
        let (_, up, _) = throw_velocity(-90.0, 0.0, &mut rng);
        assert!(down < 0.0, "aiming down throws down, got {down}");
        assert!(up > 0.3, "aiming up throws up, got {up}");
    }

    /// Four draws, in that order. A port that consumed three or five would
    /// still produce a plausible scatter and would desynchronise every later
    /// draw from the same source.
    #[test]
    fn an_aimed_throw_consumes_exactly_four_draws() {
        let mut used = LegacyRandomSource::new(99);
        let _ = throw_velocity(0.0, 0.0, &mut used);
        let mut counted = LegacyRandomSource::new(99);
        for _ in 0..4 {
            counted.next_f32();
        }
        assert_eq!(
            used.next_i64(),
            counted.next_i64(),
            "the generator is four nextFloat calls further on"
        );
    }

    /// **The spawn yaw exists, is drawn, and is not zero.**
    ///
    /// This is the assertion the module's old comment would have failed. It
    /// said `ItemEntity`'s constructor sets "no rotation, so both stay at
    /// zero" and claimed to have read that out of the bytecode; offsets 39-52
    /// of `ItemEntity(EntityType, Level)` are
    /// `setYRot(this.random.nextFloat() * 360.0F)`.
    ///
    /// A hundred spawns of a real thrower, and the test is the pair: every
    /// yaw inside `[0, 360)`, and **more than one distinct value**. Either
    /// half alone passes for a server that always writes zero.
    #[test]
    fn every_dropped_item_gets_a_random_spawn_yaw() {
        let entities = Entities::new();
        let thrower = Entity {
            id: entities.next_id(),
            uuid: 1,
            kind: crate::entities::PLAYER,
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
            // The thrower is a player, so `Avatar` and not `EntityOnly`. The
            // values are the definition defaults; nothing here reads them.
            synched: crate::synched::Synched::Avatar {
                health: 1.0,
                model_customisation: 0,
                score: 0,
            },
        };
        let mut rng = LegacyRandomSource::new(1234);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..100 {
            let item = spawn_thrown_item(&entities, &thrower, &mut rng);
            assert!(
                (0.0..360.0).contains(&item.y_rot),
                "nextFloat() * 360 is [0, 360), got {}",
                item.y_rot
            );
            assert!(
                (item.x_rot - 0.0).abs() < f32::EPSILON
                    && (item.y_head_rot - 0.0).abs() < f32::EPSILON,
                "only the yaw is drawn — pitch and head yaw stay zero"
            );
            seen.insert(item.y_rot.to_bits());
        }
        assert!(
            seen.len() > 90,
            "a hundred items should have ~a hundred distinct yaws, got {}; a \
             constant here is the bug this test exists for",
            seen.len()
        );
    }

    /// The item's generator advances by **six** draws, in vanilla's order.
    ///
    /// Two `nextLong` for `Mth.createInsecureUUID`, two `nextFloat` for
    /// `bobOffs` and the yaw, two `nextDouble` for the constructor's delta.
    /// `docs/PARITY-ROADMAP.md` §3 S4 counted two of the six; a port that
    /// drew any other number would still produce a plausible item and would
    /// put every later draw from that entity on a different value.
    ///
    /// It is asserted through the yaw because the yaw is the only one of the
    /// six that reaches an observable field: seed a generator, walk it past
    /// the two `nextLong`s and the `bobOffs` by hand, and the next
    /// `nextFloat` must be the angle the spawner produced.
    #[test]
    fn the_items_own_generator_advances_by_six_draws_in_vanillas_order() {
        let mut counted = LegacyRandomSource::new(4242);
        let uuid = insecure_uuid(&mut counted);
        let _bob = counted.next_f32();
        let yaw = counted.next_f32() * 360.0;
        let _dx = counted.next_f64();
        let _dz = counted.next_f64();

        // The uuid is RFC 4122 version 4, variant 2 — the two bit patterns
        // `Mth.createInsecureUUID`'s masks force, and the cheapest proof that
        // both longs were drawn and combined the right way round.
        assert_eq!((uuid >> 76) & 0xF, 4, "version nibble, from the msb");
        assert_eq!((uuid >> 62) & 0x3, 0b10, "variant bits, from the lsb");

        // Six draws on, and the seventh is what a caller would see next.
        let mut walked = LegacyRandomSource::new(4242);
        for _ in 0..2 {
            walked.next_i64();
        }
        assert!(
            (walked.next_f32() - _bob).abs() < f32::EPSILON,
            "bobOffs is the third draw"
        );
        assert!(
            (walked.next_f32() * 360.0 - yaw).abs() < f32::EPSILON,
            "the yaw is the fourth draw and follows bobOffs, not the other \
             way round"
        );
    }

    /// Two generators made in the same nanosecond are still different.
    ///
    /// `RandomSupport.generateUniqueSeed` xors a clock with a uniquifier that
    /// advances on every call, and the uniquifier is the half that carries
    /// this. The version this replaced used the clock alone and is called
    /// twice per thrown item — once for the item, once for the thrower — so a
    /// coarse clock would have handed both the same stream.
    #[test]
    fn two_generators_made_back_to_back_are_not_the_same_generator() {
        let mut a = unique_random();
        let mut b = unique_random();
        assert_ne!(
            a.next_i64(),
            b.next_i64(),
            "the uniquifier, not the clock, is what makes these differ"
        );
    }
}
