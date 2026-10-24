// TemperatureGolden — the surface rule's `temperature` condition, both layers.
//
// The condition asks a biome `coldEnoughToSnow`, which is a float comparison
// against a height-adjusted base temperature, which is a function of three
// static `PerlinSimplexNoise` stacks. That is two independent things to get
// wrong, so this dumps both:
//
//   1. Raw `PerlinSimplexNoise.getValue` at the exact scales the callers use,
//      for all three stacks. If the port fails here it is the noise.
//   2. `coldEnoughToSnow` for every biome over a position grid. If the port
//      passes (1) and fails (2) it is the float arithmetic or the modifier.
//
// The three noises are private statics on `Biome`, so they are rebuilt here
// from the same constructor arguments rather than read out. That is a claim
// about the reference, not a fact — so the harness checks it: `BIOME_INFO_NOISE`
// *is* public, and the dump carries a flag saying whether the locally rebuilt
// 2345/[0] stack agrees with it. A false flag means these vectors describe a
// noise vanilla does not use, and the Rust test refuses them.
//
// Converted to JSON by `ci/temperature-golden.sh`.

import com.google.common.collect.ImmutableList;
import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.levelgen.LegacyRandomSource;
import net.minecraft.world.level.levelgen.WorldgenRandom;
import net.minecraft.world.level.levelgen.synth.PerlinSimplexNoise;

public final class TemperatureGolden {

    /// Overworld sea level. `SurfaceRules.Context.getSeaLevel()` supplies it
    /// from the noise settings; hardcoded here because the condition's
    /// behaviour is what is being measured, not the plumbing.
    private static final int SEA_LEVEL = 63;

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        PerlinSimplexNoise temperature = build(1234L, ImmutableList.of(0));
        PerlinSimplexNoise frozen = build(3456L, ImmutableList.of(-2, -1, 0));
        PerlinSimplexNoise biomeInfo = build(2345L, ImmutableList.of(0));

        List<double[]> points = samplePoints();
        boolean rebuildMatches = rebuiltStackMatchesVanilla(biomeInfo, points);

        StringBuilder out = new StringBuilder();
        out.append("{\n  \"rng\": \"temperature\",\n");
        out.append("  \"seaLevel\": ").append(SEA_LEVEL).append(",\n");
        out.append("  \"rebuiltBiomeInfoMatchesVanilla\": ").append(rebuildMatches).append(",\n");
        out.append("  \"noise\": [\n");
        out.append(dumpNoise("temperature", 1234L, "[0]", temperature, points)).append(",\n");
        out.append(dumpNoise("frozen", 3456L, "[-2,-1,0]", frozen, points)).append(",\n");
        out.append(dumpNoise("biome_info", 2345L, "[0]", biomeInfo, points)).append('\n');
        out.append("  ],\n");

        List<int[]> positions = positions();
        out.append("  \"positions\": [");
        for (int i = 0; i < positions.size(); i++) {
            int[] p = positions.get(i);
            out.append(i == 0 ? "" : ", ")
               .append('[').append(p[0]).append(", ").append(p[1]).append(", ").append(p[2]).append(']');
        }
        out.append("],\n");

        out.append("  \"biomes\": [\n");
        List<String> rows = coldRows(positions);
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i));
            if (i + 1 < rows.size()) {
                out.append(',');
            }
            out.append('\n');
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static PerlinSimplexNoise build(long seed, List<Integer> octaves) {
        return new PerlinSimplexNoise(new WorldgenRandom(new LegacyRandomSource(seed)), octaves);
    }

    /// Does the locally rebuilt 2345/[0] stack equal the one `Biome` holds?
    ///
    /// This is the harness auditing its own premise. `BIOME_INFO_NOISE` is the
    /// only one of the three that is public, so it is the only one that can be
    /// checked — but all three are built the same way, so agreement here is
    /// real evidence that the other two constructor arguments were read right.
    @SuppressWarnings("deprecation")
    private static boolean rebuiltStackMatchesVanilla(PerlinSimplexNoise rebuilt, List<double[]> points) {
        for (double[] p : points) {
            double mine = rebuilt.getValue(p[0], p[1], false);
            double theirs = Biome.BIOME_INFO_NOISE.getValue(p[0], p[1], false);
            if (Double.doubleToRawLongBits(mine) != Double.doubleToRawLongBits(theirs)) {
                return false;
            }
        }
        return true;
    }

    /// Sample points built from the expressions the real callers use, so the
    /// vectors cover the scales that actually occur rather than a tidy grid.
    private static List<double[]> samplePoints() {
        List<double[]> points = new ArrayList<>();
        for (int i = -33; i <= 33; i++) {
            int c = i * 37;
            // getHeightAdjustedTemperature: int / 8.0F, widened.
            points.add(new double[]{c / 8.0F, (c + 11) / 8.0F});
            // FROZEN: the three double scales.
            points.add(new double[]{c * 0.05, (c - 5) * 0.05});
            points.add(new double[]{c * 0.2, (c - 5) * 0.2});
            points.add(new double[]{c * 0.09, (c - 5) * 0.09});
        }
        return points;
    }

    private static String dumpNoise(
        String name, long seed, String octaves, PerlinSimplexNoise noise, List<double[]> points
    ) {
        StringBuilder sb = new StringBuilder();
        sb.append("    {\"name\": \"").append(name).append("\", \"seed\": \"").append(seed)
          .append("\", \"octaves\": \"").append(octaves).append("\", \"samples\": [");
        for (int i = 0; i < points.size(); i++) {
            double[] p = points.get(i);
            sb.append(i == 0 ? "" : ", ")
              .append('[')
              .append(bits(p[0])).append(", ")
              .append(bits(p[1])).append(", ")
              .append(bits(noise.getValue(p[0], p[1], false)))
              .append(']');
        }
        sb.append("]}");
        return sb.toString();
    }

    /// The position grid.
    ///
    /// Three jobs. The sparse block straddles the snow level (80) so both
    /// branches of `getHeightAdjustedTemperature` are exercised. The dense
    /// x-sweep at sea level is there for the `frozen` modifier, whose ice
    /// patches are a few tens of blocks across — a sparse grid would sample
    /// only one side of them and the modifier would look like a constant.
    /// The far-out column checks the coordinate range where `int`-to-`float`
    /// stops being exact.
    private static List<int[]> positions() {
        List<int[]> positions = new ArrayList<>();
        for (int x : new int[]{-129, -8, 0, 7, 64, 1000}) {
            for (int z : new int[]{-77, 0, 33, 512}) {
                for (int y : new int[]{-64, 40, 63, 79, 80, 81, 100, 200, 319}) {
                    positions.add(new int[]{x, y, z});
                }
            }
        }
        for (int x = -128; x <= 128; x++) {
            positions.add(new int[]{x, SEA_LEVEL, 0});
        }
        for (int x : new int[]{16_777_215, 16_777_216, 16_777_217, 29_999_984, -29_999_984}) {
            positions.add(new int[]{x, 200, 0});
        }
        return positions;
    }

    private static List<String> coldRows(List<int[]> positions) {
        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        List<String> rows = new ArrayList<>();
        // **Sorted by name.** `listElements()` has no stable order between JVM
        // runs — two runs of this dumper produced the same rows in different
        // sequence, which churns the vector file on every regeneration and
        // would hide a real regression inside the churn. The Rust gate keys
        // by id so it was never reading the wrong row, but a vector file that
        // changes when nothing changed is not evidence. ORACLE.md 5b rule 10.
        lookup.lookupOrThrow(Registries.BIOME).listElements()
            .sorted(java.util.Comparator.comparing(h -> h.key().identifier().toString()))
            .forEach(holder -> {
            Biome biome = holder.value();
            StringBuilder cold = new StringBuilder(positions.size());
            BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();
            for (int[] p : positions) {
                cold.append(biome.coldEnoughToSnow(cursor.set(p[0], p[1], p[2]), SEA_LEVEL) ? '1' : '0');
            }
            rows.add("{\"id\": \"" + holder.key().identifier()
                + "\", \"baseTemperature\": " + bits(biome.getBaseTemperature())
                + ", \"cold\": \"" + cold + "\"}");
        });
        return rows;
    }

    /// Doubles as raw bits, quoted, so nothing depends on a decimal round
    /// trip through JSON.
    private static String bits(double v) {
        return "\"" + Double.doubleToRawLongBits(v) + "\"";
    }

    private static String bits(float v) {
        return "\"" + Float.floatToRawIntBits(v) + "\"";
    }

    private TemperatureGolden() {}
}
