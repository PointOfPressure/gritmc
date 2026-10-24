// EntityMoveGolden — vanilla running `Entity.collide`, recorded.
//
// `crates/rustmc-server/src/collide.rs` is a transcription, and a
// transcription compared against a reading of the same bytecode proves that
// two readings agree. This runs the methods.
//
// # What is recorded, and why all three
//
//   colliders  `Entity.collectAllColliders` — the block positions whose
//              shapes entered the list, in the order `Cursor3D` yielded them.
//              **This is a separate failure mode from the sweep.** A port can
//              resolve every axis perfectly against the wrong set of blocks:
//              the 1e-7 admission margin, the one-cell shell, the face/edge/
//              corner filter and `hasLargeCollisionShape` all decide
//              membership and none of them show up in a resolved delta that
//              happens to be `d` either way.
//   swept      `Entity.collideBoundingBox` — the three-axis sweep with no
//              auto-step. The axis ORDER only shows here, and only on a
//              diagonal, which is why half the probes are diagonal.
//   moved      `Entity.collide` itself, reached by reflection because it is
//              private. This is the one that includes auto-step, and it is
//              the arrangement — which box the step probe uses, which `y` the
//              candidate filter reads — that a bytecode reading gets wrong.
//
// Recording only `moved` would be cheaper and would hide which of the three
// broke. This project has twice spent a turn on a red whose cause was a layer
// down from where it was measured.
//
// # Doubles are raw bits
//
// Every coordinate, delta and result is `Double.doubleToRawLongBits`. A
// decimal rendering round-trips only through a correctly rounded parser, and
// a reader cannot tell a bit-exact fixture from one that is a half-ulp out by
// looking at it. This file's whole value is that it is exact.
//
// # The world is a HashMap and everything else throws
//
// `MoveLevel` implements the handful of `Level` methods the collision path
// actually reaches and raises `UnsupportedOperationException` for the rest,
// naming the method. Same argument as `GenLevel` and `UseGolden.TableLevel`:
// a stub that invents a plausible empty answer for a method the analysis did
// not foresee produces a fixture that is quietly wrong, and this project has
// chased four parity failures to exactly that. A throw names the method.
//
// Two answers are given rather than thrown and both are recorded facts, not
// conveniences:
//
//   * `getEntities()` is a real but **empty** `LevelEntityGetter`, so
//     `Level.getEntityCollisions` runs vanilla's own code and returns an
//     empty list. The dumper asserts that it is empty rather than assuming
//     it — `entityCollisions` in the output is that assertion's value, and
//     the Rust side passes an empty entity list on the same grounds.
//   * `getWorldBorder()` is a default `WorldBorder`, which is 60,000,000
//     wide, and every probe is within a few blocks of the origin. So
//     `isInsideCloseToBorder` is false and no border shape joins the list.
//     `borderShapes` records that count, and a non-zero one would mean the
//     probes had drifted out to where this fixture stops being about blocks.
//
// # What the scenarios have to contain to be worth running
//
// A golden that only walks a player along flat stone agrees with almost any
// implementation. The layouts below are chosen so that each one can only be
// reproduced by getting a specific thing right:
//
//   flat/slab/stair    the ordinary cases, and the y grid lines auto-step reads
//   embedded           the box starts INSIDE a multi-part shape, which is the
//                      only case where a merged box list gives a different
//                      answer from the grid — see CollisionShapesGolden
//   fence/wall         a collision shape 1.5 blocks tall, so the block is a
//                      cell BELOW the swept box and only reaches the list
//                      through the face-cell shell and `hasLargeCollisionShape`
//   corner             two walls meeting, entered diagonally, at both
//                      |x| < |z| and |x| > |z| so `axisStepOrder` picks both
//                      branches and a fixed X-then-Z port fails one of them
//   sliver             a box whose overlap with a block is 1e-8 thick, which
//                      the admission test rejects and a strict `<` admits
//   flush              faces exactly equal, where every eps comparison is at
//                      its boundary
//
// Converted to JSON by `ci/entity-move-golden.sh`. **Committed**, unlike the
// tables under `data/vanilla/`: this is a fixture that is only ever compared
// against, so a stale one turns a test red and can do nothing else.

package net.minecraft.world.level.levelgen;

import java.util.ArrayList;
import java.util.Collection;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import com.mojang.authlib.GameProfile;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Holder;
import net.minecraft.core.HolderLookup;
import net.minecraft.core.RegistryAccess;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.data.registries.VanillaRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.Difficulty;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.dimension.BuiltinDimensionTypes;
import net.minecraft.world.level.dimension.DimensionType;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.storage.LevelData;
import net.minecraft.world.level.storage.WritableLevelData;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class EntityMoveGolden {

    /// One block layout: a name and the blocks it places, relative to the
    /// origin. Every probe runs against every layout, so a layout that
    /// happens to be irrelevant to a probe simply produces the free-space
    /// answer, which is itself worth pinning.
    /// `offset` translates both the blocks and every probe box, so a layout
    /// can be re-run a long way from the origin. That is not decoration:
    /// `VoxelShape.move` adds the block position to each coordinate at read
    /// time, and whether `(c + o) - h` and `c - (h - o)` are the same double
    /// is a question about how big `o` is.
    private record Layout(String name, BlockPos offset, Map<BlockPos, BlockState> blocks) {}

    /// One movement probe: where the box is, how big, how far it is trying to
    /// go, and the two facts `Entity.collide` takes from the physics layer
    /// above it.
    private record Probe(String name, AABB box, Vec3 delta, float maxUpStep, boolean onGround) {}

    public static void main(String[] args) {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        HolderLookup.Provider lookup = VanillaRegistries.createLookup();
        Holder<DimensionType> overworld = lookup
            .lookupOrThrow(Registries.DIMENSION_TYPE)
            .getOrThrow(BuiltinDimensionTypes.OVERWORLD);

        // **Not `RegistryAccess.fromRegistryOfRegistries(BuiltInRegistries
        // .REGISTRY)`.** `Level`'s constructor reaches
        // `PalettedContainerFactory.create`, which looks up a registry that
        // lives in the datapack set and not in `BuiltInRegistries`, so that
        // shorter spelling throws inside the constructor. The lookup built by
        // `VanillaRegistries.createLookup()` carries both.
        MoveLevel level = new MoveLevel(accessOf(lookup), overworld);
        Prober prober = new Prober(level);

        List<Layout> layouts = layouts();
        List<Probe> probes = probes();

        List<String> rows = new ArrayList<>();
        int entityCollisions = 0;
        int borderShapes = 0;
        int nonEmptyColliders = 0;
        int movedDiffersFromSwept = 0;
        int clampedMoves = 0;

        for (Layout layout : layouts) {
            BlockPos o = layout.offset();
            Map<BlockPos, BlockState> placed = new HashMap<>();
            layout.blocks().forEach((at, state) -> placed.put(at.offset(o), state));
            level.load(placed);
            for (Probe raw : probes) {
                AABB box = raw.box().move(o.getX(), o.getY(), o.getZ());
                Probe probe = new Probe(
                    raw.name(), box, raw.delta(), raw.maxUpStep(), raw.onGround());
                prober.setBoundingBox(probe.box());
                prober.stepHeight = probe.maxUpStep();
                prober.setOnGround(probe.onGround());

                AABB expanded = probe.box().expandTowards(probe.delta());
                List<VoxelShape> entityShapes = level.getEntityCollisions(prober, expanded);
                entityCollisions += entityShapes.size();
                borderShapes += level.getWorldBorder()
                    .isInsideCloseToBorder(prober, expanded) ? 1 : 0;

                List<BlockPos> colliderPositions = new ArrayList<>();
                for (VoxelShape s : Entity.collectAllColliders(prober, level, expanded)) {
                    // The shape is already moved to its block, so its
                    // minimum corner is the block position for every shape
                    // whose own minimum is at its origin. That is not true
                    // in general -- a fence's shape starts at x=0.375 -- so
                    // the position is taken by flooring, which is what
                    // `Cursor3D` produced in the first place.
                    colliderPositions.add(BlockPos.containing(
                        Math.floor(s.min(net.minecraft.core.Direction.Axis.X) + 1.0E-7),
                        Math.floor(s.min(net.minecraft.core.Direction.Axis.Y) + 1.0E-7),
                        Math.floor(s.min(net.minecraft.core.Direction.Axis.Z) + 1.0E-7)));
                }
                if (!colliderPositions.isEmpty()) {
                    nonEmptyColliders++;
                }

                Vec3 swept = Entity.collideBoundingBox(
                    prober, probe.delta(), probe.box(), level, List.of());
                Vec3 moved = callCollide(prober, probe.delta());

                if (!moved.equals(swept)) {
                    movedDiffersFromSwept++;
                }
                if (!moved.equals(probe.delta())) {
                    clampedMoves++;
                }

                StringBuilder sb = new StringBuilder("{\"layout\": ")
                    .append(quote(layout.name()))
                    .append(", \"probe\": ").append(quote(probe.name()))
                    .append(", \"box\": ").append(boxJson(probe.box()))
                    .append(", \"delta\": ").append(vecJson(probe.delta()))
                    .append(", \"maxUpStep\": ")
                    .append(Float.floatToRawIntBits(probe.maxUpStep()))
                    .append(", \"onGround\": ").append(probe.onGround())
                    .append(", \"colliders\": [");
                for (int i = 0; i < colliderPositions.size(); i++) {
                    BlockPos p = colliderPositions.get(i);
                    sb.append(i == 0 ? "" : ", ")
                      .append("[").append(p.getX()).append(", ").append(p.getY())
                      .append(", ").append(p.getZ()).append("]");
                }
                sb.append("], \"swept\": ").append(vecJson(swept))
                  .append(", \"moved\": ").append(vecJson(moved))
                  .append("}");
                rows.add(sb.toString());
            }
        }

        StringBuilder out = new StringBuilder("{\n  \"rng\": \"entity_move\",\n");
        out.append("  \"layouts\": ").append(layouts.size()).append(",\n");
        out.append("  \"probes\": ").append(probes.size()).append(",\n");
        out.append("  \"cases\": ").append(rows.size()).append(",\n");
        // Instrument health, not results. A case list this size with no
        // colliders in it would be a fixture about empty space.
        out.append("  \"casesWithColliders\": ").append(nonEmptyColliders).append(",\n");
        out.append("  \"casesClamped\": ").append(clampedMoves).append(",\n");
        out.append("  \"casesWhereStepUpMoved\": ").append(movedDiffersFromSwept).append(",\n");
        out.append("  \"entityCollisions\": ").append(entityCollisions).append(",\n");
        out.append("  \"borderShapes\": ").append(borderShapes).append(",\n");
        out.append("  \"table\": [\n");
        for (int i = 0; i < rows.size(); i++) {
            out.append("    ").append(rows.get(i)).append(i + 1 < rows.size() ? ",\n" : "\n");
        }
        out.append("  ]\n}\n");
        System.out.print(out);
    }

    /// `Entity.collide(Vec3)` is private, so it is reached by reflection
    /// rather than reimplemented here. Reimplementing the arrangement in the
    /// dumper and comparing Rust against it would gate one reading of the
    /// bytecode against another, which is the exact thing this file exists
    /// to stop doing.
    private static Vec3 callCollide(Entity entity, Vec3 delta) {
        try {
            java.lang.reflect.Method m = Entity.class.getDeclaredMethod("collide", Vec3.class);
            m.setAccessible(true);
            return (Vec3) m.invoke(entity, delta);
        } catch (ReflectiveOperationException e) {
            throw new IllegalStateException(
                "Entity.collide(Vec3) is not reachable — it has been renamed or "
                + "its signature has changed, and this dumper would otherwise "
                + "quietly record only the sweep", e);
        }
    }

    /// A `RegistryAccess` over every registry the vanilla lookup knows,
    /// including the datapack ones. Same construction as `UseGolden`, for the
    /// same reason it is there.
    private static RegistryAccess accessOf(HolderLookup.Provider lookup) {
        List<net.minecraft.core.Registry<?>> registries = new ArrayList<>();
        lookup.listRegistries().forEach(r -> registries.add(rebuild(r)));
        return new RegistryAccess.ImmutableRegistryAccess(registries);
    }

    @SuppressWarnings("unchecked")
    private static <T> net.minecraft.core.Registry<T> rebuild(
        HolderLookup.RegistryLookup<T> lookup
    ) {
        net.minecraft.core.MappedRegistry<T> out = new net.minecraft.core.MappedRegistry<>(
            (net.minecraft.resources.ResourceKey<? extends net.minecraft.core.Registry<T>>)
                lookup.key(),
            com.mojang.serialization.Lifecycle.stable());
        lookup.listElements().forEach(
            h -> out.register(h.key(), h.value(), net.minecraft.core.RegistrationInfo.BUILT_IN));
        return out.freeze();
    }

    private static Map<BlockPos, BlockState> at(Object... pairs) {
        Map<BlockPos, BlockState> m = new HashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            m.put((BlockPos) pairs[i], (BlockState) pairs[i + 1]);
        }
        return m;
    }

    private static BlockPos p(int x, int y, int z) {
        return new BlockPos(x, y, z);
    }

    private static List<Layout> layouts() {
        BlockState stone = Blocks.STONE.defaultBlockState();
        BlockState slab = Blocks.OAK_SLAB.defaultBlockState();
        BlockState stairs = Blocks.OAK_STAIRS.defaultBlockState();
        BlockState fence = Blocks.OAK_FENCE.defaultBlockState();
        BlockState wall = Blocks.COBBLESTONE_WALL.defaultBlockState();
        BlockState soul = Blocks.SOUL_SAND.defaultBlockState();
        BlockState candle = Blocks.CANDLE.defaultBlockState();
        BlockState anvil = Blocks.ANVIL.defaultBlockState();
        BlockState trapdoor = Blocks.OAK_TRAPDOOR.defaultBlockState();
        BlockState hopper = Blocks.HOPPER.defaultBlockState();
        BlockState brewing = Blocks.BREWING_STAND.defaultBlockState();
        BlockState honey = Blocks.HONEY_BLOCK.defaultBlockState();
        BlockState bars = Blocks.IRON_BARS.defaultBlockState();
        BlockState lantern = Blocks.LANTERN.defaultBlockState();
        BlockState composter = Blocks.COMPOSTER.defaultBlockState();
        BlockState cake = Blocks.CAKE.defaultBlockState();
        BlockState gate = Blocks.OAK_FENCE_GATE.defaultBlockState();
        BlockState endRod = Blocks.END_ROD.defaultBlockState();
        BlockState chain = Blocks.IRON_CHAIN.defaultBlockState();
        BlockState grindstone = Blocks.GRINDSTONE.defaultBlockState();

        List<Layout> out = new ArrayList<>();
        out.add(new Layout("empty", BlockPos.ZERO, Map.of()));
        out.add(new Layout("floor", BlockPos.ZERO, floor(stone)));
        out.add(new Layout("floor_slab_step", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), slab))));
        out.add(new Layout("floor_stair_step", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), stairs))));
        out.add(new Layout("floor_block_step", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), stone))));
        out.add(new Layout("floor_fence", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), fence))));
        out.add(new Layout("floor_wall", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), wall))));
        out.add(new Layout("floor_soul_sand", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), soul))));
        out.add(new Layout("floor_honey", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), honey))));
        out.add(new Layout("floor_anvil", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), anvil))));
        out.add(new Layout("floor_trapdoor", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), trapdoor))));
        out.add(new Layout("floor_hopper", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), hopper))));
        out.add(new Layout("floor_brewing_stand", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), brewing))));
        out.add(new Layout("floor_bars", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), bars))));
        out.add(new Layout("floor_lantern", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), lantern))));
        out.add(new Layout("floor_composter", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), composter))));
        out.add(new Layout("floor_cake", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), cake))));
        out.add(new Layout("floor_gate", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), gate))));
        out.add(new Layout("floor_end_rod", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), endRod))));
        out.add(new Layout("floor_grindstone", BlockPos.ZERO, withFloor(stone, at(p(1, 0, 0), grindstone))));
        // The candle is the shape whose merged box list and grid disagree in
        // `CollisionShapesGolden`'s sweep, placed so that a probe box starts
        // inside it. Nothing else in this list exercises that.
        out.add(new Layout("embedded_candle", BlockPos.ZERO, at(p(0, 0, 0), candle)));
        out.add(new Layout("embedded_stone", BlockPos.ZERO, at(p(0, 0, 0), stone)));
        // Chain hangs in the middle of its cell, so a box can be inside the
        // block without touching the shape at all.
        out.add(new Layout("embedded_chain", BlockPos.ZERO, at(p(0, 0, 0), chain)));
        // Two walls meeting: the only layout a diagonal probe can be stopped
        // differently by depending on which horizontal axis resolves first.
        out.add(new Layout("corner_stone", BlockPos.ZERO, withFloor(stone,
            at(p(1, 0, 0), stone, p(0, 0, 1), stone, p(1, 0, 1), stone))));
        out.add(new Layout("corner_fence", BlockPos.ZERO, withFloor(stone,
            at(p(1, 0, 0), fence, p(0, 0, 1), fence, p(1, 0, 1), fence))));
        // A ceiling as well as a floor, so upward sweeps are clamped too.
        out.add(new Layout("floor_and_ceiling", BlockPos.ZERO, withFloor(stone,
            at(p(0, 3, 0), stone, p(1, 3, 0), stone, p(0, 3, 1), stone, p(-1, 3, 0), stone))));
        // A LONE block, with no floor under the probe. Everywhere else a
        // second block gives the same answer as the first, so whether one
        // particular block is admitted to the collider list cannot change the
        // result -- which is why the 1e-7 admission margin went unmeasured
        // until this layout existed.
        out.add(new Layout("lone_block", BlockPos.ZERO, at(p(1, 0, 0), stone)));
        out.add(new Layout("lone_slab", BlockPos.ZERO, at(p(1, 0, 0), slab)));
        out.add(new Layout("lone_fence", BlockPos.ZERO, at(p(1, 0, 0), fence)));
        // A ceiling one block up, so an upward move is clamped. That is the
        // only way to get `resolved.y` positive and different from the
        // requested y, which is what separates `startY` from `vec.y` and
        // what makes `grounded` have to test the direction and not just
        // whether y moved.
        out.add(new Layout("ceiling_low", BlockPos.ZERO, withFloor(stone, at(
            p(0, 1, 0), stone, p(1, 1, 0), stone, p(-1, 1, 0), stone,
            p(0, 1, 1), stone, p(1, 0, 0), slab))));
        out.add(new Layout("ceiling_low_stairs", BlockPos.ZERO, withFloor(stone, at(
            p(0, 1, 0), stone, p(1, 1, 0), stone, p(-1, 1, 0), stone,
            p(0, 1, 1), stone, p(1, 0, 0), stairs))));
        // The same world a million blocks out. Well inside the border, and
        // far enough that a block coordinate needs 20 bits before its shape's
        // fractions are added.
        BlockPos far = new BlockPos(1_000_000, 0, 1_000_000);
        out.add(new Layout("far_floor", far, floor(stone)));
        out.add(new Layout("far_slab_step", far, withFloor(stone, at(p(1, 0, 0), slab))));
        out.add(new Layout("far_corner", far, withFloor(stone,
            at(p(1, 0, 0), stone, p(0, 0, 1), stone, p(1, 0, 1), stone))));
        // A one-block hole in the floor, which is the layout auto-step must
        // NOT step across.
        Map<BlockPos, BlockState> holed = floor(stone);
        holed.remove(p(1, -1, 0));
        out.add(new Layout("floor_with_hole", BlockPos.ZERO, holed));
        return out;
    }

    /// A 5x5 floor at y = -1, which every walking probe stands on.
    private static Map<BlockPos, BlockState> floor(BlockState state) {
        Map<BlockPos, BlockState> m = new HashMap<>();
        for (int x = -2; x <= 2; x++) {
            for (int z = -2; z <= 2; z++) {
                m.put(p(x, -1, z), state);
            }
        }
        return m;
    }

    private static Map<BlockPos, BlockState> withFloor(
        BlockState floor, Map<BlockPos, BlockState> extra
    ) {
        Map<BlockPos, BlockState> m = floor(floor);
        m.putAll(extra);
        return m;
    }

    private static List<Probe> probes() {
        List<Probe> out = new ArrayList<>();
        // A player: 0.6 wide, 1.8 tall, 0.6 step height.
        AABB player = new AABB(-0.3, 0.0, -0.3, 0.3, 1.8, 0.3);
        // An item entity: 0.25 cube, no step height. This is what check 500's
        // thrown item would be.
        AABB item = new AABB(-0.125, 0.0, -0.125, 0.125, 0.25, 0.125);
        // A falling block: 0.98 cube, no step height. Check 070's sand.
        AABB falling = new AABB(-0.49, 0.0, -0.49, 0.49, 0.98, 0.49);

        double[] walks = {0.2, 0.5, 0.9, 1.4};
        for (double w : walks) {
            out.add(new Probe("walk_x_" + w, player, new Vec3(w, 0.0, 0.0), 0.6f, true));
            out.add(new Probe("walk_z_" + w, player, new Vec3(0.0, 0.0, w), 0.6f, true));
            out.add(new Probe("walk_negx_" + w, player, new Vec3(-w, 0.0, 0.0), 0.6f, true));
            // The two diagonals `axisStepOrder` splits on. `|x| < |z|`
            // resolves Z second and X third; the other way round for the
            // other. A port with a fixed order agrees with exactly one.
            out.add(new Probe("diag_xlt_" + w, player,
                new Vec3(w * 0.5, 0.0, w), 0.6f, true));
            out.add(new Probe("diag_xgt_" + w, player,
                new Vec3(w, 0.0, w * 0.5), 0.6f, true));
            out.add(new Probe("diag_equal_" + w, player, new Vec3(w, 0.0, w), 0.6f, true));
            // Walking while falling, which is the combination that makes
            // `grounded` true and moves the step box.
            out.add(new Probe("fall_walk_" + w, player,
                new Vec3(w, -0.1, w * 0.5), 0.6f, false));
        }
        // Pure vertical.
        for (double v : new double[]{-0.0784, -0.5, -1.2, 0.42, 1.0}) {
            out.add(new Probe("vertical_" + v, player, new Vec3(0.0, v, 0.0), 0.6f, false));
            out.add(new Probe("item_vertical_" + v, item, new Vec3(0.0, v, 0.0), 0.0f, false));
            out.add(new Probe("falling_vertical_" + v, falling,
                new Vec3(0.0, v, 0.0), 0.0f, false));
        }
        // Below the 1e-7 dead zone, and just over it. `Shapes.collide` clamps
        // to exactly zero inside it, and only inside it.
        for (double t : new double[]{1.0E-8, -1.0E-8, 1.0E-7, -1.0E-7, 2.0E-7, 1.0E-6}) {
            out.add(new Probe("tiny_x_" + t, player, new Vec3(t, 0.0, 0.0), 0.6f, true));
            out.add(new Probe("tiny_y_" + t, player, new Vec3(0.0, t, 0.0), 0.6f, true));
            out.add(new Probe("tiny_xyz_" + t, player, new Vec3(t, t, t), 0.6f, true));
        }
        // Zero, which short-circuits `collide` before `collideBoundingBox`
        // and is the only path where the two answers can differ for a reason
        // that is not auto-step.
        out.add(new Probe("zero", player, Vec3.ZERO, 0.6f, true));
        // Boxes offset so their faces are flush with a block face, and boxes
        // offset by a sliver past one, which is where the admission test's
        // 1e-7 margin decides membership.
        for (double o : new double[]{0.0, 1.0E-8, -1.0E-8, 1.0E-7, -1.0E-7, 0.5}) {
            AABB shifted = player.move(o, o, o);
            out.add(new Probe("flush_" + o, shifted, new Vec3(0.4, 0.0, 0.0), 0.6f, true));
            out.add(new Probe("flush_down_" + o, shifted, new Vec3(0.0, -0.4, 0.0), 0.6f, true));
            out.add(new Probe("flush_diag_" + o, shifted, new Vec3(0.4, -0.1, 0.7), 0.6f, true));
        }
        // --- the probes that exist because the falsification sweep found the
        // --- fixture could not tell these apart. Each names what it separates.

        // A lone block overlapped by 1e-8 **in the EXPANDED box**, which is
        // the box admission is decided on -- `collideBoundingBox` expands by
        // the delta before collecting colliders, so a sliver in the entity's
        // own box is not a sliver in the one that matters. Getting that wrong
        // is why the first version of these probes measured nothing.
        //
        // `joinIsNotEmpty`'s merged coordinate list treats values within
        // 1e-7 as one, so vanilla does not admit a 1e-8 overlap and a strict
        // `<` does. With no second block behind it, admission decides the
        // whole answer: admitted, the delta is clamped to 0.5; refused, it
        // keeps its 1e-8 tail.
        out.add(new Probe("sliver_over", new AABB(0.0, 0.0, -0.3, 0.5, 1.8, 0.3),
            new Vec3(0.5 + 1.0E-8, 0.0, 0.0), 0.6f, true));
        out.add(new Probe("sliver_over_z", new AABB(-0.3, 0.0, 0.0, 0.3, 1.8, 0.5),
            new Vec3(0.0, 0.0, 0.5 + 1.0E-8), 0.6f, true));
        out.add(new Probe("sliver_clear", new AABB(0.0, 0.0, -0.3, 0.5, 1.8, 0.3),
            new Vec3(0.5 + 1.0E-3, 0.0, 0.0), 0.6f, true));
        out.add(new Probe("sliver_under", new AABB(0.0, 0.0, -0.3, 0.5, 1.8, 0.3),
            new Vec3(0.5 - 1.0E-8, 0.0, 0.0), 0.6f, true));
        // A gap of 5e-8 resolves to a delta that is tiny and NON-ZERO, which
        // only survives if `Shapes.collide` tests its epsilon before each
        // shape rather than after it.
        out.add(new Probe("tiny_gap", new AABB(0.4, 0.0, -0.3, 1.0 - 5.0E-8, 1.8, 0.3),
            new Vec3(0.5, 0.0, 0.0), 0.6f, true));
        out.add(new Probe("tiny_gap_neg", new AABB(1.0 + 5.0E-8, 0.0, -0.3, 1.6, 1.8, 0.3),
            new Vec3(-0.5, 0.0, 0.0), 0.6f, true));
        // The box floor sits between 1.0E-5 and `Mth.equal`'s
        // 9.999999747378752E-6, so the step probe's downward expansion
        // reaches the block below under one constant and not the other.
        double mth = 9.9999998E-6;
        out.add(new Probe("mth_epsilon", new AABB(-0.3, mth, -0.3, 0.3, 1.8 + mth, 0.3),
            new Vec3(0.95, 0.0, 0.3), 0.6f, true));
        out.add(new Probe("mth_epsilon_z", new AABB(-0.3, mth, -0.3, 0.3, 1.8 + mth, 0.3),
            new Vec3(0.3, 0.0, 0.95), 0.6f, true));
        // Falling AND stepping in the same move. Every other falling probe
        // starts exactly on the floor, so `resolved.y` is 0 and the step box
        // and the original box are the same -- which is why nothing separated
        // them. The horizontal component has to exceed 0.7, the gap from the
        // box's own face to the step block at x=1, or the move is never
        // clamped and `collide` returns before it reaches auto-step.
        out.add(new Probe("fall_and_step", new AABB(-0.3, 0.3, -0.3, 0.3, 2.1, 0.3),
            new Vec3(0.95, -0.5, 0.0), 0.6f, false));
        out.add(new Probe("fall_and_step_diag", new AABB(-0.3, 0.45, -0.3, 0.3, 2.25, 0.3),
            new Vec3(0.9, -0.6, 0.85), 0.6f, false));
        out.add(new Probe("fall_and_step_small", new AABB(-0.3, 0.0625, -0.3, 0.3, 1.8625, 0.3),
            new Vec3(0.92, -0.2, 0.1), 0.6f, false));
        // Clamped UPWARDS while walking. `grounded` has to stay false because
        // the move is not downwards, and `startY` is the clamped y.
        out.add(new Probe("ceiling_up", new AABB(-0.3, 0.0, -0.3, 0.3, 1.0, 0.3),
            new Vec3(0.95, 0.4, 0.0), 0.6f, true));
        out.add(new Probe("ceiling_up_short",
            new AABB(-0.125, 0.0, -0.125, 0.125, 0.25, 0.125),
            new Vec3(1.05, 0.8, 0.0), 0.6f, true));
        out.add(new Probe("ceiling_up_diag", new AABB(-0.3, 0.0, -0.3, 0.3, 1.0, 0.3),
            new Vec3(0.9, 0.6, 0.95), 0.6f, true));

        // Step heights either side of a slab (0.5) and a full block (1.0), so
        // the candidate filter has to reject as well as accept.
        for (float s : new float[]{0.0f, 0.5f, 0.5625f, 0.6f, 1.0f, 1.5f}) {
            out.add(new Probe("step_" + s, player, new Vec3(0.5, 0.0, 0.0), s, true));
            out.add(new Probe("step_airborne_" + s, player,
                new Vec3(0.5, -0.05, 0.0), s, false));
        }
        return out;
    }

    /// An `Entity` that is nothing but a bounding box, a step height and an
    /// on-ground flag.
    ///
    /// `Entity.collide` reads exactly those three plus the level, so a real
    /// mob or player would drag in attributes and equipment that decide none
    /// of it. `maxUpStep()` and `onGround()` are both overridable and both
    /// are set per probe, which is what lets the fixture sweep step heights
    /// the entity's type would otherwise fix.
    static final class Prober extends Entity {

        float stepHeight;

        Prober(Level level) {
            // `MARKER` rather than a mob: it is the most inert type in the
            // registry, so nothing the constructor does can bring an
            // attribute map or an equipment slot into a fixture about
            // geometry. The bounding box is overwritten per probe anyway.
            super(EntityTypes.MARKER, level);
        }

        @Override
        public float maxUpStep() {
            return stepHeight;
        }

        @Override
        protected void defineSynchedData(net.minecraft.network.syncher.SynchedEntityData.Builder b) {
        }

        @Override
        public boolean hurtServer(
            net.minecraft.server.level.ServerLevel level,
            net.minecraft.world.damagesource.DamageSource source,
            float amount
        ) {
            throw new UnsupportedOperationException("Prober.hurtServer");
        }

        @Override
        protected void readAdditionalSaveData(net.minecraft.world.level.storage.ValueInput in) {
        }

        @Override
        protected void addAdditionalSaveData(net.minecraft.world.level.storage.ValueOutput out) {
        }
    }

    /// A `Level` that is a `HashMap` of blocks, and throws for everything
    /// else.
    static final class MoveLevel extends Level {

        private final Map<BlockPos, BlockState> blocks = new HashMap<>();
        private final net.minecraft.world.level.border.WorldBorder border =
            new net.minecraft.world.level.border.WorldBorder();

        MoveLevel(RegistryAccess access, Holder<DimensionType> dimension) {
            super(DATA, Level.OVERWORLD, access, dimension, false, false, 0L, 0);
        }

        void load(Map<BlockPos, BlockState> next) {
            blocks.clear();
            blocks.putAll(next);
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return blocks.getOrDefault(pos, Blocks.AIR.defaultBlockState());
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return getBlockState(pos).getFluidState();
        }

        /// `BlockCollisions` asks for a chunk and then reads block states out
        /// of it. The level is its own chunk here, which is honest: the map
        /// has no chunk boundaries, so no probe can straddle one.
        ///
        /// **That is a stated limit of this fixture.** Vanilla returns null
        /// for an unloaded chunk and `computeNext` then skips the whole
        /// column, which is a behaviour no probe here reaches.
        @Override
        public net.minecraft.world.level.BlockGetter getChunkForCollisions(int x, int z) {
            return this;
        }

        @Override
        public net.minecraft.world.level.border.WorldBorder getWorldBorder() {
            return border;
        }

        @Override
        public boolean isLoaded(BlockPos pos) {
            return true;
        }

        @Override
        public net.minecraft.world.level.block.entity.BlockEntity getBlockEntity(BlockPos pos) {
            return null;
        }

        @Override
        protected net.minecraft.world.level.entity.LevelEntityGetter<Entity> getEntities() {
            return NO_ENTITIES;
        }

        private static <T> T no(String what) {
            throw new UnsupportedOperationException(
                "EntityMoveGolden's level was asked for " + what
                + " — the collision path reads something this harness does not model");
        }

        @Override
        public boolean setBlock(BlockPos pos, BlockState state, int flags, int limit) {
            return no("setBlock");
        }

        @Override
        public net.minecraft.world.level.chunk.ChunkSource getChunkSource() {
            return no("getChunkSource");
        }

        @Override
        public net.minecraft.world.clock.ClockManager clockManager() {
            return no("clockManager");
        }

        /// Empty, and reached rather than hypothetical:
        /// `Level.getEntityCollisions` walks the dragon parts alongside the
        /// entity list, so this throwing stopped the first run. A world with
        /// no dragon in it has no parts, which is the same recorded fact as
        /// the empty `LevelEntityGetter` above -- and the dumper asserts the
        /// resulting entity-collision list is empty rather than assuming it.
        @Override
        public Collection<net.minecraft.world.entity.boss.enderdragon.EnderDragonPart> dragonParts() {
            return List.of();
        }

        @Override
        public Entity getEntity(int id) {
            return no("getEntity");
        }

        @Override
        public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() {
            return no("environmentAttributes");
        }

        @Override
        public net.minecraft.world.level.block.entity.FuelValues fuelValues() {
            return no("fuelValues");
        }

        @Override
        public Holder<net.minecraft.world.level.biome.Biome> getUncachedNoiseBiome(
            int x, int y, int z
        ) {
            return no("getUncachedNoiseBiome");
        }

        @Override
        public net.minecraft.world.ticks.LevelTickAccess<Block> getBlockTicks() {
            return no("getBlockTicks");
        }

        @Override
        public net.minecraft.world.ticks.LevelTickAccess<net.minecraft.world.level.material.Fluid>
            getFluidTicks() {
            return no("getFluidTicks");
        }

        @Override
        public List<? extends Player> players() {
            return List.of();
        }

        @Override
        public net.minecraft.world.level.saveddata.maps.MapItemSavedData getMapData(
            net.minecraft.world.level.saveddata.maps.MapId id
        ) {
            return no("getMapData");
        }

        @Override
        public net.minecraft.world.item.alchemy.PotionBrewing potionBrewing() {
            return no("potionBrewing");
        }

        @Override
        public net.minecraft.world.item.crafting.RecipeAccess recipeAccess() {
            return no("recipeAccess");
        }

        @Override
        public LevelData.RespawnData getRespawnData() {
            return no("getRespawnData");
        }

        @Override
        public void setRespawnData(LevelData.RespawnData data) {
            no("setRespawnData");
        }

        @Override
        public net.minecraft.world.scores.Scoreboard getScoreboard() {
            return no("getScoreboard");
        }

        @Override
        public String gatherChunkSourceStats() {
            return no("gatherChunkSourceStats");
        }

        @Override
        public net.minecraft.world.TickRateManager tickRateManager() {
            return no("tickRateManager");
        }

        @Override
        public int getSeaLevel() {
            return no("getSeaLevel");
        }

        @Override
        public void destroyBlockProgress(int id, BlockPos pos, int progress) {
            no("destroyBlockProgress");
        }

        @Override
        public void sendBlockUpdated(BlockPos pos, BlockState from, BlockState to, int flags) {
            no("sendBlockUpdated");
        }

        /// The one answer that is not a throw and not a recorded fact.
        /// `LevelReader.enabledFeatures` is consulted nowhere on the
        /// collision path -- it gates recipe and item availability -- but it
        /// is abstract, so it has to be something. The vanilla default set is
        /// the honest choice: it is what a normal world reports.
        @Override
        public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() {
            return net.minecraft.world.flag.FeatureFlags.DEFAULT_FLAGS;
        }

        @Override
        public void levelEvent(Entity source, int type, BlockPos pos, int data) {
            no("levelEvent");
        }

        @Override
        public void gameEvent(
            Holder<net.minecraft.world.level.gameevent.GameEvent> event,
            Vec3 at,
            net.minecraft.world.level.gameevent.GameEvent.Context context
        ) {
            no("gameEvent");
        }

        @Override
        public void playSeededSound(
            Entity source, double x, double y, double z,
            Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            no("playSeededSound");
        }

        @Override
        public void playSeededSound(
            Entity source, Entity target, Holder<net.minecraft.sounds.SoundEvent> sound,
            net.minecraft.sounds.SoundSource category, float volume, float pitch, long seed
        ) {
            no("playSeededSound");
        }

        @Override
        public void explode(
            Entity source, net.minecraft.world.damagesource.DamageSource damage,
            net.minecraft.world.level.ExplosionDamageCalculator calculator,
            double x, double y, double z, float radius, boolean fire,
            Level.ExplosionInteraction interaction,
            net.minecraft.core.particles.ParticleOptions small,
            net.minecraft.core.particles.ParticleOptions large,
            net.minecraft.util.random.WeightedList<
                net.minecraft.core.particles.ExplosionParticleInfo> particles,
            Holder<net.minecraft.sounds.SoundEvent> sound
        ) {
            no("explode");
        }
    }

    private static final net.minecraft.world.level.entity.LevelEntityGetter<Entity> NO_ENTITIES =
        new net.minecraft.world.level.entity.LevelEntityGetter<Entity>() {
            @Override
            public Entity get(int id) {
                return null;
            }

            @Override
            public Entity get(java.util.UUID uuid) {
                return null;
            }

            @Override
            public Iterable<Entity> getAll() {
                return List.of();
            }

            @Override
            public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                net.minecraft.util.AbortableIterationConsumer<U> consumer
            ) {
            }

            @Override
            public void get(AABB box, java.util.function.Consumer<Entity> consumer) {
            }

            @Override
            public <U extends Entity> void get(
                net.minecraft.world.level.entity.EntityTypeTest<Entity, U> test,
                AABB box,
                net.minecraft.util.AbortableIterationConsumer<U> consumer
            ) {
            }
        };

    private static final WritableLevelData DATA = new WritableLevelData() {
        @Override
        public LevelData.RespawnData getRespawnData() {
            throw new UnsupportedOperationException("levelData.getRespawnData");
        }

        @Override
        public void setSpawn(LevelData.RespawnData data) {
            throw new UnsupportedOperationException("levelData.setSpawn");
        }

        @Override
        public long getGameTime() {
            return 0L;
        }

        @Override
        public boolean isHardcore() {
            return false;
        }

        @Override
        public Difficulty getDifficulty() {
            return Difficulty.NORMAL;
        }

        @Override
        public boolean isDifficultyLocked() {
            return false;
        }
    };

    private static String bits(double v) {
        return Long.toString(Double.doubleToRawLongBits(v));
    }

    private static String vecJson(Vec3 v) {
        return "[" + bits(v.x) + ", " + bits(v.y) + ", " + bits(v.z) + "]";
    }

    private static String boxJson(AABB b) {
        return "[" + bits(b.minX) + ", " + bits(b.minY) + ", " + bits(b.minZ)
            + ", " + bits(b.maxX) + ", " + bits(b.maxY) + ", " + bits(b.maxZ) + "]";
    }

    private static String quote(String s) {
        return "\"" + s.replace("\\", "\\\\").replace("\"", "\\\"") + "\"";
    }

    private EntityMoveGolden() {}
}
