//! Crafting: `RecipeManager.getRecipeFor(CRAFTING, ...)` over the extracted
//! recipe table.
//!
//! # Why this is a different mechanism from storing
//!
//! `560-storing-an-item-in-a-chest` asks whether an item stays where it was
//! put. `562-a-crafting-table-turns-a-log-into-planks` asks whether putting it
//! somewhere makes the server **compute** something. The result slot is not a
//! correction to a client prediction like every other container packet; it is
//! sent because a recipe matched, from `CraftingMenu.slotChangedCraftingGrid`,
//! which constructs a `ClientboundContainerSetSlotPacket` directly rather than
//! going through the synchronizer.
//!
//! # The order inside `slotChangedCraftingGrid`, which decides packet counts
//!
//! Read from offsets 0..147:
//!
//! ```text
//!   input  = craftingContainer.asCraftInput()
//!   result = EMPTY
//!   recipe = recipeManager.getRecipeFor(CRAFTING, input, level, lastRecipe)
//!   if present && resultContainer.setRecipeUsed(player, holder):
//!       assembled = recipe.assemble(input)
//!       if assembled.isItemEnabled(...): result = assembled
//!   resultContainer.setItem(0, result)
//!   menu.setRemoteSlot(0, result)                       <- offset 118
//!   connection.send(new ClientboundContainerSetSlotPacket(
//!       menu.containerId, menu.incrementStateId(), 0, result))
//! ```
//!
//! **The `setRemoteSlot` at offset 118 is the load-bearing line.** It updates
//! the belief table *before* the direct send, so the `broadcastChanges` that
//! runs immediately afterwards finds slot 0 already agreed and does **not**
//! send it a second time. Omitting it produces two packets where vanilla sends
//! one, and — because the harness follows the state id — desynchronises every
//! later click in the run.
//!
//! It also runs **inside** `clicked()`, reached from `slot.setChanged()`, and
//! therefore inside `suppressRemoteUpdates`. The suppression does not stop it,
//! because the send bypasses the synchronizer entirely. So the result slot's
//! packet **precedes** the correction for the grid slot the click just filled.
//!
//! # What is implemented
//!
//! All seven of vanilla's [`RecipeType`]s have a lookup:
//! [`Recipes::find`] for `CRAFTING`, [`Recipes::find_single`] for the four
//! cooking types and stonecutting, and [`Recipes::find_smithing`] for
//! `SMITHING`. Of the 1,585 documents in `data/vanilla/recipes.json`:
//!
//! | serializer | in the extract | loaded |
//! |---|---|---|
//! | `crafting_shaped` | 733 | 733 |
//! | `crafting_shapeless` | 323 | 323 |
//! | `stonecutting` | 319 | 319 |
//! | `smelting` | 73 | 73 |
//! | `crafting_transmute` | 33 | 32 |
//! | `blasting` | 25 | 25 |
//! | `smithing_trim` | 18 | 18 (matching only) |
//! | `smithing_transform` | 12 | 12 |
//! | `campfire_cooking` | 9 | 9 |
//! | `smoking` | 9 | 9 |
//! | the rest | 31 | 0 |
//!
//! **26.2's class list is not the one an older version's would suggest.**
//! There is no `ArmorDyeRecipe`, `MapCloningRecipe`, `TippedArrowRecipe` or
//! `ShulkerBoxColoring`; those became the data-driven `crafting_dye`,
//! `crafting_transmute` (`map_cloning`), `crafting_imbue` and
//! `crafting_decorated_pot` serializers.
//!
//! # What is refused, and why each refusal is not an approximation
//!
//! Every remaining serializer computes its result out of **data components**,
//! and this server's [`crate::item::ItemStack`] is `{item, count}` with no
//! components at all. A transcription of any of them would produce a plausible
//! wrong item — a blank map called a clone, an undyed helmet called dyed — and
//! would make the branches that return `EMPTY` unreachable, so no test of them
//! could go red. They are dropped at parse:
//!
//! * `crafting_dye` (6), `crafting_imbue` (1), `crafting_special_firework_*`
//!   (3), `crafting_special_bookcloning` (1),
//!   `crafting_special_shielddecoration` (1),
//!   `crafting_special_bannerduplicate` (16),
//!   `crafting_special_mapextending` (1), `crafting_decorated_pot` (1),
//!   `crafting_special_repairitem` (1);
//! * `map_cloning`, the one `crafting_transmute` with
//!   `add_material_count_to_result`;
//! * `SmithingTrimRecipe.assemble` — the 18 trim recipes **match**, because
//!   `SmithingRecipe.matches` is three ingredient tests and nothing else, but
//!   [`Recipe::assemble_smithing`] returns `None` for them: a trim is a
//!   `TRIM` component on the base, which this table cannot write. The
//!   smithing table assembles them itself, in
//!   `crate::workstation::smithing_create_result`.
//!
//! That is 31 documents refused and one method refused on 18 more.

use crate::item::ItemStack;
use std::collections::BTreeMap;

/// One ingredient: an item, or a tag standing for a set of items.
///
/// `Ingredient` in 26.2 is a `HolderSet`, which is either a list of items or a
/// tag reference — the same two shapes the JSON has, which is why this is an
/// enum and not a resolved set. Resolving tags at load would be faster and
/// would lose the ability to say *which* tag failed when one is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingredient {
    Item(String),
    Tag(String),
    /// An explicit list of items: `["minecraft:white_bed", ...]`.
    ///
    /// **This form was missing and its absence was not a skip.** `Ingredient`
    /// wraps a `HolderSet<Item>` and `test` is `stack.is(values)`, so the
    /// `HolderSet` codec's three shapes — one id, a `#tag`, or a JSON array of
    /// ids — are all legal in every ingredient position. 26.2's data uses the
    /// array 78 times. The parser here read `as_str()` and `filter_map`ped the
    /// failures away, which for a **shapeless** recipe deleted the ingredient
    /// from the list instead of dropping the recipe: `dye_black_bed` loaded as
    /// a one-ingredient recipe whose only ingredient is `minecraft:black_dye`,
    /// so a single black dye alone in a crafting grid assembled a black bed.
    /// 65 of the 323 shapeless recipes were in that state; the other 13 uses
    /// are `key` entries in shaped recipes, where the `?` dropped the recipe
    /// whole and cost only the recipe.
    Items(Vec<String>),
}

impl Ingredient {
    /// Parse the string JSON form: `"minecraft:oak_log"` or
    /// `"#minecraft:oak_logs"`. See [`Ingredient::parse_value`] for the array.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        s.strip_prefix('#').map_or_else(
            || Self::Item(s.to_string()),
            |tag| Self::Tag(tag.to_string()),
        )
    }

    /// Every shape `HolderSet`'s codec accepts in an ingredient position.
    ///
    /// Returns `None` for anything else, and the callers **drop the whole
    /// recipe** on `None` rather than dropping the ingredient — for the reason
    /// given on [`Ingredient::Items`].
    #[must_use]
    pub fn parse_value(v: &serde_json::Value) -> Option<Self> {
        match v {
            serde_json::Value::String(s) => Some(Self::parse(s)),
            serde_json::Value::Array(a) => {
                let items: Vec<String> = a
                    .iter()
                    .filter_map(|x| x.as_str().map(ToString::to_string))
                    .collect();
                (items.len() == a.len() && !items.is_empty()).then_some(Self::Items(items))
            }
            _ => None,
        }
    }

    /// `Ingredient.test(ItemStack)`, whose body is
    /// `stack.is(this.values)` — a `HolderSet` membership test.
    ///
    /// An empty stack never matches, which is what `ItemStack.is` answers for
    /// `EMPTY` and matters because the grid is mostly empty.
    #[must_use]
    pub fn test(&self, stack: &ItemStack, tags: &ItemTags) -> bool {
        if stack.is_empty() {
            return false;
        }
        match self {
            Self::Item(name) => name == stack.item(),
            Self::Tag(tag) => tags.contains(tag, stack.item()),
            Self::Items(items) => items.iter().any(|i| i == stack.item()),
        }
    }
}

/// `minecraft:item` tags, from `data/vanilla/tags_item.json`.
#[derive(Debug, Default)]
pub struct ItemTags(BTreeMap<String, Vec<String>>);

impl ItemTags {
    /// Parse the dumped tag document.
    ///
    /// Keys in the file are bare (`oak_logs`); a recipe names them namespaced
    /// (`#minecraft:oak_logs`). The `minecraft:` prefix is stripped on lookup
    /// rather than added on load, because a modded tag would carry a different
    /// namespace and adding one here would silently match the wrong tag.
    ///
    /// # Errors
    /// If the document is not an object of `{ "values": [...] }`.
    pub fn parse(doc: &serde_json::Value) -> std::io::Result<Self> {
        let obj = doc
            .as_object()
            .ok_or_else(|| std::io::Error::other("tags_item.json is not an object"))?;
        let mut out = BTreeMap::new();
        for (name, body) in obj {
            let values = body
                .get("values")
                .and_then(serde_json::Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(ToString::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            out.insert(name.clone(), values);
        }
        Ok(Self::flatten(&out))
    }

    /// `TagLoader.build`, which resolves a tag's `#other_tag` entries into the
    /// items of that tag.
    ///
    /// **This was not being done, and it was not a small gap.** The comment
    /// this replaces said nested references were "not followed" because
    /// "nothing the corpus crafts needs one". 50 of the 224 item tags in the
    /// 26.2 extract are nothing *but* nested references: `trimmable_armor` is
    /// `["#foot_armor", "#leg_armor", "#chest_armor", "#head_armor"]` and
    /// resolved to the **empty set** here, so every one of the 18
    /// `smithing_trim` recipes matched nothing at all.
    ///
    /// Vanilla's shape: `build` feeds every tag to a `DependencySorter` and
    /// calls `orderByDependencies`, so each tag is built after the tags it
    /// names and a `TagEntry.Lookup` reads their finished lists. A cycle has
    /// no valid order and the sorter refuses it. This is the same algorithm
    /// written as an explicit walk with a `visiting` stack: a tag on a cycle
    /// contributes nothing across the back edge, and never loops.
    #[must_use]
    fn flatten(raw: &BTreeMap<String, Vec<String>>) -> Self {
        fn resolve(
            raw: &BTreeMap<String, Vec<String>>,
            done: &mut BTreeMap<String, Vec<String>>,
            visiting: &mut Vec<String>,
            name: &str,
        ) -> Vec<String> {
            if let Some(v) = done.get(name) {
                return v.clone();
            }
            if visiting.iter().any(|v| v == name) {
                return Vec::new();
            }
            visiting.push(name.to_string());
            let mut out: Vec<String> = Vec::new();
            for entry in raw.get(name).map_or(&[][..], Vec::as_slice) {
                if let Some(inner) = entry.strip_prefix('#') {
                    let key = inner.strip_prefix("minecraft:").unwrap_or(inner);
                    for item in resolve(raw, done, visiting, key) {
                        if !out.contains(&item) {
                            out.push(item);
                        }
                    }
                } else if !out.contains(entry) {
                    out.push(entry.clone());
                }
            }
            visiting.pop();
            done.insert(name.to_string(), out.clone());
            out
        }

        let mut done: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut visiting = Vec::new();
        for name in raw.keys() {
            resolve(raw, &mut done, &mut visiting, name);
        }
        Self(done)
    }

    /// Whether `item` is in `tag`.
    ///
    /// Nested references are already resolved — see [`ItemTags::flatten`].
    #[must_use]
    pub fn contains(&self, tag: &str, item: &str) -> bool {
        let key = tag.strip_prefix("minecraft:").unwrap_or(tag);
        self.0
            .get(key)
            .is_some_and(|values| values.iter().any(|v| v == item))
    }

    /// Every item an ingredient accepts — `Ingredient.items()`, which streams
    /// the `HolderSet` it wraps.
    #[must_use]
    pub fn expand(&self, ingredient: &Ingredient) -> Vec<String> {
        match ingredient {
            Ingredient::Item(name) => vec![name.clone()],
            Ingredient::Items(items) => items.clone(),
            Ingredient::Tag(tag) => {
                let key = tag.strip_prefix("minecraft:").unwrap_or(tag);
                self.0.get(key).cloned().unwrap_or_default()
            }
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// `RecipeType` — the key of `RecipeMap.byType`, and therefore the unit a
/// lookup is scoped to.
///
/// `RecipeType`'s `static {}` registers exactly seven, in this order:
/// `crafting`, `smelting`, `blasting`, `smoking`, `campfire_cooking`,
/// `stonecutting`, `smithing`. There is no separate type per *serializer* —
/// every `crafting_*` document, `crafting_shaped` and `crafting_transmute`
/// alike, lands under `CRAFTING`, and both smithing serializers land under
/// `SMITHING`. That is why a transmute recipe this server drops is not merely
/// a missing feature: it is a recipe removed from the middle of the list
/// `getRecipeFor(CRAFTING, ...)` walks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RecipeType {
    Crafting,
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
    Stonecutting,
    Smithing,
}

impl RecipeType {
    /// The registry path, from the seven `ldc` strings in `RecipeType`'s
    /// `static {}`.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Crafting => "crafting",
            Self::Smelting => "smelting",
            Self::Blasting => "blasting",
            Self::Smoking => "smoking",
            Self::CampfireCooking => "campfire_cooking",
            Self::Stonecutting => "stonecutting",
            Self::Smithing => "smithing",
        }
    }
}

/// The four `AbstractCookingRecipe` subclasses, which differ only in their
/// `RecipeType` and in the `cookingtime` default their codec carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CookingKind {
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
}

impl CookingKind {
    /// `AbstractCookingRecipe.cookingMapCodec(factory, N)` in each subclass's
    /// `static {}`: `SmeltingRecipe` pushes `sipush 200`, and the other three
    /// `bipush 100`. These are **codec** defaults, so a document that omits
    /// `cookingtime` silently takes them, which almost all of them do.
    #[must_use]
    pub const fn default_cooking_time(self) -> i32 {
        match self {
            Self::Smelting => 200,
            Self::Blasting | Self::Smoking | Self::CampfireCooking => 100,
        }
    }

    #[must_use]
    pub const fn recipe_type(self) -> RecipeType {
        match self {
            Self::Smelting => RecipeType::Smelting,
            Self::Blasting => RecipeType::Blasting,
            Self::Smoking => RecipeType::Smoking,
            Self::CampfireCooking => RecipeType::CampfireCooking,
        }
    }
}

/// The seven `RecipePropertySet` keys, from `RecipePropertySet`'s `static {}`
/// in the order its `ldc`s appear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PropertySetKey {
    SmithingBase,
    SmithingTemplate,
    SmithingAddition,
    FurnaceInput,
    BlastFurnaceInput,
    SmokerInput,
    CampfireInput,
}

impl PropertySetKey {
    /// All seven, for a caller that has to build the whole packet.
    pub const ALL: [Self; 7] = [
        Self::SmithingBase,
        Self::SmithingTemplate,
        Self::SmithingAddition,
        Self::FurnaceInput,
        Self::BlastFurnaceInput,
        Self::SmokerInput,
        Self::CampfireInput,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::SmithingBase => "smithing_base",
            Self::SmithingTemplate => "smithing_template",
            Self::SmithingAddition => "smithing_addition",
            Self::FurnaceInput => "furnace_input",
            Self::BlastFurnaceInput => "blast_furnace_input",
            Self::SmokerInput => "smoker_input",
            Self::CampfireInput => "campfire_input",
        }
    }
}

/// A recipe this server can run.
#[derive(Debug, Clone, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub kind: RecipeKind,
    pub result: ItemStack,
}

impl Recipe {
    /// `Recipe.getType()`.
    #[must_use]
    pub const fn recipe_type(&self) -> RecipeType {
        self.kind.recipe_type()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecipeKind {
    /// Order does not matter; every ingredient must be matched by exactly one
    /// stack in the grid and no stack may be left over.
    Shapeless { ingredients: Vec<Ingredient> },
    /// A `width x height` pattern of optional ingredients, in row-major order.
    Shaped {
        width: usize,
        height: usize,
        cells: Vec<Option<Ingredient>>,
    },
    /// `AbstractCookingRecipe`, which is a `SingleItemRecipe` plus an
    /// experience reward and a cooking time.
    Cooking {
        cooking: CookingKind,
        ingredient: Ingredient,
        experience: f32,
        cooking_time: i32,
    },
    /// `StonecutterRecipe` — a bare `SingleItemRecipe`.
    Stonecutting { ingredient: Ingredient },
    /// `TransmuteRecipe` — one `input` stack plus `material_count` copies of
    /// `material`, producing `result` carrying the input's components.
    Transmute {
        input: Ingredient,
        material: Ingredient,
        /// `material_count`, `optionalFieldOf(..., MinMaxBounds.Ints.exactly(1))`.
        /// `min()` and `max()` are read with `.orElse(1)` and `.orElse(8)`
        /// respectively inside `matches`, so an open bound is not unbounded.
        material_min: Option<i32>,
        material_max: Option<i32>,
        /// `add_material_count_to_result`, `optionalFieldOf(..., false)`.
        add_material_count_to_result: bool,
    },
    /// `SmithingTransformRecipe`. `template` and `addition` are
    /// `Optional<Ingredient>`, and an absent one demands an **empty** slot —
    /// see [`test_optional_ingredient`].
    SmithingTransform {
        template: Option<Ingredient>,
        base: Ingredient,
        addition: Option<Ingredient>,
    },
    /// `SmithingTrimRecipe`. Its `matches` is the same three ingredient tests
    /// as `SmithingTransformRecipe`'s — both come from the `SmithingRecipe`
    /// interface's `default` method — but its **assemble is refused**; see
    /// [`Recipe::assemble_smithing`].
    SmithingTrim {
        template: Option<Ingredient>,
        base: Ingredient,
        addition: Option<Ingredient>,
        /// The `TrimPattern` id. Carried so the refusal can name what it would
        /// have had to write, and so the recipe is not silently identical to a
        /// transform.
        pattern: String,
    },
}

/// `Ingredient.testOptionalIngredient(Optional<Ingredient>, ItemStack)`:
/// `opt.map(i -> i.test(stack)).orElseGet(stack::isEmpty)`.
///
/// **The `orElseGet` is the whole content of this function.** An absent
/// ingredient does not mean "anything goes"; it means the slot must be
/// *empty*. A port that treated `None` as an unconditional pass would let a
/// netherite upgrade fire with junk in the template slot.
#[must_use]
pub fn test_optional_ingredient(
    ingredient: Option<&Ingredient>,
    stack: &ItemStack,
    tags: &ItemTags,
) -> bool {
    ingredient.map_or_else(|| stack.is_empty(), |i| i.test(stack, tags))
}

/// The smithing table's three slots, in `SmithingRecipeInput`'s own order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SmithingInput {
    pub template: ItemStack,
    pub base: ItemStack,
    pub addition: ItemStack,
}

impl SmithingInput {
    /// `SmithingRecipeInput.isEmpty()` is `RecipeInput`'s default — every
    /// stack empty — and `getRecipesFor` returns an empty stream for it before
    /// any recipe is consulted.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.template.is_empty() && self.base.is_empty() && self.addition.is_empty()
    }
}

impl RecipeKind {
    #[must_use]
    pub const fn recipe_type(&self) -> RecipeType {
        match self {
            Self::Shapeless { .. } | Self::Shaped { .. } => RecipeType::Crafting,
            Self::Cooking { cooking, .. } => cooking.recipe_type(),
            Self::Stonecutting { .. } => RecipeType::Stonecutting,
            Self::Transmute { .. } => RecipeType::Crafting,
            Self::SmithingTransform { .. } | Self::SmithingTrim { .. } => RecipeType::Smithing,
        }
    }
}

/// `Item.getCraftingRemainder()` — what `CraftingRecipe.defaultCraftingReminder`
/// leaves in a cell after a craft consumes it.
///
/// Five items in the pinned jar are registered with
/// `Item.Properties.craftRemainder` (the five `invokevirtual craftRemainder`
/// sites in `Items.<clinit>`, offsets 12349, 12380, 12469, 16418 and 18971):
/// the three filled buckets leave a bucket and dragon's breath and a honey
/// bottle leave a glass bottle. Every other item's field is `null`, which the
/// reminder writes as `EMPTY`.
#[must_use]
pub fn crafting_remainder(item: &str) -> Option<&'static str> {
    Some(match item {
        "minecraft:water_bucket" | "minecraft:lava_bucket" | "minecraft:milk_bucket" => {
            "minecraft:bucket"
        }
        "minecraft:dragon_breath" | "minecraft:honey_bottle" => "minecraft:glass_bottle",
        _ => return None,
    })
}

/// The 3x3 grid, trimmed to the bounding box of what is in it.
///
/// `CraftingInput.of` calls `ofPositioned`, which walks the grid tracking the
/// minimum and maximum non-empty row and column and returns an input of only
/// that sub-rectangle. **A shaped recipe is therefore matched against the
/// trimmed grid and needs no offset search** — a 2x2 recipe matches a 2x2
/// input wherever in the 3x3 the player put it, because the input is 2x2
/// either way. An implementation that skipped the trim would have to search
/// offsets to compensate, and would then also match patterns vanilla does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftingInput {
    pub width: usize,
    pub height: usize,
    pub items: Vec<ItemStack>,
}

impl CraftingInput {
    /// `CraftingInput.of(width, height, items)` — `ofPositioned(..).input()`.
    #[must_use]
    pub fn of(width: usize, height: usize, items: &[ItemStack]) -> Self {
        Self::positioned(width, height, items).0
    }

    /// `CraftingInput.ofPositioned(width, height, items)`: the trimmed input
    /// and the column and row of the grid its top-left cell came from —
    /// `Positioned.left()` and `top()`, which `ResultSlot.onTake` adds back to
    /// map each trimmed cell onto the grid slot it consumes. An empty grid is
    /// `Positioned.EMPTY`, at (0, 0).
    #[must_use]
    pub fn positioned(width: usize, height: usize, items: &[ItemStack]) -> (Self, usize, usize) {
        let (mut min_x, mut min_y) = (usize::MAX, usize::MAX);
        let (mut max_x, mut max_y) = (0usize, 0usize);
        let mut any = false;
        for y in 0..height {
            for x in 0..width {
                if items.get(y * width + x).is_some_and(|s| !s.is_empty()) {
                    any = true;
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if !any {
            return (
                Self {
                    width: 0,
                    height: 0,
                    items: Vec::new(),
                },
                0,
                0,
            );
        }
        let (w, h) = (max_x - min_x + 1, max_y - min_y + 1);
        let mut trimmed = Vec::with_capacity(w * h);
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                trimmed.push(
                    items
                        .get(y * width + x)
                        .cloned()
                        .unwrap_or_else(ItemStack::empty),
                );
            }
        }
        (
            Self {
                width: w,
                height: h,
                items: trimmed,
            },
            min_x,
            min_y,
        )
    }

    /// `CraftingInput.size()` — the trimmed cell count, **not** 9.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.width * self.height
    }

    /// `CraftingInput.ingredientCount()` — how many non-empty stacks.
    #[must_use]
    pub fn ingredient_count(&self) -> usize {
        self.items.iter().filter(|s| !s.is_empty()).count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    fn get(&self, i: usize) -> ItemStack {
        self.items.get(i).cloned().unwrap_or_else(ItemStack::empty)
    }
}

impl Recipe {
    /// Whether this recipe matches the grid.
    #[must_use]
    pub fn matches(&self, input: &CraftingInput, tags: &ItemTags) -> bool {
        match &self.kind {
            RecipeKind::Shapeless { ingredients } => {
                // `ShapelessRecipe.matches`, offsets 0..69.
                if input.ingredient_count() != ingredients.len() {
                    return false;
                }
                // The one-ingredient fast path is vanilla's own, at offset 18,
                // and it is not merely an optimisation — it is the branch
                // `562` takes.
                if input.size() == 1 && ingredients.len() == 1 {
                    return ingredients[0].test(&input.get(0), tags);
                }
                // `StackedItemContents.canCraft`, which is a maximum bipartite
                // matching between ingredients and the stacks present. A
                // backtracking search gives the same **boolean** answer, which
                // is all `matches` returns; vanilla's version is faster and
                // also reports what to consume, which nothing here needs.
                let stacks: Vec<ItemStack> = input
                    .items
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect();
                let mut used = vec![false; stacks.len()];
                match_all(ingredients, &stacks, &mut used, 0, tags)
            }
            RecipeKind::Shaped {
                width,
                height,
                cells,
            } => {
                // `ShapedRecipePattern.matches`: the trimmed input must be
                // exactly the pattern's size, tried unmirrored then mirrored.
                if input.width != *width || input.height != *height {
                    return false;
                }
                self.pattern_matches(input, cells, *width, *height, false, tags)
                    || self.pattern_matches(input, cells, *width, *height, true, tags)
            }
            RecipeKind::Transmute {
                input: input_ing,
                material,
                material_min,
                material_max,
                ..
            } => self.transmute_matches(
                input,
                input_ing,
                material,
                *material_min,
                *material_max,
                tags,
            ),
            // Not `RecipeType.CRAFTING`. `getRecipeFor` is scoped to a type
            // before any `matches` runs, so vanilla never asks a cooking
            // recipe about a grid at all; answering `false` rather than
            // panicking keeps a caller that forgot the scope honest without
            // making a wrong match possible.
            RecipeKind::Cooking { .. }
            | RecipeKind::Stonecutting { .. }
            | RecipeKind::SmithingTransform { .. }
            | RecipeKind::SmithingTrim { .. } => false,
        }
    }

    /// `TransmuteRecipe.matches(CraftingInput, Level)`, offsets 0..206.
    ///
    /// One stack testing `input`, between `min` and `max` testing `material`,
    /// and **nothing else in the grid at all** — the `else` at offset 121 is
    /// an unconditional `return false`, so a foreign item anywhere refuses
    /// even if the input and material are both present.
    ///
    /// The tail at offset 156 is the rule that stops a black bundle being
    /// dyed black: when the result would be a single item, it is assembled and
    /// compared with `ItemStack.isSameItemSameComponents` against the input,
    /// and an identical one refuses. Vanilla's comparison includes components;
    /// ours compares item and count, because this server's `ItemStack` carries
    /// no components (`crate::item::ItemStack`), so the two agree on every
    /// stack this server can construct.
    fn transmute_matches(
        &self,
        input: &CraftingInput,
        input_ing: &Ingredient,
        material: &Ingredient,
        material_min: Option<i32>,
        material_max: Option<i32>,
        tags: &ItemTags,
    ) -> bool {
        // `minMaterialCount` is `materialCount.min().orElse(1)` and
        // `maxMaterialCount` is `materialCount.max().orElse(8)`. The 8 is not
        // a grid bound — it is what an open upper bound means.
        let min = material_min.unwrap_or(1);
        let max = material_max.unwrap_or(8);
        let count = i32::try_from(input.ingredient_count()).unwrap_or(i32::MAX);
        if count < min + 1 || count > max + 1 {
            return false;
        }
        let mut found: Option<ItemStack> = None;
        let mut materials = 0i32;
        for i in 0..input.size() {
            let stack = input.get(i);
            if stack.is_empty() {
                continue;
            }
            if input_ing.test(&stack, tags) {
                if found.is_some() {
                    return false; // two inputs
                }
                found = Some(stack);
            } else if material.test(&stack, tags) {
                materials += 1;
                if materials > max {
                    return false;
                }
            } else {
                return false; // a foreign item
            }
        }
        let Some(found) = found.filter(|s| !s.is_empty()) else {
            return false;
        };
        // `MinMaxBounds.Ints.matches`, which for the absent bounds this data
        // uses is `exactly(1)`.
        if materials < min || materials > max {
            return false;
        }
        if self.transmute_result_size(materials) != 1 {
            return true;
        }
        let assembled = self.transmute_result(&found, 0);
        if assembled.is_empty() {
            return false;
        }
        !ItemStack::is_same_item_same_components(&found, &assembled)
    }

    /// `TransmuteRecipe.computeResultSize(int)`.
    fn transmute_result_size(&self, materials: i32) -> i32 {
        match &self.kind {
            RecipeKind::Transmute {
                add_material_count_to_result: true,
                ..
            } => materials + self.result.count(),
            _ => self.result.count(),
        }
    }

    /// `TransmuteRecipe.computeResult(ItemStack, int)`, which is
    /// `createWithOriginalComponents(result, input, extra)`:
    /// `result.apply(result.count() + extra, input.getComponentsPatch())`.
    ///
    /// The component patch is a no-op here — this server's `ItemStack` has no
    /// components — so what survives is the item and the count.
    fn transmute_result(&self, input: &ItemStack, extra: i32) -> ItemStack {
        let _ = input;
        ItemStack::new(self.result.item(), self.result.count() + extra)
    }

    /// `SmithingRecipe.matches(SmithingRecipeInput, Level)` — the `default`
    /// method on the interface, shared by both smithing serializers.
    ///
    /// Three tests in this order, short-circuiting: template, base, addition.
    /// The template and addition go through
    /// [`test_optional_ingredient`], where an absent ingredient demands an
    /// **empty** slot.
    #[must_use]
    pub fn matches_smithing(&self, input: &SmithingInput, tags: &ItemTags) -> bool {
        let (template, base, addition) = match &self.kind {
            RecipeKind::SmithingTransform {
                template,
                base,
                addition,
            }
            | RecipeKind::SmithingTrim {
                template,
                base,
                addition,
                ..
            } => (template.as_ref(), base, addition.as_ref()),
            _ => return false,
        };
        test_optional_ingredient(template, &input.template, tags)
            && base.test(&input.base, tags)
            && test_optional_ingredient(addition, &input.addition, tags)
    }

    /// `SmithingTransformRecipe.assemble` — and a **refusal** for
    /// `SmithingTrimRecipe.assemble`.
    ///
    /// Transform is
    /// `TransmuteRecipe.createWithOriginalComponents(result, input.base())`:
    /// the result template with the base's component patch copied across.
    /// This server's stacks carry no components, so that is the result item
    /// and its count, exactly.
    ///
    /// **Trim is refused, and this is a refusal rather than an approximation.**
    /// `SmithingTrimRecipe.assemble` is `applyTrim(base, addition, pattern)`,
    /// whose whole body is data components: it reads
    /// `DataComponents.PROVIDES_TRIM_MATERIAL` off the addition, returns
    /// `ItemStack.EMPTY` when the addition has none (offset 73), builds
    /// `new ArmorTrim(material, pattern)`, returns `EMPTY` again when the base
    /// already carries an equal `DataComponents.TRIM` (offset 48), and
    /// otherwise returns `base.copyWithCount(1)` with that `TRIM` set. This
    /// server's `ItemStack` is `{item, count}`. Writing `base.copy_with_count(1)`
    /// would produce an untrimmed copy of the base — visibly the wrong item —
    /// and would make **both** `EMPTY` branches unreachable, so no test of
    /// them could go red. `matches_smithing` is transcribed and gated; the
    /// result is `None` until this server has components.
    #[must_use]
    pub fn assemble_smithing(&self, input: &SmithingInput) -> Option<ItemStack> {
        match &self.kind {
            RecipeKind::SmithingTransform { .. } => {
                let _ = &input.base;
                Some(self.result.clone())
            }
            _ => None,
        }
    }

    /// `SingleItemRecipe.matches(SingleRecipeInput, Level)` — the whole body
    /// is `this.input.test(input.item())`. `StonecutterRecipe` and all four
    /// `AbstractCookingRecipe` subclasses inherit it unchanged, which is why
    /// 435 of the 529 unserved recipe kinds are one line of matching.
    ///
    /// Note what is **not** here: no count check and no leftover check. One
    /// coal ore in a furnace matches whether the stack is 1 or 64, because
    /// `SingleRecipeInput` holds exactly one stack by construction.
    #[must_use]
    pub fn matches_single(&self, stack: &ItemStack, tags: &ItemTags) -> bool {
        match &self.kind {
            RecipeKind::Cooking { ingredient, .. } | RecipeKind::Stonecutting { ingredient } => {
                ingredient.test(stack, tags)
            }
            RecipeKind::Shapeless { .. }
            | RecipeKind::Shaped { .. }
            | RecipeKind::Transmute { .. }
            | RecipeKind::SmithingTransform { .. }
            | RecipeKind::SmithingTrim { .. } => false,
        }
    }

    /// `Recipe.assemble(input)` for the crafting types.
    ///
    /// Shaped and shapeless return their result template unchanged.
    /// `TransmuteRecipe` is the one crafting kind whose result is computed
    /// from the grid: `assemble` walks for the stack testing `input` and
    /// returns `computeResult(that, extra)`, which is
    /// `result.apply(result.count() + extra, that.getComponentsPatch())`.
    ///
    /// With no `add_material_count_to_result`, `extra` is 0 and the component
    /// patch is empty on every stack this server can build, so the result is
    /// the template — which is why this does not need to find the input stack
    /// to answer correctly, and says so rather than walking a grid for a value
    /// it would discard.
    ///
    /// **`add_material_count_to_result` is refused.** No recipe in 26.2 sets
    /// it (all 33 `crafting_transmute` documents omit both it and
    /// `material_count`), so a transcription of that arm would be a branch no
    /// vector could reach and no test could turn red. The recipe is dropped at
    /// parse rather than assembled wrongly; see [`Recipes::parse`].
    #[must_use]
    pub fn assemble(&self, input: &CraftingInput) -> ItemStack {
        let _ = input;
        match &self.kind {
            RecipeKind::Transmute { .. } => self.transmute_result(&ItemStack::empty(), 0),
            _ => self.result.clone(),
        }
    }

    /// `AbstractCookingRecipe.experience()`, or 0 for a recipe that has none.
    #[must_use]
    pub const fn experience(&self) -> f32 {
        match &self.kind {
            RecipeKind::Cooking { experience, .. } => *experience,
            _ => 0.0,
        }
    }

    /// `AbstractCookingRecipe.cookingTime()`, or 0 for a recipe that has none.
    #[must_use]
    pub const fn cooking_time(&self) -> i32 {
        match &self.kind {
            RecipeKind::Cooking { cooking_time, .. } => *cooking_time,
            _ => 0,
        }
    }

    fn pattern_matches(
        &self,
        input: &CraftingInput,
        cells: &[Option<Ingredient>],
        width: usize,
        height: usize,
        mirrored: bool,
        tags: &ItemTags,
    ) -> bool {
        for y in 0..height {
            for x in 0..width {
                let cx = if mirrored { width - 1 - x } else { x };
                let stack = input.get(y * width + x);
                match cells.get(y * width + cx).and_then(Option::as_ref) {
                    // A blank in the pattern must be an empty cell in the
                    // grid, not merely "anything" — otherwise a recipe would
                    // match a grid with extra items scattered around it.
                    None => {
                        if !stack.is_empty() {
                            return false;
                        }
                    }
                    Some(ing) => {
                        if !ing.test(&stack, tags) {
                            return false;
                        }
                    }
                }
            }
        }
        true
    }
}

/// Exact matching of every ingredient to a distinct stack.
fn match_all(
    ingredients: &[Ingredient],
    stacks: &[ItemStack],
    used: &mut Vec<bool>,
    i: usize,
    tags: &ItemTags,
) -> bool {
    if i == ingredients.len() {
        // Every ingredient placed. `ingredientCount == ingredients.size()` was
        // checked first, so every stack is used too.
        return true;
    }
    for (j, stack) in stacks.iter().enumerate() {
        if used[j] || !ingredients[i].test(stack, tags) {
            continue;
        }
        used[j] = true;
        if match_all(ingredients, stacks, used, i + 1, tags) {
            return true;
        }
        used[j] = false;
    }
    false
}

/// Every crafting recipe, in the order the extract lists them.
#[derive(Debug, Default)]
pub struct Recipes {
    recipes: Vec<Recipe>,
    tags: ItemTags,
}

impl Recipes {
    /// Load from `data/vanilla/recipes.json` and `data/vanilla/tags_item.json`.
    ///
    /// # Errors
    /// If either file is missing or not the expected shape.
    pub fn load(recipes: &std::path::Path, tags: &std::path::Path) -> std::io::Result<Self> {
        let r: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(recipes)?)
            .map_err(std::io::Error::other)?;
        let t: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(tags)?).map_err(std::io::Error::other)?;
        Self::parse(&r, &t)
    }

    /// The same from already-parsed documents — what the tests use, for the
    /// reason [`crate::player::ItemIds::parse`] gives at length.
    ///
    /// # Errors
    /// If either document is not the expected shape.
    pub fn parse(recipes: &serde_json::Value, tags: &serde_json::Value) -> std::io::Result<Self> {
        let tags = ItemTags::parse(tags)?;
        let obj = recipes
            .as_object()
            .ok_or_else(|| std::io::Error::other("recipes.json is not an object"))?;
        let mut out = Vec::new();
        for (id, body) in obj {
            let Some(kind) = body.get("type").and_then(serde_json::Value::as_str) else {
                continue;
            };
            // A `smithing_trim` document carries **no `result` field at all**
            // — `SmithingTrimRecipe` has no result template, because its
            // output is the base stack with a `TRIM` component set. All 18 of
            // them were being dropped here before the arm below existed, which
            // is why the exception is stated rather than made silently by
            // defaulting the result.
            let result = match parse_result(body.get("result")) {
                Some(r) => r,
                // A recipe whose result this cannot read is skipped rather
                // than defaulted: a wrong result is a crafting table that
                // hands out the wrong item, which is worse than one that
                // declines.
                None if kind == "minecraft:smithing_trim" => ItemStack::empty(),
                None => continue,
            };
            let kind = match kind {
                "minecraft:crafting_shapeless" => {
                    let Some(list) = body
                        .get("ingredients")
                        .and_then(serde_json::Value::as_array)
                    else {
                        continue;
                    };
                    // `collect::<Option<Vec<_>>>` and not `filter_map`: one
                    // unreadable entry must drop the recipe, because a
                    // shortened ingredient list is a recipe that matches a
                    // SMALLER grid. See `Ingredient::Items`.
                    let Some(ingredients) = list
                        .iter()
                        .map(Ingredient::parse_value)
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    if ingredients.is_empty() {
                        continue;
                    }
                    RecipeKind::Shapeless { ingredients }
                }
                "minecraft:crafting_shaped" => {
                    let Some(k) = parse_shaped(body) else {
                        continue;
                    };
                    k
                }
                "minecraft:smelting"
                | "minecraft:blasting"
                | "minecraft:smoking"
                | "minecraft:campfire_cooking" => {
                    let cooking = match kind {
                        "minecraft:smelting" => CookingKind::Smelting,
                        "minecraft:blasting" => CookingKind::Blasting,
                        "minecraft:smoking" => CookingKind::Smoking,
                        _ => CookingKind::CampfireCooking,
                    };
                    let Some(k) = parse_cooking(body, cooking) else {
                        continue;
                    };
                    k
                }
                "minecraft:stonecutting" => {
                    let Some(ingredient) = parse_ingredient(body.get("ingredient")) else {
                        continue;
                    };
                    RecipeKind::Stonecutting { ingredient }
                }
                "minecraft:crafting_transmute" => {
                    let (Some(input), Some(material)) = (
                        parse_ingredient(body.get("input")),
                        parse_ingredient(body.get("material")),
                    ) else {
                        continue;
                    };
                    // REFUSED, and the one recipe it costs is named. 26.2 has
                    // exactly one: `map_cloning`, with
                    // `add_material_count_to_result: true` and
                    // `material_count {min: 1, max: 8}` — the recipe that
                    // copies a filled map onto N blank ones. Its whole content
                    // is the component patch `computeResult` carries across
                    // (`MAP_ID` and the map decorations); this server's
                    // `ItemStack` has none, so transcribing the arm would hand
                    // the player N blank `filled_map`s and call it cloning.
                    // Dropped, which is a missing recipe rather than a wrong
                    // result.
                    if body
                        .get("add_material_count_to_result")
                        .and_then(serde_json::Value::as_bool)
                        == Some(true)
                    {
                        continue;
                    }
                    // `optionalFieldOf("material_count", DEFAULT_MATERIAL_COUNT)`,
                    // and `DEFAULT_MATERIAL_COUNT` is
                    // `MinMaxBounds.Ints.exactly(1)` — **both** bounds set to
                    // 1, not an open range. An absent field therefore means
                    // exactly one material, where `minMaterialCount`'s
                    // `orElse(1)` / `maxMaterialCount`'s `orElse(8)` are what
                    // an explicitly *open* bound means. Conflating the two
                    // lets a bundle be dyed with up to eight dyes.
                    let (material_min, material_max) =
                        body.get("material_count").map_or((Some(1), Some(1)), |b| {
                            (bounds_field(Some(b), "min"), bounds_field(Some(b), "max"))
                        });
                    RecipeKind::Transmute {
                        input,
                        material,
                        material_min,
                        material_max,
                        add_material_count_to_result: false,
                    }
                }
                "minecraft:smithing_transform" | "minecraft:smithing_trim" => {
                    let Some(base) = parse_ingredient(body.get("base")) else {
                        continue;
                    };
                    let template = parse_ingredient(body.get("template"));
                    let addition = parse_ingredient(body.get("addition"));
                    if kind == "minecraft:smithing_transform" {
                        RecipeKind::SmithingTransform {
                            template,
                            base,
                            addition,
                        }
                    } else {
                        let Some(pattern) = body.get("pattern").and_then(serde_json::Value::as_str)
                        else {
                            continue;
                        };
                        RecipeKind::SmithingTrim {
                            template,
                            base,
                            addition,
                            pattern: pattern.to_string(),
                        }
                    }
                }
                // Every `crafting_special_*`, and every remaining type.
                // Skipped, not approximated — see the module header.
                _ => continue,
            };
            out.push(Recipe {
                id: id.clone(),
                kind,
                result,
            });
        }
        Ok(Self { recipes: out, tags })
    }

    /// `RecipeManager.getRecipeFor(RecipeType.CRAFTING, input, level, last)`.
    ///
    /// # The tie-break, transcribed rather than assumed away
    ///
    /// This comment used to say the order "is a `BTreeMap`'s key order here
    /// and vanilla's own registry order there", that "the two are not the
    /// same", and that it only matters for a grid matching two recipes. Two
    /// of those three claims are wrong, and the chain is short enough to
    /// settle:
    ///
    /// * `RecipeManager.prepare` opens `new java.util.TreeMap()` (offset 0)
    ///   and hands it to `SimpleJsonResourceReloadListener.scanDirectory`,
    ///   then walks it with `SortedMap.forEach` into an `ArrayList`. So the
    ///   load order is the **sorted** order of the recipe ids.
    /// * `RecipeMap.create` puts each holder into an
    ///   `ImmutableMultimap.Builder` keyed by `getType()`. An
    ///   `ImmutableMultimap` preserves insertion order within a key, so
    ///   `byType(t)` yields that sorted order restricted to `t`.
    /// * `RecipeMap.getRecipesFor` streams `byType(t)` through a `filter` and
    ///   `getRecipeFor` takes `findFirst`. **First match wins, in sorted id
    ///   order.** There is no registry order and no priority field.
    /// * The sort is `Identifier.compareTo`, which compares **path first and
    ///   namespace second** — the reverse of how the id prints.
    ///
    /// Ours is a `serde_json::Map`, which without the `preserve_order` feature
    /// is a `BTreeMap<String, _>` over the extract's keys. `ci/dump-recipes.sh`
    /// writes a `minecraft:` recipe under its bare path, so for a
    /// vanilla-only table our key **is** vanilla's sort key and Rust's byte
    /// order agrees with `String.compareTo` on the ASCII these ids are. The
    /// two orders are therefore identical today, and
    /// `the_load_order_is_vanillas_sorted_path_order` gates it.
    ///
    /// The one live gap: a non-`minecraft` recipe is keyed `ns:name` by the
    /// dumper, which sorts on the namespace, where vanilla would sort it on
    /// the path. No such recipe exists in the pinned jar.
    #[must_use]
    pub fn find(&self, input: &CraftingInput) -> Option<&Recipe> {
        if input.is_empty() {
            return None;
        }
        self.recipes
            .iter()
            .filter(|r| r.recipe_type() == RecipeType::Crafting)
            .find(|r| r.matches(input, &self.tags))
    }

    /// `RecipeManager.getRecipeFor(type, input, level, RecipeHolder last)` —
    /// the four-argument overload, which `CraftingMenu` always calls.
    ///
    /// **The `lastRecipe` branch is a behaviour, not a cache.** Offsets 0..25
    /// are `if (last != null && last.value().matches(input, level)) return
    /// Optional.of(last)`, and they run **before** the sorted walk. So a menu
    /// holding a last recipe gets that one even when a lower-sorting recipe
    /// also matches — the crafting table's answer depends on what it made
    /// last, not only on what is in the grid.
    ///
    /// That is not a corner: 256 of the 1,561 probe grids in
    /// `data/vanilla/recipe_match.json` are won by a recipe other than the one
    /// they were built from, and passing the built-from recipe as `last` flips
    /// **all 256**. A port that ignored the argument would craft the wrong
    /// item every time a player made two different things from one arrangement
    /// of ingredients.
    ///
    /// The other overload takes a `ResourceKey` and resolves it through
    /// `byKeyTyped` first, which is a lookup and not a second rule; `last` here
    /// is the id, resolved the same way.
    #[must_use]
    pub fn find_with_last(&self, input: &CraftingInput, last: Option<&str>) -> Option<&Recipe> {
        // Note the ORDER: vanilla tests `last.matches` without first checking
        // `input.isEmpty()`. An empty input reaches `RecipeMap.getRecipesFor`'s
        // early return only on the fall-through path, and `matches` refuses it
        // anyway, so the two agree — but the branch is transcribed where
        // vanilla puts it.
        if let Some(last) = last {
            let held = self
                .recipes
                .iter()
                .find(|r| r.id == last && r.recipe_type() == RecipeType::Crafting);
            if let Some(r) = held {
                if r.matches(input, &self.tags) {
                    return Some(r);
                }
            }
        }
        self.find(input)
    }

    /// `RecipeAccess.stonecutterRecipes().selectByInput(stack)` — every
    /// stonecutting recipe whose ingredient accepts `stack`, **in recipe-id
    /// order**, which is the order of the stonecutter's button list.
    ///
    /// The order is not a choice. `RecipeManager.prepare` scans the recipe
    /// directory into a `java.util.TreeMap` keyed by `Identifier` (offset 0),
    /// copies it into an `ArrayList` in that order (31-54) and builds
    /// `RecipeMap.create` from the list; `RecipeMap.byKey` is an
    /// `ImmutableMap`, which keeps insertion order, and
    /// `finalizeRecipeLoading` walks `RecipeMap.values()` (offset 41) to build
    /// the `SingleInputSet` the stonecutter reads. `Identifier.compareTo`
    /// compares the path first, and every recipe is `minecraft:`, so the list
    /// is sorted by path. A client's button `n` is entry `n` of this.
    ///
    /// `selectByInput` keeps the entries whose `input().test(stack)` holds;
    /// an empty stack is handled by the caller, which never asks.
    #[must_use]
    pub fn stonecutter_recipes_for(&self, stack: &ItemStack) -> Vec<&Recipe> {
        let mut out: Vec<&Recipe> = self
            .recipes
            .iter()
            .filter(|r| matches!(r.kind, RecipeKind::Stonecutting { .. }))
            .filter(|r| r.matches_single(stack, &self.tags))
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    /// The recipe whose id is `id`, if it is loaded.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&Recipe> {
        self.recipes.iter().find(|r| r.id == id)
    }

    /// The same for the one-slot types, which `AbstractFurnaceBlockEntity`
    /// reaches through `RecipeManager.CachedCheck`.
    #[must_use]
    pub fn find_single_with_last(
        &self,
        ty: RecipeType,
        stack: &ItemStack,
        last: Option<&str>,
    ) -> Option<&Recipe> {
        if let Some(last) = last {
            let held = self
                .recipes
                .iter()
                .find(|r| r.id == last && r.recipe_type() == ty);
            if let Some(r) = held {
                if r.matches_single(stack, &self.tags) {
                    return Some(r);
                }
            }
        }
        self.find_single(ty, stack)
    }

    /// `RecipeManager.getRecipeFor(type, new SingleRecipeInput(stack), level)`
    /// for the five one-slot types: the four cooking ones and stonecutting.
    ///
    /// `RecipeMap.getRecipesFor` returns `Stream.empty()` for an input whose
    /// `isEmpty()` is true, before any recipe is consulted — so an empty
    /// furnace slot matches nothing even though `Ingredient.test` would also
    /// have said no. Both give the same answer here; the early return is
    /// transcribed because it is the line that would still be right if an
    /// ingredient ever tested an empty stack true.
    #[must_use]
    pub fn find_single(&self, ty: RecipeType, stack: &ItemStack) -> Option<&Recipe> {
        if stack.is_empty() {
            return None;
        }
        self.recipes
            .iter()
            .filter(|r| r.recipe_type() == ty)
            .find(|r| r.matches_single(stack, &self.tags))
    }

    /// How many recipes of one `RecipeType` were loaded — the size of the
    /// `byType` bucket `find_single` and `find` walk.
    #[must_use]
    pub fn len_of(&self, ty: RecipeType) -> usize {
        self.recipes
            .iter()
            .filter(|r| r.recipe_type() == ty)
            .count()
    }

    /// `RecipeManager.finalizeRecipeLoading`'s seven `RecipePropertySet`s —
    /// the whole first half of the `update_recipes` packet.
    ///
    /// **`update_recipes` no longer carries recipes.** In 26.2
    /// `ClientboundUpdateRecipesPacket` is a record of exactly two things: a
    /// `Map<ResourceKey<RecipePropertySet>, RecipePropertySet>` — seven named
    /// sets of item ids, used by the client to grey out a slot that can never
    /// accept what is in it — and a `SelectableRecipe.SingleInputSet` for the
    /// stonecutter, encoded with `noRecipeCodec()` so the recipes themselves
    /// are elided. The recipe table reaches the client through
    /// `recipe_book_add` displays instead.
    ///
    /// The seven extractors, from `RECIPE_PROPERTY_SETS`'s `Map.of` and the
    /// three `lambda$static$*` bodies beside it:
    ///
    /// | key | extractor |
    /// |---|---|
    /// | `furnace_input` | `SingleItemRecipe.input()` of every `SMELTING` |
    /// | `blast_furnace_input` | ditto, `BLASTING` |
    /// | `smoker_input` | ditto, `SMOKING` |
    /// | `campfire_input` | ditto, `CAMPFIRE_COOKING` |
    /// | `smithing_base` | `SmithingRecipe.baseIngredient()` |
    /// | `smithing_template` | `SmithingRecipe.templateIngredient()` |
    /// | `smithing_addition` | `SmithingRecipe.additionIngredient()` |
    ///
    /// `forSingleInput` tests `getType() == type && recipe instanceof
    /// SingleItemRecipe`, so **stonecutting contributes to no property set**
    /// even though it is a `SingleItemRecipe`: there is no key for it. The two
    /// smithing `Optional` extractors contribute nothing for a recipe whose
    /// template or addition is absent, which is not the same as contributing
    /// the empty set.
    ///
    /// Returned sorted and deduplicated. That is a decision, not a
    /// convenience: `finalizeRecipeLoading` collects into a
    /// `Collectors.toUnmodifiableMap` and `ByteBufCodecs.map` writes its
    /// entries in iteration order, which is randomised per JVM run — so there
    /// is no vanilla order to reproduce. See `ci/recipe-packet-order-probe.sh`.
    #[must_use]
    pub fn property_set(&self, key: PropertySetKey) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.recipes {
            let ingredient = match (key, &r.kind) {
                (
                    PropertySetKey::FurnaceInput,
                    RecipeKind::Cooking {
                        cooking: CookingKind::Smelting,
                        ingredient,
                        ..
                    },
                )
                | (
                    PropertySetKey::BlastFurnaceInput,
                    RecipeKind::Cooking {
                        cooking: CookingKind::Blasting,
                        ingredient,
                        ..
                    },
                )
                | (
                    PropertySetKey::SmokerInput,
                    RecipeKind::Cooking {
                        cooking: CookingKind::Smoking,
                        ingredient,
                        ..
                    },
                )
                | (
                    PropertySetKey::CampfireInput,
                    RecipeKind::Cooking {
                        cooking: CookingKind::CampfireCooking,
                        ingredient,
                        ..
                    },
                ) => Some(ingredient),
                (
                    PropertySetKey::SmithingBase,
                    RecipeKind::SmithingTransform { base, .. }
                    | RecipeKind::SmithingTrim { base, .. },
                ) => Some(base),
                (
                    PropertySetKey::SmithingTemplate,
                    RecipeKind::SmithingTransform {
                        template: Some(i), ..
                    }
                    | RecipeKind::SmithingTrim {
                        template: Some(i), ..
                    },
                )
                | (
                    PropertySetKey::SmithingAddition,
                    RecipeKind::SmithingTransform {
                        addition: Some(i), ..
                    }
                    | RecipeKind::SmithingTrim {
                        addition: Some(i), ..
                    },
                ) => Some(i),
                _ => None,
            };
            if let Some(i) = ingredient {
                out.extend(self.tags.expand(i));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Every recipe, in load order. The order is load-bearing (see
    /// [`Recipes::find`]), so this is a slice and not a set.
    #[must_use]
    pub fn all(&self) -> &[Recipe] {
        &self.recipes
    }

    /// The result of crafting `input`, or an empty stack.
    ///
    /// `slotChangedCraftingGrid` sets the result slot to `EMPTY` when nothing
    /// matches, and that write is what **clears** a stale result when the
    /// player takes the last ingredient out of the grid.
    #[must_use]
    pub fn assemble(&self, input: &CraftingInput) -> ItemStack {
        self.find(input)
            .map_or_else(ItemStack::empty, |r| r.assemble(input))
    }

    /// `RecipeManager.getRecipeFor(RecipeType.SMITHING, input, level)`.
    ///
    /// Both smithing serializers share the type, so one walk covers the 12
    /// transforms and the 18 trims and the sorted-id tie-break decides between
    /// them exactly as it does in the crafting bucket.
    #[must_use]
    pub fn find_smithing(&self, input: &SmithingInput) -> Option<&Recipe> {
        if input.is_empty() {
            return None;
        }
        self.recipes
            .iter()
            .filter(|r| r.recipe_type() == RecipeType::Smithing)
            .find(|r| r.matches_smithing(input, &self.tags))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.recipes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }

    #[must_use]
    pub const fn tags(&self) -> &ItemTags {
        &self.tags
    }
}

/// One `Ingredient` field: `"minecraft:andesite"` or `"#minecraft:planks"`.
///
/// 26.2 writes a `SingleItemRecipe`'s ingredient as a **bare string**, not as
/// `{"item": ...}`. Reading it as an object would drop all 435 of them and
/// look exactly like the extract being empty.
fn parse_ingredient(v: Option<&serde_json::Value>) -> Option<Ingredient> {
    Ingredient::parse_value(v?)
}

/// One end of `MinMaxBounds.Ints`, which serialises as `{"min": n, "max": n}`
/// or as a bare number meaning both.
fn bounds_field(v: Option<&serde_json::Value>, which: &str) -> Option<i32> {
    let v = v?;
    if let Some(n) = v.as_i64() {
        return i32::try_from(n).ok();
    }
    v.get(which)
        .and_then(serde_json::Value::as_i64)
        .and_then(|n| i32::try_from(n).ok())
}

/// `AbstractCookingRecipe.cookingMapCodec`: `ingredient` and `result` are
/// required, `experience` is `optionalFieldOf("experience", 0.0f)` and
/// `cookingtime` is `optionalFieldOf("cookingtime", <per-type default>)`.
fn parse_cooking(body: &serde_json::Value, cooking: CookingKind) -> Option<RecipeKind> {
    Some(RecipeKind::Cooking {
        cooking,
        ingredient: parse_ingredient(body.get("ingredient"))?,
        #[allow(clippy::cast_possible_truncation)]
        experience: body
            .get("experience")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(0.0) as f32,
        cooking_time: body
            .get("cookingtime")
            .and_then(serde_json::Value::as_i64)
            .and_then(|n| i32::try_from(n).ok())
            .unwrap_or_else(|| cooking.default_cooking_time()),
    })
}

/// `{ "id": "minecraft:oak_planks", "count": 4 }`, where `count` defaults to 1.
fn parse_result(v: Option<&serde_json::Value>) -> Option<ItemStack> {
    let v = v?;
    let id = v.get("id").and_then(serde_json::Value::as_str)?;
    let count = v
        .get("count")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(1);
    Some(ItemStack::new(id, i32::try_from(count).unwrap_or(1)))
}

/// `ShapedRecipePattern.shrink(List<String>)`, transcribed from offsets 0..166.
///
/// **This was missing and it silently disabled seven recipes.** A pattern is
/// written for legibility, not tightly: `spyglass` is `[" # ", " X ", " X "]`
/// and `mace` is `[" # ", " I "]`. Vanilla shrinks the pattern to its bounding
/// box before storing it, so `spyglass` is a **1x3** recipe. Ours stored the
/// 3x3 with blank border columns, and `CraftingInput.of` trims the *grid* to
/// its bounding box — so a player placing the two items correctly presented a
/// 1x3 input against a 3x3 pattern and it never matched. `creaking_heart`,
/// `mace`, `spyglass` and the four `waxed_*_chiseled_copper` recipes could not
/// be crafted on this server at all, and nothing said so: a recipe that never
/// matches looks exactly like a recipe nobody tried.
///
/// The walk, in vanilla's own variables: `minCol` is the smallest
/// `firstNonEmpty` over the rows, `maxCol` the largest `lastNonEmpty`;
/// leading blank rows are counted only while the count still equals the row
/// index, and the trailing count resets to zero on any non-blank row. An
/// all-blank pattern shrinks to nothing.
fn shrink(rows: &[&str]) -> Vec<String> {
    let mut min_col = usize::MAX;
    let mut max_col: i64 = 0;
    let mut leading = 0usize;
    let mut trailing = 0usize;
    for (i, row) in rows.iter().enumerate() {
        let chars: Vec<char> = row.chars().collect();
        // `firstNonEmpty` returns the string's LENGTH for an all-blank row,
        // which is what makes `Math.min` ignore it rather than clamp to 0.
        let first = chars.iter().position(|c| *c != ' ').unwrap_or(chars.len());
        min_col = min_col.min(first);
        let last: i64 = chars
            .iter()
            .rposition(|c| *c != ' ')
            .map_or(-1, |p| i64::try_from(p).unwrap_or(-1));
        max_col = max_col.max(last);
        if last < 0 {
            if leading == i {
                leading += 1;
            }
            trailing += 1;
        } else {
            trailing = 0;
        }
    }
    if rows.len() == trailing {
        return Vec::new();
    }
    let hi = usize::try_from(max_col + 1).unwrap_or(0);
    let mut out = Vec::with_capacity(rows.len() - trailing - leading);
    for row in &rows[leading..rows.len() - trailing] {
        let chars: Vec<char> = row.chars().collect();
        let lo = min_col.min(chars.len());
        let hi = hi.min(chars.len()).max(lo);
        out.push(chars[lo..hi].iter().collect::<String>());
    }
    out
}

/// `pattern` plus `key`, flattened into a row-major grid of optional
/// ingredients.
fn parse_shaped(body: &serde_json::Value) -> Option<RecipeKind> {
    let raw: Vec<&str> = body
        .get("pattern")?
        .as_array()?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    if raw.is_empty() {
        return None;
    }
    let rows = shrink(&raw);
    if rows.is_empty() {
        return None;
    }
    let key = body.get("key")?.as_object()?;
    let width = rows.iter().map(|r| r.chars().count()).max()?;
    let height = rows.len();
    let mut cells = Vec::with_capacity(width * height);
    for row in &rows {
        let mut chars: Vec<char> = row.chars().collect();
        // A pattern row shorter than the widest is padded with blanks, which
        // vanilla's own parser refuses; padding rather than refusing keeps a
        // hand-edited datapack from taking the whole table down.
        chars.resize(width, ' ');
        for c in chars {
            if c == ' ' {
                cells.push(None);
                continue;
            }
            // A pattern character with no `key` entry makes the whole recipe
            // unreadable, and the recipe is dropped rather than the cell being
            // treated as blank — a blank would silently turn a 3x3 recipe into
            // one that matches a smaller grid.
            let v = key.get(&c.to_string())?;
            cells.push(Some(Ingredient::parse_value(v)?));
        }
    }
    Some(RecipeKind::Shaped {
        width,
        height,
        cells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> serde_json::Value {
        serde_json::json!({
            "oak_logs": { "values": [
                "minecraft:oak_log", "minecraft:oak_wood",
                "minecraft:stripped_oak_log", "minecraft:stripped_oak_wood"
            ]},
            "planks": { "values": ["minecraft:oak_planks"] }
        })
    }

    fn recipes() -> Recipes {
        Recipes::parse(
            &serde_json::json!({
                "oak_planks": {
                    "type": "minecraft:crafting_shapeless",
                    "ingredients": ["#minecraft:oak_logs"],
                    "result": { "count": 4, "id": "minecraft:oak_planks" }
                },
                "stick": {
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["#", "#"],
                    "key": { "#": "#minecraft:planks" },
                    "result": { "count": 4, "id": "minecraft:stick" }
                },
                "chest": {
                    "type": "minecraft:crafting_shaped",
                    "pattern": ["###", "# #", "###"],
                    "key": { "#": "#minecraft:planks" },
                    "result": { "id": "minecraft:chest" }
                },
                "a_special_one": {
                    "type": "minecraft:crafting_special_armordye",
                    "result": { "id": "minecraft:leather_helmet" }
                }
            }),
            &tags(),
        )
        .expect("parse")
    }

    /// A 3x3 grid with one item at a given index.
    fn grid(at: usize, item: &str) -> CraftingInput {
        let mut items = vec![ItemStack::empty(); 9];
        items[at] = ItemStack::one(item);
        CraftingInput::of(3, 3, &items)
    }

    /// **Scenario 562's claim.** One oak log anywhere in the grid gives four
    /// oak planks. The `anywhere` is the point — it is what says the trim is
    /// happening.
    #[test]
    fn one_oak_log_anywhere_in_the_grid_makes_four_planks() {
        let r = recipes();
        for at in 0..9 {
            let out = r.assemble(&grid(at, "minecraft:oak_log"));
            assert_eq!(
                (out.item(), out.count()),
                ("minecraft:oak_planks", 4),
                "a log in grid cell {at} did not make four planks"
            );
        }
        // The scenario puts it in menu slot 1, which is grid cell 0.
        assert_eq!(r.assemble(&grid(0, "minecraft:oak_log")).count(), 4);
    }

    /// The tag, not the item: `#minecraft:oak_logs` holds four items and any
    /// of them crafts. A recipe table that resolved the tag to its name would
    /// match none of them.
    #[test]
    fn any_member_of_the_ingredient_tag_matches() {
        let r = recipes();
        for item in [
            "minecraft:oak_log",
            "minecraft:oak_wood",
            "minecraft:stripped_oak_log",
            "minecraft:stripped_oak_wood",
        ] {
            assert_eq!(
                r.assemble(&grid(4, item)).item(),
                "minecraft:oak_planks",
                "{item} is in #minecraft:oak_logs and should craft"
            );
        }
        // And something that is not in the tag does not.
        assert!(r.assemble(&grid(4, "minecraft:birch_log")).is_empty());
        assert!(r.assemble(&grid(4, "minecraft:stone")).is_empty());
    }

    /// An empty grid crafts nothing, and this is the branch that **clears** a
    /// stale result when the player takes the last ingredient back out. A
    /// recipe engine that only ever wrote a result on a match would leave four
    /// planks sitting in the output slot for ever.
    #[test]
    fn an_empty_grid_makes_nothing() {
        let r = recipes();
        let empty = CraftingInput::of(3, 3, &vec![ItemStack::empty(); 9]);
        assert!(empty.is_empty());
        assert_eq!(empty.size(), 0);
        assert!(r.assemble(&empty).is_empty());
        assert!(r.find(&empty).is_none());
    }

    /// **Two logs do not make eight planks.** `ingredientCount` must equal the
    /// ingredient list's length, so a shapeless recipe with one ingredient
    /// refuses a grid holding two stacks. Without that check the fast path
    /// would never be reached and the matcher would happily match a subset.
    #[test]
    fn a_leftover_stack_refuses_a_shapeless_recipe() {
        let r = recipes();
        let mut items = vec![ItemStack::empty(); 9];
        items[0] = ItemStack::one("minecraft:oak_log");
        items[4] = ItemStack::one("minecraft:oak_log");
        let two = CraftingInput::of(3, 3, &items);
        assert_eq!(two.ingredient_count(), 2);
        assert!(
            r.assemble(&two).is_empty(),
            "one ingredient cannot consume two stacks"
        );
    }

    /// The trim, measured directly. Two logs in the middle column give a 1x2
    /// input, not a 3x3 one — which is what makes a shaped recipe match
    /// wherever it is placed without an offset search.
    #[test]
    fn the_input_is_trimmed_to_its_bounding_box() {
        let mut items = vec![ItemStack::empty(); 9];
        items[1] = ItemStack::one("minecraft:oak_planks");
        items[4] = ItemStack::one("minecraft:oak_planks");
        let input = CraftingInput::of(3, 3, &items);
        assert_eq!((input.width, input.height), (1, 2));
        assert_eq!(input.size(), 2);
        // The same two planks in the left column trim to the same 1x2.
        let mut items = vec![ItemStack::empty(); 9];
        items[0] = ItemStack::one("minecraft:oak_planks");
        items[3] = ItemStack::one("minecraft:oak_planks");
        assert_eq!(CraftingInput::of(3, 3, &items), input);
    }

    /// A shaped recipe matches wherever it is placed, and only in its own
    /// shape. Two planks stacked make sticks; two side by side do not.
    #[test]
    fn a_shaped_recipe_matches_its_shape_and_no_other() {
        let r = recipes();
        let planks = |at: &[usize]| {
            let mut items = vec![ItemStack::empty(); 9];
            for i in at {
                items[*i] = ItemStack::one("minecraft:oak_planks");
            }
            CraftingInput::of(3, 3, &items)
        };
        // Vertical, in three different places.
        for pair in [[0, 3], [1, 4], [5, 8]] {
            let out = r.assemble(&planks(&pair));
            assert_eq!(
                (out.item(), out.count()),
                ("minecraft:stick", 4),
                "two stacked planks at {pair:?} make sticks"
            );
        }
        // Horizontal is a different shape and makes nothing.
        for pair in [[0, 1], [4, 5], [6, 7]] {
            assert!(
                r.assemble(&planks(&pair)).is_empty(),
                "two planks side by side at {pair:?} are not a stick recipe"
            );
        }
    }

    /// A pattern hole must be **empty**, not merely "not the ingredient". A
    /// chest is eight planks around a gap; eight planks with something in the
    /// gap is not a chest.
    #[test]
    fn a_hole_in_a_pattern_must_be_empty() {
        let r = recipes();
        let mut items = vec![ItemStack::one("minecraft:oak_planks"); 9];
        items[4] = ItemStack::empty();
        assert_eq!(
            r.assemble(&CraftingInput::of(3, 3, &items)).item(),
            "minecraft:chest"
        );
        // Fill the hole and it stops being a chest.
        let full = vec![ItemStack::one("minecraft:oak_planks"); 9];
        assert!(r.assemble(&CraftingInput::of(3, 3, &full)).is_empty());
    }

    /// The special recipes are skipped rather than half-parsed. A
    /// `crafting_special_armordye` in the table must not become a recipe that
    /// matches an empty grid and hands out a leather helmet.
    #[test]
    fn special_recipes_are_skipped_not_approximated() {
        let r = recipes();
        assert_eq!(r.len(), 3, "three real recipes, the special one dropped");
        assert!(r.recipes.iter().all(|x| x.id != "a_special_one"));
    }

    /// `ShapedRecipePattern.shrink`, with the case that was breaking real
    /// recipes and a control that separates shrinking from not shrinking.
    #[test]
    fn a_pattern_is_shrunk_to_its_bounding_box() {
        // `spyglass`, verbatim. Three columns written, one column of content.
        assert_eq!(shrink(&[" # ", " X ", " X "]), ["#", "X", "X"]);
        // `mace`: two rows, one column.
        assert_eq!(shrink(&[" # ", " I "]), ["#", "I"]);
        // Leading and trailing blank ROWS go too, and the leading count only
        // advances while it still equals the row index — so a blank row in the
        // MIDDLE is kept, which is what makes a `chest`-shaped hole possible.
        assert_eq!(shrink(&["   ", "###", "   "]), ["###"]);
        assert_eq!(shrink(&["#", " ", "#"]), ["#", " ", "#"]);
        // An all-blank pattern shrinks to nothing rather than to one blank row.
        assert!(shrink(&["   ", "   "]).is_empty());
        // CONTROL: a pattern already tight is returned unchanged, so a test of
        // the seven real recipes could not pass on an implementation that
        // shrank everything to its first cell.
        assert_eq!(shrink(&["###", "# #", "###"]), ["###", "# #", "###"]);
        assert_eq!(shrink(&["#", "#"]), ["#", "#"]);
    }

    /// The recipes that could not be crafted on this server at all.
    ///
    /// A 3x3 pattern with blank border columns never matches, because
    /// `CraftingInput.of` trims the GRID to its bounding box and the pattern
    /// was not trimmed to match. Nothing said so: a recipe that never matches
    /// is indistinguishable from one nobody tried.
    #[test]
    fn a_loosely_written_pattern_still_crafts() {
        let r = Recipes::parse(
            &serde_json::json!({
                "spyglass": {
                    "type": "minecraft:crafting_shaped",
                    "pattern": [" # ", " X ", " X "],
                    "key": { "#": "minecraft:amethyst_shard", "X": "minecraft:copper_ingot" },
                    "result": { "id": "minecraft:spyglass" }
                }
            }),
            &serde_json::json!({}),
        )
        .expect("parse");
        let RecipeKind::Shaped { width, height, .. } = &r.all()[0].kind else {
            panic!("shaped")
        };
        assert_eq!((*width, *height), (1, 3), "shrunk from the 3x3 as written");

        let at = |cells: &[(usize, &str)]| {
            let mut items = vec![ItemStack::empty(); 9];
            for (i, item) in cells {
                items[*i] = ItemStack::one(item);
            }
            CraftingInput::of(3, 3, &items)
        };
        // Down the left column, the middle, and the right: all the same 1x3.
        for col in 0..3 {
            let out = r.assemble(&at(&[
                (col, "minecraft:amethyst_shard"),
                (col + 3, "minecraft:copper_ingot"),
                (col + 6, "minecraft:copper_ingot"),
            ]));
            assert_eq!(out.item(), "minecraft:spyglass", "column {col}");
        }
        // And the shape still has to be right.
        assert!(r
            .assemble(&at(&[
                (0, "minecraft:amethyst_shard"),
                (1, "minecraft:copper_ingot"),
                (2, "minecraft:copper_ingot"),
            ]))
            .is_empty());
    }

    /// Nested tag references are flattened, as `TagLoader.build` does through
    /// its `DependencySorter`.
    ///
    /// The nesting is two deep and declared **out of dependency order** in the
    /// document, because a resolver that only handled one level, or that only
    /// worked when a tag appeared after the tags it names, would pass a
    /// one-level in-order test.
    #[test]
    fn a_nested_tag_reference_is_flattened() {
        let tags = ItemTags::parse(&serde_json::json!({
            "outer": { "values": ["#minecraft:middle", "minecraft:dirt"] },
            "middle": { "values": ["#minecraft:inner"] },
            "inner": { "values": ["minecraft:stone"] }
        }))
        .expect("parse");
        assert!(tags.contains("inner", "minecraft:stone"));
        assert!(tags.contains("middle", "minecraft:stone"));
        assert!(tags.contains("outer", "minecraft:stone"), "two levels deep");
        assert!(
            tags.contains("outer", "minecraft:dirt"),
            "and its own entry"
        );
        assert!(!tags.contains("inner", "minecraft:dirt"));
    }

    /// A cycle terminates and costs only the back edge, which is the property
    /// `DependencySorter` gets by refusing the cycle outright.
    ///
    /// Without this the resolver would recurse until the stack ran out, and a
    /// hand-edited datapack would take the whole server down at load.
    #[test]
    fn a_cyclic_tag_reference_terminates() {
        let tags = ItemTags::parse(&serde_json::json!({
            "a": { "values": ["#minecraft:b", "minecraft:stone"] },
            "b": { "values": ["#minecraft:a", "minecraft:dirt"] }
        }))
        .expect("parse");
        // Whichever end the walk starts from, both tags keep their own plain
        // entries and neither loops.
        assert!(tags.contains("a", "minecraft:stone"));
        assert!(tags.contains("b", "minecraft:dirt"));
    }
}
