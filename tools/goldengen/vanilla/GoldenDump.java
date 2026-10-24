// GoldenDump — emits RNG golden vectors from the REAL vanilla server jar.
//
// ORACLE.md §3 (from-scratch amendment): ground truth is the vanilla jar and
// JVM dumps taken from it, never another project's port. This file exists
// because the difference is not theoretical — the P0-5 vectors were exported
// from Minecom, and Minecom's `Mth.getSeed` equivalent widens `x` to 64 bits
// before multiplying where vanilla multiplies in 32 (verified against
// `javap -c net.minecraft.util.Mth`: `ldc 3129871; imul; i2l`). The two agree
// for |x| < ~686 and diverge everywhere beyond, so a spawn-centred diff
// cannot see it.
//
// Run via `cargo run -p goldengen -- <outdir>`, which compiles this against
// the pinned server jar. Regeneration is deliberate (version bump), never
// part of a normal build.
//
// Schema (unchanged from the P0-5 export, so consumers keep working):
//   - i64 values are JSON *strings*; a JSON number cannot hold the range.
//   - floats/doubles are raw IEEE-754 bit patterns in `output_bits`, with a
//     human-readable `repr` alongside. Compare the bits, never the repr.

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.data.worldgen.TerrainProvider;
import net.minecraft.world.level.levelgen.Noises;
import net.minecraft.resources.ResourceKey;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderGetter;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.CubicSpline;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.PositionalRandomFactory;
import net.minecraft.world.level.levelgen.RandomSupport;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.DensityFunctions;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.XoroshiroRandomSource;
import net.minecraft.world.level.levelgen.synth.BlendedNoise;
import net.minecraft.world.level.levelgen.synth.ImprovedNoise;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.levelgen.synth.PerlinNoise;

public final class GoldenDump {

    private static final int STREAM_LEN = 10_000;
    private static final int DERIVED_LEN = 100;
    private static final long DERIVED_SEED = 20260708L;

    private static final long[] STREAM_SEEDS = {0L, 1L, -1L, 123456789L, 20260708L};
    private static final int[][] POSITIONS = {
        {0, 0, 0}, {1, 2, 3}, {-1, -64, 1}, {123456, 80, -654321}, {-30000000, -64, 30000000}
    };
    private static final String[] HASH_NAMES = {
        "minecraft:offset", "minecraft:temperature", "minecraft:vegetation",
        "minecraft:ore_veininess", "octave_-4", "octave_0"
    };
    private static final long[] FROM_SEEDS = {0L, 12345L, -9876543210L};
    private static final int[] BOUNDS = {1, 2, 3, 16, 17, 100, Integer.MAX_VALUE};

    public static void main(String[] args) throws IOException {
        if (args.length != 1) {
            System.err.println("usage: GoldenDump <outdir>");
            System.exit(2);
        }
        Path out = Paths.get(args[0]);
        Files.createDirectories(out);

        // `DensityFunction` and `BlendedNoise` carry registry-backed codecs,
        // and `BuiltInRegistries`' static init refuses to run until vanilla's
        // bootstrap has. Without these two lines the dump dies in
        // `Bootstrap.checkBootstrapCalled` before writing anything. The pure
        // RNG cases don't need it; it is cheap enough to always do.
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        for (long seed : STREAM_SEEDS) {
            String suffix = seed < 0 ? "m" + (-seed) : Long.toString(seed);
            write(out.resolve("xoroshiro128pp_seed_" + suffix + ".json"),
                  stream("xoroshiro128pp", seed, new XoroshiroRandomSource(seed)));
            write(out.resolve("legacy_lcg_seed_" + suffix + ".json"),
                  stream("legacy_lcg", seed, new LegacyRandomSource(seed)));
        }

        write(out.resolve("xoroshiro128pp_derived.json"), xoroshiroDerived());
        write(out.resolve("legacy_lcg_derived.json"), legacyDerived());
        write(out.resolve("xwr_derived.json"), xwrDerived());
        write(out.resolve("noise_vectors.json"), noiseVectors());
        write(out.resolve("density_vectors.json"), densityVectors());
        write(out.resolve("named_noise_vectors.json"), namedNoiseVectors());
        write(out.resolve("router_vectors.json"), routerVectors());
        write(out.resolve("interpolated_density_vectors.json"), interpolatedDensityVectors());
        write(out.resolve("aquifer_centre_vectors.json"), aquiferCentreVectors());
        write(out.resolve("interpolator_corner_vectors.json"), interpolatorCornerVectors());
        write(out.resolve("noise_chunk_vectors.json"), noiseChunkVectors());
        write(out.resolve("climate_vectors.json"), climateVectors());

        System.out.println("goldengen: wrote " + (STREAM_SEEDS.length * 2 + 11) + " files to " + out);
    }

    // ---------------------------------------------------------------- streams

    private static String stream(String name, long seed, RandomSource rng) {
        List<String> outputs = new ArrayList<>(STREAM_LEN);
        for (int i = 0; i < STREAM_LEN; i++) {
            outputs.add(quote(rng.nextLong()));
        }
        return "{\n  \"rng\": " + quote(name)
             + ",\n  \"seed\": " + quote(seed)
             + ",\n  \"count\": " + STREAM_LEN
             + ",\n  \"outputs\": " + array(outputs)
             + "\n}\n";
    }

    // ---------------------------------------------------------------- derived

    private static String xoroshiroDerived() {
        List<String> cases = new ArrayList<>();

        // mixStafford13 — pure input -> output.
        long[] mixInputs = {0L, 1L, -1L, 20260708L, 81985529216486895L, Long.MIN_VALUE, Long.MAX_VALUE};
        List<String> mixIn = new ArrayList<>();
        List<String> mixOut = new ArrayList<>();
        for (long v : mixInputs) {
            mixIn.add(quote(v));
            mixOut.add(quote(RandomSupport.mixStafford13(v)));
        }
        cases.add("{\"case\": \"mixStafford13\", \"inputs\": " + array(mixIn)
                + ", \"outputs\": " + array(mixOut) + "}");

        // blockSeed (Mth.getSeed) — THE case that caught the upstream bug.
        List<String> posIn = new ArrayList<>();
        List<String> posOut = new ArrayList<>();
        for (int[] p : POSITIONS) {
            posIn.add("[" + p[0] + ", " + p[1] + ", " + p[2] + "]");
            posOut.add(quote(Mth.getSeed(p[0], p[1], p[2])));
        }
        cases.add("{\"case\": \"blockSeed\", \"inputs\": " + array(posIn)
                + ", \"outputs\": " + array(posOut) + "}");

        // fork
        cases.add("{\"case\": \"fork\", \"seed\": " + quote(DERIVED_SEED)
                + ", \"outputs\": " + array(longs(new XoroshiroRandomSource(DERIVED_SEED).fork())) + "}");

        // forkPositional.at / .fromHashOf / .fromSeed — each from a fresh parent.
        for (int[] p : POSITIONS) {
            RandomSource child = positional().at(p[0], p[1], p[2]);
            cases.add("{\"case\": \"forkPositional.at\", \"seed\": " + quote(DERIVED_SEED)
                    + ", \"x\": " + p[0] + ", \"y\": " + p[1] + ", \"z\": " + p[2]
                    + ", \"outputs\": " + array(longs(child)) + "}");
        }
        for (String name : HASH_NAMES) {
            RandomSource child = positional().fromHashOf(name);
            cases.add("{\"case\": \"forkPositional.fromHashOf\", \"seed\": " + quote(DERIVED_SEED)
                    + ", \"name\": " + quote(name)
                    + ", \"outputs\": " + array(longs(child)) + "}");
        }
        for (long s : FROM_SEEDS) {
            RandomSource child = positional().fromSeed(s);
            cases.add("{\"case\": \"forkPositional.fromSeed\", \"seed\": " + quote(DERIVED_SEED)
                    + ", \"fromSeed\": " + quote(s)
                    + ", \"outputs\": " + array(longs(child)) + "}");
        }

        cases.addAll(commonOps(() -> new XoroshiroRandomSource(DERIVED_SEED), true));
        return cases("xoroshiro128pp_derived", cases);
    }

    private static String legacyDerived() {
        List<String> cases = new ArrayList<>();

        // setLargeFeatureSeed takes CHUNK coordinates.
        int[][] largeFeature = {{0, 0}, {-13, 27}, {100, -100}};
        long[] largeFeatureSeeds = {20260708L, 20260708L, 1L};
        for (int i = 0; i < largeFeature.length; i++) {
            WorldgenRandom rng = legacyWorldgen();
            rng.setLargeFeatureSeed(largeFeatureSeeds[i], largeFeature[i][0], largeFeature[i][1]);
            cases.add("{\"case\": \"setLargeFeatureSeed\", \"worldSeed\": " + quote(largeFeatureSeeds[i])
                    + ", \"chunkX\": " + largeFeature[i][0] + ", \"chunkZ\": " + largeFeature[i][1]
                    + ", \"outputs\": " + array(longs(rng)) + "}");
        }

        // setDecorationSeed takes the chunk's MINIMUM BLOCK coordinates.
        int[][] decoration = {{0, 0}, {-208, 432}, {16, -16}};
        long[] decorationSeeds = {20260708L, 20260708L, 1L};
        for (int i = 0; i < decoration.length; i++) {
            WorldgenRandom rng = legacyWorldgen();
            long derived = rng.setDecorationSeed(decorationSeeds[i], decoration[i][0], decoration[i][1]);
            cases.add("{\"case\": \"setDecorationSeed\", \"worldSeed\": " + quote(decorationSeeds[i])
                    + ", \"minBlockX\": " + decoration[i][0] + ", \"minBlockZ\": " + decoration[i][1]
                    + ", \"decorationSeed\": " + quote(derived)
                    + ", \"outputs\": " + array(longs(rng)) + "}");
        }

        cases.addAll(featureSeedCases(GoldenDump::legacyWorldgen));

        int[][] salted = {{10, -20, 10387312}, {0, 0, 14357617}};
        for (int[] s : salted) {
            WorldgenRandom rng = legacyWorldgen();
            rng.setLargeFeatureWithSalt(20260708L, s[0], s[1], s[2]);
            cases.add("{\"case\": \"setLargeFeatureWithSalt\", \"worldSeed\": " + quote(20260708L)
                    + ", \"x\": " + s[0] + ", \"z\": " + s[1] + ", \"salt\": " + s[2]
                    + ", \"outputs\": " + array(longs(rng)) + "}");
        }

        // No nextIntBetweenInclusive here — matches the original export.
        cases.addAll(commonOps(() -> new LegacyRandomSource(DERIVED_SEED), false));
        return cases("legacy_lcg_derived", cases);
    }

    private static String xwrDerived() {
        List<String> cases = new ArrayList<>();

        int[][] decoration = {{0, 0}, {-208, 432}, {16, -16}};
        long[] decorationSeeds = {20260708L, 20260708L, 1L};
        for (int i = 0; i < decoration.length; i++) {
            WorldgenRandom rng = xoroshiroWorldgen();
            long derived = rng.setDecorationSeed(decorationSeeds[i], decoration[i][0], decoration[i][1]);
            cases.add("{\"case\": \"setDecorationSeed\", \"worldSeed\": " + quote(decorationSeeds[i])
                    + ", \"minBlockX\": " + decoration[i][0] + ", \"minBlockZ\": " + decoration[i][1]
                    + ", \"decorationSeed\": " + quote(derived)
                    + ", \"outputs\": " + array(longs(rng)) + "}");
        }

        cases.addAll(featureSeedCases(GoldenDump::xoroshiroWorldgen));
        cases.addAll(commonOps(GoldenDump::xoroshiroWorldgenSeeded, true));
        return cases("xoroshiro_worldgen_random_derived", cases);
    }

    private static List<String> featureSeedCases(java.util.function.Supplier<WorldgenRandom> factory) {
        List<String> cases = new ArrayList<>();
        long[] seeds = {123456789L, 123456789L, -987654321L};
        int[][] indexStep = {{0, 0}, {5, 3}, {14, 9}};
        for (int i = 0; i < seeds.length; i++) {
            WorldgenRandom rng = factory.get();
            rng.setFeatureSeed(seeds[i], indexStep[i][0], indexStep[i][1]);
            cases.add("{\"case\": \"setFeatureSeed\", \"decorationSeed\": " + quote(seeds[i])
                    + ", \"index\": " + indexStep[i][0] + ", \"step\": " + indexStep[i][1]
                    + ", \"outputs\": " + array(longs(rng)) + "}");
        }
        return cases;
    }

    /// Per-op vectors, each from a fresh instance.
    private static List<String> commonOps(java.util.function.Supplier<RandomSource> factory,
                                          boolean withBetweenInclusive) {
        List<String> cases = new ArrayList<>();

        for (int bound : BOUNDS) {
            RandomSource rng = factory.get();
            List<String> outputs = new ArrayList<>();
            for (int i = 0; i < DERIVED_LEN; i++) {
                outputs.add(quote(rng.nextInt(bound)));
            }
            cases.add("{\"case\": \"nextInt.bounded\", \"seed\": " + quote(DERIVED_SEED)
                    + ", \"bound\": " + bound + ", \"outputs\": " + array(outputs) + "}");
        }

        RandomSource doubles = factory.get();
        List<String> dBits = new ArrayList<>();
        List<String> dRepr = new ArrayList<>();
        for (int i = 0; i < DERIVED_LEN; i++) {
            double v = doubles.nextDouble();
            dBits.add(quote(Double.doubleToRawLongBits(v)));
            dRepr.add(Double.toString(v));
        }
        cases.add("{\"case\": \"nextDouble\", \"seed\": " + quote(DERIVED_SEED)
                + ", \"output_bits\": " + array(dBits) + ", \"repr\": " + array(dRepr) + "}");

        RandomSource floats = factory.get();
        List<String> fBits = new ArrayList<>();
        List<String> fRepr = new ArrayList<>();
        for (int i = 0; i < DERIVED_LEN; i++) {
            float v = floats.nextFloat();
            fBits.add(quote(Float.floatToRawIntBits(v)));
            fRepr.add(Float.toString(v));
        }
        cases.add("{\"case\": \"nextFloat\", \"seed\": " + quote(DERIVED_SEED)
                + ", \"output_bits\": " + array(fBits) + ", \"repr\": " + array(fRepr) + "}");

        RandomSource bools = factory.get();
        List<String> bOut = new ArrayList<>();
        for (int i = 0; i < DERIVED_LEN; i++) {
            bOut.add(Boolean.toString(bools.nextBoolean()));
        }
        cases.add("{\"case\": \"nextBoolean\", \"seed\": " + quote(DERIVED_SEED)
                + ", \"outputs\": " + array(bOut) + "}");

        if (withBetweenInclusive) {
            RandomSource between = factory.get();
            List<String> outputs = new ArrayList<>();
            for (int i = 0; i < DERIVED_LEN; i++) {
                outputs.add(quote(between.nextIntBetweenInclusive(-5, 10)));
            }
            cases.add("{\"case\": \"nextIntBetweenInclusive\", \"seed\": " + quote(DERIVED_SEED)
                    + ", \"min\": -5, \"max\": 10, \"outputs\": " + array(outputs) + "}");
        }

        return cases;
    }

    // ----------------------------------------------------------------- noise

    /// Sample points. Chosen to exercise the parts that silently differ
    /// between a plausible port and a correct one: negative coordinates
    /// (floor vs truncate), fractional and near-integer values (the
    /// smoothstep endpoints), deep-negative y (the -64 world floor), and a
    /// coordinate past the PerlinNoise wrap lattice.
    private static final double[][] SAMPLE_POINTS = {
        {0.0, 0.0, 0.0},
        {0.5, 0.5, 0.5},
        {1.0, 1.0, 1.0},
        {-1.0, -1.0, -1.0},
        {0.25, -0.75, 12.5},
        {123.456, 64.0, -789.012},
        {-1523.75, -64.0, 2048.125},
        {1000000.5, 320.0, -1000000.5},
        {5.0e7, 0.0, -5.0e7},
        {0.999999, 0.000001, 63.5},
    };

    /// Noise configurations, mirroring the shapes vanilla actually uses.
    private static final int[] NOISE_FIRST_OCTAVES = {-7, -3, -10, 0};
    private static final double[][] NOISE_AMPLITUDES = {
        {1.0, 1.0},
        {1.0},
        // Interior zeros: exercises skipped octaves and the non-zero span
        // used for the value factor.
        {1.5, 0.0, 1.0, 0.0, 0.0, 0.0},
        {1.0, 1.0, 1.0},
    };

    private static String noiseVectors() {
        List<String> cases = new ArrayList<>();

        // --- ImprovedNoise: one octave, straight off a seeded source.
        for (long seed : new long[] {0L, 1L, 20260708L}) {
            ImprovedNoise noise = new ImprovedNoise(new XoroshiroRandomSource(seed));
            List<String> bits = new ArrayList<>();
            List<String> repr = new ArrayList<>();
            for (double[] p : SAMPLE_POINTS) {
                double v = noise.noise(p[0], p[1], p[2]);
                bits.add(quote(Double.doubleToRawLongBits(v)));
                repr.add(Double.toString(v));
            }
            cases.add("{\"case\": \"ImprovedNoise.noise\", \"seed\": " + quote(seed)
                    + ", \"xo_bits\": " + quote(Double.doubleToRawLongBits(noise.xo))
                    + ", \"yo_bits\": " + quote(Double.doubleToRawLongBits(noise.yo))
                    + ", \"zo_bits\": " + quote(Double.doubleToRawLongBits(noise.zo))
                    + ", \"points\": " + points()
                    + ", \"output_bits\": " + array(bits)
                    + ", \"repr\": " + array(repr) + "}");
        }

        // --- ImprovedNoise with the deprecated y-fudge overload.
        {
            ImprovedNoise noise = new ImprovedNoise(new XoroshiroRandomSource(20260708L));
            double[][] fudges = {{0.0, 0.0}, {1.0, 0.5}, {0.25, -1.0}, {4.0, 2.0}};
            for (double[] f : fudges) {
                List<String> bits = new ArrayList<>();
                for (double[] p : SAMPLE_POINTS) {
                    bits.add(quote(Double.doubleToRawLongBits(
                        noise.noise(p[0], p[1], p[2], f[0], f[1]))));
                }
                cases.add("{\"case\": \"ImprovedNoise.noiseWithFudge\", \"seed\": "
                        + quote(20260708L)
                        + ", \"yScale_bits\": " + quote(Double.doubleToRawLongBits(f[0]))
                        + ", \"yFudge_bits\": " + quote(Double.doubleToRawLongBits(f[1]))
                        + ", \"points\": " + points()
                        + ", \"output_bits\": " + array(bits) + "}");
            }
        }

        // --- PerlinNoise and NormalNoise across the configurations above.
        for (int c = 0; c < NOISE_FIRST_OCTAVES.length; c++) {
            int firstOctave = NOISE_FIRST_OCTAVES[c];
            double[] amps = NOISE_AMPLITUDES[c];

            PerlinNoise perlin = PerlinNoise.create(
                new XoroshiroRandomSource(20260708L), firstOctave, amps[0], tail(amps));
            List<String> perlinBits = new ArrayList<>();
            for (double[] p : SAMPLE_POINTS) {
                perlinBits.add(quote(Double.doubleToRawLongBits(
                    perlin.getValue(p[0], p[1], p[2]))));
            }
            cases.add("{\"case\": \"PerlinNoise.getValue\", \"seed\": " + quote(20260708L)
                    + ", \"firstOctave\": " + firstOctave
                    + ", \"amplitudes\": " + doubles(amps)
                    + ", \"points\": " + points()
                    + ", \"output_bits\": " + array(perlinBits) + "}");

            NormalNoise normal = NormalNoise.create(
                new XoroshiroRandomSource(20260708L), firstOctave, amps);
            List<String> normalBits = new ArrayList<>();
            for (double[] p : SAMPLE_POINTS) {
                normalBits.add(quote(Double.doubleToRawLongBits(
                    normal.getValue(p[0], p[1], p[2]))));
            }
            cases.add("{\"case\": \"NormalNoise.getValue\", \"seed\": " + quote(20260708L)
                    + ", \"firstOctave\": " + firstOctave
                    + ", \"amplitudes\": " + doubles(amps)
                    + ", \"maxValue_bits\": " + quote(Double.doubleToRawLongBits(normal.maxValue()))
                    + ", \"points\": " + points()
                    + ", \"output_bits\": " + array(normalBits) + "}");
        }

        // --- BlendedNoise: exercises the legacy PerlinNoise path (sequential
        // octaves + skipOctave) that the modern `create` never touches.
        double[][] blendedConfigs = {
            // xzScale, yScale, xzFactor, yFactor, smearScaleMultiplier —
            // the first row is vanilla's overworld configuration.
            {1.0, 1.0, 80.0, 160.0, 8.0},
            {0.25, 0.125, 40.0, 80.0, 4.0},
            {1.0, 3.0, 100.0, 50.0, 2.0},
        };
        int[][] blockPoints = {
            {0, 0, 0},
            {4, 64, 4},
            {-100, -64, 100},
            {1523, 128, -2048},
            {100000, 320, -100000},
        };
        for (double[] cfg : blendedConfigs) {
            BlendedNoise blended = new BlendedNoise(
                new XoroshiroRandomSource(20260708L), cfg[0], cfg[1], cfg[2], cfg[3], cfg[4]);
            List<String> bits = new ArrayList<>();
            List<String> pts = new ArrayList<>();
            for (int[] p : blockPoints) {
                double v = blended.compute(
                    new DensityFunction.SinglePointContext(p[0], p[1], p[2]));
                bits.add(quote(Double.doubleToRawLongBits(v)));
                pts.add("[" + p[0] + ", " + p[1] + ", " + p[2] + "]");
            }
            cases.add("{\"case\": \"BlendedNoise.compute\", \"seed\": " + quote(20260708L)
                    + ", \"xzScale_bits\": " + quote(Double.doubleToRawLongBits(cfg[0]))
                    + ", \"yScale_bits\": " + quote(Double.doubleToRawLongBits(cfg[1]))
                    + ", \"xzFactor_bits\": " + quote(Double.doubleToRawLongBits(cfg[2]))
                    + ", \"yFactor_bits\": " + quote(Double.doubleToRawLongBits(cfg[3]))
                    + ", \"smear_bits\": " + quote(Double.doubleToRawLongBits(cfg[4]))
                    + ", \"maxValue_bits\": " + quote(Double.doubleToRawLongBits(blended.maxValue()))
                    + ", \"blockPoints\": " + array(pts)
                    + ", \"output_bits\": " + array(bits) + "}");
        }

        // SimplexNoise — 2D and 3D, from a seeded source.
        for (long seed : new long[] {0L, 20260708L}) {
            var simplex = new net.minecraft.world.level.levelgen.synth.SimplexNoise(
                new XoroshiroRandomSource(seed));
            List<String> bits2 = new ArrayList<>();
            List<String> bits3 = new ArrayList<>();
            List<String> pts = new ArrayList<>();
            for (double[] q : SAMPLE_POINTS) {
                bits2.add(quote(Double.doubleToRawLongBits(
                    simplex.getValue(q[0], q[2]))));
                bits3.add(quote(Double.doubleToRawLongBits(
                    simplex.getValue(q[0], q[1], q[2]))));
                pts.add("[" + quote(Double.doubleToRawLongBits(q[0]))
                      + ", " + quote(Double.doubleToRawLongBits(q[1]))
                      + ", " + quote(Double.doubleToRawLongBits(q[2])) + "]");
            }
            cases.add("{\"case\": \"SimplexNoise.2d\", \"seed\": " + quote(seed)
                    + ", \"points\": " + array(pts)
                    + ", \"output_bits\": " + array(bits2) + "}");
            cases.add("{\"case\": \"SimplexNoise.3d\", \"seed\": " + quote(seed)
                    + ", \"points\": " + array(pts)
                    + ", \"output_bits\": " + array(bits3) + "}");
        }

        return cases("noise", cases);
    }

    // ---------------------------------------------------------- named noise

    /// The world-seed -> named-noise chain, via vanilla's own registry so the
    /// parameters are the real ones rather than transcribed by hand. The
    /// parameters are emitted alongside the samples so the Rust side can
    /// rebuild the noise without needing the (gitignored) extracted data.
    private static String namedNoiseVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderLookup.RegistryLookup<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);

        // Every noise in the registry, not a chosen six.
        //
        // The six were the ones the density router reads, and that looked
        // like the interesting set until a surface rule named
        // `sulfur_cave_gradient` — a noise nothing here had ever sampled — and
        // the port produced cinnabar through solid deepslate. Any noise a
        // datapack can name is a noise this project has to get right, and the
        // registry is the only honest definition of "all of them".
        List<ResourceKey<NormalNoise.NoiseParameters>> keys = new ArrayList<>();
        noises.listElementIds().sorted(java.util.Comparator.comparing(k -> k.identifier().toString()))
            .forEach(keys::add);
        long[] seeds = {0L, 20260708L, -1L};

        List<String> cases = new ArrayList<>();
        for (long seed : seeds) {
            PositionalRandomFactory random = new XoroshiroRandomSource(seed).forkPositional();
            for (ResourceKey<NormalNoise.NoiseParameters> key : keys) {
                NormalNoise noise = Noises.instantiate(noises, random, key);
                NormalNoise.NoiseParameters p = noises.getOrThrow(key).value();

                List<String> amps = new ArrayList<>();
                for (int i = 0; i < p.amplitudes().size(); i++) {
                    amps.add(quote(Double.doubleToRawLongBits(p.amplitudes().getDouble(i))));
                }
                List<String> bits = new ArrayList<>();
                for (double[] q : SAMPLE_POINTS) {
                    bits.add(quote(Double.doubleToRawLongBits(
                        noise.getValue(q[0], q[1], q[2]))));
                }
                cases.add("{\"case\": \"named_noise\", \"seed\": " + quote(seed)
                        + ", \"name\": " + quote(key.identifier().toString())
                        + ", \"firstOctave\": " + p.firstOctave()
                        + ", \"amplitudes\": " + array(amps)
                        + ", \"maxValue_bits\": " + quote(Double.doubleToRawLongBits(noise.maxValue()))
                        + ", \"points\": " + points()
                        + ", \"output_bits\": " + array(bits) + "}");
            }
        }
        return cases("named_noise", cases);
    }

    // ---------------------------------------------------------- the router

    /// The real overworld router, built by vanilla itself: VanillaRegistries
    /// resolves NoiseRouterData's density functions, RandomState seeds them
    /// from the world seed, and we sample the resulting sloped_cheese /
    /// depth / factor / continents fields.
    ///
    /// Also emits the noise parameters the router used, so the Rust side can
    /// rebuild without the (gitignored) extracted registry.
    private static String routerVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);
        HolderGetter<net.minecraft.world.level.levelgen.NoiseGeneratorSettings> settingsGetter =
            registries.lookupOrThrow(Registries.NOISE_SETTINGS);

        // Emit EVERY noise in the registry, keyed by identifier. Listing the
        // ones we happen to need would reintroduce transcription risk for no
        // benefit.
        //
        // Sorted by name, because `listElements` does not promise an order and
        // in practice does not give a stable one: regenerating produced a
        // 64-line reshuffle of identical values, which is exactly the kind of
        // noise that trains people to skim a golden diff instead of reading
        // it. The Rust side keys by name, so the order is presentation only.
        List<String> params = new ArrayList<>();
        registries.lookupOrThrow(Registries.NOISE).listElements()
            .sorted(java.util.Comparator.comparing(h -> h.key().identifier().toString()))
            .forEach(holder -> {
            NormalNoise.NoiseParameters p = holder.value();
            List<String> amps = new ArrayList<>();
            for (int a = 0; a < p.amplitudes().size(); a++) {
                amps.add(quote(Double.doubleToRawLongBits(p.amplitudes().getDouble(a))));
            }
            params.add("{\"name\": " + quote(holder.key().identifier().toString())
                    + ", \"firstOctave\": " + p.firstOctave()
                    + ", \"amplitudes\": " + array(amps) + "}");
        });

        int[][] pts = {
            {0, 0, 0}, {4, 64, 4}, {-100, -64, 100}, {137, 128, -509},
            {1523, 96, -2048}, {-2048, 200, 2048}, {100000, 320, -100000},
        };
        // 31415926 is the whole-world diff seed. Added after a packed_ice
        // failure was traced to a peaks column nine blocks lower than
        // vanilla's, at which point this gate turned out to cover only the
        // two seeds it was born with.
        long[] seeds = {0L, 20260708L, 31415926L};

        List<String> cases = new ArrayList<>();
        for (long seed : seeds) {
            for (boolean amplified : new boolean[] {false, true}) {
                var settingsKey = amplified
                    ? net.minecraft.world.level.levelgen.NoiseGeneratorSettings.AMPLIFIED
                    : net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD;
                var settings = settingsGetter.getOrThrow(settingsKey).value();
                RandomState state = RandomState.create(settings, noises, seed);
                var router = state.router();

                // NOTE: router.finalDensity() is deliberately NOT dumped.
                // It folds in the slide, caves and post-processing, which
                // are wave-3 Groups E/F. Comparing our sloped_cheese to it
                // would be comparing different quantities. Group E/F adds it
                // here when those land — that is their completion gate.
                // Every field of NoiseRouter, so nothing the aquifer, the ore
                // veinifier or the biome source reads is taken on trust. The
                // four aquifer noises and the vein trio in particular use
                // `noise(data, x)`, which is the **yScale** overload — the
                // exact shape that produced the spaghetti_2d_elevation bug.
                String[] fieldNames = {
                    "depth", "continents", "erosion", "ridges", "final_density",
                    "barrier", "fluid_level_floodedness", "fluid_level_spread", "lava",
                    "temperature", "vegetation", "preliminary_surface_level",
                    "vein_toggle", "vein_ridged", "vein_gap",
                };
                DensityFunction[] fields = {
                    router.depth(), router.continents(),
                    router.erosion(), router.ridges(), router.finalDensity(),
                    router.barrierNoise(), router.fluidLevelFloodednessNoise(),
                    router.fluidLevelSpreadNoise(), router.lavaNoise(),
                    router.temperature(), router.vegetation(),
                    router.preliminarySurfaceLevel(),
                    router.veinToggle(), router.veinRidged(), router.veinGap(),
                };
                // A dense y sweep at one column, to localise any y-banded
                // divergence that a handful of spread points would miss.
                if (!amplified) {
                    List<String> sweepPts = new ArrayList<>();
                    List<String> sweepBits = new ArrayList<>();
                    // Sweep the four cell corners of one cell column, not
                    // just (0,0): a blend reads eight corners, so a corner
                    // that is only 1 ulp off is invisible from one column.
                    // (4, -2160) is the frozen_peaks column where our noise
                    // stage emits water from y44 to y52 and vanilla emits
                    // stone. Swept at every seed; it costs four columns.
                    for (int cx : new int[] {0, 4}) {
                        for (int cz : new int[] {0, 4, -2160}) {
                            for (int y = -64; y < 320; y += 4) {
                                sweepPts.add("[" + cx + ", " + y + ", " + cz + "]");
                                sweepBits.add(quote(Double.doubleToRawLongBits(
                                    router.finalDensity().compute(
                                        new DensityFunction.SinglePointContext(cx, y, cz)))));
                            }
                        }
                    }
                    cases.add("{\"case\": \"final_density\", \"seed\": " + quote(seed)
                            + ", \"amplified\": false"
                            + ", \"blockPoints\": " + array(sweepPts)
                            + ", \"output_bits\": " + array(sweepBits) + "}");
                }

                for (int f = 0; f < fields.length; f++) {
                    List<String> bits = new ArrayList<>();
                    List<String> ptsOut = new ArrayList<>();
                    for (int[] q : pts) {
                        double v = fields[f].compute(
                            new DensityFunction.SinglePointContext(q[0], q[1], q[2]));
                        bits.add(quote(Double.doubleToRawLongBits(v)));
                        ptsOut.add("[" + q[0] + ", " + q[1] + ", " + q[2] + "]");
                    }
                    cases.add("{\"case\": " + quote(fieldNames[f])
                            + ", \"seed\": " + quote(seed)
                            + ", \"amplified\": " + amplified
                            + ", \"blockPoints\": " + array(ptsOut)
                            + ", \"output_bits\": " + array(bits) + "}");
                }
            }
        }

        StringBuilder sb = new StringBuilder("{\n  \"rng\": \"router\",\n  \"parameters\": [\n");
        for (int i = 0; i < params.size(); i++) {
            sb.append("    ").append(params.get(i));
            if (i + 1 < params.size()) sb.append(',');
            sb.append('\n');
        }
        sb.append("  ],\n  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            sb.append("    ").append(cases.get(i));
            if (i + 1 < cases.size()) sb.append(',');
            sb.append('\n');
        }
        return sb.append("  ]\n}\n").toString();
    }

    // ------------------------------------------------- interpolated density

    /// Vanilla's *interpolated* density at individual blocks, via
    /// `NoiseBasedChunkGenerator.getInterpolatedNoiseValue`. That method
    /// builds a one-cell NoiseChunk around the block and runs the real
    /// interpolation, so matching it proves the whole pipeline: router,
    /// caves, slide, post-processing AND cell interpolation.
    /// The aquifer's cell centres, three ints per grid cell.
    ///
    /// `computeSubstance` picks the four nearest of these and everything
    /// after depends on which they are. Every input to them has been checked
    /// against the port and matches, and the results still differ, so the
    /// coordinates themselves are the only way to tell which check is wrong.
    ///
    /// The cells are the twelve the block (4, 44, -2160) reads: gridX of
    /// x-5, gridY of y+1, gridZ of z-5, then dx 0..1, dy -1..1, dz 0..1.
    private static String aquiferCentreVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        var noises = registries.lookupOrThrow(Registries.NOISE);
        var settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD);

        List<String> cases = new ArrayList<>();
        for (long seed : new long[] {0L, 31415926L}) {
            RandomState state = RandomState.create(settings.value(), noises, seed);
            var factory = state.aquiferRandom();
            List<String> cells = new ArrayList<>();
            for (int gx = -1; gx <= 0; gx++) {
                for (int gy = 2; gy <= 4; gy++) {
                    for (int gz = -136; gz <= -135; gz++) {
                        RandomSource rng = factory.at(gx, gy, gz);
                        int cx = gx * 16 + rng.nextInt(10);
                        int cy = gy * 12 + rng.nextInt(9);
                        int cz = gz * 16 + rng.nextInt(10);
                        cells.add("{\"cell\": [" + gx + ", " + gy + ", " + gz
                            + "], \"centre\": [" + cx + ", " + cy + ", " + cz + "]}");
                    }
                }
            }
            cases.add("{\"case\": \"aquifer_centre\", \"seed\": " + quote(seed)
                + ", \"cells\": [" + String.join(", ", cells) + "]}");
        }
        return "{\n  \"rng\": \"aquifer_centre\",\n  \"cases\": [\n    "
            + String.join(",\n    ", cases) + "\n  ]\n}\n";
    }

    private static String interpolatedDensityVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);
        var settings = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD);
        var biome = registries
            .lookupOrThrow(Registries.BIOME)
            .getOrThrow(net.minecraft.world.level.biome.Biomes.PLAINS);
        var generator = new net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator(
            new net.minecraft.world.level.biome.FixedBiomeSource(biome), settings);

        // Dense in y so cell interiors are exercised, not just corners: the
        // overworld cell is 8 tall, so a stride of 3 lands off-corner.
        // (4, -2160) is the frozen_peaks column where the aquifer is handed
        // a density of -0.010136 and answers water, while vanilla's is
        // marginally positive and answers stone. The interpolated value is
        // what the fill passes in; finalDensity at the same point is a
        // different number and already matches.
        int[][] columns = {{0, 0}, {7, 3}, {-13, 21}, {137, -509}, {4, -2160}};
        // 31415926 is the whole-world diff seed. Added after a packed_ice
        // failure was traced to a peaks column nine blocks lower than
        // vanilla's, at which point this gate turned out to cover only the
        // two seeds it was born with.
        long[] seeds = {0L, 20260708L, 31415926L};

        List<String> cases = new ArrayList<>();
        for (long seed : seeds) {
            RandomState state = RandomState.create(settings.value(), noises, seed);
            for (int[] col : columns) {
                List<String> pts = new ArrayList<>();
                List<String> bits = new ArrayList<>();
                for (int y = -64; y < 320; y += 3) {
                    double v = generator.getInterpolatedNoiseValue(
                        state, new DensityFunction.SinglePointContext(col[0], y, col[1]));
                    pts.add("[" + col[0] + ", " + y + ", " + col[1] + "]");
                    bits.add(quote(Double.doubleToRawLongBits(v)));
                }
                cases.add("{\"case\": \"interpolated_density\", \"seed\": " + quote(seed)
                        + ", \"blockPoints\": " + array(pts)
                        + ", \"output_bits\": " + array(bits) + "}");
            }
        }
        return cases("interpolated_density", cases);
    }

    // ------------------------------------------------------------- climate

    /// The overworld biome table and a **sequence** of climate lookups.
    ///
    /// Two things are emitted, and both are needed:
    ///
    /// - The parameter table itself, since the search result depends on the
    ///   tree built from it and the tree depends on the exact list order.
    /// - A sequence of queries with vanilla's answers *in order*. The order
    ///   is not incidental: `Climate.RTree.search` seeds each search with the
    ///   previous result and prunes with a strict `>`, so an exactly-tied
    ///   lookup resolves to whatever the last one returned. Answering the
    ///   same queries in a different order can give different biomes, and a
    ///   port that treats the lookup as pure will silently disagree at box
    ///   boundaries.
    ///
    /// Queries sweep a coarse lattice over the whole climate cube rather than
    /// sampling world positions, so the table is exercised everywhere
    /// including its corners, not just where the overworld's noise happens to
    /// land.
    private static String climateVectors() {
        var preset = net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList.Preset.OVERWORLD;
        var parameters = net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList
            .knownPresets().get(preset);

        List<String> entries = new ArrayList<>();
        for (var pair : parameters.values()) {
            var p = pair.getFirst();
            entries.add("{\"biome\": " + quote(pair.getSecond().identifier().toString())
                    + ", \"space\": " + array(List.of(
                        quote(p.temperature().min()), quote(p.temperature().max()),
                        quote(p.humidity().min()), quote(p.humidity().max()),
                        quote(p.continentalness().min()), quote(p.continentalness().max()),
                        quote(p.erosion().min()), quote(p.erosion().max()),
                        quote(p.depth().min()), quote(p.depth().max()),
                        quote(p.weirdness().min()), quote(p.weirdness().max()),
                        quote(p.offset())))
                    + "}");
        }

        // -1.0 .. 1.0 in steps of 0.5 on the first three axes and a coarser
        // sweep on the rest, which is 5*5*5*3*3*3 = 3375 ordered lookups.
        float[] fine = {-1.0F, -0.5F, 0.0F, 0.5F, 1.0F};
        float[] coarse = {-0.8F, 0.0F, 0.8F};
        List<String> queries = new ArrayList<>();
        List<String> answers = new ArrayList<>();
        for (float t : fine) {
            for (float h : fine) {
                for (float c : fine) {
                    for (float e : coarse) {
                        for (float d : coarse) {
                            for (float w : coarse) {
                                var target = net.minecraft.world.level.biome.Climate.target(t, h, c, e, d, w);
                                queries.add("[" + quote(target.temperature())
                                        + ", " + quote(target.humidity())
                                        + ", " + quote(target.continentalness())
                                        + ", " + quote(target.erosion())
                                        + ", " + quote(target.depth())
                                        + ", " + quote(target.weirdness()) + "]");
                                answers.add(quote(parameters.findValue(target).identifier().toString()));
                            }
                        }
                    }
                }
            }
        }

        List<String> cases = new ArrayList<>();
        cases.add("{\"case\": \"overworld_climate\", \"entries\": " + array(entries)
                + ", \"queries\": " + array(queries)
                + ", \"answers\": " + array(answers) + "}");

        // And the same table driven from real world positions, through
        // vanilla's own sampler, so the router-to-climate wiring is proved
        // too — not just the search over it. Quart coordinates, in the order
        // a chunk is filled, because the search's tie-break follows the
        // query order.
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noiseRegistry =
            registries.lookupOrThrow(Registries.NOISE);
        var settings = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD);

        int[][] chunks = {{0, 0}, {-1, -1}, {1000, -1500}};
        long[] seeds = {0L, 20260725L};
        for (long seed : seeds) {
            RandomState state = RandomState.create(settings.value(), noiseRegistry, seed);
            var sampler = state.sampler();
            for (int[] chunk : chunks) {
                List<String> positions = new ArrayList<>();
                List<String> biomes = new ArrayList<>();
                // 4 quarts across a chunk, and a spread of heights.
                for (int qy = -16; qy <= 76; qy += 4) {
                    for (int qz = 0; qz < 4; qz++) {
                        for (int qx = 0; qx < 4; qx++) {
                            int quartX = chunk[0] * 4 + qx;
                            int quartZ = chunk[1] * 4 + qz;
                            positions.add("[" + quartX + ", " + qy + ", " + quartZ + "]");
                            biomes.add(quote(
                                parameters.findValue(sampler.sample(quartX, qy, quartZ))
                                    .identifier().toString()));
                        }
                    }
                }
                cases.add("{\"case\": \"biome_at_position\", \"seed\": " + quote(seed)
                        + ", \"chunkX\": " + chunk[0]
                        + ", \"chunkZ\": " + chunk[1]
                        + ", \"quartPositions\": " + array(positions)
                        + ", \"biomes\": " + array(biomes) + "}");
            }
        }
        return cases("climate", cases);
    }

    // ------------------------------------------------------- the noise stage

    /// Whole chunks of **block states**, straight out of vanilla's noise
    /// stage, via the public `getBaseColumn`.
    ///
    /// `getBaseColumn` runs `iterateNoiseColumn`, which builds a real
    /// `NoiseChunk` and calls the same `getInterpolatedState` — the same
    /// MaterialRuleList, the same aquifer, the same ore veinifier — that
    /// `doFill` uses, substituting `defaultBlock()` for null exactly as
    /// `doFill` does. So this is the fill's output, obtained without needing
    /// a `StructureManager` or a live level.
    ///
    /// Emitted run-length encoded in y-major order (`y`, then `z`, then `x`)
    /// because noise-stage terrain is long runs of stone and air; the raw
    /// form would be 98304 entries per chunk.
    ///
    /// Only the block's registry name is dumped, not its properties. At this
    /// stage every state is a default state — water and lava are always
    /// `level=0` and nothing else here is stateful — and the Rust side has
    /// no block registry to compare properties against yet.
    private static String noiseChunkVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);
        var settings = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD);
        var biome = registries
            .lookupOrThrow(Registries.BIOME)
            .getOrThrow(net.minecraft.world.level.biome.Biomes.PLAINS);
        var generator = new net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator(
            new net.minecraft.world.level.biome.FixedBiomeSource(biome), settings);

        var noiseSettings = settings.value().noiseSettings();
        int minY = noiseSettings.minY();
        int height = noiseSettings.height();
        var heightAccessor = net.minecraft.world.level.LevelHeightAccessor.create(minY, height);

        // Chunk 0,0 and one far from the origin. The far one is not padding:
        // the upstream Mth.getSeed bug this project found was invisible
        // within ~686 blocks of spawn, so a spawn-only oracle cannot see a
        // whole class of coordinate fault.
        // Spread deliberately, not for volume. Ore veins need |veininess| >
        // 0.4 inside y -60..50, which is rare enough that a handful of
        // chunks can miss the veinifier entirely — and a parity number that
        // never exercises a rule proves nothing about it. Negative
        // coordinates on both axes are here for the same reason: the shifts
        // and floor divisions all differ from truncation only there.
        // (34, -57) is here because the *surface* gate found 3411 differing
        // blocks in it, all of them stone/deepslate replaced by ore-vein
        // blocks — a noise-stage fault the six chunks above missed. Adding it
        // here is what tells the two stages apart: if it differs at this
        // level the surface rules are innocent.
        int[][] chunks = {
            {0, 0}, {1000, -1500}, {-1, -1}, {-64, 37}, {13, -7}, {250, 250}, {34, -57},
        };
        long[] seeds = {0L, 20260725L, -4_611_686_018_427_387_904L};

        List<String> cases = new ArrayList<>();
        for (long seed : seeds) {
            RandomState state = RandomState.create(settings.value(), noises, seed);
            for (int[] chunk : chunks) {
                List<String> palette = new ArrayList<>();
                Map<String, Integer> paletteIndex = new HashMap<>();
                List<String> runs = new ArrayList<>();

                // One column at a time, cached, so each is built once and
                // read down its full height.
                net.minecraft.world.level.NoiseColumn[] columns =
                    new net.minecraft.world.level.NoiseColumn[256];
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        columns[z * 16 + x] = generator.getBaseColumn(
                            chunk[0] * 16 + x, chunk[1] * 16 + z, heightAccessor, state);
                    }
                }

                int runValue = -1;
                int runLength = 0;
                for (int y = minY; y < minY + height; y++) {
                    for (int z = 0; z < 16; z++) {
                        for (int x = 0; x < 16; x++) {
                            var block = columns[z * 16 + x].getBlock(y).getBlock();
                            String name = net.minecraft.core.registries.BuiltInRegistries.BLOCK
                                .getKey(block).toString();
                            Integer index = paletteIndex.get(name);
                            if (index == null) {
                                index = palette.size();
                                paletteIndex.put(name, index);
                                palette.add(quote(name));
                            }
                            if (index == runValue) {
                                runLength++;
                            } else {
                                if (runLength > 0) {
                                    runs.add("[" + runValue + ", " + runLength + "]");
                                }
                                runValue = index;
                                runLength = 1;
                            }
                        }
                    }
                }
                if (runLength > 0) {
                    runs.add("[" + runValue + ", " + runLength + "]");
                }

                cases.add("{\"case\": \"noise_chunk\", \"seed\": " + quote(seed)
                        + ", \"chunkX\": " + chunk[0]
                        + ", \"chunkZ\": " + chunk[1]
                        + ", \"minY\": " + minY
                        + ", \"height\": " + height
                        + ", \"palette\": " + array(palette)
                        + ", \"runs\": " + array(runs) + "}");
            }
        }
        return cases("noise_chunk", cases);
    }

    // --------------------------------------------- interpolator corner values

    /// The *wrapped* function of every `Interpolated` marker in
    /// `finalDensity`, sampled at cell-lattice corners.
    ///
    /// `interpolated_density_vectors` can only pin `finalDensity`, and that
    /// is strictly weaker than it looks: `finalDensity` is
    /// `min(squeeze(interpolated(pp)), noodle)`, `noodle` is
    /// `1.5 * max(|A|, |B|) + thickness` over three more markers, and every
    /// `min`, `max` and `rangeChoice` in that chain discards a branch. A
    /// wrapped value can be several ulp wrong at a corner, be masked by a
    /// losing branch, and only surface once the blend mixes two corners —
    /// which is exactly the residue left after the lerp3 fix.
    ///
    /// So dump the wrapped values themselves. `DensityFunctions.Marker` is
    /// package-private and this file is in the default package, so the record
    /// accessors need `setAccessible` — the server jar is on the classpath,
    /// not the module path, so that is permitted.
    ///
    /// Marker order is the graph's post-order, children before parents and
    /// arguments left to right, which is the order `mapAll` visits in and the
    /// order the Rust side numbers interpolators in.
    private static String interpolatorCornerVectors() {
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noiseRegistry =
            registries.lookupOrThrow(Registries.NOISE);
        var settings = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(net.minecraft.world.level.levelgen.NoiseGeneratorSettings.OVERWORLD);

        // Cell corners for the columns interpolated_density_vectors uses.
        int[][] cornerXZ = {{0, 0}, {4, 0}, {0, 4}, {4, 4}, {4, 0}, {8, 0}, {4, 4}, {8, 4}};
        // 31415926 is the whole-world diff seed. Added after a packed_ice
        // failure was traced to a peaks column nine blocks lower than
        // vanilla's, at which point this gate turned out to cover only the
        // two seeds it was born with.
        long[] seeds = {0L, 20260708L, 31415926L};

        List<String> cases = new ArrayList<>();
        for (long seed : seeds) {
            RandomState state = RandomState.create(settings.value(), noiseRegistry, seed);
            List<DensityFunction> wrapped = collectInterpolatedWrapped(state.router().finalDensity());

            for (int marker = 0; marker < wrapped.size(); marker++) {
                DensityFunction f = wrapped.get(marker);
                List<String> pts = new ArrayList<>();
                List<String> bits = new ArrayList<>();
                for (int[] xz : cornerXZ) {
                    // Lattice corners only: y steps by the 8-block cell height.
                    for (int y = -64; y <= 320; y += 8) {
                        double v = f.compute(new DensityFunction.SinglePointContext(xz[0], y, xz[1]));
                        pts.add("[" + xz[0] + ", " + y + ", " + xz[1] + "]");
                        bits.add(quote(Double.doubleToRawLongBits(v)));
                    }
                }
                cases.add("{\"case\": \"interpolator_corner\", \"seed\": " + quote(seed)
                        + ", \"marker\": " + marker
                        + ", \"minValue\": " + quote(Double.doubleToRawLongBits(f.minValue()))
                        + ", \"maxValue\": " + quote(Double.doubleToRawLongBits(f.maxValue()))
                        + ", \"blockPoints\": " + array(pts)
                        + ", \"output_bits\": " + array(bits) + "}");
            }
        }
        return cases("interpolator_corner", cases);
    }

    /// Walks `root` with the public `mapAll` visitor and returns the wrapped
    /// function of each `Interpolated` marker, in visit order.
    private static List<DensityFunction> collectInterpolatedWrapped(DensityFunction root) {
        List<DensityFunction> found = new ArrayList<>();
        root.mapAll(node -> {
            if (node.getClass().getName().endsWith("DensityFunctions$Marker")) {
                try {
                    var typeMethod = node.getClass().getDeclaredMethod("type");
                    typeMethod.setAccessible(true);
                    if ("Interpolated".equals(String.valueOf(typeMethod.invoke(node)))) {
                        var wrappedMethod = node.getClass().getDeclaredMethod("wrapped");
                        wrappedMethod.setAccessible(true);
                        found.add((DensityFunction) wrappedMethod.invoke(node));
                    }
                } catch (ReflectiveOperationException e) {
                    throw new IllegalStateException("cannot read Marker accessors", e);
                }
            }
            return node;
        });
        return found;
    }

    // --------------------------------------------------------------- density

    /// Block positions for density evaluation. Includes deep-negative y (the
    /// world floor), a y past the gradient span (clamping), and a far-from-
    /// origin xz (the coordinate range where the upstream getSeed bug hid).
    private static final int[][] DENSITY_POINTS = {
        {0, 0, 0},
        {4, 64, 4},
        {-100, -64, 100},
        {1523, 128, -2048},
        {7, -32, -7},
        {100000, 320, -100000},
    };

    /// Fixed noise parameters for every bound NoiseHolder, so the Rust side
    /// can rebuild the identical tree from the key alone.
    private static final int NOISE_FIRST_OCTAVE = -7;
    private static final double[] NOISE_AMPS = {1.0, 1.0};
    private static final long DENSITY_SEED = 20260708L;

    /// Binds every NoiseHolder in the tree, the way NoiseRouterData does.
    /// Each holder gets a fresh source at the same seed — deterministic and
    /// trivially reproducible, which is what a test vector needs.
    private static DensityFunction bind(DensityFunction f) {
        return f.mapAll(new DensityFunction.Visitor() {
            @Override
            public DensityFunction apply(DensityFunction input) {
                return input;
            }

            @Override
            public DensityFunction.NoiseHolder visitNoise(DensityFunction.NoiseHolder holder) {
                NormalNoise noise = NormalNoise.create(
                    new XoroshiroRandomSource(DENSITY_SEED),
                    NOISE_FIRST_OCTAVE,
                    NOISE_AMPS);
                return new DensityFunction.NoiseHolder(holder.noiseData(), noise);
            }
        });
    }

    private static String densityVectors() {
        Holder<NormalNoise.NoiseParameters> params = Holder.direct(
            new NormalNoise.NoiseParameters(NOISE_FIRST_OCTAVE, NOISE_AMPS[0], NOISE_AMPS[1]));

        // A non-constant, non-noise input so `min`/`max`/`mul` build real
        // Ap2 nodes instead of folding to MulOrAdd.
        DensityFunction ramp = DensityFunctions.yClampedGradient(-64, 320, -1.0, 1.0);
        DensityFunction ramp2 = DensityFunctions.yClampedGradient(0, 128, 2.0, -3.0);

        List<String> cases = new ArrayList<>();
        emitDensity(cases, "constant", DensityFunctions.constant(5.0));
        emitDensity(cases, "y_clamped_gradient", ramp);
        emitDensity(cases, "add_const_left", DensityFunctions.add(DensityFunctions.constant(2.5), ramp));
        emitDensity(cases, "add_const_right", DensityFunctions.add(ramp, DensityFunctions.constant(2.5)));
        emitDensity(cases, "mul_const", DensityFunctions.mul(ramp, DensityFunctions.constant(-3.0)));
        emitDensity(cases, "add_two", DensityFunctions.add(ramp, ramp2));
        emitDensity(cases, "mul_two", DensityFunctions.mul(ramp, ramp2));
        emitDensity(cases, "min_two", DensityFunctions.min(ramp, ramp2));
        emitDensity(cases, "max_two", DensityFunctions.max(ramp, ramp2));
        emitDensity(cases, "abs", ramp.abs());
        emitDensity(cases, "square", ramp.square());
        emitDensity(cases, "cube", ramp.cube());
        emitDensity(cases, "half_negative", ramp.halfNegative());
        emitDensity(cases, "quarter_negative", ramp.quarterNegative());
        emitDensity(cases, "invert", ramp2.invert());
        emitDensity(cases, "squeeze", ramp2.squeeze());
        emitDensity(cases, "clamp", ramp2.clamp(-1.5, 0.5));
        emitDensity(cases, "range_choice", DensityFunctions.rangeChoice(
            ramp, -0.5, 0.5, DensityFunctions.constant(1.0), ramp2));
        emitDensity(cases, "noise", DensityFunctions.noise(params, 0.25, 0.5));
        emitDensity(cases, "shift", DensityFunctions.shift(params));
        emitDensity(cases, "shift_a", DensityFunctions.shiftA(params));
        emitDensity(cases, "shift_b", DensityFunctions.shiftB(params));
        emitDensity(cases, "shifted_noise_2d", DensityFunctions.shiftedNoise2d(
            DensityFunctions.shiftA(params), DensityFunctions.shiftB(params), 0.25, params));
        emitDensity(cases, "old_blended_noise",
            new BlendedNoise(new XoroshiroRandomSource(DENSITY_SEED), 1.0, 1.0, 80.0, 160.0, 8.0));
        // A nested tree — proves bounds propagate through composition, which
        // is what drives the Min/Max short-circuits.
        emitDensity(cases, "nested", DensityFunctions.max(
            DensityFunctions.mul(
                ramp.square(),
                DensityFunctions.constant(0.5)),
            DensityFunctions.min(ramp2, DensityFunctions.noise(params, 1.0, 1.0))));

        // Splines — the one worldgen layer that evaluates in f32.
        DensityFunctions.Spline.Coordinate coord =
            new DensityFunctions.Spline.Coordinate(ramp);
        DensityFunctions.Spline.Coordinate coord2 =
            new DensityFunctions.Spline.Coordinate(ramp2);

        emitDensity(cases, "spline_flat", DensityFunctions.spline(
            CubicSpline.builder(coord)
                .addPoint(-1.0F, 10.0F)
                .addPoint(0.0F, 20.0F)
                .addPoint(1.0F, 30.0F)
                .build()));

        // Non-zero derivatives: exercises the cubic correction term and the
        // interior-overshoot bound.
        emitDensity(cases, "spline_derivatives", DensityFunctions.spline(
            CubicSpline.builder(coord)
                .addPoint(-1.0F, 0.0F, 2.0F)
                .addPoint(-0.2F, 1.0F, -1.5F)
                .addPoint(0.6F, -0.5F, 0.75F)
                .addPoint(1.0F, 2.0F, 0.0F)
                .build()));

        // Single point with slope: everything is linear extension.
        emitDensity(cases, "spline_extend", DensityFunctions.spline(
            CubicSpline.builder(coord).addPoint(0.0F, 5.0F, 2.0F).build()));

        // Nested — a spline whose control values are splines over a
        // *different* coordinate, which is the terrain shaper's shape.
        var inner =
            CubicSpline.builder(coord2)
                .addPoint(-3.0F, 0.25F, 0.1F)
                .addPoint(2.0F, 0.75F, -0.1F)
                .build();
        emitDensity(cases, "spline_nested", DensityFunctions.spline(
            CubicSpline.builder(coord)
                .addPoint(-1.0F, inner)
                .addPoint(0.3F, 1.5F, 0.5F)
                .addPoint(1.0F, inner)
                .build()));

        // Terrain shaper — vanilla's own TerrainProvider, driven by four
        // distinct gradient coordinates so each axis varies independently.
        DensityFunction continentsC = DensityFunctions.yClampedGradient(-64, 320, -1.0, 1.0);
        DensityFunction erosionC = DensityFunctions.yClampedGradient(-64, 320, 1.0, -1.0);
        DensityFunction ridgesC = DensityFunctions.yClampedGradient(0, 256, -1.0, 1.0);
        DensityFunction weirdnessC = DensityFunctions.yClampedGradient(-64, 128, -1.0, 1.0);
        DensityFunctions.Spline.Coordinate cc =
            new DensityFunctions.Spline.Coordinate(continentsC);
        DensityFunctions.Spline.Coordinate ec =
            new DensityFunctions.Spline.Coordinate(erosionC);
        DensityFunctions.Spline.Coordinate rc =
            new DensityFunctions.Spline.Coordinate(ridgesC);
        DensityFunctions.Spline.Coordinate wc =
            new DensityFunctions.Spline.Coordinate(weirdnessC);

        for (boolean amplified : new boolean[] {false, true}) {
            String suffix = amplified ? "_amplified" : "";
            emitDensity(cases, "terrain_offset" + suffix, DensityFunctions.spline(
                TerrainProvider.overworldOffset(cc, ec, rc, amplified)));
            emitDensity(cases, "terrain_factor" + suffix, DensityFunctions.spline(
                TerrainProvider.overworldFactor(cc, ec, wc, rc, amplified)));
            emitDensity(cases, "terrain_jaggedness" + suffix, DensityFunctions.spline(
                TerrainProvider.overworldJaggedness(cc, ec, wc, rc, amplified)));
        }

        // End islands — public factory, so vanilla builds it for us.
        for (long endSeed : new long[] {0L, 20260708L}) {
            emitDensity(cases, "end_islands_" + (endSeed == 0 ? "0" : "s"),
                DensityFunctions.endIslands(endSeed));
        }

        // Markers and blend functions — passthroughs at this layer, but the
        // BlendDensity bounds quirk and the lerp branch depend on them.
        emitDensity(cases, "blend_alpha", DensityFunctions.blendAlpha());
        emitDensity(cases, "blend_offset", DensityFunctions.blendOffset());
        emitDensity(cases, "marker_interpolated", DensityFunctions.interpolated(ramp));
        emitDensity(cases, "marker_flat_cache", DensityFunctions.flatCache(ramp));
        emitDensity(cases, "marker_cache_2d", DensityFunctions.cache2d(ramp));
        emitDensity(cases, "marker_cache_once", DensityFunctions.cacheOnce(ramp));
        emitDensity(cases, "marker_cache_all_in_cell", DensityFunctions.cacheAllInCell(ramp));
        emitDensity(cases, "marker_blend_density", DensityFunctions.blendDensity(ramp));

        // lerp both ways: constant `first` takes the direct expansion,
        // blendOffset() takes the cacheOnce branch.
        emitDensity(cases, "lerp_constant_first",
            DensityFunctions.lerp(ramp, DensityFunctions.constant(0.25), ramp2));
        emitDensity(cases, "lerp_general",
            DensityFunctions.lerp(DensityFunctions.blendAlpha(),
                DensityFunctions.blendOffset(), ramp));

        // The slide + postProcess. NoiseRouterData's own `slide` is private,
        // so the composition is rebuilt here from public factories exactly as
        // that method writes it. This proves our arithmetic and node order
        // against a Java-built equivalent; the end-to-end `final_density`
        // comparison still waits on caves (w3 Group E).
        for (boolean amplified : new boolean[] {false, true}) {
            int minY = -64, height = 384;
            int topStartY = amplified ? 16 : 80;
            int topEndY = amplified ? 0 : 64;
            double topTarget = -0.078125;
            int bottomStartY = 0, bottomEndY = 24;
            double bottomTarget = amplified ? 0.4 : 0.1171875;

            DensityFunction topFactor = DensityFunctions.yClampedGradient(
                minY + height - topStartY, minY + height - topEndY, 1.0, 0.0);
            DensityFunction withTop = DensityFunctions.lerp(topFactor, topTarget, ramp);
            DensityFunction bottomFactor = DensityFunctions.yClampedGradient(
                minY + bottomStartY, minY + bottomEndY, 0.0, 1.0);
            DensityFunction slid = DensityFunctions.lerp(bottomFactor, bottomTarget, withTop);

            String suffix = amplified ? "_amplified" : "";
            emitDensity(cases, "slide" + suffix, slid);
            emitDensity(cases, "post_process" + suffix, DensityFunctions.mul(
                DensityFunctions.interpolated(DensityFunctions.blendDensity(slid)),
                DensityFunctions.constant(0.64)).squeeze());
        }

        // IntervalSelect + the 26.2 rarity-band shape it replaced
        // WeirdScaledSampler with.
        emitDensity(cases, "interval_select", DensityFunctions.intervalSelect(
            ramp,
            it.unimi.dsi.fastutil.doubles.DoubleList.of(-0.5, 0.0, 0.5),
            java.util.List.of(
                DensityFunctions.constant(-2.0),
                DensityFunctions.constant(-1.0),
                DensityFunctions.constant(1.0),
                DensityFunctions.constant(2.0))));

        DensityFunction rarity3d = DensityFunctions.intervalSelect(
            ramp,
            it.unimi.dsi.fastutil.doubles.DoubleList.of(-0.5, 0.0, 0.5),
            java.util.List.of(
                rarityNoise(params, 0.75), rarityNoise(params, 1.0),
                rarityNoise(params, 1.5), rarityNoise(params, 2.0))).abs();
        emitDensity(cases, "rarity_3d", rarity3d);

        return cases("density", cases);
    }

    /// QuantizedSpaghettiRarity.noiseFunctionForRarity.
    private static DensityFunction rarityNoise(
            Holder<NormalNoise.NoiseParameters> noise, double rarity) {
        return DensityFunctions.mul(DensityFunctions.constant(rarity),
            DensityFunctions.noise(noise, 1.0 / rarity, 1.0 / rarity));
    }

    private static void emitDensity(List<String> cases, String key, DensityFunction raw) {
        DensityFunction f = bind(raw);
        List<String> bits = new ArrayList<>();
        List<String> pts = new ArrayList<>();
        for (int[] p : DENSITY_POINTS) {
            double v = f.compute(new DensityFunction.SinglePointContext(p[0], p[1], p[2]));
            bits.add(quote(Double.doubleToRawLongBits(v)));
            pts.add("[" + p[0] + ", " + p[1] + ", " + p[2] + "]");
        }
        cases.add("{\"case\": " + quote(key)
                + ", \"minValue_bits\": " + quote(Double.doubleToRawLongBits(f.minValue()))
                + ", \"maxValue_bits\": " + quote(Double.doubleToRawLongBits(f.maxValue()))
                + ", \"blockPoints\": " + array(pts)
                + ", \"output_bits\": " + array(bits) + "}");
    }

    private static double[] tail(double[] values) {
        double[] rest = new double[values.length - 1];
        System.arraycopy(values, 1, rest, 0, rest.length);
        return rest;
    }

    private static String points() {
        List<String> items = new ArrayList<>();
        for (double[] p : SAMPLE_POINTS) {
            items.add("[" + quote(Double.doubleToRawLongBits(p[0]))
                    + ", " + quote(Double.doubleToRawLongBits(p[1]))
                    + ", " + quote(Double.doubleToRawLongBits(p[2])) + "]");
        }
        return array(items);
    }

    private static String doubles(double[] values) {
        List<String> items = new ArrayList<>();
        for (double v : values) {
            items.add(quote(Double.doubleToRawLongBits(v)));
        }
        return array(items);
    }

    // ---------------------------------------------------------------- helpers

    private static PositionalRandomFactory positional() {
        return new XoroshiroRandomSource(DERIVED_SEED).forkPositional();
    }

    private static WorldgenRandom legacyWorldgen() {
        return new WorldgenRandom(new LegacyRandomSource(0L));
    }

    private static WorldgenRandom xoroshiroWorldgen() {
        return new WorldgenRandom(new XoroshiroRandomSource(0L));
    }

    private static RandomSource xoroshiroWorldgenSeeded() {
        return new WorldgenRandom(new XoroshiroRandomSource(DERIVED_SEED));
    }

    private static List<String> longs(RandomSource rng) {
        List<String> outputs = new ArrayList<>(DERIVED_LEN);
        for (int i = 0; i < DERIVED_LEN; i++) {
            outputs.add(quote(rng.nextLong()));
        }
        return outputs;
    }

    private static String cases(String rng, List<String> cases) {
        StringBuilder sb = new StringBuilder("{\n  \"rng\": ").append(quote(rng)).append(",\n  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            sb.append("    ").append(cases.get(i));
            if (i + 1 < cases.size()) {
                sb.append(',');
            }
            sb.append('\n');
        }
        return sb.append("  ]\n}\n").toString();
    }

    /// Wrapped at 8 items per line. A 10,000-element stream on one line is a
    /// 230 KB line: unreviewable in a diff, and reviewing the diff is the
    /// entire reason these files are committed.
    private static final int ITEMS_PER_LINE = 8;

    private static String array(List<String> items) {
        if (items.size() <= ITEMS_PER_LINE) {
            return "[" + String.join(", ", items) + "]";
        }
        StringBuilder sb = new StringBuilder("[\n     ");
        for (int i = 0; i < items.size(); i++) {
            if (i > 0) {
                sb.append(',');
                sb.append(i % ITEMS_PER_LINE == 0 ? "\n     " : " ");
            }
            sb.append(items.get(i));
        }
        return sb.append("\n    ]").toString();
    }

    private static String quote(long v) {
        return "\"" + v + "\"";
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private static void write(Path path, String content) throws IOException {
        Files.write(path, content.getBytes(StandardCharsets.UTF_8));
    }

    private GoldenDump() {}
}
