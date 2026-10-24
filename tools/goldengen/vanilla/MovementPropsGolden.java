// MovementPropsGolden — the three per-BLOCK floats the movement tick reads.
//
// `friction`, `speedFactor` and `bounceRestitution` are fields on `Block`,
// set from `BlockBehaviour.Properties` at construction. They are **not** in
// `block_props.json`, which is keyed by block *state* and carries the six
// per-face booleans; these are per block and there is no rule connecting them
// to a block's name — ice is 0.98, blue ice 0.989, slime 0.8, soul sand
// 0.4 speed, honey both — so they are data or they are wrong.
//
// WHERE EACH ONE IS READ, so that a future reader can tell which of these is
// load-bearing for what:
//
//   friction            ItemEntity.tick, when onGround():
//                         drag = getAirDrag()
//                             * level.getBlockState(getBlockPosBelowThatAffectsMyMovement())
//                                    .getBlock().getFriction()
//                       and LivingEntity.travel's ground branch.
//   speedFactor         Entity.move's last statement:
//                         setDeltaMovement(delta.multiply(f, 1.0, f))
//                       with f = getBlockSpeedFactor(), which is the factor of
//                       the block at blockPosition(), falling back to the block
//                       below when that one is 1.0.
//   bounceRestitution   Entity.getBlockBounciness(block), which is
//                         block.getBounceRestitution()
//                             * (entity instanceof LivingEntity ? 1 : 0.8)
//                       feeding restituteMovementAfterCollisions.
//
// The third is carried even though **nothing in this server can reach a
// non-zero one today**: every block in the gameplay corpus has restitution 0,
// so the restitution path reduces to "zero the collided axis". Carrying it
// costs one float per block and removes the temptation to write that
// reduction into the code as if it were the rule, which is exactly the shape
// of a one-way door this project refuses.
//
// Every float is emitted readable and as raw bits, and the reader takes the
// bits. `0.6f` is not `0.6`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.TreeMap;

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;

public final class MovementPropsGolden {
    private MovementPropsGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static String flt(String name, float v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Float.floatToRawIntBits(v);
    }

    /// Block tag membership, read from the **generated data reports** rather
    /// than from `BlockTags.FENCES`.
    ///
    /// MEASURED 2026-08-06, and this is the second thing in this tree to be
    /// caught by it: `Block.builtInRegistryHolder().is(TagKey)` returns
    /// `false` for every block after `Bootstrap.bootStrap()`, because tags are
    /// bound by the server's datapack reload and a dumper never does one. The
    /// dump came back with `oak_fence.tallSupport = false`, which is a
    /// perfectly plausible-looking table in which no block is ever tall, and
    /// only the generator's own named assertion caught it.
    ///
    /// The reports under `-Drustmc.data` are the same data the reload would
    /// have read. Values beginning `#` are tag references and are resolved
    /// recursively; nothing else in a vanilla tag file needs handling, and an
    /// unresolvable reference is an exception rather than an empty set.
    private static void collectTag(java.nio.file.Path root, String tag, java.util.Set<String> into,
            java.util.Set<String> seen) throws Exception {
        if (!seen.add(tag)) {
            return; // vanilla's tags are acyclic; this is belt and braces.
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
            throw new IllegalStateException("-Drustmc.data is required: the block tags come from "
                    + "the generated data reports, not from the runtime registry");
        }
        java.nio.file.Path root = java.nio.file.Path.of(data);
        java.util.Set<String> fenceTag = new java.util.HashSet<>();
        java.util.Set<String> wallTag = new java.util.HashSet<>();
        collectTag(root, "minecraft:fences", fenceTag, new java.util.HashSet<>());
        collectTag(root, "minecraft:walls", wallTag, new java.util.HashSet<>());

        Map<String, Block> blocks = new TreeMap<>();
        for (Identifier id : BuiltInRegistries.BLOCK.keySet()) {
            blocks.put(id.toString(), BuiltInRegistries.BLOCK.getValue(id));
        }

        // Block name -> its index in the emitted row list, so that the state
        // map below can be a plain `int` per global state id rather than a
        // string. 32,366 strings would be a megabyte of table to answer a
        // question the server asks once per block per entity per tick.
        Map<String, Integer> rowOf = new java.util.HashMap<>();
        List<String> rows = new ArrayList<>();
        int nonDefaultFriction = 0;
        int nonDefaultSpeed = 0;
        int nonZeroBounce = 0;
        for (Map.Entry<String, Block> e : blocks.entrySet()) {
            rowOf.put(e.getKey(), rows.size());
            Block b = e.getValue();
            float fr = b.getFriction();
            float sp = b.getSpeedFactor();
            float bo = b.getBounceRestitution();
            if (fr != 0.6F) {
                nonDefaultFriction++;
            }
            if (sp != 1.0F) {
                nonDefaultSpeed++;
            }
            if (bo != 0.0F) {
                nonZeroBounce++;
            }
            // Two predicates that live here rather than in a tag table,
            // because they are read on the same line as the floats above and
            // this server has no tag table at all.
            //
            //   fence         state.is(BlockTags.FENCES). **Kept separate
            //                 from the other two**, because `getOnPos` guards
            //                 it with `offset <= 0.5` and the two callers in
            //                 this server pass 0.2 and 0.500001 — one either
            //                 side of that comparison. Folding the three into
            //                 one boolean makes a fence behave identically at
            //                 both offsets, which is wrong at both.
            //   wallOrGate    state.is(BlockTags.WALLS) or
            //                 block instanceof FenceGateBlock — the two arms
            //                 `getOnPos` tests unconditionally.
            //   waterlike     Entity.getBlockSpeedFactor's short circuit:
            //                   in.is(Blocks.WATER) || in.is(Blocks.BUBBLE_COLUMN)
            boolean fence = fenceTag.contains(e.getKey());
            boolean wallOrGate = wallTag.contains(e.getKey())
                    || b instanceof net.minecraft.world.level.block.FenceGateBlock;
            boolean waterlike = b == net.minecraft.world.level.block.Blocks.WATER
                    || b == net.minecraft.world.level.block.Blocks.BUBBLE_COLUMN;
            rows.add("    {" + q("block") + ": " + q(e.getKey()) + ", " + flt("friction", fr)
                    + ", " + flt("speedFactor", sp) + ", " + flt("bounceRestitution", bo)
                    + ", " + q("fence") + ": " + fence
                    + ", " + q("wallOrGate") + ": " + wallOrGate
                    + ", " + q("waterlike") + ": " + waterlike + "}");
        }

        // Global block-state id -> row index. `Block.BLOCK_STATE_REGISTRY` is
        // the same numbering the chunk sections store, the same one
        // `collision_shapes.json` is indexed by, and the same one
        // `CollisionBlocks::state_id_at` answers — so the server can go from a
        // position to a friction without ever building a state string.
        //
        // Every state gets a row. There is no `-1` here and there must not be:
        // unlike a collision shape, every block has these three floats, so a
        // sentinel would only ever mean "the dumper missed one".
        int[] stateRow = new int[Block.BLOCK_STATE_REGISTRY.size()];
        java.util.Arrays.fill(stateRow, -1);
        for (BlockState st : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(st);
            Identifier name = BuiltInRegistries.BLOCK.getKey(st.getBlock());
            Integer row = rowOf.get(name.toString());
            if (row != null) {
                stateRow[id] = row;
            }
        }
        int unmapped = 0;
        StringBuilder states = new StringBuilder();
        for (int i = 0; i < stateRow.length; i++) {
            if (stateRow[i] < 0) {
                unmapped++;
            }
            if (i > 0) {
                states.append(',');
            }
            states.append(stateRow[i]);
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("blocks")).append(": ").append(blocks.size()).append(",\n");
        out.append("  ").append(q("states")).append(": ").append(stateRow.length).append(",\n");
        out.append("  ").append(q("unmappedStates")).append(": ").append(unmapped).append(",\n");
        out.append("  ").append(q("nonDefaultFriction")).append(": ").append(nonDefaultFriction)
                .append(",\n");
        out.append("  ").append(q("nonDefaultSpeedFactor")).append(": ").append(nonDefaultSpeed)
                .append(",\n");
        out.append("  ").append(q("nonZeroBounceRestitution")).append(": ").append(nonZeroBounce)
                .append(",\n");
        out.append("  ").append(q("fenceTagBlocks")).append(": ").append(fenceTag.size())
                .append(",\n");
        out.append("  ").append(q("wallTagBlocks")).append(": ").append(wallTag.size())
                .append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ],\n");
        out.append("  ").append(q("stateRows")).append(": [").append(states).append("]\n}\n");
        System.out.print(out);
    }
}
