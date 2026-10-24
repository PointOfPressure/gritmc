// PathFactsGolden — every per-STATE fact a mob's pathfinder, navigation and
// movement tick read off a block, keyed by global block-state id.
//
// `crates/rustmc-entity/src/host.rs::BlockFacts` is the list of block tests
// the transcribed AI makes — `isPathfindable(LAND)`, `is(BlockTags.FENCES)`,
// `DoorBlock.OPEN`, the fluid's own height, `Block.getJumpFactor()` … — and
// every one of them is either a virtual method with per-block overrides
// (`isPathfindable` has fourteen), a tag bound at datapack reload, or a
// `Block` field set from `Properties`. None is a function of the state's
// name, so they are data or they are guesses.
//
// WHERE EACH ONE IS READ:
//
//   pathLand/Water/Air   WalkNodeEvaluator.getPathTypeFromState and the
//                        navigation's own block tests.
//   blocksMotion         WalkNodeEvaluator (STONE vs. open), Entity.move's
//                        support tests through the collision table.
//   fluid*               Entity.updateFluidHeightAndDoFluidPushing
//                        (EntityFluidInteraction.update): `getHeight` is
//                        1.0 when the same fluid is above, else
//                        `getOwnHeight` = amount / 9.
//   jumpFactor           Entity.getBlockJumpFactor, feeding
//                        LivingEntity.getJumpPower.
//   the tags             getPathTypeFromState (trapdoors, fences, walls,
//                        fire, beds…), EatBlockGoal (edible_for_sheep),
//                        InteractWithDoor (mob_interactable_doors).
//
// Tags are read from the generated data reports (`-Drustmc.data`), resolved
// recursively, exactly as MovementPropsGolden does and for the reason it
// documents: `state.is(TagKey)` is false for every block after
// `Bootstrap.bootStrap()` because nothing bound the tags.
//
// Rows are de-duplicated: `rows` holds the distinct fact tuples and
// `stateRows[id]` the row index for global state `id`, so 32,366 states
// cost one int each. Every float is emitted readable and as raw bits, and
// the reader takes the bits.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.BedBlock;
import net.minecraft.world.level.block.BaseRailBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.CampfireBlock;
import net.minecraft.world.level.block.DoorBlock;
import net.minecraft.world.level.block.FenceGateBlock;
import net.minecraft.world.level.block.LeavesBlock;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.BedPart;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.level.pathfinder.PathComputationType;

public final class PathFactsGolden {
    private PathFactsGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static final String[] TAGS = {
        "trapdoors", "fences", "walls", "fire", "beds", "speleothems",
        "edible_for_sheep", "cauldrons", "doors", "mob_interactable_doors",
    };

    private static void collectTag(java.nio.file.Path root, String tag, java.util.Set<String> into,
            java.util.Set<String> seen) throws Exception {
        if (!seen.add(tag)) {
            return;
        }
        String[] parts = tag.split(":", 2);
        java.nio.file.Path f = root.resolve("data").resolve(parts[0])
                .resolve("tags").resolve("block").resolve(parts[1] + ".json");
        if (!java.nio.file.Files.exists(f)) {
            throw new IllegalStateException("no tag file for #" + tag + " at " + f);
        }
        com.google.gson.JsonObject o = com.google.gson.JsonParser
                .parseString(java.nio.file.Files.readString(f)).getAsJsonObject();
        for (com.google.gson.JsonElement v : o.getAsJsonArray("values")) {
            String s = v.isJsonObject() ? v.getAsJsonObject().get("id").getAsString()
                                        : v.getAsString();
            if (s.startsWith("#")) {
                collectTag(root, s.substring(1), into, seen);
            } else {
                into.add(s.contains(":") ? s : "minecraft:" + s);
            }
        }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("-Drustmc.data is required: block tags come from the "
                    + "generated data reports, not from the runtime registry");
        }
        java.nio.file.Path root = java.nio.file.Path.of(data);
        Map<String, java.util.Set<String>> tags = new LinkedHashMap<>();
        for (String t : TAGS) {
            java.util.Set<String> s = new java.util.HashSet<>();
            collectTag(root, "minecraft:" + t, s, new java.util.HashSet<>());
            if (s.isEmpty()) {
                throw new IllegalStateException("tag #minecraft:" + t + " resolved to nothing");
            }
            tags.put(t, s);
        }

        // Block name -> index, in registry order, so a row can carry the
        // block as an int.
        List<String> blockNames = new ArrayList<>();
        Map<Block, Integer> blockIndex = new java.util.HashMap<>();
        for (net.minecraft.resources.Identifier id : BuiltInRegistries.BLOCK.keySet()) {
            Block b = BuiltInRegistries.BLOCK.getValue(id);
            blockIndex.put(b, blockNames.size());
            blockNames.add(id.toString());
        }

        Map<String, Integer> rowOf = new LinkedHashMap<>();
        List<Integer> stateRows = new ArrayList<>();
        int pathLandCount = 0;
        int fluidCount = 0;
        int doorCount = 0;
        for (BlockState st : Block.BLOCK_STATE_REGISTRY) {
            Block b = st.getBlock();
            String name = blockNames.get(blockIndex.get(b));
            FluidState fs = st.getFluidState();
            int fluidKind = fs.isEmpty() ? 0 : (fs.getType().isSame(Fluids.WATER) ? 1
                    : (fs.getType().isSame(Fluids.LAVA) ? 2 : 3));
            if (fluidKind == 3) {
                throw new IllegalStateException("a fluid that is neither water nor lava on " + st);
            }
            boolean pathLand = st.isPathfindable(PathComputationType.LAND);
            boolean pathWater = st.isPathfindable(PathComputationType.WATER);
            boolean pathAir = st.isPathfindable(PathComputationType.AIR);
            if (pathLand) {
                pathLandCount++;
            }
            if (fluidKind != 0) {
                fluidCount++;
            }
            StringBuilder r = new StringBuilder();
            r.append("{").append(q("block")).append(": ").append(blockIndex.get(b));
            r.append(", ").append(q("air")).append(": ").append(st.isAir());
            r.append(", ").append(q("blocksMotion")).append(": ").append(st.blocksMotion());
            r.append(", ").append(q("solid")).append(": ").append(st.isSolid());
            r.append(", ").append(q("solidRender")).append(": ").append(st.isSolidRender());
            r.append(", ").append(q("redstoneConductor")).append(": ")
                    .append(st.isRedstoneConductor(EmptyBlockGetter.INSTANCE, BlockPos.ZERO));
            r.append(", ").append(q("pathLand")).append(": ").append(pathLand);
            r.append(", ").append(q("pathWater")).append(": ").append(pathWater);
            r.append(", ").append(q("pathAir")).append(": ").append(pathAir);
            r.append(", ").append(q("fluidKind")).append(": ").append(fluidKind);
            r.append(", ").append(q("fluidSource")).append(": ").append(!fs.isEmpty() && fs.isSource());
            r.append(", ").append(q("fluidAmount")).append(": ").append(fs.isEmpty() ? 0 : fs.getAmount());
            float own = fs.isEmpty() ? 0.0F : fs.getOwnHeight();
            r.append(", ").append(q("fluidOwnHeight")).append(": ").append(own)
                    .append(", ").append(q("fluidOwnHeightBits")).append(": ")
                    .append(Float.floatToRawIntBits(own));
            float jf = b.getJumpFactor();
            r.append(", ").append(q("jumpFactor")).append(": ").append(jf)
                    .append(", ").append(q("jumpFactorBits")).append(": ")
                    .append(Float.floatToRawIntBits(jf));
            for (Map.Entry<String, java.util.Set<String>> t : tags.entrySet()) {
                r.append(", ").append(q("tag_" + t.getKey())).append(": ")
                        .append(t.getValue().contains(name));
            }
            if (b instanceof DoorBlock door) {
                doorCount++;
                r.append(", ").append(q("doorOpen")).append(": ").append(st.getValue(DoorBlock.OPEN));
                r.append(", ").append(q("doorByHand")).append(": ")
                        .append(door.type().canOpenByHand());
            }
            if (b instanceof FenceGateBlock) {
                r.append(", ").append(q("fenceGateOpen")).append(": ")
                        .append(st.getValue(FenceGateBlock.OPEN));
            }
            r.append(", ").append(q("leaves")).append(": ").append(b instanceof LeavesBlock);
            r.append(", ").append(q("rail")).append(": ").append(b instanceof BaseRailBlock);
            r.append(", ").append(q("litCampfire")).append(": ").append(CampfireBlock.isLitCampfire(st));
            r.append(", ").append(q("bedHead")).append(": ")
                    .append(b instanceof BedBlock && st.getValue(BedBlock.PART) == BedPart.HEAD);
            r.append("}");
            String row = r.toString();
            Integer idx = rowOf.get(row);
            if (idx == null) {
                idx = rowOf.size();
                rowOf.put(row, idx);
            }
            stateRows.add(idx);
        }
        if (pathLandCount == 0 || fluidCount == 0 || doorCount == 0) {
            throw new IllegalStateException("a facts column came back empty: pathLand="
                    + pathLandCount + " fluid=" + fluidCount + " door=" + doorCount);
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n  ").append(q("version")).append(": ").append(q(SharedConstants.getCurrentVersion().name()));
        out.append(",\n  ").append(q("states")).append(": ").append(stateRows.size());
        out.append(",\n  ").append(q("pathLandStates")).append(": ").append(pathLandCount);
        out.append(",\n  ").append(q("fluidStates")).append(": ").append(fluidCount);
        out.append(",\n  ").append(q("tags")).append(": [");
        for (int i = 0; i < TAGS.length; i++) {
            out.append(i == 0 ? "" : ", ").append(q(TAGS[i]));
        }
        out.append("],\n  ").append(q("blocks")).append(": [");
        for (int i = 0; i < blockNames.size(); i++) {
            out.append(i == 0 ? "" : ", ").append(q(blockNames.get(i)));
        }
        out.append("],\n  ").append(q("rows")).append(": [\n");
        int i = 0;
        for (String row : rowOf.keySet()) {
            out.append(i++ == 0 ? "    " : ",\n    ").append(row);
        }
        out.append("\n  ],\n  ").append(q("stateRows")).append(": [");
        for (int s = 0; s < stateRows.size(); s++) {
            out.append(s == 0 ? "" : ",").append(stateRows.get(s));
        }
        out.append("]\n}\n");
        System.out.print(out);
    }
}
