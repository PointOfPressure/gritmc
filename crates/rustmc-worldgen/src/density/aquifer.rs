//! Aquifer geometry — the lattice of local water tables.
//!
//! Reference: `net.minecraft.world.level.levelgen.Aquifer.NoiseBasedAquifer`.
//!
//! # What an aquifer is for
//!
//! Without one, "below sea level and not solid" means water, so every cave
//! under y=63 would flood. Vanilla instead scatters aquifer centres on a
//! coarse lattice (16 x 12 x 16 blocks), gives each its own fluid level, and
//! blends between the two nearest. That is why one cave is dry, the next has
//! a lake, and the boundary between them is a barrier of stone rather than a
//! straight waterline.
//!
//! # Layout
//!
//! Two halves, kept apart on purpose. First the **pure geometry**: lattice
//! mapping, the jitter offsets and the similarity function. Then
//! [`NoiseBasedAquifer`] itself, which is stateful — it caches a jittered
//! centre and a fluid status per lattice cell, and sets a
//! `should_schedule_fluid_update` flag the chunk filler reads back.
//!
//! The geometry is separate because it is exactly where an off-by-one hides:
//! pure, cheap to test exhaustively, and a one-block error in it moves every
//! water table in the world without changing anything else.

// Read once rather than per block. See `crate::trace_switch`.
crate::trace_switch_value!(trace_aquifer, "RUSTMC_TRACE_AQUIFER");

// `RUSTMC_TRACE_AQUIFER_CENTRE=x,y,z[;x,y,z...]` traces `computeFluid` for one
// aquifer *centre* — the jittered lattice point, not the block being asked
// about. It is a separate switch from `RUSTMC_TRACE_AQUIFER` because the two
// answer different questions: that one says which centre won, this one says
// why that centre holds the level it does, and a fluid-level defect is only
// ever visible in the second.
crate::trace_switch_value!(trace_aquifer_centre, "RUSTMC_TRACE_AQUIFER_CENTRE");

/// One candidate centre, as `RUSTMC_TRACE_AQUIFER` reports it.
///
/// Named rather than a tuple because the interesting rows are the *losing*
/// ones — a boundary between two water tables is a pair of near-equal
/// `distance_sqr` values, and reading that off positional tuple fields is how
/// a trace ends up quietly printing the wrong column.
struct TracedCentre {
    cell: (i32, i32, i32),
    centre: (i32, i32, i32),
    distance_sqr: i32,
    index: usize,
}

/// Search radius, in lattice cells, around the block being evaluated.
pub const X_RANGE: i32 = 10;
pub const Y_RANGE: i32 = 9;
pub const Z_RANGE: i32 = 10;

/// How far a centre may be jittered from its cell origin.
pub const X_SEPARATION: i32 = 6;
pub const Y_SEPARATION: i32 = 3;
pub const Z_SEPARATION: i32 = 6;

/// Lattice pitch in blocks. Y is deliberately finer than XZ (12 vs 16), so
/// water tables stack vertically more often than they tile horizontally.
pub const X_SPACING: i32 = 16;
pub const Y_SPACING: i32 = 12;
pub const Z_SPACING: i32 = 16;

/// Sampling is offset by -5 in x/z and +1 in y before the lattice mapping,
/// so cell boundaries do not line up with chunk boundaries. Aligned ones
/// would make aquifer edges visibly follow chunk edges.
pub const SAMPLE_OFFSET_X: i32 = -5;
pub const SAMPLE_OFFSET_Y: i32 = 1;
pub const SAMPLE_OFFSET_Z: i32 = -5;

/// Block -> lattice cell on x.
///
/// `>> 4` is an arithmetic shift, i.e. floor division by 16 — **not**
/// truncation. Using `/ 16` would map blocks -15..-1 to cell 0 alongside
/// 0..15, doubling one cell's width and shifting every aquifer west of the
/// origin.
#[must_use]
pub const fn grid_x(block_coord: i32) -> i32 {
    block_coord >> 4
}

/// Block -> lattice cell on z. Same shift as x.
#[must_use]
pub const fn grid_z(block_coord: i32) -> i32 {
    block_coord >> 4
}

/// Block -> lattice cell on y.
///
/// 12 is not a power of two, so this is an explicit floor division rather
/// than a shift. `div_euclid` matches Java's `Math.floorDiv` for a positive
/// divisor.
#[must_use]
pub const fn grid_y(block_coord: i32) -> i32 {
    block_coord.div_euclid(Y_SPACING)
}

#[must_use]
pub const fn from_grid_x(grid_coord: i32, block_offset: i32) -> i32 {
    (grid_coord << 4) + block_offset
}

#[must_use]
pub const fn from_grid_y(grid_coord: i32, block_offset: i32) -> i32 {
    grid_coord * Y_SPACING + block_offset
}

#[must_use]
pub const fn from_grid_z(grid_coord: i32, block_offset: i32) -> i32 {
    (grid_coord << 4) + block_offset
}

/// How alike two aquifer centres are, from their squared distances.
///
/// Returns 1 when the two are equidistant and falls off as the second gets
/// further away; it goes negative past a squared-distance gap of 25. The
/// caller uses that sign to decide whether the two tables merge or a
/// barrier forms between them — so this is a blend weight, not a metric,
/// and it is deliberately unclamped.
#[must_use]
pub fn similarity(distance_sqr_1: i32, distance_sqr_2: i32) -> f64 {
    const THRESHOLD: f64 = 25.0;
    1.0 - f64::from(distance_sqr_2 - distance_sqr_1) / THRESHOLD
}

/// `FLOWING_UPDATE_SIMULARITY` — the cutoff at which vanilla schedules a
/// fluid tick, taken at squared distances 10² and 12².
#[must_use]
pub fn flowing_update_similarity() -> f64 {
    similarity(10 * 10, 12 * 12)
}

// ---------------------------------------------------------------------------
// The stateful part
// ---------------------------------------------------------------------------

use super::fill::{FluidStatus, GlobalFluidPicker, Substance};
use super::flat_cache::FlatCaches;
use super::surface_level::SurfaceLevels;
use super::{DensityFunction, FunctionContext};
use crate::level::WAY_BELOW_MIN_Y;
use crate::math;
use crate::rng::{PositionalRandomFactory, RandomSource, XoroshiroPositionalFactory};

/// How far above the local surface the aquifer stops sampling entirely.
const SURFACE_LEVEL_ADJUSTMENT: i32 = 8;

/// Chunk offsets, in *sections*, that `computeFluid` probes for a surface.
///
/// Not a disc and not symmetric: it leans west and the entry order matters,
/// because the loop returns early on the first offset whose surface pokes
/// above the aquifer cell. Reordering it changes which lakes exist.
const SURFACE_SAMPLING_OFFSETS_IN_CHUNKS: [[i32; 2]; 13] = [
    [0, 0],
    [-2, -1],
    [-1, -1],
    [0, -1],
    [1, -1],
    [-3, 0],
    [-2, 0],
    [-1, 0],
    [1, 0],
    [-2, 1],
    [-1, 1],
    [0, 1],
    [1, 1],
];

/// `Aquifer.NoiseBasedAquifer` — local water tables and the barriers between
/// them.
///
/// # What it is really deciding
///
/// Given a point the terrain says is *not* solid, three questions: is there
/// fluid here at all, which fluid, and is there a wall of stone between this
/// pocket and the next one along. The last is what stops two lakes at
/// different heights from merging into a waterfall, and it is why this can
/// return "solid" for a point the density function called open.
///
/// # State, and why it is not incidental
///
/// Two caches, both indexed by lattice cell:
///
/// - `locations` — where each cell's aquifer centre was jittered to. Drawn
///   from a positional RNG, so it is reproducible, but each cell must be
///   drawn **once**: the draw is not idempotent in the sense that matters,
///   because reading it twice is fine but the cached value is compared for
///   identity across neighbouring queries.
/// - `statuses` — the fluid and level for that centre, which costs a
///   13-probe surface scan to compute.
///
/// `should_schedule_fluid_update` is genuinely a side channel: vanilla sets
/// it during `compute_substance` and the chunk filler reads it immediately
/// after, to decide whether to schedule a fluid tick. Kept as a field for
/// exactly that reason and read through [`Self::should_schedule_fluid_update`].
#[derive(Debug)]
pub struct NoiseBasedAquifer {
    barrier: DensityFunction,
    fluid_level_floodedness: DensityFunction,
    fluid_level_spread: DensityFunction,
    lava: DensityFunction,
    erosion: DensityFunction,
    depth: DensityFunction,
    random: XoroshiroPositionalFactory,
    global: GlobalFluidPicker,

    statuses: Vec<Option<FluidStatus>>,
    /// `None` is vanilla's `Long.MAX_VALUE` sentinel. Stored unpacked
    /// because `BlockPos.asLong` is lossless for every coordinate reachable
    /// here — x/z stay inside the world border's 26 bits and y inside 12 —
    /// and the packed form is never compared as an integer.
    locations: Vec<Option<(i32, i32, i32)>>,

    min_grid_x: i32,
    min_grid_y: i32,
    min_grid_z: i32,
    grid_size_x: i32,
    grid_size_z: i32,

    skip_sampling_above_y: i32,
    should_schedule_fluid_update: bool,
}

/// Everything the aquifer needs from the router.
#[derive(Debug)]
pub struct AquiferNoises {
    pub barrier: DensityFunction,
    pub fluid_level_floodedness: DensityFunction,
    pub fluid_level_spread: DensityFunction,
    pub lava: DensityFunction,
    pub erosion: DensityFunction,
    pub depth: DensityFunction,
}

/// Which chunk, and how tall the dimension is.
///
/// Grouped rather than passed loose because the four are meaningless apart:
/// the grid is sized from all of them together, and a caller that mixed up
/// `min_block_y` with `y_block_size` would get a plausible aquifer for the
/// wrong slab of world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AquiferBounds {
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// From the noise settings — -64 for the overworld.
    pub min_block_y: i32,
    /// The dimension's height in blocks, 384 for the overworld.
    pub y_block_size: i32,
}

impl NoiseBasedAquifer {
    /// `Aquifer.create`.
    #[must_use]
    pub fn new(
        noises: AquiferNoises,
        flat: &FlatCaches,
        surface: &mut SurfaceLevels,
        bounds: AquiferBounds,
        random: XoroshiroPositionalFactory,
        global: GlobalFluidPicker,
    ) -> Self {
        let AquiferBounds {
            chunk_x,
            chunk_z,
            min_block_y,
            y_block_size,
        } = bounds;
        let min_block_x = chunk_x * 16;
        let min_block_z = chunk_z * 16;
        let max_block_x = min_block_x + 15;
        let max_block_z = min_block_z + 15;

        // The +0 / +1 / -1 terms are vanilla's MIN_/MAX_CELL_SAMPLE_*, kept
        // visible: the grid is padded by one cell on the high side of every
        // axis and one more on the low side of y, because the nearest-centre
        // search below reaches into those neighbours.
        let min_grid_x = grid_x(min_block_x + SAMPLE_OFFSET_X);
        let max_grid_x = grid_x(max_block_x + SAMPLE_OFFSET_X) + 1;
        let grid_size_x = max_grid_x - min_grid_x + 1;

        let min_grid_y = grid_y(min_block_y + SAMPLE_OFFSET_Y) - 1;
        let max_grid_y = grid_y(min_block_y + y_block_size + SAMPLE_OFFSET_Y) + 1;
        let grid_size_y = max_grid_y - min_grid_y + 1;

        let min_grid_z = grid_z(min_block_z + SAMPLE_OFFSET_Z);
        let max_grid_z = grid_z(max_block_z + SAMPLE_OFFSET_Z) + 1;
        let grid_size_z = max_grid_z - min_grid_z + 1;

        let total = (grid_size_x * grid_size_y * grid_size_z) as usize;

        // Above this there is no point sampling at all: the surface is below
        // it everywhere in range, so the global picker's answer is final.
        // Note the ceiling is taken over the *padded* grid, not the chunk.
        let max_adjusted_surface = adjust_surface_level(surface.max_over(
            flat,
            from_grid_x(min_grid_x, 0),
            from_grid_z(min_grid_z, 0),
            from_grid_x(max_grid_x, 9),
            from_grid_z(max_grid_z, 9),
        ));
        let skip_grid_y = grid_y(max_adjusted_surface + Y_SPACING) + 1;
        let skip_sampling_above_y = from_grid_y(skip_grid_y, 11) - 1;

        Self {
            barrier: noises.barrier,
            fluid_level_floodedness: noises.fluid_level_floodedness,
            fluid_level_spread: noises.fluid_level_spread,
            lava: noises.lava,
            erosion: noises.erosion,
            depth: noises.depth,
            random,
            global,
            statuses: vec![None; total],
            locations: vec![None; total],
            min_grid_x,
            min_grid_y,
            min_grid_z,
            grid_size_x,
            grid_size_z,
            skip_sampling_above_y,
            should_schedule_fluid_update: false,
        }
    }

    /// Whether the block just computed needs a fluid tick scheduled.
    ///
    /// Only meaningful immediately after a [`Self::compute_substance`] call,
    /// which is exactly vanilla's contract.
    #[must_use]
    pub const fn should_schedule_fluid_update(&self) -> bool {
        self.should_schedule_fluid_update
    }

    fn index(&self, grid_x: i32, grid_y: i32, grid_z: i32) -> usize {
        let x = grid_x - self.min_grid_x;
        let y = grid_y - self.min_grid_y;
        let z = grid_z - self.min_grid_z;
        ((y * self.grid_size_z + z) * self.grid_size_x + x) as usize
    }

    /// `NoiseBasedAquifer.computeSubstance`.
    ///
    /// `None` means "not mine" — the caller uses its default block, and the
    /// ore veinifier gets a chance at it. See [`super::fill`].
    pub fn compute_substance(
        &mut self,
        flat: &FlatCaches,
        surface: &mut SurfaceLevels,
        pos: (i32, i32, i32),
        density: f64,
    ) -> Option<Substance> {
        let (pos_x, pos_y, pos_z) = pos;
        if density > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        // `RUSTMC_TRACE_AQUIFER=x,y,z[;x,y,z...]` prints which branch of
        // `computeSubstance` answered for one block. Reading cleared every
        // line of this method against the bytecode without finding the
        // fault; the equivalent trace in the surface evaluator settled an
        // identical dead end in one run.
        //
        // A `;`-separated list rather than one position, because the question
        // this answers is almost never about a single block: a fluid level is
        // read from the *pair* of heights either side of it, and a cluster of
        // missing sources is only recognisable as one cause when every member
        // reports in the same run.
        //
        // Read once and cached: `computeSubstance` runs per open block, and
        // the `std::env::var` this line used to be was 592 of 24,097 samples
        // — 2.5% of the r=16 pipeline — with the switch unset.
        let trace = trace_aquifer().is_some_and(|t| {
            let key = format!("{pos_x},{pos_y},{pos_z}");
            t.split(';').any(|want| want.trim() == key)
        });
        macro_rules! note {
            ($($arg:tt)*) => {
                if trace {
                    println!("  aquifer({pos_x},{pos_y},{pos_z}) density={density:.6}: {}", format!($($arg)*));
                }
            };
        }

        let global_fluid = self.global.compute_fluid(pos_y);
        if pos_y > self.skip_sampling_above_y {
            note!(
                "skip_sampling_above_y {} -> global {:?}",
                self.skip_sampling_above_y,
                global_fluid.at(pos_y)
            );
            self.should_schedule_fluid_update = false;
            return Some(global_fluid.at(pos_y));
        }
        if global_fluid.at(pos_y) == Substance::Lava {
            note!("global lava");
            // Deep lava is never barriered — it is the world floor's ocean.
            self.should_schedule_fluid_update = false;
            return Some(Substance::Lava);
        }

        let anchor_x = grid_x(pos_x + SAMPLE_OFFSET_X);
        let anchor_y = grid_y(pos_y + SAMPLE_OFFSET_Y);
        let anchor_z = grid_z(pos_z + SAMPLE_OFFSET_Z);

        // The four nearest aquifer centres, kept as a hand-rolled insertion
        // sort exactly as vanilla writes it. `>=` rather than `>` at each
        // step is deliberate: on a tie the *later* cell in iteration order
        // wins, and the iteration order is x-outer, y, z-inner.
        let mut d1 = i32::MAX;
        let mut d2 = i32::MAX;
        let mut d3 = i32::MAX;
        let mut d4 = i32::MAX;
        let mut i1 = 0usize;
        let mut i2 = 0usize;
        let mut i3 = 0usize;
        let mut i4 = 0usize;
        // Only ever filled under the trace: the whole point of the detail is
        // that a near-tie between two centres is invisible in the winner
        // alone, and the loser's *level* is what says whether the tie went
        // the wrong way or the level itself is wrong.
        let mut candidates: Vec<TracedCentre> = Vec::new();

        for dx in 0..=1 {
            for dy in -1..=1 {
                for dz in 0..=1 {
                    let gx = anchor_x + dx;
                    let gy = anchor_y + dy;
                    let gz = anchor_z + dz;
                    let index = self.index(gx, gy, gz);
                    let location = match self.locations[index] {
                        Some(loc) => loc,
                        None => {
                            let mut rng = self.random.at(gx, gy, gz);
                            // Three draws, in x, y, z order, from one
                            // positional source. Bounds 10/9/10 keep the
                            // centre inside its own cell.
                            let loc = (
                                from_grid_x(gx, rng.next_i32_bound(10)),
                                from_grid_y(gy, rng.next_i32_bound(9)),
                                from_grid_z(gz, rng.next_i32_bound(10)),
                            );
                            self.locations[index] = Some(loc);
                            loc
                        }
                    };

                    let ddx = location.0 - pos_x;
                    let ddy = location.1 - pos_y;
                    let ddz = location.2 - pos_z;
                    let distance = ddx * ddx + ddy * ddy + ddz * ddz;
                    if trace {
                        candidates.push(TracedCentre {
                            cell: (gx, gy, gz),
                            centre: location,
                            distance_sqr: distance,
                            index,
                        });
                    }
                    if d1 >= distance {
                        i4 = i3;
                        i3 = i2;
                        i2 = i1;
                        i1 = index;
                        d4 = d3;
                        d3 = d2;
                        d2 = d1;
                        d1 = distance;
                    } else if d2 >= distance {
                        i4 = i3;
                        i3 = i2;
                        i2 = index;
                        d4 = d3;
                        d3 = d2;
                        d2 = distance;
                    } else if d3 >= distance {
                        i4 = i3;
                        i3 = index;
                        d4 = d3;
                        d3 = distance;
                    } else if d4 >= distance {
                        i4 = index;
                        d4 = distance;
                    }
                }
            }
        }

        let status1 = self.status(flat, surface, i1);
        let sim12 = similarity(d1, d2);
        let fluid_here = status1.at(pos_y);
        let threshold = flowing_update_similarity();

        note!(
            "centres d1={d1} d2={d2} sim12={sim12:.6} fluid_here={fluid_here:?} \
             status1.level={}",
            status1.level
        );
        if trace {
            let mut rows = std::mem::take(&mut candidates);
            rows.sort_by_key(|c| c.distance_sqr);
            for row in rows {
                let TracedCentre {
                    cell: (gx, gy, gz),
                    centre: loc,
                    distance_sqr: dist,
                    index,
                } = row;
                let st = self.status(flat, surface, index);
                let rank = if index == i1 {
                    "1"
                } else if index == i2 {
                    "2"
                } else if index == i3 {
                    "3"
                } else if index == i4 {
                    "4"
                } else {
                    "."
                };
                note!(
                    "  cand[{rank}] cell=({gx},{gy},{gz}) centre=({},{},{}) d2={dist} \
                     level={} fluid={:?} at_here={:?}",
                    loc.0,
                    loc.1,
                    loc.2,
                    st.level,
                    st.fluid,
                    st.at(pos_y)
                );
            }
        }
        if sim12 <= 0.0 {
            // Well inside one aquifer: no neighbour close enough to argue.
            self.should_schedule_fluid_update = if sim12 >= threshold {
                let status2 = self.status(flat, surface, i2);
                status1 != status2
            } else {
                false
            };
            return Some(fluid_here);
        }

        if fluid_here == Substance::Water
            && self.global.compute_fluid(pos_y - 1).at(pos_y - 1) == Substance::Lava
        {
            // Water sitting directly on the global lava sea: always ticks,
            // and never gets a barrier — this is where obsidian forms.
            self.should_schedule_fluid_update = true;
            return Some(fluid_here);
        }

        // Contested. Build a barrier if the pressure between this centre and
        // each of the next three is enough to close the gap.
        let mut barrier_noise = None;
        let status2 = self.status(flat, surface, i2);
        let pressure12 = sim12 * self.pressure(&mut barrier_noise, pos, status1, status2);
        if density + pressure12 > 0.0 {
            self.should_schedule_fluid_update = false;
            return None;
        }

        let status3 = self.status(flat, surface, i3);
        let sim13 = similarity(d1, d3);
        if sim13 > 0.0 {
            let pressure13 =
                sim12 * sim13 * self.pressure(&mut barrier_noise, pos, status1, status3);
            if density + pressure13 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        let sim23 = similarity(d2, d3);
        if sim23 > 0.0 {
            let pressure23 =
                sim12 * sim23 * self.pressure(&mut barrier_noise, pos, status2, status3);
            if density + pressure23 > 0.0 {
                self.should_schedule_fluid_update = false;
                return None;
            }
        }

        let may_flow_12 = status1 != status2;
        let may_flow_23 = sim23 >= threshold && status2 != status3;
        let may_flow_13 = sim13 >= threshold && status1 != status3;
        self.should_schedule_fluid_update = if may_flow_12 || may_flow_23 || may_flow_13 {
            true
        } else {
            // Only now is the fourth neighbour worth the surface scan.
            sim13 >= threshold
                && similarity(d1, d4) >= threshold
                && status1 != self.status(flat, surface, i4)
        };

        if trace {
            println!(
                "  aquifer({pos_x},{pos_y},{pos_z}) -> {fluid_here:?} \
                 should_schedule_fluid_update={}",
                self.should_schedule_fluid_update
            );
        }
        Some(fluid_here)
    }

    /// `calculatePressure` — how hard the two water tables push a wall of
    /// stone between themselves.
    ///
    /// `barrier_noise` is vanilla's `MutableDouble` seeded with NaN: the
    /// barrier noise is sampled at most once per `computeSubstance` call, no
    /// matter how many neighbour pairs are tested, and skipped entirely when
    /// the gradient is already past +/-2 and the noise cannot change the
    /// sign. `Option` says the same thing without borrowing NaN as a flag.
    fn pressure(
        &self,
        barrier_noise: &mut Option<f64>,
        pos: (i32, i32, i32),
        first: FluidStatus,
        second: FluidStatus,
    ) -> f64 {
        let pos_y = pos.1;
        let type1 = first.at(pos_y);
        let type2 = second.at(pos_y);

        // Lava meeting water always gets a full-strength barrier, whatever
        // the levels say. Without it the two would touch and generate
        // stone/obsidian at world-gen time instead of at tick time.
        let lava_water = (type1 == Substance::Lava && type2 == Substance::Water)
            || (type1 == Substance::Water && type2 == Substance::Lava);
        if lava_water {
            return 2.0;
        }

        let fluid_y_diff = (first.level - second.level).abs();
        if fluid_y_diff == 0 {
            return 0.0;
        }

        let average_fluid_y = 0.5 * f64::from(first.level + second.level);
        let above_average = f64::from(pos_y) + 0.5 - average_fluid_y;
        let base = f64::from(fluid_y_diff) / 2.0;
        let toward_middle = base - above_average.abs();

        // Asymmetric on purpose: the biases above and below the average
        // fluid level differ (0/2.5/1.5 against 3/10/3), which is what makes
        // barriers hang below a waterline rather than sit centred on it.
        let gradient = if above_average > 0.0 {
            let centre = 0.0 + toward_middle;
            if centre > 0.0 {
                centre / 1.5
            } else {
                centre / 2.5
            }
        } else {
            let centre = 3.0 + toward_middle;
            if centre > 0.0 {
                centre / 3.0
            } else {
                centre / 10.0
            }
        };

        // Vanilla writes this as `!(gradient < -2.0) && !(gradient > 2.0)`,
        // which is not the same as a range check: on NaN both comparisons
        // are false, so vanilla samples the noise where `!(-2.0..=2.0)
        // .contains(&gradient)` would return 0.0. The inputs here are all
        // finite integer arithmetic so NaN cannot actually arrive, but
        // swapping in the range form would be a silent semantic change for
        // a lint's sake.
        #[allow(clippy::manual_range_contains)]
        let noise_value = if gradient < -2.0 || gradient > 2.0 {
            0.0
        } else {
            *barrier_noise.get_or_insert_with(|| {
                self.barrier
                    .compute(FunctionContext::new(pos.0, pos.1, pos.2))
            })
        };

        2.0 * (noise_value + gradient)
    }

    fn status(
        &mut self,
        flat: &FlatCaches,
        surface: &mut SurfaceLevels,
        index: usize,
    ) -> FluidStatus {
        if let Some(status) = self.statuses[index] {
            return status;
        }
        let location =
            self.locations[index].expect("status asked for before its location was drawn");
        let status = self.compute_fluid(flat, surface, location);
        self.statuses[index] = Some(status);
        status
    }

    /// `computeFluid` — decide one aquifer centre's fluid and level.
    fn compute_fluid(
        &self,
        flat: &FlatCaches,
        surface: &mut SurfaceLevels,
        at: (i32, i32, i32),
    ) -> FluidStatus {
        let (x, y, z) = at;
        let trace = trace_aquifer_centre().is_some_and(|t| {
            let key = format!("{x},{y},{z}");
            t.split(';').any(|want| want.trim() == key)
        });
        macro_rules! note {
            ($($arg:tt)*) => {
                if trace {
                    println!("  centre({x},{y},{z}): {}", format!($($arg)*));
                }
            };
        }
        let global_fluid = self.global.compute_fluid(y);
        let mut lowest_preliminary_surface = i32::MAX;
        let top_of_cell = y + Y_SPACING;
        let bottom_of_cell = y - Y_SPACING;
        let mut surface_at_centre_is_under_global_fluid = false;

        for offset in SURFACE_SAMPLING_OFFSETS_IN_CHUNKS {
            let sample_x = x + (offset[0] << 4);
            let sample_z = z + (offset[1] << 4);
            let preliminary = surface.at(flat, sample_x, sample_z);
            let adjusted = adjust_surface_level(preliminary);
            let is_centre = offset[0] == 0 && offset[1] == 0;
            note!(
                "offset {:?} sample=({sample_x},{sample_z}) preliminary={preliminary} \
                 adjusted={adjusted} is_centre={is_centre}",
                offset
            );

            // Buried deep under its own column: nothing local can matter, so
            // take the global answer and stop. Checked only at the centre,
            // which is why the centre must be first in the offset table.
            if is_centre && bottom_of_cell > adjusted {
                note!("buried: bottom_of_cell {bottom_of_cell} > adjusted {adjusted} -> global");
                return global_fluid;
            }

            let pokes_above_surface = top_of_cell > adjusted;
            if pokes_above_surface || is_centre {
                let at_surface = self.global.compute_fluid(adjusted);
                if at_surface.at(adjusted) != Substance::Air {
                    if is_centre {
                        surface_at_centre_is_under_global_fluid = true;
                    }
                    if pokes_above_surface {
                        // The cell breaks the surface under an ocean, so it
                        // simply is the ocean.
                        return at_surface;
                    }
                }
            }

            lowest_preliminary_surface = lowest_preliminary_surface.min(preliminary);
        }

        let level = self.surface_level(
            flat,
            at,
            global_fluid,
            lowest_preliminary_surface,
            surface_at_centre_is_under_global_fluid,
        );
        note!(
            "lowest_preliminary={lowest_preliminary_surface} \
             under_global_fluid={surface_at_centre_is_under_global_fluid} \
             global.level={} -> level={level} target={}",
            global_fluid.level,
            self.randomized_fluid_surface_level(at, i32::MAX)
        );
        FluidStatus::new(level, self.fluid_type(at, global_fluid, level))
    }

    /// `computeSurfaceLevel` — how full this aquifer is.
    fn surface_level(
        &self,
        flat: &FlatCaches,
        at: (i32, i32, i32),
        global_fluid: FluidStatus,
        lowest_preliminary_surface: i32,
        surface_at_centre_is_under_global_fluid: bool,
    ) -> i32 {
        let (x, y, z) = at;
        // `new SinglePointContext(x, y, z)` — a bare point, **not** the flat
        // caches. A `flat_cache` node inside `erosion`, `depth` or
        // `fluid_level_floodedness` otherwise serves the value for whichever
        // cell the cache was primed on, and this is asked at aquifer centres
        // that sit well outside the chunk being filled.
        let _ = flat;
        let ctx = FunctionContext::new(x, y, z);

        let (partially_flooded, fully_flooded) = if self.is_deep_dark_region(ctx) {
            // The deep dark is dry by construction — an ancient city half
            // full of water would be unplayable.
            (-1.0, -1.0)
        } else {
            let depth_below_surface = lowest_preliminary_surface + SURFACE_LEVEL_ADJUSTMENT - y;
            let floodedness_factor = if surface_at_centre_is_under_global_fluid {
                math::clamped_map(f64::from(depth_below_surface), 0.0, 64.0, 1.0, 0.0)
            } else {
                0.0
            };
            let noise = math::clamp(self.fluid_level_floodedness.compute(ctx), -1.0, 1.0);
            // Both thresholds run 1 -> 0, i.e. shallower water under an
            // ocean floods more readily than deep inland stone.
            let fully_threshold = math::map(floodedness_factor, 1.0, 0.0, -0.3, 0.8);
            let partially_threshold = math::map(floodedness_factor, 1.0, 0.0, -0.8, 0.4);
            (noise - partially_threshold, noise - fully_threshold)
        };

        if fully_flooded > 0.0 {
            global_fluid.level
        } else if partially_flooded > 0.0 {
            self.randomized_fluid_surface_level(at, lowest_preliminary_surface)
        } else {
            // Not a height — the "no fluid at all" sentinel.
            WAY_BELOW_MIN_Y
        }
    }

    /// `OverworldBiomeBuilder.isDeepDarkRegion`.
    ///
    /// Both constants are `float` literals in vanilla, widened to double at
    /// the comparison: -0.225F is -0.22499999403953552 and 0.9F is
    /// 0.8999999761581421. Writing them as doubles would move the boundary
    /// of every ancient city.
    fn is_deep_dark_region(&self, ctx: FunctionContext) -> bool {
        self.erosion.compute(ctx) < f64::from(-0.225_f32)
            && self.depth.compute(ctx) > f64::from(0.9_f32)
    }

    /// `computeRandomizedFluidSurfaceLevel` — a partially flooded aquifer's
    /// waterline, jittered on a coarse 16x40x16 grid.
    ///
    /// The spread noise is sampled at *cell* coordinates, not block ones, so
    /// a whole 16x40x16 box shares one waterline. That is what makes cave
    /// lakes flat.
    fn randomized_fluid_surface_level(
        &self,
        at: (i32, i32, i32),
        lowest_preliminary_surface: i32,
    ) -> i32 {
        const FLUID_CELL_WIDTH: i32 = 16;
        const FLUID_CELL_HEIGHT: i32 = 40;
        const MAX_SPREAD: f64 = 10.0;

        let (x, y, z) = at;
        let cell_x = x.div_euclid(FLUID_CELL_WIDTH);
        let cell_y = y.div_euclid(FLUID_CELL_HEIGHT);
        let cell_z = z.div_euclid(FLUID_CELL_WIDTH);
        let cell_middle_y = cell_y * FLUID_CELL_HEIGHT + FLUID_CELL_HEIGHT / 2;

        let spread = self
            .fluid_level_spread
            .compute(FunctionContext::new(cell_x, cell_y, cell_z))
            * MAX_SPREAD;
        // Quantised to 3 blocks, so waterlines step rather than slide.
        let target = cell_middle_y + math::quantize(spread, 3);
        lowest_preliminary_surface.min(target)
    }

    /// `computeFluidType` — deep, isolated aquifers can be lava instead.
    fn fluid_type(
        &self,
        at: (i32, i32, i32),
        global_fluid: FluidStatus,
        fluid_surface_level: i32,
    ) -> Substance {
        const LAVA_CELL_WIDTH: i32 = 64;
        const LAVA_CELL_HEIGHT: i32 = 40;

        let (x, y, z) = at;
        if fluid_surface_level > -10
            || fluid_surface_level == WAY_BELOW_MIN_Y
            || global_fluid.fluid == Substance::Lava
        {
            return global_fluid.fluid;
        }
        let cell_x = x.div_euclid(LAVA_CELL_WIDTH);
        let cell_y = y.div_euclid(LAVA_CELL_HEIGHT);
        let cell_z = z.div_euclid(LAVA_CELL_WIDTH);
        let lava_noise = self
            .lava
            .compute(FunctionContext::new(cell_x, cell_y, cell_z));
        if lava_noise.abs() > 0.3 {
            Substance::Lava
        } else {
            global_fluid.fluid
        }
    }
}

/// `adjustSurfaceLevel` — the aquifer works 8 blocks above the estimate.
#[must_use]
const fn adjust_surface_level(preliminary_surface_level: i32) -> i32 {
    preliminary_surface_level + SURFACE_LEVEL_ADJUSTMENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_mapping_floors_on_negatives() {
        // The bug this guards: truncating division would put -1 in cell 0.
        assert_eq!(grid_x(0), 0);
        assert_eq!(grid_x(15), 0);
        assert_eq!(grid_x(16), 1);
        assert_eq!(grid_x(-1), -1, "block -1 is in cell -1, not 0");
        assert_eq!(grid_x(-16), -1);
        assert_eq!(grid_x(-17), -2);

        assert_eq!(grid_y(0), 0);
        assert_eq!(grid_y(11), 0);
        assert_eq!(grid_y(12), 1);
        assert_eq!(grid_y(-1), -1, "block -1 is in cell -1, not 0");
        assert_eq!(grid_y(-12), -1);
        assert_eq!(grid_y(-13), -2);
    }

    #[test]
    fn grid_mapping_matches_truncation_only_where_it_should() {
        // Every non-negative block must agree with plain division; every
        // negative one that is not an exact multiple must not.
        for block in 0..64 {
            assert_eq!(grid_x(block), block / 16);
            assert_eq!(grid_y(block), block / 12);
        }
        for block in -64..0 {
            let floored = grid_x(block);
            let truncated = block / 16;
            if block % 16 == 0 {
                assert_eq!(floored, truncated);
            } else {
                assert_eq!(floored, truncated - 1, "block {block}");
            }
        }
    }

    #[test]
    fn from_grid_is_the_left_inverse_of_grid() {
        for cell in -8..8 {
            assert_eq!(grid_x(from_grid_x(cell, 0)), cell);
            assert_eq!(grid_y(from_grid_y(cell, 0)), cell);
            assert_eq!(grid_z(from_grid_z(cell, 0)), cell);
        }
    }

    #[test]
    fn jitter_stays_inside_the_cell() {
        // A separation at or above the spacing would let a centre land in a
        // neighbouring cell, and the nearest-two search would miss it.
        // Compared through variables so this is a real assertion rather
        // than a constant clippy can fold away.
        let separations = [X_SEPARATION, Y_SEPARATION, Z_SEPARATION];
        let spacings = [X_SPACING, Y_SPACING, Z_SPACING];
        for (separation, spacing) in separations.into_iter().zip(spacings) {
            assert!(
                separation < spacing,
                "separation {separation} must stay under spacing {spacing}"
            );
        }
    }

    #[test]
    fn similarity_is_one_when_equidistant_and_signed_beyond_the_threshold() {
        assert_eq!(similarity(100, 100), 1.0);
        // Falls off linearly with the gap, and crosses zero at 25.
        assert_eq!(similarity(0, 25), 0.0);
        assert!(similarity(0, 26) < 0.0);
        // Deliberately unclamped: a nearer second centre exceeds 1.
        assert!(similarity(50, 0) > 1.0);
    }

    #[test]
    fn flowing_update_similarity_is_negative() {
        // 1 - (144 - 100)/25 = -0.76. The sign is what matters: at that
        // separation the two tables do not merge.
        assert!(flowing_update_similarity() < 0.0);
        assert!((flowing_update_similarity() - (-0.76)).abs() < 1e-12);
    }

    // -------------------------------------------------- the state machine

    use crate::density::build;
    use crate::density::surface_level::SurfaceLevels;
    use crate::rng::XoroshiroRandomSource;

    const SEA_LEVEL: i32 = 63;
    const MIN_Y: i32 = -64;
    const HEIGHT: i32 = 384;

    /// A flat world at y=70, so every surface query is predictable and the
    /// aquifer's geometry is the only thing under test.
    fn levels(surface: f64) -> SurfaceLevels {
        SurfaceLevels::new(build::constant(surface))
    }

    fn noises(floodedness: f64, spread: f64, lava: f64) -> AquiferNoises {
        AquiferNoises {
            barrier: build::constant(0.0),
            fluid_level_floodedness: build::constant(floodedness),
            fluid_level_spread: build::constant(spread),
            lava: build::constant(lava),
            // Erosion low + depth high would be deep dark; keep it clear of
            // that so the ordinary path is exercised.
            erosion: build::constant(0.0),
            depth: build::constant(0.0),
        }
    }

    fn aquifer(surface: f64, noises: AquiferNoises) -> (NoiseBasedAquifer, SurfaceLevels) {
        let mut levels = levels(surface);
        let factory = XoroshiroRandomSource::from_seed(20260724).fork_positional();
        let a = NoiseBasedAquifer::new(
            noises,
            &FlatCaches::none(),
            &mut levels,
            AquiferBounds {
                chunk_x: 0,
                chunk_z: 0,
                min_block_y: MIN_Y,
                y_block_size: HEIGHT,
            },
            factory,
            GlobalFluidPicker::overworld(SEA_LEVEL),
        );
        (a, levels)
    }

    #[test]
    fn positive_density_is_declined_without_touching_any_state() {
        let (mut a, mut levels) = aquifer(70.0, noises(0.0, 0.0, 0.0));
        assert_eq!(
            a.compute_substance(&FlatCaches::none(), &mut levels, (3, 20, 5), 0.0001),
            None
        );
        assert!(!a.should_schedule_fluid_update());
    }

    #[test]
    fn every_block_in_the_chunk_is_reachable_without_an_index_panic() {
        // The grid is padded by a cell on the high side of each axis and one
        // more below on y, because the nearest-centre search reaches into
        // those neighbours. If that padding is off by one, some block in the
        // chunk indexes out of bounds — which is a panic, not a wrong world,
        // so a sweep is a real test.
        let (mut a, mut levels) = aquifer(70.0, noises(0.5, 0.0, 0.0));
        for x in 0..16 {
            for z in 0..16 {
                for y in (MIN_Y..MIN_Y + HEIGHT).step_by(7) {
                    let _ = a.compute_substance(&FlatCaches::none(), &mut levels, (x, y, z), -1.0);
                }
            }
        }
    }

    #[test]
    fn well_above_the_surface_the_global_picker_decides() {
        // Past skip_sampling_above_y there is no local water table to find,
        // so the answer must be exactly sea level's — air above 63, water
        // below. Anything else means the skip threshold is misplaced.
        let (mut a, mut levels) = aquifer(70.0, noises(0.9, 0.0, 0.0));
        assert_eq!(
            a.compute_substance(&FlatCaches::none(), &mut levels, (8, 300, 8), -1.0),
            Some(Substance::Air)
        );
        assert!(!a.should_schedule_fluid_update());
    }

    #[test]
    fn the_deep_is_lava_and_never_barriered() {
        // Below the lava level the global picker wins outright: vanilla
        // returns before any aquifer sampling at all.
        let (mut a, mut levels) = aquifer(70.0, noises(0.9, 0.0, 0.0));
        assert_eq!(
            a.compute_substance(&FlatCaches::none(), &mut levels, (8, -60, 8), -1.0),
            Some(Substance::Lava)
        );
        assert!(!a.should_schedule_fluid_update());
    }

    #[test]
    fn results_are_deterministic_across_instances() {
        // Two aquifers built the same way must agree block for block —
        // the positional RNG is the only source of variation and it is
        // keyed by cell, not by call order.
        let sample = |()| {
            let (mut a, mut levels) = aquifer(70.0, noises(0.2, 0.4, 0.0));
            (0..40)
                .map(|i| {
                    let y = -50 + i * 8;
                    a.compute_substance(&FlatCaches::none(), &mut levels, (5, y, 11), -0.5)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(sample(()), sample(()));
    }

    #[test]
    fn the_cell_cache_does_not_change_the_answer() {
        // Querying a column top-down and bottom-up must agree. The status
        // and location caches fill in a different order each way, so a
        // cache that leaked state would show up here.
        let noise = || noises(0.2, 0.4, 0.0);
        let (mut a, mut levels) = aquifer(70.0, noise());
        let down: Vec<_> = (0..30)
            .map(|i| {
                a.compute_substance(&FlatCaches::none(), &mut levels, (7, 60 - i * 4, 9), -0.5)
            })
            .collect();

        let (mut b, mut levels_b) = aquifer(70.0, noise());
        let mut up: Vec<_> = (0..30)
            .rev()
            .map(|i| {
                b.compute_substance(&FlatCaches::none(), &mut levels_b, (7, 60 - i * 4, 9), -0.5)
            })
            .collect();
        up.reverse();
        assert_eq!(down, up);
    }

    #[test]
    fn pressure_is_zero_between_equal_water_tables() {
        // Same level means no barrier, whatever the noise says — vanilla
        // returns before sampling it.
        let (a, _) = aquifer(70.0, noises(0.0, 0.0, 0.0));
        let status = FluidStatus::new(40, Substance::Water);
        let mut noise = None;
        assert_eq!(a.pressure(&mut noise, (0, 30, 0), status, status), 0.0);
        assert!(noise.is_none(), "barrier noise sampled unnecessarily");
    }

    #[test]
    fn lava_meeting_water_is_always_a_full_barrier() {
        // 2.0 regardless of levels or noise: without it the two would touch
        // at generation time instead of forming stone on the first tick.
        let (a, _) = aquifer(70.0, noises(0.0, 0.0, 0.0));
        let water = FluidStatus::new(40, Substance::Water);
        let lava = FluidStatus::new(40, Substance::Lava);
        let mut noise = None;
        assert_eq!(a.pressure(&mut noise, (0, 30, 0), lava, water), 2.0);
        assert_eq!(a.pressure(&mut noise, (0, 30, 0), water, lava), 2.0);
        assert!(noise.is_none());
    }

    #[test]
    fn the_barrier_noise_is_sampled_at_most_once_per_block() {
        // vanilla threads a MutableDouble seeded with NaN through every
        // neighbour pair for exactly this reason. Three pairs, one sample.
        let (a, _) = aquifer(70.0, noises(0.0, 0.0, 0.0));
        let low = FluidStatus::new(30, Substance::Water);
        let high = FluidStatus::new(34, Substance::Water);
        let mut noise = None;
        let first = a.pressure(&mut noise, (0, 32, 0), low, high);
        assert!(noise.is_some(), "expected the noise to be sampled once");
        let cached = noise;
        let second = a.pressure(&mut noise, (0, 32, 0), low, high);
        assert_eq!(first, second);
        assert_eq!(noise, cached, "the cached sample was replaced");
    }

    #[test]
    fn a_dry_aquifer_uses_the_sentinel_not_a_height() {
        // Floodedness noise below both thresholds means "no fluid here at
        // all", encoded as WAY_BELOW_MIN_Y so `blockY < level` can never
        // pass. A real height would flood the whole cell.
        let (a, mut levels) = aquifer(70.0, noises(-1.0, 0.0, 0.0));
        let _ = &mut levels;
        let status = a.compute_fluid(&FlatCaches::none(), &mut levels, (8, 0, 8));
        assert_eq!(status.level, WAY_BELOW_MIN_Y);
        assert_eq!(status.at(-64), Substance::Air);
        assert_eq!(status.at(0), Substance::Air);
    }

    #[test]
    fn deep_dark_regions_are_always_dry() {
        // erosion < -0.225f and depth > 0.9f. Ancient cities half full of
        // water would be unplayable, so vanilla forces both floodedness
        // measures negative there.
        let mut levels = levels(70.0);
        let factory = XoroshiroRandomSource::from_seed(1).fork_positional();
        let a = NoiseBasedAquifer::new(
            AquiferNoises {
                barrier: build::constant(0.0),
                // Just inside the deep-dark test on both sides.
                erosion: build::constant(-0.3),
                depth: build::constant(1.0),
                // Floodedness that would otherwise flood it completely.
                fluid_level_floodedness: build::constant(1.0),
                fluid_level_spread: build::constant(0.0),
                lava: build::constant(0.0),
            },
            &FlatCaches::none(),
            &mut levels,
            AquiferBounds {
                chunk_x: 0,
                chunk_z: 0,
                min_block_y: MIN_Y,
                y_block_size: HEIGHT,
            },
            factory,
            GlobalFluidPicker::overworld(SEA_LEVEL),
        );
        let status = a.compute_fluid(&FlatCaches::none(), &mut levels, (8, 0, 8));
        assert_eq!(status.level, WAY_BELOW_MIN_Y, "the deep dark flooded");
    }

    #[test]
    fn the_deep_dark_test_uses_widened_float_constants() {
        // -0.225F widens to -0.22499999403953552, which is *less* negative
        // than -0.225. So there is a band of erosion values that vanilla
        // calls deep dark and a naive -0.225 does not, and this is one:
        let erosion = -0.224_999_995_f64;
        assert!(erosion < f64::from(-0.225_f32), "vanilla: deep dark");
        assert!(erosion > -0.225_f64, "naive constant: not deep dark");

        // Same shape on the depth side, mirrored: 0.9F is 0.8999999761581421,
        // so vanilla's `> 0.9F` admits values a naive `> 0.9` rejects.
        let depth = 0.899_999_98_f64;
        assert!(depth > f64::from(0.9_f32), "vanilla: deep enough");
        assert!(depth < 0.9_f64, "naive constant: not deep enough");
    }
}
