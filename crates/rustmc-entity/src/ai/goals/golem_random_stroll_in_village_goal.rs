//! `net.minecraft.world.entity.ai.goal.GolemRandomStrollInVillageGoal`,
//! from the 26.2 jar: `RandomStrollGoal(mob, speed, 240, false)` with its
//! own `getPosition()`. `POI_SECTION_SCAN_RADIUS = 2`, `VILLAGER_SCAN_RADIUS
//! = 32`, `RANDOM_POS_XY_DISTANCE = 10`, `RANDOM_POS_Y_DISTANCE = 7`. Used
//! by IronGolem.
//!
//! # `getPosition()`, offsets 0..83 — the draws are the LEVEL's random
//!
//! ```text
//! r = level.getRandom()
//! if r.nextFloat() < 0.3f: return getPositionTowardsAnywhere()
//! if r.nextFloat() < 0.7f: v = towardsVillagerWhoWantsGolem(); if null v = towardsPoi()
//! else:                    v = towardsPoi(); if null v = towardsVillagerWhoWantsGolem()
//! return v != null ? v : getPositionTowardsAnywhere()
//! ```
//!
//! `getPositionTowardsAnywhere()`: `LandRandomPos.getPos(mob, 10, 7)`.
//! `getPositionTowardsVillagerWhoWantsGolem()`: the villagers in
//! `bb.inflate(32)` wanting a golem; none is `null`, else
//! `LandRandomPos.getPosTowards(mob, 10, 7, list.get(level.random.nextInt(size)).position())`.
//! `getPositionTowardsPoi()`: a random village section within 2 (the
//! `SectionPos.cube` order, `level.random.nextInt(size)`), then a random
//! occupied point of interest within 8 of its centre (`nextInt(size)`),
//! then `getPosTowards(mob, 10, 7, atBottomCenterOf(poi))`; `null` at any
//! empty step.

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::goals::random_stroll_goal::RandomStrollGoal;
use crate::ai::mob_ai::MobAi;
use crate::ai::random_pos::{land_get_pos, land_get_pos_towards};
use crate::host::MobHost;
use crate::phys::Vec3;

/// `GolemRandomStrollInVillageGoal`.
#[derive(Debug, Clone)]
pub struct GolemRandomStrollInVillageGoal {
    inner: RandomStrollGoal,
}

impl GolemRandomStrollInVillageGoal {
    /// `GolemRandomStrollInVillageGoal(PathfinderMob, double)`.
    #[must_use]
    pub fn new(speed_modifier: f64) -> Self {
        GolemRandomStrollInVillageGoal {
            inner: RandomStrollGoal::golem_in_village(speed_modifier),
        }
    }

    /// The wanted position, for tests.
    #[must_use]
    pub fn wanted(&self) -> Vec3 {
        self.inner.wanted()
    }
}

/// `getPosition()`.
pub(crate) fn get_position<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
    if cx.mob.level_random_next_float() < 0.3 {
        return towards_anywhere(cx);
    }
    let v = if cx.mob.level_random_next_float() < 0.7 {
        towards_villager_who_wants_golem(cx).or_else(|| towards_poi(cx))
    } else {
        towards_poi(cx).or_else(|| towards_villager_who_wants_golem(cx))
    };
    v.or_else(|| towards_anywhere(cx))
}

fn towards_anywhere<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
    land_get_pos(cx, 10, 7)
}

fn towards_villager_who_wants_golem<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
    let area = cx.mob.bounding_box().inflate(32.0, 32.0, 32.0);
    let list = cx.mob.villagers_wanting_golem_in(&area);
    if list.is_empty() {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let i = cx.mob.level_random_next_int(list.len() as i32);
    #[allow(clippy::cast_sign_loss)]
    let v = cx.mob.entity(list[i as usize])?;
    land_get_pos_towards(cx, 10, 7, v.position())
}

fn towards_poi<M: MobHost>(cx: &mut MobAi<M>) -> Option<Vec3> {
    let bp = cx.mob.block_position();
    let section = (bp.x >> 4, bp.y >> 4, bp.z >> 4);
    let sections = cx.mob.village_sections_in_cube(section, 2);
    if sections.is_empty() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss
    )]
    let s = sections[cx.mob.level_random_next_int(sections.len() as i32) as usize];
    let centre = rustmc_data::BlockPos {
        x: (s.0 << 4) + 8,
        y: (s.1 << 4) + 8,
        z: (s.2 << 4) + 8,
    };
    let pois = cx.mob.occupied_pois_within(centre, 8);
    if pois.is_empty() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss
    )]
    let p = pois[cx.mob.level_random_next_int(pois.len() as i32) as usize];
    land_get_pos_towards(cx, 10, 7, Vec3::at_bottom_center_of(p))
}

impl<M: MobHost> Goal<MobAi<M>> for GolemRandomStrollInVillageGoal {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_use(cx)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        self.inner.can_continue_to_use(cx)
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        self.inner.start(cx);
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        self.inner.stop(cx);
    }

    fn flags(&self) -> FlagSet {
        Goal::<MobAi<M>>::flags(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::RandomSource;

    use crate::host::{EntityClass, EntityFacts};
    use crate::testing::{GridLevel, TestMob};

    /// With the level random's first float over 0.3 and second under
    /// 0.7, and one villager wanting a golem to the east, the stroll
    /// heads east; the mob's own random is untouched by those two draws.
    #[test]
    fn heads_for_a_villager_who_wants_a_golem() {
        let mut g = GridLevel::new();
        g.floor(-60, -60, 60, 60, 63);
        let mut mob = TestMob::new(g, 0.5, 64.0, 0.5, 6);
        mob.others.push((
            EntityClass::Villager,
            EntityFacts::living(3, 24.5, 64.0, 0.5),
        ));
        mob.golem_wanters.push(3);
        let mut ai = MobAi::new(mob);
        let mut goal = GolemRandomStrollInVillageGoal::new(0.6);
        let mut hit = None;
        for _ in 0..2000 {
            let before = ai.mob.level_random;
            if goal.can_use(&mut ai) {
                let mut twin = before;
                let a = twin.next_f32();
                let b = if a < 0.3 { 1.0 } else { twin.next_f32() };
                hit = Some((a, b, goal.wanted()));
                break;
            }
        }
        let (a, b, w) = hit.expect("the interval draw hits within 2000 ticks");
        if a >= 0.3 && b < 0.7 {
            assert!(w.x > 0.5, "toward the villager: {w:?}");
        }
    }
}
