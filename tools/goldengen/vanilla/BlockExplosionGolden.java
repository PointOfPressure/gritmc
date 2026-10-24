// BlockExplosionGolden — the two numbers vanilla's explosion ray walk reads
// off a block, dumped from the pinned jar.
//
// WHAT READS THIS, and it is exactly one expression:
//
//   ExplosionDamageCalculator.getBlockExplosionResistance, offsets 0-42
//       if (state.isAir() && fluid.isEmpty()) return Optional.empty();
//       return Optional.of(Math.max(state.getBlock().getExplosionResistance(),
//                                   fluid.getExplosionResistance()));
//
//   ServerExplosion.calculateExplodedPositions, offsets 288-316
//       if (res.isPresent()) f -= (res.get() + 0.3f) * 0.3f;
//
// So one float per *block* and one per *fluid* is the whole of the block side
// of an explosion's geometry. There is no rule behind it — obsidian is 1200
// and crying obsidian is 1200 and glowstone is 0.3, and nothing about either
// name says so — so this is a table or it is a guess.
//
// KEYED BY BLOCK, NOT BY STATE, AND THE BYTECODE IS WHY.
// The call is `state.getBlock().getExplosionResistance()`: it goes through
// the *block*, so no property of the state can move it. That is a claim, and
// the dumper does not take it on trust — it walks every state of every block
// and counts the states whose block's resistance differs from its default
// state's block's resistance, which is zero by construction and is emitted so
// that a version where the call moves to the state stops being silent.
//
// HARDNESS IS DELIBERATELY ABSENT. Nothing on the explosion path reads
// `destroyTime`: `calculateExplodedPositions` reads only the resistance,
// `BlockBehaviour.onExplosionHit` goes straight to `setBlock(pos, AIR, 3)`
// without consulting a break time, and `getDrops` takes the loot table. A
// column no reader names is the defect class this project keeps finding, so
// there is no hardness column here.
//
// THE CROSS-CHECK, AND THE FIRST VERSION OF IT THAT DID NOT WORK.
//
// The obvious second source is a reflective read of the
// `BlockBehaviour.explosionResistance` field the base implementation returns,
// against the virtual call. It was written that way first and it refused
// itself on the first run: the two agree for **every block in 26.2**, so the
// comparison had no power at all. A `javap -p -c -constants` sweep of all 405
// classes under `net/minecraft/world/level/block/` says why — exactly one
// class overrides the method,
//
//     StairBlock.getExplosionResistance()
//         0: aload_0
//         1: getfield  #75   // Field base:Lnet/minecraft/world/level/block/Block;
//         4: invokevirtual  // Block.getExplosionResistance:()F
//
// and a stair is built with `Properties.ofFullCopy(base)`, so the field it
// overrides already holds the base block's number. The override is real and
// value-preserving, which is exactly the case a value comparison cannot see.
//
// So there are two arms and both can go red:
//
//   `methodOverridesField` — blocks where the call and the field disagree.
//       Empty in 26.2, and the emptiness is a claim about the whole registry
//       rather than an absence of evidence, because the second arm explains
//       it.
//   `stairsDisagreeingWithBase` — for every `StairBlock`, the stair's
//       resistance against `StairBlock.base`'s, read through a *different*
//       field. Zero in 26.2. If a stair ever stops copying its base's
//       properties this goes non-zero **and** `methodOverridesField` gains
//       that stair, so the two arms move together and neither can be green
//       for the wrong reason. `stairsChecked` is emitted beside it: a zero
//       there means the reflective arm found no stairs and is measuring
//       nothing, which `ci/block-explosion-golden.sh` refuses.
//
// The denominator's second source is a different dumper: the state count here
// must equal `data/vanilla/block_props.json`'s, which `ci/block-props-golden.sh`
// derives from the same registry by another route. That is asserted in the
// shell script, not here.
//
// Every float is emitted readable and as raw bits, and the reader takes the
// bits. `0.30000001192092896` is not `0.3`.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.TreeSet;

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockBehaviour;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.Fluid;

public final class BlockExplosionGolden {
    private BlockExplosionGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static String flt(String name, float v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Float.floatToRawIntBits(v);
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // The second source: the field the base implementation returns.
        // `BlockBehaviour.explosionResistance` is `protected final float`, so
        // it is reachable by reflection and by nothing else.
        Field resField = BlockBehaviour.class.getDeclaredField("explosionResistance");
        resField.setAccessible(true);

        // Every distinct (resistance, dropFromExplosion) pair, first-seen over
        // the block registry, which iterates in a stable order.
        Map<String, Integer> indexOf = new LinkedHashMap<>();
        List<String> rows = new ArrayList<>();
        Map<Identifier, Integer> blockRow = new LinkedHashMap<>();

        // `StairBlock.base` — the second arm's independent field. The class is
        // looked up by name so that its absence is a loud failure rather than
        // a silently skipped check.
        Field stairBase = Class.forName("net.minecraft.world.level.block.StairBlock")
                .getDeclaredField("base");
        stairBase.setAccessible(true);
        Class<?> stairClass = stairBase.getDeclaringClass();
        int stairsChecked = 0;
        TreeSet<String> stairsDisagreeing = new TreeSet<>();

        // Blocks whose virtual call and whose field disagree. Named, not
        // counted: an override that appears in a later version has to be read
        // by a human, and a bare number cannot ask for that.
        TreeSet<String> overriding = new TreeSet<>();
        // Blocks that refuse to drop when an explosion breaks them.
        TreeSet<String> noDrop = new TreeSet<>();

        for (Identifier name : BuiltInRegistries.BLOCK.keySet()) {
            Block b = BuiltInRegistries.BLOCK.getValue(name);
            float res = b.getExplosionResistance();
            float fieldRes = resField.getFloat(b);
            if (Float.floatToRawIntBits(res) != Float.floatToRawIntBits(fieldRes)) {
                overriding.add(name.toString());
            }
            if (stairClass.isInstance(b)) {
                stairsChecked++;
                Block base = (Block) stairBase.get(b);
                if (Float.floatToRawIntBits(res)
                        != Float.floatToRawIntBits(base.getExplosionResistance())) {
                    stairsDisagreeing.add(name.toString());
                }
            }
            // `Block.dropFromExplosion` and `TntBlock.dropFromExplosion` both
            // ignore the argument (`iconst_1` / `iconst_0`); anything that did
            // not would throw here rather than answer wrongly.
            boolean drops;
            try {
                drops = b.dropFromExplosion(null);
            } catch (Throwable t) {
                throw new IllegalStateException(
                        "dropFromExplosion(null) threw for " + name
                                + " — it reads its argument and this dumper cannot answer for it",
                        t);
            }
            if (!drops) {
                noDrop.add(name.toString());
            }
            String key = Float.floatToRawIntBits(res) + "/" + drops;
            Integer idx = indexOf.get(key);
            if (idx == null) {
                idx = rows.size();
                indexOf.put(key, idx);
                rows.add("    {" + flt("resistance", res)
                        + ", " + q("dropFromExplosion") + ": " + drops + "}");
            }
            blockRow.put(name, idx);
        }

        // The claim that keying by block rather than by state is sound. The
        // call under test is `state.getBlock()`, so this is zero by
        // construction — and it is emitted so that a version in which the call
        // moves onto the state does not pass in silence.
        int stateDisagreements = 0;
        int[] stateRow = new int[Block.BLOCK_STATE_REGISTRY.size()];
        java.util.Arrays.fill(stateRow, -1);
        for (BlockState st : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(st);
            Block b = st.getBlock();
            Identifier name = BuiltInRegistries.BLOCK.getKey(b);
            Integer idx = blockRow.get(name);
            if (idx == null) {
                throw new IllegalStateException("state " + id + " has a block not in the registry");
            }
            stateRow[id] = idx;
            float viaState = st.getBlock().getExplosionResistance();
            float viaBlock = b.getExplosionResistance();
            if (Float.floatToRawIntBits(viaState) != Float.floatToRawIntBits(viaBlock)) {
                stateDisagreements++;
            }
        }
        int unmapped = 0;
        for (int v : stateRow) {
            if (v < 0) {
                unmapped++;
            }
        }

        // Fluids. `FluidState.getExplosionResistance()` is
        // `getType().getExplosionResistance()`, so this is per fluid and the
        // flowing/source pair share one instance-per-name in the registry.
        List<String> fluids = new ArrayList<>();
        for (Identifier name : BuiltInRegistries.FLUID.keySet()) {
            Fluid f = BuiltInRegistries.FLUID.getValue(name);
            // `Fluid.getExplosionResistance()` is protected; the public route
            // is the one the explosion path itself takes,
            // `FluidState.getExplosionResistance()`, which is
            // `getType().getExplosionResistance()` and so reads the same field.
            fluids.add("    {" + q("id") + ": " + q(name.toString())
                    + ", " + flt("resistance", f.defaultFluidState().getExplosionResistance())
                    + "}");
        }

        StringBuilder blocks = new StringBuilder();
        for (Map.Entry<Identifier, Integer> e : blockRow.entrySet()) {
            if (blocks.length() > 0) {
                blocks.append(",\n");
            }
            blocks.append("    {").append(q("id")).append(": ").append(q(e.getKey().toString()))
                    .append(", ").append(q("row")).append(": ").append(e.getValue()).append("}");
        }

        StringBuilder stairNames = new StringBuilder();
        for (String s : stairsDisagreeing) {
            if (stairNames.length() > 0) {
                stairNames.append(", ");
            }
            stairNames.append(q(s));
        }

        StringBuilder names = new StringBuilder();
        for (String s : overriding) {
            if (names.length() > 0) {
                names.append(", ");
            }
            names.append(q(s));
        }
        StringBuilder noDropNames = new StringBuilder();
        for (String s : noDrop) {
            if (noDropNames.length() > 0) {
                noDropNames.append(", ");
            }
            noDropNames.append(q(s));
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("blocks")).append(": ").append(blockRow.size()).append(",\n");
        out.append("  ").append(q("states")).append(": ").append(stateRow.length).append(",\n");
        out.append("  ").append(q("rows")).append(": ").append(rows.size()).append(",\n");
        out.append("  ").append(q("unmappedStates")).append(": ").append(unmapped).append(",\n");
        out.append("  ").append(q("stateDisagreements")).append(": ").append(stateDisagreements)
                .append(",\n");
        out.append("  ").append(q("methodOverridesField")).append(": [").append(names)
                .append("],\n");
        out.append("  ").append(q("stairsChecked")).append(": ").append(stairsChecked)
                .append(",\n");
        out.append("  ").append(q("stairsDisagreeingWithBase")).append(": [").append(stairNames)
                .append("],\n");
        out.append("  ").append(q("noDropFromExplosion")).append(": [").append(noDropNames)
                .append("],\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ],\n");
        out.append("  ").append(q("blockRows")).append(": [\n");
        out.append(blocks);
        out.append("\n  ],\n");
        out.append("  ").append(q("fluids")).append(": [\n");
        out.append(String.join(",\n", fluids));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
