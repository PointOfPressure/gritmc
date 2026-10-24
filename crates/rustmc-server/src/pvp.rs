//! `Player.attack` against another player: the attacker's half on the
//! attacker's connection, the target's half on the target's.
//!
//! Transcribed from the pinned 26.2 jar, `javap -p -c -constants` on
//! `Player.attack`, `Player.causeExtraKnockback`, `Player.attackVisualEffects`,
//! `Player.damageStatsAndHearts`, `ServerPlayer.swing`, `Player.tick`.
//! The arithmetic is [`crate::attack::melee`] (mob-combat's port).
//!
//! # Why the hit crosses threads, and what that costs nothing
//!
//! Vanilla hurts the target **inside** `attack`: `hurtOrSimulate` at offset
//! 242 runs the target's whole `hurtServer`, so the target's `damage_event`
//! and hurt sound leave before the attacker's `player.attack.*` sound. Here a
//! player's health lives on its own connection's loop
//! ([`crate::playertick::LivePlayer`]), so the attacker's handler computes
//! everything the attacker owns — the reach ladder, the strength scale off
//! its own ticker, `onAttack()`'s reset — and hands the rest of the method to
//! the target as one [`PendingAttack`], which the target's loop runs start to
//! finish in `attack`'s own order: the sprint-knockback sound, the hurt, the
//! extra knockback and the target's own velocity packet, the remaining
//! attacker sounds, and the attacker's food exhaustion sent back.
//!
//! **The timing is vanilla's, not an approximation of it.** A packet read
//! while the clock reads `T` is handled by vanilla at the start of `T + 1`,
//! before any player's `doTick` — so the target's `set_health` goes out in
//! that same tick's `doTick`. The pending attack is due at `T + 1` and runs
//! before the target's `doTick` for it (unlike a console `/damage`, which
//! vanilla runs after every `doTick`).
//!
//! # Not ported, by name
//!
//! Sprinting (no `ServerboundPlayerCommand` handling exists on this base, so
//! `isSprinting()` is false and the sprint-knockback arm is unreachable from
//! a packet); the sweep (`doSweepAttack` needs a sword, and no player here
//! holds an item with attributes); `DAMAGE_INDICATOR` particles (the
//! `damageStatsAndHearts` arm needs more than 2.0 health taken, which a bare
//! hand's best crit, 1.5, cannot reach); enchantment hooks (`getEnchantedDamage`
//! and `itemAttackInteraction` are identities for an item-less hand).

use crate::attack::Melee;

/// `ServerPlayer.swing` → `resetAttackStrengthTicker()`, and `Player.tick`'s
/// counter — the attack strength clock of one player.
///
/// ```text
///   Player.tick 230-235         attackStrengthTicker++
///               255-288         !ItemStack.matches(lastItemInMainHand, main)
///                                  -> !isSameItem -> resetAttackStrengthTicker()
///                                  lastItemInMainHand = main.copy()
///   ServerPlayer.swing 0-6      super.swing(hand); resetAttackStrengthTicker()
///   Player.onAttack 0-1         resetOnlyAttackStrengthTicker()
/// ```
///
/// This server's hotbar carries item names and no counts or components, so
/// `matches` and `isSameItem` both reduce to name equality.
///
/// # The clock is the game's, not the loop's
///
/// A packet this connection reads while the game clock reads `T` is, in
/// vanilla, handled at the start of `T + 1` — after tick `T`'s
/// `Player.tick`. This server's loop can read a packet before it has
/// simulated tick `T` for this player (the clock moved while the read was
/// blocked), and then the swing reset lands before tick `T`'s increment
/// instead of after it. Measured on the first survival pass of this branch
/// (`~/rustmc-entpvp`, 2026-09-30): 001's hit took 0.272 (ticker 1) where
/// vanilla's took 0.208 (ticker 0), and one of 005's two hits did the same.
/// So the ticker advances to a game time, idempotently: the tick loop
/// advances it to the tick it simulates, and the swing and the attack
/// advance it to the clock first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strength {
    pub ticker: i32,
    last_main_hand: Option<String>,
    /// The last game tick whose `Player.tick` this has run.
    last_tick: Option<i64>,
}

impl Strength {
    /// `Player.tick`'s two statements, for every tick up to `tick` not yet
    /// run, in their order.
    pub fn advance_to(&mut self, tick: i64, main_hand: Option<&str>) {
        let from = self.last_tick.map_or(tick, |t| t + 1);
        for _ in from..=tick {
            self.ticker += 1;
            if self.last_main_hand.as_deref() != main_hand {
                self.ticker = 0;
                self.last_main_hand = main_hand.map(str::to_string);
            }
        }
        self.last_tick = Some(self.last_tick.map_or(tick, |t| t.max(tick)));
    }

    /// `resetAttackStrengthTicker()` / `resetOnlyAttackStrengthTicker()` —
    /// the first also zeroes `itemSwapTicker`, which nothing here reads.
    pub fn reset(&mut self) {
        self.ticker = 0;
    }
}

/// The rest of `Player.attack`, from `hurtOrSimulate` on, for a target on
/// another connection.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingAttack {
    /// The attacker's entity and connection.
    pub attacker: i32,
    pub attacker_conn: u64,
    /// The attacker's feet, where `playServerSideSound` plays.
    pub attacker_pos: (f64, f64, f64),
    /// The attacker's `getYRot()`, which `causeExtraKnockback` aims by.
    pub attacker_y_rot: f32,
    /// [`crate::attack::melee`]'s plan.
    pub plan: Melee,
    /// The game time the packet was read at, plus one — see the module docs.
    pub at_tick: i64,
}

fn attacks() -> &'static std::sync::Mutex<Vec<(u64, PendingAttack)>> {
    static Q: std::sync::OnceLock<std::sync::Mutex<Vec<(u64, PendingAttack)>>> =
        std::sync::OnceLock::new();
    Q.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// Queue an attack for the target's connection.
pub fn submit(target_conn: u64, attack: PendingAttack) {
    attacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((target_conn, attack));
}

/// Every attack on `conn` due by tick `tick`, in the order they were read.
#[must_use]
pub fn take(conn: u64, tick: i64) -> Vec<PendingAttack> {
    let mut q = attacks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = Vec::new();
    q.retain(|(c, a)| {
        if *c == conn && a.at_tick <= tick {
            out.push(a.clone());
            false
        } else {
            true
        }
    });
    out
}

fn exhaustion() -> &'static std::sync::Mutex<Vec<(u64, f32)>> {
    static Q: std::sync::OnceLock<std::sync::Mutex<Vec<(u64, f32)>>> = std::sync::OnceLock::new();
    Q.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// `causeFoodExhaustion(0.1f)` at `attack` offset 335, owed by the attacker
/// and charged on the attacker's own loop.
pub fn owe_exhaustion(attacker_conn: u64, amount: f32) {
    exhaustion()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((attacker_conn, amount));
}

/// The exhaustion owed to `conn`, drained.
#[must_use]
pub fn take_exhaustion(conn: u64) -> Vec<f32> {
    let mut q = exhaustion()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut out = Vec::new();
    q.retain(|(c, a)| {
        if *c == conn {
            out.push(*a);
            false
        } else {
            true
        }
    });
    out
}

/// The attacker sounds `attack` plays before `hurtOrSimulate` and after it,
/// split where [`Melee::knockback_sprint`] says: the sprint-knockback sound
/// is the only one before.
#[must_use]
pub fn split_sounds(plan: &Melee) -> (Vec<&'static str>, Vec<&'static str>) {
    let n = usize::from(plan.knockback_sprint);
    (plan.sounds[..n].to_vec(), plan.sounds[n..].to_vec())
}

/// `causeExtraKnockback`'s arguments to `LivingEntity.knockback`: the
/// strength halved, and the sine and negated cosine of the attacker's yaw
/// times `0.017453292f`. `None` when the strength is not strictly positive
/// (offsets 0-3).
#[must_use]
pub fn extra_knockback_args(plan: &Melee, attacker_y_rot: f32) -> Option<(f64, f64, f64)> {
    if plan.extra_knockback <= 0.0 {
        return None;
    }
    let r = attacker_y_rot * 0.017_453_292_f32;
    let s = rustmc_entity::mth::sin(f64::from(r));
    let c = rustmc_entity::mth::cos(f64::from(r));
    Some((
        f64::from(plan.extra_knockback * 0.5),
        f64::from(s),
        f64::from(-c),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ServerPlayer.swing` then the attack in the same tick is what the
    /// harness does, and it is why vanilla's recorded hit took 0.208: the
    /// ticker reads 0, the scale `(0 + 0.5) / 5 = 0.1`, the damage
    /// `1.0 * (0.2 + 0.1^2 * 0.8)`.
    #[test]
    fn a_swing_then_an_attack_in_one_tick_is_a_tenth_strength_hit() {
        let mut st = Strength::default();
        st.advance_to(40, None);
        // The swing is read while the clock still reads 40 and tick 40 has
        // already run: advancing to 40 again is a no-op, then the reset.
        st.advance_to(40, None);
        st.reset();
        let swing = crate::attack::Swing {
            attack_strength_ticker: st.ticker,
            attack_speed: crate::attack::PLAYER_ATTACK_SPEED,
            attack_damage: crate::attack::PLAYER_ATTACK_DAMAGE,
            attack_knockback: crate::attack::PLAYER_ATTACK_KNOCKBACK,
            item_damage_bonus: 0.0,
            sprinting: false,
            crit: crate::attack::CritConditions {
                fall_distance: 0.0,
                on_ground: true,
                on_climbable: false,
                in_water: false,
                mobility_restricted: false,
                passenger: false,
                target_is_living: true,
                sprinting: false,
            },
            sweep: crate::attack::SweepConditions {
                strong: false,
                crit: false,
                knockback_sprint: false,
                on_ground: true,
                known_movement_horizontal_sqr: 0.0,
                speed: 0.0,
                holding_sword: false,
            },
        };
        let plan = crate::attack::melee(&swing);
        assert!(
            (20.0 - plan.damage - 19.792).abs() < 1e-4,
            "{}",
            plan.damage
        );
        assert_eq!(
            split_sounds(&plan),
            (vec![], vec![crate::attack::ATTACK_WEAK_SOUND])
        );
        assert_eq!(extra_knockback_args(&plan, 0.0), None);
    }

    #[test]
    fn a_new_item_in_the_hand_restarts_the_clock_and_the_same_one_does_not() {
        let mut st = Strength::default();
        st.advance_to(1, None);
        st.advance_to(2, None);
        st.advance_to(3, None);
        assert_eq!(st.ticker, 3);
        st.advance_to(4, Some("minecraft:stick"));
        assert_eq!(st.ticker, 0);
        st.advance_to(5, Some("minecraft:stick"));
        assert_eq!(st.ticker, 1);
        // A tick already run does not run twice.
        st.advance_to(5, Some("minecraft:stick"));
        assert_eq!(st.ticker, 1);
    }
}
