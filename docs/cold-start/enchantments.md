# Cold start — branch `enchantments`

---

### COLD START — branch `enchantments` off `f0a46166a` (`gate-instruments`), 2026-09-06

**The area.** 26.2's enchantments as data and as behaviour: the registry, the
`EnchantmentHelper` paths that read it, the enchanting table's three offers,
and the anvil's cost model. Full write-up in `docs/PARITY-ROADMAP.md` §106;
only what a next agent needs is repeated here.

**Base.** `main` at `1d109abcf`, rebased onto `origin/gate-instruments`
`f0a46166a` because both touch `ci/t0.sh` and that branch is pinned to gate
second. Remote dir `~/rustmc-ench`.

## What landed

| file | what |
|---|---|
| `tools/goldengen/vanilla/EnchantmentGolden.java` + `ci/enchantments-golden.sh` | `data/vanilla/enchantments.json` — 43 entries three ways, 0 mismatches |
| `crates/rustmc-server/src/enchantment.rs` | the reader and the four `Enchantment` accessors |
| `crates/rustmc-server/src/enchanting.rs` | `slotsChanged`, `getEnchantmentCost`, `selectEnchantment`, `WeightedRandom`, `getAvailableEnchantmentResults`, plus both menu shapes |
| `crates/rustmc-server/src/anvil.rs` | `AnvilMenu.createResult` whole |
| three `tests/` targets | 40 assertions, in T0's selector and in `ci/checks/test-targets.toml` |

## The three things worth another agent's five minutes

**1. A datapack registry's order is NOT `VanillaRegistries.createLookup()`'s.**
It hands back the bootstrap's registration order; a live server's registry is
sorted by identifier, because `RegistryDataLoader` walks a sorted resource
listing. This dumper's first answer was wrong and would have put a wrong
`enchantClue` on the wire for all 43. Cross-checked against
`data/vanilla/registries.bin`. **If you are extracting any datapack registry,
check this.** Built-in registries are fine.

**2. The bookshelf clamp is not where the folklore puts it.** `> 15 -> 15`
lives inside `getEnchantmentCost` (offset 19). The count it clamps runs to
**32** — `BOOKSHELF_OFFSETS` is the perimeter of a 5x5 at two heights. A port
that clamped at the counting site is wrong and observationally identical.

**3. Seven draws, two loops, and a reseed.** `setSeed(seed)`, then two
`nextInt` per slot (`nextInt(8)` then `nextInt(shelves + 1)`), then a
**separate** loop whose `getEnchantmentList` reseeds to `seed + slot`. Fusing
the loops is the tidy-up that breaks everything after the first number.

## Nulls — killed, do not re-run

* **Do not encode the registry with `Enchantment.DIRECT_CODEC`.** It was
  tried. A `HolderSet.Named` cannot encode until its tag is bound, and
  `exclusive_set` names **ENCHANTMENT** tags — a datapack registry whose
  `MappedRegistry` a harness holding only a `HolderLookup.Provider` cannot
  reach to call `prepareTagReload` on. Binding item, entity_type and block
  tags gets three quarters of the way and then stops on `exclusive_set/bow`.
  The jar's own bundled JSON needs none of it and is the better source anyway.
* **Do not look for `getDamageBonus` or `getMobLooting`.** They do not exist
  in 26.2. Damage and looting are effect components.
* **Do not add `minecraft:enchanting_table` or `minecraft:anvil` to
  `crate::menu::menu_for_state`.** It advances `ServerPlayer.containerCounter`
  on a click that opens nothing today and shifts every later window id in a
  recording. `origin/menus-2`'s change. The derived shapes are already in
  `crate::enchanting::{menu_shape, anvil_menu_shape}` — lift them, do not
  re-read the constructor.
* **Do not register a gameplay scenario for the offers or the anvil cost.**
  Both reach a client only as `container_set_data` on an open menu. Until the
  menu arm exists there is nothing to observe, and a scenario that cannot see
  its own subject is the unfalsifiable-check class.
* **Do not read `data/vanilla/enchantment.json`,
  `enchantment_provider.json`, `item_enchantability.json` or
  `item_repairable.json`.** They are on the shared `data/vanilla` on this box,
  no commit anywhere produces them, `generator_for()` has no arm and they
  carry no stamp — the `tags_block.json` hazard with four more files. This
  lane's table is `enchantments.json`, plural, generated and stamped, and
  carries the enchantability, max-damage and repair-material data itself.

## The single next item

**The enchanting-table menu, and it is not this lane's.** When `menus-2` lands
a `menu_for_state` arm for `minecraft:enchanting_table`, `crate::enchanting`
already produces exactly the ten `container_set_data` values that menu
carries. The scenario that closes it is: a console step places an enchanting
table and a ring of bookshelves with a `console_restore`, a player right-clicks
it, and the check observes `container_set_data` ids 0..9. Until that arm
exists, the offers are unobservable and the goldens in
`tests/enchanting_offers.rs` record internal consistency only.

The second item, smaller and independent: **`enchant_randomly` in
`rustmc-loot`**. Measured at 3 of the interpreter's 60 refusals (plus 1 for
`set_enchantments`); it is the only enchant function whose output the vectors
can see, because it rewrites `minecraft:book` to `minecraft:enchanted_book`.
Its shape is `Util.getRandomSafe` (one `nextInt(size)`, **no draw** on an empty
candidate list) then `Mth.nextInt(random, minLevel, maxLevel)`, with
`only_compatible` defaulting to **true** and
`include_additional_cost_component` to **false**, so no vanilla table takes the
second draw. It needs `rustmc-loot` to see an enchantment table, which is a
cross-crate change and a lane boundary.
