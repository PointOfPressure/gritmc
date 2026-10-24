// BonemealGolden — who `BoneMealItem.useOn` can grow, and by how much.
//
// # The three questions and why they are one table
//
// `BoneMealItem.useOn` -> `growCrop` asks a block three things in order
// (offsets 27-80): `isValidBonemealTarget`, `isBonemealSuccess`,
// `performBonemeal`. The first is a pure function of the state and its
// neighbourhood and extracts; the second draws RNG; the third mutates the
// world. So this table answers the first exactly, and for the third records
// the **parameters** of the one family whose write is a state change rather
// than a feature placement — the crops — so a port writes vanilla's own age
// rather than an assumed one.
//
// # The census is the point of the file
//
// `BonemealableBlock` is an interface with 45 implementing classes over 63
// blocks, and nothing in this repo has ever said which. A port that grows
// wheat and silently does nothing for the other 58 blocks looks identical, in
// every trace, to one that has not been written — so the count is extracted
// and asserted rather than left to be inferred from an absence.
//
// # What is deliberately NOT here
//
// `isBonemealSuccess` and `performBonemeal` for the 58 non-crop blocks. Both
// draw from `Level.getRandom()` and most `performBonemeal`s place a
// **configured feature** (a sapling grows a tree, nylium grows a nether
// forest patch), which is worldgen and not an item rule. Recording a
// probability here would be recording this dumper's sampling, not vanilla's
// rule. The declaring class of each is recorded instead, so the port refuses
// by name.
//
// Converted to JSON by `ci/bonemeal-golden.sh`.

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.LevelReader;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.BonemealableBlock;
import net.minecraft.world.level.block.CropBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.IntegerProperty;

public final class BonemealGolden {

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        ItemPropsGolden.bindBlockTags();
        ItemPropsGolden.bindFluidTags();
        assertTagsAreBound();

        List<String> rows = new ArrayList<>();
        Map<String, Integer> census = new TreeMap<>();
        Map<String, Integer> refusals = new TreeMap<>();
        int bonemealable = 0;
        int crops = 0;
        int states = 0;
        for (Block block : BuiltInRegistries.BLOCK) {
            if (!(block instanceof BonemealableBlock target)) {
                continue;
            }
            bonemealable++;
            String name = BuiltInRegistries.BLOCK.getKey(block).toString();
            census.merge(block.getClass().getName(), 1, Integer::sum);

            String crop = "null";
            if (block instanceof CropBlock cropBlock) {
                crops++;
                // `getMaxAge` and `getAgeProperty` are both overridden by
                // subclasses — beetroot is 3 and torchflower 2 — so neither
                // is a constant a port may assume. `getBonemealAgeIncrease`
                // is a third override and the one that changes the RNG:
                // `CropBlock`'s is `Mth.nextInt(random, 2, 5)`,
                // `TorchflowerCropBlock`'s is `iconst_1` and draws nothing at
                // all, and `BeetrootBlock`'s is `super / 3`.
                crop = "{\"maxAge\": " + cropBlock.getMaxAge()
                    + ", \"ageProperty\": " + quote(ageProperty(cropBlock).getName())
                    + ", \"ageIncrease\": "
                    + quote(declaringClassOf(block, "getBonemealAgeIncrease"))
                    + "}";
            }

            List<String> perState = new ArrayList<>();
            for (BlockState state : block.getStateDefinition().getPossibleStates()) {
                states++;
                perState.add("{\"state\": " + quote(BlockStateParser.serialize(state))
                    + ", \"valid\": " + validity(target, state, refusals) + "}");
            }
            rows.add("{\"block\": " + quote(name)
                + ", \"class\": " + quote(block.getClass().getName())
                + ", \"crop\": " + crop
                + ", \"isValidBonemealTarget\": "
                + quote(declaringClassOf(block, "isValidBonemealTarget"))
                + ", \"isBonemealSuccess\": "
                + quote(declaringClassOf(block, "isBonemealSuccess"))
                + ", \"performBonemeal\": "
                + quote(declaringClassOf(block, "performBonemeal"))
                + ", \"states\": [" + String.join(", ", perState) + "]}");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"bonemeal\",\n");
        // `BoneMealItem.useOn` offsets 55-62 and 132-140: the same event with
        // the same data, at the clicked position on the crop path and at the
        // relative position on the water-plant one. Carried here rather than
        // written into the port as a literal, so a version that renumbers it
        // moves the table.
        // See the reader: the light this sweep was taken under, named
        // because four of the 721 states depend on it and a table whose
        // conditions are not stated is a table nobody can re-derive.
        out.append("  \"rawBrightness\": 15,\n");
        out.append("  \"levelEvent\": 1505,\n");
        out.append("  \"levelEventData\": 15,\n");
        // `growCrop` offset 86: `stack.shrink(1)` — inside the success
        // branch, so a refused bone meal costs nothing.
        out.append("  \"shrink\": 1,\n");
        // `CropBlock.growCrops` offset 28: `setBlock(pos, state, 2)`, which
        // is UPDATE_CLIENTS alone — no neighbour update and no shape cascade
        // for a crop that grows.
        out.append("  \"setBlockFlags\": 2,\n");
        out.append("  \"bonemealable\": ").append(bonemealable).append(",\n");
        out.append("  \"crops\": ").append(crops).append(",\n");
        out.append("  \"states\": ").append(states).append(",\n");
        out.append("  \"census\": {\n");
        List<String> censusRows = new ArrayList<>();
        for (Map.Entry<String, Integer> e : census.entrySet()) {
            censusRows.add("    " + quote(e.getKey()) + ": " + e.getValue());
        }
        out.append(String.join(",\n", censusRows)).append("\n  },\n");
        out.append("  \"refusals\": {\n");
        List<String> refusalRows = new ArrayList<>();
        for (Map.Entry<String, Integer> e : refusals.entrySet()) {
            refusalRows.add("    " + quote(e.getKey()) + ": " + e.getValue());
        }
        out.append(String.join(",\n", refusalRows)).append("\n  },\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// `isValidBonemealTarget` in a world of nothing but this state.
    ///
    /// `null` where the rule read something the harness cannot answer — a
    /// `ServerLevel` method, a block entity — recorded rather than guessed,
    /// so a port is never asked to reproduce this dumper's fallback.
    private static String validity(
        BonemealableBlock target, BlockState state, Map<String, Integer> refusals
    ) {
        Map<BlockPos, BlockState> world = new LinkedHashMap<>();
        world.put(BlockPos.ZERO, state);
        try {
            return Boolean.toString(
                target.isValidBonemealTarget(reader(world), BlockPos.ZERO, state));
        } catch (RuntimeException | StackOverflowError e) {
            refusals.merge(e.getClass().getSimpleName()
                + ": " + (e.getMessage() == null ? "?" : e.getMessage()), 1, Integer::sum);
            return "null";
        }
    }

    private static IntegerProperty ageProperty(CropBlock crop) {
        try {
            java.lang.reflect.Method m = CropBlock.class.getDeclaredMethod("getAgeProperty");
            m.setAccessible(true);
            return (IntegerProperty) m.invoke(crop);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot read getAgeProperty", e);
        }
    }

    /// Which class declares a method of this name, by name and arity — the
    /// same reflection `ItemSurviveGolden` uses and for the same reason: the
    /// answer is a fact about the jar, and a hand-list of 45 classes goes
    /// stale with no symptom.
    private static String declaringClassOf(Block block, String method) {
        Class<?> c = block.getClass();
        while (c != null) {
            for (java.lang.reflect.Method candidate : c.getDeclaredMethods()) {
                if (candidate.getName().equals(method)) {
                    return c.getName();
                }
            }
            c = c.getSuperclass();
        }
        for (Class<?> i : allInterfaces(block.getClass())) {
            for (java.lang.reflect.Method candidate : i.getDeclaredMethods()) {
                if (candidate.getName().equals(method) && !candidate.isSynthetic()) {
                    return i.getName();
                }
            }
        }
        throw new IllegalStateException("no " + method + " anywhere above " + block.getClass());
    }

    private static List<Class<?>> allInterfaces(Class<?> c) {
        List<Class<?>> out = new ArrayList<>();
        while (c != null) {
            for (Class<?> i : c.getInterfaces()) {
                out.add(i);
                out.addAll(allInterfaces(i));
            }
            c = c.getSuperclass();
        }
        return out;
    }

    private static void assertTagsAreBound() {
        if (!Blocks.GRASS_BLOCK.defaultBlockState()
                .is(net.minecraft.tags.BlockTags.SUPPORTS_VEGETATION)) {
            throw new IllegalStateException(
                "block tags did not bind: grass_block is not in #supports_vegetation");
        }
    }

    private static LevelReader reader(Map<BlockPos, BlockState> world) {
        return (LevelReader) java.lang.reflect.Proxy.newProxyInstance(
            BonemealGolden.class.getClassLoader(),
            new Class<?>[]{LevelReader.class},
            (proxy, method, args) -> {
                String name = method.getName();
                if (name.equals("getBlockState") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState());
                }
                if (name.equals("getFluidState") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState())
                        .getFluidState();
                }
                if (name.equals("isEmptyBlock") && method.getParameterCount() == 1) {
                    return world.getOrDefault(args[0], Blocks.AIR.defaultBlockState()).isAir();
                }
                if (name.equals("getBlockEntity")) {
                    return null;
                }
                // Two `LevelReader` defaults a proxy intercepts along with
                // the abstract ones, and which the first run of this dumper
                // refused 102 of 721 states over.
                //
                // `isInsideBuildHeight` is `y >= getMinY() && y <= getMaxY()`
                // and the two are already answered above, so this is the
                // default reassembled rather than a value chosen here.
                if (name.equals("isInsideBuildHeight") && method.getParameterCount() == 1) {
                    // **It takes a `BlockPos` in 26.2, not an int.** The
                    // first version of this arm cast to `Integer` and threw
                    // `ClassCastException` on all 102 states — which the
                    // refusal counter caught, and which is the argument for
                    // counting refusals by message rather than by a boolean.
                    int y = args[0] instanceof BlockPos pos
                        ? pos.getY()
                        : (Integer) args[0];
                    return y >= -64 && y < 320;
                }
                // `getRawBrightness` has no reading to take in a world of one
                // block and no light engine. 15 — a daylit surface — is the
                // value, it is stated in the header as `rawBrightness`, and
                // the four states it decides are `GrassBlock`'s, whose
                // `isValidBonemealTarget` is the block above being light
                // enough to grow into.
                if (name.equals("getRawBrightness") && method.getParameterCount() == 2) {
                    return 15;
                }
                if (name.equals("getMinY")) {
                    return -64;
                }
                if (name.equals("getHeight") && method.getParameterCount() == 0) {
                    return 384;
                }
                if (name.equals("isClientSide")) {
                    return false;
                }
                if (name.equals("toString")) {
                    return "bonemealWorld" + world;
                }
                if (name.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (name.equals("equals")) {
                    return proxy == args[0];
                }
                throw new UnsupportedOperationException(
                    "isValidBonemealTarget read LevelReader." + name
                        + "/" + method.getParameterCount());
            });
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private BonemealGolden() {}
}
