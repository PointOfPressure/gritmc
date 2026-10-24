//! `VillagerTrade.getOffer(LootContext)` — one listing becoming one offer.
//!
//! # The whole method, offsets 0..313
//!
//! ```text
//!    0: if merchantPredicate.isPresent() && !merchantPredicate.test(ctx): return null
//!   31: stack = gives.create()
//!   39: extra = 0
//!   41: for f in givenItemModifiers:
//!   74:     stack = f.apply(stack, ctx)
//!   87:     if stack.isEmpty(): return null
//!   99: Integer c = stack.remove(DataComponents.ADDITIONAL_TRADE_COST)
//!  111: if c != null: extra += c
//!  124: if doubleTradePriceEnchantments.isPresent():
//!  146:     ItemEnchantments se = stack.get(DataComponents.STORED_ENCHANTMENTS)
//!  158:     if se != null && se.keySet().anyMatch(set::contains): extra *= 2
//!  197: ItemCost a = wants.toItemCost(ctx, extra)
//!  208: if a.count() < 1: return null
//!  219: Optional<ItemCost> b = additionalWants.map(w -> w.toItemCost(ctx, 0))
//!  234: if b.isPresent() && b.get().count() < 1: return null
//!  259: return new MerchantOffer(a, b, stack,
//!           Math.max(maxUses.getInt(ctx), 1),
//!           Math.max(xp.getInt(ctx), 0),
//!           Math.max(reputationDiscount.getFloat(ctx), 0.0f))
//! ```
//!
//! # `ADDITIONAL_TRADE_COST` is where an enchanted book's price comes from
//!
//! `librarian/1/emerald_and_book_enchanted_book` has
//! `"wants": {"count": 0.0, "id": "minecraft:emerald"}` — **zero emeralds**.
//! The whole price is the `ADDITIONAL_TRADE_COST` component that
//! `enchant_randomly` writes with `include_additional_cost_component: true`,
//! `remove`d off the stack here and passed to `toItemCost` as `extra`. Read
//! the listing without this path and every enchanted book is free; read it
//! and stop at `remove` without the doubling and every book from
//! `#minecraft:double_trade_price` is half price.
//!
//! `remove` — not `get`. The component does not survive onto the traded item;
//! it exists only to carry a number from the modifier to the cost.
//!
//! # The three `Math.max` clamps are not the same clamp
//!
//! `maxUses` floors at **1**, `xp` at **0**, `reputationDiscount` at
//! **0.0f**. A shared floor of 0 would give a permanently out-of-stock trade
//! (`isOutOfStock` is `uses >= maxUses`, and `0 >= 0`); a shared floor of 1
//! would give every trade a point of experience.
//!
//! # What this server can and cannot do today
//!
//! 327 of the 388 listings carry neither a `merchant_predicate` nor a
//! `given_item_modifiers` list, and this function produces their offer
//! exactly. The other 61 need one of two subsystems and are **refused by
//! name** rather than approximated:
//!
//! * 27 carry a `minecraft:entity_properties` predicate restricting the trade
//!   to particular villager types — an entity-predicate evaluation this crate
//!   has no context for;
//! * 41 carry loot functions: `enchant_with_levels` (17), `filtered` (38),
//!   `exploration_map` (10), `set_name` (10), `set_random_dyes` (7),
//!   `enchant_randomly` (4), `set_stew_effect` (1), `set_potion` (1),
//!   `set_random_potion` (1). Those are `rustmc-loot`'s and they **draw**, so
//!   rolling one here and recording it would be recording a sample.
//!
//! An unevaluable listing must not come back as a null offer. Vanilla would
//! have produced a trade and we cannot say which, so it reaches the draw loop
//! as [`OfferOutcome::Unported`] and the caller decides.

use serde_json::Value;

use super::draw::{OfferOutcome, Refusal};
use super::offer::{ItemCost, MerchantOffer, Stack};
use super::tables::Listing;

/// What applying `given_item_modifiers` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modified {
    /// The stack after every function, or `None` if one of them emptied it —
    /// which is `getOffer`'s offsets 87..95 and the reason a `filtered` with
    /// an `on_fail` of `discard` drops the listing.
    pub stack: Option<Stack>,
    /// The `ADDITIONAL_TRADE_COST` component's value, already removed from
    /// the stack.
    pub additional_trade_cost: i32,
    /// Whether the stack's `STORED_ENCHANTMENTS` intersect the listing's
    /// `double_trade_price_enchantments`. Meaningless when the listing has
    /// none.
    pub doubles_price: bool,
}

/// Everything `getOffer` needs that is not in the listing.
///
/// Every hook that this server cannot answer returns `None`, and `None` means
/// **unported**, never false. A `test_merchant_predicate` that answered
/// `false` because there is no entity system would silently delete 27
/// listings and read as a correct port — the same shape as the datapack grep
/// that "proved" bearding inert.
pub trait OfferEnv {
    /// `Item.getDefaultMaxStackSize()`, needed by `TradeCost.toItemCost`'s
    /// clamp and by `getModifiedCostCount`'s.
    fn default_max_stack_size(&self, item: &str) -> Option<i32>;

    /// `LootItemCondition.test(ctx)`.
    fn test_merchant_predicate(&mut self, _condition: &Value) -> Option<bool> {
        None
    }

    /// The `given_item_modifiers` loop, the `ADDITIONAL_TRADE_COST` removal
    /// and the `STORED_ENCHANTMENTS` intersection, together — because they
    /// are one pass over one stack and splitting them would let a caller
    /// apply the functions and forget the component.
    fn apply_given_item_modifiers(
        &mut self,
        _stack: Stack,
        _modifiers: &[Value],
        _double_price_enchantments: Option<&Value>,
    ) -> Option<Modified> {
        None
    }
}

/// `VillagerTrade.getOffer(LootContext)`.
pub fn get_offer<E: OfferEnv>(listing: &Listing, env: &mut E) -> OfferOutcome<MerchantOffer> {
    if let Some(cond) = &listing.merchant_predicate {
        match env.test_merchant_predicate(cond) {
            Some(true) => {}
            Some(false) => return OfferOutcome::None(Refusal::MerchantPredicate),
            None => {
                return OfferOutcome::Unported(format!(
                    "merchant_predicate {}",
                    condition_name(cond)
                ))
            }
        }
    }

    let base = Stack::new(listing.gives.item.clone(), listing.gives.count);
    let (stack, extra) = if listing.given_item_modifiers.is_empty()
        && listing.double_trade_price_enchantments.is_none()
    {
        // No functions and no doubling set: `remove(ADDITIONAL_TRADE_COST)` on
        // a freshly created stack is null, so `extra` stays 0 and the whole
        // 41..196 block is a no-op. This is the 327-listing path and it needs
        // no loot system at all.
        (base, 0)
    } else {
        let Some(m) = env.apply_given_item_modifiers(
            base,
            &listing.given_item_modifiers,
            listing.double_trade_price_enchantments.as_ref(),
        ) else {
            return OfferOutcome::Unported(unported_modifiers(listing));
        };
        let Some(s) = m.stack else {
            return OfferOutcome::None(Refusal::ModifierEmptiedResult);
        };
        let mut e = m.additional_trade_cost;
        if listing.double_trade_price_enchantments.is_some() && m.doubles_price {
            e *= 2;
        }
        (s, e)
    };

    // The `Number` refusals: a provider this server does not roll is not a
    // zero. A zero `wants.count` would be legal (the enchanted books have
    // one) and so cannot double as an error.
    let Some(want_count) = listing.wants.count.as_int() else {
        return OfferOutcome::Unported(format!("wants.count {:?}", listing.wants.count));
    };
    let Some(want_max) = env.default_max_stack_size(&listing.wants.item) else {
        return OfferOutcome::Unported(format!("no stack size for {}", listing.wants.item));
    };
    let a = ItemCost::from_trade_cost(listing.wants.item.clone(), want_count, extra, want_max);
    if a.count < 1 {
        return OfferOutcome::None(Refusal::WantsBelowOne);
    }

    let b = match &listing.additional_wants {
        None => None,
        Some(w) => {
            let Some(c) = w.count.as_int() else {
                return OfferOutcome::Unported(format!("additional_wants.count {:?}", w.count));
            };
            let Some(m) = env.default_max_stack_size(&w.item) else {
                return OfferOutcome::Unported(format!("no stack size for {}", w.item));
            };
            // `toItemCost(ctx, 0)` — the extra cost never reaches cost B.
            let cost = ItemCost::from_trade_cost(w.item.clone(), c, 0, m);
            if cost.count < 1 {
                return OfferOutcome::None(Refusal::AdditionalWantsBelowOne);
            }
            Some(cost)
        }
    };

    let (Some(max_uses), Some(xp), Some(discount)) = (
        listing.max_uses.as_int(),
        listing.xp.as_int(),
        listing.reputation_discount.as_float(),
    ) else {
        return OfferOutcome::Unported(format!(
            "non-constant provider on {} (max_uses {:?}, xp {:?}, reputation_discount {:?})",
            listing.id, listing.max_uses, listing.xp, listing.reputation_discount
        ));
    };

    OfferOutcome::Offer(MerchantOffer::new(
        a,
        b,
        stack,
        max_uses.max(1),
        xp.max(0),
        discount.max(0.0),
    ))
}

fn condition_name(c: &Value) -> String {
    c.get("condition")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>")
        .to_string()
}

fn unported_modifiers(listing: &Listing) -> String {
    let mut names: Vec<&str> = listing
        .given_item_modifiers
        .iter()
        .filter_map(|f| f.get("function").and_then(Value::as_str))
        .collect();
    if listing.double_trade_price_enchantments.is_some() {
        names.push("double_trade_price_enchantments");
    }
    format!("{}: {}", listing.id, names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trading::tables::{Number, StackTemplate, TradeCost};

    /// Every item is 64 and nothing else is answerable — the default
    /// environment a server with no loot system has.
    struct Bare;

    impl OfferEnv for Bare {
        fn default_max_stack_size(&self, _item: &str) -> Option<i32> {
            Some(64)
        }
    }

    fn listing(wants: i32, gives: i32) -> Listing {
        Listing {
            id: "minecraft:farmer/1/wheat_emerald".into(),
            wants: TradeCost {
                item: "minecraft:wheat".into(),
                count: Number::Constant(wants as f32),
                components: None,
            },
            additional_wants: None,
            gives: StackTemplate {
                item: "minecraft:emerald".into(),
                count: gives,
                components: None,
            },
            max_uses: Number::Constant(16.0),
            reputation_discount: Number::Constant(0.05),
            xp: Number::Constant(2.0),
            merchant_predicate: None,
            given_item_modifiers: Vec::new(),
            double_trade_price_enchantments: None,
            defaulted_fields: Vec::new(),
        }
    }

    /// The 327-listing path, end to end and with no environment at all.
    #[test]
    fn a_plain_listing_becomes_an_exact_offer() {
        let l = listing(20, 1);
        let OfferOutcome::Offer(o) = get_offer(&l, &mut Bare) else {
            panic!("refused a listing with no modifiers")
        };
        assert_eq!(o.cost_a(), Stack::new("minecraft:wheat", 20));
        assert_eq!(o.cost_b(), None);
        assert_eq!(*o.result(), Stack::new("minecraft:emerald", 1));
        assert_eq!(o.max_uses(), 16);
        assert_eq!(o.xp(), 2);
        assert!((o.price_multiplier() - 0.05).abs() < f32::EPSILON);
        assert_eq!(o.uses(), 0);
        assert_eq!(o.demand(), 0);
    }

    /// `wants.count` of 0 with no extra cost is a refusal, not a free trade.
    /// This is the enchanted-book shape with its modifier removed, and a port
    /// that skipped the `< 1` check would put a zero-cost trade on a villager.
    #[test]
    fn a_zero_cost_want_refuses() {
        let l = listing(0, 1);
        assert_eq!(
            get_offer(&l, &mut Bare),
            OfferOutcome::None(Refusal::WantsBelowOne)
        );
    }

    /// The `toItemCost` clamp is the cost item's stack size, so a listing
    /// wanting 40 of something that stacks to 16 wants 16.
    #[test]
    fn the_want_is_clamped_to_the_items_stack_size() {
        struct Small;
        impl OfferEnv for Small {
            fn default_max_stack_size(&self, _item: &str) -> Option<i32> {
                Some(16)
            }
        }
        let l = listing(40, 1);
        let OfferOutcome::Offer(o) = get_offer(&l, &mut Small) else {
            panic!()
        };
        assert_eq!(o.cost_a().count, 16);
    }

    /// The three floors are three different numbers.
    #[test]
    fn the_three_clamps_are_not_one_clamp() {
        let mut l = listing(1, 1);
        l.max_uses = Number::Constant(0.0);
        l.xp = Number::Constant(-5.0);
        l.reputation_discount = Number::Constant(-1.0);
        let OfferOutcome::Offer(o) = get_offer(&l, &mut Bare) else {
            panic!()
        };
        assert_eq!(o.max_uses(), 1, "maxUses floors at 1");
        assert_eq!(o.xp(), 0, "xp floors at 0, not 1");
        assert_eq!(o.price_multiplier(), 0.0);
        assert!(!o.is_out_of_stock(), "a maxUses of 0 would be out of stock");
    }

    /// A listing this server cannot evaluate must reach the caller as
    /// unported, naming what stopped it — never as "vanilla offers nothing
    /// here".
    #[test]
    fn an_unevaluable_listing_is_unported_and_names_the_function() {
        let mut l = listing(1, 1);
        l.given_item_modifiers =
            vec![serde_json::json!({"function": "minecraft:enchant_randomly"})];
        l.double_trade_price_enchantments =
            Some(serde_json::json!("#minecraft:double_trade_price"));
        match get_offer(&l, &mut Bare) {
            OfferOutcome::Unported(w) => {
                assert!(w.contains("enchant_randomly"), "{w}");
                assert!(w.contains("double_trade_price_enchantments"), "{w}");
            }
            other => panic!("{other:?}"),
        }

        let mut p = listing(1, 1);
        p.merchant_predicate =
            Some(serde_json::json!({"condition": "minecraft:entity_properties"}));
        match get_offer(&p, &mut Bare) {
            OfferOutcome::Unported(w) => assert!(w.contains("entity_properties"), "{w}"),
            other => panic!("{other:?}"),
        }
    }

    /// An environment that CAN answer produces the offer, and a predicate
    /// answering false is a genuine null rather than an unported one. Without
    /// this the refusal above is satisfied by an env that never answers.
    #[test]
    fn a_predicate_that_answers_is_obeyed_in_both_directions() {
        struct Says(bool);
        impl OfferEnv for Says {
            fn default_max_stack_size(&self, _i: &str) -> Option<i32> {
                Some(64)
            }
            fn test_merchant_predicate(&mut self, _c: &Value) -> Option<bool> {
                Some(self.0)
            }
        }
        let mut l = listing(1, 1);
        l.merchant_predicate =
            Some(serde_json::json!({"condition": "minecraft:entity_properties"}));
        assert!(matches!(
            get_offer(&l, &mut Says(true)),
            OfferOutcome::Offer(_)
        ));
        assert_eq!(
            get_offer(&l, &mut Says(false)),
            OfferOutcome::None(Refusal::MerchantPredicate)
        );
    }

    /// The enchanted-book path: a zero-emerald want, a modifier that writes
    /// `ADDITIONAL_TRADE_COST`, and a doubling set. The cost is the component
    /// alone, doubled when the rolled enchantment is in the set — so the same
    /// listing is 5 emeralds or 10 depending on the draw, which is exactly
    /// why the golden records the shape and not one roll.
    #[test]
    fn the_additional_trade_cost_component_is_the_whole_price() {
        struct Rolled {
            cost: i32,
            doubles: bool,
        }
        impl OfferEnv for Rolled {
            fn default_max_stack_size(&self, _i: &str) -> Option<i32> {
                Some(64)
            }
            fn apply_given_item_modifiers(
                &mut self,
                stack: Stack,
                _m: &[Value],
                _d: Option<&Value>,
            ) -> Option<Modified> {
                Some(Modified {
                    stack: Some(stack),
                    additional_trade_cost: self.cost,
                    doubles_price: self.doubles,
                })
            }
        }
        let mut l = listing(0, 1);
        l.wants.item = "minecraft:emerald".into();
        l.gives.item = "minecraft:enchanted_book".into();
        l.given_item_modifiers =
            vec![serde_json::json!({"function": "minecraft:enchant_randomly"})];
        l.double_trade_price_enchantments =
            Some(serde_json::json!("#minecraft:double_trade_price"));

        let OfferOutcome::Offer(cheap) = get_offer(
            &l,
            &mut Rolled {
                cost: 5,
                doubles: false,
            },
        ) else {
            panic!()
        };
        assert_eq!(cheap.cost_a().count, 5, "0 wanted + 5 of component");

        let OfferOutcome::Offer(dear) = get_offer(
            &l,
            &mut Rolled {
                cost: 5,
                doubles: true,
            },
        ) else {
            panic!()
        };
        assert_eq!(dear.cost_a().count, 10);

        // And without a doubling set on the listing, `doubles_price` is
        // ignored — the field is a property of the listing, not of the stack.
        l.double_trade_price_enchantments = None;
        let OfferOutcome::Offer(plain) = get_offer(
            &l,
            &mut Rolled {
                cost: 5,
                doubles: true,
            },
        ) else {
            panic!()
        };
        assert_eq!(plain.cost_a().count, 5);
    }

    /// A modifier that empties the stack — `filtered` with an `on_fail` of
    /// `discard`, which is 38 of the 41 modifier lists — is a null offer, not
    /// an unported one: vanilla itself drops the listing.
    #[test]
    fn a_modifier_that_empties_the_stack_is_a_genuine_null() {
        struct Discards;
        impl OfferEnv for Discards {
            fn default_max_stack_size(&self, _i: &str) -> Option<i32> {
                Some(64)
            }
            fn apply_given_item_modifiers(
                &mut self,
                _s: Stack,
                _m: &[Value],
                _d: Option<&Value>,
            ) -> Option<Modified> {
                Some(Modified {
                    stack: None,
                    additional_trade_cost: 0,
                    doubles_price: false,
                })
            }
        }
        let mut l = listing(1, 1);
        l.given_item_modifiers = vec![serde_json::json!({"function": "minecraft:filtered"})];
        assert_eq!(
            get_offer(&l, &mut Discards),
            OfferOutcome::None(Refusal::ModifierEmptiedResult)
        );
    }

    /// Cost B never gets the extra cost, and a cost B below 1 refuses the
    /// whole listing rather than dropping the second slot.
    #[test]
    fn cost_b_takes_no_extra_and_refuses_below_one() {
        let mut l = listing(1, 1);
        l.additional_wants = Some(TradeCost {
            item: "minecraft:book".into(),
            count: Number::Constant(1.0),
            components: None,
        });
        let OfferOutcome::Offer(o) = get_offer(&l, &mut Bare) else {
            panic!()
        };
        assert_eq!(o.cost_b(), Some(Stack::new("minecraft:book", 1)));

        l.additional_wants = Some(TradeCost {
            item: "minecraft:book".into(),
            count: Number::Constant(0.0),
            components: None,
        });
        assert_eq!(
            get_offer(&l, &mut Bare),
            OfferOutcome::None(Refusal::AdditionalWantsBelowOne)
        );
    }
}
