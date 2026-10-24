// RecipeMatchGolden — vanilla's OWN answer to "what does this grid craft".
//
// WHY THIS SHAPE, AND WHAT IT IS AN ORACLE FOR
//
// A recipe table can be transcribed wrongly in three ways that all look
// identical from inside: an ingredient can be read as a smaller set than it is,
// a recipe can be dropped, and — the one no unit test written from the same
// reading will ever catch — the WRONG recipe can win when two of them match the
// same grid. `RecipeManager.getRecipeFor` is `findFirst` over an
// `ImmutableMultimap` bucket built from a `TreeMap` walk, so the winner is a
// property of the whole table's sort order, not of any one recipe.
//
// So this dumper does not export recipes. It exports **(grid -> winner,
// result)** triples, and the winner is asked of vanilla's own `RecipeMap`.
// A port that loads every recipe correctly and orders them wrongly fails these
// vectors; a port that reads an ingredient as a smaller set fails them too,
// because the probe stack for a tag ingredient is chosen from vanilla's own
// resolved tag and not from ours.
//
// WHERE THE GRIDS COME FROM, AND WHY NOT FROM THE JSON
//
// From `Recipe.placementInfo()` — vanilla's own description of which stack goes
// in which slot, the same one `ServerPlaceRecipe` uses to fill a crafting grid
// from the recipe book. `slotsToIngredientIndex()` gives a slot-indexed list of
// ingredient indices with `PlacementInfo.EMPTY_SLOT` (-1) for a hole, and
// `ingredients()` gives the ingredients. Building the grid from the recipe
// DOCUMENT instead would put our own reading of the document on both sides of
// the comparison, which is this project's central defect class.
//
// `PlacementInfo.NOT_PLACEABLE` is a real answer and is recorded as one: every
// `CustomRecipe` returns it, because there is no fixed grid that makes a
// firework or a shield decoration. Those recipes appear in the `unplaceable`
// list rather than being silently absent, so the count of what this instrument
// cannot reach is a number in the file.
//
// THE NEGATIVE VECTORS ARE NOT DECORATION
//
// Half the cases replace one occupied slot with `minecraft:bedrock`, which is
// an ingredient of nothing. A table of positives alone is passed by a matcher
// that returns the first recipe unconditionally. Vanilla's answer is recorded
// rather than assumed: a handful of those perturbations genuinely still match
// something, and where they do the file says so.
//
// TAGS ARE BOUND WITH VANILLA'S OWN `TagLoader`
//
// `VanillaRegistries.createLookup()` leaves `BuiltInRegistries.ITEM` with no
// tag bindings, so every `#tag` ingredient would resolve to the empty set and
// every recipe using one would match nothing — a silent oracle producing a file
// full of `"winner": null`. The item tag documents are read out of the jar and
// run through `TagLoader.build`, whose `DependencySorter` flattens nested
// references, and the result is bound with `MappedRegistry.bindTags`. The
// bound-tag count is asserted below: an empty binding is an instrument failure
// and must not be mistaken for a finding.

package net.minecraft.world.item.crafting;

import java.io.IOException;
import java.io.InputStreamReader;
import java.io.PrintStream;
import java.io.Writer;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.TreeMap;
import java.util.zip.ZipEntry;
import java.util.zip.ZipFile;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;

import it.unimi.dsi.fastutil.ints.IntList;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.core.HolderLookup;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.tags.TagEntry;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;

public final class RecipeMatchGolden {

    /** The item nothing is made of. Every negative vector swaps one slot for it. */
    private static final Identifier POISON = Identifier.withDefaultNamespace("bedrock");

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        PrintStream log = Bootstrap.STDOUT;

        String jarPath = System.getProperty("rustmc.jar");
        Path out = Path.of(System.getProperty("rustmc.recipematch.out"));

        Map<String, JsonObject> recipeDocs = new TreeMap<>();
        Map<String, JsonObject> tagDocs = new TreeMap<>();
        try (ZipFile zip = new ZipFile(jarPath)) {
            var it = zip.entries();
            while (it.hasMoreElements()) {
                ZipEntry e = it.nextElement();
                String n = e.getName();
                if (n.startsWith("data/minecraft/recipe/") && n.endsWith(".json")) {
                    recipeDocs.put(n.substring("data/minecraft/recipe/".length(), n.length() - 5),
                                   read(zip, e));
                } else if (n.startsWith("data/minecraft/tags/item/") && n.endsWith(".json")) {
                    tagDocs.put(n.substring("data/minecraft/tags/item/".length(), n.length() - 5),
                                read(zip, e));
                }
            }
        }
        if (recipeDocs.size() < 1000) {
            throw new IllegalStateException("only " + recipeDocs.size()
                + " recipe documents in the jar — the layout moved; this is an instrument "
                + "failure, not a finding");
        }
        log.println("recipe-match: " + recipeDocs.size() + " recipe documents, "
            + tagDocs.size() + " item tag documents");

        HolderLookup.Provider registries = VanillaRegistries.createLookup();

        // Bind item data components, or every `new ItemStack(...)` throws
        // "Components not bound yet" from `Holder$Reference.components`. On a
        // real server this happens while the resource packs load; neither
        // `Bootstrap.bootStrap()` nor `Bootstrap.validate()` does it.
        // `LootGolden`'s header records the same discovery and what it cost.
        for (var pending : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(registries)) {
            pending.apply();
        }

        int bound = bindItemTags(tagDocs);
        if (bound < 100) {
            throw new IllegalStateException("only " + bound + " item tags bound — every `#tag` "
                + "ingredient would resolve empty and every case would report no winner. "
                + "Instrument failure.");
        }
        log.println("recipe-match: " + bound + " item tags bound through TagLoader.build");

        // The load order is vanilla's: `RecipeManager.prepare` walks a
        // `java.util.TreeMap` keyed by the recipe `Identifier`, and
        // `Identifier.compareTo` orders on the path before the namespace. Every
        // document here is in `minecraft`, so the TreeMap above is that order.
        List<RecipeHolder<?>> holders = new ArrayList<>();
        List<String> undecodable = new ArrayList<>();
        for (Map.Entry<String, JsonObject> e : recipeDocs.entrySet()) {
            ResourceKey<Recipe<?>> key =
                ResourceKey.create(Registries.RECIPE, Identifier.withDefaultNamespace(e.getKey()));
            try {
                holders.add(RecipeManager.fromJson(key, e.getValue(), registries));
            } catch (RuntimeException ex) {
                undecodable.add(e.getKey());
            }
        }
        if (!undecodable.isEmpty()) {
            throw new IllegalStateException("vanilla could not decode its own recipes: "
                + undecodable);
        }
        RecipeMap map = RecipeMap.create(holders);
        log.println("recipe-match: " + holders.size() + " recipes decoded by vanilla");

        List<JsonObject> cases = new ArrayList<>();
        List<String> unplaceable = new ArrayList<>();
        for (RecipeHolder<?> holder : holders) {
            String id = holder.id().identifier().getPath();
            Recipe<?> recipe = holder.value();
            PlacementInfo place = recipe.placementInfo();
            if (place.isImpossibleToPlace()) {
                unplaceable.add(id);
                continue;
            }
            List<ItemStack> stacks = probeStacks(place);
            if (stacks == null) {
                unplaceable.add(id);
                continue;
            }
            int width = recipe instanceof ShapedRecipe s ? s.getWidth() : 0;
            int height = recipe instanceof ShapedRecipe s ? s.getHeight() : 0;
            JsonObject positive = probe(map, recipe.getType(), id, stacks, width, height, false);
            cases.add(positive);

            // The negative: poison the first occupied slot. Vanilla's answer is
            // recorded whatever it is — a few of these legitimately still match.
            List<ItemStack> poisoned = new ArrayList<>(stacks);
            for (int i = 0; i < poisoned.size(); i++) {
                if (!poisoned.get(i).isEmpty()) {
                    poisoned.set(i, new ItemStack(BuiltInRegistries.ITEM.getValue(POISON)));
                    break;
                }
            }
            cases.add(probe(map, recipe.getType(), id, poisoned, width, height, true));

            // THE `lastRecipe` FAST PATH, which is not an optimisation.
            //
            // `getRecipeFor(type, input, level, RecipeHolder last)` opens
            // `if (last != null && last.value().matches(input, level)) return
            // Optional.of(last)` — BEFORE the sorted walk. So a menu holding a
            // last recipe gets THAT one even when a lower-sorting recipe also
            // matches, and `CraftingMenu.slotChangedCraftingGrid` always passes
            // one. Recorded only where it changes the answer, which is exactly
            // the 256 grids some other recipe wins: `winner_with_last` is the
            // recipe the grid was built from, and the plain `winner` is not.
            if (!positive.get("winner").isJsonNull()
                    && !positive.get("winner").getAsString().equals(id)) {
                JsonObject lastCase = probe(map, recipe.getType(), id, stacks, width, height,
                                            false, holder);
                lastCase.addProperty("last_recipe", id);
                cases.add(lastCase);
            }
        }

        JsonObject doc = new JsonObject();
        doc.add("property_sets", propertySets(holders));
        doc.add("stonecutter", stonecutterSet(holders));
        doc.addProperty("version", SharedConstants.getCurrentVersion().name());
        doc.addProperty("documents", recipeDocs.size());
        doc.addProperty("decoded", holders.size());
        doc.addProperty("tags_bound", bound);
        JsonArray un = new JsonArray();
        unplaceable.stream().sorted().forEach(un::add);
        doc.add("unplaceable", un);
        JsonArray arr = new JsonArray();
        cases.stream()
            .sorted(Comparator.comparing((JsonObject c) -> c.get("recipe").getAsString())
                .thenComparing(c -> c.get("negative").getAsBoolean())
                .thenComparing(c -> c.has("last_recipe")))
            .forEach(arr::add);
        doc.add("cases", arr);

        Gson gson = new GsonBuilder().serializeNulls().create();
        Files.createDirectories(out.getParent());
        try (Writer w = Files.newBufferedWriter(out, StandardCharsets.UTF_8)) {
            gson.toJson(doc, w);
        }
        long hits = cases.stream().filter(c -> !c.get("winner").isJsonNull()).count();
        log.println("recipe-match: " + cases.size() + " cases (" + hits + " with a winner), "
            + unplaceable.size() + " recipes not placeable -> " + out);
    }

    /**
     * One probe stack per slot, from {@code PlacementInfo}.
     *
     * <p>{@code slotsToIngredientIndex()} is slot-indexed with
     * {@link PlacementInfo#EMPTY_SLOT} for a hole; {@code ingredients()} holds the
     * distinct ingredients. The stack for an ingredient is the FIRST item of its
     * {@code HolderSet}, taken from vanilla's registry after the tags are bound —
     * so a tag ingredient is probed with a real member of vanilla's own resolved
     * tag, which is what makes this an oracle for our tag flattening and not just
     * for our matcher.
     *
     * <p>Returns null for an ingredient vanilla itself resolves to nothing.
     */
    private static List<ItemStack> probeStacks(PlacementInfo place) {
        IntList slots = place.slotsToIngredientIndex();
        List<Ingredient> ingredients = place.ingredients();
        List<ItemStack> stacks = new ArrayList<>(slots.size());
        for (int i = 0; i < slots.size(); i++) {
            int idx = slots.getInt(i);
            if (idx == PlacementInfo.EMPTY_SLOT) {
                stacks.add(ItemStack.EMPTY);
                continue;
            }
            Optional<Holder<Item>> first = ingredients.get(idx).items().findFirst();
            if (first.isEmpty()) {
                return null;
            }
            stacks.add(new ItemStack(first.get()));
        }
        return stacks;
    }

    /** Ask vanilla which recipe wins on this grid, and what it assembles. */
    private static JsonObject probe(RecipeMap map, RecipeType<?> type, String id,
                                    List<ItemStack> stacks, int width, int height,
                                    boolean negative) {
        return probe(map, type, id, stacks, width, height, negative, null);
    }

    /**
     * The same, optionally through `getRecipeFor`'s `lastRecipe` branch.
     *
     * <p>`getRecipeFor(type, input, level, RecipeHolder last)` opens
     * `if (last != null && last.value().matches(input, level)) return
     * Optional.of(last)` at offsets 0..25 — **before** the sorted walk. So it
     * is not an optimisation: a menu holding a last recipe gets that one even
     * when a lower-sorting recipe also matches the grid, and
     * `CraftingMenu.slotChangedCraftingGrid` always passes one.
     */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static JsonObject probe(RecipeMap map, RecipeType<?> type, String id,
                                    List<ItemStack> stacks, int width, int height,
                                    boolean negative, RecipeHolder<?> last) {
        // The RAW dimensions the grid is laid out in, not the trimmed ones a
        // `CraftingInput` reports. A reader has to be able to reconstruct the
        // same input, and `CraftingInput.of` trims — so publishing the trimmed
        // width would hand the reader an input it could not rebuild, and the
        // trim (which is load-bearing: it is what lets a 2x2 recipe match
        // anywhere in a 3x3) would be on the dumper's side of the comparison
        // instead of on the port's.
        int rawWidth = width > 0 ? width : Math.min(3, Math.max(1, stacks.size()));
        int rawHeight = width > 0 ? height : (stacks.size() + rawWidth - 1) / rawWidth;
        RecipeInput input = buildInput(type, stacks, width, height);
        JsonObject c = new JsonObject();
        c.addProperty("recipe", id);
        c.addProperty("negative", negative);
        c.addProperty("type", BuiltInRegistries.RECIPE_TYPE.getKey(type).getPath());
        c.addProperty("width", input instanceof CraftingInput ? rawWidth : 0);
        c.addProperty("height", input instanceof CraftingInput ? rawHeight : 0);
        JsonArray grid = new JsonArray();
        for (int i = 0; i < Math.max(stacks.size(), rawWidth * rawHeight); i++) {
            ItemStack s = at(stacks, i);
            grid.add(s.isEmpty() ? "" : BuiltInRegistries.ITEM.getKey(s.getItem()).toString());
        }
        c.add("grid", grid);

        Optional<? extends RecipeHolder<?>> winner;
        if (last != null && ((Recipe) last.value()).matches(input, null)) {
            winner = Optional.of(last);
        } else {
            winner = genericRecipesFor(map, type, input).findFirst();
        }
        if (winner.isEmpty()) {
            c.add("winner", com.google.gson.JsonNull.INSTANCE);
            c.add("result", com.google.gson.JsonNull.INSTANCE);
            c.addProperty("count", 0);
        } else {
            RecipeHolder<?> h = winner.get();
            c.addProperty("winner", h.id().identifier().getPath());
            ItemStack assembled = assemble(h.value(), input);
            if (assembled.isEmpty()) {
                // A recipe that matches and assembles nothing is a real state —
                // `SmithingTrimRecipe` reaches it — and is recorded, not hidden.
                c.add("result", com.google.gson.JsonNull.INSTANCE);
                c.addProperty("count", 0);
            } else {
                c.addProperty("result",
                    BuiltInRegistries.ITEM.getKey(assembled.getItem()).toString());
                c.addProperty("count", assembled.getCount());
            }
            if (h.value() instanceof AbstractCookingRecipe cook) {
                c.addProperty("experience", cook.experience());
                c.addProperty("cookingtime", cook.cookingTime());
            }
        }
        return c;
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static java.util.stream.Stream<? extends RecipeHolder<?>> genericRecipesFor(
            RecipeMap map, RecipeType<?> type, RecipeInput input) {
        return map.getRecipesFor((RecipeType) type, input, null);
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static ItemStack assemble(Recipe<?> recipe, RecipeInput input) {
        return ((Recipe) recipe).assemble(input);
    }

    /**
     * The input type each {@code RecipeType} is looked up with.
     *
     * <p>A shaped recipe keeps its own width and height; every other crafting
     * recipe is laid out row-major in a grid three wide, which is the widest a
     * crafting table is. `CraftingInput.of` trims to the bounding box anyway, so
     * the layout affects only which shapes can also match — and that is the
     * question, not an artefact.
     */
    private static RecipeInput buildInput(RecipeType<?> type, List<ItemStack> stacks,
                                          int width, int height) {
        if (type == RecipeType.SMITHING) {
            return new SmithingRecipeInput(at(stacks, 0), at(stacks, 1), at(stacks, 2));
        }
        if (type != RecipeType.CRAFTING) {
            return new SingleRecipeInput(at(stacks, 0));
        }
        if (width > 0 && height > 0) {
            return CraftingInput.of(width, height, stacks);
        }
        int w = Math.min(3, Math.max(1, stacks.size()));
        int h = (stacks.size() + w - 1) / w;
        List<ItemStack> padded = new ArrayList<>(stacks);
        while (padded.size() < w * h) {
            padded.add(ItemStack.EMPTY);
        }
        return CraftingInput.of(w, h, padded);
    }

    private static ItemStack at(List<ItemStack> stacks, int i) {
        return i < stacks.size() ? stacks.get(i) : ItemStack.EMPTY;
    }

    /**
     * Read the jar's item tag documents and bind them through vanilla's own
     * {@code TagLoader.build}, whose {@code DependencySorter} resolves nested
     * {@code #other/tag} references in dependency order.
     *
     * @return how many tags were bound
     */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static int bindItemTags(Map<String, JsonObject> tagDocs) {
        Map<Identifier, List<TagLoader.EntryWithSource>> entries = new LinkedHashMap<>();
        for (Map.Entry<String, JsonObject> e : tagDocs.entrySet()) {
            List<TagLoader.EntryWithSource> list = new ArrayList<>();
            JsonElement values = e.getValue().get("values");
            if (values != null && values.isJsonArray()) {
                for (JsonElement v : values.getAsJsonArray()) {
                    String s;
                    boolean required = true;
                    if (v.isJsonObject()) {
                        s = v.getAsJsonObject().get("id").getAsString();
                        JsonElement req = v.getAsJsonObject().get("required");
                        required = req == null || req.getAsBoolean();
                    } else {
                        s = v.getAsString();
                    }
                    boolean isTag = s.startsWith("#");
                    Identifier id = Identifier.parse(isTag ? s.substring(1) : s);
                    TagEntry entry = isTag
                        ? (required ? TagEntry.tag(id) : TagEntry.optionalTag(id))
                        : (required ? TagEntry.element(id) : TagEntry.optionalElement(id));
                    list.add(new TagLoader.EntryWithSource(entry, "jar"));
                }
            }
            entries.put(Identifier.withDefaultNamespace(e.getKey()), list);
        }

        TagLoader loader = new TagLoader(
            TagLoader.ElementLookup.fromFrozenRegistry(BuiltInRegistries.ITEM), "tags/item");
        Map<Identifier, List> built = loader.build(entries);
        Map<TagKey<Item>, List<Holder<Item>>> bound = new HashMap<>();
        built.forEach((id, list) -> bound.put(TagKey.create(Registries.ITEM, id), list));
        // `MappedRegistry.bindTags` is refused after `Bootstrap.bootStrap()` —
        // "Registry is already frozen", from `validateWrite`. The sanctioned
        // route on a frozen registry is the one a datapack reload takes:
        // `prepareTagReload` then `PendingTags.apply()`.
        BuiltInRegistries.ITEM
            .prepareTagReload(new TagLoader.LoadResult<>(Registries.ITEM, bound))
            .apply();
        // Touch a known item so an unbound registry cannot pass silently.
        if (!new ItemStack(Items.IRON_CHESTPLATE)
                .is(TagKey.create(Registries.ITEM,
                    Identifier.withDefaultNamespace("trimmable_armor")))) {
            throw new IllegalStateException("iron_chestplate is not in #minecraft:trimmable_armor "
                + "after binding — the nested-tag resolution did not happen, and every "
                + "smithing_trim case would report no winner");
        }
        return bound.size();
    }

    /**
     * The seven {@code RecipePropertySet}s `update_recipes` carries, computed with
     * vanilla's own extractors.
     *
     * <p>`RecipeManager.RECIPE_PROPERTY_SETS` is a private static
     * {@code Map<ResourceKey<RecipePropertySet>, IngredientExtractor>} — reflected
     * rather than reimplemented, because a golden that reimplements the thing it
     * gates is not a golden. `finalizeRecipeLoading` applies each extractor to
     * every recipe and unions the ingredients' items; the three smithing
     * extractors read `additionIngredient()`, `baseIngredient()` and
     * `templateIngredient()` off any `SmithingRecipe`, and `forSingleInput(type)`
     * reads `input()` off any `SingleItemRecipe` of that type.
     *
     * <p>The items are **sorted by id** here and the map is keyed by the property
     * set's resource id, deliberately: `finalizeRecipeLoading` collects into a
     * `Collectors.toUnmodifiableMap`, whose iteration order is randomised per JVM
     * run, and `ByteBufCodecs.map` writes entries in iteration order. The bytes
     * are therefore not reproducible even between two boots of unmodified
     * vanilla — `ci/recipe-packet-order-probe.sh` measures it — so this file
     * records the CONTENT, which is a property of the recipe table, and not the
     * wire order, which is a property of one JVM's hash salt.
     *
     * <p>No feature-flag filtering: `finalizeRecipeLoading` runs `filterDisabled`
     * against the level's `FeatureFlagSet`, and every recipe in the vanilla jar
     * is enabled under the vanilla set.
     */
    @SuppressWarnings({"unchecked", "rawtypes"})
    private static JsonObject propertySets(List<RecipeHolder<?>> holders) throws Exception {
        var field = RecipeManager.class.getDeclaredField("RECIPE_PROPERTY_SETS");
        field.setAccessible(true);
        Map<ResourceKey<?>, Object> extractors = (Map<ResourceKey<?>, Object>) field.get(null);
        var apply = Class.forName("net.minecraft.world.item.crafting.RecipeManager$IngredientExtractor")
            .getMethod("apply", Recipe.class);

        JsonObject out = new JsonObject();
        Map<String, java.util.TreeSet<String>> byKey = new TreeMap<>();
        for (Map.Entry<ResourceKey<?>, Object> e : extractors.entrySet()) {
            byKey.put(e.getKey().identifier().getPath(), new java.util.TreeSet<>());
        }
        for (Map.Entry<ResourceKey<?>, Object> e : extractors.entrySet()) {
            var items = byKey.get(e.getKey().identifier().getPath());
            for (RecipeHolder<?> h : holders) {
                Optional<Ingredient> ing = (Optional<Ingredient>) apply.invoke(e.getValue(), h.value());
                ing.ifPresent(i -> i.items().forEach(
                    it -> items.add(BuiltInRegistries.ITEM.getKey(it.value()).toString())));
            }
        }
        byKey.forEach((k, items) -> {
            JsonArray a = new JsonArray();
            items.forEach(a::add);
            out.add(k, a);
        });
        if (byKey.size() != 7) {
            throw new IllegalStateException("expected 7 recipe property sets, got " + byKey.size());
        }
        return out;
    }

    /**
     * The second field of `update_recipes`: the stonecutter's
     * {@code SelectableRecipe.SingleInputSet}, in vanilla's own list order.
     *
     * <p>`finalizeRecipeLoading` walks `RecipeMap.values()` — the `byKey`
     * `ImmutableMap`, so the load order, so sorted recipe id — and appends a
     * `SingleInputEntry(input, new SelectableRecipe(resultDisplay, ..))` for
     * every `StonecutterRecipe`. **Unlike the property-set map, this half IS
     * order-deterministic**, because it is a `List` built by an ordered walk
     * rather than a hash-probed map; that is why it is dumped as a sequence and
     * compared as one.
     *
     * <p>`SingleItemRecipe.assemble` ignores its argument entirely — its body
     * is `result.create()` — so the empty input below is not a probe value, it
     * is the absence of one.
     */
    private static JsonArray stonecutterSet(List<RecipeHolder<?>> holders) {
        JsonArray out = new JsonArray();
        for (RecipeHolder<?> h : holders) {
            if (!(h.value() instanceof StonecutterRecipe cut)) {
                continue;
            }
            JsonObject e = new JsonObject();
            e.addProperty("recipe", h.id().identifier().getPath());
            JsonArray input = new JsonArray();
            cut.input().items().forEach(
                it -> input.add(BuiltInRegistries.ITEM.getKey(it.value()).toString()));
            e.add("input", input);
            ItemStack result = cut.assemble(new SingleRecipeInput(ItemStack.EMPTY));
            e.addProperty("result", BuiltInRegistries.ITEM.getKey(result.getItem()).toString());
            e.addProperty("count", result.getCount());
            out.add(e);
        }
        return out;
    }

    private static JsonObject read(ZipFile zip, ZipEntry e) throws IOException {
        try (var in = zip.getInputStream(e)) {
            return JsonParser.parseReader(
                new InputStreamReader(in, StandardCharsets.UTF_8)).getAsJsonObject();
        }
    }

    private RecipeMatchGolden() {
    }
}
