//! The icebergs of a frozen ocean — the *feature*, not the surface rule.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `IcebergFeature`.
//!
//! # There are two icebergs in this codebase and they are different things
//!
//! [`crate::surface::iceberg`] is `SurfaceSystem#frozenOceanExtension`: a
//! per-column surface rule that raises packed ice out of the water during the
//! **surface** stage. This module is `minecraft:iceberg`, a **decoration**
//! feature, and the two are placed by different machinery at different times.
//! Having one is not having the other, and the census that opened this item
//! says so precisely: we wrote 921,068 `packed_ice` — so the surface rule was
//! working — and **zero** `blue_ice`, because nothing here ran at all.
//!
//! # One feature, two placed features
//!
//! `minecraft:iceberg` carries a `BlockStateConfiguration`, one block state,
//! and the extract wires it up twice:
//!
//! ```text
//! iceberg_packed   packed_ice   rarity_filter 16    in_square   biome
//! iceberg_blue     blue_ice     rarity_filter 200   in_square   biome
//! ```
//!
//! Both are in `frozen_ocean` and `deep_frozen_ocean` and nowhere else. So a
//! single missing arm in the configured-feature parser costs *both* blocks,
//! which is why the packed-ice shortfall and the blue-ice absence turned out
//! to be one defect and not the two the worklist predicted.
//!
//! # The origin is moved to sea level before anything else
//!
//! `place` throws away the y the placement chain chose and rebuilds the
//! origin at `chunkGenerator.getSeaLevel()`. Every offset below is relative to
//! that, so an iceberg's height above water is fixed by the feature and not by
//! where the `in_square` modifier happened to land.
//!
//! # The shape is drawn twice: a cap above and a keel below
//!
//! The first triple loop walks `y` upward from 0 and lays the cap; `smooth`
//! then erodes it; the second walks `y` downward from -1 and lays the keel.
//! Only after both does the cut-out run. The two halves use *different*
//! radius functions — `heightDependentRadiusRound`/`Ellipse` above and
//! `heightDependentRadiusSteep` below — and the round one draws, so the two
//! loops are not interchangeable even in the blocks they would write.
//!
//! # Where the draws are, because that is what a later feature inherits
//!
//! Unconditional, in this order: `nextDouble` (snow), `nextDouble` (angle),
//! `nextInt(5)`, `nextInt(3)`, `nextDouble` (ellipse), then the height draw.
//! After that the stream depends on the shape, and three places consume
//! conditionally in a way a careless port gets wrong:
//!
//! * `signedDistanceCircle` draws a `nextFloat` **per block tested**, but only
//!   on the non-ellipse arm — the ellipse arm is deterministic given the
//!   angle. A port that hoisted the distance call out of the branch would
//!   burn one draw per tested block on every ellipse iceberg.
//! * `heightDependentRadiusRound` draws `nextFloat` and `nextInt(5)`
//!   unconditionally and `nextInt(6)` only when the iceberg is tall. The
//!   `nextInt(5)` is on the right of a comparison, so it is drawn even when
//!   the branch is not taken.
//! * the keel loop in `generateCutOut` re-evaluates `-l + nextInt(5)` **every
//!   time it tests the loop condition**, including the test that ends it. The
//!   loop therefore costs one more draw than it has iterations.
//!
//! # `setIcebergBlock` computes its snow coin before it knows it needs it
//!
//! The `nextDouble() > 0.05` guard is evaluated on the ellipse arm whether or
//! not the block ends up snow, and before the `snowy` test that can discard
//! the whole question. Only the `nextInt` inside the height comparison is
//! conditional. Getting this backwards leaves the stream short on exactly the
//! icebergs that place no snow.

use super::block_predicate::PredicateWorld;
use crate::rng::RandomSource;

/// The world this feature reads and writes.
pub trait IcebergWorld: PredicateWorld {
    /// `Feature#setBlock` — `level.setBlock(pos, state, 3)`, with no
    /// replaceability test. The feature does its own filtering.
    fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str);
    /// `ChunkGenerator#getSeaLevel`.
    fn sea_level(&mut self) -> i32;
}

/// The world could not answer a question this feature has to ask.
#[derive(Debug, PartialEq, Eq)]
pub struct NeedsRules(pub String);

/// `RUSTMC_TRACE_ICEBERG=1` prints one line per shape decision, so a draw
/// trace that diverges at ordinal N can be attributed to a **position** and a
/// **branch** rather than to a method.
///
/// A draw trace says the streams parted at draw 6,866; it cannot say which of
/// the two `nextDouble` sites in this file made the extra draw, and the two
/// have different fixes — one is the fringe test in
/// [`generate_iceberg_block`], the other is the snow coin in
/// [`set_iceberg_block`], which is gated on the block already in the world.
/// The closure is not evaluated when the switch is off, so this costs a
/// thread-local read per call on the real generator's path and nothing else.
fn trace_iceberg(line: impl FnOnce() -> String) {
    thread_local! {
        static ON: bool = std::env::var_os("RUSTMC_TRACE_ICEBERG").is_some();
    }
    if ON.with(|on| *on) {
        eprintln!("ICEBERG {}", line());
    }
}

/// `BlockState#isAir` — true for cave air and void air too, which is why it
/// is [`PredicateWorld::air_at`] and not a name comparison. An unanswerable
/// read refuses rather than defaulting: a silent `false` here would erode
/// nothing in `smooth` and look like a correctly shaped iceberg.
fn is_air<W: IcebergWorld + ?Sized>(
    world: &mut W,
    x: i32,
    y: i32,
    z: i32,
) -> Result<bool, NeedsRules> {
    world
        .air_at(x, y, z)
        .ok_or_else(|| NeedsRules("iceberg: the world cannot answer `isAir`".to_string()))
}

/// `IcebergFeature#place`. Returns whether anything was placed — vanilla
/// returns `true` unconditionally once it starts.
///
/// # Errors
/// If the world cannot answer `isAir` for a position the feature reads.
pub fn place<W: IcebergWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
    state: &str,
) -> Result<bool, NeedsRules> {
    let sea_level = world.sea_level();
    let origin = (origin.0, sea_level, origin.2);

    let snowy = random.next_f64() > 0.7;
    let angle = random.next_f64() * 2.0 * std::f64::consts::PI;
    // `11 - nextInt(5)`, the ellipse's major half-width and the smoothing
    // radius. Not the same 11 as the round arm's fixed half-width below.
    let ellipse_half = 11 - random.next_i32_bound(5);
    let taper = 3 + random.next_i32_bound(3);
    let ellipse = random.next_f64() > 0.7;

    let mut height = if ellipse {
        random.next_i32_bound(6) + 6
    } else {
        random.next_i32_bound(15) + 3
    };
    // One iceberg in ten on the round arm is a spire. The draw is conditional
    // on `!ellipse` first, so an ellipse never consumes it.
    if !ellipse && random.next_f64() > 0.9 {
        height += random.next_i32_bound(19) + 7;
    }

    let depth = (height + random.next_i32_bound(11)).min(18);
    // Evaluated left to right: the `nextInt(7)` is drawn before the
    // `nextInt(5)`, and swapping them silently reshapes every iceberg.
    let spread = {
        let up = random.next_i32_bound(7);
        let down = random.next_i32_bound(5);
        (height + up - down).min(11)
    };
    let half = if ellipse { ellipse_half } else { 11 };

    // The cap. `x` outermost, then `z`, then `y` — the order matters because
    // the round arm draws inside the innermost loop.
    for x in -half..half {
        for z in -half..half {
            for y in 0..height {
                let radius = if ellipse {
                    height_dependent_radius_ellipse(y, height, spread)
                } else {
                    height_dependent_radius_round(random, y, height, spread)
                };
                // The ellipse arm ignores the radius here: its shape comes
                // from `signedDistanceEllipse` instead.
                if ellipse || x < radius {
                    generate_iceberg_block(
                        world, random, origin, height, x, y, z, radius, half, ellipse, taper,
                        angle, snowy, state,
                    )?;
                }
            }
        }
    }

    smooth(world, origin, spread, height, ellipse, ellipse_half)?;

    // The keel. `y` runs -1 downward and the half-width tapers quadratically
    // on the ellipse arm.
    for x in -half..half {
        for z in -half..half {
            let mut y = -1;
            while y > -depth {
                let taper_half = if ellipse {
                    mth_ceil(half as f32 * (1.0 - square_f32(y) / (depth as f32 * 8.0)))
                } else {
                    half
                };
                let radius = height_dependent_radius_steep(random, -y, depth, spread);
                if x < radius {
                    generate_iceberg_block(
                        world, random, origin, depth, x, y, z, radius, taper_half, ellipse, taper,
                        angle, snowy, state,
                    )?;
                }
                y -= 1;
            }
        }
    }

    // Nine ellipses in ten are cut, but only three round icebergs in ten.
    let cut = if ellipse {
        random.next_f64() > 0.1
    } else {
        random.next_f64() > 0.7
    };
    if cut {
        generate_cut_out(
            world,
            random,
            spread,
            height,
            origin,
            ellipse,
            ellipse_half,
            angle,
            taper,
        );
    }
    Ok(true)
}

/// `IcebergFeature#generateCutOut` — the notch a grounded berg carries.
#[allow(clippy::too_many_arguments)]
fn generate_cut_out<W: IcebergWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn RandomSource,
    spread: i32,
    height: i32,
    origin: (i32, i32, i32),
    ellipse: bool,
    ellipse_half: i32,
    angle: f64,
    taper: i32,
) {
    let sign_x = if random.next_bool() { -1 } else { 1 };
    let sign_z = if random.next_bool() { -1 } else { 1 };

    // Both offsets draw twice on the `nextBoolean` arm and once otherwise,
    // and the ellipse arm then throws both away and draws a third time. Every
    // one of those draws happens, discarded or not.
    let mut off_x = random.next_i32_bound((spread / 2 - 2).max(1));
    if random.next_bool() {
        off_x = spread / 2 + 1 - random.next_i32_bound((spread - spread / 2 - 1).max(1));
    }
    let mut off_z = random.next_i32_bound((spread / 2 - 2).max(1));
    if random.next_bool() {
        off_z = spread / 2 + 1 - random.next_i32_bound((spread - spread / 2 - 1).max(1));
    }
    if ellipse {
        let both = random.next_i32_bound((ellipse_half - 5).max(1));
        off_x = both;
        off_z = both;
    }

    let offset = (sign_x * off_x, sign_z * off_z);
    // The cut on an ellipse is square to the berg; on a round one it points
    // anywhere.
    let cut_angle = if ellipse {
        angle + std::f64::consts::FRAC_PI_2
    } else {
        random.next_f64() * 2.0 * std::f64::consts::PI
    };

    for y in 0..(height - 3) {
        let radius = height_dependent_radius_round(random, y, height, spread);
        carve(
            world,
            radius,
            y,
            origin,
            false,
            cut_angle,
            offset,
            ellipse_half,
            taper,
        );
    }
    // `-height + nextInt(5)` is the loop condition, so the draw happens on
    // every test — one more than the number of iterations.
    let mut y = -1;
    while y > -height + random.next_i32_bound(5) {
        let radius = height_dependent_radius_steep(random, -y, height, spread);
        carve(
            world,
            radius,
            y,
            origin,
            true,
            cut_angle,
            offset,
            ellipse_half,
            taper,
        );
        y -= 1;
    }
}

/// `IcebergFeature#carve` — one horizontal slice of the cut-out.
///
/// Above the waterline the notch is air and any snow layer resting on the
/// removed block goes with it; below, it fills with water instead.
#[allow(clippy::too_many_arguments)]
fn carve<W: IcebergWorld + ?Sized>(
    world: &mut W,
    radius: i32,
    y: i32,
    origin: (i32, i32, i32),
    flooded: bool,
    angle: f64,
    offset: (i32, i32),
    ellipse_half: i32,
    taper: i32,
) {
    let major = radius + 1 + ellipse_half / 3;
    let minor = (radius - 3).min(3) + taper / 2 - 1;
    for x in -major..major {
        for z in -major..major {
            // A POSITIVE `< 0.0` WRAPPING THE BODY, as vanilla spells it —
            // never an inverted `>= 0.0` skip. See `generate_iceberg_block`
            // for why the two differ and why `clippy` pushes towards the wrong
            // one. `minor` here is `min(radius - 3, 3) + taper / 2 - 1` and
            // reaches **zero** on the narrow slices at the ends of the cut, so
            // the column through the notch's own centre divides 0.0 by 0.0.
            if signed_distance_ellipse(x, z, offset, major, minor, angle) < 0.0 {
                let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                let block = world.block_at(pos.0, pos.1, pos.2);
                let name = plain(&block);
                // `SNOW_BLOCK` is already an iceberg state; vanilla tests it
                // twice and so does this.
                if is_iceberg_state(name) || name == "minecraft:snow_block" {
                    if flooded {
                        world.set_block(pos.0, pos.1, pos.2, "minecraft:water[level=0]");
                    } else {
                        world.set_block(pos.0, pos.1, pos.2, "minecraft:air");
                        remove_floating_snow_layer(world, pos);
                    }
                }
            }
        }
    }
}

/// `IcebergFeature#removeFloatingSnowLayer`.
fn remove_floating_snow_layer<W: IcebergWorld + ?Sized>(world: &mut W, pos: (i32, i32, i32)) {
    let above = world.block_at(pos.0, pos.1 + 1, pos.2);
    if plain(&above) == "minecraft:snow" {
        world.set_block(pos.0, pos.1 + 1, pos.2, "minecraft:air");
    }
}

/// `IcebergFeature#generateIcebergBlock` — the per-position test.
#[allow(clippy::too_many_arguments)]
fn generate_iceberg_block<W: IcebergWorld + ?Sized>(
    world: &mut W,
    random: &mut dyn RandomSource,
    origin: (i32, i32, i32),
    height: i32,
    x: i32,
    y: i32,
    z: i32,
    radius: i32,
    half: i32,
    ellipse: bool,
    taper: i32,
    angle: f64,
    snowy: bool,
    state: &str,
) -> Result<(), NeedsRules> {
    // The circle arm draws; the ellipse arm does not. Keep the call inside
    // the branch.
    let distance = if ellipse {
        signed_distance_ellipse(x, z, (0, 0), half, get_ellipse_c(y, height, taper), angle)
    } else {
        signed_distance_circle(x, z, (0, 0), radius, random)
    };
    // KEEP THIS AS A POSITIVE `< 0.0` WRAPPING THE BODY, exactly as vanilla
    // spells it — `if (d1 < 0.0) { ...body... }` — and do not "simplify" it
    // into an early `if (d1 >= 0.0) { return; }`.
    //
    // The two spellings differ on exactly one value: **NaN**, which fails `<`
    // and `>=` alike. Java skips the body; an inverted guard falls into it.
    // NaN is not hypothetical here. `signedDistanceEllipse` divides by its
    // `major` and `minor` axes, and the keel's major is
    // `Mth.ceil(half * (1 - y^2 / (depth * 8)))`, which passes through **zero**
    // on the slice where `y^2` crosses `depth * 8`. At the berg's own centre
    // column `dx == dz == 0`, so that slice evaluates `0.0 / 0.0`.
    //
    // **The negated form `!(d1 < 0.0)` is correct and `clippy` rejects it**
    // (`neg_cmp_op_on_partial_ord`, `-D warnings`), which is the likeliest way
    // the original inversion was born: write the faithful guard, get told off
    // by the lint, and "fix" it to `>=`. Mirroring vanilla's own nesting
    // satisfies the lint without an `allow`, and leaves no negated comparison
    // for anyone to be advised to rewrite.
    if distance < 0.0 {
        let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
        // The fringe: within `edge` of the boundary, one block in ten is
        // dropped so the silhouette is not a clean curve. The `nextInt(3)` is
        // inside the `distance < 0` test and the `nextDouble` inside the
        // fringe test.
        let edge = if ellipse {
            -0.5
        } else {
            f64::from(-6 - random.next_i32_bound(3))
        };
        trace_iceberg(|| {
            format!(
                "gib {},{},{} d={distance} edge={edge} fringe={}",
                pos.0,
                pos.1,
                pos.2,
                distance > edge
            )
        });
        if distance > edge && random.next_f64() > 0.9 {
            return Ok(());
        }
        return set_iceberg_block(
            world,
            pos,
            random,
            height - y,
            height,
            ellipse,
            snowy,
            state,
        );
    }
    Ok(())
}

/// `IcebergFeature#setIcebergBlock` — ice, or the snow cap on top of it.
#[allow(clippy::too_many_arguments)]
fn set_iceberg_block<W: IcebergWorld + ?Sized>(
    world: &mut W,
    pos: (i32, i32, i32),
    random: &mut dyn RandomSource,
    below_top: i32,
    height: i32,
    ellipse: bool,
    snowy: bool,
    state: &str,
) -> Result<(), NeedsRules> {
    let block = world.block_at(pos.0, pos.1, pos.2);
    let name = plain(&block).to_string();
    let is_water = name == "minecraft:water";
    let pass = is_air(world, pos.0, pos.1, pos.2)?
        || name == "minecraft:snow_block"
        || name == "minecraft:ice"
        || is_water;
    trace_iceberg(|| format!("sib {},{},{} block={name} pass={pass}", pos.0, pos.1, pos.2));
    if !pass {
        return Ok(());
    }

    // DRAWN BEFORE THE `snowy` TEST, not inside it. See the module docs.
    let coin = !ellipse || random.next_f64() > 0.05;
    let divisor = if ellipse { 3 } else { 2 };

    let capped = snowy
        && !is_water
        && f64::from(below_top)
            <= f64::from(random.next_i32_bound((height / divisor).max(1)))
                + f64::from(height) * 0.6
        && coin;
    if capped {
        world.set_block(pos.0, pos.1, pos.2, "minecraft:snow_block");
    } else {
        world.set_block(pos.0, pos.1, pos.2, state);
    }
    Ok(())
}

/// `IcebergFeature#smooth` — erode overhangs and blocks with nothing holding
/// them.
fn smooth<W: IcebergWorld + ?Sized>(
    world: &mut W,
    origin: (i32, i32, i32),
    spread: i32,
    height: i32,
    ellipse: bool,
    ellipse_half: i32,
) -> Result<(), NeedsRules> {
    let reach = if ellipse { ellipse_half } else { spread / 2 };
    // `spread` can be negative — `height + nextInt(7) - nextInt(5)` bottoms
    // out at -1 — and then this walks nothing, exactly as the Java `for` does.
    for x in -reach..=reach {
        for z in -reach..=reach {
            for y in 0..=height {
                let pos = (origin.0 + x, origin.1 + y, origin.2 + z);
                let block = world.block_at(pos.0, pos.1, pos.2);
                let name = plain(&block).to_string();
                let iceberg = is_iceberg_state(&name);
                if !(iceberg || name == "minecraft:snow") {
                    continue;
                }
                if is_air(world, pos.0, pos.1 - 1, pos.2)? {
                    // Nothing underneath: this block and whatever sat on it.
                    world.set_block(pos.0, pos.1, pos.2, "minecraft:air");
                    world.set_block(pos.0, pos.1 + 1, pos.2, "minecraft:air");
                } else if iceberg {
                    // `west, east, north, south` — the horizontal faces only,
                    // so a one-block spike survives if it is standing on
                    // something.
                    let mut bare = 0;
                    for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                        let side = world.block_at(pos.0 + dx, pos.1, pos.2 + dz);
                        if !is_iceberg_state(plain(&side)) {
                            bare += 1;
                        }
                    }
                    if bare >= 3 {
                        world.set_block(pos.0, pos.1, pos.2, "minecraft:air");
                    }
                }
            }
        }
    }
    Ok(())
}

/// `IcebergFeature#getEllipseC` — the minor axis, pinched in over the top
/// three layers so the cap is domed rather than cut flat.
fn get_ellipse_c(y: i32, height: i32, taper: i32) -> i32 {
    let mut c = taper;
    if y > 0 && height - y <= 3 {
        c -= 4 - (height - y);
    }
    c
}

/// `IcebergFeature#signedDistanceCircle`.
///
/// The `nextFloat` is the roughness: it perturbs the radius by up to a tenth
/// per block tested, which is what stops a round iceberg from being a
/// cylinder.
fn signed_distance_circle(
    x: i32,
    z: i32,
    centre: (i32, i32),
    radius: i32,
    random: &mut dyn RandomSource,
) -> f64 {
    let jitter = 10.0 * mth_clamp(random.next_f32(), 0.2, 0.8) / radius as f32;
    let dx = f64::from(x - centre.0);
    let dz = f64::from(z - centre.1);
    let r = f64::from(radius);
    f64::from(jitter) + dx * dx + dz * dz - r * r
}

/// `IcebergFeature#signedDistanceEllipse` — an axis-aligned ellipse test in a
/// frame rotated by `angle`.
fn signed_distance_ellipse(
    x: i32,
    z: i32,
    centre: (i32, i32),
    major: i32,
    minor: i32,
    angle: f64,
) -> f64 {
    // `Math.cos`/`Math.sin`, not `Mth.cos`/`Mth.sin` — the table would be
    // visibly coarse at this scale. Same call as `ore.rs` makes; see its
    // module docs for why the 1-ulp slack is tolerable here.
    let dx = f64::from(x - centre.0);
    let dz = f64::from(z - centre.1);
    let along = (dx * angle.cos() - dz * angle.sin()) / f64::from(major);
    let across = (dx * angle.sin() + dz * angle.cos()) / f64::from(minor);
    along * along + across * across - 1.0
}

/// `IcebergFeature#heightDependentRadiusRound`.
fn height_dependent_radius_round(
    random: &mut dyn RandomSource,
    y: i32,
    height: i32,
    spread: i32,
) -> i32 {
    let softness = 3.5 - random.next_f32();
    let mut radius = (1.0 - square_f32(y) / (height as f32 * softness)) * spread as f32;
    // DRAWN EVERY CALL: the `nextInt(5)` is the right operand of the
    // comparison, so it is consumed whether or not the branch is taken.
    if height > 15 + random.next_i32_bound(5) {
        let scaled = if y < 3 + random.next_i32_bound(6) {
            y / 2
        } else {
            y
        };
        radius = (1.0 - scaled as f32 / (height as f32 * softness * 0.4)) * spread as f32;
    }
    mth_ceil(radius / 2.0)
}

/// `IcebergFeature#heightDependentRadiusEllipse`. Draws nothing.
fn height_dependent_radius_ellipse(y: i32, height: i32, spread: i32) -> i32 {
    // `(float) l * 1.0f` is vanilla's own redundant multiply, kept so the
    // float rounding is identical.
    let radius = (1.0 - square_f32(y) / (height as f32 * 1.0)) * spread as f32;
    mth_ceil(radius / 2.0)
}

/// `IcebergFeature#heightDependentRadiusSteep` — the keel, which narrows
/// linearly rather than quadratically.
fn height_dependent_radius_steep(
    random: &mut dyn RandomSource,
    y: i32,
    height: i32,
    spread: i32,
) -> i32 {
    let softness = 1.0 + random.next_f32() / 2.0;
    let radius = (1.0 - y as f32 / (height as f32 * softness)) * spread as f32;
    mth_ceil(radius / 2.0)
}

/// `IcebergFeature#isIcebergState`.
fn is_iceberg_state(name: &str) -> bool {
    name == "minecraft:packed_ice" || name == "minecraft:snow_block" || name == "minecraft:blue_ice"
}

/// `(float) Math.pow(v, 2.0)` where `v` is an `int`.
///
/// `Math.pow` with an exponent of exactly 2 is specified to return `x * x`,
/// so the double multiply is the same bits and the narrowing is the only
/// rounding.
#[allow(clippy::cast_possible_truncation)]
fn square_f32(v: i32) -> f32 {
    (f64::from(v) * f64::from(v)) as f32
}

/// `Mth#ceil(float)`.
#[allow(clippy::cast_possible_truncation)]
fn mth_ceil(value: f32) -> i32 {
    let truncated = value as i32;
    if value > truncated as f32 {
        truncated + 1
    } else {
        truncated
    }
}

/// `Mth#clamp(float, float, float)`.
fn mth_clamp(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

fn plain(state: &str) -> &str {
    state.split_once('[').map_or(state, |(n, _)| n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A world of water above a flat floor, which is what a frozen ocean
    /// column looks like to this feature.
    struct Ocean {
        blocks: BTreeMap<(i32, i32, i32), String>,
        writes: Vec<((i32, i32, i32), String)>,
    }

    impl Ocean {
        fn new() -> Self {
            Self {
                blocks: BTreeMap::new(),
                writes: Vec::new(),
            }
        }
        fn count(&self, name: &str) -> usize {
            self.blocks.values().filter(|v| plain(v) == name).count()
        }
    }

    impl PredicateWorld for Ocean {
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }

        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks.get(&(x, y, z)).cloned().unwrap_or_else(|| {
                if y <= 63 {
                    "minecraft:water[level=0]".to_string()
                } else {
                    "minecraft:air".to_string()
                }
            })
        }
    }

    impl IcebergWorld for Ocean {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
            self.writes.push(((x, y, z), state.to_string()));
        }
        fn sea_level(&mut self) -> i32 {
            63
        }
    }

    fn rng(seed: i64) -> crate::rng::WorldgenRandom<crate::rng::XoroshiroRandomSource> {
        crate::rng::WorldgenRandom::new(crate::rng::XoroshiroRandomSource::from_seed(seed))
    }

    /// The whole point of the item: this feature is where `blue_ice` comes
    /// from, and the port writes some.
    #[test]
    fn a_blue_iceberg_writes_blue_ice() {
        let mut world = Ocean::new();
        let mut random = rng(1234);
        assert!(place(&mut world, &mut random, (0, 0, 0), "minecraft:blue_ice").expect("ok"));
        assert!(
            world.count("minecraft:blue_ice") > 0,
            "no blue ice written: {:?}",
            world.writes.len()
        );
    }

    /// The same feature with the other configured state, which is how the
    /// packed-ice half of the shortfall is the same defect.
    #[test]
    fn a_packed_iceberg_writes_packed_ice() {
        let mut world = Ocean::new();
        let mut random = rng(1234);
        place(&mut world, &mut random, (0, 0, 0), "minecraft:packed_ice").expect("ok");
        assert!(world.count("minecraft:packed_ice") > 0);
    }

    /// The placement chain's y is discarded. Two attempts that differ only in
    /// y must write the same blocks.
    #[test]
    fn the_origin_is_rebuilt_at_sea_level() {
        let mut low = Ocean::new();
        let mut high = Ocean::new();
        place(&mut low, &mut rng(99), (0, 10, 0), "minecraft:blue_ice").expect("ok");
        place(&mut high, &mut rng(99), (0, 200, 0), "minecraft:blue_ice").expect("ok");
        assert_eq!(low.blocks, high.blocks);
    }

    /// `getEllipseC` pinches the minor axis over the top three layers only.
    #[test]
    fn the_cap_is_domed_over_three_layers() {
        assert_eq!(get_ellipse_c(0, 10, 3), 3, "y=0 is never pinched");
        assert_eq!(get_ellipse_c(5, 10, 3), 3, "far from the top");
        assert_eq!(get_ellipse_c(8, 10, 3), 1, "l-y=2, so 3 - (4-2)");
        assert_eq!(get_ellipse_c(10, 10, 3), -1, "the very top");
    }

    /// `Mth.ceil` truncates toward zero and bumps, which is plain `ceil` —
    /// but only if the negative case is right.
    #[test]
    fn mth_ceil_matches_java() {
        assert_eq!(mth_ceil(2.5), 3);
        assert_eq!(mth_ceil(2.0), 2);
        assert_eq!(mth_ceil(-2.5), -2);
        assert_eq!(mth_ceil(-2.0), -2);
        assert_eq!(mth_ceil(0.0), 0);
    }

    /// The ellipse arm must not touch the stream in `signedDistance*`, or
    /// every feature after it in the chunk moves.
    #[test]
    fn the_ellipse_distance_draws_nothing() {
        let mut untouched = rng(7);
        let mut probed = rng(7);
        let _ = signed_distance_ellipse(3, 4, (0, 0), 5, 2, 0.5);
        assert_eq!(
            crate::rng::RandomSource::next_i64(&mut untouched),
            crate::rng::RandomSource::next_i64(&mut probed)
        );
    }

    /// A run that places nothing still has to leave the stream where vanilla
    /// leaves it, so the DRAW COUNT is the gate, not the block count. A port
    /// that skipped the shape loops on an unplaceable world would look
    /// correct in every block test and desynchronise everything after it in
    /// the chunk.
    #[test]
    fn a_world_that_takes_no_blocks_still_moves_the_stream() {
        let mut stone = Ocean::new();
        for x in -32..32 {
            for z in -32..32 {
                for y in 30..100 {
                    stone
                        .blocks
                        .insert((x, y, z), "minecraft:stone".to_string());
                }
            }
        }
        let mut random = rng(5);
        place(&mut stone, &mut random, (0, 0, 0), "minecraft:blue_ice").expect("ok");
        assert_eq!(stone.count("minecraft:blue_ice"), 0, "nothing replaceable");

        let mut fresh = rng(5);
        assert_ne!(
            crate::rng::RandomSource::next_i64(&mut random),
            crate::rng::RandomSource::next_i64(&mut fresh),
            "the feature returned without drawing"
        );
    }

    /// The same seed on the same world twice is the same iceberg. Cheap, but
    /// it is the check that catches a stray read of ambient state.
    #[test]
    fn the_same_seed_builds_the_same_iceberg() {
        let mut first = Ocean::new();
        let mut second = Ocean::new();
        place(&mut first, &mut rng(4242), (0, 0, 0), "minecraft:blue_ice").expect("ok");
        place(&mut second, &mut rng(4242), (0, 0, 0), "minecraft:blue_ice").expect("ok");
        assert_eq!(first.writes, second.writes);
    }
}
