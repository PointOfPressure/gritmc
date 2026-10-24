package net.minecraft.world.level.levelgen;

import java.io.PrintStream;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeMap;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.ai.village.poi.PoiType;
import net.minecraft.world.entity.npc.villager.VillagerProfession;
import net.minecraft.world.level.block.state.BlockState;

/**
 * The villager professions and the points of interest they work at.
 *
 * <p>Both are code registries rather than datapack ones — unlike the trades,
 * which moved into the datapack in 26.2 — so this needs a JVM where the trade
 * table did not. What comes out:
 *
 * <ul>
 *   <li>every profession's work sound, requested items, secondary POI blocks
 *       and, most usefully, its `tradeSetsByLevel` map. That map is the join
 *       between a profession and the `trade_set/<profession>/level_N` entries
 *       the datapack carries, and it is the only place the two are related —
 *       the naming convention is a convention, not a lookup;
 *   <li>every POI type's `maxTickets`, `validRange` and the **number of block
 *       states** it matches. The states themselves are a set of thousands
 *       (every bed colour and facing), so what is recorded is the count and
 *       the distinct blocks, which is what a server needs to answer "is this
 *       block a job site".
 * </ul>
 *
 * <p>`heldJobSite` and `acquirableJobSite` are `Predicate<Holder<PoiType>>`
 * and are deliberately absent: a predicate cannot be dumped, only sampled, and
 * a sample of two would read as a table. What is dumped instead is the answer
 * for every POI type in the registry, which is the same information and is
 * complete.
 *
 * <p>Run by {@code ci/villager-golden.sh}. Writes to
 * {@code crates/rustmc-server/golden/villager_professions.json}.
 */
public final class VillagerGolden {
    private VillagerGolden() {}

    public static void main(String[] args) throws Exception {
        PrintStream out = new PrintStream(java.nio.file.Files.newOutputStream(Path.of(args[0])), true, "UTF-8");
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        List<String> poiRows = new ArrayList<>();
        List<Identifier> poiNames = new ArrayList<>(BuiltInRegistries.POINT_OF_INTEREST_TYPE.keySet());
        poiNames.sort(java.util.Comparator.comparing(Identifier::toString));
        for (Identifier name : poiNames) {
            PoiType type = BuiltInRegistries.POINT_OF_INTEREST_TYPE.getValue(name);
            TreeMap<String, Integer> byBlock = new TreeMap<>();
            for (BlockState s : type.matchingStates()) {
                byBlock.merge(BuiltInRegistries.BLOCK.getKey(s.getBlock()).toString(), 1, Integer::sum);
            }
            StringBuilder blocks = new StringBuilder();
            for (var e : byBlock.entrySet()) {
                if (blocks.length() > 0) {
                    blocks.append(", ");
                }
                blocks.append('"').append(e.getKey()).append('"');
            }
            poiRows.add(String.format(
                    "{\"name\": \"%s\", \"maxTickets\": %d, \"validRange\": %d,"
                            + " \"states\": %d, \"blocks\": [%s]}",
                    name, type.maxTickets(), type.validRange(), type.matchingStates().size(), blocks));
        }

        List<String> profRows = new ArrayList<>();
        List<Identifier> profNames = new ArrayList<>(BuiltInRegistries.VILLAGER_PROFESSION.keySet());
        profNames.sort(java.util.Comparator.comparing(Identifier::toString));
        for (Identifier name : profNames) {
            VillagerProfession p = BuiltInRegistries.VILLAGER_PROFESSION.getValue(name);

            StringBuilder acquirable = new StringBuilder();
            StringBuilder held = new StringBuilder();
            for (Identifier poi : poiNames) {
                var holder = BuiltInRegistries.POINT_OF_INTEREST_TYPE
                        .get(net.minecraft.resources.ResourceKey.create(
                                net.minecraft.core.registries.Registries.POINT_OF_INTEREST_TYPE, poi))
                        .orElseThrow();
                if (p.acquirableJobSite().test(holder)) {
                    append(acquirable, poi.toString());
                }
                if (p.heldJobSite().test(holder)) {
                    append(held, poi.toString());
                }
            }

            StringBuilder requested = new StringBuilder();
            p.requestedItems().stream()
                    .map(i -> BuiltInRegistries.ITEM.getKey(i).toString())
                    .sorted()
                    .forEach(s -> append(requested, s));
            StringBuilder secondary = new StringBuilder();
            p.secondaryPoi().stream()
                    .map(b -> BuiltInRegistries.BLOCK.getKey(b).toString())
                    .sorted()
                    .forEach(s -> append(secondary, s));

            StringBuilder sets = new StringBuilder();
            for (int level = 1; level <= 5; level++) {
                var key = p.tradeSetsByLevel().get(level);
                if (key != null) {
                    if (sets.length() > 0) {
                        sets.append(", ");
                    }
                    sets.append(String.format("\"%d\": \"%s\"", level, key.identifier()));
                }
            }

            profRows.add(String.format(
                    "{\"name\": \"%s\", \"workSound\": %s, \"acquirableJobSite\": [%s],"
                            + " \"heldJobSite\": [%s], \"requestedItems\": [%s],"
                            + " \"secondaryPoi\": [%s], \"tradeSetsByLevel\": {%s}}",
                    name,
                    p.workSound() == null
                            ? "null"
                            : "\"" + BuiltInRegistries.SOUND_EVENT.getKey(p.workSound()) + "\"",
                    acquirable,
                    held,
                    requested,
                    secondary,
                    sets));
        }

        out.println("{");
        out.println("  \"source\": \"BuiltInRegistries.VILLAGER_PROFESSION and POINT_OF_INTEREST_TYPE, 26.2\",");
        out.println("  \"poiTypes\": [");
        emit(out, poiRows);
        out.println("  ],");
        out.println("  \"professions\": [");
        emit(out, profRows);
        out.println("  ]");
        out.println("}");
        out.close();
    }

    private static void append(StringBuilder sb, String s) {
        if (sb.length() > 0) {
            sb.append(", ");
        }
        sb.append('"').append(s).append('"');
    }

    private static void emit(PrintStream out, List<String> rows) {
        for (int i = 0; i < rows.size(); i++) {
            out.print("    " + rows.get(i));
            out.println(i + 1 < rows.size() ? "," : "");
        }
    }
}
