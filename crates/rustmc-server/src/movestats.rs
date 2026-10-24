//! `ServerPlayer.checkMovementStatistics` and `ServerPlayer.jumpFromGround` —
//! the exhaustion a move costs.
//!
//! Reference (26.2), read from the pinned jar with `javap -p -c -constants`:
//! `net.minecraft.server.level.ServerPlayer.checkMovementStatistics(DDD)`
//! offsets 0..452, `ServerPlayer.jumpFromGround()` offsets 0..35,
//! `ServerPlayer.didNotMove(DDD)` offsets 0..24, and
//! `net.minecraft.world.entity.player.Player.causeFoodExhaustion(F)` offsets
//! 0..29.
//!
//! # Why this file exists
//!
//! `origin/player-systems` ported [`crate::food::FoodData`] whole — the
//! four-arm regeneration ladder, the exhaustion drop at 4.0, every constant —
//! and then recorded that **hunger could not move the wire**, because nothing
//! on the server ever called `causeFoodExhaustion`. `origin/player-movement`
//! is what changed that: it produces the `(dx, dy, dz)` vector
//! `checkMovementStatistics` takes, and named this method explicitly as one
//! it did not port because "this server has neither" stats nor exhaustion.
//! Half of that is still true; the other half is this file.
//!
//! # The six arms, and what each of them charges
//!
//! ```text
//!   0    if (isPassenger()) return;
//!   7    if (didNotMove(dx, dy, dz)) return;      // all three EXACTLY 0.0
//!   18   if (isSwimming())            i = round(sqrt(dx²+dy²+dz²)*100f)
//!                                     SWIM_ONE_CM              0.01f
//!   83   else if (isEyeInFluid(WATER)) i = round(sqrt(dx²+dy²+dz²)*100f)
//!                                     WALK_UNDER_WATER_ONE_CM  0.01f
//!   151  else if (isInWater())        i = round(sqrt(dx²  +dz²)*100f)
//!                                     WALK_ON_WATER_ONE_CM     0.01f
//!   212  else if (onClimbable())      if (dy > 0) CLIMB_ONE_CM, no exhaustion
//!   244  else if (onGround())         i = round(sqrt(dx²  +dz²)*100f)
//!            278  isSprinting()       SPRINT_ONE_CM            0.1f
//!            312  isCrouching()       CROUCH_ONE_CM            0.0f
//!            344  otherwise           WALK_ONE_CM              0.0f
//!   369  else if (isFallFlying())     AVIATE_ONE_CM, no exhaustion
//!   414  else                         FLY_ONE_CM if i > 25, no exhaustion
//! ```
//!
//! Four things a reimplementation gets wrong, all of them visible in that
//! table and none of them visible in prose about hunger:
//!
//! * **The third arm drops `dy`.** `isInWater()` without the eyes under
//!   water is `sqrt(dx*dx + dz*dz)` — offsets 158..167 push `dload_1` and
//!   `dload 5` and never `dload_3`. The two arms above it are the full
//!   three-dimensional length. A port that factors "the distance" out of the
//!   six arms into one helper charges a player bobbing in the shallows for
//!   vertical movement vanilla ignores.
//! * **Walking and crouching charge exactly nothing.** `fconst_0` at offsets
//!   329 and 354. The multiplication `0.0f * i * 0.01f` still happens, and
//!   still calls `causeFoodExhaustion`, so a port that "optimises away" the
//!   call is right about the arithmetic and wrong about
//!   [`crate::food::FoodData::add_exhaustion`]'s clamp being reached. It is
//!   kept as a charge of `0.0` here for the same reason.
//! * **The multiplication is `constant * i * 0.01f`, in that order**, with
//!   `i` widened by `i2f` in the middle. Not `constant * 0.01f * i`:
//!   `0.1f * 0.01f` is not representable and the two groupings differ in the
//!   last bit, which over 800 blocks of sprinting is a whole exhaustion unit
//!   in the wrong place.
//! * **`didNotMove` is three exact comparisons against `0.0`** (`dcmpl;
//!   ifne`), not an epsilon. A player standing still on a server that writes
//!   `1e-17` into a position is charged.
//!
//! # What is not here
//!
//! **The stats themselves.** This server has no `ServerStatsCounter`, no
//! `Stats` registry and no `awardStat`. Every arm's stat is *named* on
//! [`MovementStat`] and returned with its amount, so the arm that fired is
//! observable and a later stats system has the number it needs; nothing here
//! writes one. That is a deliberate gap and not a silent one — the
//! alternative, dropping the six `awardStat` calls, would make three of the
//! eight arms indistinguishable from "no arm fired".

use crate::javamath::{round_d, round_f};

/// `net.minecraft.stats.Stats`' movement counters, one per arm.
///
/// The strings are the vanilla identifiers, so a later stats system can key
/// on them directly rather than re-deriving the mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementStat {
    /// `Stats.SWIM_ONE_CM`.
    Swim,
    /// `Stats.WALK_UNDER_WATER_ONE_CM`.
    WalkUnderWater,
    /// `Stats.WALK_ON_WATER_ONE_CM`.
    WalkOnWater,
    /// `Stats.CLIMB_ONE_CM`.
    Climb,
    /// `Stats.SPRINT_ONE_CM`.
    Sprint,
    /// `Stats.CROUCH_ONE_CM`.
    Crouch,
    /// `Stats.WALK_ONE_CM`.
    Walk,
    /// `Stats.AVIATE_ONE_CM`.
    Aviate,
    /// `Stats.FLY_ONE_CM`.
    Fly,
}

impl MovementStat {
    /// The vanilla identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Swim => "minecraft:swim_one_cm",
            Self::WalkUnderWater => "minecraft:walk_under_water_one_cm",
            Self::WalkOnWater => "minecraft:walk_on_water_one_cm",
            Self::Climb => "minecraft:climb_one_cm",
            Self::Sprint => "minecraft:sprint_one_cm",
            Self::Crouch => "minecraft:crouch_one_cm",
            Self::Walk => "minecraft:walk_one_cm",
            Self::Aviate => "minecraft:aviate_one_cm",
            Self::Fly => "minecraft:fly_one_cm",
        }
    }
}

/// The predicates `checkMovementStatistics` branches on, in its own order.
///
/// Every field is one `invokevirtual` in the method, named for it. Defaults
/// are the case a plain walking player on dry land is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MoveContext {
    /// `isPassenger()` — offset 1. Returns immediately when true.
    pub passenger: bool,
    /// `isSwimming()` — offset 19.
    pub swimming: bool,
    /// `isEyeInFluid(FluidTags.WATER)` — offset 87.
    pub eye_in_water: bool,
    /// `isInWater()` — offset 152.
    pub in_water: bool,
    /// `onClimbable()` — offset 213. **Not ported by
    /// `origin/player-movement`**, which refused it for want of a stamped
    /// `tags_block.json`; a caller with no climbable table passes `false`,
    /// which is the same answer for every block that is not a ladder.
    pub climbable: bool,
    /// `onGround()` — offset 245.
    pub on_ground: bool,
    /// `isSprinting()` — offset 279.
    pub sprinting: bool,
    /// `isCrouching()` — offset 313.
    pub crouching: bool,
    /// `isFallFlying()` — offset 370.
    pub fall_flying: bool,
}

/// What one `checkMovementStatistics` call did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MovementCharge {
    /// The stat this arm would have awarded, and the amount, when the arm's
    /// own `> 0` (or `> 25`, or `dy > 0.0`) gate opened. `None` when no arm
    /// fired or the gate stayed shut.
    pub stat: Option<(MovementStat, i32)>,
    /// The argument to `causeFoodExhaustion`, when the arm reached it.
    ///
    /// `Some(0.0)` and `None` are different answers: the walk and crouch arms
    /// call it with a computed zero, which still reaches
    /// [`crate::food::FoodData::add_exhaustion`] and its 40.0 clamp.
    pub exhaustion: Option<f32>,
}

impl MovementCharge {
    /// The two early returns, and every arm whose gate stayed shut.
    const NOTHING: Self = Self {
        stat: None,
        exhaustion: None,
    };
}

/// `ServerPlayer.didNotMove(double, double, double)` — offsets 0..24.
///
/// Three exact `dcmpl; ifne` comparisons against `0.0`. Not an epsilon, and
/// not `dcmpg`: a NaN component makes `dcmpl` answer -1, which is `ifne`, so
/// a NaN delta counts as *movement*.
#[must_use]
pub fn did_not_move(dx: f64, dy: f64, dz: f64) -> bool {
    dx == 0.0 && dy == 0.0 && dz == 0.0
}

/// `ServerPlayer.checkMovementStatistics(double, double, double)`.
///
/// The vector is `new Vec3(getX() - d7, getY() - d8, getZ() - d9)` at
/// `handleMovePlayer` offsets 1146..1162 — the same one
/// [`crate::playermove::Outcome::Moved::movement`] carries.
#[must_use]
pub fn check_movement_statistics(dx: f64, dy: f64, dz: f64, ctx: MoveContext) -> MovementCharge {
    // 0..17. Two separate returns sharing one `return` instruction, which is
    // why `isPassenger()` is checked even though a passenger has usually not
    // moved either.
    if ctx.passenger || did_not_move(dx, dy, dz) {
        return MovementCharge::NOTHING;
    }

    // The two lengths. `Math.sqrt` is a `double` operation and the `d2f` at
    // offsets 41, 109, 170, 263, 392 and 426 narrows it BEFORE the multiply
    // by 100.0f, so the multiply is done in `float`.
    let len_3d = (dx * dx + dy * dy + dz * dz).sqrt();
    let len_2d = (dx * dx + dz * dz).sqrt();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2f at checkMovementStatistics offsets 41, 109, 170, 263, 392, 426"
    )]
    let f3d = len_3d as f32;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla's own d2f, same six offsets"
    )]
    let f2d = len_2d as f32;

    // `0.01f * (float) i * 0.01f`, left to right — offsets 65..77 and its
    // three copies. Written as a closure so the grouping cannot drift
    // between the arms that share it.
    #[expect(
        clippy::cast_precision_loss,
        reason = "vanilla's own i2f at offsets 71, 139, 200, 300, 332, 357"
    )]
    let charge = |constant: f32, i: i32| constant * (i as f32) * 0.01_f32;

    if ctx.swimming {
        // 18..80
        let i = round_f(f3d * 100.0);
        if i > 0 {
            return MovementCharge {
                stat: Some((MovementStat::Swim, i)),
                exhaustion: Some(charge(crate::food::consts::EXHAUSTION_SWIM, i)),
            };
        }
        return MovementCharge::NOTHING;
    }
    if ctx.eye_in_water {
        // 83..148
        let i = round_f(f3d * 100.0);
        if i > 0 {
            return MovementCharge {
                stat: Some((MovementStat::WalkUnderWater, i)),
                exhaustion: Some(charge(crate::food::consts::EXHAUSTION_SWIM, i)),
            };
        }
        return MovementCharge::NOTHING;
    }
    if ctx.in_water {
        // 151..209 — the arm that drops `dy`.
        let i = round_f(f2d * 100.0);
        if i > 0 {
            return MovementCharge {
                stat: Some((MovementStat::WalkOnWater, i)),
                exhaustion: Some(charge(crate::food::consts::EXHAUSTION_SWIM, i)),
            };
        }
        return MovementCharge::NOTHING;
    }
    if ctx.climbable {
        // 212..241 — a `double` round and an `l2i`, and no exhaustion at all.
        if dy > 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "vanilla's own l2i at checkMovementStatistics offset 237"
            )]
            let amount = round_d(dy * 100.0) as i32;
            return MovementCharge {
                stat: Some((MovementStat::Climb, amount)),
                exhaustion: None,
            };
        }
        return MovementCharge::NOTHING;
    }
    if ctx.on_ground {
        // 244..366
        let i = round_f(f2d * 100.0);
        if i > 0 {
            let (stat, constant) = if ctx.sprinting {
                (MovementStat::Sprint, crate::food::consts::EXHAUSTION_SPRINT)
            } else if ctx.crouching {
                (MovementStat::Crouch, crate::food::consts::EXHAUSTION_CROUCH)
            } else {
                (MovementStat::Walk, crate::food::consts::EXHAUSTION_WALK)
            };
            return MovementCharge {
                stat: Some((stat, i)),
                exhaustion: Some(charge(constant, i)),
            };
        }
        return MovementCharge::NOTHING;
    }
    if ctx.fall_flying {
        // 369..411 — unconditional, no `> 0` gate.
        return MovementCharge {
            stat: Some((MovementStat::Aviate, round_f(f3d * 100.0))),
            exhaustion: None,
        };
    }
    // 414..452 — the only arm with a threshold other than zero.
    let i = round_f(f2d * 100.0);
    if i > 25 {
        return MovementCharge {
            stat: Some((MovementStat::Fly, i)),
            exhaustion: None,
        };
    }
    MovementCharge::NOTHING
}

/// `ServerPlayer.jumpFromGround()` — offsets 0..35.
///
/// `super.jumpFromGround()`, `awardStat(Stats.JUMP)`, then `0.2f` when
/// sprinting and `0.05f` otherwise. The sprint arm is four times the walking
/// one, which is the largest single exhaustion source a player can drive.
#[must_use]
pub fn jump_from_ground(sprinting: bool) -> f32 {
    if sprinting {
        crate::food::consts::EXHAUSTION_SPRINT_JUMP
    } else {
        crate::food::consts::EXHAUSTION_JUMP
    }
}

/// `Player.causeFoodExhaustion(float)` — offsets 0..29.
///
/// `if (abilities.invulnerable) return;` then, server-side,
/// `foodData.addExhaustion(f)`. **`abilities.invulnerable` is set by creative
/// mode**, which is why no amount of sprinting moves a creative player's food
/// bar and why the corpus's creative substrate cannot grade this at all.
pub fn cause_food_exhaustion(
    food: &mut crate::food::FoodData,
    abilities_invulnerable: bool,
    amount: f32,
) {
    if abilities_invulnerable {
        return;
    }
    food.add_exhaustion(amount);
}

#[cfg(test)]
mod tests {
    use super::{
        cause_food_exhaustion, check_movement_statistics, did_not_move, jump_from_ground,
        MoveContext, MovementStat,
    };

    fn ground() -> MoveContext {
        MoveContext {
            on_ground: true,
            ..MoveContext::default()
        }
    }

    #[test]
    fn a_player_who_did_not_move_is_charged_nothing_and_an_epsilon_is_movement() {
        assert!(did_not_move(0.0, 0.0, 0.0));
        assert!(!did_not_move(0.0, 1.0e-17, 0.0));
        assert!(
            !did_not_move(f64::NAN, 0.0, 0.0),
            "dcmpl sends NaN to `moved`"
        );

        let none = check_movement_statistics(0.0, 0.0, 0.0, ground());
        assert_eq!(none.stat, None);
        assert_eq!(none.exhaustion, None);

        // The positive control: the same call with a real delta does fire, so
        // the assertion above is about `didNotMove` and not about the arm
        // being unreachable.
        let moved = check_movement_statistics(0.5, 0.0, 0.0, ground());
        assert_eq!(moved.stat, Some((MovementStat::Walk, 50)));
        assert_eq!(moved.exhaustion, Some(0.0));
    }

    #[test]
    fn sprinting_on_the_ground_charges_a_tenth_per_block_and_walking_charges_nothing() {
        let sprint = check_movement_statistics(
            1.0,
            0.0,
            0.0,
            MoveContext {
                sprinting: true,
                ..ground()
            },
        );
        assert_eq!(sprint.stat, Some((MovementStat::Sprint, 100)));
        // 0.1f * 100.0f * 0.01f
        assert_eq!(sprint.exhaustion, Some(0.1_f32 * 100.0_f32 * 0.01_f32));

        let walk = check_movement_statistics(1.0, 0.0, 0.0, ground());
        assert_eq!(walk.stat, Some((MovementStat::Walk, 100)));
        assert_eq!(
            walk.exhaustion,
            Some(0.0),
            "the call still happens, with a computed zero"
        );

        let crouch = check_movement_statistics(
            1.0,
            0.0,
            0.0,
            MoveContext {
                crouching: true,
                ..ground()
            },
        );
        assert_eq!(crouch.stat, Some((MovementStat::Crouch, 100)));
        assert_eq!(crouch.exhaustion, Some(0.0));
    }

    #[test]
    fn the_in_water_arm_drops_dy_and_the_two_above_it_do_not() {
        // Three units up and four along: the 3-D length is 5, the 2-D one 4.
        let d = (3.0_f64, 4.0_f64);
        let swim = check_movement_statistics(
            d.1,
            d.0,
            0.0,
            MoveContext {
                swimming: true,
                ..MoveContext::default()
            },
        );
        assert_eq!(swim.stat, Some((MovementStat::Swim, 500)));

        let eyes = check_movement_statistics(
            d.1,
            d.0,
            0.0,
            MoveContext {
                eye_in_water: true,
                ..MoveContext::default()
            },
        );
        assert_eq!(eyes.stat, Some((MovementStat::WalkUnderWater, 500)));

        let wading = check_movement_statistics(
            d.1,
            d.0,
            0.0,
            MoveContext {
                in_water: true,
                ..MoveContext::default()
            },
        );
        assert_eq!(
            wading.stat,
            Some((MovementStat::WalkOnWater, 400)),
            "offsets 158..167 push dx and dz only"
        );
        // All three charge the same constant, so the stat is the only thing
        // that separates them and the difference above is the vertical term.
        assert_eq!(swim.exhaustion, eyes.exhaustion);
        assert_eq!(wading.exhaustion, Some(0.01_f32 * 400.0_f32 * 0.01_f32));
    }

    #[test]
    fn the_arms_are_tried_in_vanillas_order() {
        // Every predicate true at once: `isSwimming` wins, and turning it off
        // hands the same move to the next arm down. This is the test that
        // would catch an `if`-chain reordered into a `match`.
        let all = MoveContext {
            swimming: true,
            eye_in_water: true,
            in_water: true,
            climbable: true,
            on_ground: true,
            sprinting: true,
            crouching: true,
            fall_flying: true,
            passenger: false,
        };
        let order = [
            (MovementStat::Swim, all),
            (
                MovementStat::WalkUnderWater,
                MoveContext {
                    swimming: false,
                    ..all
                },
            ),
            (
                MovementStat::WalkOnWater,
                MoveContext {
                    swimming: false,
                    eye_in_water: false,
                    ..all
                },
            ),
            (
                MovementStat::Climb,
                MoveContext {
                    swimming: false,
                    eye_in_water: false,
                    in_water: false,
                    ..all
                },
            ),
            (
                MovementStat::Sprint,
                MoveContext {
                    swimming: false,
                    eye_in_water: false,
                    in_water: false,
                    climbable: false,
                    ..all
                },
            ),
            (
                MovementStat::Aviate,
                MoveContext {
                    swimming: false,
                    eye_in_water: false,
                    in_water: false,
                    climbable: false,
                    on_ground: false,
                    ..all
                },
            ),
        ];
        for (want, ctx) in order {
            let got = check_movement_statistics(1.0, 1.0, 1.0, ctx);
            assert_eq!(got.stat.map(|s| s.0), Some(want), "wrong arm for {ctx:?}");
        }
        // And a passenger takes none of them.
        assert_eq!(
            check_movement_statistics(
                1.0,
                1.0,
                1.0,
                MoveContext {
                    passenger: true,
                    ..all
                }
            )
            .stat,
            None
        );
    }

    #[test]
    fn the_fly_arm_has_a_threshold_of_twenty_five_and_the_others_have_zero() {
        let air = MoveContext::default();
        // 0.25 of a block is 25, which is NOT greater than 25.
        assert_eq!(check_movement_statistics(0.25, 0.0, 0.0, air).stat, None);
        assert_eq!(
            check_movement_statistics(0.26, 0.0, 0.0, air).stat,
            Some((MovementStat::Fly, 26))
        );
        // The positive control on the *other* threshold: on the ground the
        // same 0.25 does fire, because that arm's gate is `> 0`.
        assert_eq!(
            check_movement_statistics(0.25, 0.0, 0.0, ground()).stat,
            Some((MovementStat::Walk, 25))
        );
    }

    #[test]
    fn a_jump_costs_four_times_as_much_when_sprinting() {
        assert!((jump_from_ground(false) - 0.05).abs() < f32::EPSILON);
        assert!((jump_from_ground(true) - 0.2).abs() < f32::EPSILON);
    }

    #[test]
    fn creative_invulnerability_swallows_every_charge() {
        let mut food = crate::food::FoodData::new();
        cause_food_exhaustion(&mut food, true, 4.0);
        assert!((food.exhaustion_level() - 0.0).abs() < f32::EPSILON);
        // The positive control: the same call in survival lands.
        cause_food_exhaustion(&mut food, false, 4.0);
        assert!((food.exhaustion_level() - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn two_hundred_and_forty_one_blocks_of_sprinting_is_one_food_level() {
        // **Measured, and it refutes the figure this test was written with.**
        // `docs/cold-start/player-systems.md` reasons "4.0 exhaustion is 40
        // attacks", and 0.1 a block makes that 40 one-block sprints. It is
        // 241, because `FoodData.tick` spends each 4.0 of exhaustion on
        // SATURATION while there is any and only then on the food level: a
        // fresh player carries `START_SATURATION` 5.0, so five drops (200
        // blocks) go on saturation before the sixth (40 more) takes the
        // haunch, and the drain is `> 4.0` rather than `>= 4.0` so the block
        // that pushes it exactly to 4.0 does not spend it.
        //
        // The figure matters because it is what a corpus scenario would have
        // to drive, and 241 packets is a different proposition from 40.
        let mut food = crate::food::FoodData::new();
        let mut blocks = 0_i32;
        while food.food_level() == 20 {
            let charge = check_movement_statistics(
                1.0,
                0.0,
                0.0,
                MoveContext {
                    sprinting: true,
                    ..ground()
                },
            );
            cause_food_exhaustion(&mut food, false, charge.exhaustion.unwrap());
            food.tick(crate::food::TickInputs {
                // NORMAL, not the substrate's PEACEFUL: the food-level
                // decrement at `FoodData.tick` offsets 10..78 is gated on
                // `difficulty != PEACEFUL`, so on the corpus substrate this
                // loop would never terminate. That gate is the real one —
                // the starve arm below it has no difficulty test of its own.
                difficulty: crate::food::Difficulty::Normal,
                natural_regeneration: false,
                is_hurt: false,
                health: 20.0,
            });
            blocks += 1;
            assert!(blocks < 10_000, "the food level never moved");
        }
        assert_eq!(blocks, 241);

        // The positive control on the saturation half: the same loop from
        // zero saturation takes the 41 the naive arithmetic predicts.
        let mut food = crate::food::FoodData::new();
        food.set_saturation(0.0);
        let mut blocks = 0_i32;
        while food.food_level() == 20 {
            cause_food_exhaustion(&mut food, false, 0.1);
            food.tick(crate::food::TickInputs {
                difficulty: crate::food::Difficulty::Normal,
                natural_regeneration: false,
                is_hurt: false,
                health: 20.0,
            });
            blocks += 1;
            assert!(blocks < 10_000, "the food level never moved");
        }
        assert_eq!(
            blocks, 41,
            "4.0 exhaustion at 0.1 a block, plus the one that crosses it"
        );
    }

    #[test]
    fn on_peaceful_no_amount_of_sprinting_moves_the_food_bar() {
        // `FoodData.tick` offsets 10..78: the exhaustion is still spent and
        // the saturation still falls, but `difficulty != PEACEFUL` guards the
        // food-level decrement. Both corpus substrates are peaceful, so this
        // is the reason no scenario on either of them can grade hunger — a
        // sharper reason than "it takes too many packets".
        let mut food = crate::food::FoodData::new();
        for _ in 0..5_000 {
            cause_food_exhaustion(&mut food, false, 0.1);
            food.tick(crate::food::TickInputs {
                difficulty: crate::food::Difficulty::Peaceful,
                natural_regeneration: false,
                is_hurt: false,
                health: 20.0,
            });
        }
        assert_eq!(food.food_level(), 20, "peaceful never takes the haunch");
        assert!(
            (food.saturation_level() - 0.0).abs() < f32::EPSILON,
            "but the saturation did drain, so the drain itself ran"
        );
    }
}
