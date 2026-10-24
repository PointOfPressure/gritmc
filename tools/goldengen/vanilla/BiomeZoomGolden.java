// BiomeZoomGolden — which quart cell BiomeManager resolves a block to.
//
// `BiomeManager.getBiome` is not a quart lookup: it perturbs the distance to
// each of eight surrounding cells with a seeded fiddle and takes the nearest.
// That is what makes biome borders wobble at block resolution rather than
// stepping along a visible 4-block grid, and the surface system reads biomes
// through it — so a port that hands the raw quart source to surface rules
// generates straight-edged biome transitions.
//
// The trick here is that the biome *table* is irrelevant: handing
// BiomeManager a source that simply records the coordinates it was asked for
// isolates the lookup completely. The returned holder is never used.
//
// Converted to JSON by `ci/biome-zoom-golden.sh`.

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.biome.BiomeManager;

public class BiomeZoomGolden {
    static int[] last = new int[3];
    public static void main(String[] a) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // The source records which quart cell it was asked for, which is
        // exactly the value being ported; the holder it returns is unused.
        for (long seed : new long[]{0L, 20260725L}) {
            BiomeManager bm = new BiomeManager(
                (qx, qy, qz) -> { last[0]=qx; last[1]=qy; last[2]=qz; return null; },
                BiomeManager.obfuscateSeed(seed));
            StringBuilder sb = new StringBuilder();
            for (int x = -8; x < 8; x++)
                for (int z = -8; z < 8; z++) {
                    bm.getBiome(new BlockPos(x, 70, z));
                    sb.append(x).append(',').append(z).append('=')
                      .append(last[0]).append('/').append(last[1]).append('/').append(last[2]).append(' ');
                }
            System.out.println("SEED " + seed + " " + sb.toString().trim());
        }
    }
}
