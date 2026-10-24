//! `/give` from the server console — `GiveCommand.giveItem`, as far as a
//! survival rig needs it.
//!
//! # Why the survival corpus needs it at all
//!
//! A survival harness player cannot `hold`: `handleSetCreativeModeSlot`
//! refuses the packet when the player is not creative, silently. So without
//! this a survival scenario can dig only with bare hands, and only what the
//! superflat puts under its feet — no tool tier, no correct-tool divisor, no
//! durability, no block that needs placing first. The console is full
//! permission on both servers, so `give oracle minecraft:wooden_pickaxe` is
//! symmetric the way every other console step is.
//!
//! # What `giveItem` does, and which half of it is here
//!
//! ```text
//!    0-58   stack = input.createItemStack(count); refuse > 100 stacks
//!   91-176  per target, per max-stack chunk:
//!             added = inventory.add(copy)
//!             added && copy.isEmpty():
//!   183-202     fake = player.drop(copy, false); fake.makeFakeItem()
//!   204-259     level.playSound(null, x, y, z, ITEM_PICKUP, PLAYERS, 0.2F,
//!                               ((r.nextFloat() - r.nextFloat()) * 0.7F + 1) * 2)
//!   267         containerMenu.broadcastChanges()
//!             else: drop the rest at the player's feet, pickup delay cleared
//! ```
//!
//! The inventory write and the slot packet are here. **The fake item and the
//! pickup sound are not**: a `makeFakeItem` entity lives one tick and is
//! announced and removed inside a scenario's `[[setup]]`, whose observations
//! the recorder discards, and every scenario that gives an item gives it in
//! setup. A check that gave in a measured step would see vanilla's
//! `add_entity`/`remove_entities`/`sound` and this server's silence, and
//! would be right to call that a divergence.
//!
//! # The grammar
//!
//! `give <player> <item>[<components>] [<count>]`, with the item id's
//! namespace optional as in vanilla. One component is understood —
//! `damage=<n>` — because it is the one digging reads back; any other
//! component is refused with a message rather than dropped, since a stack
//! silently missing a component its scenario asked for is a rig nobody built.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::item::ItemStack;

/// A parsed `give`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Give {
    /// The target player's name.
    pub target: String,
    /// The stack, count included.
    pub stack: ItemStack,
}

/// Parse `give <player> <item>[components] [count]`.
///
/// # Errors
/// A message naming what was wrong, in the console's own voice.
pub fn parse(line: &str) -> Result<Give, String> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("give") {
        return Err(format!("give: not a give command: {line:?}"));
    }
    let target = parts.next().ok_or("give: no target player")?.to_string();
    let spec = parts.next().ok_or("give: no item")?;
    let (id, components) = match spec.split_once('[') {
        Some((id, rest)) => {
            let body = rest
                .strip_suffix(']')
                .ok_or_else(|| format!("give: unterminated components in {spec:?}"))?;
            (id, Some(body))
        }
        None => (spec, None),
    };
    let item = if id.contains(':') {
        id.to_string()
    } else {
        format!("minecraft:{id}")
    };
    let count = match parts.next() {
        Some(c) => c
            .parse::<i32>()
            .map_err(|_| format!("give: count {c:?} is not an integer"))?,
        None => 1,
    };
    // `IntegerArgumentType.integer(1)` on the count argument.
    if count < 1 {
        return Err(format!("give: count must be at least 1, got {count}"));
    }
    if let Some(extra) = parts.next() {
        return Err(format!("give: unexpected {extra:?}"));
    }
    let mut stack = ItemStack::new(&item, count);
    for entry in top_level(components.unwrap_or("")) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| format!("give: component {entry:?} has no value"))?;
        let key = key.trim().trim_start_matches("minecraft:");
        let value = value.trim();
        match key {
            "damage" => {
                let damage = value
                    .parse::<i32>()
                    .map_err(|_| format!("give: damage {value:?} is not an integer"))?;
                stack = stack.with_damage(damage);
            }
            // `ItemEnchantments.CODEC` is an unbounded map from enchantment
            // id to a level in 1..=255 (`Codec.intRange(1, 255)`).
            "enchantments" => {
                let body = value
                    .strip_prefix('{')
                    .and_then(|v| v.strip_suffix('}'))
                    .ok_or_else(|| format!("give: enchantments {value:?} is not a {{...}} map"))?;
                for pair in top_level(body) {
                    let (id, level) = pair
                        .split_once(':')
                        .ok_or_else(|| format!("give: enchantment {pair:?} has no level"))?;
                    // A namespaced key is quoted: `"minecraft:efficiency":5`.
                    let (id, level) = if id.trim().starts_with('"') {
                        let rest = pair.trim().trim_start_matches('"');
                        let (id, level) = rest
                            .split_once("\":")
                            .ok_or_else(|| format!("give: enchantment {pair:?} is malformed"))?;
                        (id.to_string(), level.to_string())
                    } else {
                        (id.trim().to_string(), level.to_string())
                    };
                    let level = level.trim().parse::<i32>().map_err(|_| {
                        format!("give: enchantment level {level:?} is not an integer")
                    })?;
                    if !(1..=255).contains(&level) {
                        return Err(format!(
                            "give: enchantment level {level} is outside 1..=255"
                        ));
                    }
                    stack = stack.with_enchantment(&id, level);
                }
            }
            other => {
                return Err(format!(
                    "give: component {other:?} is not understood here (only damage and \
                     enchantments are)"
                ))
            }
        }
    }
    Ok(Give { target, stack })
}

/// Split on the commas that are not inside a `{...}`.
fn top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// One inventory command waiting for its player's loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    /// `GiveCommand.giveItem`'s `Inventory.add`.
    Give(ItemStack),
    /// `ClearInventoryCommands.clearUnlimited` with no item predicate:
    /// `Inventory.clearOrCountMatchingItems(always, -1, craftSlots)`, then
    /// `containerMenu.broadcastChanges()`.
    Clear,
}

/// Commands waiting for their player's own loop, by player name.
///
/// The console runs on its own thread and the inventory belongs to the
/// player's connection thread, so the command is queued here and applied on
/// that thread's next wake — at most one tick later, which is where vanilla's
/// `handleConsoleInputs` runs it too.
fn pending() -> &'static Mutex<BTreeMap<String, Vec<Op>>> {
    static PENDING: OnceLock<Mutex<BTreeMap<String, Vec<Op>>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Parse a console line and queue its stack for the named player.
///
/// # Errors
/// The parse error, or `No player was found` for a name nobody holds — the
/// vanilla message, `argument.entity.notfound.player`'s English text.
pub fn dispatch(line: &str, players: &crate::players::Players) -> Result<(), String> {
    let give = parse(line)?;
    if !players
        .connections()
        .iter()
        .any(|c| c.name() == give.target)
    {
        return Err("No player was found".to_string());
    }
    pending()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(give.target)
        .or_default()
        .push(Op::Give(give.stack));
    Ok(())
}

/// `clear <player>` — the whole inventory, no item predicate, no count.
///
/// # Errors
/// A malformed line, or `No player was found`.
pub fn dispatch_clear(line: &str, players: &crate::players::Players) -> Result<(), String> {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("clear") {
        return Err(format!("clear: not a clear command: {line:?}"));
    }
    let target = parts.next().ok_or("clear: no target player")?;
    if let Some(extra) = parts.next() {
        return Err(format!(
            "clear: an item predicate ({extra:?}) is not understood here"
        ));
    }
    if !players.connections().iter().any(|c| c.name() == target) {
        return Err("No player was found".to_string());
    }
    pending()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(target.to_string())
        .or_default()
        .push(Op::Clear);
    Ok(())
}

/// Take whatever has been queued for `name` since the last call, in order.
#[must_use]
pub fn take(name: &str) -> Vec<Op> {
    pending()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(name)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_id_takes_the_minecraft_namespace_and_a_count_of_one() {
        let g = parse("give oracle wooden_pickaxe").expect("parses");
        assert_eq!(g.target, "oracle");
        assert_eq!(g.stack, ItemStack::new("minecraft:wooden_pickaxe", 1));
    }

    #[test]
    fn a_count_and_a_damage_component_are_read() {
        let g = parse("give oracle minecraft:diamond_pickaxe[damage=1560] 1").expect("parses");
        assert_eq!(g.stack.item(), "minecraft:diamond_pickaxe");
        assert_eq!(g.stack.damage(), 1560);
        let g = parse("give oracle stone 5").expect("parses");
        assert_eq!(g.stack.count(), 5);
        assert_eq!(g.stack.damage(), 0);
    }

    #[test]
    fn enchantments_are_a_map_and_sit_beside_damage() {
        let g = parse("give oracle diamond_pickaxe[enchantments={efficiency:5},damage=3]")
            .expect("parses");
        assert_eq!(g.stack.enchantment_level("minecraft:efficiency"), 5);
        assert_eq!(g.stack.damage(), 3);
        let g = parse(
            r#"give oracle diamond_shovel[enchantments={"minecraft:silk_touch":1,efficiency:2}]"#,
        )
        .expect("parses");
        assert_eq!(g.stack.enchantment_level("silk_touch"), 1);
        assert_eq!(g.stack.enchantment_level("efficiency"), 2);
        assert!(parse("give oracle stick[enchantments={efficiency:0}]").is_err());
        assert!(parse("give oracle stick[enchantments=efficiency]").is_err());
    }

    #[test]
    fn anything_it_cannot_honour_is_refused_not_dropped() {
        assert!(parse("give oracle stick[custom_name='x']").is_err());
        assert!(parse("give oracle stick 0").is_err());
        assert!(parse("give oracle stick two").is_err());
        assert!(parse("give oracle").is_err());
        assert!(parse("give oracle stick[damage=1").is_err());
        assert!(parse("give oracle stick 1 extra").is_err());
    }

    #[test]
    fn a_queued_stack_is_taken_once() {
        pending()
            .lock()
            .expect("lock")
            .entry("tester_take_once".into())
            .or_default()
            .push(Op::Give(ItemStack::one("minecraft:stone")));
        assert_eq!(take("tester_take_once").len(), 1);
        assert!(take("tester_take_once").is_empty());
    }
}
