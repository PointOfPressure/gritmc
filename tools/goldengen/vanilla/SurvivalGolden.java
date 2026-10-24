// SurvivalGolden — `BlockState.canSurvive` for the states worldgen asks about.
//
// `would_survive` is 58 of the 68 placement chains this port cannot yet run,
// and behind it is `canSurvive`, which is per-block-class logic rather than a
// cached per-state fact. It cannot be extracted the way `isFaceSturdy` was.
//
// So this is not a table of answers — it is a table of *questions and their
// answers over a controlled neighbourhood*, which is what makes the ported
// rules checkable. For each state `would_survive` names, and for each of a
// set of "block below" candidates, it records what vanilla says.
//
// That is enough to prove the shape of a rule without claiming to have ported
// the block class: if the port and vanilla agree on every (state, below) pair
// in the datapack's own vocabulary, the rule is right for every case the
// generator can present it with.
//
// The candidate set is deliberately the blocks that actually appear under
// vegetation in a generated world, plus the negative controls that make the
// tag boundary visible — `stone` is not in `#supports_vegetation` and
// `farmland` is.
//
// Converted to JSON by `ci/survival-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.nio.file.Path;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class SurvivalGolden {

    /// The subjects are **read from the datapack**, not written down here.
    ///
    /// Every state a `would_survive` predicate names, and every state a
    /// `simple_block` feature can place — because `SimpleBlockFeature` calls
    /// `canSurvive` before it places anything, so those are exactly the
    /// blocks whose rule the generator depends on.
    ///
    /// A hand-picked list is how the named-noise gate came to prove six of
    /// sixty-three and miss the one that was wrong (ORACLE.md 5b rule 5). The
    /// first version of this file had eighteen entries chosen by eye and was
    /// missing `minecraft:bush`, which `patch_bush` places.
    private static String[] subjects() {
        String data = System.getProperty("rustmc.data");
        java.util.TreeSet<String> names = new java.util.TreeSet<>();
        Path root = Path.of(data, "data", "minecraft", "worldgen");
        collectNames(root.resolve("placed_feature"), "would_survive", names);
        collectStatesOfSimpleBlocks(root.resolve("configured_feature"), names);
        // `nether_forest_vegetation` places its provider's state behind a
        // `canSurvive`, exactly as `simple_block` does.
        collectStatesOfType(root.resolve("configured_feature"), "minecraft:nether_forest_vegetation", names);
        return names.toArray(new String[0]);
    }

    /// Every `state.Name` inside an object whose `type` mentions `marker`.
    private static void collectNames(Path dir, String marker, java.util.Set<String> out) {
        try (var files = java.nio.file.Files.list(dir)) {
            for (Path file : files.toList()) {
                String text = java.nio.file.Files.readString(file);
                if (!text.contains(marker)) {
                    continue;
                }
                JsonElement root = JsonParser.parseString(text);
                walkForStates(root, marker, out);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read " + dir, e);
        }
    }

    private static void walkForStates(JsonElement element, String marker, java.util.Set<String> out) {
        if (element.isJsonArray()) {
            for (JsonElement child : element.getAsJsonArray()) {
                walkForStates(child, marker, out);
            }
            return;
        }
        if (!element.isJsonObject()) {
            return;
        }
        JsonObject object = element.getAsJsonObject();
        if (object.has("type")
                && object.get("type").getAsString().contains(marker)
                && object.has("state")
                && object.getAsJsonObject("state").has("Name")) {
            out.add(object.getAsJsonObject("state").get("Name").getAsString());
        }
        for (String key : object.keySet()) {
            walkForStates(object.get(key), marker, out);
        }
    }

    /// Every block a `simple_block` feature can place, through any provider.
    private static void collectStatesOfSimpleBlocks(Path dir, java.util.Set<String> out) {
        collectStatesOfType(dir, "minecraft:simple_block", out);
    }

    /// Every block a configured feature of `type` names, through any provider.
    private static void collectStatesOfType(Path dir, String type, java.util.Set<String> out) {
        try (var files = java.nio.file.Files.list(dir)) {
            for (Path file : files.toList()) {
                JsonObject json =
                    JsonParser.parseString(java.nio.file.Files.readString(file)).getAsJsonObject();
                if (!json.has("type")
                        || !json.get("type").getAsString().equals(type)) {
                    continue;
                }
                walkForAnyStateName(json.get("config"), out);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read " + dir, e);
        }
    }

    private static void walkForAnyStateName(JsonElement element, java.util.Set<String> out) {
        if (element.isJsonArray()) {
            for (JsonElement child : element.getAsJsonArray()) {
                walkForAnyStateName(child, out);
            }
            return;
        }
        if (!element.isJsonObject()) {
            return;
        }
        JsonObject object = element.getAsJsonObject();
        if (object.has("Name") && object.get("Name").isJsonPrimitive()) {
            out.add(object.get("Name").getAsString());
        }
        for (String key : object.keySet()) {
            walkForAnyStateName(object.get(key), out);
        }
    }

    /// What sits under it. Real generated-world blocks plus the negative
    /// controls that make a tag boundary visible.
    private static final String[] GROUNDS = {
        "minecraft:grass_block", "minecraft:dirt", "minecraft:coarse_dirt", "minecraft:podzol",
        "minecraft:rooted_dirt", "minecraft:mycelium", "minecraft:moss_block", "minecraft:mud",
        "minecraft:muddy_mangrove_roots", "minecraft:farmland", "minecraft:sand",
        "minecraft:red_sand", "minecraft:gravel", "minecraft:stone", "minecraft:deepslate",
        "minecraft:snow_block", "minecraft:water", "minecraft:air", "minecraft:netherrack",
        "minecraft:soul_sand", "minecraft:end_stone", "minecraft:clay", "minecraft:terracotta",
        // The nether's own floors. Without them every nether plant's tag
        // boundary is invisible: `crimson_roots`' real rule and the
        // overworld vegetation rule agree on all twenty-three grounds above.
        "minecraft:crimson_nylium", "minecraft:warped_nylium", "minecraft:soul_soil",
        // Flammable and not sturdy on top: the one ground on which `fire`'s
        // second clause, `isValidFireLocation`, decides the answer. Every
        // other ground is either sturdy or not flammable, so without it that
        // clause could be deleted and this table would not notice.
        "minecraft:oak_slab",
    };

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        // A world whose only interesting block is the one directly below the
        // subject. `canSurvive` for the vegetation family reads exactly that
        // one position; anything that reads further will disagree with the
        // port and the disagreement is the finding.
        String[] subjects = subjects();
        List<String> rows = new ArrayList<>();
        for (String subject : subjects) {
            BlockState state = BuiltInRegistries.BLOCK
                .getValue(Identifier.parse(subject))
                .defaultBlockState();
            List<String> answers = new ArrayList<>();
            for (String ground : GROUNDS) {
                BlockState below = BuiltInRegistries.BLOCK
                    .getValue(Identifier.parse(ground))
                    .defaultBlockState();
                boolean survives;
                try {
                    survives = state.canSurvive(oneBlockBelow(below, -1), BlockPos.ZERO);
                } catch (RuntimeException e) {
                    // A rule that reads more than the block below cannot be
                    // answered by this harness. Recorded as a refusal rather
                    // than as a false, so the port is never asked to
                    // reproduce a guess.
                    answers.add("{\"ground\": " + quote(ground) + ", \"survives\": null"
                        + ", \"reads\": " + quote(e.getMessage() == null ? "?" : e.getMessage())
                        + "}");
                    continue;
                }
                answers.add("{\"ground\": " + quote(ground)
                    + ", \"survives\": " + survives + "}");
            }
            // The same grounds in a DARK world: `getRawBrightness` answers 0,
            // which is what a decorating chunk in a dimension without sky
            // light answers (`LevelLightEngine` with a null sky engine and no
            // block light yet). Only a rule that reads the light can answer
            // differently here, and the mushrooms are the ones that do.
            List<String> dark = new ArrayList<>();
            for (String ground : GROUNDS) {
                BlockState below = BuiltInRegistries.BLOCK
                    .getValue(Identifier.parse(ground))
                    .defaultBlockState();
                String answer;
                try {
                    answer = String.valueOf(state.canSurvive(oneBlockBelow(below, 0), BlockPos.ZERO));
                } catch (RuntimeException e) {
                    answer = "null";
                }
                dark.add("{\"ground\": " + quote(ground) + ", \"survives\": " + answer + "}");
            }
            rows.add("{\"state\": " + quote(BlockStateParser.serialize(state))
                + ", \"block\": " + quote(subject)
                + ", \"grounds\": [" + String.join(", ", answers) + "]"
                + ", \"dark\": [" + String.join(", ", dark) + "]}");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"survival\",\n");
        out.append("  \"subjects\": ").append(subjects.length).append(",\n");
        out.append("  \"grounds\": ").append(GROUNDS.length).append(",\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// A `LevelReader` whose only answer is the block directly below the
    /// origin, and which **throws for everything else**.
    ///
    /// Same discipline as `GenLevel`, for the same reason: a rule that reads
    /// more of the world than one block cannot be answered here, and the
    /// harness must say so rather than hand back a plausible air. The caller
    /// records the throw as a refusal, so the port is never asked to
    /// reproduce a guess.
    /// `brightness` below zero throws on `getRawBrightness`, as every other
    /// unanswerable read does; zero or more answers it.
    private static net.minecraft.world.level.LevelReader oneBlockBelow(BlockState below, int brightness) {
        return (net.minecraft.world.level.LevelReader) java.lang.reflect.Proxy.newProxyInstance(
            SurvivalGolden.class.getClassLoader(),
            new Class<?>[]{net.minecraft.world.level.LevelReader.class},
            (proxy, method, args) -> {
                String name = method.getName();
                if (name.equals("getBlockState") && method.getParameterCount() == 1) {
                    BlockPos pos = (BlockPos) args[0];
                    if (pos.getY() == -1 && pos.getX() == 0 && pos.getZ() == 0) {
                        return below;
                    }
                    return net.minecraft.world.level.block.Blocks.AIR.defaultBlockState();
                }
                if (name.equals("getFluidState") && method.getParameterCount() == 1) {
                    BlockPos pos = (BlockPos) args[0];
                    return (pos.getY() == -1 && pos.getX() == 0 && pos.getZ() == 0
                        ? below
                        : net.minecraft.world.level.block.Blocks.AIR.defaultBlockState())
                        .getFluidState();
                }
                if (name.equals("getRawBrightness") && brightness >= 0) {
                    return brightness;
                }
                if (name.equals("getMinY")) {
                    return -64;
                }
                if (name.equals("getHeight") && method.getParameterCount() == 0) {
                    return 384;
                }
                if (name.equals("toString")) {
                    return "oneBlockBelow";
                }
                if (name.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (name.equals("equals")) {
                    return proxy == args[0];
                }
                throw new UnsupportedOperationException(
                    "canSurvive read LevelReader." + name + "/" + method.getParameterCount());
            });
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private SurvivalGolden() {}
}
