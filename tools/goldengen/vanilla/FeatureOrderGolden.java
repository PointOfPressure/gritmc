// FeatureOrderGolden — the global feature order, from vanilla's own sorter.
//
// Every feature in the world is seeded with `setFeatureSeed(decorationSeed,
// indexWithinStep, step)`, and that index is a position in one global ordering
// computed once from the entire biome registry. So this ordering decides the
// seed of every tree, ore and lake, and it is a topological sort with three
// separate order-sensitive steps inside it — first-encounter numbering, a
// (step, index) comparator, and a reversed post-order DFS.
//
// `FeatureSorter.buildFeaturesPerStep` is public and static, so this calls the
// real thing rather than reimplementing it, and dumps the resulting per-step
// lists as registry names.
//
// The biome list fed to it is part of the answer, and this harness had it
// **wrong** for its first three days. Features are numbered in first-encounter
// order across that list, so both which biomes are in it and what order they
// come in change the node ids and can change the sort.
//
// It is not the biome registry. `javap -c` of `ChunkGenerator`'s constructor
// gives
//
//     FeatureSorter.buildFeaturesPerStep(
//         List.copyOf(biomeSource.possibleBiomes()), ..., true)
//
// so the list is **the biome source's own possible set**, per dimension. That
// differs from the registry twice over: it holds only the biomes that
// dimension can produce, and it is ordered by first appearance in the climate
// parameter list rather than by registration.
//
// Its order is nonetheless deterministic, which is worth saying because the
// registry's is not. `BiomeSource.possibleBiomes` collects through
// `ImmutableSet.toImmutableSet()`, and Guava's ImmutableSet iterates in
// insertion order; `HolderLookup.listElements()` has no such guarantee and
// was observed changing between two runs of the same JVM.
//
// Converted to JSON by `ci/feature-order-golden.sh`.

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.levelgen.SurfaceChunkGolden;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;

public final class FeatureOrderGolden {

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        HolderLookup.RegistryLookup<Biome> biomes = lookup.lookupOrThrow(Registries.BIOME);
        HolderLookup.RegistryLookup<PlacedFeature> placed = lookup.lookupOrThrow(Registries.PLACED_FEATURE);

        // The biome source's possible set, in its order — what
        // `ChunkGenerator` actually hands the sorter. Not `biomes.listElements()`,
        // which is a different set in an unstable order; see the header.
        //
        // **Per dimension**, because the ordering is: `args[0]` selects it and
        // defaults to the overworld so the existing script keeps its meaning.
        // The nether's five biomes are not in the overworld's 55, so a nether
        // feature numbered against the overworld's ordering gets a seed from
        // a list it is not in — which is the same class of fault as oracle
        // fault 5, one dimension over.
        boolean nether = args.length > 0 && args[0].equals("nether");
        MultiNoiseBiomeSource biomeSource = MultiNoiseBiomeSource.createFromList(
            nether ? SurfaceChunkGolden.netherBiomes(lookup)
                   : SurfaceChunkGolden.overworldBiomes(lookup));
        List<Holder<Biome>> biomeList = List.copyOf(biomeSource.possibleBiomes());

        // The input, dumped alongside the output. Without it a mismatch says
        // only "the sort differs" and cannot distinguish a wrong sort from a
        // wrong reading of the biome data.
        StringBuilder sources = new StringBuilder();
        for (int i = 0; i < biomeList.size(); i++) {
            Holder<Biome> biome = biomeList.get(i);
            sources.append(i == 0 ? "" : ",\n");
            sources.append("    {\"biome\": \"")
                   .append(biome.unwrapKey().map(k -> k.identifier().toString()).orElse("?"))
                   .append("\", \"steps\": [");
            List<net.minecraft.core.HolderSet<PlacedFeature>> steps =
                biome.value().getGenerationSettings().features();
            for (int step = 0; step < steps.size(); step++) {
                sources.append(step == 0 ? "" : ", ").append('[');
                int n = 0;
                for (Holder<PlacedFeature> feature : steps.get(step)) {
                    sources.append(n++ == 0 ? "" : ", ").append('"').append(nameOf(placed, feature)).append('"');
                }
                sources.append(']');
            }
            sources.append("]}");
        }

        List<FeatureSorter.StepFeatureData> perStep = FeatureSorter.buildFeaturesPerStep(
            biomeList, biome -> biome.value().getGenerationSettings().features(), true);

        StringBuilder out = new StringBuilder();
        out.append("{\n  \"rng\": \"feature_order\",\n");
        out.append("  \"dimension\": \"").append(nether ? "nether" : "overworld").append("\",\n");
        out.append("  \"biomeCount\": ").append(biomeList.size()).append(",\n");
        out.append("  \"sources\": [\n").append(sources).append("\n  ],\n");
        out.append("  \"perStep\": [\n");
        for (int step = 0; step < perStep.size(); step++) {
            out.append("    [");
            List<PlacedFeature> features = perStep.get(step).features();
            for (int i = 0; i < features.size(); i++) {
                out.append(i == 0 ? "" : ", ").append('"')
                   .append(nameOfValue(placed, features.get(i))).append('"');
            }
            out.append(']').append(step + 1 < perStep.size() ? "," : "").append('\n');
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    private static String nameOf(HolderLookup.RegistryLookup<PlacedFeature> lookup, Holder<PlacedFeature> holder) {
        return holder.unwrapKey().map(k -> k.identifier().toString())
            .orElseGet(() -> nameOfValue(lookup, holder.value()));
    }

    /// The sorter hands back values, not holders, so the name has to be found
    /// by identity. Two registry entries with equal *content* are one node to
    /// the sorter — `Object2IntOpenHashMap` keys on `PlacedFeature`'s record
    /// equality — so a name lookup by equality could report the wrong one.
    /// Identity is what the sorter itself used.
    private static String nameOfValue(HolderLookup.RegistryLookup<PlacedFeature> lookup, PlacedFeature value) {
        return lookup.listElements()
            .filter(h -> h.value() == value)
            .findFirst()
            .map(h -> h.key().identifier().toString())
            .orElse("<unnamed>");
    }

    private FeatureOrderGolden() {}
}
