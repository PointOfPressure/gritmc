// ItemPropsGolden — every item's **default data components**, from the pinned jar.
//
// # The gap this closes, named by our own source
//
// `crates/rustmc-server/src/item.rs` carries a constant called
// `DEFAULT_MAX_STACK_SIZE = 64` and a doc comment admitting what it is: every
// item this server can stack is assumed to stack to 64, because
// `data/vanilla/items.json` is the registry *report* and the report has no
// such column. A bucket, a sword, an ender pearl and a snowball are all 64
// there. The comment says the fix is "one body to change when the column
// lands". This is the column.
//
// It is deliberately not a `max_stack_size` dumper. In 26.2 `Item.Properties`
// is a builder over `DataComponentMap`: `stacksTo` writes
// `minecraft:max_stack_size`, `durability` writes `minecraft:max_damage` and
// `minecraft:damage`, `rarity` writes `minecraft:rarity`, `food` writes
// `minecraft:food` and `minecraft:consumable`, `sword`/`pickaxe`/`axe` write
// `minecraft:tool` and `minecraft:attribute_modifiers` and
// `minecraft:weapon`, `humanoidArmor` writes `minecraft:equippable`. Every
// one of those tables the brief asks for is the same table read at a
// different key, and a dumper per key would be five dumpers that can
// disagree with each other about which item they walked.
//
// So the walk is `Item.components()` — the very map
// `Item.getDefaultMaxStackSize` reads through
// `components().getOrDefault(DataComponents.MAX_STACK_SIZE, 1)` (offsets
// 0..22 of that method, `javap -p -c -constants`). An item whose properties
// change moves this table on the next regeneration, because it is read off
// the field vanilla itself dispatches through rather than off a list.
//
// # Why it needs more than `Bootstrap.bootStrap()`
//
// `Item.components()` is `builtInRegistryHolder.components()`, and
// `Holder.Reference.components()` throws `Components not bound yet` on a JVM
// that has only bootstrapped: `Item.<init>` registers an *initializer* into
// `BuiltInRegistries.DATA_COMPONENT_INITIALIZERS` and a real server applies
// them from `ReloadableServerResources.updateComponentsAndStaticRegistryTags`
// during a datapack load. `UseGolden.bindItemComponents` already found and
// paid for that (39 refusals in `use_without_item.json` before it existed);
// the same two calls are made here, with the same assertion that they did
// something, because a binding that silently stopped working would make this
// table an exception trace rather than a short one.
//
// # Iteration order is imposed, not observed
//
// `DataComponentMap.keySet()` on a built map is a `Reference2ObjectMap`
// keyed by identity, so its order is a property of the JVM's allocation and
// not of vanilla. Components are therefore emitted sorted by their
// `minecraft:data_component_type` registry id, which is the order the wire
// uses and the order a version diff should read in. The *item* walk is
// `BuiltInRegistries.ITEM`'s own registration order and is left alone —
// that one is stable and is what every other golden in this tree relies on.
//
// Converted to JSON by `ci/item-props-golden.sh`.

import com.google.gson.Gson;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonPrimitive;
import com.mojang.serialization.DataResult;
import com.mojang.serialization.JsonOps;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.component.DataComponentInitializers;
import net.minecraft.core.component.DataComponentType;
import net.minecraft.core.component.TypedDataComponent;
import net.minecraft.core.Holder;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.block.Block;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryOps;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.item.Item;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.component.Tool;

public final class ItemPropsGolden {

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        bindItemComponents(lookup);
        bindBlockTags();
        RegistryOps<JsonElement> ops = lookup.createSerializationContext(JsonOps.INSTANCE);
        // **The equipment-break entity events, by slot.**
        //
        // `LivingEntity.onEquippedItemBroken` broadcasts
        // `entityEventForEquipmentBreak(slot)`, whose body is a
        // `tableswitch` over a `$SwitchMap` — so which arm is MAINHAND
        // **cannot be read off the listing**, exactly as
        // `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`
        // could not (docs/cold-start/items.md). Invoked here instead, once
        // per slot, so the mapping is vanilla's answer rather than a reading
        // of the compiler's numbering.
        JsonObject breakEvents = new JsonObject();
        try {
            java.lang.reflect.Method entityEvent =
                net.minecraft.world.entity.LivingEntity.class.getDeclaredMethod(
                    "entityEventForEquipmentBreak",
                    net.minecraft.world.entity.EquipmentSlot.class);
            entityEvent.setAccessible(true);
            for (net.minecraft.world.entity.EquipmentSlot slot
                    : net.minecraft.world.entity.EquipmentSlot.values()) {
                breakEvents.addProperty(slot.name(), (Byte) entityEvent.invoke(null, slot));
            }
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot read entityEventForEquipmentBreak", e);
        }


        JsonArray table = new JsonArray();
        JsonArray refusals = new JsonArray();
        // Every distinct `HolderSet` a `minecraft:tool` rule names, resolved
        // to the blocks in it. See the note on `resolveToolBlockSets`.
        JsonObject blockSets = new JsonObject();
        int items = 0;
        int componentsWritten = 0;

        for (Item item : BuiltInRegistries.ITEM) {
            Identifier itemId = BuiltInRegistries.ITEM.getKey(item);
            if (itemId == null) {
                continue;
            }
            items++;

            // Sorted by component-type registry id — see the header. The list
            // is materialised first so the sort is over the whole map rather
            // than over whatever the iterator happened to hand out.
            List<TypedDataComponent<?>> typed = new ArrayList<>();
            for (TypedDataComponent<?> c : item.components()) {
                typed.add(c);
            }
            typed.sort(Comparator.comparing(c -> String.valueOf(idOf(c.type()))));

            JsonObject comps = new JsonObject();
            for (TypedDataComponent<?> c : typed) {
                Identifier typeId = idOf(c.type());
                if (typeId == null) {
                    // A component type an item carries and the registry does
                    // not name cannot be written to the wire either, so this
                    // is a finding and not a skip.
                    refusals.add(refusal(itemId.toString(), "<unregistered>",
                        "component type is not in BuiltInRegistries.DATA_COMPONENT_TYPE"));
                    continue;
                }
                if (c.type().isTransient()) {
                    // `isTransient` is `codec() == null`. Vanilla never
                    // persists these and neither can we; recorded rather than
                    // dropped so the count of what is missing is a number.
                    refusals.add(refusal(itemId.toString(), typeId.toString(), "transient"));
                    continue;
                }
                DataResult<JsonElement> encoded = c.encodeValue(ops);
                JsonElement value = encoded.result().orElse(null);
                if (value == null) {
                    refusals.add(refusal(itemId.toString(), typeId.toString(),
                        encoded.error().map(DataResult.Error::message).orElse("encode failed")));
                    continue;
                }
                comps.add(typeId.toString(), value);
                componentsWritten++;
            }

            resolveToolBlockSets(item, comps, blockSets);

            JsonObject row = new JsonObject();
            row.addProperty("item", itemId.toString());
            row.addProperty("id", BuiltInRegistries.ITEM.getId(item));
            // The behaviour half. Components say what an item *is*; the class
            // says what its `use`, `useOn`, `finishUsingItem`, `hurtEnemy`,
            // `mineBlock` and `inventoryTick` do, and there is no table
            // anywhere in the registry report that names it. It is carried
            // here rather than in a second dumper because it is one field off
            // the same walk, and a second walk is a second thing that can
            // disagree about which items exist.
            //
            // It is what orders the transcription: `BlockItem` covers 1,068
            // items and `SpawnEggItem` 88, so a port that starts with the
            // second is a port that starts with 5.7% of the registry.
            row.addProperty("class", item.getClass().getName());
            row.add("components", comps);
            table.add(row);
        }

        JsonObject doc = new JsonObject();
        doc.addProperty("items", items);
        doc.addProperty("components", componentsWritten);
        doc.addProperty("component_types", BuiltInRegistries.DATA_COMPONENT_TYPE.size());
        doc.add("refusals", refusals);
        doc.add("equipmentBreakEvents", breakEvents);
        // **The wire id of every data component type.**
        //
        // `DataComponentPatch`'s stream codec writes each key through
        // `DataComponentType.STREAM_CODEC`, which is the registry id as a
        // varint — so a patch cannot be written at all without this map, and
        // the ids are not derivable from the sorted name list the table
        // already carries: the registry's own order is the registration
        // order, not alphabetical. All 111, not just the 59 some item
        // carries by default, because a patch names components no prototype
        // has (`minecraft:custom_name` is the obvious one).
        JsonObject componentTypeIds = new JsonObject();
        for (net.minecraft.core.component.DataComponentType<?> type
                : BuiltInRegistries.DATA_COMPONENT_TYPE) {
            componentTypeIds.addProperty(
                BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(type).toString(),
                BuiltInRegistries.DATA_COMPONENT_TYPE.getId(type));
        }
        doc.add("componentTypeIds", componentTypeIds);
        doc.add("tool_block_sets", blockSets);
        doc.add("table", table);
        // Written to a file named on the command line rather than to
        // stdout. `Bootstrap.bootStrap()` puts log4j in front of `System.out`
        // and re-emits every line with a `[HH:MM:SS] [main/INFO]: [STDOUT]: `
        // prefix; a table that has to be recovered from that by `sed` is a
        // table one stray log line can corrupt into something a JSON parser
        // still accepts.
        if (args.length != 1) {
            throw new IllegalArgumentException("usage: ItemPropsGolden <out.json>");
        }
        try {
            java.nio.file.Files.writeString(java.nio.file.Path.of(args[0]), new Gson().toJson(doc));
        } catch (java.io.IOException e) {
            throw new RuntimeException("could not write " + args[0], e);
        }
        System.out.println("WROTE\t" + items + "\t" + componentsWritten + "\t" + refusals.size());
    }

    /// Resolve each `minecraft:tool` rule's `HolderSet` to the blocks in it.
    ///
    /// # Why this is here and not in a second dumper
    ///
    /// `Tool.getMiningSpeed(state)` and `Tool.isCorrectForDrops(state)` are
    /// two four-line loops over `rules`, and everything they need is in the
    /// encoded component **except** what `BlockState.is(HolderSet)` answers.
    /// The rule's `blocks` field encodes as `"#minecraft:mineable/pickaxe"`
    /// or `"minecraft:cobweb"` — a name, not a membership — so a consumer of
    /// the component alone would have to resolve block tags, and nothing in
    /// this tree reads a block tag (see `crate::itemuse`'s header for what
    /// that cost `FlintAndSteelItem`). Resolving them here turns the two
    /// loops into pure transcription with no tag machinery behind them.
    ///
    /// # Keyed by the encoded form, and why that is sound rather than lucky
    ///
    /// The key is the JSON the rule's own `blocks` field encoded to, taken
    /// out of `comps` at the same index. `List` encodes in order, so
    /// `tool.rules().get(i)` is `comps[...]["rules"][i]`, and the key is
    /// therefore the exact string a consumer reading the component will hold.
    /// There is no name matching and no second encoding that could disagree.
    ///
    /// A set is resolved once. Across the whole registry there are far fewer
    /// distinct sets than tools, which is the reason this is a side table
    /// rather than a block list inlined into every rule of every item.
    private static void resolveToolBlockSets(Item item, JsonObject comps, JsonObject out) {
        Tool tool = item.components().get(DataComponents.TOOL);
        if (tool == null) {
            return;
        }
        JsonElement encoded = comps.get("minecraft:tool");
        if (encoded == null || !encoded.isJsonObject()) {
            // The component is present and did not encode — already recorded
            // as a refusal by the caller. Nothing to key against.
            return;
        }
        JsonArray rules = encoded.getAsJsonObject().getAsJsonArray("rules");
        if (rules == null || rules.size() != tool.rules().size()) {
            throw new IllegalStateException(
                "minecraft:tool encoded " + (rules == null ? "no" : rules.size())
                + " rules against " + tool.rules().size() + " in the component — the index "
                + "correspondence this table is keyed on does not hold");
        }
        for (int i = 0; i < rules.size(); i++) {
            String key = rules.get(i).getAsJsonObject().get("blocks").toString();
            if (out.has(key)) {
                continue;
            }
            JsonArray blocks = new JsonArray();
            for (Holder<Block> holder : tool.rules().get(i).blocks()) {
                Identifier id = BuiltInRegistries.BLOCK.getKey(holder.value());
                if (id != null) {
                    blocks.add(id.toString());
                }
            }
            out.add(key, blocks);
        }
    }

    /// Bind `#minecraft:...` **block** tags into `BuiltInRegistries.BLOCK`.
    ///
    /// Without this every `HolderSet` a tool rule names resolves to **zero
    /// blocks** and no exception is thrown — the same "an unbound tag does
    /// not throw, it says no" fault `UseGolden.bindItemTags` exists for.
    /// Measured on this dumper before it existed: 17 sets resolved, 16 of
    /// them empty, and the one non-empty was `minecraft:cobweb`, an inline
    /// block id rather than a tag. A table written in that state would say
    /// every tool mines everything at speed 1.0 and drops nothing, which is
    /// a plausible-looking answer and completely wrong.
    ///
    /// **Recursive, and that is not a refinement.** Block tags nest in
    /// directories — `tags/block/mineable/pickaxe.json` is
    /// `#minecraft:mineable/pickaxe`, and it is the single most important tag
    /// here — so a `Files.list` of the top directory misses the four
    /// `mineable/*` tags entirely and leaves exactly the tools' main rule
    /// empty. `SurfaceChunkGolden.bindBlockTags` is flat and has never needed
    /// them; this is not that function moved.
    /// Package-private rather than private since `ItemSurviveGolden` needs
    /// the same recursive binding and a second copy of it is a second thing
    /// that can go stale — which is exactly how the flat
    /// `SurfaceChunkGolden.bindBlockTags` came to be the wrong one to reach
    /// for. One binder, two dumpers.
    static void bindBlockTags() {
        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("set -Drustmc.data to the generated-data directory");
        }
        Path dir = Path.of(data, "data", "minecraft", "tags", "block");
        Map<String, List<String>> raw = new HashMap<>();
        try (var files = Files.walk(dir)) {
            for (Path file : files.toList()) {
                String name = file.getFileName().toString();
                if (!name.endsWith(".json")) {
                    continue;
                }
                String id = dir.relativize(file).toString();
                id = "minecraft:" + id.substring(0, id.length() - 5);
                JsonObject json =
                    JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                List<String> entries = new ArrayList<>();
                for (JsonElement element : json.getAsJsonArray("values")) {
                    entries.add(element.isJsonObject()
                        ? element.getAsJsonObject().get("id").getAsString()
                        : element.getAsString());
                }
                raw.put(id, entries);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read block tags from " + dir, e);
        }

        Map<TagKey<Block>, List<Holder<Block>>> bound = new HashMap<>();
        for (String name : raw.keySet()) {
            Set<String> flat = new LinkedHashSet<>();
            flatten(name, raw, flat, new LinkedHashSet<>());
            List<Holder<Block>> holders = new ArrayList<>();
            for (String block : flat) {
                BuiltInRegistries.BLOCK.get(Identifier.parse(block)).ifPresent(holders::add);
            }
            bound.put(TagKey.create(Registries.BLOCK, Identifier.parse(name)), holders);
        }
        BuiltInRegistries.BLOCK
            .prepareTagReload(new TagLoader.LoadResult<>(Registries.BLOCK, bound))
            .apply();
    }

    /// The same binding for the fluid registry.
    ///
    /// `SurfaceChunkGolden.bindFluidTags` does this already and is not
    /// reachable: it sits inside `net.minecraft.world.level.levelgen` and the
    /// dumpers that need this one are in the default package, which cannot
    /// import from a named one. So the copy is a **language boundary**, not a
    /// duplicate someone forgot to fold — and it is here, next to the block
    /// binder, rather than in each dumper, so there is one copy on this side
    /// of the boundary instead of one per caller.
    ///
    /// Flat, and correctly so: `tags/fluid/` has no subdirectories. The block
    /// binder above is recursive because `tags/block/` does.
    static void bindFluidTags() {
        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("set -Drustmc.data to the generated-data directory");
        }
        Path dir = Path.of(data, "data", "minecraft", "tags", "fluid");
        Map<String, List<String>> raw = new HashMap<>();
        try (var files = Files.walk(dir)) {
            for (Path file : files.toList()) {
                if (!file.getFileName().toString().endsWith(".json")) {
                    continue;
                }
                String id = dir.relativize(file).toString();
                id = "minecraft:" + id.substring(0, id.length() - 5);
                JsonObject json =
                    JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                List<String> entries = new ArrayList<>();
                for (JsonElement element : json.getAsJsonArray("values")) {
                    entries.add(element.isJsonObject()
                        ? element.getAsJsonObject().get("id").getAsString()
                        : element.getAsString());
                }
                raw.put(id, entries);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read fluid tags from " + dir, e);
        }

        Map<TagKey<net.minecraft.world.level.material.Fluid>,
            List<Holder<net.minecraft.world.level.material.Fluid>>> bound = new HashMap<>();
        for (String name : raw.keySet()) {
            Set<String> flat = new LinkedHashSet<>();
            flatten(name, raw, flat, new LinkedHashSet<>());
            List<Holder<net.minecraft.world.level.material.Fluid>> holders = new ArrayList<>();
            for (String fluid : flat) {
                BuiltInRegistries.FLUID.get(Identifier.parse(fluid)).ifPresent(holders::add);
            }
            bound.put(TagKey.create(Registries.FLUID, Identifier.parse(name)), holders);
        }
        BuiltInRegistries.FLUID
            .prepareTagReload(new TagLoader.LoadResult<>(Registries.FLUID, bound))
            .apply();
    }

    /// Flatten one tag's `#other` references. A missing tag and a cycle are
    /// both errors: an optional entry (`{"id": ..., "required": false}`) is
    /// read as its id above, and every id in the pinned jar's own datapack
    /// exists by construction.
    private static void flatten(
        String name, Map<String, List<String>> raw, Set<String> out, Set<String> visiting
    ) {
        if (!visiting.add(name)) {
            throw new IllegalStateException("block tag cycle at " + name);
        }
        List<String> entries = raw.get(name);
        if (entries == null) {
            throw new IllegalStateException("block tag " + name + " does not exist");
        }
        for (String entry : entries) {
            if (entry.startsWith("#")) {
                flatten(entry.substring(1), raw, out, visiting);
            } else {
                out.add(entry);
            }
        }
        visiting.remove(name);
    }

    private static Identifier idOf(DataComponentType<?> type) {
        return BuiltInRegistries.DATA_COMPONENT_TYPE.getKey(type);
    }

    private static JsonObject refusal(String item, String component, String why) {
        JsonObject o = new JsonObject();
        o.add("item", new JsonPrimitive(item));
        o.add("component", new JsonPrimitive(component));
        o.add("why", new JsonPrimitive(why));
        return o;
    }

    /// Bind every registry's data components, so `Item.components()` answers.
    ///
    /// Transcribed from `UseGolden.bindItemComponents` rather than shared,
    /// because that one lives in `net.minecraft.world.level.levelgen` to
    /// reach package-private level internals this dumper wants nothing to do
    /// with. The two calls and the assertion are the load-bearing part and
    /// they are identical; if they ever diverge, the two tables would be
    /// describing two different bindings of the same registry, which is
    /// exactly the failure the assertion below exists to make loud.
    private static void bindItemComponents(HolderLookup.Provider lookup) {
        int applied = 0;
        for (DataComponentInitializers.PendingComponents<?> pending
                : BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(lookup)) {
            pending.apply();
            applied++;
        }
        if (applied == 0) {
            throw new IllegalStateException(
                "DATA_COMPONENT_INITIALIZERS produced nothing to apply — every item's "
                + "components() would throw `Components not bound yet`, and this table "
                + "would be a stack trace rather than a short table");
        }
        // The positive control, same one UseGolden uses: the plainest item in
        // the game must mint a stack. If it cannot, nothing below is reading
        // vanilla's components.
        new ItemStack(Items.BEDROCK, 1);
    }
}
