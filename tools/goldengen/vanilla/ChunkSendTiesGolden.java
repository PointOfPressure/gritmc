package net.minecraft.server.network;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;
import java.util.Random;

import net.minecraft.server.level.ChunkMap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.LevelChunk;

/**
 * Drives the jar's own {@code PlayerChunkSender.collectChunksToSend} — the
 * real private method, over the real {@code LongOpenHashSet} and Guava's real
 * {@code Comparators.least} — through scripted add / drop / collect sequences,
 * and prints what each collect returned, in order. Consumed by
 * {@code crates/rustmc-server/src/chunksender.rs}'s golden test.
 *
 * Nothing about the order is re-implemented here. The two seams are the ones
 * the method reaches outside the sender for: {@code ChunkMap.getChunkToSend},
 * overridden on an instance built with {@code Unsafe.allocateInstance} so no
 * constructor runs, and {@code LevelChunk.getPos}, satisfied by writing
 * {@code ChunkAccess.chunkPos} on an equally unconstructed chunk.
 *
 * Output, one record per line:
 *   S m            new PlayerChunkSender(m == 1)
 *   A x z          markChunkPendingToSend(chunk at x, z)
 *   D x z          dropChunk(null, x, z) — only ever for a pending position,
 *                  so the player is never read
 *   C cx cz q s : x z x z ...
 *                  batchQuota = q; collectChunksToSend(map, (cx, cz)) with
 *                  availability salt s; the positions it returned, in order
 */
public final class ChunkSendTiesGolden {

    /** Availability both sides compute: a quarter of positions are unavailable. */
    static boolean available(int x, int z, int salt) {
        int h = (x * 73856093) ^ (z * 19349663) ^ (salt * 83492791);
        return (h & 3) != 0;
    }

    static final class Map0 extends ChunkMap {
        int salt;
        Map<Long, LevelChunk> chunks;

        Map0() {
            super(null, null, null, null, null, null, null, null, null, null, null, 0, false);
        }

        @Override
        public LevelChunk getChunkToSend(long pos) {
            int x = ChunkPos.getX(pos);
            int z = ChunkPos.getZ(pos);
            return available(x, z, salt) ? chunks.get(pos) : null;
        }
    }

    static sun.misc.Unsafe unsafe;
    static Field chunkPosField;
    static Field batchQuotaField;
    static Method collect;
    static Map<Long, LevelChunk> chunks = new HashMap<>();
    static Map0 map;
    /** The positions this driver has queued and not seen leave, to know when a join has drained. */
    static java.util.Set<Long> mirror = new java.util.HashSet<>();
    static StringBuilder out = new StringBuilder();

    static LevelChunk chunk(int x, int z) throws Exception {
        long k = ChunkPos.pack(x, z);
        LevelChunk c = chunks.get(k);
        if (c == null) {
            c = (LevelChunk) unsafe.allocateInstance(LevelChunk.class);
            chunkPosField.set(c, new ChunkPos(x, z));
            chunks.put(k, c);
        }
        return c;
    }

    static PlayerChunkSender sender(boolean memory) {
        mirror.clear();
        out.append("S ").append(memory ? 1 : 0).append('\n');
        return new PlayerChunkSender(memory);
    }

    static void add(PlayerChunkSender s, int x, int z) throws Exception {
        s.markChunkPendingToSend(chunk(x, z));
        mirror.add(ChunkPos.pack(x, z));
        out.append("A ").append(x).append(' ').append(z).append('\n');
    }

    static void drop(PlayerChunkSender s, int x, int z) {
        s.dropChunk(null, new ChunkPos(x, z));
        mirror.remove(ChunkPos.pack(x, z));
        out.append("D ").append(x).append(' ').append(z).append('\n');
    }

    @SuppressWarnings("unchecked")
    static int collect(PlayerChunkSender s, int cx, int cz, int quota, int salt) throws Exception {
        batchQuotaField.setFloat(s, (float) quota);
        map.salt = salt;
        List<LevelChunk> got = (List<LevelChunk>) collect.invoke(s, map, new ChunkPos(cx, cz));
        out.append("C ").append(cx).append(' ').append(cz).append(' ').append(quota)
           .append(' ').append(salt).append(" :");
        for (LevelChunk c : got) {
            out.append(' ').append(c.getPos().x()).append(' ').append(c.getPos().z());
            mirror.remove(c.getPos().pack());
        }
        out.append('\n');
        return got.size();
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();

        Field uf = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        uf.setAccessible(true);
        unsafe = (sun.misc.Unsafe) uf.get(null);
        chunkPosField = ChunkAccess.class.getDeclaredField("chunkPos");
        chunkPosField.setAccessible(true);
        batchQuotaField = PlayerChunkSender.class.getDeclaredField("batchQuota");
        batchQuotaField.setAccessible(true);
        collect = PlayerChunkSender.class.getDeclaredMethod("collectChunksToSend", ChunkMap.class, ChunkPos.class);
        collect.setAccessible(true);
        map = (Map0) unsafe.allocateInstance(Map0.class);
        map.chunks = chunks;

        // 1. Joins: a whole disc queued in x-major order, drained by repeated
        //    collects. Centres straddle the origin so the zero key (chunk 0,0)
        //    is in play, and radii cross the table's growth points.
        int[][] centres = {{0, 0}, {-1, -1}, {5, -3}, {-40, 17}, {1000, -1000}};
        int[] radii = {2, 4, 7, 11};
        int[] quotas = {-1, 9, 3, 40};  // -1: a memory connection
        int salt = 0;
        for (int[] c : centres) {
            for (int r : radii) {
                for (int q : quotas) {
                    PlayerChunkSender s = sender(q < 0);
                    for (int dx = -r; dx <= r; dx++) {
                        for (int dz = -r; dz <= r; dz++) {
                            if (dx * dx + dz * dz <= r * r + r) {
                                add(s, c[0] + dx, c[1] + dz);
                            }
                        }
                    }
                    for (int round = 0; round < 400 && !mirror.isEmpty(); round++) {
                        collect(s, c[0], c[1], q < 0 ? 9 : q, salt++);
                    }
                }
            }
        }

        // 2. Churn: random adds, drops of pending positions and collects, with
        //    a wandering centre, so growth, shrinking and backward-shift
        //    deletion all move entries between slots.
        Random rnd = new Random(20260924L);
        for (int scen = 0; scen < 30; scen++) {
            boolean memory = scen % 5 == 0;
            PlayerChunkSender s = sender(memory);
            int cx = rnd.nextInt(9) - 4;
            int cz = rnd.nextInt(9) - 4;
            int span = 3 + rnd.nextInt(10);
            for (int op = 0; op < 500; op++) {
                int roll = rnd.nextInt(100);
                if (roll < 3) {
                    cx += rnd.nextInt(3) - 1;
                    cz += rnd.nextInt(3) - 1;
                } else if (roll < 60) {
                    add(s, cx + rnd.nextInt(2 * span + 1) - span, cz + rnd.nextInt(2 * span + 1) - span);
                } else if (roll < 80) {
                    int x = cx + rnd.nextInt(2 * span + 1) - span;
                    int z = cz + rnd.nextInt(2 * span + 1) - span;
                    if (s.isPending(ChunkPos.pack(x, z))) {
                        drop(s, x, z);
                    }
                } else {
                    collect(s, cx, cz, rnd.nextInt(41), salt++);
                }
            }
        }
        System.out.print(out);
    }

    private ChunkSendTiesGolden() {
    }
}
