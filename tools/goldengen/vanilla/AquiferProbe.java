// AquiferProbe — ask vanilla itself what an aquifer centre is and how full it
// is, for named lattice cells at a named seed.
//
// # Why this exists
//
// `RUSTMC_TRACE_AQUIFER` prints our side of the same question: for one block,
// the twelve contending lattice cells, where each one's centre was jittered
// to, and the fluid level that centre holds. Until this probe there was no way
// to ask vanilla the same thing, so an aquifer disagreement could only be
// argued from the finished region file — and a region file shows the *winner*,
// never the numbers behind it. Two clusters of missing water were attributed
// to the aquifer on exactly that evidence, and both were something else.
//
// It answers three things, which are the three inputs to a fluid level:
//
//   CENTRE  `aquiferRandom().at(cell)` and the three bounded draws, i.e. the
//           jittered centre. Wrong centres move every water table.
//   SPREAD  `fluid_level_spread` at a 16x40x16 fluid cell, with the `* 10`,
//           the `Mth.quantize(..., 3)` and the resulting `20 + q` waterline.
//           A neighbouring pair of cells one quantize step apart is what a
//           water/air boundary inside a cave actually is.
//   FLOOD   `fluid_level_floodedness`, `erosion` and `depth` at a centre —
//           the three that choose between "full", "partially flooded" and
//           "dry", and the two that make the deep dark dry.
//
// # What it does not do
//
// It takes lattice cells, not block positions: deriving the cell from a block
// needs `NoiseBasedAquifer`'s private grid mapping, and a probe that
// re-implemented that mapping could agree with itself while both it and the
// port were wrong. Read the cell out of `RUSTMC_TRACE_AQUIFER` and hand it
// here, so the only thing being compared is what vanilla answers for a cell
// both sides already agree on.
//
// Everything used is public on `RandomState` and `NoiseRouter`, so this needs
// neither reflection nor a package trick.
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;

public final class AquiferProbe {

    private static void usage() {
        System.err.println("usage: AquiferProbe <seed> <spec>...");
        System.err.println("  centre:gx,gy,gz   jittered aquifer centre for a lattice cell");
        System.err.println("  spread:cx,cy,cz   fluid level for a 16x40x16 fluid cell");
        System.err.println("  flood:x,y,z       floodedness/erosion/depth at a centre");
        System.exit(2);
    }

    public static void main(String[] args) {
        if (args.length < 2) {
            usage();
        }
        long seed = Long.parseLong(args[0]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        var noises = registries.lookupOrThrow(Registries.NOISE);
        var settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD);
        RandomState state = RandomState.create(settings.value(), noises, seed);

        System.out.println("SEED " + seed);
        for (int i = 1; i < args.length; i++) {
            String[] half = args[i].split(":", 2);
            if (half.length != 2) {
                usage();
            }
            String[] parts = half[1].split(",");
            if (parts.length != 3) {
                usage();
            }
            int a = Integer.parseInt(parts[0].trim());
            int b = Integer.parseInt(parts[1].trim());
            int c = Integer.parseInt(parts[2].trim());
            switch (half[0]) {
                case "centre" -> centre(state, a, b, c);
                case "spread" -> spread(state, a, b, c);
                case "flood" -> flood(state, a, b, c);
                default -> usage();
            }
        }
    }

    /// The three draws are x, y, z in that order from one positional source,
    /// bounded by spacing minus separation: 16-6, 12-3, 16-6.
    private static void centre(RandomState state, int gx, int gy, int gz) {
        RandomSource rng = state.aquiferRandom().at(gx, gy, gz);
        int x = gx * 16 + rng.nextInt(10);
        int y = gy * 12 + rng.nextInt(9);
        int z = gz * 16 + rng.nextInt(10);
        System.out.println("CENTRE cell=(" + gx + "," + gy + "," + gz + ") centre=("
            + x + "," + y + "," + z + ")");
    }

    /// `computeRandomizedFluidSurfaceLevel`, minus the `min` against the
    /// preliminary surface level — that clamp needs a `NoiseChunk` and can
    /// only lower the answer, so the unclamped waterline is the number worth
    /// comparing.
    private static void spread(RandomState state, int cx, int cy, int cz) {
        DensityFunction spread = state.router().fluidLevelSpreadNoise();
        double value = spread.compute(new DensityFunction.SinglePointContext(cx, cy, cz));
        int q = Mth.quantize(value * 10.0, 3);
        System.out.println("SPREAD cell=(" + cx + "," + cy + "," + cz + ") value=" + value
            + " x10=" + (value * 10.0) + " quantize3=" + q
            + " level=" + (cy * 40 + 20 + q));
    }

    private static void flood(RandomState state, int x, int y, int z) {
        var ctx = new DensityFunction.SinglePointContext(x, y, z);
        System.out.println("FLOOD at=(" + x + "," + y + "," + z + ")"
            + " floodedness=" + state.router().fluidLevelFloodednessNoise().compute(ctx)
            + " erosion=" + state.router().erosion().compute(ctx)
            + " depth=" + state.router().depth().compute(ctx));
    }
}
