//! The enchanting table's maths, with the draw order under a microscope.
//!
//! # What each bucket here is for
//!
//! The failure mode this file exists against is a port that gets every
//! *formula* right and the *draw order* wrong. Such a port produces
//! plausible offers for every input and agrees with vanilla on none of them,
//! and no test written in terms of "the answer looks reasonable" can see it.
//!
//! So the arithmetic and the stream are separated. [`Scripted`] is a
//! `RandomSource` that hands back a written-down sequence and records every
//! call it received, which lets a test assert *which* draws were taken, in
//! what order, with what bounds — independently of what the formula did with
//! them. Then a second group runs the real LCG and pins whole offers.
//!
//! # The seeded goldens record internal consistency ONLY
//!
//! `three_offers_for_a_diamond_pickaxe_at_a_fixed_seed` and its neighbours
//! are **not** measured against vanilla. Nothing in this repository has
//! observed an enchanting table's `container_set_data`, and nothing can until
//! a menu can be opened — `crate::menu` has no `minecraft:enchanting_table`
//! arm and adding one is the container-menu lane's change, because it moves
//! `ServerPlayer.containerCounter` and every later window id with it. What
//! these tests protect is a refactor: they will catch a changed draw order,
//! a changed formula and a changed table, and they will not catch this file
//! and the port being wrong together in the same way. That is stated here so
//! nobody reads a green run as a parity figure.

use rustmc_server::enchanting::{
    available_enchantment_results, enchantment_cost, enchantment_list, filter_compatible, offers,
    select_enchantment, total_weight, weighted_random_item, Instance, Offer,
    BOOKSHELF_OFFSET_COUNT, MAX_EFFECTIVE_BOOKSHELVES,
};
use rustmc_server::enchantment::{Registry, TABLE_PATH};
use rustmc_worldgen::rng::{LegacyRandomSource, RandomSource};

fn repo(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn registry() -> Option<Registry> {
    let p = repo(TABLE_PATH);
    if !rustmc_data::fixture::available(
        &p,
        "data/vanilla/enchantments.json",
        "ci/enchantments-golden.sh",
    ) {
        return None;
    }
    Some(Registry::load(&p).expect("enchantments.json"))
}

/// One call a `RandomSource` received.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    SetSeed(i64),
    IntBound(i32),
    Float,
}

/// A `RandomSource` that answers from a script and records what it was asked.
///
/// The recording is the instrument. A test that only checked the returned
/// number would pass for a port that took the same draws in the wrong order,
/// because a scripted source answering `[a, b]` gives `a` then `b` whichever
/// call site asks first.
struct Scripted {
    ints: std::collections::VecDeque<i32>,
    floats: std::collections::VecDeque<f32>,
    calls: Vec<Call>,
}

impl Scripted {
    fn new(ints: &[i32], floats: &[f32]) -> Self {
        Self {
            ints: ints.iter().copied().collect(),
            floats: floats.iter().copied().collect(),
            calls: Vec::new(),
        }
    }
    fn bounds(&self) -> Vec<Call> {
        self.calls.clone()
    }
}

impl RandomSource for Scripted {
    fn set_seed(&mut self, seed: i64) {
        self.calls.push(Call::SetSeed(seed));
    }
    fn next_i32(&mut self) -> i32 {
        unreachable!("the enchanting path never calls nextInt()")
    }
    fn next_i32_bound(&mut self, bound: i32) -> i32 {
        self.calls.push(Call::IntBound(bound));
        self.ints.pop_front().expect("the script ran out of ints")
    }
    fn next_i64(&mut self) -> i64 {
        unreachable!("the enchanting path never calls nextLong()")
    }
    fn next_bool(&mut self) -> bool {
        unreachable!("the enchanting path never calls nextBoolean()")
    }
    fn next_f32(&mut self) -> f32 {
        self.calls.push(Call::Float);
        self.floats
            .pop_front()
            .expect("the script ran out of floats")
    }
    fn next_f64(&mut self) -> f64 {
        unreachable!("the enchanting path never calls nextDouble()")
    }
    fn next_gaussian(&mut self) -> f64 {
        unreachable!("the enchanting path never calls nextGaussian()")
    }
}

// ---------------------------------------------------------------------------
// getEnchantmentCost — the formula, isolated from the stream
// ---------------------------------------------------------------------------

/// The three slots' bands, computed from *written-down* draws.
///
/// With `nextInt(8) = 4` and `nextInt(shelves + 1) = 6` at 10 shelves:
///
/// ```text
///   base = 4 + 1 + (10 >> 1) + 6 = 16
///   slot 0 = max(16 / 3, 1)      = 5
///   slot 1 = 16 * 2 / 3 + 1      = 11      (32 / 3 truncates to 10)
///   slot 2 = max(16, 10 * 2)     = 20
/// ```
#[test]
fn the_three_level_bands_are_the_bytecode_arithmetic() {
    for (slot, want) in [(0, 5), (1, 11), (2, 20)] {
        let mut r = Scripted::new(&[4, 6], &[]);
        assert_eq!(enchantment_cost(&mut r, slot, 10, Some(10)), want);
        // Two draws, in this order, with these bounds — the `>> 1` between
        // them is arithmetic, not a draw.
        assert_eq!(r.bounds(), vec![Call::IntBound(8), Call::IntBound(11)]);
    }
}

/// The positive control for the assertion above: the bounds really do
/// separate the two orders. `nextInt(8)` and `nextInt(11)` are different
/// calls, so a port that swapped them is visible even when both scripts
/// return the same numbers.
#[test]
fn swapping_the_two_cost_draws_would_be_visible() {
    let right = vec![Call::IntBound(8), Call::IntBound(11)];
    let swapped = vec![Call::IntBound(11), Call::IntBound(8)];
    assert_ne!(right, swapped);
}

/// `slot 0` floors at 1 and `slot 2` floors at `bookshelves * 2`.
#[test]
fn the_two_floors_fire() {
    // base = 0 + 1 + 0 + 0 = 1 at zero shelves; 1 / 3 == 0, floored to 1.
    let mut r = Scripted::new(&[0, 0], &[]);
    assert_eq!(enchantment_cost(&mut r, 0, 0, Some(10)), 1);
    // base = 1 + 1 + 7 + 0 = 9 at 15 shelves; slot 2 takes max(9, 30) = 30.
    let mut r = Scripted::new(&[1, 0], &[]);
    assert_eq!(enchantment_cost(&mut r, 2, 15, Some(10)), 30);
}

/// An item with no `ENCHANTABLE` component returns 0 **and takes no draw**.
///
/// The early return is at offset 12, before the random source is touched.
/// A port that computed first and returned 0 afterwards would desynchronise
/// the stream for the next slot rather than merely answer this one.
#[test]
fn an_unenchantable_item_consumes_no_draws() {
    let mut r = Scripted::new(&[], &[]);
    assert_eq!(enchantment_cost(&mut r, 0, 10, None), 0);
    assert!(r.bounds().is_empty(), "took {:?}", r.bounds());
}

/// The clamp is at 15 and the offsets list reaches 32.
#[test]
fn fifteen_and_thirty_two_bookshelves_are_the_same_offer() {
    assert_eq!(BOOKSHELF_OFFSET_COUNT, 32);
    assert_eq!(MAX_EFFECTIVE_BOOKSHELVES, 15);
    for slot in 0..3 {
        let mut a = Scripted::new(&[3, 5], &[]);
        let mut b = Scripted::new(&[3, 5], &[]);
        assert_eq!(
            enchantment_cost(&mut a, slot, 15, Some(10)),
            enchantment_cost(&mut b, slot, 32, Some(10)),
        );
        assert_eq!(a.bounds(), b.bounds());
    }
    // And the control: 14 is not 15, so the clamp is not hiding a formula
    // that ignores the count.
    let mut a = Scripted::new(&[3, 5], &[]);
    let mut b = Scripted::new(&[3, 5], &[]);
    assert_ne!(
        enchantment_cost(&mut a, 2, 14, Some(10)),
        enchantment_cost(&mut b, 2, 15, Some(10)),
    );
}

// ---------------------------------------------------------------------------
// getAvailableEnchantmentResults, WeightedRandom, filterCompatibleEnchantments
// ---------------------------------------------------------------------------

#[test]
fn the_highest_admissible_level_is_the_one_offered() {
    let Some(reg) = registry() else { return };
    // Sharpness: min_cost 1 + 11(l-1), max_cost 21 + 11(l-1). At cost 45 the
    // bands are l=4 -> [34, 54] and l=5 -> [45, 65]; both contain 45, and the
    // downward walk must take 5.
    let s = reg.get("minecraft:sharpness").expect("sharpness");
    assert!(45 >= s.min_cost_at(4) && 45 <= s.max_cost_at(4));
    assert!(45 >= s.min_cost_at(5) && 45 <= s.max_cost_at(5));
    let got = available_enchantment_results(
        &reg,
        45,
        "minecraft:diamond_sword",
        &["minecraft:sharpness".to_string()],
    );
    assert_eq!(
        got,
        vec![Instance {
            enchantment: "minecraft:sharpness".to_string(),
            level: 5
        }]
    );
}

#[test]
fn an_item_outside_every_band_offers_nothing() {
    let Some(reg) = registry() else { return };
    let got = available_enchantment_results(
        &reg,
        0,
        "minecraft:diamond_sword",
        &["minecraft:sharpness".to_string()],
    );
    assert!(
        got.is_empty(),
        "cost 0 is below sharpness's level-1 min_cost"
    );
}

/// A non-book only draws from its **primary** items; a book draws from every
/// supported one. `sharpness`'s primary set is `#enchantable/melee_weapon`
/// and its supported set is the wider `#enchantable/sharp_weapon`.
#[test]
fn a_book_reaches_enchantments_a_tool_does_not() {
    let Some(reg) = registry() else { return };
    let stream: Vec<String> = reg.in_enchanting_table().to_vec();
    let on_book = available_enchantment_results(&reg, 30, "minecraft:book", &stream);
    let on_sword = available_enchantment_results(&reg, 30, "minecraft:diamond_sword", &stream);
    assert!(
        on_book.len() > on_sword.len(),
        "book {} vs sword {} — the isPrimaryItem filter is not firing",
        on_book.len(),
        on_sword.len()
    );
    assert!(!on_sword.is_empty(), "a diamond sword must reach something");
}

/// `WeightedRandom` on a zero total takes **no draw** — the branch a port
/// loses by hoisting the `nextInt` above the guard.
#[test]
fn a_zero_total_weight_consumes_no_draw() {
    let Some(reg) = registry() else { return };
    let mut r = Scripted::new(&[], &[]);
    // An id the registry does not hold weighs 0, so the total is 0.
    let items = vec![Instance {
        enchantment: "minecraft:not_an_enchantment".to_string(),
        level: 1,
    }];
    assert_eq!(total_weight(&reg, &items), Some(0));
    assert_eq!(weighted_random_item(&mut r, &reg, &items), None);
    assert!(r.bounds().is_empty(), "took {:?}", r.bounds());
}

/// The subtraction walk lands on the right member for every index in range.
#[test]
fn the_weighted_walk_covers_every_index() {
    let Some(reg) = registry() else { return };
    // sharpness 10, efficiency 10, unbreaking 5 -> total 25.
    let items: Vec<Instance> = [
        "minecraft:sharpness",
        "minecraft:efficiency",
        "minecraft:unbreaking",
    ]
    .iter()
    .map(|s| Instance {
        enchantment: (*s).to_string(),
        level: 1,
    })
    .collect();
    assert_eq!(total_weight(&reg, &items), Some(25));
    for (idx, want) in [
        (0, "minecraft:sharpness"),
        (9, "minecraft:sharpness"),
        (10, "minecraft:efficiency"),
        (19, "minecraft:efficiency"),
        (20, "minecraft:unbreaking"),
        (24, "minecraft:unbreaking"),
    ] {
        let mut r = Scripted::new(&[idx], &[]);
        let got = weighted_random_item(&mut r, &reg, &items).expect("in range");
        assert_eq!(got.enchantment, want, "index {idx}");
        assert_eq!(r.bounds(), vec![Call::IntBound(25)]);
    }
}

/// `filterCompatibleEnchantments` removes the chosen entry **itself**,
/// because `areCompatible(a, a)` tests `equals` first and answers false.
#[test]
fn filtering_removes_the_chosen_entry_and_its_exclusions() {
    let Some(reg) = registry() else { return };
    let mut list: Vec<Instance> = [
        "minecraft:sharpness",
        "minecraft:smite",
        "minecraft:unbreaking",
    ]
    .iter()
    .map(|s| Instance {
        enchantment: (*s).to_string(),
        level: 1,
    })
    .collect();
    let chosen = list[0].clone();
    filter_compatible(&reg, &mut list, &chosen);
    let left: Vec<&str> = list.iter().map(|i| i.enchantment.as_str()).collect();
    assert_eq!(left, vec!["minecraft:unbreaking"]);
    assert!(!reg.are_compatible("minecraft:sharpness", "minecraft:sharpness"));
    assert!(!reg.are_compatible("minecraft:sharpness", "minecraft:smite"));
    assert!(reg.are_compatible("minecraft:sharpness", "minecraft:unbreaking"));
}

// ---------------------------------------------------------------------------
// selectEnchantment — the loop shape
// ---------------------------------------------------------------------------

/// The prologue's five draws, in order, with their bounds.
///
/// A diamond sword's enchantability is 10, so the two level draws are
/// `nextInt(10 / 4 + 1)` = `nextInt(3)`; then two `nextFloat`s; then the
/// first `WeightedRandom` draw over the whole available list.
#[test]
fn select_enchantment_takes_two_ints_then_two_floats() {
    let Some(reg) = registry() else { return };
    let stream: Vec<String> = reg.in_enchanting_table().to_vec();
    // Floats 0.5/0.5 make the spread exactly 0, so `level` is unscaled and
    // the arithmetic under test is only the two int draws. The trailing 0s
    // feed the weighted draws; 49 ends the loop on its first test because
    // `nextInt(50) = 49 > level` for a small level.
    let mut r = Scripted::new(&[1, 1, 0, 49], &[0.5, 0.5]);
    let got = select_enchantment(&mut r, &reg, "minecraft:diamond_sword", 5, &stream);
    let b = r.bounds();
    assert_eq!(b[0], Call::IntBound(3), "enchantability 10 -> 10/4 + 1");
    assert_eq!(b[1], Call::IntBound(3));
    assert_eq!(b[2], Call::Float);
    assert_eq!(b[3], Call::Float);
    // then one weighted draw, then the loop's own nextInt(50)
    assert!(matches!(b[4], Call::IntBound(_)));
    assert_eq!(b[5], Call::IntBound(50));
    assert_eq!(b.len(), 6, "no draw after the loop test failed: {b:?}");
    assert_eq!(got.len(), 1);
}

/// The loop draws `nextInt(50)` **before** it tests whether the pool is
/// empty, so the draw happens even on the iteration that breaks.
#[test]
fn the_loop_condition_is_drawn_before_the_emptiness_test() {
    let Some(reg) = registry() else { return };
    // A one-member stream: after the first pick, filtering removes it (a
    // chosen entry is incompatible with itself) and the pool is empty. The
    // `nextInt(50) = 0` must still be taken before the break.
    let stream = vec!["minecraft:sharpness".to_string()];
    let mut r = Scripted::new(&[0, 0, 0, 0], &[0.5, 0.5]);
    let got = select_enchantment(&mut r, &reg, "minecraft:diamond_sword", 20, &stream);
    let b = r.bounds();
    assert_eq!(got.len(), 1, "only sharpness was available");
    assert_eq!(
        b.last(),
        Some(&Call::IntBound(50)),
        "the last draw is the loop test, taken before the break: {b:?}"
    );
}

/// An unenchantable item returns empty and takes no draw at all.
#[test]
fn select_enchantment_on_an_unenchantable_item_takes_no_draws() {
    let Some(reg) = registry() else { return };
    let stream: Vec<String> = reg.in_enchanting_table().to_vec();
    let mut r = Scripted::new(&[], &[]);
    assert!(select_enchantment(&mut r, &reg, "minecraft:stone", 30, &stream).is_empty());
    assert!(r.bounds().is_empty(), "took {:?}", r.bounds());
}

// ---------------------------------------------------------------------------
// The whole of slotsChanged, on the real LCG
// ---------------------------------------------------------------------------

/// The clue draw is reseeded per slot, so the clue for slot *i* is a function
/// of `seed + i` and its cost, and NOT of the cost stream.
///
/// Demonstrated rather than asserted from the source: two runs whose cost
/// stream differs (different bookshelf counts) but whose slot-0 cost happens
/// to coincide produce the same slot-0 clue.
#[test]
fn the_clue_stream_is_reseeded_per_slot() {
    let Some(reg) = registry() else { return };
    let mut r = LegacyRandomSource::new(0);
    let a = enchantment_list(&mut r, &reg, 12345, 0, 12, "minecraft:diamond_sword");
    // A different starting state must not matter — the function reseeds.
    let mut r2 = LegacyRandomSource::new(999_999);
    for _ in 0..37 {
        r2.next_i32_bound(7);
    }
    let b = enchantment_list(&mut r2, &reg, 12345, 0, 12, "minecraft:diamond_sword");
    assert_eq!(a, b, "getEnchantmentList must reseed to seed + slot");
    // And the slot really is part of the seed.
    let mut r3 = LegacyRandomSource::new(0);
    let c = enchantment_list(&mut r3, &reg, 12345, 1, 12, "minecraft:diamond_sword");
    assert_ne!(a, c, "slot 0 and slot 1 draw from different streams");
}

/// A seeded golden. **Internal consistency only** — see the module header.
#[test]
fn three_offers_for_a_diamond_pickaxe_at_a_fixed_seed() {
    let Some(reg) = registry() else { return };
    let mut r = LegacyRandomSource::new(0);
    let got = offers(&mut r, &reg, 12345, 15, "minecraft:diamond_pickaxe");

    // Structural invariants that hold for EVERY seed, asserted first so a
    // changed golden below is not the only thing this test says.
    for (i, o) in got.iter().enumerate() {
        let slot = i32::try_from(i).expect("0..3");
        // `costs[i] < i + 1 -> 0` at offset 131, kept in the bytecode's own
        // shape rather than as `> slot`.
        #[allow(clippy::int_plus_one)]
        {
            assert!(
                o.cost == 0 || o.cost >= slot + 1,
                "slot {i}: cost {} is below the slot's own floor",
                o.cost
            );
        }
        if o.cost == 0 {
            assert_eq!((o.enchant_clue, o.level_clue), (-1, -1));
        }
        if o.enchant_clue >= 0 {
            let id = &reg.order()[usize::try_from(o.enchant_clue).expect("in range")];
            let e = reg.get(id).expect("a real entry");
            assert!(
                reg.in_enchanting_table().contains(id),
                "{id} is not offerable"
            );
            assert!(o.level_clue >= 1 && o.level_clue <= e.max_level);
        }
    }
    assert!(got[0].cost < got[2].cost, "slot 2 is the expensive one");

    // The golden itself.
    let want = [
        Offer {
            cost: 6,
            enchant_clue: 40,
            level_clue: 1,
        },
        Offer {
            cost: 20,
            enchant_clue: 8,
            level_clue: 3,
        },
        Offer {
            cost: 30,
            enchant_clue: 34,
            level_clue: 1,
        },
    ];
    assert_eq!(
        got, want,
        "the offers moved; if this was deliberate say which draw changed"
    );
}

/// The same seed, one fewer bookshelf: every number is allowed to move, but
/// the run must still be self-consistent and must NOT be identical — which is
/// the control saying the bookshelf count reaches the arithmetic at all.
#[test]
fn a_different_bookshelf_count_moves_the_offers() {
    let Some(reg) = registry() else { return };
    let mut a = LegacyRandomSource::new(0);
    let mut b = LegacyRandomSource::new(0);
    let with_15 = offers(&mut a, &reg, 12345, 15, "minecraft:diamond_pickaxe");
    let with_14 = offers(&mut b, &reg, 12345, 14, "minecraft:diamond_pickaxe");
    assert_ne!(with_15, with_14);
}

/// An item with no `ENCHANTABLE` component gets three empty rows and the
/// second loop is never entered.
#[test]
fn an_unenchantable_item_gets_three_empty_rows() {
    let Some(reg) = registry() else { return };
    let mut r = LegacyRandomSource::new(0);
    let got = offers(&mut r, &reg, 12345, 15, "minecraft:stone");
    for o in &got {
        assert_eq!((o.cost, o.enchant_clue, o.level_clue), (0, -1, -1));
    }
}

// ---------------------------------------------------------------------------
// The tag set
// ---------------------------------------------------------------------------

/// `#in_enchanting_table` is literally `["#minecraft:non_treasure"]` in the
/// datapack, and treasure/non_treasure partition the registry.
///
/// Both are properties the table's reachability rests on: an entry in neither
/// tag would be unreachable from every route, and one in both would be
/// offerable at a table while being marked treasure. The generator asserts
/// them too; this asserts that the READER preserves them, which is a
/// different claim — a `tag()` that dropped a member would pass the
/// generator's check and fail this one.
#[test]
fn the_treasure_split_survives_the_reader() {
    let Some(reg) = registry() else { return };
    let in_table = reg.tag("minecraft:in_enchanting_table").expect("the tag");
    assert_eq!(in_table, reg.in_enchanting_table());
    let non = reg.tag("minecraft:non_treasure").expect("non_treasure");
    assert_eq!(
        in_table, non,
        "in_enchanting_table is exactly #non_treasure"
    );

    let treasure: std::collections::BTreeSet<&String> = reg
        .tag("minecraft:treasure")
        .expect("treasure")
        .iter()
        .collect();
    let non_set: std::collections::BTreeSet<&String> = non.iter().collect();
    assert!(treasure.is_disjoint(&non_set));
    let union: std::collections::BTreeSet<&String> = treasure.union(&non_set).copied().collect();
    let all: std::collections::BTreeSet<&String> = reg.order().iter().collect();
    assert_eq!(
        union,
        all,
        "the two tags must cover all {} entries",
        reg.len()
    );

    // Mending is the canonical treasure enchantment and must not be offerable.
    assert!(treasure.iter().any(|s| s.as_str() == "minecraft:mending"));
    assert!(!reg
        .in_enchanting_table()
        .iter()
        .any(|s| s == "minecraft:mending"));
}

/// An absent tag is `None`, not an empty list — and the difference is a draw.
///
/// `getEnchantmentList` returns `List.of()` at offset 46 for an absent
/// `IN_ENCHANTING_TABLE` **without** entering `selectEnchantment`, whereas an
/// empty-but-present one would take its four prologue draws first. Nothing in
/// vanilla's datapack removes that tag, so this is the reader's contract
/// rather than a live case, and it is asserted because a `map_or_default`
/// here would silently collapse the two.
#[test]
fn an_absent_tag_is_none() {
    let Some(reg) = registry() else { return };
    assert!(reg.tag("minecraft:no_such_tag").is_none());
    assert!(reg.tag_names().count() >= 20, "the tag set looks truncated");
}
