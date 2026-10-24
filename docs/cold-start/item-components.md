# COLD START — branch `item-components`

### COLD START — branch `item-components`, rebased on `main` (994e795b9, after be-furnace-brewing), 2026-10-01 late

**Item data components on the wire, in merging, and at placement.** Carries
`loot` (its commits, rebased; `origin/loot` is not an ancestor). Merge
order: `menus-workstations` lands first — see NEXT 1, it is the one real
obstacle.

**DONE (committed, pushed, recorded twice each, all match):**

* Wire: `crate::patch` writes enchantments, stored enchantments, potion
  contents, stew effects, instrument (id + 1), ominous amplifier, damage,
  custom name and raw values; `HolderIds` resolves holders. `ItemStack.patch`
  is carried by split/copy and compared by `is_same_item_same_components`.
  Loot stacks keep their patch; creative stacks keep theirs raw and echo it.
* Merging: PICKUP_ALL and QUICK_MOVE respect components (942, 943); a
  patched `max_stack_size` is the stack's maximum (948).
* Placement (`BlockItem.place` 106-171): `block_state` (949); `custom_name`
  on the twelve `BaseContainerBlockEntity` kinds (menu title, 945-946), the
  banner (`CustomName` in its update tag, 950), beacon, enchanting table and
  skull (`custom_name`, 951); `container` (947; refuses nested non-empty
  patches); `pot_decorations` (`sherds`, 952).
* `entity_data` (main's 084 silent pig) re-seated beside the patch.
* Harness `hold` encodes `custom_name` (`text`), `container`, `block_state`
  (`properties`), `pot_decorations` (`sherds`), `max_stack_size`,
  `entity_data`, `bytes` for the enchantment maps.
* Scenarios 930-952 (850-852 clashed with menus-workstations; 930-959 free
  everywhere). 886 dropped: vanilla's map order flaps, ours is content-exact
  (FINDINGS has the digest reconstruction).
* Ledgers: main's rows + 23 of ours; FIELD_UNIVERSE 138 (+1,
  `container_set_content.truncated`), the rest main's pins.

**NEXT:**

1. **Unify with `menus-workstations` when it lands.** It adds
   `ItemStack.components: Components` (typed, `ComponentKind` of 7) and
   rewrites `patch::Component::Enchantments` to ids; `workstation.rs` uses that
   API 58 times. Keep its surface (`get_component`/`set_component`/
   `remove_component`/`apply_patch_entry`, `prototype_value`) as the one
   store; add this branch's kinds and a raw value keyed by type id; write
   through one encoder with `HolderIds`. Re-record 930-952 after. Trial merge
   at 3b6f773b2: item.rs 3 hunks, patch.rs 3, player.rs 3, menu.rs 1.
2. `DATA_ITEM` join: `itementity::data_item_body` (on `train-f`/`item-pickup`)
   writes a literal `[0, 0]`; use `crate::menu::write_optional_stack`.
3. Villager offers (`villager::merchant`) refuse a patched result and a cost
   predicate; no live merchant path on main to measure.
4. Not ported: `lock` (needs `ItemPredicate`), banner `patterns` at
   placement (menus-workstations owns `banner_patterns`), skull `profile`,
   `bees`, block drops carrying components (no block-loot path on main).
