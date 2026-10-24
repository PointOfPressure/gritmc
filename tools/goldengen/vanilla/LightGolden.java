// LightGolden — vanilla's own light engine, dumped at the wire.
//
// # Why this and not the corpus
//
// The section light in `ClientboundLevelChunkWithLightPacket` is the payload
// of the most common packet in the game and nothing had ever compared it. The
// obvious instrument — a gameplay scenario — cannot do it, for two reasons
// that are both structural rather than fixable:
//
//   * `trace::observe` records a chunk packet only when its block-entity list
//     is non-empty. Measured by the chunk-send lane: **132 chunk packets
//     inside the window, 1 recorded.** That filter is correct and must stay,
//     so a light check hung off the corpus would be scoring one packet.
//   * `ci/t2-gameplay.sh` runs on **superflat**, whose own header says
//     "superflat makes every column identical". Light there is a function of
//     y alone. A light comparison on a world with uniform light is `710`
//     again — the check that ran while the code path did not, because a
//     superflat world measured `power=0` on both sides.
//
// So the observation point is here: vanilla's `LevelLightEngine`, in process,
// driven through the exact sequence `ThreadedLevelLightEngine` uses, over
// worlds this file chooses — including ones whose light is *not* uniform,
// which is the whole point. The artefact it dumps is the packet's own light
// section, built by `ClientboundLightUpdatePacketData` itself, so the golden
// is the wire bytes and not an intermediate anybody had to interpret.
//
// # The sequence, from the bytecode
//
// `ThreadedLevelLightEngine.initializeLight(chunk, isLighted)`:
//   for each section index i, if !sections[i].hasOnlyAir():
//       updateSectionStatus(SectionPos.of(pos, getSectionYFromSectionIndex(i)), false)
//   setLightEnabled(pos, isLighted); retainData(pos, false)
// `ThreadedLevelLightEngine.lightChunk(chunk, isLighted)`:
//   if (!isLighted) propagateLightSources(pos)
// then the executor drains `runLightUpdates()`.
//
// # It throws rather than answers
//
// Same discipline as `GenLevel`: the `BlockGetter` handed to the engine as
// `getLevel()` is a `java.lang.reflect.Proxy` that answers only the
// `LevelHeightAccessor` methods the engine is established to call — one call
// site, `SkyLightEngine`'s constructor, building its empty-chunk sources —
// and throws for everything else. A stub returning `Blocks.AIR` from a method
// nobody foresaw would light a world that is not the world and report no
// problem (ORACLE.md 5b).

package net.minecraft.world.level.levelgen;

import java.io.IOException;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.BitSet;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderGetter;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.SectionPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.biome.Biome;
import net.minecraft.world.level.biome.Climate;
import net.minecraft.world.level.biome.MultiNoiseBiomeSource;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.LevelChunkSection;
import net.minecraft.world.level.chunk.LightChunk;
import net.minecraft.world.level.chunk.LightChunkGetter;
import net.minecraft.world.level.chunk.PalettedContainerFactory;
import net.minecraft.world.level.chunk.ProtoChunk;
import net.minecraft.world.level.chunk.UpgradeData;
import net.minecraft.world.level.levelgen.NoiseGeneratorSettings;
import net.minecraft.world.level.lighting.LevelLightEngine;
import net.minecraft.world.level.lighting.LightEngine;

public final class LightGolden {

    /// The 3x3 grid every world is built on. Light crosses chunk borders, so
    /// a single chunk cannot show a cross-border defect at all — see the
    /// `radius` field of each case in the output, and the note in
    /// `docs/PARITY-ROADMAP.md` 75 about what radius makes a defect visible.
    private static final int RADIUS =
        Integer.getInteger("light.radius", 1);

    public static void main(String[] args) throws IOException {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
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

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  \"version\": \"").append(SharedConstants.getCurrentVersion().name()).append("\",\n");
        out.append("  \"min_section_y\": ").append(heights.getMinSectionY()).append(",\n");
        out.append("  \"sections_count\": ").append(heights.getSectionsCount()).append(",\n");
        out.append("  \"light_section_count\": ").append(heights.getSectionsCount() + 2).append(",\n");
        out.append("  \"radius\": ").append(RADIUS).append(",\n");
        out.append("  \"cases\": [\n");

        List<String> cases = new ArrayList<>();
        cases.add(dumpCase("void", heights, containers, LightGolden::buildVoid));
        cases.add(dumpCase("superflat", heights, containers, LightGolden::buildSuperflat));
        cases.add(dumpCase("roofed", heights, containers, LightGolden::buildRoofed));
        cases.add(dumpCase("emitters", heights, containers, LightGolden::buildEmitters));
        cases.add(dumpCase("overhang", heights, containers, LightGolden::buildOverhang));
        cases.add(dumpCase("relit", heights, containers,
            LightGolden::buildSealedRoom, LightGolden::removeTheOnlySource,
            "sealed_dark"));
        cases.add(dumpCase("tower", heights, containers, LightGolden::buildTower));
        cases.add(dumpCase("sealed_dark", heights, containers, LightGolden::buildSealedDark));
        cases.add(dumpCase("slab_roof", heights, containers, LightGolden::buildSlabRoof));
        // The mutation cases. Everything above reaches the engine only
        // through propagateLightSources; none of them enqueues a single
        // decrease, so a port with no decrease queue at all passes every one.
        // These five are what grade the other half.
        // No `from_scratch_control`, and the reason is a measurement:
        // vanilla's post-edit answer here is byte-identical to
        // `sealed_room_control`'s from-scratch one on all nine chunks. Adding
        // a source materialises a layer that then holds non-zero values, and
        // `DataLayer.isEmpty()` is false either way — so unlike `relit` there
        // is no history divergence to pin, and declaring a control would make
        // the divergence assertion measure nothing. `sealed_room_control` is
        // still dumped, because it is what establishes that.
        cases.add(dumpCase("lit_dark_room", heights, containers,
            LightGolden::buildSealedDark, LightGolden::addASourceToTheDark, null));
        cases.add(dumpCase("sealed_room_control", heights, containers,
            LightGolden::buildSealedRoom));
        cases.add(dumpCase("roof_opened", heights, containers,
            LightGolden::buildRoofed, LightGolden::openTheRoof, null));
        cases.add(dumpCase("sky_sealed", heights, containers,
            LightGolden::buildSuperflat, LightGolden::sealTheSky, null));
        cases.add(dumpCase("source_moved", heights, containers,
            LightGolden::buildEmitters, LightGolden::moveTheSource, null));

        for (int i = 0; i < cases.size(); i++) {
            out.append(cases.get(i));
            out.append(i + 1 < cases.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");

        Path target = Paths.get(args.length > 0 ? args[0] : "light_vectors.json");
        Files.writeString(target, out.toString(), StandardCharsets.UTF_8);
        System.out.println("LIGHTGOLDEN wrote " + target);
    }

    /// A world builder: fills one chunk of the grid.
    private interface Builder {
        void fill(ProtoChunk chunk, LevelHeightAccessor heights);
    }

    /// A change applied to an **already-lit** world, the way a player editing
    /// a block does: set the state, tell the engine, drain again. Vanilla's
    /// route is `ServerLevel.setBlock` -> `getLightEngine().checkBlock(pos)`.
    ///
    /// This exists for one reason. `DataLayer.isEmpty()` is
    /// `data == null && defaultValue == 0` — it reads the representation, not
    /// the contents — so a section whose layer was materialised and then went
    /// dark reports NOT-empty and puts 2048 zero bytes on the wire. Nothing
    /// in the five static worlds reaches that state, so without a mutation
    /// case that claim would be an assertion about the bytecode with no
    /// artefact behind it.
    private interface Mutator {
        void apply(ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
                   List<String> record);
    }

    /// Apply one block edit the way `ServerLevel.setBlock` does, and record
    /// it so a consumer can reproduce the edit rather than guess it.
    ///
    /// `from` and `to` are both written. The dumped `world` is the world
    /// AFTER the mutation, so a consumer that wants the pre-mutation world —
    /// which is the only world it can legitimately light from scratch —
    /// inverts these. Recording only `to` would make that impossible and the
    /// mutation cases ungradeable by anything but a re-implementation of the
    /// builders, which ORACLE.md 5b forbids.
    private static void edit(
        ProtoChunk chunk, LevelLightEngine engine, List<String> record,
        BlockPos pos, BlockState to
    ) {
        BlockState from = chunk.getBlockState(pos);
        // `LevelChunk.setBlockState`'s own order, and it is load-bearing: the
        // engine is told about a section that has just stopped being all-air
        // BEFORE `checkBlock`, because `checkNode` returns immediately for a
        // section the storage is not storing light for. Without this the
        // golden records an edit vanilla's engine never sees, and any
        // consumer that does maintain the section status diverges from it by
        // being MORE correct than the reference.
        LevelChunkSection sec = chunk.getSections()[chunk.getSectionIndex(pos.getY())];
        boolean wasAir = sec.hasOnlyAir();
        chunk.setBlockState(pos, to, 0);
        if (wasAir != sec.hasOnlyAir()) {
            engine.updateSectionStatus(SectionPos.of(pos), sec.hasOnlyAir());
        }
        // `ChunkSkyLightSources.update` — the piece whose absence made the
        // first version of these cases measure nothing. A `ProtoChunk` built
        // with a null light engine and a status before INITIALIZE_LIGHT skips
        // its whole light branch in `setBlockState`, so the per-column source
        // heightmap stays at its pre-edit values; `SkyLightEngine.checkNode`
        // then reads the OLD `getLowestSourceY`, decides the sealed position
        // is still a sky source, and vanilla's answer is that nothing got
        // darker. Measured, not reasoned: without this line vanilla keeps 15
        // under an 8x8 stone lid.
        if (LightEngine.hasDifferentLightProperties(from, to)) {
            chunk.getSkyLightSources().update(
                chunk, SectionPos.sectionRelative(pos.getX()), pos.getY(),
                SectionPos.sectionRelative(pos.getZ()));
        }
        engine.checkBlock(pos);
        record.add("{\"x\": " + pos.getX() + ", \"y\": " + pos.getY()
            + ", \"z\": " + pos.getZ()
            + ", \"from\": \"" + stateKey(from) + "\""
            + ", \"to\": \"" + stateKey(to) + "\"}");
    }

    /// Empty. Every section is air, so `initializeLight` calls
    /// `updateSectionStatus` for none of them — the case that exercises the
    /// **absent** wire state, which is the one a port that only knows "empty
    /// or data" cannot produce.
    private static void buildVoid(ProtoChunk chunk, LevelHeightAccessor heights) {
    }

    /// `minecraft:flat`'s shipped preset, which is the substrate
    /// `ci/t2-gameplay.sh` runs both servers on: bedrock, dirt, dirt, grass
    /// from the world bottom up.
    private static void buildSuperflat(ProtoChunk chunk, LevelHeightAccessor heights) {
        int minY = heights.getMinY();
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                int bx = chunk.getPos().getMinBlockX() + x;
                int bz = chunk.getPos().getMinBlockZ() + z;
                chunk.setBlockState(p.set(bx, minY, bz), Blocks.BEDROCK.defaultBlockState(), 0);
                chunk.setBlockState(p.set(bx, minY + 1, bz), Blocks.DIRT.defaultBlockState(), 0);
                chunk.setBlockState(p.set(bx, minY + 2, bz), Blocks.DIRT.defaultBlockState(), 0);
                chunk.setBlockState(p.set(bx, minY + 3, bz), Blocks.GRASS_BLOCK.defaultBlockState(), 0);
            }
        }
    }

    /// Superflat with a solid stone roof at y = 5 across the whole grid, and
    /// a floor at y = 0. The volume between them is enclosed, so its sky
    /// light is genuinely zero and its block light is genuinely zero — which
    /// is the case that separates "empty" from "materialised and dark", the
    /// distinction `DataLayer.isEmpty()` makes on allocation rather than on
    /// content.
    private static void buildRoofed(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSuperflat(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                int bx = chunk.getPos().getMinBlockX() + x;
                int bz = chunk.getPos().getMinBlockZ() + z;
                chunk.setBlockState(p.set(bx, 0, bz), Blocks.STONE.defaultBlockState(), 0);
                chunk.setBlockState(p.set(bx, 5, bz), Blocks.STONE.defaultBlockState(), 0);
            }
        }
    }

    /// The roofed world with light sources under the roof: one glowstone at
    /// the centre of each chunk and a torch offset from it, so block light
    /// has a real gradient with two overlapping sources and crosses a chunk
    /// border. Block light is otherwise **identically zero in every world
    /// above**, and a comparison of zero against zero is not a comparison.
    private static void buildEmitters(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildRoofed(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int bx = chunk.getPos().getMinBlockX();
        int bz = chunk.getPos().getMinBlockZ();
        chunk.setBlockState(p.set(bx + 8, 1, bz + 8), Blocks.GLOWSTONE.defaultBlockState(), 0);
        // Hard against the +x edge, so its gradient leaves the chunk.
        chunk.setBlockState(p.set(bx + 15, 3, bz + 2), Blocks.TORCH.defaultBlockState(), 0);
    }

    /// A stone shelf covering half of each chunk at y = 70 over open ground,
    /// so sky light spills sideways and downwards under the lip and every
    /// nibble under the shelf is a different value. This is the case that
    /// makes the **values** non-uniform; the others mostly grade the masks.
    private static void buildOverhang(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSuperflat(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int bx = chunk.getPos().getMinBlockX();
        int bz = chunk.getPos().getMinBlockZ();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 8; z++) {
                chunk.setBlockState(p.set(bx + x, 70, bz + z), Blocks.STONE.defaultBlockState(), 0);
            }
        }
    }

    /// A roof made of BOTTOM slabs rather than full blocks.
    ///
    /// This is the one case in this file that reaches
    /// `getLightDampeningInto`'s shape path. A slab has
    /// `canOcclude() && useShapeForLightOcclusion()`, so
    /// `LightEngine.isEmptyShape` is false for it and occlusion is decided by
    /// `Shapes.mergedFaceOccludes` on the real voxel shapes — not by
    /// `getLightDampening()`, which for a slab is 0.
    ///
    /// `ci/block-props-golden.sh` counts 6,762 of 32,366 states (20.9%) in
    /// that position. Counting them establishes how many states a
    /// dampening-only engine COULD be wrong about; it does not establish that
    /// it is wrong about any. This case is what turns the count into a
    /// reading.
    private static void buildSlabRoof(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSuperflat(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int bx = chunk.getPos().getMinBlockX();
        int bz = chunk.getPos().getMinBlockZ();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                chunk.setBlockState(p.set(bx + x, 0, bz + z), Blocks.STONE.defaultBlockState(), 0);
                // The default `stone_slab` is `type=bottom`, which occludes
                // its DOWN face and not its UP one.
                chunk.setBlockState(
                    p.set(bx + x, 5, bz + z), Blocks.STONE_SLAB.defaultBlockState(), 0);
            }
        }
    }

    /// **The control for `relit`.** Byte for byte the same final world —
    /// a sealed stone room with no light source in it — but reached by
    /// lighting it once from scratch instead of by removing a glowstone.
    ///
    /// It exists to attribute the `relit` divergence. If vanilla lighting
    /// this world from scratch puts the room's section in the EMPTY mask,
    /// then `relit`'s data-mask-with-2048-zero-bytes is a property of the
    /// edit history and not of the world, and a from-scratch relight
    /// disagreeing with it is correct rather than defective. If vanilla puts
    /// it in the data mask here too, the opposite, and the rule is about the
    /// world after all. Either way the answer is measured.
    private static void buildSealedDark(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSealedRoom(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        chunk.setBlockState(
            p.set(chunk.getPos().getMinBlockX() + 8, 2, chunk.getPos().getMinBlockZ() + 8),
            Blocks.AIR.defaultBlockState(), 0);
    }

    /// Superflat everywhere, plus a stone pillar rising to y = 120 in the
    /// **centre chunk only**.
    ///
    /// This exists to separate two rules that every other case in this file
    /// confuses, because every other case makes all nine chunks identical.
    /// The observed stored-section set has so far been exactly
    ///
    ///     {non-air sections of this chunk} union {their vertical neighbours}
    ///
    /// but a storage that marked a section and all SIX of its neighbours
    /// would give the same answer whenever the neighbouring chunks have the
    /// same non-air sections. Here they do not: chunk (1, 0) is plain
    /// superflat while chunk (0, 0) is filled to y = 120. If (1, 0) comes
    /// back with sections up at the pillar's height, the rule is
    /// three-dimensional; if it comes back with only its own surface
    /// sections, the rule is per-column.
    private static void buildTower(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSuperflat(chunk, heights);
        if (chunk.getPos().x() != 0 || chunk.getPos().z() != 0) {
            return;
        }
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int bx = chunk.getPos().getMinBlockX();
        int bz = chunk.getPos().getMinBlockZ();
        for (int y = heights.getMinY() + 4; y <= 120; y++) {
            for (int x = 6; x < 10; x++) {
                for (int z = 6; z < 10; z++) {
                    chunk.setBlockState(
                        p.set(bx + x, y, bz + z), Blocks.STONE.defaultBlockState(), 0);
                }
            }
        }
    }

    /// A sealed stone room with a single glowstone in it, under a superflat
    /// sky. `removeTheOnlySource` then takes the glowstone away, leaving the
    /// room genuinely dark — and the block-light layer for that section
    /// materialised. See `Mutator`.
    private static void buildSealedRoom(ProtoChunk chunk, LevelHeightAccessor heights) {
        buildSuperflat(chunk, heights);
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        int bx = chunk.getPos().getMinBlockX();
        int bz = chunk.getPos().getMinBlockZ();
        for (int x = 0; x < 16; x++) {
            for (int z = 0; z < 16; z++) {
                chunk.setBlockState(p.set(bx + x, 0, bz + z), Blocks.STONE.defaultBlockState(), 0);
                chunk.setBlockState(p.set(bx + x, 5, bz + z), Blocks.STONE.defaultBlockState(), 0);
            }
        }
        chunk.setBlockState(p.set(bx + 8, 2, bz + 8), Blocks.GLOWSTONE.defaultBlockState(), 0);
    }

    private static void removeTheOnlySource(
        ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
        List<String> record
    ) {
        BlockPos.MutableBlockPos p = new BlockPos.MutableBlockPos();
        for (ProtoChunk c : grid) {
            int bx = c.getPos().getMinBlockX();
            int bz = c.getPos().getMinBlockZ();
            edit(c, engine, record,
                new BlockPos(bx + 8, 2, bz + 8), Blocks.AIR.defaultBlockState());
        }
    }

    /// Put a glowstone into a sealed dark room. The pure INCREASE mutation:
    /// `checkNode` sees an emission where the stored level was zero, takes
    /// the `PULL_LIGHT_IN_ENTRY` arm rather than the decrease arm, and
    /// enqueues `increaseLightFromEmission`.
    private static void addASourceToTheDark(
        ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
        List<String> record
    ) {
        for (ProtoChunk c : grid) {
            int bx = c.getPos().getMinBlockX();
            int bz = c.getPos().getMinBlockZ();
            edit(c, engine, record,
                new BlockPos(bx + 8, 2, bz + 8), Blocks.GLOWSTONE.defaultBlockState());
        }
    }

    /// Take the roof off a sealed room: one block of it, in the centre chunk
    /// only. Sky light falls through a 1x1 hole and spreads sideways under
    /// the roof, so this grades `SkyLightEngine.checkNode`,
    /// `updateSourcesInColumn` and `addSourcesAbove` — the sky INCREASE path
    /// that no static world reaches, because in a static world every column's
    /// source is already where it will end up.
    private static void openTheRoof(
        ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
        List<String> record
    ) {
        for (ProtoChunk c : grid) {
            if (c.getPos().x() != 0 || c.getPos().z() != 0) {
                continue;
            }
            int bx = c.getPos().getMinBlockX();
            int bz = c.getPos().getMinBlockZ();
            edit(c, engine, record,
                new BlockPos(bx + 8, 5, bz + 8), Blocks.AIR.defaultBlockState());
        }
    }

    /// The opposite, and the one that only a decrease queue can get right:
    /// put a solid block on open superflat ground, so the column below it
    /// stops being a sky source and a column of 15s must be **removed**.
    /// `removeSourcesBelow` is unreachable in every other case in this file.
    private static void sealTheSky(
        ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
        List<String> record
    ) {
        for (ProtoChunk c : grid) {
            int bx = c.getPos().getMinBlockX();
            int bz = c.getPos().getMinBlockZ();
            for (int x = 4; x < 12; x++) {
                for (int z = 4; z < 12; z++) {
                    edit(c, engine, record,
                        new BlockPos(bx + x, 12, bz + z), Blocks.STONE.defaultBlockState());
                }
            }
        }
    }

    /// Remove one source and add another six blocks away, in the same drain.
    /// The decrease and the increase are then in flight together, which is
    /// the case `propagateDecrease`'s `else` arm exists for: a neighbour
    /// brighter than the removal accounts for is re-queued as an increase in
    /// exactly one direction.
    private static void moveTheSource(
        ProtoChunk[] grid, LevelLightEngine engine, LevelHeightAccessor heights,
        List<String> record
    ) {
        for (ProtoChunk c : grid) {
            int bx = c.getPos().getMinBlockX();
            int bz = c.getPos().getMinBlockZ();
            edit(c, engine, record,
                new BlockPos(bx + 8, 1, bz + 8), Blocks.AIR.defaultBlockState());
            edit(c, engine, record,
                new BlockPos(bx + 2, 1, bz + 8), Blocks.GLOWSTONE.defaultBlockState());
        }
    }

    private static String dumpCase(
        String name,
        LevelHeightAccessor heights,
        PalettedContainerFactory containers,
        Builder builder
    ) {
        return dumpCase(name, heights, containers, builder, null);
    }

    private static String dumpCase(
        String name,
        LevelHeightAccessor heights,
        PalettedContainerFactory containers,
        Builder builder,
        Mutator mutator
    ) {
        return dumpCase(name, heights, containers, builder, mutator, null);
    }

    /// `fromScratchControl` names another case in this file that builds the
    /// **same final world** without the mutation. A consumer that lights a
    /// world from scratch has no access to this one's edit history and cannot
    /// be expected to reproduce its answer; the control is what it should
    /// reproduce, and the difference between the two is the finding.
    private static String dumpCase(
        String name,
        LevelHeightAccessor heights,
        PalettedContainerFactory containers,
        Builder builder,
        Mutator mutator,
        String fromScratchControl
    ) {
        int span = 2 * RADIUS + 1;
        ProtoChunk[] grid = new ProtoChunk[span * span];
        for (int dz = -RADIUS; dz <= RADIUS; dz++) {
            for (int dx = -RADIUS; dx <= RADIUS; dx++) {
                ChunkPos pos = new ChunkPos(dx, dz);
                ProtoChunk c = new ProtoChunk(pos, UpgradeData.EMPTY, heights, containers, null);
                builder.fill(c, heights);
                // `ChunkAccess.initializeLightSources()` builds the per-column
                // sky-light source heightmap the sky engine reads. Vanilla
                // runs it at the chunk's LIGHT step; omitting it does not
                // error, it just lights a world with no sky sources — the
                // plausible-looking empty answer ORACLE.md 5b is about.
                c.initializeLightSources();
                grid[(dz + RADIUS) * span + (dx + RADIUS)] = c;
            }
        }

        LightChunkGetter getter = new GridLightChunkGetter(grid, RADIUS, heights);
        LevelLightEngine engine = new LevelLightEngine(getter, true, true);

        // initializeLight, then lightChunk, for every chunk — in that order
        // across the whole grid, because a chunk's light depends on its
        // neighbours' sections having been declared first.
        for (ProtoChunk c : grid) {
            ChunkPos pos = c.getPos();
            LevelChunkSection[] sections = c.getSections();
            for (int i = 0; i < c.getSectionsCount(); i++) {
                if (!sections[i].hasOnlyAir()) {
                    engine.updateSectionStatus(
                        SectionPos.of(pos, heights.getSectionYFromSectionIndex(i)), false);
                }
            }
            engine.setLightEnabled(pos, true);
            engine.retainData(pos, false);
        }
        for (ProtoChunk c : grid) {
            engine.propagateLightSources(c.getPos());
        }
        // Drain. `runLightUpdates` returns the number of positions it
        // processed; the server's executor keeps calling it while
        // `hasLightWork()`. A fixed iteration count here would silently
        // truncate a big propagation, so this loops on the engine's own
        // predicate and asserts it converged.
        int guard = 0;
        while (engine.hasLightWork()) {
            engine.runLightUpdates();
            if (++guard > 10_000) {
                throw new IllegalStateException("light did not converge for case " + name);
            }
        }

        int mutationDrain = 0;
        List<String> mutations = new ArrayList<>();
        if (mutator != null) {
            mutator.apply(grid, engine, heights, mutations);
            while (engine.hasLightWork()) {
                engine.runLightUpdates();
                if (++mutationDrain > 10_000) {
                    throw new IllegalStateException("mutation light did not converge: " + name);
                }
            }
        }

        StringBuilder sb = new StringBuilder();
        sb.append("    {\n");
        sb.append("      \"name\": \"").append(name).append("\",\n");
        sb.append("      \"drain_iterations\": ").append(guard).append(",\n");
        sb.append("      \"mutation_drain_iterations\": ").append(mutationDrain).append(",\n");
        sb.append("      \"mutations\": [")
            .append(String.join(", ", mutations))
            .append("],\n");
        sb.append("      \"from_scratch_control\": ")
            .append(fromScratchControl == null ? "null" : "\"" + fromScratchControl + "\"")
            .append(",\n");
        sb.append("      \"chunks\": [\n");
        List<String> chunks = new ArrayList<>();
        for (int dz = -RADIUS; dz <= RADIUS; dz++) {
            for (int dx = -RADIUS; dx <= RADIUS; dx++) {
                chunks.add(dumpChunk(
                    engine, grid[(dz + RADIUS) * span + (dx + RADIUS)], heights));
            }
        }
        for (int i = 0; i < chunks.size(); i++) {
            sb.append(chunks.get(i)).append(i + 1 < chunks.size() ? ",\n" : "\n");
        }
        sb.append("      ]\n");
        sb.append("    }");
        return sb.toString();
    }

    /// The packet's own light section, built by vanilla's own constructor.
    ///
    /// `null, null` for the two changed-section masks is what
    /// `ClientboundLevelChunkWithLightPacket` passes for a whole-chunk send,
    /// so this is the artefact a joining client actually receives.
    /// The chunk's blocks, as a palette and a run-length encoding over the
    /// 4096 positions of each non-air section in `y, z, x` order — the same
    /// order `DataLayer.getIndex` uses, so a consumer walks one index for
    /// both. Emitted because a comparison needs the **input** as well as the
    /// expected output: without it the Rust side would have to carry a second
    /// copy of these world builders, and two copies of a generation path that
    /// drift apart is the oracle fault ORACLE.md 5b is about.
    /// The same serialisation the world palette uses, so a mutation's
    /// `from`/`to` and a palette entry are the same string and index the same
    /// `block_props.json` row.
    private static String stateKey(BlockState st) {
        return net.minecraft.commands.arguments.blocks.BlockStateParser.serialize(st);
    }

    private static String dumpWorld(ProtoChunk chunk, LevelHeightAccessor heights) {
        StringBuilder sb = new StringBuilder("[");
        LevelChunkSection[] sections = chunk.getSections();
        boolean first = true;
        for (int i = 0; i < chunk.getSectionsCount(); i++) {
            if (sections[i].hasOnlyAir()) {
                continue;
            }
            if (!first) sb.append(", ");
            first = false;
            int sectionY = heights.getSectionYFromSectionIndex(i);
            List<String> palette = new ArrayList<>();
            Map<BlockState, Integer> seen = new LinkedHashMap<>();
            List<int[]> runs = new ArrayList<>();
            int runIndex = -1;
            int runLength = 0;
            for (int y = 0; y < 16; y++) {
                for (int z = 0; z < 16; z++) {
                    for (int x = 0; x < 16; x++) {
                        BlockState st = sections[i].getBlockState(x, y, z);
                        Integer idx = seen.get(st);
                        if (idx == null) {
                            idx = palette.size();
                            seen.put(st, idx);
                            palette.add(
                                net.minecraft.commands.arguments.blocks.BlockStateParser
                                    .serialize(st));
                        }
                        if (idx == runIndex) {
                            runLength++;
                        } else {
                            if (runIndex >= 0) runs.add(new int[] {runIndex, runLength});
                            runIndex = idx;
                            runLength = 1;
                        }
                    }
                }
            }
            if (runIndex >= 0) runs.add(new int[] {runIndex, runLength});
            sb.append("{\"section_y\": ").append(sectionY).append(", \"palette\": [");
            for (int k = 0; k < palette.size(); k++) {
                if (k > 0) sb.append(", ");
                sb.append('"').append(palette.get(k)).append('"');
            }
            sb.append("], \"runs\": [");
            for (int k = 0; k < runs.size(); k++) {
                if (k > 0) sb.append(", ");
                sb.append("[").append(runs.get(k)[0]).append(", ").append(runs.get(k)[1]).append("]");
            }
            sb.append("]}");
        }
        return sb.append("]").toString();
    }

    private static String dumpChunk(
        LevelLightEngine engine, ProtoChunk chunk, LevelHeightAccessor heights
    ) {
        ChunkPos pos = chunk.getPos();
        ClientboundLightUpdatePacketData data =
            new ClientboundLightUpdatePacketData(pos, engine, null, null);
        StringBuilder sb = new StringBuilder();
        sb.append("        {\n");
        sb.append("          \"x\": ").append(pos.x()).append(", \"z\": ").append(pos.z()).append(",\n");
        sb.append("          \"sky_mask\": ").append(mask(data.getSkyYMask())).append(",\n");
        sb.append("          \"block_mask\": ").append(mask(data.getBlockYMask())).append(",\n");
        sb.append("          \"empty_sky_mask\": ").append(mask(data.getEmptySkyYMask())).append(",\n");
        sb.append("          \"empty_block_mask\": ").append(mask(data.getEmptyBlockYMask())).append(",\n");
        sb.append("          \"sky\": ").append(arrays(data.getSkyUpdates())).append(",\n");
        sb.append("          \"block\": ").append(arrays(data.getBlockUpdates())).append(",\n");
        sb.append("          \"world\": ").append(dumpWorld(chunk, heights)).append("\n");
        sb.append("        }");
        return sb.toString();
    }

    /// `BitSet.toLongArray()`, which is exactly what `writeBitSet` puts on the
    /// wire — trailing zero words trimmed, so an all-clear mask is a
    /// zero-length array and not a word of zeroes. Emitted as decimal strings
    /// because a JSON number cannot hold the range, the same rule
    /// `GoldenDump` uses for i64.
    private static String mask(BitSet bits) {
        long[] words = bits.toLongArray();
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < words.length; i++) {
            if (i > 0) sb.append(", ");
            sb.append('"').append(Long.toString(words[i])).append('"');
        }
        return sb.append("]").toString();
    }

    /// The 2048 bytes of each `DataLayer`, as lowercase hex.
    ///
    /// Hex rather than base64 on purpose: the consumer is a Rust test in a
    /// workspace with no base64 crate, and the choice was between adding a
    /// dependency, hand-rolling a decoder in the one place whose whole job is
    /// to be trusted, or paying 1.3x on a golden file. The file is the
    /// cheapest of the three to be wrong about.
    private static String arrays(List<byte[]> list) {
        StringBuilder sb = new StringBuilder("[");
        for (int i = 0; i < list.size(); i++) {
            if (i > 0) sb.append(", ");
            sb.append('"');
            for (byte b : list.get(i)) {
                sb.append(Character.forDigit((b >> 4) & 15, 16));
                sb.append(Character.forDigit(b & 15, 16));
            }
            sb.append('"');
        }
        return sb.append("]").toString();
    }

    /// The grid, as the engine sees it. Chunks outside the grid are `null`,
    /// which is what a real server hands back for an unloaded chunk and is a
    /// state the engine has explicit handling for.
    private record GridLightChunkGetter(
        ProtoChunk[] grid, int radius, LevelHeightAccessor heights
    ) implements LightChunkGetter {

        @Override
        public LightChunk getChunkForLighting(int x, int z) {
            if (x < -radius || x > radius || z < -radius || z > radius) {
                return null;
            }
            int span = 2 * radius + 1;
            return grid[(z + radius) * span + (x + radius)];
        }

        @Override
        public net.minecraft.world.level.BlockGetter getLevel() {
            // Established from the bytecode: the only call site is
            // `SkyLightEngine`'s constructor, which uses it purely as a
            // `LevelHeightAccessor`. Anything else throws by construction.
            return (net.minecraft.world.level.BlockGetter) Proxy.newProxyInstance(
                LightGolden.class.getClassLoader(),
                new Class<?>[] {net.minecraft.world.level.BlockGetter.class},
                new HeightsOnly(heights));
        }
    }

    private record HeightsOnly(LevelHeightAccessor heights) implements InvocationHandler {
        @Override
        public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
            try {
                Method on = LevelHeightAccessor.class.getMethod(
                    method.getName(), method.getParameterTypes());
                return on.invoke(heights, args);
            } catch (NoSuchMethodException e) {
                throw new UnsupportedOperationException(
                    "the light engine asked getLevel() for " + method
                        + ", which this harness was not established to answer");
            }
        }
    }
}
