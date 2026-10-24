// GenLevel — a `WorldGenLevel` over generated chunks, and nothing else.
//
// A feature or a placement modifier reaches the world through a
// `WorldGenLevel`, which on a real server is a `WorldGenRegion` wrapping a
// `ServerLevel`. Neither is constructible outside a running server, so a
// harness has to stand in for one.
//
// # It throws rather than answers
//
// This is a `java.lang.reflect.Proxy` implementing the handful of methods the
// subject actually reaches — established by `javap -c`, not by guessing —
// over a real block of `ProtoChunk`s, and **throwing for everything else,
// inherited default methods included**.
//
// That is the whole safety argument, and it is bought with experience: four
// times this project has chased a parity failure to an oracle that
// reconstructed something the server provides and returned a plausible-looking
// *empty* answer rather than an error (ORACLE.md 5b). A hand-written stub
// returning `Blocks.AIR` from a method nobody foresaw would place nothing and
// report no problem. A proxy that throws turns an incomplete analysis into a
// crash naming the method it did not expect.
//
// Nothing here is reimplemented either. `getMinY`, `getHeight`,
// `getSectionIndex`, `getSectionsCount` and `getMinSectionY` forward to the
// same `LevelHeightAccessor` the chunks were built with;
// `getHeight(Types, x, z)` copies `WorldGenRegion.getHeight` including its
// `+ 1`; `ensureCanWrite` copies `WorldGenRegion.isWithinWriteZone`, a
// chessboard distance in chunks against the generating step's own write
// radius; and `getBiome` goes through a real `BiomeManager` so the fuzz is
// vanilla's.
//
// # The one identity rule
//
// The `BiomeManager` passed in must be built from the **same HolderLookup**
// as everything the caller will compare biomes against. Vanilla compares
// biome holders by object identity, and a holder minted by a second registry
// can never equal one baked into a rule or a biome's feature list — the
// failure is silent and looks like "the feature did not apply here". That is
// ORACLE.md 5b rule 1, and it cost this project a retracted parity number
// once already.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;

import net.minecraft.core.BlockPos;
import net.minecraft.core.SectionPos;
import net.minecraft.world.level.LevelHeightAccessor;
import net.minecraft.world.level.WorldGenLevel;
import net.minecraft.world.level.biome.BiomeManager;
import net.minecraft.world.level.chunk.ChunkAccess;
import net.minecraft.world.level.chunk.ProtoChunk;

final class GenLevel implements InvocationHandler {

    private final ProtoChunk[] grid;
    private final int centerX;
    private final int centerZ;
    /// Chunks either side of the centre, so a `radius` of 1 is a 3x3.
    private final int radius;
    private final LevelHeightAccessor heights;
    private final int writeRadius;
    private final long seed;
    private final int seaLevel;
    /// `WorldGenRegion.random` — and it is **not** an ambient RNG.
    ///
    /// A level-wide `getRandom()` would make worldgen non-reproducible, and
    /// it is not one: `WorldGenRegion`'s constructor builds it as
    /// `randomState.getOrCreateRandomFactory(WORLDGEN_REGION_RANDOM)
    /// .at(centreChunk.getPos().getWorldPosition())` — positional, derived
    /// from the world seed, fixed per chunk. `StructureTemplate` reaches for
    /// it while updating block shapes at a structure's edge, so a harness
    /// that handed back a fresh unseeded random would produce a world that
    /// changed between runs and blame the port.
    private final net.minecraft.util.RandomSource random;
    /// May be null. A caller whose subject never asks for a biome passes null
    /// and gets the usual throw if it turns out to ask after all.
    private final BiomeManager biomes;
    /// `WorldGenLevel.registryAccess()`. May be null, same contract as
    /// [`biomes`]: a caller whose subject never asks passes null and gets the
    /// usual throw if it turns out to ask.
    ///
    /// Exactly one thing asks, and it is worth naming because the analysis
    /// this harness rests on said nothing did: `PaleMossDecorator` resolves
    /// `VegetationFeatures.PALE_MOSS_PATCH` off the level at **place** time.
    /// Until that decorator could run — it needs both pale oaks placeable —
    /// no dump ever reached this method, so the javap analysis was right when
    /// it was made and stopped being right when the port moved.
    private final net.minecraft.core.RegistryAccess registries;

    int outsideBuildHeightCalls;
    int writeDenied;
    int biomeLookups;
    /// Ticks the subject asked to schedule. Recorded rather than performed —
    /// see the handler.
    int scheduledTicks;
    /// Light lookups. See the handler for why zero is the right answer.
    int brightnessQueries;
    /// Every `getBiome` the subject asked, as "x,y,z" -> biome name.
    ///
    /// Shipped with the vectors so the Rust side can answer the same
    /// question exactly rather than approximate it, and — more usefully —
    /// so it can assert that every lookup *it* makes is one vanilla also
    /// made. A port that asks about a different position then fails loudly
    /// instead of quietly agreeing.
    final java.util.LinkedHashMap<String, String> biomeAnswers = new java.util.LinkedHashMap<>();
    /// Every `getBlockState` the subject asked, **in order**, as
    /// "x,y,z=state", while [`recordReads`] is true.
    ///
    /// A list rather than a map, because the order is the whole point. A
    /// feature that walks a region and returns on the first failure leaves a
    /// read log whose *last entry is the cell it stopped on* — which is the
    /// only way to ask vanilla "which block made you give up" without
    /// modifying vanilla.
    ///
    /// `biomeAnswers` has done the same job for biomes since the biome-holder
    /// bug; blocks had no equivalent, and a whole session went into inferring
    /// from block *outcomes* what one read log states directly.
    final java.util.List<String> blockReads = new java.util.ArrayList<>();
    /// Armed by `DecorateGolden` only while a feature matching
    /// `-Ddecorate.reads=<substring>` is running. Off by default: a chunk's
    /// decoration makes millions of reads.
    boolean recordReads;

    GenLevel(
        ProtoChunk[] grid, int centerX, int centerZ, int radius,
        LevelHeightAccessor heights, int writeRadius, long seed, int seaLevel,
        BiomeManager biomes, net.minecraft.util.RandomSource random,
        net.minecraft.core.RegistryAccess registries
    ) {
        int side = radius * 2 + 1;
        if (grid.length != side * side) {
            throw new IllegalArgumentException(
                "a radius of " + radius + " wants " + (side * side) + " chunks, got " + grid.length);
        }
        this.grid = grid;
        this.centerX = centerX;
        this.centerZ = centerZ;
        this.radius = radius;
        this.heights = heights;
        this.writeRadius = writeRadius;
        this.seed = seed;
        this.seaLevel = seaLevel;
        this.biomes = biomes;
        this.random = random;
        this.registries = registries;
    }

    WorldGenLevel proxy() {
        return (WorldGenLevel) Proxy.newProxyInstance(
            GenLevel.class.getClassLoader(), new Class<?>[]{WorldGenLevel.class}, this);
    }

    ProtoChunk[] chunks() {
        return grid;
    }

    private ChunkAccess chunk(int chunkX, int chunkZ) {
        int dx = chunkX - centerX;
        int dz = chunkZ - centerZ;
        if (Math.abs(dx) > radius || Math.abs(dz) > radius) {
            throw new IllegalStateException(
                "reached chunk " + chunkX + ", " + chunkZ + ", outside the "
                + (radius * 2 + 1) + "x" + (radius * 2 + 1) + " world around "
                + centerX + ", " + centerZ);
        }
        return grid[(dz + radius) * (radius * 2 + 1) + dx + radius];
    }

    /// `WorldGenRegion.getHeight`, including the `+ 1`.
    private int heightAt(Heightmap.Types type, int x, int z) {
        return chunk(SectionPos.blockToSectionCoord(x), SectionPos.blockToSectionCoord(z))
            .getHeight(type, x & 15, z & 15) + 1;
    }

    @Override
    public Object invoke(Object proxy, Method method, Object[] args) {
        String name = method.getName();
        int arity = method.getParameterCount();

        if (arity == 2 && name.equals("getChunk")) {
            return chunk((Integer) args[0], (Integer) args[1]);
        }
        if (arity == 3 && name.equals("getHeight")) {
            return heightAt((Heightmap.Types) args[0], (Integer) args[1], (Integer) args[2]);
        }
        if (arity == 2 && name.equals("getHeight")) {
            // `LevelReader.getHeight(Types, BlockPos)` is a default that
            // forwards to the three-argument form with `pos.getX()` and
            // `pos.getZ()`. Read from the bytecode rather than assumed, and
            // routed through the same helper as the three-argument case —
            // two handlers answering the same question independently is how
            // they drift.
            //
            // Reached by `RootSystemFeature.placeDirtAndTree`, which is the
            // feature this arity was added for.
            BlockPos pos = (BlockPos) args[1];
            return heightAt((Heightmap.Types) args[0], pos.getX(), pos.getZ());
        }
        if (arity == 0 && name.equals("getHeight")) {
            return heights.getHeight();
        }
        if (arity == 0 && name.equals("getMinY")) {
            return heights.getMinY();
        }
        if (arity == 0 && name.equals("getMaxY")) {
            return heights.getMaxY();
        }
        if (arity == 0 && name.equals("getMinSectionY")) {
            return heights.getMinSectionY();
        }
        if (arity == 0 && name.equals("getSectionsCount")) {
            return heights.getSectionsCount();
        }
        if (arity == 1 && name.equals("getSectionIndex")) {
            return heights.getSectionIndex((Integer) args[0]);
        }
        if (arity == 3 && (name.equals("scheduleTick") || name.equals("getBlockTicks")
                || name.equals("getFluidTicks"))) {
            // A generating chunk collects scheduled ticks into the ProtoChunk
            // rather than running them; nothing this gate compares reads them,
            // and the block writes that matter have already happened by the
            // time one is scheduled. Dropped, and *counted*, so "the gate
            // ignored 400 scheduled ticks" is a number rather than a silence.
            scheduledTicks++;
            return null;
        }
        if (arity == 2 && name.equals("getBrightness")) {
            // Zero, and that is vanilla's answer too rather than a stand-in.
            //
            // `ChunkStatusTasks.initializeLight` runs **after**
            // `generateFeatures` in the generation pyramid, so no light has
            // been computed for these chunks when features run: a real
            // `WorldGenRegion` asks a light engine that has nothing yet and
            // gets 0 as well. `Biome.shouldFreeze` tests `< 10`, so this is
            // the branch a real server takes, not the convenient one.
            //
            // Counted, because "the harness answered a question it invented"
            // should be a number if it ever stops being true.
            brightnessQueries++;
            return 0;
        }
        if (arity == 0 && name.equals("getSeaLevel")) {
            // `Biome.shouldFreeze` reads it, which `freeze_top_layer` reaches
            // in the last decoration step.
            return seaLevel;
        }
        if (arity == 0 && name.equals("getRandom")) {
            return random;
        }
        if (arity == 0 && name.equals("getSeed")) {
            return seed;
        }
        if (arity == 1 && name.equals("isInsideBuildHeight")
                && method.getParameterTypes()[0] == int.class) {
            return heights.isInsideBuildHeight((Integer) args[0]);
        }
        if (arity == 1 && name.equals("isInsideBuildHeight")
                && method.getParameterTypes()[0] == BlockPos.class) {
            // `LevelHeightAccessor.isInsideBuildHeight(BlockPos)`'s own
            // default, per `javap -c`: `isInsideBuildHeight(pos.getY())`.
            // `InsideWorldBoundsPredicate` reaches it, so any chunk whose
            // decoration runs an `environment_scan` with the default
            // `allOf(insideWorldBounds, ...)` predicate needs it — chunk
            // (58, 58) at seed 20260722 is one.
            return heights.isInsideBuildHeight(((BlockPos) args[0]).getY());
        }
        if (arity == 1 && name.equals("isOutsideBuildHeight")
                && method.getParameterTypes()[0] == int.class) {
            boolean outside = heights.isOutsideBuildHeight((Integer) args[0]);
            if (outside) {
                outsideBuildHeightCalls++;
            }
            return outside;
        }
        if (arity == 1 && name.equals("ensureCanWrite")) {
            BlockPos pos = (BlockPos) args[0];
            boolean can =
                Math.abs(SectionPos.blockToSectionCoord(pos.getX()) - centerX) <= writeRadius
                && Math.abs(SectionPos.blockToSectionCoord(pos.getZ()) - centerZ) <= writeRadius;
            if (!can) {
                writeDenied++;
            }
            return can;
        }
        if (arity == 1 && name.equals("getChunk")
                && method.getParameterTypes()[0] == BlockPos.class) {
            // `LevelReader.getChunk(BlockPos)`'s own default.
            BlockPos pos = (BlockPos) args[0];
            return chunk(SectionPos.blockToSectionCoord(pos.getX()),
                         SectionPos.blockToSectionCoord(pos.getZ()));
        }
        if (arity == 1 && name.equals("getBlockState")) {
            // `WorldGenRegion.getBlockState`.
            BlockPos pos = (BlockPos) args[0];
            net.minecraft.world.level.block.state.BlockState st =
                chunk(SectionPos.blockToSectionCoord(pos.getX()),
                      SectionPos.blockToSectionCoord(pos.getZ())).getBlockState(pos);
            if (recordReads) {
                blockReads.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "="
                    + net.minecraft.commands.arguments.blocks.BlockStateParser.serialize(st));
            }
            return st;
        }
        if (arity == 1 && name.equals("getFluidState")) {
            BlockPos pos = (BlockPos) args[0];
            return chunk(SectionPos.blockToSectionCoord(pos.getX()),
                         SectionPos.blockToSectionCoord(pos.getZ())).getFluidState(pos);
        }
        if (arity == 1 && name.equals("isWaterAt")) {
            // `LevelReader.isWaterAt`'s own default, read from the bytecode:
            // `getFluidState(pos).is(FluidTags.WATER)`. Note it is the *fluid
            // tag*, not `is(Blocks.WATER)`, so a waterlogged block counts and
            // flowing water counts.
            //
            // Reached from `SpeleothemUtils.growSpeleothem`, which is how the
            // sulfur spikes decide whether to grow — so without this the
            // harness cannot run any chunk containing a sulfur cave.
            BlockPos pos = (BlockPos) args[0];
            return chunk(SectionPos.blockToSectionCoord(pos.getX()),
                         SectionPos.blockToSectionCoord(pos.getZ()))
                .getFluidState(pos)
                .is(net.minecraft.tags.FluidTags.WATER);
        }
        if (arity == 1 && name.equals("isEmptyBlock")) {
            // `BlockGetter.isEmptyBlock`'s own default: `getBlockState(pos).isAir()`.
            BlockPos pos = (BlockPos) args[0];
            return chunk(SectionPos.blockToSectionCoord(pos.getX()),
                         SectionPos.blockToSectionCoord(pos.getZ()))
                .getBlockState(pos)
                .isAir();
        }
        if (arity == 2 && name.equals("getHeightmapPos")) {
            // `LevelReader.getHeightmapPos`'s own default:
            // `pos.atY(getHeight(type, x, z))`, which is the same `+ 1`
            // WorldGenRegion applies above.
            BlockPos pos = (BlockPos) args[1];
            int height = chunk(SectionPos.blockToSectionCoord(pos.getX()),
                               SectionPos.blockToSectionCoord(pos.getZ()))
                .getHeight((Heightmap.Types) args[0], pos.getX() & 15, pos.getZ() & 15) + 1;
            return pos.atY(height);
        }
        if (arity == 2 && name.equals("isFluidAtPosition")) {
            // `WorldGenRegion.isFluidAtPosition`: the predicate against the
            // position's *fluid* state, which a waterlogged block has.
            BlockPos pos = (BlockPos) args[0];
            @SuppressWarnings("unchecked")
            java.util.function.Predicate<net.minecraft.world.level.material.FluidState> test =
                (java.util.function.Predicate<net.minecraft.world.level.material.FluidState>) args[1];
            return test.test(chunk(SectionPos.blockToSectionCoord(pos.getX()),
                                   SectionPos.blockToSectionCoord(pos.getZ()))
                .getFluidState(pos));
        }
        if (arity == 2 && name.equals("isStateAtPosition")) {
            // `LevelReader.isStateAtPosition`'s own default, spelled out
            // because the proxy intercepts defaults too.
            BlockPos pos = (BlockPos) args[0];
            @SuppressWarnings("unchecked")
            java.util.function.Predicate<net.minecraft.world.level.block.state.BlockState> test =
                (java.util.function.Predicate<net.minecraft.world.level.block.state.BlockState>) args[1];
            net.minecraft.world.level.block.state.BlockState st =
                chunk(SectionPos.blockToSectionCoord(pos.getX()),
                      SectionPos.blockToSectionCoord(pos.getZ())).getBlockState(pos);
            // Recorded too, and tagged `~` rather than `=`.
            //
            // `Column.scan` — which is how every speleothem column finds its
            // ceiling and floor — reads the world through *this*, not through
            // `getBlockState`, so a read log without it shows 721 reads where
            // the feature made thousands and shows no scan at all. Found by
            // pointing `-Ddecorate.reads` at `sulfur_spike_cluster` and
            // getting 97 column runs for a feature that visits far more.
            if (recordReads) {
                blockReads.add(pos.getX() + "," + pos.getY() + "," + pos.getZ() + "~"
                    + net.minecraft.commands.arguments.blocks.BlockStateParser.serialize(st));
            }
            return test.test(st);
        }
        if (arity == 3 && name.equals("setBlock")) {
            // `WorldGenRegion.setBlock`, minus the parts a generating chunk
            // has no use for: no neighbour updates, no tick scheduling.
            // `ProtoChunk.setBlockState` is what WorldGenRegion itself calls
            // into.
            //
            // The block-entity bookkeeping IS kept, because it is not
            // cosmetic: it decides how many values a feature draws. See
            // `getBlockEntity` below.
            BlockPos pos = (BlockPos) args[0];
            net.minecraft.world.level.block.state.BlockState state =
                (net.minecraft.world.level.block.state.BlockState) args[1];
            ChunkAccess target = chunk(SectionPos.blockToSectionCoord(pos.getX()),
                                      SectionPos.blockToSectionCoord(pos.getZ()));
            net.minecraft.world.level.block.state.BlockState old =
                target.setBlockState(pos, state);
            if (state.hasBlockEntity()) {
                // A generating chunk is a ProtoChunk, so WorldGenRegion takes
                // its non-LEVELCHUNK branch: it does not construct the block
                // entity here, it records a placeholder tag and builds the
                // real one on the first read.
                net.minecraft.nbt.CompoundTag tag = new net.minecraft.nbt.CompoundTag();
                tag.putInt("x", pos.getX());
                tag.putInt("y", pos.getY());
                tag.putInt("z", pos.getZ());
                tag.putString("id", "DUMMY");
                target.setBlockEntityNbt(tag);
            } else if (old != null && old.hasBlockEntity()) {
                target.removeBlockEntity(pos);
            }
            return true;
        }
        if (arity == 1 && name.equals("getBiome")) {
            if (biomes == null) {
                throw new UnsupportedOperationException(
                    "getBiome was reached, but this GenLevel was built without a BiomeManager. "
                    + "Pass one built from the same HolderLookup as everything the biome will "
                    + "be compared against — see this file's header on holder identity.");
            }
            biomeLookups++;
            BlockPos pos = (BlockPos) args[0];
            net.minecraft.core.Holder<net.minecraft.world.level.biome.Biome> holder =
                biomes.getBiome(pos);
            biomeAnswers.put(
                pos.getX() + "," + pos.getY() + "," + pos.getZ(),
                holder.unwrapKey().map(k -> k.identifier().toString()).orElse("?"));
            return holder;
        }
        // `WorldGenRegion.getBlockEntity(BlockPos)`.
        //
        // **This returned null unconditionally, and that made the harness lie
        // about draw counts.** The old comment argued it was harmless — "the
        // features that reach here place the same *blocks* either way; only
        // their loot is lost". That is false, and it cost a day on the
        // dungeon chest:
        //
        //   `MonsterRoomFeature` places a chest and then calls
        //   `RandomizableContainer.setBlockEntityLootTable`, whose whole body
        //   is `if (getBlockEntity(pos) instanceof RandomizableContainer c)
        //   c.setLootTable(table, random.nextLong())`. With null the
        //   `nextLong` is never evaluated — Java does not evaluate an
        //   argument to a call it does not make — so the harness's stream
        //   runs one value short **per chest**, and the feature's own second
        //   chest then lands somewhere a real server never puts it. Same for
        //   the spawner's `randomEntityId`, which is a `nextInt(4)` behind
        //   `instanceof SpawnerBlockEntity`.
        //
        // So the loot is not the only thing lost; the *stream position* is,
        // and everything the feature draws afterwards moves with it. A block
        // oracle that gets the draws wrong is not a block oracle.
        //
        // Ported from `WorldGenRegion.getBlockEntity`, which for a ProtoChunk
        // rebuilds the entity from the placeholder tag `setBlock` left:
        // real entity if one is already cached, else the "DUMMY" tag means
        // "freshly placed, construct the default", and the result is cached
        // so a second read is the same object.
        //
        // The non-DUMMY branch — `BlockEntity.loadStatic` for a tag carrying
        // real saved data — cannot arise here: nothing in this harness writes
        // a tag except the `setBlock` above, and it only ever writes DUMMY.
        // It throws rather than returning null, because a silent null is what
        // this whole comment is about.
        if (arity == 1 && name.equals("getBlockEntity")) {
            BlockPos pos = (BlockPos) args[0];
            ChunkAccess target = chunk(SectionPos.blockToSectionCoord(pos.getX()),
                                      SectionPos.blockToSectionCoord(pos.getZ()));
            net.minecraft.world.level.block.entity.BlockEntity existing = target.getBlockEntity(pos);
            if (existing != null) {
                return existing;
            }
            net.minecraft.nbt.CompoundTag tag = target.getBlockEntityNbt(pos);
            if (tag == null) {
                return null;
            }
            if (!"DUMMY".equals(tag.getStringOr("id", ""))) {
                throw new IllegalStateException(
                    "a block entity tag that this harness did not write, at " + pos + ": " + tag);
            }
            net.minecraft.world.level.block.state.BlockState state = target.getBlockState(pos);
            if (!state.hasBlockEntity()) {
                return null;
            }
            net.minecraft.world.level.block.entity.BlockEntity created =
                ((net.minecraft.world.level.block.EntityBlock) state.getBlock())
                    .newBlockEntity(pos, state);
            if (created == null) {
                return null;
            }
            target.setBlockEntity(created);
            return created;
        }
        // `WorldGenRegion.playSound(Entity, BlockPos, SoundEvent, SoundSource,
        // float, float)` — its whole body is `0: return`, checked with
        // `javap -p -c -constants` on the pinned jar. So doing nothing here is
        // not a stub, it is the reference implementation.
        //
        // The subject that reaches it is `SculkSpreader$ChargeCursor.update`,
        // which a `sculk_patch_deep_dark` runs on every cursor. Before this
        // arm the harness threw, so **DecorateGolden could not finish any
        // chunk containing a deep dark** — it aborted mid-run, having written
        // the draws of everything before the sculk patch and nothing after.
        // A truncated run is not an obviously broken one: the JSON is absent
        // but the `-Ddecorate.draws` trace on stderr looks complete, and the
        // chunks it never reached simply do not appear.
        //
        // `ChargeCursor.update` makes exactly two `LevelAccessor` calls,
        // `getBlockState` and this one, so nothing else on that path is
        // missing.
        if (arity == 6 && name.equals("playSound")) {
            return null;
        }
        // `WorldGenRegion.levelEvent(Entity, int, BlockPos, int)` is `0: return`
        // and `LevelAccessor`'s three-argument default delegates straight to
        // it, so both arities are no-ops on a generating region. Checked with
        // `javap -p -c -constants`, same as `playSound` above. A dynamic proxy
        // intercepts a `default` interface method as well as an abstract one,
        // which is why arity 3 arrives here at all.
        //
        // `SculkSpreader.updateCursors` fires it when a charge finishes, so
        // this is the SECOND no-op on the sculk path and it aborted the oracle
        // the same way the first did. Enumerated rather than found one at a
        // time: `SculkSpreader`, `ChargeCursor`, `SculkBlock` and
        // `SculkVeinBlock` together make exactly five distinct `LevelAccessor`
        // calls — `getBlockState`, `getFluidState`, `setBlock`, `playSound`,
        // `levelEvent` — and the first three were already handled, so there is
        // no third gap waiting on this path.
        if ((arity == 3 || arity == 4) && name.equals("levelEvent")) {
            return null;
        }
        // `PaleMossDecorator` is the only caller, and it does
        //
        //   registryAccess().lookup(Registries.CONFIGURED_FEATURE)
        //       .flatMap(r -> r.get(PALE_MOSS_PATCH))
        //       .ifPresent(h -> h.value().place(..))
        //
        // Note the `Optional`: to vanilla an absent registry is not an error,
        // it is a patch that never places. So a null here must **throw** and
        // must not be handed back as an empty access — the empty answer is
        // the ORACLE.md 5b failure this whole class exists to refuse, and on
        // this path it would cost the patch's draws rather than crash.
        if (arity == 0 && name.equals("registryAccess")) {
            if (registries == null) {
                throw new UnsupportedOperationException(
                    "the subject reached WorldGenLevel.registryAccess/0 and this "
                    + "GenLevel was built without one. Pass the RegistryAccess "
                    + "rather than letting the lookup come back empty: an absent "
                    + "registry places nothing and reports nothing.");
            }
            return registries;
        }
        // `WorldGenLevel.getLevel()` returns a **ServerLevel**, and this is the
        // one refusal in this class that is not an incomplete analysis. It is
        // a concrete class whose only constructor takes a `MinecraftServer`,
        // so no proxy can stand in for it and no amount of extending this
        // handler will reach it.
        //
        // `PaleMossDecorator` is the caller, and it wants exactly one thing:
        //
        //   getLevel().getChunkSource().getGenerator()
        //
        // — the `ChunkGenerator` argument to `ConfiguredFeature.place`. The
        // harness *has* that generator. What it does not have is a legitimate
        // way to hand it over, because the route runs through a server object
        // this project has decided not to fabricate (ORACLE.md 5b).
        //
        // The consequence is worth stating plainly rather than leaving as a
        // stack trace: **this harness cannot dump a chunk in which a pale oak
        // places its moss patch.** The guard upstream is
        // `random.nextFloat() < groundProbability`, so a pale garden chunk
        // dumps only until the first tree whose roll fires. See
        // `docs/PARITY-ROADMAP.md` §89.9.
        if (arity == 0 && name.equals("getLevel")) {
            throw new UnsupportedOperationException(
                "the subject reached WorldGenLevel.getLevel/0, which returns a "
                + "ServerLevel. That is a concrete class needing a MinecraftServer, "
                + "so it cannot be proxied or fabricated. PaleMossDecorator wants "
                + "getChunkSource().getGenerator() from it. See PARITY-ROADMAP 89.9.");
        }
        if (arity == 0 && name.equals("toString")) {
            return "GenLevel(" + centerX + ", " + centerZ + ", r" + radius + ")";
        }
        if (arity == 0 && name.equals("hashCode")) {
            return System.identityHashCode(proxy);
        }
        if (arity == 1 && name.equals("equals")) {
            return proxy == args[0];
        }

        throw new UnsupportedOperationException(
            "the subject reached WorldGenLevel." + name + "/" + arity
            + ", which the javap analysis behind this harness says it does not. "
            + "Extend that analysis before extending this handler.");
    }
}
