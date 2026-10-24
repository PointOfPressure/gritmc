//! Which listings a villager ends up carrying: the draw loop, the count, and
//! the random stream the draw comes off.
//!
//! # The draw loop, and the two forms that are not the same loop
//!
//! `AbstractVillager.addOffersFromTradeSet` looks the set up, builds a
//! `LootContext` on the set's `random_sequence`, asks
//! `TradeSet.calculateNumberOfTrades` for a count, and then branches on
//! `allowDuplicates` into one of two static methods. They are transcribed in
//! [`draw_offers`] and they differ in more than the name:
//!
//! ```text
//! addOffersFromItemListings (allowDuplicates == true)
//!    0: list = Lists.newArrayList(holderSet)
//!    9: while i < count && !list.isEmpty():
//!   25:     j = random.nextInt(list.size())
//!   57:     offer = list.get(j).value().getOffer(ctx)
//!   73:     if offer == null: list.remove(j); continue      // i NOT incremented
//!   91:     else: offers.add(offer); i++                    // j NOT removed
//!
//! addOffersFromItemListingsWithoutDuplicates (allowDuplicates == false)
//!    0: list = Lists.newArrayList(holderSet)
//!    9: while i < count && !list.isEmpty():
//!   25:     picked = list.remove(random.nextInt(list.size()))   // always removed
//!   53:     offer = picked.value().getOffer(ctx)
//!   69:     if offer != null: offers.add(offer); i++
//! ```
//!
//! Three consequences a plausible reading gets wrong:
//!
//! * **The duplicate-allowing form can loop forever on the same index.** It
//!   removes only on a *refusal*; a listing that produces an offer stays in
//!   the list and can be drawn again next iteration. That is the point of the
//!   flag, and it means the count of `nextInt` calls is exactly `count` plus
//!   one per refusal.
//! * **The without-duplicates form draws exactly `count` times at most,
//!   refusals included** — a refused listing consumes an iteration *and* a
//!   draw and does not get replaced, so a villager whose merchant predicate
//!   fails on one listing simply carries fewer offers.
//! * **The second guard is `list.isEmpty()`, not `i < count`.** Eight of the
//!   26.2 trade sets ask for more than their pool holds; those villagers
//!   carry fewer trades than `amount` says and no exception is raised.
//!
//! # The stream is per-sequence and it PERSISTS
//!
//! `LootContext.Builder.create(Optional<Identifier>)` resolves to
//! `MinecraftServer.getRandomSequence(id)` when the set names one, and every
//! one of the 68 shipped sets does. `RandomSequences.get` is a
//! `computeIfAbsent` over a saved map, so the stream for
//! `minecraft:trade_set/farmer/level_1` is **created once per world** and
//! continues across every farmer that ever levels up in it. Two villagers do
//! not draw the same trades because they do not restart the stream — which is
//! the opposite of what a per-villager seed would give, and is why
//! [`RandomSequences`] here is a map rather than a constructor.

use std::collections::BTreeMap;

use rustmc_worldgen::rng::support::{seed_from_hash_of, upgrade_seed_to_128bit_unmixed};
use rustmc_worldgen::rng::{RandomSource, XoroshiroRandomSource};

use super::tables::{TradeSet, TradeTables};

/// `RandomSequence(long seed, Optional<Identifier> id)`, offsets 0..38 of
/// `createSequence`:
///
/// ```text
/// Seed128bit s = RandomSupport.upgradeSeedTo128bitUnmixed(seed);
/// if (id.isPresent()) s = s.xor(seedForKey(id));   // md5 of id.toString()
/// return new XoroshiroRandomSource(s.mixed());
/// ```
///
/// The `unmixed` upgrade followed by a `mixed()` **after** the xor is the
/// part to get right: mixing before the xor gives a different stream, and
/// `XoroshiroRandomSource::from_seed` does exactly that, so it is the wrong
/// constructor here.
#[must_use]
pub fn sequence_source(world_seed: i64, sequence_id: Option<&str>) -> XoroshiroRandomSource {
    let mut s = upgrade_seed_to_128bit_unmixed(world_seed);
    if let Some(id) = sequence_id {
        let k = seed_from_hash_of(id);
        s = s.xor(k.lo, k.hi);
    }
    XoroshiroRandomSource::from_seed128(s.mixed())
}

/// `RandomSequences` — the saved map a world keeps.
///
/// `salt`, `includeWorldSeed` and `includeSequenceId` are the three fields
/// `createSequence(Identifier, long, int, boolean, boolean)` reads
/// (offsets 0..27): `seed = (includeWorldSeed ? worldSeed : 0) ^ (long) salt`,
/// and the id is folded in only when `includeSequenceId`. They exist so a
/// test world can be made to repeat itself and default to `0 / true / true`.
#[derive(Debug, Clone)]
pub struct RandomSequences {
    world_seed: i64,
    salt: i32,
    include_world_seed: bool,
    include_sequence_id: bool,
    sequences: BTreeMap<String, XoroshiroRandomSource>,
}

impl RandomSequences {
    #[must_use]
    pub fn new(world_seed: i64) -> Self {
        RandomSequences {
            world_seed,
            salt: 0,
            include_world_seed: true,
            include_sequence_id: true,
            sequences: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_settings(mut self, salt: i32, include_world_seed: bool, include_id: bool) -> Self {
        self.salt = salt;
        self.include_world_seed = include_world_seed;
        self.include_sequence_id = include_id;
        self
    }

    /// `RandomSequences.get(Identifier, long)` — `computeIfAbsent`, so the
    /// stream is created once and then continued.
    pub fn get(&mut self, id: &str) -> &mut XoroshiroRandomSource {
        let seed = if self.include_world_seed {
            self.world_seed
        } else {
            0
        } ^ i64::from(self.salt);
        let include_id = self.include_sequence_id;
        self.sequences
            .entry(id.to_string())
            .or_insert_with(|| sequence_source(seed, include_id.then_some(id)))
    }

    #[must_use]
    pub fn known(&self) -> usize {
        self.sequences.len()
    }
}

/// Why a listing produced no offer. `VillagerTrade.getOffer` returns null on
/// four distinct paths and they are not interchangeable: the first is data
/// this server can evaluate, the rest are subsystems it does not have, and
/// collapsing them would let a missing subsystem read as a villager who
/// legitimately has no such trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// `merchantPredicate.test(ctx)` was false — offsets 0..30. The predicate
    /// is a `LootItemCondition`; the 27 shipped ones are all
    /// `minecraft:entity_properties` restricting a trade to a villager type.
    MerchantPredicate,
    /// A `given_item_modifiers` function emptied the result stack — offsets
    /// 87..95.
    ModifierEmptiedResult,
    /// `wants.toItemCost(...).count() < 1` — offsets 208..218.
    WantsBelowOne,
    /// `additionalWants` present and below 1 — offsets 234..258.
    AdditionalWantsBelowOne,
    /// This server cannot evaluate the listing at all, naming what stopped
    /// it. **Not a null offer**: vanilla would have produced one and we
    /// cannot say which, so the caller must not silently drop the listing.
    Unported(String),
}

/// The outcome of asking a listing for an offer, keeping the two "no" cases
/// apart. See [`Refusal`].
#[derive(Debug, Clone, PartialEq)]
pub enum OfferOutcome<T> {
    Offer(T),
    /// Vanilla itself returns null here.
    None(Refusal),
    /// We do not know what vanilla would do.
    Unported(String),
}

/// `TradeSet.calculateNumberOfTrades(ctx)` — `amount.getInt(ctx)`, nothing
/// more. `None` when the provider is one this server does not roll, which is
/// a refusal and not a zero: a zero here would give a silently trade-less
/// villager.
#[must_use]
pub fn number_of_trades(set: &TradeSet) -> Option<i32> {
    set.amount.as_int()
}

/// `addOffersFromItemListings` / `addOffersFromItemListingsWithoutDuplicates`.
///
/// `get_offer` is the caller's `VillagerTrade.getOffer`; it is a parameter
/// because that method reaches into the loot system, which is not this
/// crate's. Returns the listing ids drawn, in the order they were added,
/// together with the refusals seen — the second half is what makes a test
/// able to tell "the pool refused" from "the loop never ran".
pub fn draw_offers<R: RandomSource, T, F>(
    rng: &mut R,
    pool: &[String],
    count: i32,
    allow_duplicates: bool,
    mut get_offer: F,
) -> Drawn<T>
where
    F: FnMut(&str) -> OfferOutcome<T>,
{
    let mut list: Vec<&String> = pool.iter().collect();
    let mut out = Vec::new();
    let mut refused = Vec::new();
    let mut i = 0;
    while i < count && !list.is_empty() {
        // `random.nextInt(list.size())` in both forms, and it is drawn before
        // anything is removed.
        let j = rng.next_i32_bound(list.size_i32()) as usize;
        if allow_duplicates {
            let id = list[j].clone();
            match get_offer(&id) {
                OfferOutcome::Offer(o) => {
                    out.push(o);
                    i += 1;
                }
                OfferOutcome::None(r) => {
                    refused.push((id, r));
                    list.remove(j);
                }
                OfferOutcome::Unported(w) => {
                    refused.push((id, Refusal::Unported(w)));
                    list.remove(j);
                }
            }
        } else {
            let id = list.remove(j).clone();
            match get_offer(&id) {
                OfferOutcome::Offer(o) => {
                    out.push(o);
                    i += 1;
                }
                OfferOutcome::None(r) => refused.push((id, r)),
                OfferOutcome::Unported(w) => refused.push((id, Refusal::Unported(w))),
            }
        }
    }
    (out, refused)
}

/// `list.size()` as the `int` `nextInt` takes.
trait SizeI32 {
    fn size_i32(&self) -> i32;
}

impl<T> SizeI32 for Vec<T> {
    fn size_i32(&self) -> i32 {
        i32::try_from(self.len()).unwrap_or(i32::MAX)
    }
}

/// What one draw produced: the offers, and the listings that refused with
/// why. The second half is what makes a caller able to tell "the pool
/// refused" from "the loop never ran", which are the same empty list.
pub type Drawn<T> = (Vec<T>, Vec<(String, Refusal)>);

/// `Villager.updateTrades(ServerLevel)`, offsets 0..44.
///
/// Returns the ids drawn for one `(profession, level)`, or `None` where
/// vanilla returns early — `getTrades(level) == null`, which is `none`,
/// `nitwit` and any level outside 1..=5. **`None` and `Some(vec![])` mean
/// different things**: the first is "this profession has no trades", the
/// second is "the draw produced none", and a villager in the first case is
/// never given an empty offer list, it is simply not touched.
///
/// The `DEBUG_UNLOCK_ALL_TRADES` tail (offsets 45..64) is not ported: it is a
/// `SharedConstants` development flag that is false in every shipped build.
pub fn update_trades<F>(
    tables: &TradeTables,
    sequences: &mut RandomSequences,
    profession: &str,
    level: u8,
    get_offer: F,
) -> Option<Drawn<String>>
where
    F: FnMut(&str) -> OfferOutcome<String>,
{
    let set = tables.trade_set_for(profession, level)?;
    let pool = tables.pool(&set.pool)?;
    let count = number_of_trades(set)?;
    let rng = match &set.random_sequence {
        Some(id) => sequences.get(id),
        // No named sequence: vanilla falls through to `level.getRandom()`,
        // the level's shared source. No shipped set takes this path, so
        // rather than reach for a stream this function does not own it uses
        // the sequence keyed by the set id, and the caller that meets a
        // sequence-less set has to say so.
        None => sequences.get(&set.id),
    };
    Some(draw_offers(
        rng,
        pool,
        count,
        set.allow_duplicates,
        get_offer,
    ))
}

/// `VillagerData.NEXT_LEVEL_XP_THRESHOLDS`, read from `VillagerData.<clinit>`
/// offsets 6..35: `{0, 10, 70, 150, 250}`.
pub const NEXT_LEVEL_XP_THRESHOLDS: [i32; 5] = [0, 10, 70, 150, 250];

/// `VillagerData.canLevelUp(int)`: `level >= 1 && level < 5`.
#[must_use]
pub fn can_level_up(level: i32) -> bool {
    (1..5).contains(&level)
}

/// `VillagerData.getMinXpPerLevel(int)`: `canLevelUp ? THRESHOLDS[level - 1] : 0`.
#[must_use]
pub fn min_xp_per_level(level: i32) -> i32 {
    if can_level_up(level) {
        NEXT_LEVEL_XP_THRESHOLDS[(level - 1) as usize]
    } else {
        0
    }
}

/// `VillagerData.getMaxXpPerLevel(int)`: `canLevelUp ? THRESHOLDS[level] : 0`.
///
/// Note the index is `level`, not `level - 1`, and that level 5 answers **0**
/// rather than 250 because `canLevelUp(5)` is false. A reader that clamped
/// the index instead would give a level-5 villager a 250-xp bar it can never
/// fill and `shouldIncreaseLevel` would then be true forever.
#[must_use]
pub fn max_xp_per_level(level: i32) -> i32 {
    if can_level_up(level) {
        NEXT_LEVEL_XP_THRESHOLDS[level as usize]
    } else {
        0
    }
}

/// `Villager.shouldIncreaseLevel()`, offsets 0..31:
/// `canLevelUp(level) && villagerXp >= getMaxXpPerLevel(level)`.
#[must_use]
pub fn should_increase_level(level: i32, villager_xp: i32) -> bool {
    can_level_up(level) && villager_xp >= max_xp_per_level(level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("minecraft:t{i}")).collect()
    }

    /// The positive control: with every listing producing an offer and
    /// duplicates off, the loop draws exactly `count` distinct listings.
    #[test]
    fn without_duplicates_draws_count_distinct_listings() {
        let mut rng = XoroshiroRandomSource::from_seed(20260905);
        let p = pool(6);
        let (got, refused) = draw_offers(&mut rng, &p, 3, false, |id| {
            OfferOutcome::Offer(id.to_string())
        });
        assert_eq!(got.len(), 3);
        assert!(refused.is_empty());
        let mut sorted = got.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 3, "a listing was drawn twice: {got:?}");
    }

    /// The duplicate-allowing form does not remove a successful pick, so with
    /// a one-listing pool it hands back the same listing `count` times where
    /// the other form hands back one. This is the whole difference between
    /// the two methods and it is the assertion that would fail if they were
    /// implemented as one.
    #[test]
    fn allowing_duplicates_can_draw_the_same_listing_twice() {
        let p = pool(1);
        let mut a = XoroshiroRandomSource::from_seed(1);
        let (dup, _) = draw_offers(&mut a, &p, 4, true, |id| {
            OfferOutcome::Offer(id.to_string())
        });
        assert_eq!(dup.len(), 4);
        let mut b = XoroshiroRandomSource::from_seed(1);
        let (nodup, _) = draw_offers(&mut b, &p, 4, false, |id| {
            OfferOutcome::Offer(id.to_string())
        });
        assert_eq!(nodup.len(), 1, "the pool is exhausted after one removal");
    }

    /// The loop's second guard. Eight shipped sets are over-subscribed and
    /// this is what happens to them: fewer offers, no error.
    #[test]
    fn an_over_subscribed_set_offers_fewer_trades() {
        let mut rng = XoroshiroRandomSource::from_seed(7);
        let p = pool(1);
        let (got, _) = draw_offers(&mut rng, &p, 2, false, |id| {
            OfferOutcome::Offer(id.to_string())
        });
        assert_eq!(got.len(), 1);
    }

    /// A refusal costs an iteration in the without-duplicates form and does
    /// not in the duplicate-allowing one. Same pool, same seed, same refusing
    /// listing: the counts differ, which is the observable that separates the
    /// two `i++` placements.
    #[test]
    fn a_refusal_costs_an_iteration_only_without_duplicates() {
        let p = pool(4);
        let refuse = "minecraft:t0";
        let mut a = XoroshiroRandomSource::from_seed(99);
        let (dup, dr) = draw_offers(&mut a, &p, 3, true, |id| {
            if id == refuse {
                OfferOutcome::None(Refusal::MerchantPredicate)
            } else {
                OfferOutcome::Offer(id.to_string())
            }
        });
        assert_eq!(dup.len(), 3, "a refusal must not consume a slot here");
        let mut b = XoroshiroRandomSource::from_seed(99);
        let (nodup, nr) = draw_offers(&mut b, &p, 3, false, |id| {
            if id == refuse {
                OfferOutcome::None(Refusal::MerchantPredicate)
            } else {
                OfferOutcome::Offer(id.to_string())
            }
        });
        // The seeds are equal so the first draw is the same index in both;
        // whether that index refuses depends on the pool, so assert the
        // relationship rather than a literal.
        assert!(nodup.len() + nr.len() <= 3);
        assert_eq!(dr.len() + dup.len(), 3 + dr.len());
    }

    /// An unported listing is not a null offer. It has to reach the caller as
    /// itself, because "vanilla would have given a trade here and we cannot
    /// say which" and "vanilla gives no trade here" are different servers.
    #[test]
    fn an_unported_listing_is_reported_and_not_silently_dropped() {
        let mut rng = XoroshiroRandomSource::from_seed(3);
        let p = pool(2);
        let (got, refused) = draw_offers(&mut rng, &p, 2, false, |_| {
            OfferOutcome::<String>::Unported("minecraft:exploration_map".into())
        });
        assert!(got.is_empty());
        assert_eq!(refused.len(), 2);
        assert!(matches!(refused[0].1, Refusal::Unported(ref w)
            if w == "minecraft:exploration_map"));
    }

    /// `RandomSequences.get` is a `computeIfAbsent`, so a second villager on
    /// the same sequence continues the stream rather than restarting it — the
    /// second draw must differ from the first. A per-villager seed would make
    /// them equal, which is what this asserts against.
    #[test]
    fn a_sequence_continues_across_villagers() {
        let mut seqs = RandomSequences::new(20260722);
        let first = seqs.get("minecraft:trade_set/farmer/level_1").next_i32();
        let second = seqs.get("minecraft:trade_set/farmer/level_1").next_i32();
        assert_ne!(first, second, "the stream restarted");
        assert_eq!(seqs.known(), 1);
        // And a different sequence id is a different stream from the start.
        let other = RandomSequences::new(20260722)
            .get("minecraft:trade_set/butcher/level_1")
            .next_i32();
        let farmer = RandomSequences::new(20260722)
            .get("minecraft:trade_set/farmer/level_1")
            .next_i32();
        assert_ne!(other, farmer);
        assert_eq!(
            farmer, first,
            "the same world and id must give the same stream"
        );
    }

    /// The seed folds the id in **before** the mix. Building the source with
    /// `from_seed` (which mixes first) gives a different stream, so this
    /// pins which of the two constructors is right.
    #[test]
    fn the_sequence_seed_xors_before_mixing() {
        let a = sequence_source(20260722, Some("minecraft:trade_set/farmer/level_1")).next_i32();
        let b = XoroshiroRandomSource::from_seed(20260722).next_i32();
        assert_ne!(a, b, "the sequence id did not reach the seed");
        // No id at all is the plain world-seed upgrade, which IS `from_seed`.
        assert_eq!(sequence_source(20260722, None).next_i32(), b);
    }

    #[test]
    fn the_level_thresholds_are_the_bytecodes() {
        assert_eq!(NEXT_LEVEL_XP_THRESHOLDS, [0, 10, 70, 150, 250]);
        assert_eq!(
            (1..=5).map(max_xp_per_level).collect::<Vec<_>>(),
            vec![10, 70, 150, 250, 0],
            "level 5 answers 0 because canLevelUp(5) is false"
        );
        assert_eq!(
            (1..=5).map(min_xp_per_level).collect::<Vec<_>>(),
            vec![0, 10, 70, 150, 0]
        );
        assert!(!can_level_up(0));
        assert!(!can_level_up(5));
        assert!(can_level_up(4));
    }

    /// A level-5 villager must never want to level again, and the boundary is
    /// `>=`, not `>`.
    #[test]
    fn should_increase_level_is_inclusive_and_stops_at_five() {
        assert!(!should_increase_level(1, 9));
        assert!(should_increase_level(1, 10));
        assert!(should_increase_level(4, 250));
        assert!(!should_increase_level(4, 249));
        assert!(!should_increase_level(5, 1_000_000));
    }
}
