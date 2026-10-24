//! The recipe table read against the real 26.2 extract.
//!
//! Every assertion here is over `data/vanilla/recipes.json` as
//! `ci/dump-recipes.sh` writes it, so a number in this file is a property of
//! the pinned jar and not of a fixture somebody typed. The counts are asserted
//! **exactly**: a `>=` would pass on a table that had lost half its rows, which
//! is the failure this file exists to catch.

use rustmc_server::item::ItemStack;
use rustmc_server::recipes::{CookingKind, RecipeKind, RecipeType, Recipes};

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

const RECIPES: &str = "data/vanilla/recipes.json";
const TAGS: &str = "data/vanilla/tags_item.json";

/// `None` only in `RUSTMC_FIXTURES=optional` mode; in the default mode
/// `fixture::available` panics rather than letting a gate skip.
fn table() -> Option<Recipes> {
    let mut ok = true;
    for p in [RECIPES, TAGS] {
        ok &= rustmc_data::fixture::available(&repo(p), p, "ci/dump-recipes.sh");
    }
    if !ok {
        return None;
    }
    Some(Recipes::load(&repo(RECIPES), &repo(TAGS)).expect("load the recipe extract"))
}

fn raw() -> Option<serde_json::Value> {
    if !rustmc_data::fixture::available(&repo(RECIPES), RECIPES, "ci/dump-recipes.sh") {
        return None;
    }
    Some(
        serde_json::from_str(&std::fs::read_to_string(repo(RECIPES)).expect("read")).expect("json"),
    )
}

/// **The whole 1,585-row extract, split by `RecipeType`.**
///
/// `docs/ASSESSMENT-2026-08-19.md` §1.2 records 1,056 of 1,585 served and 529
/// with no path. This test states where those 529 are, so that a later commit
/// closing one of them moves a number here rather than a paragraph in a doc.
#[test]
fn every_recipe_type_in_the_extract_is_accounted_for() {
    let Some(r) = table() else { return };
    let Some(doc) = raw() else { return };
    let total = doc.as_object().expect("object").len();
    assert_eq!(total, 1585, "the pinned 26.2 extract holds 1,585 recipes");

    // Vanilla's own seven `RecipeType`s. The sum of these is what this server
    // can look up at all.
    let served: usize = [
        RecipeType::Crafting,
        RecipeType::Smelting,
        RecipeType::Blasting,
        RecipeType::Smoking,
        RecipeType::CampfireCooking,
        RecipeType::Stonecutting,
        RecipeType::Smithing,
    ]
    .iter()
    .map(|t| r.len_of(*t))
    .sum();
    assert_eq!(served, r.len(), "every loaded recipe carries a type");

    assert_eq!(r.len_of(RecipeType::Smelting), 73);
    assert_eq!(r.len_of(RecipeType::Blasting), 25);
    assert_eq!(r.len_of(RecipeType::Smoking), 9);
    assert_eq!(r.len_of(RecipeType::CampfireCooking), 9);
    assert_eq!(r.len_of(RecipeType::Stonecutting), 319);
    assert_eq!(
        r.len_of(RecipeType::Smithing),
        30,
        "12 transforms, 18 trims"
    );
    // 1,056 shaped and shapeless, plus 32 of the 33 `crafting_transmute`
    // documents. The 33rd is `map_cloning`; see `Recipes::parse`.
    assert_eq!(r.len_of(RecipeType::Crafting), 1056 + 32);
}

/// **The tie-break, which is the one thing a recipe table can get wrong
/// silently.**
///
/// `RecipeManager.prepare` builds a `java.util.TreeMap` and walks it with
/// `SortedMap.forEach`; `RecipeMap.create` feeds that walk into an
/// `ImmutableMultimap`, which preserves insertion order per key; and
/// `getRecipeFor` is `findFirst` over `byType`. So first-match-in-sorted-id-
/// order decides which of two overlapping recipes wins.
///
/// This asserts our load order **is** that order over the real table. The
/// positive control below is not decoration: without it, a test that compares
/// a sorted list to itself passes on any implementation whatever.
#[test]
fn the_load_order_is_vanillas_sorted_path_order() {
    let Some(r) = table() else { return };
    let ids: Vec<&str> = r.all().iter().map(|x| x.id.as_str()).collect();

    // `Identifier.compareTo` compares `path` first and `namespace` second.
    // Every id in the pinned extract is bare (namespace `minecraft`), so the
    // sort key is the whole key, and Java's UTF-16 `String.compareTo` and
    // Rust's byte `Ord` agree on the ASCII these ids are.
    assert!(
        ids.iter().all(|i| !i.contains(':')),
        "a namespaced id would sort on its namespace here and on its path in \
         vanilla; none exists in the pinned jar, and this is the line that \
         notices when one appears"
    );
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "recipes load in sorted id order");

    // POSITIVE CONTROL. `Identifier.compareTo`'s path-before-namespace rule is
    // not the order the id prints in, and this is the pair that separates
    // them: printed, `alpha:zzz` < `beta:aaa`; by vanilla's comparator,
    // `beta:aaa` comes first because `aaa` < `zzz`. A test asserting only that
    // our list is sorted would be blind to which key it was sorted on.
    let printed = {
        let mut v = ["alpha:zzz", "beta:aaa"];
        v.sort_unstable();
        v
    };
    let vanilla = {
        let mut v = ["alpha:zzz", "beta:aaa"];
        v.sort_unstable_by_key(|s| {
            let (ns, path) = s.split_once(':').expect("namespaced");
            (path.to_string(), ns.to_string())
        });
        v
    };
    assert_ne!(
        printed, vanilla,
        "the two orders must differ, or this control proves nothing"
    );
    assert_eq!(vanilla, ["beta:aaa", "alpha:zzz"]);
}

/// The four cooking types, looked up the way a furnace does.
///
/// The cooking times are the load-bearing half: `AbstractCookingRecipe`'s
/// codec defaults `cookingtime` per subclass — 200 for smelting, 100 for the
/// other three — and 26.2's documents almost all omit the field, so a port
/// that defaulted to a single number would run every furnace at the wrong
/// speed while every value in the table still looked right.
#[test]
fn a_furnace_lookup_finds_the_recipe_and_its_time() {
    let Some(r) = table() else { return };

    let iron = r
        .find_single(RecipeType::Smelting, &ItemStack::one("minecraft:raw_iron"))
        .expect("raw iron smelts");
    assert_eq!(iron.result.item(), "minecraft:iron_ingot");
    assert_eq!(iron.cooking_time(), 200, "the smelting codec default");
    assert!((iron.experience() - 0.7).abs() < 1e-6);

    // The same ore in a blast furnace: half the time, same result.
    let blast = r
        .find_single(RecipeType::Blasting, &ItemStack::one("minecraft:raw_iron"))
        .expect("raw iron blasts");
    assert_eq!(blast.result.item(), "minecraft:iron_ingot");
    assert_eq!(blast.cooking_time(), 100);

    // A smoker cooks food and nothing else. Raw iron has no smoking recipe,
    // which is the branch `AbstractFurnaceBlockEntity.serverTick` reads to
    // leave its timer alone.
    assert!(r
        .find_single(RecipeType::Smoking, &ItemStack::one("minecraft:raw_iron"))
        .is_none());
    let beef = r
        .find_single(RecipeType::Smoking, &ItemStack::one("minecraft:beef"))
        .expect("beef smokes");
    assert_eq!(beef.result.item(), "minecraft:cooked_beef");
    assert_eq!(beef.cooking_time(), 100);

    // Campfire is the one family whose documents DO carry `cookingtime`, and
    // it is 600 — six times the codec default. A port that took the default
    // here would cook food six times too fast.
    let camp = r
        .find_single(
            RecipeType::CampfireCooking,
            &ItemStack::one("minecraft:beef"),
        )
        .expect("beef cooks on a campfire");
    assert_eq!(camp.result.item(), "minecraft:cooked_beef");
    assert_eq!(camp.cooking_time(), 600);

    // Every campfire recipe in 26.2 states its time; none takes the default.
    for x in r.all().iter().filter(|x| {
        matches!(
            x.kind,
            RecipeKind::Cooking {
                cooking: CookingKind::CampfireCooking,
                ..
            }
        )
    }) {
        assert_eq!(
            x.cooking_time(),
            600,
            "{} is a 600-tick campfire recipe",
            x.id
        );
    }
}

/// Stonecutting: 319 recipes, one ingredient each, and **many inputs match
/// several results**. That is what makes the stonecutter a menu with a button
/// list rather than a lookup, and it is also the one place in this table where
/// the sorted-order tie-break has real work to do.
#[test]
fn stonecutting_is_one_input_to_many_results() {
    let Some(r) = table() else { return };

    let from_stone: Vec<&str> = r
        .all()
        .iter()
        .filter(|x| x.recipe_type() == RecipeType::Stonecutting)
        .filter(|x| x.matches_single(&ItemStack::one("minecraft:stone"), r.tags()))
        .map(|x| x.id.as_str())
        .collect();
    assert!(
        from_stone.len() > 3,
        "stone cuts to several things, got {from_stone:?}"
    );

    // `getRecipeFor` returns the FIRST of them in sorted id order, and this is
    // the assertion that would move if the load order changed.
    let first = r
        .find_single(RecipeType::Stonecutting, &ItemStack::one("minecraft:stone"))
        .expect("stone cuts");
    assert_eq!(
        first.id, from_stone[0],
        "findFirst over byType takes the lowest id"
    );

    // A count that is not 1: stone cuts to two slabs.
    let slab = r
        .all()
        .iter()
        .find(|x| x.id == "stone_slab_from_stone_stonecutting")
        .expect("that recipe exists");
    assert_eq!(slab.result.item(), "minecraft:stone_slab");
    assert_eq!(slab.result.count(), 2);

    // A stonecutter does not smelt, and a furnace does not cut. The type scope
    // is what enforces that, so it is asserted rather than assumed.
    assert!(r
        .find_single(
            RecipeType::Smelting,
            &ItemStack::one("minecraft:stone_bricks")
        )
        .is_some());
    assert!(r
        .find_single(
            RecipeType::Stonecutting,
            &ItemStack::one("minecraft:raw_iron")
        )
        .is_none());
    assert!(r
        .find_single(RecipeType::Smelting, &ItemStack::empty())
        .is_none());
}

/// **The list-form ingredient, which was silently deleting itself.**
///
/// `Ingredient` wraps a `HolderSet<Item>`, so a JSON array of ids is as legal
/// as a single id or a `#tag`. This server read `as_str()` and `filter_map`ped
/// the failures away, which for a shapeless recipe shortened the ingredient
/// list rather than dropping the recipe. `dye_black_bed` therefore loaded as a
/// **one-ingredient** recipe over `minecraft:black_dye` alone, and one black
/// dye by itself in a crafting grid assembled a black bed.
///
/// The dye must be accompanied by a bed of another colour, and a bed that is
/// already black is not in the list.
#[test]
fn a_list_ingredient_is_a_set_and_not_a_dropped_field() {
    let Some(r) = table() else { return };

    let bed = r
        .all()
        .iter()
        .find(|x| x.id == "dye_black_bed")
        .expect("dye_black_bed is in the extract");
    let RecipeKind::Shapeless { ingredients } = &bed.kind else {
        panic!("dye_black_bed is shapeless")
    };
    assert_eq!(
        ingredients.len(),
        2,
        "one dye and one bed-of-any-other-colour, not one dye"
    );

    let grid = |items: &[&str]| {
        let mut cells = vec![ItemStack::empty(); 9];
        for (i, it) in items.iter().enumerate() {
            cells[i] = ItemStack::one(it);
        }
        rustmc_server::recipes::CraftingInput::of(3, 3, &cells)
    };

    // THE REGRESSION. A lone black dye makes nothing.
    assert!(
        r.assemble(&grid(&["minecraft:black_dye"])).is_empty(),
        "a black dye on its own is not a bed"
    );
    // Dye plus a white bed is.
    assert_eq!(
        r.assemble(&grid(&["minecraft:black_dye", "minecraft:white_bed"]))
            .item(),
        "minecraft:black_bed"
    );
    // And a black bed is not in the list, so re-dyeing it black does nothing.
    assert!(r
        .assemble(&grid(&["minecraft:black_dye", "minecraft:black_bed"]))
        .is_empty());

    // The 13 shaped recipes whose `key` holds a list were dropped whole by the
    // old `?`, which cost the recipe rather than corrupting it. They load now.
    let bolt = r
        .all()
        .iter()
        .find(|x| x.id == "bolt_armor_trim_smithing_template")
        .expect("a shaped recipe with a list key loads");
    assert!(matches!(bolt.kind, RecipeKind::Shaped { .. }));

    // Count them, so the fix is a number and not an anecdote.
    let with_list = r
        .all()
        .iter()
        .filter(|x| match &x.kind {
            RecipeKind::Shapeless { ingredients } => ingredients
                .iter()
                .any(|i| matches!(i, rustmc_server::recipes::Ingredient::Items(_))),
            RecipeKind::Shaped { cells, .. } => cells
                .iter()
                .flatten()
                .any(|i| matches!(i, rustmc_server::recipes::Ingredient::Items(_))),
            _ => false,
        })
        .count();
    assert_eq!(
        with_list, 78,
        "65 shapeless recipes were loading corrupted and 13 shaped were absent"
    );
}

/// Smithing: 30 recipes across two serializers sharing one `RecipeType`.
///
/// The load-bearing rule is `Ingredient.testOptionalIngredient`, whose body is
/// `opt.map(i -> i.test(stack)).orElseGet(stack::isEmpty)`. An **absent**
/// template or addition demands an *empty* slot, not "anything". Every 26.2
/// smithing document supplies all three, so the rule is exercised here through
/// the negative: junk in a slot must refuse.
#[test]
fn a_smithing_table_needs_all_three_slots() {
    use rustmc_server::recipes::SmithingInput;
    let Some(r) = table() else { return };

    assert_eq!(
        r.len_of(RecipeType::Smithing),
        30,
        "12 transforms, 18 trims"
    );

    let netherite = SmithingInput {
        template: ItemStack::one("minecraft:netherite_upgrade_smithing_template"),
        base: ItemStack::one("minecraft:diamond_axe"),
        addition: ItemStack::one("minecraft:netherite_ingot"),
    };
    let hit = r.find_smithing(&netherite).expect("the netherite upgrade");
    assert_eq!(hit.id, "netherite_axe_smithing");
    assert_eq!(
        hit.assemble_smithing(&netherite)
            .expect("transform assembles")
            .item(),
        "minecraft:netherite_axe"
    );

    // The template slot is not optional here, and an empty one refuses.
    let mut no_template = netherite.clone();
    no_template.template = ItemStack::empty();
    assert!(r.find_smithing(&no_template).is_none());

    // Nor is the addition. A netherite axe is not made from a diamond axe and
    // an empty slot.
    let mut no_addition = netherite.clone();
    no_addition.addition = ItemStack::empty();
    assert!(r.find_smithing(&no_addition).is_none());

    // The wrong base refuses even with the right template and addition.
    let mut wrong_base = netherite.clone();
    wrong_base.base = ItemStack::one("minecraft:stone");
    assert!(r.find_smithing(&wrong_base).is_none());

    // An entirely empty table is `RecipeInput.isEmpty()` and never reaches a
    // recipe at all.
    assert!(r.find_smithing(&SmithingInput::default()).is_none());

    // A trim MATCHES and REFUSES TO ASSEMBLE. `SmithingTrimRecipe.assemble` is
    // `applyTrim`, which is entirely data components; this server has none, so
    // the result is `None` rather than an untrimmed copy of the base.
    let trim = SmithingInput {
        template: ItemStack::one("minecraft:bolt_armor_trim_smithing_template"),
        base: ItemStack::one("minecraft:iron_chestplate"),
        addition: ItemStack::one("minecraft:redstone"),
    };
    let hit = r.find_smithing(&trim).expect("the bolt trim matches");
    assert!(hit.id.ends_with("_smithing_trim"), "{}", hit.id);
    assert!(
        hit.assemble_smithing(&trim).is_none(),
        "trim assembly is refused, not approximated"
    );
}

/// `crafting_transmute` — 33 recipes, and the one crafting kind whose
/// `matches` is not a shape test.
///
/// Two things make it worth its own test. A foreign item anywhere in the grid
/// refuses (offset 121 is an unconditional `return false`, not a `continue`),
/// and the tail at offset 156 refuses a transmute whose result would equal its
/// input — which is what stops a black bundle being dyed black.
#[test]
fn a_transmute_refuses_a_no_op_and_any_foreign_item() {
    let Some(r) = table() else { return };
    assert_eq!(
        r.all()
            .iter()
            .filter(|x| matches!(x.kind, RecipeKind::Transmute { .. }))
            .count(),
        32,
        "33 documents, one of them (`map_cloning`) refused"
    );

    let grid = |items: &[&str]| {
        let mut cells = vec![ItemStack::empty(); 9];
        for (i, it) in items.iter().enumerate() {
            cells[i] = ItemStack::one(it);
        }
        rustmc_server::recipes::CraftingInput::of(3, 3, &cells)
    };

    // A bundle and a black dye make a black bundle.
    assert_eq!(
        r.assemble(&grid(&["minecraft:bundle", "minecraft:black_dye"]))
            .item(),
        "minecraft:black_bundle"
    );

    // THE NO-OP RULE. A black bundle dyed black is refused, because
    // `isSameItemSameComponents(input, assembled)` is true.
    assert!(
        r.assemble(&grid(&["minecraft:black_bundle", "minecraft:black_dye"]))
            .is_empty(),
        "a black bundle is already black"
    );
    // But dyeing it a different colour is not a no-op.
    assert_eq!(
        r.assemble(&grid(&["minecraft:black_bundle", "minecraft:red_dye"]))
            .item(),
        "minecraft:red_bundle"
    );

    // A FOREIGN ITEM ANYWHERE REFUSES, even with input and material both
    // present and the material count still legal.
    assert!(r
        .assemble(&grid(&[
            "minecraft:bundle",
            "minecraft:black_dye",
            "minecraft:stone"
        ]))
        .is_empty());

    // `material_count` defaults to `exactly(1)`, so two dyes refuse.
    assert!(r
        .assemble(&grid(&[
            "minecraft:bundle",
            "minecraft:black_dye",
            "minecraft:black_dye"
        ]))
        .is_empty());
    // And a dye with no bundle refuses on `ingredientCount < min + 1`.
    assert!(r.assemble(&grid(&["minecraft:black_dye"])).is_empty());
}

/// The seven recipes that could not be crafted on this server at all.
///
/// `ShapedRecipePattern.shrink` trims a pattern to its bounding box before it
/// is stored, and this server did not. `spyglass` is written `[" # ", " X ",
/// " X "]` and is a **1x3** recipe; ours kept the 3x3 with blank border columns
/// while `CraftingInput.of` trimmed the player's grid to 1x3, so the two could
/// never be the same size and the recipe never matched.
///
/// Found by `recipe_match_golden.rs`, not by reading: seven vectors where
/// vanilla names a winner and this server found none.
#[test]
fn the_seven_loosely_written_patterns_craft() {
    let Some(r) = table() else { return };

    let expect = [
        ("creaking_heart", "minecraft:creaking_heart"),
        ("mace", "minecraft:mace"),
        ("spyglass", "minecraft:spyglass"),
        ("waxed_chiseled_copper", "minecraft:waxed_chiseled_copper"),
        (
            "waxed_exposed_chiseled_copper",
            "minecraft:waxed_exposed_chiseled_copper",
        ),
        (
            "waxed_oxidized_chiseled_copper",
            "minecraft:waxed_oxidized_chiseled_copper",
        ),
        (
            "waxed_weathered_chiseled_copper",
            "minecraft:waxed_weathered_chiseled_copper",
        ),
    ];
    for (id, _) in expect {
        let recipe = r
            .all()
            .iter()
            .find(|x| x.id == id)
            .unwrap_or_else(|| panic!("{id} is in the extract"));
        let RecipeKind::Shaped { width, .. } = &recipe.kind else {
            panic!("{id} is shaped")
        };
        assert_eq!(*width, 1, "{id}'s pattern shrinks to one column");
    }

    // And the one with the most interesting shape actually assembles.
    let mut items = vec![ItemStack::empty(); 9];
    items[1] = ItemStack::one("minecraft:amethyst_shard");
    items[4] = ItemStack::one("minecraft:copper_ingot");
    items[7] = ItemStack::one("minecraft:copper_ingot");
    assert_eq!(
        r.assemble(&rustmc_server::recipes::CraftingInput::of(3, 3, &items))
            .item(),
        "minecraft:spyglass"
    );
}
