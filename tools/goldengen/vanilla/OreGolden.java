// OreGolden — vanilla's `OreFeature.place` output, block for block.
//
// The ore feature is the first *feature* to get a golden gate. Everything
// under it — noise, surface, carvers — is proved at 100.000000%, and the
// feature layer was, until this file, proved only by reading the decompile.
// That is the standard which produced two 56%-error bugs in the density
// layer, so none of it is trusted until vectors from the jar say so.
//
// # Why this can run at all without a server
//
// `OreFeature` touches the world through exactly **six** methods. That is not
// a guess: it is `javap -c` over `OreFeature` and `BulkSectionAccess`, whose
// combined invoke set names only
//
//   WorldGenLevel.getHeight(Heightmap$Types, int, int)
//   WorldGenLevel.isOutsideBuildHeight(int)
//   WorldGenLevel.ensureCanWrite(BlockPos)
//   LevelAccessor.getChunk(int, int)
//   LevelAccessor.getSectionIndex(int)
//   LevelAccessor.getSectionsCount()
//
// So the level here is `GenLevel` — a proxy over `WorldGenLevel` that
// implements those six against a real 3x3 block of generated `ProtoChunk`s
// and throws for everything else, defaults included. See that file for why
// throwing is the point. It is shared with `DecorateGolden` so both harnesses
// stand in for a level the same way.
//
// # What a case is
//
// One ore config, one origin, one seeded random. The RNG is seeded the way
// `ChunkGenerator.applyBiomeDecoration` seeds it — `setDecorationSeed` for the
// chunk, then `setFeatureSeed` for the feature — so the stream is a real one
// rather than an arbitrary one.
//
// Each case dumps: whether `place` returned true, how many draws it took,
// the RNG's next `nextLong()` afterwards, and every block it changed. The
// draw count and the trailing `nextLong` are both there on purpose. The count
// is the diagnostic — it says *how far* the streams diverged; the `nextLong`
// is the assertion, because two streams that agree on it agree on the whole
// history that produced it.
//
// The world is dumped once, run-length encoded, and shipped with the cases.
// The Rust side loads it rather than regenerating it: the terrain is already
// proved by three other gates, and re-deriving it here would only add a way
// for this gate to fail for a reason that is not about ore.
//
// Converted to JSON by `ci/ore-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import com.mojang.serialization.JsonOps;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.status.ChunkPyramid;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.feature.ConfiguredFeature;
import net.minecraft.world.level.levelgen.feature.Feature;
import net.minecraft.world.level.levelgen.feature.FeaturePlaceContext;
import net.minecraft.world.level.levelgen.feature.configurations.OreConfiguration;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;

public final class OreGolden {

    private static final long SEED = 20260725L;

    /// Three centres, each generated with its eight neighbours. Chosen for
    /// what the *stone* looks like, since that is what an ore replaces:
    /// `(0, 0)` is ordinary inland terrain, `(34, -57)` is a frozen ocean
    /// whose column is water down to a deep floor, and `(80, 153)` is a stony
    /// peak, which reaches high enough that the same origin lands in solid
    /// rock rather than in the sky.
    private static final int[][] CENTERS = {{0, 0}, {34, -57}, {80, 153}};

    /// Two columns inside the centre chunk, one of them a block from the
    /// corner so a vein has to reach into the neighbours.
    private static final int[][] SPOTS = {{1, 1}, {9, 8}};

    /// Deliberately spread past both ends of the world. `-62` sits two blocks
    /// off the bedrock floor, `250` is far above any terrain here — that one
    /// exists to exercise the early return, where three draws happen and
    /// nothing is placed.
    private static final int[] LEVELS = {-62, -50, -16, 11, 48, 90, 140, 250};

    /// Offsets from the origin column's own `OCEAN_FLOOR_WG`, added to
    /// `LEVELS` per centre and spot.
    ///
    /// These exist because a mutation test found the gate could not see them
    /// missing. `place` probes `y - 2 - i <= level.getHeight(OCEAN_FLOOR_WG,
    /// ...)`, a loose inequality, and every level above is either far below
    /// the terrain (where it passes whatever the height is) or far above it
    /// (where it fails whatever the height is). So swapping the heightmap for
    /// a wrong one changed nothing, and the gate reported 100%.
    ///
    /// It is a wrong heightmap that is easy to reach for: `OCEAN_FLOOR_WG` is
    /// never re-primed before features, so it is the terrain as the noise
    /// stage left it, and recomputing it from the blocks in front of you —
    /// the obvious implementation — disagrees on **69% of the columns in
    /// these worlds, by up to 25 blocks**. Origins straddling the height are
    /// what turns that into a failing case.
    private static final int[] SURFACE_OFFSETS = {-6, 2, 9};

    /// The decoration step ores belong to, read from the enum rather than
    /// written down as 6: it is an argument to `setFeatureSeed`, so a version
    /// that inserts a step ahead of it moves every ore in every world.
    private static final int STEP =
        GenerationStep.Decoration.UNDERGROUND_ORES.ordinal();

    private static int CASES;
    private static int PLACED;
    private static int EMPTY;
    private static int WRITE_DENIED_TOTAL;
    private static int OUTSIDE_TOTAL;

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        HolderGetter<net.minecraft.world.level.levelgen.synth.NormalNoise.NoiseParameters> noises =
            lookup.lookupOrThrow(Registries.NOISE);
        Holder<NoiseGeneratorSettings> settingsHolder = lookup
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD);
        NoiseGeneratorSettings settings = settingsHolder.value();
        NoiseSettings noiseSettings = settings.noiseSettings();

        Registry<Biome> biomeRegistry = SurfaceChunkGolden.buildBiomeRegistry(lookup);
        RegistryAccess access = new RegistryAccess.ImmutableRegistryAccess(List.of(biomeRegistry));
        PalettedContainerFactory containers = PalettedContainerFactory.create(access);
        LevelHeightAccessor heights =
            LevelHeightAccessor.create(noiseSettings.minY(), noiseSettings.height());

        MultiNoiseBiomeSource biomeSource =
            MultiNoiseBiomeSource.createFromList(SurfaceChunkGolden.overworldBiomes(lookup));
        NoiseBasedChunkGenerator generator =
            new NoiseBasedChunkGenerator(biomeSource, settingsHolder);
        RandomState state = RandomState.create(settings, noises, SEED);

        // Every ore config in the registry. Not a hand-picked subset: the
        // last gate that chose six of sixty-three noises by hand missed the
        // only one that was wrong.
        //
        // **Sorted by name, and that matters.** `listElements()` does not
        // return a stable order between runs — two consecutive regenerations
        // of this file started with `ore_diamond_small` and `ore_diorite`.
        // The position in this list becomes the feature index handed to
        // `setFeatureSeed`, so an unstable order means every case gets a
        // different RNG stream each time the vectors are rebuilt: the file
        // would churn on every regeneration and a real regression would be
        // invisible inside the noise.
        List<String> names = new ArrayList<>();
        List<OreConfiguration> configs = new ArrayList<>();
        List<String> configJson = new ArrayList<>();
        List<Holder.Reference<ConfiguredFeature<?, ?>>> ores = new ArrayList<>();
        lookup.lookupOrThrow(Registries.CONFIGURED_FEATURE).listElements().forEach(holder -> {
            if (holder.value().feature() == Feature.ORE) {
                ores.add(holder);
            }
        });
        ores.sort(java.util.Comparator.comparing(h -> h.key().identifier().toString()));
        for (Holder.Reference<ConfiguredFeature<?, ?>> holder : ores) {
            OreConfiguration config = (OreConfiguration) holder.value().config();
            names.add(holder.key().identifier().toString());
            configs.add(config);
            configJson.add(OreConfiguration.CODEC
                .encodeStart(JsonOps.INSTANCE, config)
                .getOrThrow()
                .toString());
        }
        if (configs.isEmpty()) {
            throw new IllegalStateException("no ore configs in the registry");
        }

        // The radius the real generator gives a feature to write into. Read
        // from the pyramid rather than written down as 1, so a version that
        // changes it changes this too.
        int writeRadius = ChunkPyramid.GENERATION_PYRAMID
            .getStepTo(ChunkStatus.FEATURES)
            .blockStateWriteRadius();

        List<String> worlds = new ArrayList<>();
        List<String> cases = new ArrayList<>();

        for (int[] center : CENTERS) {
            ProtoChunk[] grid = new ProtoChunk[9];
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    grid[(dz + 1) * 3 + dx + 1] = SurfaceChunkGolden.generateChunk(
                        state, access, settings, noiseSettings, containers, heights,
                        biomeSource, generator, SEED,
                        center[0] + dx, center[1] + dz, null, null, null, null);
                }
            }

            // Shared with `DecorateGolden`, so both harnesses stand in for a
            // level the same way. No BiomeManager: `javap` says the ore
            // feature never asks for a biome, and passing null means it
            // throws rather than answers if that ever stops being true.
            GenLevel region = new GenLevel(
                grid, center[0], center[1], 1, heights, writeRadius, SEED, settings.seaLevel(), null,
                // `WorldGenRegion`'s own `random`, rebuilt: its
                // `WORLDGEN_REGION_RANDOM` is private, and its value is
                // `Identifier.withDefaultNamespace("worldgen_region_random")`
                // — read from the bytecode's constant pool, not guessed. The
                // factory is positional and seeded from the world, so this is
                // reproducible; a fresh `RandomSource` here would make the
                // vectors churn.
                state.getOrCreateRandomFactory(
                    net.minecraft.resources.Identifier.withDefaultNamespace(
                        "worldgen_region_random"))
                    .at(new ChunkPos(center[0], center[1]).getWorldPosition()),
                // No ore feature reaches `registryAccess`, so this passes null
                // and gets `GenLevel`'s throw if that ever stops being true.
                null);
            WorldGenLevel level = region.proxy();

            worlds.add(SurfaceChunkGolden.encodeWorld(grid, center, noiseSettings));

            int caseIndex = 0;
            for (int[] spot : SPOTS) {
                int surface = grid[4].getHeight(
                    Heightmap.Types.OCEAN_FLOOR_WG, spot[0], spot[1]);
                int[] levels = new int[LEVELS.length + SURFACE_OFFSETS.length];
                System.arraycopy(LEVELS, 0, levels, 0, LEVELS.length);
                for (int i = 0; i < SURFACE_OFFSETS.length; i++) {
                    levels[LEVELS.length + i] = surface + SURFACE_OFFSETS[i];
                }
                for (int y : levels) {
                    int ox = center[0] * 16 + spot[0];
                    int oz = center[1] * 16 + spot[1];
                    for (int i = 0; i < configs.size(); i++) {
                        cases.add(runCase(
                            level, region, generator, grid, center, noiseSettings,
                            names.get(i), configs.get(i), ox, y, oz, caseIndex++));
                    }
                }
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"ore\",\n");
        out.append("  \"seed\": \"").append(SEED).append("\",\n");
        out.append("  \"step\": ").append(STEP).append(",\n");
        out.append("  \"writeRadius\": ").append(writeRadius).append(",\n");
        out.append("  \"cases\": ").append(CASES).append(",\n");
        out.append("  \"placed\": ").append(PLACED).append(",\n");
        out.append("  \"empty\": ").append(EMPTY).append(",\n");
        out.append("  \"writeDenied\": ").append(WRITE_DENIED_TOTAL).append(",\n");
        out.append("  \"outsideBuildHeight\": ").append(OUTSIDE_TOTAL).append(",\n");
        out.append("  \"configs\": [\n");
        for (int i = 0; i < names.size(); i++) {
            out.append("    {\"name\": ").append(SurfaceChunkGolden.quote(names.get(i)))
               .append(", \"config\": ").append(configJson.get(i)).append('}');
            out.append(i + 1 < names.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"worlds\": [\n");
        for (int i = 0; i < worlds.size(); i++) {
            out.append("    ").append(worlds.get(i)).append(i + 1 < worlds.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"vectors\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i)).append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// Run one ore feature and record what it did.
    ///
    /// The world is restored afterwards from the recorded changes, so cases
    /// are independent: an early divergence stays one failing case instead of
    /// poisoning every case after it.
    private static String runCase(
        WorldGenLevel level,
        GenLevel region,
        NoiseBasedChunkGenerator generator,
        ProtoChunk[] grid,
        int[] center,
        NoiseSettings noiseSettings,
        String name,
        OreConfiguration config,
        int ox,
        int oy,
        int oz,
        int caseIndex
    ) {
        // Vanilla's own seeding, in vanilla's own order.
        WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(0L));
        long populationSeed = random.setDecorationSeed(SEED, center[0] * 16, center[1] * 16);
        random.setFeatureSeed(populationSeed, caseIndex, STEP);
        Counting counting = new Counting(random);

        // A box that certainly contains everything `doPlace` can touch:
        // vanilla's own bound plus two. Used only to decide where to look for
        // changes, so being generous costs time and nothing else.
        int margin = Mth.ceil((float) config.size / 8.0F)
            + Mth.ceil(((float) config.size / 16.0F * 2.0F + 1.0F) / 2.0F) + 2;
        int minY = noiseSettings.minY();
        int maxY = minY + noiseSettings.height() - 1;
        int y0 = Math.max(minY, oy - margin);
        int y1 = Math.min(maxY, oy + margin);

        List<String> before = new ArrayList<>();
        for (int y = y0; y <= y1; y++) {
            for (int z = oz - margin; z <= oz + margin; z++) {
                for (int x = ox - margin; x <= ox + margin; x++) {
                    before.add(nameAt(grid, center, x, y, z));
                }
            }
        }

        int deniedBefore = region.writeDenied;
        int outsideBefore = region.outsideBuildHeightCalls;
        boolean placed = Feature.ORE.place(new FeaturePlaceContext<>(
            java.util.Optional.<ConfiguredFeature<?, ?>>empty(), level, generator, counting,
            new BlockPos(ox, oy, oz), config));
        long after = random.nextLong();

        List<String> changes = new ArrayList<>();
        int index = 0;
        for (int y = y0; y <= y1; y++) {
            for (int z = oz - margin; z <= oz + margin; z++) {
                for (int x = ox - margin; x <= ox + margin; x++) {
                    String now = nameAt(grid, center, x, y, z);
                    String was = before.get(index++);
                    if (!now.equals(was)) {
                        changes.add("[" + x + ", " + y + ", " + z + ", "
                            + SurfaceChunkGolden.quote(now) + "]");
                        setAt(grid, center, x, y, z, was);
                    }
                }
            }
        }

        CASES++;
        WRITE_DENIED_TOTAL += region.writeDenied - deniedBefore;
        OUTSIDE_TOTAL += region.outsideBuildHeightCalls - outsideBefore;
        if (placed) {
            PLACED++;
        } else {
            EMPTY++;
        }

        return "{\"feature\": " + SurfaceChunkGolden.quote(name)
            + ", \"origin\": [" + ox + ", " + oy + ", " + oz + "]"
            + ", \"index\": " + caseIndex
            + ", \"placed\": " + placed
            + ", \"draws\": " + counting.draws
            + ", \"after\": \"" + after + "\""
            + ", \"writeDenied\": " + (region.writeDenied - deniedBefore)
            + ", \"outsideBuildHeight\": " + (region.outsideBuildHeightCalls - outsideBefore)
            + ", \"changes\": [" + String.join(", ", changes) + "]}";
    }

    private static ProtoChunk chunkFor(ProtoChunk[] grid, int[] center, int x, int z) {
        int dx = SectionPos.blockToSectionCoord(x) - center[0];
        int dz = SectionPos.blockToSectionCoord(z) - center[1];
        if (dx < -1 || dx > 1 || dz < -1 || dz > 1) {
            throw new IllegalStateException("outside the 3x3 world at " + x + ", " + z);
        }
        return grid[(dz + 1) * 3 + dx + 1];
    }

    /// The full state, not the block's name — `minecraft:redstone_ore` and
    /// `minecraft:redstone_ore[lit=false]` are the same block and different
    /// states, and only the second is what vanilla wrote. ORACLE.md rule 11.
    private static String nameAt(ProtoChunk[] grid, int[] center, int x, int y, int z) {
        BlockState st = chunkFor(grid, center, x, z).getBlockState(new BlockPos(x, y, z));
        return BlockStateParser.serialize(st);
    }

    /// The inverse of `nameAt`, so restoring the world between cases puts the
    /// state back and not just the block.
    private static void setAt(ProtoChunk[] grid, int[] center, int x, int y, int z, String state) {
        BlockState st;
        try {
            st = BlockStateParser
                .parseForBlock(BuiltInRegistries.BLOCK, state, false)
                .blockState();
        } catch (com.mojang.brigadier.exceptions.CommandSyntaxException e) {
            throw new IllegalStateException("cannot parse " + state, e);
        }
        chunkFor(grid, center, x, z).setBlockState(new BlockPos(x, y, z), st);
    }

    /// A `RandomSource` that counts draws and delegates everything else.
    ///
    /// Only the abstract methods are overridden; the interface's defaults
    /// (`nextInt(int, int)`, `consumeCount`, `triangle`) run as vanilla wrote
    /// them and reach the stream through the counted methods, so they are
    /// counted too — by the draws they actually make rather than as one each.
    private static final class Counting implements RandomSource {
        private final RandomSource inner;
        private int draws;

        private Counting(RandomSource inner) {
            this.inner = inner;
        }

        @Override
        public RandomSource fork() {
            draws++;
            return inner.fork();
        }

        @Override
        public PositionalRandomFactory forkPositional() {
            return inner.forkPositional();
        }

        @Override
        public void setSeed(long seed) {
            inner.setSeed(seed);
        }

        @Override
        public int nextInt() {
            draws++;
            return inner.nextInt();
        }

        @Override
        public int nextInt(int bound) {
            draws++;
            return inner.nextInt(bound);
        }

        @Override
        public long nextLong() {
            draws++;
            return inner.nextLong();
        }

        @Override
        public boolean nextBoolean() {
            draws++;
            return inner.nextBoolean();
        }

        @Override
        public float nextFloat() {
            draws++;
            return inner.nextFloat();
        }

        @Override
        public double nextDouble() {
            draws++;
            return inner.nextDouble();
        }

        @Override
        public double nextGaussian() {
            draws++;
            return inner.nextGaussian();
        }
    }

    private OreGolden() {}
}
