//! `net.minecraft.world.entity.ai.sensing.Sensing`, from the 26.2 jar.
//!
//! A per-tick memo of `Mob.hasLineOfSight(Entity)` keyed by entity id:
//! two `IntOpenHashSet`s, `seen` and `unseen`, both cleared by `tick()`
//! at the top of `Mob.serverAiStep`. The first query for an entity in a
//! tick does the ray cast and records the answer; every later query that
//! tick reads the memo. The cast itself is
//! `LivingEntity.hasLineOfSight(entity, COLLIDER, NONE, entity.getEyeY())`,
//! which the host answers.

use std::collections::HashSet;

/// `Sensing`.
#[derive(Debug, Default)]
pub struct Sensing {
    seen: HashSet<i32>,
    unseen: HashSet<i32>,
}

impl Sensing {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `tick()`: clear both sets.
    pub fn tick(&mut self) {
        self.seen.clear();
        self.unseen.clear();
    }

    /// `hasLineOfSight(Entity)`: the memo, else `cast()` and record.
    pub fn has_line_of_sight(&mut self, entity_id: i32, cast: impl FnOnce() -> bool) -> bool {
        if self.seen.contains(&entity_id) {
            return true;
        }
        if self.unseen.contains(&entity_id) {
            return false;
        }
        let v = cast();
        if v {
            self.seen.insert(entity_id);
        } else {
            self.unseen.insert(entity_id);
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_query_casts_and_the_rest_read_the_memo() {
        let mut s = Sensing::new();
        let mut casts = 0;
        assert!(s.has_line_of_sight(7, || {
            casts += 1;
            true
        }));
        assert!(s.has_line_of_sight(7, || {
            casts += 1;
            false
        }));
        assert_eq!(casts, 1);
        assert!(!s.has_line_of_sight(8, || {
            casts += 1;
            false
        }));
        assert!(!s.has_line_of_sight(8, || {
            casts += 1;
            true
        }));
        assert_eq!(casts, 2);
        s.tick();
        assert!(!s.has_line_of_sight(7, || {
            casts += 1;
            false
        }));
        assert_eq!(casts, 3);
    }
}
