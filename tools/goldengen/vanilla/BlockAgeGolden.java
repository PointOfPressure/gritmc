// BlockAgeGolden — vanilla's OWN `BlockAgeProcessor` draw tape, per position.
//
// # What this is for
//
// `BlockAgeProcessor`'s random is per position: `processBlock` opens with
// `settings.getRandom(info.pos())`, and with no random set on the settings
// that is `RandomSource.create(Mth.getSeed(pos))` on every call. So each
// block's draws are independent of every other block's, and a wrong draw
// *order within one position* changes that block and nothing else.
//
// That independence is what makes this harness possible at all: the whole
// mechanism can be driven one position at a time with no world, no chunk, no
// generator and no server — which is why the tape it prints is exhaustive
// over the positions in question rather than sampled.
//
// # Why the random is injected rather than left to the processor
//
// `StructurePlaceSettings.getRandom(pos)` returns `this.random` when one has
// been set and only falls back to the position seed when it is null. Setting
// a **recording** wrapper around `RandomSource.create(Mth.getSeed(pos))`
// therefore reproduces the exact stream the processor would have built for
// itself, while letting the draws be read off as they happen.
//
// This matters because the losing candidate array's draws are otherwise
// invisible. `maybeReplaceFullStoneBlock` builds both arrays before
// `getRandomBlock` chooses, and the array that loses is discarded — the
// `getRandomFacingStairs` draws spent on it never reach the world, so no
// block diff and no census can see them. Only the ordered sequence can.
//
// # What it prints
//
// One line per position, in the same shape as the port's own
// `RUSTMC_BLOCK_AGE_TAPE` lines, so the two can be diffed field for field:
//
//   TAPE pos=x,y,z seed=<Mth.getSeed> src=<state> draws=N 0:kind=v ... out=<state>
//
// `out=-` means the processor left the block alone. Driven by
// `ci/block-age-golden.sh`.
//
// # What it does NOT establish
//
// The source state is supplied by the caller. This harness answers "given
// this state at this position, what does vanilla's processor do" — it does
// not confirm that vanilla's template really holds that state there. The
// saved region is what confirms that, and the shell script compares against
// it.

// # Why this lives in vanilla's own package
//
// `state.is(BlockTags.SLABS)` and its siblings answer **false** for every
// state until the tags are bound, and nothing about an unbound tag looks like
// an error: the processor simply falls through every tag-gated arm and reports
// that vanilla does nothing. That is a fabricated golden of exactly the class
// this project keeps paying for — a measurement whose failure mode is silence.
//
// `SurfaceChunkGolden.bindBlockTags()` already loads them from the jar's own
// generated data and is shared by five other goldens, but it is
// package-private, so this class joins its package rather than growing a
// second copy of tag loading. Caught on 2026-08-09 by the draw-count
// histogram: unbound, vanilla taped 16 slabs at 0 draws where the port taped
// them at 1, and the 16 were exactly `#minecraft:slabs`.
package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.structure.templatesystem.BlockAgeProcessor;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructurePlaceSettings;
import net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate;

public class BlockAgeGolden {

    /// Delegates every call and keeps the sequence. The names recorded are
    /// vanilla's own call names so the tape reads against `javap` directly.
    static final class Recording implements RandomSource {
        private final RandomSource inner;
        final List<String> draws = new ArrayList<>();

        Recording(RandomSource inner) {
            this.inner = inner;
        }

        @Override public RandomSource fork() { return inner.fork(); }
        @Override public PositionalRandomFactory forkPositional() { return inner.forkPositional(); }
        @Override public void setSeed(long seed) { inner.setSeed(seed); draws.add("setSeed=" + seed); }

        @Override public int nextInt() {
            int v = inner.nextInt();
            draws.add("nextInt=" + v);
            return v;
        }

        @Override public int nextInt(int bound) {
            int v = inner.nextInt(bound);
            draws.add("nextInt(" + bound + ")=" + v);
            return v;
        }

        @Override public long nextLong() {
            long v = inner.nextLong();
            draws.add("nextLong=" + v);
            return v;
        }

        @Override public boolean nextBoolean() {
            boolean v = inner.nextBoolean();
            draws.add("nextBoolean=" + v);
            return v;
        }

        @Override public float nextFloat() {
            float v = inner.nextFloat();
            draws.add(String.format("nextFloat=%.8f", v));
            return v;
        }

        @Override public double nextDouble() {
            double v = inner.nextDouble();
            draws.add(String.format("nextDouble=%.10f", v));
            return v;
        }

        @Override public double nextGaussian() {
            double v = inner.nextGaussian();
            draws.add(String.format("nextGaussian=%.10f", v));
            return v;
        }
    }

    /// `args[0]` mossiness; positions on stdin, one `x,y,z,<block state>` per
    /// line. Stdin rather than argv because the portal box is 554 positions
    /// and a truncated argument list would silently shorten an exhaustive
    /// tape into a sampled one.
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // Without this every `state.is(BlockTags.…)` arm is silently dead.
        SurfaceChunkGolden.bindBlockTags();
        // Checked rather than assumed: a binder that loaded nothing would
        // leave the tags empty and this harness would go back to reporting
        // that vanilla never touches a slab.
        if (net.minecraft.core.registries.BuiltInRegistries.BLOCK
                .get(net.minecraft.tags.BlockTags.SLABS).isEmpty()) {
            throw new IllegalStateException(
                "#minecraft:slabs is empty after bindBlockTags — every tag-gated arm "
                + "of BlockAgeProcessor would report that vanilla does nothing");
        }

        float mossiness = Float.parseFloat(args[0]);
        BlockAgeProcessor processor = new BlockAgeProcessor(mossiness);
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderLookup<net.minecraft.world.level.block.Block> blocks =
            registries.lookupOrThrow(Registries.BLOCK);
        System.out.println("MOSSINESS " + mossiness);

        List<String> lines = new ArrayList<>();
        try (java.io.BufferedReader r = new java.io.BufferedReader(
                new java.io.InputStreamReader(System.in))) {
            String line;
            while ((line = r.readLine()) != null) {
                line = line.trim();
                if (!line.isEmpty()) {
                    lines.add(line);
                }
            }
        }

        for (int i = 0; i < lines.size(); i++) {
            String[] f = lines.get(i).split(",", 4);
            BlockPos pos = new BlockPos(
                Integer.parseInt(f[0]), Integer.parseInt(f[1]), Integer.parseInt(f[2]));
            BlockState src = BlockStateParser
                .parseForBlock(blocks, f[3], true)
                .blockState();

            long seed = Mth.getSeed(pos.getX(), pos.getY(), pos.getZ());
            Recording rec = new Recording(RandomSource.create(seed));
            StructurePlaceSettings settings = new StructurePlaceSettings().setRandom(rec);
            StructureTemplate.StructureBlockInfo info =
                new StructureTemplate.StructureBlockInfo(pos, src, null);

            // Only `info` and `settings` are read by this processor; the
            // level and the three positions are untouched by every arm.
            StructureTemplate.StructureBlockInfo out =
                processor.processBlock(null, BlockPos.ZERO, BlockPos.ZERO, BlockPos.ZERO, info, settings);

            StringBuilder sb = new StringBuilder();
            sb.append("TAPE pos=").append(pos.getX()).append(',').append(pos.getY())
              .append(',').append(pos.getZ())
              .append(" seed=").append(seed)
              .append(" src=").append(BlockStateParser.serialize(src))
              .append(" draws=").append(rec.draws.size());
            for (int d = 0; d < rec.draws.size(); d++) {
                sb.append(' ').append(d).append(':').append(rec.draws.get(d));
            }
            // `processBlock` returns the input info unchanged when no arm
            // fired. Comparing the STATE rather than the info identity is
            // what makes "left alone" and "replaced by an identical state"
            // the same answer here, which is what the port's tape records.
            boolean changed = out != null && !out.state().equals(src);
            sb.append(" out=").append(changed ? BlockStateParser.serialize(out.state()) : "-");
            System.out.println(sb);
        }
    }
}
