// SurfaceChunkGolden — vanilla's buildSurface output for a real chunk.
//
// The surface *rules* are the last unproven part of the surface system: the
// conditions and the column walk are careful reads of the decompile, which is
// the standard that produced two 56%-error bugs in the density layer. This
// runs vanilla's own `SurfaceSystem.buildSurface` over a populated chunk and
// dumps every block, so the comparison is a number rather than a claim.
//
// `SurfaceSystem.buildSurface` is used rather than
// `NoiseBasedChunkGenerator.buildSurface` because the system-level entry
// point takes the NoiseChunk directly and so needs no StructureManager and no
// live level.
//
// The awkward part is getting a `ProtoChunk` at all, and it is worth writing
// down because it is not obvious:
//
//   ProtoChunk needs a PalettedContainerFactory
//     -> PalettedContainerFactory.create needs a RegistryAccess
//        -> which must contain a real Registry<Biome>, for asHolderIdMap,
//           getOrThrow and holderByNameCodec
//           -> but VanillaRegistries.createLookup() yields a
//              HolderLookup.Provider, and RegistryLayer's STATIC layer has
//              no biome registry at all.
//
// The way through is to build a MappedRegistry from the lookup's elements and
// wrap it in an ImmutableRegistryAccess containing only that. Blocks come
// from `Block.BLOCK_STATE_REGISTRY`, a static, so the biome registry is the
// only one the factory actually reads.
//
// That rebuilt registry is a trap, and this harness fell in it. Its holders
// are *different objects* from the lookup's, and vanilla compares biome
// holders by identity. Anything the rule tree compares against — the biome
// table the chunk generator is built from — has to come from the lookup, not
// from here. See the comment at `overworldBiomes`, and ORACLE.md 5b.

package net.minecraft.world.level.levelgen;

import com.mojang.serialization.Lifecycle;
import java.util.ArrayList;
import java.util.List;

import com.mojang.datafixers.util.Pair;
import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistrationInfo;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.util.Mth;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.biome.MultiNoiseBiomeSourceParameterList;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.levelgen.carver.CarvingContext;
import net.minecraft.world.level.levelgen.carver.ConfiguredWorldCarver;
import net.minecraft.world.level.chunk.CarvingMask;
import net.minecraft.core.QuartPos;
import net.minecraft.tags.TagKey;
import net.minecraft.tags.TagLoader;
import net.minecraft.world.level.block.Block;
import net.minecraft.resources.Identifier;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.google.gson.JsonElement;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.Map;
import java.util.Set;

public final class SurfaceChunkGolden {

    /// How many carvers the sweep considered, and how many passed
    /// `isStartChunk`. Emitted with the vectors as coverage: see the note
    /// where they are written.
    static int CARVERS_SEEN = 0;
    static int CARVERS_STARTED = 0;
    /// Off for `--find` and `--survey`: those generate thousands of chunks and
    /// carvers cost 289 replays each, while neither search looks at anything a
    /// carver produces.
    static boolean RUN_CARVERS = true;
    /// `-Ddump.fill.density=x,y,z` prints the density the fill computes at
    /// one block, from inside the interpolator loop.
    static final int[] DUMP_FILL_DENSITY = parseTriple(System.getProperty("dump.fill.density"));
    /// `-Ddump.biome=x,y,z` prints `BiomeManager#getBiome` at one block — the
    /// fuzzed lookup the surface rules are handed, not the quart grid.
    static final int[] DUMP_BIOME = parseTriple(System.getProperty("dump.biome"));

    private static int[] parseTriple(String raw) {
        if (raw == null) return null;
        String[] parts = raw.split(",");
        return new int[] {Integer.parseInt(parts[0].trim()),
            Integer.parseInt(parts[1].trim()), Integer.parseInt(parts[2].trim())};
    }

    /// 31415926 is here for a different reason than the other two. They were
    /// chosen for block coverage; this one is the seed the whole-world diff
    /// runs at, and its chunks below sit inside the region where ~1,458
    /// carved blocks disagree. Without it this gate measures the surface
    /// stage only where the surface stage was already known to be right —
    /// which is how a carver spent five hypotheses being the suspect for
    /// something that may be upstream of it (ORACLE.md rule 13: stage gates
    /// cannot see seams).
    ///
    /// `-Dsurface.seeds=` and `-Dsurface.chunks=` override both tables, so a
    /// chunk that a *whole-world* diff says is wrong can be gated directly
    /// instead of hoping the fixed set happens to contain its case. It did
    /// not: the ocean-floor sand/gravel band that costs 51 blocks on the
    /// 121-chunk window at seed **20260722** is not covered by any seed or
    /// chunk below, which is exactly why this gate stayed green while the
    /// world diff did not.
    private static final long[] SEEDS = parseSeeds();

    private static long[] parseSeeds() {
        String raw = System.getProperty("surface.seeds");
        if (raw == null) {
            return new long[] {0L, 20260725L, 31415926L};
        }
        String[] parts = raw.split(",");
        long[] out = new long[parts.length];
        for (int i = 0; i < parts.length; i++) {
            out[i] = Long.parseLong(parts[i].trim());
        }
        return out;
    }

    private static int[][] parseChunks(int[][] fallback) {
        String raw = System.getProperty("surface.chunks");
        if (raw == null) {
            return fallback;
        }
        String[] parts = raw.split(";");
        int[][] out = new int[parts.length][2];
        for (int i = 0; i < parts.length; i++) {
            String[] xz = parts[i].split(",");
            out[i] = new int[] {Integer.parseInt(xz[0].trim()), Integer.parseInt(xz[1].trim())};
        }
        return out;
    }
    /// Chosen, not guessed. Three chunks near spawn measure whatever those
    /// three chunks happen to contain, and the first honest coverage
    /// measurement of this gate was ten of the twenty-six blocks the
    /// overworld rule can place.
    ///
    /// - `(34, -57)` is a frozen ocean under seed 20260725, and a sulfur cave
    ///   under seed 0 — the latter is what exposed the biome-holder bug.
    /// - `(0, -135)` is a badlands under seed 0, which is six of the missing
    ///   blocks at once: the terracotta stripes plus red sand and sandstone.
    /// - `(-76, -37)` is a mushroom island under seed 0, for mycelium.
    /// - `(-164, 47)` is a snowy peak under seed 0 — snow, and the ice the
    ///   frozen chunks did not reach.
    /// - `(-123, -168)` is a mangrove swamp under seed 0, for mud.
    /// - `(80, 153)` is a stony peak under seed 0, for calcite — which the
    ///   rule gates on `stony_peaks` and not, as I first searched, on the
    ///   frozen ones.
    /// - `(-174, 18)` is a grove under seed 0, for powder snow.
    /// - `(171, 14)` is an old-growth pine taiga under seed 0, for podzol.
    /// - `(22, -111)` is a wooded badlands under seed 0, for coarse dirt.
    /// - `(-4, -195)` is the one chunk in this set that produces red
    ///   sandstone. Its guard chain wants `stone_depth` **floor** 0 and
    ///   `stone_depth` **ceiling** 0 at the same block — the top and the
    ///   bottom of a stone run at once, which is a one-block-thick ledge with
    ///   air or fluid above and below. Rare, and worth the search: it is the
    ///   twenty-sixth of twenty-six blocks the overworld rule can place.
    ///
    /// The last two were found by `locate_biome`, a biome-only scan, after a
    /// `--find` for podzol spent fifty minutes generating chunks of
    /// `old_growth_birch_forest` — which my substring matched and which does
    /// not place podzol at all. Asking "where is this biome" and "does this
    /// chunk contain this block" as one question wasted the time; they are
    /// separate searches now.
    ///
    /// Both were located with `ci/surface-chunk-golden.sh --find`, which
    /// searches the *oracle* rather than the port — a search run through the
    /// port could only ever find chunks the port already handles.
    private static final int[][] DEFAULT_CHUNKS = {
        {0, 0}, {-1, -1}, {137, -509}, {34, -57}, {0, -135}, {-76, -37}, {-164, 47},
        {-123, -168}, {80, 153}, {-174, 18}, {171, 14}, {22, -111}, {-4, -195},
        // Three chunks inside the r7 carve disagreement at seed 31415926,
        // picked by mismatch count: (60, 68) had 70 differing mineshaft
        // blocks, (63, 65) 57, (62, 67) 51. Under the other two seeds they
        // are ordinary chunks and cost only their generation.
        {60, 68}, {63, 65}, {62, 67},
        // The eight chunks around (0, -135). The aquifer's
        // skip_sampling_above_y takes a maximum of preliminarySurfaceLevel
        // over a grid padded well past the chunk edge, so a peak in a
        // neighbour decides whether a column inside gets water or stone —
        // and dumping only the centre chunk cannot see it.
        {-1, -136}, {0, -136}, {1, -136},
        {-1, -135},            {1, -135},
        {-1, -134}, {0, -134}, {1, -134},
    };

    /// The table above unless `-Dsurface.chunks` overrides it. Declared after
    /// `DEFAULT_CHUNKS` because a static initialiser may not read a field
    /// declared below it.
    private static final int[][] CHUNKS = parseChunks(DEFAULT_CHUNKS);

    public static void main(String[] args) throws java.io.IOException {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        bindBlockTags();
        bindFluidTags();

        // `--find <seed> <radius> <block> [biomeSubstring]` scans for a chunk
        // whose surface output contains a given block, and prints its
        // coordinates. The optional biome filter is a cheap pre-reject:
        // generating a chunk costs roughly a thousand times what a biome
        // lookup does, so naming the biome that can produce the block turns a
        // twelve-minute sweep into a ten-second one.
        //
        // This lives here rather than on the Rust side because it is choosing
        // what the gate measures: a search run through the port could only
        // ever find chunks the port already gets right, and would report
        // "none found" for a condition the port had broken. Asking the oracle
        // where the interesting chunks are keeps the gate's coverage
        // independent of the thing being gated.
        boolean hunting = args.length >= 4 && args[0].equals("--find");
        if (hunting || (args.length == 3 && args[0].equals("--survey"))) {
            RUN_CARVERS = false;
        }

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        HolderGetter<net.minecraft.world.level.levelgen.synth.NormalNoise.NoiseParameters> noises =
            lookup.lookupOrThrow(Registries.NOISE);
        Holder<NoiseGeneratorSettings> settingsHolder = lookup
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(NoiseGeneratorSettings.OVERWORLD);
        NoiseGeneratorSettings settings = settingsHolder.value();
        NoiseSettings noiseSettings = settings.noiseSettings();

        Registry<Biome> biomeRegistry = buildBiomeRegistry(lookup);
        RegistryAccess access = new RegistryAccess.ImmutableRegistryAccess(List.of(biomeRegistry));
        PalettedContainerFactory containers = PalettedContainerFactory.create(access);
        LevelHeightAccessor heights =
            LevelHeightAccessor.create(noiseSettings.minY(), noiseSettings.height());

        // The real overworld biome table, resolved through the **lookup** —
        // the same one `settings.surfaceRule()` was resolved against.
        //
        // Resolving it through `biomeRegistry` instead is a silent disaster,
        // and this harness did exactly that for one round. `BiomeConditionSource`
        // tests `holderSet.contains(context.getBiome())`, and holder equality is
        // identity: a holder minted by the rebuilt MappedRegistry can never equal
        // the one baked into the rule, so *every* `biome` condition returns false.
        // The output still looks like a world — grass and dirt come from the
        // rule's biome-independent default — it just quietly loses desert sand,
        // badlands bands, and the sulfur caves. A dump that is wrong in a way
        // that looks plausible is the worst thing an oracle can be, which is why
        // the registry is threaded from one place now.
        Climate.ParameterList<Holder<Biome>> parameters = overworldBiomes(lookup);
        MultiNoiseBiomeSource biomeSource = MultiNoiseBiomeSource.createFromList(parameters);
        NoiseBasedChunkGenerator generator = new NoiseBasedChunkGenerator(biomeSource, settingsHolder);

        // `--probe <seed> <x> <y> <z>` prints the *inputs* the rule tree reads
        // at one block: the fuzzed biome and every named noise, both ways.
        // When an aggregate diverges, observing its inputs beats bisecting
        // hypotheses about it — this is the same move that closed the density
        // layer's 1-ulp gap.
        if (args.length == 5 && args[0].equals("--probe")) {
            long seed = Long.parseLong(args[1]);
            int px = Integer.parseInt(args[2]);
            int py = Integer.parseInt(args[3]);
            int pz = Integer.parseInt(args[4]);
            RandomState state = RandomState.create(settings, noises, seed);
            Climate.Sampler sampler = state.sampler();
            BiomeManager bm = new BiomeManager(
                (qx, qy, qz) -> biomeSource.getNoiseBiome(qx, qy, qz, sampler),
                BiomeManager.obfuscateSeed(seed));
            var holder = bm.getBiome(new BlockPos(px, py, pz));
            System.out.println("PROBE biome=" + holder.unwrapKey().map(k -> k.identifier().toString()).orElse("?"));
            var noiseKey = net.minecraft.resources.ResourceKey.create(
                Registries.NOISE, net.minecraft.resources.Identifier.parse("minecraft:sulfur_cave_gradient"));
            var noise = state.getOrCreateNoise(noiseKey);
            System.out.println("PROBE noise3d=" + noise.getValue(px, py, pz)
                + " bits=" + Double.doubleToRawLongBits(noise.getValue(px, py, pz)));
            System.out.println("PROBE noise2d=" + noise.getValue(px, 0.0, pz));
            return;
        }

        // `--chunk <seed> <cx> <cz>` prints one named chunk as flat block
        // lines at BOTH intermediate stages:
        //
        //   SURF x y z state   after `SurfaceSystem.buildSurface`
        //   CARV x y z state   after the carver sweep
        //
        // The default mode already computes both — it just RLE-encodes them
        // into a vector file keyed on a hard-coded chunk list, which is the
        // right shape for a gate and the wrong one for an attribution. An
        // attribution asks about the chunk a census pointed at, and that
        // chunk is never in `CHUNKS`.
        //
        // The format is `NetherRealFillProbe`'s deliberately, so that a fill
        // dump, a surface dump and a carver dump are three files a single
        // comparator can read. `ci/fill-compare.sh` is that comparator; the
        // point of the ladder is that "our finished world differs here" then
        // resolves to a *stage* instead of to a guess about one.
        //
        // Air is printed too, unlike the fill probe, because a carver's whole
        // output is air: a dump that omitted it could not show a carve at all.
        // It writes to a named file rather than to stdout, and that is not a
        // convenience. `Bootstrap.bootStrap()` redirects `System.out` into
        // log4j, which re-emits every line with a `[STDOUT]: ` prefix — so a
        // 98,304-line dump is either 98,304 log events or one event whose
        // prefix strips off only the first line. Both have been shipped in
        // this tree. A `PrintWriter` on a real file has neither problem.
        if (args.length == 5 && args[0].equals("--chunk")) {
            long seed = Long.parseLong(args[1]);
            int cx = Integer.parseInt(args[2]);
            int cz = Integer.parseInt(args[3]);
            java.io.PrintWriter sink = new java.io.PrintWriter(
                new java.io.BufferedWriter(new java.io.FileWriter(args[4])));
            RandomState state = RandomState.create(settings, noises, seed);
            List<String> surfPalette = new ArrayList<>();
            List<String> surfRuns = new ArrayList<>();
            ProtoChunk chunk = generateChunk(
                state, access, settings, noiseSettings, containers, heights,
                biomeSource, generator, seed, cx, cz, surfPalette, surfRuns,
                null, null);
            int minY = noiseSettings.minY();
            int height = noiseSettings.height();
            sink.println("SEED " + seed + " CHUNK " + cx + "," + cz
                + " minY=" + minY + " height=" + height
                + " carversSeen=" + CARVERS_SEEN + " carversStarted=" + CARVERS_STARTED);
            emitRuns(sink, "SURF", surfPalette, surfRuns, cx, cz, minY, height);
            List<String> carvPalette = new ArrayList<>();
            List<String> carvRuns = new ArrayList<>();
            encode(chunk, new BlockPos.MutableBlockPos(), new ChunkPos(cx, cz),
                minY, height, carvPalette, carvRuns);
            emitRuns(sink, "CARV", carvPalette, carvRuns, cx, cz, minY, height);
            sink.flush();
            sink.close();
            System.out.println("CHUNK DUMP WRITTEN " + args[4]
                + " carversSeen=" + CARVERS_SEEN + " carversStarted=" + CARVERS_STARTED);
            return;
        }

        // `--survey <seed> <radius>` walks chunks and prints one whenever it
        // produces a block no earlier chunk did, greedily building a small set
        // that covers as much of the rule tree's output vocabulary as it can.
        //
        // This exists because the gate's first honest coverage measurement was
        // 10 of the 26 blocks the overworld rule can place. Three chunks near
        // spawn will never contain a badlands, a mangrove swamp or a mushroom
        // island, so a chunk list chosen by hand measures whatever those
        // chunks happen to be and quietly says nothing about the rest.
        if (args.length == 3 && args[0].equals("--survey")) {
            long seed = Long.parseLong(args[1]);
            int radius = Integer.parseInt(args[2]);
            RandomState state = RandomState.create(settings, noises, seed);
            java.util.Set<String> seen = new java.util.HashSet<>();
            // A spiral out from the origin, so the chunks it names stay as
            // close to spawn as the vocabulary allows — nearer chunks are
            // cheaper to regenerate and easier to look at in a real world.
            for (int ring = 0; ring < radius; ring++) {
                for (int chunkX = -ring; chunkX <= ring; chunkX++) {
                    for (int chunkZ = -ring; chunkZ <= ring; chunkZ++) {
                        if (Math.max(Math.abs(chunkX), Math.abs(chunkZ)) != ring) {
                            continue;
                        }
                        String dump = dumpChunk(
                            state, access, settings, noiseSettings, containers, heights,
                            biomeSource, generator, seed, chunkX, chunkZ);
                        java.util.Set<String> here = paletteOf(dump);
                        if (!seen.containsAll(here)) {
                            here.removeAll(seen);
                            seen.addAll(here);
                            System.out.println("NEW " + chunkX + " " + chunkZ + " " + here);
                        }
                    }
                }
            }
            System.out.println("DONE " + seen.size() + " distinct blocks");
            return;
        }

        if (hunting) {
            long seed = Long.parseLong(args[1]);
            int radius = Integer.parseInt(args[2]);
            String wanted = args[3];
            String biomeFilter = args.length > 4 ? args[4] : null;
            RandomState state = RandomState.create(settings, noises, seed);
            Climate.Sampler sampler = state.sampler();
            for (int chunkX = -radius; chunkX < radius; chunkX++) {
                for (int chunkZ = -radius; chunkZ < radius; chunkZ++) {
                    if (biomeFilter != null) {
                        var biome = biomeSource.getNoiseBiome(
                            chunkX * 4 + 2, 15, chunkZ * 4 + 2, sampler);
                        if (!biome.unwrapKey()
                                .map(k -> k.identifier().toString().contains(biomeFilter))
                                .orElse(false)) {
                            continue;
                        }
                    }
                    String dump = dumpChunk(
                        state, access, settings, noiseSettings, containers, heights,
                        biomeSource, generator, seed, chunkX, chunkZ);
                    if (dump.contains("\"" + wanted + "\"")) {
                        System.out.println("FOUND " + seed + " " + chunkX + " " + chunkZ);
                    }
                }
            }
            System.out.println("DONE");
            return;
        }

        List<String> cases = new ArrayList<>();
        for (long seed : SEEDS) {
            RandomState state = RandomState.create(settings, noises, seed);
            for (int[] chunkPos : CHUNKS) {
                cases.add(dumpChunk(
                    state, access, settings, noiseSettings, containers, heights,
                    biomeSource, generator, seed, chunkPos[0], chunkPos[1]));
            }
        }

        // Carver coverage, in the vectors rather than on a log line: a
        // sweep that starts 567 carvers and touches no block is exactly what
        // an unbound block-tag registry looks like, and it took a counter to
        // notice. Keeping the numbers means a future regression to zero is
        // visible in the diff.
        StringBuilder out = new StringBuilder("{\n  \"rng\": \"surface_chunk\",\n");
        out.append("  \"carversSeen\": ").append(CARVERS_SEEN).append(",\n");
        out.append("  \"carversStarted\": ").append(CARVERS_STARTED).append(",\n");
        out.append("  \"cases\": [\n");
        for (int i = 0; i < cases.size(); i++) {
            out.append("    ").append(cases.get(i));
            if (i + 1 < cases.size()) {
                out.append(',');
            }
            out.append('\n');
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// `VanillaRegistries.createLookup()` gives a lookup, not a registry, and
    /// `PalettedContainerFactory` needs the real thing. Rebuild it.
    static Registry<Biome> buildBiomeRegistry(HolderLookup.Provider lookup) {
        MappedRegistry<Biome> biomes = new MappedRegistry<>(Registries.BIOME, Lifecycle.stable());
        lookup.lookupOrThrow(Registries.BIOME).listElements()
            .forEach(h -> biomes.register(h.key(), h.value(), RegistrationInfo.BUILT_IN));
        return biomes.freeze();
    }

    public static Climate.ParameterList<Holder<Biome>> overworldBiomes(HolderLookup.Provider lookup) {
        return presetBiomes(lookup, MultiNoiseBiomeSourceParameterList.Preset.OVERWORLD);
    }

    /// The nether's five-entry parameter list, from the same registry the
    /// overworld's comes from.
    ///
    /// Separate entry point rather than a boolean, because everything
    /// downstream of a biome source is dimension-specific and a caller that
    /// can flip this with a flag is a caller that can flip it by accident.
    public static Climate.ParameterList<Holder<Biome>> netherBiomes(HolderLookup.Provider lookup) {
        return presetBiomes(lookup, MultiNoiseBiomeSourceParameterList.Preset.NETHER);
    }

    /// `MultiNoiseBiomeSourceParameterList.knownPresets()`, resolved to
    /// holders.
    ///
    /// **The order is the answer, not a detail.** `Climate.ParameterList`
    /// keeps insertion order, `BiomeSource.possibleBiomes` collects through
    /// Guava's `ImmutableSet` (also insertion-ordered), and the feature
    /// sorter numbers features by first encounter across that list. So the
    /// order this returns decides every feature's seed.
    private static Climate.ParameterList<Holder<Biome>> presetBiomes(
        HolderLookup.Provider lookup, MultiNoiseBiomeSourceParameterList.Preset preset) {
        var biomes = lookup.lookupOrThrow(Registries.BIOME);
        var byKey = MultiNoiseBiomeSourceParameterList.knownPresets().get(preset);
        List<Pair<Climate.ParameterPoint, Holder<Biome>>> resolved = new ArrayList<>();
        for (var pair : byKey.values()) {
            resolved.add(Pair.of(pair.getFirst(), (Holder<Biome>) biomes.getOrThrow(pair.getSecond())));
        }
        return new Climate.ParameterList<>(resolved);
    }

    /// Generate one chunk through noise fill, surface rules and carvers, and
    /// hand back the `ProtoChunk` itself.
    ///
    /// Split out of `dumpChunk` so `OreGolden` can build a world out of these
    /// without a second copy of the pipeline. A second copy is exactly the
    /// shape of oracle fault this project keeps finding (ORACLE.md 5b, "thread
    /// it from one place"): two generation paths that drift apart produce two
    /// harnesses that disagree about a port that is fine.
    ///
    /// `surfacePalette`/`surfaceRuns`, when non-null, receive the chunk
    /// encoded at the point *between* the surface pass and the carvers — the
    /// only moment that state exists.
    static ProtoChunk generateChunk(
        RandomState state,
        RegistryAccess access,
        NoiseGeneratorSettings settings,
        NoiseSettings noiseSettings,
        PalettedContainerFactory containers,
        LevelHeightAccessor heights,
        MultiNoiseBiomeSource biomeSource,
        NoiseBasedChunkGenerator generator,
        long seed,
        int chunkX,
        int chunkZ,
        List<String> surfacePalette,
        List<String> surfaceRuns,
        /// When non-null, receives 256 `preliminarySurfaceLevel` values in
        /// `z * 16 + x` order. It is the input to `above_preliminary_surface`
        /// and it places no block itself, so nothing else in these vectors
        /// can tell whether a port computes it correctly — see
        /// swarm/tasks/w9-gate-the-density-queries.md.
        int[] preliminary,
        /// When non-null, receives 256 `getSurfaceDepth` values in
        /// `z * 16 + x` order — the other ungated context quantity behind the
        /// packed_ice failure.
        int[] surfaceDepth
    ) {
        ChunkPos pos = new ChunkPos(chunkX, chunkZ);
        ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, heights, containers, null);

        Aquifer.FluidStatus lava = new Aquifer.FluidStatus(-54, Blocks.LAVA.defaultBlockState());
        int seaLevel = settings.seaLevel();
        Aquifer.FluidStatus sea = new Aquifer.FluidStatus(seaLevel, settings.defaultFluid());
        Aquifer.FluidPicker picker = (x, y, z) -> y < Math.min(-54, seaLevel) ? lava : sea;

        int cellWidth = noiseSettings.getCellWidth();
        int cellHeight = noiseSettings.getCellHeight();
        int cellCountXZ = 16 / cellWidth;
        // `NoiseChunk.forChunk`, not the raw constructor.
        //
        // The server reaches its NoiseChunk through
        // `ChunkAccess.getOrCreateNoiseChunk(createNoiseChunk(...))`, which
        // calls `forChunk(chunk, randomState, beardifier, settings,
        // fluidPicker, blender)`. Building it by hand means choosing the cell
        // counts and the x/z origin ourselves, and the surface rules read
        // `above_preliminary_surface` and `Context.getMinSurfaceLevel` out of
        // this object rather than out of any placed block — so a difference
        // here is invisible to a block-by-block comparison of the chunk.
        NoiseChunk noiseChunk = NoiseChunk.forChunk(
            chunk, state, DensityFunctions.BeardifierMarker.INSTANCE,
            settings, picker, Blender.empty());

        // `NoiseBasedChunkGenerator.doCreateBiomes`, which vanilla runs at the
        // BIOMES step — before NOISE, and against the same NoiseChunk.
        //
        // Nothing in the surface or carver dumps reads the chunk's biome
        // container (both go through `biomeSource` directly), so this changes
        // no vector. It is here because `applyBiomeDecoration` collects the
        // biomes present in a chunk by walking each section's biome palette,
        // and an unfilled container makes that collection empty — which reads
        // as "no biome here lists any feature" and decorates nothing, with no
        // error. Same shape as the empty block tags in `bindBlockTags`.
        //
        // The sampler is `noiseChunk.cachedClimateSampler(...)`, not
        // `state.sampler()`. They are different objects and the difference is
        // not a caching detail: the cached one is built from the router with
        // the settings' spawn target.
        Climate.Sampler cachedSampler =
            noiseChunk.cachedClimateSampler(state.router(), settings.spawnTarget());
        chunk.fillBiomesFromNoise(
            net.minecraft.world.level.levelgen.BelowZeroRetrogen.getBiomeResolver(
                Blender.empty().getBiomeResolver(biomeSource), chunk),
            cachedSampler);

        // doFill, reduced to what a golden dump needs: block states and the
        // two heightmaps the surface walk reads.
        Heightmap oceanFloor = chunk.getOrCreateHeightmapUnprimed(Heightmap.Types.OCEAN_FLOOR_WG);
        Heightmap worldSurface = chunk.getOrCreateHeightmapUnprimed(Heightmap.Types.WORLD_SURFACE_WG);
        int minY = noiseSettings.minY();
        int cellMinY = Mth.floorDiv(minY, cellHeight);
        int cellCountY = Mth.floorDiv(noiseSettings.height(), cellHeight);
        BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();

        if (surfaceDepth != null) {
            // `SurfaceSystem#getSurfaceDepth` is protected and this dumper is
            // in the same package, so it can be asked directly rather than
            // inferred from what the rule did with it. That distinction is
            // the point: it is the input to `stone_depth`'s
            // `add_surface_depth` and to `water`'s multiplier, and it places
            // no block of its own.
            for (int z = 0; z < 16; z++) {
                for (int x = 0; x < 16; x++) {
                    surfaceDepth[z * 16 + x] =
                        state.surfaceSystem().getSurfaceDepth(chunkX * 16 + x, chunkZ * 16 + z);
                }
            }
        }
        if (preliminary != null) {
            for (int z = 0; z < 16; z++) {
                for (int x = 0; x < 16; x++) {
                    preliminary[z * 16 + x] =
                        noiseChunk.preliminarySurfaceLevel(chunkX * 16 + x, chunkZ * 16 + z);
                }
            }
        }
        noiseChunk.initializeForFirstCellX();
        for (int cellX = 0; cellX < cellCountXZ; cellX++) {
            noiseChunk.advanceCellX(cellX);
            for (int cellZ = 0; cellZ < cellCountXZ; cellZ++) {
                for (int cellY = cellCountY - 1; cellY >= 0; cellY--) {
                    noiseChunk.selectCellYZ(cellY, cellZ);
                    for (int yInCell = cellHeight - 1; yInCell >= 0; yInCell--) {
                        int y = (cellMinY + cellY) * cellHeight + yInCell;
                        noiseChunk.updateForY(y, (double) yInCell / cellHeight);
                        for (int xInCell = 0; xInCell < cellWidth; xInCell++) {
                            int x = pos.getMinBlockX() + cellX * cellWidth + xInCell;
                            noiseChunk.updateForX(x, (double) xInCell / cellWidth);
                            for (int zInCell = 0; zInCell < cellWidth; zInCell++) {
                                int z = pos.getMinBlockZ() + cellZ * cellWidth + zInCell;
                                noiseChunk.updateForZ(z, (double) zInCell / cellWidth);
                                // The density the fill actually hands the
                                // aquifer, taken from inside the loop rather
                                // than through a sampling API. The two are
                                // different paths and only the sampled one
                                // was ever gated; see docs/BENCH.md.
                                //
                                // It must be `noiseChunk.getInterpolatedDensity()`
                                // and not `state.router().finalDensity()`.
                                // `state.router()` is the **unwrapped** router:
                                // its `interpolated` nodes are plain
                                // `DensityFunctions.Marker`, whose `compute`
                                // delegates straight to the wrapped function,
                                // so computing it against `noiseChunk` yields
                                // the raw per-block noise. The fill reads
                                // `fullNoiseDensity`, which is
                                // `cacheAllInCell(add(finalDensity, beardifier))
                                //  .mapAll(this::wrapNew)` — every marker
                                // replaced by a live `NoiseInterpolator`.
                                //
                                // Raw and interpolated agree exactly on cell
                                // corners and nowhere else, which is a
                                // difference that looks like a lattice bug and
                                // is not one. That cost a full investigation
                                // (ORACLE.md rule 11: compare the most
                                // specific thing both sides express).
                                // A whole column: the y is ignored so one
                                // run gives every block of it, which is what
                                // a gate over the fill's own density needs.
                                if (DUMP_FILL_DENSITY != null
                                    && x == DUMP_FILL_DENSITY[0]
                                    && z == DUMP_FILL_DENSITY[2]) {
                                    try {
                                        java.nio.file.Files.writeString(
                                            java.nio.file.Path.of("/tmp/filldensity.txt"),
                                            seed + " " + x + "," + y + "," + z + " = "
                                                + noiseChunk.getInterpolatedDensity()
                                                + "\n",
                                            java.nio.file.StandardOpenOption.CREATE,
                                            java.nio.file.StandardOpenOption.APPEND);
                                    } catch (java.io.IOException ignored) {
                                    }
                                }
                                BlockState st = noiseChunk.getInterpolatedState();
                                if (st == null) {
                                    st = settings.defaultBlock();
                                }
                                if (!st.isAir()) {
                                    chunk.setBlockState(cursor.set(x, y, z), st);
                                    oceanFloor.update(x & 15, y, z & 15, st);
                                    worldSurface.update(x & 15, y, z & 15, st);
                                }
                            }
                        }
                    }
                }
            }
            // End of a cell-X column: hand the freshly filled slice over to
            // become the next column's low side. doFill does this and it is
            // easy to miss — without it every cell column past the first
            // interpolates against a stale slice, which looks like scattered
            // terrain differences rather than an obvious break.
            noiseChunk.swapSlices();
        }
        noiseChunk.stopInterpolation();

        // Now the part being measured.
        Climate.Sampler sampler = state.sampler();
        // Reads the CHUNK'S CONTAINER, not the biome source.
        //
        // A server's `region.getBiomeManager()` resolves quart cells out of
        // the stored biome container, and that container was filled by
        // `fillBiomesFromNoise` above from
        // `noiseChunk.cachedClimateSampler(...)`. Asking `biomeSource`
        // directly with `state.sampler()` is a *different computation*, and
        // the surface rules care: the overworld rule sends
        // warm/lukewarm/deep_lukewarm ocean to sand and everything else to
        // gravel, so one cell resolving differently flips sand to gravel.
        // Resolves out of the chunk's biome CONTAINER, as a server does.
        //
        // `region.getBiomeManager()` reads stored quart cells; the container
        // was filled by `fillBiomesFromNoise` from the cached climate sampler.
        // Asking `biomeSource` again at the same cell is a different
        // computation and disagrees at some of them — at seed 20260722 chunk
        // (63,68) the container holds `savanna` at (1013,45,1098) where a
        // resample says `lukewarm_ocean`, which is the difference between the
        // ocean branch's sand and the fall-through's gravel.
        //
        // Falls back to a resample only for cells outside this chunk, which a
        // server would read from a neighbour and a one-chunk harness cannot.
        BiomeManager biomeManager = new BiomeManager(
            (qx, qy, qz) -> {
                int by = QuartPos.toBlock(qy);
                if (QuartPos.toSection(qx) == pos.x() && QuartPos.toSection(qz) == pos.z()) {
                    int si = chunk.getSectionIndex(by);
                    if (si >= 0 && si < chunk.getSections().length) {
                        return chunk.getSection(si).getBiomes()
                            .get(qx & 3, qy & 3, qz & 3);
                    }
                }
                return biomeSource.getNoiseBiome(qx, qy, qz, cachedSampler);
            },
            BiomeManager.obfuscateSeed(seed));
        WorldGenerationContext generationContext = new WorldGenerationContext(generator, heights);

        // `-Ddump.biome=x,y,z` prints the biome the *rule* sees at one block:
        // `BiomeManager#getBiome`, the fuzzed lookup, which is what
        // `SurfaceRules.Context` is handed. That is a different value from the
        // quart grid the biome gate compares — the grid can match cell for
        // cell while the fuzz picks a different one of the eight candidates.
        // `-Ddump.biomes=x,y,z;x,y,z;...` — many positions, and for each one
        // BOTH the fuzzed answer the rule receives and the plain quart cell
        // it sits in. The two differ by design, and the ocean-floor rule
        // branches on the fuzzed one, so a dump of only the cell cannot
        // explain a sand-versus-gravel disagreement.
        String many = System.getProperty("dump.biomes");
        if (many != null) {
            StringBuilder sb = new StringBuilder();
            for (String one : many.split(";")) {
                String[] xyz = one.split(",");
                int bx = Integer.parseInt(xyz[0].trim());
                int by = Integer.parseInt(xyz[1].trim());
                int bz = Integer.parseInt(xyz[2].trim());
                String fuzzed = biomeManager
                    .getBiome(new net.minecraft.core.BlockPos(bx, by, bz))
                    .unwrapKey().<String>map(Object::toString).orElse("?");
                String cell = biomeSource.getNoiseBiome(
                        QuartPos.fromBlock(bx), QuartPos.fromBlock(by),
                        QuartPos.fromBlock(bz), cachedSampler)
                    .unwrapKey().<String>map(Object::toString).orElse("?");
                // And what the chunk's own biome CONTAINER holds there —
                // which is what a server's BiomeManager actually resolves,
                // filled by `fillBiomesFromNoise` from the *cached* climate
                // sampler. If this disagrees with `cell` above, then reading
                // the container and recomputing from the sampler are two
                // different answers, and every port that recomputes is wrong.
                String stored = "?";
                try {
                    int si = chunk.getSectionIndex(by);
                    if (si >= 0 && si < chunk.getSections().length) {
                        stored = chunk.getSection(si).getBiomes()
                            .get(QuartPos.fromBlock(bx) & 3,
                                 QuartPos.fromBlock(by) & 3,
                                 QuartPos.fromBlock(bz) & 3)
                            .unwrapKey().<String>map(Object::toString).orElse("?");
                    }
                } catch (RuntimeException e) {
                    stored = "ERR:" + e.getClass().getSimpleName();
                }
                sb.append(bx).append(',').append(by).append(',').append(bz)
                    .append("  fuzzed=").append(fuzzed)
                    .append("  cell=").append(cell)
                    .append("  stored=").append(stored).append('\n');
            }
            try {
                java.nio.file.Files.writeString(
                    java.nio.file.Path.of("/tmp/fuzzedbiome.txt"), sb.toString(),
                    java.nio.file.StandardOpenOption.CREATE,
                    java.nio.file.StandardOpenOption.APPEND);
            } catch (java.io.IOException ignored) {
            }
        }
        if (DUMP_BIOME != null) {
            try {
                java.nio.file.Files.writeString(
                    java.nio.file.Path.of("/tmp/fuzzedbiome.txt"),
                    seed + " " + DUMP_BIOME[0] + "," + DUMP_BIOME[1] + "," + DUMP_BIOME[2]
                        + " = "
                        + biomeManager.getBiome(new net.minecraft.core.BlockPos(
                            DUMP_BIOME[0], DUMP_BIOME[1], DUMP_BIOME[2]))
                            .unwrapKey().map(Object::toString).orElse("?")
                        + "\n",
                    java.nio.file.StandardOpenOption.CREATE,
                    java.nio.file.StandardOpenOption.APPEND);
            } catch (java.io.IOException ignored) {
            }
        }

        // `possibleBiomes` — and it is NOT optional.
        //
        // `NoiseBasedChunkGenerator.buildSurface` passes
        // `collectPossibleBiomes(region, 1)`: every biome in the chunk **and
        // its eight neighbours**, in a `ReferenceOpenHashSet`. This harness
        // passed `null`, which is legal — `BiomeConditionSource.apply` checks
        // for it — but not equivalent. With the set present the condition can
        // short-circuit to a constant via `canNeverMatch` or `willAlwaysMatch`
        // and **never call the per-block biome getter**; with null it is
        // evaluated per block. Those two answers are not always the same,
        // because the per-block lookup is `BiomeManager`'s *fuzzed* one and
        // can return a biome the shortcut did not reason about.
        //
        // Measured, not assumed: at seed 20260722 chunk (63,68) this harness
        // produced 177 sand / 79 gravel while three independently generated
        // vanilla **server** worlds all produced 161 / 95 — a 16-block disk at
        // y 45, and the only difference in the whole chunk.
        //
        // Sampled over the quart grid rather than read off chunk sections
        // because this harness generates one chunk, not a region;
        // `fillBiomesFromNoise` fills those containers from this same sampler,
        // so the resulting *set* is the same.
        java.util.Set<net.minecraft.core.Holder<net.minecraft.world.level.biome.Biome>>
            possibleBiomes = new it.unimi.dsi.fastutil.objects.ReferenceOpenHashSet<>();
        int quartMinY = QuartPos.fromBlock(minY);
        int quartMaxY = QuartPos.fromBlock(minY + noiseSettings.height()) - 1;
        for (int ncz = pos.z() - 1; ncz <= pos.z() + 1; ncz++) {
            for (int ncx = pos.x() - 1; ncx <= pos.x() + 1; ncx++) {
                int qx0 = QuartPos.fromSection(ncx);
                int qz0 = QuartPos.fromSection(ncz);
                for (int qx = qx0; qx < qx0 + 4; qx++) {
                    for (int qz = qz0; qz < qz0 + 4; qz++) {
                        for (int qy = quartMinY; qy <= quartMaxY; qy++) {
                            possibleBiomes.add(
                                biomeSource.getNoiseBiome(qx, qy, qz, sampler));
                        }
                    }
                }
            }
        }

        state.surfaceSystem().buildSurface(
            state, biomeManager, settings.useLegacyRandomSource(), generationContext,
            chunk, noiseChunk, settings.surfaceRule(), possibleBiomes);

        // Run-length encoded, y-major then z then x, matching the noise-stage
        // vectors' layout.
        if (surfacePalette != null) {
            encode(chunk, cursor, pos, minY, noiseSettings.height(), surfacePalette, surfaceRuns);
        }

        // ---- carvers ----
        //
        // Dumped from the same chunk, right after the surface pass, so a
        // difference here cannot be a terrain difference: the two sections of
        // each case share one generation. `NoiseBasedChunkGenerator.applyCarvers`
        // is not called directly because it needs a live `WorldGenRegion` with
        // its neighbours loaded; the sweep it performs is short enough to
        // reproduce exactly, and every piece of it is public.
        CarvingMask mask = new CarvingMask(noiseSettings.height(), minY);
        if (RUN_CARVERS) {
        // `applyCarvers` does not hand carvers the ordinary biome manager. It
        // wraps it:
        //
        //   biomeManager.withDifferentSource(
        //       (x, y, z) -> this.biomeSource.getNoiseBiome(x, y, z, sampler))
        //
        // so every per-position biome a carver asks about is resolved through
        // the **noise source**, not through the chunk's stored container. This
        // harness's `biomeManager` is container-backed on purpose — that is
        // what the surface stage needs and what a server does there — and
        // passing it to `carve` unwrapped made the carvers answer a different
        // question from vanilla's.
        //
        // Note the direction, because it is the opposite of the surface-stage
        // fix and that made it hard to recognise: there a server *reads* the
        // container and this project recomputed; here vanilla deliberately
        // recomputes and this harness was reading the container.
        BiomeManager carverBiomeManager = biomeManager.withDifferentSource(
            (qx, qy, qz) -> biomeSource.getNoiseBiome(qx, qy, qz, sampler));
        CarvingContext carvingContext = new CarvingContext(
            generator, access, heights, noiseChunk, state, settings.surfaceRule());
        Aquifer aquifer = noiseChunk.aquifer();
        WorldgenRandom carverRandom = new WorldgenRandom(new LegacyRandomSource(0L));

        for (int dx = -8; dx <= 8; dx++) {
            for (int dz = -8; dz <= 8; dz++) {
                ChunkPos sourcePos = new ChunkPos(chunkX + dx, chunkZ + dz);
                var sourceBiome = biomeSource.getNoiseBiome(
                    QuartPos.fromBlock(sourcePos.getMinBlockX()), 0,
                    QuartPos.fromBlock(sourcePos.getMinBlockZ()), sampler);
                int index = 0;
                for (Holder<ConfiguredWorldCarver<?>> holder
                        : sourceBiome.value().getGenerationSettings().getCarvers()) {
                    ConfiguredWorldCarver<?> carver = holder.value();
                    carverRandom.setLargeFeatureSeed(seed + index, sourcePos.x(), sourcePos.z());
                    CARVERS_SEEN++;
                    if (carver.isStartChunk(carverRandom)) {
                        CARVERS_STARTED++;
                        // The return value is not counted: both carvers
                        // return `true` unconditionally, so it says only
                        // that the method ran.
                        carver.carve(carvingContext, chunk, carverBiomeManager::getBiome,
                            carverRandom, aquifer, sourcePos, mask);
                    }
                    index++;
                }
            }
        }

        }

        return chunk;
    }

    private static String join(int[] values) {
        StringBuilder out = new StringBuilder();
        for (int i = 0; i < values.length; i++) {
            if (i > 0) out.append(", ");
            out.append(values[i]);
        }
        return out.toString();
    }

    private static String dumpChunk(
        RandomState state,
        RegistryAccess access,
        NoiseGeneratorSettings settings,
        NoiseSettings noiseSettings,
        PalettedContainerFactory containers,
        LevelHeightAccessor heights,
        MultiNoiseBiomeSource biomeSource,
        NoiseBasedChunkGenerator generator,
        long seed,
        int chunkX,
        int chunkZ
    ) {
        List<String> palette = new ArrayList<>();
        List<String> runs = new ArrayList<>();
        int[] preliminary = new int[256];
        int[] depth = new int[256];
        ProtoChunk chunk = generateChunk(
            state, access, settings, noiseSettings, containers, heights,
            biomeSource, generator, seed, chunkX, chunkZ, palette, runs,
            preliminary, depth);

        ChunkPos pos = new ChunkPos(chunkX, chunkZ);
        BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();
        int minY = noiseSettings.minY();
        Climate.Sampler sampler = state.sampler();

        List<String> carvedPalette = new ArrayList<>();
        List<String> carvedRuns = new ArrayList<>();
        encode(chunk, cursor, pos, minY, noiseSettings.height(), carvedPalette, carvedRuns);

        // The biome grid at quart resolution, straight from the biome source
        // rather than through BiomeManager's fuzz. Dumped because a wrong
        // biome and a wrong surface rule produce the same symptom — the wrong
        // block at the surface — and this tells them apart without a
        // bisection. Raw quarts rather than fuzzed block lookups so that a
        // fault in the climate sampler is not confused with one in the fuzz,
        // which `golden_biome_zoom` already covers on its own.
        List<String> biomePalette = new ArrayList<>();
        java.util.Map<String, Integer> biomeIndex = new java.util.HashMap<>();
        List<String> biomeIds = new ArrayList<>();
        int quartMinY = minY >> 2;
        int quartHeight = noiseSettings.height() >> 2;
        for (int qy = quartMinY; qy < quartMinY + quartHeight; qy++) {
            for (int qz = 0; qz < 4; qz++) {
                for (int qx = 0; qx < 4; qx++) {
                    var holder = biomeSource.getNoiseBiome(
                        (pos.getMinBlockX() >> 2) + qx, qy, (pos.getMinBlockZ() >> 2) + qz, sampler);
                    String name = holder.unwrapKey().map(k -> k.identifier().toString()).orElse("?");
                    Integer id = biomeIndex.get(name);
                    if (id == null) {
                        id = biomePalette.size();
                        biomeIndex.put(name, id);
                        biomePalette.add(quote(name));
                    }
                    biomeIds.add(id.toString());
                }
            }
        }

        return "{\"case\": \"surface_chunk\", \"seed\": " + quote(seed)
            + ", \"chunkX\": " + chunkX + ", \"chunkZ\": " + chunkZ
            + ", \"minY\": " + minY + ", \"height\": " + noiseSettings.height()
            + ", \"palette\": [" + String.join(", ", palette) + "]"
            + ", \"runs\": [" + String.join(", ", runs) + "]"
            + ", \"carvedPalette\": [" + String.join(", ", carvedPalette) + "]"
            + ", \"carvedRuns\": [" + String.join(", ", carvedRuns) + "]"
            + ", \"biomePalette\": [" + String.join(", ", biomePalette) + "]"
            + ", \"biomeQuarts\": [" + String.join(", ", biomeIds) + "]"
            + ", \"preliminarySurface\": [" + join(preliminary) + "]"
            + ", \"surfaceDepth\": [" + join(depth) + "]}";
    }

    /// The block names in a dumped case, read back out of its palette.
    private static java.util.Set<String> paletteOf(String dump) {
        java.util.Set<String> names = new java.util.HashSet<>();
        int start = dump.indexOf("\"palette\": [");
        int end = dump.indexOf(']', start);
        for (String token : dump.substring(start + 12, end).split(",")) {
            String name = token.trim();
            if (name.length() > 2) {
                names.add(name.substring(1, name.length() - 1));
            }
        }
        return names;
    }

    /// Run-length encode a chunk's blocks into a palette and run list.
    ///
    /// Called twice per case — after the surface pass and after the carvers —
    /// so the two sections are the same encoding of the same chunk at two
    /// points in its generation.
    static void encode(
        ProtoChunk chunk, BlockPos.MutableBlockPos cursor, ChunkPos pos,
        int minY, int height, List<String> palette, List<String> runs
    ) {
        java.util.Map<String, Integer> index = new java.util.HashMap<>();
        int runValue = -1;
        int runLength = 0;
        for (int y = minY; y < minY + height; y++) {
            for (int z = 0; z < 16; z++) {
                for (int x = 0; x < 16; x++) {
                    BlockState st = chunk.getBlockState(cursor.set(
                        pos.getMinBlockX() + x, y, pos.getMinBlockZ() + z));
                    // The **full state**, not the block name.
                    //
                    // The name is what this dumped for its first three days,
                    // and it is lossy in a way that matters: `minecraft:water`
                    // is sixteen states whose fluid differs, and
                    // `grass_block` has a `snowy` property the surface rules
                    // set. A gate comparing names cannot see a port that
                    // places the right block in the wrong state, and cannot
                    // answer `matching_fluids` at all.
                    String name = BlockStateParser.serialize(st);
                    Integer id = index.get(name);
                    if (id == null) {
                        id = palette.size();
                        index.put(name, id);
                        palette.add(quote(name));
                    }
                    if (id == runValue) {
                        runLength++;
                    } else {
                        if (runLength > 0) {
                            runs.add("[" + runValue + ", " + runLength + "]");
                        }
                        runValue = id;
                        runLength = 1;
                    }
                }
            }
        }
        if (runLength > 0) {
            runs.add("[" + runValue + ", " + runLength + "]");
        }
    }

    /// Expand what `encode` packed, as `<tag> x y z state` lines.
    ///
    /// The walk MUST be `encode`'s own: y outer, then z, then x. Writing it
    /// the other way round produces a file of the right length holding the
    /// right multiset of states at the wrong coordinates — which reads as a
    /// plausible world and is the failure this project has paid for more than
    /// once. The count assertion at the end is cheap and catches a palette or
    /// run list that did not cover the column.
    static void emitRuns(
        java.io.PrintWriter sink, String tag, List<String> palette, List<String> runs,
        int cx, int cz, int minY, int height
    ) {
        String[] names = new String[palette.size()];
        for (int i = 0; i < palette.size(); i++) {
            String q = palette.get(i);
            names[i] = q.startsWith("\"") ? q.substring(1, q.length() - 1) : q;
        }
        StringBuilder sb = new StringBuilder();
        int emitted = 0;
        int y = minY;
        int z = 0;
        int x = 0;
        for (String run : runs) {
            String[] p = run.replace("[", "").replace("]", "").split(",");
            int id = Integer.parseInt(p[0].trim());
            int len = Integer.parseInt(p[1].trim());
            for (int i = 0; i < len; i++) {
                sb.append(tag).append(' ').append(cx * 16 + x).append(' ').append(y)
                  .append(' ').append(cz * 16 + z).append(' ').append(names[id]).append('\n');
                emitted++;
                if (++x == 16) {
                    x = 0;
                    if (++z == 16) {
                        z = 0;
                        y++;
                    }
                }
            }
        }
        if (emitted != 16 * 16 * height) {
            throw new IllegalStateException(
                tag + ": runs cover " + emitted + " of " + (16 * 16 * height) + " positions");
        }
        sink.print(sb);
    }

    /// Bind `#minecraft:...` block tags into `BuiltInRegistries.BLOCK`.
    ///
    /// Without this every tag resolves to an **empty** holder set, because
    /// tags are datapack content and `Bootstrap.bootStrap()` loads no
    /// datapack. Carvers name what they may replace as a tag, so an unbound
    /// registry means `canReplaceBlock` is false everywhere and the carver
    /// sweep runs to completion having changed nothing — 567 carvers started,
    /// zero blocks touched, no error anywhere. That is how this was found.
    ///
    /// The tag files are Mojang's own datagen output, read and flattened here
    /// rather than run through `TagLoader`'s resource-manager path, which
    /// would need a real pack stack.
    /// Fluid tags, bound the same way and for the same reason.
    ///
    /// Missed for one round, and the way it showed was a feature that
    /// generated *nothing*: `TallSeagrassBlock.canSurvive` ends with
    /// `fluidState.is(FluidTags.WATER) && fluidState.isFull()`, and an unbound
    /// tag answers false for every fluid in the game. Vanilla's own dump then
    /// contained not one tall seagrass in three worlds — a plausible-looking
    /// world that no real server produces. Same shape as the empty block tags,
    /// and the same lesson: a tag registry that was never loaded does not
    /// throw, it just says no. ORACLE.md fault 8.
    static void bindFluidTags() {
        Map<String, java.util.List<String>> raw = readTagFolder("fluid");
        Map<TagKey<net.minecraft.world.level.material.Fluid>,
            java.util.List<Holder<net.minecraft.world.level.material.Fluid>>> bound =
            new HashMap<>();
        for (String name : raw.keySet()) {
            Set<String> flat = new LinkedHashSet<>();
            flatten(name, raw, flat, new LinkedHashSet<>());
            java.util.List<Holder<net.minecraft.world.level.material.Fluid>> holders =
                new ArrayList<>();
            for (String fluid : flat) {
                BuiltInRegistries.FLUID.get(Identifier.parse(fluid)).ifPresent(holders::add);
            }
            bound.put(TagKey.create(Registries.FLUID, Identifier.parse(name)), holders);
        }
        BuiltInRegistries.FLUID
            .prepareTagReload(new TagLoader.LoadResult<>(Registries.FLUID, bound))
            .apply();
    }

    /// One `data/minecraft/tags/<folder>` directory, read but not flattened.
    private static Map<String, java.util.List<String>> readTagFolder(String folder) {
        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("set -Drustmc.data to the generated-data directory");
        }
        Path dir = Path.of(data, "data", "minecraft", "tags", folder);
        Map<String, java.util.List<String>> raw = new HashMap<>();
        try (var files = Files.list(dir)) {
            for (Path file : files.toList()) {
                String name = file.getFileName().toString();
                if (!name.endsWith(".json")) {
                    continue;
                }
                JsonObject json = JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                java.util.List<String> entries = new ArrayList<>();
                for (JsonElement element : json.getAsJsonArray("values")) {
                    entries.add(element.isJsonObject()
                        ? element.getAsJsonObject().get("id").getAsString()
                        : element.getAsString());
                }
                raw.put("minecraft:" + name.substring(0, name.length() - 5), entries);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read " + folder + " tags from " + dir, e);
        }
        return raw;
    }

    static void bindBlockTags() {
        String data = System.getProperty("rustmc.data");
        if (data == null) {
            throw new IllegalStateException("set -Drustmc.data to the generated-data directory");
        }
        Path dir = Path.of(data, "data", "minecraft", "tags", "block");
        Map<String, java.util.List<String>> raw = new HashMap<>();
        try (var files = Files.list(dir)) {
            for (Path file : files.toList()) {
                String name = file.getFileName().toString();
                if (!name.endsWith(".json")) {
                    continue;
                }
                JsonObject json = JsonParser.parseString(Files.readString(file)).getAsJsonObject();
                java.util.List<String> entries = new ArrayList<>();
                for (JsonElement element : json.getAsJsonArray("values")) {
                    entries.add(element.isJsonObject()
                        ? element.getAsJsonObject().get("id").getAsString()
                        : element.getAsString());
                }
                raw.put("minecraft:" + name.substring(0, name.length() - 5), entries);
            }
        } catch (java.io.IOException e) {
            throw new IllegalStateException("cannot read block tags from " + dir, e);
        }

        Map<TagKey<Block>, java.util.List<Holder<Block>>> bound = new HashMap<>();
        for (String name : raw.keySet()) {
            Set<String> flat = new LinkedHashSet<>();
            flatten(name, raw, flat, new LinkedHashSet<>());
            java.util.List<Holder<Block>> holders = new ArrayList<>();
            for (String block : flat) {
                BuiltInRegistries.BLOCK.get(Identifier.parse(block)).ifPresent(holders::add);
            }
            bound.put(TagKey.create(Registries.BLOCK, Identifier.parse(name)), holders);
        }

        BuiltInRegistries.BLOCK
            .prepareTagReload(new TagLoader.LoadResult<>(Registries.BLOCK, bound))
            .apply();
    }

    private static void flatten(
        String name, Map<String, java.util.List<String>> raw, Set<String> out, Set<String> visiting
    ) {
        if (!visiting.add(name)) {
            throw new IllegalStateException("block tag cycle at " + name);
        }
        java.util.List<String> entries = raw.get(name);
        if (entries == null) {
            throw new IllegalStateException("block tag " + name + " does not exist");
        }
        for (String entry : entries) {
            if (entry.startsWith("#")) {
                flatten(entry.substring(1), raw, out, visiting);
            } else {
                out.add(entry);
            }
        }
        visiting.remove(name);
    }

    /// A 3x3 block of chunks, run-length encoded plus the one heightmap the
    /// ore feature reads. Shared by `OreGolden` and `DecorateGolden` so the
    /// Rust side has one world format to decode.
    ///
    /// `OCEAN_FLOOR_WG` is shipped rather than left to be recomputed, because
    /// it is **not** a function of the blocks beside it. In *this harness* it
    /// is filled during the noise fill and never touched again, and the value
    /// dumped here is therefore the noise-stage one.
    ///
    /// That is a property of the harness, not of vanilla, and the difference
    /// matters. `ProtoChunk#setBlockState` updates whatever
    /// `getPersistedStatus().heightmapsAfter()` names; the chunk built here
    /// never has a status set, so that set is empty and nothing updates. On a
    /// real server the persisted status lags the running step by one, so
    /// while SURFACE and CARVERS run the set is still `{OCEAN_FLOOR_WG,
    /// WORLD_SURFACE_WG}` and **both stages do update it** — a carver cutting
    /// an ocean floor lowers it, which is what puts `disk_sand` on the floor
    /// rather than four blocks up in open water.
    ///
    /// So this column is a noise-stage oracle. It is the right input for the
    /// ore and decorate gates, whose worlds come from this same chunk, and it
    /// must not be read as vanilla's value at the moment features run.
    ///
    /// The `Heightmap.primeHeightmaps` that
    /// `ChunkStatusTasks.generateFeatures` runs immediately before features
    /// primes `MOTION_BLOCKING`, `MOTION_BLOCKING_NO_LEAVES`, `OCEAN_FLOOR`
    /// and `WORLD_SURFACE` — every heightmap except this one. Recomputing it
    /// disagrees on 69% of columns, by up to 25 blocks.
    static String encodeWorld(ProtoChunk[] grid, int[] center, NoiseSettings noiseSettings) {
        List<String> chunks = new ArrayList<>();
        BlockPos.MutableBlockPos cursor = new BlockPos.MutableBlockPos();
        for (int dz = -1; dz <= 1; dz++) {
            for (int dx = -1; dx <= 1; dx++) {
                ProtoChunk chunk = grid[(dz + 1) * 3 + dx + 1];
                List<String> palette = new ArrayList<>();
                List<String> runs = new ArrayList<>();
                encode(chunk, cursor, new ChunkPos(center[0] + dx, center[1] + dz),
                    noiseSettings.minY(), noiseSettings.height(), palette, runs);
                List<String> floor = new ArrayList<>();
                List<String> surface = new ArrayList<>();
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        floor.add(Integer.toString(
                            chunk.getHeight(Heightmap.Types.OCEAN_FLOOR_WG, x, z)));
                        // `WORLD_SURFACE_WG` is shipped for exactly the reason
                        // `OCEAN_FLOOR_WG` is, and it was not until
                        // 2026-07-29: it is a **frozen** map by the time
                        // features run — CARVERS and every later status
                        // register `FINAL_HEIGHTMAPS`, which names neither
                        // `_WG` map — so nothing a feature writes moves it,
                        // and it cannot be recovered from the decorated world.
                        //
                        // `patch_grass_forest` is the placement that reads it,
                        // and the gate answered it from the *live*
                        // `WORLD_SURFACE` instead. That was invisible while
                        // this dumper's own live maps were frozen too; fixing
                        // them exposed it as a two-block y difference.
                        surface.add(Integer.toString(
                            chunk.getHeight(Heightmap.Types.WORLD_SURFACE_WG, x, z)));
                    }
                }
                chunks.add("{\"chunkX\": " + (center[0] + dx)
                    + ", \"chunkZ\": " + (center[1] + dz)
                    + ", \"palette\": [" + String.join(", ", palette) + "]"
                    + ", \"runs\": [" + String.join(", ", runs) + "]"
                    + ", \"oceanFloorWg\": [" + String.join(", ", floor) + "]"
                    + ", \"worldSurfaceWg\": [" + String.join(", ", surface) + "]}");
            }
        }
        return "{\"centerX\": " + center[0] + ", \"centerZ\": " + center[1]
            + ", \"minY\": " + noiseSettings.minY()
            + ", \"height\": " + noiseSettings.height()
            + ", \"chunks\": [" + String.join(", ", chunks) + "]}";
    }

    static String quote(long v) {
        return "\"" + v + "\"";
    }

    static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private SurfaceChunkGolden() {}
}
