//! The enchanting table's maths: three offers from a bookshelf count and a
//! per-player seed.
//!
//! Everything here is transcribed from `EnchantmentMenu`,
//! `EnchantmentHelper`, `WeightedRandom` and `EnchantingTableBlock` in the
//! pinned `server-26.2.jar`, `javap -p -c -constants` under JDK 25. The draw
//! order is the point of the file — every offer the client sees is a function
//! of one seed and the exact sequence of `RandomSource` calls below, so a
//! single extra or missing draw changes all three offers and every clue.
//!
//! # The draw order, in one place
//!
//! `EnchantmentMenu.slotsChanged` is a guard plus a `ContainerLevelAccess`
//! callback; the callback (`lambda$slotsChanged$0`, offsets 0..277) is the
//! whole mechanism:
//!
//! ```text
//!    22  bookshelves = count of EnchantingTableBlock.BOOKSHELF_OFFSETS that
//!                      pass isValidBookShelf                      NO draws
//!    70  random.setSeed(enchantmentSeed)                          1 setSeed
//!    90  for i in 0..3:
//!   103      costs[i] = getEnchantmentCost(random, i, bookshelves, stack)
//!                                                                 2 draws each
//!   115      enchantClue[i] = -1;  levelClue[i] = -1
//!   131      if costs[i] < i + 1: costs[i] = 0
//!   162  for i in 0..3:
//!   168      if costs[i] > 0:
//!   193          list = getEnchantmentList(registryAccess, stack, i, costs[i])
//!   208          if !list.isEmpty():
//!   221              inst = list.get(random.nextInt(list.size()))   1 draw
//!   236              enchantClue[i] = idMap.getId(inst.enchantment())
//!   255              levelClue[i]   = inst.level()
//!   273  broadcastChanges()
//! ```
//!
//! **The two loops are separate and that is load-bearing.** All three costs
//! are drawn before any clue is, so the six cost draws are contiguous. Fusing
//! them into one loop — which reads as an obvious tidy-up — interleaves the
//! clue draws with the cost draws and changes every number after the first.
//!
//! And `getEnchantmentList` **reseeds**: `random.setSeed(enchantmentSeed + i)`
//! at its offset 0. So the clue for slot *i* is drawn from a fresh stream, and
//! the `nextInt(list.size())` at offset 221 above continues *that* stream
//! rather than the cost stream.
//!
//! # `getEnchantmentCost`, and where the 15 actually is
//!
//! ```text
//!    0  Enchantable e = stack.get(DataComponents.ENCHANTABLE)
//!   12  if e == null                              -> 0
//!   19  if (bookshelves > 15) bookshelves = 15
//!   28  i = random.nextInt(8) + 1 + (bookshelves >> 1) + random.nextInt(bookshelves + 1)
//!   54  slot 0 -> Math.max(i / 3, 1)
//!   67  slot 1 -> i * 2 / 3 + 1
//!   81  else   -> Math.max(i, bookshelves * 2)
//! ```
//!
//! The clamp is here, not in `slotsChanged`, and the raw count it clamps is
//! not bounded by 15: `BOOKSHELF_OFFSETS` is
//! `BlockPos.betweenClosedStream(-2, 0, -2, 2, 1, 2)` filtered to
//! `abs(x) == 2 || abs(z) == 2`, which is the perimeter of a 5x5 at two
//! heights — **32 positions**. A ring of 32 bookshelves and a ring of 15 give
//! identical offers, and a port that clamped at the counting site instead
//! would agree with vanilla on every number and differ on nothing observable,
//! which is why the clamp is written where the bytecode puts it.
//!
//! Note the two draws and their order: `nextInt(8)` first, then
//! `nextInt(bookshelves + 1)`. The `(bookshelves >> 1)` between them is not a
//! draw. Reordering them is invisible at 0 bookshelves — `nextInt(1)`
//! consumes a draw but always answers 0 — and wrong everywhere else.
//!
//! # What this module does NOT do
//!
//! It computes offers. It does not open a menu: `Player.openMenu` and
//! `AbstractContainerMenu.broadcastChanges` are the container-menu lane's
//! (`crate::menu` has three menu specs and `minecraft:enchanting_table` is
//! not one of them), and adding an arm here would advance
//! `ServerPlayer.containerCounter` and shift every later window id in a
//! recording. So nothing in the gameplay corpus can observe these numbers
//! today and no scenario claims to.

use crate::enchantment::{Enchantment, Registry};
use rustmc_worldgen::rng::RandomSource;

/// `EnchantmentInstance(Holder<Enchantment>, int level)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub enchantment: String,
    pub level: i32,
}

/// One of the three rows an enchanting table shows.
///
/// `costs`, `enchantClue` and `levelClue` are three `DataSlot`s each, so all
/// three fields reach the client as `container_set_data` — which is what
/// would make this measurable if a menu could be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// `costs[i]`. Zero means the row is empty.
    pub cost: i32,
    /// `enchantClue[i]` — the ENCHANTMENT registry id, or `-1`.
    pub enchant_clue: i32,
    /// `levelClue[i]`, or `-1`.
    pub level_clue: i32,
}

impl Offer {
    pub const EMPTY: Self = Self {
        cost: 0,
        enchant_clue: -1,
        level_clue: -1,
    };
}

/// `EnchantingTableBlock.BOOKSHELF_OFFSETS.size()`.
///
/// `BlockPos.betweenClosedStream(-2, 0, -2, 2, 1, 2)` is 5x2x5 = 50
/// positions; `lambda$static$0` keeps those with `abs(x) == 2 || abs(z) == 2`,
/// which is 25 - 9 = 16 per layer over two layers.
pub const BOOKSHELF_OFFSET_COUNT: usize = 32;

/// The clamp inside `getEnchantmentCost`, offset 19.
pub const MAX_EFFECTIVE_BOOKSHELVES: i32 = 15;

/// `EnchantmentHelper.getEnchantmentCost(RandomSource, int, int, ItemStack)`.
///
/// `enchantability` is `stack.get(DataComponents.ENCHANTABLE)`; `None` is the
/// null the method returns 0 for, **before** touching the random source. That
/// early return matters: an unenchantable item consumes no draws, so the
/// stream is untouched rather than merely unused.
pub fn enchantment_cost<R: RandomSource>(
    random: &mut R,
    slot: i32,
    bookshelves: i32,
    enchantability: Option<i32>,
) -> i32 {
    if enchantability.is_none() {
        return 0;
    }
    let shelves = bookshelves.min(MAX_EFFECTIVE_BOOKSHELVES);
    let base = random.next_i32_bound(8) + 1 + (shelves >> 1) + random.next_i32_bound(shelves + 1);
    match slot {
        0 => (base / 3).max(1),
        1 => base * 2 / 3 + 1,
        _ => base.max(shelves * 2),
    }
}

/// `EnchantmentHelper.getAvailableEnchantmentResults(int, ItemStack, Stream)`.
///
/// The filter is `enchantment.isPrimaryItem(stack) || isBook`, and the body
/// walks `level` **down** from `getMaxLevel()` to `getMinLevel()`, taking the
/// first level whose `[minCost, maxCost]` band contains `cost` and breaking.
/// Downward, so the highest admissible level wins; a loop written upward
/// gives the lowest and every offer is then quietly too weak.
///
/// `stream` is the caller's iteration order and it is preserved, because
/// `WeightedRandom.getWeightedItem` walks this list subtracting weights.
#[must_use]
pub fn available_enchantment_results(
    registry: &Registry,
    cost: i32,
    item: &str,
    stream: &[String],
) -> Vec<Instance> {
    let is_book = item == "minecraft:book";
    let mut out = Vec::new();
    for id in stream {
        let Some(e) = registry.get(id) else { continue };
        if !(e.is_primary_item(item) || is_book) {
            continue;
        }
        let mut level = e.max_level;
        while level >= Enchantment::min_level() {
            if cost >= e.min_cost_at(level) && cost <= e.max_cost_at(level) {
                out.push(Instance {
                    enchantment: id.clone(),
                    level,
                });
                break;
            }
            level -= 1;
        }
    }
    out
}

/// `WeightedRandom.getTotalWeight(List, ToIntFunction)`.
///
/// Summed as a **long** and refused above `Integer.MAX_VALUE`, which is why
/// the accumulator here is `i64`. `None` is the `IllegalArgumentException`
/// vanilla throws; with 43 entries whose weights are 1..10 it is unreachable
/// from the enchanting table and is transcribed because the next caller of
/// this function need not be.
#[must_use]
pub fn total_weight(registry: &Registry, items: &[Instance]) -> Option<i32> {
    let mut total: i64 = 0;
    for i in items {
        total += i64::from(registry.get(&i.enchantment).map_or(0, |e| e.weight));
    }
    i32::try_from(total).ok()
}

/// `WeightedRandom.getRandomItem(RandomSource, List, ToIntFunction)`.
///
/// ```text
///    0  if total <  0 -> throw
///   20  if total == 0 -> Optional.empty()      NO draw
///   28  idx = random.nextInt(total)
///   37  getWeightedItem(list, idx, fn):
///          for item: idx -= weight(item); if (idx < 0) return item
///          -> Optional.empty()
/// ```
///
/// **A zero total consumes no draw.** That is the branch a port loses by
/// hoisting the `nextInt` above the guard, and losing it desynchronises the
/// stream for every later call rather than producing a wrong item here.
pub fn weighted_random_item<R: RandomSource>(
    random: &mut R,
    registry: &Registry,
    items: &[Instance],
) -> Option<Instance> {
    let total = total_weight(registry, items)?;
    if total == 0 {
        return None;
    }
    let mut idx = random.next_i32_bound(total);
    for i in items {
        idx -= registry.get(&i.enchantment).map_or(0, |e| e.weight);
        if idx < 0 {
            return Some(i.clone());
        }
    }
    None
}

/// `EnchantmentHelper.filterCompatibleEnchantments(List, EnchantmentInstance)`
/// — `list.removeIf(x -> !Enchantment.areCompatible(x.enchantment(), chosen.enchantment()))`.
///
/// Note that `areCompatible(a, a)` is **false** (offset 0 tests `equals`), so
/// this also removes the chosen entry itself. A port that special-cased
/// identity as compatible would let the same enchantment be drawn twice and
/// the second draw would then be silently discarded by `ItemEnchantments`.
pub fn filter_compatible(registry: &Registry, list: &mut Vec<Instance>, chosen: &Instance) {
    list.retain(|x| registry.are_compatible(&x.enchantment, &chosen.enchantment));
}

/// `EnchantmentHelper.selectEnchantment(RandomSource, ItemStack, int, Stream)`.
///
/// ```text
///    5  Enchantable e = stack.get(ENCHANTABLE); if null -> empty   NO draws
///   25  level += 1 + random.nextInt(e.value()/4 + 1)
///                 + random.nextInt(e.value()/4 + 1)                2 draws
///   61  f = (random.nextFloat() + random.nextFloat() - 1.0F) * 0.15F
///                                                                  2 draws
///   82  level = Mth.clamp(Math.round(level + level * f), 1, MAX_INT)
///  101  avail = getAvailableEnchantmentResults(level, stack, stream)
///  109  if avail.isEmpty() -> return empty
///  119  WeightedRandom.getRandomItem(...).ifPresent(out::add)       1 draw
///  145  while (random.nextInt(50) <= level) {                       1 draw/iter
///  157      if (!out.isEmpty()) filterCompatibleEnchantments(avail, out.getLast())
///  182      if (avail.isEmpty()) break
///  195      WeightedRandom.getRandomItem(...).ifPresent(out::add)   1 draw
///  221      level /= 2
///          }
/// ```
///
/// Three things a plausible rewrite gets wrong:
///
/// * **the loop condition is drawn before the emptiness test**, so the
///   `nextInt(50)` happens even on the iteration that breaks.
/// * **`level` is halved at the END of the body**, after the second draw.
/// * **the two `nextInt(e/4 + 1)` calls are separate draws of the same
///   bound**, not one draw doubled.
///
/// `level + level * f` is evaluated in `f32` (`i2f; i2f; fmul; fadd`), and
/// `Math.round(float)` is the JDK's bit form — see [`java_round_f32`].
pub fn select_enchantment<R: RandomSource>(
    random: &mut R,
    registry: &Registry,
    item: &str,
    cost: i32,
    stream: &[String],
) -> Vec<Instance> {
    let mut out: Vec<Instance> = Vec::new();
    let Some(enchantability) = registry.enchantability(item) else {
        return out;
    };
    let bound = enchantability / 4 + 1;
    let mut level = cost + 1 + random.next_i32_bound(bound) + random.next_i32_bound(bound);

    let spread = (random.next_f32() + random.next_f32() - 1.0) * 0.15;
    #[allow(clippy::cast_precision_loss)]
    let scaled = level as f32 + level as f32 * spread;
    level = java_round_f32(scaled).clamp(1, i32::MAX);

    let mut avail = available_enchantment_results(registry, level, item, stream);
    if avail.is_empty() {
        return out;
    }
    if let Some(first) = weighted_random_item(random, registry, &avail) {
        out.push(first);
    }
    while random.next_i32_bound(50) <= level {
        if let Some(last) = out.last().cloned() {
            filter_compatible(registry, &mut avail, &last);
        }
        if avail.is_empty() {
            break;
        }
        if let Some(next) = weighted_random_item(random, registry, &avail) {
            out.push(next);
        }
        level /= 2;
    }
    out
}

/// `EnchantmentMenu.getEnchantmentList(RegistryAccess, ItemStack, int, int)`.
///
/// ```text
///    0  random.setSeed(enchantmentSeed + slot)
///   19  tag = registry.get(EnchantmentTags.IN_ENCHANTING_TABLE)
///   38  if tag.isEmpty() -> List.of()
///   50  list = EnchantmentHelper.selectEnchantment(random, stack, cost, tag.stream())
///   73  if (stack.is(Items.BOOK) && list.size() > 1)
///  107      list.remove(random.nextInt(list.size()))
///  118  return list
/// ```
///
/// The book arm removes **one random member** rather than truncating, and the
/// draw happens only when the list has more than one, so a book with a
/// one-element result consumes no extra draw.
pub fn enchantment_list<R: RandomSource>(
    random: &mut R,
    registry: &Registry,
    seed: i32,
    slot: i32,
    cost: i32,
    item: &str,
) -> Vec<Instance> {
    random.set_seed(i64::from(seed) + i64::from(slot));
    let stream = registry.in_enchanting_table();
    if stream.is_empty() {
        return Vec::new();
    }
    let mut list = select_enchantment(random, registry, item, cost, stream);
    if item == "minecraft:book" && list.len() > 1 {
        let n = i32::try_from(list.len()).unwrap_or(i32::MAX);
        let idx = random.next_i32_bound(n);
        list.remove(usize::try_from(idx).unwrap_or(0));
    }
    list
}

/// The whole of `EnchantmentMenu.slotsChanged` for a non-empty, enchantable
/// input: the three rows a client would be sent.
///
/// `seed` is `ServerPlayer.getEnchantmentSeed()`, carried in the menu's
/// `enchantmentSeed` `DataSlot`; `bookshelves` is the raw count over
/// [`BOOKSHELF_OFFSET_COUNT`] positions, unclamped — the clamp is inside
/// [`enchantment_cost`] where vanilla puts it.
///
/// One `RandomSource` is threaded through both loops because vanilla has one
/// field, and `getEnchantmentList` reseeds it; passing two would decouple the
/// clue stream from the cost stream and hide exactly the ordering this
/// function exists to reproduce.
pub fn offers<R: RandomSource>(
    random: &mut R,
    registry: &Registry,
    seed: i32,
    bookshelves: i32,
    item: &str,
) -> [Offer; 3] {
    let mut out = [Offer::EMPTY, Offer::EMPTY, Offer::EMPTY];
    let enchantability = registry.enchantability(item);

    random.set_seed(i64::from(seed));
    for (i, offer) in out.iter_mut().enumerate() {
        let slot = i32::try_from(i).expect("0..3");
        let mut cost = enchantment_cost(random, slot, bookshelves, enchantability);
        if cost < slot + 1 {
            cost = 0;
        }
        offer.cost = cost;
    }

    for (i, offer) in out.iter_mut().enumerate() {
        if offer.cost <= 0 {
            continue;
        }
        let slot = i32::try_from(i).expect("0..3");
        let list = enchantment_list(random, registry, seed, slot, offer.cost, item);
        if list.is_empty() {
            continue;
        }
        let n = i32::try_from(list.len()).unwrap_or(i32::MAX);
        let pick = usize::try_from(random.next_i32_bound(n)).unwrap_or(0);
        let inst = &list[pick];
        offer.enchant_clue = registry.numeric_id(&inst.enchantment).unwrap_or(-1);
        offer.level_clue = inst.level;
    }
    out
}

/// `Math.round(float)`, which is neither `f32::round` nor `floor(x + 0.5)`
/// evaluated in `f32`.
///
/// The JDK does this on the bits, and the difference is real for one value:
/// `0x1.fffffep-2`, the float just below one half, where adding `0.5f` rounds
/// up to `1.0f` and the naive form answers 1 where Java answers 0. The input
/// here is `level + level * ((nextFloat() + nextFloat() - 1) * 0.15)`, an
/// arbitrary float, so the bit form is the one ported.
///
/// The same function is in `crates/rustmc-worldgen/src/feature/ore.rs`. It is
/// duplicated rather than shared because that one is private to the ore
/// feature and lifting it into a shared crate is a change to worldgen's
/// surface for a caller in another crate — see the note in this module's
/// header about not touching another lane's files.
#[must_use]
pub fn java_round_f32(value: f32) -> i32 {
    #[allow(clippy::cast_possible_wrap)]
    let bits = value.to_bits() as i32;
    let biased_exponent = (bits & 0x7f80_0000) >> 23;
    let shift = (24 - 2 + 127) - biased_exponent;
    if shift & -32 == 0 {
        let mut r = (bits & 0x007f_ffff) | 0x0080_0000;
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        #[allow(clippy::cast_possible_truncation)]
        {
            value as i32
        }
    }
}

/// The shape of `MenuType.ENCHANTMENT`, for the lane that will open it.
///
/// Derived here rather than in [`crate::menu`] because adding an arm to
/// `menu_for_state` changes behaviour — it advances
/// `ServerPlayer.containerCounter` on a click that currently opens nothing,
/// and every later window id in a recording moves with it. That is the
/// container-menu lane's change to make. What is *not* theirs to re-derive is
/// the arithmetic, so the numbers are read off the constructor here and can be
/// lifted verbatim.
///
/// `EnchantmentMenu(int, Inventory, ContainerLevelAccess)`, offsets 0..269:
///
/// ```text
///     1  super(MenuType.ENCHANTMENT, id)
///   100  addSlot(...)                       the item slot
///   122  addSlot(...)                       the lapis slot
///   135  addStandardInventorySlots(...)     36 more
///   147  addDataSlot(shared(costs, 0))
///   160  addDataSlot(shared(costs, 1))
///   173  addDataSlot(shared(costs, 2))
///   182  addDataSlot(enchantmentSeed)
///   192  enchantmentSeed.set(player.getEnchantmentSeed())
///   204  addDataSlot(shared(enchantClue, 0))
///   217  addDataSlot(shared(enchantClue, 1))
///   230  addDataSlot(shared(enchantClue, 2))
///   243  addDataSlot(shared(levelClue, 0))
///   256  addDataSlot(shared(levelClue, 1))
///   269  addDataSlot(shared(levelClue, 2))
/// ```
///
/// So the ten `container_set_data` ids are, in order: costs 0..2, the seed,
/// enchantClue 0..2, levelClue 0..2. [`offers`] produces exactly the values
/// those ten carry, minus the seed, which is the menu's input rather than its
/// output.
pub mod menu_shape {
    /// `MenuType.ENCHANTMENT`'s registry name.
    pub const MENU_TYPE: &str = "minecraft:enchantment";
    /// `EnchantingTableBlockEntity`'s default name, the title
    /// `EnchantingTableBlock.getMenuProvider` passes to `SimpleMenuProvider`.
    pub const TITLE: &str = "container.enchant";
    /// The two `addSlot` calls: the item and the lapis.
    pub const CONTAINER_SLOTS: usize = 2;
    /// Those two plus `addStandardInventorySlots`' 36.
    pub const SLOTS: usize = 38;
    /// costs 0..2, seed, enchantClue 0..2, levelClue 0..2.
    pub const DATA_SLOTS: usize = 10;
    /// The `container_set_data` id of `costs[i]`.
    pub const DATA_COST: [i16; 3] = [0, 1, 2];
    /// The `container_set_data` id of `enchantmentSeed`.
    pub const DATA_SEED: i16 = 3;
    /// The `container_set_data` id of `enchantClue[i]`.
    pub const DATA_ENCHANT_CLUE: [i16; 3] = [4, 5, 6];
    /// The `container_set_data` id of `levelClue[i]`.
    pub const DATA_LEVEL_CLUE: [i16; 3] = [7, 8, 9];
}

/// The shape of `MenuType.ANVIL`, on the same terms as [`menu_shape`].
///
/// `AnvilMenu(int, Inventory, ContainerLevelAccess)` passes
/// `MenuType.ANVIL` to `ItemCombinerMenu`, whose constructor adds the slot
/// definition's three slots and then `addStandardInventorySlots`; the anvil
/// then adds **one** data slot, `cost`. The title is `container.repair`, a
/// literal in `AnvilBlock`.
pub mod anvil_menu_shape {
    pub const MENU_TYPE: &str = "minecraft:anvil";
    pub const TITLE: &str = "container.repair";
    /// `INPUT_SLOT`, `ADDITIONAL_SLOT`, `RESULT_SLOT`.
    pub const CONTAINER_SLOTS: usize = 3;
    pub const SLOTS: usize = 39;
    /// One: `cost`.
    pub const DATA_SLOTS: usize = 1;
    /// The `container_set_data` id of `cost`.
    pub const DATA_COST: i16 = 0;
    /// `AnvilMenu.MAX_NAME_LENGTH`.
    pub const MAX_NAME_LENGTH: usize = 50;
}
