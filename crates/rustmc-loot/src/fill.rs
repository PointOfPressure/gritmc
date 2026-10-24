//! `LootTable.fill` — how a rolled list becomes a chest's slots.
//!
//! Rolling the table is only the first third of filling a container. The
//! other two thirds draw from the **same** generator and are not visible in
//! any loot table's JSON, from `javap -p -c -constants` on the pinned jar:
//!
//! ```text
//! fill(Container, LootParams, long seed)
//!    0: ctx   = new LootContext.Builder(params).withOptionalRandomSeed(seed).create(randomSequence)
//!   21: items = getRandomItems(ctx)            -- the roll, through createStackSplitter
//!   29: rnd   = ctx.getRandom()                -- the SAME generator the roll advanced
//!   36: slots = getAvailableSlots(container, rnd)
//!   45: shuffleAndSplitItems(items, slots.size(), rnd)
//!   67: for each stack: slots empty ? (warn "Tried to over-fill a container"; return)
//!                       : container.setItem(slots.remove(slots.size() - 1), stack)
//! ```
//!
//! `createStackSplitter(level, consumer)` (the lambda at its offset 0): a
//! stack whose item is not enabled is dropped; one whose count is below its
//! max stack size passes through **including the empty stack** (count 0);
//! anything else is cut into `min(maxStackSize, remaining)` pieces.
//!
//! `getAvailableSlots`: every index whose current item `isEmpty()`, in order,
//! then `Util.shuffle(list, random)`.
//!
//! `shuffleAndSplitItems(list, emptySlots, random)`:
//!
//! ```text
//!    5: for it in list: isEmpty -> it.remove(); else count > 1 -> splits.add, it.remove()
//!   80: while (emptySlots - list.size() - splits.size() > 0 && !splits.isEmpty())
//!  107:     stack = splits.remove(Mth.nextInt(random, 0, splits.size() - 1))
//!  133:     n     = Mth.nextInt(random, 1, stack.getCount() / 2)
//!  147:     split = stack.split(n)
//!  156:     stack.count > 1 && random.nextBoolean() ? splits.add(stack) : list.add(stack)
//!  194:     split.count > 1 && random.nextBoolean() ? splits.add(split) : list.add(split)
//!  235: list.addAll(splits); Util.shuffle(list, random)
//! ```
//!
//! The `&&` is load-bearing: `nextBoolean` is **not** drawn for a count of 1.
//! `Util.shuffle` walks `i = size .. 2` drawing `nextInt(i)` and swapping
//! `i - 1` with the draw — Fisher-Yates from the top, which is not
//! `Collections.shuffle`'s draw sequence over a `java.util.Random`.

use rustmc_worldgen::rng::RandomSource;

use crate::Stack;

/// `Mth.nextInt(RandomSource, int, int)`: `min >= max ? min :
/// random.nextInt(max - min + 1) + min`. The guard means a degenerate range
/// does **not** draw, which `RandomSource::next_i32_between_inclusive` would.
pub fn mth_next_int(rng: &mut dyn RandomSource, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        rng.next_i32_bound(max - min + 1) + min
    }
}

/// `Util.shuffle(List, RandomSource)`.
pub fn util_shuffle<T>(list: &mut [T], rng: &mut dyn RandomSource) {
    let mut i = list.len();
    while i > 1 {
        let j = rng.next_i32_bound(i as i32) as usize;
        list.swap(i - 1, j);
        i -= 1;
    }
}

/// `LootTable.createStackSplitter`'s body over a rolled list, in order.
///
/// `max_stack` answers `ItemStack.getMaxStackSize()` for an item id. The
/// feature-flag arm is not modelled: every item a shipped loot table names is
/// enabled under the default flag set, and an item that were not would be a
/// data error this crate should not paper over.
pub fn split_stacks(raw: Vec<Stack>, max_stack: &dyn Fn(&str) -> i32) -> Vec<Stack> {
    let mut out = Vec::with_capacity(raw.len());
    for s in raw {
        let max = if s.count <= 0 { 64 } else { max_stack(&s.item) };
        if s.count < max {
            out.push(s);
            continue;
        }
        let mut left = s.count;
        while left > 0 {
            let n = max.min(left);
            out.push(Stack {
                count: n,
                ..s.clone()
            });
            left -= n;
        }
    }
    out
}

/// Deliberate faults, for `tests/falsify_loot.rs` only: each is a plausible
/// misreading of the bytecode above, and the fill golden must go red under
/// every one. Always default outside the falsifier.
#[derive(Debug, Clone, Copy, Default)]
pub struct FillFaults {
    /// Draw `nextBoolean` for a piece of count 1 too — reading the `&&` at
    /// offsets 156/194 as two independent statements.
    pub draw_bool_for_singletons: bool,
    /// Walk `Util.shuffle` from the bottom (`i = 2 .. size`) instead of the top.
    pub shuffle_ascending: bool,
    /// Take the available slots in index order instead of popping from the
    /// end of the shuffled list (offset 130: `slots.remove(size - 1)`).
    pub pop_from_front: bool,
}

fn shuffle_with<T>(list: &mut [T], rng: &mut dyn RandomSource, f: FillFaults) {
    if !f.shuffle_ascending {
        util_shuffle(list, rng);
        return;
    }
    for i in 2..=list.len() {
        let j = rng.next_i32_bound(i as i32) as usize;
        list.swap(i - 1, j);
    }
}

/// `LootTable.getAvailableSlots`: the empty slots, shuffled.
pub fn available_slots(container: &[Option<Stack>], rng: &mut dyn RandomSource) -> Vec<usize> {
    available_slots_with(container, rng, FillFaults::default())
}

fn available_slots_with(
    container: &[Option<Stack>],
    rng: &mut dyn RandomSource,
    f: FillFaults,
) -> Vec<usize> {
    let mut v: Vec<usize> = container
        .iter()
        .enumerate()
        .filter(|(_, s)| s.as_ref().is_none_or(|s| s.count <= 0))
        .map(|(i, _)| i)
        .collect();
    shuffle_with(&mut v, rng, f);
    v
}

/// `LootTable.shuffleAndSplitItems`.
pub fn shuffle_and_split(list: &mut Vec<Stack>, empty_slots: usize, rng: &mut dyn RandomSource) {
    shuffle_and_split_with(list, empty_slots, rng, FillFaults::default());
}

fn shuffle_and_split_with(
    list: &mut Vec<Stack>,
    empty_slots: usize,
    rng: &mut dyn RandomSource,
    f: FillFaults,
) {
    let mut splits: Vec<Stack> = Vec::new();
    let mut kept = Vec::with_capacity(list.len());
    for s in list.drain(..) {
        if s.count <= 0 {
            continue;
        }
        if s.count > 1 {
            splits.push(s);
        } else {
            kept.push(s);
        }
    }
    *list = kept;
    while empty_slots as i64 - list.len() as i64 - splits.len() as i64 > 0 && !splits.is_empty() {
        let at = mth_next_int(rng, 0, splits.len() as i32 - 1) as usize;
        let mut stack = splits.remove(at);
        let n = mth_next_int(rng, 1, stack.count / 2);
        // `ItemStack.split(n)`: min(n, count) taken off the original.
        let taken = n.min(stack.count);
        let split = Stack {
            count: taken,
            ..stack.clone()
        };
        stack.count -= taken;
        let keep = |n: i32, rng: &mut dyn RandomSource| {
            if f.draw_bool_for_singletons {
                let b = rng.next_bool();
                n > 1 && b
            } else {
                n > 1 && rng.next_bool()
            }
        };
        if keep(stack.count, rng) {
            splits.push(stack);
        } else {
            list.push(stack);
        }
        if keep(split.count, rng) {
            splits.push(split);
        } else {
            list.push(split);
        }
    }
    list.append(&mut splits);
    shuffle_with(list, rng, f);
}

/// The tail of `LootTable.fill` after the roll: place `rolled` (already
/// through [`split_stacks`]) into `container`, drawing from `rng` — the same
/// generator the roll used.
///
/// A slot whose stack is empty is written empty, which `fill` does
/// explicitly (offset 118) rather than skipping; it still consumes a slot.
pub fn place(rolled: Vec<Stack>, container: &mut [Option<Stack>], rng: &mut dyn RandomSource) {
    place_with(rolled, container, rng, FillFaults::default());
}

/// [`place`] with faults injected. For the falsifier.
pub fn place_with(
    mut rolled: Vec<Stack>,
    container: &mut [Option<Stack>],
    rng: &mut dyn RandomSource,
    f: FillFaults,
) {
    let mut slots = available_slots_with(container, rng, f);
    shuffle_and_split_with(&mut rolled, slots.len(), rng, f);
    if f.pop_from_front {
        slots.reverse();
    }
    for stack in rolled {
        let Some(slot) = slots.pop() else {
            // "Tried to over-fill a container" — a warning and a return.
            return;
        };
        container[slot] = if stack.count <= 0 { None } else { Some(stack) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmc_worldgen::rng::LegacyRandomSource;

    /// `Mth.nextInt` does not draw on a degenerate range; a port through
    /// `next_i32_between_inclusive` would, and every later slot would move.
    #[test]
    fn a_degenerate_range_draws_nothing() {
        let mut a = LegacyRandomSource::new(7);
        let mut b = LegacyRandomSource::new(7);
        assert_eq!(mth_next_int(&mut a, 3, 3), 3);
        assert_eq!(a.next_i64(), b.next_i64());
    }

    /// Splitting conserves every item: whatever the draws, the counts that
    /// land equal the counts rolled, as long as there is room.
    #[test]
    fn shuffle_and_split_conserves_the_total() {
        for seed in 0..200 {
            let mut rng = LegacyRandomSource::new(seed);
            let mut list = vec![
                Stack::new("minecraft:bread", 5),
                Stack::new("minecraft:air", 0),
                Stack::new("minecraft:iron_ingot", 1),
                Stack::new("minecraft:wheat", 17),
            ];
            shuffle_and_split(&mut list, 27, &mut rng);
            let total: i32 = list.iter().map(|s| s.count).sum();
            assert_eq!(total, 23, "seed {seed}");
            assert!(list.len() <= 27);
            assert!(list.iter().all(|s| s.count > 0));
        }
    }
}
