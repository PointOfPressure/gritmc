// DecorateGolden — one whole decoration step, the way the server runs it.
//
// `golden_ore` proves the ore feature in isolation: given an origin and a
// seeded random, does the port write the same blocks. What it cannot prove is
// the part that *chooses* those origins — the placement chain, the feature
// index, the seeding — and that is where a draw-count error actually lives.
// Arithmetic bugs move one feature. Wiring bugs move every feature after
// them.
//
// So this runs the `UNDERGROUND_ORES` step of
// `ChunkGenerator.applyBiomeDecoration` over a real 3x3 world and dumps, per
// feature: the positions its placement chain produced, the blocks it wrote,
// the draws it took, and the RNG's state afterwards.
//
// # Why the step loop is reproduced rather than called
//
// `applyBiomeDecoration` reaches `WorldGenLevel.getLevel()`, which returns a
// concrete `ServerLevel` and is passed to `FeatureCountTracker`, and it takes
// a `StructureManager`. Neither is constructible outside a running server.
// The loop itself is eleven lines of public API, so it is reproduced here —
// the same call the carver sweep in `SurfaceChunkGolden` makes, for the same
// reason, and it is written against `javap -c` of the real method rather than
// from memory:
//
//   origin  = SectionPos.of(chunkPos, level.getMinSectionY()).origin()
//   popSeed = random.setDecorationSeed(level.getSeed(), origin.x, origin.z)
//   biomes  = every biome in the 3x3's section palettes, retained against
//             biomeSource.possibleBiomes()
//   indices = for each biome, its features at this step, mapped through
//             StepFeatureData.indexMapping  -- then **sorted**
//   for i in indices:
//       random.setFeatureSeed(popSeed, i, step)
//       features.get(i).placeWithBiomeCheck(level, generator, random, origin)
//
// Three things in that are load-bearing and easy to get wrong. The index is
// the feature's position in a **global** ordering computed from the whole
// biome registry, not its position in this biome's list. The indices are
// **sorted**, so the order features run in is that global order and not the
// order the biomes were walked. And the origin is the chunk's **minimum**
// corner at the world's minimum section, not its centre and not y=0.
//
// # Biomes have to be in the chunk, not just in the biome source
//
// The biome set is collected by walking each section's biome *palette*. A
// harness that never filled those containers collects nothing, retains
// nothing, and decorates nothing — with no error, because an empty set is a
// legal answer. `SurfaceChunkGolden.generateChunk` now runs vanilla's own
// `doCreateBiomes` for exactly this reason; see the note there. It is the
// same shape as the empty block tags that made 567 carvers touch zero blocks.
//
// # What this gate does not cover
//
// Only this one step runs. In a real chunk the six steps before it have
// already written blocks, and ores replace what those left behind. Both sides
// here start from the same post-carver world, so the comparison is exact, but
// it is a comparison of one step in isolation rather than of a finished
// chunk. The finished chunk is the region-diff gate's job.
//
// Converted to JSON by `ci/decorate-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.EnumSet;
import java.util.List;

import com.mojang.serialization.Lifecycle;
import it.unimi.dsi.fastutil.ints.IntArraySet;
import it.unimi.dsi.fastutil.ints.IntSet;
import it.unimi.dsi.fastutil.objects.ObjectArraySet;
import net.minecraft.SharedConstants;
import net.minecraft.world.level.levelgen.feature.ConfiguredFeature;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.HolderSet;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.core.QuartPos;
import net.minecraft.util.Mth;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.FeatureSorter;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.status.ChunkPyramid;
import net.minecraft.world.level.chunk.status.ChunkStatus;
import net.minecraft.world.level.levelgen.placement.PlacedFeature;
import net.minecraft.world.level.levelgen.placement.PlacementContext;
import net.minecraft.world.level.levelgen.placement.PlacementModifier;

public final class DecorateGolden {

    /// `-Ddecorate.seed=<long>` and `-Ddecorate.centers=x,z[;x,z...]` point
    /// this harness at somewhere other than its committed vectors.
    ///
    /// Ad-hoc rather than added to the vector set on purpose: the r7 diff
    /// throws up one chunk at a time, and regenerating and re-gating the
    /// committed vectors for each is a much bigger change than asking a
    /// question. What this exists for is the class of fault where our own
    /// feature positions are dumpable and vanilla's are not — the diorite
    /// vein of chunk (64, 59), and every fault shaped like it.
    ///
    /// A finding that survives should then earn a real vector, not stay here.
    private static final long SEED =
        Long.getLong("decorate.seed", 20260725L);

    /// Feature-name substring whose per-modifier positions to record, or null.
    private static final String TRACE = System.getProperty("decorate.trace");
    /// `-Ddecorate.reads=<substring>` — record every `getBlockState` the
    /// matching feature makes, in order.
    ///
    /// The order is the point. A feature that walks a region and returns on
    /// the first failure leaves a read log whose **last entry is the cell it
    /// gave up on**, which is the only way to ask vanilla "which block
    /// stopped you" without modifying vanilla. `LakeFeature.place`'s validity
    /// pass is exactly that shape and cost a session to inference before this
    /// existed.
    private static final String READS = System.getProperty("decorate.reads");
    /// `-Ddecorate.draws=<substring>` — log every draw of the matching
    /// feature. See [`Counting.logging`].
    private static final String DRAWS = System.getProperty("decorate.draws");
    /// How many stage entries to keep **per modifier**.
    ///
    /// Per modifier rather than overall: the first `count` in a chain emits
    /// its whole batch — two hundred copies of one position for
    /// `sulfur_spike` — and a single shared cap is spent entirely on that.
    /// How many positions per modifier the trace keeps.
    ///
    /// Per *modifier*, not per feature — which matters when reading the
    /// output, because once the early stages hit the cap the later ones keep
    /// appending and a stage-3 entry appears with no stage-2 parent above it.
    /// That looks exactly like a scan moving a position sideways, which it
    /// cannot do. `-Ddecorate.traceCap=` raises it when the whole candidate
    /// list is the thing being compared.
    private static final int TRACE_CAP = Integer.getInteger("decorate.traceCap", 40);

    /// The same three centres the ore gate uses, so a failure here can be
    /// compared against a gate that already passes on the same terrain.
    private static final int[][] CENTERS = parseCenters();

    private static int[][] parseCenters() {
        String raw = System.getProperty("decorate.centers");
        if (raw == null) {
            return new int[][] {{0, 0}, {34, -57}, {80, 153}};
        }
        String[] parts = raw.split(";");
        int[][] out = new int[parts.length][2];
        for (int i = 0; i < parts.length; i++) {
            String[] xz = parts[i].split(",");
            out[i] = new int[] {Integer.parseInt(xz[0].trim()), Integer.parseInt(xz[1].trim())};
        }
        return out;
    }

    /// Every decoration step, in the order `applyBiomeDecoration` runs them.
    ///
    /// All of them rather than one, because a step does not start from the
    /// post-carver world — it starts from whatever the steps before it left,
    /// and a gate that runs step 6 in isolation is measuring a chunk that
    /// never exists. Running them in order is also the only way the shared
    /// `WorldgenRandom` sees the sequence it really sees.
    private static final int STEPS = GenerationStep.Decoration.values().length;

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
        // The configured features go in as well, because ONE decorator asks the
        // level for them at place time. `PaleMossDecorator`:
        //
        //   registryAccess().lookup(Registries.CONFIGURED_FEATURE)
        //       .flatMap(r -> r.get(VegetationFeatures.PALE_MOSS_PATCH))
        //       .ifPresent(h -> h.value().place(level, ..))
        //
        // Without it this harness threw `UnsupportedOperationException` on
        // `WorldGenLevel.registryAccess/0` and **could not dump a pale garden
        // chunk at all** — which is how it was found: the one square that holds
        // a pale garden is the one the census needed a draw table for.
        //
        // The rebuilt-registry warning next door in `buildBiomeRegistry` does
        // NOT apply here, and it is worth saying why rather than leaving the
        // reader to wonder. That warning is about holder **identity**: a biome
        // holder minted by a second registry can never equal one baked into a
        // surface rule, and every `biome` condition silently returns false.
        // This decorator resolves by `ResourceKey` and immediately calls
        // `.value().place(..)`. It compares no holders, and the value handed
        // back is the same `ConfiguredFeature` object the lookup holds, because
        // that is what is registered below.
        //
        // Note the `Optional` in that chain: an ABSENT registry is not an
        // error to vanilla, it is a patch that never places. So getting this
        // wrong would not have crashed — it would have produced a pale oak
        // with no moss and the wrong draw count, and only a census would ever
        // have said so.
        Registry<ConfiguredFeature<?, ?>> featureRegistry = buildConfiguredFeatureRegistry(lookup);
        RegistryAccess access =
            new RegistryAccess.ImmutableRegistryAccess(List.of(biomeRegistry, featureRegistry));
        PalettedContainerFactory containers = PalettedContainerFactory.create(access);
        LevelHeightAccessor heights =
            LevelHeightAccessor.create(noiseSettings.minY(), noiseSettings.height());

        MultiNoiseBiomeSource biomeSource =
            MultiNoiseBiomeSource.createFromList(SurfaceChunkGolden.overworldBiomes(lookup));
        NoiseBasedChunkGenerator generator =
            new NoiseBasedChunkGenerator(biomeSource, settingsHolder);
        RandomState state = RandomState.create(settings, noises, SEED);

        HolderLookup.RegistryLookup<PlacedFeature> placedLookup =
            lookup.lookupOrThrow(Registries.PLACED_FEATURE);
        // `List.copyOf(biomeSource.possibleBiomes())` — what `ChunkGenerator`
        // hands the sorter, per `javap -c` of its constructor. The registry's
        // `listElements()` is a different set in an order that is not stable
        // between JVM runs; using it here made this harness's own
        // regenerate-twice check fail, which is how the mistake was found.
        List<Holder<Biome>> biomeList = List.copyOf(biomeSource.possibleBiomes());

        // The same global ordering `golden_feature_order` already proves, from
        // the same public entry point. Threaded from one place rather than
        // recomputed, so a disagreement between the two gates is impossible
        // by construction instead of by luck.
        List<FeatureSorter.StepFeatureData> perStep = FeatureSorter.buildFeaturesPerStep(
            biomeList, biome -> biome.value().getGenerationSettings().features(), true);


        int writeRadius = ChunkPyramid.GENERATION_PYRAMID
            .getStepTo(ChunkStatus.FEATURES)
            .blockStateWriteRadius();

        List<String> worlds = new ArrayList<>();
        List<String> cases = new ArrayList<>();
        int totalFeatures = 0;
        int totalPositions = 0;
        int totalChanges = 0;
        int biomeLookups = 0;
        List<String> biomeAnswerBlocks = new ArrayList<>();
        // Quart cells the biome manager was asked about that lie outside the
        // level grid. A server reads those from a neighbouring chunk's
        // container; this harness resamples the biome source, which is what
        // `getUncachedNoiseBiome` does and is the only honest answer available
        // to it. An array because the lambda that increments it closes over
        // it. If this is ever large, the grid is too small.
        final int[] outOfGridBiomeLookups = {0};

        // `-Ddecorate.region=<r>` decorates a chunk's neighbourhood before
        // recording it.
        //
        // Without it this dumper built a fresh 3x3 per centre and decorated
        // only the middle one. Neighbours were present for *reads* and never
        // themselves decorated, which is a world no server produces. Measured
        // at seed 31415926 centre (67,62), **11 of that chunk's 59 features
        // behave differently** once the neighbourhood decorates —
        // `lush_caves_ceiling_vegetation` goes 595 draws/131 changes/1
        // position to 898/288/2, and `ore_tuff` moves by 200 blocks. Three
        // phantom defects this session came from believing the old numbers.
        //
        // With it, each centre gets its own grid of radius r+1, and the
        // chunks of the inner (2r+1)^2 decorate in raster order **up to and
        // including the centre** — later ones cannot affect it. Only the
        // centre is recorded, so the vector file keeps its shape; what
        // changes is that the world it records against is one a server can
        // actually have.
        //
        // Each chunk still keys `setDecorationSeed`/`setFeatureSeed` to its
        // own origin. The world is shared; the seeds are not.
        //
        // Raster is an assumption, and an honest one to name: cross-chunk
        // features depend on which neighbour decorated first, which on a
        // server is ticket order, not a raster. It is strictly better than
        // the old assumption — that no neighbour ever decorates — but a
        // cross-chunk feature's vector is still order-dependent.
        Integer regionRadius = Integer.getInteger("decorate.region");
        List<Object[]> targets = new ArrayList<>();
        if (regionRadius != null) {
            for (int[] c : CENTERS) {
                int r = regionRadius;
                int gridRadius = r + 1;
                int side = gridRadius * 2 + 1;
                ProtoChunk[] shared = new ProtoChunk[side * side];
                for (int dz = -gridRadius; dz <= gridRadius; dz++) {
                    for (int dx = -gridRadius; dx <= gridRadius; dx++) {
                        shared[(dz + gridRadius) * side + dx + gridRadius] =
                            SurfaceChunkGolden.generateChunk(
                                state, access, settings, noiseSettings, containers, heights,
                                biomeSource, generator, SEED,
                                c[0] + dx, c[1] + dz, null, null, null, null);
                    }
                }
                boolean done = false;
                for (int dz = -r; dz <= r && !done; dz++) {
                    for (int dx = -r; dx <= r && !done; dx++) {
                        boolean rec = dx == 0 && dz == 0;
                        targets.add(new Object[] {
                            shared, c[0], c[1], gridRadius, c[0] + dx, c[1] + dz, rec});
                        done = rec;
                    }
                }
            }
        } else {
            for (int[] c : CENTERS) {
                targets.add(new Object[] {null, c[0], c[1], 1, c[0], c[1], true});
            }
        }

        for (Object[] target : targets) {
            ProtoChunk[] shared = (ProtoChunk[]) target[0];
            int levelCx = (Integer) target[1];
            int levelCz = (Integer) target[2];
            int levelRadius = (Integer) target[3];
            int[] center = new int[] {(Integer) target[4], (Integer) target[5]};
            // Whether this chunk's features go into the vector file, or only
            // into the world the recorded chunk will read.
            boolean record = (Boolean) target[6];

            ProtoChunk[] levelGrid;
            if (shared != null) {
                levelGrid = shared;
            } else {
                levelGrid = new ProtoChunk[9];
                for (int dz = -1; dz <= 1; dz++) {
                    for (int dx = -1; dx <= 1; dx++) {
                        levelGrid[(dz + 1) * 3 + dx + 1] = SurfaceChunkGolden.generateChunk(
                            state, access, settings, noiseSettings, containers, heights,
                            biomeSource, generator, SEED,
                            center[0] + dx, center[1] + dz, null, null, null, null);
                    }
                }
            }
            // The grid the *diff* looks at: always this chunk's 3x3, so a
            // snapshot stays the same size however large the region is.
            ProtoChunk[] grid = new ProtoChunk[9];
            for (int dz = -1; dz <= 1; dz++) {
                for (int dx = -1; dx <= 1; dx++) {
                    int side = levelRadius * 2 + 1;
                    int gx = center[0] + dx - levelCx + levelRadius;
                    int gz = center[1] + dz - levelCz + levelRadius;
                    grid[(dz + 1) * 3 + dx + 1] = levelGrid[gz * side + gx];
                }
            }

            // The biome manager the level answers `getBiome` with, wired the
            // way `WorldGenRegion` wires its own.
            //
            // **This resampled the biome source until 2026-07-28, and a
            // server does not.** `WorldGenRegion.<init>` is
            // `new BiomeManager(this, obfuscateSeed(seed))` — the region
            // itself is the `NoiseBiomeSource` — and `WorldGenRegion` does
            // not override `getNoiseBiome`, so it takes `LevelReader`'s
            // default (bytecode):
            //
            //     chunk = getChunk(QuartPos.toSection(qx), QuartPos.toSection(qz),
            //                      ChunkStatus.BIOMES, false);
            //     return chunk != null ? chunk.getNoiseBiome(qx, qy, qz)
            //                          : getUncachedNoiseBiome(qx, qy, qz);
            //
            // and `ChunkAccess.getNoiseBiome` clamps qy into the chunk's own
            // height and reads the **stored container** at `(qx & 3, qy & 3,
            // qz & 3)`. That container was filled by `fillBiomesFromNoise`
            // from `noiseChunk.cachedClimateSampler(router, spawnTarget)` —
            // not `state.sampler()`. They are different computations and they
            // disagree at some cells: `SurfaceChunkGolden` already records a
            // measured one, seed 20260722 chunk (63,68), where the container
            // holds `savanna` at (1013,45,1098) and a resample says
            // `lukewarm_ocean`.
            //
            // `LevelReader.getBiome(pos)` is `getBiomeManager().getBiome(pos)`,
            // so **every `minecraft:biome` placement filter** ran against the
            // wrong one of the two here. `SurfaceChunkGolden` had this right
            // and this file did not; the lambda below is the same shape,
            // widened from one chunk to the level grid.
            //
            // The fallback is a resample, because a quart cell outside the
            // grid is one a server reads from a chunk this harness does not
            // have — the same case `getUncachedNoiseBiome` covers. Counted,
            // not silent: `outOfGridBiomeLookups` says how often the harness
            // answered a question it could not read.
            Climate.Sampler sampler = state.sampler();
            final ProtoChunk[] biomeGrid = levelGrid;
            final int biomeGridCx = levelCx;
            final int biomeGridCz = levelCz;
            final int biomeGridRadius = levelRadius;
            BiomeManager biomeManager = new BiomeManager(
                (qx, qy, qz) -> {
                    int cx = QuartPos.toSection(qx);
                    int cz = QuartPos.toSection(qz);
                    int dx = cx - biomeGridCx;
                    int dz = cz - biomeGridCz;
                    if (Math.abs(dx) <= biomeGridRadius && Math.abs(dz) <= biomeGridRadius) {
                        int side = biomeGridRadius * 2 + 1;
                        ProtoChunk c = biomeGrid[(dz + biomeGridRadius) * side
                            + dx + biomeGridRadius];
                        // `ChunkAccess.getNoiseBiome`: clamp qy into the
                        // chunk, then read the section's container.
                        int qLo = QuartPos.fromBlock(c.getMinY());
                        int qHi = qLo + QuartPos.fromBlock(c.getHeight()) - 1;
                        int qyc = Mth.clamp(qy, qLo, qHi);
                        int si = c.getSectionIndex(QuartPos.toBlock(qyc));
                        if (si >= 0 && si < c.getSections().length) {
                            return c.getSection(si).getBiomes()
                                .get(qx & 3, qyc & 3, qz & 3);
                        }
                    }
                    outOfGridBiomeLookups[0]++;
                    return biomeSource.getNoiseBiome(qx, qy, qz, sampler);
                },
                BiomeManager.obfuscateSeed(SEED));
            GenLevel region = new GenLevel(
                levelGrid, levelCx, levelCz, levelRadius,
                heights, writeRadius, SEED, settings.seaLevel(), biomeManager,
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
                access);
            WorldGenLevel level = region.proxy();

            ChunkPos chunkPos = new ChunkPos(center[0], center[1]);
            BlockPos origin = SectionPos.of(chunkPos, heights.getMinSectionY()).origin();

            WorldgenRandom random = new WorldgenRandom(new XoroshiroRandomSource(0L));
            long populationSeed = random.setDecorationSeed(SEED, origin.getX(), origin.getZ());

            // Every biome present in the 3x3's section palettes, retained
            // against the source's possible set — `applyBiomeDecoration`'s own
            // two steps, in its own order. `ObjectArraySet` because vanilla
            // uses one: it is insertion-ordered, and the index set built from
            // it is sorted afterwards anyway, but a hash set here would be a
            // map iteration at a behaviour-visible site.
            ObjectArraySet<Holder<Biome>> present = new ObjectArraySet<>();
            for (ProtoChunk chunk : grid) {
                for (LevelChunkSection section : chunk.getSections()) {
                    section.getBiomes().getAll(present::add);
                }
            }
            present.retainAll(biomeSource.possibleBiomes());
            if (present.isEmpty()) {
                throw new IllegalStateException(
                    "no biomes collected at " + center[0] + ", " + center[1]
                    + " — the chunks' biome containers are empty, which decorates nothing "
                    + "and reports no error. See doCreateBiomes in SurfaceChunkGolden.");
            }

            // Shipped with the world, because the Rust side cannot derive it:
            // it is what the chunks' biome *containers* hold, and those are
            // filled by `fillBiomesFromNoise` through the NoiseChunk's cached
            // climate sampler — not by asking the biome source. The retain
            // against `possibleBiomes` is vanilla's and is applied above.
            List<String> presentNames = new ArrayList<>();
            for (Holder<Biome> biome : present) {
                presentNames.add(SurfaceChunkGolden.quote(
                    biome.unwrapKey().map(k -> k.identifier().toString()).orElse("?")));
            }
            if (record) {
                worlds.add(SurfaceChunkGolden.encodeWorld(grid, center, noiseSettings)
                .replaceFirst("\\{", "{\"biomes\": [" + String.join(", ", presentNames) + "]"
                    + ", \"origin\": [" + origin.getX() + ", " + origin.getY() + ", " + origin.getZ() + "]"
                    + ", \"populationSeed\": \"" + populationSeed + "\", "));
            }

            // `ChunkStatusTasks.generateFeatures`'s first two statements, and
            // without them every heightmap placement modifier in this dumper
            // reads a **frozen** column.
            //
            // `ProtoChunk#setBlockState` updates exactly
            // `getPersistedStatus().heightmapsAfter()`. A chunk built by
            // `SurfaceChunkGolden.generateChunk` is `new ProtoChunk(...)`,
            // whose `status` field starts at `ChunkStatus.EMPTY` — and
            // `EMPTY` is registered with `WORLDGEN_HEIGHTMAPS`
            // = {OCEAN_FLOOR_WG, WORLD_SURFACE_WG} (ChunkStatus static init,
            // offsets 30-42). So while features ran, this dumper updated the
            // two *worldgen* maps and **never** OCEAN_FLOOR, WORLD_SURFACE,
            // MOTION_BLOCKING or MOTION_BLOCKING_NO_LEAVES.
            //
            // `ChunkAccess.getHeight` primes a missing map on first read, so
            // the effect was not an exception or a zero: the first feature to
            // ask for OCEAN_FLOOR primed it from the blocks present at that
            // instant, and every later feature in every later step read that
            // same instant back. A tree that raised a column by four never
            // moved the next tree.
            //
            // A server does neither of those things.
            // `ChunkStep.completeChunkGeneration` calls `setPersistedStatus`
            // **after** the step's `doWork`, so during FEATURES the persisted
            // status is CARVERS — and CARVERS *and* FEATURES are both
            // registered with `FINAL_HEIGHTMAPS` = the four above. All four
            // are live while features write, and `generateFeatures` primes
            // them from the post-carver blocks immediately beforehand.
            //
            // Both halves are reproduced here: the status once per grid, the
            // priming per decorated chunk, which is the scope vanilla gives
            // each. `isBefore` guards the status so a chunk that has already
            // decorated as somebody else's neighbour is not moved backwards.
            for (ProtoChunk c : levelGrid) {
                if (c.getPersistedStatus().isBefore(ChunkStatus.CARVERS)) {
                    c.setPersistedStatus(ChunkStatus.CARVERS);
                }
            }
            Heightmap.primeHeightmaps(grid[4], EnumSet.of(
                Heightmap.Types.MOTION_BLOCKING,
                Heightmap.Types.MOTION_BLOCKING_NO_LEAVES,
                Heightmap.Types.OCEAN_FLOOR,
                Heightmap.Types.WORLD_SURFACE));

            int[] before = snapshot(grid, noiseSettings);
            int stepCount = Math.max(STEPS, perStep.size());
            for (int step = 0; step < stepCount; step++) {
                if (step >= perStep.size()) {
                    continue;
                }
                FeatureSorter.StepFeatureData stepData = perStep.get(step);
                IntSet indices = new IntArraySet();
                for (Holder<Biome> biome : present) {
                    List<HolderSet<PlacedFeature>> features =
                        biome.value().getGenerationSettings().features();
                    if (step >= features.size()) {
                        continue;
                    }
                    for (Holder<PlacedFeature> feature : features.get(step)) {
                        indices.add(stepData.indexMapping().applyAsInt(feature.value()));
                    }
                }
                int[] order = indices.toIntArray();
                java.util.Arrays.sort(order);

            for (int index : order) {
                PlacedFeature feature = stepData.features().get(index);
                String name = nameOf(placedLookup, feature);

                // `PlacedFeature.placeWithContext`, reproduced so the positions
                // can be recorded as they are produced.
                //
                // It cannot be observed from outside: `placeWithContext` is
                // private, and `placeWithBiomeCheck` returns only a boolean.
                // The first version of this harness therefore replayed the
                // chain on a *separate* identically-seeded random to get the
                // positions — and that was **wrong**, for the same reason the
                // port was wrong at the same moment.
                //
                // Java streams are lazy. The feature runs at position n before
                // position n+1 is computed, and both draw from this one
                // random. A chain replayed on its own random therefore
                // produces a *different* position list from the one the
                // feature actually placed at: correct for the first position
                // and drifting after it, since the replay never pays the
                // feature's draws. Comparing a port against it would have
                // demanded the port be wrong in the same way.
                random.setFeatureSeed(populationSeed, index, step);
                Counting counting = new Counting(random);
                Counting.on = DRAWS != null && name.contains(DRAWS);
                if (Counting.on) {
                    // Label the run. An unlabelled draw log invites slicing it
                    // by assumption — "the centre chunk runs last" — and a
                    // misaligned slice compares two different chunks' runs and
                    // calls them identical.
                    System.err.println("DRAWS-BEGIN " + name
                        + " chunk " + center[0] + "," + center[1]
                        + " record=" + record);
                }
                PlacementContext context =
                    new PlacementContext(level, generator, java.util.Optional.of(feature));
                java.util.stream.Stream<BlockPos> stream = java.util.stream.Stream.of(origin);
                // `-Ddecorate.trace=<substring>` records what each modifier
                // in the chain produced, as [index, x, y, z]. The emitted
                // positions alone cannot say which modifier moved a position
                // where; this can, because a modifier's input is the previous
                // index's output. Off by default and capped, because it is a
                // few hundred entries per feature and the committed vectors
                // are not the place for it.
                List<String> stageTrace = new ArrayList<>();
                boolean traceThis =
                    TRACE != null && name.contains(TRACE);
                int modifierIndex = 0;
                for (PlacementModifier modifier : feature.placement()) {
                    final int mi = modifierIndex++;
                    stream = stream.flatMap(pos -> modifier.getPositions(context, counting, pos));
                    if (traceThis) {
                        final int[] kept = {0};
                        stream = stream.peek(pos -> {
                            if (kept[0]++ < TRACE_CAP) {
                                stageTrace.add("[" + mi + ", " + pos.getX() + ", "
                                    + pos.getY() + ", " + pos.getZ() + "]");
                            }
                        });
                    }
                }
                List<String> positions = new ArrayList<>();
                // The draw count at the moment each position was emitted.
                //
                // The position list says what the chain produced and `draws`
                // says how much stream the whole feature spent, but neither
                // locates a divergence: two generators can emit different
                // positions from the same offset, or the same position from
                // different offsets, and those are different bugs. The stream
                // above is lazy, so reading the counter inside this consumer
                // is exactly the offset the chain reached that position at —
                // and it is read BEFORE `place`, so the feature body's own
                // draws belong to the next entry, not this one.
                List<String> positionDraws = new ArrayList<>();
                net.minecraft.world.level.levelgen.feature.ConfiguredFeature<?, ?> configured =
                    feature.feature().value();
                boolean[] placedAny = {false};
                boolean readThis = READS != null && name.contains(READS);
                if (readThis) {
                    region.blockReads.clear();
                    region.recordReads = true;
                }
                stream.forEach(pos -> {
                    positions.add(
                        "[" + pos.getX() + ", " + pos.getY() + ", " + pos.getZ() + "]");
                    positionDraws.add(Integer.toString(counting.draws));
                    if (configured.place(level, generator, counting, pos)) {
                        placedAny[0] = true;
                    }
                });
                boolean placed = placedAny[0];
                region.recordReads = false;
                long after = random.nextLong();

                // Close the draw log for this run, with its own count.
                //
                // **A `DRAWS-BEGIN` with no `DRAWS-END` is the only way to see
                // that this harness died part way through**, and until
                // 2026-08-18 there was no such way. `GenLevel` threw on two
                // `WorldGenLevel` methods `SculkSpreader` calls, so from the
                // day the deep dark started generating any run whose
                // neighbourhood held one aborted — and the failure is silent
                // in the place people read: no JSON is written, but the draw
                // trace has already streamed to stderr and looks complete. A
                // five-centre run of `trees_jungle` reported three chunks and
                // was read as "only three chunks here have jungle".
                //
                // This is the rule from `docs/AGENT-BRIEF.md` applied to the
                // harness's own output: ask which outcomes the thing has no
                // string for, because a reader can only see the ones it does.
                //
                // ABOVE the `!record` guard, not below it. Below it the
                // neighbourhood chunks — which are exactly the ones a
                // `-Ddecorate.draws` run is usually reading — print a BEGIN
                // and never an END, so the pairing check that this line exists
                // to enable fires on every healthy run. Caught by running it:
                // ten BEGINs and two ENDs.
                if (Counting.on) {
                    System.err.println("DRAWS-END " + name
                        + " chunk " + center[0] + "," + center[1]
                        + " draws=" + counting.draws);
                }

                List<String> changes = diff(grid, noiseSettings, before);
                if (!record) {
                    // A neighbour: it decorates into the shared world so the
                    // recorded chunk reads what a server would, but its own
                    // features are not the thing under test.
                    continue;
                }
                totalFeatures++;
                totalPositions += positions.size();
                totalChanges += changes.size();


                cases.add("{\"center\": [" + center[0] + ", " + center[1] + "]"
                    + ", \"step\": " + step
                    + ", \"feature\": " + SurfaceChunkGolden.quote(name)
                    + ", \"index\": " + index
                    + ", \"placed\": " + placed
                    + ", \"draws\": " + counting.draws
                    + ", \"after\": \"" + after + "\""
                    + ", \"positions\": [" + String.join(", ", positions) + "]"
                    + ", \"positionDraws\": [" + String.join(", ", positionDraws) + "]"
                    + (traceThis
                        ? ", \"stageTrace\": [" + String.join(", ", stageTrace) + "]"
                        : "")
                    + (readThis
                        ? ", \"blockReads\": ["
                            + region.blockReads.stream()
                                .map(SurfaceChunkGolden::quote)
                                .collect(java.util.stream.Collectors.joining(", "))
                            + "]"
                        : "")
                    + ", \"changes\": [" + String.join(", ", changes) + "]}");
            }
            }
            if (!record) {
                continue;
            }
            biomeLookups += region.biomeLookups;
            List<String> answers = new ArrayList<>();
            for (java.util.Map.Entry<String, String> e : region.biomeAnswers.entrySet()) {
                answers.add("[" + SurfaceChunkGolden.quote(e.getKey()) + ", "
                    + SurfaceChunkGolden.quote(e.getValue()) + "]");
            }
            biomeAnswerBlocks.add("[" + String.join(", ", answers) + "]");
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"decorate\",\n");
        out.append("  \"seed\": \"").append(SEED).append("\",\n");
        out.append("  \"steps\": ").append(STEPS).append(",\n");
        out.append("  \"writeRadius\": ").append(writeRadius).append(",\n");
        out.append("  \"features\": ").append(totalFeatures).append(",\n");
        out.append("  \"positions\": ").append(totalPositions).append(",\n");
        out.append("  \"changes\": ").append(totalChanges).append(",\n");
        out.append("  \"biomeLookups\": ").append(biomeLookups).append(",\n");
        // Not a JSON field: the vector file's shape is compared by the gate,
        // and a new key there is churn for a diagnostic. stderr, where
        // `decorate-once.sh` keeps the log.
        if (outOfGridBiomeLookups[0] > 0) {
            System.err.println("OUT-OF-GRID BIOME LOOKUPS " + outOfGridBiomeLookups[0]
                + " — a quart cell outside the level grid was resampled from the biome "
                + "source, where a server would have read a neighbour's container");
        }
        out.append("  \"biomeAnswers\": [\n");
        for (int i = 0; i < biomeAnswerBlocks.size(); i++) {
            out.append("    ").append(biomeAnswerBlocks.get(i))
               .append(i + 1 < biomeAnswerBlocks.size() ? ",\n" : "\n");
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
        // `-Ddecorate.out=<path>` writes the JSON to a file instead of
        // stdout, and the script always sets it.
        //
        // `Bootstrap.bootStrap()` redirects System.out through log4j, so
        // anything vanilla logs lands in the *same stream* as this dump. It
        // does log during decoration — `monster_room` emits
        // "Failed to fetch mob spawner entity at (x, y, z)" because this
        // harness has no block entities — and the line carries a wall-clock
        // timestamp. That corrupted the JSON and, because the timestamp
        // differs between runs, made the two-runs-must-agree guard report
        // "something in it iterates a map", which sent this investigation
        // looking for non-determinism that was never there.
        String outPath = System.getProperty("decorate.out");
        if (outPath == null) {
            System.out.print(out);
        } else {
            try {
                java.nio.file.Files.writeString(java.nio.file.Path.of(outPath), out.toString());
            } catch (java.io.IOException e) {
                throw new RuntimeException("cannot write " + outPath, e);
            }
        }
    }

    /// Block-state ids for the whole 3x3, as one flat array.
    ///
    /// Ids rather than states so the diff is an int compare, and one array
    /// rather than a `Map` so there is no iteration order to get wrong. It is
    /// updated in place by `diff`, which makes each feature's changes relative
    /// to the world the feature before it left — which is what vanilla does.
    private static int[] snapshot(ProtoChunk[] grid, NoiseSettings noiseSettings) {
        int height = noiseSettings.height();
        int[] out = new int[grid.length * height * 256];
        BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();
        int at = 0;
        for (ProtoChunk chunk : grid) {
            int baseX = chunk.getPos().getMinBlockX();
            int baseZ = chunk.getPos().getMinBlockZ();
            for (int y = noiseSettings.minY(); y < noiseSettings.minY() + height; y++) {
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        out[at++] = Block.BLOCK_STATE_REGISTRY.getId(
                            chunk.getBlockState(cursor.set(baseX + x, y, baseZ + z)));
                    }
                }
            }
        }
        return out;
    }

    /// Every block that changed since `before`, in y-major then z then x
    /// order, updating `before` as it goes.
    private static List<String> diff(ProtoChunk[] grid, NoiseSettings noiseSettings, int[] before) {
        List<String> changes = new ArrayList<>();
        int height = noiseSettings.height();
        BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();
        int at = 0;
        for (ProtoChunk chunk : grid) {
            int baseX = chunk.getPos().getMinBlockX();
            int baseZ = chunk.getPos().getMinBlockZ();
            for (int y = noiseSettings.minY(); y < noiseSettings.minY() + height; y++) {
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++, at++) {
                        BlockState now = chunk.getBlockState(cursor.set(baseX + x, y, baseZ + z));
                        int id = Block.BLOCK_STATE_REGISTRY.getId(now);
                        if (id == before[at]) {
                            continue;
                        }
                        before[at] = id;
                        // The full state, not the block's name. The comparison
                        // above is by state id, so a write that only changes a
                        // property is already a change here — recording the
                        // name would report it without saying what it became,
                        // and a port that wrote `glow_lichen` with the wrong
                        // faces would pass. ORACLE.md rule 11.
                        changes.add("[" + (baseX + x) + ", " + y + ", " + (baseZ + z) + ", "
                            + SurfaceChunkGolden.quote(BlockStateParser.serialize(now)) + "]");
                    }
                }
            }
        }
        return changes;
    }

    /// The sorter hands back values, not holders, so the name is found by
    /// identity — two registry entries with equal content are one node to the
    /// sorter, and an equality lookup could name the wrong one.
    private static String nameOf(
        HolderLookup.RegistryLookup<PlacedFeature> lookup, PlacedFeature value
    ) {
        return lookup.listElements()
            .filter(h -> h.value() == value)
            .findFirst()
            .map(h -> h.key().identifier().toString())
            .orElse("<unnamed>");
    }

    /// A `RandomSource` that counts draws and delegates everything else.
    private static final class Counting implements RandomSource {
        private final RandomSource inner;
        private int draws;
        /// `-Ddecorate.draws=<substring>` logs every draw of the matching
        /// feature as `DRAW <n> <method>(<bound>) -> <value>`.
        ///
        /// The count alone stopped being enough at `monster_room_deep`. Its
        /// body spends 73 draws here and 75 in the port, and the port's 75
        /// decomposes cleanly — two radii, sixty-three mossy coins, four chest
        /// attempts at two each, a loot seed, a mob — while 73 admits no
        /// arrangement of the same terms. When totals stop decomposing the
        /// only way on is to see the sequence, and until now only the port's
        /// half of it was observable.
        static String logging;
        static boolean on;

        private Counting(RandomSource inner) {
            this.inner = inner;
        }

        private void log(String what, long value) {
            if (on) {
                System.err.println("DRAW " + draws + " " + what + " -> " + value);
            }
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
            int v = inner.nextInt();
            log("nextIntUnbounded(0)", v);
            return v;
        }

        @Override
        public int nextInt(int bound) {
            draws++;
            int v = inner.nextInt(bound);
            log("nextInt(" + bound + ")", v);
            return v;
        }

        /// `nextLong` printed `log("nextLong", (int) v)` — no parentheses and
        /// the high 32 bits thrown away — while the port printed
        /// `nextLong(0) -> <the whole long>`. Two logs that can never match on
        /// a `nextLong` line, in a tool whose entire purpose is diffing them.
        /// Nothing had noticed because no traced feature had drawn one yet.
        @Override
        public long nextLong() {
            draws++;
            long v = inner.nextLong();
            log("nextLong(0)", v);
            return v;
        }

        /// The four below print their value as raw **bits**, not as text.
        ///
        /// Java and Rust format the same float differently, and a formatting
        /// difference in a draw log reads exactly like a value difference —
        /// which is the one thing this log exists to rule out. Bits are the
        /// only representation both sides can print identically.
        @Override
        public boolean nextBoolean() {
            draws++;
            boolean v = inner.nextBoolean();
            log("nextBoolean(0)", v ? 1 : 0);
            return v;
        }

        @Override
        public float nextFloat() {
            draws++;
            float v = inner.nextFloat();
            log("nextFloat(0)", Float.floatToRawIntBits(v));
            return v;
        }

        @Override
        public double nextDouble() {
            draws++;
            double v = inner.nextDouble();
            log("nextDouble(0)", Double.doubleToRawLongBits(v));
            return v;
        }

        @Override
        public double nextGaussian() {
            draws++;
            double v = inner.nextGaussian();
            log("nextGaussian(0)", Double.doubleToRawLongBits(v));
            return v;
        }
    }

    private DecorateGolden() {}

    /// The configured features as a real `Registry`, for the one decorator that
    /// asks the level for them.
    ///
    /// Same shape as `SurfaceChunkGolden.buildBiomeRegistry` and for the same
    /// reason: `HolderLookup.Provider` is not a `Registry`, and
    /// `RegistryAccess.ImmutableRegistryAccess` wants registries. Registering
    /// `h.value()` rather than a copy is what keeps the placed feature the very
    /// object the lookup resolved.
    static Registry<ConfiguredFeature<?, ?>> buildConfiguredFeatureRegistry(
        HolderLookup.Provider lookup
    ) {
        MappedRegistry<ConfiguredFeature<?, ?>> features =
            new MappedRegistry<>(Registries.CONFIGURED_FEATURE, Lifecycle.stable());
        lookup.lookupOrThrow(Registries.CONFIGURED_FEATURE).listElements()
            .forEach(h -> features.register(h.key(), h.value(), RegistrationInfo.BUILT_IN));
        return features.freeze();
    }

}
