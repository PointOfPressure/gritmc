//! `AbstractArrow.playerTouch` and `tryPickup` — how a stuck arrow is
//! recovered.
//!
//! ```text
//!   playerTouch(Player p), offsets 0-50:
//!       if (level.isClientSide) return;
//!       if (!(isInGround() || isNoPhysics())) return;
//!       if (shakeTime > 0) return;
//!       if (tryPickup(p)) { p.take(this, 1); discard(); }
//!
//!   tryPickup(Player p), a tableswitch on Pickup.ordinal():
//!       DISALLOWED     -> false
//!       ALLOWED        -> p.getInventory().add(getPickupItem())
//!       CREATIVE_ONLY  -> p.hasInfiniteMaterials()
//! ```
//!
//! # The three things a reading gets wrong
//!
//! **`isInGround() || isNoPhysics()`, not `isInGround()` alone.** Offsets
//! 10-21 are `isInGround(); ifne 24` then `isNoPhysics(); ifeq 31`, so a
//! no-physics arrow still in flight is pickable. That is the branch a
//! spectator-mode or `/data`-edited arrow takes and it is not dead.
//!
//! **`shakeTime > 0` is a refusal, and `shakeTime` is 7 on landing.** So an
//! arrow cannot be picked up for the seven ticks after it sticks, however
//! close the player is standing. A port that dropped the guard hands the
//! arrow back a third of a second early, which is exactly long enough to see.
//!
//! **`ALLOWED` calls `Inventory.add` and takes its answer**, so a full
//! inventory means `tryPickup` is false and the arrow is **not** consumed —
//! it stays in the world. `CREATIVE_ONLY` never touches the inventory at all:
//! it returns `hasInfiniteMaterials()`, so a creative player's `take`
//! animation plays and nothing is added, and a survival player gets nothing
//! and the arrow stays.
//!
//! # What is refused
//!
//! `Player.getInventory().add(ItemStack)` is the inventory lane's, and
//! `Player.take(Entity, int)` is a `ClientboundTakeItemEntityPacket` the
//! tracker owns. [`Outcome`] reports which of the two happened and this
//! module performs neither.

use crate::projectile::arrow::{Arrow, Pickup};

/// What `playerTouch` decided, without performing any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The guards refused before `tryPickup` was reached: the arrow is
    /// still flying, or its `shakeTime` has not run out.
    NotYet,
    /// `tryPickup` returned false. **The arrow stays in the world** — a full
    /// inventory and a survival player meeting a creative-only arrow both
    /// land here.
    Refused,
    /// `tryPickup` returned true: `player.take(this, 1)` then `discard()`.
    Taken,
}

/// Everything about the player that `playerTouch` and `tryPickup` read.
///
/// Two booleans rather than a player handle, because the player lanes own the
/// player and this module needs no more than these: `Inventory.add`'s answer
/// and `hasInfiniteMaterials()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Toucher {
    /// `player.getInventory().add(getPickupItem())` — **its return value**,
    /// which is false when nothing fitted.
    pub inventory_accepted: bool,
    /// `player.hasInfiniteMaterials()`.
    pub has_infinite_materials: bool,
}

/// `AbstractArrow.tryPickup(Player)`.
#[must_use]
pub const fn try_pickup(arrow: &Arrow, p: Toucher) -> bool {
    match arrow.pickup {
        Pickup::Disallowed => false,
        Pickup::Allowed => p.inventory_accepted,
        Pickup::CreativeOnly => p.has_infinite_materials,
    }
}

/// `AbstractArrow.playerTouch(Player)`, server side.
///
/// Does not mutate the arrow: `Outcome::Taken` is the caller's instruction to
/// send `take` and discard, because both of those are somebody else's.
#[must_use]
pub fn player_touch(arrow: &Arrow, p: Toucher) -> Outcome {
    if !(arrow.in_ground || arrow.no_physics) {
        return Outcome::NotYet;
    }
    if arrow.shake_time > 0 {
        return Outcome::NotYet;
    }
    if try_pickup(arrow, p) {
        Outcome::Taken
    } else {
        Outcome::Refused
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collide::Vec3;
    use crate::projectile::arrow::SHAKE_TIME;

    fn stuck(pickup: Pickup) -> Arrow {
        let mut a = Arrow::at(Vec3::new(0.0, 64.0, 0.0));
        a.in_ground = true;
        a.shake_time = 0;
        a.pickup = pickup;
        a
    }

    fn room() -> Toucher {
        Toucher {
            inventory_accepted: true,
            has_infinite_materials: false,
        }
    }

    #[test]
    fn a_stuck_allowed_arrow_is_taken() {
        assert_eq!(
            player_touch(&stuck(Pickup::Allowed), room()),
            Outcome::Taken
        );
    }

    #[test]
    fn the_shake_refuses_for_seven_ticks_after_landing() {
        let mut a = stuck(Pickup::Allowed);
        a.shake_time = SHAKE_TIME;
        for _ in 0..SHAKE_TIME {
            assert_eq!(
                player_touch(&a, room()),
                Outcome::NotYet,
                "{}",
                a.shake_time
            );
            a.shake_time -= 1;
        }
        assert_eq!(a.shake_time, 0);
        assert_eq!(player_touch(&a, room()), Outcome::Taken);
    }

    #[test]
    fn an_arrow_still_in_flight_is_not_pickable_unless_it_has_no_physics() {
        let mut a = stuck(Pickup::Allowed);
        a.in_ground = false;
        assert_eq!(player_touch(&a, room()), Outcome::NotYet);
        // The second disjunct, which a reading of `isInGround()` alone loses.
        a.no_physics = true;
        assert_eq!(player_touch(&a, room()), Outcome::Taken);
    }

    #[test]
    fn a_full_inventory_leaves_the_arrow_in_the_world() {
        let full = Toucher {
            inventory_accepted: false,
            has_infinite_materials: false,
        };
        assert_eq!(
            player_touch(&stuck(Pickup::Allowed), full),
            Outcome::Refused
        );
        // Positive control: the same arrow and a player with room.
        assert_eq!(
            player_touch(&stuck(Pickup::Allowed), room()),
            Outcome::Taken
        );
    }

    #[test]
    fn creative_only_reads_the_flag_and_never_the_inventory() {
        let survival_with_room = room();
        assert_eq!(
            player_touch(&stuck(Pickup::CreativeOnly), survival_with_room),
            Outcome::Refused,
            "a survival player gets nothing however much room they have"
        );
        let creative_and_full = Toucher {
            inventory_accepted: false,
            has_infinite_materials: true,
        };
        assert_eq!(
            player_touch(&stuck(Pickup::CreativeOnly), creative_and_full),
            Outcome::Taken,
            "and a creative player is taken even with a full inventory"
        );
    }

    #[test]
    fn disallowed_refuses_every_player() {
        for p in [
            room(),
            Toucher {
                inventory_accepted: false,
                has_infinite_materials: false,
            },
            Toucher {
                inventory_accepted: true,
                has_infinite_materials: true,
            },
        ] {
            assert_eq!(
                player_touch(&stuck(Pickup::Disallowed), p),
                Outcome::Refused
            );
        }
    }

    #[test]
    fn the_default_pickup_mode_is_disallowed() {
        // `AbstractArrow`'s field initialiser is `Pickup.DISALLOWED`, so an
        // arrow this server creates without saying otherwise cannot be
        // recovered — which is what a dispenser-fired or `/summon`ed one does.
        let a = Arrow::at(Vec3::new(0.0, 64.0, 0.0));
        assert_eq!(a.pickup, Pickup::Disallowed);
    }
}
