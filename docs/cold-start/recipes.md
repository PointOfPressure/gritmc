# COLD START — branch `recipes`

### COLD START — branch `recipes`, 2026-09-05

**The area:** the 529 recipe kinds `crates/rustmc-server/src/recipes.rs` had no
path for, the crafting result matching vanilla 26.2 performs, and the
recipe-sync packets. Transcribed from
`oracle-work/vanilla-26.2/versions/26.2/server-26.2.jar` under JDK 25.

## What the table does now

1,585 documents in `data/vanilla/recipes.json`. **1,553 load** (was 1,043, and
65 of those loaded corrupted). 32 are refused, each named below. All seven of
vanilla's `RecipeType`s have a lookup: `Recipes::find` for `CRAFTING`,
`find_single` for the four cooking types and stonecutting, `find_smithing` for
`SMITHING`.

## Four defects on `main`, three of which no unit test could have found

* **`Ingredient` is a `HolderSet`, so a JSON array of ids is legal wherever a
  bare id or a `#tag` is.** 26.2 uses the array 78 times. Our parser read
  `as_str()` and `filter_map`ped the failures away — which for a *shapeless*
  recipe deleted the ingredient from the list rather than dropping the recipe.
  `dye_black_bed` loaded as a **one-ingredient** recipe over
  `minecraft:black_dye`, so a lone black dye in a crafting grid assembled a
  black bed. 65 of 323 shapeless recipes were in that state; the other 13 uses
  are shaped `key` entries, where the `?` dropped the recipe and cost only the
  recipe.
* **`ItemTags` did not follow a `#other_tag` entry.** 50 of the 224 item tags
  in the extract carry one and 24 have no plain entries at all;
  `#minecraft:trimmable_armor` is four nested references and resolved to the
  **empty set**. 29 recipes had an ingredient matching nothing (18
  `smithing_trim`, 10 `crafting_shaped`, 1 `smelting`) and one more resolved
  short. Fixed as `TagLoader.build`'s `DependencySorter` walk, with a
  `visiting` stack for the cycle it refuses.
* **`ShapedRecipePattern.shrink` was missing, and it silently disabled seven
  recipes.** A pattern is written for legibility: `spyglass` is
  `[" # ", " X ", " X "]` and vanilla shrinks it to a **1x3**. Ours kept the
  3x3 with blank border columns while `CraftingInput.of` trims the *grid* to
  1x3, so the sizes could never match. `creaking_heart`, `mace`, `spyglass`
  and the four `waxed_*_chiseled_copper` **could not be crafted on this server
  at all**, and nothing said so — a recipe that never matches looks exactly
  like one nobody tried. **Found by the golden, not by reading.**
* **`TransmuteRecipe`'s `material_count` defaults to
  `MinMaxBounds.Ints.exactly(1)`**, not to an open range. The `orElse(1)` and
  `orElse(8)` inside `minMaterialCount`/`maxMaterialCount` are what an
  explicitly open bound means; conflating them lets a bundle take eight dyes.

## The tie-break, which was documented as unknowable and is not

`recipes.rs` used to say the iteration order "is a `BTreeMap`'s key order here
and vanilla's own registry order there" and that "the two are not the same".
Both halves were wrong. `RecipeManager.prepare` offset 0 is
`new java.util.TreeMap()`, walked by `SortedMap.forEach` into the list
`RecipeMap.create` feeds to an `ImmutableMultimap` (insertion-order-preserving
per key), and `getRecipeFor` is `findFirst` over `byType`. **First match in
sorted recipe-id order**, and the sort is `Identifier.compareTo`, which
compares **path before namespace**. Our `serde_json::Map` over
`ci/dump-recipes.sh`'s bare-path keys is byte-for-byte that order for a
vanilla-only table. Gated with a positive control that separates path-first
from printed order.

The one live gap: a non-`minecraft` recipe is keyed `ns:name` by the dumper and
would sort on the namespace. None exists in the pinned jar; the test asserts it.

## The instrument

`ci/recipe-match-golden.sh` + `tools/goldengen/vanilla/RecipeMatchGolden.java`.
It asks the jar's own `RecipeMap.getRecipesFor(type, input, null).findFirst()`
which recipe wins on a grid and what it assembles, over grids built from
vanilla's `Recipe.placementInfo()` — so **neither side of the comparison is our
reading of the recipe documents.**

```
1,585 recipes decoded by vanilla, 224 item tags bound through TagLoader.build
24 CustomRecipes reported unplaceable rather than silently absent
3,122 vectors: 1,561 positive + 1,561 with one slot poisoned with bedrock
our matcher agrees with 3,094; the 28 are the declared refusals
256 of the 1,561 positive grids are won by a DIFFERENT recipe
```

That last number is why the file exists. `getRecipeFor` is `findFirst` over a
bucket ordered by recipe id, so the winner is a property of the whole table's
sort order and has no local symptom: every recipe can be right and the answer
wrong. A table ordered any other way fails 256 vectors where a unit test
written from the same reading of the data fails none.

Two instrument traps, both of which produce a plausible file rather than an
error, and both refused by the script before it writes:

* `VanillaRegistries.createLookup()` leaves item tags unbound, so every `#tag`
  ingredient resolves empty and every case reports no winner.
  `MappedRegistry.bindTags` is refused after `Bootstrap.bootStrap()`
  ("Registry is already frozen"); the route that works is
  `prepareTagReload(new TagLoader.LoadResult<>(..)).apply()`.
* Data components need `DATA_COMPONENT_INITIALIZERS.build(registries)` or every
  `new ItemStack(...)` throws "Components not bound yet". `LootGolden`'s header
  records the same trap.

`recipe_match.json` is registered in `ci/gen-gameplay-tables.sh`, separately
from `ci/dump-recipes.sh`: that one restates the recipe documents, this one
measures vanilla's behaviour over them, and a table that agrees with the
documents by construction cannot gate what they leave open.

## The packet half

**`update_recipes` no longer carries recipes.**
`ClientboundUpdateRecipesPacket` is a record of two fields: a
`Map<ResourceKey<RecipePropertySet>, RecipePropertySet>` — seven named item-id
sets — and a stonecutter `SelectableRecipe.SingleInputSet` written with
`noRecipeCodec()`, which elides the recipes. The table reaches the client
through `recipe_book_add` displays, which `crate::recipe_book` already serves
from captured bytes. `Recipes::property_set` computes all seven;
**303 item entries, all seven equal to vanilla's own reflected
`IngredientExtractor`s.** Note that `forSingleInput` tests
`getType() == type && instanceof SingleItemRecipe`, so **stonecutting
contributes to no property set** despite all 319 of its recipes being
`SingleItemRecipe`s.

## What was KILLED as a null — do not re-run these

* **A byte-identical `update_recipes` capture. Refused, with a measurement.**
  `ByteBufCodecs.map` writes entries in the map's iteration order, and
  `finalizeRecipeLoading` builds that map with
  `Collectors.toUnmodifiableMap`, whose `ImmutableCollections` result probes
  from a per-JVM-run `SALT`. `ci/recipe-packet-order-probe.sh 6` on
  sayer-desktop, JDK 25.0.4: **6 JVM runs, 4 distinct orders.** Two boots of
  *unmodified vanilla* send the same seven sets in different bytes. Such a gate
  would pin one JVM's hash salt and call it parity. Do not build one; the probe
  prints a WEAK verdict rather than a clean one if N runs happen to agree, so
  raising N is the only honest way to revisit it.
* **`SmithingTrimRecipe.assemble`, `map_cloning`, and the 31 remaining
  `crafting_*` documents.** Every one computes its result out of data
  components. `applyTrim` reads `PROVIDES_TRIM_MATERIAL` off the addition and
  returns `ItemStack.EMPTY` at offsets 48 and 73; `TransmuteRecipe.computeResult`
  copies `getComponentsPatch()`. `crate::item::ItemStack` is `{item, count}`.
  Writing `base.copy_with_count(1)` for a trim would return a visibly untrimmed
  chestplate **and make both `EMPTY` branches unreachable**, so no test of them
  could go red. The 18 trims still MATCH; only the result is `None`.
* **The `cooking.rs` duplication.** The brief said `origin/block-entity-behaviour`
  "needs a smelting/blasting/smoking/campfire lookup by input". It does not —
  that branch already ships its own `CookingRecipes::parse`/`find` with the
  same per-subclass codec defaults. This branch did not write a second one for
  it; see the next section.

## A second item, smaller and independent

`crates/rustmc-server/src/click.rs` calls `Recipes::assemble`, which is the
no-`lastRecipe` path. `CraftingMenu.slotChangedCraftingGrid` always passes the
menu's held recipe, and that argument **changes the answer**: 256 of the 1,561
probe grids are won by a recipe other than the one they were built from, and
passing the built-from recipe as `last` flips all 256. `Recipes::find_with_last`
is transcribed and gated; what is missing is the menu holding the id across
clicks, which belongs to whichever lane owns the container menus. Until it is
wired, a player making two different things from one arrangement of ingredients
gets the sorted-first one both times.

## The single next item

**Point `crates/rustmc-server/src/cooking.rs` at `Recipes::find_single` and
delete `CookingRecipes`.** `origin/block-entity-behaviour` carries an
independent parser for the same 116 cooking documents, written before this
branch existed. Two parsers over one table is the shape that lets one of them
drift; and only this one is gated by `recipe_match_golden.rs`, so the furnace
would be running an ungated table while a gate a few files away reported green.
The closing result: `cooking.rs` has no `parse` of its own, the 116 cooking
vectors in `recipe_match.json` still pass, and
`crates/rustmc-server/src/cooking.rs`'s own tests pass against
`Recipes::find_single`. It is a merge-order item — do it on whichever of the two
lands second.
