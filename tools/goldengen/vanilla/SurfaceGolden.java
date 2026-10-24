// SurfaceGolden — vanilla's surface-depth field and badlands band palette.
//
// Declared inside vanilla's own package so it can call `SurfaceSystem`'s
// protected `getSurfaceDepth` and `getBand` without reflection. That needs a
// signature-stripped jar, since the JVM refuses to mix an unsigned class into
// a signed package — `ci/surface-golden.sh` makes one, exactly as
// `ci/bench-vanilla.sh` does.
//
// Why these two in particular: they are the pieces of the surface system that
// are pure functions of the world seed, so they can be compared without
// building a chunk. Between them they exercise the parts most likely to be
// quietly wrong:
//
//   getSurfaceDepth  the `noise * 2.75 + 3.0 + random * 0.25` formula, the
//                    truncating cast, and the fact that the jitter comes from
//                    the world's *base* positional factory rather than a
//                    named stream.
//   getBand          the whole 192-entry palette — the orange pass's double
//                    increment, three overwrite passes, the white bands with
//                    their grey edges — plus `Math.round` on the offset noise
//                    and the wrapping modulo.
//
// The rest of the surface system needs a populated chunk and a biome manager;
// that harness is a separate job, and this one is deliberately the part that
// can be proved today.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.synth.NormalNoise;

public final class SurfaceGolden {

    /// Seeds chosen to include zero and a negative, since the seeding chain
    /// runs through a 128-bit upgrade where sign handling is easy to lose.
    private static final long[] SEEDS = {0L, 20260725L, -4611686018427387904L};

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);
        NoiseGeneratorSettings settings = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD)
            .value();

        List<String> cases = new ArrayList<>();
        for (long seed : SEEDS) {
            RandomState state = RandomState.create(settings, noises, seed);
            SurfaceSystem system = state.surfaceSystem();

            // A spread of columns rather than a solid block: near the origin,
            // far from it, and negative on both axes, because every floor
            // division and shift in the chain behaves differently there.
            List<String> depthPoints = new ArrayList<>();
            List<String> depths = new ArrayList<>();
            for (int[] origin : new int[][] {{0, 0}, {-1, -1}, {137, -509}, {16000, 16000}}) {
                for (int dx = 0; dx < 8; dx++) {
                    for (int dz = 0; dz < 8; dz++) {
                        int x = origin[0] + dx;
                        int z = origin[1] + dz;
                        depthPoints.add("[" + x + ", " + z + "]");
                        depths.add(Integer.toString(system.getSurfaceDepth(x, z)));
                    }
                }
            }

            // The band palette, read back through getBand rather than the
            // private array: that covers the offset noise, Math.round and the
            // wrapping modulo as well as the table itself.
            List<String> bandPoints = new ArrayList<>();
            List<String> bandNames = new ArrayList<>();
            for (int[] column : new int[][] {{0, 0}, {73, -41}, {-1000, 1000}}) {
                for (int y = -64; y < 320; y += 7) {
                    BlockState band = system.getBand(column[0], y, column[1]);
                    bandPoints.add("[" + column[0] + ", " + y + ", " + column[1] + "]");
                    bandNames.add(quote(net.minecraft.core.registries.BuiltInRegistries.BLOCK
                        .getKey(band.getBlock()).toString()));
                }
            }

            cases.add("{\"case\": \"surface\", \"seed\": " + quote(seed)
                    + ", \"depthPoints\": " + array(depthPoints)
                    + ", \"depths\": " + array(depths)
                    + ", \"bandPoints\": " + array(bandPoints)
                    + ", \"bands\": " + array(bandNames) + "}");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"surface\",\n  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i));
            if (i + 1 < cases.size()) {
                out.append(',');
            }
            out.append('\n');
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static String array(List<String> items) {
        return "[" + String.join(", ", items) + "]";
    }

    private static String quote(long v) {
        return "\"" + v + "\"";
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private SurfaceGolden() {}
}
