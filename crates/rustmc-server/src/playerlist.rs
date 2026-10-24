//! `PlayerList.broadcast(Player, double, double, double, double, ResourceKey,
//! Packet)` — the positional broadcast, which is a different method from the
//! one [`crate::players::Players::broadcast`] models.
//!
//! # Two broadcasts, one name, and they were being conflated
//!
//! `Players::broadcast` and `broadcast_except` reproduce
//! `PlayerList.broadcastAll` and the two-argument exclusion: everybody, or
//! everybody but one. Vanilla has a **third** overload that takes a point and
//! a radius, and three channels reach it:
//!
//! ```text
//!   ServerLevel.playSeededSound   x, y, z, sound.getRange(volume)
//!   ServerLevel.levelEvent        pos.getX/Y/Z, 64.0        <- a constant
//!   ServerLevel.sendParticles     per-player, a different rule again
//! ```
//!
//! The first two share this function. The third does **not** and must not be
//! made to. `ServerLevel.sendParticles` walks `this.players` itself and calls
//! `sendParticles(ServerPlayer, …)`, whose test is
//! `player.blockPosition().closerToCenterThan(vec, longDistance ? 512 : 32)`.
//! Three differences, all of them checked in the jar rather than assumed:
//!
//! * the radius is **32**, or 512 when the caller sets `longDistance`, not
//!   the sound rule's 16 and not the level event's 64;
//! * the distance is measured from the player's **block centre** —
//!   `Vec3i.distToCenterSqr` is `getX() + 0.5` per axis — and not from the
//!   player's exact position, which is what `PlayerList.broadcast` uses.
//!   A player standing at x = 3.9 is measured at 3.5 by one rule and 3.9 by
//!   the other;
//! * a dimension mismatch returns `false` from the per-player call rather
//!   than being filtered in a loop, so `sendParticles` also *counts* how many
//!   players it reached, which nothing here does.
//!
//! Only the comparison is shared: `dcmpg; ifge` in `closerToCenterThan` too,
//! so that boundary is exclusive as well. Folding particles in here would be
//! a plausible-looking simplification that gets two of those three wrong.
//!
//! # The loop, from offsets 0-129
//!
//! ```text
//!   for (i = 0; i < players.size(); i++) {
//!       ServerPlayer p = players.get(i);
//!       if (p == except) continue;                       // 33-39
//!       if (p.level().dimension() != dimension) continue; // 44-55
//!       double dx = x - p.getX(), dy = ..., dz = ...;     // 58-85
//!       if (dx*dx + dy*dy + dz*dz < range*range)          // 87-110
//!           p.connection.send(packet);
//!   }
//! ```
//!
//! `dcmpg; ifge` is the comparison, so the boundary is **exclusive**: a player
//! standing at exactly `range` is not sent the packet.
//!
//! The dimension test is not reproduced. This server has one dimension; when
//! it has two, that arm goes in here and **before** the distance, because a
//! nether coordinate is not an overworld one and the squared distance between
//! them is a number with no meaning.
//!
//! # An unlocatable listener is sent the packet
//!
//! A connection whose entity is not in [`crate::entities::Entities`] has no
//! position, and there is no honest distance for it. Vanilla has no such
//! state: `ServerPlayer.getX()` always answers. Two things produce one here —
//! the window between `players.join` and the `entities.insert` a few lines
//! later, and every unit test that builds connections and no entities — and
//! in both cases silencing them would make a missing entity look exactly like
//! the feature not existing, which is the one failure mode nothing downstream
//! can tell apart from success.

use crate::entities::Entities;
use crate::players::Connection;
use std::sync::Arc;

/// `ServerLevel.levelEvent`'s broadcast radius, `ldc2_w double 64.0d` at its
/// offset 41.
///
/// A constant in the bytecode and not a property of the event: a composter
/// filling and a block breaking carry exactly as far as each other, which is
/// why this is one number rather than a column in `level_events.json`.
pub const LEVEL_EVENT_RANGE: f64 = 64.0;

/// Whether one listener is inside the radius, given where they are.
///
/// `None` for a listener whose position is unknown, which this returns `true`
/// for — see the module header.
#[must_use]
pub fn within(pos: (f64, f64, f64), listener: Option<(f64, f64, f64)>, range: f64) -> bool {
    let Some(l) = listener else {
        return true;
    };
    let (dx, dy, dz) = (pos.0 - l.0, pos.1 - l.1, pos.2 - l.2);
    // Each square on its own line, and that is not style. `dmul; dmul; dadd;
    // dmul; dadd` at offsets 87-103 is three multiplies and two adds, five
    // roundings; `dx.mul_add(dx, ...)` is a fused multiply-add and rounds
    // once. The two differ in the last bit, and this comparison decides who
    // receives a packet — so the expression is written in a shape nothing is
    // tempted to fuse.
    let dx2 = dx * dx;
    let dy2 = dy * dy;
    let dz2 = dz * dz;
    dx2 + dy2 + dz2 < range * range
}

/// Send one packet to every connection inside `range` of `pos`, except one.
///
/// `conns` is a snapshot the caller already took, for the reason
/// [`crate::players::Players::connections`] gives: two walks of the list under
/// separate locks can disagree about who is connected, and a caller sending
/// several packets must send all of them to one audience.
///
/// `locate` answers "where is the player who is entity `id`". It is a closure
/// rather than an `&Entities` because the two callers already hold different
/// things: `crate::containers::run_block_events` is handed exactly this
/// closure by the tick loop, and everything else has the registry. Both go
/// through [`entity_locator`] or their own equivalent, and a locator that
/// answers `None` means "unlocatable" — see the module header.
pub fn broadcast_within(
    conns: &[Arc<Connection>],
    locate: &dyn Fn(i32) -> Option<[f64; 3]>,
    except: Option<u64>,
    pos: (f64, f64, f64),
    range: f64,
    packet: i32,
    body: &[u8],
) {
    for c in conns {
        if except == Some(c.id()) {
            continue;
        }
        let at = locate(c.entity_id()).map(|p| (p[0], p[1], p[2]));
        if !within(pos, at, range) {
            continue;
        }
        let _ = c.send(packet, body);
    }
}

/// `ServerLevel.players()`' positions, as a locator.
///
/// `None` for the registry itself being absent, which is what a unit test
/// that builds connections and no world has: every listener is then
/// unlocatable and hears everything, which is what this function did before
/// any radius existed.
// No `#[must_use]`: the returned closure is `impl Fn`, which is already
// `#[must_use]`, and clippy refuses the redundant attribute.
pub fn entity_locator(entities: Option<&Entities>) -> impl Fn(i32) -> Option<[f64; 3]> + '_ {
    move |id| entities.and_then(|es| es.get(id)).map(|e| [e.x, e.y, e.z])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dcmpg; ifge` — strictly less than. A `<=` here passes every other
    /// case in this file and fails only this one.
    #[test]
    fn the_boundary_is_exclusive() {
        let origin = (0.0, 0.0, 0.0);
        assert!(within(origin, Some((15.999, 0.0, 0.0)), 16.0));
        assert!(!within(origin, Some((16.0, 0.0, 0.0)), 16.0));
        assert!(!within(origin, Some((16.001, 0.0, 0.0)), 16.0));
    }

    /// All three axes are squared and summed, which a version that compared
    /// per-axis would get right on the tests above and wrong here: this
    /// listener is 9.24 blocks out along each axis and 16.0 away.
    #[test]
    fn the_distance_is_euclidean_and_not_a_box() {
        let d = 16.0 / 3.0_f64.sqrt();
        assert!(!within((0.0, 0.0, 0.0), Some((d, d, d)), 16.0));
        assert!(within((0.0, 0.0, 0.0), Some((d, d, d)), 16.001));
        // A per-axis box test would call this one in, because each component
        // is under sixteen.
        assert!(!within((0.0, 0.0, 0.0), Some((10.0, 10.0, 10.0)), 16.0));
    }

    /// y counts. A radius test written in two dimensions — which is what a
    /// chunk-distance habit produces — passes everything above.
    #[test]
    fn height_counts_toward_the_distance() {
        assert!(!within((0.0, 0.0, 0.0), Some((0.0, 20.0, 0.0)), 16.0));
        assert!(within((0.0, 0.0, 0.0), Some((0.0, 20.0, 0.0)), 21.0));
    }

    /// The deliberate asymmetry, stated as a test so that changing it has to
    /// be a decision rather than a tidy-up.
    #[test]
    fn an_unlocatable_listener_is_inside_every_radius() {
        assert!(within((0.0, 0.0, 0.0), None, 16.0));
        assert!(within((1e9, 0.0, 0.0), None, 0.5));
    }
}
