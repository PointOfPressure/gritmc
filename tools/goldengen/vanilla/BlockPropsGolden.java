// BlockPropsGolden — the per-block-state facts worldgen asks about.
//
// Worldgen keeps asking questions that are *code*, not data. `disk_grass`'s
// state provider asks whether the block above is `solid`. `underwater_magma`
// asks for a face's occlusion shape. `matching_fluids` asks for a fluid
// state, which a waterlogged block has and its name does not mention. None of
// those are in `blocks.json`, and all of them decide where blocks go.
//
// # Why this is extraction and not reimplementation
//
// The alternative is porting `VoxelShape`, `Shapes.joinIsNotEmpty`, and every
// block class's shape declaration — thousands of lines of geometry to answer
// questions whose answers are a handful of booleans per state. Vanilla
// computes them once at bootstrap and caches them in
// `BlockBehaviour.BlockStateBase.cache`; this dumps that cache.
//
// The result is Mojang-derived data and lives with the rest of it under the
// same rule: gitignored, regenerable from the jar in one command, never
// committed.
//
// # What is dumped, and why each one
//
//   state            `BlockStateParser.serialize` — the key. Keyed by state
//                    and not by block name because the two are not the same
//                    question: `minecraft:water` is sixteen states whose
//                    fluid differs by level, and a table keyed by name has no
//                    answer for it.
//   air              `BlockState.isAir` — the single most-asked question in
//                    the whole generator.
//   fluid            the block's own `FluidState`, as a registry name, empty
//                    when there is none. **This is what `matching_fluids`
//                    tests**, and it is not the block name: `seagrass` is not
//                    water and its fluid state is.
//   solidRender      `isSolidRender`, which is what `BlockPredicate.solid`
//                    resolves to.
//   blocksMotion     `BlockState.blocksMotion`, and `leaves` for
//                    `instanceof LeavesBlock`. Together with `air` and
//                    `fluid` these are the four heightmap predicates:
//
//                      WORLD_SURFACE[_WG]  !isAir
//                      OCEAN_FLOOR[_WG]    blocksMotion
//                      MOTION_BLOCKING     blocksMotion || fluid
//                      MOTION_BLOCKING_NO_LEAVES  that, and not a LeavesBlock
//
//                    `_WG` and non-`_WG` share a predicate and differ only in
//                    when they are primed — which is the whole of the
//                    OCEAN_FLOOR_WG fossil story in ORACLE.md 5b.
//   centerFaces      six bits, `isFaceSturdy(dir, SupportType.CENTER)`.
//                    NOT derivable from sturdyFaces: a fence post covers the
//                    centre of its down face and none of the rest of it.
//   unstableBottomCenter
//                    in `#minecraft:unstable_bottom_center`, which
//                    `Block.canSupportCenter` refuses for DOWN before it
//                    looks at any shape.
//   instrument       `BlockState.instrument()` — the `NoteBlockInstrument`
//                    this state gives a note block resting on it, or sitting
//                    under it. **Per state and not per block**, and not
//                    derivable from anything else in this table: it is a
//                    `BlockBehaviour.Properties` field with no relationship
//                    to hardness, sound type or material.
//
//                    It is a column here because `NoteBlock.setInstrument`
//                    reads **two** world positions — `pos.above()` first,
//                    then `pos.below()` — so no placement table keyed by the
//                    clicked face can carry the answer. `placement.json` has
//                    one cell per face and necessarily records whichever
//                    instrument the golden harness happened to have under
//                    the block, which is why an unmodified server places a
//                    note block reading `basedrum` on ground that gives
//                    `harp`.
//
//                    `instruments` at the top level carries the enum itself:
//                    the three `Type` predicates and the sound event, which
//                    are properties of the instrument and not of any state,
//                    so repeating them 32,366 times would be a table
//                    measuring its own key.
//   sturdyFaces      six bits, `isFaceSturdy(dir, SupportType.FULL)` — what
//                    `has_sturdy_face` asks. Position-independent for the
//                    same reason the occlusion shapes are: the two-argument
//                    form delegates to `BlockStateBase.Cache`, which is
//                    computed once at bootstrap and indexed by direction and
//                    support type alone.
//   fullFaces        six bits, one per `Direction`, each true when
//                    `Block.isShapeFullBlock(state.getFaceOcclusionShape(d))`.
//                    `UnderwaterMagmaFeature.isVisibleFromOutside` is the
//                    negation of this, and reads it on one face at a time.
//   redstoneConductor
//                    `BlockStateBase.isRedstoneConductor`, a per-block
//                    `StatePredicate` on `BlockBehaviour.Properties` evaluated
//                    per state. **Not derivable from any column above**: the
//                    default is a whole-shape `isCollisionShapeFullBlock` test
//                    that the per-face `collisionFullFaces` cannot answer, and
//                    a block may override the predicate outright.
//                    `RedStoneWireBlock.getConnectingSide` reads it at 106.
//   signalSource     `BlockState.isSignalSource()`. The whole of what
//                    `RedStoneWireBlock.shouldConnectTo` needs from a table —
//                    the wire, the repeater and the observer are decided by
//                    block identity and their own `facing`, and the comparator
//                    has no facing rule there at all.
//
// The last two arrived on 2026-08-05 for port `450`. Adding them changes this
// file's digest, which `ci/provenance.sh` stamps into `block_props.json`, so
// the table MUST be regenerated in the same commit or every gate refuses it.
//
// Directions are dumped in `Direction.values()` order — DOWN, UP, NORTH,
// SOUTH, WEST, EAST — because that is the order everything in vanilla
// iterates them in, and a table whose column order disagrees with the
// enum is a silent transposition.
//
// Converted to JSON by `ci/block-props-golden.sh`.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.commands.arguments.blocks.BlockStateParser;

public final class BlockPropsGolden {

    /// The block tags a `canSurvive` rule reads **of the block it is standing
    /// on**, resolved here by `state.is(tag)` rather than by a Rust-side tag
    /// flattener.
    ///
    /// Two reasons and the second is the load-bearing one. Vanilla's own
    /// answer is the fact, and a second flattener is a second thing that can
    /// be wrong — `#minecraft:supports_vegetation` is
    /// `{#minecraft:substrate_overworld, minecraft:farmland}` and
    /// `#minecraft:unstable_bottom_center` is `{#minecraft:fence_gates}`, so
    /// every one of these needs the nested resolution that a flat read of
    /// `tags_block.json` does not do. `unstableBottomCenter` has been a
    /// column of this table for exactly that reason since it was written;
    /// these six are the same decision applied to the rest of the set.
    ///
    /// The order is the header's `placeTagOrder` and the column is a
    /// bitstring in it, so adding a seventh is one entry here and one bit per
    /// row.
    /// Refuse a run whose tags did not bind, rather than emit a column of
    /// zeroes.
    ///
    /// Two blocks, one per shape of tag: `oak_fence_gate` reaches
    /// `#unstable_bottom_center` through one `#` reference, `grass_block`
    /// reaches `#supports_vegetation` through two. A control that only
    /// checked a flat tag would pass on a binder that had stopped following
    /// references.
    private static void assertPlaceTagsAreBound() {
        if (!net.minecraft.world.level.block.Blocks.OAK_FENCE_GATE.defaultBlockState()
                .is(net.minecraft.tags.BlockTags.UNSTABLE_BOTTOM_CENTER)) {
            throw new IllegalStateException(
                "block tags did not bind: oak_fence_gate is not in #unstable_bottom_center");
        }
        if (!net.minecraft.world.level.block.Blocks.GRASS_BLOCK.defaultBlockState()
                .is(net.minecraft.tags.BlockTags.SUPPORTS_VEGETATION)) {
            throw new IllegalStateException(
                "block tags did not bind: grass_block is not in #supports_vegetation");
        }
        if (!net.minecraft.world.level.block.Blocks.WATER.defaultBlockState().getFluidState()
                .is(net.minecraft.tags.FluidTags.SUPPORTS_LILY_PAD)) {
            throw new IllegalStateException(
                "fluid tags did not bind: water is not in #supports_lily_pad");
        }
    }

    private static final net.minecraft.tags.TagKey<?>[] PLACE_TAGS = {
        net.minecraft.tags.BlockTags.SUPPORTS_VEGETATION,
        net.minecraft.tags.BlockTags.SUPPORTS_CROPS,
        net.minecraft.tags.BlockTags.CANNOT_SUPPORT_SNOW_LAYER,
        net.minecraft.tags.BlockTags.SUPPORT_OVERRIDE_SNOW_LAYER,
        net.minecraft.tags.BlockTags.OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT,
        net.minecraft.tags.BlockTags.WALL_HANGING_SIGNS,
        net.minecraft.tags.BlockTags.SUPPORTS_DRY_VEGETATION,
        net.minecraft.tags.BlockTags.SUPPORTS_AZALEA,
        net.minecraft.tags.BlockTags.SUPPORTS_NETHER_SPROUTS,
        net.minecraft.tags.BlockTags.SUPPORTS_NETHER_WART,
        net.minecraft.tags.BlockTags.SUPPORTS_WITHER_ROSE,
        net.minecraft.tags.BlockTags.CANNOT_SUPPORT_SEAGRASS,
        net.minecraft.tags.BlockTags.SUPPORT_OVERRIDE_CACTUS_FLOWER,
        net.minecraft.tags.BlockTags.SUPPORTS_LILY_PAD,
    };

    /// The one **fluid** tag a `mayPlaceOn` reads.
    ///
    /// `LilyPadBlock.mayPlaceOn` is `fluidBelow.is(FluidTags.SUPPORTS_LILY_PAD)
    /// || below.is(BlockTags.SUPPORTS_LILY_PAD)`, and the two are different
    /// registries with the same name. A port that read only the block one
    /// refuses a lily pad on water, which is every lily pad.
    private static final net.minecraft.tags.TagKey<
        net.minecraft.world.level.material.Fluid>[] PLACE_FLUID_TAGS = fluidTags();

    @SuppressWarnings("unchecked")
    private static net.minecraft.tags.TagKey<net.minecraft.world.level.material.Fluid>[]
        fluidTags() {
        return (net.minecraft.tags.TagKey<net.minecraft.world.level.material.Fluid>[])
            new net.minecraft.tags.TagKey<?>[] {
                net.minecraft.tags.FluidTags.SUPPORTS_LILY_PAD,
            };
    }

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        // **`unstableBottomCenter` was 0 of 32,366 states until this line**,
        // and its doc comment in `crates/rustmc-behavior/src/block_props.rs`
        // described a refusal no row implemented. An unbound tag is not an
        // error in this codebase — `HolderSet` resolves empty and
        // `state.is(tag)` answers `false` — so the column shipped, was read,
        // and was silently a constant. Same class as the 16-of-17 tool
        // `HolderSet`s in `docs/cold-start/items.md`.
        //
        // `SurfaceChunkGolden.bindBlockTags` is the flat one and is correct
        // here: every tag this file reads resolves inside the top-level
        // directory (`#unstable_bottom_center` -> `#fence_gates`,
        // `#supports_vegetation` -> `#substrate_overworld` -> four more, all
        // top-level). `mineable/*` is the only subdirectory and nothing here
        // reads it.
        SurfaceChunkGolden.bindBlockTags();
        SurfaceChunkGolden.bindFluidTags();
        assertPlaceTagsAreBound();

        // `Block.BLOCK_STATE_REGISTRY` in id order, so the table is indexed
        // by the same number the chunk sections store.
        List<String> rows = new ArrayList<>();
        int air = 0;
        int withFluid = 0;
        int solid = 0;
        int total = 0;
        /// States where `LightEngine.getOcclusionShape` is NOT empty, i.e.
        /// where occlusion needs the voxel shape and not just `lightDampening`.
        int shapeLightOcclusion = 0;
        /// States with `getLightEmission() > 0` — the block-light sources.
        int emitters = 0;
        int randomlyTicking = 0;
        int fluidRandomlyTicking = 0;

        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(state);
            String block = BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString();

            String fluid = "";
            var fluidState = state.getFluidState();
            if (!fluidState.isEmpty()) {
                fluid = BuiltInRegistries.FLUID.getKey(fluidState.getType()).toString();
                withFluid++;
            }

            StringBuilder faces = new StringBuilder(6);
            StringBuilder sturdy = new StringBuilder(6);
            StringBuilder center = new StringBuilder(6);
            StringBuilder collision = new StringBuilder(6);
            // `LightEngine.getOcclusionShape(state, dir)` reduced against an
            // empty neighbour, which is `Shapes.faceShapeOccludes(shape,
            // Shapes.empty())` — "does this face stop light on its own".
            //
            // Note what `getOcclusionShape` actually is:
            //   isEmptyShape(state) ? Shapes.empty()
            //                       : state.getFaceOcclusionShape(dir)
            // and `isEmptyShape` is `!canOcclude() || !useShapeForLightOcclusion()`.
            // So for every state where that predicate is false the shape is
            // EMPTY and this column is all zeroes — occlusion is then decided
            // entirely by `lightDampening`. The column matters only for the
            // states `shapeLightOcclusion` marks, and the count of those is
            // emitted below as `shapeLightOcclusionStates` so a port can say
            // exactly how much of the block set a dampening-only light engine
            // is exact for, instead of guessing.
            StringBuilder lightOcclusion = new StringBuilder(6);
            StringBuilder rigid = new StringBuilder(6);
            for (Direction direction : Direction.values()) {
                boolean full = Block.isShapeFullBlock(state.getFaceOcclusionShape(direction));
                faces.append(full ? '1' : '0');
                // `EmptyBlockGetter.INSTANCE` and the origin: the cached path
                // reads neither, and a state whose cache is absent is one
                // this table has no business claiming to know. Vanilla's
                // three-argument form fixes SupportType.FULL, which is what
                // `has_sturdy_face` uses.
                sturdy.append(state.isFaceSturdy(
                    net.minecraft.world.level.EmptyBlockGetter.INSTANCE,
                    net.minecraft.core.BlockPos.ZERO,
                    direction) ? '1' : '0');
                // `MultifaceBlock.canAttachTo` accepts a face that is full on
                // *either* the support shape or the collision shape, and the
                // two disagree — a slab's collision shape is full on top, its
                // support shape is not. Neither column alone answers it.
                collision.append(Block.isFaceFull(
                    state.getCollisionShape(
                        net.minecraft.world.level.EmptyBlockGetter.INSTANCE,
                        net.minecraft.core.BlockPos.ZERO),
                    direction) ? '1' : '0');
                // SupportType.CENTER, which is a *different question* from
                // the FULL column above and not derivable from it: a fence
                // post covers the centre of its own down face and none of
                // the rest of it. `Block.canSupportCenter` is what decides
                // whether a mineshaft hangs a chain from something, so
                // answering it with the FULL column drops every chain and
                // fills a pillar of logs down to bedrock instead.
                center.append(state.isFaceSturdy(
                    net.minecraft.world.level.EmptyBlockGetter.INSTANCE,
                    net.minecraft.core.BlockPos.ZERO,
                    direction,
                    net.minecraft.world.level.block.SupportType.CENTER) ? '1' : '0');
                lightOcclusion.append(
                    net.minecraft.world.phys.shapes.Shapes.faceShapeOccludes(
                        net.minecraft.world.level.lighting.LightEngine
                            .getOcclusionShape(state, direction),
                        net.minecraft.world.phys.shapes.Shapes.empty()) ? '1' : '0');
                // SupportType.RIGID, and a THIRD question again. It joins the
                // block support shape's face against a 12x16 column and asks
                // whether anything of that column is left uncovered, so a
                // block may be FULL and not RIGID and the other way round.
                // `Block.canSupportRigidBlock` is `isFaceSturdy(UP, RIGID)`
                // of the block below and it is the whole of
                // `BaseRailBlock.canSurvive` — a rail on a hopper is the case
                // the FULL column gets wrong.
                rigid.append(state.isFaceSturdy(
                    net.minecraft.world.level.EmptyBlockGetter.INSTANCE,
                    net.minecraft.core.BlockPos.ZERO,
                    direction,
                    net.minecraft.world.level.block.SupportType.RIGID) ? '1' : '0');
            }
            if (state.canOcclude() && state.useShapeForLightOcclusion()) {
                shapeLightOcclusion++;
            }
            if (state.getLightEmission() > 0) {
                emitters++;
            }

            if (state.isAir()) {
                air++;
            }
            if (state.isSolidRender()) {
                solid++;
            }
            total++;

            StringBuilder placeTags = new StringBuilder(PLACE_TAGS.length);
            for (net.minecraft.tags.TagKey<?> tag : PLACE_TAGS) {
                @SuppressWarnings("unchecked")
                net.minecraft.tags.TagKey<net.minecraft.world.level.block.Block> blockTag =
                    (net.minecraft.tags.TagKey<net.minecraft.world.level.block.Block>) tag;
                placeTags.append(state.is(blockTag) ? '1' : '0');
            }

            StringBuilder placeFluidTags = new StringBuilder(PLACE_FLUID_TAGS.length);
            for (net.minecraft.tags.TagKey<net.minecraft.world.level.material.Fluid> tag
                    : PLACE_FLUID_TAGS) {
                placeFluidTags.append(state.getFluidState().is(tag) ? '1' : '0');
            }

            boolean leaves = state.getBlock() instanceof net.minecraft.world.level.block.LeavesBlock;
            rows.add("{\"id\": " + id
                + ", \"block\": " + quote(block)
                + ", \"state\": " + quote(BlockStateParser.serialize(state))
                + ", \"air\": " + state.isAir()
                + ", \"fluid\": " + quote(fluid)
                + ", \"solidRender\": " + state.isSolidRender()
                // The three light properties, added 2026-08-18 for
                // `crate::sky` and the daylight detector. All three are
                // public on `BlockBehaviour$BlockStateBase`.
                //
                // `propagatesSkylightDown` is the one the detector needs: it
                // is exactly "sky light passes straight down through this
                // state undiminished", so a column is under open sky at y if
                // every state above y propagates. It is NOT `!canOcclude` —
                // water propagates nothing and occludes nothing — and it is
                // not `isSolidRender` either, which is a rendering question.
                + ", \"propagatesSkylightDown\": " + state.propagatesSkylightDown()
                + ", \"lightDampening\": " + state.getLightDampening()
                + ", \"lightEmission\": " + state.getLightEmission()
                + ", \"blocksMotion\": " + state.blocksMotion()
                + ", \"leaves\": " + leaves
                // `BlockState.canBeReplaced()` — the no-argument form, which
                // is the block's `replaceable` property and not the
                // placement-context form. Sculk vein spreads into anything
                // replaceable.
                + ", \"replaceable\": " + state.canBeReplaced()
                // `BlockState.isSolid()` — the legacy "is this a solid block"
                // flag, and **not** `isSolidRender`. Monster rooms test their
                // floor and ceiling with it.
                + ", \"solid\": " + state.isSolid()
                // Whether this is the block's *default* state. A datapack
                // writes `{"Name": "minecraft:redstone_ore"}` and vanilla
                // resolves it through `defaultBlockState()`, which carries
                // every property the block has — `[lit=false]`. Without this
                // flag a port has no way to spell the state vanilla wrote.
                + ", \"default\": "
                + (state == state.getBlock().defaultBlockState())
                // The four light inputs. Nothing in this table could answer
                // any of them before 2026-08-18, which is why the section
                // light in every chunk packet was 0 of 26 classified.
                //
                // `getLightDampening` is what 1.21 renamed `getLightBlock` to;
                // it is the per-state opacity the light engine subtracts, and
                // it is NOT derivable from `solidRender` — water dampens 1 and
                // renders non-solid, a barrier dampens 0 and occludes.
                + ", \"lightEmission\": " + state.getLightEmission()
                + ", \"lightDampening\": " + state.getLightDampening()
                + ", \"canOcclude\": " + state.canOcclude()
                + ", \"useShapeForLightOcclusion\": " + state.useShapeForLightOcclusion()
                // `!LightEngine.isEmptyShape(state)` — true exactly when the
                // shape path is live and a dampening-only engine can be wrong.
                + ", \"shapeLightOcclusion\": "
                + (state.canOcclude() && state.useShapeForLightOcclusion())
                + ", \"lightOcclusionFaces\": " + quote(lightOcclusion.toString())
                + ", \"fullFaces\": " + quote(faces.toString())
                + ", \"sturdyFaces\": " + quote(sturdy.toString())
                + ", \"collisionFullFaces\": " + quote(collision.toString())
                + ", \"centerFaces\": " + quote(center.toString())
                + ", \"rigidFaces\": " + quote(rigid.toString())
                + ", \"placeTags\": " + quote(placeTags.toString())
                + ", \"placeFluidTags\": " + quote(placeFluidTags.toString())
                // `Block.canSupportCenter` refuses a DOWN face outright when
                // the block is in this tag, before it ever looks at a shape.
                + ", \"unstableBottomCenter\": "
                + state.is(net.minecraft.tags.BlockTags.UNSTABLE_BOTTOM_CENTER)
                // `BlockState.instrument()` — see the header. Serialized
                // name, so it is the same spelling the `instrument` block
                // property uses and a port can compare the two directly.
                + ", \"instrument\": "
                + quote(state.instrument().getSerializedName())
                // `BlockStateBase.isRedstoneConductor`, which is a **per-block
                // `StatePredicate` field on `BlockBehaviour.Properties`,
                // evaluated per state** — the method body is nothing but
                // `this.isRedstoneConductor.test(state, level, pos)`.
                //
                // None of the eight columns above can stand in for it. The
                // DEFAULT predicate is a whole-shape `isCollisionShapeFullBlock`
                // test, which `collisionFullFaces` cannot answer because that
                // one is per face; and any block may replace the predicate
                // outright, so even a correct whole-shape column would be
                // wrong for the blocks that override.
                //
                // `RedStoneWireBlock.getConnectingSide` asks it at offset 106
                // to decide whether a wire is blocked from climbing, and there
                // was no way to answer that from this file until now.
                + ", \"redstoneConductor\": "
                + state.isRedstoneConductor(
                    net.minecraft.world.level.EmptyBlockGetter.INSTANCE,
                    net.minecraft.core.BlockPos.ZERO)
                // `BlockState.isSignalSource()` — the whole of what
                // `RedStoneWireBlock.shouldConnectTo` needs from a table.
                //
                // Read from the two-argument overload's bytecode rather than
                // assumed, because the shape of that method is not what a
                // description of it suggests:
                //
                //   if (state.is(REDSTONE_WIRE)) return true;          //  0-11
                //   if (state.is(REPEATER))                            // 12-19
                //       return FACING == dir || FACING.opposite == dir; // 22-50
                //   if (state.is(OBSERVER))                            // 52-59
                //       return dir == FACING;                          // 62-77
                //   return state.isSignalSource() && dir != null;      // 79-95
                //
                // So the wire, the repeater and the observer are decided by
                // **block identity plus their own `facing`**, both of which a
                // state string already carries, and only the fall-through
                // needs a column. A COMPARATOR IS NOT SPECIAL-CASED: it has no
                // facing rule here at all and reaches the last line like any
                // other signal source, which is the opposite of what "including
                // repeater and comparator facing" would lead a port to build.
                + ", \"signalSource\": " + state.isSignalSource()
                // `BlockStateBase.isRandomlyTicking()` — whether
                // `ServerLevel.tickChunk`'s block pass calls `randomTick` on
                // this state when it draws its position.
                //
                // **Per state and not per block**, which is the whole reason
                // it is a column here rather than a name list somewhere. The
                // field is cached at state construction from
                // `Block.isRandomlyTicking(state)`, whose default is the
                // block's `Properties.randomTicks` flag — and **twenty-five
                // classes override it per state**: leaves random tick only at
                // `distance=7, persistent=false`, a crop only below its final
                // age, a weathering copper only while it can still oxidise. A
                // table keyed by block name is wrong for every one of them,
                // in both directions.
                + ", \"randomlyTicking\": " + state.isRandomlyTicking()
                // `FluidState.isRandomlyTicking()`, asked separately by the
                // same loop immediately afterwards and **not** implied by the
                // column above: `tickChunk` tests the block, ticks it, then
                // tests the state's fluid and ticks that too, so a waterlogged
                // block can random tick twice from one drawn position. It is
                // also what makes a section randomly ticking when no *block*
                // in it is — `LevelChunkSection.isRandomlyTicking()` is
                // `tickingBlockCount > 0 || tickingFluidCount > 0`.
                + ", \"fluidRandomlyTicking\": " + state.getFluidState().isRandomlyTicking()
                + "}");
            if (state.isRandomlyTicking()) {
                randomlyTicking++;
            }
            if (state.getFluidState().isRandomlyTicking()) {
                fluidRandomlyTicking++;
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"block_props\",\n");
        out.append("  \"directionOrder\": [");
        for (int i = 0; i < Direction.values().length; i++) {
            out.append(i == 0 ? "" : ", ").append(quote(Direction.values()[i].name()));
        }
        out.append("],\n");
        out.append("  \"placeTagOrder\": [");
        for (int i = 0; i < PLACE_TAGS.length; i++) {
            out.append(i == 0 ? "" : ", ")
                .append(quote(PLACE_TAGS[i].location().toString()));
        }
        out.append("],\n");
        out.append("  \"placeFluidTagOrder\": [");
        for (int i = 0; i < PLACE_FLUID_TAGS.length; i++) {
            out.append(i == 0 ? "" : ", ")
                .append(quote(PLACE_FLUID_TAGS[i].location().toString()));
        }
        out.append("],\n");
        out.append("  \"states\": ").append(total).append(",\n");
        out.append("  \"airStates\": ").append(air).append(",\n");
        out.append("  \"fluidStates\": ").append(withFluid).append(",\n");
        out.append("  \"solidRenderStates\": ").append(solid).append(",\n");
        // Coverage figures for the light engine, so "a dampening-only engine
        // is exact for N of M states" is a measurement and not a hope.
        out.append("  \"shapeLightOcclusionStates\": ")
            .append(shapeLightOcclusion).append(",\n");
        out.append("  \"lightEmittingStates\": ").append(emitters).append(",\n");
        out.append("  \"randomlyTickingStates\": ").append(randomlyTicking).append(",\n");
        out.append("  \"fluidRandomlyTickingStates\": ").append(fluidRandomlyTicking).append(",\n");
        // The enum itself. `NoteBlock` asks three questions of an
        // instrument and none of them is a property of a block state:
        // `worksAboveNoteBlock` picks which of the two neighbours wins in
        // `setInstrument`, `isTunable` decides whether `triggerEvent` reads
        // NOTE for a pitch or uses 1.0F, and `hasCustomSound` sends it to a
        // block entity for a sound id instead of the enum's own.
        //
        // All three are `type ==`/`!=` tests on the same field, so `type` is
        // dumped beside them: a port that reads `type` and a port that reads
        // the three booleans cannot disagree, and if a later version adds a
        // fourth Type the booleans alone would silently stop covering it.
        out.append("  \"instruments\": [\n");
        var instruments = net.minecraft.world.level.block.state.properties.NoteBlockInstrument.values();
        for (int i = 0; i < instruments.length; i++) {
            var instrument = instruments[i];
            // `getSoundEvent()` is the enum's own field and is **absent for
            // the CUSTOM type**, whose sound comes from the skull block
            // entity at play time. Dumping a null here rather than skipping
            // the row keeps the array a complete enumeration of the enum.
            String soundName = "";
            int soundId = -1;
            if (!instrument.hasCustomSound()) {
                var holder = instrument.getSoundEvent();
                if (holder != null && holder.value() != null) {
                    soundName = BuiltInRegistries.SOUND_EVENT
                        .getKey(holder.value()).toString();
                    soundId = BuiltInRegistries.SOUND_EVENT.getId(holder.value());
                }
            }
            out.append("    {\"name\": ").append(quote(instrument.getSerializedName()))
                .append(", \"type\": ").append(quote(instrumentType(instrument)))
                .append(", \"tunable\": ").append(instrument.isTunable())
                .append(", \"worksAbove\": ").append(instrument.worksAboveNoteBlock())
                .append(", \"customSound\": ").append(instrument.hasCustomSound())
                .append(", \"sound\": ").append(quote(soundName))
                .append(", \"soundId\": ").append(soundId)
                .append("}").append(i + 1 < instruments.length ? ",\n" : "\n");
        }
        out.append("  ],\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// The `NoteBlockInstrument$Type` this instrument carries, recovered
    /// from the three public predicates rather than the private `type`
    /// field. The three are mutually exclusive by construction:
    /// `isTunable` is `type == BASE_BLOCK`, `hasCustomSound` is
    /// `type == CUSTOM`, and `worksAboveNoteBlock` is `type != BASE_BLOCK`,
    /// so MOB_HEAD is the one that works above and has no custom sound.
    private static String instrumentType(
            net.minecraft.world.level.block.state.properties.NoteBlockInstrument instrument) {
        if (instrument.isTunable()) {
            return "BASE_BLOCK";
        }
        return instrument.hasCustomSound() ? "CUSTOM" : "MOB_HEAD";
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private BlockPropsGolden() {}
}
