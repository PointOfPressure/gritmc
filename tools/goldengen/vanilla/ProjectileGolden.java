// ProjectileGolden — vanilla running the projectile path's arithmetic.
//
// # WHY THIS ONE HAS NO `Level`
//
// `EntityMoveGolden` builds a whole fake `Level` so it can call
// `Entity.collide` on a real entity. The projectile path needs the same
// treatment for `AbstractArrow.tick` and one day should get it — but the
// pieces that a transcription is most likely to get *numerically* wrong are
// all reachable with no world at all, and this file takes those first
// because they are cheap and they are where the bits are.
//
// Every one of the following is a public or reflection-reachable method on a
// class that touches no registry state, so this harness is 200 lines instead
// of 900 and needs no `MoveLevel`:
//
//   Vec3.normalize / horizontalDistance   the launch direction
//   RandomSource.triangle                 the inaccuracy draw, six per shot
//   Mth.atan2 * 57.2957763671875          both rotations, at the call site
//   Projectile.lerpRotation               protected static; the wrap loops
//   BowItem.getPowerForTime               public static; the quadratic curve
//   CrossbowItem.getShotPitch             private static; the index-0 branch
//   AABB.clip(Vec3, Vec3)                 the entity hit test
//   AABB.getCenter / intersects           the margin probe's aim point
//   VoxelShape.clip(Vec3, Vec3, BlockPos) the block hit, inside path included
//   BlockGetter.traverseBlocks            public static, takes lambdas; the
//                                         whole DDA walk with no Level
//
// # WHAT IS DELIBERATELY NOT HERE, AND WHY THAT IS NOT A GAP TO PAPER OVER
//
// `ProjectileUtil.computeMargin(Entity)` reads `entity.tickCount` off a real
// `Entity`; `getManyEntityHitResult` needs a `Level.getEntities`; and
// `AbstractArrow.tick` needs both plus a `ServerLevel`. Those want the
// `MoveLevel` treatment and they are named in
// `docs/cold-start/projectiles.md` as the next instrument rather than
// approximated here. A golden that quietly replaced `computeMargin` with a
// literal would be measuring its own input.
//
// # DOUBLES ARE RAW BITS
//
// Every coordinate and result is `Double.doubleToRawLongBits`, floats are
// `Float.floatToRawIntBits`. A decimal rendering round-trips only through a
// correctly rounded parser and a reader cannot tell a bit-exact fixture from
// one that is half an ulp out by looking at it.
//
// Converted to JSON by `ci/projectile-golden.sh`.

// The package is deliberately NOT one of vanilla's. Every other golden in
// this directory sits inside `net.minecraft.world.level.levelgen` to reach
// package-private members; this one needs none, and putting a class of ours
// into a signed package and then calling `Bootstrap.bootStrap()` throws
//
//   java.lang.SecurityException: class "net.minecraft.world.level.levelgen
//   .SurfaceRules$ConditionSource"'s signer information does not match signer
//   information of other classes in the same package
//
// at `BuiltInRegistries.<clinit>`. `ci/entity-move-golden.sh` solves that by
// stripping the jar's signatures into a second copy; this harness avoids
// needing to.
package rustmcgolden;

import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.List;
import java.util.Optional;

import net.minecraft.SharedConstants;
import net.minecraft.core.BlockPos;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.util.RandomSource;
import net.minecraft.world.level.BlockGetter;
import net.minecraft.world.phys.AABB;
import net.minecraft.world.phys.BlockHitResult;
import net.minecraft.world.phys.Vec3;
import net.minecraft.world.phys.shapes.Shapes;
import net.minecraft.world.phys.shapes.VoxelShape;

public final class ProjectileGolden {

    private static long db(double d) {
        return Double.doubleToRawLongBits(d);
    }

    private static int fb(float f) {
        return Float.floatToRawIntBits(f);
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();

        StringBuilder out = new StringBuilder("{\n");
        out.append("  \"version\": \"").append(SharedConstants.getCurrentVersion().name())
           .append("\",\n");

        // --- the constants, read off the classes rather than off a comment ---
        out.append("  \"radToDeg\": ").append(db(57.2957763671875d)).append(",\n");

        // --- Vec3.normalize and horizontalDistance ---------------------------
        //
        // The inputs are the direction vectors `shootFromRotation` produces
        // over a pitch/yaw grid, promoted from float exactly as vanilla does,
        // so the sweep is over values that occur rather than over a uniform
        // cube.
        out.append("  \"launchDirections\": [\n");
        StringBuilder rows = new StringBuilder();
        int n = 0;
        for (int pitch = -90; pitch <= 90; pitch += 15) {
            for (int yaw = -180; yaw <= 180; yaw += 15) {
                float xRot = pitch;
                float yRot = yaw;
                float f = -Mth.sin(yRot * 0.017453292f) * Mth.cos(xRot * 0.017453292f);
                float g = -Mth.sin((xRot + 0.0f) * 0.017453292f);
                float h = Mth.cos(yRot * 0.017453292f) * Mth.cos(xRot * 0.017453292f);
                Vec3 v = new Vec3((double) f, (double) g, (double) h);
                Vec3 nv = v.normalize();
                rows.append(n++ == 0 ? "" : ",\n")
                    .append("    {\"xRot\": ").append(pitch)
                    .append(", \"yRot\": ").append(yaw)
                    .append(", \"f\": ").append(fb(f))
                    .append(", \"g\": ").append(fb(g))
                    .append(", \"h\": ").append(fb(h))
                    .append(", \"nx\": ").append(db(nv.x))
                    .append(", \"ny\": ").append(db(nv.y))
                    .append(", \"nz\": ").append(db(nv.z))
                    .append(", \"hd\": ").append(db(nv.horizontalDistance()))
                    .append(", \"yawDeg\": ")
                    .append(fb((float) (Mth.atan2(nv.x, nv.z) * 57.2957763671875d)))
                    .append(", \"pitchDeg\": ")
                    .append(fb((float) (Mth.atan2(nv.y, nv.horizontalDistance())
                                        * 57.2957763671875d)))
                    .append("}");
            }
        }
        out.append(rows).append("\n  ],\n");

        // --- RandomSource.triangle, six draws per shot -----------------------
        //
        // `RandomSource.create(long)` is `new LegacyRandomSource(seed)`, whose
        // `nextDouble` is `java.util.Random`'s. The record is the whole
        // `getMovementToShoot` offset for one seed and one inaccuracy, so a
        // port that draws the wrong NUMBER of values diverges on the second
        // shot even when the first agrees.
        out.append("  \"triangleStreams\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (long seed : new long[] {0L, 1L, 42L, 20260906L, -7L}) {
            for (float inaccuracy : new float[] {0.0f, 1.0f, 3.0f, 17.0f}) {
                RandomSource r = RandomSource.create(seed);
                StringBuilder draws = new StringBuilder();
                for (int shot = 0; shot < 3; shot++) {
                    double e = 0.0172275d * (double) inaccuracy;
                    double dx = r.triangle(0.0d, e);
                    double dy = r.triangle(0.0d, e);
                    double dz = r.triangle(0.0d, e);
                    draws.append(shot == 0 ? "" : ", ")
                         .append(db(dx)).append(", ").append(db(dy)).append(", ")
                         .append(db(dz));
                }
                rows.append(n++ == 0 ? "" : ",\n")
                    .append("    {\"seed\": ").append(seed)
                    .append(", \"inaccuracy\": ").append(fb(inaccuracy))
                    .append(", \"draws\": [").append(draws).append("]}");
            }
        }
        out.append(rows).append("\n  ],\n");

        // --- Projectile.lerpRotation, by reflection --------------------------
        Method lerpRotation = Class
            .forName("net.minecraft.world.entity.projectile.Projectile")
            .getDeclaredMethod("lerpRotation", float.class, float.class);
        lerpRotation.setAccessible(true);
        out.append("  \"lerpRotation\": [\n");
        rows = new StringBuilder();
        n = 0;
        float[] angles = {-360.0f, -180.0f, -179.9f, -90.0f, -0.5f, 0.0f, 0.5f,
                          89.5f, 90.0f, 179.9f, 180.0f, 270.0f, 350.0f, 359.5f, 720.0f};
        for (float cur : angles) {
            for (float tgt : angles) {
                float got = (Float) lerpRotation.invoke(null, cur, tgt);
                rows.append(n++ == 0 ? "" : ",\n")
                    .append("    {\"current\": ").append(fb(cur))
                    .append(", \"target\": ").append(fb(tgt))
                    .append(", \"out\": ").append(fb(got)).append("}");
            }
        }
        out.append(rows).append("\n  ],\n");

        // --- BowItem.getPowerForTime ----------------------------------------
        out.append("  \"bowPower\": [\n");
        rows = new StringBuilder();
        for (int charge = 0; charge <= 40; charge++) {
            rows.append(charge == 0 ? "" : ",\n")
                .append("    {\"charge\": ").append(charge)
                .append(", \"power\": ")
                .append(fb(net.minecraft.world.item.BowItem.getPowerForTime(charge)))
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        // --- CrossbowItem.getShotPitch, by reflection ------------------------
        Method shotPitch = Class.forName("net.minecraft.world.item.CrossbowItem")
            .getDeclaredMethod("getShotPitch", RandomSource.class, int.class);
        shotPitch.setAccessible(true);
        out.append("  \"crossbowShotPitch\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (long seed : new long[] {0L, 5L, 20260906L}) {
            RandomSource r = RandomSource.create(seed);
            StringBuilder ps = new StringBuilder();
            for (int index = 0; index < 6; index++) {
                float p = (Float) shotPitch.invoke(null, r, index);
                ps.append(index == 0 ? "" : ", ").append(fb(p));
            }
            rows.append(n++ == 0 ? "" : ",\n")
                .append("    {\"seed\": ").append(seed)
                .append(", \"pitches\": [").append(ps).append("]}");
        }
        out.append(rows).append("\n  ],\n");

        // --- AABB.clip, getCenter, intersects --------------------------------
        //
        // The boxes and the segments are chosen so that each of the six faces
        // wins at least once, that a segment which would reach a box beyond
        // its end misses, and that a delta under the 1e-7 axis threshold is
        // ignored.
        AABB[] boxes = {
            new AABB(0.0, 0.0, 0.0, 1.0, 1.0, 1.0),
            new AABB(1.0, 0.0, 0.0, 2.0, 1.0, 1.0),
            new AABB(-0.3, 63.7, -0.6, 2.6, 67.1, 0.6),
            new AABB(1.7, 65.0, -0.3, 2.3, 66.8, 0.3),
            new AABB(0.1, 0.0, 0.0, 0.30000000000000004, 1.0, 1.0),
        };
        Vec3[][] segs = {
            {new Vec3(-1.0, 0.5, 0.5), new Vec3(3.0, 0.5, 0.5)},
            {new Vec3(3.0, 0.5, 0.5), new Vec3(-1.0, 0.5, 0.5)},
            {new Vec3(0.5, -1.0, 0.5), new Vec3(0.5, 3.0, 0.5)},
            {new Vec3(0.5, 3.0, 0.5), new Vec3(0.5, -1.0, 0.5)},
            {new Vec3(0.5, 0.5, -1.0), new Vec3(0.5, 0.5, 3.0)},
            {new Vec3(0.5, 0.5, 3.0), new Vec3(0.5, 0.5, -1.0)},
            {new Vec3(0.0, 0.5, 0.5), new Vec3(0.5, 0.5, 0.5)},
            {new Vec3(0.99999999, 0.5, 0.5), new Vec3(1.00000004, 0.5, 0.5)},
            {new Vec3(0.0, 64.75, 0.0), new Vec3(8.0, 64.75, 0.0)},
            {new Vec3(1.4, 64.75, 0.0), new Vec3(2.0, 65.9, 0.0)},
            {new Vec3(-2.0, -2.0, -2.0), new Vec3(4.0, 4.0, 4.0)},
        };
        out.append("  \"aabbClip\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (int b = 0; b < boxes.length; b++) {
            for (int s = 0; s < segs.length; s++) {
                Optional<Vec3> hit = boxes[b].clip(segs[s][0], segs[s][1]);
                rows.append(n++ == 0 ? "" : ",\n")
                    .append("    {\"box\": ").append(b).append(", \"seg\": ").append(s)
                    .append(", \"hit\": ").append(hit.isPresent());
                if (hit.isPresent()) {
                    Vec3 v = hit.get();
                    rows.append(", \"x\": ").append(db(v.x))
                        .append(", \"y\": ").append(db(v.y))
                        .append(", \"z\": ").append(db(v.z));
                }
                rows.append("}");
            }
        }
        out.append(rows).append("\n  ],\n");

        out.append("  \"aabbBoxes\": [\n");
        rows = new StringBuilder();
        for (int b = 0; b < boxes.length; b++) {
            Vec3 c = boxes[b].getCenter();
            rows.append(b == 0 ? "" : ",\n")
                .append("    {\"minX\": ").append(db(boxes[b].minX))
                .append(", \"minY\": ").append(db(boxes[b].minY))
                .append(", \"minZ\": ").append(db(boxes[b].minZ))
                .append(", \"maxX\": ").append(db(boxes[b].maxX))
                .append(", \"maxY\": ").append(db(boxes[b].maxY))
                .append(", \"maxZ\": ").append(db(boxes[b].maxZ))
                .append(", \"cx\": ").append(db(c.x))
                .append(", \"cy\": ").append(db(c.y))
                .append(", \"cz\": ").append(db(c.z))
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        out.append("  \"aabbSegments\": [\n");
        rows = new StringBuilder();
        for (int s = 0; s < segs.length; s++) {
            rows.append(s == 0 ? "" : ",\n")
                .append("    {\"fx\": ").append(db(segs[s][0].x))
                .append(", \"fy\": ").append(db(segs[s][0].y))
                .append(", \"fz\": ").append(db(segs[s][0].z))
                .append(", \"tx\": ").append(db(segs[s][1].x))
                .append(", \"ty\": ").append(db(segs[s][1].y))
                .append(", \"tz\": ").append(db(segs[s][1].z))
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        // --- VoxelShape.clip --------------------------------------------------
        //
        // A full block, a bottom slab, and a two-part shape whose interior
        // faces are what makes a merged box list and an occupied-cell list
        // different objects. The occupied-cell claim in
        // `crates/rustmc-server/src/projectile/clip.rs` is exactly what these
        // rows can falsify.
        VoxelShape[] shapes = {
            Shapes.block(),
            Shapes.box(0.0, 0.0, 0.0, 1.0, 0.5, 1.0),
            Shapes.or(Shapes.box(0.0, 0.0, 0.0, 0.25, 1.0, 1.0),
                      Shapes.box(0.75, 0.0, 0.0, 1.0, 1.0, 1.0)),
            Shapes.box(0.375, 0.0, 0.375, 0.625, 1.0, 0.625),
        };
        BlockPos[] shapePositions = {
            new BlockPos(0, 0, 0), new BlockPos(3, 64, -2),
        };
        out.append("  \"voxelClip\": [\n");
        rows = new StringBuilder();
        n = 0;
        for (int sh = 0; sh < shapes.length; sh++) {
            for (int p = 0; p < shapePositions.length; p++) {
                BlockPos bp = shapePositions[p];
                for (int s = 0; s < segs.length; s++) {
                    Vec3 from = segs[s][0].add(bp.getX(), bp.getY(), bp.getZ());
                    Vec3 to = segs[s][1].add(bp.getX(), bp.getY(), bp.getZ());
                    BlockHitResult r = shapes[sh].clip(from, to, bp);
                    rows.append(n++ == 0 ? "" : ",\n")
                        .append("    {\"shape\": ").append(sh)
                        .append(", \"pos\": ").append(p)
                        .append(", \"seg\": ").append(s)
                        .append(", \"hit\": ").append(r != null);
                    if (r != null) {
                        rows.append(", \"x\": ").append(db(r.getLocation().x))
                            .append(", \"y\": ").append(db(r.getLocation().y))
                            .append(", \"z\": ").append(db(r.getLocation().z))
                            .append(", \"dir\": \"").append(r.getDirection().getName())
                            .append("\", \"inside\": ").append(r.isInside());
                    }
                    rows.append("}");
                }
            }
        }
        out.append(rows).append("\n  ],\n");

        out.append("  \"voxelShapes\": [\n");
        rows = new StringBuilder();
        for (int sh = 0; sh < shapes.length; sh++) {
            StringBuilder aabbs = new StringBuilder();
            int m = 0;
            for (AABB a : shapes[sh].toAabbs()) {
                aabbs.append(m++ == 0 ? "" : ", ")
                     .append("[").append(db(a.minX)).append(", ").append(db(a.minY))
                     .append(", ").append(db(a.minZ)).append(", ").append(db(a.maxX))
                     .append(", ").append(db(a.maxY)).append(", ").append(db(a.maxZ))
                     .append("]");
            }
            rows.append(sh == 0 ? "" : ",\n")
                .append("    {\"aabbs\": [").append(aabbs).append("]}");
        }
        out.append(rows).append("\n  ],\n");

        out.append("  \"shapePositions\": [\n");
        rows = new StringBuilder();
        for (int p = 0; p < shapePositions.length; p++) {
            rows.append(p == 0 ? "" : ",\n")
                .append("    {\"x\": ").append(shapePositions[p].getX())
                .append(", \"y\": ").append(shapePositions[p].getY())
                .append(", \"z\": ").append(shapePositions[p].getZ())
                .append("}");
        }
        out.append(rows).append("\n  ],\n");

        // --- BlockGetter.traverseBlocks --------------------------------------
        //
        // Public static and it takes lambdas, so the whole DDA walk is
        // recordable with no `Level` at all: the visitor records the position
        // and returns null (continue), and the miss function ends it.
        out.append("  \"traverse\": [\n");
        rows = new StringBuilder();
        n = 0;
        Vec3[][] walks = {
            {new Vec3(0.5, 64.5, 0.5), new Vec3(8.5, 64.5, 0.5)},
            {new Vec3(0.5, 64.5, 0.5), new Vec3(0.5, 64.5, 0.5)},
            {new Vec3(0.0, 64.0, 0.0), new Vec3(3.0, 67.0, 3.0)},
            {new Vec3(3.0, 67.0, 3.0), new Vec3(0.0, 64.0, 0.0)},
            {new Vec3(-0.5, 64.5, -0.5), new Vec3(2.5, 61.5, 2.5)},
            {new Vec3(0.5, 64.75, 0.5), new Vec3(0.5, 64.75, 5.5)},
            {new Vec3(1.4, 64.75, 0.0), new Vec3(2.0, 65.9, 0.0)},
            {new Vec3(0.999999, 64.5, 0.5), new Vec3(1.000001, 64.5, 0.5)},
            {new Vec3(-2.25, -1.25, -3.75), new Vec3(2.25, 1.25, 3.75)},
        };
        for (int w = 0; w < walks.length; w++) {
            List<BlockPos> seen = new ArrayList<>();
            BlockGetter.traverseBlocks(
                walks[w][0], walks[w][1], seen,
                (ctx, pos) -> { ctx.add(pos.immutable()); return null; },
                ctx -> Boolean.TRUE);
            StringBuilder cells = new StringBuilder();
            int m = 0;
            for (BlockPos bp : seen) {
                cells.append(m++ == 0 ? "" : ", ")
                     .append("[").append(bp.getX()).append(", ").append(bp.getY())
                     .append(", ").append(bp.getZ()).append("]");
            }
            rows.append(n++ == 0 ? "" : ",\n")
                .append("    {\"fx\": ").append(db(walks[w][0].x))
                .append(", \"fy\": ").append(db(walks[w][0].y))
                .append(", \"fz\": ").append(db(walks[w][0].z))
                .append(", \"tx\": ").append(db(walks[w][1].x))
                .append(", \"ty\": ").append(db(walks[w][1].y))
                .append(", \"tz\": ").append(db(walks[w][1].z))
                .append(", \"cells\": [").append(cells).append("]}");
        }
        out.append(rows).append("\n  ]\n");

        out.append("}\n");
        System.out.print(out);
    }
}
