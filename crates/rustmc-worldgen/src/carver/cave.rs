//! The cave carver — rooms and branching tunnels.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `CaveWorldCarver`, `NetherWorldCarver`.
//!
//! # Two random sources, and they are not the same one
//!
//! `carve` draws from the **shared** stream — the one the driver reseeded
//! per carver — to pick a cave's origin, its radius multipliers and its
//! floor level. It then draws a `nextLong()` from that stream and uses it to
//! seed a **fresh** `SingleThreadedRandomSource` for each tunnel.
//!
//! So a tunnel's whole shape is a function of one long, and tunnels are
//! independent of each other except through the order those longs were
//! drawn. Getting that split wrong — walking the tunnel on the shared
//! stream — produces caves that look right and are wrong everywhere.
//!
//! # A tunnel that splits does not continue
//!
//! At the split point it spawns two children and **returns**. It does not
//! carve the remaining steps. A port that lets the parent run on gets a
//! three-way junction where vanilla has a fork.

use super::carve::{can_reach, carve_ellipsoid, CarveBounds, CarveTarget};
use super::config::CaveConfig;
use super::mask::CarvingMask;
use crate::math::{cos, sin};
use crate::rng::{LegacyRandomSource, RandomSource, WorldgenRandom};
use crate::tags::BlockTags;

/// `WorldCarver#getRange` — how far, in chunks, a carver reaches.
pub const RANGE: i32 = 4;

/// Which flavour of the cave carver: the two differ only in four constants,
/// but each of them changes the shape of every cave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaveKind {
    Overworld,
    Nether,
}

impl CaveKind {
    /// `getCaveBound` — the ceiling on the triple-nested cave count draw.
    const fn cave_bound(self) -> i32 {
        match self {
            Self::Overworld => 15,
            Self::Nether => 10,
        }
    }

    /// `getYScale`.
    const fn y_scale(self) -> f64 {
        match self {
            Self::Overworld => 1.0,
            Self::Nether => 5.0,
        }
    }

    /// `getThickness`.
    ///
    /// The overworld's has a one-in-ten chance of a third draw; the nether's
    /// never does. That difference is a **draw count** difference, so it
    /// shifts every value after it in the same carve, not just the
    /// thickness.
    fn thickness<R: RandomSource>(self, random: &mut R) -> f32 {
        match self {
            Self::Overworld => {
                let mut thickness = random.next_f32() * 2.0 + random.next_f32();
                if random.next_i32_bound(10) == 0 {
                    thickness *= random.next_f32() * random.next_f32() * 3.0 + 1.0;
                }
                thickness
            }
            Self::Nether => (random.next_f32() * 2.0 + random.next_f32()) * 2.0,
        }
    }
}

/// `CaveWorldCarver#carve`.
///
/// `random` is the driver's shared stream, already reseeded for this carver
/// and this source chunk.
#[allow(clippy::too_many_arguments)]
pub fn carve<T: CarveTarget, R: RandomSource>(
    target: &mut T,
    tags: &BlockTags,
    config: &CaveConfig,
    kind: CaveKind,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    random: &mut R,
    source_chunk_x: i32,
    source_chunk_z: i32,
) {
    // `SectionPos.sectionToBlockCoord(getRange() * 2 - 1)` — seven sections,
    // 112 blocks.
    let max_distance = (RANGE * 2 - 1) * 16;
    // `NetherWorldCarver` overrides `carveBlock` and never reads
    // `config.lavaLevel`; its lava line is the literal `getMinGenY() + 31`.
    // `carvers_nether.json` still *says* `lava_level: {above_bottom: 10}`,
    // and using it would put the line 21 blocks too low — a value that is
    // parsed and then deliberately ignored is exactly the kind of thing a
    // port gets wrong by being tidy.
    let style = match kind {
        CaveKind::Overworld => super::carve::CarveStyle::Standard {
            lava_level_y: config
                .base
                .lava_level
                .resolve(bounds.min_gen_y, bounds.gen_depth),
        },
        CaveKind::Nether => super::carve::CarveStyle::Nether {
            lava_at_or_below: bounds.min_gen_y + 31,
        },
    };

    // Three nested draws, and the nesting is the distribution: most chunks
    // get zero or one cave and a few get many. Flattening it to a single
    // `nextInt` would both change the shape and consume two fewer draws.
    let cave_count = {
        let a = random.next_i32_bound(kind.cave_bound()) + 1;
        let b = random.next_i32_bound(a) + 1;
        random.next_i32_bound(b)
    };

    for _ in 0..cave_count {
        let x = f64::from(source_chunk_x * 16 + random.next_i32_bound(16));
        let y = f64::from(
            config
                .base
                .y
                .sample(random, bounds.min_gen_y, bounds.gen_depth),
        );
        let z = f64::from(source_chunk_z * 16 + random.next_i32_bound(16));
        let horizontal_radius_multiplier =
            f64::from(config.horizontal_radius_multiplier.sample(random));
        let vertical_radius_multiplier =
            f64::from(config.vertical_radius_multiplier.sample(random));
        let floor_level = f64::from(config.floor_level.sample(random));

        let mut tunnels = 1;
        if random.next_i32_bound(4) == 0 {
            let y_scale = f64::from(config.base.y_scale.sample(random));
            let thickness = 1.0 + random.next_f32() * 6.0;
            create_room(
                target,
                tags,
                config,
                style,
                bounds,
                mask,
                x,
                y,
                z,
                thickness,
                y_scale,
                floor_level,
            );
            tunnels += random.next_i32_bound(4);
        }

        for _ in 0..tunnels {
            let horizontal_rotation = random.next_f32() * std::f32::consts::PI * 2.0;
            let vertical_rotation = (random.next_f32() - 0.5) / 4.0;
            let thickness = kind.thickness(random);
            let distance = max_distance - random.next_i32_bound(max_distance / 4);
            // The tunnel gets its own stream, seeded from one long off the
            // shared one. See the module docs.
            let tunnel_seed = random.next_i64();
            create_tunnel(
                target,
                tags,
                config,
                style,
                bounds,
                mask,
                tunnel_seed,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                thickness,
                horizontal_rotation,
                vertical_rotation,
                0,
                distance,
                kind.y_scale(),
                floor_level,
            );
        }
    }
}

/// `CaveWorldCarver#createRoom`.
#[allow(clippy::too_many_arguments)]
fn create_room<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    config: &CaveConfig,
    style: super::carve::CarveStyle,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    x: f64,
    y: f64,
    z: f64,
    thickness: f32,
    y_scale: f64,
    floor_level: f64,
) {
    // `Mth.sin((float)(Math.PI / 2))`. The argument is cast to `float`
    // *before* being widened for the 26.2 `sin(double)`, and that changes
    // which table entry is selected — 16384 rather than 16383. Here both
    // entries round to 1.0f because sine is quadratically flat at its peak,
    // so this particular constant is 1.0 either way; the cast is kept
    // because the next angle along will not be so forgiving.
    let horizontal_radius =
        1.5 + f64::from(sin(f64::from(std::f32::consts::FRAC_PI_2))) * f64::from(thickness);
    let vertical_radius = horizontal_radius * y_scale;
    // The `x + 1.0` is vanilla's, and looks like a bug. It is behaviour.
    carve_ellipsoid(
        target,
        tags,
        &config.base.replaceable,
        style,
        bounds,
        mask,
        x + 1.0,
        y,
        z,
        horizontal_radius,
        vertical_radius,
        &move |xd, yd, zd, _| should_skip(xd, yd, zd, floor_level),
    );
}

/// `CaveWorldCarver#createTunnel`.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn create_tunnel<T: CarveTarget>(
    target: &mut T,
    tags: &BlockTags,
    config: &CaveConfig,
    style: super::carve::CarveStyle,
    bounds: CarveBounds,
    mask: &mut CarvingMask,
    tunnel_seed: i64,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    horizontal_radius_multiplier: f64,
    vertical_radius_multiplier: f64,
    thickness: f32,
    mut horizontal_rotation: f32,
    mut vertical_rotation: f32,
    step: i32,
    dist: i32,
    y_scale: f64,
    floor_level: f64,
) {
    // `RandomSource.createThreadLocalInstance(seed)` is a
    // `SingleThreadedRandomSource` — the same legacy LCG as
    // `LegacyRandomSource`, minus the atomics.
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(tunnel_seed));
    let split_point = random.next_i32_bound(dist / 2) + dist / 4;
    let steep = random.next_i32_bound(6) == 0;
    let mut y_rota = 0.0f32;
    let mut x_rota = 0.0f32;

    for current_step in step..dist {
        #[allow(clippy::cast_precision_loss)]
        let angle = std::f32::consts::PI * current_step as f32 / dist as f32;
        let horizontal_radius = 1.5 + f64::from(sin(f64::from(angle))) * f64::from(thickness);
        let vertical_radius = horizontal_radius * y_scale;
        let cos_x = cos(f64::from(vertical_rotation));
        x += f64::from(cos(f64::from(horizontal_rotation))) * f64::from(cos_x);
        y += f64::from(sin(f64::from(vertical_rotation)));
        z += f64::from(sin(f64::from(horizontal_rotation))) * f64::from(cos_x);
        vertical_rotation *= if steep { 0.92 } else { 0.7 };
        vertical_rotation += x_rota * 0.1;
        horizontal_rotation += y_rota * 0.1;
        x_rota *= 0.9;
        y_rota *= 0.75;
        x_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 2.0;
        y_rota += (random.next_f32() - random.next_f32()) * random.next_f32() * 4.0;

        if current_step == split_point && thickness > 1.0 {
            // Two children, then **return** — the parent does not carve its
            // remaining steps.
            for side in [-std::f32::consts::FRAC_PI_2, std::f32::consts::FRAC_PI_2] {
                let child_seed = random.next_i64();
                let child_thickness = random.next_f32() * 0.5 + 0.5;
                create_tunnel(
                    target,
                    tags,
                    config,
                    style,
                    bounds,
                    mask,
                    child_seed,
                    x,
                    y,
                    z,
                    horizontal_radius_multiplier,
                    vertical_radius_multiplier,
                    child_thickness,
                    horizontal_rotation + side,
                    vertical_rotation / 3.0,
                    current_step,
                    dist,
                    1.0,
                    floor_level,
                );
            }
            return;
        }

        if random.next_i32_bound(4) != 0 {
            if !can_reach(bounds, x, z, current_step, dist, thickness) {
                return;
            }
            carve_ellipsoid(
                target,
                tags,
                &config.base.replaceable,
                style,
                bounds,
                mask,
                x,
                y,
                z,
                horizontal_radius * horizontal_radius_multiplier,
                vertical_radius * vertical_radius_multiplier,
                &move |xd, yd, zd, _| should_skip(xd, yd, zd, floor_level),
            );
        }
    }
}

/// `CaveWorldCarver#shouldSkip`.
///
/// The `floorLevel` cut is what gives caves a flat bottom instead of a round
/// one: everything at or below it in the normalised ellipsoid is skipped.
fn should_skip(xd: f64, yd: f64, zd: f64, floor_level: f64) -> bool {
    if yd <= floor_level {
        return true;
    }
    xd * xd + yd * yd + zd * zd >= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random(seed: i64) -> WorldgenRandom<LegacyRandomSource> {
        WorldgenRandom::new(LegacyRandomSource::new(seed))
    }

    #[test]
    fn the_floor_cut_is_what_flattens_a_cave() {
        // Without it a cave is a plain ellipsoid. With it, everything below
        // the floor level is skipped, which is why you can walk in one.
        let below_floor = should_skip(0.0, -0.9, 0.0, -0.5);
        let above_floor = should_skip(0.0, -0.1, 0.0, -0.5);
        assert!(below_floor, "below the floor should be skipped");
        assert!(!above_floor, "above the floor should be carved");
        // And outside the unit sphere is skipped regardless.
        assert!(should_skip(0.9, 0.9, 0.0, -1.0));
    }

    #[test]
    fn the_nether_thickness_never_takes_a_third_draw() {
        // The overworld's has a one-in-ten chance of a third draw, so the
        // two kinds consume different numbers of values from the same
        // stream — a difference that moves everything after it, not just the
        // thickness.
        let mut overworld_extra = 0;
        for seed in 0..200i64 {
            let mut r = random(seed);
            let before = r.count();
            let _ = CaveKind::Overworld.thickness(&mut r);
            if r.count() - before > 3 {
                overworld_extra += 1;
            }

            let mut r = random(seed);
            let before = r.count();
            let _ = CaveKind::Nether.thickness(&mut r);
            assert_eq!(r.count() - before, 2, "nether thickness took extra draws");
        }
        assert!(
            overworld_extra > 0,
            "the overworld's extra draw never fired in 200 seeds"
        );
    }

    #[test]
    fn the_nether_carver_differs_in_every_constant_that_shapes_a_cave() {
        assert_eq!(CaveKind::Overworld.cave_bound(), 15);
        assert_eq!(CaveKind::Nether.cave_bound(), 10);
        assert!((CaveKind::Overworld.y_scale() - 1.0).abs() < f64::EPSILON);
        assert!((CaveKind::Nether.y_scale() - 5.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_room_radius_constant_is_one_but_the_cast_still_matters() {
        // Written first asserting the table value was *below* 1.0, on the
        // reasoning that a quantised angle cannot land exactly on the peak.
        // It does: sine is quadratically flat there, so both candidate
        // entries round to 1.0f. The constant is 1.0.
        assert!((sin(f64::from(std::f32::consts::FRAC_PI_2)) - 1.0).abs() < f32::EPSILON);
        assert!((sin(std::f64::consts::FRAC_PI_2) - 1.0).abs() < f32::EPSILON);

        // The cast is still not decoration: it selects a different table
        // entry, and away from the peak that is visible. Somewhere in a
        // sweep of ordinary angles the float-cast and double arguments must
        // disagree, or `Mth.sin` taking a double in 26.2 would be a
        // distinction without a difference.
        let differing = (1..20_000)
            .filter(|&i| {
                let angle = f64::from(i) * 0.001;
                #[allow(clippy::cast_possible_truncation)]
                let as_float = f64::from(angle as f32);
                sin(as_float) != sin(angle)
            })
            .count();
        assert!(
            differing > 0,
            "float-cast and double angles never select different table entries"
        );
    }

    #[test]
    fn the_cave_count_draw_is_nested_three_deep() {
        // The nesting is the distribution: most chunks get no caves and a
        // few get several. A flat `nextInt(15)` would give a uniform spread
        // and consume two fewer draws.
        let mut counts = [0usize; 16];
        for seed in 0..2000i64 {
            let mut r = random(seed);
            let a = r.next_i32_bound(15) + 1;
            let b = r.next_i32_bound(a) + 1;
            let count = r.next_i32_bound(b);
            #[allow(clippy::cast_sign_loss)]
            {
                counts[count as usize] += 1;
            }
        }
        assert!(
            counts[0] > counts[1] && counts[1] > counts[3],
            "the distribution should fall off sharply: {counts:?}"
        );
        assert!(
            counts[0] * 3 > counts.iter().sum::<usize>(),
            "zero should dominate"
        );
    }
}
