//! `CombatTracker`, as far as a player's wire can see it: the two combat
//! packets `ServerPlayer.onEnterCombat` and `onLeaveCombat` send.
//!
//! `javap -p -c -constants` on the pinned 26.2 jar:
//!
//! ```text
//! LivingEntity.actuallyHurt 111-124   if (f == 0.0f) return; combatTracker.recordDamage(source, f)
//!                                       -- f the damage after armour, magic and absorption
//! CombatTracker.recordDamage 0-115    recheckStatus(); entries.add(..); lastDamageTime = tickCount;
//!                                     takingDamage = true;
//!                                     if (!inCombat && mob.isAlive() && shouldEnterCombat(source))
//!                                        { inCombat = true; combatStart = combatEnd = tickCount;
//!                                          mob.onEnterCombat(); }
//!   shouldEnterCombat                 source.getEntity() instanceof LivingEntity
//! CombatTracker.recheckStatus 0-95    limit = inCombat ? 300 : 100
//!                                     if (takingDamage && (!mob.isAlive() || tickCount - lastDamageTime > limit))
//!                                        { was = inCombat; takingDamage = inCombat = false;
//!                                          combatEnd = tickCount; if (was) mob.onLeaveCombat();
//!                                          entries.clear(); }
//! LivingEntity.tick 129-142           if (tickCount % 20 == 0) combatTracker.recheckStatus()
//! getCombatDuration                   inCombat ? tickCount - combatStart : combatEnd - combatStart
//! ServerPlayer.onEnterCombat          connection.send(ClientboundPlayerCombatEnterPacket.INSTANCE)
//! ServerPlayer.onLeaveCombat          connection.send(new ClientboundPlayerCombatEndPacket(tracker))
//!                                       -- the packet carries getCombatDuration(), a varint
//! ```
//!
//! The entries themselves (the death message's "was slain by" history) are
//! not kept: nothing here builds a combat death message from them.

/// What one call asks the connection to send, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatPacket {
    /// `player_combat_enter`, no fields.
    Enter,
    /// `player_combat_end`, with the combat's duration in ticks.
    End { duration: i32 },
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CombatTracker {
    in_combat: bool,
    taking_damage: bool,
    last_damage_time: i32,
    combat_start: i32,
    combat_end: i32,
}

impl CombatTracker {
    /// `recheckStatus()`.
    pub fn recheck(&mut self, tick_count: i32, alive: bool) -> Option<CombatPacket> {
        let limit = if self.in_combat { 300 } else { 100 };
        if self.taking_damage && (!alive || tick_count - self.last_damage_time > limit) {
            let was = self.in_combat;
            self.taking_damage = false;
            self.in_combat = false;
            self.combat_end = tick_count;
            if was {
                return Some(CombatPacket::End {
                    duration: self.combat_end - self.combat_start,
                });
            }
        }
        None
    }

    /// `recordDamage(source, damage)`, for a damage `actuallyHurt` did not
    /// return early on. `living_source` is `source.getEntity() instanceof
    /// LivingEntity`.
    pub fn record_damage(
        &mut self,
        tick_count: i32,
        alive: bool,
        living_source: bool,
    ) -> Vec<CombatPacket> {
        let mut out: Vec<CombatPacket> = self.recheck(tick_count, alive).into_iter().collect();
        self.last_damage_time = tick_count;
        self.taking_damage = true;
        if !self.in_combat && alive && living_source {
            self.in_combat = true;
            self.combat_start = tick_count;
            self.combat_end = tick_count;
            out.push(CombatPacket::Enter);
        }
        out
    }
}

/// `ClientboundPlayerCombatEndPacket`'s body: the duration, a varint.
///
/// # Errors
/// Only if the buffer write fails, which for a `Vec` it cannot.
pub fn combat_end_body(duration: i32) -> std::io::Result<Vec<u8>> {
    let mut b = Vec::with_capacity(5);
    rustmc_proto::varint::write_varint(&mut b, duration)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::{CombatPacket, CombatTracker};

    #[test]
    fn a_hit_by_a_living_source_enters_combat_once_and_a_quiet_300_ticks_leaves_it() {
        let mut c = CombatTracker::default();
        assert_eq!(c.record_damage(10, true, true), vec![CombatPacket::Enter]);
        assert_eq!(c.record_damage(20, true, true), vec![]);
        // `> 300`, not `>=`: 320 after the last hit is not yet out.
        assert_eq!(c.recheck(320, true), None);
        assert_eq!(
            c.recheck(321, true),
            Some(CombatPacket::End { duration: 311 })
        );
        assert_eq!(c.recheck(400, true), None);
    }

    #[test]
    fn a_hit_with_no_living_source_takes_damage_without_combat() {
        let mut c = CombatTracker::default();
        assert_eq!(c.record_damage(10, true, false), vec![]);
        // Taking damage ends quietly after 100: it was never combat.
        assert_eq!(c.recheck(111, true), None);
        assert_eq!(c.record_damage(200, true, true), vec![CombatPacket::Enter]);
    }
}
