//! The workstation menus' computed half — `crate::workstation` — driven
//! through the same click path the server runs.
//!
//! Each test names the bytecode branch it pins and carries a control that
//! would have to change if the branch were faked by a constant: two recipe
//! buttons that give different results, two damage values that give
//! different repairs, a press that must change nothing beside one that must.

use rustmc_server::click::{
    handle_button_click, handle_click, ClickPacket, ContainerInput, Menu, Reply, Slots,
};
use rustmc_server::item::{ComponentKind, ComponentValue, ItemStack};
use rustmc_server::menu::menu_for_state;
use rustmc_server::player::Hotbar;
use rustmc_server::workstation::{Effect, Env};

fn repo(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel)
}

fn props() -> Option<&'static rustmc_server::itemprops::ItemProps> {
    let p = repo(rustmc_server::itemprops::TABLE_PATH);
    if !rustmc_data::fixture::available(
        &p,
        "data/vanilla/item_props.json",
        "ci/item-props-golden.sh",
    ) {
        return None;
    }
    Some(rustmc_server::itemprops::ItemProps::shared(&p).expect("item_props.json"))
}

fn registry() -> Option<&'static rustmc_server::enchantment::Registry> {
    let p = repo(rustmc_server::enchantment::TABLE_PATH);
    if !rustmc_data::fixture::available(
        &p,
        "data/vanilla/enchantments.json",
        "ci/enchantments-golden.sh",
    ) {
        return None;
    }
    rustmc_server::enchantment::Registry::load_shared(&p)
}

fn click(menu: &Menu, slot: i32) -> ClickPacket {
    ClickPacket {
        container_id: menu.container_id,
        state_id: menu.state_id,
        slot_num: slot,
        button: 0,
        input: ContainerInput::Pickup,
        changed_slots: Vec::new(),
        hashed_slots: Vec::new(),
        carried: ItemStack::empty(),
    }
}

/// Two stonecutting recipes over stone, written in the file **out of id
/// order**, so a list in load order and a list in id order disagree about
/// which is button 0.
fn two_stone_recipes() -> rustmc_server::recipes::Recipes {
    let recipes = serde_json::json!({
        "stone_slab_from_stone_stonecutting": {
            "type": "minecraft:stonecutting",
            "ingredient": "minecraft:stone",
            "result": {"id": "minecraft:stone_slab", "count": 2}
        },
        "cobblestone_from_stone_stonecutting": {
            "type": "minecraft:stonecutting",
            "ingredient": "minecraft:stone",
            "result": {"id": "minecraft:cobblestone"}
        }
    });
    rustmc_server::recipes::Recipes::parse(&recipes, &serde_json::json!({})).expect("recipes")
}

fn data_replies(replies: &[Reply]) -> Vec<(i16, i32)> {
    replies
        .iter()
        .filter_map(|r| match r {
            Reply::SetData { id, value, .. } => Some((*id, *value)),
            _ => None,
        })
        .collect()
}

fn slot_reply(replies: &[Reply], slot: i16) -> Option<ItemStack> {
    replies.iter().find_map(|r| match r {
        Reply::SetSlot { slot: s, stack, .. } if *s == slot => Some(stack.clone()),
        _ => None,
    })
}

/// `StonecutterMenu`: a stone in the input resets the selection to -1
/// (`setupRecipeList`), a button assembles that entry of the list **in
/// recipe-id order**, the same button again is `return false`, an
/// out-of-range button is `return true` with nothing changed, and taking the
/// result spends the stone and plays the take sound.
#[test]
fn a_stonecutter_selects_by_id_order_and_spends_its_input() {
    let recipes = two_stone_recipes();
    let spec = menu_for_state("minecraft:stonecutter").expect("stonecutter");
    let mut menu = Menu::new(3, spec);
    let mut inv = Hotbar::new();
    inv.set_stack(0, ItemStack::one("minecraft:stone"));
    let mut slots = Slots {
        menu: &mut menu,
        inventory: &mut inv,
        container: None,
        recipes: Some(&recipes),
        second: None,
        env: Env {
            pos: Some((1, 0, 0)),
            game_time: 100,
            creative: true,
            ..Env::default()
        },
    };
    let _ = rustmc_server::click::send_all_data_to_remote(&mut slots);
    let _ = rustmc_server::click::send_all_data_slots(&mut slots);

    let up = click(slots.menu, 29);
    let _ = handle_click(&mut slots, &up);
    let down = click(slots.menu, 0);
    let out = handle_click(&mut slots, &down);
    assert_eq!(
        data_replies(&out.replies),
        vec![(0, -1)],
        "setupRecipeList sets -1"
    );

    // Button 0 is `cobblestone_from_stone_stonecutting` — first by id, last
    // in the file.
    let id = slots.menu.container_id;
    let r = handle_button_click(&mut slots, id, 0);
    assert_eq!(
        slot_reply(&r, 1),
        Some(ItemStack::new("minecraft:cobblestone", 1))
    );
    assert_eq!(data_replies(&r), vec![(0, 0)]);
    let r = handle_button_click(&mut slots, id, 1);
    assert_eq!(
        slot_reply(&r, 1),
        Some(ItemStack::new("minecraft:stone_slab", 2))
    );

    // The selected button again: `clickMenuButton` returns false, so no
    // broadcast at all.
    assert!(handle_button_click(&mut slots, id, 1).is_empty());
    // Past the list: true, and nothing moved.
    assert!(handle_button_click(&mut slots, id, 7).is_empty());
    assert_eq!(slots.menu.data[0], 1);

    // Take the slabs: the stone is spent, the selection resets, the sound
    // plays once at the block.
    let take = click(slots.menu, 1);
    let out = handle_click(&mut slots, &take);
    assert_eq!(slot_reply(&out.replies, 0), Some(ItemStack::empty()));
    assert_eq!(data_replies(&out.replies), vec![(0, -1)]);
    assert_eq!(
        slots.menu.carried,
        ItemStack::new("minecraft:stone_slab", 2)
    );
    assert!(matches!(
        slots.env.effects.as_slice(),
        [Effect::Sound(s)] if s.event == "minecraft:ui.stonecutter.take_result" && s.pitch == 1.0
    ));
}

fn damaged(item: &str, damage: i32) -> ItemStack {
    let mut s = ItemStack::one(item);
    s.set_component(ComponentKind::Damage, ComponentValue::Int(damage), true);
    s
}

/// `AnvilMenu.createResult`'s material arm through the menu: one ingot heals
/// `min(damage, 250 / 4)`, and two starting damages give two different
/// results, so neither the repair nor the cost can be a constant.
#[test]
fn an_anvil_repairs_by_a_quarter_and_publishes_the_cost() {
    let (Some(props), Some(registry)) = (props(), registry()) else {
        return;
    };
    for (start, want) in [(200, 138), (40, 0)] {
        let spec = menu_for_state("minecraft:anvil").expect("anvil");
        let mut menu = Menu::new(4, spec);
        let mut inv = Hotbar::new().with_item_props(Some(props));
        inv.set_stack(0, damaged("minecraft:iron_pickaxe", start));
        inv.set_stack(1, ItemStack::one("minecraft:iron_ingot"));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: None,
            second: None,
            env: Env {
                pos: Some((1, 0, 0)),
                creative: true,
                enchantments: Some(registry),
                ..Env::default()
            },
        };
        for s in [30, 0, 31, 1] {
            let c = click(slots.menu, s);
            let _ = handle_click(&mut slots, &c);
        }
        let result = slots.menu.grid[2].clone();
        assert_eq!(result.item(), "minecraft:iron_pickaxe");
        assert_eq!(result.damage_value(), want, "start {start}");
        assert_eq!(result.repair_cost(), 1);
        assert_eq!(slots.menu.data[0], 1);
        // Damage 0 is the prototype's, so a full repair leaves no damage
        // entry at all — `PatchedDataComponentMap.set` removes it.
        let has_damage = result
            .components()
            .entries()
            .iter()
            .any(|(k, _)| *k == ComponentKind::Damage);
        assert_eq!(has_damage, want != 0);

        let take = click(slots.menu, 2);
        let _ = handle_click(&mut slots, &take);
        assert!(
            slots.menu.grid.iter().all(ItemStack::is_empty),
            "both inputs spent"
        );
        assert_eq!(slots.menu.data[0], 0);
        assert!(matches!(
            slots.env.effects.as_slice(),
            [Effect::LevelEvent { event: 1030, .. }]
        ));
    }
}

/// `GrindstoneMenu.mergeItems`: five percent of bonus where the anvil gives
/// twelve, and a lone unenchanted tool makes nothing.
#[test]
fn a_grindstone_pools_durability_with_a_five_percent_bonus() {
    let (Some(props), Some(registry)) = (props(), registry()) else {
        return;
    };
    let spec = menu_for_state("minecraft:grindstone").expect("grindstone");
    let mut menu = Menu::new(5, spec);
    let mut inv = Hotbar::new().with_item_props(Some(props));
    inv.set_stack(0, damaged("minecraft:iron_pickaxe", 200));
    inv.set_stack(1, damaged("minecraft:iron_pickaxe", 100));
    inv.set_stack(2, ItemStack::one("minecraft:stone"));
    let mut slots = Slots {
        menu: &mut menu,
        inventory: &mut inv,
        container: None,
        recipes: None,
        second: None,
        env: Env {
            pos: Some((1, 0, 0)),
            creative: true,
            enchantments: Some(registry),
            ..Env::default()
        },
    };
    for s in [30, 0] {
        let c = click(slots.menu, s);
        let _ = handle_click(&mut slots, &c);
    }
    assert!(
        slots.menu.grid[2].is_empty(),
        "one plain tool alone makes nothing"
    );
    for s in [31, 1] {
        let c = click(slots.menu, s);
        let _ = handle_click(&mut slots, &c);
    }
    // 50 + 150 + 250 * 5 / 100 = 212 left, damage 38.
    assert_eq!(slots.menu.grid[2].damage_value(), 38);
    // A stone may not go in: it stays on the cursor.
    for s in [32, 0] {
        let c = click(slots.menu, s);
        let _ = handle_click(&mut slots, &c);
    }
    assert_eq!(slots.menu.carried, ItemStack::one("minecraft:stone"));
}

/// The patch keeps `Reference2ObjectArrayMap` order: a key set again stays
/// where it was, a new key goes on the end, and a value equal to the
/// prototype's leaves the patch altogether.
#[test]
fn a_component_patch_keeps_insertion_order_and_drops_prototype_values() {
    let mut s = damaged("minecraft:iron_pickaxe", 5);
    s.set_component(
        ComponentKind::CustomName,
        ComponentValue::Text("A".into()),
        true,
    );
    s.set_component(ComponentKind::Damage, ComponentValue::Int(9), true);
    let kinds: Vec<_> = s.components().entries().iter().map(|(k, _)| *k).collect();
    assert_eq!(
        kinds,
        vec![ComponentKind::Damage, ComponentKind::CustomName]
    );
    s.set_component(ComponentKind::RepairCost, ComponentValue::Int(0), true);
    s.set_component(ComponentKind::Damage, ComponentValue::Int(0), true);
    let kinds: Vec<_> = s.components().entries().iter().map(|(k, _)| *k).collect();
    assert_eq!(kinds, vec![ComponentKind::CustomName]);
    // And two stacks that differ only in a component no longer merge.
    assert!(!ItemStack::is_same_item_same_components(
        &damaged("minecraft:iron_pickaxe", 1),
        &damaged("minecraft:iron_pickaxe", 2)
    ));
}

/// The loom writes a banner pattern as its network id, and that id is the
/// pattern's index in the registry the client was sent at configuration —
/// held here against the capture, in both directions.
#[test]
fn the_banner_pattern_ids_are_the_captured_registry_order() {
    let p = repo("data/vanilla/registries.bin");
    if !rustmc_data::fixture::available(&p, "data/vanilla/registries.bin", "ci/dump-registries.sh")
    {
        return;
    }
    let caps = rustmc_server::configuration::CapturedRegistries::load(&p).expect("capture");
    let ids = caps
        .entry_ids("minecraft:banner_pattern")
        .expect("parses")
        .expect("the capture holds minecraft:banner_pattern");
    assert_eq!(
        ids,
        rustmc_server::workstation::BANNER_PATTERNS
            .map(str::to_string)
            .to_vec()
    );
}

/// The creative slot's patch is `DELIMITED_STREAM_CODEC`: each value carries
/// a varint byte count. Damage 200 is `c8 01`, so the entry is
/// `03 02 c8 01` — read without the prefix it was damage 2, which is what
/// the first recording of `821` caught.
#[test]
fn a_creative_slot_reads_a_length_prefixed_damage() {
    let Some(props) = props() else {
        return;
    };
    let damage = props
        .component_type_id("minecraft:damage")
        .expect("damage id");
    // The item table names the component types too — the reader keeps the
    // patch by name, and a type the table does not name drops it whole.
    let items = rustmc_server::player::ItemIds::parse(&serde_json::json!({
        "items": {"minecraft:iron_pickaxe": 961},
        "data_component_types": {"minecraft:damage": damage},
    }))
    .expect("ids");
    let mut body = vec![0x00, 36, 0x01];
    rustmc_proto::varint::write_varint(&mut body, 961).expect("id");
    body.extend([0x01, 0x00]);
    rustmc_proto::varint::write_varint(&mut body, damage).expect("type");
    body.extend([0x02, 0xc8, 0x01]);
    let mut inv = Hotbar::new().with_item_props(Some(props));
    rustmc_server::player::apply_hand(
        rustmc_proto::ids::play::sb::SET_CREATIVE_MODE_SLOT,
        &body,
        &mut inv,
        Some(&items),
    )
    .expect("decoded");
    assert_eq!(inv.stack(0).damage_value(), 200);
}

/// The trim registries' network ids are the captured order, as the banner
/// patterns' are — a `TRIM` written with a wrong index is a different trim on
/// the client and the same bytes count on the wire.
#[test]
fn the_trim_registry_ids_are_the_captured_registry_order() {
    let p = repo("data/vanilla/registries.bin");
    if !rustmc_data::fixture::available(&p, "data/vanilla/registries.bin", "ci/dump-registries.sh")
    {
        return;
    }
    let caps = rustmc_server::configuration::CapturedRegistries::load(&p).expect("capture");
    for (key, ours) in [
        (
            "minecraft:trim_material",
            rustmc_server::workstation::TRIM_MATERIALS.to_vec(),
        ),
        (
            "minecraft:trim_pattern",
            rustmc_server::workstation::TRIM_PATTERNS.to_vec(),
        ),
    ] {
        let ids = caps
            .entry_ids(key)
            .expect("parses")
            .unwrap_or_else(|| panic!("the capture holds {key}"));
        assert_eq!(
            ids,
            ours.iter().map(|s| (*s).to_string()).collect::<Vec<_>>()
        );
    }
}

/// `SmithingTrimRecipe.applyTrim` through the menu: a coast template, an iron
/// chestplate and redstone make one chestplate carrying `TRIM(redstone,
/// coast)` and `hasRecipeError` stays 0; putting that chestplate back with the
/// same template and material makes **nothing** — `Objects.equals` on the
/// trim — and, all three slots being full, `hasRecipeError` goes to 1.
#[test]
fn a_smithing_table_trims_a_chestplate_once() {
    let recipes = rustmc_server::recipes::Recipes::parse(
        &serde_json::json!({
            "coast_armor_trim_smithing_template_smithing_trim": {
                "type": "minecraft:smithing_trim",
                "template": "minecraft:coast_armor_trim_smithing_template",
                "base": "#minecraft:trimmable_armor",
                "addition": "#minecraft:trim_materials",
                "pattern": "minecraft:coast"
            }
        }),
        &serde_json::json!({
            "trimmable_armor": { "values": ["minecraft:iron_chestplate"] },
            "trim_materials": { "values": ["minecraft:redstone"] }
        }),
    )
    .expect("one trim recipe");
    let trim = ComponentValue::Trim {
        material: "minecraft:redstone".into(),
        pattern: "minecraft:coast".into(),
    };
    for (base, want_result) in [
        (ItemStack::one("minecraft:iron_chestplate"), true),
        (
            {
                let mut s = ItemStack::one("minecraft:iron_chestplate");
                s.set_component(ComponentKind::Trim, trim.clone(), true);
                s
            },
            false,
        ),
    ] {
        let spec = menu_for_state("minecraft:smithing_table").expect("smithing");
        let mut menu = Menu::new(6, spec);
        let mut inv = Hotbar::new();
        inv.set_stack(
            0,
            ItemStack::one("minecraft:coast_armor_trim_smithing_template"),
        );
        inv.set_stack(1, base.clone());
        inv.set_stack(2, ItemStack::new("minecraft:redstone", 3));
        let mut slots = Slots {
            menu: &mut menu,
            inventory: &mut inv,
            container: None,
            recipes: Some(&recipes),
            second: None,
            env: Env {
                pos: Some((1, 0, 0)),
                creative: true,
                ..Env::default()
            },
        };
        for s in [31, 0, 32, 1, 33, 2] {
            let c = click(slots.menu, s);
            let _ = handle_click(&mut slots, &c);
        }
        let result = slots.menu.grid[3].clone();
        if want_result {
            assert_eq!(result.item(), "minecraft:iron_chestplate");
            assert_eq!(result.count(), 1);
            assert_eq!(
                result.get_component(ComponentKind::Trim, true),
                Some(trim.clone())
            );
            assert_eq!(slots.menu.data[0], 0);
        } else {
            assert!(result.is_empty(), "the same trim again makes nothing");
            assert_eq!(slots.menu.data[0], 1);
        }
    }
}

/// `CartographyTableMenu.quickMoveStack`: paper goes to slot 1, the
/// additional slot, and a stone — none of the three it routes — is shuffled
/// from the hotbar into the main inventory instead.
#[test]
fn a_shift_clicked_paper_goes_to_the_cartography_tables_second_slot() {
    let spec = menu_for_state("minecraft:cartography_table").expect("cartography");
    let mut menu = Menu::new(9, spec);
    let mut inv = Hotbar::new();
    inv.set_stack(0, ItemStack::one("minecraft:paper"));
    inv.set_stack(1, ItemStack::one("minecraft:stone"));
    let mut slots = Slots {
        menu: &mut menu,
        inventory: &mut inv,
        container: None,
        recipes: None,
        second: None,
        env: Env::default(),
    };
    for s in [30, 31] {
        let mut c = click(slots.menu, s);
        c.input = rustmc_server::click::ContainerInput::QuickMove;
        let _ = handle_click(&mut slots, &c);
    }
    assert_eq!(menu.grid[1], ItemStack::one("minecraft:paper"));
    assert!(menu.grid[0].is_empty());
    assert!(inv.stack(1).is_empty());
    assert_eq!(inv.stack(9), ItemStack::one("minecraft:stone"));
}
