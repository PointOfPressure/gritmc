//! `TrunkPlacer` — the logs, and where the foliage hangs off them.
//!
//! Reference (26.2 decompile, behavior ported — never transcribed, CLAUDE.md
//! rule 4): `TrunkPlacer`, `StraightTrunkPlacer`, `ForkingTrunkPlacer`.
//!
//! # `getTreeHeight` is always two draws
//!
//! ```text
//! baseHeight + nextInt(heightRandA + 1) + nextInt(heightRandB + 1)
//! ```
//!
//! Oak is `4 + nextInt(3) + nextInt(1)`. That last term can only be zero, and
//! it is still a draw — `RandomSource.nextInt(1)` consumes a value and
//! returns it modulo one. Nineteen of the datapack's thirty-nine trees have
//! `height_rand_b` of zero, so a port that optimised the term away would
//! shift almost every tree in the world.
//!
//! # The block under the trunk goes in first
//!
//! `placeBelowTrunkBlock` runs before the first log, and it draws whatever
//! `below_trunk_provider` draws — for the vanilla trees that is a
//! `rule_based_state_provider` whose rule tests the block that is there, so
//! the dirt under a tree is placed before the tree.

use super::foliage::FoliageAttachment;
use super::{is_air_or_leaves, valid_tree_pos, NeedsRules, Placed, Pos, TreeConfig, TreeWorld};
use crate::rng::RandomSource;
use crate::tags::BlockTags;

/// One of vanilla's trunk placers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrunkPlacer {
    Straight {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    /// `dark_oak_trunk_placer` — the 2x2 trunk that leans once and then
    /// sprouts up to twelve stubs around its crown.
    ///
    /// Three things distinguish it from [`Self::MegaJungle`], which is the
    /// other 2x2 placer here.
    ///
    /// **The whole 2x2 layer is gated on ONE position.** `isAirOrLeaves` is
    /// asked about the north-west column only, and all four logs are placed
    /// or none are. `GiantTrunkPlacer` asks per column with `isFree`.
    ///
    /// **The lean is drawn before the loop and spent inside it.** `i` is the
    /// layer the lean starts at and `j` is how many layers it lasts, both
    /// drawn up front; `j` decrements only on the layers the lean actually
    /// moves, so a lean that starts above the tree never happens and never
    /// costs anything.
    ///
    /// **The stubs are placed relative to the ORIGIN and the attachments are
    /// too, while the crown attachment uses the LEANED column.** Vanilla
    /// reads `k`/`m` for the crown and `i1`/`j1` — the untouched origin x/z —
    /// for the twelve around it, so a leaning dark oak's outer canopy stays
    /// over its roots while its middle moves. Using the leaned column for
    /// both is the natural mistake and it is a shape difference no draw count
    /// would catch.
    DarkOak {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    /// Parsed but not placed. Carries its type name so a refusal says which.
    Unimplemented {
        name: String,
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    /// `forking_trunk_placer` — the acacia trunk, which leans once and then
    /// may sprout a second arm in a different direction.
    ///
    /// It returns **up to two** foliage attachments with *different* radius
    /// offsets (1 for the main arm, 0 for the fork), which is why an acacia
    /// canopy is two overlapping discs rather than one.
    Forking {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    /// `fancy_trunk_placer` — the big branching oak, and the only placer here
    /// that is a *geometry* rather than a stack of logs.
    ///
    /// Branch ends are sampled on a circle whose radius comes from
    /// [`tree_shape`], each branch is walked as a line by [`make_limb`], and
    /// the surviving branch ends become the foliage attachments. So the trunk
    /// placer decides where the canopy clusters go, which is why it and
    /// `fancy_foliage_placer` have to be right together.
    Fancy {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
    /// `mega_jungle_trunk_placer` — `MegaJungleTrunkPlacer`, a
    /// `GiantTrunkPlacer` (2x2 column, four blocks under it) with branches
    /// bolted on top.
    ///
    /// The branch loop is the whole of the risk, and it is a *while* loop
    /// whose bound is drawn:
    ///
    /// ```java
    ///   int y = height - 2 - random.nextInt(4);
    ///   while (y > height / 2) { ...; y -= 2 + random.nextInt(4); }
    /// ```
    ///
    /// so the number of branches — and therefore the number of draws the
    /// tree spends — is not a function of anything the terrain knows. Each
    /// iteration spends one `nextFloat` for the branch angle, whatever the
    /// trunk provider spends over five `placeLog` calls, and one `nextInt(4)`
    /// to step down. A tree refused for an unported placer spends **none** of
    /// this, which is why leaving it unported displaces every feature
    /// decorated after a jungle tree rather than merely omitting the tree.
    MegaJungle {
        base_height: i32,
        height_rand_a: i32,
        height_rand_b: i32,
    },
}

/// `FancyTrunkPlacer.treeShape` — the radius of the branch circle at height
/// `y` of a tree `height` tall, or a negative number for "no branches here".
///
/// All of it is `float`, and the `d == 0` and `abs(d) >= half` branches are
/// both reachable, so this is transcribed arm for arm rather than simplified.
#[must_use]
fn tree_shape(height: i32, y: i32) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "vanilla widens the same way")]
    let (h, yf) = (height as f32, y as f32);
    if yf < h * 0.3 {
        return -1.0;
    }
    let half = h / 2.0;
    let d = half - yf;
    let mut r = (half * half - d * d).sqrt();
    if d == 0.0 {
        r = half;
    } else if d.abs() >= half {
        return 0.0;
    }
    r * 0.5
}

/// `FancyTrunkPlacer.getSteps` — the Chebyshev length of a delta, which is
/// how many blocks a limb walk takes.
const fn steps_for(dx: i32, dy: i32, dz: i32) -> i32 {
    let (ax, ay, az) = (dx.abs(), dy.abs(), dz.abs());
    if ax > ay {
        if ax > az {
            ax
        } else {
            az
        }
    } else if ay > az {
        ay
    } else {
        az
    }
}

/// `FancyTrunkPlacer.getLogAxis` — `y` unless the limb moves further in x or
/// z, and x wins a tie.
fn log_axis(from: Pos, to: Pos) -> &'static str {
    let dx = (to.0 - from.0).abs();
    let dz = (to.2 - from.2).abs();
    let m = dx.max(dz);
    if m > 0 {
        if dx == m {
            "x"
        } else {
            "z"
        }
    } else {
        "y"
    }
}

/// `FancyTrunkPlacer.trimBranches` — a branch base below a fifth of the
/// tree's height is dropped.
fn trim_branches(height: i32, branch_y: i32) -> bool {
    f64::from(branch_y) >= f64::from(height) * 0.2
}

fn with_axis(state: &str, axis: &str) -> String {
    let Some((name, rest)) = state.split_once('[') else {
        return state.to_string();
    };
    let pairs: Vec<String> = rest
        .trim_end_matches(']')
        .split(',')
        .map(|pair| match pair.split_once('=') {
            Some(("axis", _)) => format!("axis={axis}"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{name}[{}]", pairs.join(","))
}

/// `FancyTrunkPlacer.makeLimb` — walk a straight line from `from` to `to`.
///
/// With `place` false this is a *probe*: it answers whether every block on
/// the line is free and writes nothing. With `place` true it lays logs and
/// always answers true. The early `from == to` return is guarded on `!place`,
/// so a placing walk of zero length still writes one log — and it gets there
/// through a division by zero steps, which in Java floors `NaN` to 0 and so
/// lands back on `from`. Rust's `as i32` on a NaN also gives 0, and
/// `floor_f32` below is written to match rather than relying on that.
#[expect(
    clippy::too_many_arguments,
    reason = "vanilla's signature plus the write log"
)]
fn make_limb<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    from: Pos,
    to: Pos,
    place: bool,
    placed: &mut Placed,
) -> Result<bool, NeedsRules> {
    if !place && from == to {
        return Ok(true);
    }
    let (dx, dy, dz) = (to.0 - from.0, to.1 - from.1, to.2 - from.2);
    let steps = steps_for(dx, dy, dz);
    #[expect(clippy::cast_precision_loss, reason = "vanilla divides in f32")]
    let (sx, sy, sz) = (
        dx as f32 / steps as f32,
        dy as f32 / steps as f32,
        dz as f32 / steps as f32,
    );
    for i in 0..=steps {
        #[expect(clippy::cast_precision_loss, reason = "vanilla widens the same way")]
        let f = i as f32;
        let at = (
            from.0 + floor_f32(0.5 + f * sx),
            from.1 + floor_f32(0.5 + f * sy),
            from.2 + floor_f32(0.5 + f * sz),
        );
        if place {
            // `placeLog` with a state function that forces the axis. The
            // axis is computed against the walk's *current* position, not
            // its end, so a limb that changes dominant direction changes
            // axis partway along.
            if valid_tree_pos(world, tags, at)? {
                let state = config
                    .trunk_provider
                    .state(world, tags, random, at)
                    .map_err(|e| NeedsRules(e.0))?;
                set_log(world, placed, at, &with_axis(&state, log_axis(from, at)))?;
            }
        } else if !is_free(world, tags, at)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `TrunkPlacer.isFree` — a valid tree position **or** an existing log.
///
/// The log clause is what lets a branch probe pass through wood already laid
/// by this same tree; without it a fancy oak would refuse most of its own
/// branches the moment the trunk was in the way.
fn is_free<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    pos: Pos,
) -> Result<bool, NeedsRules> {
    if valid_tree_pos(world, tags, pos)? {
        return Ok(true);
    }
    let state = world.block_at(pos.0, pos.1, pos.2);
    Ok(tags.contains("minecraft:logs", super::plain(&state)))
}

/// `Mth.floor(float)` — toward negative infinity, and `NaN` to zero, which is
/// what a zero-length placing limb walk depends on.
fn floor_f32(v: f32) -> i32 {
    if v.is_nan() {
        return 0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "tree coordinates are small"
    )]
    let i = v as i32;
    #[expect(clippy::cast_precision_loss, reason = "comparison only")]
    if v < i as f32 {
        i - 1
    } else {
        i
    }
}

/// `Direction.Plane.HORIZONTAL`, in the array order `getRandomDirection`
/// indexes: NORTH, EAST, SOUTH, WEST.
const HORIZONTAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// `Direction.Plane.HORIZONTAL.getRandomDirection` — one `nextInt(4)`.
///
/// Returned as an index rather than a step pair because the second arm is
/// placed only when it differs from the first, and comparing directions is
/// what vanilla does (`!= direction`), not comparing steps.
fn random_horizontal(random: &mut dyn RandomSource) -> usize {
    random.next_i32_bound(4) as usize
}

impl TrunkPlacer {
    /// # Errors
    /// If a shared field is missing.
    pub fn parse(value: &serde_json::Value) -> Result<Self, String> {
        let kind = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("trunk placer has no `type`: {value}"))?;
        let int = |name: &str| -> Result<i32, String> {
            value
                .get(name)
                .and_then(serde_json::Value::as_i64)
                .and_then(|v| i32::try_from(v).ok())
                .ok_or_else(|| format!("{kind} has no `{name}`"))
        };
        let base_height = int("base_height")?;
        let height_rand_a = int("height_rand_a")?;
        let height_rand_b = int("height_rand_b")?;
        match kind {
            "minecraft:straight_trunk_placer" => Ok(Self::Straight {
                base_height,
                height_rand_a,
                height_rand_b,
            }),
            "minecraft:forking_trunk_placer" => Ok(Self::Forking {
                base_height,
                height_rand_a,
                height_rand_b,
            }),
            "minecraft:fancy_trunk_placer" => Ok(Self::Fancy {
                base_height,
                height_rand_a,
                height_rand_b,
            }),
            "minecraft:mega_jungle_trunk_placer" => Ok(Self::MegaJungle {
                base_height,
                height_rand_a,
                height_rand_b,
            }),
            "minecraft:dark_oak_trunk_placer" => Ok(Self::DarkOak {
                base_height,
                height_rand_a,
                height_rand_b,
            }),
            other => Ok(Self::Unimplemented {
                name: other.to_string(),
                base_height,
                height_rand_a,
                height_rand_b,
            }),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Straight { .. } => "straight",
            Self::Forking { .. } => "forking",
            Self::Fancy { .. } => "fancy",
            Self::MegaJungle { .. } => "mega_jungle",
            Self::DarkOak { .. } => "dark_oak",
            Self::Unimplemented { name, .. } => name,
        }
    }

    #[must_use]
    pub fn is_supported(&self) -> bool {
        matches!(
            self,
            Self::Straight { .. }
                | Self::Forking { .. }
                | Self::Fancy { .. }
                | Self::MegaJungle { .. }
                | Self::DarkOak { .. }
        )
    }

    /// `TrunkPlacer#getTreeHeight`. Two draws, always.
    pub fn tree_height(&self, random: &mut dyn RandomSource) -> i32 {
        let (base, a, b) = match self {
            Self::Straight {
                base_height,
                height_rand_a,
                height_rand_b,
            }
            | Self::Forking {
                base_height,
                height_rand_a,
                height_rand_b,
            }
            | Self::Fancy {
                base_height,
                height_rand_a,
                height_rand_b,
            }
            | Self::MegaJungle {
                base_height,
                height_rand_a,
                height_rand_b,
            }
            | Self::DarkOak {
                base_height,
                height_rand_a,
                height_rand_b,
            }
            | Self::Unimplemented {
                base_height,
                height_rand_a,
                height_rand_b,
                ..
            } => (*base_height, *height_rand_a, *height_rand_b),
        };
        base + random.next_i32_bound(a + 1) + random.next_i32_bound(b + 1)
    }

    /// `TrunkPlacer#placeTrunk`, returning the foliage attachments.
    ///
    /// # Errors
    /// If the placer is unported or the world cannot answer.
    #[allow(
        clippy::too_many_arguments,
        reason = "\
        vanilla's own signature plus the write log; bundling them would only
        move the list somewhere the reference no longer matches"
    )]
    pub fn place_trunk<W: TreeWorld + ?Sized>(
        &self,
        world: &mut W,
        tags: &BlockTags,
        config: &TreeConfig,
        random: &mut dyn RandomSource,
        height: i32,
        origin: Pos,
        placed: &mut Placed,
    ) -> Result<Vec<FoliageAttachment>, NeedsRules> {
        match self {
            Self::Straight { .. } => {
                place_below_trunk_block(world, tags, config, random, origin, placed)?;
                for i in 0..height {
                    place_log(
                        world,
                        tags,
                        config,
                        random,
                        (origin.0, origin.1 + i, origin.2),
                        placed,
                    )?;
                }
                Ok(vec![FoliageAttachment {
                    pos: (origin.0, origin.1 + height, origin.2),
                    radius_offset: 0,
                    double_trunk: false,
                }])
            }
            // `FancyTrunkPlacer#placeTrunk`.
            //
            // Draw counts are the whole risk here: **two `nextFloat`s per
            // cluster attempt**, and an attempt happens for every y from
            // `height - 5` down to 0 whose `treeShape` is not negative. Both
            // draws are taken before either is used and before any world
            // read, so the count does not depend on the terrain — which is
            // what makes this portable at all.
            Self::Fancy { .. } => {
                let height_f = f64::from(height + 2);
                let height = height + 2;
                // `Mth.floor(height * 0.618)` — TRUNK_HEIGHT_SCALE.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "Mth.floor of a tree height"
                )]
                let trunk_height = (height_f * 0.618).floor() as i32;
                place_below_trunk_block(world, tags, config, random, origin, placed)?;

                // `Math.min(1, Mth.floor(1.382 + Math.pow(1.0 * height / 13.0, 2.0)))`.
                //
                // The `min` is against **1**, not against the floor's own
                // bound, so this is always 1 — `floor(1.382 + x)` for
                // non-negative x is at least 1, and the minimum caps it
                // there. It is written out rather than folded to a constant
                // because it is what the bytecode does, and a later version
                // that changes the 1 should show up as a diff here.
                let clusters_per_y = 1.min((1.382 + (height_f / 13.0).powf(2.0)).floor() as i32);

                let top_y = origin.1 + trunk_height;
                let mut coords: Vec<(Pos, i32)> = Vec::new();
                let start = height - 5;
                coords.push(((origin.0, origin.1 + start, origin.2), top_y));

                let mut y = start;
                while y >= 0 {
                    let shape = tree_shape(height, y);
                    if shape < 0.0 {
                        y -= 1;
                        continue;
                    }
                    for _ in 0..clusters_per_y {
                        // CLUSTER_DENSITY_MAGIC is 1.0 and multiplies the
                        // length; kept visible so the constant is findable.
                        let length =
                            1.0 * f64::from(shape) * (f64::from(random.next_f32()) + 0.328);
                        let angle = f64::from(random.next_f32() * 2.0) * std::f64::consts::PI;
                        let dx = length * angle.sin() + 0.5;
                        let dz = length * angle.cos() + 0.5;
                        let end = (
                            origin.0 + dx.floor() as i32,
                            origin.1 + y - 1,
                            origin.2 + dz.floor() as i32,
                        );
                        let top = (end.0, end.1 + 5, end.2);
                        if !make_limb(world, tags, config, random, end, top, false, placed)? {
                            continue;
                        }
                        // BRANCH_SLOPE = 0.381: how far down the trunk the
                        // branch has to start to reach this end.
                        let ddx = f64::from(origin.0 - end.0);
                        let ddz = f64::from(origin.2 - end.2);
                        let base = f64::from(end.1) - (ddx * ddx + ddz * ddz).sqrt() * 0.381;
                        let branch_base = if base > f64::from(top_y) {
                            top_y
                        } else {
                            base as i32
                        };
                        let trunk_at = (origin.0, branch_base, origin.2);
                        if make_limb(world, tags, config, random, trunk_at, end, false, placed)? {
                            coords.push((end, trunk_at.1));
                        }
                    }
                    y -= 1;
                }

                // The trunk itself, then the branches, then the surviving
                // attachments. `makeLimb` with `place` true never fails.
                make_limb(
                    world,
                    tags,
                    config,
                    random,
                    origin,
                    (origin.0, origin.1 + trunk_height, origin.2),
                    true,
                    placed,
                )?;
                // `makeBranches`: a limb from the trunk out to each cluster,
                // skipping the one whose base *is* the cluster.
                for &(end, base) in &coords {
                    let trunk_at = (origin.0, base, origin.2);
                    if trunk_at != end && trim_branches(height, base - origin.1) {
                        make_limb(world, tags, config, random, trunk_at, end, true, placed)?;
                    }
                }
                Ok(coords
                    .iter()
                    .filter(|&&(_, base)| trim_branches(height, base - origin.1))
                    .map(|&(end, _)| FoliageAttachment {
                        pos: end,
                        radius_offset: 0,
                        double_trunk: false,
                    })
                    .collect())
            }
            Self::Forking { .. } => {
                place_below_trunk_block(world, tags, config, random, origin, placed)?;
                let mut attachments = Vec::new();

                // Draw order: the direction first, then the two shape
                // numbers. All three happen whether or not the trunk ends up
                // leaning, so they are unconditional.
                let first_dir = random_horizontal(random);
                let lean_from = height - random.next_i32_bound(4) - 1;
                let mut lean_left = 3 - random.next_i32_bound(3);

                let (mut x, mut z) = (origin.0, origin.2);
                let mut top: Option<i32> = None;
                for i in 0..height {
                    let y = origin.1 + i;
                    if i >= lean_from && lean_left > 0 {
                        x += HORIZONTAL[first_dir].0;
                        z += HORIZONTAL[first_dir].1;
                        lean_left -= 1;
                    }
                    if place_log(world, tags, config, random, (x, y, z), placed)? {
                        top = Some(y + 1);
                    }
                }
                if let Some(y) = top {
                    attachments.push(FoliageAttachment {
                        pos: (x, y, z),
                        radius_offset: 1,
                        double_trunk: false,
                    });
                }

                // The fork restarts from the origin column, but the *lean*
                // is not undone: `x`/`z` are reset, `lean_from` is not.
                let (mut x, mut z) = (origin.0, origin.2);
                let second_dir = random_horizontal(random);
                if second_dir != first_dir {
                    let start = lean_from - random.next_i32_bound(2) - 1;
                    let mut left = 1 + random.next_i32_bound(3);
                    let mut top: Option<i32> = None;
                    let mut i = start;
                    while i < height && left > 0 {
                        // Below y=1 the step is skipped — but `left` still
                        // decrements, so a fork starting underground is
                        // shorter rather than delayed. Getting this wrong
                        // grows the arm by however many rows it skipped.
                        if i >= 1 {
                            let y = origin.1 + i;
                            x += HORIZONTAL[second_dir].0;
                            z += HORIZONTAL[second_dir].1;
                            if place_log(world, tags, config, random, (x, y, z), placed)? {
                                top = Some(y + 1);
                            }
                        }
                        i += 1;
                        left -= 1;
                    }
                    if let Some(y) = top {
                        attachments.push(FoliageAttachment {
                            pos: (x, y, z),
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                }
                Ok(attachments)
            }
            // `MegaJungleTrunkPlacer#placeTrunk` — the giant trunk, then a
            // ladder of branches walking down from near the top.
            Self::MegaJungle { .. } => {
                let mut attachments =
                    place_giant_trunk(world, tags, config, random, height, origin, placed)?;

                // `int y = height - 2 - random.nextInt(4);`
                let mut y = height - 2 - random.next_i32_bound(4);
                // `while (y > height / 2)`. Java's `/` and Rust's both
                // truncate toward zero, and `height` is positive here.
                while y > height / 2 {
                    // `random.nextFloat() * 6.2831855f` — the literal is
                    // vanilla's own float approximation of a full turn, not
                    // `2.0 * PI` computed in `f64` and narrowed. Written out
                    // so a version that changes it shows as a diff.
                    let angle = random.next_f32() * 6.283_185_5_f32;
                    let (mut dx, mut dz) = (0, 0);
                    for i in 0..5 {
                        // `Mth.cos`/`Mth.sin` are the 65,536-entry TABLE, not
                        // libm — `crate::math` holds the table and the
                        // narrowing rules. A port calling `f32::cos` here
                        // produces branches at subtly different angles, which
                        // is a *shape* difference and not a draw difference,
                        // so nothing in the stream would catch it.
                        #[expect(
                            clippy::cast_precision_loss,
                            reason = "vanilla widens the loop counter to float"
                        )]
                        let step = i as f32;
                        // `(int)` on a float truncates toward zero in Java
                        // and in Rust alike — so `1.5 + (-2.0)` is `0` in
                        // both, and NOT `-1` as a floor would give. The
                        // asymmetry is visible: a branch heading west starts
                        // one block nearer the trunk than one heading east.
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "vanilla's own (int) cast, truncating toward zero"
                        )]
                        {
                            dx = (1.5_f32 + crate::math::cos(f64::from(angle)) * step) as i32;
                            dz = (1.5_f32 + crate::math::sin(f64::from(angle)) * step) as i32;
                        }
                        // `origin.offset(dx, y - 3 + i / 2, dz)` — the branch
                        // rises half a block per step, by integer division.
                        place_log(
                            world,
                            tags,
                            config,
                            random,
                            (origin.0 + dx, origin.1 + y - 3 + i / 2, origin.2 + dz),
                            placed,
                        )?;
                    }
                    // The attachment uses the LAST `dx`/`dz` the loop left
                    // behind — vanilla declares them outside the loop and
                    // reads them after it, so the canopy hangs off the far
                    // end of the branch. `radiusOffset` is -2 and
                    // `doubleTrunk` is false, and that false is what makes
                    // `MegaJungleFoliagePlacer` take its `nextInt(2)`.
                    attachments.push(FoliageAttachment {
                        pos: (origin.0 + dx, origin.1 + y, origin.2 + dz),
                        radius_offset: -2,
                        double_trunk: false,
                    });
                    // `y -= 2 + random.nextInt(4);`
                    y -= 2 + random.next_i32_bound(4);
                }
                Ok(attachments)
            }
            // `DarkOakTrunkPlacer#placeTrunk`.
            Self::DarkOak { .. } => {
                let mut attachments = Vec::new();
                let below = (origin.0, origin.1 - 1, origin.2);
                place_below_trunk_block_at(world, tags, config, random, below, placed)?;
                place_below_trunk_block_at(
                    world,
                    tags,
                    config,
                    random,
                    (below.0 + 1, below.1, below.2),
                    placed,
                )?;
                place_below_trunk_block_at(
                    world,
                    tags,
                    config,
                    random,
                    (below.0, below.1, below.2 + 1),
                    placed,
                )?;
                place_below_trunk_block_at(
                    world,
                    tags,
                    config,
                    random,
                    (below.0 + 1, below.1, below.2 + 1),
                    placed,
                )?;

                // THREE DRAWS, in this order, before a single log.
                let direction = random_horizontal(random);
                let lean_from = height - random.next_i32_bound(4);
                let mut lean_left = 2 - random.next_i32_bound(3);

                let (x0, y0, z0) = origin;
                let (mut x, mut z) = (x0, z0);
                // `y0 + height - 1` — the crown's y, fixed before the loop and
                // NOT recomputed from where the trunk ended.
                let top = y0 + height - 1;

                for layer in 0..height {
                    if layer >= lean_from && lean_left > 0 {
                        x += HORIZONTAL[direction].0;
                        z += HORIZONTAL[direction].1;
                        lean_left -= 1;
                    }
                    let y = y0 + layer;
                    // One question for the whole 2x2: all four or none.
                    if is_air_or_leaves(world, tags, (x, y, z))? {
                        place_log(world, tags, config, random, (x, y, z), placed)?;
                        place_log(world, tags, config, random, (x + 1, y, z), placed)?;
                        place_log(world, tags, config, random, (x, y, z + 1), placed)?;
                        place_log(world, tags, config, random, (x + 1, y, z + 1), placed)?;
                    }
                }

                // The crown, over the LEANED column.
                attachments.push(FoliageAttachment {
                    pos: (x, top, z),
                    radius_offset: 0,
                    double_trunk: true,
                });

                // Twelve stubs in the 4x4 ring around the 2x2 centre, each a
                // 1-in-3 coin followed by a length. Both draws are inside the
                // `if`, so a skipped cell costs one value and not two.
                for dx in -1..=2 {
                    for dz in -1..=2 {
                        if (0..=1).contains(&dx) && (0..=1).contains(&dz) {
                            continue;
                        }
                        if random.next_i32_bound(3) > 0 {
                            continue;
                        }
                        let length = random.next_i32_bound(3) + 2;
                        for n in 0..length {
                            place_log(
                                world,
                                tags,
                                config,
                                random,
                                (x0 + dx, top - n - 1, z0 + dz),
                                placed,
                            )?;
                        }
                        // Read the reference twice here: the stub is built at
                        // `x0 + dx` and the attachment is placed at `x + dx`,
                        // the LEANED column. Vanilla really does use the two
                        // different origins in two consecutive statements.
                        attachments.push(FoliageAttachment {
                            pos: (x + dx, top, z + dz),
                            radius_offset: 0,
                            double_trunk: false,
                        });
                    }
                }
                Ok(attachments)
            }
            Self::Unimplemented { name, .. } => Err(NeedsRules(format!("{name} trunk placer"))),
        }
    }
}

/// `GiantTrunkPlacer#placeTrunk` — the 2x2 column shared by the mega jungle,
/// mega pine and mega spruce trees.
///
/// ```java
///   BlockPos below = origin.below();
///   placeBelowTrunkBlock(level, c, random, below, config);
///   placeBelowTrunkBlock(level, c, random, below.east(), config);
///   placeBelowTrunkBlock(level, c, random, below.south(), config);
///   placeBelowTrunkBlock(level, c, random, below.south().east(), config);
///   for (int y = 0; y < height; y++) {
///       placeLogIfFreeWithOffset(0, y, 0);
///       if (y < height - 1) {
///           placeLogIfFreeWithOffset(1, y, 0);
///           placeLogIfFreeWithOffset(1, y, 1);
///           placeLogIfFreeWithOffset(0, y, 1);
///       }
///   }
///   return ImmutableList.of(new FoliageAttachment(origin.above(height), 0, true));
/// ```
///
/// Two details that are easy to lose:
///
///   * the **top layer is one block, not four** — the `y < height - 1` guard
///     skips the other three columns on the last row, which is what makes a
///     giant trunk taper to a single log under its crown;
///   * `placeLogIfFree` tests `isFree` *before* `placeLog`, and `placeLog`
///     then re-tests `validTreePos` before drawing from the trunk provider.
///     `isFree` is the weaker test — it also accepts an existing log — so the
///     provider can be skipped by either, and the two are not interchangeable.
fn place_giant_trunk<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    height: i32,
    origin: Pos,
    placed: &mut Placed,
) -> Result<Vec<FoliageAttachment>, NeedsRules> {
    let below = (origin.0, origin.1 - 1, origin.2);
    for pos in [
        below,
        (below.0 + 1, below.1, below.2),
        (below.0, below.1, below.2 + 1),
        (below.0 + 1, below.1, below.2 + 1),
    ] {
        place_below_trunk_block_at(world, tags, config, random, pos, placed)?;
    }

    for y in 0..height {
        place_log_if_free(
            world,
            tags,
            config,
            random,
            (origin.0, origin.1 + y, origin.2),
            placed,
        )?;
        if y < height - 1 {
            for (dx, dz) in [(1, 0), (1, 1), (0, 1)] {
                place_log_if_free(
                    world,
                    tags,
                    config,
                    random,
                    (origin.0 + dx, origin.1 + y, origin.2 + dz),
                    placed,
                )?;
            }
        }
    }

    Ok(vec![FoliageAttachment {
        pos: (origin.0, origin.1 + height, origin.2),
        radius_offset: 0,
        double_trunk: true,
    }])
}

/// `TrunkPlacer#placeLogIfFree` — `if (isFree(pos)) placeLog(pos)`.
fn place_log_if_free<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    pos: Pos,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    if is_free(world, tags, pos)? {
        place_log(world, tags, config, random, pos, placed)?;
    }
    Ok(())
}

/// `TrunkPlacer#placeBelowTrunkBlock` — one block under the trunk, from
/// `below_trunk_provider`, and only if the provider offers one.
fn place_below_trunk_block<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    origin: Pos,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    place_below_trunk_block_at(
        world,
        tags,
        config,
        random,
        (origin.0, origin.1 - 1, origin.2),
        placed,
    )
}

/// The same call on an ARBITRARY position rather than on `origin.below()`.
///
/// `placeBelowTrunkBlock` is static and takes the position, and
/// `GiantTrunkPlacer` passes four different ones — `below`, `below.east()`,
/// `below.south()` and `below.south().east()`. Folding the `-1` into the
/// helper, as the single-column placers allowed, makes three of those four
/// unreachable.
fn place_below_trunk_block_at<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    pos: Pos,
    placed: &mut Placed,
) -> Result<(), NeedsRules> {
    let state = config
        .below_trunk_provider
        .optional_state(world, tags, random, pos)
        .map_err(|e| NeedsRules(e.0))?;
    // `getOptionalState` returns null when a rule-based provider matched
    // nothing, and vanilla then writes nothing at all.
    if let Some(state) = state {
        set_root(world, placed, pos, &state)?;
    }
    Ok(())
}

/// `TrunkPlacer#placeLog` — a log if the position is valid, and the provider
/// draws only when it is.
fn place_log<W: TreeWorld + ?Sized>(
    world: &mut W,
    tags: &BlockTags,
    config: &TreeConfig,
    random: &mut dyn RandomSource,
    pos: Pos,
    placed: &mut Placed,
) -> Result<bool, NeedsRules> {
    if !valid_tree_pos(world, tags, pos)? {
        return Ok(false);
    }
    let state = config
        .trunk_provider
        .state(world, tags, random, pos)
        .map_err(|e| NeedsRules(e.0))?;
    set_log(world, placed, pos, &state)?;
    Ok(true)
}

fn set_log<W: TreeWorld + ?Sized>(
    world: &mut W,
    placed: &mut Placed,
    pos: Pos,
    state: &str,
) -> Result<(), NeedsRules> {
    world.set_block(pos.0, pos.1, pos.2, state);
    // A refusal here, not a crash. `java_hash_set` reproduces treeified
    // buckets now, so the only thing left that can refuse is a bucket
    // treeifying with two equal spread hashes in it — `java.util.HashMap`
    // would order that tree by `System.identityHashCode`, which nothing can
    // reproduce. It has never been observed on a measured window, because a
    // tree's positions are local and `(y + z * 31) * 31 + x` is injective
    // over a box that small. The project counts refusals so a thinner world
    // is reported rather than passed off as a complete one.
    //
    // It was an `.expect` here, which made a designed limit fatal, and the
    // effect was invisible for a long time because every measurement window
    // this project uses is ocean-ish: the first attempt to generate a
    // *forested* window died on it, and that is also why coverage looked the
    // way it did.
    placed.logs.insert(pos).map_err(|e| NeedsRules(e.0))?;
    Ok(())
}

fn set_root<W: TreeWorld + ?Sized>(
    world: &mut W,
    placed: &mut Placed,
    pos: Pos,
    state: &str,
) -> Result<(), NeedsRules> {
    world.set_block(pos.0, pos.1, pos.2, state);
    // A refusal here, not a crash. `java_hash_set` reproduces treeified
    // buckets now, so the only thing left that can refuse is a bucket
    // treeifying with two equal spread hashes in it — `java.util.HashMap`
    // would order that tree by `System.identityHashCode`, which nothing can
    // reproduce. It has never been observed on a measured window, because a
    // tree's positions are local and `(y + z * 31) * 31 + x` is injective
    // over a box that small. The project counts refusals so a thinner world
    // is reported rather than passed off as a complete one.
    //
    // It was an `.expect` here, which made a designed limit fatal, and the
    // effect was invisible for a long time because every measurement window
    // this project uses is ocean-ish: the first attempt to generate a
    // *forested* window died on it, and that is also why coverage looked the
    // way it did.
    placed.roots.insert(pos).map_err(|e| NeedsRules(e.0))?;
    Ok(())
}

#[cfg(test)]
mod dark_oak_tests {
    use super::*;
    use crate::feature::block_predicate::PredicateWorld;
    use crate::feature::tree::TreeConfig;
    use crate::rng::draws;
    use crate::rng::legacy::LegacyRandomSource;
    use crate::rng::worldgen_random::WorldgenRandom;
    use std::collections::BTreeMap;

    /// Open air over a grass plane at y = 63.
    struct Field {
        blocks: BTreeMap<Pos, String>,
    }

    impl Field {
        fn new() -> Self {
            let mut blocks = BTreeMap::new();
            for x in -32..=32 {
                for z in -32..=32 {
                    blocks.insert((x, 63, z), "minecraft:grass_block[snowy=false]".to_string());
                    blocks.insert((x, 62, z), "minecraft:dirt".to_string());
                }
            }
            Self { blocks }
        }
        fn count(&self, name: &str) -> usize {
            self.blocks
                .values()
                .filter(|v| v.split_once('[').map_or(v.as_str(), |(n, _)| n) == name)
                .count()
        }
    }

    impl PredicateWorld for Field {
        fn block_at(&mut self, x: i32, y: i32, z: i32) -> String {
            self.blocks
                .get(&(x, y, z))
                .cloned()
                .unwrap_or_else(|| "minecraft:air".to_string())
        }
        fn air_at(&mut self, x: i32, y: i32, z: i32) -> Option<bool> {
            Some(self.block_at(x, y, z) == "minecraft:air")
        }
    }

    impl TreeWorld for Field {
        fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
            self.blocks.insert((x, y, z), state.to_string());
        }
        fn min_y(&self) -> i32 {
            -64
        }
        fn max_y(&self) -> i32 {
            319
        }
        fn motion_blocking_no_leaves(&mut self, _x: i32, _z: i32) -> i32 {
            64
        }
    }

    fn tags() -> BlockTags {
        BlockTags::from_entries(BTreeMap::from([
            (
                "minecraft:replaceable_by_trees".to_string(),
                vec!["minecraft:short_grass".to_string()],
            ),
            (
                "minecraft:leaves".to_string(),
                vec!["minecraft:dark_oak_leaves".to_string()],
            ),
            (
                "minecraft:logs".to_string(),
                vec!["minecraft:dark_oak_log".to_string()],
            ),
            (
                "minecraft:cannot_replace_below_tree_trunk".to_string(),
                vec!["minecraft:bedrock".to_string()],
            ),
            (
                "minecraft:dirt".to_string(),
                vec![
                    "minecraft:dirt".to_string(),
                    "minecraft:grass_block".to_string(),
                ],
            ),
        ]))
        .expect("tags")
    }

    /// `minecraft:dark_oak`, as the datapack writes it.
    fn dark_oak_config() -> TreeConfig {
        let states = crate::feature::state_spec::DefaultStateTable::from_pairs(&[
            ("minecraft:dark_oak_log", "minecraft:dark_oak_log[axis=y]"),
            (
                "minecraft:dark_oak_leaves",
                "minecraft:dark_oak_leaves[distance=7,persistent=false,waterlogged=false]",
            ),
            ("minecraft:dirt", "minecraft:dirt"),
        ]);
        TreeConfig::parse(
            &serde_json::from_str(
                r#"{
                    "trunk_placer": {"type":"minecraft:dark_oak_trunk_placer",
                        "base_height":6,"height_rand_a":2,"height_rand_b":1},
                    "foliage_placer": {"type":"minecraft:dark_oak_foliage_placer",
                        "offset":0,"radius":0},
                    "minimum_size": {"type":"minecraft:three_layers_feature_size","upper_size":2},
                    "trunk_provider": {"type":"minecraft:simple_state_provider",
                        "state":{"Name":"minecraft:dark_oak_log"}},
                    "foliage_provider": {"type":"minecraft:simple_state_provider",
                        "state":{"Name":"minecraft:dark_oak_leaves"}},
                    "below_trunk_provider": {"type":"minecraft:simple_state_provider",
                        "state":{"Name":"minecraft:dirt"}},
                    "decorators": []
                }"#,
            )
            .expect("json"),
            &states,
            &crate::feature::tree::decorator::no_registry,
        )
        .expect("config")
    }

    #[test]
    fn the_whole_dark_oak_family_is_supported_now() {
        let config = dark_oak_config();
        assert!(config.is_supported());
        assert_eq!(config.trunk_placer.name(), "dark_oak");
        assert_eq!(config.foliage_placer.name(), "dark_oak");
    }

    /// The three fixed draws before a single log, in order: direction, the
    /// layer the lean starts at, and how many layers it lasts.
    ///
    /// A `dark_oak_trunk_placer` that drew them in any other order would place
    /// a correct-looking tree leaning the wrong way, and would move every draw
    /// after it in the chunk.
    #[test]
    fn three_draws_come_before_the_first_log_and_twelve_coins_after_it() {
        let config = dark_oak_config();
        let mut world = Field::new();
        let mut random = WorldgenRandom::new(LegacyRandomSource::new(11));
        let mut placed = Placed::default();
        draws::arm();
        let attachments = config
            .trunk_placer
            .place_trunk(
                &mut world,
                &tags(),
                &config,
                &mut random,
                7,
                (0, 64, 0),
                &mut placed,
            )
            .expect("trunk");
        let spent = draws::take();
        // 3 fixed, then between 12 and 24 for the stubs: one `nextInt(3)` per
        // cell and a second draw only for the cells that pass.
        assert!(
            spent >= 15,
            "at least three fixed and twelve coins, got {spent}"
        );
        assert!(
            spent <= 27,
            "at most a second draw for each of twelve, got {spent}"
        );
        // The crown, plus one attachment per stub that was built.
        assert!(!attachments.is_empty());
        assert!(attachments[0].double_trunk, "the crown is the double trunk");
        assert_eq!(attachments[0].radius_offset, 0);
        for a in &attachments[1..] {
            assert!(!a.double_trunk, "a stub is never a double trunk");
        }
        assert_eq!(attachments.len(), 1 + (spent as usize - 15));
        // A 2x2 trunk seven layers tall is 28 logs before the stubs.
        assert!(world.count("minecraft:dark_oak_log") >= 28);
        // Four dirt blocks under the 2x2, replacing grass and placed BEFORE
        // the first log — the field already holds 65x65 of dirt one lower.
        assert_eq!(world.count("minecraft:dirt"), 65 * 65 + 4);
    }

    /// The crown hangs over the LEANED column and the stubs are built at the
    /// ORIGIN column. Vanilla uses the two different origins in consecutive
    /// statements, and using one for both is a shape change no draw count
    /// would catch.
    #[test]
    fn the_lean_moves_the_crown_and_not_the_stubs() {
        let config = dark_oak_config();
        let mut leaned = None;
        // Search seeds until one leans, so the test cannot pass by the tree
        // happening to stay put.
        for seed in 0..64 {
            let mut world = Field::new();
            let mut random = WorldgenRandom::new(LegacyRandomSource::new(seed));
            let mut placed = Placed::default();
            let attachments = config
                .trunk_placer
                .place_trunk(
                    &mut world,
                    &tags(),
                    &config,
                    &mut random,
                    8,
                    (0, 64, 0),
                    &mut placed,
                )
                .expect("trunk");
            let crown = attachments[0].pos;
            if (crown.0, crown.2) != (0, 0) && attachments.len() > 1 {
                leaned = Some((crown, attachments[1..].to_vec(), world));
                break;
            }
        }
        let (crown, stubs, world) = leaned.expect("some seed under 64 leans and grows a stub");
        assert_eq!(
            crown.1,
            64 + 8 - 1,
            "the crown's y is fixed before the loop"
        );
        // Every stub attachment is within one of the CROWN column, because
        // vanilla offsets `x`/`z` — the leaned pair — by the loop indices.
        for s in &stubs {
            assert!((s.pos.0 - crown.0).abs() <= 2 && (s.pos.2 - crown.2).abs() <= 2);
            assert_eq!(s.pos.1, crown.1);
        }
        // But the stub LOGS are at the origin column: at least one log stands
        // outside the 2x2 the leaned trunk occupies.
        let mut w = world;
        let mut found_at_origin_ring = false;
        for dx in -1..=2 {
            for dz in -1..=2 {
                if (0..=1).contains(&dx) && (0..=1).contains(&dz) {
                    continue;
                }
                for y in (64 + 8 - 5)..(64 + 8 - 1) {
                    if w.block_at(dx, y, dz).starts_with("minecraft:dark_oak_log") {
                        found_at_origin_ring = true;
                    }
                }
            }
        }
        assert!(
            found_at_origin_ring,
            "the stubs are built at the origin column, not the leaned one"
        );
    }
}
