//! Our matcher against vanilla's own answers.
//!
//! `data/vanilla/recipe_match.json` is written by `ci/recipe-match-golden.sh`,
//! which asks the jar's own `RecipeMap.getRecipesFor(...).findFirst()` which
//! recipe wins on a grid and what it assembles. The grids come from vanilla's
//! `Recipe.placementInfo()` — the same description `ServerPlaceRecipe` uses to
//! fill a crafting grid from the recipe book — so neither side of this
//! comparison is our reading of the recipe document.
//!
//! # Why this file is the gate and the unit tests are not
//!
//! A recipe engine written from one reading of the data will agree with itself
//! about every recipe in it. Three failures survive that: an ingredient read as
//! a smaller set than it is, a recipe dropped, and the **wrong recipe winning**
//! a grid two recipes both match. The third has no local symptom at all —
//! every recipe is right and the answer is wrong — and 256 of the 1,561
//! positive vectors here are grids won by a recipe other than the one they were
//! built from, so a table in the wrong order fails 256 cases rather than none.
//!
//! # What this file must NOT become
//!
//! An allowlist that grows to swallow disagreements. The refusal set below is
//! asserted **exactly**, in both directions: a recipe that starts agreeing must
//! be taken out of it, and a new disagreement fails the test rather than being
//! absorbed. `docs/FINDINGS.md` and `crate::recipes`'s header carry the same
//! list, and each entry names the vanilla method whose body this server cannot
//! evaluate.

use rustmc_server::item::ItemStack;
use rustmc_server::recipes::{CraftingInput, RecipeType, Recipes, SmithingInput};

fn repo(relative: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

const GOLDEN: &str = "data/vanilla/recipe_match.json";
const RECIPES: &str = "data/vanilla/recipes.json";
const TAGS: &str = "data/vanilla/tags_item.json";

/// Every recipe whose result this server refuses to compute, and the vanilla
/// method it would have had to evaluate to do it. Each is a data-component
/// computation and `crate::item::ItemStack` is `{item, count}`.
///
/// A vector is expected to disagree if **either** the recipe it was built from
/// or the recipe vanilla says wins is in this set.
const REFUSED: [(&str, &str); 28] = [
    // `DyeRecipe.assemble` — a `DYED_COLOR` component write.
    ("leather_boots_dyed", "DyeRecipe.assemble"),
    ("leather_chestplate_dyed", "DyeRecipe.assemble"),
    ("leather_helmet_dyed", "DyeRecipe.assemble"),
    ("leather_horse_armor_dyed", "DyeRecipe.assemble"),
    ("leather_leggings_dyed", "DyeRecipe.assemble"),
    ("wolf_armor_dyed", "DyeRecipe.assemble"),
    // `ImbueRecipe.assemble` — copies the potion contents onto the arrows.
    ("tipped_arrow", "ImbueRecipe.assemble"),
    // The one transmute with `add_material_count_to_result`; its content is
    // the `MAP_ID` patch `computeResult` copies across.
    (
        "map_cloning",
        "TransmuteRecipe.computeResult component patch",
    ),
    // `SmithingTrimRecipe.assemble` -> `applyTrim`, which reads
    // `PROVIDES_TRIM_MATERIAL` and writes `DataComponents.TRIM`. These MATCH;
    // only the result is refused.
    (
        "bolt_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "coast_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "dune_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "eye_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "flow_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "host_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "raiser_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "rib_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "sentry_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "shaper_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "silence_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "snout_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "spire_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "tide_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "vex_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "ward_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "wayfinder_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    (
        "wild_armor_trim_smithing_template_smithing_trim",
        "applyTrim",
    ),
    // NOT refused for their own sake — these two are `CustomRecipe`s that
    // `PlacementInfo` calls unplaceable, so the golden has no vector FOR them.
    // They still appear here because they WIN two vectors built from other
    // recipes: `decorated_pot` sorts before `decorated_pot_simple` and
    // `firework_rocket` before `firework_rocket_simple`, and both take the
    // grid their `_simple` twin was built from. That is the tie-break this
    // file exists to gate, showing what a refusal costs: this server answers
    // with the `_simple` recipe, which happens to make the same item, and
    // would not if the two ever differed.
    ("decorated_pot", "DecoratedPotRecipe (CustomRecipe)"),
    ("firework_rocket", "FireworkRocketRecipe (CustomRecipe)"),
];

fn recipe_type(name: &str) -> RecipeType {
    match name {
        "crafting" => RecipeType::Crafting,
        "smelting" => RecipeType::Smelting,
        "blasting" => RecipeType::Blasting,
        "smoking" => RecipeType::Smoking,
        "campfire_cooking" => RecipeType::CampfireCooking,
        "stonecutting" => RecipeType::Stonecutting,
        "smithing" => RecipeType::Smithing,
        other => panic!("unknown recipe type {other} in the golden"),
    }
}

#[test]
fn our_matcher_agrees_with_vanillas_own_answers() {
    let mut ok = true;
    for p in [GOLDEN, RECIPES, TAGS] {
        let by = if p == GOLDEN {
            "ci/recipe-match-golden.sh"
        } else {
            "ci/dump-recipes.sh"
        };
        ok &= rustmc_data::fixture::available(&repo(p), p, by);
    }
    if !ok {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo(GOLDEN)).expect("read the golden"))
            .expect("the golden is json");
    let table = Recipes::load(&repo(RECIPES), &repo(TAGS)).expect("load the recipe extract");

    let cases = doc["cases"].as_array().expect("cases is an array");
    assert!(
        cases.len() > 3300,
        "the golden holds {} cases; a short one is an instrument failure, not a pass",
        cases.len()
    );

    // A vector with no winner on BOTH sides is a real case, but a file of them
    // would pass any matcher that always answered "no". Counted, and floored.
    let mut positives = 0usize;
    let mut with_last = 0usize;
    let mut tie_break = 0usize;
    let mut agreed = 0usize;
    let mut disagreed: Vec<(String, String)> = Vec::new();

    for c in cases {
        let id = c["recipe"].as_str().expect("recipe id");
        let ty = recipe_type(c["type"].as_str().expect("type"));
        let negative = c["negative"].as_bool().expect("negative");
        let grid: Vec<ItemStack> = c["grid"]
            .as_array()
            .expect("grid")
            .iter()
            .map(|v| match v.as_str().expect("grid entry") {
                "" => ItemStack::empty(),
                item => ItemStack::one(item),
            })
            .collect();

        // `getRecipeFor`'s four-argument overload. A vector carrying
        // `last_recipe` asks what a menu that just made THAT recipe answers,
        // which is a different question from what an empty menu answers — see
        // `Recipes::find_with_last`.
        let last = c["last_recipe"].as_str();
        let ours = match ty {
            RecipeType::Crafting => {
                let w = usize::try_from(c["width"].as_u64().expect("width")).expect("width fits");
                let h =
                    usize::try_from(c["height"].as_u64().expect("height")).expect("height fits");
                table.find_with_last(&CraftingInput::of(w, h, &grid), last)
            }
            RecipeType::Smithing => table.find_smithing(&SmithingInput {
                template: grid.first().cloned().unwrap_or_else(ItemStack::empty),
                base: grid.get(1).cloned().unwrap_or_else(ItemStack::empty),
                addition: grid.get(2).cloned().unwrap_or_else(ItemStack::empty),
            }),
            single => table.find_single_with_last(
                single,
                grid.first().unwrap_or(&ItemStack::empty()),
                last,
            ),
        };

        let want_winner = c["winner"].as_str();
        if !negative && last.is_none() {
            positives += 1;
            if want_winner != Some(id) {
                tie_break += 1;
            }
        }
        if last.is_some() {
            with_last += 1;
            // The whole content of these vectors: without `last`, a different
            // recipe wins this grid; with it, the held one does. If vanilla did
            // not return the held recipe they would be a second copy of the
            // plain positives and would gate nothing.
            assert_eq!(
                want_winner, last,
                "vector `{id}` carries last_recipe but vanilla did not return it; \
                 the golden is not exercising getRecipeFor's four-argument overload"
            );
        }

        // What vanilla assembled, as (item, count). `null` means the recipe
        // matched and produced nothing, which `SmithingTrimRecipe` can do.
        let want_result = c["result"].as_str();
        let want_count = c["count"].as_i64().unwrap_or(0);

        let got_winner = ours.map(|r| r.id.as_str());
        let (got_result, got_count) = match (ty, ours) {
            (RecipeType::Smithing, Some(r)) => {
                let s = r.assemble_smithing(&SmithingInput {
                    template: grid.first().cloned().unwrap_or_else(ItemStack::empty),
                    base: grid.get(1).cloned().unwrap_or_else(ItemStack::empty),
                    addition: grid.get(2).cloned().unwrap_or_else(ItemStack::empty),
                });
                s.map_or((None, 0), |s| {
                    (Some(s.item().to_string()), i64::from(s.count()))
                })
            }
            (RecipeType::Crafting, Some(r)) => {
                let w = usize::try_from(c["width"].as_u64().expect("width")).expect("width fits");
                let h =
                    usize::try_from(c["height"].as_u64().expect("height")).expect("height fits");
                let s = r.assemble(&CraftingInput::of(w, h, &grid));
                (Some(s.item().to_string()), i64::from(s.count()))
            }
            (_, Some(r)) => (
                Some(r.result.item().to_string()),
                i64::from(r.result.count()),
            ),
            (_, None) => (None, 0),
        };

        let same = got_winner == want_winner
            && got_result.as_deref() == want_result
            && got_count == want_count;
        if same {
            agreed += 1;
        } else {
            let blame = want_winner.unwrap_or(id);
            disagreed.push((
                id.to_string(),
                format!(
                    "want {want_winner:?} -> {want_result:?} x{want_count}, \
                     got {got_winner:?} -> {got_result:?} x{got_count} (blame {blame})"
                ),
            ));
        }
    }

    // Every disagreement must be a NAMED refusal, in both directions.
    let mut unexpected: Vec<&(String, String)> = Vec::new();
    for d in &disagreed {
        let blames_refused = REFUSED
            .iter()
            .any(|(r, _)| d.1.contains(&format!("(blame {r})")) || d.0 == *r);
        if !blames_refused {
            unexpected.push(d);
        }
    }
    assert!(
        unexpected.is_empty(),
        "{} vectors disagree for a reason that is not a declared refusal:\n{}",
        unexpected.len(),
        unexpected
            .iter()
            .take(12)
            .map(|(id, why)| format!("  {id}: {why}"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    // And every declared refusal must still be earning its place. A refusal
    // nothing disagrees about is a line this file no longer needs, and an
    // allowlist nobody prunes is how one grows to swallow real failures.
    for (id, why) in REFUSED {
        assert!(
            disagreed
                .iter()
                .any(|(d, w)| d == id || w.contains(&format!("(blame {id})"))),
            "`{id}` is declared refused for `{why}` but no vector disagrees about it — \
             either it now works and this line should go, or the golden no longer reaches it"
        );
    }

    assert_eq!(positives, 1561, "one positive vector per placeable recipe");
    assert_eq!(
        with_last, tie_break,
        "one `lastRecipe` vector for every grid another recipe wins without it"
    );
    assert!(
        tie_break >= 200,
        "only {tie_break} of the {positives} positive grids are won by a recipe other than the \
         one they were built from; the tie-break this file exists to gate would then be barely \
         tested, and the probe should be suspected before the port"
    );
    println!(
        "recipe-match: {agreed} of {} vectors agree with vanilla, {} declared refusals, \
         {tie_break} tie-break grids, {with_last} of them re-asked with a lastRecipe",
        cases.len(),
        disagreed.len()
    );
}

/// The `update_recipes` payload's content, against vanilla's own extractors.
///
/// # Why content and not bytes
///
/// `ClientboundUpdateRecipesPacket` is a record of a
/// `Map<ResourceKey<RecipePropertySet>, RecipePropertySet>` and a stonecutter
/// `SingleInputSet`. The map is written by `ByteBufCodecs.map` in **iteration
/// order**, and `RecipeManager.finalizeRecipeLoading` builds it with
/// `Collectors.toUnmodifiableMap`, whose result probes from a per-JVM-run salt.
/// `ci/recipe-packet-order-probe.sh` measures it: six JVM runs, four distinct
/// orders. Two boots of **unmodified vanilla** send these seven sets in
/// different orders, so a byte-identical capture would be pinning one JVM's
/// hash salt. The content is the part that is a property of the server.
///
/// The golden's side is computed by reflecting `RECIPE_PROPERTY_SETS` and
/// invoking vanilla's own `IngredientExtractor`s, so this compares two
/// independent derivations of the same seven sets.
#[test]
fn the_update_recipes_property_sets_match_vanillas() {
    use rustmc_server::recipes::PropertySetKey;

    let mut ok = true;
    for p in [GOLDEN, RECIPES, TAGS] {
        let by = if p == GOLDEN {
            "ci/recipe-match-golden.sh"
        } else {
            "ci/dump-recipes.sh"
        };
        ok &= rustmc_data::fixture::available(&repo(p), p, by);
    }
    if !ok {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo(GOLDEN)).expect("read the golden"))
            .expect("json");
    let table = Recipes::load(&repo(RECIPES), &repo(TAGS)).expect("load");
    let want = doc["property_sets"]
        .as_object()
        .expect("the golden carries property_sets");
    assert_eq!(want.len(), 7, "seven RecipePropertySets");

    let mut total = 0usize;
    for key in PropertySetKey::ALL {
        let expected: Vec<&str> = want[key.id()]
            .as_array()
            .unwrap_or_else(|| panic!("{} is in the golden", key.id()))
            .iter()
            .map(|v| v.as_str().expect("item id"))
            .collect();
        let ours = table.property_set(key);
        assert_eq!(
            ours.iter().map(String::as_str).collect::<Vec<_>>(),
            expected,
            "property set `{}` differs from vanilla's",
            key.id()
        );
        assert!(
            !expected.is_empty(),
            "`{}` is empty; an all-empty set is what an unbound tag registry \
             produces and it is not a result",
            key.id()
        );
        total += expected.len();
    }
    // A floor on the whole payload, so a bug that emptied every set at once
    // could not pass the per-set equality above by emptying both sides.
    assert!(
        total > 250,
        "only {total} items across the seven sets; the golden and this server \
         would agree on an empty payload, so the size is asserted separately"
    );
    println!("update_recipes: 7 property sets, {total} item entries, all agree with vanilla");
}

/// The other half of `update_recipes`: the stonecutter's
/// `SelectableRecipe.SingleInputSet`, as an ordered sequence.
///
/// **This half IS order-deterministic**, unlike the property-set map beside it,
/// and the difference is structural rather than lucky.
/// `finalizeRecipeLoading` builds the map with `Collectors.toUnmodifiableMap`
/// — a hash-probed immutable map with a per-run salt — but builds this one by
/// walking `RecipeMap.values()` and appending to a `List`. That walk is the
/// `byKey` `ImmutableMap`'s insertion order, which is the sorted recipe id
/// order `RecipeManager.prepare`'s `TreeMap` produced. So the 319 entries have
/// exactly one correct order and it is compared as a sequence.
#[test]
fn the_stonecutter_set_matches_vanillas_in_order() {
    let mut ok = true;
    for p in [GOLDEN, RECIPES, TAGS] {
        let by = if p == GOLDEN {
            "ci/recipe-match-golden.sh"
        } else {
            "ci/dump-recipes.sh"
        };
        ok &= rustmc_data::fixture::available(&repo(p), p, by);
    }
    if !ok {
        return;
    }
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(repo(GOLDEN)).expect("read")).expect("json");
    let table = Recipes::load(&repo(RECIPES), &repo(TAGS)).expect("load");

    let want = doc["stonecutter"].as_array().expect("stonecutter set");
    assert_eq!(want.len(), 319, "319 stonecutting recipes");

    let ours: Vec<&rustmc_server::recipes::Recipe> = table
        .all()
        .iter()
        .filter(|r| r.recipe_type() == RecipeType::Stonecutting)
        .collect();
    assert_eq!(ours.len(), want.len());

    for (i, (w, r)) in want.iter().zip(ours.iter()).enumerate() {
        assert_eq!(
            r.id,
            w["recipe"].as_str().expect("recipe id"),
            "entry {i} is a different recipe; the list order is the load order \
             and a mismatch here means the sort key moved"
        );
        assert_eq!(r.result.item(), w["result"].as_str().expect("result"));
        assert_eq!(
            i64::from(r.result.count()),
            w["count"].as_i64().expect("count")
        );

        // `SingleInputEntry.input()` is the ingredient itself, so the item set
        // is compared and not just a representative of it. That is what makes
        // this sensitive to an ingredient read as a smaller set.
        let want_items: Vec<&str> = w["input"]
            .as_array()
            .expect("input")
            .iter()
            .map(|v| v.as_str().expect("item"))
            .collect();
        let rustmc_server::recipes::RecipeKind::Stonecutting { ingredient } = &r.kind else {
            panic!("{} is stonecutting", r.id)
        };
        let mut got = table.tags().expand(ingredient);
        got.sort_unstable();
        let mut want_sorted = want_items.clone();
        want_sorted.sort_unstable();
        assert_eq!(
            got, want_sorted,
            "entry {i} ({}) accepts a different set",
            r.id
        );
    }
    println!("update_recipes: 319 stonecutter entries agree with vanilla, in order");
}
