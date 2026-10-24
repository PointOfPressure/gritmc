//! Against the extract, and against the four orderings it cannot state.

use super::*;

fn props() -> Option<ItemProps> {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push(crate::itemprops::TABLE_PATH);
    ItemProps::load(&path).ok()
}

/// **The eight slot events, and three of them are not what the listing's arm
/// order says.**
///
/// `entityEventForEquipmentBreak`'s `tableswitch` reads
/// `47, 48, 49, 50, 52, 51, 65, 68`. Pairing those with
/// `EquipmentSlot.values()` — MAINHAND, OFFHAND, FEET, LEGS, CHEST, HEAD,
/// BODY, SADDLE — gives FEET 49, LEGS 50, CHEST 52, HEAD 51. Vanilla's own
/// answer is FEET 52, LEGS 51, CHEST 50, HEAD 49: **three of the eight
/// wrong**, and the two that happen to agree are the ones a test of MAINHAND
/// alone would have checked.
#[test]
fn the_equipment_break_events_are_not_the_listings_arm_order() {
    let Some(p) = props() else { return };
    assert_eq!(p.equipment_break_event("MAINHAND"), Some(47));
    assert_eq!(p.equipment_break_event("OFFHAND"), Some(48));
    assert_eq!(p.equipment_break_event("HEAD"), Some(49));
    assert_eq!(p.equipment_break_event("CHEST"), Some(50));
    assert_eq!(p.equipment_break_event("LEGS"), Some(51));
    assert_eq!(p.equipment_break_event("FEET"), Some(52));
    assert_eq!(p.equipment_break_event("BODY"), Some(65));
    assert_eq!(p.equipment_break_event("SADDLE"), Some(68));
    assert_eq!(p.equipment_break_event("NOT_A_SLOT"), None);
}

/// A shovel wears down one point at a time and breaks **at** its maximum,
/// having taken `maxDamage` hits.
#[test]
fn a_shovel_takes_damage_and_breaks_at_its_maximum() {
    let Some(p) = props() else { return };
    let max = p.max_damage("minecraft:iron_shovel");
    assert!(max > 0, "an iron shovel is damageable");
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", 0, 1, false),
        Hurt::Damaged { damage: 1 }
    );
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", max - 2, 1, false),
        Hurt::Damaged { damage: max - 1 }
    );
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", max - 1, 1, false),
        Hurt::Broken {
            damage: max,
            entity_event: 47
        }
    );
}

/// **Creative is checked before the amount is looked at.** A creative
/// player's tool never takes damage, never breaks and never fires the
/// advancement trigger — `processDurabilityChange` offsets 9-21, above the
/// `amount > 0` test at 22.
#[test]
fn an_infinite_materials_player_never_damages_anything() {
    let Some(p) = props() else { return };
    let max = p.max_damage("minecraft:iron_shovel");
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", max - 1, 1, true),
        Hurt::Nothing
    );
}

/// An item with no `minecraft:max_damage` is not damageable and nothing
/// happens to it. Both halves, because a rule that damaged everything would
/// pass the first assertion of the test above on its own.
#[test]
fn a_block_item_is_not_damageable() {
    let Some(p) = props() else { return };
    assert!(!p.is_damageable_item("minecraft:stone"));
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:stone", 0, 1, false),
        Hurt::Nothing
    );
}

/// `hurtAndBreak` offsets 9-11: the test is on the **change**, so a zero
/// change writes nothing at all rather than writing the same value back.
#[test]
fn a_zero_change_writes_nothing() {
    let Some(p) = props() else { return };
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", 5, 0, false),
        Hurt::Nothing
    );
}

/// `setDamageValue` clamps, so an amount past the maximum lands exactly on
/// it rather than beyond — which matters because the stack a break leaves
/// behind carries that value.
#[test]
fn damage_is_clamped_at_the_maximum() {
    let Some(p) = props() else { return };
    let max = p.max_damage("minecraft:iron_shovel");
    assert_eq!(
        hurt_and_break(Some(&p), "minecraft:iron_shovel", 0, max * 4, false),
        Hurt::Broken {
            damage: max,
            entity_event: 47
        }
    );
}

/// `Item.mineBlock`'s three ways to cost nothing, kept apart from the one
/// way it costs something.
#[test]
fn only_a_tool_on_a_breakable_block_costs_durability() {
    let Some(p) = props() else { return };
    assert_eq!(
        mine_block_damage(Some(&p), "minecraft:iron_shovel", 1.5),
        Some(1)
    );
    assert_eq!(
        mine_block_damage(Some(&p), "minecraft:iron_shovel", 0.0),
        None,
        "getDestroySpeed == 0 — bedrock costs a tool nothing"
    );
    assert_eq!(
        mine_block_damage(Some(&p), "minecraft:stone", 1.5),
        None,
        "no minecraft:tool component at all, and mineBlock returns FALSE for it"
    );
    assert_eq!(mine_block_damage(None, "minecraft:iron_shovel", 1.5), None);
}

/// `ClientboundEntityEventPacket` is a plain big-endian int and one byte —
/// five bytes, no varint. A varint id would be one byte for a small entity id
/// and the packet would still parse as *something*, which is why this is a
/// vector rather than a reading.
#[test]
fn the_entity_event_body_is_five_flat_bytes() {
    assert_eq!(entity_event_body(1, 47), vec![0, 0, 0, 1, 47]);
    assert_eq!(entity_event_body(300, 68), vec![0, 0, 1, 44, 68]);
}
