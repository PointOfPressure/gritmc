//! The saved form of a merchant's offers and of a villager's trading state.
//!
//! # `MerchantOffer.CODEC`, offsets 4..217 of `lambda$static$0`
//!
//! ```text
//! buy              ItemCost.CODEC          fieldOf                required
//! buyB             ItemCost.CODEC          lenientOptionalFieldOf  absent
//! sell             ItemStack.CODEC         fieldOf                required
//! uses             int                     lenientOptionalFieldOf  0
//! maxUses          int                     lenientOptionalFieldOf  4
//! rewardExp        boolean                 lenientOptionalFieldOf  true
//! specialPrice     int                     lenientOptionalFieldOf  0
//! demand           int                     lenientOptionalFieldOf  0
//! priceMultiplier  float                   lenientOptionalFieldOf  0.0f
//! xp               int                     lenientOptionalFieldOf  1
//! ```
//!
//! **The key is `specialPrice`, not `specialPriceDiff`**, and the wire and the
//! save disagree about which fields are optional — `writeToStream` sends all
//! ten unconditionally while the codec omits any that equal their default. A
//! save written from the wire's field list would name a tag that does not
//! exist.
//!
//! **`maxUses` defaults to 4 here and `priceMultiplier` to 0.0f.** The 4 is
//! the same number `VillagerTrade.CODEC` defaults `max_uses` to and it is not
//! a coincidence, but the 0.0f is: a listing with no `reputation_discount`
//! also gets 0.0f, so a restored offer with no `priceMultiplier` tag is
//! demand-proof rather than free — which is the safe direction, and worth
//! knowing before trusting a hand-written offer NBT.
//!
//! `MerchantOffers.CODEC` is
//! `MerchantOffer.CODEC.listOf().optionalFieldOf("Recipes", List.of())`
//! xmapped, so the list lives one level down under **`Recipes`**, and
//! `AbstractVillager.addAdditionalSaveData` stores that whole object under
//! **`Offers`**. Two nested names, neither of them the field's own.
//!
//! # `Villager.addAdditionalSaveData` / `readAdditionalSaveData`
//!
//! ```text
//! VillagerData                 VillagerData.CODEC
//! VillagerDataFinalized        boolean
//! FoodLevel                    byte
//! Gossips                      GossipContainer.CODEC
//! Xp                           int
//! LastRestock                  long
//! LastGossipDecay              long
//! RestocksToday                int
//! AssignProfessionWhenSpawned  boolean
//! ```
//!
//! **`lastRestockCheckDay` IS NOT SAVED.** The whole class holds exactly two
//! restock-shaped string constants, `LastRestock` and `RestocksToday`; the
//! field is written and read only inside `shouldRestock` itself (offsets 75,
//! 86 and 104). So it comes back as 0 after every world load, the
//! `lastRestockCheckDay > 0` guard is false on the first check after a load,
//! and a villager cannot be handed a day-rollover restock for a day it spent
//! unloaded — it has to earn one from the 12000-tick term instead. Persisting
//! it, which is the obvious "fix", would change behaviour on every chunk
//! reload.
//!
//! # This is a codec, not an NBT writer
//!
//! Mojang's `Codec` is ops-generic and is run against `NbtOps` on disk and
//! `JsonOps` in a data pack. The functions below are written against
//! `serde_json::Value` for the same reason: what is being ported is the field
//! names, the optionality and the defaults, and this crate has no NBT layer to
//! bind to. A caller with one binds the same map.

use serde_json::{json, Map, Value};

use super::offer::{ItemCost, MerchantOffer, MerchantOffers, Stack};
use super::restock::RestockState;

/// `ItemCost.CODEC`'s two fields this crate can carry. `count` defaults to 1
/// and `components` is not interpreted here.
fn write_item_cost(c: &ItemCost) -> Value {
    let mut m = Map::new();
    m.insert("id".into(), json!(c.item));
    if c.count != 1 {
        m.insert("count".into(), json!(c.count));
    }
    Value::Object(m)
}

fn read_item_cost(v: &Value, max_stack_size: i32) -> Option<ItemCost> {
    let o = v.as_object()?;
    Some(ItemCost {
        item: o.get("id")?.as_str()?.to_string(),
        count: o.get("count").and_then(Value::as_i64).unwrap_or(1) as i32,
        max_stack_size,
    })
}

/// `MerchantOffer.CODEC` encode. A field equal to its default is **omitted**,
/// which is what `lenientOptionalFieldOf` does on the way out and is why a
/// vanilla `Offers` tag is far smaller than ten fields per offer.
#[must_use]
pub fn write_offer(o: &MerchantOffer) -> Value {
    let mut m = Map::new();
    m.insert("buy".into(), write_item_cost(o.base_cost_a()));
    if let Some(b) = o.item_cost_b() {
        m.insert("buyB".into(), write_item_cost(b));
    }
    m.insert(
        "sell".into(),
        json!({"id": o.result().item, "count": o.result().count}),
    );
    if o.uses() != 0 {
        m.insert("uses".into(), json!(o.uses()));
    }
    if o.max_uses() != 4 {
        m.insert("maxUses".into(), json!(o.max_uses()));
    }
    if !o.should_reward_exp() {
        m.insert("rewardExp".into(), json!(false));
    }
    if o.special_price_diff() != 0 {
        m.insert("specialPrice".into(), json!(o.special_price_diff()));
    }
    if o.demand() != 0 {
        m.insert("demand".into(), json!(o.demand()));
    }
    if o.price_multiplier() != 0.0 {
        m.insert("priceMultiplier".into(), json!(o.price_multiplier()));
    }
    if o.xp() != 1 {
        m.insert("xp".into(), json!(o.xp()));
    }
    Value::Object(m)
}

/// `MerchantOffer.CODEC` decode, with every default applied.
///
/// `max_stack_size` is the caller's because it is a property of the item and
/// not of the saved offer; vanilla recovers it from the registry when
/// `ItemCost` builds its stack.
#[must_use]
pub fn read_offer(v: &Value, max_stack_size: i32) -> Option<MerchantOffer> {
    let o = v.as_object()?;
    let buy = read_item_cost(o.get("buy")?, max_stack_size)?;
    let buy_b = o
        .get("buyB")
        .and_then(|b| read_item_cost(b, max_stack_size));
    let sell = o.get("sell")?.as_object()?;
    let result = Stack::new(
        sell.get("id")?.as_str()?,
        sell.get("count").and_then(Value::as_i64).unwrap_or(1) as i32,
    );
    let int = |k: &str, d: i64| o.get(k).and_then(Value::as_i64).unwrap_or(d) as i32;
    Some(MerchantOffer::with_state(
        buy,
        buy_b,
        result,
        int("uses", 0),
        int("maxUses", 4),
        o.get("rewardExp").and_then(Value::as_bool).unwrap_or(true),
        int("specialPrice", 0),
        int("demand", 0),
        o.get("priceMultiplier")
            .and_then(Value::as_f64)
            .unwrap_or(0.0) as f32,
        int("xp", 1),
    ))
}

/// `AbstractVillager`'s `Offers` tag: `{"Recipes": [...]}`.
#[must_use]
pub fn write_offers(offers: &MerchantOffers) -> Value {
    json!({ "Recipes": offers.0.iter().map(write_offer).collect::<Vec<_>>() })
}

/// The other direction. An absent `Recipes` is an empty list, not an error —
/// `optionalFieldOf("Recipes", List.of())`.
#[must_use]
pub fn read_offers(v: &Value, max_stack_size: i32) -> MerchantOffers {
    let mut out = MerchantOffers::new();
    if let Some(list) = v.get("Recipes").and_then(Value::as_array) {
        for e in list {
            if let Some(o) = read_offer(e, max_stack_size) {
                out.push(o);
            }
        }
    }
    out
}

/// The restock fields `Villager.addAdditionalSaveData` writes — and only
/// those. See the module header for why `lastRestockCheckDay` is not here.
#[must_use]
pub fn write_restock(st: &RestockState, villager_xp: i32) -> Value {
    json!({
        "Xp": villager_xp,
        "LastRestock": st.last_restock_game_time,
        "RestocksToday": st.number_of_restocks_today,
    })
}

/// Reads them back. `lastRestockCheckDay` comes back **0** whatever the tag
/// holds, because vanilla never wrote it.
#[must_use]
pub fn read_restock(v: &Value) -> (RestockState, i32) {
    (
        RestockState {
            last_restock_game_time: v.get("LastRestock").and_then(Value::as_i64).unwrap_or(0),
            number_of_restocks_today: v.get("RestocksToday").and_then(Value::as_i64).unwrap_or(0)
                as i32,
            last_restock_check_day: 0,
        },
        v.get("Xp").and_then(Value::as_i64).unwrap_or(0) as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer() -> MerchantOffer {
        MerchantOffer::new(
            ItemCost::new("minecraft:emerald", 5, 64),
            None,
            Stack::new("minecraft:book", 1),
            16,
            2,
            0.05,
        )
    }

    #[test]
    fn an_offer_round_trips_through_the_save_form() {
        let mut o = offer();
        o.increase_uses();
        o.add_to_special_price_diff(-3);
        o.update_demand();
        let v = write_offer(&o);
        assert_eq!(read_offer(&v, 64), Some(o));
    }

    /// Fields equal to their defaults are omitted, so a default-shaped offer
    /// is three tags and not ten. `maxUses` 4, `xp` 1, `priceMultiplier` 0.0,
    /// `rewardExp` true, everything else 0.
    #[test]
    fn a_default_shaped_offer_writes_three_tags() {
        let plain = MerchantOffer::new(
            ItemCost::new("minecraft:emerald", 1, 64),
            None,
            Stack::new("minecraft:bread", 6),
            4,
            1,
            0.0,
        );
        let v = write_offer(&plain);
        let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
        assert_eq!(keys, vec!["buy", "sell"], "{v}");
        assert_eq!(read_offer(&v, 64), Some(plain));
    }

    /// The key is `specialPrice`. A save written from the wire's field names
    /// would put `specialPriceDiff` there and it would read back as 0 — a
    /// silently reset discount rather than a load failure.
    #[test]
    fn the_special_price_key_is_not_the_field_name() {
        let mut o = offer();
        o.add_to_special_price_diff(-4);
        let v = write_offer(&o);
        assert!(v.get("specialPrice").is_some());
        assert!(v.get("specialPriceDiff").is_none());
        let wrong = json!({"buy": {"id": "minecraft:emerald", "count": 5},
                           "sell": {"id": "minecraft:book", "count": 1},
                           "specialPriceDiff": -4});
        assert_eq!(read_offer(&wrong, 64).unwrap().special_price_diff(), 0);
    }

    /// Every default applied on the way in, from a tag carrying only the two
    /// required fields.
    #[test]
    fn a_minimal_tag_gets_every_default() {
        let v = json!({"buy": {"id": "minecraft:emerald"},
                       "sell": {"id": "minecraft:bread", "count": 6}});
        let o = read_offer(&v, 64).unwrap();
        assert_eq!(o.base_cost_a().count, 1, "ItemCost count defaults to 1");
        assert_eq!(o.uses(), 0);
        assert_eq!(o.max_uses(), 4);
        assert!(o.should_reward_exp());
        assert_eq!(o.special_price_diff(), 0);
        assert_eq!(o.demand(), 0);
        assert_eq!(o.price_multiplier(), 0.0);
        assert_eq!(o.xp(), 1);
        assert!(!o.is_out_of_stock(), "0 of 4");
    }

    /// The list is two names deep and neither is the field's.
    #[test]
    fn the_offer_list_lives_under_offers_then_recipes() {
        let mut offers = MerchantOffers::new();
        offers.push(offer());
        let v = write_offers(&offers);
        assert!(v.get("Recipes").is_some());
        assert_eq!(v.get("Recipes").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(read_offers(&v, 64).len(), 1);
        // An absent list is empty rather than an error.
        assert!(read_offers(&json!({}), 64).is_empty());
    }

    /// `lastRestockCheckDay` is never written, so it comes back 0 and the day
    /// term is disarmed for the first check after a load. Persisting it would
    /// change behaviour on every chunk reload, which is why this asserts the
    /// absence rather than the field.
    #[test]
    fn the_restock_check_day_does_not_survive_a_save() {
        let st = RestockState {
            last_restock_game_time: 123_456,
            number_of_restocks_today: 2,
            last_restock_check_day: 77,
        };
        let v = write_restock(&st, 40);
        assert!(v.get("LastRestockCheckDay").is_none());
        let (back, xp) = read_restock(&v);
        assert_eq!(xp, 40);
        assert_eq!(back.last_restock_game_time, 123_456);
        assert_eq!(back.number_of_restocks_today, 2);
        assert_eq!(back.last_restock_check_day, 0, "not saved, so not restored");

        // And the consequence, driven rather than asserted: with the day term
        // disarmed, a villager loaded on a later day gets no rollover restock
        // from the day comparison on its first check.
        let mut loaded = back;
        let mut offers = MerchantOffers::new();
        offers.push(offer());
        offers.0[0].increase_uses();
        let (_, e) = loaded.should_restock(&mut offers, 123_457, 9_999);
        assert!(!e.day_rolled, "a fresh load must not roll on day 9999");
        // The second check does roll, because the first one recorded the day.
        let (_, e2) = loaded.should_restock(&mut offers, 123_458, 10_000);
        assert!(e2.day_rolled);
    }
}
