// VanillaBench — time vanilla's own noise stage, for an apples-to-apples
// comparison against rustmc's.
//
// Why this exists: quoting a remembered "vanilla does ~10 ms a chunk" against
// a number measured on this machine is not a comparison, it is two unrelated
// facts. The only honest version runs both on the same hardware, in the same
// session, doing the same work.
//
// What "the same work" means here is precise. This replicates
// NoiseBasedChunkGenerator.doFill's inner loop:
//
//   new NoiseChunk(...) -> initializeForFirstCellX -> advanceCellX
//     -> selectCellYZ -> updateForY/X/Z -> getInterpolatedState
//
// i.e. density, cell interpolation, the aquifer and ore veins, for every
// block in the chunk. It does NOT write into LevelChunkSections or update
// heightmaps, because that is chunk *storage* rather than generation, and
// rustmc's bench does the equivalent amount of storage work into a Vec.
//
// It also does not run surfaces, carvers, features or structures — the same
// scope rustmc's bench declares.
//
// Declared in vanilla's own package so it can reach `BeardifierMarker` and
// `NoiseChunk`'s protected members directly. That is legal because the server
// jar is on the classpath rather than the module path, and it beats
// reflection here: a reflective call per block would be timing the reflection
// rather than the generator.

package net.minecraft.world.level.levelgen;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.synth.NormalNoise;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;

public final class VanillaBench {

    public static void main(String[] args) {
        long seed = args.length > 0 ? Long.parseLong(args[0]) : 20260725L;
        int count = args.length > 1 ? Integer.parseInt(args[1]) : 64;
        int warmup = args.length > 2 ? Integer.parseInt(args[2]) : 32;

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderGetter<NormalNoise.NoiseParameters> noises =
            registries.lookupOrThrow(Registries.NOISE);
        var settingsHolder = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD);
        NoiseGeneratorSettings settings = settingsHolder.value();
        NoiseSettings noiseSettings = settings.noiseSettings();

        long routerStart = System.nanoTime();
        RandomState state = RandomState.create(settings, noises, seed);
        double routerMs = (System.nanoTime() - routerStart) / 1e6;

        // NoiseBasedChunkGenerator.createFluidPicker, rebuilt here because it
        // is private: lava below min(-54, seaLevel), water up to seaLevel.
        int seaLevel = settings.seaLevel();
        Aquifer.FluidStatus lava = new Aquifer.FluidStatus(-54, Blocks.LAVA.defaultBlockState());
        Aquifer.FluidStatus sea = new Aquifer.FluidStatus(seaLevel, settings.defaultFluid());
        Aquifer.FluidPicker picker =
            (x, y, z) -> y < Math.min(-54, seaLevel) ? lava : sea;

        // The JIT needs to see this loop a few thousand times before it is
        // representative; without a warmup the first chunks are interpreted
        // and the number is meaningless (and flattering to Rust).
        long sink = fill(noiseSettings, settings, state, picker, warmup);

        long start = System.nanoTime();
        sink += fill(noiseSettings, settings, state, picker, count);
        double elapsed = (System.nanoTime() - start) / 1e9;

        System.out.printf("router build      %.1f ms (once per world)%n", routerMs);
        System.out.printf("warmup chunks     %d%n", warmup);
        System.out.printf("chunks            %d%n", count);
        System.out.printf("total             %.3f s%n", elapsed);
        System.out.printf("per chunk         %.2f ms%n", elapsed * 1000.0 / count);
        System.out.printf("throughput        %.1f chunks/s%n", count / elapsed);
        System.out.printf("                  %.1f M blocks/s%n", count * 98304.0 / elapsed / 1e6);
        System.out.println("(noise stage only: density, aquifer, ore veins, fill — no surfaces,");
        System.out.println(" carvers, features or structures, and no disk IO)");
        System.err.println("(sanity: " + sink + " non-null states)");
    }

    /// Generates `count` chunks in a square around the origin and returns a
    /// checksum, so the loop cannot be optimised away.
    private static long fill(
        NoiseSettings noiseSettings,
        NoiseGeneratorSettings settings,
        RandomState state,
        Aquifer.FluidPicker picker,
        int count
    ) {
        int cellWidth = noiseSettings.getCellWidth();
        int cellHeight = noiseSettings.getCellHeight();
        int minY = noiseSettings.minY();
        int cellMinY = Math.floorDiv(minY, cellHeight);
        int cellCountY = Math.floorDiv(noiseSettings.height(), cellHeight);
        int cellCountXZ = 16 / cellWidth;
        BlockState defaultBlock = settings.defaultBlock();

        int side = (int) Math.ceil(Math.sqrt(count));
        long sink = 0;
        int done = 0;

        for (int cz = 0; cz < side && done < count; cz++) {
            for (int cx = 0; cx < side && done < count; cx++) {
                int chunkMinX = (cx - side / 2) * 16;
                int chunkMinZ = (cz - side / 2) * 16;
                NoiseChunk chunk = new NoiseChunk(
                    cellCountXZ, state, chunkMinX, chunkMinZ, noiseSettings,
                    DensityFunctions.BeardifierMarker.INSTANCE, settings, picker, Blender.empty());

                chunk.initializeForFirstCellX();
                for (int cellX = 0; cellX < cellCountXZ; cellX++) {
                    chunk.advanceCellX(cellX);
                    for (int cellZ = 0; cellZ < cellCountXZ; cellZ++) {
                        for (int cellY = cellCountY - 1; cellY >= 0; cellY--) {
                            chunk.selectCellYZ(cellY, cellZ);
                            for (int yInCell = cellHeight - 1; yInCell >= 0; yInCell--) {
                                int posY = (cellMinY + cellY) * cellHeight + yInCell;
                                chunk.updateForY(posY, (double) yInCell / cellHeight);
                                for (int xInCell = 0; xInCell < cellWidth; xInCell++) {
                                    int posX = chunkMinX + cellX * cellWidth + xInCell;
                                    chunk.updateForX(posX, (double) xInCell / cellWidth);
                                    for (int zInCell = 0; zInCell < cellWidth; zInCell++) {
                                        int posZ = chunkMinZ + cellZ * cellWidth + zInCell;
                                        chunk.updateForZ(posZ, (double) zInCell / cellWidth);
                                        BlockState st = chunk.getInterpolatedState();
                                        if (st == null) {
                                            st = defaultBlock;
                                        }
                                        sink += st.isAir() ? 0 : 1;
                                    }
                                }
                            }
                        }
                    }
                    // End of a cell-X column. doFill swaps the slices here;
                    // without it every column past the first interpolates
                    // against a stale one. It costs a pointer swap, so it
                    // does not distort the timing — but a benchmark of the
                    // wrong computation is not a benchmark.
                    chunk.swapSlices();
                }
                chunk.stopInterpolation();
                done++;
            }
        }
        return sink;
    }

    private VanillaBench() {}
}
