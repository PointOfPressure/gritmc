// ExplosionEntityGolden — vanilla running `getSeenPercent`'s sampling grid and
// `getEntityDamageAmount`'s arithmetic, recorded.
//
// The entity half of an explosion is arithmetic plus one raycast. This runs
// both of vanilla's halves and separates them:
//
//   samples  every `Vec3` `ServerExplosion.getSeenPercent` hands to
//            `Level.clip`, in the order the three loops emit them. The stub
//            level records the origin and answers MISS, so the fraction is
//            1.0 and the recorded list is the grid itself. **That grid is the
//            part this server can reproduce**; the raycast is refused (there
//            is no `ClipContext` in this workspace).
//   fraction the same call with the stub answering HIT on every k-th sample,
//            so `(float) hits / (float) total` is exercised at values other
//            than 1.0 and 0.0.
//   damage   `new ExplosionDamageCalculator().getEntityDamageAmount(explosion,
//            entity, seenPercent)` over a grid of radii, distances and
//            exposures, and `getKnockbackMultiplier` beside it.
//
// # Why the grid is worth a golden of its own
//
// Three of its properties are invisible to a careful reading:
//
//   * **there is no `oy`.** The bytecode has two `Math.floor` calls, at
//     offsets 65 and 82, and they build the x and z offsets. The y sample is
//     the bare `Mth.lerp`. A symmetric-looking rewrite adds a third.
//   * **the loop bound is `<= 1.0` on an accumulating `double`.** How many
//     samples a box gets is decided by floating-point accumulation, not by a
//     computed count, and `(1.0 / d).round() + 1` disagrees for some sizes.
//   * **`dy < 0.0` is strict**, so a zero-height box samples rather than
//     returning 0.0f.
//
// # How the objects exist without a world
//
// Same trick as `ExplosionRayGolden`: `SeenLevel` and `SeenEntity` are
// allocated through `sun.reflect.ReflectionFactory.newConstructorForSerialization`
// and their fields set by reflection. `Entity.getBoundingBox()` is `final` and
// returns the private `bb` field, so the field is what gets set;
// `Entity.level()` returns the private `level` field, and `distanceToSqr`
// reads `position`. `Level.clip` is a `default` method on `BlockGetter` and is
// overridden here — which is the point, because the real one needs a chunk
// source.
//
// Every field not named above is null on both objects, so an unforeseen call
// is a NullPointerException at its own line rather than a plausible answer.
//
// Converted to JSON on stdout; `ci/explosion-entity-golden.sh` checks it.

package net.minecraft.world.level.levelgen;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.util.ArrayList;
import java.util.List;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.level.ClipContext;
import net.minecraft.world.level.Explosion;
import net.minecraft.world.level.ExplosionDamageCalculator;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.ServerExplosion;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;

public final class ExplosionEntityGolden {
    private ExplosionEntityGolden() {}

    /** Records every clip origin and answers HIT on every `hitEvery`-th. */
    public static final class SeenLevel extends net.minecraft.server.level.ServerLevel {
        public List<Vec3> seen;
        public int hitEvery;
        public int calls;

        // `ServerLevel` rather than `Level` because `Level` is abstract in
        // eighteen methods this fixture would have to stub for no reason;
        // `ServerLevel` implements all of them and is never constructed here.
        // `Level.clip` is a `default` on `BlockGetter` that `ServerLevel` does
        // not override, so the override below is the one that runs.
        private SeenLevel() {
            super(null, null, null, null, null, null, false, 0L, null, false);
            throw new UnsupportedOperationException("SeenLevel is allocated, never constructed");
        }

        @Override
        public BlockHitResult clip(ClipContext ctx) {
            this.seen.add(ctx.getFrom());
            this.calls++;
            if (this.hitEvery > 0 && this.calls % this.hitEvery == 0) {
                // Anything that is not MISS. `getSeenPercent` counts only
                // `HitResult.Type.MISS`, so the block and the face are
                // irrelevant and are recorded as such.
                return new BlockHitResult(ctx.getFrom(), Direction.UP, BlockPos.ZERO, false);
            }
            return BlockHitResult.miss(ctx.getTo(), Direction.UP, BlockPos.ZERO);
        }
    }

    /** An `Entity` that is a bounding box, a position and a level. */
    public static final class SeenEntity extends Entity {
        private SeenEntity() {
            super(null, null);
            throw new UnsupportedOperationException("SeenEntity is allocated, never constructed");
        }

        // `getSeenPercent` builds a `ClipContext`, whose `CollisionContext.of`
        // reads the entity's crouch state out of `SynchedEntityData` — which a
        // constructor-less allocation does not have. Answered directly rather
        // than by building a synched-data table, because the answer is a
        // recorded input to the fixture and not a convenience: every probe
        // here is an upright entity.
        @Override
        public boolean isShiftKeyDown() {
            return false;
        }

        @Override
        public boolean isDescending() {
            return false;
        }

        @Override
        protected void defineSynchedData(net.minecraft.network.syncher.SynchedEntityData.Builder b) {
        }

        @Override
        public boolean hurtServer(
                net.minecraft.server.level.ServerLevel level,
                net.minecraft.world.damagesource.DamageSource source,
                float amount) {
            throw new UnsupportedOperationException("SeenEntity.hurtServer");
        }

        @Override
        protected void readAdditionalSaveData(net.minecraft.world.level.storage.ValueInput in) {
        }

        @Override
        protected void addAdditionalSaveData(net.minecraft.world.level.storage.ValueOutput out) {
        }
    }

    /** The two methods `getEntityDamageAmount` calls, and a throw for the rest. */
    private record StubExplosion(Vec3 centre, float r) implements Explosion {
        @Override
        public net.minecraft.server.level.ServerLevel level() {
            throw new UnsupportedOperationException("Explosion.level");
        }

        @Override
        public Explosion.BlockInteraction getBlockInteraction() {
            throw new UnsupportedOperationException("Explosion.getBlockInteraction");
        }

        @Override
        public net.minecraft.world.entity.LivingEntity getIndirectSourceEntity() {
            throw new UnsupportedOperationException("Explosion.getIndirectSourceEntity");
        }

        @Override
        public Entity getDirectSourceEntity() {
            throw new UnsupportedOperationException("Explosion.getDirectSourceEntity");
        }

        @Override
        public float radius() {
            return this.r;
        }

        @Override
        public Vec3 center() {
            return this.centre;
        }

        @Override
        public boolean canTriggerBlocks() {
            throw new UnsupportedOperationException("Explosion.canTriggerBlocks");
        }

        @Override
        public boolean shouldAffectBlocklikeEntities() {
            throw new UnsupportedOperationException("Explosion.shouldAffectBlocklikeEntities");
        }
    }

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

    private static String q(String s) {
        return "\"" + s + "\"";
    }

    private static String bits(double v) {
        return Long.toString(Double.doubleToRawLongBits(v));
    }

    private static String seenCase(String name, double minX, double minY, double minZ,
            double maxX, double maxY, double maxZ, double cx, double cy, double cz,
            int hitEvery) throws Exception {
        SeenLevel level = (SeenLevel) allocate(SeenLevel.class);
        level.seen = new ArrayList<>();
        level.hitEvery = hitEvery;
        level.calls = 0;

        SeenEntity entity = (SeenEntity) allocate(SeenEntity.class);
        set(entity, Entity.class, "level", level);
        set(entity, Entity.class, "bb", new AABB(minX, minY, minZ, maxX, maxY, maxZ));
        set(entity, Entity.class, "position",
                new Vec3((minX + maxX) / 2.0, minY, (minZ + maxZ) / 2.0));

        Vec3 centre = new Vec3(cx, cy, cz);
        float pct = ServerExplosion.getSeenPercent(centre, entity);

        StringBuilder samples = new StringBuilder();
        for (Vec3 v : level.seen) {
            if (samples.length() > 0) {
                samples.append(',');
            }
            samples.append(bits(v.x)).append(',').append(bits(v.y)).append(',').append(bits(v.z));
        }
        return "    {" + q("name") + ": " + q(name)
                + ", " + q("minBits") + ": [" + bits(minX) + ", " + bits(minY) + ", "
                + bits(minZ) + "]"
                + ", " + q("maxBits") + ": [" + bits(maxX) + ", " + bits(maxY) + ", "
                + bits(maxZ) + "]"
                + ", " + q("centerBits") + ": [" + bits(cx) + ", " + bits(cy) + ", "
                + bits(cz) + "]"
                + ", " + q("hitEvery") + ": " + hitEvery
                + ", " + q("samples") + ": " + level.seen.size()
                + ", " + q("seenPercentBits") + ": " + Float.floatToRawIntBits(pct)
                + ", " + q("seenPercent") + ": " + pct
                + ",\n     " + q("origins") + ": [" + samples + "]}";
    }

    private static String damageCase(float radius, double distance, float seen) {
        ExplosionDamageCalculator calc = new ExplosionDamageCalculator();
        Vec3 centre = new Vec3(0.0, 0.0, 0.0);
        Explosion exp = new StubExplosion(centre, radius);
        // `distanceToSqr` is over the entity's `position`, so the entity is
        // placed on the x axis at exactly `distance`.
        SeenEntity entity;
        try {
            entity = (SeenEntity) allocate(SeenEntity.class);
            set(entity, Entity.class, "position", new Vec3(distance, 0.0, 0.0));
        } catch (Exception e) {
            throw new IllegalStateException(e);
        }
        float dmg = calc.getEntityDamageAmount(exp, entity, seen);
        float kb = calc.getKnockbackMultiplier(entity);
        return "    {" + q("radiusBits") + ": " + Float.floatToRawIntBits(radius)
                + ", " + q("radius") + ": " + radius
                + ", " + q("distanceBits") + ": " + bits(distance)
                + ", " + q("distance") + ": " + distance
                + ", " + q("seenBits") + ": " + Float.floatToRawIntBits(seen)
                + ", " + q("seen") + ": " + seen
                + ", " + q("damageBits") + ": " + Float.floatToRawIntBits(dmg)
                + ", " + q("damage") + ": " + dmg
                + ", " + q("knockbackMultiplierBits") + ": " + Float.floatToRawIntBits(kb) + "}";
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        List<String> seen = new ArrayList<>();
        // A player: 0.6 x 1.8 x 0.6.
        seen.add(seenCase("player", -0.3, 64.0, -0.3, 0.3, 65.8, 0.3, 0.5, 65.5, 0.5, 0));
        // The same box with every third ray blocked, so the fraction is not 1.
        seen.add(seenCase("player_third_blocked", -0.3, 64.0, -0.3, 0.3, 65.8, 0.3,
                0.5, 65.5, 0.5, 3));
        // Every ray blocked: `hits / total` is 0.0f and not a division by zero.
        seen.add(seenCase("player_all_blocked", -0.3, 64.0, -0.3, 0.3, 65.8, 0.3,
                0.5, 65.5, 0.5, 1));
        // A unit cube, whose dx = dy = dz = 1/3.
        seen.add(seenCase("unit_cube", 0.0, 64.0, 0.0, 1.0, 65.0, 1.0, 4.5, 65.5, 4.5, 0));
        // Wide and flat: dx = dz = 1/5, dy = 1. The flat one is the `dy < 0.0`
        // guard's case — strictly less, so it samples.
        seen.add(seenCase("wide_flat", -1.0, 64.0, -1.0, 1.0, 64.0, 1.0, 0.5, 66.0, 0.5, 0));
        // Tall and thin, so the three step sizes are all different.
        seen.add(seenCase("tall_thin", -0.1, 64.0, -0.25, 0.1, 67.0, 0.25, 3.5, 65.5, 3.5, 0));
        // An off-axis box at a negative origin, which exercises the offsets'
        // sign.
        seen.add(seenCase("negative_origin", -7.8, 64.0, -3.55, -7.2, 65.8, -2.95,
                -7.5, 64.5, -3.25, 0));

        List<String> damage = new ArrayList<>();
        for (float radius : new float[] {1.5F, 3.0F, 4.0F, 5.0F, 8.0F}) {
            for (double distance : new double[] {0.0, 0.5, 1.0, 2.0, 4.0, 7.9, 8.0}) {
                for (float pct : new float[] {0.0F, 0.25F, 0.5F, 1.0F}) {
                    damage.add(damageCase(radius, distance, pct));
                }
            }
        }

        StringBuilder out = new StringBuilder();
        out.append("{\n");
        out.append("  ").append(q("seenCases")).append(": [\n");
        out.append(String.join(",\n", seen));
        out.append("\n  ],\n");
        out.append("  ").append(q("damageCases")).append(": [\n");
        out.append(String.join(",\n", damage));
        out.append("\n  ]\n}\n");
        System.out.print(out);
    }
}
