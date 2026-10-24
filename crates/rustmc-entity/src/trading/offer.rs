//! `net.minecraft.world.item.trading.MerchantOffer` and `MerchantOffers`,
//! from the 26.2 jar.
//!
//! # The price arithmetic, and the name that changed
//!
//! Pre-26.2 accounts call the price path `getModifiedPrice`. In 26.2 it is
//! **`getModifiedCostCount(ItemCost)`**, private, and `getCostA()` is its only
//! caller besides `satisfiedBy` and `take`:
//!
//! ```text
//! private int getModifiedCostCount(ItemCost cost);
//!    0: cost.count()                       -> base
//!    5: Math.max(0, Mth.floor((float)(base * demand) * priceMultiplier))
//!   25: Mth.clamp(base + that + specialPriceDiff, 1, cost.itemStack().getMaxStackSize())
//! ```
//!
//! Three things in there are easy to get wrong and all three are checked
//! below. The `max(0, ..)` clamps the **demand term only**, so a negative
//! `specialPriceDiff` — which is how a hero-of-the-village discount is
//! applied — still reduces the price. `Mth.floor` is applied to the product
//! `base * demand * priceMultiplier` as a **float**, not to each factor. And
//! the outer clamp's lower bound is **1**, not 0: a trade can never become
//! free however large the discount.
//!
//! `updateDemand` is `demand = demand + uses - (maxUses - uses)`
//! (offsets 0..23), which is `demand + 2*uses - maxUses`. It is signed and
//! unbounded below, so an untouched offer restocked repeatedly drives demand
//! negative and the `max(0, ..)` above is what stops that lowering the price.
//!
//! # `specialPriceDiff` is an input here
//!
//! Nothing in this module writes it except [`MerchantOffer::set_special_price_diff`]
//! and [`MerchantOffer::add_to_special_price_diff`], which are the two
//! mutators vanilla exposes. What *calls* them is
//! `Villager.updateSpecialPrices`, reading `GossipContainer` /
//! `getPlayerReputation` — a separate area, and refused here by name rather
//! than approximated. Same for `priceMultiplier`, which
//! `VillagerTrade.getOffer` fills from the listing's `reputation_discount`.

/// `net.minecraft.world.item.trading.ItemCost` — an item, a count and an
/// exact-component predicate.
///
/// `max_stack_size` is carried on the cost because the outer clamp in
/// `getModifiedCostCount` needs `cost.itemStack().getMaxStackSize()` and this
/// crate has no item registry. A caller that does not know it should pass
/// vanilla's default of 64 knowingly, not by omission — the two differ on
/// every listing that wants a bucket, a bed, a boat or a banner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemCost {
    pub item: String,
    pub count: i32,
    pub max_stack_size: i32,
}

impl ItemCost {
    #[must_use]
    pub fn new(item: impl Into<String>, count: i32, max_stack_size: i32) -> Self {
        ItemCost {
            item: item.into(),
            count,
            max_stack_size,
        }
    }

    /// `TradeCost.toItemCost(ctx, extra)`, offsets 0..31:
    /// `Mth.clamp(count.getInt(ctx) + extra, 0, item.getDefaultMaxStackSize())`.
    ///
    /// Note the lower bound is **0** here and **1** in
    /// `getModifiedCostCount`. `VillagerTrade.getOffer` then refuses the whole
    /// listing when this lands below 1 (offsets 208..218), which is a
    /// different outcome from clamping it up to 1 and would put an
    /// unobtainable trade on the villager.
    #[must_use]
    pub fn from_trade_cost(
        item: impl Into<String>,
        count: i32,
        extra: i32,
        default_max_stack_size: i32,
    ) -> Self {
        ItemCost {
            item: item.into(),
            count: (count + extra).clamp(0, default_max_stack_size),
            max_stack_size: default_max_stack_size,
        }
    }
}

/// A stack as far as this module needs one: what it is and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stack {
    pub item: String,
    pub count: i32,
}

impl Stack {
    #[must_use]
    pub fn new(item: impl Into<String>, count: i32) -> Self {
        Stack {
            item: item.into(),
            count,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.count <= 0 || self.item.is_empty()
    }
}

/// `MerchantOffer`.
#[derive(Debug, Clone, PartialEq)]
pub struct MerchantOffer {
    base_cost_a: ItemCost,
    cost_b: Option<ItemCost>,
    result: Stack,
    uses: i32,
    max_uses: i32,
    reward_exp: bool,
    special_price_diff: i32,
    demand: i32,
    price_multiplier: f32,
    xp: i32,
}

impl MerchantOffer {
    /// The constructor `VillagerTrade.getOffer` calls (offsets 259..310):
    /// `MerchantOffer(ItemCost, Optional<ItemCost>, ItemStack, int maxUses,
    /// int xp, float priceMultiplier)`, with `uses = 0`, `demand = 0`,
    /// `specialPriceDiff = 0` and `rewardExp = true`.
    #[must_use]
    pub fn new(
        base_cost_a: ItemCost,
        cost_b: Option<ItemCost>,
        result: Stack,
        max_uses: i32,
        xp: i32,
        price_multiplier: f32,
    ) -> Self {
        MerchantOffer {
            base_cost_a,
            cost_b,
            result,
            uses: 0,
            max_uses,
            reward_exp: true,
            special_price_diff: 0,
            demand: 0,
            price_multiplier,
            xp,
        }
    }

    /// The full-field form, which is what `createFromStream` builds and what a
    /// save round trip needs.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn with_state(
        base_cost_a: ItemCost,
        cost_b: Option<ItemCost>,
        result: Stack,
        uses: i32,
        max_uses: i32,
        reward_exp: bool,
        special_price_diff: i32,
        demand: i32,
        price_multiplier: f32,
        xp: i32,
    ) -> Self {
        MerchantOffer {
            base_cost_a,
            cost_b,
            result,
            uses,
            max_uses,
            reward_exp,
            special_price_diff,
            demand,
            price_multiplier,
            xp,
        }
    }

    /// `getModifiedCostCount(ItemCost)`.
    #[must_use]
    pub fn modified_cost_count(&self, cost: &ItemCost) -> i32 {
        let base = cost.count;
        // `(float)(base * demand) * priceMultiplier` — the int product is
        // widened to f32 first, exactly as `i2f` then `fmul` does.
        let adj = ((base * self.demand) as f32 * self.price_multiplier)
            .floor()
            .max(0.0) as i32;
        (base + adj + self.special_price_diff).clamp(1, cost.max_stack_size)
    }

    /// `getCostA()` — the base cost stack with the modified count.
    #[must_use]
    pub fn cost_a(&self) -> Stack {
        Stack::new(
            self.base_cost_a.item.clone(),
            self.modified_cost_count(&self.base_cost_a),
        )
    }

    /// `getCostB()` — unmodified. The second cost has never been price-adjusted
    /// in any version and `writeToStream` sends it as the raw `ItemCost`.
    #[must_use]
    pub fn cost_b(&self) -> Option<Stack> {
        self.cost_b
            .as_ref()
            .map(|c| Stack::new(c.item.clone(), c.count))
    }

    #[must_use]
    pub fn base_cost_a(&self) -> &ItemCost {
        &self.base_cost_a
    }

    #[must_use]
    pub fn item_cost_b(&self) -> Option<&ItemCost> {
        self.cost_b.as_ref()
    }

    #[must_use]
    pub fn result(&self) -> &Stack {
        &self.result
    }

    /// `assemble()` — a copy of the result. It is a copy in vanilla because
    /// the caller mutates it; here the clone says the same thing.
    #[must_use]
    pub fn assemble(&self) -> Stack {
        self.result.clone()
    }

    /// `updateDemand()`: `demand += uses - (maxUses - uses)`.
    pub fn update_demand(&mut self) {
        self.demand = self.demand + self.uses - (self.max_uses - self.uses);
    }

    #[must_use]
    pub fn uses(&self) -> i32 {
        self.uses
    }

    pub fn reset_uses(&mut self) {
        self.uses = 0;
    }

    pub fn increase_uses(&mut self) {
        self.uses += 1;
    }

    #[must_use]
    pub fn max_uses(&self) -> i32 {
        self.max_uses
    }

    #[must_use]
    pub fn demand(&self) -> i32 {
        self.demand
    }

    pub fn add_to_special_price_diff(&mut self, d: i32) {
        self.special_price_diff += d;
    }

    pub fn reset_special_price_diff(&mut self) {
        self.special_price_diff = 0;
    }

    pub fn set_special_price_diff(&mut self, d: i32) {
        self.special_price_diff = d;
    }

    #[must_use]
    pub fn special_price_diff(&self) -> i32 {
        self.special_price_diff
    }

    #[must_use]
    pub fn price_multiplier(&self) -> f32 {
        self.price_multiplier
    }

    #[must_use]
    pub fn xp(&self) -> i32 {
        self.xp
    }

    /// `isOutOfStock()`: `uses >= maxUses`.
    #[must_use]
    pub fn is_out_of_stock(&self) -> bool {
        self.uses >= self.max_uses
    }

    /// `setToOutOfStock()`: `uses = maxUses`.
    pub fn set_to_out_of_stock(&mut self) {
        self.uses = self.max_uses;
    }

    /// `needsRestock()`: `uses > 0`. Note this is **not** the negation of
    /// `isOutOfStock` — a partly-used offer needs a restock and is in stock.
    #[must_use]
    pub fn needs_restock(&self) -> bool {
        self.uses > 0
    }

    #[must_use]
    pub fn should_reward_exp(&self) -> bool {
        self.reward_exp
    }

    /// `satisfiedBy(ItemStack, ItemStack)`, offsets 0..85.
    ///
    /// Cost A is tested against the **modified** count and cost B against the
    /// **raw** one. When there is no cost B the second slot must be *empty*,
    /// so a player holding something in it cannot complete a one-cost trade.
    #[must_use]
    pub fn satisfied_by(&self, a: &Stack, b: &Stack) -> bool {
        if a.item != self.base_cost_a.item || a.count < self.modified_cost_count(&self.base_cost_a)
        {
            return false;
        }
        match &self.cost_b {
            Some(cb) => b.item == cb.item && b.count >= cb.count,
            None => b.is_empty(),
        }
    }

    /// `take(ItemStack, ItemStack)`: shrink both slots by what the offer
    /// costs, or refuse. Returns whether it took.
    pub fn take(&self, a: &mut Stack, b: &mut Stack) -> bool {
        if !self.satisfied_by(a, b) {
            return false;
        }
        a.count -= self.cost_a().count;
        if let Some(cb) = self.cost_b() {
            if !cb.is_empty() {
                b.count -= cb.count;
            }
        }
        true
    }
}

/// `MerchantOffers`, an `ArrayList<MerchantOffer>` with one extra method.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MerchantOffers(pub Vec<MerchantOffer>);

impl MerchantOffers {
    #[must_use]
    pub fn new() -> Self {
        MerchantOffers(Vec::new())
    }

    pub fn push(&mut self, o: MerchantOffer) {
        self.0.push(o);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `getRecipeFor(ItemStack, ItemStack, int index)`, offsets 0..80.
    ///
    /// The `index` branch is the one a `select_trade` packet lands in, and it
    /// is **not** a fallback: when `index > 0` and in range, the answer is
    /// that offer or nothing at all — the scan never runs. `index == 0` falls
    /// through to the scan, so slot 0 behaves like "no selection". Reading
    /// this as "try the index, then search" would let a client that selected
    /// an unsatisfiable trade silently execute a different one.
    #[must_use]
    pub fn recipe_for(&self, a: &Stack, b: &Stack, index: i32) -> Option<&MerchantOffer> {
        if index > 0 && (index as usize) < self.0.len() {
            let o = &self.0[index as usize];
            return if o.satisfied_by(a, b) { Some(o) } else { None };
        }
        self.0.iter().find(|o| o.satisfied_by(a, b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emeralds(n: i32) -> ItemCost {
        ItemCost::new("minecraft:emerald", n, 64)
    }

    fn offer() -> MerchantOffer {
        // farmer/1/wheat_emerald read backwards: 1 emerald for a book, so that
        // the price arithmetic has room to move in both directions.
        MerchantOffer::new(
            emeralds(5),
            None,
            Stack::new("minecraft:book", 1),
            16,
            2,
            0.05,
        )
    }

    /// The positive control for the whole price path: with demand and
    /// special price both zero the modified count is the base count, so any
    /// later assertion that a modifier moved it is measuring the modifier.
    #[test]
    fn an_untouched_offer_costs_its_base() {
        let o = offer();
        assert_eq!(o.cost_a(), Stack::new("minecraft:emerald", 5));
        assert_eq!(o.demand(), 0);
        assert_eq!(o.special_price_diff(), 0);
    }

    #[test]
    fn demand_raises_the_price_through_a_float_floor() {
        let mut o = offer();
        // 5 * 4 = 20, * 0.05 = 1.0 exactly -> +1.
        o.set_demand_for_test(4);
        assert_eq!(o.cost_a().count, 6);
        // 5 * 3 = 15, * 0.05 = 0.75 -> floor 0 -> no change. A per-factor
        // round would have made this 1.
        o.set_demand_for_test(3);
        assert_eq!(o.cost_a().count, 5);
    }

    /// The `max(0, ..)` is on the demand term only, so negative demand cannot
    /// lower the price but a negative `specialPriceDiff` can.
    #[test]
    fn negative_demand_is_floored_but_a_discount_is_not() {
        let mut o = offer();
        o.set_demand_for_test(-40);
        assert_eq!(o.cost_a().count, 5, "negative demand must not discount");
        o.add_to_special_price_diff(-3);
        assert_eq!(o.cost_a().count, 2);
        // And the outer clamp's floor is 1, not 0: no trade is ever free.
        o.add_to_special_price_diff(-100);
        assert_eq!(o.cost_a().count, 1);
    }

    /// The upper clamp is the cost item's max stack size, so a bucket-costed
    /// trade caps at 1 where an emerald-costed one caps at 64. Passing 64 by
    /// default would put 16 buckets in a slot that holds one.
    #[test]
    fn the_upper_clamp_is_the_cost_items_stack_size() {
        let mut o = MerchantOffer::new(
            ItemCost::new("minecraft:water_bucket", 1, 1),
            None,
            Stack::new("minecraft:emerald", 1),
            4,
            1,
            0.2,
        );
        o.add_to_special_price_diff(10);
        assert_eq!(o.cost_a().count, 1);
        let mut e = offer();
        e.add_to_special_price_diff(1000);
        assert_eq!(e.cost_a().count, 64);
    }

    #[test]
    fn update_demand_is_uses_minus_remaining() {
        let mut o = offer(); // maxUses 16
        o.update_demand();
        assert_eq!(o.demand(), -16, "an untouched offer drives demand down");
        let mut p = offer();
        for _ in 0..16 {
            p.increase_uses();
        }
        p.update_demand();
        assert_eq!(p.demand(), 16, "a fully-used offer drives it up by maxUses");
        let mut q = offer();
        for _ in 0..8 {
            q.increase_uses();
        }
        q.update_demand();
        assert_eq!(q.demand(), 0, "half used is neutral");
    }

    #[test]
    fn stock_counters_are_not_each_others_negation() {
        let mut o = offer();
        assert!(!o.is_out_of_stock());
        assert!(!o.needs_restock());
        o.increase_uses();
        assert!(!o.is_out_of_stock(), "1 of 16 is still in stock");
        assert!(o.needs_restock(), "but it does need restocking");
        o.set_to_out_of_stock();
        assert!(o.is_out_of_stock());
        assert_eq!(o.uses(), 16);
        o.reset_uses();
        assert!(!o.needs_restock());
    }

    #[test]
    fn satisfied_by_reads_the_modified_price_and_an_empty_second_slot() {
        let mut o = offer();
        let empty = Stack::new("", 0);
        assert!(o.satisfied_by(&Stack::new("minecraft:emerald", 5), &empty));
        assert!(!o.satisfied_by(&Stack::new("minecraft:emerald", 4), &empty));
        assert!(
            !o.satisfied_by(
                &Stack::new("minecraft:emerald", 5),
                &Stack::new("minecraft:book", 1)
            ),
            "a one-cost trade refuses anything in the second slot"
        );
        assert!(!o.satisfied_by(&Stack::new("minecraft:diamond", 64), &empty));
        // The price moving must move the answer, or `satisfied_by` is reading
        // the base count and the whole modifier path is untested.
        o.add_to_special_price_diff(1);
        assert!(!o.satisfied_by(&Stack::new("minecraft:emerald", 5), &empty));
        assert!(o.satisfied_by(&Stack::new("minecraft:emerald", 6), &empty));
    }

    #[test]
    fn take_shrinks_by_the_modified_cost_and_refuses_otherwise() {
        let mut o = offer();
        o.add_to_special_price_diff(2);
        let mut a = Stack::new("minecraft:emerald", 10);
        let mut b = Stack::new("", 0);
        assert!(o.take(&mut a, &mut b));
        assert_eq!(a.count, 3, "10 - (5 + 2)");
        let mut short = Stack::new("minecraft:emerald", 1);
        assert!(!o.take(&mut short, &mut b));
        assert_eq!(short.count, 1, "a refused take must not shrink anything");
    }

    #[test]
    fn two_cost_offers_take_from_both_slots() {
        let o = MerchantOffer::new(
            ItemCost::new("minecraft:emerald", 1, 64),
            Some(ItemCost::new("minecraft:book", 1, 64)),
            Stack::new("minecraft:enchanted_book", 1),
            12,
            10,
            0.2,
        );
        let mut a = Stack::new("minecraft:emerald", 3);
        let mut b = Stack::new("minecraft:book", 2);
        assert!(o.take(&mut a, &mut b));
        assert_eq!((a.count, b.count), (2, 1));
        assert!(!o.satisfied_by(&Stack::new("minecraft:emerald", 3), &Stack::new("", 0)));
    }

    #[test]
    fn recipe_for_index_does_not_fall_back_to_the_scan() {
        let mut offers = MerchantOffers::new();
        offers.push(MerchantOffer::new(
            emeralds(1),
            None,
            Stack::new("minecraft:bread", 6),
            16,
            1,
            0.05,
        ));
        offers.push(MerchantOffer::new(
            ItemCost::new("minecraft:coal", 15, 64),
            None,
            Stack::new("minecraft:emerald", 1),
            16,
            1,
            0.05,
        ));
        let a = Stack::new("minecraft:emerald", 1);
        let empty = Stack::new("", 0);
        // Index 1 names the coal trade, which an emerald does not satisfy;
        // the emerald trade at index 0 must NOT be returned instead.
        assert!(offers.recipe_for(&a, &empty, 1).is_none());
        // Index 0 is "no selection" and falls through to the scan.
        assert_eq!(
            offers
                .recipe_for(&a, &empty, 0)
                .map(|o| o.result().item.clone()),
            Some("minecraft:bread".to_string())
        );
        // Out of range does the same.
        assert!(offers.recipe_for(&a, &empty, 9).is_some());
    }

    impl MerchantOffer {
        /// Demand has no public setter in vanilla — it is only ever moved by
        /// `updateDemand` — so the tests that need a specific value set it
        /// here rather than by driving twelve restocks.
        fn set_demand_for_test(&mut self, d: i32) {
            self.demand = d;
        }
    }
}
