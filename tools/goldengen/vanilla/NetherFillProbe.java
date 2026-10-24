// NetherFillProbe — run vanilla's own nether noise fill for one chunk and
// print the block it writes in one column.
//
// # Why this exists, and why it is not the density probe
//
// `NetherDensityProbe` proved our nether `finalDensity` bit-for-bit identical
// to vanilla's at every point sampled, including all eight lattice corners of
// the disputed cell. It cannot go further, because it evaluates through a
// `SinglePointContext`, and a `DensityFunctions$Marker` of type `Interpolated`
// delegates `compute` to its wrapped function when the context is not the
// `NoiseChunk` — so what it prints is the value the cell lattice is built
// *from*, never the trilinear blend the filler actually writes blocks by.
//
// That leaves exactly one question the region files cannot answer: does our
// FILL agree with vanilla's? An exact density can still produce a different
// world if the interpolation lattice, its origin, or the blend differ. This
// answers it by running vanilla's fill and printing the column.
//
// It has to be built BEFORE porting any nether feature. The nether's 24,220
// twin-masked mismatches are almost entirely order-invariant, our nether
// decoration is missing every structure and refuses 3,437 `basalt_pillar`s,
// and the temptation is to port features and re-measure. If the fill also
// disagrees, that work would bury a terrain bug under a feature list and the
// number would improve for the wrong reason.
//
// # It drives vanilla's loop; it does not reimplement it
//
// Every number comes from vanilla. What is transcribed is the *loop*, read out
// of `NoiseBasedChunkGenerator.doFill`'s bytecode rather than remembered:
//
//   ns          = settings.noiseSettings().clampToHeightAccessor(accessor)
//   minCellY    = Mth.floorDiv(ns.minY(), ns.getCellHeight())
//   cellCountY  = Mth.floorDiv(ns.height(), ns.getCellHeight())
//   cellCountXZ = 16 / ns.getCellWidth()                       (NoiseChunk.forChunk)
//
//   initializeForFirstCellX()
//   for cx in 0..cellCountXZ            advanceCellX(cx)
//     for cz in 0..cellCountXZ
//       for cy in cellCountY-1 down to 0    selectCellYZ(cy, cz)
//         for yy in cellHeight-1 down to 0
//           y = (minCellY + cy) * cellHeight + yy               updateForY(y, yy/cellHeight)
//           for xx in 0..cellWidth
//             x = minBlockX + cx*cellWidth + xx                 updateForX(x, xx/cellWidth)
//             for zz in 0..cellWidth
//               z = minBlockZ + cz*cellWidth + zz               updateForZ(z, zz/cellWidth)
//               state = getInterpolatedState()  ?: defaultBlock
//     swapSlices()
//   stopInterpolation()
//
// The `?: defaultBlock` is doFill's own substitution and is the only branch of
// its body reproduced here — the rest of that body writes sections, updates
// heightmaps and marks fluid positions, none of which changes the state.
//
// # Two private members, reached the way GoldenDump already reaches Marker
//
// `NoiseChunk.getInterpolatedState()` is protected and
// `NoiseBasedChunkGenerator.createFluidPicker(..)` is private static. Both are
// taken with `setAccessible`, which is permitted because the server jar is on
// the classpath and not the module path. Using vanilla's own `createFluidPicker`
// matters more than the convenience: the picker decides lava against air below
// sea level, and a reconstructed one could agree with itself while both it and
// the port were wrong. That is the mistake `AquiferProbe` was written to avoid.
//
// # The one thing supplied rather than read
//
// `clampToHeightAccessor` needs a `LevelHeightAccessor`, which in the server
// comes from the `ChunkAccess`. Building a `ProtoChunk` needs a level, so the
// accessor is supplied inline instead: the nether's dimension range, min y 0
// and height 256, which is data and not an algorithm — a vanilla nether chunk
// on disk carries sixteen sections. The noise settings' own 0..128 is narrower,
// so the clamp returns the noise range either way; passing the dimension range
// is what the server does and is the honest input.
import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import java.lang.reflect.Method;

public final class NetherFillProbe {

    /// `DimensionType` gives the nether y 0..255 — measured, a vanilla nether
    /// chunk on disk has sixteen sections. See the header note.
    private static final int DIM_MIN_Y = 0;
    private static final int DIM_HEIGHT = 256;

    private static void usage() {
        System.err.println("usage: NetherFillProbe <seed> <x> <z> <y0> <y1> [--chunk]");
        System.err.println("  runs vanilla's nether noise fill for the chunk containing");
        System.err.println("  (x, z) and prints the block it writes at each y in [y0, y1]");
        System.err.println("  --chunk  print every non-air block of the WHOLE chunk instead,");
        System.err.println("           as `BLOCK x y z name`, for a full-chunk comparison");
        System.exit(2);
    }

    public static void main(String[] args) throws Exception {
        if (args.length < 5 || args.length > 6) {
            usage();
        }
        boolean wholeChunk = args.length == 6 && "--chunk".equals(args[5]);
        long seed = Long.parseLong(args[0]);
        int targetX = Integer.parseInt(args[1]);
        int targetZ = Integer.parseInt(args[2]);
        int y0 = Integer.parseInt(args[3]);
        int y1 = Integer.parseInt(args[4]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        var noises = registries.lookupOrThrow(Registries.NOISE);
        var settingsHolder = registries.lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.NETHER);
        NoiseGeneratorSettings settings = settingsHolder.value();
        RandomState randomState = RandomState.create(settings, noises, seed);

        Method pickerFactory = Class
            .forName("net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator")
            .getDeclaredMethod("createFluidPicker", NoiseGeneratorSettings.class);
        pickerFactory.setAccessible(true);
        Aquifer.FluidPicker fluidPicker = (Aquifer.FluidPicker) pickerFactory.invoke(null, settings);

        // `DensityFunctions$BeardifierMarker` is package-private, so its
        // `INSTANCE` comes the same way the two members above do. It is the
        // "no structures" beardifier — its `compute` returns 0 — which is
        // what a fresh chunk with no structure starts gets anyway, and this
        // probe deliberately asks about terrain and not about structures.
        Class<?> markerClass = Class
            .forName("net.minecraft.world.level.levelgen.DensityFunctions$BeardifierMarker");
        var instanceField = markerClass.getDeclaredField("INSTANCE");
        instanceField.setAccessible(true);
        Object beardifier = instanceField.get(null);

        LevelHeightAccessor accessor = new LevelHeightAccessor() {
            @Override public int getHeight() { return DIM_HEIGHT; }
            @Override public int getMinY() { return DIM_MIN_Y; }
        };
        NoiseSettings ns = settings.noiseSettings().clampToHeightAccessor(accessor);
        int cellWidth = ns.getCellWidth();
        int cellHeight = ns.getCellHeight();
        int minCellY = Mth.floorDiv(ns.minY(), cellHeight);
        int cellCountY = Mth.floorDiv(ns.height(), cellHeight);
        int cellCountXZ = 16 / cellWidth;

        int minBlockX = (targetX >> 4) << 4;
        int minBlockZ = (targetZ >> 4) << 4;

        // The constructor's beardifier parameter is the package-private
        // interface `DensityFunctions$BeardifierOrMarker`, so the NoiseChunk
        // is constructed reflectively too rather than by naming the type.
        Class<?> beardifierType = Class
            .forName("net.minecraft.world.level.levelgen.DensityFunctions$BeardifierOrMarker");
        var noiseChunkCtor = NoiseChunk.class.getDeclaredConstructor(
            int.class, RandomState.class, int.class, int.class, NoiseSettings.class,
            beardifierType, NoiseGeneratorSettings.class,
            Aquifer.FluidPicker.class, Blender.class);
        noiseChunkCtor.setAccessible(true);
        NoiseChunk noiseChunk = (NoiseChunk) noiseChunkCtor.newInstance(
            cellCountXZ, randomState, minBlockX, minBlockZ, ns,
            beardifier, settings, fluidPicker, Blender.empty());

        Method interpolatedState = NoiseChunk.class.getDeclaredMethod("getInterpolatedState");
        interpolatedState.setAccessible(true);

        String[] column = new String[ns.height()];
        // `--chunk` keeps the whole 16 x height x 16 volume so the comparison
        // can be the whole chunk rather than one column. One column proved the
        // fill agrees where the residual pointed; a chunk is what turns that
        // into a claim about the stage.
        String[][][] volume = wholeChunk ? new String[16][ns.height()][16] : null;
        BlockState defaultBlock = settings.defaultBlock();

        noiseChunk.initializeForFirstCellX();
        for (int cx = 0; cx < cellCountXZ; cx++) {
            noiseChunk.advanceCellX(cx);
            for (int cz = 0; cz < cellCountXZ; cz++) {
                for (int cy = cellCountY - 1; cy >= 0; cy--) {
                    noiseChunk.selectCellYZ(cy, cz);
                    for (int yy = cellHeight - 1; yy >= 0; yy--) {
                        int y = (minCellY + cy) * cellHeight + yy;
                        noiseChunk.updateForY(y, (double) yy / (double) cellHeight);
                        for (int xx = 0; xx < cellWidth; xx++) {
                            int x = minBlockX + cx * cellWidth + xx;
                            noiseChunk.updateForX(x, (double) xx / (double) cellWidth);
                            for (int zz = 0; zz < cellWidth; zz++) {
                                int z = minBlockZ + cz * cellWidth + zz;
                                noiseChunk.updateForZ(z, (double) zz / (double) cellWidth);
                                BlockState state = (BlockState) interpolatedState.invoke(noiseChunk);
                                if (state == null) {
                                    state = defaultBlock;
                                }
                                int index = y - ns.minY();
                                if (x == targetX && z == targetZ
                                        && index >= 0 && index < column.length) {
                                    column[index] = state.toString();
                                }
                                if (volume != null && index >= 0 && index < ns.height()) {
                                    volume[x - minBlockX][index][z - minBlockZ] =
                                        state.toString();
                                }
                            }
                        }
                    }
                }
            }
            noiseChunk.swapSlices();
        }
        noiseChunk.stopInterpolation();

        System.out.println("SEED " + seed + " COLUMN " + targetX + "," + targetZ
            + " chunk=(" + (targetX >> 4) + "," + (targetZ >> 4) + ")"
            + " cell=" + cellWidth + "x" + cellHeight
            + " minCellY=" + minCellY + " cellCountY=" + cellCountY
            + " noise_min_y=" + ns.minY() + " noise_height=" + ns.height());
        if (volume != null) {
            long nonAir = 0;
            for (int lx = 0; lx < 16; lx++) {
                for (int index = 0; index < ns.height(); index++) {
                    for (int lz = 0; lz < 16; lz++) {
                        String state = volume[lx][index][lz];
                        if (state == null || state.contains("minecraft:air")) {
                            continue;
                        }
                        nonAir++;
                        System.out.println("BLOCK " + (minBlockX + lx) + " "
                            + (ns.minY() + index) + " " + (minBlockZ + lz) + " " + state);
                    }
                }
            }
            System.out.println("NONAIR " + nonAir + " of " + (16 * ns.height() * 16));
            return;
        }
        for (int y = y1; y >= y0; y--) {
            int index = y - ns.minY();
            String state = index >= 0 && index < column.length ? column[index] : "OUT_OF_RANGE";
            System.out.println("FILL y=" + y + " " + state);
        }
    }
}
