//! The stages, composed.
//!
//! Every stage of this generator has been proved on its own — noise against
//! 1.77M blocks, surfaces against dumped palettes, carvers against carved
//! chunks, decoration against 17,478 blocks over eleven steps. What none of
//! those gates can see is the seam *between* them: a heightmap carried
//! forward stale, a biome container filled after the stage that reads it, a
//! chunk decorated before its neighbour was carved. Those show up only when
//! the stages run in one process against one world, which is what this
//! module is for.
//!
//! # The order is the contract
//!
//! ```text
//! for every chunk in the canvas, in x-then-z order:
//!     noise fill
//!     biomes
//! for every chunk:
//!     surface
//! for every chunk:
//!     carvers
//! for every chunk:
//!     prime heightmaps, then decorate
//! ```
//!
//! Stage-major, not chunk-major, and that is load-bearing rather than tidy.
//! Carvers read a 17x17 neighbourhood, so every chunk in reach must have been
//! noise-filled first; decoration writes into its neighbours, so every chunk
//! in reach must have been carved first. A generator that finished one chunk
//! before starting the next would read half-built neighbours and produce a
//! world that depends on its own visiting order — ORACLE.md §6, bought once
//! already.
//!
//! # A margin, and what it costs
//!
//! A chunk is only correct if every chunk that can write into it has been
//! decorated **correctly**, and that second word is where the margin comes
//! from. One ring covers everything that can write inward; it does not make
//! that ring's own decoration right, because decoration reads neighbours and
//! the ring's outward neighbours do not exist. So the ring writes its
//! mistakes inward. [`DECORATION_MARGIN`] carries the measurement that
//! settled the width.
//!
//! Only the inner square is offered as output; [`Pipeline::inner`] is the
//! honest extent. Vanilla achieves the same thing with chunk statuses and a
//! load radius — the shape is different, the requirement is not.
//!
//! # What is not here yet
//!
//! Structures are not driven by *this module*, and that is a statement about
//! where the code lives rather than about what runs. The driver is
//! `worldgen_assembly::generate::generate_resuming`, which lays six overworld
//! structure sets out **before** the noise stage — `ChunkPyramid` orders
//! `STRUCTURE_STARTS` ahead of `NOISE` because a structure beards the terrain
//! around it — and then places their pieces inside the decoration stage. Both
//! `tools/worldgen-dump` and `crates/rustmc-server`'s `World` reach the stages
//! through that one function, so neither of them is missing villages.
//!
//! This paragraph used to say "structures ... are not here", full stop, and
//! two documents and a server module quoted it as evidence the running server
//! had none. It did have them. `crates/rustmc-server/tests/served_structures.rs`
//! is the measurement.
//!
//! What is genuinely absent here is three of the twenty structure sets: the
//! nether's and the End's, which their own dimensions' entry points build.

use crate::canvas::{Canvas, HeightmapPredicates};

/// How far outside the requested area to generate so the inside is correct.
///
/// **Two, and it was one until 2026-07-27.** The old reasoning was that
/// decoration writes at most one chunk away — vanilla's `WorldGenRegion` for
/// features is a 3x3 — so one ring is enough to cover everything that can
/// write into the requested area. That much is true, and it is not
/// sufficient, because it only accounts for where a feature *writes*.
///
/// A margin chunk also has to decorate **correctly**, and decoration reads
/// its neighbours. With a one-chunk margin the ring's own outward neighbours
/// were never generated, so it decorates against an unfinished world — a
/// scan finds a floor that should not be there, an `isEmptyOrWater` answers
/// about void — and then it writes that mistake inward, into the very area
/// the margin exists to protect.
///
/// Measured rather than argued, seed 20260722, the 25-chunk window at
/// (64,64), diffing our own output at three widths:
///
/// ```text
///   1 chunk of context vs 2    604 blocks differ
///   2 chunks vs 3                0 blocks differ   <- converged
/// ```
///
/// One was wrong by more blocks than the window's entire disagreement with
/// vanilla, and every parity number taken before that was measuring its own
/// margin as much as the generator.
///
/// **"Two is where it stops moving" is what this said until 2026-08-12, and
/// it was one seed's answer read as the constant's.** That measurement was
/// taken at seed 20260722, where the property does hold — re-checked out to a
/// 10-ring context and 225 delivered chunks, still 0. At seed 12345 the same
/// comparison moves 40 blocks in the delivered chunk at a 2-ring context, 56
/// at 3, 41 at 4, 29 at 5, 33 at 6 and 33 at 7; at seed 0 the delivered
/// *square* is never still, 45,145 blocks at a 10-ring context.
///
/// **There is no width that converges, and it is the shape rather than the
/// number.** A tile of radius `r` decorates out to `r + m` and delivers out
/// to `r`, so its rim is at depth `m` below the outermost decorated ring
/// **whatever m is**: the ring decorates against a world that stops at its
/// own edge, writes that inward, and widening the margin moves the rim out
/// with it. Injected at 1, 2 and 4 the gate's total is 5,753, 2,201 and 1,130
/// — decreasing, never zero. `docs/DETERMINISM.md` §31,
/// `crates/rustmc-server/tests/tiled_world.rs`.
///
/// So two is not a converged width; it is the width whose cost is bearable,
/// and the fix is generate-once status-based generation over one canvas
/// rather than a bigger constant. The cost is the reason to name it here: the
/// generated area is `(2r + 2m + 1)^2`, so a 25-chunk request goes from 49
/// chunks to 81.
///
/// **SUPERSEDED 2026-08-12 AND KEPT ONLY FOR THE COMPARISON.** This constant
/// now governs the extent of one thing: the shared-canvas model that
/// `RUSTMC_DECORATE_ISOLATED=0` restores, which exists so §31's numbers can
/// be re-taken in the same binary as §32's. The shipped model is the
/// pyramid — see [`isolated_decoration`] and [`Pipeline::decorated`] — where
/// the extent is not a margin at all but two different radii read off
/// `ChunkStep.getAccumulatedRadiusOf` in [`crate::status`], and where the
/// outermost generated ring deliberately does **not** decorate. Do not add
/// to this number; it has no converged value and `docs/DETERMINISM.md` §31.3
/// is the measurement that says so.
pub const DECORATION_MARGIN: i32 = 2;

/// How far one chunk's decoration reaches, in chunks — reads and writes both.
///
/// This is the constant the **parallel** decoration pass rests on. Two chunks
/// more than `2 * DECORATE_REACH` apart cannot see or overwrite each other's
/// work, so the sequential decoration order says nothing about their relative
/// order and they may run at the same time. Set it too small and a feature
/// reads a chunk its group does not hold, which answers the void — silently.
/// So it is not trusted: `Canvas::outside_accesses` counts exactly those, and
/// the sum over the detached groups must equal a whole-canvas run's count on
/// the same square, where the only misses are the true edge.
///
/// Two, not one, because that is [`DECORATION_MARGIN`] and the margin is the
/// measured reach of a chunk's influence. It is deliberately one step more
/// conservative than vanilla's own bound — `WorldGenRegion` refuses a write
/// more than one chunk from the centre — because the margin was raised from
/// one to two on evidence, and the cost of being generous here is parallelism
/// rather than correctness. `RUSTMC_DECORATE_REACH` overrides it, which is
/// how the two are compared.
pub const DECORATE_REACH: i32 = 2;

/// Which stages to run.
///
/// A generator that stops early is useful — the noise stage alone is what the
/// benchmark times — but a *comparison* against vanilla is only meaningful
/// with all of them, so this is explicit rather than a set of booleans
/// scattered through the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Through {
    /// Density, aquifers, ore veins, block fill.
    Noise,
    /// ...and the biome containers.
    Biomes,
    /// ...and the surface rules.
    Surface,
    /// ...and caves and canyons.
    Carvers,
    /// ...and the eleven decoration steps. The only setting a parity number
    /// may be taken from.
    Decoration,
}

/// A chunk area to generate, in chunk coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub min_chunk_x: i32,
    pub min_chunk_z: i32,
    pub max_chunk_x: i32,
    pub max_chunk_z: i32,
}

impl Area {
    /// The square of `radius` chunks around a centre.
    #[must_use]
    pub const fn square(center_x: i32, center_z: i32, radius: i32) -> Self {
        Self {
            min_chunk_x: center_x - radius,
            min_chunk_z: center_z - radius,
            max_chunk_x: center_x + radius,
            max_chunk_z: center_z + radius,
        }
    }

    /// Grown by `margin` chunks on every side.
    #[must_use]
    pub const fn grown(self, margin: i32) -> Self {
        Self {
            min_chunk_x: self.min_chunk_x - margin,
            min_chunk_z: self.min_chunk_z - margin,
            max_chunk_x: self.max_chunk_x + margin,
            max_chunk_z: self.max_chunk_z + margin,
        }
    }

    #[must_use]
    pub const fn contains(self, chunk_x: i32, chunk_z: i32) -> bool {
        chunk_x >= self.min_chunk_x
            && chunk_x <= self.max_chunk_x
            && chunk_z >= self.min_chunk_z
            && chunk_z <= self.max_chunk_z
    }

    #[must_use]
    pub const fn chunk_count(self) -> usize {
        let w = (self.max_chunk_x - self.min_chunk_x + 1) as usize;
        let h = (self.max_chunk_z - self.min_chunk_z + 1) as usize;
        w * h
    }

    /// Every chunk in the area, x-then-z — the order the stages walk.
    ///
    /// A named order rather than whatever a map iterator gives, because two
    /// runs that visit chunks differently must still produce the same world
    /// and the cheapest way to guarantee that is not to vary.
    pub fn chunks(self) -> impl Iterator<Item = (i32, i32)> {
        (self.min_chunk_x..=self.max_chunk_x)
            .flat_map(move |x| (self.min_chunk_z..=self.max_chunk_z).map(move |z| (x, z)))
    }
}

/// One stage of the pipeline, as a caller supplies it.
///
/// Each is a closure over one chunk rather than a trait object with a dozen
/// methods: the stages share nothing but the canvas, and the wiring each one
/// needs — noise parameters, a biome source, a rule tree, a feature registry
/// — belongs to whoever assembles them.
pub trait Stage {
    /// Run this stage for one chunk. The canvas already holds every chunk in
    /// the area, and earlier stages have run for all of them.
    fn run(&mut self, canvas: &mut Canvas, chunk_x: i32, chunk_z: i32);
}

impl<F: FnMut(&mut Canvas, i32, i32)> Stage for F {
    fn run(&mut self, canvas: &mut Canvas, chunk_x: i32, chunk_z: i32) {
        self(canvas, chunk_x, chunk_z);
    }
}

/// Which decoration model the generator runs.
///
/// Three arms in one binary, deliberately, because the comparison between
/// them is the measurement — `docs/DETERMINISM.md` §32 — and two builds
/// cannot be compared without also comparing their compilers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationModel {
    /// **The pyramid extent, and each chunk decorating against a world no
    /// other chunk has written into.** A chunk's feature writes become a
    /// pure function of the seed and its position, so a delivered chunk stops
    /// depending on how big the area around it was — the property
    /// `crates/rustmc-server/tests/tiled_world.rs` asks for, and the only
    /// model that answers it with zeroes.
    ///
    /// **It is not the default, and §32.3 is why: it is measurably further
    /// from vanilla.** 8,712 -> 42,898 mismatches against
    /// `oracle-work/van-s1-c62` on the seed-20260722 r=7 window at (64,64),
    /// and the End city window's published 100.000000% falls to 99.998861%.
    /// `RUSTMC_DECORATE_ISOLATED=1`.
    Isolated,
    /// **The pyramid extent, and decoration into the shared canvas.** The
    /// default. Decoration stops one [`isolation_reach`] inside the
    /// generated area, so no chunk decorates with a void beside it — which
    /// is what `ChunkStep`'s `addRequirement(CARVERS, 1)` says has to be
    /// true — and reads stay live, which is what vanilla's
    /// `WorldGenRegion.getChunk` actually hands back.
    ExtentOnly,
    /// The uniform [`DECORATION_MARGIN`] the tree ran until 2026-08-12: every
    /// generated chunk decorates, including the outermost ring, which has
    /// nothing beyond it. `RUSTMC_DECORATE_ISOLATED=0`, kept so §31's numbers
    /// can be re-taken in this binary.
    Shared,
}

/// Read the model off the environment.
#[must_use]
pub fn decoration_model() -> DecorationModel {
    match std::env::var("RUSTMC_DECORATE_ISOLATED").as_deref() {
        Ok("1") => DecorationModel::Isolated,
        Ok("0") => DecorationModel::Shared,
        _ => DecorationModel::ExtentOnly,
    }
}

/// Whether decoration runs isolated per chunk and is composed afterwards.
#[must_use]
pub fn isolated_decoration() -> bool {
    decoration_model() == DecorationModel::Isolated
}

/// How far outside its own chunk an isolated decoration is given the world.
///
/// A decorating chunk is handed a copy of the carved canvas out to this many
/// chunks and sees the void beyond it, exactly as a
/// [`Canvas::detach`]ed group does today. Vanilla's own bound is **one** —
/// `ChunkStep`'s `blockStateWriteRadius(1)` for `FEATURES`, and
/// `WorldGenRegion.warnIfReadOutsideWriteZone` logs a *read* past the same
/// ring — and this tree's parallel decoration pass has reproduced the
/// sequential run byte for byte at reaches 1, 2, 3, 4, 6 and 8
/// (`docs/BENCH.md`), so one is a measured bound and not only a declared
/// one.
///
/// It is [`DECORATE_REACH`] rather than 1 because the isolated pass is a new
/// claim on an old measurement: the reach sweep above was taken on the
/// shared-canvas model, where a group's neighbours already held their
/// features. `RUSTMC_ISOLATION_REACH` overrides it, and widening it must not
/// change a block — ORACLE rule 14.
///
/// [`Canvas::detach`]: crate::canvas::Canvas::detach
#[must_use]
pub fn isolation_reach() -> i32 {
    std::env::var("RUSTMC_ISOLATION_REACH")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(DECORATE_REACH)
}

/// Which chunks decorate and which chunks are generated at all, to deliver
/// `area`.
///
/// A free function rather than a private method because the two extents are
/// what a caller **growing** a world has to know: after a pass that delivered
/// `area`, `decorated` is the set that must never decorate again and
/// `generated` is the set that must never have a terrain stage run on it
/// again. A caller that derived either of them for itself would be a second
/// copy of the pyramid arithmetic, and the failure of the copy would look like
/// a generator defect.
///
/// Returns `(decorated, generated)`.
#[must_use]
pub fn extents_for(area: Area) -> (Area, Area) {
    // The three models differ in *which* chunks decorate, not in the stages.
    // Under the pyramid a delivered chunk needs its own ring decorated —
    // `FEATURES` at accumulated radius 1 for a chunk reaching `FULL` — and
    // each of those needs carved terrain around it, which is where the outer
    // ring comes from. Under the old model every generated chunk decorated,
    // including the outermost ring, which had nothing beyond it:
    // `docs/DETERMINISM.md` §31.3.
    match decoration_model() {
        DecorationModel::Shared => {
            let generated = area.grown(DECORATION_MARGIN);
            (generated, generated)
        }
        // The pyramid's own two radii, and nothing else: a chunk reaching
        // `FULL` needs `FEATURES` at 1 and `CARVERS` at 2, so the terrain
        // extent is exactly what `DECORATION_MARGIN` generated and one ring
        // fewer of it decorates.
        DecorationModel::ExtentOnly => {
            let step =
                crate::status::generation_pyramid().step_to(crate::status::ChunkStatus::Full);
            (
                area.grown(step.layer_radius(crate::status::ChunkStatus::Features)),
                area.grown(step.layer_radius(crate::status::ChunkStatus::Carvers)),
            )
        }
        // An isolated decoration is only a pure function of its position if
        // its group is *complete*, so the terrain has to reach one isolation
        // reach past the last decorating chunk rather than stopping where the
        // pyramid says a live-read decoration could stop. That is the extra
        // ring the isolated arm pays for.
        DecorationModel::Isolated => {
            let step =
                crate::status::generation_pyramid().step_to(crate::status::ChunkStatus::Full);
            let decorated = area.grown(step.layer_radius(crate::status::ChunkStatus::Features));
            (decorated, decorated.grown(isolation_reach()))
        }
    }
}

/// A canvas the generator has already filled, and the statuses reached on it.
///
/// # What this is for, and it is not a cache
///
/// `docs/DETERMINISM.md` §32.4 retired extent-independence: a delivered chunk
/// cannot be a pure function of the seed and its position while a feature
/// reads a neighbour's features, and vanilla's features read them. §32.6
/// states the property that survives — **a chunk, once delivered, is never
/// written again** — and this type is what makes it reachable. The world holds
/// **one canvas**; growing the delivered square hands that canvas back to the
/// generator with the statuses already on it, so every chunk is upgraded at
/// most once and no stage ever writes a chunk twice.
///
/// The two sets are the status map, projected onto the two statuses this
/// crate's stages actually gate on:
///
/// * `carved` — at `CARVERS` or later. No terrain stage may run on one again;
///   re-running the noise fill over a decorated chunk would overwrite its
///   features with fresh stone.
/// * `decorated` — at `FEATURES`. It must never decorate again.
///
/// Sets rather than an [`Area`] because the growth a server actually does is
/// not concentric: a player walks east and the world grows east. A concentric
/// ladder is what the closing measurement uses, and it is a special case.
///
/// **Measured, §33.** Over four seed/centre ladders grown 0 -> 1 -> 2, this
/// route moves **0** blocks in every already-delivered chunk on all eight
/// rungs, where regenerating the bigger square moves **4,059** on the same
/// eight; the same growth moves 50,378 in the ring one chunk further out,
/// which is what says the zero is the delivery rule rather than a generator
/// that stopped generating. It also carves each chunk once — 81 chunks to
/// deliver radius 2, against 155.
pub struct Resumed {
    /// The one canvas, exactly as the previous pass left it.
    pub canvas: Canvas,
    /// Every chunk already at `CARVERS` or later.
    pub carved: std::collections::BTreeSet<(i32, i32)>,
    /// Every chunk already at `FEATURES`.
    pub decorated: std::collections::BTreeSet<(i32, i32)>,
}

/// The composed generator.
pub struct Pipeline {
    area: Area,
    decorated: Area,
    generated: Area,
    canvas: Canvas,
    /// Chunks whose terrain a previous pass already ran — see [`Resumed`].
    /// Empty for a pipeline that starts from nothing, which is every caller
    /// that is not growing a world.
    settled_terrain: std::collections::BTreeSet<(i32, i32)>,
    /// Chunks a previous pass already decorated.
    settled_decoration: std::collections::BTreeSet<(i32, i32)>,
}

impl Pipeline {
    /// The two extents the shipped model derives from the pyramid — see
    /// [`extents_for`], which is where they are computed.
    fn extents(area: Area) -> (Area, Area) {
        extents_for(area)
    }

    /// A pipeline that will produce `area` correctly, generating a margin
    /// around it.
    #[must_use]
    pub fn new(area: Area, min_y: i32, height: i32, fill: &str) -> Self {
        let (decorated, generated) = Self::extents(area);
        let mut canvas = Canvas::new(min_y, height);
        // `RUSTMC_RECORD_BLOCK_ENTITIES=1` switches on the block-entity side
        // channel — `docs/DETERMINISM.md` §42. Off by default, per the
        // standing rule that new behaviour lands opt-in until it is measured,
        // and off by default for a second reason: the gate on this channel is
        // that the same window generates byte-identically in its block states
        // with recording on and off, so both settings have to be reachable
        // from one binary. `ci/be-channel-identical.sh`.
        if std::env::var("RUSTMC_RECORD_BLOCK_ENTITIES").as_deref() == Ok("1") {
            canvas.record_block_entities();
        }
        for (x, z) in generated.chunks() {
            canvas.ensure_chunk(x, z, fill);
        }
        Self {
            area,
            decorated,
            generated,
            canvas,
            settled_terrain: std::collections::BTreeSet::new(),
            settled_decoration: std::collections::BTreeSet::new(),
        }
    }

    /// A pipeline that will produce `area` correctly **onto a canvas that is
    /// already partly generated**.
    ///
    /// The chunks the resumed canvas already holds keep every block they hold
    /// — [`Canvas::ensure_chunk`] is idempotent — and the statuses in
    /// [`Resumed`] are what stops a stage running on one of them a second
    /// time. What is left is exactly the chunks this square adds.
    ///
    /// **The post-processing marks are dropped, and that is not tidiness.**
    /// A mark is a position `stage::post_process::run` has to revisit once
    /// the world around it is finished, and the previous pass ran it over
    /// every mark it had. Carrying them forward would re-run fluid ticks and
    /// shape updates inside chunks that are already delivered, which is the
    /// one thing this whole construction exists to prevent.
    #[must_use]
    pub fn resuming(area: Area, min_y: i32, height: i32, fill: &str, resume: Resumed) -> Self {
        let (decorated, generated) = Self::extents(area);
        let Resumed {
            mut canvas,
            carved,
            decorated: already_decorated,
        } = resume;
        // The vertical extent is the canvas's, not this call's, and the two
        // have to be the same canvas. A resumed pass whose `min_y` disagreed
        // with the canvas it was handed would index every column of every
        // inherited chunk at an offset — silently, because `Canvas::index`
        // answers a shifted cell rather than refusing one.
        assert_eq!(
            (canvas.min_y(), canvas.height()),
            (min_y, height),
            "a resumed pass must have the vertical extent of the canvas it resumes"
        );
        let _ = canvas.take_post_processing();
        for (x, z) in generated.chunks() {
            canvas.ensure_chunk(x, z, fill);
        }
        Self {
            area,
            decorated,
            generated,
            canvas,
            settled_terrain: carved,
            settled_decoration: already_decorated,
        }
    }

    /// Whether a terrain stage may run on this chunk — false once it is
    /// carved, whatever square is being generated now.
    #[must_use]
    pub fn needs_terrain(&self, chunk_x: i32, chunk_z: i32) -> bool {
        !self.settled_terrain.contains(&(chunk_x, chunk_z))
    }

    /// Whether this chunk may decorate — false once it has.
    #[must_use]
    pub fn needs_decoration(&self, chunk_x: i32, chunk_z: i32) -> bool {
        !self.settled_decoration.contains(&(chunk_x, chunk_z))
    }

    /// The chunks a terrain stage skips, as a set a caller can hold while the
    /// canvas is borrowed mutably.
    ///
    /// Every terrain stage in `worldgen-assembly` takes `&mut Canvas` (or a
    /// `ChunkWriter` out of it) for the whole loop, so it cannot ask the
    /// pipeline anything while it runs. One clone at the top of the pass is
    /// cheaper than threading a lifetime through six stages.
    #[must_use]
    pub fn settled_terrain(&self) -> std::collections::BTreeSet<(i32, i32)> {
        self.settled_terrain.clone()
    }

    /// The chunks the decoration stage skips. See [`Self::settled_terrain`].
    #[must_use]
    pub fn settled_decoration(&self) -> std::collections::BTreeSet<(i32, i32)> {
        self.settled_decoration.clone()
    }

    /// The chunks that decorate.
    ///
    /// **Under the generate-once model this is strictly inside
    /// [`Self::generated`], and that containment is the fix.** Every chunk
    /// that decorates has the full carved world out to [`isolation_reach`]
    /// around it, whatever `area` was asked for, so its feature writes are a
    /// function of the seed and its position and of nothing else. A delivered
    /// chunk is then its own writes plus those of the eight chunks that can
    /// reach it — all of them inside this area — composed in the canonical
    /// chunk order. That is why a delivered chunk stops depending on how big
    /// the area around it was.
    ///
    /// Under `RUSTMC_DECORATE_ISOLATED=0` it is [`Self::generated`], which is
    /// the model §31 measured: the outermost decorated ring is the outermost
    /// generated ring, it decorates against a world that stops, and it writes
    /// that inward.
    #[must_use]
    pub const fn decorated(&self) -> Area {
        self.decorated
    }

    /// The area the caller asked for — the part that is correct.
    #[must_use]
    pub const fn inner(&self) -> Area {
        self.area
    }

    /// The area actually generated, margin included.
    #[must_use]
    pub const fn generated(&self) -> Area {
        self.generated
    }

    #[must_use]
    pub const fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// The finished canvas, consuming the pipeline.
    #[must_use]
    pub fn into_canvas(self) -> Canvas {
        self.canvas
    }

    #[must_use]
    pub const fn canvas_mut(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// Run one stage over every generated chunk that has not already had it,
    /// in the named order.
    pub fn run_stage<S: Stage>(&mut self, stage: &mut S) {
        for (x, z) in self.generated.chunks() {
            if !self.needs_terrain(x, z) {
                continue;
            }
            stage.run(&mut self.canvas, x, z);
        }
    }

    /// Prime the four live heightmaps for every chunk, which is what
    /// `generateFeatures` does on the line before the first feature.
    ///
    /// A resumed pass primes only the chunks it added. The chunks it inherited
    /// were primed by the pass that carved them — before any decoration ran,
    /// which is where vanilla primes them too — and re-priming a chunk that
    /// has since decorated would replace a heightmap that has its features in
    /// it with one taken off the carved terrain.
    pub fn prime_heightmaps<P: HeightmapPredicates + ?Sized>(&mut self, props: &P) {
        for (x, z) in self.generated.chunks() {
            if !self.needs_terrain(x, z) {
                continue;
            }
            self.canvas.prime_heightmaps(x, z, props);
        }
    }
}

/// How many threads a **chunk-local** stage may use.
///
/// `RUSTMC_WORKERS=<n>` overrides it; `1` forces the sequential path, which
/// is what the byte-for-byte comparison between one worker and many is run
/// against. Unset means one thread per **physical core** — not per hardware
/// thread. See [`physical_cores`] for the measurement that decides that.
///
/// This is only ever a division of work. A stage is eligible for it when it
/// writes into its own chunk and nothing else — see [`Canvas::chunk_writers`]
/// for the property and `stage::noise::fill_chunk_into` for the first stage
/// that had it. Dividing work cannot change a generated block; a stage that
/// is not chunk-local must not be run this way, and the compiler checks the
/// half of that it can see, since a `ChunkWriter` cannot reach another
/// chunk's blocks at all. The half it cannot see — that the stage's *reads*
/// are chunk-local too — is what each stage's own byte-identical gate is
/// for.
///
/// [`Canvas::chunk_writers`]: crate::canvas::Canvas::chunk_writers
#[must_use]
pub fn worker_threads() -> usize {
    static COUNT: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *COUNT.get_or_init(|| {
        std::env::var("RUSTMC_WORKERS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|n| *n > 0)
            .unwrap_or_else(|| {
                let logical =
                    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
                physical_cores().map_or(logical, |cores| cores.clamp(1, logical))
            })
    })
}

/// How many physical cores this machine has, from the kernel's own topology.
///
/// # Why this exists rather than `available_parallelism`
///
/// `available_parallelism` counts **hardware threads**, and on an SMT machine
/// filling them is a bad trade for this workload. Measured on the bench box —
/// a 12-core/24-thread Threadripper 1920X, r=16 square at (64,64), seed
/// 20260722, 1089 delivered chunks, identical build, `perf stat`:
///
/// ```text
///   threads   wall     CPU-s   instructions   cycles     IPC
///    1        78.99 s  78.54   808.786 G      322.42 G   2.51
///   12 (phys) 60.08 s  82.69   810.152 G      325.32 G   2.49
///   24 (SMT)  59.72 s  98.48   810.428 G      383.37 G   2.11
/// ```
///
/// The instruction count is the same to within 0.2% at every width, so
/// parallelism does no extra **work**. What SMT costs is *cycles*: two
/// threads sharing one core's execution units retire fewer instructions each
/// (IPC 2.51 -> 2.11), and the kernel bills both threads' seconds, so the
/// same 810 G instructions arrive as 383 G cycles instead of 325 G. Twelve
/// threads on twelve distinct cores cost **+0.9% cycles**; twenty-four cost
/// **+18.9%**, and buy 0.6% of wall time for it. On a metric that is CPU-s
/// per chunk, that is a straight loss.
///
/// Returns `None` off Linux or if `sysfs` is not readable, and the caller
/// then falls back to the hardware-thread count.
#[must_use]
pub fn physical_cores() -> Option<usize> {
    // One entry per distinct sibling *set*: two hardware threads of one core
    // report the same `thread_siblings_list`, so the number of distinct lists
    // is the number of cores. Reading the list rather than `core_id` avoids
    // having to pair it with `physical_package_id` on a multi-socket box.
    let mut groups = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir("/sys/devices/system/cpu").ok()?.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(digits) = name.strip_prefix("cpu") else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let path = entry.path().join("topology/thread_siblings_list");
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        groups.insert(text.trim().to_owned());
    }
    (!groups.is_empty()).then_some(groups.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The core count has to be usable as a thread count, which means at
    /// least one and never more hardware threads than exist. A machine that
    /// reported more cores than threads would silently oversubscribe every
    /// parallel stage, which is the exact mistake this function was written
    /// to stop.
    #[test]
    fn physical_cores_are_at_least_one_and_no_more_than_the_hardware_threads() {
        let logical = std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
        if let Some(cores) = physical_cores() {
            assert!(cores >= 1, "a machine with no cores is running this test");
            assert!(
                cores <= logical,
                "{cores} physical cores against {logical} hardware threads"
            );
        }
        assert!(worker_threads() >= 1, "a stage needs at least one worker");
    }

    /// The extent under the pyramid, spelled out rather than derived: a test
    /// that recomputes the code's own formula agrees with the code however
    /// wrong they both are.
    ///
    /// The containment in the last assertion is the load-bearing one. Under
    /// the shared-canvas model the decorated area *was* the generated area,
    /// and `docs/DETERMINISM.md` §31.3 is what that cost; the whole of the
    /// generate-once model is that the outermost generated ring stands there
    /// to be read and never decorates.
    #[test]
    fn decoration_stops_one_reach_inside_the_generated_area() {
        assert_eq!(
            decoration_model(),
            DecorationModel::ExtentOnly,
            "these numbers are for the shipped model"
        );
        assert_eq!(isolation_reach(), 2, "and for an isolation reach of two");
        let area = Area::square(0, 0, 1);
        let pipeline = Pipeline::new(area, -64, 384, "minecraft:air");
        assert_eq!(pipeline.inner().chunk_count(), 9);
        assert_eq!(
            pipeline.decorated().chunk_count(),
            25,
            "3x3 grown by the FEATURES layer radius of a chunk reaching FULL"
        );
        assert_eq!(
            pipeline.generated().chunk_count(),
            49,
            "3x3 grown by the CARVERS layer radius — the same terrain \
             DECORATION_MARGIN generated"
        );
        assert_eq!(pipeline.canvas().chunk_count(), 49);
        assert!(pipeline.canvas().has_chunk(-3, -3));
        assert!(!pipeline.canvas().has_chunk(-4, -3));
        // Every decorating chunk has its own carved ring, which is the whole
        // of `addRequirement(CARVERS, 1)` and the thing the uniform margin
        // could not say: its outermost decorating ring had no ring at all,
        // and vanilla's `WorldGenRegion.getChunk` throws
        // `Requested chunk unavailable during world generation` rather than
        // decorate there.
        for (x, z) in pipeline.decorated().chunks() {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    assert!(
                        pipeline.canvas().has_chunk(x + dx, z + dz),
                        "chunk ({x},{z}) decorates against a hole at ({},{})",
                        x + dx,
                        z + dz
                    );
                }
            }
        }
    }

    /// The model §31 measured, still reachable in the same binary so the
    /// before/after is one build. Its extent is the old one exactly.
    #[test]
    fn the_shared_canvas_model_still_generates_what_it_always_did() {
        // Serialised against nothing: the env var is read per `Pipeline`,
        // and this test sets it around one construction. Rust runs tests in
        // threads, so this is deliberately not asserting the *global* state
        // afterwards — only that the branch taken with the var set produces
        // the old extent.
        let area = Area::square(0, 0, 1);
        let margin = DECORATION_MARGIN;
        let generated = area.grown(margin);
        assert_eq!(generated.chunk_count(), 49, "3x3 grown by two");
        assert_eq!(margin, 2, "these numbers are for a margin of two");
    }

    /// The visiting order is x-then-z and it is a promise, not an accident.
    #[test]
    fn chunks_are_visited_in_a_named_order() {
        let order: Vec<(i32, i32)> = Area::square(0, 0, 1).chunks().collect();
        assert_eq!(order.first(), Some(&(-1, -1)));
        assert_eq!(order[1], (-1, 0));
        assert_eq!(order[3], (0, -1), "x advances only after z is exhausted");
        assert_eq!(order.last(), Some(&(1, 1)));
        assert_eq!(order.len(), 9);
    }

    /// A stage runs for every generated chunk, margin included — the margin
    /// exists precisely so it is decorated too.
    #[test]
    fn a_stage_runs_over_the_margin_as_well() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        let mut seen: Vec<(i32, i32)> = Vec::new();
        pipeline.run_stage(&mut |_: &mut Canvas, x: i32, z: i32| seen.push((x, z)));
        assert_eq!(
            seen.len(),
            25,
            "one chunk, the ring that decorates around it, and that ring's \
             own carved ring"
        );
        assert!(seen.contains(&(0, 0)));
        assert!(seen.contains(&(-2, -2)));
        // And it runs over chunks that will never decorate — that is the
        // point of them. The number is the same 25 the uniform margin
        // generated; what changed is that the outer ring of it is terrain
        // for the decorating ring to stand on rather than a ring that
        // decorates against nothing.
        assert!(!pipeline.decorated().contains(-2, -2));
        assert!(pipeline.decorated().contains(-1, -1));
    }

    /// A stage writing across a chunk border reaches the neighbour, which is
    /// the whole reason the canvas is shared.
    #[test]
    fn a_stage_can_write_into_its_neighbour() {
        let mut pipeline = Pipeline::new(Area::square(0, 0, 0), -64, 384, "minecraft:air");
        pipeline.run_stage(&mut |canvas: &mut Canvas, x: i32, z: i32| {
            if (x, z) == (0, 0) {
                // One block west of this chunk's origin, i.e. in chunk -1.
                canvas.set_block(-1, 70, 0, "minecraft:stone");
            }
        });
        assert_eq!(pipeline.canvas().block_at(-1, 70, 0), "minecraft:stone");
    }

    #[test]
    fn an_area_grows_symmetrically() {
        let area = Area::square(5, -3, 2).grown(1);
        assert_eq!(area.min_chunk_x, 2);
        assert_eq!(area.max_chunk_x, 8);
        assert_eq!(area.min_chunk_z, -6);
        assert_eq!(area.max_chunk_z, 0);
        assert!(area.contains(2, -6));
        assert!(!area.contains(1, -6));
    }

    /// The stage ordering is a total order, so `Through::Noise < Through::
    /// Decoration` reads the way a caller expects when gating stages.
    #[test]
    fn the_stages_are_ordered() {
        assert!(Through::Noise < Through::Decoration);
        assert!(Through::Carvers < Through::Decoration);
        assert!(Through::Surface > Through::Biomes);
    }
}
