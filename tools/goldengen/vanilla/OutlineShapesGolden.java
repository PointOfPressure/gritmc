// OutlineShapesGolden -- every block state's OUTLINE shape (`getShape`), the
// shape `ClipContext.Block.OUTLINE` ray-traces against, in exactly the
// carrier `collision_shapes.json` uses (`shapes` with `coords` + `voxels`,
// `rows` per global state id, `-1` for "this table has no answer"), so the
// server reads both with one parser and clips both with one walker.
//
// Written for `BoatItem.use` -> `Item.getPlayerPOVHitResult(level, player,
// ClipContext.Fluid.ANY)`, whose `ClipContext` is `Block.OUTLINE`.
//
// A state is `-1` (refused, never guessed) when its outline is not a function
// of the state alone, which is measured here rather than listed by hand:
//
//   * it has an offset function (`BlockStateBase.hasOffsetFunction()`) and
//     its shape is NOT the same base shape moved by `getOffset(pos)` at two
//     probe positions. One that is (flowers, grass, bamboo, ...) is dumped
//     UNSHIFTED -- `getShape(ZERO).move(-getOffset(ZERO))` -- with its
//     `offsetStates` row `[id, xyz, maxHorizontal, maxVertical]`, and the
//     server moves it by `BlockStateBase.getOffset(pos)` itself;
//     `offsetGolden` carries vanilla's own offsets at a spread of positions
//     for the server's unit test of that formula;
//   * `getShape` read the `CollisionContext` it was handed -- a recording
//     `java.lang.reflect.Proxy` answers every method and notes the call
//     (scaffolding, light, ...): vanilla asks with the PLAYER's context;
//   * `getShape` read the `BlockGetter` -- a recording proxy over
//     `EmptyBlockGetter` notes any call (shulker boxes read their block
//     entity; a moving piston's OUTLINE is a plain `Shapes.empty()`, its
//     COLLISION shape is the one that reads).
//
// Run by `ci/outline-shapes-golden.sh`, twice, compared byte for byte.
package net.minecraft.world.level.levelgen;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.level.EmptyBlockGetter;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.phys.shapes.CollisionContext;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class OutlineShapesGolden {

    /** Records whether any method of the proxied interface was called. */
    static final class Recorder implements InvocationHandler {
        final Object delegate;
        boolean touched;
        Recorder(Object delegate) { this.delegate = delegate; }
        @Override
        public Object invoke(Object proxy, Method m, Object[] args) throws Throwable {
            if (m.getDeclaringClass() == Object.class) {
                return m.invoke(this, args);
            }
            touched = true;
            if (delegate != null) {
                return m.invoke(delegate, args);
            }
            Class<?> r = m.getReturnType();
            if (r == boolean.class) return Boolean.FALSE;
            if (r == int.class) return 0;
            if (r == double.class) return 0.0;
            if (r == float.class) return 0.0f;
            return null;
        }
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        Method signature = CollisionShapesGolden.class.getDeclaredMethod("signature", VoxelShape.class);
        signature.setAccessible(true);
        Class<?> gridClass = Class.forName("net.minecraft.world.level.levelgen.CollisionShapesGolden$Grid");
        Method gridOf = gridClass.getDeclaredMethod("of", VoxelShape.class);
        gridOf.setAccessible(true);
        Method maxH = net.minecraft.world.level.block.state.BlockBehaviour.class.getDeclaredMethod("getMaxHorizontalOffset");
        maxH.setAccessible(true);
        Method maxV = net.minecraft.world.level.block.state.BlockBehaviour.class.getDeclaredMethod("getMaxVerticalOffset");
        maxV.setAccessible(true);
        Method shapeRow = CollisionShapesGolden.class.getDeclaredMethod("shapeRow", VoxelShape.class, gridClass);
        shapeRow.setAccessible(true);

        Map<String, Integer> index = new LinkedHashMap<>();
        List<String> shapeRows = new ArrayList<>();
        int[] rows = new int[Block.BLOCK_STATE_REGISTRY.size()];
        List<Integer> movingPiston = new ArrayList<>();
        Set<String> offsetBlocks = new TreeSet<>();
        Set<String> unmovedBlocks = new TreeSet<>();
        List<String> offsetStates = new ArrayList<>();
        List<String> offsetGolden = new ArrayList<>();
        Set<String> goldenBlocks = new TreeSet<>();
        final int[][] probes = {{0, 0}, {7, 13}, {-5, 3}, {1000, -77}, {-31, -31}, {12345, 678}};
        Set<String> contextBlocks = new TreeSet<>();
        Set<String> levelBlocks = new TreeSet<>();
        int states = 0;
        int refused = 0;

        for (BlockState state : Block.BLOCK_STATE_REGISTRY) {
            int id = Block.BLOCK_STATE_REGISTRY.getId(state);
            states++;
            String name = BuiltInRegistries.BLOCK.getKey(state.getBlock()).toString();
            if (state.is(net.minecraft.world.level.block.Blocks.MOVING_PISTON)) {
                movingPiston.add(id);
            }
            Recorder ctx = new Recorder(null);
            Recorder lvl = new Recorder(EmptyBlockGetter.INSTANCE);
            CollisionContext context = (CollisionContext) Proxy.newProxyInstance(
                CollisionContext.class.getClassLoader(), new Class<?>[] {CollisionContext.class}, ctx);
            BlockGetter level = (BlockGetter) Proxy.newProxyInstance(
                BlockGetter.class.getClassLoader(), new Class<?>[] {BlockGetter.class}, lvl);
            VoxelShape shape;
            boolean offsetConsistent = true;
            boolean unmoved = false;
            try {
                shape = state.getShape(level, BlockPos.ZERO, context);
                if (state.hasOffsetFunction()) {
                    // The UNSHIFTED shape, exactly: `VoxelShape.move` wraps
                    // each axis in an `OffsetDoubleList(delegate, offset)`, so
                    // the delegate IS the block's own coordinates, with no
                    // subtraction to round. Moving it back out by
                    // `getOffset(p)` must then reproduce `getShape(p)` at
                    // every probe, or the block is refused.
                    VoxelShape base = unshift(shape);
                    if (base == null) {
                        // No offset list on the axes: this block has an
                        // offset function (for rendering) and a `getShape`
                        // that never applies it. Then the outline must be
                        // the same everywhere, and it is carried plain.
                        Object at0 = signature.invoke(null, shape);
                        for (int[] pr : probes) {
                            BlockPos p = new BlockPos(pr[0], 64, pr[1]);
                            offsetConsistent &= at0.equals(signature.invoke(null, state.getShape(level, p, context)));
                        }
                        unmoved = offsetConsistent;
                    } else {
                        for (int[] pr : probes) {
                            BlockPos p = new BlockPos(pr[0], 64, pr[1]);
                            Vec3 op = state.getOffset(p);
                            Object want = signature.invoke(null, state.getShape(level, p, context));
                            Object got = signature.invoke(null, base.move(op.x, op.y, op.z));
                            offsetConsistent &= want.equals(got);
                        }
                        shape = base;
                    }
                }
            } catch (RuntimeException e) {
                // A shape that cannot be computed without a real level is a
                // level read by another name.
                lvl.touched = true;
                shape = null;
            }
            boolean offset = state.hasOffsetFunction();
            if (offset) offsetBlocks.add(name);
            if (ctx.touched) contextBlocks.add(name);
            if (lvl.touched) levelBlocks.add(name);
            if (offset && unmoved && !ctx.touched && !lvl.touched && shape != null) {
                unmovedBlocks.add(name);
            } else if (offset && offsetConsistent && !ctx.touched && !lvl.touched && shape != null) {
                boolean xyz = false;
                for (int x = -40; x < 40 && !xyz; x++) {
                    for (int z = -40; z < 40 && !xyz; z++) {
                        xyz = state.getOffset(new BlockPos(x, 0, z)).y != 0.0;
                    }
                }
                offsetStates.add("[" + id + ", " + (xyz ? 1 : 0) + ", "
                    + Float.floatToRawIntBits((Float) maxH.invoke(state.getBlock())) + ", "
                    + Float.floatToRawIntBits((Float) maxV.invoke(state.getBlock())) + "]");
                if (goldenBlocks.add(name)) {
                    for (int[] pr : probes) {
                        Vec3 o = state.getOffset(new BlockPos(pr[0], 64, pr[1]));
                        offsetGolden.add("[" + id + ", " + pr[0] + ", " + pr[1] + ", "
                            + Double.doubleToRawLongBits(o.x) + ", " + Double.doubleToRawLongBits(o.y) + ", "
                            + Double.doubleToRawLongBits(o.z) + "]");
                    }
                }
            } else if (offset || ctx.touched || lvl.touched || shape == null) {
                rows[id] = -1;
                refused++;
                continue;
            }
            String key = (String) signature.invoke(null, shape);
            Integer at = index.get(key);
            if (at == null) {
                at = shapeRows.size();
                index.put(key, at);
                shapeRows.add((String) shapeRow.invoke(null, shape, gridOf.invoke(null, shape)));
            }
            rows[id] = at;
        }

        StringBuilder out = new StringBuilder("{\n");
        out.append("  \"rng\": \"outline_shapes\",\n");
        out.append("  \"states\": ").append(states).append(",\n");
        out.append("  \"distinctShapes\": ").append(shapeRows.size()).append(",\n");
        out.append("  \"refusedStates\": ").append(refused).append(",\n");
        out.append("  \"movingPistonStates\": ").append(movingPiston).append(",\n");
        out.append("  \"offsetBlocks\": ").append(quoted(offsetBlocks)).append(",\n");
        out.append("  \"contextBlocks\": ").append(quoted(contextBlocks)).append(",\n");
        out.append("  \"levelBlocks\": ").append(quoted(levelBlocks)).append(",\n");
        out.append("  \"unmovedOffsetBlocks\": ").append(quoted(unmovedBlocks)).append(",\n");
        out.append("  \"offsetStates\": ").append(offsetStates).append(",\n");
        out.append("  \"offsetGolden\": ").append(offsetGolden).append(",\n");
        out.append("  \"shapes\": [\n");
        for (int i = 0; i < shapeRows.size(); i++) {
            out.append("    ").append(shapeRows.get(i)).append(i + 1 < shapeRows.size() ? ",\n" : "\n");
        }
        out.append("  ],\n  \"rows\": [");
        for (int i = 0; i < rows.length; i++) {
            if (i > 0) out.append(", ");
            out.append(rows[i]);
        }
        out.append("]\n}\n");
        System.out.print(out);
    }

    /// The `OffsetDoubleList` delegates of a moved shape, rebuilt into an
    /// `ArrayVoxelShape` over the same `DiscreteVoxelShape`; `null` when an
    /// axis is not an offset list (a shape that was not moved).
    static VoxelShape unshift(VoxelShape moved) throws Exception {
        if (moved.isEmpty()) {
            return moved;
        }
        Class<?> odl = Class.forName("net.minecraft.world.phys.shapes.OffsetDoubleList");
        java.lang.reflect.Field delegate = odl.getDeclaredField("delegate");
        delegate.setAccessible(true);
        Object[] lists = new Object[3];
        int i = 0;
        for (net.minecraft.core.Direction.Axis a : net.minecraft.core.Direction.Axis.values()) {
            Object c = moved.getCoords(a);
            if (!odl.isInstance(c)) {
                return null;
            }
            lists[i++] = delegate.get(c);
        }
        java.lang.reflect.Field discrete = VoxelShape.class.getDeclaredField("shape");
        discrete.setAccessible(true);
        Class<?> avs = Class.forName("net.minecraft.world.phys.shapes.ArrayVoxelShape");
        Class<?> dvs = Class.forName("net.minecraft.world.phys.shapes.DiscreteVoxelShape");
        Class<?> dl = Class.forName("it.unimi.dsi.fastutil.doubles.DoubleList");
        java.lang.reflect.Constructor<?> k = avs.getDeclaredConstructor(dvs, dl, dl, dl);
        k.setAccessible(true);
        return (VoxelShape) k.newInstance(discrete.get(moved), lists[0], lists[1], lists[2]);
    }

    private static String quoted(Set<String> s) {
        StringBuilder b = new StringBuilder("[");
        boolean first = true;
        for (String x : s) {
            if (!first) b.append(", ");
            b.append('"').append(x).append('"');
            first = false;
        }
        return b.append(']').toString();
    }
}
