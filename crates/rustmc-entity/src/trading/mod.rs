//! Villager and wandering-trader trading: the listings, the offers they
//! become, and how those offers change.
//!
//! # What is here and what is deliberately elsewhere
//!
//! * [`tables`] — the two generated extracts: 388 listings, 68 trade sets,
//!   73 pools, and the profession -> trade-set map.
//! * [`offer`] — `MerchantOffer` and `MerchantOffers`: the price arithmetic,
//!   the stock counters and the `satisfiedBy`/`take` pair.
//!
//! **The merchant MENU is not here.** Opening a trading screen is
//! `ServerPlayer.openMenu(villager)` followed by a `merchant_offers` packet
//! on the same container id, and `MerchantMenu`/`MerchantContainer`/
//! `MerchantResultSlot` — the slots, the click handling and the execution of
//! a trade — belong to the container-menu lane. This module owns the offers
//! and their wire form; the join is `ServerPlayer.openMenu` with the
//! `merchant_offers` follow-up.
//!
//! **Gossip is not here either.** `MerchantOffer.getModifiedCostCount` takes
//! `specialPriceDiff` as an *input*, and the thing that writes it is
//! `Villager.updateSpecialPrices` reading `GossipContainer.getReputation` /
//! `getPlayerReputation`. Those are a separate area; this module ports the
//! arithmetic that consumes the number and refuses to invent its source.

pub mod complete;
pub mod create;
pub mod draw;
pub mod offer;
pub mod restock;
pub mod save;
pub mod tables;
pub mod wire;
