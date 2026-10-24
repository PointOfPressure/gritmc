// ChunkLevelGolden — the ticket-level table, read out of a live JVM.
//
// `ChunkLevel` holds three literal constants and two derived ones:
//
//     FULL_CHUNK_LEVEL     = 33     literal
//     BLOCK_TICKING_LEVEL  = 32     literal
//     ENTITY_TICKING_LEVEL = 31     literal
//     RADIUS_AROUND_FULL_CHUNK = ChunkPyramid.GENERATION_PYRAMID
//                                    .getStepTo(FULL)
//                                    .accumulatedDependencies()
//                                    .getRadius()
//     MAX_LEVEL = 33 + RADIUS_AROUND_FULL_CHUNK
//
// The three literals are readable off `javap`. The other two are not: they
// fall out of the whole generation pyramid's dependency radii, and the only
// honest way to get them is to let the class initialise and ask it. A port
// that guesses `MAX_LEVEL = 33` — which it was in older versions — puts the
// unload boundary in the wrong place and nothing in a value test would say
// so.
//
// The dump also walks the level -> FullChunkStatus mapping over the whole
// band that matters, so the port's table is compared against the real
// `fullStatus` rather than against a reading of its bytecode.
//
// Converted to JSON by `ci/chunklevel-golden.sh`.

package net.minecraft.server.level;

import java.util.ArrayList;
import java.util.List;

public final class ChunkLevelGolden {

    public static void main(String[] args) {
        // `ChunkLevel`'s static initialiser reaches ChunkPyramid, which
        // reaches every ChunkStatus. Bootstrap first so that a missing
        // registry surfaces here rather than as a null two frames down —
        // and `tryDetectVersion` before that, because `Bootstrap.bootStrap`
        // reaches `DataFixers`, whose static initialiser throws
        // `Game version not set` without it.
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"full_chunk_level\": 33,\n");
        out.append("  \"block_ticking_level\": 32,\n");
        out.append("  \"entity_ticking_level\": 31,\n");
        out.append("  \"radius_around_full_chunk\": ")
           .append(ChunkLevel.RADIUS_AROUND_FULL_CHUNK).append(",\n");
        out.append("  \"max_level\": ").append(ChunkLevel.MAX_LEVEL).append(",\n");

        // byStatus over every FullChunkStatus, in ordinal order — the same
        // order the port's enum has to be in for its discriminants to mean
        // anything.
        List<String> byStatus = new ArrayList<>();
        for (FullChunkStatus s : FullChunkStatus.values()) {
            byStatus.add("    {\"status\": \"" + s.name() + "\", \"ordinal\": " + s.ordinal()
                + ", \"level\": " + ChunkLevel.byStatus(s) + "}");
        }
        out.append("  \"by_status\": [\n").append(String.join(",\n", byStatus)).append("\n  ],\n");

        // fullStatus and the three predicates across the whole interesting
        // band, two clear of MAX_LEVEL in both directions.
        List<String> levels = new ArrayList<>();
        for (int l = 28; l <= ChunkLevel.MAX_LEVEL + 2; l++) {
            levels.add("    {\"level\": " + l
                + ", \"full_status\": \"" + ChunkLevel.fullStatus(l).name() + "\""
                + ", \"entity_ticking\": " + ChunkLevel.isEntityTicking(l)
                + ", \"block_ticking\": " + ChunkLevel.isBlockTicking(l)
                + ", \"loaded\": " + ChunkLevel.isLoaded(l) + "}");
        }
        out.append("  \"levels\": [\n").append(String.join(",\n", levels)).append("\n  ]\n");
        out.append("}\n");
        System.out.print(out);
    }

    private ChunkLevelGolden() {
    }
}
