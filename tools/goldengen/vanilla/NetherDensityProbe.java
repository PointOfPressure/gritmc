// NetherDensityProbe — ask vanilla itself what the nether's final density is,
// block by block, up a column.
//
// # Why this exists
//
// The nether was measured for the first time on 2026-08-02 and came in at
// 99.671% with the residual almost entirely order-INVARIANT — the mirror of
// the overworld, where 95.5% of it was chunk-visitation order. A column at
// (915, z=1028) narrowed it to the noise stage: our carver touches nothing
// there, our noise-stage world already has the solid slab at y 63..67, and
// vanilla's finished world has it at 58..63. A carver cannot carve five
// blocks into existence.
//
// From that point a region file has nothing left to say. It shows blocks, and
// the question is about the *number* the density function produced — whether
// it crosses zero at the wrong y, or has the wrong sign, or is merely offset.
// Those are three different bugs with three different fixes and the same
// block output.
//
// This is the counterpart to `worldgen-dump nether-density`, exactly as
// `AquiferProbe` is the counterpart to `RUSTMC_TRACE_AQUIFER`. Same seed,
// same column, two lists of doubles, printed as raw bits so a comparison is
// exact rather than "agrees to six places".
//
// # What SinglePointContext does and does not give you, and it is the point
//
// `finalDensity` for the nether is
// `squeeze(interpolated(mul(0.64, blend_density(<slide>))))`. A
// `DensityFunctions$Marker` of type `Interpolated` delegates `compute` to its
// wrapped function, so evaluating through a `SinglePointContext` gives the
// **un-interpolated** value at that exact block — the value the cell-corner
// lattice would be built from, not the trilinear blend the chunk filler
// actually uses.
//
// That is a feature. It splits the remaining question in two, and the split
// is the whole reason to build this:
//
//   * the columns AGREE  -> the base noise, the slide and the squeeze are all
//                           right, and the defect is in the interpolation:
//                           the cell lattice, its origin, or the lerp.
//   * the columns DIFFER -> the defect is upstream of interpolation and the
//                           lattice is innocent.
//
// `--corners` prints only the cell-lattice rows (y a multiple of the nether's
// 8-block cell height, from min_y 0), because those are the values the
// interpolator actually samples; the rows between them are diagnostic colour.
//
// # Nether-specific parameters, which is why the overworld's zero says nothing
//
// The nether's `old_blended_noise` runs at `y_scale 0.375` and `y_factor 60`
// where the overworld runs at 0.125 and 160, and it is the only dimension
// that combines `min_y 0` with an 8-block cell. The overworld reading zero on
// two windows therefore does not clear any of that arithmetic.
//
// Everything used here is public on `RandomState` and `NoiseRouter`, so this
// needs neither reflection nor a package trick.
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.DensityFunction;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.RandomState;

public final class NetherDensityProbe {

    /// `worldgen/noise_settings/nether.json`: `size_vertical: 2` over a
    /// 4-block base, and `min_y: 0`.
    private static final int CELL_HEIGHT = 8;
    private static final int MIN_Y = 0;

    private static void usage() {
        System.err.println("usage: NetherDensityProbe <seed> <x> <z> <y0> <y1> [--corners]");
        System.err.println("  prints vanilla's nether finalDensity at each y in [y0, y1]");
        System.err.println("  --corners  only y on the 8-block cell lattice from min_y 0");
        System.exit(2);
    }

    public static void main(String[] args) {
        if (args.length < 5) {
            usage();
        }
        long seed = Long.parseLong(args[0]);
        int x = Integer.parseInt(args[1]);
        int z = Integer.parseInt(args[2]);
        int y0 = Integer.parseInt(args[3]);
        int y1 = Integer.parseInt(args[4]);
        boolean cornersOnly = args.length > 5 && "--corners".equals(args[5]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        var noises = registries.lookupOrThrow(Registries.NOISE);
        var settings = registries.lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.NETHER);
        RandomState state = RandomState.create(settings.value(), noises, seed);
        DensityFunction finalDensity = state.router().finalDensity();

        System.out.println("SEED " + seed + " COLUMN " + x + "," + z
            + " cell_height=" + CELL_HEIGHT + " min_y=" + MIN_Y);
        for (int y = y0; y <= y1; y++) {
            boolean corner = Math.floorMod(y - MIN_Y, CELL_HEIGHT) == 0;
            if (cornersOnly && !corner) {
                continue;
            }
            double v = finalDensity.compute(new DensityFunction.SinglePointContext(x, y, z));
            // The sign is what the filler reads: `> 0` is the default block
            // and everything else is the fluid picker's answer. Printed
            // beside the value so a column can be read without arithmetic.
            System.out.println("DENSITY y=" + y
                + (corner ? " CORNER" : "       ")
                + " value=" + v
                + " bits=" + Long.toHexString(Double.doubleToRawLongBits(v))
                + " solid=" + (v > 0.0));
        }
    }
}
