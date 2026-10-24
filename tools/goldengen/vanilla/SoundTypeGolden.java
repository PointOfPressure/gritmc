// SoundTypeGolden — the `SoundType` behind every block state.
//
// WHAT READS THIS, and why one table serves three call sites:
//
//   BlockItem.place, offsets 201-240
//       SoundType t = level.getBlockState(pos).getSoundType();
//       level.playSound(player, pos, getPlaceSound(state), SoundSource.BLOCKS,
//                       (t.getVolume() + 1.0F) / 2.0F, t.getPitch() * 0.8F);
//   Entity.playStepSound, offsets 0-25
//       SoundType t = state.getSoundType();
//       playSound(t.getStepSound(), t.getVolume() * 0.15F, t.getPitch());
//   Block.playerWillDestroy / destroy particles
//       t.getBreakSound(), at a volume this server does not send yet.
//
// So the volume and pitch are the *same two floats* at every site and the
// arithmetic on them is the site's. Getting them from a table rather than
// from a rule is not a shortcut: there is no rule. `SoundType.GRASS` is
// volume 1.0 pitch 1.0 and `SoundType.METAL` is 1.0 / 1.5, and nothing about
// either block's name says so.
//
// KEYED BY STATE, NOT BY BLOCK, AND THAT IS DELIBERATE.
// `BlockBehaviour.getSoundType(BlockState)` is `protected` and returns the
// `soundType` field, but it is virtual and subclasses override it — a cherry
// leaves pile and a stacked candle do not sound like their defaults. Calling
// `state.getSoundType()` per state costs one int per state in the table and
// makes every override correct for free; deriving it per block would be a
// claim that no block overrides it, which this dumper counts and reports
// rather than assumes (`overridingBlocks`).
//
// Every float is emitted readable and as raw bits, and the reader takes the
// bits. `0.9f` is not `0.9`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.SoundType;
import net.minecraft.world.level.block.state.BlockState;

public final class SoundTypeGolden {
    private SoundTypeGolden() {}

    private static String q(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static String flt(String name, float v) {
        return q(name) + ": " + v + ", " + q(name + "Bits") + ": " + Float.floatToRawIntBits(v);
    }

    private static String ev(String name, SoundEvent e) {
        return q(name) + ": " + (e == null ? q("") : q(e.location().toString()));
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        // Identity-keyed, because `SoundType` has no `equals` and two
        // distinct instances with the same five events are still two
        // instances. The order is first-seen over the state registry, which
        // is stable because that registry's iteration order is.
        Map<SoundType, Integer> indexOf = new LinkedHashMap<>();
        List<String> rows = new ArrayList<>();

        int[] stateType = new int[Block.BLOCK_STATE_REGISTRY.size()];
        java.util.Arrays.fill(stateType, -1);
        for (BlockState st : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(st);
            SoundType t = st.getSoundType();
            Integer idx = indexOf.get(t);
            if (idx == null) {
                idx = rows.size();
                indexOf.put(t, idx);
                rows.add("    {" + flt("volume", t.getVolume())
                        + ", " + flt("pitch", t.getPitch())
                        + ", " + ev("break", t.getBreakSound())
                        + ", " + ev("step", t.getStepSound())
                        + ", " + ev("place", t.getPlaceSound())
                        + ", " + ev("hit", t.getHitSound())
                        + ", " + ev("fall", t.getFallSound()) + "}");
            }
            stateType[id] = idx;
        }

        // How many blocks have a state whose sound type is not the one their
        // default state reports. This is the number that says whether keying
        // by state was necessary; it is reported rather than asserted,
        // because a version in which it is zero is not a broken dump.
        int overriding = 0;
        for (Identifier name : BuiltInRegistries.BLOCK.keySet()) {
            Block b = BuiltInRegistries.BLOCK.getValue(name);
            SoundType d = b.defaultBlockState().getSoundType();
            for (BlockState st : b.getStateDefinition().getPossibleStates()) {
                if (st.getSoundType() != d) {
                    overriding++;
                    break;
                }
            }
        }

        int unmapped = 0;
        StringBuilder states = new StringBuilder();
        for (int i = 0; i < stateType.length; i++) {
            if (stateType[i] < 0) {
                unmapped++;
            }
            if (i > 0) {
                states.append(',');
            }
            states.append(stateType[i]);
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("states")).append(": ").append(stateType.length).append(",\n");
        out.append("  ").append(q("soundTypes")).append(": ").append(rows.size()).append(",\n");
        out.append("  ").append(q("unmappedStates")).append(": ").append(unmapped).append(",\n");
        out.append("  ").append(q("overridingBlocks")).append(": ").append(overriding)
                .append(",\n");
        out.append("  ").append(q("table")).append(": [\n");
        out.append(String.join(",\n", rows));
        out.append("\n  ],\n");
        out.append("  ").append(q("stateTypes")).append(": [").append(states).append("]\n}\n");
        System.out.print(out);
    }
}
