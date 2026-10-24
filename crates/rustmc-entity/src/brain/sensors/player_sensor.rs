//! `net.minecraft.world.entity.ai.sensing.PlayerSensor`, from the 26.2 jar.
//! `Sensor()` (scan rate 20); `requires()` = `{NEAREST_PLAYERS,
//! NEAREST_VISIBLE_PLAYER, NEAREST_VISIBLE_ATTACKABLE_PLAYER,
//! NEAREST_VISIBLE_ATTACKABLE_PLAYERS}`.
//!
//! ```text
//! doTick (0..208):
//!   players = level.players().stream().filter(NO_SPECTATORS)
//!                 .filter(p -> p.closerThan(mob, getFollowDistance(mob)))     <- p.distanceToSqr(mob) < d*d, d = FOLLOW_RANGE
//!                 .sorted(comparingDouble(mob::distanceToSqr)).toList()     <- stable; ServerLevel.players() order breaks ties
//!   brain.setMemory(NEAREST_PLAYERS, players)                                <- empty CLEARS
//!   visible = players.filter(p -> Sensor.isEntityTargetable(level, mob, p))
//!   brain.setMemory(NEAREST_VISIBLE_PLAYER, visible.isEmpty() ? null : visible.get(0))
//!   attackable = visible.filter(p -> Sensor.isEntityAttackable(level, mob, p))
//!   brain.setMemory(NEAREST_VISIBLE_ATTACKABLE_PLAYERS, attackable)
//!   brain.setMemory(NEAREST_VISIBLE_ATTACKABLE_PLAYER, attackable.isEmpty() ? null : attackable.get(0))
//! ```
//!
//! `isEntityTargetable` / `isEntityAttackable` run under the six shared
//! conditions, whose range this sensor's own `tick` set to the mob's
//! follow range just before `doTick` (`sensor.rs`).

use crate::ai::mob_ai::MobAi;
use crate::brain::memory::{MemoryModuleType, MemoryValue};
use crate::brain::sensor::{
    is_entity_attackable, is_entity_targetable, Sensor, SensorBody, SensorType,
};
use crate::brain::BrainState;
use crate::host::MobHost;

/// `PlayerSensor`.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlayerSensor;

impl PlayerSensor {
    /// `SensorType.NEAREST_PLAYERS.create()`.
    #[must_use]
    pub fn new() -> Sensor<PlayerSensor> {
        Sensor::new(PlayerSensor)
    }

    /// `getFollowDistance(mob)`: `FOLLOW_RANGE`.
    fn follow_distance<M: MobHost>(mob: &M) -> f64 {
        mob.follow_range()
    }
}

impl<M: MobHost> SensorBody<M> for PlayerSensor {
    fn do_tick(&mut self, brain: &mut BrainState, cx: &mut MobAi<M>) {
        let d = Self::follow_distance(&cx.mob);
        let mut players: Vec<(i32, f64)> = cx
            .mob
            .players()
            .into_iter()
            .filter_map(|id| {
                let f = cx.mob.entity(id)?;
                if f.is_spectator {
                    return None;
                }
                let sq = cx.mob.distance_to_sqr_entity(id)?;
                (sq < d * d).then_some((id, sq))
            })
            .collect();
        players.sort_by(|a, b| a.1.total_cmp(&b.1));
        let players: Vec<i32> = players.into_iter().map(|(id, _)| id).collect();
        brain.set_memory_value(
            MemoryModuleType::NearestPlayers,
            MemoryValue::Entities(players.clone()),
        );
        let visible: Vec<i32> = players
            .iter()
            .copied()
            .filter(|id| {
                cx.mob
                    .entity(*id)
                    .is_some_and(|e| is_entity_targetable(brain, cx, &e))
            })
            .collect();
        brain.set_memory(
            MemoryModuleType::NearestVisiblePlayer,
            visible.first().map(|id| MemoryValue::Entity(*id)),
        );
        let attackable: Vec<i32> = visible
            .iter()
            .copied()
            .filter(|id| {
                cx.mob
                    .entity(*id)
                    .is_some_and(|e| is_entity_attackable(brain, cx, &e))
            })
            .collect();
        brain.set_memory_value(
            MemoryModuleType::NearestVisibleAttackablePlayers,
            MemoryValue::Entities(attackable.clone()),
        );
        brain.set_memory(
            MemoryModuleType::NearestVisibleAttackablePlayer,
            attackable.first().map(|id| MemoryValue::Entity(*id)),
        );
    }

    fn requires(&self) -> Vec<MemoryModuleType> {
        vec![
            MemoryModuleType::NearestPlayers,
            MemoryModuleType::NearestVisiblePlayer,
            MemoryModuleType::NearestVisibleAttackablePlayer,
            MemoryModuleType::NearestVisibleAttackablePlayers,
        ]
    }

    fn sensor_type(&self) -> SensorType {
        SensorType::NearestPlayers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::memory::MemoryModuleType as T;
    use crate::brain::sensor::update_targeting_condition_ranges;
    use crate::brain::Brain;
    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    #[test]
    fn nearest_players_are_sorted_and_the_visible_ones_need_line_of_sight() {
        let mut cx = MobAi::new(TestMob::new(GridLevel::new(), 0.5, 1.0, 0.5, 1));
        cx.mob.follow_range = 16.0;
        cx.mob.others = vec![
            (EntityClass::Player, EntityFacts::player(11, 6.5, 1.0, 0.5)),
            (EntityClass::Player, EntityFacts::player(12, 3.5, 1.0, 0.5)),
            (EntityClass::Player, EntityFacts::player(13, 30.5, 1.0, 0.5)),
        ];
        cx.mob.visible = vec![11];
        update_targeting_condition_ranges(16.0);
        let mut brain = Brain::<TestMob>::new(
            &[
                T::NearestPlayers,
                T::NearestVisiblePlayer,
                T::NearestVisibleAttackablePlayer,
                T::NearestVisibleAttackablePlayers,
            ],
            vec![],
        )
        .state;
        let mut s = PlayerSensor;
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert_eq!(
            brain.get_memory(T::NearestPlayers),
            Some(&MemoryValue::Entities(vec![12, 11])),
            "13 is beyond 16 blocks"
        );
        assert_eq!(
            brain.get_memory(T::NearestVisiblePlayer),
            Some(&MemoryValue::Entity(11)),
            "12 has no line of sight"
        );
        assert_eq!(
            brain.get_memory(T::NearestVisibleAttackablePlayers),
            Some(&MemoryValue::Entities(vec![11]))
        );
        assert_eq!(
            brain.get_memory(T::NearestVisibleAttackablePlayer),
            Some(&MemoryValue::Entity(11))
        );
        cx.mob.others.clear();
        SensorBody::<TestMob>::do_tick(&mut s, &mut brain, &mut cx);
        assert!(!brain.has_memory_value(T::NearestPlayers));
        assert!(!brain.has_memory_value(T::NearestVisiblePlayer));
        assert!(!brain.has_memory_value(T::NearestVisibleAttackablePlayers));
        assert!(!brain.has_memory_value(T::NearestVisibleAttackablePlayer));
    }
}
