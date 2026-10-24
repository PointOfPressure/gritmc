//! `Villager`'s restock state machine, from the 26.2 jar.
//!
//! Five methods and four fields, and every one of the five reads or writes
//! state that another one of them set, which is why they are ported together
//! as a type rather than one at a time.
//!
//! ```text
//! canRestock()        -> iconst_1; ireturn        // a Villager always can
//! needsToRestock()    -> any offer.needsRestock()  (uses > 0)
//! allowedToRestock()  -> numberOfRestocksToday == 0
//!                        || (numberOfRestocksToday < 2
//!                            && gameTime > lastRestockGameTime + 2400)
//! shouldRestock(level):
//!    0: nextRestock = lastRestockGameTime + 12000
//!   18: flag = gameTime > nextRestock
//!   32: day = registryAccess.get(Timelines.OVERWORLD_DAY)
//!              .map(t -> t.getPeriodCount(clockManager)).orElse(0)
//!   72: flag |= (lastRestockCheckDay > 0 && day > lastRestockCheckDay)
//!  101: lastRestockCheckDay = day
//!  107: if flag: lastRestockGameTime = gameTime; resetNumberOfRestocks()
//!  122: return allowedToRestock() && needsToRestock()
//! restock():
//!    0: updateDemand()                 // every offer
//!    4: every offer.resetUses()
//!   38: resendOffersToTradingPlayer()
//!   42: lastRestockGameTime = gameTime
//!   53: numberOfRestocksToday++
//! resetNumberOfRestocks() -> catchUpDemand(); numberOfRestocksToday = 0
//! catchUpDemand():
//!    0: n = 2 - numberOfRestocksToday
//!    7: if n > 0: every offer.resetUses()
//!   45: repeat n times: updateDemand()
//!   62: resendOffersToTradingPlayer()
//! ```
//!
//! # Three things a plausible reading gets wrong
//!
//! * **`shouldRestock` has a side effect and is called every tick a villager
//!   works.** It writes `lastRestockCheckDay` unconditionally and, on the
//!   day/time boundary, `lastRestockGameTime` and the restock counter. Making
//!   it a pure predicate would freeze `numberOfRestocksToday` at 2 and the
//!   villager would restock twice and then never again.
//! * **The day term is armed only after the first call.**
//!   `lastRestockCheckDay > 0` is required, so a villager loaded on day 0 —
//!   or one whose saved field is absent — does not get a free restock from
//!   the day comparison on its first tick; it gets one from the 12000-tick
//!   term or not at all. `DEFAULT_LAST_RESTOCK` is 0 and that is what makes
//!   the guard meaningful rather than decoration.
//! * **`catchUpDemand` runs `updateDemand` for the restocks that did NOT
//!   happen**, after resetting uses. `updateDemand` is
//!   `demand += uses - (maxUses - uses)`, so with uses now 0 each catch-up
//!   round subtracts `maxUses` from demand — the mechanism by which a trade
//!   nobody buys gets cheaper over days. Skipping it, or running it before
//!   the reset, leaves prices permanently high.
//!
//! The caller is `WorkAtPoi.tick`: `useWorkstation(level, villager)`, then
//! `if (villager.shouldRestock(level)) villager.restock()`. That behaviour
//! belongs to the brain lane; this module is the villager side of it.

use super::offer::MerchantOffers;

/// `Villager.DEFAULT_LAST_RESTOCK` / `DEFAULT_RESTOCKS_TODAY`, and the two
/// windows.
pub const RESTOCK_COOLDOWN_TICKS: i64 = 2400;
pub const RESTOCK_DAY_TICKS: i64 = 12000;
pub const MAX_RESTOCKS_PER_DAY: i32 = 2;

/// The four save fields the restock path reads and writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestockState {
    pub last_restock_game_time: i64,
    pub number_of_restocks_today: i32,
    pub last_restock_check_day: i64,
}

/// What `restock`/`shouldRestock` did, so a caller can see the side effects
/// without reading the fields back. `resend` is
/// `resendOffersToTradingPlayer()`, which re-sends `merchant_offers` to the
/// player currently in the trading screen — it is not bookkeeping, a client
/// that never gets it shows stale stock and prices.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RestockEffects {
    pub resend_offers: bool,
    pub day_rolled: bool,
}

impl RestockState {
    /// `Villager.canRestock()` — `iconst_1`.
    ///
    /// It is a method because the **interface default is the opposite**:
    /// `Merchant.canRestock()` is `iconst_0`, and `WanderingTrader` does not
    /// override it, so a wandering trader's stock never comes back. Reading
    /// the villager's `true` as the merchant behaviour would give every
    /// wandering trader an infinite shop. The flag is also sent on the wire —
    /// `Merchant.openTradingScreen` passes it as the last argument of
    /// `sendMerchantOffers`, which is `merchant_offers`' final boolean.
    #[must_use]
    pub fn can_restock() -> bool {
        true
    }

    /// `needsToRestock()`: any offer with `uses > 0`.
    #[must_use]
    pub fn needs_to_restock(offers: &MerchantOffers) -> bool {
        offers
            .0
            .iter()
            .any(super::offer::MerchantOffer::needs_restock)
    }

    /// `allowedToRestock()`.
    #[must_use]
    pub fn allowed_to_restock(&self, game_time: i64) -> bool {
        self.number_of_restocks_today == 0
            || (self.number_of_restocks_today < MAX_RESTOCKS_PER_DAY
                && game_time > self.last_restock_game_time + RESTOCK_COOLDOWN_TICKS)
    }

    /// `updateDemand()` — the villager's, which is `updateDemand` on every
    /// offer.
    pub fn update_demand(offers: &mut MerchantOffers) {
        for o in &mut offers.0 {
            o.update_demand();
        }
    }

    /// `catchUpDemand()`.
    pub fn reset_number_of_restocks(&mut self, offers: &mut MerchantOffers) -> RestockEffects {
        let n = MAX_RESTOCKS_PER_DAY - self.number_of_restocks_today;
        if n > 0 {
            for o in &mut offers.0 {
                o.reset_uses();
            }
        }
        for _ in 0..n {
            Self::update_demand(offers);
        }
        self.number_of_restocks_today = 0;
        RestockEffects {
            resend_offers: true,
            day_rolled: false,
        }
    }

    /// `shouldRestock(ServerLevel)` — **not pure**. `day` is
    /// `Timelines.OVERWORLD_DAY.getPeriodCount(clockManager)`, which the
    /// caller supplies because clocks are not this module's.
    pub fn should_restock(
        &mut self,
        offers: &mut MerchantOffers,
        game_time: i64,
        overworld_day: i64,
    ) -> (bool, RestockEffects) {
        let next_restock = self.last_restock_game_time + RESTOCK_DAY_TICKS;
        let mut flag = game_time > next_restock;
        flag |= self.last_restock_check_day > 0 && overworld_day > self.last_restock_check_day;
        self.last_restock_check_day = overworld_day;
        let mut effects = RestockEffects::default();
        if flag {
            self.last_restock_game_time = game_time;
            effects = self.reset_number_of_restocks(offers);
            effects.day_rolled = true;
        }
        (
            self.allowed_to_restock(game_time) && Self::needs_to_restock(offers),
            effects,
        )
    }

    /// `restock()`. Note the order: demand is updated **before** uses are
    /// reset, so the demand step sees the uses this period actually had.
    /// Resetting first would make every restock read `uses = 0` and drive
    /// demand down by `maxUses` however busy the trade was.
    pub fn restock(&mut self, offers: &mut MerchantOffers, game_time: i64) -> RestockEffects {
        Self::update_demand(offers);
        for o in &mut offers.0 {
            o.reset_uses();
        }
        self.last_restock_game_time = game_time;
        self.number_of_restocks_today += 1;
        RestockEffects {
            resend_offers: true,
            day_rolled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::offer::{ItemCost, MerchantOffer, Stack};
    use super::*;

    fn offers(max_uses: i32) -> MerchantOffers {
        let mut o = MerchantOffers::new();
        o.push(MerchantOffer::new(
            ItemCost::new("minecraft:emerald", 5, 64),
            None,
            Stack::new("minecraft:book", 1),
            max_uses,
            2,
            0.05,
        ));
        o
    }

    /// The positive control: a villager nobody has traded with needs no
    /// restock however long it waits, so any later "it restocked" is the
    /// trading and not the clock.
    #[test]
    fn an_untraded_villager_never_needs_a_restock() {
        let mut st = RestockState::default();
        let mut o = offers(4);
        let (should, _) = st.should_restock(&mut o, 1_000_000, 12);
        assert!(!should);
        assert!(!RestockState::needs_to_restock(&o));
    }

    #[test]
    fn one_use_is_enough_to_need_a_restock() {
        let mut o = offers(4);
        o.0[0].increase_uses();
        assert!(RestockState::needs_to_restock(&o));
        let mut st = RestockState::default();
        let (should, _) = st.should_restock(&mut o, 100, 0);
        assert!(should, "the first restock of the day is always allowed");
    }

    /// Two restocks a day, and the second one waits 2400 ticks. The third is
    /// refused however long it waits.
    #[test]
    fn two_restocks_a_day_with_a_2400_tick_gap() {
        let mut st = RestockState::default();
        let mut o = offers(4);
        o.0[0].increase_uses();
        assert!(st.allowed_to_restock(0), "none yet today");
        st.restock(&mut o, 1_000);
        assert_eq!(st.number_of_restocks_today, 1);
        assert_eq!(st.last_restock_game_time, 1_000);
        assert!(
            !st.allowed_to_restock(1_000 + 2_400),
            "the gap is exclusive"
        );
        assert!(st.allowed_to_restock(1_000 + 2_401));
        o.0[0].increase_uses();
        st.restock(&mut o, 4_000);
        assert_eq!(st.number_of_restocks_today, 2);
        assert!(
            !st.allowed_to_restock(4_000 + 100_000),
            "the third restock of a day is refused whatever the clock says"
        );
    }

    /// `restock` updates demand from the uses the period **had**, then zeroes
    /// them. A busy trade gets dearer, an idle one gets cheaper.
    #[test]
    fn restock_prices_from_the_uses_it_is_about_to_clear() {
        let mut st = RestockState::default();
        let mut busy = offers(4);
        for _ in 0..4 {
            busy.0[0].increase_uses();
        }
        st.restock(&mut busy, 10);
        assert_eq!(busy.0[0].demand(), 4, "4 uses of 4 -> +maxUses");
        assert_eq!(busy.0[0].uses(), 0);

        let mut st2 = RestockState::default();
        let mut idle = offers(4);
        idle.0[0].increase_uses();
        st2.restock(&mut idle, 10);
        assert_eq!(idle.0[0].demand(), -2, "1 use of 4 -> 1 - 3");
    }

    /// The day term is armed only once `lastRestockCheckDay` is above zero.
    /// A villager whose first ever tick is on day 7 must not be handed a
    /// day-rollover restock for a day it was not alive through.
    #[test]
    fn the_day_term_needs_a_previous_check() {
        let mut st = RestockState::default();
        let mut o = offers(4);
        o.0[0].increase_uses();
        // First call: `lastRestockCheckDay` is 0, and the game time is inside
        // the 12000 window, so nothing rolls — but the day is recorded.
        let (_, e) = st.should_restock(&mut o, 5, 7);
        assert!(!e.day_rolled, "day 7 on the first check must not roll");
        assert_eq!(st.last_restock_check_day, 7);
        // Second call on day 8 rolls, resets the counter and re-stocks uses.
        st.number_of_restocks_today = 2;
        let (_, e2) = st.should_restock(&mut o, 6, 8);
        assert!(e2.day_rolled);
        assert_eq!(st.number_of_restocks_today, 0);
        assert!(e2.resend_offers);
    }

    /// The 12000-tick term works on its own, with the day never moving.
    #[test]
    fn twelve_thousand_ticks_rolls_without_a_day_change() {
        let mut st = RestockState {
            last_restock_game_time: 100,
            number_of_restocks_today: 2,
            last_restock_check_day: 3,
        };
        let mut o = offers(4);
        let (_, e) = st.should_restock(&mut o, 100 + 12_000, 3);
        assert!(!e.day_rolled, "the comparison is strictly greater");
        let (_, e2) = st.should_restock(&mut o, 100 + 12_001, 3);
        assert!(e2.day_rolled);
        assert_eq!(st.number_of_restocks_today, 0);
    }

    /// `catchUpDemand` runs `updateDemand` once per restock the villager did
    /// NOT do today, after clearing uses — so a day with no restocks at all
    /// takes demand down by `2 * maxUses`. That is the whole "unbought trades
    /// get cheaper" mechanism and it is invisible in any single-day test.
    #[test]
    fn catching_up_runs_update_demand_for_the_missed_restocks() {
        let mut st = RestockState::default();
        let mut o = offers(6);
        st.reset_number_of_restocks(&mut o);
        assert_eq!(o.0[0].demand(), -12, "two catch-up rounds of -maxUses");

        let mut st1 = RestockState {
            number_of_restocks_today: 1,
            ..Default::default()
        };
        let mut o1 = offers(6);
        st1.reset_number_of_restocks(&mut o1);
        assert_eq!(o1.0[0].demand(), -6, "one round missed");

        let mut st2 = RestockState {
            number_of_restocks_today: 2,
            ..Default::default()
        };
        let mut o2 = offers(6);
        o2.0[0].increase_uses();
        st2.reset_number_of_restocks(&mut o2);
        assert_eq!(o2.0[0].demand(), 0, "nothing missed, no rounds");
        assert_eq!(
            o2.0[0].uses(),
            1,
            "and uses are NOT cleared when n <= 0, which is the `if n > 0` guard"
        );
    }

    /// Demand cannot make a trade cheaper than its base — the price path's
    /// `max(0, ..)` — so all that negative demand is stored potential, not a
    /// discount. Checked here rather than only in `offer.rs` because this is
    /// where the negative numbers come from.
    #[test]
    fn accumulated_negative_demand_does_not_discount() {
        let mut st = RestockState::default();
        let mut o = offers(6);
        for _ in 0..10 {
            st.number_of_restocks_today = 0;
            st.reset_number_of_restocks(&mut o);
        }
        assert!(o.0[0].demand() <= -100, "{}", o.0[0].demand());
        assert_eq!(o.0[0].cost_a().count, 5, "still the base price");
    }
}
