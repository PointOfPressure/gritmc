// EndPodiumGolden — every block `EndPodiumFeature.place` writes, in order.
//
// The exit portal is built by `EnderDragonFight.spawnExitPortal`, which runs
// only with a player in the End, so no chunk harvest holds one. This calls the
// feature itself on a level that records each `setBlock` and answers air to
// every read, for both `active` values and two origins. The port's
// `podium_writes` must reproduce the list write for write.
//
// Converted to JSON by `ci/end-podium-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

import net.minecraft.SharedConstants;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.levelgen.feature.EndPodiumFeature;
import net.minecraft.world.level.levelgen.feature.FeaturePlaceContext;
import net.minecraft.world.level.levelgen.feature.configurations.NoneFeatureConfiguration;

public final class EndPodiumGolden {

    private static WorldGenLevel recording(List<String> writes) {
        return (WorldGenLevel) java.lang.reflect.Proxy.newProxyInstance(
            EndPodiumGolden.class.getClassLoader(),
            new Class<?>[]{WorldGenLevel.class},
            (proxy, method, args) -> {
                String name = method.getName();
                if (name.equals("setBlock") && args.length >= 3) {
                    BlockPos p = (BlockPos) args[0];
                    BlockState s = (BlockState) args[1];
                    writes.add("[" + p.getX() + ", " + p.getY() + ", " + p.getZ() + ", \""
                        + BlockStateParser.serialize(s) + "\"]");
                    return true;
                }
                if (name.equals("getBlockState")) {
                    return Blocks.AIR.defaultBlockState();
                }
                if (name.equals("destroyBlock")) {
                    return true;
                }
                if (name.equals("hashCode")) {
                    return System.identityHashCode(proxy);
                }
                if (name.equals("equals")) {
                    return proxy == args[0];
                }
                if (name.equals("toString")) {
                    return "recording";
                }
                throw new UnsupportedOperationException("EndPodiumFeature read WorldGenLevel." + name);
            });
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        int[][] origins = {{0, 64, 0}, {5, 70, -3}};
        List<String> cases = new ArrayList<>();
        for (int[] o : origins) {
            for (boolean active : new boolean[]{false, true}) {
                List<String> writes = new ArrayList<>();
                BlockPos origin = new BlockPos(o[0], o[1], o[2]);
                new EndPodiumFeature(active).place(new FeaturePlaceContext<>(
                    Optional.empty(), recording(writes), null, RandomSource.create(0L), origin,
                    NoneFeatureConfiguration.INSTANCE));
                cases.add("{\"origin\": [" + o[0] + ", " + o[1] + ", " + o[2] + "], \"active\": "
                    + active + ", \"writes\": [" + String.join(", ", writes) + "]}");
            }
        }
        System.out.print("{\n  \"rng\": \"end_podium\",\n  \"cases\": [\n    "
            + String.join(",\n    ", cases) + "\n  ]\n}\n");
    }
}
