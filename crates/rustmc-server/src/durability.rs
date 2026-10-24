//! `ItemStack.hurtAndBreak` — item damage, and what breaking one puts on the
//! wire.
//!
//! # The chain, in the order the jar has it
//!
//! ```text
//!   Item.mineBlock(stack, level, state, pos, entity)
//!      0-18   tool = stack.get(TOOL);  if (tool == null) return FALSE
//!     26-35   state.getDestroySpeed(level, pos) != 0
//!     38-43   tool.damagePerBlock() > 0
//!     46-57     stack.hurtAndBreak(damagePerBlock, entity, MAINHAND)
//!        60   return true
//!
//!   ItemStack.hurtAndBreak(amount, level, player, onBreak)
//!      0-7    n = processDurabilityChange(amount, level, player)
//!     9-25    if (n != 0) applyDamage(getDamageValue() + n, player, onBreak)
//!
//!   processDurabilityChange(amount, level, player)
//!      0-8    !isDamageableItem()                     -> 0
//!     9-21    player != null && hasInfiniteMaterials() -> 0
//!    22-32    amount > 0 -> EnchantmentHelper.processDurabilityChange(...)
//!       33    else amount
//!
//!   applyDamage(damage, player, onBreak)
//!      0-13   CriteriaTriggers.ITEM_DURABILITY_CHANGED.trigger(...)
//!     13-17   setDamageValue(damage)
//!     18-22   if (isBroken())
//!     31-33     shrink(1)
//!     36-39     onBreak.accept(item)  -> LivingEntity.onEquippedItemBroken
//! ```
//!
//! # Four things a reading gets wrong
//!
//! * **An item with no `minecraft:tool` component makes `Item.mineBlock`
//!   return `false`**, not `true` — offsets 12-18. `ItemStack.mineBlock`
//!   awards the `ITEM_USED` statistic only on `true`, so digging with a sword
//!   awards nothing at all. That is a return value, not an omission.
//! * **`getDamageValue` clamps on the way out as well as in.**
//!   `Mth.clamp(get(DAMAGE), 0, getMaxDamage())` at offsets 17-22, and
//!   `setDamageValue` clamps the same way. A damage component above the
//!   maximum reads back as the maximum rather than as itself.
//! * **`isBroken()` is `damage >= maxDamage`, and `shrink(1)` follows it** —
//!   so an item breaks *at* its maximum damage, having taken maxDamage hits,
//!   and the stack loses one. `nextDamageWillBreak` is the `>= max - 1`
//!   form and is a different question.
//! * **Nothing plays a sound.** `applyDamage` sends no packet at all beyond
//!   the entity event `onEquippedItemBroken` broadcasts; the break sound is
//!   the client's own response to that event. A server that sent one would
//!   be sending a packet vanilla does not.
//!
//! # Unbreaking
//!
//! `EnchantmentHelper.processDurabilityChange` runs a `MutableFloat` through
//! every enchantment on the stack. It is `crate::item_damage`, passed in
//! through [`hurt_and_break_with`]; [`hurt_and_break`] is the identity step,
//! exact for every unenchanted stack.

use crate::itemprops::ItemProps;

/// `EquipmentSlot.MAINHAND`'s equipment-break entity event.
///
/// **Read out of `data/vanilla/item_props.json`, not written here.**
/// `LivingEntity.entityEventForEquipmentBreak` is a `tableswitch` over a
/// `$SwitchMap` whose numbering is the compiler's, so the listing's arm order
/// is not the slot order: transcribing it by pairing the arms
/// `47, 48, 49, 50, 52, 51, 65, 68` with `EquipmentSlot.values()` gets
/// **three of the eight wrong** — FEET is 52 and not 49, HEAD is 49 and not
/// 51, CHEST is 50 and not 52. The dumper invokes the method per slot
/// instead.
pub const MAINHAND: &str = "MAINHAND";

/// What one damage event does to a stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hurt {
    /// `processDurabilityChange` returned 0: the item is not damageable, or
    /// the player has infinite materials. **Nothing at all happens** — no
    /// component write, no packet, no trigger.
    Nothing,
    /// `applyDamage` ran and the item survived. The stack's
    /// `minecraft:damage` component is now `damage`.
    Damaged { damage: i32 },
    /// `isBroken()` — the stack is shrunk by one and
    /// `LivingEntity.onEquippedItemBroken` broadcasts `entity_event`
    /// [`Self::Broken::entity_event`] for the slot.
    ///
    /// `damage` is still carried: `setDamageValue` runs **before** the break
    /// check, so a stack of two that breaks one leaves a second at the
    /// maximum damage rather than a fresh one.
    Broken { damage: i32, entity_event: i32 },
}

/// `ItemStack.hurtAndBreak(amount, ...)` for a stack of `item` at `damage`.
///
/// `amount` is what reaches `applyDamage`, i.e. **after**
/// `EnchantmentHelper.processDurabilityChange` — see the module note. For an
/// unenchanted stack the two are equal.
///
/// `infinite_materials` is `ServerPlayer.hasInfiniteMaterials()`, which is
/// creative mode. It is checked in `processDurabilityChange` at offsets 9-21,
/// **before** the amount is looked at, so a creative player's tools never
/// take damage and never fire the advancement trigger either.
#[must_use]
pub fn hurt_and_break(
    props: Option<&ItemProps>,
    item: &str,
    damage: i32,
    amount: i32,
    infinite_materials: bool,
) -> Hurt {
    hurt_and_break_with(props, item, damage, amount, infinite_materials, |a| a)
}

/// [`hurt_and_break`] with `processDurabilityChange`'s enchantment step:
/// `enchant` is `EnchantmentHelper.processDurabilityChange`, run at
/// offsets 22-32 only for a positive amount on a damageable item a
/// non-creative player holds — so a stack that cannot take damage draws
/// nothing. See `crate::item_damage`.
#[must_use]
pub fn hurt_and_break_with(
    props: Option<&ItemProps>,
    item: &str,
    damage: i32,
    amount: i32,
    infinite_materials: bool,
    enchant: impl FnOnce(i32) -> i32,
) -> Hurt {
    let Some(props) = props else {
        return Hurt::Nothing;
    };
    // `processDurabilityChange` offsets 0-8.
    if !props.is_damageable_item(item) {
        return Hurt::Nothing;
    }
    // offsets 9-21.
    if infinite_materials {
        return Hurt::Nothing;
    }
    // offsets 22-33: `amount > 0 ? EnchantmentHelper.processDurabilityChange(..) : amount`.
    let amount = if amount > 0 { enchant(amount) } else { amount };
    // offsets 9-11 of `hurtAndBreak`: a zero change does nothing, and it is
    // the *change* that is tested, not the original amount.
    if amount == 0 {
        return Hurt::Nothing;
    }
    let max = props.max_damage(item);
    // `setDamageValue` -> `Mth.clamp(damage, 0, getMaxDamage())`.
    let next = (damage + amount).clamp(0, max);
    // `isBroken()` — `damage >= maxDamage`, checked after the write.
    if next >= max {
        Hurt::Broken {
            damage: next,
            entity_event: props.equipment_break_event(MAINHAND).unwrap_or(0),
        }
    } else {
        Hurt::Damaged { damage: next }
    }
}

/// `Item.mineBlock` — how much damage breaking one block costs this item, or
/// `None` when the item takes none.
///
/// `None` covers three different vanilla answers and the distinction does not
/// reach the caller because none of them writes anything: no
/// `minecraft:tool` component at all (and `mineBlock` then returns `false`,
/// awarding no statistic), a `damage_per_block` of zero, or a block whose
/// `getDestroySpeed` is zero.
///
/// `destroy_speed` is the caller's `state.getDestroySpeed(level, pos)`.
#[must_use]
pub fn mine_block_damage(props: Option<&ItemProps>, item: &str, destroy_speed: f32) -> Option<i32> {
    let tool = props?.tool(item)?;
    if destroy_speed == 0.0 {
        return None;
    }
    // `Tool.damagePerBlock() > 0` — offsets 38-43. The default is 1 and a
    // tool may set it to 0, which is how a few items mine without wearing.
    (tool.damage_per_block > 0).then_some(tool.damage_per_block)
}

/// `LivingEntity.onEquippedItemBroken`'s body — `broadcastEntityEvent(entity,
/// event)`, which is `ClientboundEntityEventPacket`: an **int** entity id and
/// a single byte.
///
/// Not a varint. `ClientboundEntityEventPacket`'s codec is `writeInt` then
/// `writeByte`, five bytes flat.
#[must_use]
pub fn entity_event_body(entity_id: i32, event: i32) -> Vec<u8> {
    let mut body = Vec::with_capacity(5);
    body.extend_from_slice(&entity_id.to_be_bytes());
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the event is a byte in the jar; every value in \
                  item_props.json's equipmentBreakEvents is 47..=68"
    )]
    body.push(event as u8);
    body
}

#[cfg(test)]
mod tests;
