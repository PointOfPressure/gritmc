# COLD START — branch `menus-workstations`

### COLD START — branch `menus-workstations`, 2026-10-01 (halted at the owner's usage stop)

* Done: `crate::workstation` (stonecutter, anvil, grindstone, enchanting table, smithing transform, loom, cartography predicates, quickMoveStack), component patches on `ItemStack`, length-prefixed creative-slot patches, data-slot sync, `container_button_click`/`rename_item` verbs. Subset r3 (tree `30f5d0e05`): 810-871 declared 26/26 match, strict 25/26; unit tests `tests/workstation_menus.rs` 6/6. Rebased onto main `e4a2ee75f` (dropped the duplicate `container_set_data` arm; main's `47dc7620f` decodes the same fields).
* Not done — T0 is RED on the branch until these land: `ci/checks/gameplay-falsifiable.toml` needs the splice sweep (gate pair copy at scratchpad `menus-ws/splice/` + subset r4 `~/rustmc-menus-ws/oracle-work/gameplay/subset/`), then `FIELD_UNIVERSE` 129 -> the swept value and `MAX_UNASKABLE` 14 -> 15 (two `waypoint` pairs from main's decoder, not this branch); r4 (816/824/834/844/863, quickMove) was recording when halted — baseline rows for those five not yet added.
* Next: re-splice with r4, sweep, set the two pins with reasons, add the five baseline rows and five gameplay-grades rows (`grades_append.py` in the scratchpad), T0 via `ci/run-detached.sh` in `~/rustmc-menus-t0`.

### COLD START — branch `menus-workstations`, 2026-10-01 (evening)

* Done since the halt: rebased onto main 4024. Crafting table: `ResultSlot.onTake` (grid consumed per cell, remainders), per-write grid recompute (the `grid_written` flag is gone), `CraftingMenu.quickMoveStack`, `ResultContainer`'s whole-stack `removeItem`, `canTakeItemForPickAll` overrides, `safeTake`/SWAP `onTake`. Smithing trims (`TRIM`, trim registries). `CartographyTableMenu.quickMoveStack`. Checks 800-808, 854, 855, 864; 843 renamed 849 so 844 runs before the one enchant (r6 diverged on the seed slot alone).
* Measured: subset r6 (42 checks, tree 8689f04b2) declared 41/42, r7 (864) 1/1, Threadripper, seed 20260803. Splice of the 11:50 stack gate pair + r6 + r7: 222 scenarios, 222 tier 2, dead 0/142, unaskable 15/157. `FIELD_UNIVERSE` 142, `MAX_UNASKABLE` 15, with reasons in `ci/corpus-falsifiable.py`.
* Refused (nulls, do not re-open as menus work): cartography **results** need `MapItemSavedData` (`lambda$setupResultSlot$0` returns at offset 13 on null) — a maps port; survival XP costs are unreachable by any scenario here (creative substrate has no `gamemode` command on this server; survival substrate cannot `hold`).
* Named gaps: a crafting remainder that fits neither its cell nor the inventory is `Player.drop` — not spawned; `checkTakeAchievements` (stats, recipe award) not ported.
* Next: r8 (840-844, 849 in the new order) -> re-splice -> sweep -> T0 (`mws-round8`); then the gate.
