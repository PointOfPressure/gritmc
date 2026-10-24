// ExplosionRayGolden — vanilla running its own explosion ray walk, recorded.
//
// `crates/rustmc-server/src/explosion.rs` is a transcription, and a
// transcription compared against a second reading of the same bytecode proves
// only that two readings agree. This runs the method.
//
// # What is recorded, and why both lists
//
//   positions  `ServerExplosion.calculateExplodedPositions()`, reached by
//              reflection because it is private. This is the whole block pass:
//              the 1,352 shell rays, the one `nextFloat()` each, the
//              `0.30000001192092896` step, the `0.22500001f` decay, the
//              `(res + 0.3f) * 0.3f` subtraction, and — the part no reading of
//              the loop gives you — the **`java.util.HashSet` iteration order**
//              the `new ObjectArrayList<>(set)` at offset 416 freezes.
//   shuffled   the same list after `Util.shuffle(list, level.random)`, which
//              is what `interactWithBlocks` walks. It continues the *same*
//              stream the ray walk drew from, so recording it separately is
//              the only way to catch a port that gets 1,352 draws right and
//              then re-seeds.
//
// Recording only the shuffled list would be cheaper and would hide which of
// the two broke.
//
// # How a `ServerExplosion` exists at all without a world
//
// `ServerExplosion`'s `level` field is a `ServerLevel`, and constructing one
// means a chunk source, an entity manager and a save directory. So neither
// object is constructed: both are allocated through
// `sun.reflect.ReflectionFactory.newConstructorForSerialization`, which is how
// every serialization framework on the JVM makes an instance without running a
// constructor, and their fields are then set by reflection. `RayLevel` extends
// `ServerLevel` and overrides the three methods the ray walk calls —
// `getBlockState`, `getFluidState`, `isInWorldBounds` — and nothing else is
// ever touched, because nothing else is ever called.
//
// `Level.random` is read as a **field** (`getfield ServerLevel.random` at
// offset 167), not through a getter, so overriding a method would not do: the
// field itself is set, which reflection permits for a non-static final.
//
// This is a stub with the same contract as `EntityMoveGolden.MoveLevel` and
// `GenLevel`: a method the analysis did not foresee throws and names itself
// rather than inventing a plausible empty answer. Here the enforcement is
// stronger than a throw — every field of both objects is null, so an
// unforeseen call is a `NullPointerException` with a stack trace pointing at
// the exact line, and the dumper does not catch it.
//
// # The worlds, and why each one is here
//
//   open_air     nothing anywhere. `getBlockExplosionResistance` returns
//                `Optional.empty()`, so a ray is stopped only by its own
//                `0.22500001f` decay. This is the case that pins the two
//                constants and the reach.
//   stone        resistance 6.0 everywhere: `(6.0 + 0.3) * 0.3 = 1.89` a step
//                on top of the decay, so a TNT blast is a small ball.
//   obsidian     1200.0: `360.09` on the first step takes any `f` negative
//                before the membership test, so the set is empty and the
//                positions list is the empty list rather than absent.
//   floor        air at y >= 64, stone below. The realistic case, and the only
//                one whose result is not spherically symmetric — a port with
//                its axes transposed passes all three above and fails this.
//   water        a flooded world at 100.0, which is `30.09` a step: the fluid
//                arm of `Math.max(block, fluid)` rather than the block arm.
//   air_over_lava  air above, lava below, so the two arms of that `max` are
//                exercised in one walk.
//   out_of_bounds  `isInWorldBounds` false everywhere: every ray breaks on its
//                first step **after** spending its draw, which is what makes
//                the draw count independent of the world.
//
// Each is run at TNT's radius 4.0 and again at a creeper's 3.0, and `open_air`
// is run at two seeds so that a port ignoring the stream cannot pass.
//
// Converted to JSON on stdout; `ci/explosion-ray-golden.sh` checks it.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
import net.minecraft.util.Util;
import net.minecraft.world.level.Explosion;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.ServerExplosion;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;
import net.minecraft.world.phys.Vec3;

public final class ExplosionRayGolden {
    private ExplosionRayGolden() {}

    /** Which world a probe position is in. */
    private enum World {
        OPEN_AIR,
        STONE,
        OBSIDIAN,
        FLOOR,
        WATER,
        AIR_OVER_LAVA,
        OUT_OF_BOUNDS,
        WALL_EAST,
        CEILING
    }

    /**
     * A `ServerLevel` that answers the three questions the ray walk asks and
     * would throw on anything else, because every other field is null.
     */
    public static final class RayLevel extends ServerLevel {
        // Not final and not set by any constructor: this object is allocated
        // without one.
        public World world;

        // Never called. Present only because the compiler wants a constructor
        // and because a call to it would be a loud failure rather than a
        // silent one.
        private RayLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
            throw new UnsupportedOperationException("RayLevel is allocated, never constructed");
        }

        @Override
        public BlockState getBlockState(BlockPos pos) {
            return switch (this.world) {
                case OPEN_AIR, OUT_OF_BOUNDS, WATER -> Blocks.AIR.defaultBlockState();
                case STONE -> Blocks.STONE.defaultBlockState();
                case OBSIDIAN -> Blocks.OBSIDIAN.defaultBlockState();
                case FLOOR -> pos.getY() >= 64
                        ? Blocks.AIR.defaultBlockState()
                        : Blocks.STONE.defaultBlockState();
                case AIR_OVER_LAVA -> Blocks.AIR.defaultBlockState();
                // Obsidian on one side only. A port that transposed x and z in
                // its resistance lookup passes every symmetric world above and
                // fails this one; `floor` catches the same defect on y alone.
                case WALL_EAST -> pos.getX() >= 2
                        ? Blocks.OBSIDIAN.defaultBlockState()
                        : Blocks.AIR.defaultBlockState();
                case CEILING -> Blocks.AIR.defaultBlockState();
            };
        }

        @Override
        public FluidState getFluidState(BlockPos pos) {
            return switch (this.world) {
                case WATER -> Fluids.WATER.defaultFluidState();
                case AIR_OVER_LAVA -> pos.getY() < 64
                        ? Fluids.LAVA.defaultFluidState()
                        : Fluids.EMPTY.defaultFluidState();
                default -> Fluids.EMPTY.defaultFluidState();
            };
        }

        @Override
        public boolean isInWorldBounds(BlockPos pos) {
            // `CEILING` is a build-height limit rather than an absent world:
            // a ray that reaches it BREAKS, where a hard block only subtracts.
            // The two are different terminations and only this case separates
            // them.
            if (this.world == World.CEILING) {
                return pos.getY() < 68;
            }
            return this.world != World.OUT_OF_BOUNDS;
        }

        /** Where `createFire` put a fire, in the order it put them. */
        public List<BlockPos> fires;

        @Override
        public boolean setBlockAndUpdate(BlockPos pos, BlockState state) {
            this.fires.add(pos.immutable());
            return true;
        }
    }

    /** `16^3 - 14^3` — the shell cells, and so the ray-walk draw count. */
    private static final int RAYS = 16 * 16 * 16 - 14 * 14 * 14;

    private static Object allocate(Class<?> cls) throws Exception {
        Constructor<Object> obj = Object.class.getDeclaredConstructor();
        Constructor<?> c = sun.reflect.ReflectionFactory.getReflectionFactory()
                .newConstructorForSerialization(cls, obj);
        c.setAccessible(true);
        return c.newInstance();
    }

    private static void set(Object target, Class<?> declaring, String name, Object value)
            throws Exception {
        Field f = declaring.getDeclaredField(name);
        f.setAccessible(true);
        f.set(target, value);
    }

    private static void setFloat(Object target, Class<?> declaring, String name, float value)
            throws Exception {
        Field f = declaring.getDeclaredField(name);
        f.setAccessible(true);
        f.setFloat(target, value);
    }

    private static String q(String s) {
        return "\"" + s + "\"";
    }

    private static String posList(List<BlockPos> l) {
        StringBuilder b = new StringBuilder();
        for (BlockPos p : l) {
            if (b.length() > 0) {
                b.append(',');
            }
            b.append(p.getX()).append(',').append(p.getY()).append(',').append(p.getZ());
        }
        return b.toString();
    }

    @SuppressWarnings("unchecked")
    private static String run(String name, World world, double cx, double cy, double cz,
            float radius, long seed) throws Exception {
        RayLevel level = (RayLevel) allocate(RayLevel.class);
        level.world = world;
        RandomSource random = RandomSource.create(seed);
        // `getfield ServerLevel.random` at offset 167 resolves to Level's
        // field, so that is the one to set.
        set(level, net.minecraft.world.level.Level.class, "random", random);

        ServerExplosion exp = (ServerExplosion) allocate(ServerExplosion.class);
        set(exp, ServerExplosion.class, "level", level);
        set(exp, ServerExplosion.class, "center", new Vec3(cx, cy, cz));
        setFloat(exp, ServerExplosion.class, "radius", radius);
        set(exp, ServerExplosion.class, "damageCalculator", new ExplosionDamageCalculator());
        set(exp, ServerExplosion.class, "blockInteraction",
                Explosion.BlockInteraction.DESTROY_WITH_DECAY);

        Method m = ServerExplosion.class.getDeclaredMethod("calculateExplodedPositions");
        m.setAccessible(true);
        List<BlockPos> positions = (List<BlockPos>) m.invoke(exp);

        // The stream is not re-seeded: `interactWithBlocks` shuffles with the
        // same `level.random` the ray walk has just drawn 1,352 floats from.
        List<BlockPos> shuffled = new ArrayList<>(positions);
        Util.shuffle(shuffled, random);

        // `createFire`, twice, on two RandomSources at the SAME state the walk
        // left them in, over the two orders `explode()` can hand it.
        //
        //   fireAfterShuffle  what a DESTROY* or TRIGGER_BLOCK explosion does:
        //                     `interactWithBlocks` shuffled the one list in
        //                     place and `createFire` gets that object.
        //   fireNoShuffle     what a KEEP explosion does: `interactWithBlocks`
        //                     was never called, so neither the shuffle's draws
        //                     nor its permutation happened.
        //
        // The two are recorded from the same starting state so that the
        // difference between them is the shuffle and nothing else. This
        // isolates `createFire`: the world is unmodified, where in a real
        // `explode()` the blocks would already have been set to air.
        Method fireM = ServerExplosion.class.getDeclaredMethod("createFire", List.class);
        fireM.setAccessible(true);

        RandomSource afterWalk = RandomSource.create(seed);
        for (int i = 0; i < RAYS; i++) {
            afterWalk.nextFloat();
        }
        List<BlockPos> fireNoShuffle;
        {
            RayLevel l2 = (RayLevel) allocate(RayLevel.class);
            l2.world = world;
            l2.fires = new ArrayList<>();
            RandomSource r2 = RandomSource.create(seed);
            for (int i = 0; i < RAYS; i++) {
                r2.nextFloat();
            }
            set(l2, net.minecraft.world.level.Level.class, "random", r2);
            ServerExplosion e2 = (ServerExplosion) allocate(ServerExplosion.class);
            set(e2, ServerExplosion.class, "level", l2);
            fireM.invoke(e2, new ArrayList<>(positions));
            fireNoShuffle = l2.fires;
        }
        List<BlockPos> fireAfterShuffle;
        {
            RayLevel l3 = (RayLevel) allocate(RayLevel.class);
            l3.world = world;
            l3.fires = new ArrayList<>();
            RandomSource r3 = RandomSource.create(seed);
            for (int i = 0; i < RAYS; i++) {
                r3.nextFloat();
            }
            // The shuffle's own draws, spent on the same stream, exactly as
            // `interactWithBlocks` spends them before `createFire` runs.
            Util.shuffle(new ArrayList<>(positions), r3);
            set(l3, net.minecraft.world.level.Level.class, "random", r3);
            ServerExplosion e3 = (ServerExplosion) allocate(ServerExplosion.class);
            set(e3, ServerExplosion.class, "level", l3);
            fireM.invoke(e3, new ArrayList<>(shuffled));
            fireAfterShuffle = l3.fires;
        }

        return "    {" + q("name") + ": " + q(name)
                + ", " + q("world") + ": " + q(world.name().toLowerCase(java.util.Locale.ROOT))
                + ", " + q("centerBits") + ": [" + Double.doubleToRawLongBits(cx)
                + ", " + Double.doubleToRawLongBits(cy)
                + ", " + Double.doubleToRawLongBits(cz) + "]"
                + ", " + q("center") + ": [" + cx + ", " + cy + ", " + cz + "]"
                + ", " + q("radiusBits") + ": " + Float.floatToRawIntBits(radius)
                + ", " + q("radius") + ": " + radius
                + ", " + q("seed") + ": " + seed
                + ", " + q("count") + ": " + positions.size()
                + ",\n     " + q("positions") + ": [" + posList(positions) + "]"
                + ",\n     " + q("shuffled") + ": [" + posList(shuffled) + "]"
                + ",\n     " + q("fireNoShuffle") + ": [" + posList(fireNoShuffle) + "]"
                + ",\n     " + q("fireAfterShuffle") + ": [" + posList(fireAfterShuffle) + "]}";
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        List<String> cases = new ArrayList<>();
        // TNT's radius, at a block centre, on a floor at y = 64.
        cases.add(run("open_air_r4_s1", World.OPEN_AIR, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("open_air_r4_s2", World.OPEN_AIR, 0.5, 65.5, 0.5, 4.0F, 2L));
        cases.add(run("stone_r4", World.STONE, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("obsidian_r4", World.OBSIDIAN, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("floor_r4", World.FLOOR, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("water_r4", World.WATER, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("air_over_lava_r4", World.AIR_OVER_LAVA, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("out_of_bounds_r4", World.OUT_OF_BOUNDS, 0.5, 65.5, 0.5, 4.0F, 1L));
        // A creeper's radius, and an off-centre origin so that the `Mth.floor`
        // of a negative coordinate is exercised.
        cases.add(run("open_air_r3", World.OPEN_AIR, 0.5, 65.5, 0.5, 3.0F, 1L));
        cases.add(run("floor_r3", World.FLOOR, 0.5, 65.5, 0.5, 3.0F, 1L));
        cases.add(run("floor_r4_negative", World.FLOOR, -7.5, 64.0625, -3.25, 4.0F, 5L));
        // A radius of zero: `f` starts at 0.0, the `while` never runs, and the
        // 1,352 draws are still spent.
        cases.add(run("zero_radius", World.OPEN_AIR, 0.5, 65.5, 0.5, 0.0F, 1L));
        // Asymmetric in x, and a hard vertical boundary in y that is a `break`
        // rather than a subtraction.
        cases.add(run("wall_east_r4", World.WALL_EAST, 0.5, 65.5, 0.5, 4.0F, 1L));
        cases.add(run("ceiling_r4", World.CEILING, 0.5, 65.5, 0.5, 4.0F, 1L));

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("rays")).append(": ").append(RAYS).append(",\n");
        out.append("  ").append(q("cases")).append(": [\n");
        out.append(String.join(",\n", cases));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
