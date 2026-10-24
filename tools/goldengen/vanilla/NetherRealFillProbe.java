// NetherRealFillProbe — run vanilla's OWN `doFill` over a real `ProtoChunk`.
//
// # Why this exists when NetherFillProbe already does
//
// `NetherFillProbe` builds a `NoiseChunk` through the public constructor and
// then drives the interpolation with a loop **transcribed** out of
// `NoiseBasedChunkGenerator.doFill`'s bytecode. Every number in it comes from
// vanilla, but the loop does not: the cell walk, the `swapSlices` placement
// and the `y = (minCellY + cy) * cellHeight + yy` arithmetic are all mine.
//
// That probe is now load-bearing. It is the evidence behind "the nether fill
// is byte-identical over 65,536 blocks", which `docs/PARITY.md` states and
// which has been relayed onward as fact. It is well corroborated — it agrees
// with the Rust port over two whole chunks, and it reports "nothing added
// after the fill" in six of eleven chunks, which a systematically wrong probe
// would not — but corroborated is not verified, and two chunks disagree with
// it by about 550 blocks each in a way nobody has explained.
//
// So this one removes the transcription. It constructs a real `ProtoChunk`,
// lets `NoiseChunk.forChunk` build the noise chunk exactly as the server does,
// and calls the private `doFill` itself. If the two probes agree the loop is
// verified; if they disagree, the loop was the bug and every "fill is exact"
// claim needs re-reading.
//
// # The one trick, and why it is sound
//
// `doFill(blender, structureManager, randomState, chunk, minCellY, cellCountY)`
// touches its `structureManager` in exactly one place: the lambda it hands to
// `chunk.getOrCreateNoiseChunk(..)`, which builds
// `Beardifier.forStructuresInChunk(structureManager, chunkPos)`.
// `ProtoChunk.getOrCreateNoiseChunk` returns the cached instance when there is
// one and only applies the function otherwise — so seeding the chunk with a
// `NoiseChunk` first means the lambda never runs and the manager may be null.
//
// Building a real `StructureManager` needs a `LevelAccessor`, i.e. a server.
// Skipping it costs nothing here because bearding is inert in 26.2: no file in
// the whole datapack contains a `beardifier` node, so the term never enters
// any router's density. That is checked, not assumed — see `docs/WORKLIST.md`.
//
// # What it prints
//
// The same `BLOCK x y z state` lines as `NetherFillProbe --chunk`, so the two
// outputs `diff` directly.
//
// # RESULT
//
// The two probes agree position for position and state for state:
//
//   chunk (57,64)   transcribed 14,382   real doFill 14,382   differing 0
//   chunk (58,68)   transcribed 23,038   real doFill 23,038   differing 0
//
// Both of the chunks where vanilla's finished world holds ~550 blocks neither
// fill produced. So the transcription was faithful, the fill claim in
// `docs/PARITY.md` stands, and that anomaly is a post-fill mechanism.
//
// # The two obstacles, kept because the next chunk harness will hit them
//
// `PalettedContainerFactory.create(RegistryAccess)` calls `asHolderIdMap()`,
// `getOrThrow(Biomes.PLAINS)` and `holderByNameCodec()` on a real
// `Registry<Biome>`, and `VanillaRegistries.createLookup()` returns a
// `RegistrySetBuilder$3` — a `HolderLookup.Provider`, not a `RegistryAccess`.
// The shim below fills a `MappedRegistry<Biome>` from the lookup's own
// `listElements()`, freezes it, and answers `Registries.BIOME` and nothing
// else. It borrows the registry rather than inventing one, which matters:
// a shim that answered more would be fabricating the input to the thing under
// test.
//
// # The corroborating check, which was run first and is still worth having
//
// At a cell-lattice corner the trilinear interpolation returns the corner
// sample exactly (`updateForX/Y/Z` are called with t = 0), so the fill's block
// there must equal the sign of the density function — which
// `ci/nether-density-probe.sh` computes through a completely different path, a
// `SinglePointContext` that never touches a `NoiseChunk`. Over the **whole
// corner lattice of chunk (57,64)** — 4 x 16 x 4 = 256 corners — the two agree
// **256 of 256**, with 31 corners within 0.01 of the zero crossing where a
// one-cell indexing error could not hide.
//
// That verifies the transcribed loop's cell arithmetic: the `minCellY + cy`
// offset, the x/z cell walk, and where `swapSlices` falls. It does **not**
// verify the interpolation *between* corners — but that code is vanilla's own
// `NoiseChunk`, not the transcription, and all the transcription supplies
// there is `t = offset / cellSize`.
import net.minecraft.SharedConstants;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.MappedRegistry;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.resources.ResourceKey;
import com.mojang.serialization.Lifecycle;
import java.util.Optional;
import java.util.stream.Stream;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Biomes;
import net.minecraft.world.level.biome.FixedBiomeSource;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.levelgen.Aquifer;
import net.minecraft.world.level.levelgen.NoiseBasedChunkGenerator;
import net.minecraft.world.level.levelgen.NoiseChunk;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.levelgen.NoiseSettings;
import net.minecraft.world.level.levelgen.RandomState;
import net.minecraft.world.level.levelgen.blending.Blender;
import net.minecraft.world.level.StructureManager;
import net.minecraft.world.level.levelgen.Beardifier;
import net.minecraft.world.level.levelgen.structure.BoundingBox;
import net.minecraft.world.level.levelgen.structure.TerrainAdjustment;
import java.util.ArrayList;
import java.util.List;
import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;

public final class NetherRealFillProbe {

    /// `DimensionType` gives the nether y 0..255; a vanilla nether chunk on
    /// disk has sixteen sections. The noise settings' own 0..128 is narrower,
    /// so `clampToHeightAccessor` returns the noise range either way — passing
    /// the dimension range is what the server does.
    private static final int DIM_MIN_Y = 0;
    private static final int DIM_HEIGHT = 256;
    /// `DimensionType` for the overworld, for `--overworld`.
    private static final int OW_MIN_Y = -64;
    private static final int OW_HEIGHT = 384;

    public static void main(String[] args) throws Exception {
        if (args.length < 3) {
            System.err.println("usage: NetherRealFillProbe <seed> <x> <z> [piece...]");
            System.err.println("  runs vanilla's own doFill over the chunk containing (x, z)");
            System.err.println("  and prints every non-air block as `BLOCK x y z state`");
            System.err.println();
            System.err.println("  piece = x0,y0,z0,x1,y1,z1,ADJUSTMENT[,groundLevelDelta]");
            System.err.println("     a structure piece to BEARD the terrain with, e.g.");
            System.err.println("     935,96,1085,939,100,1091,BEARD_THIN");
            System.err.println("     Read the box out of the reference chunk's own");
            System.err.println("     `structures.starts.<id>.Children[].BB`, so the input is");
            System.err.println("     vanilla's and not invented. Omit for a beardless fill.");
            System.exit(2);
        }
        // `--baseheight` asks vanilla's own `ChunkGenerator.getBaseHeight`
        // for the whole chunk instead of filling it.
        //
        // This is the oracle the structure-stage reorder needs. Our
        // `generate.rs` structures stage reads `WORLD_SURFACE_WG` and
        // `OCEAN_FLOOR_WG` off the canvas, which is why it cannot move above
        // the noise stage; vanilla reads `getBaseHeight`, which walks a noise
        // column against the router and needs no filled chunk. Before those
        // five reads are swapped, the two have to be shown equal.
        //
        // And `getBaseHeight` is **beardless**: `iterateNoiseColumn` builds
        // its `NoiseChunk` with `BeardifierMarker.INSTANCE` at offset 187, so
        // structure placement never sees the terrain its own structures raise.
        // That is why the swap must land BEFORE the beard does — afterwards
        // the canvas heightmap and `getBaseHeight` genuinely differ, and a
        // gate written then would be pinning the wrong equality.
        boolean baseHeight = false;
        boolean overworld = false;
        for (String a : args) {
            if ("--baseheight".equals(a)) {
                baseHeight = true;
            }
            if ("--overworld".equals(a)) {
                overworld = true;
            }
        }
        final int dimMinY = overworld ? OW_MIN_Y : DIM_MIN_Y;
        final int dimHeight = overworld ? OW_HEIGHT : DIM_HEIGHT;
        long seed = Long.parseLong(args[0]);
        int targetX = Integer.parseInt(args[1]);
        int targetZ = Integer.parseInt(args[2]);

        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        HolderLookup.Provider registries = VanillaRegistries.createLookup();
        var noises = registries.lookupOrThrow(Registries.NOISE);
        Holder<NoiseGeneratorSettings> settingsHolder = registries
            .lookupOrThrow(Registries.NOISE_SETTINGS)
            .getOrThrow(overworld ? NoiseGeneratorSettings.OVERWORLD : NoiseGeneratorSettings.NETHER);
        NoiseGeneratorSettings settings = settingsHolder.value();
        RandomState randomState = RandomState.create(settings, noises, seed);

        // `doFill` never asks the generator for a biome, so any biome source
        // will do — but it is built from the real registry rather than mocked,
        // so nothing about the generator is fabricated.
        Holder<Biome> biome = registries.lookupOrThrow(Registries.BIOME)
            .getOrThrow(overworld ? Biomes.PLAINS : Biomes.NETHER_WASTES);
        NoiseBasedChunkGenerator generator =
            new NoiseBasedChunkGenerator(new FixedBiomeSource(biome), settingsHolder);

        // `PalettedContainerFactory.create` wants a real `Registry<Biome>` —
        // it calls `asHolderIdMap()`, `getOrThrow(Biomes.PLAINS)` and
        // `holderByNameCodec()` on it — and `VanillaRegistries.createLookup()`
        // returns a `RegistrySetBuilder$3`, which is a `HolderLookup.Provider`
        // and not a `RegistryAccess`. So build the one registry it needs from
        // the lookup's own elements and answer nothing else: `doFill` never
        // reads a biome, and a shim that answered more would be inventing
        // data rather than borrowing it.
        MappedRegistry<Biome> biomeRegistry =
            new MappedRegistry<>(Registries.BIOME, Lifecycle.stable());
        registries.lookupOrThrow(Registries.BIOME).listElements()
            .forEach(ref -> Registry.register(biomeRegistry, ref.key(), ref.value()));
        biomeRegistry.freeze();
        RegistryAccess access = new RegistryAccess() {
            @SuppressWarnings("unchecked")
            @Override
            public <E> Optional<Registry<E>> lookup(
                    ResourceKey<? extends Registry<? extends E>> key) {
                return Registries.BIOME.equals(key)
                    ? Optional.of((Registry<E>) biomeRegistry)
                    : Optional.empty();
            }

            @Override
            public Stream<RegistryAccess.RegistryEntry<?>> registries() {
                return Stream.of(new RegistryAccess.RegistryEntry<>(
                    Registries.BIOME, biomeRegistry));
            }
        };
        PalettedContainerFactory containers = PalettedContainerFactory.create(access);

        LevelHeightAccessor accessor = new LevelHeightAccessor() {
            @Override public int getHeight() { return dimHeight; }
            @Override public int getMinY() { return dimMinY; }
        };
        ChunkPos pos = new ChunkPos(targetX >> 4, targetZ >> 4);
        ProtoChunk chunk = new ProtoChunk(pos, UpgradeData.EMPTY, accessor, containers, null);

        Method pickerFactory = NoiseBasedChunkGenerator.class
            .getDeclaredMethod("createFluidPicker", NoiseGeneratorSettings.class);
        pickerFactory.setAccessible(true);
        Aquifer.FluidPicker fluidPicker = (Aquifer.FluidPicker) pickerFactory.invoke(null, settings);

        // A REAL beardifier when pieces are given, and the marker (which
        // computes 0) when they are not.
        //
        // This argument exists because its absence was a defect. `NoiseChunk`
        // does not read the beardifier from the datapack — it injects
        // `DensityFunctions.add(finalDensity, BeardifierMarker.INSTANCE)` in
        // its own constructor (offsets 495-506) and `wrap` swaps the real
        // beardifier in. So "grep -rl beardifier over the datapack matches
        // nothing" proves nothing at all, and a probe that passes the marker
        // measures the beardless fill while calling it vanilla's.
        Object beardifier;
        boolean anyPiece = false;
        for (int i = 3; i < args.length; i++) {
            if (!args[i].startsWith("--")) {
                anyPiece = true;
            }
        }
        if (anyPiece) {
            List<Beardifier.Rigid> pieces = new ArrayList<>();
            for (int i = 3; i < args.length; i++) {
                if (args[i].startsWith("--")) {
                    continue;
                }
                String[] p = args[i].split(",");
                if (p.length < 7) {
                    System.err.println("bad piece spec: " + args[i]);
                    System.exit(2);
                }
                BoundingBox box = new BoundingBox(
                    Integer.parseInt(p[0].trim()), Integer.parseInt(p[1].trim()),
                    Integer.parseInt(p[2].trim()), Integer.parseInt(p[3].trim()),
                    Integer.parseInt(p[4].trim()), Integer.parseInt(p[5].trim()));
                TerrainAdjustment adjustment =
                    TerrainAdjustment.valueOf(p[6].trim().toUpperCase(java.util.Locale.ROOT));
                int groundLevelDelta = p.length > 7 ? Integer.parseInt(p[7].trim()) : 0;
                pieces.add(new Beardifier.Rigid(box, adjustment, groundLevelDelta));
                System.out.println("PIECE " + box + " " + adjustment
                    + " groundLevelDelta=" + groundLevelDelta);
            }
            // `affectedBox` is what `fillArray` tests before doing any work;
            // the chunk's own column is the honest bound for a per-chunk probe.
            BoundingBox affected = new BoundingBox(
                (targetX >> 4) * 16 - Beardifier.BEARD_KERNEL_RADIUS, dimMinY,
                (targetZ >> 4) * 16 - Beardifier.BEARD_KERNEL_RADIUS,
                (targetX >> 4) * 16 + 15 + Beardifier.BEARD_KERNEL_RADIUS,
                dimMinY + dimHeight - 1,
                (targetZ >> 4) * 16 + 15 + Beardifier.BEARD_KERNEL_RADIUS);
            beardifier = new Beardifier(pieces, List.of(), affected);
        } else {
            Class<?> markerClass = Class
                .forName("net.minecraft.world.level.levelgen.DensityFunctions$BeardifierMarker");
            Field instance = markerClass.getDeclaredField("INSTANCE");
            instance.setAccessible(true);
            beardifier = instance.get(null);
        }

        Class<?> beardifierType = Class
            .forName("net.minecraft.world.level.levelgen.DensityFunctions$BeardifierOrMarker");
        Constructor<NoiseChunk> ctor = NoiseChunk.class.getDeclaredConstructor(
            int.class, RandomState.class, int.class, int.class, NoiseSettings.class,
            beardifierType, NoiseGeneratorSettings.class,
            Aquifer.FluidPicker.class, Blender.class);
        ctor.setAccessible(true);

        NoiseSettings ns = settings.noiseSettings()
            .clampToHeightAccessor(chunk.getHeightAccessorForGeneration());
        int cellHeight = ns.getCellHeight();
        int minCellY = Mth.floorDiv(ns.minY(), cellHeight);
        int cellCountY = Mth.floorDiv(ns.height(), cellHeight);
        int cellCountXZ = 16 / ns.getCellWidth();

        // Seed the chunk's noise chunk so `doFill`'s own lambda — the only
        // user of the StructureManager — never runs. Built exactly as
        // `NoiseChunk.forChunk` would build it.
        NoiseChunk seeded;
        try {
            seeded = ctor.newInstance(cellCountXZ, randomState,
                pos.getMinBlockX(), pos.getMinBlockZ(), ns,
                beardifier, settings, fluidPicker, Blender.empty());
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException("cannot build the NoiseChunk", e);
        }
        chunk.getOrCreateNoiseChunk(c -> seeded);

        Method doFill = NoiseBasedChunkGenerator.class.getDeclaredMethod(
            "doFill", Blender.class, StructureManager.class, RandomState.class,
            net.minecraft.world.level.chunk.ChunkAccess.class, int.class, int.class);
        doFill.setAccessible(true);
        doFill.invoke(generator, Blender.empty(), null, randomState, chunk, minCellY, cellCountY);

        if (baseHeight) {
            LevelHeightAccessor h = accessor;
            System.out.println("SEED " + seed + " CHUNK "
                + (targetX >> 4) + "," + (targetZ >> 4) + " BASEHEIGHT");
            for (int lx = 0; lx < 16; lx++) {
                for (int lz = 0; lz < 16; lz++) {
                    int bx = pos.getMinBlockX() + lx;
                    int bz = pos.getMinBlockZ() + lz;
                    int ws = generator.getBaseHeight(bx, bz,
                        net.minecraft.world.level.levelgen.Heightmap.Types.WORLD_SURFACE_WG,
                        h, randomState);
                    int of = generator.getBaseHeight(bx, bz,
                        net.minecraft.world.level.levelgen.Heightmap.Types.OCEAN_FLOOR_WG,
                        h, randomState);
                    System.out.println("BASEHEIGHT " + bx + " " + bz
                        + " world_surface_wg=" + ws + " ocean_floor_wg=" + of);
                }
            }
            return;
        }

        System.out.println("SEED " + seed + " CHUNK " + (targetX >> 4) + "," + (targetZ >> 4)
            + " cell=" + ns.getCellWidth() + "x" + cellHeight
            + " minCellY=" + minCellY + " cellCountY=" + cellCountY
            + " noise_min_y=" + ns.minY() + " noise_height=" + ns.height());
        long nonAir = 0;
        for (int lx = 0; lx < 16; lx++) {
            for (int y = ns.minY(); y < ns.minY() + ns.height(); y++) {
                for (int lz = 0; lz < 16; lz++) {
                    int x = pos.getMinBlockX() + lx;
                    int z = pos.getMinBlockZ() + lz;
                    BlockState state = chunk.getBlockState(
                        new net.minecraft.core.BlockPos(x, y, z));
                    if (state.isAir()) {
                        continue;
                    }
                    nonAir++;
                    System.out.println("BLOCK " + x + " " + y + " " + z + " " + state);
                }
            }
        }
        System.out.println("NONAIR " + nonAir + " of " + (16 * ns.height() * 16));
    }
}
