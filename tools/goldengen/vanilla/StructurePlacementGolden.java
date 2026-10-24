// StructurePlacementGolden — which chunks each structure set may start in.
//
// Structures are the largest single item left in the whole-world diff
// (BENCH.md: ~2,400 of 5,374 remaining mismatches are one mineshaft). This
// dumps the *floor* of that work: `StructurePlacement.isStructureChunk` over
// a square of chunks, for every structure set in the registry, at several
// seeds.
//
// It is worth dumping on its own because it is a pure function of the seed
// and the chunk position — no world, no biomes, no pieces — so the port can
// be gated before a single structure block exists. A generator that builds
// the right pieces in the wrong chunks is not closer than one that builds
// nothing.
//
// # Why the whole three-part test and not just the grid
//
// `isStructureChunk` is `isPlacementChunk && applyAdditionalChunkRestrictions
// && applyInteractionsWithOtherStructures`. Dumping only the grid would be
// easier and would silently drop the frequency reducers — of which there are
// four, and the one the mineshafts use *ignores the salt*, which no test of
// the grid alone can notice. So the dump records the grid, the full answer,
// and the potential chunk each region picks, and the gate compares all three:
// three columns that fail independently, per ORACLE.md rule 11.
//
// # What this dump does NOT cover, and why
//
// `isStructureChunk` takes a `ChunkGeneratorStructureState`, and building a
// real one is not possible from `VanillaRegistries.createLookup()`:
// `createForNormal` filters the sets through `hasBiomesForStructureSet`,
// which dereferences biome tags, and the lookup throws
//
//     Tag TagKey[minecraft:worldgen/biome / minecraft:has_structure/...]
//     can't be dereferenced during construction
//
// Rather than stand in for the state — the tempting move, and the one
// ORACLE.md §5b rule 8 warns about — this dumps the two parts that need only
// the seed, and says so:
//
//   getPotentialStructureChunk(seed, x, z)     -> the grid
//   applyAdditionalChunkRestrictions(x, z, seed) -> the frequency coin
//
// Together those are `isStructureChunk` in full for **19 of the 20** sets,
// because only `pillager_outposts` has an exclusion zone. That one set's
// third test is therefore covered by unit tests and not by this vector, which
// is a real gap and is named in the gate rather than papered over.
//
// Converted to JSON by `ci/structure-placement-golden.sh`.

package net.minecraft.world.level.levelgen.structure;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.levelgen.structure.placement.StructurePlacement;

public final class StructurePlacementGolden {

    /// The chunk square dumped per seed, centred on the origin. 96x96 is
    /// nine regions of 32 and two of 34, so every set with a grid gets
    /// several regions rather than one — a single region cannot tell a
    /// correct jitter from a constant.
    private static final int RADIUS = 48;

    private static final long[] SEEDS = {0L, 1L, 20260722L, -4242L};

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        HolderLookup.RegistryLookup<StructureSet> sets =
            registries.lookupOrThrow(Registries.STRUCTURE_SET);

        List<String> cases = new ArrayList<>();
        // Sorted by name: `listElements()` has no stable order between JVM
        // runs, and an unstable order here would make the file churn and hide
        // a real change inside the churn (ORACLE.md rule 10, learned from
        // HashOrderGolden).
        List<ResourceKey<StructureSet>> keys = new ArrayList<>();
        sets.listElementIds().forEach(keys::add);
        keys.sort((a, b) -> a.identifier().toString().compareTo(b.identifier().toString()));

        for (long seed : SEEDS) {
            for (ResourceKey<StructureSet> key : keys) {
                StructureSet set = sets.getOrThrow(key).value();
                cases.add(dump(key.identifier().toString(), seed, set.placement()));
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"structure_placement\",\n");
        out.append("  \"radius\": ").append(RADIUS).append(",\n  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i)).append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static String dump(String name, long seed, StructurePlacement placement) {
        List<String> structureChunks = new ArrayList<>();
        List<String> placementChunks = new ArrayList<>();
        List<String> potential = new ArrayList<>();
        boolean randomSpread =
            placement instanceof net.minecraft.world.level.levelgen.structure.placement
                .RandomSpreadStructurePlacement;

        for (int x = -RADIUS; x <= RADIUS; x++) {
            for (int z = -RADIUS; z <= RADIUS; z++) {
                if (!randomSpread) {
                    continue;
                }
                var spread = (net.minecraft.world.level.levelgen.structure.placement
                    .RandomSpreadStructurePlacement) placement;
                var pos = spread.getPotentialStructureChunk(seed, x, z);
                boolean onGrid = pos.x() == x && pos.z() == z;
                if (onGrid) {
                    placementChunks.add("[" + x + ", " + z + "]");
                    // The frequency coin is only asked where the grid already
                    // said yes, exactly as `isStructureChunk` asks it — the
                    // conjunction short-circuits, and a dump that evaluated
                    // both unconditionally would be recording a function
                    // vanilla never computes.
                    if (placement.applyAdditionalChunkRestrictions(x, z, seed)) {
                        structureChunks.add("[" + x + ", " + z + "]");
                    }
                }
            }
        }
        // The picked chunk for a handful of regions, which pins the jitter
        // even where the frequency coin rejects every chunk in sight.
        if (randomSpread) {
            var spread = (net.minecraft.world.level.levelgen.structure.placement
                .RandomSpreadStructurePlacement) placement;
            for (int rx = -3; rx <= 3; rx++) {
                for (int rz = -3; rz <= 3; rz++) {
                    int x = rx * spread.spacing();
                    int z = rz * spread.spacing();
                    var pos = spread.getPotentialStructureChunk(seed, x, z);
                    potential.add("[" + x + ", " + z + ", " + pos.x() + ", " + pos.z() + "]");
                }
            }
        }

        return "{\"set\": \"" + name + "\", \"seed\": \"" + seed + "\""
             + ", \"random_spread\": " + randomSpread
             + ", \"structure_chunks\": [" + String.join(", ", structureChunks) + "]"
             + ", \"placement_chunks\": [" + String.join(", ", placementChunks) + "]"
             + ", \"potential\": [" + String.join(", ", potential) + "]}";
    }

}
