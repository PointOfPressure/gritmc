//! What happens when a player takes a trade: `AbstractVillager.notifyTrade`
//! and the two `rewardTradeXp` implementations, from the 26.2 jar.
//!
//! ```text
//! AbstractVillager.notifyTrade(offer), offsets 0..46:
//!    0: offer.increaseUses()
//!    4: ambientSoundTime = -getAmbientSoundInterval()      // ineg
//!   13: rewardTradeXp(offer)
//!   18: if (tradingPlayer instanceof ServerPlayer) CriteriaTriggers.TRADE.trigger(...)
//!
//! Villager.rewardTradeXp(offer), offsets 0..98:
//!    0: int orbs = 3 + random.nextInt(4)
//!   13: villagerXp += offer.getXp()
//!   26: lastTradedPlayer = getTradingPlayer()
//!   34: if (shouldIncreaseLevel()):
//!   41:     updateMerchantTimer = 40
//!   47:     increaseProfessionLevelOnUpdate = true
//!   52:     orbs += 5                                       // iinc 2, 5
//!   55: if (offer.shouldRewardExp()):
//!   62:     level.addFreshEntity(new ExperienceOrb(level, x, y + 0.5, z, orbs))
//!
//! WanderingTrader.rewardTradeXp(offer), offsets 0..56:
//!    0: if (offer.shouldRewardExp()):
//!    7:     int orbs = 3 + random.nextInt(4)
//!   20:     level.addFreshEntity(new ExperienceOrb(level, x, y + 0.5, z, orbs))
//! ```
//!
//! # The four things that separate the two
//!
//! * **The villager draws its orb count unconditionally and the wandering
//!   trader draws it only when the offer rewards experience.** Same
//!   expression, different position relative to the `shouldRewardExp` guard,
//!   so the two consume the mob's `random` a different number of times for the
//!   same trade. That is a draw-count difference, which is behaviour.
//! * **`orbs += 5` is inside the level-up branch**, so the trade that levels a
//!   villager drops 8..11 orbs and every other one drops 3..6.
//! * **`villagerXp += offer.getXp()` happens before `shouldIncreaseLevel()`
//!   reads it**, so the trade that crosses a threshold levels the villager on
//!   that same trade rather than on the next one.
//! * The wandering trader accumulates no `villagerXp` at all and has no
//!   level. `AbstractVillager.showProgressBar()` is `iconst_1` and
//!   `WanderingTrader` overrides it to `iconst_0`, so the trader's
//!   `merchant_offers` carries `showProgress = false` and its level and xp
//!   fields are meaningless.
//!
//! # The level-up is deferred by 40 ticks and that is observable
//!
//! `increaseProfessionLevelOnUpdate` is read in `Villager.customServerAiStep`,
//! not here: the villager sets a 40-tick `updateMerchantTimer`, and only when
//! that expires does it call `increaseMerchantCareer` — which is
//! `setVillagerData(data.withLevel(level + 1))` then `updateTrades`. So the
//! new trades appear two seconds after the trade that earned them, with the
//! regeneration/particle burst in between. Levelling inside `rewardTradeXp`
//! would put them there immediately and change what a client sees.

use super::draw::should_increase_level;
use super::offer::MerchantOffer;

/// The villager-side state a completed trade moves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TradeState {
    pub villager_xp: i32,
    pub level: i32,
    /// `updateMerchantTimer`, counted down in `customServerAiStep`.
    pub update_merchant_timer: i32,
    /// `increaseProfessionLevelOnUpdate`, read when that timer expires.
    pub increase_profession_level_on_update: bool,
    pub ambient_sound_time: i32,
}

/// What a completed trade asks the world to do. Returned rather than done,
/// because none of it is this crate's: spawning an orb, firing an advancement
/// trigger and playing a sound all need the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeEffects {
    /// The `ExperienceOrb` value, or `None` when the offer does not reward
    /// experience. `Some(0)` is not possible — the count is at least 3.
    pub experience_orbs: Option<i32>,
    /// `CriteriaTriggers.TRADE.trigger(player, villager, offer.getResult())`,
    /// fired only for a `ServerPlayer`.
    pub trade_trigger: bool,
    /// Whether this trade armed the deferred level-up.
    pub levelled: bool,
}

/// `Villager.rewardTradeXp` — and the orb draw, which the caller supplies
/// because it comes off the **mob's** `random`, not off a trade sequence.
///
/// `orb_draw` must be `random.nextInt(4)`.
pub fn villager_reward_trade_xp(
    state: &mut TradeState,
    offer: &MerchantOffer,
    orb_draw: i32,
) -> TradeEffects {
    let mut orbs = 3 + orb_draw;
    state.villager_xp += offer.xp();
    let mut levelled = false;
    if should_increase_level(state.level, state.villager_xp) {
        state.update_merchant_timer = 40;
        state.increase_profession_level_on_update = true;
        orbs += 5;
        levelled = true;
    }
    TradeEffects {
        experience_orbs: offer.should_reward_exp().then_some(orbs),
        trade_trigger: false,
        levelled,
    }
}

/// `WanderingTrader.rewardTradeXp`. The guard comes **first**, so the draw
/// only happens when the offer rewards experience.
pub fn wandering_trader_reward_trade_xp(offer: &MerchantOffer, orb_draw: i32) -> TradeEffects {
    TradeEffects {
        experience_orbs: offer.should_reward_exp().then(|| 3 + orb_draw),
        trade_trigger: false,
        levelled: false,
    }
}

/// `AbstractVillager.notifyTrade`, with the villager's `rewardTradeXp`.
///
/// `ambient_sound_interval` is `Mob.getAmbientSoundInterval()`, negated —
/// which pushes the next ambient sound a full interval further out rather
/// than resetting it to zero, so a villager that has just traded is quiet for
/// twice as long as one that has just spoken.
pub fn villager_notify_trade(
    state: &mut TradeState,
    offer: &mut MerchantOffer,
    orb_draw: i32,
    ambient_sound_interval: i32,
    trading_player_is_server_player: bool,
) -> TradeEffects {
    offer.increase_uses();
    state.ambient_sound_time = -ambient_sound_interval;
    let mut e = villager_reward_trade_xp(state, offer, orb_draw);
    e.trade_trigger = trading_player_is_server_player;
    e
}

/// The order `WanderingTrader.updateTrades` adds its three trade sets in.
///
/// **BUYING, UNCOMMON, COMMON** — offsets 8, 17 and 26. Not the order they
/// are declared in `TradeSets`, not alphabetical, and not the order the data
/// files sort in. It decides the index of every offer in the list the client
/// is sent and therefore what a `select_trade` index means, and the three
/// sets draw from three different random sequences in this order.
pub const WANDERING_TRADER_TRADE_SETS: [&str; 3] = [
    "minecraft:wandering_trader/buying",
    "minecraft:wandering_trader/uncommon",
    "minecraft:wandering_trader/common",
];

#[cfg(test)]
mod tests {
    use super::super::offer::{ItemCost, MerchantOffer, Stack};
    use super::*;

    fn offer(xp: i32) -> MerchantOffer {
        MerchantOffer::new(
            ItemCost::new("minecraft:emerald", 1, 64),
            None,
            Stack::new("minecraft:bread", 6),
            16,
            xp,
            0.05,
        )
    }

    /// The positive control: a trade below the threshold banks its xp, uses a
    /// charge and drops 3..6 orbs, and nothing about the level moves.
    #[test]
    fn an_ordinary_trade_banks_xp_and_drops_three_to_six_orbs() {
        let mut st = TradeState {
            level: 1,
            ..Default::default()
        };
        let mut o = offer(2);
        let e = villager_notify_trade(&mut st, &mut o, 0, 80, true);
        assert_eq!(o.uses(), 1);
        assert_eq!(st.villager_xp, 2);
        assert_eq!(st.ambient_sound_time, -80);
        assert_eq!(e.experience_orbs, Some(3));
        assert!(!e.levelled);
        assert!(!st.increase_profession_level_on_update);
        assert_eq!(st.update_merchant_timer, 0);
        assert!(e.trade_trigger);

        let mut st2 = TradeState {
            level: 1,
            ..Default::default()
        };
        let e2 = villager_notify_trade(&mut st2, &mut offer(2), 3, 80, false);
        assert_eq!(e2.experience_orbs, Some(6), "nextInt(4) tops out at 3");
        assert!(!e2.trade_trigger);
    }

    /// The trade that crosses the threshold levels on ITSELF — the xp is
    /// banked before `shouldIncreaseLevel` reads it — and drops five extra
    /// orbs.
    #[test]
    fn the_trade_that_crosses_the_threshold_levels_on_itself_and_pays_five_more() {
        let mut st = TradeState {
            level: 1,
            villager_xp: 8,
            ..Default::default()
        };
        let e = villager_reward_trade_xp(&mut st, &offer(2), 0);
        assert_eq!(st.villager_xp, 10, "level 1's threshold is exactly 10");
        assert!(e.levelled);
        assert_eq!(e.experience_orbs, Some(8), "3 + 0 + 5");
        assert_eq!(st.update_merchant_timer, 40);
        assert!(st.increase_profession_level_on_update);

        // One xp short and none of it happens, which is what makes the
        // assertion above about the threshold rather than about the branch.
        let mut short = TradeState {
            level: 1,
            villager_xp: 7,
            ..Default::default()
        };
        let e2 = villager_reward_trade_xp(&mut short, &offer(2), 0);
        assert!(!e2.levelled);
        assert_eq!(e2.experience_orbs, Some(3));
    }

    /// A level-5 villager banks xp forever and never levels again, so the
    /// bonus orbs stop too.
    #[test]
    fn a_level_five_villager_never_gets_the_bonus() {
        let mut st = TradeState {
            level: 5,
            villager_xp: 10_000,
            ..Default::default()
        };
        let e = villager_reward_trade_xp(&mut st, &offer(5), 3);
        assert!(!e.levelled);
        assert_eq!(e.experience_orbs, Some(6));
        assert_eq!(st.villager_xp, 10_005);
    }

    /// An offer with `rewardExp` off spawns nothing — but the villager still
    /// banks the xp and can still level on it. The two are separate fields
    /// and collapsing them would stop such a trade counting towards a level.
    #[test]
    fn an_offer_that_rewards_no_experience_still_banks_villager_xp() {
        let mut st = TradeState {
            level: 1,
            villager_xp: 9,
            ..Default::default()
        };
        let mut o = MerchantOffer::with_state(
            ItemCost::new("minecraft:emerald", 1, 64),
            None,
            Stack::new("minecraft:bread", 6),
            0,
            16,
            false,
            0,
            0,
            0.05,
            2,
        );
        let e = villager_notify_trade(&mut st, &mut o, 1, 80, false);
        assert_eq!(e.experience_orbs, None);
        assert_eq!(st.villager_xp, 11);
        assert!(e.levelled, "it still crossed the threshold");
    }

    /// The wandering trader's guard is before its draw and the villager's is
    /// after, so for an offer that rewards nothing the villager has consumed
    /// a `nextInt(4)` and the trader has not.
    #[test]
    fn the_two_reward_paths_consume_the_draw_differently() {
        let no_exp = MerchantOffer::with_state(
            ItemCost::new("minecraft:emerald", 1, 64),
            None,
            Stack::new("minecraft:bread", 6),
            0,
            16,
            false,
            0,
            0,
            0.05,
            1,
        );
        assert_eq!(
            wandering_trader_reward_trade_xp(&no_exp, 2).experience_orbs,
            None
        );
        assert_eq!(
            wandering_trader_reward_trade_xp(&offer(1), 2).experience_orbs,
            Some(5)
        );
        // The trader never levels and banks nothing, however large the xp.
        assert!(!wandering_trader_reward_trade_xp(&offer(1000), 0).levelled);
    }

    /// The wandering trader adds BUYING, UNCOMMON, COMMON in that order —
    /// not the declaration order and not the file order, and it fixes what
    /// every `select_trade` index means.
    #[test]
    fn the_wandering_traders_set_order_is_buying_uncommon_common() {
        assert_eq!(
            WANDERING_TRADER_TRADE_SETS,
            [
                "minecraft:wandering_trader/buying",
                "minecraft:wandering_trader/uncommon",
                "minecraft:wandering_trader/common",
            ]
        );
        let mut sorted = WANDERING_TRADER_TRADE_SETS;
        sorted.sort_unstable();
        assert_ne!(
            sorted, WANDERING_TRADER_TRADE_SETS,
            "the order is not alphabetical, so it cannot be recovered by sorting"
        );
    }
}
