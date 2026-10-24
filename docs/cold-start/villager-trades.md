# COLD START — branch `villager-trades`

### COLD START — branch `villager-trades`, 2026-09-05

**The brief's premise was refuted by the first `unzip`.** It described
`VillagerTrades.TRADES` as an `Int2ObjectMap<Int2ObjectMap<ItemListing[]>>`
built in a static initialiser out of `EmeraldForItems`, `ItemsForEmeralds`,
`EnchantedItemForEmeralds`, `TreasureMapForEmeralds`, `DyedArmorForEmeralds`,
`SuspiciousStewForEmerald`, `TippedArrowForItemsAndEmeralds`,
`EnchantBookForEmeralds`, `ItemsAndEmeraldsToItems` and `TypeSpecificTrade`.
**None of those classes are in the 26.2 jar** —
`unzip -l server-26.2.jar | grep -c ItemListing` is 0 — and
`javap -p net/minecraft/world/item/trading/VillagerTrades` is a class whose
every field is a `ResourceKey<VillagerTrade>` and whose only real method is
`bootstrap(BootstrapContext)`.

Two other briefed facts were also wrong about main, and both are cheap to
re-check before believing them again:
`crates/rustmc-entity/src/brain/behaviors/show_trades_to_player.rs` **is not on
main** (the behaviours directory has 30 files and that is not one of them), and
`crates/rustmc-entity/src/host.rs` carries **no `MerchantOffer` surface** — its
only trade-adjacent lines are `offer_flower` and a llama predicate.
`trade_with_player_goal.rs` and `golden/villager_schedule.json` are there as
described.

## What a trade is in 26.2

Two datapack registries and a tag over one of them:

| thing | count | where |
|---|---|---|
| `minecraft:villager_trade` (`VillagerTrade.CODEC`) | 388 | `data/minecraft/villager_trade/**.json` |
| `minecraft:trade_set` (`TradeSet.CODEC`) | 68 | `data/minecraft/trade_set/**.json` |
| pools (tags over the first) | 73 | `data/minecraft/tags/villager_trade/**.json` |

68 = 13 professions x 5 levels + the wandering trader's three. 388 = 291
profession listings + 97 wandering-trader ones. The only piece still written in
Java is the profession -> trade-set map, `VillagerProfession.tradeSetsByLevel`,
reached through `getTrades(int)`.

## What landed

`ci/villager-trade-golden.sh` + `tools/goldengen/vanilla/VillagerTradeGolden.java`
write `data/vanilla/villager_trades.json` and
`data/vanilla/wandering_trader_trades.json` in one JVM run, arms in
`ci/gen-gameplay-tables.sh`, `ci/check-generator-arms.sh` green at 30/30.

**The cross-check is a second source, not a second read.** The same 529 files
exist packed in the jar and under `oracle-work/vanilla-26.2/generated-data/`,
which Mojang's own data generator wrote; the dumper reads both independently
and refuses on any file present in one and not the other or parsing to a
different tree. 0 mismatches over 529. It then runs twice and `cmp`s.

`crates/rustmc-entity/src/trading/`:

* `tables.rs` — the two extracts, with the codec defaults written in
  explicitly (`max_uses` 4.0, `xp` 1.0, `reputation_discount` 0.0,
  `given_item_modifiers` [], `allow_duplicates` false) and a
  `defaultedFields` list so a defaulted 4 is distinguishable from a written 4.
* `offer.rs` — `MerchantOffer`, `MerchantOffers`, the price arithmetic.
* `create.rs` — `VillagerTrade.getOffer`.
* `draw.rs` — the two draw loops, `RandomSequences`, the level thresholds.
* `restock.rs` — the five-method restock state machine.
* `complete.rs` — `notifyTrade` and both `rewardTradeXp`s.
* `wire.rs` — `merchant_offers` (cb 52) and `select_trade` (sb 51).

308 unit tests and 12 integration tests, `cargo fmt --check` and
`clippy --all-targets -D warnings` clean.

## The findings, one line each

Eleven are in `docs/FINDINGS.md` dated 2026-09-05. The three worth reading
before touching anything here:

1. **Eight trade sets are over-subscribed** and the without-duplicates loop's
   second guard is `list.isEmpty()`, not `i < count`, so those villagers offer
   fewer trades than `amount` says and nothing raises.
2. **The RNG is a saved per-sequence stream**, not a per-villager seed:
   `RandomSequences.get` is a `computeIfAbsent` over a saved map, so two
   farmers of the same level do not get the same trades.
3. **`getModifiedPrice` does not exist**; it is `getModifiedCostCount`, and
   its `max(0, ..)` wraps the demand term only.

## Refused, with the class names

* **The merchant menu.** `MerchantMenu`, `MerchantContainer`,
  `MerchantResultSlot` and the click that executes a trade are the
  container-menu lane's. The join is `Merchant.openTradingScreen` ->
  `Player.openMenu(new SimpleMenuProvider(MerchantMenu::new, title))`, and
  **only if that returns a container id and the offer list is non-empty** does
  it call `sendMerchantOffers(id, offers, level, xp, showProgressBar(),
  canRestock())`. A merchant with no offers opens a screen and sends no
  packet.
* **Gossip.** `MerchantOffer.getModifiedCostCount` takes `specialPriceDiff` as
  an input and this branch ports the arithmetic that consumes it;
  `Villager.updateSpecialPrices` reading `GossipContainer.getReputation` /
  `getPlayerReputation` is what writes it, and is not ported.
* **The 61 listings that need another subsystem.** 27 carry a
  `minecraft:entity_properties` merchant predicate; 41 carry loot functions.
  They reach the draw loop as `OfferOutcome::Unported` naming the function,
  never as a null offer — a `test_merchant_predicate` answering `false`
  because there is no entity system would delete 27 listings and read as a
  correct port.

## Registered nothing false — the null that matters

**There is no capture of `merchant_offers` anywhere on this box.**
`grep -l merchant_offers ~/rustmc*/oracle-work/gameplay/*.json
/home/sayer/grit/*/oracle-work/gameplay/*.json` matches nothing across **107**
gameplay recording directories. So:

* the wire form's byte assertions are against the bytecode's field order and
  widths and nothing else;
* **no golden of "the offers a seeded level-1 farmer draws" was written.** It
  could only be checked against this implementation, which is the
  measures-its-own-input defect; there is nothing to compare it to and none
  was manufactured. When a corpus scenario drives a villager, that recording
  is the check this branch is missing, and it is the single next item.

## The single next item

**A gameplay scenario that opens a villager's trading screen**, so that
`merchant_offers` exists in a recording. It closes three things at once: the
packet's byte layout, the draw's listing ids under a known world seed, and
whether `RandomSequences` really continues across villagers. Everything else
here is derived from bytecode and unit-tested; that recording is the only
thing that can turn any of it into a parity figure.

Second, in order: wire `trading::wire::StackWire` up in `rustmc-server` off
the existing `ItemStack` codec, once the container-menu lane's
`ServerPlayer.openMenu` is landed — it is a new file
(`crates/rustmc-server/src/merchant.rs`), not an edit to `menu.rs`.
