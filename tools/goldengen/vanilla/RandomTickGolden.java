// RandomTickGolden — vanilla's own `Level.getBlockRandomPos`, dumped.
//
// # Why this file exists at all
//
// The random-tick position function is nine instructions and every one of
// them is a place to be quietly wrong. From `Level.getBlockRandomPos`:
//
//     0: randValue = randValue * 3 + 1013904223;
//    13: int r = randValue >> 2;                     // ARITHMETIC shift
//    21: return new BlockPos(x + (r      & 15),
//                            y + (r >> 16 & mask),
//                            z + (r >>  8 & 15));
//
// **The y component takes bits 16.. and the z component takes bits 8..**, and
// y is the only one of the three that consults `mask`. Every account of this
// function outside the bytecode has them the other way round, and a port
// written from one of those produces positions that are uniform, plausible,
// inside the section, and wrong — which is a defect no distribution test and
// no "did anything happen" check can see, because both orderings tick the
// same *number* of blocks in the same section at the same rate.
//
// So the transcription is pinned against vanilla running the method, not
// against a reading of it.
//
// **One thing this golden does NOT catch, said here because the first draft
// of this file claimed it did.** `>> 2` is `ishr` and not `iushr`, and that
// distinction is **unobservable**: the two shifted values differ only in bits
// 30-31 and the method reads bits 0-3, 8-11 and 16-19. Measured over 20,008
// seeds, zero disagreements. A port written with `>>>` is byte-identical
// here, for every input, for ever.
//
// The negative seeds below earn their place for the other reason: the
// multiply wraps at 32 bits, and **that** does move `randValue` itself, which
// is why `endValue` is dumped beside the positions.
//
// # What this dumps and what it deliberately does not
//
// Only the position function. The *rate* — `randomTickSpeed` picks per
// randomly-ticking section per block-ticking chunk per tick — is loop
// structure in `ServerLevel.tickChunk`, and is pinned by counting in
// `crates/rustmc-server/src/randomtick.rs`, not here.
//
// **The absolute sequence is not reproducible across servers and this file
// does not claim it is.** `Level`'s constructor seeds `randValue` from
// `RandomSource.createThreadLocalInstance().nextInt()`, so two vanilla
// servers of the same version on the same world disagree about every random
// tick position they ever choose. What is reproducible, and what is dumped
// here, is the *transition*: given a `randValue`, the next position and the
// next `randValue`. That is the whole of the function, and it is the only
// part of it any implementation can be held to.
//
// # The instance
//
// `getBlockRandomPos` reads and writes exactly one field — `randValue` — and
// calls nothing. So the instance it runs on is allocated without its
// constructor (`Unsafe.allocateInstance`) rather than by standing up a real
// `ServerLevel`, which is not constructible outside a running server.
//
// That is the same argument `GenLevel` makes and it holds for the same
// reason: nothing here reconstructs vanilla behaviour or supplies a plausible
// default. Every field except `randValue` is left at its zero value, so a
// method that reached one would throw rather than answer — and this one does
// not reach any, which is a fact from the bytecode above rather than a hope.
package net.minecraft.world.level.levelgen;

import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.Level;

public final class RandomTickGolden {

    /** How many positions to draw per case. */
    private static final int DRAWS = 24;

    /**
     * `randValue` start values.
     *
     * Zero because it is the one an implementation reaches by forgetting to
     * seed; `Integer.MIN_VALUE` and `Integer.MAX_VALUE` because `* 3`
     * overflows both, which is the transition this golden can actually
     * discriminate; and the negatives because they are where a wrong
     * wrapping shows up first in `endValue`.
     *
     * NOT because of the shift — see the header. That one is invisible.
     */
    private static final int[] SEEDS = {
        0, 1, -1, 1013904223, -1013904223, Integer.MIN_VALUE, Integer.MAX_VALUE, 12345
    };

    /**
     * `(x, y, z, mask)` cases.
     *
     * The first is `tickChunk`'s own block pass at the origin section. The
     * negatives matter: a chunk at negative x has a negative `getMinBlockX`,
     * and the addition is plain integer addition on a value the mask has
     * already made non-negative — so nothing about the sign is subtle, and
     * that is exactly why it is pinned rather than assumed.
     *
     * `mask` is 15 everywhere `tickChunk` calls this, and both other values
     * are here because the argument exists: a port that hard-codes 15 for the
     * y component passes every case with mask 15 and is wrong the moment
     * anything else calls it.
     */
    private static final int[][] CASES = {
        {0, 0, 0, 15},
        {16, 64, 16, 15},
        {-32, -64, -48, 15},
        {0, 0, 0, 0},
        {0, 0, 0, 255},
        {-2368, -64, 0, 15},
    };

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Field randValue = Level.class.getDeclaredField("randValue");
        randValue.setAccessible(true);

        sun.misc.Unsafe unsafe = unsafe();
        Level level = (Level) unsafe.allocateInstance(
            net.minecraft.server.level.ServerLevel.class);

        List<String> cases = new ArrayList<>();
        for (int[] c : CASES) {
            for (int seed : SEEDS) {
                randValue.setInt(level, seed);
                StringBuilder positions = new StringBuilder();
                for (int i = 0; i < DRAWS; i++) {
                    BlockPos p = level.getBlockRandomPos(c[0], c[1], c[2], c[3]);
                    positions.append(i == 0 ? "" : ", ")
                        .append("[").append(p.getX())
                        .append(", ").append(p.getY())
                        .append(", ").append(p.getZ()).append("]");
                }
                cases.add("{\"x\": " + c[0]
                    + ", \"y\": " + c[1]
                    + ", \"z\": " + c[2]
                    + ", \"mask\": " + c[3]
                    + ", \"seed\": " + seed
                    // The value the field is left holding after the draws
                    // above. An implementation can agree on every position
                    // and still be advancing a different amount per call —
                    // it would show up on the twenty-fifth draw and not the
                    // twenty-fourth, so the state is compared as well as the
                    // output.
                    + ", \"endValue\": " + randValue.getInt(level)
                    + ", \"positions\": [" + positions + "]}");
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"random_tick_pos\",\n");
        out.append("  \"source\": \"net.minecraft.world.level.Level.getBlockRandomPos\",\n");
        out.append("  \"draws\": ").append(DRAWS).append(",\n");
        out.append("  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i)).append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static sun.misc.Unsafe unsafe() throws Exception {
        Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        f.setAccessible(true);
        return (sun.misc.Unsafe) f.get(null);
    }

    private RandomTickGolden() {}
}
