package net.minecraft.world.level.dimension.end;

import it.unimi.dsi.fastutil.objects.ObjectArrayList;
import java.util.List;
import net.minecraft.util.RandomSource;
import net.minecraft.util.Util;

/**
 * The 20 return gateways `EnderDragonFight` spawns, one per dragon kill.
 *
 * Order: the fight's `init` shuffles 0..19 with `Util.shuffle` on
 * `RandomSource.createThreadLocalInstance(worldSeed)` (both called here, from
 * the jar), and `spawnNewGateway()` removes from the END of the list.
 * Position: `spawnNewGateway()`'s private expression, in Java's `Math`, so
 * the Rust port is held to the JVM's cos/sin and floor, not its own.
 * Writes JSON to stdout: data/vanilla/end_gateway_ring.json.
 */
public final class GatewayRingGolden {
    public static void main(String[] args) {
        long[] seeds = {20260722L, 12345L, 31415926L, 0L, -1L, Long.MAX_VALUE, Long.MIN_VALUE, -4172144997902289642L};
        StringBuilder out = new StringBuilder("{\n  \"positions\": [");
        for (int i = 0; i < 20; i++) {
            int x = (int) Math.floor(96.0 * Math.cos(2.0 * (-Math.PI + 0.15707963267948966 * i)));
            int z = (int) Math.floor(96.0 * Math.sin(2.0 * (-Math.PI + 0.15707963267948966 * i)));
            out.append(i == 0 ? "" : ", ").append("[").append(x).append(", 75, ").append(z).append("]");
        }
        out.append("],\n  \"spawn_order\": [");
        for (int s = 0; s < seeds.length; s++) {
            List<Integer> list = new ObjectArrayList<>();
            for (int i = 0; i < 20; i++) list.add(i);
            Util.shuffle(list, RandomSource.createThreadLocalInstance(seeds[s]));
            out.append(s == 0 ? "\n    " : ",\n    ").append("{\"seed\": \"").append(seeds[s]).append("\", \"order\": [");
            for (int k = 0; k < 20; k++) {
                out.append(k == 0 ? "" : ", ").append(list.get(19 - k));
            }
            out.append("]}");
        }
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
