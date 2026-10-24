//! `net.minecraft.world.entity.ai.goal.UseItemGoal<T extends Mob>`, from
//! the 26.2 jar. No flags (`setFlags` is never called). Used by the
//! WanderingTrader twice (invisibility potion at dusk, milk at dawn).
//!
//! ```text
//! canUse()            canUseSelector.test(mob)
//! canContinueToUse()  mob.isUsingItem()
//! start()             mob.setItemSlot(MAINHAND, item.copy()); mob.startUsingItem(MAIN_HAND)
//! stop()              mob.setItemSlot(MAINHAND, ItemStack.EMPTY)
//!                     if finishUsingSound != null:
//!                         mob.playSound(finishUsingSound, 1.0f, mob.getRandom().nextFloat() * 0.2f + 0.9f)
//! ```
//!
//! The pitch draw is one `nextFloat` at offset 36 of `stop`, taken only
//! when there is a sound.

use rustmc_worldgen::rng::RandomSource;

use crate::ai::goal::{FlagSet, Goal};
use crate::ai::mob_ai::MobAi;
use crate::host::{ItemSpec, MobHost, SoundSpec};

/// `UseItemGoal`.
pub struct UseItemGoal<M: MobHost> {
    item: ItemSpec,
    finish_using_sound: Option<SoundSpec>,
    can_use_selector: fn(&M) -> bool,
}

impl<M: MobHost> std::fmt::Debug for UseItemGoal<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UseItemGoal")
            .field("item", &self.item)
            .field("finish_using_sound", &self.finish_using_sound)
            .finish_non_exhaustive()
    }
}

impl<M: MobHost> UseItemGoal<M> {
    /// `UseItemGoal(T, ItemStack, SoundEvent, Predicate<? super T>)`.
    #[must_use]
    pub fn new(
        item: ItemSpec,
        finish_using_sound: Option<SoundSpec>,
        can_use_selector: fn(&M) -> bool,
    ) -> Self {
        UseItemGoal {
            item,
            finish_using_sound,
            can_use_selector,
        }
    }
}

impl<M: MobHost> Goal<MobAi<M>> for UseItemGoal<M> {
    fn can_use(&mut self, cx: &mut MobAi<M>) -> bool {
        (self.can_use_selector)(&cx.mob)
    }

    fn can_continue_to_use(&mut self, cx: &mut MobAi<M>) -> bool {
        cx.mob.is_using_item()
    }

    fn start(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_main_hand_item(Some(&self.item));
        cx.mob.start_using_main_hand();
    }

    fn stop(&mut self, cx: &mut MobAi<M>) {
        cx.mob.set_main_hand_item(None);
        if let Some(sound) = self.finish_using_sound {
            let pitch = cx.mob.random().next_f32() * 0.2 + 0.9;
            cx.mob.play_sound(&sound, 1.0, pitch);
        }
    }

    fn flags(&self) -> FlagSet {
        FlagSet::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    use crate::testing::{GridLevel, TestMob};

    const MILK: ItemSpec = ItemSpec {
        id: "milk_bucket",
        count: 1,
        component: None,
    };

    #[test]
    fn uses_the_item_while_the_predicate_holds_and_plays_the_finish_sound() {
        let mob = TestMob::new(GridLevel::new(), 0.5, 64.0, 0.5, 7);
        let mut ai = MobAi::new(mob);
        let mut goal = UseItemGoal::new(
            MILK,
            Some(SoundSpec("entity.wandering_trader.drink_milk")),
            |m: &TestMob| m.tick_count > 5,
        );
        assert!(!goal.can_use(&mut ai));
        ai.mob.tick_count = 6;
        assert!(goal.can_use(&mut ai));
        goal.start(&mut ai);
        assert_eq!(ai.mob.main_hand.as_ref().map(|i| i.id), Some("milk_bucket"));
        assert!(goal.can_continue_to_use(&mut ai));
        ai.mob.using_item = false;
        assert!(!goal.can_continue_to_use(&mut ai));
        let mut twin = LegacyRandomSource::new(7);
        let pitch = twin.next_f32() * 0.2 + 0.9;
        goal.stop(&mut ai);
        assert!(ai.mob.main_hand.is_none());
        assert_eq!(
            ai.mob.calls.last().map(String::as_str),
            Some(format!("playSound(entity.wandering_trader.drink_milk, 1, {pitch})").as_str())
        );
        assert!(goal.flags().is_empty());
    }
}
