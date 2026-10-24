// BlockEntityGolden — which blocks make which block entity, and how big it is.
//
// # The question this answers, and why it cannot be answered from a report
//
// `crate::blockentity::for_state` is `EntityBlock.newBlockEntity` — a map from
// a block to the block-entity type it creates. Nothing in the generator's own
// reports carries it. `blocks.json` is states and properties; `registries.json`
// names the forty-nine types and says nothing about which blocks they attach
// to. The map lives in one place only: the `Block...` varargs handed to
// `BlockEntityTypes.register`, which end up in `BlockEntityType.validBlocks`.
//
// Transcribing it by reading that static initialiser is exactly what was done
// (`javap -p -c -constants`, ~800 lines of `getstatic Blocks.*`), and a
// transcription checked against nothing is a table this server compares only to
// itself. Four of the forty-nine registrations are not literal block lists at
// all — `CHEST` is `Util.copyAndAdd(Blocks.COPPER_CHEST.asList(), Blocks.CHEST)`
// and `BANNER` is `Util.join` of two `ColorCollection.asList()`s — so the
// transcription has to *expand a collection by hand*, in the right order, from
// a class the bytecode never enumerates. That is precisely where a hand table
// goes wrong quietly: a missing `waxed_exposed_copper_chest` is a chest that
// keeps nothing, and no gate in this tree would have noticed.
//
// So this dumps the map from the bootstrapped registry itself.
//
// # What is dumped, and why each one
//
//   type            the `minecraft:block_entity_type` registry name.
//   protocolId      `BuiltInRegistries.BLOCK_ENTITY_TYPE.getId` — the number
//                   `ClientboundBlockEntityDataPacket` carries. Dumped beside
//                   the name because they are two different id spaces over one
//                   object and this file is where they can be compared.
//   blocks          every block whose *default* state satisfies
//                   `BlockEntityType.isValid`, in `BuiltInRegistries.BLOCK`
//                   order. `isValid` is `validBlocks.contains(state.getBlock())`
//                   — a property of the block, not of the state — so the
//                   default state is a total probe and not a sample.
//   containerSize   `Container.getContainerSize()` on a freshly created
//                   instance, or -1 when the type does not implement
//                   `Container` at all. **-1 and 0 are different answers**:
//                   a campfire is not a container and a lectern's own
//                   container is one slot, and a single number cannot say
//                   both. `crate::blockentity::Kind::container_size` folds
//                   -1 to 0 at its own boundary and says so.
//   itemSlots       the length of the type's `NonNullList<ItemStack>`, found
//                   by reflection over the instance's fields, or -1 when it
//                   holds none. This is what `ContainerHelper.saveAllItems`
//                   iterates, and it is **not** `containerSize`:
//                   `CampfireBlockEntity` has four stacks and no container.
//   saved           the keys a *freshly constructed* block entity's
//                   `saveWithFullMetadata` writes, each as
//                   `name:tagTypeId:value` — `Tag.getId()` for the type and
//                   the tag's own `toString()` for the value. This is the
//                   shape `crate::blockentity::BlockEntity::to_nbt` has to
//                   produce, taken from vanilla rather than transcribed:
//                   the key names, the **tag widths** (`BrewTime` is a short
//                   and `Fuel` a byte one line apart) and the constructor's
//                   starting values (a hopper's `TransferCooldown` is -1,
//                   not 0) are three different things a round-trip test
//                   cannot see.
//
//                   `TagValueOutput.createWithoutContext` is used rather than
//                   a registry-backed one: an empty block entity stores
//                   nothing through a codec that needs registry ops, and a
//                   type whose save throws is reported as `saved: null`
//                   rather than as an empty list, so an absent answer cannot
//                   be read as "writes no keys".
//   updatePacket    whether any class between the instance's own and
//                   `BlockEntity` **declares** `getUpdatePacket()`. Twenty-nine
//                   of the forty-nine inherit `BlockEntity`'s
//                   `aconst_null; areturn`, and a server that broadcast every
//                   block entity would be wrong about more types than it was
//                   right about — `ci/checks/gameplay/761` is the check that
//                   says so and this is the census behind it.
//
//                   **This column read `getUpdatePacket() != null` for one
//                   revision of this file and was false for all forty-nine.**
//                   `ClientboundBlockEntityDataPacket.create` goes through
//                   `getUpdateTag(this.level.registryAccess())`, and nothing
//                   here has a level, so every call threw and every answer was
//                   the same one. A column with one value cannot distinguish
//                   anything, which is this project's central defect class
//                   arriving in a brand-new instrument on its first run. The
//                   declaration is the fact `broadcastBlockEntity`'s `ifnull`
//                   actually turns on, and it needs no level.
//
//   tickers         the names of every **declared** static method on the
//                   instance's class chain, up to but excluding `BlockEntity`,
//                   whose signature is `(Level, BlockPos, BlockState, T)V` —
//                   the shape `BlockEntityTicker.tick` is bound to. Sorted,
//                   so the column is a set rather than a reflection order.
//
//                   This is a **declaration** walk for the same reason
//                   `updatePacket` is one: the thing a consumer wants is
//                   `EntityBlock.getTicker`'s answer, and every one of the
//                   thirty-two block classes that overrides it reaches that
//                   answer through `Level.isClientSide()`, so asking it needs
//                   a level and there is none here. What the declaration walk
//                   CANNOT tell you is which side a ticker is handed to: it
//                   reports `clientTick` and `serverTick` alike. That residue
//                   is closed by reading each block's `getTicker` branch and
//                   is recorded at the line in `crate::behaviour`, not here —
//                   a column that guessed would be a column with one value by
//                   another route.
//
//                   Twelve `BlockEntityType`s in the pinned jar declare no
//                   such method anywhere on their chain, and a server that
//                   ticked them would be running a pass vanilla does not.
//   blockTicker     whether the class of the first block valid for this type
//                   declares `getTicker` anywhere up to `Block`. A type whose
//                   entity declares a ticker method but whose block never
//                   hands one out never ticks, and the two facts are
//                   independent.
//
// A type whose `create` throws is dumped with its blocks and `"created":
// false`, rather than omitted. An omission would read as "no such type" and
// is the shape of a hole that passes; a false is a fact the consumer can
// decide about.
//
// Mojang-derived data under the usual rule: gitignored, regenerable from the
// jar in one command, never committed.
//
// Converted to JSON by `ci/block-entity-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.lang.reflect.Modifier;
import java.util.TreeSet;
import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.Tag;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.level.storage.TagValueOutput;
import net.minecraft.core.BlockPos;
import net.minecraft.core.NonNullList;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.Container;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.entity.BlockEntity;
import net.minecraft.world.level.block.entity.BlockEntityType;
import net.minecraft.world.level.block.state.BlockState;

public final class BlockEntityGolden {

    private static String esc(String s) {
        return s.replace("\\", "\\\\").replace("\"", "\\\"");
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        List<String> rows = new ArrayList<>();
        int totalBlocks = 0;
        int withUpdatePacket = 0;

        // Registry iteration order is registration order for a
        // `MappedRegistry`, which is protocol-id order, which is the order
        // `registries.json` prints. Nothing downstream depends on it — the
        // consumer compares sets — but a stable order is what lets
        // `ci/block-entity-golden.sh` diff two runs and refuse a table whose
        // generator iterates a HashMap. That check has caught one dumper here
        // already; see `ci/block-props-golden.sh`'s header.
        for (BlockEntityType<?> type : BuiltInRegistries.BLOCK_ENTITY_TYPE) {
            String name = BuiltInRegistries.BLOCK_ENTITY_TYPE.getKey(type).toString();
            int protocolId = BuiltInRegistries.BLOCK_ENTITY_TYPE.getId(type);

            List<String> blocks = new ArrayList<>();
            Block first = null;
            for (Block block : BuiltInRegistries.BLOCK) {
                BlockState state = block.defaultBlockState();
                if (type.isValid(state)) {
                    blocks.add("\"" + esc(BuiltInRegistries.BLOCK.getKey(block).toString()) + "\"");
                    if (first == null) {
                        first = block;
                    }
                }
            }
            totalBlocks += blocks.size();

            boolean created = false;
            int containerSize = -1;
            int itemSlots = -1;
            String updatePacket = "unknown";
            String tickers = null;
            String saved = null;
            if (first != null) {
                try {
                    BlockEntity be = type.create(BlockPos.ZERO, first.defaultBlockState());
                    if (be != null) {
                        created = true;
                        if (be instanceof Container c) {
                            containerSize = c.getContainerSize();
                        }
                        itemSlots = itemSlots(be);
                        saved = savedKeys(be);
                        updatePacket = declaresUpdatePacket(be.getClass()) ? "yes" : "no";
                        tickers = tickerMethods(be.getClass());
                        if (updatePacket.equals("yes")) {
                            withUpdatePacket++;
                        }
                    }
                } catch (Throwable t) {
                    // Deliberately swallowed and reported as `created: false`.
                    // A type whose constructor wants a level is a fact about
                    // the type; a dumper that died on it would be a fact about
                    // the dumper, and the consumer could not tell them apart.
                }
            }

            rows.add("    {\"type\": \"" + esc(name) + "\", \"protocolId\": " + protocolId
                    + ", \"created\": " + created
                    + ", \"containerSize\": " + containerSize
                    + ", \"itemSlots\": " + itemSlots
                    + ", \"updatePacket\": \"" + updatePacket + "\""
                    + ", \"tickers\": " + (tickers == null ? "null" : "[" + tickers + "]")
                    + ", \"blockTicker\": " + (first == null ? "null"
                            : Boolean.toString(declaresGetTicker(first.getClass())))
                    + ", \"saved\": " + (saved == null ? "null" : "[" + saved + "]")
                    + ", \"blocks\": [" + String.join(", ", blocks) + "]}");
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"rng\": \"block_entity\",\n");
        out.append("  \"types\": ").append(rows.size()).append(",\n");
        out.append("  \"blocks\": ").append(totalBlocks).append(",\n");
        out.append("  \"withUpdatePacket\": ").append(withUpdatePacket).append(",\n");
        out.append("  \"table\": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }

    /// The keys a freshly constructed block entity saves, as
    /// `"name:tagTypeId:value"`, sorted by name.
    ///
    /// Sorted because `CompoundTag.keySet()` is a `HashMap` view and its order
    /// is not something the JVM promises across runs — the two-run `cmp` in
    /// `ci/block-entity-golden.sh` would otherwise fail on the dumper rather
    /// than on a change. The consumer compares sets, so nothing depends on it
    /// beyond reproducibility.
    private static String savedKeys(BlockEntity be) {
        try {
            TagValueOutput out = TagValueOutput.createWithoutContext(ProblemReporter.DISCARDING);
            be.saveWithFullMetadata(out);
            CompoundTag tag = out.buildResult();
            List<String> keys = new ArrayList<>();
            for (String k : tag.keySet()) {
                Tag v = tag.get(k);
                keys.add("\"" + esc(k) + ":" + v.getId() + ":" + esc(v.toString()) + "\"");
            }
            java.util.Collections.sort(keys);
            return String.join(", ", keys);
        } catch (Throwable t) {
            return null;
        }
    }

    /// Whether any class from `c` up to but excluding `BlockEntity` declares a
    /// no-argument `getUpdatePacket`.
    ///
    /// This is the predicate `ChunkHolder.broadcastBlockEntity`'s `ifnull`
    /// turns on, expressed without a level. A subclass that inherits it from a
    /// modelled superclass answers `true` — `HangingSignBlockEntity` gets
    /// `SignBlockEntity`'s — which is correct: what matters is whether the
    /// method that runs is `BlockEntity`'s `aconst_null; areturn` or somebody
    /// else's.
    private static boolean declaresUpdatePacket(Class<?> c) {
        for (; c != null && c != BlockEntity.class; c = c.getSuperclass()) {
            for (var m : c.getDeclaredMethods()) {
                if (m.getName().equals("getUpdatePacket") && m.getParameterCount() == 0) {
                    return true;
                }
            }
        }
        return false;
    }

    /// Every declared static `(Level, BlockPos, BlockState, T)V` on the class
    /// chain from `c` up to but excluding `BlockEntity`, sorted, as JSON
    /// strings.
    ///
    /// The signature is the whole test. `BlockEntityTicker.tick` is a
    /// functional interface of exactly that shape, so a method matching it is
    /// a candidate ticker and a method not matching it cannot be bound to one
    /// however it is named — which is why this asks the descriptor rather than
    /// the name. `CampfireBlockEntity.cookTick`, `HopperBlockEntity.
    /// pushItemsTick` and `SkullBlockEntity.animation` are all found by it and
    /// none of them is called `tick`.
    ///
    /// A `TreeSet` because two classes on one chain can declare the same name
    /// (an override that keeps the signature) and because reflection order is
    /// not stable between JVM runs — the same hazard `ci/jukebox-song-golden.sh`
    /// records for a registry index, one level down.
    private static String tickerMethods(Class<?> c) {
        TreeSet<String> found = new TreeSet<>();
        for (; c != null && c != BlockEntity.class; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                if (!Modifier.isStatic(m.getModifiers())) {
                    continue;
                }
                if (m.isSynthetic() || m.getReturnType() != void.class) {
                    continue;
                }
                Class<?>[] p = m.getParameterTypes();
                // Four is the arity of `BlockEntityTicker.tick`; five is what
                // `CampfireBlockEntity.cookTick` has, because
                // `CampfireBlock.getTicker` captures a `RecipeManager
                // .CachedCheck` into the lambda. Anything past the fourth is
                // a capture and not part of the interface.
                if (p.length < 4 || p.length > 5) {
                    continue;
                }
                // `Level.isAssignableFrom` and not `== Level.class`: five of
                // the tickers in the registry are declared on `ServerLevel`,
                // `AbstractFurnaceBlockEntity.serverTick` among them, and an
                // exact test reported the furnace as having no ticker at all.
                if (Level.class.isAssignableFrom(p[0])
                        && p[1] == BlockPos.class
                        && p[2] == BlockState.class
                        && BlockEntity.class.isAssignableFrom(p[3])) {
                    found.add("\"" + esc(m.getName()) + "\"");
                }
            }
        }
        return String.join(", ", found);
    }

    /// Whether any class from `c` up to and including `Block` declares
    /// `getTicker`.
    ///
    /// `EntityBlock.getTicker` has a default that returns null, and a block
    /// that does not override it hands out no ticker on either side however
    /// many ticker-shaped methods its entity declares. `ChestBlockEntity` and
    /// `EnderChestBlockEntity` each declare `lidAnimateTick` and their blocks
    /// DO override `getTicker` — with the server arm `aconst_null` — so this
    /// column does not separate those two; nothing here does, and the header
    /// says so rather than implying otherwise.
    private static boolean declaresGetTicker(Class<?> c) {
        for (; c != null; c = c.getSuperclass()) {
            for (Method m : c.getDeclaredMethods()) {
                if (m.getName().equals("getTicker")) {
                    return true;
                }
            }
            if (c == Block.class) {
                break;
            }
        }
        return false;
    }

    /// The length of the instance's own `NonNullList`, or -1.
    ///
    /// Reflection rather than a per-class switch because the field is private
    /// on twelve different classes with three different names (`items`,
    /// `inventory`, `stacks`), and a switch listing them would be the hand
    /// table this whole file exists to avoid. A class holding two such lists
    /// would make this ambiguous — none does in the pinned jar, and the
    /// consumer's cross-check against `containerSize` is what would catch it
    /// if one appeared.
    private static int itemSlots(BlockEntity be) {
        for (Class<?> c = be.getClass(); c != null && c != Object.class; c = c.getSuperclass()) {
            for (Field f : c.getDeclaredFields()) {
                if (!NonNullList.class.isAssignableFrom(f.getType())) {
                    continue;
                }
                try {
                    f.setAccessible(true);
                    Object v = f.get(be);
                    if (v instanceof NonNullList<?> list) {
                        return list.size();
                    }
                } catch (Throwable t) {
                    return -1;
                }
            }
        }
        return -1;
    }
}
