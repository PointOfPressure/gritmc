// GaussianGolden — `RandomSource.nextGaussian`, from the JVM itself.
//
// Every other RNG primitive this port uses consumes a fixed number of bits.
// `nextGaussian` does not: it is Marsaglia polar, so it takes two
// `nextDouble`s per attempt and *retries* until the point lands inside the
// unit circle — about 21% of attempts fail — and then caches the second value
// of the accepted pair for the following call. So the first gaussian on a
// stream costs two-or-more draws and the second costs none, and both halves
// of that are behaviour.
//
// It reaches worldgen through `Mth.normal` -> `ClampedNormalFloat.sample` ->
// `SpeleothemClusterFeature.randomBetweenBiased`, which is called once per
// speleothem in every cluster in the world. A port that got the retry loop or
// the cache wrong would desynchronise the feature stream from the first
// cluster onward while still producing a plausible-looking normal
// distribution — which is exactly the kind of fault a statistical check
// passes and a golden vector catches.
//
// Three things are dumped, because they fail independently:
//
//   `sequence`    — N consecutive gaussians, as raw bits.
//   `interleaved` — a scripted mix of gaussian/long/double calls, recording
//                   every output. This is what pins the *stream position*
//                   after a gaussian, which the sequence alone cannot: a port
//                   that drew one pair too many would still produce a
//                   self-consistent sequence of its own.
//   `reseeded`    — a gaussian, then `setSeed`, then another. `setSeed` must
//                   clear the cached second value; a port that kept it would
//                   return a number belonging to the previous seed.
//
// Converted to JSON by `ci/gaussian-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.util.RandomSource;

public final class GaussianGolden {

    /// Long enough that the rejection branch is taken many times: at ~21%
    /// rejection, 256 gaussians take roughly 320 attempts.
    private static final int SEQUENCE_LEN = 256;

    private static final long[] SEEDS = {0L, 1L, -1L, 123456789L, 20260708L};

    public static void main(String[] args) {
        List<String> cases = new ArrayList<>();
        for (long seed : SEEDS) {
            cases.add(sequence("legacy_lcg", seed, new LegacyRandomSource(seed)));
            cases.add(sequence("xoroshiro128pp", seed, new XoroshiroRandomSource(seed)));
            cases.add(interleaved("legacy_lcg", seed, new LegacyRandomSource(seed)));
            cases.add(interleaved("xoroshiro128pp", seed, new XoroshiroRandomSource(seed)));
            cases.add(reseeded("legacy_lcg", seed, new LegacyRandomSource(seed)));
            cases.add(reseeded("xoroshiro128pp", seed, new XoroshiroRandomSource(seed)));
            cases.add(longRun("legacy_lcg", seed, new LegacyRandomSource(seed)));
            cases.add(longRun("xoroshiro128pp", seed, new XoroshiroRandomSource(seed)));
            cases.add(worldgenReseeded("legacy_lcg", seed));
            cases.add(worldgenReseeded("xoroshiro128pp", seed));
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"gaussian\",\n  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i)).append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// A long run, sampled — because the verbatim sequences above are too
    /// short to see what this is for.
    ///
    /// The failure that matters to worldgen is a **desynchronised stream**: a
    /// gaussian that consumes or caches the wrong number of `nextDouble`s
    /// shifts every draw after it, and every feature placed from that point
    /// lands somewhere else. The 30 sequence cases hold ~7,680 gaussians
    /// between them, which is a small window to catch a rejection-loop bug
    /// that only bites on an unusual pair.
    ///
    /// So: a million gaussians per seed, recording every `LONG_RUN_STRIDE`-th
    /// one. A desync anywhere shows up at the next sample and every sample
    /// after it, while the file stays a thousand values per case instead of a
    /// million.
    ///
    /// Deliberately **not** a fold or a checksum. `Math.log` is not correctly
    /// rounded and this port's `precise_log` disagrees with the JVM on
    /// roughly 4 inputs in 300,000, so a bit-exact digest over a million
    /// values would be red by construction and would say nothing about
    /// *where* or *how badly*. Sampled values keep the existing 1-ulp
    /// tolerance meaningful and still pin the position.
    private static final int LONG_RUN_LEN = 1_000_000;
    private static final int LONG_RUN_STRIDE = 1_000;

    private static String longRun(String rng, long seed, RandomSource source) {
        List<String> bits = new ArrayList<>(LONG_RUN_LEN / LONG_RUN_STRIDE);
        for (int i = 0; i < LONG_RUN_LEN; i++) {
            double g = source.nextGaussian();
            if ((i + 1) % LONG_RUN_STRIDE == 0) {
                bits.add(quote(Double.doubleToRawLongBits(g)));
            }
        }
        return "{\"case\": \"long_run\", \"rng\": \"" + rng + "\", \"seed\": \"" + seed
                + "\", \"length\": " + LONG_RUN_LEN + ", \"stride\": " + LONG_RUN_STRIDE
                + ", \"sampled_bits\": [" + String.join(", ", bits) + "]}";
    }

    private static String sequence(String rng, long seed, RandomSource source) {
        List<String> bits = new ArrayList<>(SEQUENCE_LEN);
        for (int i = 0; i < SEQUENCE_LEN; i++) {
            bits.add(quote(Double.doubleToRawLongBits(source.nextGaussian())));
        }
        // Where the stream ended up. A port whose retry loop ran a different
        // number of times could still match every value above by accident of
        // the cache; it cannot also match this.
        String after = quote(source.nextLong());
        return "{\"case\": \"sequence\", \"rng\": " + quote(rng)
             + ", \"seed\": " + quote(seed)
             + ", \"output_bits\": " + array(bits)
             + ", \"next_long_after\": " + after + "}";
    }

    // A scripted mix. `g` is a gaussian, `l` a long, `d` a double — chosen so
    // that a gaussian is followed by a plain draw both when its cache is full
    // (after an odd-numbered call) and when it is empty.
    private static final String SCRIPT = "gldggllddgggdlg";

    private static String interleaved(String rng, long seed, RandomSource source) {
        List<String> kinds = new ArrayList<>();
        List<String> bits = new ArrayList<>();
        for (int i = 0; i < SCRIPT.length(); i++) {
            char c = SCRIPT.charAt(i);
            kinds.add(quote(String.valueOf(c)));
            switch (c) {
                case 'g' -> bits.add(quote(Double.doubleToRawLongBits(source.nextGaussian())));
                case 'l' -> bits.add(quote(source.nextLong()));
                case 'd' -> bits.add(quote(Double.doubleToRawLongBits(source.nextDouble())));
                default -> throw new IllegalStateException("bad script char " + c);
            }
        }
        return "{\"case\": \"interleaved\", \"rng\": " + quote(rng)
             + ", \"seed\": " + quote(seed)
             + ", \"script\": " + quote(SCRIPT)
             + ", \"kinds\": " + array(kinds)
             + ", \"output_bits\": " + array(bits) + "}";
    }

    private static String reseeded(String rng, long seed, RandomSource source) {
        // One gaussian leaves a cached value behind.
        long first = Double.doubleToRawLongBits(source.nextGaussian());
        source.setSeed(seed);
        // If the cache survived `setSeed`, this returns the *other* half of
        // the first pair rather than a fresh draw — and the two would be
        // related in a way no distribution check would notice.
        long afterReseed = Double.doubleToRawLongBits(source.nextGaussian());
        // The same seed from scratch: `afterReseed` must equal this.
        RandomSource fresh = rng.equals("legacy_lcg")
            ? new LegacyRandomSource(seed)
            : new XoroshiroRandomSource(seed);
        long fromScratch = Double.doubleToRawLongBits(fresh.nextGaussian());
        return "{\"case\": \"reseeded\", \"rng\": " + quote(rng)
             + ", \"seed\": " + quote(seed)
             + ", \"first_bits\": " + quote(first)
             + ", \"after_reseed_bits\": " + quote(afterReseed)
             + ", \"from_scratch_bits\": " + quote(fromScratch) + "}";
    }

    /// `WorldgenRandom.setSeed` and the gaussian cache — the opposite of
    /// `reseeded`, and it is the opposite on purpose.
    ///
    /// `WorldgenRandom extends LegacyRandomSource`, whose `setSeed` resets
    /// the `MarsagliaPolarGaussian`. But `WorldgenRandom` **overrides**
    /// `setSeed`, and the override's entire body is
    /// `this.randomSource.setSeed(seed)` — it never calls `super`, so the
    /// gaussian source it *also* inherits `nextGaussian` from is never
    /// reset. A cached half therefore survives `setFeatureSeed` and is
    /// handed to the next feature as its first gaussian.
    ///
    /// This is dumped rather than asserted from a reading, because it is
    /// precisely the behaviour a careful port assumes is not there: the
    /// parent class resets, the docs say nothing, and the difference is
    /// invisible to any call-count or distribution check.
    private static String worldgenReseeded(String rng, long seed) {
        WorldgenRandom source = new WorldgenRandom(backing(rng, seed));
        long first = Double.doubleToRawLongBits(source.nextGaussian());
        source.setSeed(seed);
        // If the cache survives — and it does — this is `first`'s partner,
        // not a fresh draw for `seed`.
        long afterReseed = Double.doubleToRawLongBits(source.nextGaussian());
        long fromScratch = Double.doubleToRawLongBits(
            new WorldgenRandom(backing(rng, seed)).nextGaussian());
        return "{\"case\": \"worldgen_reseeded\", \"rng\": " + quote(rng)
             + ", \"seed\": " + quote(seed)
             + ", \"first_bits\": " + quote(first)
             + ", \"after_reseed_bits\": " + quote(afterReseed)
             + ", \"from_scratch_bits\": " + quote(fromScratch) + "}";
    }

    private static RandomSource backing(String rng, long seed) {
        return rng.equals("legacy_lcg")
            ? new LegacyRandomSource(seed)
            : new XoroshiroRandomSource(seed);
    }

    private static String quote(long v) {
        return "\"" + v + "\"";
    }

    private static String quote(String v) {
        return "\"" + v + "\"";
    }

    private static String array(List<String> items) {
        return "[" + String.join(", ", items) + "]";
    }
}
